//! Transient native branch and remote workflows. All Git preparation runs on
//! the serialized worker; confirmations retain the exact repository and plan.
use crate::*;
use gitturtle_core::{BranchCommand, BranchPlan, RemoteConfig, WriteCommand};
use gpui_kit::component::{
    WindowExt,
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;

const PREPARING: &str = "Preparing branch action…";
const CHOICE_LIMIT: usize = 40;
type PreparationFailure = Box<dyn FnOnce(String, &mut Window, &mut Context<GitTurtle>)>;

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
}

#[derive(Clone)]
enum ChoicePurpose {
    Track,
    Manage,
    Integrate { rebase: bool },
    Upstream(Arc<BranchPlan>),
}

impl ChoicePurpose {
    fn title(&self) -> String {
        match self {
            Self::Track => "Create a tracking branch".into(),
            Self::Manage => "Manage a local branch".into(),
            Self::Integrate { rebase: false } => "Choose a branch to merge".into(),
            Self::Integrate { rebase: true } => "Choose a new base".into(),
            Self::Upstream(branch) => format!("Upstream for {}", branch.name),
        }
    }
}

struct BranchChoice {
    name: String,
    reference: String,
    oid: String,
    remote: bool,
    search: String,
}

struct RemoteChoice {
    config: Arc<RemoteConfig>,
    search: String,
}

enum Prepared {
    Choices {
        purpose: ChoicePurpose,
        choices: Arc<Vec<BranchChoice>>,
    },
    Local(Arc<BranchPlan>),
    Remote {
        reference: String,
        oid: String,
    },
    Remotes(Arc<Vec<RemoteChoice>>),
    Confirmation {
        title: String,
        explanation: String,
        action: &'static str,
        command: WriteCommand,
    },
}

#[derive(Clone, Copy)]
enum MenuAction {
    ManageCurrent,
    ManageLocal,
    Track,
    Merge,
    Rebase,
    Remotes,
}

impl GitTurtle {
    pub(super) fn cancel_branch_action(&mut self) {
        self.branch_actions.generation = self.branch_actions.generation.wrapping_add(1);
        self.branch_actions.task = None;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }

