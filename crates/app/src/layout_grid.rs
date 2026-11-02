//! The layout grid: every page and dialog on the width walk's fixture, at 13
//! and 18 pt in a wide and a narrow window, fails where one panel starts its
//! text at more than one left edge (DESIGN.md, "One left edge").
//!
//! The walk's text mark paints a fill exactly behind every text run
//! (`width_walk`), so the grid reads where each run starts from the painted
//! quads. The kit's Input paints its text without it, so a field's text and
//! placeholder start where its bordered box puts them: the box's 1 px edge
//! and the kit's padding for the box's height in. A run belongs to the first
//! [`Panel`] of its surface whose area holds its start; the runs of one
//! panel group into lines by their height, and each line starts at its
//! leftmost run. A header, a row's label, a field's text and a notice each
//! start a line; a count or control after them on the same line starts
//! none, and neither does a line nearer the panel's right side than its left
//! (its actions, a count on its own) or one centred in it (an empty state). A
//! panel's edge is its leftmost line start: every other line starts there,
//! or at one of the panel's [`Offset`]s from it, each named with its reason.
//! A run its clip cuts on the left, scrolled sideways, starts no line, and
//! neither does one it cuts at its top or foot, at a list's or the body's
//! edge, where the other runs of its line may lie past the clip: at 18 pt
//! the foot of the conversation list left 5 px of a card's comment count
//! showing without the Reply and Resolve Buttons before it, which read as a
//! line 433 px into the GitHub panel.
//!
//! The bars of controls (the repository header, the Git actions, the
//! toolbars and the status strip) lay their groups side by side rather than
//! in lines, and Compare's code scrolls in its editor; neither is a panel.

use crate::focus_ring::sweep::open_dialog;
use crate::width_walk::{self, frame, text_mark};
use crate::*;
use core::prelude::v1::test;
use gpui_kit::ScaledPixels;
use std::collections::BTreeSet;

/// What the grid lays out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    /// History with a commit selected, its inspector beside it.
    History,
    /// Compare on the commit's text file.
    Compare,
    /// Working Changes, the composer below its lists.
    Working,
    /// The GitHub panel on its offline fixture.
    GitHub,
    /// The GitHub panel on its offline fixture's Overview with its bordered
    /// panes: the connection pane open and a reply open and reviewed, its
    /// confirmation showing, read at the body's top and its end.
    GitHubPanes,
    ProjectHub,
    /// History beside the project pane.
    ProjectPane,
    Reflog,
    Rebase,
    CommandPalette,
    QuickOpen,
    /// Settings, its four sections each a bordered card, read a scroll
    /// view at a time from the page's top to its end.
    Settings,
}

