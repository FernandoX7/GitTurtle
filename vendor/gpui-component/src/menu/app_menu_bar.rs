use crate::{
    InteractiveElementExt as _, Selectable, Sizable,
    actions::{Cancel, SelectLeft, SelectRight},
    button::{Button, ButtonVariants},
    global_state::GlobalState,
    h_flex,
    menu::PopupMenu,
};
use gpui::{
    App, AppContext as _, ClickEvent, Context, DismissEvent, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, MouseButton, OwnedMenu, ParentElement,
    Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription, WeakEntity,
    Window, anchored, deferred, div, prelude::FluentBuilder,
};

const CONTEXT: &str = "AppMenuBar";
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
    ]);
}

/// The application menu bar, for Windows and Linux.
pub struct AppMenuBar {
    menus: Vec<Entity<AppMenu>>,
    selected_index: Option<usize>,
    action_context: Option<FocusHandle>,
}

impl AppMenuBar {
    /// Create a new app menu bar.
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let mut this = Self {
                selected_index: None,
                action_context: None,
                menus: Vec::new(),
            };
            this.reload(cx);
            this
        })
    }

    /// Reload the menus from the app.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let menu_bar = cx.weak_entity();
        let menus: Vec<OwnedMenu> = GlobalState::global(cx)
            .app_menus()
            .iter()
            .cloned()
            .collect();
        self.menus = menus
            .iter()
            .enumerate()
            .map(|(ix, menu)| AppMenu::new(ix, menu, menu_bar.clone(), cx))
            .collect();
        self.selected_index = None;
        self.action_context = None;
        cx.notify();
    }

    fn on_move_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_index) = self.selected_index else {
            return;
        };

        let new_ix = if selected_index == 0 {
            self.menus.len().saturating_sub(1)
        } else {
            selected_index.saturating_sub(1)
        };
        self.set_selected_index(Some(new_ix), window, cx);
    }

    fn on_move_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_index) = self.selected_index else {
            return;
        };

        let new_ix = if selected_index + 1 >= self.menus.len() {
            0
        } else {
            selected_index + 1
        };
        self.set_selected_index(Some(new_ix), window, cx);
    }

    fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.set_selected_index(None, window, cx);
    }

    fn set_selected_index(
        &mut self,
        ix: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_index.is_none() && ix.is_some() {
            self.action_context = window.focused(cx);
        } else if ix.is_none() {
            if let Some(action_context) = self.action_context.as_ref() {
                action_context.focus(window, cx);
            }
            self.action_context = None;
        }

        self.selected_index = ix;
        cx.notify();
    }

    #[inline]
    fn has_activated_menu(&self) -> bool {
        self.selected_index.is_some()
    }
}

impl Render for AppMenuBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("app-menu-bar")
            .role(Role::MenuBar)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_move_left))
            .on_action(cx.listener(Self::on_move_right))
            .on_action(cx.listener(Self::on_cancel))
            .size_full()
            .gap_x_1()
            .overflow_x_scroll()
            .lock_scroll_axis()
            .children(self.menus.clone())
    }
}

/// A menu in the menu bar.
pub(super) struct AppMenu {
    /// GitTurtle patch: the bar, held weakly. The bar holds its menus, and
    /// upstream's strong handle back made a pair no drop broke, so a bar
    /// taken away kept itself and every menu, with their popup menus, for
    /// the app's life (GITTURTLE-PATCH.md). Once the bar is gone a menu
    /// reads as closed and its input does nothing.
    menu_bar: WeakEntity<AppMenuBar>,
    ix: usize,
    name: SharedString,
    menu: OwnedMenu,
    popup_menu: Option<Entity<PopupMenu>>,

    _subscription: Option<Subscription>,
}

impl AppMenu {
    pub(super) fn new(
        ix: usize,
        menu: &OwnedMenu,
        menu_bar: WeakEntity<AppMenuBar>,
        cx: &mut App,
    ) -> Entity<Self> {
        let name = menu.name.clone();
        cx.new(|_| Self {
            ix,
            menu_bar,
            name,
            menu: menu.clone(),
            popup_menu: None,
            _subscription: None,
        })
    }

