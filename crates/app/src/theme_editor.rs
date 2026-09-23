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
//! preference executor. Export and import use the native save and open
//! dialogs and do every file operation on that same executor: an export writes
//! the specification's document through temp-and-rename, an import reads it
//! with the bounded store reader and parses it there, and only the name, the
//! bound and the save are decided on the UI thread. An imported theme is added
//! and highlighted, never applied. The contract is
//! `docs/development/themes/spec.md` §Editor, §Import and export and §Failure
//! and recovery, and `DESIGN.md` §Custom theme editor.

use crate::*;
use appearance::custom::{
    self, CustomTheme, ImportedTheme, ReadabilityBackground, ReadabilityForeground,
    ReadabilityIssue, ThemeSelection, TokenGroup, TokenKind,
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
    /// A failed delete, export or import, shown in the Your themes card.
    error: Option<String>,
    /// A finished export or import, shown in the Your themes card.
    notice: Option<String>,
    /// A native theme dialog or a theme file job is in flight. The card's
    /// actions wait for it, so one transfer has one visible owner.
    transfer_pending: bool,
    /// The theme the last import added. The card highlights that row; the
    /// window's own theme is untouched.
    imported: Option<u32>,
    /// The Your themes card's warning counts, by theme id and palette, so a
    /// Settings frame does not rerun the readability rules for every theme.
    warning_counts: std::cell::RefCell<Vec<(u32, Palette, usize)>>,
    /// The Your themes rows' viewport (`settings.rs`): `min(rows, 8)` rows
    /// tall and virtualized with `uniform_list`, so a Settings frame lays out
    /// at most eight rows however many themes are saved.
    rows_scroll: UniformListScrollHandle,
    /// One focus handle per saved theme, tracked on its Your themes row and
    /// never a tab stop itself. The rows are virtualized, so a row's Edit…,
    /// Export… and Delete… (the kit's own tab stops, in that order) exist
    /// only while the row is drawn; the row's handle lets a render find the
    /// row holding focus and scroll it into view (`focused_row`), and lets
    /// Tab reach a row's actions once the row is drawn (`focus_row_action`).
    row_focus: std::cell::RefCell<Vec<(u32, FocusHandle)>>,
    /// A row action (0 Edit…, 1 Export…, 2 Delete…) to focus once its row is
    /// drawn: Tab across the viewport boundary scrolls the row in and the
    /// list's next render focuses it (`GitTurtle::tab_theme_rows`,
    /// `take_row_focus_request`).
    row_focus_request: std::cell::Cell<Option<(usize, usize)>>,
    /// The row last scrolled into view because one of its actions held focus,
    /// so a wheel scroll away from a focused row is not undone by the next
    /// render.
    revealed_row: std::cell::Cell<Option<usize>>,
    /// The first live-preview edit, or the open, since the last traced frame,
    /// stamped at its handler's entry. The root render takes it into the
    /// probe that ends `gitturtle.theme_edit_frame_ms`
    /// (`GitTurtle::edit_trace_probe`); later edits before that frame join
    /// the same burst.
    edit_started: Option<Instant>,
    /// Test-only: every probe paint, with the draw it belonged to.
    #[cfg(test)]
    edit_traces: Vec<EditTrace>,
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

    pub(super) fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// An export or import is waiting for its dialog, the file or the store.
    pub(super) fn transfer_pending(&self) -> bool {
        self.transfer_pending
    }

    /// The row the last import added, highlighted until the next card action.
    pub(super) fn imported(&self) -> Option<u32> {
        self.imported
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

    /// Take the pending edit stamp for the frame being drawn.
    pub(super) fn take_edit_trace(&mut self) -> Option<Instant> {
        self.edit_started.take()
    }

    /// The Your themes rows' scroll handle.
    pub(super) fn rows_scroll(&self) -> &UniformListScrollHandle {
        &self.rows_scroll
    }

    /// Theme `id`'s row handle, created on first use and kept while the
    /// theme is saved.
    pub(super) fn row_focus(&self, id: u32, cx: &App) -> FocusHandle {
        let mut rows = self.row_focus.borrow_mut();
        if let Some((_, handle)) = rows.iter().find(|(row, _)| *row == id) {
            return handle.clone();
        }
        let handle = cx.focus_handle();
        rows.push((id, handle.clone()));
        handle
    }

    /// Drop the handles of themes no longer saved.
    pub(super) fn retain_row_focus(&self, themes: &[CustomTheme]) {
        self.row_focus
            .borrow_mut()
            .retain(|(id, _)| themes.iter().any(|theme| theme.id == *id));
    }

    /// The drawn row holding focus, itself or through one of its actions.
    pub(super) fn focused_row(
        &self,
        themes: &[CustomTheme],
        window: &Window,
        cx: &App,
    ) -> Option<usize> {
        let rows = self.row_focus.borrow();
        themes.iter().position(|theme| {
            rows.iter()
                .any(|(id, handle)| *id == theme.id && handle.contains_focused(window, cx))
        })
    }

    /// Ask the list's next render to focus `action` of `row` once drawn.
    pub(super) fn request_row_focus(&self, row: usize, action: usize) {
        self.row_focus_request.set(Some((row, action)));
    }

    /// The request whose row this render draws, if any; a request for a row
    /// no longer saved is dropped.
    pub(super) fn take_row_focus_request(
        &self,
        drawn: &std::ops::Range<usize>,
        count: usize,
    ) -> Option<(usize, usize)> {
        let (row, action) = self.row_focus_request.get()?;
        if row >= count {
            self.row_focus_request.set(None);
            return None;
        }
        drawn.contains(&row).then(|| {
            self.row_focus_request.set(None);
            (row, action)
        })
    }

    /// Focus `action` of the row drawn with `handle`: the row's handle is in
    /// the tab order ahead of its actions and is not a stop itself, so the
    /// first stop after it is Edit…, then Export…, then Delete….
    pub(super) fn focus_row_action(
        handle: &FocusHandle,
        action: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        handle.focus(window, cx);
        for _ in 0..=action {
            window.focus_next(cx);
        }
    }

    /// Test-only: the drawn row and action (0 Edit…, 1 Export…, 2 Delete…)
    /// holding focus. The actions' handles are the kit's, so the action is
    /// found by stepping from the row's handle until the focused one is
    /// reached; focus ends where it started.
    #[cfg(test)]
    pub(super) fn focused_row_action(
        &self,
        themes: &[CustomTheme],
        window: &mut Window,
        cx: &mut App,
    ) -> Option<(usize, usize)> {
        let focused = window.focused(cx)?;
        let row = self.focused_row(themes, window, cx)?;
        let handle = self.row_focus(themes[row].id, cx);
        handle.focus(window, cx);
        let mut action = None;
        for step in 0..3 {
            window.focus_next(cx);
            if window.focused(cx) == Some(focused.clone()) {
                action = Some(step);
                break;
            }
        }
        focused.focus(window, cx);
        Some((row, action?))
    }

    /// Scroll the row holding focus into view once per focus change
    /// (`ScrollStrategy::Nearest`); a wheel scroll away from it afterwards is
    /// left alone.
    pub(super) fn reveal_focused_row(&self, focused: Option<usize>) {
        if self.revealed_row.get() == focused {
            return;
        }
        self.revealed_row.set(focused);
        if let Some(row) = focused {
            self.rows_scroll
                .scroll_to_item(row, ScrollStrategy::Nearest);
        }
    }
}

/// Test-only: one paint of the edit-frame trace probe.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(super) struct EditTrace {
    /// The handler's entry.
    pub(super) started: Instant,
    /// The probe's paint: the end of `gitturtle.theme_edit_frame_ms`.
    pub(super) ended: Instant,
    /// The root draw that painted it (`GitTurtle::draws.len()` at the time).
    pub(super) draw: usize,
}

gpui::actions!(theme_editor, [NextThemeAction, PreviousThemeAction]);

/// The Settings page's key context, deeper than the toolkit root's, so the
/// bindings of [`init`] are tried first and fall through when they do not
/// apply.
pub(super) const SETTINGS_CONTEXT: &str = "GitTurtleSettings";

/// Tab and Shift-Tab inside Settings walk the Your themes rows through
/// [`GitTurtle::tab_theme_rows`] before the toolkit's own tab order, which
/// knows only the rows drawn.
pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", NextThemeAction, Some(SETTINGS_CONTEXT)),
        KeyBinding::new("shift-tab", PreviousThemeAction, Some(SETTINGS_CONTEXT)),
    ]);
}

/// `Instant::now()` when the edit-frame trace runs: under `GITTURTLE_TRACE`,
/// and in tests, where the probe records instead of printing.
fn trace_stamp() -> Option<Instant> {
    (cfg!(test) || trace_enabled()).then(Instant::now)
}

/// Above every dialog (`10 + layer`), popup (100) and tooltip (200) the
/// toolkit defers, so the probe sorts last among a frame's deferred draws.
const EDIT_TRACE_PRIORITY: usize = usize::MAX;

/// What a finished custom-theme save means for the window.
enum SaveOutcome {
    Edited {
        id: u32,
        form: WeakEntity<ThemeForm>,
    },
    /// A theme read from a document, which is added and highlighted but never
    /// applied: importing a theme does not change the window's appearance.
    Imported { id: u32, notice: String },
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

/// The written path for an export notice. A destination can be arbitrarily
/// deep, and the card's message is one line of prose, so this clamps the path
/// from the front: the file name the user just chose is what they are looking
/// for, and the leading ellipsis says the folders above it were dropped.
fn written_path_text(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    let characters = text.chars().count();
    if characters <= custom::MAX_MESSAGE_FRAGMENT_CHARS {
        return text;
    }
    let start = text
        .char_indices()
        .nth(characters - custom::MAX_MESSAGE_FRAGMENT_CHARS)
        .map_or(0, |(index, _)| index);
    format!("…{}", &text[start..])
}

/// Read a chosen theme document with the preference store's bounded reader: a
/// regular file, no symbolic link as the final component, at most 64 KiB, and
/// one descriptor for the check and the read. Its refusals are translated here
/// so the card names the problem in the terms the user chose the file in.
fn read_theme_document(path: &std::path::Path) -> Result<Vec<u8>, String> {
    const LIMIT_KIB: usize = custom::MAX_THEME_DOCUMENT_BYTES / 1024;
    preferences::read_store(path, custom::MAX_THEME_DOCUMENT_BYTES as u64).map_err(|error| {
        // Only for the message: the bytes never came through this second look.
        let metadata = std::fs::symlink_metadata(path).ok();
        let kind = metadata.as_ref().map(|metadata| metadata.file_type());
        let oversized = metadata
            .as_ref()
            .is_some_and(|metadata| metadata.len() > custom::MAX_THEME_DOCUMENT_BYTES as u64);
        match kind {
            Some(kind) if kind.is_symlink() => {
                "A theme file must be a regular file; this one is a symbolic link.".into()
            }
            Some(kind) if kind.is_dir() => {
                "A theme file must be a regular file; this one is a folder.".into()
            }
            Some(_) if oversized => {
                format!(
                    "This theme file is larger than {LIMIT_KIB} KiB, the most GitTurtle imports."
                )
            }
            Some(kind) if !kind.is_file() => "A theme file must be a regular file.".into(),
            _ => format!("Could not read the theme file: {error}."),
        }
    })
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
        // The open's trace starts here, before the form is built.
        let started = trace_stamp();
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
                    self.notify_settings_page(cx);
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
        self.clear_theme_transfer_result();
        self.preview_theme_draft(palette, started, window, cx);
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
    /// from the next frame callback. No worker job is involved.
    ///
    /// `started` is the edit's or the open's handler entry. The first of a
    /// burst is kept for the root render, whose probe ends
    /// `gitturtle.theme_edit_frame_ms` in the paint of the next draw
    /// ([`GitTurtle::edit_trace_probe`]). The switch metric's callback
    /// boundary is not used here: a callback runs only on the platform's frame
    /// tick and never sees the draw itself.
    fn preview_theme_draft(
        &mut self,
        draft: Palette,
        started: Option<Instant>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_editor.form.is_none() {
            return;
        }
        if let Some(started) = started {
            self.theme_editor.edit_started.get_or_insert(started);
        }
        self.theme_editor.preview = Some(draft);
        if self.theme_editor.preview_scheduled {
            self.theme_editor.preview_dirty = true;
            return;
        }
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
            });
        });
    }

    /// The end of `gitturtle.theme_edit_frame_ms`: a zero-size element the
    /// root render defers above the dialog layer (and every popup and
    /// tooltip), so its paint is the last thing `Window::draw` paints in the
    /// first frame drawn after the handler applied the draft, whether the
    /// platform's frame tick draws it or `Window::dispatch_key_event` draws
    /// the dirty window before the next key. The value therefore holds the
    /// parse, the readability recompute, `apply_appearance` and the whole
    /// window's request_layout, layout, prepaint and paint, and none of a
    /// callback wait, frame-finish bookkeeping, present or input delivery.
    pub(super) fn edit_trace_probe(&self, started: Instant, cx: &Context<Self>) -> AnyElement {
        #[cfg(test)]
        let owner = cx.entity().downgrade();
        #[cfg(not(test))]
        let _ = cx;
        gpui::deferred(
            canvas(
                |_, _, _| (),
                move |_, _, _window, _cx| {
                    let ended = Instant::now();
                    if trace_enabled() {
                        eprintln!(
                            "gitturtle.theme_edit_frame_ms={:.3}",
                            (ended - started).as_secs_f64() * 1000.
                        );
                    }
                    #[cfg(test)]
                    {
                        let _ = owner.update(_cx, |this, _| {
                            let draw = this.draws.len();
                            this.theme_editor.edit_traces.push(EditTrace {
                                started,
                                ended,
                                draw,
                            });
                        });
                    }
                },
            )
            .w(px(0.))
            .h(px(0.)),
        )
        .with_priority(EDIT_TRACE_PRIORITY)
        .into_any_element()
    }

    /// Tab (`forward`) and Shift-Tab in Settings, tried before the toolkit's
    /// tab order. Inside the Your themes rows they move to the next or
    /// previous action, row by row, scrolling that row into view, since a row
    /// not drawn has no tab stop of its own; from the last action of the last
    /// row and the first of the first they fall through to the page. From
    /// outside, the frame's own order moves focus, and a move that enters the
    /// rows is redirected to the first row's Edit… or the last row's Delete…,
    /// which the rows drawn at that moment need not include.
    pub(super) fn tab_theme_rows(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.custom_themes.len();
        // A dialog or menu owns Tab while it is open.
        if count == 0 || gpui_kit::base::active_focus_trap(window, cx).is_some() {
            cx.propagate();
            return;
        }
        let focused = window.focused(cx);
        let from = self
            .theme_editor
            .focused_row(&self.custom_themes, window, cx);
        // GPUI's own step is right whenever the row it should reach is
        // drawn: within a row, onto the neighbouring drawn row, or out of the
        // rows at their end. It is corrected only where the row it should
        // reach is not drawn.
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
        let to = self
            .theme_editor
            .focused_row(&self.custom_themes, window, cx);
        let target = match (from, to) {
            (Some(from), Some(to))
                if from == to || (forward && to == from + 1) || (!forward && from == to + 1) =>
            {
                return;
            }
            (Some(from), _) if forward && from + 1 < count => (from + 1, 0),
            (Some(from), _) if !forward && from > 0 => (from - 1, 2),
            (Some(_), _) => return,
            (None, Some(to)) if forward && to != 0 => (0, 0),
            (None, Some(to)) if !forward && to + 1 != count => (count - 1, 2),
            (None, _) => return,
        };
        // Focus stays put for the frame that draws the target row; that
        // render focuses the action (`settings.rs`). Every branch that
        // reaches here moved focus, and `Window::focus` refreshed the window,
        // so the cached Settings page is built again without its own notice.
        if let Some(focused) = focused {
            focused.focus(window, cx);
        }
        self.theme_editor
            .rows_scroll()
            .scroll_to_item(target.0, ScrollStrategy::Nearest);
        self.theme_editor.request_row_focus(target.0, target.1);
        cx.notify();
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
        self.clear_theme_transfer_result();
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

    /// A finished export or import stops being current as soon as the user
    /// acts on the card again.
    fn clear_theme_transfer_result(&mut self) {
        self.theme_editor.error = None;
        self.theme_editor.notice = None;
        self.theme_editor.imported = None;
    }

    /// Whether the card's actions are available: one dialog, file job or store
    /// save at a time, so every outcome has a visible owner.
    fn theme_transfer_busy(&self) -> bool {
        self.theme_editor.transfer_pending || self.theme_editor.save_pending()
    }

    /// Export a saved theme to a JSON document the user places with the native
    /// save dialog. The document is built here, but the write runs on the
    /// preference executor through temp-and-rename, so the UI thread never
    /// touches the file system and a refused destination leaves nothing
    /// behind. Cancelling the dialog is quiet.
    pub(super) fn export_custom_theme(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_transfer_busy() {
            return;
        }
        let Some(theme) = self.custom_themes.iter().find(|theme| theme.id == id) else {
            return;
        };
        let name = theme.name.clone();
        let bytes = theme.to_document();
        let suggested = theme.suggested_file_name();
        let response = cx.prompt_for_new_path(&preferences::home_directory(), Some(&suggested));
        self.theme_editor.transfer_pending = true;
        self.clear_theme_transfer_result();
        cx.notify();
        self.notify_settings_page(cx);
        cx.spawn_in(window, async move |this, cx| {
            let outcome: Result<Option<String>, String> = async {
                let Some(destination) = folder_picker::new_path(response).await? else {
                    return Ok(None);
                };
                let document = destination.clone();
                let Ok(job) = this.update(cx, |this, _| {
                    this.preferences_writer
                        .submit(move || preferences::write_exported_document(&document, &bytes))
                }) else {
                    return Ok(None);
                };
                match job.await {
                    Ok(Ok(())) => Ok(Some(format!(
                        "Exported “{name}” to {}.",
                        written_path_text(&destination)
                    ))),
                    Ok(Err(error)) => Err(format!("Could not export “{name}”: {error:#}")),
                    Err(_) => Err(format!(
                        "Could not export “{name}”: the settings writer stopped before reporting a result."
                    )),
                }
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.theme_editor.transfer_pending = false;
                match outcome {
                    Ok(Some(notice)) => this.theme_editor.notice = Some(notice),
                    Ok(None) => {}
                    Err(error) => this.theme_editor.error = Some(error),
                }
                cx.notify();
                this.notify_settings_page(cx);
            });
        })
        .detach();
    }

    /// Import a theme document the user chooses with the native open dialog.
    /// The bounded read, the parse and the document's own validation run on the
    /// preference executor; the UI thread only resolves the name against the
    /// saved themes, checks the bound and submits the save. Cancelling the
    /// dialog is quiet, and a refused document changes nothing.
    pub(super) fn import_custom_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_transfer_busy() {
            return;
        }
        let response = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import theme".into()),
        });
        self.theme_editor.transfer_pending = true;
        self.clear_theme_transfer_result();
        cx.notify();
        self.notify_settings_page(cx);
        cx.spawn_in(window, async move |this, cx| {
            let outcome: Result<Option<ImportedTheme>, String> = async {
                let chosen =
                    folder_picker::selected_path_for(folder_picker::Picker::File, response).await?;
                let Some(path) = chosen else {
                    return Ok(None);
                };
                let Ok(job) = this.update(cx, |this, _| {
                    this.preferences_writer.submit(move || {
                        Ok(read_theme_document(&path)
                            .and_then(|bytes| CustomTheme::from_document(&bytes)))
                    })
                }) else {
                    return Ok(None);
                };
                match job.await {
                    Ok(Ok(Ok(imported))) => Ok(Some(imported)),
                    // The document's own refusal already names the problem.
                    Ok(Ok(Err(refusal))) => Err(refusal),
                    Ok(Err(error)) => Err(format!("Could not read the theme file: {error:#}")),
                    Err(_) => Err(
                        "Could not read the theme file: the settings writer stopped before reporting a result."
                            .into(),
                    ),
                }
            }
            .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.theme_editor.transfer_pending = false;
                match outcome {
                    Ok(Some(imported)) => this.add_imported_theme(imported, window, cx),
                    Ok(None) => {}
                    Err(error) => this.theme_editor.error = Some(error),
                }
                cx.notify();
                this.notify_settings_page(cx);
            });
        })
        .detach();
    }

    /// Give a parsed document a free name and an id and save it. A collision
    /// with a saved theme or a built-in label becomes "<name> (imported)", the
    /// 32-theme bound refuses the import with the bound, and the saved theme is
    /// highlighted rather than applied.
    fn add_imported_theme(
        &mut self,
        imported: ImportedTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.custom_themes.len() >= MAX_CUSTOM_THEMES {
            self.theme_editor.error = Some(format!(
                "Up to {MAX_CUSTOM_THEMES} custom themes can be saved. Delete one before importing another."
            ));
            return;
        }
        let name = match custom::import_name(&imported.name, &self.custom_themes) {
            Ok(name) => name,
            Err(error) => {
                self.theme_editor.error = Some(error);
                return;
            }
        };
        let Some(id) = custom::next_custom_theme_id(&self.custom_themes) else {
            self.theme_editor.error =
                Some("No theme id is available. Delete a theme and import again.".into());
            return;
        };
        // The notice quotes up to three document strings — both names and an
        // unknown base — each clamped like a document fragment and sharing
        // the card's two-line budget, so at 128 bytes a name the base
        // substitution after it is still told. The row above carries the
        // imported theme's full name and its resolved base.
        let notice = custom::import_notice(
            &name,
            &imported.name,
            imported.unknown_base.as_deref(),
            imported.base,
        );
        let mut themes = self.custom_themes.clone();
        themes.push(CustomTheme {
            name,
            ..imported.into_theme(id)
        });
        self.submit_custom_themes(themes, SaveOutcome::Imported { id, notice }, window, cx);
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
        self.notify_settings_page(cx);
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
                self.set_custom_themes(themes, cx);
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
            (Ok(themes), SaveOutcome::Imported { id, notice }) => {
                // Added and highlighted, not applied: the window keeps its theme.
                self.set_custom_themes(themes, cx);
                self.theme_editor.error = None;
                self.theme_editor.notice = Some(notice);
                self.theme_editor.imported = Some(id);
            }
            (Err(error), SaveOutcome::Imported { .. }) => {
                self.theme_editor.error =
                    Some(format!("Could not save the imported theme: {error:#}"));
            }
            (Ok(themes), SaveOutcome::Deleted { id, base, previous }) => {
                self.set_custom_themes(themes, cx);
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
        self.notify_settings_page(cx);
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
    /// The row as its own view, so a dialog frame rebuilds only the rows it
    /// alters ([`TokenRowView`]).
    view: Entity<TokenRowView>,
}

/// What a token row shows from its form: [`ThemeForm::render`] builds a row
/// again exactly when this differs from what the row last showed.
#[derive(Clone, Copy, PartialEq)]
struct RowKey {
    color: u32,
    invalid: bool,
    flagged: bool,
    pending: bool,
    /// The applied palette: the row's text, borders and rings follow it.
    active: Palette,
    rem: Pixels,
    picker_focused: bool,
}

/// One token row of the editor, a view the form embeds with
/// [`Entity::cached`], so a frame that alters one row, a keystroke or a caret
/// blink in its hex field, replays the other rows instead of building them.
///
/// The form owns the row and its state ([`TokenRow`]); the row renders
/// `ThemeForm::render_row` through a weak handle and keeps no state of its
/// own. It is built again in three ways, enumerated in
/// `crates/app/docs/content-and-layout.md`:
/// - the form's render compares each row's [`RowKey`] with the last one the
///   row showed and embeds a changed row uncached for that frame, so every
///   form-side change (draft color, validity, readability flag, a pending
///   save, a palette application, the text size, the picker's focus) builds
///   it without a notify site;
/// - the row observes its hex field and its color picker: the field's view
///   node is registered outside the row (`settings::ViewNodeAnchor`, as the
///   Settings page does), so the kit input's notify from paint on every frame
///   dirties the field and the form but not the row, and the typing, caret,
///   selection and popover changes the row must show reach it through these
///   observers;
/// - GPUI's own refresh (hover, press, focus, a window refresh) builds every
///   cached view.
///
/// The failure mode of a missed rebuild is a row that keeps showing a
/// previous frame: a hex field whose text or caret stands still, a swatch or
/// glyph that does not follow the draft.
pub(super) struct TokenRowView {
    form: WeakEntity<ThemeForm>,
    kind: TokenKind,
    /// The key the last build showed.
    seen: Option<RowKey>,
    /// Test-only: builds of this row.
    #[cfg(test)]
    renders: usize,
    _subscriptions: [Subscription; 2],
}

impl TokenRowView {
    fn new(
        form: WeakEntity<ThemeForm>,
        kind: TokenKind,
        hex: &Entity<InputState>,
        picker: &Entity<ColorPickerState>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            form,
            kind,
            seen: None,
            #[cfg(test)]
            renders: 0,
            _subscriptions: [
                cx.observe(hex, |_, _, cx| cx.notify()),
                cx.observe(picker, |_, _, cx| cx.notify()),
            ],
        }
    }
}