/// Where a panel's text lies.
#[derive(Clone, Copy, Debug)]
enum Area {
    /// The bounds of the element with this debug selector.
    Selector(&'static str),
    /// The bounds of every element with one of these selectors together.
    Union(&'static [&'static str]),
    /// A table column: the x range of the column titled by the element with
    /// the first selector, over the height of the second, within the x range
    /// of the third, the part of the table its viewport shows: a narrow
    /// window's table runs on under the inspector, whose lines are not the
    /// columns' there.
    Column(&'static str, &'static str, &'static str),
    /// The open dialog: the first fill painted above its overlay.
    Dialog,
    /// The part of the element with the first selector that the element
    /// with the second, a scrolling body, shows: a card the body scrolls
    /// past its foot runs on under what lies below the body.
    Within(&'static str, &'static str),
}

/// A panel whose text starts at one left edge.
struct Panel {
    name: &'static str,
    area: Area,
    /// How far the panel's text starts in from where its boxes stand, for a
    /// panel whose text starts at a compact Button's label inset, where the
    /// Buttons at its edge, transparent at rest, start their labels: a field
    /// there stands its box with their frames and keeps its own padding
    /// (DESIGN.md, "One left edge"), so the grid measures it by its box, as
    /// if its text started this far in from it.
    boxes: Option<fn() -> Pixels>,
    /// Where the panel's edge stands in from its area's left, for a panel
    /// every line of which leads with an icon or tile at that edge, so no
    /// text starts there: its lines start at one of its offsets from it.
    /// Without it, a panel's edge is its leftmost line start.
    edge: Option<fn() -> Pixels>,
}

const fn panel(name: &'static str, area: Area) -> Panel {
    Panel {
        name,
        area,
        boxes: None,
        edge: None,
    }
}

/// A [`panel`] whose text starts `inset` in from where its boxes stand.
const fn boxed_panel(name: &'static str, area: Area, inset: fn() -> Pixels) -> Panel {
    Panel {
        name,
        area,
        boxes: Some(inset),
        edge: None,
    }
}

/// A [`panel`] whose edge stands `edge` in from its area's left, where its
/// lines' leading icons and tiles stand.
const fn icon_panel(name: &'static str, area: Area, edge: fn() -> Pixels) -> Panel {
    Panel {
        name,
        area,
        boxes: None,
        edge: Some(edge),
    }
}

/// The panels of each surface, the most specific first: a run belongs to the
/// first whose area holds its start, so a nested group lists before the
/// panel around it.
fn panels(surface: Surface) -> &'static [Panel] {
    use Area::*;
    const HISTORY: &[Panel] = &[
        panel("navigation", Selector("history-sidebar")),
        panel(
            "References column",
            Column("history-column-0", "history-pane", "history-list-edge"),
        ),
        panel(
            "Graph column",
            Column("history-column-1", "history-pane", "history-list-edge"),
        ),
        panel(
            "Subject column",
            Column("history-column-2", "history-pane", "history-list-edge"),
        ),
        panel(
            "Author column",
            Column("history-column-3", "history-pane", "history-list-edge"),
        ),
        panel(
            "Date column",
            Column("history-column-4", "history-pane", "history-list-edge"),
        ),
        panel(
            "SHA column",
            Column("history-column-5", "history-pane", "history-list-edge"),
        ),
        panel("inspector", Selector("commit-inspector")),
    ];
    const COMPARE: &[Panel] = &[panel("inspector", Selector("commit-inspector"))];
    const WORKING: &[Panel] = &[
        icon_panel(
            "working files",
            Union(&["working-header", "working-pane"]),
            || appearance::ui_size(appearance::PANE_INSET) + px(2.),
        ),
        panel("composer", Selector("commit-composer")),
    ];
    const GITHUB: &[Panel] = &[boxed_panel(
        "GitHub panel",
        Selector("github-panel"),
        || appearance::ui_size(crate::LABEL_INSET),
    )];
    // The conversations are cards inside the Conversations pane, each
    // bordered and padded inside the pane's text edge: a nested group.
    const GITHUB_PANES: &[Panel] = &[
        panel("conversation cards", Selector("github-threads")),
        boxed_panel("GitHub panel", Selector("github-panel"), || {
            appearance::ui_size(crate::LABEL_INSET)
        }),
    ];
    const PROJECT_HUB: &[Panel] = &[
        // A bordered box inside the recent list, its rows a group of their own.
        panel("recent project rows", Selector("project-hub-recent-list")),
        panel("recent projects", Selector("project-hub-recent")),
        panel("project action", Selector("project-hub-action")),
    ];
    const PROJECT_PANE: &[Panel] = &[panel("project pane", Selector("project-pane"))];
    const REFLOG: &[Panel] = &[panel("Reflog", Dialog)];
    const REBASE: &[Panel] = &[panel("interactive rebase", Dialog)];
    const PALETTE: &[Panel] = &[panel("command palette", Dialog)];
    const QUICK_OPEN: &[Panel] = &[panel("Quick Open", Dialog)];
    // Each section is a bordered card; Your themes, a bordered card inside
    // Appearance, and the repository's box at the head of Git identity, a
    // filled one, are nested groups.
    const SETTINGS: &[Panel] = &[
        panel(
            "Your themes",
            Within("custom-themes-card", "settings-scroll"),
        ),
        panel(
            "repository identity",
            Within("settings-repository-identity", "settings-scroll"),
        ),
        panel(
            "Appearance",
            Within("settings-section: Appearance", "settings-scroll"),
        ),
        panel(
            "History columns",
            Within("settings-section: History columns", "settings-scroll"),
        ),
        panel(
            "Projects",
            Within("settings-section: Projects", "settings-scroll"),
        ),
        panel(
            "Git identity",
            Within("settings-section: Git identity", "settings-scroll"),
        ),
    ];
    match surface {
        Surface::History => HISTORY,
        Surface::Compare => COMPARE,
        Surface::Working => WORKING,
        Surface::GitHub => GITHUB,
        Surface::GitHubPanes => GITHUB_PANES,
        Surface::ProjectHub => PROJECT_HUB,
        Surface::ProjectPane => PROJECT_PANE,
        Surface::Reflog => REFLOG,
        Surface::Rebase => REBASE,
        Surface::CommandPalette => PALETTE,
        Surface::QuickOpen => QUICK_OPEN,
        Surface::Settings => SETTINGS,
    }
}

