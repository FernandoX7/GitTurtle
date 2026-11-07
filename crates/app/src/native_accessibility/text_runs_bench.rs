//! Opt-in release benchmark of what a text field writes to assistive
//! technology in a frame ([`text_runs_release_benchmark`]): the text runs
//! AccessKit's AT-SPI adapter reads a field's value through, from
//! `InputState::accessible_text` to the return of the adapter's
//! `Adapter::update`, for a short field, two multi-line fields and three
//! code-editor texts, in a frame drawn again, after a caret move, a caret
//! move across a block of runs, a keystroke and a line break, and in the
//! first frame, and the same states drawn with no screen reader.

use super::atspi_state_tests::IgnoreCallbacks;
use super::input_tests::field_tree;
use accesskit_atspi_common::{Adapter, AppContext, WindowBounds};
use gpui_kit::base::input::InputAccessibility;
use gpui_kit::component::input::{EditorState, InputState, TextareaState};
use gpui_kit::gpui::accesskit::{Node, Role};
use gpui_kit::gpui::{
    self, AppContext as _, Entity, EntityInputHandler as _, TestAppContext, VisualTestContext,
};
use std::time::Instant;

/// A field's text, built once per fixture, untimed.
struct Fixture {
    name: &'static str,
    role: Role,
    text: String,
    /// Two caret bytes in one window of runs, and one in another window
    /// (the text's end), where the text has one.
    near: [usize; 2],
    far: Option<usize>,
}

fn fixtures() -> Vec<Fixture> {
    let lines = |count: usize, width: usize| -> String {
        (0..count)
            .map(|line| {
                let words = format!("line {line} ").repeat(width / 7 + 1);
                format!("{}\n", &words[..width - 1])
            })
            .collect()
    };
    let input = "qa-fixture@example.invalid".to_owned();
    let textarea = lines(40, 50);
    let textarea_64k = lines(1_000, 64);
    let editor = lines(100_000, 50);
    let wide = lines(4, 256 * 1024);
    let one_line = "abcd ".repeat(1024 * 1024);
    let middle = |text: &str| {
        let mut at = text.len() / 2;
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let fixture = |name, role, text: String, far: bool| {
        let at = middle(&text).min(text.len().saturating_sub(1));
        let far = far.then_some(text.len());
        Fixture {
            name,
            role,
            near: [at, at + 1],
            far,
            text,
        }
    };
    vec![
        Fixture {
            near: [input.len() - 1, input.len()],
            ..fixture("input_26b", Role::TextInput, input, false)
        },
        fixture(
            "textarea_2kb_40_lines",
            Role::MultilineTextInput,
            textarea,
            false,
        ),
        fixture(
            "textarea_64kb",
            Role::MultilineTextInput,
            textarea_64k,
            true,
        ),
        fixture(
            "editor_5mb_100k_lines",
            Role::MultilineTextInput,
            editor,
            true,
        ),
        fixture(
            "editor_4_lines_of_256kb",
            Role::MultilineTextInput,
            wide,
            true,
        ),
        fixture(
            "editor_one_5mb_line",
            Role::MultilineTextInput,
            one_line,
            true,
        ),
    ]
}

/// A field state of the fixture's kind holding its text, presented as the
/// styled input presents it (`Input::render`), with no frame drawn.
enum Field {
    Input(Entity<InputState>),
    Textarea(Entity<TextareaState>),
    Editor(Entity<EditorState>),
}

macro_rules! on_field {
    ($field:expr, $state:ident => $body:expr) => {
        match $field {
            Field::Input($state) => $body,
            Field::Textarea($state) => $body,
            Field::Editor($state) => $body,
        }
    };
}

fn field(fixture: &Fixture, cx: &mut VisualTestContext) -> Field {
    let text = fixture.text.clone();
    let presentation = InputAccessibility {
        role: Some(fixture.role),
        label: Some(fixture.name.into()),
        placeholder: None,
        author_id: None,
        expose_value: true,
        description: None,
    };
    cx.update(|window, cx| {
        let field = match fixture.name {
            name if name.starts_with("input") => {
                Field::Input(cx.new(|cx| InputState::new(window, cx).default_value(text)))
            }
            name if name.starts_with("textarea") => {
                Field::Textarea(cx.new(|cx| TextareaState::new(window, cx).default_value(text)))
            }
            _ => Field::Editor(cx.new(|cx| EditorState::new(window, cx).default_value(text))),
        };
        on_field!(&field, state => state.update(cx, |state, cx| {
            state.set_accessibility_presentation(presentation, cx)
        }));
        field
    })
}

/// One frame's work with a screen reader (`on`): the field's text, its
/// runs written into the field's node and the window's tree, and the AT-SPI
/// adapter's update; or with none, only what render asks of the field.
fn frame(field: &Field, role: Role, adapter: &mut Adapter, on: bool, cx: &mut VisualTestContext) {
    let text = cx.update(
        |_, cx| on_field!(field, state => state.update(cx, |state, _| state.accessible_text(on))),
    );
    if on {
        let mut node = Node::new(role);
        node.set_label("Field");
        adapter.update(field_tree(node, text));
    }
}

/// The change a case makes before its timed frame, untimed: none, a caret
/// moved between two bytes in one window or between windows, a character
/// or a line break typed at the caret, or what the field kept dropped.
fn change(case: &str, fixture: &Fixture, field: &Field, sample: usize, cx: &mut VisualTestContext) {
    let caret = |at: usize, cx: &mut VisualTestContext| {
        cx.update(|_, cx| on_field!(field, state => state.update(cx, |state, cx| state.set_selected_range(at..at, cx))));
    };
    let typed = |typed: &str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| on_field!(field, state => state.update(cx, |state, cx| state.replace_text_in_range(None, typed, window, cx))));
    };
    match case {
        "redraw" => {}
        "caret" => caret(fixture.near[sample % 2], cx),
        "caret_across_a_block" => caret(
            if sample.is_multiple_of(2) {
                fixture.far.unwrap()
            } else {
                fixture.near[0]
            },
            cx,
        ),
        "keystroke" => typed("x", cx),
        "newline" => typed("\n", cx),
        "first" => {
            cx.update(|_, cx| on_field!(field, state => state.update(cx, |state, _| { state.accessible_text(false); })));
        }
        _ => unreachable!("{case}"),
    }
}

