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

/// The widest digit advance of code text at the current code size, so a
/// line-number column can fit its numbers in the [`CodeFont`] family as the
/// code size changes. Glyph advances come from the text system's cache.
pub fn code_digit_width(cx: &App) -> Pixels {
    let family = Theme::global(cx).mono_font_family.clone();
    let font = gpui_kit::Font {
        features: code_font_features_for(&family),
        ..gpui_kit::font(family)
    };
    let text = cx.text_system();
    let face = text.resolve_font(&font);
    let size = code_text();
    ('0'..='9')
        .filter_map(|digit| text.advance(face, size, digit).ok())
        .map(|advance| advance.width)
        .fold(
            px(0.),
            |widest, width| if width > widest { width } else { widest },
        )
}

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
    let theme = Theme::global_mut(cx);
    theme.font_size = ui_text(13.);
    theme.mono_font_size = code_text();
    Theme::sync_base(cx);
    // Root uses font_size for rem geometry, so native control padding and
    // heights grow together with explicit app text and custom canvas rows.
    window.set_rem_size(ui_text(13.));
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
    /// Only for a palette whose [`Palette::control_label`] is not `text`.
    selected: Option<ButtonCustomVariant>,
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
        (0..=32)
            .map(|step| {
                [16, 8, 0].into_iter().fold(0, |color, shift| {
                    let from = (self.text >> shift) & 0xff;
                    let to = (extreme >> shift) & 0xff;
                    color | (((from * (32 - step) + to * step) / 32) << shift)
                })
            })
            .find(|&label| readable(label, LABEL_RULE + LABEL_MARGIN))
            .unwrap_or(extreme)
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

    /// The selected shared button in a palette whose control label moved: the
    /// secondary's `selected` surface with that label. The kit's secondary
    /// paints its label in `text` for every button, and a text color on the
    /// button itself would outlast its disabled state, because the kit replays
    /// the caller's style over the disabled colors.
    fn selected_control_button(self, cx: &App) -> Option<ButtonCustomVariant> {
        let label = self.control_label();
        (label != self.text).then(|| {
            ButtonCustomVariant::new(cx)
                .foreground(rgb(label).into())
                .active(rgb(self.selected).into())
        })
    }
}

/// The text rule, and the margin tuned tokens keep above it because small text
/// at 1x renders about 0.2 lower (DESIGN.md, semantic palette ownership).
const LABEL_RULE: f64 = 4.5;
const LABEL_MARGIN: f64 = 0.25;