/// A deliberate offset from a panel's edge: a line of the panel may start
/// `by` past it, at the interface size, for `reason`. An offset for no
/// panel in particular holds in every panel; one for some holds only in the
/// panels it names, each with its surface, which are the panels its reason
/// lists.
struct Offset {
    panel: Option<&'static [(Surface, &'static str)]>,
    by: fn() -> Pixels,
    #[allow(dead_code, reason = "read by people, as a follow-up is")]
    reason: &'static str,
}

/// The inspector, on each surface that draws it.
const INSPECTOR: &[(Surface, &str)] = &[
    (Surface::History, "inspector"),
    (Surface::Compare, "inspector"),
];

/// Settings' section cards.
const SETTINGS_SECTIONS: &[(Surface, &str)] = &[
    (Surface::Settings, "Appearance"),
    (Surface::Settings, "History columns"),
    (Surface::Settings, "Projects"),
    (Surface::Settings, "Git identity"),
];

const OFFSETS: &[Offset] = &[
    Offset {
        panel: None,
        by: || appearance::ui_size(crate::LABEL_INSET),
        reason: "a compact Button at the edge starts its label its label inset in; within the \
                 grid's half pixel this also holds Settings' 0.75 rem insets, 9.75 px at the \
                 default size: a text size's sample, its text after its canvas box's padding, \
                 and List density's segmented control, stacked below its description in a \
                 narrow card, its first label after the control's 0.25 rem padding and the \
                 small Button's 0.5 rem",
    },
    Offset {
        panel: None,
        by: || appearance::FIELD_TEXT_INSET,
        reason: "a field at the edge starts its text and placeholder past its border and padding",
    },
    Offset {
        panel: Some(&[
            (Surface::History, "inspector"),
            (Surface::Compare, "inspector"),
            (Surface::GitHubPanes, "GitHub panel"),
            (Surface::Settings, "Git identity"),
        ]),
        by: || appearance::ui_size(crate::LABEL_INSET + 16. + 6.),
        reason: "a compact Button standing at the edge whose 16 px icon leads its label starts \
                 its label past its label inset, the icon and the 6 px gap: the inspector's \
                 Copy message, and in a bordered pane of the GitHub panel, whose Buttons stand \
                 their frames at its text edge, Refresh conversations on a line of its own; \
                 Git identity's Reset edits and Manage named profiles…",
    },
    Offset {
        panel: Some(&[
            (Surface::History, "navigation"),
            (Surface::History, "inspector"),
            (Surface::Compare, "inspector"),
            (Surface::Working, "working files"),
            (Surface::ProjectPane, "project pane"),
        ]),
        by: || px(15.) + appearance::ui_size(6.5),
        reason: "a row's or header's 15 px icon stands at the edge, its text after the 0.5 \
                 rem gap: History's branch and worktree rows, the inspector's Changed files \
                 header, Working's Staged and Unstaged headings and the project pane's \
                 projects; and the Changes header over Working's list, which shares the \
                 filter's and the composer's inset, the list's 1 px margin and border before \
                 the list's column, where the headings' icons and the file tiles stand inside \
                 their rings' room, starts its label after its 17 px commit icon and the gap, \
                 as far past the column",
    },
    Offset {
        panel: Some(&[
            (Surface::History, "inspector"),
            (Surface::Compare, "inspector"),
            (Surface::Working, "working files"),
        ]),
        by: || px(26.) + appearance::ui_size(6.5),
        reason: "a changed file's 26 px status tile stands at the edge, its name and folder \
                 after the 0.5 rem gap, in the inspector and in Working Changes",
    },
    Offset {
        panel: Some(&[(Surface::History, "References column")]),
        by: || appearance::ui_size(3.25),
        reason: "a reference's pill stands at the column's edge and pads its name 0.25 rem in",
    },
    Offset {
        panel: Some(INSPECTOR),
        by: || px(15.) + appearance::ui_size(6.5 + 4.875),
        reason: "where the Changed files label has left a narrow inspector, the count stands \
                 in its place after the header's 15 px icon and pads its figure 0.375 rem in",
    },
    Offset {
        panel: Some(&[(Surface::Working, "composer")]),
        by: || px(13.) + appearance::ui_size(4.875),
        reason: "the branch line's 13 px branch glyph stands at the edge, its name after \
                 the 0.375 rem gap",
    },
    Offset {
        panel: Some(&[(Surface::ProjectHub, "project action")]),
        by: || px(42.) + appearance::ui_size(9.75),
        reason: "the action's 42 px tile stands at the card's edge, its heading after the \
                 0.75 rem gap",
    },
    Offset {
        panel: Some(&[(Surface::ProjectHub, "project action")]),
        by: || px(16.) + appearance::ui_size(9.75),
        reason: "a feature's 16 px icon stands at the card's edge, its title and detail \
                 after the 0.75 rem gap",
    },
    Offset {
        panel: Some(&[
            (Surface::GitHub, "GitHub panel"),
            (Surface::GitHubPanes, "GitHub panel"),
        ]),
        by: || appearance::ui_size(16. + 6.),
        reason: "a Button whose icon leads its label, as Pull requests, Refresh PRs and New \
                 pull request do, stands its 16 px icon at the text's edge, where its frame's \
                 label inset puts it, and starts its label after the icon's 6 px gap",
    },
    Offset {
        panel: Some(SETTINGS_SECTIONS),
        by: || px(32.) + appearance::ui_size(9.75),
        reason: "a section's 32 px icon tile stands at its card's edge, its title and \
                 description after the 0.75 rem gap",
    },
    Offset {
        panel: Some(&[(Surface::Settings, "Appearance")]),
        by: || px(4.) + appearance::ui_size(9.75),
        reason: "a theme card stands at the section's edge, its caption's name and description \
                 inside the card's 1 px border, its 2 px padding and its preview box's 1 px \
                 border, after the caption's 0.75 rem padding",
    },
    Offset {
        panel: Some(&[(Surface::Settings, "Appearance")]),
        by: || appearance::ui_size(6.5),
        reason: "a text size's controls, wrapped below its description, stand their small ghost \
                 − at the edge, its label after the small Button's 0.5 rem padding",
    },
    Offset {
        panel: Some(&[(Surface::Settings, "Git identity")]),
        by: || px(14.) + appearance::ui_size(8.125 + 6.5),
        reason: "Save repository identity, the kit's medium primary Button, stands at the edge, \
                 its label after its 0.625 rem padding, its 14 px check icon and the 0.5 rem gap",
    },
    Offset {
        panel: Some(&[(Surface::Settings, "repository identity")]),
        by: || px(14.) + appearance::ui_size(6.5),
        reason: "the repository's 14 px folder icon stands at its box's edge, its name after the \
                 0.5 rem gap",
    },
];

