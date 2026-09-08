//! Repository-wide search shares the replaceable reader with previews.
use crate::*;
use gitturtle_core::{HistoryScope, HistorySearchStop};
use std::time::Duration;

const SEARCH_LABEL: &str = "Searching repository history…";
const MAX_RESULTS: usize = 10_000;
const MAX_RESULT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub struct State {
    normal: Option<Retained>,
    progress: Option<Progress>,
    task: Option<Task<()>>,
    debounce: Option<Task<()>>,
    epoch: u64,
}

struct Retained {
    commits: Vec<Commit>,
    visible: Vec<usize>,
    graph: Vec<graph::GraphRow>,
    graph_notice: Option<String>,
    selected_commit: Option<usize>,
    files: Vec<FileChange>,
    selected_file: Option<usize>,
    parent: usize,
    preferred_file: Option<PathBuf>,
    scroll: UniformListScrollHandle,
    retained_commit: Option<usize>,
}

#[derive(Default)]
struct Progress {
    query: String,
    scope_label: String,
    pinned: Option<HistoryScope>,
    next_offset: usize,
    scanned: usize,
    stop: Option<HistorySearchStop>,
    paused: bool,
    error: Option<String>,
    retained_bytes: usize,
}

impl Progress {
    fn exhausted(&self) -> bool {
        self.stop == Some(HistorySearchStop::Exhausted)
    }
    fn accept(&mut self, page: &gitturtle_core::HistorySearchPage, bytes: usize) {
        self.pinned = Some(page.scope.clone());
        self.scanned += page.scanned;
        self.next_offset = page.next_offset.unwrap_or(self.next_offset + page.scanned);
        self.stop = Some(page.stop);
        self.paused = false;
        self.retained_bytes += bytes;
    }
}

impl GitTurtle {
    pub(super) fn history_search_active(&self) -> bool {
        self.history_search.progress.is_some()
    }
    pub(super) fn history_search_pending(&self) -> bool {
        self.history_search.task.is_some() || self.history_search.debounce.is_some()
    }

    /// Every foreground read may supersede a search. Keep its immutable results
    /// and cursor so selecting a match never silently restarts the search.
    pub(super) fn pause_history_search_for_read(&mut self) {
        if !self.history_search_pending() {
            return;
        }
        self.history_search.epoch = self.history_search.epoch.wrapping_add(1);
        self.history_search.task = None;
        self.history_search.debounce = None;
        if let Some(progress) = &mut self.history_search.progress {
            progress.paused = true;
        }
        self.worker.cancel();
        if self.loading == Some(SEARCH_LABEL) {
            self.loading = None;
        }
    }

    pub(super) fn discard_history_search(&mut self) {
        self.pause_history_search_for_read();
        let epoch = self.history_search.epoch.wrapping_add(1);
        self.history_search = State {
            epoch,
            ..Default::default()
        };
    }

