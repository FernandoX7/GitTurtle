use crate::*;
use gitturtle_core::{ChangeArea, ChangeStatus, WriteCommand};
use gpui_kit::prelude::FluentBuilder;
use preferences::CommitDraft;

#[derive(Clone, Copy)]
pub(super) enum WorkingRow {
    Heading(ChangeArea, usize),
    File(usize, ChangeArea),
}

impl GitTurtle {
    pub(super) fn current_commit_draft(&self, cx: &App) -> CommitDraft {
        CommitDraft {
            title: self.commit_title.read(cx).value().to_string(),
            description: self.commit_message.read(cx).value().to_string(),
        }
    }

    pub(super) fn persist_commit_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(worktree) = self.draft_repository.clone() else {
            return;
        };
        let draft = self.current_commit_draft(cx);
        self.save_commit_draft(worktree, draft, window, cx);
        cx.notify();
    }

    fn save_commit_draft(
        &mut self,
        worktree: PathBuf,
        draft: CommitDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if draft.is_empty() {
            self.commit_drafts.remove(&worktree);
        } else {
            self.commit_drafts.insert(worktree.clone(), draft.clone());
        }
        let Some(response) = self
            .draft_saver
            .queue(&self.preferences_writer, worktree, draft)
        else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Draft save ended without a result")));
            let _ = this.update_in(cx, |this, _, cx| {
                this.draft_save_error = result.err().map(|error| format!("{error:#}"));
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn restore_commit_draft(
        &mut self,
        worktree: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft_repository.as_ref() == Some(&worktree) {
            return;
        }
        self.persist_commit_draft(window, cx);
        let draft = self
            .commit_drafts
            .get(&worktree)
            .cloned()
            .unwrap_or_default();
        self.draft_repository = Some(worktree);
        self.commit_title
            .update(cx, |input, cx| input.set_value(draft.title, window, cx));
        self.commit_message.update(cx, |input, cx| {
            input.set_value(draft.description, window, cx)
        });
    }

    fn clear_committed_draft(
        &mut self,
        worktree: &PathBuf,
        submitted_message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft_repository.as_ref() == Some(worktree) {
            if self.current_commit_draft(cx).message() != submitted_message {
                return;
            }
            self.commit_title
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.commit_message
                .update(cx, |input, cx| input.set_value("", window, cx));
        } else if self
            .commit_drafts
            .get(worktree)
            .is_none_or(|draft| draft.message() != submitted_message)
        {
            return;
        }
        self.save_commit_draft(worktree.clone(), CommitDraft::default(), window, cx);
    }

    pub(super) fn project_event(
        &mut self,
        event: &projects::ProjectEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        match event {
            projects::ProjectEvent::Open(path) => self.open(path.clone(), None, window, cx),
            projects::ProjectEvent::Back => {
                self.page = AppPage::Repository;
                self.resume_file_history(window, cx);
                if matches!(self.mode, WorkspaceMode::Compare | WorkspaceMode::Working) {
                    self.ensure_editor(window, cx);
                    window.focus(&self.file_focus, cx);
                } else {
                    window.focus(&self.focus, cx);
                }
                self.restore_page_return_focus(window, cx);
                self.try_automatic_refresh(window, cx);
                cx.notify();
            }
            projects::ProjectEvent::Clone {
                source,
                destination,
            } => {
                let source = source.clone();
                let destination = destination.clone();
                self.create_project(
                    "Cloning repository…",
                    move || GitRepository::clone_repository(&source, &destination),
                    window,
                    cx,
                );
            }
            projects::ProjectEvent::Create {
                destination,
                branch,
            } => {
                let destination = destination.clone();
                let branch = branch.clone();
                self.create_project(
                    "Creating repository…",
                    move || GitRepository::init(&destination, &branch),
                    window,
                    cx,
                );
            }
        }
    }

    fn create_project(
        &mut self,
        label: &'static str,
        operation: impl FnOnce() -> anyhow::Result<GitRepository> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.operation_busy = Some(label);
        self.operation_error = None;
        self.operation_notice = None;
        self.hub.update(cx, |hub, cx| hub.set_busy(true, cx));
        let response = self.operations.submit(operation);
        self.operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Repository operation stopped without a result. Inspect the destination before retrying.")));
            let _ = this.update_in(cx, |this, window, cx| {
                this.operation_busy = None;
                this.hub.update(cx, |hub, cx| hub.set_busy(false, cx));
                match result {
                    Ok(repo) => {
                        let message = format!("{} · {}", if label.starts_with("Cloning") { "Repository cloned" } else { "Repository created" }, repo.name());
                        this.limit = 500; this.open(repo.path().to_owned(), None, window, cx);
                        this.operation_notice = Some(message);
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        this.hub.update(cx, |hub, cx| hub.set_error(Some(message.clone()), cx));
                        this.operation_error = Some(message);
                        this.operation_notice = None;
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn show_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_branch_action();
        self.cancel_recovery_read();
        if self.operation_busy.is_some() {
            return;
        }
        self.capture_page_return_focus(window, cx);
        self.page = AppPage::Projects;
        window.focus(&self.app_focus, cx);
        self.hub.update(cx, |hub, cx| {
            hub.set_busy(false, cx);
            hub.set_error(None, cx);
            hub.set_can_go_back(self.repository.is_some(), cx);
        });
        cx.notify();
    }

    pub(super) fn remember_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let response = self.preferences_writer.submit(move || {
            let mut prefs = Preferences::load();
            prefs.remember_repository(&path)?;
            Ok(prefs)
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(result) = response.await {
                let _ = this.update_in(cx, |this, _, cx| match result {
                    Ok(prefs) => this
                        .hub
                        .update(cx, |hub, cx| hub.set_recent(prefs.recent_repositories, cx)),
                    Err(error) => {
                        this.operation_error =
                            Some(format!("Could not save recent projects: {error:#}"))
                    }
                });
            }
        })
        .detach();
    }

    /// The UI generation rejects late replies, while the worker cancellation
    /// also bounds obsolete computation when no replacement preview is needed.
    pub(super) fn invalidate_read(&mut self) {
        self.pause_history_search_for_read();
        self.cancel_automatic_read();
        self.generation = self.generation.wrapping_add(1);
        self.worker.cancel();
        self.task = None;
        self.loading = None;
    }

    pub(super) fn refresh_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.repository.clone() else {
            return;
        };
        self.cancel_automatic_read();
        self.ensure_local_watcher(true, window, cx);
        if self.mode == WorkspaceMode::Working {
            self.invalidate_read();
            self.clear_preview();
            self.files.clear();
            self.selected_file = None;
        }
        self.work_generation = self.work_generation.wrapping_add(1);
        let generation = self.work_generation;
        let path = repo.path().to_owned();
        self.status_task = None;
        let response = self
            .operations
            .submit_read(move || worker::WorkingState::read(&repo));
        self.status_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Working-copy refresh ended without a result"
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.work_generation != generation || this.path.as_ref() != Some(&path) {
                    return;
                }
                this.status_task = None;
                match result {
                    Ok(state) => this.apply_worktree_state(state, false, window, cx),
                    Err(error) => {
                        this.work_status = None;
                        this.integration_state = None;
                        this.working_rows.clear();
                        this.working_selected = None;
                        if this.mode == WorkspaceMode::Working {
                            this.invalidate_read();
                            this.clear_preview();
                            this.files.clear();
                            this.selected_file = None;
                        }
                        this.operation_error
                            .get_or_insert_with(|| format!("Working tree: {error:#}"));
                    }
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn apply_worktree_state(
        &mut self,
        state: worker::WorkingState,
        quiet: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let worker::WorkingState {
            status,
            profile,
            remotes,
            operation,
        } = state;
        let path = self.path.clone();
        self.integration_state = match operation {
            Ok(state) => state,
            Err(error) => {
                self.operation_error
                    .get_or_insert_with(|| format!("Operation state: {error:#}"));
                None
            }
        };
        let preferred = self.working_selected.and_then(|(index, area)| {
            self.work_status
                .as_ref()?
                .entries
                .get(index)
                .map(|entry| (entry.path.clone(), area))
        });
        if !quiet {
            self.conflict_drafts.retain(|(repository, file), _| {
                Some(repository) != path.as_ref()
                    || status
                        .entries
                        .iter()
                        .any(|entry| entry.conflicted && &entry.path == file)
            });
        }
        let entries_changed = self
            .work_status
            .as_ref()
            .is_none_or(|previous| previous.entries != status.entries);
        if entries_changed {
            let offset = self.working_scroll.0.borrow().base_handle.offset();
            let height = self.settings.density.file_row_height();
            let top = ((-f32::from(offset.y)) / height).max(0.) as usize;
            let anchor = self.working_rows.get(top).and_then(|row| match row {
                WorkingRow::Heading(area, _) => Some((None, *area)),
                WorkingRow::File(index, area) => self
                    .work_status
                    .as_ref()?
                    .entries
                    .get(*index)
                    .map(|entry| (Some(entry.path.clone()), *area)),
            });
            self.working_rows.clear();
            for area in [ChangeArea::Staged, ChangeArea::Unstaged] {
                let indices: Vec<_> = status
                    .entries
                    .iter()
                    .enumerate()
                    .filter_map(|(i, entry)| {
                        let present = if area == ChangeArea::Staged {
                            entry.staged.is_some()
                        } else {
                            entry.unstaged.is_some() || entry.untracked || entry.conflicted
                        };
                        present.then_some(i)
                    })
                    .collect();
                self.working_rows
                    .push(WorkingRow::Heading(area, indices.len()));
                self.working_rows
                    .extend(indices.into_iter().map(|i| WorkingRow::File(i, area)));
            }
            self.working_selected = preferred.as_ref().and_then(|(path, area)| {
                status.entries.iter().enumerate().find_map(|(i, entry)| {
                    if &entry.path != path {
                        return None;
                    }
                    retained_area(
                        *area,
                        entry.staged.is_some(),
                        entry.unstaged.is_some() || entry.untracked || entry.conflicted,
                    )
                    .map(|area| (i, area))
                })
            });
            if quiet
                && let Some((anchor_path, anchor_area)) = anchor
                && let Some(next_top) = self.working_rows.iter().position(|row| match row {
                    WorkingRow::Heading(area, _) => anchor_path.is_none() && *area == anchor_area,
                    WorkingRow::File(index, area) => {
                        *area == anchor_area
                            && anchor_path.as_ref() == Some(&status.entries[*index].path)
                    }
                })
            {
                self.working_scroll.0.borrow().base_handle.set_offset(point(
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
        let remotes = match remotes {
            Ok(remotes) => remotes,
            Err(error) => {
                self.operation_error
                    .get_or_insert_with(|| format!("Remote configuration: {error:#}"));
                Vec::new()
            }
        };
        let branch_changed = self.work_status.as_ref().is_none_or(|previous| {
            previous.branch != status.branch || previous.upstream != status.upstream
        });
        let (default_remote, default_branch) = remote_defaults(&status, &remotes);
        if branch_changed || self.remote_name.read(cx).value().is_empty() {
            self.remote_name
                .update(cx, |input, cx| input.set_value(default_remote, window, cx));
        }
        if branch_changed || self.remote_branch.read(cx).value().is_empty() {
            self.remote_branch
                .update(cx, |input, cx| input.set_value(default_branch, window, cx));
        }
        self.profile = match profile {
            Ok(profile) => Some(profile),
            Err(error) => {
                self.operation_error
                    .get_or_insert_with(|| format!("Git identity: {error:#}"));
                None
            }
        };
        self.work_status = Some(status);
        self.remotes = remotes;
        if !quiet && self.mode == WorkspaceMode::Working {
            self.invalidate_read();
            if let Some((index, area)) = self.working_selected {
                self.select_working(index, area, window, cx);
                if let Some(position) = self.working_rows.iter().position(
                    |row| matches!(row, WorkingRow::File(i, a) if *i == index && *a == area),
                ) {
                    self.working_scroll
                        .scroll_to_item(position, ScrollStrategy::Center);
                }
            } else {
                self.clear_preview();
                self.files.clear();
                self.selected_file = None;
            }
        }
    }

    pub(super) fn show_working(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_file_history(window, cx);
        if self.operation_busy.is_some() {
            return;
        }
        if self.repository.is_none() {
            self.show_projects(window, cx);
            return;
        }
        if self.mode != WorkspaceMode::Working {
            if self.mode == WorkspaceMode::History {
                self.history_sidebar = self.sidebar;
            }
            self.retained_history_files =
                Some((std::mem::take(&mut self.files), self.selected_file.take()));
            self.invalidate_read();
            self.clear_preview();
        }
        self.mode = WorkspaceMode::Working;
        self.page = AppPage::Repository;
        self.sidebar = false;
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        self.refresh_worktree(window, cx);
        cx.notify();
    }

    pub(super) fn select_working(
        &mut self,
        index: usize,
        area: ChangeArea,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        let (Some(repo), Some(status)) = (&self.repository, &self.work_status) else {
            return;
        };
        let Some(entry) = status.entries.get(index).cloned() else {
            return;
        };
        let available = if area == ChangeArea::Staged {
            entry.staged.is_some()
        } else {
            entry.unstaged.is_some() || entry.untracked || entry.conflicted
        };
        if !available {
            self.invalidate_read();
            self.working_selected = None;
            self.clear_preview();
            self.files.clear();
            self.selected_file = None;
            return;
        }
        let repo = repo.clone();
        self.mode = WorkspaceMode::Working;
        self.working_selected = Some((index, area));
        self.clear_preview();
        self.files.clear();
        self.selected_file = None;
        self.zoom = 0.;
        self.image_scroll.set_offset(point(px(0.), px(0.)));
        self.interaction_started = Some(Instant::now());
        window.focus(&self.file_focus, cx);
        self.request(
            Job::WorkingPreview { repo, entry, area },
            "Reading working comparison…",
            window,
            cx,
        );
    }

    pub(super) fn move_working_selection(
        &mut self,
        direction: i32,
        edge: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files: Vec<_> = self
            .working_rows
            .iter()
            .enumerate()
            .filter_map(|(position, row)| match row {
                WorkingRow::File(i, area) => Some((position, *i, *area)),
                _ => None,
            })
            .collect();
        if files.is_empty() {
            return;
        }
        let current = files
            .iter()
            .position(|(_, i, area)| Some((*i, *area)) == self.working_selected)
            .unwrap_or(0);
        let next = if edge {
            if direction < 0 { 0 } else { files.len() - 1 }
        } else {
            (current as i32 + direction).clamp(0, files.len() as i32 - 1) as usize
        };
        let (position, index, area) = files[next];
        self.select_working(index, area, window, cx);
        self.working_scroll
            .scroll_to_item(position, ScrollStrategy::Center);
    }

    pub(super) fn write(
        &mut self,
        command: WriteCommand,
        label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        self.close_file_history(window, cx);
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let path = repo.path().to_owned();
        let ending_integration = matches!(
            &command,
            WriteCommand::Integration(
                gitturtle_core::IntegrationCommand::Abort { .. }
                    | gitturtle_core::IntegrationCommand::Quit { .. }
            )
        );
        let resolved_path = match &command {
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                expected,
                ..
            }) => Some(expected.path.clone()),
            _ => None,
        };
        let submitted_message = match &command {
            WriteCommand::Commit { message } => Some(message.clone()),
            _ => None,
        };
        let submitted_branch = match &command {
            WriteCommand::Checkout { branch } => Some(branch.clone()),
            WriteCommand::CreateBranch { name, .. } => Some(name.clone()),
            _ => None,
        };
        let success_notice = match &command {
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                expected,
                ..
            }) => Some(format!("Resolved and staged {}", expected.path.display())),
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Continue {
                expected,
            }) => Some(format!(
                "Continued {} on {}",
                expected.kind.label().to_lowercase(),
                expected.branch
            )),
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Abort { expected }) => {
                Some(format!(
                    "Aborted {} on {}",
                    expected.kind.label().to_lowercase(),
                    expected.branch
                ))
            }
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Quit { expected }) => {
                Some(format!(
                    "Stopped {} and kept the current files",
                    expected.kind.label().to_lowercase()
                ))
            }
            WriteCommand::Stage { paths } => Some(match paths.as_slice() {
                [path] => format!("Staged {}", path.display()),
                _ => "Staged selected changes".into(),
            }),
            WriteCommand::Unstage { paths } => Some(match paths.as_slice() {
                [path] => format!("Unstaged {}", path.display()),
                _ => "Unstaged selected changes".into(),
            }),
            WriteCommand::StageAll => Some("Staged all working changes".into()),
            WriteCommand::UnstageAll => Some("Unstaged all changes".into()),
            WriteCommand::Checkout { branch } => Some(format!("Switched to {branch}")),
            WriteCommand::CreateBranch { name, .. } => {
                Some(format!("Created and switched to {name}"))
            }
            WriteCommand::Fetch { remote } => Some(format!("Fetched {remote}")),
            WriteCommand::Pull { remote, branch } => Some(format!("Pulled {remote}/{branch}")),
            WriteCommand::Push {
                remote,
                local_branch,
                remote_branch,
            } => Some(format!("Pushed {local_branch} to {remote}/{remote_branch}")),
            _ => None,
        };
        let recovery = if let WriteCommand::Recovery(command) = &command {
            Some(Arc::clone(command))
        } else {
            None
        };
        let refresh_history = matches!(
            &command,
            WriteCommand::Commit { .. }
                | WriteCommand::Branch(_)
                | WriteCommand::Checkout { .. }
                | WriteCommand::CreateBranch { .. }
                | WriteCommand::Fetch { .. }
                | WriteCommand::Pull { .. }
                | WriteCommand::Push { .. }
        ) || matches!(&command, WriteCommand::Integration(operation) if !matches!(operation, gitturtle_core::IntegrationCommand::Resolve { .. }))
            || matches!(&command, WriteCommand::Recovery(command) if matches!(command.as_ref(), gitturtle_core::RecoveryCommand::Amend { .. } | gitturtle_core::RecoveryCommand::Undo { .. } | gitturtle_core::RecoveryCommand::Revert { .. } | gitturtle_core::RecoveryCommand::CherryPick { .. }));
        self.operation_busy = Some(label);
        self.operation_error = None;
        self.operation_notice = None;
        self.work_generation = self.work_generation.wrapping_add(1);
        self.status_task = None;
        if self.mode == WorkspaceMode::Working {
            self.invalidate_read();
            self.clear_preview();
            self.files.clear();
            self.selected_file = None;
        }
        let response = self.operations.submit(move || repo.execute(&command));
        self.operation_task = Some(cx.spawn_in(window, async move |this,cx| {
            let result=response.await.unwrap_or_else(|_|Err(anyhow::anyhow!("Operation ended without a result. Refresh before retrying; repository state may have changed.")));
            let _=this.update_in(cx,|this,window,cx| {
                this.operation_busy=None;
                let succeeded=result.is_ok();
                match result {
                    Ok(outcome) => {
                        let notice = if let (Some(oid), Some(message)) = (outcome.commit_oid.as_ref(), submitted_message.as_deref()) {
                            format!("Committed {} · {}", short_oid(oid), message.lines().next().unwrap_or_default())
                        } else if let Some(notice) = &success_notice {
                            notice.clone()
                        } else {
                            outcome.message.lines().find(|line| !line.trim().is_empty()).unwrap_or("Git operation completed").to_owned()
                        };
                        this.status = notice.clone();
                        this.operation_notice = Some(notice);
                    }
                    Err(error) => {
                        this.operation_notice = None;
                        this.operation_error = Some(format!("{error:#}"));
                    }
                }
                if let Some(command) = &recovery { this.finish_recovery_write(&path, command, succeeded, cx); }
                if succeeded && ending_integration { this.conflict_drafts.retain(|(repository, _), _| repository != &path); }
                if succeeded && let Some(resolved) = &resolved_path {
                    this.conflict_drafts.remove(&(path.clone(), resolved.clone()));
                }
                if succeeded && let Some(message) = &submitted_message {
                    this.clear_committed_draft(&path, message, window, cx);
                }
                if this.path.as_ref()==Some(&path) {
                    if succeeded && submitted_branch.as_deref().is_some_and(|branch| this.branch_name.read(cx).value().trim() == branch) {
                        this.branch_name.update(cx,|input,cx|input.set_value("",window,cx));
                    }
                    if refresh_history { this.refresh_after_write(path, window, cx); }
                    else { this.refresh_worktree(window,cx); }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn refresh_after_write(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == WorkspaceMode::Working {
            self.restore_commit = self
                .selected_commit
                .and_then(|index| self.commits.get(index).map(|commit| commit.oid.clone()));
            self.retained_history_files = None;
            self.request(
                Job::Open {
                    path,
                    scope: self.scope.as_ref().map(|scope| scope.1.clone()),
                    limit: self.limit,
                },
                "Refreshing repository…",
                window,
                cx,
            );
        } else {
            self.open(path, None, window, cx);
        }
    }

    pub(super) fn render_working_inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let staged = self.work_status.as_ref().map_or(0, |status| {
            status.entries.iter().filter(|e| e.staged.is_some()).count()
        });
        let total = self
            .work_status
            .as_ref()
            .map_or(0, |status| status.entries.len());
        let conflicted = self
            .work_status
            .as_ref()
            .is_some_and(|s| s.entries.iter().any(|e| e.conflicted));
        let identity_ready = self
            .profile
            .as_ref()
            .is_some_and(|p| !p.name.is_empty() && !p.email.is_empty());
        let busy = self.operation_busy.is_some();
        let refreshing = self.status_task.is_some();
        let status_ready = self.work_status.is_some();
        let branch = self
            .work_status
            .as_ref()
            .and_then(|s| s.branch.as_deref())
            .unwrap_or("detached HEAD");
        let summary_length = self.commit_title.read(cx).value().chars().count();
        let list = uniform_list(
            "working-files",
            self.working_rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|i| this.render_working_row(i, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.working_scroll);
        div()
            .size_full().flex().flex_col().bg(rgb(p.panel))
            .child(
                div().h(px(52.)).px_4().flex().items_center().gap_2()
                    .border_b_1().border_color(rgb(p.border))
                    .child(icon("commit", 17., p.accent))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Changes"))
                    .child(div().px_2().py_0p5().rounded(px(5.)).bg(rgb(p.hover))
                        .text_size(px(11.)).text_color(rgb(p.muted)).child(total.to_string()))
                    .child(div().flex_1())
                    .when(refreshing, |el| el.child(div().text_size(px(10.)).text_color(rgb(p.muted)).child("Refreshing…")))
                    .child(self.render_working_recovery_menu(cx))
                    .child(button("refresh-working", "", "refresh", false)
                        .accessibility_label("Refresh working changes").tooltip("Refresh working changes")
                        .disabled(busy || refreshing).on_click(cx.listener(|this, _, window, cx| this.refresh_worktree(window, cx)))),
            )
            .child(
                div().id("working-pane").role(Role::ListBox).aria_label("Working tree files")
                    .tab_stop(true).key_context("GitTurtleList").track_focus(&self.file_focus)
                    .flex_1().min_h_0().overflow_hidden()
                    .when(total > 0, |el| el.child(list))
                    .when(total == 0, |el| el.child(
                        div().size_full().flex().flex_col().items_center().justify_center().p_5().gap_2()
                            .child(div().size(px(40.)).rounded(px(12.))
                                .bg(rgb(if status_ready { p.added_background } else { p.hover }))
                                .flex().items_center().justify_center()
                                .child(icon(if status_ready { "check" } else if refreshing { "refresh" } else { "changes" },
                                    22., if status_ready { p.added } else { p.muted })))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM)
                                .child(if status_ready { "Working tree clean" } else if refreshing { "Reading working tree…" } else { "Changes unavailable" }))
                            .child(div().max_w(px(220.)).text_center().text_size(px(11.)).text_color(rgb(p.muted))
                                .child(if status_ready { "No staged changes or local edits. Your next changes will appear here." }
                                    else if refreshing { "Checking staged changes and local edits." }
                                    else { "Refresh to try reading this working tree again." })),
                    )),
            )
            .when(self.work_status.as_ref().is_none_or(|status| status.operation.is_none()), |el| el.child(
                div().flex_shrink_0().p_3().flex().flex_col().gap_2()
                    .bg(rgb(p.subtle)).border_t_1().border_color(rgb(p.border))
                    .child(div().flex().items_center().gap_2()
                        .child(icon("commit", 16., p.accent))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Create a commit"))
                        .child(div().flex_1())
                        .child(div().px_1p5().py_0p5().rounded(px(4.))
                            .bg(rgb(if staged > 0 { p.added_background } else { p.hover }))
                            .text_size(px(10.)).text_color(rgb(if staged > 0 { p.added } else { p.muted }))
                            .child(format!("{staged} staged"))))
                    .child(div().flex().items_center().gap_1p5().min_w_0().text_size(px(11.))
                        .text_color(rgb(p.muted)).child(icon("branch", 13., p.muted))
                        .child(div().truncate().child(branch.to_owned())))
                    .child(div().flex().items_center().gap_2().text_size(px(11.))
                        .child(div().flex_1().font_weight(FontWeight::MEDIUM).child("Title"))
                        .child(div().text_color(rgb(if summary_length > 72 { p.warning } else { p.muted }))
                            .child(format!("{summary_length}/72 suggested"))))
                    .child(Input::new(&self.commit_title).readonly(busy).aria_label("Commit title").text_size(px(12.)))
                    .when(summary_length > 72, |el| el.child(
                        div().text_size(px(10.)).text_color(rgb(p.muted)).child("A shorter title is easier to scan. This title is still valid."),
                    ))
                    .child(div().flex().items_center().gap_1().text_size(px(11.))
                        .child(div().font_weight(FontWeight::MEDIUM).child("Description"))
                        .child(div().text_color(rgb(p.muted)).child("· optional")))
                    .child(Textarea::new(&self.commit_message).readonly(busy)
                        .aria_label("Commit description, optional")
                        .h(px(if self.settings.density == appearance::Density::Compact { 100. } else { 128. }))
                        .text_size(px(12.)))
                    .child(div().flex().items_center().gap_2().text_size(px(10.)).text_color(rgb(p.muted))
                        .child(div().flex_1().child(if staged == 0 { "Stage files to include in your commit" } else { "Only staged changes will be committed" })))
                    .when_some(self.draft_save_error.as_ref(), |el, error| el.child(
                        div().flex().flex_col().gap_1().text_size(px(10.)).text_color(rgb(p.warning))
                            .child("Draft could not be saved on this computer. Keep the app open and retry.")
                            .child(button("retry-commit-draft", "Retry saving draft", "refresh", false)
                                .tooltip(error.clone())
                                .on_click(cx.listener(|this, _, window, cx| this.persist_commit_draft(window, cx)))),
                    ))
                    .when(!identity_ready && status_ready, |el| el.child(
                        button("configure-identity", "Set your commit identity", "", false)
                            .on_click(cx.listener(|this, _, window, cx| this.show_settings(window, cx))),
                    ))
                    .when(conflicted, |el| el.child(
                        div().flex().items_center().gap_2().text_size(px(11.)).text_color(rgb(p.warning))
                            .child(icon("file-conflict", 15., p.warning))
                            .child("Resolve conflicted files before committing."),
                    ))
                    .child(
                        Button::new("commit-staged").primary().w_full().h(px(36.))
                            .icon(Icon::default().path("icons/commit.svg").size(px(16.)))
                            .label(if busy {
                                "Working…".to_owned()
                            } else if !status_ready {
                                if refreshing { "Reading working changes…" } else { "Changes unavailable" }.to_owned()
                            } else if conflicted {
                                "Resolve conflicts to commit".to_owned()
                            } else if !identity_ready {
                                "Set your commit identity".to_owned()
                            } else if staged == 0 {
                                "Stage files to commit".to_owned()
                            } else if self.commit_title.read(cx).value().trim().is_empty() {
                                "Write a commit title".to_owned()
                            } else {
                                format!("Commit {staged} {}", if staged == 1 { "file" } else { "files" })
                            })
                            .disabled(busy || staged == 0 || conflicted || !identity_ready
                                || self.commit_title.read(cx).value().trim().is_empty())
                            .tooltip(format!("Commit staged changes to {branch}"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let message = this.current_commit_draft(cx).message();
                                this.write(WriteCommand::Commit { message }, "Creating commit…", window, cx);
                            })),
                    ),
            ))
            .into_any_element()
    }

    fn render_working_row(&self, position: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        match self.working_rows[position] {
            WorkingRow::Heading(area, count) => {
                let staged = area == ChangeArea::Staged;
                div()
                    .w_full()
                    .h(px(self.settings.density.file_row_height()))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.subtle))
                    .child(icon(
                        if staged { "check" } else { "code" },
                        15.,
                        if staged { p.added } else { p.modified },
                    ))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if staged { "Staged" } else { "Unstaged" }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(p.muted))
                            .child(count.to_string()),
                    )
                    .child(div().flex_1())
                    .child(
                        button(
                            if staged { "unstage-all" } else { "stage-all" },
                            if staged { "Unstage all" } else { "Stage all" },
                            if staged { "minus" } else { "plus" },
                            false,
                        )
                        .disabled(busy || count == 0)
                        .tooltip(if staged {
                            "Remove all staged changes from the next commit"
                        } else {
                            "Include all local changes in the next commit"
                        })
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.write(
                                    if staged {
                                        WriteCommand::UnstageAll
                                    } else {
                                        WriteCommand::StageAll
                                    },
                                    if staged {
                                        "Unstaging files…"
                                    } else {
                                        "Staging files…"
                                    },
                                    window,
                                    cx,
                                )
                            },
                        )),
                    )
                    .into_any_element()
            }
            WorkingRow::File(index, area) => {
                let entry = &self.work_status.as_ref().unwrap().entries[index];
                let paths = entry.paths();
                let name = entry
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let parent = entry
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .filter(|p| !p.is_empty())
                    .unwrap_or("Repository root".into());
                let active = self.working_selected == Some((index, area));
                let staged = area == ChangeArea::Staged;
                let path_detail = entry.original_path.as_ref().map_or_else(
                    || entry.path.display().to_string(),
                    |original| format!("{} → {}", original.display(), entry.path.display()),
                );
                let stage_label = format!(
                    "{} {}",
                    if staged { "Unstage" } else { "Stage" },
                    entry.path.display()
                );
                let change = if staged { entry.staged } else { entry.unstaged };
                let (symbol, status_label, color, background) = if entry.conflicted {
                    ("file-conflict", "Conflict", p.warning, p.hover)
                } else if entry.untracked || change == Some(ChangeStatus::Added) {
                    ("file-added", "New", p.added, p.added_background)
                } else {
                    match change.unwrap_or(ChangeStatus::Modified) {
                        ChangeStatus::Added => ("file-added", "New", p.added, p.added_background),
                        ChangeStatus::Modified => {
                            ("file-modified", "Modified", p.modified, p.hover)
                        }
                        ChangeStatus::Deleted => {
                            ("file-deleted", "Deleted", p.removed, p.removed_background)
                        }
                        ChangeStatus::Renamed => ("file-renamed", "Renamed", p.renamed, p.hover),
                        ChangeStatus::TypeChanged => ("file-type", "Type", p.warning, p.hover),
                    }
                };
                div()
                    .id((
                        if staged {
                            "staged-file"
                        } else {
                            "unstaged-file"
                        },
                        index,
                    ))
                    .role(Role::ListBoxOption)
                    .aria_label(format!(
                        "{} · {} · {}",
                        entry.path.display(),
                        status_label,
                        if staged { "Staged" } else { "Unstaged" }
                    ))
                    .aria_selected(active)
                    .h(px(self.settings.density.file_row_height()))
                    .w_full()
                    .pl(px(10.))
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgb(if active { p.selected } else { p.panel }))
                    .border_l_2()
                    .border_color(if active { rgb(p.accent) } else { rgba(0) })
                    .hover(move |s| s.bg(rgb(p.row_hover(active))))
                    .active(move |s| s.bg(rgb(p.selected)))
                    .cursor_pointer()
                    .child(
                        div()
                            .size(px(26.))
                            .flex_shrink_0()
                            .rounded(px(7.))
                            .bg(rgb(background))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(symbol, 16., color)),
                    )
                    .child(
                        div()
                            .id(("working-path", position))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .tooltip(move |window, cx| {
                                Tooltip::new(path_detail.clone()).build(window, cx)
                            })
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(10.))
                                    .text_color(rgb(p.muted))
                                    .child(parent),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.))
                            .text_color(rgb(color))
                            .child(status_label),
                    )
                    .child(
                        button(
                            (if staged { "unstage" } else { "stage" }, index),
                            "",
                            if staged { "minus" } else { "plus" },
                            false,
                        )
                        .accessibility_label(stage_label.clone())
                        .tooltip(stage_label)
                        .disabled(busy)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.write(
                                    if staged {
                                        WriteCommand::Unstage {
                                            paths: paths.clone(),
                                        }
                                    } else {
                                        WriteCommand::Stage {
                                            paths: paths.clone(),
                                        }
                                    },
                                    if staged {
                                        "Unstaging file…"
                                    } else {
                                        "Staging file…"
                                    },
                                    window,
                                    cx,
                                );
                            },
                        )),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_working(index, area, window, cx)
                    }))
                    .into_any_element()
            }
        }
    }

    pub(super) fn render_git_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        let branch = self
            .work_status
            .as_ref()
            .and_then(|s| s.branch.clone())
            .unwrap_or("Detached HEAD".into());
        let selected_remote = self.remote_name.read(cx).value().trim().to_owned();
        let selected_branch = self.remote_branch.read(cx).value().trim().to_owned();
        let remote = self
            .remotes
            .iter()
            .find(|remote| remote.name == selected_remote);
        let target = format!("{selected_remote}/{selected_branch}");
        let branch_empty = self.branch_name.read(cx).value().trim().is_empty();
        let detached = self.work_status.as_ref().is_none_or(|s| s.branch.is_none());
        let branch_picker = self.render_branch_picker(cx);
        div().flex().flex_col().flex_shrink_0().bg(rgb(p.panel)).border_b_1().border_color(rgb(p.border))
            .child(
                div().min_h(px(54.)).px_4().py_2().flex().flex_wrap().items_center().gap_2()
                    .child(branch_picker)
                    .children(self.work_status.as_ref().map(|status| {
                        let tooltip = status.upstream.as_ref().map_or_else(
                            || "No upstream is configured. Choose a remote and branch in Targets to push.".to_owned(),
                            |upstream| format!("Compared with local {upstream}. Fetch to update remote information."),
                        );
                        div().id("branch-sync-state").flex().items_center().gap_2().px_2().text_size(px(11.))
                            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                            .when(status.upstream.is_some(), |el| el
                                .child(div().flex().items_center().gap_1()
                                    .text_color(rgb(if status.ahead > 0 { p.added } else { p.muted }))
                                    .child(icon("push", 12., if status.ahead > 0 { p.added } else { p.muted }))
                                    .child(format!("{} ahead", status.ahead)))
                                .child(div().flex().items_center().gap_1()
                                    .text_color(rgb(if status.behind > 0 { p.modified } else { p.muted }))
                                    .child(icon("pull", 12., if status.behind > 0 { p.modified } else { p.muted }))
                                    .child(format!("{} behind", status.behind))))
                            .when(status.upstream.is_none(), |el| el.child(div().text_color(rgb(p.muted)).child("No upstream")))
                    }))
                    .children(self.work_status.as_ref().and_then(|s| s.operation.as_ref()).map(|operation| {
                        div().px_2().py_1().rounded(px(5.)).bg(rgb(p.hover)).text_size(px(11.))
                            .text_color(rgb(p.warning)).child(format!("{operation} in progress"))
                    }))
                    .child(div().flex_1())
                    .children([("fetch", "Fetch"), ("pull", "Pull"), ("push", "Push")].into_iter().map(|(id, label)| {
                        let unavailable = busy || remote.is_none() || (id != "fetch" && (selected_branch.is_empty() || detached));
                        let tooltip = if busy { self.operation_busy.unwrap_or_default().to_owned() }
                        else if remote.is_none() { "No configured remote selected · Check Targets".to_owned() }
                        else if id != "fetch" && detached { "Switch to a local branch before pulling or pushing".to_owned() }
                        else if id != "fetch" && selected_branch.is_empty() { "Choose a remote branch in Targets".to_owned() }
                        else {
                            match id {
                                "fetch" => format!("Fetch from {selected_remote}"),
                                "pull" => format!("Pull {target} into {branch} · fast-forward only"),
                                _ => format!("Push {branch} to {target}"),
                            }
                        };
                        action_button(id, label, id).when(id == "push", |button| button.primary())
                            .disabled(unavailable).tooltip(tooltip)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let remote = this.remote_name.read(cx).value().trim().to_owned();
                                let branch = this.remote_branch.read(cx).value().trim().to_owned();
                                let command = match id {
                                    "fetch" => WriteCommand::Fetch { remote },
                                    "pull" => WriteCommand::Pull { remote, branch },
                                    _ => WriteCommand::Push {
                                        remote,
                                        local_branch: this.work_status.as_ref().and_then(|s| s.branch.clone()).unwrap_or_default(),
                                        remote_branch: branch,
                                    },
                                };
                                this.write(command, match id { "fetch" => "Fetching remote…", "pull" => "Pulling fast-forward…", _ => "Pushing branch…" }, window, cx);
                            }))
                    }))
                    .child(button("git-actions-toggle", "Targets", "sliders", self.git_actions_open)
                        .toggled(self.git_actions_open)
                        .tooltip(if self.git_actions_open { "Hide branch and remote targets" } else { "Show branch and remote targets" })
                        .on_click(cx.listener(|this, _, _, cx| { this.git_actions_open = !this.git_actions_open; cx.notify(); }))),
            )
            .when(self.git_actions_open, |el| el.child(
                div().px_4().pb_3().flex().flex_col().gap_2()
                    .child(div().flex().flex_wrap().items_end().gap_3()
                        .child(div().flex().flex_col().gap_1()
                            .child(action_field_label("Find or create a branch", p.muted))
                            .child(div().flex().items_center().gap_1()
                                .child(div().w(px(170.)).child(Input::new(&self.branch_name).text_size(px(12.)).disabled(busy)))
                                .child(button("checkout-branch", "Switch", "branch", false).disabled(busy || branch_empty)
                                    .tooltip("Switch to the named local branch")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let branch = this.branch_name.read(cx).value().trim().to_owned();
                                        this.write(WriteCommand::Checkout { branch }, "Switching branch…", window, cx);
                                    })))
                                .child(button("create-branch", "Create", "branch-add", false).disabled(busy || branch_empty)
                                    .tooltip("Create and switch to a branch from HEAD")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let name = this.branch_name.read(cx).value().trim().to_owned();
                                        this.write(WriteCommand::CreateBranch { name, start_point: None }, "Creating branch…", window, cx);
                                    })))))
                        .child(div().w(px(1.)).h(px(32.)).mx_1().bg(rgb(p.border)))
                        .child(div().w(px(120.)).flex().flex_col().gap_1()
                            .child(action_field_label("Remote", p.muted))
                            .child(Input::new(&self.remote_name).text_size(px(12.)).disabled(busy)))
                        .child(div().w(px(200.)).flex().flex_col().gap_1()
                            .child(action_field_label("Remote branch", p.muted))
                            .child(Input::new(&self.remote_branch).text_size(px(12.)).disabled(busy))))
                    .child(div().flex().items_center().gap_1p5().text_size(px(11.)).text_color(rgb(p.muted))
                        .child(icon("remote", 12., p.muted))
                        .child(div().min_w_0().truncate().child(remote.map_or_else(
                            || "No remote configured · branch and commit actions are available locally".to_owned(),
                            |remote| format!("Push to {} · {}", target, display_remote_url(&remote.push_url)),
                        )))),
            ))
            .into_any_element()
    }
}

