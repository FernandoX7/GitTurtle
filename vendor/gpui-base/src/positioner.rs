//! Shared popup positioning.
//!
//! Every anchored surface in Base resolves its position here so that flipping,
//! alignment, and viewport clamping cannot drift apart between popups,
//! tooltips, and menus.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Anchor, AnyElement, App, Bounds, Display, Element, GlobalElementId, Half as _, HitboxBehavior,
    InspectorElementId, IntoElement, LayoutId, ParentElement, Pixels, Point, Position, Size, Style,
    Window, point, px,
};

use crate::Placement;

/// Alignment of a popup along the side it is placed on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Align {
    /// Align with the trigger's leading edge.
    Start,
    /// Center on the trigger.
    #[default]
    Center,
    /// Align with the trigger's trailing edge.
    End,
}

/// How a popup derives its position.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Strategy {
    /// Place `anchor`'s corner of the popup at `position`, then clamp into the
    /// viewport. This reproduces GPUI's `anchored` corner behavior and does not
    /// flip to the opposite side.
    Corner {
        anchor: Anchor,
        position: Point<Pixels>,
    },
    /// Place the popup on `placement`'s side of the trigger, flipping to the
    /// opposite side when it does not fit, then clamp into the viewport.
    Side {
        trigger_bounds: Bounds<Pixels>,
        placement: Option<Placement>,
        align: Align,
        offset: Pixels,
    },
    /// GitTurtle patch: place the popup wholly below `position`, a pointer's
    /// place, or wholly above it, `gap` clear of the device pixel row the
    /// point lies in at `scale_factor`, the window's, which the positioner
    /// sets as it resolves the popup ([`Positioner::point`]).
    Point {
        position: Point<Pixels>,
        gap: Pixels,
        scale_factor: f32,
    },
}

impl Strategy {
    /// GitTurtle patch: a [`Strategy::Point`] as the side placement it is:
    /// below the device pixel row the point lies in and `gap` past it, its
    /// leading edge at the point, or above that row and its gap where the
    /// popup fits only there, or where it fits on neither side and there is
    /// more room above. The gap's far edges are rounded out to the device
    /// pixels at or past them ([`point_row_clear_by`]), where the popup is
    /// placed as it is prepainted, so it keeps the whole gap. At scale 1 or
    /// 2 the row is one whole pixel or within it; at a fractional scale,
    /// where GPUI paints a popup's ring over the device pixels it touches,
    /// the ring a gap of its width holds still ends clear of the device row
    /// under the pointer.
    fn as_side(self) -> Self {
        match self {
            Self::Point {
                position,
                gap,
                scale_factor,
            } => {
                let (top, bottom) = point_row_clear_by(position.y, gap, scale_factor);
                Self::Side {
                    trigger_bounds: Bounds::from_corners(
                        point(position.x, top),
                        point(position.x, bottom),
                    ),
                    placement: Some(Placement::Bottom),
                    align: Align::Start,
                    offset: px(0.),
                }
            }
            strategy => strategy,
        }
    }

    /// GitTurtle patch: for a [`Strategy::Point`], the tallest the popup may
    /// be in `viewport_size` with `margin` to lie wholly below or wholly
    /// above its point and gap: the larger of the rooms
    /// [`resolve_placement`] weighs ([`Positioner::point_room`]). `None`
    /// for the other placements.
    fn point_room(self, viewport_size: Size<Pixels>, margin: Pixels) -> Option<Pixels> {
        let (Self::Point { scale_factor, .. }, Self::Side { trigger_bounds, .. }) =
            (self, self.as_side())
        else {
            return None;
        };
        // GitTurtle patch: measured to the device pixels at or inside the
        // margins, where a popup clamped to them lies, so a popup capped at
        // the room lies wholly on its side of the point clear of them too.
        let bottom_limit =
            device_pixel_at_or_before((viewport_size.height - margin).max(margin), scale_factor);
        let below = bottom_limit - trigger_bounds.bottom();
        let above = trigger_bounds.top() - device_pixel_at_or_after(margin, scale_factor);
        Some(below.max(above).max(px(0.)))
    }

    /// GitTurtle patch: for a [`Strategy::Point`], whether `bounds`, where
    /// [`resolve`] placed the popup, lie wholly below or wholly above the
    /// point's pixel row and gap, not clamped over them. `false` for the
    /// other placements.
    fn clear_of_point(self, bounds: Bounds<Pixels>) -> bool {
        let (Self::Point { .. }, Self::Side { trigger_bounds, .. }) = (self, self.as_side()) else {
            return false;
        };
        bounds.top() >= trigger_bounds.bottom() - SIDE_TOLERANCE
            || bounds.bottom() <= trigger_bounds.top() + SIDE_TOLERANCE
    }
}

/// GitTurtle patch: where a popup opened at a pointer's place, `y`, may end
/// above it and begin below it on a display of `scale_factor`: `gap` clear
/// of the device pixel row the point lies in, the far edges rounded out to
/// the device pixels at or past them, as [`Positioner::point`] places a
/// popup. A popup that draws a ring of `gap` outside its fill and starts at
/// the second edge draws it from the device row below the pointer's, never
/// on it, and one that ends at the first edge ends its ring on the device
/// row above the pointer's, at a fractional scale too, where GPUI paints
/// the ring over the device pixels it touches. Rounded out to whole logical
/// pixels instead, at 1.2 and 1.25 the edges lay part way into a device
/// pixel, which GPUI rounds an element's place to, halves toward zero, and
/// the ring reached the pointer's own device row; at 2 they left an empty
/// device row beside it.
pub fn point_row_clear_by(y: Pixels, gap: Pixels, scale_factor: f32) -> (Pixels, Pixels) {
    let (top, bottom) = point_device_row(y, scale_factor);
    (
        device_pixel_at_or_before(top - gap, scale_factor),
        device_pixel_at_or_after(bottom + gap, scale_factor),
    )
}

/// GitTurtle patch: the device pixel row a pointer's place, `y`, lies in on
/// a display of `scale_factor`, as its top and bottom edges in logical
/// pixels: where what a popup opened at the point opened from begins and
/// ends, which nothing the popup draws reaches. A point within a thousandth
/// of a device pixel of a row's top lies in that row: a pointer on a whole
/// device pixel, `k / scale_factor` in logical pixels, can come back from
/// that division a hair short of `k` times the scale, as at 1.1, 1.333 and
/// 1.75, and took the row above.
pub fn point_device_row(y: Pixels, scale_factor: f32) -> (Pixels, Pixels) {
    const TOLERANCE: f32 = 1e-3;
    let scale_factor = scale_factor.max(f32::EPSILON);
    let row = (f32::from(y) * scale_factor + TOLERANCE).floor();
    (px(row / scale_factor), px((row + 1.) / scale_factor))
}

/// GitTurtle patch: how near a popup's trailing edge lies to the window's
/// right or foot margin, or a popup's edge to a point's gap, to stand on
/// it: the clamp or placement that moves it there subtracts, and can leave
/// it a rounding error short.
const SIDE_TOLERANCE: Pixels = px(0.01);

/// GitTurtle patch: the first device pixel boundary at or after `x` on a
/// display of `scale_factor`, in logical pixels: where a popup's fill
/// starts so that it lies wholly on device pixels at or inside a leading
/// margin of `x`. A boundary within a thousandth of a device pixel of `x`
/// is `x`'s own, as a margin of 9 px times a scale can come back a hair
/// past a whole device pixel.
pub fn device_pixel_at_or_after(x: Pixels, scale_factor: f32) -> Pixels {
    const TOLERANCE: f32 = 1e-3;
    let scale_factor = scale_factor.max(f32::EPSILON);
    px((f32::from(x) * scale_factor - TOLERANCE).ceil() / scale_factor)
}

