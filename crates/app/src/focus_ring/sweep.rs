//! The focus-visibility sweep: every page and dialog, at the default and the
//! largest interface size, walked with Tab until focus comes round again.
//! At every stop something drawn holds focus, never nothing or the window's
//! root, and it paints its ring whole: all four sides drawn and none cut by
//! a clip around it, the window's edge included.
//!
//! A stop's ring is what focus adds to the frame: the outlines in the accent,
//! at any opacity, that the frame draws with the stop focused and not with
//! the window blurred, its layout otherwise the same. A Button's 2 px ring,
//! an Input's accent edge and the kit's ring outside it, and a list's or a
//! pane's 1 px edge all count, so a stop shows its focus however it draws it.
//! The modules whose dialogs need their own fixtures walk them with
//! [`walk`]: `discard::tests`, `worktrees::tests` and `github_view::tests`.
use super::{PaintedOutline, painted_outlines_where};
use crate::appearance::{ThemeChoice, custom::ThemeSelection};
use crate::*;
use ::core::prelude::v1::test;

/// The interface sizes the sweep walks, in points: the default and the
/// largest.
pub(crate) const SIZES: [u8; 2] = [13, 18];

/// More stops than any surface has: a walk that has not come round by then
/// never will.
const MAX_STOPS: usize = 300;

/// Draws the window `frames` times, running what each frame started, so a
/// scroll that a focus change requests lands before the frame is read.
fn draw(cx: &mut VisualTestContext, frames: usize) {
    for _ in 0..frames {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.executor().run_until_parked();
    }
}

/// The last frame's outlines in the accent, at any opacity.
fn accent_outlines(cx: &mut VisualTestContext) -> Vec<PaintedOutline> {
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    painted_outlines_where(cx, move |color| {
        color.a > 0.
            && Hsla {
                a: accent.a,
                ..color
            } == accent
    })
}

/// What holding focus on `focus` adds to the frame: the accent outlines that
/// show with it focused and that a blurred window does not draw. Focus is
/// back on `focus` afterwards.
fn ring(cx: &mut VisualTestContext, focus: &FocusHandle) -> Vec<PaintedOutline> {
    let focused = accent_outlines(cx);
    cx.update(|window, cx| window.blur(cx));
    draw(cx, 1);
    let blurred = accent_outlines(cx);
    cx.update(|window, cx| window.focus(focus, cx));
    draw(cx, 1);
    focused
        .into_iter()
        .filter(|outline| {
            outline.drawn
                && !blurred
                    .iter()
                    .any(|other| other.bounds == outline.bounds && other.color == outline.color)
        })
        .collect()
}

/// Walks the shown surface with Tab from wherever it put focus until focus
/// comes round again, checking every stop, and adds each failing stop to
/// `failures`, named by `surface`, the interface size and its place in the
/// order. Returns the bounds of every ring the walk saw, so a caller can
/// check that Tab reached a particular pane.
pub(crate) fn walk(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    interface: u8,
    failures: &mut Vec<String>,
) -> Vec<Bounds<Pixels>> {
    let root = app.read_with(cx, |app, _| app.app_focus.clone());
    let mut order: Vec<FocusHandle> = Vec::new();
    let mut rings = Vec::new();
    for stop in 0..MAX_STOPS {
        cx.simulate_keystrokes("tab");
        draw(cx, 2);
        // Moving the pointer without a press keeps the ring: GPUI's
        // `focus_visible` dropped it here.
        cx.simulate_mouse_move(point(px(2.), px(2.)), None, Modifiers::none());
        draw(cx, 1);
        // The key contexts around focus name the stop well enough to find.
        let contexts = cx.update(|window, _| {
            let stack = window.context_stack();
            let names: Vec<_> = stack
                .iter()
                .rev()
                .take(3)
                .map(|context| format!("{context:?}"))
                .collect();
            names.join(" < ")
        });
        let at = format!("{surface} at {interface} pt, stop {stop} ({contexts})");
        let Some(focused) = cx.update(|window, cx| window.focused(cx)) else {
            failures.push(format!("{at}: Tab left nothing focused"));
            return rings;
        };
        if let Some(seen) = order.iter().position(|other| *other == focused) {
            if seen > 0 && seen + 1 == order.len() {
                failures.push(format!("{at}: Tab keeps focus where it was"));
            }
            return rings;
        }
        if focused == root {
            failures.push(format!(
                "{at}: the window's root holds focus and shows none"
            ));
        } else if contexts.is_empty() {
            failures.push(format!(
                "{at}: focus is on an element the frame does not draw"
            ));
        } else {
            let ring = ring(cx, &focused);
            rings.extend(ring.iter().map(|outline| outline.bounds));
            if ring.is_empty() {
                failures.push(format!("{at}: the focused element paints no ring"));
            }
            for outline in ring.iter().filter(|outline| !outline.whole) {
                let cut: Vec<_> = ["top", "right", "bottom", "left"]
                    .into_iter()
                    .zip(outline.sides)
                    .filter_map(|(side, whole)| (!whole).then_some(side))
                    .collect();
                failures.push(format!(
                    "{at}: a clip cuts the {} of its ring at {:?}, {:?} wide",
                    cut.join(", "),
                    outline.bounds,
                    outline.width,
                ));
            }
        }
        order.push(focused);
    }
    failures.push(format!(
        "{surface} at {interface} pt: Tab did not come round in {MAX_STOPS} stops"
    ));
    rings
}

