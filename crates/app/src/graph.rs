use crate::appearance::{Palette, palette};
use gitturtle_core::Commit;
use gpui_kit::{
    AnyElement, App, Bounds, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point,
    px, quad, rgb, size,
};
use std::collections::{HashMap, HashSet};

const DARK_COLORS: [u32; 6] = [0x7adfb4, 0x8db7f6, 0xc3a5f2, 0xe8be7a, 0xea9ca5, 0x72ccd8];
const LIGHT_COLORS: [u32; 6] = [0x146c53, 0x315da7, 0x7651aa, 0x8c5916, 0xa42d63, 0x156b7c];
const NODE_MARGIN: f32 = 10.;
const LANE_SPACING: f32 = 12.;

/// Colors change with appearance; worker-prepared lane identities do not.
pub(super) fn colors(cx: &App) -> [u32; 6] {
    palette_colors(palette(cx))
}

fn palette_colors(palette: Palette) -> [u32; 6] {
    // These themes use a light foreground on dark surfaces, or the inverse.
    // Keep palette selection cheap for each visible row and paint callback.
    let brightness = |color: u32| {
        ((color >> 16) & 255) * 2126 + ((color >> 8) & 255) * 7152 + (color & 255) * 722
    };
    if brightness(palette.canvas) > brightness(palette.text) {
        LIGHT_COLORS
    } else {
        DARK_COLORS
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Lane at the top boundary, or the commit's lane when `from_node`.
    pub from: usize,
    /// Lane at the bottom boundary, shared with the following row's top.
    pub to: usize,
    pub color: usize,
    pub from_node: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRow {
    pub lane: usize,
    pub color: usize,
    pub incoming: bool,
    pub edges: Vec<Edge>,
    /// Required lanes including both boundary frontiers and the node itself.
    pub width: usize,
}

#[derive(Clone, Copy)]
struct Lane<'a> {
    oid: &'a str,
    color: usize,
}

/// Lay out a topologically ordered, newest-first history. The frontier contains
/// one lane per pending parent OID, so converging ancestry joins before its
/// parent row. Boundary lane indices and colors are shared by adjacent rows.
///
/// Input parents can extend beyond a paged history; their edges remain open at
/// the bottom. Existing rows retain their topology when more rows are appended.
pub fn layout(commits: &[Commit]) -> Vec<GraphRow> {
    let mut lanes: Vec<Lane<'_>> = Vec::new();
    let mut next_color = 0;
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        let existing_lane = lanes.iter().position(|lane| lane.oid == commit.oid);
        let incoming = existing_lane.is_some();
        let lane = existing_lane.unwrap_or_else(|| {
            let index = lanes.len();
            lanes.push(Lane {
                oid: &commit.oid,
                color: next_color,
            });
            next_color += 1;
            index
        });
        // Borrowed OIDs make frontier snapshots cheap even in wide histories.
        let before = lanes.clone();
        let color = lanes[lane].color;
        lanes.remove(lane);

        let mut seen = HashSet::new();
        let parents: Vec<_> = commit
            .parents
            .iter()
            .map(String::as_str)
            .filter(|parent| seen.insert(*parent))
            .collect();
        let mut insertion = lane.min(lanes.len());
        for (index, &parent) in parents.iter().enumerate() {
            if !lanes.iter().any(|lane| lane.oid == parent) {
                let parent_color = if index == 0 {
                    color
                } else {
                    let allocated = next_color;
                    next_color += 1;
                    allocated
                };
                lanes.insert(
                    insertion,
                    Lane {
                        oid: parent,
                        color: parent_color,
                    },
                );
                insertion += 1;
            }
        }

        let positions: HashMap<_, _> = lanes
            .iter()
            .enumerate()
            .map(|(index, lane)| (lane.oid, index))
            .collect();
        let mut edges = Vec::with_capacity(before.len() - 1 + parents.len());
        for (from, previous) in before.iter().enumerate() {
            if from != lane
                && let Some(&to) = positions.get(previous.oid)
            {
                edges.push(Edge {
                    from,
                    to,
                    color: previous.color,
                    from_node: false,
                });
            }
        }
        for parent in parents {
            if let Some(&to) = positions.get(parent) {
                edges.push(Edge {
                    from: lane,
                    to,
                    // Match the destination lane at the next boundary. This
                    // matters for secondary parents and existing ancestry.
                    color: lanes[to].color,
                    from_node: true,
                });
            }
        }
        rows.push(GraphRow {
            lane,
            color,
            incoming,
            edges,
            width: before.len().max(lanes.len()),
        });
    }
    rows
}

