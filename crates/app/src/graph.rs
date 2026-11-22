use crate::appearance::palette;
pub use gitturtle_diff_model::graph::*;
pub(crate) use gitturtle_theme::palette_colors;
use gpui_kit::{
    AnyElement, App, Bounds, ContentMask, IntoElement, ParentElement, PathBuilder, Styled, canvas,
    div, fill, point, px, quad, rgb, size,
};

/// Colors change with appearance; worker-prepared lane identities do not.
pub(super) fn colors(cx: &App) -> [u32; 6] {
    palette_colors(palette(cx))
}

pub struct RowStyle {
    pub active: bool,
    pub merge: bool,
    pub filtered: bool,
}

pub fn render(
    row: GraphRow,
    width: f32,
    lane_offset: usize,
    lane_spacing: f32,
    row_height: f32,
    style: RowStyle,
) -> AnyElement {
    let RowStyle {
        active,
        merge,
        filtered,
    } = style;
    // The lanes the cell holds whole, from `lane_offset`.
    let last = lane_offset + visible_lane_capacity(width, lane_spacing) - 1;
    div()
        .w(px(width))
        .h(px(row_height))
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let palette = palette(cx);
                    let colors = palette_colors(palette);
                    let x =
                        |lane| bounds.origin.x + px(lane_x(width, lane_offset, lane, lane_spacing));
                    let top = bounds.origin.y;
                    let middle = top + bounds.size.height / 2.;
                    let bottom = top + bounds.size.height;
                    // Strokes end half a pitch past the lanes the cell holds,
                    // short of the next lane's node and line, so a lane the
                    // cell has no room for shows no cut node or line at its
                    // edge; an edge to it leaves toward it.
                    let half = px(lane_spacing / 2.);
                    let visible_left = x(lane_offset) - half;
                    let visible_right = x(last) + half;
                    let band = Bounds::from_corners(
                        point(visible_left, top),
                        point(visible_right, bottom),
                    );
                    let scale = window.scale_factor().max(1.);
                    let stroke_dp = (EDGE_WIDTH * scale).round().max(1.);
                    let stroke = px(stroke_dp / scale);
                    let half_stroke = stroke / 2.;
                    // Snap each straight vertical stroke to an integer device-pixel
                    // span of exact `stroke_dp` width so straight quads never
                    // oscillate by 1 px across lanes or mismatch curved thickness,
                    // while keeping curved endpoints and commit nodes at exact `x(lane)`.
                    let lane_quad_span = |lane| {
                        let raw_center = f32::from(x(lane));
                        let left_dp = (raw_center * scale - stroke_dp * 0.5).round();
                        let right_dp = left_dp + stroke_dp;
                        (px(left_dp / scale), px(right_dp / scale))
                    };
                    window.with_content_mask(Some(ContentMask { bounds: band }), |window| {
                        if !filtered {
                            // Straight vertical lanes paint directly as GPU quads
                            // without Lyon path tessellation; curves meet adjacent
                            // rows at exact lane coordinates with vertical tangents.
                            for edge in row.edges.iter() {
                                let start_y = if edge.from_node { middle } else { top };
                                let color = rgb(colors[edge.color % colors.len()]);
                                if edge.from == edge.to {
                                    let (left, right) = lane_quad_span(edge.from);
                                    if right < visible_left || left > visible_right {
                                        continue;
                                    }
                                    window.paint_quad(fill(
                                        Bounds::from_corners(
                                            point(left, start_y),
                                            point(right, bottom),
                                        ),
                                        color,
                                    ));
                                    continue;
                                }
                                let start_x = x(edge.from);
                                let end_x = x(edge.to);
                                if start_x.max(end_x) + half_stroke < visible_left
                                    || start_x.min(end_x) - half_stroke > visible_right
                                {
                                    continue;
                                }
                                let start = point(start_x, start_y);
                                let end = point(end_x, bottom);
                                let distance = end.y - start.y;
                                let mut path = PathBuilder::stroke(stroke);
                                path.move_to(start);
                                path.cubic_bezier_to(
                                    end,
                                    point(start.x, start.y + distance * 0.5),
                                    point(end.x, end.y - distance * 0.5),
                                );
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, color);
                                }
                            }
                            if row.incoming {
                                let (left, right) = lane_quad_span(row.lane);
                                if right >= visible_left && left <= visible_right {
                                    window.paint_quad(fill(
                                        Bounds::from_corners(
                                            point(left, top),
                                            point(right, middle),
                                        ),
                                        rgb(colors[row.color % colors.len()]),
                                    ));
                                }
                            }
                        }
                    });
                    // Search results are discontinuous history: isolated nodes do
                    // not imply parent relationships across hidden rows.
                    let lane = if filtered { 0 } else { row.lane };
                    // A node only on a lane the cell holds whole.
                    if !(lane_offset..=last).contains(&lane) {
                        return;
                    }
                    let center = point(x(lane), middle);
                    let color = rgb(colors[row.color % colors.len()]);
                    let radius = if active {
                        SELECTED_RADIUS
                    } else if merge {
                        4.3
                    } else {
                        3.8
                    };
                    window.paint_quad(quad(
                        Bounds::new(
                            center - point(px(radius), px(radius)),
                            size(px(radius * 2.), px(radius * 2.)),
                        ),
                        px(radius),
                        if merge {
                            rgb(if active {
                                palette.selected
                            } else {
                                palette.canvas
                            })
                        } else {
                            color
                        },
                        px(if merge {
                            1.8
                        } else if active {
                            1.
                        } else {
                            0.
                        }),
                        if active { rgb(palette.text) } else { color },
                        Default::default(),
                    ));
                },
            )
            .size_full(),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[crate::gpui::test]
    async fn straight_vertical_lanes_paint_as_masked_quads_and_skip_offscreen_or_filtered(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::{Context, Render, Window};

        struct GraphHarness {
            row: GraphRow,
            width: f32,
            lane_offset: usize,
            lane_spacing: f32,
            row_height: f32,
            filtered: bool,
        }

        impl Render for GraphHarness {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                render(
                    self.row.clone(),
                    self.width,
                    self.lane_offset,
                    self.lane_spacing,
                    self.row_height,
                    RowStyle {
                        active: false,
                        merge: false,
                        filtered: self.filtered,
                    },
                )
            }
        }

        // Row on lane 1 with incoming segment, straight pass-through on lane 0,
        // straight outgoing on lane 1, curved branch to lane 2, and an offscreen
        // pass-through on lane 8 outside the visible 3-lane viewport.
        let row = GraphRow {
            lane: 1,
            color: 1,
            incoming: true,
            edges: Arc::from([
                Edge {
                    from: 0,
                    to: 0,
                    color: 0,
                    from_node: false,
                },
                Edge {
                    from: 1,
                    to: 1,
                    color: 1,
                    from_node: true,
                },
                Edge {
                    from: 1,
                    to: 2,
                    color: 2,
                    from_node: true,
                },
                Edge {
                    from: 8,
                    to: 8,
                    color: 3,
                    from_node: false,
                },
            ]),
            width: 9,
        };
        let (view, cx) = cx.add_window_view(|_, _| GraphHarness {
            row,
            width: 64.,
            lane_offset: 0,
            lane_spacing: 19.3,
            row_height: 28.,
            filtered: false,
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let (scale, quads) = cx.update(|window, _| (window.scale_factor(), window.painted_quads()));
        let expected_dp = (EDGE_WIDTH * scale.max(1.)).round().max(1.);
        // Straight lane strokes paint as sharp rectangular quads with uniform
        // device-pixel width across lanes, even at fractional lane spacing.
        let stroke_quads: Vec<_> = quads
            .iter()
            .filter(|q| q.corner_radii.top_left.0 == 0. && q.bounds.size.width.0 / scale <= 2.5)
            .collect();
        // Expect 3 vertical stroke quads: pass-through lane 0 (full 28 px),
        // outgoing lane 1 (half 14 px), and incoming lane 1 (half 14 px).
        // Offscreen lane 8 is culled; curved edge 1->2 uses a path, not a quad.
        assert_eq!(stroke_quads.len(), 3, "stroke_quads={stroke_quads:#?}");
        for stroke_quad in &stroke_quads {
            assert!(
                (stroke_quad.bounds.size.width.0 - expected_dp).abs() < 0.01,
                "uniform device-pixel width expected {expected_dp}, got {}",
                stroke_quad.bounds.size.width.0
            );
        }
        let mut heights: Vec<i32> = stroke_quads
            .iter()
            .map(|q| (q.bounds.size.height.0 / scale).round() as i32)
            .collect();
        heights.sort_unstable();
        assert_eq!(heights, [14, 14, 28]);

        // Filtered rows hide ancestry strokes and paint only the node circle.
        view.update(cx, |harness, cx| {
            harness.filtered = true;
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let filtered_strokes = cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter(|q| q.corner_radii.top_left.0 == 0. && q.bounds.size.width.0 / scale <= 2.5)
                .count()
        });
        assert_eq!(filtered_strokes, 0);
    }
}
