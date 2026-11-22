use crate::*;
use appearance::custom::{CustomTheme, ThemeSelection};
use appearance::{CodeFont, Density, ThemeChoice};
use columns::{ColumnId, ColumnSettings};
use gitturtle_core::WriteCommand;
use gpui_kit::base::{FocusableExt, Scrollbar, ScrollbarMode};
use gpui_kit::component::{checkbox::Checkbox, switch::Switch};
use gpui_kit::prelude::FluentBuilder;
use preferences::DisplayChoice;
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
/// ring's room at one end of it. The answer is taken from where the list will
/// stand ([`planned_rows_top`]), which the page reveal of a focused row reads
/// too (`reveal_in_page`). Within half a device pixel of a boundary counts as
/// on it, since no painted edge moves.
fn rows_off_boundary(
    scroll: &UniformListScrollHandle,
    row: Pixels,
    viewport: Pixels,
    rows: usize,
    window: &Window,
) -> bool {
    let top = planned_rows_top(scroll, row, viewport, rows);
    let row = f32::from(row);
    let phase = f32::from(top).rem_euclid(row);
    phase.min(row - phase) > 0.5 / window.scale_factor()
}

/// How far the Your themes list will be scrolled in the frame being drawn.
/// The list applies a pending `scroll_to_item` in its prepaint, after this
/// render, so the answer is where the list will stand and not where it stood:
/// `reveal_focused_row`'s Nearest scroll leaves the offset alone while the row
/// is whole in the viewport and otherwise lands the row's top or bottom on the
/// viewport's, a row boundary, as the list itself resolves it.
fn planned_rows_top(
    scroll: &UniformListScrollHandle,
    row: Pixels,
    viewport: Pixels,
    rows: usize,
) -> Pixels {
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
    top
}

/// A Page Up, Page Down, Home or End in Settings (`scroll_settings_page`).
#[derive(Clone, Copy)]
pub(super) enum PageStep {
    Up,
    Down,
    Top,
    Bottom,
}

/// Scroll the Settings page the least that brings the span from window y
/// `top` to `bottom`, measured at the page's current offset, inside the
/// page's viewport, as the Your themes list's own Nearest reveal does inside
/// the list: nothing while it fits, else its top onto the viewport's top or
/// its bottom onto the viewport's bottom, the top when it is taller than the
/// viewport. The viewport is the page's from the last frame it was
/// prepainted in, which a focus change does not move; GPUI clamps the offset
/// to the page's reach.
fn reveal_in_page(page: &ScrollHandle, top: Pixels, bottom: Pixels) {
    let viewport = page.bounds();
    if viewport.size.height <= Pixels::ZERO {
        return;
    }
    let delta = if top < viewport.top() || bottom - top > viewport.size.height {
        viewport.top() - top
    } else if bottom > viewport.bottom() {
        viewport.bottom() - bottom
    } else {
        return;
    };
    let offset = page.offset();
    page.set_offset(point(offset.x, offset.y + delta));
}

/// The Your themes rows' corner radius.
const ROW_RADIUS: Pixels = px(6.);

/// Whether the Your themes row on `hitbox` shows its hover fill in the frame
/// being painted, decided as GPUI decides an element's hover style: the
/// pointer is over the row, no drag is active, and the last input was not a
/// touch, which leaves the pointer where it was and so would otherwise keep
/// the row hovered. GPUI keeps the last input's modality to itself
/// (`Window::last_input_was_touch` is crate-private), so the row asks GPUI's
/// own style computation, `Interactivity::compute_style`, for a hover style
/// that shows a probe: the answer follows every modality rule GPUI applies to
/// hover styles, now and after an upgrade.
fn row_hovered(hitbox: &Hitbox, window: &mut Window, cx: &mut App) -> bool {
    let mut probe = div().invisible().hover(|style| style.visible());
    probe
        .interactivity()
        .compute_style(None, Some(hitbox), window, cx)
        .visibility
        == gpui::Visibility::Visible
}

/// Where a Your themes row is drawn in the frame being prepainted, for the
/// fill the list paints beneath the rows: the row's bounds, its content mask,
/// a hitbox of the row's own bounds, which is hovered exactly when the row
/// is, and whether the row is the just-imported one.
struct RowFill {
    bounds: Bounds<Pixels>,
    mask: ContentMask<Pixels>,
    hitbox: Hitbox,
    imported: bool,
}

/// Settings-only views and text fields, created lazily on first entry to
/// Settings so cold startup into a repository allocates none of them.
pub(super) struct SettingsUi {
    pub(super) page: Entity<SettingsPage>,
    pub(super) branch: Entity<InputState>,
    pub(super) editor: Entity<InputState>,
    pub(super) terminal: Entity<InputState>,
    pub(super) identity_name: Entity<InputState>,
    pub(super) identity_email: Entity<InputState>,
}

/// Baselines distinguish a saved value moving externally from an unfinished edit.
pub(super) struct DraftState {
    branch: String,
    /// The repository Name and Email hold text for, and the identity they
    /// were last filled with there: `None` while they wait for one to be
    /// read ([`GitTurtle::loaded_identity`]), empty with their placeholders
    /// but for what the person typed for that repository
    /// ([`GitTurtle::sync_identity_fields`]).
    identity: Option<(PathBuf, Option<(String, String)>)>,
    /// The settings whose save last failed, in use and not on disk, until a
    /// save of the settings in use succeeds: Return in Default branch saves
    /// them again, as Retry save does (`GitTurtle::save_default_branch`),
    /// whatever the window's error line says by then. Read from that
    /// line's text, the failure was forgotten once another operation's
    /// error replaced it, and Return in the field saved nothing.
    failed_save: Option<AppSettings>,
    /// Coalesces rapid setting toggles onto one queued preference-writer job
    /// instead of overflowing the 8-slot writer queue with Busy errors.
    saver: crate::commit_drafts::CoalescingSaver<(), AppSettings>,
    /// Monotonic generation incremented each time a new settings-save batch is
    /// spawned, so stale `Ok` or `Err` replies never overwrite a newer batch.
    save_generation: u64,
    retry: Option<Task<()>>,
}

