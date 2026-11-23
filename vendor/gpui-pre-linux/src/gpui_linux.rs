#![cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;

#[cfg(any(feature = "wayland", feature = "x11"))]
pub use linux::{
    active_window_identifier, portal_accent_color, wait_initial_portal_appearance,
};
pub use linux::{
    activate_wayland_window_with_token, current_platform, linux_platform, set_wayland_ime_password,
};
