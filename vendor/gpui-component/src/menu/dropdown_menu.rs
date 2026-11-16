use std::rc::Rc;

use gpui::{
    Anchor, App, Context, DismissEvent, ElementId, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled,
    Subscription, Window, prelude::FluentBuilder,
};

use crate::{Selectable, button::Button, menu::PopupMenu, popover::Popover};

/// A dropdown menu trait for buttons and other interactive elements
pub trait DropdownMenu: Styled + Selectable + InteractiveElement + IntoElement + 'static {
    /// Create a dropdown menu with the given items, anchored to the TopLeft corner
    fn dropdown_menu(
        self,
        f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> DropdownMenuPopover<Self> {
        self.dropdown_menu_with_anchor(Anchor::TopLeft, f)
    }

    /// Create a dropdown menu with the given items, anchored to the given corner
    fn dropdown_menu_with_anchor(
        mut self,
        anchor: impl Into<Anchor>,
        f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> DropdownMenuPopover<Self> {
        let style = take_placement(self.style());
        let id = self.interactivity().element_id.clone();

        DropdownMenuPopover::new(id.unwrap_or(0.into()), anchor, self, f).trigger_style(style)
    }

    /// GitTurtle patch: has the trigger track `owned`, where it tracks no
    /// focus handle of its own and can take one, and returns the trigger with
    /// the handle it tracks: the control a dialog opened from its menu gives
    /// focus back to ([`PopupMenu::set_dialog_return_focus`]). A trigger that
    /// takes no handle returns `None`, and a dialog from its menu keeps the
    /// fallback it has.
    #[doc(hidden)]
    fn tracking_opener_focus(self, _owned: &FocusHandle) -> (Self, Option<FocusHandle>)
    where
        Self: Sized,
    {
        (self, None)
    }
}

/// GitTurtle patch: the part of a trigger's style that places it among its
/// siblings, its margins and flex-item properties, taken from the trigger.
///
/// The popover lays the trigger out inside a box of its own, and that box is
/// what the trigger's siblings lay out against. Upstream copied the trigger's
/// whole style for the popover, which dropped it, and left it on the trigger,
/// inside that box, where flex-item properties do nothing: a `flex_1` trigger
/// kept its content width. (Margins reached the row anyway, because a block
/// box counts its child's margins in its size.) The box now takes the
/// placement and the trigger keeps the rest: its size, padding, colours and
/// opacity would apply twice on both boxes, so the whole style does not move.
fn take_placement(style: &mut StyleRefinement) -> StyleRefinement {
    let mut placement = StyleRefinement::default();
    placement.margin = std::mem::take(&mut style.margin);
    placement.flex_grow = style.flex_grow.take();
    placement.flex_shrink = style.flex_shrink.take();
    placement.flex_basis = style.flex_basis.take();
    placement.align_self = style.align_self.take();
    placement
}

impl DropdownMenu for Button {
    fn tracking_opener_focus(self, owned: &FocusHandle) -> (Self, Option<FocusHandle>) {
        // A Button that is no Tab stop need not be drawn to be seen, as a
        // row's menu Button that shows only on hover can be at opacity 0, so
        // it takes no focus back from a dialog.
        if !self.is_tab_stop() {
            return (self, None);
        }
        match self.provided_focus_handle().cloned() {
            Some(handle) => (self, Some(handle)),
            None => (self.track_focus(owned), Some(owned.clone())),
        }
    }
}

#[derive(IntoElement)]
pub struct DropdownMenuPopover<T: DropdownMenu> {
    id: ElementId,
    style: StyleRefinement,
    anchor: Anchor,
    trigger: T,
    builder: Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>,
    on_open_change: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
}

impl<T> DropdownMenuPopover<T>
where
    T: DropdownMenu,
{
    fn new(
        id: ElementId,
        anchor: impl Into<Anchor>,
        trigger: T,
        builder: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        Self {
            id: SharedString::from(format!("dropdown-menu:{:?}", id)).into(),
            style: StyleRefinement::default(),
            anchor: anchor.into(),
            trigger,
            builder: Rc::new(builder),
            on_open_change: None,
        }
    }

    /// Set the anchor corner for the dropdown menu popover.
    pub fn anchor(mut self, anchor: impl Into<Anchor>) -> Self {
        self.anchor = anchor.into();
        self
    }

    /// Set the style refinement for the dropdown menu trigger.
    fn trigger_style(mut self, style: StyleRefinement) -> Self {
        self.style = style;
        self
    }

    /// Add a callback to be called when the menu opens or closes.
    ///
    /// The `&bool` parameter is the **new open state**.
    pub fn on_open_change(
        mut self,
        callback: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(callback));
        self
    }
}