/// A panel whose text the grid finds at more than one edge until the named
/// follow-up fixes it. The grid fails on a finding no entry names, and on an
/// entry nothing matches, so a fix removes its entry.
struct Known {
    surface: Surface,
    panel: &'static str,
    sizes: &'static [u8],
    /// The follow-up that owns it.
    follow_up: &'static str,
}

const KNOWN: &[Known] = &[
    // The conversation cards inside the Conversations pane, bordered and
    // padded 0.75 rem, start their author line at their padding, a line 10.5
    // px further and their Buttons' labels 20.5 px in, at 13 pt.
    Known {
        surface: Surface::GitHubPanes,
        panel: "conversation cards",
        sizes: &[13],
        follow_up: "the conversation cards start their lines and Buttons' labels at one edge",
    },
];

/// The widths the grid lays each surface out at: a 1,480 px window and a
/// 461 px half tile, both 980 px tall.
const WIDTHS: [f32; 2] = [1480., 461.];
const HEIGHT: f32 = 980.;

/// What the last frame drew: its text runs and fields' text, each where it
/// starts and as high as its line, and the open dialog's bounds.
struct Drawn {
    runs: Vec<Run>,
    dialog: Option<Bounds<Pixels>>,
}

/// A text run, or a field's text, where it starts and as high as its line.
#[derive(Clone, Copy)]
struct Run {
    bounds: Bounds<Pixels>,
    /// For a field's text, where the field's box starts.
    field: Option<Pixels>,
}

