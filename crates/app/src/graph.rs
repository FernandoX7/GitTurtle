use crate::appearance::{Palette, palette};
use gitturtle_core::Commit;
use gpui_kit::{
    AnyElement, App, Bounds, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point,
    px, quad, rgb, size,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const DARK_COLORS: [u32; 6] = [0x7adfb4, 0x8db7f6, 0xc3a5f2, 0xe8be7a, 0xea9ca5, 0x72ccd8];
const LIGHT_COLORS: [u32; 6] = [0x146c53, 0x315da7, 0x7651aa, 0x8c5916, 0xa42d63, 0x156b7c];
const NODE_MARGIN: f32 = 10.;
pub(super) fn required_width(lane_count: usize, spacing: f32) -> f32 {
    NODE_MARGIN * 2. + lane_count.saturating_sub(1).min(128) as f32 * spacing
}

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
    /// Immutable geometry is shared with visible row paint callbacks. Cloning
    /// a row during scrolling must not allocate or copy its ancestry edges.
    pub edges: Arc<[Edge]>,
    /// Required lanes including both boundary frontiers and the node itself.
    pub width: usize,
}

#[derive(Clone, Copy)]
struct Lane<'a> {
    oid: &'a str,
    color: usize,
}

/// Bounded frontier and branch-color continuity between history pages. No
/// previously rendered commits or geometry are retained by this cursor.
#[derive(Clone, Debug, Default)]
pub struct GraphCursor {
    frontier: Vec<(String, usize)>,
    next_color: usize,
    hidden: bool,
}

impl GraphCursor {
    /// Prepare only the new page, committing frontier state after successful
    /// cancellation checks. A budget fallback stays node-only until reset;
    /// callers should also replace retained rows when `hidden` becomes true.
    pub fn append<E>(
        &mut self,
        commits: &[Commit],
        lane_limit: usize,
        edge_limit: usize,
        mut checkpoint: impl FnMut() -> Result<(), E>,
    ) -> Result<(Vec<GraphRow>, bool), E> {
        let mut hidden = self.hidden;
        if !hidden {
            let mut frontier: HashSet<&str> =
                self.frontier.iter().map(|(oid, _)| oid.as_str()).collect();
            let mut parents = HashSet::new();
            let mut edges = 0usize;
            let mut parent_entries = 0usize;
            for commit in commits {
                checkpoint()?;
                let width = frontier.len() + usize::from(!frontier.contains(commit.oid.as_str()));
                if width > lane_limit {
                    hidden = true;
                    break;
                }
                frontier.remove(commit.oid.as_str());
                parents.clear();
                for parent in &commit.parents {
                    parent_entries += 1;
                    parents.insert(parent.as_str());
                    frontier.insert(parent.as_str());
                    if frontier.len() > lane_limit || parent_entries > edge_limit {
                        hidden = true;
                        break;
                    }
                }
                edges = edges.saturating_add(width.saturating_sub(1) + parents.len());
                if hidden || edges > edge_limit {
                    hidden = true;
                    break;
                }
            }
        }
        if hidden {
            let rows = commits
                .iter()
                .map(|_| {
                    checkpoint()?;
                    Ok(GraphRow {
                        width: 1,
                        ..Default::default()
                    })
                })
                .collect::<Result<Vec<_>, E>>()?;
            self.frontier.clear();
            self.hidden = true;
            return Ok((rows, true));
        }
        let (rows, frontier, next_color) =
            layout_page(commits, &self.frontier, self.next_color, checkpoint)?;
        self.frontier = frontier;
        self.next_color = next_color;
        Ok((rows, false))
    }
}

/// Lay out a topologically ordered, newest-first history. Paged callers use
/// GraphCursor to preserve exactly these lanes/colors without prefix replay.
pub fn layout<E>(
    commits: &[Commit],
    checkpoint: impl FnMut() -> Result<(), E>,
) -> Result<Vec<GraphRow>, E> {
    layout_page(commits, &[], 0, checkpoint).map(|(rows, _, _)| rows)
}

type PageLayout = (Vec<GraphRow>, Vec<(String, usize)>, usize);

fn layout_page<E>(
    commits: &[Commit],
    frontier: &[(String, usize)],
    mut next_color: usize,
    mut checkpoint: impl FnMut() -> Result<(), E>,
) -> Result<PageLayout, E> {
    let mut lanes: Vec<Lane<'_>> = frontier
        .iter()
        .map(|(oid, color)| Lane { oid, color: *color })
        .collect();
    let mut before = Vec::new();
    let mut seen = HashSet::new();
    let mut parents = Vec::new();
    let mut positions = HashMap::new();
    let mut rows = Vec::with_capacity(commits.len());
    let empty_edges: Arc<[Edge]> = Arc::default();
    for commit in commits {
        checkpoint()?;
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
        // Retain bounded scratch allocations across rows. Rows without edges
        // share an empty buffer; other final geometry is allocated here once.
        before.clear();
        before.extend_from_slice(&lanes);
        let color = lanes[lane].color;
        lanes.remove(lane);

        seen.clear();
        parents.clear();
        parents.extend(
            commit
                .parents
                .iter()
                .map(String::as_str)
                .filter(|parent| seen.insert(*parent)),
        );
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

        positions.clear();
        positions.extend(
            lanes
                .iter()
                .enumerate()
                .map(|(index, lane)| (lane.oid, index)),
        );
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
        for &parent in &parents {
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
            edges: if edges.is_empty() {
                Arc::clone(&empty_edges)
            } else {
                edges.into()
            },
            width: before.len().max(lanes.len()),
        });
    }
    let frontier = lanes
        .into_iter()
        .map(|lane| (lane.oid.to_owned(), lane.color))
        .collect();
    Ok((rows, frontier, next_color))
}

