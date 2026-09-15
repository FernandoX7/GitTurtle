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

#[derive(Default)]
pub(super) struct State {
    observed: Option<HistoryScope>,
    complete: Option<HashSet<String>>,
    pub pending: Option<Update>,
    followed: bool,
    scope_error: Option<String>,
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
    pub fn report_scope_error(&mut self, error: &str, operation_error: &mut Option<String>) {
        let message = format!(
            "History scope changed: {error}. The displayed history is retained; choose a current branch to browse its history."
        );
        if operation_error.is_none() || operation_error.as_ref() == self.scope_error.as_ref() {
            *operation_error = Some(message.clone());
            self.scope_error = Some(message);
        }
    }
    pub fn clear_scope_error(&mut self, operation_error: &mut Option<String>) {
        if let Some(previous) = self.scope_error.take()
            && operation_error.as_ref() == Some(&previous)
        {
            *operation_error = None;
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
    pub(super) fn apply_quiet_snapshot(
        &mut self,
        snapshot: worker::Snapshot,
        cx: &mut Context<Self>,
    ) {
        self.history_updates
            .clear_scope_error(&mut self.operation_error);
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
        self.install_current_history(snapshot, following, cx);
    }

    fn install_current_history(
        &mut self,
        snapshot: worker::Snapshot,
        following: bool,
        cx: &mut Context<Self>,
    ) {
        self.history_paging = history_paging::State::from_snapshot(&snapshot);
        let selected = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
            .cloned();
        let offset = self.history_scroll.0.borrow().base_handle.offset();
        let height = self.settings.density.history_row_height();
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
                            Output::Snapshot(snapshot) => {
                                this.history_updates
                                    .clear_scope_error(&mut this.operation_error);
                                this.history_updates.captured(&snapshot);
                                this.install_current_history(snapshot, true, cx);
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
                        this.error = Some(format!("Could not read local history: {error:#}"))
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
                app.install_current_history(original, true, cx);
                app.mode = WorkspaceMode::History;
                app.selected_commit = Some(0);
                let content = Arc::new(Content::Notice("retained inspector".into()));
                app.content = Some(content.clone());
                window.focus(&app.file_focus, cx);
                let focus = window.focused(cx);
                app.apply_quiet_snapshot(snapshot(vec![commit("b", &["a"]), commit("a", &[])]), cx);
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
}
