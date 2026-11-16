//! Local refresh scheduling and application without resetting browsing state.

use crate::*;
use futures::StreamExt;
use local_refresh::{LocalChange, LocalChanges, LocalWatcher, WatchRoots};

#[derive(Default)]
pub struct State {
    /// What the watcher subscribes through, kept across [`Self::reset`].
    backend: local_refresh::Backend,
    watcher: Option<LocalWatcher>,
    roots: Option<WatchRoots>,
    watch_task: Option<Task<()>>,
    watch_failed: bool,
    watch_warning: Option<String>,
    events_task: Option<Task<()>>,
    task: Option<Task<()>>,
    pending: LocalChange,
    inflight: Option<LocalChange>,
    epoch: u64,
    pub retained_commit: Option<usize>,
    /// The "Local refresh:" banner a failed quiet read put in the window's
    /// shared error slot, as written ([`Self::report_failure`]): the next
    /// quiet read that succeeds takes that banner and no other, and a slot
    /// that says anything else, a write's or an explicit operation's
    /// failure in whatever words, stays. The text a person could read does
    /// not tell them apart, so the whole failure is kept and compared, its
    /// report and its banner line. A tab left keeps it with the slot.
    failure: Option<crate::views::OperationFailure>,
    /// Test-only: quiet reads wait to start while set, as a busy reader
    /// holds them ([`Self::hold_quiet_reads`]).
    #[cfg(test)]
    held: bool,
}

impl State {
    /// Whether the quiet reload a write asked for as it ended
    /// (`refresh_after_write`) runs, or waits for nothing but what starts it
    /// as it ends (a running read, a history load or search, a status read,
    /// a write): until it lands the window shows what the write found, and
    /// menus refuse writes (`GitTurtle::write_unsettled`). `may_start` is
    /// false while another page or an open inspection holds quiet reads
    /// (`GitTurtle::quiet_reads_held`), so the reload that waits then refuses
    /// nothing until the window returns to it: the menus of a window left on
    /// Blame or Settings once stayed refused, with nothing to say why. A read
    /// the watcher asked for alone refuses nothing, so saving a file never
    /// dims a menu.
    pub(super) fn reloading_after_write(&self, may_start: bool) -> bool {
        self.inflight.as_ref().is_some_and(|read| read.after_write)
            || (self.pending.after_write && may_start)
    }
    /// Test-only: holds quiet reads from starting, or lets them start
    /// again, so a test can act between a write's end and its reload.
    #[cfg(test)]
    pub(crate) fn hold_quiet_reads(&mut self, held: bool) {
        self.held = held;
    }
    /// Whether a quiet read runs, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn quiet_read_running(&self) -> bool {
        self.inflight.is_some()
    }
    pub(super) fn watch_warning(&self) -> Option<&str> {
        self.watch_warning.as_deref()
    }
    /// Says a quiet read failed in the window's shared error slot, where
    /// that slot is free: a write's failure or an explicit operation's
    /// stays, and nothing of it is taken for this read's.
    fn report_failure(
        &mut self,
        failure: crate::views::OperationFailure,
        operation_error: &mut Option<crate::views::OperationFailure>,
    ) {
        if operation_error.is_none() {
            *operation_error = Some(failure.clone());
            self.failure = Some(failure);
        }
    }
    /// Takes the banner a failed quiet read left ([`Self::report_failure`])
    /// as the next one succeeds, where the slot still says it.
    fn clear_failure(&mut self, operation_error: &mut Option<crate::views::OperationFailure>) {
        if let Some(failure) = self.failure.take()
            && operation_error.as_ref() == Some(&failure)
        {
            *operation_error = None;
        }
    }
    /// The banner to carry away with a tab left ([`Self::restore_failure`]),
    /// which `reset` forgets with the rest of the repository's quiet reads.
    pub(super) fn take_failure(&mut self) -> Option<crate::views::OperationFailure> {
        self.failure.take()
    }
    /// The banner of a tab shown again, retained with its error slot.
    pub(super) fn restore_failure(&mut self, failure: Option<crate::views::OperationFailure>) {
        self.failure = failure;
    }
    /// Whether a rescan waits to run, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn rescan_queued(&self) -> bool {
        self.pending.rescan
    }
    /// Whether neither a quiet read nor the watcher's setup waits for its
    /// reply, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn replies_landed(&self) -> bool {
        self.task.is_none() && self.watch_task.is_none()
    }
    /// Whether the repository is watched, its roots known, as the window
    /// keeps a shown tab, for tests in other modules: a check of the watch
    /// then subscribes nothing new and asks for no rescan.
    #[cfg(test)]
    pub(crate) fn watching(&self) -> bool {
        self.watcher.is_some() && self.roots.is_some()
    }
    /// Reports `change` through the repository's subscription, as the
    /// watcher reports a change made outside the app, for tests in other
    /// modules: the window takes it as it takes the watcher's
    /// (`GitTurtle::listen_for_local_changes`).
    #[cfg(test)]
    pub(crate) fn report_local_change(&mut self, change: LocalChange) {
        self.watcher
            .as_mut()
            .expect("a watched repository")
            .report(change);
    }
    /// Whether a quiet read, which reads the working state whatever it was
    /// asked for, waits to run or runs, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn quiet_read_requested(&self) -> bool {
        !self.pending.is_empty() || self.inflight.is_some()
    }
    /// Forgets the repository's watch and quiet reads, as opening another
    /// repository does. A test's hold on quiet reads stays. It belongs to
    /// the write the test holds them across, not to this state, and cleared
    /// here it would let that write's reload start before the test acts.
    pub fn reset(&mut self) {
        let epoch = self.epoch.wrapping_add(1);
        *self = Self {
            epoch,
            backend: self.backend,
            #[cfg(test)]
            held: self.held,
            ..Default::default()
        };
    }

    fn can_start(
        &self,
        repository_page: bool,
        writing: bool,
        reading: bool,
        status_reading: bool,
    ) -> bool {
        #[cfg(test)]
        if self.held {
            return false;
        }
        repository_page
            && !writing
            && !reading
            && !status_reading
            && self.task.is_none()
            && !self.pending.is_empty()
    }
}

