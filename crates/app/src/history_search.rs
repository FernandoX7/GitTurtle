//! Repository-wide search shares the replaceable reader with previews.
use crate::*;
use gitturtle_core::{HistoryScope, HistorySearchFilter, HistorySearchStop, PickaxeMode};
use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::FluentBuilder;
use std::time::Duration;

const SEARCH_LABEL: &str = "Searching repository history…";
/// How long typing must pause before a query is searched.
pub(crate) const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);
const MAX_RESULTS: usize = 10_000;
const MAX_RESULT_BYTES: usize = 64 * 1024 * 1024;
/// The narrowest the controls' summary is drawn beside its buttons, at the
/// default interface size: below it the summary wrapped a character to a
/// line.
const SUMMARY_FLOOR: f32 = 160.;

thread_local! {
    /// Cancel's, Continue's and Restart's labels, laid out together once per
    /// font and size.
    static CONTROL_LABELS: std::cell::RefCell<Option<(views::LineKey, Pixels)>> =
        const { std::cell::RefCell::new(None) };
}

/// How the search's controls fit History's column (DESIGN.md, "One
/// responsive order"), decided from widths measured once per font and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ControlsForm {
    /// Cancel, Continue and Restart keep only their icons, names and
    /// tooltips: the column cannot hold the summary's floor beside their
    /// labels. The list's running, paused and failed states then say only
    /// their title, their explanation in its tooltip and accessible
    /// description.
    pub icons: bool,
    /// The summary is drawn: its floor fits beside the buttons as they are
    /// drawn. Where it does not, as in History's column of a half tile, it
    /// leaves; the scope toolbar keeps the match count.
    pub summary: bool,
}

impl ControlsForm {
    /// The form for a `column`-wide History.
    fn of(column: Pixels, window: &Window) -> Self {
        let rem = window.rem_size();
        // The row's room inside `px_3`, and its three `gap_2` gaps.
        let room = column - rem * 1.5;
        let gaps = rem * 1.5;
        let floor = appearance::ui_size(SUMMARY_FLOOR);
        let labels = views::cached_width(&CONTROL_LABELS, &views::LineKey::current(window), || {
            let label = appearance::ui_text(12.);
            views::line_widths(
                &[("Cancel", label), ("Continue", label), ("Restart", label)],
                window,
            )
        });
        // Each labelled button is its label between the shared helper's
        // insets; each icon a 28 px square.
        let labelled = floor + gaps + labels + appearance::ui_size(LABEL_INSET * 6.);
        let icons = room < labelled;
        let summary = !icons || room >= floor + gaps + appearance::ui_size(3. * 28.);
        Self { icons, summary }
    }
}

#[derive(Default)]
pub struct State {
    normal: Option<Retained>,
    progress: Option<Progress>,
    task: Option<Task<()>>,
    debounce: Option<Task<()>>,
    epoch: u64,
    pub(super) filters_open: bool,
    pub(super) filter: HistorySearchFilter,
    pickaxe_regex: bool,
    clearing_filters: bool,
    filter_inputs: Option<FilterInputs>,
}