impl Render for TokenRowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let Some(form) = self.form.upgrade() else {
            return div().into_any_element();
        };
        let form = form.read(cx);
        let row = form.row(self.kind);
        let key = form.row_key(row, window, cx);
        self.seen = Some(key);
        form.render_row(row, key.flagged, key.active, window, cx)
    }
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
    /// The draft's miniature, kept beside the draft so a frame that changes
    /// only the name, the warnings or the focus reuses it
    /// (`settings::ThemePreviewBody`).
    preview_body: Entity<settings::ThemePreviewBody>,
    /// Test-only: when this form last painted, so a test can place the
    /// edit-frame probe's paint after the dialog layer's.
    #[cfg(test)]
    painted: Option<Instant>,
    /// Test-only: builds of this form, so a test can assert that a keystroke
    /// in the dialog rebuilds the dialog and reuses the Settings page behind it.
    #[cfg(test)]
    renders: usize,
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
        let form = cx.entity().downgrade();
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
            let view = cx.new(|cx| TokenRowView::new(form.clone(), kind, &hex, &picker, cx));
            rows.push(TokenRow {
                kind,
                hex,
                picker,
                invalid: false,
                view,
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
            preview_body: cx.new(|_| settings::ThemePreviewBody::new(palette)),
            #[cfg(test)]
            painted: None,
            #[cfg(test)]
            renders: 0,
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

    /// What `row` shows from this form in the current frame ([`RowKey`]).
    fn row_key(&self, row: &TokenRow, window: &Window, cx: &App) -> RowKey {
        RowKey {
            color: self.draft.get(row.kind),
            invalid: row.invalid,
            flagged: self
                .warnings
                .iter()
                .any(|issue| issue_tokens(issue).any(|kind| kind == row.kind)),
            pending: self.pending,
            active: palette(cx),
            rem: window.rem_size(),
            picker_focused: row.picker.focus_handle(cx).is_focused(window),
        }
    }

    /// `row` for the token column: its cached view when the row already shows
    /// the current frame's [`RowKey`], else built this frame. Assistive
    /// technology reads accessibility nodes only from prepainted elements, so
    /// while it is active every row is built, as the Settings page is.
    fn row_element(&self, row: &TokenRow, window: &Window, cx: &App) -> AnyElement {
        let key = self.row_key(row, window, cx);
        if !window.is_a11y_active() && row.view.read(cx).seen == Some(key) {
            row.view
                .clone()
                .cached(
                    StyleRefinement::default()
                        .w_full()
                        .h(appearance::ui_size(30.))
                        .flex_shrink_0(),
                )
                .into_any_element()
        } else {
            row.view.clone().into_any_element()
        }
    }

    fn can_save(&self) -> bool {
        !self.pending && self.name_error.is_none() && self.rows.iter().all(|row| !row.invalid)
    }

    /// Typing in a hex field: a valid value updates the draft; anything else
    /// marks the field and keeps the last valid draft.
    fn hex_changed(&mut self, kind: TokenKind, window: &mut Window, cx: &mut Context<Self>) {
        // `gitturtle.theme_edit_frame_ms` starts here, before the parse.
        let started = trace_stamp();
        let value = self.row(kind).hex.read(cx).value().to_string();
        match custom::parse_hex(value.trim()) {
            Some(color) => {
                self.row_mut(kind).invalid = false;
                if self.draft.get(kind) != color {
                    self.draft.set(kind, color);
                    self.sync_picker(kind, color, window, cx);
                    self.draft_changed(started, window, cx);
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
        let started = trace_stamp();
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
        self.draft_changed(started, window, cx);
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

    /// Recompute the warnings and hand the draft to the window's coalesced
    /// preview; `started` is the edit's handler entry for the frame trace.
    fn draft_changed(
        &mut self,
        started: Option<Instant>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.warnings = self.draft.readability_issues();
        let draft = self.draft;
        self.preview_body
            .update(cx, |body, cx| body.set_palette(draft, cx));
        let _ = self.owner.update(cx, |this, cx| {
            this.preview_theme_draft(draft, started, window, cx)
        });
    }

    /// Replace every token, as Reset to base and a base change do.
    fn set_palette(&mut self, palette: Palette, window: &mut Window, cx: &mut Context<Self>) {
        let started = trace_stamp();
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
        self.draft_changed(started, window, cx);
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
            // As wide as the cached view's slot it is laid out in.
            .w_full()
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
            .children(if row.invalid {
                Some(
                    invalid_glyph(p)
                        .id(("theme-token-invalid", kind as usize))
                        .debug_selector(move || format!("theme-token-invalid-{}", kind.key()))
                        .role(Role::Label)
                        .aria_label(format!("{label} value is not #rrggbb")),
                )
            } else if flagged {
                Some(
                    warning_glyph(p)
                        .id(("theme-token-warning", kind as usize))
                        .role(Role::Label)
                        .aria_label(format!("{label} has a readability warning")),
                )
            } else {
                None
            })
            .child(
                div()
                    // Without a glyph the swatch keeps the glyph's 14 px slot
                    // and the row gap before it as its own margin, so nothing
                    // moves when a glyph appears and the row lays out one node
                    // less.
                    .when(!row.invalid && !flagged, |swatch| {
                        swatch.ml(appearance::ui_size(14.) + window.rem_size() * 0.5)
                    })
                    .size(appearance::ui_size(16.))
                    .flex_shrink_0()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(color)),
            )
            .child(
                // The field takes its authored width, its shrink and the mono
                // face through the kit Input's own style refinement, so the
                // row lays out no wrapper around it.
                Input::new(&row.hex)
                    .small()
                    .w(Self::hex_width())
                    .flex_shrink_0()
                    .font_family(mono())
                    .disabled(self.pending)
                    .aria_label(if row.invalid {
                        format!("{label} color, invalid: use #rrggbb")
                    } else {
                        format!("{label} color")
                    })
                    // The toolkit's focus border would hide the removed
                    // outline while the field is edited.
                    .when(row.invalid, |input| {
                        input.focus_ring(false).border_color(rgb(p.removed))
                    }),
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
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let p = palette(cx);
        let viewport = window.viewport_size();
        let wide = viewport.width >= appearance::ui_size(1060.);
        self.wide = wide;
        self.reveal_focused(window, cx);
        let max_height = Self::max_height(window);
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
                    .map(|row| self.row_element(row, window, cx)),
            );
        }
        let name = self.name.read(cx).value().trim().to_owned();
        // The settings-card miniature at the picker card's own height, so the
        // draft previews with the picker's proportions.
        let preview = div()
            .debug_selector(|| "theme-editor-preview".into())
            .h(appearance::ui_size(132.))
            .flex_shrink_0()
            .p(px(2.))
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(p.border))
            .overflow_hidden()
            .child(settings::theme_preview(
                &self.preview_body,
                self.draft,
                if name.is_empty() {
                    "Untitled theme".to_owned()
                } else {
                    name
                },
                format!("Based on {}", self.base.label()),
                false,
                0,
                p,
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
        #[cfg(test)]
        let paint_stamp = {
            let form = cx.entity().downgrade();
            Some(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, cx| {
                        let _ = form.update(cx, |form, _| form.painted = Some(Instant::now()));
                    },
                )
                .w(px(0.))
                .h(px(0.))
                .into_any_element(),
            )
        };
        #[cfg(not(test))]
        let paint_stamp: Option<AnyElement> = None;
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
            .children(paint_stamp)
            // After the rows: each hex field's view node is registered here,
            // outside its cached row (`TokenRowView`).
            .children(
                self.rows
                    .iter()
                    .map(|row| settings::ViewNodeAnchor::new(row.hex.entity_id())),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    fn open_app(cx: &mut TestAppContext) -> (Entity<GitTurtle>, &mut VisualTestContext) {
        // GitTurtle::new starts a real preferences worker; saves reply from it.
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
            init(cx);
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
        settings::shown(cx, selector).unwrap_or_else(|| panic!("{selector} is rendered"))
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
        let bounds =
            settings::shown(cx, selector).unwrap_or_else(|| panic!("{selector} is rendered"));
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

    /// Wait until the preference writer has answered everything submitted so
    /// far, without drawing. The executor is serialized, so a job queued now
    /// runs after the pending save, and parking then delivers that save's
    /// reply to the window.
    fn drain_preference_writer(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) {
        let answered = Arc::new(AtomicBool::new(false));
        let flag = answered.clone();
        let _response = cx.read(|cx| {
            app.read(cx).preferences_writer.submit(move || {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            })
        });
        wait_without_drawing(cx, |_| answered.load(Ordering::SeqCst));
        cx.run_until_parked();
    }

    /// The palette of every draw since the last call, and clear the record.
    fn drawn(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> Vec<Palette> {
        cx.update(|_, cx| app.update(cx, |app, _| std::mem::take(&mut app.draws)))
    }

    /// Builds of the Settings picker's twenty built-in miniatures so far.
    fn preview_renders(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> usize {
        cx.read(|cx| {
            app.read(cx)
                .theme_previews
                .iter()
                .filter(|(drawn, _)| matches!(drawn, ThemeSelection::BuiltIn(_)))
                .map(|(_, body)| body.read(cx).renders())
                .sum()
        })
    }

    /// Builds of each custom card's miniature so far, by theme id.
    fn custom_preview_renders(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
    ) -> Vec<(u32, usize)> {
        cx.read(|cx| {
            app.read(cx)
                .theme_previews
                .iter()
                .filter_map(|(drawn, body)| match drawn {
                    ThemeSelection::Custom(id) => Some((*id, body.read(cx).renders())),
                    ThemeSelection::BuiltIn(_) => None,
                })
                .collect()
        })
    }

    /// Palette changes each custom card's miniature has taken, by theme id.
    fn custom_palette_changes(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
    ) -> Vec<(u32, usize)> {
        cx.read(|cx| {
            app.read(cx)
                .theme_previews
                .iter()
                .filter_map(|(drawn, body)| match drawn {
                    ThemeSelection::Custom(id) => Some((*id, body.read(cx).palette_changes())),
                    ThemeSelection::BuiltIn(_) => None,
                })
                .collect()
        })
    }

    /// Whether the picker marks `selection`'s card as the active theme.
    fn card_checked(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        selection: ThemeSelection,
    ) -> bool {
        let miniature = cx.read(|cx| app.read(cx).theme_preview_body(selection).entity_id());
        let selector: &'static str = format!("theme-check-{miniature}").leak();
        settings::page_shows(cx, selector)
    }

    /// Whether the picker shows the readability glyph on `selection`'s card.
    fn card_warned(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        selection: ThemeSelection,
    ) -> bool {
        let miniature = cx.read(|cx| app.read(cx).theme_preview_body(selection).entity_id());
        let selector: &'static str = format!("theme-warning-{miniature}").leak();
        settings::page_shows(cx, selector)
    }

    /// Palette applications through `apply_appearance` so far.
    fn applications(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> usize {
        cx.read(|cx| app.read(cx).theme_editor.preview_applications)
    }

    fn submissions(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> (u64, bool, bool) {
        cx.read(|cx| {
            let app = app.read(cx);
            (app.generation, app.task.is_some(), app.loading.is_some())
        })
    }

    /// Builds of the cached Settings page so far (`settings::SettingsPage`).
    fn page_renders(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> usize {
        cx.read(|cx| app.read(cx).settings_page.read(cx).renders())
    }

    /// Builds of the open editor's form so far.
    fn form_renders(cx: &mut VisualTestContext, form: &Entity<ThemeForm>) -> usize {
        cx.read(|cx| form.read(cx).renders)
    }

    /// Settle, run `act`, flush its effects, and return how many times the
    /// Settings page was built and the window drawn as a result. Nothing here
    /// asks for a frame: the draws are the ones the input's own effects ask
    /// for, so a page build counted here came from a notification or a
    /// refresh that the input caused.
    fn page_builds_after(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        act: impl FnOnce(&mut VisualTestContext),
    ) -> (usize, usize) {
        settle(cx);
        let page = page_renders(cx, app);
        let draws = draw_count(cx, app);
        act(cx);
        cx.run_until_parked();
        (page_renders(cx, app) - page, draw_count(cx, app) - draws)
    }

    /// Take the pointer off the window, so no hovered control's tooltip
    /// timer or hover state joins the frames under test.
    fn mouse_away(cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(point(px(-8.), px(-8.)), None, Modifiers::default());
        settle(cx);
    }

    /// An empty repository in a temporary directory.
    fn repository_fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(directory.path())
            .args([
                "-c",
                "init.templateDir=",
                "init",
                "-q",
                "--initial-branch=main",
            ])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success(), "git init");
        directory
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
            settings::page_shows(cx, "theme-token-invalid-panel"),
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
            !settings::page_shows(cx, "theme-token-invalid-panel"),
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
            // The field is the kit Input itself, sized by its style
            // refinement; the state reports its text area, the authored width
            // less the small input's 8 px padding and 1 px border per side.
            let inset = px(2. * 8.) + px(2. * 1.);
            let hex = cx.read(|cx| {
                form.read(cx)
                    .row(TokenKind::ALL[0])
                    .hex
                    .read(cx)
                    .input_bounds()
            });
            assert_eq!(hex.size.width, ThemeForm::hex_width() - inset);
            let fit = cx.update(|window, _| ThemeForm::hex_fit_width(window));
            assert!(
                hex.size.width + inset >= fit,
                "seven characters and the caret fit the hex field: {:?} < {fit:?}",
                hex.size.width + inset
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
                app.set_custom_themes(vec![theme.clone()], cx);
                app.settings.theme = ThemeSelection::Custom(7);
                app.apply_appearance(window, cx);
            })
        });
        settle(cx);
        theme
    }

    /// Keyboard-only: from the card, Tab past New theme…, Import…, Edit… and
    /// Export… reaches Delete…, and Space opens the alert as a native input.
    /// The frame after the key draws the alert; the next frame runs its
    /// callbacks, then draws.
    fn open_delete_alert(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) {
        let card = cx.update(|_, cx| app.read(cx).theme_editor.card_focus(cx));
        cx.update(|window, cx| {
            card.focus(window, cx);
            window.focus_next(cx);
        });
        cx.simulate_keystrokes("tab tab tab tab");
        settle(cx);
        assert!(
            cx.update(|window, cx| card.contains_focused(window, cx)),
            "Delete… is the fifth tab stop in the card"
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
        assert!(!settings::page_shows(cx, "delete-custom-theme-7"));
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

    /// The card's current export or import result.
    fn transfer_result(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
    ) -> (Option<String>, Option<String>, bool) {
        cx.read(|cx| {
            let state = &app.read(cx).theme_editor;
            (
                state.notice.clone(),
                state.error.clone(),
                state.transfer_pending,
            )
        })
    }

    /// Occupy the preference executor until the returned sender is dropped.
    /// A transfer whose file work is on that executor can make no progress
    /// meanwhile; one that read or wrote on the UI thread would finish anyway.
    fn hold_preference_executor(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
    ) -> std::sync::mpsc::Sender<()> {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let _reply = cx.read(|cx| {
            app.read(cx).preferences_writer.submit(move || {
                let _ = wait.recv();
                Ok(())
            })
        });
        settle(cx);
        release
    }

    /// While `hold` occupies the preference executor, the transfer reports
    /// nothing and keeps the card.
    fn assert_waits_for_the_executor(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        hold: std::sync::mpsc::Sender<()>,
    ) {
        for _ in 0..3 {
            settle(cx);
        }
        let (notice, error, pending) = transfer_result(cx, app);
        assert_eq!(
            (notice, error),
            (None, None),
            "the file work waits for the preference executor"
        );
        assert!(pending, "the card keeps the transfer until it reports");
        drop(hold);
    }

    /// Answer the export dialog with `destination` and wait for the card to
    /// report. The preference executor answers from its own thread.
    fn export_to(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        id: u32,
        destination: Option<PathBuf>,
        hold: Option<std::sync::mpsc::Sender<()>>,
    ) -> (Option<String>, Option<String>) {
        click(cx, format!("export-custom-theme-{id}").leak());
        cx.simulate_new_path_selection(|_| destination);
        if let Some(hold) = hold {
            assert_waits_for_the_executor(cx, app, hold);
        }
        wait_for(cx, |cx| {
            let (notice, error, pending) = transfer_result(cx, app);
            !pending && (notice.is_some() || error.is_some() || !cx.did_prompt_for_paths())
        });
        let (notice, error, pending) = transfer_result(cx, app);
        assert!(!pending, "the export released the card");
        (notice, error)
    }

    /// Answer the import dialog with `chosen` and wait for the card to report.
    fn import_file(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        chosen: Option<PathBuf>,
        hold: Option<std::sync::mpsc::Sender<()>>,
    ) -> (Option<String>, Option<String>) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.import_custom_theme(window, cx));
        });
        settle(cx);
        cx.simulate_path_prompt_response(|options| {
            assert!(
                options.files && !options.directories && !options.multiple,
                "the theme picker chooses one existing file"
            );
            chosen.map(|path| vec![path])
        });
        if let Some(hold) = hold {
            assert_waits_for_the_executor(cx, app, hold);
        }
        wait_for(cx, |cx| {
            let (notice, error, pending) = transfer_result(cx, app);
            !pending && (notice.is_some() || error.is_some() || !cx.did_prompt_for_paths())
        });
        let (notice, error, pending) = transfer_result(cx, app);
        assert!(!pending, "the import released the card");
        (notice, error)
    }

    /// Export writes the specification's document byte for byte on the
    /// preference executor, and importing it back into the store that still
    /// holds the original adds "<name> (imported)" with the same tokens,
    /// highlighted but not applied.
    #[gpui::test]
    fn exporting_and_importing_round_trips_a_theme_without_applying_it(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let theme = with_active_harbor(cx, &app);
        let fixture = tempfile::tempdir().unwrap();
        let destination = fixture.path().join("harbor.gitturtle-theme.json");

        let hold = hold_preference_executor(cx, &app);
        let (notice, error) = export_to(cx, &app, 7, Some(destination.clone()), Some(hold));
        assert_eq!(error, None);
        let notice = notice.expect("the card reports the written path");
        assert!(notice.contains("Harbor"), "{notice}");
        // The written path, clamped from the front so the file name survives.
        assert!(
            notice.contains(&written_path_text(&destination)),
            "{notice}"
        );
        assert!(
            notice.contains("harbor.gitturtle-theme.json"),
            "the file name survives the clamp: {notice}"
        );
        // Byte for byte the document from the specification.
        assert_eq!(std::fs::read(&destination).unwrap(), theme.to_document());
        // Temp-and-rename leaves nothing beside it.
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);

        let applied_before = applied(cx);
        let hold = hold_preference_executor(cx, &app);
        let (notice, error) = import_file(cx, &app, Some(destination), Some(hold));
        assert_eq!(error, None);
        let notice = notice.expect("the card reports the import");
        assert!(notice.contains("Harbor (imported)"), "{notice}");
        let (themes, selection, highlighted) = cx.read(|cx| {
            let app = app.read(cx);
            (
                app.custom_themes.clone(),
                app.settings.theme,
                app.theme_editor.imported,
            )
        });
        assert_eq!(themes.len(), 2);
        assert_eq!(themes[0], theme);
        assert_eq!(themes[1].name, "Harbor (imported)");
        assert_eq!(themes[1].palette, theme.palette);
        assert_eq!(themes[1].base, theme.base);
        assert_eq!(
            highlighted,
            Some(themes[1].id),
            "the new row is highlighted"
        );
        // Added, not applied: the window keeps the theme it had.
        assert_eq!(selection, ThemeSelection::Custom(7));
        assert_eq!(applied(cx), applied_before);
        settle(cx);
        assert!(settings::page_shows(cx, "export-custom-theme-8"));
    }

    /// Cancelling either dialog is quiet, and an unwritable destination
    /// reports the failure without leaving a partial file behind.
    #[gpui::test]
    fn cancelling_is_quiet_and_an_unwritable_destination_reports_its_failure(
        cx: &mut TestAppContext,
    ) {
        let (app, cx) = open_app(cx);
        let theme = with_active_harbor(cx, &app);

        assert_eq!(export_to(cx, &app, 7, None, None), (None, None));
        assert_eq!(import_file(cx, &app, None, None), (None, None));
        assert_eq!(
            cx.read(|cx| app.read(cx).custom_themes.clone()),
            vec![theme]
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let fixture = tempfile::tempdir().unwrap();
            let locked = fixture.path().join("locked");
            std::fs::create_dir(&locked).unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
            let (notice, error) = export_to(cx, &app, 7, Some(locked.join("harbor.json")), None);
            let entries = std::fs::read_dir(&locked).unwrap().count();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(notice, None);
            let error = error.expect("the card reports the refused destination");
            assert!(error.starts_with("Could not export “Harbor”:"), "{error}");
            assert_eq!(entries, 0, "no partial or temporary file is left behind");
        }
    }

    /// Every refusal in the specification's "Import and export" rules reaches
    /// the card with its own message, and none of them touches the store.
    #[gpui::test]
    fn refused_documents_name_their_problem_and_change_nothing(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let theme = with_active_harbor(cx, &app);
        let fixture = tempfile::tempdir().unwrap();
        let sunset = CustomTheme::from_base(1, "Sunset", ThemeChoice::Nord);
        let document = |edit: fn(&mut serde_json::Value)| {
            let mut value: serde_json::Value =
                serde_json::from_slice(&sunset.to_document()).unwrap();
            edit(&mut value);
            serde_json::to_vec(&value).unwrap()
        };
        let write = |name: &str, bytes: Vec<u8>| {
            let path = fixture.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };

        let mut cases: Vec<(&str, PathBuf, String)> = vec![
            (
                "65 KiB file",
                write("large.json", vec![b' '; 65 * 1024]),
                "This theme file is larger than 64 KiB, the most GitTurtle imports.".into(),
            ),
            (
                "a folder",
                fixture.path().to_path_buf(),
                "A theme file must be a regular file; this one is a folder.".into(),
            ),
            (
                "not JSON",
                write("prose.json", b"this is not a theme".to_vec()),
                "This file is not a GitTurtle theme: expected ident at line 1 column 2.".into(),
            ),
            (
                "a newer format version",
                write(
                    "newer.json",
                    document(|value| value["version"] = serde_json::json!(2)),
                ),
                "This theme was exported by a newer GitTurtle (format version 2). Update GitTurtle to import it.".into(),
            ),
            (
                "a missing token",
                write(
                    "missing.json",
                    document(|value| {
                        value["tokens"].as_object_mut().unwrap().remove("hunk");
                    }),
                ),
                "The theme file is missing the token “hunk”.".into(),
            ),
            (
                "an unknown key",
                write(
                    "unknown.json",
                    document(|value| value["sparkle"] = serde_json::json!("#ffffff")),
                ),
                "The theme file has an unknown key “sparkle”.".into(),
            ),
        ];
        #[cfg(unix)]
        {
            let target = write("target.json", sunset.to_document());
            let link = fixture.path().join("linked.json");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            cases.push((
                "a symbolic link",
                link,
                "A theme file must be a regular file; this one is a symbolic link.".into(),
            ));
        }

        for (what, path, expected) in cases {
            let (notice, error) = import_file(cx, &app, Some(path), None);
            assert_eq!(notice, None, "{what}");
            assert_eq!(error, Some(expected), "{what}");
            let (themes, highlighted) = cx.read(|cx| {
                let app = app.read(cx);
                (app.custom_themes.clone(), app.theme_editor.imported)
            });
            assert_eq!(themes, vec![theme.clone()], "{what} leaves the store alone");
            assert_eq!(highlighted, None, "{what}");
        }

        // The 32-theme bound. Import… is disabled at the bound, so only a
        // store that filled up while the dialog was open reaches this.
        let valid = write("sunset.json", sunset.to_document());
        let full: Vec<CustomTheme> = (1..=MAX_CUSTOM_THEMES as u32)
            .map(|id| CustomTheme::from_base(id, format!("Theme {id}"), ThemeChoice::Nord))
            .collect();
        cx.update(|_, cx| {
            app.update(cx, |app, cx| app.set_custom_themes(full.clone(), cx));
        });
        settle(cx);
        let (notice, error) = import_file(cx, &app, Some(valid), None);
        assert_eq!(notice, None);
        assert_eq!(
            error,
            Some(format!(
                "Up to {MAX_CUSTOM_THEMES} custom themes can be saved. Delete one before importing another."
            ))
        );
        assert_eq!(cx.read(|cx| app.read(cx).custom_themes.clone()), full);
    }

    /// A document may be valid apart from a 64 KiB run of its own text. That
    /// import still succeeds against the fallback base, and the notice it
    /// leaves in the card stays short enough for the card to keep its shape.
    #[gpui::test]
    fn a_document_with_an_enormous_base_imports_with_a_bounded_notice(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let theme = with_active_harbor(cx, &app);
        let fixture = tempfile::tempdir().unwrap();
        let sunset = CustomTheme::from_base(1, "Sunset", ThemeChoice::Nord);
        let mut value: serde_json::Value = serde_json::from_slice(&sunset.to_document()).unwrap();
        // Just under the reader's bound, so the document is read and parsed.
        let enormous = "b".repeat(63 * 1024);
        value["base"] = serde_json::json!(enormous);
        let path = fixture.path().join("enormous-base.json");
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(
            bytes.len() < custom::MAX_THEME_DOCUMENT_BYTES,
            "{}",
            bytes.len()
        );
        std::fs::write(&path, bytes).unwrap();

        let (notice, error) = import_file(cx, &app, Some(path), None);
        assert_eq!(error, None);
        // A 64 KiB document takes longer to read and parse than the card
        // releases the transfer for, so the notice lands with the save.
        let notice = notice.unwrap_or_else(|| {
            wait_for(cx, |cx| {
                let (notice, error, _) = transfer_result(cx, &app);
                notice.is_some() || error.is_some()
            });
            let (notice, error, _) = transfer_result(cx, &app);
            assert_eq!(error, None);
            notice.expect("the card reports the import")
        });
        assert!(notice.contains("Imported “Sunset”."), "{notice}");
        // The quoted base is clamped where the message is built, to the
        // base's own cap so the unbreakable token fits one line with its
        // tail, and nothing downstream — the card's text or its aria_label —
        // is unbounded.
        assert!(
            notice.len() < 400,
            "the notice stays a sentence, not a document: {} characters",
            notice.chars().count()
        );
        assert!(
            notice.contains(&format!(
                "The base theme “{}…” is not available",
                "b".repeat(custom::MAX_BASE_FRAGMENT_CHARS)
            )),
            "{notice}"
        );
        let themes = cx.read(|cx| app.read(cx).custom_themes.clone());
        assert_eq!(themes.len(), 2);
        assert_eq!(themes[0], theme);
        assert_eq!(themes[1].name, "Sunset");
        // Stored against the fallback base for its lightness, with its tokens.
        assert_eq!(themes[1].base, ThemeChoice::Midnight);
        assert_eq!(themes[1].palette, sunset.palette);
    }

    /// A collision between two names at the 128-byte bound is a legitimate
    /// import, and the notice about it still has to read: each quoted string
    /// is clamped to a fragment with an ellipsis, and when the document's
    /// base is unknown as well the three fragments share the card's two-line
    /// budget so the sentence fits `MAX_NOTICE_CHARS`, ends with the base
    /// that was used instead, and never stops mid-word without an ellipsis.
    #[gpui::test]
    fn a_long_name_collision_keeps_the_notice_within_the_card(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let long = "a".repeat(128);
        let existing = CustomTheme::from_base(7, long.clone(), ThemeChoice::Nord);
        let clamped = format!("{}…", "a".repeat(custom::MAX_MESSAGE_FRAGMENT_CHARS));
        let long_base = "aurora-nine-unknown-base-".repeat(5);
        assert_eq!(long_base.len(), 125);

        for unknown_base in [false, true] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.set_custom_themes(vec![existing.clone()], cx);
                    app.settings.theme = ThemeSelection::Custom(7);
                    app.apply_appearance(window, cx);
                })
            });
            settle(cx);
            let mut value: serde_json::Value =
                serde_json::from_slice(&existing.to_document()).unwrap();
            if unknown_base {
                value["base"] = serde_json::json!(long_base);
            }
            let path = fixture
                .path()
                .join(format!("collision-{unknown_base}.json"));
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

            let (notice, error) = import_file(cx, &app, Some(path), None);
            assert_eq!(error, None);
            let notice = notice.unwrap_or_else(|| {
                wait_for(cx, |cx| {
                    let (notice, error, _) = transfer_result(cx, &app);
                    notice.is_some() || error.is_some()
                });
                let (notice, error, _) = transfer_result(cx, &app);
                assert_eq!(error, None);
                notice.expect("the card reports the import")
            });
            if unknown_base {
                // Three clauses share the budget, so each fragment is shorter
                // than the single-clause clamp; the sentence still reaches the
                // base that was used instead.
                assert!(
                    notice.chars().count() <= custom::MAX_NOTICE_CHARS,
                    "{} characters: {notice}",
                    notice.chars().count()
                );
                assert!(notice.starts_with("Imported “aaaaaaaa"), "{notice}");
                assert!(notice.contains("…”. A theme named “aaaaaaaa"), "{notice}");
                assert!(
                    notice.contains("…” already exists, so the imported one was renamed. The base theme “aurora-nine-"),
                    "{notice}"
                );
                assert!(
                    notice.ends_with("…” is not available in this version of GitTurtle; Midnight is used as the base."),
                    "{notice}"
                );
            } else {
                assert_eq!(
                    notice,
                    format!(
                        "Imported “{clamped}”. A theme named “{clamped}” already exists, so the imported one was renamed."
                    )
                );
            }
            // Every quoted fragment is at most 64 characters plus its ellipsis,
            // and nothing that was cut ends without one: a cut fragment is a
            // strict prefix of the string it quotes followed by the ellipsis.
            let resolved = format!("{} (imported)", "a".repeat(117));
            let quoted_sources = [resolved.as_str(), existing.name.as_str(), &long_base];
            let fragments = notice
                .split('“')
                .skip(1)
                .map(|rest| rest.split('”').next().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(fragments.len(), if unknown_base { 3 } else { 2 });
            for (fragment, source) in fragments.iter().zip(quoted_sources) {
                let kept = fragment
                    .strip_suffix('…')
                    .expect("a 125-byte string is cut");
                assert!(
                    kept.chars().count() >= custom::MIN_MESSAGE_FRAGMENT_CHARS
                        && kept.chars().count() <= custom::MAX_MESSAGE_FRAGMENT_CHARS
                        && kept.len() < source.len()
                        && source.starts_with(kept),
                    "{fragment}"
                );
            }
            // The store holds the full resolved name; only the notice is clamped.
            let themes = cx.read(|cx| app.read(cx).custom_themes.clone());
            assert_eq!(themes.len(), 2);
            assert_eq!(themes[0], existing);
            assert_eq!(themes[1].name, format!("{} (imported)", "a".repeat(117)));
            assert_eq!(
                themes[1].base,
                if unknown_base {
                    ThemeChoice::Midnight
                } else {
                    ThemeChoice::Nord
                }
            );
            assert_eq!(themes[1].palette, existing.palette);
        }
    }

    /// Export… is the only statement of what the row writes, so its tooltip
    /// has to open over the row the same way the card header's does.
    #[gpui::test]
    fn hovering_export_opens_its_tooltip_without_moving_the_row(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        with_active_harbor(cx, &app);
        // The tooltip's holder is laid out as the button itself was, at the
        // card widths the evidence captures use: the same height as its
        // neighbours and the same gap on either side.
        let mut export = Bounds::default();
        for width in [px(1000.), px(1440.)] {
            cx.simulate_resize(size(width, px(2400.)));
            settle(cx);
            let edit = bounds(cx, "edit-custom-theme-7".into());
            export = bounds(cx, "export-custom-theme-7".into());
            let delete = bounds(cx, "delete-custom-theme-7".into());
            assert_eq!(
                export.size.height, edit.size.height,
                "{width:?}: {export:?} {edit:?}"
            );
            assert_eq!(
                delete.origin.x - export.right(),
                export.origin.x - edit.right(),
                "{width:?}: {edit:?} {export:?} {delete:?}"
            );
        }

        assert!(cx.debug_bounds("export-custom-theme-tooltip").is_none());
        cx.simulate_mouse_move(export.center(), None, Modifiers::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        settle(cx);
        let tooltip = cx
            .debug_bounds("export-custom-theme-tooltip")
            .expect("the export tooltip opens over the row");
        assert!(tooltip.size.width > px(0.), "{tooltip:?}");
        // The row keeps its place while the tooltip is open.
        assert_eq!(bounds(cx, "export-custom-theme-7".into()), export);
    }

    /// Mouse down, then mouse up, on a Settings theme card: the card's real
    /// click path. GPUI refreshes the window for the press (its pressed
    /// state) and again for the release, so the press is dispatched on its
    /// own and its frame drained; returned are the draws after the release
    /// and the built-in and custom picker miniatures the release rebuilt.
    fn click_card(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        choice: ThemeChoice,
    ) -> (Vec<Palette>, usize, usize) {
        let position = bounds(cx, format!("settings-theme-{}", choice as usize)).center();
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        let _ = drawn(cx, app);
        let previews_before = preview_renders(cx, app);
        let custom_before: usize = custom_preview_renders(cx, app)
            .into_iter()
            .map(|(_, renders)| renders)
            .sum();
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
        });
        let draws = drawn(cx, app);
        let custom_after: usize = custom_preview_renders(cx, app)
            .into_iter()
            .map(|(_, renders)| renders)
            .sum();
        (
            draws,
            preview_renders(cx, app) - previews_before,
            custom_after - custom_before,
        )
    }

    /// Builds of the open editor's own miniature so far.
    fn draft_renders(cx: &mut VisualTestContext, form: &Entity<ThemeForm>) -> usize {
        cx.read(|cx| form.read(cx).preview_body.read(cx).renders())
    }

    /// Every live-preview edit and every Settings theme switch costs exactly
    /// one whole-window draw, and that draw already shows the new palette.
    ///
    /// The frame, not the palette work, is what this path is measured by
    /// (`docs/development/themes/spec.md` §Performance): one whole-window
    /// layout, paint and present of Settings with the dialog over it. Three
    /// ways to spend a second one are asserted absent here: a frame that still
    /// shows the old palette because the draft was applied later, a redundant
    /// frame after the coalesced application of a same-frame burst, and the
    /// frame a completed preference save asks for when its result changed
    /// nothing the window shows.
    ///
    /// `draws` records the palette of every draw. In a test build GPUI draws
    /// each dirty window as effects flush, exactly as the platform draws it
    /// when a frame is requested, so these are the window's own draws and not
    /// the harness's: nothing below forces one. The switch is a real click on
    /// the card; GPUI's click machinery calls `Window::refresh` on that mouse
    /// up, which bars cached-view reuse for the frame, so the miniature reuse
    /// is asserted on the edit frames and the click's rebuild is recorded.
    ///
    /// Two saved themes put a Your themes group in the picker, so the custom
    /// miniatures are under the same measurement: an edit of one of them
    /// reuses every custom miniature, the edited theme's own included, and its
    /// save gives only that theme's miniature the new palette.
    #[gpui::test]
    fn an_edit_and_a_switch_each_draw_the_window_once(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        settle(cx);
        seed_themes(cx, &app, 2);

        // A Settings theme switch. The first save also writes the status line,
        // a visible change that rightly costs its frame; every switch after it
        // is the steady state.
        let _ = drawn(cx, &app);
        let (first, ..) = click_card(cx, &app, ThemeChoice::TokyoNight);
        assert!(
            !first.is_empty()
                && first
                    .iter()
                    .all(|p| *p == ThemeChoice::TokyoNight.palette()),
            "the click draws the new palette and never the old one: {first:?}"
        );
        drain_preference_writer(cx, &app);
        assert_eq!(
            cx.read(|cx| app.read(cx).status.clone()),
            "Settings saved",
            "the first save reports itself"
        );
        settle(cx);
        let _ = drawn(cx, &app);
        cx.run_until_parked();
        assert_eq!(
            drawn(cx, &app),
            [],
            "nothing is pending before the steady-state switch"
        );
        let (draws, rebuilt, custom_rebuilt) = click_card(cx, &app, ThemeChoice::Nord);
        assert_eq!(
            draws,
            [ThemeChoice::Nord.palette()],
            "a click switch draws the window once after its release, in the new palette"
        );
        // Recorded, not asserted away: GPUI's click path refreshes the window
        // on the mouse up, and a refresh rebuilds every cached view in that
        // frame, the twenty miniatures included. The reuse a switch could get
        // is the edit frames' below.
        assert_eq!(
            (rebuilt, custom_rebuilt),
            (ThemeChoice::ALL.len(), 2),
            "the click's Window::refresh rebuilds all twenty built-in miniatures and both custom ones"
        );
        drain_preference_writer(cx, &app);
        assert_eq!(
            drawn(cx, &app),
            [],
            "the completed save changed nothing visible, so it draws nothing"
        );

        // Live-preview edits of a saved theme, Seed 01, opened with its row's
        // Edit…. Opening the editor is its own frame; measure from the first
        // edit after it. While the dialog is open the focused name field
        // blinks its caret, which asks for frames of its own; those are not
        // this path's, so the edits below assert the applications they cost
        // and that no frame ever shows a stale palette.
        click(cx, "edit-custom-theme-1");
        let form = form(cx, &app);
        assert_eq!(cx.read(|cx| form.read(cx).base), ThemeChoice::ALL[0]);
        next_frame(cx);
        let inputs = cx.read(|cx| {
            [TokenKind::Canvas, TokenKind::Panel, TokenKind::Accent]
                .map(|kind| form.read(cx).row(kind).hex.clone())
        });
        let _ = drawn(cx, &app);

        for value in ["#101a26", "#0c1119", "#161c2a"] {
            let applied_before = applications(cx, &app);
            let previews_before = preview_renders(cx, &app);
            let custom_before = custom_preview_renders(cx, &app);
            let draft_before = draft_renders(cx, &form);
            // `replace_all` emits Change as typing the seventh character does.
            cx.update(|window, cx| {
                inputs[0].update(cx, |input, cx| input.replace_all(value, window, cx))
            });
            let draft = cx.read(|cx| form.read(cx).draft);
            assert_eq!(
                drawn(cx, &app),
                [draft],
                "{value} draws the window once, showing the draft and not the old palette"
            );
            assert_eq!(
                applications(cx, &app),
                applied_before + 1,
                "{value} applies its palette once"
            );
            // The picker's miniatures each draw one built-in palette, which
            // the draft does not alter: the edit frame reuses all twenty and
            // rebuilds only the draft's own.
            assert_eq!(
                preview_renders(cx, &app),
                previews_before,
                "{value} reuses every picker miniature"
            );
            // Seed 01's picker miniature draws its saved palette, not the
            // draft: it is reused like every other custom miniature.
            assert_eq!(
                custom_preview_renders(cx, &app),
                custom_before,
                "{value} reuses every custom miniature, the edited theme's own included"
            );
            assert_eq!(
                draft_renders(cx, &form),
                draft_before + 1,
                "{value} rebuilds only the draft's miniature"
            );
            // The coalescing callback has nothing left to apply, so it costs
            // no application and can only repeat the draft already on screen.
            let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
            assert!(ran > 0, "{value} registered its coalescing callback");
            assert_eq!(
                applications(cx, &app),
                applied_before + 1,
                "the coalescing frame after {value} applies nothing"
            );
            for palette in drawn(cx, &app) {
                assert_eq!(
                    palette, draft,
                    "no frame after {value} draws a stale palette"
                );
            }
        }

        // A burst inside one frame: the first edit applies in its handler, so
        // its frame already shows it, and the rest apply once at the next
        // frame callback. Two applications for three edits, never an old
        // palette.
        let applied_before = applications(cx, &app);
        cx.update(|window, cx| {
            for (index, input) in inputs.iter().enumerate() {
                let text = format!("#10{index:02}20");
                input.update(cx, |input, cx| input.replace_all(text, window, cx));
            }
        });
        let in_handler = drawn(cx, &app);
        assert_eq!(in_handler.len(), 1, "the burst draws once in its handler");
        assert_eq!(
            in_handler[0].canvas, 0x100020,
            "and that frame already shows the burst's first edit"
        );
        assert_eq!(
            applications(cx, &app),
            applied_before + 1,
            "the burst's first edit applies in its handler"
        );
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        let draft = cx.read(|cx| form.read(cx).draft);
        let coalesced = drawn(cx, &app);
        assert!(
            !coalesced.is_empty(),
            "the coalescing frame draws the rest of the burst"
        );
        for palette in coalesced {
            assert_eq!(palette, draft, "and every frame of it draws that draft");
        }
        assert_eq!(
            applications(cx, &app),
            applied_before + 2,
            "the rest of the burst applies once, at the frame"
        );

        // Save the edited theme. The Save click's release refreshes the window
        // as every click does, so the save is measured from after the click's
        // frames have settled: the preference executor is held until then. Of
        // the retained miniatures only Seed 01's is given the saved palette,
        // and it draws that palette in the frame that shows the save. That
        // frame is drawn under GPUI's refresh again, because the kit restores
        // focus to Edit… as it closes the dialog and `Window::focus` refreshes;
        // recorded like the click's rebuild, not asserted away. Every frame
        // after it reuses every miniature.
        let hold = hold_preference_executor(cx, &app);
        click(cx, "theme-editor-save");
        settle(cx);
        assert!(
            cx.read(|cx| app.read(cx).theme_editor.save_pending()),
            "the save waits for the executor"
        );
        let _ = drawn(cx, &app);
        let changes_before = custom_palette_changes(cx, &app);
        drop(hold);
        wait_without_drawing(cx, |cx| {
            cx.read(|cx| {
                let app = app.read(cx);
                !app.theme_editor.save_pending() && app.theme_editor.form.is_none()
            })
        });
        let saved = cx.read(|cx| app.read(cx).custom_themes[0].clone());
        assert_eq!(saved.palette, draft, "Seed 01 holds the burst's draft");
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.theme),
            ThemeSelection::Custom(1),
            "the saved theme is selected"
        );
        assert_eq!(
            custom_palette_changes(cx, &app),
            [(1, changes_before[0].1 + 1), (2, changes_before[1].1)],
            "the save gives only Seed 01's miniature a new palette"
        );
        assert_eq!(
            cx.read(|cx| {
                app.read(cx)
                    .theme_preview_body(ThemeSelection::Custom(1))
                    .read(cx)
                    .palette()
            }),
            saved.palette,
            "and that miniature draws it"
        );
        let draws = drawn(cx, &app);
        assert!(
            !draws.is_empty() && draws.iter().all(|palette| *palette == saved.palette),
            "the frames that show the save draw the saved palette: {draws:?}"
        );
        let custom_after = custom_preview_renders(cx, &app);
        assert!(
            custom_after[0].1 > changes_before[0].1,
            "Seed 01's miniature was rebuilt with its new palette: {custom_after:?}"
        );
        let after_save = (preview_renders(cx, &app), custom_after);
        cx.update(|_, cx| app.update(cx, |_, cx| cx.notify()));
        settle(cx);
        assert_eq!(
            (preview_renders(cx, &app), custom_preview_renders(cx, &app)),
            after_save,
            "a frame after the save reuses every miniature"
        );
    }

    /// The probe paints of `gitturtle.theme_edit_frame_ms` since the last
    /// call, and clear the record.
    fn edit_traces(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> Vec<EditTrace> {
        cx.update(|_, cx| {
            app.update(cx, |app, _| {
                std::mem::take(&mut app.theme_editor.edit_traces)
            })
        })
    }

    fn draw_count(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> usize {
        cx.read(|cx| app.read(cx).draws.len())
    }

    /// `gitturtle.theme_edit_frame_ms` is stamped once per open and once per
    /// burst of live-preview edits, and its end is taken in the paint of the
    /// first draw after the handler, after the dialog layer painted, never in
    /// a next-frame callback; an invalid value is not an edit.
    #[gpui::test]
    fn an_edit_trace_ends_in_that_frames_paint_after_the_dialog(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let _ = edit_traces(cx, &app);
        let _ = drawn(cx, &app);

        // The open. `click` settles with two forced draws afterwards; the
        // trace is still taken once, by the first draw after the handler.
        let draws_before = draw_count(cx, &app);
        click(cx, "custom-themes-new");
        let opened = edit_traces(cx, &app);
        assert_eq!(opened.len(), 1, "an open is traced once: {opened:?}");
        assert!(
            opened[0].draw > draws_before && opened[0].draw <= draw_count(cx, &app),
            "the open's trace ends in a draw after its handler"
        );
        let form = form(cx, &app);
        next_frame(cx);
        assert!(
            edit_traces(cx, &app).is_empty(),
            "frames after the open add no trace"
        );
        let input = cx.read(|cx| form.read(cx).row(TokenKind::Canvas).hex.clone());

        for value in ["#101a26", "#0c1119"] {
            let draws_before = draw_count(cx, &app);
            cx.update(|window, cx| {
                input.update(cx, |input, cx| input.replace_all(value, window, cx))
            });
            let draws_after = draw_count(cx, &app);
            assert_eq!(draws_after, draws_before + 1, "{value} draws once");
            let traces = edit_traces(cx, &app);
            assert_eq!(traces.len(), 1, "{value} is traced once: {traces:?}");
            let trace = traces[0];
            assert_eq!(
                trace.draw, draws_after,
                "the end stamp of {value} is taken during that draw's paint"
            );
            let painted = cx
                .read(|cx| form.read(cx).painted)
                .expect("the dialog painted");
            assert!(
                trace.started <= painted && painted <= trace.ended,
                "{value}: the probe paints after the dialog layer"
            );
            // The frame callbacks that follow the draw, and later frames, add
            // no line: the boundary is the paint, not a callback.
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
            });
            assert!(
                edit_traces(cx, &app).is_empty(),
                "no trace from the next-frame callback after {value}"
            );
            settle(cx);
            assert!(edit_traces(cx, &app).is_empty());
        }

        // An invalid value applies nothing and prints nothing.
        cx.update(|window, cx| input.update(cx, |input, cx| input.replace_all("#12", window, cx)));
        assert!(
            edit_traces(cx, &app).is_empty(),
            "an invalid value is not a live-preview edit"
        );

        // A burst before any draw is one line, from its first edit.
        let inputs = cx.read(|cx| {
            [TokenKind::Canvas, TokenKind::Panel, TokenKind::Accent]
                .map(|kind| form.read(cx).row(kind).hex.clone())
        });
        let before = Instant::now();
        cx.update(|window, cx| {
            for (index, input) in inputs.iter().enumerate() {
                let text = format!("#10{index:02}20");
                input.update(cx, |input, cx| input.replace_all(text, window, cx));
            }
        });
        let traces = edit_traces(cx, &app);
        assert_eq!(traces.len(), 1, "a same-frame burst is traced once");
        assert!(traces[0].started >= before);
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        assert!(
            edit_traces(cx, &app).is_empty(),
            "the coalesced application of the burst is not traced again"
        );
    }

    fn seed_themes(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, count: u32) {
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                let themes = (1..=count)
                    .map(|n| {
                        let base = ThemeChoice::ALL[(n as usize - 1) % ThemeChoice::ALL.len()];
                        CustomTheme::from_base(n, format!("Seed {n:02}"), base)
                    })
                    .collect();
                app.set_custom_themes(themes, cx);
                cx.notify();
            })
        });
        settle(cx);
    }

    fn focused_row(
        app: &Entity<GitTurtle>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<(usize, usize)> {
        app.update(cx, |app, cx| {
            app.theme_editor
                .focused_row_action(&app.custom_themes, window, cx)
        })
    }

    /// Whether row `row` lies wholly inside the Your themes list's viewport,
    /// from the list's own scroll state after its prepaint (`item` is the
    /// list's padded viewport, `contents` all rows).
    fn row_in_view(app: &Entity<GitTurtle>, row: usize, cx: &App) -> bool {
        let app = app.read(cx);
        let state = app.theme_editor.rows_scroll().0.borrow();
        let Some(size) = state.last_item_size else {
            return false;
        };
        let viewport = size.item.height;
        let height = size.contents.height / app.custom_themes.len() as f32;
        let top = height * row as f32 + state.base_handle.offset().y;
        top >= px(-0.5) && top + height <= viewport + px(0.5)
    }

    /// With 32 saved themes the list is eight rows tall and draws eight rows.
    /// Tab reaches every row's Edit…, Export… and Delete… in order across the
    /// viewport boundary, and the frame after each key shows the focused row
    /// inside the list. This walks the first sixteen rows and the step onto
    /// the seventeenth; `tab_walks_the_last_rows_and_leaves_and_reenters_the_rows`
    /// walks the rest, so each half fits the gate's twenty-seed rerun.
    #[gpui::test]
    fn tab_walks_every_row_action_and_keeps_the_focused_row_in_view(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        seed_themes(cx, &app, 32);
        let list = bounds(cx, "custom-themes-rows".into());
        let row = bounds(cx, "custom-theme-1".into());
        assert_eq!(
            list.size.height,
            row.size.height * 8. + px(6.),
            "eight rows tall, plus the focus ring's room above and below"
        );
        assert!(settings::page_shows(cx, "custom-theme-8"));
        assert!(
            !settings::page_shows(cx, "custom-theme-9"),
            "the ninth row is not drawn"
        );
        assert!(settings::page_shows(cx, "custom-themes-scrollbar"));

        // From the card, whose New theme… and Import… are disabled at the
        // bound, GPUI's own step reaches the first row's Edit….
        let card = cx.update(|_, cx| app.read(cx).theme_editor.card_focus(cx));
        cx.update(|window, cx| {
            card.focus(window, cx);
            window.focus_next(cx);
        });
        settle(cx);
        assert_eq!(
            cx.update(|window, cx| focused_row(&app, window, cx)),
            Some((0, 0)),
            "the first row's Edit… follows the card"
        );
        let actions = (0..16)
            .flat_map(|row| (0..3).map(move |action| (row, action)))
            .chain(std::iter::once((16, 0)));
        for (row, action) in actions.skip(1) {
            let (focused, in_view) = key_frames(
                cx,
                "tab",
                |_, _| (),
                |window, cx| (focused_row(&app, window, cx), row_in_view(&app, row, cx)),
            )
            .1;
            assert_eq!(
                focused,
                Some((row, action)),
                "Tab reaches row {row} action {action}"
            );
            assert!(
                in_view,
                "row {row} is in view in the frame after Tab onto action {action}"
            );
        }
    }

    /// The kit paints a focused control's 3 px ring outside the control and
    /// the list clips to its own bounds, so the list keeps the ring's room
    /// above its first row and below its last (`DESIGN.md`, Your themes): on
    /// a one-theme store and in the first and last viewport slots of a
    /// 32-theme store, at either end of the list, a focused action's ring
    /// bounds lie inside the list's bounds, while the rows keep their 30 px
    /// pitch from the list's first row and the scrollbar track stays on the
    /// rows.
    #[gpui::test]
    fn a_focused_row_actions_ring_lies_inside_the_list_in_every_slot(cx: &mut TestAppContext) {
        const RING: Pixels = px(3.);
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        fn assert_ring_inside(
            cx: &mut VisualTestContext,
            app: &Entity<GitTurtle>,
            id: u32,
            action: usize,
        ) {
            let handle = cx.update(|_, cx| app.read(cx).theme_editor.row_focus(id, cx));
            cx.update(|window, cx| State::focus_row_action(&handle, action, window, cx));
            native_frame(cx);
            let name = ["edit", "export", "delete"][action];
            let button = bounds(cx, format!("{name}-custom-theme-{id}"));
            let list = bounds(cx, "custom-themes-rows".into());
            let row = bounds(cx, format!("custom-theme-{id}"));
            assert!(
                button.top() - RING >= list.top()
                    && button.bottom() + RING <= list.bottom()
                    && button.left() - RING >= list.left()
                    && button.right() + RING <= list.right(),
                "{name} of theme {id}: ring {:?} inside the list {:?}",
                Bounds::from_corners(
                    button.origin - gpui::point(RING, RING),
                    button.bottom_right() + gpui::point(RING, RING),
                ),
                list
            );
            assert!(
                button.top() > row.top() && button.bottom() < row.bottom(),
                "the button sits inside its row"
            );
        }
        seed_themes(cx, &app, 1);
        let list = bounds(cx, "custom-themes-rows".into());
        let row = bounds(cx, "custom-theme-1".into());
        assert_eq!(
            row.top(),
            list.top() + RING,
            "the first row follows the ring's room"
        );
        assert_eq!(list.size.height, row.size.height + RING * 2.);
        assert_eq!(row.size.height, appearance::ui_size(30.));
        for action in 0..3 {
            assert_ring_inside(cx, &app, 1, action);
        }

        seed_themes(cx, &app, 32);
        let list = bounds(cx, "custom-themes-rows".into());
        let first = bounds(cx, "custom-theme-1".into());
        let second = bounds(cx, "custom-theme-2".into());
        assert_eq!(first.top(), list.top() + RING);
        assert_eq!(
            second.top() - first.top(),
            appearance::ui_size(30.),
            "row pitch"
        );
        let track = bounds(cx, "custom-themes-scrollbar".into());
        assert_eq!(
            track.top(),
            first.top(),
            "the track starts on the first row"
        );
        assert_eq!(
            track.size.height,
            appearance::ui_size(30.) * 8.,
            "and spans the eight rows"
        );
        assert_ring_inside(cx, &app, 1, 0);
        assert_ring_inside(cx, &app, 8, 2);
        cx.update(|_, cx| {
            app.read(cx)
                .theme_editor
                .rows_scroll()
                .scroll_to_item(31, ScrollStrategy::Top)
        });
        native_frame(cx);
        let last = bounds(cx, "custom-theme-32".into());
        assert_eq!(
            last.bottom(),
            list.bottom() - RING,
            "the last row ends at the ring's room"
        );
        assert_ring_inside(cx, &app, 25, 0);
        assert_ring_inside(cx, &app, 32, 2);
    }

    /// The rest of the walk: from the seventeenth row's Edit…, reached as
    /// `tab_theme_rows` reaches a row that is not drawn (the row scrolled in
    /// and `request_row_focus` for the render that draws it), Tab reaches
    /// every remaining action in order across the viewport boundary and
    /// leaves the rows past the last Delete…, Shift-Tab walks back, entering
    /// the list from either side lands on its edge row whatever rows are
    /// drawn, and the frame after each key shows the focused row inside the
    /// list.
    #[gpui::test]
    fn tab_walks_the_last_rows_and_leaves_and_reenters_the_rows(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        seed_themes(cx, &app, 32);
        let card = cx.update(|_, cx| app.read(cx).theme_editor.card_focus(cx));
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.theme_editor
                    .rows_scroll()
                    .scroll_to_item(16, ScrollStrategy::Nearest);
                app.theme_editor.request_row_focus(16, 0);
                cx.notify();
            })
        });
        // The render that draws the row registers the focus for the next
        // frame; native frames deliver it.
        native_frame(cx);
        native_frame(cx);
        assert_eq!(
            cx.update(|window, cx| focused_row(&app, window, cx)),
            Some((16, 0)),
            "the seventeenth row's Edit… is focused once drawn"
        );
        let actions = (16..32).flat_map(|row| (0..3).map(move |action| (row, action)));
        for (row, action) in actions.skip(1) {
            let (focused, in_view) = key_frames(
                cx,
                "tab",
                |_, _| (),
                |window, cx| (focused_row(&app, window, cx), row_in_view(&app, row, cx)),
            )
            .1;
            assert_eq!(
                focused,
                Some((row, action)),
                "Tab reaches row {row} action {action}"
            );
            assert!(
                in_view,
                "row {row} is in view in the frame after Tab onto action {action}"
            );
        }
        // Past the last Delete…, Tab leaves the rows; Shift-Tab returns to
        // that Delete… although the rows drawn need not include it.
        assert_eq!(
            key_frame(cx, "tab", |window, cx| focused_row(&app, window, cx)),
            None,
            "Tab from the last Delete… leaves the rows"
        );
        assert!(
            cx.update(|window, cx| window.focused(cx)).is_some(),
            "and lands on the control after the list"
        );
        cx.update(|_, cx| {
            app.read(cx)
                .theme_editor
                .rows_scroll()
                .scroll_to_item(0, ScrollStrategy::Top)
        });
        native_frame(cx);
        assert!(!settings::page_shows(cx, "custom-theme-32"));
        for expected in [(31, 2), (31, 1), (31, 0), (30, 2)] {
            let (focused, in_view) = key_frames(
                cx,
                "shift-tab",
                |_, _| (),
                |window, cx| {
                    (
                        focused_row(&app, window, cx),
                        row_in_view(&app, expected.0, cx),
                    )
                },
            )
            .1;
            assert_eq!(
                focused,
                Some(expected),
                "Shift-Tab walks back to {expected:?}"
            );
            assert!(in_view, "row {} is in view after Shift-Tab", expected.0);
        }
        // Entering forwards while the list shows its end lands on the first
        // row, drawn or not.
        cx.update(|window, cx| card.focus(window, cx));
        settle(cx);
        assert!(
            !settings::page_shows(cx, "custom-theme-1"),
            "the list still shows its end"
        );
        let (focused, in_view) = key_frames(
            cx,
            "tab",
            |_, _| (),
            |window, cx| (focused_row(&app, window, cx), row_in_view(&app, 0, cx)),
        )
        .1;
        assert_eq!(
            focused,
            Some((0, 0)),
            "Tab from the card reaches the first row's Edit…"
        );
        assert!(in_view, "and the first row is in view");
    }

    /// The card keeps the plain stack's geometry around the virtualized rows:
    /// the rows' box is exactly `30 px × min(rows, 8)` and starts where the
    /// stack's first row stood (the "No custom themes yet." line's place), the
    /// list over it is 3 px taller at each end for the focus ring and nothing
    /// else, the card grows by the rows less the empty line and the setting
    /// below it keeps its distance, with one, two, eight and 32 saved themes.
    /// The card itself follows the picker above it, whose Your themes group
    /// grows with the saved themes, at a constant distance, and everything
    /// inside the card is measured from that offset. A collapsed card or a
    /// moved row fails here even when the list's own geometry holds.
    /// Positions are exact; the card's height and the distances across its
    /// bottom edge allow half a device pixel, since the test window is at 2x
    /// and layout snaps to that grid.
    #[gpui::test]
    fn the_card_keeps_the_plain_stacks_geometry_around_the_rows(cx: &mut TestAppContext) {
        const RING: Pixels = px(3.);
        fn within_half_device_pixel(actual: Pixels, expected: Pixels, what: String) {
            assert!(
                (actual - expected).abs() <= px(0.25),
                "{what}: {actual:?} against {expected:?}"
            );
        }
        let (app, cx) = open_app(cx);
        let rem = cx.update(|window, _| window.rem_size());
        let (border, padding) = (px(1.), rem);
        let picker = bounds(cx, "settings-theme-picker".into());
        let card = bounds(cx, "custom-themes-card".into());
        let header = bounds(cx, "custom-themes-header".into());
        let empty = bounds(cx, "custom-themes-empty".into());
        let below = bounds(cx, "text-size-setting-interface".into());
        assert_eq!(header.top(), card.top() + border + padding);
        assert!(
            empty.top() > header.bottom(),
            "the content follows the header"
        );
        within_half_device_pixel(
            card.bottom() - empty.bottom(),
            padding + border,
            "zero themes: the empty line ends at the card's padding".into(),
        );
        let stack_top = empty.top();
        let distance_below = below.top() - card.bottom();
        for themes in [1u32, 2, 8, 32] {
            seed_themes(cx, &app, themes);
            let rows = appearance::ui_size(30.) * themes.min(8) as f32;
            let picker_now = bounds(cx, "settings-theme-picker".into());
            let card_now = bounds(cx, "custom-themes-card".into());
            let header_now = bounds(cx, "custom-themes-header".into());
            let list_box = bounds(cx, "custom-themes-rows-box".into());
            let list = bounds(cx, "custom-themes-rows".into());
            let first = bounds(cx, "custom-theme-1".into());
            let below_now = bounds(cx, "text-size-setting-interface".into());
            assert!(
                picker_now.bottom() > picker.bottom(),
                "{themes} themes: the picker grew by its Your themes group"
            );
            assert_eq!(
                card_now.top() - picker_now.bottom(),
                card.top() - picker.bottom(),
                "{themes} themes: the card keeps its distance from the picker"
            );
            let offset = card_now.top() - card.top();
            assert_eq!(
                (header_now.top(), header_now.left(), header_now.size),
                (header.top() + offset, header.left(), header.size),
                "{themes} themes: the header moves with the card and nothing else"
            );
            assert_eq!(
                list_box.top(),
                stack_top + offset,
                "{themes} themes: the rows start where the stack's first row stood"
            );
            assert_eq!(
                list_box.size.height, rows,
                "{themes} themes: the box is the rows"
            );
            assert_eq!(
                first.top(),
                list_box.top(),
                "{themes} themes: the first row is at its top"
            );
            assert_eq!(first.size.height, appearance::ui_size(30.));
            assert_eq!(
                list.top(),
                list_box.top() - RING,
                "{themes} themes: the ring's room above"
            );
            assert_eq!(
                list.bottom(),
                list_box.bottom() + RING,
                "{themes} themes: and below"
            );
            assert_eq!(list.left(), list_box.left());
            assert_eq!(list.right(), list_box.right());
            within_half_device_pixel(
                card_now.size.height,
                card.size.height - empty.size.height + rows,
                format!("{themes} themes: the card grows by the rows less the empty line"),
            );
            within_half_device_pixel(
                card_now.bottom() - list_box.bottom(),
                padding + border,
                format!("{themes} themes: the rows end at the card's padding"),
            );
            within_half_device_pixel(
                below_now.top() - card_now.bottom(),
                distance_below,
                format!("{themes} themes: the setting below keeps its distance"),
            );
            if themes > 8 {
                let track = bounds(cx, "custom-themes-scrollbar".into());
                assert_eq!(
                    track.top(),
                    list_box.top(),
                    "the track starts on the first row"
                );
                assert_eq!(
                    track.size.height, list_box.size.height,
                    "and spans the eight rows"
                );
                assert_eq!(track.right(), list_box.right(), "on the rows' right edge");
            }
        }
    }

    /// The Your themes list sits inside the Settings page's own scroll
    /// container. A wheel step over the rows moves the list and leaves the
    /// page where it is while the list can still move that way; once the
    /// list has reached its end, the same step scrolls the page, and a step
    /// back is the list's again.
    #[gpui::test]
    fn a_wheel_step_over_the_rows_scrolls_the_list_before_the_page(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        seed_themes(cx, &app, 32);
        // A window the page overflows, so the page can scroll at all.
        cx.simulate_resize(size(px(1440.), px(900.)));
        settle(cx);
        fn wheel(cx: &mut VisualTestContext, position: Point<Pixels>, step: Pixels) {
            cx.update(|window, cx| {
                window.dispatch_event(
                    PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                        position,
                        delta: gpui::ScrollDelta::Pixels(gpui::point(Pixels::ZERO, step)),
                        modifiers: Modifiers::default(),
                        touch_phase: gpui::TouchPhase::Moved,
                    }),
                    cx,
                );
            });
            settle(cx);
        }
        fn list_offset(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> Pixels {
            cx.read(|cx| {
                app.read(cx)
                    .theme_editor
                    .rows_scroll()
                    .0
                    .borrow()
                    .base_handle
                    .offset()
                    .y
            })
        }
        // Scroll the page over its right column, away from the list, until
        // the list is in view.
        let viewport = cx.update(|window, _| window.viewport_size());
        let mut rows = bounds(cx, "custom-themes-rows".into());
        for _ in 0..60 {
            if rows.top() >= Pixels::ZERO && rows.bottom() <= viewport.height {
                break;
            }
            wheel(cx, gpui::point(px(1100.), px(300.)), px(-200.));
            rows = bounds(cx, "custom-themes-rows".into());
        }
        assert!(
            rows.top() >= Pixels::ZERO && rows.bottom() <= viewport.height,
            "the list is in view: {rows:?} in {viewport:?}"
        );
        assert_eq!(
            list_offset(cx, &app),
            Pixels::ZERO,
            "the list starts at its top"
        );
        let page_before = rows.top();
        let over_list = rows.center();
        let reach = appearance::ui_size(30.) * 24.;

        wheel(cx, over_list, px(-30.));
        assert_eq!(list_offset(cx, &app), px(-30.), "the step moves the list");
        assert_eq!(
            bounds(cx, "custom-themes-rows".into()).top(),
            page_before,
            "and not the page"
        );

        wheel(cx, over_list, px(-10_000.));
        assert_eq!(
            list_offset(cx, &app),
            -reach,
            "a long step reaches the list's end"
        );
        assert_eq!(
            bounds(cx, "custom-themes-rows".into()).top(),
            page_before,
            "and still not the page"
        );

        wheel(cx, over_list, px(-30.));
        let page_after = bounds(cx, "custom-themes-rows".into()).top();
        assert_eq!(list_offset(cx, &app), -reach, "the list stays at its end");
        assert_eq!(
            page_after,
            page_before - px(30.),
            "so the step scrolls the page"
        );

        wheel(cx, gpui::point(over_list.x, over_list.y - px(30.)), px(30.));
        assert_eq!(
            list_offset(cx, &app),
            px(30.) - reach,
            "a step back is the list's"
        );
        assert_eq!(
            bounds(cx, "custom-themes-rows".into()).top(),
            page_after,
            "and the page stays"
        );
    }

    /// The list clips at its own bounds, the ring's room included, so a row
    /// partly scrolled out of the viewport paints into that room. Off a row
    /// boundary, two strips of the card's surface cover the room at each end
    /// of the viewport (`DESIGN.md`, Your themes: the room is the ring's); on
    /// a boundary the rows fill the viewport exactly, the strips are absent
    /// and a focused edge-slot action's ring shows in the room. The boundary
    /// is judged where the list will stand after the frame's reveal: focusing
    /// a partly hidden row snaps the list to a boundary in that same frame,
    /// so its ring is never drawn under a strip, and a scroll away afterwards
    /// leaves the reveal alone and brings the strips back.
    #[gpui::test]
    fn strips_cover_the_ring_room_while_the_rows_stand_off_a_boundary(cx: &mut TestAppContext) {
        const RING: Pixels = px(3.);
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        seed_themes(cx, &app, 32);
        fn list_offset(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) -> Pixels {
            cx.read(|cx| {
                app.read(cx)
                    .theme_editor
                    .rows_scroll()
                    .0
                    .borrow()
                    .base_handle
                    .offset()
                    .y
            })
        }
        /// Move the rows as a wheel step or a scrollbar drag does, then draw.
        /// Both notify the view they were painted in, the Settings page.
        fn scroll_rows(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, y: Pixels) {
            cx.update(|_, cx| {
                app.update(cx, |app, cx| {
                    app.theme_editor
                        .rows_scroll()
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(gpui::point(Pixels::ZERO, y));
                    app.notify_settings_page(cx);
                })
            });
            native_frame(cx);
        }
        fn strips(cx: &mut VisualTestContext) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
            let top = cx.debug_bounds("custom-themes-ring-room-top");
            let bottom = cx.debug_bounds("custom-themes-ring-room-bottom");
            assert_eq!(top.is_some(), bottom.is_some(), "the strips come as a pair");
            top.zip(bottom)
        }
        /// The strips of a frame that builds the page: a replayed frame
        /// records no page selector.
        fn built_strips(cx: &mut VisualTestContext) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
            assert!(settings::page_shows(cx, "custom-themes-rows"));
            strips(cx)
        }
        /// Focus `action` of theme `id` and stop after the one frame that
        /// reveals its row, which the test app draws as the focus change's
        /// effects flush (or here, if it did not), so the caller inspects
        /// that frame and not a later one drawn after the offset settled.
        fn focus_action(
            cx: &mut VisualTestContext,
            app: &Entity<GitTurtle>,
            id: u32,
            action: usize,
        ) {
            let handle = cx.update(|_, cx| app.read(cx).theme_editor.row_focus(id, cx));
            let draws = cx.read(|cx| app.read(cx).draws.len());
            let page = page_renders(cx, app);
            cx.update(|window, cx| State::focus_row_action(&handle, action, window, cx));
            if cx.read(|cx| app.read(cx).draws.len()) == draws {
                cx.update(|window, cx| window.draw(cx).clear(cx));
            }
            assert_eq!(
                cx.read(|cx| app.read(cx).draws.len()),
                draws + 1,
                "the frame inspected is the one the focus change drew"
            );
            assert!(
                page_renders(cx, app) > page,
                "and it built the page, so its selectors are recorded"
            );
        }

        let list = bounds(cx, "custom-themes-rows".into());
        let rows_box = bounds(cx, "custom-themes-rows-box".into());
        assert_eq!(list_offset(cx, &app), Pixels::ZERO);
        assert!(
            built_strips(cx).is_none(),
            "on a boundary the room is the ring's"
        );

        // Ten pixels down, the first and the ninth row straddle the
        // viewport's ends, and the strips are exactly the room.
        scroll_rows(cx, &app, px(-10.));
        let first = bounds(cx, "custom-theme-1".into());
        let ninth = bounds(cx, "custom-theme-9".into());
        assert!(
            first.top() < rows_box.top() && first.bottom() > rows_box.top(),
            "the first row straddles the viewport's top: {first:?} in {rows_box:?}"
        );
        assert!(
            ninth.top() < rows_box.bottom() && ninth.bottom() > rows_box.bottom(),
            "the ninth row straddles the viewport's bottom: {ninth:?} in {rows_box:?}"
        );
        let (top, bottom) = built_strips(cx).expect("off a boundary the strips cover the room");
        assert_eq!(
            top,
            Bounds::new(list.origin, size(list.size.width, RING)),
            "the top strip is the room above the rows"
        );
        assert_eq!(
            bottom,
            Bounds::new(
                gpui::point(list.left(), rows_box.bottom()),
                size(list.size.width, RING)
            ),
            "the bottom strip is the room below them"
        );
        assert_eq!(
            top.bottom(),
            rows_box.top(),
            "and neither covers a row's box"
        );
        assert_eq!(bottom.bottom(), list.bottom());

        // Focusing the first row's Edit… while the row is partly hidden
        // reveals it: the list snaps to its top in this frame, and this
        // frame draws no strip over the ring.
        focus_action(cx, &app, 1, 0);
        assert_eq!(
            list_offset(cx, &app),
            Pixels::ZERO,
            "the reveal lands on a boundary"
        );
        assert!(
            strips(cx).is_none(),
            "and the revealing frame draws no strip"
        );
        let edit = bounds(cx, "edit-custom-theme-1".into());
        assert!(
            edit.top() - RING >= list.top() && edit.top() - RING < rows_box.top(),
            "the ring reaches into the room above: {edit:?} in {list:?}"
        );
        settle(cx);

        // A scroll away leaves the reveal alone and brings the strips back.
        scroll_rows(cx, &app, px(-10.));
        assert_eq!(
            list_offset(cx, &app),
            px(-10.),
            "the reveal is not repeated"
        );
        assert!(
            built_strips(cx).is_some(),
            "off a boundary again, the strips return"
        );
        assert_eq!(
            cx.update(|window, cx| focused_row(&app, window, cx)),
            Some((0, 0)),
            "with focus where it was"
        );

        // The ninth row's Delete… from here, the row drawn partly below the
        // viewport: the reveal lands its bottom on the viewport's, the next
        // boundary down, with no strip under the ring's bottom band.
        focus_action(cx, &app, 9, 2);
        assert_eq!(
            list_offset(cx, &app),
            -appearance::ui_size(30.),
            "the reveal lands the ninth row's bottom on the viewport's"
        );
        assert!(strips(cx).is_none(), "which is a boundary: no strip");
        let delete = bounds(cx, "delete-custom-theme-9".into());
        assert!(
            delete.bottom() + RING <= list.bottom() && delete.bottom() + RING > rows_box.bottom(),
            "the ring reaches into the room below: {delete:?} in {list:?}"
        );
        settle(cx);
        assert!(
            built_strips(cx).is_none(),
            "and none once the frame has settled"
        );
    }

    /// Every picker card draws its own theme's miniature.
    ///
    /// The miniatures are retained entities, so a card is paired with its body
    /// by a lookup and not by construction. `ThemeChoice::ALL` is in display
    /// order while `choice as usize` is the declaration order, and the two
    /// differ from the first card on: a lookup by one into a list built in the
    /// other drew nineteen of the twenty cards with another theme's body on
    /// their own canvas. Both halves are asserted from the rendered tree: the
    /// body a card embeds draws that card's palette, and that body is laid out
    /// inside that card.
    #[gpui::test]
    fn every_picker_card_draws_its_own_miniature(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let inside = |outer: Bounds<Pixels>, inner: Bounds<Pixels>| {
            inner.left() >= outer.left()
                && inner.right() <= outer.right()
                && inner.top() >= outer.top()
                && inner.bottom() <= outer.bottom()
        };
        for choice in ThemeChoice::ALL {
            let card = bounds(cx, format!("settings-theme-{}", choice as usize));
            // Exactly one retained body is laid out inside this card.
            let embedded: Vec<(Palette, usize)> = cx
                .read(|cx| {
                    app.read(cx)
                        .theme_previews
                        .iter()
                        .map(|(_, body)| {
                            let drawn = body.read(cx);
                            (body.entity_id(), drawn.palette(), drawn.renders())
                        })
                        .collect::<Vec<_>>()
                })
                .into_iter()
                .filter(|(id, ..)| {
                    let selector: &'static str = format!("theme-miniature-{id}").leak();
                    cx.debug_bounds(selector)
                        .is_some_and(|miniature| inside(card, miniature))
                })
                .map(|(_, palette, renders)| (palette, renders))
                .collect();
            assert_eq!(embedded.len(), 1, "{} holds one miniature", choice.label());
            let (palette, renders) = embedded[0];
            assert!(renders > 0, "{}'s miniature was drawn", choice.label());
            assert_eq!(
                palette.canvas,
                choice.palette().canvas,
                "{}'s miniature is drawn on its own canvas",
                choice.label()
            );
            assert_eq!(
                palette,
                choice.palette(),
                "{}'s miniature draws its own palette",
                choice.label()
            );
            // The lookup the picker uses agrees with what it rendered.
            assert_eq!(
                cx.read(|cx| {
                    app.read(cx)
                        .theme_preview_body(ThemeSelection::BuiltIn(choice))
                        .read(cx)
                        .palette()
                }),
                choice.palette()
            );
        }
    }

    /// A finished export, a finished import and a confirmed delete each reach
    /// the window by themselves, after the round trip through a native file
    /// dialog that deactivates and reactivates the window.
    ///
    /// Nothing here forces a draw once the dialog is answered: the preference
    /// executor is held until the window is active again and its activation
    /// frames are drawn, so the only thing left that can ask for a frame is the
    /// completion's own `cx.notify()`. The one-render work removed
    /// `Window::refresh` from `apply_appearance`, guards the preference-save
    /// notification and embeds cached miniatures; none of that may leave a
    /// completion without its frame, or the card stays busy on screen and its
    /// disabled actions swallow whatever the user does next.
    #[gpui::test]
    fn a_finished_transfer_and_a_confirmed_delete_draw_their_result(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        let theme = with_active_harbor(cx, &app);
        let fixture = tempfile::tempdir().unwrap();
        let destination = fixture.path().join("harbor.gitturtle-theme.json");
        // The native dialog takes the window's activation and gives it back.
        let answer_dialog = |cx: &mut VisualTestContext,
                             answer: &dyn Fn(&mut VisualTestContext)| {
            cx.deactivate_window();
            answer(cx);
            cx.update(|window, _| window.activate_window());
            cx.run_until_parked();
        };

        // Export.
        let hold = hold_preference_executor(cx, &app);
        click(cx, "export-custom-theme-7");
        assert!(!settings::page_shows(cx, "custom-themes-notice"));
        answer_dialog(cx, &|cx| {
            let destination = destination.clone();
            cx.simulate_new_path_selection(move |_| Some(destination));
        });
        assert!(
            transfer_result(cx, &app).2,
            "the write waits for the executor"
        );
        let _ = drawn(cx, &app);
        drop(hold);
        wait_without_drawing(cx, |cx| !transfer_result(cx, &app).2);
        let (notice, error, _) = transfer_result(cx, &app);
        assert_eq!(error, None);
        assert!(notice.is_some_and(|notice| notice.contains("Harbor")));
        assert!(
            !drawn(cx, &app).is_empty(),
            "the finished export draws the window"
        );
        assert!(
            settings::page_shows(cx, "custom-themes-notice"),
            "and that frame shows the export notice"
        );
        assert_eq!(std::fs::read(&destination).unwrap(), theme.to_document());

        // Import.
        let hold = hold_preference_executor(cx, &app);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.import_custom_theme(window, cx));
        });
        cx.run_until_parked();
        assert!(
            !settings::page_shows(cx, "custom-themes-notice"),
            "acting on the card again clears the finished export"
        );
        answer_dialog(cx, &|cx| {
            let chosen = destination.clone();
            cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
        });
        assert!(
            transfer_result(cx, &app).2,
            "the read waits for the executor"
        );
        let _ = drawn(cx, &app);
        drop(hold);
        wait_without_drawing(cx, |cx| {
            let (notice, error, pending) = transfer_result(cx, &app);
            !pending
                && (notice.is_some() || error.is_some())
                && !cx.read(|cx| app.read(cx).theme_editor.save_pending())
        });
        let (notice, error, _) = transfer_result(cx, &app);
        assert_eq!(error, None);
        assert!(notice.is_some_and(|notice| notice.contains("Harbor (imported)")));
        assert!(
            !drawn(cx, &app).is_empty(),
            "the finished import draws the window"
        );
        assert!(
            settings::page_shows(cx, "export-custom-theme-8"),
            "and that frame lists the imported theme"
        );
        assert!(settings::page_shows(cx, "custom-themes-notice"));

        // Delete, confirmed in the alert. Harbor is the selection, so the
        // confirmed removal also applies its base.
        let hold = hold_preference_executor(cx, &app);
        open_delete_alert(cx, &app);
        assert!(key_frame(cx, "tab", |window, cx| window.has_active_dialog(cx)));
        assert!(
            !key_frame(cx, "enter", |window, cx| window.has_active_dialog(cx)),
            "Return on Delete theme closes the alert"
        );
        assert!(
            cx.read(|cx| app.read(cx).theme_editor.save_pending()),
            "the removal waits for the store"
        );
        let _ = drawn(cx, &app);
        drop(hold);
        wait_without_drawing(cx, |cx| {
            cx.read(|cx| {
                let app = app.read(cx);
                !app.theme_editor.save_pending() && app.custom_themes.len() == 1
            })
        });
        let draws = drawn(cx, &app);
        assert!(!draws.is_empty(), "the confirmed delete draws the window");
        assert_eq!(
            draws.last().copied(),
            Some(theme.base.palette()),
            "in the deleted theme's base palette"
        );
        assert!(
            !settings::page_shows(cx, "export-custom-theme-7"),
            "and that frame no longer lists the deleted theme"
        );
        assert!(settings::page_shows(cx, "export-custom-theme-8"));
    }

    /// A keystroke in the theme editor's Name field and a caret blink in it
    /// each draw the window with the dialog built again and the Settings page
    /// behind it replayed from the previous frame: the page is a cached view
    /// (`settings::SettingsPage`) that nothing inside the dialog notifies.
    #[gpui::test]
    fn dialog_keystrokes_and_caret_blinks_reuse_the_settings_page(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        settle(cx);
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        mouse_away(cx);
        let name = cx.read(|cx| form.read(cx).name.clone());
        assert!(
            cx.update(|window, cx| name.read(cx).focus_handle(cx).is_focused(window)),
            "the editor opens with Name focused"
        );
        assert_eq!(
            page_builds_after(cx, &app, |_| {}),
            (0, 0),
            "nothing is pending before the keystroke"
        );

        let dialog = form_renders(cx, &form);
        let before = cx.read(|cx| name.read(cx).value().to_string());
        // The first key after pointer input switches the window's input
        // modality, and GPUI refreshes the whole window for that
        // (`Window::dispatch_event`); a warm-up key takes that frame.
        cx.simulate_input("w");
        settle(cx);
        let (built, drew) = page_builds_after(cx, &app, |cx| cx.simulate_input("x"));
        let after = cx.read(|cx| name.read(cx).value().to_string());
        assert!(
            after != before && after.contains('x'),
            "the key reached the Name field: {before:?} -> {after:?}"
        );
        assert!(drew >= 1, "the keystroke draws the window");
        assert!(
            form_renders(cx, &form) > dialog,
            "and builds the dialog again"
        );
        assert_eq!(built, 0, "while the Settings page behind it is replayed");

        let dialog = form_renders(cx, &form);
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(600));
        });
        assert!(drew >= 1, "the caret blink draws the window");
        assert!(
            form_renders(cx, &form) > dialog,
            "and builds the dialog again"
        );
        assert_eq!(built, 0, "while the Settings page behind it is replayed");
    }

    /// The token rows whose views were built while `act` ran.
    fn rows_built_after(
        cx: &mut VisualTestContext,
        form: &Entity<ThemeForm>,
        act: impl FnOnce(&mut VisualTestContext),
    ) -> Vec<TokenKind> {
        let renders = |cx: &mut VisualTestContext| -> Vec<(TokenKind, usize)> {
            cx.read(|cx| {
                form.read(cx)
                    .rows
                    .iter()
                    .map(|row| (row.kind, row.view.read(cx).renders))
                    .collect()
            })
        };
        let before = renders(cx);
        act(cx);
        cx.run_until_parked();
        renders(cx)
            .into_iter()
            .zip(before)
            .filter(|((_, after), (_, before))| after > before)
            .map(|((kind, _), _)| kind)
            .collect()
    }

    /// A keystroke in a hex field builds that token row again and replays the
    /// other twenty from the previous frame (`TokenRowView`): the field's
    /// notify from paint no longer dirties its row (`ViewNodeAnchor`), a key
    /// that leaves the row's key alone reaches the row through its observer
    /// of the field, a caret blink too, and the row's observer of its color
    /// picker builds only that row.
    #[gpui::test]
    fn a_hex_keystroke_builds_its_row_and_replays_the_others(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        settle(cx);
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        mouse_away(cx);
        let kind = TokenKind::ALL[4];
        let other = TokenKind::ALL[9];
        let hex = cx.read(|cx| form.read(cx).row(kind).hex.clone());
        let picker = cx.read(|cx| form.read(cx).row(other).picker.clone());
        cx.update(|window, cx| hex.read(cx).focus_handle(cx).focus(window, cx));
        settle(cx);
        // The first key switches the input modality, which refreshes the
        // window, and the second leaves the field's text invalid, which
        // changes the row's key; after those, keys leave the key alone.
        cx.simulate_input("6");
        settle(cx);
        cx.simulate_input("6");
        settle(cx);
        assert!(cx.read(|cx| form.read(cx).row(kind).invalid));

        let before = cx.read(|cx| hex.read(cx).value().to_string());
        let built = rows_built_after(cx, &form, |cx| cx.simulate_input("7"));
        let after = cx.read(|cx| hex.read(cx).value().to_string());
        assert!(
            after.len() == before.len() + 1 && after.contains("67"),
            "the key reached the field: {before:?} -> {after:?}"
        );
        assert_eq!(built, vec![kind], "the keystroke builds its row alone");

        let built = rows_built_after(cx, &form, |cx| {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(600));
        });
        assert_eq!(built, vec![kind], "a caret blink builds its row alone");

        let built = rows_built_after(cx, &form, |cx| {
            cx.update(|_, cx| picker.update(cx, |_, cx| cx.notify()));
        });
        assert_eq!(built, vec![other], "a picker change builds its row alone");

        let built = rows_built_after(cx, &form, draw_once);
        assert!(
            built.is_empty(),
            "an idle frame replays every row: {built:?}"
        );
    }

    /// Pointer hover, press and keyboard focus on a picker card, and the
    /// switch a card makes, each build the page again through the real input;
    /// the first save's status line builds it once more when the store
    /// answers, through the app's reconciler.
    #[gpui::test]
    fn input_on_a_picker_card_builds_the_page_again(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        settle(cx);
        let card = ThemeChoice::TokyoNight;
        let position = bounds(cx, format!("settings-theme-{}", card as usize)).center();
        let modifiers = Modifiers::default();

        let (built, _) = page_builds_after(cx, &app, |cx| {
            cx.simulate_mouse_move(position, None, modifiers)
        });
        assert!(
            built >= 1,
            "hovering a card builds the page again ({built})"
        );
        assert_eq!(
            page_builds_after(cx, &app, |cx| {
                cx.update(|_, cx| app.update(cx, |_, cx| cx.notify()))
            }),
            (0, 1),
            "a frame that moves nothing on the hovered page replays it"
        );

        let (built, _) = page_builds_after(cx, &app, |cx| {
            cx.simulate_event(MouseDownEvent {
                position,
                modifiers,
                button: MouseButton::Left,
                click_count: 1,
                first_mouse: false,
            })
        });
        assert!(
            built >= 1,
            "pressing a card builds the page again ({built})"
        );

        // The switch's save waits behind this, so the store's answer and the
        // status line it sets come in a frame of their own.
        let hold = hold_preference_executor(cx, &app);
        let (built, _) = page_builds_after(cx, &app, |cx| {
            cx.simulate_event(MouseUpEvent {
                position,
                modifiers,
                button: MouseButton::Left,
                click_count: 1,
            })
        });
        assert!(
            built >= 1,
            "a picker switch builds the page again ({built})"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.theme),
            ThemeSelection::BuiltIn(card)
        );
        let page = page_renders(cx, &app);
        assert_ne!(cx.read(|cx| app.read(cx).status.clone()), "Settings saved");
        drop(hold);
        drain_preference_writer(cx, &app);
        assert_eq!(cx.read(|cx| app.read(cx).status.clone()), "Settings saved");
        assert_eq!(
            page_renders(cx, &app),
            page + 1,
            "the first save's status line builds the page again"
        );

        // Keyboard focus on another card builds the page (its focus ring).
        // Focus moves through `Window::focus`, where Tab and a press end.
        let target = ThemeChoice::CatppuccinMocha;
        assert_ne!(target, card);
        let handle = cx.read(|cx| {
            app.read(cx)
                .theme_card_focus(ThemeSelection::BuiltIn(target))
                .clone()
        });
        let (built, _) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| window.focus(&handle, cx))
        });
        assert!(
            built >= 1,
            "moving keyboard focus onto a card builds the page again ({built})"
        );
        assert!(cx.update(|window, _| handle.is_focused(window)));
    }

    /// A hovered card's preview border is drawn over its miniature, as when
    /// the miniature was inline: the card layer paints each card's miniature,
    /// caption fill and cached body, then the hover ring last, from the
    /// pointer state the page's probe read in the same frame. The caption's
    /// fill follows hover and press as its group styles did. Pressing keeps
    /// the card hovered, the card stays ringed once selected, and leaving it
    /// drops the ring while its miniature stays in the layer.
    #[gpui::test]
    fn a_hovered_card_draws_its_miniature_under_its_border(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        settle(cx);
        let choice = ThemeChoice::TokyoNight;
        let card = ThemeSelection::BuiltIn(choice);
        let p = choice.palette();
        let (body, slots) = cx.read(|cx| {
            let app = app.read(cx);
            (
                app.theme_preview_body(card).entity_id(),
                app.picker_cards.clone(),
            )
        });
        use settings::CardPart::{Body, Fill, Miniature, Ring};
        assert!(slots.len() >= ThemeChoice::ALL.len());
        assert_eq!(
            slots.painted(body),
            [Miniature, Fill(p.panel), Body],
            "no card is hovered yet"
        );
        let position = bounds(cx, format!("settings-theme-{}", choice as usize)).center();
        let modifiers = Modifiers::default();

        cx.simulate_mouse_move(position, None, modifiers);
        settle(cx);
        assert_eq!(
            slots.painted(body),
            [Miniature, Fill(p.hover), Body, Ring],
            "the hovered card's ring, over its miniature"
        );

        cx.simulate_event(MouseDownEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        settle(cx);
        assert_eq!(
            slots.painted(body),
            [Miniature, Fill(p.selected), Body, Ring],
            "a pressed card is hovered"
        );

        cx.simulate_event(MouseUpEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
        });
        settle(cx);
        assert!(card_checked(cx, &app, card), "the click selected the card");
        settle(cx);
        assert_eq!(
            slots.painted(body),
            [Miniature, Fill(p.hover), Body, Ring],
            "a selected, hovered card"
        );

        mouse_away(cx);
        assert_eq!(
            slots.painted(body),
            [Miniature, Fill(p.panel), Body],
            "no card is hovered"
        );
    }

    /// Builds of each picker card's cached body so far.
    fn card_body_renders(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
    ) -> Vec<(ThemeSelection, usize)> {
        cx.read(|cx| {
            app.read(cx)
                .theme_card_bodies
                .iter()
                .map(|(selection, body)| (*selection, body.read(cx).renders()))
                .collect()
        })
    }

    /// Draw single frames, with no work run between them, until one has
    /// built the Settings page since `page` builds: the frame that shows a
    /// change the page follows. The test app also draws a dirty window when
    /// it flushes an update's effects, which may be that frame.
    fn draw_until_page_built(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, page: usize) {
        for _ in 0..3 {
            if page_renders(cx, app) > page {
                return;
            }
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        assert!(page_renders(cx, app) > page, "a frame builds the page");
    }

    /// The cards whose bodies were built between two counts.
    fn bodies_built(
        before: &[(ThemeSelection, usize)],
        after: &[(ThemeSelection, usize)],
    ) -> Vec<ThemeSelection> {
        after
            .iter()
            .filter(|(selection, renders)| {
                before
                    .iter()
                    .find(|(drawn, _)| drawn == selection)
                    .is_none_or(|(_, was)| was != renders)
            })
            .map(|(selection, _)| *selection)
            .collect()
    }

    /// A live-preview edit builds the cached page again but replays every
    /// picker card body whose key it left alone (`settings::CardKey`, set by
    /// the app's self-observer). An edit of the canvas changes no card's key;
    /// an edit of the accent changes the key of the one card whose check
    /// badge draws it, which is built again in the edit's own frame.
    #[gpui::test]
    fn a_live_preview_edit_replays_the_card_bodies_it_leaves_alone(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        settle(cx);
        let selected = cx
            .read(|cx| app.read(cx).selected_theme_card())
            .expect("a card is selected");
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        mouse_away(cx);
        let inputs = cx.read(|cx| {
            [TokenKind::Canvas, TokenKind::Accent].map(|kind| form.read(cx).row(kind).hex.clone())
        });
        for (input, value, built) in [
            (&inputs[0], "#101a26", vec![]),
            (&inputs[1], "#ff8800", vec![selected]),
        ] {
            settle(cx);
            let applied = applications(cx, &app);
            let bodies = card_body_renders(cx, &app);
            let page = page_renders(cx, &app);
            cx.update(|window, cx| {
                input.update(cx, |input, cx| input.replace_all(value, window, cx))
            });
            cx.update(|window, cx| window.simulate_next_frame(cx));
            draw_until_page_built(cx, &app, page);
            assert_eq!(applications(cx, &app), applied + 1, "{value} is previewed");
            assert_eq!(
                bodies_built(&bodies, &card_body_renders(cx, &app)),
                built,
                "{value}'s frame builds the card bodies whose key it changed, and no other"
            );
        }
    }

    /// A picker switch moves the check badge: the app's self-observer sets
    /// the new keys before the switch's frame, which builds the two bodies
    /// whose badge changed and replays every other card body.
    #[gpui::test]
    fn a_switch_builds_the_two_card_bodies_whose_badge_moved(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        settle(cx);
        let from = cx
            .read(|cx| app.read(cx).selected_theme_card())
            .expect("a card is selected");
        let to = ThemeChoice::ALL
            .into_iter()
            .map(ThemeSelection::BuiltIn)
            .find(|choice| *choice != from)
            .expect("another built-in theme");
        mouse_away(cx);
        let bodies = card_body_renders(cx, &app);
        let page = page_renders(cx, &app);
        cx.update(|window, cx| app.update(cx, |app, cx| app.choose_theme(to, window, cx)));
        draw_until_page_built(cx, &app, page);
        let mut built = bodies_built(&bodies, &card_body_renders(cx, &app));
        built.sort_by_key(|selection| format!("{selection:?}"));
        let mut expected = vec![from, to];
        expected.sort_by_key(|selection| format!("{selection:?}"));
        assert_eq!(
            built, expected,
            "the switch's frame builds the two cards whose badge moved"
        );
        assert!(card_checked(cx, &app, to));
        assert!(!card_checked(cx, &app, from));
    }

    /// The app's own sites, none of which refreshes the window: a settings
    /// change and a save refused by validation (`save_preferences`) and a
    /// palette application (`apply_appearance`, here a live-preview edit)
    /// each build the page once in their one frame, and the edit still reuses
    /// every picker miniature. A desktop text-scale change refreshes the
    /// window (`appearance::apply_text_sizes`) and builds the page once too.
    #[gpui::test]
    fn settings_text_scale_and_palette_changes_build_the_page_once(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        // The first save's reply sets the status line, which builds the page
        // in a frame of its own (`input_on_a_picker_card_builds_the_page_again`);
        // take it before measuring.
        cx.update(|window, cx| app.update(cx, |app, cx| app.save_preferences(window, cx)));
        drain_preference_writer(cx, &app);
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.settings.graph_spacing += 2;
                    app.save_preferences(window, cx);
                })
            })
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "a settings change builds the page once in its frame"
        );
        drain_preference_writer(cx, &app);

        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.settings.default_branch.clear();
                    app.save_preferences(window, cx);
                })
            })
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "a save refused by validation builds the page once, with its error"
        );
        assert!(cx.read(|cx| {
            app.read(cx)
                .operation_error
                .as_deref()
                .is_some_and(|error| error.starts_with("Could not save settings:"))
        }));
        cx.update(|_, cx| app.update(cx, |app, _| app.settings.default_branch = "main".into()));

        // As `appearance::set_desktop_text_scale` does: the new factor, then
        // the text sizes re-applied to the window, which refreshes it.
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| {
                cx.set_global(appearance::DesktopTextScale(1.25));
                let settings = app.read(cx).settings.clone();
                appearance::apply_text_sizes(
                    settings.interface_text_size,
                    settings.code_text_size,
                    window,
                    cx,
                );
            })
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "a desktop text-scale change builds the page once"
        );

        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        mouse_away(cx);
        let hex = cx.read(|cx| form.read(cx).row(TokenKind::Canvas).hex.clone());
        let previews = preview_renders(cx, &app);
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| {
                hex.update(cx, |input, cx| input.replace_all("#101a26", window, cx))
            })
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "a live-preview edit builds the page once in its one frame"
        );
        assert_eq!(
            preview_renders(cx, &app),
            previews,
            "and that frame still reuses every picker miniature"
        );
    }

    /// The Your themes card's sites, each held apart from any refresh by
    /// occupying the preference executor: an export's start and its result,
    /// an import's start, its parsed document and the store's answer, a save
    /// submitted from the editor, the bound error on New theme…, and the
    /// store's answer to a confirmed delete.
    #[gpui::test]
    fn the_your_themes_card_builds_the_page_once_at_each_step(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        with_active_harbor(cx, &app);
        let fixture = tempfile::tempdir().unwrap();
        let exported = fixture.path().join("harbor.gitturtle-theme.json");

        // Export.
        let hold = hold_preference_executor(cx, &app);
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.export_custom_theme(7, window, cx)))
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "starting an export builds the busy card once"
        );
        cx.simulate_new_path_selection({
            let exported = exported.clone();
            move |_| Some(exported)
        });
        settle(cx);
        let page = page_renders(cx, &app);
        drop(hold);
        wait_without_drawing(cx, |cx| !transfer_result(cx, &app).2);
        assert_eq!(
            page_renders(cx, &app),
            page + 1,
            "the finished export builds the page once"
        );
        assert!(
            settings::page_shows(cx, "custom-themes-notice"),
            "and that frame shows its notice"
        );

        // Import: the read job queues behind `hold`, and `hold_save`, queued
        // after it, holds the save the parsed document submits.
        let hold = hold_preference_executor(cx, &app);
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.import_custom_theme(window, cx)))
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "starting an import builds the busy card once"
        );
        assert!(
            !settings::page_shows(cx, "custom-themes-notice"),
            "acting on the card again clears the export notice"
        );
        cx.simulate_path_prompt_response({
            let chosen = exported.clone();
            move |_| Some(vec![chosen])
        });
        cx.run_until_parked();
        let hold_save = hold_preference_executor(cx, &app);
        let page = page_renders(cx, &app);
        drop(hold);
        wait_without_drawing(cx, |cx| !transfer_result(cx, &app).2);
        assert!(
            cx.read(|cx| app.read(cx).theme_editor.save_pending()),
            "the parsed document's save waits for the store"
        );
        assert_eq!(
            page_renders(cx, &app),
            page + 1,
            "the parsed document builds the page once"
        );
        let page = page_renders(cx, &app);
        drop(hold_save);
        wait_without_drawing(cx, |cx| {
            !cx.read(|cx| app.read(cx).theme_editor.save_pending())
        });
        assert_eq!(
            page_renders(cx, &app),
            page + 1,
            "the store's answer builds the page once"
        );
        assert!(
            settings::page_shows(cx, "export-custom-theme-8"),
            "and that frame lists the imported theme"
        );

        // A save submitted from the editor.
        click(cx, "custom-themes-new");
        let form = form(cx, &app);
        next_frame(cx);
        mouse_away(cx);
        let hold = hold_preference_executor(cx, &app);
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| form.update(cx, |form, cx| form.submit(window, cx)))
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "a submitted save builds the busy card once"
        );
        assert!(cx.read(|cx| app.read(cx).theme_editor.save_pending()));
        drop(hold);
        wait_for(cx, |cx| {
            !cx.read(|cx| app.read(cx).theme_editor.save_pending())
        });
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "the saved theme closes the editor"
        );

        // A confirmed delete of the first row, Harbor. The alert's close
        // restores focus over the next frames and a 300 ms recheck; those pass
        // before the store answers.
        let rows = cx.read(|cx| app.read(cx).custom_themes.len());
        let hold = hold_preference_executor(cx, &app);
        open_delete_alert(cx, &app);
        assert!(key_frame(cx, "tab", |window, cx| window.has_active_dialog(cx)));
        assert!(
            !key_frame(cx, "enter", |window, cx| window.has_active_dialog(cx)),
            "Return on Delete theme closes the alert"
        );
        assert!(cx.read(|cx| app.read(cx).theme_editor.save_pending()));
        native_frame(cx);
        native_frame(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        settle(cx);
        let page = page_renders(cx, &app);
        drop(hold);
        wait_without_drawing(cx, |cx| {
            !cx.read(|cx| app.read(cx).theme_editor.save_pending())
        });
        assert_eq!(cx.read(|cx| app.read(cx).custom_themes.len()), rows - 1);
        assert_eq!(
            page_renders(cx, &app),
            page + 1,
            "the confirmed delete's answer builds the page once"
        );
        assert!(
            !settings::page_shows(cx, "export-custom-theme-7")
                && settings::page_shows(cx, "export-custom-theme-8"),
            "and that frame lists the rows without the deleted one"
        );

        // New theme… at the bound.
        seed_themes(cx, &app, MAX_CUSTOM_THEMES as u32);
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.open_theme_editor(None, window, cx))
            })
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "the bound error builds the page once"
        );
        assert!(cx.read(|cx| app.read(cx).theme_editor.error.is_some()));
    }

    /// An import the user cancels in the file prompt builds the page at its
    /// start and again at its end, which re-enables the card. The end has
    /// no other notify of the page: a cancelled import adds no theme and saves
    /// nothing, so only `import_custom_theme`'s completion can build it.
    #[gpui::test]
    fn a_cancelled_import_builds_the_page_when_it_ends(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.import_custom_theme(window, cx)))
        });
        assert_eq!(
            (built, drew),
            (1, 1),
            "the import's start builds the page once"
        );
        assert!(
            transfer_result(cx, &app).2,
            "the card is busy while the prompt is open"
        );
        let (built, drew) =
            page_builds_after(cx, &app, |cx| cx.simulate_path_prompt_response(|_| None));
        assert!(!transfer_result(cx, &app).2, "the cancelled import ended");
        assert_eq!(
            (built, drew),
            (1, 1),
            "and its end builds the page once, so the card is enabled again"
        );
    }

    /// The page's own text inputs reach it through its observers
    /// (`SettingsPage::new`): typing in Default branch and a caret blink there
    /// each build the page, since the input's view node is registered outside
    /// the page (`ViewNodeAnchor`) and its notify dirties only the input, the
    /// app and the root.
    #[gpui::test]
    fn typing_and_a_caret_blink_in_a_settings_input_build_the_page(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.update(|window, _| window.activate_window());
        let input = cx.read(|cx| app.read(cx).settings_branch.clone());
        cx.update(|window, cx| input.read(cx).focus_handle(cx).focus(window, cx));
        settle(cx);
        // The first key after pointer input refreshes the whole window
        // (`Window::dispatch_event`'s input-modality switch).
        cx.simulate_input("x");
        settle(cx);
        let before = cx.read(|cx| input.read(cx).value().to_string());
        let (built, drew) = page_builds_after(cx, &app, |cx| cx.simulate_input("y"));
        let after = cx.read(|cx| input.read(cx).value().to_string());
        assert!(
            after != before && after.contains('y'),
            "the key reached Default branch: {before:?} -> {after:?}"
        );
        assert!(drew >= 1, "the keystroke draws the window");
        assert!(
            built >= 1,
            "and builds the page, which shows the value ({built})"
        );

        let (built, drew) = page_builds_after(cx, &app, |cx| {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(600));
        });
        assert!(drew >= 1, "the caret blink draws the window");
        assert!(
            built >= 1,
            "and builds the page, which shows the caret ({built})"
        );
    }

    /// State the page shows but other features change reaches it through the
    /// app's self-observer (`GitTurtle::reconcile_settings_page`): the
    /// operation error and busy state, the effective identity and the open
    /// repository each build the page once when the app is notified, and a
    /// notification that moved none of them replays it.
    #[gpui::test]
    fn state_from_other_features_builds_the_page_through_the_reconciler(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        fn notify(
            cx: &mut VisualTestContext,
            app: &Entity<GitTurtle>,
            change: fn(&mut GitTurtle),
        ) -> (usize, usize) {
            page_builds_after(cx, app, |cx| {
                cx.update(|_, cx| {
                    app.update(cx, |app, cx| {
                        change(app);
                        cx.notify();
                    })
                })
            })
        }
        assert_eq!(
            notify(cx, &app, |_| {}),
            (0, 1),
            "a notification that moves nothing the page shows draws the window with the page replayed"
        );
        assert_eq!(
            notify(cx, &app, |app| {
                app.operation_error = Some("Default branch: refused".into())
            }),
            (1, 1),
            "an operation error builds the page once"
        );
        assert_eq!(
            notify(cx, &app, |app| {
                app.operation_busy = Some("Saving repository identity…")
            }),
            (1, 1),
            "a busy operation builds the page once"
        );
        assert_eq!(notify(cx, &app, |app| app.operation_busy = None), (1, 1));
        assert_eq!(
            notify(cx, &app, |app| {
                app.profile = Some(gitturtle_core::GitProfile {
                    signing: true,
                    ..Default::default()
                })
            }),
            (1, 1),
            "a resolved identity builds the page once"
        );
        let repository = repository_fixture();
        let opened = GitRepository::open(repository.path()).unwrap();
        assert_eq!(
            page_builds_after(cx, &app, |cx| {
                cx.update(|_, cx| {
                    app.update(cx, |app, cx| {
                        app.repository = Some(opened);
                        cx.notify();
                    })
                })
            }),
            (1, 1),
            "an opened repository builds the page once"
        );
        assert_eq!(
            notify(cx, &app, |_| {}),
            (0, 1),
            "and at rest the page is replayed again"
        );
    }

    /// Saved custom themes are a third picker group of the same cards: named
    /// "<name> theme", described by their base, marked when selected, warned
    /// when their palette has readability findings, and chosen by a click
    /// that applies the palette and saves the selection. Without a saved
    /// theme there is no third group.
    #[gpui::test]
    fn custom_themes_form_a_third_picker_group(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let initial = cx.read(|cx| app.read(cx).settings.theme);
        assert!(settings::page_shows(cx, "settings-theme-group-light"));
        assert!(settings::page_shows(cx, "settings-theme-group-dark"));
        assert!(
            !settings::page_shows(cx, "settings-theme-group-custom"),
            "no third group without a custom theme"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).card_names.borrow().len()),
            ThemeChoice::ALL.len()
        );
        assert!(card_checked(cx, &app, initial));

        let harbor = CustomTheme::from_base(7, "Harbor", ThemeChoice::Nord);
        let mut dawn = CustomTheme::from_base(9, "Dawn Chorus", ThemeChoice::RosePineDawn);
        // Muted text on the canvas at 1:1 fails the body-text rule.
        dawn.palette.muted = dawn.palette.canvas;
        assert!(!dawn.palette.readability_issues().is_empty());
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.set_custom_themes(vec![harbor.clone(), dawn.clone()], cx)
            })
        });
        settle(cx);

        let group = bounds(cx, "settings-theme-group-custom".into());
        let dark = bounds(cx, "settings-theme-group-dark".into());
        assert!(
            group.top() > dark.top(),
            "Your themes follows the built-in groups"
        );
        let harbor_card = bounds(cx, "settings-custom-theme-7".into());
        let dawn_card = bounds(cx, "settings-custom-theme-9".into());
        assert!(harbor_card.top() > group.top());
        assert_eq!(
            dawn_card.top(),
            harbor_card.top(),
            "two cards share the group's first row"
        );
        assert!(dawn_card.left() > harbor_card.right());

        // Every card is named "<name> theme" for assistive technology.
        let names = cx.read(|cx| app.read(cx).card_names.borrow().clone());
        assert_eq!(names.len(), ThemeChoice::ALL.len() + 2);
        let nord: &'static str = format!("settings-theme-{}", ThemeChoice::Nord as usize).leak();
        for (selector, name) in [
            ("settings-custom-theme-7", "Harbor theme"),
            ("settings-custom-theme-9", "Dawn Chorus theme"),
            (nord, "Nord theme"),
        ] {
            assert!(
                names.contains(&(selector.into(), name.into())),
                "{selector} is named {name:?}: {names:?}"
            );
        }

        // Each custom card holds its own retained miniature, drawing its palette.
        for theme in [&harbor, &dawn] {
            let card = bounds(cx, format!("settings-custom-theme-{}", theme.id));
            let (miniature, palette) = cx.read(|cx| {
                let body = app
                    .read(cx)
                    .theme_preview_body(ThemeSelection::Custom(theme.id));
                (body.entity_id(), body.read(cx).palette())
            });
            assert_eq!(palette, theme.palette, "{}'s miniature", theme.name);
            let inside = bounds(cx, format!("theme-miniature-{miniature}"));
            assert!(
                inside.top() >= card.top()
                    && inside.bottom() <= card.bottom()
                    && inside.left() >= card.left()
                    && inside.right() <= card.right(),
                "{}'s miniature is inside its card: {inside:?} in {card:?}",
                theme.name
            );
        }
        assert!(!card_warned(cx, &app, ThemeSelection::Custom(7)));
        assert!(
            card_warned(cx, &app, ThemeSelection::Custom(9)),
            "Dawn Chorus shows the readability glyph"
        );
        assert!(card_checked(cx, &app, initial));
        assert!(!card_checked(cx, &app, ThemeSelection::Custom(7)));

        // Clicking Harbor applies its palette, marks its card and saves the
        // selection; it submits no read.
        let idle = submissions(cx, &app);
        cx.simulate_click(harbor_card.center(), Modifiers::default());
        settle(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.theme),
            ThemeSelection::Custom(7)
        );
        assert_eq!(
            applied(cx),
            harbor.palette,
            "the click applied the custom palette"
        );
        assert!(card_checked(cx, &app, ThemeSelection::Custom(7)));
        assert!(!card_checked(cx, &app, initial));
        assert_eq!(
            submissions(cx, &app),
            idle,
            "a custom switch submitted a read"
        );
        drain_preference_writer(cx, &app);
        assert_eq!(
            Preferences::load().settings.theme,
            ThemeSelection::Custom(7),
            "the selection reached the store"
        );

        // Following the system marks no card and keeps the selection.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.follow_system = true;
                app.apply_appearance(window, cx);
            })
        });
        settle(cx);
        assert!(!card_checked(cx, &app, ThemeSelection::Custom(7)));
        assert_eq!(
            cx.read(|cx| app.read(cx).settings.theme),
            ThemeSelection::Custom(7)
        );

        // A deleted theme loses its card and its miniature; the group goes
        // with the last one.
        cx.update(|_, cx| app.update(cx, |app, cx| app.set_custom_themes(vec![dawn.clone()], cx)));
        settle(cx);
        assert!(!settings::page_shows(cx, "settings-custom-theme-7"));
        assert!(settings::page_shows(cx, "settings-custom-theme-9"));
        assert_eq!(
            cx.read(|cx| app.read(cx).theme_previews.len()),
            ThemeChoice::ALL.len() + 1
        );
        cx.update(|_, cx| app.update(cx, |app, cx| app.set_custom_themes(Vec::new(), cx)));
        settle(cx);
        assert!(!settings::page_shows(cx, "settings-theme-group-custom"));
        assert_eq!(
            cx.read(|cx| app.read(cx).theme_previews.len()),
            ThemeChoice::ALL.len()
        );
    }

    /// The Your themes card names its actions for assistive technology: New
    /// theme… by its own label, Import… by the file it asks for, and each
    /// row's actions by the theme they act on.
    #[gpui::test]
    fn your_themes_actions_carry_their_accessible_names(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let harbor = CustomTheme::from_base(7, "Harbor", ThemeChoice::Nord);
        cx.update(|_, cx| app.update(cx, |app, cx| app.set_custom_themes(vec![harbor], cx)));
        settle(cx);
        let expected = [
            ("custom-themes-new", "New theme…"),
            ("custom-themes-import", "Import a theme file"),
            ("edit-custom-theme-7", "Edit Harbor theme"),
            ("export-custom-theme-7", "Export Harbor theme"),
            ("delete-custom-theme-7", "Delete Harbor theme"),
        ];
        for (selector, _) in expected {
            assert!(settings::page_shows(cx, selector), "{selector} is drawn");
        }
        let names: std::collections::HashSet<(String, String)> = cx
            .read(|cx| app.read(cx).theme_action_names.borrow().clone())
            .into_iter()
            .collect();
        assert_eq!(
            names,
            expected
                .map(|(selector, name)| (selector.to_owned(), name.to_owned()))
                .into()
        );
    }

    /// Cards are 132 px tall in a four-column grid at and above 1,060 px,
    /// three columns in the compact layout and two in the narrow layout; the
    /// twenty built-ins take five rows at 1,440 × 900. The custom group starts
    /// its rows on the same grid.
    #[gpui::test]
    fn picker_cards_keep_the_grid_geometry(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        seed_themes(cx, &app, 2);
        let light = ThemeChoice::ALL
            .iter()
            .filter(|choice| choice.is_light())
            .count();
        let dark = ThemeChoice::ALL.len() - light;
        let column = |card: &Bounds<Pixels>| f32::from(card.left()).round() as i64;
        for (width, height, columns) in [
            (1440., 900., 4),
            (1060., 900., 4),
            (1000., 680., 3),
            (700., 680., 2),
        ] {
            cx.simulate_resize(size(px(width), px(height)));
            settle(cx);
            let built_in: Vec<Bounds<Pixels>> = ThemeChoice::ALL
                .into_iter()
                .map(|choice| bounds(cx, format!("settings-theme-{}", choice as usize)))
                .collect();
            let custom: Vec<Bounds<Pixels>> = [1, 2]
                .into_iter()
                .map(|id| bounds(cx, format!("settings-custom-theme-{id}")))
                .collect();
            for card in built_in.iter().chain(&custom) {
                assert_eq!(card.size.height, px(132.), "at {width} px: {card:?}");
            }
            let lefts: std::collections::BTreeSet<i64> = built_in.iter().map(column).collect();
            assert_eq!(
                lefts.len(),
                columns,
                "at {width} px the grid has {columns} columns: {lefts:?}"
            );
            let tops: std::collections::BTreeSet<i64> = built_in
                .iter()
                .map(|card| f32::from(card.top()).round() as i64)
                .collect();
            assert_eq!(
                tops.len(),
                light.div_ceil(columns) + dark.div_ceil(columns),
                "at {width} px the built-ins fill their rows: {tops:?}"
            );
            if width == 1440. {
                assert!(
                    tops.len() <= 5,
                    "twenty built-ins take at most five rows at 1,440 × 900"
                );
            }
            assert_eq!(column(&custom[0]), *lefts.iter().next().unwrap());
            assert_eq!(column(&custom[1]), *lefts.iter().nth(1).unwrap());
            let last_built_in =
                built_in
                    .iter()
                    .map(|card| card.bottom())
                    .fold(
                        px(0.),
                        |top, bottom| if bottom > top { bottom } else { top },
                    );
            assert!(custom[0].top() > last_built_in);
        }
    }

    /// The miniature `selection`'s picker card embeds, for its selectors.
    fn card_miniature(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        selection: ThemeSelection,
    ) -> gpui::EntityId {
        cx.read(|cx| app.read(cx).theme_preview_body(selection).entity_id())
    }

    /// The caption is at least 54 px, not exactly: at interface text sizes
    /// 12 and 11 its fixed padding, gaps, badge and swatches would squeeze the
    /// truncating name and description below their line height and cut their
    /// glyphs. The caption grows there and the miniature gives up the
    /// difference; at the default size the card keeps its 70 px miniature.
    #[gpui::test]
    fn the_caption_keeps_its_text_lines_at_small_interface_sizes(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        cx.simulate_resize(size(px(1440.), px(900.)));
        let mut dawn = CustomTheme::from_base(9, "Dawn Chorus", ThemeChoice::RosePineDawn);
        dawn.palette.muted = dawn.palette.canvas;
        assert!(!dawn.palette.readability_issues().is_empty());
        let plain = ThemeSelection::BuiltIn(ThemeChoice::Nord);
        let code = cx.read(|cx| app.read(cx).settings.code_text_size);
        // Dawn Chorus is selected and warned: its name row holds the check
        // badge and the warning glyph, the tallest caption a card draws.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.set_custom_themes(vec![dawn.clone()], cx);
                app.settings.theme = ThemeSelection::Custom(9);
                app.apply_appearance(window, cx);
            })
        });
        settle(cx);
        assert!(card_checked(cx, &app, ThemeSelection::Custom(9)));
        assert!(card_warned(cx, &app, ThemeSelection::Custom(9)));
        assert!(!card_checked(cx, &app, plain) && !card_warned(cx, &app, plain));
        for interface in [13, 12, 11] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.settings.interface_text_size = interface;
                    appearance::apply_text_sizes(interface, code, window, cx);
                    cx.notify();
                })
            });
            settle(cx);
            assert!(
                (f32::from(appearance::ui_text(13.)) - f32::from(interface)).abs() < 0.01,
                "interface text is {interface} px"
            );
            // Layout snaps each edge to a device pixel, so a whole line may
            // lose at most one of them.
            let device = cx.update(|window, _| px(1. / window.scale_factor()));
            let name_line = appearance::ui_text(12.) * 1.3 - device;
            let description_line = appearance::ui_text(10.) * 1.3 - device;
            for selection in [ThemeSelection::Custom(9), plain] {
                let miniature = card_miniature(cx, &app, selection);
                let card = bounds(
                    cx,
                    match selection {
                        ThemeSelection::Custom(id) => format!("settings-custom-theme-{id}"),
                        ThemeSelection::BuiltIn(choice) => {
                            format!("settings-theme-{}", choice as usize)
                        }
                    },
                );
                let name = bounds(cx, format!("theme-name-{miniature}"));
                let description = bounds(cx, format!("theme-description-{miniature}"));
                assert!(
                    name.size.height >= name_line,
                    "at interface size {interface} {selection:?}'s name keeps its \
                     {name_line:?} line: {name:?}"
                );
                assert!(
                    description.size.height >= description_line,
                    "at interface size {interface} {selection:?}'s description keeps \
                     its {description_line:?} line: {description:?}"
                );
                assert!(
                    description.top() >= name.bottom() && description.bottom() < card.bottom(),
                    "at interface size {interface} {selection:?}'s lines stack inside \
                     the card: {name:?}, {description:?} in {card:?}"
                );
                let body = bounds(cx, format!("theme-miniature-{miniature}"));
                assert!(body.bottom() <= name.top());
                if interface == appearance::DEFAULT_INTERFACE_TEXT_SIZE {
                    assert_eq!(card.size.height, px(132.));
                    assert_eq!(
                        body.size.height,
                        px(70.),
                        "the default caption stays 54 px under a 70 px miniature"
                    );
                }
            }
        }
    }

    /// The warning glyph is a status mark, drawn in the active palette as the
    /// check badge is: a saved theme whose warning color equals its own panel
    /// would otherwise draw an invisible disc on its caption.
    #[gpui::test]
    fn the_picker_warning_glyph_is_drawn_in_the_active_palette(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        let mut lantern = CustomTheme::from_base(5, "Lantern", ThemeChoice::Nord);
        lantern.palette.warning = lantern.palette.panel;
        assert!(
            !lantern.palette.readability_issues().is_empty(),
            "a warning on its own panel fails the surface rule"
        );
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.set_custom_themes(vec![lantern.clone()], cx);
                cx.notify();
            })
        });
        settle(cx);
        let active = applied(cx);
        assert_ne!(active, lantern.palette, "Lantern is not the applied theme");
        assert_ne!(active.warning, lantern.palette.warning);
        assert!(card_warned(cx, &app, ThemeSelection::Custom(5)));
        let miniature = card_miniature(cx, &app, ThemeSelection::Custom(5));
        let glyph = |cx: &mut VisualTestContext, p: Palette| {
            let selector: &'static str = format!(
                "theme-warning-glyph-{miniature}-{:06x}-{:06x}",
                p.warning, p.canvas
            )
            .leak();
            settings::shown(cx, selector)
        };
        assert!(
            glyph(cx, active).is_some(),
            "the glyph's disc and mark are the active warning and canvas"
        );
        assert!(
            glyph(cx, lantern.palette).is_none(),
            "the glyph is not drawn in the palette it flags"
        );
    }

    /// The editor previews its draft on the settings card: the same 132 px
    /// card around the same 70 px miniature as a picker card.
    #[gpui::test]
    fn the_editor_preview_card_matches_a_picker_card(cx: &mut TestAppContext) {
        let (app, cx) = open_app(cx);
        for viewport in [size(px(1000.), px(680.)), size(px(1440.), px(900.))] {
            cx.simulate_resize(viewport);
            settle(cx);
            let choice = ThemeChoice::Nord;
            let card = bounds(cx, format!("settings-theme-{}", choice as usize));
            let miniature = card_miniature(cx, &app, ThemeSelection::BuiltIn(choice));
            let card_body = bounds(cx, format!("theme-miniature-{miniature}"));
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.open_theme_editor(None, window, cx))
            });
            settle(cx);
            let form = form(cx, &app);
            next_frame(cx);
            let preview = bounds(cx, "theme-editor-preview".into());
            let draft = cx.read(|cx| form.read(cx).preview_body.entity_id());
            let preview_body = bounds(cx, format!("theme-miniature-{draft}"));
            assert_eq!(
                preview.size.height, card.size.height,
                "at {viewport:?} the editor's preview card is a picker card's height"
            );
            assert_eq!(
                preview_body.size.height, card_body.size.height,
                "at {viewport:?} the editor's miniature is a picker miniature's height"
            );
            assert_eq!(preview.size.height, px(132.));
            cx.update(|window, cx| app.update(cx, |app, cx| app.close_theme_editor(window, cx)));
            settle(cx);
        }
    }
}