impl GitTurtle {
    pub(super) fn ensure_local_watcher(
        &mut self,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.check_local_watcher(force, true, window, cx);
    }

    fn check_local_watcher(
        &mut self,
        force: bool,
        retry_failed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository.clone() else {
            return;
        };
        if self.automatic.watch_task.is_some() || (!force && self.automatic.watcher.is_some()) {
            return;
        }
        let path = repo.path().to_owned();
        let previous = if self.automatic.watch_failed && retry_failed {
            None
        } else {
            self.automatic.roots.clone()
        };
        let backend = self.automatic.backend;
        let response = self.operations.submit_read(move || {
            // A setup that fails because the repository went away while it
            // ran is the repository gone, whatever step it failed in: Git
            // says "not a git repository" once the `.git` is gone or half
            // removed, which Git then decides, but a `.git` moved between
            // Git's report of the administration directories and their
            // resolution fails that resolution, and their watch can fail
            // too. Only a repository still at its path gets the watcher's
            // warning; that warning once won the race.
            let setup = || {
                let (private_git, common_git) = repo.git_directories()?;
                let roots = WatchRoots {
                    worktree: repo.path().to_owned(),
                    private_git,
                    common_git,
                };
                let subscription = if previous.as_ref() == Some(&roots) {
                    None
                } else {
                    Some(local_refresh::subscribe(backend, roots.clone())?)
                };
                anyhow::Ok((roots, subscription))
            };
            setup().map_err(|error| worker::gone_unless_found(&repo, error))
        });
        self.automatic.watch_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Local watcher setup stopped")));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.path.as_ref() != Some(&path) { return; }
                let shown = QuietDrawn::read(this);
                this.automatic.watch_task = None;
                match result {
                    Ok((roots, subscription)) => {
                        this.automatic.roots = Some(roots);
                        if let Some((watcher, changes)) = subscription {
                            this.automatic.watch_failed = false;
                            this.automatic.watch_warning = None;
                            this.automatic.watcher = Some(watcher);
                            this.listen_for_local_changes(path, changes, window, cx);
                            // Close the small interval between the preceding
                            // snapshot and completed watcher registration.
                            this.queue_automatic_refresh(LocalChange { rescan: true, ..Default::default() }, window, cx);
                        }
                    },
                    // Gone from its root, the repository is read as Refresh
                    // reads it, which marks the tab and says why; it once
                    // said "Automatic refresh could not start … not a git
                    // repository" and, with no watcher, nothing marked it.
                    Err(error) if error.is::<worker::RepositoryGone>() => {
                        this.automatic.watch_failed = true;
                        this.tab_repository_gone(window, cx);
                    }
                    Err(error) => {
                        this.automatic.watch_failed = true;
                        this.automatic.watch_warning = Some(format!("Automatic refresh could not start.\n\n{error:#}\n\nRefresh reads current local state and retries the watcher."));
                    }
                }
                // A watch set up as it was, or registered with the rescan it
                // asks for waiting behind the one running, draws nothing new
                // (`QuietDrawn`): a tab shown again drew the same frame again
                // as its watch was set up and as each rescan checked it.
                if QuietDrawn::read(this) != shown {
                    cx.notify();
                }
            });
        }));
    }

    fn listen_for_local_changes(
        &mut self,
        path: PathBuf,
        mut changes: LocalChanges,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.automatic.events_task = Some(cx.spawn_in(window, async move |this, cx| {
            while let Some(change) = changes.next().await {
                let active = this.update_in(cx, |this, window, cx| {
                    if this.path.as_ref() != Some(&path) {
                        return false;
                    }
                    this.queue_automatic_refresh(change, window, cx);
                    true
                });
                if !matches!(active, Ok(true)) {
                    break;
                }
            }
        }));
    }

    pub(super) fn queue_automatic_refresh(
        &mut self,
        change: LocalChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if change.recovered {
            self.automatic.watch_failed = false;
            self.automatic.watch_warning = None;
        } else if let Some(error) = &change.error {
            self.automatic.watch_failed = true;
            self.automatic.watch_warning = Some(error.clone());
        }
        self.automatic.pending.merge(change);
        self.try_automatic_refresh(window, cx);
    }

    /// Drops the repository's local watch and subscribes again through
    /// `backend`, as opening the repository does: with
    /// [`local_refresh::Backend::Unavailable`] the status bar shows that some
    /// changes need Refresh, as when the host's inotify instances run out.
    #[cfg(test)]
    pub(crate) fn watch_through(
        &mut self,
        backend: local_refresh::Backend,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.automatic.reset();
        self.automatic.backend = backend;
        self.ensure_local_watcher(true, window, cx);
    }

    pub(super) fn retry_automatic_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_local_watcher(true, window, cx);
        self.queue_automatic_refresh(
            LocalChange {
                rescan: true,
                ..Default::default()
            },
            window,
            cx,
        );
        cx.notify();
    }

    pub(super) fn show_automatic_refresh_details(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui_kit::component::WindowExt;
        let Some(details) = self.automatic.watch_warning.clone() else {
            return;
        };
        let editor = text::editor(&details, "text", None, window, cx);
        // Keyed by the details, read once: the dialog shows them unchanged.
        let mark = copy_feedback::Mark::new("copy-refresh-details", &details);
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let details = details.clone();
            let copy_mark = mark.clone();
            dialog.title("Automatic refresh")
                .width(appearance::dialog_width(px(620.), window)).max_h(appearance::dialog_max_height(window))
                .child(div().child("Some local changes may need Refresh. Existing watches continue where available. Refresh reads local changes and retries coverage."))
                .child(crate::editor_find::Editor::new(&editor).readonly(true).h(px(240.)).aria_label("Automatic refresh details"))
                .child(copy_feedback::copy_button(mark.clone(), "Copy details", "", cx).on_click(move |_, window, cx| {
                    copy_feedback::copy(window, cx, Some(copy_mark.clone()), "refresh details", details.clone());
                }))
                .footer(crate::tags::dialog_footer("Close", None))
        });
    }

    pub(super) fn cancel_automatic_read(&mut self) {
        if let Some(change) = self.automatic.inflight.take() {
            self.automatic.pending.merge(change);
            self.worker.cancel();
        }
        self.automatic.task = None;
        self.automatic.epoch = self.automatic.epoch.wrapping_add(1);
    }

    /// Whether a passive reader runs or is open, which reads the repository
    /// as it was asked to and holds quiet reads off until it ends or closes,
    /// then starts them: a history load or search, File history, a revision
    /// inspection or Blame.
    fn passive_reading(&self) -> bool {
        self.loading.is_some() || self.history_search_pending() || self.inspection_open()
    }

    /// Whether File history, a revision inspection or Blame is open, which
    /// holds quiet reads off until it closes.
    fn inspection_open(&self) -> bool {
        self.file_history.is_active()
            || self.revision_inspection.is_active()
            || self.blame.is_visible()
    }

    /// Whether the window holds quiet reads off for as long as it stays as
    /// it is: another page is shown, as returning to the repository starts
    /// them, or an inspection is open ([`Self::inspection_open`]). A history
    /// load or search holds them only while it runs: one starts as a commit
    /// or a row is selected, putting a running read back to wait
    /// ([`Self::cancel_automatic_read`]), and starts it again as it ends or
    /// as the read that replaces it does. Counted here, it let a write's
    /// reload refuse nothing while a selection's read ran, so a commit's
    /// menu opened then offered Revert and Undo of a commit an Undo had just
    /// taken away, and one open showed its items available, refused and
    /// available again, its cursor reset at each.
    pub(super) fn quiet_reads_held(&self) -> bool {
        self.page != AppPage::Repository || self.inspection_open()
    }

    /// Whether a quiet read reads the shown repository's working state,
    /// whatever it was asked for: one runs, or one waits to start as the
    /// write, read, search or page that holds it back ends. One an open
    /// inspection holds back waits for whoever closes it
    /// ([`Self::inspection_open`]), so it does not count: with attribution
    /// open on Changes and its status read failed, a change reported
    /// meanwhile once kept Changes saying "Reading the working tree…" for
    /// as long as attribution stayed open, though nothing read.
    pub(super) fn quiet_read_reads_working(&self) -> bool {
        self.automatic.task.is_some()
            || (!self.automatic.pending.is_empty() && !self.inspection_open())
    }

    pub(super) fn try_automatic_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.automatic.can_start(
            self.page == AppPage::Repository,
            self.operation_busy.is_some(),
            self.passive_reading(),
            self.status_task.is_some(),
        ) {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let change = std::mem::take(&mut self.automatic.pending);
        // A tab whose Open failed shows no history: the read that finds its
        // repository again reads the history Refresh would, so clearing the
        // mark brings back what Open would have shown
        // ([`Self::tab_found_readable`]).
        let history = change.git || change.rescan || self.shown_tab_unread();
        let selected = (self.mode == WorkspaceMode::Working)
            .then(|| {
                let (index, area) = self.working_selected?;
                let entry = self.work_status.as_ref()?.entries.get(index)?;
                Some(worker::WorkingSelection {
                    path: entry.path.clone(),
                    area,
                    file: self.files.first().cloned(),
                    content: self.content.clone(),
                })
            })
            .flatten();
        let path = repo.path().to_owned();
        let generation = self.generation;
        let work_generation = self.work_generation;
        let requested = self.request_status_read();
        self.automatic.epoch = self.automatic.epoch.wrapping_add(1);
        let epoch = self.automatic.epoch;
        self.automatic.inflight = Some(change);
        let response = self.worker.submit(Job::QuietRefresh {
            repo,
            scope: self.scope.as_ref().map(|scope| scope.1.clone()),
            limit: self.limit,
            history,
            selected,
        });
        self.automatic.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.automatic.epoch != epoch {
                    return;
                }
                let shown = QuietDrawn::read(this);
                let mut found = true;
                this.automatic.task = None;
                let changed = this.automatic.inflight.take().unwrap_or_default();
                if !reply_is_current(
                    this.path.as_ref(),
                    &path,
                    this.generation,
                    generation,
                    this.work_generation,
                    work_generation,
                ) || this.operation_busy.is_some()
                {
                    this.automatic.pending.merge(changed);
                    this.try_automatic_refresh(window, cx);
                    return;
                }
                match result {
                    Ok(Ok(Output::QuietRefresh(refresh))) => {
                        // Before it lands, so a banner this read writes
                        // finds the slot free.
                        this.automatic.clear_failure(&mut this.operation_error);
                        // Read whole, the repository is readable again: a
                        // tab marked by a failed Open states that, as an
                        // explicit Refresh's accepted snapshot does. A read
                        // that fails keeps the mark.
                        // Only a read that brings History back counts: one
                        // whose scope vanished (`Retained`) leaves the
                        // History the failed Open cleared empty, and cleared
                        // over it, the mark left "No commits yet" with
                        // nothing to say why where an earlier read's error
                        // held the banner. Refresh opens All history then.
                        let readable = matches!(
                            refresh.snapshot,
                            None | Some(Ok(worker::QuietHistory::Refreshed(_)))
                        );
                        let shown = this.shows_quiet_refresh(&refresh);
                        let entries = this.apply_quiet_refresh(
                            *refresh,
                            changed.after_write,
                            requested,
                            window,
                            cx,
                        );
                        found = !shown || entries;
                        if readable && this.tab_found_readable(cx) {
                            found = true;
                        }
                        // A failed native watch gets one local state read. A
                        // later event, manual Refresh, or regained focus may
                        // reconnect; do not spin on a persistent watch error.
                        if history && changed.error.is_none() {
                            this.check_local_watcher(true, false, window, cx);
                        }
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => this.quiet_read_failed(error, window, cx),
                    Err(_) => {
                        this.automatic.pending.merge(changed);
                    }
                }
                this.try_automatic_refresh(window, cx);
                // A read that found the repository as the window shows it
                // draws nothing new, as the rescan of a tab shown again
                // finds it: that frame once drew the tab again, and the
                // rescan the watcher asks for once registered a third time.
                if found || QuietDrawn::read(this) != shown {
                    cx.notify();
                }
            });
        }));
    }

    /// What a quiet read's failure says. Gone from its root, the repository
    /// is read as Refresh reads it, whose failed Open marks the tab and says
    /// why. Changed while the read opened it, at its own path, a marked
    /// tab's mark says that already: a banner saying the same beside it once
    /// stayed after the next good read cleared the mark. Anything else, an
    /// unmarked tab's such change among it, is the "Local refresh:" banner
    /// the next read that succeeds takes away; no Open runs for it, which
    /// would cancel an open form and leave Changes for a passing failure.
    pub(super) fn quiet_read_failed(
        &mut self,
        error: anyhow::Error,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if error.is::<worker::RepositoryGone>() {
            self.tab_repository_gone(window, cx);
        } else if !(error.is::<gitturtle_core::ChangedWhileOpening>()
            && self.shown_tab_unavailable())
        {
            self.automatic.report_failure(
                crate::views::OperationFailure::of_read("Local refresh", &error),
                &mut self.operation_error,
            );
        }
    }

    /// Whether a quiet read found what the window shows, as its landing
    /// installs it whatever History shows: the working state but its
    /// entries, the branches, worktrees and references History and its
    /// navigation draw, no scope gone and the working file's comparison
    /// unchanged. The entries are compared once, as they are applied
    /// (`apply_worktree_state`), and History's rows, which the landing
    /// installs only where History follows its newest, by whether it
    /// installed a history that differs ([`QuietDrawn`]). Each field is
    /// named, so one added to the status is compared or the build fails.
    fn shows_quiet_refresh(&self, refresh: &worker::QuietRefresh) -> bool {
        let working = &refresh.working;
        let gitturtle_core::RepositoryStatus {
            entries: _,
            branch,
            head,
            upstream,
            ahead,
            behind,
            operation,
        } = &working.status;
        let same_status = self.work_status.as_deref().is_some_and(|shown| {
            shown.branch == *branch
                && shown.head == *head
                && shown.upstream == *upstream
                && shown.ahead == *ahead
                && shown.behind == *behind
                && shown.operation == *operation
        });
        let same_working = same_status
            && matches!(&working.profile, Ok(profile) if self.profile.as_ref() == Some(profile))
            && matches!(&working.remotes, Ok(remotes) if *remotes == self.remotes)
            && matches!(&working.operation, Ok(operation) if *operation == self.integration_state);
        let same_history = match &refresh.snapshot {
            None => true,
            Some(Ok(worker::QuietHistory::Refreshed(snapshot))) => {
                snapshot.vanished_scope.is_none()
                    && snapshot.refs == self.refs
                    && snapshot.branches == self.branches
                    && snapshot.worktrees == self.worktrees
            }
            Some(_) => false,
        };
        let same_preview = matches!(
            refresh.preview,
            None | Some(Ok(worker::QuietPreview::Unchanged))
        );
        same_working && same_history && same_preview
    }

    /// Applies a quiet read asked for as the `requested`th working-state
    /// read (`request_status_read`), and returns whether the working
    /// entries it brought differ from those shown. `from_write` says a
    /// write asked for it as it ended.
    fn apply_quiet_refresh(
        &mut self,
        refresh: worker::QuietRefresh,
        from_write: bool,
        requested: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let found = match &refresh.snapshot {
            Some(Ok(worker::QuietHistory::Refreshed(snapshot))) => Some(&snapshot.repository),
            Some(Ok(worker::QuietHistory::Retained { metadata, .. })) => Some(&metadata.repository),
            _ => None,
        };
        // Another repository than the shown status and remotes were read
        // from, as a re-clone at the tab's path leaves, is a fresh capture,
        // as an accepted Refresh's is (`shows_another_repository`): what was
        // read of the other goes, History's rows and selection with it, and
        // the found one's status is read explicitly, since this read's went
        // through the window's handle by path and may hold either's.
        let replaced = found.is_some_and(|found| self.shows_another_repository(found));
        if let Some(found) = found {
            self.repository_tabs.follow_read_repository(found);
        }
        // A fresh capture whose found repository lacks History's scope
        // reads All history in place, as an Open with a vanished scope does
        // ([`Self::read_all_history_in_place`]).
        let mut read_history = false;
        let entries = if replaced {
            self.forget_captured_history(window, cx);
            true
        } else {
            self.apply_worktree_state(refresh.working, true, from_write, requested, window, cx)
        };
        if let Some(snapshot) = refresh.snapshot {
            match snapshot {
                // Installed whole wherever History stood, as an Open's
                // snapshot is: the rows it would keep below the newest, or
                // beside a search, went with the replaced repository.
                Ok(worker::QuietHistory::Refreshed(snapshot)) if replaced => {
                    self.history_updates
                        .clear_scope_error(&mut self.operation_error);
                    self.history_updates.captured(&snapshot);
                    self.install_current_history(snapshot, true, window, cx);
                }
                Ok(worker::QuietHistory::Refreshed(snapshot)) => {
                    self.apply_quiet_snapshot(snapshot, window, cx)
                }
                Ok(worker::QuietHistory::Retained {
                    metadata,
                    error,
                    summary,
                    vanished,
                }) => {
                    // A fresh capture's rows went with the replaced
                    // repository, and so does the traversal they were read
                    // at: kept, Older stayed offered under the empty list
                    // and asked the found repository for the replaced one's
                    // next page, and the tab saved the replaced pinned tips.
                    // No history of the found one is installed here, so it
                    // reads All history, as an Open with a vanished scope
                    // does, and says so as that lands: "The displayed
                    // history is retained" stood over an empty History. The
                    // tab's own repository keeps its rows, their paging and
                    // that banner.
                    if replaced {
                        self.history_paging = Default::default();
                        read_history = true;
                    }
                    self.refs = metadata.refs;
                    self.branches = metadata.branches;
                    self.worktrees = metadata.worktrees;
                    self.repository = Some(metadata.repository);
                    self.rebuild_navigation(cx);
                    if !replaced {
                        self.history_updates.report_scope_error(
                            &error,
                            summary.as_deref(),
                            vanished.as_ref(),
                            &mut self.operation_error,
                        );
                    }
                }
                Err(error) => {
                    self.operation_error.get_or_insert_with(|| {
                        crate::views::OperationFailure::of_read("History scope changed", &error)
                    });
                }
            }
        }
        if replaced {
            self.seed_navigation_folders();
            self.rebuild_navigation(cx);
            // The comparison read for the other's row goes with its rows.
            self.refresh_worktree(window, cx);
            // A commit's comparison was the other's: Compare goes back to
            // History, as an Open's does, once the status read holds quiet
            // reads off. A query typed in History's search runs again over
            // the found repository's history, as after an Open.
            if self.mode == WorkspaceMode::Compare && self.page == AppPage::Repository {
                self.back_to_history(window, cx);
            } else if self.mode == WorkspaceMode::Compare {
                self.mode = WorkspaceMode::History;
                self.sidebar = self.history_sidebar;
            }
            if read_history {
                // After Back, which ends the read it would start: its
                // landing selects and searches as an Open's does.
                self.read_all_history_in_place(window, cx);
                return entries;
            }
            self.select_newest_as_an_open_does(window, cx);
            if !self.search.read(cx).value().is_empty() {
                self.history_query_changed(window, cx);
            }
            return entries;
        }
        if self.mode == WorkspaceMode::Working
            && let Some(preview) = refresh.preview
        {
            match preview {
                Ok(worker::QuietPreview::Unchanged) => {}
                Ok(worker::QuietPreview::Absent) => {
                    // Keep an actively edited resolution available for review.
                    if self.has_active_conflict_draft() {
                        self.operation_notice = Some("The conflict changed outside GitTurtle. Your resolution draft is retained; refresh to review current files.".into());
                    } else {
                        self.clear_preview();
                        self.files.clear();
                        self.file_paths.reset();
                        self.selected_file = None;
                    }
                }
                Ok(worker::QuietPreview::Changed { file, content }) => {
                    if self.has_active_conflict_draft() {
                        self.operation_notice = Some("The working file changed outside GitTurtle. Your resolution draft is retained; refresh to review the latest version.".into());
                    } else {
                        self.replace_quiet_preview(file, content, window, cx);
                    }
                }
                Err(error) => {
                    self.operation_error.get_or_insert_with(|| {
                        crate::views::OperationFailure::of_read("Updated comparison", &error)
                    });
                }
            }
        }
        entries
    }

    fn has_active_conflict_draft(&self) -> bool {
        let Some(Content::Conflict(presentation)) = self.content.as_deref() else {
            return false;
        };
        self.path.as_ref().is_some_and(|path| {
            self.conflict_drafts
                .contains_key(&(path.clone(), presentation.snapshot.path.clone()))
        })
    }

    fn replace_quiet_preview(
        &mut self,
        file: FileChange,
        content: Arc<Content>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.review = crate::text_review::State::default();
        let same_text = matches!(self.content.as_deref(), Some(Content::Text { .. }))
            && matches!(content.as_ref(), Content::Text { .. });
        if same_text {
            if let Content::Text {
                patch,
                old,
                new,
                presentation,
                partial,
                split,
                ..
            } = content.as_ref()
            {
                if let Some(editor) = &self.patch_editor {
                    text::refresh_editor(
                        editor,
                        patch,
                        self.patch_decoration
                            .as_ref()
                            .map(|collection| (collection, presentation.as_ref())),
                        window,
                        cx,
                    );
                }
                if let Some(editor) = &self.before_editor {
                    text::refresh_editor(editor, old, None, window, cx);
                }
                if let Some(editor) = &self.after_editor {
                    text::refresh_editor(editor, new, None, window, cx);
                }
                if let Some(view) = &self.patch_view {
                    view.update(cx, |view, cx| {
                        view.refresh(presentation, partial.clone(), cx)
                    });
                }
                if let Some(view) = &self.split_view {
                    view.update(cx, |view, cx| view.refresh(Arc::clone(split), window, cx));
                }
            }
        } else {
            self.clear_preview();
        }
        self.files = vec![file];
        self.refresh_file_filter(window, cx);
        self.selected_file = Some(0);
        if let Content::Images { old, new } = content.as_ref() {
            self.images = [old.render.clone(), new.render.clone()];
        }
        self.content = Some(content);
        if let (
            Some(view),
            Some(Content::Text {
                markdown: Some(markdown),
                ..
            }),
        ) = (&self.markdown_view, self.content.as_deref())
        {
            view.update(cx, |view, cx| view.replace(markdown.clone(), cx));
        }
        // A passive read: a conflict's view shows without taking focus.
        if self.page == AppPage::Repository {
            self.prepare_editor(false, window, cx);
        }
    }
}

