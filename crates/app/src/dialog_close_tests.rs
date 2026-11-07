//! Input that arrives after a dialog's close is decided (Return on its
//! Done, Escape, a choice) acts on what the window draws once the dialog is
//! gone (native QA-106e D6). A click on a History row right after the key
//! that closes a dialog selects that row and keeps focus on the commit
//! list, whenever it arrives: before the window draws again, as before the
//! key comes up, after the key-up, in the frame after or two frames later,
//! with motion reduced or not. A click where the closed dialog's Buttons
//! were presses none of them.
//!
//! GPUI hit-tests a press against the frame the window last drew, and a key
//! event draws a dirty window before it is dispatched, a mouse event never:
//! a press that came before the next frame and before the closing key's
//! key-up reached the gone dialog's backdrop (`vendor/gpui-component`,
//! `Root`'s close, which now has the window draw again first).

use crate::views::tests::{draw, history_script, history_window_from, settle};
use crate::*;
use core::prelude::v1::test;

/// When the click arrives after the key that closes the dialog goes down.
#[derive(Clone, Copy, Debug)]
enum Timing {
    /// Before the key comes up and before the window draws again: the
    /// platform delivers it in the frame the key closed the dialog in.
    BeforeKeyUp,
    /// After the key-up, before the platform's next frame.
    AfterKeyUp,
    /// After the key-up and the frame after it.
    NextFrame,
    /// After the key-up and two frames.
    TwoFramesLater,
}

const TIMINGS: [Timing; 4] = [
    Timing::BeforeKeyUp,
    Timing::AfterKeyUp,
    Timing::NextFrame,
    Timing::TwoFramesLater,
];

/// A press of `key` going down, or coming up.
fn key(key: &str, down: bool) -> PlatformInput {
    let keystroke = Keystroke::parse(key).expect("a keystroke");
    if down {
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        })
    } else {
        PlatformInput::KeyUp(KeyUpEvent { keystroke })
    }
}

/// A left click at `at`: its press and its release.
fn click(at: Point<Pixels>) -> [PlatformInput; 2] {
    [
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
    ]
}

/// How many times an event's delivery yields to the effects queued before
/// it, so every effect the event before it queued, and what those queue in
/// turn, has run first, as it has once the platform hands over the next
/// event.
const EFFECT_ROUNDS: usize = 32;

/// Delivers `events` to the window one after another as a platform hands
/// over input that arrives before the window's next frame: each once every
/// effect the one before it queued has run, with no frame drawn between
/// them but those the window draws itself. A test window otherwise draws
/// after every event.
fn deliver_before_a_frame(cx: &mut VisualTestContext, events: Vec<PlatformInput>) {
    fn deliver(
        window: AnyWindowHandle,
        mut events: std::vec::IntoIter<PlatformInput>,
        rounds: usize,
        cx: &mut App,
    ) {
        if rounds > 0 {
            cx.defer(move |cx| deliver(window, events, rounds - 1, cx));
            return;
        }
        let Some(event) = events.next() else {
            return;
        };
        window
            .update(cx, |_, window, cx| window.dispatch_event(event, cx))
            .expect("the window is open");
        cx.defer(move |cx| deliver(window, events, EFFECT_ROUNDS, cx));
    }
    cx.update(|window, cx| {
        let handle = window.window_handle();
        let mut events = events.into_iter();
        if let Some(first) = events.next() {
            window.dispatch_event(first, cx);
        }
        cx.defer(move |cx| deliver(handle, events, EFFECT_ROUNDS, cx));
    });
    cx.run_until_parked();
}

/// Delivers `event` as the platform does, the window drawing after it.
fn deliver(cx: &mut VisualTestContext, event: PlatformInput) {
    match event {
        PlatformInput::KeyDown(event) => cx.simulate_event(event),
        PlatformInput::KeyUp(event) => cx.simulate_event(event),
        PlatformInput::MouseDown(event) => cx.simulate_event(event),
        PlatformInput::MouseUp(event) => cx.simulate_event(event),
        other => panic!("not delivered here: {other:?}"),
    }
}

/// The platform's next frame: what waits for it runs, and the window draws.
fn platform_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
}

/// Closes the open dialog with a whole press of `closing` and clicks at
/// `at` at `timing`.
fn close_and_click(cx: &mut VisualTestContext, closing: &str, at: Point<Pixels>, timing: Timing) {
    let [press, release] = click(at);
    let (down, up) = (key(closing, true), key(closing, false));
    match timing {
        Timing::BeforeKeyUp => deliver_before_a_frame(cx, vec![down, press, release, up]),
        Timing::AfterKeyUp => deliver_before_a_frame(cx, vec![down, up, press, release]),
        Timing::NextFrame | Timing::TwoFramesLater => {
            deliver(cx, down);
            deliver(cx, up);
            platform_frame(cx);
            if matches!(timing, Timing::TwoFramesLater) {
                platform_frame(cx);
            }
            deliver(cx, press);
            deliver(cx, release);
        }
    }
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| window.has_active_dialog(cx))
}