    /// Replaces the existing picker; its frequent switch actions remain first.
    pub(super) fn render_branch_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let branch = self
            .work_status
            .as_ref()
            .and_then(|status| status.branch.clone())
            .unwrap_or_else(|| "Detached HEAD".into());
        let owner = cx.entity().downgrade();
        Button::new("current-branch")
            .secondary()
            .h(px(34.))
            .px_3()
            .max_w(px(280.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .label(branch.clone())
            .icon(Icon::default().path("icons/branch.svg").size(px(16.)))
            .dropdown_caret(true)
            .disabled(self.operation_busy.is_some())
            .accessibility_label(format!(
                "Current branch: {branch}. Switch or manage branches"
            ))
            .tooltip(format!("{branch} · Switch branch and repository actions"))
            .dropdown_menu(move |mut menu, _, cx| {
                let Some(view) = owner.upgrade() else {
                    return menu;
                };
                let this = view.read(cx);
                let path = this.path.clone();
                let current = this
                    .work_status
                    .as_ref()
                    .and_then(|status| status.branch.as_deref());
                let query = this.branch_name.read(cx).value().trim().to_lowercase();
                menu = menu
                    .label(if query.is_empty() {
                        "Switch branch".to_owned()
                    } else {
                        format!("Branches matching “{query}”")
                    })
                    .max_h(px(430.))
                    .scrollable(true);
                let candidates: Vec<_> = this
                    .branches
                    .iter()
                    .filter(|branch| {
                        !branch.remote
                            && Some(branch.name.as_str()) != current
                            && (query.is_empty() || branch.name.to_lowercase().contains(&query))
                    })
                    .take(CHOICE_LIMIT + 1)
                    .collect();
                if let Some(current) = current {
                    menu = menu.item(
                        PopupMenuItem::new(current.to_owned())
                            .checked(true)
                            .disabled(true),
                    );
                }
                for branch in candidates.iter().take(CHOICE_LIMIT) {
                    let name = branch.name.clone();
                    let occupied = this
                        .worktrees
                        .iter()
                        .find(|tree| tree.branch.as_deref() == Some(name.as_str()));
                    let owner = owner.clone();
                    let path = path.clone();
                    let label = occupied
                        .map_or_else(|| name.clone(), |_| format!("{name} · another worktree"));
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .disabled(occupied.is_some())
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if this.path == path && this.page == AppPage::Repository {
                                        this.write(
                                            WriteCommand::Checkout {
                                                branch: name.clone(),
                                            },
                                            "Switching branch…",
                                            window,
                                            cx,
                                        );
                                    }
                                });
                            }),
                    );
                }
                if candidates.len() > CHOICE_LIMIT {
                    menu = menu.label("Showing 40 branches · Find to narrow the list");
                }
                if candidates.is_empty() {
                    menu = menu.label("No other matching local branches");
                }
                let find_owner = owner.clone();
                let find_path = path.clone();
                menu =
                    menu.separator()
                        .item(PopupMenuItem::new("Find or create a branch…").on_click(
                            move |_, window, cx| {
                                let _ = find_owner.update(cx, |this, cx| {
                                    if this.path == find_path && this.page == AppPage::Repository {
                                        this.git_actions_open = true;
                                        this.branch_name
                                            .read(cx)
                                            .focus_handle(cx)
                                            .focus(window, cx);
                                        cx.notify();
                                    }
                                });
                            },
                        ));
                if current.is_some() {
                    menu = add_menu_action(
                        menu,
                        "Current branch settings…",
                        MenuAction::ManageCurrent,
                        &owner,
                        &path,
                    );
                }
                menu = add_menu_action(
                    menu,
                    "Manage another branch…",
                    MenuAction::ManageLocal,
                    &owner,
                    &path,
                );
                menu = add_menu_action(
                    menu,
                    "Create from a remote branch…",
                    MenuAction::Track,
                    &owner,
                    &path,
                );
                if current.is_some() && this.integration_state.is_none() {
                    menu = add_menu_action(
                        menu.separator(),
                        "Merge a branch…",
                        MenuAction::Merge,
                        &owner,
                        &path,
                    );
                    menu = add_menu_action(
                        menu,
                        "Rebase onto a branch…",
                        MenuAction::Rebase,
                        &owner,
                        &path,
                    );
                }
                add_menu_action(
                    menu.separator(),
                    "Manage remotes…",
                    MenuAction::Remotes,
                    &owner,
                    &path,
                )
            })
            .into_any_element()
    }

    fn branch_menu_action(
        &mut self,
        action: MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            MenuAction::ManageCurrent => {
                if let Some(name) = self
                    .work_status
                    .as_ref()
                    .and_then(|status| status.branch.clone())
                {
                    self.open_contextual_branch(name, false, window, cx);
                }
            }
            MenuAction::ManageLocal => self.choose_branch(ChoicePurpose::Manage, window, cx),
            MenuAction::Track => self.choose_branch(ChoicePurpose::Track, window, cx),
            MenuAction::Merge => {
                self.choose_branch(ChoicePurpose::Integrate { rebase: false }, window, cx)
            }
            MenuAction::Rebase => {
                self.choose_branch(ChoicePurpose::Integrate { rebase: true }, window, cx)
            }
            MenuAction::Remotes => self.open_remote_manager(window, cx),
        }
    }

    pub(super) fn open_contextual_branch(
        &mut self,
        name: String,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(
            move |repo| {
                if remote {
                    let branch = repo
                        .branches()?
                        .into_iter()
                        .find(|branch| branch.remote && branch.name == name)
                        .ok_or_else(|| {
                            anyhow::anyhow!("This remote branch changed; refresh the branch list.")
                        })?;
                    Ok(Prepared::Remote {
                        reference: format!("refs/remotes/{}", branch.name),
                        oid: branch.oid,
                    })
                } else {
                    Ok(Prepared::Local(Arc::new(repo.branch_plan(&name)?)))
                }
            },
            window,
            cx,
        );
    }

    fn choose_branch(
        &mut self,
        purpose: ChoicePurpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(
            move |repo| {
                let current = repo.status()?.branch;
                let choices = repo
                    .branches()?
                    .into_iter()
                    .filter(|branch| match &purpose {
                        ChoicePurpose::Track => branch.remote,
                        ChoicePurpose::Manage => !branch.remote,
                        ChoicePurpose::Integrate { .. } => {
                            branch.remote || Some(&branch.name) != current.as_ref()
                        }
                        ChoicePurpose::Upstream(selected) => {
                            branch.remote || branch.name != selected.name
                        }
                    })
                    .map(|branch| BranchChoice {
                        reference: format!(
                            "refs/{}/{}",
                            if branch.remote { "remotes" } else { "heads" },
                            branch.name
                        ),
                        search: branch.name.to_lowercase(),
                        name: branch.name,
                        oid: branch.oid,
                        remote: branch.remote,
                    })
                    .collect();
                Ok(Prepared::Choices {
                    purpose,
                    choices: Arc::new(choices),
                })
            },
            window,
            cx,
        );
    }

    fn open_remote_manager(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_branch_action(
            |repo| {
                Ok(Prepared::Remotes(Arc::new(
                    repo.remote_configs()?
                        .into_iter()
                        .map(|config| RemoteChoice {
                            search: config.name.to_lowercase(),
                            config: Arc::new(config),
                        })
                        .collect(),
                )))
            },
            window,
            cx,
        );
    }

    fn read_branch_action(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<Prepared> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.read_branch_action_recovering(prepare, None, window, cx)
    }

    fn read_branch_action_recovering(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<Prepared> + Send + 'static,
        on_failure: Option<PreparationFailure>,
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
        self.cancel_branch_action();
        let generation = self.branch_actions.generation;
        self.operation_busy = Some(PREPARING);
        self.operation_error = None;
        let response = self.operations.submit_read(move || prepare(repo));
        self.branch_actions.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Branch preparation was interrupted. Open the action again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.branch_actions.generation != generation {
                    return;
                }
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                if this.path.as_ref() != Some(&path) || this.page != AppPage::Repository {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(prepared) => this.show_prepared_branch_action(prepared, window, cx),
                    Err(error) => {
                        let message = format!("{error:#}");
                        if let Some(on_failure) = on_failure {
                            on_failure(message, window, cx);
                        } else {
                            this.operation_error = Some(message);
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }

    fn show_prepared_branch_action(
        &mut self,
        prepared: Prepared,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match prepared {
            Prepared::Confirmation {
                title,
                explanation,
                action,
                command,
            } => self.confirm_git_write(title, explanation, action, command, window, cx),
            Prepared::Local(plan) => self.show_local_branch(plan, window, cx),
            Prepared::Remote { reference, oid } => {
                self.show_remote_branch(reference, oid, window, cx)
            }
            Prepared::Choices { purpose, choices } => {
                let owner = cx.entity().downgrade();
                let path = self.path.clone();
                let title = purpose.title();
                let chooser =
                    cx.new(|cx| BranchChooser::new(owner, path, purpose, choices, window, cx));
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    let chooser_ok = chooser.clone();
                    dialog
                        .title(title.clone())
                        .w(px(540.))
                        .child(chooser.clone())
                        .button_props(
                            DialogButtonProps::default()
                                .ok_text("Use first match")
                                .cancel_text("Cancel")
                                .show_cancel(true),
                        )
                        .on_ok(move |_, window, cx| {
                            chooser_ok.update(cx, |chooser, cx| chooser.activate_first(window, cx));
                            false
                        })
                });
            }
            Prepared::Remotes(remotes) => {
                let owner = cx.entity().downgrade();
                let path = self.path.clone();
                let manager = cx.new(|cx| RemoteManager::new(owner, path, remotes, window, cx));
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    dialog
                        .title("Remotes")
                        .w(px(580.))
                        .child(manager.clone())
                        .button_props(DialogButtonProps::default().ok_text("Done"))
                });
            }
        }
    }

    fn show_local_branch(
        &mut self,
        plan: Arc<BranchPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let current = plan.current_branch.as_deref() == Some(plan.name.as_str());
        let can_delete = plan.checked_out_in.is_empty() && plan.unmerged_commits == Some(0);
        let own_path = self.path.as_deref();
        let other_worktree = plan
            .checked_out_in
            .iter()
            .any(|tree| Some(tree.as_path()) != own_path);
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let p = palette(cx);
            let rename = Arc::clone(&plan);
            let upstream = Arc::clone(&plan);
            let clear_upstream = Arc::clone(&plan);
            let delete = Arc::clone(&plan);
            let source = format!("refs/heads/{}", plan.name);
            let mut actions = div().flex().flex_wrap().gap_2()
                .child(dialog_action("rename-branch", "Rename…", &owner, &path, other_worktree, move |this, window, cx| this.open_branch_form(BranchFormMode::Rename(Arc::clone(&rename)), window, cx)))
                .child(dialog_action("choose-upstream", "Change upstream…", &owner, &path, other_worktree, move |this, window, cx| this.choose_branch(ChoicePurpose::Upstream(Arc::clone(&upstream)), window, cx)))
                .child(dialog_action("remove-upstream", "Remove upstream…", &owner, &path, other_worktree || plan.upstream.is_none(), move |this, window, cx| this.prepare_upstream(Arc::clone(&clear_upstream), None, window, cx)))
                .child(dialog_action("delete-branch", "Delete branch…", &owner, &path, !can_delete, move |this, window, cx| this.prepare_branch_delete(Arc::clone(&delete), window, cx)));
            if !current && plan.current_branch.is_some() {
                let merge_source = source.clone();
                actions = actions.child(dialog_action("merge-selected-branch", "Merge into current…", &owner, &path, false, move |this, window, cx| this.prepare_integration(merge_source.clone(), false, window, cx)))
                    .child(dialog_action("rebase-selected-branch", "Rebase current onto this…", &owner, &path, false, move |this, window, cx| this.prepare_integration(source.clone(), true, window, cx)));
            }
            let deletion_note = if !plan.checked_out_in.is_empty() {
                format!("Checked out in {}. Switch that worktree to another branch before deleting.{}", path_list(&plan.checked_out_in), if other_worktree { " Rename and upstream changes belong in that worktree." } else { "" })
            } else if let Some(count) = plan.unmerged_commits.filter(|count| *count > 0) {
                format!("{count} commits are outside {}. Merge or preserve them before deleting this branch.", plan.merge_target.as_ref().map_or("HEAD", |target| short_reference(&target.name)))
            } else if let Some(target) = &plan.merge_target { format!("All commits are retained by {}.", short_reference(&target.name)) } else { "No retained target is available to verify safe deletion. Create or select a branch that contains these commits first.".into() };
            let metadata = format!("{}{} · {}", if current { "Current branch · " } else { "" }, short_oid(&plan.oid), plan.upstream.as_deref().map_or("No upstream".into(), |upstream| format!("Tracks {}", short_reference(upstream))));
            let content = div().flex().flex_col().gap_3()
                .child(div().text_size(px(12.)).text_color(rgb(p.muted)).child(metadata))
                .child(div().text_size(px(12.)).child(deletion_note))
                .child(actions);
            dialog.title(plan.name.clone()).w(px(560.)).child(content)
                .button_props(DialogButtonProps::default().ok_text("Done"))
        });
    }

    fn show_remote_branch(
        &mut self,
        reference: String,
        oid: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let current = self
            .work_status
            .as_ref()
            .and_then(|status| status.branch.clone());
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let p = palette(cx);
            let tracking_ref = reference.clone();
            let merge_ref = reference.clone();
            let rebase_ref = reference.clone();
            let actions = div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(dialog_action(
                    "track-remote-branch",
                    "Create local tracking branch…",
                    &owner,
                    &path,
                    false,
                    move |this, window, cx| {
                        this.open_branch_form(
                            BranchFormMode::Track(tracking_ref.clone()),
                            window,
                            cx,
                        )
                    },
                ))
                .child(dialog_action(
                    "merge-remote-branch",
                    "Merge into current…",
                    &owner,
                    &path,
                    current.is_none(),
                    move |this, window, cx| {
                        this.prepare_integration(merge_ref.clone(), false, window, cx)
                    },
                ))
                .child(dialog_action(
                    "rebase-remote-branch",
                    "Rebase current onto this…",
                    &owner,
                    &path,
                    current.is_none(),
                    move |this, window, cx| {
                        this.prepare_integration(rebase_ref.clone(), true, window, cx)
                    },
                ));
            let content = div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(p.muted))
                        .child(format!(
                            "Locally available remote branch · {}",
                            short_oid(&oid)
                        )),
                )
                .child(actions);
            dialog
                .title(short_reference(&reference).to_owned())
                .w(px(540.))
                .child(content)
                .button_props(DialogButtonProps::default().ok_text("Done"))
        });
    }

    fn open_branch_form(
        &mut self,
        mode: BranchFormMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let initial = match &mode {
            BranchFormMode::Rename(plan) => plan.name.clone(),
            BranchFormMode::Track(reference) => {
                let branch = reference.strip_prefix("refs/remotes/").unwrap_or(reference);
                self.remotes
                    .iter()
                    .filter_map(|remote| {
                        branch
                            .strip_prefix(&format!("{}/", remote.name))
                            .map(|name| (remote.name.len(), name))
                    })
                    .max_by_key(|(length, _)| *length)
                    .map(|(_, name)| name.to_owned())
                    .unwrap_or_else(|| {
                        branch
                            .split_once('/')
                            .map_or(branch, |(_, name)| name)
                            .to_owned()
                    })
            }
        };
        let form = cx.new(|cx| BranchForm::new(owner, path, mode, initial, window, cx));
        show_branch_form(form, window, cx);
    }

    fn prepare_branch_delete(
        &mut self,
        expected: Arc<BranchPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(move |repo| {
            let plan = repo.branch_plan(&expected.name)?;
            anyhow::ensure!(plan == *expected, "This branch changed; open its actions again.");
            anyhow::ensure!(plan.checked_out_in.is_empty() && plan.unmerged_commits == Some(0), "Switch away from this branch and preserve its unmerged commits before deleting it.");
            let target = plan.merge_target.as_ref().map_or("HEAD", |target| short_reference(&target.name));
            Ok(Prepared::Confirmation { title: format!("Delete local branch {}", plan.name),
                explanation: format!("Delete the local branch label {} at {}. Its commits are retained by {target}. Working files and remote branches remain available.", plan.name, short_oid(&plan.oid)),
                action: "Delete branch", command: WriteCommand::Branch(BranchCommand::Delete { plan }.into()) })
        }, window, cx);
    }

    fn prepare_upstream(
        &mut self,
        expected: Arc<BranchPlan>,
        upstream: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(move |repo| {
            let plan = repo.upstream_plan(&expected.name, upstream.as_deref())?;
            anyhow::ensure!(plan.branch == *expected, "This branch changed; review its tracking relationship again.");
            let (title, explanation, action) = if let Some(reference) = &plan.upstream_ref {
                (format!("Set upstream for {}", plan.branch.name), format!("Track {} from local branch {}. Ahead/behind counts and future pull defaults will use this relationship. This updates local configuration and does not fetch or push.", short_reference(reference), plan.branch.name), "Set upstream")
            } else {
                (format!("Remove upstream from {}", plan.branch.name), format!("Remove {} as the upstream for {}. Future pulls need an explicit destination. The branch and its commits stay in place.", plan.branch.upstream.as_deref().map_or("the current target", short_reference), plan.branch.name), "Remove upstream")
            };
            Ok(Prepared::Confirmation { title, explanation, action, command: WriteCommand::Branch(BranchCommand::SetUpstream { plan }.into()) })
        }, window, cx);
    }

    fn open_remote_form(
        &mut self,
        expected: Option<Arc<RemoteConfig>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let form = cx.new(|cx| RemoteForm::new(owner, path, expected, window, cx));
        show_remote_form(form, window, cx);
    }

    fn prepare_remote_remove(
        &mut self,
        expected: Arc<RemoteConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(move |repo| {
            let current = repo.remote_configs()?.into_iter().find(|remote| remote.name == expected.name)
                .ok_or_else(|| anyhow::anyhow!("This remote no longer exists; refresh the remote list."))?;
            anyhow::ensure!(current == *expected, "The remote or its tracking references changed; review it again.");
            let mut explanation = format!("Remove local configuration for {}. The remote server is unchanged.\n\n{} upstream relationships will be disconnected; {} unshared remote-tracking references will be removed. Local branches and working files stay in place.", current.name, current.upstream_branches.len(), current.tracking_refs.len());
            append_names(&mut explanation, "Upstream branches", current.upstream_branches.iter().map(String::as_str));
            append_names(&mut explanation, "Remote-tracking references", current.tracking_refs.iter().map(|reference| short_reference(&reference.name)));
            Ok(Prepared::Confirmation { title: format!("Remove remote {}", current.name), explanation, action: "Remove remote", command: WriteCommand::Branch(BranchCommand::RemoveRemote { expected: current }.into()) })
        }, window, cx);
    }
}

