//! Linux client decorations for compositors that do not supply a title bar.
use crate::*;
use gpui_kit::component::{IconName, InteractiveElementExt as _};
use gpui_kit::prelude::FluentBuilder as _;

#[derive(Clone, Copy)]
pub(super) enum Side {
    Left,
    Right,
}

fn client_decorated(window: &Window) -> bool {
    matches!(window.window_decorations(), Decorations::Client { .. })
}

pub(super) fn controls(side: Side, busy: bool, window: &Window, cx: &App) -> Option<AnyElement> {
    if !client_decorated(window) {
        return None;
    }
    let layout = cx
        .button_layout()
        .unwrap_or_else(WindowButtonLayout::linux_default);
    let supported = window.window_controls();
    let buttons = match side {
        Side::Left => layout.left,
        Side::Right => layout.right,
    };
    let buttons = buttons.into_iter().flatten().filter(|button| match button {
        WindowButton::Minimize => supported.minimize,
        WindowButton::Maximize => supported.maximize,
        WindowButton::Close => true,
    });
    let buttons: Vec<_> = buttons
        .map(|kind| control(kind, busy, window.is_maximized()))
        .collect();
    if buttons.is_empty() {
        return None;
    }
    Some(
        div()
            .id(match side {
                Side::Left => "left-window-controls",
                Side::Right => "right-window-controls",
            })
            .role(Role::Group)
            .aria_label("Window controls")
            .flex()
            .items_center()
            .flex_shrink_0()
            .gap_1()
            .when(matches!(side, Side::Left), |group| group.mr_2())
            .when(matches!(side, Side::Right), |group| group.ml_2())
            .children(buttons)
            .into_any_element(),
    )
}

fn control(kind: WindowButton, busy: bool, maximized: bool) -> Button {
    let (id, label, symbol) = match kind {
        WindowButton::Minimize => (
            "minimize-window",
            "Minimize window",
            IconName::WindowMinimize,
        ),
        WindowButton::Maximize if maximized => {
            ("maximize-window", "Restore window", IconName::WindowRestore)
        }
        WindowButton::Maximize => (
            "maximize-window",
            "Maximize window",
            IconName::WindowMaximize,
        ),
        WindowButton::Close => ("close-window", "Close window", IconName::WindowClose),
    };
    button(id, "", "", false)
        .icon(Icon::new(symbol).size(appearance::ui_size(16.)))
        .rounded_full()
        .accessibility_label(label)
        .tooltip(if kind == WindowButton::Close && busy {
            "Wait for the current Git operation to finish before closing"
        } else {
            label
        })
        .disabled(kind == WindowButton::Close && busy)
        .on_click(move |_, window, cx| {
            let action: Box<dyn Action> = match kind {
                WindowButton::Minimize => Box::new(MinimizeWindow),
                WindowButton::Maximize => Box::new(ZoomWindow),
                WindowButton::Close => Box::new(CloseWindow),
            };
            window.dispatch_action(action, cx);
        })
}

struct HeaderDrag {
    armed: bool,
}

impl Render for HeaderDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Only blank header space starts a move; tab and menu clicks retain their actions.
pub(super) fn drag_region(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    if !client_decorated(window) {
        return None;
    }
    let state = window.use_state(cx, |_, _| HeaderDrag { armed: false });
    let supported = window.window_controls();
    Some(
        div()
            .id("window-header-drag")
            .flex_1()
            .min_w(appearance::ui_size(32.))
            .h(appearance::ui_size(28.))
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&state, |state, event: &MouseDownEvent, _, _| {
                    state.armed = event.click_count == 1;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&state, |state, _, _, _| state.armed = false),
            )
            .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| state.armed = false))
            .on_mouse_move(window.listener_for(
                &state,
                |state, event: &MouseMoveEvent, window, _| {
                    if state.armed {
                        state.armed = false;
                        if event.pressed_button == Some(MouseButton::Left) {
                            window.start_window_move();
                        }
                    }
                },
            ))
            .when(supported.maximize, |region| {
                region.on_double_click(|_, window, _| window.zoom_window())
            })
            .when(supported.window_menu, |region| {
                region.on_mouse_down(MouseButton::Right, |event, window, _| {
                    window.show_window_menu(event.position);
                })
            })
            .into_any_element(),
    )
}
