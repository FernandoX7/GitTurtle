//! Native GPUI presentation choices shared by history, previews, and settings.
//! Pure theme tokens, contrast/OKLab fitting, custom themes, and Omarchy palette
//! parsing live in [`gitturtle_theme`].

#[cfg(any(target_os = "linux", test))]
pub use gitturtle_theme::desktop_text_scale_value;
pub use gitturtle_theme::{
    CODE_TEXT_RANGE, DEFAULT_CODE_TEXT_SIZE, DEFAULT_INTERFACE_TEXT_SIZE, Density,
    EditorHighlights, INTERFACE_TEXT_RANGE, Labels, Palette, REF_TINT_ALPHA, RgbaColor,
    SyntaxRoles, ThemeChoice, code_text_size, custom, desktop_text_scale, store_text_sizes,
    ui_scale,
};
#[cfg(test)]
pub(crate) use gitturtle_theme::{
    DIALOG_BACKDROP_MAX_PERCENT, DIALOG_EDGE, DIALOG_LIFT, DISABLED_EDGE, DISABLED_LABEL,
    DISABLED_LABEL_MARGIN, DISABLED_SHARE, HIGHLIGHT_DISTANCE, HIGHLIGHT_LIGHTNESS_GAP,
    LABEL_MARGIN, LABEL_RULE, MORE_CONTRAST_DISABLED, MORE_CONTRAST_DISABLED_SHARE,
    MORE_CONTRAST_EDGE, MORE_CONTRAST_TEXT, POPUP_EDGE, SCROLL_THUMB, WORD_LIGHTNESS_GAP,
    backdrop_over, composite as composite_color, drawn_nearer,
    highlights::{
        GREY_CHROMA, HighlightFit, beyond_line, colour_blind_lightness, drawn_target, from_linear,
        keeps_colour_blind_gaps, lightness_beyond, lightness_lifts, line_moves, linear_in_gamut,
        mix, stands_beyond,
    },
    oklab, oklab_distance,
};
#[cfg(target_os = "linux")]
use gitturtle_theme::{interface_text_size, store_desktop_text_scale};
#[cfg(test)]
pub(crate) use gitturtle_theme::{sources, test_palettes};

// Linux only: the desktop's current Omarchy theme global and filesystem watcher.
#[cfg(target_os = "linux")]
pub(crate) mod omarchy;

use gpui_kit::component::button::{ButtonCustomVariant, ButtonVariant};
use gpui_kit::component::highlighter::{SyntaxColors, ThemeStyle};
use gpui_kit::component::{Colorize, FocusRing, Theme, ThemeMode};
use gpui_kit::{App, FontFeatures, Global, Pixels, Rgba, StyleRefinement, Styled, Window, px, rgb};
use std::cell::Cell;
use std::sync::{Arc, LazyLock};

/// Convert a pure [`RgbaColor`] from [`gitturtle_theme`] into a GPUI [`Rgba`].
pub(crate) fn gpui_rgba(color: RgbaColor) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

/// Composite a GPUI [`Rgba`] layer over an opaque `0xRRGGBB` surface.
#[cfg(test)]
pub(crate) fn composite(layer: Rgba, surface: u32) -> u32 {
    composite_color(
        RgbaColor {
            r: layer.r,
            g: layer.g,
            b: layer.b,
            a: layer.a,
        },
        surface,
    )
}

/// GPUI global wrapper for the applied [`Palette`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GlobalPalette(pub Palette);
impl Global for GlobalPalette {}

/// GPUI global wrapper for the fitted [`Labels`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GlobalLabels(pub Labels);
impl Global for GlobalLabels {}

/// GPUI global wrapper for the fitted [`EditorHighlights`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GlobalEditorHighlights(pub EditorHighlights);
impl Global for GlobalEditorHighlights {}

/// Per-app notification for retained workspace viewports. Observers keep their
/// last applied factor so coalesced desktop updates are applied exactly once.
#[cfg(any(target_os = "linux", test))]
pub(super) struct DesktopTextScale(pub f32);
#[cfg(any(target_os = "linux", test))]
impl Global for DesktopTextScale {}