struct FilterInputs {
    pickaxe: Entity<InputState>,
    author: Entity<InputState>,
    since: Entity<InputState>,
    until: Entity<InputState>,
    path: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

struct Retained {
    paging: history_paging::State,
    commits: Vec<Commit>,
    visible: Vec<usize>,
    graph: Vec<graph::GraphRow>,
    graph_notice: Option<String>,
    selected_commit: Option<usize>,
    history_range: Option<HistoryRange>,
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
    filter: HistorySearchFilter,
    scope_label: String,
    pinned: Option<HistoryScope>,
    next_offset: usize,
    scanned: usize,
    stop: Option<HistorySearchStop>,
    paused: bool,
    error: Option<String>,
    retained_bytes: usize,
}

impl State {
    pub(super) fn pinned_scope(&self) -> Option<HistoryScope> {
        self.progress
            .as_ref()
            .and_then(|progress| progress.pinned.clone())
            .or_else(|| {
                self.normal
                    .as_ref()
                    .and_then(|normal| normal.paging.scope.clone())
            })
    }
    pub(super) fn restore_pinned_scope(&mut self, scope: Option<HistoryScope>) {
        if let Some(progress) = &mut self.progress {
            progress.pinned = scope;
        }
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.normal.as_ref().map_or(0, |normal| {
            repository_tabs::history_bytes(&normal.commits, &normal.graph)
                + normal
                    .files
                    .iter()
                    .map(repository_tabs::file_bytes)
                    .sum::<usize>()
        })
    }
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

/// Runs `app`'s search to its end, for tests in other modules: past its
/// typing pause, then its read, awaited rather than polled.
#[cfg(test)]
pub(crate) async fn settle_history_search(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
) {
    for _ in 0..8 {
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.executor().run_until_parked();
        match app.update(cx, |app, _| app.history_search.task.take()) {
            Some(task) => task.await,
            None if !app.read_with(cx, |app, _| app.history_search_pending()) => break,
            None => {}
        }
    }
    cx.executor().run_until_parked();
    assert!(!app.read_with(cx, |app, _| app.history_search_pending()));
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
    /// A cold tab's restore whose read this one replaces ends here too
    /// (`end_replaced_restore`).
    pub(super) fn pause_history_search_for_read(&mut self) {
        self.end_replaced_restore();
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
        self.worker.release_search();
        let epoch = self.history_search.epoch.wrapping_add(1);
        self.history_search = State {
            epoch,
            ..Default::default()
        };
    }

    pub(super) fn history_query_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.repository_tabs.switching || self.history_search.clearing_filters {
            return;
        }
        let value = self.search.read(cx).value();
        let filter = self.read_history_filter(cx);
        self.history_search.filter = filter.clone();
        if self
            .history_search
            .progress
            .as_ref()
            .is_some_and(|state| state.query == value.as_ref() && state.filter == filter)
        {
            return;
        }
        self.close_inspections(window, cx);
        self.pause_history_search_for_read();
        if value.is_empty() && !filter.filters_without_query() {
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
                paging: self.history_paging.take_settled(),
                commits: std::mem::take(&mut self.commits),
                visible: std::mem::take(&mut self.visible),
                graph: std::mem::take(&mut self.graph),
                graph_notice: self.graph_notice.take(),
                selected_commit: self.selected_commit.take(),
                history_range: self.history_range.take(),
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
        self.history_range = None;
        self.selected_file = None;
        self.files.clear();
        self.file_paths.reset();
        self.clear_preview();
        self.parent = 0;
        self.history_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.history_search.epoch = self.history_search.epoch.wrapping_add(1);
        self.history_search.progress = Some(Progress {
            query: value.to_string(),
            filter,
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
        let timer = cx.background_executor().timer(SEARCH_DEBOUNCE);
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

    pub(super) fn restore_normal_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_search.progress = None;
        self.worker.release_search();
        let Some(normal) = self.history_search.normal.take() else {
            return;
        };
        self.history_paging = normal.paging;
        self.commits = normal.commits;
        self.visible = normal.visible;
        self.graph = normal.graph;
        self.graph_notice = normal.graph_notice;
        self.graph_lanes = self.graph.iter().map(|row| row.width).max().unwrap_or(1);
        self.selected_commit = normal.selected_commit;
        self.history_range = normal.history_range;
        self.files = normal.files;
        self.refresh_file_filter(window, cx);
        self.selected_file = normal.selected_file;
        self.parent = normal.parent;
        self.preferred_file = normal.preferred_file;
        self.history_scroll = normal.scroll;
        self.automatic.retained_commit = normal.retained_commit;
        self.clear_preview();
        self.error = None;
        if self.files.is_empty()
            && let Some(range) = self.history_range.clone()
        {
            self.request_history_range_changes(&range, window, cx);
        } else if self.files.is_empty()
            && let Some(index) = self.selected_commit
        {
            self.select_commit(index, window, cx);
        }
        self.try_automatic_refresh(window, cx);
    }

    pub(super) fn continue_history_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_search_pending() || self.commits.len() >= MAX_RESULTS {
            return;
        }
        if self.operation_busy.is_some() {
            // A cold tab's restore whose saved query a running write keeps
            // from being searched, with no read of its own under way, starts
            // none, and nothing searches it once the write ends: the restore
            // ends in History (`end_restore_in_history`), where the search
            // can be run again.
            if self.task.is_none() {
                self.end_restore_in_history(window, cx);
            }
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
            filter: progress.filter.clone(),
            offset: progress.next_offset,
            limit: 500.min(MAX_RESULTS - self.commits.len()),
            previous: self
                .commits
                .iter()
                .map(|commit| (commit.oid.clone(), commit.parents.clone()))
                .collect(),
            remaining_bytes: MAX_RESULT_BYTES.saturating_sub(progress.retained_bytes),
        };
        // The next page drops the read in flight, which may be a cold tab's
        // restore's.
        self.end_replaced_restore();
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
                // A search that ends with no page to use, failed or
                // dropped, ends a cold tab's restore waiting for it, as a
                // read that replaces it does (`end_replaced_restore`). A
                // restore that would have gone on to Compare stays in
                // History then, where History's search asked for meanwhile
                // opens once the reply is recorded (`run_pending_search`);
                // held for that Compare, it was lost.
                let restore_ended = !matches!(&result, Ok(Ok(Output::SearchHistory(_))))
                    && this.repository_tabs.restoring.take().is_some();
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
                        if this.repository_tabs.restoring.is_some() {
                            this.restore_tab_selection(window, cx);
                        } else if was_empty
                            && !this.commits.is_empty()
                            && this.mode == WorkspaceMode::History
                            && this.page == AppPage::Repository
                        {
                            this.select_commit(0, window, cx);
                        }
                    }
                    Ok(Err(error)) => {
                        // An unpinned search resolves the scope like Refresh;
                        // a vanished one is a scope change, not a failed read.
                        let message = match error.downcast_ref::<worker::VanishedScope>() {
                            Some(vanished) => format!(
                                "History scope changed: {vanished}. Refresh to show All history."
                            ),
                            None => format!("{error:#}"),
                        };
                        if let Some(progress) = &mut this.history_search.progress {
                            progress.error = Some(message);
                        }
                    }
                    Err(_) | Ok(Ok(_)) => {
                        if let Some(progress) = &mut this.history_search.progress {
                            progress.paused = true;
                        }
                    }
                }
                if restore_ended {
                    this.run_pending_search(window, cx);
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn restart_history_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Refused at once while a write runs, before Restart looks so.
        if self.operation_busy.is_some() {
            return;
        }
        self.history_search.progress = None;
        self.history_query_changed(window, cx);
    }

    /// An exhausted search's one line, when it found nothing: how much of
    /// which scope it searched (`empty_state`).
    pub(super) fn history_search_no_match(&self) -> Option<String> {
        let progress = self.history_search.progress.as_ref()?;
        (progress.error.is_none() && !self.history_search_pending() && progress.exhausted()).then(
            || {
                format!(
                    "No commits match · searched {} in {}",
                    crate::counted(progress.scanned, "commit", "commits"),
                    progress.scope_label
                )
            },
        )
    }

    /// History's state for a search with no match: `line`, and Clear search.
    pub(super) fn history_no_match(&self, line: String, cx: &mut Context<Self>) -> AnyElement {
        EmptyState::new("history-no-match", line)
            .action(
                crate::empty_state::action("history-clear-search", "Clear search", "close")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.clear_history_search(window, cx)),
                    ),
            )
            .pane(cx)
    }

