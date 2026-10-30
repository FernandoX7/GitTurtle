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
    /// more room above. The gap's far edges are rounded out to whole pixels,
    /// so a popup placed there by whole pixels keeps the whole gap. At scale
    /// 1 or 2 the row is one whole pixel or within it; at a fractional
    /// scale, where GPUI paints a popup's ring over the device pixels it
    /// touches, the ring a gap of its width holds still ends clear of the
    /// device row under the pointer.
    fn as_side(self) -> Self {
        match self {
            Self::Point {
                position,
                gap,
                scale_factor,
            } => {
                let scale_factor = scale_factor.max(f32::EPSILON);
                let row = (f32::from(position.y) * scale_factor).floor();
                let top = px(row / scale_factor) - gap;
                let bottom = px((row + 1.) / scale_factor) + gap;
                Self::Side {
                    trigger_bounds: Bounds::from_corners(
                        point(position.x, top.floor()),
                        point(position.x, bottom.ceil()),
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
        let (Self::Point { .. }, Self::Side { trigger_bounds, .. }) = (self, self.as_side()) else {
            return None;
        };
        let bottom_limit = (viewport_size.height - margin).max(margin);
        let below = bottom_limit - trigger_bounds.bottom();
        let above = trigger_bounds.top() - margin;
        Some(below.max(above).max(px(0.)))
    }
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
    /// does, and by whole pixels away from the point, never onto it. It
    /// lies beside the pixel row the point lies in, never on it, with
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
        // Ahead of the children so it blocks what is behind the popup without
        // blocking the popup's own content.
        if self.occlude {
            window.insert_hitbox(position.bounds, HitboxBehavior::BlockMouse);
        }

        let offset = position.bounds.origin - bounds.origin;
        // GitTurtle patch: a popup that ends inside the room above the
        // viewport's foot is moved by whole pixels without crossing it:
        // rounding to the nearest pixel put a popup ending 8 px above a
        // status bar another half pixel lower.
        let bottom = viewport.height - margin;
        // GitTurtle patch: a popup above a point moves up to a whole pixel,
        // never down toward the point: its height can end in half a pixel,
        // as on a display of scale 2, and rounding to the nearest pixel then
        // carried its bottom half a pixel into the gap kept above the
        // point's pixel row, and its ring onto that row. One below the point
        // starts at a whole pixel already, its gap rounded out to one
        // ([`Strategy::as_side`]).
        let y = match (self.strategy, position.placement) {
            (Strategy::Point { .. }, Some(Placement::Top)) => offset.y.floor(),
            _ => match offset.y.round() {
                y if position.bounds.bottom() <= bottom
                    && position.bounds.bottom() + (y - offset.y) > bottom =>
                {
                    offset.y.floor()
                }
                y => y,
            },
        };
        let offset = point(offset.x.round(), y);

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