impl DraftState {
    pub(super) fn new(branch: String) -> Self {
        Self {
            branch,
            identity: None,
            failed_save: None,
            saver: crate::commit_drafts::CoalescingSaver::default(),
            save_generation: 0,
            retry: None,
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
        if let Some((scope, Some(baseline))) = &self.identity
            && scope == path
            && edited != baseline
            && edited != effective
        {
            return None;
        }
        self.identity = Some((path.to_owned(), Some(effective.clone())));
        (edited != effective).then(|| effective.clone())
    }
    fn reset_identity(&mut self, path: Option<&Path>, effective: (String, String)) {
        self.identity = path.map(|path| (path.to_owned(), Some(effective)));
    }
    /// Whether `value`, the text of field `index` (Name, then Email), is
    /// one read from the repository rather than typed: the identity the
    /// window holds, `read`, or the one the fields were last filled with,
    /// `baseline`, which an unsaved edit of the other field keeps.
    fn was_read(
        baseline: &(String, String),
        read: Option<&(String, String)>,
        index: usize,
        value: &str,
    ) -> bool {
        let holds =
            |(name, email): &(String, String)| *value == *if index == 0 { name } else { email };
        read.is_some_and(holds) || holds(baseline)
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
                if this.page == AppPage::Settings {
                    this.ensure_settings_ui(window, cx);
                    this.ensure_theme_cards(cx);
                }
                this.sync_settings_drafts(window, cx);
                this.reconcile_settings_page(cx);
                if this.page == AppPage::Settings {
                    this.sync_theme_cards(cx);
                }
                // Every page change notifies the app: the desktop's Omarchy
                // theme is watched while Settings can show its card.
                #[cfg(target_os = "linux")]
                appearance::omarchy::show(this.page == AppPage::Settings, cx);
            }));
        // A change in one of the page's inputs reaches the page through its
        // own observation of the input (`SettingsPage::new`); the app has
        // nothing outside the page that shows an unsaved value. Return in a
        // field saves from the page's key-down listener
        // (`Self::settings_field_enter`).
    }

    /// Lazily constructs the Settings page view and its four text inputs on
    /// first entry to Settings.
    pub(super) fn ensure_settings_ui(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> &SettingsUi {
        if self.settings_ui.is_none() {
            let identity_name = cx.new(|cx| InputState::new(window, cx).placeholder("Your name"));
            let identity_email =
                cx.new(|cx| InputState::new(window, cx).placeholder("you@example.com"));
            let default_branch = self.settings.default_branch.clone();
            let branch = cx.new(|cx| InputState::new(window, cx).default_value(default_branch));
            let external_editor = self.settings.external_editor.clone();
            let editor = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(external_editor)
                    .placeholder(if cfg!(target_os = "macos") {
                        "Visual Studio Code"
                    } else {
                        "code"
                    })
            });
            let external_terminal = self.settings.external_terminal.clone();
            let terminal = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(external_terminal)
                    .placeholder(if cfg!(target_os = "macos") {
                        "Terminal"
                    } else {
                        "ghostty"
                    })
            });
            let owner = cx.entity().downgrade();
            let page = cx.new(|cx| {
                SettingsPage::new(
                    owner,
                    [&branch, &editor, &terminal, &identity_name, &identity_email],
                    cx,
                )
            });
            self.settings_ui = Some(SettingsUi {
                page,
                branch,
                editor,
                terminal,
                identity_name,
                identity_email,
            });
        }
        self.settings_ui
            .as_ref()
            .expect("settings_ui was just initialized")
    }

    /// Lazily constructs the retained theme picker miniatures, card bodies, and
    /// card focus handles on first entry to Settings.
    pub(super) fn ensure_theme_cards(&mut self, cx: &mut Context<Self>) {
        if self.theme_cards_ready {
            return;
        }
        self.theme_previews = ThemeChoice::ALL
            .into_iter()
            .map(|choice| (ThemeSelection::BuiltIn(choice), choice.palette()))
            .chain(
                self.custom_themes
                    .iter()
                    .map(|theme| (ThemeSelection::Custom(theme.id), theme.palette)),
            )
            .map(|(selection, palette)| (selection, cx.new(|_| ThemePreviewBody::new(palette))))
            .collect();
        self.theme_card_bodies = self
            .theme_previews
            .iter()
            .map(|(selection, miniature)| {
                let miniature = miniature.entity_id();
                (*selection, cx.new(|_| ThemeCardBody::new(miniature)))
            })
            .collect();
        self.theme_card_focus = ThemeChoice::ALL
            .into_iter()
            .map(ThemeSelection::BuiltIn)
            .chain(
                self.custom_themes
                    .iter()
                    .map(|theme| ThemeSelection::Custom(theme.id)),
            )
            .map(|selection| (selection, cx.focus_handle()))
            .collect();
        self.theme_cards_ready = true;
        #[cfg(target_os = "linux")]
        {
            let _ = self.sync_omarchy_card(cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn settings_page_for_test(&self) -> Entity<SettingsPage> {
        self.settings_ui
            .as_ref()
            .expect("settings UI initialized before accessing settings_page")
            .page
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn identity_name_for_test(&self) -> Entity<InputState> {
        self.settings_ui
            .as_ref()
            .expect("settings UI initialized before accessing identity_name")
            .identity_name
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn identity_email_for_test(&self) -> Entity<InputState> {
        self.settings_ui
            .as_ref()
            .expect("settings UI initialized before accessing identity_email")
            .identity_email
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn settings_branch_for_test(&self) -> Entity<InputState> {
        self.settings_ui
            .as_ref()
            .expect("settings UI initialized before accessing settings_branch")
            .branch
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn settings_editor_for_test(&self) -> Entity<InputState> {
        self.settings_ui
            .as_ref()
            .expect("settings UI initialized before accessing settings_editor")
            .editor
            .clone()
    }

    /// Return in the Default branch field saves it, and in Name or Email the
    /// repository's Git identity, once per press: a plain Enter (Shift-Enter
    /// too) going down, not marked held, not repeating the press the toolkit
    /// recorded (`enter_press_is_repeat`), in the field focused as that press
    /// began (`enter_press_began_on`), the conditions a dialog's default
    /// action keeps and the project hub's fields keep. The page's key-down
    /// listener asks, after the field's own Enter has let the key go on. The
    /// fields saved on their own `PressEnter`, which the field sends for
    /// every Enter key-down, a held key's repeats included on every platform,
    /// so a held Return after an identity write that failed at once began
    /// another Git configuration write on each repeat. The secondary Enter
    /// (Command-Enter on macOS, Control-Enter elsewhere) saves nothing: on
    /// macOS it records no press, since Cocoa sends no key-up for a key
    /// pressed with Command held, so its repeats could not be told from
    /// presses. Neither does an Alt- or Fn-Enter.
    fn settings_field_enter(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = &event.keystroke.modifiers;
        if event.keystroke.key != "enter"
            || event.is_held
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
        {
            return;
        }
        let Some(ui) = &self.settings_ui else {
            return;
        };
        let Some((index, field)) = [&ui.branch, &ui.identity_name, &ui.identity_email]
            .into_iter()
            .map(|field| field.read(cx).focus_handle(cx))
            .enumerate()
            .find(|(_, field)| field.is_focused(window))
        else {
            return;
        };
        if gpui_kit::base::enter_press_is_repeat(window, cx)
            || !gpui_kit::base::enter_press_began_on(&field, window, cx)
        {
            return;
        }
        if index == 0 {
            self.save_default_branch(window, cx);
        } else {
            self.save_identity(window, cx);
        }
    }

    /// The page for `GitTurtle::render`: the [`SettingsPage`] view, cached so
    /// a frame that alters nothing on it replays its layout and paint. GPUI
    /// collects accessibility nodes only while an element is prepainted, so
    /// while assistive technology is reading the window the page is embedded
    /// uncached and built every frame.
    pub(super) fn settings_page_element(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_theme_cards(cx);
        let ui = self.ensure_settings_ui(window, cx);
        let page = if window.is_a11y_active() {
            ui.page.clone().into_any_element()
        } else {
            ui.page
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element()
        };
        let anchors = [
            ui.editor.entity_id(),
            ui.terminal.entity_id(),
            ui.branch.entity_id(),
            ui.identity_name.entity_id(),
            ui.identity_email.entity_id(),
        ];
        div()
            .size_full()
            .child(page)
            .children(anchors.map(ViewNodeAnchor::new))
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
        if let Some(ui) = &self.settings_ui {
            App::notify(cx, ui.page.entity_id());
        }
    }

    /// Page Up and Page Down move the Settings page by most of its viewport,
    /// as they move the commit message; Home and End go to its top and
    /// bottom. On another page the key goes on to the focused view.
    pub(super) fn scroll_settings_page(&self, step: PageStep, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings {
            cx.propagate();
            return;
        }
        let page = self.theme_editor.page_scroll();
        let offset = page.offset();
        let bottom = -page.max_offset().y;
        let y = match step {
            PageStep::Up => offset.y + page.bounds().size.height * 0.85,
            PageStep::Down => offset.y - page.bounds().size.height * 0.85,
            PageStep::Top => Pixels::ZERO,
            PageStep::Bottom => bottom,
        };
        page.set_offset(point(offset.x, y.max(bottom).min(Pixels::ZERO)));
        self.notify_settings_page(cx);
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
            looks_busy: self.operation_shown().is_some(),
            status: self.status.clone(),
            profile: self.profile.clone(),
            identity_loaded: self.loaded_identity().is_some(),
            identity_wait: self.identity_wait(),
            repository: self.repository.as_ref().map(|repository| {
                let path = repository.path();
                (path.to_owned(), self.project_name(path))
            }),
            marked: self.shown_tab_unavailable(),
        }
    }

    fn reconcile_settings_page(&self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings {
            return;
        }
        let Some(ui) = &self.settings_ui else {
            return;
        };
        let key = self.settings_page_key();
        if ui
            .page
            .read(cx)
            .seen
            .as_ref()
            .is_some_and(|seen| *seen != key)
        {
            self.notify_settings_page(cx);
        }
    }

    pub(super) fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_settings_ui(window, cx);
        self.ensure_theme_cards(cx);
        if self.page != AppPage::Settings {
            self.page_origin = self.page;
            self.theme_editor.settings_entered();
        }
        self.capture_page_return_focus(window, cx);
        self.page = AppPage::Settings;
        // Cancels every review being prepared, and runs the read Changes
        // asked for meanwhile, which their ends would have run: clicking the
        // tab back, which reads nothing, once left Refresh unrun.
        self.end_preparations(page_navigation::PageSwitch::Away, window, cx);
        self.column_menu = false;
        self.column_drag = None;
        self.image_drag = None;
        self.end_image_drag();
        self.sync_settings_drafts(window, cx);
        // Focus opens on the page's first control, inside the page so the
        // application's keys still reach the window's handlers, with its
        // ring drawn and the page scrolled to show it, as a key moving focus
        // there would, whether a key or a click opened the page.
        let first = self.settings_first_control(cx);
        window.focus(&first, cx);
        self.focus_ring.mark(window);
        self.theme_editor.page().reveal_next_focus();
        self.repaint_page(window, cx);
    }

    /// The control Settings opens with focus on, its first: Follow system
    /// appearance, or, while the desktop's Omarchy theme decides the
    /// appearance and that switch is unavailable, the selected theme's card.
    /// The page opened with focus on the window's root, which draws nothing,
    /// so nothing showed where the keyboard stood until a Tab landed on the
    /// tab strip.
    fn settings_first_control(&self, cx: &mut App) -> FocusHandle {
        if !self.follows_desktop_theme(cx) {
            return self.page_focus("follow-system", cx);
        }
        let selected = self.settings.theme.on_desktop(cx);
        self.theme_card_focus
            .iter()
            .find_map(|(drawn, focus)| (*drawn == selected).then(|| focus.clone()))
            .unwrap_or_else(|| self.app_focus.clone())
    }

    pub(super) fn capture_page_return_focus(&mut self, window: &Window, cx: &App) {
        if self.page == AppPage::Repository {
            pdf_view::pause(self.content.as_deref());
            model_view::pause(self.content.as_deref());
            markdown_view::pause(self.content.as_deref());
            self.page_return_focus = native_accessibility::pointer_menu_return(window, cx)
                .unwrap_or_else(|| window.focused(cx));
        }
    }

    /// The Git identity Settings has loaded, the one answer its fields'
    /// fill, Save repository identity, Reset edits, their handlers, the
    /// identity line, the signing line and the note all ask: the identity
    /// read for the repository the tab now shows, and since its mark where
    /// it was marked ([`Self::read_since_the_mark`]). The window's identity
    /// goes as another repository is shown (`forget_repository_state`, a
    /// warm tab's own restored), so one held is the shown repository's. Until
    /// such a read lands Settings has none (decided 2026-11-15: a write acts
    /// only on the captured repository and on values read from it, and
    /// nothing from before the mark): on a cold tab until its status read
    /// lands, for good where that read fails, on a marked tab, after a
    /// Refresh's snapshot has cleared the mark in place, and in a tab left
    /// meanwhile and come back to. Each asked the window's identity alone,
    /// or nothing: Save, offered whenever none was held, wrote the Name and
    /// Email another repository's read had left in the fields into the one
    /// shown, and after a mark the ones read before it.
    pub(super) fn loaded_identity(&self) -> Option<&gitturtle_core::GitProfile> {
        (self.profile.as_ref()).filter(|_| self.repository.is_some() && self.read_since_the_mark())
    }

    /// Why Save repository identity and Reset edits are refused where no
    /// identity is loaded ([`Self::loaded_identity`]), in the words the
    /// bar's Fetch, Pull and Push give for the same two states of the same
    /// read: "Reading the working tree…" while the working state, which
    /// brings the identity, is read or waits to be
    /// (`working_read_pending`), "Working tree not read · Refresh" where
    /// none is, as after a read that failed. `None` where one is loaded,
    /// with no repository shown, and on a marked tab, whose own words win.
    fn identity_wait(&self) -> Option<&'static str> {
        if self.repository.is_none()
            || self.shown_tab_unavailable()
            || self.loaded_identity().is_some()
        {
            return None;
        }
        Some(if self.working_read_pending() {
            crate::workspace::READING_WORKING_TREE
        } else {
            crate::workspace::NOT_READ_REASON
        })
    }

    /// Reset edits: Name and Email take the loaded identity again, which
    /// becomes what they were last filled with. Refused while a write runs
    /// and where no identity is loaded ([`Self::loaded_identity`]), as its
    /// Button is, so a press that reaches it there restores nothing: it
    /// put the identity read before the mark back into both fields, to be
    /// kept as an edit and saved.
    pub(super) fn reset_identity_edits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() {
            return;
        }
        let Some(path) = self
            .repository
            .as_ref()
            .map(|repository| repository.path().to_owned())
        else {
            return;
        };
        let Some(read) = self
            .loaded_identity()
            .map(|profile| (profile.name.clone(), profile.email.clone()))
        else {
            return;
        };
        let Some(ui) = &self.settings_ui else {
            return;
        };
        let inputs = [ui.identity_name.clone(), ui.identity_email.clone()];
        for (input, value) in inputs.iter().zip([&read.0, &read.1]) {
            input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        }
        self.settings_drafts.reset_identity(Some(&path), read);
    }

    fn sync_settings_drafts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // On every page: a mark comes and goes behind any of them, and what
        // a read left in the fields goes with it.
        self.note_the_mark();
        self.withhold_identity_fields(window, cx);
        if self.page != AppPage::Settings {
            return;
        }
        let branch = self.ensure_settings_ui(window, cx).branch.clone();
        if let Some(value) = self.settings_drafts.branch_update(
            &self.settings.default_branch,
            branch.read(cx).value().as_ref(),
        ) {
            branch.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.sync_identity_fields(window, cx);
    }

    /// With no identity loaded for the repository Name and Email hold text
    /// for ([`Self::loaded_identity`]), empties each holding a value read
    /// from it (`DraftState::was_read`), to wait with its placeholder for
    /// the read that loads one, while text the person typed and has not
    /// saved is theirs and stays as typed (decided 2026-11-14). On every
    /// page, as the app notifies: a field's text is the person's only if
    /// they typed it, whichever page shows as the mark or the read lands.
    /// Done only while Settings showed, a Name typed there and left unsaved
    /// kept the Email read before a mark that came and went behind History,
    /// both taken for one unfinished edit as the read after the mark landed,
    /// and Save wrote that Email over the one configured since (the code
    /// re-check of FU44-D11).
    fn withhold_identity_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ui) = &self.settings_ui else {
            return;
        };
        let fields = [ui.identity_name.clone(), ui.identity_email.clone()];
        let Some(path) = self
            .workspace
            .repository
            .as_ref()
            .map(|repository| repository.path())
        else {
            return;
        };
        if self.loaded_identity().is_some() {
            return;
        }
        let Some(baseline) = (self.settings_drafts.identity.as_mut())
            .filter(|(scope, _)| scope.as_path() == path)
            .and_then(|(_, filled)| filled.take())
        else {
            return;
        };
        // What the window holds, where it holds one, is not loaded: read
        // before the mark, as the baseline was.
        let read = self
            .profile
            .as_ref()
            .map(|profile| (profile.name.clone(), profile.email.clone()));
        for (index, field) in fields.iter().enumerate() {
            let value = field.read(cx).value();
            if !value.is_empty() && DraftState::was_read(&baseline, read.as_ref(), index, &value) {
                field.update(cx, |input, cx| input.set_value("", window, cx));
            }
        }
    }

    /// Brings Name and Email to the repository shown and the identity loaded
    /// for it ([`Self::loaded_identity`]), as Settings is shown, as the app
    /// notifies while it is, and before Save reads them:
    ///
    /// - Holding text for another repository, read or typed, both take the
    ///   loaded identity, or wait empty: nothing of another repository
    ///   follows to this one. Left as they were while none was loaded, they
    ///   kept the other's Name and Email for Save to write here, and as the
    ///   tab was marked they were taken for the person's typing and kept
    ///   through the mark (the security re-check of FU44-D10, X1 and X2).
    /// - With none loaded, each holding a value read from the repository
    ///   waits empty ([`Self::withhold_identity_fields`]).
    /// - As one is loaded after such a wait, each still empty takes what it
    ///   found, which is what the fields were last filled with, so typing
    ///   meanwhile stays an edit.
    /// - Loaded before and now, they follow the read unless they hold an
    ///   unfinished edit (`DraftState::identity_update`).
    fn sync_identity_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.withhold_identity_fields(window, cx);
        let Some(ui) = &self.settings_ui else {
            return;
        };
        let fields = [ui.identity_name.clone(), ui.identity_email.clone()];
        let Some(path) = self
            .workspace
            .repository
            .as_ref()
            .map(|repository| repository.path())
        else {
            return;
        };
        let loaded = self
            .loaded_identity()
            .map(|profile| (profile.name.clone(), profile.email.clone()));
        // A field given the value it holds would only draw the window again.
        let set = |index: usize, value: &str, window: &mut Window, cx: &mut Context<Self>| {
            if fields[index].read(cx).value().as_ref() != value {
                fields[index].update(cx, |input, cx| {
                    input.set_value(value.to_owned(), window, cx)
                });
            }
        };
        let filled = match &mut self.settings_drafts.identity {
            Some((scope, filled)) if scope.as_path() == path => filled,
            held => {
                let (name, email) = loaded.clone().unwrap_or_default();
                set(0, &name, window, cx);
                set(1, &email, window, cx);
                *held = Some((path.to_owned(), loaded));
                return;
            }
        };
        match (filled.as_mut(), loaded) {
            // Emptied already of what a read left, and waiting.
            (_, None) => {}
            (None, Some(read)) => {
                for (index, value) in [&read.0, &read.1].into_iter().enumerate() {
                    if fields[index].read(cx).value().is_empty() {
                        set(index, value, window, cx);
                    }
                }
                *filled = Some(read);
            }
            (Some(_), Some(read)) => {
                let edited = (
                    fields[0].read(cx).value().to_string(),
                    fields[1].read(cx).value().to_string(),
                );
                if let Some((name, email)) =
                    self.settings_drafts.identity_update(path, &read, &edited)
                {
                    set(0, &name, window, cx);
                    set(1, &email, window, cx);
                }
            }
        }
    }

    /// Whether Settings' Name or Email holds keyboard focus.
    pub(super) fn identity_field_focused(&self, window: &Window, cx: &App) -> bool {
        let Some(ui) = &self.settings_ui else {
            return false;
        };
        [&ui.identity_name, &ui.identity_email]
            .into_iter()
            .any(|field| field.focus_handle(cx).is_focused(window))
    }

    /// This method never replaces current UI settings with an older disk reply.
    /// The dedicated executor serializes these saves with recent-project writes,
    /// coalescing rapid toggles onto the latest settings state.
    pub(super) fn save_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.normalize();
        if let Err(error) = self.settings.validate() {
            self.operation_error = Some(format!("Could not save settings: {error:#}").into());
            self.settings_drafts.failed_save = Some(self.settings.clone());
            cx.notify();
            return;
        }
        let submitted = self.settings.clone();
        let response = self.settings_drafts.saver.queue_with(
            &self.preferences_writer,
            (),
            submitted,
            |pending| {
                let Some(settings) = pending.get(&()) else {
                    return Ok(());
                };
                Preferences::save_settings(settings).map(|_| ())
            },
        );
        if let Some(response) = response {
            self.settings_drafts.save_generation =
                self.settings_drafts.save_generation.wrapping_add(1);
            let generation = self.settings_drafts.save_generation;
            cx.spawn_in(window, async move |this, cx| {
                let result = response.await.unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "The settings writer stopped before reporting a result"
                    ))
                });
                let _ =
                    this.update_in(cx, |this, window, cx| {
                        if this.settings_drafts.save_generation != generation {
                            return;
                        }
                        if let Err(ref error) = result
                            && error.is::<operations::Busy>()
                        {
                            this.ask_again_to_save_preferences(window, cx);
                            return;
                        }
                        let error_before = this.operation_error.clone();
                        let status_before = this.status.clone();
                        match result {
                            Ok(_) => {
                                // Only suppress stale success notices when a newer
                                // save is still pending; failed saves stay pending in
                                // CoalescingSaver for retry and must still report error.
                                if this.settings_drafts.saver.is_pending() {
                                    return;
                                }
                                if this.operation_error.as_ref().is_some_and(|error| {
                                    error.starts_with("Could not save settings:")
                                }) {
                                    this.operation_error = None;
                                }
                                this.settings_drafts.failed_save = None;
                                this.status = "Settings saved".into();
                            }
                            Err(error) => {
                                this.operation_error =
                                    Some(format!("Could not save settings: {error:#}").into());
                                this.settings_drafts.failed_save = Some(this.settings.clone());
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
        }
        cx.notify();
        self.notify_settings_page(cx);
    }

    fn ask_again_to_save_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_drafts.retry.is_some() {
            return;
        }
        let retry = self.retry_refused_save(window, cx, |this, window, cx| {
            this.settings_drafts.retry = None;
            if this.settings_drafts.saver.is_pending() {
                this.save_preferences(window, cx);
            }
        });
        self.settings_drafts.retry = Some(retry);
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

    /// Settings' Reduce motion or Increase contrast: the title and the
    /// description of the current choice at the row's left edge, the three
    /// choices at its trailing edge, built as List density's segments and
    /// stacked below the title in a `narrow` page. The description says what
    /// the desktop asks while following it, and the row keeps the height of
    /// its tallest description, so a choice moves nothing.
    fn render_display_setting(
        &self,
        setting: DisplaySetting,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let choice = setting.choice(&self.settings);
        let asks = setting.asks(native_accessibility::display_preferences(cx));
        let reserve = setting.descriptions().map(SharedString::from);
        let key = setting.key();
        let focus: Vec<_> = (0..DisplayChoice::ALL.len())
            .map(|index| self.page_focus((key, index), cx))
            .collect();
        // A focused segment shows its whole row, the description a choice
        // changes with it.
        let (row_in_view, segments_drawn) = self.keep_row_in_view(&focus, cx);
        let segments = div()
            .debug_selector(move || format!("{key}-segments"))
            .on_children_prepainted(segments_drawn)
            .flex()
            .flex_shrink_0()
            .gap_1()
            .p_1()
            .rounded(px(8.))
            .bg(rgb(p.canvas))
            .children(
                DisplayChoice::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, option)| {
                        display_segment(setting, index, option == choice, &focus[index]).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.choose_display(setting, option, window, cx)
                            }),
                        )
                    }),
            );
        div()
            .on_children_prepainted(row_in_view)
            .child(
                div()
                    .debug_selector(move || format!("{key}-row"))
                    .flex()
                    // Every row's trailing control, these segments as List
                    // density's, the text sizes', each switch's track and
                    // Your themes' actions, is centred on the title's first
                    // line (`on_title_line`), where it stays as the
                    // description rewraps; centred on the row, it drifted
                    // down with each wrapped line.
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .when(narrow, |row| row.flex_col())
                    .pt_4()
                    .border_t_1()
                    .border_color(rgb(p.border))
                    // Stacked, the text takes the row's width, so its longest
                    // description wraps inside the card rather than widening it.
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .map(|text| if narrow { text.w_full() } else { text.flex_1() })
                            .child(read_setting_text(
                                setting.title(),
                                &setting.description(choice, asks).into(),
                                &reserve,
                                cx,
                            )),
                    )
                    // Below the separator, the row's top padding is the
                    // room above its title.
                    .child(if narrow {
                        segments.into_any_element()
                    } else {
                        on_title_line(segments, rem(), cx).into_any_element()
                    }),
            )
            .into_any_element()
    }

    /// A Reduce motion or Increase contrast segment pressed: the choice is
    /// kept, resolved with the desktop's answer and shown at once (a change
    /// of More contrast applies the theme again, traced as
    /// `gitturtle.contrast_apply_frame_ms` from here to the next frame), and
    /// saved.
    fn choose_display(
        &mut self,
        setting: DisplaySetting,
        choice: DisplayChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = trace_enabled().then(std::time::Instant::now);
        let current = match setting {
            DisplaySetting::ReduceMotion => &mut self.settings.reduce_motion,
            DisplaySetting::IncreaseContrast => &mut self.settings.increase_contrast,
        };
        if *current == choice {
            return;
        }
        *current = choice;
        native_accessibility::set_display_choices(&self.settings, cx);
        native_accessibility::sync_preferences(cx);
        self.show_display_preferences(started, window, cx);
        self.notify_settings_page(cx);
        cx.notify();
        self.save_preferences(window, cx);
    }

    /// Linux: code in the desktop's fontconfig `monospace` family, used only
    /// when the toolkit loaded it and it draws at one advance.
    fn render_code_font_setting(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = cx;
            None
        }
        #[cfg(target_os = "linux")]
        {
            use crate::desktop_text::{self, SystemCodeFont};
            let bundled = crate::mono();
            let focus = self.page_focus("settings-system-code-font", cx);
            let off: SharedString = format!(
                "Code uses the bundled {bundled}. Turn on to use fontconfig's monospace font when it is fixed-width."
            )
            .into();
            let pending: SharedString = "Looking up the desktop's monospace font…".into();
            let description = match desktop_text::system_code_font(cx) {
                SystemCodeFont::Off => off.clone(),
                SystemCodeFont::Pending => pending.clone(),
                SystemCodeFont::Loaded(family) => {
                    format!("Code uses {family}, the desktop's monospace font.").into()
                }
                SystemCodeFont::Unavailable(reason) => {
                    format!("{reason} Code uses the bundled {bundled}.").into()
                }
            };
            Some(
                div()
                    .debug_selector(|| "code-font-setting".into())
                    .child(
                        div()
                            .on_children_prepainted(
                                self.keep_in_view(vec![Some(focus.clone())], cx),
                            )
                            .debug_selector(|| "code-font-switch".into())
                            .child(
                                // Toggling shows the pending line, then the
                                // answer, and turning off shows the off line:
                                // the row keeps the height of the tallest, so
                                // neither the Switch nor the rows below move
                                // when one replaces another.
                                switch_row(
                                    "settings-system-code-font",
                                    "Use the desktop's monospace font",
                                    "Use the desktop's monospace font for code",
                                    description,
                                    &[off, pending],
                                    cx,
                                )
                                .track_focus(&focus)
                                .checked(self.settings.system_code_font)
                                .on_click(cx.listener(
                                    |this, checked: &bool, window, cx| {
                                        if this.settings.system_code_font != *checked {
                                            this.settings.system_code_font = *checked;
                                            desktop_text::set_system_code_font(*checked, cx);
                                            this.save_preferences(window, cx);
                                        }
                                    },
                                )),
                            ),
                    )
                    .into_any_element(),
            )
        }
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
        let index = usize::from(code);
        let decrease = self.page_focus(("text-size-decrease", index), cx);
        let increase = self.page_focus(("text-size-increase", index), cx);
        let reset = self.page_focus(("text-size-reset", index), cx);
        div()
            .debug_selector(move || {
                format!(
                    "text-size-setting-{}",
                    if code { "code" } else { "interface" }
                )
            })
            .flex()
            .flex_col()
            .gap_3()
            // Rows are spaced more than a rem apart. Too narrow for the
            // controls beside the text, the row puts them on a line of their
            // own below it.
            .child(
                wrapping_row(
                    div().debug_selector(move || {
                        format!("text-size-row-{}", if code { "code" } else { "interface" })
                    }),
                    setting_description(label, explanation, cx),
                    div()
                        .debug_selector(move || {
                            format!(
                                "text-size-controls-{}",
                                if code { "code" } else { "interface" }
                            )
                        })
                        .on_children_prepainted(self.keep_in_view(
                            vec![
                                Some(decrease.clone()),
                                None,
                                Some(increase.clone()),
                                Some(reset.clone()),
                            ],
                            cx,
                        ))
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new(("text-size-decrease", usize::from(code)))
                                .track_focus(&decrease)
                                .debug_selector(move || {
                                    format!(
                                        "text-size-decrease-{}",
                                        if code { "code" } else { "interface" }
                                    )
                                })
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
                            native_accessibility::static_text(
                                div().id(("text-size-value", usize::from(code))),
                                format!("{label}, {value} points"),
                            )
                            .min_w(appearance::ui_size(42.))
                            .text_center()
                            .text_size(appearance::ui_text(12.))
                            .child(format!("{value} pt")),
                        )
                        .child(
                            Button::new(("text-size-increase", usize::from(code)))
                                .track_focus(&increase)
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
                                .track_focus(&reset)
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
                    // The kit's small Buttons' height (`h_6`), which the value
                    // between them does not exceed.
                    rem() * 1.5,
                    rem(),
                    cx,
                )
                // The small ghost −'s frame pads its label by the small Button's
                // 0.5 rem (`px_2`): wrapped, its label stands on the title's edge.
                .ghost_lead(rem() * 0.5),
            )
            .child(
                div()
                    .w_full()
                    .p_3()
                    .rounded(px(6.))
                    .bg(rgb(p.canvas))
                    .text_color(rgb(p.text))
                    .when(code, |el| {
                        el.code_font(cx).text_size(appearance::code_text())
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
        if !self.theme_cards_ready {
            self.custom_themes = themes;
            self.notify_settings_page(cx);
            return;
        }
        let kept = |drawn: &ThemeSelection| match drawn {
            ThemeSelection::BuiltIn(_) | ThemeSelection::Omarchy => true,
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
        self.notify_settings_page(cx);
    }

    /// Linux: keep the Omarchy card's miniature, body and focus handle in step
    /// with what [`appearance::omarchy`] read, creating them the first time
    /// there is anything to show. Called when that state changes and when the
    /// window opens; returns the card.
    #[cfg(target_os = "linux")]
    fn sync_omarchy_card(&mut self, cx: &mut Context<Self>) -> Option<appearance::omarchy::Card> {
        let card = appearance::omarchy::card(cx)?;
        if !self.theme_cards_ready {
            return Some(card);
        }
        let selection = ThemeSelection::Omarchy;
        let retained = self
            .theme_previews
            .iter()
            .find_map(|(drawn, body)| (*drawn == selection).then(|| body.clone()));
        match retained {
            Some(body) => body.update(cx, |body, cx| body.set_palette(card.palette, cx)),
            None => {
                let body = cx.new(|_| ThemePreviewBody::new(card.palette));
                let miniature = body.entity_id();
                self.theme_previews.push((selection, body));
                self.theme_card_bodies
                    .push((selection, cx.new(|_| ThemeCardBody::new(miniature))));
                self.theme_card_focus.push((selection, cx.focus_handle()));
            }
        }
        Some(card)
    }

    /// Linux: the desktop's Omarchy theme was read again and something it
    /// shows changed. A selected Omarchy theme whose palette moved is applied
    /// through the one application path (not while the theme editor previews
    /// a draft, whose close applies the saved selection); anything else only
    /// redraws the picker card. `gitturtle.omarchy_apply_frame_ms` measures an
    /// application from the read's arrival to the next frame callback, except
    /// the window's first take (deferred from `GitTurtle::new`): that applies
    /// what launch read before the window existed, before its first frame is
    /// requested, and is no switch.
    #[cfg(target_os = "linux")]
    pub(super) fn omarchy_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(revision) = appearance::omarchy::card(cx).map(|card| card.revision) else {
            return;
        };
        if self.omarchy_revision == Some(revision) {
            return;
        }
        let first = self.omarchy_revision.is_none();
        self.omarchy_revision = Some(revision);
        let Some(card) = self.sync_omarchy_card(cx) else {
            return;
        };
        // A choice of the card waiting for its read applies what it finds
        // (`choose_omarchy`), so nothing applies here meanwhile. The palette
        // is compared as it would be applied: with More contrast on, the
        // applied global is the adjusted one, so the raw palette always
        // differed and every revision applied it again.
        if self.settings.theme == ThemeSelection::Omarchy
            && self.omarchy_choice.waiting.is_none()
            && self.theme_editor.preview().is_none()
            && self.effective_theme(cx).palette.as_applied(cx) != palette(cx)
        {
            self.apply_appearance(window, cx);
            if let (true, false, Some(arrived)) = (trace_enabled(), first, card.arrived) {
                Self::trace_next_frame("omarchy_apply_frame_ms", arrived, window);
            }
        } else {
            self.notify_settings_page(cx);
            cx.notify();
        }
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
    /// built-in and a custom selection alike; for the Omarchy card, through
    /// the frame after the application its read decides (`choose_omarchy`).
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
        #[cfg(target_os = "linux")]
        {
            self.omarchy_choice.waiting = None;
            if theme == ThemeSelection::Omarchy && self.choose_omarchy(started, window, cx) {
                self.save_preferences(window, cx);
                return;
            }
        }
        self.apply_appearance(window, cx);
        self.save_preferences(window, cx);
        if let Some(start) = started {
            Self::trace_next_frame("theme_apply_frame_ms", start, window);
        }
    }

    /// Linux: choosing the Omarchy card reads `current/` first, since the
    /// card may show a theme the desktop has left while nothing watched it,
    /// and applies only what that read finds: exactly one application when
    /// the read lands within [`OMARCHY_CHOICE_WAIT`]. A slower read (one that
    /// joins a switch still settling) lets the cached palette apply at the
    /// wait's end, and its own result then applies through `omarchy_changed`
    /// when it differs. A read that fails applies the fallback the card
    /// shows. `false` without the reader, when the choice applies at once.
    #[cfg(target_os = "linux")]
    fn choose_omarchy(
        &mut self,
        started: Option<std::time::Instant>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(read) = appearance::omarchy::read_now(cx) else {
            return false;
        };
        appearance::omarchy::follow(true, cx);
        self.omarchy_choice.made += 1;
        let choice = self.omarchy_choice.made;
        self.omarchy_choice.waiting = Some((choice, started));
        let wait = cx.background_executor().timer(OMARCHY_CHOICE_WAIT);
        cx.spawn_in(window, async move |this, cx| {
            futures::future::select(read, wait).await;
            this.update_in(cx, |this, window, cx| {
                this.apply_omarchy_choice(choice, window, cx)
            })
            .ok();
        })
        .detach();
        // The card shows as chosen, and Follow system as locked, at once.
        self.notify_settings_page(cx);
        cx.notify();
        true
    }

    /// Apply the Omarchy card's `choice` once its read landed or the wait
    /// ended, unless another choice has replaced it since.
    #[cfg(target_os = "linux")]
    fn apply_omarchy_choice(&mut self, choice: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((waiting, started)) = self.omarchy_choice.waiting else {
            return;
        };
        if waiting != choice {
            return;
        }
        self.omarchy_choice.waiting = None;
        self.apply_appearance(window, cx);
        if let Some(start) = started {
            Self::trace_next_frame("theme_apply_frame_ms", start, window);
        }
    }

    fn save_default_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ui) = &self.settings_ui else {
            return;
        };
        let mut edited = self.settings.clone();
        edited.default_branch = ui.branch.read(cx).value().to_string();
        if let Err(error) = edited.validate() {
            self.operation_error = Some(format!("Default branch: {error:#}").into());
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
        // A save that failed leaves the value it carried in use and on screen,
        // with the failure and Retry save beside it, as every setting's does:
        // a press then saves it again, as Retry save would, until a save
        // succeeds, even once another operation's error has replaced the
        // failure's (`DraftState::failed_save`). A press found nothing new
        // and did nothing, so Return in the field, which still shows the
        // value, was a dead key until another setting changed.
        let save_failed = self.settings_drafts.failed_save.as_ref() == Some(&self.settings);
        if edited == self.settings && !save_failed {
            return;
        }
        if edited != self.settings {
            self.settings = edited;
            self.hub.update(cx, |hub, cx| {
                hub.set_default_branch(self.settings.default_branch.clone(), cx)
            });
        }
        self.save_preferences(window, cx);
    }

    /// Saves the identity fields to the shown repository's Git
    /// configuration, refused on a marked tab, whose repository is the one
    /// read before the mark: where its `.git` came back before a read, the
    /// save wrote `user.name` and `user.email` there (security re-check of
    /// FU44-D3; decided 2026-11-11: an entry is offered only where it can
    /// act), as Apply profile… is refused there. Refused too wherever no
    /// identity is loaded for the repository shown
    /// ([`Self::loaded_identity`]), by Return in a field or a stale press as
    /// by the Button, and the fields are brought to that repository before
    /// they are read ([`Self::sync_identity_fields`]), so what is written
    /// was typed for it or read from it since its mark.
    pub(super) fn save_identity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() {
            return;
        }
        self.sync_identity_fields(window, cx);
        let Some(ui) = &self.settings_ui else {
            return;
        };
        let name = ui.identity_name.read(cx).value().trim().to_owned();
        let email = ui.identity_email.read(cx).value().trim().to_owned();
        let Some(unchanged) = self
            .loaded_identity()
            .map(|loaded| loaded.name == name && loaded.email == email)
        else {
            return;
        };
        if name.is_empty() || email.is_empty() {
            self.operation_error = Some("Enter both your Git name and email address.".into());
            cx.notify();
            return;
        }
        if unchanged {
            return;
        }
        self.write(
            WriteCommand::SetIdentity { name, email },
            "Saving repository identity…",
            window,
            cx,
        );
    }

    /// History's column controls, in Settings (`in_settings`) or History's
    /// own Columns panel. In Settings the keyboard focusing one scrolls the
    /// page to show it.
    pub(super) fn render_columns_controls(
        &self,
        in_settings: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let focus =
            |id: ElementId, cx: &mut Context<Self>| in_settings.then(|| self.page_focus(id, cx));
        // Outside Settings nothing records where the controls lie.
        let keep_in_view = |handles: Vec<Option<FocusHandle>>, cx: &App| {
            in_settings.then(|| self.keep_in_view(handles, cx))
        };
        let (less, more, reset) = (
            focus("graph-spacing-less".into(), cx),
            focus("graph-spacing-more".into(), cx),
            focus("reset-history-columns".into(), cx),
        );
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
            // In Settings the steppers are centred on the title's first line,
            // as every Settings row's trailing control, with the value
            // centred between them; the paragraph above is the gap's 0.75 rem
            // away. History's Columns panel tops them on the row, as
            // before.
            .child(div()
                .debug_selector(|| "graph-spacing-row".into())
                .flex().items_start().flex_wrap().gap_2()
                .child(setting_description("Graph lane spacing", "Distinct lanes keep this spacing; wide histories scroll horizontally.", cx))
                .child({
                let steppers = div()
                    .when_some(keep_in_view(vec![less.clone(), None, more.clone()], cx), |steppers, reveal| {
                        steppers.on_children_prepainted(reveal)
                    })
                    .debug_selector(|| "graph-spacing-controls".into())
                    .flex().items_center().gap_2()
                    .child(button("graph-spacing-less", "−", "", false)
                        .when_some(less.as_ref(), |button, focus| button.track_focus(focus))
                        .accessibility_label("Decrease graph lane spacing").disabled(self.settings.graph_spacing <= 12)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.settings.graph_spacing = this.settings.graph_spacing.saturating_sub(2).max(12);
                            this.save_preferences(window, cx);
                        })))
                    .child(div().min_w(px(38.)).text_center().child(format!("{} px", self.settings.graph_spacing)))
                    .child(button("graph-spacing-more", "+", "", false)
                        .when_some(more.as_ref(), |button, focus| button.track_focus(focus))
                        .accessibility_label("Increase graph lane spacing").disabled(self.settings.graph_spacing >= 32)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.settings.graph_spacing = (this.settings.graph_spacing + 2).min(32);
                            this.save_preferences(window, cx);
                        })));
                if in_settings {
                    on_title_line(steppers, rem() * 0.75, cx).into_any_element()
                } else {
                    steppers.into_any_element()
                }
            }))
            .children(ColumnId::ALL.into_iter().enumerate().map(|(index, id)| {
                let setting = self.settings.columns.get(id);
                let column = focus(("history-column", index).into(), cx);
                div()
                    .debug_selector(move || format!("history-column-row-{index}"))
                    .when_some(keep_in_view(vec![column.clone()], cx), |row, reveal| {
                        row.on_children_prepainted(reveal)
                    })
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_h(crate::appearance::ui_size(32.))
                    .child(
                        column_checkbox(index, id, setting.visible)
                            .when_some(column.as_ref(), |checkbox, focus| checkbox.track_focus(focus))
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
                                ALWAYS_SHOWN.to_owned()
                            } else {
                                format!("{:.0} px", setting.width)
                            }),
                    )
            }))
            .child(
                div()
                    .when_some(keep_in_view(vec![reset.clone()], cx), |row, reveal| {
                        row.on_children_prepainted(reveal)
                    })
                    .pt_2()
                    .child(
                    button("reset-history-columns", "Reset column layout", "refresh", false)
                        .when_some(reset.as_ref(), |button, focus| button.track_focus(focus))
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
    pub(super) fn selected_theme_card(&self, cx: &App) -> Option<ThemeSelection> {
        let selected = self
            .settings
            .theme
            .on_desktop(cx)
            .resolve(&self.custom_themes)
            .selection;
        (!self.settings.follow_system || selected == ThemeSelection::Omarchy).then_some(selected)
    }

    /// Whether the Omarchy theme is the selection this desktop honours
    /// (`ThemeSelection::on_desktop`), which Follow system never overrides:
    /// never without its reader, so the switch is never locked without a card.
    pub(super) fn follows_desktop_theme(&self, cx: &App) -> bool {
        self.settings.theme.on_desktop(cx) == ThemeSelection::Omarchy
    }

    /// The Desktop group: on Linux, the Omarchy card while the desktop has an
    /// Omarchy theme, or while it is the selection, which then stays visible
    /// with the reason the theme cannot be read.
    fn desktop_theme_cards(&self, cx: &App) -> Vec<ThemeCard> {
        #[cfg(target_os = "linux")]
        if let Some(card) = appearance::omarchy::card(cx)
            && (card.offered || self.follows_desktop_theme(cx))
            && self
                .theme_card_bodies
                .iter()
                .any(|(drawn, _)| *drawn == ThemeSelection::Omarchy)
        {
            return vec![ThemeCard {
                selection: ThemeSelection::Omarchy,
                palette: card.palette,
                name: "Omarchy".into(),
                description: card.description.into(),
                warnings: 0,
                detail: Some(card.status.into()),
                accessible_name: Some(card.accessible_name.into()),
                tag: card.unavailable.then(|| "Unavailable".into()),
                wrap: true,
            }];
        }
        let _ = cx;
        Vec::new()
    }

    /// The picker's groups and their cards, in display order.
    fn theme_card_groups(&self, cx: &App) -> [(&'static str, &'static str, Vec<ThemeCard>); 4] {
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
                    detail: None,
                    accessible_name: None,
                    tag: None,
                    wrap: false,
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
                detail: None,
                accessible_name: None,
                tag: None,
                wrap: false,
            })
            .collect::<Vec<_>>();
        [
            ("desktop", "Desktop", self.desktop_theme_cards(cx)),
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
        if !self.theme_cards_ready {
            return;
        }
        let active = palette(cx);
        let selected = self.selected_theme_card(cx);
        for (_, _, cards) in self.theme_card_groups(cx) {
            for card in cards {
                let key = CardKey {
                    wrap: card.wrap,
                    ..CardKey::new(
                        card.palette,
                        card.name,
                        card.description,
                        card.warnings,
                        card.tag,
                        selected == Some(card.selection),
                        active,
                    )
                };
                self.theme_card_body(card.selection)
                    .update(cx, |body, cx| body.set_key(key, active, cx));
            }
        }
    }

    fn render_theme_picker(&self, columns: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let selected = self.selected_theme_card(cx);
        // Keyboard focus, as the window's focus ring shows it: after a key,
        // and still while the pointer moves, until a pointer press.
        let focused = self
            .theme_card_focus
            .iter()
            .find(|(_, handle)| self.focus_ring.shows(handle))
            .map(|(selection, _)| *selection);
        #[cfg(test)]
        self.card_names.borrow_mut().clear();
        let groups = self.theme_card_groups(cx);
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
                                let focus = row
                                    .iter()
                                    .map(|card| Some(self.theme_card_focus(card.selection).clone()))
                                    .collect();
                                div()
                                    .on_children_prepainted(self.keep_in_view(focus, cx))
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
                            .when(key == "desktop", |group| {
                                group.children(self.desktop_theme_reason(p, cx))
                            })
                    }),
            )
            .into_any_element()
    }

    /// Linux: while the Omarchy card is unavailable, the reason in its own
    /// words, as a muted line under the card's row; the caption keeps only
    /// "Unavailable · Keeping ‹Theme›". Bounded to two lines with the Your
    /// themes notice's styling, so a long reason never moves the palettes.
    fn desktop_theme_reason(&self, p: appearance::Palette, cx: &App) -> Option<AnyElement> {
        #[cfg(target_os = "linux")]
        {
            let reason = appearance::omarchy::card(cx)?.reason?;
            Some(
                native_accessibility::static_text(
                    div()
                        .id("settings-theme-omarchy-reason")
                        .debug_selector(|| "settings-theme-omarchy-reason".into()),
                    reason.clone(),
                )
                .text_size(appearance::ui_text(12.))
                .text_color(rgb(p.muted))
                .line_clamp(2)
                .text_ellipsis()
                .child(reason)
                .into_any_element(),
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (p, cx);
            None
        }
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
            ThemeSelection::Omarchy => ElementId::from("settings-theme-omarchy"),
        };
        let selector = move || match selection {
            ThemeSelection::BuiltIn(choice) => format!("settings-theme-{}", choice as usize),
            ThemeSelection::Custom(id) => format!("settings-custom-theme-{id}"),
            ThemeSelection::Omarchy => "settings-theme-omarchy".into(),
        };
        let accessible_name = card
            .accessible_name
            .clone()
            .unwrap_or_else(|| format!("{} theme", card.name).into());
        #[cfg(test)]
        self.card_names
            .borrow_mut()
            .push((selector(), accessible_name.to_string()));
        let tooltip = if let Some(detail) = &card.detail {
            format!("{} · {detail}", card.name)
        } else if card.warnings == 0 {
            format!("{} · {}", card.name, card.description)
        } else {
            format!(
                "{} · {} · {}",
                card.name,
                card.description,
                crate::counted(card.warnings, "readability warning", "readability warnings")
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
        let new_theme = self.page_focus("custom-themes-new", cx);
        let import_theme = self.page_focus("custom-themes-import", cx);
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
            // Centred on the title's first line, as every Settings row's
            // trailing control, inside the card's padding; too narrow for
            // them beside the text, the actions take a line of their own
            // below it, as the text sizes' controls do, New theme…'s "+" on
            // the title's edge, its transparent frame its label inset before
            // it.
            .child(
                wrapping_row(
                    div().debug_selector(|| "custom-themes-header".into()),
                    setting_description(
                        "Your themes",
                        "Start from any built-in theme, adjust its colors with a live preview, and save it under your own name.",
                        cx,
                    ),
                    div()
                        .debug_selector(|| "custom-themes-actions".into())
                        .on_children_prepainted(self.keep_in_view(
                            vec![Some(new_theme.clone()), Some(import_theme.clone())],
                            cx,
                        ))
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap_2()
                        .child(
                            button("custom-themes-new", new_label, "plus", false)
                                .track_focus(&new_theme)
                                // Every button on the card rings for keyboard
                                // focus only, from the shared ring: a key
                                // shows it, a pointer press ends it, so the
                                // theme editor's close by a pointer press
                                // leaves New theme… focused without a ring,
                                // and a key close draws it (decided
                                // 2026-11-10).
                                .focus_ring(self.focus_ring.shows(&new_theme))
                                .debug_selector(|| "custom-themes-new".into())
                                .disabled(pending || full)
                                .tooltip(new_tooltip)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_theme_editor(None, window, cx)
                                })),
                        )
                        .child(
                            button("custom-themes-import", "Import…", "", false)
                                .track_focus(&import_theme)
                                .focus_ring(self.focus_ring.shows(&import_theme))
                                .debug_selector(|| "custom-themes-import".into())
                                .accessibility_label(import_name)
                                .disabled(pending || full)
                                .tooltip(import_tooltip)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.import_custom_theme(window, cx)
                                })),
                        ),
                    // The `button` helper's height.
                    appearance::ui_size(28.),
                    rem(),
                    cx,
                )
                // New theme…'s frame pads its icon by the helper's label
                // inset.
                .ghost_lead(appearance::ui_size(crate::LABEL_INSET)),
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
            // paragraph break on its last allowed line. The folder picker's
            // failure is two lines, its guidance and then the service's error,
            // shortened where it is built (`folder_picker::failure`), so a cut
            // there drops the quoted error, not the guidance.
            .children(self.theme_editor.notice().map(|notice| {
                native_accessibility::static_text(
                    div()
                        .id("custom-themes-notice")
                        .debug_selector(|| "custom-themes-notice".into()),
                    notice.to_owned(),
                )
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .line_clamp(2)
                    .when(!notice.contains('\n'), |el| el.text_ellipsis())
                    .child(notice.to_owned())
            }))
            .children(self.theme_editor.error().map(|error| {
                // One text, so the bound and the accessible name cover the
                // whole message; a dialog failure's quoted service error is
                // muted (`folder_picker::styled_message`). The selector names
                // the colors and where the muted run starts, for tests.
                let detail = folder_picker::detail_start(error);
                native_accessibility::static_text(
                    div()
                        .id("custom-themes-error")
                        .debug_selector(move || match detail {
                            Some(start) => format!(
                                "custom-themes-error-{:06x}-{start}-{:06x}",
                                p.warning, p.muted
                            ),
                            None => format!("custom-themes-error-{:06x}", p.warning),
                        }),
                    error.to_owned(),
                )
                .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.warning))
                    .line_clamp(2)
                    .when(!error.contains('\n'), |el| el.text_ellipsis())
                    .child(folder_picker::styled_message(error, p.muted))
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
        // The room the installed ring takes outside a focused row action,
        // which the list keeps above its first row and below its last, the
        // page reveal keeps around the revealed row, and the strips cover.
        let ring_room = appearance::button_ring_room(cx);
        let rows = self.custom_themes.len();
        let scrolls = rows > VISIBLE_ROWS;
        self.theme_editor.retain_row_focus(&self.custom_themes);
        let focused = self
            .theme_editor
            .focused_row(&self.custom_themes, window, cx);
        // An action that keeps focus while a scroll has taken its row out of
        // the list has no node of its own; the list tracks its handle
        // meanwhile, so Tab still reaches `tab_theme_rows` in the Settings
        // key context rather than the window root's order.
        let scrolled_out = focused
            .as_ref()
            .filter(|focused| {
                self.theme_editor
                    .focus_scrolled_out(&self.custom_themes, focused, window, cx)
            })
            .map(|(_, handle)| handle.clone());
        let revealed = self.theme_editor.reveal_focused_row(focused);
        let scroll = self.theme_editor.rows_scroll().clone();
        let row_height = appearance::ui_size(30.);
        let viewport = appearance::ui_size(30. * VISIBLE_ROWS as f32);
        // The Settings page scrolls too, and its viewport clips the ring of a
        // row the list has revealed as surely as the list would: the row
        // holding focus is revealed in the page as well, once per focus
        // change as in the list, with the installed ring's footprint of room
        // above and below it. It is placed where the list will stand once
        // its own reveal lands (`planned_rows_top`, as `rows_off_boundary`
        // judges the strips), from where the page last laid the rows out.
        if let Some(row) = revealed
            && let Some(anchor) = self.theme_editor.rows_anchor().get()
        {
            let page = self.theme_editor.page_scroll();
            let top = anchor + page.offset().y + row_height * row as f32
                - planned_rows_top(&scroll, row_height, viewport, rows);
            reveal_in_page(page, top - ring_room, top + row_height + ring_room);
        }
        // The list clips at its own bounds, the ring's room included, so a
        // row partly scrolled out of the viewport paints into that room. Two
        // strips of the card's surface cover the room while the rows stand
        // off a row boundary; on a boundary the rows fill the viewport
        // exactly and the room is the ring's alone (`rows_off_boundary`).
        let strips = scrolls && rows_off_boundary(&scroll, row_height, viewport, rows, window);
        let p = palette(cx);
        let surface = rgb(p.subtle);
        // Every row's fill, the import highlight and the hover surface, is
        // painted beneath all the rows rather than as the row's own
        // background: a row paints after the row above it, so its own fill
        // covered the bottom edge of the ring around a focused action on the
        // row above (`render_custom_theme_row` records where each row is
        // drawn, at most the nine rows the viewport can show).
        let drawn = std::rc::Rc::new(std::cell::RefCell::new(Vec::<RowFill>::new()));
        let fills = drawn.clone();
        // The box the rows occupy in the card's column: exactly the rows, so
        // the card, the row pitch and everything below the card stay where
        // the plain stack put them. The list itself is positioned over it the
        // ring's room taller at each end (`ring_room`) rather than laid out with
        // negative margins: an absolute child does not enter taffy's sizing
        // of the card, whereas a flow child whose margins pull its content
        // contribution below its flex basis collapses the card's content
        // height under max-content sizing (taffy 0.13 scales that negative
        // difference by the item's inner flex basis).
        let page = self.theme_editor.page_scroll().clone();
        let anchor = self.theme_editor.rows_anchor().clone();
        div()
            .debug_selector(|| "custom-themes-rows-box".into())
            .relative()
            // The rows' hover surface bleeds into the card padding so the row
            // content aligns with the card title.
            .mx(appearance::ui_size(-8.))
            .h(appearance::ui_size(30. * rows.min(VISIBLE_ROWS) as f32))
            .flex_shrink_0()
            // The first row starts the ring's room below the list's top; the
            // page's offset, applied to these bounds, is taken back out.
            .on_children_prepainted(move |children, _, _| {
                if let Some(list) = children.first() {
                    anchor.set(Some(list.top() + ring_room - page.offset().y));
                }
            })
            .child(
                div()
                    .debug_selector(|| "custom-themes-rows".into())
                    // The kit's focus ring sits outside a row action and the
                    // list clips to its own bounds, so the list keeps the
                    // ring's room above its first row and below its last,
                    // over the card's gaps: the rows and the card do not move
                    // (`DESIGN.md`).
                    .absolute()
                    .top(-ring_room)
                    .bottom(-ring_room)
                    .left_0()
                    .right_0()
                    .when_some(scrolled_out, |list, handle| list.track_focus(&handle))
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
                    .on_scroll(move |event, window, cx| {
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
                        canvas(
                            |_, _, _| (),
                            move |_, (), window, cx| {
                                for row in fills.take() {
                                    // What the row's own background and hover
                                    // style painted: the plain hover surface
                                    // under the pointer, and on the imported
                                    // row the selected surface, blended as a
                                    // hovered selected row under the pointer.
                                    let hover = row_hovered(&row.hitbox, window, cx);
                                    let color = match (row.imported, hover) {
                                        (imported, true) => p.row_hover(imported),
                                        (true, false) => p.selected,
                                        (false, false) => continue,
                                    };
                                    let color = Hsla::from(rgb(color));
                                    window.with_content_mask(Some(row.mask), |window| {
                                        window.paint_quad(quad(
                                            row.bounds,
                                            Corners::all(ROW_RADIUS)
                                                .clamp_radii_for_quad_size(row.bounds.size),
                                            color,
                                            Edges::default(),
                                            color.alpha(0.),
                                            BorderStyle::default(),
                                        ))
                                    });
                                }
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        uniform_list(
                            "custom-theme-rows",
                            rows,
                            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                                // Tab across the viewport boundary asked for a
                                // row this render draws: its actions are tab
                                // stops from this frame on, so the next frame
                                // focuses one or takes the key's step.
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
                                        this.render_custom_theme_row(index, count, &drawn, cx)
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        // The ring's room: the list's content mask is its own
                        // bounds, padding included, while the rows are laid
                        // out inside it.
                        .py(ring_room)
                        // The scrollbar paints an absolute overlay; reserve
                        // its track so the thumb never covers Delete….
                        .when(scrolls, |list| list.pr(Scrollbar::width()))
                        .track_scroll(self.theme_editor.rows_scroll()),
                    ),
            )
            .when(strips, |list| {
                // Painted after the rows, over the ring's room at each end
                // of the viewport and nothing of the rows' box; absolute, so
                // the card and the rows keep their geometry.
                let strip = |name: &'static str| {
                    div()
                        .debug_selector(move || name.into())
                        .absolute()
                        .left_0()
                        .right_0()
                        .h(ring_room)
                        .bg(surface)
                };
                list.child(strip("custom-themes-ring-room-top").top(-ring_room))
                    .child(strip("custom-themes-ring-room-bottom").bottom(-ring_room))
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
    /// The row records where it is drawn in `drawn`, and the list paints its
    /// fill beneath the rows (`render_custom_theme_rows`).
    fn render_custom_theme_row(
        &self,
        index: usize,
        warnings: usize,
        drawn: &std::rc::Rc<std::cell::RefCell<Vec<RowFill>>>,
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
        let [edit, export, delete] = [0, 1, 2].map(|n| self.theme_editor.action_focus(id, n, cx));
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
            .rounded(ROW_RADIUS)
            // A hovered row takes the hover surface. A just-imported theme
            // is highlighted on the selected surface. It is added, not
            // applied: the checkmark still marks the theme the window
            // uses. Hovering the row is not a card action, so it keeps
            // the highlight, blended as a hovered selected row, rather
            // than replacing it with the plain hover surface. The list
            // paints every one of these fills beneath all the rows from
            // what this probe records, so no row paints a fill of its own
            // over the ring of a focused action on the row above. The
            // empty hover style still has GPUI draw the page again when
            // the pointer enters or leaves the row.
            .child({
                let drawn = drawn.clone();
                let imported = imported == Some(id);
                canvas(
                    move |bounds, window, _| {
                        drawn.borrow_mut().push(RowFill {
                            bounds,
                            mask: window.content_mask(),
                            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
                            imported,
                        })
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full()
            })
            .hover(|row| row)
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
                let text = crate::counted(warnings, "readability warning", "readability warnings");
                row.child(
                    native_accessibility::static_text(
                        div().id(("custom-theme-warnings", id as usize)),
                        format!("{}: {text}", theme.name),
                    )
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
                    .track_focus(&edit)
                    // Ringed for keyboard focus only, as every button on the
                    // card is (decided 2026-11-10).
                    .focus_ring(self.focus_ring.shows(&edit))
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
                            .track_focus(&export)
                            .focus_ring(self.focus_ring.shows(&export))
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
                    .track_focus(&delete)
                    .focus_ring(self.focus_ring.shows(&delete))
                    .debug_selector(move || format!("delete-custom-theme-{id}"))
                    .accessibility_label(delete_name)
                    .disabled(pending)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_delete_theme(id, window, cx)
                    })),
            );
        Some(row.into_any_element())
    }

    /// The focus handle Settings control `id` tracks, the same in every
    /// frame, so the page can tell when the keyboard focuses it.
    pub(super) fn page_focus(&self, id: impl Into<ElementId>, cx: &mut App) -> FocusHandle {
        self.theme_editor.page().focus(id, cx)
    }

    /// For the `on_children_prepainted` of a Settings element whose children
    /// include controls, `focus` holding each child's handle in order: the
    /// keyboard focusing one scrolls the page by the least that shows it with
    /// the focus ring's room.
    fn keep_in_view<'a>(
        &'a self,
        focus: Vec<Option<FocusHandle>>,
        cx: &App,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static + use<'a> {
        self.theme_editor
            .page()
            .controls(focus, appearance::button_ring_room(cx))
    }

    /// [`Self::keep_in_view`] for a row of segments, as List density's,
    /// Reduce motion's and Increase contrast's, `focus` holding the
    /// segments' handles in order: the first listener is for the
    /// `on_children_prepainted` of the row's wrapper, whose one child is the
    /// row, the second for the segments' group. The keyboard focusing a
    /// segment scrolls the page by the least that shows the whole row, its
    /// title, the description a choice changes and its segments, with the
    /// focus ring's room, so a press's feedback is in view beside it. A row
    /// taller than the page's view shows the focused segment with its ring.
    /// Where the row fits, the row is also the control a resize judges
    /// (`FocusReveal::finish`): a resize reveals it again only after a frame
    /// that showed it whole, so a row the person has scrolled partly out of
    /// view is left where it is, as any control is. Revealed alone, the
    /// group left Increase contrast's description below the fold of a
    /// 1,480 × 973 window.
    fn keep_row_in_view<'a>(
        &'a self,
        focus: &[FocusHandle],
        cx: &App,
    ) -> (impl Prepainted + use<'a>, impl Prepainted + use<'a>) {
        let page = self.theme_editor.page();
        let room = appearance::button_ring_room(cx);
        let row = page.group(focus.to_vec(), room);
        let segment = page.controls(focus.iter().cloned().map(Some).collect(), room);
        let view = page.scroll().clone();
        // The group is prepainted before the row's wrapper, whose listener
        // then finds where the segments lie in the same frame.
        let segments = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let drawn = segments.clone();
        (
            move |bounds: Vec<Bounds<Pixels>>, window: &mut Window, cx: &mut App| {
                let segments = drawn.take();
                let Some(bounds) = bounds.into_iter().next() else {
                    return;
                };
                if bounds.size.height + room * 2. <= view.bounds().size.height {
                    row(vec![bounds], window, cx);
                } else {
                    segment(segments, window, cx);
                }
            },
            move |bounds: Vec<Bounds<Pixels>>, _: &mut Window, _: &mut App| {
                *segments.borrow_mut() = bounds;
            },
        )
    }

    /// The Settings page, laid out `width` wide: the width it is drawn at in
    /// this frame ([`SettingsPage`]), which beside the project list is less
    /// than the window's.
    pub(super) fn render_settings(
        &self,
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(ui) = &self.settings_ui else {
            return div().into_any_element();
        };
        // A control the keyboard has focused scrolls into view in this frame.
        self.theme_editor.page().reveal(window, cx);
        let p = palette(cx);
        // A write refuses the identity's fields and actions at once; they
        // look so once the write looks busy (`workspace::BusyLook`).
        let busy = self.operation_busy.is_some();
        let looks_busy = self.operation_shown().is_some();
        // The page's breakpoints, the theme grid's columns among them, are
        // the page's own width, not the window's: beside the project list a
        // 1,200 px window leaves the page 960 px, where the grid laid four
        // columns out across the two groups' room.
        let compact = width < appearance::ui_size(1060.);
        let narrow = width < appearance::ui_size(720.);
        let retry = self.page_focus("retry-save-settings", cx);
        let theme_columns = if narrow {
            2
        } else if compact {
            3
        } else {
            4
        };
        let branch_edited =
            ui.branch.read(cx).value().as_ref() != self.settings.default_branch.as_str();
        // Against the identity loaded for the repository shown: with none
        // loaded there is no edit to save or reset, whatever the fields
        // hold. Asked of the window's identity alone, every field counted
        // as edited while none was held, and Save was offered for another
        // repository's Name and Email.
        let loaded = self.loaded_identity();
        // Both fields are read at every build, with or without an identity
        // loaded, as they were whenever one was held: read only once one was
        // loaded, the frame that first drew the card built the page twice,
        // Name and Email notifying it after that frame
        // (`theme_editor::tests::state_from_other_features_builds_the_page_through_the_reconciler`).
        let (name, email) = (
            ui.identity_name.read(cx).value(),
            ui.identity_email.read(cx).value(),
        );
        let identity_edited = loaded
            .is_some_and(|profile| name.trim() != profile.name || email.trim() != profile.email);
        let repository_name = self
            .repository
            .as_ref()
            .map(|repository| self.project_name(repository.path()));
        // While the Omarchy theme is selected, Follow system shows off and
        // cannot be turned on: the desktop decides the theme.
        let desktop_theme = self.follows_desktop_theme(cx);
        let follow_checked = self.settings.follow_system && !desktop_theme;
        let follow = self.page_focus("follow-system", cx);
        let follow_description: SharedString = if desktop_theme {
            "Omarchy follows your desktop theme".into()
        } else {
            "Braden in Light Mode; your selected dark palette in Dark Mode.".into()
        };
        #[cfg(test)]
        self.follow_system_row.replace(Some((
            follow_checked,
            desktop_theme,
            follow_description.clone(),
        )));
        let density_focus: Vec<_> = (0..Density::ALL.len())
            .map(|index| self.page_focus(("settings-density", index), cx))
            .collect();
        let (density_row_in_view, density_segments_drawn) =
            self.keep_row_in_view(&density_focus, cx);
        let density_segments = div()
            .debug_selector(|| "settings-density-segments".into())
            .on_children_prepainted(density_segments_drawn)
            .flex()
            .gap_1()
            .p_1()
            .rounded(px(8.))
            .bg(rgb(p.canvas))
            .children(
                Density::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, density)| {
                        let selected = self.settings.density == density;
                        // The shared helper's look, as History's segments:
                        // unselected, transparent at rest with the palette's
                        // hover and pressed fills, so a press steps to
                        // almost the selected fill; selected, the `selected`
                        // surface, tinted on hover rather than faded, which
                        // would fade the focus ring with it.
                        Button::new(("settings-density", index))
                            .track_focus(&density_focus[index])
                            .small()
                            .with_variant(appearance::control_button_variant(selected))
                            .label(density.label())
                            .selected(selected)
                            .toggled(selected)
                            .debug_selector(move || format!("settings-density-{index}"))
                            .when(selected, |button| {
                                button.hover(appearance::control_selected_hover)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.settings.density != density {
                                    this.settings.density = density;
                                    this.save_preferences(window, cx);
                                }
                            }))
                    }),
            );
        let appearance = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                div()
                    .on_children_prepainted(self.keep_in_view(vec![Some(follow.clone())], cx))
                    .debug_selector(|| "follow-system-switch".into())
                    .child(
                        switch_row(
                            "follow-system",
                            FOLLOW_SYSTEM,
                            FOLLOW_SYSTEM,
                            follow_description,
                            &[],
                            cx,
                        )
                        .track_focus(&follow)
                        .checked(follow_checked)
                        .disabled(desktop_theme)
                        .on_click(cx.listener(
                            |this, checked, window, cx| {
                                // The Omarchy theme follows the desktop instead.
                                if this.follows_desktop_theme(cx) {
                                    return;
                                }
                                this.settings.follow_system = *checked;
                                this.apply_appearance(window, cx);
                                this.save_preferences(window, cx);
                            },
                        )),
                    ),
            )
            .child(self.render_theme_picker(theme_columns, cx))
            .child(self.render_custom_themes(window, cx))
            .child(self.render_text_size_setting(false, cx))
            .child(self.render_text_size_setting(true, cx))
            .children(self.render_code_font_setting(cx))
            .child(
                div().on_children_prepainted(density_row_in_view).child(
                    div()
                        .debug_selector(|| "settings-density-row".into())
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap_3()
                        .when(narrow, |row| row.flex_col())
                        .pt_4()
                        .border_t_1()
                        .border_color(rgb(p.border))
                        .child(setting_description(
                            "List density",
                            "A little breathing room, or more at a glance.",
                            cx,
                        ))
                        // Below the separator, the row's top padding is
                        // the room above its title.
                        .child(if narrow {
                            density_segments.into_any_element()
                        } else {
                            on_title_line(density_segments, rem(), cx).into_any_element()
                        }),
                ),
            )
            .child(self.render_display_setting(DisplaySetting::ReduceMotion, narrow, cx))
            .child(self.render_display_setting(DisplaySetting::IncreaseContrast, narrow, cx))
            .into_any_element();

        let save_editor = self.page_focus("save-external-editor", cx);
        let save_terminal = self.page_focus("save-external-terminal", cx);
        let reopen_last = self.page_focus("settings-reopen-last", cx);
        let project_pane = self.page_focus("settings-project-pane", cx);
        let save_branch = self.page_focus("save-default-branch", cx);
        let startup = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(div()
                .flex().flex_col().gap_2()
                .child(setting_description("External editor", if cfg!(target_os = "macos") { "Application name, such as Visual Studio Code. File → Open Repository in Editor opens the current folder." } else { "Executable name or path, such as code. Menu → Open repository in editor opens the current folder; left empty on Omarchy, your Omarchy editor opens it." }, cx))
                // As the default branch's field and Save are: the Button keeps
                // its content's width and its own fill at the card's trailing
                // edge, where the transparent helper stretched to the card's
                // width drew only a centred dim label while disabled.
                .child(div()
                    .on_children_prepainted(self.keep_in_view(
                        vec![Some(ui.editor.focus_handle(cx)), Some(save_editor.clone())],
                        cx,
                    ))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().debug_selector(|| "settings-editor-field".into()).flex_1().min_w_0()
                        .child(Input::new(&ui.editor).aria_label("External editor")))
                    .child(Button::new("save-external-editor")
                        .track_focus(&save_editor)
                        .debug_selector(|| "save-external-editor".into())
                        .label("Save editor")
                        .disabled(ui.editor.read(cx).value().trim() == self.settings.external_editor)
                        .tooltip("Save the editor that opens repositories")
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(ui) = &this.settings_ui {
                                this.settings.external_editor = ui.editor.read(cx).value().trim().to_owned();
                                this.save_preferences(window, cx);
                            }
                        })))))
            .child(div()
                .flex().flex_col().gap_2()
                .child(setting_description("External terminal", if cfg!(target_os = "macos") { "Optional application name, such as Ghostty or iTerm. Leave empty to use Terminal." } else { "Optional executable name or path, such as ghostty or kitty. Leave empty to use xdg-terminal-exec or $TERMINAL." }, cx))
                .child(div()
                    .on_children_prepainted(self.keep_in_view(
                        vec![Some(ui.terminal.focus_handle(cx)), Some(save_terminal.clone())],
                        cx,
                    ))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().debug_selector(|| "settings-terminal-field".into()).flex_1().min_w_0()
                        .child(Input::new(&ui.terminal).aria_label("External terminal")))
                    .child(Button::new("save-external-terminal")
                        .track_focus(&save_terminal)
                        .debug_selector(|| "save-external-terminal".into())
                        .label("Save terminal")
                        .disabled(ui.terminal.read(cx).value().trim() == self.settings.external_terminal)
                        .tooltip("Save the terminal that opens repositories and terminal editors")
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(ui) = &this.settings_ui {
                                this.settings.external_terminal = ui.terminal.read(cx).value().trim().to_owned();
                                this.save_preferences(window, cx);
                            }
                        })))))
            .child(
                div()
                    .on_children_prepainted(self.keep_in_view(vec![Some(reopen_last.clone())], cx))
                    .debug_selector(|| "settings-reopen-last-row".into())
                    .child(
                        switch_row(
                            "settings-reopen-last",
                            REOPEN_LAST,
                            REOPEN_LAST,
                            "Reopen the selected tab and retain up to eight saved repository tabs.",
                            &[],
                            cx,
                        )
                            .track_focus(&reopen_last)
                            .checked(self.settings.reopen_last)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                if this.settings.reopen_last != *checked {
                                    this.settings.reopen_last = *checked;
                                    this.save_preferences(window, cx);
                                }
                            })),
                    ),
            )
            .child({
                // On where the window holds the pane back, the switch says why
                // no list is drawn; the row keeps the taller description's
                // height, so neither the switch nor the rows below move as one
                // replaces the other.
                let shown: SharedString = PROJECT_PANE_DESCRIPTION.into();
                let held: SharedString = crate::project_pane::HELD_BACK_DESCRIPTION.into();
                let held_back = self.settings.project_pane && !self.project_pane_fits(window);
                div()
                    .on_children_prepainted(self.keep_in_view(vec![Some(project_pane.clone())], cx))
                    .debug_selector(|| "settings-project-pane-row".into())
                    .child(
                        switch_row(
                            "settings-project-pane",
                            PROJECT_PANE,
                            PROJECT_PANE,
                            if held_back { held.clone() } else { shown.clone() },
                            &[shown, held],
                            cx,
                        )
                            .track_focus(&project_pane)
                            .checked(self.settings.project_pane)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                this.switch_project_pane(*checked, window, cx)
                            })),
                    )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_description(
                        "Default branch",
                        "Used when creating a new repository.",
                        cx,
                    ))
                    .child(
                        div()
                            .on_children_prepainted(self.keep_in_view(
                                vec![Some(ui.branch.focus_handle(cx)), Some(save_branch.clone())],
                                cx,
                            ))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&ui.branch).aria_label("Default branch")),
                            )
                            .child(
                                Button::new("save-default-branch")
                                    .track_focus(&save_branch)
                                    .debug_selector(|| "save-default-branch".into())
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

        let save_identity = self.page_focus("save-repository-identity", cx);
        let reset_identity = self.page_focus("use-current-identity", cx);
        // A marked tab's repository is the one read before the mark: the
        // identity card's actions on it are refused with the mark's reason.
        let marked = self.shown_tab_unavailable();
        let identity = if let Some(repository_name) = repository_name {
            // Its actions, drawn as recorded for tests, which read the name
            // and state each gives assistive technology.
            use native_accessibility::DrawnButton as _;
            let repository_identity = format!(
                "{}\n{}",
                repository_name,
                self.path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            );
            let manage_profiles = self.page_focus("settings-manage-profiles", cx);
            // How far Reset edits and Manage named profiles…, ghost Buttons,
            // stand their frames before the card's text edge
            // (`crate::ghost_pull`). With their 3 px ring it takes 13 px of the
            // section cards' 1.25 rem padding at 13 pt and 17 of 22.5 at 18 pt;
            // standing its frame on the edge, a wrapped Reset edits started its
            // icon 10 px past the field labels at 13 pt and 14 px at 18 pt.
            let pull = ghost_pull(window);
            // On a marked tab what the mark says and its way back, nothing
            // from before it, as the bar and the navigation say there: the
            // identity read before the mark read as the repository's own, and
            // nothing on the page said why its fields and actions were refused
            // (decided 2026-11-12).
            let line: SharedString = match loaded {
                _ if marked => format!(
                    "{} · {}",
                    crate::branch_actions::UNAVAILABLE_REASON,
                    crate::branch_actions::UNAVAILABLE_WAY_BACK
                )
                .into(),
                Some(profile) if !profile.name.is_empty() && !profile.email.is_empty() => {
                    format!("Current identity: {} <{}>", profile.name, profile.email).into()
                }
                Some(_) => "A commit identity has not been configured yet.".into(),
                None => "Git identity has not been loaded yet.".into(),
            };
            // The repository card, Name, Email, the signing line, the save
            // buttons, the note and Manage named profiles…; on a marked tab
            // neither the signing line nor the note, both chosen from the
            // identity read before the mark (decided 2026-11-14), and with
            // no identity loaded no note, which says where a save goes as
            // only that read decides (decided 2026-11-15).
            let noted = !marked && loaded.is_some();
            // Why Save and Reset edits are refused: the mark's reason, or
            // with no identity loaded the read's (`identity_wait`).
            let refusal = if marked {
                Some(crate::branch_actions::UNAVAILABLE_REASON)
            } else {
                self.identity_wait()
            };
            let mut kept = vec![
                None,
                Some(ui.identity_name.focus_handle(cx)),
                Some(ui.identity_email.focus_handle(cx)),
            ];
            kept.extend((!marked).then_some(None));
            kept.push(None);
            kept.extend(noted.then_some(None));
            kept.push(Some(manage_profiles.clone()));
            div()
                .on_children_prepainted(self.keep_in_view(kept, cx))
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    native_accessibility::static_text(
                        div()
                            .id("settings-repository-identity")
                            .debug_selector(|| "settings-repository-identity".into()),
                        format!("Repository {repository_identity}"),
                    )
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
                        .child(
                            div()
                                .debug_selector({
                                    let line = line.clone();
                                    move || format!("settings-identity-line: {line}")
                                })
                                .text_size(crate::appearance::ui_text(11.))
                                .line_height(relative(1.5))
                                .text_color(rgb(p.muted))
                                .child(line.clone()),
                        ),
                )
                // A marked tab refuses them as a write does, refused and
                // dimmed, so nothing typed there waits for a save no route
                // makes: they took a Name and Email the dimmed Save could not
                // write (decided 2026-11-12). There they draw no value read
                // before the mark (`sync_identity_fields`), say why as the
                // line does and leave the Tab order (decided 2026-11-14).
                .child(settings_field("Name", &ui.identity_name, (busy || marked, looks_busy || marked), marked.then(|| line.clone()), cx))
                .child(settings_field("Email", &ui.identity_email, (busy || marked, looks_busy || marked), marked.then(|| line.clone()), cx))
                .when(!marked, |card| {
                    let signing = match loaded {
                        Some(profile) if profile.signing => "Commit signing is enabled in Git. GitTurtle uses your configured signer.",
                        Some(_) => "Commit signing is off in Git. Your existing Git signing configuration is preserved.",
                        None => "Signing follows your existing Git configuration.",
                    };
                    card.child(
                        div()
                            .debug_selector(move || format!("settings-signing-line: {signing}"))
                            .text_size(crate::appearance::ui_text(11.))
                            .line_height(relative(1.5))
                            .text_color(rgb(p.muted))
                            .child(signing),
                    )
                })
                .child(
                    div()
                        .on_children_prepainted(self.keep_in_view(
                            vec![Some(save_identity.clone()), Some(reset_identity.clone())],
                            cx,
                        ))
                        .flex()
                        .flex_wrap()
                        // Reset edits, a ghost Button, wrapped onto a line of
                        // its own stands its icon on the card's edge, its
                        // frame `pull` before it; beside Save its margin takes
                        // back what the gap adds, so there it stays the gap
                        // past Save and the row wraps where it did, as a
                        // wrapping row's ghost lead does.
                        .gap_y(window.rem_size() * 0.5)
                        .gap_x(window.rem_size() * 0.5 + pull)
                        .child(
                            Button::new("save-repository-identity")
                                .track_focus(&save_identity)
                                .debug_selector(|| "save-repository-identity".into())
                                .primary()
                                .icon(Icon::default().path("icons/check.svg").size(px(14.)))
                                .label(if looks_busy { "Working…" } else { "Save repository identity" })
                                .map(|save| {
                                    let unavailable = !identity_edited || marked;
                                    let save = crate::workspace::refusing(save, || "save-repository-identity".into(), busy || unavailable, looks_busy || unavailable);
                                    says_why(save, "save-repository-identity", "Save repository identity", None, refusal)
                                })
                                .on_click(cx.listener(|this, _, window, cx| this.save_identity(window, cx)))
                                .node_recorded(),
                        )
                        .child(
                            button("use-current-identity", "Reset edits", "refresh", false)
                                .track_focus(&reset_identity)
                                .debug_selector(|| "use-current-identity".into())
                                .ml(-pull)
                                .map(|reset| {
                                    let unavailable = !identity_edited || marked;
                                    let reset = crate::workspace::refusing(reset, || "use-current-identity".into(), busy || unavailable, looks_busy || unavailable);
                                    says_why(reset, "use-current-identity", "Reset edits", Some("Restore the repository’s current name and email"), refusal)
                                })
                                // Refused at once while a write runs, on a
                                // marked tab and with no identity loaded.
                                .on_click(cx.listener(|this, _, window, cx| this.reset_identity_edits(window, cx)))
                                .node_recorded(),
                        ),
                )
                // Where a save goes, which the identity read chose: on a
                // marked tab it is not drawn, as the signing line is not,
                // nor with no identity loaded, where it claimed the shared
                // configuration before any read had said so.
                .when(noted, |card| {
                    let note = if loaded.is_some_and(|profile| profile.private_worktree) { "Name and email are saved only to this worktree’s existing private Git configuration." } else { "Name and email are saved to repository Git configuration shared by linked worktrees. Global identity is preserved." };
                    card.child(div().debug_selector(move || format!("settings-identity-note: {note}")).text_size(crate::appearance::ui_text(11.)).line_height(relative(1.5)).text_color(rgb(p.muted)).child(note))
                })
                // A ghost Button at the card's text edge: its icon on the
                // edge, its frame `pull` before it, as wide as its content
                // as Settings' other ghost Buttons are. Stretched across the
                // column, it centred its icon and label in the card. On a
                // marked tab it is refused with the mark's reason, as the
                // profile picker's Manage profiles… and the palette's Manage
                // Git profiles… are, since no route opens the manager there
                // (`open_profiles`): it was drawn enabled, and a click opened
                // nothing and said nothing.
                .child(button("settings-manage-profiles", "Manage named profiles…", "user", false).track_focus(&manage_profiles).debug_selector(|| "settings-manage-profiles".into()).self_start().ml(-pull).map(|manage| {
                    let manage = crate::workspace::refusing(manage, || "settings-manage-profiles".into(), busy || marked, looks_busy || marked);
                    says_why(manage, "settings-manage-profiles", "Manage named profiles…", None, marked.then_some(crate::branch_actions::UNAVAILABLE_REASON))
                }).on_click(cx.listener(|this, _, window, cx| this.open_profiles(window, cx))).node_recorded())
                .into_any_element()
        } else {
            let open_projects = self.page_focus("settings-open-projects", cx);
            div()
                .on_children_prepainted(
                    self.keep_in_view(vec![None, None, None, Some(open_projects.clone())], cx),
                )
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
                // A ghost Button at the card's text edge: its icon on the
                // edge, its frame its label inset before it.
                .child(
                    button(
                        "settings-open-projects",
                        "Choose a project",
                        "folder",
                        false,
                    )
                    .track_focus(&open_projects)
                    .debug_selector(|| "settings-open-projects".into())
                    .ml(-ghost_pull(window))
                    .on_click(cx.listener(|this, _, window, cx| this.show_projects(window, cx))),
                )
                .into_any_element()
        };

        div()
            .size_full()
            // Tab and Shift-Tab walk the Your themes rows before the toolkit's
            // frame-bound tab order (`theme_editor::init`).
            .key_context(theme_editor::SETTINGS_CONTEXT)
            // Return in a field saves it from here, once per press
            // ([`Self::settings_field_enter`]).
            .on_key_down(cx.listener(Self::settings_field_enter))
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
                crate::native_accessibility::clips_children(div()
                    // Record even focus outside the page, so returning to the
                    // same control reveals it again.
                    .on_children_prepainted(self.theme_editor.page().observe())
                    .id("settings-scroll")
                    .debug_selector(|| "settings-scroll".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    // Tracked so that the Your themes row holding focus is
                    // revealed here too (`reveal_in_page`), as is every other
                    // control the keyboard focuses (`keep_in_view`).
                    .track_scroll(self.theme_editor.page_scroll())
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
                                        .on_children_prepainted(self.keep_in_view(vec![None, Some(retry.clone())], cx))
                                        .p_3()
                                        .rounded(px(8.))
                                        .bg(rgb(p.removed_background))
                                        .text_color(rgb(p.removed))
                                        .text_size(crate::appearance::ui_text(12.))
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .child(div().flex_1().min_w_0().child(error.to_string()))
                                        .when(error.starts_with("Could not save settings:"), |notice| {
                                            notice.child(Button::new("retry-save-settings").track_focus(&retry).label("Retry save").on_click(cx.listener(|this, _, window, cx| this.save_preferences(window, cx))))
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
                                                self.render_columns_controls(true, cx),
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
                    )),
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
    error: Option<crate::views::OperationFailure>,
    busy: bool,
    /// Whether the write has looked busy, which the page's write controls
    /// wait for before they dim (`workspace::BusyLook`).
    looks_busy: bool,
    status: String,
    profile: Option<gitturtle_core::GitProfile>,
    /// Whether that identity is loaded for the repository shown
    /// (`GitTurtle::loaded_identity`), and why none is
    /// (`GitTurtle::identity_wait`).
    identity_loaded: bool,
    identity_wait: Option<&'static str>,
    repository: Option<(PathBuf, String)>,
    /// Whether the shown tab is marked, which refuses Manage named
    /// profiles… with the mark's reason.
    marked: bool,
}

impl SettingsPage {
    pub(super) fn new(
        app: WeakEntity<GitTurtle>,
        inputs: [&Entity<InputState>; 5],
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let Some(app) = self.app.upgrade() else {
            return div().into_any_element();
        };
        let key = app.update(cx, |app, cx| {
            // This build's placeholders record the cards again, when the
            // page below is prepainted.
            app.picker_cards.0.borrow_mut().clear();
            app.ensure_theme_cards(cx);
            app.sync_theme_cards(cx);
            app.settings_page_key()
        });
        self.seen = Some(key);
        // Built for the width the page is drawn at in this frame, so its
        // breakpoints follow the room it has: beside the project list, or a
        // width the window's resizable columns give it, rather than the
        // window's.
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                crate::in_frame::in_frame(move |bounds, window, cx| {
                    app.update(cx, |app, cx| {
                        app.render_settings(bounds.size.width, window, cx)
                    })
                })
                .named("settings-page"),
            )
            .into_any_element()
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

impl CardPointer {
    /// The state [`CardProbe`]'s box shows in `style`, the text style it
    /// sets for its content.
    fn shown(style: &TextStyle) -> Self {
        Self {
            hovered: style.font_style == FontStyle::Italic,
            pressed: style.font_weight == FontWeight::BLACK,
        }
    }
}

/// What a picker card's page elements leave for [`LayerCard`]: the pointer
/// state [`CardProbe`] expected at prepaint and read at paint, and the
/// content mask at the card button's content ([`CardProbe`]'s prepaint),
/// under which the preview's hover ring was drawn.
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
    tag: Option<SharedString>,
    /// The description takes a second line when one cannot hold it, before
    /// it ends in an ellipsis: the Omarchy card's. Every other card keeps
    /// its description to one line.
    wrap: bool,
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
        tag: Option<SharedString>,
        selected: bool,
        active: appearance::Palette,
    ) -> Self {
        Self {
            palette,
            name,
            description,
            warnings,
            tag,
            wrap: false,
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
///
/// Its prepaint also stores the state the paint is expected to read, which
/// [`LayerCard`] prepaints the card's content under: before the paint, the
/// box's styles fall back to the hover and press its listeners recorded at
/// the input, and a hover is not shown after keyboard input. So the frame
/// that shows a hover change, a press or a switch prepaints the card under
/// its clip, and no further frame is drawn to correct it.
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
        let expected = CardPointer::shown(&window.text_style());
        self.0.pointer.set(CardPointer {
            hovered: expected.hovered && !window.last_input_was_keyboard(),
            ..expected
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
        _: &mut App,
    ) {
        self.0.pointer.set(CardPointer::shown(&window.text_style()));
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

/// A picker card body's last layout, which [`LayerCard`] draws around: the
/// miniature's box, the caption, and whether the caption's description took
/// a second line ([`CardKey`]'s `wrap`), so the caption grew into that box.
#[derive(Clone, Copy, Default)]
struct CardParts {
    place: Bounds<Pixels>,
    caption: Bounds<Pixels>,
    wrapped: bool,
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
/// state known at prepaint is the one [`CardProbe`] expected when the page
/// was last prepainted, or the last page paint's on a frame that replays
/// the page. When this frame's paint finds it changed, as a scroll under a
/// resting pointer can, the card asks for one more frame, which prepaints it
/// under the right clip.
struct LayerCard {
    text: TextStyleRefinement,
    body: AnyElement,
    miniature: Option<AnyElement>,
    /// The miniature box and the caption, as the body last laid them out.
    parts: std::rc::Rc<std::cell::Cell<CardParts>>,
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
                let CardParts { place, wrapped, .. } = self.parts.get();
                let mut miniature = div()
                    .w(place.size.width)
                    .h(place.size.height)
                    // Pinned as in the body: nothing inside draws text.
                    .text_color(rgb(self.palette.text))
                    // Read back by [`whole_rows`].
                    .when(wrapped, |this| this.italic())
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
        let caption = self.parts.get().caption;
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
    parts: std::rc::Rc<std::cell::Cell<CardParts>>,
    /// Test-only: builds of this body.
    #[cfg(test)]
    renders: usize,
    /// Test-only: the description's text as the last build laid it out, so
    /// a test reads the lines it wrapped to and where an ellipsis cut it.
    #[cfg(all(test, target_os = "linux"))]
    description: TextLayout,
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
            #[cfg(all(test, target_os = "linux"))]
            description: TextLayout::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn renders(&self) -> usize {
        self.renders
    }

    /// Test-only: the description as drawn, one line per wrapped line.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn drawn_description(&self) -> String {
        self.description.wrapped_text()
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
        let description = StyledText::new(key.description);
        let text = description.layout().clone();
        #[cfg(all(test, target_os = "linux"))]
        {
            self.description = text.clone();
        }
        let wrap = key.wrap;
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_children_prepainted(move |bounds, _, _| {
                if let [place, caption] = bounds[..] {
                    // The description was measured when the body was laid
                    // out, before this prepaint.
                    let wrapped = wrap
                        && text
                            .line_layouts()
                            .iter()
                            .map(|line| line.wrap_boundaries().len() + 1)
                            .sum::<usize>()
                            > 1;
                    parts.set(CardParts {
                        place,
                        caption,
                        wrapped,
                    });
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
                description,
                key.wrap,
                key.badge.is_some(),
                key.warnings,
                key.tag,
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
    /// Test-only: the changed-file rows the last paint drew.
    #[cfg(all(test, target_os = "linux"))]
    painted_rows: std::rc::Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
}

impl ThemePreviewBody {
    pub(super) fn new(palette: appearance::Palette) -> Self {
        Self {
            palette,
            #[cfg(test)]
            renders: 0,
            #[cfg(test)]
            palette_changes: 0,
            #[cfg(all(test, target_os = "linux"))]
            painted_rows: std::rc::Rc::default(),
        }
    }

    /// Test-only: the changed-file rows the last paint drew, top to bottom.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn painted_rows(&self) -> Vec<Bounds<Pixels>> {
        self.painted_rows.borrow().clone()
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
        #[cfg(all(test, target_os = "linux"))]
        let painted = std::rc::Rc::clone(&self.painted_rows);
        // One painted element, like `swatch_run`: a frame that rebuilds every
        // card, as a switch's click or a Tab's focus does through
        // `Window::refresh`, lays out one node per miniature, not forty.
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let whole = whole_rows(&window.text_style());
                #[cfg(all(test, target_os = "linux"))]
                let mut drawn = Vec::new();
                paint_miniature(bounds, p, whole, window, |_row| {
                    #[cfg(all(test, target_os = "linux"))]
                    drawn.push(_row);
                });
                #[cfg(all(test, target_os = "linux"))]
                painted.replace(drawn);
            },
        )
        .size_full()
    }
}

/// Whether a picker card's miniature draws only the rows its box holds whole:
/// once the card's caption has taken a second line ([`CardParts`]'s
/// `wrapped`) and grown into the box, a row it has no room for is left out
/// rather than cut short by the caption; with one line the miniature draws as
/// every other card's. [`LayerCard`] says so through the text style it draws
/// the miniature under, italic, since nothing in the miniature draws text and
/// GPUI's reuse key for the cached miniature compares that style, so a change
/// draws the miniature again.
fn whole_rows(style: &TextStyle) -> bool {
    style.font_style == FontStyle::Italic
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
///
/// With `whole_rows` a row or stripe that would end below the box is left
/// out, so the Omarchy card's two-line caption, which takes about 12 px of
/// its 70 px box, leaves three whole rows rather than a fourth cut short.
/// `on_row` is given each changed-file row it draws.
fn paint_miniature(
    bounds: Bounds<Pixels>,
    p: appearance::Palette,
    whole_rows: bool,
    window: &mut Window,
    mut on_row: impl FnMut(Bounds<Pixels>),
) {
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
    let fits = |top: Pixels, tall: Pixels| !whole_rows || top + tall <= height;
    for row in 0..4 {
        let color = if row == 1 { p.accent } else { p.border };
        let y = bar + px(5.) + px(8.) * row as f32;
        if fits(y, px(3.)) {
            quad(px(5.), y, px(15.), px(3.), color, px(1.5));
        }
    }
    let stripe_width = width - px(57.);
    for row in 0..4 {
        let top = bar + px(5.) + px(10.) * row as f32;
        if !fits(top, px(10.)) {
            break;
        }
        on_row(Bounds::new(
            bounds.origin + point(px(26.), top),
            size(width - px(26.), px(10.)),
        ));
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

/// How long choosing the Omarchy card waits for its read before the cached
/// palette applies (`GitTurtle::choose_omarchy`).
#[cfg(target_os = "linux")]
pub(super) const OMARCHY_CHOICE_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

/// Linux: the Omarchy card's choices (`GitTurtle::choose_omarchy`).
#[cfg(target_os = "linux")]
#[derive(Default)]
pub(super) struct OmarchyChoice {
    /// Bumped by every choice, so a wait that outlived its choice applies
    /// nothing.
    made: u64,
    /// The choice waiting for its read, with when it was made for
    /// `gitturtle.theme_apply_frame_ms`.
    waiting: Option<(u64, Option<std::time::Instant>)>,
}

/// What a picker card shows: the theme it selects, the palette its miniature
/// draws, its caption and the count of its readability findings.
struct ThemeCard {
    selection: ThemeSelection,
    palette: appearance::Palette,
    name: SharedString,
    description: SharedString,
    warnings: usize,
    /// The whole state when the caption shortens it (the Omarchy card), for
    /// the tooltip.
    detail: Option<SharedString>,
    /// The name for assistive technology when "‹name› theme" is not all it
    /// needs: an unavailable Omarchy card adds its state, as the kit's
    /// button takes a label but no description.
    accessible_name: Option<SharedString>,
    /// A muted word beside the name, "Unavailable" on an Omarchy card whose
    /// theme cannot be read; the description then names the palette in use.
    tag: Option<SharedString>,
    /// The description takes a second line when one cannot hold it: the
    /// Omarchy card's, which names a desktop theme up to 64 bytes long.
    wrap: bool,
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
                StyledText::new(description),
                false,
                selected,
                warnings,
                None,
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
/// filled by [`LayerCard`], outside its cached body. `wrap` lets the
/// description take a second line; otherwise it is one line, cut short with
/// an ellipsis.
#[expect(clippy::too_many_arguments)]
fn preview_caption(
    p: appearance::Palette,
    label: SharedString,
    description: StyledText,
    wrap: bool,
    selected: bool,
    warnings: usize,
    tag: Option<SharedString>,
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
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(p.text))
                .child(label)
                .children(appearance::text_probe(move || {
                    format!("theme-name-{miniature}")
                }));
            if selected || warnings > 0 || tag.is_some() {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(name.flex_1())
                    .when_some(tag, |row, tag| {
                        // Beside the name, in the warning count's muted
                        // 10 px, so the description keeps its line for the
                        // palette in use.
                        row.child(
                            div()
                                .debug_selector(move || format!("theme-tag-{miniature}"))
                                .flex_shrink_0()
                                .text_size(crate::appearance::ui_text(10.))
                                .line_height(relative(1.3))
                                .text_color(rgb(p.muted))
                                .child(tag),
                        )
                    })
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
        .child({
            let line = div()
                .debug_selector(move || format!("theme-description-{miniature}"))
                .text_size(crate::appearance::ui_text(10.))
                .line_height(relative(1.3))
                .font_weight(FontWeight::NORMAL)
                .text_color(rgb(p.muted));
            if wrap {
                // Wrapped at word boundaries into a second line when one
                // cannot hold it, then ended in an ellipsis. The caption
                // grows by the second line and the miniature above gives it
                // the room ([`whole_rows`]); the card keeps its height. The
                // card's inherited text style does not wrap
                // (the toolkit button's, which [`LayerCard`] pins), so the
                // line sets its own, and clips to its box as `truncate`
                // does for one line.
                line.overflow_hidden()
                    .whitespace_normal()
                    .line_clamp(2)
                    .text_ellipsis()
            } else {
                line.truncate()
            }
            .child(description)
        })
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

/// An element's `on_children_prepainted` listener, as
/// [`GitTurtle::keep_row_in_view`] returns two of.
trait Prepainted: Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {}

impl<F: Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static> Prepainted for F {}

/// A setting's title and description, read together.
fn setting_description(
    title: &'static str,
    description: impl Into<SharedString>,
    cx: &App,
) -> AnyElement {
    read_setting_text(title, &description.into(), &[], cx)
}

/// Settings' two display preferences, each a row of three choices
/// ([`GitTurtle::render_display_setting`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DisplaySetting {
    ReduceMotion,
    IncreaseContrast,
}

impl DisplaySetting {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 2] = [Self::ReduceMotion, Self::IncreaseContrast];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::ReduceMotion => "Reduce motion",
            Self::IncreaseContrast => "Increase contrast",
        }
    }

    /// The ID its segments and their focus handles are built from.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::ReduceMotion => "settings-reduce-motion",
            Self::IncreaseContrast => "settings-increase-contrast",
        }
    }

    pub(crate) fn choice(self, settings: &AppSettings) -> DisplayChoice {
        match self {
            Self::ReduceMotion => settings.reduce_motion,
            Self::IncreaseContrast => settings.increase_contrast,
        }
    }

    /// Whether the desktop's answer asks for this preference; a desktop
    /// that has not answered asks for nothing.
    fn asks(self, desktop: Option<native_accessibility::DisplayPreferences>) -> bool {
        desktop.is_some_and(|desktop| match self {
            Self::ReduceMotion => desktop.reduce_motion,
            Self::IncreaseContrast => desktop.increase_contrast,
        })
    }

    /// What `choice` does, following a desktop that `asks` or does not.
    /// macOS is named as the system that asks there.
    pub(crate) fn description(self, choice: DisplayChoice, asks: bool) -> &'static str {
        #[cfg(target_os = "macos")]
        let (motion_asks, motion_does_not, contrast_asks, contrast_does_not) = (
            "macOS asks for reduced motion.",
            "macOS does not ask for reduced motion.",
            "macOS asks for more contrast.",
            "macOS does not ask for more contrast.",
        );
        #[cfg(not(target_os = "macos"))]
        let (motion_asks, motion_does_not, contrast_asks, contrast_does_not) = (
            "Your desktop asks for reduced motion.",
            "Your desktop does not ask for reduced motion.",
            "Your desktop asks for more contrast.",
            "Your desktop does not ask for more contrast.",
        );
        match (self, choice) {
            (Self::ReduceMotion, DisplayChoice::Follow) if asks => motion_asks,
            (Self::ReduceMotion, DisplayChoice::Follow) => motion_does_not,
            (Self::ReduceMotion, DisplayChoice::On) => {
                "Transitions finish at once and GIFs and model clips stay paused, whatever the desktop asks."
            }
            (Self::ReduceMotion, DisplayChoice::Off) => {
                "GitTurtle's short transitions play, whatever the desktop asks."
            }
            (Self::IncreaseContrast, DisplayChoice::Follow) if asks => contrast_asks,
            (Self::IncreaseContrast, DisplayChoice::Follow) => contrast_does_not,
            (Self::IncreaseContrast, DisplayChoice::On) => {
                "Secondary text, dividers and control edges are drawn with more contrast, in every theme."
            }
            (Self::IncreaseContrast, DisplayChoice::Off) => {
                "Each theme's own contrast, whatever the desktop asks."
            }
        }
    }

    /// Every description the row can show, which it reserves the height of.
    pub(crate) fn descriptions(self) -> [&'static str; 4] {
        [
            self.description(DisplayChoice::Follow, true),
            self.description(DisplayChoice::Follow, false),
            self.description(DisplayChoice::On, false),
            self.description(DisplayChoice::Off, false),
        ]
    }
}

/// A display choice's segment label: the first follows the desktop, which
/// on macOS is the system.
pub(crate) fn display_choice_label(choice: DisplayChoice) -> &'static str {
    match choice {
        #[cfg(target_os = "macos")]
        DisplayChoice::Follow => "Follow the system",
        #[cfg(not(target_os = "macos"))]
        DisplayChoice::Follow => "Follow the desktop",
        DisplayChoice::On => "On",
        DisplayChoice::Off => "Off",
    }
}

/// `setting`'s segment `index`, the choice [`DisplayChoice::ALL`] holds
/// there, built as List density's segments: the shared helper's look, its
/// toggled state the selection, tracking `focus`, and named by the setting
/// and the choice ("Reduce motion: Off").
pub(crate) fn display_segment(
    setting: DisplaySetting,
    index: usize,
    selected: bool,
    focus: &FocusHandle,
) -> Button {
    let key = setting.key();
    let label = display_choice_label(DisplayChoice::ALL[index]);
    let name = format!("{}: {label}", setting.title());
    #[cfg(test)]
    SEGMENTS.with(|segments| {
        segments
            .borrow_mut()
            .insert(format!("{key}-{index}"), (name.clone(), selected))
    });
    Button::new((key, index))
        .track_focus(focus)
        .small()
        .with_variant(appearance::control_button_variant(selected))
        .label(label)
        .accessibility_label(name)
        .selected(selected)
        .toggled(selected)
        .debug_selector(move || format!("{key}-{index}"))
        .when(selected, |button| {
            button.hover(appearance::control_selected_hover)
        })
}

#[cfg(test)]
thread_local! {
    /// The accessible name and toggled state each display segment was last
    /// built with, by its selector: the test platform builds no
    /// accessibility tree.
    static SEGMENTS: std::cell::RefCell<
        std::collections::BTreeMap<String, (String, bool)>,
    > = std::cell::RefCell::default();
}

/// The accessible name and toggled state the display segment `selector`
/// was last built with.
#[cfg(test)]
pub(crate) fn segment_built(selector: &str) -> Option<(String, bool)> {
    SEGMENTS.with(|segments| segments.borrow().get(selector).cloned())
}

/// A field's title and description, the title the field's name, so the
/// text reads only the description: the Default branch's read "Default
/// branch. Used when creating a new repository." before the field named
/// "Default branch".
fn field_description(
    title: &'static str,
    description: impl Into<SharedString>,
    cx: &App,
) -> AnyElement {
    let description = description.into();
    let label = description.to_string();
    setting_text_read_as(title, &description, label, &[], cx)
}

/// A setting's title and description, read together, the description as
/// tall as the tallest of it and `reserve`.
fn read_setting_text(
    title: &'static str,
    description: &SharedString,
    reserve: &[SharedString],
    cx: &App,
) -> AnyElement {
    let label = format!("{title}. {description}");
    setting_text_read_as(title, description, label, reserve, cx)
}

/// A setting's title and description, read as `label`, the description as
/// tall as the tallest of it and `reserve`.
fn setting_text_read_as(
    title: &'static str,
    description: &SharedString,
    label: String,
    reserve: &[SharedString],
    cx: &App,
) -> AnyElement {
    let selector = label.clone();
    setting_text(
        native_accessibility::static_text(
            div()
                .id(title)
                // Tests read the accessible name here; other builds skip it.
                .debug_selector(move || format!("setting-description: {selector}")),
            label,
        ),
        title,
        description,
        reserve,
        cx,
    )
    .into_any_element()
}

/// The titles of the switches' rows, which name their switches.
pub(crate) const FOLLOW_SYSTEM: &str = "Follow system appearance";
const REOPEN_LAST: &str = "Restore repository tabs on startup";
const PROJECT_PANE: &str = "Show the project list";
/// The project list switch's description where the window draws the pane,
/// or it is off; where the window holds it back, the switch says why instead
/// (`project_pane::HELD_BACK_DESCRIPTION`).
const PROJECT_PANE_DESCRIPTION: &str =
    "Keep a pane on the far left to jump between known projects and sort them into groups.";

/// Why History's Subject column cannot be hidden, as the pill beside its
/// checkbox says.
const ALWAYS_SHOWN: &str = "Always shown";

/// The checkbox that shows History's column `id`, the `index`th, checked
/// while `visible`, without its focus or click. Subject's is unavailable,
/// since the column is always shown, and says so as its description, as the
/// pill beside it does: announced unavailable, it gave no reason, the pill
/// being text of its own after it.
fn column_checkbox(index: usize, id: ColumnId, visible: bool) -> Checkbox {
    Checkbox::new(("history-column", index))
        .label(id.label())
        .checked(visible)
        .disabled(id == ColumnId::Subject)
        .when(id == ColumnId::Subject, |checkbox| {
            checkbox.aria_description(ALWAYS_SHOWN)
        })
}

/// A Settings switch whose row is the control: its `title` and
/// `description` are drawn before its track (the kit's `Switch::leading`),
/// the description as tall as the tallest of it and `reserve`, the other
/// descriptions the row can show, so replacing one with another moves
/// nothing. The switch is named `name`, its title or that title said in
/// full, and described by `description`, so assistive technology reads the
/// row once, with the switch: the row's text read the title before the
/// description, the switch's name again, and the description apart from
/// the switch. A press anywhere on the row toggles the switch, hovering it
/// lifts the track, and the title takes the disabled label's dim with the
/// switch, as Follow system appearance's label did. The track is centred on
/// the title's first line ([`on_title_line`]), its top never above the
/// row's. The switch draws no label of
/// its own: Follow system appearance drew one beside its track as well as
/// its title, which at 18 pt in a 461 px window wrapped to three lines
/// beside it.
pub(crate) fn switch_row(
    id: &'static str,
    title: &'static str,
    name: &'static str,
    description: impl Into<SharedString>,
    reserve: &[SharedString],
    cx: &App,
) -> Switch {
    let description = description.into();
    #[cfg(test)]
    SWITCHES.with(|switches| {
        switches
            .borrow_mut()
            .insert(id, (name, description.to_string()))
    });
    // The switch fills its row, which the page reveals with a ring's room
    // around it, so the track's ring may take that room above the row and
    // no more: the track never rises above the row's top. At 13 pt the 20 px
    // track, taller than the title's line (19.5 px on a 2x display, 19 px at
    // 1x), rose 0.25 px above it (0.5 px at 1x), and the page's edge cut its
    // ring with the row revealed at the top of the view.
    let ring = appearance::button_ring_room(cx);
    // The kit leaves 8 px before the track; the rows beside keep 12.
    Switch::new(id)
        .w_full()
        .accessibility_label(name)
        .accessibility_description(description.clone())
        .leading(setting_text(div().pr_1(), title, &description, reserve, cx))
        // A ring's room above the title: the track's top stays on the row's.
        .place_track(move |track| TitleLine::new(track, ring, ring).into_any_element())
}

#[cfg(test)]
thread_local! {
    /// The name and description each Settings switch was last built with,
    /// by ID: the test platform builds no accessibility tree.
    static SWITCHES: std::cell::RefCell<
        std::collections::BTreeMap<&'static str, (&'static str, String)>,
    > = std::cell::RefCell::default();
}

/// What the Settings switch `id` was last built with: its name and its
/// description.
#[cfg(test)]
pub(crate) fn switch_built(id: &str) -> Option<(&'static str, String)> {
    SWITCHES.with(|switches| switches.borrow().get(id).cloned())
}

/// A setting title's line height, GPUI's default (`phi`, the golden ratio),
/// set on the title so its first line box, which the row's trailing control
/// is centred on, is known while the row is built.
const TITLE_LINE_HEIGHT: f32 = std::f32::consts::GOLDEN_RATIO;

/// The height of a setting title's first line box in `window`, as GPUI lays
/// a text line out: the line height snapped to the window's device pixels,
/// 19.5 px at 13 pt and 27 px at 18 pt on a 2x display.
fn title_line(window: &Window) -> Pixels {
    window.pixel_snap(appearance::ui_text(12.) * TITLE_LINE_HEIGHT)
}

/// One rem, the rem `appearance::apply_text_sizes` sets: the top padding
/// (`pt_4`) a separated Settings row and the Your themes card keep above
/// their titles, and less than the gap (`gap_5`) between spaced rows.
fn rem() -> Pixels {
    appearance::ui_text(13.)
}

/// `control`, a Settings row's trailing control beside its text, centred on
/// the title's first line box: its own text then shares the title's baseline
/// at every size, and it stays put as the description rewraps. With their
/// tops on that line instead, a 36 px segment group sat 4 px below the
/// title's baseline at 18 pt and a switch 3.5 px above its cap centre. A
/// control taller than the line reaches above the title, but never so far
/// that its focus ring would pass `room` above it, into the row above or its
/// separator: such a control is lowered just enough instead. The slot itself
/// is one title line tall, so the row keeps its text's height. A control
/// stacked below its text is placed without it, as is History's Columns
/// panel, where the steppers stand under the panel's paragraph rather than
/// beside a row's title; a row whose control wraps below its text when
/// narrow is a [`wrapping_row`].
fn on_title_line(control: impl IntoElement, room: Pixels, cx: &App) -> TitleLine {
    TitleLine::new(
        control.into_any_element(),
        room,
        appearance::button_ring_room(cx),
    )
}

/// [`on_title_line`]'s slot for a control whose focus ring reaches `ring`
/// outside its edge, built as it is drawn, when the window's device pixels,
/// which the title's line is snapped to, are known.
#[derive(IntoElement)]
struct TitleLine {
    control: AnyElement,
    room: Pixels,
    ring: Pixels,
}

impl TitleLine {
    fn new(control: AnyElement, room: Pixels, ring: Pixels) -> Self {
        Self {
            control,
            room,
            ring,
        }
    }
}

impl RenderOnce for TitleLine {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        title_line_slot(self.control, title_line(window), self.room, self.ring)
    }
}

/// The slot [`TitleLine`] draws for a title line `line` tall.
fn title_line_slot(control: AnyElement, line: Pixels, room: Pixels, ring: Pixels) -> Div {
    // How far above the title's top the control's edge may go.
    let reach = room - ring;
    div().flex_shrink_0().h(line).flex().flex_col().child(
        // From `reach` above the title's line to as far below it, so a
        // control centred in it is centred on the line; one too tall for
        // it makes it taller downwards and starts at its top.
        div()
            .flex_shrink_0()
            .mt(-reach)
            .min_h((line + reach * 2.).max(Pixels::ZERO))
            .flex()
            .items_center()
            .child(control),
    )
}

/// Where a control `height` tall starts beside a title whose first line box
/// is `line` tall, from the title's top: centred on that line, unless that
/// takes its edge more than `reach` above the title, where it starts
/// `reach` above. [`title_line_slot`] places a control there by layout.
fn beside_title(line: Pixels, height: Pixels, reach: Pixels) -> Pixels {
    ((line - height) / 2.).max(-reach)
}

/// The least width a [`wrapping_row`]'s text keeps beside its control: 180 px
/// at the default interface size, scaled with it. At 461 px and 18 pt Your
/// themes' header, which never wrapped, left its text 66 px beside its
/// actions, its title on two lines and its description a word a line.
fn wrapping_text_min() -> Pixels {
    appearance::ui_size(180.)
}

/// `row` holding a setting's `text` and its trailing `control`, `height`
/// tall, that wraps: beside the text while the text keeps at least
/// [`wrapping_text_min`], the control is centred on the title's first line
/// as [`on_title_line`] centres one, its focus ring kept within `room` above
/// the title; narrower, it starts a line of its own at the text's left edge,
/// the row's gap (a `gap_3`) below the text, with nothing reserved above it:
/// its frame does, or, for a ghost Button, its content
/// ([`WrappingRow::ghost_lead`]). Only the text sizes' rows and Your themes'
/// header wrap; the others stack below a breakpoint or keep their control
/// beside their text.
fn wrapping_row(
    row: Div,
    text: impl IntoElement,
    control: impl IntoElement,
    height: Pixels,
    room: Pixels,
    cx: &App,
) -> WrappingRow {
    WrappingRow {
        row,
        text: text.into_any_element(),
        control: control.into_any_element(),
        height,
        room,
        ring: appearance::button_ring_room(cx),
        lead: Pixels::ZERO,
    }
}

/// [`wrapping_row`]'s row, built as it is drawn, when the window's device
/// pixels, which the title's line is snapped to, are known.
#[derive(IntoElement)]
struct WrappingRow {
    row: Div,
    text: AnyElement,
    control: AnyElement,
    height: Pixels,
    room: Pixels,
    ring: Pixels,
    /// How far the control's content starts inside its frame, where that
    /// frame draws nothing at rest ([`Self::ghost_lead`]).
    lead: Pixels,
}

impl WrappingRow {
    /// The row for a control that leads with a ghost Button, transparent at
    /// rest, whose content (its leading icon, else its label) starts `inset`
    /// inside its frame: on a line of its own, the control's frame stands
    /// that far before the text's edge, its hover fill and focus ring
    /// spreading into the padding before it, so what a person sees starts at
    /// the edge (DESIGN.md, "One left edge"). Beside the text it stays where
    /// it was. Standing its frame at the edge, Your themes' New theme…, a
    /// ghost Button, started its "+" icon 10 px past the title at 13 pt and
    /// 14 px at 18 pt on a 2x display, and the cross itself, inset in its
    /// icon, 12.5 and 17 px past it: a third left edge in the card.
    fn ghost_lead(mut self, inset: Pixels) -> Self {
        self.lead = inset;
        self
    }
}

impl RenderOnce for WrappingRow {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let top = beside_title(title_line(window), self.height, self.room - self.ring);
        let gap = window.rem_size() * 0.75;
        // On the device pixel the Button's own padding is laid out at, so its
        // content lands on the text's edge rather than a rounding beside it.
        let lead = window.pixel_snap(self.lead);
        self.row
            .flex()
            .flex_wrap()
            .items_start()
            // Beside the text, the control's margin takes back the lead the
            // gap adds, so it stays the gap past the text and the row wraps
            // where it did: the control's outer width and the gap sum to what
            // they were. A line of its own has no gap before the control, so
            // the same margin stands its frame the lead before the text's
            // edge.
            .gap_x(gap + lead)
            // Beside the text, the control's margin places it on the title's
            // line. A line of its own starts `top` short of the gap, so the
            // same margin puts the control's edge the gap below the text:
            // placed in a title line's slot there, as beside the text, a
            // 28 px control started 4.25 px above the gap at 13 pt.
            .gap_y(gap - top)
            // At least its least width beside the control, which wraps
            // first; alone on a narrower row, the row's width.
            .child(
                div()
                    .flex_grow(1.)
                    .flex_shrink(1.)
                    .flex_basis(wrapping_text_min())
                    .min_w_0()
                    .child(self.text),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .h(self.height)
                    .mt(top)
                    .ml(-lead)
                    .flex()
                    .items_center()
                    .child(self.control),
            )
    }
}