/// GitTurtle patch: the last device pixel boundary at or before `x` on a
/// display of `scale_factor`, in logical pixels: where a popup's fill ends
/// so that it lies wholly on device pixels at or inside a trailing margin
/// at `x`. A boundary within a thousandth of a device pixel of `x` is
/// `x`'s own.
pub fn device_pixel_at_or_before(x: Pixels, scale_factor: f32) -> Pixels {
    const TOLERANCE: f32 = 1e-3;
    let scale_factor = scale_factor.max(f32::EPSILON);
    px((f32::from(x) * scale_factor + TOLERANCE).floor() / scale_factor)
}

/// The bounds a popup resolved to, and the side it ended up on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedPosition {
    /// Final popup bounds in window coordinates.
    pub bounds: Bounds<Pixels>,
    /// The side the popup was placed on. `None` for corner positioning, which
    /// has no notion of a side.
    pub placement: Option<Placement>,
}

/// An unstyled element that positions its children as an anchored popup.
///
/// This owns measurement, side selection, alignment, and viewport clamping. It
/// installs no presentation of its own and adds no wrapper element around its
/// children.
pub struct Positioner {
    strategy: Strategy,
    margin: Pixels,
    /// GitTurtle patch: the room kept clear at the viewport's foot
    /// ([`Positioner::bottom_inset`]).
    bottom_inset: Pixels,
    /// GitTurtle patch: where a corner popup's position is read as it is
    /// prepainted ([`Positioner::corner_following`]).
    following: Option<Rc<Cell<Point<Pixels>>>>,
    occlude: bool,
    children: Vec<AnyElement>,
}

#[doc(hidden)]
pub struct PositionerState {
    child_layout_ids: Vec<LayoutId>,
}

impl Positioner {
    /// Places the popup on a side of `trigger_bounds`, flipping when needed.
    pub fn side(trigger_bounds: Bounds<Pixels>) -> Self {
        Self {
            strategy: Strategy::Side {
                trigger_bounds,
                placement: None,
                align: Align::Center,
                offset: px(0.),
            },
            margin: px(4.),
            bottom_inset: px(0.),
            following: None,
            occlude: false,
            children: Vec::new(),
        }
    }

    /// Places `anchor`'s corner of the popup at `position`.
    ///
    /// This is the corner-anchoring path used by triggers that were written
    /// against GPUI's `anchored` element. It clamps into the viewport without
    /// changing the requested anchor.
    pub fn corner(anchor: Anchor, position: Point<Pixels>) -> Self {
        Self {
            strategy: Strategy::Corner { anchor, position },
            margin: px(4.),
            bottom_inset: px(0.),
            following: None,
            occlude: false,
            children: Vec::new(),
        }
    }

    /// GitTurtle patch: places `anchor`'s corner of the popup at the point
    /// `position` holds when the popup is prepainted, not when it is built.
    /// A [`crate::Popup`] records its trigger's corner there as the trigger
    /// is prepainted, which is before any deferred content is, so an open
    /// popup follows a trigger in the frame that moves it.
    pub fn corner_following(anchor: Anchor, position: Rc<Cell<Point<Pixels>>>) -> Self {
        let mut positioner = Self::corner(anchor, position.get());
        positioner.following = Some(position);
        positioner
    }

    /// GitTurtle patch: places the popup at `position`, a pointer's place,
    /// so that it never covers the point: its top-left corner there where
    /// its height fits below it, otherwise its bottom-left corner there
    /// where it fits above it, and where it fits on neither side, on the
    /// side with more room. It moves in from the window's sides as a corner
    /// does, and lies on the device pixel its gap ends on, never nearer the
    /// point. It lies beside the pixel row the point lies in, never on it, with
    /// [`Positioner::offset`]'s gap between them, the room a caller keeps
    /// for a ring the popup draws outside its fill, so that nothing it
    /// draws covers the point. With no gap its top lies on the row's lower
    /// edge, so a pointer on a whole pixel, as X11 reports one, is not
    /// inside it.
    /// A popup that fits on neither side is still clamped into the window,
    /// over the point, unless the caller caps it at
    /// [`Positioner::point_room`]. [`Positioner::corner`] keeps the corner at
    /// the point and only clamps, so a popup near the window's foot slid up
    /// over the point it was opened at.
    pub fn point(position: Point<Pixels>) -> Self {
        Self {
            strategy: Strategy::Point {
                position,
                gap: px(0.),
                scale_factor: 1.,
            },
            ..Self::corner(Anchor::TopLeft, position)
        }
    }

    /// GitTurtle patch: for [`Positioner::point`], the tallest the popup
    /// may be in `window` to lie wholly below or wholly above its point and
    /// gap, with this positioner's margin and bottom inset: the room on the
    /// side with more. A caller caps a scrolling popup at it so that it
    /// never covers the point, however short the window. `None` for the
    /// other placements.
    pub fn point_room(&self, window: &Window) -> Option<Pixels> {
        let (viewport, margin) = self.viewport(window);
        self.strategy_in(window).point_room(viewport, margin)
    }

    /// GitTurtle patch: the strategy as `window` resolves it: a followed
    /// corner as it stands now, and a point on the window's device pixels.
    fn strategy_in(&self, window: &Window) -> Strategy {
        let mut strategy = self.strategy;
        match (&mut strategy, &self.following) {
            (Strategy::Corner { position, .. }, Some(following)) => *position = following.get(),
            (Strategy::Point { scale_factor, .. }, _) => *scale_factor = window.scale_factor(),
            _ => {}
        }
        strategy
    }

    /// The viewport a popup is resolved in and the margin it keeps there:
    /// the window's, the margin outside the client inset, and the viewport
    /// shortened by the part of the bottom inset past that margin, so the
    /// margin then ends at the inset.
    fn viewport(&self, window: &Window) -> (Size<Pixels>, Pixels) {
        let client_inset = window.client_inset().unwrap_or(px(0.));
        let margin = self.margin + client_inset;
        let mut viewport = window.viewport_size();
        viewport.height -= (self.bottom_inset - margin).max(px(0.));
        (viewport, margin)
    }

    /// Sets the preferred side. Only meaningful for [`Positioner::side`].
    pub fn placement(mut self, placement: Placement) -> Self {
        if let Strategy::Side {
            placement: slot, ..
        } = &mut self.strategy
        {
            *slot = Some(placement);
        }
        self
    }

    /// Sets the alignment along the chosen side. Only meaningful for
    /// [`Positioner::side`].
    pub fn align(mut self, align: Align) -> Self {
        if let Strategy::Side { align: slot, .. } = &mut self.strategy {
            *slot = align;
        }
        self
    }

    /// Sets the gap between the trigger and the popup. Only meaningful for
    /// [`Positioner::side`].
    ///
    /// GitTurtle patch: and for [`Positioner::point`], the gap between the
    /// pixel row the point lies in and the popup.
    pub fn offset(mut self, offset: Pixels) -> Self {
        match &mut self.strategy {
            Strategy::Side { offset: slot, .. } | Strategy::Point { gap: slot, .. } => {
                *slot = offset;
            }
            Strategy::Corner { .. } => {}
        }
        self
    }

    /// Blocks the mouse over the positioned popup.
    ///
    /// Off by default, because a tooltip that swallowed the pointer would
    /// un-hover the very trigger keeping it open. An interactive surface — a
    /// popover, a menu, a dropdown — wants it on: what the surface covers is
    /// the surface's, not the panel underneath.
    pub fn occlude(mut self) -> Self {
        self.occlude = true;
        self
    }

    /// Sets the minimum distance kept between the popup and the viewport edge.
    pub fn margin(mut self, margin: Pixels) -> Self {
        self.margin = margin;
        self
    }

