//! Native worktree manager; all reads run on a bounded serial worker and all
//! accepted mutations enter GitTurtle's existing operation executor.
use crate::*;
use gitturtle_core::{CreateWorktreePlan, WorktreeCommand, WorktreeDetails, WriteCommand};
use gpui_kit::{
    component::{WindowExt, dialog::DialogButtonProps},
    prelude::FluentBuilder,
};

#[derive(Default)]
pub(super) struct State {
    draft: Option<Entity<WorktreeManager>>,
}

/// Which reviewed removal a navigator action or manager button requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemovalMode {
    /// `git worktree remove` on a clean, unlocked linked worktree.
    Ordinary,
    /// `git worktree remove --force`: deletes dirty content and unfinished
    /// operation state. Locked, main, current and missing worktrees stay refused.
    Force,
}

impl RemovalMode {
    fn blocked(self, details: &WorktreeDetails) -> Option<&str> {
        match self {
            Self::Ordinary => details.removal_blocked.as_deref(),
            Self::Force => details.force_removal_blocked.as_deref(),
        }
    }
}

fn label(id: &'static str, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value)
}

impl GitTurtle {
    pub(super) fn open_worktree_manager(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_worktree_manager(None, window, cx);
    }

    pub(super) fn open_worktree_actions(
        &mut self,
        tree: Worktree,
        removal: Option<RemovalMode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_worktree_manager(Some((tree, removal)), window, cx);
    }

    fn show_worktree_manager(
        &mut self,
        target: Option<(Worktree, Option<RemovalMode>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let owner = cx.entity().downgrade();
        let manager = self
            .worktree_management
            .draft
            .as_ref()
            .filter(|draft| draft.read(cx).repo.path() == repo.path())
            .cloned()
            .unwrap_or_else(|| cx.new(|cx| WorktreeManager::new(owner, repo, window, cx)));
        self.worktree_management.draft = Some(manager.clone());
        manager.update(cx, |this, cx| {
            this.closed
                .store(false, std::sync::atomic::Ordering::Release);
            if let Some((tree, removal)) = target {
                this.creating = false;
                this.filter
                    .update(cx, |input, cx| input.set_value("", window, cx));
                this.refresh_target(Some((tree, removal)), window, cx);
            } else {
                this.refresh(window, cx);
            }
        });
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let done = manager.clone();
            let cancel = manager.clone();
            let closing = manager.downgrade();
            let closed = manager.read(cx).closed.clone();
            dialog
                .title(label("worktree-manager-title", "Worktrees"))
                .width(px(700.))
                .child(manager.clone())
                .button_props(DialogButtonProps::default().ok_text("Done"))
                .on_ok(move |_, _, cx| {
                    done.update(cx, |this, _| this.cancel());
                    true
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, _| this.cancel());
                    true
                })
                .on_close(move |_, _, cx| {
                    closed.store(true, std::sync::atomic::Ordering::Release);
                    let closing = closing.clone();
                    cx.defer(move |cx| {
                        let _ = closing.update(cx, |this, _| this.cancel());
                    });
                })
        });
    }

    pub(super) fn finish_worktree_write(
        &mut self,
        path: &std::path::Path,
        command: &WorktreeCommand,
        succeeded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !succeeded || self.path.as_deref() != Some(path) {
            return;
        }
        let WorktreeCommand::Create(plan) = command else {
            return;
        };
        let plan = plan.clone();
        let owner = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let open_owner = owner.clone(); let open_path = plan.destination.clone();
            let finder = plan.destination.clone(); let editor_owner = owner.clone(); let editor_path = plan.destination.clone();
            dialog.title(label("worktree-created-title", "Worktree created"))
                .width(px(600.)).child(div().flex().flex_col().gap_3()
                    .child(label("worktree-created-target", format!("{}\n{}", plan.branch, plan.destination.display())).text_size(crate::appearance::ui_text(13.)))
                    .child(label("worktree-created-context", "Each worktree keeps its own working files, index, HEAD, and GitTurtle commit draft. Branches and objects are shared.").text_size(crate::appearance::ui_text(12.)))
                    .child(div().flex().flex_wrap().gap_2()
                        .child(button("open-created-worktree", "Open in GitTurtle", "", false).on_click(move |_, window, cx| { let _ = open_owner.update(cx, |this, cx| { if this.operation_busy.is_none() { window.close_dialog(cx); this.open(open_path.clone(), None, window, cx); } }); }))
                        .child(button("reveal-created-worktree", if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in files" }, "", false).on_click(move |_, _, cx| cx.reveal_path(&finder)))
                        .child(button("edit-created-worktree", "Open in editor", "", false).on_click(move |_, window, cx| { let _ = editor_owner.update(cx, |this, cx| this.open_worktree_editor(editor_path.clone(), window, cx)); }))))
                .button_props(DialogButtonProps::default().ok_text("Done"))
        });
    }

    fn open_worktree_editor(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.settings.external_editor.trim().to_owned();
        if editor.is_empty() {
            window.close_dialog(cx);
            self.show_settings(window, cx);
            self.operation_notice = Some(
                "Choose an editor in Settings → Projects, then use Worktrees → Open in editor."
                    .into(),
            );
            return;
        }
        let response = self.preferences_writer.submit_read(move || {
            let mut command = if cfg!(target_os = "macos") { let mut command = std::process::Command::new("/usr/bin/open"); command.args(["-a", &editor, "--"]); command } else { std::process::Command::new(&editor) };
            let mut child = command.arg(path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn()?;
            if cfg!(target_os = "macos") {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                loop {
                    if let Some(status) = child.try_wait()? { anyhow::ensure!(status.success(), "Could not open the configured editor. Check its application name in Settings."); break; }
                    if std::time::Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); anyhow::bail!("The editor launcher did not report a result. Check whether it opened before trying again."); }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            } else { std::thread::spawn(move || { let _ = child.wait(); }); }
            Ok(())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ =
                this.update_in(cx, |this, _, cx| {
                    match result {
                        Ok(Ok(())) => {
                            this.operation_notice =
                                Some("Opened the selected worktree in your editor.".into())
                        }
                        Ok(Err(error)) => this.operation_error = Some(format!("{error:#}")),
                        Err(_) => this.operation_error = Some(
                            "Editor launcher interrupted; check whether it opened before retrying."
                                .into(),
                        ),
                    };
                    cx.notify();
                });
        })
        .detach();
    }
}