/// `element` holding a setting's title and description, the description as
/// tall as the tallest of it and `reserve`.
fn setting_text<E: Styled + ParentElement>(
    element: E,
    title: &'static str,
    description: &SharedString,
    reserve: &[SharedString],
    cx: &App,
) -> E {
    let p = palette(cx);
    element
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .debug_selector(move || format!("setting-title: {title}"))
                .text_size(crate::appearance::ui_text(12.))
                .line_height(relative(TITLE_LINE_HEIGHT))
                .font_weight(FontWeight::MEDIUM)
                .child(title)
                .children(appearance::text_probe(move || {
                    format!("setting-title: {title}")
                })),
        )
        .child(
            div()
                .debug_selector(move || format!("setting-detail: {title}"))
                .grid()
                .grid_cols(1)
                .text_size(crate::appearance::ui_text(11.))
                .line_height(relative(1.5))
                .text_color(rgb(p.muted))
                .child(div().row_start(1).col_start(1).child(description.clone()))
                .children(
                    reserve
                        .iter()
                        .filter(|text| *text != description)
                        .map(|text| {
                            div()
                                .row_start(1)
                                .col_start(1)
                                .invisible()
                                .child(text.clone())
                        }),
                ),
        )
}

/// `control`, an action of the Git identity card with element ID `id` and
/// label `label`, refused for `reason`: on a marked tab the mark's, as Fetch
/// is refused there (decided 2026-11-11: an entry is offered only where it
/// can act), and for Save repository identity and Reset edits with no
/// identity loaded the read's, as Fetch gives it (`GitTurtle::identity_wait`;
/// decided 2026-11-15). Its tooltip is what it does, `purpose` or its
/// label, then the reason, and its accessible name its label and the
/// reason, as Fetch's is, since a refused control leaves the Tab order and
/// its tooltip shows only under the pointer. With no reason it keeps
/// `purpose` as its tooltip, where it has one. Formatted only where there
/// is a reason, as the cached page is built again.
fn says_why(
    control: Button,
    id: &'static str,
    label: &'static str,
    purpose: Option<&'static str>,
    reason: Option<&'static str>,
) -> Button {
    let Some(reason) = reason else {
        return match purpose {
            Some(purpose) => control.tooltip(purpose),
            None => control,
        };
    };
    let name: SharedString = format!("{label}: {reason}").into();
    let tooltip = format!("{} · {reason}", purpose.unwrap_or(label));
    crate::views::note_control_tooltip(&id.into(), &tooltip);
    control.tooltip(tooltip).accessibility_label(name)
}

