pub use crate::component_traits::{Collapsible, Disableable, Selectable};
pub use crate::sizing::{Sizable, Size, StyleSized};
use gpui::{
    AnyElement, App, Bounds, BoxShadow, ContentMask, Corners, Edges, Element, ElementId,
    FontWeight, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, ParentElement,
    Pixels, StyleRefinement, Styled, Window, div, hsla, point, px, rems,
};
pub use gpui_base::{FocusableExt, RoleOverride, StyledExt, box_shadow, h_flex, v_flex};

use crate::ActiveTheme as _;

const FOCUS_RING_WIDTH: Pixels = px(3.);
const FOCUS_RING_OPACITY: f32 = 0.5;

/// The ring a focused control draws outside its edge.
///
/// The default is the ring every control in the kit draws: 3px of the theme's
/// `ring` colour at half opacity, directly outside the border.
/// [`crate::Theme::button_focus_ring`] lets an application draw a different one
/// around Buttons, Switches, Inputs and Checkboxes. Width and gap are expected
/// to be non-negative: a negative gap paints the ring over the element's own
/// fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusRing {
    /// How wide the ring is.
    pub width: Pixels,
    /// How far outside the element's border the ring starts.
    pub gap: Pixels,
    /// The opacity the theme's `ring` colour is painted at.
    pub opacity: f32,
}

impl Default for FocusRing {
    fn default() -> Self {
        Self {
            width: FOCUS_RING_WIDTH,
            gap: Pixels::ZERO,
            opacity: FOCUS_RING_OPACITY,
        }
    }
}

/// Ink every layer of a surface's shadow carries — the `rgb(0 0 0 / 0.1)`
/// shadcn/ui spends at each elevation.
const SURFACE_SHADOW_INK: f32 = 0.1;

/// Ink of the hairline ring standing in for a popover's border.
///
/// shadcn/ui draws no border on a popup surface at all: its edge is a 1px
/// `rgb(0 0 0 / 0.1)` ring spent as a shadow layer. Because the ring is
/// translucent the shadow shows *through* it, which is what makes the edge read
/// as part of one grounded surface rather than as an outline with a separate
/// shadow below it. An opaque border cannot reproduce that — a border composites
/// over the element's own background, not over the shadow.
const POPOVER_RING_INK: f32 = 0.1;

/// GitTurtle patch: the band outside a popup surface's fill in which its
/// edge, the ring in [`popover_shadow`], is drawn: a popup's visible extent is
/// its bounds grown by this on every side, so an application that keeps a
/// popup clear of something keeps it this much further.
pub const POPOVER_EDGE_WIDTH: Pixels = px(1.);

/// GitTurtle patch: how far every popup's drawn edge, the ring in the band
/// outside its fill ([`POPOVER_EDGE_WIDTH`]), stays inside its window's
/// sides, top and foot: 8 px of page, for every menu and popover alike,
/// since the edge, not the fill, is what the eye reads as a menu's side.
/// A popup host keeps the fill [`popup_fill_margin`] in. The hosts kept
/// their fill 8 px in, so a menu they moved in from a side drew its ring
/// 7 px from it, where the input's drawn menu kept its ring 8 px in.
pub const POPUP_WINDOW_MARGIN: Pixels = px(8.);

/// GitTurtle patch: how far a popup host keeps a popup's fill inside its
/// window, so that its drawn edge stands [`POPUP_WINDOW_MARGIN`] in: that
/// margin and the edge's band, 9 px.
pub fn popup_fill_margin() -> Pixels {
    POPUP_WINDOW_MARGIN + POPOVER_EDGE_WIDTH
}

/// The colour of a popup surface's hairline ring in this theme: the theme's
/// [`crate::Theme::popover_edge`] when it sets one, and otherwise 10% of the
/// foreground.
///
/// shadcn spends black on it in light mode and white in dark
/// (`oklch(1 0 0 / 10%)`), so by default it follows the foreground rather than
/// the border token: a fixed black ring would all but vanish on a dark surface.
/// A theme cannot tune that tint, and a popup on a surface of its own colour
/// stood off it by about 1.15:1, so an application may set the edge.
pub fn popover_ring(cx: &App) -> Hsla {
    cx.theme()
        .popover_edge
        .unwrap_or_else(|| cx.theme().foreground.alpha(POPOVER_RING_INK))
}

