//! Custom theme editor: the form behind Settings → Your themes.
//!
//! `State` belongs to the window's `GitTurtle`, like `profiles::State`. While the
//! editor is open, `State::preview` holds the draft palette and
//! `GitTurtle::apply_appearance` applies it instead of the saved selection, so the
//! draft uses the one application path (`Palette::apply`) and every later
//! re-application (system appearance, text size) keeps showing it. The first
//! edit of a frame applies in its handler, so the keystroke's own render already
//! shows the draft; later edits in the same frame coalesce to one application at
//! the next frame callback. Return is routed by focus: Name submits, a hex field
//! commits its value and every other control activates itself, so the dialog's
//! own Confirm never saves from Cancel. Cancel, Escape and a closed dialog drop
//! the draft and re-apply the saved selection. Nothing reaches the store until
//! Save, which goes through `Preferences::save_custom_themes` on the serialized
//! preference executor. The contract is `docs/development/themes/spec.md`
//! §Editor and §Failure and recovery, and `DESIGN.md` §Custom theme editor.

use crate::*;
use appearance::custom::{
    self, CustomTheme, ReadabilityBackground, ReadabilityForeground, ReadabilityIssue,
    ThemeSelection, TokenGroup, TokenKind,
};
use appearance::{Palette, ThemeChoice};
use gpui_kit::base::{FocusableExt, Scrollbar, ScrollbarMode};
use gpui_kit::component::{
    WindowExt,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    dialog::Confirm,
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use preferences::MAX_CUSTOM_THEMES;

#[derive(Default)]
pub(super) struct State {
    form: Option<Entity<ThemeForm>>,
    /// The draft the open editor shows; `apply_appearance` applies it in place
    /// of the saved selection.
    preview: Option<Palette>,
    /// The next frame callback is registered; drafts until then coalesce.
    preview_scheduled: bool,
    /// A later draft of the same frame is waiting for that callback.
    preview_dirty: bool,
    /// The Your themes card tracks this handle (not a tab stop), so focus can
    /// return to New theme… after a delete removes the focused row.
    card_focus: std::cell::OnceCell<FocusHandle>,
    /// A delete finished: New theme… takes focus once a frame has drawn the
    /// card without the row (`return_focus_after_delete`).
    focus_after_delete: Option<DeleteFocus>,
    /// Custom-theme saves submitted from this window and not yet answered.
    /// Only their replies replace `GitTurtle::custom_themes`.
    pending_saves: usize,
    /// A failed delete, shown in the Your themes card.
    error: Option<String>,
    /// The Your themes card's warning counts, by theme id and palette, so a
    /// Settings frame does not rerun the readability rules for every theme.
    warning_counts: std::cell::RefCell<Vec<(u32, Palette, usize)>>,
    #[cfg(test)]
    preview_applications: usize,
}

impl State {
    pub(super) fn preview(&self) -> Option<Palette> {
        self.preview
    }

    pub(super) fn save_pending(&self) -> bool {
        self.pending_saves > 0
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// `readability_issues().len()` for each theme, in order; a theme is
    /// recomputed only when its palette changes.
    pub(super) fn warning_counts(&self, themes: &[CustomTheme]) -> Vec<usize> {
        let mut cache = self.warning_counts.borrow_mut();
        let current = themes
            .iter()
            .zip(cache.iter())
            .take_while(|(theme, (id, palette, _))| theme.id == *id && theme.palette == *palette)
            .count();
        if current != themes.len() || current != cache.len() {
            let previous = std::mem::take(&mut *cache);
            *cache = themes
                .iter()
                .map(|theme| {
                    let count = previous
                        .iter()
                        .find(|(id, palette, _)| *id == theme.id && *palette == theme.palette)
                        .map_or_else(|| theme.palette.readability_issues().len(), |c| c.2);
                    (theme.id, theme.palette, count)
                })
                .collect();
        }
        cache.iter().map(|(_, _, count)| *count).collect()
    }

    /// The Your themes card's handle; `focus_next` from it reaches New theme….
    pub(super) fn card_focus(&self, cx: &App) -> FocusHandle {
        self.card_focus.get_or_init(|| cx.focus_handle()).clone()
    }
}

/// What a finished custom-theme save means for the window.
enum SaveOutcome {
    Edited {
        id: u32,
        form: WeakEntity<ThemeForm>,
    },
    Deleted {
        id: u32,
        base: ThemeChoice,
        /// What had focus when Delete… was activated: the row's own action in
        /// the keyboard flow. The alert restores it on close, and it goes with
        /// the row.
        previous: Option<FocusHandle>,
    },
}

/// Where a finished delete left keyboard focus.
struct DeleteFocus {
    /// What had focus when Delete… was activated, which the alert restored:
    /// the deleted row's own action in the keyboard flow.
    restored: Option<FocusHandle>,
}

/// The tokens a readability issue names, for the per-row warning glyphs.
fn issue_tokens(issue: &ReadabilityIssue) -> impl Iterator<Item = TokenKind> {
    let foreground = match issue.foreground {
        ReadabilityForeground::Token(kind) => Some(kind),
        ReadabilityForeground::Lane(_) => None,
    };
    let background = match issue.background {
        ReadabilityBackground::Token(kind) => Some(kind),
        ReadabilityBackground::SelectedRowHover => Some(TokenKind::Selected),
    };
    foreground.into_iter().chain(background)
}

fn hsla_rgb(color: Hsla) -> u32 {
    let color = color.to_rgb();
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u32;
    (channel(color.r) << 16) | (channel(color.g) << 8) | channel(color.b)
}

fn rgb_hsla(color: u32) -> Hsla {
    rgb(color).into()
}

/// A small warning mark: the palette's warning fill with a canvas "!".
pub(super) fn warning_glyph(p: Palette) -> Div {
    div()
        .size(appearance::ui_size(14.))
        .flex_shrink_0()
        .rounded_full()
        .bg(rgb(p.warning))
        .flex()
        .items_center()
        .justify_center()
        .text_size(appearance::ui_text(10.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(p.canvas))
        .child("!")
}

/// A row whose field does not hold `#rrggbb`: a square with a cross, so the
/// cue differs from the round readability glyph in shape, not only in color.
fn invalid_glyph(p: Palette) -> Div {
    div()
        .size(appearance::ui_size(14.))
        .flex_shrink_0()
        .rounded(px(3.))
        .bg(rgb(p.removed))
        .flex()
        .items_center()
        .justify_center()
        .text_size(appearance::ui_text(11.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(p.canvas))
        .child("×")
}

/// "My Nord", then "My Nord 2", … — the first name the store accepts.
fn default_theme_name(base: ThemeChoice, customs: &[CustomTheme]) -> String {
    let stem = format!("My {}", base.label());
    (1..=MAX_CUSTOM_THEMES + 1)
        .map(|n| {
            if n == 1 {
                stem.clone()
            } else {
                format!("{stem} {n}")
            }
        })
        .find(|name| custom::validate_theme_name(name, customs, None).is_ok())
        .unwrap_or(stem)
}

impl GitTurtle {
    /// The built-in the active theme derives from: itself, or a custom theme's base.
    fn active_base(&self, cx: &App) -> ThemeChoice {
        match self.effective_theme(cx).selection {
            ThemeSelection::BuiltIn(choice) => choice,
            ThemeSelection::Custom(id) => self
                .custom_themes
                .iter()
                .find(|theme| theme.id == id)
                .map_or_else(ThemeChoice::default, |theme| theme.base),
        }
    }

    /// Open the editor for a new theme (`None`) or the saved theme `editing`.
    pub(super) fn open_theme_editor(
        &mut self,
        editing: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_editor.form.is_some() || self.theme_editor.save_pending() {
            return;
        }
        let (name, base, palette) = match editing {
            Some(id) => {
                let Some(theme) = self.custom_themes.iter().find(|theme| theme.id == id) else {
                    return;
                };
                (theme.name.clone(), theme.base, theme.palette)
            }
            None => {
                if self.custom_themes.len() >= MAX_CUSTOM_THEMES {
                    self.theme_editor.error = Some(format!(
                        "Up to {MAX_CUSTOM_THEMES} custom themes can be saved. Delete one before adding another."
                    ));
                    cx.notify();
                    return;
                }
                let base = self.active_base(cx);
                (
                    default_theme_name(base, &self.custom_themes),
                    base,
                    base.palette(),
                )
            }
        };
        let owner = cx.entity().downgrade();
        let customs = self.custom_themes.clone();
        let form =
            cx.new(|cx| ThemeForm::new(owner, editing, name, base, palette, customs, window, cx));
        self.theme_editor.form = Some(form.clone());
        self.theme_editor.error = None;
        self.preview_theme_draft(palette, window, cx);
        let focus = form.read(cx).name.read(cx).focus_handle(cx);
        let focus_form = form.downgrade();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let viewport = window.viewport_size();
            let wide = viewport.width >= appearance::ui_size(1060.);
            let width = if wide { px(1000.) } else { px(640.) }
                .min(viewport.width - px(32.))
                .max(px(320.));
            let submit = form.clone();
            let cancel = form.clone();
            let title = if form.read(cx).editing.is_some() {
                "Edit theme"
            } else {
                "New theme"
            };
            dialog
                .title(title)
                .width(width)
                .child(form.clone())
                .footer(ThemeForm::footer(&form, cx))
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.submit(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |form, cx| form.cancel(window, cx))
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

    /// Show `draft` in the whole window. The first draft of a frame applies
    /// here, as a Settings switch does, so the render the keystroke itself
    /// triggers already shows it instead of the old theme; later drafts in the
    /// same frame (a picker drag, a burst of edits) replace it and apply once
    /// from the next frame callback. No worker job is involved. The trace keeps
    /// attempt 3's boundary: stamped here, printed at the frame callback after
    /// the one that follows the application.
    fn preview_theme_draft(&mut self, draft: Palette, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_editor.form.is_none() {
            return;
        }
        self.theme_editor.preview = Some(draft);
        if self.theme_editor.preview_scheduled {
            self.theme_editor.preview_dirty = true;
            return;
        }
        let started = trace_enabled().then(Instant::now);
        self.apply_appearance(window, cx);
        #[cfg(test)]
        {
            self.theme_editor.preview_applications += 1;
        }
        self.theme_editor.preview_scheduled = true;
        self.theme_editor.preview_dirty = false;
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.theme_editor.preview_scheduled = false;
                let dirty = std::mem::take(&mut this.theme_editor.preview_dirty);
                if dirty && this.theme_editor.preview.is_some() {
                    this.apply_appearance(window, cx);
                    #[cfg(test)]
                    {
                        this.theme_editor.preview_applications += 1;
                    }
                }
                if let Some(start) = started {
                    Self::trace_next_frame("theme_apply_frame_ms", start, window);
                }
            });
        });
    }

    /// Drop the draft and re-apply the saved selection.
    fn close_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_editor.form = None;
        self.theme_editor.preview = None;
        self.apply_appearance(window, cx);
    }

    /// Submit the editor's theme. Errors are returned to the form, which shows
    /// them beside Save.
    fn save_theme_from_editor(
        &mut self,
        theme: CustomTheme,
        editing: Option<u32>,
        form: WeakEntity<ThemeForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.theme_editor.save_pending() {
            return Err("Wait for the current theme save to finish.".into());
        }
        custom::validate_theme_name(&theme.name, &self.custom_themes, editing)?;
        let mut themes = self.custom_themes.clone();
        let id = match editing {
            Some(id) => {
                let Some(slot) = themes.iter_mut().find(|saved| saved.id == id) else {
                    return Err("This theme no longer exists. Cancel to close the editor.".into());
                };
                *slot = CustomTheme { id, ..theme };
                id
            }
            None => {
                if themes.len() >= MAX_CUSTOM_THEMES {
                    return Err(format!(
                        "Up to {MAX_CUSTOM_THEMES} custom themes can be saved. Delete one before adding another."
                    ));
                }
                let Some(id) = custom::next_custom_theme_id(&themes) else {
                    return Err("No theme id is available. Delete a theme and try again.".into());
                };
                themes.push(CustomTheme { id, ..theme });
                id
            }
        };
        self.submit_custom_themes(themes, SaveOutcome::Edited { id, form }, window, cx);
        Ok(())
    }

    /// Ask before deleting a saved theme, naming it and what happens when it is
    /// active. The alert opens with Cancel focused and routes Return by focus,
    /// so Return never deletes from Cancel or from the alert itself.
    ///
    /// The toolkit focuses the alert's host when it opens, which keeps Tab and
    /// Escape inside the alert. Cancel is the host's first tab stop, but
    /// `focus_next` walks the last drawn frame, and GPUI runs frame callbacks
    /// before that frame's draw: a callback registered here would run before
    /// the alert was ever drawn and send focus to the page behind it. The
    /// builder runs while the alert is drawn, so the move to Cancel is
    /// registered there, once, and runs after the first frame that contains it.
    pub(super) fn confirm_delete_theme(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_editor.save_pending() {
            return;
        }
        let Some(theme) = self.custom_themes.iter().find(|theme| theme.id == id) else {
            return;
        };
        let title = format!("Delete “{}”?", theme.name);
        let explanation = if self.settings.theme == ThemeSelection::Custom(id) {
            format!(
                "“{}” is your current theme. Deleting it switches to its base, {}. Other themes and settings are kept.",
                theme.name,
                theme.base.label()
            )
        } else {
            format!(
                "“{}” is removed from your themes. Other themes and settings are kept.",
                theme.name
            )
        };
        let owner = cx.entity().downgrade();
        let previous = window.focused(cx);
        let cancel_focus = cx.focus_handle();
        let delete_focus = cx.focus_handle();
        let focus_cancel = std::rc::Rc::new(std::cell::Cell::new(true));
        window.open_alert_dialog(cx, move |dialog, window, _| {
            if focus_cancel.replace(false) {
                let (cancel, delete) = (cancel_focus.clone(), delete_focus.clone());
                window.on_next_frame(move |window, cx| {
                    if window.has_active_dialog(cx)
                        && !cancel.contains_focused(window, cx)
                        && !delete.contains_focused(window, cx)
                    {
                        // From the wrapper, the next tab stop is its button.
                        cancel.focus(window, cx);
                        window.focus_next(cx);
                    }
                });
            }
            let owner = owner.clone();
            let delete = owner.clone();
            let (route_previous, click_previous) = (previous.clone(), previous.clone());
            let (cancel_focus, delete_focus) = (cancel_focus.clone(), delete_focus.clone());
            let (route_cancel, route_delete) = (cancel_focus.clone(), delete_focus.clone());
            dialog
                .title(title.clone())
                .child(
                    div()
                        .id("delete-theme-consequences")
                        .role(Role::Label)
                        .aria_label(explanation.clone())
                        .child(explanation.clone()),
                )
                .footer(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        // Return is the dialog's Confirm; it activates the
                        // focused button and does nothing anywhere else.
                        .on_action(move |_: &Confirm, window, cx| {
                            if route_cancel.contains_focused(window, cx) {
                                window.close_dialog(cx);
                            } else if route_delete.contains_focused(window, cx) {
                                window.close_dialog(cx);
                                let previous = route_previous.clone();
                                let _ = owner.update(cx, |this, cx| {
                                    this.delete_custom_theme(id, previous, window, cx)
                                });
                            }
                        })
                        .child(
                            div().track_focus(&cancel_focus).child(
                                button("delete-theme-cancel", "Cancel", "", false)
                                    .secondary()
                                    .debug_selector(|| "delete-theme-cancel".into())
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            ),
                        )
                        .child(
                            div().track_focus(&delete_focus).child(
                                button("delete-theme-confirm", "Delete theme", "", false)
                                    .danger()
                                    .debug_selector(|| "delete-theme-confirm".into())
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let previous = click_previous.clone();
                                        let _ = delete.update(cx, |this, cx| {
                                            this.delete_custom_theme(id, previous, window, cx)
                                        });
                                    }),
                            ),
                        ),
                )
                // Return with focus on the alert itself neither deletes nor closes.
                .on_ok(|_, _, _| false)
        });
    }

    /// After a delete, New theme… takes keyboard focus. `GitTurtle::render`
    /// calls this while a frame is drawn, and the move is registered only once
    /// that frame shows the card without the row and with its actions enabled,
    /// so `focus_next` from the card walks that frame whatever the store's reply
    /// timing. The row's Delete… was disabled while the delete was pending and
    /// held no focus node, so the modal focus repair may already have given the
    /// lost focus to the page; that, the restored handle, the card, no focus or
    /// a detached focus all count as where the delete left it. A control the
    /// user chose meanwhile keeps focus.
    pub(super) fn return_focus_after_delete(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_editor.save_pending() {
            return;
        }
        let Some(DeleteFocus { restored }) = self.theme_editor.focus_after_delete.take() else {
            return;
        };
        if self.page != AppPage::Settings {
            return;
        }
        let card = self.theme_editor.card_focus(cx);
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| {
                let left_by_delete = window.context_stack().is_empty()
                    || window.focused(cx).is_none_or(|focused| {
                        Some(&focused) == restored.as_ref()
                            || focused == this.app_focus
                            || focused == card
                    });
                if left_by_delete && this.page == AppPage::Settings && !window.has_active_dialog(cx)
                {
                    card.focus(window, cx);
                    window.focus_next(cx);
                }
            });
        });
    }

    /// Remove a saved theme. When it is the selection, its base is selected and
    /// applied once the store confirms the removal. `previous` is the focus the
    /// confirmation restores; when it goes with the row, New theme… takes it.
    pub(super) fn delete_custom_theme(
        &mut self,
        id: u32,
        previous: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_editor.save_pending() {
            return;
        }
        let Some(base) = self
            .custom_themes
            .iter()
            .find(|theme| theme.id == id)
            .map(|theme| theme.base)
        else {
            return;
        };
        let themes = self
            .custom_themes
            .iter()
            .filter(|theme| theme.id != id)
            .cloned()
            .collect();
        self.submit_custom_themes(
            themes,
            SaveOutcome::Deleted { id, base, previous },
            window,
            cx,
        );
    }

    fn submit_custom_themes(
        &mut self,
        themes: Vec<CustomTheme>,
        outcome: SaveOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_editor.pending_saves += 1;
        let response = self
            .preferences_writer
            .submit(move || Preferences::save_custom_themes(&themes));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "The settings writer stopped before reporting a result"
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_custom_theme_save(
                    result.map(|saved| saved.custom_themes),
                    outcome,
                    window,
                    cx,
                );
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_custom_theme_save(
        &mut self,
        result: anyhow::Result<Vec<CustomTheme>>,
        outcome: SaveOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_editor.pending_saves = self.theme_editor.pending_saves.saturating_sub(1);
        match (result, outcome) {
            (Ok(themes), SaveOutcome::Edited { id, form }) => {
                self.custom_themes = themes;
                let _ = form.update(cx, |form, cx| {
                    form.pending = false;
                    form.visible = false;
                    cx.notify();
                });
                if self
                    .theme_editor
                    .form
                    .as_ref()
                    .is_some_and(|open| open.entity_id() == form.entity_id())
                {
                    self.theme_editor.form = None;
                    self.theme_editor.preview = None;
                    window.close_dialog(cx);
                }
                // The window keeps what the editor showed: the saved theme is selected.
                self.settings.theme = ThemeSelection::Custom(id);
                self.settings.follow_system = false;
                self.apply_appearance(window, cx);
                self.save_preferences(window, cx);
            }
            (Err(error), SaveOutcome::Edited { form, .. }) => {
                // The dialog, draft and preview stay; Save can be retried.
                let _ = form.update(cx, |form, cx| {
                    form.pending = false;
                    form.error = Some(format!("Could not save the theme: {error:#}"));
                    cx.notify();
                });
            }
            (Ok(themes), SaveOutcome::Deleted { id, base, previous }) => {
                self.custom_themes = themes;
                self.theme_editor.error = None;
                if self.settings.theme == ThemeSelection::Custom(id) {
                    self.settings.theme = ThemeSelection::BuiltIn(base);
                    self.apply_appearance(window, cx);
                    self.save_preferences(window, cx);
                }
                // The focus the alert restored goes with the deleted row.
                self.theme_editor.focus_after_delete = Some(DeleteFocus { restored: previous });
            }
            (Err(error), SaveOutcome::Deleted { .. }) => {
                self.theme_editor.error = Some(format!("Could not delete the theme: {error:#}"));
            }
        }
        cx.notify();
    }
}