/// A labelled field which a write leaves `(refused, dimmed)`
/// (`workspace::refusing_input`), named by its label, the words drawn above
/// it: its placeholder is no name, yet the Git identity's Name and Email
/// fields, named by nothing else, were announced by it ("Your name").
///
/// On a marked tab, `marked` holds the identity line, which the field gives
/// assistive technology as its description, so a screen reader says why on
/// it as the refused buttons' names do, and the field leaves the Tab order,
/// as those dimmed buttons do: a disabled field draws no focus ring, so Tab
/// stopped on Name and Email with nothing drawn to show it (decided
/// 2026-11-14). The tab stop is the focus handle's own, which every copy
/// GPUI tracks reads.
fn settings_field(
    label: &'static str,
    input: &Entity<InputState>,
    (refused, dimmed): (bool, bool),
    marked: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    let _ = input.focus_handle(cx).tab_stop(marked.is_none());
    div()
        .flex()
        .flex_col()
        .gap_2()
        .text_size(crate::appearance::ui_text(12.))
        .text_color(rgb(palette(cx).text))
        .child(
            // Medium, as the setting titles beside it are; the field's own
            // text stays regular.
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(label)
                .children(appearance::text_probe(|| {
                    format!("settings-field-label: {label}")
                })),
        )
        .child(crate::workspace::refusing_input(
            Input::new(input)
                .aria_label(label)
                .when_some(marked, |input, line| input.aria_description(line)),
            || format!("settings-field-{label}"),
            refused,
            dimmed,
        ))
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
        .debug_selector(move || format!("settings-section: {title}"))
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
        let split = Some(Arc::new(SplitPresentation::prepare(
            &old,
            &new,
            &presentation,
        )));
        Arc::new(Content::Text {
            diagrams: None,
            markdown: None,
            patch: patch.into(),
            old,
            new,
            presentation,
            split,
            split_partial: None,
            partial: None,
            partial_unavailable: None,
            lfs_missing: [false; 2],
            lfs_failed: [None; 2],
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
        crate::views::tests::init_app(cx);
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
#[path = "settings_picker_tests.rs"]
mod picker_tests;

/// History's Columns panel, which keeps its controls off the title line.
#[cfg(test)]
mod columns_popover_tests {
    use super::*;
    use core::prelude::v1::test;

    /// The node `checkbox`, a kit Checkbox, writes for assistive technology
    /// as a window draws it.
    fn checkbox_node(
        cx: &mut gpui::VisualTestContext,
        checkbox: impl Fn() -> Checkbox + 'static,
    ) -> gpui::accesskit::Node {
        crate::views::tests::view_node(cx, move |window, cx| {
            use gpui::Element as _;
            fn of_type<'a, E: gpui::Element>(
                _witness: &E,
                element: &'a mut gpui::AnyElement,
            ) -> Option<&'a mut E> {
                element.downcast_mut()
            }
            let mut outer = gpui::RenderOnce::render(checkbox(), window, cx).into_element();
            let (_, mut layout) = outer.request_layout(None, None, window, cx);
            let mut base = (&mut layout as &mut dyn std::any::Any)
                .downcast_mut::<Option<gpui::AnyElement>>()
                .and_then(Option::take)
                .expect("the kit's Checkbox renders a base Checkbox");
            let witness =
                gpui::RenderOnce::render(gpui_kit::base::Checkbox::new("witness"), window, cx)
                    .into_element();
            let element = of_type(&witness, &mut base).expect("the base element writes the node");
            let mut node = gpui::accesskit::Node::new(element.a11y_role().expect("a role"));
            element.write_a11y_info(&mut node);
            node
        })
    }

    /// History's Subject column checkbox, which cannot hide the column, is
    /// announced unavailable with its reason, "Always shown", as the pill
    /// beside it draws it; the other columns' checkboxes are available and
    /// describe nothing. Announced unavailable, it gave no reason: the pill
    /// is text of its own after it.
    #[gpui::test]
    async fn the_subject_column_checkbox_says_why_it_is_unavailable(cx: &mut TestAppContext) {
        let (_fixture, _app, cx) = crate::views::tests::history_window(cx).await;
        for (index, id) in ColumnId::ALL.into_iter().enumerate() {
            let node = checkbox_node(cx, move || column_checkbox(index, id, true));
            assert_eq!(node.label(), Some(id.label()), "{id:?}");
            if id == ColumnId::Subject {
                assert!(node.is_disabled(), "Subject is announced unavailable");
                assert_eq!(node.description(), Some(ALWAYS_SHOWN), "with its reason");
            } else {
                assert!(!node.is_disabled(), "{id:?} is available");
                assert_eq!(node.description(), None, "{id:?}");
            }
        }
    }

    /// History's Columns panel lays its graph lane spacing out as it did
    /// before Settings centred its rows' controls on their titles' first
    /// lines: the steppers top their row and lie inside it, and the column
    /// list starts the panel's 0.75 rem gap below that row, at 13 and 18 pt
    /// in a 1,280 and a 461 px window. The title-line slot belongs beside a
    /// Settings row's title; in the panel it lifted the steppers above
    /// their row, 4.25 px at 13 pt and 6 px at 18 pt.
    #[gpui::test]
    async fn the_columns_popover_tops_its_steppers_on_their_row(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let code = cx.read(|cx| app.read(cx).settings.code_text_size);
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.column_menu = true;
                cx.notify();
            })
        });
        crate::views::tests::settle(&app, cx).await;
        for width in [1280., 461.] {
            for interface in [13, 18] {
                let at = format!("{interface} pt at {width} px");
                cx.simulate_resize(size(px(width), px(1200.)));
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        app.settings.interface_text_size = interface;
                        appearance::apply_text_sizes(interface, code, window, cx);
                        cx.notify();
                    })
                });
                crate::views::tests::settle(&app, cx).await;
                cx.update(|window, cx| window.draw(cx).clear(cx));
                let (Some(row), Some(steppers), Some(first)) = (
                    cx.debug_bounds("graph-spacing-row"),
                    cx.debug_bounds("graph-spacing-controls"),
                    cx.debug_bounds("history-column-row-0"),
                ) else {
                    panic!("{at}: the panel draws its graph lane spacing and columns");
                };
                let half = cx.update(|window, _| px(0.5 / window.scale_factor()));
                assert!(
                    steppers.top() == row.top() && steppers.bottom() <= row.bottom(),
                    "{at}: the steppers {steppers:?} do not top their row {row:?}"
                );
                let gap = appearance::ui_size(9.75);
                assert!(
                    (first.top() - row.bottom() - gap).abs() <= half,
                    "{at}: the column list starts at {:?}, not {gap:?} below the row's \
                     foot at {:?}",
                    first.top(),
                    row.bottom()
                );
            }
        }
    }
}

