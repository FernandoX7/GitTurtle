// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{Color, Point, Rect};
use accesskit_consumer::{Node, TextPosition, TextRange};
use objc2::encode::{Encoding, RefEncode};
use objc2::{msg_send, rc::Id, runtime::AnyObject};
use objc2_app_kit::*;
use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize};

pub(crate) fn from_ns_range<'a>(node: &'a Node<'a>, ns_range: NSRange) -> Option<TextRange<'a>> {
    let pos = node.text_position_from_global_utf16_index(ns_range.location)?;
    let mut range = pos.to_degenerate_range();
    if ns_range.length > 0 {
        let end =
            node.text_position_from_global_utf16_index(ns_range.location + ns_range.length)?;
        range.set_end(end);
    }
    Some(range)
}

pub(crate) fn to_ns_range(range: &TextRange) -> NSRange {
    let start = range.start().to_global_utf16_index();
    let end = range.end().to_global_utf16_index();
    NSRange::from(start..end)
}

pub(crate) fn to_ns_range_for_character(pos: &TextPosition) -> NSRange {
    let mut range = pos.to_degenerate_range();
    if !pos.is_document_end() {
        range.set_end(pos.forward_to_character_end());
    }
    to_ns_range(&range)
}

pub(crate) fn from_ns_point(view: &NSView, node: &Node, point: NSPoint) -> Point {
    let window = view.window().unwrap();
    let point = window.convertPointFromScreen(point);
    let point = view.convertPoint_fromView(point, None);
    // AccessKit coordinates are in physical (DPI-dependent) pixels, but
    // macOS provides logical (DPI-independent) coordinates here.
    let factor = window.backingScaleFactor();
    let point = Point::new(
        point.x * factor,
        if view.isFlipped() {
            point.y * factor
        } else {
            let view_bounds = view.bounds();
            (view_bounds.size.height - point.y) * factor
        },
    );
    node.transform().inverse() * point
}

pub(crate) fn to_ns_rect(view: &NSView, rect: Rect) -> NSRect {
    let window = view.window().unwrap();
    // AccessKit coordinates are in physical (DPI-dependent)
    // pixels, but macOS expects logical (DPI-independent)
    // coordinates here.
    let factor = window.backingScaleFactor();
    let rect = NSRect {
        origin: NSPoint {
            x: rect.x0 / factor,
            y: if view.isFlipped() {
                rect.y0 / factor
            } else {
                let view_bounds = view.bounds();
                view_bounds.size.height - rect.y1 / factor
            },
        },
        size: NSSize {
            width: rect.width() / factor,
            height: rect.height() / factor,
        },
    };
    let rect = view.convertRect_toView(rect, None);
    let window = view.window().unwrap();
    window.convertRectToScreen(rect)
}

fn color_channel_to_f64(channel: u8) -> f64 {
    (channel as f64) / 255.0
}

// TODO: can be removed after updating objc2 to 0.6 which has proper `CGColor` support
#[repr(C)]
struct CGColor {
    _private: [u8; 0],
}

unsafe impl RefEncode for CGColor {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGColor", &[]));
}

pub(crate) fn to_color_attribute(color: Color) -> Id<AnyObject> {
    let ns_color = unsafe {
        NSColor::colorWithSRGBRed_green_blue_alpha(
            color_channel_to_f64(color.red),
            color_channel_to_f64(color.green),
            color_channel_to_f64(color.blue),
            color_channel_to_f64(color.alpha),
        )
    };
    let cg_color: *const CGColor = unsafe { msg_send![&ns_color, CGColor] };
    unsafe { Id::retain(cg_color as *mut AnyObject).unwrap() }
}

// GitTurtle patch (GITTURTLE-PATCH.md): reject non-finite or empty/inverted
// rectangles so NaN/Inf/degenerate layout coordinates fail closed as clipped out.
fn is_valid_rect(rect: Rect) -> bool {
    rect.x0.is_finite()
        && rect.y0.is_finite()
        && rect.x1.is_finite()
        && rect.y1.is_finite()
        && rect.x1 > rect.x0
        && rect.y1 > rect.y0
}