/// A focus target the form scrolls into view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reveal {
    Row(TokenKind),
    Warnings,
}

struct TokenRow {
    kind: TokenKind,
    hex: Entity<InputState>,
    picker: Entity<ColorPickerState>,
    /// The field holds text that is not `#rrggbb`; the draft keeps the last valid value.
    invalid: bool,
}

pub(super) struct ThemeForm {
    owner: WeakEntity<GitTurtle>,
    editing: Option<u32>,
    /// The saved themes when the editor opened, for inline name checks. Save
    /// checks again against the window's current list.
    customs: Vec<CustomTheme>,
    name: Entity<InputState>,
    name_error: Option<String>,
    base: ThemeChoice,
    /// A base chosen after edits, waiting for Replace colors or Keep colors.
    pending_base: Option<ThemeChoice>,
    draft: Palette,
    rows: Vec<TokenRow>,
    warnings: Vec<ReadabilityIssue>,
    pending: bool,
    visible: bool,
    error: Option<String>,
    /// The token column (wide layout) or the whole body (stacked): group
    /// labels and rows are its direct children, so a focused row scrolls into view.
    scroll: ScrollHandle,
    /// The wide layout's preview-and-warnings column.
    side_scroll: ScrollHandle,
    /// The last render used the two-column layout.
    wide: bool,
    /// Wrap the toolkit buttons, whose own focus handles are private. Return
    /// arrives as the dialog's Confirm action before any key listener runs, so
    /// `confirm_pressed` activates the wrapped button that contains focus.
    keep_focus: FocusHandle,
    replace_focus: FocusHandle,
    reset_focus: FocusHandle,
    cancel_focus: FocusHandle,
    save_focus: FocusHandle,
    /// The focused row or Readability list the last render scrolled into view.
    revealed: Option<Reveal>,
    /// The Readability list is a tab stop, so the keyboard can bring it into view.
    warnings_focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl ThemeForm {
    #[allow(clippy::too_many_arguments)]
    fn new(
        owner: WeakEntity<GitTurtle>,
        editing: Option<u32>,
        name: String,
        base: ThemeChoice,
        palette: Palette,
        customs: Vec<CustomTheme>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_error = custom::validate_theme_name(&name, &customs, editing).err();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(name)
                .placeholder("Theme name")
        });
        // Return reaches the form as the dialog's Confirm action (see
        // `confirm_pressed`), so no field subscribes to PressEnter.
        let mut subscriptions =
            vec![
                cx.subscribe_in(&name, window, |this, _, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        let name = this.name.read(cx).value().to_string();
                        this.name_error =
                            custom::validate_theme_name(&name, &this.customs, this.editing).err();
                        cx.notify();
                    }
                }),
            ];
        let warnings_focus = cx.focus_handle().tab_stop(true);
        let mut rows = Vec::with_capacity(TokenKind::ALL.len());
        for kind in TokenKind::ALL {
            let color = palette.get(kind);
            let hex = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(custom::format_hex(color))
                    .placeholder("#rrggbb")
            });
            let picker =
                cx.new(|cx| ColorPickerState::new(window, cx).default_value(rgb_hsla(color)));
            subscriptions.push(cx.subscribe_in(
                &hex,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::Change = event {
                        this.hex_changed(kind, window, cx);
                    }
                },
            ));
            subscriptions.push(cx.subscribe_in(
                &picker,
                window,
                move |this, _, event: &ColorPickerEvent, window, cx| {
                    if let ColorPickerEvent::Change(Some(color)) = event {
                        this.picker_changed(kind, *color, window, cx);
                    }
                },
            ));
            rows.push(TokenRow {
                kind,
                hex,
                picker,
                invalid: false,
            });
        }
        Self {
            owner,
            editing,
            customs,
            name,
            name_error,
            base,
            pending_base: None,
            draft: palette,
            rows,
            warnings: palette.readability_issues(),
            pending: false,
            visible: true,
            error: None,
            scroll: ScrollHandle::new(),
            side_scroll: ScrollHandle::new(),
            wide: false,
            keep_focus: cx.focus_handle(),
            replace_focus: cx.focus_handle(),
            reset_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            save_focus: cx.focus_handle(),
            revealed: None,
            warnings_focus,
            _subscriptions: subscriptions,
        }
    }

    /// Group labels and token rows, in render order.
    const TOKEN_ENTRIES: usize = TokenGroup::ALL.len() + TokenKind::ALL.len();

    /// The Base button and its menu share this width.
    fn base_width() -> Pixels {
        appearance::ui_size(200.)
    }

    /// Seven monospace characters at the small input size, the caret and the
    /// toolkit's 10 px right margin (below which the field scrolls the leading
    /// `#` out of view), inside 8 px padding and a 1 px border on each side.
    fn hex_width() -> Pixels {
        appearance::ui_size(78.)
    }

    /// The narrowest hex field that never scrolls a seven-character value:
    /// the toolkit keeps the caret [`Self::HEX_CARET_MARGIN`] inside the text
    /// area, which the small input pads by 8 px and borders by 1 px per side.
    #[cfg(test)]
    fn hex_fit_width(window: &Window) -> Pixels {
        let font = gpui::Font {
            family: mono().into(),
            ..Default::default()
        };
        let size = window.rem_size() * 0.875;
        let text: SharedString = "#ffffff".into();
        let run = TextRun {
            len: text.len(),
            font,
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(text, size, &[run], None);
        line.width() + Self::HEX_CARET_MARGIN + px(2. * 8.) + px(2. * 1.)
    }

    /// `RIGHT_MARGIN` of the toolkit's single-line input element.
    #[cfg(test)]
    const HEX_CARET_MARGIN: Pixels = px(10.);

    /// The Base menu opens under the header, a tenth of the way down plus the
    /// title and Name row; it may use the window below that and then scrolls.
    fn base_menu_height(window: &Window) -> Pixels {
        (window.viewport_size().height * 0.9 - appearance::ui_size(120.))
            .max(appearance::ui_size(240.))
    }

    /// The tallest the form may be: the window less the alert's fixed chrome.
    /// The alert opens a tenth of the way down and sizes to its content; around
    /// the form it adds 16 px panel padding above and below, the one-rem
    /// (13 px) title with an 8 px gap under it, a 16 px gap above the footer
    /// and the 28 px footer buttons, and 16 px stay under the panel. The body
    /// takes what the header and base question leave, by layout.
    fn max_height(window: &Window) -> Pixels {
        let viewport = window.viewport_size().height;
        let chrome =
            viewport / 10. + px(16. + 8. + 16. + 16.) + appearance::ui_size(13. + 28. + 16.);
        (viewport - chrome).max(px(200.))
    }

    /// A token row's index among the scroll container's children, which are
    /// the group labels and rows interleaved in `TokenKind::ALL` order.
    fn row_entry(kind: TokenKind) -> usize {
        let mut index = 0;
        for group in TokenGroup::ALL {
            index += 1;
            for candidate in TokenKind::ALL.iter().filter(|k| k.group() == group) {
                if *candidate == kind {
                    return index;
                }
                index += 1;
            }
        }
        index
    }

    /// A row whose hex field or picker gains focus, or the Readability list,
    /// scrolls into view in the draw that shows the focus. A focus change
    /// refreshes the window, so this render belongs to that draw and the
    /// target is applied by its prepaint. Focus listeners would be too late:
    /// GPUI dispatches them after the draw's layout, where a refresh no longer
    /// produces a frame, so the scroll would wait for the next input. In the
    /// wide layout the list follows the miniature in the side column; stacked,
    /// it follows the rows.
    fn reveal_focused(&mut self, window: &Window, cx: &App) {
        let focused = if self.warnings_focus.is_focused(window) {
            Some(Reveal::Warnings)
        } else {
            self.rows
                .iter()
                .find(|row| {
                    row.hex.read(cx).focus_handle(cx).is_focused(window)
                        || row.picker.focus_handle(cx).is_focused(window)
                })
                .map(|row| Reveal::Row(row.kind))
        };
        if focused == self.revealed {
            return;
        }
        self.revealed = focused;
        match focused {
            Some(Reveal::Row(kind)) => self.scroll.scroll_to_item(Self::row_entry(kind)),
            Some(Reveal::Warnings) if self.wide => self.side_scroll.scroll_to_item(1),
            Some(Reveal::Warnings) => self.scroll.scroll_to_item(Self::TOKEN_ENTRIES + 1),
            None => {}
        }
    }

    /// Return, delivered as the dialog's Confirm action and routed by focus:
    /// Name submits, a hex field commits its value, a wrapped button activates
    /// (the toolkit's own Enter click never runs once the action is handled),
    /// and anything else (Base, a picker, the Readability list) does nothing.
    fn confirm_pressed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(focused) = window.focused(cx) else {
            return;
        };
        if focused == self.name.read(cx).focus_handle(cx) {
            self.submit(window, cx);
        } else if let Some(kind) = self
            .rows
            .iter()
            .find(|row| row.hex.read(cx).focus_handle(cx) == focused)
            .map(|row| row.kind)
        {
            self.hex_committed(kind, window, cx);
        } else if self.keep_focus.contains_focused(window, cx) {
            self.resolve_base(false, window, cx);
        } else if self.replace_focus.contains_focused(window, cx) {
            self.resolve_base(true, window, cx);
        } else if self.reset_focus.contains_focused(window, cx) {
            self.reset(window, cx);
        } else if self.cancel_focus.contains_focused(window, cx) {
            if self.cancel(window, cx) {
                window.close_dialog(cx);
            }
        } else if self.save_focus.contains_focused(window, cx) {
            self.submit(window, cx);
        }
    }

    fn row(&self, kind: TokenKind) -> &TokenRow {
        &self.rows[TokenKind::ALL
            .iter()
            .position(|candidate| *candidate == kind)
            .expect("every token has a row")]
    }

    fn row_mut(&mut self, kind: TokenKind) -> &mut TokenRow {
        let index = TokenKind::ALL
            .iter()
            .position(|candidate| *candidate == kind)
            .expect("every token has a row");
        &mut self.rows[index]
    }

    fn can_save(&self) -> bool {
        !self.pending && self.name_error.is_none() && self.rows.iter().all(|row| !row.invalid)
    }

    /// Typing in a hex field: a valid value updates the draft; anything else
    /// marks the field and keeps the last valid draft.
    fn hex_changed(&mut self, kind: TokenKind, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.row(kind).hex.read(cx).value().to_string();
        match custom::parse_hex(value.trim()) {
            Some(color) => {
                self.row_mut(kind).invalid = false;
                if self.draft.get(kind) != color {
                    self.draft.set(kind, color);
                    self.sync_picker(kind, color, window, cx);
                    self.draft_changed(window, cx);
                }
            }
            None => self.row_mut(kind).invalid = true,
        }
        cx.notify();
    }

    /// Return in a hex field writes the value back in canonical `#rrggbb` form.
    fn hex_committed(&mut self, kind: TokenKind, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.row(kind).hex.read(cx).value().to_string();
        if let Some(color) = custom::parse_hex(value.trim()) {
            let text = custom::format_hex(color);
            if text != value {
                self.row(kind)
                    .hex
                    .clone()
                    .update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
    }

    fn picker_changed(
        &mut self,
        kind: TokenKind,
        color: Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let color = hsla_rgb(color);
        if self.draft.get(kind) == color && !self.row(kind).invalid {
            return;
        }
        self.draft.set(kind, color);
        self.row_mut(kind).invalid = false;
        let hex = self.row(kind).hex.clone();
        hex.update(cx, |input, cx| {
            input.set_value(custom::format_hex(color), window, cx)
        });
        self.draft_changed(window, cx);
        cx.notify();
    }

    fn sync_picker(&self, kind: TokenKind, color: u32, window: &mut Window, cx: &mut App) {
        let picker = self.row(kind).picker.clone();
        if picker.read(cx).value().map(hsla_rgb) != Some(color) {
            picker.update(cx, |picker, cx| {
                picker.set_value(rgb_hsla(color), window, cx)
            });
        }
    }

    /// Recompute the warnings and hand the draft to the window's coalesced preview.
    fn draft_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.warnings = self.draft.readability_issues();
        let draft = self.draft;
        let _ = self
            .owner
            .update(cx, |this, cx| this.preview_theme_draft(draft, window, cx));
    }

    /// Replace every token, as Reset to base and a base change do.
    fn set_palette(&mut self, palette: Palette, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = palette;
        for index in 0..self.rows.len() {
            let kind = self.rows[index].kind;
            let color = palette.get(kind);
            self.rows[index].invalid = false;
            let hex = self.rows[index].hex.clone();
            hex.update(cx, |input, cx| {
                input.set_value(custom::format_hex(color), window, cx)
            });
            self.sync_picker(kind, color, window, cx);
        }
        self.draft_changed(window, cx);
        cx.notify();
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending_base = None;
        self.set_palette(self.base.palette(), window, cx);
    }

    /// Choose a new base. An unedited draft follows it; after edits the form
    /// asks before replacing the draft tokens.
    fn choose_base(&mut self, base: ThemeChoice, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if base == self.base {
            self.pending_base = None;
        } else if self.draft == self.base.palette() && self.rows.iter().all(|row| !row.invalid) {
            self.base = base;
            self.pending_base = None;
            self.set_palette(base.palette(), window, cx);
        } else {
            self.pending_base = Some(base);
        }
        cx.notify();
    }

    /// Answer the base-change question: both answers adopt the base.
    fn resolve_base(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base) = self.pending_base.take() else {
            return;
        };
        // The answered question leaves the tree with the focused button; hand
        // focus back to Base, the tab stop before Keep colors, while the
        // rendered frame still has the question. Replace colors is one further.
        if self.keep_focus.contains_focused(window, cx) {
            window.focus_prev(cx);
        } else if self.replace_focus.contains_focused(window, cx) {
            window.focus_prev(cx);
            window.focus_prev(cx);
        }
        self.base = base;
        if replace {
            self.set_palette(base.palette(), window, cx);
        }
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_save() {
            return;
        }
        let editing = self.editing;
        let theme = CustomTheme {
            id: editing.unwrap_or_default(),
            name: self.name.read(cx).value().trim().to_owned(),
            base: self.base,
            palette: self.draft,
        };
        let form = cx.entity().downgrade();
        let result = self
            .owner
            .update(cx, |this, cx| {
                this.save_theme_from_editor(theme, editing, form, window, cx)
            })
            .unwrap_or_else(|_| Err("The window closed before the theme was saved.".into()));
        match result {
            Ok(()) => {
                self.pending = true;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    /// Cancel, Escape and closing the dialog: drop the draft and re-apply the
    /// saved selection. Refused while a save is in flight.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.pending || !self.visible {
            return false;
        }
        self.visible = false;
        let _ = self
            .owner
            .update(cx, |this, cx| this.close_theme_editor(window, cx));
        true
    }

    /// Reset to base on the left; the save error, Cancel and Save on the right.
    fn footer(form: &Entity<Self>, cx: &App) -> AnyElement {
        let p = palette(cx);
        let state = form.read(cx);
        let reset = form.clone();
        let cancel = form.clone();
        let save = form.clone();
        let confirm = form.clone();
        let (reset_focus, cancel_focus, save_focus) = (
            state.reset_focus.clone(),
            state.cancel_focus.clone(),
            state.save_focus.clone(),
        );
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            // Return on a footer button is the dialog's Confirm; the form
            // activates the focused button instead of letting on_ok save.
            .on_action(move |_: &Confirm, window, cx| {
                confirm.update(cx, |form, cx| form.confirm_pressed(window, cx))
            })
            .child(
                div().track_focus(&reset_focus).child(
                    button("theme-editor-reset", "Reset to base", "", false)
                        .secondary()
                        .disabled(state.pending)
                        .accessibility_label(format!("Reset every color to {}", state.base.label()))
                        .on_click(move |_, window, cx| {
                            reset.update(cx, |form, cx| form.reset(window, cx))
                        }),
                ),
            )
            .child(div().flex_1())
            .children(state.error.clone().map(|error| {
                div()
                    .id("theme-editor-error")
                    .role(Role::Label)
                    .aria_label(error.clone())
                    .min_w_0()
                    .max_w(px(420.))
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.warning))
                    .child(error)
            }))
            .when(state.pending, |footer| {
                footer.child(
                    div()
                        .text_size(appearance::ui_text(12.))
                        .text_color(rgb(p.muted))
                        .child("Saving…"),
                )
            })
            .child(
                div().track_focus(&cancel_focus).child(
                    button("theme-editor-cancel", "Cancel", "", false)
                        .secondary()
                        .disabled(state.pending)
                        .on_click(move |_, window, cx| {
                            if cancel.update(cx, |form, cx| form.cancel(window, cx)) {
                                window.close_dialog(cx);
                            }
                        }),
                ),
            )
            .child(
                div().track_focus(&save_focus).child(
                    button("theme-editor-save", "Save", "", false)
                        .primary()
                        .debug_selector(|| "theme-editor-save".into())
                        .disabled(!state.can_save())
                        .on_click(move |_, window, cx| {
                            save.update(cx, |form, cx| form.submit(window, cx))
                        }),
                ),
            )
            .into_any_element()
    }

    fn render_header(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let form = cx.entity().downgrade();
        let current = self.base;
        let field_label = |text: &'static str| {
            div()
                .text_size(appearance::ui_text(12.))
                .text_color(rgb(p.text))
                .font_weight(FontWeight::MEDIUM)
                .child(text)
        };
        div()
            .flex()
            .items_start()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field_label("Name"))
                    .child(
                        div().debug_selector(|| "theme-editor-name".into()).child(
                            Input::new(&self.name)
                                .aria_label("Theme name")
                                .disabled(self.pending)
                                // Match the 28 px Base button beside it.
                                .min_h(appearance::ui_size(28.))
                                // The toolkit's focus border would hide the
                                // removed outline while the name is edited.
                                .when(self.name_error.is_some(), |input| {
                                    input.focus_ring(false).border_color(rgb(p.removed))
                                }),
                        ),
                    )
                    .children(self.name_error.clone().map(|error| {
                        div()
                            .id("theme-editor-name-error")
                            .debug_selector(|| "theme-editor-name-error".into())
                            .role(Role::Label)
                            .aria_label(error.clone())
                            .text_size(appearance::ui_text(11.))
                            .text_color(rgb(p.warning))
                            .child(error)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field_label("Base"))
                    .child(
                        Button::new("theme-editor-base")
                            .secondary()
                            .debug_selector(|| "theme-editor-base".into())
                            .h(appearance::ui_size(28.))
                            .w(Self::base_width())
                            .px(appearance::ui_size(10.))
                            .text_size(appearance::ui_text(12.))
                            .label(current.label())
                            .dropdown_caret(true)
                            .disabled(self.pending)
                            .accessibility_label(format!("Base theme: {}", current.label()))
                            .dropdown_menu(move |mut menu, window, _| {
                                // As wide as the button; as tall as the window
                                // below the header allows, then scrolling.
                                menu = menu
                                    .min_w(Self::base_width())
                                    .max_h(Self::base_menu_height(window))
                                    .scrollable(true);
                                for light in [true, false] {
                                    menu = menu.label(if light {
                                        "Light palettes"
                                    } else {
                                        "Dark palettes"
                                    });
                                    for choice in ThemeChoice::ALL
                                        .into_iter()
                                        .filter(|choice| choice.is_light() == light)
                                    {
                                        let form = form.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(choice.label())
                                                .checked(choice == current)
                                                .on_click(move |_, window, cx| {
                                                    let _ = form.update(cx, |form, cx| {
                                                        form.choose_base(choice, window, cx)
                                                    });
                                                }),
                                        );
                                    }
                                }
                                menu
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_base_question(&self, p: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let base = self.pending_base?;
        let question = format!("Replace your edited colors with {}’s colors?", base.label());
        Some(
            div()
                .id("theme-editor-base-question")
                .p_2()
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(p.border))
                .bg(rgb(p.subtle))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("theme-editor-base-question-text")
                        .flex_1()
                        .min_w_0()
                        .role(Role::Label)
                        .aria_label(question.clone())
                        .text_size(appearance::ui_text(12.))
                        .child(question),
                )
                .child(
                    div().track_focus(&self.keep_focus).child(
                        button("theme-editor-keep-colors", "Keep colors", "", false)
                            .secondary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.resolve_base(false, window, cx)
                            })),
                    ),
                )
                .child(
                    div().track_focus(&self.replace_focus).child(
                        button("theme-editor-replace-colors", "Replace colors", "", false)
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.resolve_base(true, window, cx)
                            })),
                    ),
                )
                .into_any_element(),
        )
    }

    fn render_row(
        &self,
        row: &TokenRow,
        flagged: bool,
        p: Palette,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let kind = row.kind;
        let color = self.draft.get(kind);
        let label = kind.label();
        let picker_focused = row.picker.focus_handle(cx).is_focused(window);
        div()
            .id(("theme-token", kind as usize))
            .debug_selector(move || format!("theme-token-{}", kind.key()))
            .h(appearance::ui_size(30.))
            // The scroll column shrinks to the window; rows keep their height.
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(appearance::ui_size(128.))
                    .flex_shrink_0()
                    .truncate()
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.text))
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(kind.description()),
            )
            .child(if row.invalid {
                invalid_glyph(p)
                    .id(("theme-token-invalid", kind as usize))
                    .debug_selector(move || format!("theme-token-invalid-{}", kind.key()))
                    .role(Role::Label)
                    .aria_label(format!("{label} value is not #rrggbb"))
                    .into_any_element()
            } else if flagged {
                warning_glyph(p)
                    .id(("theme-token-warning", kind as usize))
                    .role(Role::Label)
                    .aria_label(format!("{label} has a readability warning"))
                    .into_any_element()
            } else {
                div()
                    .size(appearance::ui_size(14.))
                    .flex_shrink_0()
                    .into_any_element()
            })
            .child(
                div()
                    .size(appearance::ui_size(16.))
                    .flex_shrink_0()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(color)),
            )
            .child(
                div()
                    .debug_selector(move || format!("theme-hex-{}", kind.key()))
                    .w(Self::hex_width())
                    .flex_shrink_0()
                    .font_family(mono())
                    .child(
                        Input::new(&row.hex)
                            .small()
                            .disabled(self.pending)
                            .aria_label(if row.invalid {
                                format!("{label} color, invalid: use #rrggbb")
                            } else {
                                format!("{label} color")
                            })
                            // The toolkit's focus border would hide the
                            // removed outline while the field is edited.
                            .when(row.invalid, |input| {
                                input.focus_ring(false).border_color(rgb(p.removed))
                            }),
                    ),
            )
            .child(
                // The toolkit trigger paints no focus indicator of its own, and
                // its swatch would vanish on a matching background: a 1 px
                // border edge at rest, the 2 px accent ring when focused.
                div()
                    .rounded(px(7.))
                    .map(|ring| {
                        if picker_focused {
                            ring.border_2().border_color(rgb(p.accent))
                        } else {
                            ring.border_1().border_color(rgb(p.border)).p(px(1.))
                        }
                    })
                    .child(
                        ColorPicker::new(&row.picker)
                            .small()
                            .anchor(Anchor::TopRight)
                            .accessibility_label(format!("Choose the {label} color")),
                    ),
            )
            .into_any_element()
    }

    /// The token column's always-visible scrollbar (`DESIGN.md`: a bounded
    /// scrolling area shows one), in the track the body reserves. The wrapper
    /// marks the track for tests; the scrollbar positions itself over the
    /// tracked container.
    fn render_scrollbar(&self) -> AnyElement {
        div()
            .debug_selector(|| "theme-editor-scrollbar".into())
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(Scrollbar::width())
            .child(
                Scrollbar::vertical(&self.scroll)
                    .id("theme-editor-scrollbar")
                    .mode(ScrollbarMode::Always),
            )
            .into_any_element()
    }

    fn render_warnings(&self, p: Palette, window: &Window) -> AnyElement {
        let count = self.warnings.len();
        let focused = self.warnings_focus.is_focused(window);
        div()
            .id("theme-editor-warnings")
            // A tab stop after the last row: the list is content, not a
            // control, but focusing it scrolls it into view.
            .track_focus(&self.warnings_focus)
            .role(Role::Group)
            .aria_label(format!(
                "Readability, {count} warning{}",
                if count == 1 { "" } else { "s" }
            ))
            .rounded(px(8.))
            .border_1()
            .border_color(transparent_black())
            .when(focused, |list| list.border_color(rgb(p.accent)))
            .p_1()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.text))
                    .child("Readability")
                    .when(count > 0, |heading| {
                        heading.child(
                            div()
                                .text_color(rgb(p.muted))
                                .font_weight(FontWeight::NORMAL)
                                .child(format!(
                                    "{count} warning{}",
                                    if count == 1 { "" } else { "s" }
                                )),
                        )
                    }),
            )
            .when(count == 0, |list| {
                list.child(
                    div()
                        .text_size(appearance::ui_text(11.))
                        .text_color(rgb(p.muted))
                        .child("Every pair meets its contrast minimum."),
                )
            })
            .children(self.warnings.iter().enumerate().map(|(index, issue)| {
                let text = issue.to_string();
                div()
                    .id(("theme-warning", index))
                    .debug_selector(move || format!("theme-warning-{index}"))
                    .role(Role::Label)
                    .aria_label(text.clone())
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(warning_glyph(p))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(appearance::ui_text(11.))
                            .line_height(relative(1.4))
                            .text_color(rgb(p.text))
                            .child(text),
                    )
            }))
            .when(count > 0, |list| {
                list.child(
                    div()
                        .text_size(appearance::ui_text(11.))
                        .text_color(rgb(p.muted))
                        .child("Warnings do not prevent saving."),
                )
            })
            .into_any_element()
    }
}

