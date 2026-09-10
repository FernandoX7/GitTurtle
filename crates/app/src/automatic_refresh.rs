//! Local refresh scheduling and application without resetting browsing state.

use crate::*;
use futures::StreamExt;
use local_refresh::{LocalChange, LocalChanges, LocalWatcher, WatchRoots};

#[derive(Default)]
pub struct State {
    watcher: Option<LocalWatcher>,
    roots: Option<WatchRoots>,
    watch_task: Option<Task<()>>,
    watch_failed: bool,
    events_task: Option<Task<()>>,
    task: Option<Task<()>>,
    pending: LocalChange,
    inflight: Option<LocalChange>,
    epoch: u64,
    pub retained_commit: Option<usize>,
}

impl State {
    pub fn reset(&mut self) {
        let epoch = self.epoch.wrapping_add(1);
        *self = Self {
            epoch,
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
        let Some(repo) = self.repository.clone() else {
            return;
        };
        if self.automatic.watch_task.is_some() || (!force && self.automatic.watcher.is_some()) {
            return;
        }
        let path = repo.path().to_owned();
        let previous = if self.automatic.watch_failed {
            None
        } else {
            self.automatic.roots.clone()
        };
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
                Some(local_refresh::watch(roots.clone())?)
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
                        this.automatic.watch_failed = false;
                        if let Some((watcher, changes)) = subscription {
                            this.automatic.watcher = Some(watcher);
                            this.listen_for_local_changes(path, changes, window, cx);
                            // Close the small interval between the preceding
                            // snapshot and completed watcher registration.
                            this.queue_automatic_refresh(LocalChange { rescan: true, ..Default::default() }, window, cx);
                        }
                    },
                    Err(error) => {
                        this.automatic.watch_failed = true;
                        this.operation_error.get_or_insert_with(|| format!("Automatic refresh is unavailable: {error:#}. Use Refresh to read current local changes."));
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
        if let Some(error) = &change.error {
            self.automatic.watch_failed = true;
            self.operation_error.get_or_insert_with(|| {
                format!(
                    "Automatic refresh: {error}. Refresh manually if local changes are missing."
                )
            });
        }
        self.automatic.pending.merge(change);
        self.try_automatic_refresh(window, cx);
    }

    pub(super) fn cancel_automatic_read(&mut self) {
        if let Some(change) = self.automatic.inflight.take() {
            self.automatic.pending.merge(change);
            self.worker.cancel();
        }
        self.automatic.task = None;
        self.automatic.epoch = self.automatic.epoch.wrapping_add(1);
    }

    pub(super) fn try_automatic_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.automatic.can_start(
            self.page == AppPage::Repository,
            self.operation_busy.is_some(),
            self.loading.is_some()
                || self.history_search_pending()
                || self.file_history.is_active()
                || self.revision_inspection.is_active()
                || self.blame.is_visible(),
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
                            this.ensure_local_watcher(true, window, cx);
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
                    self.apply_quiet_snapshot(snapshot, cx)
                }
                Ok(worker::QuietHistory::Retained { metadata, error }) => {
                    self.refs = metadata.refs;
                    self.branches = metadata.branches;
                    self.worktrees = metadata.worktrees;
                    self.repository = Some(metadata.repository);
                    self.rebuild_navigation(cx);
                    self.operation_error.get_or_insert_with(|| format!("History scope changed: {error}. The displayed history is retained; choose a current branch to browse its history."));
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
        self.refresh_file_filter(cx);
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
        if self.page == AppPage::Repository {
            self.ensure_editor(window, cx);
        }
    }

    fn apply_quiet_snapshot(&mut self, snapshot: worker::Snapshot, cx: &mut Context<Self>) {
        let Some(snapshot) = self.retain_search_snapshot(snapshot, cx) else {
            return;
        };
        if self.history_paging.is_deep(self.visible.len()) {
            // Deep browsing remains pinned even when local refs move. Refresh
            // navigation metadata without silently replacing its loaded window.
            self.refs = snapshot.refs;
            self.branches = snapshot.branches;
            self.worktrees = snapshot.worktrees;
            self.repository = Some(snapshot.repository);
            self.rebuild_navigation(cx);
            return;
        }
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
            // Keep immutable inspector/Compare context when a ref was rewritten
            // or a page moved. The retained commit is outside the refreshed list.
            let index = self.commits.len();
            self.commits.push(selected);
            self.graph.push(graph::GraphRow::default());
            self.selected_commit = Some(index);
            self.automatic.retained_commit = Some(index);
            self.status = "Selected comparison retained outside the current history scope".into();
        }
        self.filter_history_retaining_scroll(cx);
        self.rebuild_navigation(cx);
        if let Some(anchor) = anchor
            && let Some(next_top) = self
                .visible
                .iter()
                .position(|index| self.commits[*index].oid == anchor)
        {
            self.history_scroll.0.borrow().base_handle.set_offset(point(
                offset.x,
                px(reanchor_offset(f32::from(offset.y), top, next_top, height)),
            ));
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
    #[::core::prelude::v1::test]
    fn rows_inserted_above_keep_the_same_fractional_scroll_anchor() {
        assert_eq!(reanchor_offset(-345., 10, 12, 34.), -413.);
        assert_eq!(reanchor_offset(-345., 10, 8, 34.), -277.);
        assert_eq!(reanchor_offset(-5., 0, 0, 34.), -5.);
        assert_eq!(reanchor_offset(-5., 10, 0, 34.), 0.);
    }
}
