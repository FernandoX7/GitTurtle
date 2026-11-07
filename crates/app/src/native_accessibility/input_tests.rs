use crate::*;
use core::prelude::v1::test;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::component::input::{InputContentType, Textarea, TextareaState};
use std::{cell::RefCell, rc::Rc};

#[gpui::test]
fn input_semantics_follow_the_editing_focus_owner(cx: &mut TestAppContext) {
    type Nodes = Rc<RefCell<Vec<gpui::accesskit::Node>>>;
    struct Probe {
        input: Entity<InputState>,
        textarea: Entity<TextareaState>,
        nodes: Nodes,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let input = self.input.clone();
            let textarea = self.textarea.clone();
            let nodes = self.nodes.clone();
            div()
                .child(
                    Textarea::new(&self.textarea)
                        .aria_label("Review body")
                        .readonly(true),
                )
                .child(canvas(
                    move |_, window, cx| {
                        let mut captured = Vec::new();
                        for (label, disabled, readonly, secret) in [
                            ("Repository name", false, false, false),
                            ("Renamed field", true, false, false),
                            ("Captured value", false, true, false),
                            ("Fixture password", false, false, true),
                        ] {
                            let frame = RenderOnce::render(
                                Input::new(&input)
                                    .aria_label(label)
                                    .accessibility_id("fixture.input")
                                    .disabled(disabled)
                                    .readonly(readonly)
                                    .when(secret, |this| {
                                        this.content_type(InputContentType::Password)
                                    }),
                                window,
                                cx,
                            )
                            .into_element();
                            assert_eq!(frame.a11y_role(), None);
                            captured.push(input.update(cx, |state, cx| {
                                if !disabled {
                                    state.focus(window, cx);
                                    assert!(state.focus_handle(cx).is_focused(window));
                                }
                                let editor = Render::render(state, window, cx).into_element();
                                let mut node = gpui::accesskit::Node::new(
                                    editor
                                        .a11y_role()
                                        .expect("the editing focus owner has a role"),
                                );
                                editor.write_a11y_info(&mut node);
                                node
                            }));
                        }
                        // The real Textarea child projected metadata during layout.
                        captured.push(textarea.update(cx, |state, cx| {
                            state.focus(window, cx);
                            assert!(state.focus_handle(cx).is_focused(window));
                            let editor = Render::render(state, window, cx).into_element();
                            let mut node = gpui::accesskit::Node::new(
                                editor
                                    .a11y_role()
                                    .expect("multiline editing focus owner has a role"),
                            );
                            editor.write_a11y_info(&mut node);
                            node
                        }));
                        *nodes.borrow_mut() = captured;
                    },
                    |_, _, _, _| {},
                ))
        }
    }
    cx.update(gpui_kit::init);
    let nodes: Nodes = Default::default();
    let observed = nodes.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| Probe {
        input: cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Repository placeholder")
                .default_value("synthetic test value")
        }),
        textarea: cx.new(|cx| TextareaState::new(window, cx)),
        nodes,
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let nodes = observed.borrow();
    assert_eq!(nodes.len(), 5);
    for (node, label, role, editable) in [
        (&nodes[0], "Repository name", Role::TextInput, true),
        (&nodes[1], "Renamed field", Role::TextInput, false),
        (&nodes[2], "Captured value", Role::TextInput, false),
        (&nodes[3], "Fixture password", Role::PasswordInput, true),
        (&nodes[4], "Review body", Role::MultilineTextInput, false),
    ] {
        assert_eq!(node.role(), role);
        assert_eq!(node.label(), Some(label));
        assert!(node.supports_action(gpui::AccessibleAction::Focus));
        assert_eq!(
            node.supports_action(gpui::AccessibleAction::SetValue),
            editable
        );
        // The headless platform has no active assistive client. Rendering
        // ordinary or secret fields must not materialize their text value.
        assert_eq!(node.value(), None);
    }
    assert_eq!(nodes[0].author_id(), Some("fixture.input"));
    assert_eq!(nodes[0].placeholder(), Some("Repository placeholder"));
}