/// The colour of a menu separator in this theme: the theme's
/// [`crate::Theme::popover_edge`] when it sets one, and otherwise the border
/// token, as upstream draws it.
///
/// A separator sits on the popup's own surface, so an edge fitted against that
/// surface holds there too; the border token is fitted against the page.
pub fn menu_separator(cx: &App) -> Hsla {
    cx.theme().popover_edge.unwrap_or(cx.theme().border)
}

/// GitTurtle patch: the colour of a menu label, the text that titles or
/// explains the items around it: secondary text. Upstream drew a label as a
/// disabled item, in the colour that says a command is unavailable, although
/// a label acts on nothing.
pub fn menu_label_color(cx: &App) -> Hsla {
    cx.theme().muted_foreground
}

/// GitTurtle patch: the weight of a menu label. Upstream drew a label with an
/// item's size and weight, so only its colour set it apart, and a theme's
/// secondary text can be nearly as strong as its text: a heading then read as
/// one more command. A label is a section heading, at medium weight.
pub const MENU_LABEL_WEIGHT: FontWeight = FontWeight::MEDIUM;

/// GitTurtle patch: the text size of a menu label:
/// [`crate::Theme::menu_label_size`] where the application sets it, otherwise
/// the items' own `text_sm`.
pub fn menu_label_size(window: &Window, cx: &App) -> Pixels {
    cx.theme()
        .menu_label_size
        .unwrap_or_else(|| rems(0.875).to_pixels(window.rem_size()))
}

/// shadcn/ui's popup surface shadow — a hairline `ring` plus `shadow-md` — at
/// `strength` of its full ink.
///
/// Callers animating a surface in pass a rising `strength`; a resting surface
/// passes `1.0`.
///
/// The two blurred layers use Tailwind's radii **halved**, which is the
/// conversion CSS requires and not a taste adjustment. CSS defines a box
/// shadow's blur radius as twice the gaussian's standard deviation, while GPUI's
/// shader takes the field as the deviation itself (`gaussian(y, sigma)`).
/// Copying Tailwind's `6px` and `4px` across therefore spreads the shadow over
/// twice the distance, which is why [`Styled::shadow_md`] reads as a wide grey
/// haze next to a browser's compact one.
///
/// Measured against shadcn's own render, this lands within a luminance step of
/// it the whole way down the falloff.
///
/// The ring is taken as a colour rather than read from the theme here so that an
/// animation can hold it across frames, where no `App` is in hand.
pub(crate) fn popover_shadow(ring: Hsla, strength: f32) -> Vec<BoxShadow> {
    let strength = strength.clamp(0., 1.);
    let ink = hsla(0., 0., 0., SURFACE_SHADOW_INK * strength);
    vec![
        BoxShadow::new(px(0.), px(4.), ink)
            .blur_radius(px(3.))
            .spread_radius(px(-1.)),
        BoxShadow::new(px(0.), px(2.), ink)
            .blur_radius(px(2.))
            .spread_radius(px(-2.)),
        // The ring, sitting in the 1px band outside the surface. No blur, so it
        // takes the shader's crisp path rather than the gaussian one. GPUI
        // paints later layers on top, the reverse of CSS, so the ring comes
        // last: under the ink it read up to 11% darker along the bottom.
        BoxShadow::new(px(0.), px(0.), ring.alpha(ring.a * strength))
            .blur_radius(px(0.))
            .spread_radius(POPOVER_EDGE_WIDTH),
    ]
}