/// What a quiet read's landing, or the watcher's setup, can change that the
/// window draws besides the state a read brings
/// ([`GitTurtle::shows_quiet_refresh`]), read as the reply lands and again
/// once it is applied: a reply that changes none of it, nor that state,
/// draws no frame. Each reading is a handful of words and a hash of a few
/// short texts, so a landing reads nothing in proportion to the history,
/// its references or the working tree, and allocates nothing.
#[derive(PartialEq)]
struct QuietDrawn {
    /// The differing histories quiet reads installed
    /// (`history_updates::State::installed`), in place of History's rows,
    /// graph and page.
    history: u64,
    history_offset: Point<Pixels>,
    selected_commit: Option<usize>,
    graph_offset: usize,
    cue: Option<history_updates::Update>,
    /// The status line, the operation's error and notice and the watch's
    /// warning, hashed.
    text: u64,
    watch_failed: bool,
    travel: std::mem::Discriminant<motion::RowTravel>,
    rows_pending: bool,
    /// Whether writes are refused while the reload a write asked for runs.
    unsettled: bool,
    /// Whether Changes says its working state is being read, which it says
    /// only while it shows no status of its own.
    reading: bool,
}
impl QuietDrawn {
    fn read(app: &GitTurtle) -> Self {
        use std::hash::{Hash, Hasher};
        let mut text = std::collections::hash_map::DefaultHasher::new();
        (
            &app.status,
            app.operation_error.as_deref(),
            &app.operation_notice,
            &app.automatic.watch_warning,
        )
            .hash(&mut text);
        let unshown = app.work_status.is_none()
            || app.working_filter_waiting()
            || app.working_paths.error.is_some();
        Self {
            history: app.history_updates.installed,
            history_offset: app.history_scroll.0.borrow().base_handle.offset(),
            selected_commit: app.selected_commit,
            graph_offset: app.graph_offset,
            cue: app.history_updates.pending,
            text: text.finish(),
            watch_failed: app.automatic.watch_failed,
            travel: std::mem::discriminant(&app.working_travel),
            rows_pending: app.working_paths.pending,
            unsettled: app.write_unsettled(),
            reading: unshown && app.working_read_pending(),
        }
    }
}

