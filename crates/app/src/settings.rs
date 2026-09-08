use crate::*;
use appearance::{Density, ThemeChoice};
use columns::{ColumnId, ColumnSettings};
use gitturtle_core::WriteCommand;
use gpui_kit::component::{checkbox::Checkbox, switch::Switch};
use gpui_kit::prelude::FluentBuilder;

impl GitTurtle {
    pub(super) fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = AppPage::Settings;
        self.column_menu = false;
        self.git_actions_open = false;
        self.column_drag = None;
        self.image_drag = None;
        self.settings_branch.update(cx, |input, cx| {
            input.set_value(self.settings.default_branch.clone(), window, cx)
        });
        self.fill_identity_inputs(window, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn fill_identity_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profile = self.profile.clone().unwrap_or_default();
        self.identity_name
            .update(cx, |input, cx| input.set_value(profile.name, window, cx));
        self.identity_email
            .update(cx, |input, cx| input.set_value(profile.email, window, cx));
    }

    /// This method never replaces current UI settings with an older disk reply.
    /// The dedicated executor serializes these saves with recent-project writes.
    pub(super) fn save_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.normalize();
        if let Err(error) = self.settings.validate() {
            self.operation_error = Some(format!("Could not save settings: {error:#}"));
            cx.notify();
            return;
        }
        let settings = self.settings.clone();
        let submitted = settings.clone();
        let response = self
            .preferences_writer
            .submit(move || Preferences::save_settings(&settings));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "The settings writer stopped before reporting a result"
                ))
            });
            let _ = this.update_in(cx, |this, _, cx| {
                if this.settings != submitted {
                    return;
                }
                match result {
                    Ok(_) => {
                        if this
                            .operation_error
                            .as_ref()
                            .is_some_and(|error| error.starts_with("Could not save settings:"))
                        {
                            this.operation_error = None;
                        }
                        this.status = "Settings saved".into();
                    }
                    Err(error) => {
                        this.operation_error = Some(format!("Could not save settings: {error:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn choose_theme(&mut self, theme: ThemeChoice, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.theme == theme {
            return;
        }
        self.settings.theme = theme;
        theme.apply(Some(window), cx);
        // Existing decorations contain concrete colors. Preserve prepared
        // content while recreating only the editor that is actually visible.
        self.patch_editor = None;
        self.patch_view = None;
        self.before_editor = None;
        self.after_editor = None;
        if self.page == AppPage::Repository && self.mode != WorkspaceMode::History {
            self.ensure_editor(window, cx);
        }
        self.save_preferences(window, cx);
    }

    fn save_default_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut edited = self.settings.clone();
        edited.default_branch = self.settings_branch.read(cx).value().to_string();
        if let Err(error) = edited.validate() {
            self.operation_error = Some(format!("Default branch: {error:#}"));
            cx.notify();
            return;
        }
        if self
            .operation_error
            .as_ref()
            .is_some_and(|error| error.starts_with("Default branch:"))
        {
            self.operation_error = None;
            cx.notify();
        }
        if edited == self.settings {
            return;
        }
        self.settings = edited;
        self.hub.update(cx, |hub, cx| {
            hub.set_default_branch(self.settings.default_branch.clone(), cx)
        });
        self.save_preferences(window, cx);
    }

    fn save_identity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.repository.is_none() || self.operation_busy.is_some() {
            return;
        }
        let name = self.identity_name.read(cx).value().trim().to_owned();
        let email = self.identity_email.read(cx).value().trim().to_owned();
        if name.is_empty() || email.is_empty() {
            self.operation_error = Some("Enter both your Git name and email address.".into());
            cx.notify();
            return;
        }
        if self
            .profile
            .as_ref()
            .is_some_and(|profile| profile.name == name && profile.email == email)
        {
            return;
        }
        self.write(
            WriteCommand::SetIdentity { name, email },
            "Saving repository identity…",
            window,
            cx,
        );
    }

    pub(super) fn render_columns_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(relative(1.5))
                    .text_color(rgb(p.muted))
                    .child("Choose what appears in history. Drag a column divider to resize; scroll horizontally when your columns need more room."),
            )
            .children(ColumnId::ALL.into_iter().enumerate().map(|(index, id)| {
                let setting = self.settings.columns.get(id);
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Checkbox::new(("history-column", index))
                            .label(id.label())
                            .checked(setting.visible)
                            .disabled(id == ColumnId::Subject)
                            .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                                if this.settings.columns.get(id).visible != *checked {
                                    this.settings.columns.set_visible(id, *checked);
                                    this.save_preferences(window, cx);
                                }
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(p.muted))
                            .child(if id == ColumnId::Subject {
                                "Always shown".to_owned()
                            } else {
                                format!("{:.0} px", setting.width)
                            }),
                    )
            }))
            .child(
                div().pt_2().child(
                    button("reset-history-columns", "Reset column layout", "refresh", false)
                        .disabled(self.settings.columns == ColumnSettings::default())
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.settings.columns != ColumnSettings::default() {
                                this.settings.columns.reset();
                                this.history_horizontal.set_offset(point(px(0.), px(0.)));
                                this.save_preferences(window, cx);
                            }
                        })),
                ),
            )
            .into_any_element()
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        let repository_name = self.repository.as_ref().map(|repository| repository.name());
        let appearance =
            div()
                .flex()
                .flex_col()
                .gap_5()
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .children(ThemeChoice::ALL.into_iter().enumerate().map(
                            |(index, choice)| {
                                let preview = choice.palette();
                                let selected = self.settings.theme == choice;
                                Button::new(("settings-theme", index))
                                    .ghost()
                                    .accessibility_label(format!("{} theme", choice.label()))
                                    .selected(selected)
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(154.))
                                    .p_0()
                                    .border_1()
                                    .border_color(rgb(if selected { p.accent } else { p.border }))
                                    .rounded(px(10.))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .size_full()
                                            .flex()
                                            .flex_col()
                                            .bg(rgb(preview.canvas))
                                            .child(
                                                div()
                                                    .h(px(26.))
                                                    .flex_shrink_0()
                                                    .px_3()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .bg(rgb(preview.panel))
                                                    .children(
                                                        [
                                                            preview.accent,
                                                            preview.muted,
                                                            preview.border,
                                                        ]
                                                        .map(|color| {
                                                            div()
                                                                .size(px(5.))
                                                                .rounded_full()
                                                                .bg(rgb(color))
                                                        }),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_h_0()
                                                    .p_3()
                                                    .flex()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .w(px(25.))
                                                            .rounded(px(3.))
                                                            .bg(rgb(preview.panel)),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .flex()
                                                            .flex_col()
                                                            .gap_2()
                                                            .children((0..4).map(|row| {
                                                                div()
                                                                    .h(px(8.))
                                                                    .w(if row == 2 {
                                                                        relative(0.68)
                                                                    } else {
                                                                        relative(1.)
                                                                    })
                                                                    .rounded(px(2.))
                                                                    .bg(rgb(if row == 1 {
                                                                        preview.selected
                                                                    } else {
                                                                        preview.hover
                                                                    }))
                                                                    .child(
                                                                        div()
                                                                            .w(px(4.))
                                                                            .h_full()
                                                                            .bg(rgb(if row == 1 {
                                                                                preview.accent
                                                                            } else {
                                                                                preview.border
                                                                            })),
                                                                    )
                                                            })),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .h(px(34.))
                                                    .px_3()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .bg(rgb(preview.panel))
                                                    .text_size(px(12.))
                                                    .text_color(rgb(preview.text))
                                                    .child(choice.label())
                                                    .when(selected, |element| {
                                                        element.child(
                                                            div()
                                                                .text_size(px(10.))
                                                                .text_color(rgb(preview.accent))
                                                                .child("ACTIVE"),
                                                        )
                                                    }),
                                            ),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.choose_theme(choice, window, cx)
                                    }))
                            },
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(setting_description(
                            "List density",
                            "Make room for more commits and files.",
                            cx,
                        ))
                        .child(
                            div().flex().gap_1().children(
                                Density::ALL
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, density)| {
                                        Button::new(("settings-density", index))
                                            .small()
                                            .ghost()
                                            .label(density.label())
                                            .selected(self.settings.density == density)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                if this.settings.density != density {
                                                    this.settings.density = density;
                                                    this.save_preferences(window, cx);
                                                }
                                            }))
                                    }),
                            ),
                        ),
                )
                .into_any_element();

        let startup = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(setting_description(
                        "Reopen the last project",
                        "Continue where you left off when GitTurtle starts.",
                        cx,
                    ))
                    .child(
                        Switch::new("settings-reopen-last")
                            .accessibility_label("Reopen the last project")
                            .checked(self.settings.reopen_last)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                if this.settings.reopen_last != *checked {
                                    this.settings.reopen_last = *checked;
                                    this.save_preferences(window, cx);
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(setting_description(
                        "Default branch",
                        "Used when creating a new repository.",
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&self.settings_branch)),
                            )
                            .child(Button::new("save-default-branch").label("Save").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.save_default_branch(window, cx)
                                }),
                            )),
                    ),
            )
            .into_any_element();

        let identity = if let Some(repository_name) = repository_name {
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .rounded(px(8.))
                        .bg(rgb(p.canvas))
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child(repository_name))
                        .child(
                            div().text_size(px(11.)).text_color(rgb(p.muted)).child(
                                self.path.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
                            ),
                        )
                        .child(div().text_size(px(11.)).text_color(rgb(p.muted)).child(
                            match &self.profile {
                                Some(profile) if !profile.name.is_empty() && !profile.email.is_empty() => {
                                    format!("Current identity: {} <{}>", profile.name, profile.email)
                                }
                                Some(_) => "A commit identity has not been configured yet.".into(),
                                None => "Git identity has not been loaded yet.".into(),
                            },
                        )),
                )
                .child(settings_field("Name", &self.identity_name, busy, cx))
                .child(settings_field("Email", &self.identity_email, busy, cx))
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(relative(1.5))
                        .text_color(rgb(p.muted))
                        .child(match self.profile.as_ref() {
                            Some(profile) if profile.signing => "Commit signing is enabled in Git. GitTurtle uses your configured signer.",
                            Some(_) => "Commit signing is off in Git. Your existing Git signing configuration is preserved.",
                            None => "Signing follows your existing Git configuration.",
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("save-repository-identity")
                                .primary()
                                .label(if busy { "Working…" } else { "Save repository identity" })
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, window, cx| this.save_identity(window, cx))),
                        )
                        .child(
                            button("use-current-identity", "Use current identity", "refresh", false)
                                .disabled(busy || self.profile.is_none())
                                .on_click(cx.listener(|this, _, window, cx| this.fill_identity_inputs(window, cx))),
                        ),
                )
                .child(div().text_size(px(11.)).line_height(relative(1.5)).text_color(rgb(p.muted)).child("Name and email are saved to this repository’s Git configuration."))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap_3()
                .child(icon("commit", 28., p.muted))
                .child(
                    div()
                        .text_size(px(13.))
                        .child("Open a project to set its Git identity."),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(relative(1.5))
                        .text_color(rgb(p.muted))
                        .child("Your Git name, email, and signing preference will appear here."),
                )
                .child(
                    button(
                        "settings-open-projects",
                        "Choose a project",
                        "folder",
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.show_projects(cx))),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(p.canvas))
            .text_color(rgb(p.text))
            .child(
                div()
                    .h(px(54.))
                    .flex_shrink_0()
                    .px_6()
                    .flex()
                    .items_center()
                    .gap_3()
                    .bg(rgb(p.panel))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .child(
                        button("settings-back", "Back", "chevron", false).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.page = if this.repository.is_some() {
                                    AppPage::Repository
                                } else {
                                    AppPage::Projects
                                };
                                if this.page == AppPage::Repository
                                    && this.mode != WorkspaceMode::History
                                {
                                    this.ensure_editor(window, cx);
                                }
                                window.focus(&this.focus, cx);
                                cx.notify();
                            },
                        )),
                    )
                    .child(div().h(px(18.)).w(px(1.)).bg(rgb(p.border)))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Settings"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(p.muted))
                            .child("Make GitTurtle yours"),
                    ),
            )
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .child(
                        div()
                            .max_w(px(1160.))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .when_some(self.operation_error.as_ref(), |element, error| {
                                element.child(
                                    div()
                                        .p_3()
                                        .rounded(px(8.))
                                        .bg(rgb(p.removed_background))
                                        .text_color(rgb(p.removed))
                                        .text_size(px(12.))
                                        .child(error.clone()),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap_5()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_5()
                                            .child(settings_section(
                                                "Appearance",
                                                "A familiar workspace, in your colors.",
                                                appearance,
                                                cx,
                                            ))
                                            .child(settings_section(
                                                "History columns",
                                                "Keep the information you care about in view.",
                                                self.render_columns_controls(cx),
                                                cx,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .w(px(360.))
                                            .flex_shrink_0()
                                            .flex()
                                            .flex_col()
                                            .gap_5()
                                            .child(settings_section(
                                                "Projects",
                                                "Choose how new sessions and projects start.",
                                                startup,
                                                cx,
                                            ))
                                            .child(settings_section(
                                                "Git identity",
                                                "The name behind your commits.",
                                                identity,
                                                cx,
                                            )),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn setting_description(title: &'static str, description: &'static str, cx: &App) -> AnyElement {
    let p = palette(cx);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.))
                .line_height(relative(1.5))
                .text_color(rgb(p.muted))
                .child(description),
        )
        .into_any_element()
}

fn settings_field(
    label: &'static str,
    input: &Entity<InputState>,
    disabled: bool,
    cx: &App,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .text_size(px(12.))
        .text_color(rgb(palette(cx).text))
        .child(label)
        .child(Input::new(input).disabled(disabled))
        .into_any_element()
}

fn settings_section(
    title: &'static str,
    description: &'static str,
    content: AnyElement,
    cx: &App,
) -> AnyElement {
    let p = palette(cx);
    div()
        .p_5()
        .rounded(px(14.))
        .border_1()
        .border_color(rgb(p.border))
        .bg(rgb(p.panel))
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(p.muted))
                        .child(description),
                ),
        )
        .child(content)
        .into_any_element()
}