    /// GitTurtle patch: keeps the popup at least `inset` above the
    /// viewport's bottom edge, client inset included, where that is more
    /// than the margin: the room an application keeps at a window's foot,
    /// such as a status bar ([`crate::set_popup_bottom_inset`]). It decides
    /// the room below a trigger for a side's flip as well.
    pub fn bottom_inset(mut self, inset: Pixels) -> Self {
        self.bottom_inset = inset;
        self
    }
}

/// Resolves the bounds of a popup of `popup_size`.
///
/// Side placement picks the preferred side when the popup fits, otherwise the
/// opposite side, otherwise whichever side has more room. The result is always
/// clamped into the viewport with `margin`.
fn resolve(
    strategy: Strategy,
    popup_size: Size<Pixels>,
    viewport_size: Size<Pixels>,
    margin: Pixels,
) -> ResolvedPosition {
    match strategy {
        Strategy::Point { .. } => resolve(strategy.as_side(), popup_size, viewport_size, margin),
        Strategy::Corner { anchor, position } => ResolvedPosition {
            bounds: clamp(
                Bounds::from_anchor_and_size(anchor, position, popup_size),
                viewport_size,
                margin,
            ),
            placement: None,
        },
        Strategy::Side {
            trigger_bounds,
            placement,
            align,
            offset,
        } => {
            let placement =
                resolve_placement(trigger_bounds, popup_size, viewport_size, margin, placement);
            let origin = side_origin(trigger_bounds, popup_size, placement, align, offset);
            ResolvedPosition {
                bounds: clamp(Bounds::new(origin, popup_size), viewport_size, margin),
                placement: Some(placement),
            }
        }
    }
}

fn resolve_placement(
    trigger_bounds: Bounds<Pixels>,
    popup_size: Size<Pixels>,
    viewport_size: Size<Pixels>,
    margin: Pixels,
    preferred: Option<Placement>,
) -> Placement {
    let right_limit = (viewport_size.width - margin).max(margin);
    let bottom_limit = (viewport_size.height - margin).max(margin);
    let available_left = (trigger_bounds.left() - margin).max(px(0.));
    let available_right = (right_limit - trigger_bounds.right()).max(px(0.));
    let available_above = (trigger_bounds.top() - margin).max(px(0.));
    let available_below = (bottom_limit - trigger_bounds.bottom()).max(px(0.));

    match preferred {
        Some(Placement::Right) if popup_size.width <= available_right => Placement::Right,
        Some(Placement::Right) if popup_size.width <= available_left => Placement::Left,
        Some(Placement::Right) if available_right >= available_left => Placement::Right,
        Some(Placement::Right) => Placement::Left,
        Some(Placement::Left) if popup_size.width <= available_left => Placement::Left,
        Some(Placement::Left) if popup_size.width <= available_right => Placement::Right,
        Some(Placement::Left) if available_left >= available_right => Placement::Left,
        Some(Placement::Left) => Placement::Right,
        Some(Placement::Bottom) if popup_size.height <= available_below => Placement::Bottom,
        Some(Placement::Bottom) if popup_size.height <= available_above => Placement::Top,
        Some(Placement::Bottom) if available_below >= available_above => Placement::Bottom,
        Some(Placement::Bottom) => Placement::Top,
        Some(Placement::Top) | None if popup_size.height <= available_above => Placement::Top,
        Some(Placement::Top) | None if popup_size.height <= available_below => Placement::Bottom,
        Some(Placement::Top) | None if available_below >= available_above => Placement::Bottom,
        Some(Placement::Top) | None => Placement::Top,
    }
}

fn side_origin(
    trigger_bounds: Bounds<Pixels>,
    popup_size: Size<Pixels>,
    placement: Placement,
    align: Align,
    offset: Pixels,
) -> Point<Pixels> {
    let aligned_x = match align {
        Align::Start => trigger_bounds.left(),
        Align::Center => trigger_bounds.center().x - popup_size.width.half(),
        Align::End => trigger_bounds.right() - popup_size.width,
    };
    let aligned_y = match align {
        Align::Start => trigger_bounds.top(),
        Align::Center => trigger_bounds.center().y - popup_size.height.half(),
        Align::End => trigger_bounds.bottom() - popup_size.height,
    };

    match placement {
        Placement::Top => point(aligned_x, trigger_bounds.top() - popup_size.height - offset),
        Placement::Bottom => point(aligned_x, trigger_bounds.bottom() + offset),
        Placement::Left => point(trigger_bounds.left() - popup_size.width - offset, aligned_y),
        Placement::Right => point(trigger_bounds.right() + offset, aligned_y),
    }
}

fn clamp(
    mut bounds: Bounds<Pixels>,
    viewport_size: Size<Pixels>,
    margin: Pixels,
) -> Bounds<Pixels> {
    let right_limit = (viewport_size.width - margin).max(margin);
    let bottom_limit = (viewport_size.height - margin).max(margin);

    if bounds.right() > right_limit {
        bounds.origin.x -= bounds.right() - right_limit;
    }
    if bounds.left() < margin {
        bounds.origin.x = margin;
    }
    if bounds.bottom() > bottom_limit {
        bounds.origin.y -= bounds.bottom() - bottom_limit;
    }
    if bounds.top() < margin {
        bounds.origin.y = margin;
    }

    bounds
}

impl ParentElement for Positioner {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Element for Positioner {
    type RequestLayoutState = PositionerState;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child_layout_ids = self
            .children
            .iter_mut()
            .map(|child| child.request_layout(window, cx))
            .collect::<Vec<_>>();
        let layout_id = window.request_layout(
            Style {
                position: Position::Absolute,
                display: Display::Flex,
                ..Style::default()
            },
            child_layout_ids.iter().copied(),
            cx,
        );

        (layout_id, PositionerState { child_layout_ids })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if request_layout.child_layout_ids.is_empty() {
            return;
        }

        let mut child_min = point(Pixels::MAX, Pixels::MAX);
        let mut child_max = Point::default();
        for child_layout_id in &request_layout.child_layout_ids {
            let child_bounds = window.layout_bounds(*child_layout_id);
            child_min = child_min.min(&child_bounds.origin);
            child_max = child_max.max(&child_bounds.bottom_right());
        }

        let popup_size = (child_max - child_min).into();
        // GitTurtle patch: a bottom inset larger than the margin shortens
        // the viewport the popup is resolved in by the difference, so the
        // margin then ends at the inset.
        let (viewport, margin) = self.viewport(window);
        // GitTurtle patch: a followed corner as it stands now, and a point
        // on the window's device pixels.
        let position = resolve(self.strategy_in(window), popup_size, viewport, margin);

