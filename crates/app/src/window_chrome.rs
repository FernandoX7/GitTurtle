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
    #[cfg(test)]
    if CLIENT_DECORATED.with(std::cell::Cell::get) {
        return true;
    }
    matches!(window.window_decorations(), Decorations::Client { .. })
}

#[cfg(test)]
thread_local! {
    /// Whether a test draws its window as client-decorated, with the window
    /// controls in the tab strip: GPUI's test window never is.
    pub(crate) static CLIENT_DECORATED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The window buttons drawn on `side`: none where the window is not
/// client-decorated.
fn buttons(side: Side, window: &Window, cx: &App) -> Vec<WindowButton> {
    if !client_decorated(window) {
        return Vec::new();
    }
    let layout = cx
        .button_layout()
        .unwrap_or_else(WindowButtonLayout::linux_default);
    let supported = window.window_controls();
    let buttons = match side {
        Side::Left => layout.left,
        Side::Right => layout.right,
    };
    buttons
        .into_iter()
        .flatten()
        .filter(|button| match button {
            WindowButton::Minimize => supported.minimize,
            WindowButton::Maximize => supported.maximize,
            WindowButton::Close => true,
        })
        .collect()
}

/// The width the window controls and the drag region take from the
/// repository tab strip, its gap (`repository_tabs::tab_gap`) beside each
/// included, as the strip and the tab list draw it: each side's 28 px
/// buttons, the same gap apart ([`controls`]), with the group's `mr_2` or
/// `ml_2`, and the drag region's least width after the tabs. Nothing where
/// the window is not client-decorated.
pub(super) fn strip_width(window: &Window, cx: &App) -> Pixels {
    if !client_decorated(window) {
        return px(0.);
    }
    let snap = |length: Pixels| window.pixel_snap(length);
    let rem = window.rem_size();
    let gap = crate::repository_tabs::tab_gap(window);
    let control = snap(appearance::ui_size(28.));
    let group = |side| match buttons(side, window, cx).len() {
        0 => px(0.),
        count => control * count as f32 + gap * (count - 1) as f32 + snap(rem * 0.5) + gap,
    };
    group(Side::Left) + group(Side::Right) + gap + snap(appearance::ui_size(32.))
}

/// The window's controls on `side`. A write in flight, `busy`, leaves Close
/// refused at once (`CloseWindow`) and announced so, and dimmed once its
/// spell looks busy, `looks_busy`, with why it waits, the spell's reason
/// (`GitTurtle::busy_reason`), in its tooltip (`workspace::refusing`).
pub(super) fn controls(
    side: Side,
    busy: bool,
    looks_busy: Option<&'static str>,
    window: &Window,
    cx: &App,
) -> Option<AnyElement> {
    let buttons: Vec<_> = buttons(side, window, cx)
        .into_iter()
        .map(|kind| control(kind, busy, looks_busy, window.is_maximized()))
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
            // A ring's room apart, as the strip's controls and its tabs are:
            // at 11 pt a focused control's ring reached a device pixel into
            // the one beside it (`repository_tabs::tab_gap`).
            .gap(crate::repository_tabs::tab_gap(window))
            .when(matches!(side, Side::Left), |group| group.mr_2())
            .when(matches!(side, Side::Right), |group| group.ml_2())
            .children(buttons)
            .into_any_element(),
    )
}

fn control(
    kind: WindowButton,
    busy: bool,
    looks_busy: Option<&'static str>,
    maximized: bool,
) -> Button {
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
        .debug_selector(move || id.into())
        .icon(Icon::new(symbol).size(appearance::ui_size(16.)))
        .rounded_full()
        .accessibility_label(label)
        .tooltip(match looks_busy {
            Some(reason) if kind == WindowButton::Close => {
                SharedString::from(format!("{reason} before closing"))
            }
            _ => label.into(),
        })
        .map(|control| {
            let close = kind == WindowButton::Close;
            crate::workspace::refusing(
                control,
                move || id.into(),
                close && busy,
                close && looks_busy.is_some(),
            )
        })
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
