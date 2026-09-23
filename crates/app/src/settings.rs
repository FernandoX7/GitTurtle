use crate::*;
use appearance::custom::{CustomTheme, ThemeSelection};
use appearance::{Density, ThemeChoice};
use columns::{ColumnId, ColumnSettings};
use gitturtle_core::WriteCommand;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
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

/// Whether the Your themes rows stand off a row boundary in the frame being
/// drawn, so that a row partly scrolled out of the viewport crosses the focus
/// ring's room at one end of it. The list applies a pending `scroll_to_item`
/// in its prepaint, after this render, so the answer is taken from where the
/// list will stand and not from where it stood: `reveal_focused_row`'s
/// Nearest scroll leaves the offset alone while the row is whole in the
/// viewport and otherwise lands the row's top or bottom on the viewport's, a
/// row boundary, as the list itself resolves it. Within half a device pixel
/// of a boundary counts as on it, since no painted edge moves.
fn rows_off_boundary(
    scroll: &UniformListScrollHandle,
    row: Pixels,
    viewport: Pixels,
    rows: usize,
    window: &Window,
) -> bool {
    use gpui::ScrollStrategy::{Bottom, Center, Nearest, Top};
    let state = scroll.0.borrow();
    let mut top = -state.base_handle.offset().y;
    if let Some(reveal) = &state.deferred_scroll_to_item {
        let item_top = row * reveal.item_index as f32;
        let lead = row * reveal.offset as f32;
        let above = item_top < top + lead;
        let below = item_top + row > top + viewport;
        if reveal.scroll_strict || above || below {
            let strategy = match reveal.strategy {
                Nearest if above => Top,
                Nearest if below => Bottom,
                strategy => strategy,
            };
            let target = match strategy {
                Top => item_top - lead,
                Bottom => item_top + row - viewport,
                Center => item_top + row / 2. - (lead + (viewport - lead) / 2.),
                Nearest => top,
            };
            let reach = (row * rows as f32 - viewport).max(Pixels::ZERO);
            top = target.max(Pixels::ZERO).min(reach);
        }
    }
    let row = f32::from(row);
    let phase = f32::from(top).rem_euclid(row);
    phase.min(row - phase) > 0.5 / window.scale_factor()
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
        // visible. Reconcile after parent notifications, outside rendering,
        // and ask the cached page for a frame when state it shows from the
        // rest of the app has moved (`reconcile_settings_page`).
        self.subscriptions
            .push(cx.observe_in(&cx.entity(), window, |this, _, window, cx| {
                this.sync_settings_drafts(window, cx);
                this.reconcile_settings_page(cx);
                if this.page == AppPage::Settings {
                    this.sync_theme_cards(cx);
                }
            }));
        // A change in one of the page's inputs reaches the page through its
        // own observation of the input (`SettingsPage::new`); the app has
        // nothing outside the page that shows an unsaved value.
        self.subscriptions.push(cx.subscribe_in(
            &self.settings_branch,
            window,
            |this, _, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.save_default_branch(window, cx)
                }
            },
        ));
        for input in [&self.identity_name, &self.identity_email] {
            self.subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this, _, event, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.save_identity(window, cx)
                    }
                },
            ));
        }
    }

    /// The page for `GitTurtle::render`: the [`SettingsPage`] view, cached so
    /// a frame that alters nothing on it replays its layout and paint. GPUI
    /// collects accessibility nodes only while an element is prepainted, so
    /// while assistive technology is reading the window the page is embedded
    /// uncached and built every frame.
    pub(super) fn settings_page_element(&self, window: &Window) -> AnyElement {
        let page = if window.is_a11y_active() {
            self.settings_page.clone().into_any_element()
        } else {
            self.settings_page
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element()
        };
        div()
            .size_full()
            .child(page)
            .children(
                [
                    &self.settings_editor,
                    &self.settings_branch,
                    &self.identity_name,
                    &self.identity_email,
                ]
                .map(|input| ViewNodeAnchor::new(input.entity_id())),
            )
            .child(CardLayer {
                slots: self.picker_cards.clone(),
            })
            .into_any_element()
    }

    /// Have the next frame build the Settings page again. Every site that
    /// alters what the page shows calls this beside its own `cx.notify()`;
    /// the page is a cached view, so a notification of the app alone would
    /// draw the window with the page replayed from the previous frame. The
    /// sites are enumerated in `crates/app/docs/content-and-layout.md`.
    ///
    /// This notifies the page's entity without leasing it, so it is safe from
    /// any app method, including ones the page's own render calls.
    pub(super) fn notify_settings_page(&self, cx: &mut App) {
        App::notify(cx, self.settings_page.entity_id());
    }

    /// What the page shows that other features change: the operation error
    /// and busy state (`workspace::write` and its completion), the status
    /// line, the effective identity a status reply resolves and the open
    /// repository a tab switch replaces. Those sites notify the app, not the
    /// page; the app's self-observer compares this key with the one the page
    /// last built from and notifies the page on a difference.
    fn settings_page_key(&self) -> PageKey {
        PageKey {
            error: self.operation_error.clone(),
            busy: self.operation_busy.is_some(),
            status: self.status.clone(),
            profile: self.profile.clone(),
            repository: self.repository.as_ref().map(|repository| {
                let path = repository.path();
                (path.to_owned(), self.project_name(path))
            }),
        }
    }

    fn reconcile_settings_page(&self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings {
            return;
        }
        let key = self.settings_page_key();
        if self
            .settings_page
            .read(cx)
            .seen
            .as_ref()
            .is_some_and(|seen| *seen != key)
        {
            self.notify_settings_page(cx);
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
                let error_before = this.operation_error.clone();
                let status_before = this.status.clone();
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
                // A completed save costs a whole-window frame, so it only asks
                // for one when it changed something the window shows. Every
                // theme switch after the first leaves both of these alone.
                if this.status != status_before || this.operation_error != error_before {
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
        self.notify_settings_page(cx);
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
            .debug_selector(move || {
                format!(
                    "text-size-setting-{}",
                    if code { "code" } else { "interface" }
                )
            })
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

    /// The retained miniature that draws `selection`'s own palette. The lookup
    /// is by the selection stored beside each body, so neither the order of
    /// [`ThemeChoice::ALL`], nor the enum's discriminants, nor a custom id can
    /// pair a card with another theme's miniature. Every built-in and every
    /// saved custom theme has one: [`Self::set_custom_themes`] keeps the custom
    /// entries in step with `custom_themes`.
    pub(super) fn theme_preview_body(
        &self,
        selection: ThemeSelection,
    ) -> &Entity<ThemePreviewBody> {
        self.theme_previews
            .iter()
            .find_map(|(drawn, body)| (*drawn == selection).then_some(body))
            .expect("every built-in and every saved custom theme has a retained miniature")
    }

    /// Replace the saved custom themes and keep their picker miniatures in
    /// step: a theme keeps its retained body, which takes the new palette and
    /// notifies only when it changed; a new theme gets a body; a deleted
    /// theme's body is dropped. The cards' focus handles follow the same way.
    /// This is the only writer of `custom_themes`, so a custom card is never
    /// drawn without its miniature.
    pub(super) fn set_custom_themes(&mut self, themes: Vec<CustomTheme>, cx: &mut Context<Self>) {
        let kept = |drawn: &ThemeSelection| match drawn {
            ThemeSelection::BuiltIn(_) => true,
            ThemeSelection::Custom(id) => themes.iter().any(|theme| theme.id == *id),
        };
        self.theme_previews.retain(|(drawn, _)| kept(drawn));
        self.theme_card_focus.retain(|(drawn, _)| kept(drawn));
        self.theme_card_bodies.retain(|(drawn, _)| kept(drawn));
        for theme in &themes {
            let selection = ThemeSelection::Custom(theme.id);
            let palette = theme.palette;
            let retained = self
                .theme_previews
                .iter()
                .find_map(|(drawn, body)| (*drawn == selection).then(|| body.clone()));
            let miniature = match retained {
                Some(body) => {
                    body.update(cx, |body, cx| body.set_palette(palette, cx));
                    body.entity_id()
                }
                None => {
                    let body = cx.new(|_| ThemePreviewBody::new(palette));
                    let miniature = body.entity_id();
                    self.theme_previews.push((selection, body));
                    miniature
                }
            };
            if !self
                .theme_card_bodies
                .iter()
                .any(|(drawn, _)| *drawn == selection)
            {
                self.theme_card_bodies
                    .push((selection, cx.new(|_| ThemeCardBody::new(miniature))));
            }
            if !self
                .theme_card_focus
                .iter()
                .any(|(drawn, _)| *drawn == selection)
            {
                self.theme_card_focus.push((selection, cx.focus_handle()));
            }
        }
        self.custom_themes = themes;
    }

    /// The cached body of `selection`'s picker card, found by the selection
    /// stored beside it, as [`Self::theme_preview_body`] finds its miniature.
    pub(super) fn theme_card_body(&self, selection: ThemeSelection) -> &Entity<ThemeCardBody> {
        self.theme_card_bodies
            .iter()
            .find_map(|(drawn, body)| (*drawn == selection).then_some(body))
            .expect("every built-in and every saved custom theme has a card body")
    }

    /// The focus handle of `selection`'s picker card, found by the selection
    /// stored beside it, as [`Self::theme_preview_body`] finds its miniature.
    pub(super) fn theme_card_focus(&self, selection: ThemeSelection) -> &FocusHandle {
        self.theme_card_focus
            .iter()
            .find_map(|(drawn, focus)| (*drawn == selection).then_some(focus))
            .expect("every built-in and every saved custom theme has a picker card")
    }

    /// Theme switches restyle retained editors in place: no worker job, no
    /// content re-preparation. `gitturtle.theme_apply_frame_ms` measures this
    /// handler through the next frame callback under `GITTURTLE_TRACE`, for a
    /// built-in and a custom selection alike.
    pub(super) fn choose_theme(
        &mut self,
        theme: ThemeSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = trace_enabled().then(std::time::Instant::now);
        if self.settings.theme == theme && !self.settings.follow_system {
            return;
        }
        if let ThemeSelection::Custom(id) = theme
            && !self.custom_themes.iter().any(|saved| saved.id == id)
        {
            // A card exists only for a saved theme; a click that lands after
            // its delete must not select what the store no longer holds.
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

    /// The picker: Light palettes, Dark palettes and, when any is saved, Your
    /// themes. Every card is the same element built from a palette, a name, a
    /// description and a readability count; a custom card describes its base
    /// and shows the warning glyph when its palette has findings.
    /// The picker card a selection marks: a selection naming a missing
    /// custom theme shows the default it resolves to; following the system
    /// marks no card.
    pub(super) fn selected_theme_card(&self) -> Option<ThemeSelection> {
        (!self.settings.follow_system)
            .then(|| self.settings.theme.resolve(&self.custom_themes).selection)
    }

    /// The picker's groups and their cards, in display order.
    fn theme_card_groups(&self) -> [(&'static str, &'static str, Vec<ThemeCard>); 3] {
        let built_in = |light: bool| {
            ThemeChoice::ALL
                .into_iter()
                .filter(|choice| choice.is_light() == light)
                .map(|choice| ThemeCard {
                    selection: ThemeSelection::BuiltIn(choice),
                    palette: choice.palette(),
                    name: choice.label().into(),
                    description: choice.description().into(),
                    warnings: 0,
                })
                .collect::<Vec<_>>()
        };
        // Cached per palette, so a frame recomputes no theme's findings.
        let warnings = self.theme_editor.warning_counts(&self.custom_themes);
        let custom = self
            .custom_themes
            .iter()
            .zip(warnings)
            .map(|(theme, warnings)| ThemeCard {
                selection: ThemeSelection::Custom(theme.id),
                palette: theme.palette,
                name: theme.name.clone().into(),
                description: format!("Based on {}", theme.base.label()).into(),
                warnings,
            })
            .collect::<Vec<_>>();
        [
            ("light", "Light palettes", built_in(true)),
            ("dark", "Dark palettes", built_in(false)),
            ("custom", "Your themes", custom),
        ]
    }

    /// Set every picker card body's [`CardKey`] from the app's state and the
    /// applied palette. A body notifies only when its key changed, so this
    /// runs on every app notification while Settings is shown (the app's
    /// self-observer) and in every build of the page, and the frame that
    /// shows a change builds only the bodies it altered.
    pub(super) fn sync_theme_cards(&self, cx: &mut App) {
        let active = palette(cx);
        let selected = self.selected_theme_card();
        for (_, _, cards) in self.theme_card_groups() {
            for card in cards {
                let key = CardKey::new(
                    card.palette,
                    card.name,
                    card.description,
                    card.warnings,
                    selected == Some(card.selection),
                    active,
                );
                self.theme_card_body(card.selection)
                    .update(cx, |body, cx| body.set_key(key, active, cx));
            }
        }
    }

    fn render_theme_picker(
        &self,
        columns: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let selected = self.selected_theme_card();
        // Keyboard focus, as GPUI's `focus_visible` judges it: the last input
        // was a key. A change of focus or of input modality refreshes the
        // window, so the page is built again whenever this changes.
        let focused = window
            .last_input_was_keyboard()
            .then(|| {
                self.theme_card_focus
                    .iter()
                    .find(|(_, handle)| handle.is_focused(window))
                    .map(|(selection, _)| *selection)
            })
            .flatten();
        #[cfg(test)]
        self.card_names.borrow_mut().clear();
        let groups = self.theme_card_groups();
        div()
            .debug_selector(|| "settings-theme-picker".into())
            .flex()
            .flex_col()
            .gap_4()
            .children(
                groups
                    .into_iter()
                    .filter(|(_, _, cards)| !cards.is_empty())
                    .map(|(key, title, cards)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(|| format!("settings-theme-group-{key}"))
                                    .text_size(appearance::ui_text(12.))
                                    .text_color(rgb(p.muted))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(title),
                            )
                            .children(cards.chunks(columns).map(|row| {
                                div()
                                    .flex()
                                    .gap_3()
                                    .children(row.iter().map(|card| {
                                        let button = self.theme_card(
                                            card,
                                            selected == Some(card.selection),
                                            p,
                                            cx,
                                        );
                                        if focused == Some(card.selection) {
                                            focused_theme_card(button, p.accent)
                                        } else {
                                            button.into_any_element()
                                        }
                                    }))
                                    // A partial row's fillers share the
                                    // cards' padding and border, so its
                                    // cards stay on the full rows' columns.
                                    .children(
                                        (row.len()..columns)
                                            .map(|_| div().flex_1().min_w_0().p(px(2.)).border_1()),
                                    )
                            }))
                    }),
            )
            .into_any_element()
    }

    /// One picker card: a ghost toggle button holding the retained miniature
    /// and the caption. A built-in and a custom card differ only in their data.
    fn theme_card(
        &self,
        card: &ThemeCard,
        selected: bool,
        p: appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Button {
        let selection = card.selection;
        let id = match selection {
            ThemeSelection::BuiltIn(choice) => ElementId::from(("settings-theme", choice as usize)),
            ThemeSelection::Custom(id) => ElementId::from(("settings-custom-theme", id as usize)),
        };
        let selector = move || match selection {
            ThemeSelection::BuiltIn(choice) => format!("settings-theme-{}", choice as usize),
            ThemeSelection::Custom(id) => format!("settings-custom-theme-{id}"),
        };
        let accessible_name = format!("{} theme", card.name);
        #[cfg(test)]
        self.card_names
            .borrow_mut()
            .push((selector(), accessible_name.clone()));
        let tooltip = if card.warnings == 0 {
            format!("{} · {}", card.name, card.description)
        } else {
            format!(
                "{} · {} · {} readability warning{}",
                card.name,
                card.description,
                card.warnings,
                if card.warnings == 1 { "" } else { "s" }
            )
        };
        Button::new(id)
            .ghost()
            .group(CARD_GROUP)
            .debug_selector(selector)
            .accessibility_label(accessible_name)
            .track_focus(self.theme_card_focus(selection))
            .selected(selected)
            .toggled(selected)
            .tooltip(tooltip)
            .flex_1()
            .min_w_0()
            .h(appearance::ui_size(132.))
            .p(px(2.))
            .border_1()
            .border_color(rgb(if selected { p.accent } else { p.border }))
            .rounded(px(10.))
            .overflow_hidden()
            .children(self.theme_card_content(card, p))
            .on_click(
                cx.listener(move |this, _, window, cx| this.choose_theme(selection, window, cx)),
            )
    }

    /// A picker card's content on the page: the preview's rounded,
    /// canvas-colored box holding the slot of its body, which [`CardLayer`]
    /// draws, and the pointer probe beside it ([`CardProbe`]). The box's
    /// border stays transparent here; the layer draws it over the body.
    fn theme_card_content(&self, card: &ThemeCard, active: appearance::Palette) -> [AnyElement; 2] {
        let shared = std::rc::Rc::<CardShared>::default();
        [
            div()
                .size_full()
                .rounded(px(7.))
                .border_1()
                .border_color(gpui_kit::transparent_black())
                .overflow_hidden()
                .bg(rgb(card.palette.canvas))
                .child(CardSlot {
                    body: self.theme_card_body(card.selection).clone(),
                    miniature: self.theme_preview_body(card.selection).clone(),
                    palette: card.palette,
                    accent: active.accent,
                    shared: std::rc::Rc::clone(&shared),
                    slots: self.picker_cards.clone(),
                })
                .into_any_element(),
            div()
                .id("theme-card-pointer")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .group_hover(CARD_GROUP, |style| style.italic())
                .group_active(CARD_GROUP, |style| style.font_weight(FontWeight::BLACK))
                .child(CardProbe(shared))
                .into_any_element(),
        ]
    }

    /// Your themes: the saved custom themes with Edit…, Export… and Delete…,
    /// and New theme… and Import… for the card itself.
    fn render_custom_themes(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        // A save, a native theme dialog or a theme file job owns the card's
        // actions until it reports, so one outcome has one visible owner.
        let pending = self.theme_editor.save_pending() || self.theme_editor.transfer_pending();
        let full = self.custom_themes.len() >= preferences::MAX_CUSTOM_THEMES;
        // New theme… is named by its visible label, which the kit's button
        // exposes when no other name is given; Import… says what it opens.
        let (new_label, import_name) = ("New theme…", "Import a theme file");
        // At the bound both actions are disabled, so their tooltips are the
        // only place that says what to do, in the words the bound's own
        // refusals use (`open_theme_editor`, `add_imported_theme`).
        let (new_tooltip, import_tooltip): (SharedString, SharedString) = if full {
            let bound = format!(
                "Up to {} custom themes can be saved.",
                preferences::MAX_CUSTOM_THEMES
            );
            (
                format!("{bound} Delete one before adding another.").into(),
                format!("{bound} Delete one before importing another.").into(),
            )
        } else {
            (
                "Create a theme from a built-in base".into(),
                "Add a theme from an exported JSON file".into(),
            )
        };
        #[cfg(test)]
        {
            let mut names = self.theme_action_names.borrow_mut();
            names.clear();
            names.push(("custom-themes-new".into(), new_label.into()));
            names.push(("custom-themes-import".into(), import_name.into()));
            *self.theme_action_tooltips.borrow_mut() = [
                ("custom-themes-new".into(), new_tooltip.to_string()),
                ("custom-themes-import".into(), import_tooltip.to_string()),
            ];
        }
        div()
            .id("custom-themes-card")
            .debug_selector(|| "custom-themes-card".into())
            // Not a tab stop; lets focus return to New theme… after a delete
            // removes the focused row (`theme_editor::State::card_focus`).
            .track_focus(&self.theme_editor.card_focus(cx))
            .p_4()
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.subtle))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .debug_selector(|| "custom-themes-header".into())
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .child(setting_description(
                        "Your themes",
                        "Start from any built-in theme, adjust its colors with a live preview, and save it under your own name.",
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_2()
                            .child(
                                button("custom-themes-new", new_label, "plus", false)
                                    .debug_selector(|| "custom-themes-new".into())
                                    .disabled(pending || full)
                                    .tooltip(new_tooltip)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_theme_editor(None, window, cx)
                                    })),
                            )
                            .child(
                                button("custom-themes-import", "Import…", "", false)
                                    .debug_selector(|| "custom-themes-import".into())
                                    .accessibility_label(import_name)
                                    .disabled(pending || full)
                                    .tooltip(import_tooltip)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.import_custom_theme(window, cx)
                                    })),
                            ),
                    ),
            )
            .when(self.custom_themes.is_empty(), |card| {
                card.child(
                    div()
                        .debug_selector(|| "custom-themes-empty".into())
                        .text_size(appearance::ui_text(11.))
                        .text_color(rgb(p.muted))
                        .child("No custom themes yet."),
                )
            })
            .when(!self.custom_themes.is_empty(), |card| {
                card.child(self.render_custom_theme_rows(window, cx))
            })
            // The message quotes a path the user chose and, on a refusal, text
            // from the document. Both are clamped where they are built; this
            // second bound keeps the card's own height stable so the settings
            // below it never move, whatever a message turns out to say. The
            // source clamps count characters, not pixels, so a run of wide
            // glyphs or an unbreakable quoted token that wraps whole can still
            // overrun the second line: the ellipsis marks that cut at the
            // render site, and text that fits its two lines is left untouched.
            // Only a single-paragraph message gets that mark, and every import
            // and export notice and every document refusal is one: GPUI's
            // line-clamp truncation cuts a multi-paragraph text at the
            // paragraph break on its last allowed line, and the folder picker's
            // Linux portal guidance is two paragraphs whose second one, the
            // only actionable sentence, the committed no-portal frames show
            // whole at three lines.
            .children(self.theme_editor.notice().map(|notice| {
                div()
                    .id("custom-themes-notice")
                    .debug_selector(|| "custom-themes-notice".into())
                    .role(Role::Label)
                    .aria_label(notice.to_owned())
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .line_clamp(2)
                    .when(!notice.contains('\n'), |el| el.text_ellipsis())
                    .child(notice.to_owned())
            }))
            .children(self.theme_editor.error().map(|error| {
                div()
                    .id("custom-themes-error")
                    .role(Role::Label)
                    .aria_label(error.to_owned())
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.warning))
                    .line_clamp(2)
                    .when(!error.contains('\n'), |el| el.text_ellipsis())
                    .child(error.to_owned())
            }))
            .into_any_element()
    }

    /// The rows, `min(rows, 8)` × 30 px tall and virtualized with
    /// `uniform_list` as the repository's other lists, so a Settings frame
    /// lays out the rows on screen and not every saved theme. Past eight rows
    /// the list scrolls inside the card with the toolkit scrollbar in a
    /// reserved track; every row's Edit…, Export… and Delete… stay tab stops
    /// in order (`theme_editor::State::row_focus`, `GitTurtle::tab_theme_rows`)
    /// and the row holding focus is scrolled into view (`reveal_focused_row`).
    fn render_custom_theme_rows(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        const VISIBLE_ROWS: usize = 8;
        /// The kit's focus ring width, which a focused control paints outside
        /// its own bounds.
        const ROW_RING_ROOM: Pixels = px(3.);
        let rows = self.custom_themes.len();
        let scrolls = rows > VISIBLE_ROWS;
        self.theme_editor.retain_row_focus(&self.custom_themes);
        let focused = self
            .theme_editor
            .focused_row(&self.custom_themes, window, cx);
        self.theme_editor.reveal_focused_row(focused);
        let scroll = self.theme_editor.rows_scroll().clone();
        // The list clips at its own bounds, the ring's room included, so a
        // row partly scrolled out of the viewport paints into that room. Two
        // strips of the card's surface cover the room while the rows stand
        // off a row boundary; on a boundary the rows fill the viewport
        // exactly and the room is the ring's alone (`rows_off_boundary`).
        let strips = scrolls
            && rows_off_boundary(
                &scroll,
                appearance::ui_size(30.),
                appearance::ui_size(30. * VISIBLE_ROWS as f32),
                rows,
                window,
            );
        let surface = rgb(palette(cx).subtle);
        // The box the rows occupy in the card's column: exactly the rows, so
        // the card, the row pitch and everything below the card stay where
        // the plain stack put them. The list itself is positioned over it 3
        // px taller at each end (`ROW_RING_ROOM`) rather than laid out with
        // negative margins: an absolute child does not enter taffy's sizing
        // of the card, whereas a flow child whose margins pull its content
        // contribution below its flex basis collapses the card's content
        // height under max-content sizing (taffy 0.13 scales that negative
        // difference by the item's inner flex basis).
        div()
            .debug_selector(|| "custom-themes-rows-box".into())
            .relative()
            // The rows' hover surface bleeds into the card padding so the row
            // content aligns with the card title.
            .mx(appearance::ui_size(-8.))
            .h(appearance::ui_size(30. * rows.min(VISIBLE_ROWS) as f32))
            .flex_shrink_0()
            .child(
                div()
                    .debug_selector(|| "custom-themes-rows".into())
                    // The kit's focus ring sits outside a row action and the
                    // list clips to its own bounds, so the list keeps the
                    // ring's room above its first row and below its last,
                    // over the card's gaps: the rows and the card do not move
                    // (`DESIGN.md`).
                    .absolute()
                    .top(-ROW_RING_ROOM)
                    .bottom(-ROW_RING_ROOM)
                    .left_0()
                    .right_0()
                    // The list sits inside the Settings page's own scroll
                    // container, and GPUI's scroll listeners never stop a
                    // wheel event, so a step over the rows would move the
                    // list and the page together. Nested scrolling chains
                    // natively: the step is the list's while the list can
                    // still move that way, and the page's only once it
                    // cannot. The list's listener has already added the step
                    // when this one runs (bubble order, innermost first), so
                    // the offset it left, less the step, is where the list
                    // stood.
                    .on_scroll_wheel(move |event, window, cx| {
                        let state = scroll.0.borrow();
                        let Some(size) = state.last_item_size else {
                            return;
                        };
                        let step = event.delta.pixel_delta(window.line_height()).y;
                        if step.is_zero() {
                            return;
                        }
                        let reach = (size.contents.height - size.item.height).max(Pixels::ZERO);
                        let before = state.base_handle.offset().y - step;
                        let list_moves = if step < Pixels::ZERO {
                            before > px(0.5) - reach
                        } else {
                            before < px(-0.5)
                        };
                        if list_moves {
                            cx.stop_propagation();
                        }
                    })
                    .child(
                        uniform_list(
                            "custom-theme-rows",
                            rows,
                            cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                                // Tab across the viewport boundary asked for a
                                // row this render draws: its actions are tab
                                // stops from this frame on, so the next frame
                                // focuses one.
                                if let Some((row, action)) = this
                                    .theme_editor
                                    .take_row_focus_request(&range, this.custom_themes.len())
                                {
                                    let handle =
                                        this.theme_editor.row_focus(this.custom_themes[row].id, cx);
                                    window.on_next_frame(move |window, cx| {
                                        theme_editor::State::focus_row_action(
                                            &handle, action, window, cx,
                                        );
                                    });
                                }
                                let warnings =
                                    this.theme_editor.warning_counts(&this.custom_themes);
                                range
                                    .filter_map(|index| {
                                        let count = warnings.get(index).copied().unwrap_or(0);
                                        this.render_custom_theme_row(index, count, cx)
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        // The ring's room: the list's content mask is its own
                        // bounds, padding included, while the rows are laid
                        // out inside it.
                        .py(ROW_RING_ROOM)
                        // The scrollbar paints an absolute overlay; reserve
                        // its track so the thumb never covers Delete….
                        .when(scrolls, |list| list.pr(Scrollbar::width()))
                        .track_scroll(self.theme_editor.rows_scroll()),
                    ),
            )
            .when(strips, |list| {
                // Painted after the rows, over the room's 3 px at each end
                // of the viewport and nothing of the rows' box; absolute, so
                // the card and the rows keep their geometry.
                let strip = |name: &'static str| {
                    div()
                        .debug_selector(move || name.into())
                        .absolute()
                        .left_0()
                        .right_0()
                        .h(ROW_RING_ROOM)
                        .bg(surface)
                };
                list.child(strip("custom-themes-ring-room-top").top(-ROW_RING_ROOM))
                    .child(strip("custom-themes-ring-room-bottom").bottom(-ROW_RING_ROOM))
            })
            .when(scrolls, |list| {
                list.child(
                    div()
                        .debug_selector(|| "custom-themes-scrollbar".into())
                        .absolute()
                        // The track is the rows' box, not the ring's room.
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(Scrollbar::width())
                        .child(
                            Scrollbar::vertical(self.theme_editor.rows_scroll())
                                .id("custom-themes-scrollbar")
                                .mode(ScrollbarMode::Always)
                                // The handle's bounds include the ring's
                                // room; the thumb is sized from the rows'
                                // viewport and their content, as before it.
                                .viewport_from_layout()
                                .scroll_size(size(
                                    Pixels::ZERO,
                                    appearance::ui_size(30. * rows as f32),
                                )),
                        ),
                )
            })
            .into_any_element()
    }

    /// One Your themes row, built only while it is in the list's viewport.
    fn render_custom_theme_row(
        &self,
        index: usize,
        warnings: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = self.custom_themes.get(index)?;
        let p = palette(cx);
        let pending = self.theme_editor.save_pending() || self.theme_editor.transfer_pending();
        let imported = self.theme_editor.imported();
        let active = (!self.settings.follow_system)
            .then(|| self.settings.theme.resolve(&self.custom_themes).selection);
        let id = theme.id;
        let t = theme.palette;
        let is_active = active == Some(appearance::custom::ThemeSelection::Custom(id));
        let [edit_name, export_name, delete_name] =
            ["Edit", "Export", "Delete"].map(|action| format!("{action} {} theme", theme.name));
        #[cfg(test)]
        self.theme_action_names.borrow_mut().extend([
            (format!("edit-custom-theme-{id}"), edit_name.clone()),
            (format!("export-custom-theme-{id}"), export_name.clone()),
            (format!("delete-custom-theme-{id}"), delete_name.clone()),
        ]);
        let row = div()
            .id(("custom-theme", id as usize))
            .debug_selector(move || format!("custom-theme-{id}"))
            // Not a tab stop; lets a render find the row holding focus and
            // lets Tab reach the row's actions once it is drawn
            // (`theme_editor::State::row_focus`).
            .track_focus(&self.theme_editor.row_focus(id, cx))
            .w_full()
            .h(appearance::ui_size(30.))
            .px(appearance::ui_size(8.))
            .rounded(px(6.))
            // A just-imported theme is highlighted on the selected
            // surface. It is added, not applied: the checkmark still
            // marks the theme the window uses. Hovering the row is not
            // a card action, so it keeps the highlight rather than
            // replacing it with the plain hover surface.
            .when(imported == Some(id), |row| row.bg(rgb(p.selected)))
            .hover(|row| row.bg(rgb(p.row_hover(imported == Some(id)))))
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .rounded(px(3.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(swatch_run(
                        [t.canvas, t.panel, t.accent, t.added, t.removed],
                        appearance::ui_size(8.),
                        appearance::ui_size(16.),
                        Pixels::ZERO,
                        Pixels::ZERO,
                    )),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.text))
                    .child(theme.name.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(theme.base.label()),
            )
            .when(is_active, |row| row.child(icon("check", 12., p.accent)))
            .child(div().flex_1())
            .when(warnings > 0, |row| {
                let text = format!(
                    "{warnings} readability warning{}",
                    if warnings == 1 { "" } else { "s" }
                );
                row.child(
                    div()
                        .id(("custom-theme-warnings", id as usize))
                        .role(Role::Label)
                        .aria_label(format!("{}: {text}", theme.name))
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(appearance::ui_text(11.))
                        .text_color(rgb(p.muted))
                        .child(crate::theme_editor::warning_glyph(p))
                        .child(warnings.to_string()),
                )
            })
            .child(
                button(("edit-custom-theme", id as usize), "Edit…", "", false)
                    .debug_selector(move || format!("edit-custom-theme-{id}"))
                    .accessibility_label(edit_name)
                    .disabled(pending)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_theme_editor(Some(id), window, cx)
                    })),
            )
            .child(
                // The kit's own button tooltip carries the only
                // statement of what Export… writes, and it never
                // opened here: the same declaration opens on the
                // card header, and the row's hover style is the one
                // structural difference. GPUI's element tooltip is
                // driven from prepaint and absolute bounds instead of
                // the kit's hover listener, so it opens inside the
                // row. The holder is a shrink-wrapped flex box around
                // the button and takes no space of its own.
                div()
                    .id(("export-custom-theme-hover", id as usize))
                    .flex()
                    .flex_shrink_0()
                    .tooltip(|window, cx| {
                        Tooltip::element(|_, _| {
                            div()
                                .id("export-custom-theme-tooltip")
                                .debug_selector(|| "export-custom-theme-tooltip".into())
                                .child("Save this theme as a JSON file")
                        })
                        .build(window, cx)
                    })
                    .child(
                        button(("export-custom-theme", id as usize), "Export…", "", false)
                            .debug_selector(move || format!("export-custom-theme-{id}"))
                            .accessibility_label(export_name)
                            .disabled(pending)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.export_custom_theme(id, window, cx)
                            })),
                    ),
            )
            .child(
                button(("delete-custom-theme", id as usize), "Delete…", "", false)
                    .debug_selector(move || format!("delete-custom-theme-{id}"))
                    .accessibility_label(delete_name)
                    .disabled(pending)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_delete_theme(id, window, cx)
                    })),
            );
        Some(row.into_any_element())
    }

    pub(super) fn render_settings(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        let compact = window.viewport_size().width < appearance::ui_size(1060.);
        let narrow = window.viewport_size().width < appearance::ui_size(720.);
        let theme_columns = if narrow {
            2
        } else if compact {
            3
        } else {
            4
        };
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
            .child(self.render_theme_picker(theme_columns, window, cx))
            .child(self.render_custom_themes(window, cx))
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
            // Tab and Shift-Tab walk the Your themes rows before the toolkit's
            // frame-bound tab order (`theme_editor::init`).
            .key_context(theme_editor::SETTINGS_CONTEXT)
            .on_action(cx.listener(|this, _: &theme_editor::NextThemeAction, window, cx| {
                this.tab_theme_rows(true, window, cx)
            }))
            .on_action(cx.listener(
                |this, _: &theme_editor::PreviousThemeAction, window, cx| {
                    this.tab_theme_rows(false, window, cx)
                },
            ))
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

/// The Settings page as its own view.
///
/// `GitTurtle::render` embeds it with [`Entity::cached`]
/// (`GitTurtle::settings_page_element`), so a frame that alters nothing on
/// the page, such as a keystroke in the theme editor's inputs or a caret blink
/// in the dialog over it, replays the page's prepaint and paint instead of
/// laying it out and painting it again. The page owns no state of its own: it
/// renders `GitTurtle::render_settings` through a weak handle to the app,
/// which owns the page and notifies it at every site that alters what it
/// shows (`GitTurtle::notify_settings_page`, the reconciler in the app's
/// self-observer, and GPUI's own hover, press, focus and refresh paths). It
/// observes the page's four text inputs itself: each input's view node is
/// registered outside the page ([`ViewNodeAnchor`]), so an input's
/// notification dirties the input and the app but no longer the page, and the
/// value it shows and its caret would otherwise stand still on the page.
///
/// The failure mode of a missed notification is a page that keeps showing the
/// previous frame's controls until something else refreshes the window: a
/// busy Your themes card that stays disabled, a list without the theme just
/// imported, an error that never appears.
pub(super) struct SettingsPage {
    app: WeakEntity<GitTurtle>,
    /// The cross-feature state the last build showed
    /// (`GitTurtle::settings_page_key`).
    seen: Option<PageKey>,
    /// Test-only: builds of the page, so a test can assert that a frame
    /// reused it or built it again.
    #[cfg(test)]
    renders: usize,
    _subscriptions: Vec<Subscription>,
}

/// The state the Settings page shows that is changed outside its own sites;
/// see `GitTurtle::settings_page_key`.
#[derive(PartialEq)]
struct PageKey {
    error: Option<String>,
    busy: bool,
    status: String,
    profile: Option<gitturtle_core::GitProfile>,
    repository: Option<(PathBuf, String)>,
}

impl SettingsPage {
    pub(super) fn new(
        app: WeakEntity<GitTurtle>,
        inputs: [&Entity<InputState>; 4],
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            app,
            seen: None,
            #[cfg(test)]
            renders: 0,
            _subscriptions: inputs
                .into_iter()
                .map(|input| cx.observe(input, |_, _, cx| cx.notify()))
                .collect(),
        }
    }

    #[cfg(test)]
    pub(super) fn renders(&self) -> usize {
        self.renders
    }
}