fn add_menu_action(
    mut menu: PopupMenu,
    label: &'static str,
    action: MenuAction,
    owner: &WeakEntity<GitTurtle>,
    path: &Option<PathBuf>,
) -> PopupMenu {
    let owner = owner.clone();
    let path = path.clone();
    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
        let _ = owner.update(cx, |this, cx| {
            if this.path == path && this.page == AppPage::Repository {
                this.branch_menu_action(action, window, cx);
            }
        });
    }));
    menu
}

fn dialog_action(
    id: &'static str,
    label: &'static str,
    owner: &WeakEntity<GitTurtle>,
    path: &Option<PathBuf>,
    disabled: bool,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> Button {
    let owner = owner.clone();
    let path = path.clone();
    button(id, label, "", false)
        .secondary()
        .disabled(disabled)
        .on_click(move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.path == path
                    && this.page == AppPage::Repository
                    && this.operation_busy.is_none()
                {
                    window.close_dialog(cx);
                    action(this, window, cx);
                }
            });
        })
}

fn short_reference(reference: &str) -> &str {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/"))
        .unwrap_or(reference)
}

fn path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn append_names<'a>(text: &mut String, label: &str, names: impl Iterator<Item = &'a str>) {
    let names: Vec<_> = names.collect();
    if names.is_empty() {
        return;
    }
    text.push_str(&format!(
        "\n\n{label}:\n{}",
        names
            .iter()
            .take(12)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    ));
    if names.len() > 12 {
        text.push_str(&format!("\n…and {} more", names.len() - 12));
    }
}

