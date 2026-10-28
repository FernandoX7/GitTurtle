use std::rc::Rc;

use gpui::{
    Anchor, App, Context, DismissEvent, ElementId, Entity, Focusable, InteractiveElement,
    IntoElement, RenderOnce, SharedString, StyleRefinement, Styled, Window, prelude::FluentBuilder,
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

impl DropdownMenu for Button {}

#[derive(IntoElement)]
pub struct DropdownMenuPopover<T: Selectable + IntoElement + 'static> {
    id: ElementId,
    style: StyleRefinement,
    anchor: Anchor,
    trigger: T,
    builder: Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>,
    on_open_change: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
}

impl<T> DropdownMenuPopover<T>
where
    T: Selectable + IntoElement + 'static,
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

#[derive(Default)]
struct DropdownMenuState {
    menu: Option<Entity<PopupMenu>>,
}

impl<T> RenderOnce for DropdownMenuPopover<T>
where
    T: Selectable + IntoElement + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let builder = self.builder.clone();
        let menu_state =
            window.use_keyed_state(self.id.clone(), cx, |_, _| DropdownMenuState::default());

        Popover::new(SharedString::from(format!("popover:{}", self.id)))
            .appearance(false)
            .overlay_closable(false)
            .trigger(self.trigger)
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
                        menu_state.update(cx, |state, _| {
                            state.menu = Some(menu.clone());
                        });
                        menu.focus_handle(cx).focus(window, cx);

                        // Listen for dismiss events from the PopupMenu to close the popover.
                        let popover_state = cx.entity();
                        window
                            .subscribe(&menu, cx, {
                                let menu_state = menu_state.clone();
                                move |_, _: &DismissEvent, window, cx| {
                                    popover_state.update(cx, |state, cx| {
                                        state.dismiss(window, cx);
                                    });
                                    menu_state.update(cx, |state, _| {
                                        state.menu = None;
                                    });
                                }
                            })
                            .detach();

                        menu.clone()
                    }
                };

                menu.clone()
            })
    }
}
