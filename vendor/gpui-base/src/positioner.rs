//! Shared popup positioning.
//!
//! Every anchored surface in Base resolves its position here so that flipping,
//! alignment, and viewport clamping cannot drift apart between popups,
//! tooltips, and menus.

use gpui::{
    Anchor, AnyElement, App, Bounds, Decorations, Display, Edges, Element, GlobalElementId,
    Half as _, HitboxBehavior, InspectorElementId, LayoutId, ParentElement, Pixels, Point,
    Position, Size, Style, Window, point, px,
};

use crate::{LayoutDirection, Placement};
use std::{cell::Cell, rc::Rc};

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
    /// more room above.
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
    fn point_room(self, viewport_size: Size<Pixels>, margin: Edges<Pixels>) -> Option<Pixels> {
        let (Self::Point { scale_factor, .. }, Self::Side { trigger_bounds, .. }) =
            (self, self.as_side())
        else {
            return None;
        };
        let bottom_limit = device_pixel_at_or_before(
            (viewport_size.height - margin.bottom).max(margin.top),
            scale_factor,
        );
        let below = bottom_limit - trigger_bounds.bottom();
        let above = trigger_bounds.top() - device_pixel_at_or_after(margin.top, scale_factor);
        Some(below.max(above).max(px(0.)))
    }

    /// GitTurtle patch: for a [`Strategy::Point`], whether `bounds`, where
    /// [`resolve_in`] placed the popup, lie wholly below or wholly above the
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
/// popup.
pub fn point_row_clear_by(y: Pixels, gap: Pixels, scale_factor: f32) -> (Pixels, Pixels) {
    let (top, bottom) = point_device_row(y, scale_factor);
    (
        device_pixel_at_or_before(top - gap, scale_factor),
        device_pixel_at_or_after(bottom + gap, scale_factor),
    )
}

/// GitTurtle patch: the device pixel row a pointer's place, `y`, lies in on
/// a display of `scale_factor`, as its top and bottom edges in logical
/// pixels.
pub fn point_device_row(y: Pixels, scale_factor: f32) -> (Pixels, Pixels) {
    const TOLERANCE: f32 = 1e-3;
    let scale_factor = scale_factor.max(f32::EPSILON);
    let row = (f32::from(y) * scale_factor + TOLERANCE).floor();
    (px(row / scale_factor), px((row + 1.) / scale_factor))
}

/// GitTurtle patch: how near a popup's trailing edge lies to the window's
/// right or foot margin, or a popup's edge to a point's gap, to stand on
/// it.
const SIDE_TOLERANCE: Pixels = px(0.01);

/// GitTurtle patch: the first device pixel boundary at or after `x` on a
/// display of `scale_factor`, in logical pixels.
pub fn device_pixel_at_or_after(x: Pixels, scale_factor: f32) -> Pixels {
    const TOLERANCE: f32 = 1e-3;
    let scale_factor = scale_factor.max(f32::EPSILON);
    px((f32::from(x) * scale_factor - TOLERANCE).ceil() / scale_factor)
}

/// GitTurtle patch: the last device pixel boundary at or before `x` on a
/// display of `scale_factor`, in logical pixels.
pub fn device_pixel_at_or_before(x: Pixels, scale_factor: f32) -> Pixels {
    const TOLERANCE: f32 = 1e-3;
    let scale_factor = scale_factor.max(f32::EPSILON);
    px((f32::from(x) * scale_factor + TOLERANCE).floor() / scale_factor)
}