fn action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, symbol: &str) -> Button {
    Button::new(id)
        .secondary()
        .h(px(34.))
        .px_3()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .label(label)
        .icon(
            Icon::default()
                .path(format!("icons/{symbol}.svg"))
                .size(px(16.)),
        )
}

fn action_field_label(label: &'static str, color: u32) -> impl IntoElement {
    div()
        .text_size(px(10.))
        .text_color(rgb(color))
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

pub(super) fn retained_area(
    preferred: ChangeArea,
    staged: bool,
    unstaged: bool,
) -> Option<ChangeArea> {
    match preferred {
        ChangeArea::Staged if staged => Some(ChangeArea::Staged),
        ChangeArea::Unstaged if unstaged => Some(ChangeArea::Unstaged),
        _ if staged => Some(ChangeArea::Staged),
        _ if unstaged => Some(ChangeArea::Unstaged),
        _ => None,
    }
}

fn remote_defaults(
    status: &gitturtle_core::RepositoryStatus,
    remotes: &[gitturtle_core::Remote],
) -> (String, String) {
    if let Some(upstream) = &status.upstream
        && let Some((remote, branch)) = remotes
            .iter()
            .filter_map(|remote| {
                upstream
                    .strip_prefix(&format!("{}/", remote.name))
                    .map(|branch| (remote, branch))
            })
            .max_by_key(|(remote, _)| remote.name.len())
    {
        return (remote.name.clone(), branch.into());
    }
    let remote = remotes
        .iter()
        .find(|remote| remote.name == "origin")
        .or(remotes.first())
        .map(|remote| remote.name.clone())
        .unwrap_or_default();
    (remote, status.branch.clone().unwrap_or_default())
}

pub(super) fn display_remote_url(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    if let Some((scheme, remainder)) = without_query.split_once("://") {
        let (authority, path) = remainder
            .split_once('/')
            .map(|(authority, path)| (authority, format!("/{path}")))
            .unwrap_or((remainder, String::new()));
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        format!("{scheme}://{host}{path}")
    } else {
        without_query.into()
    }
}

#[cfg(test)]
mod tests {
    use super::{display_remote_url, remote_defaults, retained_area};
    use core::prelude::v1::test;
    use gitturtle_core::ChangeArea;

    #[test]
    fn staging_preserves_path_selection_and_moves_area_only_when_needed() {
        assert_eq!(
            retained_area(ChangeArea::Unstaged, true, false),
            Some(ChangeArea::Staged)
        );
        assert_eq!(
            retained_area(ChangeArea::Staged, false, true),
            Some(ChangeArea::Unstaged)
        );
        assert_eq!(
            retained_area(ChangeArea::Unstaged, true, true),
            Some(ChangeArea::Unstaged)
        );
        assert_eq!(retained_area(ChangeArea::Staged, false, false), None);
    }

    #[test]
    fn remote_defaults_follow_the_new_branch_and_its_upstream() {
        let remotes = [
            gitturtle_core::Remote {
                name: "origin".into(),
                url: "local".into(),
                push_url: "local".into(),
            },
            gitturtle_core::Remote {
                name: "team/upstream".into(),
                url: "other".into(),
                push_url: "other".into(),
            },
        ];
        let mut status = gitturtle_core::RepositoryStatus {
            branch: Some("feature/new".into()),
            ..Default::default()
        };
        assert_eq!(
            remote_defaults(&status, &remotes),
            ("origin".into(), "feature/new".into())
        );
        status.upstream = Some("team/upstream/different-name".into());
        assert_eq!(
            remote_defaults(&status, &remotes),
            ("team/upstream".into(), "different-name".into())
        );
    }

    #[test]
    fn displayed_remote_address_does_not_expose_embedded_credentials_or_query() {
        assert_eq!(
            display_remote_url("https://user:secret@example.invalid/team/repo?token=private"),
            "https://example.invalid/team/repo"
        );
        assert_eq!(
            display_remote_url("git@example.invalid:team/repo.git"),
            "git@example.invalid:team/repo.git"
        );
    }
}