/// Where `selector` is on screen, for tests. A frame that replays a cached
/// view records no debug bounds inside it (GPUI records them only while a
/// div paints), so a selector on the cached Settings page is missing after a
/// frame that reused the page; when the last frame lacks `selector`, draw one
/// that builds every view and look again. Read page selectors before counting
/// builds, never inside a measured window.
#[cfg(test)]
pub(crate) fn shown(
    cx: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(selector).or_else(|| {
        page_shows(cx, selector);
        cx.debug_bounds(selector)
    })
}

/// Whether `selector` is on screen in a frame that builds every view, for
/// tests: a negative check on a page selector needs that frame, since after a
/// replayed frame every page selector is missing. Pending work runs first, so
/// no later frame that replays a picker card body replaces that frame.
#[cfg(test)]
pub(crate) fn page_shows(cx: &mut gpui::VisualTestContext, selector: &'static str) -> bool {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let shown = cx.debug_bounds(selector).is_some();
    cx.run_until_parked();
    shown
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let Some(app) = self.app.upgrade() else {
            return div().into_any_element();
        };
        let (page, key) = app.update(cx, |app, cx| {
            // This build's placeholders record the cards again.
            app.picker_cards.0.borrow_mut().clear();
            app.sync_theme_cards(cx);
            (app.render_settings(window, cx), app.settings_page_key())
        });
        self.seen = Some(key);
        page
    }
}