/// Stores the desktop factor and re-applies the text sizes of every open
/// window. Returns whether the factor changed.
#[cfg(target_os = "linux")]
pub fn set_desktop_text_scale(scale: f32, cx: &mut App) -> bool {
    let scale = desktop_text_scale_value(scale);
    if !store_desktop_text_scale(scale) {
        return false;
    }
    cx.set_global(DesktopTextScale(scale));
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, cx| {
            apply_text_sizes(interface_text_size(), code_text_size(), window, cx)
        });
    }
    true
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
    store_text_sizes(interface, code);
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
            [fill(gpui_rgba(palette.control_fill(palette.hover)))],
            "{name} {state:?} paints {fills:?}"
        ),
        UnselectedState::Pressed => assert_eq!(
            fills,
            [fill(gpui_rgba(palette.control_fill(palette.selected)))],
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

fn palette_control_button(palette: Palette, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(rgb(palette.control_label()).into())
        .hover(gpui_rgba(palette.control_fill(palette.hover)).into())
        .active(gpui_rgba(palette.control_fill(palette.selected)).into())
}

fn palette_still_control_button(palette: Palette, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(rgb(palette.control_label()).into())
        .active(gpui_rgba(palette.control_fill(palette.selected)).into())
}

fn palette_selected_control_button(palette: Palette, cx: &App) -> Option<ButtonCustomVariant> {
    let label = palette.control_label();
    (label != palette.text).then(|| {
        ButtonCustomVariant::new(cx)
            .foreground(rgb(label).into())
            .active(rgb(palette.selected).into())
            .disabled(rgb(palette.subtle).into())
    })
}

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

/// The applied palette's [`Labels`].
pub fn labels(cx: &App) -> Labels {
    let palette = palette(cx);
    cx.try_global::<GlobalLabels>()
        .map(|labels| labels.0)
        .filter(|labels| labels.palette == palette)
        .unwrap_or_else(|| Labels::fit(palette))
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
    cx.try_global::<GlobalPalette>()
        .map(|palette| palette.0)
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

/// GPUI application helpers for [`ThemeChoice`].
#[cfg(test)]
pub trait ThemeChoiceExt {
    fn apply(self, window: Option<&mut Window>, cx: &mut App);
}

#[cfg(test)]
impl ThemeChoiceExt for ThemeChoice {
    fn apply(self, window: Option<&mut Window>, cx: &mut App) {
        custom::ResolvedTheme::built_in(self).apply(window, cx);
    }
}

/// GPUI desktop-awareness helpers for [`custom::ThemeSelection`].
pub trait ThemeSelectionExt {
    fn on_desktop(self, cx: &App) -> Self;
}

impl ThemeSelectionExt for custom::ThemeSelection {
    fn on_desktop(self, cx: &App) -> Self {
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

/// Cached macOS system accent color (`NSColor.controlAccentColor` converted to sRGB `0xRRGGBB`).
/// Updated at startup and on `NSSystemColorsDidChangeNotification`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MacSystemAccent(Option<u32>);
impl Global for MacSystemAccent {}

/// Reads the current macOS system accent color as sRGB `0xRRGGBB`.
#[cfg(target_os = "macos")]
fn read_macos_system_accent() -> Option<u32> {
    objc2::rc::autoreleasepool(|_| {
        use objc2_app_kit::{NSColor, NSColorSpace};
        let accent = NSColor::controlAccentColor();
        let srgb = accent.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
        let channel = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
        let r = channel(srgb.redComponent());
        let g = channel(srgb.greenComponent());
        let b = channel(srgb.blueComponent());
        Some((r << 16) | (g << 8) | b)
    })
}

/// Refreshes the cached macOS system accent color in the GPUI global store.
/// Returns whether the stored value changed.
pub fn sync_macos_system_accent(cx: &mut App) -> bool {
    #[cfg(target_os = "macos")]
    let next = read_macos_system_accent();
    #[cfg(not(target_os = "macos"))]
    let next = None;
    let current = cx.try_global::<MacSystemAccent>().and_then(|g| g.0);
    if current == next {
        return false;
    }
    cx.set_global(MacSystemAccent(next));
    true
}

#[cfg(test)]
pub(crate) fn set_macos_system_accent_for_test(accent: Option<u32>, cx: &mut App) {
    cx.set_global(MacSystemAccent(accent));
}

/// GPUI application helpers for [`custom::ResolvedTheme`].
pub trait ResolvedThemeExt {
    fn apply(self, window: Option<&mut Window>, cx: &mut App);
    fn with_desktop(self, cx: &App) -> Self;
    fn with_system_accent(self, enabled: bool, cx: &App) -> Self;
}

impl ResolvedThemeExt for custom::ResolvedTheme {
    fn apply(self, window: Option<&mut Window>, cx: &mut App) {
        self.palette.apply(self.is_light, window, cx);
    }

    fn with_desktop(self, cx: &App) -> Self {
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

    fn with_system_accent(mut self, enabled: bool, cx: &App) -> Self {
        if enabled && let Some(raw_accent) = cx.try_global::<MacSystemAccent>().and_then(|g| g.0) {
            self.palette = gitturtle_theme::with_fitted_accent(self.palette, raw_accent);
        }
        self
    }
}

/// GPUI application helpers for [`Palette`].
pub trait PaletteExt: Sized {
    fn apply(self, is_light: bool, window: Option<&mut Window>, cx: &mut App);
    #[cfg(any(test, doc, target_os = "linux"))]
    #[expect(clippy::wrong_self_convention)]
    fn as_applied(self, cx: &App) -> Self;
    #[cfg(test)]
    fn configured_theme(self, is_light: bool) -> Theme;
}

impl PaletteExt for Palette {
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
    fn apply(self, is_light: bool, window: Option<&mut Window>, cx: &mut App) {
        let more_contrast = more_contrast(cx);
        let (palette, roles, highlights) = self.fitted(more_contrast);
        let mode = if is_light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        {
            let theme = Theme::global_mut(cx);
            theme.mode = mode;
            let config = if mode.is_dark() {
                theme.dark_theme.clone()
            } else {
                theme.light_theme.clone()
            };
            theme.apply_config(&config);
            configure_palette(
                palette,
                is_light,
                more_contrast,
                self,
                roles,
                highlights,
                theme,
            );
            theme.tokens = (&theme.colors).into();
        }
        CONTROL_BUTTON.set(Some(ControlButton {
            idle: palette_control_button(palette, cx),
            still: palette_still_control_button(palette, cx),
            selected: palette_selected_control_button(palette, cx),
            on_fill: ButtonCustomVariant::new(cx).foreground(rgb(palette.control_label()).into()),
            selected_hover: palette.row_hover(true),
        }));
        cx.set_global(GlobalEditorHighlights(highlights));
        cx.set_global(GlobalLabels(Labels::fit(palette)));
        cx.set_global(GlobalPalette(palette));
        Theme::sync_base(cx);
        if let Some(window) = window {
            window.refresh();
        }
    }

    #[cfg(any(test, doc, target_os = "linux"))]
    fn as_applied(self, cx: &App) -> Self {
        if more_contrast(cx) {
            self.with_more_contrast()
        } else {
            self
        }
    }

    #[cfg(test)]
    fn configured_theme(self, is_light: bool) -> Theme {
        use gpui_kit::component::highlighter::HighlightTheme;
        let mut theme = Theme::default();
        theme.highlight_theme = if is_light {
            HighlightTheme::default_light()
        } else {
            HighlightTheme::default_dark()
        };
        let roles = self.syntax_roles(None);
        configure_palette(
            self,
            is_light,
            false,
            self,
            roles,
            EditorHighlights::fit(self, roles, None),
            &mut theme,
        );
        theme.tokens = (&theme.colors).into();
        theme
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
fn configure_palette(
    palette: Palette,
    is_light: bool,
    more_contrast: bool,
    chosen: Palette,
    roles: SyntaxRoles,
    highlights: EditorHighlights,
    theme: &mut Theme,
) {
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
    theme.colors.list_highlight_foreground = rgb(palette.accent).into();
    theme.colors.chart_grid = rgb(palette.border).into();
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
        gpui_rgba(if more_contrast {
            palette.more_contrast_disabled_label(chosen)
        } else {
            palette.disabled_label()
        })
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
    theme.ghost_hover = Some(gpui_rgba(palette.control_fill(palette.hover)).into());
    theme.ghost_active = Some(gpui_rgba(palette.control_fill(palette.selected)).into());
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
    syntax.style.syntax = syntax_colors(roles, &syntax.style.syntax);
    set_theme_text_sizes(theme);
    theme.radius = px(7.);
    theme.radius_lg = px(12.);
    // GPUI Kit 0.6 paints component backgrounds from resolved ThemeTokens,
    // while foregrounds still read ThemeColor. Sync both representations
    // before Theme::sync_base propagates them to native controls.
    theme.tokens = (&theme.colors).into();
}

/// The toolkit's syntax styles with each field's color taken from its role; italic and
/// weight stay as the toolkit set them. The literal names every field, so a field a later
/// toolkit adds fails to compile until it has a role. A capture without a field of its own
/// takes the field of its first segment (`variable.builtin` takes `variable`) or, with none,
/// the editor foreground, which is `text`, so every capture a grammar emits draws in a
/// palette color.
fn syntax_colors(roles: SyntaxRoles, kit: &SyntaxColors) -> SyntaxColors {
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
    ] = roles.colors().map(Some);
    let mut colors = kit.clone();
    colors.keyword = restyle(kit.keyword, keyword);
    colors.preproc = restyle(kit.preproc, keyword);
    colors.string = restyle(kit.string, string);
    colors.string_escape = restyle(kit.string_escape, string);
    colors.string_regex = restyle(kit.string_regex, string);
    colors.string_special = restyle(kit.string_special, string);
    colors.string_special_symbol = restyle(kit.string_special_symbol, string);
    colors.text_literal = restyle(kit.text_literal, string);
    colors.text_code_span = restyle(kit.text_code_span, string);
    colors.number = restyle(kit.number, constant);
    colors.boolean = restyle(kit.boolean, constant);
    colors.constant = restyle(kit.constant, constant);
    colors.type_ = restyle(kit.type_, type_);
    colors.constructor = restyle(kit.constructor, type_);
    colors.diff_context = restyle(kit.diff_context, text);
    colors.diff_header = restyle(kit.diff_header, text);
    colors.diff_hunk = restyle(kit.diff_hunk, function);
    colors.diff_minus = restyle(kit.diff_minus, removed);
    colors.diff_plus = restyle(kit.diff_plus, string);
    colors.enum_ = restyle(kit.enum_, type_);
    colors.variant = restyle(kit.variant, type_);
    colors.function = restyle(kit.function, function);
    colors.tag = restyle(kit.tag, markup);
    colors.tag_doctype = restyle(kit.tag_doctype, markup);
    colors.attribute = restyle(kit.attribute, markup);
    colors.property = restyle(kit.property, markup);
    colors.link_text = restyle(kit.link_text, markup);
    colors.link_uri = restyle(kit.link_uri, markup);
    colors.label = restyle(kit.label, markup);
    colors.title = restyle(kit.title, markup);
    colors.comment = restyle(kit.comment, comment);
    colors.comment_doc = restyle(kit.comment_doc, comment);
    colors.hint = restyle(kit.hint, comment);
    colors.predictive = restyle(kit.predictive, comment);
    colors.variable = restyle(kit.variable, text);
    colors.variable_special = restyle(kit.variable_special, text);
    colors.embedded = restyle(kit.embedded, text);
    colors.operator = restyle(kit.operator, text);
    colors.punctuation = restyle(kit.punctuation, text);
    colors.punctuation_bracket = restyle(kit.punctuation_bracket, text);
    colors.punctuation_delimiter = restyle(kit.punctuation_delimiter, text);
    colors.punctuation_list_marker = restyle(kit.punctuation_list_marker, text);
    colors.punctuation_special = restyle(kit.punctuation_special, text);
    colors.primary = restyle(kit.primary, text);
    // Emphasis keeps its italic or weight in the color around it.
    colors.emphasis = restyle(kit.emphasis, None);
    colors.emphasis_strong = restyle(kit.emphasis_strong, None);
    colors
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

/// The applied palette's editor highlights.
pub fn editor_highlights(cx: &App) -> EditorHighlights {
    cx.try_global::<GlobalEditorHighlights>()
        .map(|highlights| highlights.0)
        .unwrap_or_else(|| palette(cx).fitted(more_contrast(cx)).2)
}

#[cfg(test)]
#[path = "appearance_tests.rs"]
mod tests;
