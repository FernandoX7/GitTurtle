//! Native named-profile picker. Definitions are app preferences; applying one is
//! a separate, captured Git operation with a visible configuration-scope review.
use crate::*;
use gitturtle_core::{ProfilePlan, WriteCommand};
use gpui_kit::{
    component::{
        WindowExt,
        checkbox::Checkbox,
        dialog::DialogButtonProps,
        menu::{DropdownMenu, PopupMenuItem},
    },
    prelude::FluentBuilder,
};
mod store;
use store::{Definition, Mutation, Signing, Store};

#[derive(Default)]
pub(super) struct State {
    store: Option<Store>,
    error: Option<String>,
    saving: bool,
    failed: Option<Mutation>,
    draft: Option<Entity<ProfileForm>>,
    pub prepared: Option<Definition>,
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
    pub(super) fn profile_save_pending(&self) -> bool {
        self.profiles.saving
    }
    pub(super) fn load_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let response = self.preferences_writer.submit_read(Store::load);
        cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Profile loading was interrupted")));
            let _ = this.update_in(cx, |this, _, cx| {
                match result {
                    Ok(store) => {
                        this.profiles.store = Some(store);
                        this.profiles.error = None;
                    }
                    Err(error) => this.profiles.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn render_profile_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let assignment = self
            .path
            .as_ref()
            .and_then(|path| self.profiles.store.as_ref()?.assigned(path));
        let mismatch = assignment.is_some_and(|saved| {
            self.profile
                .as_ref()
                .is_some_and(|effective| !saved.identity().matches(effective))
        });
        let title = assignment
            .map(|saved| {
                format!(
                    "{}{}",
                    saved.label,
                    if mismatch { " · differs" } else { "" }
                )
            })
            .unwrap_or_else(|| {
                self.profile
                    .as_ref()
                    .filter(|profile| !profile.name.is_empty())
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "Profiles".into())
            });
        let owner = cx.entity().downgrade();
        Button::new("profile")
            .secondary()
            .h(crate::appearance::ui_size(36.))
            .px_3()
            .max_w(px(230.))
            .label(title.clone())
            .icon(Icon::default().path("icons/user.svg").size(px(16.)))
            .dropdown_caret(true)
            .disabled(self.operation_busy.is_some() || self.profiles.saving)
            .accessibility_label(format!("Git profile: {title}. Choose or manage profiles"))
            .tooltip(
                self.profile
                    .as_ref()
                    .map(|p| {
                        format!(
                            "{} <{}> · {}",
                            p.name,
                            p.email,
                            if p.private_worktree {
                                "Private worktree configuration"
                            } else {
                                "Repository configuration shared by linked worktrees"
                            }
                        )
                    })
                    .unwrap_or_else(|| "Choose or manage named Git author profiles".into()),
            )
            .dropdown_menu(move |mut menu, _, cx| {
                let Some(view) = owner.upgrade() else {
                    return menu;
                };
                let this = view.read(cx);
                let path = this.path.clone();
                menu = menu
                    .max_h(px(440.))
                    .scrollable(true)
                    .label("Apply a saved Git profile…");
                if let Some(profile) = &this.profile {
                    menu = menu.label(format!("{} <{}>", profile.name, profile.email));
                }
                if let Some(store) = &this.profiles.store {
                    let assigned = path.as_ref().and_then(|path| store.assigned(path));
                    for profile in &store.definitions {
                        let profile = profile.clone();
                        let owner = owner.clone();
                        let path = path.clone();
                        let checked = assigned.is_some_and(|current| current.id == profile.id);
                        menu = menu.item(
                            PopupMenuItem::new(format!("{} · {}", profile.label, profile.email))
                                .checked(checked)
                                .disabled(
                                    this.repository.is_none() || this.page == AppPage::Projects,
                                )
                                .on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        if this.path == path {
                                            this.prepare_profile(profile.clone(), window, cx);
                                        }
                                    });
                                }),
                        );
                    }
                    if store.definitions.is_empty() {
                        menu = menu.label("Create Personal, Work, or another identity");
                    }
                } else {
                    menu = menu.label("Profiles are unavailable or still loading");
                }
                if let Some(error) = &this.profiles.error {
                    menu = menu.label(format!(
                        "Profiles: {}",
                        error.lines().next().unwrap_or(error)
                    ));
                }
                let manage_owner = owner.clone();
                menu = menu
                    .separator()
                    .item(
                        PopupMenuItem::new("Manage profiles…").on_click(move |_, window, cx| {
                            let _ =
                                manage_owner.update(cx, |this, cx| this.open_profiles(window, cx));
                        }),
                    );
                if this.profiles.error.is_some() {
                    let retry_owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new("Retry profile storage").on_click(
                        move |_, window, cx| {
                            let _ = retry_owner.update(cx, |this, cx| {
                                if let Some(mutation) = this.profiles.failed.clone() {
                                    this.save_profile_mutation(mutation, None, window, cx);
                                } else {
                                    this.load_profiles(window, cx);
                                }
                            });
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }
    pub(super) fn open_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() || self.profiles.saving {
            return;
        }
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let target = self.repository.as_ref().filter(|_| self.page != AppPage::Projects).map(|repo| format!("Target: {}\n{}", repo.name(), repo.path().display())).unwrap_or_else(|| "Choose a repository to apply a profile. Saved profiles are available in every project.".into());
        let store = self.profiles.store.clone();
        let error = self.profiles.error.clone();
        let can_apply = self.repository.is_some() && self.page != AppPage::Projects;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let p = palette(cx); let create_owner = owner.clone();
            let content = div().id("profile-manager-content").max_h(px(430.)).overflow_y_scroll().flex().flex_col().gap_3()
                .child(label("profiles-context", target.clone()).text_size(crate::appearance::ui_text(12.)))
                .child(label("profiles-purpose", "Profiles choose Git author identity. Provider sign-in, credential helpers and private keys stay separate. Editing or deleting a definition does not change Git configuration.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                .children(error.as_ref().map(|error| label("profiles-error", error.clone()).text_color(rgb(p.warning))))
                .child(button("new-git-profile", "New profile…", "plus", false).disabled(store.is_none()).on_click(move |_, window, cx| { let _ = create_owner.update(cx, |this, cx| { window.close_dialog(cx); this.edit_profile(None, window, cx); }); }))
                .children(store.iter().flat_map(|store| &store.definitions).enumerate().map(|(index, profile)| {
                    let apply_owner = owner.clone(); let edit_owner = owner.clone(); let delete_owner = owner.clone();
                    let apply_profile = profile.clone(); let edit_profile = profile.clone(); let delete_profile = profile.clone(); let path = path.clone();
                    div().p_3().rounded(px(8.)).border_1().border_color(rgb(p.border)).flex().flex_col().gap_2()
                        .child(div().text_size(crate::appearance::ui_text(13.)).font_weight(FontWeight::SEMIBOLD).child(profile.label.clone()))
                        .child(div().text_size(crate::appearance::ui_text(12.)).child(format!("{} <{}>", profile.name, profile.email)))
                        .child(div().flex().gap_2()
                            .child(button(("apply-profile", index), "Apply…", "", false).disabled(!can_apply).on_click(move |_, window, cx| { let _ = apply_owner.update(cx, |this, cx| { if this.path == path { window.close_dialog(cx); this.prepare_profile(apply_profile.clone(), window, cx); } }); }))
                            .child(button(("edit-profile", index), "Edit…", "", false).on_click(move |_, window, cx| { let _ = edit_owner.update(cx, |this, cx| { window.close_dialog(cx); this.edit_profile(Some(edit_profile.clone()), window, cx); }); }))
                            .child(button(("delete-profile", index), "Delete…", "", false).on_click(move |_, window, cx| { let _ = delete_owner.update(cx, |this, cx| { window.close_dialog(cx); this.delete_profile(delete_profile.clone(), window, cx); }); })))
                }));
            dialog.title("Git profiles").width(px(600.)).child(content).button_props(DialogButtonProps::default().ok_text("Done"))
        });
        window.refresh();
    }
    fn prepare_profile(
        &mut self,
        definition: Definition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.profiles.saving || self.page == AppPage::Projects {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let path = repo.path().to_owned();
        let identity = definition.identity();
        self.operation_busy = Some("Reviewing Git profile…");
        let response = self
            .operations
            .submit_read(move || repo.profile_plan(identity));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Profile preparation was interrupted")));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.operation_busy == Some("Reviewing Git profile…") { this.operation_busy = None; }
                if this.path.as_ref() != Some(&path) || this.page == AppPage::Projects { return; }
                match result {
                    Ok(plan) => {
                        let scope = if plan.private_worktree { "Only this worktree's existing private Git configuration will change." } else { "Repository configuration is shared with every linked worktree. Other worktrees inheriting these keys will also use this identity; private worktree overrides remain effective." };
                        let signing = definition.signing.as_ref().map(|s| format!("Existing signing reference: {}\nFormat: {} · Require signed commits: {} · Require signed tags: {} · Signed annotated tags: {}.\nExisting signing requirements are never disabled.", s.key.as_deref().unwrap_or("keep configured key"), s.format.as_deref().unwrap_or("keep configured format"), s.commits, s.tags, s.annotated_tags)).unwrap_or_else(|| "All existing signing settings and requirements remain inherited.".into());
                        let explanation = format!("Apply '{}' as {} <{}>\n\nWorktree: {}\nConfiguration: {}\n\n{scope}\n\n{signing}\n\nThis explicit action affects subsequent Git commits and annotated tags. Global configuration, hooks and credentials are preserved. Reopening a repository never reapplies a profile.", definition.label, definition.name, definition.email, plan.worktree.display(), plan.config_path.display());
                        this.profiles.prepared = Some(definition.clone());
                        this.confirm_git_write(format!("Apply {} profile", definition.label), explanation, "Apply profile", WriteCommand::ApplyProfile(Arc::new(plan)), window, cx);
                    }
                    Err(error) => this.operation_error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }
    pub(super) fn submitted_profile(&self, plan: &ProfilePlan) -> Option<Definition> {
        self.profiles
            .prepared
            .as_ref()
            .filter(|profile| profile.identity() == plan.identity)
            .cloned()
    }
    pub(super) fn finish_profile_write(
        &mut self,
        path: &std::path::Path,
        profile: Definition,
        succeeded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if succeeded {
            self.save_profile_mutation(
                Mutation::Assign {
                    worktree: path.to_owned(),
                    profile,
                },
                None,
                window,
                cx,
            );
        }
    }
    fn save_profile_mutation(
        &mut self,
        mutation: Mutation,
        form: Option<WeakEntity<ProfileForm>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.profiles.saving {
            return;
        }
        self.profiles.saving = true;
        self.profiles.error = None;
        let write_mutation = mutation.clone();
        let response = self
            .preferences_writer
            .submit(move || Store::update(&write_mutation));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Profile save ended without a result; inspect storage before retrying"
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.profiles.saving = false;
                match result {
                    Ok(store) => {
                        this.profiles.store = Some(store);
                        this.profiles.failed = None;
                        this.profiles.error = None;
                        if let Some(form) = &form {
                            let _ = form.update(cx, |form, cx| {
                                form.pending = false;
                                cx.notify();
                            });
                            let visible = form.upgrade().is_some_and(|form| form.read(cx).visible);
                            this.profiles.draft = None;
                            if visible {
                                window.close_dialog(cx);
                                this.open_profiles(window, cx);
                            }
                        } else {
                            this.operation_notice = Some("Profile preferences saved".into());
                        }
                    }
                    Err(error) => {
                        let error = format!("{error:#}");
                        this.profiles.error = Some(error.clone());
                        this.profiles.failed = if form.is_some() { None } else { Some(mutation) };
                        this.operation_error = Some(format!("Profile persistence: {error}"));
                        if let Some(form) = &form {
                            let _ = form.update(cx, |form, cx| {
                                form.pending = false;
                                form.error = Some(error);
                                cx.notify();
                            });
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn edit_profile(
        &mut self,
        previous: Option<Definition>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.profiles.saving {
            return;
        }
        let owner = cx.entity().downgrade();
        let effective = self.profile.clone().unwrap_or_default();
        let form = self
            .profiles
            .draft
            .as_ref()
            .filter(|form| form.read(cx).previous == previous)
            .cloned()
            .unwrap_or_else(|| {
                cx.new(|cx| ProfileForm::new(owner, previous.clone(), effective, window, cx))
            });
        form.update(cx, |form, _| form.visible = true);
        self.profiles.draft = Some(form.clone());
        let focus = form.read(cx).label.read(cx).focus_handle(cx);
        let focus_form = form.downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let submit = form.clone();
            let cancel = form.clone();
            dialog
                .title(if previous.is_some() {
                    "Edit Git profile"
                } else {
                    "Create Git profile"
                })
                .width(px(550.))
                .child(form.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Save profile")
                        .cancel_text("Cancel")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.submit(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |form, _| form.visible = false);
                    true
                })
        });
        window.refresh();
        window.on_next_frame(move |window, cx| {
            if focus_form
                .upgrade()
                .is_some_and(|form| form.read(cx).visible)
            {
                focus.focus(window, cx);
            }
        });
    }
    fn delete_profile(
        &mut self,
        definition: Definition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone(); let definition = definition.clone(); let explanation = format!("Delete the saved '{}' profile and its app assignments? Repository Git configuration remains as it is. Repositories, keys and drafts are preserved.", definition.label);
            dialog.title("Delete saved profile").child(label("delete-profile-consequences", explanation))
                .button_props(DialogButtonProps::default().ok_text("Delete profile").cancel_text("Cancel").show_cancel(true))
                .on_ok(move |_, window, cx| { let _ = owner.update(cx, |this, cx| { if this.operation_busy.is_none() { this.save_profile_mutation(Mutation::Delete(definition.clone()), None, window, cx); } }); true })
        });
        window.refresh();
    }
}

struct ProfileForm {
    owner: WeakEntity<GitTurtle>,
    previous: Option<Definition>,
    id: String,
    label: Entity<InputState>,
    name: Entity<InputState>,
    email: Entity<InputState>,
    signing: Signing,
    current_signing: Signing,
    capture_signing: bool,
    pending: bool,
    visible: bool,
    error: Option<String>,
}
impl ProfileForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        previous: Option<Definition>,
        effective: gitturtle_core::GitProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let id = previous.as_ref().map(|p| p.id.clone()).unwrap_or_else(|| {
            format!(
                "profile-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )
        });
        let text = |value: String,
                    placeholder: &'static str,
                    window: &mut Window,
                    cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(placeholder)
            })
        };
        Self {
            owner,
            id,
            label: text(
                previous
                    .as_ref()
                    .map(|p| p.label.clone())
                    .unwrap_or_default(),
                "Personal or Work",
                window,
                cx,
            ),
            name: text(
                previous
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| effective.name.clone()),
                "Git author name",
                window,
                cx,
            ),
            email: text(
                previous
                    .as_ref()
                    .map(|p| p.email.clone())
                    .unwrap_or_else(|| effective.email.clone()),
                "author@example.com",
                window,
                cx,
            ),
            signing: previous
                .as_ref()
                .and_then(|p| p.signing.clone())
                .unwrap_or_else(|| Signing::capture(&effective)),
            current_signing: Signing::capture(&effective),
            capture_signing: previous.as_ref().is_some_and(|p| p.signing.is_some()),
            previous,
            pending: false,
            visible: true,
            error: None,
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let definition = Definition {
            id: self.id.clone(),
            label: self.label.read(cx).value().trim().into(),
            name: self.name.read(cx).value().trim().into(),
            email: self.email.read(cx).value().trim().into(),
            signing: self.capture_signing.then(|| self.signing.clone()),
        };
        if let Err(error) = definition.validate() {
            self.error = Some(format!("{error:#}"));
            cx.notify();
            return;
        }
        let form = cx.entity().downgrade();
        let mutation = Mutation::Save {
            previous: self.previous.clone(),
            definition,
        };
        let accepted = self
            .owner
            .update(cx, |this, cx| {
                if this.operation_busy.is_some() || this.profiles.saving {
                    return false;
                }
                this.save_profile_mutation(mutation, Some(form), window, cx);
                true
            })
            .unwrap_or(false);
        self.pending = accepted;
        if !accepted {
            self.error = Some("Wait for the current operation before editing profiles.".into());
        }
        cx.notify();
    }
}
impl Render for ProfileForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div().id("profile-form").max_h(px(420.)).overflow_y_scroll().flex().flex_col().gap_3()
            .child(label("profile-label", "Profile name").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.label).aria_label("Profile name").disabled(self.pending))
            .child(label("profile-author-label", "Git author name").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.name).aria_label("Profile Git author name").disabled(self.pending))
            .child(label("profile-email-label", "Git author email").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.email).aria_label("Profile Git author email").disabled(self.pending))
            .child(Checkbox::new("profile-capture-signing").label("Include the existing signing configuration").checked(self.capture_signing).disabled(self.pending).on_click(cx.listener(|this, checked: &bool, _, cx| { this.capture_signing = *checked; cx.notify(); })))
            .child(button("profile-use-current-signer", "Use current repository signing settings", "", false).disabled(self.pending).on_click(cx.listener(|this, _, _, cx| { this.signing = this.current_signing.clone(); this.capture_signing = true; cx.notify(); })))
            .when(self.capture_signing, |element| element.child(label("profile-signing-reference", format!("Key reference: {}\nFormat: {} · Commit signing: {} · Tag signing: {} · Annotated signing: {}", self.signing.key.as_deref().unwrap_or("Keep repository key"), self.signing.format.as_deref().unwrap_or("Keep repository format"), self.signing.commits, self.signing.tags, self.signing.annotated_tags)).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))))
            .child(label("profile-save-explanation", "Save creates or edits this reusable profile. Apply it separately to a reviewed repository. Existing signing requirements remain enabled; unavailable keys produce Git errors without an unsigned fallback.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
            .when(self.pending, |element| element.child(label("profile-save-pending", "Saving profile…").text_color(rgb(p.accent))))
            .children(self.error.as_ref().map(|error| label("profile-form-error", error.clone()).text_color(rgb(p.warning))))
    }
}
