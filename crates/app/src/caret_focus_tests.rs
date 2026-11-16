//! The caret a field draws as it takes focus (`vendor/gpui-base`,
//! `BlinkCursor`): each kind of field the application draws, a single-line
//! field, a multi-line field and the code editor, focused by Tab, by a click
//! and from code, paints its caret in the first frame after the focus. The
//! command palette's query, a dialog's field, is in
//! `command_palette::render_tests`. A focused single-line and multi-line
//! field also paint it in the first frame after each edit, caret move and
//! selection, and hold it a whole visible phase of the blink from there.

use crate::width_walk::{self, Page};
use crate::*;
use core::prelude::v1::test;
use gpui_kit::base::input::{InputBaseState, InputModeKind};
use std::time::Duration;

/// How a field takes focus.
#[derive(Clone, Copy, Debug)]
enum Route {
    /// Tab, from what precedes the field in the window's order of controls.
    Tab,
    /// A click on the field's text.
    Click,
    /// From code (`InputState::focus`), as the project hub focuses its name
    /// field when the folder picker returns.
    Code,
}

/// The carets the window's last frame painted: quads in the kit's caret
/// colour, as wide as its caret.
pub(crate) fn painted_carets(window: &Window, cx: &App) -> usize {
    let caret = Background::from(gpui_kit::component::Theme::global(cx).caret);
    let width = if cfg!(target_os = "macos") { 1.5 } else { 2. } * window.scale_factor();
    window
        .painted_quads()
        .into_iter()
        .filter(|quad| {
            quad.background == caret && (quad.bounds.size.width.as_f32() - width).abs() < 0.01
        })
        .count()
}

/// Lets timers that are due run, then draws a frame.
fn settle(cx: &mut VisualTestContext, time: Duration) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
    width_walk::frame(cx);
}

/// `field` (`name`), focused by each [`Route`], paints its caret in the
/// first frame after the focus. Each time it is focused first, left with its
/// caret hidden in its blink for what precedes it in the window's order of
/// controls, as Shift-Tab leaves it, and focused again at once, before
/// that blink would next have run, so only the blur shows the caret for the
/// focus that follows (`BlinkCursor::stop`); the frame is drawn
/// in the update that focuses it, before GPUI reports the focus, which
/// starts the blink, so a caret that only the report showed would be
/// missing from it, drawn a frame late.
pub(crate) fn draws_its_caret_as_it_takes_focus<M: InputModeKind + 'static>(
    cx: &mut VisualTestContext,
    name: &str,
    field: &Entity<InputBaseState<M>>,
) {
    // The caret is drawn only in the active window.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let handle = cx.update(|_, cx| field.read(cx).focus_handle(cx));
    let focused = |cx: &mut VisualTestContext| cx.update(|window, _| handle.is_focused(window));
    for route in [Route::Tab, Route::Click, Route::Code] {
        let at = format!("{name}, focused by {route:?}");
        cx.update(|window, cx| field.update(cx, |field, cx| field.focus(window, cx)));
        width_walk::frame(cx);
        assert!(focused(cx), "{at}: focused to begin with");
        // The blink hides the caret one interval after the focus.
        settle(cx, Duration::from_millis(750));
        assert_eq!(
            cx.update(|window, cx| painted_carets(window, cx)),
            0,
            "{at}: the caret is hidden in its blink as the field is left"
        );
        cx.update(|window, cx| window.focus_prev(cx));
        width_walk::frame(cx);
        assert!(!focused(cx), "{at}: the field is left");
        // Focused again at once, as Shift-Tab and then Tab focus it, before
        // the blink the field was left in would next have run.
        settle(cx, Duration::ZERO);
        let center = cx.update(|_, cx| field.read(cx).input_bounds().center());
        let carets = cx.update(|window, cx| {
            match route {
                Route::Tab => {
                    window.dispatch_keystroke(Keystroke::parse("tab").unwrap(), cx);
                }
                Route::Click => {
                    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
                    window.dispatch_event(
                        PlatformInput::MouseDown(MouseDownEvent {
                            button,
                            position: center,
                            modifiers,
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            button,
                            position: center,
                            modifiers,
                            click_count: 1,
                        }),
                        cx,
                    );
                }
                Route::Code => field.update(cx, |field, cx| field.focus(window, cx)),
            }
            window.draw(cx).clear(cx);
            painted_carets(window, cx)
        });
        assert!(focused(cx), "{at}: the field takes focus");
        assert_eq!(
            carets, 1,
            "{at}: the first frame after the focus draws the caret"
        );
        width_walk::frame(cx);
    }
}

