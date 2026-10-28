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
fn rendered_component_titles_unwrap_the_erased_alert_and_dialog_title(cx: &mut TestAppContext) {
    struct ComponentTitles {
        nodes: Nodes,
    }
    impl Render for ComponentTitles {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.nodes.borrow_mut().clear();
            // Component Dialog::title and AlertDialog::title retain AnyElement,
            // then build their styled DialogTitle with .child(title). Calling
            // .child with erased content adds an additional AnyElement layer.
            let titles = [
                "Command palette".into_any_element(),
                String::from("GitHub pull requests").into_any_element(),
                SharedString::from("After · Page 20 text").into_any_element(),
            ];
            div().children(titles.into_iter().enumerate().map(|(index, title)| {
                Observe {
                    inner: div()
                        .id(("component-modal", index))
                        .role(Role::AlertDialog)
                        .focus_trap(("component-modal-trap", index), &cx.focus_handle())
                        .child(gpui_kit::component::dialog::DialogTitle::new().child(title)),
                    nodes: self.nodes.clone(),
                }
            }))
        }
    }
    cx.update(gpui_kit::init);
    let nodes: Nodes = Default::default();
    let observed = nodes.clone();
    let (_, cx) = cx.add_window_view(move |_, _| ComponentTitles { nodes });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let nodes = observed.borrow();
    assert_eq!(
        nodes.iter().map(|node| node.label()).collect::<Vec<_>>(),
        [
            Some("Command palette"),
            Some("GitHub pull requests"),
            Some("After · Page 20 text")
        ]
    );
    assert!(
        nodes
            .iter()
            .all(|node| node.role() == Role::AlertDialog && node.is_modal())
    );
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

/// A styled title that names itself, as the Reflog's, the rebase form's and
/// the GitHub panel's do (a `div` with an ID, a role and an accessible
/// name, styled), names its dialog through the component Dialog's erased
/// title, as plain text does. Each was announced by the fallback "Alert
/// dialog".
#[gpui::test]
fn self_named_titles_name_their_dialog(cx: &mut TestAppContext) {
    struct Titles {
        nodes: Nodes,
    }
    impl Render for Titles {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.nodes.borrow_mut().clear();
            let titles = [
                div()
                    .id("reflog-title")
                    .role(Role::Label)
                    .aria_label("Local reflog and recovery")
                    .child("Local reflog and recovery")
                    .into_any_element(),
                div()
                    .id("interactive-rebase-title")
                    .role(Role::Label)
                    .aria_label("Edit local commits")
                    .mb(px(-2.))
                    .child("Edit local commits")
                    .into_any_element(),
                // Neither readable text nor a name: the fallback stays.
                div().child("Custom rich title").into_any_element(),
            ];
            div().children(titles.into_iter().enumerate().map(|(index, title)| {
                Observe {
                    inner: div()
                        .id(("self-named-modal", index))
                        .role(Role::AlertDialog)
                        .focus_trap(("self-named-trap", index), &cx.focus_handle())
                        .child(gpui_kit::component::dialog::DialogTitle::new().child(title)),
                    nodes: self.nodes.clone(),
                }
            }))
        }
    }
    cx.update(gpui_kit::init);
    let nodes: Nodes = Default::default();
    let observed = nodes.clone();
    let (_, cx) = cx.add_window_view(move |_, _| Titles { nodes });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let nodes = observed.borrow();
    assert_eq!(
        nodes.iter().map(|node| node.label()).collect::<Vec<_>>(),
        [
            Some("Local reflog and recovery"),
            Some("Edit local commits"),
            Some("Alert dialog")
        ]
    );
    assert!(
        nodes
            .iter()
            .all(|node| node.role() == Role::AlertDialog && node.is_modal())
    );
}

