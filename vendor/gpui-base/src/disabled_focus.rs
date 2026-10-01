//! GitTurtle patch: focus tracking for controls that can turn disabled while
//! they hold keyboard focus.

use gpui::{FocusHandle, InteractiveElement, Window};

/// Tracks a control's focus handle while it is enabled, as upstream did, and
/// also while it is disabled but still holds focus.
///
/// Upstream tracked the handle only while the control was enabled, so a
/// control disabled while focused left the rendered frame with the window's
/// focus still on it. GPUI then dispatches keys from the window's root
/// dispatch node, above any key context that binds Tab and Shift+Tab, and
/// focus stays on the dimmed control until a pointer click. While focused, a
/// disabled control now keeps its handle in the frame as a target that is not
/// a tab stop, so Tab and Shift+Tab move on from its place and never land on
/// it. Unfocused disabled controls track nothing, as before.
///
/// `tab_stop(false)` also writes the handle's window-wide record; the enabled
/// path rewrites it on every render, so a control that is enabled again is a
/// tab stop again.
pub(crate) fn track_control_focus<E: InteractiveElement>(
    element: E,
    focus_handle: FocusHandle,
    disabled: bool,
    tab_index: isize,
    tab_stop: bool,
    window: &Window,
) -> E {
    if !disabled {
        element.track_focus(&focus_handle.tab_index(tab_index).tab_stop(tab_stop))
    } else if focus_handle.is_focused(window) {
        element.track_focus(&focus_handle.tab_index(tab_index).tab_stop(false))
    } else {
        element
    }
}