        let offset = position.bounds.origin - bounds.origin;
        let scale_factor = window.scale_factor();
        // GitTurtle patch: a popup that ends inside the room above the
        // viewport's foot is moved by whole pixels without crossing it:
        // rounding to the nearest pixel put a popup ending 8 px above a
        // status bar another half pixel lower.
        let bottom = (viewport.height - margin).max(margin);
        // GitTurtle patch: the device pixels at or inside the top and foot
        // margins, where a popup clamped to either lies: each margin rounds
        // outward, never to the nearest device pixel, so no popup that fits
        // between them crosses either.
        let (top, foot) = (
            device_pixel_at_or_after(margin, scale_factor),
            device_pixel_at_or_before(bottom, scale_factor),
        );
        let height = position.bounds.size.height;
        // GitTurtle patch: the window's top, or its client inset where it
        // has one, on the device pixel at or below it.
        let window_top = device_pixel_at_or_after(margin - self.margin, scale_factor);
        let y = if self.strategy_in(window).clear_of_point(position.bounds) {
            // GitTurtle patch: a popup wholly below or above a point lies on
            // the device pixel its gap ends on ([`Strategy::as_side`]), its
            // drawn height a whole number of device pixels, as GPUI laid it
            // out. Moved by whole pixels, below the point it started part
            // way into a device pixel at 1.2, 1.25 and 1.5, which GPUI
            // rounds an element's place to, halves toward zero, back towards
            // the point, and its ring covered the pointer's device row, and
            // at 2 it left a device row empty; above it, moved up to a whole
            // pixel, it ended a device row short of its gap at 2.
            offset.y
        } else if position.bounds.top() <= margin
            && height - (foot - top) > SIDE_TOLERANCE
            && height - (foot - window_top) < SIDE_TOLERANCE
        {
            // GitTurtle patch: a popup clamped at both the top and the foot,
            // too tall for the room between their device pixels but no
            // taller than the room from the window's top to the foot's,
            // keeps the foot's, which keeps it off a status bar or an
            // application's bottom inset, and gives up only as much of the
            // top margin as it needs. Placed on the top margin, as one
            // clamped there alone is, it ran past the foot's device pixel:
            // by a device pixel where the window was a device pixel too
            // short, as at 1.2, where the 9 px margins are 10.8 device
            // pixels and leave a room 0.4 of one shorter than the margins'
            // 282 px. One taller still crosses the foot's wherever it lies,
            // so it keeps the top margin, as before: moved to the window's
            // top it gave up the margin and still crossed the foot's.
            (foot - height).max(window_top) - bounds.origin.y
        } else if position.bounds.top() <= margin {
            // GitTurtle patch: a popup on the top margin, moved down to it,
            // lies on the device pixel at or below it, counted from the
            // window's top, as one on a side margin does. Rounded to whole
            // pixels, at 1.25 and 1.5 GPUI's rounding of its place, halves
            // toward zero, put its fill a device pixel nearer the top and its
            // ring 7.2 px from it.
            top - bounds.origin.y
        } else if bottom - position.bounds.bottom() < SIDE_TOLERANCE {
            // GitTurtle patch: and one on the foot's margin, moved up to it,
            // ends on the device pixel at or above it.
            foot - height - bounds.origin.y
        } else {
            match offset.y.round() {
                y if position.bounds.bottom() <= bottom
                    && position.bounds.bottom() + (y - offset.y) > bottom =>
                {
                    offset.y.floor()
                }
                y => y,
            }
        };
        // GitTurtle patch: a popup inside the window's side margins is moved
        // by whole pixels without crossing either: rounding to the nearest
        // pixel put a popup moved in from a side up to half a pixel past its
        // margin, and a ring drawn outside its fill nearer the side.
        let right = viewport.width - margin;
        let x = if position.bounds.left() <= margin {
            // GitTurtle patch: a popup on a side margin, moved in from that
            // side or placed at it, lies on the device pixel at or inside the
            // margin, counted from the window's side. Whole pixels from where
            // this positioner was laid out, half a pixel into one at scale 2
            // beside a trigger that starts there, put its fill half a pixel
            // further in, and the ring outside it 8.5 px from the side.
            device_pixel_at_or_after(margin, scale_factor) - bounds.origin.x
        } else if right - position.bounds.right() < SIDE_TOLERANCE {
            device_pixel_at_or_before(right, scale_factor)
                - position.bounds.size.width
                - bounds.origin.x
        } else {
            match offset.x.round() {
                x if position.bounds.right() <= right
                    && position.bounds.right() + (x - offset.x) > right =>
                {
                    offset.x.floor()
                }
                x if position.bounds.left() >= margin
                    && position.bounds.left() + (x - offset.x) < margin =>
                {
                    offset.x.ceil()
                }
                x => x,
            }
        };
        let offset = point(x, y);
        // Ahead of the children so it blocks what is behind the popup without
        // blocking the popup's own content. GitTurtle patch: at the bounds
        // the popup is drawn at, not the bounds it was resolved at: a popup
        // the placement above moved, as up to the window's top when clamped
        // at both edges, left a strip below its foot that blocked the mouse
        // and a strip at its top that did not.
        if self.occlude {
            window.insert_hitbox(
                Bounds {
                    origin: bounds.origin + offset,
                    size: position.bounds.size,
                },
                HitboxBehavior::BlockMouse,
            );
        }

        window.with_element_offset(offset, |window| {
            for child in &mut self.children {
                child.prepaint(window, cx);
            }
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for child in &mut self.children {
            child.paint(window, cx);
        }
    }
}

impl IntoElement for Positioner {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARGIN: Pixels = px(4.);

    fn trigger(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), Size::new(px(w), px(h)))
    }

    fn viewport() -> Size<Pixels> {
        Size::new(px(500.), px(400.))
    }

    fn side(
        trigger_bounds: Bounds<Pixels>,
        placement: Option<Placement>,
        align: Align,
        popup: Size<Pixels>,
    ) -> ResolvedPosition {
        resolve(
            Strategy::Side {
                trigger_bounds,
                placement,
                align,
                offset: px(0.),
            },
            popup,
            viewport(),
            MARGIN,
        )
    }

    #[test]
    fn prefers_the_requested_side_when_it_fits() {
        let position = side(
            trigger(200., 200., 40., 20.),
            Some(Placement::Top),
            Align::Center,
            Size::new(px(80.), px(30.)),
        );

        assert_eq!(position.placement, Some(Placement::Top));
        assert_eq!(position.bounds.bottom(), px(200.));
    }

    #[test]
    fn flips_to_the_opposite_side_when_the_preferred_side_does_not_fit() {
        let position = side(
            trigger(200., 10., 40., 20.),
            Some(Placement::Top),
            Align::Center,
            Size::new(px(80.), px(60.)),
        );

        assert_eq!(position.placement, Some(Placement::Bottom));
        assert_eq!(position.bounds.top(), px(30.));
    }

    #[test]
    fn clamps_into_the_viewport_while_keeping_the_flipped_side() {
        let position = side(
            trigger(480., 200., 40., 20.),
            Some(Placement::Bottom),
            Align::Center,
            Size::new(px(120.), px(30.)),
        );

        assert_eq!(position.placement, Some(Placement::Bottom));
        assert_eq!(position.bounds.right(), viewport().width - MARGIN);
    }

    #[test]
    fn alignment_selects_the_leading_center_or_trailing_edge() {
        let trigger_bounds = trigger(200., 200., 100., 20.);
        let popup = Size::new(px(40.), px(30.));

        let start = side(trigger_bounds, Some(Placement::Bottom), Align::Start, popup);
        let center = side(
            trigger_bounds,
            Some(Placement::Bottom),
            Align::Center,
            popup,
        );
        let end = side(trigger_bounds, Some(Placement::Bottom), Align::End, popup);

        assert_eq!(start.bounds.left(), px(200.));
        assert_eq!(center.bounds.left(), px(230.));
        assert_eq!(end.bounds.left(), px(260.));
    }

    #[test]
    fn side_offset_adds_a_gap_between_trigger_and_popup() {
        let position = resolve(
            Strategy::Side {
                trigger_bounds: trigger(200., 200., 40., 20.),
                placement: Some(Placement::Bottom),
                align: Align::Center,
                offset: px(8.),
            },
            Size::new(px(40.), px(30.)),
            viewport(),
            MARGIN,
        );

        assert_eq!(position.bounds.top(), px(228.));
    }

    #[test]
    fn corner_positioning_places_the_named_corner_and_never_reports_a_side() {
        let position = resolve(
            Strategy::Corner {
                anchor: Anchor::TopLeft,
                position: point(px(100.), px(100.)),
            },
            Size::new(px(40.), px(30.)),
            viewport(),
            MARGIN,
        );

        assert_eq!(position.placement, None);
        assert_eq!(position.bounds.origin, point(px(100.), px(100.)));
    }

    #[test]
    fn corner_positioning_clamps_but_does_not_flip() {
        let position = resolve(
            Strategy::Corner {
                anchor: Anchor::TopLeft,
                position: point(px(480.), px(390.)),
            },
            Size::new(px(40.), px(30.)),
            viewport(),
            MARGIN,
        );

        assert_eq!(position.placement, None);
        assert_eq!(position.bounds.right(), viewport().width - MARGIN);
        assert_eq!(position.bounds.bottom(), viewport().height - MARGIN);
    }