// GitTurtle patch (GITTURTLE-PATCH.md): bounding boxes of all clipping
// ancestors of `node`, nearest first, in root container physical coordinates.
pub(crate) fn clips<'a>(node: &Node<'a>) -> impl Iterator<Item = Rect> + 'a {
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .filter(|ancestor| ancestor.clips_children())
        .filter_map(|ancestor| ancestor.bounding_box())
}

// GitTurtle patch (GITTURTLE-PATCH.md): intersect `node`'s bounding box with
// every clipping ancestor so partially clipped VoiceOver focus boxes stay
// inside the scroll viewport and wholly clipped-out controls return `None`
// (`NSRect::ZERO`) instead of overlapping unrelated sibling controls.
pub(crate) fn clipped_bounds(node: &Node<'_>) -> Option<Rect> {
    let mut rect = node.bounding_box()?;
    if !is_valid_rect(rect) {
        return None;
    }
    for clip in clips(node) {
        if !is_valid_rect(clip) {
            return None;
        }
        let x0 = rect.x0.max(clip.x0);
        let y0 = rect.y0.max(clip.y0);
        let x1 = rect.x1.min(clip.x1);
        let y1 = rect.y1.min(clip.y1);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        rect = Rect::new(x0, y0, x1, y1);
    }
    Some(rect)
}

// GitTurtle patch (GITTURTLE-PATCH.md): whether any clipping ancestor of
// `node` excludes `point` (in root container physical coordinates).
pub(crate) fn clipped_at(node: &Node<'_>, point: Point) -> bool {
    if !point.x.is_finite() || !point.y.is_finite() {
        return true;
    }
    clips(node).any(|clip| !is_valid_rect(clip) || !clip.contains(point))
}

#[cfg(test)]
mod clip_tests {
    use super::*;
    use accesskit::{Node as NodeBuilder, NodeId, Role, Tree as TreeData, TreeId, TreeUpdate};
    use accesskit_consumer::{FilterResult, Tree};

    fn make_node(role: Role, bounds: Rect, clips_children: bool, children: Vec<NodeId>) -> NodeBuilder {
        let mut node = NodeBuilder::new(role);
        node.set_bounds(bounds);
        if clips_children {
            node.set_clips_children();
        }
        if !children.is_empty() {
            node.set_children(children);
        }
        node
    }