    fn is_selected(&self, cx: &App) -> bool {
        self.menu_bar
            .upgrade()
            .is_some_and(|menu_bar| menu_bar.read(cx).selected_index == Some(self.ix))
    }

    fn build_popup_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<PopupMenu> {
        let action_context = self
            .menu_bar
            .upgrade()
            .and_then(|menu_bar| menu_bar.read(cx).action_context.clone());
        let popup_menu = match self.popup_menu.as_ref() {
            None => {
                let items = self.menu.items.clone();
                let popup_menu = PopupMenu::build(window, cx, |menu, window, cx| {
                    menu.with_menu_items(items, window, cx)
                });
                popup_menu.update(cx, |menu, cx| {
                    menu.set_action_context(action_context.clone(), cx);
                });
                self._subscription =
                    Some(cx.subscribe_in(&popup_menu, window, Self::handle_dismiss));
                self.popup_menu = Some(popup_menu.clone());

                popup_menu
            }
            Some(menu) => {
                menu.update(cx, |menu, cx| {
                    menu.set_action_context(action_context.clone(), cx);
                });
                menu.clone()
            }
        };

        let focus_handle = popup_menu.read(cx).focus_handle(cx);
        if !focus_handle.contains_focused(window, cx) {
            focus_handle.focus(window, cx);
        }

        popup_menu
    }

    fn handle_dismiss(
        &mut self,
        _: &Entity<PopupMenu>,
        _: &DismissEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscription.take();
        self.popup_menu.take();
        _ = self.menu_bar.update(cx, |state, cx| {
            state.on_cancel(&Cancel, window, cx);
        });
    }

    fn handle_trigger_click(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, ClickEvent::Mouse(_)) {
            return;
        }

        self.toggle(window, cx);
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let is_selected = self.is_selected(cx);
        _ = self.menu_bar.update(cx, |state, cx| {
            let new_ix = if is_selected { None } else { Some(self.ix) };
            state.set_selected_index(new_ix, window, cx);
        });
    }

    fn handle_hover(&mut self, hovered: &bool, window: &mut Window, cx: &mut Context<Self>) {
        if !*hovered {
            return;
        }

        let has_activated_menu = self
            .menu_bar
            .upgrade()
            .is_some_and(|menu_bar| menu_bar.read(cx).has_activated_menu());
        if !has_activated_menu {
            return;
        }

        _ = self.menu_bar.update(cx, |state, cx| {
            state.set_selected_index(Some(self.ix), window, cx);
        });
    }
}