fn field_label(label: &'static str, cx: &App) -> AnyElement {
    div()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(palette(cx).text))
        .child(label)
        .into_any_element()
}

fn show_branch_form(form: Entity<BranchForm>, window: &mut Window, cx: &mut App) {
    let title = match &form.read(cx).mode {
        BranchFormMode::Rename(_) => "Rename branch",
        BranchFormMode::Track(_) => "Create tracking branch",
    };
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let submit = form.clone();
        dialog
            .title(title)
            .w(px(520.))
            .child(form.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Review action")
                    .cancel_text("Cancel")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}

fn show_remote_form(form: Entity<RemoteForm>, window: &mut Window, cx: &mut App) {
    let title = form
        .read(cx)
        .expected
        .as_ref()
        .map_or("Add remote".into(), |remote| {
            format!("Edit {}", remote.name)
        });
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let submit = form.clone();
        dialog
            .title(title.clone())
            .w(px(580.))
            .child(form.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Review settings")
                    .cancel_text("Cancel")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}

struct BranchChooser {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    purpose: ChoicePurpose,
    choices: Arc<Vec<BranchChoice>>,
    query: Entity<InputState>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl BranchChooser {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        purpose: ChoicePurpose,
        choices: Arc<Vec<BranchChoice>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find a branch"));
        let subscription =
            cx.subscribe_in(&query, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.scroll = ScrollHandle::new();
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.activate_first(window, cx),
                _ => {}
            });
        Self {
            owner,
            path,
            purpose,
            choices,
            query,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    }

    fn matching(&self, cx: &App) -> Vec<usize> {
        let query = self.query.read(cx).value().trim().to_lowercase();
        self.choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| query.is_empty() || choice.search.contains(&query))
            .take(CHOICE_LIMIT + 1)
            .map(|(index, _)| index)
            .collect()
    }

    fn activate_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.matching(cx).first().copied() {
            self.activate(index, window, cx);
        }
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = self.choices.get(index) else {
            return;
        };
        let reference = choice.reference.clone();
        let name = choice.name.clone();
        let purpose = self.purpose.clone();
        let _ = self.owner.update(cx, |this, cx| {
            if this.path != self.path
                || this.page != AppPage::Repository
                || this.operation_busy.is_some()
            {
                return;
            }
            window.close_dialog(cx);
            match purpose {
                ChoicePurpose::Track => {
                    this.open_branch_form(BranchFormMode::Track(reference), window, cx)
                }
                ChoicePurpose::Manage => this.open_contextual_branch(name, false, window, cx),
                ChoicePurpose::Integrate { rebase } => {
                    this.prepare_integration(reference, rebase, window, cx)
                }
                ChoicePurpose::Upstream(plan) => {
                    this.prepare_upstream(plan, Some(reference), window, cx)
                }
            }
        });
    }
}