/// The tree of a window holding only a field's node, `node` as its editing
/// element writes it, with what that element writes beside it in a frame
/// drawn with an assistive technology active: the value of `text`
/// (`InputState::accessible_text(true)`), as its render's `aria_value`,
/// and its text runs and their selection, through the writer its render
/// hands GPUI's synthetic-child builder (`AccessibleText::write`), with
/// focus on it.
pub(crate) fn field_tree(
    mut node: gpui::accesskit::Node,
    text: Option<gpui_kit::base::input::AccessibleText>,
) -> gpui::accesskit::TreeUpdate {
    use gpui::accesskit::{Node, NodeId, Tree, TreeId, TreeUpdate};
    use gpui_kit::base::input::AccessibleTextSink;
    /// The field's node and its runs, as GPUI's builder holds them.
    struct Sink {
        field: Node,
        runs: Vec<(NodeId, Node)>,
    }
    impl AccessibleTextSink for Sink {
        // A run's ID from its key, never the root's or the field's.
        fn run_id(&self, key: usize) -> NodeId {
            use std::hash::{Hash as _, Hasher as _};
            let mut hasher = std::hash::DefaultHasher::new();
            key.hash(&mut hasher);
            NodeId((hasher.finish() >> 2) + 2)
        }
        fn push_run(&mut self, id: NodeId, run: Node) {
            self.runs.push((id, run));
        }
        fn field(&mut self) -> &mut Node {
            &mut self.field
        }
    }
    let field = NodeId(1);
    if let Some(text) = &text {
        node.set_value(text.value().to_string());
    }
    let mut sink = Sink {
        field: node,
        runs: Vec::new(),
    };
    if let Some(text) = &text {
        text.write(&mut sink);
    }
    let Sink {
        field: mut node,
        runs,
    } = sink;
    node.set_children(runs.iter().map(|(id, _)| *id).collect::<Vec<_>>());
    let mut root = Node::new(Role::Window);
    root.set_children(vec![field]);
    let mut nodes = vec![(NodeId(0), root), (field, node)];
    nodes.extend(runs);
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(NodeId(0))),
        tree_id: TreeId::ROOT,
        focus: field,
    }
}

