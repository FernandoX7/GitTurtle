use std::rc::Rc;

use gpui::{
    Anchor, AnyElement, App, Context, DismissEvent, Element, ElementId, Entity, FocusHandle,
    Focusable, GlobalElementId, InspectorElementId, InteractiveElement, IntoElement, LayoutId,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, prelude::FluentBuilder,
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
/// what the trigger's siblings lay out against. Moving placement onto the
/// trigger container lets a `flex_1` or margined trigger keep its placement
/// among its siblings while keeping its own size, padding, colours and opacity
/// on the trigger itself.
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        TriggerFocus::new(self.id.clone(), move |trigger_focus, window, cx| {
            self.render_popover(trigger_focus, window, cx)
                .into_any_element()
        })
    }
}

impl<T> DropdownMenuPopover<T>
where
    T: DropdownMenu,
{
    fn render_popover(
        self,
        trigger_focus: FocusHandle,
        window: &mut Window,
        cx: &mut App,
    ) -> Popover {
        let builder = self.builder.clone();
        let menu_state = window.use_keyed_state(self.id.clone(), cx, |_, cx| DropdownMenuState {
            menu: None,
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
                        menu.update(cx, |menu, cx| {
                            menu.set_trigger_focus(Some(trigger_focus.clone()), cx);
                            // A trigger that takes no focus leaves what the
                            // menu's builder set standing.
                            if let Some(opener) = opener.clone() {
                                menu.set_dialog_return_focus(Some(opener));
                            }
                        });
                        menu_state.update(cx, |state, _| {
                            state.menu = Some(menu.clone());
                        });
                        menu.focus_handle(cx).focus(window, cx);

                        // Listen for dismiss events from the PopupMenu to close the popover.
                        //
                        // Hold Weak handles here, not strong clones: the listener
                        // lives as long as `menu`, which `menu_state` owns, so a
                        // strong capture would close the cycle
                        // `menu_state -> menu -> listener -> menu_state` and leak
                        // the `PopupMenu` and `DropdownMenuState` when the trigger
                        // stops being rendered while the menu is open.
                        let popover_state = cx.entity().downgrade();
                        window
                            .subscribe(&menu, cx, {
                                let menu_state = menu_state.downgrade();
                                move |_, _: &DismissEvent, window, cx| {
                                    if let Some(popover_state) = popover_state.upgrade() {
                                        popover_state.update(cx, |state, cx| {
                                            state.dismiss(window, cx);
                                        });
                                    }
                                    _ = menu_state.update(cx, |state, _| {
                                        state.menu = None;
                                    });
                                }
                            })
                            .detach();

                        menu
                    }
                };

                menu
            })
    }
}

type TriggerFocusBuild = Box<dyn FnOnce(FocusHandle, &mut Window, &mut App) -> AnyElement>;

/// Registers a focus handle on the trigger's dispatch node without ever
/// focusing it, so the menu opened from the trigger can resolve its shortcut
/// hints against the trigger's key contexts on the frame it opens. GPUI looks
/// a handle up in the previously rendered frame; the trigger was in it when
/// the menu was not yet.
struct TriggerFocus {
    id: ElementId,
    build: Option<TriggerFocusBuild>,
}

#[derive(Default)]
struct TriggerFocusState {
    focus_handle: Option<FocusHandle>,
}

struct TriggerFocusFrame {
    focus_handle: FocusHandle,
    child: AnyElement,
}

impl TriggerFocus {
    fn new(
        id: ElementId,
        build: impl FnOnce(FocusHandle, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id,
            build: Some(Box::new(build)),
        }
    }
}

#[cfg(not(feature = "gpui-fast"))]
impl IntoElement for TriggerFocus {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TriggerFocus {
    type RequestLayoutState = TriggerFocusFrame;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let focus_handle =
            window.with_optional_element_state::<TriggerFocusState, _>(id, |state, _| {
                let mut state = state.flatten().unwrap_or_default();
                let focus_handle = state
                    .focus_handle
                    .get_or_insert_with(|| cx.focus_handle())
                    .clone();
                (focus_handle, Some(state))
            });
        let build = self.build.take().expect("TriggerFocus is laid out once");
        let mut child = build(focus_handle.clone(), window, cx);
        let layout_id = child.request_layout(window, cx);