/// One coordinate system for every row. The caller passes the maximum `width`
/// across the complete loaded graph, never a visible row's individual width.
/// Node-safe margins keep the last lane inside a bounded graph column even
/// when a repository has many simultaneously active branches.
fn lane_x(width: f32, lane_count: usize, lane: usize) -> f32 {
    let available = (width - NODE_MARGIN * 2.).max(0.);
    let spacing = LANE_SPACING.min(available / lane_count.saturating_sub(1).max(1) as f32);
    NODE_MARGIN.min(width / 2.) + lane as f32 * spacing
}

pub fn render(
    row: GraphRow,
    width: f32,
    lane_count: usize,
    row_height: f32,
    active: bool,
    merge: bool,
    filtered: bool,
) -> AnyElement {
    div()
        .w(px(width))
        .h(px(row_height))
        .flex_shrink_0()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let palette = palette(cx);
                    let colors = palette_colors(palette);
                    let x = |lane| bounds.origin.x + px(lane_x(width, lane_count, lane));
                    let top = bounds.origin.y;
                    let middle = top + bounds.size.height / 2.;
                    let bottom = top + bounds.size.height;
                    if !filtered {
                        // Curves meet adjacent rows at exact lane coordinates with
                        // vertical tangents, including collapsing lanes.
                        for edge in &row.edges {
                            let start =
                                point(x(edge.from), if edge.from_node { middle } else { top });
                            let end = point(x(edge.to), bottom);
                            let distance = end.y - start.y;
                            let mut path = PathBuilder::stroke(px(1.6));
                            path.move_to(start);
                            path.cubic_bezier_to(
                                end,
                                point(start.x, start.y + distance * 0.5),
                                point(end.x, end.y - distance * 0.5),
                            );
                            if let Ok(path) = path.build() {
                                window.paint_path(path, rgb(colors[edge.color % colors.len()]));
                            }
                        }
                        if row.incoming {
                            let mut path = PathBuilder::stroke(px(1.6));
                            path.move_to(point(x(row.lane), top));
                            path.line_to(point(x(row.lane), middle));
                            if let Ok(path) = path.build() {
                                window.paint_path(path, rgb(colors[row.color % colors.len()]));
                            }
                        }
                    }
                    // Search results are discontinuous history: isolated nodes do
                    // not imply parent relationships across hidden rows.
                    let center = point(x(if filtered { 0 } else { row.lane }), middle);
                    let color = rgb(colors[row.color % colors.len()]);
                    let radius = if active {
                        5.
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

    #[test]
    fn graph_lanes_remain_visible_on_all_theme_surfaces_without_changing_identity() {
        let luminance = |color: u32| {
            let linear = |channel: u32| {
                let value = f64::from(channel) / 255.;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear((color >> 16) & 255)
                + 0.7152 * linear((color >> 8) & 255)
                + 0.0722 * linear(color & 255)
        };
        for choice in crate::appearance::ThemeChoice::ALL {
            let palette = choice.palette();
            let colors = palette_colors(palette);
            assert_eq!(colors.len(), DARK_COLORS.len());
            for color in colors {
                for background in [palette.canvas, palette.hover, palette.selected] {
                    let foreground = luminance(color);
                    let background = luminance(background);
                    let contrast =
                        (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05);
                    assert!(contrast >= 3., "{choice:?}: {color:#x} contrast {contrast}");
                }
            }
        }
    }

    fn commit(id: &str, parents: &[&str]) -> Commit {
        Commit {
            oid: id.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            author: String::new(),
            timestamp: 0,
            subject: String::new(),
            body: String::new(),
        }
    }

    /// Trace every rendered parent edge until it reaches its node. This tests
    /// actual relationships through lane shifts, not only edge/node counts.
    fn assert_complete_topology(commits: &[Commit]) {
        let rows = layout(commits);
        for (index, row) in rows.iter().enumerate() {
            let mut reached = HashSet::new();
            for edge in row.edges.iter().filter(|edge| edge.from_node) {
                let mut lane = edge.to;
                let mut destination = None;
                for (next_index, next_row) in rows.iter().enumerate().skip(index + 1) {
                    if next_row.incoming && next_row.lane == lane {
                        assert_eq!(next_row.color, edge.color, "Incoming node color changed");
                        destination = Some(commits[next_index].oid.as_str());
                        break;
                    }
                    let continuations: Vec<_> = next_row
                        .edges
                        .iter()
                        .filter(|next| !next.from_node && next.from == lane)
                        .collect();
                    assert_eq!(
                        continuations.len(),
                        1,
                        "Ancestry lane disappeared or forked"
                    );
                    let continuation = continuations[0];
                    assert_eq!(continuation.color, edge.color, "Boundary color changed");
                    lane = continuation.to;
                }
                assert!(reached.insert(destination.expect("Parent edge never reached a node")));
            }
            let expected: HashSet<_> = commits[index].parents.iter().map(String::as_str).collect();
            assert_eq!(reached, expected, "Rendered parent relationships changed");
        }

        // Every lane arriving at a boundary meets this row's node or continues
        // below it. No orphaned line may enter from above.
        for adjacent in rows.windows(2) {
            let (previous, current) = (&adjacent[0], &adjacent[1]);
            let mut incoming = HashMap::new();
            for edge in &previous.edges {
                if let Some(color) = incoming.insert(edge.to, edge.color) {
                    assert_eq!(color, edge.color, "Joined ancestry has conflicting colors");
                }
            }
            let mut consumed = HashMap::new();
            for edge in current.edges.iter().filter(|edge| !edge.from_node) {
                assert!(consumed.insert(edge.from, edge.color).is_none());
            }
            if current.incoming {
                assert!(consumed.insert(current.lane, current.color).is_none());
            }
            assert_eq!(incoming, consumed, "Adjacent graph boundaries do not match");
        }
    }

    #[test]
    fn octopus_merge_and_shared_ancestry_have_exact_parent_paths() {
        assert_complete_topology(&[
            commit("merge", &["a", "b", "c", "d"]),
            commit("a", &["root"]),
            commit("b", &["root"]),
            commit("c", &["root"]),
            commit("d", &["root"]),
            commit("root", &[]),
        ]);
    }

    #[test]
    fn existing_first_parent_and_new_secondary_parent_preserve_flow() {
        assert_complete_topology(&[
            commit("head-a", &["left", "join"]),
            commit("head-b", &["right"]),
            commit("left", &["join", "extra"]),
            commit("right", &["extra", "join"]),
            commit("extra", &["join"]),
            commit("join", &["root"]),
            commit("unrelated", &[]),
            commit("root", &[]),
        ]);
    }

    #[test]
    fn duplicate_parent_entries_do_not_duplicate_rendered_edges() {
        assert_complete_topology(&[commit("a", &["b", "b", "b"]), commit("b", &[])]);
    }

    #[test]
    fn first_parent_color_is_stable_and_new_heads_have_no_incoming_edge() {
        let rows = layout(&[commit("a", &["b"]), commit("b", &["c"]), commit("c", &[])]);
        assert!(
            rows.iter()
                .all(|row| row.color == rows[0].color && row.lane == 0)
        );
        assert!(!rows[0].incoming);
        assert!(rows[1].incoming);
        assert!(rows[2].incoming);
        assert!(rows[2].edges.is_empty());
    }

    #[test]
    fn appending_history_preserves_loaded_rows_and_open_parent_edges() {
        let commits = [
            commit("merge", &["a", "b"]),
            commit("a", &["root"]),
            commit("b", &["root"]),
            commit("root", &[]),
        ];
        let prefix = layout(&commits[..2]);
        let complete = layout(&commits);
        assert!(!prefix[1].edges.is_empty());
        assert_eq!(prefix, complete[..prefix.len()]);
    }

    #[test]
    fn wide_graph_lanes_remain_ordered_and_inside_column() {
        for count in [1, 2, 3, 32, 128, 1024] {
            let positions: Vec<_> = (0..count).map(|lane| lane_x(84., count, lane)).collect();
            assert!(
                positions
                    .iter()
                    .all(|&x| (NODE_MARGIN..=84. - NODE_MARGIN).contains(&x))
            );
            assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        }
        assert_eq!(lane_x(84., 3, 0), 10.);
        assert_eq!(lane_x(84., 3, 1), 22.);
        assert_eq!(lane_x(84., 3, 2), 34.);
    }
}