/// Reads the last frame: the text runs and fields above an open dialog's
/// overlay that their clips leave whole on the left.
fn drawn(cx: &mut VisualTestContext) -> Drawn {
    let mark = gpui_kit::Background::from(text_mark());
    let (scale, rem, viewport, dialog, field, focused, mut quads) = cx.update(|window, cx| {
        let colors = gpui_kit::component::Theme::global(cx).colors;
        (
            window.scale_factor(),
            window.rem_size(),
            window.viewport_size(),
            window.has_active_dialog(cx),
            colors.input,
            colors.ring,
            window.painted_quads(),
        )
    });
    quads.sort_by_key(|quad| quad.order);
    let logical = |value: ScaledPixels| px(value.0 / scale);
    let bounds = |bounds: Bounds<ScaledPixels>| {
        Bounds::new(
            point(logical(bounds.origin.x), logical(bounds.origin.y)),
            size(logical(bounds.size.width), logical(bounds.size.height)),
        )
    };
    // A dialog draws above the last fill that spans the window, its overlay;
    // what it covers is laid out on its own surface.
    let overlay = dialog
        .then(|| {
            quads
                .iter()
                .filter(|quad| {
                    logical(quad.bounds.size.width) >= viewport.width - px(1.)
                        && logical(quad.bounds.size.height) >= viewport.height - px(1.)
                })
                .map(|quad| quad.order)
                .max()
        })
        .flatten();
    quads.retain(|quad| overlay.is_none_or(|overlay| quad.order > overlay));
    let dialog = overlay.and_then(|_| {
        quads
            .iter()
            .find(|quad| quad.background != mark)
            .map(|quad| bounds(quad.bounds))
    });
    // The kit's Input pads its text by its size, which its height tells
    // apart: 12, 10, 8 and 4 px inside 2.75, 2, 1.5 and 1.25 rem.
    let padding = |height: Pixels| {
        [(2.75, 12.), (2., 10.), (1.5, 8.), (1.25, 4.)]
            .into_iter()
            .find(|(rems, _)| (height - rem * *rems).abs() <= px(0.5))
            .map(|(_, padding)| px(padding))
    };
    let edge = ScaledPixels(scale);
    let slack = px(0.5);
    // The palette's colours reach the kit's tokens through a conversion.
    let near = |a: Hsla, b: Hsla| {
        [(a.h, b.h), (a.s, b.s), (a.l, b.l), (a.a, b.a)]
            .iter()
            .all(|(a, b)| (a - b).abs() < 0.01)
    };
    let runs = quads
        .iter()
        .filter_map(|quad| {
            let (drawn, clip) = (bounds(quad.bounds), bounds(quad.content_mask.bounds));
            let (run, field_box) = if quad.background == mark {
                (drawn, None)
            } else if (near(quad.border_color, field) || near(quad.border_color, focused))
                && [
                    quad.border_widths.top,
                    quad.border_widths.right,
                    quad.border_widths.bottom,
                    quad.border_widths.left,
                ]
                .iter()
                .all(|width| (width.0 - edge.0).abs() < 0.01)
            {
                // Where its text starts, a pixel wide: a field's box is
                // centred in its panel, its text not.
                let inset = px(1.) + padding(drawn.size.height)?;
                (
                    Bounds::new(
                        point(drawn.left() + inset, drawn.top()),
                        size(px(1.), drawn.size.height),
                    ),
                    Some(drawn.left()),
                )
            } else {
                return None;
            };
            // A field is read by its box, which its clip may cut by its ring's
            // room and still show its text whole.
            let whole = if field_box.is_some() {
                run.top() < clip.bottom() && run.bottom() > clip.top()
            } else {
                run.top() >= clip.top() - slack && run.bottom() <= clip.bottom() + slack
            };
            (run.left() >= clip.left() - slack && run.left() < clip.right() && whole).then_some(
                Run {
                    bounds: run,
                    field: field_box,
                },
            )
        })
        .collect();
    Drawn { runs, dialog }
}

/// The bounds of `area` in the last frame.
fn area_bounds(
    area: Area,
    dialog: Option<Bounds<Pixels>>,
    cx: &mut VisualTestContext,
) -> Option<Bounds<Pixels>> {
    match area {
        Area::Selector(selector) => cx.debug_bounds(selector),
        Area::Union(selectors) => selectors
            .iter()
            .filter_map(|selector| cx.debug_bounds(selector))
            .reduce(|a, b| a.union(&b)),
        Area::Column(title, within, viewport) => {
            let (title, within, viewport) = (
                cx.debug_bounds(title)?,
                cx.debug_bounds(within)?,
                cx.debug_bounds(viewport)?,
            );
            let column = Bounds::from_corners(
                point(title.left(), within.top().min(title.top())),
                point(title.right(), within.bottom()),
            );
            // A column past the table's visible edge shows nothing.
            let shown = column.intersect(&Bounds::from_corners(
                point(within.left().max(viewport.left()), column.top()),
                point(within.right().min(viewport.right()), column.bottom()),
            ));
            (shown.size.width > px(0.)).then_some(shown)
        }
        Area::Dialog => dialog,
        Area::Within(selector, body) => {
            let shown = cx
                .debug_bounds(selector)?
                .intersect(&cx.debug_bounds(body)?);
            (shown.size.width > px(0.) && shown.size.height > px(0.)).then_some(shown)
        }
    }
}