        (
            layout_id,
            TriggerFocusFrame {
                focus_handle,
                child,
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: gpui::Bounds<gpui::Pixels>,
        frame: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.set_focus_handle(&frame.focus_handle, cx);
        frame.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: gpui::Bounds<gpui::Pixels>,
        frame: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        frame.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        KeyBinding, MouseButton, ParentElement as _, Render, TestAppContext, WeakEntity, actions,
        div, point, px,
    };
    use std::cell::{Cell, RefCell};

    actions!(dropdown_menu_test, [CopyText]);

    const CONTEXT: &str = "dropdown_menu_test";

    /// The story shape: the key binding lives in the key context of the
    /// trigger's ancestor, the menu names no `action_context`, and other
    /// content outside that context paints after the trigger.
    struct TestRoot {
        frames: Rc<Cell<usize>>,
    }

    impl Render for TestRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.frames.set(self.frames.get() + 1);
            div()
                .size_full()
                .child(
                    div()
                        .key_context(CONTEXT)
                        .on_action(|_: &CopyText, _, _| {})
                        .child(
                            Button::new("trigger")
                                .label("Edit")
                                .w(px(100.))
                                .h(px(30.))
                                .dropdown_menu(|menu, _, _| menu.menu("Copy", Box::new(CopyText))),
                        ),
                )
                .child(div().child("Status"))
        }
    }

    /// Records the `PopupMenu` the dropdown builds, so a test can observe
    /// its release through a weak handle.
    struct MenuProbeRoot {
        menu: Rc<RefCell<Option<WeakEntity<PopupMenu>>>>,
    }

    impl Render for MenuProbeRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let slot = self.menu.clone();
            div().size_full().child(
                Button::new("trigger")
                    .label("Edit")
                    .w(px(100.))
                    .h(px(30.))
                    .dropdown_menu(move |menu, _, cx| {
                        *slot.borrow_mut() = Some(cx.weak_entity());
                        menu.menu("Copy", Box::new(CopyText))
                    }),
            )
        }
    }

    /// Opening the menu and closing the window without dismissing it must
    /// release the `PopupMenu`: the dismiss subscription lives as long as the
    /// menu, which `DropdownMenuState` owns, so strong captures in it formed
    /// a cycle that no element-state collection could break. The state still
    /// holds the menu while it is open, so the menu's release also proves the
    /// `DropdownMenuState` was released.
    ///
    /// The popover must go too: it holds the deferred-popover registration
    /// while open, and a leaked one keeps every later right-click menu
    /// stepping aside as if a popup were still showing.
    #[gpui::test]
    fn open_without_dismiss_releases_the_menu(cx: &mut TestAppContext) {
        cx.update(|cx| crate::init(cx));
        let menu = Rc::new(RefCell::new(None::<WeakEntity<PopupMenu>>));

        {
            let (_, cx) = cx.add_window_view(|_, _| MenuProbeRoot { menu: menu.clone() });
            cx.update(|window, cx| window.draw(cx).clear(cx));

            // Click the trigger; the menu opens and is left open.
            cx.simulate_mouse_down(
                point(px(10.), px(10.)),
                MouseButton::Left,
                Default::default(),
            );
            cx.run_until_parked();
            assert!(
                menu.borrow()
                    .as_ref()
                    .and_then(|menu| menu.upgrade())
                    .is_some(),
                "the menu must be open before the window closes"
            );

            // Close the window without dismissing the menu.
            cx.update(|window, _| window.remove_window());
            cx.run_until_parked();
        }

        assert!(
            menu.borrow()
                .as_ref()
                .and_then(|menu| menu.upgrade())
                .is_none(),
            "the PopupMenu must be released with the window"
        );
        cx.update(|cx| {
            assert!(
                !gpui_base::GlobalState::is_in_deferred_context(cx),
                "the popover's deferred registration must be released with the window"
            )
        });
    }

    #[gpui::test]
    fn shortcut_hint_is_painted_on_the_frame_the_menu_opens(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.bind_keys([KeyBinding::new("ctrl-c", CopyText, Some(CONTEXT))]);
        });
        let frames = Rc::new(Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let frames = frames.clone();
            move |_, _| TestRoot { frames }
        });
        // The popup host captures its trigger bounds on the first frame.
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let frames_before_open = frames.get();

        cx.simulate_mouse_down(
            point(px(10.), px(10.)),
            MouseButton::Left,
            Default::default(),
        );

        assert_eq!(
            frames.get(),
            frames_before_open + 1,
            "the press must be followed by exactly one frame for this to test the first one"
        );
        assert!(
            cx.debug_bounds("kbd:ctrl-c").is_some(),
            "the shortcut hint must be painted on the same frame as its item"
        );
    }
}