    pub(super) fn history_query_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.search.read(cx).value();
        if self
            .history_search
            .progress
            .as_ref()
            .is_some_and(|state| state.query == value.as_ref())
        {
            return;
        }
        self.close_file_history(window, cx);
        self.pause_history_search_for_read();
        if value.is_empty() {
            if !self.history_search_active() && self.history_search.normal.is_none() {
                return;
            }
            self.restore_normal_history(window, cx);
            cx.notify();
            return;
        }
        if self.repository.is_none() || self.loading == Some("Reading local history…") {
            return;
        }
        self.cancel_automatic_read();
        self.generation = self.generation.wrapping_add(1);
        self.worker.cancel();
        self.task = None;
        self.loading = None;
        self.error = None;
        if self.history_search.normal.is_none() {
            self.history_search.normal = Some(Retained {
                commits: std::mem::take(&mut self.commits),
                visible: std::mem::take(&mut self.visible),
                graph: std::mem::take(&mut self.graph),
                graph_notice: self.graph_notice.take(),
                selected_commit: self.selected_commit.take(),
                files: std::mem::take(&mut self.files),
                selected_file: self.selected_file.take(),
                parent: self.parent,
                preferred_file: self.preferred_file.clone(),
                scroll: std::mem::take(&mut self.history_scroll),
                retained_commit: self.automatic.retained_commit.take(),
            });
        }
        self.commits.clear();
        self.visible.clear();
        self.graph.clear();
        self.graph_notice = None;
        self.selected_commit = None;
        self.selected_file = None;
        self.files.clear();
        self.clear_preview();
        self.parent = 0;
        self.history_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.history_search.epoch = self.history_search.epoch.wrapping_add(1);
        self.history_search.progress = Some(Progress {
            query: value.to_string(),
            scope_label: self
                .scope
                .as_ref()
                .map(|scope| scope.0.clone())
                .unwrap_or_else(|| "All local refs".into()),
            error: (value.len() > 4096).then(|| {
                "Search query exceeds 4 KiB. Shorten it to search repository history.".into()
            }),
            ..Default::default()
        });
        if value.len() > 4096 {
            cx.notify();
            return;
        }
        let epoch = self.history_search.epoch;
        let timer = cx.background_executor().timer(Duration::from_millis(250));
        self.history_search.debounce = Some(cx.spawn_in(window, async move |this, cx| {
            timer.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.history_search.epoch != epoch {
                    return;
                }
                this.history_search.debounce = None;
                this.continue_history_search(window, cx);
            });
        }));
        cx.notify();
    }

    fn restore_normal_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_search.progress = None;
        let Some(normal) = self.history_search.normal.take() else {
            return;
        };
        self.commits = normal.commits;
        self.visible = normal.visible;
        self.graph = normal.graph;
        self.graph_notice = normal.graph_notice;
        self.graph_lanes = self.graph.iter().map(|row| row.width).max().unwrap_or(1);
        self.selected_commit = normal.selected_commit;
        self.files = normal.files;
        self.selected_file = normal.selected_file;
        self.parent = normal.parent;
        self.preferred_file = normal.preferred_file;
        self.history_scroll = normal.scroll;
        self.automatic.retained_commit = normal.retained_commit;
        self.clear_preview();
        self.error = None;
        if self.files.is_empty()
            && let Some(index) = self.selected_commit
        {
            self.select_commit(index, window, cx);
        }
        self.try_automatic_refresh(window, cx);
    }

    pub(super) fn continue_history_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_search_pending()
            || self.operation_busy.is_some()
            || self.commits.len() >= MAX_RESULTS
        {
            return;
        }
        let (Some(repo), Some(progress)) = (self.repository.clone(), &self.history_search.progress)
        else {
            return;
        };
        if progress.exhausted() || progress.query.len() > 4096 {
            return;
        }
        let job = Job::SearchHistory {
            repo,
            scope: self.scope.as_ref().map(|scope| scope.1.clone()),
            pinned: progress.pinned.clone(),
            query: progress.query.clone(),
            offset: progress.next_offset,
            limit: 500.min(MAX_RESULTS - self.commits.len()),
            previous: self
                .commits
                .iter()
                .map(|commit| (commit.oid.clone(), commit.parents.clone()))
                .collect(),
            remaining_bytes: MAX_RESULT_BYTES.saturating_sub(progress.retained_bytes),
        };
        self.cancel_automatic_read();
        self.generation = self.generation.wrapping_add(1);
        self.task = None;
        let generation = self.generation;
        let epoch = self.history_search.epoch;
        let path = self.path.clone();
        self.loading = Some(SEARCH_LABEL);
        if let Some(progress) = &mut self.history_search.progress {
            progress.error = None;
            progress.paused = false;
        }
        let response = self.worker.submit(job);
        self.history_search.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !search_reply_current(
                    this.history_search.epoch,
                    epoch,
                    this.generation,
                    generation,
                    &this.path,
                    &path,
                ) {
                    return;
                }
                this.history_search.task = None;
                this.loading = None;
                match result {
                    Ok(Ok(Output::SearchHistory(result))) => {
                        if let Some(progress) = &mut this.history_search.progress {
                            progress.accept(&result.page, result.retained_bytes);
                        }
                        let was_empty = this.commits.is_empty();
                        this.commits.extend(result.page.commits);
                        this.visible = (0..this.commits.len()).collect();
                        this.graph = result.graph;
                        this.graph_notice = result.graph_notice;
                        this.graph_lanes =
                            this.graph.iter().map(|row| row.width).max().unwrap_or(1);
                        if was_empty
                            && !this.commits.is_empty()
                            && this.mode == WorkspaceMode::History
                            && this.page == AppPage::Repository
                        {
                            this.select_commit(0, window, cx);
                        }
                    }
                    Ok(Err(error)) => {
                        if let Some(progress) = &mut this.history_search.progress {
                            progress.error = Some(format!("{error:#}"));
                        }
                    }
                    Err(_) | Ok(Ok(_)) => {
                        if let Some(progress) = &mut this.history_search.progress {
                            progress.paused = true;
                        }
                    }
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn restart_history_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(progress) = &mut self.history_search.progress {
            progress.query.clear();
        }
        self.history_query_changed(window, cx);
    }

    pub(super) fn history_search_empty(&self) -> Option<(&'static str, String)> {
        let progress = self.history_search.progress.as_ref()?;
        if let Some(error) = &progress.error {
            return Some(("Search could not complete", error.clone()));
        }
        if self.history_search_pending() {
            return Some((
                "Searching repository history…",
                format!(
                    "{} · literal message, author and hash search",
                    progress.scope_label
                ),
            ));
        }
        if progress.exhausted() {
            return Some((
                "No matching commits",
                format!(
                    "Searched {} commits in {}.",
                    progress.scanned, progress.scope_label
                ),
            ));
        }
        Some((
            if progress.paused {
                "Search paused"
            } else {
                "No matches in this part of history"
            },
            format!(
                "{} commits searched. Continue to inspect older history in {}.",
                progress.scanned, progress.scope_label
            ),
        ))
    }

    pub(super) fn render_history_search_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(progress) = &self.history_search.progress else {
            return div().into_any_element();
        };
        let colors = palette(cx);
        let running = self.history_search_pending();
        let detail = if let Some(error) = &progress.error {
            error.clone()
        } else if running {
            "Searching…".into()
        } else if self.commits.len() >= MAX_RESULTS {
            "10,000 matches shown. Narrow the query for more precise results.".into()
        } else if progress.paused {
            "Search paused · results retained".into()
        } else {
            progress
                .stop
                .map(HistorySearchStop::label)
                .unwrap_or("Ready to search")
                .into()
        };
        let scope = match &progress.pinned {
            Some(HistoryScope::FromCommit(oid)) => {
                format!("{} · pinned {}", progress.scope_label, short_oid(oid))
            }
            Some(HistoryScope::PinnedRefs(refs)) => {
                format!("{} · {} pinned tips", progress.scope_label, refs.len())
            }
            _ => progress.scope_label.clone(),
        };
        div()
            .px_3()
            .pb_2()
            .flex()
            .flex_col()
            .gap_1()
            .text_size(px(10.))
            .text_color(rgb(colors.muted))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(format!(
                        "{scope} · {} matches · {} commits searched",
                        self.commits.len(),
                        progress.scanned
                    )))
                    .child(
                        button("history-search-cancel", "Cancel", "", false)
                            .disabled(!running)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.pause_history_search_for_read();
                                this.try_automatic_refresh(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        button(
                            "history-search-continue",
                            if progress.error.is_some() {
                                "Retry"
                            } else {
                                "Continue"
                            },
                            "",
                            false,
                        )
                        .disabled(
                            running
                                || progress.exhausted()
                                || self.commits.len() >= MAX_RESULTS
                                || self.operation_busy.is_some(),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.continue_history_search(window, cx)
                        })),
                    )
                    .child(
                        button("history-search-restart", "Restart", "", false)
                            .disabled(self.operation_busy.is_some())
                            .tooltip(
                                "Restart this search at the current local branch and worktree tips",
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.restart_history_search(window, cx)
                            })),
                    ),
            )
            .child(detail)
            .into_any_element()
    }

    /// Quiet refresh updates navigation and the normal history beneath search;
    /// the visible search session keeps its immutable tips and exact cursor.
    pub(super) fn retain_search_snapshot(
        &mut self,
        snapshot: worker::Snapshot,
        cx: &mut Context<Self>,
    ) -> Option<worker::Snapshot> {
        let Some(normal) = &mut self.history_search.normal else {
            return Some(snapshot);
        };
        let offset = normal.scroll.0.borrow().base_handle.offset();
        let height = self.settings.density.history_row_height();
        let top = ((-f32::from(offset.y)) / height).max(0.) as usize;
        let anchor = normal
            .visible
            .get(top)
            .and_then(|index| normal.commits.get(*index))
            .map(|commit| commit.oid.clone());
        let selected = normal
            .selected_commit
            .and_then(|index| normal.commits.get(index))
            .cloned();
        normal.commits = snapshot.commits;
        normal.graph = snapshot.graph;
        normal.graph_notice = snapshot.graph_notice;
        normal.selected_commit = selected.as_ref().and_then(|old| {
            normal
                .commits
                .iter()
                .position(|commit| commit.oid == old.oid)
        });
        normal.retained_commit = None;
        if normal.selected_commit.is_none()
            && let Some(selected) = selected
        {
            normal.selected_commit = Some(normal.commits.len());
            normal.retained_commit = normal.selected_commit;
            normal.commits.push(selected);
            normal.graph.push(graph::GraphRow::default());
        }
        normal.visible = (0..normal.commits.len())
            .filter(|index| Some(*index) != normal.retained_commit)
            .collect();
        if let Some(anchor) = anchor
            && let Some(next_top) = normal
                .visible
                .iter()
                .position(|index| normal.commits[*index].oid == anchor)
        {
            normal.scroll.0.borrow().base_handle.set_offset(point(
                offset.x,
                px(automatic_refresh::reanchor_offset(
                    f32::from(offset.y),
                    top,
                    next_top,
                    height,
                )),
            ));
        }
        self.refs = snapshot.refs;
        self.branches = snapshot.branches;
        self.worktrees = snapshot.worktrees;
        self.repository = Some(snapshot.repository);
        self.rebuild_navigation(cx);
        None
    }
}