    #[test]
    fn clipped_bounds_and_hit_test_respect_single_and_nested_clips() {
        let root_id = NodeId(0);
        let outer_id = NodeId(1);
        let inner_id = NodeId(2);
        let inside_id = NodeId(3);
        let partial_id = NodeId(4);
        let outside_id = NodeId(5);
        let nested_outside_id = NodeId(6);

        let update = TreeUpdate {
            nodes: vec![
                (
                    root_id,
                    make_node(
                        Role::Window,
                        Rect::new(0., 0., 400., 300.),
                        false,
                        vec![outer_id],
                    ),
                ),
                (
                    outer_id,
                    make_node(
                        Role::ScrollView,
                        Rect::new(0., 40., 400., 140.),
                        true,
                        vec![inside_id, partial_id, outside_id, inner_id],
                    ),
                ),
                (
                    inside_id,
                    make_node(Role::Button, Rect::new(10., 50., 110., 80.), false, vec![]),
                ),
                (
                    partial_id,
                    make_node(Role::Button, Rect::new(10., 120., 110., 170.), false, vec![]),
                ),
                (
                    outside_id,
                    make_node(Role::Button, Rect::new(10., 180., 110., 210.), false, vec![]),
                ),
                (
                    inner_id,
                    make_node(
                        Role::ScrollView,
                        Rect::new(0., 150., 400., 250.),
                        true,
                        vec![nested_outside_id],
                    ),
                ),
                (
                    nested_outside_id,
                    make_node(Role::Button, Rect::new(10., 160., 110., 190.), false, vec![]),
                ),
            ],
            tree: Some(TreeData::new(root_id)),
            tree_id: TreeId::ROOT,
            focus: root_id,
        };
        let tree = Tree::new(update, true);
        let state = tree.state();
        let root = state.root();

        assert_eq!(
            clipped_bounds(&state.node_by_tree_local_id(inside_id, TreeId::ROOT).unwrap()),
            Some(Rect::new(10., 50., 110., 80.))
        );
        assert_eq!(
            clipped_bounds(&state.node_by_tree_local_id(partial_id, TreeId::ROOT).unwrap()),
            Some(Rect::new(10., 120., 110., 140.))
        );
        assert_eq!(
            clipped_bounds(&state.node_by_tree_local_id(outside_id, TreeId::ROOT).unwrap()),
            None
        );
        // Inside `inner_id` (150..250) yet wholly outside `outer_id` (40..140).
        assert_eq!(
            clipped_bounds(
                &state
                    .node_by_tree_local_id(nested_outside_id, TreeId::ROOT)
                    .unwrap()
            ),
            None
        );

        let hit = |pt: Point| {
            let container_pt = root.transform() * pt;
            root.node_at_point(pt, &|candidate| {
                if clipped_at(candidate, container_pt) {
                    FilterResult::ExcludeSubtree
                } else {
                    FilterResult::Include
                }
            })
            .map(|n| n.locate().0)
        };
        // Visible half of `partial_id` hits `partial_id`; clipped lower half does not.
        assert_eq!(hit(Point::new(50., 130.)), Some(partial_id));
        assert_ne!(hit(Point::new(50., 155.)), Some(partial_id));
        // Wholly clipped-out buttons are never hit.
        assert_ne!(hit(Point::new(50., 195.)), Some(outside_id));
        assert_ne!(hit(Point::new(50., 175.)), Some(nested_outside_id));
    }

    #[test]
    fn clipped_bounds_and_hit_test_fail_closed_on_non_finite_and_inverted_rects() {
        let root_id = NodeId(0);
        let nan_clip_id = NodeId(1);
        let nan_child_id = NodeId(2);
        let inverted_id = NodeId(3);
        let inf_id = NodeId(4);

        let update = TreeUpdate {
            nodes: vec![
                (
                    root_id,
                    make_node(
                        Role::Window,
                        Rect::new(0., 0., 400., 300.),
                        false,
                        vec![nan_clip_id, inverted_id, inf_id],
                    ),
                ),
                (
                    nan_clip_id,
                    make_node(
                        Role::ScrollView,
                        Rect::new(0., f64::NAN, 400., 140.),
                        true,
                        vec![nan_child_id],
                    ),
                ),
                (
                    nan_child_id,
                    make_node(Role::Button, Rect::new(10., 50., 110., 80.), false, vec![]),
                ),
                (
                    inverted_id,
                    make_node(Role::Button, Rect::new(110., 50., 10., 80.), false, vec![]),
                ),
                (
                    inf_id,
                    make_node(
                        Role::Button,
                        Rect::new(10., 50., f64::INFINITY, 80.),
                        false,
                        vec![],
                    ),
                ),
            ],
            tree: Some(TreeData::new(root_id)),
            tree_id: TreeId::ROOT,
            focus: root_id,
        };
        let tree = Tree::new(update, true);
        let state = tree.state();

        let nan_child = state
            .node_by_tree_local_id(nan_child_id, TreeId::ROOT)
            .unwrap();
        assert_eq!(clipped_bounds(&nan_child), None);
        assert!(clipped_at(&nan_child, Point::new(50., 60.)));
        assert_eq!(
            clipped_bounds(&state.node_by_tree_local_id(inverted_id, TreeId::ROOT).unwrap()),
            None
        );
        assert_eq!(
            clipped_bounds(&state.node_by_tree_local_id(inf_id, TreeId::ROOT).unwrap()),
            None
        );
    }
}
