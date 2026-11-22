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

use crate::focus_reveal::{FocusReveal, Trigger};
use crate::*;
use appearance::custom::{
    self, CustomTheme, ImportedTheme, ReadabilityIssue, ThemeSelection, TokenGroup, TokenKind,
};
use appearance::{Palette, ThemeChoice};
use gpui_kit::base::{FocusableExt, Scrollbar, ScrollbarMode};
use gpui_kit::component::{
    WindowExt,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    dialog::Confirm,
    menu::PopupMenuItem,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder;
use preferences::MAX_CUSTOM_THEMES;

/// The Settings page's scroll, which reveals a control only when the
/// keyboard moves focus onto it: a click on a partly visible control scrolls
/// nothing.
struct SettingsPage(FocusReveal);

impl Default for SettingsPage {
    fn default() -> Self {
        Self(FocusReveal::new(Trigger::Keyboard))
    }
}

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
    /// What the open editor was opened from: New theme… (`Some(None)`) or
    /// the edited theme's Edit… (`Some(Some(id))`). The editor's close gives
    /// focus back to that control and brings it into view
    /// (`GitTurtle::reveal_theme_editor_opener`), except a new theme's Save,
    /// which gives it to the saved theme's Edit….
    opened_from: Option<Option<u32>>,
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
    /// Per saved theme, one focus handle tracked on its Your themes row and
    /// never a tab stop itself, and one tab stop per row action (0 Edit…,
    /// 1 Export…, 2 Delete…) that the action's kit button tracks. The rows are
    /// virtualized, so a row's actions exist only while the row is drawn;
    /// owning their handles keeps an action focused while a scroll takes its
    /// row out of the list, and the row comes back with that focus. The
    /// handles let a render find the row holding focus and scroll it into
    /// view (`focused_row`), and the row's handle lets Tab reach a row's
    /// actions once the row is drawn (`focus_row_action`).
    row_focus: std::cell::RefCell<Vec<(u32, FocusHandle, [FocusHandle; 3])>>,
    /// What to do with a row once it is drawn: Tab across the viewport
    /// boundary scrolls the row in and the list's next render focuses one of
    /// its actions, or takes the key's step from the action holding focus
    /// (`GitTurtle::tab_theme_rows`, `take_row_focus_request`).
    row_focus_request: std::cell::Cell<Option<(usize, RowFocus)>>,
    /// The row last scrolled into view because one of its actions held focus,
    /// with that action, so a wheel scroll away from a focused row is not
    /// undone by the next render while Tab onto another of its actions
    /// reveals the row again.
    revealed_row: std::cell::RefCell<Option<(usize, WeakFocusHandle)>>,
    /// The Settings page's own scroll (`settings.rs`), so that revealing the
    /// row holding focus also brings it, with its focus ring's room, into the
    /// page's viewport and not only into the list's, and every other control
    /// the keyboard focuses is revealed there too. The page starts at its
    /// top whenever Settings is entered (`settings_entered`), as it did while
    /// the offset lived in the page's element state.
    page: SettingsPage,
    /// The window y of the first Your themes row with the page and the list
    /// both at offset zero, recorded each time the page is prepainted: the
    /// page reveal places a row from it before the frame lays the page out.
    rows_anchor: std::rc::Rc<std::cell::Cell<Option<Pixels>>>,
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

    /// Test-only: a failure at the foot of the Your themes card, for a message
    /// no test platform dialog can produce.
    #[cfg(test)]
    pub(super) fn set_error(&mut self, error: String) {
        self.error = Some(error);
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

    /// Theme `id`'s row handle and action handles, created on first use and
    /// kept while the theme is saved.
    fn row_handles(&self, id: u32, cx: &App) -> (FocusHandle, [FocusHandle; 3]) {
        let mut rows = self.row_focus.borrow_mut();
        if let Some((_, row, actions)) = rows.iter().find(|(row, ..)| *row == id) {
            return (row.clone(), actions.clone());
        }
        let row = cx.focus_handle();
        let actions = std::array::from_fn(|_| cx.focus_handle().tab_stop(true));
        rows.push((id, row.clone(), actions.clone()));
        (row, actions)
    }

    /// Theme `id`'s row handle.
    pub(super) fn row_focus(&self, id: u32, cx: &App) -> FocusHandle {
        self.row_handles(id, cx).0
    }

    /// The handle of theme `id`'s Edit… (0), Export… (1) or Delete… (2).
    pub(super) fn action_focus(&self, id: u32, action: usize, cx: &App) -> FocusHandle {
        self.row_handles(id, cx).1[action].clone()
    }

    /// Drop the handles of themes no longer saved.
    pub(super) fn retain_row_focus(&self, themes: &[CustomTheme]) {
        self.row_focus
            .borrow_mut()
            .retain(|(id, ..)| themes.iter().any(|theme| theme.id == *id));
    }

    /// The row holding focus, itself or through one of its actions, with the
    /// focused handle, whether or not the row is drawn.
    pub(super) fn focused_row(
        &self,
        themes: &[CustomTheme],
        window: &Window,
        cx: &App,
    ) -> Option<(usize, FocusHandle)> {
        let focused = window.focused(cx)?;
        let rows = self.row_focus.borrow();
        let row = themes.iter().position(|theme| {
            rows.iter().any(|(id, row, actions)| {
                *id == theme.id && (actions.contains(&focused) || row.contains_focused(window, cx))
            })
        })?;
        Some((row, focused))
    }

    /// Whether `focused` (from `focused_row`) is an action whose row is not
    /// drawn: a wheel or scrollbar scroll took the row out of the list while
    /// the action kept focus. The action has no node of its own then, so the
    /// list tracks its handle (`settings.rs`) to keep Tab in the Settings key
    /// context.
    pub(super) fn focus_scrolled_out(
        &self,
        themes: &[CustomTheme],
        focused: &(usize, FocusHandle),
        window: &Window,
        cx: &App,
    ) -> bool {
        !self
            .row_focus(themes[focused.0].id, cx)
            .contains_focused(window, cx)
    }

    /// Ask the list's next render to act on `row` once drawn.
    pub(super) fn request_row_focus(&self, row: usize, focus: RowFocus) {
        self.row_focus_request.set(Some((row, focus)));
    }

    /// The request whose row this render draws, if any; a request for a row
    /// no longer saved is dropped.
    pub(super) fn take_row_focus_request(
        &self,
        drawn: &std::ops::Range<usize>,
        count: usize,
    ) -> Option<(usize, RowFocus)> {
        let (row, focus) = self.row_focus_request.get()?;
        if row >= count {
            self.row_focus_request.set(None);
            return None;
        }
        drawn.contains(&row).then(|| {
            self.row_focus_request.set(None);
            (row, focus)
        })
    }

    /// Act on the row drawn with `handle`. The row's handle is in the tab
    /// order ahead of its actions and is not a stop itself, so the first stop
    /// after it is Edit…, then Export…, then Delete…. A step is the Tab or
    /// Shift-Tab again, dispatched from the row to `GitTurtle::tab_theme_rows`
    /// now that the action holding focus is a stop.
    pub(super) fn focus_row_action(
        handle: &FocusHandle,
        focus: RowFocus,
        window: &mut Window,
        cx: &mut App,
    ) {
        match focus {
            RowFocus::Action(action) => {
                handle.focus(window, cx);
                for _ in 0..=action {
                    window.focus_next(cx);
                }
            }
            RowFocus::Step(true) => handle.dispatch_action(&NextThemeAction, window, cx),
            RowFocus::Step(false) => handle.dispatch_action(&PreviousThemeAction, window, cx),
        }
    }

    /// Test-only: the row and action (0 Edit…, 1 Export…, 2 Delete…) holding
    /// focus, whether or not the row is drawn.
    #[cfg(test)]
    pub(super) fn focused_row_action(
        &self,
        themes: &[CustomTheme],
        window: &Window,
        cx: &App,
    ) -> Option<(usize, usize)> {
        let focused = window.focused(cx)?;
        let rows = self.row_focus.borrow();
        themes.iter().enumerate().find_map(|(index, theme)| {
            let (.., actions) = rows.iter().find(|(id, ..)| *id == theme.id)?;
            Some((index, actions.iter().position(|action| *action == focused)?))
        })
    }

    /// Scroll the row holding focus into view once per focus change
    /// (`ScrollStrategy::Nearest`), a move to another action of the same row
    /// included; a wheel or scrollbar scroll away from it afterwards is left
    /// alone. Returns the row scrolled to, which the caller also reveals in
    /// the Settings page (`settings.rs`, `reveal_in_page`).
    pub(super) fn reveal_focused_row(
        &self,
        focused: Option<(usize, FocusHandle)>,
    ) -> Option<usize> {
        let focused = focused.map(|(row, handle)| (row, handle.downgrade()));
        if *self.revealed_row.borrow() == focused {
            return None;
        }
        let row = focused.as_ref().map(|(row, _)| *row);
        *self.revealed_row.borrow_mut() = focused;
        if let Some(row) = row {
            self.rows_scroll
                .scroll_to_item(row, ScrollStrategy::Nearest);
        }
        row
    }

    /// The Settings page's scroll handle.
    pub(super) fn page_scroll(&self) -> &ScrollHandle {
        self.page.0.scroll()
    }

    /// Reveals a Settings control in the page when the keyboard focuses it.
    pub(super) fn page(&self) -> &FocusReveal {
        &self.page.0
    }

    /// Where the first Your themes row stands with the page and the list at
    /// offset zero, as the page last prepainted it.
    pub(super) fn rows_anchor(&self) -> &std::rc::Rc<std::cell::Cell<Option<Pixels>>> {
        &self.rows_anchor
    }

    /// Settings was entered from another page: the page starts at its top and
    /// the rows' anchor waits for the page's first prepaint.
    pub(super) fn settings_entered(&self) {
        self.page_scroll().set_offset(Point::default());
        self.rows_anchor.set(None);
    }
}

/// What the Your themes list does with a requested row once it draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowFocus {
    /// Focus Edit… (0), Export… (1) or Delete… (2).
    Action(usize),
    /// Tab (`true`) or Shift-Tab from the action holding focus, which had no
    /// tab stop when the key arrived because its row was not drawn.
    Step(bool),
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

gpui::actions!(
    theme_editor,
    [
        NextThemeAction,
        PreviousThemeAction,
        PageUpAction,
        PageDownAction,
        PageTopAction,
        PageBottomAction,
        NextPickerStop,
        PreviousPickerStop
    ]
);

/// The Settings page's key context, deeper than the toolkit root's, so the
/// bindings of [`init`] are tried first and fall through when they do not
/// apply.
pub(super) const SETTINGS_CONTEXT: &str = "GitTurtleSettings";

/// The key context around a row's colour picker, its trigger and its
/// popover ([`step_in_picker`]).
const PICKER_CONTEXT: &str = "GitTurtleColorPicker";

/// Tab and Shift-Tab inside Settings walk the Your themes rows through
/// [`GitTurtle::tab_theme_rows`] before the toolkit's own tab order, which
/// knows only the rows drawn.
///
/// Page Up, Page Down, Home and End scroll the Settings page
/// (`GitTurtle::scroll_settings_page`). They are bound at the app's root,
/// which holds focus when Settings opens. A focused text field binds them in
/// its own, deeper context, so they keep moving its caret, and on other pages
/// the root passes them on to the focused view's own key handling.
pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", NextThemeAction, Some(SETTINGS_CONTEXT)),
        KeyBinding::new("shift-tab", PreviousThemeAction, Some(SETTINGS_CONTEXT)),
        KeyBinding::new("pageup", PageUpAction, Some("GitTurtle")),
        KeyBinding::new("pagedown", PageDownAction, Some("GitTurtle")),
        KeyBinding::new("home", PageTopAction, Some("GitTurtle")),
        KeyBinding::new("end", PageBottomAction, Some("GitTurtle")),
        // In the kit popover's context inside a row's picker, deeper than the
        // dialog's Tab and below a text field's, which passes Tab on.
        KeyBinding::new(
            "tab",
            NextPickerStop,
            Some("GitTurtleColorPicker > Popover"),
        ),
        KeyBinding::new(
            "shift-tab",
            PreviousPickerStop,
            Some("GitTurtleColorPicker > Popover"),
        ),
    ]);
}