impl Render for BranchChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let matching = self.matching(cx);
        let detail = match &self.purpose {
            ChoicePurpose::Track => {
                "Choose a locally available remote branch. A new local branch will track it."
            }
            ChoicePurpose::Manage => {
                "Choose a branch to rename, delete safely, or configure its upstream."
            }
            ChoicePurpose::Integrate { rebase: false } => {
                "The selected branch will be merged into the current branch. Review both tips before proceeding."
            }
            ChoicePurpose::Integrate { rebase: true } => {
                "The current branch's commits will be replayed onto the selected branch. Review the history change before proceeding."
            }
            ChoicePurpose::Upstream(_) => {
                "Choose the local or remote branch used for tracking and ahead/behind counts."
            }
        };
        div().flex().flex_col().gap_3()
            .child(div().text_size(px(12.)).text_color(rgb(p.muted)).child(detail))
            .child(Input::new(&self.query).cleanable(true).prefix(Icon::default().path("icons/search.svg").size(px(14.))))
            .child(div().id("branch-chooser-list").max_h(px(330.)).overflow_y_scroll().track_scroll(&self.scroll).flex().flex_col().gap_1()
                .children(matching.iter().take(CHOICE_LIMIT).map(|index| {
                    let index = *index;
                    let choice = &self.choices[index];
                    Button::new(("branch-choice", index)).ghost().w_full().h(px(34.))
                        .text_size(px(12.)).accessibility_label(choice.name.clone())
                        .child(div().w_full().min_w_0().flex().items_center().justify_start().gap_2()
                            .child(Icon::default().path(if choice.remote { "icons/remote.svg" } else { "icons/branch.svg" }).size(px(14.)))
                            .child(div().flex_1().min_w_0().truncate().child(choice.name.clone())))
                        .tooltip(format!("{} · {}", choice.reference, short_oid(&choice.oid)))
                        .on_click(cx.listener(move |this, _, window, cx| this.activate(index, window, cx)))
                }))
                .when(matching.is_empty(), |element| element.child(div().p_3().text_size(px(12.)).text_color(rgb(p.muted)).child(if self.choices.is_empty() { "No branches are available for this action. Fetch explicitly to update remote branches." } else { "No branches match this search." }))))
            .when(matching.len() > CHOICE_LIMIT, |element| element.child(div().text_size(px(11.)).text_color(rgb(p.muted)).child("Showing 40 matches. Narrow the search to find another branch.")))
    }
}