impl Render for AppMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_selected = self.is_selected(cx);

        div()
            .id(self.ix)
            .relative()
            .child(
                Button::new("menu")
                    .small()
                    .py_0p5()
                    .compact()
                    .ghost()
                    .label(self.name.clone())
                    .selected(is_selected)
                    .on_mouse_down(
                        MouseButton::Left,
                        window.listener_for(&cx.entity(), move |this, _, window, cx| {
                            // Stop propagation to avoid dragging the window.
                            window.prevent_default();
                            cx.stop_propagation();
                            this.toggle(window, cx);
                        }),
                    )
                    .on_click(cx.listener(Self::handle_trigger_click)),
            )
            .on_hover(cx.listener(Self::handle_hover))
            .when(is_selected, |this| {
                // GitTurtle patch: moved in from the window's side, the
                // menu's drawn edge, the ring outside its fill, stands the
                // popup margin in, as every menu's does
                // ([`crate::popup_fill_margin`]); 8 px to the fill left the
                // ring 7 px from it.
                this.child(deferred(
                    anchored()
                        .anchor(gpui::Anchor::TopLeft)
                        .snap_to_window_with_margin(crate::popup_fill_margin())
                        .child(
                            div()
                                .size_full()
                                .occlude()
                                .top_1()
                                .child(self.build_popup_menu(window, cx)),
                        ),
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use gpui::{
        Action, AnyWeakEntity, OwnedMenuItem, TestAppContext, VisualTestContext, actions, point, px,
    };

    actions!(app_menu_bar_test, [NewFile, OpenFile, CopyText]);

    struct TestRoot {
        menu_bar: Entity<AppMenuBar>,
        first_focus: FocusHandle,
        second_focus: FocusHandle,
    }

    impl Render for TestRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(div().id("first").track_focus(&self.first_focus))
                .child(div().id("second").track_focus(&self.second_focus))
                .child(self.menu_bar.clone())
        }
    }

    #[gpui::test]
    fn preserves_action_context_while_switching_menus(cx: &mut TestAppContext) {
        let (root, cx) = cx.add_window_view(|window, cx| {
            let first_focus = cx.focus_handle();
            let second_focus = cx.focus_handle();
            first_focus.focus(window, cx);

            TestRoot {
                menu_bar: cx.new(|_| AppMenuBar {
                    menus: Vec::new(),
                    selected_index: None,
                    action_context: None,
                }),
                first_focus,
                second_focus,
            }
        });

        let (menu_bar, first_focus, second_focus) = root.read_with(cx, |root, _| {
            (
                root.menu_bar.clone(),
                root.first_focus.clone(),
                root.second_focus.clone(),
            )
        });

        menu_bar.update_in(cx, |menu_bar, window, cx| {
            menu_bar.set_selected_index(Some(0), window, cx);
            assert_eq!(menu_bar.action_context.as_ref(), Some(&first_focus));

            second_focus.focus(window, cx);
            menu_bar.set_selected_index(Some(1), window, cx);
            assert_eq!(menu_bar.action_context.as_ref(), Some(&first_focus));

            menu_bar.set_selected_index(None, window, cx);
            assert!(menu_bar.action_context.is_none());
            assert_eq!(window.focused(cx).as_ref(), Some(&first_focus));

            second_focus.focus(window, cx);
            menu_bar.set_selected_index(Some(0), window, cx);
            assert_eq!(menu_bar.action_context.as_ref(), Some(&second_focus));
        });
    }

    /// A window holding `bar` while it is set, in a row 32 px tall.
    struct BarRoot {
        bar: Option<Entity<AppMenuBar>>,
    }

    impl Render for BarRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().h(px(32.)).w_full().children(self.bar.clone())
        }
    }

    fn menus() -> Vec<OwnedMenu> {
        let item = |name: &str, action: Box<dyn Action>| OwnedMenuItem::Action {
            name: name.into(),
            action,
            os_action: None,
            checked: false,
            disabled: false,
        };
        vec![
            OwnedMenu {
                name: "File".into(),
                items: vec![
                    item("New", Box::new(NewFile)),
                    item("Open", Box::new(OpenFile)),
                ],
                disabled: false,
            },
            OwnedMenu {
                name: "Edit".into(),
                items: vec![item("Copy", Box::new(CopyText))],
                disabled: false,
            },
        ]
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    const NEW: &str = "menu-item: New · MenuItem";
    const COPY: &str = "menu-item: Copy · MenuItem";

    /// Which of File's and Edit's menus is drawn.
    fn drawn(cx: &mut VisualTestContext) -> (bool, bool) {
        (
            cx.debug_bounds(NEW).is_some(),
            cx.debug_bounds(COPY).is_some(),
        )
    }

    /// GitTurtle patch: a bar's menu that runs past the window's right side
    /// is moved in until its drawn edge, the ring outside its fill, stands
    /// 8 px from that side, as every menu's does. With 8 px to its fill,
    /// its ring stood 7 px from the side.
    #[gpui::test]
    fn a_bar_menu_keeps_its_drawn_edge_8_px_from_the_windows_side(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            GlobalState::global_mut(cx).set_app_menus(menus());
        });
        let (_, cx) = cx.add_window_view(|_, cx| BarRoot {
            bar: Some(AppMenuBar::new(cx)),
        });
        let width = px(150.);
        cx.simulate_resize(gpui::size(width, px(300.)));
        draw(cx);
        cx.simulate_click(point(px(4.), px(16.)), Default::default());
        draw(cx);
        cx.simulate_keystrokes("right");
        draw(cx);
        let copy = cx.debug_bounds(COPY).expect("Edit's menu");
        let fill = cx.update(|window, cx| {
            let popover = gpui::Background::from(crate::ActiveTheme::theme(cx).popover);
            let scale = window.scale_factor();
            let logical = |value: gpui::ScaledPixels| px(value.as_f32() / scale);
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.background == popover)
                .map(|quad| {
                    gpui::Bounds::from_corners(
                        point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                        point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                    )
                })
                .filter(|fill| fill.contains(&copy.center()))
                .max_by(|a, b| a.size.width.partial_cmp(&b.size.width).unwrap())
                .expect("the menu's surface")
        });
        let side = width - (fill.right() + crate::POPOVER_EDGE_WIDTH);
        assert!(
            side >= px(8.) - px(0.01) && side < px(9.),
            "Edit's menu {fill:?} draws its edge {side:?} from the window's side, not 8 px"
        );
    }

    /// A menu bar taken away goes with its menus and their popup menus. Each
    /// menu held the bar strongly, and the bar holds its menus, a pair no
    /// drop broke, so a bar taken away kept itself, every menu and every
    /// popup menu it had opened for the app's life. On the way, a click
    /// opens File, Right and Left move between the open menus, the pointer
    /// moving onto Edit opens it in File's place and Escape closes it.
    #[gpui::test]
    fn a_dropped_bar_releases_its_menus(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            GlobalState::global_mut(cx).set_app_menus(menus());
        });
        let (root, cx) = cx.add_window_view(|_, cx| BarRoot {
            bar: Some(AppMenuBar::new(cx)),
        });
        draw(cx);
        let bar = root.read_with(cx, |root, _| root.bar.clone().expect("the bar"));
        let menus = bar.read_with(cx, |bar, _| bar.menus.clone());
        assert_eq!(menus.len(), 2);
        let popup = |ix: usize, cx: &mut VisualTestContext| {
            menus[ix].read_with(cx, |menu, _| {
                menu.popup_menu.as_ref().map(Entity::downgrade)
            })
        };

        // File's button starts the row.
        cx.simulate_click(point(px(4.), px(16.)), Default::default());
        draw(cx);
        assert_eq!(drawn(cx), (true, false), "the click opened File");
        let file = popup(0, cx).expect("File's popup menu");

        cx.simulate_keystrokes("right");
        draw(cx);
        assert_eq!(drawn(cx), (false, true), "Right moved to Edit");
        let edit = popup(1, cx).expect("Edit's popup menu");
        // Edit's menu opens at its button's leading edge.
        let edit_x = cx.debug_bounds(COPY).expect("Edit's item").left() + px(4.);

        cx.simulate_keystrokes("left");
        draw(cx);
        assert_eq!(drawn(cx), (true, false), "Left moved back to File");

        cx.simulate_mouse_move(point(edit_x, px(16.)), None, Default::default());
        draw(cx);
        assert_eq!(drawn(cx), (false, true), "the pointer moved to Edit");

        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(drawn(cx), (false, false), "Escape closed it");
        assert_eq!(bar.read_with(cx, |bar, _| bar.selected_index), None);

        let views: Vec<(&str, AnyWeakEntity)> = vec![
            ("bar", bar.downgrade().into()),
            ("File", menus[0].downgrade().into()),
            ("Edit", menus[1].downgrade().into()),
            ("File's popup menu", file.into()),
            ("Edit's popup menu", edit.into()),
        ];
        drop((bar, menus));
        root.update(cx, |root, cx| {
            root.bar = None;
            cx.notify();
        });
        draw(cx);
        draw(cx);
        let alive = views
            .iter()
            .filter(|(_, view)| view.is_upgradable())
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        assert!(alive.is_empty(), "alive after their bar went: {alive:?}");
    }
}
