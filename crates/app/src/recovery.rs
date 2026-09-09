//! Explicit, prepared recovery actions and a read-only stash inspector.
use crate::*;
use gitturtle_core::{
    CommitRecoveryPlan, RecoveryCommand, RecoveryKind, StashPage, StashSnapshot, WriteCommand,
};
use gpui_kit::component::{
    WindowExt,
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;

const PREPARING: &str = "Preparing recovery action…";
const STASH_PAGE_SIZE: usize = 40;
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

    pub(super) fn render_working_recovery_menu(&self, cx: &mut Context<Self>) -> AnyElement {
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
        Button::new("working-recovery-menu")
            .small()
            .ghost()
            .label("Actions")
            .dropdown_caret(true)
            .accessibility_label("Working changes and recovery actions")
            .tooltip("Ignore selected untracked content, stashes, and last-commit actions")
            .disabled(self.operation_busy.is_some())
            .dropdown_menu(move |menu, _, _| {
                let ignore_entry = ignore_entry.clone();
                let menu = working_menu_item(
                    menu,
                    "Ignore selected untracked file…",
                    ignore_entry.is_none(),
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
                    !has_changes || paused,
                    &owner,
                    &repository,
                    |this, window, cx| this.open_stash_form(window, cx),
                );
                let menu = working_menu_item(
                    menu,
                    "Browse stashes…",
                    false,
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
                    paused,
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
                    paused,
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
            .into_any_element()
    }

    pub(super) fn render_commit_recovery_menu(
        &self,
        commit: &Commit,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let commit = commit_target(commit);
        Button::new("commit-recovery-menu")
            .small()
            .ghost()
            .label("Actions")
            .dropdown_caret(true)
            .accessibility_label("Selected commit actions")
            .disabled(self.operation_busy.is_some())
            .dropdown_menu(move |menu, _, cx| commit_menu(menu, &owner, &repository, &commit, cx))
            .into_any_element()
    }

    pub(super) fn recovery_context_menu(
        menu: PopupMenu,
        owner: &WeakEntity<Self>,
        repository: &Option<PathBuf>,
        commit: &Arc<CommitTarget>,
        cx: &App,
    ) -> PopupMenu {
        commit_menu(menu, owner, repository, commit, cx)
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
            window.open_alert_dialog(cx, move |dialog, _, _| {
                dialog.title("Choose the mainline parent").width(px(560.))
                    .child(div().text_size(crate::appearance::ui_text(12.)).child(format!("{} is a merge commit. Choose the parent against which its change should be calculated.", short_oid(&commit.oid))))
                    .child(div().id("recovery-mainline-parents").max_h(px(320.)).overflow_y_scroll().flex().flex_col().gap_2()
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
                        })))
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

    fn open_stashes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let owner = cx.entity().downgrade();
        let browser = cx.new(|cx| StashBrowser::new(owner, repo, window, cx));
        let width = (f32::from(window.viewport_size().width) - 48.).clamp(400., 1060.);
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Stashes")
                .width(px(width))
                .child(browser.clone())
                .button_props(DialogButtonProps::default().ok_text("Close"))
        });
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
                            form.error = self.operation_error.clone();
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
                            form.error = self.operation_error.clone();
                            cx.notify();
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn working_menu_item(
    menu: PopupMenu,
    label: &'static str,
    disabled: bool,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> PopupMenu {
    let owner = owner.clone();
    let repository = repository.clone();
    menu.item(
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
            }),
    )
}

fn commit_menu(
    mut menu: PopupMenu,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    commit: &Arc<CommitTarget>,
    cx: &App,
) -> PopupMenu {
    let Some(this) = owner.upgrade() else {
        return menu;
    };
    let this = this.read(cx);
    let disabled = this.operation_busy.is_some() || this.integration_state.is_some();
    let current = this
        .work_status
        .as_ref()
        .and_then(|status| status.head.as_ref())
        == Some(&commit.oid);
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
                    .child(
                        Textarea::new(&self.description)
                            .h(px(170.))
                            .aria_label("Amended commit description"),
                    ),
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
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let submit = form.clone();
        dialog
            .title("Amend last commit")
            .width(px(580.))
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
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let submit = form.clone();
        dialog
            .title("Save changes to stash")
            .width(px(520.))
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
struct StashBrowser {
    owner: WeakEntity<GitTurtle>,
    repository: GitRepository,
    metadata: SerialExecutor,
    preview_worker: Worker,
    generation: u64,
    preview_generation: u64,
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
    patch_view: Option<Entity<diff_view::DiffView>>,
    stash_scroll: UniformListScrollHandle,
    file_scroll: UniformListScrollHandle,
    stash_focus: FocusHandle,
    file_focus: FocusHandle,
    loading: Option<&'static str>,
    error: Option<String>,
    preview_error: Option<String>,
    restore_index: bool,
}

impl StashBrowser {
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
            patch_view: None,
            stash_scroll: UniformListScrollHandle::new(),
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

    fn clear_preview(&mut self) {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.preview_worker.cancel();
        self.preview_task = None;
        self.content = None;
        self.editors = [None, None, None];
        self.patch_view = None;
        self.preview_error = None;
    }

    fn load_page(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.clear_preview();
        self.snapshot = None;
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
        let editor = text::editor(
            value,
            if self.text_mode == 0 { "diff" } else { "text" },
            (self.text_mode == 0).then_some(presentation.as_ref()),
            window,
            cx,
        );
        if self.text_mode == 0 {
            self.patch_view = Some(diff_view::new(editor.clone(), presentation, window, cx));
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

    fn render_content(&self, cx: &App) -> AnyElement {
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
            return div()
                .p_4()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.muted))
                .child("Reading saved file…")
                .into_any_element();
        }
        match self.content.as_deref() {
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
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when_some(side.render.clone(), |element, image| {
                                            element.child(
                                                img(image)
                                                    .size_full()
                                                    .object_fit(ObjectFit::Contain),
                                            )
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
            _ => div()
                .p_4()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.muted))
                .child("Select a saved file to inspect its exact comparison.")
                .into_any_element(),
        }
    }
}

impl Render for StashBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
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
        div().h(px(height)).flex().gap_3().when(narrow, |element| element.flex_col())
            .child(div().w(px(255.)).min_w_0().flex_shrink_0().h_full().flex().flex_col().border_1().border_color(rgb(p.border)).rounded(px(7.)).overflow_hidden()
                .when(narrow, |element| element.w_full().h(px(140.)))
                .child(div().h(crate::appearance::ui_size(38.)).flex().items_center().px_2().gap_1().border_b_1().border_color(rgb(p.border))
                    .child(div().flex_1().text_size(crate::appearance::ui_text(11.)).font_weight(FontWeight::SEMIBOLD).child("Saved work"))
                    .child(button("refresh-stashes", "", "refresh", false).accessibility_label("Refresh saved stashes").disabled(busy).on_click(cx.listener(|this, _, window, cx| this.load_page(0, window, cx)))))
                .child(div().id("stash-list-focus").role(Role::ListBox).aria_label("Saved stashes").tab_stop(true).track_focus(&self.stash_focus).flex_1().min_h_0()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if let Some(index) = list_key_selection(event.keystroke.key.as_str(), this.selected, this.page.as_ref().map_or(0, |page| page.entries.len())) {
                            cx.stop_propagation();
                            this.select_stash(index, window, cx);
                            this.stash_scroll.scroll_to_item(index, ScrollStrategy::Center);
                        }
                    }))
                    .when(self.page.as_ref().is_some_and(|page| !page.entries.is_empty()), |element| element.child(list))
                    .when(self.page.as_ref().is_some_and(|page| page.entries.is_empty()), |element| element.child(div().p_4().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child("No saved stashes. Save current changes from the Actions menu in Changes."))))
                .child(div().h(crate::appearance::ui_size(36.)).flex().items_center().justify_between().px_2().border_t_1().border_color(rgb(p.border))
                    .child(button("previous-stashes", "Previous", "", false).disabled(busy || self.offset == 0).on_click(cx.listener(|this, _, window, cx| this.load_page(this.offset.saturating_sub(STASH_PAGE_SIZE), window, cx))))
                    .child(button("next-stashes", "More", "", false).disabled(busy || self.page.as_ref().and_then(|page| page.next_offset).is_none()).on_click(cx.listener(|this, _, window, cx| { if let Some(offset) = this.page.as_ref().and_then(|page| page.next_offset) { this.load_page(offset, window, cx); } })))))
            .child(div().flex_1().min_w_0().min_h_0().flex().flex_col().gap_2()
                .children(self.loading.map(|loading| div().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child(loading)))
                .children(self.error.as_ref().map(|error| form_error(error, cx)))
                .when_some(snapshot, |element, snapshot| element
                    .child(div().flex().flex_col().gap_1()
                        .child(div().truncate().text_size(crate::appearance::ui_text(14.)).font_weight(FontWeight::SEMIBOLD).child(snapshot.stash.name.clone()))
                        .child(div().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)).child(format!("{} · base {} · saved index {}", short_oid(&snapshot.stash.oid), short_oid(&snapshot.base_oid), short_oid(&snapshot.index_oid)))))
                    .child(div().flex().flex_wrap().items_center().gap_1().children([StashArea::Staged, StashArea::Unstaged, StashArea::Untracked].into_iter().map(|area| {
                        button(("stash-area", area as usize), format!("{} ({})", area.label(), area.files(&snapshot).len()), "", self.area == area).on_click(cx.listener(move |this, _, _, cx| this.choose_area(area, cx)))
                    })))
                    .child(div().id("stash-files-focus").role(Role::ListBox).aria_label("Saved files").tab_stop(true).track_focus(&self.file_focus).h(px(if narrow { 85. } else { 130. })).flex_shrink_0().border_1().border_color(rgb(p.border)).rounded(px(5.)).overflow_hidden()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if let Some(index) = list_key_selection(event.keystroke.key.as_str(), this.selected_file, this.files().len()) {
                                cx.stop_propagation();
                                this.select_file(index, window, cx);
                                this.file_scroll.scroll_to_item(index, ScrollStrategy::Center);
                            }
                        }))
                        .when(!self.files().is_empty(), |element| element.child(files))
                        .when(self.files().is_empty(), |element| element.child(div().p_3().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("No saved files in this group."))))
                    .child(div().flex().items_center().gap_1().children(["Diff", "Before", "After"].into_iter().enumerate().map(|(mode, label)| {
                        button(("stash-text-mode", mode), label, "", self.text_mode == mode).disabled(!matches!(self.content.as_deref(), Some(Content::Text { .. }))).on_click(cx.listener(move |this, _, window, cx| { this.text_mode = mode; this.ensure_editor(window, cx); cx.notify(); }))
                    })))
                    .child(div().flex_1().min_h_0().border_1().border_color(rgb(p.border)).rounded(px(5.)).overflow_hidden().child(self.render_content(cx)))
                    .child(div().flex().flex_wrap().items_center().gap_2().pt_1()
                        .child(Checkbox::new("stash-restore-index").label("Restore staged state").checked(self.restore_index).on_click(cx.listener(|this, checked, _, cx| { this.restore_index = *checked; cx.notify(); })))
                        .child(div().flex_1())
                        .child(button("drop-saved-stash", "Drop…", "", false).disabled(busy).on_click(cx.listener(|this, _, window, cx| this.drop_stash(window, cx))))
                        .child(Button::new("restore-saved-stash").primary().small().label("Review restore…").disabled(busy).on_click(cx.listener(|this, _, window, cx| this.restore(window, cx)))))
                    .child(div().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)).child("Restoring keeps this stash saved. Drop it separately after checking the restored work.")))
                .when(self.snapshot.is_none() && self.loading.is_none() && self.error.is_none(), |element| element.child(div().flex_1().flex().items_center().justify_center().p_5().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child("Choose a stash to inspect its staged, unstaged and untracked files."))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn amend_fields_preserve_description_whitespace_and_comment_lines() {
        let message = "Title\n\n  indented\n# literal description\n\n";
        let draft = split_message(message);
        assert_eq!(draft.title, "Title");
        assert_eq!(draft.description, "  indented\n# literal description\n\n");
        assert_eq!(draft.message(), message);
    }
    #[::core::prelude::v1::test]
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

    #[::core::prelude::v1::test]
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

    #[::core::prelude::v1::test]
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
}