/// Where the lines of `runs` start in `area`, top to bottom: runs whose
/// middles lie within one another's height share a line, which starts at
/// its leftmost run. A line nearer the area's right side than its left,
/// right-aligned, or one run centred in it starts none.
fn line_starts(mut runs: Vec<Bounds<Pixels>>, area: Bounds<Pixels>) -> Vec<(Pixels, Pixels)> {
    runs.sort_by(|a, b| f32::from(a.top()).total_cmp(&f32::from(b.top())));
    // Each line by its first run's height, and all its runs together.
    let mut lines: Vec<(Bounds<Pixels>, Bounds<Pixels>, usize)> = Vec::new();
    for run in runs {
        let middle = run.center().y;
        match lines
            .iter_mut()
            .find(|(first, _, _)| first.top() <= middle && middle <= first.bottom())
        {
            Some((_, line, count)) => {
                *line = line.union(&run);
                *count += 1;
            }
            None => lines.push((run, run, 1)),
        }
    }
    lines
        .into_iter()
        .filter(|(_, line, count)| {
            line.left() - area.left() < area.right() - line.right()
                && (*count > 1 || (line.center().x - area.center().x).abs() > px(1.))
        })
        .map(|(_, line, _)| (line.left(), line.top()))
        .collect()
}

/// A panel whose lines start at more than its edge and its offsets.
#[derive(Debug)]
struct Finding {
    panel: &'static str,
    text: u8,
    width: f32,
    /// Each edge the panel's lines start at, in from the panel's left, with
    /// how many lines start there and how far down the first one is.
    detail: String,
}