    /// Empties the search and restores the selection context it replaced,
    /// the list holding focus, as Escape and Clear search do.
    pub(super) fn clear_history_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.clear_history_filters_only(window, cx);
        self.history_query_changed(window, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn ensure_filter_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_search.filter_inputs.is_some() {
            return;
        }
        let pickaxe = cx.new(|cx| InputState::new(window, cx).placeholder("Changed code (-S/-G)"));
        let author = cx.new(|cx| InputState::new(window, cx).placeholder("Author"));
        let since = cx.new(|cx| InputState::new(window, cx).placeholder("Since (e.g. 2026-01-01)"));
        let until = cx.new(|cx| InputState::new(window, cx).placeholder("Until"));
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("Path (e.g. crates/app)"));
        let mut subscriptions = Vec::with_capacity(5);
        for input in [&pickaxe, &author, &since, &until, &path] {
            subscriptions.push(
                cx.subscribe_in(input, window, |this, _, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.history_query_changed(window, cx);
                    }
                }),
            );
        }
        self.history_search.filter_inputs = Some(FilterInputs {
            pickaxe,
            author,
            since,
            until,
            path,
            _subscriptions: subscriptions,
        });
    }

    fn read_history_filter(&self, cx: &App) -> HistorySearchFilter {
        let mut filter = HistorySearchFilter {
            regex: self.history_search.filter.regex,
            first_parent: self.history_search.filter.first_parent,
            no_merges: self.history_search.filter.no_merges,
            ..Default::default()
        };
        if let Some(inputs) = &self.history_search.filter_inputs {
            let pick = inputs.pickaxe.read(cx).value().trim().to_owned();
            if !pick.is_empty() {
                let mode = if self.history_search.pickaxe_regex {
                    PickaxeMode::Regex
                } else {
                    PickaxeMode::String
                };
                filter.pickaxe = Some((mode, pick));
            }
            let author = inputs.author.read(cx).value().trim().to_owned();
            if !author.is_empty() {
                filter.author = Some(author);
            }
            let since = inputs.since.read(cx).value().trim().to_owned();
            if !since.is_empty() {
                filter.since = Some(since);
            }
            let until = inputs.until.read(cx).value().trim().to_owned();
            if !until.is_empty() {
                filter.until = Some(until);
            }
            let path = inputs.path.read(cx).value().trim().to_owned();
            if !path.is_empty() {
                filter.paths = path
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect();
            }
        }
        filter
    }

    fn clear_history_filters_only(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_search.clearing_filters = true;
        self.history_search.filter = HistorySearchFilter::default();
        self.history_search.pickaxe_regex = false;
        if let Some(inputs) = &self.history_search.filter_inputs {
            for input in [
                &inputs.pickaxe,
                &inputs.author,
                &inputs.since,
                &inputs.until,
                &inputs.path,
            ] {
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
        }
        self.history_search.clearing_filters = false;
    }

    pub(super) fn toggle_history_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_search.filters_open = !self.history_search.filters_open;
        if self.history_search.filters_open {
            self.ensure_filter_inputs(window, cx);
        }
        cx.notify();
    }

    pub(super) fn render_history_filter_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let open = self.history_search.filters_open;
        let active = !self.history_search.filter.is_empty();
        let a11y_label = if !open && active {
            "History search filters (active)"
        } else {
            "History search filters"
        };
        div()
            .relative()
            .flex_shrink_0()
            .child(
                views::compact_control_name(
                    button("history-search-filters-toggle", "", "sliders", open),
                    "history-search-filters-toggle",
                    a11y_label,
                )
                .debug_selector(|| "history-search-filters-toggle".into())
                .toggled(open)
                .tooltip(if !open && active {
                    "Toggle history filters (active filters applied)"
                } else {
                    "Toggle history filters (pickaxe -S/-G, author, date, path, merges)"
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_history_filters(window, cx);
                })),
            )
            .when(!open && active, |el| {
                el.child(
                    div()
                        .debug_selector(|| "history-search-filters-active-dot".into())
                        .absolute()
                        .top(px(3.))
                        .right(px(3.))
                        .size(px(5.))
                        .rounded_full()
                        .bg(rgb(p.accent)),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_history_filter_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.history_search.filters_open {
            return None;
        }
        let inputs = self.history_search.filter_inputs.as_ref()?;
        let p = palette(cx);
        let filter = &self.history_search.filter;
        let pickaxe_regex = self.history_search.pickaxe_regex;
        let has_filters = !filter.is_empty();
        Some(
            crate::native_accessibility::clips_children(
                div()
                    .id("history-search-filter-bar")
                    .debug_selector(|| "history-search-filter-bar".into())
                    .flex_shrink_0()
                    .max_h(appearance::ui_size(140.))
                    .overflow_y_scroll()
                .px_3()
                .pb_2()
                .flex()
                .flex_col()
                .gap_1p5()
                .border_b_1()
                .border_color(rgb(p.border))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_1p5()
                        .child(
                            div()
                                .flex_1()
                                .min_w(appearance::ui_size(130.))
                                .child(
                                    Input::new(&inputs.pickaxe)
                                        .aria_label("Changed code pickaxe filter")
                                        .text_size(appearance::ui_text(11.)),
                                ),
                        )
                        .child(
                            button(
                                "history-filter-pickaxe-mode",
                                if pickaxe_regex { "-G" } else { "-S" },
                                "",
                                pickaxe_regex,
                            )
                            .debug_selector(|| "history-filter-pickaxe-mode".into())
                            .accessibility_label(if pickaxe_regex {
                                "Pickaxe mode: regular expression (-G)"
                            } else {
                                "Pickaxe mode: literal string (-S)"
                            })
                            .toggled(pickaxe_regex)
                            .tooltip(if pickaxe_regex {
                                "Pickaxe regex (-G): match changed diff lines by regex"
                            } else {
                                "Pickaxe string (-S): match commits changing occurrences of literal text"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.history_search.pickaxe_regex =
                                    !this.history_search.pickaxe_regex;
                                this.history_query_changed(window, cx);
                            })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(appearance::ui_size(110.))
                                .child(
                                    Input::new(&inputs.author)
                                        .aria_label("Author filter")
                                        .text_size(appearance::ui_text(11.)),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(appearance::ui_size(110.))
                                .child(
                                    Input::new(&inputs.path)
                                        .aria_label("Path filter")
                                        .text_size(appearance::ui_text(11.)),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_1p5()
                        .child(
                            div()
                                .flex_1()
                                .min_w(appearance::ui_size(120.))
                                .max_w(appearance::ui_size(180.))
                                .child(
                                    Input::new(&inputs.since)
                                        .aria_label("Since date filter")
                                        .text_size(appearance::ui_text(11.)),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(appearance::ui_size(100.))
                                .max_w(appearance::ui_size(160.))
                                .child(
                                    Input::new(&inputs.until)
                                        .aria_label("Until date filter")
                                        .text_size(appearance::ui_text(11.)),
                                ),
                        )
                        .child(
                            button("history-filter-regex", "Regex", "", filter.regex)
                                .debug_selector(|| "history-filter-regex".into())
                                .toggled(filter.regex)
                                .tooltip("Match message query as extended regex")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.history_search.filter.regex =
                                        !this.history_search.filter.regex;
                                    this.history_query_changed(window, cx);
                                })),
                        )
                        .child(
                            button(
                                "history-filter-first-parent",
                                "First-parent",
                                "",
                                filter.first_parent,
                            )
                            .debug_selector(|| "history-filter-first-parent".into())
                            .toggled(filter.first_parent)
                            .tooltip("Follow only the first parent on merge commits")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.history_search.filter.first_parent =
                                    !this.history_search.filter.first_parent;
                                this.history_query_changed(window, cx);
                            })),
                        )
                        .child(
                            button(
                                "history-filter-no-merges",
                                "No merges",
                                "",
                                filter.no_merges,
                            )
                            .debug_selector(|| "history-filter-no-merges".into())
                            .toggled(filter.no_merges)
                            .tooltip("Exclude merge commits")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.history_search.filter.no_merges =
                                    !this.history_search.filter.no_merges;
                                this.history_query_changed(window, cx);
                            })),
                        )
                        .when(has_filters, |el| {
                            el.child(
                                button("history-filter-clear", "Clear filters", "close", false)
                                    .debug_selector(|| "history-filter-clear".into())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.clear_history_filters_only(window, cx);
                                        this.history_query_changed(window, cx);
                                    })),
                            )
                        }),
                ),
            )
            .into_any_element(),
        )
    }

    pub(super) fn jump_to_oid(&mut self, oid: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Repository {
            self.page = AppPage::Repository;
        }
        if self.mode != WorkspaceMode::History {
            self.show_history(window, cx);
        }
        if let Some((pos, &index)) = self
            .visible
            .iter()
            .enumerate()
            .find(|(_, index)| self.commits.get(**index).is_some_and(|c| c.oid == oid))
        {
            self.select_commit(index, window, cx);
            self.history_scroll
                .scroll_to_item(pos, ScrollStrategy::Center);
            window.focus(&self.focus, cx);
            self.status = format!("Jumped to {}", short_oid(&oid));
            cx.notify();
            return;
        }
        if self.history_search_active() {
            self.clear_history_search(window, cx);
            if let Some((pos, &index)) = self
                .visible
                .iter()
                .enumerate()
                .find(|(_, index)| self.commits.get(**index).is_some_and(|c| c.oid == oid))
            {
                self.select_commit(index, window, cx);
                self.history_scroll
                    .scroll_to_item(pos, ScrollStrategy::Center);
                window.focus(&self.focus, cx);
                self.status = format!("Jumped to {}", short_oid(&oid));
                cx.notify();
                return;
            }
        }
        if self.scope.is_some()
            && let Some(path) = self.path.clone()
        {
            self.restore_commit = Some(oid);
            self.open(path, None, window, cx);
            return;
        }
        self.operation_notice = Some(format!(
            "Commit {} is outside the currently loaded history window. Load more commits or open Compare revisions.",
            short_oid(&oid)
        ));
        cx.notify();
    }

    pub(super) fn jump_to_head(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let head_oid = self
            .work_status
            .as_ref()
            .and_then(|s| s.head.clone())
            .or_else(|| {
                self.branches
                    .iter()
                    .find(|b| b.current)
                    .map(|b| b.oid.clone())
            });
        let Some(oid) = head_oid else {
            self.operation_notice = Some("HEAD commit is not available yet.".into());
            cx.notify();
            return;
        };
        self.jump_to_oid(oid, window, cx);
    }

    pub(super) fn open_jump_to_revision(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        if self.repository.is_none() {
            return;
        }
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let form = cx.new(|cx| JumpToRevisionForm::new(owner, repository, window, cx));
        let input_focus = form.read(cx).input.read(cx).focus_handle(cx);
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let submit = form.clone();
            dialog
                .title(native_accessibility::dialog_title(
                    "jump-to-revision-title",
                    "Jump to commit or ref",
                ))
                .width(appearance::dialog_width(px(460.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(form.clone())
                .footer(crate::tags::dialog_footer("Jump", Some("Cancel")))
                .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
        });
        input_focus.focus(window, cx);
    }
}