#[derive(Clone)]
enum BranchFormMode {
    Rename(Arc<BranchPlan>),
    Track(String),
}

struct BranchForm {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    mode: BranchFormMode,
    name: Entity<InputState>,
    checkout: bool,
    error: Option<String>,
    _subscription: Subscription,
}

impl BranchForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        mode: BranchFormMode,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial)
                .placeholder("feature/my-change")
        });
        let subscription =
            cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } if this.submit(window, cx) => {
                    window.close_dialog(cx);
                }
                _ => {}
            });
        Self {
            owner,
            path,
            mode,
            name,
            checkout: true,
            error: None,
            _subscription: subscription,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.error = Some("Enter a branch name.".into());
            cx.notify();
            return false;
        }
        let mode = self.mode.clone();
        let checkout = self.checkout;
        let form = cx.entity();
        let recover: PreparationFailure = Box::new(move |message, window, cx| {
            form.update(cx, |form, cx| {
                form.error = Some(message);
                cx.notify();
            });
            show_branch_form(form, window, cx);
        });
        let accepted = self.owner.update(cx, |this, cx| {
            if this.path != self.path { return false; }
            this.read_branch_action_recovering(move |repo| match mode {
                BranchFormMode::Rename(expected) => {
                    let plan = repo.rename_branch_plan(expected.as_ref(), &name)?;
                    let explanation = format!("Rename the local branch {} at {} to {name}. Its commit history and tracking configuration move to the new name.{}", plan.name, short_oid(&plan.oid), if plan.current_branch.as_deref() == Some(plan.name.as_str()) { " This is the current branch." } else { "" });
                    Ok(Prepared::Confirmation { title: format!("Rename {} to {name}", plan.name), explanation, action: "Rename branch", command: WriteCommand::Branch(BranchCommand::Rename { plan, new_name: name }.into()) })
                }
                BranchFormMode::Track(reference) => {
                    let plan = repo.tracking_branch_plan(&name, &reference, checkout)?;
                    let explanation = format!("Create local branch {name} at {} and track {}.{} The locally available remote tip is used; this action does not fetch.", short_oid(&plan.remote_oid), short_reference(&reference), if checkout { " Git will also switch this worktree to the new branch and preserve unrelated work." } else { " The current branch stays selected." });
                    Ok(Prepared::Confirmation { title: format!("Create tracking branch {name}"), explanation, action: if checkout { "Create and switch" } else { "Create branch" }, command: WriteCommand::Branch(BranchCommand::CreateTracking { plan }.into()) })
                }
            }, Some(recover), window, cx)
        }).unwrap_or(false);
        if !accepted {
            self.error = Some("The repository changed or another operation is running. Close this dialog and open the action again.".into());
            cx.notify();
        }
        accepted
    }
}

impl Render for BranchForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let target = match &self.mode {
            BranchFormMode::Rename(plan) => format!("{} · {}", plan.name, short_oid(&plan.oid)),
            BranchFormMode::Track(reference) => format!("Track {}", short_reference(reference)),
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(p.muted))
                    .child(target),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field_label("Local branch name", cx))
                    .child(Input::new(&self.name)),
            )
            .when(matches!(&self.mode, BranchFormMode::Track(_)), |element| {
                element.child(
                    Checkbox::new("checkout-new-tracking-branch")
                        .label("Switch to the new branch")
                        .checked(self.checkout)
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.checkout = *checked;
                            cx.notify();
                        })),
                )
            })
            .children(self.error.as_ref().map(|error| {
                div()
                    .text_size(px(12.))
                    .text_color(rgb(p.warning))
                    .child(error.clone())
            }))
    }
}

struct RemoteManager {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    remotes: Arc<Vec<RemoteChoice>>,
    query: Entity<InputState>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl RemoteManager {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        remotes: Arc<Vec<RemoteChoice>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find a remote"));
        let subscription = cx.subscribe(&query, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.scroll = ScrollHandle::new();
                cx.notify();
            }
        });
        Self {
            owner,
            path,
            remotes,
            query,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    }
}

