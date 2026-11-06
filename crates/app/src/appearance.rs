//! Native presentation choices shared by history, previews, and settings.

use gpui_kit::component::button::{ButtonCustomVariant, ButtonVariant};
use gpui_kit::component::highlighter::{SyntaxColors, ThemeStyle};
use gpui_kit::component::{Colorize, FocusRing, Theme, ThemeMode};
use gpui_kit::{App, FontFeatures, Global, Pixels, Rgba, StyleRefinement, Styled, Window, px, rgb};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::BinaryHeap;
use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicU8, AtomicU32, Ordering},
};

// Upstream values for the adapted built-in themes. Families whose themes have
// not landed yet are referenced only by the source tests.
#[cfg_attr(not(test), allow(dead_code))]
mod sources;

// The preference store reads the custom theme model; the theme editor and picker
// will consume the readability rules, token names and document format. Until
// they land, the tests are the only readers of most of them.
#[cfg_attr(not(test), allow(dead_code))]
pub mod custom;

// Linux only: the palette of the desktop's current Omarchy theme, for the Omarchy selection.
#[cfg(target_os = "linux")]
pub(crate) mod omarchy;

pub const DEFAULT_INTERFACE_TEXT_SIZE: u8 = 13;
pub const DEFAULT_CODE_TEXT_SIZE: u8 = 12;
pub const INTERFACE_TEXT_RANGE: std::ops::RangeInclusive<u8> = 11..=18;
pub const CODE_TEXT_RANGE: std::ops::RangeInclusive<u8> = 10..=24;
/// Bounds for the desktop's own text scaling factor (GNOME "Large Text").
#[cfg(any(target_os = "linux", test))]
pub const DESKTOP_TEXT_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;

// One application appearance applies to every native window. Pixel helpers are
// also usable by canvas geometry and pure row-height consumers without a UI
// context. No preference writes or repository work happen through these reads.
#[cfg(not(test))]
static INTERFACE_TEXT_SIZE: AtomicU8 = AtomicU8::new(DEFAULT_INTERFACE_TEXT_SIZE);
#[cfg(not(test))]
static CODE_TEXT_SIZE: AtomicU8 = AtomicU8::new(DEFAULT_CODE_TEXT_SIZE);
// The desktop's text scaling factor multiplies both app sizes so the saved
// interface/code preferences keep their meaning across desktops. Only the
// Linux desktop bridge changes it; other platforms scale through the toolkit.
#[cfg(not(test))]
static DESKTOP_TEXT_SCALE: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

// Each GPUI test owns its application on one test thread. Keep its simulated
// appearance there too: changing the font size must not move another test's
// controls between measuring their bounds and dispatching a click. Production
// continues to share the atomic values across the application's native windows.
#[cfg(test)]
thread_local! {
    static INTERFACE_TEXT_SIZE: AtomicU8 = const { AtomicU8::new(DEFAULT_INTERFACE_TEXT_SIZE) };
    static CODE_TEXT_SIZE: AtomicU8 = const { AtomicU8::new(DEFAULT_CODE_TEXT_SIZE) };
    static DESKTOP_TEXT_SCALE: AtomicU32 = const { AtomicU32::new(1.0f32.to_bits()) };
}

fn with_text_sizes<R>(read: impl FnOnce(&AtomicU8, &AtomicU8, &AtomicU32) -> R) -> R {
    #[cfg(not(test))]
    {
        read(&INTERFACE_TEXT_SIZE, &CODE_TEXT_SIZE, &DESKTOP_TEXT_SCALE)
    }
    #[cfg(test)]
    {
        INTERFACE_TEXT_SIZE.with(|interface| {
            CODE_TEXT_SIZE
                .with(|code| DESKTOP_TEXT_SCALE.with(|desktop| read(interface, code, desktop)))
        })
    }
}

fn interface_text_size() -> u8 {
    with_text_sizes(|interface, _, _| interface.load(Ordering::Relaxed))
}

fn code_text_size() -> u8 {
    with_text_sizes(|_, code, _| code.load(Ordering::Relaxed))
}

/// Per-app notification for retained workspace viewports. Observers keep their
/// last applied factor so coalesced desktop updates are applied exactly once.
#[cfg(any(target_os = "linux", test))]
pub(super) struct DesktopTextScale(pub f32);
#[cfg(any(target_os = "linux", test))]
impl Global for DesktopTextScale {}

pub fn desktop_text_scale() -> f32 {
    with_text_sizes(|_, _, desktop| f32::from_bits(desktop.load(Ordering::Relaxed)))
}
/// Normalizes a reported factor: non-positive/non-finite values mean "no scaling".
#[cfg(any(target_os = "linux", test))]
pub fn desktop_text_scale_value(scale: impl Into<f64>) -> f32 {
    let scale = scale.into();
    if scale.is_finite() && scale > 0.0 {
        (scale as f32).clamp(
            *DESKTOP_TEXT_SCALE_RANGE.start(),
            *DESKTOP_TEXT_SCALE_RANGE.end(),
        )
    } else {
        1.0
    }
}
/// Stores the desktop factor and re-applies the text sizes of every open
/// window. Returns whether the factor changed.
#[cfg(target_os = "linux")]
pub fn set_desktop_text_scale(scale: f32, cx: &mut App) -> bool {
    let scale = desktop_text_scale_value(scale);
    if scale.to_bits() == desktop_text_scale().to_bits() {
        return false;
    }
    with_text_sizes(|_, _, desktop| desktop.store(scale.to_bits(), Ordering::Relaxed));
    cx.set_global(DesktopTextScale(scale));
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, cx| {
            apply_text_sizes(interface_text_size(), code_text_size(), window, cx)
        });
    }
    true
}

pub fn ui_scale() -> f32 {
    f32::from(interface_text_size()) / f32::from(DEFAULT_INTERFACE_TEXT_SIZE) * desktop_text_scale()
}
pub fn ui_size(base: f32) -> Pixels {
    px(base * ui_scale())
}
/// The layout grid's insets (DESIGN.md, "One left edge"): where a panel's
/// text starts in from its content edge, and what a box at that edge (a
/// compact Button, a field) puts between its own edge and its text. A
/// compact Button's label inset is `crate::LABEL_INSET` beside the `button`
/// helper. `layout_grid` measures every panel against them.
///
/// A pane's content edge, in from its side, at the default interface size:
/// the kit's 0.75 rem, so its section labels, rows' leading icons, fields'
/// boxes and Buttons' frames stand at `ui_size(PANE_INSET)`.
pub const PANE_INSET: f32 = 9.75;

/// The kit's medium Input pads its text this far inside its 1 px border at
/// every interface size; rows in a bordered list under a field pad their
/// text as much, so they start where the field's text does.
pub const FIELD_TEXT_PADDING: Pixels = px(10.);

/// Where a medium Input's text and placeholder start, in from its outer
/// edge: its 1 px border and [`FIELD_TEXT_PADDING`]. Text that lines up with
/// a field beside it starts here, as the Reflog's does.
pub const FIELD_TEXT_INSET: Pixels = px(11.);

/// The room a dialog keeps from either side of a window too narrow for its
/// width, as in a 461 px half tile.
pub const DIALOG_MARGIN: Pixels = px(24.);

/// A dialog's width: `width`, or, in a window too narrow for it, the window
/// less [`DIALOG_MARGIN`] on either side. The kit centres a dialog at its
/// width whatever the window's, so a wider one would start left of the
/// window. The window is what the kit centres the dialog in: inside a
/// client-decorated frame's shadow padding. A dialog reads it on every frame,
/// so one open while the window narrows stays inside it. A dialog that sets
/// no width of its own keeps the same margin: the kit narrows its 448 px by
/// the theme's `dialog_margin`, which a palette's application sets to
/// [`DIALOG_MARGIN`].
pub fn dialog_width(width: Pixels, window: &Window) -> Pixels {
    let paddings = gpui_kit::component::window_paddings(window);
    let room = window.viewport_size().width - paddings.left - paddings.right;
    width.min(room - DIALOG_MARGIN * 2.).max(Pixels::ZERO)
}

/// The tallest a dialog may be: from where the kit opens it, a tenth of the
/// window down inside a client-decorated frame's padding, to
/// [`DIALOG_MARGIN`] above the status bar's top edge
/// (`theme_editor::status_bar_top`), so its foot never lies on the bar. A
/// dialog's scrolling list gives up the height first, and its body scrolls
/// past that. A dialog reads it on every frame, so one open while the window
/// shortens or the text grows stays above the bar. A dialog opened over
/// another opens 16 px lower for each dialog beneath it, and the kit takes
/// that offset from this cap, so its foot keeps the margin too.
pub fn dialog_max_height(window: &Window) -> Pixels {
    let paddings = gpui_kit::component::window_paddings(window);
    let room = window.viewport_size().height - paddings.top - paddings.bottom;
    let top = paddings.top + window.pixel_snap(room / 10.);
    (crate::theme_editor::status_bar_top(window) - DIALOG_MARGIN - top).max(Pixels::ZERO)
}

pub fn ui_text(base: f32) -> Pixels {
    ui_size(base)
}
pub fn code_text() -> Pixels {
    px(f32::from(code_text_size()) * desktop_text_scale())
}
pub fn code_scale() -> f32 {
    f32::from(code_text()) / f32::from(DEFAULT_CODE_TEXT_SIZE)
}

/// The font features of code text drawn in `family`. Code shows the characters
/// as typed, so a ligature font such as JetBrains Mono never joins `--`, `->`
/// or `!=` into one glyph. Fonts put programming ligatures in contextual
/// alternates (`calt`, as JetBrains Mono and Fira Code do) or in standard
/// ligatures (`liga`), so both are off. The bundled
/// [`BUNDLED_CODE_FAMILY`](crate::desktop_text::BUNDLED_CODE_FAMILY) has no
/// such ligatures, and turning `calt` and `liga` off costs it about 7% more
/// shaping time per line (`docs/benchmarks/2026-09-29-code-font-features.md`),
/// so it gets none.
pub fn code_font_features_for(family: &str) -> FontFeatures {
    static NONE: LazyLock<FontFeatures> = LazyLock::new(FontFeatures::default);
    static WITHOUT_LIGATURES: LazyLock<FontFeatures> =
        LazyLock::new(|| FontFeatures(Arc::new(vec![("calt".into(), 0), ("liga".into(), 0)])));
    if family == crate::desktop_text::BUNDLED_CODE_FAMILY {
        NONE.clone()
    } else {
        WITHOUT_LIGATURES.clone()
    }
}

/// Code text: the code family, with its [`code_font_features_for`].
pub trait CodeFont: Styled + Sized {
    fn code_font(self, cx: &App) -> Self {
        let family = Theme::global(cx).mono_font_family.clone();
        let features = code_font_features_for(&family);
        self.font_family(family).font_features(features)
    }
}
impl<T: Styled> CodeFont for T {}

/// Store the text sizes and project them onto the toolkit theme and the
/// window's rem geometry, leaving invalidation to the caller.
///
/// The one appearance application path ([`crate::GitTurtle::apply_appearance`]) calls
/// this and invalidates with the root's `cx.notify()` instead of
/// [`Window::refresh`]. Both mark the window dirty, but `refresh` also bars
/// GPUI's view reuse for that frame, which would rebuild the twenty
/// palette-independent theme miniatures ([`crate::settings::ThemePreviewBody`])
/// that a palette change does not alter.
pub fn sync_text_sizes(interface: u8, code: u8, window: &mut Window, cx: &mut App) {
    with_text_sizes(|interface_size, code_size, _| {
        interface_size.store(
            interface.clamp(*INTERFACE_TEXT_RANGE.start(), *INTERFACE_TEXT_RANGE.end()),
            Ordering::Relaxed,
        );
        code_size.store(
            code.clamp(*CODE_TEXT_RANGE.start(), *CODE_TEXT_RANGE.end()),
            Ordering::Relaxed,
        );
    });
    set_theme_text_sizes(Theme::global_mut(cx));
    Theme::sync_base(cx);
    // Root uses font_size for rem geometry, so native control padding and
    // heights grow together with explicit app text and custom canvas rows.
    window.set_rem_size(ui_text(13.));
}

/// A section label's text size at the default interface size, drawn at
/// medium weight: the navigation's headings, such as LOCAL BRANCHES, and a
/// menu's labels, the text that titles or explains the items around it.
///
/// A menu label sits beside items in the kit's `text_sm` (11.375 px at the
/// 13 px root) at regular weight, so weight sets it apart where colour cannot:
/// secondary text stands only 1.9:1 off an item in Midnight and Braden and
/// barely at all in One Dark or Solarized Dark. It is no larger than an item,
/// so a heading never outweighs the commands it titles, as a 12 px one did
/// in those palettes.
pub const SECTION_LABEL_TEXT: f32 = 11.;

/// The page every menu leaves between its drawn edge and the button or row
/// it opened from, at the default interface size: a dropdown below or above
/// its button, a row's menu below or above its row (`DESIGN.md`, "One gap
/// between a menu and what it opened from"). It scales with the interface
/// text, as the rows and buttons beside it do.
pub const POPUP_GAP: f32 = 4.;

/// [`POPUP_GAP`] at the current interface size.
pub fn popup_gap() -> Pixels {
    ui_size(POPUP_GAP)
}

/// The kit theme's text sizes at the current interface and code sizes: its
/// root and code text, the menu label's size and the gap its popups keep
/// from their trigger, which scales with them. Applying a palette restores
/// the kit's default theme first, so both paths set all four.
fn set_theme_text_sizes(theme: &mut Theme) {
    theme.font_size = ui_text(13.);
    theme.mono_font_size = code_text();
    theme.menu_label_size = Some(ui_text(SECTION_LABEL_TEXT));
    theme.popup_gap = Some(popup_gap());
}

/// [`sync_text_sizes`] for callers that own no entity to notify: a text-size
/// change moves every measured box, so refreshing the whole window is right
/// here.
pub fn apply_text_sizes(interface: u8, code: u8, window: &mut Window, cx: &mut App) {
    sync_text_sizes(interface, code, window, cx);
    window.refresh();
}

// The shared `button` helper builds its control without a UI context, so the
// palette application path leaves the control's colors here. Unlike the text
// sizes, only element construction reads them, which runs on the thread that
// owns the application, as palette application does; each GPUI test owns its
// application on its own thread.
thread_local! {
    static CONTROL_BUTTON: Cell<Option<ControlButton>> = const { Cell::new(None) };
}

#[derive(Clone, Copy)]
struct ControlButton {
    idle: ButtonCustomVariant,
    /// `idle` without its hover fill, for a control that arrived under a
    /// still pointer ([`still_control_button_variant`]).
    still: ButtonCustomVariant,
    /// Only for a palette whose [`Palette::control_label`] is not `text`.
    selected: Option<ButtonCustomVariant>,
    /// The selected look on a fill its container paints, with no fill of its
    /// own ([`control_button_variant_on_fill`]).
    on_fill: ButtonCustomVariant,
    /// The selected button's fill while hovered, [`Palette::row_hover`] of a
    /// selected row.
    selected_hover: u32,
}

/// The ring every focused Button draws: 2 px of `ring`, the palette accent, at
/// full opacity, 1 px outside its edge.
///
/// The kit's own ring is 3 px of `ring` at half opacity directly outside the
/// edge, below the graphic rule on most surfaces. The gap keeps this ring off a
/// primary or danger fill, so ring and gap both lie on the surface beneath,
/// where the accent rule holds `accent` to 3:1 on every readability surface.
/// It takes the same 3 px as the kit's ring, so clipping and the room layouts
/// leave for it do not change.
pub const BUTTON_FOCUS_RING: FocusRing = FocusRing {
    width: px(2.),
    gap: px(1.),
    opacity: 1.,
};

/// The room the installed `Theme::button_focus_ring` takes outside a focused
/// Button's edge, its gap plus its width: what a container that clips its
/// children keeps around a Button so the ring is drawn whole. It follows the
/// ring the theme installs rather than [`BUTTON_FOCUS_RING`], 3 px at the
/// default.
pub fn button_ring_room(cx: &App) -> Pixels {
    let ring = Theme::global(cx).button_focus_ring;
    ring.gap + ring.width
}

/// The hover of a Button that is selected in the shared helper's look,
/// [`control_button_variant`]`(true)`: the helper itself, the Settings density
/// segments and the project hub's mode segments. The kit gives a selected
/// control no hover surface, so this paints the resting `selected` fill
/// blended toward `accent`, as a hovered selected row does. It changes the
/// fill alone, so the focus ring keeps full opacity, and it stays
/// [`custom::PRESSED_STEP`] from the resting fill. Before any palette is
/// applied it leaves the style alone.
pub fn control_selected_hover(style: StyleRefinement) -> StyleRefinement {
    match CONTROL_BUTTON.get() {
        Some(control) => style.bg(rgb(control.selected_hover)),
        None => style,
    }
}

#[cfg(test)]
thread_local! {
    /// The weight, color and size each [`text_probe`] last saw its label's
    /// text drawn with, by key.
    static DRAWN_TEXT: std::cell::RefCell<
        std::collections::HashMap<String, (gpui_kit::FontWeight, gpui_kit::Hsla, Pixels)>,
    > = Default::default();
}

/// In tests, a zero-size child for a label's element that records, under
/// `key`, the font weight the label's text is shaped at, the color it is
/// painted in and its size, for [`drawn_weight`], [`drawn_color`] and
/// [`drawn_size`]; in other builds, nothing. Text takes each from the
/// window's text style where it is laid out, which is what the probe reads,
/// so a test sees what the label draws, inherited values included. The test
/// text system draws every weight in one face and paints no glyphs a test
/// can read, so neither the shaped runs nor the frame carry them. Absolutely
/// positioned, it moves nothing.
pub(crate) fn text_probe(key: impl FnOnce() -> String) -> Option<gpui_kit::AnyElement> {
    #[cfg(test)]
    {
        use gpui_kit::{IntoElement, canvas};
        let key = key();
        Some(
            canvas(
                move |_, window, _| {
                    let style = window.text_style();
                    let size = style.font_size.to_pixels(window.rem_size());
                    DRAWN_TEXT.with(|drawn| {
                        drawn
                            .borrow_mut()
                            .insert(key, (style.font_weight, style.color, size))
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_0()
            .into_any_element(),
        )
    }
    #[cfg(not(test))]
    {
        let _ = key;
        None
    }
}

/// The weight the [`text_probe`] under `key` last saw its label shaped at.
#[cfg(test)]
pub(crate) fn drawn_weight(key: &str) -> Option<gpui_kit::FontWeight> {
    DRAWN_TEXT.with(|drawn| drawn.borrow().get(key).map(|(weight, ..)| *weight))
}

/// The color the [`text_probe`] under `key` last saw its label painted in.
#[cfg(test)]
pub(crate) fn drawn_color(key: &str) -> Option<gpui_kit::Hsla> {
    DRAWN_TEXT.with(|drawn| drawn.borrow().get(key).map(|(_, color, _)| *color))
}

/// The size the [`text_probe`] under `key` last saw its label laid out at.
#[cfg(test)]
pub(crate) fn drawn_size(key: &str) -> Option<Pixels> {
    DRAWN_TEXT.with(|drawn| drawn.borrow().get(key).map(|(.., size)| *size))
}

/// What the last frame painted for the Button at `element`, for tests: the
/// visible fills on its bounds and the colors of the focus rings drawn around
/// it, at the footprint of the applied `Theme::button_focus_ring`. The kit
/// offsets the ring by the Button's own border, so a bordered Button's ring
/// has the same footprint (`focused_buttons_draw_the_theme_button_focus_ring`
/// checks the default variant's 1 px border). GPUI paints a border-only quad
/// once per side, so each ring color counts once.
#[cfg(test)]
pub(crate) fn painted_button(
    cx: &mut gpui_kit::VisualTestContext,
    element: gpui_kit::Bounds<Pixels>,
) -> (Vec<gpui_kit::Background>, Vec<gpui_kit::Hsla>) {
    use gpui_kit::{Bounds, ScaledPixels, point};
    cx.update(|window, cx| {
        let scale = window.scale_factor();
        let device = px(1. / scale);
        let logical = |value: ScaledPixels| px(value.as_f32() / scale);
        let near = |drawn: Bounds<Pixels>, expected: Bounds<Pixels>| {
            [
                (drawn.left(), expected.left()),
                (drawn.top(), expected.top()),
                (drawn.right(), expected.right()),
                (drawn.bottom(), expected.bottom()),
            ]
            .into_iter()
            .all(|(edge, expected)| (edge - expected).abs() <= device)
        };
        let setting = Theme::global(cx).button_focus_ring;
        let footprint = element.dilate(setting.gap + setting.width);
        let (mut fills, mut rings) = (Vec::new(), Vec::new());
        for quad in window.painted_quads() {
            let bounds = quad.bounds;
            let drawn = Bounds::from_corners(
                point(logical(bounds.left()), logical(bounds.top())),
                point(logical(bounds.right()), logical(bounds.bottom())),
            );
            let widths = quad.border_widths;
            let bordered = [widths.top, widths.right, widths.bottom, widths.left]
                .into_iter()
                .any(|width| width.as_f32() > 0.);
            if near(drawn, element) && !quad.background.is_transparent() {
                fills.push(quad.background);
            } else if bordered && near(drawn, footprint) && !rings.contains(&quad.border_color) {
                rings.push(quad.border_color);
            }
        }
        (fills, rings)
    })
}

/// The state [`assert_selected_button`] checks a selected Button in.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectedState {
    Resting,
    Hovered,
    FocusedAndHovered,
}

/// For tests of a Button selected in the shared helper's look,
/// [`control_button_variant`]`(true)` hovered with [`control_selected_hover`]:
/// asserts that the last frame filled `element`'s bounds with nothing but the
/// applied palette's opaque `selected` at rest or its `selected_hover` under
/// the pointer, and drew a focus ring around it in full `ring` exactly when it
/// is focused. A hover that fades the whole Button fails both, since the fade
/// reaches its fill and its ring alike.
#[cfg(test)]
pub(crate) fn assert_selected_button(
    cx: &mut gpui_kit::VisualTestContext,
    name: &str,
    element: gpui_kit::Bounds<Pixels>,
    state: SelectedState,
) {
    let (selected, ring) = cx.update(|_, cx| (palette(cx).selected, Theme::global(cx).ring));
    let expected = if state == SelectedState::Resting {
        gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(selected)))
    } else {
        control_selected_hover(StyleRefinement::default())
            .background
            .and_then(|fill| fill.color())
            .expect("a palette is applied")
    };
    let (fills, rings) = painted_button(cx, element);
    assert_eq!(fills, [expected], "{name} {state:?} paints {fills:?}");
    if state == SelectedState::FocusedAndHovered {
        assert_eq!(rings, [ring], "{name} {state:?} draws {rings:?}");
    } else {
        assert!(rings.is_empty(), "{name} {state:?} draws {rings:?}");
    }
}

/// The state [`assert_unselected_button`] checks an unselected Button in.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnselectedState {
    Resting,
    Hovered,
    /// Held down under the pointer.
    Pressed,
}

/// For tests of an unfocused Button in the shared helper's unselected look,
/// [`control_button_variant`]`(false)`, as History's segments draw it: asserts
/// that the last frame drew no focus ring around `element` and filled its
/// bounds with nothing at rest, with nothing but the applied palette's
/// [`Palette::control_fill`] of `hover` under the pointer, and with nothing
/// but its `control_fill` of `selected` while held pressed, so a press steps
/// to almost the selected fill.
#[cfg(test)]
pub(crate) fn assert_unselected_button(
    cx: &mut gpui_kit::VisualTestContext,
    name: &str,
    element: gpui_kit::Bounds<Pixels>,
    state: UnselectedState,
) {
    let palette = cx.update(|_, cx| palette(cx));
    let fill = |layer: Rgba| gpui_kit::Background::from(gpui_kit::Hsla::from(layer));
    let (fills, rings) = painted_button(cx, element);
    match state {
        UnselectedState::Resting => {
            assert!(fills.is_empty(), "{name} {state:?} paints {fills:?}")
        }
        UnselectedState::Hovered => assert_eq!(
            fills,
            [fill(palette.control_fill(palette.hover))],
            "{name} {state:?} paints {fills:?}"
        ),
        UnselectedState::Pressed => assert_eq!(
            fills,
            [fill(palette.control_fill(palette.selected))],
            "{name} {state:?} paints {fills:?}"
        ),
    }
    assert!(rings.is_empty(), "{name} {state:?} draws {rings:?}");
}

/// The shared compact button's variant: the applied palette's control fills,
/// or the kit's ghost before any palette is applied. A selected button keeps
/// the kit's secondary, which paints `selected` with `text`, unless the palette
/// moved the control label.
pub fn control_button_variant(selected: bool) -> ButtonVariant {
    match (selected, CONTROL_BUTTON.get()) {
        (false, Some(control)) => ButtonVariant::Custom(control.idle),
        (false, None) => ButtonVariant::Ghost,
        (
            true,
            Some(ControlButton {
                selected: Some(variant),
                ..
            }),
        ) => ButtonVariant::Custom(variant),
        (true, _) => ButtonVariant::Secondary,
    }
}

/// [`control_button_variant`] for a Button that lies on a selected fill its
/// container paints and edges with [`resting_edge`], as a selected
/// repository tab's frame joins its Button to its close button. While More
/// contrast is on, a selected one takes that look without a fill of its own
/// (the container's `selected` shows through it, the same colour), so the
/// kit draws no second resting edge inside the container's; its label and
/// its hover ([`control_selected_hover`]) are unchanged. Off, or unselected,
/// it is [`control_button_variant`].
pub fn control_button_variant_on_fill(selected: bool, cx: &App) -> ButtonVariant {
    match CONTROL_BUTTON.get() {
        Some(control) if selected && Theme::global(cx).resting_edge.is_some() => {
            ButtonVariant::Custom(control.on_fill)
        }
        _ => control_button_variant(selected),
    }
}

/// The 1 px edge a control the app draws itself lays over its bounds while
/// More contrast is on, in the colour the kit's `Theme::resting_edge` edges
/// its own filled controls with, for one that rests on a fill of its own:
/// the selected repository tab's frame. An absolutely placed layer with
/// `radius` corners and no role, added after the control's content so it
/// paints over it, moves nothing and adds no accessibility node. `None`
/// while More contrast is off.
pub fn resting_edge(radius: Pixels, cx: &App) -> Option<gpui_kit::Div> {
    let edge = Theme::global(cx).resting_edge?;
    Some(
        gpui_kit::div()
            .absolute()
            .top_0()
            .left_0()
            .bottom_0()
            .right_0()
            .border_1()
            .border_color(edge)
            .rounded(radius),
    )
}

/// The shared control at rest, as [`control_button_variant`]`(false)`, with
/// no hover fill: a control a list's rows moved under a pointer that has not
/// moved since shows no hover until the pointer moves (`motion::StillPointer`).
/// Before any palette is applied it is the kit's ghost.
pub fn still_control_button_variant() -> ButtonVariant {
    match CONTROL_BUTTON.get() {
        Some(control) => ButtonVariant::Custom(control.still),
        None => ButtonVariant::Ghost,
    }
}

/// Fills every shared control at rest with `fill` until a palette is applied
/// again, so a test finds each one among the painted quads with the content
/// mask that clips it (`width_walk`); a control transparent at rest paints
/// nothing to find.
#[cfg(test)]
pub(crate) fn mark_controls(fill: gpui_kit::Hsla) {
    if let Some(mut control) = CONTROL_BUTTON.get() {
        control.idle = control.idle.color(fill);
        control.still = control.still.color(fill);
        CONTROL_BUTTON.set(Some(control));
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    Graphite,
    Daylight,
    TokyoNight,
    CatppuccinMocha,
    Nord,
    Porcelain,
    Sandstone,
    DeepSea,
    Ember,
    SolarizedDark,
    SolarizedLight,
    OneDark,
    OneLight,
    RosePine,
    RosePineDawn,
    Dracula,
    Alucard,
    KanagawaWave,
    KanagawaLotus,
    #[default]
    #[serde(other)]
    Midnight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub canvas: u32,
    pub panel: u32,
    pub subtle: u32,
    pub hover: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_foreground: u32,
    pub accent_hover: u32,
    pub accent_active: u32,
    pub selected: u32,
    pub added: u32,
    pub removed: u32,
    pub modified: u32,
    pub renamed: u32,
    pub warning: u32,
    pub added_background: u32,
    pub removed_background: u32,
    pub hunk: u32,
    pub line_number: u32,
}

impl Global for Palette {}

impl Palette {
    /// Preserve the selection while giving a selected, clickable row feedback.
    pub fn row_hover(self, selected: bool) -> u32 {
        if !selected {
            return self.hover;
        }
        let mut color = 0;
        for shift in [0, 8, 16] {
            let base = (self.selected >> shift) & 0xff;
            let accent = (self.accent >> shift) & 0xff;
            color |= ((base * 93 + accent * 7) / 100) << shift;
        }
        color
    }

    /// The shared button's fill for a state surface (`hover` or `selected`): a
    /// translucent layer that GPUI composites over whatever the button sits on.
    ///
    /// Over `panel`, the surface the readability rules measure both states
    /// against, the layer composites to exactly `state`. Elsewhere it repeats
    /// that step, so a button in a hovered or selected row lifts again, where an
    /// opaque fill would vanish into a row of the same surface; and hover and
    /// pressed stay as far apart on every surface as the palette's own rows. The
    /// opacity is the smallest that still reaches `state` from `panel` with a
    /// displayable color, which keeps the step nearly the same everywhere: the
    /// layer moves a surface `s` by `(state - panel) + alpha * (panel - s)`.
    pub fn control_fill(self, state: u32) -> Rgba {
        let (panel, state) = (rgb(self.panel), rgb(state));
        let steps = [(panel.r, state.r), (panel.g, state.g), (panel.b, state.b)];
        // Below one 8-bit step the layer could not change a pixel.
        let alpha = steps.iter().fold(1. / 255., |alpha: f32, &(from, to)| {
            // Past this opacity the channel's layer color leaves 0..=1.
            alpha.max(if to > from {
                (to - from) / (1. - from)
            } else if to < from {
                (from - to) / from
            } else {
                0.
            })
        });
        let [r, g, b] = steps.map(|(from, to)| (from + (to - from) / alpha).clamp(0., 1.));
        Rgba { r, g, b, a: alpha }
    }

    /// What the shared button ([`control_button_variant`]) shows over an
    /// opaque `surface` at rest, hovered and pressed, selected where
    /// `selected`, as GPUI blends its fill: for content drawn on the button
    /// that cuts itself out of it, as the compact Changes count's ring does.
    /// Unselected, it is transparent at rest and lays its
    /// [`Self::control_fill`] layers over the surface; selected, it is
    /// opaque `selected`, and [`control_selected_hover`] while hovered or
    /// pressed, since the kit gives a selected control no pressed fill.
    pub fn control_fills(self, selected: bool, surface: u32) -> [u32; 3] {
        if selected {
            let hovered = self.row_hover(true);
            [self.selected, hovered, hovered]
        } else {
            [
                surface,
                composite(self.control_fill(self.hover), surface),
                composite(self.control_fill(self.selected), surface),
            ]
        }
    }

    /// An enabled, unchecked Switch's track under the pointer: its resting
    /// `border` fill moved by the step a row takes from `panel` to `hover`, so
    /// the track lifts as the rows around it do. The shared button's
    /// [`Self::control_fill`] layer would shrink that step over a `border`
    /// lighter than `panel`, below a visible change in One Dark.
    pub fn switch_hover(self) -> u32 {
        [16, 8, 0].into_iter().fold(0, |color, shift| {
            let channel = |value: u32| ((value >> shift) & 0xff) as i32;
            let lifted = channel(self.border) + channel(self.hover) - channel(self.panel);
            color | ((lifted.clamp(0, 255) as u32) << shift)
        })
    }

    /// The shared button's label: `text`, unless `text` falls below the text
    /// rule over one of the fills the button composites. Then it moves toward
    /// black in a light palette and white in a dark one until it clears the rule
    /// with the rasterization margin tuned tokens keep.
    ///
    /// A fill that lifts a surface toward the label lowers the label's contrast
    /// by exactly that lift, so where `text` has little room on a row band (the
    /// hovered selected row in Kanagawa Lotus and One Dark) no opacity keeps
    /// both the lift and the rule; the label takes the difference instead. The
    /// kit paints one foreground for every state, so the resting label is the
    /// same color.
    pub fn control_label(self) -> u32 {
        let fills = [
            self.control_fill(self.hover),
            self.control_fill(self.selected),
        ];
        let surfaces = [
            self.panel,
            self.subtle,
            self.canvas,
            self.hover,
            self.selected,
            self.row_hover(true),
        ];
        let readable = |label, minimum| {
            fills.iter().all(|&fill| {
                surfaces
                    .iter()
                    .all(|&surface| custom::contrast(label, composite(fill, surface)) >= minimum)
            })
        };
        if readable(self.text, LABEL_RULE) {
            return self.text;
        }
        let extreme = if custom::luminance(self.text) < custom::luminance(self.panel) {
            0x000000
        } else {
            0xffffff
        };
        toward_extreme(self.text, extreme, |label| {
            readable(label, LABEL_RULE + LABEL_MARGIN)
        })
    }

    /// The surface of menus, popovers, selects and tooltips: the lightest of
    /// the page's surfaces (subtle, canvas and panel, the panel on a tie), so
    /// a floating surface sits raised above the page, lighter in dark and
    /// light palettes alike, as GNOME and macOS draw menus. Its edge is the
    /// `border`. Every surface rule already holds `text` and `muted` on it,
    /// and the menu's hover row keeps its step off it.
    pub fn raised(self) -> u32 {
        [self.subtle, self.canvas, self.panel]
            .into_iter()
            .max_by(|&a, &b| custom::luminance(a).total_cmp(&custom::luminance(b)))
            .unwrap_or(self.panel)
    }

    /// The edge of menus, popovers and selects: `border`, which the border
    /// rule holds only against the panel, unless it misses [`POPUP_EDGE`]
    /// against the raised surface or a page surface the popup may lie on. Then
    /// it takes the first of 256 even steps toward black in a light palette or
    /// white in a dark one that clears it on all of them, which lands it just
    /// above the floor rather than a coarse step past it.
    pub fn raised_edge(self) -> u32 {
        let surfaces = [self.raised(), self.subtle, self.canvas, self.panel];
        let clears = |edge| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(edge, surface) >= POPUP_EDGE)
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// The opacity of the black a dialog's backdrop lays over the page: the
    /// least, from the kit's own (20% in a dark palette, 5% in a light one)
    /// in steps of 1%, that leaves the dialog's surface, `canvas`, at least
    /// [`DIALOG_LIFT`] lighter than every page surface it dims (canvas, panel
    /// and subtle), so the page recedes beneath the dialog. With the kit's
    /// 20%, Midnight's dialog stood 1.03:1 over the dimmed canvas, 1.00:1
    /// over the dimmed subtle and darker than History's dimmed panel
    /// (1.03:1 the other way), so it read as a hole in the page with only
    /// its edge to mark it. Where no backdrop up to [`DIALOG_BACKDROP_MAX_PERCENT`]
    /// lifts a near-black canvas that far over a lighter panel (Omarchy
    /// ethereal and hackerman), the least that leaves the dialog no darker
    /// than any dimmed surface; where none does that either (vantablack's
    /// black canvas), the kit's own, since no more black would help. The
    /// backdrop is blended over the page's sRGB values, as it is drawn.
    pub fn dialog_backdrop(self) -> f64 {
        let lifts = |alpha: f64, lift: f64| {
            [self.canvas, self.panel, self.subtle]
                .iter()
                .all(|&surface| {
                    let dimmed = backdrop_over(surface, alpha);
                    custom::luminance(self.canvas) >= custom::luminance(dimmed)
                        && custom::contrast(self.canvas, dimmed) >= lift
                })
        };
        let base = if self.is_light() { 5 } else { 20 };
        let steps =
            || (base..=DIALOG_BACKDROP_MAX_PERCENT).map(|percent| f64::from(percent) / 100.);
        steps()
            .find(|&alpha| lifts(alpha, DIALOG_LIFT))
            .or_else(|| steps().find(|&alpha| lifts(alpha, 1.)))
            .unwrap_or(f64::from(base) / 100.)
    }

    /// The 1 px edge of a dialog's panel (the kit's `Theme::dialog_edge`) and
    /// of the bordered Cancel on it (the kit's `Theme::secondary_edge`):
    /// `border`, unless it misses [`DIALOG_EDGE`] against the dialog's
    /// surface, `canvas`, or a page surface its backdrop dims
    /// ([`Self::dialog_backdrop`]), as every palette's does: Braden's stood
    /// 1.28:1 against its dialog and 1.08:1 against the dimmed page. Then it
    /// takes the first of 256 even steps toward black in a light palette or
    /// white in a dark one, the text's side as for the popup edge, that
    /// clears it on all of them.
    pub fn dialog_edge(self) -> u32 {
        let alpha = self.dialog_backdrop();
        let surfaces = [
            self.canvas,
            backdrop_over(self.canvas, alpha),
            backdrop_over(self.panel, alpha),
            backdrop_over(self.subtle, alpha),
        ];
        let clears = |edge| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(edge, surface) >= DIALOG_EDGE)
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// The thumb of every scrollbar the app draws (the kit's
    /// `scrollbar_thumb`): `tags::ScrollCue`'s lists, the dialogs' and
    /// pickers' bodies and every other scrollbar the kit draws. It is `border`,
    /// unless that misses [`SCROLL_THUMB`] against a surface a track lies
    /// on, the canvas, the panel, `subtle` and the raised surface the
    /// pickers' tracks rest on, either as fitted or as drawn a channel step
    /// nearer that surface ([`drawn_nearer`]). Then it takes the first of 256
    /// even steps toward black in a light palette or white in a dark one, the
    /// text's side, that clears it on all of them. A thumb is the only cue
    /// that more content lies past a list's edge, and in `border` it stood
    /// 1.52:1 off Midnight's canvas, 1.39:1 off its panel and 1.28:1 off
    /// Braden's canvas. Fitted without the step, Midnight's #444f5f held
    /// 2.01:1 on its raised panel and drew #434e5e, 1.98:1, on Keyboard
    /// Shortcuts' track. Hovered, it takes `muted`, secondary text, which
    /// reads 4.5:1.
    pub fn scrollbar_thumb(self) -> u32 {
        let surfaces = [self.canvas, self.panel, self.subtle, self.raised()];
        let clears = |thumb| {
            surfaces.iter().all(|&surface| {
                custom::contrast(thumb, surface) >= SCROLL_THUMB
                    && custom::contrast(drawn_nearer(thumb, surface), surface) >= SCROLL_THUMB
            })
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// The edge of an outline drawn on a picker's selected row, such as a
    /// Keyboard Shortcuts key chip: [`Self::raised_edge`], the edge it takes
    /// on a resting row, unless that misses [`POPUP_EDGE`] against `selected`,
    /// as Midnight's #2c384a did on #223b3b (1.01:1) and Braden's #c4ced9 on
    /// #d6e7e2 (1.24:1). Then it takes the first of 256 even steps toward black
    /// in a light palette or white in a dark one, the text's side as for the
    /// raised edge, that clears it there.
    pub fn selected_edge(self) -> u32 {
        let clears = |edge| custom::contrast(edge, self.selected) >= POPUP_EDGE;
        let edge = self.raised_edge();
        if clears(edge) {
            return edge;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, edge, extreme, clears)
    }

    /// The 1 px edge of a disabled Button that rests on the neutral `subtle`
    /// fill, a primary action, a secondary control or a selected shared
    /// helper (the kit's `Theme::disabled_edge`): `border`, unless it misses
    /// [`DISABLED_EDGE`] against a surface such a Button rests on, the page's
    /// surfaces and the raised menu surface, the fill itself among them. Then
    /// it takes the first of 256 even steps toward black in a light palette or
    /// white in a dark one, the text's side as for the popup edge, that clears
    /// it on all of them. The fill stands no more than 1.34:1 off any of them,
    /// and 1.03 to 1.05:1 off a dialog in Rosé Pine, Graphite, Midnight,
    /// Braden and Ember, where a disabled default action drew only its dim
    /// label. A fill 1.5:1 off its surface would read stronger than an
    /// enabled secondary control and pull the disabled label below 3:1.
    /// Disabled filled Buttons stand on page surfaces only, so the hovered
    /// and selected rows are left out: fitting against `selected` made every
    /// edge heavier (2 to 2.4:1 on dark pages) for a case that does not
    /// occur. A future one on a row surface gets its own fit then.
    pub fn disabled_edge(self) -> u32 {
        let surfaces = [self.canvas, self.panel, self.subtle, self.raised()];
        let clears = |edge| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(edge, surface) >= DISABLED_EDGE)
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// The label of a control that cannot be used, the one dim disabled
    /// Buttons, Switch labels, menu items with their shortcuts and command
    /// palette rows share (the kit's `Theme::disabled_foreground`): a layer of
    /// `muted` at half strength, or at the first of 16 even steps toward three
    /// quarters that reads [`DISABLED_LABEL`] on every surface such a label
    /// rests on: the page's surfaces and the raised menu surface, and the
    /// translucent fill the kit's default Button (Settings' Save) takes on
    /// each of them while disabled ([`Self::default_disabled_fill`]). It keeps
    /// [`DISABLED_LABEL_MARGIN`] of room above that for 1x rendering, unless
    /// the step that does would take more than [`DISABLED_SHARE`] of enabled
    /// secondary text's contrast on one of them or on the highlighted row
    /// (`selected`), as in Kanagawa Lotus. Half of a dark secondary text reads
    /// about 2:1 on a light surface, where most dark palettes already read 3:1
    /// at half. A translucent layer still dims with whatever it lies on, and
    /// the three-quarter bound keeps it fainter than enabled secondary text
    /// even where `muted` itself is weak.
    pub fn disabled_label(self) -> Rgba {
        self.fit_disabled_label(
            DISABLED_LABEL,
            DISABLED_SHARE,
            16,
            self.default_disabled_fill(),
        )
    }

    /// [`Self::disabled_label`] under More contrast, fitted on the adjusted
    /// palette ([`Self::with_more_contrast`]): the same layer of `muted`,
    /// from half strength in up to 28 steps of 1/64 to fifteen sixteenths,
    /// that reads [`MORE_CONTRAST_DISABLED`] on the same surfaces with the
    /// same margin while keeping at most [`MORE_CONTRAST_DISABLED_SHARE`] of
    /// secondary text's contrast, so it stays fainter than the secondary
    /// text beside it. Half of a 7:1 `muted` reads about 3:1 on a dark
    /// surface, and the three quarters [`Self::disabled_label`] stops at
    /// about 4:1. The default Button's disabled fill it is measured on is
    /// `chosen`'s, the palette before the adjustment, since a fill keeps its
    /// value under More contrast ([`Self::configure`]).
    fn more_contrast_disabled_label(self, chosen: Palette) -> Rgba {
        self.fit_disabled_label(
            MORE_CONTRAST_DISABLED,
            MORE_CONTRAST_DISABLED_SHARE,
            28,
            chosen.default_disabled_fill(),
        )
    }

    /// The disabled label's fit: the first layer of `muted`, from half
    /// strength in up to `steps` steps of 1/64, that reads `floor` with
    /// [`DISABLED_LABEL_MARGIN`] on every surface a disabled label rests on
    /// (the page's, the raised one and the default Button's disabled `fill`
    /// over each) while keeping at most `share` of secondary text's contrast
    /// there and on `selected`; else the first that reads `floor`; else the
    /// last step.
    fn fit_disabled_label(self, floor: f64, share: f64, steps: u8, fill: Rgba) -> Rgba {
        let page = [self.canvas, self.panel, self.subtle, self.raised()];
        let surfaces = page
            .into_iter()
            .chain(page.map(|surface| composite(fill, surface)));
        let reading = |label, surface| custom::contrast(composite(label, surface), surface);
        let reads = |label, floor| {
            surfaces
                .clone()
                .all(|surface| reading(label, surface) >= floor)
        };
        let fainter = |label| {
            surfaces.clone().chain([self.selected]).all(|surface| {
                reading(label, surface) <= custom::contrast(self.muted, surface) * share
            })
        };
        let layer = |step: u8| Rgba {
            a: 0.5 + f32::from(step) / 64.,
            ..rgb(self.muted)
        };
        let layers = || (0..=steps).map(layer);
        layers()
            .find(|&label| reads(label, floor + DISABLED_LABEL_MARGIN) && fainter(label))
            .or_else(|| layers().find(|&label| reads(label, floor)))
            .unwrap_or(layer(steps))
    }

    /// The fill the kit's default Button (no variant: Settings' Save)
    /// paints over its surface while disabled: half of the
    /// kit's input background, which is the `input` color (the app's `border`)
    /// at 30% in a dark palette and the `background` (`canvas`) in a light
    /// one. Over Settings' panel card it is Midnight's #1a222f, where the dim
    /// fitted without it read 3.002:1. Under More contrast the kit derives
    /// the fill from the palette as chosen (`Theme::input_fill_base`,
    /// [`Self::configure`]), so the fill drawn is the unadjusted palette's.
    fn default_disabled_fill(self) -> Rgba {
        if self.is_light() {
            Rgba {
                a: 0.5,
                ..rgb(self.canvas)
            }
        } else {
            Rgba {
                a: 0.15,
                ..rgb(self.border)
            }
        }
    }

    /// This palette as More contrast draws it, the one adjustment
    /// [`Self::apply`] makes while [`MoreContrast`] is on, before it derives
    /// anything else, derived on the fly and never saved: the added and
    /// removed line tints [`Self::more_contrast_fit`] decides, and secondary
    /// text, gutter numbers and `border` raised on them
    /// ([`Self::with_raised_floors`]). Every other token stays.
    ///
    /// Built only where it is called, through [`Self::as_applied`] and by
    /// tests, and for documentation, which links to it: [`Self::apply`] takes
    /// its palette from [`Self::more_contrast_fit`] itself, so a macOS build
    /// left it never used and hosted clippy denied the warning.
    #[cfg(any(test, doc, target_os = "linux"))]
    pub fn with_more_contrast(self) -> Self {
        self.more_contrast_fit().0
    }

    /// [`Self::with_more_contrast`] with the changed-word tints decided beside its
    /// line tints: the one place that decides the diff's tints under More
    /// contrast. Each word tint drawn with More contrast off that lies beyond
    /// its line from the canvas ([`lightness_beyond`]) is lifted from its line
    /// in OKLab lightness where it stands faintest
    /// ([`EditorHighlights::lifted_word_tint`]), measured on the line tints as
    /// chosen with the syntax raised on them. Where such a word tint still
    /// stands nearer its line than [`WORD_LIGHTNESS_GAP`], that side's line
    /// tint moves away from it ([`line_moves`]), the added side first, through
    /// the steps on which every color drawn on the diff's lines and words reads
    /// at least as it does with More contrast off, with secondary text, gutter
    /// numbers and syntax raised on the moved line ([`Self::diff_reads_as_off`]),
    /// the selection and Find's matches stand apart from it as they do off
    /// ([`EditorHighlights::stay_apart_from`]), and the word stands from it in
    /// colour-blind vision at least as far as from the line drawn off
    /// ([`keeps_colour_blind_gaps`]): to the step before the first that would
    /// not, never past it. Every line of that side draws the moved
    /// tint: whole added or removed lines, changed lines around their words,
    /// in Diff and Split, and the gutter beside them. Last, a word tint that
    /// does not stand beyond its line from the canvas ([`stands_beyond`]),
    /// one drawn off on the line's canvas side, which keeps its line, or one
    /// the lift and the line's move leave nearer its line than
    /// [`HIGHLIGHT_DISTANCE`], steps to the line's far side
    /// ([`EditorHighlights::crossed_word_tint`]).
    ///
    /// Why the line: a word tint between its line and the text on it can only
    /// lift toward that text, which lowers the text's contrast, so where the
    /// word has no room left the line may still have some. Why a word on the
    /// canvas side keeps its line: the line would move away from it toward the
    /// text, lowering the text's contrast, and leave the word stepping across
    /// further. Why the step across: the order canvas, line, word is what makes
    /// a changed word read as emphasis of its line, and a word lifted toward
    /// the canvas reads as a hole in it (Omarchy flexoki-light's added word
    /// `#f7ffdf` at 1.005:1 against its canvas, Solarized Dark's `#0c2f00` at
    /// 1.017:1, apart by hue alone; decision, 2026-11-05: strictly, in every
    /// palette, where with More contrast off 29 palettes drew a word on its
    /// line's canvas side, Braden's removed word `#fcf6f7` on its line
    /// `#f9e5eb`).
    fn more_contrast_fit(self) -> (Self, Chosen) {
        let own = self.syntax_roles(None);
        let raised = self.with_raised_floors();
        let roles = raised.syntax_roles(Some(self));
        let sides = [
            (self.added, self.added_background),
            (self.removed, self.removed_background),
        ];
        let highlights = EditorHighlights::fit(self, own, None);
        let off = [highlights.added_word, highlights.removed_word];
        // A word drawn off on its line's canvas side neither lifts nor moves its line: it steps
        // across the line last.
        let beyond = [0, 1].map(|side| lightness_beyond(off[side], sides[side].1, self) > 0.);
        let words = [0, 1].map(|side| {
            let (diff, line) = sides[side];
            if beyond[side] {
                EditorHighlights::lifted_word_tint(roles, own, off[side], diff, line)
            } else {
                off[side]
            }
        });
        let with_lines = |[added_background, removed_background]: [u32; 2]| {
            Self {
                added_background,
                removed_background,
                ..self
            }
            .with_raised_floors()
        };
        let mut lines = sides.map(|(_, line)| line);
        for side in (0..2).filter(|&side| beyond[side]) {
            let off_pair = (off[side], sides[side].1);
            let moved = line_moves(lines[side], words[side], off_pair, self.canvas)
                .into_iter()
                .take_while(|&line| {
                    let mut moved = lines;
                    moved[side] = line;
                    highlights.stay_apart_from(line, sides[side].1)
                        && keeps_colour_blind_gaps(words[side], line, sides[side].1)
                        && self.diff_reads_as_off(own, with_lines(moved), off, words)
                })
                .last();
            if let Some(line) = moved {
                lines[side] = line;
            }
        }
        let palette = if lines == sides.map(|(_, line)| line) {
            raised
        } else {
            with_lines(lines)
        };
        let roles = palette.syntax_roles(Some(self));
        let patch = [roles.string, roles.removed];
        let words = [0, 1].map(|side| {
            let (diff, line) = (sides[side].0, lines[side]);
            if stands_beyond(words[side], line, self) {
                words[side]
            } else {
                EditorHighlights::crossed_word_tint(
                    roles,
                    [diff, patch[side]],
                    (line, off[side]),
                    self,
                )
            }
        });
        let chosen = Chosen {
            palette: self,
            off: highlights,
            words,
        };
        (palette, chosen)
    }

    /// Whether every color drawn on the diff's line and changed-word tints in
    /// `adjusted`, this palette under More contrast with `words` its word tints,
    /// reads there at least as the same color reads with More contrast off, on
    /// this palette's line tints and `off`, the word tints drawn off, with
    /// `own` this palette's syntax: on each line tint `text`, every syntax role,
    /// the side's diff color, secondary text and gutter numbers; on each word
    /// tint `text`, every syntax role but `removed` and the side's diff color
    /// ([`SyntaxRoles::word_colors`]).
    fn diff_reads_as_off(
        self,
        own: SyntaxRoles,
        adjusted: Self,
        off: [u32; 2],
        words: [u32; 2],
    ) -> bool {
        fn reads(mut drawn: impl Iterator<Item = (u32, u32)>, on: u32, off_on: u32) -> bool {
            drawn.all(|(is, was)| custom::contrast(is, on) >= custom::contrast(was, off_on))
        }
        let roles = adjusted.syntax_roles(Some(self));
        let on_line = |palette: Self, roles: SyntaxRoles, diff: u32| {
            roles
                .colors()
                .into_iter()
                .chain([diff, palette.muted, palette.line_number])
        };
        let on_word = |roles: SyntaxRoles, diff: u32| roles.word_colors().into_iter().chain([diff]);
        [
            (
                self.added,
                (adjusted.added_background, self.added_background),
                (words[0], off[0]),
            ),
            (
                self.removed,
                (adjusted.removed_background, self.removed_background),
                (words[1], off[1]),
            ),
        ]
        .into_iter()
        .all(|(diff, (line, off_line), (word, off_word))| {
            reads(
                on_line(adjusted, roles, diff).zip(on_line(self, own, diff)),
                line,
                off_line,
            ) && reads(on_word(roles, diff).zip(on_word(own, diff)), word, off_word)
        })
    }

    /// This palette with secondary text (`muted`) and gutter numbers
    /// (`line_number`) moved toward `text` until each reads
    /// [`MORE_CONTRAST_TEXT`] on the six surfaces the readability rules
    /// judge, measured as they rest: on canvas, panel, subtle and the
    /// selected row, and on the raised surface and the added and removed
    /// line tints. On the momentary surfaces, hover and the hovered selected
    /// row, each needs only the text rule ([`LABEL_RULE`]), so a hover alone
    /// never flattens secondary text into body text. On a surface where
    /// `text` itself reads below the floor, the floor is `text`'s own
    /// contrast, which in practice makes the token `text` where such a
    /// surface rests. `border` takes [`Self::resting_edge`], so dividers,
    /// fields' outlines and the popup edge fitted from it
    /// ([`Self::raised_edge`]) hold [`MORE_CONTRAST_EDGE`]; the disabled edge,
    /// the unchecked Switch track and a dark field's fill keep the unadjusted
    /// palette's ([`Self::configure`]). Every other token stays: the hues,
    /// the accent with its foreground, hover and pressed fills, the
    /// selection, the status and diff colours and their line tints. A token
    /// already past its floor keeps its value, and nothing the floors are
    /// measured on changes, so raising them again changes nothing.
    fn with_raised_floors(self) -> Self {
        let resting = [
            self.canvas,
            self.panel,
            self.subtle,
            self.raised(),
            self.selected,
            self.added_background,
            self.removed_background,
        ];
        let momentary = [self.hover, self.row_hover(true)];
        let floors: Vec<(u32, f64)> = resting
            .map(|surface| (surface, MORE_CONTRAST_TEXT))
            .into_iter()
            .chain(momentary.map(|surface| (surface, LABEL_RULE)))
            .collect();
        Self {
            muted: self.toward_text(self.muted, &floors),
            line_number: self.toward_text(self.line_number, &floors),
            border: self.resting_edge(),
            ..self
        }
    }

    /// `color` moved toward `text` in 256ths of each sRGB channel ([`mix`]),
    /// to the first step that reads its floor on every surface of `floors`,
    /// or `text`'s own contrast on one where that is lower. `text` reaches
    /// every floor, so the walk ends there at the latest.
    fn toward_text(self, color: u32, floors: &[(u32, f64)]) -> u32 {
        let text = custom::luminance(self.text);
        let floors: Vec<(f64, f64)> = floors
            .iter()
            .map(|&(surface, floor)| {
                let surface = custom::luminance(surface);
                let reachable = custom::luminance_contrast(text, surface);
                (surface, floor.min(reachable))
            })
            .collect();
        let reads = |color: u32| {
            let color = custom::luminance(color);
            floors
                .iter()
                .all(|&(surface, floor)| custom::luminance_contrast(color, surface) >= floor)
        };
        (0..=256)
            .map(|step| mix(color, self.text, step))
            .find(|&step| reads(step))
            .unwrap_or(self.text)
    }

    /// The 1 px edge an enabled control that rests on a fill of its own
    /// draws over its bounds while More contrast is on, and the `border`
    /// [`Self::with_more_contrast`] gives: `border`, unless it misses
    /// [`MORE_CONTRAST_EDGE`] against the canvas, the panel, `subtle` or the
    /// raised surface. Then it takes the first of 256 even steps toward black
    /// in a light palette or white in a dark one, the text's side as for the
    /// popup edge, that clears it on all of them. On an adjusted palette it
    /// is that palette's `border`.
    pub fn resting_edge(self) -> u32 {
        let surfaces = [self.canvas, self.panel, self.subtle, self.raised()];
        let clears = |edge| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(edge, surface) >= MORE_CONTRAST_EDGE)
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// A sidebar row's label and icon colors. The label stays `text`, which
    /// reads at the text rule on every row state, and selection marks the icon
    /// in `accent`, a graphic at 3:1, beside the selected surface: `accent` is
    /// fitted only to 3:1, which left a selected label at 3.0:1 in Omarchy
    /// Solitude.
    pub fn sidebar_row(self, active: bool) -> (u32, u32) {
        (self.text, if active { self.accent } else { self.muted })
    }

    /// `color` as a small label on every one of `surfaces`: badges, chips and
    /// gutter numbers at 10 to 12 px render about 0.2 below their token, so
    /// `color` stays only where it clears the text rule with the margin tuned
    /// tokens keep. Otherwise it takes the first step toward black in a light
    /// palette and white in a dark one that does, which keeps its hue.
    pub fn small_label(self, color: u32, surfaces: &[u32]) -> u32 {
        let readable = |label| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(label, surface) >= LABEL_RULE + LABEL_MARGIN)
        };
        if readable(color) {
            return color;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme(color, extreme, readable)
    }

    /// The shared button's colors: transparent at rest with its label in
    /// [`Self::control_label`], and the [`Self::control_fill`] layers of `hover`
    /// and `selected` while hovered and pressed.
    fn control_button(self, cx: &App) -> ButtonCustomVariant {
        ButtonCustomVariant::new(cx)
            .foreground(rgb(self.control_label()).into())
            .hover(self.control_fill(self.hover).into())
            .active(self.control_fill(self.selected).into())
    }

    /// [`Self::control_button`] with the kit's transparent hover.
    fn still_control_button(self, cx: &App) -> ButtonCustomVariant {
        ButtonCustomVariant::new(cx)
            .foreground(rgb(self.control_label()).into())
            .active(self.control_fill(self.selected).into())
    }

    /// The selected shared button in a palette whose control label moved: the
    /// secondary's `selected` surface with that label, and disabled, the
    /// secondary's neutral `subtle` fill, as the kit's secondary takes it. The
    /// kit's secondary paints its label in `text` for every button, and a text
    /// color on the button itself would outlast its disabled state, because the
    /// kit replays the caller's style over the disabled colors.
    fn selected_control_button(self, cx: &App) -> Option<ButtonCustomVariant> {
        let label = self.control_label();
        (label != self.text).then(|| {
            ButtonCustomVariant::new(cx)
                .foreground(rgb(label).into())
                .active(rgb(self.selected).into())
                .disabled(rgb(self.subtle).into())
        })
    }
}

/// The text rule, and the margin tuned tokens keep above it because small text
/// at 1x renders about 0.2 lower (DESIGN.md, semantic palette ownership).
const LABEL_RULE: f64 = 4.5;
const LABEL_MARGIN: f64 = 0.25;

/// The contrast a disabled label keeps on the surfaces it rests on
/// ([`Palette::disabled_label`]): readable, and well under the text rule
/// enabled labels keep, so it still reads as unavailable.
const DISABLED_LABEL: f64 = 3.0;
/// The room [`Palette::disabled_label`] keeps above [`DISABLED_LABEL`] on its
/// closest surface: a label at 1x renders below its token, so one fitted to
/// exactly 3:1 (Midnight's disabled Save, 3.002:1) read under it natively.
const DISABLED_LABEL_MARGIN: f64 = 0.05;
/// The most of enabled secondary text's contrast a disabled label keeps on a
/// surface, so disabled stays clearly fainter than enabled text.
const DISABLED_SHARE: f64 = 0.6;

/// The contrast a scrollbar's thumb keeps against every surface its track
/// lies on ([`Palette::scrollbar_thumb`]): the thumb is the only cue that
/// more content exists, so it reads plainly, and it stays well under the
/// 4.5:1 of secondary text, which its hovered state takes, so it never
/// competes with the content it scrolls.
const SCROLL_THUMB: f64 = 2.0;

/// The contrast a disabled Button's edge keeps against every surface it
/// rests on and its own fill ([`Palette::disabled_edge`]): enough that it
/// reads as a control's shape, and well under the 3:1 of the bordered Cancel
/// beside a dialog's default action, so with its dim label it still reads as
/// one that cannot act.
const DISABLED_EDGE: f64 = 1.5;

/// The contrast secondary text, gutter numbers and syntax reach under More
/// contrast ([`Palette::with_more_contrast`], [`Palette::apply`]): WCAG's
/// enhanced contrast for text (1.4.6), up from the text rule's 4.5:1 and
/// syntax's 4.75:1.
const MORE_CONTRAST_TEXT: f64 = 7.0;

/// The contrast dividers, fields' outlines and the popup, dialog, bordered
/// secondary, disabled and resting edges keep under More contrast: WCAG's
/// non-text contrast (1.4.11), up from the border rule's 1.3:1, the popup
/// edge's 1.4:1 and the disabled edge's 1.5:1.
const MORE_CONTRAST_EDGE: f64 = 3.0;

/// The contrast a disabled label reaches under More contrast
/// ([`Palette::more_contrast_disabled_label`]): the text rule, up from
/// [`DISABLED_LABEL`].
const MORE_CONTRAST_DISABLED: f64 = 4.5;
/// The most of the adjusted secondary text's contrast that label keeps on a
/// surface. Secondary text reads 7:1 there, so 4.5:1 is nearly two thirds of
/// it: the [`DISABLED_SHARE`] kept beside 4.5:1 secondary text cannot hold.
const MORE_CONTRAST_DISABLED_SHARE: f64 = 0.75;

/// The keyboard's cursor on a picker's highlighted row (the command palette,
/// Quick Open and Keyboard Shortcuts), marked as the kit's menus mark theirs
/// and as a list's selected row is: over the row's `selected` fill, 2 px of
/// `accent` along its leading edge, laid over the row, which must be
/// `relative`, so it moves nothing. The fill alone stood 1.40:1 off the raised
/// surface in Midnight and 1.28:1 in Braden; `accent` holds 3:1 on `selected`
/// and on every page surface, the raised one among them.
pub fn cursor_marker(palette: Palette) -> gpui_kit::Div {
    gpui_kit::div()
        .absolute()
        .top_0()
        .left_0()
        .bottom_0()
        .right_0()
        .border_l_2()
        .border_color(rgb(palette.accent))
}

/// The contrast a popup's 1 px edge keeps against its surface and the page
/// beneath it ([`Palette::raised_edge`]). Antialiasing and fractional scales
/// draw a hairline below its token: an edge fitted to the border rule's 1.3
/// drew 1.21:1 to 1.26:1 natively, so the token keeps room above that rule.
const POPUP_EDGE: f64 = 1.4;

/// The contrast a dialog's 1 px edge keeps against the dialog and every page
/// surface its backdrop dims, which its bordered Cancel keeps against the
/// dialog too ([`Palette::dialog_edge`]): the 3:1 a control's or a
/// container's boundary keeps (WCAG's non-text contrast). The backdrop lifts
/// a dialog as little as 1.05:1 off the page it dims, so the edge is what sets
/// the dialog apart; at the popup edge's 1.4:1 Braden's read 1.41:1 against
/// its dimmed page, and its Cancel, in `border`, 1.28:1 against the dialog.
const DIALOG_EDGE: f64 = 3.0;

/// How much lighter a dialog's surface stands than any page surface its
/// backdrop dims ([`Palette::dialog_backdrop`]): the step at which the page
/// reads as receding beneath the dialog rather than level with it.
const DIALOG_LIFT: f64 = 1.05;

/// The darkest a dialog's backdrop grows, so the page stays legible beneath
/// it.
const DIALOG_BACKDROP_MAX_PERCENT: u32 = 60;

/// `surface` under a backdrop of black at `alpha`, blended per sRGB channel.
pub(crate) fn backdrop_over(surface: u32, alpha: f64) -> u32 {
    [16, 8, 0]
        .map(|shift| {
            let channel = f64::from((surface >> shift) & 0xff) * (1. - alpha);
            (channel.round() as u32) << shift
        })
        .into_iter()
        .fold(0, |color, channel| color | channel)
}

/// The first of 32 even steps from `from` to `extreme`, `from` included, that
/// `accepts`, or `extreme`.
fn toward_extreme(from: u32, extreme: u32, accepts: impl Fn(u32) -> bool) -> u32 {
    toward_extreme_in(32, from, extreme, accepts)
}

/// `color` as the renderer may draw it over `surface`: each 8-bit channel
/// one step nearer the surface's, where they differ. A colour passes through
/// the toolkit's floating-point HSLA and the GPU's rounding on its way to the
/// screen, and Midnight's scrollbar thumb, #444f5f, drew #434e5e.
fn drawn_nearer(color: u32, surface: u32) -> u32 {
    [16, 8, 0].into_iter().fold(0, |drawn, shift| {
        let (channel, toward) = ((color >> shift) & 0xff, (surface >> shift) & 0xff);
        let channel = match channel.cmp(&toward) {
            std::cmp::Ordering::Greater => channel - 1,
            std::cmp::Ordering::Less => channel + 1,
            std::cmp::Ordering::Equal => channel,
        };
        drawn | (channel << shift)
    })
}

/// The first of `steps` even steps from `from` to `extreme`, `from` included,
/// that `accepts`, or `extreme`.
fn toward_extreme_in(steps: u32, from: u32, extreme: u32, accepts: impl Fn(u32) -> bool) -> u32 {
    (0..=steps)
        .map(|step| {
            [16, 8, 0].into_iter().fold(0, |color, shift| {
                let from = (from >> shift) & 0xff;
                let to = (extreme >> shift) & 0xff;
                color | (((from * (steps - step) + to * step) / steps) << shift)
            })
        })
        .find(|&label| accepts(label))
        .unwrap_or(extreme)
}

/// The alpha of a History reference badge's tint: its lane color over the row.
pub const REF_TINT_ALPHA: u32 = 0x1e;

/// Small labels fitted once per palette, so the rows and gutters that draw them
/// read a color instead of fitting one every frame. [`Palette::apply`]
/// installs them; [`labels`] refits for a palette applied some other way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Labels {
    palette: Palette,
    /// A History reference badge's label for each graph lane, on the lane's
    /// tint over every History row surface: the canvas, hovered, selected and
    /// hovered selected rows.
    pub refs: [u32; 6],
    /// The Unstaged badge's label: `modified` on `subtle`.
    pub unstaged: u32,
    /// `added` on its diff tile, `added_background`: the gutter numbers of
    /// added lines and the Staged and "N staged" badges.
    pub added: u32,
    /// `removed` on its diff tile: the gutter numbers of removed lines.
    pub removed: u32,
    /// A count chip's number: `text` on its `hover` fill.
    pub count: u32,
    /// A failure message beside the control that failed, such as a refused
    /// Open in default app: `warning` on the canvas, panel and raised
    /// surfaces a comparison and its dialogs rest on.
    pub warning: u32,
}

impl Global for Labels {}

impl Labels {
    pub fn fit(palette: Palette) -> Self {
        let rows = [
            palette.canvas,
            palette.hover,
            palette.selected,
            palette.row_hover(true),
        ];
        let refs = crate::graph::palette_colors(palette).map(|lane| {
            let tint = Rgba {
                a: REF_TINT_ALPHA as f32 / 255.,
                ..rgb(lane)
            };
            palette.small_label(lane, &rows.map(|row| composite(tint, row)))
        });
        Self {
            palette,
            refs,
            unstaged: palette.small_label(palette.modified, &[palette.subtle]),
            added: palette.small_label(palette.added, &[palette.added_background]),
            removed: palette.small_label(palette.removed, &[palette.removed_background]),
            count: palette.small_label(palette.text, &[palette.hover]),
            warning: palette.small_label(
                palette.warning,
                &[palette.canvas, palette.panel, palette.raised()],
            ),
        }
    }
}

/// The applied palette's [`Labels`].
pub fn labels(cx: &App) -> Labels {
    let palette = palette(cx);
    cx.try_global::<Labels>()
        .copied()
        .filter(|labels| labels.palette == palette)
        .unwrap_or_else(|| Labels::fit(palette))
}

/// Composite a translucent layer over an opaque surface as GPUI's renderers
/// blend a quad: on the gamma-encoded values of their non-sRGB targets.
pub(crate) fn composite(layer: Rgba, surface: u32) -> u32 {
    let beneath = rgb(surface);
    [
        (layer.r, beneath.r, 16),
        (layer.g, beneath.g, 8),
        (layer.b, beneath.b, 0),
    ]
    .into_iter()
    .fold(0, |color, (top, bottom, shift)| {
        let value = (layer.a * top + (1. - layer.a) * bottom) * 255.;
        color | ((value.round() as u32) << shift)
    })
}

/// Every palette a theme-wide test judges: each built-in by its label and, on Linux, the
/// palette each embedded Omarchy fixture maps to, as `Omarchy <file name>`.
#[cfg(test)]
pub(crate) fn test_palettes() -> Vec<(String, Palette)> {
    let built_in = ThemeChoice::ALL
        .into_iter()
        .map(|choice| (choice.label().to_owned(), choice.palette()));
    #[cfg(target_os = "linux")]
    let omarchy = omarchy::fixture_palettes()
        .into_iter()
        .map(|(name, palette)| (format!("Omarchy {name}"), palette));
    #[cfg(not(target_os = "linux"))]
    let omarchy = std::iter::empty();
    built_in.chain(omarchy).collect()
}

/// A painted color as `0xRRGGBB`, its alpha dropped.
#[cfg(test)]
pub(crate) fn packed(color: gpui_kit::Hsla) -> u32 {
    let color = color.to_rgb();
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u32;
    (channel(color.r) << 16) | (channel(color.g) << 8) | channel(color.b)
}

/// What the last frame painted at a row's `bounds` (logical pixels, within a
/// device pixel): its fill, and a leading marker, a border on the leading side
/// alone, as its color and logical width. A border-only quad is painted as a
/// copy per side, which count once.
#[cfg(test)]
pub(crate) fn painted_row(
    cx: &mut gpui_kit::VisualTestContext,
    bounds: gpui_kit::Bounds<Pixels>,
) -> (Vec<gpui_kit::Background>, Vec<(gpui_kit::Hsla, Pixels)>) {
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let near = |device: gpui_kit::ScaledPixels, logical: Pixels| {
            (device.as_f32() - logical.as_f32() * scale).abs() <= 1.
        };
        let (mut fills, mut markers) = (Vec::new(), Vec::new());
        for quad in window.painted_quads() {
            let at = near(quad.bounds.left(), bounds.left())
                && near(quad.bounds.top(), bounds.top())
                && near(quad.bounds.right(), bounds.right())
                && near(quad.bounds.bottom(), bounds.bottom());
            if !at {
                continue;
            }
            if !quad.background.is_transparent() && !fills.contains(&quad.background) {
                fills.push(quad.background);
            }
            let widths = quad.border_widths;
            let leading = widths.left.as_f32() > 0.
                && [widths.top, widths.right, widths.bottom]
                    .into_iter()
                    .all(|width| width.as_f32() == 0.);
            let marker = (quad.border_color, px(widths.left.as_f32() / scale));
            if leading && !quad.border_color.is_transparent() && !markers.contains(&marker) {
                markers.push(marker);
            }
        }
        (fills, markers)
    })
}

/// A bordered quad the last frame painted inside a control's bounds.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintedEdge {
    /// Its border's color, as `0xRRGGBB` composited over `beneath`.
    pub(crate) color: u32,
    /// Whether every side is one logical pixel wide, to the device pixel.
    pub(crate) one_px: bool,
    pub(crate) bounds: gpui_kit::Bounds<Pixels>,
    /// The opaque surface painted beneath it: the topmost solid quad
    /// drawn before it that holds its bounds and is larger than them.
    pub(crate) beneath: Option<u32>,
}

/// Each bordered quad the last frame painted inside `bounds` (logical
/// pixels, within a device pixel), once: GPUI paints a border over a
/// transparent fill as a copy per side. A focus ring lies outside a
/// control's bounds, so it is not among them.
#[cfg(test)]
pub(crate) fn painted_edges(
    cx: &mut gpui_kit::VisualTestContext,
    bounds: gpui_kit::Bounds<Pixels>,
) -> Vec<PaintedEdge> {
    use gpui_kit::{Bounds, ScaledPixels, point};
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |b: Bounds<ScaledPixels>| {
            Bounds::from_corners(
                point(px(b.left().as_f32() / scale), px(b.top().as_f32() / scale)),
                point(
                    px(b.right().as_f32() / scale),
                    px(b.bottom().as_f32() / scale),
                ),
            )
        };
        let device = px(1. / scale);
        let holds = |outer: Bounds<Pixels>, inner: Bounds<Pixels>| {
            inner.left() >= outer.left() - device
                && inner.top() >= outer.top() - device
                && inner.right() <= outer.right() + device
                && inner.bottom() <= outer.bottom() + device
        };
        let larger = |outer: Bounds<Pixels>, inner: Bounds<Pixels>| {
            holds(outer, inner) && !holds(inner, outer)
        };
        let quads = window.painted_quads();
        let mut edges = Vec::new();
        for quad in &quads {
            let drawn = logical(quad.bounds);
            let widths = quad.border_widths;
            let sides = [widths.top, widths.right, widths.bottom, widths.left];
            if !sides.iter().any(|width| width.as_f32() > 0.) || !holds(bounds, drawn) {
                continue;
            }
            let beneath = quads
                .iter()
                .filter(|under| {
                    under.order < quad.order
                        && under.background.as_solid().is_some_and(|fill| fill.a >= 1.)
                        && larger(logical(under.bounds), drawn)
                })
                .max_by_key(|under| under.order)
                .and_then(|under| under.background.as_solid())
                .map(packed);
            let border = quad.border_color.to_rgb();
            let edge = PaintedEdge {
                color: beneath.map_or(packed(quad.border_color), |surface| {
                    composite(border, surface)
                }),
                one_px: sides
                    .iter()
                    .all(|width| (width.as_f32() - scale).abs() < 0.5),
                bounds: drawn,
                beneath,
            };
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
        edges
    })
}

/// How the keyboard's cursor painted at `bounds` misses its contract in
/// `palette`, named `name`: the `selected` fill, and one leading marker at
/// least 2 px wide at 3:1 or more against that fill and against the raised
/// surface of the menu or picker it lies on (`Palette::raised`).
#[cfg(test)]
pub(crate) fn cursor_issues(
    cx: &mut gpui_kit::VisualTestContext,
    bounds: gpui_kit::Bounds<Pixels>,
    palette: Palette,
    name: &str,
) -> Vec<String> {
    let (fills, markers) = painted_row(cx, bounds);
    let mut issues = Vec::new();
    let selected = gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(palette.selected)));
    if fills != [selected] {
        issues.push(format!("{name}: cursor fills {fills:?}, not selected"));
    }
    match markers[..] {
        [(color, width)] if width >= px(2.) => {
            for (what, beneath) in [("fill", palette.selected), ("surface", palette.raised())] {
                let ratio = custom::contrast(packed(color), beneath);
                if ratio < 3. {
                    issues.push(format!(
                        "{name}: marker #{:06x} on the {what} #{beneath:06x} {ratio:.2}:1",
                        packed(color)
                    ));
                }
            }
        }
        _ => issues.push(format!("{name}: cursor markers {markers:?}")),
    }
    issues
}

pub fn palette(cx: &App) -> Palette {
    cx.try_global::<Palette>()
        .copied()
        .unwrap_or_else(|| ThemeChoice::default().palette())
}

/// Whether the applied palette takes More contrast
/// ([`Palette::with_more_contrast`]): application state, set from the
/// desktop's or Settings' answer through [`set_more_contrast`] and never
/// saved. Absent, it is off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MoreContrast(pub bool);

impl Global for MoreContrast {}

/// Whether More contrast is on.
pub fn more_contrast(cx: &App) -> bool {
    cx.try_global::<MoreContrast>().is_some_and(|more| more.0)
}

/// Stores whether More contrast is on and returns whether that changed. It
/// applies nothing: a caller that sees a change applies the theme again, as
/// callers of `set_desktop_text_scale` redraw, so one application shows it.
pub fn set_more_contrast(on: bool, cx: &mut App) -> bool {
    if more_contrast(cx) == on {
        return false;
    }
    cx.set_global(MoreContrast(on));
    true
}

impl ThemeChoice {
    pub const ALL: [Self; 20] = [
        Self::Midnight,
        Self::Daylight,
        Self::Graphite,
        Self::TokyoNight,
        Self::CatppuccinMocha,
        Self::Nord,
        Self::Porcelain,
        Self::Sandstone,
        Self::DeepSea,
        Self::Ember,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::OneDark,
        Self::OneLight,
        Self::RosePine,
        Self::RosePineDawn,
        Self::Dracula,
        Self::Alucard,
        Self::KanagawaWave,
        Self::KanagawaLotus,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Graphite => "Graphite",
            Self::Daylight => "Braden",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Nord => "Nord",
            Self::Porcelain => "Porcelain",
            Self::Sandstone => "Sandstone",
            Self::DeepSea => "Deep Sea",
            Self::Ember => "Ember",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::OneDark => "One Dark",
            Self::OneLight => "One Light",
            Self::RosePine => "Rosé Pine",
            Self::RosePineDawn => "Rosé Pine Dawn",
            Self::Dracula => "Dracula",
            Self::Alucard => "Alucard",
            Self::KanagawaWave => "Kanagawa Wave",
            Self::KanagawaLotus => "Kanagawa Lotus",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Midnight => "Deep slate · mint",
            Self::Daylight => "Soft white · evergreen",
            Self::Graphite => "Warm charcoal · lilac",
            Self::TokyoNight => "City blues · neon",
            Self::CatppuccinMocha => "Cozy pastels · mauve",
            Self::Nord => "Arctic blue · frost",
            Self::Porcelain => "Cool ivory · sapphire",
            Self::Sandstone => "Warm paper · terracotta",
            Self::DeepSea => "Ocean ink · turquoise",
            Self::Ember => "Smoked plum · apricot",
            Self::SolarizedDark => "Deep teal · azure",
            Self::SolarizedLight => "Warm cream · azure",
            Self::OneDark => "Soft charcoal · sky",
            Self::OneLight => "Clean paper · cobalt",
            Self::RosePine => "Dusky violet · rose",
            Self::RosePineDawn => "Blush paper · pine",
            Self::Dracula => "Night charcoal · purple",
            Self::Alucard => "Pale parchment · violet",
            Self::KanagawaWave => "Inky dusk · cornflower",
            Self::KanagawaLotus => "Rice paper · denim",
        }
    }

    pub fn is_light(self) -> bool {
        matches!(
            self,
            Self::Daylight
                | Self::Porcelain
                | Self::Sandstone
                | Self::SolarizedLight
                | Self::OneLight
                | Self::RosePineDawn
                | Self::Alucard
                | Self::KanagawaLotus
        )
    }

    pub fn palette(self) -> Palette {
        match self {
            // Original GitTurtle palettes, complete semantic surface sets.
            Self::Porcelain => Palette {
                canvas: 0xf6f7fc,
                panel: 0xffffff,
                subtle: 0xeff1f8,
                // Tuned: 1.08:1 off subtle, as the Omarchy fitter holds hover.
                hover: 0xe4e8f3,
                border: 0xcbd3e4,
                text: 0x242e49,
                muted: 0x4d5b78,
                accent: 0x3455a6,
                accent_foreground: 0xffffff,
                accent_hover: 0x294790,
                accent_active: 0x203978,
                // Leans toward the sapphire accent so a pressed button stands apart from
                // its hover on every surface (the pressed-step readability rule).
                selected: 0xdce6f6,
                added: 0x246448,
                removed: 0xa92d4e,
                modified: 0x795314,
                renamed: 0x6c459a,
                warning: 0x795314,
                added_background: 0xe3f0e9,
                removed_background: 0xf8e5ed,
                hunk: 0x3455a6,
                line_number: 0x5f6c86,
            },
            Self::Sandstone => Palette {
                canvas: 0xf8f3ea,
                panel: 0xfffcf6,
                subtle: 0xf0eade,
                hover: 0xeae1d3,
                border: 0xd4c6b5,
                text: 0x3b302b,
                muted: 0x635446,
                accent: 0x965034,
                accent_foreground: 0xffffff,
                accent_hover: 0x82432b,
                accent_active: 0x6e3723,
                // Leans toward the terracotta accent so a pressed button stands apart from
                // its hover on every surface (the pressed-step readability rule).
                selected: 0xf2dcd0,
                added: 0x396241,
                removed: 0xa13243,
                modified: 0x755012,
                renamed: 0x794a84,
                warning: 0x755012,
                added_background: 0xe7efdc,
                removed_background: 0xf6e3dd,
                hunk: 0x365e8b,
                line_number: 0x70614f,
            },
            Self::DeepSea => Palette {
                canvas: 0x0d1c27,
                panel: 0x132735,
                subtle: 0x10222f,
                hover: 0x213b4b,
                border: 0x365366,
                text: 0xe4f2f7,
                muted: 0xb2c9d6,
                accent: 0x68dccb,
                accent_foreground: 0x072d2c,
                accent_hover: 0x91e9dc,
                accent_active: 0x58c9b9,
                selected: 0x21434c,
                added: 0x83d8ae,
                removed: 0xf7a0ad,
                modified: 0xe9ca8a,
                renamed: 0xc5b2f0,
                warning: 0xe9ca8a,
                added_background: 0x183c35,
                removed_background: 0x3b2c3b,
                hunk: 0x94c9f4,
                line_number: 0x9bb7c9,
            },
            Self::Ember => Palette {
                canvas: 0x201a22,
                panel: 0x2a222c,
                subtle: 0x251e27,
                hover: 0x3b303d,
                border: 0x514052,
                text: 0xf7ece5,
                muted: 0xd0bfc7,
                accent: 0xf2b38c,
                accent_foreground: 0x382119,
                accent_hover: 0xffcba6,
                accent_active: 0xe3a27a,
                selected: 0x48343d,
                added: 0xadd3a5,
                removed: 0xf2a2b2,
                modified: 0xe8c88b,
                renamed: 0xd1b0ef,
                warning: 0xe8c88b,
                added_background: 0x303b2e,
                removed_background: 0x472b37,
                hunk: 0xb4c7ee,
                line_number: 0xbda6b6,
            },
            Self::Midnight => Palette {
                canvas: 0x10151f,
                panel: 0x171e2b,
                subtle: 0x131a25,
                hover: 0x222c3c,
                border: 0x2b3749,
                text: 0xe8eef7,
                muted: 0xa4b1c5,
                accent: 0x75e0bb,
                accent_foreground: 0x0a241d,
                accent_hover: 0x99edcf,
                accent_active: 0x5ccca6,
                selected: 0x223b3b,
                added: 0x75e0bb,
                removed: 0xff95a8,
                modified: 0xe8bc79,
                renamed: 0xc1a5f5,
                warning: 0xe8bc79,
                added_background: 0x19322d,
                removed_background: 0x382531,
                hunk: 0x95baff,
                line_number: 0x899bb5,
            },
            Self::Graphite => Palette {
                canvas: 0x18191d,
                panel: 0x202126,
                subtle: 0x1c1d22,
                hover: 0x2b2c33,
                border: 0x3b3d47,
                text: 0xefeff4,
                muted: 0xa8a9b8,
                accent: 0xb7a5ff,
                accent_foreground: 0x211936,
                accent_hover: 0xcabaff,
                accent_active: 0xa994f0,
                selected: 0x38324e,
                added: 0x91ddb3,
                removed: 0xf5a0ac,
                modified: 0xe8c58c,
                renamed: 0xb7a5ff,
                warning: 0xe8c58c,
                added_background: 0x22352c,
                removed_background: 0x3e2830,
                hunk: 0xa8bfff,
                line_number: 0x9b9daa,
            },
            Self::Daylight => Palette {
                canvas: 0xf4f6fa,
                panel: 0xffffff,
                subtle: 0xedf1f7,
                // Tuned: 1.08:1 off subtle, as the Omarchy fitter holds hover.
                hover: 0xe3e8ef,
                border: 0xd2dce8,
                text: 0x253247,
                // Tuned: 5 darker per channel for 4.75:1 on the darker selected rows.
                muted: 0x4d5d74,
                accent: 0x08755d,
                accent_foreground: 0xffffff,
                accent_hover: 0x06654f,
                accent_active: 0x055440,
                // Tuned: darkened in linear light, keeping its hue, so a pressed
                // control reads darker than its hover, at least 10 from it.
                selected: 0xd6e7e2,
                added: 0x146744,
                removed: 0xb53351,
                modified: 0x875a14,
                renamed: 0x7850b4,
                warning: 0x875a14,
                added_background: 0xe2f1e9,
                removed_background: 0xf9e5eb,
                hunk: 0x3764ae,
                line_number: 0x5b6c84,
            },
            // Base hues follow the original palette; secondary text and status
            // surfaces are tuned for readable, small native UI labels.
            // https://github.com/tokyo-night/tokyo-night-vscode-theme
            Self::TokyoNight => Palette {
                canvas: 0x1a1b26,
                panel: 0x202231,
                subtle: 0x16161e,
                hover: 0x292e42,
                border: 0x373e59,
                text: 0xc0caf5,
                muted: 0xa9b1d6,
                accent: 0x7aa2f7,
                accent_foreground: 0x151c30,
                accent_hover: 0x99b9ff,
                accent_active: 0x7299e8,
                selected: 0x2c3552,
                added: 0x9ece6a,
                removed: 0xf7768e,
                modified: 0xe0af68,
                renamed: 0xbb9af7,
                warning: 0xe0af68,
                added_background: 0x26362c,
                removed_background: 0x3c2638,
                hunk: 0x7dcfff,
                line_number: 0x8997bd,
            },
            // https://catppuccin.com/palette/#mocha
            Self::CatppuccinMocha => Palette {
                canvas: 0x1e1e2e,
                panel: 0x242436,
                subtle: 0x181825,
                hover: 0x313244,
                border: 0x45475a,
                text: 0xcdd6f4,
                muted: 0xa6adc8,
                accent: 0xcba6f7,
                accent_foreground: 0x251b33,
                accent_hover: 0xddc0ff,
                accent_active: 0xba97e8,
                selected: 0x36324b,
                added: 0xa6e3a1,
                removed: 0xf38ba8,
                modified: 0xf9e2af,
                renamed: 0xcba6f7,
                warning: 0xf9e2af,
                added_background: 0x28392f,
                removed_background: 0x3e293d,
                hunk: 0x89b4fa,
                line_number: 0x9399b2,
            },
            // https://www.nordtheme.com/docs/colors-and-palettes
            Self::Nord => Palette {
                canvas: 0x2e3440,
                panel: 0x343c4b,
                subtle: 0x292f3b,
                hover: 0x3e4555,
                border: 0x4c566a,
                text: 0xeceff4,
                muted: 0xc0c9d8,
                accent: 0x88c0d0,
                accent_foreground: 0x243039,
                accent_hover: 0xa1d3df,
                accent_active: 0x80b5c7,
                selected: 0x3b485c,
                added: 0xa3be8c,
                removed: 0xe89aa3,
                modified: 0xebcb8b,
                renamed: 0xd0acd0,
                warning: 0xebcb8b,
                added_background: 0x303e39,
                removed_background: 0x45333f,
                hunk: 0x88c0d0,
                line_number: 0xa7b4c9,
            },
            // Families adapted from `sources`. Named constants are upstream
            // values used unchanged; hex literals marked "tuned" depart from
            // the upstream value for a readability rule, with the margin 1x
            // rasterization needs, and are listed in DESIGN.md. Unmarked
            // literals are derived surfaces the upstream palette does not
            // define (subtle, hover, selected, diff tiles, accent states).
            Self::SolarizedDark => {
                use sources::solarized::{self as s, dark};
                Palette {
                    canvas: dark::CANVAS,
                    panel: dark::PANEL,
                    subtle: 0x01313d,
                    hover: 0x103c48,
                    selected: 0x0b4154,
                    border: s::BASE01,
                    // Tuned: base0 and base1 lightened for 4.5:1 on selected rows.
                    text: 0xb2bdbe,
                    muted: 0xaab5b5,
                    // Tuned: blue lightened for 3:1 on selected and hovered rows.
                    accent: 0x3499df,
                    accent_foreground: 0x001e26,
                    accent_hover: 0x48a0de,
                    accent_active: 0x278ed6,
                    // Tuned: green, red and violet lightened for 3:1 on row
                    // surfaces, 4.5:1 in diff tiles and the canvas label; yellow
                    // for 4.5:1 as warning text on subtle surfaces, and modified
                    // keeps warning's value.
                    added: 0x92a802,
                    removed: 0xea706e,
                    modified: 0xbe9209,
                    renamed: 0x898dd2,
                    warning: 0xbe9209,
                    added_background: 0x103830,
                    removed_background: 0x1a2c35,
                    // Tuned: cyan lightened for 4.5:1 on panels.
                    hunk: 0x30aea5,
                    // Tuned: base01 lightened for 4.5:1 in the gutter.
                    line_number: 0x8aa0a8,
                }
            }
            Self::SolarizedLight => {
                use sources::solarized::{self as s, light};
                Palette {
                    canvas: light::CANVAS,
                    panel: light::PANEL,
                    subtle: 0xf6efdc,
                    hover: 0xe3dfcf,
                    selected: 0xd0dad5,
                    border: s::BASE1,
                    // Tuned: base00 and base01 darkened for 4.5:1 on selected rows.
                    text: 0x394549,
                    muted: 0x44565c,
                    // Tuned: blue darkened for a 4.5:1 white label and 3:1 on rows.
                    accent: 0x1c73b1,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x1d6aa0,
                    accent_active: 0x1a5e8f,
                    // Tuned: every accent darkened for 3:1 on row surfaces,
                    // 4.5:1 in diff tiles, the canvas label on fills and warning
                    // text on subtle surfaces.
                    added: 0x5b6900,
                    removed: 0xc2201e,
                    modified: 0x846200,
                    renamed: 0x6166bd,
                    warning: 0x846200,
                    added_background: 0xece9c3,
                    removed_background: 0xfae2d1,
                    // Tuned: blue darkened for 4.5:1 on resting surfaces and 3:1 on rows.
                    hunk: 0x19689e,
                    // Tuned: base01 darkened for 4.5:1 on panels.
                    line_number: 0x51666d,
                }
            }
            Self::OneDark => {
                use sources::one::dark as o;
                Palette {
                    canvas: o::BG,
                    panel: 0x2e333d,
                    subtle: 0x21252b,
                    hover: 0x333943,
                    selected: 0x323d52,
                    border: 0x4b5263,
                    // Tuned: mono-1 and mono-2 lightened for 4.5:1 on the hovered selected
                    // row at 1x; text is kept above secondary text.
                    text: 0xaeb5c2,
                    muted: 0xb0b5bc,
                    accent: o::BLUE,
                    accent_foreground: 0x1b2533,
                    accent_hover: 0x7dbdf2,
                    accent_active: 0x53a8ee,
                    added: o::GREEN,
                    // Tuned: red-1 lightened for 4.5:1 in its diff tile and under the canvas label.
                    removed: 0xe98991,
                    modified: o::ORANGE_2,
                    renamed: o::PURPLE,
                    warning: o::ORANGE_2,
                    added_background: 0x353e3c,
                    removed_background: 0x3e343c,
                    hunk: o::CYAN,
                    // Tuned: mono-2 lightened for 4.5:1 in the gutter.
                    line_number: 0x9aa0ab,
                }
            }
            Self::OneLight => {
                use sources::one::light as o;
                Palette {
                    canvas: o::BG,
                    panel: 0xffffff,
                    subtle: 0xf0f0f1,
                    // Tuned: 1.08:1 off subtle, as the Omarchy fitter holds hover.
                    hover: 0xe7e7e9,
                    // Tuned: stepped darker from #e9edff until hover reads 0.005 lighter in OKLab,
                    // so a hovered row never reads heavier than the selected one.
                    selected: 0xe0e4f6,
                    border: 0xd3d3d6,
                    text: o::MONO_1,
                    // Tuned: mono-2 darkened for 4.5:1 on selected rows.
                    muted: 0x5a5d67,
                    // Tuned: blue darkened for a 4.5:1 white label.
                    accent: 0x2d6aef,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x175bef,
                    accent_active: 0x0f52e3,
                    // Tuned: green darkened for 3:1 on rows and 4.5:1 in diff tiles.
                    added: 0x377236,
                    removed: o::RED_2,
                    // Tuned: orange-1 darkened for 4.5:1 as warning text on subtle
                    // surfaces; modified keeps warning's value.
                    modified: 0x8e5e00,
                    renamed: o::PURPLE,
                    warning: 0x8e5e00,
                    added_background: 0xe6efe5,
                    removed_background: 0xf6e7eb,
                    // Tuned: cyan darkened for 4.5:1 on subtle surfaces.
                    hunk: 0x0070a1,
                    line_number: o::MONO_2,
                }
            }
            Self::RosePine => {
                use sources::rose_pine::main as r;
                Palette {
                    canvas: r::BASE,
                    panel: r::SURFACE,
                    subtle: 0x16141f,
                    hover: r::OVERLAY,
                    selected: 0x2d2a45,
                    border: 0x403d52,
                    text: r::TEXT,
                    // Tuned: subtle lightened for 4.5:1 on selected rows and diff tiles.
                    muted: 0xa6a2bc,
                    accent: r::ROSE,
                    accent_foreground: 0x2a1d25,
                    accent_hover: 0xf3cfcd,
                    accent_active: 0xe2aeac,
                    added: r::FOAM,
                    removed: r::LOVE,
                    modified: r::GOLD,
                    renamed: r::IRIS,
                    warning: r::GOLD,
                    added_background: 0x1f2e36,
                    removed_background: 0x351f30,
                    hunk: r::IRIS,
                    line_number: r::SUBTLE,
                }
            }
            Self::RosePineDawn => {
                use sources::rose_pine::dawn as r;
                Palette {
                    canvas: r::BASE,
                    panel: r::SURFACE,
                    subtle: 0xf4ede4,
                    // Tuned: overlay darkened for 1.08:1 off subtle, as the Omarchy fitter holds hover.
                    hover: 0xece3dc,
                    // Tuned: a step darker and toward iris, so a pressed control
                    // stands at least 10 from its hover, darker than it.
                    selected: 0xeadde7,
                    border: 0xdfdad9,
                    text: r::TEXT,
                    // Tuned: subtle darkened for 4.5:1 on every row surface and diff tile.
                    muted: 0x59566e,
                    accent: r::PINE,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x225a70,
                    accent_active: 0x1d4d60,
                    // Tuned: foam, love, gold and iris darkened for 3:1 on row
                    // surfaces, 4.5:1 in diff tiles, the canvas label on fills and
                    // warning text on subtle surfaces.
                    added: 0x3c6b74,
                    removed: 0x934e62,
                    modified: 0x8e5c18,
                    renamed: 0x806b97,
                    warning: 0x8e5c18,
                    added_background: 0xe4ecea,
                    removed_background: 0xf6e3e3,
                    hunk: r::PINE,
                    // Tuned: subtle darkened for 4.5:1 on canvas.
                    line_number: 0x6b6783,
                }
            }
            Self::Dracula => {
                use sources::dracula::dark as d;
                Palette {
                    canvas: d::BACKGROUND,
                    panel: 0x2e303e,
                    subtle: 0x21222c,
                    hover: 0x383a4a,
                    selected: d::SELECTION,
                    border: 0x4a4d62,
                    text: d::FOREGROUND,
                    // Tuned: comment lightened for 4.5:1 on every row surface and diff tile.
                    muted: 0xbdc4db,
                    accent: d::PURPLE,
                    accent_foreground: d::BACKGROUND,
                    accent_hover: 0xcfaefb,
                    accent_active: 0xb083f7,
                    added: d::GREEN,
                    // Tuned: red lightened for 3:1 on selected rows and 4.5:1 in its diff tile.
                    removed: 0xff7979,
                    modified: d::ORANGE,
                    renamed: d::PURPLE,
                    warning: d::ORANGE,
                    added_background: 0x2b4136,
                    removed_background: 0x472e3a,
                    hunk: d::CYAN,
                    // Tuned: comment lightened for 4.5:1 in the gutter.
                    line_number: 0x909cc1,
                }
            }
            Self::Alucard => {
                use sources::dracula::alucard as a;
                Palette {
                    canvas: a::BACKGROUND,
                    panel: 0xfffdf5,
                    subtle: 0xf5f1e1,
                    // Tuned: 1.08:1 off subtle, as the Omarchy fitter holds hover.
                    hover: 0xebe8d8,
                    selected: a::SELECTION,
                    border: 0xd9d4bf,
                    text: a::FOREGROUND,
                    // Tuned: comment darkened for 4.5:1 on selected and hovered selected rows.
                    muted: 0x534e38,
                    accent: a::PURPLE,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x563cb8,
                    accent_active: 0x4a32a2,
                    added: a::GREEN,
                    // Tuned: red darkened for 3:1 on hovered selected rows and 4.5:1 in its diff tile.
                    removed: 0xba3223,
                    modified: a::ORANGE,
                    renamed: a::PURPLE,
                    warning: a::ORANGE,
                    added_background: 0xe3f0da,
                    removed_background: 0xfbe3dc,
                    hunk: a::CYAN,
                    line_number: a::COMMENT,
                }
            }
            Self::KanagawaWave => {
                use sources::kanagawa::wave as k;
                Palette {
                    canvas: k::SUMI_INK_3,
                    panel: k::SUMI_INK_4,
                    subtle: k::SUMI_INK_2,
                    // Tuned: sumiInk5 darkened below the selected row's lightness, so a hovered
                    // row never reads heavier than the selected one.
                    hover: 0x323242,
                    // Tuned: waveBlue1 lightened to stay 1.15:1 apart from the panel.
                    selected: 0x24364e,
                    border: k::SUMI_INK_6,
                    text: k::FUJI_WHITE,
                    muted: k::OLD_WHITE,
                    accent: k::CRYSTAL_BLUE,
                    accent_foreground: k::SUMI_INK_3,
                    accent_hover: 0x94aee0,
                    accent_active: 0x6d8dce,
                    // Tuned: autumnGreen lightened for 4.5:1 in its diff tile.
                    added: 0x89a47e,
                    removed: k::PEACH_RED,
                    modified: k::AUTUMN_YELLOW,
                    renamed: k::ONI_VIOLET,
                    warning: k::RONIN_YELLOW,
                    added_background: k::WINTER_GREEN,
                    removed_background: k::WINTER_RED,
                    hunk: k::SPRING_BLUE,
                    // Tuned: sumiInk6 lightened for 4.5:1 in the gutter.
                    line_number: 0x9494ad,
                }
            }
            Self::KanagawaLotus => {
                use sources::kanagawa::lotus as k;
                Palette {
                    canvas: k::LOTUS_WHITE_3,
                    panel: 0xf7f3d1,
                    subtle: k::LOTUS_WHITE_2,
                    hover: k::LOTUS_WHITE_1,
                    // Tuned: lotusBlue1 stepped darker until hover reads 0.005 lighter in OKLab
                    // on the canvas, panel and subtle, so a hovered row never reads heavier than
                    // the selected one.
                    selected: 0xc4d4dd,
                    border: k::LOTUS_WHITE_0,
                    // Tuned: lotusInk1 darkened to stay above secondary text, which keeps its
                    // margin on the darker hovered selected row.
                    text: 0x4f4f5f,
                    // Tuned: lotusGray2 darkened for 4.5:1 on every row surface and diff tile,
                    // with the margin on the hovered selected row.
                    muted: 0x545147,
                    accent: k::LOTUS_BLUE_4,
                    accent_foreground: k::LOTUS_WHITE_3,
                    accent_hover: 0x435c89,
                    accent_active: 0x3a5077,
                    // Tuned: lotusGreen2, lotusRed2, lotusYellow3 and lotusOrange2 darkened for
                    // 3:1 on row surfaces, 4.5:1 in diff tiles, the canvas label on fills and
                    // warning text on subtle surfaces.
                    added: 0x49613e,
                    removed: 0xa72428,
                    modified: 0x916100,
                    renamed: k::LOTUS_VIOLET_4,
                    warning: 0x8b4c00,
                    // Tuned: lotusGreen3 blended halfway to the canvas and lotusRed4 a little
                    // past halfway, so text keeps 4.5:1 inside diff tiles.
                    added_background: 0xd4deb5,
                    removed_background: 0xeed0b0,
                    // Tuned: lotusBlue4 darkened for 4.5:1 on subtle surfaces.
                    hunk: 0x425c8b,
                    // Tuned: lotusGray2 darkened for 4.5:1 on the canvas.
                    line_number: 0x676458,
                }
            }
        }
    }

    /// Apply this built-in theme through the one palette application path. The app
    /// applies a `ResolvedTheme`; test fixtures apply a built-in directly.
    #[cfg(test)]
    pub fn apply(self, window: Option<&mut Window>, cx: &mut App) {
        custom::ResolvedTheme::built_in(self).apply(window, cx);
    }
}

impl custom::ThemeSelection {
    /// The selection this desktop honours: the Omarchy theme only on Linux with its reader
    /// installed (it needs an absolute `$HOME`), and otherwise the default theme, so Follow
    /// system is never locked for a theme without a visible card.
    pub fn on_desktop(self, cx: &App) -> Self {
        #[cfg(target_os = "linux")]
        let honoured = self != Self::Omarchy || cx.has_global::<omarchy::Omarchy>();
        #[cfg(not(target_os = "linux"))]
        let honoured = {
            let _ = cx;
            self != Self::Omarchy
        };
        if honoured { self } else { Self::default() }
    }
}

impl custom::ResolvedTheme {
    /// Apply the resolved built-in or custom palette through the one application path.
    pub fn apply(self, window: Option<&mut Window>, cx: &mut App) {
        self.palette.apply(self.is_light, window, cx);
    }

    /// On Linux, a resolved Omarchy selection with the palette the desktop's theme maps to
    /// once it has been read ([`omarchy::resolve`]); otherwise unchanged.
    pub fn with_desktop(self, cx: &App) -> Self {
        #[cfg(target_os = "linux")]
        {
            omarchy::resolve(self, cx)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = cx;
            self
        }
    }
}

impl Palette {
    /// Apply native controls and editor defaults together. Built-in and custom themes both use
    /// this path. Call after GPUI Kit initialization; callers invalidate/rebuild existing custom
    /// decorations.
    ///
    /// `window` is only the invalidation: passing `Some` refreshes it, which
    /// also bars view reuse for that frame. The app path passes `None` and
    /// notifies its root instead, so the frame that shows the new palette
    /// keeps the subtrees the palette does not change (see
    /// [`sync_text_sizes`]).
    ///
    /// While [`MoreContrast`] is on, the palette is adjusted first
    /// ([`Self::more_contrast_fit`], as [`Self::as_applied`] gives it), so the
    /// `Palette` global, its [`Labels`], the shared button's colors and the
    /// kit theme are all built from the adjusted one, and the floors derived
    /// here rise with it: syntax is
    /// fitted at [`MORE_CONTRAST_TEXT`] and the disabled label at
    /// [`MORE_CONTRAST_DISABLED`]. The changed-word tints never read worse
    /// than this palette's own ([`EditorHighlights::fit`]), and the disabled
    /// edge, the unchecked Switch track and its hover and a dark field's fill
    /// keep this palette's values ([`Self::configure`]). Off, the palette is
    /// applied as it is.
    pub fn apply(self, is_light: bool, window: Option<&mut Window>, cx: &mut App) {
        let more_contrast = more_contrast(cx);
        let (palette, roles, highlights) = self.fitted(more_contrast);
        Theme::change(
            if is_light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            },
            None,
            cx,
        );
        palette.configure(
            is_light,
            more_contrast,
            self,
            roles,
            highlights,
            Theme::global_mut(cx),
        );
        CONTROL_BUTTON.set(Some(ControlButton {
            idle: palette.control_button(cx),
            still: palette.still_control_button(cx),
            selected: palette.selected_control_button(cx),
            on_fill: ButtonCustomVariant::new(cx).foreground(rgb(palette.control_label()).into()),
            selected_hover: palette.row_hover(true),
        }));
        cx.set_global(highlights);
        cx.set_global(Labels::fit(palette));
        cx.set_global(palette);
        Theme::sync_base(cx);
        if let Some(window) = window {
            window.refresh();
        }
    }

    /// What [`Self::apply`] draws for this palette, decided once per
    /// application: the palette it stores, adjusted by
    /// [`Self::more_contrast_fit`] while `more_contrast` is on and itself
    /// otherwise, its syntax roles and its editor highlights, fitted from this
    /// palette as chosen under More contrast ([`EditorHighlights::fit`]).
    /// [`editor_highlights`] fits a test application's default palette
    /// through it too, so no path fits highlights another way.
    fn fitted(self, more_contrast: bool) -> (Self, SyntaxRoles, EditorHighlights) {
        let (palette, chosen) = if more_contrast {
            let (palette, chosen) = self.more_contrast_fit();
            (palette, Some(chosen))
        } else {
            (self, None)
        };
        let roles = palette.syntax_roles(more_contrast.then_some(self));
        (
            palette,
            roles,
            EditorHighlights::fit(palette, roles, chosen),
        )
    }

    /// The palette [`Self::apply`] stores for this one as the `Palette`
    /// global: adjusted by [`Self::with_more_contrast`] while
    /// [`MoreContrast`] is on, and itself otherwise. A caller that compares a
    /// resolved theme with the applied global compares through it.
    ///
    /// Built only where it is called, by the Linux Omarchy revision check
    /// (`GitTurtle::omarchy_changed`) and by tests, and for documentation,
    /// which links to it: [`Self::apply`] adjusts through
    /// [`Self::more_contrast_fit`] itself, so a macOS build left it never used
    /// and hosted clippy denied the warning.
    #[cfg(any(test, doc, target_os = "linux"))]
    pub fn as_applied(self, cx: &App) -> Self {
        if more_contrast(cx) {
            self.with_more_contrast()
        } else {
            self
        }
    }

    /// Set the kit theme from this palette, the one [`Self::apply`] stores:
    /// adjusted under More contrast, with `chosen` the palette before that
    /// adjustment (itself while More contrast is off). These fills and edges
    /// take `chosen`'s values, so More contrast leaves them as they were: the
    /// disabled edge, whose [`DISABLED_EDGE`] fit keeps a control that cannot
    /// act fainter than the 3:1 resting edge every enabled control gains,
    /// the unchecked Switch track's fill and its hover, and a dark field's
    /// fill with the default Button's disabled fill derived from it, which
    /// are fills, not edges, inside or under the adjusted edges.
    fn configure(
        self,
        is_light: bool,
        more_contrast: bool,
        chosen: Palette,
        roles: SyntaxRoles,
        highlights: EditorHighlights,
        theme: &mut Theme,
    ) {
        let palette = self;
        theme.colors.background = rgb(palette.canvas).into();
        // A dialog's backdrop dims the page until the dialog stands above it.
        theme.colors.overlay = gpui_kit::hsla(0., 0., 0., palette.dialog_backdrop() as f32);
        theme.colors.foreground = rgb(palette.text).into();
        theme.colors.muted = rgb(palette.hover).into();
        theme.colors.muted_foreground = rgb(palette.muted).into();
        theme.colors.primary = rgb(palette.accent).into();
        theme.colors.primary_foreground = rgb(palette.accent_foreground).into();
        theme.colors.primary_hover = rgb(palette.accent_hover).into();
        theme.colors.primary_active = rgb(palette.accent_active).into();
        theme.colors.border = rgb(palette.border).into();
        theme.colors.input = rgb(palette.border).into();
        // A fill is not an edge. The kit derives a field's fills from
        // `input`, the outline: a dark field's at 30%
        // (`Theme::input_background`), with the default Button's disabled
        // fill at half that, and a disabled field's at 80% in either mode
        // (`Theme::disabled_input_background`). Under More contrast they
        // derive from the chosen `border` (`Theme::input_fill_base`), so they
        // stay as they are off while the outline is adjusted; off, the kit
        // derives them as upstream does. With the base set, a disabled
        // field's outline is the disabled edge set below, as a disabled
        // Button's, so it stays fainter than the adjusted outline.
        theme.input_fill_base = more_contrast.then(|| rgb(chosen.border).into());
        // Text selection in the editors and inputs; rows and lists keep `selected` below.
        theme.colors.selection = rgb(highlights.selection).into();
        theme.colors.accent = rgb(palette.hover).into();
        theme.colors.accent_foreground = rgb(palette.text).into();
        theme.colors.button = rgb(palette.panel).into();
        theme.colors.button_hover = rgb(palette.hover).into();
        theme.colors.button_active = rgb(palette.selected).into();
        theme.colors.button_foreground = rgb(palette.text).into();
        theme.colors.button_primary = rgb(palette.accent).into();
        theme.colors.button_primary_foreground = rgb(palette.accent_foreground).into();
        theme.colors.button_primary_hover = rgb(palette.accent_hover).into();
        theme.colors.button_primary_active = rgb(palette.accent_active).into();
        // Danger buttons have their own tokens; the general danger color
        // alone leaves the toolkit's low-contrast default button untouched.
        let danger: gpui_kit::Hsla = rgb(palette.removed).into();
        theme.colors.button_danger = danger;
        theme.colors.button_danger_foreground = rgb(palette.canvas).into();
        theme.colors.button_danger_hover = if is_light {
            danger.darken(0.05)
        } else {
            danger.lighten(0.05)
        };
        theme.colors.button_danger_active = if is_light {
            danger.darken(0.1)
        } else {
            danger.lighten(0.1)
        };
        theme.colors.secondary = rgb(palette.subtle).into();
        theme.colors.secondary_foreground = rgb(palette.text).into();
        theme.colors.secondary_hover = rgb(palette.hover).into();
        theme.colors.secondary_active = rgb(palette.selected).into();
        theme.colors.button_secondary = rgb(palette.subtle).into();
        theme.colors.button_secondary_foreground = rgb(palette.text).into();
        theme.colors.button_secondary_hover = rgb(palette.hover).into();
        theme.colors.button_secondary_active = rgb(palette.selected).into();
        theme.colors.list = rgb(palette.panel).into();
        theme.colors.list_head = rgb(palette.panel).into();
        theme.colors.list_hover = rgb(palette.hover).into();
        theme.colors.list_active = rgb(palette.selected).into();
        theme.colors.list_active_border = rgb(palette.accent).into();
        theme.colors.list_even = rgb(palette.subtle).into();
        theme.colors.popover = rgb(palette.raised()).into();
        theme.popover_edge = Some(rgb(palette.raised_edge()).into());
        let dialog_edge = rgb(palette.dialog_edge()).into();
        theme.dialog_edge = Some(dialog_edge);
        // A dialog that sets no width of its own keeps the margin
        // `dialog_width` keeps (`DESIGN.md`, "One responsive order").
        theme.dialog_margin = Some(DIALOG_MARGIN);
        // The only bordered secondary Button is a dialog's Cancel, on the
        // dialog's surface, so it draws the dialog's edge in every state.
        theme.secondary_edge = Some(dialog_edge);
        theme.disabled_label = Some(
            if more_contrast {
                palette.more_contrast_disabled_label(chosen)
            } else {
                palette.disabled_label()
            }
            .into(),
        );
        theme.disabled_edge = Some(rgb(chosen.disabled_edge()).into());
        // Under More contrast an enabled control that rests on a fill of its
        // own (a secondary Button, the selected shared helper, an unchecked
        // Switch track) draws a 1 px edge at 3:1 over its bounds, moving
        // nothing; off, the kit draws none, as upstream.
        theme.resting_edge = more_contrast.then(|| rgb(palette.resting_edge()).into());
        theme.colors.popover_foreground = rgb(palette.text).into();
        theme.colors.sidebar = rgb(palette.panel).into();
        theme.colors.sidebar_foreground = rgb(palette.text).into();
        theme.colors.sidebar_border = rgb(palette.border).into();
        theme.colors.sidebar_accent = rgb(palette.selected).into();
        theme.colors.sidebar_accent_foreground = rgb(palette.text).into();
        theme.colors.sidebar_primary = rgb(palette.accent).into();
        theme.colors.sidebar_primary_foreground = rgb(palette.accent_foreground).into();
        theme.colors.tab = rgb(palette.subtle).into();
        theme.colors.tab_foreground = rgb(palette.muted).into();
        theme.colors.tab_active = rgb(palette.panel).into();
        theme.colors.tab_active_foreground = rgb(palette.text).into();
        theme.colors.tab_bar = rgb(palette.subtle).into();
        theme.colors.tab_bar_segmented = rgb(palette.canvas).into();
        theme.colors.table = rgb(palette.canvas).into();
        theme.colors.table_head = rgb(palette.panel).into();
        theme.colors.table_head_foreground = rgb(palette.muted).into();
        theme.colors.table_hover = rgb(palette.hover).into();
        theme.colors.table_active = rgb(palette.selected).into();
        theme.colors.table_active_border = rgb(palette.accent).into();
        theme.colors.table_row_border = rgb(palette.border).into();
        theme.colors.table_even = rgb(palette.subtle).into();
        theme.colors.title_bar = rgb(palette.panel).into();
        theme.colors.title_bar_border = rgb(palette.border).into();
        theme.colors.status_bar = rgb(palette.subtle).into();
        theme.colors.status_bar_border = rgb(palette.border).into();
        theme.colors.window_border = rgb(palette.border).into();
        theme.colors.scrollbar = rgb(palette.canvas).into();
        theme.colors.scrollbar_thumb = rgb(palette.scrollbar_thumb()).into();
        theme.colors.scrollbar_thumb_hover = rgb(palette.muted).into();
        theme.colors.switch = rgb(chosen.border).into();
        theme.colors.switch_thumb = rgb(palette.text).into();
        theme.colors.progress_bar = rgb(palette.accent).into();
        theme.colors.skeleton = rgb(palette.hover).into();
        theme.colors.ring = rgb(palette.accent).into();
        theme.button_focus_ring = BUTTON_FOCUS_RING;
        theme.switch_hover = Some(rgb(chosen.switch_hover()).into());
        // Ghost Buttons, the kit's own among them, lift and press as the shared
        // controls do; a toggled one keeps the pressed fill.
        theme.ghost_hover = Some(palette.control_fill(palette.hover).into());
        theme.ghost_active = Some(palette.control_fill(palette.selected).into());
        theme.colors.caret = rgb(palette.accent).into();
        theme.colors.link = rgb(palette.hunk).into();
        theme.colors.link_hover = rgb(palette.accent).into();
        theme.colors.link_active = rgb(palette.accent).into();
        theme.colors.danger = rgb(palette.removed).into();
        theme.colors.danger_foreground = rgb(palette.canvas).into();
        theme.colors.success = rgb(palette.added).into();
        theme.colors.success_foreground = rgb(palette.canvas).into();
        theme.colors.warning = rgb(palette.warning).into();
        theme.colors.warning_foreground = rgb(palette.canvas).into();
        theme.colors.info = rgb(palette.hunk).into();
        theme.colors.info_foreground = rgb(palette.canvas).into();
        let syntax = Arc::make_mut(&mut theme.highlight_theme);
        syntax.style.editor_background = Some(rgb(palette.canvas).into());
        syntax.style.editor_gutter_background = Some(rgb(palette.canvas).into());
        syntax.style.editor_active_line = Some(rgb(palette.panel).into());
        syntax.style.editor_line_number = Some(rgb(palette.line_number).into());
        syntax.style.editor_foreground = Some(rgb(palette.text).into());
        syntax.style.syntax = roles.syntax_colors(&syntax.style.syntax);
        set_theme_text_sizes(theme);
        theme.radius = px(7.);
        theme.radius_lg = px(12.);
        // GPUI Kit 0.6 paints component backgrounds from resolved ThemeTokens,
        // while foregrounds still read ThemeColor. Sync both representations
        // before Theme::sync_base propagates them to native controls.
        theme.tokens = (&theme.colors).into();
    }

    /// Each syntax role's palette color fitted, measured once per application. With More
    /// contrast off, `chosen` is `None` and each is fitted by [`SyntaxTargets::fit`] at the
    /// text rule with the rasterization margin. Under More contrast, this palette is the one
    /// More contrast draws and `chosen` the palette as chosen, before it was adjusted: each is
    /// fitted at [`MORE_CONTRAST_TEXT`] in OKLab lightness, its hue kept, and never reads on
    /// a background worse than the role drawn off on `chosen` reads on `chosen`'s
    /// ([`SyntaxTargets::fit_in_lightness`]). Comments are secondary text and follow `muted`,
    /// which More contrast raises toward `text` by the secondary-text rule
    /// ([`Self::with_raised_floors`]), so they are fitted by [`SyntaxTargets::fit`] at the rule
    /// in force in both.
    ///
    /// Why `chosen`: where a line tint moved ([`Self::more_contrast_fit`]), the off fit on this
    /// palette is not the role drawn off, and a role held only to it could read worse on the
    /// editor background or the active line than the role drawn off does.
    fn syntax_roles(self, chosen: Option<Palette>) -> SyntaxRoles {
        let rule = if chosen.is_some() {
            MORE_CONTRAST_TEXT
        } else {
            LABEL_RULE + LABEL_MARGIN
        };
        let targets = SyntaxTargets::new(self, rule);
        let off = chosen.map(|chosen| SyntaxTargets::new(chosen, LABEL_RULE + LABEL_MARGIN));
        let fit = |color: u32| match &off {
            Some(off) => targets.fit_in_lightness(color, off),
            None => targets.fit(color),
        };
        SyntaxRoles {
            keyword: fit(self.renamed),
            string: fit(self.added),
            constant: fit(self.warning),
            type_: fit(self.modified),
            function: fit(self.hunk),
            markup: fit(self.accent),
            comment: targets.fit(self.muted),
            removed: fit(self.removed),
            // `text` reaches every target by definition, so fitting would return it unchanged.
            text: self.text,
        }
    }

    /// The kit theme [`Self::apply`] leaves for this palette, without an application:
    /// `Theme::change` restores the mode's default theme, highlight theme included, and
    /// `configure` then applies this palette over it.
    #[cfg(test)]
    pub(crate) fn configured_theme(self, is_light: bool) -> Theme {
        use gpui_kit::component::highlighter::HighlightTheme;
        let mut theme = Theme {
            highlight_theme: if is_light {
                HighlightTheme::default_light()
            } else {
                HighlightTheme::default_dark()
            },
            ..Theme::default()
        };
        let roles = self.syntax_roles(None);
        self.configure(
            is_light,
            false,
            self,
            roles,
            EditorHighlights::fit(self, roles, None),
            &mut theme,
        );
        theme
    }

    /// The editor highlights [`Self::apply`] fits for this palette, without an application.
    #[cfg(test)]
    pub(crate) fn editor_highlights(self) -> EditorHighlights {
        EditorHighlights::fit(self, self.syntax_roles(None), None)
    }
}

/// The palette color each syntax role draws in once fitted: `keyword` from `renamed`, `string`
/// from `added`, `constant` from `warning`, `type_` from `modified`, `function` from `hunk`,
/// `markup` from `accent`, `comment` from `muted`, `removed` from `removed`, and `text` as it
/// is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SyntaxRoles {
    keyword: u32,
    string: u32,
    constant: u32,
    type_: u32,
    function: u32,
    markup: u32,
    comment: u32,
    removed: u32,
    text: u32,
}

impl SyntaxRoles {
    /// The toolkit's syntax styles with each field's color taken from its role; italic and
    /// weight stay as the toolkit set them. The literal names every field, so a field a later
    /// toolkit adds fails to compile until it has a role. A capture without a field of its own
    /// takes the field of its first segment (`variable.builtin` takes `variable`) or, with none,
    /// the editor foreground, which is `text`, so every capture a grammar emits draws in a
    /// palette color.
    fn syntax_colors(self, kit: &SyntaxColors) -> SyntaxColors {
        let [
            keyword,
            string,
            constant,
            type_,
            function,
            markup,
            comment,
            removed,
            text,
        ] = self.colors().map(Some);
        SyntaxColors {
            keyword: restyle(kit.keyword, keyword),
            preproc: restyle(kit.preproc, keyword),
            string: restyle(kit.string, string),
            string_escape: restyle(kit.string_escape, string),
            string_regex: restyle(kit.string_regex, string),
            string_special: restyle(kit.string_special, string),
            string_special_symbol: restyle(kit.string_special_symbol, string),
            text_literal: restyle(kit.text_literal, string),
            text_code_span: restyle(kit.text_code_span, string),
            number: restyle(kit.number, constant),
            boolean: restyle(kit.boolean, constant),
            constant: restyle(kit.constant, constant),
            type_: restyle(kit.type_, type_),
            constructor: restyle(kit.constructor, type_),
            diff_context: restyle(kit.diff_context, text),
            diff_header: restyle(kit.diff_header, text),
            diff_hunk: restyle(kit.diff_hunk, function),
            diff_minus: restyle(kit.diff_minus, removed),
            diff_plus: restyle(kit.diff_plus, string),
            enum_: restyle(kit.enum_, type_),
            variant: restyle(kit.variant, type_),
            function: restyle(kit.function, function),
            tag: restyle(kit.tag, markup),
            tag_doctype: restyle(kit.tag_doctype, markup),
            attribute: restyle(kit.attribute, markup),
            property: restyle(kit.property, markup),
            link_text: restyle(kit.link_text, markup),
            link_uri: restyle(kit.link_uri, markup),
            label: restyle(kit.label, markup),
            title: restyle(kit.title, markup),
            comment: restyle(kit.comment, comment),
            comment_doc: restyle(kit.comment_doc, comment),
            hint: restyle(kit.hint, comment),
            predictive: restyle(kit.predictive, comment),
            variable: restyle(kit.variable, text),
            variable_special: restyle(kit.variable_special, text),
            embedded: restyle(kit.embedded, text),
            operator: restyle(kit.operator, text),
            punctuation: restyle(kit.punctuation, text),
            punctuation_bracket: restyle(kit.punctuation_bracket, text),
            punctuation_delimiter: restyle(kit.punctuation_delimiter, text),
            punctuation_list_marker: restyle(kit.punctuation_list_marker, text),
            punctuation_special: restyle(kit.punctuation_special, text),
            primary: restyle(kit.primary, text),
            // Emphasis keeps its italic or weight in the color around it.
            emphasis: restyle(kit.emphasis, None),
            emphasis_strong: restyle(kit.emphasis_strong, None),
        }
    }

    /// The colors a word tint carries: every role but `removed`, which only a diff file's
    /// removed lines draw in, and which a compared language's words never take.
    fn word_colors(self) -> [u32; 8] {
        [
            self.keyword,
            self.string,
            self.constant,
            self.type_,
            self.function,
            self.markup,
            self.comment,
            self.text,
        ]
    }

    /// Every color the editors draw syntax in: the eight fitted roles and `text`.
    fn colors(self) -> [u32; 9] {
        [
            self.keyword,
            self.string,
            self.constant,
            self.type_,
            self.function,
            self.markup,
            self.comment,
            self.removed,
            self.text,
        ]
    }
}

/// `kit` with its color replaced by `color`, or removed for `None`, keeping its italic and
/// weight; a field the toolkit leaves empty gets a style with the color alone. `ThemeStyle` keeps
/// its fields private, so the style is rebuilt through the toolkit's theme format, which names
/// them.
fn restyle(kit: Option<ThemeStyle>, color: Option<u32>) -> Option<ThemeStyle> {
    use serde_json::{Map, Value};
    let mut entry = match serde_json::to_value(kit) {
        Ok(Value::Object(entry)) => entry,
        Ok(Value::Null) => Map::new(),
        unexpected => {
            debug_assert!(
                false,
                "a theme style serializes as an object: {unexpected:?}"
            );
            return kit;
        }
    };
    match color {
        Some(color) => entry.insert("color".into(), custom::format_hex(color).into()),
        None => entry.remove("color"),
    };
    let restyled = serde_json::from_value(Value::Object(entry));
    debug_assert!(
        restyled.is_ok(),
        "a theme style reads back its own format: {restyled:?}"
    );
    restyled.ok().or(kit)
}

/// The backgrounds syntax colors are drawn on, measured once per palette application: the
/// editor background, which hunk header lines keep (their decoration sets no background), the
/// active line, and the added and removed line tints of the unified and split diffs.
struct SyntaxTargets {
    text: u32,
    /// Whether `text` reads at the text rule on every background.
    text_reads: bool,
    /// Whether `text` is lighter than the editor background, as in a dark palette: the side
    /// [`Self::fit_in_lightness`] moves a role toward.
    text_lighter: bool,
    /// Each background's luminance and the contrast a syntax color must reach on it: the
    /// fitting's rule (the text rule plus the rasterization margin, or More contrast's 7:1), or
    /// `text`'s own contrast where that is lower.
    backgrounds: [(f64, f64); 4],
}

impl SyntaxTargets {
    fn new(palette: Palette, rule: f64) -> Self {
        let text = custom::luminance(palette.text);
        let mut text_reads = true;
        let backgrounds = [
            palette.canvas,
            palette.panel,
            palette.added_background,
            palette.removed_background,
        ]
        .map(|background| {
            let background = custom::luminance(background);
            let text_contrast = custom::luminance_contrast(text, background);
            text_reads &= text_contrast >= LABEL_RULE;
            (background, rule.min(text_contrast))
        });
        Self {
            text: palette.text,
            text_reads,
            text_lighter: text > custom::luminance(palette.canvas),
            backgrounds,
        }
    }

    fn reads(&self, color: u32) -> bool {
        let color = custom::luminance(color);
        self.backgrounds
            .iter()
            .all(|&(background, minimum)| custom::luminance_contrast(color, background) >= minimum)
    }

    /// `color` as it is, when it reads on every background, or moved toward `text` by a step
    /// that reads, found by a binary search over 256ths of each sRGB channel; every step it
    /// accepts reads, so the result is at least 4.5:1 on each background. `text` itself reads
    /// everywhere, so the search always ends. A palette whose `text` is below the text rule on
    /// one of them, which only a custom theme with a readability warning can be, draws its
    /// syntax in `text`.
    fn fit(&self, color: u32) -> u32 {
        if !self.text_reads {
            return self.text;
        }
        if self.reads(color) {
            return color;
        }
        let toward_text = |step: u32| mix(color, self.text, step);
        // `high` always reads (at 256 steps the color is `text`) and `low` never does.
        let (mut low, mut high) = (0, 256);
        while high - low > 1 {
            let middle = (low + high) / 2;
            if self.reads(toward_text(middle)) {
                high = middle;
            } else {
                low = middle;
            }
        }
        toward_text(high)
    }

    /// More contrast's fit for the syntax role drawn from `color`, with `off` the targets of
    /// the palette as chosen with More contrast off: `color` as it is where it reads on every
    /// background, and
    /// otherwise `color` moved in OKLab lightness toward `text`'s side, lighter in a dark
    /// palette and darker in a light one, its hue and chroma kept, or where sRGB cannot hold
    /// that chroma at that lightness the most of it sRGB holds in the same hue
    /// ([`linear_in_gamut`]), to the least step, in thousandths of lightness, that reads on
    /// every background, found by a binary search. It never moves past `text`'s lightness:
    /// where no step short of it reads, the role stops at `text`'s lightness in its own hue,
    /// and a color already level with `text` or past it does not move. Where what that gives
    /// reads on a background below the role drawn off (`off`'s fit of `color`) on the same
    /// background as chosen, the role drawn off stays, so no role reads worse than with More
    /// contrast off; the role drawn off is fitted only then. A palette whose `text` is below
    /// the text rule on a background, which only
    /// a custom theme with a readability warning can be, draws its syntax in `text`, as
    /// [`Self::fit`] does.
    ///
    /// Why lightness: moving toward `text` in sRGB spends chroma and turns the hue, so where
    /// `text` reads under 7:1 roles ended at `text` itself (Omarchy tokyo-night's keyword,
    /// function and markup), and short of that they greyed (Omarchy catppuccin's removed
    /// #f38ba8 drew #d9bedb); lightness is the channel hierarchy is built from, so contrast
    /// raised through it keeps each role's hue. Why the bound: syntax is never lighter than
    /// body text (darker in a light palette), the lightness form of the secondary-text rule;
    /// unbound, 29 roles in the shipped and Omarchy palettes passed `text`'s lightness, One
    /// Dark's keyword by 0.020.
    fn fit_in_lightness(&self, color: u32, off: &SyntaxTargets) -> u32 {
        if !self.text_reads {
            return self.text;
        }
        if self.reads(color) {
            return color;
        }
        let ([lightness, a, b], text) = (oklab(color), oklab(self.text)[0]);
        let toward = if self.text_lighter { 1. } else { -1. };
        let span = (text - lightness) * toward;
        let fitted = if span > 0. {
            let steps = (span * 1000.).ceil() as u32;
            let at = |step: u32| {
                let lightness = if step >= steps {
                    text
                } else {
                    lightness + toward * f64::from(step) / 1000.
                };
                from_linear(linear_in_gamut([lightness, a, b]))
            };
            if self.reads(at(steps)) {
                // `high` always reads and `low` never does (at 0 steps the color is `color`).
                let (mut low, mut high) = (0, steps);
                while high - low > 1 {
                    let middle = (low + high) / 2;
                    if self.reads(at(middle)) {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                at(high)
            } else {
                at(steps)
            }
        } else {
            color
        };
        let drawn_off = off.fit(color);
        let (fitted_luminance, off_luminance) =
            (custom::luminance(fitted), custom::luminance(drawn_off));
        // Both hold the editor background, the active line and the two line tints in order.
        let reads_as_off = self.backgrounds.iter().zip(&off.backgrounds).all(
            |(&(background, _), &(off_background, _))| {
                custom::luminance_contrast(fitted_luminance, background)
                    >= custom::luminance_contrast(off_luminance, off_background)
            },
        );
        if reads_as_off { fitted } else { drawn_off }
    }
}

/// `from` moved `step` 256ths of the way to `to` in each sRGB channel, rounded.
fn mix(from: u32, to: u32, step: u32) -> u32 {
    [16, 8, 0].into_iter().fold(0, |mixed, shift| {
        let (from, to) = ((from >> shift) & 0xff, (to >> shift) & 0xff);
        mixed | (((from * (256 - step) + to * step + 128) / 256) << shift)
    })
}

/// The colors the code editors draw behind text, fitted once per palette application by
/// [`EditorHighlights::fit`] and kept as a global beside the palette: the text selection, Find's
/// other and current matches, and the added and removed changed-word tints. Rows and lists keep
/// the palette's `selected`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorHighlights {
    pub selection: u32,
    pub find_match: u32,
    pub find_current: u32,
    pub added_word: u32,
    pub removed_word: u32,
    /// The 1 px rule an added word draws along its highlight box's bottom edge under More
    /// contrast ([`Self::word_rule`]), so a changed word is marked by more than colour; `None`
    /// with More contrast off.
    pub added_rule: Option<u32>,
    /// The removed word's rule, as [`Self::added_rule`].
    pub removed_rule: Option<u32>,
    /// The colours the unified patch draws its text in, as `text::theme_decorations` decorates
    /// it: added lines and their words, removed lines and their words, and hunk headers. With
    /// More contrast off these are the palette's `added`, `removed` and `hunk` as chosen, so a
    /// patch draws as it always has; under More contrast they are the syntax roles fitted from
    /// them, `string`, `removed` and `function` ([`SyntaxRoles`]), the colours Split, Before and
    /// After draw a patch's lines in, so one file never draws one change in two colours by view
    /// (decision, 2026-11-05: off, the Diff view kept its colours drawn off beside Split's
    /// raised roles, and a removed word read 4.82:1 in Omarchy tokyo-night).
    pub patch_text: PatchText,
}

/// The unified patch's text colours ([`EditorHighlights::patch_text`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchText {
    pub added: u32,
    pub removed: u32,
    pub hunk: u32,
}

impl Global for EditorHighlights {}

/// The applied palette's editor highlights. Every application sets them with the palette; only
/// a test application that has not applied one fits the default palette's here, as
/// [`Palette::apply`] would fit them, More contrast included ([`Palette::fitted`]). Fitted with
/// no palette as chosen under More contrast, the fallback drew no changed-word rule and kept
/// the current match and the word tints as drawn off.
pub fn editor_highlights(cx: &App) -> EditorHighlights {
    cx.try_global::<EditorHighlights>()
        .copied()
        .unwrap_or_else(|| palette(cx).fitted(more_contrast(cx)).2)
}

/// How far every fitted editor highlight stands from each line background it is drawn on,
/// measured as ΔEOK, the Euclidean distance between the two colors in OKLab (the measure CSS
/// Color 4 names deltaEOK). [`EditorHighlights::fit`] keeps it for every highlight.
///
/// Why this measure: a highlight may only move in lightness until every color on it reads, and
/// where a palette's syntax is fitted to 4.75:1 on a line, a highlight lighter than that line
/// (toward the syntax) by the selected-row rule's 1.15:1 would leave that syntax at 4.13:1, so a
/// contrast ratio cannot both keep the syntax readable and the highlight apart. ΔEOK counts
/// lightness, chroma and hue together, so a highlight that keeps its seed's hue, or gains
/// chroma, stands apart from a line of nearly the same lightness, as a tinted selection over a
/// grey line visibly does.
///
/// Why this threshold: one just-noticeable difference in OKLab is about 0.02; 0.04 is two, and
/// sits just under the closest `selected` any built-in keeps from its canvas or active line on
/// origin/main (0.044, One Light's canvas and One Dark's active line), the distance the curated
/// palettes already accept as a visible selection. Origin's word tints stood at least 0.10 from
/// their lines, and its `selected` as little as 0.017 from Daylight's added lines.
const HIGHLIGHT_DISTANCE: f64 = 0.04;

/// The least gap in OKLab lightness a changed-word tint keeps from its line tint under More
/// contrast where the colors on it allow ([`EditorHighlights::lifted_word_tint`]).
///
/// Why lightness: a tint can stand [`HIGHLIGHT_DISTANCE`] from its line through hue and chroma
/// alone, and those are what colour-blind vision loses; the gap in lightness is what survives
/// it, so a rule met in OKLab distance could be met through chroma alone. With More contrast
/// off, Omarchy tokyo-night's added word stands 0.005 from its line in lightness, and 0.001 in
/// a protanopia model, so the changed word nearly vanishes. Why this gap: the same two
/// just-noticeable differences as [`HIGHLIGHT_DISTANCE`], here in lightness alone.
const WORD_LIGHTNESS_GAP: f64 = 0.04;

/// The least gap in OKLab lightness the highlights drawn beside one another keep under More
/// contrast, besides [`HIGHLIGHT_DISTANCE`] in ΔEOK: Find's current match from the selection,
/// and Find's other matches from the selection, the current match and the changed-word tints
/// ([`HighlightFit::apart_from`]), and a word stepped across its line from the line
/// ([`EditorHighlights::crossed_word_tint`]).
///
/// Why lightness: two highlights can stand [`HIGHLIGHT_DISTANCE`] apart through chroma alone,
/// which colour-blind vision loses (with More contrast off, One Dark's selection and current
/// match stand 0.044 apart and 0.0001 in lightness), and a distance met by saturation alone
/// failed a review before. Why this gap: one just-noticeable difference in OKLab, so the
/// lightness step alone is visible while hue and chroma carry the rest of the distance. The
/// five highlights share the lightness band in which every colour on them still reads, and two
/// differences between each pair exhausted it: Alucard's other matches found no colour that
/// reads and stands 0.04 in lightness from the selection, the current match and both word
/// tints, and fell back to the selection's, 0 apart from it, while other palettes' matches were
/// pushed to the band's edges (Kanagawa Lotus's other matches `#f2f6f8`, beside its canvas,
/// Rosé Pine Dawn's a saturated `#ffabff`; measured 2026-11-05).
const HIGHLIGHT_LIGHTNESS_GAP: f64 = 0.02;

impl EditorHighlights {
    /// Fit each highlight to this palette.
    ///
    /// Each starts from a seed: the selection and Find's other matches from `selected`, the
    /// current match from `selected` a quarter of the way to `accent`, and each word tint from its
    /// line tint a quarter of the way to its diff color, the tint origin/main drew. A seed is kept
    /// when every color drawn on it reads and it stands [`HIGHLIGHT_DISTANCE`] from every line
    /// background it sits on. Otherwise the highlight takes the candidate nearest the seed, by
    /// ΔEOK, that does both: candidates are the seed moved toward black in a dark palette or white
    /// in a light one in 256ths of each sRGB channel, and the seed's hue at more chroma
    /// ([`chroma_rows`]), which stands apart where lightness has no room left. A chroma
    /// candidate is ranked by the coordinates it was built at, before clipping to sRGB.
    ///
    /// A color reads when it reaches 4.75:1 on the highlight (the text rule plus the
    /// rasterization margin), or its own contrast on the line it is drawn on where that is
    /// lower, never below 4.5:1, as syntax fitting does; when no candidate does that and stands
    /// apart, the nearest at 4.5:1 that stands apart, and when none stands apart at all, the first
    /// step toward black or white at 4.5:1. The colors drawn on the selection and on Find's
    /// matches are `text` and every syntax role, drawn on the editor background, the active line
    /// and both line tints, and the unified patch's `added`, `removed` and `hunk`, drawn on their
    /// line tints and the editor background; a word tint carries `text`, every syntax role but
    /// `removed` ([`SyntaxRoles::word_colors`]) and its diff color, and sits on its line tint. The current match stands apart from the other
    /// matches too where it can; its accent underline marks it where it cannot.
    ///
    /// Under More contrast, `palette` is the adjusted palette, `roles` are fitted at
    /// [`MORE_CONTRAST_TEXT`] and `chosen` is the palette as chosen, and the selection, Find's
    /// current match and the word tints start from the ones `chosen` draws off, so no color
    /// reads worse on them and they stand from their lines on the same side, no nearer and with
    /// no less luminance contrast than with More contrast off, where the rules beside allow it:
    /// the selection and the current match raised ([`Self::raised`], [`Self::fit_raised`]), the
    /// current match and the other matches apart from the selection and from one another in
    /// lightness too, the other matches in a colour of their own, fitted by the text rule, and
    /// each word tint lifted from its line in lightness where it stands
    /// nearer than [`WORD_LIGHTNESS_GAP`] ([`Self::lifted_word_tint`]), or stepped beyond its
    /// line where it lies on the canvas side ([`Self::crossed_word_tint`]), which
    /// [`Palette::more_contrast_fit`] decides beside the line tints and hands in `chosen`. Off,
    /// `chosen` is `None`, and none of those rules runs.
    fn fit(palette: Palette, roles: SyntaxRoles, chosen: Option<Chosen>) -> Self {
        if let Some(chosen) = chosen {
            return Self::fit_raised(palette, roles, chosen);
        }
        let away = Self::away(palette);
        let lines = Self::lines(palette);
        let patch = Self::patch_colors(palette, roles, &lines);
        let fit = |seed: u32, drawn: &[(u32, &[u32])], lines: &[u32], tint: u32| {
            let fit = HighlightFit::new(drawn, lines);
            fit.search(seed, away, tint)
                .unwrap_or_else(|| fit.readable(seed, away))
        };
        let selection = fit(palette.selected, &patch, &lines, palette.accent);
        let find_match = selection;
        let current = mix(palette.selected, palette.accent, 64);
        let find_current = HighlightFit::new(&patch, &[&lines[..], &[find_match]].concat())
            .search(current, away, palette.accent)
            .unwrap_or_else(|| fit(current, &patch, &lines, palette.accent));
        Self {
            selection,
            find_match,
            find_current,
            added_word: Self::word_tint(palette, roles, palette.added, palette.added_background),
            removed_word: Self::word_tint(
                palette,
                roles,
                palette.removed,
                palette.removed_background,
            ),
            added_rule: None,
            removed_rule: None,
            patch_text: PatchText {
                added: palette.added,
                removed: palette.removed,
                hunk: palette.hunk,
            },
        }
    }

    /// [`Self::fit`] under More contrast. The selection and current match are raised from the
    /// ones `chosen` draws with More contrast off ([`Self::raised`]), or else are those: the
    /// selection from `selected`, and the current match from `selected` a quarter of the way to
    /// `accent`. Each word tint is the one `chosen` carries, the one drawn off lifted from its
    /// line where it stands faintest, or stepped beyond its line where it lies on the canvas side
    /// ([`Self::lifted_word_tint`], [`Self::crossed_word_tint`], [`Palette::more_contrast_fit`]).
    /// Fitted only to the text rule against the raised syntax, the selection and Find's matches
    /// settled lighter, nearer their seeds, and the colors on them read worse than off
    /// (Midnight's strings on the current match 6.18:1 where they read 6.63:1).
    ///
    /// The current match stands from the selection at least [`HIGHLIGHT_DISTANCE`] and
    /// [`HIGHLIGHT_LIGHTNESS_GAP`] in lightness, and at least as far as the off current match
    /// stands from it off wherever a candidate can ([`Self::current_beside`]). Held to its own
    /// distance from the raised
    /// selection, it found no candidate wherever the selection had moved, and settled beside the
    /// other matches (Omarchy alacritty-dark 0.040 to 0.024), or kept its off highlight beside a
    /// selection that had moved toward it (Omarchy ristretto 0.090 to 0.039).
    ///
    /// Find's other matches take a colour of their own, where with More contrast off they share
    /// the selection's: the candidate nearest that one that reads by the text rule as
    /// [`Self::fit`] fits and stands [`HIGHLIGHT_DISTANCE`] and [`HIGHLIGHT_LIGHTNESS_GAP`] from
    /// the selection, the current match and both word tints, and from the current match at least
    /// as far as the current match drawn off stands from them off, searched toward black or white
    /// as it searches and then toward the text, where a light palette's pale highlights have
    /// room left (searched one way only, seven light palettes found none), and then the same only
    /// [`HIGHLIGHT_DISTANCE`] from the current match. Where none fits, they keep the one drawn
    /// off, which reads, never the selection as More contrast draws it; since the one drawn off
    /// is the selection's colour drawn off, beside a selection that keeps that colour, as every
    /// palette's measured does, they still draw the selection's colour (decision, 2026-11-05
    /// evening; no shipped palette reaches it). They are not raised from the one drawn off
    /// as the selection is: in no built-in or Omarchy palette did a raised candidate stand so
    /// apart (none of the 45 [`Palette::more_contrast_fit`]'s tests read, 2026-11-05 evening);
    /// a custom theme in which one does takes the text-rule fit, which drew the same colour as
    /// the raised one in the 4 of 476 synthetic custom palettes where one stood apart (Midnight
    /// and Graphite with their text dimmed toward the canvas, or Graphite's selection taken to
    /// its canvas; 2026-11-05 night). Why: drawn in the selection's colour, they stood 0 from
    /// the selection in every palette, so a match could not be told from selected text
    /// (decision, 2026-11-05).
    fn fit_raised(palette: Palette, roles: SyntaxRoles, chosen: Chosen) -> Self {
        let Chosen {
            palette: chosen,
            off,
            words: [added_word, removed_word],
        } = chosen;
        let own_roles = chosen.syntax_roles(None);
        let (away, lines) = (Self::away(palette), Self::lines(palette));
        let patch = Self::patch_colors(palette, roles, &lines);
        let own: Vec<u32> = Self::patch_colors(chosen, own_roles, &Self::lines(chosen))
            .into_iter()
            .map(|(color, _)| color)
            .collect();
        let raised = |off: u32| Self::raised(&patch, &own, off, &lines);
        // A highlight fitted by the text rule where none is raised, under the colours drawn on
        // it as text under More contrast, `text` and every role on every line (the patch's
        // text is `string`, `removed` and `function`, [`PatchText`]): toward `away`, which
        // raises the text's contrast, and else toward the text, where a light palette's pale
        // highlights have room left.
        let as_text: Vec<(u32, &[u32])> = roles.colors().map(|color| (color, &lines[..])).into();
        let readable = |seed: u32, fit: &dyn Fn(HighlightFit) -> HighlightFit| {
            let fit = fit(HighlightFit::new(&as_text, &lines));
            fit.search(seed, away, palette.accent)
                .or_else(|| fit.search(seed, away ^ 0xffffff, palette.accent))
        };
        let selection = raised(off.selection)
            .raise(palette.selected, away, palette.accent)
            .unwrap_or(off.selection);
        let current = mix(palette.selected, palette.accent, 64);
        let (selection, find_current) = Self::current_beside(
            (off.selection, off.find_current),
            selection,
            |distance| {
                raised(off.find_current)
                    .apart_from(selection, distance)
                    .raise(current, away, palette.accent)
            },
            |selection, distance| readable(current, &|fit| fit.apart_from(selection, distance)),
        );
        // Find's other matches: apart from the selection, the current match, at least as far as
        // the current match drawn off stands from them, and the word tints.
        let from_current =
            oklab_distance(oklab(off.find_current), oklab(off.find_match)).max(HIGHLIGHT_DISTANCE);
        let beside = |fit: HighlightFit, from_current: f64| {
            fit.apart_from(selection, HIGHLIGHT_DISTANCE)
                .apart_from(find_current, from_current)
                .apart_from(added_word, HIGHLIGHT_DISTANCE)
                .apart_from(removed_word, HIGHLIGHT_DISTANCE)
        };
        let find_match = readable(off.find_match, &|fit| beside(fit, from_current))
            .or_else(|| readable(off.find_match, &|fit| beside(fit, HIGHLIGHT_DISTANCE)))
            .unwrap_or(off.find_match);
        Self {
            selection,
            find_match,
            find_current,
            added_word,
            removed_word,
            added_rule: Some(Self::word_rule(palette, palette.added, added_word)),
            removed_rule: Some(Self::word_rule(palette, palette.removed, removed_word)),
            patch_text: PatchText {
                added: roles.string,
                removed: roles.removed,
                hunk: roles.function,
            },
        }
    }

    /// The rule a changed word draws along its highlight box's bottom edge under More contrast:
    /// its side's diff colour, `diff`, as applied, where that reads [`MORE_CONTRAST_EDGE`] on the
    /// word's tint, `word`, otherwise the first of 256 even steps from it toward black in a light
    /// palette or white in a dark one that does, as [`Palette::resting_edge`] is fitted.
    ///
    /// Why a rule: a changed line carries cues beyond colour, its `+` or `-` and Split's filler
    /// rows, and a changed word carried none; in colour-blind models some words nearly vanish
    /// (Solarized Dark's removed words), and WCAG 1.4.1 asks for a cue beyond colour. Why 3:1:
    /// the rule is a graphic, held to the non-text contrast every other edge under More contrast
    /// holds. Why every palette and both sides: marking only some would read as a difference in
    /// meaning.
    fn word_rule(palette: Palette, diff: u32, word: u32) -> u32 {
        let extreme = if palette.is_light() {
            0x000000
        } else {
            0xffffff
        };
        toward_extreme_in(256, diff, extreme, |rule| {
            custom::contrast(rule, word) >= MORE_CONTRAST_EDGE
        })
    }

    /// The selection and current match [`Self::fit_raised`] draws: `selection` is raised from
    /// `off_selection`, the selection drawn with More contrast off (or is it, where none is
    /// raised), `beside(distance)` raises the current match from `off_current`, the one drawn
    /// off, to stand at least `distance` and [`HIGHLIGHT_LIGHTNESS_GAP`] in lightness from
    /// `selection`, or is `None` where none does, and `fitted(selection, distance)` fits one by
    /// the text rule, as [`Self::fit`] fits, that stands `distance` and that gap from
    /// `selection`, or is `None`. The current match stands from the selection at least `apart`,
    /// as far as off and at least [`HIGHLIGHT_DISTANCE`], with that gap: raised beside the raised
    /// selection; or the off current match beside it, or beside the off selection, where it
    /// stands so; or else fitted beside the raised selection, or beside the off one, `apart` from
    /// it where a fitted candidate stands so, and where none does, only [`HIGHLIGHT_DISTANCE`]
    /// with the gap. Raised only as far as off where that was less, the current match stood 0.003
    /// from the selection in Sandstone, and 0.044 in One Dark by chroma alone, 0.0001 in
    /// lightness; fitted only [`HIGHLIGHT_DISTANCE`] from it, One Dark's `#104266` stood 0.041
    /// from its selection, nearer than the pair drawn off stand apart through chroma (0.044),
    /// where fitted as far as off it is `#13406d`, 0.045 from it.
    fn current_beside(
        (off_selection, off_current): (u32, u32),
        selection: u32,
        beside: impl Fn(f64) -> Option<u32>,
        fitted: impl Fn(u32, f64) -> Option<u32>,
    ) -> (u32, u32) {
        let apart =
            oklab_distance(oklab(off_current), oklab(off_selection)).max(HIGHLIGHT_DISTANCE);
        let stands = |current: u32, selection: u32| {
            let (current, selection) = (oklab(current), oklab(selection));
            oklab_distance(current, selection) >= apart
                && (current[0] - selection[0]).abs() >= HIGHLIGHT_LIGHTNESS_GAP
        };
        if let Some(current) = beside(apart) {
            (selection, current)
        } else if stands(off_current, selection) {
            (selection, off_current)
        } else if stands(off_current, off_selection) {
            (off_selection, off_current)
        } else {
            // As far as off where a fitted candidate stands so, else the floor; beside the raised
            // selection, else the off one (each tried once where they are the same).
            let distances = [apart, HIGHLIGHT_DISTANCE];
            let distances = &distances[..if apart > HIGHLIGHT_DISTANCE { 2 } else { 1 }];
            let selections = [selection, off_selection];
            let selections = &selections[..if selection != off_selection { 2 } else { 1 }];
            distances
                .iter()
                .flat_map(|&distance| selections.iter().map(move |&beside| (beside, distance)))
                .find_map(|(beside, distance)| {
                    fitted(beside, distance).map(|current| (beside, current))
                })
                .unwrap_or((off_selection, off_current))
        }
    }

    /// The fit that raises a highlight under More contrast from `off`, the same highlight fitted
    /// with More contrast off, so it reads and marks no worse: each color in `drawn`, with the
    /// lines it is drawn on, reaches [`MORE_CONTRAST_TEXT`], or its own lowest contrast on those
    /// lines where that is lower ([`drawn_target`]), and at least the contrast its counterpart
    /// in `own` reads on `off`, on a highlight that stands apart from each of `lines` as `off`
    /// does, on the same side, at least as far and with at least its luminance contrast
    /// ([`HighlightFit::at_least_as_apart_as`]).
    /// [`HighlightFit::raise`] finds the candidate nearest a seed that reaches all of that,
    /// searched as [`Self::fit`] searches, or `None`, and the caller keeps `off`.
    fn raised(drawn: &[(u32, &[u32])], own: &[u32], off: u32, lines: &[u32]) -> HighlightFit {
        let targets: Vec<_> = drawn
            .iter()
            .zip(own)
            .map(|(&(color, on), &own)| {
                let target =
                    drawn_target(color, on, MORE_CONTRAST_TEXT).max(custom::contrast(own, off));
                (color, target)
            })
            .collect();
        HighlightFit::targeted(&targets, lines).at_least_as_apart_as(off)
    }

    /// The colors drawn on the selection and Find's matches, each with the lines it is drawn on:
    /// `text` and every syntax role on every line of `lines` ([`Self::lines`]), and the patch's
    /// `added`, `removed` and `hunk` on their line tints and the editor background.
    fn patch_colors(palette: Palette, roles: SyntaxRoles, lines: &[u32; 4]) -> Vec<(u32, &[u32])> {
        roles
            .colors()
            .map(|color| (color, &lines[..]))
            .into_iter()
            .chain([
                (palette.added, &lines[2..3]),
                (palette.removed, &lines[3..4]),
                (palette.hunk, &lines[..1]),
            ])
            .collect()
    }

    /// The side a highlight moves toward when its seed does not read: black in a dark palette,
    /// white in a light one.
    fn away(palette: Palette) -> u32 {
        if palette.is_light() {
            0xffffff
        } else {
            0x000000
        }
    }

    /// The backgrounds the editors draw lines on: the editor background, the active line and
    /// the added and removed line tints.
    fn lines(palette: Palette) -> [u32; 4] {
        [
            palette.canvas,
            palette.panel,
            palette.added_background,
            palette.removed_background,
        ]
    }

    /// The colors drawn on the changed-word tint for `diff` on its line tint `line`, each with
    /// the lines it is drawn on: `text` and every syntax role but `removed` on every line, and
    /// `diff` on its own.
    fn word_colors<'a>(
        roles: SyntaxRoles,
        lines: &'a [u32; 4],
        diff: u32,
        line: &'a [u32; 1],
    ) -> impl Iterator<Item = (u32, &'a [u32])> {
        roles
            .word_colors()
            .map(|color| (color, &lines[..]))
            .into_iter()
            .chain([(diff, &line[..])])
    }

    /// The seed of the changed-word tint for `diff` on its line tint `line`: the line tint a
    /// quarter of the way to `diff`, the tint origin/main drew.
    fn word_seed(diff: u32, line: u32) -> u32 {
        [16, 8, 0].into_iter().fold(0, |seed, shift| {
            let (line, diff) = ((line >> shift) & 0xff, (diff >> shift) & 0xff);
            seed | ((line * 3 + diff) / 4) << shift
        })
    }

    /// The changed-word tint for `diff` on its line tint `line`, fitted as [`Self::fit`]
    /// describes for a palette applied as it is.
    fn word_tint(palette: Palette, roles: SyntaxRoles, diff: u32, line: u32) -> u32 {
        let (lines, on_line) = (Self::lines(palette), [line]);
        let drawn: Vec<_> = Self::word_colors(roles, &lines, diff, &on_line).collect();
        let fit = HighlightFit::new(&drawn, &on_line);
        let (seed, away) = (Self::word_seed(diff, line), Self::away(palette));
        fit.search(seed, away, diff)
            .unwrap_or_else(|| fit.readable(seed, away))
    }

    /// Whether the selection and Find's matches, these highlights drawn with More contrast off,
    /// stand from `line`, a moved line tint, at least [`HIGHLIGHT_DISTANCE`] in OKLab, or as far
    /// as from `off_line`, the line drawn off, where that is less. A raised highlight stands from
    /// every line at least as far as the one drawn off ([`HighlightFit::at_least_as_apart_as`]),
    /// and where none is raised the one drawn off stays, so a line held to this keeps every
    /// highlight on it [`HIGHLIGHT_DISTANCE`] apart, as [`Self::fit`] keeps them off. A line
    /// moved toward the canvas, free of it, came within 0.034 of Omarchy miasma's selection,
    /// which stands 0.044 from the line drawn off.
    fn stay_apart_from(self, line: u32, off_line: u32) -> bool {
        let (line, off_line) = (oklab(line), oklab(off_line));
        [self.selection, self.find_current]
            .into_iter()
            .all(|highlight| {
                let highlight = oklab(highlight);
                let off_apart = oklab_distance(highlight, off_line);
                oklab_distance(highlight, line) >= HIGHLIGHT_DISTANCE.min(off_apart)
            })
    }

    /// The changed-word tint for `diff` on its line tint `line` under More contrast, for a word
    /// whose tint drawn with More contrast off, `off`, lies beyond `line` from the canvas
    /// ([`Palette::more_contrast_fit`]; a word on the line's canvas side steps across it instead,
    /// [`Self::crossed_word_tint`]): `off` where it stands at least [`WORD_LIGHTNESS_GAP`] from
    /// `line` in OKLab lightness, and otherwise `off` lifted away from `line`, on the side of it
    /// `off` lies, lighter or darker, until it does ([`lightness_lifts`]), or as far toward that
    /// as the colors drawn on it allow: each of `roles`, fitted at [`MORE_CONTRAST_TEXT`], but
    /// `removed`, and `diff` read on the lift at least the contrast their counterparts in
    /// `own`, the roles drawn off, read on `off`, and the lift stands from `line` at least as
    /// far as `off` does, in OKLab and in luminance on the same side, so the word is never
    /// marked more faintly. The lift stops at the step before the first on which some color
    /// reads worse than that, never past it, even where a step beyond would read well again;
    /// where the first step reads worse, `off` stays. A lift that runs out of lightness, where
    /// white or black stands nearer `line` than [`WORD_LIGHTNESS_GAP`], stops at the last step
    /// before it.
    ///
    /// More contrast lifts only the faintest tints because their lightness gap is what
    /// colour-blind vision keeps of them; a tint already that far keeps `off` exactly. A lift
    /// on the side of the line toward the text on it lowers that text's contrast, and `text`
    /// and the diff colors are not raised, so such a tint keeps `off`; one on the far side of
    /// its line from its text lifts, and the text reads better for it. Raised instead toward
    /// 7:1 for every color on it and held no nearer its line in luminance, a tint between its
    /// line and its text could only gain by moving toward the line, so it kept `off` as well.
    /// A word on its line's canvas side once lifted toward the canvas, held at half its line's
    /// gap from it, which left it on that side, short of the order canvas, line, word that the
    /// step across now always restores.
    fn lifted_word_tint(
        roles: SyntaxRoles,
        own: SyntaxRoles,
        off: u32,
        diff: u32,
        line: u32,
    ) -> u32 {
        let targets: Vec<_> = roles
            .word_colors()
            .into_iter()
            .zip(own.word_colors())
            .chain([(diff, diff)])
            .map(|(color, own)| (color, custom::contrast(own, off)))
            .collect();
        let reads = HighlightFit::targeted(&targets, &[]);
        lightness_lifts(off, line)
            .into_iter()
            .take_while(|&lift| reads.reading(lift) == Reading::Target)
            .last()
            .unwrap_or(off)
    }

    /// The changed-word tint under More contrast for a word whose tint does not stand beyond its
    /// line tint `line` from the canvas of `chosen`, the palette as chosen ([`stands_beyond`]):
    /// one drawn with More contrast off on the line's canvas side, or one lifted
    /// ([`Self::lifted_word_tint`]) and its line moved short of [`HIGHLIGHT_DISTANCE`] from it.
    /// It is the step beyond `line`
    /// ([`beyond_line`]), in the hue and chroma of the word's seed ([`Self::word_seed`]), else
    /// halfway to its diff color's, else the diff color's own, that
    /// stands at least [`HIGHLIGHT_DISTANCE`] from `line` and [`WORD_LIGHTNESS_GAP`] from it in
    /// OKLab lightness, or where the colors drawn on the word bind first, the last step before
    /// one on which some color would not read that stands apart: [`HIGHLIGHT_DISTANCE`] from
    /// `line` and [`HIGHLIGHT_LIGHTNESS_GAP`] beyond it in lightness. The
    /// colors are those drawn on the word as text under More contrast: `text` and every syntax
    /// role in `roles` but `removed`, as raised on the line, and `patch`, the patch text the Diff
    /// view draws on the word ([`PatchText`]); `diff`, the side's diff color, lends the seed its
    /// hue and is drawn on the word only as its rule, fitted to it afterward
    /// ([`Self::word_rule`]). Each reads [`MORE_CONTRAST_TEXT`], or its own contrast on `line`
    /// where that is lower; where no step that stands apart lets every color do so, each reads
    /// the text rule with its rasterization margin, or its own contrast on `line` where lower,
    /// and then the text rule alone. Where no step that stands so apart reads the text rule,
    /// the last step on which every color does that stands [`HIGHLIGHT_DISTANCE`] from `line`,
    /// beyond it but short of the lightness gap, since the text rule is the floor every color
    /// keeps (One Dark's added word, stepped 0.02 beyond its line, drew the raised keyword
    /// `#e090f7` at 4.39:1, and Kanagawa Lotus's removed word, before its text darkened on
    /// 2026-11-05, drew its strings at 4.49:1). With
    /// no step beyond `line` that both reads the text rule and stands [`HIGHLIGHT_DISTANCE`] from
    /// it, `off`, the word's tint drawn with More contrast off, which reads: the nearest step
    /// that stood apart, or the line's own colour, broke the text floor or drew the word as its
    /// line (decision, 2026-11-05 evening; no shipped palette reaches it).
    ///
    /// Why the seed's hue: the word tint is its line tint a quarter of the way to its diff color,
    /// the tint origin/main drew, and the one drawn off lay on the canvas side because it read
    /// better there. Why it may read worse than off: the text on a word lies on the line's far
    /// side from the canvas, so a word stepped across nears it; the order canvas, line, word is
    /// the decided rule (decision, 2026-11-05), and More contrast's 7:1 is kept wherever the step
    /// allows it. Held also to the diff color, which reads about 4.75:1 on its line by the off
    /// fit, a word stepped across had no room and fell under 4.5:1 (Braden's removed 4.42:1).
    /// Why the lightness gap: held to [`HIGHLIGHT_DISTANCE`] alone, a step stood apart through
    /// its seed's chroma with almost no lightness beyond its line (Solarized Dark's added word
    /// `#2d3500`, 0.0004 beyond), a mark colour-blind vision loses; one just-noticeable
    /// difference beyond, the order canvas, line, word holds in lightness too (decision,
    /// 2026-11-05 afternoon).
    fn crossed_word_tint(
        roles: SyntaxRoles,
        [diff, patch]: [u32; 2],
        (line, off): (u32, u32),
        chosen: Palette,
    ) -> u32 {
        let on_line = [line];
        let colors: Vec<u32> = roles.word_colors().into_iter().chain([patch]).collect();
        let seed = Self::word_seed(diff, line);
        // The seed's hue and chroma first, then halfway to the diff color's and the diff
        // color's own, which stand apart from the line with less lightness to spend.
        let rows = [seed, mix(seed, diff, 128), diff].map(|hue| beyond_line(hue, line, chosen));
        let line_lab = oklab(line);
        let from_line = |step: u32| oklab_distance(oklab(step), line_lab) >= HIGHLIGHT_DISTANCE;
        let apart = |step: u32| {
            from_line(step) && lightness_beyond(step, line, chosen) >= HIGHLIGHT_LIGHTNESS_GAP
        };
        // The last step of the first row on which every color reads `rule` that stands `apart`.
        let reading = |rule: f64, apart: &dyn Fn(u32) -> bool| {
            let targets: Vec<_> = colors
                .iter()
                .map(|&color| (color, drawn_target(color, &on_line, rule)))
                .collect();
            let reads = HighlightFit::targeted(&targets, &[]);
            rows.iter().find_map(|steps| {
                steps
                    .iter()
                    .copied()
                    .take_while(|&step| reads.reading(step) == Reading::Target)
                    .filter(|&step| apart(step))
                    .last()
            })
        };
        [MORE_CONTRAST_TEXT, LABEL_RULE + LABEL_MARGIN, LABEL_RULE]
            .into_iter()
            .find_map(|rule| reading(rule, &apart))
            .or_else(|| reading(LABEL_RULE, &from_line))
            .unwrap_or(off)
    }
}

/// Whether `word`, a changed-word tint, stands beyond `line`, its line tint, from the canvas of
/// `chosen` under More contrast: further from the canvas than `line` in OKLab lightness, on the
/// same side of it ([`lightness_beyond`]), and at least [`HIGHLIGHT_DISTANCE`] from `line`. The
/// order canvas, line, word is what makes a changed word read as emphasis of its line.
fn stands_beyond(word: u32, line: u32, chosen: Palette) -> bool {
    lightness_beyond(word, line, chosen) > 0.
        && oklab_distance(oklab(word), oklab(line)) >= HIGHLIGHT_DISTANCE
}

/// How far `color` lies beyond `line`, a line tint, from the canvas of `chosen` in OKLab
/// lightness ([`away_from_canvas`]): positive where it stands further from the canvas than
/// `line`, on the same side, and negative on the line's canvas side.
fn lightness_beyond(color: u32, line: u32, chosen: Palette) -> f64 {
    let line = oklab(line)[0];
    (oklab(color)[0] - line) * away_from_canvas(line, chosen.canvas, chosen.is_light())
}

/// The direction in OKLab lightness that leads from the canvas past a line of lightness
/// `line`: `1.` where the line is lighter than `canvas`, `-1.` where darker, and where level,
/// toward the text, lighter in a dark palette and darker in a light one (`light`).
fn away_from_canvas(line: f64, canvas: u32, light: bool) -> f64 {
    match line.total_cmp(&oklab(canvas)[0]) {
        std::cmp::Ordering::Greater => 1.,
        std::cmp::Ordering::Less => -1.,
        std::cmp::Ordering::Equal if light => -1.,
        std::cmp::Ordering::Equal => 1.,
    }
}

/// The steps beyond `line` from the canvas of `chosen` ([`away_from_canvas`]), nearest first: OKLab
/// lightness moves from `line`'s a thousandth at a time, in `seed`'s hue and chroma, or where
/// sRGB cannot hold that chroma at that lightness, the most of it sRGB holds, in the same hue.
/// Each step is rounded to sRGB and listed only where it stands further beyond `line` in
/// lightness than the last one listed; the last is the first that stands at least
/// [`HIGHLIGHT_DISTANCE`] from `line` and [`WORD_LIGHTNESS_GAP`] from it in lightness, or the
/// last before lightness runs out.
fn beyond_line(seed: u32, line: u32, chosen: Palette) -> Vec<u32> {
    let ([_, a, b], line_lab) = (oklab(seed), oklab(line));
    let direction = away_from_canvas(line_lab[0], chosen.canvas, chosen.is_light());
    let beyond = |lab: [f64; 3]| (lab[0] - line_lab[0]) * direction;
    let (mut steps, mut last) = (Vec::new(), 0.);
    for step in 1..=1000 {
        let lab = [line_lab[0] + direction * f64::from(step) / 1000., a, b];
        if !(0. ..=1.).contains(&lab[0]) {
            break;
        }
        let color = from_linear(linear_in_gamut(lab));
        let color_lab = oklab(color);
        if beyond(color_lab) <= last {
            continue;
        }
        last = beyond(color_lab);
        steps.push(color);
        if last >= WORD_LIGHTNESS_GAP && oklab_distance(color_lab, line_lab) >= HIGHLIGHT_DISTANCE {
            break;
        }
    }
    steps
}

/// What [`EditorHighlights::fit`] fits from under More contrast, from
/// [`Palette::more_contrast_fit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Chosen {
    /// The palette as chosen.
    palette: Palette,
    /// Its editor highlights with More contrast off, which the selection and Find's matches are
    /// raised from.
    off: EditorHighlights,
    /// The added and removed changed-word tints under More contrast, decided beside the line
    /// tints.
    words: [u32; 2],
}

/// The steps that lift `tint` away from `line` in OKLab lightness, on the side of it `tint`
/// lies, nearest first, until the lightness gap reaches [`WORD_LIGHTNESS_GAP`]: lightness moves
/// a thousandth at a time, keeping `tint`'s hue and chroma, or where sRGB cannot hold that
/// chroma at that lightness, the most of it sRGB holds, in the same hue. Each step is rounded to
/// sRGB and listed only where it stands further from `line` in lightness than the last one
/// listed, and at least as far from it as `tint` in OKLab and in luminance contrast, on `tint`'s
/// side in luminance, so no step marks the word more faintly than `tint` (rounding, or chroma
/// the gamut takes, can leave a step nearer in OKLab than `tint`); the last is the first whose
/// gap reaches [`WORD_LIGHTNESS_GAP`], or, where white or black stands nearer `line` than that,
/// the last step before lightness runs out. Empty where `tint` already stands that far or is
/// level with `line`, and where it lies on one side of `line` in lightness and on the other in
/// luminance, as a saturated red can, since every step away in lightness then nears `line` in
/// luminance or crosses it.
fn lightness_lifts(tint: u32, line: u32) -> Vec<u32> {
    use std::cmp::Ordering;
    let ([lightness, a, b], line_lab) = (oklab(tint), oklab(line));
    let gap = |color: u32| (oklab(color)[0] - line_lab[0]).abs();
    let line_luminance = custom::luminance(line);
    let side = |color: u32| custom::luminance(color).total_cmp(&line_luminance);
    let (apart, contrast) = (
        oklab_distance(oklab(tint), line_lab),
        custom::contrast(tint, line),
    );
    let direction = match (lightness.total_cmp(&line_lab[0]), side(tint)) {
        (Ordering::Equal, _) => return Vec::new(),
        (lighter, luminance) if luminance.is_ne() && luminance != lighter => return Vec::new(),
        (Ordering::Greater, _) => 1.,
        (Ordering::Less, _) => -1.,
    };
    let (mut lifts, mut last) = (Vec::new(), tint);
    for step in 1..=1000 {
        if gap(last) >= WORD_LIGHTNESS_GAP {
            break;
        }
        let lab = [lightness + direction * f64::from(step) / 1000., a, b];
        if !(0. ..=1.).contains(&lab[0]) {
            break;
        }
        let lift = from_linear(linear_in_gamut(lab));
        if gap(lift) > gap(last)
            && oklab_distance(oklab(lift), line_lab) >= apart
            && custom::contrast(lift, line) >= contrast
            && (side(tint).is_eq() || side(lift) == side(tint))
        {
            last = lift;
            lifts.push(lift);
        }
    }
    lifts
}

/// The steps that move the line tint `line` away from `word`, the changed-word tint drawn on it,
/// in OKLab lightness, on the line's far side from the word, nearest first, until the word stands
/// [`WORD_LIGHTNESS_GAP`] from it: lightness moves a thousandth at a time, keeping `line`'s hue
/// and chroma, or where sRGB cannot hold that chroma at that lightness, the most of it sRGB
/// holds, in the same hue. Each step is rounded to sRGB and listed only where it stands further
/// from `word` in lightness than the last one listed, and `word` stands from it at least as far
/// as `off_word` stands from `off_line`, the pair drawn with More contrast off, in OKLab and in
/// luminance contrast, on that pair's side in luminance. The steps end before the first that
/// stands nearer `canvas` in lightness than [`WORD_LIGHTNESS_GAP`], or than `line` does where
/// `line` already stands nearer, so such a line never moves toward the canvas, and at the first
/// whose gap reaches [`WORD_LIGHTNESS_GAP`]. Empty where `word` already stands that far or is
/// level with `line`, and where it lies on one side of `line` in lightness and on the other in
/// luminance, since every step away in lightness then nears `word` in luminance or crosses it.
///
/// Why the canvas floor: the line tint is itself the mark of an added or removed line, so it
/// keeps from the editor background the two just-noticeable differences a word keeps from it.
fn line_moves(line: u32, word: u32, (off_word, off_line): (u32, u32), canvas: u32) -> Vec<u32> {
    use std::cmp::Ordering;
    let ([lightness, a, b], word_lab) = (oklab(line), oklab(word));
    let gap = |color: u32| (oklab(color)[0] - word_lab[0]).abs();
    let canvas = oklab(canvas)[0];
    let from_canvas = |color: u32| (oklab(color)[0] - canvas).abs();
    let floor = WORD_LIGHTNESS_GAP.min(from_canvas(line));
    let word_luminance = custom::luminance(word);
    let side = |color: u32| word_luminance.total_cmp(&custom::luminance(color));
    let off_side = custom::luminance(off_word).total_cmp(&custom::luminance(off_line));
    let (apart, contrast) = (
        oklab_distance(oklab(off_word), oklab(off_line)),
        custom::contrast(off_word, off_line),
    );
    let direction = match (word_lab[0].total_cmp(&lightness), side(line)) {
        (Ordering::Equal, _) => return Vec::new(),
        (lighter, luminance) if luminance.is_ne() && luminance != lighter => return Vec::new(),
        (Ordering::Greater, _) => -1.,
        (Ordering::Less, _) => 1.,
    };
    let (mut moves, mut last) = (Vec::new(), line);
    for step in 1..=1000 {
        if gap(last) >= WORD_LIGHTNESS_GAP {
            break;
        }
        let lab = [lightness + direction * f64::from(step) / 1000., a, b];
        if !(0. ..=1.).contains(&lab[0]) {
            break;
        }
        let moved = from_linear(linear_in_gamut(lab));
        if from_canvas(moved) < floor {
            break;
        }
        if gap(moved) > gap(last)
            && oklab_distance(word_lab, oklab(moved)) >= apart
            && custom::contrast(word, moved) >= contrast
            && (off_side.is_eq() || side(moved) == off_side)
        {
            last = moved;
            moves.push(moved);
        }
    }
    moves
}

/// The OKLab lightness of `color` as protanopia and deuteranopia see it, in that order: the
/// color through Machado, Oliveira and Fernandes' model (2009, severity 1, on linear sRGB), the
/// model the design reviews measured, rounded to sRGB.
fn colour_blind_lightness(color: u32) -> [f64; 2] {
    const MODELS: [[[f64; 3]; 3]; 2] = [
        [
            [0.152_286, 1.052_583, -0.204_868],
            [0.114_503, 0.786_281, 0.099_216],
            [-0.003_882, -0.048_116, 1.051_998],
        ],
        [
            [0.367_322, 0.860_646, -0.227_968],
            [0.280_085, 0.672_501, 0.047_413],
            [-0.011_820, 0.042_940, 0.968_881],
        ],
    ];
    let linear = [16, 8, 0].map(|shift| custom::linear_channel((color >> shift) & 0xff));
    MODELS.map(|model| {
        let seen = from_linear(model.map(|row| {
            let channel: f64 = row.iter().zip(linear).map(|(m, c)| m * c).sum();
            channel.clamp(0., 1.)
        }));
        oklab(seen)[0]
    })
}

/// Whether `word`, a changed-word tint, stands from `line`, a moved line tint, in OKLab
/// lightness at least as far as from `off_line`, the line drawn off, as protanopia and as
/// deuteranopia see the three ([`colour_blind_lightness`]).
///
/// Why: the word's lightness gap is what colour-blind vision keeps of it, so a line moved for
/// it must not narrow that gap there. Moved by normal vision alone, red-ish words, which read
/// darker in a protanopia model than their lines where they read lighter in normal vision, drew
/// nearer their lines there (Omarchy kanagawa's removed word 0.020 to 0.018 from its line,
/// alacritty-dark's 0.016 to 0.013, catppuccin-latte's added word crossing its line).
fn keeps_colour_blind_gaps(word: u32, line: u32, off_line: u32) -> bool {
    let [word, line, off_line] = [word, line, off_line].map(colour_blind_lightness);
    (0..2).all(|model| (word[model] - line[model]).abs() >= (word[model] - off_line[model]).abs())
}

/// The linear-light sRGB channels of OKLab coordinates, with their chroma lowered, hue kept,
/// as little as sRGB needs to hold them.
fn linear_in_gamut([lightness, a, b]: [f64; 3]) -> [f64; 3] {
    let at = |share: f64| linear_from_oklab_unclipped([lightness, a * share, b * share]);
    let holds = |linear: [f64; 3]| linear.iter().all(|&channel| (0. ..=1.).contains(&channel));
    let mut share = 1.;
    if !holds(at(share)) {
        let (mut held, mut lost) = (0., 1.);
        for _ in 0..24 {
            let middle = (held + lost) / 2.;
            if holds(at(middle)) {
                held = middle;
            } else {
                lost = middle;
            }
        }
        share = held;
    }
    at(share).map(|channel| channel.clamp(0., 1.))
}

/// The contrast a color drawn on a highlight should reach there: `rule`, or the color's own
/// lowest contrast on the lines it is drawn on where that is lower, never below the text rule.
fn drawn_target(color: u32, on: &[u32], rule: f64) -> f64 {
    let color = custom::luminance(color);
    on.iter()
        .map(|&line| custom::luminance_contrast(color, custom::luminance(line)))
        .fold(rule, f64::min)
        .max(LABEL_RULE)
}

/// The colors one highlight carries and sits on, measured once for its fitting.
struct HighlightFit {
    /// Each color drawn on the highlight: its luminance and the contrast it should reach there,
    /// 4.75:1 or its own lowest contrast on the lines it is drawn on, never below 4.5:1 (a
    /// highlight under More contrast sets its own, [`EditorHighlights::raised`]).
    drawn: Vec<(f64, f64)>,
    /// Each background the highlight sits on, and each highlight drawn beside it
    /// ([`Self::apart_from`]), with how it stands apart from it.
    lines: Vec<Apart>,
}

/// How a highlight stands apart from one color: a background it sits on, or another highlight
/// drawn beside it ([`HighlightFit::apart_from`]).
struct Apart {
    /// The color's OKLab coordinates.
    oklab: [f64; 3],
    /// The color's relative luminance.
    luminance: f64,
    /// The distance, in ΔEOK, the highlight stands from it at least: [`HIGHLIGHT_DISTANCE`], or
    /// more for a raised highlight ([`HighlightFit::at_least_as_apart_as`]).
    distance: f64,
    /// The gap in OKLab lightness the highlight keeps from it at least: none from a background,
    /// and [`HIGHLIGHT_LIGHTNESS_GAP`] from another highlight drawn beside it under More contrast
    /// ([`HighlightFit::apart_from`]), so that distance never comes from chroma alone.
    lightness: f64,
    /// The side of it, in OKLab lightness, the highlight keeps, lighter (`Greater`) or darker
    /// (`Less`), or `None` for either: a raised highlight keeps the side of the one drawn with
    /// More contrast off ([`HighlightFit::at_least_as_apart_as`]).
    side: Option<std::cmp::Ordering>,
    /// The relative luminance the highlight keeps beyond, with the side of the color's it lies
    /// on: at least it where that side is lighter (`Greater`), at most it where darker (`Less`),
    /// or `None` for no bound. A raised highlight keeps the luminance of the one drawn with More
    /// contrast off, so its luminance contrast with the color is at least the off one's, on the
    /// same side ([`HighlightFit::at_least_as_apart_as`]).
    beyond: Option<(std::cmp::Ordering, f64)>,
}

/// How a candidate highlight reads under the colors drawn on it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reading {
    /// Some color misses the text rule.
    Unreadable,
    /// Every color reaches the text rule, but not its target.
    Rule,
    /// Every color reaches its target.
    Target,
}

impl HighlightFit {
    /// Each color in `drawn` with the lines it is drawn on, to reach the text rule plus the
    /// rasterization margin on the highlight ([`drawn_target`]).
    fn new(drawn: &[(u32, &[u32])], lines: &[u32]) -> Self {
        let targets: Vec<_> = drawn
            .iter()
            .map(|&(color, on)| (color, drawn_target(color, on, LABEL_RULE + LABEL_MARGIN)))
            .collect();
        Self::targeted(&targets, lines)
    }

    /// Each color in `drawn` with the contrast it should reach on the highlight.
    fn targeted(drawn: &[(u32, f64)], lines: &[u32]) -> Self {
        Self {
            drawn: drawn
                .iter()
                .map(|&(color, target)| (custom::luminance(color), target))
                .collect(),
            lines: lines
                .iter()
                .map(|&line| Apart {
                    oklab: oklab(line),
                    luminance: custom::luminance(line),
                    distance: HIGHLIGHT_DISTANCE,
                    lightness: 0.,
                    side: None,
                    beyond: None,
                })
                .collect(),
        }
    }

    fn reading(&self, color: u32) -> Reading {
        let color = custom::luminance(color);
        self.drawn
            .iter()
            .map(|&(drawn, target)| {
                let contrast = custom::luminance_contrast(drawn, color);
                if contrast >= target {
                    Reading::Target
                } else if contrast >= LABEL_RULE {
                    Reading::Rule
                } else {
                    Reading::Unreadable
                }
            })
            .min()
            .unwrap_or(Reading::Target)
    }

    /// Whether a color of this luminance before rounding to 8 bits could read at the text rule
    /// once rounded: rounding the three channels moves a contrast ratio by under 2%, so a margin
    /// of 3% of the rule keeps every candidate that can.
    fn may_read(&self, luminance: f64) -> bool {
        self.drawn
            .iter()
            .all(|&(drawn, _)| custom::luminance_contrast(drawn, luminance) >= LABEL_RULE * 0.97)
    }

    /// This fit for a highlight that stands apart from each line as `off` does: on the same
    /// side of it in OKLab lightness, lighter or darker, and at least as far from it, never
    /// nearer than [`HIGHLIGHT_DISTANCE`], and with at least `off`'s luminance contrast with it,
    /// on the same side in luminance. Under More contrast a raised highlight never marks its
    /// text more faintly than the highlight drawn with it off, and never with the other polarity:
    /// a changed-word tint that turned darker than its lighter line tint read as a gap in the
    /// line (Midnight's added words, #0f1d19 on #19322d), and a selection that turned near black
    /// lost its hue (Midnight's, #0b3e3e to #000001). Its lightness against the line is the mark
    /// that survives colour-blind vision, so distance gained through saturation alone does not
    /// stand in for it: held only to the distance, every raised word tint took its line's
    /// luminance, 1.00 to 1.01:1, where the ones drawn off stand 1.08 to 1.37:1 from theirs, and
    /// in a protanopia and deuteranopia model Midnight's removed word turned darker than its line
    /// and Porcelain's added word fell from 0.067 to 0.018 from it.
    fn at_least_as_apart_as(mut self, off: u32) -> Self {
        let (off, off_luminance) = (oklab(off), custom::luminance(off));
        for apart in &mut self.lines {
            apart.distance = apart.distance.max(oklab_distance(off, apart.oklab));
            apart.side = match off[0].total_cmp(&apart.oklab[0]) {
                std::cmp::Ordering::Equal => None,
                side => Some(side),
            };
            apart.beyond = match off_luminance.total_cmp(&apart.luminance) {
                std::cmp::Ordering::Equal => None,
                side => Some((side, off_luminance)),
            };
        }
        self
    }

    /// This fit for a highlight that also stands at least `distance` from `color`, another
    /// highlight drawn beside it, on either side of it, and at least [`HIGHLIGHT_LIGHTNESS_GAP`]
    /// from it in OKLab lightness: under More contrast, Find's current match from the selection,
    /// and Find's other matches from the selection, the current match and the changed-word tints.
    fn apart_from(mut self, color: u32, distance: f64) -> Self {
        self.lines.push(Apart {
            oklab: oklab(color),
            luminance: custom::luminance(color),
            distance,
            lightness: HIGHLIGHT_LIGHTNESS_GAP,
            side: None,
            beyond: None,
        });
        self
    }

    /// The candidate nearest `seed` that reaches every target and stands apart, searched as
    /// [`Self::search`] searches, `tint` lending its hue, or `None` where none does.
    fn raise(&self, seed: u32, away: u32, tint: u32) -> Option<u32> {
        self.search(seed, away, tint)
            .filter(|&highlight| self.reading(highlight) == Reading::Target)
    }

    /// Whether `color` stands from every line at least its distance ([`HIGHLIGHT_DISTANCE`]
    /// unless [`Self::at_least_as_apart_as`] raised it) and its lightness gap, on its side where
    /// it keeps one, and beyond the luminance it keeps there, if any.
    fn distinct(&self, color: u32) -> bool {
        let (lab, luminance) = (oklab(color), custom::luminance(color));
        self.lines.iter().all(|apart| {
            oklab_distance(lab, apart.oklab) >= apart.distance
                && (lab[0] - apart.oklab[0]).abs() >= apart.lightness
                && apart
                    .side
                    .is_none_or(|side| lab[0].total_cmp(&apart.oklab[0]) == side)
                && apart.beyond.is_none_or(|(side, bound)| match side {
                    std::cmp::Ordering::Greater => luminance >= bound,
                    _ => luminance <= bound,
                })
        })
    }

    /// `seed` fitted as [`EditorHighlights::fit`] describes, when a candidate both reads and
    /// stands apart; `tint` lends its hue to a seed too grey to have one of its own.
    fn search(&self, seed: u32, away: u32, tint: u32) -> Option<u32> {
        let origin = oklab(seed);
        if self.reading(seed) == Reading::Target && self.distinct(seed) {
            return Some(seed);
        }
        // Candidates in order of the distance from the seed each was built at, merged from rows
        // along which that distance grows: the steps toward `away`, by their own distance, and
        // each chroma row of [`chroma_rows`], by its unclipped coordinates. Only candidates up to
        // the first that fits are built. A chroma candidate's luminance before rounding to 8 bits
        // screens it, so only those near enough to read are rounded and measured. The first that
        // reaches its targets and stands apart wins; the first at the rule that stands apart
        // stands in when none does.
        let end = oklab(away)[0];
        let rows = chroma_rows(seed, tint);
        let at = |row: usize, step: u32| match row.checked_sub(1) {
            None => {
                let color = mix(seed, away, step);
                (
                    oklab_distance(oklab(color), origin),
                    Candidate::Color(color),
                )
            }
            Some(chroma) => {
                let lightness = origin[0] + (end - origin[0]) * f64::from(step) / 64.;
                let lab = [lightness, rows[chroma][0], rows[chroma][1]];
                (oklab_distance(lab, origin), Candidate::Lab(lab))
            }
        };
        let next = |row: usize, step: u32| {
            (step <= if row == 0 { 256 } else { 64 }).then(|| {
                let (distance, candidate) = at(row, step);
                Nearest {
                    distance,
                    row,
                    step,
                    candidate,
                }
            })
        };
        let mut heap = (0..=rows.len())
            .filter_map(|row| next(row, 0))
            .collect::<BinaryHeap<_>>();
        let mut fallback = None;
        while let Some(Nearest {
            row,
            step,
            candidate,
            ..
        }) = heap.pop()
        {
            heap.extend(next(row, step + 1));
            let color = match candidate {
                Candidate::Color(color) => color,
                Candidate::Lab(lab) => {
                    let linear = linear_from_oklab(lab);
                    if !self.may_read(linear_luminance(linear)) {
                        continue;
                    }
                    from_linear(linear)
                }
            };
            match self.reading(color) {
                Reading::Target if self.distinct(color) => return Some(color),
                Reading::Rule if fallback.is_none() && self.distinct(color) => {
                    fallback = Some(color);
                }
                _ => {}
            }
        }
        fallback
    }

    /// The first step from `seed` toward `away` that reads at the text rule, for a palette where
    /// no candidate also stands apart.
    fn readable(&self, seed: u32, away: u32) -> u32 {
        toward_away(seed, away)
            .find(|&color| self.reading(color) != Reading::Unreadable)
            .unwrap_or(away)
    }
}

/// `seed` and each 256th of the way from it to `away`.
fn toward_away(seed: u32, away: u32) -> impl Iterator<Item = u32> {
    (0..=256).map(move |step| mix(seed, away, step))
}

/// Below this OKLab chroma a color is treated as grey, with no hue of its own to keep.
const GREY_CHROMA: f64 = 0.01;

/// A highlight candidate: an sRGB color, or OKLab coordinates the search rounds to one, clipped
/// to sRGB, only when it reaches them.
enum Candidate {
    Color(u32),
    Lab([f64; 3]),
}

/// A candidate on the search's heap, which pops the nearest to the seed first (then the
/// earliest row and step, so the order is deterministic).
struct Nearest {
    distance: f64,
    row: usize,
    step: u32,
    candidate: Candidate,
}

impl Nearest {
    fn key(&self) -> (f64, usize, u32) {
        (self.distance, self.row, self.step)
    }
}

impl PartialEq for Nearest {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Nearest {}

impl PartialOrd for Nearest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nearest {
    /// Reversed, so the max-heap pops the nearest.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let ((d, row, step), (other_d, other_row, other_step)) = (self.key(), other.key());
        other_d
            .total_cmp(&d)
            .then(other_row.cmp(&row))
            .then(other_step.cmp(&step))
    }
}

/// The opponent axes of each chroma row a highlight that cannot stand apart by lightness alone
/// searches: `seed`'s chroma raised by 0.01 at a time up to 0.1 more, in `seed`'s hue and in
/// `tint`'s (a grey has no hue; with neither, blue). Along a row the lightness moves from
/// `seed`'s toward `away`'s in 64ths; coordinates outside sRGB are clipped when converted, and the
/// fitting measures the clipped color.
fn chroma_rows(seed: u32, tint: u32) -> Vec<[f64; 2]> {
    let [_, a, b] = oklab(seed);
    let chroma = a.hypot(b);
    let hue = |color: u32| {
        let [_, a, b] = oklab(color);
        let chroma = a.hypot(b);
        (chroma >= GREY_CHROMA).then(|| [a / chroma, b / chroma])
    };
    let mut hues = [hue(seed), hue(tint)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    hues.dedup();
    if hues.is_empty() {
        hues.push([-0.105, -0.995]);
    }
    hues.into_iter()
        .flat_map(|hue| {
            (1..=10).map(move |raise| {
                let chroma = chroma + f64::from(raise) * 0.01;
                [chroma * hue[0], chroma * hue[1]]
            })
        })
        .collect()
}

/// OKLab coordinates (lightness and the two opponent axes) of an `0xrrggbb` color.
fn oklab(color: u32) -> [f64; 3] {
    let [r, g, b] = [16, 8, 0].map(|shift| custom::linear_channel((color >> shift) & 0xff));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// The linear-light sRGB channels of OKLab coordinates, each clipped to 0..=1.
fn linear_from_oklab(lab: [f64; 3]) -> [f64; 3] {
    linear_from_oklab_unclipped(lab).map(|channel| channel.clamp(0., 1.))
}

/// The linear-light sRGB channels of OKLab coordinates, outside 0..=1 where sRGB cannot show
/// them.
fn linear_from_oklab_unclipped([lightness, a, b]: [f64; 3]) -> [f64; 3] {
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ]
}

/// The WCAG relative luminance of linear-light sRGB channels.
fn linear_luminance([r, g, b]: [f64; 3]) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The `0xrrggbb` color whose channels are nearest linear-light values.
fn from_linear(linear: [f64; 3]) -> u32 {
    linear
        .into_iter()
        .zip([16, 8, 0])
        .fold(0, |color, (channel, shift)| {
            color | custom::encode_linear(channel) << shift
        })
}

/// ΔEOK: the Euclidean distance between two OKLab colors.
fn oklab_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    #[serde(other)]
    Comfortable,
}

impl Density {
    pub const ALL: [Self; 2] = [Self::Comfortable, Self::Compact];

    pub fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "Comfortable",
            Self::Compact => "Compact",
        }
    }

    pub fn history_row_height(self) -> f32 {
        self.history_row_height_at_scale(ui_scale())
    }
    pub(super) fn history_row_height_at_scale(self, scale: f32) -> f32 {
        scale
            * match self {
                Self::Comfortable => 34.,
                Self::Compact => 28.,
            }
    }

    pub fn file_row_height(self) -> f32 {
        self.file_row_height_at_scale(ui_scale())
    }
    pub(super) fn file_row_height_at_scale(self, scale: f32) -> f32 {
        scale
            * match self {
                Self::Comfortable => 44.,
                Self::Compact => 34.,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::custom::{CustomTheme, TokenKind, channel_distance, contrast, luminance};
    use super::*;
    use gpui_kit as gpui;
    use gpui_kit::{Background, Hsla};

    /// In every palette a dialog's backdrop dims the page until the dialog's
    /// surface stands at least 1.05:1 lighter than every page surface beneath
    /// it, with no more than 60% black and no more black than that takes;
    /// where 60% cannot lift a near-black canvas that far, the dialog is at
    /// least level with every dimmed surface wherever 60% allows that. With
    /// the kit's 20% black, Midnight's dialog was darker than History's
    /// dimmed panel (1.03:1 the other way) and 1.00:1 over the dimmed
    /// subtle; there its edge keeps 3:1 against the dialog and the dimmed
    /// page alike.
    #[test]
    fn every_dialog_stands_above_the_page_its_backdrop_dims() {
        /// The least lift of `canvas` over the page surfaces dimmed by
        /// `alpha`, below 1 where one dimmed surface is lighter.
        fn lift(palette: Palette, alpha: f64) -> f64 {
            [palette.canvas, palette.panel, palette.subtle]
                .map(|surface| {
                    let dimmed = backdrop_over(surface, alpha);
                    let ratio = contrast(palette.canvas, dimmed);
                    if luminance(palette.canvas) >= luminance(dimmed) {
                        ratio
                    } else {
                        1. / ratio
                    }
                })
                .into_iter()
                .fold(f64::INFINITY, f64::min)
        }
        let mut lifted = 0;
        let mut failures = Vec::new();
        for (name, palette) in test_palettes() {
            let alpha = palette.dialog_backdrop();
            let kit = if palette.is_light() { 0.05 } else { 0.2 };
            if !(kit..=f64::from(DIALOG_BACKDROP_MAX_PERCENT) / 100.).contains(&alpha) {
                failures.push(format!("{name}: backdrop {alpha}"));
            }
            let most = lift(palette, f64::from(DIALOG_BACKDROP_MAX_PERCENT) / 100.);
            // Where even 60% cannot leave the dialog level with the dimmed
            // page, as over a black canvas, more black would not help: the
            // kit's own dim stays.
            if most < 1. {
                if (alpha - kit).abs() > 1e-9 {
                    failures.push(format!("{name}: {alpha} where no dim helps"));
                }
                continue;
            }
            let floor = if most >= DIALOG_LIFT { DIALOG_LIFT } else { 1. };
            let reached = lift(palette, alpha);
            if reached < floor {
                failures.push(format!("{name}: {reached:.3}:1 at {alpha}, needs {floor}"));
            }
            if alpha > kit + 0.005 && lift(palette, alpha - 0.01) >= floor {
                failures.push(format!("{name}: {alpha} where {} lifts it", alpha - 0.01));
            }
            lifted += usize::from(floor == DIALOG_LIFT);
        }
        assert!(failures.is_empty(), "{failures:#?}");
        assert!(lifted >= 20, "the built-in palettes lift: {lifted}");

        let midnight = ThemeChoice::Midnight.palette();
        let alpha = midnight.dialog_backdrop();
        assert!(
            alpha > 0.2,
            "Midnight's backdrop {alpha} darkens past the kit's"
        );
        for surface in [midnight.canvas, midnight.panel, midnight.subtle] {
            let dimmed = backdrop_over(surface, alpha);
            let edge = contrast(midnight.border, dimmed);
            assert!(
                edge >= POPUP_EDGE,
                "Midnight's edge over #{dimmed:06x}: {edge:.3}:1"
            );
        }
        assert!(contrast(midnight.border, midnight.canvas) >= POPUP_EDGE);
    }

    /// Applying a palette installs its backdrop as the kit's overlay, which
    /// every dialog draws beneath itself.
    #[gpui::test]
    fn the_applied_overlay_is_the_palettes_backdrop(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            for choice in [ThemeChoice::Midnight, ThemeChoice::Daylight] {
                choice.apply(None, cx);
                let overlay = Theme::global(cx).colors.overlay;
                let alpha = choice.palette().dialog_backdrop() as f32;
                assert!(
                    overlay.l == 0. && (overlay.a - alpha).abs() < 1e-6,
                    "{choice:?}: overlay {overlay:?}, backdrop {alpha}"
                );
            }
        });
    }

    /// A probe that opens a bare kit dialog holding `dialog-edge-probe`.
    struct DialogProbe;

    impl gpui::Render for DialogProbe {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui_kit::{ParentElement as _, Styled as _};
            gpui::div()
                .size_full()
                .children(gpui_kit::component::Root::render_dialog_layer(window, cx))
        }
    }

    /// A window holding [`DialogProbe`] with its dialog open.
    pub(crate) fn dialog_probe(cx: &mut gpui::TestAppContext) -> &mut gpui::VisualTestContext {
        use gpui_kit::component::{Root, WindowExt as _};
        use gpui_kit::{AppContext as _, InteractiveElement as _, ParentElement as _, Styled as _};
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| DialogProbe);
            Root::new(probe, window, cx)
        });
        cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(600.)));
        cx.update(|window, cx| {
            window.open_dialog(cx, |dialog, _, _| {
                dialog.title("Probe").child(
                    gpui::div()
                        .debug_selector(|| "dialog-edge-probe".into())
                        .h(gpui::px(40.)),
                )
            })
        });
        crate::tags::tests::draw(cx);
        crate::tags::tests::draw(cx);
        cx
    }

    /// The border colours of the dialog panel the last frame painted: the
    /// smallest quad with a 1 px border on every side around the probe's
    /// content, each colour once.
    pub(crate) fn painted_dialog_edge(cx: &mut gpui::VisualTestContext) -> Vec<Hsla> {
        let content = cx
            .debug_bounds("dialog-edge-probe")
            .expect("the dialog's content");
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let logical = |value: gpui_kit::ScaledPixels| gpui::px(value.as_f32() / scale);
            let quads = window.painted_quads();
            let bordered = |quad: &&gpui_kit::Quad| {
                let widths = quad.border_widths;
                [widths.top, widths.right, widths.bottom, widths.left]
                    .iter()
                    .all(|width| logical(*width) == gpui::px(1.))
            };
            let holds = |quad: &&gpui_kit::Quad| {
                logical(quad.bounds.left()) < content.left()
                    && logical(quad.bounds.top()) < content.top()
                    && logical(quad.bounds.right()) > content.right()
                    && logical(quad.bounds.bottom()) > content.bottom()
            };
            let area = |quad: &&gpui_kit::Quad| {
                logical(quad.bounds.size.width).as_f32() * logical(quad.bounds.size.height).as_f32()
            };
            let panel = quads
                .iter()
                .filter(bordered)
                .filter(holds)
                .min_by(|a, b| area(a).total_cmp(&area(b)))
                .expect("the dialog paints a bordered panel around its content")
                .bounds;
            let mut edges = Vec::new();
            for quad in quads
                .iter()
                .filter(|quad| quad.bounds == panel)
                .filter(bordered)
            {
                if !edges.contains(&quad.border_color) {
                    edges.push(quad.border_color);
                }
            }
            edges
        })
    }

    /// A kit dialog draws its panel's 1 px edge in the theme's `dialog_edge`
    /// where an application sets one, and in the shared `border` otherwise,
    /// as upstream does. The page's `border` is fitted against the page, not
    /// against a dialog over a dimmed page: 1.28:1 in Braden.
    #[gpui::test]
    fn a_dialog_draws_its_edge_in_the_themes_dialog_edge(cx: &mut gpui::TestAppContext) {
        let cx = dialog_probe(cx);
        let unset = cx.update(|_, cx| {
            let theme = Theme::global_mut(cx);
            theme.dialog_edge = None;
            theme.border
        });
        crate::tags::tests::draw(cx);
        assert_eq!(painted_dialog_edge(cx), vec![unset], "unset: the border");
        let edge = Hsla::from(rgb(0x7a3e9d));
        cx.update(|_, cx| Theme::global_mut(cx).dialog_edge = Some(edge));
        crate::tags::tests::draw(cx);
        assert_eq!(painted_dialog_edge(cx), vec![edge], "set: the dialog edge");
    }

    /// Enter on a Button Tab focused in a dialog's body presses that Button,
    /// once, as the key goes down, and runs no default action: the ring
    /// shows the control the keyboard acts on (`DESIGN.md`, "Keyboard").
    /// A held Enter's repeats and its release press nothing more. In a
    /// single-line field and on the default Button, Enter is the dialog's
    /// default action, as before, and a multi-line field takes it as a new
    /// line. Bound to the kit's Confirm, Enter on the body's Button ran the
    /// default action and pressed nothing.
    #[gpui::test]
    fn enter_presses_the_focused_button_in_a_dialog(cx: &mut gpui::TestAppContext) {
        use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
        use gpui_kit::component::{Root, WindowExt as _};
        use gpui_kit::{
            AppContext as _, Focusable as _, KeyDownEvent, Keystroke, ParentElement as _,
        };
        use std::{cell::Cell, rc::Rc};
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| DialogProbe);
            Root::new(probe, window, cx)
        });
        cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(600.)));
        let (presses, confirms) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let press = cx.update(|_, cx| cx.focus_handle());
        let (field, notes) = cx.update(|window, cx| {
            (
                cx.new(|cx| InputState::new(window, cx)),
                cx.new(|cx| TextareaState::new(window, cx)),
            )
        });
        cx.update(|window, cx| {
            let (presses, confirms) = (presses.clone(), confirms.clone());
            let (press, field, notes) = (press.clone(), field.clone(), notes.clone());
            window.open_alert_dialog(cx, move |dialog, _, _| {
                let (presses, confirms) = (presses.clone(), confirms.clone());
                dialog
                    .title("Probe")
                    .child(Textarea::new(&notes))
                    .child(Input::new(&field))
                    .child(
                        crate::button("probe-press", "Press", "", false)
                            .track_focus(&press)
                            .on_click(move |_, _, _| presses.set(presses.get() + 1)),
                    )
                    .footer(crate::tags::dialog_footer("OK", Some("Cancel")))
                    .on_ok(move |_, _, _| {
                        confirms.set(confirms.get() + 1);
                        false
                    })
            })
        });
        crate::tags::tests::draw(cx);
        let focused = |cx: &mut gpui::VisualTestContext, handle: &gpui::FocusHandle| {
            cx.update(|window, _| handle.is_focused(window))
        };
        let field_focus = cx.update(|_, cx| field.read(cx).focus_handle(cx));
        cx.update(|window, cx| window.focus(&field_focus, cx));
        crate::tags::tests::draw(cx);
        cx.simulate_keystrokes("tab");
        crate::tags::tests::draw(cx);
        assert!(focused(cx, &press), "Tab moves from the field onto Press");

        cx.simulate_keystrokes("enter");
        assert_eq!(
            (presses.get(), confirms.get()),
            (1, 0),
            "Enter presses Press"
        );
        for _ in 0..2 {
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse("enter").unwrap(),
                is_held: true,
                prefer_character_input: false,
            });
        }
        cx.simulate_keystrokes("enter");
        assert_eq!(
            (presses.get(), confirms.get()),
            (2, 0),
            "a held Enter's repeats press nothing; a fresh press presses once"
        );
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));

        cx.update(|window, cx| window.focus(&field_focus, cx));
        crate::tags::tests::draw(cx);
        cx.simulate_keystrokes("enter");
        assert_eq!(
            (presses.get(), confirms.get()),
            (2, 1),
            "in the field, the default"
        );

        let notes_focus = cx.update(|_, cx| notes.read(cx).focus_handle(cx));
        cx.update(|window, cx| window.focus(&notes_focus, cx));
        crate::tags::tests::draw(cx);
        cx.simulate_keystrokes("enter");
        assert_eq!(
            (presses.get(), confirms.get()),
            (2, 1),
            "in the notes, no default"
        );
        assert_eq!(cx.update(|_, cx| notes.read(cx).value().to_string()), "\n");

        // The default Button: the footer's last, after Cancel.
        cx.update(|window, cx| window.focus(&press, cx));
        crate::tags::tests::draw(cx);
        cx.simulate_keystrokes("tab tab");
        crate::tags::tests::draw(cx);
        cx.simulate_keystrokes("enter");
        assert_eq!(
            (presses.get(), confirms.get()),
            (2, 2),
            "on OK, the default"
        );
    }

    /// A window holding the dialog layer, 800 × 600, with nothing open.
    fn enter_probe(cx: &mut gpui::TestAppContext) -> &mut gpui::VisualTestContext {
        use gpui_kit::AppContext as _;
        use gpui_kit::component::Root;
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| DialogProbe);
            Root::new(probe, window, cx)
        });
        cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(600.)));
        cx
    }

    fn key_down(cx: &mut gpui::VisualTestContext, key: &str, is_held: bool) {
        cx.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: gpui_kit::Keystroke::parse(key).unwrap(),
            is_held,
            prefer_character_input: false,
        });
        crate::tags::tests::draw(cx);
    }

    fn key_up(cx: &mut gpui::VisualTestContext, key: &str) {
        cx.simulate_event(gpui_kit::KeyUpEvent {
            keystroke: gpui_kit::Keystroke::parse(key).unwrap(),
        });
        crate::tags::tests::draw(cx);
    }

    /// Opens dialog A, its focused body Button Delete… opening confirmation
    /// B, whose default action counts a write: over A where `nested`, else
    /// once A has closed, as Git profiles' Delete… opens Delete saved
    /// profile. Returns the writes B ran.
    fn open_delete(
        cx: &mut gpui::VisualTestContext,
        nested: bool,
    ) -> std::rc::Rc<std::cell::Cell<usize>> {
        use gpui_kit::ParentElement as _;
        use gpui_kit::component::WindowExt as _;
        let writes = std::rc::Rc::new(std::cell::Cell::new(0));
        let press = cx.update(|_, cx| cx.focus_handle());
        cx.update(|window, cx| {
            let (writes, press) = (writes.clone(), press.clone());
            window.open_alert_dialog(cx, move |dialog, _, _| {
                let writes = writes.clone();
                dialog
                    .title("A")
                    .child(
                        crate::button("delete", "Delete…", "", false)
                            .track_focus(&press)
                            .on_click(move |_, window, cx| {
                                if !nested {
                                    window.close_dialog(cx);
                                }
                                let writes = writes.clone();
                                window.open_alert_dialog(cx, move |dialog, _, _| {
                                    let writes = writes.clone();
                                    dialog
                                        .title("B")
                                        .footer(crate::tags::dialog_footer(
                                            "Delete profile",
                                            Some("Cancel"),
                                        ))
                                        .on_ok(move |_, _, _| {
                                            writes.set(writes.get() + 1);
                                            true
                                        })
                                });
                            }),
                    )
                    .footer(crate::tags::dialog_footer("Close", None))
            })
        });
        crate::tags::tests::draw(cx);
        cx.update(|window, cx| window.focus(&press, cx));
        crate::tags::tests::draw(cx);
        writes
    }

    /// Enter held on a body Button that opens a confirmation, over its
    /// dialog (`nested`) or in its place, runs no write: the press opens it,
    /// and the held key's repeats reach a dialog that waits for that press
    /// to come up and confirms only on a fresh one. A released press leaves
    /// it open, and a fresh Enter then confirms it. The dialog's `Confirm`
    /// binding took every repeat, so one press ran the write.
    fn held_enter_on_a_button_that_opens_a_confirmation(
        cx: &mut gpui::TestAppContext,
        nested: bool,
    ) {
        use gpui_kit::component::WindowExt as _;
        let cx = enter_probe(cx);
        let writes = open_delete(cx, nested);
        key_down(cx, "enter", false);
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        key_down(cx, "enter", true);
        key_down(cx, "enter", true);
        key_up(cx, "enter");
        assert_eq!(
            writes.get(),
            0,
            "nested {nested}: a held Enter ran no write"
        );
        key_down(cx, "enter", false);
        key_up(cx, "enter");
        assert_eq!(writes.get(), 1, "nested {nested}: a fresh Enter confirms");
    }

    #[gpui::test]
    fn a_held_enter_never_confirms_what_it_opened_over_its_dialog(cx: &mut gpui::TestAppContext) {
        held_enter_on_a_button_that_opens_a_confirmation(cx, true);
    }

    #[gpui::test]
    fn a_held_enter_never_confirms_what_it_opened_in_its_dialogs_place(
        cx: &mut gpui::TestAppContext,
    ) {
        held_enter_on_a_button_that_opens_a_confirmation(cx, false);
    }

    /// Under X11 a held key's repeats arrive as fresh presses with no
    /// release between them: a second Enter going down before the first
    /// comes up confirms nothing in the confirmation the first opened.
    #[gpui::test]
    fn a_repeat_that_looks_fresh_waits_for_the_enter_that_opened_the_dialog(
        cx: &mut gpui::TestAppContext,
    ) {
        let cx = enter_probe(cx);
        let writes = open_delete(cx, true);
        key_down(cx, "enter", false);
        key_down(cx, "enter", false);
        key_down(cx, "enter", false);
        assert_eq!(
            writes.get(),
            0,
            "no confirm before the opening Enter comes up"
        );
        key_up(cx, "enter");
        key_down(cx, "enter", false);
        assert_eq!(writes.get(), 1, "a fresh press after it confirms");
    }

    /// A dialog opened with no Enter down takes its first Enter, once, as
    /// the key goes down; the held key's repeats confirm nothing more, and
    /// Space on a body Button acts as it comes up, opening nothing early.
    #[gpui::test]
    fn a_dialog_opened_with_no_enter_down_confirms_on_the_first_press(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui_kit::component::WindowExt as _;
        let cx = enter_probe(cx);
        let confirms = std::rc::Rc::new(std::cell::Cell::new(0));
        cx.update(|window, cx| {
            let confirms = confirms.clone();
            window.open_alert_dialog(cx, move |dialog, _, _| {
                let confirms = confirms.clone();
                dialog
                    .title("Opened by a click")
                    .footer(crate::tags::dialog_footer("OK", Some("Cancel")))
                    .on_ok(move |_, _, _| {
                        confirms.set(confirms.get() + 1);
                        false
                    })
            })
        });
        crate::tags::tests::draw(cx);
        key_down(cx, "enter", false);
        assert_eq!(confirms.get(), 1, "the first Enter confirms");
        key_down(cx, "enter", true);
        key_down(cx, "enter", true);
        key_up(cx, "enter");
        assert_eq!(confirms.get(), 1, "a held Enter's repeats confirm nothing");
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::tags::tests::draw(cx);

        let writes = open_delete(cx, true);
        key_down(cx, "space", false);
        key_down(cx, "space", true);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "A stays open"
        );
        key_up(cx, "space");
        assert_eq!(
            writes.get(),
            0,
            "Space opened the confirmation and ran nothing"
        );
    }

    /// In every palette a dialog's edge holds 3:1 against the dialog's
    /// surface and against every page surface its backdrop dims, keeping
    /// `border` where that already does and otherwise taking the least step
    /// toward the text side that does. Braden's `border` stood 1.28:1 off its
    /// dialog, and the popup edge's 1.4:1 left its edge 1.41:1 off the dimmed
    /// page. Every applied palette draws its dialog in that edge.
    #[gpui::test]
    fn every_dialog_edge_holds_against_the_dialog_and_the_dimmed_page(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut failures = Vec::new();
        for (name, palette) in test_palettes() {
            let edge = palette.dialog_edge();
            let alpha = palette.dialog_backdrop();
            let surfaces = [
                ("dialog", palette.canvas),
                ("dimmed canvas", backdrop_over(palette.canvas, alpha)),
                ("dimmed panel", backdrop_over(palette.panel, alpha)),
                ("dimmed subtle", backdrop_over(palette.subtle, alpha)),
            ];
            let least = |color: u32| {
                surfaces
                    .iter()
                    .map(|&(_, surface)| contrast(color, surface))
                    .fold(f64::INFINITY, f64::min)
            };
            for (surface, color) in surfaces {
                let ratio = contrast(edge, color);
                if ratio < DIALOG_EDGE {
                    failures.push(format!(
                        "{name}: #{edge:06x} on the {surface} #{color:06x} {ratio:.3}:1"
                    ));
                }
            }
            if least(palette.border) >= DIALOG_EDGE {
                if edge != palette.border {
                    failures.push(format!("{name}: #{edge:06x} where the border clears"));
                }
            } else {
                // The edge is a step from the border toward the text side,
                // and the step before it, one 256th nearer the border,
                // misses the rule: it moves no further than it must.
                let extreme = if palette.is_light() {
                    0x000000
                } else {
                    0xffffff
                };
                let nth = |n: u32| {
                    [16, 8, 0].into_iter().fold(0, |color, shift| {
                        let from = (palette.border >> shift) & 0xff;
                        let to = (extreme >> shift) & 0xff;
                        color | (((from * (256 - n) + to * n) / 256) << shift)
                    })
                };
                match (1..=256).find(|&n| nth(n) == edge) {
                    Some(n) if least(nth(n - 1)) < DIALOG_EDGE => {}
                    _ => failures.push(format!("{name}: #{edge:06x} is not the least step")),
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        let braden = ThemeChoice::Daylight.palette();
        assert_ne!(braden.dialog_edge(), braden.border, "Braden's edge moves");

        let cx = dialog_probe(cx);
        for (name, palette) in test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            crate::tags::tests::draw(cx);
            assert_eq!(
                painted_dialog_edge(cx),
                vec![Hsla::from(rgb(palette.dialog_edge()))],
                "{name}: the dialog draws its edge"
            );
        }
    }

    /// A dialog's bordered Cancel (`tags::dialog_cancel`) draws the dialog's
    /// edge at rest, hovered and pressed in every palette, 3:1 or more
    /// against the dialog around it. In the page's `border` it stood 1.28:1
    /// off Braden's dialog, and the kit sets a secondary Button's edge from
    /// its theme in each of those states, over any colour its caller gives.
    #[gpui::test]
    fn a_dialogs_cancel_draws_the_dialog_edge_in_every_state(cx: &mut gpui::TestAppContext) {
        use gpui_kit::component::{Root, WindowExt as _};
        use gpui_kit::{
            AppContext as _, InteractiveElement as _, Modifiers, MouseButton, ParentElement as _,
            point,
        };
        cx.update(gpui_kit::init);
        // The dialog otherwise slides in on the wall clock, its Cancel
        // moving between the bounds read and the press.
        cx.update(|cx| cx.set_reduce_motion(true));
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| DialogProbe);
            Root::new(probe, window, cx)
        });
        cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(600.)));
        cx.update(|window, cx| {
            window.open_dialog(cx, |dialog, _, _| {
                dialog.title("Probe").child(
                    crate::tags::dialog_cancel("probe-cancel", "Cancel")
                        .debug_selector(|| "probe-cancel".into()),
                )
            })
        });
        let mut failures = Vec::new();
        for (name, palette) in test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            crate::tags::tests::draw(cx);
            let edge = palette.dialog_edge();
            let ratio = contrast(edge, palette.canvas);
            if ratio < DIALOG_EDGE {
                failures.push(format!("{name}: #{edge:06x} {ratio:.3}:1 on the dialog"));
            }
            let center = cx
                .debug_bounds("probe-cancel")
                .expect("the Cancel")
                .center();
            let painted = |cx: &mut gpui::VisualTestContext| {
                crate::tags::tests::draw(cx);
                crate::views::tests::painted_edges(cx, "probe-cancel")
            };
            let rest = painted(cx);
            cx.simulate_mouse_move(center, None, Modifiers::default());
            let hovered = painted(cx);
            cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::default());
            let pressed = painted(cx);
            cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::default());
            let away = point(gpui::px(-1.), gpui::px(-1.));
            cx.simulate_mouse_move(away, None, Modifiers::default());
            for (state, edges) in [
                ("at rest", rest),
                ("hovered", hovered),
                ("pressed", pressed),
            ] {
                if edges != vec![Hsla::from(rgb(edge))] {
                    failures.push(format!("{name} {state}: {edges:?}, not #{edge:06x}"));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// In every palette, light and dark, a scrollbar's thumb holds 2:1
    /// against each surface its track lies on, the canvas, the panel,
    /// `subtle` and the raised surface the pickers' tracks rest on, as fitted
    /// and as drawn a channel step nearer that surface, keeping `border`
    /// where that already does and otherwise taking the least step toward
    /// the text side that does, and stays quieter than secondary text there.
    /// In `border` it stood 1.52:1 off Midnight's canvas, 1.39:1 off its
    /// panel and 1.28:1 off Braden's canvas; fitted without the drawn step,
    /// Midnight's #444f5f drew #434e5e on Keyboard Shortcuts' raised track,
    /// 1.98:1.
    #[test]
    fn every_scrollbar_thumb_reads_two_to_one_and_quieter_than_secondary_text() {
        let mut failures = Vec::new();
        let mut moved = 0;
        let (mut light, mut dark) = (0, 0);
        for (name, palette) in test_palettes() {
            if palette.is_light() {
                light += 1;
            } else {
                dark += 1;
            }
            let thumb = palette.scrollbar_thumb();
            let surfaces = [
                ("canvas", palette.canvas),
                ("panel", palette.panel),
                ("subtle", palette.subtle),
                ("raised", palette.raised()),
            ];
            // The least of the colour's contrasts, as fitted and as drawn a
            // channel step nearer each surface.
            let least = |color: u32| {
                surfaces
                    .iter()
                    .map(|&(_, surface)| {
                        contrast(color, surface)
                            .min(contrast(drawn_nearer(color, surface), surface))
                    })
                    .fold(f64::INFINITY, f64::min)
            };
            for (surface, color) in surfaces {
                let ratio = contrast(thumb, color);
                let drawn = contrast(drawn_nearer(thumb, color), color);
                let text = contrast(palette.muted, color);
                if ratio < SCROLL_THUMB || drawn < SCROLL_THUMB || ratio >= text {
                    failures.push(format!(
                        "{name}: #{thumb:06x} {ratio:.3}:1 ({drawn:.3}:1 drawn) on the {surface}, \
                         secondary text {text:.3}:1"
                    ));
                }
            }
            if least(palette.border) >= SCROLL_THUMB {
                if thumb != palette.border {
                    failures.push(format!("{name}: #{thumb:06x} where the border clears"));
                }
                continue;
            }
            moved += 1;
            // The thumb is a step from the border toward the text side, and
            // the step before it, one 256th nearer the border, misses.
            let extreme = if palette.is_light() {
                0x000000
            } else {
                0xffffff
            };
            let nth = |n: u32| {
                [16, 8, 0].into_iter().fold(0, |color, shift| {
                    let from = (palette.border >> shift) & 0xff;
                    let to = (extreme >> shift) & 0xff;
                    color | (((from * (256 - n) + to * n) / 256) << shift)
                })
            };
            match (1..=256).find(|&n| nth(n) == thumb) {
                Some(n) if least(nth(n - 1)) < SCROLL_THUMB => {}
                _ => failures.push(format!("{name}: #{thumb:06x} is not the least step")),
            }
        }
        assert!(moved > 0, "some palette's thumb moves off its border");
        assert!(
            light > 0 && dark > 0,
            "light and dark palettes: {light}, {dark}"
        );
        let midnight = test_palettes()
            .into_iter()
            .find(|(name, _)| name == "Midnight")
            .expect("Midnight")
            .1;
        let thumb = midnight.scrollbar_thumb();
        assert!(
            contrast(drawn_nearer(thumb, midnight.raised()), midnight.raised()) >= SCROLL_THUMB,
            "Midnight's #{thumb:06x} holds 2:1 drawn on Keyboard Shortcuts' raised track"
        );
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// An application that has applied no palette fits its editor highlights as applying the
    /// default palette would, with More contrast off and on: fitted with no palette as chosen
    /// under More contrast, the fallback drew no changed-word rule and kept the current match
    /// and the word tints as drawn off (Midnight's current match #284943 where an application
    /// draws #004836).
    #[gpui::test]
    fn unapplied_highlights_fit_as_the_default_palette_applies(cx: &mut gpui::TestAppContext) {
        cx.update(gpui::init);
        for more in [false, true] {
            let (fallback, applied) = cx.update(|cx| {
                if cx.has_global::<EditorHighlights>() {
                    cx.remove_global::<EditorHighlights>();
                    cx.remove_global::<Palette>();
                }
                set_more_contrast(more, cx);
                let fallback = editor_highlights(cx);
                ThemeChoice::default().apply(None, cx);
                (fallback, editor_highlights(cx))
            });
            assert_eq!(fallback, applied, "More contrast {more}");
            assert_eq!(fallback.added_rule.is_some(), more, "More contrast {more}");
        }
    }

    /// Hovering an enabled unchecked Switch visibly lifts its track in every
    /// palette: at least 6 of 255 in one channel from its resting `border`.
    #[test]
    fn every_palette_lifts_a_hovered_switch_track() {
        for (name, palette) in test_palettes() {
            let (rest, hover) = (palette.border, palette.switch_hover());
            let step = [16, 8, 0]
                .map(|shift| ((rest >> shift) & 0xff).abs_diff((hover >> shift) & 0xff))
                .into_iter()
                .max()
                .unwrap();
            assert!(step >= 6, "{name}: #{rest:06x} hovers to #{hover:06x}");
        }
    }

    #[test]
    fn parallel_test_applications_keep_their_own_text_geometry() {
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for (interface, code, desktop) in [(13, 12, 1.0f32), (18, 24, 1.5f32)] {
                let barrier = &barrier;
                scope.spawn(move || {
                    with_text_sizes(|interface_size, code_size, desktop_scale| {
                        interface_size.store(interface, Ordering::Relaxed);
                        code_size.store(code, Ordering::Relaxed);
                        desktop_scale.store(desktop.to_bits(), Ordering::Relaxed);
                    });
                    // Both applications have set their sizes before either
                    // consumes geometry, exposing shared global storage reliably.
                    barrier.wait();
                    assert_eq!(ui_text(13.), px(f32::from(interface) * desktop));
                    assert_eq!(code_text(), px(f32::from(code) * desktop));
                });
            }
        });
    }

    #[test]
    fn desktop_text_scale_values_are_bounded_and_finite() {
        assert_eq!(desktop_text_scale_value(1.0), 1.0);
        assert_eq!(desktop_text_scale_value(1.25), 1.25);
        assert_eq!(desktop_text_scale_value(0.1), 0.5);
        assert_eq!(desktop_text_scale_value(12.0), 3.0);
        assert_eq!(desktop_text_scale_value(0.0), 1.0);
        assert_eq!(desktop_text_scale_value(-1.0), 1.0);
        assert_eq!(desktop_text_scale_value(f64::NAN), 1.0);
        assert_eq!(desktop_text_scale_value(f64::INFINITY), 1.0);
        // This test application begins with the default desktop factor.
        assert_eq!(desktop_text_scale(), 1.0);
    }

    #[test]
    fn palettes_keep_text_and_diff_content_readable_in_each_theme() {
        for choice in ThemeChoice::ALL {
            let issues = choice.palette().readability_issues();
            assert!(
                issues.is_empty(),
                "{choice:?} breaks readability rules: {}",
                issues
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
    }

    /// `#rrggbbaa`, as the kit serializes a color, as `0xrrggbb`.
    fn serialized(color: &serde_json::Value) -> u32 {
        let digits = color.as_str().expect("a serialized color");
        u32::from_str_radix(&digits[1..7], 16).expect("hex digits")
    }

    /// Every color the source and patch editors draw text in under a configured theme, by
    /// syntax field in the toolkit's theme format: each field's color, and the editor
    /// foreground, which a capture without a syntax color and unstyled text take. `None` for a
    /// field without a color.
    fn editor_text_colors(theme: &Theme) -> Vec<(String, Option<u32>)> {
        let syntax = serde_json::to_value(&theme.highlight_theme.style.syntax).unwrap();
        let mut colors = syntax
            .as_object()
            .unwrap()
            .iter()
            .map(|(field, style)| {
                let color = style.get("color").filter(|color| !color.is_null());
                (field.clone(), color.map(serialized))
            })
            .collect::<Vec<_>>();
        colors.push((
            "editor foreground".into(),
            Some(serialized(
                &serde_json::to_value(theme.colors.foreground).unwrap(),
            )),
        ));
        colors
    }

    /// Every syntax color the diff and source editors use, and the editor foreground, reads at
    /// 4.5:1 on each background it is drawn on in each case's configured theme: the editor
    /// background, where hunk header lines draw too, the active line, and the added and removed
    /// line tints. Every field but emphasis has a color, so no capture falls back to the
    /// toolkit's.
    pub(crate) fn assert_syntax_colors_read(
        cases: impl IntoIterator<Item = (String, Palette, bool)>,
    ) {
        let mut below = Vec::new();
        let mut uncolored = Vec::new();
        let (mut palettes, mut measured) = (0, 0);
        for (name, palette, is_light) in cases {
            palettes += 1;
            let theme = palette.configured_theme(is_light);
            let style = &theme.highlight_theme.style;
            let background = |color: Option<Hsla>| {
                serialized(&serde_json::to_value(color.expect("a configured color")).unwrap())
            };
            let backgrounds = [
                ("editor background", background(style.editor_background)),
                ("active line", background(style.editor_active_line)),
                ("added lines", palette.added_background),
                ("removed lines", palette.removed_background),
            ];
            for (token, color) in editor_text_colors(&theme) {
                // Emphasis keeps its italic or weight and takes the color around it.
                let Some(color) = color else {
                    if !token.starts_with("emphasis") {
                        uncolored.push(format!("{name}: {token}"));
                    }
                    continue;
                };
                for (surface, background) in backgrounds {
                    measured += 1;
                    let ratio = contrast(color, background);
                    if ratio < 4.5 {
                        below.push(format!(
                            "{name}: {token} #{color:06x} on {surface} #{background:06x} {ratio:.2}:1"
                        ));
                    }
                }
            }
        }
        assert!(
            below.is_empty(),
            "{} of {measured} pairs below 4.5:1:\n{}",
            below.len(),
            below.join("\n")
        );
        assert_eq!(uncolored, Vec::<String>::new());
        // Forty-four colored fields and the editor foreground, on four backgrounds.
        assert_eq!(measured, palettes * 45 * 4);
    }

    /// Text, every syntax color and the unified patch's diff colors read at 4.5:1 on each
    /// highlight the editors draw behind them in each case's configured theme, and each
    /// highlight stands [`HIGHLIGHT_DISTANCE`] (ΔEOK) from every line background it is drawn on:
    /// the selection (the configured theme's text selection) and Find's other and current
    /// matches on the editor background, the active line and both line tints; each word tint on
    /// its own line tint, under text, the syntax colors but a diff file's removed lines
    /// (`diff.minus`, [`SyntaxRoles::word_colors`]) and its diff color. Returns the cases
    /// whose current Find match stands less than that distance from the other matches (its
    /// accent underline still marks it).
    pub(crate) fn assert_editor_highlights_read(
        cases: impl IntoIterator<Item = (String, Palette, bool)>,
    ) -> Vec<String> {
        let mut below = Vec::new();
        let mut close = Vec::new();
        let mut current_like_others = Vec::new();
        let mut palettes = 0;
        for (name, palette, is_light) in cases {
            palettes += 1;
            let theme = palette.configured_theme(is_light);
            let highlights = palette.editor_highlights();
            let selection = serialized(&serde_json::to_value(theme.colors.selection).unwrap());
            assert_eq!(selection, highlights.selection, "{name}: applied selection");
            // Rows and lists keep the palette's `selected`.
            let list_active = serialized(&serde_json::to_value(theme.colors.list_active).unwrap());
            assert_eq!(list_active, palette.selected, "{name}: list selection");
            let syntax = editor_text_colors(&theme)
                .into_iter()
                .filter_map(|(token, color)| color.map(|color| (token, color)))
                .collect::<Vec<_>>();
            let with = |extra: &[(&str, u32)]| {
                let mut drawn = syntax.clone();
                drawn.extend(
                    extra
                        .iter()
                        .map(|&(token, color)| (token.to_owned(), color)),
                );
                drawn
            };
            let word = |extra: &[(&str, u32)]| {
                let mut drawn = with(extra);
                drawn.retain(|(token, _)| token != "diff.minus");
                drawn
            };
            let unified = with(&[
                ("unified added", palette.added),
                ("unified removed", palette.removed),
                ("unified hunk", palette.hunk),
            ]);
            let lines = [
                ("editor background", palette.canvas),
                ("active line", palette.panel),
                ("added lines", palette.added_background),
                ("removed lines", palette.removed_background),
            ];
            if oklab_distance(oklab(highlights.find_current), oklab(highlights.find_match))
                < HIGHLIGHT_DISTANCE
            {
                current_like_others.push(name.clone());
            }
            // Each highlight, its color, the colors drawn on it and the lines it sits on.
            type Case<'a> = (&'a str, u32, Vec<(String, u32)>, Vec<(&'a str, u32)>);
            let cases: [Case; 5] = [
                ("selection", selection, unified.clone(), lines.to_vec()),
                (
                    "Find match",
                    highlights.find_match,
                    unified.clone(),
                    lines.to_vec(),
                ),
                (
                    "current Find match",
                    highlights.find_current,
                    unified,
                    lines.to_vec(),
                ),
                (
                    "added word tint",
                    highlights.added_word,
                    word(&[("unified added", palette.added)]),
                    vec![lines[2]],
                ),
                (
                    "removed word tint",
                    highlights.removed_word,
                    word(&[("unified removed", palette.removed)]),
                    vec![lines[3]],
                ),
            ];
            for (highlight, background, drawn, lines) in cases {
                for (token, color) in drawn {
                    let ratio = contrast(color, background);
                    if ratio < 4.5 {
                        below.push(format!(
                            "{name}: {token} #{color:06x} on {highlight} #{background:06x} {ratio:.2}:1"
                        ));
                    }
                }
                for (surface, line) in lines {
                    let distance = oklab_distance(oklab(background), oklab(line));
                    if distance < HIGHLIGHT_DISTANCE {
                        close.push(format!(
                            "{name}: {highlight} #{background:06x} on {surface} #{line:06x} ΔEOK {distance:.3}"
                        ));
                    }
                }
            }
        }
        assert!(palettes > 0);
        assert!(
            below.is_empty(),
            "{} pairs below 4.5:1:\n{}",
            below.len(),
            below.join("\n")
        );
        assert!(
            close.is_empty(),
            "{} highlights within ΔEOK {HIGHLIGHT_DISTANCE} of their line:\n{}",
            close.len(),
            close.join("\n")
        );
        current_like_others
    }

    /// Every built-in, dark and light, and the faint-status custom theme below keep their syntax
    /// readable on the editor highlights; `omarchy::palette::tests` measures the Omarchy
    /// fixtures and arbitrary themes. On origin/main the selection and Find drew on `selected`,
    /// where Alucard's `boolean` read at 3.83:1, and the word tints were a fixed mix, where
    /// Dracula's `attribute` read at 2.59:1 on its added word tint.
    #[test]
    fn syntax_reads_on_every_editor_highlight() {
        let mut custom = CustomTheme::from_base(1, "Faint statuses", ThemeChoice::Porcelain);
        custom.palette.set(TokenKind::Renamed, 0x8b62c4);
        custom.palette.set(TokenKind::Modified, 0x9a6a1c);
        let like = assert_editor_highlights_read(
            ThemeChoice::ALL
                .into_iter()
                .map(|choice| (format!("{choice:?}"), choice.palette(), choice.is_light()))
                .chain([(custom.name.clone(), custom.palette, custom.is_light())]),
        );
        // Their accent underline marks the current match where its background cannot stand apart.
        assert_eq!(like, ["Sandstone"]);
    }

    /// The syntax colors of the twenty built-ins, and of a custom theme whose keyword and type
    /// colors sit just above the 3:1 status rule it is held to, read on every editor
    /// background; `omarchy::palette::tests` measures the Omarchy fixtures. On origin/main
    /// these were the toolkit's default highlight theme's colors.
    /// A ```diff fence in Markdown source draws its file headers and context lines in `text`
    /// and its hunk,
    /// removed and added lines in the fitted `hunk`, `removed` and `added` roles, in every
    /// built-in and Omarchy palette, each at 4.5:1 or more on the canvas and the panel. On
    /// origin/main a removed line took the keyword color (`renamed`), a hunk line the markup
    /// color (`accent`), the headers keyword and string, and a context line the fence's code
    /// color (`text.literal`, `added`). Markdown without a diff fence,
    /// another language injected, draws exactly as before: no style it takes depends on the
    /// diff fields.
    #[test]
    fn a_diff_fence_in_markdown_draws_the_patch_colours() {
        use gpui_kit::{
            combine_highlights,
            component::{Rope, highlighter::SyntaxHighlighter},
        };
        let fence = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,3 +1,3 @@ intro\n kept line\n-old line\n+new line\n";
        let document = format!("# Notes\n\nThe change:\n\n```diff\n{fence}```\n");
        let without =
            "# Notes\n\nSome `code` and **bold**.\n\n```rust\nfn main() { let s = \"x\"; }\n```\n";
        let styles = |text: &str, theme: &gpui_kit::component::highlighter::HighlightTheme| {
            let mut highlighter = SyntaxHighlighter::new("markdown");
            highlighter.update(None, &Rope::from(text), None);
            combine_highlights(Vec::new(), highlighter.styles(&(0..text.len()), theme))
                .collect::<Vec<_>>()
        };
        for (name, palette) in test_palettes() {
            let theme = palette.configured_theme(palette.is_light()).highlight_theme;
            let roles = palette.syntax_roles(None);
            let drawn = styles(&document, &theme);
            let mut start = document.find("```diff\n").unwrap() + "```diff\n".len();
            for line in fence.split_inclusive('\n') {
                let range = start..start + line.len() - 1;
                start += line.len();
                let (role, expected) = if line.starts_with("---")
                    || line.starts_with("+++")
                    || line.starts_with(' ')
                {
                    ("text", roles.text)
                } else if line.starts_with("@@") {
                    ("hunk", roles.function)
                } else if line.starts_with('-') {
                    ("removed", roles.removed)
                } else {
                    ("added", roles.string)
                };
                let colors: Vec<_> = drawn
                    .iter()
                    .filter(|(segment, _)| segment.start < range.end && segment.end > range.start)
                    .map(|(_, style)| style.color)
                    .collect();
                assert!(
                    !colors.is_empty()
                        && colors
                            .iter()
                            .all(|color| *color == Some(rgb(expected).into())),
                    "{name}: {line:?} draws {colors:?}, not its {role} color #{expected:06x}"
                );
                for surface in [palette.canvas, palette.panel] {
                    let ratio = custom::contrast(expected, surface);
                    assert!(
                        ratio >= 4.5,
                        "{name}: {role} #{expected:06x} reads {ratio:.2}:1 on #{surface:06x}"
                    );
                }
            }
            let mut without_diff = (*theme).clone();
            let syntax = &mut without_diff.style.syntax;
            (
                syntax.diff_header,
                syntax.diff_hunk,
                syntax.diff_minus,
                syntax.diff_plus,
            ) = (None, None, None, None);
            assert_eq!(
                styles(without, &theme),
                styles(without, &without_diff),
                "{name}: Markdown without a diff fence takes a diff style"
            );
        }
    }

    /// A complete highlight, `update` without a timeout, keeps every layer it injects however
    /// long one takes to parse. The toolkit stopped each injected parse after 20 ms of wall
    /// time even then and dropped that layer, so Markdown's inline layer (code spans, emphasis,
    /// links) went missing from a long paragraph, and from a short one whenever a loaded
    /// machine paused the thread mid-parse: `a_diff_fence_in_markdown_draws_the_patch_colours`
    /// once compared a pass without the inline layer with one that had it. This paragraph,
    /// within the toolkit's 512 KiB bound on one injected language, parses for far longer.
    #[test]
    fn a_complete_markdown_highlight_keeps_inline_styles_however_long_they_parse() {
        use gpui_kit::{
            FontWeight,
            component::{Rope, highlighter::SyntaxHighlighter},
        };
        let document = format!(
            "{}The last **word**.\n",
            "Some `code` and **bold** text.\n".repeat(16_000)
        );
        let mut highlighter = SyntaxHighlighter::new("markdown");
        assert!(highlighter.update(None, &Rope::from(document.as_str()), None));
        let palette = ThemeChoice::Midnight.palette();
        let theme = palette.configured_theme(palette.is_light()).highlight_theme;
        let start = document.rfind("**word**").unwrap();
        let styles = highlighter.styles(&(start..start + "**word**".len()), theme.as_ref());
        assert!(
            styles
                .iter()
                .all(|(_, style)| style.font_weight == Some(FontWeight::BOLD)),
            "the last bold run draws {styles:?}"
        );
    }

    #[test]
    fn syntax_colors_read_on_every_editor_background() {
        let mut custom = CustomTheme::from_base(1, "Faint statuses", ThemeChoice::Porcelain);
        custom.palette.set(TokenKind::Renamed, 0x8b62c4);
        custom.palette.set(TokenKind::Modified, 0x9a6a1c);
        assert_eq!(custom.palette.readability_issues(), Vec::new());
        assert_syntax_colors_read(
            ThemeChoice::ALL
                .into_iter()
                .map(|choice| (format!("{choice:?}"), choice.palette(), choice.is_light()))
                .chain([(custom.name.clone(), custom.palette, custom.is_light())]),
        );
    }

    /// Each syntax field draws in its role's palette color. Pinned on Tokyo Night with its
    /// orange for `warning`, which Tokyo Night shares with `modified`: its nine role colors
    /// are then distinct and each already reads on every editor background, so none is mixed
    /// toward `text`.
    #[test]
    fn syntax_fields_take_their_palette_roles() {
        let mut custom = CustomTheme::from_base(1, "Tokyo orange", ThemeChoice::TokyoNight);
        custom.palette.set(TokenKind::Warning, 0xff9e64);
        let palette = custom.palette;
        assert_eq!(palette.readability_issues(), Vec::new());
        let roles: [(Option<u32>, &[&str]); 10] = [
            (Some(palette.renamed), &["keyword", "preproc"]),
            (
                Some(palette.added),
                &[
                    "string",
                    "string.escape",
                    "string.regex",
                    "string.special",
                    "string.special.symbol",
                    "text.literal",
                    "text.code.span",
                    "diff.plus",
                ],
            ),
            (Some(palette.removed), &["diff.minus"]),
            (Some(palette.warning), &["number", "boolean", "constant"]),
            (
                Some(palette.modified),
                &["type", "constructor", "enum", "variant"],
            ),
            (Some(palette.hunk), &["function", "diff.hunk"]),
            (
                Some(palette.accent),
                &[
                    "tag",
                    "tag.doctype",
                    "attribute",
                    "property",
                    "link_text",
                    "link_uri",
                    "label",
                    "title",
                ],
            ),
            (
                Some(palette.muted),
                &["comment", "comment_doc", "hint", "predictive"],
            ),
            (
                Some(palette.text),
                &[
                    "variable",
                    "variable.special",
                    "embedded",
                    "operator",
                    "punctuation",
                    "punctuation.bracket",
                    "punctuation.delimiter",
                    "punctuation.list_marker",
                    "punctuation.special",
                    "primary",
                    "diff.context",
                    "diff.header",
                    "editor foreground",
                ],
            ),
            (None, &["emphasis", "emphasis.strong"]),
        ];
        let mut distinct = roles
            .iter()
            .filter_map(|(color, _)| *color)
            .collect::<Vec<_>>();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 9, "the role colors are distinct");

        let drawn = editor_text_colors(&palette.configured_theme(custom.is_light()));
        let mut wrong = Vec::new();
        for (field, color) in &drawn {
            let role = roles
                .iter()
                .find(|(_, fields)| fields.contains(&field.as_str()))
                .map(|&(role, _)| role);
            if role != Some(*color) {
                wrong.push(format!(
                    "{field}: {}, wanted {}",
                    color.map_or("none".into(), custom::format_hex),
                    role.map_or("a role".into(), |role| role
                        .map_or("none".into(), custom::format_hex)),
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "fields off their roles:\n{}",
            wrong.join("\n")
        );
        // Every pinned field is a field the toolkit has.
        assert_eq!(
            drawn.len(),
            roles.iter().map(|(_, fields)| fields.len()).sum::<usize>()
        );
    }

    /// A custom theme whose `text` misses the text rule on one editor background draws all its
    /// syntax in `text`, the color its readability warning names, rather than fitting each
    /// role to a contrast `text` itself does not reach: Porcelain with a removed-line tint
    /// `text` reads at 4.47:1 on.
    #[test]
    fn syntax_draws_in_text_where_text_misses_the_rule() {
        use custom::{ReadabilityBackground, ReadabilityForeground};
        let mut custom = CustomTheme::from_base(1, "Dim removals", ThemeChoice::Porcelain);
        custom.palette.set(TokenKind::RemovedBackground, 0xa88e98);
        let palette = custom.palette;
        let ratio = contrast(palette.text, palette.removed_background);
        assert!(
            (4.4..4.5).contains(&ratio),
            "text on removed lines {ratio:.3}:1"
        );
        assert!(palette.readability_issues().iter().any(|issue| {
            issue.foreground == ReadabilityForeground::Token(TokenKind::Text)
                && issue.background == ReadabilityBackground::Token(TokenKind::RemovedBackground)
        }));

        let drawn = editor_text_colors(&palette.configured_theme(custom.is_light()));
        let off_text = drawn
            .iter()
            .filter(|(field, color)| {
                let wanted = (!field.starts_with("emphasis")).then_some(palette.text);
                *color != wanted
            })
            .map(|(field, color)| format!("{field}: {:?}", color.map(custom::format_hex)))
            .collect::<Vec<_>>();
        assert!(
            off_text.is_empty(),
            "fields off text:\n{}",
            off_text.join("\n")
        );
        assert_eq!(drawn.len(), 47);
    }

    /// Tuned tokens clear their rule by this much: the rules are computed on declared colors,
    /// and small text at 1x renders about 0.2 lower (DESIGN.md, semantic palette ownership).
    const RASTERIZATION_MARGIN: f64 = 0.25;

    #[test]
    fn warning_messages_stay_readable_on_the_subtle_surface() {
        for choice in ThemeChoice::ALL {
            let palette = choice.palette();
            let ratio = contrast(palette.warning, palette.subtle);
            assert!(ratio >= 4.5, "{choice:?} warning on subtle {ratio:.3}:1");
        }
        // The five built-ins that were below 4.5:1 before the rule existed.
        for choice in [
            ThemeChoice::KanagawaLotus,
            ThemeChoice::RosePineDawn,
            ThemeChoice::OneLight,
            ThemeChoice::SolarizedDark,
            ThemeChoice::SolarizedLight,
        ] {
            let palette = choice.palette();
            let ratio = contrast(palette.warning, palette.subtle);
            assert!(
                ratio >= 4.5 + RASTERIZATION_MARGIN,
                "{choice:?} warning on subtle {ratio:.3}:1"
            );
        }
    }

    /// Kanagawa Lotus joined on 2026-11-05: its `selected` stepped darker for
    /// the hover rule, and its secondary text with it, from 4.52:1 to 4.82:1
    /// on the hovered selected row, its text darkened to stay above it.
    #[test]
    fn tuned_secondary_text_keeps_its_margin_on_hovered_selected_rows() {
        for choice in [
            ThemeChoice::SolarizedDark,
            ThemeChoice::SolarizedLight,
            ThemeChoice::OneDark,
            ThemeChoice::OneLight,
            ThemeChoice::RosePine,
            ThemeChoice::RosePineDawn,
            ThemeChoice::Dracula,
            ThemeChoice::KanagawaLotus,
        ] {
            let palette = choice.palette();
            let ratio = contrast(palette.muted, palette.row_hover(true));
            assert!(
                ratio >= 4.5 + RASTERIZATION_MARGIN,
                "{choice:?} muted on the hovered selected row {ratio:.3}:1"
            );
        }
    }

    #[test]
    fn secondary_text_never_reads_above_body_text() {
        for choice in ThemeChoice::ALL {
            let palette = choice.palette();
            for (surface, color) in [
                ("canvas", palette.canvas),
                ("panel", palette.panel),
                ("subtle", palette.subtle),
                ("hover", palette.hover),
                ("selected", palette.selected),
                ("hovered selected row", palette.row_hover(true)),
                ("added tile", palette.added_background),
                ("removed tile", palette.removed_background),
            ] {
                let text = contrast(palette.text, color);
                let muted = contrast(palette.muted, color);
                assert!(
                    muted <= text,
                    "{choice:?} muted {muted:.3}:1 above text {text:.3}:1 on {surface}"
                );
            }
        }
    }

    #[test]
    fn palette_lightness_matches_each_built_in_choice() {
        for choice in ThemeChoice::ALL {
            assert_eq!(choice.is_light(), choice.palette().is_light(), "{choice:?}");
        }
    }

    #[test]
    fn theme_switch_updates_resolved_component_backgrounds_with_foregrounds() {
        // Reuse one theme to cover dark/light and dark/dark switches. Component
        // buttons read token backgrounds but legacy foreground colors, so a
        // palette-only assertion cannot catch a stale white primary button.
        // A custom palette, built from a base with two edited tokens, takes the same path.
        let mut edited = ThemeChoice::Nord.palette();
        edited.set(custom::TokenKind::Accent, 0x1f6f5c);
        edited.set(custom::TokenKind::Removed, 0xff9aa2);
        let custom = custom::CustomTheme {
            id: 1,
            name: "Edited Nord".into(),
            base: ThemeChoice::Nord,
            palette: edited,
        };
        let cases = ThemeChoice::ALL
            .into_iter()
            .map(|choice| (format!("{choice:?}"), choice.palette(), choice.is_light()))
            .chain([(custom.name.clone(), custom.palette, custom.is_light())]);
        let mut theme = Theme::default();
        for (choice, palette, is_light) in cases {
            let roles = palette.syntax_roles(None);
            palette.configure(
                is_light,
                false,
                palette,
                roles,
                EditorHighlights::fit(palette, roles, None),
                &mut theme,
            );
            let foreground: Hsla = rgb(palette.accent_foreground).into();
            assert_eq!(theme.colors.button_primary_foreground, foreground);
            for token in [
                theme.tokens.button_danger,
                theme.tokens.button_danger_hover,
                theme.tokens.button_danger_active,
            ] {
                let foreground = u32::from(theme.colors.button_danger_foreground.to_rgb()) >> 8;
                let background = u32::from(token.color.to_rgb()) >> 8;
                assert!(
                    contrast(foreground, background) >= 4.5,
                    "{choice} destructive action label must remain readable"
                );
                assert_eq!(token.background, Background::from(token.color));
            }
            for (token, expected) in [
                (theme.tokens.button_primary, palette.accent),
                (theme.tokens.button_primary_hover, palette.accent_hover),
                (theme.tokens.button_primary_active, palette.accent_active),
                (theme.tokens.button_secondary, palette.subtle),
                (theme.tokens.button_secondary_hover, palette.hover),
                (theme.tokens.button_secondary_active, palette.selected),
                (theme.tokens.list_active, palette.selected),
                (theme.tokens.popover, palette.raised()),
                (theme.tokens.scrollbar_thumb, palette.scrollbar_thumb()),
            ] {
                let color: Hsla = rgb(expected).into();
                assert_eq!(token.color, color, "{choice} resolved color");
                assert_eq!(
                    token.background,
                    Background::from(color),
                    "{choice} renderable background"
                );
            }
        }
    }

    /// The kit's ghost hover read 1.16:1 the wrong way over a hovered row. The
    /// tightest built-in lift is Rosé Pine's 1.078:1 over its canvas, from a
    /// hover that is only 1.087:1 on its own panel (DESIGN.md records that limit).
    const CONTROL_HOVER_LIFT: f64 = 1.07;
    /// Selected surfaces keep 1.15:1 from panels; the tightest pressed lift is
    /// Kanagawa Wave's 1.128:1 over a hovered row.
    const CONTROL_PRESS_LIFT: f64 = 1.12;
    /// The kit's ghost pressed fill differed from its hover by (1, 2, 2). This
    /// is the pressed-step readability rule's distance: Sandstone and Porcelain
    /// were tuned past it (from 3 and 4 to 8), and Catppuccin Mocha and Nord
    /// hold it exactly over their selected rows.
    const CONTROL_PRESS_DISTANCE: u32 = custom::PRESSED_STEP;

    /// Two Buttons a light palette presses: the kit's secondary (Fetch, Pull,
    /// the branch and profile pickers), whose pressed fill is `selected` over
    /// its hovered `hover`, and the shared helper, whose layers repeat that
    /// step, each on the panel and on `subtle`.
    struct PressTargets;

    impl gpui::Render for PressTargets {
        fn render(
            &mut self,
            _: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui_kit::component::button::{Button, ButtonVariants as _};
            use gpui_kit::{InteractiveElement as _, ParentElement as _, div};

            let p = palette(cx);
            div()
                .flex()
                .flex_col()
                .children(
                    [("panel", p.panel), ("subtle", p.subtle)].map(|(surface, color)| {
                        div().flex().gap_4().p_4().bg(rgb(color)).children(
                            [("secondary", "Fetch"), ("control", "Options")].map(
                                |(kind, label)| {
                                    let button = Button::new(disabled_selector(kind, surface))
                                        .debug_selector(move || format!("{kind}-{surface}"))
                                        .label(label)
                                        .on_click(|_, _, _| {});
                                    if kind == "secondary" {
                                        button.secondary()
                                    } else {
                                        button.with_variant(control_button_variant(false))
                                    }
                                },
                            ),
                        )
                    }),
                )
        }
    }

    /// Braden (`daylight`) and Rosé Pine Dawn press deeper than they hover.
    /// Braden's pressed fill had the brightness of its hover and differed by
    /// hue, 1.02:1 lighter over its selected row, and Dawn's stood exactly the
    /// pressed step, 6 of 255 in one channel, from its hover. Now on each of
    /// the six surfaces the shared button's pressed layer reads at least
    /// 1.02:1 darker than its hover layer and at least 10 from it, four past
    /// the step, and so does the kit's secondary, `selected` over `hover`, as
    /// the scene paints both Buttons hovered and held down. Text and secondary
    /// text keep 4.75:1 on the darker selected rows.
    #[gpui::test]
    fn braden_and_rose_pine_dawn_press_deeper_than_they_hover(cx: &mut gpui::TestAppContext) {
        use gpui_kit::{Modifiers, MouseButton, point};

        const DEEPER: f64 = 1.02;
        const STEP: u32 = custom::PRESSED_STEP + 4;
        // How much darker `pressed` reads than `hovered`: below 1 when lighter.
        let deeper =
            |hovered: u32, pressed: u32| (luminance(hovered) + 0.05) / (luminance(pressed) + 0.05);
        let mut failures = Vec::new();
        for choice in [ThemeChoice::Daylight, ThemeChoice::RosePineDawn] {
            let palette = choice.palette();
            let (hover, pressed) = (
                palette.control_fill(palette.hover),
                palette.control_fill(palette.selected),
            );
            let surfaces = [
                palette.canvas,
                palette.panel,
                palette.subtle,
                palette.hover,
                palette.selected,
                palette.row_hover(true),
            ];
            let mut fills = surfaces
                .map(|surface| (composite(hover, surface), composite(pressed, surface)))
                .to_vec();
            fills.push((palette.hover, palette.selected));
            for (hovered, held) in fills {
                let (depth, step) = (deeper(hovered, held), channel_distance(hovered, held));
                if depth < DEEPER || step < STEP {
                    failures.push(format!(
                        "{choice:?} pressed {held:06x} under hover {hovered:06x}: {depth:.3}:1, {step} apart"
                    ));
                }
            }
            if palette.pressed_step() < STEP {
                failures.push(format!(
                    "{choice:?} pressed step {}",
                    palette.pressed_step()
                ));
            }
            for row in [palette.selected, palette.row_hover(true)] {
                for (what, color) in [("text", palette.text), ("muted", palette.muted)] {
                    let ratio = contrast(color, row);
                    if ratio < LABEL_RULE + LABEL_MARGIN {
                        failures.push(format!("{choice:?} {what} on {row:06x} {ratio:.3}:1"));
                    }
                }
            }
        }

        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _| PressTargets);
        for choice in [ThemeChoice::Daylight, ThemeChoice::RosePineDawn] {
            cx.update(|window, cx| choice.apply(Some(window), cx));
            let palette = choice.palette();
            for (surface, color) in [("panel", palette.panel), ("subtle", palette.subtle)] {
                for kind in ["secondary", "control"] {
                    let selector = disabled_selector(kind, surface);
                    let bounds = cx.debug_bounds(selector).expect("the button");
                    let painted = |cx: &mut gpui_kit::VisualTestContext| {
                        cx.update(|window, cx| window.draw(cx).clear(cx));
                        let (fills, _) = painted_button(cx, bounds);
                        fills.iter().fold(color, |beneath, fill| {
                            composite(fill.as_solid().expect("a solid fill").to_rgb(), beneath)
                        })
                    };
                    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
                    let hovered = painted(cx);
                    cx.simulate_mouse_down(
                        bounds.center(),
                        MouseButton::Left,
                        Modifiers::default(),
                    );
                    let held = painted(cx);
                    cx.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
                    cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
                    let (depth, step) = (deeper(hovered, held), channel_distance(hovered, held));
                    if hovered == color || depth < DEEPER || step < STEP {
                        failures.push(format!(
                            "{choice:?} {kind} on the {surface}: hovered {hovered:06x}, held {held:06x}, {depth:.3}:1, {step} apart"
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The Omarchy fitter's rule (`omarchy::palette`'s `HOVER_STEP`) for the
    /// built-ins too: a secondary control's hover stays distinguishable from its
    /// resting `subtle` fill.
    #[test]
    fn every_built_in_hover_steps_off_subtle() {
        let short: Vec<_> = ThemeChoice::ALL
            .into_iter()
            .map(|choice| (choice, choice.palette()))
            .map(|(choice, p)| (choice, p.hover, p.subtle, contrast(p.hover, p.subtle)))
            .filter(|(.., ratio)| *ratio < 1.08)
            .collect();
        assert!(
            short.is_empty(),
            "hover too close to subtle: {}",
            short
                .iter()
                .map(|(choice, hover, subtle, ratio)| format!(
                    "{choice:?} #{hover:06x} on #{subtle:06x} {ratio:.4}"
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    #[test]
    fn shared_button_fills_lift_every_surface_and_keep_the_label_readable() {
        assert_eq!(ThemeChoice::ALL.len(), 20);
        for choice in ThemeChoice::ALL {
            let palette = choice.palette();
            let (hover, pressed) = (
                palette.control_fill(palette.hover),
                palette.control_fill(palette.selected),
            );
            let label = palette.control_label();
            // Only the two palettes whose `text` falls below the rule over a
            // fill move their label; every other palette keeps `text`.
            assert_eq!(
                label == palette.text,
                !matches!(choice, ThemeChoice::KanagawaLotus | ThemeChoice::OneDark),
                "{choice:?} label {label:06x}, text {:06x}",
                palette.text
            );
            for (layer, state) in [(hover, palette.hover), (pressed, palette.selected)] {
                let over_panel = composite(layer, palette.panel);
                assert!(
                    channel_distance(over_panel, state) <= 1,
                    "{choice:?} fill over panel {over_panel:06x}, not {state:06x}"
                );
            }
            // The filled color, and its contrast with the surface when it moves
            // the way hover moves in this theme (0 otherwise).
            let lift = |layer, beneath| {
                let filled = composite(layer, beneath);
                let lifted = if choice.is_light() {
                    luminance(filled) < luminance(beneath)
                } else {
                    luminance(filled) > luminance(beneath)
                };
                let ratio = if lifted {
                    contrast(filled, beneath)
                } else {
                    0.
                };
                (filled, ratio)
            };
            for (surface, beneath) in [
                ("panel", palette.panel),
                ("subtle", palette.subtle),
                ("canvas", palette.canvas),
                ("hovered row", palette.hover),
                ("selected row", palette.selected),
                ("hovered selected row", palette.row_hover(true)),
            ] {
                let (hovered, ratio) = lift(hover, beneath);
                assert!(
                    ratio >= CONTROL_HOVER_LIFT,
                    "{choice:?} hover {hovered:06x} over the {surface} {beneath:06x}: {ratio:.4}"
                );
                let (held, ratio) = lift(pressed, beneath);
                assert!(
                    ratio >= CONTROL_PRESS_LIFT,
                    "{choice:?} pressed {held:06x} over the {surface} {beneath:06x}: {ratio:.4}"
                );
                for (state, filled) in [("hover", hovered), ("pressed", held)] {
                    // A moved label clears the rule with the margin as well.
                    let ratio = contrast(label, filled);
                    let margin = if label == palette.text {
                        0.
                    } else {
                        RASTERIZATION_MARGIN
                    };
                    assert!(
                        ratio >= 4.5 + margin,
                        "{choice:?} label {label:06x} on the {state} fill {filled:06x} over \
                         the {surface}: {ratio:.3}"
                    );
                }
                let distance = channel_distance(held, hovered);
                assert!(
                    distance >= CONTROL_PRESS_DISTANCE,
                    "{choice:?} pressed {held:06x} is {distance} from hover {hovered:06x} \
                     on the {surface}"
                );
            }
        }
    }

    #[test]
    fn selected_shared_button_hover_stays_a_pressed_step_from_rest() {
        for choice in ThemeChoice::ALL {
            let palette = choice.palette();
            // Both fills are opaque, so the step is the same on every surface.
            let (resting, hovered) = (palette.selected, palette.row_hover(true));
            let distance = channel_distance(hovered, resting);
            assert!(
                distance >= CONTROL_PRESS_DISTANCE,
                "{choice:?} selected hover {hovered:06x} is {distance} from {resting:06x}"
            );
            let label = palette.control_label();
            let margin = if label == palette.text {
                0.
            } else {
                RASTERIZATION_MARGIN
            };
            let ratio = contrast(label, hovered);
            assert!(
                ratio >= 4.5 + margin,
                "{choice:?} label {label:06x} on the selected hover {hovered:06x}: {ratio:.3}"
            );
        }
    }

    /// The ring a focused Button draws, as the palette application installs it,
    /// against the same pixel unfocused. Ring and gap lie outside the edge, on
    /// the surface beneath, so the Button's fill never reaches them; only an
    /// opacity a state sets on the whole Button dims the ring with it.
    #[gpui::test]
    fn focused_button_ring_clears_the_graphic_rule_on_every_surface_and_state(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let mut failures = Vec::new();
            for choice in ThemeChoice::ALL {
                choice.apply(None, cx);
                let palette = choice.palette();
                let theme = Theme::global(cx);
                let (setting, ring) = (theme.button_focus_ring, theme.colors.ring.to_rgb());
                if setting.width < px(2.) || setting.gap < px(1.) {
                    failures.push(format!(
                        "{choice:?} ring {:?} wide, {:?} outside the edge",
                        setting.width, setting.gap
                    ));
                }
                // The kit fades no state the helper paints (only a loading
                // Button), so the helper's selected hover is the one to read.
                let selected_hover = control_selected_hover(StyleRefinement::default())
                    .opacity
                    .unwrap_or(1.);
                let states = [
                    ("at rest", 1.),
                    ("selected", 1.),
                    ("selected and hovered", selected_hover),
                    ("hovered", 1.),
                    ("pressed", 1.),
                    ("disabled", 1.),
                ];
                for (surface, beneath) in [
                    ("panel", palette.panel),
                    ("subtle", palette.subtle),
                    ("canvas", palette.canvas),
                    ("hovered row", palette.hover),
                    ("selected row", palette.selected),
                    ("hovered selected row", palette.row_hover(true)),
                ] {
                    let below = states
                        .iter()
                        .filter_map(|&(state, opacity)| {
                            let layer = Rgba {
                                a: ring.a * setting.opacity * opacity,
                                ..ring
                            };
                            let ratio = contrast(composite(layer, beneath), beneath);
                            (ratio < custom::GRAPHIC).then(|| format!("{state} {ratio:.2}:1"))
                        })
                        .collect::<Vec<_>>();
                    if !below.is_empty() {
                        failures.push(format!(
                            "{choice:?} on the {surface} {beneath:06x}: {}",
                            below.join(", ")
                        ));
                    }
                }
            }
            assert!(
                failures.is_empty(),
                "focused Button ring below {}:1 or not 2 px wide, 1 px outside the edge\n{}",
                custom::GRAPHIC,
                failures.join("\n")
            );
        });
    }

    /// Every focus ring a kit control draws, a text field's, a checkbox's and
    /// a Switch's, read from the frame with the control focused: each quad in
    /// the theme's ring colour around it, at the opacity it is painted with,
    /// over each surface such a control rests on (the canvas a dialog shows,
    /// the panel and `subtle`), reads 3:1 (WCAG 1.4.11), in every built-in
    /// palette with More contrast off and on, with the ring the palette's
    /// accent. All three draw the Button's ring, 2 px of the full accent 1 px
    /// outside their edge, and each must paint a quad of the ring's width:
    /// the field's own 1 px accent edge, in the same colour, is not its ring.
    /// The field and the checkbox drew the kit's, 3 px at half opacity
    /// directly outside it, which read under 3:1 in 16 of the 20 palettes:
    /// 2.27:1 on Porcelain's canvas, 2.06:1 on Sandstone's `subtle`.
    #[gpui::test]
    fn every_palette_draws_kit_focus_rings_at_three_to_one(cx: &mut gpui::TestAppContext) {
        use gpui_kit::component::checkbox::Checkbox;
        use gpui_kit::component::input::{Input, InputState};
        use gpui_kit::component::switch::Switch;
        use gpui_kit::{
            AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _,
            InteractiveElement as _, IntoElement, ParentElement as _, Render, ScaledPixels,
            Styled as _, div, point,
        };

        struct Controls {
            field: Entity<InputState>,
            checkbox: FocusHandle,
            switch: FocusHandle,
        }
        impl Render for Controls {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .flex()
                    .items_start()
                    .gap_8()
                    .p_8()
                    .child(
                        div()
                            .debug_selector(|| "ring-field".into())
                            .w(px(160.))
                            .child(Input::new(&self.field)),
                    )
                    .child(
                        div().debug_selector(|| "ring-checkbox".into()).child(
                            Checkbox::new("ring-checkbox")
                                .track_focus(&self.checkbox)
                                .label("Include"),
                        ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "ring-switch".into())
                            .child(Switch::new("ring-switch").track_focus(&self.switch)),
                    )
            }
        }

        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|window, cx| Controls {
            field: cx.new(|cx| InputState::new(window, cx)),
            checkbox: cx.focus_handle(),
            switch: cx.focus_handle(),
        });
        let handles = cx.update(|_, cx| {
            let view = view.read(cx);
            [
                ("ring-field", view.field.read(cx).focus_handle(cx)),
                ("ring-checkbox", view.checkbox.clone()),
                ("ring-switch", view.switch.clone()),
            ]
        });
        let mut failures = Vec::new();
        for more in [false, true] {
            for choice in ThemeChoice::ALL {
                cx.update(|window, cx| {
                    set_more_contrast(more, cx);
                    choice.apply(Some(window), cx);
                });
                for (id, handle) in &handles {
                    cx.update(|window, cx| window.focus(handle, cx));
                    crate::tags::tests::draw(cx);
                    let control = cx.debug_bounds(id).expect("the control");
                    let (applied, ring, width, mut rings) = cx.update(|window, cx| {
                        let scale = window.scale_factor();
                        let logical = |value: ScaledPixels| px(value.as_f32() / scale);
                        let theme = Theme::global(cx);
                        let (ring, width) = (packed(theme.ring), theme.button_focus_ring.width);
                        let rings = window
                            .painted_quads()
                            .into_iter()
                            .filter(|quad| quad.border_widths.top.as_f32() > 0.)
                            .filter(|quad| {
                                packed(quad.border_color) == ring && quad.border_color.a > 0.
                            })
                            .filter(|quad| {
                                let drawn = Bounds::from_corners(
                                    point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                                    point(
                                        logical(quad.bounds.right()),
                                        logical(quad.bounds.bottom()),
                                    ),
                                );
                                control.dilate(px(8.)).contains(&drawn.origin)
                            })
                            .map(|quad| {
                                (quad.border_color.to_rgb(), logical(quad.border_widths.top))
                            })
                            .collect::<Vec<_>>();
                        (palette(cx), ring, width, rings)
                    });
                    // GPUI paints a border-only quad once per side.
                    rings.dedup();
                    if ring != applied.accent {
                        failures.push(format!(
                            "{choice:?} more {more}: ring {ring:06x}, not the accent {:06x}",
                            applied.accent
                        ));
                    }
                    // The field's own 1 px edge takes the accent too, so a
                    // quad in the ring colour alone does not show its ring.
                    if !rings
                        .iter()
                        .any(|(_, drawn)| (*drawn - width).abs() < px(0.01))
                    {
                        failures.push(format!(
                            "{choice:?} more {more}: {id} draws no {width:?} ring, only {rings:?}"
                        ));
                    }
                    for (layer, _) in rings {
                        for (surface, beneath) in [
                            ("canvas", applied.canvas),
                            ("panel", applied.panel),
                            ("subtle", applied.subtle),
                        ] {
                            let ratio = contrast(composite(layer, beneath), beneath);
                            if ratio < custom::GRAPHIC {
                                failures.push(format!(
                                    "{choice:?} more {more}: {id} at {:.2} on the {surface} \
                                     {beneath:06x}: {ratio:.2}:1",
                                    layer.a
                                ));
                            }
                        }
                    }
                }
            }
        }
        cx.update(|_, cx| set_more_contrast(false, cx));
        failures.dedup();
        assert!(
            failures.is_empty(),
            "kit focus rings below {}:1\n{}",
            custom::GRAPHIC,
            failures.join("\n")
        );
    }

    #[gpui::test]
    fn palette_application_hands_the_shared_button_its_fills_and_label(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            for choice in ThemeChoice::ALL {
                choice.apply(None, cx);
                let palette = choice.palette();
                let label: Hsla = rgb(palette.control_label()).into();
                let expected = ButtonCustomVariant::new(cx)
                    .foreground(label)
                    .hover(palette.control_fill(palette.hover).into())
                    .active(palette.control_fill(palette.selected).into());
                assert_eq!(
                    control_button_variant(false),
                    ButtonVariant::Custom(expected),
                    "{choice:?}"
                );
                // A selected helper reads the same label on the `selected`
                // surface, through the kit's secondary where the label is `text`,
                // and disabled rests on the secondary's `subtle`.
                match control_button_variant(true) {
                    ButtonVariant::Secondary => {
                        let theme = Theme::global(cx);
                        assert_eq!(
                            theme.colors.button_secondary_foreground, label,
                            "{choice:?}"
                        );
                        assert_eq!(
                            theme.tokens.button_secondary_active.color,
                            rgb(palette.selected).into(),
                            "{choice:?}"
                        );
                        assert_eq!(
                            theme.colors.button_secondary,
                            rgb(palette.subtle).into(),
                            "{choice:?}"
                        );
                    }
                    ButtonVariant::Custom(variant) => {
                        assert!(
                            matches!(choice, ThemeChoice::KanagawaLotus | ThemeChoice::OneDark),
                            "{choice:?} leaves the kit's secondary"
                        );
                        let expected = ButtonCustomVariant::new(cx)
                            .foreground(label)
                            .active(rgb(palette.selected).into())
                            .disabled(rgb(palette.subtle).into());
                        assert_eq!(variant, expected, "{choice:?}");
                    }
                    variant => panic!("{choice:?} selects with {variant:?}"),
                }
                // Every focused Button draws 2 px of full `ring`, this `accent`,
                // 1 px outside its edge.
                let theme = Theme::global(cx);
                assert_eq!(
                    theme.button_focus_ring,
                    FocusRing {
                        width: px(2.),
                        gap: px(1.),
                        opacity: 1.,
                    },
                    "{choice:?}"
                );
                assert_eq!(theme.colors.ring, rgb(palette.accent).into());
                // A hovered selected helper tints its fill and leaves the
                // whole Button, ring included, at full opacity.
                let hovered = control_selected_hover(StyleRefinement::default());
                assert_eq!(
                    hovered.background,
                    Some(rgb(palette.row_hover(true)).into()),
                    "{choice:?}"
                );
                assert_eq!(hovered.opacity, None, "{choice:?}");
            }
        });
    }

    #[test]
    fn appearance_choices_round_trip_and_unknown_choices_have_safe_defaults() {
        for choice in ThemeChoice::ALL {
            let encoded = serde_json::to_string(&choice).unwrap();
            assert_eq!(
                serde_json::from_str::<ThemeChoice>(&encoded).unwrap(),
                choice
            );
        }
        assert_eq!(
            serde_json::from_str::<ThemeChoice>("\"future-theme\"").unwrap(),
            ThemeChoice::Midnight
        );
        assert_eq!(
            serde_json::from_str::<Density>("\"future-density\"").unwrap(),
            Density::Comfortable
        );
        assert!(Density::Compact.history_row_height() < Density::Comfortable.history_row_height());
        assert!(Density::Compact.file_row_height() < Density::Comfortable.file_row_height());
    }

    #[test]
    fn adapted_family_themes_keep_their_storage_names_labels_and_lightness() {
        assert_eq!(ThemeChoice::ALL.len(), 20);
        for (choice, stored, label, light) in [
            (
                ThemeChoice::SolarizedDark,
                "solarized_dark",
                "Solarized Dark",
                false,
            ),
            (
                ThemeChoice::SolarizedLight,
                "solarized_light",
                "Solarized Light",
                true,
            ),
            (ThemeChoice::OneDark, "one_dark", "One Dark", false),
            (ThemeChoice::OneLight, "one_light", "One Light", true),
            (ThemeChoice::RosePine, "rose_pine", "Rosé Pine", false),
            (
                ThemeChoice::RosePineDawn,
                "rose_pine_dawn",
                "Rosé Pine Dawn",
                true,
            ),
            (ThemeChoice::Dracula, "dracula", "Dracula", false),
            (ThemeChoice::Alucard, "alucard", "Alucard", true),
            (
                ThemeChoice::KanagawaWave,
                "kanagawa_wave",
                "Kanagawa Wave",
                false,
            ),
            (
                ThemeChoice::KanagawaLotus,
                "kanagawa_lotus",
                "Kanagawa Lotus",
                true,
            ),
        ] {
            assert!(ThemeChoice::ALL.contains(&choice));
            assert_eq!(serde_json::to_value(choice).unwrap(), stored);
            assert_eq!(
                serde_json::from_value::<ThemeChoice>(stored.into()).unwrap(),
                choice
            );
            assert_eq!(choice.label(), label);
            assert_eq!(choice.is_light(), light);
            // Two words around a middle dot, like "Deep slate · mint".
            let (surface, accent) = choice.description().split_once(" · ").unwrap();
            assert_eq!(surface.split(' ').count(), 2, "{choice:?}");
            assert_eq!(accent.split(' ').count(), 1, "{choice:?}");
        }
    }

    #[test]
    fn saved_daylight_remains_braden_without_changing_its_storage_or_light_mapping() {
        let saved =
            r#"{"theme":"daylight","follow_system":false,"density":"compact","code_text_size":19}"#;
        let mut settings: crate::preferences::AppSettings = serde_json::from_str(saved).unwrap();
        assert_eq!(
            settings.theme,
            custom::ThemeSelection::BuiltIn(ThemeChoice::Daylight)
        );
        assert_eq!(ThemeChoice::Daylight.label(), "Braden");
        assert_eq!(
            serde_json::to_value(&settings).unwrap()["theme"],
            "daylight"
        );
        assert_eq!(settings.density, Density::Compact);
        assert_eq!(settings.code_text_size, 19);
        let resolved = |settings: &crate::preferences::AppSettings, appearance| {
            settings.resolved_theme(appearance, &[]).selection
        };
        let built_in = custom::ThemeSelection::BuiltIn;
        assert_eq!(
            resolved(&settings, gpui_kit::WindowAppearance::Dark),
            built_in(ThemeChoice::Daylight)
        );
        settings.follow_system = true;
        for light in [
            ThemeChoice::Daylight,
            ThemeChoice::Porcelain,
            ThemeChoice::Sandstone,
            ThemeChoice::SolarizedLight,
            ThemeChoice::OneLight,
            ThemeChoice::RosePineDawn,
            ThemeChoice::Alucard,
            ThemeChoice::KanagawaLotus,
        ] {
            settings.theme = built_in(light);
            assert_eq!(
                resolved(&settings, gpui_kit::WindowAppearance::Light),
                built_in(ThemeChoice::Daylight)
            );
            assert_eq!(
                resolved(&settings, gpui_kit::WindowAppearance::Dark),
                built_in(ThemeChoice::Midnight)
            );
        }
    }

    /// The font that an element under [`CodeFont`] hands its descendants, and
    /// so the font that the kit's editor and the diff gutter shape with
    /// (`window.text_style().font()`), while the code family is `family`.
    fn code_text_font(cx: &mut gpui::TestAppContext, family: &'static str) -> gpui_kit::Font {
        use gpui_kit::{Context, Font, IntoElement, ParentElement, Render, canvas, div};
        use std::{cell::RefCell, rc::Rc};

        struct Probe(Rc<RefCell<Option<Font>>>);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let seen = self.0.clone();
                div().code_font(cx).child(
                    canvas(
                        move |_, window, _| *seen.borrow_mut() = Some(window.text_style().font()),
                        |_, _, _, _| {},
                    )
                    .w(px(1.))
                    .h(px(1.)),
                )
            }
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            Theme::global_mut(cx).mono_font_family = family.into();
        });
        let seen = Rc::new(RefCell::new(None));
        let probe = Probe(seen.clone());
        let (_, cx) = cx.add_window_view(move |_, _| probe);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        seen.borrow().clone().expect("the probe painted")
    }

    /// Code text draws what was typed: a desktop family shapes with its
    /// ligatures off.
    #[gpui::test]
    fn code_text_shapes_the_code_family_without_ligatures(cx: &mut gpui::TestAppContext) {
        let font = code_text_font(cx, "Desktop Mono");
        assert_eq!(font.family, "Desktop Mono");
        assert_eq!(font.features.is_calt_enabled(), Some(false));
        assert!(font.features.tag_value_list().contains(&("liga".into(), 0)));
    }

    /// The bundled family has no ligatures to turn off, so its code text
    /// shapes with no explicit feature, which is faster.
    #[gpui::test]
    fn code_text_in_the_bundled_family_shapes_without_features(cx: &mut gpui::TestAppContext) {
        let bundled = crate::desktop_text::BUNDLED_CODE_FAMILY;
        let font = code_text_font(cx, bundled);
        assert_eq!(font.family, bundled);
        assert!(font.features.tag_value_list().is_empty(), "{font:?}");
    }

    /// A checked Switch's knob reads on its accent track in every palette:
    /// the thumb takes the accent's label colour (`accent_foreground`, the
    /// primary Button's) at 3:1 or more, where `text` drew it at 1.0:1 in
    /// Omarchy Kanagawa, whose accent is its text colour. An unchecked thumb
    /// stays `text` on the `border` track, and a disabled checked Switch fades
    /// its accent label with the track. Built-ins and every embedded Omarchy
    /// fixture.
    #[gpui::test]
    fn every_palette_paints_a_checked_switch_knob_that_reads_on_its_track(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui_kit::component::Disableable as _;
        use gpui_kit::component::switch::{self, Switch};
        use gpui_kit::{
            Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
            ScaledPixels, Styled as _, div, point,
        };

        const SWITCHES: [(&str, bool, bool); 3] = [
            ("switch-on", true, false),
            ("switch-off", false, false),
            ("switch-on-disabled", true, true),
        ];
        struct Switches;
        impl Render for Switches {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .flex()
                    .gap_4()
                    .p_4()
                    .children(SWITCHES.map(|(id, checked, disabled)| {
                        div()
                            .debug_selector(move || id.into())
                            .child(Switch::new(id).checked(checked).disabled(disabled))
                    }))
            }
        }
        // The fills inside the switch at `id`: its track, then its thumb.
        let painted = |cx: &mut gpui_kit::VisualTestContext, id: &'static str| {
            let switch = cx.debug_bounds(id).expect("the switch");
            cx.update(|window, _| {
                let scale = window.scale_factor();
                let logical = |value: ScaledPixels| px(value.as_f32() / scale);
                let fills = window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| !quad.background.is_transparent())
                    .filter(|quad| {
                        let drawn = Bounds::from_corners(
                            point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                            point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                        );
                        switch.dilate(px(1.)).contains(&drawn.origin)
                            && drawn.right() <= switch.right() + px(1.)
                    })
                    .map(|quad| quad.background)
                    .collect::<Vec<_>>();
                let [track, thumb] = fills[..] else {
                    panic!("{id} paints a track and a thumb: {fills:?}");
                };
                (track, thumb)
            })
        };
        let fill = |color: u32, faded: bool| {
            let fill = Background::from(Hsla::from(rgb(color)));
            if faded { fill.opacity(0.5) } else { fill }
        };

        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _| Switches);
        let mut failures = Vec::new();
        for (name, palette) in test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(
                cx.update(|_, cx| switch::thumb_color(true, cx)),
                Hsla::from(rgb(palette.accent_foreground)),
                "{name}"
            );
            for (id, checked, disabled) in SWITCHES {
                let track = if checked {
                    palette.accent
                } else {
                    palette.border
                };
                let thumb = if checked {
                    palette.accent_foreground
                } else {
                    palette.text
                };
                assert_eq!(
                    painted(cx, id),
                    (fill(track, disabled), fill(thumb, disabled)),
                    "{name}: {id}"
                );
            }
            let knob = contrast(palette.accent_foreground, palette.accent);
            if knob < custom::GRAPHIC {
                failures.push(format!("{name} {knob:.2}:1"));
            }
        }
        assert!(
            failures.is_empty(),
            "checked knob below {}:1 on its track: {}",
            custom::GRAPHIC,
            failures.join(", ")
        );
    }

    /// Every pair of a small label and a surface it sits on that falls below
    /// the text rule with its rasterization margin, as `name label on surface
    /// ratio`.
    fn small_label_failures(
        name: &str,
        what: &str,
        label: u32,
        surfaces: &[u32],
    ) -> Option<String> {
        let low = surfaces
            .iter()
            .map(|&surface| (surface, contrast(label, surface)))
            .filter(|&(_, ratio)| ratio < LABEL_RULE + LABEL_MARGIN)
            .map(|(surface, ratio)| format!("{surface:06x} {ratio:.2}:1"))
            .collect::<Vec<_>>();
        (!low.is_empty()).then(|| format!("{name} {what} {label:06x} on {}", low.join(", ")))
    }

    /// History's reference badge draws its label in its graph lane's color on a
    /// tint of that color, and the lanes are fitted only as graphics, to 3:1:
    /// on the selected row Omarchy Catppuccin Latte's first lane read 2.95:1.
    /// Every palette's badge labels clear the text rule with the margin on
    /// their tint over each History row surface, and a lane that already does
    /// keeps its own color. The applied palette installs them.
    #[gpui::test]
    fn every_palette_fits_history_reference_labels_on_their_tint(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut failures = Vec::new();
        let mut lanes_below = 0;
        for (name, palette) in test_palettes() {
            let labels = cx.update(|cx| {
                palette.apply(palette.is_light(), None, cx);
                super::labels(cx)
            });
            assert_eq!(labels, Labels::fit(palette), "{name} installs its labels");
            let rows = [
                palette.canvas,
                palette.hover,
                palette.selected,
                palette.row_hover(true),
            ];
            for (lane, (color, label)) in crate::graph::palette_colors(palette)
                .into_iter()
                .zip(labels.refs)
                .enumerate()
            {
                let tint = Rgba {
                    a: REF_TINT_ALPHA as f32 / 255.,
                    ..rgb(color)
                };
                let badges = rows.map(|row| composite(tint, row));
                let own = small_label_failures(name.as_str(), "", color, &badges);
                lanes_below += usize::from(own.is_some());
                if own.is_none() {
                    assert_eq!(label, color, "{name} lane {lane} keeps its color");
                }
                failures.extend(small_label_failures(
                    &name,
                    &format!("lane {lane}"),
                    label,
                    &badges,
                ));
            }
        }
        assert!(lanes_below > 0, "some lane needs its label fitted");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A selected sidebar row (repository navigation and the project list)
    /// labels itself in `text` at the text rule on the selected and hovered
    /// selected rows, where `accent`, fitted only to 3:1, fell to 3.0:1 in
    /// Omarchy Solitude; its icon keeps `accent` at 3:1 as a graphic. Unselected
    /// rows read on the panel and its hover.
    #[test]
    fn every_palette_labels_a_selected_sidebar_row_in_readable_text() {
        let mut failures = Vec::new();
        let mut accent_below = false;
        for (name, palette) in test_palettes() {
            for (active, rows) in [
                (true, [palette.selected, palette.row_hover(true)]),
                (false, [palette.panel, palette.hover]),
            ] {
                let (label, icon) = palette.sidebar_row(active);
                for row in rows {
                    let (text, graphic) = (contrast(label, row), contrast(icon, row));
                    if text < LABEL_RULE || graphic < custom::GRAPHIC {
                        failures.push(format!(
                            "{name} active {active} on {row:06x}: label {text:.2}:1, icon {graphic:.2}:1"
                        ));
                    }
                    accent_below |= active && contrast(palette.accent, row) < LABEL_RULE;
                }
            }
        }
        assert!(
            accent_below,
            "accent alone would miss the text rule somewhere"
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The Unstaged badge (`modified` on `subtle`) and the Staged and "N
    /// staged" badges (`added` on its diff tile) are 10 px labels whose tokens
    /// keep only the text rule, and `modified` not even that: 4.1:1 in Omarchy
    /// Rose Pine and 3.9:1 in Kanagawa Lotus. Every palette's badge labels
    /// clear the rule with the margin, and a token that already does is kept.
    #[test]
    fn every_palette_fits_staged_and_unstaged_badge_labels() {
        let mut failures = Vec::new();
        let mut moved = 0;
        for (name, palette) in test_palettes() {
            let labels = Labels::fit(palette);
            for (what, token, label, surface) in [
                (
                    "unstaged",
                    palette.modified,
                    labels.unstaged,
                    palette.subtle,
                ),
                (
                    "staged",
                    palette.added,
                    labels.added,
                    palette.added_background,
                ),
            ] {
                match small_label_failures(&name, what, token, &[surface]) {
                    Some(_) => moved += 1,
                    None => assert_eq!(label, token, "{name} keeps its {what} color"),
                }
                failures.extend(small_label_failures(&name, what, label, &[surface]));
            }
        }
        assert!(moved > 0, "some palette needs a badge label fitted");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The surfaces a disabled control's label rests on, by name: the page's
    /// surfaces and the raised menu surface.
    fn disabled_label_surfaces(palette: Palette) -> [(&'static str, u32); 4] {
        [
            ("canvas", palette.canvas),
            ("panel", palette.panel),
            ("subtle", palette.subtle),
            ("menu", palette.raised()),
        ]
    }

    /// The disabled Buttons [`DisabledControls`] draws, by kind: the kit's
    /// secondary (Fetch and Pull) and primary (Push, Commit), the shared helper
    /// at rest and selected (a dialog's default action), the kit's ghost, and
    /// its default Button with no variant (Settings' Save). A dialog's Cancel
    /// is the bordered secondary (`tags::dialog_cancel`).
    const DISABLED_BUTTONS: [&str; 6] = [
        "secondary",
        "primary",
        "control",
        "control-selected",
        "ghost",
        "default",
    ];

    /// `kind-surface`, the debug selector [`DisabledControls`] gives a Button.
    fn disabled_selector(kind: &str, surface: &str) -> &'static str {
        Box::leak(format!("{kind}-{surface}").into_boxed_str())
    }

    /// Each kind of disabled Button and a disabled Settings switch row, in a
    /// row on each of [`disabled_label_surfaces`] of the applied palette.
    struct DisabledControls;

    impl gpui::Render for DisabledControls {
        fn render(
            &mut self,
            _: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui_kit::component::button::{Button, ButtonVariants as _};
            use gpui_kit::component::{Disableable as _, Selectable as _};
            use gpui_kit::{InteractiveElement as _, ParentElement as _, div};

            let surfaces = disabled_label_surfaces(palette(cx));
            div()
                .flex()
                .flex_col()
                .children(surfaces.map(|(surface, color)| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .bg(rgb(color))
                        .children(DISABLED_BUTTONS.map(|kind| {
                            let button = Button::new(disabled_selector(kind, surface))
                                .debug_selector(move || format!("{kind}-{surface}"))
                                .label("Push");
                            match kind {
                                "secondary" => button.secondary(),
                                "primary" => button.primary(),
                                "control" => button.with_variant(control_button_variant(false)),
                                "control-selected" => button
                                    .with_variant(control_button_variant(true))
                                    .selected(true),
                                "ghost" => button.ghost(),
                                _ => button,
                            }
                            .disabled(true)
                        }))
                        // Follow system appearance's row as Settings draws
                        // it, its title the switch's label.
                        .child(
                            crate::settings::switch_row(
                                disabled_selector("switch", surface),
                                crate::settings::FOLLOW_SYSTEM,
                                crate::settings::FOLLOW_SYSTEM,
                                "Omarchy follows your desktop theme",
                                &[],
                                cx,
                            )
                            .disabled(true),
                        )
                }))
        }
    }

    /// Disabled Buttons, Switch labels, menu items with their shortcuts and
    /// command palette rows share one dim, `Palette::disabled_label`, which the
    /// kit's `disabled_foreground` returns. There were two, and in light
    /// palettes both fell near 2:1: Buttons and menus drew `muted` at half
    /// strength (Omarchy Catppuccin Latte's disabled Push 2.06:1, Flexoki
    /// Light's toolbar 2.12:1), and Switch labels `text` at half (the Follow
    /// system label 2.35:1 in Latte). In every palette the dim reads 3:1 on
    /// every surface a disabled label rests on, the page's and the menu's, and
    /// on the fill each disabled Button paints there, the kit's default
    /// Button's translucent one included: that one was left out of the fit, so
    /// Midnight's disabled Save on Settings' card read 3.002:1, under 3
    /// natively, and Nord's and Solarized Dark's below 3:1. It keeps the margin
    /// 1x rendering takes above that, unless the first step that does would
    /// break the 60% bound below (Kanagawa Lotus's highlighted row), and
    /// Midnight keeps it. Where it had to move off half strength it reads no
    /// more than 0.15 above its floor on its closest surface, about 3:1. It keeps
    /// at most 60% of enabled secondary text's contrast on each of those and
    /// on the selected row (a highlighted disabled command), so disabled stays
    /// clearly fainter than enabled text.
    #[gpui::test]
    fn every_palette_shares_one_disabled_dim_that_reads_three_to_one(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui_kit::component::switch;

        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _| DisabledControls);
        let mut failures = Vec::new();
        let (mut moved, mut kept) = (0, 0);
        for (name, palette) in test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let (dim, switch_label) = cx.update(|_, cx| {
                (
                    Theme::global(cx).disabled_foreground(),
                    switch::disabled_label_color(cx),
                )
            });
            assert_eq!(dim, Hsla::from(palette.disabled_label()), "{name} dim");
            assert_eq!(switch_label, dim, "{name} Switch label");
            // A disabled Settings switch's title, its label, draws in it.
            assert_eq!(
                drawn_color(&format!(
                    "setting-title: {}",
                    crate::settings::FOLLOW_SYSTEM
                )),
                Some(dim),
                "{name} disabled switch's title"
            );
            let dim = dim.to_rgb();
            // The dim's contrast on `beneath`, recording a failure where it
            // keeps more than 60% of enabled secondary text's.
            let check = |failures: &mut Vec<String>, what: &str, beneath: u32| {
                let disabled = contrast(composite(dim, beneath), beneath);
                let enabled = contrast(palette.muted, beneath);
                if disabled > enabled * DISABLED_SHARE {
                    failures.push(format!(
                        "{name} {what} {beneath:06x}: disabled {disabled:.2}:1, secondary text {enabled:.2}:1"
                    ));
                }
                disabled
            };
            // A Switch's label and a menu item's lie on the surface itself,
            // a disabled Button's on the fills it paints there.
            let mut readings = Vec::new();
            for (surface, color) in disabled_label_surfaces(palette) {
                readings.push((format!("label on the {surface}"), color));
                for kind in DISABLED_BUTTONS {
                    let bounds = cx
                        .debug_bounds(disabled_selector(kind, surface))
                        .expect("the button");
                    let (fills, _) = painted_button(cx, bounds);
                    let painted = fills.iter().fold(color, |beneath, fill| {
                        composite(fill.as_solid().expect("a solid fill").to_rgb(), beneath)
                    });
                    readings.push((format!("{kind} on the {surface}"), painted));
                }
            }
            let mut lowest = f64::MAX;
            for (what, beneath) in &readings {
                let disabled = check(&mut failures, what, *beneath);
                lowest = lowest.min(disabled);
                if disabled < DISABLED_LABEL {
                    failures.push(format!("{name} {what} {beneath:06x}: {disabled:.3}:1"));
                }
            }
            let on_selected = check(&mut failures, "highlighted row", palette.selected);
            if on_selected < 1.5 {
                failures.push(format!("{name} highlighted row {on_selected:.2}:1"));
            }
            // Below the margin only where the first stronger step that keeps
            // it would take more than 60% of secondary text's contrast.
            let floor = if lowest >= DISABLED_LABEL + DISABLED_LABEL_MARGIN {
                kept += 1;
                DISABLED_LABEL + DISABLED_LABEL_MARGIN
            } else {
                let stronger = (1..=16u8)
                    .map(|step| Rgba {
                        a: dim.a + f32::from(step) / 64.,
                        ..dim
                    })
                    .take_while(|layer| layer.a <= 0.75)
                    .find(|&layer| {
                        readings.iter().all(|&(_, beneath)| {
                            contrast(composite(layer, beneath), beneath)
                                >= DISABLED_LABEL + DISABLED_LABEL_MARGIN
                        })
                    });
                let fainter = |layer: Rgba| {
                    readings
                        .iter()
                        .map(|&(_, beneath)| beneath)
                        .chain([palette.selected])
                        .all(|beneath| {
                            contrast(composite(layer, beneath), beneath)
                                <= contrast(palette.muted, beneath) * DISABLED_SHARE
                        })
                };
                if stronger.is_some_and(fainter) {
                    failures.push(format!(
                        "{name} reads {lowest:.3}:1 at its closest, where {stronger:?} keeps the margin"
                    ));
                }
                DISABLED_LABEL
            };
            if name == "Midnight" && floor == DISABLED_LABEL {
                failures.push(format!("Midnight reads {lowest:.3}:1 at its closest"));
            }
            if palette.disabled_label().a > 0.5 {
                moved += 1;
                if lowest > floor + 0.15 {
                    failures.push(format!("{name} reads {lowest:.2}:1 at its closest"));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert!(moved > 0, "some palette needs more than half strength");
        assert!(kept > 0, "some palette keeps the margin");
    }
    /// A disabled primary Button (Push, Commit) rests on the neutral fill a
    /// disabled secondary one does, the opaque `subtle` beside it, as does the
    /// shared helper's selected look (the command palette's Open command). The
    /// primary drew a 15% tint of the accent instead, grey #2a292f in Omarchy
    /// Retro-82's navy toolbar beside a navy Fetch, and the helper, where a
    /// palette moves its label (Omarchy Catppuccin Latte), drew no fill at all.
    /// Every palette, on each page surface and the menu's.
    #[gpui::test]
    fn every_palette_rests_disabled_primary_buttons_on_the_neutral_fill(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _| DisabledControls);
        let mut failures = Vec::new();
        for (name, palette) in test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let neutral = vec![Background::from(Hsla::from(rgb(palette.subtle)))];
            for (surface, _) in disabled_label_surfaces(palette) {
                for kind in ["secondary", "primary", "control-selected"] {
                    let bounds = cx
                        .debug_bounds(disabled_selector(kind, surface))
                        .expect("the button");
                    let (fills, _) = painted_button(cx, bounds);
                    if fills != neutral {
                        failures.push(format!("{name} {kind} on the {surface}: {fills:?}"));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A disabled Button that keeps a fill, the kit's secondary (Fetch, a
    /// disabled Cancel) and primary (Push, a dialog's default action) and
    /// the shared helper's selected look (Open command), keeps its shape with
    /// the kit's disabled edge on each page surface and the menu's, a
    /// dialog's canvas among them, in every palette: its neutral `subtle`
    /// fill stood 1.03 to 1.05:1 off the dialog in Rosé Pine, Graphite,
    /// Midnight, Braden and Ember, and no more than 1.34:1 off any surface,
    /// so it drew only its dim label. The edge holds 1.5:1 against each
    /// surface and the fill, and stays fainter than the dialog Cancel's
    /// edge; the Buttons that turn transparent, the shared helper at rest and
    /// the ghost, draw none. With More contrast on, each disabled Button
    /// draws the same edge, the palette's own 1.5:1 fit, which reads below
    /// the 3:1 resting edge every enabled control on a fill gains, on every
    /// surface: taken from the adjusted `border`, it was the resting edge
    /// itself (Midnight's #606976 on Delete branch… and Rename… beside it).
    #[gpui::test]
    fn every_disabled_button_that_keeps_a_fill_keeps_its_shape_with_an_edge(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _| DisabledControls);
        let mut failures = Vec::new();
        for (palette_name, palette) in test_palettes() {
            for more in [false, true] {
                let name = if more {
                    format!("{palette_name} with More contrast")
                } else {
                    palette_name.clone()
                };
                cx.update(|window, cx| {
                    set_more_contrast(more, cx);
                    palette.apply(palette.is_light(), Some(window), cx);
                });
                cx.update(|window, cx| window.draw(cx).clear(cx));
                let applied = if more {
                    palette.with_more_contrast()
                } else {
                    palette
                };
                let edge = palette.disabled_edge();
                for (surface, color) in disabled_label_surfaces(palette) {
                    for kind in DISABLED_BUTTONS {
                        let expected = match kind {
                            "secondary" | "primary" | "control-selected" => {
                                vec![Hsla::from(rgb(edge))]
                            }
                            // The kit's default Button keeps a border of its own.
                            "default" => continue,
                            _ => Vec::new(),
                        };
                        let edges = crate::views::tests::painted_edges(
                            cx,
                            disabled_selector(kind, surface),
                        );
                        if edges != expected {
                            failures.push(format!("{name} {kind} on the {surface}: {edges:?}"));
                        }
                    }
                    let ratio = contrast(edge, color);
                    if ratio < DISABLED_EDGE {
                        failures.push(format!("{name}: #{edge:06x} {ratio:.3}:1 on the {surface}"));
                    }
                    let resting = applied.resting_edge();
                    let enabled = contrast(resting, color);
                    if more && ratio >= enabled {
                        failures.push(format!(
                            "{name}: #{edge:06x} {ratio:.3}:1 on the {surface}, no fainter than \
                             the resting edge #{resting:06x} at {enabled:.3}:1"
                        ));
                    }
                }
                let on_fill = contrast(edge, palette.subtle);
                if on_fill < DISABLED_EDGE {
                    failures.push(format!("{name}: #{edge:06x} {on_fill:.3}:1 on its fill"));
                }
                let cancel = contrast(applied.dialog_edge(), palette.canvas);
                if contrast(edge, palette.canvas) >= cancel {
                    failures.push(format!("{name}: #{edge:06x} no fainter than Cancel's edge"));
                }
            }
        }
        cx.update(|_, cx| set_more_contrast(false, cx));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Compare's gutter numbers on added and removed lines are code-size text
    /// on the diff tiles, in `added` and `removed`, which the palettes fit to
    /// the text rule with no room: 4.51:1 and 4.57:1 in Omarchy Flexoki Light,
    /// read natively at 4.32:1 and 4.43:1. Every palette's numbers clear the
    /// rule with the margin on their tile, keeping a token that already does.
    #[test]
    fn every_palette_fits_gutter_numbers_on_the_diff_tiles() {
        let mut failures = Vec::new();
        let mut moved = 0;
        for (name, palette) in test_palettes() {
            let labels = Labels::fit(palette);
            for (what, token, label, tile) in [
                (
                    "added",
                    palette.added,
                    labels.added,
                    palette.added_background,
                ),
                (
                    "removed",
                    palette.removed,
                    labels.removed,
                    palette.removed_background,
                ),
            ] {
                match small_label_failures(&name, what, token, &[tile]) {
                    Some(_) => moved += 1,
                    None => assert_eq!(label, token, "{name} keeps its {what} color"),
                }
                failures.extend(small_label_failures(&name, what, label, &[tile]));
            }
        }
        assert!(moved > 0, "some palette needs its gutter numbers fitted");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Count chips (the Changes count, the changed-file count and "0 staged")
    /// draw their number in `text` on their `hover` fill, fitted with the
    /// margin: they drew `muted`, which secondary text keeps at the text rule
    /// alone, and 11 px digits read 4.32:1 natively in Omarchy Catppuccin
    /// Latte. Every palette's count clears the rule with the margin.
    #[test]
    fn every_palette_fits_count_chip_numbers() {
        let mut failures = Vec::new();
        for (name, palette) in test_palettes() {
            let count = Labels::fit(palette).count;
            if small_label_failures(&name, "", palette.text, &[palette.hover]).is_none() {
                assert_eq!(count, palette.text, "{name} keeps its text color");
            }
            failures.extend(small_label_failures(
                &name,
                "count",
                count,
                &[palette.hover],
            ));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Menus, popovers, selects and tooltips float on `Palette::raised`, the
    /// lightest page surface, edged by `Palette::raised_edge`: the Linux Menu
    /// used to share the panel's color, 1.00:1 against the header it drops
    /// from, with a 10% text-colored hairline at 1.12:1 to 1.15:1 in Omarchy
    /// Catppuccin Latte, Flexoki Light, Ethereal and Hackerman, and `border`
    /// alone would read 1.17:1 on Omarchy Matte Black's raised surface. In
    /// every palette the surface is at least as light as each page surface,
    /// its edge reads 1.4:1 against the surface and against every page surface
    /// beneath it, the canvas included, separators take that edge, text and
    /// secondary text clear the text rule on it, and the menu's hover row keeps
    /// the hover step off it. An edge fitted to the border rule's 1.3 alone
    /// (1.31:1 in Braden, 1.33:1 in Omarchy Everforest) drew 1.21:1 to 1.26:1
    /// once antialiased; one that has to move off `border` stops within 0.03
    /// above 1.4 on its closest surface, rather than a coarse step past it.
    #[gpui::test]
    fn every_palette_raises_menus_above_the_page_with_a_border_edge(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut failures = Vec::new();
        let mut moved = 0;
        for (name, palette) in test_palettes() {
            let (popover, ring, separator, hover) = cx.update(|cx| {
                palette.apply(palette.is_light(), None, cx);
                let theme = Theme::global(cx);
                (
                    theme.popover,
                    gpui_kit::component::popover_ring(cx),
                    gpui_kit::component::menu_separator(cx),
                    theme.tokens.accent.color,
                )
            });
            assert_eq!(popover, Hsla::from(rgb(palette.raised())), "{name}");
            assert_eq!(ring, Hsla::from(rgb(palette.raised_edge())), "{name} edge");
            // Separators lie on the menu, where `border` alone read 1.16:1 in
            // Omarchy Matte Black and 1.25:1 in Lumon.
            assert_eq!(separator, ring, "{name} separator");
            // The menu's hover row; the keyboard's cursor takes `list_active`.
            assert_eq!(hover, Hsla::from(rgb(palette.hover)), "{name} hover row");
            let (popover, hover) = (palette.raised(), palette.hover);
            let mut check = |what: &str, ratio: f64, minimum: f64| {
                if ratio < minimum {
                    failures.push(format!("{name} {what} {ratio:.3}:1, needs {minimum}"));
                }
            };
            for (page, surface) in [
                ("subtle", palette.subtle),
                ("canvas", palette.canvas),
                ("panel", palette.panel),
            ] {
                // Never darker than a page surface: a ratio of 1 where it is not.
                let lift = if luminance(popover) < luminance(surface) {
                    0.
                } else {
                    1.
                };
                check(&format!("lift over {page}"), lift, 1.);
                let edge = palette.raised_edge();
                check(&format!("edge on {page}"), contrast(edge, surface), 1.4);
            }
            check(
                "edge on the menu",
                contrast(palette.raised_edge(), popover),
                1.4,
            );
            check(
                "text on the menu",
                contrast(palette.text, popover),
                LABEL_RULE,
            );
            check(
                "muted on the menu",
                contrast(palette.muted, popover),
                LABEL_RULE,
            );
            check("hover row on the menu", contrast(hover, popover), 1.08);
            let edge = palette.raised_edge();
            if edge != palette.border {
                moved += 1;
                let closest = [popover, palette.subtle, palette.canvas, palette.panel]
                    .map(|surface| contrast(edge, surface))
                    .into_iter()
                    .fold(f64::MAX, f64::min);
                if closest > 1.43 {
                    failures.push(format!(
                        "{name} edge {edge:06x} {closest:.3}:1 at its closest"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert!(moved > 0, "some palette's edge moves off its border");
    }

    /// A custom theme on Midnight whose secondary text, gutter numbers and
    /// dividers sit close to its surfaces: 2.33:1, 1.81:1 and 1.15:1 off its
    /// canvas.
    fn faint_custom_palette() -> Palette {
        let mut custom = CustomTheme::from_base(1, "Faint", ThemeChoice::Midnight);
        custom.palette.set(TokenKind::Muted, 0x4a5262);
        custom.palette.set(TokenKind::LineNumber, 0x3a4252);
        custom.palette.set(TokenKind::Border, 0x1b2230);
        custom.palette
    }

    /// Every palette the More contrast tests judge: `test_palettes` and
    /// [`faint_custom_palette`].
    fn more_contrast_palettes() -> Vec<(String, Palette)> {
        let mut palettes = test_palettes();
        palettes.push(("custom Faint".into(), faint_custom_palette()));
        palettes
    }

    /// The surfaces More contrast reads secondary text and gutter numbers
    /// on, by name, each with whether it is momentary: the six surfaces of
    /// the readability rules, the raised surface and the added and removed
    /// line tints, of which hover and the hovered selected row are momentary.
    fn more_contrast_text_surfaces(palette: Palette) -> [(&'static str, u32, bool); 9] {
        [
            ("canvas", palette.canvas, false),
            ("panel", palette.panel, false),
            ("subtle", palette.subtle, false),
            ("raised", palette.raised(), false),
            ("selected", palette.selected, false),
            ("added lines", palette.added_background, false),
            ("removed lines", palette.removed_background, false),
            ("hover", palette.hover, true),
            ("hovered selected row", palette.row_hover(true), true),
        ]
    }

    /// What More contrast holds secondary text and gutter numbers to on
    /// `surface`: 7:1 where it rests and the text rule's 4.5:1 where it is
    /// `momentary`, or as far as `text` itself reads there where that is
    /// lower.
    fn more_contrast_text_floor(palette: Palette, surface: u32, momentary: bool) -> f64 {
        let floor = if momentary {
            LABEL_RULE
        } else {
            MORE_CONTRAST_TEXT
        };
        floor.min(contrast(palette.text, surface))
    }

    /// The surfaces dividers and the edges fitted from them rest on, by name.
    fn more_contrast_edge_surfaces(palette: Palette) -> [(&'static str, u32); 4] {
        [
            ("canvas", palette.canvas),
            ("panel", palette.panel),
            ("subtle", palette.subtle),
            ("raised", palette.raised()),
        ]
    }

    /// More contrast on every palette, Omarchy's on Linux and a custom one
    /// with faint tokens among them: secondary text and gutter numbers move
    /// toward `text` by the least step of `mix` that reads 7:1 on the
    /// resting surfaces and both line tints and 4.5:1 on hover and the
    /// hovered selected row, or as `text` reads where that is lower, so they
    /// stay apart from `text` wherever `text` reads 7:1 on every resting
    /// surface, a hover falling short included; dividers hold 3:1 on the
    /// canvas, panel, subtle and raised surfaces, and so do the popup and
    /// resting edges fitted from them. Secondary text and gutter numbers are
    /// measured on the line tints as adjusted, where a line moved from a
    /// changed word it is drawn under
    /// (`more_contrast_moves_a_line_its_word_cannot_stand_apart_from`). No
    /// other token moves, a token already past its floor keeps its value, so
    /// raising the floors of an adjusted palette changes nothing, and where
    /// no line tint moves, adjusting twice is adjusting once.
    #[test]
    fn more_contrast_raises_secondary_text_gutters_and_dividers_alone() {
        let mut failures = Vec::new();
        let mut moved = std::collections::BTreeMap::<&str, usize>::new();
        // Palettes whose `text` misses 7:1 only on a momentary surface.
        let mut hover_short = Vec::new();
        for (name, palette) in more_contrast_palettes() {
            let adjusted = palette.with_more_contrast();
            let surfaces = more_contrast_text_surfaces(adjusted);
            let reads = |color: u32| {
                surfaces.iter().all(|&(_, surface, momentary)| {
                    contrast(color, surface)
                        >= more_contrast_text_floor(palette, surface, momentary)
                })
            };
            let text_short = |momentary: bool| {
                surfaces.iter().any(|&(_, surface, on_hover)| {
                    on_hover == momentary && contrast(palette.text, surface) < MORE_CONTRAST_TEXT
                })
            };
            if text_short(true) && !text_short(false) {
                hover_short.push(name.clone());
            }
            for (token, before, after) in [
                ("muted", palette.muted, adjusted.muted),
                ("line_number", palette.line_number, adjusted.line_number),
            ] {
                // Where `text` reads 7:1 on every resting surface, the token
                // reads below it on one of them: it stays secondary.
                let apart = surfaces.iter().any(|&(_, surface, momentary)| {
                    !momentary && contrast(after, surface) < contrast(palette.text, surface)
                });
                if !text_short(false) && !apart {
                    failures.push(format!(
                        "{name} {token} #{after:06x} reads as text #{:06x} does",
                        palette.text
                    ));
                }
                for (surface, color, momentary) in surfaces {
                    let (ratio, floor) = (
                        contrast(after, color),
                        more_contrast_text_floor(palette, color, momentary),
                    );
                    if ratio < floor {
                        failures.push(format!(
                            "{name} {token} #{after:06x} on {surface} {ratio:.3}:1, needs {floor:.3}"
                        ));
                    }
                }
                // The least step toward `text` that reads.
                match (0..=256).find(|&step| mix(before, palette.text, step) == after) {
                    Some(step) if step == 0 || !reads(mix(before, palette.text, step - 1)) => {}
                    step => failures.push(format!(
                        "{name} {token} #{before:06x} -> #{after:06x} is not the least reading \
                         step toward text: {step:?}"
                    )),
                }
                if after != before {
                    *moved.entry(token).or_default() += 1;
                }
            }
            for (surface, color) in more_contrast_edge_surfaces(palette) {
                for (edge, value) in [
                    ("border", adjusted.border),
                    ("popup edge", adjusted.raised_edge()),
                    ("resting edge", adjusted.resting_edge()),
                ] {
                    let ratio = contrast(value, color);
                    if ratio < MORE_CONTRAST_EDGE {
                        failures.push(format!(
                            "{name} {edge} #{value:06x} on {surface} {ratio:.3}:1"
                        ));
                    }
                }
            }
            let edge_holds = more_contrast_edge_surfaces(palette)
                .iter()
                .all(|&(_, surface)| contrast(palette.border, surface) >= MORE_CONTRAST_EDGE);
            if edge_holds && adjusted.border != palette.border {
                failures.push(format!("{name} border past its floor moved"));
            }
            if adjusted.border != palette.border {
                *moved.entry("border").or_default() += 1;
            }
            let untouched = Palette {
                muted: palette.muted,
                line_number: palette.line_number,
                border: palette.border,
                added_background: palette.added_background,
                removed_background: palette.removed_background,
                ..adjusted
            };
            if untouched != palette {
                failures.push(format!("{name} moved another token: {untouched:x?}"));
            }
            if adjusted.with_raised_floors() != adjusted {
                failures.push(format!(
                    "{name} raises its floors again on the adjusted palette"
                ));
            }
            let lines = |palette: Palette| (palette.added_background, palette.removed_background);
            if lines(adjusted) == lines(palette) && adjusted.with_more_contrast() != adjusted {
                failures.push(format!("{name} moves again when adjusted twice"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert!(
            hover_short.iter().any(|name| name == "Tokyo Night"),
            "a palette whose text misses 7:1 only on a hover keeps its secondary text apart: \
             {hover_short:?}"
        );
        // Each adjustment does something somewhere, the faint palette's all three.
        let faint = faint_custom_palette();
        let adjusted = faint.with_more_contrast();
        assert_ne!(adjusted.muted, faint.muted);
        assert_ne!(adjusted.line_number, faint.line_number);
        assert_ne!(adjusted.border, faint.border);
        assert!(moved.values().all(|&count| count > 1), "{moved:?}");
        // Already past every floor, a palette is its own adjustment.
        let strong = Palette {
            muted: faint.text,
            line_number: faint.text,
            border: adjusted.border,
            ..faint
        };
        assert_eq!(strong.with_more_contrast(), strong);
    }

    /// With More contrast on, what `Palette::apply` installs meets the
    /// raised floors in every palette. The `Palette` global is the adjusted
    /// palette, and `as_applied` returns it; the kit draws secondary text and
    /// gutter numbers from it. Every syntax color and the editor foreground
    /// read 7:1, or as `text` reads where that is lower, on the editor
    /// background, the active line and both line tints, or stand at `text`'s
    /// lightness, where a role's fit stops short of that. The kit's disabled
    /// label reads 4.5:1 on every surface a disabled label rests on and on
    /// the default Button's disabled fill over each, fainter than the
    /// adjusted secondary text there and on the selected row, and at most 75%
    /// of its contrast wherever a layer reading 4.5:1 can be. The dividers,
    /// the fields' outlines and the popup, dialog and bordered secondary
    /// edges hold 3:1 on the surfaces each is fitted for, as the resting edge
    /// does, while the disabled edge and the unchecked Switch track's fill
    /// and hover keep the chosen palette's values.
    #[gpui::test]
    fn more_contrast_applies_its_raised_floors_in_every_palette(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut failures = Vec::new();
        for (name, chosen) in more_contrast_palettes() {
            let (applied, as_applied, theme) = cx.update(|cx| {
                set_more_contrast(true, cx);
                chosen.apply(chosen.is_light(), None, cx);
                (
                    palette(cx),
                    chosen.as_applied(cx),
                    Theme::global(cx).clone(),
                )
            });
            assert_eq!(applied, chosen.with_more_contrast(), "{name} applied");
            assert_eq!(as_applied, applied, "{name} as applied");
            let hsla = |color: u32| Hsla::from(rgb(color));
            assert_eq!(theme.colors.muted_foreground, hsla(applied.muted), "{name}");
            assert_eq!(theme.colors.border, hsla(applied.border), "{name}");
            assert_eq!(theme.colors.input, hsla(applied.border), "{name}");
            let style = &theme.highlight_theme.style;
            assert_eq!(
                style.editor_line_number,
                Some(hsla(applied.line_number)),
                "{name}"
            );
            let line = |color: Option<Hsla>| packed(color.expect("a configured line"));
            let lines = [
                ("editor background", line(style.editor_background)),
                ("active line", line(style.editor_active_line)),
                ("added lines", applied.added_background),
                ("removed lines", applied.removed_background),
            ];
            for (field, color) in editor_text_colors(&theme) {
                let Some(color) = color else { continue };
                for (surface, background) in lines {
                    let floor = MORE_CONTRAST_TEXT.min(contrast(applied.text, background));
                    let ratio = contrast(color, background);
                    // A role short of its floor at `text`'s lightness stopped there
                    // (`more_contrast_raises_syntax_in_lightness_keeping_its_hue`).
                    if ratio < floor && !at_text_lightness(color, applied.text) {
                        failures.push(format!(
                            "{name} {field} #{color:06x} on {surface} {ratio:.3}:1, needs {floor:.3}"
                        ));
                    }
                }
            }
            let dim = theme.disabled_foreground().to_rgb();
            let page = disabled_label_surfaces(applied);
            // The default Button's disabled fill is a fill, the chosen
            // palette's, as the kit derives it from `input_fill_base`.
            let fill = chosen.default_disabled_fill();
            let beneath = page
                .iter()
                .map(|&(surface, color)| (surface.to_owned(), color))
                .chain(page.iter().map(|&(surface, color)| {
                    (
                        format!("disabled fill on {surface}"),
                        composite(fill, color),
                    )
                }));
            for (surface, color) in beneath {
                let (disabled, secondary) = (
                    contrast(composite(dim, color), color),
                    contrast(applied.muted, color),
                );
                if disabled < MORE_CONTRAST_DISABLED || disabled >= secondary {
                    failures.push(format!(
                        "{name} disabled label on {surface} {disabled:.3}:1, secondary text {secondary:.3}:1"
                    ));
                }
            }
            let on_selected = contrast(composite(dim, applied.selected), applied.selected);
            if on_selected >= contrast(applied.muted, applied.selected) {
                failures.push(format!(
                    "{name} disabled label on the selected row {on_selected:.3}:1"
                ));
            }
            // The 75% ceiling: wherever a layer of `muted` reads 4.5:1 with
            // the margin and keeps at most 75% of secondary text's contrast,
            // the label keeps it; where none does, no fainter layer reads
            // 4.5:1, so the label passes the ceiling no further than the
            // floor takes it.
            let rests: Vec<u32> = page
                .iter()
                .map(|&(_, color)| color)
                .chain(page.iter().map(|&(_, color)| composite(fill, color)))
                .collect();
            let layer = |a: f32| Rgba {
                a,
                ..rgb(applied.muted)
            };
            let reading = |label: Rgba, surface: u32| contrast(composite(label, surface), surface);
            let reads = |label: Rgba, floor: f64| {
                rests
                    .iter()
                    .all(|&surface| reading(label, surface) >= floor)
            };
            let under_ceiling = |label: Rgba| {
                rests.iter().chain([&applied.selected]).all(|&surface| {
                    reading(label, surface)
                        <= contrast(applied.muted, surface) * MORE_CONTRAST_DISABLED_SHARE + 1e-9
                })
            };
            let layers = (0..=28u8).map(|step| layer(0.5 + f32::from(step) / 64.));
            let label = layer(dim.a);
            if layers.clone().any(|candidate| {
                reads(candidate, MORE_CONTRAST_DISABLED + DISABLED_LABEL_MARGIN)
                    && under_ceiling(candidate)
            }) {
                if !under_ceiling(label) {
                    failures.push(format!(
                        "{name} disabled label at {} passes 75% of secondary text",
                        dim.a
                    ));
                }
            } else if let Some(fainter) = layers
                .clone()
                .find(|candidate| candidate.a < dim.a && reads(*candidate, MORE_CONTRAST_DISABLED))
            {
                failures.push(format!(
                    "{name} disabled label at {} where {} reads 4.5:1 nearer the ceiling",
                    dim.a, fainter.a
                ));
            }
            let backdrop = applied.dialog_backdrop();
            let dialog = [
                ("dialog", applied.canvas),
                ("dimmed canvas", backdrop_over(applied.canvas, backdrop)),
                ("dimmed panel", backdrop_over(applied.panel, backdrop)),
                ("dimmed subtle", backdrop_over(applied.subtle, backdrop)),
            ];
            let page = more_contrast_edge_surfaces(applied);
            let edges = [
                ("divider", Some(theme.colors.border), &page[..]),
                ("field outline", Some(theme.colors.input), &page[..]),
                ("popup edge", theme.popover_edge, &page[..]),
                ("dialog edge", theme.dialog_edge, &dialog[..]),
                (
                    "bordered secondary edge",
                    theme.secondary_edge,
                    &dialog[..1],
                ),
                ("resting edge", theme.resting_edge, &page[..]),
            ];
            // The kit's resting edge is the adjusted palette's own, its
            // `border`.
            assert_eq!(
                theme.resting_edge,
                Some(hsla(applied.resting_edge())),
                "{name} resting edge"
            );
            assert_eq!(applied.resting_edge(), applied.border, "{name}");
            // A disabled control stays fainter, and a track is a fill.
            assert_eq!(
                (theme.disabled_edge, theme.colors.switch, theme.switch_hover),
                (
                    Some(hsla(chosen.disabled_edge())),
                    hsla(chosen.border),
                    Some(hsla(chosen.switch_hover()))
                ),
                "{name} disabled edge and switch track"
            );
            for (edge, color, surfaces) in edges {
                let Some(color) = color.map(packed) else {
                    failures.push(format!("{name} {edge} unset"));
                    continue;
                };
                for &(surface, beneath) in surfaces {
                    let ratio = contrast(color, beneath);
                    if ratio < MORE_CONTRAST_EDGE {
                        failures.push(format!(
                            "{name} {edge} #{color:06x} on {surface} {ratio:.3}:1"
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The most sRGB rounding leaves between a syntax role More contrast
    /// stops at `text`'s lightness and that lightness, with margin: the
    /// largest in every test palette is 0.0012.
    const AT_TEXT_LIGHTNESS: f64 = 0.002;

    /// The most sRGB rounding moves a role's OKLab opponent coordinates off
    /// its palette color's hue, or off the chroma sRGB holds at its
    /// lightness, with margin: the largest in every test palette are 0.0014
    /// off the hue and 0.0015 off the chroma.
    const HUE_ROUNDING: f64 = 0.004;

    /// Whether `color` stands at `text`'s OKLab lightness, as a syntax role
    /// whose fit stops there under More contrast does.
    fn at_text_lightness(color: u32, text: u32) -> bool {
        (oklab(color)[0] - oklab(text)[0]).abs() <= AT_TEXT_LIGHTNESS
    }

    /// How far `color`'s OKLab opponent coordinates stand from the ray of
    /// `source`'s hue, and whether they lie on that ray rather than past
    /// grey from it.
    fn hue_stray(color: u32, source: u32) -> (f64, bool) {
        let ([_, a, b], [_, source_a, source_b]) = (oklab(color), oklab(source));
        let chroma = source_a.hypot(source_b);
        let (along_a, along_b) = (source_a / chroma, source_b / chroma);
        (
            (a * along_b - b * along_a).abs(),
            a * along_a + b * along_b > 0.,
        )
    }

    /// The roles More contrast fits in lightness, by name, each with the
    /// palette color it draws from and its color in `roles`: every role but
    /// comments, which follow secondary text, and `text`.
    fn lightness_roles(palette: Palette, roles: SyntaxRoles) -> [(&'static str, u32, u32); 7] {
        [
            ("keyword", palette.renamed, roles.keyword),
            ("string", palette.added, roles.string),
            ("constant", palette.warning, roles.constant),
            ("type", palette.modified, roles.type_),
            ("function", palette.hunk, roles.function),
            ("markup", palette.accent, roles.markup),
            ("removed", palette.removed, roles.removed),
        ]
    }

    /// The backgrounds syntax is fitted on, by name: the editor background,
    /// the active line and both line tints.
    fn syntax_backgrounds(palette: Palette) -> [(&'static str, u32); 4] {
        [
            ("editor background", palette.canvas),
            ("active line", palette.panel),
            ("added lines", palette.added_background),
            ("removed lines", palette.removed_background),
        ]
    }

    /// Under More contrast every syntax role but comments, in every palette,
    /// moves only in OKLab lightness, toward `text`'s side, keeping its
    /// palette color's hue and chroma, or where sRGB cannot hold that chroma
    /// at its lightness the most sRGB holds there, in that hue: it reads
    /// 7:1, or as `text` reads where that is lower, on the editor
    /// background, the active line and both line tints as More contrast
    /// draws them, or stands at `text`'s lightness, and never moves past it.
    /// A role whose palette color already reads there is that color, byte
    /// for byte, as it is drawn off, and every role reads on each of those
    /// backgrounds at least as the role drawn off reads there with More
    /// contrast off. Mixed toward `text` in sRGB, the roles spent their
    /// chroma and turned their hue, and where `text` reads under 7:1 ended
    /// at `text` itself: Omarchy tokyo-night's keyword, function and markup
    /// drew #a9b1d6, where they keep their hues at `text`'s lightness
    /// (#bfa0f9, #8db2ff, #8db2ff). Unbound by `text`'s lightness, 29 roles
    /// passed it by more than rounding, One Dark's keyword by 0.020.
    #[test]
    fn more_contrast_raises_syntax_in_lightness_keeping_its_hue() {
        let mut failures = Vec::new();
        let (mut kept, mut stopped) = (0, 0);
        for (name, chosen) in more_contrast_palettes() {
            let applied = chosen.with_more_contrast();
            let (off, on) = (
                chosen.syntax_roles(None),
                applied.syntax_roles(Some(chosen)),
            );
            let backgrounds = syntax_backgrounds(applied);
            let reads = |color: u32| {
                backgrounds.iter().all(|&(_, background)| {
                    let floor = MORE_CONTRAST_TEXT.min(contrast(applied.text, background));
                    contrast(color, background) >= floor
                })
            };
            let text = oklab(applied.text)[0];
            let toward = if luminance(applied.text) > luminance(applied.canvas) {
                1.
            } else {
                -1.
            };
            let drawn_off = lightness_roles(chosen, off).map(|(.., color)| color);
            for ((role, source, fitted), drawn_off) in
                lightness_roles(chosen, on).into_iter().zip(drawn_off)
            {
                let at = format!("{name} {role} #{fitted:06x}");
                for ((surface, background), (_, off_background)) in
                    backgrounds.into_iter().zip(syntax_backgrounds(chosen))
                {
                    let (is, was) = (
                        contrast(fitted, background),
                        contrast(drawn_off, off_background),
                    );
                    if is < was {
                        failures.push(format!(
                            "{at} on {surface} {is:.3}:1, #{drawn_off:06x} {was:.3}:1 off"
                        ));
                    }
                }
                if reads(source) {
                    kept += 1;
                    if fitted != source || drawn_off != source {
                        failures.push(format!(
                            "{at} from #{source:06x}, which reads, #{drawn_off:06x} off"
                        ));
                    }
                    continue;
                }
                let [lightness, a, b] = oklab(fitted);
                let past = (lightness - text) * toward;
                if fitted != source && past > AT_TEXT_LIGHTNESS {
                    failures.push(format!("{at} {past:.4} past text's lightness"));
                }
                if !reads(fitted) {
                    stopped += 1;
                    if past < -AT_TEXT_LIGHTNESS {
                        failures.push(format!(
                            "{at} short of its floor {:.4} before text's lightness",
                            -past
                        ));
                    }
                }
                let [_, source_a, source_b] = oklab(source);
                if source_a.hypot(source_b) >= GREY_CHROMA {
                    let (stray, along) = hue_stray(fitted, source);
                    if stray > HUE_ROUNDING || !along {
                        failures.push(format!("{at} {stray:.4} off #{source:06x}'s hue"));
                    }
                }
                let held = oklab(from_linear(linear_in_gamut([
                    lightness, source_a, source_b,
                ])));
                let (chroma, held) = (a.hypot(b), held[1].hypot(held[2]));
                if (chroma - held).abs() > HUE_ROUNDING {
                    failures.push(format!(
                        "{at} chroma {chroma:.4}, #{source:06x}'s held there {held:.4}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert!(
            kept > 0 && stopped > 0,
            "{kept} roles kept, {stopped} stopped at text's lightness"
        );
    }

    /// Tokyo Night with its added lines lightened until `text` reads 6.25:1
    /// there: under More contrast a role reads as `text` reads on those
    /// lines and 7:1 elsewhere, short of `text`'s lightness where it can.
    /// Its string #9ece6a stops at #aadb76, 0.011 darker than `text`; held
    /// to 7:1 on those lines too, it ran on to `text`'s lightness.
    #[test]
    fn a_role_reads_as_text_does_where_text_reads_under_seven() {
        let palette = Palette {
            added_background: 0x33463c,
            ..ThemeChoice::TokyoNight.palette()
        };
        let on_added = contrast(palette.text, palette.added_background);
        assert!(on_added < MORE_CONTRAST_TEXT, "text {on_added:.3}:1");
        let string = palette.syntax_roles(Some(palette)).string;
        for (surface, background) in syntax_backgrounds(palette) {
            let (ratio, floor) = (
                contrast(string, background),
                MORE_CONTRAST_TEXT.min(contrast(palette.text, background)),
            );
            assert!(
                ratio >= floor,
                "#{string:06x} on {surface} {ratio:.3}:1, needs {floor:.3}"
            );
        }
        let short = oklab(palette.text)[0] - oklab(string)[0];
        assert!(short > 0.005, "#{string:06x} {short:.4} short of text");
        let (stray, along) = hue_stray(string, palette.added);
        assert!(stray <= HUE_ROUNDING && along, "#{string:06x} {stray:.4}");
        assert_eq!(string, 0xaadb76);
    }

    /// Tokyo Night with its added lines lightened until `text` reads 7.05:1
    /// there: its removed #f7768e reaches `text`'s lightness before 7:1 on
    /// those lines, so it stops there, #ffb5bf in its own hue at 6.83:1,
    /// never lighter than `text`; unbound, it passed `text` by 0.007 to
    /// read 7:1.
    #[test]
    fn a_role_stops_at_text_lightness_where_that_comes_first() {
        let palette = Palette {
            added_background: 0x2c3e34,
            ..ThemeChoice::TokyoNight.palette()
        };
        let on_added = contrast(palette.text, palette.added_background);
        assert!(on_added >= MORE_CONTRAST_TEXT, "text {on_added:.3}:1");
        let removed = palette.syntax_roles(Some(palette)).removed;
        let ratio = contrast(removed, palette.added_background);
        assert!(ratio < MORE_CONTRAST_TEXT, "#{removed:06x} {ratio:.3}:1");
        assert!(
            at_text_lightness(removed, palette.text),
            "#{removed:06x} {:+.4} from text's lightness",
            oklab(removed)[0] - oklab(palette.text)[0]
        );
        let (stray, along) = hue_stray(removed, palette.removed);
        assert!(stray <= HUE_ROUNDING && along, "#{removed:06x} {stray:.4}");
        assert_eq!(removed, 0xffb5bf);
    }

    /// Tokyo Night with its added lines lightened until `text` reads 4.88:1
    /// there: its removed #f7768e stopped at `text`'s lightness, #ffb5bf,
    /// reads 4.73:1 on those lines, below the 4.77:1 the role drawn off,
    /// #c2c7f1, reads there, so under More contrast it keeps the role drawn
    /// off.
    #[test]
    fn a_role_that_would_read_worse_than_off_keeps_the_role_drawn_off() {
        let palette = Palette {
            added_background: 0x42564c,
            ..ThemeChoice::TokyoNight.palette()
        };
        let (off, on) = (
            palette.syntax_roles(None).removed,
            palette.syntax_roles(Some(palette)).removed,
        );
        let [_, a, b] = oklab(palette.removed);
        let stopped = from_linear(linear_in_gamut([oklab(palette.text)[0], a, b]));
        let line = palette.added_background;
        assert!(
            contrast(stopped, line) < contrast(off, line),
            "#{stopped:06x} {:.3}:1, #{off:06x} {:.3}:1 off",
            contrast(stopped, line),
            contrast(off, line)
        );
        assert_eq!(on, off, "#{on:06x}, #{off:06x} off");
        assert_ne!(on, palette.removed);
    }

    /// Tokyo Night with its added lines lightened until `text` reads 4.92:1
    /// there, then moved darker under More contrast as a line a word cannot
    /// leave moves, from #44554a to #405146: each role reads on the editor
    /// background, the active line and both line tints at least as the role
    /// drawn off reads there on the palette as chosen. Its removed #f7768e,
    /// held only to the off fit on the moved line, stopped at `text`'s
    /// lightness, #ffb5bf, 10.27:1 on the editor background, where the role
    /// drawn off, #c3c6f0, reads 10.30:1, so it keeps that role.
    #[test]
    fn a_role_reads_as_the_role_drawn_off_on_the_palette_as_chosen() {
        let chosen = Palette {
            added_background: 0x44554a,
            ..ThemeChoice::TokyoNight.palette()
        };
        let adjusted = Palette {
            added_background: 0x405146,
            ..chosen
        };
        let (off, on) = (
            chosen.syntax_roles(None),
            adjusted.syntax_roles(Some(chosen)),
        );
        let drawn_off = lightness_roles(chosen, off).map(|(.., color)| color);
        for ((role, _, fitted), drawn_off) in lightness_roles(chosen, on).into_iter().zip(drawn_off)
        {
            for ((surface, background), (_, off_background)) in syntax_backgrounds(adjusted)
                .into_iter()
                .zip(syntax_backgrounds(chosen))
            {
                let (is, was) = (
                    contrast(fitted, background),
                    contrast(drawn_off, off_background),
                );
                assert!(
                    is >= was,
                    "{role} #{fitted:06x} on {surface} {is:.3}:1, #{drawn_off:06x} {was:.3}:1 off"
                );
            }
        }
        assert_eq!(on.removed, off.removed);
        assert_eq!(off.removed, 0xc3c6f0);
    }

    /// A word tint's gap from its line in OKLab lightness, signed: positive
    /// where the tint is lighter.
    fn lightness_gap(tint: u32, line: u32) -> f64 {
        oklab(tint)[0] - oklab(line)[0]
    }

    /// [`lightness_gap`] as protanopia and deuteranopia see the two colors
    /// ([`colour_blind_lightness`]).
    fn colour_blind_gaps(tint: u32, line: u32) -> [f64; 2] {
        let [tint, line] = [tint, line].map(colour_blind_lightness);
        [0, 1].map(|model| tint[model] - line[model])
    }

    /// The colors drawn on a word tint, as `roles` carry them, and `diff`.
    fn on_a_word(roles: SyntaxRoles, diff: u32) -> impl Iterator<Item = u32> {
        roles.word_colors().into_iter().chain([diff])
    }

    /// Of the colors `roles` and `diff` draw on the word tint `on`, the
    /// first that reads below its counterpart in `own` on `off`, the tint
    /// drawn with More contrast off, as a message.
    fn reads_worse_on_a_word(
        (roles, on): (SyntaxRoles, u32),
        (own, off): (SyntaxRoles, u32),
        diff: u32,
    ) -> Option<String> {
        on_a_word(roles, diff)
            .zip(on_a_word(own, diff))
            .find(|&(color, own)| contrast(color, on) < contrast(own, off))
            .map(|(color, own)| {
                format!(
                    "#{color:06x} {:.3}:1 on #{on:06x}, #{own:06x} {:.3}:1 on #{off:06x}",
                    contrast(color, on),
                    contrast(own, off)
                )
            })
    }

    /// Under More contrast a word tint nearer its line in OKLab lightness
    /// than [`WORD_LIGHTNESS_GAP`] lifts away from it on the side it lies,
    /// in synthetic tints on a dark line. Darker than the line under light
    /// text, 0.011 from it, the tint lifts darker until the gap reaches
    /// 0.04 and stops there, its hue and chroma kept, its luminance contrast
    /// with the line growing, and the text reading better. Lighter than the
    /// line, 0.011 from it, under light text, the lift lowers the text's
    /// contrast, so it stops at the last step where the raised text reads
    /// at least as the text drawn off (a partial lift, 0.027), and where the
    /// raised text reads no better than off, the tint drawn off stays.
    #[test]
    fn a_faint_word_tint_lifts_from_its_line_in_lightness() {
        let all = |color: u32| SyntaxRoles {
            keyword: color,
            string: color,
            constant: color,
            type_: color,
            function: color,
            markup: color,
            comment: color,
            removed: color,
            text: color,
        };
        let line = 0x30343a;
        let lifted = |(raised, own): (u32, u32), off: u32, diff: u32| {
            EditorHighlights::lifted_word_tint(all(raised), all(own), off, diff, line)
        };
        let hue = |color: u32| oklab(color)[2].atan2(oklab(color)[1]);
        let chroma = |color: u32| oklab(color)[1].hypot(oklab(color)[2]);

        // Darker than its line, beneath light text: the full lift.
        let (off, text, diff) = (0x1e372c, (0xb4b8be, 0xa0a4aa), 0xa0d0b0);
        assert!((-0.012..-0.01).contains(&lightness_gap(off, line)));
        let tint = lifted(text, off, diff);
        let gap = lightness_gap(tint, line);
        assert!(
            (-0.045..=-WORD_LIGHTNESS_GAP).contains(&gap),
            "#{tint:06x} {gap:+.4} from its line #{line:06x}, off #{off:06x}"
        );
        assert_eq!(lightness_lifts(off, line).last(), Some(&tint));
        assert!((hue(tint) - hue(off)).abs() < 0.05 && (chroma(tint) - chroma(off)).abs() < 0.003);
        assert!(fainter_on_its_line(tint, off, line).is_none());
        assert!(contrast(tint, line) > contrast(off, line));
        assert_eq!(
            reads_worse_on_a_word((all(text.0), tint), (all(text.1), off), diff),
            None
        );

        // Lighter than its line, between it and light text: the partial
        // lift, stopped where the raised text would read worse than off.
        let (off, diff) = (0x233d31, 0x101010);
        assert!((0.01..0.012).contains(&lightness_gap(off, line)));
        let text = (0xb4b8be, 0xaeb2b8);
        let tint = lifted(text, off, diff);
        let gap = lightness_gap(tint, line);
        assert!(
            (0.02..WORD_LIGHTNESS_GAP).contains(&gap),
            "#{tint:06x} {gap:+.4} from its line, off #{off:06x}"
        );
        assert_eq!(
            reads_worse_on_a_word((all(text.0), tint), (all(text.1), off), diff),
            None
        );
        let path = lightness_lifts(off, line);
        let next = path[path.iter().position(|&lift| lift == tint).unwrap() + 1];
        assert!(
            reads_worse_on_a_word((all(text.0), next), (all(text.1), off), diff).is_some(),
            "the step past #{tint:06x}, #{next:06x}, keeps the text"
        );
        assert!(fainter_on_its_line(tint, off, line).is_none());

        // The raised text reads no better than off: no lift keeps it.
        assert_eq!(lifted((0xb4b8be, 0xb4b8be), off, diff), off);

        // Rounding can leave a step further from the line in lightness
        // and luminance but nearer in OKLab than the tint: #1d362c, the
        // first step from #1e372c. Where text reads on that step but not
        // on the next, the lift would stop on it, nearer its line than
        // off; the step is never listed, so the tint drawn off stays.
        let (off, near, line) = (0x1e372c, 0x1d362c, 0x30343a);
        let apart = |tint: u32| oklab_distance(oklab(tint), oklab(line));
        assert!(
            lightness_gap(near, line).abs() > lightness_gap(off, line).abs()
                && contrast(near, line) > contrast(off, line)
                && apart(near) < apart(off)
        );
        // Black text reads on #1d362c at least as #000302 reads off, and
        // not on the next step.
        let (raised, own) = (0x000000, 0x000302);
        let tint = EditorHighlights::lifted_word_tint(all(raised), all(own), off, 0xffffff, line);
        assert_eq!(tint, off, "#{tint:06x}, off #{off:06x}");
        let path = lightness_lifts(off, line);
        assert!(!path.is_empty() && !path.contains(&near));
        assert!(contrast(raised, near) >= contrast(own, off));
        assert!(contrast(raised, path[0]) < contrast(own, off));
    }

    /// The hue difference of two colors in OKLab, in radians.
    fn hue_difference(a: u32, b: u32) -> f64 {
        let hue = |color: u32| oklab(color)[2].atan2(oklab(color)[1]);
        let turn = std::f64::consts::TAU;
        ((hue(a) - hue(b) + turn / 2.).rem_euclid(turn) - turn / 2.).abs()
    }

    /// Where sRGB cannot hold a lifted tint's chroma at its new lightness,
    /// near black and near white, the lift lowers the chroma and keeps the
    /// hue: in a near-black green (#00351d on #19322d) and a near-white one
    /// (#e8f4c5 on #efeedc), every step stays within 0.03 radians of the
    /// tint's hue and never gains chroma beyond rounding, and the last
    /// reaches the gap with its chroma lowered. Clipped to sRGB instead,
    /// the steps turned up to 0.057 and 0.091 radians.
    #[test]
    fn a_word_tint_lift_keeps_its_hue_where_the_gamut_takes_chroma() {
        let chroma = |color: u32| oklab(color)[1].hypot(oklab(color)[2]);
        for (off, line) in [(0x00351d, 0x19322d), (0xe8f4c5, 0xefeedc)] {
            let path = lightness_lifts(off, line);
            let last = *path.last().expect("a lift");
            assert!(lightness_gap(last, line).abs() >= WORD_LIGHTNESS_GAP);
            assert!(
                chroma(last) < chroma(off) - 0.005,
                "#{last:06x} chroma {:.4}, off #{off:06x} {:.4}: the gamut binds",
                chroma(last),
                chroma(off)
            );
            for step in path {
                assert!(
                    hue_difference(step, off) < 0.03 && chroma(step) <= chroma(off) + 0.0015,
                    "#{step:06x} turned {:.4} from #{off:06x}, chroma {:.4} from {:.4}",
                    hue_difference(step, off),
                    chroma(step),
                    chroma(off)
                );
            }
        }
    }

    /// A lift that runs out of lightness stops at the last step before
    /// white: on a line #f5f3f0, from which white stands 0.035 in OKLab
    /// lightness, a tint #f9f8f6 lighter than it under dark text lifts to
    /// the end of its path, short of the gap, its text reading better.
    #[test]
    fn a_word_tint_lift_stops_where_lightness_runs_out() {
        let (off, line, text) = (0xf9f8f6, 0xf5f3f0, 0x202020);
        assert!(lightness_gap(0xffffff, line) < WORD_LIGHTNESS_GAP);
        let roles = SyntaxRoles {
            keyword: text,
            string: text,
            constant: text,
            type_: text,
            function: text,
            markup: text,
            comment: text,
            removed: text,
            text,
        };
        let tint = EditorHighlights::lifted_word_tint(roles, roles, off, text, line);
        assert_eq!(lightness_lifts(off, line).last(), Some(&tint));
        let gap = lightness_gap(tint, line);
        assert!(
            gap > lightness_gap(off, line) + 0.015 && gap < WORD_LIGHTNESS_GAP,
            "#{tint:06x} {gap:+.4} from its line, off #{off:06x}"
        );
        assert!(contrast(tint, line) > contrast(off, line));
        assert!(contrast(text, tint) > contrast(text, off));
    }

    /// How a highlight drawn under More contrast, `on`, marks itself on
    /// `line` more faintly than `off`, the one drawn with it off, if it
    /// does: by a lower luminance contrast with the line, or on the other
    /// side of it in luminance. A highlight's lightness against its line is
    /// the mark that survives colour-blind vision; one that stood apart
    /// through saturation alone took its line's luminance (every raised word
    /// tint 1.00 to 1.01:1, where the ones drawn off stand 1.08 to 1.37:1).
    fn fainter_on_its_line(on: u32, off: u32, line: u32) -> Option<String> {
        let side = |tint: u32| custom::luminance(tint).total_cmp(&custom::luminance(line));
        let (ratio, off_ratio) = (contrast(on, line), contrast(off, line));
        let crossed = side(off).is_ne() && side(on) != side(off);
        (ratio < off_ratio || crossed).then(|| {
            format!(
                "#{on:06x} {ratio:.3}:1 on the line #{line:06x}, {:?} it in luminance, off \
                 #{off:06x} {off_ratio:.3}:1, {:?}",
                side(on),
                side(off)
            )
        })
    }

    /// With More contrast on, no color reads worse on a changed word than it
    /// does with More contrast off and no changed word is marked more
    /// faintly, and the faintest are lifted from their lines, in every
    /// palette. On the added and removed word tints the editors draw, `text`,
    /// every syntax role but `removed` and the tint's diff color each read
    /// at least the contrast the same role reads on the tint drawn with it
    /// off; each tint stands from its line tint, in OKLab and in luminance
    /// contrast, at least as far as the tint drawn off stands from the line
    /// drawn off, and on the same side of it, the line moved or not, and its
    /// lightness gap from the line drawn off in a protanopia and
    /// deuteranopia model is no smaller than off. A tint
    /// drawn off at least [`WORD_LIGHTNESS_GAP`] from its line in OKLab
    /// lightness stays exactly; a nearer one stands at least that far from
    /// the line drawn off, or is the last step of its lift
    /// ([`lightness_lifts`]) before one on which some color would read worse
    /// than off, or before lightness runs out, or has no lift, lying on one
    /// side of its line in lightness and on the other in luminance. These
    /// hold for every word drawn off beyond its line from the canvas
    /// ([`lightness_beyond`]); a word drawn off on its line's canvas side
    /// steps across the line instead, and its place is read in
    /// `more_contrast_keeps_the_highlight_rules_in_every_palette`. Re-fitted
    /// at 4.75:1
    /// against syntax raised to 7:1, the added word tint grew heavier, and
    /// the added text on it read 5.81:1 where it read 6.45:1 (Midnight);
    /// held only to the readings, it drew nearer its line (Midnight's added
    /// words 0.077 to 0.041 from it, removed 0.094 to 0.056); held to the
    /// distance alone, it could cross to the line's other side (Midnight's
    /// added words #0f1d19, darker than their line #19322d, where the tint
    /// drawn off is lighter); lifted toward the canvas, Omarchy
    /// flexoki-light's added word reached #f7ffdf, 1.005:1 against its
    /// canvas, and held at half its line's gap from the canvas, 31 words
    /// stayed on their line's canvas side.
    #[gpui::test]
    fn more_contrast_never_reads_worse_on_a_changed_word(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        const ROLES: [&str; 9] = [
            "keyword", "string", "constant", "type", "function", "markup", "comment", "text",
            "diff",
        ];
        let mut failures = Vec::new();
        let (mut lifted, mut bound, mut level, mut edged, mut kept) = (0, 0, 0, 0, 0);
        let mut crossed = 0;
        for (name, chosen) in more_contrast_palettes() {
            let [off, on] = [false, true].map(|more| {
                cx.update(|cx| {
                    set_more_contrast(more, cx);
                    chosen.apply(chosen.is_light(), None, cx);
                    let applied = palette(cx);
                    (
                        applied,
                        editor_highlights(cx),
                        applied.syntax_roles(more.then_some(chosen)),
                    )
                })
            });
            let ((before, off_tints, off_roles), (after, on_tints, on_roles)) = (off, on);
            // The syntax the lift is judged under: raised on the line tints
            // as chosen, before any line moves.
            let lift_roles = chosen.with_raised_floors().syntax_roles(Some(chosen));
            for (tint, diff, (off_line, on_line), off_tint, on_tint) in [
                (
                    "added",
                    after.added,
                    (before.added_background, after.added_background),
                    off_tints.added_word,
                    on_tints.added_word,
                ),
                (
                    "removed",
                    after.removed,
                    (before.removed_background, after.removed_background),
                    off_tints.removed_word,
                    on_tints.removed_word,
                ),
            ] {
                // A word drawn off on its line's canvas side steps across the
                // line instead ([`EditorHighlights::crossed_word_tint`]).
                if lightness_beyond(off_tint, off_line, chosen) <= 0. {
                    crossed += 1;
                    continue;
                }
                let colors = |roles: SyntaxRoles| on_a_word(roles, diff);
                for ((role, was), is) in ROLES.iter().zip(colors(off_roles)).zip(colors(on_roles)) {
                    let (off_ratio, on_ratio) = (contrast(was, off_tint), contrast(is, on_tint));
                    if on_ratio < off_ratio {
                        failures.push(format!(
                            "{name} {role} on the {tint} word #{on_tint:06x} {on_ratio:.3}:1, \
                             off #{off_tint:06x} {off_ratio:.3}:1"
                        ));
                    }
                }
                let gap = |tint: u32| lightness_gap(tint, off_line).abs();
                if gap(off_tint) >= WORD_LIGHTNESS_GAP {
                    if on_tint != off_tint {
                        failures.push(format!(
                            "{name} {tint} word #{on_tint:06x}, off #{off_tint:06x} {:.4} from \
                             its line in lightness",
                            gap(off_tint)
                        ));
                    }
                    kept += 1;
                } else if gap(on_tint) >= WORD_LIGHTNESS_GAP {
                    lifted += 1;
                } else {
                    // Short of the gap, the next step of the lift would read
                    // some color worse than off; or the tint has no lift: it lies on one
                    // side of its line in lightness and on the other in
                    // luminance, so any step away in lightness would near the
                    // line in luminance or cross it; or the lift ran out of
                    // lightness, white or black on its side standing nearer
                    // the line than the gap.
                    let path = lightness_lifts(off_tint, off_line);
                    let next = match path.iter().position(|&lift| lift == on_tint) {
                        Some(at) => path.get(at + 1),
                        None => path.first().filter(|_| on_tint == off_tint),
                    };
                    let disagrees = lightness_gap(off_tint, off_line).signum()
                        != (luminance(off_tint) - luminance(off_line)).signum();
                    let edge = if lightness_gap(off_tint, off_line) > 0. {
                        0xffffff
                    } else {
                        0x000000
                    };
                    let ran_out = path.last() == Some(&on_tint) && gap(edge) < WORD_LIGHTNESS_GAP;
                    match next {
                        Some(&next)
                            if reads_worse_on_a_word(
                                (lift_roles, next),
                                (off_roles, off_tint),
                                diff,
                            )
                            .is_some() =>
                        {
                            bound += 1;
                        }
                        None if path.is_empty() && on_tint == off_tint && disagrees => {
                            level += 1;
                        }
                        None if ran_out => edged += 1,
                        _ => failures.push(format!(
                            "{name} {tint} word #{on_tint:06x} {:.4} from its line #{off_line:06x} \
                             in lightness, off #{off_tint:06x} {:.4}, short of the gap, not \
                             where the text binds and not at the end of lightness (next step \
                             {next:x?})",
                            gap(on_tint),
                            gap(off_tint)
                        )),
                    }
                }
                let apart = |tint: u32, line: u32| oklab_distance(oklab(tint), oklab(line));
                if apart(on_tint, on_line) < apart(off_tint, off_line) {
                    failures.push(format!(
                        "{name} {tint} word #{on_tint:06x} {:.3} from its line #{on_line:06x}, \
                         off #{off_tint:06x} {:.3} from #{off_line:06x}",
                        apart(on_tint, on_line),
                        apart(off_tint, off_line)
                    ));
                }
                let side = |tint: u32, line: u32| oklab(tint)[0].total_cmp(&oklab(line)[0]);
                if side(on_tint, on_line) != side(off_tint, off_line) {
                    failures.push(format!(
                        "{name} {tint} word #{on_tint:06x} {:?} its line #{on_line:06x} in \
                         lightness, off #{off_tint:06x} {:?} #{off_line:06x}",
                        side(on_tint, on_line),
                        side(off_tint, off_line)
                    ));
                }
                let luminance_side =
                    |tint: u32, line: u32| luminance(tint).total_cmp(&luminance(line));
                let off_side = luminance_side(off_tint, off_line);
                if contrast(on_tint, on_line) < contrast(off_tint, off_line)
                    || (off_side.is_ne() && luminance_side(on_tint, on_line) != off_side)
                {
                    failures.push(format!(
                        "{name} {tint} word #{on_tint:06x} {:.3}:1 on its line #{on_line:06x}, \
                         {:?} it in luminance, off #{off_tint:06x} {:.3}:1 on #{off_line:06x}, \
                         {off_side:?}",
                        contrast(on_tint, on_line),
                        luminance_side(on_tint, on_line),
                        contrast(off_tint, off_line)
                    ));
                }
                // The lift's own gap in colour-blind vision, from the line
                // drawn off; a moved line is held to normal vision.
                let seen = colour_blind_gaps(on_tint, off_line)
                    .into_iter()
                    .zip(colour_blind_gaps(off_tint, off_line));
                for (vision, (on, off)) in ["protanopia", "deuteranopia"].into_iter().zip(seen) {
                    if on.abs() < off.abs() {
                        failures.push(format!(
                            "{name} {tint} word #{on_tint:06x} {on:+.4} from its line in \
                             {vision}, off #{off_tint:06x} {off:+.4}"
                        ));
                    }
                }
            }
        }
        cx.update(|cx| set_more_contrast(false, cx));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        // Every case a palette reaches is reached: tints kept, stopped where
        // the text binds, with no lift, and stepped across their lines. No
        // palette's word drawn off beyond its line lifts to the gap or to
        // the end of lightness (every lift that reached the gap went toward
        // the canvas); `a_faint_word_tint_lifts_from_its_line_in_lightness`
        // and `a_word_tint_lift_stops_where_lightness_runs_out` read those.
        assert!(
            kept > 0 && bound > 0 && level > 0 && crossed > 0,
            "word tints kept {kept}, lifted {lifted}, at the text's bound {bound}, with no lift \
             {level}, at the end of lightness {edged}, stepped across {crossed}"
        );
    }

    /// Of the colors drawn on a diff side's line and word tints under More
    /// contrast, `after` with `on_roles`, the first that reads below the same
    /// color drawn with More contrast off, `before` with `off_roles`, as a
    /// message: on the line `text`, every syntax role, the side's diff color,
    /// secondary text and line numbers; on the word `text`, every role but
    /// `removed` and the diff color.
    fn diff_side_reads_worse(
        (after, on_roles, on_line, on_word): (Palette, SyntaxRoles, u32, u32),
        (before, off_roles, off_line, off_word): (Palette, SyntaxRoles, u32, u32),
        diff: u32,
    ) -> Option<String> {
        let on_line_colors = |palette: Palette, roles: SyntaxRoles| {
            roles
                .colors()
                .into_iter()
                .chain([diff, palette.muted, palette.line_number])
        };
        let line = on_line_colors(after, on_roles)
            .zip(on_line_colors(before, off_roles))
            .map(|pair| (pair, (on_line, off_line)));
        let word = on_a_word(on_roles, diff)
            .zip(on_a_word(off_roles, diff))
            .map(|pair| (pair, (on_word, off_word)));
        line.chain(word)
            .find(|&((is, was), (on, off))| contrast(is, on) < contrast(was, off))
            .map(|((is, was), (on, off))| {
                format!(
                    "#{is:06x} {:.3}:1 on #{on:06x}, off #{was:06x} {:.3}:1 on #{off:06x}",
                    contrast(is, on),
                    contrast(was, off)
                )
            })
    }

    /// Why `rule` is not the rule a changed word whose tint is `word` draws
    /// for the diff colour `diff` in a light or dark palette
    /// ([`EditorHighlights::word_rule`]), or `None` where it is: it reads
    /// [`MORE_CONTRAST_EDGE`] on the tint, it is `diff` where `diff` already
    /// reads so, and otherwise it is one of 256 even steps from `diff`
    /// toward black in a light palette or white in a dark one, the step
    /// before it reading under 3:1.
    fn word_rule_misses(light: bool, diff: u32, word: u32, rule: u32) -> Option<String> {
        let (ratio, diff_ratio) = (contrast(rule, word), contrast(diff, word));
        if ratio < MORE_CONTRAST_EDGE {
            return Some(format!(
                "#{rule:06x} reads {ratio:.3}:1 on its word #{word:06x}"
            ));
        }
        if diff_ratio >= MORE_CONTRAST_EDGE {
            return (rule != diff).then(|| {
                format!(
                    "#{rule:06x} leaves its diff colour #{diff:06x}, which reads \
                     {diff_ratio:.3}:1 on its word #{word:06x}"
                )
            });
        }
        let extreme = if light { 0x000000 } else { 0xffffff };
        let step = |k: u32| {
            [16, 8, 0].into_iter().fold(0, |color, shift| {
                let (from, to) = ((diff >> shift) & 0xff, (extreme >> shift) & 0xff);
                color | (((from * (256 - k) + to * k) / 256) << shift)
            })
        };
        let first = (1..=256)
            .find(|&k| step(k) == rule)
            .is_some_and(|k| contrast(step(k - 1), word) < MORE_CONTRAST_EDGE);
        (!first).then(|| {
            format!(
                "#{rule:06x} is not the first step from #{diff:06x} toward #{extreme:06x} that \
                 reads 3:1 on its word #{word:06x}"
            )
        })
    }

    /// Under More contrast every changed word, in every palette and on both
    /// sides, draws a rule along its highlight box's bottom edge
    /// ([`EditorHighlights::word_rule`]) that reads [`MORE_CONTRAST_EDGE`]
    /// on the word's tint as applied, its side's diff colour where that
    /// already reads so ([`word_rule_misses`]). Off, no word draws one, so the
    /// decorations are those drawn before the rule existed. Every shipped
    /// palette's diff colours read 3:1 on their word tints, which are fitted
    /// for them as text (the least, Kanagawa Wave's removed `#ff5d62` on
    /// `#5c0c1c`, 4.59:1, measured on 2026-11-05), so each draws its diff
    /// colour; `a_word_rule_steps_from_a_diff_colour_under_three_to_one`
    /// reads the step where one would not.
    #[gpui::test]
    fn more_contrast_rules_every_changed_word_at_three_to_one(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut failures = Vec::new();
        for (name, chosen) in more_contrast_palettes() {
            let off = cx.update(|cx| {
                set_more_contrast(false, cx);
                chosen.apply(chosen.is_light(), None, cx);
                editor_highlights(cx)
            });
            if off.added_rule.is_some() || off.removed_rule.is_some() {
                failures.push(format!(
                    "{name} rules a changed word with More contrast off"
                ));
            }
            let (applied, on) = cx.update(|cx| {
                set_more_contrast(true, cx);
                chosen.apply(chosen.is_light(), None, cx);
                (palette(cx), editor_highlights(cx))
            });
            for (side, diff, word, rule) in [
                ("added", applied.added, on.added_word, on.added_rule),
                ("removed", applied.removed, on.removed_word, on.removed_rule),
            ] {
                match rule {
                    None => failures.push(format!("{name}'s {side} word draws no rule")),
                    Some(rule) => failures.extend(
                        word_rule_misses(applied.is_light(), diff, word, rule)
                            .map(|miss| format!("{name}'s {side} rule {miss}")),
                    ),
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A diff colour that reads under 3:1 on its word's tint gives the rule
    /// the first of 256 steps from it toward white in a dark palette or black
    /// in a light one that reads 3:1 there, and one that reads 3:1 is kept.
    /// No shipped palette has such a pair, so these are made: Midnight's
    /// removed word tint under a dim rose, and Daylight's added word tint
    /// under a pale green.
    #[test]
    fn a_word_rule_steps_from_a_diff_colour_under_three_to_one() {
        for (choice, diff, word) in [
            (ThemeChoice::Midnight, 0x8a4a5c, 0x5f3241),
            (ThemeChoice::Midnight, 0xff95a8, 0x5f3241),
            (ThemeChoice::Daylight, 0x9fd8a8, 0xd6f5da),
            (ThemeChoice::Daylight, 0x1a7f37, 0xd6f5da),
        ] {
            let palette = choice.palette();
            let rule = EditorHighlights::word_rule(palette, diff, word);
            assert_eq!(
                word_rule_misses(palette.is_light(), diff, word, rule),
                None,
                "{choice:?}: #{diff:06x} on #{word:06x}"
            );
            assert_eq!(
                rule == diff,
                contrast(diff, word) >= MORE_CONTRAST_EDGE,
                "{choice:?}: #{diff:06x} on #{word:06x} moves only where it misses 3:1"
            );
        }
    }

    /// Under More contrast a changed word that cannot stand
    /// [`WORD_LIGHTNESS_GAP`] from its line in OKLab lightness moves its line
    /// instead, in every palette ([`Palette::more_contrast_fit`]). A side
    /// whose word is drawn off on its line's canvas side keeps its line, and
    /// its word steps across it (read in
    /// `more_contrast_keeps_the_highlight_rules_in_every_palette`). On every
    /// other side the line moves for the word's lift
    /// ([`EditorHighlights::lifted_word_tint`]), which is the word drawn, or
    /// where it does not stand beyond the moved line, steps across it too. A
    /// side whose lift stands that far from the line drawn off keeps that
    /// line. Every
    /// other side stands that far from its moved line, or has its line at a
    /// bound: the next step of its move ([`line_moves`]) would stand nearer
    /// the selection or Find's matches drawn off than they allow, narrow the
    /// word's lightness gap in a protanopia or deuteranopia model, or read
    /// some color on the diff's lines or words worse than off; or the move
    /// ended at the line's 0.04 from the canvas, or has no step, the line
    /// standing within 0.04 of the canvas on the side it would move toward,
    /// or the word lying on one side of it in lightness and the other in
    /// luminance. On every side every color drawn on line and the word the
    /// line moves for reads at least as off (the word's own place is read in
    /// `more_contrast_never_reads_worse_on_a_changed_word`). A moved line
    /// stands 0.04 from the canvas in lightness, or no nearer than off, the
    /// selection and Find's current match drawn off stand 0.04 from it, or as
    /// far as from the line drawn off, the word stands from it in a
    /// protanopia and a deuteranopia model at least as far as from the line
    /// drawn off, and secondary text and line numbers read 7:1 on it, or as
    /// `text` does.
    #[gpui::test]
    fn more_contrast_moves_a_line_its_word_cannot_stand_apart_from(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut failures = Vec::new();
        let (mut full, mut floored, mut within, mut text_bound, mut apart_bound, mut level) =
            (0, 0, 0, 0, 0, 0);
        let (mut colour_blind_bound, mut kept_by_crossing) = (0, 0);
        for (name, chosen) in more_contrast_palettes() {
            let [off, on] = [false, true].map(|more| {
                cx.update(|cx| {
                    set_more_contrast(more, cx);
                    chosen.apply(chosen.is_light(), None, cx);
                    let applied = palette(cx);
                    (
                        applied,
                        editor_highlights(cx),
                        applied.syntax_roles(more.then_some(chosen)),
                    )
                })
            });
            let ((before, off_tints, off_roles), (after, on_tints, on_roles)) = (off, on);
            let canvas = oklab(chosen.canvas)[0];
            let from_canvas = |color: u32| (oklab(color)[0] - canvas).abs();
            let off_words = [off_tints.added_word, off_tints.removed_word];
            let on_words = [on_tints.added_word, on_tints.removed_word];
            let sides = [
                (
                    "added",
                    chosen.added,
                    (before.added_background, after.added_background),
                ),
                (
                    "removed",
                    chosen.removed,
                    (before.removed_background, after.removed_background),
                ),
            ];
            let fit_words = words_lines_move_for(chosen);
            let crossing = [0, 1]
                .map(|index| lightness_beyond(off_words[index], sides[index].2.0, chosen) <= 0.);
            let gap = |word: u32, line: u32| lightness_gap(word, line).abs();
            let drawn = |palette: Palette, roles: SyntaxRoles, line: u32, word: u32| {
                (palette, roles, line, word)
            };
            // The bound a step of side `index`'s move fails, if any: the
            // highlights' distance, the word's colour-blind gaps or a color
            // on the diff's lines and words reading worse than off, measured
            // as the fit measures it, the added line moving beside the
            // removed line drawn off.
            let failing_bound = |index: usize, step: u32| -> Option<&'static str> {
                let off_line = sides[index].2.0;
                let off_line_lab = oklab(off_line);
                let nearer = [off_tints.selection, off_tints.find_current]
                    .into_iter()
                    .any(|highlight| {
                        let highlight = oklab(highlight);
                        oklab_distance(highlight, oklab(step))
                            < HIGHLIGHT_DISTANCE.min(oklab_distance(highlight, off_line_lab))
                    });
                let narrows = colour_blind_gaps(fit_words[index], step)
                    .into_iter()
                    .zip(colour_blind_gaps(fit_words[index], off_line))
                    .any(|(moved, off)| moved.abs() < off.abs());
                let mut lines = [after.added_background, after.removed_background];
                if index == 0 {
                    lines[1] = before.removed_background;
                }
                lines[index] = step;
                let candidate = Palette {
                    added_background: lines[0],
                    removed_background: lines[1],
                    ..chosen
                }
                .with_raised_floors();
                let roles = candidate.syntax_roles(Some(chosen));
                let worse = || {
                    sides.iter().enumerate().any(|(at, &(_, diff, (off, _)))| {
                        diff_side_reads_worse(
                            drawn(candidate, roles, lines[at], fit_words[at]),
                            drawn(before, off_roles, off, off_words[at]),
                            diff,
                        )
                        .is_some()
                    })
                };
                if nearer {
                    Some("highlights")
                } else if narrows {
                    Some("colour-blind")
                } else if worse() {
                    Some("text")
                } else {
                    None
                }
            };
            for (index, &(side, diff, (off_line, on_line))) in sides.iter().enumerate() {
                let (off_word, on_word) = (off_words[index], fit_words[index]);
                if let Some(worse) = diff_side_reads_worse(
                    drawn(after, on_roles, on_line, on_word),
                    drawn(before, off_roles, off_line, off_word),
                    diff,
                ) {
                    failures.push(format!("{name} {side}: {worse}"));
                }
                if crossing[index] {
                    if on_line != off_line {
                        failures.push(format!(
                            "{name} {side} line #{off_line:06x} moved to #{on_line:06x} for a \
                             word on its canvas side #{off_word:06x}"
                        ));
                    }
                    kept_by_crossing += 1;
                    continue;
                }
                // The word drawn is the lift, or steps across a line it does
                // not stand beyond.
                let drawn_word = on_words[index];
                if drawn_word != on_word && stands_beyond(on_word, on_line, chosen) {
                    failures.push(format!(
                        "{name} {side} word #{drawn_word:06x}, its lift #{on_word:06x} stands \
                         beyond its line #{on_line:06x}"
                    ));
                }
                if gap(on_word, off_line) >= WORD_LIGHTNESS_GAP && on_line != off_line {
                    failures.push(format!(
                        "{name} {side} line #{off_line:06x} moved to #{on_line:06x}, its word \
                         #{on_word:06x} {:.4} from it",
                        gap(on_word, off_line)
                    ));
                }
                if on_line != off_line {
                    if from_canvas(on_line) < WORD_LIGHTNESS_GAP.min(from_canvas(off_line)) {
                        failures.push(format!(
                            "{name} {side} line #{on_line:06x} {:.4} from the canvas, off \
                             #{off_line:06x} {:.4}",
                            from_canvas(on_line),
                            from_canvas(off_line)
                        ));
                    }
                    for (highlight, color) in [
                        ("selection", off_tints.selection),
                        ("current Find match", off_tints.find_current),
                    ] {
                        let apart = |line: u32| oklab_distance(oklab(color), oklab(line));
                        if apart(on_line) < HIGHLIGHT_DISTANCE.min(apart(off_line)) {
                            failures.push(format!(
                                "{name} {side} line #{on_line:06x} {:.4} from the {highlight} \
                                 drawn off #{color:06x}, off #{off_line:06x} {:.4}",
                                apart(on_line),
                                apart(off_line)
                            ));
                        }
                    }
                    let seen = colour_blind_gaps(on_word, on_line)
                        .into_iter()
                        .zip(colour_blind_gaps(on_word, off_line));
                    for (vision, (on, off)) in ["protanopia", "deuteranopia"].into_iter().zip(seen)
                    {
                        if on.abs() < off.abs() {
                            failures.push(format!(
                                "{name} {side} word #{on_word:06x} {on:+.4} from its moved line \
                                 #{on_line:06x} in {vision}, {off:+.4} from #{off_line:06x}"
                            ));
                        }
                    }
                    let floor = MORE_CONTRAST_TEXT.min(contrast(after.text, on_line));
                    for (token, color) in
                        [("muted", after.muted), ("line_number", after.line_number)]
                    {
                        if contrast(color, on_line) < floor {
                            failures.push(format!(
                                "{name} {token} #{color:06x} {:.3}:1 on the moved {side} line \
                                 #{on_line:06x}, needs {floor:.3}",
                                contrast(color, on_line)
                            ));
                        }
                    }
                }
                let moves = line_moves(off_line, on_word, (off_word, off_line), chosen.canvas);
                // No step up to the line drawn fails a bound: the move never
                // passes a step that would, even where a later one would not.
                if let Some(at) = moves.iter().position(|&line| line == on_line) {
                    for &step in &moves[..=at] {
                        if let Some(bound) = failing_bound(index, step) {
                            failures.push(format!(
                                "{name} {side} line #{on_line:06x} passed #{step:06x}, which \
                                 fails the {bound} bound"
                            ));
                        }
                    }
                }
                if gap(on_word, on_line) >= WORD_LIGHTNESS_GAP {
                    full += usize::from(on_line != off_line);
                    continue;
                }
                let next = match moves.iter().position(|&line| line == on_line) {
                    Some(at) => moves.get(at + 1).copied(),
                    None if on_line == off_line => moves.first().copied(),
                    None => {
                        failures.push(format!(
                            "{name} {side} line #{on_line:06x} is no step of its move {moves:x?}"
                        ));
                        continue;
                    }
                };
                let disagrees = lightness_gap(on_word, off_line).signum()
                    != (luminance(on_word) - luminance(off_line)).signum();
                let toward_canvas = (oklab(off_line)[0] - oklab(on_word)[0]).signum()
                    == (canvas - oklab(off_line)[0]).signum();
                match next {
                    Some(next) => match failing_bound(index, next) {
                        Some("highlights") => apart_bound += 1,
                        Some("colour-blind") => colour_blind_bound += 1,
                        Some(_) => text_bound += 1,
                        None => failures.push(format!(
                            "{name} {side} line #{on_line:06x} stopped short of #{next:06x}, its \
                             word #{on_word:06x} {:.4} from it, where every bound holds",
                            gap(on_word, on_line)
                        )),
                    },
                    None if moves.is_empty() && disagrees => level += 1,
                    None if moves.is_empty()
                        && toward_canvas
                        && from_canvas(off_line) < WORD_LIGHTNESS_GAP =>
                    {
                        within += 1;
                    }
                    // The move ended at the canvas floor: the line stands
                    // within a step and its rounding of 0.04 from the canvas.
                    None if toward_canvas && from_canvas(on_line) < WORD_LIGHTNESS_GAP + 0.004 => {
                        floored += 1;
                    }
                    None => failures.push(format!(
                        "{name} {side} line #{on_line:06x}, its word #{on_word:06x} {:.4} from \
                         it, ended its move {moves:x?} at no bound",
                        gap(on_word, on_line)
                    )),
                }
            }
        }
        cx.update(|cx| set_more_contrast(false, cx));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        // Each bound is reached by a built-in palette but the selection's,
        // which the Omarchy palettes and a synthetic case reach
        // (`a_moved_line_stops_at_its_bounds`), and the text's: a line moves
        // away from its word, toward the canvas, where its text reads better,
        // and the text stopped only lines that would move toward it, for
        // words on their canvas side, which keep their lines now. The
        // colour-blind gaps stop One Dark's and Dracula's removed lines.
        assert!(
            full > 0
                && floored > 0
                && within > 0
                && colour_blind_bound > 0
                && level > 0
                && kept_by_crossing > 0,
            "lines moved to the gap {full}, at the canvas floor {floored}, within it {within}, \
             at the text's bound {text_bound}, the highlights' {apart_bound}, the colour-blind \
             gaps' {colour_blind_bound}, with no move {level}, kept for a word that steps \
             across {kept_by_crossing}"
        );
    }

    /// The diff tints [`Palette::more_contrast_fit`] decides for a palette,
    /// with the editor highlights drawn off: the adjusted palette, the word
    /// tints under More contrast and those drawn off, added then removed.
    fn diff_tints(chosen: Palette) -> (Palette, [u32; 2], [u32; 2]) {
        let (adjusted, fit) = chosen.more_contrast_fit();
        let off = EditorHighlights::fit(chosen, chosen.syntax_roles(None), None);
        (adjusted, fit.words, [off.added_word, off.removed_word])
    }

    /// The word tints [`Palette::more_contrast_fit`] moves `chosen`'s line
    /// tints for, added then removed: each drawn with More contrast off
    /// beyond its line from the canvas lifted
    /// ([`EditorHighlights::lifted_word_tint`]), and one on its line's
    /// canvas side as drawn off, since it steps across its line afterward.
    fn words_lines_move_for(chosen: Palette) -> [u32; 2] {
        let (_, _, off) = diff_tints(chosen);
        let roles = chosen.with_raised_floors().syntax_roles(Some(chosen));
        let own = chosen.syntax_roles(None);
        [
            (chosen.added, chosen.added_background),
            (chosen.removed, chosen.removed_background),
        ]
        .into_iter()
        .zip(off)
        .map(|((diff, line), off)| {
            if lightness_beyond(off, line, chosen) > 0. {
                EditorHighlights::lifted_word_tint(roles, own, off, diff, line)
            } else {
                off
            }
        })
        .collect::<Vec<_>>()
        .try_into()
        .expect("two sides")
    }

    /// Whether the text rule keeps a word that steps across `line` short of
    /// [`HIGHLIGHT_LIGHTNESS_GAP`] beyond it: in each hue
    /// [`EditorHighlights::crossed_word_tint`] steps in, the seed's of `diff`
    /// on `line`, halfway to `diff`'s and `diff`'s own, the first step that
    /// stands [`HIGHLIGHT_DISTANCE`] from the line and the gap beyond it
    /// reads some color in `colors` under the text rule, or there is none.
    fn the_gap_cannot_read(colors: &[u32], diff: u32, line: u32, chosen: Palette) -> bool {
        let seed = EditorHighlights::word_seed(diff, line);
        [seed, mix(seed, diff, 128), diff].into_iter().all(|hue| {
            beyond_line(hue, line, chosen)
                .into_iter()
                .find(|&step| {
                    lightness_beyond(step, line, chosen) >= HIGHLIGHT_LIGHTNESS_GAP
                        && oklab_distance(oklab(step), oklab(line)) >= HIGHLIGHT_DISTANCE
                })
                .is_none_or(|step| {
                    colors
                        .iter()
                        .any(|&color| contrast(color, step) < LABEL_RULE)
                })
        })
    }

    /// A color's OKLab lightness gap from `palette`'s canvas.
    fn from_canvas(palette: Palette, color: u32) -> f64 {
        (oklab(color)[0] - oklab(palette.canvas)[0]).abs()
    }

    /// Under More contrast a word tint drawn off on its line's canvas side
    /// never lifts toward the canvas: its line keeps its tint and the word
    /// steps across it ([`EditorHighlights::crossed_word_tint`]), in
    /// built-in palettes. It stands beyond the line from the canvas, at least
    /// [`HIGHLIGHT_DISTANCE`] from it and [`HIGHLIGHT_LIGHTNESS_GAP`] beyond
    /// it in lightness, and every color drawn on it as text reads the text
    /// rule; One Dark's added word stops short of the gap, beyond its line,
    /// since at the gap its raised keyword would read 4.39:1, under the text
    /// rule (Kanagawa Lotus's removed word stopped short too, its strings
    /// 4.49:1 at the gap, until its text darkened on 2026-11-05). It is the
    /// furthest such step in its row of steps ([`beyond_line`]): the next
    /// step in the row reads some color below the rule the word keeps, or
    /// the row ends there (Solarized Dark's added `#214514`, One Dark's added
    /// `#2f491d`, Kanagawa Lotus's removed `#f7bea6`, where it drew `#ffbdb4`
    /// 0.018 beyond its line). One Dark's added word, drawn off darker than its line, once
    /// lifted toward the canvas and stopped at half its line's gap from it
    /// (#253b23), still on the canvas side; Rosé Pine Dawn's removed word,
    /// drawn off 0.001 from its canvas, kept the tint drawn off; Braden's
    /// removed word (#fcf6f7 on #f9e5eb), lighter than its canvas, lifted
    /// away from it, past the canvas from its line. Solarized Dark's added
    /// word, held to the distance from its line alone, stepped across
    /// through its seed's chroma to #2d3500, 0.0004 beyond its line in
    /// lightness.
    #[test]
    fn a_word_on_its_lines_canvas_side_steps_across_it() {
        for (case, theme, side, at_gap, pinned) in [
            (
                "One Dark added",
                ThemeChoice::OneDark,
                0,
                false,
                Some(0x2f491d),
            ),
            (
                "Rosé Pine Dawn removed",
                ThemeChoice::RosePineDawn,
                1,
                true,
                None,
            ),
            ("Braden removed", ThemeChoice::Daylight, 1, true, None),
            (
                "Solarized Dark added",
                ThemeChoice::SolarizedDark,
                0,
                true,
                Some(0x214514),
            ),
            (
                "Kanagawa Lotus removed",
                ThemeChoice::KanagawaLotus,
                1,
                true,
                Some(0xf7bea6),
            ),
        ] {
            let chosen = theme.palette();
            let (adjusted, words, off) = diff_tints(chosen);
            let roles = adjusted.syntax_roles(Some(chosen));
            let ((diff, line, moved), patch) = if side == 0 {
                (
                    (
                        chosen.added,
                        chosen.added_background,
                        adjusted.added_background,
                    ),
                    roles.string,
                )
            } else {
                (
                    (
                        chosen.removed,
                        chosen.removed_background,
                        adjusted.removed_background,
                    ),
                    roles.removed,
                )
            };
            let word = words[side];
            assert!(
                lightness_beyond(off[side], line, chosen) < 0.,
                "{case}: #{:06x} drawn off on its line's canvas side",
                off[side]
            );
            assert_eq!(moved, line, "{case}: the line keeps its tint");
            assert_eq!(
                word,
                EditorHighlights::crossed_word_tint(
                    roles,
                    [diff, patch],
                    (line, off[side]),
                    chosen
                ),
                "{case}: the word steps across"
            );
            let beyond = lightness_beyond(word, line, chosen);
            let apart = oklab_distance(oklab(word), oklab(line));
            let colors: Vec<u32> = roles.word_colors().into_iter().chain([patch]).collect();
            assert!(
                beyond > 0. && apart >= HIGHLIGHT_DISTANCE,
                "{case}: #{word:06x} {beyond:+.4} beyond its line #{line:06x}, {apart:.4} from it"
            );
            assert_eq!(
                beyond >= HIGHLIGHT_LIGHTNESS_GAP,
                at_gap,
                "{case}: #{word:06x} {beyond:+.4} beyond its line"
            );
            assert_eq!(
                the_gap_cannot_read(&colors, diff, line, chosen),
                !at_gap,
                "{case}: whether the text rule keeps the word short of the gap"
            );
            for &color in &colors {
                assert!(
                    contrast(color, word) >= LABEL_RULE,
                    "{case}: #{color:06x} {:.3}:1 on #{word:06x}",
                    contrast(color, word)
                );
            }
            if let Some(pinned) = pinned {
                assert_eq!(word, pinned, "{case}: #{word:06x}, not #{pinned:06x}");
            }
            // The furthest step: the next one in the word's row reads some
            // color below the rule the word keeps, or the row ends.
            let seed = EditorHighlights::word_seed(diff, line);
            let rows = [seed, mix(seed, diff, 128), diff].map(|hue| beyond_line(hue, line, chosen));
            let (row, at) = rows
                .iter()
                .find_map(|row| {
                    row.iter()
                        .position(|&step| step == word)
                        .map(|at| (row, at))
                })
                .unwrap_or_else(|| panic!("{case}: #{word:06x} is a step beyond its line"));
            let keeps = |step: u32, rule: f64| {
                colors
                    .iter()
                    .all(|&color| contrast(color, step) >= drawn_target(color, &[line], rule))
            };
            let rule = [MORE_CONTRAST_TEXT, LABEL_RULE + LABEL_MARGIN, LABEL_RULE]
                .into_iter()
                .find(|&rule| keeps(word, rule))
                .unwrap_or_else(|| panic!("{case}: #{word:06x} reads the text rule"));
            if let Some(&next) = row.get(at + 1) {
                assert!(
                    !keeps(next, rule),
                    "{case}: the next step #{next:06x} beyond #{word:06x} also reads {rule}:1"
                );
            }
        }
    }

    /// Under More contrast a word drawn off on its line's canvas side keeps
    /// its tint drawn off where no step beyond its line both reads the text
    /// rule and stands [`HIGHLIGHT_DISTANCE`] from it: a custom theme on
    /// Midnight whose added lines are pale, `#607372`, so the text on a word
    /// stepped across them reads under 4.5:1 by the step that stands apart.
    /// The nearest step that stood apart, `#588177`, broke the text floor.
    #[test]
    fn a_crossed_word_with_no_readable_step_keeps_its_tint_drawn_off() {
        let mut custom = CustomTheme::from_base(1, "Pale lines", ThemeChoice::Midnight);
        custom.palette.set(TokenKind::AddedBackground, 0x607372);
        let chosen = custom.palette;
        let (adjusted, words, off) = diff_tints(chosen);
        let roles = adjusted.syntax_roles(Some(chosen));
        let (diff, line) = (chosen.added, chosen.added_background);
        assert!(
            lightness_beyond(off[0], line, chosen) < 0.,
            "#{:06x} drawn off on its line's canvas side",
            off[0]
        );
        assert_eq!(adjusted.added_background, line, "the line keeps its tint");
        let colors: Vec<u32> = roles
            .word_colors()
            .into_iter()
            .chain([roles.string])
            .collect();
        // Along each row the word steps in, every step that stands apart
        // reads some color under the text rule.
        let seed = EditorHighlights::word_seed(diff, line);
        for hue in [seed, mix(seed, diff, 128), diff] {
            for step in beyond_line(hue, line, chosen) {
                assert!(
                    oklab_distance(oklab(step), oklab(line)) < HIGHLIGHT_DISTANCE
                        || colors
                            .iter()
                            .any(|&color| contrast(color, step) < LABEL_RULE),
                    "#{step:06x} reads and stands apart"
                );
            }
        }
        assert_eq!(words[0], off[0], "#{:06x}, not #{:06x}", words[0], off[0]);
        for &color in &colors {
            assert!(
                contrast(color, words[0]) >= LABEL_RULE,
                "#{color:06x} {:.3}:1 on #{:06x}",
                contrast(color, words[0]),
                words[0]
            );
        }
    }

    /// Under More contrast a word that cannot lift moves its line, in
    /// built-in palettes. Tokyo Night's added word lies between its line and
    /// the text, 0.013 from the line, so no step of a lift keeps the text;
    /// the line, 0.089 from the canvas, moves away from the word to the first
    /// step at the gap, its hue kept, and the word keeps the tint drawn off.
    /// The text reads better on it, and where a line tint binds secondary
    /// text and line numbers, as with a darker selected row, they are fitted
    /// on the moved line. Solarized Light's added line, 0.047 from its
    /// canvas, has less room than its word needs: it stops at the last step
    /// that stands 0.04 from the canvas, short of the gap.
    #[test]
    fn a_word_that_cannot_lift_moves_its_line_to_the_gap() {
        let gap = |word: u32, line: u32| lightness_gap(word, line).abs();
        let chosen = ThemeChoice::TokyoNight.palette();
        let (adjusted, words, off) = diff_tints(chosen);
        let (line, moved, word) = (chosen.added_background, adjusted.added_background, words[0]);
        assert_eq!(
            word, off[0],
            "Tokyo Night's added word keeps the tint drawn off"
        );
        assert!(gap(word, line) < 0.02);
        assert!(
            gap(word, moved) >= WORD_LIGHTNESS_GAP,
            "#{moved:06x} {:.4} from #{word:06x}",
            gap(word, moved)
        );
        let moves = line_moves(line, word, (off[0], line), chosen.canvas);
        assert_eq!(moves.last(), Some(&moved));
        assert!(
            moves[..moves.len() - 1]
                .iter()
                .all(|&step| gap(word, step) < WORD_LIGHTNESS_GAP)
        );
        assert!(from_canvas(chosen, moved) >= WORD_LIGHTNESS_GAP);
        assert!(hue_difference(moved, line) < 0.05);
        assert!(contrast(chosen.text, moved) > contrast(chosen.text, line));

        // Where a line tint binds secondary text and line numbers, they are
        // fitted on the moved line: with a selected row #1a2030 darker than
        // Tokyo Night's line tints, each is the least step toward `text` that
        // reads on the surfaces as adjusted, nearer its own token than the
        // step the lines drawn off need.
        let chosen = Palette {
            selected: 0x1a2030,
            ..ThemeChoice::TokyoNight.palette()
        };
        let adjusted = chosen.with_more_contrast();
        let lines = |palette: Palette| (palette.added_background, palette.removed_background);
        assert_ne!(lines(adjusted), lines(chosen), "the line tints move");
        let on_lines_drawn_off = chosen.with_raised_floors();
        let surfaces = more_contrast_text_surfaces(adjusted);
        let reads = |color: u32| {
            surfaces.iter().all(|&(_, surface, momentary)| {
                contrast(color, surface) >= more_contrast_text_floor(adjusted, surface, momentary)
            })
        };
        for (token, own, fitted, drawn_off) in [
            (
                "muted",
                chosen.muted,
                adjusted.muted,
                on_lines_drawn_off.muted,
            ),
            (
                "line_number",
                chosen.line_number,
                adjusted.line_number,
                on_lines_drawn_off.line_number,
            ),
        ] {
            assert_ne!(fitted, drawn_off, "{token} fitted on the lines drawn off");
            let step = (0..=256)
                .find(|&step| mix(own, chosen.text, step) == fitted)
                .expect("a step toward text");
            assert!(
                reads(fitted) && (step == 0 || !reads(mix(own, chosen.text, step - 1))),
                "{token} #{fitted:06x} is the least step that reads"
            );
        }

        let chosen = ThemeChoice::SolarizedLight.palette();
        let (adjusted, words, off) = diff_tints(chosen);
        let (line, moved, word) = (chosen.added_background, adjusted.added_background, words[0]);
        assert_ne!(moved, line, "Solarized Light's added line moves");
        assert!(gap(word, moved) < WORD_LIGHTNESS_GAP && gap(word, moved) > gap(word, line));
        let moves = line_moves(line, word, (off[0], line), chosen.canvas);
        assert_eq!(moves.last(), Some(&moved));
        let reach = from_canvas(chosen, moved);
        assert!(
            (WORD_LIGHTNESS_GAP..WORD_LIGHTNESS_GAP + 0.004).contains(&reach),
            "#{moved:06x} {reach:.4} from the canvas"
        );
    }

    /// Under More contrast a moved line stops at each of its bounds, in
    /// built-in palettes and one built from Tokyo Night. Nord's added line
    /// stands 0.025 from its canvas, within 0.04, so though its word stands
    /// 0.032 from it between the line and the text, the line keeps its tint.
    /// One Dark's added word, drawn off on the canvas side of its line,
    /// steps across it, and its line keeps its tint: moved away from the
    /// word, the line would step toward the text, and its first step reads
    /// `text` worse than the line drawn off. Tokyo Night with a selected row #002c28 near its
    /// added line's path stops the line where the next step would stand
    /// nearer the selection than 0.04, short of the gap it reaches with its
    /// own selection.
    #[test]
    fn a_moved_line_stops_at_its_bounds() {
        let gap = |word: u32, line: u32| lightness_gap(word, line).abs();
        let chosen = ThemeChoice::Nord.palette();
        let (adjusted, words, off) = diff_tints(chosen);
        let line = chosen.added_background;
        assert!(from_canvas(chosen, line) < WORD_LIGHTNESS_GAP);
        assert!(gap(words[0], line) < WORD_LIGHTNESS_GAP);
        assert!(line_moves(line, words[0], (off[0], line), chosen.canvas).is_empty());
        assert_eq!(adjusted.added_background, line, "Nord's added line");

        let chosen = ThemeChoice::OneDark.palette();
        let (adjusted, words, off) = diff_tints(chosen);
        let line = chosen.added_background;
        assert!(
            lightness_beyond(off[0], line, chosen) < 0. && gap(off[0], line) < WORD_LIGHTNESS_GAP
        );
        let moves = line_moves(line, off[0], (off[0], line), chosen.canvas);
        assert!(contrast(chosen.text, moves[0]) < contrast(chosen.text, line));
        assert_eq!(adjusted.added_background, line, "One Dark's added line");
        assert!(
            stands_beyond(words[0], line, chosen),
            "One Dark's added word steps across"
        );

        let tokyo = ThemeChoice::TokyoNight.palette();
        let chosen = Palette {
            selected: 0x002c28,
            ..tokyo
        };
        let highlights = EditorHighlights::fit(chosen, chosen.syntax_roles(None), None);
        assert_eq!(highlights.selection, 0x002c28);
        let (adjusted, words, off) = diff_tints(chosen);
        let (line, moved) = (chosen.added_background, adjusted.added_background);
        let apart = |color: u32| oklab_distance(oklab(highlights.selection), oklab(color));
        assert!(apart(line) >= HIGHLIGHT_DISTANCE && apart(moved) >= HIGHLIGHT_DISTANCE);
        assert!(gap(words[0], moved) < WORD_LIGHTNESS_GAP);
        assert_ne!(moved, line, "the line moves up to the selection's bound");
        assert_eq!(tokyo.with_more_contrast().added_background, 0x1f2f25);
        let moves = line_moves(line, words[0], (off[0], line), chosen.canvas);
        let next = moves[moves.iter().position(|&step| step == moved).unwrap() + 1];
        assert!(
            apart(next) < HIGHLIGHT_DISTANCE,
            "#{next:06x} {:.4}",
            apart(next)
        );
    }

    /// A line's move stops at the step before the first that fails a bound,
    /// never passing it for a later step that holds every bound, in a palette
    /// built from Midnight (the faint custom palette). Its removed word reads
    /// lighter than its line in normal vision and darker in a protanopia
    /// model, so the first steps of the line's move narrow that gap, then
    /// cross it, and the steps past the crossing widen it again and hold
    /// every bound, the last at the word's 0.04. The line keeps the tint drawn
    /// off; kept to the steps that hold, it moved to #32202b.
    #[test]
    fn a_moved_line_never_passes_a_step_that_fails_a_bound() {
        let chosen = faint_custom_palette();
        let (adjusted, _, off) = diff_tints(chosen);
        let words = words_lines_move_for(chosen);
        let highlights = EditorHighlights::fit(chosen, chosen.syntax_roles(None), None);
        let (line, word) = (chosen.removed_background, words[1]);
        let holds = |step: u32| {
            let candidate = Palette {
                added_background: adjusted.added_background,
                removed_background: step,
                ..chosen
            }
            .with_raised_floors();
            highlights.stay_apart_from(step, line)
                && keeps_colour_blind_gaps(word, step, line)
                && chosen.diff_reads_as_off(chosen.syntax_roles(None), candidate, off, words)
        };
        let moves = line_moves(line, word, (off[1], line), chosen.canvas);
        assert!(
            !keeps_colour_blind_gaps(word, moves[0], line),
            "the first step narrows the word's gap in protanopia"
        );
        let past = moves.iter().copied().rev().find(|&step| holds(step));
        assert!(
            past.is_some_and(|step| lightness_gap(word, step).abs() >= WORD_LIGHTNESS_GAP),
            "a later step holds every bound at the gap: {past:x?}"
        );
        assert_eq!(
            adjusted.removed_background, line,
            "the removed line keeps its tint"
        );
    }

    /// A raised highlight keeps at least the luminance contrast with its line
    /// that the one drawn with More contrast off has, on the same side, or
    /// that one stays: the bound the selection and Find's matches are raised
    /// under ([`EditorHighlights::raised`]). The fixtures are word-tint
    /// inputs, where the bound was found, run through that raise directly;
    /// no product path raises a word tint now, since word tints are lifted
    /// ([`EditorHighlights::lifted_word_tint`]). Midnight's removed word and
    /// Porcelain's added word, held only to their distance from their line
    /// and their side of it in OKLab lightness, were raised through
    /// saturation alone to their line's luminance (#5d0016 and #aefed9, 1.00
    /// to 1.01:1, where the tints drawn off stand 1.37:1 and 1.22:1 from
    /// their lines), and in a
    /// protanopia and deuteranopia model the removed word turned darker than
    /// its line and the added word fell from 0.067 to 0.018 from it; held to
    /// the luminance too, none is raised. A highlight on the far side of its
    /// line from the text on it, darker than a dark line under light text
    /// here, is still raised: away from the line, so its text reaches 7:1
    /// and the highlight stands further from the line in luminance than off.
    #[test]
    fn a_raised_highlight_keeps_its_off_luminance_contrast_with_its_line() {
        for (case, theme, removed) in [
            ("Midnight removed-word inputs", ThemeChoice::Midnight, true),
            ("Porcelain added-word inputs", ThemeChoice::Porcelain, false),
        ] {
            let chosen = theme.palette();
            let palette = chosen.with_more_contrast();
            let roles = palette.syntax_roles(Some(chosen));
            let (diff, line) = if removed {
                (palette.removed, palette.removed_background)
            } else {
                (palette.added, palette.added_background)
            };
            let own_roles = chosen.syntax_roles(None);
            let off = EditorHighlights::word_tint(chosen, own_roles, diff, line);
            let (lines, on_line) = (EditorHighlights::lines(palette), [line]);
            let drawn: Vec<_> =
                EditorHighlights::word_colors(roles, &lines, diff, &on_line).collect();
            let own: Vec<_> = own_roles.word_colors().into_iter().chain([diff]).collect();
            let seed = EditorHighlights::word_seed(diff, line);
            let away = EditorHighlights::away(palette);
            let fit = || EditorHighlights::raised(&drawn, &own, off, &on_line);
            assert_eq!(fit().raise(seed, away, diff), None, "{case}: none raised");
            let mut by_distance = fit();
            for apart in &mut by_distance.lines {
                apart.beyond = None;
            }
            let saturated = by_distance
                .raise(seed, away, diff)
                .expect("held only to distance and side, a tint is raised");
            assert!(
                contrast(saturated, line) < contrast(off, line),
                "{case}: held only to distance and side, #{saturated:06x} {:.3}:1 on its line, \
                 off #{off:06x} {:.3}:1",
                contrast(saturated, line),
                contrast(off, line)
            );
        }

        // Darker than its dark line under light text, a highlight is raised
        // away from the line until the text reads 7:1 (6.29:1 on the one
        // drawn off).
        let (line, off, text) = (0x30343a, 0x202328, 0xa0a4aa);
        assert!(contrast(text, off) < MORE_CONTRAST_TEXT);
        let raised = HighlightFit::targeted(&[(text, MORE_CONTRAST_TEXT)], &[line])
            .at_least_as_apart_as(off)
            .raise(off, 0x000000, off)
            .expect("raised away from its line");
        assert!(
            contrast(text, raised) >= MORE_CONTRAST_TEXT
                && custom::luminance(raised) < custom::luminance(off)
                && fainter_on_its_line(raised, off, line).is_none(),
            "#{raised:06x}: text {:.3}:1, {:.3}:1 on its line, off {:.3}:1",
            contrast(text, raised),
            contrast(raised, line),
            contrast(off, line)
        );
    }

    /// [`EditorHighlights::current_beside`] in literal greys, each way it
    /// can place the current match beside the selection, always at least
    /// [`HIGHLIGHT_DISTANCE`] and [`HIGHLIGHT_LIGHTNESS_GAP`] in lightness
    /// from it: raised to stand from the raised selection as far as the off
    /// current match stands from the off selection, or at least that floor
    /// where the two drawn off stand nearer; where none is raised, the off
    /// current match beside the raised selection, or beside the off one,
    /// where it stands so; or else fitted by the text rule beside the raised
    /// selection, or beside the off one. Raised only as far as off where
    /// that was less, the current match stood 0.003 from the selection in
    /// Sandstone; kept beside a selection it stood 0.044 from by chroma
    /// alone, 0.0001 in lightness in One Dark. Held to its distance from the
    /// raised selection alone, the current match settled beside the other
    /// matches (Omarchy alacritty-dark 0.040 to 0.024), and the raised
    /// selection beside the off current match (Omarchy ristretto 0.090 to
    /// 0.039).
    #[test]
    fn the_current_match_stands_from_a_raised_selection_as_far_as_off() {
        let apart = |a: u32, b: u32| oklab_distance(oklab(a), oklab(b));
        let (off_selection, near, far) = (0x404040, 0x434343, 0x707070);
        assert!(apart(off_selection, near) < HIGHLIGHT_DISTANCE);
        assert!(apart(off_selection, far) >= HIGHLIGHT_DISTANCE);
        // The selection's lightness at 0.05 of chroma: apart by chroma alone.
        let [lightness, ..] = oklab(off_selection);
        let tinted = from_linear(linear_in_gamut([lightness, 0.05, 0.]));
        assert!(apart(off_selection, tinted) >= HIGHLIGHT_DISTANCE);
        assert!((oklab(tinted)[0] - lightness).abs() < HIGHLIGHT_LIGHTNESS_GAP);
        let asked = std::cell::RefCell::new(Vec::new());
        let raised = |current: u32, from: f64| {
            let asked = &asked;
            move |distance: f64| {
                asked.borrow_mut().push(distance);
                (distance <= from).then_some(current)
            }
        };
        let none = || raised(0x000000, f64::NEG_INFINITY);
        let fits = std::cell::RefCell::new(Vec::new());
        // A fitted candidate beside `beside` (or any selection) that stands
        // at most `reach` from it.
        let fitted = |current: Option<u32>, beside: Option<u32>, reach: f64| {
            let fits = &fits;
            move |selection: u32, distance: f64| {
                fits.borrow_mut().push((selection, distance));
                current.filter(|_| {
                    beside.is_none_or(|beside| beside == selection) && distance <= reach
                })
            }
        };
        let never = || fitted(None, None, 0.);

        // Raised as far from the raised selection as off.
        let off_apart = apart(off_selection, far);
        assert_eq!(
            EditorHighlights::current_beside(
                (off_selection, far),
                0x383838,
                raised(0xaa2222, 1.),
                never()
            ),
            (0x383838, 0xaa2222),
            "raised"
        );
        assert_eq!(asked.take(), [off_apart], "asked as far as off");
        assert!(fits.take().is_empty(), "none fitted");

        // Drawn off nearer than the floor: raised to the floor, never only
        // as far as off.
        assert_eq!(
            EditorHighlights::current_beside(
                (off_selection, near),
                0x383838,
                raised(0x505050, 1.),
                never()
            ),
            (0x383838, 0x505050),
            "raised to the floor"
        );
        assert_eq!(asked.take(), [HIGHLIGHT_DISTANCE], "asked at the floor");

        // None raised: the off current match beside the raised selection
        // that stands from it as far as off, with the lightness gap.
        assert!(apart(far, 0x202020) >= off_apart);
        assert_eq!(
            EditorHighlights::current_beside((off_selection, far), 0x202020, none(), never()),
            (0x202020, far),
            "the off current match beside the raised selection"
        );
        assert_eq!(asked.take(), [off_apart], "asked once, as far as off");
        // Nearer the raised selection than off: both drawn off.
        assert!(apart(far, 0x606060) < off_apart);
        assert_eq!(
            EditorHighlights::current_beside((off_selection, far), 0x606060, none(), never()),
            (off_selection, far),
            "both drawn off"
        );
        assert!(
            fits.take().is_empty(),
            "none fitted while the off pair stands apart"
        );
        asked.take();

        // Drawn off nearer than the floor, or apart by chroma alone: fitted
        // beside the raised selection, else beside the off one, as far as
        // the pair drawn off stand apart, at least the floor.
        for off_current in [near, tinted] {
            let as_off = apart(off_selection, off_current).max(HIGHLIGHT_DISTANCE);
            assert_eq!(
                EditorHighlights::current_beside(
                    (off_selection, off_current),
                    0x3c3c3c,
                    none(),
                    fitted(Some(0x606060), None, 1.)
                ),
                (0x3c3c3c, 0x606060),
                "fitted beside the raised selection, #{off_current:06x} drawn off"
            );
            assert_eq!(
                fits.take(),
                [(0x3c3c3c, as_off)],
                "fitted beside the raised selection, as far as off"
            );
            assert_eq!(
                EditorHighlights::current_beside(
                    (off_selection, off_current),
                    0x3c3c3c,
                    none(),
                    fitted(Some(0x606060), Some(off_selection), 1.)
                ),
                (off_selection, 0x606060),
                "fitted beside the off selection, #{off_current:06x} drawn off"
            );
            assert_eq!(
                fits.take(),
                [(0x3c3c3c, as_off), (off_selection, as_off)],
                "then beside the off one"
            );
        }
        // Where no fitted candidate stands as far as off, beside either
        // selection, the floor: beside the raised selection first.
        let as_off = apart(off_selection, tinted);
        assert!(as_off > HIGHLIGHT_DISTANCE);
        assert_eq!(
            EditorHighlights::current_beside(
                (off_selection, tinted),
                0x3c3c3c,
                none(),
                fitted(Some(0x606060), None, HIGHLIGHT_DISTANCE)
            ),
            (0x3c3c3c, 0x606060),
            "fitted at the floor"
        );
        assert_eq!(
            fits.take(),
            [
                (0x3c3c3c, as_off),
                (off_selection, as_off),
                (0x3c3c3c, HIGHLIGHT_DISTANCE)
            ],
            "as far as off beside both selections, then the floor"
        );
        asked.take();
    }

    /// Fitted by the text rule, Find's current match keeps the distance the
    /// pair drawn off stand apart wherever a fitted candidate can: here a
    /// grey selection and a current match drawn off 0.10 apart through
    /// chroma, under [`HIGHLIGHT_LIGHTNESS_GAP`] apart in lightness, so the
    /// current match drawn off cannot stay, and none is raised. Fitted only
    /// [`HIGHLIGHT_DISTANCE`] from the selection, as before, the current
    /// match stood nearer it than the pair drawn off (One Dark's `#104266`,
    /// 0.041 from its selection, where the pair drawn off stand 0.044 apart
    /// through chroma).
    #[test]
    fn a_fitted_current_match_keeps_its_off_distance_where_it_can() {
        let apart = |a: u32, b: u32| oklab_distance(oklab(a), oklab(b));
        let (canvas, text, accent) = (0x202020, 0xe0e0e0, 0xc8553d);
        let off_selection = 0x404040;
        let [lightness, ..] = oklab(off_selection);
        let off_current = from_linear(linear_in_gamut([lightness, 0.102, 0.]));
        let off_apart = apart(off_selection, off_current);
        assert!(off_apart >= 0.1, "{off_apart:.4}");
        assert!((oklab(off_current)[0] - lightness).abs() < HIGHLIGHT_LIGHTNESS_GAP);
        // As [`EditorHighlights::fit_raised`] fits one: the text rule, toward
        // black and then toward white.
        let seed = mix(off_selection, accent, 64);
        let fitted = |selection: u32, distance: f64| {
            let fit = HighlightFit::new(&[(text, &[canvas][..])], &[canvas])
                .apart_from(selection, distance);
            fit.search(seed, 0x000000, accent)
                .or_else(|| fit.search(seed, 0xffffff, accent))
        };
        let floor = fitted(off_selection, HIGHLIGHT_DISTANCE).expect("one at the floor");
        assert!(
            apart(floor, off_selection) < off_apart,
            "at the floor #{floor:06x} {:.4} from the selection",
            apart(floor, off_selection)
        );
        let (selection, current) = EditorHighlights::current_beside(
            (off_selection, off_current),
            off_selection,
            |_| None,
            fitted,
        );
        assert_eq!(selection, off_selection);
        assert!(
            apart(current, selection) >= off_apart
                && (oklab(current)[0] - oklab(selection)[0]).abs() >= HIGHLIGHT_LIGHTNESS_GAP,
            "#{current:06x} {:.4} from the selection, off {off_apart:.4}",
            apart(current, selection)
        );
        assert!(contrast(text, current) >= LABEL_RULE);
    }

    /// Find's current match keeps the distance it stands from the other
    /// matches with More contrast off, in two synthetic cases built from
    /// built-in palettes, so a run without the Omarchy palettes covers it:
    /// Midnight and Graphite with a grey selected row and a warm accent, in
    /// which the current match drawn off stands at least
    /// [`HIGHLIGHT_DISTANCE`] from the other matches. The selection keeps the
    /// one drawn off, and the current match stands as far as off from the
    /// selection and from the other matches, which take a colour of their
    /// own, and [`HIGHLIGHT_LIGHTNESS_GAP`] from the selection in lightness
    /// (the pair drawn off stand 0.05 apart, under 0.02 in lightness, so the
    /// current match moves). Held only to its distance from its lines, the selection alone
    /// was once raised toward the current match drawn off here (Midnight's
    /// #44251e to #551205, 0.012 from the current match #591509, where the
    /// selection drawn off stands 0.053 from it); held also to its luminance
    /// contrast with each line, it is not. Each way the current match is
    /// placed beside a raised selection is read in
    /// `the_current_match_stands_from_a_raised_selection_as_far_as_off`.
    #[test]
    fn the_current_match_keeps_its_off_distance_from_the_other_matches() {
        for (case, base, selected, accent) in [
            ("Midnight", ThemeChoice::Midnight, 0x383838, 0xc8553d),
            ("Graphite", ThemeChoice::Graphite, 0x343335, 0xb5651d),
        ] {
            let mut custom = CustomTheme::from_base(1, case, base);
            custom.palette.set(TokenKind::Selected, selected);
            custom.palette.set(TokenKind::Accent, accent);
            let chosen = custom.palette;
            let (palette, raised) = chosen.more_contrast_fit();
            let roles = palette.syntax_roles(Some(chosen));
            let off = EditorHighlights::fit(chosen, chosen.syntax_roles(None), None);
            let on = EditorHighlights::fit(palette, roles, Some(raised));
            let apart = |current: u32, others: u32| oklab_distance(oklab(current), oklab(others));
            let off_apart = apart(off.find_current, off.selection);
            assert!(
                off_apart >= HIGHLIGHT_DISTANCE,
                "{case}: the current match drawn off stands {off_apart:.3} from the others"
            );

            assert_eq!(
                on.selection, off.selection,
                "{case}: the selection drawn off"
            );
            for (others, color) in [
                ("selection", on.selection),
                ("other matches", on.find_match),
            ] {
                assert!(
                    apart(on.find_current, color) >= off_apart,
                    "{case}: the current match #{:06x} {:.3} from the {others} #{color:06x}, off \
                     {off_apart:.3}",
                    on.find_current,
                    apart(on.find_current, color),
                );
            }
            let lightness = (oklab(on.find_current)[0] - oklab(on.selection)[0]).abs();
            assert!(
                lightness >= HIGHLIGHT_LIGHTNESS_GAP,
                "{case}: the current match #{:06x} {lightness:.4} from the selection in lightness",
                on.find_current
            );
        }
    }

    /// With More contrast on, no color reads worse on the text selection than
    /// it does with More contrast off, nor on Find's current match where it
    /// keeps or raises the one drawn off, in every palette (Find's other
    /// matches are fitted by the text rule, never raised):
    /// `text`, every syntax role and the patch's `added`,
    /// `removed` and `hunk`, the colors those highlights are fitted under, each
    /// read on the highlight drawn on at least the contrast it reads on the
    /// one drawn off, and where the highlight moved, 7:1 or its own contrast
    /// on the lines it is drawn on where that is lower; each highlight stands
    /// from every editor line, in OKLab, at least as far as the one drawn off
    /// and on the same side of it in lightness, so in a dark palette none
    /// turns darker than its darkest line where the one drawn off is lighter;
    /// and wherever the current match drawn off stands
    /// [`HIGHLIGHT_DISTANCE`] from the other matches, the current match
    /// stands from them at least as far. Where the current match or the
    /// other matches can do neither and stand apart as the rules of
    /// 2026-11-05 afternoon ask (the current match [`HIGHLIGHT_DISTANCE`]
    /// and [`HIGHLIGHT_LIGHTNESS_GAP`] from the selection, the other matches
    /// in a colour of their own), each is fitted by the text rule instead:
    /// every color on it reads 4.5:1 and it stands [`HIGHLIGHT_DISTANCE`]
    /// from every editor line. Fitted only to the text rule against
    /// the raised syntax, they settled lighter and nearer their seeds, and
    /// 213 colors read worse on them across the palettes (Midnight's strings
    /// on the current match 6.18:1 where they read 6.63:1); held to the
    /// distance alone, the selection turned near black in seven dark
    /// palettes (Midnight's #0b3e3e to #000001), and with the current
    /// match's distance taken from the raised selection, the current match
    /// settled beside the other matches (Omarchy alacritty-dark 0.040 to
    /// 0.024, ristretto 0.090 to 0.039).
    #[gpui::test]
    fn more_contrast_never_reads_worse_on_the_selection_or_find(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        const DRAWN: [&str; 12] = [
            "keyword",
            "string",
            "constant",
            "type",
            "function",
            "markup",
            "comment",
            "removed",
            "text",
            "added",
            "removed lines",
            "hunk",
        ];
        let mut failures = Vec::new();
        let (mut raised, mut kept, mut separated) = (0, 0, 0);
        let (mut current_as_off, mut current_fitted, mut matches_fitted) = (0, 0, 0);
        for (name, chosen) in more_contrast_palettes() {
            let [off, on] = [false, true].map(|more| {
                cx.update(|cx| {
                    set_more_contrast(more, cx);
                    chosen.apply(chosen.is_light(), None, cx);
                    let applied = palette(cx);
                    let lines = EditorHighlights::lines(applied);
                    let drawn: Vec<(u32, Vec<u32>)> = EditorHighlights::patch_colors(
                        applied,
                        applied.syntax_roles(more.then_some(chosen)),
                        &lines,
                    )
                    .into_iter()
                    .map(|(color, on)| (color, on.to_vec()))
                    .collect();
                    (editor_highlights(cx), drawn, lines)
                })
            });
            let ((off_tints, was, _), (on_tints, is, lines)) = (off, on);
            if on_tints.selection == off_tints.selection {
                kept += 1;
            } else {
                raised += 1;
            }
            let darkest = lines
                .iter()
                .map(|&line| oklab(line)[0])
                .fold(f64::INFINITY, f64::min);
            for (highlight, off_tint, on_tint) in [
                ("selection", off_tints.selection, on_tints.selection),
                ("Find match", off_tints.find_match, on_tints.find_match),
                (
                    "current Find match",
                    off_tints.find_current,
                    on_tints.find_current,
                ),
            ] {
                // How the highlight reads worse or stands nearer than off, if
                // it does.
                let mut worse = Vec::new();
                for ((color, (was, _)), (is, on)) in DRAWN.iter().zip(&was).zip(&is) {
                    let (was, is) = (*was, *is);
                    let (off_ratio, on_ratio) = (contrast(was, off_tint), contrast(is, on_tint));
                    if on_ratio < off_ratio {
                        worse.push(format!(
                            "{name} {color} #{is:06x} on the {highlight} #{on_tint:06x} \
                             {on_ratio:.3}:1, off #{was:06x} on #{off_tint:06x} {off_ratio:.3}:1"
                        ));
                    }
                    let floor = on
                        .iter()
                        .map(|&beneath| contrast(is, beneath))
                        .fold(MORE_CONTRAST_TEXT, f64::min)
                        .max(LABEL_RULE);
                    if on_tint != off_tint && on_ratio < floor {
                        worse.push(format!(
                            "{name} {color} on the moved {highlight} #{on_tint:06x} \
                             {on_ratio:.3}:1, needs {floor:.3}"
                        ));
                    }
                }
                for line in lines {
                    let apart = |tint: u32| oklab_distance(oklab(tint), oklab(line));
                    if apart(on_tint) < apart(off_tint) {
                        worse.push(format!(
                            "{name} {highlight} #{on_tint:06x} {:.3} from the line #{line:06x}, \
                             off #{off_tint:06x} {:.3}",
                            apart(on_tint),
                            apart(off_tint)
                        ));
                    }
                    let side = |tint: u32| oklab(tint)[0].total_cmp(&oklab(line)[0]);
                    if side(on_tint) != side(off_tint) {
                        worse.push(format!(
                            "{name} {highlight} #{on_tint:06x} {:?} the line #{line:06x} in \
                             lightness, off #{off_tint:06x} {:?}",
                            side(on_tint),
                            side(off_tint)
                        ));
                    }
                    if let Some(failure) = fainter_on_its_line(on_tint, off_tint, line) {
                        worse.push(format!("{name} {highlight} {failure}"));
                    }
                }
                if !chosen.is_light()
                    && oklab(off_tint)[0] > darkest
                    && oklab(on_tint)[0] <= darkest
                {
                    worse.push(format!(
                        "{name} {highlight} #{on_tint:06x} darker than the darkest line, off \
                         #{off_tint:06x} lighter"
                    ));
                }
                if worse.is_empty() {
                    current_as_off += usize::from(highlight == "current Find match");
                    continue;
                }
                // The selection is always raised or drawn off; the current
                // match and the other matches may be fitted by the text rule.
                // Under More contrast the patch draws its text in the fitted
                // roles ([`PatchText`]), so the diff colors are not text there.
                let fitted = highlight != "selection"
                    && is
                        .iter()
                        .zip(DRAWN)
                        .filter(|(_, color)| !matches!(*color, "added" | "removed lines" | "hunk"))
                        .all(|(&(color, _), _)| contrast(color, on_tint) >= LABEL_RULE)
                    && lines.iter().all(|&line| {
                        oklab_distance(oklab(on_tint), oklab(line)) >= HIGHLIGHT_DISTANCE
                    });
                if fitted {
                    match highlight {
                        "current Find match" => current_fitted += 1,
                        _ => matches_fitted += 1,
                    }
                } else {
                    failures.extend(worse);
                }
            }
            let beside = |tints: EditorHighlights| {
                oklab_distance(oklab(tints.find_current), oklab(tints.find_match))
            };
            if beside(off_tints) >= HIGHLIGHT_DISTANCE {
                separated += 1;
                if beside(on_tints) < beside(off_tints) {
                    failures.push(format!(
                        "{name} current Find match #{:06x} {:.3} from the other matches \
                         #{:06x}, off #{:06x} {:.3} from #{:06x}",
                        on_tints.find_current,
                        beside(on_tints),
                        on_tints.find_match,
                        off_tints.find_current,
                        beside(off_tints),
                        off_tints.find_match
                    ));
                }
            }
        }
        cx.update(|cx| set_more_contrast(false, cx));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        // Held to their off luminance contrast with every line, the
        // selection and current match are rarely raised; each way the
        // current match is placed beside a raised selection is read in
        // `the_current_match_stands_from_a_raised_selection_as_far_as_off`.
        assert!(
            kept > 0
                && separated > 0
                && current_as_off > 0
                && current_fitted > 0
                && matches_fitted > 0,
            "selections raised {raised}, kept {kept}; current matches apart off {separated}, \
             kept or raised {current_as_off}, fitted {current_fitted}; other matches fitted \
             {matches_fitted}"
        );
    }

    /// The controls [`FieldControls`] draws on each surface, by kind: a text
    /// field with a placeholder, enabled and disabled, and Settings' Save
    /// (the kit's default Button) enabled and disabled.
    const FIELD_CONTROLS: [&str; 4] = ["field", "disabled-field", "save", "disabled-save"];

    /// Each of [`FIELD_CONTROLS`] in a row on each of
    /// [`more_contrast_edge_surfaces`] of the applied palette, selected as
    /// `kind-surface`.
    struct FieldControls {
        /// Each surface's field and disabled field.
        fields: Vec<[gpui::Entity<gpui_kit::component::input::InputState>; 2]>,
    }

    impl FieldControls {
        fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
            use gpui_kit::AppContext as _;
            use gpui_kit::component::input::InputState;

            let fields = more_contrast_edge_surfaces(palette(cx))
                .map(|_| {
                    [(); 2].map(|_| cx.new(|cx| InputState::new(window, cx).placeholder("Title")))
                })
                .into();
            Self { fields }
        }
    }

    impl gpui::Render for FieldControls {
        fn render(
            &mut self,
            _: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui_kit::component::Disableable as _;
            use gpui_kit::component::button::Button;
            use gpui_kit::component::input::Input;
            use gpui_kit::{InteractiveElement as _, ParentElement as _, div};

            let surfaces = more_contrast_edge_surfaces(palette(cx));
            div()
                .flex()
                .flex_col()
                .children(surfaces.into_iter().zip(self.fields.clone()).map(
                    |((surface, color), [field, disabled_field])| {
                        let id = move |kind: &'static str| resting_selector(kind, surface);
                        let selector = move |kind: &'static str| move || id(kind).into();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .p_2()
                            .bg(rgb(color))
                            .child(
                                div()
                                    .w(px(200.))
                                    .debug_selector(selector("field"))
                                    .child(Input::new(&field)),
                            )
                            .child(
                                div()
                                    .w(px(200.))
                                    .debug_selector(selector("disabled-field"))
                                    .child(Input::new(&disabled_field).disabled(true)),
                            )
                            .child(
                                // Mirrors `settings.rs`'s Save chain: the kit's default Button.
                                Button::new(id("save"))
                                    .label("Save")
                                    .debug_selector(selector("save")),
                            )
                            .child(
                                Button::new(id("disabled-save"))
                                    .label("Save")
                                    .disabled(true)
                                    .debug_selector(selector("disabled-save")),
                            )
                    },
                ))
        }
    }

    /// A fill is not an edge. With More contrast on, in every palette, a text
    /// field and Settings' Save, each enabled and disabled, paint on the
    /// canvas, panel, subtle and raised surfaces the fills they paint with it
    /// off, while the enabled field's outline is the adjusted `border` and
    /// reads 3:1 or more against each surface, and the disabled field's is
    /// the disabled edge, as a disabled Button's, and reads below it on each
    /// surface; off, both outlines are `border`, as the kit draws them. A
    /// disabled field once drew the enabled field's 3:1 outline beside a
    /// disabled Button's 1.66:1 edge (Midnight #606976 against #313d4e),
    /// marked only by its dim text. The kit derived a
    /// dark field's fill from `input`, the outline, at 30%, so the fill
    /// lifted with the outline (Midnight #1a2330 to #2a323d), the disabled
    /// Save's fill, half of it, with it, and the placeholder on the field
    /// read worse (Omarchy tokyo-night 5.99:1 to 5.32:1 on the subtle
    /// surface); a disabled field's fill, `input` at 80% in either mode,
    /// lifted in every palette. The placeholder, drawn in `muted_foreground`,
    /// now reads on the field at least as it does off on every surface
    /// (tokyo-night's 6.63:1 there).
    #[gpui::test]
    fn more_contrast_keeps_the_field_and_save_fills_in_every_palette(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(FieldControls::new);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.simulate_resize(gpui::size(px(800.), px(400.)));
        let mut failures = Vec::new();
        let mut tokyo_night = Vec::new();
        for (name, chosen) in test_palettes() {
            // Each control on each surface: the surface, its fills and the
            // 1 px edges it paints, and the placeholder's color.
            let [off, on] = [false, true].map(|more| {
                cx.update(|window, cx| {
                    set_more_contrast(more, cx);
                    chosen.apply(chosen.is_light(), Some(window), cx);
                });
                cx.update(|window, cx| window.draw(cx).clear(cx));
                let placeholder = cx.update(|_, cx| packed(Theme::global(cx).muted_foreground));
                let mut drawn = Vec::new();
                for (surface, color) in more_contrast_edge_surfaces(chosen) {
                    for kind in FIELD_CONTROLS {
                        let bounds = cx
                            .debug_bounds(resting_selector(kind, surface))
                            .expect(kind);
                        let fills = painted_button(cx, bounds).0;
                        let edges = painted_edges(cx, bounds);
                        drawn.push((kind, surface, color, fills, edges));
                    }
                }
                (placeholder, drawn)
            });
            let ((off_placeholder, off), (placeholder, on)) = (off, on);
            let (outline, disabled_edge) =
                (chosen.with_more_contrast().border, chosen.disabled_edge());
            for (before, after) in off.iter().zip(&on) {
                let (kind, surface, color, fills, edges) = after;
                let at = format!("{name} {kind} on the {surface}");
                if *fills != before.3 {
                    failures.push(format!("{at}: fills {fills:?}, off {:?}", before.3));
                }
                if !kind.ends_with("field") {
                    continue;
                }
                let one_px = |edges: &[PaintedEdge]| -> Vec<u32> {
                    edges
                        .iter()
                        .filter(|edge| edge.one_px)
                        .map(|edge| edge.color)
                        .collect()
                };
                if !one_px(&before.4).contains(&chosen.border) {
                    failures.push(format!(
                        "{at}: off, outlines {:06x?}, none #{:06x}",
                        one_px(&before.4),
                        chosen.border
                    ));
                }
                let drawn = if *kind == "field" {
                    outline
                } else {
                    disabled_edge
                };
                let outlines = one_px(edges);
                if !outlines.contains(&drawn) || (drawn != outline && outlines.contains(&outline)) {
                    failures.push(format!("{at}: outlines {outlines:06x?}, not #{drawn:06x}"));
                }
                let (ratio, enabled) = (contrast(drawn, *color), contrast(outline, *color));
                if *kind != "field" {
                    if ratio >= enabled {
                        failures.push(format!(
                            "{at}: outline #{drawn:06x} {ratio:.3}:1, the enabled field's \
                             #{outline:06x} {enabled:.3}:1"
                        ));
                    }
                    continue;
                }
                if ratio < MORE_CONTRAST_EDGE {
                    failures.push(format!("{at}: outline #{drawn:06x} {ratio:.3}:1"));
                }
                let reading = |placeholder: u32, fills: &[Background]| {
                    let fill = fills.last().and_then(|fill| fill.as_solid())?;
                    let beneath = composite(fill.to_rgb(), *color);
                    Some(contrast(placeholder, beneath))
                };
                match (
                    reading(placeholder, fills),
                    reading(off_placeholder, &before.3),
                ) {
                    (Some(ratio), Some(off_ratio)) => {
                        if ratio < off_ratio {
                            failures.push(format!(
                                "{at}: placeholder {ratio:.3}:1, off {off_ratio:.3}:1"
                            ));
                        }
                        if name == "Omarchy tokyo-night" {
                            tokyo_night.push((*surface, ratio, off_ratio));
                        }
                    }
                    readings => failures.push(format!("{at}: field fills {readings:?}")),
                }
            }
        }
        cx.update(|window, cx| {
            set_more_contrast(false, cx);
            ThemeChoice::Midnight.apply(Some(window), cx);
        });
        // Omarchy tokyo-night (on Linux): its composer's field rests on the
        // subtle surface, where the placeholder read 5.99:1 off and 5.32:1 on
        // the lifted fill.
        if let Some(&(_, ratio, off_ratio)) = tokyo_night
            .iter()
            .find(|(surface, ..)| *surface == "subtle")
        {
            assert!(
                (off_ratio - 5.99).abs() < 0.005 && ratio >= off_ratio,
                "Omarchy tokyo-night placeholder {ratio:.3}:1, off {off_ratio:.3}:1"
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// What an application left installed: the `Palette` global, its editor
    /// highlights and labels, the kit's colors and editor style as its theme
    /// format writes them, and the kit fields the palette sets beside them.
    fn installed(
        cx: &App,
    ) -> (
        Palette,
        EditorHighlights,
        Labels,
        String,
        [Option<Hsla>; 10],
    ) {
        let theme = Theme::global(cx);
        let kit = serde_json::to_string(&(theme.colors, &theme.highlight_theme.style)).unwrap();
        let fields = [
            theme.popover_edge,
            theme.dialog_edge,
            theme.secondary_edge,
            theme.disabled_label,
            theme.disabled_edge,
            theme.switch_hover,
            theme.ghost_hover,
            theme.ghost_active,
            theme.resting_edge,
            theme.input_fill_base,
        ];
        let labels = *cx.global::<Labels>();
        (palette(cx), editor_highlights(cx), labels, kit, fields)
    }

    /// A palette's label, syntax colors by field and disabled label strength.
    type Fingerprint = (&'static str, &'static [(&'static str, u32)], f32);

    /// What the off path drew before More contrast existed (51a32680), in two
    /// palettes whose syntax the fitting moves, a dark and a light one: the
    /// fitted syntax colors by field and the disabled label's strength. The
    /// hover rule of 2026-11-05 darkened Kanagawa Lotus's text and secondary
    /// text, so the light guard is Lotus's tokens before then
    /// ([`lotus_before_the_hover_rule`]), which still draw what 51a32680
    /// drew, and the shipped Lotus is pinned as it draws since.
    const OFF_PATH_FINGERPRINT: [Fingerprint; 3] = [
        (
            "Solarized Dark",
            &[
                ("keyword", 0x949acd),
                ("constant", 0xbc9a29),
                ("function", 0x33aea6),
                ("tag", 0x5ca5d4),
                ("diff.minus", 0xda8785),
            ],
            0.625,
        ),
        (
            "Kanagawa Lotus",
            &[
                ("string", 0x4a5f41),
                ("constant", 0x834c0c),
                ("type", 0x6a5638),
                ("function", 0x455982),
                ("tag", 0x4e5875),
            ],
            0.703125,
        ),
        (
            LOTUS_BEFORE_THE_HOVER_RULE,
            &[
                ("string", 0x4b5e45),
                ("constant", 0x7f4e15),
                ("type", 0x63574d),
                ("function", 0x48597e),
                ("tag", 0x535870),
            ],
            0.71875,
        ),
    ];

    const LOTUS_BEFORE_THE_HOVER_RULE: &str = "Kanagawa Lotus before the hover rule";

    /// Kanagawa Lotus's tokens before the hover rule of 2026-11-05 stepped its
    /// `selected` darker and, with it, its text, secondary text and modified.
    fn lotus_before_the_hover_rule() -> Palette {
        Palette {
            selected: sources::kanagawa::lotus::LOTUS_BLUE_1,
            text: sources::kanagawa::lotus::LOTUS_INK_1,
            muted: 0x5a574d,
            modified: 0x936300,
            ..ThemeChoice::KanagawaLotus.palette()
        }
    }

    /// With More contrast off, `Palette::apply` installs the palette as it
    /// is, in every palette: the palette, the editor highlights and small
    /// labels fitted from it, and the kit fields `configure` sets from it;
    /// and after More contrast was on, turning it off installs exactly that
    /// again. `as_applied` is the palette, and `set_more_contrast` reports
    /// only a change. Those comparisons run the same helpers `apply` does,
    /// so they check that the off path is the palette's own path, not that
    /// it draws what it drew before More contrast existed; for that,
    /// [`OFF_PATH_FINGERPRINT`] pins two palettes' fitted syntax and
    /// disabled label as the off path drew them then, one of them through
    /// [`lotus_before_the_hover_rule`].
    #[gpui::test]
    fn without_more_contrast_every_palette_applies_as_it_is(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        assert!(
            !cx.update(|cx| set_more_contrast(false, cx)),
            "off by default"
        );
        let mut pinned = 0;
        let palettes = more_contrast_palettes().into_iter().chain([(
            LOTUS_BEFORE_THE_HOVER_RULE.to_owned(),
            lotus_before_the_hover_rule(),
        )]);
        for (name, chosen) in palettes {
            let (before, drawn, dim) = cx.update(|cx| {
                chosen.apply(chosen.is_light(), None, cx);
                assert_eq!(chosen.as_applied(cx), chosen, "{name} as applied");
                let theme = Theme::global(cx);
                let dim = theme.disabled_label.map(|label| label.a);
                (installed(cx), editor_text_colors(theme), dim)
            });
            for (palette, fields, strength) in OFF_PATH_FINGERPRINT {
                if palette != name {
                    continue;
                }
                pinned += 1;
                for &(field, color) in fields {
                    let found = drawn.iter().find(|(drawn, _)| drawn.as_str() == field);
                    assert_eq!(
                        found.and_then(|(_, color)| *color),
                        Some(color),
                        "{name} {field}"
                    );
                }
                assert_eq!(dim, Some(strength), "{name} disabled label");
            }
            let expected = chosen.configured_theme(chosen.is_light());
            assert_eq!(before.0, chosen, "{name} palette");
            assert_eq!(before.1, chosen.editor_highlights(), "{name} highlights");
            assert_eq!(before.2, Labels::fit(chosen), "{name} labels");
            assert_eq!(
                before.4,
                [
                    expected.popover_edge,
                    expected.dialog_edge,
                    expected.secondary_edge,
                    expected.disabled_label,
                    expected.disabled_edge,
                    expected.switch_hover,
                    expected.ghost_hover,
                    expected.ghost_active,
                    None,
                    None,
                ],
                "{name} kit edges and fills"
            );
            let after = cx.update(|cx| {
                assert!(set_more_contrast(true, cx), "{name} turns it on");
                assert!(!set_more_contrast(true, cx), "{name} on again");
                chosen.apply(chosen.is_light(), None, cx);
                assert_ne!(installed(cx), before, "{name} adjusted");
                assert!(set_more_contrast(false, cx), "{name} turns it off");
                chosen.apply(chosen.is_light(), None, cx);
                installed(cx)
            });
            assert_eq!(after, before, "{name} after More contrast");
        }
        assert_eq!(pinned, OFF_PATH_FINGERPRINT.len(), "every pinned palette");
    }

    /// The selectors the width walk (`width_walk`) and the layout grid
    /// (`layout_grid`) read: what More contrast must leave where it is.
    const LAID_OUT: &[&str] = &[
        "branch-edge",
        "branch-edge-line",
        "changed-files-count",
        "changed-files-header",
        "changed-files-label",
        "changes-count-badge",
        "changes-tab",
        "code-editor",
        "commit-composer",
        "commit-description",
        "commit-inspector",
        "commit-inspector-files",
        "commit-message-toolbar",
        "commit-message-viewport",
        "commit-recovery-menu",
        "commit-staged",
        "commit-staged-count",
        "commit-title",
        "commit-title-counter",
        "compare-footer",
        "compare-toolbar",
        "copy-commit",
        "current-branch",
        "custom-themes-card",
        "edge-add-remote",
        "edge-create-branch",
        "files-pane",
        "git-actions",
        "git-actions-toggle",
        "history-clear-search",
        "history-column-0",
        "history-column-1",
        "history-column-2",
        "history-column-3",
        "history-column-4",
        "history-column-5",
        "history-empty-state",
        "history-list-edge",
        "history-no-match-line",
        "history-pane",
        "history-scope-count",
        "history-scope-name",
        "history-scope-toolbar",
        "history-search",
        "history-search-controls",
        "history-search-summary",
        "history-sidebar",
        "image-system-preview-0",
        "image-system-preview-1",
        "local-refresh-details",
        "local-refresh-warning",
        "main-menu",
        "navigation-filter",
        "navigation-hint",
        "nav-mode-local",
        "nav-mode-remote",
        "nav-mode-worktrees",
        "open-blame",
        "preview-content",
        "preview-file-path",
        "project-pane",
        "project-pane-tree",
        "repository-header",
        "repository-navigation",
        "repository-rail",
        "repository-tab-0",
        "repository-tab-strip",
        "repository-workspaces",
        "retry-local-refresh",
        "selected-changed-file",
        "settings-repository-identity",
        "settings-scroll",
        "settings-section: Appearance",
        "settings-section: Git identity",
        "settings-section: History columns",
        "settings-section: Projects",
        "stage-all",
        "staged-heading-count",
        "status-hints",
        "status-line",
        "system-preview-0",
        "system-preview-1",
        "text-mode-menu",
        "unstage-all",
        "unstaged-heading-count",
        "working-header",
        "working-header-count",
        "working-header-label",
        "working-pane",
        "working-toolbar",
    ];

    /// More contrast recolors and moves nothing. On the fixture in History,
    /// then Compare, then Settings, in a 1,480 × 973 window and a 461 × 490
    /// half tile, at 13 and 18 pt, every selector of [`LAID_OUT`] keeps its
    /// bounds with More contrast on and off, the window drawn whole (Settings
    /// is a cached view) with motion reduced.
    #[gpui::test]
    async fn more_contrast_moves_nothing_on_any_page(cx: &mut gpui::TestAppContext) {
        use crate::width_walk;
        let (_fixture, app, cx, _) = width_walk::open(width_walk::Page::History, cx).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let mut moved = Vec::new();
        for page in ["History", "Compare", "Settings"] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| match page {
                    "Compare" => {
                        let readme = app
                            .files
                            .iter()
                            .position(|file| file.path() == std::path::Path::new("README.md"))
                            .expect("README.md among the commit's files");
                        app.select_file(readme, window, cx);
                    }
                    "Settings" => app.show_settings(window, cx),
                    _ => {}
                })
            });
            crate::views::tests::settle(&app, cx).await;
            let mut drawn = 0;
            for (width, height) in [(1480., 973.), (461., 490.)] {
                for text in [13, 18] {
                    cx.update(|window, cx| {
                        apply_text_sizes(text, DEFAULT_CODE_TEXT_SIZE, window, cx)
                    });
                    cx.simulate_resize(gpui::size(px(width), px(height)));
                    let [off, on] = [false, true].map(|on| {
                        cx.update(|_, cx| {
                            set_more_contrast(on, cx);
                            ThemeChoice::Midnight.apply(None, cx);
                        });
                        width_walk::frame(cx);
                        app.update(cx, |_, cx| cx.notify());
                        width_walk::frame(cx);
                        cx.update(|window, cx| {
                            window.refresh();
                            window.draw(cx).clear(cx);
                        });
                        LAID_OUT
                            .iter()
                            .map(|&selector| (selector, cx.debug_bounds(selector)))
                            .collect::<Vec<_>>()
                    });
                    drawn += off.iter().filter(|(_, bounds)| bounds.is_some()).count();
                    for ((selector, off), (_, on)) in off.into_iter().zip(on) {
                        if off != on {
                            moved.push(format!(
                                "{page} {width}x{height} at {text} pt: {selector} {off:?} -> {on:?}"
                            ));
                        }
                    }
                }
            }
            assert!(drawn >= 4 * 5, "{page} draws {drawn} of the selectors");
        }
        cx.update(|window, cx| {
            set_more_contrast(false, cx);
            apply_text_sizes(
                DEFAULT_INTERFACE_TEXT_SIZE,
                DEFAULT_CODE_TEXT_SIZE,
                window,
                cx,
            )
        });
        assert!(moved.is_empty(), "{}", moved.join("\n"));
    }

    /// The controls [`RestingControls`] draws, by kind: History's selected
    /// and unselected mode toggles (`crate::button`, through `button_frame`),
    /// the action bar's Fetch, a kit secondary Button (`workspace::action_frame`),
    /// Settings' Save (the kit's default Button, which draws its own border),
    /// a Settings switch row, unchecked, and a text field.
    const RESTING_CONTROLS: [&str; 6] = [
        "selected",
        "unselected",
        "secondary",
        "default",
        "switch",
        "field",
    ];

    /// `kind-surface`, the debug selector and ID [`RestingControls`] gives a
    /// control, made once.
    fn resting_selector(kind: &'static str, surface: &'static str) -> &'static str {
        thread_local! {
            static SELECTORS: std::cell::RefCell<
                std::collections::HashMap<(&'static str, &'static str), &'static str>,
            > = Default::default();
        }
        SELECTORS.with(|selectors| {
            *selectors
                .borrow_mut()
                .entry((kind, surface))
                .or_insert_with(|| disabled_selector(kind, surface))
        })
    }

    /// Each of [`RESTING_CONTROLS`], enabled and as the app builds it, in a
    /// row on each of [`more_contrast_edge_surfaces`] of the applied palette,
    /// selected as `kind-surface`, with a focus handle of its own.
    struct RestingControls {
        focus: std::collections::HashMap<&'static str, gpui::FocusHandle>,
        fields: Vec<gpui::Entity<gpui_kit::component::input::InputState>>,
    }

    impl RestingControls {
        fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
            use gpui_kit::component::input::InputState;
            use gpui_kit::{AppContext as _, Focusable as _};

            let (mut focus, mut fields) = (std::collections::HashMap::new(), Vec::new());
            for (surface, _) in more_contrast_edge_surfaces(palette(cx)) {
                for kind in RESTING_CONTROLS {
                    focus.insert(resting_selector(kind, surface), cx.focus_handle());
                }
                let field = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
                focus.insert(
                    resting_selector("field", surface),
                    field.read(cx).focus_handle(cx),
                );
                fields.push(field);
            }
            Self { focus, fields }
        }
    }

    impl gpui::Render for RestingControls {
        fn render(
            &mut self,
            _: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui_kit::component::button::Button;
            use gpui_kit::component::input::Input;
            use gpui_kit::{InteractiveElement as _, ParentElement as _, div};

            let surfaces = more_contrast_edge_surfaces(palette(cx));
            div()
                .flex()
                .flex_col()
                .children(surfaces.into_iter().zip(self.fields.clone()).map(
                    |((surface, color), field)| {
                        let id = move |kind: &'static str| resting_selector(kind, surface);
                        let focus = |kind: &'static str| self.focus[id(kind)].clone();
                        let selector = move |kind: &'static str| move || id(kind).into();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .p_2()
                            .bg(rgb(color))
                            .child(
                                crate::button(id("selected"), "Local", "", true)
                                    .track_focus(&focus("selected"))
                                    .debug_selector(selector("selected")),
                            )
                            .child(
                                crate::button(id("unselected"), "Remote", "", false)
                                    .track_focus(&focus("unselected"))
                                    .debug_selector(selector("unselected")),
                            )
                            .child(
                                crate::workspace::action_frame(id("secondary"))
                                    .label("Fetch")
                                    .track_focus(&focus("secondary"))
                                    .debug_selector(selector("secondary")),
                            )
                            .child(
                                // Mirrors `settings.rs`'s Save chain: the kit's default Button.
                                Button::new(id("default"))
                                    .label("Save")
                                    .track_focus(&focus("default"))
                                    .debug_selector(selector("default")),
                            )
                            .child(
                                div().w(px(260.)).debug_selector(selector("switch")).child(
                                    crate::settings::switch_row(
                                        id("switch"),
                                        crate::settings::FOLLOW_SYSTEM,
                                        crate::settings::FOLLOW_SYSTEM,
                                        "Omarchy follows your desktop theme",
                                        &[],
                                        cx,
                                    )
                                    .track_focus(&focus("switch")),
                                ),
                            )
                            .child(
                                div()
                                    .w(px(200.))
                                    .debug_selector(selector("field"))
                                    .child(Input::new(&field)),
                            )
                    },
                ))
        }
    }

    /// The pointer and focus states a control's resting edge is read in.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum EdgeState {
        Resting,
        Hovered,
        Pressed,
        Focused,
    }

    /// What a control painted, as [`painted_control`] reads it.
    type PaintedControl = (
        gpui::Bounds<Pixels>,
        gpui::Bounds<Pixels>,
        Vec<PaintedEdge>,
        (Vec<Background>, Vec<Hsla>),
    );

    /// The largest filled quad the last frame painted inside `bounds` and
    /// smaller than them: a switch row's track.
    fn inner_fill(
        cx: &mut gpui::VisualTestContext,
        bounds: gpui::Bounds<Pixels>,
    ) -> Option<gpui::Bounds<Pixels>> {
        use gpui::{Bounds, point};
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let device = px(1. / scale);
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .map(|quad| {
                    let b = quad.bounds;
                    Bounds::from_corners(
                        point(px(b.left().as_f32() / scale), px(b.top().as_f32() / scale)),
                        point(
                            px(b.right().as_f32() / scale),
                            px(b.bottom().as_f32() / scale),
                        ),
                    )
                })
                .filter(|drawn| {
                    drawn.left() >= bounds.left() - device
                        && drawn.top() >= bounds.top() - device
                        && drawn.right() <= bounds.right() + device
                        && drawn.bottom() <= bounds.bottom() + device
                        && drawn.size.width < bounds.size.width - device
                })
                .max_by(|a, b| {
                    let area = |b: &Bounds<Pixels>| b.size.width.as_f32() * b.size.height.as_f32();
                    area(a).total_cmp(&area(b))
                })
        })
    }

    /// What one frame painted for a control: its bounds, the frame its edge
    /// outlines (its bounds, or a switch row's track), the bordered quads
    /// inside its bounds, and its fills and the focus rings around that
    /// frame (`painted_button`), with the pointer away, over it, pressing it
    /// or away with the control focused.
    fn painted_control(
        cx: &mut gpui::VisualTestContext,
        selector: &'static str,
        state: EdgeState,
        focus: &gpui::FocusHandle,
    ) -> PaintedControl {
        use gpui::{Modifiers, MouseButton, point};

        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        draw(cx);
        let center = cx.debug_bounds(selector).expect(selector).center();
        let away = point(px(1.), px(1.));
        match state {
            EdgeState::Resting => cx.simulate_mouse_move(away, None, Modifiers::default()),
            EdgeState::Hovered => cx.simulate_mouse_move(center, None, Modifiers::default()),
            EdgeState::Pressed => {
                cx.simulate_mouse_move(center, None, Modifiers::default());
                cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::default());
            }
            EdgeState::Focused => {
                cx.simulate_mouse_move(away, None, Modifiers::default());
                cx.update(|window, cx| window.focus(focus, cx));
            }
        }
        draw(cx);
        let bounds = cx.debug_bounds(selector).expect(selector);
        let edges = painted_edges(cx, bounds);
        // A switch row's edge is its track's, and so is its ring.
        let frame = if selector.starts_with("switch") {
            inner_fill(cx, bounds).expect("a switch track")
        } else {
            bounds
        };
        let painted = painted_button(cx, frame);
        if state == EdgeState::Pressed {
            cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::default());
        }
        cx.simulate_mouse_move(away, None, Modifiers::default());
        cx.update(|window, cx| window.blur(cx));
        (bounds, frame, edges, painted)
    }

    /// The filled Buttons the kit gives no resting edge keep a boundary of
    /// their own: with More contrast on, in every palette, the danger
    /// Button's opaque `removed` fill (Delete theme, Discard and Force remove
    /// worktree, the only tinted variant the app draws) and the primary
    /// Button's opaque `accent` fill (Commit, Push and each dialog's default
    /// action) read at least 3:1 on the canvas, panel, subtle and raised
    /// surfaces, so each fill's own edge is its edge.
    #[gpui::test]
    fn more_contrast_leaves_the_danger_and_primary_fills_their_own_edge(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let mut failures = Vec::new();
        for (name, chosen) in test_palettes() {
            cx.update(|cx| {
                set_more_contrast(true, cx);
                chosen.apply(chosen.is_light(), None, cx);
                let applied = palette(cx);
                let tokens = &Theme::global(cx).tokens;
                for (variant, fill) in [
                    ("danger", tokens.button_danger.background.as_solid()),
                    ("primary", tokens.button_primary.background.as_solid()),
                ] {
                    let Some(fill) = fill.filter(|fill| fill.a >= 1.) else {
                        failures.push(format!("{name} {variant} fill {fill:?} is not opaque"));
                        continue;
                    };
                    for (surface, color) in more_contrast_edge_surfaces(applied) {
                        let ratio = contrast(packed(fill), color);
                        if ratio < MORE_CONTRAST_EDGE {
                            failures.push(format!(
                                "{name} {variant} #{:06x} on {surface} {ratio:.3}:1",
                                packed(fill)
                            ));
                        }
                    }
                }
            });
        }
        cx.update(|cx| set_more_contrast(false, cx));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The palettes the resting edge is drawn in: Midnight, Braden and, on
    /// Linux, Omarchy's default, Tokyo Night.
    fn resting_edge_palettes() -> Vec<(String, Palette)> {
        test_palettes()
            .into_iter()
            .filter(|(name, _)| {
                ["Midnight", "Braden", "Omarchy tokyo-night"].contains(&name.as_str())
            })
            .collect()
    }

    /// More contrast hands the kit its resting edge. With it on, in Midnight,
    /// Braden and Omarchy's Tokyo Night, the selected shared compact Button
    /// (History's active mode toggle), a secondary Button (Fetch), Settings'
    /// Save (a default Button, with its own border), an unchecked Settings
    /// switch's track and a text field each paint a 1 px edge over their
    /// bounds that reads 3:1 against the surface beneath, on the canvas,
    /// panel, subtle and raised surfaces at rest, and on the panel hovered,
    /// pressed and focused; the unselected compact Button, transparent at
    /// rest, paints none in any state. Every control keeps its bounds and its
    /// focus ring with it on and off, and off, the selected, secondary and
    /// switch controls paint no edge and each frame is the one drawn before
    /// More contrast was turned on. The switch's track is a fill, not an
    /// edge: in every state it paints the fill it paints off, and the thumb
    /// reads on it as it does off; filled with the adjusted `border`, it hid
    /// the edge and the thumb fell from 10.3:1 to 4.8:1 on it (Midnight).
    #[gpui::test]
    fn more_contrast_edges_the_controls_that_rest_on_a_fill(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(RestingControls::new);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.simulate_resize(gpui::size(px(1400.), px(600.)));
        let palettes = resting_edge_palettes();
        assert_eq!(
            palettes.len(),
            if cfg!(target_os = "linux") { 3 } else { 2 }
        );
        let mut failures = Vec::new();
        for (name, chosen) in palettes {
            let mut passes: Vec<Vec<(&str, &str, EdgeState, PaintedControl)>> = Vec::new();
            // The unchecked Switch's thumb in each pass, the kit's `switch_thumb`.
            let mut thumbs = Vec::new();
            for on in [false, true, false] {
                cx.update(|window, cx| {
                    set_more_contrast(on, cx);
                    chosen.apply(chosen.is_light(), Some(window), cx);
                });
                thumbs.push(cx.update(|_, cx| packed(Theme::global(cx).switch_thumb)));
                let mut drawn = Vec::new();
                for (surface, _) in more_contrast_edge_surfaces(chosen) {
                    let states: &[EdgeState] = if surface == "panel" {
                        &[
                            EdgeState::Resting,
                            EdgeState::Hovered,
                            EdgeState::Pressed,
                            EdgeState::Focused,
                        ]
                    } else {
                        &[EdgeState::Resting]
                    };
                    for kind in RESTING_CONTROLS {
                        let selector = disabled_selector(kind, surface);
                        let focus = view.read_with(cx, |view, _| view.focus[selector].clone());
                        for &state in states {
                            let painted = painted_control(cx, selector, state, &focus);
                            drawn.push((kind, surface, state, painted));
                        }
                    }
                }
                passes.push(drawn);
            }
            let edge = chosen.with_more_contrast().resting_edge();
            for ((before, on), after) in passes[0].iter().zip(&passes[1]).zip(&passes[2]) {
                let (kind, surface, state, (bounds, frame, edges, painted)) = on;
                let at = format!("{name} {kind} on {surface} {state:?}");
                if before.3 != after.3 {
                    failures.push(format!(
                        "{at}: off after on {:?}, before {:?}",
                        after.3, before.3
                    ));
                }
                let (off_bounds, off_frame, off_edges, off_painted) = &before.3;
                if (bounds, frame) != (off_bounds, off_frame) {
                    failures.push(format!(
                        "{at}: bounds {bounds:?} {frame:?}, off {off_bounds:?} {off_frame:?}"
                    ));
                }
                if painted.1 != off_painted.1 {
                    failures.push(format!(
                        "{at}: rings {:?}, off {:?}",
                        painted.1, off_painted.1
                    ));
                }
                // A field shows its focus in its own border, read below.
                if *state == EdgeState::Focused && *kind != "field" && painted.1.is_empty() {
                    failures.push(format!("{at}: draws no focus ring"));
                }
                // A track is a fill, not an edge: it rests and lifts as it
                // does off, and its thumb reads on it as it does off.
                if *kind == "switch" {
                    let track = |fills: &[Background]| {
                        fills.last().and_then(|fill| fill.as_solid()).map(packed)
                    };
                    let (thumb, off_thumb) = (thumbs[1], thumbs[0]);
                    match (track(&painted.0), track(&off_painted.0)) {
                        (Some(track), Some(off_track)) => {
                            let (ratio, off_ratio) =
                                (contrast(thumb, track), contrast(off_thumb, off_track));
                            if painted.0 != off_painted.0 || ratio < off_ratio {
                                failures.push(format!(
                                    "{at}: track #{track:06x}, its thumb {ratio:.3}:1; off \
                                     #{off_track:06x}, {off_ratio:.3}:1"
                                ));
                            }
                        }
                        fills => failures.push(format!("{at}: track fills {fills:?}")),
                    }
                }
                let kit_edged = matches!(*kind, "selected" | "secondary" | "switch");
                if *kind == "unselected" {
                    if !edges.is_empty() {
                        failures.push(format!("{at}: paints {edges:?}"));
                    }
                    continue;
                }
                if kit_edged && !off_edges.is_empty() {
                    failures.push(format!("{at}: off paints {off_edges:?}"));
                }
                let reading = edges.iter().find(|drawn| {
                    drawn.one_px
                        && drawn.beneath.is_some_and(|beneath| {
                            contrast(drawn.color, beneath) >= MORE_CONTRAST_EDGE
                        })
                });
                let Some(reading) = reading else {
                    failures.push(format!("{at}: no 1 px edge reading 3:1 in {edges:?}"));
                    continue;
                };
                // The kit's own layer is the palette's resting edge, over
                // the whole control or the switch's track.
                let over = |a: gpui::Bounds<Pixels>, b: gpui::Bounds<Pixels>| {
                    [
                        (a.left(), b.left()),
                        (a.top(), b.top()),
                        (a.right(), b.right()),
                        (a.bottom(), b.bottom()),
                    ]
                    .into_iter()
                    .all(|(a, b)| (a - b).abs() < px(0.5))
                };
                if kit_edged && (reading.color != edge || !over(reading.bounds, *frame)) {
                    failures.push(format!(
                        "{at}: edge {reading:?}, not #{edge:06x} over {frame:?}"
                    ));
                }
            }
        }
        cx.update(|window, cx| {
            set_more_contrast(false, cx);
            ThemeChoice::Midnight.apply(Some(window), cx);
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Under More contrast the editor highlights keep the rules decided on
    /// 2026-11-05, in every palette and in [`faint_custom_palette`], a
    /// palette made from a built-in; a word steps across its line on each
    /// side outside the Omarchy palettes, so a run without them (macOS)
    /// reads the step on both sides. Each changed-word tint stands beyond
    /// its line tint from the canvas: further from it in OKLab lightness, on
    /// the same side, and at least [`HIGHLIGHT_DISTANCE`] from the line, with
    /// every color drawn on it as text (`text`, every syntax role but
    /// `removed` and the patch text the Diff view draws on it) reading the
    /// text rule; a word that stepped across, one that does not stand beyond
    /// its line as lifted ([`stands_beyond`]: drawn off on its canvas side, or
    /// left nearer it than [`HIGHLIGHT_DISTANCE`] by its line's move), stands
    /// [`HIGHLIGHT_LIGHTNESS_GAP`] beyond its line in lightness, or short of it only where no step that
    /// far reads the text rule ([`the_gap_cannot_read`]: One Dark's added
    /// word, whose raised keyword would read 4.39:1).
    /// Find's other matches stand [`HIGHLIGHT_DISTANCE`] and
    /// [`HIGHLIGHT_LIGHTNESS_GAP`] in lightness from the selection, the
    /// current match and both word tints, [`HIGHLIGHT_DISTANCE`] from every
    /// editor line, and every color drawn on them as text (`text` and every
    /// role, the patch text among them) reads the text rule; and
    /// the selection and the current match stand as far apart, with the
    /// gap. With More contrast off, 31 word tints across the palettes lay on
    /// their line's canvas side (Braden's removed word `#fdf7f8` on its line
    /// `#f9e5eb`), Find's other matches drew the selection's colour in every
    /// palette, and the selection and current match stood 0.003 apart in
    /// Sandstone and 0.044 apart in One Dark by chroma alone; held to its
    /// distance from its line alone, Solarized Dark's added word stepped
    /// across to 0.0004 beyond it.
    #[gpui::test]
    fn more_contrast_keeps_the_highlight_rules_in_every_palette(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let distance = |a: u32, b: u32| oklab_distance(oklab(a), oklab(b));
        let lightness = |a: u32, b: u32| (oklab(a)[0] - oklab(b)[0]).abs();
        let mut failures = Vec::new();
        let mut crossed_in = [Vec::new(), Vec::new()];
        for (name, chosen) in more_contrast_palettes() {
            let (applied, on) = cx.update(|cx| {
                set_more_contrast(true, cx);
                chosen.apply(chosen.is_light(), None, cx);
                (palette(cx), editor_highlights(cx))
            });
            let roles = applied.syntax_roles(Some(chosen));
            // The words as lifted, or drawn off on the canvas side, before
            // any steps across.
            let lifted = words_lines_move_for(chosen);
            for (index, side, (diff, line), word, drawn) in [
                (
                    0,
                    "added",
                    (chosen.added, applied.added_background),
                    on.added_word,
                    [on.patch_text.added],
                ),
                (
                    1,
                    "removed",
                    (chosen.removed, applied.removed_background),
                    on.removed_word,
                    [on.patch_text.removed],
                ),
            ] {
                // Short of standing beyond its line as the line was moved
                // for it, drawn off on its canvas side or left nearer it than
                // [`HIGHLIGHT_DISTANCE`] by its line's move, the word stepped
                // across.
                let crossed = !stands_beyond(lifted[index], line, chosen);
                let beyond = lightness_beyond(word, line, chosen);
                let colors: Vec<u32> = roles.word_colors().into_iter().chain(drawn).collect();
                let short = crossed
                    && beyond < HIGHLIGHT_LIGHTNESS_GAP
                    && !the_gap_cannot_read(&colors, diff, line, chosen);
                if beyond <= 0. || short {
                    failures.push(format!(
                        "{name} {side} word #{word:06x} {beyond:+.4} beyond its line \
                         #{line:06x}{}",
                        if crossed { ", stepped across" } else { "" }
                    ));
                }
                if distance(word, line) < HIGHLIGHT_DISTANCE {
                    failures.push(format!(
                        "{name} {side} word #{word:06x} {:.4} from its line #{line:06x}",
                        distance(word, line)
                    ));
                }
                for color in colors {
                    if contrast(color, word) < LABEL_RULE {
                        failures.push(format!(
                            "{name} #{color:06x} on the {side} word #{word:06x} {:.3}:1",
                            contrast(color, word)
                        ));
                    }
                }
                if crossed {
                    crossed_in[index].push(name.clone());
                }
            }
            for (other, color) in [
                ("selection", on.selection),
                ("current match", on.find_current),
                ("added word", on.added_word),
                ("removed word", on.removed_word),
            ] {
                if distance(on.find_match, color) < HIGHLIGHT_DISTANCE
                    || lightness(on.find_match, color) < HIGHLIGHT_LIGHTNESS_GAP
                {
                    failures.push(format!(
                        "{name} Find match #{:06x} {:.4} from the {other} #{color:06x}, {:.4} in \
                         lightness",
                        on.find_match,
                        distance(on.find_match, color),
                        lightness(on.find_match, color)
                    ));
                }
            }
            let lines = EditorHighlights::lines(applied);
            for line in lines {
                if distance(on.find_match, line) < HIGHLIGHT_DISTANCE {
                    failures.push(format!(
                        "{name} Find match #{:06x} {:.4} from the line #{line:06x}",
                        on.find_match,
                        distance(on.find_match, line)
                    ));
                }
            }
            for color in roles.colors() {
                if contrast(color, on.find_match) < LABEL_RULE {
                    failures.push(format!(
                        "{name} #{color:06x} on the Find match #{:06x} {:.3}:1",
                        on.find_match,
                        contrast(color, on.find_match)
                    ));
                }
            }
            if distance(on.selection, on.find_current) < HIGHLIGHT_DISTANCE
                || lightness(on.selection, on.find_current) < HIGHLIGHT_LIGHTNESS_GAP
            {
                failures.push(format!(
                    "{name} current match #{:06x} {:.4} from the selection #{:06x}, {:.4} in \
                     lightness",
                    on.find_current,
                    distance(on.selection, on.find_current),
                    on.selection,
                    lightness(on.selection, on.find_current)
                ));
            }
        }
        cx.update(|cx| set_more_contrast(false, cx));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        // Words drawn off on their line's canvas side step across on both
        // sides, in the built-in palettes and the palette made from one.
        for (side, names) in ["added", "removed"].into_iter().zip(&crossed_in) {
            assert!(
                names.iter().any(|name| !name.starts_with("Omarchy ")),
                "no {side} word drawn off on its line's canvas side outside the Omarchy \
                 palettes: {names:?}"
            );
        }
    }

    /// With More contrast off, the selection, Find's other and current
    /// matches and the changed-word tints draw exactly the colours they drew
    /// before More contrast's highlight rules (a dark and a light palette),
    /// and no changed word draws a rule: Off is the palette's tuned look and
    /// runs none of those rules.
    #[gpui::test]
    fn more_contrast_off_draws_the_highlights_as_before(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        for (choice, drawn) in [
            (
                ThemeChoice::Midnight,
                [0x0b3e3e, 0x0b3e3e, 0x004836, 0x114939, 0x5f3241],
            ),
            (
                ThemeChoice::Daylight,
                [0xbff6e7, 0xbff6e7, 0x93fde6, 0xc3f5de, 0xfcf6f7],
            ),
        ] {
            let highlights = cx.update(|cx| {
                set_more_contrast(false, cx);
                choice.apply(None, cx);
                editor_highlights(cx)
            });
            assert_eq!(
                [
                    highlights.selection,
                    highlights.find_match,
                    highlights.find_current,
                    highlights.added_word,
                    highlights.removed_word,
                ],
                drawn,
                "{choice:?}: selection, Find match, current match, added and removed words"
            );
            assert_eq!(
                (highlights.added_rule, highlights.removed_rule),
                (None, None),
                "{choice:?}: no changed-word rule"
            );
        }
    }

    /// A hovered row never reads heavier than the selected row, in every
    /// built-in and Omarchy palette and in [`faint_custom_palette`], with
    /// More contrast off and on: on the canvas, the panel and `subtle`,
    /// `hover` stands nearer than `selected` in OKLab lightness, in ΔEOK and
    /// in WCAG luminance contrast, and both move from each surface the same
    /// way, away from the canvas (darker in a light palette, lighter in a
    /// dark one), in OKLab lightness and in luminance, so hover over
    /// `subtle` never turns toward the canvas. One Light's hover stood 0.020
    /// further from its canvas in lightness than its selected row, Kanagawa
    /// Wave's 0.011 and Kanagawa Lotus's 0.0015, until One Light's
    /// `selected` (`#e9edff` to `#e0e4f6`), Kanagawa Wave's `hover`
    /// (`#363646` to `#323242`) and Kanagawa Lotus's `selected` (`#c7d7e0` to
    /// `#c4d4dd`) were tuned; Lotus's hover lightened to `#ede6bc` instead
    /// stood lighter than its `subtle` (`#e5ddb0`), so a hovered row on it
    /// turned toward the canvas, 1.051:1 from the canvas where the selected
    /// row stands 1.230:1.
    #[gpui::test]
    fn a_hovered_row_never_reads_heavier_than_the_selected_row(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut failures = Vec::new();
        for (name, chosen) in more_contrast_palettes() {
            for more in [false, true] {
                let applied = cx.update(|cx| {
                    set_more_contrast(more, cx);
                    chosen.apply(chosen.is_light(), None, cx);
                    palette(cx)
                });
                let (hover, selected) = (applied.hover, applied.selected);
                for (surface, background) in [
                    ("canvas", applied.canvas),
                    ("panel", applied.panel),
                    ("subtle", applied.subtle),
                ] {
                    let lightness = |color: u32| (oklab(color)[0] - oklab(background)[0]).abs();
                    let apart = |color: u32| oklab_distance(oklab(color), oklab(background));
                    let ratio = |color: u32| contrast(color, background);
                    if lightness(hover) >= lightness(selected)
                        || apart(hover) >= apart(selected)
                        || ratio(hover) >= ratio(selected)
                    {
                        failures.push(format!(
                            "{name}, More contrast {more}: hover #{hover:06x} {:.4} from the \
                             {surface} #{background:06x} in lightness, {:.4} in ΔEOK and \
                             {:.4}:1, the selected row #{selected:06x} {:.4}, {:.4} and {:.4}:1",
                            lightness(hover),
                            apart(hover),
                            ratio(hover),
                            lightness(selected),
                            apart(selected),
                            ratio(selected)
                        ));
                    }
                    // Away from the canvas: darker in a light palette, lighter in a dark one.
                    let side = if applied.is_light() {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Greater
                    };
                    let away = |color: u32| {
                        oklab(color)[0].total_cmp(&oklab(background)[0]) == side
                            && luminance(color).total_cmp(&luminance(background)) == side
                    };
                    for (state, color) in [("hover", hover), ("selected row", selected)] {
                        if !away(color) {
                            failures.push(format!(
                                "{name}, More contrast {more}: the {state} #{color:06x} moves \
                                 from the {surface} #{background:06x} toward the canvas \
                                 #{:06x}",
                                applied.canvas
                            ));
                        }
                    }
                }
            }
        }
        cx.update(|cx| set_more_contrast(false, cx));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
