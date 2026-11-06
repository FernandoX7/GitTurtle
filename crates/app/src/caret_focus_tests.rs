//! The caret a field draws as it takes focus (`vendor/gpui-base`,
//! `BlinkCursor`): each kind of field the application draws, a single-line
//! field, a multi-line field and the code editor, focused by Tab, by a click
//! and from code, paints its caret in the first frame after the focus. The
//! command palette's query, a dialog's field, is in
//! `command_palette::render_tests`.

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
