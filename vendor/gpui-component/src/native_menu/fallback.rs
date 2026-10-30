//! Fallback popup menu for platforms without an OS-native popup (e.g. Linux).
//!
//! It renders gpui-component's drawn [`PopupMenu`] through an overlay held by
//! [`Root`]. Unlike a real native menu it is clipped to the window, but it keeps
//! the [`super::NativeMenu`] API working on every platform.
//!
//! GitTurtle patch: placed at the pointer by the base `Positioner`, inside
//! the window less 8 px and above the room an application keeps at its foot
//! (`gpui_base::popup_bottom_inset`), and never over the point it was opened
//! at: below the pointer where it fits, otherwise above it, its bottom edge
//! at the pointer. GPUI's `anchored` snap kept it only inside the window, so
//! an input's right-click menu near the window's foot ended over a status
//! bar, and kept above the bar by sliding up, it covered the line clicked.

use gpui::{
    App, Context, DismissEvent, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Pixels,
    Point, Render, Subscription, Window, deferred, div, px,
};

use crate::menu::{PopupMenu, PopupMenuItem};
use crate::root::Root;

use super::NativeMenuItem;

/// Overlay held by [`Root`] that renders the active fallback popup menu, if any.
pub(crate) struct FallbackMenuOverlay {
    active: Option<ActiveMenu>,
}

struct ActiveMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

/// GitTurtle patch: how the menu opened at `position` is placed: wholly
/// below the point or wholly above it ([`gpui_base::Positioner::point`]),
/// 8 px inside the window and above the room the application keeps at its
/// foot, as the kit's context menu and dropdowns keep theirs.
fn placement(position: Point<Pixels>, window: &Window, cx: &App) -> gpui_base::Positioner {
    gpui_base::Positioner::point(position)
        .margin(px(8.))
        .bottom_inset(gpui_base::popup_bottom_inset(window, cx))
}

impl FallbackMenuOverlay {
    pub(crate) fn new() -> Self {
        Self { active: None }
    }

    /// Build a [`PopupMenu`] from `items` and show it anchored at `position`.
    ///
    /// `action_context` is the focus handle the selected action is dispatched
    /// to, matching the native backends (which dispatch to the window's focus).
    #[allow(dead_code)] // Only used where the native menu falls back (e.g. Linux).
    fn show(
        &mut self,
        items: Vec<NativeMenuItem>,
        position: Point<Pixels>,
        action_context: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = build_popup(items, action_context, window, cx);

        let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.active = None;
            cx.notify();
        });
        menu.focus_handle(cx).focus(window, cx);

        self.active = Some(ActiveMenu {
            menu,
            position,
            _subscription: subscription,
        });
        cx.notify();
    }
}

/// Recursively build a [`PopupMenu`] entity from native menu items.
#[allow(dead_code)] // Only used where the native menu falls back (e.g. Linux).
fn build_popup(
    items: Vec<NativeMenuItem>,
    action_context: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    PopupMenu::build(window, cx, move |mut menu, window, cx| {
        if let Some(handle) = action_context.clone() {
            menu = menu.action_context(handle);
        }
        for item in items {
            menu = match item {
                NativeMenuItem::Separator => menu.separator(),
                NativeMenuItem::Item {
                    label,
                    disabled,
                    checked,
                    icon: Some(icon),
                    action: Some(action),
                } => menu.item(
                    PopupMenuItem::new(label)
                        .icon(*icon)
                        .action(action)
                        .disabled(disabled)
                        .checked(checked),
                ),
                NativeMenuItem::Item {
                    label,
                    disabled,
                    checked,
                    icon: None,
                    action: Some(action),
                } => menu.menu_with_check_and_disabled(label, checked, action, disabled),
                NativeMenuItem::Item { action: None, .. } => menu,
                NativeMenuItem::Submenu {
                    label,
                    disabled,
                    items,
                } => {
                    let submenu = build_popup(items, action_context.clone(), window, cx);
                    menu.item(PopupMenuItem::submenu(label, submenu).disabled(disabled))
                }
            };
        }
        menu
    })
}

impl Render for FallbackMenuOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div();
        if let Some(active) = self.active.as_ref() {
            root = root.child(
                deferred(
                    // GitTurtle patch: placed wholly below or above the
                    // pointer, inside the window less its margin and above
                    // the room the application keeps at its foot. GPUI's
                    // `anchored` snap kept it only inside the window, so it
                    // could end over a status bar, and a clamp that moved it
                    // up covered the point it was opened at.
                    placement(active.position, window, cx).child(active.menu.clone()),
                )
                .with_priority(gpui_base::POPUP_PRIORITY),
            );
        }
        root
    }
}

/// Show the fallback popup menu through [`Root`]'s overlay.
#[allow(dead_code)] // Only called where the native menu falls back (e.g. Linux).
pub(super) fn show(
    items: Vec<NativeMenuItem>,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let action_context = window.focused(cx);
    let Some(overlay) = Root::native_menu_overlay(window, cx) else {
        return;
    };
    overlay.update(cx, |overlay, cx| {
        overlay.show(items, position, action_context, window, cx);
    });
}