/// Records each panel of `surface` whose lines in the last frame start at
/// more than its edge and its offsets, and adds the panels with a line to
/// `measured`.
fn inspect(
    surface: Surface,
    text: u8,
    width: f32,
    measured: &mut BTreeSet<&'static str>,
    cx: &mut VisualTestContext,
) -> Vec<Finding> {
    let Drawn { runs, dialog } = drawn(cx);
    let areas: Vec<(&Panel, Bounds<Pixels>)> = panels(surface)
        .iter()
        .filter_map(|panel| area_bounds(panel.area, dialog, cx).map(|bounds| (panel, bounds)))
        .collect();
    let mut owned: Vec<Vec<Bounds<Pixels>>> = vec![Vec::new(); areas.len()];
    for Run { bounds: run, field } in runs {
        let start = point(run.left() + px(0.5), run.center().y);
        if let Some(index) = areas.iter().position(|(_, area)| area.contains(&start)) {
            // A field in a panel that measures it by its box starts where the
            // panel's text would, in from its box.
            let run = match (field, areas[index].0.boxes) {
                (Some(field), Some(inset)) => Bounds::new(
                    point(field + inset(), run.top()),
                    size(px(1.), run.size.height),
                ),
                _ => run,
            };
            owned[index].push(run);
        }
    }
    let tolerance = px(0.5);
    let mut findings = Vec::new();
    for ((panel, area), runs) in areas.iter().zip(owned) {
        let starts = line_starts(runs, *area);
        let Some(leftmost) = starts.iter().map(|(x, _)| *x).reduce(Pixels::min) else {
            continue;
        };
        let edge = panel.edge.map_or(leftmost, |edge| area.left() + edge());
        measured.insert(panel.name);
        let offsets: Vec<Pixels> = OFFSETS
            .iter()
            .filter(|offset| {
                offset
                    .panel
                    .is_none_or(|panels| panels.contains(&(surface, panel.name)))
            })
            .map(|offset| (offset.by)())
            .collect();
        let stray = starts.iter().any(|(x, _)| {
            let past = *x - edge;
            past.abs() > tolerance
                && !offsets
                    .iter()
                    .any(|offset| (past - *offset).abs() <= tolerance)
        });
        if !stray {
            continue;
        }
        let mut edges: Vec<(Pixels, usize, Pixels)> = Vec::new();
        for (x, top) in &starts {
            match edges
                .iter_mut()
                .find(|(edge, _, _)| (*edge - *x).abs() <= tolerance)
            {
                Some((_, lines, _)) => *lines += 1,
                None => edges.push((*x, 1, *top)),
            }
        }
        edges.sort_by(|a, b| f32::from(a.0).total_cmp(&f32::from(b.0)));
        let detail = edges
            .iter()
            .map(|(x, lines, top)| {
                format!(
                    "{:.2} px in ({lines} lines, the first {:.1} px down)",
                    f32::from(*x - area.left()),
                    f32::from(*top - area.top()),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        findings.push(Finding {
            panel: panel.name,
            text,
            width,
            detail,
        });
    }
    findings
}

/// The first line of each of the GitHub panel's bordered panes, by its
/// selector: the reply composer's heading, the confirmation's destination and
/// the connection pane's worktree.
const PANE_LINES: [&str; 3] = [
    "github-reply-heading",
    "github-confirm-destination",
    "github-worktree",
];

/// Where a bordered pane of the GitHub panel starts its text in the last
/// frame, when that is not exactly the panel's text edge: a compact Button's
/// label inset in from the content's side, where the panes' frames stand with
/// the Buttons' (DESIGN.md, "One left edge"). The grid reads drawn pixels,
/// which round a pane 0.75 px off, as the reply composer's padding once set
/// it, to 0.5 px, inside its tolerance, so it reads the panes' laid-out
/// bounds too. Adds each pane it finds to `seen`.
fn pane_insets(
    text: u8,
    width: f32,
    seen: &mut BTreeSet<&'static str>,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    let Some(content) = cx.debug_bounds("github-panel-content") else {
        return Vec::new();
    };
    let inset = appearance::ui_size(crate::LABEL_INSET);
    let mut off = Vec::new();
    for selector in PANE_LINES {
        let Some(line) = cx.debug_bounds(selector) else {
            continue;
        };
        seen.insert(selector);
        let at = line.left() - content.left();
        if (at - inset).abs() > px(0.25) {
            off.push(format!(
                "\n  {selector} at {text} pt, {width} px wide: {:.2} px in, not {:.2}",
                f32::from(at),
                f32::from(inset)
            ));
        }
    }
    off
}

/// Fails on every finding no [`KNOWN`] entry names, on every entry for
/// `surface` that nothing matched, and on every panel of `surface` where no
/// layout found a line, which the grid would pass unread.
fn verdict(
    surface: Surface,
    findings: Vec<Finding>,
    measured: BTreeSet<&'static str>,
    layouts: usize,
) {
    let known: Vec<&Known> = KNOWN
        .iter()
        .filter(|known| known.surface == surface)
        .collect();
    let mut matched = vec![false; known.len()];
    let mut report = String::new();
    for finding in &findings {
        match known
            .iter()
            .position(|known| known.panel == finding.panel && known.sizes.contains(&finding.text))
        {
            Some(index) => matched[index] = true,
            None => report.push_str(&format!(
                "\n  {} at {} pt, {} px wide: {}",
                finding.panel, finding.text, finding.width, finding.detail
            )),
        }
    }
    for (known, matched) in known.iter().zip(matched) {
        if !matched {
            report.push_str(&format!(
                "\n  {} starts its text at one edge: remove its entry ({})",
                known.panel, known.follow_up
            ));
        }
    }
    for panel in panels(surface) {
        if !measured.contains(panel.name) {
            report.push_str(&format!("\n  {} draws no line to measure", panel.name));
        }
    }
    assert!(report.is_empty(), "{surface:?}, {layouts} layouts:{report}");
}

/// Opens `surface` on the width walk's fixture and lays it out at both sizes
/// and widths.
async fn grid(surface: Surface, cx: &mut TestAppContext) {
    let page = match surface {
        Surface::Compare => width_walk::Page::CompareText,
        Surface::Working => width_walk::Page::Working,
        Surface::GitHub => width_walk::Page::GitHub,
        _ => width_walk::Page::History,
    };
    let (_fixture, app, cx, _) = width_walk::open(page, cx).await;
    cx.update(|window, cx| {
        app.update(cx, |app, cx| match surface {
            Surface::ProjectHub => app.show_projects(window, cx),
            Surface::ProjectPane if !app.settings.project_pane => {
                app.toggle_project_pane(window, cx)
            }
            Surface::Settings => app.show_settings(window, cx),
            _ => {}
        })
    });
    views::tests::settle(&app, cx).await;
    // The panes' panel's body, which the grid reads at its top and its end.
    let body = if surface == Surface::GitHubPanes {
        let body = cx
            .update(|window, cx| crate::github_view::open_offline_fixture_panes(&app, window, cx));
        let (operations, preferences) = app.read_with(cx, |app, _| {
            (
                app.operations.submit_read(|| Ok(())),
                app.preferences_writer.submit_read(|| Ok(())),
            )
        });
        operations.await.unwrap().unwrap();
        preferences.await.unwrap().unwrap();
        views::tests::settle(&app, cx).await;
        Some(body)
    } else {
        None
    };
    let name = format!("{surface:?}");
    match surface {
        Surface::Reflog => {
            open_dialog(&app, cx, &name, |app, window, cx| {
                app.open_reflog_browser(window, cx)
            })
            .await
        }
        Surface::Rebase => {
            open_dialog(&app, cx, &name, |app, window, cx| {
                app.open_interactive_rebase(None, window, cx)
            })
            .await
        }
        Surface::CommandPalette => {
            open_dialog(&app, cx, &name, |app, window, cx| {
                app.open_command_palette(window, cx)
            })
            .await
        }
        Surface::QuickOpen => {
            open_dialog(&app, cx, &name, |app, window, cx| {
                app.open_quick_file(window, cx)
            })
            .await
        }
        _ => {}
    }
    let mut findings = Vec::new();
    let mut measured = BTreeSet::new();
    let mut layouts = 0;
    // The bordered panes read by their laid-out bounds, and which were drawn.
    let (mut panes, mut seen) = (String::new(), BTreeSet::new());
    for text in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        for width in WIDTHS {
            cx.simulate_resize(size(px(width), px(HEIGHT)));
            frame(cx);
            app.update(cx, |_, cx| cx.notify());
            frame(cx);
            if surface == Surface::Settings {
                // The page, a scroll view at a time from its top to its end.
                let page = app.read_with(cx, |app, _| app.theme_editor.page_scroll().clone());
                let mut top = px(0.);
                loop {
                    page.set_offset(point(px(0.), -top));
                    // Settings is a cached view, which a frame that changes
                    // nothing on it replays without its debug bounds
                    // (`settings::shown`), and a scroll alone does not draw
                    // it again: a frame that builds every view.
                    frame(cx);
                    cx.update(|window, cx| {
                        window.refresh();
                        window.draw(cx).clear(cx);
                    });
                    layouts += 1;
                    findings.extend(inspect(surface, text, width, &mut measured, cx));
                    let view = cx
                        .debug_bounds("settings-scroll")
                        .expect("Settings is drawn")
                        .size
                        .height;
                    let end = page.max_offset().y.max(px(0.));
                    if top >= end {
                        break;
                    }
                    top = (top + view).min(end);
                }
                page.set_offset(point(px(0.), px(0.)));
                continue;
            }
            layouts += 1;
            findings.extend(inspect(surface, text, width, &mut measured, cx));
            if let Some(body) = &body {
                panes.extend(pane_insets(text, width, &mut seen, cx));
                body.set_offset(point(px(0.), -body.max_offset().y));
                frame(cx);
                app.update(cx, |_, cx| cx.notify());
                frame(cx);
                layouts += 1;
                findings.extend(inspect(surface, text, width, &mut measured, cx));
                panes.extend(pane_insets(text, width, &mut seen, cx));
                body.set_offset(point(px(0.), px(0.)));
            }
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    if body.is_some() {
        for selector in PANE_LINES {
            if !seen.contains(selector) {
                panes.push_str(&format!("\n  {selector} is never drawn"));
            }
        }
        assert!(
            panes.is_empty(),
            "the bordered panes start their text off the panel's text edge:{panes}"
        );
    }
    verdict(surface, findings, measured, layouts);
}

#[gpui::test]
async fn history_and_its_inspector_start_each_panel_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::History, cx).await;
}

#[gpui::test]
async fn compare_starts_its_inspector_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::Compare, cx).await;
}

