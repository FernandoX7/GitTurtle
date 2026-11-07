//! The Tab-order audit: each page walked with Tab and again with Shift+Tab
//! at 13 pt in a 1,480 × 973 window, from the window's root until focus
//! comes round, its stops recorded by name. Every list that holds a
//! keyboard selection, and the commit message's text, is a stop met once
//! each way, in the page's visual order (DESIGN.md, one responsive order),
//! and the sweep's checks run at every stop, so each shows its ring whole
//! (`focus_ring::sweep`). Shift+Tab meets the stops Tab meets, in the other
//! order. GPUI ignores a div's `tab_stop` where the div tracks a focus
//! handle of its own, and the handle's own flag decides; the lists' handles
//! were made without it, so Tab passed over History's commit list, the
//! changed files, Working Changes' files, File history's revisions, the
//! navigator, the project list and Blame's lines, and over the message.

use crate::focus_ring::{PaintedOutline, sweep};
use crate::views::tests::{history_window, history_window_from, settle};
use crate::*;
use core::prelude::v1::test;

/// What names a stop: a focus handle the app keeps, or a drawn region,
/// found by its debug selector, that holds the stop's ring.
#[derive(Default)]
struct Names {
    handles: Vec<(FocusHandle, &'static str)>,
    regions: Vec<(&'static str, &'static str)>,
}

impl Names {
    fn handle(mut self, handle: Option<FocusHandle>, name: &'static str) -> Self {
        if let Some(handle) = handle {
            self.handles.push((handle, name));
        }
        self
    }

    /// The name of the stop holding focus, whose ring is `ring`: its
    /// handle's, else the first region holding the ring's centre, else the
    /// key contexts around it and where its ring lies.
    fn of(&self, cx: &mut VisualTestContext, ring: &[PaintedOutline]) -> String {
        let focused = cx.update(|window, cx| window.focused(cx));
        if let Some((_, name)) = self
            .handles
            .iter()
            .find(|(handle, _)| Some(handle) == focused.as_ref())
        {
            return (*name).to_owned();
        }
        let centre = ring.first().map(|outline| outline.bounds.center());
        if let Some(centre) = centre {
            for (selector, name) in &self.regions {
                if cx
                    .debug_bounds(selector)
                    .is_some_and(|region| region.contains(&centre))
                {
                    return (*name).to_owned();
                }
            }
        }
        let contexts = cx.update(|window, _| {
            window
                .context_stack()
                .iter()
                .rev()
                .take(2)
                .map(|context| format!("{context:?}"))
                .collect::<Vec<_>>()
                .join(" < ")
        });
        match centre {
            Some(centre) => format!(
                "({contexts}) at {:.0}, {:.0}",
                f32::from(centre.x),
                f32::from(centre.y)
            ),
            None => format!("({contexts})"),
        }
    }
}

/// Opens the audit's window: 1,480 × 973 at 13 pt, Midnight, the app's key
/// bindings and motion reduced.
fn prepare(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    sweep::prepare(app, cx);
    cx.simulate_resize(size(px(1480.), px(973.)));
    sweep::interface_size(cx, 13);
}

/// A stop a walk met: the handle holding focus there and its name.
type Stop = (Option<FocusHandle>, String);

/// The stops `key` meets on the shown page from `start`, or from the
/// window's root, until focus comes round, with every failure of the
/// sweep's checks at a stop added to `failures`.
fn stops(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    key: &str,
    surface: &str,
    names: &Names,
    start: Option<FocusHandle>,
    failures: &mut Vec<String>,
) -> Vec<Stop> {
    let start = start.unwrap_or_else(|| app.read_with(cx, |app, _| app.app_focus.clone()));
    cx.update(|window, cx| window.focus(&start, cx));
    views::tests::draw(cx);
    let mut met = Vec::new();
    sweep::walk_each(app, cx, key, surface, 13, failures, &mut |cx, _, ring| {
        let focused = cx.update(|window, cx| window.focused(cx));
        met.push((focused, names.of(cx, ring)));
    });
    met
}

/// Walks the shown page both ways from `start`, or from the window's root,
/// prints what each walk met for the audit's record, and checks that every
/// name in `lists` is met `once` each way (else at least once, its stops in
/// one run), in that order with Tab and the other with Shift+Tab, and that
/// Shift+Tab meets the stops Tab does, in the other order.
fn audit(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    names: &Names,
    lists: &[&str],
    once: bool,
    failures: &mut Vec<String>,
) {
    let start = (!once)
        .then(|| cx.update(|window, cx| window.focused(cx)))
        .flatten();
    let forward = stops(app, cx, "tab", surface, names, start.clone(), failures);
    let backward = stops(app, cx, "shift-tab", surface, names, start, failures);
    let named =
        |walk: &[Stop]| -> Vec<String> { walk.iter().map(|(_, name)| name.clone()).collect() };
    println!(
        "{surface}, Tab: {:#?}\n{surface}, Shift+Tab: {:#?}",
        named(&forward),
        named(&backward)
    );
    for (key, walk) in [("Tab", &forward), ("Shift+Tab", &backward)] {
        for list in lists {
            let at: Vec<usize> = walk
                .iter()
                .enumerate()
                .filter_map(|(index, (_, name))| (name == list).then_some(index))
                .collect();
            let run = at
                .first()
                .zip(at.last())
                .is_some_and(|(first, last)| last - first + 1 == at.len());
            if (once && at.len() != 1) || at.is_empty() || !run {
                failures.push(format!("{surface}: {key} meets {list} at {at:?}"));
            }
        }
    }
    // Each list once, at its first stop, in the page's visual order from the
    // window's root; a dialog's walk starts at its own stop, so its order
    // depends on that start and only its set of stops is compared.
    let order = |walk: &[Stop]| -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        for (_, name) in walk {
            if lists.contains(&name.as_str()) && order.last() != Some(name) {
                order.push(name.clone());
            }
        }
        order
    };
    let expected: Vec<String> = lists.iter().map(|list| (*list).to_owned()).collect();
    if once && order(&forward) != expected {
        failures.push(format!(
            "{surface}: Tab meets the lists as {:?}, not {expected:?}",
            order(&forward)
        ));
    }
    let mut back = order(&backward);
    back.reverse();
    if once && back != expected {
        failures.push(format!(
            "{surface}: Shift+Tab meets the lists as {back:?} reversed, not {expected:?}"
        ));
    }
    // The same stops the other way round. A page walked from the window's
    // root meets them in the reverse order; a dialog walked from the stop it
    // opened on meets the same set of them.
    let handles = |walk: &[Stop]| -> Vec<Option<FocusHandle>> {
        walk.iter().map(|(handle, _)| handle.clone()).collect()
    };
    let (forward_handles, mut reversed) = (handles(&forward), handles(&backward));
    reversed.reverse();
    let same = if once {
        reversed == forward_handles
    } else {
        reversed.len() == forward_handles.len()
            && forward_handles
                .iter()
                .all(|handle| reversed.contains(handle))
    };
    if !same {
        failures.push(format!(
            "{surface}: Shift+Tab meets {:?} reversed, Tab {:?}",
            named(&backward),
            named(&forward)
        ));
    }
}