/// Settings' own segmented controls as a real window draws them.
#[cfg(test)]
mod segment_tests {
    use super::picker_tests::{open_app, settle};
    use super::*;
    use core::prelude::v1::test;

    /// In two dark and two light palettes, two of them with a moved control
    /// label, every density looks like History's segments, the shared helper.
    /// Selected, it rests on the palette's `selected`, hovers to its
    /// `selected_hover`, and keeps the whole Button, focus ring included, at
    /// full opacity. Unselected, it has no fill at rest, the palette's control
    /// fill of `hover` under the pointer and of `selected` while held pressed,
    /// rather than the ghost's translucent fills. Only fills and rings are
    /// checked; the test platform paints no text, so a label's color goes
    /// unchecked.
    #[gpui::test]
    fn every_density_rests_hovers_and_presses_like_the_shared_helper(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.density),
            Density::Comfortable
        );
        let selector_of = |wanted: Density| -> &'static str {
            let index = Density::ALL.iter().position(|density| *density == wanted);
            format!("settings-density-{}", index.unwrap()).leak()
        };
        let (selector, comfortable) = (
            selector_of(Density::Compact),
            selector_of(Density::Comfortable),
        );
        let unselected = shown(cx, selector).expect("the Compact density is drawn");
        // Tab from the Your themes card passes the text size controls before it
        // reaches the density segments; Space selecting Compact shows that is
        // where focus is.
        let card = cx.read(|cx| app.read(cx).theme_editor.card_focus(cx));
        cx.update(|window, cx| window.focus(&card, cx));
        let mut reached = false;
        for _ in 0..24 {
            cx.update(|window, cx| window.focus_next(cx));
            settle(cx);
            if !appearance::painted_button(cx, unselected).1.is_empty() {
                reached = true;
                break;
            }
        }
        assert!(
            reached,
            "Tab from the Your themes card reaches Compact within 24 stops"
        );
        let compact = cx
            .update(|window, cx| window.focused(cx))
            .expect("a focused density");
        let space = Keystroke::parse("space").expect("a keystroke");
        cx.simulate_event(KeyDownEvent {
            keystroke: space.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke: space });
        settle(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.density),
            Density::Compact,
            "Space on the focused density selects Compact"
        );
        for choice in [
            ThemeChoice::Midnight,
            ThemeChoice::Porcelain,
            ThemeChoice::KanagawaLotus,
            ThemeChoice::OneDark,
        ] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.settings.follow_system = false;
                    app.settings.theme = ThemeSelection::BuiltIn(choice);
                    app.apply_appearance(window, cx);
                });
                window.blur(cx);
            });
            cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
            settle(cx);
            let segment = shown(cx, selector).expect("the selected density is drawn");
            let other = shown(cx, comfortable).expect("the Comfortable density is drawn");
            let name = format!("{choice:?} Compact density");
            let other_name = format!("{choice:?} Comfortable density");
            appearance::assert_selected_button(
                cx,
                &name,
                segment,
                appearance::SelectedState::Resting,
            );
            appearance::assert_unselected_button(
                cx,
                &other_name,
                other,
                appearance::UnselectedState::Resting,
            );
            cx.simulate_mouse_move(other.center(), None, Modifiers::default());
            settle(cx);
            appearance::assert_unselected_button(
                cx,
                &other_name,
                other,
                appearance::UnselectedState::Hovered,
            );
            cx.simulate_mouse_down(other.center(), MouseButton::Left, Modifiers::default());
            settle(cx);
            appearance::assert_unselected_button(
                cx,
                &other_name,
                other,
                appearance::UnselectedState::Pressed,
            );
            // Released off the segment, the press selects nothing.
            let away = point(px(0.), px(0.));
            cx.simulate_mouse_move(away, Some(MouseButton::Left), Modifiers::default());
            cx.simulate_mouse_up(away, MouseButton::Left, Modifiers::default());
            settle(cx);
            assert_eq!(
                cx.read(|cx| app.read(cx).settings.density),
                Density::Compact,
                "{other_name} released off the segment selects nothing"
            );
            cx.simulate_mouse_move(segment.center(), None, Modifiers::default());
            settle(cx);
            appearance::assert_selected_button(
                cx,
                &name,
                segment,
                appearance::SelectedState::Hovered,
            );
            cx.update(|window, cx| window.focus(&compact, cx));
            settle(cx);
            assert!(
                cx.update(|window, _| compact.is_focused(window)),
                "{name} takes focus back"
            );
            appearance::assert_selected_button(
                cx,
                &name,
                segment,
                appearance::SelectedState::FocusedAndHovered,
            );
        }
    }
}