/// Every text field reaches assistive technology with its value: a
/// single-line field, an empty one, a multi-line field and the code editor
/// each write text runs holding their text, which is what AccessKit's
/// adapters read it through (the AT-SPI adapter gives a field the Text
/// interface only with them), with the caret on them; a password writes
/// none. Settings' Name field holding "QA Fixture" wrote none, and Orca
/// read it by its placeholder, "Your name" (QA-106b S4).
#[gpui::test]
fn every_text_field_writes_its_text_as_runs(cx: &mut TestAppContext) {
    use gpui_kit::component::input::{Editor, EditorState};
    struct Probe {
        name: Entity<InputState>,
        email: Entity<InputState>,
        token: Entity<InputState>,
        message: Entity<TextareaState>,
        source: Entity<EditorState>,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(Input::new(&self.name).aria_label("Name"))
                .child(Input::new(&self.email).aria_label("Email"))
                .child(
                    Input::new(&self.token)
                        .aria_label("Token")
                        .content_type(InputContentType::Password),
                )
                .child(Textarea::new(&self.message).aria_label("Message"))
                .child(
                    Editor::new(&self.source)
                        .aria_label("File text")
                        .h(px(120.)),
                )
        }
    }
    cx.update(gpui_kit::init);
    let (probe, cx) = cx.add_window_view(|window, cx| Probe {
        name: cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Your name")
                .default_value("QA Fixture")
        }),
        email: cx.new(|cx| InputState::new(window, cx).placeholder("you@example.com")),
        token: cx.new(|cx| InputState::new(window, cx).default_value("hunter2")),
        message: cx.new(|cx| TextareaState::new(window, cx).default_value("Subject\n\nBody")),
        source: cx.new(|cx| EditorState::new(window, cx).default_value("fn main() {}\n")),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let fields = cx.read(|cx| {
        let probe = probe.read(cx);
        (
            probe.name.clone(),
            probe.email.clone(),
            probe.token.clone(),
            probe.message.clone(),
            probe.source.clone(),
        )
    });

    /// What the field's node gives every adapter, read through AccessKit's
    /// consumer: whether it has text runs, the text they hold, how many
    /// runs it has and the caret's character offset in that text.
    fn read(update: gpui::accesskit::TreeUpdate) -> (bool, String, usize, Option<usize>) {
        let tree = accesskit_consumer::Tree::new(update, true);
        let state = tree.state();
        let field = state.focus().expect("the field holds focus");
        if !field.supports_text_ranges() {
            return (false, String::new(), 0, None);
        }
        let caret = field
            .text_selection_focus()
            .map(|focus| focus.to_global_usv_index());
        (
            true,
            field.document_range().text(),
            field.children().count(),
            caret,
        )
    }
    macro_rules! tree {
        ($field:expr) => {{
            let node = crate::views::tests::input_node(cx, &$field);
            let text = cx.update(|_, cx| $field.update(cx, |state, _| state.accessible_text(true)));
            field_tree(node, text)
        }};
    }

    let (runs, text, count, caret) = read(tree!(fields.0));
    assert!(runs, "the Name field writes text runs");
    let value = cx.update(|_, cx| fields.0.update(cx, |state, _| state.accessible_text(true)));
    assert_eq!(
        value.map(|text| text.value()).as_deref(),
        Some("QA Fixture")
    );
    let off = cx.update(|_, cx| fields.0.update(cx, |state, _| state.accessible_text(false)));
    assert!(off.is_none(), "a frame drawn with no client writes no text");
    assert_eq!(text, "QA Fixture");
    assert_eq!(count, 1, "one line, one run");
    assert_eq!(
        caret,
        Some(0),
        "the caret where the value set it, at its start"
    );
    let name = crate::views::tests::input_node(cx, &fields.0);
    assert_eq!(name.label(), Some("Name"));
    assert_eq!(name.placeholder(), Some("Your name"));

    let (runs, text, count, caret) = read(tree!(fields.1));
    assert!(runs, "an empty field writes its one, empty, run");
    assert_eq!((text.as_str(), count, caret), ("", 1, Some(0)));

    let (runs, ..) = read(tree!(fields.2));
    assert!(!runs, "a password writes no text");
    let token = cx.update(|_, cx| fields.2.update(cx, |state, _| state.accessible_text(true)));
    assert!(token.is_none(), "a password exposes no value");

    let (runs, text, count, _) = read(tree!(fields.3));
    assert!(runs, "the multi-line field writes text runs");
    assert_eq!(
        (text.as_str(), count),
        ("Subject\n\nBody", 3),
        "a run a line"
    );

    let (runs, text, count, _) = read(tree!(fields.4));
    assert!(runs, "the code editor writes text runs");
    assert_eq!(text, "fn main() {}\n");
    assert_eq!(count, 2, "its line, and the empty line after its break");

    // Typing reaches the runs, the caret after what was typed.
    cx.update(|window, cx| fields.0.update(cx, |state, cx| state.focus(window, cx)));
    cx.simulate_keystrokes("end");
    cx.simulate_input(" Two");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (_, text, _, caret) = read(tree!(fields.0));
    assert_eq!(text, "QA Fixture Two");
    assert_eq!(caret, Some("QA Fixture Two".len()));
}

