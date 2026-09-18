use crate::*;
use appearance::{Density, ThemeChoice};
use columns::{ColumnId, ColumnSettings};
use gitturtle_core::WriteCommand;
use gpui_kit::component::{checkbox::Checkbox, switch::Switch};
use gpui_kit::prelude::FluentBuilder;
use std::path::Path;

/// Fixed list rows are snapped by GPUI before layout. Preserve row positions
/// with the same old/new metrics, including each window's device scale.
#[derive(Clone, Copy)]
pub(super) struct ListScales {
    pub history: f32,
    pub files: f32,
    pub navigation: f32,
    pub lineage: f32,
}
impl ListScales {
    pub(super) fn new(
        old: f32,
        new: f32,
        density: appearance::Density,
        window: &Window,
    ) -> Option<Self> {
        if old == new {
            return None;
        }
        let ratio = |base| {
            f32::from(window.pixel_snap(px(base * new)))
                / f32::from(window.pixel_snap(px(base * old)))
        };
        Some(Self {
            history: ratio(density.history_row_height_at_scale(1.)),
            files: ratio(density.file_row_height_at_scale(1.)),
            navigation: ratio(30.),
            lineage: ratio(68.),
        })
    }
}

pub(super) fn rescale_list_scroll(scroll: &UniformListScrollHandle, ratio: f32) {
    let mut state = scroll.0.borrow_mut();
    let offset = state.base_handle.offset();
    state
        .base_handle
        .set_offset(point(offset.x, offset.y * ratio));
    // Geometry now belongs to the old font. In particular, Working Changes
    // must not mistake this intentional viewport change for a density resize
    // and reveal an offscreen selected row over the user's manual position.
    state.last_item_size = None;
}

/// Baselines distinguish a saved value moving externally from an unfinished edit.
pub(super) struct DraftState {
    branch: String,
    identity: Option<(PathBuf, (String, String))>,
}

impl DraftState {
    pub(super) fn new(branch: String) -> Self {
        Self {
            branch,
            identity: None,
        }
    }
    fn branch_update(&mut self, saved: &str, edited: &str) -> Option<String> {
        if edited != self.branch && edited != saved {
            return None;
        }
        self.branch = saved.into();
        (edited != saved).then(|| saved.into())
    }
    fn identity_update(
        &mut self,
        path: &Path,
        effective: &(String, String),
        edited: &(String, String),
    ) -> Option<(String, String)> {
        if let Some((scope, baseline)) = &self.identity
            && scope == path
            && edited != baseline
            && edited != effective
        {
            return None;
        }
        self.identity = Some((path.to_owned(), effective.clone()));
        (edited != effective).then(|| effective.clone())
    }
    fn reset_identity(&mut self, path: Option<&Path>, effective: (String, String)) {
        self.identity = path.map(|path| (path.to_owned(), effective));
    }
}