impl Render for ThemeForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let viewport = window.viewport_size();
        let wide = viewport.width >= appearance::ui_size(1060.);
        self.wide = wide;
        self.reveal_focused(window, cx);
        let max_height = Self::max_height(window);
        let flagged: std::collections::HashSet<TokenKind> =
            self.warnings.iter().flat_map(issue_tokens).collect();
        // Group labels and rows are direct children of the scroll container,
        // so `ScrollHandle::scroll_to_item` can reveal a focused row.
        let mut entries: Vec<AnyElement> = Vec::with_capacity(Self::TOKEN_ENTRIES + 2);
        for (index, group) in TokenGroup::ALL.into_iter().enumerate() {
            entries.push(
                div()
                    .id(("theme-group", index))
                    .debug_selector(move || format!("theme-group-{index}"))
                    .h(appearance::ui_size(22.))
                    .flex_shrink_0()
                    .when(index > 0, |label| label.mt_2())
                    .flex()
                    .items_end()
                    .pb_1()
                    .text_size(appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.muted))
                    .child(group.label())
                    .into_any_element(),
            );
            entries.extend(
                self.rows
                    .iter()
                    .filter(|row| row.kind.group() == group)
                    .map(|row| self.render_row(row, flagged.contains(&row.kind), p, window, cx)),
            );
        }
        let name = self.name.read(cx).value().trim().to_owned();
        let preview = div()
            .h(appearance::ui_size(166.))
            .flex_shrink_0()
            .p(px(2.))
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(p.border))
            .overflow_hidden()
            .child(settings::theme_preview(
                self.draft,
                if name.is_empty() {
                    "Untitled theme".to_owned()
                } else {
                    name
                },
                format!("Based on {}", self.base.label()),
                false,
                p.accent,
                p.accent_foreground,
            ));
        let warnings = self.render_warnings(p, window);
        // The body shrinks to what the capped form leaves; the columns stretch
        // to the body and scroll within it.
        let body = if wide {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .gap_5()
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("theme-editor-body")
                                .debug_selector(|| "theme-editor-body".into())
                                .flex_1()
                                .min_h_0()
                                // The scrollbar paints an absolute overlay;
                                // reserve its track so the thumb never
                                // covers the pickers.
                                .pr(Scrollbar::width())
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll)
                                .flex()
                                .flex_col()
                                .children(entries),
                        )
                        .child(self.render_scrollbar()),
                )
                .child(
                    div()
                        .id("theme-editor-side")
                        .w(px(300.))
                        .flex_shrink_0()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.side_scroll)
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(preview)
                        .child(warnings),
                )
                .into_any_element()
        } else {
            entries.push(preview.mt_5().into_any_element());
            entries.push(div().mt_4().child(warnings).into_any_element());
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("theme-editor-body")
                        .debug_selector(|| "theme-editor-body".into())
                        .flex_1()
                        .min_h_0()
                        .pr(Scrollbar::width())
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .flex()
                        .flex_col()
                        .children(entries),
                )
                .child(self.render_scrollbar())
                .into_any_element()
        };
        div()
            .id("theme-editor")
            .debug_selector(|| "theme-editor".into())
            .max_h(max_height)
            .flex()
            .flex_col()
            .gap_3()
            .on_action(
                cx.listener(|this, _: &Confirm, window, cx| this.confirm_pressed(window, cx)),
            )
            .child(self.render_header(p, cx))
            .children(self.render_base_question(p, cx))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::{cell::RefCell, rc::Rc};

    fn open_app(cx: &mut TestAppContext) -> (Entity<GitTurtle>, &mut VisualTestContext) {
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
        // Tall enough that the Your themes card is on screen without scrolling.
        cx.simulate_resize(size(px(1440.), px(2400.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                // Startup applies the saved selection before the first frame.
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

    /// Exactly one draw, as the window's next frame after an input.
    fn draw_once(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    fn bounds(cx: &mut VisualTestContext, selector: String) -> Bounds<Pixels> {
        let selector: &'static str = selector.leak();
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is rendered"))
    }

    /// Deliver the next frame (and its coalesced preview), then draw.
    fn next_frame(cx: &mut VisualTestContext) -> usize {
        let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
        settle(cx);
        ran
    }

    /// Wait for replies from the real preference writer thread.
    fn wait_for(cx: &mut VisualTestContext, mut done: impl FnMut(&mut VisualTestContext) -> bool) {
        for _ in 0..500 {
            settle(cx);
            if done(cx) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the preference writer did not answer");
    }

    fn form(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> Entity<ThemeForm> {
        cx.read(|cx| app.read(cx).theme_editor.form.clone())
            .expect("the theme editor is open")
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is rendered"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        settle(cx);
    }

    fn type_hex(cx: &mut VisualTestContext, form: &Entity<ThemeForm>, kind: TokenKind, text: &str) {
        let input = cx.read(|cx| form.read(cx).row(kind).hex.clone());
        cx.update(|window, cx| {
            input.update(cx, |input, cx| input.set_value("", window, cx));
            input.read(cx).focus_handle(cx).focus(window, cx);
        });
        settle(cx);
        cx.simulate_input(text);
        settle(cx);
    }

    /// A full key press: GPUI's Enter and Space click needs the key up.
    fn press(cx: &mut VisualTestContext, key: &str) {
        let keystroke = Keystroke::parse(key).expect("a keystroke");
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
        settle(cx);
    }

    /// One native frame in GPUI's order: the frame callbacks registered so far
    /// run, then the window draws once. The test app also draws a dirty window
    /// whenever it flushes effects; here that flush follows the frame's draw.
    fn native_frame(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }

    /// A full key press as native input, observed right after the frame that
    /// follows the key-up. See [`key_frames`].
    fn key_frame<R>(
        cx: &mut VisualTestContext,
        key: &str,
        observe: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> R {
        key_frames(cx, key, |_, _| (), observe).1
    }

    /// A full key press as native input: the key-down, the display frame that
    /// follows it, the key-up, and the frame that follows that. Each frame runs
    /// the frame callbacks registered so far and then draws once; `down` and
    /// `up` observe the window right after those two draws. Natively a key
    /// draws nothing by itself (GPUI draws a dirty window before dispatching
    /// the next key, here the key-up), while the test app would draw as soon
    /// as the input's effects flush, before any callback; so the whole press
    /// shares one update and nothing else draws in between.
    fn key_frames<D, U>(
        cx: &mut VisualTestContext,
        key: &str,
        down: impl FnOnce(&mut Window, &mut App) -> D,
        up: impl FnOnce(&mut Window, &mut App) -> U,
    ) -> (D, U) {
        fn frame(window: &mut Window, cx: &mut App) {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        }
        let keystroke = Keystroke::parse(key).expect("a keystroke");
        let observed = cx.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            frame(window, cx);
            let down = down(window, cx);
            window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
            frame(window, cx);
            (down, up(window, cx))
        });
        cx.run_until_parked();
        observed
    }

    /// Wait for the preference writer's reply without drawing a frame.
    fn wait_without_drawing(
        cx: &mut VisualTestContext,
        mut done: impl FnMut(&mut VisualTestContext) -> bool,
    ) {
        for _ in 0..500 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the preference writer did not answer");
    }

    fn applied(cx: &mut VisualTestContext) -> Palette {
        cx.read(|cx| *cx.global::<Palette>())
    }

    fn submissions(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> (u64, bool, bool) {
        cx.read(|cx| {
            let app = app.read(cx);
            (app.generation, app.task.is_some(), app.loading.is_some())
        })
    }

    #[gpui::test]
    fn new_edit_save_cancel_and_delete_follow_the_saved_selection(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let initial = applied(cx);
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.choose_base(ThemeChoice::Nord, window, cx)
            })
        });
        next_frame(cx);
        assert_eq!(cx.read(|cx| form.read(cx).base), ThemeChoice::Nord);
        assert_eq!(
            applied(cx),
            ThemeChoice::Nord.palette(),
            "an unedited draft follows its base"
        );

        type_hex(cx, &form, TokenKind::Accent, "#12AB34");
        next_frame(cx);
        let draft = cx.read(|cx| form.read(cx).draft);
        assert_eq!(draft.accent, 0x12ab34);
        assert_eq!(applied(cx), draft, "the window shows the draft");

        cx.update(|window, cx| {
            form.read(cx)
                .name
                .clone()
                .update(cx, |input, cx| input.replace_all("Harbor", window, cx))
        });
        settle(cx);
        click(cx, "theme-editor-save");
        wait_for(cx, |cx| {
            cx.read(|cx| app.read(cx).theme_editor.form.is_none())
        });
        let saved = Preferences::load();
        let theme = saved
            .custom_themes
            .iter()
            .find(|theme| theme.name == "Harbor")
            .expect("the saved theme is in the loaded preferences")
            .clone();
        assert_eq!(theme.base, ThemeChoice::Nord);
        assert_eq!(theme.palette, draft);
        wait_for(cx, |cx| {
            cx.read(|cx| {
                app.read(cx).settings.theme == ThemeSelection::Custom(theme.id)
                    && !app.read(cx).theme_editor.save_pending()
            })
        });
        assert_eq!(applied(cx), draft, "the saved theme stays applied");
        assert_ne!(applied(cx), initial);

        // Cancel after edits restores the palette applied before the editor opened.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_theme_editor(Some(theme.id), window, cx)
            })
        });
        settle(cx);
        let form = self::form(cx, &app);
        type_hex(cx, &form, TokenKind::Canvas, "000000");
        next_frame(cx);
        assert_eq!(applied(cx).canvas, 0x000000);
        cx.simulate_keystrokes("escape");
        settle(cx);
        assert!(cx.read(|cx| app.read(cx).theme_editor.form.is_none()));
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(applied(cx), draft, "Escape re-applied the saved selection");
        assert_eq!(
            Preferences::load().custom_themes,
            vec![theme.clone()],
            "nothing unsaved reached the store"
        );

        // Deleting the active theme selects and applies its base.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.confirm_delete_theme(theme.id, window, cx))
        });
        settle(cx);
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        cx.update(|window, cx| {
            window.close_dialog(cx);
            app.update(cx, |app, cx| {
                app.delete_custom_theme(theme.id, None, window, cx)
            })
        });
        wait_for(cx, |cx| cx.read(|cx| app.read(cx).custom_themes.is_empty()));
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.theme),
            ThemeSelection::BuiltIn(ThemeChoice::Nord)
        );
        assert_eq!(applied(cx), ThemeChoice::Nord.palette());
        assert!(Preferences::load().custom_themes.is_empty());
    }

    #[gpui::test]
    fn invalid_values_names_and_readability_warnings(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.choose_base(ThemeChoice::Daylight, window, cx)
            })
        });
        next_frame(cx);
        assert!(cx.read(|cx| form.read(cx).can_save()));

        // An invalid value marks its field, keeps the last valid draft and disables Save.
        let before = cx.read(|cx| form.read(cx).draft);
        type_hex(cx, &form, TokenKind::Panel, "#12zz");
        next_frame(cx);
        cx.read(|cx| {
            let form = form.read(cx);
            assert!(form.row(TokenKind::Panel).invalid);
            assert_eq!(form.draft, before);
            assert!(!form.can_save());
        });
        assert_eq!(applied(cx), before);
        // The row's warning slot shows the invalid glyph, a cue beyond the
        // field's outline color, until the value is valid again.
        assert!(
            cx.debug_bounds("theme-token-invalid-panel").is_some(),
            "the invalid row shows its glyph"
        );
        type_hex(
            cx,
            &form,
            TokenKind::Panel,
            &custom::format_hex(before.panel),
        );
        assert!(cx.read(|cx| form.read(cx).can_save()));
        assert!(
            cx.debug_bounds("theme-token-invalid-panel").is_none(),
            "a valid value clears the glyph"
        );

        // A built-in label shows validate_theme_name's message and disables Save.
        let name = cx.read(|cx| form.read(cx).name.clone());
        cx.update(|window, cx| name.update(cx, |input, cx| input.replace_all("nord", window, cx)));
        settle(cx);
        let expected = custom::validate_theme_name("nord", &[], None).unwrap_err();
        cx.read(|cx| {
            assert_eq!(form.read(cx).name_error.as_deref(), Some(expected.as_str()));
            assert!(!form.read(cx).can_save());
        });
        assert!(cx.debug_bounds("theme-editor-name-error").is_some());

        // A name collision with a saved theme shows its message too.
        let taken = CustomTheme::from_base(4, "Sunrise", ThemeChoice::Daylight);
        cx.update(|_, cx| form.update(cx, |form, _| form.customs = vec![taken.clone()]));
        cx.update(|window, cx| {
            name.update(cx, |input, cx| input.replace_all("SUNRISE", window, cx))
        });
        settle(cx);
        let expected = custom::validate_theme_name("SUNRISE", &[taken], None).unwrap_err();
        assert_eq!(
            cx.read(|cx| form.read(cx).name_error.clone()),
            Some(expected)
        );
        cx.update(|window, cx| name.update(cx, |input, cx| input.replace_all("Paper", window, cx)));
        settle(cx);

        // Muted text as light as the selected surface fails a readability rule:
        // the warning names both tokens, the ratio and the minimum, and Save stays enabled.
        let selected = cx.read(|cx| form.read(cx).draft.selected);
        type_hex(cx, &form, TokenKind::Muted, &custom::format_hex(selected));
        next_frame(cx);
        let (warnings, can_save) = cx.read(|cx| {
            let form = form.read(cx);
            (form.warnings.clone(), form.can_save())
        });
        assert!(can_save, "warnings never block Save");
        assert_eq!(
            warnings,
            cx.read(|cx| form.read(cx).draft.readability_issues())
        );
        let issue = warnings
            .iter()
            .find(|issue| {
                issue.foreground == ReadabilityForeground::Token(TokenKind::Muted)
                    && issue.background == ReadabilityBackground::Token(TokenKind::Selected)
            })
            .expect("muted on selected is reported");
        let text = issue.to_string();
        assert!(text.starts_with("Muted text on Selected 1.0:1"), "{text}");
        assert!(text.ends_with("needs 4.5:1"), "{text}");
        for index in 0..warnings.len() {
            let selector: &'static str = format!("theme-warning-{index}").leak();
            assert!(
                cx.debug_bounds(selector).is_some(),
                "warning {index} is listed"
            );
        }
        assert!(cx.debug_bounds("theme-editor-save").is_some());
    }

    #[gpui::test]
    fn a_burst_of_edits_applies_once_per_frame_without_a_worker_job(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        let idle = submissions(cx, &app);
        let before = cx.read(|cx| app.read(cx).theme_editor.preview_applications);
        let inputs = cx.read(|cx| {
            [
                TokenKind::Canvas,
                TokenKind::Panel,
                TokenKind::Accent,
                TokenKind::Added,
                TokenKind::Hunk,
            ]
            .map(|kind| form.read(cx).row(kind).hex.clone())
        });
        cx.update(|window, cx| {
            for (index, input) in inputs.iter().enumerate() {
                let text = format!("#10{index:02}20");
                // `replace_all` emits Change as typing does; five edits, one frame.
                input.update(cx, |input, cx| input.replace_all(text, window, cx));
            }
        });
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| app.read(cx).theme_editor.preview_applications),
            before + 1,
            "the first edit of a burst applies in its handler"
        );
        assert_eq!(
            applied(cx).canvas,
            0x100020,
            "the keystroke's own render already shows the first edit"
        );
        next_frame(cx);
        let draft = cx.read(|cx| form.read(cx).draft);
        assert_eq!(draft.hunk, 0x100420);
        assert_eq!(
            cx.read(|cx| app.read(cx).theme_editor.preview_applications),
            before + 2,
            "the other four edits apply once, at the frame"
        );
        assert_eq!(applied(cx), draft);
        next_frame(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).theme_editor.preview_applications),
            before + 2,
            "an idle frame applies nothing"
        );
        // A single edit applies once and leaves nothing for the frame.
        cx.update(|window, cx| {
            inputs[0].update(cx, |input, cx| input.replace_all("#101020", window, cx))
        });
        cx.run_until_parked();
        assert_eq!(applied(cx).canvas, 0x101020);
        next_frame(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).theme_editor.preview_applications),
            before + 3,
            "one edit is one application"
        );
        assert_eq!(
            submissions(cx, &app),
            idle,
            "the preview submitted a worker job"
        );
    }

    /// Focus starts in Name, Tab moves Name → Base → each token's hex field and
    /// picker in group order → footer, and Shift-Tab from Name stays in the dialog.
    #[gpui::test]
    fn keyboard_order_is_name_base_rows_then_footer(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        let focused_in = |cx: &mut VisualTestContext, input: &Entity<InputState>| {
            cx.update(|window, cx| input.read(cx).focus_handle(cx).is_focused(window))
        };
        let name = cx.read(|cx| form.read(cx).name.clone());
        assert!(focused_in(cx, &name), "the editor opens with Name focused");
        cx.simulate_keystrokes("tab");
        settle(cx);
        assert!(!focused_in(cx, &name), "Base follows Name");
        let hexes = cx.read(|cx| {
            form.read(cx)
                .rows
                .iter()
                .map(|row| row.hex.clone())
                .collect::<Vec<_>>()
        });
        cx.simulate_keystrokes("tab");
        settle(cx);
        assert!(
            focused_in(cx, &hexes[0]),
            "the first token row follows Base"
        );
        for (index, hex) in hexes.iter().enumerate().skip(1) {
            // The row's color picker sits between one hex field and the next.
            cx.simulate_keystrokes("tab tab");
            settle(cx);
            assert!(
                focused_in(cx, hex),
                "{} follows the previous row",
                TokenKind::ALL[index].label()
            );
        }
        // The Readability list is a tab stop after the last row's picker.
        cx.simulate_keystrokes("tab tab");
        settle(cx);
        assert!(
            cx.update(|window, cx| form.read(cx).warnings_focus.is_focused(window)),
            "the Readability list follows the last row"
        );
        cx.update(|window, cx| name.read(cx).focus_handle(cx).focus(window, cx));
        cx.simulate_keystrokes("shift-tab");
        settle(cx);
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            let focused = window.focused(cx).expect("focus stays in the dialog");
            assert!(!name.read(cx).focus_handle(cx).is_focused(window));
            assert!(
                hexes
                    .iter()
                    .all(|hex| hex.read(cx).focus_handle(cx) != focused),
                "Shift-Tab from Name wraps to the footer, not a token row"
            );
        });
    }

    /// Return in a valid hex field rewrites it in canonical form and saves
    /// nothing; Return on the focused Cancel button cancels.
    #[gpui::test]
    fn return_commits_a_hex_field_and_activates_the_focused_button(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let before = applied(cx);
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        type_hex(cx, &form, TokenKind::Accent, "12AB34");
        next_frame(cx);
        cx.simulate_keystrokes("enter");
        settle(cx);
        assert!(
            cx.read(|cx| app.read(cx).theme_editor.form.is_some()),
            "Return in a hex field keeps the editor open"
        );
        assert!(!cx.read(|cx| app.read(cx).theme_editor.save_pending()));
        assert_eq!(
            cx.read(|cx| form.read(cx).row(TokenKind::Accent).hex.read(cx).value()),
            "#12ab34",
            "Return rewrites the field as lowercase #rrggbb"
        );
        assert_eq!(cx.read(|cx| form.read(cx).draft.accent), 0x12ab34);
        assert!(Preferences::load().custom_themes.is_empty());

        // Shift-Tab from Name wraps to Save, then Cancel.
        let name = cx.read(|cx| form.read(cx).name.clone());
        cx.update(|window, cx| name.read(cx).focus_handle(cx).focus(window, cx));
        cx.simulate_keystrokes("shift-tab shift-tab");
        settle(cx);
        assert!(
            cx.update(|window, cx| form.read(cx).cancel_focus.contains_focused(window, cx)),
            "Shift-Tab twice from Name reaches Cancel"
        );
        press(cx, "enter");
        assert!(
            cx.read(|cx| app.read(cx).theme_editor.form.is_none()),
            "Return on Cancel closes the editor"
        );
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        wait_for(cx, |cx| {
            cx.read(|cx| !app.read(cx).theme_editor.save_pending())
        });
        assert_eq!(applied(cx), before, "Cancel re-applied the saved selection");
        assert!(
            Preferences::load().custom_themes.is_empty(),
            "Return on Cancel saved the draft"
        );
    }

    /// Keep colors and Replace colors leave the tree when answered; focus
    /// returns to Base so Tab and Escape keep working.
    #[gpui::test]
    fn answering_the_base_question_returns_focus_to_base(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        type_hex(cx, &form, TokenKind::Accent, "#12ab34");
        next_frame(cx);
        let current = cx.read(|cx| form.read(cx).base);
        let other = ThemeChoice::ALL
            .into_iter()
            .find(|choice| *choice != current)
            .expect("another base exists");
        cx.update(|window, cx| form.update(cx, |form, cx| form.choose_base(other, window, cx)));
        settle(cx);
        assert_eq!(cx.read(|cx| form.read(cx).pending_base), Some(other));

        let name = cx.read(|cx| form.read(cx).name.clone());
        cx.update(|window, cx| name.read(cx).focus_handle(cx).focus(window, cx));
        cx.simulate_keystrokes("tab tab");
        settle(cx);
        assert!(
            cx.update(|window, cx| form.read(cx).keep_focus.contains_focused(window, cx)),
            "Keep colors follows Base while the question is shown"
        );
        press(cx, "space");
        cx.read(|cx| {
            let form = form.read(cx);
            assert_eq!(form.pending_base, None);
            assert_eq!(form.base, other);
            assert_eq!(form.draft.accent, 0x12ab34, "Keep colors kept the draft");
        });
        assert!(
            cx.update(|window, cx| window.focused(cx).is_some()),
            "focus is lost after answering the question"
        );
        let first_hex = cx.read(|cx| form.read(cx).rows[0].hex.clone());
        cx.simulate_keystrokes("tab");
        settle(cx);
        assert!(
            cx.update(|window, cx| first_hex.read(cx).focus_handle(cx).is_focused(window)),
            "focus returned to Base, so Tab reaches the first row"
        );
        cx.simulate_keystrokes("escape");
        settle(cx);
        assert!(
            cx.read(|cx| app.read(cx).theme_editor.form.is_none()),
            "Escape cancels after the base question"
        );
    }

    fn entry_visible(scroll: &ScrollHandle, index: usize) -> bool {
        let container = scroll.bounds();
        let offset = scroll.offset();
        scroll.bounds_for_item(index).is_some_and(|item| {
            item.top() + offset.y >= container.top() - px(0.5)
                && item.bottom() + offset.y <= container.bottom() + px(0.5)
        })
    }

    /// Tab onto each row's hex field and picker, then onto the Readability
    /// list, brings the focused entry into view in the one frame that follows
    /// the key-down, in the stacked layout at 1,000 × 680 and in the wide
    /// layout. Nothing else draws in between: no key-up, caret blink, pointer
    /// or second frame.
    #[gpui::test]
    fn tab_brings_each_row_and_the_readability_list_into_view_in_one_frame(
        cx: &mut TestAppContext,
    ) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        settle(cx);
        cx.simulate_resize(size(px(1000.), px(680.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_theme_editor(None, window, cx)));
        settle(cx);
        let form = form(cx, &app);
        next_frame(cx);
        assert!(!cx.read(|cx| form.read(cx).wide));
        let last = TokenKind::ALL[TokenKind::ALL.len() - 1];
        let last_entry = ThemeForm::row_entry(last);
        assert_eq!(last_entry, ThemeForm::TOKEN_ENTRIES - 1);
        let scroll = cx.read(|cx| form.read(cx).scroll.clone());
        let side_scroll = cx.read(|cx| form.read(cx).side_scroll.clone());
        assert!(
            !entry_visible(&scroll, last_entry),
            "the last row starts below the fold at 1,000 × 680"
        );

        // Name -> Base, then every row's hex field and picker.
        key_frame(cx, "tab", |_, _| ());
        for kind in TokenKind::ALL {
            for control in ["hex field", "picker"] {
                let (visible, _) = key_frames(
                    cx,
                    "tab",
                    |_, _| entry_visible(&scroll, ThemeForm::row_entry(kind)),
                    |_, _| (),
                );
                assert_eq!(
                    cx.read(|cx| form.read(cx).revealed),
                    Some(Reveal::Row(kind))
                );
                assert!(
                    visible,
                    "{} {control} is in view in the frame after Tab",
                    kind.label()
                );
            }
        }
        let warnings = cx.read(|cx| form.read(cx).warnings_focus.clone());
        let list = ThemeForm::TOKEN_ENTRIES + 1;
        assert!(
            !entry_visible(&scroll, list),
            "the Readability list is below the fold after the last row"
        );
        let (visible, _) = key_frames(
            cx,
            "tab",
            |window, _| warnings.is_focused(window) && entry_visible(&scroll, list),
            |_, _| (),
        );
        assert!(
            visible,
            "the focused Readability list is in view in the frame after Tab"
        );

        // Back to the first row, then Tab through the wide layout's side column.
        cx.update(|window, cx| {
            let input = form.read(cx).row(TokenKind::ALL[0]).hex.clone();
            input.read(cx).focus_handle(cx).focus(window, cx);
        });
        native_frame(cx);
        assert!(entry_visible(
            &scroll,
            ThemeForm::row_entry(TokenKind::ALL[0])
        ));
        cx.simulate_resize(size(px(1440.), px(900.)));
        settle(cx);
        assert!(cx.read(|cx| form.read(cx).wide));
        cx.update(|window, cx| {
            let picker = form.read(cx).row(last).picker.clone();
            picker.focus_handle(cx).focus(window, cx);
        });
        native_frame(cx);
        assert!(
            entry_visible(&scroll, last_entry),
            "the focused row is in view in the wide layout"
        );
        let (visible, _) = key_frames(
            cx,
            "tab",
            |window, _| warnings.is_focused(window) && entry_visible(&side_scroll, 1),
            |_, _| (),
        );
        assert!(
            visible,
            "the focused Readability list is in view in the side column"
        );
    }

    /// Rows and group labels keep their height inside the height-capped scroll
    /// column, Name matches the Base button, the hex field is seven characters
    /// wide, and the alert uses the window height down to its bottom margin so
    /// the footer stays on screen, stacked and wide.
    #[gpui::test]
    fn rows_keep_their_geometry_and_the_dialog_fills_the_window(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        for (viewport, wide) in [
            (size(px(1000.), px(680.)), false),
            (size(px(1440.), px(900.)), true),
        ] {
            cx.simulate_resize(viewport);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.open_theme_editor(None, window, cx))
            });
            settle(cx);
            let form = form(cx, &app);
            next_frame(cx);
            assert_eq!(cx.read(|cx| form.read(cx).wide), wide);
            // The alert slides in over real time; measure once it has settled.
            let mut last = bounds(cx, "theme-editor-save".into());
            for _ in 0..100 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                draw_once(cx);
                let now = bounds(cx, "theme-editor-save".into());
                if now == last {
                    break;
                }
                last = now;
            }
            let row = appearance::ui_size(30.);
            for (index, group) in TokenGroup::ALL.into_iter().enumerate() {
                let label = bounds(cx, format!("theme-group-{index}"));
                assert_eq!(
                    label.size.height,
                    appearance::ui_size(22.),
                    "{} label at {viewport:?}",
                    group.label()
                );
                let mut previous: Option<Bounds<Pixels>> = None;
                for kind in TokenKind::ALL.iter().filter(|kind| kind.group() == group) {
                    let bounds = bounds(cx, format!("theme-token-{}", kind.key()));
                    assert_eq!(
                        bounds.size.height,
                        row,
                        "{} row at {viewport:?}",
                        kind.label()
                    );
                    let above = previous.map_or(label.bottom(), |previous| previous.bottom());
                    assert_eq!(
                        bounds.top(),
                        above,
                        "{} follows its predecessor directly at {viewport:?}",
                        kind.label()
                    );
                    previous = Some(bounds);
                }
            }
            let name = bounds(cx, "theme-editor-name".into());
            let base = bounds(cx, "theme-editor-base".into());
            assert_eq!(base.size.height, appearance::ui_size(28.));
            assert_eq!(
                name.size.height, base.size.height,
                "Name is as tall as Base"
            );
            let hex = bounds(cx, format!("theme-hex-{}", TokenKind::ALL[0].key()));
            assert_eq!(hex.size.width, ThemeForm::hex_width());
            let fit = cx.update(|window, _| ThemeForm::hex_fit_width(window));
            assert!(
                hex.size.width >= fit,
                "seven characters and the caret fit the hex field: {:?} < {fit:?}",
                hex.size.width
            );

            // The token column overflows at both sizes and shows an
            // always-visible scrollbar in a reserved track beside the rows.
            let scroll = cx.read(|cx| form.read(cx).scroll.clone());
            assert!(
                scroll.max_offset().y > px(0.),
                "the token column scrolls at {viewport:?}"
            );
            let body = bounds(cx, "theme-editor-body".into());
            let track = bounds(cx, "theme-editor-scrollbar".into());
            assert_eq!(track.size.width, Scrollbar::width());
            assert_eq!(
                track.right(),
                body.right(),
                "the track is at the column's edge"
            );
            assert_eq!(track.top(), body.top());
            assert_eq!(track.size.height, body.size.height);
            let last = bounds(cx, format!("theme-token-{}", TokenKind::ALL[0].key()));
            assert!(
                last.right() <= track.left(),
                "rows end before the track: {:?} > {:?}",
                last.right(),
                track.left()
            );

            // The panel ends 16 px of padding under Save; 16 px stay under the panel.
            let save = bounds(cx, "theme-editor-save".into());
            let panel_bottom = save.bottom() + px(16.);
            let margin = appearance::ui_size(16.);
            assert!(
                panel_bottom <= viewport.height - margin + px(1.),
                "the footer is on screen: panel bottom {panel_bottom:?} in {viewport:?}"
            );
            assert!(
                panel_bottom >= viewport.height - margin - px(1.),
                "the body uses the window height: panel bottom {panel_bottom:?} in {viewport:?}"
            );
            let editor = bounds(cx, "theme-editor".into());
            assert_eq!(
                editor.size.height,
                cx.update(|window, _| ThemeForm::max_height(window)),
                "the form is capped at {viewport:?}"
            );

            cx.simulate_keystrokes("escape");
            settle(cx);
            assert!(cx.read(|cx| app.read(cx).theme_editor.form.is_none()));
        }
    }

    /// Harbor (id 7, based on Nord) is saved and active.
    fn with_active_harbor(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> CustomTheme {
        let theme = CustomTheme::from_base(7, "Harbor", ThemeChoice::Nord);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.custom_themes = vec![theme.clone()];
                app.settings.theme = ThemeSelection::Custom(7);
                app.apply_appearance(window, cx);
            })
        });
        settle(cx);
        theme
    }

    /// Keyboard-only: from the card, Tab past New theme… and Edit… reaches
    /// Delete…, and Space opens the alert as a native input. The frame after
    /// the key draws the alert; the next frame runs its callbacks, then draws.
    fn open_delete_alert(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) {
        let card = cx.update(|_, cx| app.read(cx).theme_editor.card_focus(cx));
        cx.update(|window, cx| {
            card.focus(window, cx);
            window.focus_next(cx);
        });
        cx.simulate_keystrokes("tab tab");
        settle(cx);
        assert!(
            cx.update(|window, cx| card.contains_focused(window, cx)),
            "Delete… is the third tab stop in the card"
        );
        assert!(key_frame(cx, "space", |window, cx| window.has_active_dialog(cx)));
        native_frame(cx);
        assert!(cx.debug_bounds("delete-theme-cancel").is_some());
        assert!(cx.debug_bounds("delete-theme-confirm").is_some());
    }

    /// Delete… opens its alert with keyboard focus on Cancel in GPUI's native
    /// frame order, where a frame's callbacks run before that frame is drawn:
    /// Escape and Return on Cancel close it and keep the theme, Tab reaches
    /// Delete theme, and Return there deletes and selects the base.
    #[gpui::test]
    fn delete_confirmation_opens_on_cancel_in_the_native_frame_order(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        let theme = with_active_harbor(cx, &app);
        let kept = |cx: &mut VisualTestContext| {
            !cx.update(|window, cx| window.has_active_dialog(cx))
                && cx.read(|cx| {
                    let app = app.read(cx);
                    app.custom_themes == vec![theme.clone()] && !app.theme_editor.save_pending()
                })
        };

        open_delete_alert(cx, &app);
        key_frame(cx, "escape", |_, _| ());
        assert!(kept(cx), "Escape closes the alert and keeps the theme");

        open_delete_alert(cx, &app);
        key_frame(cx, "enter", |_, _| ());
        assert!(
            kept(cx),
            "Return on the focused Cancel closes the alert and keeps the theme"
        );

        open_delete_alert(cx, &app);
        assert!(key_frame(cx, "tab", |window, cx| window.has_active_dialog(cx)));
        assert!(
            !key_frame(cx, "enter", |window, cx| window.has_active_dialog(cx)),
            "Return on Delete theme closes the alert"
        );
        wait_for(cx, |cx| {
            cx.read(|cx| {
                let app = app.read(cx);
                app.custom_themes.is_empty() && !app.theme_editor.save_pending()
            })
        });
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.theme),
            ThemeSelection::BuiltIn(ThemeChoice::Nord),
            "deleting the active theme selects its base"
        );
        assert_eq!(applied(cx), ThemeChoice::Nord.palette());
        assert!(cx.debug_bounds("delete-custom-theme-7").is_none());
    }

    /// Delete the active Harbor with the keyboard, the store answering before
    /// the next frame or only after three native frames have drawn the row's
    /// actions disabled and run the modal focus repair. Either way Space then
    /// opens New theme…, also after the repair's delayed recheck.
    fn delete_then_space_opens_new_theme(cx: &mut TestAppContext, late_reply: bool) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        with_active_harbor(cx, &app);
        // A job queued ahead of the delete holds the serialized writer.
        let release = late_reply.then(|| {
            let (release, hold) = std::sync::mpsc::channel::<()>();
            let _reply = cx.read(|cx| {
                app.read(cx).preferences_writer.submit(move || {
                    let _ = hold.recv();
                    anyhow::Ok(())
                })
            });
            release
        });
        open_delete_alert(cx, &app);
        key_frame(cx, "tab", |_, _| ());
        assert!(!key_frame(cx, "enter", |window, cx| window.has_active_dialog(cx)));
        if let Some(release) = release {
            for _ in 0..3 {
                native_frame(cx);
            }
            assert!(
                cx.read(|cx| app.read(cx).theme_editor.save_pending()),
                "the reply is still held"
            );
            release.send(()).expect("the writer holds the job");
        }
        wait_without_drawing(cx, |cx| {
            cx.read(|cx| {
                let app = app.read(cx);
                app.custom_themes.is_empty() && !app.theme_editor.save_pending()
            })
        });
        // The first frame draws the card without the row; the next moves focus.
        native_frame(cx);
        native_frame(cx);
        // The modal focus repair rechecks 300 ms after the alert closed.
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        native_frame(cx);
        key_frame(cx, "space", |_, _| ());
        assert!(
            cx.read(|cx| app.read(cx).theme_editor.form.is_some()),
            "Space on the focused New theme… opens the editor"
        );
    }

    #[gpui::test]
    fn after_a_delete_answered_before_the_next_frame_focus_is_on_new_theme(
        cx: &mut TestAppContext,
    ) {
        delete_then_space_opens_new_theme(cx, false);
    }

    #[gpui::test]
    fn after_a_delete_answered_after_the_modal_focus_repair_focus_is_on_new_theme(
        cx: &mut TestAppContext,
    ) {
        delete_then_space_opens_new_theme(cx, true);
    }
}
