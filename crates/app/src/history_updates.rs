//! Local history discovery, independent of the selected inspector and paging cursor.
use crate::*;
use gitturtle_core::HistoryScope;

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
    scope_error: Option<String>,
    /// The vanished scope already reported, so later quiet refreshes and
    /// accepted writes do not raise it again after it is dismissed.
    reported_vanished: Option<worker::VanishedScope>,
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
            + self
                .reported_vanished
                .as_ref()
                .map_or(0, |vanished| match &vanished.0 {
                    worker::Scope::Branch { name, .. } => name.capacity(),
                    worker::Scope::Worktree { path } => path.capacity(),
                })
    }
    pub fn reset_history(&mut self) {
        self.observed = None;
        self.complete = None;
        self.pending = None;
    }
    pub fn scope_unavailable(&mut self) {
        self.pending = Some(Update::Changed);
    }
    /// A quiet read could not resolve the scope; the displayed history stays.
    /// One vanished scope is reported once: after its report is dismissed,
    /// later quiet refreshes and accepted writes leave it dismissed.
    pub fn report_scope_error(
        &mut self,
        error: &str,
        vanished: Option<&worker::VanishedScope>,
        operation_error: &mut Option<String>,
    ) {
        if vanished.is_some() && vanished == self.reported_vanished.as_ref() {
            return;
        }
        self.scope_unavailable();
        let message = format!(
            "History scope changed: {error}. The displayed history is retained; choose a current branch to browse its history."
        );
        if operation_error.is_none() || operation_error.as_ref() == self.scope_error.as_ref() {
            *operation_error = Some(message.clone());
            self.scope_error = Some(message);
            self.reported_vanished = vanished.cloned();
        }
    }
    /// A snapshot resolved its scope (or replaced it): a later disappearance
    /// is a new change and is reported again.
    pub fn clear_scope_error(&mut self, operation_error: &mut Option<String>) {
        self.reported_vanished = None;
        if let Some(previous) = self.scope_error.take()
            && operation_error.as_ref() == Some(&previous)
        {
            *operation_error = None;
        }
    }
    pub fn captured(&mut self, snapshot: &worker::Snapshot) {
        self.remember(snapshot);
        self.pending = None;
    }
    /// A quiet read's `snapshot`. Where History is `following` its newest
    /// rows, the read is installed at the top of the list it follows, so the
    /// rows a cue would announce, this read's and any an earlier one cued,
    /// are the ones it shows: the cue ends rather than staying, as it did
    /// through the reads after Pull and Push with the new commit already on
    /// top. Elsewhere the list keeps its rows and the update is cued.
    fn observe(&mut self, snapshot: &worker::Snapshot, following: bool) {
        if following {
            self.pending = None;
        } else if let Some(previous) = &self.observed
            && let Some(update) =
                classify_snapshot_update(previous, self.complete.as_ref(), snapshot)
        {
            self.pending = Some(self.pending.map_or(update, |pending| pending.merge(update)));
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
    /// Accept the scope of an explicit snapshot (Refresh, Show latest, a
    /// restored tab). When its branch or worktree scope had vanished the worker
    /// read All history: drop the stale scope and say once, politely, why.
    pub(super) fn accept_snapshot_scope(&mut self, snapshot: &worker::Snapshot) {
        self.history_updates
            .clear_scope_error(&mut self.operation_error);
        if let Some(vanished) = &snapshot.vanished_scope {
            self.scope = None;
            // A restored tab's pinned traversal belonged to the vanished
            // scope; All history starts from current tips instead.
            if let Some(saved) = &mut self.repository_tabs.restoring {
                saved.scope = None;
                saved.pinned = None;
                saved.offset = 0;
            }
            self.operation_notice = Some(format!(
                "History scope changed: {vanished}. Showing All history."
            ));
        }
    }

    pub(super) fn apply_quiet_snapshot(
        &mut self,
        snapshot: worker::Snapshot,
        window: &Window,
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
                                this.accept_snapshot_scope(&snapshot);
                                this.history_updates.captured(&snapshot);
                                this.install_current_history(snapshot, true, window, cx);
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

    /// The cue History's scope toolbar shows while a quiet read has
    /// observed history the list does not show yet
    /// (`views::render_history_scope_toolbar`). It lives in the toolbar,
    /// where the bar it replaced, a full-width row above History in every
    /// mode, pushed the page down 35 px after a Fetch; Latest, beside it, is
    /// its action, which reads current local tips and ends it, as the bar's
    /// Show latest and Dismiss did. A later quiet read that History follows
    /// at its top, such as the one after a Pull, shows those rows and ends
    /// it too, as Refresh, which captures current tips, does.
    pub(super) fn history_update_cue(&self) -> Option<UpdateCue> {
        let update = self.history_updates.pending?;
        Some(UpdateCue {
            label: update.label().into(),
        })
    }
}

/// [`GitTurtle::history_update_cue`].
pub(super) struct UpdateCue {
    /// The words the toolbar draws where they fit and announces once,
    /// politely: "1 new commit", "History updated". They stay the same as
    /// History scrolls, so the status is not announced again.
    pub label: SharedString,
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
        let repo = crate::views::tests::fixture_repository(fixture.path().join("repo"));
        let snapshot = move |commits: Vec<Commit>| worker::Snapshot {
            repository: repo.clone(),
            branches: vec![],
            worktrees: vec![],
            history_scope: HistoryScope::FromCommit(commits[0].oid.clone()),
            history_offset: 0,
            history_next_offset: None,
            graph: vec![graph::GraphRow::default(); commits.len()],
            graph_notice: None,
            refs: Default::default(),
            elapsed: std::time::Duration::ZERO,
            vanished_scope: None,
            commits,
        };
        crate::views::tests::init_app(cx);
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
                // "b", followed at the top, is shown; "c" and "d", read
                // while History kept its rows, are cued.
                assert_eq!(app.history_updates.pending, Some(Update::New(2)));
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
            ..Default::default()
        };
        state.scope_unavailable();
        assert_eq!(state.pending, Some(Update::Changed));
        let mut error = None;
        state.report_scope_error("branch missing", None, &mut error);
        state.clear_scope_error(&mut error);
        assert!(error.is_none());
        state.report_scope_error("branch missing", None, &mut error);
        error = Some("unrelated commit failure".into());
        state.clear_scope_error(&mut error);
        assert_eq!(error.as_deref(), Some("unrelated commit failure"));
        assert_eq!(Update::New(2).merge(Update::New(3)), Update::New(5));
        assert_eq!(
            Update::New(2).merge(Update::Changed).merge(Update::New(3)),
            Update::Changed
        );
    }

    /// Draws History with `update` cued. A quiet read History follows at its
    /// top, such as the local watcher's after the fixture's own writes, shows
    /// current rows and ends the cue, and can land between a draw's frames,
    /// so the cue is set and drawn again until it lasted through them all.
    fn draw_cued(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, update: Update) {
        for _ in 0..50 {
            app.update(cx, |app, _| app.history_updates.pending = Some(update));
            crate::views::tests::draw(cx);
            if app.read_with(cx, |app, _| app.history_updates.pending == Some(update)) {
                return;
            }
        }
        panic!("the cue lasted through a draw");
    }

    /// The new-history cue lives in History's scope toolbar, so nothing
    /// below it moves: the full-width bar it replaced pushed the list down
    /// 35 px after a Fetch. Once a quiet read finds a new commit, as the
    /// refresh after a Fetch does, the list's first row keeps its place at
    /// 13 and 18 pt in 1,480 × 980, 1,000 × 680 and 461 × 490 windows; the
    /// cue's status lies inside the toolbar, its words whole and clear of
    /// Columns where they fit (not in a half tile, where the toolbar keeps
    /// only its controls); and Latest, its action, is drawn there marked by
    /// its dot, and compact, is named for the cue. Pressing Latest ends it.
    #[gpui::test]
    async fn new_history_is_cued_inside_the_scope_toolbar_and_moves_nothing(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, fixture_git, history_script, history_window_from, settle};
        let (fixture, app, cx) = history_window_from(cx, &history_script(3, 1)).await;
        settle(&app, cx).await;
        let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        let within = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left()
                && inner.right() <= outer.right()
                && inner.top() >= outer.top()
                && inner.bottom() <= outer.bottom()
        };
        let first_row = bounds(cx, "history-row-0").top();

        // A commit made elsewhere, read by a quiet refresh as a Fetch's is,
        // in Changes, where History keeps its rows: at History's top the
        // read would show it there, with nothing to cue.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        fixture_git(
            &fixture.path().join("narrow"),
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "--quiet",
                "-m",
                "fetched",
            ],
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_automatic_refresh(
                    local_refresh::LocalChange {
                        git: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        });
        for _ in 0..2000 {
            cx.run_until_parked();
            if app.read_with(cx, |app, _| app.history_updates.pending.is_some()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.history_updates.pending),
            Some(Update::New(1))
        );
        // Back at History's top, a later quiet read, such as the local
        // watcher's of the same commit, is followed and ends the cue
        // (`a_followed_quiet_read_shows_the_cued_rows_and_ends_the_cue`), so
        // each layout below draws the cue from the update this read found.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        settle(&app, cx).await;
        assert_eq!(
            bounds(cx, "history-row-0").top(),
            first_row,
            "nothing moved"
        );

        for (text, width, height, words) in [
            (13, 1480., 980., true),
            (18, 1480., 980., true),
            (13, 1000., 680., true),
            (18, 1000., 680., true),
            (13, 461., 490., false),
            (18, 461., 490., false),
        ] {
            let at = format!("{text} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            crate::views::COMPACT_LABELS.with(|names| names.borrow_mut().clear());
            draw_cued(&app, cx, Update::New(1));
            let toolbar = bounds(cx, "history-scope-toolbar");
            let status = bounds(cx, "history-update-status");
            assert!(within(status, toolbar), "{at}: {status:?} in {toolbar:?}");
            assert_eq!(status.size.width > px(1.), words, "{at}: the words");
            if words {
                let columns = bounds(cx, "columns");
                assert!(status.right() <= columns.left(), "{at}: {status:?}");
            }
            let latest = bounds(cx, "history-newest");
            assert!(within(latest, toolbar), "{at}: Latest {latest:?}");
            let dot = bounds(cx, "history-newest-dot");
            assert!(within(dot, latest), "{at}: its dot {dot:?}");
            if cx.debug_bounds("history-scope-label").is_some() {
                let name = crate::views::COMPACT_LABELS
                    .with(|names| names.borrow().get("history-newest").cloned());
                assert_eq!(
                    name.as_deref(),
                    Some("Latest · 1 new commit"),
                    "{at}: compact Latest's name"
                );
            }
            let with_cue = bounds(cx, "history-row-0").top();
            let pending = app.update(cx, |app, _| app.history_updates.pending.take());
            draw(cx);
            assert_eq!(
                bounds(cx, "history-row-0").top(),
                with_cue,
                "{at}: the cue moved the list"
            );
            assert!(cx.debug_bounds("history-update-status").is_none());
            assert!(cx.debug_bounds("history-newest-dot").is_none());
            app.update(cx, |app, _| app.history_updates.pending = pending);
        }

        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw_cued(&app, cx, Update::New(1));
        let latest = bounds(cx, "history-newest");
        cx.simulate_click(latest.center(), Modifiers::default());
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.history_updates.pending),
            None
        );
        assert!(cx.debug_bounds("history-update-status").is_none());
    }

    /// A quiet read History follows at its top, as the read after a Pull or
    /// a Push from History is (`refresh_after_write` queues it), installs
    /// its rows there and ends the cue an earlier read, made in Changes,
    /// raised: the rows it announced are shown, so its status and Latest's
    /// dot go. The cue once stayed through Pull and Push with the new commit
    /// already the top row.
    #[gpui::test]
    async fn a_followed_quiet_read_shows_the_cued_rows_and_ends_the_cue(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, fixture_git, history_script, history_window_from, settle};
        let (fixture, app, cx) = history_window_from(cx, &history_script(3, 1)).await;
        settle(&app, cx).await;
        let repository = fixture.path().join("narrow");
        let commit = |message: &str| {
            fixture_git(
                &repository,
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "--allow-empty",
                    "--quiet",
                    "-m",
                    message,
                ],
            )
        };
        let quiet_read = |cx: &mut VisualTestContext, done: &dyn Fn(&GitTurtle) -> bool| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.queue_automatic_refresh(
                        local_refresh::LocalChange {
                            git: true,
                            worktree: true,
                            ..Default::default()
                        },
                        window,
                        cx,
                    )
                })
            });
            for _ in 0..2000 {
                cx.run_until_parked();
                if app.read_with(cx, |app, _| done(app)) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("the quiet read landed");
        };
        let rows = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.visible
                    .iter()
                    .take(2)
                    .map(|&row| app.commits[row].subject.clone())
                    .collect::<Vec<_>>()
            })
        };

        // A commit read in Changes, where History keeps its rows, is cued.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        commit("fetched");
        quiet_read(cx, &|app| app.history_updates.pending.is_some());
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.history_updates.pending),
            Some(Update::New(1))
        );
        // Back at History's top the cue lasts exactly while the list does
        // not show the commit: the local watcher's own read of it, followed
        // there, may already have shown it and ended the cue.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        settle(&app, cx).await;
        draw(cx);
        let cued = app.read_with(cx, |app, _| app.history_updates.pending.is_some());
        assert_eq!(
            cued,
            rows(cx).first().map(String::as_str) != Some("fetched"),
            "the cue lasts while the list does not show the commit"
        );
        assert_eq!(cx.debug_bounds("history-newest-dot").is_some(), cued);

        // The read after a Pull, followed at History's top, shows it and
        // the commit the Pull brought, and the cue ends.
        commit("pulled");
        quiet_read(cx, &|app| {
            app.visible
                .first()
                .is_some_and(|&row| app.commits[row].subject == "pulled")
        });
        settle(&app, cx).await;
        draw(cx);
        assert_eq!(rows(cx), ["pulled", "fetched"]);
        assert_eq!(
            app.read_with(cx, |app, _| app.history_updates.pending),
            None,
            "the rows the cue announced are shown"
        );
        assert!(cx.debug_bounds("history-update-status").is_none());
        assert!(cx.debug_bounds("history-newest-dot").is_none());
    }

    /// While the cue lasts, Latest's dot is 5 px across, 1 px inside the
    /// button's top right corner, scaled with the text, at 13 and 18 pt in
    /// the labelled and the compact toolbar. In the compact square it stands
    /// at least 2 px clear of the up arrow, whose ink beside the rows the
    /// dot covers ends 8.8 px in from the square's right edge at 13 pt: the
    /// 6 px dot 2 px in stood 0.8 px from it there and 0.9 px at 18 pt.
    #[gpui::test]
    async fn latest_marks_the_cue_with_a_dot_clear_of_its_arrow(cx: &mut TestAppContext) {
        use crate::views::tests::{history_script, history_window_from, settle};
        let (_fixture, app, cx) = history_window_from(cx, &history_script(3, 1)).await;
        settle(&app, cx).await;
        let near = |a: Pixels, b: Pixels| (f32::from(a) - f32::from(b)).abs() <= 0.5;
        for (text, width, height, compact) in [
            (13, 1480., 980., false),
            (18, 1480., 980., false),
            (13, 461., 490., true),
            (18, 461., 490., true),
        ] {
            let at = format!("{text} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw_cued(&app, cx, Update::New(1));
            let latest = cx.debug_bounds("history-newest").expect("Latest is drawn");
            let dot = cx
                .debug_bounds("history-newest-dot")
                .expect("its dot is drawn");
            let inset = appearance::ui_size(1.);
            assert!(
                near(dot.size.width, appearance::ui_size(5.)),
                "{at}: {dot:?}"
            );
            assert!(
                near(dot.size.height, appearance::ui_size(5.)),
                "{at}: {dot:?}"
            );
            assert!(
                near(dot.top() - latest.top(), inset),
                "{at}: {dot:?} in {latest:?}"
            );
            assert!(
                near(latest.right() - dot.right(), inset),
                "{at}: {dot:?} in {latest:?}"
            );
            assert_eq!(
                near(latest.size.width, latest.size.height),
                compact,
                "{at}: Latest's square {latest:?}"
            );
            if compact {
                let ink = latest.right() - appearance::ui_size(8.8);
                assert!(
                    f32::from(dot.left() - ink) >= 2.,
                    "{at}: {dot:?} clear of the arrow's ink at {ink:?}"
                );
            }
        }
    }
}
