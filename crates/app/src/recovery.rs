//! Explicit, prepared recovery actions and a read-only stash inspector.
use crate::*;
use gitturtle_core::{
    CommitRecoveryPlan, RecoveryCommand, RecoveryKind, StashPage, StashSnapshot, WriteCommand,
};
use gpui_kit::component::{
    WindowExt,
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;

const PREPARING: &str = "Preparing recovery action…";
const STASH_PAGE_SIZE: usize = 40;
/// The least height a saved file's comparison keeps in the Stashes detail,
/// at the default interface size, where the window leaves the detail less
/// than its inspection and it scrolls: about six lines of a patch.
const STASH_CONTENT_FLOOR: f32 = 120.;
type Failure = Box<dyn FnOnce(String, &mut Window, &mut Context<GitTurtle>)>;

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
    amend: Option<Entity<AmendForm>>,
    stash: Option<Entity<StashForm>>,
}

pub(super) struct CommitTarget {
    oid: String,
    parents: Vec<String>,
}

pub(super) fn commit_target(commit: &Commit) -> Arc<CommitTarget> {
    Arc::new(CommitTarget {
        oid: commit.oid.clone(),
        parents: commit.parents.clone(),
    })
}

enum Prepared {
    Amend(Arc<CommitRecoveryPlan>),
    Confirmation {
        title: String,
        explanation: String,
        action: &'static str,
        command: Arc<RecoveryCommand>,
    },
}

impl GitTurtle {
    pub(super) fn cancel_recovery_read(&mut self) {
        self.recovery.generation = self.recovery.generation.wrapping_add(1);
        self.recovery.task = None;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }

