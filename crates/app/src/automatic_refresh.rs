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
    /// Whether a rescan waits to run, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn rescan_queued(&self) -> bool {
        self.pending.rescan
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
    pub fn reset(&mut self) {
        let epoch = self.epoch.wrapping_add(1);
        *self = Self {
            epoch,
            backend: self.backend,
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
            Ok((roots, subscription))
        });
        self.automatic.watch_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Local watcher setup stopped")));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.path.as_ref() != Some(&path) { return; }
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
                    Err(error) => {
                        this.automatic.watch_failed = true;
                        this.automatic.watch_warning = Some(format!("Automatic refresh could not start.\n\n{error:#}\n\nRefresh reads current local state and retries the watcher."));
                    }
                }
                cx.notify();
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
        use gpui_kit::component::{WindowExt, dialog::DialogButtonProps};
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
                .button_props(DialogButtonProps::default().ok_text("Close"))
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
        let history = change.git || change.rescan;
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
                        this.apply_quiet_refresh(*refresh, window, cx);
                        // A failed native watch gets one local state read. A
                        // later event, manual Refresh, or regained focus may
                        // reconnect; do not spin on a persistent watch error.
                        if history && changed.error.is_none() {
                            this.check_local_watcher(true, false, window, cx);
                        }
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        this.operation_error
                            .get_or_insert_with(|| format!("Local refresh: {error:#}"));
                    }
                    Err(_) => {
                        this.automatic.pending.merge(changed);
                    }
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
    }

    fn apply_quiet_refresh(
        &mut self,
        refresh: worker::QuietRefresh,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_worktree_state(refresh.working, true, window, cx);
        if let Some(snapshot) = refresh.snapshot {
            match snapshot {
                Ok(worker::QuietHistory::Refreshed(snapshot)) => {
                    self.apply_quiet_snapshot(snapshot, window, cx)
                }
                Ok(worker::QuietHistory::Retained {
                    metadata,
                    error,
                    vanished,
                }) => {
                    self.refs = metadata.refs;
                    self.branches = metadata.branches;
                    self.worktrees = metadata.worktrees;
                    self.repository = Some(metadata.repository);
                    self.rebuild_navigation(cx);
                    self.history_updates.report_scope_error(
                        &error,
                        vanished.as_ref(),
                        &mut self.operation_error,
                    );
                }
                Err(error) => {
                    self.operation_error
                        .get_or_insert_with(|| format!("History scope changed: {error:#}"));
                }
            }
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
                    self.operation_error
                        .get_or_insert_with(|| format!("Updated comparison: {error:#}"));
                }
            }
        }
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