/// Linux: the Omarchy theme followed while More contrast is on.
#[cfg(all(test, target_os = "linux"))]
mod omarchy_contrast_tests {
    use super::picker_tests::{open_app, settle};
    use super::*;
    use core::prelude::v1::test;

    /// With More contrast on and the Omarchy theme selected, a reread that
    /// changes what the card shows but not its palette (`colors.toml` gone,
    /// so the card keeps Tokyo Night) applies nothing and the applied palette
    /// stays the adjusted mapping; a reread that changes the palette applies
    /// it once, adjusted. Compared raw with the adjusted global, every
    /// revision applied the theme again.
    #[gpui::test]
    fn an_omarchy_revision_under_more_contrast_applies_only_a_new_palette(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("current");
        std::fs::create_dir_all(root.join("theme")).unwrap();
        let colors = include_bytes!("../tests/fixtures/omarchy/tokyo-night.toml");
        std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
        std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
        cx.update(|cx| drop(appearance::omarchy::install(Some(root.clone()), cx)));
        let (app, cx) = open_app(cx);
        settle(cx);
        let card = shown(cx, "settings-theme-omarchy").expect("the Omarchy card");
        cx.simulate_click(card.center(), Modifiers::default());
        settle(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.increase_contrast = crate::preferences::DisplayChoice::On;
                native_accessibility::set_display_choices(&app.settings, cx);
                native_accessibility::sync_preferences(cx);
                app.show_display_preferences(None, window, cx);
            })
        });
        settle(cx);
        let shown_palette = |cx: &mut VisualTestContext| {
            cx.read(|cx| appearance::omarchy::card(cx).unwrap().palette)
        };
        let mapped = shown_palette(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.theme),
            ThemeSelection::Omarchy
        );
        assert_eq!(cx.read(palette), mapped.with_more_contrast());
        let revision = |cx: &mut VisualTestContext| {
            cx.read(|cx| appearance::omarchy::card(cx).unwrap().revision)
        };
        let (before, applied) = (revision(cx), platform_polish::appearance_applications());

        std::fs::remove_file(root.join("theme/colors.toml")).unwrap();
        cx.update(|_, cx| appearance::omarchy::refresh(cx));
        settle(cx);
        assert!(revision(cx) > before, "the card takes a new revision");
        assert_eq!(shown_palette(cx), mapped, "the card keeps its palette");
        assert_eq!(
            platform_polish::appearance_applications(),
            applied,
            "an unchanged palette applies nothing"
        );
        assert_eq!(cx.read(palette), mapped.with_more_contrast());

        let latte = include_bytes!("../tests/fixtures/omarchy/catppuccin-latte.toml");
        std::fs::write(root.join("theme/colors.toml"), latte).unwrap();
        cx.update(|_, cx| appearance::omarchy::refresh(cx));
        settle(cx);
        let changed = shown_palette(cx);
        assert_ne!(changed, mapped, "the card shows the new palette");
        assert_eq!(
            platform_polish::appearance_applications() - applied,
            1,
            "a new palette applies once"
        );
        assert_eq!(cx.read(palette), changed.with_more_contrast());
    }
}