#[gpui::test]
async fn working_changes_and_the_composer_start_each_panel_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::Working, cx).await;
}

#[gpui::test]
async fn the_github_panel_starts_its_text_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::GitHub, cx).await;
}

/// The GitHub panel's bordered panes, the connection pane, the reply
/// composer and the confirmation, start their text at the panel's text edge,
/// as the Conversations pane does.
#[gpui::test]
async fn the_github_panel_s_bordered_panes_start_their_text_at_its_edge(cx: &mut TestAppContext) {
    grid(Surface::GitHubPanes, cx).await;
}

#[gpui::test]
async fn the_project_hub_starts_each_panel_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::ProjectHub, cx).await;
}

#[gpui::test]
async fn the_project_pane_starts_its_text_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::ProjectPane, cx).await;
}

#[gpui::test]
async fn the_reflog_starts_its_text_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::Reflog, cx).await;
}

#[gpui::test]
async fn interactive_rebase_starts_each_panel_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::Rebase, cx).await;
}

#[gpui::test]
async fn the_command_palette_starts_its_text_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::CommandPalette, cx).await;
}

#[gpui::test]
async fn quick_open_starts_each_panel_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::QuickOpen, cx).await;
}

#[gpui::test]
async fn settings_starts_each_section_at_one_edge(cx: &mut TestAppContext) {
    grid(Surface::Settings, cx).await;
}