fn reply_is_current(
    current_path: Option<&PathBuf>,
    requested_path: &PathBuf,
    current_generation: u64,
    requested_generation: u64,
    current_work_generation: u64,
    requested_work_generation: u64,
) -> bool {
    current_path == Some(requested_path)
        && current_generation == requested_generation
        && current_work_generation == requested_work_generation
}

pub(super) fn reanchor_offset(offset: f32, previous: usize, next: usize, row_height: f32) -> f32 {
    (offset + (previous as f32 - next as f32) * row_height).min(0.)
}

#[cfg(test)]
mod tests {
    use super::*;
    // What `#[gpui::test]` expands to, not the glob's `gpui::test`.
    use core::prelude::v1::test;
    #[::core::prelude::v1::test]
    fn quiet_reads_defer_writes_foreground_reads_and_hidden_pages() {
        let state = State {
            pending: LocalChange {
                worktree: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(state.can_start(true, false, false, false));
        assert!(!state.can_start(false, false, false, false));
        assert!(!state.can_start(true, true, false, false));
        assert!(!state.can_start(true, false, true, false));
        assert!(!state.can_start(true, false, false, true));
        assert!(!State::default().can_start(true, false, false, false));
    }
    /// A test's hold on quiet reads outlasts a reset, as opening another
    /// repository or subscribing again (`GitTurtle::watch_through`) resets
    /// the state: the hold is the held write's, and only the test lifts it.
    #[::core::prelude::v1::test]
    fn a_reset_keeps_a_tests_hold_on_quiet_reads() {
        let worktree = || LocalChange {
            worktree: true,
            ..Default::default()
        };
        let mut state = State::default();
        state.hold_quiet_reads(true);
        state.reset();
        state.pending.merge(worktree());
        assert!(
            !state.can_start(true, false, false, false),
            "the hold outlasts the reset"
        );
        state.hold_quiet_reads(false);
        assert!(
            state.can_start(true, false, false, false),
            "lifted, it starts"
        );
    }
    #[::core::prelude::v1::test]
    fn late_results_require_repository_and_both_read_generations() {
        let path = PathBuf::from("/worktree");
        assert!(reply_is_current(Some(&path), &path, 8, 8, 5, 5));
        assert!(!reply_is_current(
            Some(&PathBuf::from("/other")),
            &path,
            8,
            8,
            5,
            5
        ));
        assert!(!reply_is_current(Some(&path), &path, 9, 8, 5, 5));
        assert!(!reply_is_current(Some(&path), &path, 8, 8, 6, 5));
    }
    /// A test's app subscribes to its repository as the app does, with no
    /// warning, but through the inert backend, holding none of the host's
    /// inotify instances: each app a suite built held one, so concurrent
    /// suites exhausted the per-user budget and every app opened then showed
    /// "Some changes need Refresh", failing tests about the status bar.
    #[gpui::test]
    async fn a_test_app_watches_without_the_hosts_inotify_budget(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        // The subscription is made on the operation thread.
        let watched = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.automatic.watcher.is_some())
        };
        for _ in 0..400 {
            cx.run_until_parked();
            if watched(cx) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.read_with(cx, |app, _| {
            let watcher = app.automatic.watcher.as_ref();
            assert!(
                watcher.is_some_and(LocalWatcher::is_inert),
                "the repository is watched without a native watch"
            );
            assert_eq!(app.automatic.watch_warning(), None);
        });
    }

    #[::core::prelude::v1::test]
    fn rows_inserted_above_keep_the_same_fractional_scroll_anchor() {
        assert_eq!(reanchor_offset(-345., 10, 12, 34.), -413.);
        assert_eq!(reanchor_offset(-345., 10, 8, 34.), -277.);
        assert_eq!(reanchor_offset(-5., 0, 0, 34.), -5.);
        assert_eq!(reanchor_offset(-5., 10, 0, 34.), 0.);
    }
}