fn search_reply_current(
    epoch: u64,
    requested_epoch: u64,
    generation: u64,
    requested_generation: u64,
    path: &Option<PathBuf>,
    requested_path: &Option<PathBuf>,
) -> bool {
    epoch == requested_epoch && generation == requested_generation && path == requested_path
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn budget_pages_remain_incomplete_and_keep_the_returned_pinned_scope() {
        let mut progress = Progress::default();
        let scope = HistoryScope::PinnedRefs(vec!["a".repeat(40)]);
        progress.accept(
            &gitturtle_core::HistorySearchPage {
                scope: scope.clone(),
                commits: Vec::new(),
                scanned: 50_000,
                next_offset: Some(50_000),
                stop: HistorySearchStop::ScanLimit,
            },
            0,
        );
        assert!(!progress.exhausted());
        assert_eq!(progress.next_offset, 50_000);
        assert_eq!(progress.pinned, Some(scope));
        progress.accept(
            &gitturtle_core::HistorySearchPage {
                scope: progress.pinned.clone().unwrap(),
                commits: Vec::new(),
                scanned: 3,
                next_offset: None,
                stop: HistorySearchStop::Exhausted,
            },
            0,
        );
        assert!(progress.exhausted());
        assert_eq!(progress.scanned, 50_003);
    }

    #[test]
    fn no_progress_timeout_keeps_a_retry_cursor_without_claiming_completion() {
        let mut progress = Progress {
            next_offset: 42,
            scanned: 42,
            ..Default::default()
        };
        progress.accept(
            &gitturtle_core::HistorySearchPage {
                scope: HistoryScope::FromCommit("b".repeat(40)),
                commits: Vec::new(),
                scanned: 0,
                next_offset: Some(42),
                stop: HistorySearchStop::TimeLimit,
            },
            0,
        );
        assert_eq!(progress.next_offset, 42);
        assert_eq!(progress.scanned, 42);
        assert!(!progress.exhausted());
    }

    #[test]
    fn replies_require_the_same_query_epoch_repository_and_read_generation() {
        let path = Some(PathBuf::from("/repo"));
        assert!(search_reply_current(3, 3, 5, 5, &path, &path));
        assert!(!search_reply_current(4, 3, 5, 5, &path, &path));
        assert!(!search_reply_current(3, 3, 6, 5, &path, &path));
        assert!(!search_reply_current(
            3,
            3,
            5,
            5,
            &Some(PathBuf::from("/other")),
            &path
        ));
    }
}
