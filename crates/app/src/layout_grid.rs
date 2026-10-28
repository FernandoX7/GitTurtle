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
//! A run its clip cuts on the left, scrolled sideways, starts no line.
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
    ProjectHub,
    /// History beside the project pane.
    ProjectPane,
    Reflog,
    Rebase,
    CommandPalette,
    QuickOpen,
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
}

/// A panel whose text starts at one left edge.
struct Panel {
    name: &'static str,
    area: Area,
}

const fn panel(name: &'static str, area: Area) -> Panel {
    Panel { name, area }
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
        panel("working files", Union(&["working-header", "working-pane"])),
        panel("composer", Selector("commit-composer")),
    ];
    const GITHUB: &[Panel] = &[panel("GitHub panel", Selector("github-panel"))];
    const PROJECT_HUB: &[Panel] = &[
        panel("recent projects", Selector("project-hub-recent")),
        panel("project action", Selector("project-hub-action")),
    ];
    const PROJECT_PANE: &[Panel] = &[panel("project pane", Selector("project-pane"))];
    const REFLOG: &[Panel] = &[panel("Reflog", Dialog)];
    const REBASE: &[Panel] = &[panel("interactive rebase", Dialog)];
    const PALETTE: &[Panel] = &[panel("command palette", Dialog)];
    const QUICK_OPEN: &[Panel] = &[panel("Quick Open", Dialog)];
    match surface {
        Surface::History => HISTORY,
        Surface::Compare => COMPARE,
        Surface::Working => WORKING,
        Surface::GitHub => GITHUB,
        Surface::ProjectHub => PROJECT_HUB,
        Surface::ProjectPane => PROJECT_PANE,
        Surface::Reflog => REFLOG,
        Surface::Rebase => REBASE,
        Surface::CommandPalette => PALETTE,
        Surface::QuickOpen => QUICK_OPEN,
    }
}