/// GitTurtle patch: how far a popup's fill stands from the trigger it opened
/// from: [`crate::Theme::popup_gap`] of page between the trigger and the
/// popup's drawn edge, plus that edge's band ([`POPOVER_EDGE_WIDTH`]), or,
/// where the theme sets no gap, upstream's quarter rem between the trigger
/// and the fill (`mt_1`), which left the drawn edge 2.25 px from a button at
/// GitTurtle's 13 px root, 3.5 px at 18 pt.
pub fn popup_offset(window: &Window, cx: &App) -> Pixels {
    cx.theme().popup_gap.map_or_else(
        || rems(0.25).to_pixels(window.rem_size()),
        |gap| gap + POPOVER_EDGE_WIDTH,
    )
}

/// GitTurtle patch: where what a popup opened from lies beside it, for
/// [`ClearOfOpener`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Opener {
    /// Above the popup, ending this far above its fill: a dropdown's
    /// button, the dropdown open below it.
    Above(Pixels),
    /// Below the popup, starting this far below its fill: a dropdown's
    /// button, the dropdown open above it.
    Below(Pixels),
    /// About `y`, a pointer's place or a row's edge: above the popup where
    /// the popup's fill starts at or below `y`, below it where the fill ends
    /// at or above `y`, `gap` past the fill either way. A popup whose fill
    /// spans `y`, clamped over what it opened from, is not clipped.
    At {
        /// The place the popup opened at.
        y: Pixels,
        /// How far past the fill what it opened from begins.
        gap: Pixels,
    },
    /// The pixel row a pointer lies in, from `top` to `bottom`: above the
    /// popup where the popup's fill starts at or below `bottom`, the mask
    /// beginning there, and below it where the fill ends at or above `top`,
    /// the mask ending there, wherever the popup's placement rounded its
    /// fill to. A popup whose fill spans the row is not clipped.
    Row {
        /// The row's upper edge.
        top: Pixels,
        /// The row's lower edge.
        bottom: Pixels,
    },
}

/// GitTurtle patch: a popup drawn only on its own side of what it opened
/// from, a trigger, a row or the pixel row a pointer lies in, so that its
/// shadow never darkens it.
///
/// A popup's shadow ([`popover_shadow`]) falls 4 px with a 3 px blur, so its
/// ink reaches 9 px below the fill, at 1 % at 7 px, past any gap a menu keeps
/// from what it opened from: a menu opened above its button or row, or above
/// a pointer, shaded the control it was opened from. The child is drawn
/// within a content mask that ends at the line where that opener begins,
/// past the gap; its fill and drawn edge lie inside the mask, and only the
/// shadow's tail on that side is cut. The wrapper adds no layout node of its
/// own: the host measures and places the child as it would without it.
pub struct ClearOfOpener {
    child: AnyElement,
    opener: Opener,
}

impl ClearOfOpener {
    /// `child`, opened from `opener`.
    pub fn new(child: impl IntoElement, opener: Opener) -> Self {
        Self {
            child: child.into_any_element(),
            opener,
        }
    }

    /// The content mask that keeps what a popup with its fill at `fill`
    /// draws on its own side of `opener` in a window whose viewport is
    /// `viewport`, or `None` where the fill spans the opener's place.
    pub fn mask(
        fill: Bounds<Pixels>,
        opener: Opener,
        viewport: gpui::Size<Pixels>,
    ) -> Option<ContentMask<Pixels>> {
        let far = Self::reach(viewport);
        // Well under a device pixel at any scale, and well over the
        // rounding error of a window's coordinates times its scale.
        const ROW_SNAP: Pixels = px(0.01);
        let (top, bottom) = match opener {
            Opener::Above(gap) => (fill.top() - gap, far),
            Opener::Below(gap) => (-far, fill.bottom() + gap),
            Opener::At { y, gap } if fill.top() >= y => (fill.top() - gap, far),
            Opener::At { y, gap } if fill.bottom() <= y => (-far, fill.bottom() + gap),
            Opener::At { .. } => return None,
            // A row's edges lie on device pixel boundaries, which GPUI
            // rounds a mask out to from their product with the scale: an
            // edge that came out a hair past its boundary, as 200.67 px at
            // scale 1.5 does, took the row's own pixel. Drawn a hundredth of
            // a pixel toward the popup, it rounds onto the boundary.
            Opener::Row { bottom, .. } if fill.top() >= bottom => (bottom + ROW_SNAP, far),
            Opener::Row { top, .. } if fill.bottom() <= top => (-far, top - ROW_SNAP),
            Opener::Row { .. } => return None,
        };
        Some(ContentMask {
            bounds: Bounds::from_corners(point(-far, top), point(far, bottom)),
        })
    }