impl Render for RemoteManager {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let query = self.query.read(cx).value().trim().to_lowercase();
        let matches: Vec<_> = self
            .remotes
            .iter()
            .enumerate()
            .filter(|(_, remote)| query.is_empty() || remote.search.contains(&query))
            .take(CHOICE_LIMIT + 1)
            .collect();
        div().flex().flex_col().gap_3()
            .child(div().flex().items_center().gap_2()
                .child(div().flex_1().child(Input::new(&self.query).cleanable(true)))
                .child(dialog_action("add-remote", "Add remote…", &self.owner, &self.path, false, |this, window, cx| this.open_remote_form(None, window, cx))))
            .child(div().text_size(px(12.)).text_color(rgb(p.muted)).child("Configure local destinations. Network activity starts only when you explicitly fetch, pull, or push."))
            .child(div().id("remote-manager-list").max_h(px(360.)).overflow_y_scroll().track_scroll(&self.scroll).flex().flex_col().gap_2()
                .children(matches.iter().take(CHOICE_LIMIT).map(|(index, choice)| {
                    let edit = Arc::clone(&choice.config);
                    let remove = Arc::clone(&choice.config);
                    let owner = self.owner.clone(); let path = self.path.clone();
                    let remove_owner = self.owner.clone(); let remove_path = self.path.clone();
                    let remote = &choice.config;
                    div().id(("managed-remote", *index)).p_3().rounded(px(8.)).border_1().border_color(rgb(p.border)).flex().items_center().gap_3()
                        .child(div().flex_1().min_w_0().flex().flex_col().gap_1()
                            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(remote.name.clone()))
                            .child(div().truncate().text_size(px(11.)).text_color(rgb(p.muted)).child(remote.urls.first().map_or("No fetch URL configured".into(), |url| workspace::display_remote_url(url))))
                            .child(div().text_size(px(11.)).text_color(rgb(p.muted)).child(format!("{} fetch URLs · {} upstream branches", remote.urls.len(), remote.upstream_branches.len()))))
                        .child(button(("edit-remote", *index), "Edit…", "", false).on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| { if this.path == path && this.page == AppPage::Repository && this.operation_busy.is_none() { window.close_dialog(cx); this.open_remote_form(Some(Arc::clone(&edit)), window, cx); } });
                        }))
                        .child(button(("remove-remote", *index), "Remove…", "", false).on_click(move |_, window, cx| {
                            let _ = remove_owner.update(cx, |this, cx| { if this.path == remove_path && this.page == AppPage::Repository && this.operation_busy.is_none() { window.close_dialog(cx); this.prepare_remote_remove(Arc::clone(&remove), window, cx); } });
                        }))
                }))
                .when(matches.is_empty(), |element| element.child(div().p_3().text_size(px(12.)).text_color(rgb(p.muted)).child(if self.remotes.is_empty() { "No remotes configured. Add a destination to fetch or publish your work." } else { "No remotes match this search." }))))
            .when(matches.len() > CHOICE_LIMIT, |element| element.child(div().text_size(px(11.)).text_color(rgb(p.muted)).child("Showing 40 matches. Narrow the search to find another remote.")))
    }
}

struct RemoteForm {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    expected: Option<Arc<RemoteConfig>>,
    name: Entity<InputState>,
    fetch: Entity<TextareaState>,
    push: Entity<TextareaState>,
    rules: Entity<TextareaState>,
    inherit_push: bool,
    store_refs: bool,
    advanced: bool,
    error: Option<String>,
}