struct DropdownMenuState {
    menu: Option<Entity<PopupMenu>>,
    /// GitTurtle patch: the open menu's dismiss subscription, kept with the
    /// menu and dropped with it.
    dismiss: Option<Subscription>,
    /// GitTurtle patch: the focus handle a trigger with none of its own
    /// tracks, so the dropdown can name the control that opened its menu:
    /// a Button refuses focus on a press, so what held focus as a pointer
    /// opened the menu need not be the trigger. It lives as long as the
    /// trigger is drawn, and a dialog holds it weakly.
    trigger_focus: FocusHandle,
}

impl<T> RenderOnce for DropdownMenuPopover<T>
where
    T: DropdownMenu,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let builder = self.builder.clone();
        let menu_state = window.use_keyed_state(self.id.clone(), cx, |_, cx| DropdownMenuState {
            menu: None,
            dismiss: None,
            trigger_focus: cx.focus_handle(),
        });
        let owned = menu_state.read(cx).trigger_focus.clone();
        let (trigger, opener) = self.trigger.tracking_opener_focus(&owned);

        Popover::new(SharedString::from(format!("popover:{}", self.id)))
            .appearance(false)
            .overlay_closable(false)
            .trigger(trigger)
            .trigger_style(self.style)
            .anchor(self.anchor)
            .when_some(self.on_open_change, |this, callback| {
                this.on_open_change(move |open, window, cx| callback(open, window, cx))
            })
            .content(move |_, window, cx| {
                // Here is special logic to only create the PopupMenu once and reuse it.
                // Because this `content` will called in every time render, so we need to store the menu
                // in state to avoid recreating at every render.
                //
                // And we also need to rebuild the menu when it is dismissed, to rebuild menu items
                // dynamically for support `dropdown_menu` method, so we listen for DismissEvent below.
                let menu = match menu_state.read(cx).menu.clone() {
                    Some(menu) => menu,
                    None => {
                        let builder = builder.clone();
                        let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
                            builder(menu, window, cx)
                        });
                        // A trigger that takes no focus leaves what the
                        // menu's builder set standing.
                        if let Some(opener) = opener.clone() {
                            menu.update(cx, |menu, _| {
                                menu.set_dialog_return_focus(Some(opener));
                            });
                        }
                        menu.focus_handle(cx).focus(window, cx);

                        // Listen for dismiss events from the PopupMenu to close the popover.
                        //
                        // GitTurtle patch: the callback holds both states
                        // weakly, and the subscription lives with the menu
                        // in the dropdown's state. Upstream detached it,
                        // holding both strongly: GPUI keeps a detached
                        // callback while the menu lives, and the state holds
                        // the menu, so a dropdown taken away with its menu
                        // open kept its state, its popover state and the menu,
                        // with all its items held, for the app's life
                        // (GITTURTLE-PATCH.md).
                        let popover_state = cx.weak_entity();
                        let dismiss = window.subscribe(&menu, cx, {
                            let menu_state = menu_state.downgrade();
                            move |_, _: &DismissEvent, window, cx| {
                                if let Some(popover_state) = popover_state.upgrade() {
                                    popover_state.update(cx, |state, cx| {
                                        state.dismiss(window, cx);
                                    });
                                }
                                if let Some(menu_state) = menu_state.upgrade() {
                                    menu_state.update(cx, |state, _| {
                                        state.menu = None;
                                        state.dismiss = None;
                                    });
                                }
                            }
                        });
                        menu_state.update(cx, |state, _| {
                            state.menu = Some(menu.clone());
                            state.dismiss = Some(dismiss);
                        });

                        menu.clone()
                    }
                };

                menu.clone()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::PopupMenuItem;
    use gpui::{
        AnyWeakEntity, Context, ParentElement as _, Render, TestAppContext, VisualTestContext, div,
        point, px,
    };
    use gpui_base::GlobalState;
    use std::cell::{Cell, RefCell};

    /// A dropdown, drawn while `shown` holds. Its menu's item holds `token`,
    /// as an item holds what it acts on, and counts in `chosen` the times it
    /// ran.
    struct Owner {
        shown: bool,
        token: Rc<()>,
        chosen: Rc<Cell<usize>>,
    }

    impl Render for Owner {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let (token, chosen) = (self.token.clone(), self.chosen.clone());
            div().size_full().when(self.shown, |this| {
                this.child(
                    Button::new("trigger")
                        .debug_selector(|| "trigger".into())
                        .label("Open")
                        .dropdown_menu(move |menu, _, _| {
                            let (token, chosen) = (token.clone(), chosen.clone());
                            menu.item(PopupMenuItem::new("Act").on_click(move |_, _, _| {
                                let _held = &token;
                                chosen.set(chosen.get() + 1);
                            }))
                        }),
                )
            })
        }
    }

    const ACT: &str = "menu-item: Act · MenuItem";

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// The dropdown states and menus the window builds, held weakly.
    #[derive(Clone, Default)]
    struct Built(Rc<RefCell<Vec<(&'static str, AnyWeakEntity)>>>);

    impl Built {
        fn record<T: 'static>(&self, name: &'static str, cx: &mut App) -> gpui::Subscription {
            let built = self.clone();
            cx.observe_new(move |_: &mut T, _, cx: &mut Context<T>| {
                built
                    .0
                    .borrow_mut()
                    .push((name, AnyWeakEntity::from(cx.weak_entity())));
            })
        }

        /// The names of those still alive.
        fn alive(&self) -> Vec<&'static str> {
            self.0
                .borrow()
                .iter()
                .filter(|(_, view)| view.is_upgradable())
                .map(|(name, _)| *name)
                .collect()
        }
    }

    /// Opens the dropdown with a click on its trigger.
    fn open(cx: &mut VisualTestContext) {
        let trigger = cx.debug_bounds("trigger").expect("the trigger");
        cx.simulate_click(trigger.center(), Default::default());
        draw(cx);
        assert!(cx.debug_bounds(ACT).is_some(), "the click opened it");
    }

    /// A dropdown taken away while its menu is open lets its state, its
    /// popover state and its menu go, with what the menu's items hold, and
    /// leaves no deferred popup registered. Upstream's dismiss callback held
    /// the dropdown's state and popover state strongly and was detached, so
    /// GPUI kept it while the menu lived, and the state holds the menu:
    /// taken away open, as a view built again without it takes it, the
    /// dropdown kept all three until its menu was next dismissed, which no
    /// input could do once the dropdown was gone. The popover state goes
    /// only with gpui-base's popover patch, whose own dismiss subscription
    /// held it too. Escape, a click outside, an item chosen and a second
    /// click on the trigger each still close the menu and let it go, and
    /// the next click builds it again.
    #[gpui::test]
    fn a_dropdown_taken_away_open_releases_its_menu(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let built = Built::default();
        let _recording = cx.update(|cx| {
            [
                built.record::<DropdownMenuState>("dropdown state", cx),
                built.record::<gpui_base::PopoverState>("popover state", cx),
                built.record::<PopupMenu>("menu", cx),
            ]
        });
        let (owner, cx) = cx.add_window_view(|_, _| Owner {
            shown: true,
            token: Rc::new(()),
            chosen: Rc::default(),
        });
        draw(cx);
        let token = owner.read_with(cx, |owner, _| Rc::downgrade(&owner.token));
        let trigger = cx.debug_bounds("trigger").expect("the trigger").center();

        type Close = fn(&mut VisualTestContext, gpui::Point<gpui::Pixels>);
        let ways: [(&str, Close); 4] = [
            ("Escape", |cx, _| cx.simulate_keystrokes("escape")),
            ("a click outside", |cx, _| {
                cx.simulate_click(point(px(300.), px(300.)), Default::default())
            }),
            ("an item chosen", |cx, _| {
                cx.simulate_keystrokes("down enter")
            }),
            ("the trigger", |cx, trigger| {
                cx.simulate_click(trigger, Default::default())
            }),
        ];
        for (way, close) in ways {
            open(cx);
            assert_eq!(
                built.alive(),
                ["dropdown state", "popover state", "menu"],
                "{way}: one menu"
            );
            close(cx, trigger);
            draw(cx);
            assert!(cx.debug_bounds(ACT).is_none(), "{way} closed it");
            assert!(
                !cx.update(|_, cx| GlobalState::is_in_deferred_context(cx)),
                "{way} closed its popover"
            );
            assert_eq!(
                built.alive(),
                ["dropdown state", "popover state"],
                "{way} let the menu go"
            );
            assert_eq!(
                token.strong_count(),
                1,
                "{way}: only the owner holds the token"
            );
        }
        assert_eq!(owner.read_with(cx, |owner, _| owner.chosen.get()), 1);
        assert_eq!(
            built.0.borrow().len(),
            2 + ways.len(),
            "each open built its menu"
        );

        open(cx);
        owner.update(cx, |owner, cx| {
            owner.shown = false;
            cx.notify();
        });
        draw(cx);
        draw(cx);
        assert!(cx.debug_bounds("trigger").is_none(), "the dropdown went");
        assert!(cx.debug_bounds(ACT).is_none(), "with its menu");
        assert!(
            built.alive().is_empty(),
            "alive after their dropdown went: {:?}",
            built.alive()
        );
        assert!(
            !cx.update(|_, cx| GlobalState::is_in_deferred_context(cx)),
            "the dropdown left its popover's deferred popup registered"
        );
        assert_eq!(token.strong_count(), 1, "only the owner holds the token");
    }
}