/// Whether the accent focus ring the last frame painted lies around the
/// control drawn as `selector`.
fn ring_on(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    let Some(control) = cx.debug_bounds(selector) else {
        return false;
    };
    let ring =
        control.dilate(appearance::BUTTON_FOCUS_RING.gap + appearance::BUTTON_FOCUS_RING.width);
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    crate::focus_ring::painted_outlines_where(cx, |color| {
        color.a > 0.
            && Hsla {
                a: accent.a,
                ..color
            } == accent
    })
    .into_iter()
    .any(|outline| {
        outline.drawn
            && [
                (outline.bounds.left(), ring.left()),
                (outline.bounds.top(), ring.top()),
                (outline.bounds.right(), ring.right()),
                (outline.bounds.bottom(), ring.bottom()),
            ]
            .into_iter()
            .all(|(drawn, expected)| (drawn - expected).abs() <= px(0.5))
    })
}

/// Lets the reads the app's operation executor runs, as a dialog's
/// preparation, answer, and draws what they open.
fn reads_land(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    app.read_with(cx, |app, _| app.operations.drain());
    cx.run_until_parked();
    draw(cx);
}

/// History of a repository with twelve commits at 18 pt in a 1480 × 800
/// window, in the app's palette.
async fn history(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
    let (fixture, app, cx) = history_window_from(cx, history_script(12, 1)).await;
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.apply_appearance(window, cx));
        crate::appearance::apply_text_sizes(18, 12, window, cx);
    });
    cx.simulate_resize(size(px(1480.), px(800.)));
    settle(&app, cx).await;
    (fixture, app, cx)
}

/// Selects History's first commit and focuses the commit list, with motion
/// reduced or not.
async fn rest_on_the_first_commit(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    motion: bool,
) {
    cx.update(|window, cx| {
        cx.set_reduce_motion(!motion);
        app.update(cx, |app, cx| app.select_commit(0, window, cx));
        let list = app.read(cx).focus.clone();
        window.focus(&list, cx);
    });
    settle(app, cx).await;
    assert_eq!(app.read_with(cx, |app, _| app.selected_commit), Some(0));
}

/// Runs the command palette's `command`, typed, with a whole press of
/// Enter, and lets the dialog it prepares open.
fn run_from_the_palette(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, command: &str) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
    draw(cx);
    cx.simulate_input(command);
    draw(cx);
    deliver(cx, key("enter", true));
    deliver(cx, key("enter", false));
    reads_land(app, cx);
    assert!(dialog_open(cx), "{command} opens its dialog");
}

/// Moves focus with Tab onto the open dialog's Done.
fn tab_to_done(cx: &mut VisualTestContext) {
    for _ in 0..12 {
        cx.simulate_keystrokes("tab");
        draw(cx);
        if ring_on(cx, "dialog-ok: Done") {
            return;
        }
    }
    panic!("Tab never reached Done");
}

/// The open dialog's control `selector`, as the window last drew it.
fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
}

/// The History row whose drawn bounds hold `at`, if any.
fn row_at(cx: &mut VisualTestContext, at: Point<Pixels>) -> Option<usize> {
    (0..12).find(|index| {
        cx.debug_bounds(format!("history-row-{index}").leak())
            .is_some_and(|row| row.contains(&at))
    })
}

/// The first History row drawn wholly below the open dialog's `footer`
/// control, so a click there lands on the dialog's backdrop while it shows:
/// its index and its centre.
fn row_below(cx: &mut VisualTestContext, footer: &'static str) -> (usize, Point<Pixels>) {
    let foot = drawn(cx, footer);
    (1..12)
        .find_map(|index| {
            let row = cx.debug_bounds(format!("history-row-{index}").leak())?;
            (row.top() > foot.bottom() + px(40.)).then_some((index, row.center()))
        })
        .expect("a History row lies below the dialog")
}

/// Once the click has landed and the window's repairs and 400 ms have
/// passed: no dialog is open, History's commit `row` is selected and the
/// commit list holds focus.
async fn assert_the_click_won(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    row: usize,
    case: &str,
) {
    draw(cx);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    settle(app, cx).await;
    let (selected, list) = app.read_with(cx, |app, _| (app.selected_commit, app.focus.clone()));
    let focused = cx.update(|window, _| list.is_focused(window));
    assert!(!dialog_open(cx), "{case}: no dialog is open");
    assert_eq!(selected, Some(row), "{case}: the clicked row is selected");
    assert!(focused, "{case}: the commit list keeps focus");
}