    /// GitTurtle patch: a point at `y`, `gap` clear of its pixel row, at
    /// `scale_factor`.
    fn point_at(y: f32, gap: f32, scale_factor: f32) -> Strategy {
        Strategy::Point {
            position: point(px(100.), px(y)),
            gap: px(gap),
            scale_factor,
        }
    }

    /// GitTurtle patch: a popup at a point lies below the pixel row the
    /// point lies in where it fits there, otherwise above it where it fits
    /// there, otherwise on the side with more room. A point on a whole
    /// pixel, as X11 reports one, lies in the row that pixel starts, not
    /// inside a popup that starts there.
    #[test]
    fn point_positioning_opens_below_or_wholly_above_the_point() {
        let at = |y: f32, height: f32| {
            resolve(
                point_at(y, 0., 1.),
                Size::new(px(40.), px(height)),
                viewport(),
                MARGIN,
            )
        };

        let below = at(150.4, 100.);
        assert_eq!(below.placement, Some(Placement::Bottom));
        assert_eq!(below.bounds.origin, point(px(100.), px(151.)));

        let whole = at(150., 100.);
        assert_eq!(whole.bounds.origin, point(px(100.), px(151.)));
        assert!(!whole.bounds.contains(&point(px(100.), px(150.))));

        let above = at(350.6, 100.);
        assert_eq!(above.placement, Some(Placement::Top));
        assert_eq!(above.bounds.bottom(), px(350.));

        // 175 px below, 216 px above: neither holds it.
        assert_eq!(at(220., 300.).placement, Some(Placement::Top));
        assert_eq!(at(180., 300.).placement, Some(Placement::Bottom));
    }

    /// GitTurtle patch: with a gap as wide as the ring a popup draws in the
    /// band outside its fill, the ring, painted over every device pixel it
    /// touches as GPUI paints a shadow (`cover_bounds`), never reaches the
    /// device row under the pointer, below or above it, at scale 1, 2 and
    /// the fractional 1.25 and 1.5, for a pointer anywhere in its pixel,
    /// on a whole pixel among them; and neither fill nor ring holds the
    /// point. Without the gap the ring lay on the pointer's row, and a
    /// pointer on a whole pixel lay inside a popup below it.
    #[test]
    fn point_positioning_keeps_its_ring_off_the_pointers_device_row() {
        const RING: f32 = 1.;
        for scale_factor in [1., 2., 1.25, 1.5] {
            for step in 0..40 {
                for (row, height, side) in [
                    (150., 100., Placement::Bottom),
                    (350., 100.5, Placement::Top),
                ] {
                    let y = row + step as f32 * 0.05;
                    let pointer = point(px(100.), px(y));
                    let at = format!("{y} at scale {scale_factor}");
                    let resolved = resolve(
                        point_at(y, RING, scale_factor),
                        Size::new(px(40.), px(height)),
                        viewport(),
                        MARGIN,
                    );
                    assert_eq!(resolved.placement, Some(side), "{at}");
                    let ring = resolved.bounds.dilate(px(RING));
                    assert!(!ring.contains(&pointer), "{at}: {ring:?} holds the point");
                    let pointer_row = (y * scale_factor).floor();
                    match side {
                        Placement::Bottom => assert!(
                            (f32::from(ring.top()) * scale_factor).floor() > pointer_row,
                            "{at}: the ring {ring:?} reaches device row {pointer_row}"
                        ),
                        _ => assert!(
                            (f32::from(ring.bottom()) * scale_factor).ceil() <= pointer_row,
                            "{at}: the ring {ring:?} reaches device row {pointer_row}"
                        ),
                    }
                }
            }
        }
    }

    /// GitTurtle patch: a pointer on a whole device pixel, or part way into
    /// one, lies in that pixel's row at scales 1.1, 1.25, 1.333, 1.5, 1.75
    /// and 2, for rows 0 to 3,999: `k / scale` times the scale came back a
    /// hair short of `k` at some of them, and the row above was taken.
    #[test]
    fn a_pointer_on_a_whole_device_pixel_lies_in_that_pixels_row() {
        for scale in [1.1_f32, 1.25, 4. / 3., 1.333, 1.5, 1.75, 2.] {
            for row in 0..4000_u32 {
                let k = row as f32;
                for (into, expected) in [(0., k), (0.5, k), (0.99, k)] {
                    let y = px((k + into) / scale);
                    let (top, bottom) = point_device_row(y, scale);
                    assert!(
                        (f32::from(top) * scale - expected).abs() < 1e-2
                            && (f32::from(bottom) * scale - expected - 1.).abs() < 1e-2,
                        "scale {scale}: a pointer {into} into device row {row}, at {y:?}, lies in {top:?}..{bottom:?}"
                    );
                }
            }
        }
    }

    /// GitTurtle patch: the room a point leaves is the room on its side with
    /// more, past its gap, so a popup capped there lies wholly on that side:
    /// below a point high in the window, above one low in it.
    #[test]
    fn point_room_is_the_room_on_the_side_with_more() {
        for (y, room, side) in [
            // 396 - 102 below, 99 - 4 above.
            (100.5, 294., Placement::Bottom),
            // 396 - 302 below, 299 - 4 above.
            (300.5, 295., Placement::Top),
        ] {
            let strategy = point_at(y, 1., 1.);
            assert_eq!(
                strategy.point_room(viewport(), MARGIN),
                Some(px(room)),
                "{y}"
            );
            let capped = resolve(strategy, Size::new(px(40.), px(room)), viewport(), MARGIN);
            assert_eq!(capped.placement, Some(side), "{y}");
            assert!(
                !capped
                    .bounds
                    .dilate(px(1.))
                    .contains(&point(px(100.), px(y))),
                "{y}: {capped:?}"
            );
        }
        let corner = Strategy::Corner {
            anchor: Anchor::TopLeft,
            position: point(px(100.), px(100.)),
        };
        assert_eq!(corner.point_room(viewport(), MARGIN), None);
    }

    /// GitTurtle patch: a popup above a point whose height ends in half a
    /// pixel, as on a display of scale 2, ends on the device pixel its gap
    /// ends on as it is prepainted, never further down toward the point:
    /// moved to the nearest pixel, it went half a pixel down, and the ring
    /// in its gap reached the row of a pointer early in its pixel.
    #[gpui::test]
    fn a_popup_above_a_point_ends_where_its_gap_does(cx: &mut gpui::TestAppContext) {
        use gpui::{InteractiveElement as _, Styled as _};

        struct Harness;
        impl gpui::Render for Harness {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                gpui::div().size_full().child(
                    Positioner::point(point(px(100.), px(350.3)))
                        .offset(px(1.))
                        .child(
                            gpui::div()
                                .debug_selector(|| "point-popup".into())
                                .w(px(40.))
                                .h(px(100.5)),
                        ),
                )
            }
        }