    /// How far past the window's origin the mask's band reaches on every
    /// side: the least power of two at least twice the viewport's larger
    /// side, so the band crosses any window whole, and so near enough that
    /// the f32 size from its far edge holds the line it ends at to a few
    /// thousandths of a pixel. A band a million pixels out held that line
    /// to a sixteenth, and 200.67 px stood as 200.69, which GPUI's outward
    /// rounding carried a whole device pixel on at scale 1.5; one 10,000 px
    /// out clipped a popup in a window wider than that, as an X11 window
    /// across three 4K monitors is.
    fn reach(viewport: gpui::Size<Pixels>) -> Pixels {
        let side = f32::from(viewport.width.max(viewport.height)).max(1.) * 2.;
        px(2f32.powi(side.log2().ceil() as i32))
    }
}

impl IntoElement for ClearOfOpener {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ClearOfOpener {
    type RequestLayoutState = ();
    /// The mask the child is drawn in.
    type PrepaintState = Option<ContentMask<Pixels>>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // The child's own node, as GPUI's `deferred` does: the host measures
        // the child itself.
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<ContentMask<Pixels>> {
        let mask = Self::mask(bounds, self.opener, window.viewport_size());
        window.with_content_mask(mask, |window| self.child.prepaint(window, cx));
        mask
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        mask: &mut Option<ContentMask<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(*mask, |window| self.child.paint(window, cx));
    }
}

/// shadcn/ui's `shadow-lg`, the elevation it lifts a toast to, at `strength` of
/// its full ink.
///
/// A toast sits higher than a popover and is built differently: shadcn gives it
/// a real 1px border rather than the translucent ring it puts on a popup, so
/// there is no ring layer here. Its corner radius is left to the caller.
///
/// The radii are Tailwind's halved, for the reason [`popover_shadow`] explains.
pub(crate) fn toast_shadow(strength: f32) -> Vec<BoxShadow> {
    let ink = hsla(0., 0., 0., SURFACE_SHADOW_INK * strength.clamp(0., 1.));
    vec![
        BoxShadow::new(px(0.), px(10.), ink)
            .blur_radius(px(7.5))
            .spread_radius(px(-3.)),
        BoxShadow::new(px(0.), px(4.), ink)
            .blur_radius(px(3.))
            .spread_radius(px(-4.)),
    ]
}

/// shadcn/ui's `shadow-sm`, the elevation it spends on a control raised out of
/// the container it sits in — the active pill of a segmented tab bar — at full
/// ink.
///
/// Unlike a popover or a toast this surface is not floating over the page: it
/// sits *inside* a trough only a few pixels wider than itself, and that trough
/// clips. Both are reasons to keep the falloff tight — there is no room for a
/// wide one, and a wide one would read as grime against the trough wall rather
/// than as lift.
///
/// The radii are Tailwind's halved, for the reason [`popover_shadow`] explains:
/// CSS defines a box shadow's blur radius as twice the gaussian's standard
/// deviation, while GPUI's shader takes the field as the deviation itself
/// (`gaussian(y, sigma)`). Copying Tailwind's `3px` and `2px` across therefore
/// spreads the shadow over twice the distance, which is why
/// [`Styled::shadow_sm`] leaves a haze around a 24px pill where shadcn draws a
/// compact line.
pub(crate) fn raised_shadow() -> Vec<BoxShadow> {
    let ink = hsla(0., 0., 0., SURFACE_SHADOW_INK);
    vec![
        BoxShadow::new(px(0.), px(1.), ink).blur_radius(px(1.5)),
        BoxShadow::new(px(0.), px(1.), ink)
            .blur_radius(px(1.))
            .spread_radius(px(-1.)),
    ]
}

/// Finished styles that read the theme.
///
/// Separate from [`StyledExt`], which holds neutral helpers that make no
/// visual decisions. Everything here does: it reaches into the theme and
/// produces a specific look, which is why it belongs above the base layer.
pub trait ThemeStyled: Styled + Sized {
    /// Give this element the focus appearance the framework's own controls
    /// use: its border tinted with the focus colour, and the ring outside it.
    ///
    /// The ring is dropped when [`crate::Theme::focus_ring`] is off, leaving
    /// the tinted border — an application whose layout clips its containers can
    /// turn it off rather than finding room for the ring in each of them.
    ///
    /// Calling this turns the ring on; gate it with `when` for the conditions
    /// that decide whether the control shows one at all — its focus state,
    /// [`FocusableExt::focus_ring`], appearance, and so on.
    ///
    /// The ring sits outside the element's border, so an ancestor that clips
    /// its content will cut it off — leave it a few pixels of room, or don't
    /// clip.
    fn focus_ring_style(self, window: &Window, cx: &App) -> Self
    where
        Self: ParentElement;

