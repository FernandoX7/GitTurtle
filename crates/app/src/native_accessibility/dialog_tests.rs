use crate::*;
use core::prelude::v1::test;
use gpui_kit::base::{Dialog as BaseDialog, DialogTitle, FocusTrapElement};
use std::{cell::RefCell, rc::Rc};

type Nodes = Rc<RefCell<Vec<gpui::accesskit::Node>>>;
struct Observe<E> {
    inner: E,
    nodes: Nodes,
}
impl<E: Element> IntoElement for Observe<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl<E: Element> Element for Observe<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = self
            .inner
            .prepaint(id, inspector, bounds, layout, window, cx);
        let mut node = gpui::accesskit::Node::new(self.inner.a11y_role().unwrap());
        self.inner.write_a11y_info(&mut node);
        self.nodes.borrow_mut().push(node);
        state
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .paint(id, inspector, bounds, layout, state, window, cx);
    }
}

#[gpui::test]
fn dialog_titles_follow_nested_layout_scopes_without_stale_names(cx: &mut TestAppContext) {
    struct Titles {
        nodes: Nodes,
    }
    impl Render for Titles {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.nodes.borrow_mut().clear();
            let inner = div()
                .id("inner")
                .role(Role::AlertDialog)
                .focus_trap("inner-trap", &cx.focus_handle())
                .child(DialogTitle::new().child("Inner confirmation"));
            let outer = div()
                .id("outer")
                .role(Role::Dialog)
                .focus_trap("outer-trap", &cx.focus_handle())
                .child(Observe {
                    inner,
                    nodes: self.nodes.clone(),
                })
                .child(
                    DialogTitle::new()
                        .child(String::from("Captured local document · docs/companion.md")),
                );
            let shared = div()
                .id("shared")
                .role(Role::Dialog)
                .focus_trap("shared-trap", &cx.focus_handle())
                .child(DialogTitle::new().child(SharedString::from("After · Page 20 text")));
            let rich = div()
                .id("rich")
                .role(Role::Dialog)
                .focus_trap("rich-trap", &cx.focus_handle())
                .child(DialogTitle::new().child(div().child("Custom rich title")));
            div().children([
                Observe {
                    inner: outer,
                    nodes: self.nodes.clone(),
                }
                .into_any_element(),
                Observe {
                    inner: shared,
                    nodes: self.nodes.clone(),
                }
                .into_any_element(),
                Observe {
                    inner: rich,
                    nodes: self.nodes.clone(),
                }
                .into_any_element(),
            ])
        }
    }
    cx.update(gpui_kit::base::init);
    let nodes: Nodes = Default::default();
    let observed = nodes.clone();
    let (_, cx) = cx.add_window_view(move |_, _| Titles { nodes });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let nodes = observed.borrow();
    let labels: Vec<_> = nodes.iter().map(|node| node.label().unwrap()).collect();
    assert_eq!(
        labels,
        [
            "Inner confirmation",
            "Captured local document · docs/companion.md",
            "After · Page 20 text",
            "Dialog"
        ]
    );
    assert!(nodes.iter().all(|node| node.is_modal()));
}

#[gpui::test]
fn focused_input_escape_reaches_dialog_and_respects_disabled_keyboard(cx: &mut TestAppContext) {
    struct InputDialog {
        input: Entity<InputState>,
        focus: FocusHandle,
        closes: Rc<RefCell<usize>>,
        keyboard: bool,
    }
    impl Render for InputDialog {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let closes = self.closes.clone();
            BaseDialog::new(cx)
                .focus_handle(self.focus.clone())
                .close_on_escape(self.keyboard)
                .on_cancel(move |_, _, _| {
                    *closes.borrow_mut() += 1;
                    true
                })
                .popup(div().w(px(400.)).h(px(100.)).child(Input::new(&self.input)))
        }
    }
    cx.update(gpui_kit::init);
    let closes = Rc::new(RefCell::new(0));
    let observed = closes.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| InputDialog {
        input: cx.new(|cx| InputState::new(window, cx).default_value("invalid page")),
        focus: cx.focus_handle(),
        closes,
        keyboard: true,
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let focus = view.update(cx, |view, cx| view.input.read(cx).focus_handle(cx));
    cx.update(|window, cx| focus.focus(window, cx));
    cx.simulate_keystrokes("escape");
    assert_eq!(*observed.borrow(), 1);
    view.update(cx, |view, cx| {
        view.keyboard = false;
        cx.notify();
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("escape");
    assert_eq!(*observed.borrow(), 1);
}

#[gpui::test]
fn focused_reader_closes_find_before_the_dialog(cx: &mut TestAppContext) {
    struct ReaderDialog {
        editor: Entity<EditorState>,
        focus: FocusHandle,
        closes: Rc<RefCell<usize>>,
    }
    impl Render for ReaderDialog {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let closes = self.closes.clone();
            BaseDialog::new(cx)
                .focus_handle(self.focus.clone())
                .on_cancel(move |_, _, _| {
                    *closes.borrow_mut() += 1;
                    true
                })
                .popup(
                    div().w(px(500.)).h(px(300.)).child(
                        editor_find::Editor::new(&self.editor)
                            .readonly(true)
                            .h_full(),
                    ),
                )
        }
    }
    cx.update(gpui_kit::init);
    let closes = Rc::new(RefCell::new(0));
    let observed = closes.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| ReaderDialog {
        editor: text::editor("AFTER - Page 20 of 24", "text", None, window, cx),
        focus: cx.focus_handle(),
        closes,
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let focus = view.update(cx, |view, cx| view.editor.read(cx).focus_handle(cx));
    cx.update(|window, cx| focus.focus(window, cx));
    #[cfg(target_os = "macos")]
    cx.simulate_keystrokes("cmd-f");
    #[cfg(not(target_os = "macos"))]
    cx.simulate_keystrokes("ctrl-f");
    cx.simulate_keystrokes("escape");
    assert_eq!(*observed.borrow(), 0, "first Escape closes Find");
    cx.simulate_keystrokes("escape");
    assert_eq!(*observed.borrow(), 1, "second Escape closes the dialog");
}