/// Return on the branch dialog's Done, opened by the command palette's
/// "Manage current branch…" and reached with Tab, then a click on a History
/// row below the dialog: the row is selected and keeps focus at every
/// timing, with motion reduced and not. Clicked before Return came up, the
/// press reached the gone dialog's backdrop, which swallowed it.
#[gpui::test]
async fn a_click_after_return_on_done_selects_its_row(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history(cx).await;
    for motion in [false, true] {
        click_after_return_on_done(&app, cx, motion, &TIMINGS).await;
    }
}

/// As [`a_click_after_return_on_done_selects_its_row`], with the dialog
/// opened from a stash's row, History's first, as on the native route: the
/// row clicked before Return comes up, and after, is selected and keeps
/// focus.
#[gpui::test]
async fn a_click_after_return_on_done_from_a_stash_row_selects_its_row(cx: &mut TestAppContext) {
    use crate::views::tests::fixture_git;
    let (_fixture, app, cx) = history(cx).await;
    let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    std::fs::write(path.join("notes.txt"), "stashed\n").unwrap();
    fixture_git(
        &path,
        &["stash", "push", "--quiet", "--message", "Stash number 4"],
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.open(path, None, window, cx)));
    settle(&app, cx).await;
    let first = app.read_with(cx, |app, _| app.commits[app.visible[0]].subject.clone());
    assert!(
        first.starts_with("On main: Stash number 4"),
        "History's first row is the stash: {first:?}"
    );
    click_after_return_on_done(&app, cx, false, &[Timing::BeforeKeyUp, Timing::AfterKeyUp]).await;
}

/// Opens the branch dialog from History's first row by the command palette,
/// moves focus to its Done with Tab, and closes it with Return and a click on
/// a History row below the dialog at each of `timings`: the row is selected
/// and keeps focus.
async fn click_after_return_on_done(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    motion: bool,
    timings: &[Timing],
) {
    for &timing in timings {
        rest_on_the_first_commit(app, cx, motion).await;
        run_from_the_palette(app, cx, "Manage current branch");
        tab_to_done(cx);
        let (row, at) = row_below(cx, "dialog-ok: Done");
        // The pointer rests on the row as the key goes down.
        cx.simulate_mouse_move(at, None, Modifiers::default());
        draw(cx);
        close_and_click(cx, "enter", at, timing);
        let case = format!("motion {motion}, {timing:?}");
        assert_the_click_won(app, cx, row, &case).await;
    }
}

/// A click where the branch dialog's Rename… was, before Return on its Done
/// comes up, presses no Rename…: no form opens, and the page beneath takes
/// the click, a History row drawn there being selected. The gone dialog's
/// Rename… took the click and opened the rename form.
#[gpui::test]
async fn a_click_where_a_closed_dialogs_button_was_presses_nothing(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history(cx).await;
    rest_on_the_first_commit(&app, cx, false).await;
    run_from_the_palette(&app, cx, "Manage current branch");
    tab_to_done(cx);
    let at = drawn(cx, "rename-branch").center();
    let row = row_at(cx, at);
    close_and_click(cx, "enter", at, Timing::BeforeKeyUp);
    draw(cx);
    settle(&app, cx).await;
    assert!(!dialog_open(cx), "Rename… opened no form");
    if let Some(row) = row {
        let selected = app.read_with(cx, |app, _| app.selected_commit);
        assert_eq!(selected, Some(row), "the row drawn there is selected");
    }
}

/// Return on the Tags dialog's Done, opened by the command palette's
/// "Browse and manage tags…" and reached with Tab, closes it, and a click on
/// a History row before the key comes up selects that row. (Return in its
/// filter inspects the first tag the filter matches.)
#[gpui::test]
async fn a_click_after_return_closes_tags_selects_its_row(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history(cx).await;
    rest_on_the_first_commit(&app, cx, false).await;
    run_from_the_palette(&app, cx, "Browse and manage tags");
    tab_to_done(cx);
    let (row, at) = row_below(cx, "dialog-ok: Done");
    close_and_click(cx, "enter", at, Timing::BeforeKeyUp);
    assert_the_click_won(&app, cx, row, "Tags").await;
}

/// Escape closes the branch dialog, and a click on a History row before the
/// key comes up selects that row.
#[gpui::test]
async fn a_click_after_escape_selects_its_row(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history(cx).await;
    rest_on_the_first_commit(&app, cx, false).await;
    run_from_the_palette(&app, cx, "Manage current branch");
    let (row, at) = row_below(cx, "dialog-ok: Done");
    close_and_click(cx, "escape", at, Timing::BeforeKeyUp);
    assert_the_click_won(&app, cx, row, "Escape").await;
}