impl RemoteForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        expected: Option<Arc<RemoteConfig>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            InputState::new(window, cx).default_value(
                expected
                    .as_ref()
                    .map_or("origin".into(), |remote| remote.name.clone()),
            )
        });
        let fetch = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(if expected.is_some() {
                "Leave blank to keep current fetch URLs"
            } else {
                "https://host/owner/repository.git"
            })
        });
        let push = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("One push URL per line; blank keeps current URLs")
        });
        let rules = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Blank uses the standard branch mapping")
                .default_value(
                    expected
                        .as_ref()
                        .map_or(String::new(), |remote| remote.fetch_refspecs.join("\n")),
                )
        });
        let inherit_push = expected
            .as_ref()
            .is_none_or(|remote| remote.push_urls.is_empty());
        let store_refs = expected
            .as_ref()
            .is_none_or(|remote| !remote.fetch_refspecs.is_empty());
        Self {
            owner,
            path,
            expected,
            name,
            fetch,
            push,
            rules,
            inherit_push,
            store_refs,
            advanced: false,
            error: None,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let name = self.name.read(cx).value().trim().to_owned();
        let fetch = input_lines(&self.fetch.read(cx).value());
        let push = input_lines(&self.push.read(cx).value());
        let rules = input_lines(&self.rules.read(cx).value());
        if name.is_empty() || (self.expected.is_none() && fetch.is_empty()) {
            self.error = Some("Enter a remote name and at least one fetch URL.".into());
            cx.notify();
            return false;
        }
        if !self.inherit_push
            && push.is_empty()
            && self
                .expected
                .as_ref()
                .is_none_or(|remote| remote.push_urls.is_empty())
        {
            self.error = Some("Enter a push URL or choose to use the fetch URLs.".into());
            cx.notify();
            return false;
        }
        let expected = self.expected.clone();
        let inherit_push = self.inherit_push;
        let store_refs = self.store_refs;
        let form = cx.entity();
        let recover: PreparationFailure = Box::new(move |message, window, cx| {
            form.update(cx, |form, cx| {
                form.error = Some(message);
                cx.notify();
            });
            show_remote_form(form, window, cx);
        });
        let accepted = self.owner.update(cx, |this, cx| {
            if this.path != self.path { return false; }
            this.read_branch_action_recovering(move |repo| {
                if let Some(expected) = &expected {
                    let current = repo.remote_configs()?.into_iter().find(|remote| remote.name == expected.name).ok_or_else(|| anyhow::anyhow!("This remote no longer exists; reopen remote settings."))?;
                    anyhow::ensure!(current == **expected, "The remote changed; reopen its settings before editing it.");
                } else {
                    anyhow::ensure!(!repo.remote_configs()?.iter().any(|remote| remote.name == name), "A remote named '{name}' already exists.");
                }
                let mut replacement = expected.as_ref().map_or_else(|| RemoteConfig::new(&name, fetch.first().cloned().unwrap_or_default()), |remote| remote.as_ref().clone());
                if !fetch.is_empty() { replacement.urls = fetch; }
                if inherit_push { replacement.push_urls.clear(); } else if !push.is_empty() { replacement.push_urls = push; }
                if !store_refs { replacement.fetch_refspecs.clear(); }
                else if !rules.is_empty() { replacement.fetch_refspecs = rules; }
                else if replacement.fetch_refspecs.is_empty() { replacement.fetch_refspecs = RemoteConfig::new(&name, "").fetch_refspecs; }
                let mut explanation = format!("{} the local configuration for {name}. Future fetch and push actions use these destinations. Saving these settings does not contact a server.", if expected.is_some() { "Update" } else { "Add" });
                let fetch_display: Vec<_> = replacement.urls.iter().map(|url| workspace::display_remote_url(url)).collect();
                append_names(&mut explanation, "Fetch URLs", fetch_display.iter().map(String::as_str));
                if replacement.push_urls.is_empty() { explanation.push_str("\n\nPush uses the fetch URLs."); }
                else {
                    let push_display: Vec<_> = replacement.push_urls.iter().map(|url| workspace::display_remote_url(url)).collect();
                    append_names(&mut explanation, "Push URLs", push_display.iter().map(String::as_str));
                }
                append_names(&mut explanation, "Fetch rules", replacement.fetch_refspecs.iter().map(String::as_str));
                if replacement.fetch_refspecs.is_empty() { explanation.push_str("\n\nFetched branches will not be stored as remote-tracking references."); }
                let command = if let Some(expected) = expected { BranchCommand::EditRemote { expected: expected.as_ref().clone(), replacement } } else { BranchCommand::AddRemote { remote: replacement } };
                Ok(Prepared::Confirmation { title: format!("Save remote {name}"), explanation, action: "Save remote", command: WriteCommand::Branch(command.into()) })
            }, Some(recover), window, cx)
        }).unwrap_or(false);
        if !accepted {
            self.error = Some("The repository changed or another operation is running. Reopen remote settings when it finishes.".into());
            cx.notify();
        }
        accepted
    }
}

impl Render for RemoteForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div().flex().flex_col().gap_3()
            .child(div().flex().flex_col().gap_1().child(field_label("Remote name", cx)).child(Input::new(&self.name).disabled(self.expected.is_some())))
            .children(self.expected.as_ref().map(|remote| {
                div().text_size(px(11.)).text_color(rgb(p.muted)).child(format!("Current fetch: {}", remote.urls.iter().map(|url| workspace::display_remote_url(url)).collect::<Vec<_>>().join(" · ")))
            }))
            .child(div().flex().flex_col().gap_1().child(field_label(if self.expected.is_some() { "New fetch URLs" } else { "Fetch URLs" }, cx))
                .child(Textarea::new(&self.fetch).h(px(70.)).aria_label("Fetch URLs, one per line"))
                .child(div().text_size(px(11.)).text_color(rgb(p.muted)).child(if self.expected.is_some() { "One URL per line. Leave blank to preserve the existing URLs." } else { "One URL or local repository path per line." })))
            .child(Checkbox::new("remote-push-inherits-fetch").label("Use fetch URLs for push").checked(self.inherit_push)
                .on_click(cx.listener(|this, checked, _, cx| { this.inherit_push = *checked; cx.notify(); })))
            .when(!self.inherit_push, |element| element.child(div().flex().flex_col().gap_1().child(field_label("Push URLs", cx))
                .children(self.expected.as_ref().filter(|remote| !remote.push_urls.is_empty()).map(|remote| div().text_size(px(11.)).text_color(rgb(p.muted)).child(format!("Current: {}", remote.push_urls.iter().map(|url| workspace::display_remote_url(url)).collect::<Vec<_>>().join(" · ")))))
                .child(Textarea::new(&self.push).h(px(70.)).aria_label("Push URLs, one per line"))))
            .child(button("remote-advanced-settings", if self.advanced { "Hide fetch rules" } else { "Fetch rules…" }, "", false)
                .on_click(cx.listener(|this, _, _, cx| { this.advanced = !this.advanced; cx.notify(); })))
            .when(self.advanced, |element| element.child(div().flex().flex_col().gap_2()
                .child(Checkbox::new("remote-store-tracking-refs").label("Store fetched branches as remote-tracking references").checked(self.store_refs)
                    .on_click(cx.listener(|this, checked, _, cx| { this.store_refs = *checked; cx.notify(); })))
                .when(self.store_refs, |element| element.child(Textarea::new(&self.rules).h(px(90.)).aria_label("Git fetch refspecs, one per line")))
                .child(div().text_size(px(11.)).text_color(rgb(p.muted)).child("Advanced Git refspecs, one per line. The standard mapping tracks every remote branch under this remote's name."))))
            .children(self.error.as_ref().map(|error| div().text_size(px(12.)).text_color(rgb(p.warning)).child(error.clone())))
    }
}

fn input_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}