    fn read_recovery(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<Prepared> + Send + 'static,
        failure: Option<Failure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return false;
        }
        let Some(repo) = self.repository.clone() else {
            return false;
        };
        let path = repo.path().to_owned();
        self.cancel_recovery_read();
        let generation = self.recovery.generation;
        self.operation_busy = Some(PREPARING);
        self.operation_error = None;
        let response = self.operations.submit_read(move || prepare(repo));
        self.recovery.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Recovery preparation stopped. Open the action again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.recovery.generation != generation {
                    return;
                }
                this.recovery.task = None;
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                if this.path.as_ref() != Some(&path) || this.page != AppPage::Repository {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Prepared::Amend(plan)) => this.show_amend_form(plan, window, cx),
                    Ok(Prepared::Confirmation {
                        title,
                        explanation,
                        action,
                        command,
                    }) => {
                        this.confirm_git_write(
                            title,
                            explanation,
                            action,
                            WriteCommand::Recovery(command),
                            window,
                            cx,
                        );
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        if let Some(failure) = failure {
                            failure(message, window, cx);
                        } else {
                            this.operation_error = Some(message);
                        }
                    }
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
        true
    }

    /// The Changes header's Actions menu, opened by `working_actions_trigger`
    /// (`compact` in an inspector too narrow for its label beside Refresh,
    /// `workspace::WorkingForms`). `tight`, in an inspector too
    /// short for the selection controls (`workspace::WorkingFit`), the menu
    /// leads with Directories and the selection's Stage and Unstage, and
    /// with Refresh where it holds `Some(true)`, as Refresh then has no
    /// control of its own.
    pub(super) fn render_working_recovery_menu(
        &self,
        compact: bool,
        tight: Option<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (staged, unstaged) = self.selection_counts();
        let grouped = self.working_selection.grouped;
        let selection_unavailable = self.operation_busy.is_some()
            || self.working_paths.pending
            || self.working_paths.error.is_some();
        let refreshing = self.status_task.is_some();
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let head = self
            .work_status
            .as_ref()
            .and_then(|status| status.head.clone());
        let has_changes = self
            .work_status
            .as_ref()
            .is_some_and(|status| !status.entries.is_empty());
        let paused = self.integration_state.is_some();
        let ignore_entry = self
            .working_selected
            .and_then(|(index, area)| {
                (area == gitturtle_core::ChangeArea::Unstaged).then_some(index)
            })
            .and_then(|index| self.work_status.as_ref()?.entries.get(index))
            .filter(|entry| entry.untracked)
            .cloned();
        working_actions_trigger(compact, tight.is_some())
            .disabled(self.operation_shown().is_some())
            .app_menu(move |mut menu, _, cx| {
                // Every item refuses while a write runs (`working_item`), so
                // each is disabled then from the moment the write starts,
                // though the trigger opens the menu until the write looks
                // busy: an assistive technology heard each one available.
                let busy = owner
                    .upgrade()
                    .is_none_or(|owner| owner.read(cx).operation_busy.is_some());
                if let Some(refresh) = tight {
                    menu = menu
                        .item(
                            working_item(
                                "Directories",
                                busy,
                                &owner,
                                &repository,
                                |this, window, cx| {
                                    this.working_selection.grouped =
                                        !this.working_selection.grouped;
                                    this.filter_working_paths(window, cx);
                                },
                            )
                            .checked(grouped),
                        )
                        .item(working_item(
                            format!("Stage {unstaged} selected"),
                            unstaged == 0 || selection_unavailable || busy,
                            &owner,
                            &repository,
                            |this, window, cx| this.apply_selected_files(false, window, cx),
                        ))
                        .item(working_item(
                            format!("Unstage {staged} selected"),
                            staged == 0 || selection_unavailable || busy,
                            &owner,
                            &repository,
                            |this, window, cx| this.apply_selected_files(true, window, cx),
                        ));
                    if refresh {
                        menu = menu.item(working_item(
                            "Refresh working changes",
                            refreshing || busy,
                            &owner,
                            &repository,
                            |this, window, cx| this.refresh_worktree(window, cx),
                        ));
                    }
                    menu = menu.separator();
                }
                let ignore_entry = ignore_entry.clone();
                let menu = working_menu_item(
                    menu,
                    "Ignore selected untracked file…",
                    ignore_entry.is_none() || busy,
                    &owner,
                    &repository,
                    move |this, window, cx| {
                        if let Some(entry) = &ignore_entry {
                            this.open_ignore(entry.clone(), window, cx);
                        }
                    },
                );
                let menu = working_menu_item(
                    menu,
                    "Save changes to stash…",
                    !has_changes || paused || busy,
                    &owner,
                    &repository,
                    |this, window, cx| this.open_stash_form(window, cx),
                );
                let menu = working_menu_item(
                    menu,
                    "Browse stashes…",
                    busy,
                    &owner,
                    &repository,
                    |this, window, cx| this.open_stashes(window, cx),
                );
                let Some(head) = &head else {
                    return menu;
                };
                let amend = head.clone();
                let undo = head.clone();
                let menu = working_menu_item(
                    menu.separator(),
                    "Amend last commit…",
                    paused || busy,
                    &owner,
                    &repository,
                    move |this, window, cx| {
                        this.prepare_commit_recovery(
                            RecoveryKind::Amend,
                            amend.clone(),
                            None,
                            window,
                            cx,
                        )
                    },
                );
                working_menu_item(
                    menu,
                    "Undo last local commit…",
                    paused || busy,
                    &owner,
                    &repository,
                    move |this, window, cx| {
                        this.prepare_commit_recovery(
                            RecoveryKind::Undo,
                            undo.clone(),
                            None,
                            window,
                            cx,
                        )
                    },
                )
            })
            // A write's start or end builds the open menu again.
            .follows_writes(cx.entity().downgrade())
            .into_any_element()
    }

    /// The selected commit's Actions menu, at the trailing edge of the
    /// message toolbar's copy controls (`commit_message::Toolbar`), labelled
    /// where the toolbar fits its label and caret there.
    pub(super) fn render_commit_recovery_menu(
        &self,
        commit: &Commit,
        labelled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let commit = commit_target(commit);
        commit_actions_trigger(labelled)
            .ml_auto()
            .disabled(self.operation_shown().is_some())
            .app_menu(move |menu, _, cx| {
                Self::recovery_context_menu(menu, &owner, &repository, &commit, cx)
            })
            // A write's start or end builds the open menu again.
            .follows_writes(cx.entity().downgrade())
            .into_any_element()
    }

    pub(super) fn recovery_context_menu(
        menu: PopupMenu,
        owner: &WeakEntity<Self>,
        repository: &Option<PathBuf>,
        commit: &Arc<CommitTarget>,
        cx: &App,
    ) -> PopupMenu {
        let Some(this) = owner.upgrade() else {
            return menu;
        };
        let (disabled, current) = this.read(cx).commit_menu_state(commit);
        commit_menu(menu, owner, repository, commit, disabled, current)
    }

    /// The commit's menu as a right-click builds it, for the keyboard to
    /// open below the selected row: built from the window's state now,
    /// since the menu is built while the window is being updated.
    pub(super) fn keyboard_commit_menu(
        &self,
        owner: WeakEntity<Self>,
        repository: Option<PathBuf>,
        commit: Arc<CommitTarget>,
    ) -> impl FnOnce(PopupMenu) -> PopupMenu + 'static {
        let (disabled, current) = self.commit_menu_state(&commit);
        move |menu| commit_menu(menu, &owner, &repository, &commit, disabled, current)
    }

    /// Whether a commit's menu offers nothing while a write or an
    /// integration runs, and whether the commit is HEAD, which adds Amend
    /// and Undo.
    fn commit_menu_state(&self, commit: &CommitTarget) -> (bool, bool) {
        let disabled = self.operation_busy.is_some() || self.integration_state.is_some();
        let current = self
            .work_status
            .as_ref()
            .and_then(|status| status.head.as_ref())
            == Some(&commit.oid);
        (disabled, current)
    }

    fn choose_commit_recovery(
        &mut self,
        kind: RecoveryKind,
        commit: Arc<CommitTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(kind, RecoveryKind::Revert | RecoveryKind::CherryPick)
            && commit.parents.len() > 1
        {
            let owner = cx.entity().downgrade();
            let repository = self.path.clone();
            window.open_alert_dialog(cx, move |dialog, window, _| {
                dialog.title("Choose the mainline parent").width(appearance::dialog_width(px(560.), window)).max_h(appearance::dialog_max_height(window))
                    .child(div().text_size(crate::appearance::ui_text(12.)).child(format!("{} is a merge commit. Choose the parent against which its change should be calculated.", short_oid(&commit.oid))))
                    .child(crate::native_accessibility::clips_children(div().id("recovery-mainline-parents").max_h(px(320.)).overflow_y_scroll().flex().flex_col().gap_2()
                        .children(commit.parents.iter().enumerate().take(128).map(|(index, parent)| {
                            let owner = owner.clone(); let repository = repository.clone(); let oid = commit.oid.clone();
                            button(("recovery-mainline", index), format!("Parent {} · {}", index + 1, short_oid(parent)), "", false)
                                .w_full().on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        if this.path == repository && this.page == AppPage::Repository {
                                            window.close_dialog(cx);
                                            this.prepare_commit_recovery(kind, oid.clone(), Some(index + 1), window, cx);
                                        }
                                    });
                                })
                        }))))
                    .button_props(DialogButtonProps::default().ok_text("Cancel"))
            });
        } else {
            self.prepare_commit_recovery(kind, commit.oid.clone(), None, window, cx);
        }
    }

    fn prepare_commit_recovery(
        &mut self,
        kind: RecoveryKind,
        oid: String,
        mainline: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_recovery(move |repo| {
            let plan = repo.recovery_plan(kind, Some(&oid), mainline)?;
            anyhow::ensure!(plan.target.oid == oid, "The selected commit changed. Open its actions again.");
            if kind == RecoveryKind::Amend { return Ok(Prepared::Amend(Arc::new(plan))); }
            let (title, action, command, consequence) = match kind {
                RecoveryKind::Undo => (
                    "Undo last local commit", "Undo local commit", RecoveryCommand::Undo { plan: plan.clone() },
                    "Move the current branch back one commit while preserving the exact index and working files. The undone commit remains staged relative to the previous commit; your existing unstaged edits remain available.",
                ),
                RecoveryKind::Revert => (
                    "Revert commit", "Create revert commit", RecoveryCommand::Revert { plan: plan.clone() },
                    "Create a new commit that reverses this commit's change. Existing commits remain in history. Conflicts pause the operation for review in Changes.",
                ),
                RecoveryKind::CherryPick => (
                    "Cherry-pick commit", "Cherry-pick commit", RecoveryCommand::CherryPick { plan: plan.clone() },
                    "Apply this commit's change to the current branch as a new commit. Conflicts pause the operation for review in Changes.",
                ),
                RecoveryKind::Amend => unreachable!(),
            };
            let mut explanation = format!("{} · {}\n\nTarget branch: {} at {}\n\n{consequence}", short_oid(&plan.target.oid), plan.target.subject, plan.current.branch, short_oid(&plan.current.head));
            if let Some(parent) = plan.mainline { explanation.push_str(&format!("\n\nMainline: parent {parent} · {}", short_oid(&plan.target.parents[parent - 1]))); }
            append_paths(&mut explanation, &plan.affected_paths);
            if kind != RecoveryKind::Undo { explanation.push_str("\n\nGit uses its configured hooks and signing. This action does not contact a remote."); }
            Ok(Prepared::Confirmation { title: title.into(), explanation, action, command: command.into() })
        }, None, window, cx);
    }

    fn show_amend_form(
        &mut self,
        plan: Arc<CommitRecoveryPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.path.clone();
        let reuse = self
            .recovery
            .amend
            .as_ref()
            .filter(|form| {
                let form = form.read(cx);
                form.repository == path && form.plan.target.oid == plan.target.oid
            })
            .cloned();
        let form = if let Some(form) = reuse {
            form.update(cx, |form, _| form.plan = Arc::clone(&plan));
            form
        } else {
            let owner = cx.entity().downgrade();
            cx.new(|cx| AmendForm::new(owner, path, plan, window, cx))
        };
        self.recovery.amend = Some(form.clone());
        show_amend(form, window, cx);
    }

    fn open_stash_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() {
            return;
        }
        let path = self.path.clone();
        let form = self
            .recovery
            .stash
            .as_ref()
            .filter(|form| form.read(cx).repository == path)
            .cloned()
            .unwrap_or_else(|| {
                let owner = cx.entity().downgrade();
                cx.new(|cx| StashForm::new(owner, path, window, cx))
            });
        self.recovery.stash = Some(form.clone());
        show_stash_form(form, window, cx);
    }

    pub(super) fn open_stashes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_stash_browser(window, cx);
    }

    /// Opens Stashes, as [`Self::open_stashes`] does, and returns its
    /// browser, which a test waits on for its reads (`StashBrowser::lands`).
    pub(super) fn open_stash_browser(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<StashBrowser>> {
        if self.operation_busy.is_some() {
            return None;
        }
        let repo = self.repository.clone()?;
        let owner = cx.entity().downgrade();
        let browser = cx.new(|cx| StashBrowser::new(owner, repo, window, cx));
        let opened = browser.clone();
        window.open_alert_dialog(cx, move |dialog, window, _| {
            dialog
                .title(native_accessibility::dialog_title(
                    "stashes-title",
                    "Stashes",
                ))
                .width(appearance::dialog_width(px(1060.), window))
                .max_h(appearance::dialog_max_height(window))
                .button_props(DialogButtonProps::default().ok_text("Close"))
                // In the dialog's content rather than its scrolling body,
                // whose scroller lays the browser out at the height it asks
                // for and cut its lines mid-line where the cap left less:
                // here the browser gives up what the cap takes.
                .content({
                    let browser = browser.clone();
                    move |content, _, _| content.min_h_0().child(browser.clone())
                })
        });
        Some(opened)
    }

    pub(super) fn finish_recovery_write(
        &mut self,
        repository: &PathBuf,
        command: &RecoveryCommand,
        succeeded: bool,
        cx: &mut Context<Self>,
    ) {
        match command {
            RecoveryCommand::Amend { message, .. } => {
                if let Some(form) = &self.recovery.amend
                    && form.read(cx).repository.as_ref() == Some(repository)
                {
                    if succeeded && form.read(cx).message(cx) == *message {
                        self.recovery.amend = None;
                    } else if !succeeded {
                        form.update(cx, |form, cx| {
                            form.error = self.tab_operation_error(repository);
                            cx.notify();
                        });
                    }
                }
            }
            RecoveryCommand::CreateStash { name, .. } => {
                if let Some(form) = &self.recovery.stash
                    && form.read(cx).repository.as_ref() == Some(repository)
                {
                    if succeeded && form.read(cx).name.read(cx).value().as_ref() == name {
                        self.recovery.stash = None;
                    } else if !succeeded {
                        form.update(cx, |form, cx| {
                            form.error = self.tab_operation_error(repository);
                            cx.notify();
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// The Working Changes Actions menu's trigger: "Actions" with its caret, or,
/// `compact`, where its label does not fit beside Refresh, a 28 px control
/// that shows the more-actions dots in the shared helper's frame, as
/// Refresh beside it shows its arrows, rather than a bare caret that read
/// as a disclosure arrow. Either is named "Working changes and recovery
/// actions"; the compact one's tooltip leads with "Actions" and, `tight`,
/// names the selection controls its menu then leads with.
pub(super) fn working_actions_trigger(compact: bool, tight: bool) -> Button {
    const NAME: &str = "Working changes and recovery actions";
    const DETAIL: &str = "Ignore selected untracked content, stashes, and last-commit actions";
    const TIGHT_DETAIL: &str = "Directories, Stage and Unstage for the selection, ignore, stashes, and last-commit actions";
    let control = if compact {
        crate::views::compact_control_name(
            button("working-recovery-menu", "", "", false).child(
                div()
                    .debug_selector(|| "working-recovery-more".into())
                    .flex()
                    .child(control_icon("more")),
            ),
            "working-recovery-menu",
            NAME,
        )
        .tooltip(format!(
            "Actions · {}",
            if tight { TIGHT_DETAIL } else { DETAIL }
        ))
    } else {
        // The shared 28 px compact control, as Refresh beside it is, with its
        // caret: the kit's small Button is 1.5 rem, shorter than Refresh.
        button("working-recovery-menu", "Actions", "", false)
            .accessibility_label(NAME)
            .tooltip(DETAIL)
            .dropdown_caret(true)
    };
    control.debug_selector(|| "working-recovery-menu".into())
}

/// The selected commit's Actions menu trigger: "Actions" with its caret in
/// the shared 28 px compact control, as tall as the copy controls beside it
/// and as wide as its label and caret, with the palette's hover and pressed
/// fills (the kit's small Button is 1.5 rem); or, not `labelled`, a 28 px
/// control showing the more-actions dots in the helper's frame, as Working's
/// compact Actions does. Either is named "Selected commit actions"; the
/// compact one's tooltip leads with "Actions".
pub(super) fn commit_actions_trigger(labelled: bool) -> Button {
    const NAME: &str = "Selected commit actions";
    const DETAIL: &str = "Revert or cherry-pick this commit, or amend or undo the last commit";
    let control = if labelled {
        button("commit-recovery-menu", "Actions", "", false)
            .accessibility_label(NAME)
            .tooltip(DETAIL)
            .dropdown_caret(true)
    } else {
        crate::views::compact_control_name(
            button("commit-recovery-menu", "", "", false).child(
                div()
                    .debug_selector(|| "commit-recovery-more".into())
                    .flex()
                    .child(control_icon("more")),
            ),
            "commit-recovery-menu",
            NAME,
        )
        .tooltip(format!("Actions · {DETAIL}"))
    };
    control.debug_selector(|| "commit-recovery-menu".into())
}

fn working_menu_item(
    menu: PopupMenu,
    label: &'static str,
    disabled: bool,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> PopupMenu {
    menu.item(working_item(label, disabled, owner, repository, action))
}

/// A Working Changes menu item that acts only on the repository it was
/// built for, on the repository page, while no operation runs.
fn working_item(
    label: impl Into<SharedString>,
    disabled: bool,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> PopupMenuItem {
    let owner = owner.clone();
    let repository = repository.clone();
    PopupMenuItem::new(label)
        .disabled(disabled)
        .on_click(move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.path == repository
                    && this.page == AppPage::Repository
                    && this.operation_busy.is_none()
                {
                    action(this, window, cx);
                }
            });
        })
}

fn commit_menu(
    mut menu: PopupMenu,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    commit: &Arc<CommitTarget>,
    disabled: bool,
    current: bool,
) -> PopupMenu {
    for (label, kind) in [
        ("Revert this commit…", RecoveryKind::Revert),
        ("Cherry-pick this commit…", RecoveryKind::CherryPick),
    ] {
        let commit = Arc::clone(commit);
        menu = working_menu_item(
            menu,
            label,
            disabled,
            owner,
            repository,
            move |this, window, cx| {
                this.choose_commit_recovery(kind, Arc::clone(&commit), window, cx)
            },
        );
    }
    if current {
        menu = menu.separator();
        for (label, kind) in [
            ("Amend last commit…", RecoveryKind::Amend),
            ("Undo last local commit…", RecoveryKind::Undo),
        ] {
            let commit = Arc::clone(commit);
            menu = working_menu_item(
                menu,
                label,
                disabled,
                owner,
                repository,
                move |this, window, cx| {
                    this.choose_commit_recovery(kind, Arc::clone(&commit), window, cx)
                },
            );
        }
    }
    menu
}

fn append_paths(explanation: &mut String, paths: &[PathBuf]) {
    explanation.push_str(&format!(
        "\n\n{} affected {}",
        paths.len(),
        if paths.len() == 1 { "path" } else { "paths" }
    ));
    for path in paths.iter().take(24) {
        explanation.push_str(&format!("\n{}", path.display()));
    }
    if paths.len() > 24 {
        explanation.push_str(&format!("\n… and {} more", paths.len() - 24));
    }
}

fn split_message(message: &str) -> CommitDraft {
    let (title, description) = message.split_once('\n').unwrap_or((message, ""));
    CommitDraft {
        title: title.to_owned(),
        description: description
            .strip_prefix('\n')
            .unwrap_or(description)
            .to_owned(),
    }
}

fn amended_message(original: &str, initial: &CommitDraft, draft: &CommitDraft) -> String {
    if draft == initial {
        original.to_owned()
    } else {
        draft.message()
    }
}

fn list_key_selection(key: &str, selected: Option<usize>, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    match key {
        "up" => Some(selected.unwrap_or(0).saturating_sub(1).min(count - 1)),
        "down" => Some(selected.map_or(0, |index| index.saturating_add(1).min(count - 1))),
        "home" => Some(0),
        "end" => Some(count - 1),
        _ => None,
    }
}

struct AmendForm {
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    plan: Arc<CommitRecoveryPlan>,
    title: Entity<InputState>,
    description: Entity<TextareaState>,
    initial: CommitDraft,
    error: Option<String>,
}
impl AmendForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repository: Option<PathBuf>,
        plan: Arc<CommitRecoveryPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial = split_message(&plan.target.message);
        let title = cx.new(|cx| InputState::new(window, cx).default_value(initial.title.clone()));
        let description =
            cx.new(|cx| TextareaState::new(window, cx).default_value(initial.description.clone()));
        Self {
            owner,
            repository,
            plan,
            title,
            description,
            initial,
            error: None,
        }
    }
    fn draft(&self, cx: &App) -> CommitDraft {
        CommitDraft {
            title: self.title.read(cx).value().to_string(),
            description: self.description.read(cx).value().to_string(),
        }
    }
    fn message(&self, cx: &App) -> String {
        amended_message(&self.plan.target.message, &self.initial, &self.draft(cx))
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.title.read(cx).value().trim().is_empty() {
            self.error = Some("Enter a commit title.".into());
            cx.notify();
            return false;
        }
        let message = self.message(cx);
        let expected = Arc::clone(&self.plan);
        let form = cx.entity();
        let failure: Failure = Box::new(move |error, window, cx| {
            form.update(cx, |form, cx| {
                form.error = Some(error);
                cx.notify();
            });
            show_amend(form, window, cx);
        });
        self.owner.update(cx, |this, cx| {
            if this.path != self.repository { return false; }
            this.read_recovery(move |repo| {
                let plan = repo.recovery_plan(RecoveryKind::Amend, Some(&expected.target.oid), None)?;
                anyhow::ensure!(plan == *expected, "The branch or staged contents changed. Close this form and choose Amend again; your message is retained.");
                let mut explanation = format!("Replace {} on {} with a new commit using the staged contents and this message. {} currently staged paths will be included in the amendment. Unstaged working files remain available.\n\n{}", short_oid(&plan.target.oid), plan.current.branch, plan.staged_paths.len(), message);
                if !plan.known_published_refs.is_empty() { explanation.push_str(&format!("\n\nThis commit is reachable from locally available remote-tracking refs: {}. Amending rewrites its identity; coordinate with anyone using this history.", plan.known_published_refs.iter().take(12).map(|reference| reference.name.as_str()).collect::<Vec<_>>().join(", "))); }
                append_paths(&mut explanation, &plan.affected_paths);
                explanation.push_str("\n\nGit uses its configured hooks and signing. No remote is changed.");
                Ok(Prepared::Confirmation { title: "Amend last commit".into(), explanation, action: "Amend commit", command: RecoveryCommand::Amend { plan, message }.into() })
            }, Some(failure), window, cx)
        }).unwrap_or(false)
    }
}
impl Render for AmendForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .child(format!(
                        "{} · {} · {} staged paths",
                        self.plan.current.branch,
                        short_oid(&self.plan.target.oid),
                        self.plan.staged_paths.len()
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child("Title")
                    .child(Input::new(&self.title).aria_label("Amended commit title")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child("Description · optional")
                    .child(shortcuts::tab_leaves(
                        Textarea::new(&self.description)
                            .h(px(170.))
                            .aria_label("Amended commit description"),
                    )),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child("The next step reviews the staged snapshot and the effect on history."),
            )
            .children(self.error.as_ref().map(|error| form_error(error, cx)))
    }
}
fn show_amend(form: Entity<AmendForm>, window: &mut Window, cx: &mut App) {
    window.open_alert_dialog(cx, move |dialog, window, _| {
        let submit = form.clone();
        dialog
            .title("Amend last commit")
            .width(appearance::dialog_width(px(580.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(form.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Review amend")
                    .cancel_text("Cancel")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}

struct StashForm {
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    name: Entity<InputState>,
    include_untracked: bool,
    error: Option<String>,
}
impl StashForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repository: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx
            .new(|cx| InputState::new(window, cx).placeholder("Describe the work you are pausing"));
        Self {
            owner,
            repository,
            name,
            include_untracked: false,
            error: None,
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let name = self.name.read(cx).value().to_string();
        if name.trim().is_empty() {
            self.error = Some("Give this stash a name so you can find it later.".into());
            cx.notify();
            return false;
        }
        let include_untracked = self.include_untracked;
        let form = cx.entity();
        let failure: Failure = Box::new(move |error, window, cx| {
            form.update(cx, |form, cx| {
                form.error = Some(error);
                cx.notify();
            });
            show_stash_form(form, window, cx);
        });
        self.owner.update(cx, |this, cx| {
            if this.path != self.repository { return false; }
            this.read_recovery(move |repo| {
                let plan = repo.stash_create_plan(include_untracked)?;
                let mut explanation = format!("Save “{name}” from {}. Tracked staged and unstaged changes will be saved separately, then removed from this working copy. {}", plan.current.branch,
                    if include_untracked { "Untracked files are included and removed from the working copy. Ignored files remain." } else { "Untracked and ignored files remain in the working copy." });
                append_paths(&mut explanation, &plan.affected_paths);
                Ok(Prepared::Confirmation { title: "Save changes to stash".into(), explanation, action: "Save stash", command: RecoveryCommand::CreateStash { plan, name }.into() })
            }, Some(failure), window, cx)
        }).unwrap_or(false)
    }
}
impl Render for StashForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div().flex().flex_col().gap_3()
            .child(div().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child("Pause the current work and keep a named snapshot you can inspect before restoring."))
            .child(div().flex().flex_col().gap_1().child("Stash name").child(Input::new(&self.name).aria_label("Stash name")))
            .child(Checkbox::new("stash-include-untracked").label("Include untracked files").checked(self.include_untracked)
                .on_click(cx.listener(|this, checked, _, cx| { this.include_untracked = *checked; cx.notify(); })))
            .children(self.error.as_ref().map(|error| form_error(error, cx)))
    }
}
fn show_stash_form(form: Entity<StashForm>, window: &mut Window, cx: &mut App) {
    window.open_alert_dialog(cx, move |dialog, window, _| {
        let submit = form.clone();
        dialog
            .title("Save changes to stash")
            .width(appearance::dialog_width(px(520.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(form.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Review stash")
                    .cancel_text("Cancel")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}
fn form_error(error: &str, cx: &App) -> AnyElement {
    div()
        .id("recovery-form-error")
        .max_h(px(130.))
        .overflow_y_scroll()
        .text_size(crate::appearance::ui_text(12.))
        .text_color(rgb(palette(cx).warning))
        .child(error.to_owned())
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StashArea {
    Staged,
    Unstaged,
    Untracked,
}
impl StashArea {
    fn label(self) -> &'static str {
        match self {
            Self::Staged => "Staged",
            Self::Unstaged => "Unstaged",
            Self::Untracked => "Untracked",
        }
    }
    fn files(self, snapshot: &StashSnapshot) -> &[FileChange] {
        match self {
            Self::Staged => &snapshot.staged,
            Self::Unstaged => &snapshot.unstaged,
            Self::Untracked => &snapshot.untracked,
        }
    }
}

/// This transient inspector owns one bounded metadata executor and one
/// replaceable preview worker. Closing it releases both and its preview cache;
/// its replies never replace the main workspace's current comparison.
pub(crate) struct StashBrowser {
    owner: WeakEntity<GitTurtle>,
    repository: GitRepository,
    metadata: SerialExecutor,
    preview_worker: Worker,
    generation: u64,
    preview_generation: u64,
    /// This showing of the file preview, which a line beneath an open
    /// control belongs to; [`Self::clear_preview`] starts a new one.
    showing: crate::system_preview::Showing,
    metadata_task: Option<Task<()>>,
    preview_task: Option<Task<()>>,
    page: Option<StashPage>,
    offset: usize,
    selected: Option<usize>,
    snapshot: Option<Arc<StashSnapshot>>,
    area: StashArea,
    selected_file: Option<usize>,
    content: Option<Arc<Content>>,
    text_mode: usize,
    editors: [Option<Entity<EditorState>>; 3],
    /// The patch editor's decorations, kept for its life like Compare's.
    decoration: Option<crate::editor_find::PatchDecorations>,
    patch_view: Option<Entity<diff_view::DiffView>>,
    stash_scroll: UniformListScrollHandle,
    /// The detail's scroll, back at its top for each stash it inspects,
    /// which reveals the control the keyboard moves focus onto.
    detail: crate::focus_reveal::FocusReveal,
    file_scroll: UniformListScrollHandle,
    stash_focus: FocusHandle,
    file_focus: FocusHandle,
    loading: Option<&'static str>,
    error: Option<String>,
    preview_error: Option<String>,
    restore_index: bool,
}

impl Drop for StashBrowser {
    fn drop(&mut self) {
        crate::model_view::pause(self.content.as_deref());
    }
}

impl StashBrowser {
    /// Returns once every read the browser has started, the saved stashes
    /// or a selected stash's inspection, has landed and been drawn. They run
    /// on the browser's own thread, which `run_until_parked` cannot wait for:
    /// once it drains, each has answered, which holds the test clock, and the
    /// replies land as the test parks.
    #[cfg(test)]
    pub(crate) fn lands(browser: &Entity<Self>, cx: &mut VisualTestContext) {
        browser.read_with(cx, |browser, _| browser.metadata.drain());
        crate::views::tests::draw(cx);
        assert!(
            browser.read_with(cx, |browser, _| browser.metadata_task.is_none()
                && browser.loading.is_none()),
            "the Stashes read lands"
        );
    }

    fn new(
        owner: WeakEntity<GitTurtle>,
        repository: GitRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            owner,
            repository,
            metadata: SerialExecutor::new("stash-metadata"),
            preview_worker: Worker::new(),
            generation: 0,
            preview_generation: 0,
            showing: crate::system_preview::Showing::next(),
            metadata_task: None,
            preview_task: None,
            page: None,
            offset: 0,
            selected: None,
            snapshot: None,
            area: StashArea::Staged,
            selected_file: None,
            content: None,
            text_mode: 0,
            editors: [None, None, None],
            decoration: None,
            patch_view: None,
            stash_scroll: UniformListScrollHandle::new(),
            detail: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
            file_scroll: UniformListScrollHandle::new(),
            stash_focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            loading: None,
            error: None,
            preview_error: None,
            restore_index: false,
        };
        this.load_page(0, window, cx);
        this
    }

    fn is_current(&self, cx: &App) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            let owner = owner.read(cx);
            owner.path.as_deref() == Some(self.repository.path())
                && owner.page == AppPage::Repository
        })
    }

    /// Opens captured bytes for this showing's open controls through the
    /// window's `GitTurtle`.
    fn opener(&self) -> crate::system_preview::Opener {
        crate::system_preview::Opener::new(self.owner.clone(), self.showing)
    }

    fn clear_preview(&mut self) {
        crate::model_view::pause(self.content.as_deref());
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.showing = crate::system_preview::Showing::next();
        self.preview_worker.cancel();
        self.preview_task = None;
        self.content = None;
        self.editors = [None, None, None];
        self.decoration = None;
        self.patch_view = None;
        self.preview_error = None;
    }

    /// CPU bytes the open preview retains: its prepared content, which includes the patch
    /// presentation, with the same reserve for native editors as a repository tab, plus the
    /// patch editor's decorations. This transient inspector has no retained-bytes bound of its
    /// own, and a modal stays out of the repository tabs' bound because tabs cannot switch
    /// while it is open; its preview worker's cache bounds the content separately.
    #[cfg_attr(not(test), allow(dead_code))]
    fn preview_retained_bytes(&self) -> usize {
        self.content
            .as_ref()
            .map_or(0, |content| content.bytes().saturating_mul(4))
            + self
                .decoration
                .as_ref()
                .map_or(0, crate::editor_find::PatchDecorations::retained_bytes)
    }

    fn load_page(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.clear_preview();
        self.snapshot = None;
        self.detail.scroll().set_offset(Point::default());
        self.selected = None;
        self.selected_file = None;
        self.loading = Some("Reading stashes…");
        self.error = None;
        let repo = self.repository.clone();
        let response = self
            .metadata
            .submit_read(move || repo.stash_list(offset, STASH_PAGE_SIZE));
        self.metadata_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Stash listing was interrupted. Refresh to try again."
                ))
            });
            let _ = this.update_in(cx, |this, _, cx| {
                if this.generation != generation {
                    return;
                }
                this.metadata_task = None;
                this.loading = None;
                if !this.is_current(cx) {
                    this.error = Some(
                        "The repository changed. Close this inspector and open Stashes again."
                            .into(),
                    );
                    cx.notify();
                    return;
                }
                match result {
                    Ok(page) => {
                        this.page = Some(page);
                        this.offset = offset;
                        this.stash_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_stash(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stash) = self
            .page
            .as_ref()
            .and_then(|page| page.entries.get(index))
            .cloned()
        else {
            return;
        };
        window.focus(&self.stash_focus, cx);
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.clear_preview();
        self.snapshot = None;
        self.detail.scroll().set_offset(Point::default());
        self.selected = Some(index);
        self.selected_file = None;
        self.loading = Some("Reading saved changes…");
        self.error = None;
        let repo = self.repository.clone();
        let response = self
            .metadata
            .submit_read(move || repo.stash_snapshot(&stash));
        self.metadata_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Stash inspection was interrupted. Select the stash again."
                ))
            });
            let _ = this.update_in(cx, |this, _, cx| {
                if this.generation != generation {
                    return;
                }
                this.metadata_task = None;
                this.loading = None;
                if !this.is_current(cx) {
                    this.error = Some("The repository changed. Open Stashes again.".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        this.area = [StashArea::Staged, StashArea::Unstaged, StashArea::Untracked]
                            .into_iter()
                            .find(|area| !area.files(&snapshot).is_empty())
                            .unwrap_or(StashArea::Staged);
                        this.snapshot = Some(Arc::new(snapshot));
                        this.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn choose_area(&mut self, area: StashArea, cx: &mut Context<Self>) {
        if self.area == area {
            return;
        }
        self.area = area;
        self.selected_file = None;
        self.clear_preview();
        self.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn files(&self) -> &[FileChange] {
        self.snapshot
            .as_ref()
            .map_or(&[], |snapshot| self.area.files(snapshot))
    }

    fn select_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.files().get(index).cloned() else {
            return;
        };
        window.focus(&self.file_focus, cx);
        self.clear_preview();
        self.selected_file = Some(index);
        let generation = self.preview_generation;
        let response = self.preview_worker.submit(Job::Preview {
            origins: Default::default(),
            repo: self.repository.clone(),
            file,
        });
        self.preview_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.preview_generation != generation {
                    return;
                }
                this.preview_task = None;
                if !this.is_current(cx) {
                    this.preview_error = Some("The repository changed. Open Stashes again.".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Output::Preview(content, _))) => {
                        this.content = Some(content);
                        this.ensure_editor(window, cx);
                    }
                    Ok(Err(error)) => this.preview_error = Some(format!("{error:#}")),
                    _ => {
                        this.preview_error =
                            Some("Preview was interrupted. Select the file again.".into())
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn ensure_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_mode == 3 {
            return;
        }
        if self.editors[self.text_mode].is_some() {
            return;
        }
        let Some(Content::Text {
            patch,
            old,
            new,
            presentation,
            ..
        }) = self.content.as_deref()
        else {
            return;
        };
        let value = [patch, old, new][self.text_mode];
        let (editor, decoration) = text::editor_with_decorations(
            value,
            if self.text_mode == 0 { "diff" } else { "text" },
            (self.text_mode == 0).then_some(presentation.as_ref()),
            window,
            cx,
        );
        if self.text_mode == 0 {
            self.patch_view = Some(diff_view::new(editor.clone(), presentation, window, cx));
            self.decoration = decoration;
        }
        self.editors[self.text_mode] = Some(editor);
    }

    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let stash = snapshot.stash.clone();
        let restore_index = self.restore_index;
        let repository = self.repository.path().to_owned();
        let _ = self.owner.update(cx, |this, cx| {
            if this.path.as_ref() != Some(&repository) || this.operation_busy.is_some() { return; }
            window.close_dialog(cx);
            this.read_recovery(move |repo| {
                let plan = repo.stash_apply_plan(&stash, restore_index)?;
                let mut explanation = format!("Restore “{}” ({}) onto {} at {}.\n\n{}\n\nThe stash remains saved after restoration. Conflicts leave the applied work in place for review in Changes.", stash.name, short_oid(&stash.oid), plan.current.branch, short_oid(&plan.current.head),
                    if restore_index { "Restore the original staged state as well as working files. Git may refuse if that staged state cannot be recreated safely." } else { "Restore tracked changes into the working files without recreating the stash's staged state. Saved untracked files are restored when present." });
                append_paths(&mut explanation, &plan.affected_paths);
                Ok(Prepared::Confirmation { title: "Restore stash".into(), explanation, action: "Restore stash", command: RecoveryCommand::ApplyStash { plan }.into() })
            }, None, window, cx);
        });
    }

    fn drop_stash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let stash = snapshot.stash.clone();
        let repository = self.repository.path().to_owned();
        let _ = self.owner.update(cx, |this, cx| {
            if this.path.as_ref() != Some(&repository) || this.operation_busy.is_some() { return; }
            window.close_dialog(cx);
            this.confirm_git_write("Drop saved stash".into(), format!("Remove “{}” ({}) from the saved stash list. Its saved changes will no longer be available here. Current index and working files stay as they are.\n\nInspect or restore any work you need before dropping this stash.", stash.name, short_oid(&stash.oid)), "Drop stash", WriteCommand::Recovery(RecoveryCommand::DropStash { stash }.into()), window, cx);
        });
    }

    fn render_stash_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(stash) = self.page.as_ref().and_then(|page| page.entries.get(index)) else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let selected = self.selected == Some(index);
        div()
            .id(("saved-stash", stash.ordinal))
            .debug_selector(move || format!("saved-stash-row-{index}"))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .aria_label(format!("{} · {}", stash.name, stash.selector))
            .h(crate::appearance::ui_size(58.))
            .w_full()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .overflow_hidden()
            .bg(rgb(if selected { p.selected } else { p.panel }))
            .hover(|style| style.bg(rgb(p.row_hover(selected))))
            .cursor_pointer()
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(stash.name.clone()),
            )
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(format!(
                        "{} · {} · {}",
                        stash.selector,
                        short_oid(&stash.oid),
                        short_date(stash.timestamp)
                    )),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select_stash(index, window, cx)))
            .into_any_element()
    }

    fn render_file_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(file) = self.files().get(index) else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let selected = self.selected_file == Some(index);
        div()
            .id(("stash-file", index))
            .debug_selector(move || format!("stash-file-{index}"))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .aria_label(format!("{} · {}", self.area.label(), file.path().display()))
            .h(crate::appearance::ui_size(28.))
            .w_full()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .bg(rgb(if selected { p.selected } else { p.canvas }))
            .hover(|style| style.bg(rgb(p.row_hover(selected))))
            .cursor_pointer()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(crate::appearance::ui_text(11.))
                    .child(file.path().display().to_string()),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(format!("{:?}", file.status)),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select_file(index, window, cx)))
            .into_any_element()
    }

    /// The selected file, `width` wide as its box was last laid out.
    fn render_content(&self, width: Pixels, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        if let Some(error) = &self.preview_error {
            return div()
                .p_3()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.warning))
                .child(error.clone())
                .into_any_element();
        }
        if self.preview_task.is_some() {
            return native_accessibility::text("stash-file-reading", "Reading saved file…")
                .p_4()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.muted))
                .into_any_element();
        }
        match self.content.as_deref() {
            Some(Content::Text { diagrams, .. }) if self.text_mode == 3 => {
                diagrams.as_ref().map_or_else(
                    || {
                        div()
                            .child("No Mermaid diagrams on this file; use Source.")
                            .into_any_element()
                    },
                    |preview| {
                        crate::rich_preview::render_comparison(
                            preview,
                            false,
                            self.opener(),
                            width,
                            window,
                            cx,
                        )
                    },
                )
            }
            Some(Content::Rich(preview)) => crate::rich_preview::render_comparison(
                preview,
                false,
                self.opener(),
                width,
                window,
                cx,
            ),
            Some(Content::Text { .. }) if self.text_mode == 0 => {
                self.patch_view.as_ref().map_or_else(
                    || div().into_any_element(),
                    |view| view.clone().into_any_element(),
                )
            }
            Some(Content::Text { .. }) => self.editors[self.text_mode].as_ref().map_or_else(
                || div().into_any_element(),
                |editor| {
                    crate::editor_find::Editor::new(editor)
                        .readonly(true)
                        .size_full()
                        .aria_label("Saved stash file text")
                        .into_any_element()
                },
            ),
            Some(Content::Images { old, new }) => div()
                .size_full()
                .flex()
                .gap_2()
                .children(
                    [("Before", old), ("After", new)]
                        .into_iter()
                        .map(|(label, side)| {
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .p_2()
                                .child(
                                    div()
                                        .text_size(crate::appearance::ui_text(11.))
                                        .text_color(rgb(p.muted))
                                        .child(if side.animation.is_some() {
                                            format!("{label} · first frame")
                                        } else {
                                            label.into()
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when_some(side.render.clone(), |element, image| {
                                            element.child(crate::gif_playback::static_image(image))
                                        })
                                        .when(side.render.is_none(), |element| {
                                            element.child(
                                                div()
                                                    .text_size(crate::appearance::ui_text(11.))
                                                    .text_color(rgb(p.muted))
                                                    .child(
                                                        side.message
                                                            .clone()
                                                            .unwrap_or("Absent".into()),
                                                    ),
                                            )
                                        }),
                                )
                        }),
                )
                .into_any_element(),
            Some(Content::Notice(message)) => div()
                .p_4()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.muted))
                .child(message.clone())
                .into_any_element(),
            _ => native_accessibility::text(
                "stash-file-prompt",
                "Select a saved file to inspect its exact comparison.",
            )
            .p_4()
            .text_size(crate::appearance::ui_text(12.))
            .text_color(rgb(p.muted))
            .into_any_element(),
        }
    }
}