/// Settings' Reduce motion and Increase contrast rows as a real window
/// draws them.
#[cfg(test)]
mod display_row_tests {
    use super::picker_tests::{open_app, settle};
    use super::*;
    use core::prelude::v1::test;

    /// The focus handle of `setting`'s segment `index`, as the page tracks it.
    fn segment_focus(
        app: &Entity<GitTurtle>,
        setting: DisplaySetting,
        index: usize,
        cx: &mut VisualTestContext,
    ) -> FocusHandle {
        let page = cx.read(|cx| app.read(cx).theme_editor.page().clone());
        cx.update(|_, cx| page.focus((setting.key(), index), cx))
    }

    /// Where `selector` is drawn, for a test that expects it.
    fn drawn(cx: &mut VisualTestContext, selector: String) -> Bounds<Pixels> {
        let leaked: &'static str = selector.clone().leak();
        shown(cx, leaked).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    /// Presses a key no binding takes, so that the window takes keyboard
    /// input before a walk's first key whose frames are checked. The first
    /// key after pointer input, or after none, switches the window to
    /// keyboard input, which leaves it to be drawn again, and GPUI draws a
    /// window left so inside the key's dispatch, before acting on the key
    /// (`Window::dispatch_event`, then `dispatch_key_event`): a frame that
    /// still holds focus on the previous control, now with its keyboard
    /// ring, at the page's offset before the key, which the page's record
    /// of painted frames counts. No platform presents that frame: the frame
    /// request draws again once the key has moved focus and presents only
    /// that frame, so no one sees the ring on the previous control or the
    /// page at its old offset. A probe found that frame drawn inside the
    /// dispatch for such a first key only, again after a pointer move, and
    /// never after a key no binding takes.
    fn keyboard_input(cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("f13");
        settle(cx);
    }

    /// Interface text at `size` points.
    fn text_size(app: &Entity<GitTurtle>, size: u8, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = size;
                let code = app.settings.code_text_size;
                appearance::apply_text_sizes(size, code, window, cx);
                cx.notify();
            })
        });
    }

    /// Both rows draw whole at 11, 13 and 18 pt in 1480 × 973 and
    /// 461 × 490 windows, each scrolled to the top of the page's view: its
    /// three segments share one height (no label wraps), are as wide at
    /// 461 px as at 1480 px (no label is cut to fit) and lie inside their
    /// group, and the group and the title lie inside the row and the row
    /// inside the Appearance card, within the page's view. At 1480 px the
    /// group ends the row beside the title; at 461 px it stacks below it.
    #[gpui::test]
    fn the_display_rows_draw_whole_at_every_size(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let page = cx.read(|cx| app.read(cx).theme_editor.page().clone());
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() + slack >= outer.left()
                && inner.right() <= outer.right() + slack
                && inner.top() + slack >= outer.top()
                && inner.bottom() <= outer.bottom() + slack
        };
        let mut wide_widths = std::collections::HashMap::new();
        let mut failures = Vec::new();
        for (width, height) in [(1480., 973.), (461., 490.)] {
            for text in [11, 13, 18] {
                text_size(&app, text, cx);
                cx.simulate_resize(size(px(width), px(height)));
                settle(cx);
                for setting in DisplaySetting::ALL {
                    let at = format!("{} at {text} pt in {width} × {height}", setting.title());
                    let key = setting.key();
                    // Scrolled so the row starts 8 px below the view's top.
                    let row = drawn(cx, format!("{key}-row"));
                    let view = page.scroll().bounds();
                    let offset = page.scroll().offset();
                    page.scroll().set_offset(point(
                        offset.x,
                        offset.y - (row.top() - view.top() - px(8.)),
                    ));
                    cx.update(|_, cx| app.update(cx, |app, cx| app.notify_settings_page(cx)));
                    settle(cx);
                    let row = drawn(cx, format!("{key}-row"));
                    let group = drawn(cx, format!("{key}-segments"));
                    let title = drawn(cx, format!("setting-title: {}", setting.title()));
                    let card = drawn(cx, "settings-section: Appearance".into());
                    let view = page.scroll().bounds();
                    let segments: Vec<_> = (0..DisplayChoice::ALL.len())
                        .map(|index| drawn(cx, format!("{key}-{index}")))
                        .collect();
                    if !(inside(group, row) && inside(title, row) && inside(row, card)) {
                        failures.push(format!(
                            "{at}: the group {group:?} or title {title:?} leaves the row \
                             {row:?}, or the row the card {card:?}"
                        ));
                    }
                    if !(row.left() + slack >= view.left() && row.right() <= view.right() + slack)
                        || !inside(group, view)
                    {
                        failures.push(format!(
                            "{at}: the row {row:?} or group {group:?} is cut by the view {view:?}"
                        ));
                    }
                    // Stacked, the group starts the row's next line at its
                    // left edge; centred beside a tall description, it could
                    // also lie below the title's line.
                    let stacked = title.bottom() <= group.top() + slack
                        && (group.left() - row.left()).abs() <= slack;
                    let beside = title.right() <= group.left() + slack
                        && (group.right() - row.right()).abs() <= slack;
                    if (width < 1000. && !stacked) || (width > 1000. && !beside) {
                        failures.push(format!(
                            "{at}: the group {group:?} is not {} the title {title:?}",
                            if width < 1000. { "below" } else { "beside" }
                        ));
                    }
                    for (index, segment) in segments.iter().enumerate() {
                        if !inside(*segment, group) {
                            failures.push(format!(
                                "{at}: segment {index} {segment:?} leaves its group {group:?}"
                            ));
                        }
                        if (segment.size.height - segments[1].size.height).abs() > slack {
                            failures.push(format!(
                                "{at}: segment {index} is {:?} tall, \"On\" {:?}: a label wraps",
                                segment.size.height, segments[1].size.height
                            ));
                        }
                        let widths = (key, text, index);
                        if width > 1000. {
                            wide_widths.insert(widths, segment.size.width);
                        } else if let Some(wide) = wide_widths.get(&widths)
                            && (segment.size.width - *wide).abs() > slack
                        {
                            failures.push(format!(
                                "{at}: segment {index} is {:?} wide, {wide:?} in the wide \
                                 window: its label is cut",
                                segment.size.width
                            ));
                        }
                    }
                }
            }
        }
        assert_eq!(wide_widths.len(), 2 * 3 * 3, "every wide segment measured");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Each choice, and following a desktop that asks and one that does
    /// not, keeps each row's height and leaves the rows below where they
    /// were, at 13 and 18 pt in a 461 px and a 1480 px window: every row
    /// reserves its tallest description.
    #[gpui::test]
    fn each_choice_keeps_the_rows_height(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let mut failures = Vec::new();
        let mut descriptions = std::collections::BTreeSet::new();
        for width in [461., 1480.] {
            for text in [13, 18] {
                text_size(&app, text, cx);
                // Tall enough to draw the whole page without scrolling.
                cx.simulate_resize(size(px(width), px(9000.)));
                settle(cx);
                let rows = |cx: &mut VisualTestContext| {
                    DisplaySetting::ALL.map(|setting| drawn(cx, format!("{}-row", setting.key())))
                };
                let at_rest = rows(cx);
                for setting in DisplaySetting::ALL {
                    for (choice, asks) in [
                        (DisplayChoice::On, false),
                        (DisplayChoice::Off, false),
                        (DisplayChoice::Follow, true),
                        (DisplayChoice::Follow, false),
                    ] {
                        // On macOS, following shows the Mac's own answer, which a test cannot set.
                        if cfg!(target_os = "macos") && choice == DisplayChoice::Follow {
                            continue;
                        }
                        let at = format!(
                            "{} {choice:?} (asks {asks}) at {text} pt in {width} px",
                            setting.title()
                        );
                        cx.update(|window, cx| {
                            app.update(cx, |app, cx| {
                                match setting {
                                    DisplaySetting::ReduceMotion => {
                                        app.settings.reduce_motion = choice
                                    }
                                    DisplaySetting::IncreaseContrast => {
                                        app.settings.increase_contrast = choice
                                    }
                                }
                                #[cfg(target_os = "linux")]
                                native_accessibility::store_desktop_preferences(
                                    native_accessibility::DisplayPreferences {
                                        reduce_motion: asks,
                                        increase_contrast: asks,
                                        reduce_transparency: false,
                                    },
                                    cx,
                                );
                                app.notify_settings_page(cx);
                                window.refresh();
                            })
                        });
                        settle(cx);
                        let drawn_rows = rows(cx);
                        if drawn_rows != at_rest {
                            failures.push(format!("{at}: {at_rest:?} -> {drawn_rows:?}"));
                        }
                        let shown = setting.description(choice, asks);
                        let read = format!("setting-description: {}. {shown}", setting.title());
                        if !page_shows(cx, read.clone().leak()) {
                            failures.push(format!("{at}: does not read {read:?}"));
                        }
                        descriptions.insert(shown);
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        #[cfg(target_os = "linux")]
        assert_eq!(
            descriptions.len(),
            8,
            "every description drawn: {descriptions:?}"
        );
    }

    /// From List density's first segment, focused in code below the page's
    /// view, Tab reaches its last and then Reduce motion's three segments
    /// and Increase contrast's, in order, in a 461 × 490 window at 18 pt,
    /// and every frame painted after each key, the first key's included,
    /// shows the focused segment's row, which fits the page's view, with the
    /// focus ring's room inside that view.
    #[gpui::test]
    fn tab_reaches_the_six_segments_in_order_and_keeps_each_in_view(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (app, cx) = open_app(cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        text_size(&app, 18, cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        settle(cx);
        let page = cx.read(|cx| app.read(cx).theme_editor.page().clone());
        let room = cx.read(appearance::button_ring_room);
        // List density's first segment, focused in code before any key, so
        // the page leaves it below its view; then a key no binding takes,
        // which moves no focus and so reveals nothing, and Tab to the
        // segment's neighbour, which the keyboard's focus scrolls into view
        // in the first frame after that Tab. Focused in code after a key,
        // the segment was revealed at once, and the walk's first Tab began
        // inside the view.
        let density = |index: usize, cx: &mut VisualTestContext| {
            cx.update(|_, cx| page.focus(("settings-density", index), cx))
        };
        let first = density(0, cx);
        cx.update(|window, cx| window.focus(&first, cx));
        settle(cx);
        keyboard_input(cx);
        let view = page.scroll().bounds();
        let below = drawn(cx, "settings-density-0".into());
        assert!(
            cx.update(|window, _| first.is_focused(window)) && below.top() > view.bottom(),
            "List density's first segment, {below:?}, holds focus below the page's view, \
             {view:?}, before the first Tab"
        );
        let slack = px(0.01);
        let mut steps = vec![("List density's segment 1".to_owned(), density(1, cx))];
        for setting in DisplaySetting::ALL {
            for index in 0..DisplayChoice::ALL.len() {
                let at = format!("{}'s segment {index}", setting.title());
                steps.push((at, segment_focus(&app, setting, index, cx)));
            }
        }
        for (at, expected) in steps {
            page.take_painted();
            cx.simulate_keystrokes("tab");
            draw(cx);
            assert!(
                cx.update(|window, _| expected.is_focused(window)),
                "Tab reaches {at}"
            );
            let frames = page.take_painted();
            assert!(!frames.is_empty(), "{at}: Tab paints the page");
            for (frame, painted) in frames.iter().enumerate() {
                let ring = painted
                    .control
                    .unwrap_or_else(|| panic!("{at}: frame {frame} draws the focused row"))
                    .dilate(room);
                assert!(
                    ring.top() + slack >= painted.viewport.top()
                        && ring.bottom() <= painted.viewport.bottom() + slack,
                    "{at}: frame {frame} paints the segment's row with its ring's room, \
                         {ring:?}, outside the page, {:?}",
                    painted.viewport
                );
            }
        }
    }

    /// The first key after pointer input. A click on an empty spot of the
    /// page focuses the window's root, and the key after it, Tab or
    /// Shift+Tab, paints one stale frame first, drawn inside its dispatch
    /// ([`keyboard_input`]), with the root still focused and the page at its
    /// offset before the key; every frame after it holds the focus the key
    /// moved to and, where that is a control of the page, shows it with its
    /// ring's room inside the page's view, and otherwise leaves the page
    /// where it was. From the root Tab reaches the window's first control,
    /// in the header above the page, and Shift+Tab its last, a control of
    /// the page above the view of a page scrolled to its foot, which the
    /// page scrolls to.
    #[gpui::test]
    fn the_first_key_after_a_click_paints_one_stale_frame_then_its_focus(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (app, cx) = open_app(cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.simulate_resize(size(px(1480.), px(973.)));
        settle(cx);
        let page = cx.read(|cx| app.read(cx).theme_editor.page().clone());
        let room = cx.read(appearance::button_ring_room);
        let root = cx.read(|cx| app.read(cx).app_focus.clone());
        let mut revealed = false;
        for key in ["tab", "shift-tab"] {
            let foot = page.scroll().max_offset().y;
            page.scroll().set_offset(point(px(0.), -foot));
            cx.update(|_, cx| app.update(cx, |app, cx| app.notify_settings_page(cx)));
            settle(cx);
            // Beside the page's columns, where no control lies.
            let view = page.scroll().bounds();
            cx.simulate_click(
                point(view.right() - px(24.), view.bottom() - px(24.)),
                Modifiers::default(),
            );
            settle(cx);
            assert!(
                cx.update(|window, _| root.is_focused(window)),
                "the click focuses the window's root"
            );
            let before = page.scroll().offset().y;
            page.take_painted();
            cx.simulate_keystrokes(key);
            draw(cx);
            let focus = cx.update(|window, cx| window.focused(cx));
            assert!(
                focus.as_ref().is_some_and(|focus| *focus != root),
                "{key} moves focus from the root"
            );
            let frames = page.take_painted();
            let Some((stale, after)) = frames.split_first().filter(|(_, after)| !after.is_empty())
            else {
                panic!("{key} paints a stale frame and then more: {frames:?}");
            };
            assert!(
                stale.focus.as_ref().is_some_and(|focus| *focus == root) && stale.offset == before,
                "{key}'s first frame is drawn before focus moves, at the page's offset \
                 {before:?}: {stale:?}"
            );
            let in_page = cx.update(|window, _| page.focused(window)).is_some();
            for (index, painted) in after.iter().enumerate() {
                let frame = index + 1;
                assert!(
                    painted.focus == focus.as_ref().map(FocusHandle::downgrade),
                    "{key}: frame {frame} holds the focus the key moved to: {painted:?}"
                );
                if in_page {
                    let control = painted.control.unwrap_or_else(|| {
                        panic!("{key}: frame {frame} draws the focused control")
                    });
                    assert!(
                        within(control.dilate(room), painted.viewport),
                        "{key}: frame {frame} paints the focused control, {control:?}, with its \
                         ring's room outside the page's view, {:?}",
                        painted.viewport
                    );
                } else {
                    assert_eq!(
                        painted.offset, before,
                        "{key}: frame {frame} leaves the page"
                    );
                }
            }
            revealed |= in_page && page.scroll().offset().y > before;
        }
        assert!(
            revealed,
            "Shift+Tab from the root scrolls the page to its last control"
        );
    }

    /// List density's, Reduce motion's and Increase contrast's rows: each
    /// row's drawn name with its segments' focus handles and drawn names.
    fn segment_rows(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) -> Vec<(String, Vec<(FocusHandle, String)>)> {
        let page = cx.read(|cx| app.read(cx).theme_editor.page().clone());
        let density = (0..Density::ALL.len())
            .map(|index| {
                let focus = cx.update(|_, cx| page.focus(("settings-density", index), cx));
                (focus, format!("settings-density-{index}"))
            })
            .collect();
        let mut rows = vec![("settings-density-row".to_owned(), density)];
        for setting in DisplaySetting::ALL {
            let key = setting.key();
            let segments = (0..DisplayChoice::ALL.len())
                .map(|index| {
                    let focus = segment_focus(app, setting, index, cx);
                    (focus, format!("{key}-{index}"))
                })
                .collect();
            rows.push((format!("{key}-row"), segments));
        }
        rows
    }

    /// Focuses the control before `segment` in the Tab order, then scrolls
    /// the page so that `row` starts 8 px below its view (`below`) or ends
    /// 8 px above it, for a Tab onto the segment.
    fn ready_tab_onto(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        page: &ScrollHandle,
        row: &str,
        segment: &FocusHandle,
        below: bool,
    ) {
        cx.update(|window, cx| {
            window.focus(segment, cx);
            window.focus_prev(cx);
        });
        settle(cx);
        let at = drawn(cx, row.to_owned());
        let view = page.bounds();
        let shift = if below {
            view.bottom() + px(8.) - at.top()
        } else {
            view.top() - px(8.) - at.bottom()
        };
        let offset = page.offset();
        page.set_offset(point(offset.x, offset.y + shift));
        cx.update(|_, cx| app.update(cx, |app, cx| app.notify_settings_page(cx)));
        settle(cx);
        let at = drawn(cx, row.to_owned());
        assert!(
            if below {
                at.top() > view.bottom()
            } else {
                at.bottom() < view.top()
            },
            "{row} at {at:?} lies {} the page's view {view:?}",
            if below { "below" } else { "above" }
        );
    }

    /// Whether `bounds` lies inside `view`'s height.
    fn within(bounds: Bounds<Pixels>, view: Bounds<Pixels>) -> bool {
        let slack = px(0.01);
        bounds.top() + slack >= view.top() && bounds.bottom() <= view.bottom() + slack
    }

    /// Tab onto each segment of List density, Reduce motion and Increase
    /// contrast, from a page scrolled so that the segment's row starts below
    /// its view, at 13 and 18 pt in 1480 × 973 and 461 × 490 windows: every
    /// frame painted after the key, and the page once settled, show the whole
    /// row, its title, its description at its reserved height and its
    /// segments, with the focus ring's room, and focus stays on the segment
    /// with its ring's room in view. Revealed alone, the segments left
    /// Increase contrast's description below the fold of the wide window.
    #[gpui::test]
    fn tab_onto_a_segment_shows_its_whole_row(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (app, cx) = open_app(cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let page = cx.read(|cx| app.read(cx).theme_editor.page().clone());
        let room = cx.read(appearance::button_ring_room);
        let rows = segment_rows(&app, cx);
        // Without it the first step's frames began with one holding focus on
        // the control before List density's first segment, passed only
        // because the page showed it just above that row.
        keyboard_input(cx);
        let mut failures = Vec::new();
        let mut steps = 0;
        for (width, height) in [(1480., 973.), (461., 490.)] {
            for text in [13, 18] {
                text_size(&app, text, cx);
                cx.simulate_resize(size(px(width), px(height)));
                settle(cx);
                for (row, segments) in &rows {
                    for (focus, segment) in segments {
                        let at = format!("{segment} at {text} pt in {width} × {height}");
                        ready_tab_onto(&app, cx, page.scroll(), row, focus, true);
                        page.take_painted();
                        cx.simulate_keystrokes("tab");
                        draw(cx);
                        steps += 1;
                        if !cx.update(|window, _| focus.is_focused(window)) {
                            failures.push(format!("{at}: Tab does not focus the segment"));
                            continue;
                        }
                        let frames = page.take_painted();
                        if frames.is_empty() {
                            failures.push(format!("{at}: Tab paints nothing"));
                        }
                        for (frame, painted) in frames.iter().enumerate() {
                            match painted.control {
                                Some(shown) if within(shown.dilate(room), painted.viewport) => {}
                                shown => failures.push(format!(
                                    "{at}: frame {frame} paints the focused row {shown:?} \
                                     outside the page {:?}",
                                    painted.viewport
                                )),
                            }
                        }
                        let view = page.scroll().bounds();
                        let shown_row = drawn(cx, row.clone());
                        if !within(shown_row.dilate(room), view) {
                            failures.push(format!(
                                "{at}: the row {shown_row:?}, with the ring's room, \
                                 leaves the page {view:?}"
                            ));
                        }
                        let shown_segment = drawn(cx, segment.clone());
                        if !within(shown_segment.dilate(room), view) {
                            failures.push(format!(
                                "{at}: the segment {shown_segment:?} with its ring \
                                 leaves the page {view:?}"
                            ));
                        }
                        if !cx.update(|window, _| focus.is_focused(window)) {
                            failures.push(format!("{at}: the reveal moves focus"));
                        }
                    }
                }
            }
        }
        assert_eq!(steps, 4 * 8, "every segment reached at every size");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// In a window cut so that the page's view is shorter than each row but
    /// holds a segment with its focus ring, at 18 pt and 461 and 1480 px
    /// wide, Tab onto each segment from a page scrolled so that its row lies
    /// below the view and so that it lies above: from the frame that first
    /// draws the focus, the page shows the focused segment with its ring's
    /// room rather than the row's top or foot.
    #[gpui::test]
    fn a_row_taller_than_the_page_shows_its_focused_segment(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (app, cx) = open_app(cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let page = cx.read(|cx| app.read(cx).theme_editor.page().clone());
        let room = cx.read(appearance::button_ring_room);
        let rows = segment_rows(&app, cx);
        text_size(&app, 18, cx);
        // As the test's first key, the first step's Tab painted one frame
        // that still held focus on the control before the segment, at the
        // page's offset before the key, a frame no platform presents.
        keyboard_input(cx);
        let mut failures = Vec::new();
        let mut steps = 0;
        for width in [461., 1480.] {
            let height = px(490.);
            cx.simulate_resize(size(px(width), height));
            settle(cx);
            let one = drawn(cx, rows[0].1[0].1.clone()).size.height;
            let chrome = height - page.scroll().bounds().size.height;
            cx.simulate_resize(size(px(width), chrome + one + room * 2. + px(12.)));
            settle(cx);
            let view = page.scroll().bounds().size.height;
            for (row, _) in &rows {
                let tall = drawn(cx, row.clone()).size.height;
                assert!(
                    tall + room * 2. > view && one + room * 2. <= view,
                    "{row}, {tall:?} tall, does not fit the page's view, {view:?}, \
                     which holds a segment, {one:?}"
                );
            }
            for (row, segments) in &rows {
                for (focus, segment) in segments {
                    for below in [true, false] {
                        let at = format!(
                            "{segment} at {width} px from {}",
                            if below { "below" } else { "above" }
                        );
                        ready_tab_onto(&app, cx, page.scroll(), row, focus, below);
                        page.take_painted();
                        cx.simulate_keystrokes("tab");
                        draw(cx);
                        steps += 1;
                        if !cx.update(|window, _| focus.is_focused(window)) {
                            failures.push(format!("{at}: Tab does not focus the segment"));
                            continue;
                        }
                        // From the frame that first draws the focus.
                        let frames = page.take_painted();
                        if frames.is_empty() {
                            failures.push(format!("{at}: Tab paints nothing"));
                        }
                        for (frame, painted) in frames.iter().enumerate() {
                            match painted.control {
                                Some(shown) if within(shown.dilate(room), painted.viewport) => {}
                                shown => failures.push(format!(
                                    "{at}: frame {frame} paints the focused segment {shown:?} \
                                     with its ring outside the page {:?}",
                                    painted.viewport
                                )),
                            }
                        }
                        let view = page.scroll().bounds();
                        let shown = drawn(cx, segment.clone());
                        if !within(shown.dilate(room), view) {
                            failures.push(format!(
                                "{at}: the segment {shown:?} with its ring leaves the \
                                 page {view:?}"
                            ));
                        }
                    }
                }
            }
        }
        assert_eq!(steps, 2 * 8 * 2, "every segment reached from both sides");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// What the settings file on disk holds for `setting` once every save
    /// asked for so far has landed. A save is the preference writer's job on
    /// its own thread, which settling the window does not wait for: read at
    /// once, the file held the save before (hosted run 42696574651) or none.
    fn stored(
        app: &Entity<GitTurtle>,
        setting: DisplaySetting,
        cx: &mut VisualTestContext,
    ) -> serde_json::Value {
        crate::theme_editor::tests::drain_preference_writer(cx, app);
        let path = preferences::settings_path().expect("the test settings path");
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("a saved store")).expect("a store");
        let field = match setting {
            DisplaySetting::ReduceMotion => "reduce_motion",
            DisplaySetting::IncreaseContrast => "increase_contrast",
        };
        stored["settings"][field].clone()
    }

    /// A click on a segment, or Space on the focused one, selects its
    /// choice, saves it and applies it at once: Reduce motion On reduces
    /// the toolkit's motion, Off plays it; Increase contrast On applies the
    /// adjusted palette, once, and Off the theme's own, once. A press on the
    /// selected segment does nothing.
    #[gpui::test]
    fn a_click_or_space_selects_saves_and_applies(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.simulate_resize(size(px(1480.), px(9000.)));
        settle(cx);
        let own = cx.read(|cx| app.read(cx).effective_theme(cx).palette);
        let click = |cx: &mut VisualTestContext, setting: DisplaySetting, index: usize| {
            let segment = drawn(cx, format!("{}-{index}", setting.key()));
            cx.simulate_click(segment.center(), Modifiers::default());
            settle(cx);
        };
        let space = |cx: &mut VisualTestContext, setting: DisplaySetting, index: usize| {
            let focus = segment_focus(&app, setting, index, cx);
            cx.update(|window, cx| window.focus(&focus, cx));
            settle(cx);
            let space = Keystroke::parse("space").expect("a keystroke");
            cx.simulate_event(KeyDownEvent {
                keystroke: space.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            cx.simulate_event(KeyUpEvent { keystroke: space });
            settle(cx);
        };
        let state = |cx: &mut VisualTestContext| {
            cx.read(|cx| {
                let app = app.read(cx);
                (
                    app.settings.reduce_motion,
                    app.settings.increase_contrast,
                    cx.reduce_motion(),
                    appearance::more_contrast(cx),
                    palette(cx),
                )
            })
        };
        let motion = DisplaySetting::ReduceMotion;
        let contrast = DisplaySetting::IncreaseContrast;
        let index = |choice: DisplayChoice| {
            DisplayChoice::ALL
                .iter()
                .position(|option| *option == choice)
                .unwrap()
        };
        // On macOS, following takes the Mac's own Increase Contrast: Off
        // there keeps the test independent of it.
        let unchosen = if cfg!(target_os = "macos") {
            DisplayChoice::Off
        } else {
            DisplayChoice::Follow
        };
        #[cfg(target_os = "macos")]
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.settings.increase_contrast = unchosen;
                native_accessibility::set_display_choices(&app.settings, cx);
            })
        });

        click(cx, motion, index(DisplayChoice::On));
        assert_eq!(
            state(cx),
            (DisplayChoice::On, unchosen, true, false, own),
            "a click on Reduce motion: On"
        );
        assert_eq!(stored(&app, motion, cx), "on");
        space(cx, motion, index(DisplayChoice::Off));
        assert_eq!(
            state(cx),
            (DisplayChoice::Off, unchosen, false, false, own),
            "Space on Reduce motion: Off"
        );
        assert_eq!(stored(&app, motion, cx), "off");

        let applied = platform_polish::appearance_applications();
        space(cx, contrast, index(DisplayChoice::On));
        assert_eq!(
            state(cx),
            (
                DisplayChoice::Off,
                DisplayChoice::On,
                false,
                true,
                own.with_more_contrast()
            ),
            "Space on Increase contrast: On"
        );
        assert_eq!(stored(&app, contrast, cx), "on");
        assert_eq!(platform_polish::appearance_applications() - applied, 1);
        click(cx, contrast, index(DisplayChoice::On));
        assert_eq!(
            platform_polish::appearance_applications() - applied,
            1,
            "the selected segment again applies nothing"
        );
        click(cx, contrast, index(DisplayChoice::Off));
        assert_eq!(
            state(cx),
            (DisplayChoice::Off, DisplayChoice::Off, false, false, own),
            "a click on Increase contrast: Off"
        );
        assert_eq!(stored(&app, contrast, cx), "off");
        assert_eq!(platform_polish::appearance_applications() - applied, 2);
        click(cx, contrast, index(DisplayChoice::Follow));
        assert_eq!(stored(&app, contrast, cx), "follow");
    }

    /// The accessibility node `segment` writes as it is drawn: its base
    /// Button, laid out inside the kit's, writes it, as `reflog`'s entry
    /// nodes are read.
    fn segment_node(segment: Button, window: &mut Window, cx: &mut App) -> gpui::accesskit::Node {
        use gpui::Element as _;
        fn of_type<'a, E: gpui::Element>(
            _witness: &E,
            element: &'a mut AnyElement,
        ) -> Option<&'a mut E> {
            element.downcast_mut()
        }
        let mut outer = RenderOnce::render(segment, window, cx).into_element();
        let (_, mut layout) = outer.request_layout(None, None, window, cx);
        let mut base = (&mut layout as &mut dyn std::any::Any)
            .downcast_mut::<Option<AnyElement>>()
            .and_then(Option::take)
            .expect("the kit's Button renders a base Button");
        let witness = RenderOnce::render(
            gpui_kit::base::Button::new("segment-node-witness"),
            window,
            cx,
        )
        .into_element();
        let element = of_type(&witness, &mut base).expect("the base Button writes the node");
        let mut node = gpui::accesskit::Node::new(element.a11y_role().expect("a role"));
        element.write_a11y_info(&mut node);
        node
    }

    /// Each segment is named by its setting and its choice, and only the
    /// selected one is toggled: as the page builds them for each choice,
    /// and in the node such a segment writes.
    #[gpui::test]
    fn each_segment_names_its_setting_and_choice_and_toggles_the_selected(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.simulate_resize(size(px(1480.), px(9000.)));
        settle(cx);
        let follow = if cfg!(target_os = "macos") {
            "Follow the system"
        } else {
            "Follow the desktop"
        };
        for setting in DisplaySetting::ALL {
            let names = [follow, "On", "Off"].map(|label| format!("{}: {label}", setting.title()));
            for (selected, choice) in DisplayChoice::ALL.into_iter().enumerate() {
                let segment = drawn(cx, format!("{}-{selected}", setting.key()));
                cx.simulate_click(segment.center(), Modifiers::default());
                settle(cx);
                assert_eq!(cx.read(|cx| setting.choice(&app.read(cx).settings)), choice);
                page_shows(cx, "settings-section: Appearance");
                for (index, name) in names.iter().enumerate() {
                    let selector = format!("{}-{index}", setting.key());
                    assert_eq!(
                        segment_built(&selector),
                        Some((name.clone(), index == selected)),
                        "{selector} with {choice:?} chosen"
                    );
                    let focus = segment_focus(&app, setting, index, cx);
                    let node = crate::views::tests::view_node(cx, move |window, cx| {
                        segment_node(
                            display_segment(setting, index, index == selected, &focus),
                            window,
                            cx,
                        )
                    });
                    assert_eq!(
                        (node.role(), node.label(), node.toggled()),
                        (
                            gpui::accesskit::Role::Button,
                            Some(name.as_str()),
                            Some(if index == selected {
                                gpui::accesskit::Toggled::True
                            } else {
                                gpui::accesskit::Toggled::False
                            })
                        ),
                        "{selector}'s node with {choice:?} chosen"
                    );
                }
            }
        }
    }
}