/// A line's words read as themselves past its 256th character. A field's
/// line of 300 characters, 60 words "w00" to "w59" each with two spaces
/// after it, with the caret at 262, inside "w52": the word there is "w52"
/// and its spaces, the next word starts after them, and the line is the
/// whole line. Written as one run, its word starts could be listed only
/// among its first 256 characters, and AccessKit's consumer looks a
/// position up as a `u8`, so the word at 262 read as "w01  ", 256 before
/// it, and the next word from 262 went back to 10.
#[gpui::test]
fn a_long_lines_words_read_as_themselves_past_its_256th_character(cx: &mut TestAppContext) {
    struct Probe {
        field: Entity<InputState>,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(Input::new(&self.field).aria_label("Words"))
        }
    }
    let line: String = (0..60).map(|word| format!("w{word:02}  ")).collect();
    assert_eq!(line.len(), 300);
    cx.update(gpui_kit::init);
    let value = line.clone();
    let (probe, cx) = cx.add_window_view(move |window, cx| Probe {
        field: cx.new(|cx| InputState::new(window, cx).default_value(value)),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let field = cx.read(|cx| probe.read(cx).field.clone());
    cx.update(|_, cx| field.update(cx, |state, cx| state.set_selected_range(262..262, cx)));
    let node = crate::views::tests::input_node(cx, &field);
    let text = cx.update(|_, cx| field.update(cx, |state, _| state.accessible_text(true)));
    let tree = accesskit_consumer::Tree::new(field_tree(node, text), true);
    let state = tree.state();
    let focused = state.focus().expect("the field holds focus");
    let caret = focused
        .text_selection_focus()
        .expect("the field holds the caret");
    assert_eq!(caret.to_global_usv_index(), 262);
    // As the AT-SPI adapter reads the word and the line at an offset
    // (`accesskit_atspi_common::util::text_range_from_offset`).
    let span = |start: accesskit_consumer::TextPosition<'_>,
                end: accesskit_consumer::TextPosition<'_>| {
        let mut range = start.to_degenerate_range();
        range.set_end(end);
        (
            range.text(),
            start.to_global_usv_index(),
            end.to_global_usv_index(),
        )
    };
    let word = if caret.is_word_start() {
        caret
    } else {
        caret.backward_to_word_start()
    };
    assert_eq!(
        span(word, word.forward_to_word_end()),
        ("w52  ".to_owned(), 260, 265)
    );
    assert_eq!(caret.forward_to_word_start().to_global_usv_index(), 265);
    let start = if caret.is_line_start() {
        caret
    } else {
        caret.backward_to_line_start()
    };
    assert_eq!(span(start, start.forward_to_line_end()), (line, 0, 300));
}

/// A selection from the end of a long line that ends the text reads as
/// the field wrote it. End, then Shift+Home, in a single-line field of
/// 130,561 characters puts the caret at 0, whose window of runs holds the
/// first 130,560, and the selection's anchor at the end, past it, with no
/// line after it: the anchor named a run the frame never wrote, and
/// AccessKit's consumer panicked reading the selection, as the AT-SPI
/// adapter reads it on the UI thread for Orca (macOS's adapter takes the
/// same unwrap). The field of 130,560, its window whole, reads alike.
#[gpui::test]
fn a_selection_from_the_end_of_a_long_last_line_reads_as_written(cx: &mut TestAppContext) {
    struct Probe {
        field: Entity<InputState>,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(Input::new(&self.field).aria_label("Minified"))
        }
    }
    cx.update(gpui_kit::init);
    let (probe, cx) = cx.add_window_view(|window, cx| Probe {
        field: cx.new(|cx| InputState::new(window, cx)),
    });
    let field = cx.read(|cx| probe.read(cx).field.clone());
    for length in [130_560, 130_561] {
        cx.update(|window, cx| {
            field.update(cx, |state, cx| {
                state.set_value("a".repeat(length), window, cx);
                state.focus(window, cx);
            })
        });
        cx.simulate_keystrokes("end shift-home");
        let node = crate::views::tests::input_node(cx, &field);
        let text = cx.update(|_, cx| field.update(cx, |state, _| state.accessible_text(true)));
        let tree = accesskit_consumer::Tree::new(field_tree(node, text), true);
        let state = tree.state();
        let focused = state.focus().expect("the field holds focus");
        let selection = focused
            .text_selection()
            .expect("the field holds its selection");
        let caret = focused
            .text_selection_focus()
            .expect("the field holds the caret");
        assert_eq!(caret.to_global_usv_index(), 0, "{length}");
        assert_eq!(
            (
                selection.start().to_global_usv_index(),
                selection.end().to_global_usv_index()
            ),
            (0, length),
            "the whole line selected"
        );
    }
}