/// A shared horizontal lane viewport keeps true spacing and continuous edges
/// across every visible row. Clipping is explicit; lanes are never compressed
/// into indistinguishable pixels to accommodate offscreen ancestry.
pub fn visible_lane_capacity(width: f32, lane_spacing: f32) -> usize {
    (((width - NODE_MARGIN * 2.).max(0.) / lane_spacing.max(1.)).floor() as usize + 1).max(1)
}

fn lane_x(width: f32, lane_offset: usize, lane: usize, lane_spacing: f32) -> f32 {
    NODE_MARGIN.min(width / 2.) + (lane as f32 - lane_offset as f32) * lane_spacing
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
                    if !filtered {
                        // Curves meet adjacent rows at exact lane coordinates with
                        // vertical tangents, including collapsing lanes.
                        for edge in row.edges.iter() {
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

    fn layout(commits: &[Commit]) -> Vec<GraphRow> {
        super::layout(commits, || Ok::<_, ()>(())).unwrap()
    }

    #[test]
    fn superseded_layout_stops_between_rows() {
        let commits: Vec<_> = (0..10_000)
            .map(|index| {
                commit(
                    &format!("commit-{index}"),
                    &[&format!("commit-{}", index + 1)],
                )
            })
            .collect();
        let mut checkpoints = 0;
        let rows = super::layout(&commits, || {
            checkpoints += 1;
            if checkpoints == 7 {
                Err("superseded")
            } else {
                Ok(())
            }
        });
        assert_eq!(rows.unwrap_err(), "superseded");
        assert_eq!(checkpoints, 7);
        // A later request can still build the same topology from scratch.
        assert_eq!(layout(&commits).len(), commits.len());
    }

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
            for edge in previous.edges.iter() {
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
    fn lane_viewport_keeps_spacing_and_can_reach_offscreen_ancestry() {
        assert_eq!(visible_lane_capacity(84., 20.), 4);
        assert_eq!(lane_x(84., 0, 0, 20.), 10.);
        assert_eq!(lane_x(84., 0, 4, 20.), 90.);
        assert_eq!(lane_x(84., 4, 4, 20.), 10.);
        assert_eq!(lane_x(84., 4, 3, 20.), -10.);
        assert_eq!(lane_x(84., 4, 7, 20.), 70.);
    }

    #[test]
    fn readable_graph_width_preserves_spacing_when_history_grows() {
        for spacing in [12., 20., 32., 44.] {
            for lanes in [2, 8, 32, 128] {
                let width = required_width(lanes, spacing);
                let positions: Vec<_> = (0..lanes)
                    .map(|lane| lane_x(width, 0, lane, spacing))
                    .collect();
                assert!(
                    positions
                        .windows(2)
                        .all(|pair| pair[1] - pair[0] >= spacing - 0.01)
                );
                assert!(positions.last().unwrap() + 5. < width);
                assert_eq!(
                    positions[1],
                    lane_x(required_width(128, spacing), 0, 1, spacing)
                );
            }
        }
    }

    #[test]
    fn incremental_pages_match_complete_topology_and_colors_without_retaining_rows() {
        let commits = [
            commit("head", &["a", "b", "c"]),
            commit("a", &["shared"]),
            commit("b", &["shared", "extra"]),
            commit("c", &["extra"]),
            commit("extra", &["root"]),
            commit("shared", &["root"]),
            commit("root", &[]),
        ];
        let expected = layout(&commits);
        for page_size in 1..=commits.len() {
            let mut cursor = GraphCursor::default();
            let mut rows = Vec::new();
            for page in commits.chunks(page_size) {
                let (next, hidden) = cursor
                    .append(page, 128, 200_000, || Ok::<_, ()>(()))
                    .unwrap();
                assert!(!hidden);
                rows.extend(next);
            }
            assert_eq!(rows, expected);
            assert!(cursor.frontier.is_empty());
        }
    }

    #[test]
    fn incremental_cancellation_preserves_cursor_and_wide_pages_latch_honest_fallback() {
        let mut cursor = GraphCursor::default();
        cursor
            .append(&[commit("head", &["a", "b"])], 128, 200_000, || {
                Ok::<_, ()>(())
            })
            .unwrap();
        let saved = cursor.frontier.clone();
        let mut count = 0;
        let next = [commit("a", &["root"]), commit("b", &["root"])];
        assert!(
            cursor
                .append(&next, 128, 200_000, || {
                    count += 1;
                    if count == 4 { Err(()) } else { Ok(()) }
                })
                .is_err()
        );
        assert_eq!(cursor.frontier, saved);
        let (_, hidden) = cursor
            .append(&next, 1, 200_000, || Ok::<_, ()>(()))
            .unwrap();
        assert!(hidden);
        assert!(cursor.frontier.is_empty());
        let (rows, hidden) = cursor
            .append(&[commit("root", &[])], 128, 200_000, || Ok::<_, ()>(()))
            .unwrap();
        assert!(hidden);
        assert!(rows[0].edges.is_empty());
    }
}