struct JumpToRevisionForm {
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    input: Entity<InputState>,
    error: Option<String>,
    busy: bool,
}

impl JumpToRevisionForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repository: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Commit SHA, branch, tag, or HEAD~3")
        });
        Self {
            owner,
            repository,
            input,
            error: None,
            busy: false,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.busy {
            return false;
        }
        let spec = self.input.read(cx).value().trim().to_owned();
        if spec.is_empty() {
            self.error = Some("Enter a commit SHA, branch, tag, or revision expression.".into());
            cx.notify();
            return false;
        }
        let Some(owner_entity) = self.owner.upgrade() else {
            return true;
        };
        let repo = owner_entity.read(cx).repository.clone();
        let Some(repo) = repo else {
            return true;
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        let response = owner_entity
            .read(cx)
            .operations
            .submit_read(move || repo.resolve_revision(&spec));
        let owner = self.owner.clone();
        let repository = self.repository.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Revision lookup was interrupted.")));
            let _ = this.update_in(cx, |form, window, cx| {
                form.busy = false;
                match result {
                    Ok(oid) => {
                        window.close_dialog(cx);
                        let _ = owner.update(cx, |app, cx| {
                            if app.path == repository && app.page == AppPage::Repository {
                                app.jump_to_oid(oid, window, cx);
                            }
                        });
                    }
                    Err(error) => {
                        form.error = Some(
                            gitturtle_core::strip_bidi_controls(&format!("{error:#}")).into_owned(),
                        );
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        false
    }
}

impl Render for JumpToRevisionForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div()
            .debug_selector(|| "jump-to-revision-dialog".into())
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .child(
                        "Resolve any local commit SHA, branch, tag, or expression such as HEAD~5.",
                    ),
            )
            .child(Input::new(&self.input).aria_label("Revision expression"))
            .children(self.error.as_ref().map(|error| {
                div()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(p.warning))
                    .child(error.clone())
            }))
    }
}