impl GitTurtle {
    pub(super) fn subscribe_settings_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(any(target_os = "linux", test))]
        {
            let mut applied_scale = appearance::desktop_text_scale();
            self.subscriptions
                .push(cx.observe_global_in::<appearance::DesktopTextScale>(
                    window,
                    move |this, window, cx| {
                        let scale = cx.global::<appearance::DesktopTextScale>().0;
                        if scale != applied_scale {
                            let ratio = scale / applied_scale;
                            let interface = f32::from(this.settings.interface_text_size)
                                / f32::from(appearance::DEFAULT_INTERFACE_TEXT_SIZE);
                            let lists = ListScales::new(
                                interface * applied_scale,
                                interface * scale,
                                this.settings.density,
                                window,
                            );
                            applied_scale = scale;
                            this.rescale_text_viewports(lists, ratio, cx);
                        }
                    },
                ));
        }
        // Status replies can change the effective identity while Settings is
        // visible. Reconcile after parent notifications, outside rendering.
        self.subscriptions
            .push(cx.observe_in(&cx.entity(), window, |this, _, window, cx| {
                this.sync_settings_drafts(window, cx)
            }));
        self.subscriptions.push(cx.subscribe_in(
            &self.settings_branch,
            window,
            |this, _, event, window, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => this.save_default_branch(window, cx),
                _ => {}
            },
        ));
        self.subscriptions.push(cx.subscribe_in(
            &self.settings_editor,
            window,
            |_, _, _: &InputEvent, _, cx| cx.notify(),
        ));
        for input in [&self.identity_name, &self.identity_email] {
            self.subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this, _, event, window, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } => this.save_identity(window, cx),
                    _ => {}
                },
            ));
        }
    }

    pub(super) fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings {
            self.page_origin = self.page;
        }
        self.capture_page_return_focus(window, cx);
        self.cancel_branch_action();
        self.cancel_interactive_rebase_action();
        self.cancel_discard_action();
        self.page = AppPage::Settings;
        self.column_menu = false;
        self.column_drag = None;
        self.image_drag = None;
        self.end_image_drag();
        self.sync_settings_drafts(window, cx);
        window.focus(&self.app_focus, cx);
        self.repaint_page(window, cx);
    }

    pub(super) fn capture_page_return_focus(&mut self, window: &Window, cx: &App) {
        if self.page == AppPage::Repository {
            pdf_view::pause(self.content.as_deref());
            model_view::pause(self.content.as_deref());
            markdown_view::pause(self.content.as_deref());
            self.page_return_focus = window.focused(cx);
        }
    }

    fn fill_identity_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profile = self.profile.clone().unwrap_or_default();
        self.settings_drafts.reset_identity(
            self.repository.as_ref().map(|repository| repository.path()),
            (profile.name.clone(), profile.email.clone()),
        );
        self.identity_name
            .update(cx, |input, cx| input.set_value(profile.name, window, cx));
        self.identity_email
            .update(cx, |input, cx| input.set_value(profile.email, window, cx));
    }

    fn sync_settings_drafts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings {
            return;
        }
        if let Some(value) = self.settings_drafts.branch_update(
            &self.settings.default_branch,
            self.settings_branch.read(cx).value().as_ref(),
        ) {
            self.settings_branch
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
        let (Some(repository), Some(profile)) = (&self.repository, &self.profile) else {
            return;
        };
        let effective = (profile.name.clone(), profile.email.clone());
        let edited = (
            self.identity_name.read(cx).value().to_string(),
            self.identity_email.read(cx).value().to_string(),
        );
        if let Some((name, email)) =
            self.settings_drafts
                .identity_update(repository.path(), &effective, &edited)
        {
            self.identity_name
                .update(cx, |input, cx| input.set_value(name, window, cx));
            self.identity_email
                .update(cx, |input, cx| input.set_value(email, window, cx));
        }
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

    fn choose_text_size(
        &mut self,
        code: bool,
        size: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let old_code = self.settings.code_text_size;
        let old_interface = self.settings.interface_text_size;
        if code {
            self.settings.code_text_size = size;
        } else {
            self.settings.interface_text_size = size;
        }
        self.settings.normalize();
        if (old_code, old_interface)
            == (
                self.settings.code_text_size,
                self.settings.interface_text_size,
            )
        {
            return;
        }
        let scale =
            appearance::desktop_text_scale() / f32::from(appearance::DEFAULT_INTERFACE_TEXT_SIZE);
        self.rescale_text_viewports(
            ListScales::new(
                f32::from(old_interface) * scale,
                f32::from(self.settings.interface_text_size) * scale,
                self.settings.density,
                window,
            ),
            f32::from(self.settings.code_text_size) / f32::from(old_code),
            cx,
        );
        appearance::apply_text_sizes(
            self.settings.interface_text_size,
            self.settings.code_text_size,
            window,
            cx,
        );
        self.save_preferences(window, cx);
    }

    /// Resize retained geometry without replacing editors, saving preferences,
    /// or issuing repository work. Desktop changes apply this in every window.
    pub(super) fn rescale_text_viewports(
        &mut self,
        lists: Option<ListScales>,
        code_ratio: f32,
        cx: &mut Context<Self>,
    ) {
        if code_ratio != 1.0 {
            for editor in [&self.patch_editor, &self.before_editor, &self.after_editor]
                .into_iter()
                .flatten()
            {
                text::rescale_editor(editor, code_ratio, cx);
            }
            if let Some(view) = &self.split_view {
                view.update(cx, |view, cx| view.rescale_code(code_ratio, cx));
            }
            self.file_history.rescale_code(code_ratio, cx);
            self.revision_inspection.rescale_code(code_ratio, cx);
            if let Some(view) = &self.conflict_view {
                view.update(cx, |view, cx| view.rescale_code(code_ratio, cx));
            }
        }
        if let Some(scales) = lists {
            for (scroll, ratio) in [
                (&self.history_scroll, scales.history),
                (&self.file_scroll, scales.files),
                (&self.working_scroll, scales.files),
                (&self.nav_scroll, scales.navigation),
            ] {
                rescale_list_scroll(scroll, ratio);
            }
            self.blame.rescale_lists(scales);
            self.file_history.rescale_lists(scales);
            self.revision_inspection.rescale_lists(scales);
            // Treat the next layout as a new baseline: revealing the selected
            // row here would discard an intentionally scrolled viewport.
            self.history_list_layout = None;
            self.file_list_layout = None;
        }
        self.repository_tabs
            .rescale_text_viewports(lists, code_ratio, cx);
        cx.notify();
    }

    fn render_text_size_setting(&self, code: bool, cx: &mut Context<Self>) -> AnyElement {
        let (label, explanation, value, bounds, default) = if code {
            (
                "Code text size",
                "Diffs, source, gutters, and conflict content.",
                self.settings.code_text_size,
                appearance::CODE_TEXT_RANGE,
                appearance::DEFAULT_CODE_TEXT_SIZE,
            )
        } else {
            (
                "Interface text size",
                "Lists, controls, dialogs, and navigation.",
                self.settings.interface_text_size,
                appearance::INTERFACE_TEXT_RANGE,
                appearance::DEFAULT_INTERFACE_TEXT_SIZE,
            )
        };
        let p = palette(cx);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .child(div().flex_1().min_w(px(180.)).child(setting_description(
                label,
                explanation,
                cx,
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new(("text-size-decrease", usize::from(code)))
                            .small()
                            .ghost()
                            .label("−")
                            .accessibility_label(format!("Decrease {}", label.to_lowercase()))
                            .disabled(value <= *bounds.start())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_text_size(code, value.saturating_sub(1), window, cx)
                            })),
                    )
                    .child(
                        div()
                            .id(("text-size-value", usize::from(code)))
                            .role(Role::Label)
                            .aria_label(format!("{label}, {value} points"))
                            .min_w(appearance::ui_size(42.))
                            .text_center()
                            .text_size(appearance::ui_text(12.))
                            .child(format!("{value} pt")),
                    )
                    .child(
                        Button::new(("text-size-increase", usize::from(code)))
                            .small()
                            .ghost()
                            .label("+")
                            .accessibility_label(format!("Increase {}", label.to_lowercase()))
                            .disabled(value >= *bounds.end())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_text_size(code, value.saturating_add(1), window, cx)
                            })),
                    )
                    .child(
                        Button::new(("text-size-reset", usize::from(code)))
                            .small()
                            .ghost()
                            .label("Reset")
                            .accessibility_label(format!(
                                "Reset {} to {default} points",
                                label.to_lowercase()
                            ))
                            .disabled(value == default)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_text_size(code, default, window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .p_3()
                    .rounded(px(6.))
                    .bg(rgb(p.canvas))
                    .text_color(rgb(p.text))
                    .when(code, |el| {
                        el.font_family(
                            gpui_kit::component::Theme::global(cx)
                                .mono_font_family
                                .clone(),
                        )
                        .text_size(appearance::code_text())
                    })
                    .when(!code, |el| el.text_size(appearance::ui_text(12.)))
                    .child(if code {
                        "let changes = repository.status();  // 🐢"
                    } else {
                        "Review changes · Keep your place · Find files"
                    }),
            )
            .into_any_element()
    }

    /// Theme switches restyle retained editors in place: no worker job, no
    /// content re-preparation. `gitturtle.theme_apply_frame_ms` measures this
    /// handler through the next frame callback under `GITTURTLE_TRACE`.
    fn choose_theme(&mut self, theme: ThemeChoice, window: &mut Window, cx: &mut Context<Self>) {
        let started = trace_enabled().then(std::time::Instant::now);
        let theme = appearance::custom::ThemeSelection::BuiltIn(theme);
        if self.settings.theme == theme && !self.settings.follow_system {
            return;
        }
        self.settings.theme = theme;
        self.settings.follow_system = false;
        self.apply_appearance(window, cx);
        self.save_preferences(window, cx);
        if let Some(start) = started {
            Self::trace_next_frame("theme_apply_frame_ms", start, window);
        }
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
                    .text_size(crate::appearance::ui_text(12.))
                    .line_height(relative(1.5))
                    .text_color(rgb(p.muted))
                    .child("Choose what appears in history. Drag a column divider to resize; scroll horizontally when your columns need more room."),
            )
            .child(div().flex().items_center().flex_wrap().gap_2()
                .child(setting_description("Graph lane spacing", "Distinct lanes keep this spacing; wide histories scroll horizontally.", cx))
                .child(button("graph-spacing-less", "−", "", false)
                    .accessibility_label("Decrease graph lane spacing").disabled(self.settings.graph_spacing <= 12)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.settings.graph_spacing = this.settings.graph_spacing.saturating_sub(2).max(12);
                        this.save_preferences(window, cx);
                    })))
                .child(div().min_w(px(38.)).text_center().child(format!("{} px", self.settings.graph_spacing)))
                .child(button("graph-spacing-more", "+", "", false)
                    .accessibility_label("Increase graph lane spacing").disabled(self.settings.graph_spacing >= 32)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.settings.graph_spacing = (this.settings.graph_spacing + 2).min(32);
                        this.save_preferences(window, cx);
                    }))))
            .children(ColumnId::ALL.into_iter().enumerate().map(|(index, id)| {
                let setting = self.settings.columns.get(id);
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_h(crate::appearance::ui_size(32.))
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
                            .px_2()
                            .py_1()
                            .rounded(px(5.))
                            .bg(rgb(p.canvas))
                            .text_size(crate::appearance::ui_text(11.))
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

    fn render_theme_picker(&self, columns: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        // A selection naming a missing custom theme shows the default it resolves to.
        let selected_theme = self.settings.theme.resolve(&self.custom_themes).selection;
        div()
            .flex()
            .flex_col()
            .gap_4()
            .children([true, false].into_iter().map(|light| {
                let choices: Vec<_> = ThemeChoice::ALL
                    .into_iter()
                    .filter(|choice| choice.is_light() == light)
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(appearance::ui_text(12.))
                            .text_color(rgb(p.muted))
                            .font_weight(FontWeight::MEDIUM)
                            .child(if light {
                                "Light palettes"
                            } else {
                                "Dark palettes"
                            }),
                    )
                    .children(choices.chunks(columns).map(|row| {
                        div()
                            .flex()
                            .gap_3()
                            .children(row.iter().copied().map(|choice| {
                                let selected = !self.settings.follow_system
                                    && selected_theme
                                        == appearance::custom::ThemeSelection::BuiltIn(choice);
                                Button::new(("settings-theme", choice as usize))
                                    .ghost()
                                    .group("settings-theme-choice")
                                    .debug_selector(move || {
                                        format!("settings-theme-{}", choice as usize)
                                    })
                                    .accessibility_label(format!("{} theme", choice.label()))
                                    .selected(selected)
                                    .toggled(selected)
                                    .tooltip(format!(
                                        "{} · {}",
                                        choice.label(),
                                        choice.description()
                                    ))
                                    .flex_1()
                                    .min_w_0()
                                    .h(appearance::ui_size(166.))
                                    .p(px(2.))
                                    .border_1()
                                    .border_color(rgb(if selected { p.accent } else { p.border }))
                                    .rounded(px(10.))
                                    .overflow_hidden()
                                    .child(theme_preview(
                                        choice,
                                        selected,
                                        p.accent,
                                        p.accent_foreground,
                                    ))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.choose_theme(choice, window, cx)
                                    }))
                            }))
                            .children((row.len()..columns).map(|_| div().flex_1().min_w_0()))
                    }))
            }))
            .into_any_element()
    }

    pub(super) fn render_settings(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        let compact = window.viewport_size().width < appearance::ui_size(1060.);
        let narrow = window.viewport_size().width < appearance::ui_size(720.);
        let theme_columns = if narrow { 2 } else { 3 };
        let branch_edited =
            self.settings_branch.read(cx).value().as_ref() != self.settings.default_branch.as_str();
        let identity_edited = self.profile.as_ref().is_none_or(|profile| {
            self.identity_name.read(cx).value().trim() != profile.name
                || self.identity_email.read(cx).value().trim() != profile.email
        });
        let repository_name = self
            .repository
            .as_ref()
            .map(|repository| self.project_name(repository.path()));
        let appearance = div()
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
                        "Follow system appearance",
                        "Braden in Light Mode; your selected dark palette in Dark Mode.",
                        cx,
                    ))
                    .child(
                        Switch::new("follow-system")
                            .checked(self.settings.follow_system)
                            .label("Follow system appearance")
                            .on_click(cx.listener(|this, checked, window, cx| {
                                this.settings.follow_system = *checked;
                                this.apply_appearance(window, cx);
                                this.save_preferences(window, cx);
                            })),
                    ),
            )
            .child(self.render_theme_picker(theme_columns, cx))
            .child(self.render_text_size_setting(false, cx))
            .child(self.render_text_size_setting(true, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .when(narrow, |row| row.flex_col().items_start())
                    .pt_4()
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .child(setting_description(
                        "List density",
                        "A little breathing room, or more at a glance.",
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .p_1()
                            .rounded(px(8.))
                            .bg(rgb(p.canvas))
                            .children(Density::ALL.into_iter().enumerate().map(
                                |(index, density)| {
                                    Button::new(("settings-density", index))
                                        .small()
                                        .ghost()
                                        .label(density.label())
                                        .selected(self.settings.density == density)
                                        .toggled(self.settings.density == density)
                                        .when(self.settings.density == density, |button| {
                                            button.hover(|style| style.opacity(0.9))
                                        })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            if this.settings.density != density {
                                                this.settings.density = density;
                                                this.save_preferences(window, cx);
                                            }
                                        }))
                                },
                            )),
                    ),
            )
            .into_any_element();

        let startup = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(div().flex().flex_col().gap_2()
                .child(setting_description("External editor", "Application name on macOS; executable path on Linux. File → Open Repository in Editor opens the current folder.", cx))
                .child(Input::new(&self.settings_editor).aria_label("Preferred external editor"))
                .child(button("save-external-editor", "Save editor", "", false)
                    .disabled(self.settings_editor.read(cx).value().trim() == self.settings.external_editor)
                    .on_click(cx.listener(|this, _, window, cx| { this.settings.external_editor = this.settings_editor.read(cx).value().trim().to_owned(); this.save_preferences(window, cx); }))))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(setting_description(
                        "Restore repository tabs on startup",
                        "Reopen the selected tab and retain up to eight saved repository tabs.",
                        cx,
                    ))
                    .child(
                        Switch::new("settings-reopen-last")
                            .accessibility_label("Restore repository tabs on startup")
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
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(setting_description(
                        "Show the project list",
                        "Keep a pane on the far left to jump between known projects and sort them into groups.",
                        cx,
                    ))
                    .child(
                        Switch::new("settings-project-pane")
                            .accessibility_label("Show the project list")
                            .checked(self.settings.project_pane)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                if this.settings.project_pane != *checked {
                                    this.settings.project_pane = *checked;
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
                            .child(
                                Button::new("save-default-branch")
                                    .label("Save")
                                    .disabled(!branch_edited)
                                    .tooltip("Save the default branch for new repositories")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_default_branch(window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element();

        let identity = if let Some(repository_name) = repository_name {
            let repository_identity = format!(
                "{}\n{}",
                repository_name,
                self.path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            );
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .id("settings-repository-identity")
                        .role(Role::Label)
                        .aria_label(format!("Repository {repository_identity}"))
                        .tooltip(move |window, cx| {
                            Tooltip::new(repository_identity.clone()).build(window, cx)
                        })
                        .rounded(px(8.))
                        .bg(rgb(p.canvas))
                        .p_3()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().flex().items_center().gap_2().child(icon("folder", 14., p.accent)).child(div().min_w_0().truncate().text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::MEDIUM).child(repository_name)))
                        .child(
                            div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).overflow_hidden().child(
                                self.path.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
                            ),
                        )
                        .child(div().text_size(crate::appearance::ui_text(11.)).line_height(relative(1.5)).text_color(rgb(p.muted)).child(
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
                        .text_size(crate::appearance::ui_text(11.))
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
                                .icon(Icon::default().path("icons/check.svg").size(px(14.)))
                                .label(if busy { "Working…" } else { "Save repository identity" })
                                .disabled(busy || !identity_edited)
                                .on_click(cx.listener(|this, _, window, cx| this.save_identity(window, cx))),
                        )
                        .child(
                            button("use-current-identity", "Reset edits", "refresh", false)
                                .tooltip("Restore the repository’s current name and email")
                                .disabled(busy || self.profile.is_none() || !identity_edited)
                                .on_click(cx.listener(|this, _, window, cx| this.fill_identity_inputs(window, cx))),
                        ),
                )
                .child(div().text_size(crate::appearance::ui_text(11.)).line_height(relative(1.5)).text_color(rgb(p.muted)).child(if self.profile.as_ref().is_some_and(|profile| profile.private_worktree) { "Name and email are saved only to this worktree’s existing private Git configuration." } else { "Name and email are saved to repository Git configuration shared by linked worktrees. Global identity is preserved." }))
                .child(button("settings-manage-profiles", "Manage named profiles…", "user", false).disabled(busy).on_click(cx.listener(|this, _, window, cx| this.open_profiles(window, cx))))
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
                        .text_size(crate::appearance::ui_text(13.))
                        .child("Open a project to set its Git identity."),
                )
                .child(
                    div()
                        .text_size(crate::appearance::ui_text(12.))
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
                    .on_click(cx.listener(|this, _, window, cx| this.show_projects(window, cx))),
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
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .when(narrow, |body| body.p_4())
                    .child(
                        div()
                            .w_full()
                            .max_w(px(1160.))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(crate::appearance::ui_text(12.))
                                    .text_color(rgb(p.muted))
                                    .child(icon("check", 14., p.accent))
                                    .child("Appearance and layout save automatically."),
                            )
                            .when_some(self.operation_error.as_ref(), |element, error| {
                                element.child(
                                    div()
                                        .p_3()
                                        .rounded(px(8.))
                                        .bg(rgb(p.removed_background))
                                        .text_color(rgb(p.removed))
                                        .text_size(crate::appearance::ui_text(12.))
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .child(div().flex_1().min_w_0().child(error.clone()))
                                        .when(error.starts_with("Could not save settings:"), |notice| {
                                            notice.child(Button::new("retry-save-settings").label("Retry save").on_click(cx.listener(|this, _, window, cx| this.save_preferences(window, cx))))
                                        }),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap_5()
                                    .when(compact, |columns| columns.flex_col())
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .when(compact, |column| column.flex_none().w_full())
                                            .flex()
                                            .flex_col()
                                            .gap_5()
                                            .child(settings_section(
                                                "Appearance",
                                                "sliders",
                                                "Choose a palette for your workspace and code previews.",
                                                appearance,
                                                cx,
                                            ))
                                            .child(settings_section(
                                                "History columns",
                                                "columns",
                                                "Keep the information you care about in view.",
                                                self.render_columns_controls(cx),
                                                cx,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .w(px(360.))
                                            .when(compact, |column| column.w_full())
                                            .flex_shrink_0()
                                            .flex()
                                            .flex_col()
                                            .gap_5()
                                            .child(settings_section(
                                                "Projects",
                                                "folder",
                                                "Choose how new sessions and projects start.",
                                                startup,
                                                cx,
                                            ))
                                            .child(settings_section(
                                                "Git identity",
                                                "user",
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

/// A tiny workspace built from native elements stays crisp at any display scale
/// and previews the same tokens the real controls and diff viewer will use.
fn theme_preview(
    choice: ThemeChoice,
    selected: bool,
    active_accent: u32,
    active_foreground: u32,
) -> AnyElement {
    let p = choice.palette();
    div()
        .size_full()
        .rounded(px(7.))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .group_hover("settings-theme-choice", move |style| {
            style.border_color(rgb(active_accent))
        })
        .overflow_hidden()
        .flex()
        .flex_col()
        .bg(rgb(p.canvas))
        .child(
            div()
                .h(crate::appearance::ui_size(25.))
                .flex_shrink_0()
                .px_2()
                .flex()
                .items_center()
                .gap_1()
                .bg(rgb(p.panel))
                .border_b_1()
                .border_color(rgb(p.border))
                .children(
                    [p.removed, p.modified, p.added]
                        .map(|color| div().size(px(4.)).rounded_full().bg(rgb(color))),
                )
                .child(div().flex_1())
                .child(div().w(px(21.)).h(px(6.)).rounded(px(2.)).bg(rgb(p.hover)))
                .child(div().w(px(24.)).h(px(8.)).rounded(px(2.)).bg(rgb(p.accent))),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .w(px(30.))
                        .flex_shrink_0()
                        .h_full()
                        .p(px(6.))
                        .flex()
                        .flex_col()
                        .gap(px(7.))
                        .bg(rgb(p.subtle))
                        .border_r_1()
                        .border_color(rgb(p.border))
                        .children((0..4).map(|row| {
                            div().h(px(3.)).w_full().rounded_full().bg(rgb(if row == 1 {
                                p.accent
                            } else {
                                p.border
                            }))
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .py_2()
                        .flex()
                        .flex_col()
                        .children((0..4).map(|row| {
                            let color = [p.added, p.accent, p.renamed, p.modified][row];
                            div()
                                .h(px(14.))
                                .px_2()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .when(row == 1, |element| element.bg(rgb(p.selected)))
                                .child(
                                    div()
                                        .relative()
                                        .w(px(9.))
                                        .h_full()
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(div().w(px(2.)).h_full().bg(rgb(color)))
                                        .child(
                                            div()
                                                .absolute()
                                                .size(px(5.))
                                                .rounded_full()
                                                .bg(rgb(color)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .h(px(3.))
                                                .w(relative([0.68, 0.84, 0.55, 0.74][row]))
                                                .rounded_full()
                                                .bg(rgb(if row == 1 { p.muted } else { p.border })),
                                        ),
                                )
                        })),
                ),
        )
        .child(
            div()
                .id("theme-preview-caption")
                .h(crate::appearance::ui_size(70.))
                .flex_shrink_0()
                .px_3()
                .py_2()
                .flex()
                .flex_col()
                .gap(px(4.))
                .bg(rgb(p.panel))
                .group_hover("settings-theme-choice", |style| style.bg(rgb(p.hover)))
                .group_active("settings-theme-choice", |style| style.bg(rgb(p.selected)))
                .border_t_1()
                .border_color(rgb(p.border))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_1()
                        .text_size(crate::appearance::ui_text(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text))
                        .child(div().min_w_0().truncate().child(choice.label()))
                        .when(selected, |element| {
                            element.child(
                                div()
                                    .size(px(18.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(rgb(active_accent))
                                    .child(icon("check", 12., active_foreground)),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(crate::appearance::ui_text(10.))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(rgb(p.muted))
                        .truncate()
                        .child(choice.description()),
                )
                .child(
                    div().flex().gap(px(4.)).children(
                        [p.accent, p.added, p.hunk, p.renamed, p.modified, p.removed]
                            .map(|color| div().size(px(5.)).rounded_full().bg(rgb(color))),
                    ),
                ),
        )
        .into_any_element()
}

fn setting_description(title: &'static str, description: &'static str, cx: &App) -> AnyElement {
    let p = palette(cx);
    div()
        .id(title)
        .role(Role::Label)
        .aria_label(format!("{title}. {description}"))
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(crate::appearance::ui_text(12.))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_text(11.))
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
        .text_size(crate::appearance::ui_text(12.))
        .text_color(rgb(palette(cx).text))
        .child(label)
        .child(Input::new(input).disabled(disabled))
        .into_any_element()
}

fn settings_section(
    title: &'static str,
    symbol: &'static str,
    description: &'static str,
    content: AnyElement,
    cx: &App,
) -> AnyElement {
    let p = palette(cx);
    div()
        .w_full()
        .min_w_0()
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
                .items_start()
                .gap_3()
                .child(
                    div()
                        .size(px(32.))
                        .flex_shrink_0()
                        .rounded(px(9.))
                        .bg(rgb(p.selected))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(symbol, 16., p.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::appearance::ui_text(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(crate::appearance::ui_text(11.))
                                .line_height(relative(1.5))
                                .text_color(rgb(p.muted))
                                .child(description),
                        ),
                ),
        )
        .child(content)
        .into_any_element()
}

#[cfg(test)]
mod draft_tests {
    use super::DraftState;
    use std::path::Path;

    fn identity(name: &str) -> (String, String) {
        (name.into(), format!("{name}@example.invalid"))
    }

    #[test]
    fn unsaved_default_branch_survives_reentry_and_external_updates_until_saved() {
        let mut state = DraftState::new("main".into());
        for saved in ["main", "main", "external"] {
            assert_eq!(state.branch_update(saved, "draft-branch"), None);
        }
        assert_eq!(state.branch_update("draft-branch", "draft-branch"), None);
        assert_eq!(
            state.branch_update("new-default", "draft-branch"),
            Some("new-default".into())
        );
    }

    #[test]
    fn identity_edits_retain_scope_and_reset_or_untouched_fields_follow_effective_git() {
        let personal = Path::new("/fixture/personal");
        let work = Path::new("/fixture/work");
        let mut state = DraftState::new("main".into());
        assert_eq!(
            state.identity_update(personal, &identity("Alice"), &identity("")),
            Some(identity("Alice"))
        );
        for effective in ["Alice", "Alice", "External"] {
            assert_eq!(
                state.identity_update(personal, &identity(effective), &identity("Draft")),
                None
            );
        }
        state.reset_identity(Some(personal), identity("External"));
        assert_eq!(
            state.identity_update(personal, &identity("Updated"), &identity("External")),
            Some(identity("Updated"))
        );
        assert_eq!(
            state.identity_update(work, &identity("Work"), &identity("PersonalDraft")),
            Some(identity("Work"))
        );
        assert_eq!(
            state.identity_update(work, &identity("SavedEdit"), &identity("SavedEdit")),
            None
        );
        assert_eq!(
            state.identity_update(work, &identity("NewEffective"), &identity("SavedEdit")),
            Some(identity("NewEffective"))
        );
    }
}

#[cfg(test)]
mod list_scale_tests {
    use super::*;
    use core::prelude::v1::test;

    struct Probe {
        scroll: UniformListScrollHandle,
        row_height: Pixels,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).h(px(260.)).child(
                uniform_list(
                    "scaled-list",
                    800,
                    cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                        range
                            .map(|_| div().h(this.row_height).w_full().into_any_element())
                            .collect::<Vec<_>>()
                    }),
                )
                .size_full()
                .track_scroll(&self.scroll),
            )
        }
    }

    #[gpui::test]
    fn fractional_list_scaling_preserves_measured_row_and_partial_offset(cx: &mut TestAppContext) {
        let (probe, cx) = cx.add_window_view(|_, _| Probe {
            scroll: UniformListScrollHandle::new(),
            row_height: px(34.),
        });
        cx.update(|window, cx| {
            for (base, metric) in [(34., 0), (44., 1), (30., 2), (68., 3)] {
                probe.update(cx, |probe, cx| {
                    probe.row_height = px(base);
                    probe
                        .scroll
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(px(0.), px(0.)));
                    cx.notify();
                });
                window.draw(cx).clear(cx);
                let scroll = probe.read(cx).scroll.clone();
                let initial_height =
                    scroll.0.borrow().last_item_size.unwrap().contents.height / 800.;
                scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), -initial_height * 500.25));
                window.draw(cx).clear(cx);
                let mut previous = 1.;
                for scale in [1.25, 1.15, 1.] {
                    let scales =
                        ListScales::new(previous, scale, appearance::Density::Comfortable, window)
                            .unwrap();
                    let ratio = [
                        scales.history,
                        scales.files,
                        scales.navigation,
                        scales.lineage,
                    ][metric];
                    probe.update(cx, |probe, cx| {
                        probe.row_height = px(base * scale);
                        rescale_list_scroll(&probe.scroll, ratio);
                        assert!(probe.scroll.0.borrow().last_item_size.is_none());
                        // A quiet snapshot may arrive after invalidation and
                        // before paint. Its snapped fallback must identify the
                        // same anchor rather than using the fractional style.
                        let height = f32::from(window.pixel_snap(probe.row_height));
                        let top =
                            -f32::from(probe.scroll.0.borrow().base_handle.offset().y) / height;
                        assert!((top - 500.25).abs() < 0.001);
                        cx.notify();
                    });
                    window.draw(cx).clear(cx);
                    let state = scroll.0.borrow();
                    let height = state.last_item_size.unwrap().contents.height / 800.;
                    assert_eq!(window.pixel_snap(px(base * scale)), height);
                    let top = -f32::from(state.base_handle.offset().y) / f32::from(height);
                    assert!(
                        (top - 500.25).abs() < 0.001,
                        "base={base} scale={scale} top={top}"
                    );
                    assert_eq!(state.base_handle.offset().x, px(0.));
                    previous = scale;
                }
            }
        });
    }
}

#[cfg(test)]
mod theme_apply_tests {
    use super::*;
    use crate::{split_diff::SplitPresentation, text::PatchPresentation};
    use core::prelude::v1::test;
    use std::{cell::RefCell, rc::Rc};

    fn comparison() -> Arc<Content> {
        let old = (0..200)
            .map(|row| format!("row {row}\n"))
            .collect::<String>();
        let new = old.replace("row 120\n", "row 120 turtle\n");
        let patch = "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -120,3 +120,3 @@\n row 119\n-row 120\n+row 120 turtle\n row 121\n";
        let presentation = Arc::new(PatchPresentation::prepare(patch));
        let split = Arc::new(SplitPresentation::prepare(&old, &new, &presentation));
        Arc::new(Content::Text {
            diagrams: None,
            markdown: None,
            patch: patch.into(),
            old,
            new,
            presentation,
            split,
            partial: None,
            partial_unavailable: None,
        })
    }

    fn settle(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// A theme switch restyles the retained comparison in place. It must not
    /// submit a read, re-prepare content, replace an editor, or reset Find.
    /// The switches are real clicks on Settings theme cards, so they run the
    /// same `choose_theme` handler that `gitturtle.theme_apply_frame_ms` traces.
    #[gpui::test]
    fn theme_switch_submits_no_job_and_keeps_editor_and_find(cx: &mut TestAppContext) {
        // GitTurtle::new starts a real preferences worker; saves reply from it.
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                )
            });
            *output.borrow_mut() = Some(app.clone());
            gpui_kit::component::Root::new(app, window, cx)
        });
        let app = captured.borrow().as_ref().unwrap().clone();
        let content = comparison();
        // The comparison retains a split view beside the unified editor that
        // carries Find, as one does after the user has visited both modes.
        let (editor, split) = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.mode = WorkspaceMode::Compare;
                app.content = Some(Arc::clone(&content));
                app.text_mode = TextMode::Split;
                app.ensure_editor(window, cx);
                app.text_mode = TextMode::Unified;
                app.ensure_editor(window, cx);
                let split = app.split_view.clone().expect("a retained split view");
                let editor = app.patch_editor.clone().expect("a retained patch editor");
                editor.update(cx, |editor, cx| {
                    editor.open_search(false, cx);
                    editor.set_search_query("row 12", false, cx);
                    editor.next_search_match(cx);
                    editor.next_search_match(cx);
                });
                app.show_settings(window, cx);
                (editor, split)
            })
        });
        settle(cx);
        let find = |cx: &mut VisualTestContext| {
            cx.read(|cx| {
                let session = editor.read(cx).search_session();
                (
                    session.open,
                    session.query.clone(),
                    session.matcher.matched_ranges().as_ref().clone(),
                    session.matcher.current_match_index(),
                )
            })
        };
        let before = find(cx);
        assert!(before.0, "Find is open before the switch");
        assert_eq!(before.2.len(), 3, "the patch has three `row 12` lines");
        assert!(before.3 > 0, "Find advanced past the first match");
        // `GitTurtle::request` advances the generation, retains a reply task and
        // shows a loading label for every content read it submits; the
        // retained content `Arc` below proves nothing re-prepared it.
        let submissions = |cx: &mut VisualTestContext| {
            cx.read(|cx| {
                let app = app.read(cx);
                (app.generation, app.task.is_some(), app.loading)
            })
        };
        let idle = submissions(cx);
        assert!(!idle.1 && idle.2.is_none());
        let initial = cx.read(|cx| app.read(cx).settings.theme);
        // Light palettes are the first rows of the picker, so their cards are
        // on screen without scrolling the Settings page.
        let choices = ThemeChoice::ALL
            .into_iter()
            .filter(|choice| {
                choice.is_light() && appearance::custom::ThemeSelection::BuiltIn(*choice) != initial
            })
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(choices.len(), 2);
        for choice in choices {
            let selector: &'static str = format!("settings-theme-{}", choice as usize).leak();
            let card = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{} theme card is rendered", choice.label()));
            cx.simulate_click(card.center(), Modifiers::default());
            settle(cx);
            assert_eq!(
                cx.read(|cx| app.read(cx).settings.theme),
                appearance::custom::ThemeSelection::BuiltIn(choice),
                "clicking the {} card chose it",
                choice.label()
            );
        }
        assert_eq!(submissions(cx), idle, "a theme switch submitted a read");
        cx.read(|cx| {
            let app = app.read(cx);
            assert_eq!(app.page, AppPage::Settings, "Settings stays open");
            assert!(Arc::ptr_eq(app.content.as_ref().unwrap(), &content));
            assert_eq!(
                app.patch_editor.as_ref().map(Entity::entity_id),
                Some(editor.entity_id())
            );
            assert_eq!(
                app.split_view.as_ref().map(Entity::entity_id),
                Some(split.entity_id())
            );
        });
        assert_eq!(find(cx), before, "Find query and matches survive");
    }
}