/// Composite a translucent layer over an opaque surface as GPUI's renderers
/// blend a quad: on the gamma-encoded values of their non-sRGB targets.
fn composite(layer: Rgba, surface: u32) -> u32 {
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

pub fn palette(cx: &App) -> Palette {
    cx.try_global::<Palette>()
        .copied()
        .unwrap_or_else(|| ThemeChoice::default().palette())
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
                hover: 0xe5e9f4,
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
                hover: 0xe7ecf3,
                border: 0xd2dce8,
                text: 0x253247,
                muted: 0x526279,
                accent: 0x08755d,
                accent_foreground: 0xffffff,
                accent_hover: 0x06654f,
                accent_active: 0x055440,
                selected: 0xdaece6,
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
                    hover: 0xf1f1f3,
                    selected: 0xe9edff,
                    border: 0xd3d3d6,
                    text: o::MONO_1,
                    // Tuned: mono-2 darkened for 4.5:1 on selected rows.
                    muted: 0x5c5f69,
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
                    hover: r::OVERLAY,
                    selected: 0xe8dfe2,
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
                    hover: 0xefebdb,
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
                    hover: k::SUMI_INK_5,
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
                    selected: k::LOTUS_BLUE_1,
                    border: k::LOTUS_WHITE_0,
                    text: k::LOTUS_INK_1,
                    // Tuned: lotusGray2 darkened for 4.5:1 on every row surface and diff tile.
                    muted: 0x5a574d,
                    accent: k::LOTUS_BLUE_4,
                    accent_foreground: k::LOTUS_WHITE_3,
                    accent_hover: 0x435c89,
                    accent_active: 0x3a5077,
                    // Tuned: lotusGreen2, lotusRed2, lotusYellow3 and lotusOrange2 darkened for
                    // 3:1 on row surfaces, 4.5:1 in diff tiles, the canvas label on fills and
                    // warning text on subtle surfaces.
                    added: 0x49613e,
                    removed: 0xa72428,
                    modified: 0x936300,
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
    pub fn apply(self, is_light: bool, window: Option<&mut Window>, cx: &mut App) {
        Theme::change(
            if is_light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            },
            None,
            cx,
        );
        let roles = self.syntax_roles();
        let highlights = EditorHighlights::fit(self, roles);
        self.configure(is_light, roles, highlights, Theme::global_mut(cx));
        CONTROL_BUTTON.set(Some(ControlButton {
            idle: self.control_button(cx),
            selected: self.selected_control_button(cx),
            selected_hover: self.row_hover(true),
        }));
        cx.set_global(highlights);
        cx.set_global(self);
        Theme::sync_base(cx);
        if let Some(window) = window {
            window.refresh();
        }
    }

    fn configure(
        self,
        is_light: bool,
        roles: SyntaxRoles,
        highlights: EditorHighlights,
        theme: &mut Theme,
    ) {
        let palette = self;
        theme.colors.background = rgb(palette.canvas).into();
        theme.colors.foreground = rgb(palette.text).into();
        theme.colors.muted = rgb(palette.hover).into();
        theme.colors.muted_foreground = rgb(palette.muted).into();
        theme.colors.primary = rgb(palette.accent).into();
        theme.colors.primary_foreground = rgb(palette.accent_foreground).into();
        theme.colors.primary_hover = rgb(palette.accent_hover).into();
        theme.colors.primary_active = rgb(palette.accent_active).into();
        theme.colors.border = rgb(palette.border).into();
        theme.colors.input = rgb(palette.border).into();
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
        theme.colors.popover = rgb(palette.panel).into();
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
        theme.colors.scrollbar_thumb = rgb(palette.border).into();
        theme.colors.scrollbar_thumb_hover = rgb(palette.muted).into();
        theme.colors.switch = rgb(palette.border).into();
        theme.colors.switch_thumb = rgb(palette.text).into();
        theme.colors.progress_bar = rgb(palette.accent).into();
        theme.colors.skeleton = rgb(palette.hover).into();
        theme.colors.ring = rgb(palette.accent).into();
        theme.button_focus_ring = BUTTON_FOCUS_RING;
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
        theme.font_size = ui_text(13.);
        theme.mono_font_size = code_text();
        theme.radius = px(7.);
        theme.radius_lg = px(12.);
        // GPUI Kit 0.6 paints component backgrounds from resolved ThemeTokens,
        // while foregrounds still read ThemeColor. Sync both representations
        // before Theme::sync_base propagates them to native controls.
        theme.tokens = (&theme.colors).into();
    }

    /// Each syntax role's palette color fitted by [`SyntaxTargets::fit`], measured once per
    /// application.
    fn syntax_roles(self) -> SyntaxRoles {
        let targets = SyntaxTargets::new(self);
        SyntaxRoles {
            keyword: targets.fit(self.renamed),
            string: targets.fit(self.added),
            constant: targets.fit(self.warning),
            type_: targets.fit(self.modified),
            function: targets.fit(self.hunk),
            markup: targets.fit(self.accent),
            comment: targets.fit(self.muted),
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
        let roles = self.syntax_roles();
        self.configure(
            is_light,
            roles,
            EditorHighlights::fit(self, roles),
            &mut theme,
        );
        theme
    }

    /// The editor highlights [`Self::apply`] fits for this palette, without an application.
    #[cfg(test)]
    pub(crate) fn editor_highlights(self) -> EditorHighlights {
        EditorHighlights::fit(self, self.syntax_roles())
    }
}

/// The palette color each syntax role draws in once fitted: `keyword` from `renamed`, `string`
/// from `added`, `constant` from `warning`, `type_` from `modified`, `function` from `hunk`,
/// `markup` from `accent`, `comment` from `muted`, and `text` as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SyntaxRoles {
    keyword: u32,
    string: u32,
    constant: u32,
    type_: u32,
    function: u32,
    markup: u32,
    comment: u32,
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

    /// Every color the editors draw syntax in: the seven fitted roles and `text`.
    fn colors(self) -> [u32; 8] {
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
    /// Each background's luminance and the contrast a syntax color must reach on it: the text
    /// rule plus the rasterization margin, or `text`'s own contrast where that is lower.
    backgrounds: [(f64, f64); 4],
}

impl SyntaxTargets {
    fn new(palette: Palette) -> Self {
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
            (background, (LABEL_RULE + LABEL_MARGIN).min(text_contrast))
        });
        Self {
            text: palette.text,
            text_reads,
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
}

impl Global for EditorHighlights {}

/// The applied palette's editor highlights. Every application sets them with the palette; only
/// a test application that has not applied one fits the default palette's here.
pub fn editor_highlights(cx: &App) -> EditorHighlights {
    cx.try_global::<EditorHighlights>()
        .copied()
        .unwrap_or_else(|| {
            let palette = palette(cx);
            EditorHighlights::fit(palette, palette.syntax_roles())
        })
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
    /// line tints and the editor background; a word tint carries `text`, every syntax role and its
    /// diff color, and sits on its line tint. The current match stands apart from the other
    /// matches too where it can; its accent underline marks it where it cannot.
    fn fit(palette: Palette, roles: SyntaxRoles) -> Self {
        let away = if palette.is_light() {
            0xffffff
        } else {
            0x000000
        };
        let lines = [
            palette.canvas,
            palette.panel,
            palette.added_background,
            palette.removed_background,
        ];
        let syntax = roles.colors().map(|color| (color, &lines[..]));
        let diff = [
            (palette.added, &lines[2..3]),
            (palette.removed, &lines[3..4]),
            (palette.hunk, &lines[..1]),
        ];
        let patch = [&syntax[..], &diff].concat();
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
        let word = |(diff, line): (u32, &[u32])| {
            let line = line[0];
            let seed = [16, 8, 0].into_iter().fold(0, |seed, shift| {
                let (line, diff) = ((line >> shift) & 0xff, (diff >> shift) & 0xff);
                seed | ((line * 3 + diff) / 4) << shift
            });
            fit(
                seed,
                &[&syntax[..], &[(diff, &[line])]].concat(),
                &[line],
                diff,
            )
        };
        Self {
            selection,
            find_match,
            find_current,
            added_word: word(diff[0]),
            removed_word: word(diff[1]),
        }
    }
}

/// The colors one highlight carries and sits on, measured once for its fitting.
struct HighlightFit {
    /// Each color drawn on the highlight: its luminance and the contrast it should reach there,
    /// 4.75:1 or its own lowest contrast on the lines it is drawn on, never below 4.5:1.
    drawn: Vec<(f64, f64)>,
    /// The OKLab coordinates of each background the highlight sits on.
    lines: Vec<[f64; 3]>,
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
    fn new(drawn: &[(u32, &[u32])], lines: &[u32]) -> Self {
        Self {
            drawn: drawn
                .iter()
                .map(|&(color, on)| {
                    let color = custom::luminance(color);
                    let own = on
                        .iter()
                        .map(|&line| custom::luminance_contrast(color, custom::luminance(line)))
                        .fold(LABEL_RULE + LABEL_MARGIN, f64::min);
                    (color, own.max(LABEL_RULE))
                })
                .collect(),
            lines: lines.iter().map(|&line| oklab(line)).collect(),
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

    /// Whether `color` stands [`HIGHLIGHT_DISTANCE`] from every line.
    fn distinct(&self, color: [f64; 3]) -> bool {
        self.lines
            .iter()
            .all(|line| oklab_distance(color, *line) >= HIGHLIGHT_DISTANCE)
    }

    /// `seed` fitted as [`EditorHighlights::fit`] describes, when a candidate both reads and
    /// stands apart; `tint` lends its hue to a seed too grey to have one of its own.
    fn search(&self, seed: u32, away: u32, tint: u32) -> Option<u32> {
        let origin = oklab(seed);
        if self.reading(seed) == Reading::Target && self.distinct(origin) {
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
                Reading::Target if self.distinct(oklab(color)) => return Some(color),
                Reading::Rule if fallback.is_none() && self.distinct(oklab(color)) => {
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
fn linear_from_oklab([lightness, a, b]: [f64; 3]) -> [f64; 3] {
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ]
    .map(|channel| channel.clamp(0., 1.))
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
        // Thirty-nine colored fields and the editor foreground, on four backgrounds.
        assert_eq!(measured, palettes * 40 * 4);
    }

    /// Text, every syntax color and the unified patch's diff colors read at 4.5:1 on each
    /// highlight the editors draw behind them in each case's configured theme, and each
    /// highlight stands [`HIGHLIGHT_DISTANCE`] (ΔEOK) from every line background it is drawn on:
    /// the selection (the configured theme's text selection) and Find's other and current
    /// matches on the editor background, the active line and both line tints; each word tint on
    /// its own line tint, under text, the syntax colors and its diff color. Returns the cases
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
                    with(&[("unified added", palette.added)]),
                    vec![lines[2]],
                ),
                (
                    "removed word tint",
                    highlights.removed_word,
                    with(&[("unified removed", palette.removed)]),
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
    /// orange for `warning`, which Tokyo Night shares with `modified`: its eight role colors
    /// are then distinct and each already reads on every editor background, so none is mixed
    /// toward `text`.
    #[test]
    fn syntax_fields_take_their_palette_roles() {
        let mut custom = CustomTheme::from_base(1, "Tokyo orange", ThemeChoice::TokyoNight);
        custom.palette.set(TokenKind::Warning, 0xff9e64);
        let palette = custom.palette;
        assert_eq!(palette.readability_issues(), Vec::new());
        let roles: [(Option<u32>, &[&str]); 9] = [
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
                ],
            ),
            (Some(palette.warning), &["number", "boolean", "constant"]),
            (
                Some(palette.modified),
                &["type", "constructor", "enum", "variant"],
            ),
            (Some(palette.hunk), &["function"]),
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
        assert_eq!(distinct.len(), 8, "the role colors are distinct");

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
        assert_eq!(drawn.len(), 42);
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
            let roles = palette.syntax_roles();
            palette.configure(
                is_light,
                roles,
                EditorHighlights::fit(palette, roles),
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
                (theme.tokens.popover, palette.panel),
                (theme.tokens.scrollbar_thumb, palette.border),
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
                // surface, through the kit's secondary where the label is `text`.
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
                    }
                    ButtonVariant::Custom(variant) => {
                        assert!(
                            matches!(choice, ThemeChoice::KanagawaLotus | ThemeChoice::OneDark),
                            "{choice:?} leaves the kit's secondary"
                        );
                        let expected = ButtonCustomVariant::new(cx)
                            .foreground(label)
                            .active(rgb(palette.selected).into());
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
}