/// A deliberate offset from a panel's edge: a line of the panel may start
/// `by` past it, at the interface size, for `reason`. An offset for no
/// panel in particular holds in every panel.
struct Offset {
    panel: Option<(Surface, &'static str)>,
    by: fn() -> Pixels,
    #[allow(dead_code, reason = "read by people, as a follow-up is")]
    reason: &'static str,
}

const OFFSETS: &[Offset] = &[
    Offset {
        panel: None,
        by: || appearance::ui_size(crate::LABEL_INSET),
        reason: "a compact Button at the edge starts its label its label inset in",
    },
    Offset {
        panel: None,
        by: || appearance::FIELD_TEXT_INSET,
        reason: "a field at the edge starts its text and placeholder past its border and padding",
    },
    Offset {
        panel: Some((Surface::History, "navigation")),
        by: || px(15.) + appearance::ui_size(6.5),
        reason: "a branch or worktree row's 15 px icon stands at the edge, its name after the gap",
    },
    Offset {
        panel: Some((Surface::History, "References column")),
        by: || appearance::ui_size(3.25),
        reason: "a reference's pill stands at the column's edge and pads its name 0.25 rem in",
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
    // The commit details start 10.5 px in (14.5 at 18 pt), the changed-files
    // header's icon at the pane's 9.75 (13.5), the inspector's heading 42.5
    // px in (59) and the file rows' names 46 (52.5): no single edge with a
    // leading icon or status tile at it.
    Known {
        surface: Surface::History,
        panel: "inspector",
        sizes: &[13, 18],
        follow_up: "the inspector's details, heading, files header and file rows share one edge",
    },
    Known {
        surface: Surface::Compare,
        panel: "inspector",
        sizes: &[13, 18],
        follow_up: "the inspector's details, heading, files header and file rows share one edge",
    },
    // At 18 pt in a 461 px window, narrow History's subject rows start 6.5
    // px into the column, its title 10 px.
    Known {
        surface: Surface::History,
        panel: "Subject column",
        sizes: &[18],
        follow_up: "narrow History's subject cell starts at the column's 10 px inset",
    },
    // Text at its 10 px label inset (14 px at 18 pt), fields' text at 11, a
    // card's Button label at 20.5, a nested card's text 32 and 42.5 (44.5).
    Known {
        surface: Surface::GitHub,
        panel: "GitHub panel",
        sizes: &[13, 18],
        follow_up: "the GitHub panel's fields, cards and nested rows start at its text inset",
    },
    // The search field's text 11 px in, the project rows' names 72 (83.5).
    Known {
        surface: Surface::ProjectHub,
        panel: "recent projects",
        sizes: &[13, 18],
        follow_up: "the hub's project rows start at the recent list's edge, after their icon",
    },
    // The action card's text 17 px in (23.5 at 18 pt), its fields' labels
    // 42.5 (53) and its tab labels 68.5.
    Known {
        surface: Surface::ProjectHub,
        panel: "project action",
        sizes: &[13, 18],
        follow_up: "the hub's action card starts its title, tabs and field labels at one edge",
    },
    // The pane's heading 9.5 px in (13.5 at 18 pt), a project row's name
    // 33.5 (36), its icon not at the heading's edge.
    Known {
        surface: Surface::ProjectPane,
        panel: "project pane",
        sizes: &[13, 18],
        follow_up: "the project pane's rows put their icon at its heading's edge",
    },
    // The Staged and Unstaged headings 33 px in (39.5 at 18 pt), the file
    // rows' names 46.5 (49): their leading glyph and status tile differ.
    Known {
        surface: Surface::Working,
        panel: "working files",
        sizes: &[13, 18],
        follow_up: "the Working headings and file rows put their glyph and tile at one edge",
    },
    // The composer's labels 9.5 px in (13.5 at 18 pt), the line below its
    // heading 27.5, a note under the description 22.5 (26.5).
    Known {
        surface: Surface::Working,
        panel: "composer",
        sizes: &[13, 18],
        follow_up: "the composer starts its staged summary and notes at its labels' edge",
    },
];

/// The widths the grid lays each surface out at: a 1,480 px window and a
/// 461 px half tile, both 980 px tall.
const WIDTHS: [f32; 2] = [1480., 461.];
const HEIGHT: f32 = 980.;

/// What the last frame drew: its text runs and fields' text, each where it
/// starts and as high as its line, and the open dialog's bounds.
struct Drawn {
    runs: Vec<Bounds<Pixels>>,
    dialog: Option<Bounds<Pixels>>,
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
            let run = if quad.background == mark {
                drawn
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
                Bounds::new(
                    point(drawn.left() + inset, drawn.top()),
                    size(px(1.), drawn.size.height),
                )
            } else {
                return None;
            };
            (run.left() >= clip.left() - slack
                && run.left() < clip.right()
                && run.top() < clip.bottom()
                && run.bottom() > clip.top())
            .then_some(run)
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
    for run in runs {
        let start = point(run.left() + px(0.5), run.center().y);
        if let Some(index) = areas.iter().position(|(_, area)| area.contains(&start)) {
            owned[index].push(run);
        }
    }
    let tolerance = px(0.5);
    let mut findings = Vec::new();
    for ((panel, area), runs) in areas.iter().zip(owned) {
        let starts = line_starts(runs, *area);
        let Some(edge) = starts.iter().map(|(x, _)| *x).reduce(Pixels::min) else {
            continue;
        };
        measured.insert(panel.name);
        let offsets: Vec<Pixels> = OFFSETS
            .iter()
            .filter(|offset| offset.panel.is_none_or(|own| own == (surface, panel.name)))
            .map(|offset| (offset.by)())
            .collect();
        let stray = starts.iter().any(|(x, _)| {
            let past = *x - edge;
            past > tolerance
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
            _ => {}
        })
    });
    views::tests::settle(&app, cx).await;
    let dialog: Option<fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>)> = match surface {
        Surface::Reflog => Some(|app, window, cx| app.open_reflog_browser(window, cx)),
        Surface::Rebase => Some(|app, window, cx| app.open_interactive_rebase(None, window, cx)),
        Surface::CommandPalette => Some(|app, window, cx| app.open_command_palette(window, cx)),
        Surface::QuickOpen => Some(|app, window, cx| app.open_quick_file(window, cx)),
        _ => None,
    };
    if let Some(open) = dialog {
        open_dialog(&app, cx, &format!("{surface:?}"), open).await;
    }
    let mut findings = Vec::new();
    let mut measured = BTreeSet::new();
    let mut layouts = 0;
    for text in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        for width in WIDTHS {
            cx.simulate_resize(size(px(width), px(HEIGHT)));
            frame(cx);
            app.update(cx, |_, cx| cx.notify());
            frame(cx);
            layouts += 1;
            findings.extend(inspect(surface, text, width, &mut measured, cx));
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
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