/// Tab (`forward`) or Shift-Tab in a row's open colour picker, whose
/// `trigger` holds its popover: focus moves to the popover's next or
/// previous stop, its swatches (one stop, which the arrow keys move inside)
/// or its hex field, and wraps at either end, so it never rests behind the
/// drawn popover (decided 2026-11-07, `DESIGN.md`'s focus rules), which
/// closes only on Escape, a choice or a click outside. The popover is drawn
/// after the dialog, and GPUI's order reaches the dialog's stops past either
/// end of the popover's, so each key steps on round that order until it is
/// back inside the popover. With no other stop in the popover, focus stays
/// where it was.
fn step_in_picker(trigger: &FocusHandle, forward: bool, window: &mut Window, cx: &mut App) {
    let Some(start) = window.focused(cx) else {
        return;
    };
    let mut passed = Vec::new();
    loop {
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
        let Some(reached) = window.focused(cx) else {
            break;
        };
        if reached == start || passed.contains(&reached) {
            break;
        }
        if trigger.contains_focused(window, cx) && !trigger.is_focused(window) {
            return;
        }
        passed.push(reached);
    }
    window.focus(&start, cx);
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

/// The Readability list with no warnings. It names every rule rather than
/// contrast alone: the pressed step is measured in one channel, not as a ratio.
const NO_READABILITY_WARNINGS: &str = "Every readability rule is met.";

/// The window's status bar at 13 pt (the foot of `GitTurtle::render` in
/// `views.rs`), scaled by the interface text size as the bar is. A modal
/// panel capped by the window ends 16 px above it: under a modal the bar
/// stays in place, dimmed and inert under the backdrop like the rest of the
/// window, and no panel edge crosses it (`DESIGN.md`).
const STATUS_BAR_HEIGHT: f32 = 26.;

/// The client-decoration frame between the window's edges and its content,
/// for the lengths that place a modal panel vertically: the toolkit's
/// `window_paddings`, inside which the alert lays itself out, and the frame's
/// 1 px border inside the bottom padding, which the content (and the status
/// bar at its foot) ends above. Server decorations and a tiled edge have
/// neither.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowFrame {
    padding_top: Pixels,
    padding_bottom: Pixels,
    border_bottom: Pixels,
}