/// Each sample is one frame's work (`frame`) after its case's change, in
/// `us`, for each fixture and case, with a screen reader (`on`) and without
/// (`off`). Fixture texts, field states, the change before each sample and
/// every warm-up are untimed; no frame is drawn. Samples per series come
/// from `GITTURTLE_A11Y_TEXT_BENCH_SAMPLES` (200 unless set); a debug build
/// runs only as a check of the driver, at ten or fewer.
#[gpui::test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_A11Y_TEXT_BENCH_OUTPUT and run with --release --ignored"]
fn text_runs_release_benchmark(cx: &mut TestAppContext) {
    let output = std::env::var_os("GITTURTLE_A11Y_TEXT_BENCH_OUTPUT")
        .expect("the runner supplies the result destination");
    let samples: usize = std::env::var("GITTURTLE_A11Y_TEXT_BENCH_SAMPLES")
        .map(|count| count.parse().expect("a whole number of samples"))
        .unwrap_or(200);
    let release = !std::hint::black_box(cfg!(debug_assertions));
    assert!(samples > 0, "at least one sample");
    assert!(
        release || samples <= 10,
        "performance evidence requires --release; a debug build only checks the driver, at \
         ten samples or fewer"
    );
    let warmups = (samples / 10).max(1);
    cx.update(gpui_kit::init);
    let cx = cx.add_empty_window();
    let mut series = Vec::new();
    let mut described = Vec::new();
    for fixture in fixtures() {
        let field = field(&fixture, cx);
        described.push(serde_json::json!({
            "name": fixture.name,
            "bytes": fixture.text.len(),
            "lines": fixture.text.matches('\n').count() + 1,
        }));
        let initial = {
            let text = cx.update(|_, cx| on_field!(&field, state => state.update(cx, |state, _| state.accessible_text(true))));
            field_tree(Node::new(fixture.role), text)
        };
        let mut adapter = Adapter::new(
            &AppContext::new(None),
            IgnoreCallbacks,
            initial,
            true,
            WindowBounds::default(),
            IgnoreCallbacks,
        );
        let multi_line = fixture.role == Role::MultilineTextInput;
        for case in [
            "first",
            "redraw",
            "caret",
            "caret_across_a_block",
            "keystroke",
            "newline",
        ] {
            if (case == "caret_across_a_block" && fixture.far.is_none())
                || (case == "newline" && !multi_line)
            {
                continue;
            }
            for on in [true, false] {
                let mut us = Vec::with_capacity(samples);
                for sample in 0..warmups + samples {
                    // A frame with a screen reader keeps what the field
                    // segmented; one without drops it, as the field's render
                    // does, so each series starts from its own state.
                    change(case, &fixture, &field, sample, cx);
                    let started = Instant::now();
                    frame(&field, fixture.role, &mut adapter, on, cx);
                    let elapsed = started.elapsed().as_secs_f64() * 1e6;
                    if sample >= warmups {
                        us.push(elapsed);
                    }
                }
                series.push(serde_json::json!({
                    "fixture": fixture.name,
                    "case": case,
                    "screen_reader": if on { "on" } else { "off" },
                    "us": us,
                }));
            }
        }
    }
    let result = serde_json::json!({
        "schema": 1,
        "profile": if release { "release" } else { "debug" },
        "samples": samples,
        "warmups": warmups,
        "fixtures": described,
        "boundary": "One frame's accessible-text work on the UI thread: `InputState::accessible_text(on)`, then with a screen reader the field's node, its runs and selection written through `AccessibleText::write`, the window's `TreeUpdate`, and `accesskit_atspi_common::Adapter::update` until it returns (callbacks ignored; no D-Bus). No layout, prepaint or paint. The change before each sample (`set_selected_range`, `replace_text_in_range` at the caret, or `accessible_text(false)` for `first`) is untimed. `off` is the same frame drawn with no screen reader: `accessible_text(false)` alone.",
        "series": series,
    });
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
