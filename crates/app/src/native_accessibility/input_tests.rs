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