/// The status bar's top edge in `window`: the foot of the window's body,
/// inside the client-decoration frame with its insets rounded to device
/// pixels as the layout rounds them (`views::body_bounds`), less the bar,
/// drawn at a whole device pixel. A menu stays above it
/// (`native_accessibility::open_menu`). At scale factor 1.5 an untiled
/// frame's foot is 31 device pixels, not the 31.5 its 21 px make, so the
/// bar's top is a third of a pixel lower than the window less the frame's
/// unrounded insets puts it.
pub(crate) fn status_bar_top(window: &Window) -> Pixels {
    window.pixel_snap(
        crate::views::body_bounds(window).bottom()
            - window.pixel_snap(appearance::ui_size(STATUS_BAR_HEIGHT)),
    )
}

impl WindowFrame {
    /// The frame's 1 px border on each untiled side of a client-decorated
    /// window (`window_border` in the toolkit, which `Root` draws).
    const BORDER: Pixels = px(1.);

    fn of(window: &Window) -> Self {
        Self::new(
            gpui_kit::component::window_paddings(window),
            window.window_decorations(),
        )
    }

    fn new(paddings: Edges<Pixels>, decorations: Decorations) -> Self {
        let border_bottom = match decorations {
            Decorations::Client { tiling } if !tiling.bottom => Self::BORDER,
            _ => Pixels::ZERO,
        };
        Self {
            padding_top: paddings.top,
            padding_bottom: paddings.bottom,
            border_bottom,
        }
    }
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
/// The cross is the stroke-drawn `close` icon at the square's own size: its
/// strokes cross whole pixels on the diagonals, so about as much of its ink
/// reaches the 4.5:1 floor as of the readability `!`'s stem, where a text
/// `×` is all antialiased diagonals.
fn invalid_glyph(p: Palette) -> Div {
    div()
        .size(appearance::ui_size(14.))
        .flex_shrink_0()
        .rounded(px(3.))
        .bg(rgb(p.removed))
        .child(
            svg()
                .debug_selector(|| "theme-token-invalid-cross".into())
                .path("icons/close.svg")
                .size(appearance::ui_size(14.))
                .text_color(rgb(p.canvas)),
        )
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
    // Starts between whole grapheme clusters.
    let start = text
        .char_indices()
        .nth(characters - custom::MAX_MESSAGE_FRAGMENT_CHARS)
        .map_or(0, |(index, _)| crate::text_cut::ceil_cluster(&text, index));
    format!("…{}", &text[start..])
}

/// Focus `pressed`, the Your themes button whose action starts, unless it
/// holds focus already: focus stays where the person acted, as a dialog's
/// close gives focus back to its opener (decided 2026-11-12), and the kit's
/// Button takes none from a pointer press. A button its action disables
/// keeps the focus it holds (the kit `Button::track_focus` patch), so the
/// button holds it again once the action has finished, without a ring when
/// the pointer pressed it, since nothing the chooser or the alert does marks
/// keyboard focus but a key in the window.
fn focus_pressed(pressed: &FocusHandle, window: &mut Window, cx: &mut App) {
    if !pressed.is_focused(window) {
        window.focus(pressed, cx);
    }
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
        let effective = self.effective_theme(cx);
        match effective.selection {
            ThemeSelection::BuiltIn(choice) => choice,
            ThemeSelection::Custom(id) => self
                .custom_themes
                .iter()
                .find(|theme| theme.id == id)
                .map_or_else(ThemeChoice::default, |theme| theme.base),
            // The Omarchy theme has no base: the built-in of its lightness stands in.
            ThemeSelection::Omarchy => appearance::custom::fallback_base(effective.palette),
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
        let ring = self.focus_ring.clone();
        let form = cx.new(|cx| {
            ThemeForm::new(
                owner, editing, name, base, palette, customs, ring, window, cx,
            )
        });
        self.theme_editor.form = Some(form.clone());
        self.clear_theme_transfer_result();
        self.preview_theme_draft(palette, started, window, cx);
        let focus = form.read(cx).name.read(cx).focus_handle(cx);
        let focus_form = form.downgrade();
        // The dialog's close gives focus back to the focus it opened with,
        // which is the control that opened it: a press on New theme… or
        // Edit… focuses neither, and the close gave focus back to the page's
        // first control, Follow system appearance, scrolled out of view
        // (QA-110, found outside the checks, 5).
        let opener = match editing {
            None => self.page_focus("custom-themes-new", cx),
            Some(id) => self.theme_editor.action_focus(id, 0, cx),
        };
        focus_pressed(&opener, window, cx);
        self.theme_editor.opened_from = Some(editing);
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let viewport = window.viewport_size();
            let wide = viewport.width >= appearance::ui_size(1060.);
            // The rows scale with the interface text size, so the content
            // width scales, and the chrome is what the panel draws around
            // it: its side padding, which the kit sizes for a ghost Button's
            // pull and ring (16 px at 13 pt, 17 px at 18 pt:
            // `crate::dialog_ghost_room`), and the token column's track,
            // which stays. The longest description then keeps its clearance
            // from the warning slot at every size, like the scaled breakpoint
            // above (1,000 and 648 px at 13 pt).
            let content = appearance::ui_size(if wide { 952. } else { 600. });
            let padding = gpui_kit::component::dialog::dialog_side_padding(window, cx);
            let width = (padding * 2. + Scrollbar::width() + content)
                .min(viewport.width - px(32.))
                .max(px(320.));
            let submit = form.clone();
            let cancel = form.clone();
            let title = if form.read(cx).editing.is_some() {
                "Edit theme"
            } else {
                "New theme"
            };
            // The footer is the form's last row, inside its height cap, so a
            // footer that grows (a wrapped save error) takes its room from the
            // scrolling body and never moves the panel's edge. The alert's own
            // footer slot stays empty, since without one it adds an OK button,
            // and the alert's bottom padding is zero. The dialog clips its
            // body to the body's bounds, so the footer keeps the focus ring's
            // room under its buttons inside them, and the alert's gap above
            // the empty slot gives the room back: the buttons keep the
            // panel's 16 px padding under them (`DIALOG_FOOTER_GAP`).
            let room = appearance::button_ring_room(cx);
            dialog
                .title(title)
                .width(width)
                .gap(tags::DIALOG_FOOTER_GAP - room)
                .pb_0()
                .child(form.clone())
                .footer(div())
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
        // A wheel or scrollbar scroll took the row of the action holding
        // focus out of the list, so GPUI has no stop to step from: the row
        // scrolls back in, focus stays put, and the list's next render takes
        // the step once the row's stops exist.
        if let Some(from) = &from
            && self
                .theme_editor
                .focus_scrolled_out(&self.custom_themes, from, window, cx)
        {
            self.theme_editor
                .rows_scroll()
                .scroll_to_item(from.0, ScrollStrategy::Nearest);
            self.theme_editor
                .request_row_focus(from.0, RowFocus::Step(forward));
            self.notify_settings_page(cx);
            cx.notify();
            return;
        }
        let from = from.map(|(row, _)| row);
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
            .focused_row(&self.custom_themes, window, cx)
            .map(|(row, _)| row);
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
        self.theme_editor
            .request_row_focus(target.0, RowFocus::Action(target.1));
        cx.notify();
    }

    /// Drop the draft and re-apply the saved selection.
    fn close_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_editor.form = None;
        self.theme_editor.preview = None;
        self.reveal_theme_editor_opener(cx);
        self.apply_appearance(window, cx);
    }