    /// Give this element the surface, edge, shadow and radius of a popover.
    ///
    /// This is the one surface every popup shares — Popover, PopupMenu, Select,
    /// Combobox, DatePicker and the editor's hover popovers — so they cannot
    /// drift apart. See [`popover_shadow`] for what the shadow is modelled on.
    fn popover_style(self, cx: &App) -> Self;

    /// Round this element as far as its size allows — a circle if it is square,
    /// a pill if it is not — unless the theme squares its corners.
    ///
    /// Use this instead of [`gpui::Styled::rounded_full`] on anything the theme
    /// owns. A hardcoded `rounded_full` survives [`crate::Theme::radius`] being
    /// set to zero, which leaves avatars, badge dots and slider thumbs round in
    /// a UI that is square everywhere else. See [`crate::Theme::radius_full`].
    fn rounded_full_style(self, cx: &App) -> Self {
        self.rounded(cx.theme().radius_full())
    }
}

/// Tint `element`'s border with the focus colour and draw `ring` outside it.
///
/// [`ThemeStyled::focus_ring_style`] draws the default ring through this, and
/// Button, Switch, Input and Checkbox draw [`crate::Theme::button_focus_ring`],
/// so all are the same absolutely positioned child, which takes no layout
/// space.
pub(crate) fn apply_focus_ring<E: Styled + ParentElement>(
    mut element: E,
    ring: FocusRing,
    window: &Window,
    cx: &App,
) -> E {
    // The ring is painted outside the border, so a clipping ancestor cuts
    // it off. An application whose layout clips heavily turns it off in the
    // theme and keeps the tinted border, which takes no space.
    if !cx.theme().focus_ring {
        return element.border_color(cx.theme().ring);
    }

    let rem_size = window.rem_size();
    let inset = ring.gap + ring.width;
    let style = element.style();
    let border_widths = Edges::<Pixels> {
        top: style
            .border_widths
            .top
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        bottom: style
            .border_widths
            .bottom
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        left: style
            .border_widths
            .left
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        right: style
            .border_widths
            .right
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
    };
    let radius = Corners::<Pixels> {
        top_left: style
            .corner_radii
            .top_left
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        top_right: style
            .corner_radii
            .top_right
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        bottom_left: style
            .corner_radii
            .bottom_left
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        bottom_right: style
            .corner_radii
            .bottom_right
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
    }
    .map(|value| *value + inset);
    let mut ring_style = StyleRefinement::default();
    ring_style.corner_radii.top_left = Some(radius.top_left.into());
    ring_style.corner_radii.top_right = Some(radius.top_right.into());
    ring_style.corner_radii.bottom_left = Some(radius.bottom_left.into());
    ring_style.corner_radii.bottom_right = Some(radius.bottom_right.into());

    element.border_color(cx.theme().ring).child(
        div()
            .flex_none()
            .absolute()
            .top(-(inset + border_widths.top))
            .left(-(inset + border_widths.left))
            .right(-(inset + border_widths.right))
            .bottom(-(inset + border_widths.bottom))
            .border(ring.width)
            .border_color(cx.theme().ring.alpha(ring.opacity))
            .refine_style(&ring_style),
    )
}

impl<T: Styled + Sized> ThemeStyled for T {
    /// Draw the focus ring the framework's own controls use.
    ///
    /// Calling this turns the ring on; gate it with `when` for the conditions
    /// that decide whether the control shows one at all — its focus state,
    /// [`crate::FocusableExt::focus_ring`], appearance, and so on.
    ///
    /// The ring sits outside the element's border, so an ancestor that clips
    /// its content will cut it off — leave it a few pixels of room, or don't
    /// clip.
    fn focus_ring_style(self, window: &Window, cx: &App) -> Self
    where
        Self: ParentElement,
    {
        apply_focus_ring(self, FocusRing::default(), window, cx)
    }