        let (_, cx) = cx.add_window_view(|_, _| Harness);
        cx.simulate_resize(Size::new(px(500.), px(400.)));
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.draw(cx).clear(cx);
        });
        let popup = cx.debug_bounds("point-popup").expect("the popup");
        // The point's device row, at the test window's scale of 2, is 350 to
        // 350.5; its 1 px gap ends at 349, a device pixel. The popup, 100.5
        // px tall, ends there, its top on a device pixel too, and its ring
        // ends at 350, where the point's row begins. Moved up to a whole
        // pixel, it ended half a pixel higher, a device row short of it.
        assert_eq!(popup.bottom(), px(349.), "{popup:?}");
        assert_eq!(popup.top(), px(248.5), "{popup:?}");
    }

    /// GitTurtle patch: in a window of scale 1.25, as Omarchy sets, a popup
    /// prepainted at a pointer late in its pixel keeps its ring off the
    /// device row under the pointer, below it and above it: the point is
    /// placed on the window's device pixels. Placed on a scale of 1 instead,
    /// the ring below a pointer at 149.9 started at 150, which GPUI paints
    /// from device row 187, the pointer's (149.9 × 1.25 = 187.4), and the
    /// ring above one at 350.1 ended at 350, painted through device row 437,
    /// the pointer's.
    #[gpui::test]
    fn a_popup_at_a_point_keeps_its_ring_off_the_pointers_row_at_scale_1_25(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{InteractiveElement as _, Styled as _};
        const SCALE: f32 = 1.25;

        struct Harness {
            y: f32,
        }
        impl gpui::Render for Harness {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                gpui::div().size_full().child(
                    Positioner::point(point(px(100.), px(self.y)))
                        .offset(px(1.))
                        .child(
                            gpui::div()
                                .debug_selector(|| "point-popup".into())
                                .w(px(40.))
                                .h(px(100.)),
                        ),
                )
            }
        }

        // A pointer with room below, and one near the foot, above which the
        // popup goes; the fill's edge each is placed at.
        for (y, side, edge) in [
            (149.9, Placement::Bottom, 152.),
            (350.1, Placement::Top, 348.),
        ] {
            let (_, cx) = cx.add_window_view(move |_, _| Harness { y });
            cx.simulate_resize(Size::new(px(500.), px(400.)));
            cx.update(|window, cx| {
                window.set_scale_factor(SCALE);
                window.draw(cx).clear(cx);
                window.draw(cx).clear(cx);
            });
            let popup = cx.debug_bounds("point-popup").expect("the popup");
            let ring = popup.dilate(px(1.));
            let pointer_row = (y * SCALE).floor();
            match side {
                Placement::Bottom => {
                    assert_eq!(popup.top(), px(edge), "{y}: {popup:?}");
                    assert!(
                        (f32::from(ring.top()) * SCALE).floor() > pointer_row,
                        "{y}: the ring {ring:?} reaches device row {pointer_row}"
                    );
                }
                _ => {
                    assert_eq!(popup.bottom(), px(edge), "{y}: {popup:?}");
                    assert!(
                        (f32::from(ring.bottom()) * SCALE).ceil() <= pointer_row,
                        "{y}: the ring {ring:?} reaches device row {pointer_row}"
                    );
                }
            }
        }
    }

    /// GitTurtle patch: a popup moved in from either side of a 460 px window,
    /// from a positioner laid out part way into a pixel, as a popup host is
    /// beside a trigger that starts there, or at a whole one, lies on the
    /// device pixel at or inside its 9 px margin, at scale 1, 1.5 and 2.
    /// Moved by whole pixels from where it was laid out, and placed there by
    /// GPUI, which rounds an element's offset to the device pixel with halves
    /// toward zero: at scale 2 beside a trigger 6.5 px in it lay half a pixel
    /// further in, and at 1.5 from the window's side its 9 px offset, 13.5
    /// device pixels, rounded to 13, its ring then from device pixel 11, and
    /// from 4/3 px in it lay a device pixel further in on the right.
    #[gpui::test]
    fn a_popup_moved_in_from_a_side_lies_on_the_device_pixel_at_its_margin(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{InteractiveElement as _, ParentElement as _, Styled as _};
        const WIDTH: f32 = 460.;
        const MARGIN: Pixels = px(9.);

        struct Harness {
            lead: Pixels,
            anchor: Anchor,
            corner: Point<Pixels>,
        }
        impl gpui::Render for Harness {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                gpui::div().size_full().pl(self.lead).child(
                    Positioner::corner(self.anchor, self.corner)
                        .margin(MARGIN)
                        .child(
                            gpui::div()
                                .debug_selector(|| "side-popup".into())
                                .w(px(120.))
                                .h(px(60.)),
                        ),
                )
            }
        }

        for scale in [1., 1.5, 2.] {
            let device = |value: Pixels| f32::from(value) * scale;
            let at_left = (device(MARGIN) - 1e-3).ceil();
            let at_right = (device(px(WIDTH) - MARGIN) + 1e-3).floor();
            for (lead, anchor, corner, left) in [
                (6.5, Anchor::TopLeft, 6.5, true),
                (WIDTH - 46.5, Anchor::TopRight, WIDTH - 6.5, false),
                // Laid out at the window's side, and 4/3 px in, two device
                // pixels at scale 1.5, where GPUI's rounding of the old
                // whole-pixel offsets put the popup off the margin.
                (0., Anchor::TopLeft, 6.5, true),
                (4. / 3., Anchor::TopRight, WIDTH - 6.5, false),
            ] {
                let at = format!("scale {scale}, {anchor:?} at {corner}, laid out {lead} px in");
                let (_, cx) = cx.add_window_view(move |_, _| Harness {
                    lead: px(lead),
                    anchor,
                    corner: point(px(corner), px(50.)),
                });
                cx.simulate_resize(Size::new(px(WIDTH), px(300.)));
                cx.update(|window, cx| {
                    window.set_scale_factor(scale);
                    window.draw(cx).clear(cx);
                    window.draw(cx).clear(cx);
                });
                let popup = cx.debug_bounds("side-popup").expect("the popup");
                let (edge, expected) = if left {
                    (device(popup.left()), at_left)
                } else {
                    (device(popup.right()), at_right)
                };
                assert!(
                    (edge - expected).abs() < 1e-3,
                    "{at}: the popup {popup:?} ends on device pixel {edge}, not {expected}"
                );
            }
        }
    }

    /// GitTurtle patch: the failures a test across scales gathered, one line
    /// per scale with its count and its first, so that a placement wrong at
    /// several scales names each.
    fn failures_by_scale(failures: &[(f32, String)]) -> String {
        let mut lines: Vec<(f32, usize, &str)> = Vec::new();
        for (scale, failure) in failures {
            match lines.iter_mut().find(|(at, ..)| at == scale) {
                Some((_, count, _)) => *count += 1,
                None => lines.push((*scale, 1, failure)),
            }
        }
        lines
            .iter()
            .map(|(scale, count, first)| format!("scale {scale}: {count}, first: {first}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// GitTurtle patch: a popup at a pointer, as an input's drawn menu is,
    /// starts its fill on the device pixel at or after the pointer's device
    /// row and its 1 px gap below it, or ends it on the one at or before
    /// them above it, so its ring, painted over every device pixel it
    /// touches, starts on the device row below the pointer's or ends on the
    /// row above it, at scales 1, 1.2, 1.25, 1.5 and 2, for a pointer on
    /// and part way into each of twenty device rows. Rounded out to a whole
    /// pixel, below the pointer at 1.2 and 1.25 the fill started part way
    /// into a device pixel that GPUI rounds back towards the pointer, and
    /// the ring covered the pointer's own device row; at 2 it left a device
    /// row empty.
    #[gpui::test]
    fn a_popup_at_a_point_lies_on_the_device_pixel_past_its_gap(cx: &mut gpui::TestAppContext) {
        use gpui::{InteractiveElement as _, Styled as _};
        const GAP: f32 = 1.;

        struct Harness {
            y: f32,
        }
        impl gpui::Render for Harness {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                gpui::div().size_full().child(
                    Positioner::point(point(px(100.), px(self.y)))
                        .offset(px(GAP))
                        .margin(px(9.))
                        .child(
                            gpui::div()
                                .debug_selector(|| "point-popup".into())
                                .w(px(40.))
                                .h(px(100.)),
                        ),
                )
            }
        }

        let mut failures = Vec::new();
        for scale in [1., 1.2, 1.25, 1.5, 2.] {
            let (view, cx) = cx.add_window_view(|_, _| Harness { y: 150. });
            cx.simulate_resize(Size::new(px(500.), px(400.)));
            cx.update(|window, cx| {
                window.set_scale_factor(scale);
                window.draw(cx).clear(cx);
            });
            let device = |value: Pixels| f32::from(value) * scale;
            let gap = GAP * scale;
            // Rows with room below at about 150 px, and near the foot at
            // about 350 px, where the popup goes above.
            for (first, side) in [(150., Placement::Bottom), (350., Placement::Top)] {
                let first_row = (first * scale).floor();
                for step in 0..20 {
                    let row = first_row + step as f32;
                    for into in [0., 0.5] {
                        let y = (row + into) / scale;
                        view.update(cx, |harness, cx| {
                            harness.y = y;
                            cx.notify();
                        });
                        cx.update(|window, cx| window.draw(cx).clear(cx));
                        let popup = cx.debug_bounds("point-popup").expect("the popup");
                        let at = format!("{into} into device row {row}, {popup:?}");
                        let (edge, fill, ring_clear) = if side == Placement::Bottom {
                            let edge = device(popup.top());
                            let ring = (edge - gap).floor();
                            (edge, (row + 1. + gap - 1e-3).ceil(), ring > row)
                        } else {
                            let edge = device(popup.bottom());
                            let ring = (edge + gap).ceil();
                            (edge, (row - gap + 1e-3).floor(), ring <= row)
                        };
                        if !ring_clear {
                            failures.push((scale, format!("{at}: its ring is on the row")));
                        } else if (edge - fill).abs() >= 1e-3 {
                            failures.push((scale, format!("{at}: edge {edge}, not {fill}")));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures_by_scale(&failures));
    }

    /// GitTurtle patch: a popup moved down to the window's top margin or up
    /// to its foot's, from a positioner laid out at the window's top, part
    /// way into a pixel or 4/3 px down, lies on the device pixel at or
    /// inside its 9 px margin, so a ring outside its fill stands exactly
    /// 8 px from the window's top or foot, at scale 1, 1.25, 1.5 and 2,
    /// while a popup the margins leave alone keeps its place. Moved by whole
    /// pixels from where it was laid out, and placed by GPUI, which rounds
    /// an element's place to the device pixel with halves toward zero, at
    /// 1.25 a popup clamped 9 px down, 11.25 device pixels, lay on device
    /// pixel 11, its ring 7.2 px from the top, and at 1.5 the 13.5 rounded
    /// to 13.
    #[gpui::test]
    fn a_popup_clamped_to_the_top_or_foot_lies_on_the_device_pixel_at_its_margin(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{InteractiveElement as _, ParentElement as _, Styled as _};
        const HEIGHT: f32 = 300.;
        const MARGIN: Pixels = px(9.);

        struct Harness {
            lead: Pixels,
            corner: Point<Pixels>,
        }
        impl gpui::Render for Harness {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                gpui::div().size_full().pt(self.lead).child(
                    Positioner::corner(Anchor::TopLeft, self.corner)
                        .margin(MARGIN)
                        .child(
                            gpui::div()
                                .debug_selector(|| "clamped-popup".into())
                                .w(px(120.))
                                .h(px(60.)),
                        ),
                )
            }
        }

        let mut failures = Vec::new();
        for scale in [1., 1.25, 1.5, 2.] {
            let device = |value: Pixels| f32::from(value) * scale;
            let at_top = (device(MARGIN) - 1e-3).ceil();
            let at_foot = (device(px(HEIGHT) - MARGIN) + 1e-3).floor();
            for lead in [0., 6.5, 4. / 3.] {
                for (corner, top, clamped) in [
                    // Clamped down to the top margin, and up to the foot's.
                    (2., true, Some(at_top)),
                    (HEIGHT - 40., false, Some(at_foot)),
                    // Inside both margins, laid out at the window's top:
                    // where it was asked to be.
                    (100., true, None),
                ] {
                    if clamped.is_none() && lead != 0. {
                        continue;
                    }
                    let (_, cx) = cx.add_window_view(move |_, _| Harness {
                        lead: px(lead),
                        corner: point(px(100.), px(corner)),
                    });
                    cx.simulate_resize(Size::new(px(460.), px(HEIGHT)));
                    cx.update(|window, cx| {
                        window.set_scale_factor(scale);
                        window.draw(cx).clear(cx);
                        window.draw(cx).clear(cx);
                    });
                    let popup = cx.debug_bounds("clamped-popup").expect("the popup");
                    let edge = device(if top { popup.top() } else { popup.bottom() });
                    let expected = clamped.unwrap_or_else(|| device(px(corner)));
                    if (edge - expected).abs() >= 1e-3 {
                        failures.push((
                            scale,
                            format!(
                                "at {corner}, laid out {lead} px down: {popup:?} on device pixel {edge}, not {expected}"
                            ),
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures_by_scale(&failures));
    }

    /// GitTurtle patch: a popup clamped at both the top and the foot of a
    /// 300 px window with 9 px margins, one device pixel taller than the
    /// room between the device pixels at or inside them, ends on the foot's
    /// device pixel and starts one device pixel above the top's, at scale
    /// 1, 1.2, 1.25, 1.5 and 2, from a positioner laid out at the window's
    /// top and 4/3 px down; one exactly as tall as the window down to the
    /// foot's starts at the window's top and ends on the foot's; one a
    /// device pixel taller than that, or taller than the window, keeps the
    /// top margin's, since it crosses the foot's wherever it lies; and one
    /// that fits lies on both. Placed on the top margin, the device pixel
    /// too tall ran past the foot's, at 1.2 onto device pixel 350 of a foot
    /// at 349; moved to the window's top, the one a device pixel taller
    /// than the window down to the foot's gave up the top margin and still
    /// crossed the foot's.
    #[gpui::test]
    fn a_popup_clamped_at_both_ends_on_the_device_pixel_at_the_foot_margin(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{InteractiveElement as _, ParentElement as _, Styled as _};
        const HEIGHT: f32 = 300.;
        const MARGIN: Pixels = px(9.);

        struct Harness {
            lead: Pixels,
            height: Pixels,
        }
        impl gpui::Render for Harness {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                gpui::div().size_full().pt(self.lead).child(
                    Positioner::corner(Anchor::TopLeft, point(px(100.), px(2.)))
                        .margin(MARGIN)
                        .child(
                            gpui::div()
                                .debug_selector(|| "tall-popup".into())
                                .w(px(120.))
                                .h(self.height),
                        ),
                )
            }
        }

        let mut failures = Vec::new();
        for scale in [1., 1.2, 1.25, 1.5, 2.] {
            let device = |value: Pixels| f32::from(value) * scale;
            let top = (device(MARGIN) - 1e-3).ceil();
            let foot = (device(px(HEIGHT) - MARGIN) + 1e-3).floor();
            let room = foot - top;
            for lead in [0., 4. / 3.] {
                for (rows, expected_top) in [
                    // One device pixel too tall: on the foot's, a device
                    // pixel above the top's.
                    (room + 1., top - 1.),
                    // As tall as the window down to the foot's: at the
                    // window's top, on the foot's.
                    (foot, 0.),
                    // A device pixel taller than that, or taller than the
                    // window: it crosses the foot's anyway, so on the top's.
                    (foot + 1., top),
                    (device(px(HEIGHT)), top),
                    // Fits: on both.
                    (room, top),
                ] {
                    let (_, cx) = cx.add_window_view(move |_, _| Harness {
                        lead: px(lead),
                        height: px(rows / scale),
                    });
                    cx.simulate_resize(Size::new(px(460.), px(HEIGHT)));
                    cx.update(|window, cx| {
                        window.set_scale_factor(scale);
                        window.draw(cx).clear(cx);
                        window.draw(cx).clear(cx);
                    });
                    let popup = cx.debug_bounds("tall-popup").expect("the popup");
                    let (start, end) = (device(popup.top()), device(popup.bottom()));
                    let expected_end = expected_top + rows;
                    if (start - expected_top).abs() >= 1e-3 || (end - expected_end).abs() >= 1e-3 {
                        failures.push((
                            scale,
                            format!(
                                "{rows} device pixels tall, laid out {lead} px down: \
                                 {popup:?} on device pixels {start} to {end}, not \
                                 {expected_top} to {expected_end} (the foot's is {foot})"
                            ),
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures_by_scale(&failures));
    }

    /// GitTurtle patch: an occluding popup blocks the mouse over the bounds
    /// it is drawn at. One as tall as a 300 px window down to the device
    /// pixel at or inside its 9 px foot margin, clamped at both edges and so
    /// moved up from the top margin to the window's top, at scale 1, 1.2
    /// and 2, laid out at the window's top and 4/3 px down, takes a press
    /// half a device pixel inside its drawn top or foot from what lies
    /// behind it, and leaves one half a device pixel below its drawn foot,
    /// or beside it, to that. Blocking at the bounds it was resolved at,
    /// 9 px lower, it left its top 9 px to what lay behind it and blocked a
    /// strip as tall below its foot.
    #[gpui::test]
    fn an_occluding_popup_blocks_the_mouse_where_it_is_drawn(cx: &mut gpui::TestAppContext) {
        use gpui::{
            InteractiveElement as _, Modifiers, MouseButton, ParentElement as _, Styled as _,
        };
        const HEIGHT: f32 = 300.;
        const MARGIN: Pixels = px(9.);

        struct Harness {
            lead: Pixels,
            height: Pixels,
            presses: Rc<Cell<usize>>,
        }
        impl gpui::Render for Harness {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                let presses = self.presses.clone();
                gpui::div()
                    .size_full()
                    .pt(self.lead)
                    .on_mouse_down(MouseButton::Left, move |_, _, _| {
                        presses.set(presses.get() + 1);
                    })
                    .child(
                        Positioner::corner(Anchor::TopLeft, point(px(100.), px(2.)))
                            .margin(MARGIN)
                            .occlude()
                            .child(
                                gpui::div()
                                    .debug_selector(|| "occluding-popup".into())
                                    .w(px(120.))
                                    .h(self.height),
                            ),
                    )
            }
        }

        let mut failures = Vec::new();
        for scale in [1., 1.2, 2.] {
            let foot = (f32::from(px(HEIGHT) - MARGIN) * scale + 1e-3).floor();
            let half = px(0.5 / scale);
            for lead in [0., 4. / 3.] {
                let presses = Rc::new(Cell::new(0));
                let (_, cx) = cx.add_window_view({
                    let presses = presses.clone();
                    move |_, _| Harness {
                        lead: px(lead),
                        height: px(foot / scale),
                        presses,
                    }
                });
                cx.simulate_resize(Size::new(px(460.), px(HEIGHT)));
                cx.update(|window, cx| {
                    window.set_scale_factor(scale);
                    window.draw(cx).clear(cx);
                    window.draw(cx).clear(cx);
                });
                let popup = cx.debug_bounds("occluding-popup").expect("the popup");
                if f32::from(popup.top()).abs() >= 1e-3 {
                    failures.push((
                        scale,
                        format!("laid out {lead} px down: {popup:?} not moved to the window's top"),
                    ));
                    continue;
                }
                let x = popup.center().x;
                for (at, place, blocked) in [
                    ("inside its drawn top", point(x, popup.top() + half), true),
                    (
                        "inside its drawn foot",
                        point(x, popup.bottom() - half),
                        true,
                    ),
                    (
                        "below its drawn foot",
                        point(x, popup.bottom() + half),
                        false,
                    ),
                    (
                        "beside it",
                        point(popup.right() + px(20.), popup.center().y),
                        false,
                    ),
                ] {
                    let before = presses.get();
                    cx.simulate_mouse_down(place, MouseButton::Left, Modifiers::default());
                    cx.simulate_mouse_up(place, MouseButton::Left, Modifiers::default());
                    if (presses.get() > before) == blocked {
                        failures.push((
                            scale,
                            format!(
                                "laid out {lead} px down, drawn at {popup:?}: a press {at}, at \
                                 {place:?}, {}",
                                if blocked {
                                    "reached what lies behind it"
                                } else {
                                    "was blocked"
                                }
                            ),
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures_by_scale(&failures));
    }

    // Migrated from the tooltip module when its private positioning logic was
    // merged into this one. They passed unchanged across that move, which is
    // what proves the merge preserved tooltip placement behavior.
    const WINDOW_MARGIN: Pixels = px(4.);

    fn tooltip_placement(
        trigger_bounds: Bounds<Pixels>,
        popup: Size<Pixels>,
        viewport: Size<Pixels>,
        margin: Pixels,
        placement: Option<Placement>,
    ) -> (Bounds<Pixels>, Placement) {
        let resolved = resolve(
            Strategy::Side {
                trigger_bounds,
                placement,
                align: Align::Center,
                offset: px(0.),
            },
            popup,
            viewport,
            margin,
        );
        (resolved.bounds, resolved.placement.unwrap())
    }

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), Size::new(px(width), px(height)))
    }

    fn size_px(width: f32, height: f32) -> Size<Pixels> {
        Size::new(px(width), px(height))
    }

    #[test]
    fn prefers_above_when_space_allows() {
        let trigger = bounds(100., 80., 80., 24.);
        let position = tooltip_placement(
            trigger,
            size_px(120., 30.),
            size_px(300., 200.),
            WINDOW_MARGIN,
            None,
        );
        assert_eq!(position.1, Placement::Top);
        assert_eq!(position.0.origin, point(px(80.), px(50.)));
    }
    #[test]
    fn flips_and_clamps_on_each_axis() {
        let top = tooltip_placement(
            bounds(24., 4., 120., 32.),
            size_px(240., 32.),
            size_px(520., 260.),
            WINDOW_MARGIN,
            None,
        );
        assert_eq!(top.1, Placement::Bottom);

        let right = tooltip_placement(
            bounds(260., 60., 32., 32.),
            size_px(120., 30.),
            size_px(300., 200.),
            WINDOW_MARGIN,
            Some(Placement::Right),
        );
        assert_eq!(right.1, Placement::Left);

        let left_edge = tooltip_placement(
            bounds(4., 80., 24., 24.),
            size_px(120., 30.),
            size_px(300., 200.),
            WINDOW_MARGIN,
            None,
        );
        assert_eq!(left_edge.0.left(), WINDOW_MARGIN);
    }
    #[test]
    fn places_tooltip_to_the_right() {
        let trigger = bounds(20., 60., 32., 32.);
        let position = tooltip_placement(
            trigger,
            size_px(120., 30.),
            size_px(300., 200.),
            WINDOW_MARGIN,
            Some(Placement::Right),
        );
        assert_eq!(position.1, Placement::Right);
        assert_eq!(position.0.left(), trigger.right());
        assert_eq!(position.0.center().y, trigger.center().y);
    }
    #[test]
    fn right_placement_clamps_vertical_edges() {
        let trigger = bounds(20., 2., 32., 20.);
        let position = tooltip_placement(
            trigger,
            size_px(120., 40.),
            size_px(300., 200.),
            WINDOW_MARGIN,
            Some(Placement::Right),
        );
        assert_eq!(position.1, Placement::Right);
        assert_eq!(position.0.top(), WINDOW_MARGIN);
        assert_eq!(position.0.left(), trigger.right());
    }
    #[test]
    fn uses_larger_side_when_neither_vertical_side_fits() {
        let position = tooltip_placement(
            bounds(120., 20., 40., 20.),
            size_px(160., 120.),
            size_px(300., 100.),
            WINDOW_MARGIN,
            None,
        );
        assert_eq!(position.1, Placement::Bottom);
        assert_eq!(position.0.top(), WINDOW_MARGIN);
        assert_eq!(position.0.left(), px(60.));
    }
}