/// Whether one of `rings` lies on `pane`'s own edge, as a pane's 1 px
/// keyboard ring does.
pub(crate) fn rings_edge(rings: &[Bounds<Pixels>], pane: Bounds<Pixels>) -> bool {
    rings.iter().any(|ring| {
        [
            (ring.left(), pane.left()),
            (ring.top(), pane.top()),
            (ring.right(), pane.right()),
            (ring.bottom(), pane.bottom()),
        ]
        .into_iter()
        .all(|(drawn, edge)| (drawn - edge).abs() <= px(0.5))
    })
}

/// Fails with every stop that failed.
pub(crate) fn assert_whole(failures: Vec<String>) {
    assert_whole_except(failures, &[]);
}

/// Fails with every stop that failed, except the known failures, each a
/// stop's place (the start of its failure, such as "Quick Open at 13 pt,
/// stop 1") and its finding (part of the failure's text), awaiting the
/// follow-up its caller names. A known failure that no longer happens fails
/// too, so its entry leaves with its fix.
pub(crate) fn assert_whole_except(failures: Vec<String>, known: &[(&str, &str)]) {
    let matches = |failure: &String, (at, finding): &(&str, &str)| {
        failure.starts_with(at) && failure.contains(finding)
    };
    let unexpected: Vec<_> = failures
        .iter()
        .filter(|failure| !known.iter().any(|entry| matches(failure, entry)))
        .cloned()
        .collect();
    let fixed: Vec<_> = known
        .iter()
        .filter(|entry| !failures.iter().any(|failure| matches(failure, entry)))
        .map(|(at, finding)| format!("{at}: {finding}"))
        .collect();
    assert!(
        unexpected.is_empty() && fixed.is_empty(),
        "stops that fail:\n{}\nknown failures that no longer happen; remove them:\n{}",
        unexpected.join("\n"),
        fixed.join("\n")
    );
}

/// The app's key bindings, Midnight, dialogs opening at rest and a 1280 ×
/// 800 window.
pub(crate) fn prepare(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        native_accessibility::bind_keys(cx);
        shortcuts::bind_keys(cx);
        cx.set_reduce_motion(true);
        app.update(cx, |app, cx| {
            app.settings.theme = ThemeSelection::BuiltIn(ThemeChoice::Midnight);
            app.settings.follow_system = false;
            app.apply_appearance(window, cx);
        })
    });
    cx.simulate_resize(size(px(1280.), px(800.)));
    draw(cx, 2);
}

/// Sets the interface size the next walk uses.
pub(crate) fn interface_size(cx: &mut VisualTestContext, interface: u8) {
    cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
    draw(cx, 2);
}

/// Closes `surface`, the open dialog, with Escape, as a person would, so it
/// closes through its own path; a dialog that Escape leaves open fails.
pub(crate) fn close_dialogs(
    cx: &mut VisualTestContext,
    surface: &str,
    interface: u8,
    failures: &mut Vec<String>,
) {
    for _ in 0..4 {
        if !cx.update(|window, cx| window.has_active_dialog(cx)) {
            return;
        }
        cx.simulate_keystrokes("escape");
        draw(cx, 2);
    }
    failures.push(format!(
        "{surface} at {interface} pt: Escape leaves it open"
    ));
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx, 2);
}

/// Opens a dialog with `open`, lets its reads finish, walks it, and closes
/// it.
async fn walk_dialog(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    interface: u8,
    failures: &mut Vec<String>,
    open: impl FnOnce(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
) {
    open_dialog(app, cx, surface, open).await;
    walk(app, cx, surface, interface, failures);
    close_dialogs(cx, surface, interface, failures);
}

/// Opens a dialog with `open` from History's list, lets its reads finish and
/// waits for the control it focused to draw; the layout grid
/// (`layout_grid`) opens its dialogs here too.
pub(crate) async fn open_dialog(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    open: impl FnOnce(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
) {
    // Each dialog opens from History's list, as from the page a person is
    // on, rather than from wherever the last dialog's close left focus.
    let list = app.read_with(cx, |app, _| app.focus.clone());
    cx.update(|window, cx| window.focus(&list, cx));
    draw(cx, 2);
    cx.update(|window, cx| app.update(cx, |app, cx| open(app, window, cx)));
    views::tests::settle(app, cx).await;
    // A dialog whose read finishes late draws the control it focused a frame
    // or more after opening; a walk that never finds focus drawn still fails.
    for _ in 0..10 {
        if !cx.update(|window, _| window.context_stack().is_empty()) {
            break;
        }
        draw(cx, 1);
    }
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "{surface} opens"
    );
}

/// History and, with its file activated, Compare.
#[gpui::test]
async fn history_and_compare_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk(&app, cx, "History", interface, &mut failures);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        views::tests::settle(&app, cx).await;
        walk(&app, cx, "Compare", interface, &mut failures);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        views::tests::settle(&app, cx).await;
    }
    // Follow-up (focus sweep, 2026-10-28): Compare's text, a read-only kit
    // Input, shows its focus only by its caret; its pane should draw the
    // window's keyboard ring on its edge, as the file lists do.
    assert_whole_except(
        failures,
        &[
            // The fixture has no remote, so the action bar offers Add
            // remote… as a stop before these (`empty_state`).
            ("Compare at 13 pt, stop 27", "paints no ring"),
            ("Compare at 18 pt, stop 26", "paints no ring"),
        ],
    );
}