    fn popover_style(self, cx: &App) -> Self {
        let theme = cx.theme();
        // No border: the edge is the ring inside `popover_shadow`, which is how
        // shadcn draws it and the only way the shadow can show through it.
        self.bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow(popover_shadow(popover_ring(cx), 1.))
            .rounded(theme.radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    fn fill(top: f32, bottom: f32) -> Bounds<Pixels> {
        Bounds::from_corners(point(px(100.), px(top)), point(px(300.), px(bottom)))
    }

    /// GitTurtle patch: an opener's mask crosses its window whole, however
    /// wide or tall: in a 12,000 × 800 window, as an X11 window across three
    /// 4K monitors is, and a 600 × 12,000 one, every kind of opener's band
    /// reaches past both sides. A band 10,000 px out clipped a popup past
    /// that line.
    #[test]
    fn an_openers_mask_crosses_a_window_of_any_size() {
        let openers = [
            (fill(40., 200.), Opener::Above(px(5.))),
            (fill(40., 200.), Opener::Below(px(5.))),
            (fill(40., 200.), Opener::At { y: px(35.), gap: px(5.) }),
            (fill(40., 200.), Opener::At { y: px(205.), gap: px(5.) }),
            (fill(40., 200.), Opener::Row { top: px(36.), bottom: px(37.) }),
            (fill(40., 200.), Opener::Row { top: px(201.), bottom: px(202.) }),
        ];
        for viewport in [size(px(12_000.), px(800.)), size(px(600.), px(12_000.))] {
            for (fill, opener) in openers {
                let mask = ClearOfOpener::mask(fill, opener, viewport).expect("a mask");
                let band = mask.bounds;
                assert!(
                    band.left() <= px(0.) && band.right() >= viewport.width,
                    "{opener:?} in {viewport:?}: {band:?} crosses the window's width"
                );
                assert!(
                    band.top() <= px(0.) || band.bottom() >= viewport.height,
                    "{opener:?} in {viewport:?}: {band:?} reaches the window's far edge"
                );
            }
        }
    }

    /// GitTurtle patch: the line a mask ends at keeps its place to a
    /// thousandth of a pixel, so GPUI's outward rounding of it, from its
    /// product with the scale, lands on the device pixel the line lies on:
    /// a pointer's device row at 200.67 to 201.33 px at scale 1.5, below a
    /// menu above it, ends the mask at device pixel 301. A band a million
    /// pixels out held the line as 200.69 px, rounded out to 302.
    #[test]
    fn an_openers_mask_keeps_its_line_on_its_device_pixel() {
        let scale = 1.5_f32;
        let (top, bottom) = (px(301. / scale), px(302. / scale));
        let viewport = size(px(461.), px(330.));
        let mask = ClearOfOpener::mask(fill(17., 198.), Opener::Row { top, bottom }, viewport)
            .expect("a mask");
        assert_eq!(
            (f32::from(mask.bounds.bottom()) * scale).ceil(),
            301.,
            "{mask:?} ends where the row begins"
        );
        let mask = ClearOfOpener::mask(fill(203., 380.), Opener::Row { top, bottom }, viewport)
            .expect("a mask");
        assert_eq!(
            (f32::from(mask.bounds.top()) * scale).floor(),
            302.,
            "{mask:?} begins where the row ends"
        );
    }
}
