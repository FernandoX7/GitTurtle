//! Read-only text refuses text input (`vendor/gpui-base`,
//! `InputBaseState::accepts_text_input`). A text the user cannot change gives
//! the platform an input handler that takes no text. Wayland then keeps its
//! text input off while the text is focused, so an input method such as fcitx5
//! no longer turns a held key's repeats into fresh presses. macOS sends
//! printable keys to the application's bindings before the input method of a
//! non-ASCII source. Fields the user types into still take text input, and a
//! field made read-only for a while takes it again once it is editable.

use crate::views::tests::{draw, fixture_git, history_script, history_window_from, settle};
use crate::*;
use core::prelude::v1::test;

gpui_kit::actions!(read_only_text_tests, [Chord]);

/// Binds a two-key binding that [`takes_text_input`] begins and completes.
/// No other binding starts with its first key.
fn bind_chord(cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        cx.bind_keys([KeyBinding::new("q z", Chord, None)]);
        cx.on_action(|_: &Chord, _| {});
    });
}

/// Whether the input handler the window's last frame gave the platform (the
/// focused element's) takes text input. GPUI asks it when the first key of a
/// two-key binding goes down: a pending binding waits to flush its keys as
/// text only where the handler takes text input (the timeout of
/// `Window::pending_input`). The second key completes the binding, so nothing
/// is typed.
fn takes_text_input(cx: &mut VisualTestContext) -> bool {
    draw(cx);
    cx.simulate_keystrokes("q");
    let waits = cx.update(|window, _| {
        window
            .pending_input()
            .expect("the binding's first key is pending")
            .timeout()
            .is_some()
    });
    cx.simulate_keystrokes("z");
    assert!(
        cx.update(|window, _| window.pending_input().is_none()),
        "the binding completes"
    );
    waits
}

/// Focuses `editor`'s text, as a click or Tab would.
fn focus(editor: &Entity<EditorState>, cx: &mut VisualTestContext) {
    let focus = cx.update(|_, cx| editor.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    assert!(cx.update(|window, _| focus.is_focused(window)));
}

/// Opens Find on `editor`, focused, and checks that its field, which then
/// holds focus, takes text input and types: `typed` reaches the query.
async fn find_field_takes_text_input(
    app: &Entity<GitTurtle>,
    editor: &Entity<EditorState>,
    typed: &str,
    at: &str,
    cx: &mut VisualTestContext,
) {
    focus(editor, cx);
    cx.dispatch_action(gpui_kit::component::input::Search);
    settle(app, cx).await;
    draw(cx);
    assert!(
        cx.update(|_, cx| crate::editor_find::panel_height(editor, cx) > px(0.)),
        "{at}: Find opens"
    );
    assert!(
        cx.update(|window, cx| !editor.focus_handle(cx).is_focused(window)),
        "{at}: Find's field holds focus"
    );
    assert!(takes_text_input(cx), "{at}: Find's field takes text input");
    cx.simulate_input(typed);
    settle(app, cx).await;
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.search_session().query.clone()),
        typed,
        "{at}: Find's field takes the keys typed into it"
    );
    cx.simulate_keystrokes("escape");
    settle(app, cx).await;
}

/// The Diff view's text, focused while its per-line staging controls show,
/// refuses text input, as Space selects lines there; Find's field opened on
/// it takes text input. Before, the diff's read-only text took text input, so
/// under Wayland a held Space reached the view as fresh presses, each toggling
/// the line again, and on macOS a non-ASCII source's input method took Space.
#[gpui::test]
async fn the_diff_text_with_line_controls_refuses_text_input(cx: &mut TestAppContext) {
    let (fixture, app, cx) = history_window_from(cx, &history_script(1, 12)).await;
    bind_chord(cx);
    let root = fixture.path().join("narrow");
    fixture_git(&root, &["reset", "--hard", "--quiet"]);
    let edited: String = (1..=12)
        .map(|line| match line {
            4 => "four\n".to_owned(),
            5 => "five\n".to_owned(),
            _ => format!("note {line}\n"),
        })
        .collect();
    std::fs::write(root.join("notes.txt"), edited).unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    crate::path_filter::filters_land(&app, cx);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.text_mode = TextMode::Unified;
            app.select_working(0, gitturtle_core::ChangeArea::Unstaged, window, cx);
        })
    });
    settle(&app, cx).await;
    let (editor, view) = app.read_with(cx, |app, _| {
        assert!(
            matches!(
                app.content.as_deref(),
                Some(Content::Text {
                    partial: Some(_),
                    ..
                })
            ),
            "the diff offers its per-line actions"
        );
        (
            app.patch_editor.clone().expect("the patch's editor"),
            app.patch_view.clone().expect("the Diff view"),
        )
    });

    focus(&editor, cx);
    assert_eq!(
        cx.update(|window, cx| view.read(cx).line_keys(window, cx)),
        Some(false),
        "the per-line controls show, to stage, with the text focused"
    );
    assert!(
        !takes_text_input(cx),
        "the diff's read-only text refuses text input"
    );

    find_field_takes_text_input(&app, &editor, "four", "the Diff view", cx).await;
    focus(&editor, cx);
    assert!(
        !takes_text_input(cx),
        "the diff's text, focused again, still refuses text input"
    );
}

/// Compare's text refuses text input in each of its modes, the Diff view, the
/// After side of Split, Before and After, and Find's field opened on each
/// takes it.
#[gpui::test]
async fn compares_text_refuses_text_input_and_its_find_field_takes_it(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
    bind_chord(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
    settle(&app, cx).await;
    for (mode, name) in [
        (TextMode::Unified, "Diff"),
        (TextMode::Split, "Split"),
        (TextMode::Before, "Before"),
        (TextMode::After, "After"),
    ] {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.text_mode = mode;
                app.ensure_editor(window, cx);
                cx.notify();
            })
        });
        settle(&app, cx).await;
        let editor = app.read_with(cx, |app, cx| match mode {
            TextMode::Unified => app.patch_editor.clone().unwrap(),
            TextMode::Before => app.before_editor.clone().unwrap(),
            TextMode::After => app.after_editor.clone().unwrap(),
            _ => app.split_view.as_ref().unwrap().read(cx).editors()[1].clone(),
        });
        focus(&editor, cx);
        assert!(
            !takes_text_input(cx),
            "{name}: Compare's read-only text refuses text input"
        );
        find_field_takes_text_input(&app, &editor, "line", name, cx).await;
    }
}

/// The commit title, made read-only while a write runs, refuses text input
/// then and takes it again in the frame that draws it editable once the
/// write ends.
#[gpui::test]
async fn the_commit_title_takes_text_input_again_once_a_write_ends(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window_from(cx, &history_script(1, 4)).await;
    bind_chord(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    let title = app.read_with(cx, |app, _| app.commit_title.clone());
    let focus = cx.update(|_, cx| title.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    assert!(takes_text_input(cx), "the commit title takes text input");

    app.update(cx, |app, cx| {
        app.operation_busy = Some("Testing");
        cx.notify();
    });
    assert!(
        cx.update(|window, _| focus.is_focused(window)),
        "the commit title keeps focus"
    );
    assert!(
        !takes_text_input(cx),
        "the commit title refuses text input while a write runs"
    );

    app.update(cx, |app, cx| {
        app.operation_busy = None;
        cx.notify();
    });
    assert!(
        takes_text_input(cx),
        "the commit title takes text input again once the write ends"
    );
    cx.simulate_input("Title");
    assert_eq!(
        title.read_with(cx, |title, _| title.value()),
        "Title",
        "the commit title takes the keys typed into it"
    );
}
