//! Bounded ordinary-history windows over one captured, incremental traversal.
use crate::*;

pub const PAGE_SIZE: usize = 500;
const MAX_WINDOW_ROWS: usize = 5_000;
const MAX_WINDOW_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct State {
    pub scope: Option<gitturtle_core::HistoryScope>,
    pub offset: usize,
    pub next_offset: Option<usize>,
    /// The scope of the page read under way where it is not `scope`, as a
    /// restore's saved scope is: the window takes it with that page's rows
    /// as they land (`receive_history_page`), so a page refused or failed
    /// leaves paging as it was. Pinned as the page was asked for, a scope
    /// whose page never came stayed under the snapshot's rows, and Older
    /// then continued it there. Every other end of that read clears it: a
    /// read that replaces it, fails or is dropped (`GitTurtle::request`),
    /// and a search or another tab that takes paging over meanwhile
    /// ([`Self::take_settled`]).
    reading: Option<gitturtle_core::HistoryScope>,
}

impl State {
    pub fn from_snapshot(snapshot: &worker::Snapshot) -> Self {
        Self {
            scope: Some(snapshot.history_scope.clone()),
            offset: snapshot.history_offset,
            next_offset: snapshot.history_next_offset,
            reading: None,
        }
    }

    /// Ends the page read under way's claim on the window, as that read
    /// ends without its rows.
    pub fn end_read(&mut self) {
        self.reading = None;
    }

    /// Paging as a search or a tab left takes it over, without the page read
    /// under way, whose reply then never lands.
    pub fn take_settled(&mut self) -> Self {
        let mut taken = std::mem::take(self);
        taken.end_read();
        taken
    }

    pub fn is_deep(&self, rows: usize) -> bool {
        self.offset > 0 || rows > PAGE_SIZE
    }
}

/// One retained inspector commit is outside the ordinary virtualized window;
/// it is not a hidden row in that window's continuous ancestry. Search results
/// remain discontinuous even when all of their returned matches are visible.
pub(super) fn graph_is_filtered(
    commit_count: usize,
    visible_count: usize,
    retained_commit: Option<usize>,
    search_active: bool,
) -> bool {
    let retained = usize::from(retained_commit.is_some_and(|index| index < commit_count));
    search_active || visible_count != commit_count - retained
}

fn fits_window(
    current_rows: usize,
    current_bytes: usize,
    page_rows: usize,
    page_bytes: usize,
) -> bool {
    current_rows.saturating_add(page_rows) <= MAX_WINDOW_ROWS
        && current_bytes.saturating_add(page_bytes) <= MAX_WINDOW_BYTES
}

fn retained_bytes(commits: &[Commit], graph: &[graph::GraphRow]) -> usize {
    commits.iter().map(Commit::history_bytes).sum::<usize>()
        + graph
            .iter()
            .map(|row| {
                std::mem::size_of::<graph::GraphRow>()
                    + row.edges.len() * std::mem::size_of::<graph::Edge>()
            })
            .sum::<usize>()
}

impl GitTurtle {
    /// Reads the ordinary-history page at `offset` in the window's scope.
    /// Returns whether it asked for it: refused while a read, a search or a
    /// write runs, or with no repository or captured scope.
    pub(super) fn request_history_page(
        &mut self,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.read_history_page(None, offset, window, cx)
    }

    /// Reads the page at `offset` in `scope`, a restore's saved scope, which
    /// the window takes only with the page's rows: refused as
    /// [`Self::request_history_page`] is, or failed, the page leaves paging
    /// on the scope the window shows, and a cold tab's restore then ends
    /// (`finish_tab_snapshot`).
    pub(super) fn request_history_page_in(
        &mut self,
        scope: gitturtle_core::HistoryScope,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.read_history_page(Some(scope), offset, window, cx)
    }