/// Every dialog the repository window opens is a modal dialog named by its
/// title, never by the toolkit's fallback "Alert dialog", which announced
/// the Reflog and the interactive rebase form, their styled titles unread.
/// Only a confirmation that interrupts with a warning, such as a reviewed
/// write's last step, is an alert dialog: every dialog was one, so Orca
/// announced the Reflog, a picker or a sheet as an alert. Each is read from
/// the node its focus trap was laid out to write, which the toolkit records
/// in debug builds: GPUI writes the window's tree only while an assistive
/// technology is active.
#[cfg(debug_assertions)]
#[gpui::test]
async fn every_repository_dialog_is_named_by_its_title(cx: &mut TestAppContext) {
    type Open = fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>);
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    crate::focus_ring::sweep::prepare(&app, cx);
    let dialogs: [(&str, Open); 15] = [
        ("Local reflog and recovery", |app, window, cx| {
            app.open_reflog_browser(window, cx);
        }),
        ("Edit local commits", |app, window, cx| {
            app.open_interactive_rebase(None, window, cx)
        }),
        ("Review rewritten series", |app, window, cx| {
            app.open_rewrite_review(window, cx)
        }),
        ("Command palette", |app, window, cx| {
            app.open_command_palette(window, cx)
        }),
        ("Quick Open File", |app, window, cx| {
            app.open_quick_file(window, cx)
        }),
        ("Keyboard Shortcuts", |app, window, cx| {
            app.shortcut_help(window, cx)
        }),
        ("Tags", |app, window, cx| app.open_tags(window, cx)),
        ("Stashes", |app, window, cx| app.open_stashes(window, cx)),
        ("Worktrees", |app, window, cx| {
            app.open_worktree_manager(window, cx)
        }),
        ("GitHub pull requests", |app, window, cx| {
            app.open_github(window, cx)
        }),
        ("New theme", |app, window, cx| {
            app.open_theme_editor(None, window, cx)
        }),
        ("GitTurtle activity", |app, window, cx| {
            app.open_activity(window, cx)
        }),
        ("Git profiles", |app, window, cx| {
            app.open_profiles(window, cx)
        }),
        ("Compare local revisions", |app, window, cx| {
            app.open_revision_comparison(window, cx)
        }),
        ("Saved recovery drafts", |app, window, cx| {
            app.open_recovery_drafts(window, cx)
        }),
    ];
    for (title, open) in dialogs {
        crate::focus_ring::sweep::open_dialog(&app, cx, title, open).await;
        let nodes = cx.update(|window, cx| gpui_kit::base::focused_modal_nodes(window, cx));
        let [node] = nodes.as_slice() else {
            panic!("{title}: one modal holds focus, {nodes:?}");
        };
        // None of these interrupts with a warning: a dialog, not an alert.
        assert_eq!(node.role(), Role::Dialog, "{title}");
        assert!(node.is_modal(), "{title}");
        assert_eq!(node.label(), Some(title), "{title}");
        cx.update(|window, cx| window.close_dialog(cx));
        views::tests::draw(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "{title} closes"
        );
    }
    // A reviewed write's last step, shared by every Git write the window
    // confirms (Delete local tag, Drop saved stash, Abort operation…),
    // interrupts with its consequences: an alert dialog. Nothing runs, as
    // the review closes unconfirmed.
    let title = "Stage every file";
    crate::focus_ring::sweep::open_dialog(&app, cx, title, |app, window, cx| {
        app.confirm_git_write(
            title.into(),
            "Stage every working change.".into(),
            "Stage all",
            gitturtle_core::WriteCommand::StageAll,
            window,
            cx,
        )
    })
    .await;
    let nodes = cx.update(|window, cx| gpui_kit::base::focused_modal_nodes(window, cx));
    let [node] = nodes.as_slice() else {
        panic!("{title}: one modal holds focus, {nodes:?}");
    };
    assert_eq!(node.role(), Role::AlertDialog, "{title}");
    assert!(node.is_modal(), "{title}");
    assert_eq!(node.label(), Some(title), "{title}");
    cx.update(|window, cx| window.close_dialog(cx));
    views::tests::draw(cx);
}