/// Working Changes with a modified and an untracked file.
#[gpui::test]
async fn working_changes_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (fixture, app, cx) = views::tests::history_window(cx).await;
    let root = fixture.path().join("narrow");
    std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    views::tests::settle(&app, cx).await;
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk(&app, cx, "Working Changes", interface, &mut failures);
    }
    // Follow-up (focus sweep, 2026-10-28): at 13 pt the composer's
    // description field keeps Tab, so the walk ends there and the stops after
    // it go unchecked at that size; at 18 pt the walk comes round. Tab should
    // leave the field, or the field should name the key that does.
    assert_whole_except(
        failures,
        &[(
            "Working Changes at 13 pt, stop 6",
            "Tab keeps focus where it was",
        )],
    );
}

/// Settings and the theme editor it opens.
#[gpui::test]
async fn settings_and_the_theme_editor_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    cx.update(|_, cx| theme_editor::init(cx));
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    views::tests::settle(&app, cx).await;
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk(&app, cx, "Settings", interface, &mut failures);
        walk_dialog(
            &app,
            cx,
            "the theme editor",
            interface,
            &mut failures,
            |app, window, cx| app.open_theme_editor(None, window, cx),
        )
        .await;
    }
    assert_whole(failures);
}

/// The project hub, and History beside the project pane.
#[gpui::test]
async fn the_project_hub_and_pane_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
        views::tests::settle(&app, cx).await;
        walk(&app, cx, "the project hub", interface, &mut failures);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_history(window, cx);
                if !app.settings.project_pane {
                    app.toggle_project_pane(window, cx);
                }
            })
        });
        views::tests::settle(&app, cx).await;
        walk(
            &app,
            cx,
            "History beside the project pane",
            interface,
            &mut failures,
        );
    }
    // Follow-up (focus sweep, 2026-10-28): the hub's project list clips the
    // rings of its first project row and of that row's action button.
    assert_whole_except(
        failures,
        &[
            ("the project hub at 13 pt, stop 7", "a clip cuts"),
            ("the project hub at 13 pt, stop 8", "a clip cuts"),
            ("the project hub at 18 pt, stop 7", "a clip cuts"),
            ("the project hub at 18 pt, stop 8", "a clip cuts"),
        ],
    );
}

/// The repository's dialogs: Reflog, interactive rebase, the rewrite review,
/// the command palette, Quick Open and Keyboard Shortcuts.
#[gpui::test]
async fn repository_dialogs_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk_dialog(
            &app,
            cx,
            "the Reflog",
            interface,
            &mut failures,
            |app, window, cx| app.open_reflog_browser(window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "interactive rebase",
            interface,
            &mut failures,
            |app, window, cx| app.open_interactive_rebase(None, window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "the rewrite review",
            interface,
            &mut failures,
            |app, window, cx| app.open_rewrite_review(window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "the command palette",
            interface,
            &mut failures,
            |app, window, cx| app.open_command_palette(window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "Quick Open",
            interface,
            &mut failures,
            |app, window, cx| app.open_quick_file(window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "Keyboard Shortcuts",
            interface,
            &mut failures,
            |app, window, cx| app.shortcut_help(window, cx),
        )
        .await;
    }
    // Follow-up (focus sweep, 2026-10-28): Quick Open's scrolling form and
    // the Keyboard Shortcuts dialog's body clip at their content edges, so
    // the scope buttons and the filters along them lose part of their rings.
    // They should keep the ring's room inside their clips, as the rebase form
    // and the rewrite review do, without moving their text off the dialog's
    // inset.
    assert_whole_except(
        failures,
        &[
            ("Quick Open at 13 pt, stop 1", "a clip cuts"),
            ("Quick Open at 13 pt, stop 2", "a clip cuts"),
            ("Quick Open at 13 pt, stop 3", "a clip cuts"),
            ("Keyboard Shortcuts at 13 pt, stop 1", "a clip cuts"),
            ("Quick Open at 18 pt, stop 1", "a clip cuts"),
            ("Quick Open at 18 pt, stop 2", "a clip cuts"),
            ("Quick Open at 18 pt, stop 3", "a clip cuts"),
            ("Keyboard Shortcuts at 18 pt, stop 1", "a clip cuts"),
        ],
    );
}