    fn read_history_page(
        &mut self,
        reading: Option<gitturtle_core::HistoryScope>,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.loading.is_some() || self.history_search_active() || self.operation_busy.is_some() {
            return false;
        }
        let Some(repo) = self.repository.clone() else {
            return false;
        };
        let Some(scope) = reading
            .clone()
            .or_else(|| self.history_paging.scope.clone())
        else {
            return false;
        };
        self.interaction_started = Some(Instant::now());
        self.request(
            Job::HistoryPage {
                repo,
                scope,
                offset,
                limit: PAGE_SIZE,
            },
            "Reading older history…",
            window,
            cx,
        );
        // After `request`, which ends any earlier read's.
        self.history_paging.reading = reading;
        true
    }

    pub(super) fn receive_history_page(
        &mut self,
        result: worker::HistoryPageResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reading = self.history_paging.reading.take();
        if self.history_search_active() {
            // A search the person started meanwhile ends a cold tab's restore
            // waiting for this page, as a read that replaces it does, and
            // History's search asked for before it goes with it
            // (`end_replaced_restore`). Defensive: `history_query_changed`
            // ends the restore before it sets the search's progress, and
            // replies are generation-checked, so no read reaches this.
            if self.repository_tabs.restoring.take().is_some() {
                self.repository_tabs.drop_pending_search();
            }
            return;
        }
        let worker::HistoryPageResult {
            page,
            graph,
            graph_notice,
            elapsed,
        } = result;
        if page.commits.is_empty() {
            let end = page.offset;
            if reading.is_none() && Some(end) == self.history_paging.next_offset {
                self.history_paging.next_offset = None;
                self.status = "Reached the end of this captured history".into();
                self.end_restore_in_history(window, cx);
                return;
            }
            // The page asked for lies past its history's end, `end`, as a
            // restored tab's saved page does where a fetch at a shallower
            // depth cut that history since: the last page's worth of rows up
            // to the end shows in its place, in the same scope, and a
            // restore goes on from it to its saved selection. Shown as it
            // came, the empty page emptied History's window.
            if end > 0 && self.read_history_page(reading, end.saturating_sub(PAGE_SIZE), window, cx)
            {
                return;
            }
            // A scope with no commits, or that page refused: paging stays as
            // it was, and a restore ends in History. Left set, a restore
            // waited for a page that never came, and held History's search
            // asked for meanwhile.
            self.end_restore_in_history(window, cx);
            return;
        }
        // A page in another scope, as a restore's saved one, replaces the
        // window whatever its offset: appended where its offset continued the
        // snapshot's rows, it joined two histories, skipping or repeating
        // commits at the seam, and Older went on in the saved scope.
        let same_scope = reading
            .as_ref()
            .is_none_or(|scope| self.history_paging.scope.as_ref() == Some(scope));
        if let Some(scope) = reading {
            self.history_paging.scope = Some(scope);
        }
        let selected = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
            .cloned();
        if let Some(index) = self.automatic.retained_commit.take()
            && index < self.commits.len()
        {
            self.commits.remove(index);
            if index < self.graph.len() {
                self.graph.remove(index);
            }
        }
        let append = same_scope
            && page.offset
                == self
                    .history_paging
                    .offset
                    .saturating_add(self.commits.len())
            && fits_window(
                self.commits.len(),
                retained_bytes(&self.commits, &self.graph),
                page.commits.len(),
                retained_bytes(&page.commits, &graph),
            );
        if append {
            self.commits.extend(page.commits);
            self.graph.extend(graph);
        } else {
            self.commits = page.commits;
            self.graph = graph;
            self.history_paging.offset = page.offset;
        }
        self.history_paging.next_offset = page.next_offset;
        self.graph_notice = graph_notice;
        if self.graph_notice.is_some() {
            for row in &mut self.graph {
                *row = graph::GraphRow {
                    width: 1,
                    ..Default::default()
                };
            }
        }
        self.graph_lanes = self.graph.iter().map(|row| row.width).max().unwrap_or(1);
        self.selected_commit = selected.as_ref().and_then(|selected| {
            self.commits
                .iter()
                .position(|commit| commit.oid == selected.oid)
        });
        self.trace_frame("history_page_frame_ms", window, cx);
        if self.repository_tabs.restoring.is_some() {
            self.restore_tab_selection(window, cx);
            return;
        }
        if self.selected_commit.is_none()
            && let Some(selected) = selected
        {
            // Inspector/Compare identity survives window eviction. One selected
            // commit remains outside the virtualized list, as on quiet refresh.
            self.selected_commit = Some(self.commits.len());
            self.automatic.retained_commit = self.selected_commit;
            self.commits.push(selected);
            self.graph.push(graph::GraphRow::default());
        }
        self.filter_history_retaining_scroll(cx);
        if !append {
            self.history_scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        self.status = format!(
            "History rows {}–{} · {:.1} ms · captured snapshot{}",
            self.history_paging.offset + usize::from(!self.visible.is_empty()),
            self.history_paging.offset + self.visible.len(),
            elapsed.as_secs_f64() * 1000.,
            if self.automatic.retained_commit.is_some() {
                " · selected comparison retained"
            } else {
                ""
            }
        );
        if self.selected_commit.is_none()
            && let Some(index) = self.visible.first().copied()
        {
            self.select_commit(index, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn deep_window_retains_ancestry_edges_with_selection_outside_visible_rows() {
        let commits = (0..6001)
            .map(|index| Commit {
                oid: format!("commit-{index}"),
                parents: if index < 6000 {
                    vec![format!("commit-{}", index + 1)]
                } else {
                    vec![]
                },
                author: String::new(),
                timestamp: 0,
                subject: String::new(),
                body: String::new(),
            })
            .collect::<Vec<_>>();
        let selected = commits[0].clone();
        let mut cursor = graph::GraphCursor::default();
        let mut visible_commits = Vec::new();
        let mut rows = Vec::new();
        let mut offset = 0;
        for (page, chunk) in commits[..6000].chunks(PAGE_SIZE).enumerate() {
            let (prepared, hidden) = cursor
                .append(chunk, 128, 1_000_000, || Ok::<_, ()>(()))
                .unwrap();
            assert!(!hidden);
            if !fits_window(
                visible_commits.len(),
                retained_bytes(&visible_commits, &rows),
                chunk.len(),
                retained_bytes(chunk, &prepared),
            ) {
                visible_commits.clear();
                rows.clear();
                offset = page * PAGE_SIZE;
            }
            visible_commits.extend_from_slice(chunk);
            rows.extend(prepared);
        }
        assert_eq!(offset, 5000);
        assert_eq!(visible_commits.len(), 1000);
        assert!(rows.iter().all(|row| row.incoming && !row.edges.is_empty()));
        let visible_count = visible_commits.len();
        let retained_commit = Some(visible_count);
        visible_commits.push(selected);
        rows.push(graph::GraphRow::default());
        assert!(!graph_is_filtered(
            visible_commits.len(),
            visible_count,
            retained_commit,
            false
        ));
        assert!(graph_is_filtered(
            visible_commits.len(),
            visible_count - 1,
            retained_commit,
            false
        ));
        assert_eq!(
            rows[..visible_count]
                .iter()
                .map(|row| row.edges.len())
                .sum::<usize>(),
            1000
        );
    }

    #[::core::prelude::v1::test]
    fn search_and_other_missing_rows_remain_discontinuous() {
        assert!(graph_is_filtered(500, 500, None, true));
        assert!(graph_is_filtered(501, 500, Some(500), true));
        assert!(graph_is_filtered(501, 499, Some(500), false));
        assert!(graph_is_filtered(501, 500, Some(999), false));
        assert!(!graph_is_filtered(500, 500, None, false));
        assert!(!graph_is_filtered(0, 0, Some(0), false));
    }

    #[::core::prelude::v1::test]
    fn window_can_continue_at_arbitrary_depth_and_bounds_rows_and_bytes_independently() {
        assert!(fits_window(4500, 1024, 500, 1024));
        assert!(!fits_window(5000, 1024, 1, 1024));
        assert!(!fits_window(1, MAX_WINDOW_BYTES - 1, 1, 2));
        let deep = State {
            offset: 100_000,
            next_offset: Some(100_500),
            ..State::default()
        };
        assert!(deep.is_deep(500));
        assert_eq!(deep.next_offset, Some(100_500));
    }
}
