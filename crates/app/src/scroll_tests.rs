//! Consuming regressions for the vendored editor's viewport contract. These use
//! real component layout and wheel dispatch; native frame quality is separate QA.

use crate::*;
use core::prelude::v1::test;
use gpui_kit::component::input::Editor;

struct ScrollProbe {
    editor: Entity<EditorState>,
}

impl Render for ScrollProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(480.)).h(px(260.)).child(
            Editor::new(&self.editor)
                .readonly(true)
                .text_size(px(12.))
                .w_full()
                .h_full(),
        )
    }
}

fn fixture(
    cx: &mut TestAppContext,
    initial: Option<Point<Pixels>>,
) -> (Entity<ScrollProbe>, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(move |window, cx| ScrollProbe {
        editor: cx.new(|cx| {
            // Long lines exercise independent horizontal and vertical bounds.
            // The document remains small enough for deterministic unit tests.
            let source = (0..400)
                .map(|row| format!("row {row:03}: {}\n", "abcdefghij".repeat(20)))
                .collect::<String>();
            let mut editor = EditorState::new(window, cx)
                .language("text")
                .line_number(false)
                .folding(false)
                .soft_wrap(false)
                .default_value(source);
            if let Some(offset) = initial {
                assert!(editor.line_height().is_none());
                editor.set_scroll_offset(offset, cx);
            }
            editor
        }),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

fn assert_rendered_viewport(editor: &EditorState) {
    let offset = editor.scroll_offset();
    let height = editor.line_height().expect("a rendered editor");
    let rows = editor.visible_row_range().expect("rendered visible rows");
    let expected_top = (f32::from(-offset.y) / f32::from(height)).floor() as usize;
    assert_eq!(
        rows.start, expected_top,
        "painted row range must describe the current viewport"
    );
    assert!(rows.end > rows.start, "the viewport contains source rows");
    assert!(rows.end <= 401, "no rows beyond the supplied document");
}

#[gpui::test]
fn linked_offsets_are_current_before_the_next_repaint(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx, None);
    let editor = cx.read(|cx| view.read(cx).editor.clone());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(5..12, cx);
            editor.focus(window, cx);
        });
        window.draw(cx).clear(cx);
        editor.update(cx, |editor, cx| {
            for y in [-147., -169.] {
                let next = point(px(-80.), px(y));
                editor.set_scroll_offset(next, cx);
                assert_eq!(
                    editor.scroll_offset(),
                    next,
                    "linked observers must not read an older paint acknowledgement"
                );
            }
            assert_eq!(editor.selected_range(), 5..12);
            assert!(editor.focus_handle(cx).is_focused(window));
        });
        window.draw(cx).clear(cx);
        let editor = editor.read(cx);
        assert_eq!(editor.scroll_offset(), point(px(-80.), px(-169.)));
        assert_eq!(editor.selected_range(), 5..12);
        assert!(editor.focus_handle(cx).is_focused(window));
        assert_rendered_viewport(editor);
    });
}

#[gpui::test]
fn a_new_wheel_gesture_supersedes_pending_linked_scroll(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx, None);
    let editor = cx.read(|cx| view.read(cx).editor.clone());
    cx.update(|window, cx| {
        let position = editor.read(cx).input_bounds().center();
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(point(px(-80.), px(-169.)), cx);
        });
        // Deliver real wheel input before the pending setter gets a paint.
        window.dispatch_event(
            gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-23.))),
                modifiers: Modifiers::default(),
                touch_phase: gpui::TouchPhase::Moved,
            }),
            cx,
        );
        assert_eq!(
            editor.read(cx).scroll_offset(),
            point(px(-80.), px(-192.)),
            "the gesture must start at the most recently accepted linked position"
        );
        window.draw(cx).clear(cx);
        assert_eq!(
            editor.read(cx).scroll_offset(),
            point(px(-80.), px(-192.)),
            "painting must not restore an older programmatic request"
        );
        assert_rendered_viewport(editor.read(cx));
    });
}

#[gpui::test]
fn programmatic_scroll_stays_within_rendered_document_bounds(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx, None);
    let editor = cx.read(|cx| view.read(cx).editor.clone());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(point(px(-1_000_000.), px(-1_000_000.)), cx);
            let offset = editor.scroll_offset();
            assert!(offset.x < px(0.) && offset.x > px(-1_000_000.));
            assert!(offset.y < px(0.) && offset.y > px(-1_000_000.));
        });
        window.draw(cx).clear(cx);
        assert_rendered_viewport(editor.read(cx));
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(point(px(1_000.), px(1_000.)), cx);
            assert_eq!(editor.scroll_offset(), point(px(0.), px(0.)));
        });
        window.draw(cx).clear(cx);
        assert_eq!(editor.read(cx).scroll_offset(), point(px(0.), px(0.)));
        assert_rendered_viewport(editor.read(cx));
    });
}

#[gpui::test]
fn a_cold_editor_applies_its_requested_viewport_on_first_layout(cx: &mut TestAppContext) {
    let requested = point(px(-80.), px(-169.));
    let (view, cx) = fixture(cx, Some(requested));
    let editor = cx.read(|cx| view.read(cx).editor.clone());
    cx.read(|cx| {
        assert_eq!(editor.read(cx).scroll_offset(), requested);
        assert_rendered_viewport(editor.read(cx));
    });
}