impl Render for StashBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let (frame, width) = crate::rich_preview::measured_box("stash-file-width", window, cx);
        let narrow = window.viewport_size().width < px(800.);
        let height = (f32::from(window.viewport_size().height) - 230.).clamp(300., 620.);
        let busy = self.metadata_task.is_some();
        let list = uniform_list(
            "saved-stashes",
            self.page.as_ref().map_or(0, |page| page.entries.len()),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.render_stash_row(index, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.stash_scroll);
        let files = uniform_list(
            "stash-files",
            self.files().len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.render_file_row(index, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.file_scroll);
        let snapshot = self.snapshot.clone();
        // Stacked in a narrow window, the saved-work panel is as tall as its
        // header, one 58 px row and its footer at the default size, scaled
        // with them: unscaled, its header and footer left the list 35.6 px
        // at 18 pt in a 461 × 490 window, short of a row and of the empty
        // line, which ran past the list's visible bottom. That line takes
        // the rows' own inset, where its 1 rem padding started it past
        // their text and outgrew the list. With no stash saved there is
        // nothing to inspect: no detail is drawn and the list takes the
        // browser's room. The browser asks for `height` and gives up what
        // the dialog's height cap takes.
        let detail = self.error.is_some()
            || !self
                .page
                .as_ref()
                .is_some_and(|page| page.entries.is_empty());
        let room = appearance::button_ring_room(cx);
        // A control the keyboard moves focus onto scrolls into the detail's
        // view in this frame with its ring; each reports where it lies.
        self.detail.reveal(window, cx);
        let cue = crate::tags::ScrollCue::new(self.detail.scroll(), room);
        let areas: Vec<_> = (0..3_usize)
            .map(|area| self.detail.focus(("stash-area", area), cx))
            .collect();
        let modes: Vec<_> = ["Diff", "Before", "After", "Diagrams"]
            .into_iter()
            .enumerate()
            .filter(|(mode, _)| {
                *mode != 3
                    || matches!(
                        self.content.as_deref(),
                        Some(Content::Text {
                            diagrams: Some(_),
                            ..
                        })
                    )
            })
            .map(|(mode, label)| {
                (
                    mode,
                    label,
                    self.detail.focus(("stash-text-mode", mode), cx),
                )
            })
            .collect();
        let mode_focus: Vec<_> = modes
            .iter()
            .map(|(_, _, focus)| Some(focus.clone()))
            .collect();
        // The comparison's text, where the keyboard reaches it.
        let text_focus: Vec<_> = self
            .editors
            .iter()
            .flatten()
            .map(|editor| editor.read(cx).focus_handle(cx))
            .chain(
                self.patch_view
                    .as_ref()
                    .map(|view| view.read(cx).focus_handle(cx)),
            )
            .collect();
        let restore_index = self.detail.focus("stash-restore-index", cx);
        let drop = self.detail.focus("drop-saved-stash", cx);
        let restore = self.detail.focus("restore-saved-stash", cx);
        // Beside the list, the restore actions and their note are the
        // detail's footer, whole on selection whatever the comparison takes:
        // at 18 pt in a 1000 × 680 window they lay below the fold of the
        // scrolling detail until the user scrolled. Stacked below the list in
        // a narrow window, where the detail is a few lines tall (about 67 px
        // at 18 pt in 461 × 490), they scroll with the rest, revealed with
        // their rings as the keyboard reaches them.
        let pinned = !narrow;
        let actions = self.snapshot.is_some().then(|| {
            let row = div().debug_selector(|| "stash-actions".into()).flex().flex_wrap().items_center().gap_2().pt_1()
                .when(!pinned, |row| row.on_children_prepainted(self.detail.controls(vec![Some(restore_index.clone()), None, Some(drop.clone()), Some(restore.clone())], room)))
                .child(Checkbox::new("stash-restore-index").track_focus(&restore_index).label("Restore staged state").checked(self.restore_index).on_click(cx.listener(|this, checked, _, cx| { this.restore_index = *checked; cx.notify(); })))
                .child(div().flex_1())
                .child(button("drop-saved-stash", "Drop…", "", false).track_focus(&drop).disabled(busy).on_click(cx.listener(|this, _, window, cx| this.drop_stash(window, cx))))
                .child(Button::new("restore-saved-stash").primary().small().label("Review restore…").track_focus(&restore).disabled(busy).on_click(cx.listener(|this, _, window, cx| this.restore(window, cx))));
            div().flex().flex_col().gap_2().flex_shrink_0()
                .child(row)
                .child(native_accessibility::text("stash-restore-note", "Restoring keeps this stash saved. Drop it separately after checking the restored work.").text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)))
        });
        let (footer, inline) = if pinned {
            (actions, None)
        } else {
            (None, actions)
        };
        // Until a file is chosen the comparison holds only its prompt and
        // keeps that height, which takes no room from the actions; a chosen
        // file's comparison keeps a floor of its own.
        let file_chosen = self.selected_file.is_some();
        div().debug_selector(|| "stash-browser".into()).h(px(height)).min_h_0().flex().gap_3().when(narrow, |element| element.flex_col())
            .child(div().debug_selector(|| "stash-list-panel".into()).w(px(255.)).min_w_0().flex_shrink_0().h_full().flex().flex_col().border_1().border_color(rgb(p.border)).rounded(px(7.)).overflow_hidden()
                .when(narrow && detail, |element| element.w_full().h(crate::appearance::ui_size(140.)))
                .when(!detail, |element| element.w_full().flex_1())
                .child(div().h(crate::appearance::ui_size(38.)).flex().items_center().px_2().gap_1().border_b_1().border_color(rgb(p.border))
                    .child(native_accessibility::text("stash-list-heading", "Saved work").flex_1().text_size(crate::appearance::ui_text(11.)).font_weight(FontWeight::SEMIBOLD))
                    .child(button("refresh-stashes", "", "refresh", false).accessibility_label("Refresh saved stashes").disabled(busy).on_click(cx.listener(|this, _, window, cx| this.load_page(0, window, cx)))))
                .child(crate::native_accessibility::clips_children(div().id("stash-list-focus").debug_selector(|| "stash-list".into()).role(Role::ListBox).aria_label("Saved stashes").tab_stop(true).track_focus(&self.stash_focus).flex_1().min_h_0()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if let Some(index) = list_key_selection(event.keystroke.key.as_str(), this.selected, this.page.as_ref().map_or(0, |page| page.entries.len())) {
                            cx.stop_propagation();
                            this.select_stash(index, window, cx);
                            this.stash_scroll.scroll_to_item(index, ScrollStrategy::Center);
                        }
                    }))
                    .when(self.page.as_ref().is_some_and(|page| !page.entries.is_empty()), |element| element.child(list))
                    .when(self.page.as_ref().is_some_and(|page| page.entries.is_empty()), |element| element.child(div().px_3().py_2().child(EmptyState::new("stash-list-empty", "No saved stashes · Save one from the Actions menu in Changes").block(cx))))))
                .child(div().h(crate::appearance::ui_size(36.)).flex().items_center().justify_between().px_2().border_t_1().border_color(rgb(p.border))
                    .child(button("previous-stashes", "Previous", "", false).disabled(busy || self.offset == 0).on_click(cx.listener(|this, _, window, cx| this.load_page(this.offset.saturating_sub(STASH_PAGE_SIZE), window, cx))))
                    .child(button("next-stashes", "More", "", false).disabled(busy || self.page.as_ref().and_then(|page| page.next_offset).is_none()).on_click(cx.listener(|this, _, window, cx| { if let Some(offset) = this.page.as_ref().and_then(|page| page.next_offset) { this.load_page(offset, window, cx); } })))))
            // The detail scrolls where the window leaves it less than its
            // inspection, as below the stacked list in a short window, and
            // cues the rest with its scrollbar, as the dialogs' lists do. It
            // keeps the room of its controls' rings inside its clip and
            // gives it back, so nothing moves, except below its last line:
            // there the dialog's content clips at the browser's foot, which
            // cut the ring of a control revealed at the detail's bottom. A
            // control the keyboard moves onto scrolls into view with its
            // ring, as the Reflog's do. A chosen file's comparison keeps a
            // floor of its own rather than give its height up to the
            // controls around it, as at 18 pt in a 1000 × 680 window, where
            // it kept 30 px without one. Beside the list the actions are the
            // footer below it (`pinned`).
            .when(detail, |element| element.child(div().debug_selector(|| "stash-detail-column".into()).flex_1().min_w_0().min_h_0().flex().flex_col().gap_2().child(cue.list(div().on_children_prepainted(cue.observe(self.detail.observe())).id("stash-detail").debug_selector(|| "stash-detail".into()).flex_1().min_w_0().min_h_0().p(room).mx(-room).mt(-room).overflow_y_scroll().track_scroll(self.detail.scroll()).flex().flex_col().gap_2()
                .children(self.loading.map(|loading| native_accessibility::text("stash-detail-loading", loading).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted))))
                .children(self.error.as_ref().map(|error| form_error(error, cx)))
                .when_some(snapshot, |element, snapshot| element
                    .child(div().flex().flex_col().gap_1()
                        .child(native_accessibility::text("stash-detail-name", snapshot.stash.name.clone()).truncate().text_size(crate::appearance::ui_text(14.)).font_weight(FontWeight::SEMIBOLD))
                        .child(native_accessibility::text("stash-detail-identity", format!("{} · base {} · saved index {}", short_oid(&snapshot.stash.oid), short_oid(&snapshot.base_oid), short_oid(&snapshot.index_oid))).text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted))))
                    .child(div().flex().flex_wrap().items_center().gap_1().on_children_prepainted(self.detail.controls(areas.iter().cloned().map(Some).collect(), room)).children([StashArea::Staged, StashArea::Unstaged, StashArea::Untracked].into_iter().zip(areas).map(|(area, focus)| {
                        button(("stash-area", area as usize), format!("{} ({})", area.label(), area.files(&snapshot).len()), "", self.area == area).track_focus(&focus).debug_selector(move || format!("stash-area-{}", area as usize)).on_click(cx.listener(move |this, _, _, cx| this.choose_area(area, cx)))
                    })))
                    .child(div().flex().flex_col().flex_shrink_0().on_children_prepainted(self.detail.control(self.file_focus.clone(), room)).child(crate::native_accessibility::clips_children(div().id("stash-files-focus").role(Role::ListBox).aria_label("Saved files").tab_stop(true).track_focus(&self.file_focus).h(px(if narrow { 85. } else { 130. })).flex_shrink_0().border_1().border_color(rgb(p.border)).rounded(px(5.)).overflow_hidden()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if let Some(index) = list_key_selection(event.keystroke.key.as_str(), this.selected_file, this.files().len()) {
                                cx.stop_propagation();
                                this.select_file(index, window, cx);
                                this.file_scroll.scroll_to_item(index, ScrollStrategy::Center);
                            }
                        }))
                        .when(!self.files().is_empty(), |element| element.child(files))
                        .when(self.files().is_empty(), |element| element.child(native_accessibility::text("stash-files-empty", "No saved files in this group.").p_3().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))))))
                    .child(div().flex().items_center().gap_1().on_children_prepainted(self.detail.controls(mode_focus, room)).children(modes.into_iter().map(|(mode, label, focus)| {
                        button(("stash-text-mode", mode), label, "", self.text_mode == mode).track_focus(&focus).disabled(!matches!(self.content.as_deref(), Some(Content::Text { .. }))).on_click(cx.listener(move |this, _, window, cx| { this.text_mode = mode; this.ensure_editor(window, cx); cx.notify(); }))
                    })))
                    .child(div().debug_selector(|| "stash-comparison".into()).when(file_chosen, |element| element.flex_1().min_h(crate::appearance::ui_size(STASH_CONTENT_FLOOR))).when(!file_chosen, |element| element.flex_grow(1.).flex_shrink_0()).border_1().border_color(rgb(p.border)).rounded(px(5.)).overflow_hidden().on_children_prepainted(self.detail.group(text_focus, room)).child(frame.child(self.render_content(width, window, cx))))
                    .children(inline))
                // Where its loading line stood, at the detail's edge, and
                // short enough for one line beside the stacked list at 18 pt
                // in a 461 × 490 window: the centred prompt that named every
                // group wrapped there, past the detail's room, and left
                // "files." alone on its last line at 13 pt.
                .when(self.snapshot.is_none() && self.loading.is_none() && self.error.is_none(), |element| element.child(native_accessibility::text("stash-detail-placeholder", "Choose a stash to inspect its files.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))), "stash-detail-scrollbar").flex_1().min_w_0().min_h_0()).children(footer)))
    }
}

/// Prepares Amend last commit for `head`, whose form opens once the read
/// answers, for the prose fields' Tab regression in `views::tests`.
#[cfg(test)]
pub(crate) fn prepare_amend(
    app: &mut GitTurtle,
    head: String,
    window: &mut Window,
    cx: &mut Context<GitTurtle>,
) {
    app.prepare_commit_recovery(RecoveryKind::Amend, head, None, window, cx);
}

/// The open Amend form's description field.
#[cfg(test)]
pub(crate) fn amend_description(app: &GitTurtle, cx: &App) -> Option<Entity<TextareaState>> {
    let form = app.recovery.amend.as_ref()?;
    Some(form.read(cx).description.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn amend_fields_preserve_description_whitespace_and_comment_lines() {
        let message = "Title\n\n  indented\n# literal description\n\n";
        let draft = split_message(message);
        assert_eq!(draft.title, "Title");
        assert_eq!(draft.description, "  indented\n# literal description\n\n");
        assert_eq!(draft.message(), message);
    }
    #[test]
    fn legacy_single_newline_messages_split_without_losing_body_text() {
        assert_eq!(
            split_message("Title\nbody\n"),
            CommitDraft {
                title: "Title".into(),
                description: "body\n".into()
            }
        );
        assert_eq!(
            split_message("Only title"),
            CommitDraft {
                title: "Only title".into(),
                description: String::new()
            }
        );
    }

    #[test]
    fn unchanged_amend_preserves_exact_legacy_message_and_edits_use_blank_separator() {
        let original = "Title\n  body\n# literal\n";
        let initial = split_message(original);
        assert_eq!(amended_message(original, &initial, &initial), original);
        let edited = CommitDraft {
            title: "Revised title".into(),
            description: initial.description.clone(),
        };
        assert_eq!(
            amended_message(original, &initial, &edited),
            "Revised title\n\n  body\n# literal\n"
        );
    }

    /// The stash inspector's unified patch keeps its decorations for the editor's life, and
    /// its preview figure counts them along with the presentation they were built from.
    #[gpui::test]
    async fn stash_patch_preview_counts_its_decorations_and_presentation(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repository = GitRepository::init(fixture.path().join("repository"), "main").unwrap();
        cx.update(gpui_kit::init);
        let captured: std::rc::Rc<std::cell::RefCell<Option<Entity<StashBrowser>>>> =
            Default::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let browser =
                cx.new(|cx| StashBrowser::new(WeakEntity::new_invalid(), repository, window, cx));
            *output.borrow_mut() = Some(browser.clone());
            gpui_kit::component::Root::new(browser, window, cx)
        });
        cx.run_until_parked();
        let browser = captured.borrow().as_ref().unwrap().clone();
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.content = Some(Arc::new(text::decorated_patch_content()));
                browser.text_mode = 0;
                browser.ensure_editor(window, cx);
                let Some(Content::Text { presentation, .. }) = browser.content.as_deref() else {
                    unreachable!()
                };
                let presentation = presentation.retained_bytes();
                let decoration = browser.decoration.clone().expect("kept with the editor");
                let decorated = decoration.retained_bytes();
                assert!(decorated > 0, "a patch with changes has decorations");

                let with = browser.preview_retained_bytes();
                browser.decoration = None;
                let without = browser.preview_retained_bytes();
                assert_eq!(with - without, decorated);
                browser.content = None;
                assert!(without - browser.preview_retained_bytes() >= presentation);

                browser.decoration = Some(decoration);
                browser.clear_preview();
                assert!(browser.decoration.is_none());
                assert_eq!(browser.preview_retained_bytes(), 0);
            })
        });
    }

    #[test]
    fn stash_list_navigation_bounds_empty_and_changed_pages() {
        assert_eq!(list_key_selection("down", None, 0), None);
        assert_eq!(list_key_selection("down", None, 4), Some(0));
        assert_eq!(list_key_selection("up", Some(0), 4), Some(0));
        assert_eq!(list_key_selection("down", Some(3), 4), Some(3));
        assert_eq!(list_key_selection("up", Some(39), 4), Some(3));
        assert_eq!(list_key_selection("end", None, 4), Some(3));
        assert_eq!(list_key_selection("home", Some(3), 4), Some(0));
        assert_eq!(list_key_selection("escape", Some(3), 4), None);
    }

    /// In a 461 × 490 half tile, at 13 and 18 pt, with a stash selected,
    /// every Tab stop of the Stashes dialog draws its ring whole and in view:
    /// the detail scrolls the control the keyboard moves onto into view with
    /// its ring, as the dialogs' lists do. Its group, mode and restore
    /// controls once stayed below the detail's fold, drawing no ring.
    #[gpui::test]
    async fn a_narrow_stash_detail_shows_every_stop_whole(cx: &mut gpui_kit::TestAppContext) {
        use crate::focus_ring::sweep;
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "kept for later",
            ],
        );
        let (app, cx) = window(cx, &repo);
        sweep::prepare(&app, cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        let mut failures = Vec::new();
        for interface in sweep::SIZES {
            sweep::interface_size(cx, interface);
            sweep::open_dialog(&app, cx, "Stashes", |app, window, cx| {
                app.open_stashes(window, cx)
            })
            .await;
            // The saved stashes, then the selected stash's inspection, are
            // read on the browser's own threads.
            let landed = |cx: &mut VisualTestContext, selector: &'static str| {
                for _ in 0..400 {
                    cx.executor()
                        .advance_clock(std::time::Duration::from_millis(50));
                    draw(cx);
                    if cx.debug_bounds(selector).is_some() {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                panic!("{selector} never drawn");
            };
            landed(cx, "saved-stash-row-0");
            // A click focuses the list; Home selects the stash from the
            // keyboard, as a person walking it with keys would.
            let row = cx.debug_bounds("saved-stash-row-0").unwrap();
            cx.simulate_click(row.center(), Modifiers::none());
            cx.simulate_keystrokes("home");
            landed(cx, "stash-area-0");
            sweep::walk(&app, cx, "Stashes", interface, &mut failures);
            sweep::close_dialogs(cx, "Stashes", interface, &mut failures);
        }
        sweep::assert_whole(failures);
    }

    /// At 18 pt in a 1000 × 680 window the Stashes detail shows Restore
    /// staged state, Drop… and Review restore… whole as a stash is selected,
    /// without scrolling, and keeps them there, its footer, as a chosen
    /// file's comparison takes its floor and the detail scrolls: they lay
    /// below the scrolling detail's fold, as the empty comparison kept the
    /// floor a file's takes. Until a file is chosen the comparison keeps its
    /// prompt's height. Stacked in a 461 × 490 window, where the detail is a
    /// few lines tall, the actions scroll with the rest.
    #[gpui::test]
    async fn the_stash_actions_stay_in_view_beside_the_list(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, scroll_down, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "kept for later",
            ],
        );
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        let landed = |cx: &mut VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("no {selector}"))
        };
        let slack = px(0.5);
        for (width, height, pinned) in [(1000., 680., true), (461., 490., false)] {
            let at = format!("18 pt in {width} × {height}");
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
            landed(cx, "saved-stash-row-0");
            let row = bounds(cx, "saved-stash-row-0");
            cx.simulate_click(row.center(), Modifiers::none());
            landed(cx, "stash-actions");
            // Until a file is chosen, the comparison is its prompt's height.
            let comparison = bounds(cx, "stash-comparison");
            let floor = appearance::ui_size(STASH_CONTENT_FLOOR);
            // Layout snaps the floor to whole pixels.
            assert!(
                comparison.size.height < floor - px(1.),
                "{at}: the empty comparison {comparison:?} keeps a file's floor"
            );
            let browser = bounds(cx, "stash-browser");
            let detail = bounds(cx, "stash-detail");
            let viewport = cx.update(|window, _| window.viewport_size());
            let whole = |actions: Bounds<Pixels>| {
                actions.top() >= detail.top() - slack
                    && actions.bottom() <= browser.bottom().min(viewport.height) + slack
            };
            let actions = bounds(cx, "stash-actions");
            assert_eq!(whole(actions), pinned, "{at}: the actions {actions:?}");
            if pinned {
                // The footer, below the detail's scrolling part.
                assert!(
                    actions.top() >= detail.bottom(),
                    "{at}: {actions:?} in {detail:?}"
                );
                let file = bounds(cx, "stash-file-0");
                cx.simulate_click(file.center(), Modifiers::none());
                landed(cx, "stash-comparison");
                draw(cx);
                let comparison = bounds(cx, "stash-comparison");
                assert!(
                    comparison.size.height >= floor - slack,
                    "{at}: the chosen file's comparison {comparison:?}"
                );
            }
            // Scrolled to its end, the detail moves its actions only where
            // they are not its footer.
            let before = bounds(cx, "stash-actions");
            let delta = ScrollDelta::Pixels(point(px(0.), px(-10_000.)));
            let middle = bounds(cx, "stash-detail").center();
            scroll_down(cx, middle, delta);
            draw(cx);
            let after = bounds(cx, "stash-actions");
            assert_eq!(after == before, pinned, "{at}: {before:?}, then {after:?}");
            if pinned {
                assert!(whole(after), "{at}: the scrolled actions {after:?}");
            }
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Stacked in a 461 × 490 window, the Stashes dialog's saved-work list
    /// shows its first stash whole at 13 and 18 pt: its unscaled height left
    /// the list 35.6 px at 18 pt, short of an 80 px row. Every line the
    /// dialog draws lies whole inside its clip, and inside its pane, the
    /// list's or the detail's, where the browser holds it, and the detail's
    /// prompt is one whole line: at 18 pt the dialog's scrolling body cut
    /// "Choose a stash… / unstaged and untracked files." mid-line, as the
    /// browser asked for more height than the dialog's cap left it, and at
    /// 13 pt the prompt left "files." alone on its last line.
    #[gpui::test]
    async fn a_narrow_stash_list_keeps_its_first_row_whole(cx: &mut gpui_kit::TestAppContext) {
        use crate::empty_state::tests::{marked_runs, text_mark};
        use crate::views::tests::{draw, fixture_git, history_script, styled_history_window};
        let (_fixture, app, cx) =
            styled_history_window(cx, history_script(1, 1).as_bytes(), |root| {
                root.text_bg(text_mark())
            })
            .await;
        let root = app.read_with(cx, |app, _| app.path.clone().unwrap());
        fixture_git(&root, &["config", "user.name", "Stash Fixture"]);
        fixture_git(&root, &["config", "user.email", "stash@example.invalid"]);
        std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
        fixture_git(&root, &["stash", "push", "-m", "kept for later"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("saved-stash-row-0").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for interface in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            let at = format!("{interface} pt in 461 × 490");
            let list = cx.debug_bounds("stash-list").expect("the saved-work list");
            let row = cx
                .debug_bounds("saved-stash-row-0")
                .expect("the saved stash's row");
            let slack = px(0.5);
            let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
                inner.left() >= outer.left() - slack
                    && inner.right() <= outer.right() + slack
                    && inner.top() >= outer.top() - slack
                    && inner.bottom() <= outer.bottom() + slack
            };
            assert!(
                row.top() >= list.top() - slack && row.bottom() <= list.bottom() + slack,
                "{at}: the row {row:?} is cut by the list {list:?}"
            );
            let [browser, panel, detail, prompt] = [
                "stash-browser",
                "stash-list-panel",
                "stash-detail",
                "stash-detail-placeholder",
            ]
            .map(|name| {
                cx.debug_bounds(name)
                    .unwrap_or_else(|| panic!("{at}: no {name}"))
            });
            let viewport = cx.update(|window, _| window.viewport_size());
            let runs = marked_runs(cx, Bounds::new(Point::default(), viewport));
            for run in &runs {
                assert!(
                    !run.cut,
                    "{at}: the line {:?} crosses its clip {:?}",
                    run.drawn, run.clip
                );
                assert!(
                    !browser.contains(&run.drawn.center())
                        || inside(run.drawn, panel)
                        || inside(run.drawn, detail),
                    "{at}: the line {:?} leaves its pane, the list {panel:?} or the detail {detail:?}",
                    run.drawn
                );
            }
            // The browser ends above the dialog's last line, Close in its
            // footer, rather than past the fold of a body that scrolls it.
            let footer = runs
                .iter()
                .map(|run| run.drawn)
                .max_by(|a, b| a.top().partial_cmp(&b.top()).unwrap())
                .unwrap();
            assert!(
                browser.bottom() <= footer.top(),
                "{at}: the browser {browser:?} ends past the footer's line {footer:?}"
            );
            let lines = runs
                .iter()
                .filter(|run| prompt.contains(&run.drawn.center()))
                .count();
            assert!(
                lines == 1 && inside(prompt, detail),
                "{at}: the prompt {prompt:?} is {lines} drawn lines in the detail {detail:?}"
            );
        }
    }

    /// A stashed document's comparison, at the floor the Stashes dialog keeps
    /// it (a scaled 120 px), says why a click on Open in default app opened
    /// nothing on one whole line beneath its header's controls, its whole
    /// answer in the line's tooltip, and the page beneath gives way below its
    /// minimum for it: at 18 pt in a 1000 × 680 window, the detail scrolled
    /// to the comparison, the room beneath the header held no line above that
    /// minimum, so the line drew none and the click changed nothing on screen.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_stashed_document_says_why_on_one_whole_line(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, scroll_down, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        std::fs::write(&document, pdf("first")).unwrap();
        git(repo.path(), &["add", "document.pdf"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add a document"]);
        std::fs::write(&document, pdf("second")).unwrap();
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let landed = |cx: &mut VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        let file = cx.debug_bounds("stash-file-0").unwrap();
        cx.simulate_click(file.center(), Modifiers::none());
        landed(cx, "system-preview-1");
        // The detail scrolls; at its end the comparison is in view.
        let middle = cx.debug_bounds("stash-detail").unwrap().center();
        scroll_down(cx, middle, ScrollDelta::Pixels(point(px(0.), px(-10_000.))));
        draw(cx);
        crate::system_preview::tests::a_failed_open_answers_on_one_line(
            cx,
            "the Stashes dialog at 18 pt in 1000 × 680",
            [
                "system-preview-1",
                "system-preview-1/note",
                "rich-sides/body",
            ],
            &["stash-comparison", "stash-detail"],
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }
}
