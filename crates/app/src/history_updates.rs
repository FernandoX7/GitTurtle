//! Local history discovery, independent of the selected inspector and paging cursor.
use crate::*;
use gitturtle_core::HistoryScope;
use gpui_kit::prelude::FluentBuilder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Update {
    New(usize),
    Changed,
}
impl Update {
    fn merge(self, next: Self) -> Self {
        match (self, next) {
            (Self::New(a), Self::New(b)) => Self::New(a.saturating_add(b)),
            _ => Self::Changed,
        }
    }
    fn label(self) -> String {
        match self {
            Self::New(1) => "1 new commit".into(),
            Self::New(count) => format!("{count} new commits"),
            Self::Changed => "History updated".into(),
        }
    }
}

const SCOPE_CHANGED: &str = "History scope changed: ";
const SCOPE_RETAINED: &str =
    ". The displayed history is retained; choose a current branch to browse its history.";
const SCOPE_FALLBACK: &str = ". Showing All history.";

#[derive(Default)]
pub(super) struct State {
    observed: Option<HistoryScope>,
    complete: Option<HashSet<String>>,
    pub pending: Option<Update>,
    followed: bool,
    scope_error: Option<String>,
    /// The last explicit open's All history explanation. A quiet read does
    /// not clear it; the next explicit open does.
    fallback: Option<String>,
    /// A failed quiet read that the operation banner reported, and whether
    /// only its history part failed: the same failure is not raised again
    /// after Dismiss.
    quiet_failure: Option<(String, bool)>,
    pub committed: Option<String>,
}
impl State {
    pub fn retained_bytes(&self) -> usize {
        let scope = match &self.observed {
            Some(HistoryScope::PinnedRefs(tips)) => {
                tips.capacity() * std::mem::size_of::<String>()
                    + tips.iter().map(String::capacity).sum::<usize>()
            }
            Some(HistoryScope::FromCommit(oid)) => oid.capacity(),
            _ => 0,
        };
        scope
            + self.complete.as_ref().map_or(0, |oids| {
                oids.capacity() * (std::mem::size_of::<String>() + std::mem::size_of::<usize>())
                    + oids.iter().map(String::capacity).sum::<usize>()
            })
            + self.committed.as_ref().map_or(0, String::capacity)
            + self.scope_error.as_ref().map_or(0, String::capacity)
            + self.fallback.as_ref().map_or(0, String::capacity)
            + self
                .quiet_failure
                .as_ref()
                .map_or(0, |(message, _)| message.capacity())
    }
    pub fn reset_history(&mut self) {
        self.observed = None;
        self.complete = None;
        self.pending = None;
        self.followed = false;
    }
    pub fn scope_unavailable(&mut self) {
        self.pending = Some(Update::Changed);
        self.followed = false;
    }
    /// Report a vanished scope once, in the notice banner, where it waits
    /// behind a failure that holds the operation banner. `scope_error`
    /// remembers the report after it is dismissed (or an accepted write clears
    /// the notice), so later quiet reads and searches leave it dismissed;
    /// reading the scope again or opening another one forgets it.
    pub fn report_scope_error(&mut self, error: &str, notice: &mut Option<String>) {
        let message = format!("{SCOPE_CHANGED}{error}{SCOPE_RETAINED}");
        if self.scope_error.as_ref() == Some(&message) {
            return;
        }
        *notice = Some(message.clone());
        self.scope_error = Some(message);
    }
    /// An explicit open found its scope gone and read All history. The
    /// explanation takes the notice banner; a failure that holds the
    /// operation banner stays, and the explanation shows once it is dismissed.
    fn report_scope_fallback(
        &mut self,
        vanished: &worker::VanishedScope,
        notice: &mut Option<String>,
    ) {
        self.opened(notice);
        let message = format!("{SCOPE_CHANGED}{vanished}{SCOPE_FALLBACK}");
        *notice = Some(message.clone());
        self.fallback = Some(message);
    }
    /// An explicit open was accepted: forget earlier scope reports and quiet
    /// failures, and take down the notice if it still shows a scope report.
    fn opened(&mut self, notice: &mut Option<String>) {
        self.clear_scope_error(notice);
        if let Some(previous) = self.fallback.take()
            && notice.as_ref() == Some(&previous)
        {
            *notice = None;
        }
        self.quiet_failure = None;
    }
    /// Report a failed quiet read in the operation banner once. After Dismiss
    /// the same failure is not raised again until a read of the part that
    /// failed succeeds or an explicit open. A failure already in the banner
    /// stays; this one is reported by a later read. `history` says whether
    /// only the history part of the read failed.
    pub fn report_quiet_failure(
        &mut self,
        message: String,
        history: bool,
        operation_error: &mut Option<String>,
    ) {
        if operation_error.is_some()
            || self
                .quiet_failure
                .as_ref()
                .is_some_and(|(previous, _)| *previous == message)
        {
            return;
        }
        *operation_error = Some(message.clone());
        self.quiet_failure = Some((message, history));
    }
    /// A quiet read succeeded, having read history too when `history`. Any
    /// success forgets a whole failed read; only a history read forgets a
    /// failed history part.
    pub fn quiet_read_succeeded(&mut self, history: bool) {
        if self
            .quiet_failure
            .as_ref()
            .is_some_and(|(_, history_only)| history || !history_only)
        {
            self.quiet_failure = None;
        }
    }
    pub fn clear_scope_error(&mut self, notice: &mut Option<String>) {
        if let Some(previous) = self.scope_error.take()
            && notice.as_ref() == Some(&previous)
        {
            *notice = None;
        }
    }
    pub fn captured(&mut self, snapshot: &worker::Snapshot) {
        self.remember(snapshot);
        self.pending = None;
        self.followed = false;
    }
    fn observe(&mut self, snapshot: &worker::Snapshot, following: bool) {
        if let Some(previous) = &self.observed
            && let Some(update) =
                classify_snapshot_update(previous, self.complete.as_ref(), snapshot)
        {
            self.pending = Some(self.pending.map_or(update, |pending| pending.merge(update)));
            self.followed = following;
        }
        if following && self.pending.is_some() {
            self.followed = true;
        }
        self.remember(snapshot);
    }
    fn remember(&mut self, snapshot: &worker::Snapshot) {
        self.observed = Some(snapshot.history_scope.clone());
        self.complete = (snapshot.history_next_offset.is_none()
            && snapshot.commits.len() <= history_paging::PAGE_SIZE)
            .then(|| {
                snapshot
                    .commits
                    .iter()
                    .map(|commit| commit.oid.clone())
                    .collect()
            });
    }
}

fn classify_snapshot_update(
    previous: &HistoryScope,
    complete: Option<&HashSet<String>>,
    snapshot: &worker::Snapshot,
) -> Option<Update> {
    if previous == &snapshot.history_scope {
        return None;
    }
    if let Some(old) = complete
        && snapshot.history_next_offset.is_none()
        && snapshot.commits.len() <= history_paging::PAGE_SIZE
    {
        return complete_update(old, &snapshot.commits);
    }
    classify_update(previous, &snapshot.history_scope, &snapshot.commits)
}