/// The kit's blink interval (`BlinkCursor`): a blinking caret is shown this
/// long, then hidden this long.
const BLINK: Duration = Duration::from_millis(500);

/// What edits a focused field or moves its caret.
#[derive(Clone, Copy, Debug)]
enum Change {
    /// A character typed into the field.
    Type,
    /// Left, moving the caret a character back.
    Left,
    /// A click on the field's text, placing the caret there.
    Click,
    /// A double-click on the field's first word, selecting it.
    WordClick,
    /// A triple-click on the field's first line, selecting it.
    LineClick,
    /// Undo, Primary+Z: a bound action, which no key-down reaches, taking
    /// the typed character back.
    Undo,
    /// Paste, Primary+V, another bound action.
    Paste,
    /// Shift+Left, selecting the character before the caret.
    SelectLeft,
    /// Select all, Primary+A.
    SelectAll,
    /// An input method's composition, which comes with no key-down.
    Compose,
}

/// Every change, in the order [`holds_its_caret_after_an_edit_and_a_move`]
/// makes them.
const CHANGES: [Change; 10] = [
    Change::Type,
    Change::Left,
    Change::Click,
    Change::WordClick,
    Change::LineClick,
    Change::Undo,
    Change::Paste,
    Change::SelectLeft,
    Change::SelectAll,
    Change::Compose,
];

/// `field` (`name`), holding "one two three" and focused, draws its caret in
/// the first frame after each [`Change`], each made while the caret is
/// hidden in its blink, and holds it shown a whole visible phase of the
/// blink from then before it first blinks off, as GTK and AppKit hold an
/// insertion point that has just moved. It fails with every change that
/// missed, each printed. The kit held it 300 ms (`BlinkCursor::pause`), so a
/// caret typed or moved hid again before it had stood shown for one phase;
/// it paused for an edit only a caret already shown, so undo, paste and the
/// other bound actions that no key-down reaches drew no caret from a hidden
/// one; and a selection, a composition and a double- or triple-click did not
/// pause it at all.
fn holds_its_caret_after_an_edit_and_a_move<M: InputModeKind + 'static>(
    cx: &mut VisualTestContext,
    name: &str,
    field: &Entity<InputBaseState<M>>,
) {
    use gpui::EntityInputHandler as _;
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    // The caret is drawn only in the active window.
    cx.update(|window, cx| {
        window.activate_window();
        field.update(cx, |field, cx| field.set_value("one two three", window, cx));
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("pasted".into()));
    });
    cx.run_until_parked();
    cx.update(|window, cx| field.update(cx, |field, cx| field.focus(window, cx)));
    width_walk::frame(cx);
    let carets = |cx: &mut VisualTestContext| cx.update(|window, cx| painted_carets(window, cx));
    let read = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let field = field.read(cx);
            (
                field.value().to_string(),
                field.selected_range(),
                field.cursor(),
            )
        })
    };
    let press = |cx: &mut VisualTestContext, position: Point<Pixels>, click_count: usize| {
        let (button, modifiers) = (MouseButton::Left, Modifiers::default());
        cx.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button,
                    position,
                    modifiers,
                    click_count,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button,
                    position,
                    modifiers,
                    click_count,
                }),
                cx,
            );
        });
        cx.run_until_parked();
    };
    let mut missed = Vec::new();
    for change in CHANGES {
        let at = format!("{name}, after {change:?}");
        // Each change starts from the caret hidden in its blink, which it is
        // within an interval.
        for _ in 0..6 {
            if carets(cx) == 0 {
                break;
            }
            settle(cx, BLINK / 5);
        }
        assert_eq!(carets(cx), 0, "{at}: the caret hides in its blink");
        let (value, _, cursor) = read(cx);
        let first_word = cx.update(|_, cx| {
            let text = field.read(cx).text_bounds().expect("the field's text");
            point(text.left() + px(4.), text.top() + px(4.))
        });
        match change {
            Change::Type => cx.simulate_keystrokes("x"),
            Change::Left => cx.simulate_keystrokes("left"),
            Change::Click => {
                let center = cx.update(|_, cx| field.read(cx).input_bounds().center());
                cx.simulate_click(center, Modifiers::default());
            }
            Change::WordClick => press(cx, first_word, 2),
            Change::LineClick => press(cx, first_word, 3),
            Change::Undo => cx.simulate_keystrokes(&format!("{primary}-z")),
            Change::Paste => cx.simulate_keystrokes(&format!("{primary}-v")),
            Change::SelectLeft => cx.simulate_keystrokes("shift-left"),
            Change::SelectAll => cx.simulate_keystrokes(&format!("{primary}-a")),
            Change::Compose => cx.update(|window, cx| {
                field.update(cx, |field, cx| {
                    field.replace_and_mark_text_in_range(None, "ü", None, window, cx)
                })
            }),
        }
        width_walk::frame(cx);
        let (now, selected, moved) = read(cx);
        match change {
            Change::Type => assert_eq!(now.len(), value.len() + 1, "{at}: the key types"),
            Change::Left => assert_eq!(moved + 1, cursor, "{at}: the caret moves back"),
            Change::Click => assert_eq!(now, value, "{at}: the click types nothing"),
            Change::WordClick => assert!(
                !selected.is_empty() && !now[selected.clone()].contains(' '),
                "{at}: a word is selected in {now:?}: {selected:?}"
            ),
            Change::LineClick | Change::SelectAll => {
                assert_eq!(selected, 0..now.len(), "{at}: the whole text is selected")
            }
            Change::Undo => assert_ne!(now, value, "{at}: the typing is undone"),
            Change::Paste => assert!(now.contains("pasted"), "{at}: pastes into {now:?}"),
            Change::SelectLeft => assert_eq!(selected.len(), 1, "{at}: selects a character"),
            Change::Compose => assert!(now.contains('ü'), "{at}: composes into {now:?}"),
        }
        let first = carets(cx);
        settle(cx, BLINK - Duration::from_millis(1));
        let held = carets(cx);
        settle(cx, Duration::from_millis(1));
        let off = carets(cx);
        if (first, held, off) != (1, 1, 0) {
            missed.push(format!(
                "{at}: {first} caret in the first frame, {held} 499 ms later and {off} at \
                 500 ms, for 1, 1 and 0"
            ));
        }
    }
    assert!(
        missed.is_empty(),
        "changes that draw no caret at once or hold it no whole phase:\n{}",
        missed.join("\n")
    );
    settle(cx, BLINK);
    assert_eq!(carets(cx), 1, "{name}: the caret blinks on again");
}

