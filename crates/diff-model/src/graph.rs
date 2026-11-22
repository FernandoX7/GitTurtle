use gitturtle_core::Commit;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub const NODE_MARGIN: f32 = 10.;
/// Lane stroke thickness in logical pixels, shared by straight vertical quads
/// and curved merge/branch paths so joints stay flush across row boundaries.
pub const EDGE_WIDTH: f32 = 1.6;
/// The selected node's radius, the widest a node is drawn.
pub const SELECTED_RADIUS: f32 = 5.;
/// The narrowest compact lane pitch. Nodes keep their radii at every pitch,
/// so this leaves a selected node (5 px) 2.2 px clear of a neighbouring
/// lane's 1.6 px stroke.
pub const COMPACT_PITCH_MIN: f32 = 8.;
pub fn required_width(lane_count: usize, spacing: f32) -> f32 {
    NODE_MARGIN * 2. + lane_count.saturating_sub(1).min(128) as f32 * spacing
}

/// Narrow History's lane pitch for a chosen `spacing`: half of it, so the
/// graph keeps its shape in about half the width, and never so tight that a
/// node touches the next lane's edge.
pub fn compact_spacing(spacing: f32) -> f32 {
    (spacing * 0.5).max(COMPACT_PITCH_MIN)
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

/// How far past its node a cell must reach to hold a lane whole: half a
/// pitch, where the lane's strokes end, short of the next lane, yet never
/// less than the selected node's radius, so the node is whole at the
/// narrowest pitch, nor more than the margin `required_width` leaves after
/// its last lane, so a graph of that width holds every one of its lanes.
fn lane_reach(lane_spacing: f32) -> f32 {
    (lane_spacing / 2.).clamp(SELECTED_RADIUS, NODE_MARGIN)
}

/// A shared horizontal lane viewport keeps true spacing and continuous edges
/// across every visible row. Clipping is explicit; lanes are never compressed
/// into indistinguishable pixels to accommodate offscreen ancestry. A lane is
/// held, and drawn, where its node and `lane_reach` past it fit in `width`,
/// rounding aside, and not at all otherwise.
pub fn visible_lane_capacity(width: f32, lane_spacing: f32) -> usize {
    (((width - NODE_MARGIN - lane_reach(lane_spacing)).max(0.) / lane_spacing.max(1.) + 1e-3)
        .floor() as usize
        + 1)
    .max(1)
}

/// Narrow History's graph within `room`: every one of its `lanes` that
/// `room` holds whole (`visible_lane_capacity`), and no wider than those
/// lanes take at their full margin, so the room short of the next lane is
/// the subject's. Never less than one lane.
pub fn fitted_width(lanes: usize, room: f32, lane_spacing: f32) -> f32 {
    let held = visible_lane_capacity(room, lane_spacing).min(lanes.max(1));
    room.min(required_width(held, lane_spacing))
        .max(required_width(1, lane_spacing))
}

pub fn lane_x(width: f32, lane_offset: usize, lane: usize, lane_spacing: f32) -> f32 {
    NODE_MARGIN.min(width / 2.) + (lane as f32 - lane_offset as f32) * lane_spacing
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

    /// Narrow History's pitch is half the chosen spacing, scaled or not, and
    /// a selected node with its ring never reaches the next lane's stroke.
    #[test]
    fn compact_pitch_halves_the_spacing_and_keeps_nodes_clear() {
        assert_eq!(compact_spacing(20.), 10.);
        assert_eq!(compact_spacing(32. * 18. / 13.), 16. * 18. / 13.);
        for spacing in [12., 14., 20., 32., 44.3] {
            let pitch = compact_spacing(spacing);
            assert!(pitch <= spacing * 0.5 || pitch == COMPACT_PITCH_MIN);
            assert!(5. + 0.8 < pitch, "{spacing}: {pitch}");
            assert!(required_width(8, pitch) < required_width(8, spacing));
        }
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
    fn a_lane_is_held_where_its_node_and_half_a_pitch_fit() {
        // Narrow History's compact pitch at 13 and 18 pt.
        for pitch in [compact_spacing(20.), compact_spacing(20. * 18. / 13.)] {
            for lanes in 1..=4 {
                let node = NODE_MARGIN + (lanes - 1) as f32 * pitch;
                assert_eq!(visible_lane_capacity(node + pitch / 2., pitch), lanes);
                assert_eq!(
                    visible_lane_capacity(node + pitch / 2. - 0.1, pitch),
                    (lanes - 1).max(1),
                    "{pitch}: {lanes}"
                );
            }
        }
        // At the narrowest pitch the selected node, not half a pitch, needs
        // the room; at a wide one, the margin after the last lane suffices.
        assert_eq!(visible_lane_capacity(NODE_MARGIN + 8. + 4.9, 8.), 1);
        assert_eq!(visible_lane_capacity(NODE_MARGIN + 8. + 5., 8.), 2);
        for spacing in [8., 10., 20., 27.7, 44.3] {
            for lanes in 1..=4 {
                assert_eq!(
                    visible_lane_capacity(required_width(lanes, spacing), spacing),
                    lanes,
                    "{spacing}: {lanes}"
                );
            }
        }
        // Narrow History's graph keeps each lane the room holds whole and
        // leaves the room short of the next to the subject: at 29 px the
        // second lane's node and half a pitch (25 px) fit and the third's
        // (35 px) do not.
        assert_eq!(fitted_width(3, 24., 10.), 20.);
        assert_eq!(fitted_width(3, 29., 10.), 29.);
        assert_eq!(fitted_width(3, 33., 10.), 30.);
        assert_eq!(fitted_width(3, 36., 10.), 36.);
        assert_eq!(fitted_width(3, 60., 10.), 40.);
        assert_eq!(fitted_width(3, 12., 10.), 20.);
        assert_eq!(fitted_width(0, 60., 10.), 20.);
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