struct WorktreeManager {
    closed: Arc<std::sync::atomic::AtomicBool>,
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    reader: operations::SerialExecutor,
    task: Option<Task<()>>,
    cancellation: gitturtle_core::HistoryCancellation,
    trees: Vec<Worktree>,
    branches: Vec<Branch>,
    selected: Option<PathBuf>,
    details: Option<WorktreeDetails>,
    pending: bool,
    reviewing: bool,
    error: Option<String>,
    creating: bool,
    new_branch: bool,
    filter: Entity<InputState>,
    branch: Entity<InputState>,
    start: Entity<InputState>,
    folder: Entity<InputState>,
    parent: PathBuf,
    _subscriptions: Vec<Subscription>,
}
impl Drop for WorktreeManager {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
impl WorktreeManager {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter worktrees by branch or path")
        });
        let branch = cx.new(|cx| InputState::new(window, cx).placeholder("Branch name"));
        let subscriptions = [&filter, &branch]
            .map(|input| cx.subscribe_in(input, window, |_, _, _: &InputEvent, _, cx| cx.notify()))
            .into();
        let parent = repo.path().parent().unwrap_or(repo.path()).to_owned();
        Self {
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            owner,
            repo,
            reader: operations::SerialExecutor::new("worktree-manager"),
            task: None,
            cancellation: Default::default(),
            trees: Vec::new(),
            branches: Vec::new(),
            selected: None,
            details: None,
            pending: false,
            reviewing: false,
            error: None,
            creating: false,
            new_branch: true,
            filter,
            branch,
            start: cx.new(|cx| InputState::new(window, cx).default_value("HEAD")),
            folder: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("New folder name")
                    .default_value("parallel-work")
            }),
            parent,
            _subscriptions: subscriptions,
        }
    }
    fn cancel(&mut self) {
        self.cancellation.cancel();
        self.task = None;
        self.pending = false;
        self.reviewing = false;
    }
    fn current(&self, cx: &App) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            let owner = owner.read(cx);
            owner.path.as_deref() == Some(self.repo.path())
                && owner.page == AppPage::Repository
                && owner.operation_busy.is_none()
        })
    }
    fn set_creating(&mut self, creating: bool, cx: &mut Context<Self>) {
        if self.creating != creating {
            // A different workflow supersedes a pending review, including a
            // navigator removal that would otherwise open its confirmation.
            if self.reviewing {
                self.cancel();
            }
            self.creating = creating;
            cx.notify();
        }
    }
    fn read<T: Send + 'static>(
        &mut self,
        reviewing: bool,
        operation: impl FnOnce(GitRepository) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel();
        self.pending = true;
        self.reviewing = reviewing;
        self.error = None;
        self.cancellation = Default::default();
        let cancellation = self.cancellation.clone();
        let repo = self.repo.clone();
        let response = self.reader.submit_read(move || {
            gitturtle_core::run_cancellable_inspection(cancellation, || operation(repo))
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Worktree read interrupted. Refresh to try again.")));
            let _ = this.update_in(cx, |this, window, cx| { if this.closed.load(std::sync::atomic::Ordering::Acquire) { return; } this.pending = false; this.reviewing = false; if !this.current(cx) { this.error = Some("The repository context changed or an operation started. Reopen Worktrees when it finishes.".into()); } else { match result { Ok(value) => receive(this, value, window, cx), Err(error) => this.error = Some(format!("{error:#}")) } } cx.notify(); });
        }));
        cx.notify();
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_target(None, window, cx);
    }
    fn refresh_target(
        &mut self,
        target: Option<(Worktree, Option<RemovalMode>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.details = None;
        self.read(
            target
                .as_ref()
                .is_some_and(|(_, removal)| removal.is_some()),
            |repo| Ok((repo.worktrees()?, repo.branches()?)),
            move |this, (trees, branches), window, cx| {
                this.trees = trees;
                this.branches = branches;
                if let Some((tree, removal)) = target {
                    // Inspect the captured row identity. A stale or removed target
                    // must never select a different tree for removal.
                    this.inspect(tree, removal, window, cx);
                    return;
                }
                let selected = this
                    .selected
                    .as_ref()
                    .and_then(|path| this.trees.iter().find(|tree| &tree.path == path))
                    .or_else(|| this.trees.first())
                    .cloned();
                if let Some(tree) = selected {
                    this.select(tree, window, cx);
                }
            },
            window,
            cx,
        );
    }
    fn select(&mut self, tree: Worktree, window: &mut Window, cx: &mut Context<Self>) {
        self.inspect(tree, None, window, cx);
    }
    fn inspect(
        &mut self,
        tree: Worktree,
        removal: Option<RemovalMode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some(tree.path.clone());
        self.details = None;
        self.read(
            removal.is_some(),
            move |repo| repo.worktree_details(&tree),
            move |this, details, window, cx| {
                this.details = Some(details);
                if let Some(mode) = removal {
                    this.confirm_removal(mode, window, cx);
                }
            },
            window,
            cx,
        );
    }
    fn choose_parent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let response = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose worktree parent folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let response = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                match response {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.parent = path;
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => this.error = Some(error.to_string()),
                    Err(_) => this.error = Some("Folder picker was interrupted.".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn prepare_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.current(cx) {
            return;
        }
        let branch = self.branch.read(cx).value().trim().to_owned();
        let start = self.start.read(cx).value().trim().to_owned();
        let folder = self.folder.read(cx).value().to_string();
        if folder.is_empty()
            || PathBuf::from(&folder).components().count() != 1
            || !matches!(
                PathBuf::from(&folder).components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            self.error =
                Some("Enter one new folder name; use Choose parent to select its location.".into());
            cx.notify();
            return;
        }
        let destination = self.parent.join(folder);
        let new_branch = self.new_branch;
        self.read(true, move |repo| repo.create_worktree_plan(&destination, &branch, new_branch, &start), |this, plan: CreateWorktreePlan, window, cx| {
            let _ = this.owner.update(cx, |owner, cx| {
                if owner.path.as_deref() != Some(this.repo.path()) || owner.operation_busy.is_some() { return; }
                window.close_dialog(cx);
                owner.confirm_git_write("Create worktree".into(), format!("{} branch: {}\nCommit: {}\nDestination: {}\n\nGit will create and check out a linked worktree in this new folder. Configured checkout hooks and filters apply.\n\nThe original worktree and its staged/unstaged changes remain intact. Each worktree has its own GitTurtle commit draft. The branch and repository objects are shared.", if plan.new_branch { "New" } else { "Existing" }, plan.branch, plan.target_oid, plan.destination.display()), "Create worktree", WriteCommand::Worktree(Arc::new(WorktreeCommand::Create(plan))), window, cx);
            });
        }, window, cx);
    }
    fn remove(&mut self, mode: RemovalMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.current(cx) {
            return;
        }
        let Some(tree) = self
            .details
            .as_ref()
            .filter(|details| mode.blocked(details).is_none())
            .map(|details| details.tree.clone())
        else {
            return;
        };
        // Selection details can remain open while another tool changes the
        // target. Prepare a fresh review of this exact identity before asking
        // for confirmation; execution performs its own final stale check.
        self.inspect(tree, Some(mode), window, cx);
    }
    fn confirm_removal(&mut self, mode: RemovalMode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = self
            .details
            .clone()
            .filter(|details| mode.blocked(details).is_none())
        else {
            return;
        };
        if self.pending || !self.current(cx) {
            return;
        }
        let identity = format!(
            "Folder: {}\nBranch: {}\nCommit: {}",
            details.tree.path.display(),
            details.tree.branch.as_deref().unwrap_or("Detached HEAD"),
            details.tree.oid
        );
        let (title, explanation, action, command) = match mode {
            RemovalMode::Ordinary => (
                "Remove linked worktree",
                format!(
                    "{identity}\n\nGit will remove this clean worktree folder and its private worktree metadata. Removing a worktree does not delete branches or shared objects. Any branch remains available for another worktree.\n\nRemoval refuses if the worktree changes, is locked, becomes unavailable, has active Git state or index flags that hide changes, or contains tracked, untracked, or ignored changes. No force removal or metadata repair is attempted."
                ),
                "Remove worktree",
                WorktreeCommand::Remove(details),
            ),
            RemovalMode::Force => (
                "Force remove linked worktree",
                format!(
                    "{identity}\nContent to delete: {} changed or untracked files · {} ignored files\n\nGit will delete this worktree folder with its changed, untracked, and ignored files, any unfinished merge, rebase, cherry-pick, bisect, or sequencer state, and any initialized submodule checkout inside it. Uncommitted content is not recoverable from Git. Its private worktree metadata is removed. Committed work stays in the shared repository; the branch remains available for another worktree.\n\nForce removal still refuses if the worktree is the main or current worktree, is locked, holds a Git lock file, becomes unavailable, or changes after this review. No lock override or metadata repair is attempted.",
                    details.changed_files, details.ignored_files
                ),
                "Force remove worktree",
                WorktreeCommand::ForceRemove(details),
            ),
        };
        let _ = self.owner.update(cx, |owner, cx| {
            window.close_dialog(cx);
            owner.confirm_git_write(
                title.into(),
                explanation,
                action,
                WriteCommand::Worktree(Arc::new(command)),
                window,
                cx,
            );
        });
    }
}

impl Render for WorktreeManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let body_height = (window.viewport_size().height - px(240.)).max(px(120.));
        let unavailable = self.pending || !self.current(cx);
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let trees: Vec<_> = self
            .trees
            .iter()
            .filter(|tree| {
                tree.path.to_string_lossy().to_lowercase().contains(&query)
                    || tree
                        .branch
                        .as_ref()
                        .is_some_and(|branch| branch.to_lowercase().contains(&query))
            })
            .take(101)
            .cloned()
            .collect();
        let branch_query = self.branch.read(cx).value().trim().to_lowercase();
        let choices: Vec<_> = self
            .branches
            .iter()
            .filter(|branch| !branch.remote && branch.name.to_lowercase().contains(&branch_query))
            .take(12)
            .cloned()
            .collect();
        div().id("worktree-manager-content").flex().flex_col().gap_3().max_h(px(570.).min(body_height)).overflow_y_scroll()
            .child(div().flex().gap_2()
                .child(button("worktree-browse-tab", "Manage", "", !self.creating).debug_selector(|| "worktree-browse-tab".to_string()).toggled(!self.creating).on_click(cx.listener(|this, _, _, cx| this.set_creating(false, cx))))
                .child(button("worktree-create-tab", "Create worktree…", "plus", self.creating).debug_selector(|| "worktree-create-tab".to_string()).toggled(self.creating).on_click(cx.listener(|this, _, _, cx| this.set_creating(true, cx))))
                .child(button("refresh-worktrees", "Refresh", "refresh-cw", false).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx)))))
            .when(self.pending, |element| element.child(div().flex().gap_2().child(label("worktree-loading", "Reading worktree identities and content…").text_size(crate::appearance::ui_text(12.))).child(button("cancel-worktree-read", "Cancel", "", false).debug_selector(|| "cancel-worktree-read".to_string()).on_click(cx.listener(|this, _, _, cx| { this.cancel(); cx.notify(); })))))
            .children(self.error.as_ref().map(|error| label("worktree-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .when(!self.creating, |element| element
                .child(Input::new(&self.filter).aria_label("Filter worktrees by branch or path").cleanable(true))
                .child(div().id("managed-worktree-list").max_h(px(190.)).overflow_y_scroll().flex().flex_col().gap_1().children(trees.iter().take(100).enumerate().map(|(index, tree)| {
                    let tree = tree.clone(); let selected = self.selected.as_ref() == Some(&tree.path); let text = format!("{} · {}{}{}", tree.branch.as_deref().unwrap_or("Detached HEAD"), tree.path.display(), if tree.locked { " · Locked" } else { "" }, if tree.prunable { " · Missing or stale" } else { "" });
                    Button::new(("managed-worktree-row", index)).ghost().w_full().h_auto().min_h(crate::appearance::ui_size(36.)).justify_start().px_3().py_2().label(text.clone()).accessibility_label(text).selected(selected).toggled(selected).border_1().border_color(rgb(if selected { p.accent } else { p.border })).when(selected, |row| row.bg(rgb(p.selected))).when(selected, |row| row.hover(|row| row.bg(rgb(p.row_hover(true))))).on_click(cx.listener(move |this, _, window, cx| this.select(tree.clone(), window, cx)))
                })).when(trees.is_empty() && !self.pending, |element| element.child(label("worktrees-empty", "No worktrees match this filter.").text_size(crate::appearance::ui_text(12.)))))
                .when(trees.len() > 100, |element| element.child(label("worktrees-match-limit", "Showing 100 matches. Narrow the filter to find another worktree.").text_size(crate::appearance::ui_text(12.))))
                .when_some(self.details.as_ref(), |element, details| {
                    let details = details.clone(); let path = details.tree.path.clone(); let finder = path.clone(); let editor = path.clone();
                    element.child(div().flex().flex_col().gap_2()
                        .child(label("worktree-selected-identity", format!("{}{} · {}\n{}", details.tree.branch.as_deref().unwrap_or("Detached HEAD"), if details.main { " · Main worktree" } else if details.current { " · Current worktree" } else { "" }, short_oid(&details.tree.oid), details.tree.path.display())).text_size(crate::appearance::ui_text(12.)))
                        .child(label("worktree-selected-status", if details.missing { "Folder missing or unavailable".into() } else { format!("{} changed/untracked files · {} ignored files{}", details.changed_files, details.ignored_files, if details.removal_blocked.is_none() { " · Clean" } else { "" }) }).text_size(crate::appearance::ui_text(12.)))
                        .when_some(details.removal_blocked.as_ref(), |element, reason| element.child(label("worktree-removal-protection", reason.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted))))
                        .child(div().flex().flex_wrap().gap_2()
                            .child(button("open-managed-worktree", "Open in GitTurtle", "", false).disabled(unavailable || details.missing || details.current).on_click(cx.listener(move |this, _, window, cx| { let _ = this.owner.update(cx, |owner, cx| { if owner.operation_busy.is_none() && owner.path.as_deref() == Some(this.repo.path()) { window.close_dialog(cx); owner.open(path.clone(), None, window, cx); } }); })))
                            .child(button("reveal-managed-worktree", if cfg!(target_os = "macos") { "Finder" } else { "Files" }, "", false).disabled(details.missing).on_click(move |_, _, cx| cx.reveal_path(&finder)))
                            .child(button("edit-managed-worktree", "Editor", "", false).disabled(unavailable || details.missing).on_click(cx.listener(move |this, _, window, cx| { let _ = this.owner.update(cx, |owner, cx| owner.open_worktree_editor(editor.clone(), window, cx)); })))
                            .child(button("remove-managed-worktree", "Remove worktree…", "", false).debug_selector(|| "remove-managed-worktree".to_string()).disabled(unavailable || details.removal_blocked.is_some()).on_click(cx.listener(|this, _, window, cx| this.remove(RemovalMode::Ordinary, window, cx))))
                            .child(button("force-remove-managed-worktree", "Force remove worktree…", "", false).debug_selector(|| "force-remove-managed-worktree".to_string()).disabled(unavailable || details.force_removal_blocked.is_some()).on_click(cx.listener(|this, _, window, cx| this.remove(RemovalMode::Force, window, cx))))))
                }))
            .when(self.creating, |element| element
                .child(div().flex().gap_2().children([(false, "Existing branch"), (true, "New branch")].map(|(new, name)| button(name, name, "", self.new_branch == new).toggled(self.new_branch == new).disabled(self.pending).on_click(cx.listener(move |this, _, _, cx| { this.new_branch = new; this.error = None; cx.notify(); })))))
                .child(label("worktree-branch-label", if self.new_branch { "New branch name" } else { "Choose a local branch" }).text_size(crate::appearance::ui_text(12.)))
                .child(Input::new(&self.branch).aria_label("Worktree branch name"))
                .when(!self.new_branch, |element| element.child(div().id("worktree-branch-choices").max_h(px(140.)).overflow_y_scroll().flex().flex_col().gap_1().children(choices.iter().enumerate().map(|(index, branch)| {
                    let name = branch.name.clone(); let occupied = self.trees.iter().any(|tree| tree.branch.as_deref() == Some(&name));
                    button(("worktree-branch-choice", index), format!("{}{}", name, if occupied { " · In use" } else { "" }), "", false).disabled(occupied || self.pending).on_click(cx.listener(move |this, _, window, cx| { this.branch.update(cx, |input, cx| input.set_value(name.clone(), window, cx)); cx.notify(); }))
                })).when(choices.is_empty(), |element| element.child(label("worktree-branches-empty", "No local branch matches. Create a new branch or change the name.").text_size(crate::appearance::ui_text(12.))))))
                .when(self.new_branch, |element| element.child(label("worktree-start-label", "Start from local branch, tag, or commit").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.start).aria_label("New worktree starting revision")))
                .child(label("worktree-parent", format!("Parent folder: {}", self.parent.display())).text_size(crate::appearance::ui_text(12.)))
                .child(button("choose-worktree-parent", "Choose parent folder…", "folder", false).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.choose_parent(window, cx))))
                .child(Input::new(&self.folder).aria_label("New worktree folder name"))
                .child(label("worktree-destination-rule", "Choose a new folder inside the parent. Existing folders and branches checked out elsewhere are protected. Review the resolved branch and destination before creating.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                .child(button("review-create-worktree", "Review worktree…", "", false).disabled(unavailable).on_click(cx.listener(|this, _, window, cx| this.prepare_create(window, cx)))))
    }
}

#[cfg(test)]
mod tests;