fn complete_update(old: &HashSet<String>, commits: &[Commit]) -> Option<Update> {
    let current = commits
        .iter()
        .map(|commit| commit.oid.clone())
        .collect::<HashSet<_>>();
    if old == &current {
        return None;
    }
    if old.is_subset(&current) {
        Some(Update::New(current.len() - old.len()))
    } else {
        Some(Update::Changed)
    }
}

fn single_tip(scope: &HistoryScope) -> Option<&str> {
    match scope {
        HistoryScope::FromCommit(oid) => Some(oid),
        HistoryScope::PinnedRefs(tips) if tips.len() == 1 => Some(&tips[0]),
        _ => None,
    }
}

/// Exact only when the bounded page proves one new linear chain reaches the
/// previous single tip. A merge, removed ref, rewrite, or page limit is honest
/// uncertainty: row positions alone do not establish new reachable commits.
fn classify_update(
    previous: &HistoryScope,
    current: &HistoryScope,
    commits: &[Commit],
) -> Option<Update> {
    if previous == current {
        return None;
    }
    let (Some(old), Some(mut next)) = (single_tip(previous), single_tip(current)) else {
        return Some(Update::Changed);
    };
    if old == next {
        return None;
    }
    for (index, commit) in commits.iter().enumerate() {
        if commit.oid != next || commit.parents.len() != 1 {
            return Some(Update::Changed);
        }
        next = &commit.parents[0];
        if next == old {
            return Some(Update::New(index + 1));
        }
    }
    Some(Update::Changed)
}

fn follows_latest(mode: WorkspaceMode, searching: bool, deep: bool, offset: f32) -> bool {
    mode == WorkspaceMode::History && !searching && !deep && offset >= -0.5
}

impl GitTurtle {
    /// An explicit open's snapshot is All history when its scope vanished:
    /// drop the scope and say why once. Every explicit open falls back here,
    /// as Refresh does: the vanished scope's saved position goes, and its
    /// selected commit stays only where All history lists it; otherwise All
    /// history's first row is selected. Returns whether the open fell back.
    /// Otherwise the open forgets, and takes down, earlier scope reports.
    pub(super) fn accept_open_scope(&mut self, snapshot: &mut worker::Snapshot) -> bool {
        let Some(vanished) = snapshot.vanished_scope.take() else {
            self.history_updates.opened(&mut self.operation_notice);
            return false;
        };
        self.scope = None;
        let listed = |oid: &str| snapshot.commits.iter().any(|commit| commit.oid == oid);
        // Refresh's selection, which it restores or replaces with the first
        // row, and Show latest's retained inspector.
        if self
            .restore_commit
            .as_deref()
            .is_some_and(|oid| !listed(oid))
        {
            self.restore_commit = None;
        }
        if self
            .selected_commit
            .and_then(|index| self.commits.get(index))
            .is_some_and(|commit| !listed(&commit.oid))
        {
            self.selected_commit = None;
        }
        // A restoring tab's position and pinned tips belong to the vanished
        // scope's history; its selection, with its comparison, goes as
        // Refresh's does.
        if let Some(saved) = &mut self.repository_tabs.restoring {
            saved.pinned = None;
            saved.offset = 0;
            saved.history_y = 0.;
            if !saved.selected_oid.as_deref().is_some_and(listed) {
                saved.retain_history_only();
                saved.selection = None;
                saved.parent = 0;
                saved.selected_oid = snapshot.commits.first().map(|commit| commit.oid.clone());
            }
        }
        self.history_updates
            .report_scope_fallback(&vanished, &mut self.operation_notice);
        true
    }

    pub(super) fn apply_quiet_snapshot(
        &mut self,
        snapshot: worker::Snapshot,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.history_updates
            .clear_scope_error(&mut self.operation_notice);
        let offset = f32::from(self.history_scroll.0.borrow().base_handle.offset().y);
        let following = follows_latest(
            self.mode,
            self.history_search_active(),
            self.history_paging.is_deep(self.visible.len()),
            offset,
        );
        self.history_updates.observe(&snapshot, following);
        let Some(snapshot) = self.retain_search_snapshot(snapshot, cx) else {
            return;
        };
        if !following {
            self.refs = snapshot.refs;
            self.branches = snapshot.branches;
            self.worktrees = snapshot.worktrees;
            self.repository = Some(snapshot.repository);
            self.rebuild_navigation(cx);
            return;
        }
        self.install_current_history(snapshot, following, window, cx);
    }

