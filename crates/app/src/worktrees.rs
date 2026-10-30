//! Native worktree manager; all reads run on a bounded serial worker and all
//! accepted mutations enter GitTurtle's existing operation executor.
use crate::*;
use gitturtle_core::{CreateWorktreePlan, WorktreeCommand, WorktreeDetails, WriteCommand};

mod removal_review;
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

/// Where the worktree manager's text starts, in from its content edge: the
/// filter's text inset, where its placeholder starts at every size. The
/// title, the labels, the details and each worktree row's name start here,
/// as the Reflog's text does; the rows centred their names, and the other
/// lines started at the content's edge.
const TEXT_INSET: Pixels = appearance::FIELD_TEXT_INSET;

/// [`native_accessibility::text`] at the manager's [`TEXT_INSET`].
fn label(
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
) -> native_accessibility::StaticText<Stateful<Div>> {
    native_accessibility::text(id, value).ml(TEXT_INSET)
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
            this.reveal_field = true;
            if let Some((tree, removal)) = target {
                this.creating = false;
                this.filter
                    .update(cx, |input, cx| input.set_value("", window, cx));
                this.refresh_target(Some((tree, removal)), window, cx);
            } else {
                this.refresh(window, cx);
            }
        });
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let done = manager.clone();
            let cancel = manager.clone();
            // The dialog clips its body to the body's bounds, and the manager
            // keeps the focus ring's room inside them above Manage and below
            // its last control. The title's margin and the footer's gap give
            // the room back, so nothing in the dialog moves.
            let room = appearance::button_ring_room(cx);
            dialog
                .title(
                    native_accessibility::dialog_title("worktree-manager-title", "Worktrees")
                        .ml(TEXT_INSET)
                        .mb(-room),
                )
                .width(appearance::dialog_width(px(700.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(crate::tags::DIALOG_FOOTER_GAP - room)
                .child(manager.clone())
                .footer(crate::tags::dialog_footer("Done", None))
                // Done and Escape close the dialog here, stopping the read.
                // The kit's alert dialog never calls an `on_close` (its
                // surface takes the alert's own button props, which hold
                // none), so one set here never ran.
                .on_ok(move |_, _, cx| {
                    done.update(cx, |this, _| this.cancel());
                    true
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, _| this.cancel());
                    true
                })
        });
        // Opening the dialog focuses its shell, which draws no ring: focus
        // the shown form's first field, the filter in Manage or the branch
        // field in Create, so the first frame draws its ring, as the Reflog
        // focuses its filter. The dialog keeps the opener to return to.
        let field = self
            .worktree_management
            .draft
            .as_ref()
            .map(|manager| manager.read(cx).first_field(cx));
        if let Some(field) = field {
            field.focus(window, cx);
        }
        window.refresh();
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
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let open_owner = owner.clone(); let open_path = plan.destination.clone();
            let finder = plan.destination.clone(); let editor_owner = owner.clone(); let editor_path = plan.destination.clone();
            dialog.title(native_accessibility::dialog_title("worktree-created-title", "Worktree created"))
                .width(appearance::dialog_width(px(600.), window)).max_h(appearance::dialog_max_height(window)).child(div().flex().flex_col().gap_3()
                    .child(native_accessibility::text("worktree-created-target", format!("{}\n{}", plan.branch, plan.destination.display())).text_size(crate::appearance::ui_text(13.)))
                    .child(native_accessibility::text("worktree-created-context", "Each worktree keeps its own working files, index, HEAD, and GitTurtle commit draft. Branches and objects are shared.").text_size(crate::appearance::ui_text(12.)))
                    .child(div().flex().flex_wrap().gap_2()
                        .child(button("open-created-worktree", "Open in GitTurtle", "", false).on_click(move |_, window, cx| { let _ = open_owner.update(cx, |this, cx| { if this.operation_busy.is_none() { window.close_dialog(cx); this.open(open_path.clone(), None, window, cx); } }); }))
                        .child(button("reveal-created-worktree", if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in files" }, "", false).on_click(move |_, _, cx| cx.reveal_path(&finder)))
                        .child(button("edit-created-worktree", "Open in editor", "", false).on_click(move |_, window, cx| { let _ = editor_owner.update(cx, |this, cx| this.open_worktree_editor(editor_path.clone(), window, cx)); }))))
                .footer(crate::tags::dialog_footer("Done", None))
        });
    }

    fn open_worktree_editor(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.settings.external_editor.trim().to_owned();
        let response = self
            .preferences_writer
            .submit_read(move || platform_polish::open_in_editor(&editor, &path));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ =
                this.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(Ok(Some(_))) => {
                            this.operation_notice =
                                Some("Opened the selected worktree in your editor.".into())
                        }
                        Ok(Ok(None)) => {
                            window.close_dialog(cx);
                            this.show_settings(window, cx);
                            this.operation_notice = Some(
                                "Choose an editor in Settings → Projects, then use Worktrees → Open in editor."
                                    .into(),
                            );
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
    /// The loading row's Cancel, which hands focus to the shown form's first
    /// field as the row goes.
    cancel_read: focus_ring::LoadingCancel,
    /// Whether the next frame scrolls the content to show the shown form's
    /// first field, if it holds focus: set as the dialog opens.
    reveal_field: bool,
    /// The content's scroll, which its scrollbar follows and which reveals
    /// the control the keyboard moves focus onto, the worktree list or
    /// Create's branch choices as a whole for one of theirs.
    content: crate::focus_reveal::FocusReveal,
    /// The worktree list's scroll, which its scroll cue follows and which
    /// reveals the row the keyboard moves focus onto.
    rows: crate::focus_reveal::FocusReveal,
    /// Create's branch choices' scroll, which reveals the choice the
    /// keyboard moves focus onto.
    choices: crate::focus_reveal::FocusReveal,
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
            cancel_read: focus_ring::LoadingCancel::new(cx),
            reveal_field: false,
            content: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
            rows: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
            choices: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
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
    /// The shown form's first field: the filter in Manage, the branch field
    /// in Create.
    fn first_field(&self, cx: &App) -> FocusHandle {
        let field = if self.creating {
            &self.branch
        } else {
            &self.filter
        };
        field.read(cx).focus_handle(cx)
    }
    /// As the loading row goes, focus its Cancel holds moves to the first
    /// field below it: the filter in Manage, the branch field in Create.
    fn leave_loading_row(&self, window: &mut Window, cx: &mut App) {
        let field = self.first_field(cx);
        self.cancel_read.release_to(&field, window, cx);
    }
    /// Whether the app refuses the dialog's actions, and whether they look
    /// it: refused while its repository is no longer the one shown or a
    /// write runs, and dimmed with a write only once it looks busy
    /// (`workspace::BusyLook`), as everywhere else; they dimmed the moment
    /// any write started. Its own Create and Remove close the dialog before
    /// their review submits, so a write it sees is never its own.
    fn refusal(&self, cx: &App) -> (bool, bool) {
        crate::workspace::dialog_refusal(&self.owner, Some(self.repo.path()), cx)
    }
    /// Whether the dialog may act now ([`Self::refusal`]).
    fn current(&self, cx: &App) -> bool {
        !self.refusal(cx).0
    }
    /// [`Self::refusal`], with the dialog's own read in progress.
    fn unavailable(&self, cx: &App) -> (bool, bool) {
        let (refused, dimmed) = self.refusal(cx);
        (self.pending || refused, self.pending || dimmed)
    }
    /// Shows Manage or Create from its tab. Focus the hidden form's first
    /// field held, as it does from the dialog's opening, moves to the shown
    /// form's first field: left on a field no frame draws, Tab started from
    /// nowhere and Enter reached no dialog action.
    fn switch_form(&mut self, creating: bool, window: &mut Window, cx: &mut Context<Self>) {
        let held = self.first_field(cx).contains_focused(window, cx);
        self.set_creating(creating, cx);
        if held {
            self.first_field(cx).focus(window, cx);
        }
    }
    fn set_creating(&mut self, creating: bool, cx: &mut Context<Self>) {
        if self.creating != creating {
            // A different workflow supersedes a pending review, including a
            // navigator removal that would otherwise open its confirmation.
            if self.reviewing {
                self.cancel();
            }
            self.creating = creating;
            self.show_first_choice();
            cx.notify();
        }
    }
    /// Shows Create for an existing branch, with its choices, or a new one.
    fn set_new_branch(&mut self, new_branch: bool, cx: &mut Context<Self>) {
        if self.new_branch != new_branch {
            self.new_branch = new_branch;
            self.show_first_choice();
        }
        self.error = None;
        cx.notify();
    }
    /// The branch choices' scroll stays with the manager while they are
    /// hidden, so they show again from their first choice rather than where
    /// a reveal or a wheel last left them.
    fn show_first_choice(&self) {
        self.choices.scroll().set_offset(Point::default());
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
            let _ = this.update_in(cx, |this, window, cx| { this.pending = false; this.reviewing = false; this.leave_loading_row(window, cx); if !this.current(cx) { crate::tags::set_error_line(&mut this.error, "The repository context changed or an operation started. Reopen Worktrees when it finishes.", window, cx); } else { match result { Ok(value) => receive(this, value, window, cx), Err(error) => crate::tags::set_error_line(&mut this.error, format!("{error:#}"), window, cx) } } cx.notify(); });
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
            let _ = this.update_in(cx, |this, window, cx| {
                match response {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.parent = path;
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        crate::tags::set_error_line(&mut this.error, error.to_string(), window, cx)
                    }
                    Err(_) => crate::tags::set_error_line(
                        &mut this.error,
                        "Folder picker was interrupted.",
                        window,
                        cx,
                    ),
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
            crate::tags::set_error_line(
                &mut self.error,
                "Enter one new folder name; use Choose parent to select its location.",
                window,
                cx,
            );
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
        let _ = self.owner.update(cx, |owner, cx| {
            window.close_dialog(cx);
            if mode == RemovalMode::Force {
                owner.confirm_force_worktree_removal(details, window, cx);
            } else {
                let identity = format!(
                    "Folder: {}\nBranch: {}\nCommit: {}",
                    details.tree.path.display(),
                    details.tree.branch.as_deref().unwrap_or("Detached HEAD"),
                    details.tree.oid
                );
                owner.confirm_git_write(
                    "Remove linked worktree".into(),
                    format!("{identity}\n\nGit will remove this clean worktree folder and its private worktree metadata. Removing a worktree does not delete branches or shared objects. Any branch remains available for another worktree.\n\nRemoval refuses if the worktree changes, is locked, becomes unavailable, has active Git state or index flags that hide changes, or contains tracked, untracked, or ignored changes. No force removal or metadata repair is attempted."),
                    "Remove worktree",
                    WriteCommand::Worktree(Arc::new(WorktreeCommand::Remove(details))),
                    window,
                    cx,
                );
            }
        });
    }
}

/// The list's surface a focused worktree row's ring keeps before the next
/// row's border: rows stand apart by [`appearance::button_ring_room`] plus
/// this, 5 px at the default ring and at every text size, so the ring never
/// reads as touching its neighbour.
const ROW_RING_CLEARANCE: Pixels = px(2.);

/// One row of the managed worktree list. A selected row shows its selection
/// through the selected surface and its selected and toggled state, and keeps
/// the neutral border of every other row, so an accent outline around a row
/// always means keyboard focus: the focused row's ring, drawn once. Its name
/// starts inside its 1 px border at the field's padding, on the manager's
/// [`TEXT_INSET`], rather than centred as the kit centres a Button's label.
fn worktree_row(index: usize, text: String, selected: bool, p: appearance::Palette) -> Button {
    Button::new(("managed-worktree-row", index))
        .debug_selector(move || format!("managed-worktree-row-{index}"))
        .ghost()
        .w_full()
        .h_auto()
        .min_h(crate::appearance::ui_size(36.))
        .pl(appearance::FIELD_TEXT_PADDING)
        .pr_3()
        .py_2()
        .child(crate::tags::row_text(
            format!("managed-worktree-row-text-{index}"),
            text.clone(),
        ))
        .accessibility_label(text)
        .selected(selected)
        .toggled(selected)
        .border_1()
        .border_color(rgb(p.border))
        .when(selected, |row| {
            row.bg(rgb(p.selected))
                .hover(|row| row.bg(rgb(p.row_hover(true))))
        })
}

impl Render for WorktreeManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let body_height = (window.viewport_size().height - px(240.)).max(px(120.));
        let (unavailable, dimmed) = self.unavailable(cx);
        let refusing = |control: Button, key: &'static str, refused: bool, dims: bool| {
            crate::workspace::refusing(
                control,
                || key.into(),
                unavailable || refused,
                dimmed || dims,
            )
        };
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
        // The content, its list of worktrees and its list of branch choices
        // scroll, so each clips to its bounds on both axes and keeps the
        // focus ring's room inside: the content around its controls, giving
        // the sides back through its margin and the top and bottom through
        // the dialog, and each list around its rows, giving it all back
        // through its margin. The content counts each child's box in what it
        // can scroll, so each list borrows through a wrapper that shrinks as
        // the list did. No control moves. The worktree rows stand apart by
        // the ring's room and `ROW_RING_CLEARANCE` more, so a focused row's
        // ring keeps some of the list's surface before the next row's border.
        // The list's wrapper shrinks no shorter than two whole rows, or the
        // one a single worktree takes, so in a short window, or a narrow one
        // where the details below wrap, the content scrolls rather than
        // squeeze the list: at 13 pt in a 461 × 490 half tile it kept one of
        // two worktrees, too short a track for a scrollbar's thumb to move
        // in. A list with no worktree keeps its whole explanation: at 18 pt
        // in the half tile, with a worktree's details shown, a filter that
        // matched nothing left "No worktrees match this filter" in a list
        // 6 px tall. Past its height the list cues the rest with its
        // scrollbar, and the content, beside it in the dialog's side
        // padding, cues its own. Create's branch choices keep two whole
        // choices, or the one there is, or their whole explanation, the
        // same way, and cue theirs: in the half tile the content squeezed
        // them to nothing, so a choice Tab moved onto showed nowhere.
        let room = appearance::button_ring_room(cx);
        let row = appearance::ui_size(36.);
        let floor_rows = trees.len().clamp(1, 2) as f32;
        let rows_floor = row * floor_rows + (room + ROW_RING_CLEARANCE) * (floor_rows - 1.);
        let floor_choices = choices.len().clamp(1, 2) as f32;
        let choices_floor = appearance::ui_size(28.) * floor_choices
            + rems(0.25).to_pixels(window.rem_size()) * (floor_choices - 1.);
        // A control the keyboard moves focus onto scrolls into view in this
        // frame with its ring: the list shows the row, and the content the
        // whole list, or the control, as the Reflog's do. Each control
        // reports where it lies to the content; the selected worktree's
        // details, below the list, once stayed below the content's fold, as
        // did every control of Create's form. Create's branch choices reveal
        // a choice as the worktree list reveals a row, and the content shows
        // the choices' list.
        self.rows.reveal(window, cx);
        self.choices.reveal(window, cx);
        self.content.reveal(window, cx);
        let rows: Vec<_> = (0..trees.len().min(100))
            .map(|index| self.rows.focus(("managed-worktree-row", index), cx))
            .collect();
        let reveal = self.rows.items(rows.clone(), room);
        let show_list = self.content.group(rows.clone(), room);
        let tabs = [
            "worktree-browse-tab",
            "worktree-create-tab",
            "refresh-worktrees",
        ]
        .map(|id| self.content.focus(id, cx));
        let filter = self.filter.read(cx).focus_handle(cx);
        let actions = [
            "open-managed-worktree",
            "reveal-managed-worktree",
            "edit-managed-worktree",
            "remove-managed-worktree",
            "force-remove-managed-worktree",
        ]
        .map(|id| self.content.focus(id, cx));
        let kinds = ["worktree-existing-branch", "worktree-new-branch"]
            .map(|id| self.content.focus(id, cx));
        let branch_field = self.branch.read(cx).focus_handle(cx);
        let start_field = self.start.read(cx).focus_handle(cx);
        let folder_field = self.folder.read(cx).focus_handle(cx);
        let [parent_action, review_action] = ["choose-worktree-parent", "review-create-worktree"]
            .map(|id| self.content.focus(id, cx));
        let choice_focus: Vec<_> = (0..choices.len())
            .map(|index| self.choices.focus(("worktree-branch-choice", index), cx))
            .collect();
        let reveal_choice = self.choices.items(choice_focus.clone(), room);
        let show_choices = self.content.group(choice_focus.clone(), room);
        let cue = crate::tags::ScrollCue::revealing(&self.rows, room);
        let choice_cue = crate::tags::ScrollCue::revealing(&self.choices, room);
        // As the dialog opens, the content scrolls the shown form's first
        // field into view with its ring: it follows the tabs' row, the
        // loading row and the error, and in Create the branch choice and its
        // label.
        if std::mem::take(&mut self.reveal_field) && self.first_field(cx).is_focused(window) {
            let field = 1
                + usize::from(self.pending)
                + usize::from(self.error.is_some())
                + if self.creating { 2 } else { 0 };
            self.content.scroll().scroll_to_item(field);
        }
        let content = crate::native_accessibility::clips_children(div().on_children_prepainted(self.content.observe()).id("worktree-manager-content").debug_selector(|| "worktree-manager-content".into()).flex().flex_col().gap_3().max_h(px(570.).min(body_height) + room * 2.).p(room).mx(-room).overflow_y_scroll().track_scroll(self.content.scroll())
            .child(div().flex().flex_wrap().gap_2().on_children_prepainted(self.content.controls(tabs.iter().cloned().map(Some).collect(), room))
                .child(button("worktree-browse-tab", "Manage", "", !self.creating).track_focus(&tabs[0]).debug_selector(|| "worktree-browse-tab".to_string()).toggled(!self.creating).on_click(cx.listener(|this, _, window, cx| this.switch_form(false, window, cx))))
                .child(button("worktree-create-tab", "Create worktree…", "plus", self.creating).track_focus(&tabs[1]).debug_selector(|| "worktree-create-tab".to_string()).toggled(self.creating).on_click(cx.listener(|this, _, window, cx| this.switch_form(true, window, cx))))
                .child(button("refresh-worktrees", "Refresh", "refresh", false).track_focus(&tabs[2]).debug_selector(|| "refresh-worktrees".to_string()).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx)))))
            .when(self.pending, |element| element.child(div().flex().gap_2().on_children_prepainted(self.content.controls(vec![None, Some(self.cancel_read.handle().clone())], room)).child(label("worktree-loading", "Reading worktree identities and content…").min_w_0().text_size(crate::appearance::ui_text(12.))).child(button("cancel-worktree-read", "Cancel", "", false).track_focus(self.cancel_read.handle()).debug_selector(|| "cancel-worktree-read".to_string()).on_click(cx.listener(|this, _, window, cx| { this.cancel(); this.leave_loading_row(window, cx); cx.notify(); })))))
            .children(self.error.as_ref().map(|error| label("worktree-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .when(!self.creating, |element| element
                // Each first field stands in the ring's room, given back
                // through its margin, so revealing it shows its ring whole.
                .child(div().on_children_prepainted(self.content.control(filter, room)).py(room).my(-room).child(Input::new(&self.filter).aria_label("Filter worktrees by branch or path").cleanable(true)))
                // Past its height, as where a half tile leaves it one row,
                // the list cues the rest with its scrollbar, and a row Tab
                // moves onto scrolls into view with its ring.
                .child(div().debug_selector(|| "managed-worktree-entries".into()).on_children_prepainted(show_list).flex().flex_col().min_h_0().map(|entries| if trees.is_empty() { entries.flex_shrink_0() } else { entries.min_h(rows_floor) }).child(cue.list(div().on_children_prepainted(cue.observe(reveal)).id("managed-worktree-list").debug_selector(|| "managed-worktree-list".into()).max_h(px(190.) + room * 2.).p(room).m(-room).overflow_y_scroll().track_scroll(self.rows.scroll()).flex().flex_col().gap(room + ROW_RING_CLEARANCE).children(trees.iter().take(100).zip(rows).enumerate().map(|(index, (tree, focus))| {
                    let tree = tree.clone(); let selected = self.selected.as_ref() == Some(&tree.path); let text = format!("{} · {}{}{}", tree.branch.as_deref().unwrap_or("Detached HEAD"), tree.path.display(), if tree.locked { " · Locked" } else { "" }, if tree.prunable { " · Missing or stale" } else { "" });
                    worktree_row(index, text, selected, p).track_focus(&focus).on_click(cx.listener(move |this, _, window, cx| this.select(tree.clone(), window, cx)))
                })).when(trees.is_empty() && !self.pending, |element| element.child(div().py_3().ml(TEXT_INSET).child(crate::empty_state::EmptyState::new("worktrees-empty", "No worktrees match this filter").block(cx)))), "managed-worktree-scrollbar").min_h_0()))
                .when(trees.len() > 100, |element| element.child(label("worktrees-match-limit", "Showing 100 matches. Narrow the filter to find another worktree.").text_size(crate::appearance::ui_text(12.))))
                .when_some(self.details.as_ref(), |element, details| {
                    let details = details.clone(); let path = details.tree.path.clone(); let finder = path.clone(); let editor = path.clone();
                    element.child(div().flex().flex_col().gap_2()
                        .child(label("worktree-selected-identity", format!("{}{} · {}\n{}", details.tree.branch.as_deref().unwrap_or("Detached HEAD"), if details.main { " · Main worktree" } else if details.current { " · Current worktree" } else { "" }, short_oid(&details.tree.oid), details.tree.path.display())).text_size(crate::appearance::ui_text(12.)))
                        .child(label("worktree-selected-status", if details.missing { "Folder missing or unavailable".into() } else { format!("{} · {}{}", crate::counted(details.changed_files, "changed/untracked file", "changed/untracked files"), crate::counted(details.ignored_files, "ignored file", "ignored files"), if details.removal_blocked.is_none() { " · Clean" } else { "" }) }).text_size(crate::appearance::ui_text(12.)))
                        .when(details.force_removal_blocked.is_none(), |element| element.when_some(details.removal_blocked.as_ref(), |element, reason| element.child(label("worktree-removal-protection", reason.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))))
                        .child(div().flex().flex_wrap().gap_2().on_children_prepainted(self.content.controls(actions.iter().cloned().map(Some).collect(), room))
                            .child(refusing(button("open-managed-worktree", "Open in GitTurtle", "", false).track_focus(&actions[0]), "open-managed-worktree", details.missing || details.current, details.missing || details.current).on_click(cx.listener(move |this, _, window, cx| { let _ = this.owner.update(cx, |owner, cx| { if owner.operation_busy.is_none() && owner.path.as_deref() == Some(this.repo.path()) { window.close_dialog(cx); owner.open(path.clone(), None, window, cx); } }); })))
                            .child(button("reveal-managed-worktree", if cfg!(target_os = "macos") { "Finder" } else { "Files" }, "", false).track_focus(&actions[1]).disabled(details.missing).on_click(move |_, _, cx| cx.reveal_path(&finder)))
                            .child(refusing(button("edit-managed-worktree", "Editor", "", false).track_focus(&actions[2]).debug_selector(|| "edit-managed-worktree".into()), "edit-managed-worktree", details.missing, details.missing).on_click(cx.listener(move |this, _, window, cx| { if this.unavailable(cx).0 { return; } let _ = this.owner.update(cx, |owner, cx| owner.open_worktree_editor(editor.clone(), window, cx)); })))
                            .child(button("remove-managed-worktree", "Remove worktree…", "", false).track_focus(&actions[3]).debug_selector(|| "remove-managed-worktree".to_string()).map(|remove| refusing(remove, "remove-managed-worktree", details.removal_blocked.is_some(), details.removal_blocked.is_some())).on_click(cx.listener(|this, _, window, cx| this.remove(RemovalMode::Ordinary, window, cx))))
                            .child(button("force-remove-managed-worktree", "Force remove worktree…", "", false).track_focus(&actions[4]).debug_selector(|| "force-remove-managed-worktree".to_string()).when(!dimmed && details.force_removal_blocked.is_none(), |button| button.text_color(rgb(p.removed))).map(|remove| refusing(remove, "force-remove-managed-worktree", details.force_removal_blocked.is_some(), details.force_removal_blocked.is_some())).on_click(cx.listener(|this, _, window, cx| this.remove(RemovalMode::Force, window, cx)))))
                        .when_some(details.force_removal_blocked.as_ref(), |element, reason| element.child(label("worktree-force-removal-protection", format!("Force removal unavailable: {reason}")).debug_selector(|| "worktree-force-removal-protection".into()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))))
                }))
            .when(self.creating, |element| element
                .child(div().flex().gap_2().on_children_prepainted(self.content.controls(kinds.iter().cloned().map(Some).collect(), room)).children([(false, "Existing branch", "worktree-existing-branch"), (true, "New branch", "worktree-new-branch")].into_iter().zip(kinds).map(|((new, name, selector), focus)| button(name, name, "", self.new_branch == new).track_focus(&focus).debug_selector(move || selector.to_string()).toggled(self.new_branch == new).disabled(self.pending).on_click(cx.listener(move |this, _, _, cx| this.set_new_branch(new, cx))))))
                .child(label("worktree-branch-label", if self.new_branch { "New branch name" } else { "Choose a local branch" }).text_size(crate::appearance::ui_text(12.)))
                .child(div().on_children_prepainted(self.content.control(branch_field, room)).py(room).my(-room).child(Input::new(&self.branch).aria_label("Worktree branch name")))
                // A choice Tab moves onto scrolls into the list's view with
                // its ring, and the content shows the list.
                .when(!self.new_branch, |element| element.child(div().debug_selector(|| "worktree-branch-entries".into()).on_children_prepainted(show_choices).flex().flex_col().min_h_0().map(|entries| if choices.is_empty() { entries.flex_shrink_0() } else { entries.min_h(choices_floor) }).child(choice_cue.list(div().on_children_prepainted(choice_cue.observe(reveal_choice)).id("worktree-branch-choices").debug_selector(|| "worktree-branch-choices".into()).max_h(px(140.) + room * 2.).p(room).m(-room).overflow_y_scroll().track_scroll(self.choices.scroll()).flex().flex_col().gap_1().children(choices.iter().zip(choice_focus).enumerate().map(|(index, (branch, focus))| {
                    let name = branch.name.clone(); let occupied = self.trees.iter().any(|tree| tree.branch.as_deref() == Some(&name));
                    button(("worktree-branch-choice", index), format!("{}{}", name, if occupied { " · In use" } else { "" }), "", false).track_focus(&focus).debug_selector(move || format!("worktree-branch-choice-{index}")).disabled(occupied || self.pending).on_click(cx.listener(move |this, _, window, cx| { this.branch.update(cx, |input, cx| input.set_value(name.clone(), window, cx)); cx.notify(); }))
                })).when(choices.is_empty(), |element| element.child(label("worktree-branches-empty", "No local branch matches. Create a new branch or change the name.").text_size(crate::appearance::ui_text(12.)))), "worktree-branch-choices-scrollbar").min_h_0())))
                .when(self.new_branch, |element| element.child(label("worktree-start-label", "Start from local branch, tag, or commit").text_size(crate::appearance::ui_text(12.))).child(div().on_children_prepainted(self.content.control(start_field, room)).child(Input::new(&self.start).aria_label("New worktree starting revision"))))
                // The rest of the form, laid out as the content's own
                // children are, so that its controls report where they lie.
                .child(div().flex().flex_col().gap_3().on_children_prepainted(self.content.controls(vec![None, Some(parent_action.clone()), Some(folder_field), None, Some(review_action.clone())], room))
                    .child(label("worktree-parent", format!("Parent folder: {}", self.parent.display())).text_size(crate::appearance::ui_text(12.)))
                    .child(button("choose-worktree-parent", "Choose parent folder…", "folder", false).track_focus(&parent_action).debug_selector(|| "choose-worktree-parent".to_string()).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.choose_parent(window, cx))))
                    .child(Input::new(&self.folder).aria_label("New worktree folder name"))
                    .child(label("worktree-destination-rule", "Choose a new folder inside the parent. Existing folders and branches checked out elsewhere are protected. Review the resolved branch and destination before creating.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                    .child(refusing(button("review-create-worktree", "Review worktree…", "", false).track_focus(&review_action).debug_selector(|| "review-create-worktree".to_string()), "review-create-worktree", false, false).on_click(cx.listener(|this, _, window, cx| this.prepare_create(window, cx)))))));
        // The content's scrollbar stands in the dialog's side padding, beside
        // the content and its ring room, as the Reflog's does, and paints
        // only while the content overflows.
        div()
            .relative()
            .min_h_0()
            .flex()
            .flex_col()
            .child(content)
            .child(
                div()
                    .debug_selector(|| "worktree-manager-scrollbar".into())
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(-gpui_kit::component::scroll::Scrollbar::width() - room)
                    .w(gpui_kit::component::scroll::Scrollbar::width())
                    .child(
                        gpui_kit::component::scroll::Scrollbar::vertical(self.content.scroll())
                            .id("worktree-manager-scrollbar")
                            .mode(gpui_kit::component::scroll::ScrollbarMode::Always)
                            .viewport_from_layout(),
                    ),
            )
    }
}

#[cfg(test)]
pub(crate) mod tests;

/// Lets the open manager's reads land, the registrations and then the row a
/// targeted refresh inspects, so the narrow-window walk (`width_walk`)
/// measures the manager with its worktrees listed. They run on the
/// manager's own tasks, which `run_until_parked` alone does not finish.
#[cfg(test)]
pub(crate) async fn settle_manager(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let Some(manager) = app.read_with(cx, |app, _| app.worktree_management.draft.clone()) else {
        return;
    };
    for _ in 0..4 {
        cx.executor().run_until_parked();
        let Some(task) = manager.update(cx, |manager, _| manager.task.take()) else {
            break;
        };
        task.await;
    }
    cx.executor().run_until_parked();
    assert!(!manager.read_with(cx, |manager, _| manager.pending));
}

/// The open manager's first field: the filter in Manage, the branch field
/// in Create, for the focus sweep.
#[cfg(test)]
pub(crate) fn first_field(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> FocusHandle {
    let manager = app
        .read_with(cx, |app, _| app.worktree_management.draft.clone())
        .expect("the manager is open");
    manager.read_with(cx, |manager, cx| manager.first_field(cx))
}

/// Shows the open manager as it stands while a read runs, its reading row
/// and Cancel above the list, until the next read lands.
#[cfg(test)]
pub(crate) fn show_reading(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let manager = app
        .read_with(cx, |app, _| app.worktree_management.draft.clone())
        .expect("the manager is open");
    manager.update(cx, |manager, cx| {
        manager.pending = true;
        cx.notify();
    });
}

/// The manager's room for focus rings, on the shared clipping fixtures of the
/// Tags and Reflog dialogs.
#[cfg(test)]
pub(crate) mod ring_room_tests {
    use super::*;
    use crate::tags::tests::{
        assert_rings_whole, assert_room_for_rings, assert_room_moves_nothing, draw, git, rendered,
        scroll_down, tagged_repository, window,
    };
    use ::core::prelude::v1::test;
    use gpui::accesskit::Toggled;
    use gpui_kit::component::Theme;

    /// The manager on `repo`, opened in a window, with its first refresh
    /// finished.
    async fn opened_manager<'a>(
        cx: &'a mut TestAppContext,
        repo: &GitRepository,
    ) -> (
        Entity<GitTurtle>,
        Entity<WorktreeManager>,
        &'a mut VisualTestContext,
    ) {
        let (app, cx) = window(cx, repo);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
        let manager = app.read_with(cx, |app, _| {
            app.worktree_management
                .draft
                .clone()
                .expect("the manager opens")
        });
        // A refresh lists the registrations, then inspects any selected row.
        for _ in 0..4 {
            cx.executor().run_until_parked();
            let Some(task) = manager.update(cx, |manager, _| manager.task.take()) else {
                break;
            };
            task.await;
        }
        draw(cx);
        assert!(!manager.read_with(cx, |manager, _| manager.pending));
        (app, manager, cx)
    }

    /// The dialog clips its body to the body's bounds, and the manager's
    /// content and its list of worktrees scroll, so GPUI clips each to its
    /// bounds on both axes. Manage, Create worktree… and Refresh, at the
    /// content's top edge, and the first and last worktree rows keep their
    /// rings whole, and the room the content and the list keep moves
    /// nothing. Rows stand apart by the ring's room and 2 px more, so a ring
    /// of another size moves every row after the first; the room is therefore
    /// checked with the list filtered to one row.
    #[gpui::test]
    async fn manager_keeps_room_for_every_focus_ring(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for branch in ["first", "second"] {
            let folder = fixture.path().join(branch);
            git(
                repo.path(),
                &[
                    "worktree",
                    "add",
                    "--quiet",
                    "-b",
                    branch,
                    folder.to_str().unwrap(),
                ],
            );
        }
        let (app, manager, cx) = opened_manager(cx, &repo).await;
        assert_eq!(manager.read_with(cx, |manager, _| manager.trees.len()), 3);
        let tabs = [
            "worktree-browse-tab",
            "worktree-create-tab",
            "refresh-worktrees",
        ];
        assert_rings_whole(
            cx,
            &[
                &tabs[..],
                &["managed-worktree-row-0", "managed-worktree-row-2"],
            ]
            .concat(),
        );

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager
                    .filter
                    .update(cx, |input, cx| input.set_value("second", window, cx))
            })
        });
        draw(cx);
        assert!(cx.debug_bounds("managed-worktree-row-1").is_none());
        assert_room_for_rings(
            cx,
            &app,
            &[&tabs[..], &["managed-worktree-row-0"]].concat(),
            &["worktree-manager-title", "managed-worktree-entries"],
        );
    }

    /// The create form's list of branch choices scrolls, so GPUI clips it to
    /// its bounds on both axes, inside the manager's scrolling content.
    /// Scrolled to either end, the choice at that end keeps its ring whole,
    /// and nothing moves.
    #[gpui::test]
    async fn branch_choices_keep_room_for_rings_at_either_end(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for index in 0..11 {
            git(repo.path(), &["branch", &format!("feature-{index:02}")]);
        }
        // With no branch checked out, every choice is available, so each
        // paints the hovered fill that shows the mask it paints in.
        git(repo.path(), &["checkout", "--quiet", "--detach"]);
        let (app, manager, cx) = opened_manager(cx, &repo).await;
        manager.update(cx, |manager, cx| {
            manager.set_creating(true, cx);
            manager.new_branch = false;
            cx.notify();
        });
        draw(cx);

        // Main and eleven features: the twelve choices the form shows.
        let (first, last) = ("worktree-branch-choice-0", "worktree-branch-choice-11");
        let fixed = [
            "worktree-manager-title",
            "worktree-branch-label",
            "worktree-branch-entries",
            "worktree-parent",
        ];
        let list = rendered(cx, "worktree-branch-choices");
        assert!(
            rendered(cx, last).bottom() > list.bottom(),
            "the list is long enough to scroll"
        );
        assert_rings_whole(cx, &[first]);
        assert_room_moves_nothing(cx, &app, &[first], &fixed);

        scroll_down(
            cx,
            list.center(),
            ScrollDelta::Pixels(point(px(0.), px(-10_000.))),
        );
        let list = rendered(cx, "worktree-branch-choices");
        assert!(
            rendered(cx, first).top() < list.top(),
            "the list scrolls to its end"
        );
        assert_rings_whole(cx, &[last]);
        assert_room_moves_nothing(cx, &app, &[last], &fixed);
    }

    /// Four worktrees on `fixture`: the tagged repository's main worktree and
    /// three linked ones.
    fn four_worktrees(fixture: &std::path::Path) -> GitRepository {
        let repo = tagged_repository(fixture);
        for branch in ["first", "second", "third"] {
            let folder = fixture.join(branch);
            git(
                repo.path(),
                &[
                    "worktree",
                    "add",
                    "--quiet",
                    "-b",
                    branch,
                    folder.to_str().unwrap(),
                ],
            );
        }
        repo
    }

    /// Applies `interface` as the interface text size, as Settings does.
    fn set_interface_text_size(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, interface: u8) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                let code = app.settings.code_text_size;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        draw(cx);
    }

    /// Moves the pointer onto the dialog's backdrop, where it hovers nothing.
    fn park_pointer(cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        draw(cx);
    }

    /// The room the installed Button focus ring takes outside an edge.
    fn ring_footprint(cx: &mut VisualTestContext) -> Pixels {
        cx.read(|cx| {
            let ring = Theme::global(cx).button_focus_ring;
            ring.gap + ring.width
        })
    }

    /// Moves keyboard focus along the window's tab order until the Button at
    /// `selector` draws its focus ring.
    fn focus_button(cx: &mut VisualTestContext, selector: &'static str) {
        for _ in 0..64 {
            cx.update(|window, cx| window.focus_next(cx));
            draw(cx);
            let bounds = rendered(cx, selector);
            if !appearance::painted_button(cx, bounds).1.is_empty() {
                return;
            }
        }
        panic!("keyboard focus never reaches {selector}");
    }

    /// The bounds of every outline the last frame drew in `color` on or around
    /// `element`: border-only or bordered quads lying at its edge or at the
    /// installed ring's footprint, each counted once although GPUI paints a
    /// border-only quad once per side.
    fn outlines(
        cx: &mut VisualTestContext,
        element: Bounds<Pixels>,
        color: Hsla,
    ) -> Vec<Bounds<Pixels>> {
        let footprint = ring_footprint(cx);
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let device = px(1. / scale);
            let logical = |value: ScaledPixels| px(value.as_f32() / scale);
            let near = |drawn: Bounds<Pixels>, expected: Bounds<Pixels>| {
                [
                    (drawn.left(), expected.left()),
                    (drawn.top(), expected.top()),
                    (drawn.right(), expected.right()),
                    (drawn.bottom(), expected.bottom()),
                ]
                .into_iter()
                .all(|(edge, expected)| (edge - expected).abs() <= device)
            };
            let mut found: Vec<Bounds<Pixels>> = Vec::new();
            for quad in window.painted_quads() {
                let widths = quad.border_widths;
                let bordered = [widths.top, widths.right, widths.bottom, widths.left]
                    .into_iter()
                    .any(|width| width.as_f32() > 0.);
                let drawn = Bounds::from_corners(
                    point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                    point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                );
                let around = near(drawn, element) || near(drawn, element.dilate(footprint));
                if bordered && around && quad.border_color == color && !found.contains(&drawn) {
                    found.push(drawn);
                }
            }
            found
        })
    }

    /// Rows stand apart by the ring's room and 2 px more at the smallest,
    /// default and largest tested interface text sizes. Each row, focused,
    /// draws its ring at its bounds grown by the installed ring's gap plus
    /// width, and that footprint ends at least 2 px before the next row's
    /// border and begins at least 2 px after the previous row's; scrolled to
    /// the end of the list that shows it whole, it lies inside the content
    /// mask the row paints in, the intersection of every ancestor's.
    #[gpui::test]
    async fn focused_worktree_rows_keep_clear_of_the_next_row(cx: &mut TestAppContext) {
        const ROWS: [&str; 4] = [
            "managed-worktree-row-0",
            "managed-worktree-row-1",
            "managed-worktree-row-2",
            "managed-worktree-row-3",
        ];
        let fixture = tempfile::tempdir().unwrap();
        let repo = four_worktrees(fixture.path());
        let (app, manager, cx) = opened_manager(cx, &repo).await;
        assert_eq!(manager.read_with(cx, |manager, _| manager.trees.len()), 4);
        let footprint = ring_footprint(cx);
        assert_eq!(footprint, px(3.), "the default ring is installed");
        let ring = cx.read(|cx| Theme::global(cx).ring);
        let clear = |clearance: Pixels| clearance >= ROW_RING_CLEARANCE - px(0.01);

        for size in [11_u8, 13, 18] {
            set_interface_text_size(cx, &app, size);
            // The rows whose rings the list shows at the top of its scroll,
            // then, scrolled to its end, the rest.
            let mut whole = Vec::new();
            for end in [false, true] {
                if end {
                    let list = rendered(cx, "managed-worktree-list");
                    scroll_down(
                        cx,
                        list.center(),
                        ScrollDelta::Pixels(point(px(0.), px(-10_000.))),
                    );
                }
                let list = rendered(cx, "managed-worktree-list");
                let shown: Vec<_> = ROWS
                    .into_iter()
                    .filter(|row| !whole.contains(row))
                    .filter(|row| {
                        let ring = rendered(cx, row).dilate(footprint);
                        ring.top() >= list.top() && ring.bottom() <= list.bottom()
                    })
                    .collect();
                assert_rings_whole(cx, &shown);
                for &row in &shown {
                    focus_button(cx, row);
                    park_pointer(cx);
                    let bounds = rendered(cx, row);
                    let (_, rings) = appearance::painted_button(cx, bounds);
                    assert_eq!(
                        rings,
                        [ring],
                        "at {size} pt the focused {row} draws {rings:?}"
                    );
                    let drawn = bounds.dilate(footprint);
                    let index = ROWS.iter().position(|other| *other == row).unwrap();
                    if let Some(&next) = ROWS.get(index + 1) {
                        let border = rendered(cx, next).top();
                        let clearance = border - drawn.bottom();
                        assert!(
                            clear(clearance),
                            "at {size} pt the ring around {row} ends at {:?}, \
                             {clearance:?} before the next row's border at {border:?}",
                            drawn.bottom()
                        );
                    }
                    if let Some(&previous) = index.checked_sub(1).and_then(|at| ROWS.get(at)) {
                        let border = rendered(cx, previous).bottom();
                        let clearance = drawn.top() - border;
                        assert!(
                            clear(clearance),
                            "at {size} pt the ring around {row} begins at {:?}, \
                             {clearance:?} after the previous row's border at {border:?}",
                            drawn.top()
                        );
                    }
                }
                whole.extend(shown);
            }
            assert_eq!(
                whole.len(),
                ROWS.len(),
                "at {size} pt only {whole:?} show whole"
            );
            let list = rendered(cx, "managed-worktree-list");
            scroll_down(
                cx,
                list.center(),
                ScrollDelta::Pixels(point(px(0.), px(10_000.))),
            );
        }
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    /// A selected worktree row shows its selection through the selected
    /// surface and its selected and toggled state, and keeps the palette's
    /// neutral border, so the only accent outline around it is the focus
    /// ring, drawn once when it is focused.
    #[gpui::test]
    async fn a_selected_worktree_row_draws_one_accent_outline(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = four_worktrees(fixture.path());
        let (_app, manager, cx) = opened_manager(cx, &repo).await;
        manager.update(cx, |manager, cx| {
            manager.selected = Some(manager.trees[1].path.clone());
            cx.notify();
        });
        park_pointer(cx);
        let (p, ring) = cx.read(|cx| (palette(cx), Theme::global(cx).ring));
        let color = |value: u32| Hsla::from(rgb(value));
        let selected_fill = gpui_kit::Background::from(color(p.selected));
        let row = rendered(cx, "managed-worktree-row-1");
        let footprint = row.dilate(ring_footprint(cx));

        let (fills, rings) = appearance::painted_button(cx, row);
        assert_eq!(fills, [selected_fill], "the selected row paints {fills:?}");
        assert!(rings.is_empty(), "the unfocused row draws {rings:?}");
        assert_eq!(
            outlines(cx, row, color(p.border)),
            [row],
            "the border is neutral"
        );
        assert!(outlines(cx, row, color(p.accent)).is_empty());
        assert!(outlines(cx, row, ring).is_empty());

        focus_button(cx, "managed-worktree-row-1");
        park_pointer(cx);
        let (fills, rings) = appearance::painted_button(cx, row);
        assert_eq!(
            fills,
            [selected_fill],
            "the focused selected row paints {fills:?}"
        );
        assert_eq!(rings, [ring], "the focused selected row draws {rings:?}");
        assert_eq!(
            outlines(cx, row, color(p.border)),
            [row],
            "the border stays neutral"
        );
        let accent = [color(p.accent), ring]
            .into_iter()
            .flat_map(|accent| outlines(cx, row, accent))
            .fold(Vec::new(), |mut all, outline| {
                if !all.contains(&outline) {
                    all.push(outline);
                }
                all
            });
        assert_eq!(accent.len(), 1, "one accent outline, not {accent:?}");
        let near = |a: Bounds<Pixels>, b: Bounds<Pixels>| {
            (a.left() - b.left()).abs() < px(0.01)
                && (a.top() - b.top()).abs() < px(0.01)
                && (a.right() - b.right()).abs() < px(0.01)
                && (a.bottom() - b.bottom()).abs() < px(0.01)
        };
        assert!(
            near(accent[0], footprint),
            "the accent outline {accent:?} is the ring"
        );

        // The row keeps the kit's selected state, and the node its Button
        // writes reports the selection as the row's toggled state. The kit
        // gives AccessKit's selected state to tab roles only, so a row's
        // node, selected or not, reports none.
        assert!(worktree_row(1, "second".into(), true, p).is_selected());
        let rows = [(1, true, Toggled::True), (0, false, Toggled::False)];
        let text = |index: usize| format!("worktree row {index}");
        let nodes = row_nodes(
            cx,
            rows.map(|(index, selected, _)| worktree_row(index, text(index), selected, p))
                .into(),
        );
        assert_eq!(nodes.len(), rows.len());
        for ((index, selected, toggled), node) in rows.into_iter().zip(&nodes) {
            assert_eq!(node.role(), Role::Button);
            assert_eq!(node.label(), Some(text(index).as_str()));
            assert_eq!(
                node.toggled(),
                Some(toggled),
                "the node of a row with selected {selected}"
            );
            assert_eq!(
                node.is_selected(),
                None,
                "the node of a row with selected {selected}"
            );
        }
    }

    /// The accessibility nodes the kit's Button writes for `rows`: the role
    /// and properties a window's tree gives each while assistive technology
    /// is active, less its bounds. The test platform never turns
    /// accessibility on, so no frame builds that tree; like the rendered
    /// Button checks in `native_accessibility`, a probe view renders each
    /// row, while it is prepainted, to the element that writes its node, and
    /// asks that element. The kit's Button renders a base Button, which GPUI
    /// renders in turn when it is laid out, to an element type of the base's
    /// own; a base Button rendered beside it names that type.
    pub(crate) fn row_nodes(
        cx: &mut VisualTestContext,
        rows: Vec<Button>,
    ) -> Vec<gpui::accesskit::Node> {
        use gpui::Element as _;
        type Nodes = std::rc::Rc<std::cell::RefCell<Vec<gpui::accesskit::Node>>>;
        /// The element `element` renders when it is laid out.
        fn laid_out<E: gpui::Element>(
            mut element: E,
            window: &mut Window,
            cx: &mut App,
        ) -> Option<AnyElement> {
            let (_, mut layout) = element.request_layout(None, None, window, cx);
            (&mut layout as &mut dyn std::any::Any)
                .downcast_mut::<Option<AnyElement>>()?
                .take()
        }
        /// `element`, if it is of `witness`'s type.
        fn of_type<'a, E: gpui::Element>(
            _witness: &E,
            element: &'a mut AnyElement,
        ) -> Option<&'a mut E> {
            element.downcast_mut()
        }
        fn node(row: Button, window: &mut Window, cx: &mut App) -> gpui::accesskit::Node {
            let outer = RenderOnce::render(row, window, cx).into_element();
            let mut base =
                laid_out(outer, window, cx).expect("the kit's Button renders a base Button");
            let witness =
                RenderOnce::render(gpui_kit::base::Button::new("row-node-witness"), window, cx)
                    .into_element();
            let element = of_type(&witness, &mut base)
                .expect("a base Button renders the element that writes its node");
            let mut node =
                gpui::accesskit::Node::new(element.a11y_role().expect("the row has a role"));
            element.write_a11y_info(&mut node);
            node
        }
        struct Probe {
            rows: Vec<Button>,
            nodes: Nodes,
        }
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let rows = std::mem::take(&mut self.rows);
                let nodes = self.nodes.clone();
                canvas(
                    move |_, window, cx| {
                        let rendered = rows.into_iter().map(|row| node(row, window, cx));
                        nodes.borrow_mut().extend(rendered);
                    },
                    |_, _, _, _| {},
                )
                .size_full()
            }
        }
        let nodes = Nodes::default();
        let (_, probe) = cx.add_window_view({
            let nodes = nodes.clone();
            move |_, _| Probe { rows, nodes }
        });
        probe.update(|window, cx| window.draw(cx).clear(cx));
        nodes.take()
    }

    /// The manager's text starts at one inset, where its filter's text and
    /// placeholder start, at 13 and 18 pt in a 1480 × 980 window and a
    /// 461 × 490 half tile: its title, each worktree row's name and the
    /// selected worktree's details. A row's name was centred in its row,
    /// and the other lines started at the content's edge.
    #[gpui::test]
    async fn manager_text_starts_at_the_filters_inset(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = four_worktrees(fixture.path());
        let (app, manager, cx) = opened_manager(cx, &repo).await;
        let filter = manager.read_with(cx, |manager, _| manager.filter.clone());
        for (width, height) in [(1480., 980.), (461., 490.)] {
            for size in [13, 18] {
                cx.simulate_resize(gpui::size(px(width), px(height)));
                set_interface_text_size(cx, &app, size);
                park_pointer(cx);
                let at = format!("{size} pt in {width} × {height}");
                let text = cx.read(|cx| filter.read(cx).input_bounds().left());
                for selector in [
                    "worktree-manager-title",
                    "managed-worktree-row-text-0",
                    "managed-worktree-row-text-3",
                ] {
                    let start = rendered(cx, selector).left();
                    assert!(
                        (start - text).abs() < px(0.01),
                        "{at}: {selector} starts at {start:?}, the filter's text at {text:?}"
                    );
                }
            }
        }
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    /// In a 461 × 490 half tile, with a worktree selected and its details
    /// below the list, the manager's list shows both of two worktrees whole
    /// at 13 and 18 pt: it kept one, with nothing to cue the other.
    #[gpui::test]
    async fn a_half_tile_keeps_two_worktree_rows_whole(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let folder = fixture.path().join("second");
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "second",
                folder.to_str().unwrap(),
            ],
        );
        let (app, manager, cx) = opened_manager(cx, &repo).await;
        let tree = manager.read_with(cx, |manager, _| manager.trees[1].clone());
        cx.update(|window, cx| manager.update(cx, |manager, cx| manager.select(tree, window, cx)));
        for _ in 0..4 {
            cx.executor().run_until_parked();
            let Some(task) = manager.update(cx, |manager, _| manager.task.take()) else {
                break;
            };
            task.await;
        }
        cx.simulate_resize(gpui::size(px(461.), px(490.)));
        for size in [13, 18] {
            set_interface_text_size(cx, &app, size);
            park_pointer(cx);
            assert!(
                cx.debug_bounds("worktree-selected-identity").is_some(),
                "at {size} pt a worktree's details are shown"
            );
            let list = rendered(cx, "managed-worktree-list");
            for selector in ["managed-worktree-row-0", "managed-worktree-row-1"] {
                let row = rendered(cx, selector);
                assert!(
                    row.top() >= list.top() - px(0.01) && row.bottom() <= list.bottom() + px(0.01),
                    "at {size} pt {selector} {row:?} lies whole in the list {list:?}"
                );
            }
            // The details run past the content's fold, which its scrollbar
            // cues beside it, clear of the content and its rings' room.
            let content = rendered(cx, "worktree-manager-content");
            let track = rendered(cx, "worktree-manager-scrollbar");
            assert!(
                rendered(cx, "worktree-selected-identity").bottom() > content.bottom()
                    || cx
                        .debug_bounds("remove-managed-worktree")
                        .is_none_or(|remove| remove.bottom() > content.bottom()),
                "at {size} pt the details overflow the content {content:?}"
            );
            let thumb = crate::tags::tests::painted_thumb(cx, track).unwrap_or_else(|| {
                panic!("at {size} pt the content's scrollbar {track:?} paints a thumb")
            });
            assert!(
                thumb.left() >= content.right() && track.left() >= content.right() - px(0.01),
                "at {size} pt the thumb {thumb:?} stands beside the content {content:?}"
            );
        }
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    /// In a 461 × 490 half tile four worktrees overflow the manager's list
    /// at 13 and 18 pt, and its scrollbar's thumb follows the list's offset
    /// in a gutter clear of every row's ring; the list cued nothing.
    #[gpui::test]
    async fn a_short_worktree_list_cues_rows_beyond_its_edge(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = four_worktrees(fixture.path());
        let (app, manager, cx) = opened_manager(cx, &repo).await;
        cx.simulate_resize(gpui::size(px(461.), px(490.)));
        draw(cx);
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let scroll = manager.read_with(cx, |manager, _| manager.rows.scroll().clone());
        let rows = crate::tags::tests::selectors("managed-worktree-row", 4);
        crate::tags::tests::assert_scroll_cue(
            cx,
            &app,
            &scroll,
            "managed-worktree-list",
            room,
            &rows,
        );
    }

    /// Create's twelve branch choices overflow their list in a 461 px wide
    /// window and in a 1480 × 980 one, at 13 and 18 pt, and their
    /// scrollbar's thumb follows the list's offset in a gutter clear of
    /// every choice's ring, as the worktree list's does; the choices cued
    /// nothing. With one choice, the list keeps no gutter and draws none.
    #[gpui::test]
    async fn branch_choices_cue_choices_beyond_their_edge(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for index in 0..11 {
            git(repo.path(), &["branch", &format!("feature-{index:02}")]);
        }
        git(repo.path(), &["checkout", "--quiet", "--detach"]);
        let (app, manager, cx) = opened_manager(cx, &repo).await;
        manager.update(cx, |manager, cx| {
            manager.set_creating(true, cx);
            manager.set_new_branch(false, cx);
        });
        draw(cx);
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let scroll = manager.read_with(cx, |manager, _| manager.choices.scroll().clone());
        let choices = crate::tags::tests::selectors("worktree-branch-choice", 12);
        // In a 461 × 490 half tile at 18 pt the list lies below the
        // content's fold until a choice Tab moves onto reveals it, so its
        // thumb is checked in a window as narrow and taller.
        for (width, height) in [(461., 900.), (1480., 980.)] {
            cx.simulate_resize(gpui::size(px(width), px(height)));
            draw(cx);
            crate::tags::tests::assert_scroll_cue(
                cx,
                &app,
                &scroll,
                "worktree-branch-choices",
                room,
                &choices,
            );
        }
        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager
                    .branch
                    .update(cx, |input, cx| input.set_value("feature-07", window, cx))
            })
        });
        draw(cx);
        draw(cx);
        crate::tags::tests::assert_no_scroll_cue(
            cx,
            &app,
            "worktree-branch-choices",
            room,
            &["worktree-branch-choice-0"],
        );
    }
}