/// Re-homes a toolkit input's view node behind the cached Settings page.
///
/// `Input` renders its `InputState` as a view, and the toolkit's text element
/// notifies that state from `paint` on every frame. A notify issued while the
/// window draws runs no observer: GPUI parks the entity in the window's dirty
/// set and, at the next draw, marks it and every view above its dispatch node
/// dirty. With the input's node inside the page, that made the page rebuild on
/// every frame, whatever caused the frame. This element, prepainted after the
/// page, registers the input's node here instead, so the next draw marks only
/// the input, the app view and the root. The page still follows the inputs it
/// shows through `SettingsPage`'s observers, which fire for the notifies that
/// change an input: typing, the caret, focus and selection.
///
/// This relies on the pinned GPUI's `DispatchTree::set_view_id`, where the
/// last registration of a view's node wins and `view_path_reversed` is the
/// only reader; GPUI marks `set_view_id` for removal. A GPUI update that
/// changes either must keep
/// `state_from_other_features_builds_the_page_through_the_reconciler` green:
/// its idle notification asserts the page is replayed.
pub(super) struct ViewNodeAnchor {
    view: EntityId,
}

impl ViewNodeAnchor {
    pub(super) fn new(view: EntityId) -> Self {
        Self { view }
    }
}

impl IntoElement for ViewNodeAnchor {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ViewNodeAnchor {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        window.set_view_id(self.view);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

/// Where the Settings page's last build placed each picker card's body: its
/// retained views, its bounds, and the content mask and text style of its
/// place. The page's render clears the list and each [`CardSlot`] records its
/// slot while the page is prepainted; a frame that replays the page keeps the
/// previous build's slots, which a replayed page still matches.
#[derive(Clone, Default)]
pub(super) struct CardSlots(std::rc::Rc<std::cell::RefCell<Vec<CardSlotState>>>);

#[cfg(test)]
impl CardSlots {
    /// What [`CardLayer`] painted last for the card that shows `miniature`,
    /// in paint order.
    pub(super) fn painted(&self, miniature: EntityId) -> Vec<CardPart> {
        self.0
            .borrow()
            .iter()
            .find(|slot| slot.miniature.entity_id() == miniature)
            .map(|slot| slot.painted.borrow().clone())
            .unwrap_or_default()
    }