    fn install_current_history(
        &mut self,
        snapshot: worker::Snapshot,
        following: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.history_paging = history_paging::State::from_snapshot(&snapshot);
        let selected = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
            .cloned();
        let offset = self.history_scroll.0.borrow().base_handle.offset();
        let height = f32::from(window.pixel_snap(px(self.settings.density.history_row_height())));
        let top = ((-f32::from(offset.y)) / height).max(0.) as usize;
        let anchor = self
            .visible
            .get(top)
            .and_then(|index| self.commits.get(*index))
            .map(|commit| commit.oid.clone());
        self.refs = snapshot.refs;
        self.branches = snapshot.branches;
        self.worktrees = snapshot.worktrees;
        self.commits = snapshot.commits;
        self.graph = snapshot.graph;
        self.graph_notice = snapshot.graph_notice;
        self.graph_lanes = self.graph.iter().map(|row| row.width).max().unwrap_or(1);
        self.repository = Some(snapshot.repository);
        self.automatic.retained_commit = None;
        self.selected_commit = selected.as_ref().and_then(|selected| {
            self.commits
                .iter()
                .position(|commit| commit.oid == selected.oid)
        });
        if self.selected_commit.is_none()
            && let Some(selected) = selected
        {
            self.selected_commit = Some(self.commits.len());
            self.automatic.retained_commit = self.selected_commit;
            self.commits.push(selected);
            self.graph.push(graph::GraphRow::default());
        }
        self.filter_history_retaining_scroll(cx);
        self.rebuild_navigation(cx);
        if following {
            self.history_scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(offset.x, px(0.)));
        } else if let Some(anchor) = anchor
            && let Some(next_top) = self
                .visible
                .iter()
                .position(|index| self.commits[*index].oid == anchor)
        {
            self.history_scroll.0.borrow().base_handle.set_offset(point(
                offset.x,
                px(automatic_refresh::reanchor_offset(
                    f32::from(offset.y),
                    top,
                    next_top,
                    height,
                )),
            ));
        }
    }

    /// Explicitly capture current local tips. Unlike paging, this never reads
    /// page zero of an old traversal, and it does not select a different commit.
    pub(super) fn show_latest_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.request_history_discovery(
            Job::Open {
                linked: self.repository_tabs.linked_worktree(&path),
                path,
                scope: self.scope.as_ref().map(|scope| scope.1.clone()),
                limit: history_paging::PAGE_SIZE,
            },
            false,
            window,
            cx,
        );
    }

    pub(super) fn view_created_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(repo), Some(oid)) = (
            self.repository.clone(),
            self.history_updates.committed.clone(),
        ) else {
            return;
        };
        self.request_history_discovery(
            Job::HistoryPage {
                repo,
                scope: HistoryScope::FromCommit(oid),
                offset: 0,
                limit: 1,
            },
            true,
            window,
            cx,
        );
    }

    fn request_history_discovery(
        &mut self,
        job: Job,
        view_commit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.loading.is_some() {
            return;
        }
        self.pause_history_search_for_read();
        self.cancel_automatic_read();
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let path = self.path.clone();
        self.loading = Some(if view_commit {
            "Reading created commit…"
        } else {
            "Reading latest local history…"
        });
        self.error = None;
        let response = self.worker.submit(job);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || this.path != path {
                    return;
                }
                this.loading = None;
                this.task = None;
                match result {
                    Ok(Ok(output)) => {
                        this.show_history(window, cx);
                        if view_commit && this.history_search_active() {
                            this.restore_normal_history(window, cx);
                        }
                        this.discard_history_search();
                        this.search
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        match output {
                            Output::Snapshot(mut snapshot) => {
                                let fell_back = this.accept_open_scope(&mut snapshot);
                                this.history_updates.captured(&snapshot);
                                this.install_current_history(snapshot, true, window, cx);
                                if fell_back
                                    && this.selected_commit.is_none()
                                    && let Some(&first) = this.visible.first()
                                {
                                    this.select_commit(first, window, cx);
                                }
                                this.status =
                                    "Latest local history · selected inspector retained".into();
                            }
                            Output::HistoryPage(result) => {
                                if let Some(commit) = result.page.commits.into_iter().next() {
                                    let index = this
                                        .commits
                                        .iter()
                                        .position(|old| old.oid == commit.oid)
                                        .unwrap_or_else(|| {
                                            if let Some(index) =
                                                this.automatic.retained_commit.take()
                                            {
                                                this.commits.remove(index);
                                                this.graph.remove(index);
                                            }
                                            let index = this.commits.len();
                                            this.commits.push(commit);
                                            this.graph.push(graph::GraphRow::default());
                                            this.automatic.retained_commit = Some(index);
                                            index
                                        });
                                    this.filter_history_retaining_scroll(cx);
                                    this.select_commit(index, window, cx);
                                    if let Some(row) = this
                                        .visible
                                        .iter()
                                        .position(|candidate| *candidate == index)
                                    {
                                        this.history_scroll
                                            .scroll_to_item(row, ScrollStrategy::Center);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(Err(error)) => {
                        this.error = Some(format!("Could not read local history: {error:#}"));
                        this.removed_worktree_error = error
                            .downcast_ref::<worker::RemovedWorktree>()
                            .and(this.error.clone());
                    }
                    Err(_) => this.error = Some("Local history read stopped. Try again.".into()),
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn render_history_update_notice(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let update = self.history_updates.pending?;
        let colors = palette(cx);
        let label = update.label();
        let following = self.history_updates.followed
            && follows_latest(
                self.mode,
                self.history_search_active(),
                self.history_paging.is_deep(self.visible.len()),
                f32::from(self.history_scroll.0.borrow().base_handle.offset().y),
            );
        Some(div().px_4().py_1().flex().items_center().gap_2().border_b_1().border_color(rgb(colors.border)).text_size(appearance::ui_text(11.)).text_color(rgb(colors.muted))
            .child(div().id("history-update-status").role(Role::Status).a11y_synthetic_children(native_accessibility::polite).aria_label(label.clone()).child(label))
            .when(following, |row| row.child("· Latest rows visible"))
            .when(!following, |row| row.child("·").child(button("show-latest-history", "Show latest", "", false).disabled(self.loading.is_some() || self.operation_busy.is_some()).tooltip("Read current local tips and show the newest rows; leaves the captured search and keeps the selected inspector").on_click(cx.listener(|this, _, window, cx| this.show_latest_history(window, cx)))))
            .child(div().flex_1())
            .when(following, |row| row.child(button("dismiss-history-update", "Dismiss", "", false).on_click(cx.listener(|this, _, _, cx| { this.history_updates.pending = None; cx.notify(); }))))
            .into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    fn commit(oid: &str, parents: &[&str]) -> Commit {
        Commit {
            oid: oid.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            author: String::new(),
            timestamp: 0,
            subject: String::new(),
            body: String::new(),
        }
    }
    fn tip(oid: &str) -> HistoryScope {
        HistoryScope::FromCommit(oid.into())
    }
    #[::core::prelude::v1::test]
    fn exact_count_requires_proven_single_tip_extension() {
        assert_eq!(
            classify_update(
                &tip("a"),
                &tip("c"),
                &[commit("c", &["b"]), commit("b", &["a"])]
            ),
            Some(Update::New(2))
        );
        assert_eq!(classify_update(&tip("a"), &tip("a"), &[]), None);
        assert_eq!(
            classify_update(&tip("a"), &tip("c"), &[commit("c", &["b"])]),
            Some(Update::Changed)
        );
        assert_eq!(
            classify_update(&tip("a"), &tip("c"), &[commit("c", &["a", "b"])]),
            Some(Update::Changed)
        );
        assert_eq!(
            classify_update(
                &tip("a"),
                &tip("c"),
                &[commit("c", &["b"]), commit("b", &[])]
            ),
            Some(Update::Changed)
        );
        assert_eq!(
            classify_update(
                &HistoryScope::PinnedRefs(vec!["a".into(), "b".into()]),
                &tip("c"),
                &[commit("c", &["a"])]
            ),
            Some(Update::Changed)
        );
    }
    #[::core::prelude::v1::test]
    fn complete_small_scopes_count_new_commits_without_counting_existing_merge_ancestry() {
        let old = HashSet::from(["a".into(), "b".into()]);
        assert_eq!(
            complete_update(
                &old,
                &[
                    commit("merge", &["a", "b"]),
                    commit("a", &[]),
                    commit("b", &[])
                ]
            ),
            Some(Update::New(1))
        );
        assert_eq!(
            complete_update(&old, &[commit("a", &[]), commit("b", &[])]),
            None
        );
        assert_eq!(
            complete_update(&old, &[commit("c", &["a"]), commit("a", &[])]),
            Some(Update::Changed)
        );
        assert_eq!(
            complete_update(&HashSet::new(), &[commit("root", &[])]),
            Some(Update::New(1))
        );
    }
    #[::core::prelude::v1::test]
    fn follow_top_is_independent_of_selection_and_never_moves_other_contexts() {
        assert!(follows_latest(WorkspaceMode::History, false, false, 0.));
        assert!(!follows_latest(WorkspaceMode::History, false, false, -34.));
        assert!(!follows_latest(WorkspaceMode::Compare, false, false, 0.));
        assert!(!follows_latest(WorkspaceMode::Working, false, false, 0.));
        assert!(!follows_latest(WorkspaceMode::History, true, false, 0.));
        assert!(!follows_latest(WorkspaceMode::History, false, true, 0.));
    }
    #[gpui::test]
    async fn quiet_updates_follow_only_the_top_and_preserve_inspector_focus_and_older_rows(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        // GitTurtle::new starts a real preferences worker. Its reply may wake
        // the deterministic scheduler from another thread, including on macOS.
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repo = GitRepository::init(fixture.path().join("repo"), "main").unwrap();
        let snapshot = move |commits: Vec<Commit>| worker::Snapshot {
            repository: repo.clone(),
            branches: vec![],
            worktrees: vec![],
            history_scope: HistoryScope::FromCommit(commits[0].oid.clone()),
            history_offset: 0,
            history_next_offset: None,
            graph: vec![graph::GraphRow::default(); commits.len()],
            graph_notice: None,
            refs: HashMap::new(),
            elapsed: std::time::Duration::ZERO,
            vanished_scope: None,
            linked_worktree: None,
            commits,
        };
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, window_cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                let original = snapshot(vec![commit("a", &[])]);
                app.history_updates.captured(&original);
                app.install_current_history(original, true, window, cx);
                app.mode = WorkspaceMode::History;
                app.selected_commit = Some(0);
                let content = Arc::new(Content::Notice("retained inspector".into()));
                app.content = Some(content.clone());
                window.focus(&app.file_focus, cx);
                let focus = window.focused(cx);
                app.apply_quiet_snapshot(
                    snapshot(vec![commit("b", &["a"]), commit("a", &[])]),
                    window,
                    cx,
                );
                assert_eq!(app.commits[app.selected_commit.unwrap()].oid, "a");
                assert_eq!(app.commits[app.visible[0]].oid, "b");
                assert_eq!(app.history_scroll.0.borrow().base_handle.offset().y, px(0.));
                assert!(Arc::ptr_eq(app.content.as_ref().unwrap(), &content));
                assert_eq!(window.focused(cx), focus);
                app.history_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(-34.)));
                app.apply_quiet_snapshot(
                    snapshot(vec![
                        commit("c", &["b"]),
                        commit("b", &["a"]),
                        commit("a", &[]),
                    ]),
                    window,
                    cx,
                );
                assert_eq!(
                    app.commits[app.visible[0]].oid, "b",
                    "older browsing retains the captured rows"
                );
                assert_eq!(
                    app.history_scroll.0.borrow().base_handle.offset().y,
                    px(-34.)
                );
                assert_eq!(app.commits[app.selected_commit.unwrap()].oid, "a");
                app.mode = WorkspaceMode::Compare;
                app.history_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(0.)));
                app.apply_quiet_snapshot(
                    snapshot(vec![
                        commit("d", &["c"]),
                        commit("c", &["b"]),
                        commit("b", &["a"]),
                        commit("a", &[]),
                    ]),
                    window,
                    cx,
                );
                assert_eq!(
                    app.commits[app.visible[0]].oid, "b",
                    "Compare never follows its hidden viewport"
                );
                assert!(Arc::ptr_eq(app.content.as_ref().unwrap(), &content));
                assert_eq!(window.focused(cx), focus);
                assert_eq!(app.history_updates.pending, Some(Update::New(3)));
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        let preferences = app.read_with(window_cx, |app, _| {
            app.preferences_writer.submit_read(|| Ok(()))
        });
        preferences.await.unwrap().unwrap();
        window_cx.executor().run_until_parked();
        window_cx.update(|_, cx| {
            app.update(cx, |app, _| app._display_preferences_task = None);
        });
    }

    #[::core::prelude::v1::test]
    fn real_local_snapshots_distinguish_captured_paging_latest_and_created_commit() {
        let fixture = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(fixture.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "History fixture"]);
        git(&["config", "user.email", "history@example.invalid"]);
        git(&["commit", "--allow-empty", "-qm", "initial"]);
        let reader = Worker::new();
        let snapshot = || {
            let output = futures::executor::block_on(reader.submit(Job::Open {
                path: fixture.path().to_owned(),
                scope: None,
                limit: history_paging::PAGE_SIZE,
                linked: None,
            }))
            .unwrap()
            .unwrap();
            let Output::Snapshot(snapshot) = output else {
                panic!("expected snapshot")
            };
            snapshot
        };
        let initial = snapshot();
        let initial_oid = initial.commits[0].oid.clone();
        let initial_scope = initial.history_scope.clone();
        let mut updates = State::default();
        updates.captured(&initial);
        git(&["branch", "other"]);
        git(&["commit", "--allow-empty", "-qm", "new local commit"]);
        let fresh = snapshot();
        let new_oid = fresh.commits[0].oid.clone();
        updates.observe(&fresh, false);
        assert_eq!(updates.pending, Some(Update::New(1)));
        updates.observe(&fresh, false);
        assert_eq!(
            updates.pending,
            Some(Update::New(1)),
            "unchanged reads do not repeat counts"
        );
        let repo = fresh.repository.clone();
        let output = futures::executor::block_on(reader.submit(Job::HistoryPage {
            repo: repo.clone(),
            scope: initial_scope,
            offset: 0,
            limit: 1,
        }))
        .unwrap()
        .unwrap();
        let Output::HistoryPage(captured) = output else {
            panic!("expected page")
        };
        assert_eq!(captured.page.commits[0].oid, initial_oid);
        assert_ne!(
            new_oid, initial_oid,
            "Latest must capture current local tips instead of restoring captured page zero"
        );
        git(&["commit", "--allow-empty", "-qm", "subsequent commit"]);
        let output = futures::executor::block_on(reader.submit(Job::HistoryPage {
            repo,
            scope: HistoryScope::FromCommit(new_oid.clone()),
            offset: 0,
            limit: 1,
        }))
        .unwrap()
        .unwrap();
        let Output::HistoryPage(created) = output else {
            panic!("expected page")
        };
        assert_eq!(
            created.page.commits[0].oid, new_oid,
            "View commit resolves its receipt, not current HEAD"
        );
        git(&[
            "commit",
            "--amend",
            "--allow-empty",
            "-qm",
            "rewritten commit",
        ]);
        let rewritten = snapshot();
        updates.observe(&rewritten, false);
        // The preceding unobserved commit is not included in the count. This
        // addition still extends the observed new_oid and is exactly one row.
        assert_eq!(updates.pending, Some(Update::New(2)));
        git(&["reset", "--hard", "-q", &initial_oid]);
        updates.observe(&snapshot(), false);
        assert_eq!(updates.pending, Some(Update::Changed));
        updates.reset_history();
        assert_eq!(updates.pending, None);
        assert!(updates.observed.is_none());
    }

    #[::core::prelude::v1::test]
    fn bursts_coalesce_and_uncertainty_remains_honest() {
        let mut state = State {
            pending: Some(Update::New(2)),
            followed: true,
            ..Default::default()
        };
        state.scope_unavailable();
        assert_eq!(state.pending, Some(Update::Changed));
        assert!(
            !state.followed,
            "a vanished scope cannot claim current rows are visible"
        );
        let mut error = None;
        state.report_scope_error("branch missing", &mut error);
        state.clear_scope_error(&mut error);
        assert!(error.is_none());
        state.report_scope_error("branch missing", &mut error);
        error = Some("unrelated commit failure".into());
        state.clear_scope_error(&mut error);
        assert_eq!(error.as_deref(), Some("unrelated commit failure"));
        assert_eq!(Update::New(2).merge(Update::New(3)), Update::New(5));
        assert_eq!(
            Update::New(2).merge(Update::Changed).merge(Update::New(3)),
            Update::Changed
        );
    }

    /// Git in a disposable fixture, without the caller's configuration.
    fn git(directory: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(directory)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// `main` checked out with "main work" on top of "initial", and `topic`
    /// branched from "initial" with "topic work".
    fn scoped_fixture() -> (tempfile::TempDir, PathBuf) {
        let fixture = tempfile::tempdir().unwrap();
        let path = GitRepository::init(fixture.path().join("repo"), "main")
            .unwrap()
            .path()
            .to_owned();
        for args in [
            &["config", "user.name", "Scope fixture"][..],
            &["config", "user.email", "scope@example.invalid"],
            &["config", "commit.gpgsign", "false"],
            &["commit", "--allow-empty", "-qm", "initial"],
            &["switch", "-qc", "topic"],
            &["commit", "--allow-empty", "-qm", "topic work"],
            &["switch", "-q", "main"],
            &["commit", "--allow-empty", "-qm", "main work"],
        ] {
            git(&path, args);
        }
        (fixture, path)
    }

    fn branch_scope(name: &str) -> Option<(String, worker::Scope)> {
        Some((
            name.into(),
            worker::Scope::Branch {
                name: name.into(),
                remote: false,
            },
        ))
    }

    /// A window whose application starts as a launch does: with `initial`
    /// as the repository to open and `session` as the saved tabs.
    fn launched_window(
        cx: &mut TestAppContext,
        initial: Option<PathBuf>,
        session: repository_tabs::Session,
    ) -> (Entity<GitTurtle>, &mut VisualTestContext) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                GitTurtle::new(
                    initial,
                    Preferences::default(),
                    session,
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                )
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        (app, cx)
    }

    /// A window whose application has opened `path` in `scope`.
    async fn scoped_window<'a>(
        cx: &'a mut TestAppContext,
        path: &std::path::Path,
        scope: Option<(String, worker::Scope)>,
    ) -> (Entity<GitTurtle>, &'a mut VisualTestContext) {
        let (app, cx) = launched_window(cx, None, repository_tabs::Session::default());
        let path = path.to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open(path, scope, window, cx)));
        settle(&app, cx).await;
        (app, cx)
    }

    /// Let every read, write, quiet refresh and search page in flight finish,
    /// with the ones their replies start. Each task stays where the app keeps
    /// it, so the rules that keep a quiet read from starting beside a search,
    /// a status read or a write still hold. No app state is reset.
    async fn settle(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            // The serial executors have answered once they run a marker
            // queued now; the read worker answers on its own thread.
            app.read_with(cx, |app, _| {
                app.operations.drain();
                app.preferences_writer.drain();
            });
            cx.executor().run_until_parked();
            let pending = app.read_with(cx, |app, _| {
                [
                    ("read", app.task.is_some()),
                    ("working status", app.status_task.is_some()),
                    ("operation", app.operation_busy.is_some()),
                    ("quiet read", app.automatic.reading()),
                    ("search page", app.history_search.searching()),
                ]
                .into_iter()
                .filter_map(|(name, busy)| busy.then_some(name))
                .collect::<Vec<_>>()
            });
            if pending.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "still in flight after 30 s: {pending:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // The real watcher would turn each of the fixture's own `git`
        // commands into a quiet read at a moment the test does not choose;
        // the tests queue the quiet reads they mean. Only the watcher stops.
        app.update(cx, |app, _| {
            app.automatic.stop_watching();
            app._display_preferences_task = None;
        });
    }

    fn subjects(app: &GitTurtle) -> Vec<&str> {
        app.visible
            .iter()
            .map(|&index| app.commits[index].subject.as_str())
            .collect()
    }

    fn selected_subject(app: &GitTurtle) -> Option<&str> {
        app.selected_commit
            .map(|index| app.commits[index].subject.as_str())
    }

    /// Refresh as the keyboard shortcut and menu deliver it.
    async fn dispatch_refresh(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            app.update(cx, |app, cx| app.app_focus.focus(window, cx));
            window.dispatch_action(Box::new(Refresh), cx);
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.loading),
            Some("Reading local history…"),
            "Refresh reached the repository page"
        );
        settle(app, cx).await;
    }

    /// A quiet read of the repository's Git state, as a filesystem event
    /// queues it.
    async fn quiet_refresh(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        app.update_in(cx, |app, window, cx| {
            app.queue_automatic_refresh(
                local_refresh::LocalChange {
                    git: true,
                    ..Default::default()
                },
                window,
                cx,
            );
        });
        settle(app, cx).await;
    }

    /// Activate the first navigation row that `matches`, as a click or Enter
    /// on it does.
    async fn activate_row(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        matches: impl Fn(&GitTurtle, &NavRow) -> bool,
    ) {
        let row = app.read_with(cx, |app, _| {
            app.nav_rows
                .iter()
                .position(|row| matches(app, row))
                .expect("a matching navigation row")
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            app.update(cx, |app, cx| app.activate_navigation(row, window, cx));
        });
        settle(app, cx).await;
    }

    /// Press a banner's button found by its debug selector.
    fn press(cx: &mut VisualTestContext, selector: &'static str) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = cx.debug_bounds(selector).expect("a drawn banner button");
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// The operation banner's summary node as AccessKit receives it. The test
    /// platform builds no accessibility tree, so this reads the role, label
    /// and value the element writes; its live politeness is set with its
    /// synthetic children.
    fn banner_node(message: &str) -> gpui::accesskit::Node {
        let summary = crate::views::operation_error_summary(message);
        let mut node = gpui::accesskit::Node::new(summary.a11y_role().expect("summary role"));
        summary.write_a11y_info(&mut node);
        node
    }

    /// A scope report is a notice, not a failure: it holds the notice banner,
    /// whose summary is a status with its text as label and value.
    fn assert_notice(app: &GitTurtle, message: &str) {
        assert_eq!(app.operation_notice.as_deref(), Some(message));
        let summary = crate::views::operation_notice_summary(message, true);
        let mut node = gpui::accesskit::Node::new(summary.a11y_role().expect("summary role"));
        summary.write_a11y_info(&mut node);
        assert_eq!(node.role(), Role::Status, "{message}");
        assert_eq!(node.label(), Some(message));
        assert_eq!(node.value(), Some(message));
    }

    /// An explicit open fell back to All history: no repository error or
    /// unavailable tab, no scope, no failure, and one explanation in the
    /// notice banner.
    fn assert_fell_back(app: &GitTurtle, vanished: &str) {
        assert!(app.error.is_none(), "repository error: {:?}", app.error);
        let tab = &app.repository_tabs.tabs[app.repository_tabs.active.unwrap()];
        assert!(tab.error.is_none(), "unavailable tab: {:?}", tab.error);
        assert!(app.scope.is_none(), "scope kept: {:?}", app.scope);
        assert_eq!(app.operation_error, None, "a scope change is not a failure");
        assert_notice(
            app,
            &format!("History scope changed: {vanished}. Showing All history."),
        );
    }

    /// All history's rows only, with its first row selected: the vanished
    /// scope's selected commit is not among them.
    fn assert_showing_all_history(app: &GitTurtle, vanished: &str) {
        assert_eq!(subjects(app), ["main work", "initial"]);
        assert_eq!(selected_subject(app), Some("main work"));
        assert_eq!(app.commits.len(), 2, "a commit kept outside All history");
        assert_fell_back(app, vanished);
    }

    const TOPIC_GONE: &str =
        "Local branch 'topic' no longer exists in the local repository snapshot";

    fn quiet_report(name: &str) -> String {
        format!(
            "History scope changed: Local branch '{name}' no longer exists in the local repository snapshot. The displayed history is retained; choose a current branch to browse its history."
        )
    }

    #[gpui::test]
    async fn refresh_opens_all_history_when_the_scoped_branch_is_gone(cx: &mut TestAppContext) {
        let (_fixture, path) = scoped_fixture();
        let (app, cx) = scoped_window(cx, &path, branch_scope("topic")).await;
        app.read_with(cx, |app, _| {
            assert_eq!(subjects(app), ["topic work", "initial"]);
        });
        git(&path, &["branch", "-D", "topic"]);
        dispatch_refresh(&app, cx).await;
        app.read_with(cx, |app, _| assert_showing_all_history(app, TOPIC_GONE));
        // The explanation is reported once: later reads leave it alone.
        quiet_refresh(&app, cx).await;
        app.read_with(cx, |app, _| assert_showing_all_history(app, TOPIC_GONE));
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// `topic` checked out in a linked worktree beside the fixture's
    /// repository, opened from the Worktrees list in its own tab. Returns the
    /// worktree's path as the list names it.
    async fn linked_worktree_tab<'a>(
        cx: &'a mut TestAppContext,
        fixture: &tempfile::TempDir,
        path: &std::path::Path,
    ) -> (Entity<GitTurtle>, &'a mut VisualTestContext, PathBuf) {
        let linked = fixture.path().join("linked");
        git(
            path,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "linked-topic",
                linked.to_str().unwrap(),
                "topic",
            ],
        );
        let (app, cx) = scoped_window(cx, path, None).await;
        app.update(cx, |app, cx| {
            app.nav_mode = NavMode::Worktrees;
            app.rebuild_navigation(cx);
        });
        activate_row(&app, cx, |app, row| {
            matches!(row, NavRow::Worktree(index)
                if app.worktrees[*index].branch.as_deref() == Some("linked-topic"))
        })
        .await;
        let listed = app.read_with(cx, |app, _| {
            let Some((_, worker::Scope::Worktree { path: listed })) = &app.scope else {
                panic!("a worktree scope: {:?}", app.scope)
            };
            assert_eq!(app.path.as_ref(), Some(listed));
            assert_eq!(app.repository_tabs.tabs.len(), 2);
            listed.clone()
        });
        (app, cx, listed)
    }

    /// The page error says the worktree was removed, from the typed failure,
    /// and keeps the original error beneath it.
    fn assert_says_removed(app: &GitTurtle, listed: &std::path::Path) {
        let error = app.error.as_deref().expect("an open failure");
        let removed = format!("The worktree {} was removed.", listed.display());
        assert!(error.contains(&removed), "{error}");
        assert_eq!(app.open_failure_title(error), "Worktree removed");
        let missing = std::io::Error::from_raw_os_error(2).to_string();
        assert!(error.contains(&missing), "the original error: {error}");
    }

    #[gpui::test]
    async fn refresh_says_plainly_when_a_worktree_tab_was_removed(cx: &mut TestAppContext) {
        let (fixture, path) = scoped_fixture();
        let (app, cx, listed) = linked_worktree_tab(cx, &fixture, &path).await;
        git(
            &path,
            &["worktree", "remove", "--force", listed.to_str().unwrap()],
        );
        dispatch_refresh(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_says_removed(app, &listed);
            let tab = &app.repository_tabs.tabs[app.repository_tabs.active.unwrap()];
            assert!(tab.error.is_some(), "the tab reads unavailable");
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    async fn a_linked_worktree_tab_in_all_history_says_its_worktree_was_removed(
        cx: &mut TestAppContext,
    ) {
        let (fixture, path) = scoped_fixture();
        let (app, cx, listed) = linked_worktree_tab(cx, &fixture, &path).await;
        activate_row(&app, cx, |_, row| matches!(row, NavRow::All)).await;
        app.read_with(cx, |app, _| {
            assert!(app.scope.is_none(), "All history: {:?}", app.scope);
            assert_eq!(app.path.as_ref(), Some(&listed));
        });
        git(
            &path,
            &["worktree", "remove", "--force", listed.to_str().unwrap()],
        );
        // Show latest keeps the displayed rows; its failure says why.
        app.update_in(cx, |app, window, cx| app.show_latest_history(window, cx));
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            let error = app.error.as_deref().expect("a failed read");
            assert!(
                error.contains(&format!("The worktree {} was removed.", listed.display())),
                "{error}"
            );
            assert_eq!(app.open_failure_title(error), "Worktree removed");
        });
        dispatch_refresh(&app, cx).await;
        app.read_with(cx, |app, _| assert_says_removed(app, &listed));
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// The open diagnostic for a folder that is missing, not removed.
    fn assert_says_missing(app: &GitTurtle) {
        let error = app.error.as_deref().expect("an open failure");
        assert!(
            error.starts_with("The repository folder or a required path is missing."),
            "{error}"
        );
        assert_eq!(app.open_failure_title(error), "Could not open repository");
    }

    #[gpui::test]
    async fn a_main_repository_tab_whose_folder_moved_keeps_the_open_diagnostic(
        cx: &mut TestAppContext,
    ) {
        let (fixture, path) = scoped_fixture();
        let (app, cx) = scoped_window(cx, &path, None).await;
        // Scoped to its own, main, worktree.
        app.update(cx, |app, cx| {
            app.nav_mode = NavMode::Worktrees;
            app.rebuild_navigation(cx);
        });
        let main = app.read_with(cx, |app, _| app.path.clone().unwrap());
        activate_row(
            &app,
            cx,
            |app, row| matches!(row, NavRow::Worktree(index) if app.worktrees[*index].path == main),
        )
        .await;
        app.read_with(cx, |app, _| {
            assert!(
                matches!(&app.scope, Some((_, worker::Scope::Worktree { path })) if *path == main),
                "{:?}",
                app.scope
            );
        });
        std::fs::rename(&main, fixture.path().join("moved-repo")).unwrap();
        dispatch_refresh(&app, cx).await;
        app.read_with(cx, |app, _| assert_says_missing(app));
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    async fn a_moved_worktree_keeps_the_open_diagnostic(cx: &mut TestAppContext) {
        let (fixture, path) = scoped_fixture();
        let (app, cx, listed) = linked_worktree_tab(cx, &fixture, &path).await;
        // `git worktree move` keeps the worktree registered elsewhere.
        let moved = fixture.path().join("moved-linked");
        git(
            &path,
            &[
                "worktree",
                "move",
                listed.to_str().unwrap(),
                moved.to_str().unwrap(),
            ],
        );
        dispatch_refresh(&app, cx).await;
        app.read_with(cx, |app, _| assert_says_missing(app));
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    async fn a_fallback_behind_a_failure_shows_once_that_failure_is_dismissed(
        cx: &mut TestAppContext,
    ) {
        let failure = "Commit failed: the pre-commit hook refused the change";
        let (_fixture, path) = scoped_fixture();
        let (app, cx) = scoped_window(cx, &path, branch_scope("topic")).await;
        app.update(cx, |app, _| app.operation_error = Some(failure.into()));
        git(&path, &["branch", "-D", "topic"]);
        dispatch_refresh(&app, cx).await;
        let fallback = format!("History scope changed: {TOPIC_GONE}. Showing All history.");
        cx.update(|window, cx| window.draw(cx).clear(cx));
        app.read_with(cx, |app, _| {
            // The failure keeps the banner; the explanation waits behind it.
            assert_eq!(app.operation_error.as_deref(), Some(failure));
            assert!(app.scope.is_none());
            assert_eq!(subjects(app), ["main work", "initial"]);
            assert_eq!(app.operation_notice.as_deref(), Some(fallback.as_str()));
            assert!(
                app.notice_announcements.is_empty(),
                "announced behind the failure: {:?}",
                app.notice_announcements
            );
        });
        assert!(cx.debug_bounds("operation-notice-summary").is_none());
        assert_eq!(banner_node(failure).role(), Role::Alert);
        press(cx, "dismiss-operation-error");
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_error, None);
            assert_notice(app, &fallback);
            assert_eq!(app.notice_announcements, [fallback.as_str()]);
        });
        assert!(cx.debug_bounds("operation-notice-summary").is_some());
        // Drawn again, and again after leaving the Repository page and coming
        // back, the notice stays without being announced a second time.
        app.update_in(cx, |app, window, cx| app.show_settings(window, cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        app.update_in(cx, |app, window, cx| app.return_from_page(window, cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("operation-notice-summary").is_some());
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_notice.as_deref(), Some(fallback.as_str()));
            assert_eq!(app.announced_notice.as_deref(), Some(fallback.as_str()));
            assert_eq!(app.notice_announcements, [fallback.as_str()]);
        });
    }

    #[gpui::test]
    async fn an_explicit_open_after_a_fallback_takes_its_banner_down(cx: &mut TestAppContext) {
        let (_fixture, path) = scoped_fixture();
        let (app, cx) = scoped_window(cx, &path, branch_scope("topic")).await;
        git(&path, &["branch", "-D", "topic"]);
        dispatch_refresh(&app, cx).await;
        app.read_with(cx, |app, _| assert_showing_all_history(app, TOPIC_GONE));
        activate_row(&app, cx, |app, row| {
            matches!(row, NavRow::Branch(index, _) if app.branches[*index].name == "main")
        })
        .await;
        app.read_with(cx, |app, _| {
            assert_eq!(
                app.scope.as_ref().map(|scope| scope.0.as_str()),
                Some("main")
            );
            assert_eq!(app.operation_notice, None, "the explanation is taken down");
        });
    }

    #[gpui::test]
    async fn a_dismissed_vanished_scope_is_not_reported_again_by_later_writes(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, path) = scoped_fixture();
        let (app, cx) = scoped_window(cx, &path, branch_scope("topic")).await;
        git(&path, &["branch", "-D", "topic"]);
        quiet_refresh(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_error, None, "a scope change is not a failure");
            assert_notice(app, &quiet_report("topic"));
            assert_eq!(
                subjects(app),
                ["topic work", "initial"],
                "quiet refresh keeps the displayed history"
            );
        });
        press(cx, "dismiss-operation-notice");
        app.read_with(cx, |app, _| assert_eq!(app.operation_notice, None));
        for message in ["first write", "second write"] {
            std::fs::write(path.join(format!("{message}.txt")), message).unwrap();
            git(&path, &["add", "--", &format!("{message}.txt")]);
            app.update_in(cx, |app, window, cx| {
                app.write(
                    gitturtle_core::WriteCommand::Commit {
                        message: message.into(),
                    },
                    "Creating commit…",
                    window,
                    cx,
                );
            });
            settle(&app, cx).await;
            let head = git(&path, &["rev-parse", "HEAD"]);
            app.read_with(cx, |app, _| {
                assert!(
                    app.operation_notice
                        .as_deref()
                        .is_some_and(|notice| notice.contains(message)),
                    "{message} was accepted, and its notice stays: {:?} {:?}",
                    app.operation_notice,
                    app.operation_error
                );
                assert!(
                    app.branches
                        .iter()
                        .any(|branch| branch.name == "main" && branch.oid == head),
                    "a quiet refresh followed {message}"
                );
                assert_eq!(app.operation_error, None);
            });
        }
        press(cx, "dismiss-operation-notice");
        quiet_refresh(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_notice, None);
            assert_eq!(app.operation_error, None);
        });
        // A new scope choice forgets the report, so another vanished scope
        // is reported.
        git(&path, &["branch", "other"]);
        let path_for_open = path.clone();
        app.update_in(cx, |app, window, cx| {
            app.open(path_for_open, branch_scope("other"), window, cx)
        });
        settle(&app, cx).await;
        git(&path, &["branch", "-D", "other"]);
        quiet_refresh(&app, cx).await;
        app.read_with(cx, |app, _| assert_notice(app, &quiet_report("other")));
    }

    #[gpui::test]
    async fn a_dismissed_quiet_read_failure_is_not_raised_again_until_a_read_succeeds(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, path) = scoped_fixture();
        // `topic` reaches a parent whose object goes missing, so reading its
        // history fails while the branch still resolves.
        git(&path, &["switch", "-q", "topic"]);
        git(&path, &["commit", "--allow-empty", "-qm", "lost parent"]);
        let lost = git(&path, &["rev-parse", "HEAD"]);
        git(&path, &["commit", "--allow-empty", "-qm", "topic tip"]);
        git(&path, &["switch", "-q", "main"]);
        let object = path.join(".git/objects").join(&lost[..2]).join(&lost[2..]);
        let bytes = std::fs::read(&object).unwrap();
        let (app, cx) = scoped_window(cx, &path, branch_scope("topic")).await;
        let raised =
            |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.operation_error.clone());
        std::fs::remove_file(&object).unwrap();
        quiet_refresh(&app, cx).await;
        let failure = raised(cx).expect("the failed read is reported");
        assert!(failure.starts_with("Local refresh: "), "{failure}");
        assert_eq!(banner_node(&failure).role(), Role::Alert);
        press(cx, "dismiss-operation-error");
        quiet_refresh(&app, cx).await;
        assert_eq!(raised(cx), None, "raised again after Dismiss");
        // An explicit open forgets it.
        std::fs::write(&object, &bytes).unwrap();
        dispatch_refresh(&app, cx).await;
        assert_eq!(app.read_with(cx, |app, _| app.error.clone()), None);
        std::fs::remove_file(&object).unwrap();
        quiet_refresh(&app, cx).await;
        assert_eq!(raised(cx), Some(failure.clone()));
        press(cx, "dismiss-operation-error");
        // So does a quiet read that succeeds.
        std::fs::write(&object, &bytes).unwrap();
        quiet_refresh(&app, cx).await;
        assert_eq!(raised(cx), None);
        std::fs::remove_file(&object).unwrap();
        quiet_refresh(&app, cx).await;
        assert_eq!(raised(cx), Some(failure));
    }

    #[gpui::test]
    async fn search_under_a_vanished_scope_reports_it_and_refresh_searches_all_history(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, path) = scoped_fixture();
        let (app, cx) = scoped_window(cx, &path, branch_scope("topic")).await;
        git(&path, &["branch", "-D", "topic"]);
        let search = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.search
                        .update(cx, |input, cx| input.set_value("work", window, cx));
                    app.history_query_changed(window, cx);
                })
            });
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(300));
        };
        search(cx);
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.error.is_none(), "repository error: {:?}", app.error);
            let tab = &app.repository_tabs.tabs[app.repository_tabs.active.unwrap()];
            assert!(tab.error.is_none(), "unavailable tab: {:?}", tab.error);
            assert_eq!(app.operation_error, None, "a scope change is not a failure");
            assert_notice(app, &quiet_report("topic"));
            assert_eq!(
                app.history_search_empty().map(|(title, _)| title),
                Some("Search could not complete")
            );
        });
        // Refresh shows All history, and the search runs again there rather
        // than against the vanished scope.
        dispatch_refresh(&app, cx).await;
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(300));
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.scope.is_none());
            assert!(app.history_search_active());
            assert_eq!(subjects(app), ["main work"]);
            assert_notice(
                app,
                &format!("History scope changed: {TOPIC_GONE}. Showing All history."),
            );
        });
    }

    #[gpui::test]
    async fn show_latest_opens_all_history_when_the_scoped_branch_is_gone(cx: &mut TestAppContext) {
        let (_fixture, path) = scoped_fixture();
        let (app, cx) = scoped_window(cx, &path, branch_scope("topic")).await;
        app.read_with(cx, |app, _| {
            assert_eq!(selected_subject(app), Some("topic work"));
        });
        git(&path, &["branch", "-D", "topic"]);
        app.update_in(cx, |app, window, cx| app.show_latest_history(window, cx));
        assert_eq!(
            app.read_with(cx, |app, _| app.loading),
            Some("Reading latest local history…")
        );
        settle(&app, cx).await;
        // As Refresh: the vanished scope's selected commit, which All
        // history does not list, gives way to All history's first row.
        app.read_with(cx, |app, _| assert_showing_all_history(app, TOPIC_GONE));
    }

    #[gpui::test]
    async fn a_restored_tab_opens_all_history_when_its_saved_scope_is_gone(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, path) = scoped_fixture();
        let topic = git(&path, &["rev-parse", "topic"]);
        let selected = Commit {
            oid: topic.clone(),
            parents: vec![git(&path, &["rev-parse", "topic^"])],
            author: "Scope fixture".into(),
            timestamp: 0,
            subject: "topic work".into(),
            body: String::new(),
        };
        // Deleted while the app was closed.
        git(&path, &["branch", "-D", "topic"]);
        let bookmark = repository_tabs::Bookmark {
            scope: Some((
                "topic".into(),
                repository_tabs::SavedScope::Branch {
                    name: "topic".into(),
                    remote: false,
                },
            )),
            // A position in the vanished scope's own history, which All
            // history must not restore, and a selected commit that only the
            // deleted branch reached.
            pinned: Some(repository_tabs::SavedHistoryScope::Commit(topic.clone())),
            offset: 1,
            selected_oid: Some(topic),
            selection: Some(repository_tabs::SavedCommit::new(&selected)),
            ..Default::default()
        };
        let session = repository_tabs::Session {
            version: 1,
            active: 0,
            tabs: vec![repository_tabs::SavedTab {
                path: repository_tabs::SavedPath::Text(path.to_str().unwrap().into()),
                bookmark,
            }],
            ..Default::default()
        };
        let (app, cx) = launched_window(cx, Some(path.clone()), session);
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_deref(), Some(path.as_path()));
            assert_showing_all_history(app, TOPIC_GONE);
        });
    }

    #[gpui::test]
    async fn a_branch_row_opens_all_history_when_its_branch_is_gone(cx: &mut TestAppContext) {
        let (_fixture, path) = scoped_fixture();
        let (app, cx) = scoped_window(cx, &path, None).await;
        let row = app.read_with(cx, |app, _| {
            app.nav_rows
                .iter()
                .position(|row| {
                    matches!(row, NavRow::Branch(index, _) if app.branches[*index].name == "topic")
                })
                .expect("a topic row")
        });
        // Deleted outside GitTurtle; the sidebar has not refreshed.
        git(&path, &["branch", "-D", "topic"]);
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            // What clicking the row, or Enter on it, does.
            app.update(cx, |app, cx| app.activate_navigation(row, window, cx));
        });
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_showing_all_history(app, TOPIC_GONE);
            assert!(
                app.branches.iter().all(|branch| branch.name != "topic"),
                "the sidebar now lists current branches"
            );
        });
    }
}
