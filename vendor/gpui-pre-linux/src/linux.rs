mod dispatcher;
mod display_connection;
mod headless;
mod keyboard;
mod platform;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod power;
mod system_notifications;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod text_system;
#[cfg(feature = "wayland")]
mod wayland;
#[cfg(feature = "x11")]
mod x11;

#[cfg(any(feature = "wayland", feature = "x11"))]
mod xdg_desktop_portal;

pub use dispatcher::*;
pub(crate) use display_connection::{Backend, DisplayConnection, select_backend};
pub(crate) use headless::*;
pub(crate) use keyboard::*;
pub(crate) use platform::*;
#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) use text_system::*;
#[cfg(feature = "wayland")]
pub(crate) use wayland::*;
#[cfg(feature = "x11")]
pub(crate) use x11::*;

use std::rc::Rc;

use gpui::WindowingModes;

/// Returns the default platform implementation for the current OS.
///
/// A windowed platform connects to the display server the process environment names, or starts
/// headless when it names none, and can switch between those modes later. A headless platform
/// stays headless.
pub fn current_platform(headless: bool) -> Rc<dyn gpui::Platform> {
    if headless || std::env::var_os("ZED_HEADLESS").is_some() {
        linux_platform(WindowingModes::HEADLESS)
    } else {
        linux_platform(WindowingModes::all())
    }
}

/// Returns a platform that may switch among `allowed_modes`.
///
/// It starts windowed in the process's own environment, or headless if that names no allowed
/// display server. Set another initial mode with [`gpui::Application::with_windowing`].
pub fn linux_platform(allowed_modes: WindowingModes) -> Rc<dyn gpui::Platform> {
    Rc::new(LinuxPlatform::new(allowed_modes))
}

/// Updates the active Linux IME content type between password
/// (`ContentHint::SensitiveData | ContentHint::HiddenText` +
/// `ContentPurpose::Password` on Wayland; suspended XIM forwarding on X11)
/// and normal text. No-op on headless connections.
pub fn set_wayland_ime_password(password: bool) {
    #[cfg(feature = "wayland")]
    wayland::set_wayland_ime_password(password);
    #[cfg(feature = "x11")]
    x11::set_x11_ime_password(password);
    #[cfg(not(any(feature = "wayland", feature = "x11")))]
    let _ = password;
}

/// Applies a forwarded `XDG_ACTIVATION_TOKEN` to the active Wayland surface via
/// `xdg_activation_v1` so focus-stealing-preventing compositors raise the window.
pub fn activate_wayland_window_with_token(token: String) -> bool {
    #[cfg(feature = "wayland")]
    {
        wayland::activate_wayland_window_with_token(token)
    }
    #[cfg(not(feature = "wayland"))]
    {
        let _ = token;
        false
    }
}

/// Resolves an `ashpd::WindowIdentifier` for the active Wayland or X11 window
/// so desktop portal and Secret Service prompts attach to the triggering window.
#[cfg(any(feature = "wayland", feature = "x11"))]
pub fn active_window_identifier() -> futures::future::BoxFuture<'static, Option<ashpd::WindowIdentifier>>
{
    #[cfg(feature = "wayland")]
    if let Some(future) = wayland::active_wayland_window_identifier() {
        return future;
    }
    #[cfg(feature = "x11")]
    if let Some(future) = x11::active_x11_window_identifier() {
        return future;
    }
    Box::pin(std::future::ready(None))
}

/// Waits at most `timeout` for the initial Linux desktop appearance to be
/// resolved and applied on the active display connection.
#[cfg(any(feature = "wayland", feature = "x11"))]
pub fn wait_initial_portal_appearance(
    timeout: std::time::Duration,
) -> futures::future::BoxFuture<'static, Option<gpui::WindowAppearance>> {
    xdg_desktop_portal::wait_initial_portal_appearance(timeout)
}

/// Returns the latest sRGB accent color (`[r, g, b]` in `0.0..=1.0`) reported by
/// `org.freedesktop.appearance accent-color`, if available.
#[cfg(any(feature = "wayland", feature = "x11"))]
pub fn portal_accent_color() -> Option<[f64; 3]> {
    xdg_desktop_portal::portal_accent_color()
}