impl GitTurtle {
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
                    "Searched {} in {}.",
                    crate::counted(progress.scanned, "commit", "commits"),
                    progress.scope_label
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
                "{} searched. Continue to inspect older history in {}.",
                crate::counted(progress.scanned, "commit", "commits"),
                progress.scope_label
            ),
        ))
    }

    /// How the search's controls fit the width History last measured.
    pub(super) fn history_search_form(&self, window: &Window) -> ControlsForm {
        ControlsForm::of(px(self.history_width), window)
    }

    /// History's list while a search with nothing to list runs, paused or
    /// failed: its title and explanation, or, in a column whose controls keep
    /// only their icons, its title alone as one line, the explanation its
    /// tooltip and accessible description. In a half tile the explanation
    /// wrapped over the list's whole height and ran past it.
    pub(super) fn history_search_state(
        &self,
        title: &'static str,
        description: String,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        if self.history_search_form(window).icons {
            EmptyState::new("history-search-state", title)
                .tooltip(description)
                .pane(cx)
        } else {
            empty(title, &description)
        }
    }

    pub(super) fn render_history_search_controls(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(progress) = &self.history_search.progress else {
            return div().into_any_element();
        };
        let colors = palette(cx);
        let running = self.history_search_pending();
        let form = self.history_search_form(window);
        // An exhausted search with no match says how much it searched in the
        // list's own line beside Clear search, where Cancel and Continue are
        // disabled. Where the controls keep only their icons the status below
        // them, which would repeat it, leaves. In the narrowest column, as a
        // half tile's, and in a window short enough for the compact chrome,
        // as any 490 px tall one at 18 pt, the row leaves too, so the line
        // and Clear search keep the list's whole height: beside the row the
        // list was 94.5 px at 13 pt in 461 × 490 and 68.5 px at 18 pt in
        // 861 × 490, which the state outgrew. The row returns with a wider
        // column or a taller window; until then editing the query searches
        // again from the current tips, as Restart does.
        let no_match = self.visible.is_empty() && self.history_search_no_match().is_some();
        if no_match && (!form.summary || self.chrome(window, cx).is_compact()) {
            return div().into_any_element();
        }
        let status = !(form.icons && no_match);
        // Labelled, or in a narrow column the icon, named and with a tooltip
        // that leads with the name.
        let control =
            |id: &'static str, name: &'static str, symbol: &str, tooltip: &'static str| {
                if form.icons {
                    views::compact_control_name(button(id, "", symbol, false), id, name)
                        .debug_selector(move || id.into())
                        .tooltip(tooltip)
                } else {
                    button(id, name, "", false).debug_selector(move || id.into())
                }
            };
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
                format!(
                    "{} · {}",
                    progress.scope_label,
                    crate::counted(refs.len(), "pinned tip", "pinned tips")
                )
            }
            _ => progress.scope_label.clone(),
        };
        let (continue_name, continue_tooltip) = if progress.error.is_some() {
            ("Retry", "Retry · Search this part of history again")
        } else {
            ("Continue", "Continue · Search older history in this scope")
        };
        div()
            .px_3()
            .pb_2()
            .flex()
            .flex_col()
            .gap_1()
            .text_size(crate::appearance::ui_text(10.))
            .text_color(rgb(colors.muted))
            // Where the column cannot hold the summary's floor beside the
            // labelled buttons, they keep only their icons, and where it
            // cannot hold the floor beside those either the summary leaves,
            // so in a half tile the row is one line of icons: wrapped below
            // the summary, the labels took three rows there and left the
            // list 2 px. A summary given no width at all wrapped a character
            // to a line and pushed the list out of the window.
            .child(
                div()
                    .debug_selector(|| "history-search-controls".into())
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .when(form.summary, |row| {
                        row.child(
                            div()
                                .debug_selector(|| "history-search-summary".into())
                                .flex_grow(1.)
                                .flex_basis(crate::appearance::ui_size(SUMMARY_FLOOR))
                                .min_w_0()
                                .child(format!(
                                    "{scope} · {} · {} searched",
                                    crate::counted(self.commits.len(), "match", "matches"),
                                    crate::counted(progress.scanned, "commit", "commits")
                                )),
                        )
                    })
                    .child(
                        control(
                            "history-search-cancel",
                            "Cancel",
                            "close",
                            "Cancel · Stop searching; the matches found so far stay",
                        )
                        .disabled(!running)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.pause_history_search_for_read();
                                this.try_automatic_refresh(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        control(
                            "history-search-continue",
                            continue_name,
                            "chevron",
                            continue_tooltip,
                        )
                        // A write refuses it at once
                        // (`continue_history_search`); it looks so once the
                        // write looks busy (`workspace::BusyLook`).
                        .map(|more| {
                            let spent = running
                                || progress.exhausted()
                                || self.commits.len() >= MAX_RESULTS;
                            crate::workspace::refusing(
                                more,
                                || "history-search-continue".into(),
                                spent || self.operation_busy.is_some(),
                                spent || self.operation_shown().is_some(),
                            )
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.continue_history_search(window, cx)
                        })),
                    )
                    .child(
                        control(
                            "history-search-restart",
                            "Restart",
                            "refresh",
                            "Restart · Restart this search at the current local branch and worktree tips",
                        )
                        .when(!form.icons, |restart| {
                            restart.tooltip(
                                "Restart this search at the current local branch and worktree tips",
                            )
                        })
                        .map(|restart| {
                            crate::workspace::refusing(
                                restart,
                                || "history-search-restart".into(),
                                self.operation_busy.is_some(),
                                self.operation_shown().is_some(),
                            )
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.restart_history_search(window, cx)
                        })),
                    ),
            )
            .when(status, |controls| {
                controls.child(
                    div()
                        .debug_selector(|| "history-search-status".into())
                        .child(detail),
                )
            })
            .into_any_element()
    }

    /// A captured search and its normal-history return context stay pinned.
    /// Current navigation metadata and the separate update cue may advance.
    pub(super) fn retain_search_snapshot(
        &mut self,
        snapshot: worker::Snapshot,
        cx: &mut Context<Self>,
    ) -> Option<worker::Snapshot> {
        if self.history_search.normal.is_none() {
            return Some(snapshot);
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

/// Returns once the search a query change started has finished: past its
/// debounce on the test clock, then each page read on the app's reader,
/// a thread `run_until_parked` cannot wait for, has answered and landed.
/// The test clock moves only by a debounce, and by one more whenever one is
/// still armed, as a change the app handles while it settles arms it again.
#[cfg(test)]
pub(crate) fn search_lands(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    for _ in 0..8 {
        if app.read_with(cx, |app, _| app.history_search.debounce.is_some()) {
            cx.executor().advance_clock(SEARCH_DEBOUNCE);
        }
        cx.run_until_parked();
        if !app.read_with(cx, |app, _| app.history_search_pending()) {
            return;
        }
        app.read_with(cx, |app, _| app.worker.wait_idle());
    }
    panic!("the search never finished");
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

    /// A count agrees with its noun wherever a search reports one: Quick
    /// Open's and History search's matches, the commits searched and the
    /// pinned tips. History search read "1 matches · 1 commits searched".
    #[test]
    fn search_counts_agree_with_their_nouns() {
        for (count, one, many, shown) in [
            (0, "match", "matches", "0 matches"),
            (1, "match", "matches", "1 match"),
            (2, "match", "matches", "2 matches"),
            (1, "commit", "commits", "1 commit"),
            (50_003, "commit", "commits", "50003 commits"),
            (1, "pinned tip", "pinned tips", "1 pinned tip"),
        ] {
            assert_eq!(crate::counted(count, one, many), shown);
        }
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

    #[gpui::test]
    async fn history_filters_pickaxe_and_jump_to_revision_and_head(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("alpha.txt"), "needle_alpha\n").unwrap();
        git(repo.path(), &["add", "alpha.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add alpha"]);
        let alpha_oid = String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        git(repo.path(), &["tag", "alpha-mark"]);

        std::fs::write(repo.path().join("beta.txt"), "needle_beta\n").unwrap();
        git(repo.path(), &["add", "beta.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add beta"]);
        let head_oid = String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();

        let (app, cx) = crate::views::tests::opened_repository_window(cx, &repo).await;
        draw(cx);
        assert!(
            cx.debug_bounds("history-search-filter-bar").is_none(),
            "filter bar stays hidden by default"
        );
        let toggle = cx
            .debug_bounds("history-search-filters-toggle")
            .expect("filters toggle is drawn");
        cx.simulate_click(toggle.center(), gpui::Modifiers::none());
        draw(cx);
        assert!(
            cx.debug_bounds("history-search-filter-bar").is_some(),
            "clicking toggle reveals collapsible filter bar"
        );

        // Filter by pickaxe (-S needle_alpha) with an empty message query.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let pickaxe = app
                    .history_search
                    .filter_inputs
                    .as_ref()
                    .expect("filter inputs initialized")
                    .pickaxe
                    .clone();
                pickaxe.update(cx, |input, cx| {
                    input.set_value("needle_alpha", window, cx);
                });
                app.history_query_changed(window, cx);
            });
        });
        search_lands(&app, cx);
        draw(cx);
        let matched = app.read_with(cx, |app, _| {
            app.commits
                .iter()
                .map(|c| c.oid.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            matched,
            vec![alpha_oid.clone()],
            "pickaxe filter finds only the commit touching needle_alpha"
        );

        // Collapsing while filter is active shows the active accent dot.
        let toggle = cx.debug_bounds("history-search-filters-toggle").unwrap();
        cx.simulate_click(toggle.center(), gpui::Modifiers::none());
        draw(cx);
        assert!(
            cx.debug_bounds("history-search-filters-active-dot")
                .is_some(),
            "collapsed active filter shows accent dot"
        );

        // Clearing filters restores normal history.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.clear_history_filters_only(window, cx);
                app.history_query_changed(window, cx);
            });
        });
        cx.run_until_parked();
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.commits.len() >= 3),
            "normal history restored (commits={}, error={:?}, loading={:?}, normal={})",
            app.read_with(cx, |a, _| a.commits.len()),
            app.read_with(cx, |a, _| a.error.clone()),
            app.read_with(cx, |a, _| a.loading),
            app.read_with(cx, |a, _| a.history_search.normal.is_some()),
        );

        // Jump to older commit by OID and jump back to HEAD.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.jump_to_oid(alpha_oid.clone(), window, cx))
        });
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| {
                app.selected_commit.map(|i| app.commits[i].oid.clone())
            }),
            Some(alpha_oid),
            "jump_to_oid selects the target commit"
        );

        cx.update(|window, cx| app.update(cx, |app, cx| app.jump_to_head(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| {
                app.selected_commit.map(|i| app.commits[i].oid.clone())
            }),
            Some(head_oid),
            "jump_to_head returns selection to HEAD"
        );
    }
}