impl Align {
    /// The physical alignment along the horizontal axis: `Start` is the
    /// left edge in LTR and the right edge in RTL.
    fn horizontal(self, direction: LayoutDirection) -> Self {
        match (self, direction) {
            (Align::Start, LayoutDirection::Rtl) => Align::End,
            (Align::End, LayoutDirection::Rtl) => Align::Start,
            (align, _) => align,
        }
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
    corner_position: Option<Rc<Cell<Point<Pixels>>>>,
    /// GitTurtle patch: where a side popup's preferred side is read as it is
    /// prepainted ([`Positioner::placement_following`]).
    placement_following: Option<Rc<Cell<Option<Placement>>>>,
    on_position: Option<Box<dyn Fn(ResolvedPosition)>>,
    margin: Pixels,
    /// GitTurtle patch: the room kept clear at the viewport's foot
    /// ([`Positioner::bottom_inset`]).
    bottom_inset: Pixels,
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
            corner_position: None,
            placement_following: None,
            on_position: None,
            margin: px(4.),
            bottom_inset: px(0.),
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
            corner_position: None,
            placement_following: None,
            on_position: None,
            margin: px(4.),
            bottom_inset: px(0.),
            occlude: false,
            children: Vec::new(),
        }
    }

    /// GitTurtle patch: places the popup at `position`, a pointer's place,
    /// so that it never covers the point: wholly below the pointer's device
    /// pixel row where it fits, otherwise wholly above it, otherwise on the
    /// side with more room.
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
    /// side with more. `None` for the other placements.
    pub fn point_room(&self, window: &Window) -> Option<Pixels> {
        let (viewport, margin, _) = self.viewport(window);
        self.strategy_in(window).point_room(viewport, margin)
    }

    /// GitTurtle patch: the strategy as `window` resolves it: a followed
    /// corner as it stands now, a followed side as it stands now where it
    /// holds one, and a point on the window's device pixels.
    fn strategy_in(&self, window: &Window) -> Strategy {
        let mut strategy = self.strategy;
        match (&mut strategy, &self.corner_position) {
            (Strategy::Corner { position, .. }, Some(following)) => *position = following.get(),
            (Strategy::Side { placement, .. }, _) => {
                if let Some(followed) = self
                    .placement_following
                    .as_ref()
                    .and_then(|side| side.get())
                {
                    *placement = Some(followed);
                }
            }
            (Strategy::Point { scale_factor, .. }, _) => *scale_factor = window.scale_factor(),
            _ => {}
        }
        strategy
    }

    /// The viewport a popup is resolved in, its per-side margins, and its
    /// frame insets: the window's untiled frame insets plus `self.margin`,
    /// with the viewport shortened by the part of `self.bottom_inset` past
    /// the bottom margin.
    fn viewport(&self, window: &Window) -> (Size<Pixels>, Edges<Pixels>, Edges<Pixels>) {
        let frame = frame_insets(
            window.window_decorations(),
            window.client_inset().unwrap_or(px(0.)),
        );
        let margin = frame.map(|inset| *inset + self.margin);
        let mut viewport = window.viewport_size();
        viewport.height -= (self.bottom_inset - margin.bottom).max(px(0.));
        (viewport, margin, frame)
    }

    /// Updates the requested corner position.
    ///
    /// This is useful when an animation moves an already-composed popup. It
    /// has no effect on a side-positioned popup.
    pub fn position(mut self, position: Point<Pixels>) -> Self {
        if let Strategy::Corner {
            position: current, ..
        } = &mut self.strategy
        {
            *current = position;
        }
        self
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

    /// GitTurtle patch: prefers the side `placement` holds when the popup is
    /// prepainted, not when it is built, where it holds one, and otherwise
    /// [`Positioner::placement`]'s. Only meaningful for [`Positioner::side`].
    pub fn placement_following(mut self, placement: Rc<Cell<Option<Placement>>>) -> Self {
        self.placement_following = Some(placement);
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
    /// [`Positioner::side`] and [`Positioner::point`].
    pub fn offset(mut self, offset: Pixels) -> Self {
        match &mut self.strategy {
            Strategy::Side { offset: slot, .. } | Strategy::Point { gap: slot, .. } => {
                *slot = offset;
            }
            Strategy::Corner { .. } => {}
        }
        self
    }

    // Read after trigger prepaint so an open popup follows a moving trigger.
    pub(crate) fn tracked_corner_position(mut self, position: Rc<Cell<Point<Pixels>>>) -> Self {
        self.corner_position = Some(position);
        self
    }

    /// Observe resolved geometry before children prepaint.
    pub fn on_position(mut self, callback: impl Fn(ResolvedPosition) + 'static) -> Self {
        self.on_position = Some(Box::new(callback));
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

/// The part of the window's viewport that is frame rather than content, per
/// side.
///
/// A window drawn with client-side decorations pads its content by the client
/// inset to make room for a shadow, except along an edge that is tiled against
/// the screen, where the frame draws no shadow and the content runs to the
/// viewport edge. `Window::client_inset` is the one value on every side (the
/// platform needs it stable across tiling changes to size the window), so the
/// tiling is what says where it actually applies. A server-decorated window
/// has no frame of its own.
fn frame_insets(decorations: Decorations, client_inset: Pixels) -> Edges<Pixels> {
    match decorations {
        Decorations::Server => Edges::default(),
        Decorations::Client { tiling } => Edges {
            top: if tiling.top { px(0.) } else { client_inset },
            right: if tiling.right { px(0.) } else { client_inset },
            bottom: if tiling.bottom { px(0.) } else { client_inset },
            left: if tiling.left { px(0.) } else { client_inset },
        },
    }
}

/// Resolves the bounds of a popup of `popup_size`.
///
/// Side placement picks the preferred side when the popup fits, otherwise the
/// opposite side, otherwise whichever side has more room. The result is always
/// clamped into the viewport, keeping `margin` from each edge.
#[cfg(test)]
fn resolve(
    strategy: Strategy,
    popup_size: Size<Pixels>,
    viewport_size: Size<Pixels>,
    margin: Edges<Pixels>,
) -> ResolvedPosition {
    resolve_in(
        strategy,
        popup_size,
        viewport_size,
        margin,
        LayoutDirection::Ltr,
    )
}

/// [`resolve`] for a layout direction: horizontal [`Align::Start`] and
/// [`Align::End`] follow it, while [`Placement`] sides stay physical.
fn resolve_in(
    strategy: Strategy,
    popup_size: Size<Pixels>,
    viewport_size: Size<Pixels>,
    margin: Edges<Pixels>,
    direction: LayoutDirection,
) -> ResolvedPosition {
    match strategy {
        Strategy::Point { .. } => {
            resolve_in(strategy.as_side(), popup_size, viewport_size, margin, direction)
        }
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
            let origin = side_origin(
                trigger_bounds,
                popup_size,
                placement,
                align,
                offset,
                direction,
            );
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
    margin: Edges<Pixels>,
    preferred: Option<Placement>,
) -> Placement {
    let right_limit = (viewport_size.width - margin.right).max(margin.left);
    let bottom_limit = (viewport_size.height - margin.bottom).max(margin.top);
    let available_left = (trigger_bounds.left() - margin.left).max(px(0.));
    let available_right = (right_limit - trigger_bounds.right()).max(px(0.));
    let available_above = (trigger_bounds.top() - margin.top).max(px(0.));
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
    direction: LayoutDirection,
) -> Point<Pixels> {
    let aligned_x = match align.horizontal(direction) {
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
    margin: Edges<Pixels>,
) -> Bounds<Pixels> {
    let right_limit = (viewport_size.width - margin.right).max(margin.left);
    let bottom_limit = (viewport_size.height - margin.bottom).max(margin.top);

    if bounds.right() > right_limit {
        bounds.origin.x -= bounds.right() - right_limit;
    }
    if bounds.left() < margin.left {
        bounds.origin.x = margin.left;
    }
    if bounds.bottom() > bottom_limit {
        bounds.origin.y -= bounds.bottom() - bottom_limit;
    }
    if bounds.top() < margin.top {
        bounds.origin.y = margin.top;
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
        let (viewport, margin, frame) = self.viewport(window);
        // GitTurtle patch: a followed corner or side as it stands now, and a
        // point on the window's device pixels.
        let strategy = self.strategy_in(window);
        let position = resolve_in(
            strategy,
            popup_size,
            viewport,
            margin,
            LayoutDirection::global(cx),
        );
        if let Some(callback) = &self.on_position {
            callback(position);
        }

        let offset = position.bounds.origin - bounds.origin;
        let scale_factor = window.scale_factor();
        let bottom = (viewport.height - margin.bottom).max(margin.top);
        let (top, foot) = (
            device_pixel_at_or_after(margin.top, scale_factor),
            device_pixel_at_or_before(bottom, scale_factor),
        );
        let height = position.bounds.size.height;
        let window_top = device_pixel_at_or_after(frame.top, scale_factor);
        let y = if strategy.clear_of_point(position.bounds) {
            offset.y
        } else if position.bounds.top() <= margin.top
            && height - (foot - top) > SIDE_TOLERANCE
            && height - (foot - window_top) < SIDE_TOLERANCE
        {
            (foot - height).max(window_top) - bounds.origin.y
        } else if position.bounds.top() <= margin.top {
            top - bounds.origin.y
        } else if bottom - position.bounds.bottom() < SIDE_TOLERANCE {
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
        let right = (viewport.width - margin.right).max(margin.left);
        let x = if position.bounds.left() <= margin.left {
            device_pixel_at_or_after(margin.left, scale_factor) - bounds.origin.x
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
                x if position.bounds.left() >= margin.left
                    && position.bounds.left() + (x - offset.x) < margin.left =>
                {
                    offset.x.ceil()
                }
                x => x,
            }
        };
        let offset = point(x, y);
        // Ahead of the children so it blocks what is behind the popup without
        // blocking the popup's own content. GitTurtle patch: at the bounds
        // the popup is drawn at, not the bounds it was resolved at.
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

#[cfg(not(feature = "gpui-fast"))]
impl gpui::IntoElement for Positioner {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Tiling;

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
            Edges::all(MARGIN),
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
    fn rtl_aligns_start_with_the_right_edge_of_the_trigger() {
        let strategy = |align| Strategy::Side {
            trigger_bounds: trigger(200., 200., 100., 20.),
            placement: Some(Placement::Bottom),
            align,
            offset: px(0.),
        };
        let popup = Size::new(px(40.), px(30.));
        let resolve_rtl = |align| {
            resolve_in(
                strategy(align),
                popup,
                viewport(),
                Edges::all(MARGIN),
                LayoutDirection::Rtl,
            )
        };

        assert_eq!(resolve_rtl(Align::Start).bounds.right(), px(300.));
        assert_eq!(resolve_rtl(Align::Center).bounds.left(), px(230.));
        assert_eq!(resolve_rtl(Align::End).bounds.left(), px(200.));
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
            Edges::all(MARGIN),
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
            Edges::all(MARGIN),
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
            Edges::all(MARGIN),
        );

        assert_eq!(position.placement, None);
        assert_eq!(position.bounds.right(), viewport().width - MARGIN);
        assert_eq!(position.bounds.bottom(), viewport().height - MARGIN);
    }

    #[test]
    fn corner_position_can_be_updated_without_rebuilding_the_positioner() {
        let positioner = Positioner::corner(Anchor::TopLeft, point(px(10.), px(20.)))
            .position(point(px(30.), px(40.)));

        assert!(matches!(
            positioner.strategy,
            Strategy::Corner {
                position,
                ..
            } if position == point(px(30.), px(40.))
        ));
    }

    /// A tiled edge draws no shadow, so a popup may run right up to the
    /// margin there; an untiled edge keeps the whole client inset as well.
    #[test]
    fn frame_insets_apply_the_client_inset_only_on_untiled_edges() {
        let inset = px(20.);

        assert_eq!(
            frame_insets(Decorations::Server, inset),
            Edges::default(),
            "a server-decorated window has no frame to keep clear of"
        );
        assert_eq!(
            frame_insets(
                Decorations::Client {
                    tiling: Tiling::tiled()
                },
                inset
            ),
            Edges::default(),
            "a window tiled on every side draws no shadow padding"
        );
        assert_eq!(
            frame_insets(
                Decorations::Client {
                    tiling: Tiling {
                        top: false,
                        left: true,
                        right: false,
                        bottom: true,
                    }
                },
                inset
            ),
            Edges {
                top: inset,
                right: inset,
                bottom: px(0.),
                left: px(0.),
            }
        );
    }

    /// The bug this guards: a menu opened from a trigger near the right edge
    /// of a tiled window was pushed inward by the client inset although no
    /// shadow was drawn there, so it no longer lined up with its trigger.
    #[test]
    fn clamping_keeps_only_the_margin_on_a_tiled_edge() {
        let popup = Size::new(px(120.), px(30.));
        // A menu aligned to a trigger that ends 10px short of the right edge.
        let corner = Strategy::Corner {
            anchor: Anchor::TopRight,
            position: point(viewport().width - px(10.), px(100.)),
        };
        let margin = Edges::all(MARGIN);
        let inset = px(20.);

        let tiled = resolve(corner, popup, viewport(), margin);
        assert_eq!(tiled.bounds.right(), viewport().width - px(10.));

        let untiled = resolve(
            corner,
            popup,
            viewport(),
            margin.map(|margin| *margin + inset),
        );
        assert_eq!(untiled.bounds.right(), viewport().width - MARGIN - inset);
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

    #[test]
    fn point_positioning_opens_below_or_wholly_above_the_point() {
        let at = |y: f32, height: f32| {
            resolve(
                point_at(y, 0., 1.),
                Size::new(px(40.), px(height)),
                viewport(),
                Edges::all(MARGIN),
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

        assert_eq!(at(220., 300.).placement, Some(Placement::Top));
        assert_eq!(at(180., 300.).placement, Some(Placement::Bottom));
    }

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
                        Edges::all(MARGIN),
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

    #[test]
    fn point_room_is_the_room_on_the_side_with_more() {
        for (y, room, side) in [(100.5, 294., Placement::Bottom), (300.5, 295., Placement::Top)] {
            let strategy = point_at(y, 1., 1.);
            assert_eq!(
                strategy.point_room(viewport(), Edges::all(MARGIN)),
                Some(px(room)),
                "{y}"
            );
            let capped = resolve(
                strategy,
                Size::new(px(40.), px(room)),
                viewport(),
                Edges::all(MARGIN),
            );
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
        assert_eq!(corner.point_room(viewport(), Edges::all(MARGIN)), None);
    }

    #[gpui::test]
    fn a_popup_above_a_point_ends_where_its_gap_does(cx: &mut gpui::TestAppContext) {
        use gpui::{InteractiveElement as _, IntoElement, Styled as _};

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
        assert_eq!(popup.bottom(), px(349.), "{popup:?}");
        assert_eq!(popup.top(), px(248.5), "{popup:?}");
    }

    #[gpui::test]
    fn a_popup_at_a_point_keeps_its_ring_off_the_pointers_row_at_scale_1_25(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{InteractiveElement as _, IntoElement, Styled as _};
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

    #[gpui::test]
    fn a_popup_moved_in_from_a_side_lies_on_the_device_pixel_at_its_margin(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{InteractiveElement as _, IntoElement, ParentElement as _, Styled as _};
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

    #[gpui::test]
    fn a_popup_at_a_point_lies_on_the_device_pixel_past_its_gap(cx: &mut gpui::TestAppContext) {
        use gpui::{InteractiveElement as _, IntoElement, Styled as _};
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

    #[gpui::test]
    fn a_popup_clamped_to_the_top_or_foot_lies_on_the_device_pixel_at_its_margin(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{InteractiveElement as _, IntoElement, ParentElement as _, Styled as _};
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
                    (2., true, Some(at_top)),
                    (HEIGHT - 40., false, Some(at_foot)),
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

    #[gpui::test]
    fn a_popup_clamped_at_both_ends_on_the_device_pixel_at_the_foot_margin(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{InteractiveElement as _, IntoElement, ParentElement as _, Styled as _};
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
                    (room + 1., top - 1.),
                    (foot, 0.),
                    (foot + 1., top),
                    (device(px(HEIGHT)), top),
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

    #[gpui::test]
    fn an_occluding_popup_blocks_the_mouse_where_it_is_drawn(cx: &mut gpui::TestAppContext) {
        use gpui::{
            InteractiveElement as _, IntoElement, Modifiers, MouseButton, ParentElement as _,
            Styled as _,
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
            Edges::all(margin),
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