    /// The editor is closing, and its close gives focus back to the control
    /// that opened it (`open_theme_editor`), or, after a new theme's Save,
    /// to the saved theme's Edit… (`finish_custom_theme_save` sets
    /// `opened_from` to it): the Settings page reveals that control with its
    /// ring's room in the first frame after the close, whatever closed the
    /// editor, a pointer press on Save or Cancel included, and a theme's row
    /// comes into the list's view, so a window resized while the editor was
    /// open still shows the control focus lands on.
    fn reveal_theme_editor_opener(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.theme_editor.opened_from.take() else {
            return;
        };
        self.theme_editor.page().reveal_next_focus();
        if let Some(row) =
            editing.and_then(|id| self.custom_themes.iter().position(|theme| theme.id == id))
        {
            // The row reveals itself, in the list and in the page, once per
            // focus change; the one before the editor opened may have been
            // this same action.
            self.theme_editor.revealed_row.borrow_mut().take();
            self.theme_editor
                .rows_scroll()
                .scroll_to_item(row, ScrollStrategy::Nearest);
        }
        self.notify_settings_page(cx);
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
        // The alert gives focus back to Delete…, which a press focuses
        // first, as its close and a delete the store refuses leave it
        // (decided 2026-11-12).
        focus_pressed(&self.theme_editor.action_focus(id, 2, cx), window, cx);
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
                .kind(DialogKind::Alert)
                .max_h(appearance::dialog_max_height(window))
                .title(title.clone())
                .child(native_accessibility::text(
                    "delete-theme-consequences",
                    explanation.clone(),
                ))
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
        // Export… keeps focus through the chooser and holds it once the
        // export finishes (decided 2026-11-12).
        focus_pressed(&self.theme_editor.action_focus(id, 1, cx), window, cx);
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
        // Import… keeps focus through the chooser and holds it once the
        // import finishes (decided 2026-11-12).
        focus_pressed(&self.page_focus("custom-themes-import", cx), window, cx);
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
                    // The job itself always answers `Ok`; the executor answers
                    // for it when its queue is full or the job panicked
                    // (`SerialExecutor::submit`).
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
    /// with a saved theme or a built-in label becomes "`<name>` (imported)", the
    /// 32-theme bound refuses the import with the bound, and the saved theme is
    /// highlighted rather than applied.
    fn add_imported_theme(
        &mut self,
        imported: ImportedTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A race guard: Import… is disabled at the bound, and every card
        // action waits while the dialog and the read run
        // (`theme_transfer_busy`), so only a list that reached the bound
        // between the click and this handler gets here. It keeps the import's
        // own sentence rather than the store's refusal of a 33rd theme.
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
                    // A new theme's Save gives focus to the saved theme's
                    // Edit…, not back to New theme…: one spoken result that
                    // names what was saved, and never a disabled control,
                    // which New theme… is once the save reaches
                    // `MAX_CUSTOM_THEMES` (decided 2026-11-10). The close
                    // gives focus back to New theme… first, which recorded
                    // it as the dialog opened.
                    let saved = (self.theme_editor.opened_from == Some(None)).then_some(id);
                    if saved.is_some() {
                        self.theme_editor.opened_from = Some(saved);
                    }
                    self.reveal_theme_editor_opener(cx);
                    window.close_dialog(cx);
                    if let Some(id) = saved {
                        window.focus(&self.theme_editor.action_focus(id, 0, cx), cx);
                    }
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
                // An import that fills the last place disables Import…, so
                // focus it still holds goes to the imported theme's Edit…,
                // which names what was imported, as a new theme's Save gives
                // focus to the saved theme, rather than resting on a disabled
                // control (decided 2026-11-14). Focus the person moved
                // meanwhile stays where they put it. A row's action that
                // gains focus brings its row into the list's view and the
                // page's (`State::reveal_focused_row`), the new row included.
                let filled = themes.len() >= MAX_CUSTOM_THEMES
                    && self
                        .page_focus("custom-themes-import", cx)
                        .is_focused(window);
                // Added and highlighted, not applied: the window keeps its theme.
                self.set_custom_themes(themes, cx);
                self.theme_editor.error = None;
                self.theme_editor.notice = Some(notice);
                self.theme_editor.imported = Some(id);
                if filled {
                    window.focus(&self.theme_editor.action_focus(id, 0, cx), cx);
                }
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
    /// The trigger draws its keyboard ring ([`ThemeForm::focus_ring`]).
    picker_ringed: bool,
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
///   save, a palette application, the text size, the picker trigger's
///   keyboard ring) builds it without a notify site;
/// - the row observes its hex field and its color picker: the field's view
///   node is registered outside the row (`settings::ViewNodeAnchor`, as the
///   Settings page does), so the kit input's notify from paint on every frame
///   dirties the field and the form but not the row, and the typing, caret,
///   selection and popover changes the row must show reach it through these
///   observers;
/// - GPUI's own refresh (hover, press, focus, a window refresh) builds every
///   cached view, and a frame that moves or resizes a row (a scroll, a
///   resize, the dialog's opening slide) builds it for its new bounds.
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
    /// labels and rows are its direct children. Each row reports its hex
    /// field and its picker's trigger (`column_row`), and the stacked body
    /// the Readability list after them (`report_warnings`), so the control
    /// holding focus scrolls into view, with its ring's room, in the frame
    /// that first draws the focus, a pointer's focus included, and again in
    /// the frame after a resize or text-size change that cuts it, as the
    /// project hub's form does (decided 2026-11-10): revealed only on a
    /// focus change, a trigger Tab had shown low in the column was left cut
    /// by a window shrunk under it.
    column: FocusReveal,
    /// The wide layout's preview-and-warnings column, which keeps the
    /// Readability list in view as `column` keeps a row's controls
    /// (`report_warnings`). The stacked body ends its frames too, with
    /// nothing drawn in it, so a widening finds the focus the last frame
    /// drew and reveals the list only where the stacked body showed it
    /// (`hand_over_warnings`): recorded only while wide, it counted the
    /// list's focus as new once widened and revealed a list the person had
    /// scrolled out of the stacked body's view.
    side: FocusReveal,
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
    /// The window's keyboard ring: a picker's trigger draws its ring only
    /// while it holds focus a key gave it, so a picker opened and closed by
    /// the pointer leaves its trigger focused without one, and Escape, Enter
    /// or Space closing a picker shows it.
    focus_ring: focus_ring::FocusRing,
    /// The Readability list is a tab stop, so the keyboard can bring it into view.
    warnings_focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl ThemeForm {
    #[expect(clippy::too_many_arguments)]
    fn new(
        owner: WeakEntity<GitTurtle>,
        editing: Option<u32>,
        name: String,
        base: ThemeChoice,
        palette: Palette,
        customs: Vec<CustomTheme>,
        focus_ring: focus_ring::FocusRing,
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
            column: FocusReveal::new(Trigger::AnyFocus),
            side: FocusReveal::new(Trigger::AnyFocus),
            wide: false,
            keep_focus: cx.focus_handle(),
            replace_focus: cx.focus_handle(),
            reset_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            save_focus: cx.focus_handle(),
            focus_ring,
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

    /// The tallest the form may be: the window less its client-decoration
    /// frame, the status bar, the 16 px above the bar and the alert's fixed
    /// chrome, each term drawn at a whole device pixel as the window's layout
    /// snaps it.
    fn max_height(window: &Window, cx: &App) -> Pixels {
        Self::max_height_for(
            window.viewport_size().height,
            WindowFrame::of(window),
            window.rem_size(),
            appearance::ui_size(STATUS_BAR_HEIGHT),
            appearance::button_ring_room(cx),
            |length| window.pixel_snap(length),
        )
    }

    /// [`Self::max_height`] for a window `viewport` tall with `frame` around
    /// its content, whose layout snaps a length with `snap`, with the alert's
    /// text at `rem`, the status bar `status_bar` tall and a focused Button's
    /// ring `ring_room` outside its edge.
    ///
    /// The alert lays itself out inside the frame's paddings and opens a
    /// tenth of that height down; the window's content, and the status bar
    /// at its foot, ends inside the paddings and the frame's border. The
    /// panel's lower edge stays 16 px above the bar's top edge. Inside the
    /// panel are its 1 px border and 16 px padding above, the title's
    /// one-rem line with the kit's half-rem gap under it, the form, which
    /// holds the footer and the ring's room under its buttons, the alert's
    /// gap above its empty footer slot, which is the rest of the 16 px
    /// padding under the footer, and the 1 px border below. The rem terms
    /// and the bar follow the interface text size, and the pixel terms do
    /// not. GPUI draws each term at the nearest device pixel, halves toward
    /// zero, so each is counted that way (at scale 1 the 6.5 px gap at
    /// 13 pt is 6 px), and the cap, whole device pixels in turn, is drawn as
    /// computed. The body takes what the header, the base question and the
    /// footer leave, by layout.
    fn max_height_for(
        viewport: Pixels,
        frame: WindowFrame,
        rem: Pixels,
        status_bar: Pixels,
        ring_room: Pixels,
        snap: impl Fn(Pixels) -> Pixels,
    ) -> Pixels {
        let panel_top =
            frame.padding_top + snap((viewport - frame.padding_top - frame.padding_bottom) / 10.);
        let status_top = viewport - frame.padding_bottom - frame.border_bottom - snap(status_bar);
        let chrome = snap(px(1.))
            + snap(px(16.))
            + snap(rem)
            + snap(rem * 0.5)
            + snap(tags::DIALOG_FOOTER_GAP - ring_room)
            + snap(px(1.));
        (status_top - snap(px(16.)) - panel_top - chrome).max(px(200.))
    }

    /// A token row's index among the scroll container's children, which are
    /// the group labels and rows interleaved in `TokenKind::ALL` order.
    #[cfg(test)]
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

    /// `warnings`, the Readability list, reported to `reveal`, the reveal of
    /// the container that holds it (`side` in the wide layout, `column`
    /// stacked), which shows it with the ring's room as it shows a row's
    /// controls: in the frame that first draws its focus, a pointer's
    /// included, and in the frame after a resize or text-size change that
    /// cuts it. Scrolled into view only as it gained focus, the list was
    /// left cut by a window shrunk under it. The wrapper is laid out as the
    /// list and takes no focus.
    fn report_warnings(&self, reveal: &FocusReveal, warnings: AnyElement, cx: &App) -> Div {
        div()
            .on_children_prepainted(reveal.control(
                self.warnings_focus.clone(),
                appearance::button_ring_room(cx),
            ))
            .child(warnings)
    }

    /// A layout that crosses the breakpoint moves the Readability list
    /// between the side column and the stacked body, two containers that
    /// each keep only what they drew: where the one it leaves showed it
    /// holding focus in the last frame, as that container's reveal judges it
    /// (`FocusReveal::shows_focus`), the one it enters reveals it, in the
    /// next frame, as a resize does within one, and otherwise leaves it
    /// where it lies. Narrowed from 1,440 × 900 to 1,000 × 680, the focused
    /// list moved below the stacked body's fold and stayed there; widened
    /// from a 1,000 × 420 stacked body scrolled to its top, a list out of
    /// view there was scrolled into the side column.
    fn hand_over_warnings(&self, wide: bool, window: &Window) {
        if wide == self.wide || !self.warnings_focus.is_focused(window) {
            return;
        }
        let (from, to) = if wide {
            (&self.column, &self.side)
        } else {
            (&self.side, &self.column)
        };
        if from.shows_focus() {
            to.reveal_next_focus();
        }
    }

    /// Return, delivered as the dialog's Confirm action and routed by focus:
    /// Name submits, a hex field commits its value, a wrapped button activates
    /// (the toolkit's own Enter click never runs once the action is handled),
    /// and anything else (Base, a picker, the Readability list) does nothing.
    ///
    /// Return acts only for an Enter whose press began on the control it acts
    /// on, as the rewrite review's list does: the dialog confirms each fresh
    /// key-down, and under X11 a held Enter's repeats arrive as fresh
    /// presses, so a repeat after focus moved onto Save, Cancel or Reset, as
    /// a picker closed by Enter in its hex field once gave focus to Save,
    /// saved the theme or closed the editor. Such a press acts on nothing
    /// until its key-up and a new key-down.
    ///
    /// It acts once for a held Enter where focus stays, too: a repeat, which
    /// X11 sends as a fresh press the dialog confirms, began on the control
    /// it lands on, so a held Return in Name began another save after each
    /// failed or refused one, each announcing its error again.
    fn confirm_pressed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(focused) = window.focused(cx) else {
            return;
        };
        if gpui_kit::base::enter_press_is_repeat(window, cx)
            || !gpui_kit::base::enter_press_began_on(&focused, window, cx)
        {
            return;
        }
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
                .any(|issue| issue.tokens().any(|kind| kind == row.kind)),
            pending: self.pending,
            active: palette(cx),
            rem: window.rem_size(),
            picker_ringed: self.focus_ring.shows(&row.picker.focus_handle(cx)),
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

    /// `row` in the token column, reporting its hex field and its picker's
    /// trigger to the column's reveal (`column`): the wrapper is laid out as
    /// the row, whose cached view keeps its own prepaint, while the
    /// wrapper's report runs in every frame the form draws.
    fn column_row(&self, row: &TokenRow, window: &Window, cx: &App) -> AnyElement {
        let focus = vec![
            row.hex.read(cx).focus_handle(cx),
            row.picker.focus_handle(cx),
        ];
        div()
            .w_full()
            .h(appearance::ui_size(30.))
            .flex_shrink_0()
            .on_children_prepainted(self.column.group(focus, appearance::button_ring_room(cx)))
            .child(self.row_element(row, window, cx))
            .into_any_element()
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
    /// The form's last row, 16 px under the body: a save error that wraps
    /// makes the row taller and the body, which shrinks first, gives up the
    /// room. The dialog clips its body to the body's bounds, so the row keeps
    /// a focused button's ring room under the buttons; the 16 px above and
    /// the body's side padding already hold it.
    fn render_footer(&self, p: Palette, cx: &Context<Self>) -> AnyElement {
        let form = cx.entity();
        let reset = form.clone();
        let cancel = form.clone();
        let save = form.clone();
        let confirm = form;
        let (reset_focus, cancel_focus, save_focus) = (
            self.reset_focus.clone(),
            self.cancel_focus.clone(),
            self.save_focus.clone(),
        );
        div()
            .debug_selector(|| "theme-editor-footer".into())
            .w_full()
            .flex_shrink_0()
            .mt(px(16.))
            .pb(appearance::button_ring_room(cx))
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
                        .debug_selector(|| "theme-editor-reset".into())
                        .disabled(self.pending)
                        .accessibility_label(format!("Reset every color to {}", self.base.label()))
                        .on_click(move |_, window, cx| {
                            reset.update(cx, |form, cx| form.reset(window, cx))
                        }),
                ),
            )
            .child(div().flex_1())
            .children(self.error.clone().map(|error| {
                native_accessibility::static_text(
                    div()
                        .id("theme-editor-error")
                        .debug_selector(|| "theme-editor-error".into()),
                    error.clone(),
                )
                .min_w_0()
                .max_w(px(420.))
                .text_size(appearance::ui_text(12.))
                .text_color(rgb(p.warning))
                .child(error)
            }))
            .when(self.pending, |footer| {
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
                        .debug_selector(|| "theme-editor-cancel".into())
                        .disabled(self.pending)
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
                        .disabled(!self.can_save())
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
                        native_accessibility::static_text(
                            div()
                                .id("theme-editor-name-error")
                                .debug_selector(|| "theme-editor-name-error".into()),
                            error.clone(),
                        )
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
                            .app_menu(move |mut menu, _, _| {
                                // As wide as the button; as tall as the window
                                // allows above the status bar, then scrolling,
                                // as every menu is.
                                menu = menu.min_w(Self::base_width());
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
                    native_accessibility::static_text(
                        div().id("theme-editor-base-question-text"),
                        question.clone(),
                    )
                    .flex_1()
                    .min_w_0()
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

    /// What a token row's name and description say whole where the row cuts
    /// them short.
    fn row_tooltip(kind: TokenKind) -> SharedString {
        format!("{} · {}", kind.label(), kind.description()).into()
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
        let tooltip = Self::row_tooltip(kind);
        let trigger = row.picker.focus_handle(cx);
        let picker_ringed = self.focus_ring.shows(&trigger);
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
                // The name gives way for the row's controls, after the
                // description: at 18 pt in a 461 px window the row's fixed
                // columns ran 21 px past its content, under the scrollbar's
                // track and past the panel's clip, and cut the colour
                // picker's ring.
                div()
                    .id(("theme-token-name", kind as usize))
                    .debug_selector(move || format!("theme-token-name-{}", kind.key()))
                    .w(appearance::ui_size(128.))
                    .min_w_0()
                    .truncate()
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(p.text))
                    .tooltip({
                        let tooltip = tooltip.clone();
                        move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)
                    })
                    .child(label),
            )
            .child(
                div()
                    .id(("theme-token-description", kind as usize))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .debug_selector(move || format!("theme-token-description-{}", kind.key()))
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .child(kind.description()),
            )
            .children(if row.invalid {
                Some(native_accessibility::static_text(
                    invalid_glyph(p)
                        .id(("theme-token-invalid", kind as usize))
                        .debug_selector(move || format!("theme-token-invalid-{}", kind.key())),
                    format!("{label} value is not #rrggbb"),
                ))
            } else if flagged {
                Some(native_accessibility::static_text(
                    warning_glyph(p)
                        .id(("theme-token-warning", kind as usize))
                        .debug_selector(move || format!("theme-token-warning-{}", kind.key())),
                    format!("{label} has a readability warning"),
                ))
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
                // border edge at rest, the 2 px accent ring while it holds
                // keyboard focus.
                div()
                    .debug_selector(move || format!("theme-token-picker-{}", kind.key()))
                    .key_context(PICKER_CONTEXT)
                    .on_action({
                        let trigger = trigger.clone();
                        move |_: &NextPickerStop, window, cx| {
                            step_in_picker(&trigger, true, window, cx)
                        }
                    })
                    .on_action(move |_: &PreviousPickerStop, window, cx| {
                        step_in_picker(&trigger, false, window, cx)
                    })
                    .rounded(px(7.))
                    .map(|ring| {
                        if picker_ringed {
                            ring.border_2().border_color(rgb(p.accent))
                        } else {
                            ring.border_1().border_color(rgb(p.border)).p(px(1.))
                        }
                    })
                    .child(
                        ColorPicker::new(&row.picker)
                            .small()
                            .anchor(Anchor::TopRight)
                            .accessibility_label(format!("Choose the {label} color"))
                            // The open popover is named by the color it
                            // sets, as the row's hex field is.
                            .popover_label(format!("{label} color")),
                    ),
            )
            .into_any_element()
    }

    /// A column's always-visible scrollbar (`DESIGN.md`: a bounded scrolling
    /// area shows one), in the track the column reserves at its right edge.
    /// The wrapper marks the track for tests; the scrollbar positions itself
    /// over the tracked container.
    fn render_scrollbar(scroll: &ScrollHandle, id: &'static str) -> AnyElement {
        div()
            .debug_selector(move || id.into())
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(Scrollbar::width())
            .child(
                Scrollbar::vertical(scroll)
                    .id(id)
                    .mode(ScrollbarMode::Always),
            )
            .into_any_element()
    }

    fn render_warnings(&self, p: Palette, window: &Window) -> AnyElement {
        let count = self.warnings.len();
        let focused = self.warnings_focus.is_focused(window);
        // The group sits between the scrolling column and its warnings, and
        // AccessKit compares a child with its nearest clipping parent alone,
        // so the group clips its warnings: those below the column's edge
        // leave the tree with the part of the group the window does not draw.
        crate::native_accessibility::clips_children(
            div()
                .id("theme-editor-warnings")
                .debug_selector(|| "theme-editor-warnings".into())
                // A tab stop after the last row: the list is content, not a
                // control, but focusing it scrolls it into view.
                .track_focus(&self.warnings_focus)
                .role(Role::Group)
                .aria_label(format!(
                    "Readability, {}",
                    crate::counted(count, "warning", "warnings")
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
                                    .child(crate::counted(count, "warning", "warnings")),
                            )
                        }),
                )
                .when(count == 0, |list| {
                    list.child(
                        div()
                            .debug_selector(|| "theme-editor-no-warnings".into())
                            .text_size(appearance::ui_text(11.))
                            .text_color(rgb(p.muted))
                            .child(NO_READABILITY_WARNINGS),
                    )
                })
                .children(self.warnings.iter().enumerate().map(|(index, issue)| {
                    let text = issue.to_string();
                    native_accessibility::static_text(
                        div()
                            .id(("theme-warning", index))
                            .debug_selector(move || format!("theme-warning-{index}")),
                        text.clone(),
                    )
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
                }),
        )
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
        self.hand_over_warnings(wide, window);
        self.wide = wide;
        // A picker a press has just opened holds focus in its popover, which
        // the column counts as its row's once a frame has drawn it: its
        // trigger comes into view in this frame, the popover's first.
        if let Some(row) = self.rows.iter().find(|row| row.picker.read(cx).is_open()) {
            self.column
                .reveal_opener(&row.picker.focus_handle(cx), window, cx);
        }
        // A control focus has moved onto, or a resize has cut, comes into
        // view in this frame: a row's, or the Readability list in its column.
        self.column.reveal(window, cx);
        if wide {
            self.side.reveal(window, cx);
        }
        let max_height = Self::max_height(window, cx);
        // Group labels and rows are direct children of the scroll container,
        // which keeps where each lies (`ScrollHandle::bounds_for_item`).
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
                    .map(|row| self.column_row(row, window, cx)),
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
            // The side column scrolls on its own. While it overflows (past
            // about seventeen warnings at 1,440 × 900) it gains the scrollbar
            // in a track added at its right edge, so its 300 px content keeps
            // its width and a frame without overflow is unchanged. The last
            // layout decides, and the column's prepaint asks for the next
            // frame when this one laid it out the other way (a notify during
            // a draw requests no frame).
            let side_overflows = self.side.scroll().max_offset().y > Pixels::ZERO;
            let side_track = if side_overflows {
                Scrollbar::width()
            } else {
                Pixels::ZERO
            };
            let form = cx.entity().downgrade();
            let side_scroll = self.side.scroll().clone();
            // Records the focus this frame drew with, after the list has
            // reported.
            let side_observed = self.side.observe();
            let warnings = self.report_warnings(&self.side, warnings, cx);
            div()
                .relative()
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
                        .child(crate::native_accessibility::clips_children(
                            div()
                                // Records the focus this frame drew with,
                                // after every row has reported.
                                .on_children_prepainted(self.column.observe())
                                .id("theme-editor-body")
                                .debug_selector(|| "theme-editor-body".into())
                                .flex_1()
                                .min_h_0()
                                // The scrollbar paints an absolute overlay;
                                // reserve its track so the thumb never
                                // covers the pickers.
                                .pr(Scrollbar::width())
                                .overflow_y_scroll()
                                .track_scroll(self.column.scroll())
                                .flex()
                                .flex_col()
                                .children(entries),
                        ))
                        .child(Self::render_scrollbar(
                            self.column.scroll(),
                            "theme-editor-scrollbar",
                        )),
                )
                .child(crate::native_accessibility::clips_children(
                    div()
                        .on_children_prepainted(move |bounds, window, cx| {
                            if (side_scroll.max_offset().y > Pixels::ZERO) != side_overflows {
                                let form = form.clone();
                                window.on_next_frame(move |_, cx| {
                                    form.update(cx, |_, cx| cx.notify()).ok();
                                });
                            }
                            side_observed(bounds, window, cx);
                        })
                        .id("theme-editor-side")
                        .debug_selector(|| "theme-editor-side".into())
                        .w(px(300.) + side_track)
                        .pr(side_track)
                        .flex_shrink_0()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(self.side.scroll())
                        // The focus ring's room under the list, which its
                        // reveal leaves below it (`report_warnings`).
                        .pb(appearance::button_ring_room(cx))
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(preview)
                        .child(warnings),
                ))
                .when(side_overflows, |body| {
                    body.child(Self::render_scrollbar(
                        self.side.scroll(),
                        "theme-editor-side-scrollbar",
                    ))
                })
                .into_any_element()
        } else {
            entries.push(preview.mt_5().into_any_element());
            entries.push(
                self.report_warnings(&self.column, warnings, cx)
                    .mt_4()
                    .into_any_element(),
            );
            let column_observed = self.column.observe();
            // The side column draws nothing while stacked, and records so.
            let side_observed = self.side.observe();
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(crate::native_accessibility::clips_children(
                    div()
                        .on_children_prepainted(move |bounds, window, cx| {
                            column_observed(bounds, window, cx);
                            side_observed(Vec::new(), window, cx);
                        })
                        .id("theme-editor-body")
                        .debug_selector(|| "theme-editor-body".into())
                        .flex_1()
                        .min_h_0()
                        .pr(Scrollbar::width())
                        // The focus ring's room under the Readability list,
                        // its last entry, which its reveal leaves below it.
                        .pb(appearance::button_ring_room(cx))
                        .overflow_y_scroll()
                        .track_scroll(self.column.scroll())
                        .flex()
                        .flex_col()
                        .children(entries),
                ))
                .child(Self::render_scrollbar(
                    self.column.scroll(),
                    "theme-editor-scrollbar",
                ))
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
        // The cap holds the footer too: the content above it shrinks, by its
        // scrolling body, to what the footer leaves.
        div()
            .id("theme-editor")
            .debug_selector(|| "theme-editor".into())
            .max_h(max_height)
            .flex()
            .flex_col()
            .on_action(
                cx.listener(|this, _: &Confirm, window, cx| this.confirm_pressed(window, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(self.render_header(p, cx))
                    .children(self.render_base_question(p, cx))
                    .child(body),
            )
            .child(self.render_footer(p, cx))
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
#[path = "theme_editor_tests.rs"]
pub(crate) mod tests;