/// History's search field, a single-line field, draws its caret in the
/// first frame after Tab, a click or code focuses it.
#[gpui::test]
async fn history_s_search_field_draws_its_caret_as_it_takes_focus(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = width_walk::open(Page::History, cx).await;
    let search = app.read_with(cx, |app, _| app.search.clone());
    draws_its_caret_as_it_takes_focus(cx, "History's search field", &search);
}

/// The composer's commit message, a multi-line field, draws its caret in
/// the first frame after Tab, a click or code focuses it.
#[gpui::test]
async fn the_commit_message_draws_its_caret_as_it_takes_focus(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = width_walk::open(Page::Working, cx).await;
    let message = app.read_with(cx, |app, _| app.commit_message.clone());
    draws_its_caret_as_it_takes_focus(cx, "the commit message", &message);
}

/// Compare's editor, the code editor, draws its caret in the first frame
/// after Tab, a click or code focuses it.
#[gpui::test]
async fn compare_s_editor_draws_its_caret_as_it_takes_focus(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = width_walk::open(Page::CompareText, cx).await;
    let editor = app.read_with(cx, |app, _| {
        app.patch_editor.clone().expect("Compare's editor")
    });
    draws_its_caret_as_it_takes_focus(cx, "Compare's editor", &editor);
}

/// History's search field, a single-line field, draws its caret at once
/// after each edit, caret move and selection, typed or by a bound action, a
/// click or an input method, and holds it a whole phase of the blink before
/// it blinks again.
#[gpui::test]
async fn history_s_search_field_holds_its_caret_after_an_edit_and_a_move(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = width_walk::open(Page::History, cx).await;
    let search = app.read_with(cx, |app, _| app.search.clone());
    holds_its_caret_after_an_edit_and_a_move(cx, "History's search field", &search);
}

/// The composer's commit message, a multi-line field, draws its caret at
/// once after each edit, caret move and selection, typed or by a bound
/// action, a click or an input method, and holds it a whole phase of the
/// blink before it blinks again.
#[gpui::test]
async fn the_commit_message_holds_its_caret_after_an_edit_and_a_move(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = width_walk::open(Page::Working, cx).await;
    let message = app.read_with(cx, |app, _| app.commit_message.clone());
    holds_its_caret_after_an_edit_and_a_move(cx, "the commit message", &message);
}