/// The names of History's and Compare's stops that the app keeps handles
/// for.
fn repository_names(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Names {
    cx.update(|_, cx| {
        let app = app.read(cx);
        Names::default()
            .handle(Some(app.nav_focus.clone()), "the navigator")
            .handle(Some(app.search.read(cx).focus_handle(cx)), "the search")
            .handle(Some(app.focus.clone()), "the commit list")
            .handle(app.inspector_message.focus(), "the commit message")
            .handle(Some(app.file_focus.clone()), "the file list")
    })
}

#[gpui::test]
async fn history_meets_each_list_and_the_message_once_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    prepare(&app, cx);
    let names = repository_names(&app, cx);
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "History",
        &names,
        &[
            "the navigator",
            "the search",
            "the commit list",
            "the commit message",
            "the file list",
        ],
        true,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

#[gpui::test]
async fn compare_and_blame_meet_their_lists_once_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
    settle(&app, cx).await;
    let names = repository_names(&app, cx);
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "Compare",
        &names,
        &["the commit message", "the file list"],
        true,
        &mut failures,
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(0, window, cx)));
    settle(&app, cx).await;
    let blame = app.read_with(cx, |app, _| app.blame.focus());
    let names = repository_names(&app, cx).handle(blame, "the attribution lines");
    audit(
        &app,
        cx,
        "Blame",
        &names,
        &[
            "the attribution lines",
            "the commit message",
            "the file list",
        ],
        true,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

/// The colour of the 2 px leading marker the inspector's selected file row
/// paints in the last frame, none where it paints it transparent.
fn selected_row_marker(cx: &mut VisualTestContext) -> Option<Hsla> {
    let row = cx
        .debug_bounds("selected-changed-file")
        .expect("the selected file's row is drawn");
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let near = |drawn: f32, laid: Pixels| (drawn / scale - f32::from(laid)).abs() <= 0.5;
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| {
                near(quad.bounds.left().as_f32(), row.left())
                    && near(quad.bounds.top().as_f32(), row.top())
                    && near(quad.bounds.right().as_f32(), row.right())
                    && near(quad.bounds.bottom().as_f32(), row.bottom())
                    && quad.border_widths.left.as_f32() > 0.
                    && !quad.border_color.is_transparent()
            })
            .map(|quad| quad.border_color)
            .next_back()
    })
}