    /// The cards the last page build placed.
    pub(super) fn len(&self) -> usize {
        self.0.borrow().len()
    }
}

/// Test-only: one part [`LayerCard`] painted. `Fill` names the caption color
/// it chose from the card's own palette.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CardPart {
    Fill(u32),
    Body,
    Miniature,
    Ring,
}

/// The picker card group whose hover and press the card body shows.
const CARD_GROUP: &str = "settings-theme-choice";

/// The pointer state of a picker card as its last page paint saw it, by the
/// predicates GPUI evaluates for `group_hover` and `group_active`.
#[derive(Clone, Copy, Default, PartialEq)]
struct CardPointer {
    hovered: bool,
    pressed: bool,
}

/// What a picker card's page elements leave for [`LayerCard`]: the pointer
/// state [`CardProbe`] read at paint, and the content mask at the card
/// button's content ([`CardProbe`]'s prepaint), under which the preview's
/// hover ring was drawn.
#[derive(Default)]
struct CardShared {
    pointer: std::cell::Cell<CardPointer>,
    outer: std::cell::Cell<ContentMask<Pixels>>,
}

pub(super) struct CardSlotState {
    body: Entity<ThemeCardBody>,
    miniature: Entity<ThemePreviewBody>,
    /// The card's own palette: the caption fill and the miniature's text pin.
    palette: appearance::Palette,
    /// The applied accent, which draws the hover ring.
    accent: u32,
    bounds: Bounds<Pixels>,
    mask: ContentMask<Pixels>,
    text: TextStyle,
    shared: std::rc::Rc<CardShared>,
    #[cfg(test)]
    painted: std::rc::Rc<std::cell::RefCell<Vec<CardPart>>>,
}

/// What a picker card's body shows, apart from pointer state: the card's
/// theme and whether it is the applied one, and the applied palette's tokens
/// its status marks draw in, only where it draws them. [`ThemeCardBody`] is
/// built again only when this changes, so a live-preview edit replays every
/// card body except the selected card's when it edits the accent, accent
/// foreground or canvas color and the warned cards' when it edits the warning
/// or canvas color. The hover ring, also in the accent, is drawn outside the body.
#[derive(Clone, PartialEq)]
pub(super) struct CardKey {
    palette: appearance::Palette,
    name: SharedString,
    description: SharedString,
    warnings: usize,
    /// The applied accent, accent foreground and canvas, which draw the
    /// check badge and its ring; absent on a card that is not selected,
    /// which draws no badge.
    badge: Option<(u32, u32, u32)>,
    /// The applied warning and canvas colors, which draw the readability
    /// glyph; absent on a card without findings, which draws no glyph.
    marks: Option<(u32, u32)>,
}

impl CardKey {
    fn new(
        palette: appearance::Palette,
        name: SharedString,
        description: SharedString,
        warnings: usize,
        selected: bool,
        active: appearance::Palette,
    ) -> Self {
        Self {
            palette,
            name,
            description,
            warnings,
            badge: selected.then_some((active.accent, active.accent_foreground, active.canvas)),
            marks: (warnings > 0).then_some((active.warning, active.canvas)),
        }
    }
}

/// A picker card's place on the page: laid out in flow like the cached body
/// it stands for, and recorded in [`CardSlots`] when the page is prepainted,
/// with the text style the body would inherit there. [`CardLayer`] draws the
/// body.
///
/// The page is rebuilt with `Window::refreshing` set on every palette
/// application, which bars reuse for every cached view nested in it, so card
/// bodies inside the page were built again on every live-preview edit. Nor
/// can the page issue the deferred draw itself: a replayed page copies its
/// deferred draws without prepainting them, which leaves each body's cached
/// prepaint range pointing into an older frame.
struct CardSlot {
    body: Entity<ThemeCardBody>,
    miniature: Entity<ThemePreviewBody>,
    palette: appearance::Palette,
    accent: u32,
    shared: std::rc::Rc<CardShared>,
    slots: CardSlots,
}

impl Element for CardSlot {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // The layout a cached view requests: its root style alone.
        let mut style = Style::default();
        style.refine(&StyleRefinement::default().size_full());
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        self.slots.0.borrow_mut().push(CardSlotState {
            body: self.body.clone(),
            miniature: self.miniature.clone(),
            palette: self.palette,
            accent: self.accent,
            bounds,
            mask: window.content_mask(),
            text: window.text_style(),
            shared: std::rc::Rc::clone(&self.shared),
            #[cfg(test)]
            painted: std::rc::Rc::default(),
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

impl IntoElement for CardSlot {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Reads a picker card's pointer state while the page paints. GPUI knows
/// which group is hovered only then: it registers group hitboxes during
/// paint, after the frame's hit test. The probe's box turns its text style
/// italic under the card group's hover and black under its press, the same
/// predicates the card's `group_hover` and `group_active` styles used, and
/// the probe stores what it reads for [`LayerCard`], which paints later in
/// the same frame. Nothing inside draws text. The box's own listeners notify
/// the page on a hover change and refresh the window on a press, so the
/// state is read again whenever it changes. The probe stands beside the
/// preview in the card button, so its prepaint also records the button's
/// content mask.
struct CardProbe(std::rc::Rc<CardShared>);

impl Element for CardProbe {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.refine(&StyleRefinement::default().size_full());
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        self.0.outer.set(window.content_mask());
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let style = window.text_style();
        self.0.pointer.set(CardPointer {
            hovered: style.font_style == FontStyle::Italic,
            pressed: style.font_weight == FontWeight::BLACK,
        });
    }
}

impl IntoElement for CardProbe {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Draws each picker card's body at the slot the page recorded, after the
/// root tree as a deferred draw (`Window::defer_draw`) under the slot's
/// content mask, so the page's scroll viewport still clips it.
///
/// It stands beside the cached page, outside it, so it runs every frame: each
/// card's body and miniature are cached views prepainted with the window's
/// own `refreshing`, reused on a notify-driven frame whether the page around
/// them was built or replayed. At priority 0 the cards paint before dialogs,
/// popups and tooltips, and after every part of the page that is not itself
/// deferred.
pub(super) struct CardLayer {
    slots: CardSlots,
}

impl Element for CardLayer {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        for slot in self.slots.0.borrow().iter() {
            let size = slot.bounds.size;
            let mut card = LayerCard {
                // The text style the body inherits on the page, with its
                // color pinned to the card's own: GPUI's reuse key compares
                // the ambient text style, and the page's follows the active
                // palette. Every text in the body sets its own color.
                text: pinned_text(&slot.text, slot.palette.text),
                body: div()
                    .w(size.width)
                    .h(size.height)
                    .child(
                        slot.body
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    )
                    .into_any_element(),
                miniature: None,
                parts: slot.body.read(cx).parts.clone(),
                view: slot.miniature.clone(),
                palette: slot.palette,
                accent: slot.accent,
                bounds: slot.bounds,
                mask: slot.mask,
                hovered: slot.shared.pointer.get().hovered,
                shared: std::rc::Rc::clone(&slot.shared),
                #[cfg(test)]
                painted: std::rc::Rc::clone(&slot.painted),
            }
            .into_any_element();
            card.layout_as_root(size.map(AvailableSpace::Definite), window, cx);
            // Under the button's mask, where the ring was drawn; the
            // preview's own content paints under the slot's.
            let outer = slot.shared.outer.get();
            window.defer_draw(card, slot.bounds.origin, 0, Some(outer));
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

impl IntoElement for CardLayer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// `style` as a refinement that sets every field, with `color` in place of
/// its color.
fn pinned_text(style: &TextStyle, color: u32) -> TextStyleRefinement {
    TextStyleRefinement {
        color: Some(rgb(color).into()),
        font_family: Some(style.font_family.clone()),
        font_features: Some(style.font_features.clone()),
        font_fallbacks: style.font_fallbacks.clone(),
        font_size: Some(style.font_size),
        line_height: Some(style.line_height),
        font_weight: Some(style.font_weight),
        font_style: Some(style.font_style),
        background_color: style.background_color,
        underline: style.underline,
        strikethrough: style.strikethrough,
        white_space: Some(style.white_space),
        text_overflow: style.text_overflow.clone(),
        text_align: Some(style.text_align),
        line_clamp: style.line_clamp,
    }
}

/// One card of [`CardLayer`], painted in the order the card's own div tree
/// painted it: the cached miniature, the caption's fill, the cached body (the
/// caption's text, marks, swatches and top border), and last the preview's
/// 1 px hover ring, over the miniature's top corners and anti-aliased edge.
/// The fill and the ring follow the pointer state this frame's page paint
/// read ([`CardProbe`]), so the cached body holds nothing that changes with
/// hover or press and a replayed body is never stale.
///
/// The preview clipped its content to the inside of its border while the
/// border was visible (`Style::overflow_mask`), so a hovered card's content
/// is prepainted and painted under that clip too; the clip is part of GPUI's
/// reuse key, so the body and the miniature are built again under it. The
/// state known at prepaint is the last page paint's: when this frame's paint
/// finds it changed, the card asks for one more frame, which prepaints it
/// under the right clip.
struct LayerCard {
    text: TextStyleRefinement,
    body: AnyElement,
    miniature: Option<AnyElement>,
    /// The miniature box and the caption, as the body last laid them out.
    parts: std::rc::Rc<std::cell::Cell<(Bounds<Pixels>, Bounds<Pixels>)>>,
    view: Entity<ThemePreviewBody>,
    palette: appearance::Palette,
    accent: u32,
    bounds: Bounds<Pixels>,
    /// The content mask of the card's slot, inside the preview.
    mask: ContentMask<Pixels>,
    /// Whether the card was hovered when this was prepainted.
    hovered: bool,
    shared: std::rc::Rc<CardShared>,
    #[cfg(test)]
    painted: std::rc::Rc<std::cell::RefCell<Vec<CardPart>>>,
}

impl LayerCard {
    /// The clip of the preview's content: its slot's, and inside the
    /// preview's border while that border is drawn.
    fn content_mask(&self) -> ContentMask<Pixels> {
        if self.hovered {
            self.mask.intersect(&ContentMask {
                bounds: self.bounds,
            })
        } else {
            self.mask
        }
    }
}

impl Element for LayerCard {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let body = &mut self.body;
        let layout = window.with_text_style(Some(self.text.clone()), |window| {
            body.request_layout(window, cx)
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let text = self.text.clone();
        let mask = self.content_mask();
        window.with_content_mask(Some(mask), |window| {
            window.with_text_style(Some(text), |window| {
                self.body.prepaint(window, cx);
                // The body has laid its miniature box out by now, this frame or
                // in the frame it replays, which had the same bounds.
                let (place, _) = self.parts.get();
                let mut miniature = div()
                    .w(place.size.width)
                    .h(place.size.height)
                    // Pinned as in the body: nothing inside draws text.
                    .text_color(rgb(self.palette.text))
                    .child(
                        self.view
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    )
                    .into_any_element();
                miniature.layout_as_root(place.size.map(AvailableSpace::Definite), window, cx);
                miniature.prepaint_at(place.origin, window, cx);
                self.miniature = Some(miniature);
            });
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let pointer = self.shared.pointer.get();
        if pointer.hovered != self.hovered {
            window.request_animation_frame();
        }
        let p = self.palette;
        #[cfg(test)]
        let mut painted = Vec::new();
        // The caption's background: `group_active` over `group_hover` over
        // its own color, as its styles refined.
        let fill = if pointer.pressed {
            p.selected
        } else if pointer.hovered {
            p.hover
        } else {
            p.panel
        };
        let (_, caption) = self.parts.get();
        let text = self.text.clone();
        let mask = self.content_mask();
        window.with_content_mask(Some(mask), |window| {
            window.with_text_style(Some(text), |window| {
                if let Some(miniature) = self.miniature.as_mut() {
                    miniature.paint(window, cx);
                    #[cfg(test)]
                    painted.push(CardPart::Miniature);
                }
                window.paint_quad(background_quad(caption, Corners::default(), fill));
                #[cfg(test)]
                painted.push(CardPart::Fill(fill));
                self.body.paint(window, cx);
                #[cfg(test)]
                painted.push(CardPart::Body);
            });
        });
        if pointer.hovered {
            // The preview's `rounded(7.)`, `border_1()` accent border, which
            // wraps the body 1 px outside it, drawn as its style drew it.
            let ring = self.bounds.dilate(px(1.));
            let mut color: Hsla = rgb(self.accent).into();
            let border = color;
            color.a = 0.;
            window.paint_quad(quad(
                ring,
                Corners::all(px(7.)).clamp_radii_for_quad_size(ring.size),
                color,
                Edges::all(px(1.)),
                border,
                BorderStyle::default(),
            ));
            #[cfg(test)]
            painted.push(CardPart::Ring);
        }
        #[cfg(test)]
        {
            *self.painted.borrow_mut() = painted;
        }
    }
}

impl IntoElement for LayerCard {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// A background quad as `Style::paint` draws a div's `bg`.
fn background_quad(bounds: Bounds<Pixels>, radii: Corners<Pixels>, color: u32) -> PaintQuad {
    let background: Hsla = rgb(color).into();
    let mut border = background;
    border.a = 0.;
    quad(
        bounds,
        radii,
        background,
        Edges::default(),
        border,
        BorderStyle::default(),
    )
}

/// A picker card's body: the preview's miniature box and caption, drawn by
/// [`CardLayer`] outside the cached Settings page.
///
/// The app owns one per built-in and per saved custom theme, beside the
/// card's miniature (`GitTurtle::theme_card_body`), and sets its
/// [`CardKey`] from the app's state (`GitTurtle::sync_theme_cards`): only a
/// change of the key notifies it, so a frame that rebuilds the page for a
/// live-preview edit replays every body whose key the edit left alone. It
/// renders from its own state and reads no other entity. The miniature box
/// is empty: the layer draws the miniature, a separate cached view, at the
/// box's bounds, so a switch that rebuilds the bodies still replays the
/// miniatures. The caption's background and the hover ring follow the
/// pointer and are drawn by the layer, not here.
///
/// The failure mode of a missed key change is a card that keeps its previous
/// caption: the old name, check badge or glyph, or a badge in the previous
/// accent.
pub(super) struct ThemeCardBody {
    key: Option<CardKey>,
    /// The applied palette its status marks draw in; only the tokens in the
    /// key are read.
    active: appearance::Palette,
    /// The card's miniature, which names the body's selectors.
    miniature: EntityId,
    parts: std::rc::Rc<std::cell::Cell<(Bounds<Pixels>, Bounds<Pixels>)>>,
    /// Test-only: builds of this body.
    #[cfg(test)]
    renders: usize,
}

impl ThemeCardBody {
    pub(super) fn new(miniature: EntityId) -> Self {
        Self {
            key: None,
            active: ThemeChoice::default().palette(),
            miniature,
            parts: std::rc::Rc::default(),
            #[cfg(test)]
            renders: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn renders(&self) -> usize {
        self.renders
    }

    fn set_key(&mut self, key: CardKey, active: appearance::Palette, cx: &mut Context<Self>) {
        self.active = active;
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            cx.notify();
        }
    }
}

impl Render for ThemeCardBody {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let Some(key) = self.key.clone() else {
            return div().into_any_element();
        };
        let parts = std::rc::Rc::clone(&self.parts);
        let miniature = self.miniature;
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_children_prepainted(move |bounds, _, _| {
                if let [place, caption] = bounds[..] {
                    parts.set((place, caption));
                }
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .debug_selector(move || format!("theme-miniature-{miniature}")),
            )
            .child(preview_caption(
                key.palette,
                key.name,
                key.description,
                key.badge.is_some(),
                key.warnings,
                self.active,
                miniature,
            ))
            .into_any_element()
    }
}

/// The miniature workspace inside a theme preview: everything a preview draws
/// from the previewed palette alone.
///
/// It has its own entity so the picker can embed it with [`Entity::cached`].
/// Applying a palette recolors the window around these miniatures, one per
/// built-in and one per saved custom theme, without altering one of their
/// pixels, and
/// [`GitTurtle::apply_appearance`](crate::GitTurtle::apply_appearance) notifies
/// its root instead of refreshing the window, so the frame that shows the new
/// palette reuses their layout and paint instead of building them again. The
/// caption stays outside: it follows the active accent and the card's hover
/// state, which a reused subtree cannot see.
pub(super) struct ThemePreviewBody {
    palette: appearance::Palette,
    /// Test-only: builds of this miniature, so a test can assert that a
    /// palette change reuses it instead of laying it out and painting it
    /// again.
    #[cfg(test)]
    renders: usize,
    /// Test-only: real palette changes, so a test can assert which
    /// miniatures a save invalidated.
    #[cfg(test)]
    palette_changes: usize,
}

impl ThemePreviewBody {
    pub(super) fn new(palette: appearance::Palette) -> Self {
        Self {
            palette,
            #[cfg(test)]
            renders: 0,
            #[cfg(test)]
            palette_changes: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn renders(&self) -> usize {
        self.renders
    }

    #[cfg(test)]
    pub(super) fn palette_changes(&self) -> usize {
        self.palette_changes
    }

    /// Test-only: the palette this miniature draws.
    #[cfg(test)]
    pub(super) fn palette(&self) -> appearance::Palette {
        self.palette
    }

    /// Show `palette`. Only a real change notifies, so an editor edit that
    /// leaves the miniature's tokens alone keeps the reused frame.
    pub(super) fn set_palette(&mut self, palette: appearance::Palette, cx: &mut Context<Self>) {
        if self.palette != palette {
            self.palette = palette;
            #[cfg(test)]
            {
                self.palette_changes += 1;
            }
            cx.notify();
        }
    }
}

impl Render for ThemePreviewBody {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let p = self.palette;
        // One painted element, like `swatch_run`: a frame that rebuilds every
        // card, as a switch's click or a Tab's focus does through
        // `Window::refresh`, lays out one node per miniature, not forty.
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| paint_miniature(bounds, p, window),
        )
        .size_full()
    }
}

/// Paint the miniature workspace into `bounds` on palette `p`, as the boxes a
/// div tree of this geometry would paint, through the same `paint_quad`
/// snapping. It is not the base's div tree redrawn: the miniature is
/// re-proportioned for the 132 px card, with a 20 px top bar where the divs
/// had 25 px, a 26 px sidebar where they had 30 px and 10 px rows where they
/// had 14 px (the caption's check badge went from 18 to 16 px with it). A top
/// bar `ui_size(20.)` tall on the panel color over a 1 px border carries three
/// 4 px status dots and two pills; below it a 26 px sidebar on the subtle
/// color, with a 1 px right border and four 3 px stripes inside 5 px of
/// padding, stands beside four 10 px changed-file rows after 5 px, each 8 px
/// in from either edge with a 9 px graph column (a 2 px line under a 5 px
/// node) and a text stripe 6 px on; the second row is selected and its stripe
/// muted.
fn paint_miniature(bounds: Bounds<Pixels>, p: appearance::Palette, window: &mut Window) {
    let mut quad = |x: Pixels, y: Pixels, w: Pixels, h: Pixels, color: u32, radius: Pixels| {
        window.paint_quad(
            gpui::fill(
                Bounds::new(bounds.origin + point(x, y), size(w, h)),
                rgb(color),
            )
            .corner_radii(gpui::Corners::all(radius)),
        );
    };
    let (width, height) = (bounds.size.width, bounds.size.height);
    let one = px(1.);
    let bar = appearance::ui_size(20.);
    quad(px(0.), px(0.), width, bar, p.panel, px(0.));
    quad(px(0.), bar - one, width, one, p.border, px(0.));
    let middle = (bar - one) / 2.;
    for (index, color) in [p.removed, p.modified, p.added].into_iter().enumerate() {
        let x = px(8.) + px(8.) * index as f32;
        quad(x, middle - px(2.), px(4.), px(4.), color, px(2.));
    }
    quad(
        width - px(57.),
        middle - px(3.),
        px(21.),
        px(6.),
        p.hover,
        px(2.),
    );
    quad(
        width - px(32.),
        middle - px(4.),
        px(24.),
        px(8.),
        p.accent,
        px(2.),
    );
    let body = height - bar;
    quad(px(0.), bar, px(26.), body, p.subtle, px(0.));
    quad(px(25.), bar, one, body, p.border, px(0.));
    for row in 0..4 {
        let color = if row == 1 { p.accent } else { p.border };
        let y = bar + px(5.) + px(8.) * row as f32;
        quad(px(5.), y, px(15.), px(3.), color, px(1.5));
    }
    let stripe_width = width - px(57.);
    for row in 0..4 {
        let top = bar + px(5.) + px(10.) * row as f32;
        let color = [p.added, p.accent, p.renamed, p.modified][row];
        if row == 1 {
            quad(px(26.), top, width - px(26.), px(10.), p.selected, px(0.));
        }
        quad(px(37.5), top, px(2.), px(10.), color, px(0.));
        quad(px(36.), top + px(2.5), px(5.), px(5.), color, px(2.5));
        let share: f32 = [0.68, 0.84, 0.55, 0.74][row];
        let stripe = if row == 1 { p.muted } else { p.border };
        quad(
            px(49.),
            top + px(3.5),
            stripe_width * share,
            px(3.),
            stripe,
            px(1.5),
        );
    }
}

/// The picker card holding keyboard focus, in a box that takes the card's
/// place in its row and adds a 2 px accent ring 1 px outside the card's
/// border, inside the grid's 10 px column and 6 px row gaps. The card's own
/// focus shows only as its border, which the selected card already draws in
/// the accent, and it clips its content, so the ring cannot be its child. Only
/// the focused card pays for the box and the ring.
fn focused_theme_card(card: Button, accent: u32) -> AnyElement {
    const OUTSET: f32 = 3.;
    div()
        .flex()
        .flex_1()
        // A card's flex base is its 2 px padding and 1 px border on either
        // side, since its basis is zero; the box starts from the same 6 px,
        // so it takes the width the card beside it takes.
        .flex_basis(px(6.))
        .min_w_0()
        .relative()
        .child(card)
        .child(
            div()
                .debug_selector(|| "theme-card-focus-ring".into())
                .absolute()
                .top(px(-OUTSET))
                .left(px(-OUTSET))
                .right(px(-OUTSET))
                .bottom(px(-OUTSET))
                .border_2()
                .border_color(rgb(accent))
                .rounded(px(10. + OUTSET)),
        )
        .into_any_element()
}

/// What a picker card shows: the theme it selects, the palette its miniature
/// draws, its caption and the count of its readability findings.
struct ThemeCard {
    selection: ThemeSelection,
    palette: appearance::Palette,
    name: SharedString,
    description: SharedString,
    warnings: usize,
}

/// A tiny workspace built from native elements stays crisp at any display scale
/// and previews the same tokens the real controls and diff viewer will use.
/// The theme editor previews its draft with the same miniature.
///
/// `body` supplies [`ThemePreviewBody`]; the caller keeps it and its palette,
/// so a frame that changes neither reuses the miniature. `warnings` is the
/// palette's readability finding count; a non-zero count shows the glyph
/// beside the name. The card is 132 px tall: a 70 px miniature over a 54 px
/// caption inside the button's padding and borders. 54 px is the caption's
/// minimum: at the smallest interface text sizes its fixed padding, gaps,
/// badge and swatches leave the text lines less than their line height, so
/// the caption grows and the miniature gives up the difference.
///
/// `active` is the applied palette. The hover border, the check badge and the
/// warning glyph are status marks, so they are drawn in it rather than in `p`,
/// the palette the card previews and the glyph may flag: a saved theme whose
/// warning color matches its own panel still shows the glyph's shape.
pub(super) fn theme_preview(
    body: &Entity<ThemePreviewBody>,
    p: appearance::Palette,
    label: impl Into<SharedString>,
    description: impl Into<SharedString>,
    selected: bool,
    warnings: usize,
    active: appearance::Palette,
) -> AnyElement {
    // The caption's lines and marks name the embedded body, as the miniature
    // does, so a test can read which card is checked or warned, and the
    // height its text keeps, from the rendered tree.
    let miniature = body.entity_id();
    div()
        .size_full()
        .rounded(px(7.))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .group_hover(CARD_GROUP, move |style| {
            style.border_color(rgb(active.accent))
        })
        .overflow_hidden()
        .flex()
        .flex_col()
        .bg(rgb(p.canvas))
        .child(
            // The miniature, in the box its strip and rows filled before.
            // Pinning the inherited text color keeps GPUI's reuse key stable:
            // it compares the ambient text style, which the page and the
            // toolkit button both color from the active palette. Nothing
            // inside draws text.
            div()
                .flex_1()
                .min_h_0()
                .text_color(rgb(p.text))
                // Names the embedded body, so a test can find which miniature
                // a card holds in the rendered tree.
                .debug_selector(move || format!("theme-miniature-{miniature}"))
                .child(body.clone().cached(StyleRefinement::default().size_full())),
        )
        .child(
            preview_caption(
                p,
                label.into(),
                description.into(),
                selected,
                warnings,
                active,
                miniature,
            )
            .bg(rgb(p.panel))
            .group_hover(CARD_GROUP, |style| style.bg(rgb(p.hover)))
            .group_active(CARD_GROUP, |style| style.bg(rgb(p.selected))),
        )
        .into_any_element()
}

/// A theme preview's caption, without the background that follows the
/// pointer: [`theme_preview`] styles it, and a picker card's caption is
/// filled by [`LayerCard`], outside its cached body.
fn preview_caption(
    p: appearance::Palette,
    label: SharedString,
    description: SharedString,
    selected: bool,
    warnings: usize,
    active: appearance::Palette,
    miniature: EntityId,
) -> Stateful<Div> {
    div()
        .id("theme-preview-caption")
        .debug_selector(move || format!("theme-caption-{miniature}"))
        .min_h(crate::appearance::ui_size(54.))
        .flex_shrink_0()
        .px_3()
        .py(px(6.))
        .flex()
        .flex_col()
        .gap(px(3.))
        .border_t_1()
        .border_color(rgb(p.border))
        .child({
            let name = div()
                .debug_selector(move || format!("theme-name-{miniature}"))
                .min_w_0()
                .truncate()
                .text_size(crate::appearance::ui_text(12.))
                .line_height(relative(1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.text))
                .child(label);
            if selected || warnings > 0 {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(name.flex_1())
                    .when(warnings > 0, |row| {
                        row.child(
                            div()
                                .debug_selector(move || format!("theme-warning-{miniature}"))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap(px(3.))
                                .text_size(crate::appearance::ui_text(10.))
                                .line_height(relative(1.3))
                                .text_color(rgb(p.muted))
                                .child(card_warning_glyph(active, miniature))
                                .child(warnings.to_string()),
                        )
                    })
                    .when(selected, |row| {
                        // The name's line, 15.6 px at the default size: a
                        // taller badge, even `ui_size(16.)` at 11 pt, grows
                        // the selected caption above its neighbours'.
                        // The active canvas rings it, as it
                        // does the glyph, so the mark stays whole on a
                        // caption near the active accent.
                        row.child(
                            div()
                                .debug_selector(move || format!("theme-check-{miniature}"))
                                .size(crate::appearance::ui_text(12.) * 1.3)
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(active.canvas))
                                .bg(rgb(active.accent))
                                .child(icon(
                                    "check",
                                    f32::from(crate::appearance::ui_size(11.)),
                                    active.accent_foreground,
                                )),
                        )
                    })
            } else {
                // Without a mark the name is the whole line, at the
                // same place and width: the row that would hold both
                // is not laid out.
                name
            }
        })
        .child(
            div()
                .debug_selector(move || format!("theme-description-{miniature}"))
                .text_size(crate::appearance::ui_text(10.))
                .line_height(relative(1.3))
                .font_weight(FontWeight::NORMAL)
                .text_color(rgb(p.muted))
                .truncate()
                .child(description),
        )
        .child(swatch_run(
            [p.accent, p.added, p.hunk, p.renamed, p.modified, p.removed],
            px(5.),
            px(5.),
            px(4.),
            px(2.5),
        ))
}

/// A picker card's readability glyph, drawn in `marks` and ringed by 1 px of
/// its canvas inside the glyph's box. The card passes the active palette, as
/// it does for its check badge. The selector names the
/// colors the glyph drew and the card's miniature, so a test reads from the
/// rendered tree which palette the mark on that card used.
fn card_warning_glyph(marks: appearance::Palette, miniature: gpui::EntityId) -> Div {
    crate::theme_editor::warning_glyph(marks)
        .border_1()
        .border_color(rgb(marks.canvas))
        .debug_selector(move || {
            format!(
                "theme-warning-glyph-{miniature}-{:06x}-{:06x}",
                marks.warning, marks.canvas
            )
        })
}

/// `colors` as a run of `width` × `height` boxes `gap` apart with `radius`
/// corners, painted by one element instead of one `div` per color. It paints
/// the quads the divs painted (`paint_quad`, the same bounds and radii from the
/// same origin), so the picker captions and the Your themes swatch strips keep
/// their pixels while a Settings frame lays out one node per strip.
fn swatch_run<const N: usize>(
    colors: [u32; N],
    width: Pixels,
    height: Pixels,
    gap: Pixels,
    radius: Pixels,
) -> AnyElement {
    let count = N as f32;
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            for (index, color) in colors.into_iter().enumerate() {
                let origin = bounds.origin + point((width + gap) * index as f32, Pixels::ZERO);
                window.paint_quad(
                    gpui::fill(Bounds::new(origin, size(width, height)), rgb(color))
                        .corner_radii(gpui::Corners::all(radius)),
                );
            }
        },
    )
    .w(width * count + gap * (count - 1.))
    .h(height)
    .flex_shrink_0()
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
        // Every read reaches the reader through its queue, whether or not
        // `GitTurtle::request` submits it, so the queue's own count is the
        // measure; the retained content `Arc` below proves nothing
        // re-prepared it.
        let submissions =
            |cx: &mut VisualTestContext| cx.read(|cx| app.read(cx).worker.submissions());
        let idle = submissions(cx);
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
            let card = shown(cx, selector)
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
        let submitted = submissions(cx) - idle;
        assert_eq!(
            submitted, 0,
            "a theme switch submitted {submitted} worker jobs"
        );
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
        // The count is live: any submission moves it.
        cx.read(|cx| app.read(cx).worker.release_history());
        assert_eq!(submissions(cx), idle + 1);
    }
}

/// Your themes and picker cards as a real window draws them.
#[cfg(test)]
mod picker_tests {
    use super::*;
    use core::prelude::v1::test;
    use std::{cell::RefCell, rc::Rc};

    fn open_app(cx: &mut TestAppContext) -> (Entity<GitTurtle>, &mut VisualTestContext) {
        // GitTurtle::new starts a real preferences worker; saves reply from it.
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
            theme_editor::init(cx);
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
        // Tall enough that the Your themes card is on screen without scrolling.
        cx.simulate_resize(size(px(1440.), px(2400.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.apply_appearance(window, cx);
                app.show_settings(window, cx)
            })
        });
        settle(cx);
        (app, cx)
    }

    fn settle(cx: &mut VisualTestContext) {
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
    }

    fn seed_themes(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, count: u32) {
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                let themes = (1..=count)
                    .map(|n| CustomTheme::from_base(n, format!("Seed {n:02}"), ThemeChoice::Nord))
                    .collect();
                app.set_custom_themes(themes, cx);
                cx.notify();
            })
        });
        settle(cx);
    }

    /// The miniature `selection`'s picker card embeds, which names the
    /// selectors of its caption.
    fn miniature(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        selection: ThemeSelection,
    ) -> EntityId {
        cx.read(|cx| app.read(cx).theme_preview_body(selection).entity_id())
    }

    fn drawn(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
        shown(cx, selector.leak())
    }

    /// The check badge is the name's line tall at every interface text size:
    /// the selected card's caption, which here
    /// also holds the warning glyph, starts level with its row neighbours'
    /// at the smallest, the default and the largest sizes.
    #[gpui::test]
    fn a_selected_caption_starts_level_with_its_row(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let mut dawn = CustomTheme::from_base(1, "Dawn Chorus", ThemeChoice::RosePineDawn);
        dawn.palette.muted = dawn.palette.canvas;
        assert!(!dawn.palette.readability_issues().is_empty());
        let themes = std::iter::once(dawn)
            .chain(
                (2..=4)
                    .map(|id| CustomTheme::from_base(id, format!("Seed {id}"), ThemeChoice::Nord)),
            )
            .collect();
        let code = cx.read(|cx| app.read(cx).settings.code_text_size);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.set_custom_themes(themes, cx);
                app.settings.theme = ThemeSelection::Custom(1);
                app.apply_appearance(window, cx);
            })
        });
        settle(cx);
        let selected = miniature(cx, &app, ThemeSelection::Custom(1));
        for interface in [11, 12, 13, 18] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.settings.interface_text_size = interface;
                    appearance::apply_text_sizes(interface, code, window, cx);
                    cx.notify();
                })
            });
            settle(cx);
            let badge = drawn(cx, format!("theme-check-{selected}"))
                .unwrap_or_else(|| panic!("at {interface} pt the selected card has its badge"));
            let name = drawn(cx, format!("theme-name-{selected}")).expect("a name");
            assert_eq!(
                badge.size.height, name.size.height,
                "at {interface} pt the badge is the name's line"
            );
            assert!(drawn(cx, format!("theme-warning-{selected}")).is_some());
            let card = |cx: &mut VisualTestContext, id: u32| {
                drawn(cx, format!("settings-custom-theme-{id}")).expect("the card is drawn")
            };
            let caption = drawn(cx, format!("theme-caption-{selected}")).expect("a caption");
            // Four columns fit at 1,440 px up to the scaled breakpoint and
            // three at 18 pt, so Seed 4 may start the next row.
            let row = card(cx, 1).top();
            let beside = (2..=4)
                .filter(|id| card(cx, *id).top() == row)
                .collect::<Vec<_>>();
            assert!(beside.len() >= 2, "at {interface} pt: {beside:?}");
            for id in beside {
                let neighbour = miniature(cx, &app, ThemeSelection::Custom(id));
                assert!(drawn(cx, format!("theme-check-{neighbour}")).is_none());
                let beside = drawn(cx, format!("theme-caption-{neighbour}")).expect("a caption");
                assert_eq!(
                    caption.top(),
                    beside.top(),
                    "at {interface} pt the selected caption {caption:?} starts level with \
                     Seed {id}'s {beside:?}"
                );
            }
        }
    }

    /// A picker card that holds keyboard focus draws a 2 px accent ring 1 px
    /// outside its border, clear of every other card, so focus shows on the
    /// selected card, whose border is already accent. Only the focused card
    /// draws it, and only while the keyboard was the last input.
    #[gpui::test]
    fn keyboard_focus_rings_the_picker_card_outside_its_border(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        let selected = cx
            .read(|cx| app.read(cx).selected_theme_card())
            .expect("a card is selected");
        let selector = |selection: ThemeSelection| match selection {
            ThemeSelection::BuiltIn(choice) => format!("settings-theme-{}", choice as usize),
            ThemeSelection::Custom(id) => format!("settings-custom-theme-{id}"),
        };
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                ThemeChoice::ALL
                    .into_iter()
                    .map(ThemeSelection::BuiltIn)
                    .find(|choice| app.read(cx).theme_card_focus(*choice).is_focused(window))
            })
        };
        let ring = |cx: &mut VisualTestContext| {
            page_shows(cx, "theme-card-focus-ring");
            cx.debug_bounds("theme-card-focus-ring")
        };
        let device = cx.update(|window, _| px(1. / window.scale_factor()));
        let around = |ring: Bounds<Pixels>, card: Bounds<Pixels>| {
            let outset = card.dilate(px(3.));
            [
                (ring.left(), outset.left()),
                (ring.top(), outset.top()),
                (ring.right(), outset.right()),
                (ring.bottom(), outset.bottom()),
            ]
            .into_iter()
            .all(|(edge, expected)| (edge - expected).abs() <= device)
        };

        // Tab away from the selected card and back: keyboard focus on it.
        let handle = cx.read(|cx| app.read(cx).theme_card_focus(selected).clone());
        cx.update(|window, cx| window.focus(&handle, cx));
        settle(cx);
        cx.simulate_keystrokes("tab");
        cx.simulate_keystrokes("shift-tab");
        settle(cx);
        assert_eq!(focused(cx), Some(selected));
        let card = drawn(cx, selector(selected)).expect("the selected card is drawn");
        let drawn_ring = ring(cx).expect("the focused selected card draws the ring");
        assert!(around(drawn_ring, card), "{drawn_ring:?} around {card:?}");
        let clear = |cx: &mut VisualTestContext, ring: Bounds<Pixels>| {
            for other in ThemeChoice::ALL.map(ThemeSelection::BuiltIn) {
                if other != selected {
                    let other = drawn(cx, selector(other)).expect("every card is drawn");
                    assert!(
                        !ring.intersects(&other),
                        "{ring:?} stays in the gap beside {other:?}"
                    );
                }
            }
        };
        clear(cx, drawn_ring);

        // Tab on: the ring follows focus and leaves the selected card.
        cx.simulate_keystrokes("tab");
        settle(cx);
        let next = focused(cx).expect("Tab reaches the next card");
        assert_ne!(next, selected);
        let next_card = drawn(cx, selector(next)).expect("the next card is drawn");
        let drawn_ring = ring(cx).expect("the focused card draws the ring");
        assert!(
            around(drawn_ring, next_card),
            "{drawn_ring:?} around {next_card:?}"
        );
        assert!(!around(drawn_ring, card));
        assert_eq!(
            drawn(cx, selector(selected)),
            Some(card),
            "the ring's box does not move the card it holds"
        );

        // A click moves no focus (the kit's buttons refuse it on mouse down),
        // and the ring, which marks keyboard focus, goes.
        cx.simulate_click(card.center(), Modifiers::default());
        settle(cx);
        assert_eq!(focused(cx), Some(next));
        assert_eq!(ring(cx), None);

        // The compact grid, three columns with the same gaps.
        cx.simulate_resize(size(px(1000.), px(2400.)));
        settle(cx);
        let resting = drawn(cx, selector(selected)).expect("the selected card is drawn");
        cx.simulate_keystrokes("shift-tab");
        settle(cx);
        assert_eq!(focused(cx), Some(selected));
        assert_eq!(drawn(cx, selector(selected)), Some(resting));
        let drawn_ring = ring(cx).expect("the focused selected card draws the ring");
        assert!(
            around(drawn_ring, resting),
            "{drawn_ring:?} around {resting:?}"
        );
        clear(cx, drawn_ring);
    }

    /// At the 32-theme bound New theme… and Import… are disabled, so their
    /// tooltips are the only place that gives the reason and the remedy.
    #[gpui::test]
    fn bound_tooltips_say_what_to_delete(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        // A frame that builds the page, which a seeded store alone does not.
        let tooltips = |cx: &mut VisualTestContext| {
            assert!(page_shows(cx, "custom-themes-import"));
            cx.read(|cx| app.read(cx).theme_action_tooltips.borrow().clone())
        };
        let expected = |new: &str, import: &str| {
            [
                ("custom-themes-new".to_owned(), new.to_owned()),
                ("custom-themes-import".to_owned(), import.to_owned()),
            ]
        };
        seed_themes(cx, &app, preferences::MAX_CUSTOM_THEMES as u32 - 1);
        assert_eq!(
            tooltips(cx),
            expected(
                "Create a theme from a built-in base",
                "Add a theme from an exported JSON file"
            )
        );

        seed_themes(cx, &app, preferences::MAX_CUSTOM_THEMES as u32);
        assert_eq!(
            tooltips(cx),
            expected(
                "Up to 32 custom themes can be saved. Delete one before adding another.",
                "Up to 32 custom themes can be saved. Delete one before importing another."
            )
        );
        // Both are disabled: a click reaches neither the editor, which would
        // refuse with the bound, nor the import dialog.
        for selector in ["custom-themes-new", "custom-themes-import"] {
            let button = shown(cx, selector).unwrap_or_else(|| panic!("{selector} is drawn"));
            cx.simulate_click(button.center(), Modifiers::default());
            settle(cx);
            cx.read(|cx| {
                let editor = &app.read(cx).theme_editor;
                assert_eq!(editor.error(), None, "{selector}");
                assert_eq!(editor.preview(), None, "{selector}");
                assert!(!editor.transfer_pending(), "{selector}");
            });
        }
    }
}