/// A frame drawn again with no change builds nothing again, nor does one
/// whose caret moved inside its window of runs: a field segments its text
/// only when the text changed, and builds its window's run nodes only then
/// or when the caret left the window. Each frame here is what the field's
/// render writes with a client active (`accessible_text(true)` and
/// `AccessibleText::write`), in a multi-line field of 1,000 lines.
#[gpui::test]
fn a_frame_drawn_again_with_no_change_builds_nothing_again(cx: &mut TestAppContext) {
    use gpui_kit::base::input::{AccessibleText, Builds};
    struct Probe {
        field: Entity<TextareaState>,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(Textarea::new(&self.field).aria_label("Lines"))
        }
    }
    let lines: String = (0..1_000).map(|line| format!("line {line:04}\n")).collect();
    cx.update(gpui_kit::init);
    let (probe, cx) = cx.add_window_view(move |window, cx| Probe {
        field: cx.new(|cx| TextareaState::new(window, cx).default_value(lines)),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let field = cx.read(|cx| probe.read(cx).field.clone());
    let builds = |segmented, runs| Builds { segmented, runs };
    let at = |line: usize| line * "line 0000\n".len() + 3;
    // The frames and changes run in one update: a test window has no
    // client, so a frame it drew between them would drop what the field
    // kept, as a field's render does with none.
    cx.update(|window, cx| {
        field.update(cx, |state, cx| {
            let frame = |state: &mut TextareaState| {
                let text = state
                    .accessible_text(true)
                    .expect("the field writes its text");
                let value = text.value();
                field_tree(
                    gpui::accesskit::Node::new(Role::MultilineTextInput),
                    Some(text),
                );
                (AccessibleText::builds(), value)
            };
            // Drop what the field kept, and count from none.
            state.accessible_text(false);
            AccessibleText::reset_builds();
            assert_eq!(frame(state).0, builds(1, 1), "the first frame builds both");
            assert_eq!(
                frame(state).0,
                builds(1, 1),
                "drawn again, it builds nothing"
            );
            state.set_selected_range(at(200)..at(200), cx);
            assert_eq!(
                frame(state).0,
                builds(1, 1),
                "the caret moved in its window"
            );
            state.set_selected_range(at(800)..at(800), cx);
            assert_eq!(frame(state).0, builds(1, 2), "the caret left its window");
            gpui::EntityInputHandler::replace_text_in_range(state, None, "x", window, cx);
            let (counted, value) = frame(state);
            assert_eq!(counted, builds(2, 3), "a keystroke changes the text");
            assert!(
                value.contains("linxe 0800"),
                "the frame holds the keystroke"
            );
            assert_eq!(frame(state).0, builds(2, 3));
        })
    });
}

/// A single-line and a multi-line field carry the accessible description
/// they are given on the node their editing element writes, where every
/// adapter reads it (AT-SPI's description), so the helper text drawn beside
/// a field can be read with it; a field given none has none.
#[gpui::test]
fn a_field_carries_its_accessible_description(cx: &mut TestAppContext) {
    struct Probe {
        described: Entity<InputState>,
        plain: Entity<InputState>,
        message: Entity<TextareaState>,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(
                    Input::new(&self.described)
                        .aria_label("Name")
                        .aria_description("Used for new commits"),
                )
                .child(Input::new(&self.plain).aria_label("Email"))
                .child(
                    Textarea::new(&self.message)
                        .aria_label("Commit message")
                        .aria_description("The first line is the subject"),
                )
        }
    }
    cx.update(gpui_kit::init);
    let (probe, cx) = cx.add_window_view(|window, cx| Probe {
        described: cx.new(|cx| InputState::new(window, cx)),
        plain: cx.new(|cx| InputState::new(window, cx)),
        message: cx.new(|cx| TextareaState::new(window, cx)),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (described, plain, message) = cx.read(|cx| {
        let probe = probe.read(cx);
        (
            probe.described.clone(),
            probe.plain.clone(),
            probe.message.clone(),
        )
    });
    let described = crate::views::tests::input_node(cx, &described);
    assert_eq!(described.label(), Some("Name"));
    assert_eq!(described.description(), Some("Used for new commits"));
    let plain = crate::views::tests::input_node(cx, &plain);
    assert_eq!(plain.description(), None);
    let message = crate::views::tests::input_node(cx, &message);
    assert_eq!(message.role(), Role::MultilineTextInput);
    assert_eq!(message.description(), Some("The first line is the subject"));
}