/// While Blame's lines show their keyboard ring, the selected file's row in
/// the inspector beside them leaves its 2 px marker to the ring, keeping
/// its selected fill, as it does beside Compare's text: at 13 pt in a 461
/// px window the two read as one doubled stroke (DESIGN.md, selected row).
/// The marker returns once the file list takes the ring.
#[gpui::test]
async fn blame_s_ring_takes_the_selected_rows_marker(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
    settle(&app, cx).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(0, window, cx)));
    blame::lands(&app, cx);
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    let (lines, list) = app.read_with(cx, |app, _| {
        (
            app.blame.focus().expect("Blame's lines"),
            app.file_focus.clone(),
        )
    });
    assert_eq!(selected_row_marker(cx), Some(accent), "at rest");
    cx.update(|window, cx| window.focus(&lines, cx));
    cx.simulate_keystrokes("down");
    views::tests::draw(cx);
    assert!(
        app.read_with(cx, |app, _| app.focus_ring.shows(&lines)),
        "Blame's lines show their ring"
    );
    assert_eq!(selected_row_marker(cx), None, "beside Blame's ring");
    cx.update(|window, cx| window.focus(&list, cx));
    cx.simulate_keystrokes("down");
    views::tests::draw(cx);
    assert_eq!(
        selected_row_marker(cx),
        Some(accent),
        "the file list holds the ring"
    );
}

/// Compare showing a Markdown file rendered: the rendered text, focusable
/// read-only text, is a stop with its ring on its own edge, before the
/// inspector's message and files.
#[gpui::test]
async fn compare_meets_the_rendered_markdown_once_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window_from(
        cx,
        "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 13\nfixture notes\nM 100644 inline README.md\ndata 19\n# Notes\n\nSome text\n\ndone\n",
    )
    .await;
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
    settle(&app, cx).await;
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.text_mode = TextMode::Markdown;
            app.prepare_editor(false, window, cx);
            cx.notify();
        })
    });
    settle(&app, cx).await;
    let rendered = cx.update(|_, cx| {
        app.read(cx)
            .markdown_view
            .as_ref()
            .map(|view| view.read(cx).focus_handle())
    });
    assert!(rendered.is_some(), "the Markdown is rendered");
    let names = repository_names(&app, cx).handle(rendered, "the rendered Markdown");
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "Compare in Rendered",
        &names,
        &[
            "the rendered Markdown",
            "the commit message",
            "the file list",
        ],
        true,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

#[gpui::test]
async fn working_changes_meet_their_list_once_in_order(cx: &mut TestAppContext) {
    let (fixture, app, cx) = history_window(cx).await;
    let root = fixture.path().join("narrow");
    std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    let names = cx.update(|_, cx| {
        let app = app.read(cx);
        Names::default()
            .handle(
                Some(app.working_filter.read(cx).focus_handle(cx)),
                "the filter",
            )
            .handle(Some(app.file_focus.clone()), "the working files")
    });
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "Working Changes",
        &names,
        &["the filter", "the working files"],
        true,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

#[gpui::test]
async fn file_history_meets_its_revisions_and_message_once_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window_from(cx, views::tests::history_script(4, 2)).await;
    prepare(&app, cx);
    views::tests::hold_quiet_reads_for_test(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
    settle(&app, cx).await;
    file_history::tests::open_landed(&app, cx).await;
    // File history's revisions take the file list's handle.
    let names = cx.update(|_, cx| {
        let app = app.read(cx);
        Names::default()
            .handle(app.inspector_message.focus(), "the commit message")
            .handle(Some(app.file_focus.clone()), "the revisions")
    });
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "File history",
        &names,
        &["the commit message", "the revisions"],
        true,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

#[gpui::test]
async fn history_beside_the_project_pane_meets_the_projects_once(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    prepare(&app, cx);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            if !app.settings.project_pane {
                app.toggle_project_pane(window, cx);
            }
        })
    });
    settle(&app, cx).await;
    let projects = app.read_with(cx, |app, _| app.project_pane.focus.clone());
    let names = repository_names(&app, cx).handle(Some(projects), "the projects");
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "History beside the project pane",
        &names,
        &[
            "the projects",
            "the navigator",
            "the search",
            "the commit list",
            "the commit message",
            "the file list",
        ],
        true,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

#[gpui::test]
async fn settings_meet_their_stops_the_same_both_ways(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    cx.update(|_, cx| theme_editor::init(cx));
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let mut failures = Vec::new();
    // Settings holds no selection list: its theme cards are Buttons, and the
    // Your themes rows are no stops by design (`theme_editor::State::row_focus`).
    audit(
        &app,
        cx,
        "Settings",
        &Names::default(),
        &[],
        true,
        &mut failures,
    );
    sweep::assert_whole(failures);
}
