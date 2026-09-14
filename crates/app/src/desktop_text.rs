//! Desktop text preferences on Linux: glyph antialiasing and text scaling.
//!
//! The toolkit's Linux backend always recommends subpixel (RGB stripe) glyph
//! rendering and never consults the session's font settings. That fringes on
//! panels without a horizontal RGB stripe, such as WOLED and QD-OLED monitors
//! or rotated screens, and diverges from GTK 4, which renders every glyph
//! grayscale. This module reads the desktop preference through the XDG
//! settings portal, falls back to fontconfig's resolved defaults when a portal
//! backend does not publish GNOME's keys, and follows changes while the app
//! runs. On Wayland it also applies the desktop text scaling factor ("Large
//! Text"), where the toolkit has no DPI source; the X11 backend already scales
//! the whole window through `Xft.dpi`. Nothing here writes a setting.

use gpui_kit::AsyncApp;

/// Resolves once the first desktop snapshot has been applied, so the first
/// frame already uses the session's antialiasing and text scale. It gives up
/// after a short wait so a stalled portal cannot delay startup.
#[cfg(target_os = "linux")]
pub(super) struct Initial(futures::channel::oneshot::Receiver<()>);
#[cfg(not(target_os = "linux"))]
pub(super) struct Initial;

#[cfg(target_os = "linux")]
const STARTUP_WAIT: std::time::Duration = std::time::Duration::from_millis(400);

#[cfg(target_os = "linux")]
pub(super) fn start(cx: &mut gpui_kit::App) -> Initial {
    use futures::StreamExt as _;

    let (sender, mut snapshots) = futures::channel::mpsc::unbounded();
    let (ready, initial) = futures::channel::oneshot::channel();
    let scales_text = gpui_kit::guess_compositor() == "Wayland";
    cx.background_executor()
        .spawn(portal::observe(scales_text, sender))
        .detach();
    cx.spawn(async move |cx| {
        let mut ready = Some(ready);
        while let Some(snapshot) = snapshots.next().await {
            cx.update(|cx| apply(snapshot, cx));
            if let Some(ready) = ready.take() {
                let _ = ready.send(());
            }
        }
    })
    .detach();
    Initial(initial)
}
#[cfg(not(target_os = "linux"))]
pub(super) fn start(_cx: &mut gpui_kit::App) -> Initial {
    Initial
}

#[cfg(target_os = "linux")]
pub(super) async fn ready(initial: Initial, cx: &AsyncApp) {
    let timeout = cx.background_executor().timer(STARTUP_WAIT);
    futures::future::select(Box::pin(initial.0), Box::pin(timeout)).await;
}
#[cfg(not(target_os = "linux"))]
pub(super) async fn ready(_initial: Initial, _cx: &AsyncApp) {}

#[cfg(target_os = "linux")]
fn apply(snapshot: DesktopText, cx: &mut gpui_kit::App) {
    let mut changed = crate::appearance::set_desktop_text_scale(snapshot.text_scale, cx);
    if cx.text_rendering_mode() != snapshot.rendering {
        cx.set_text_rendering_mode(snapshot.rendering);
        changed = true;
    }
    if changed {
        cx.refresh_windows();
    }
}

/// The session's resolved text preferences.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DesktopText {
    pub rendering: gpui_kit::TextRenderingMode,
    pub text_scale: f32,
}

/// Raw values as reported by the desktop; each source may be absent.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Observed {
    /// GNOME `font-rendering`: "automatic" (GNOME 47+ default) or "manual".
    pub font_rendering: Option<String>,
    /// GNOME `font-antialiasing`: "rgba", "grayscale" or "none".
    pub antialiasing: Option<String>,
    /// GNOME `text-scaling-factor`.
    pub text_scale: Option<f64>,
    /// fontconfig's `antialias|rgba` defaults for the generic sans family.
    pub fontconfig: Option<String>,
}

#[cfg(any(target_os = "linux", test))]
impl Observed {
    /// Prefers the desktop's own preference, then fontconfig, then grayscale:
    /// the one mode that cannot fringe on an unknown panel layout and matches
    /// GTK 4 on the same desktop. Text scaling applies only where the toolkit
    /// would not otherwise scale the window.
    pub(super) fn resolve(&self, scales_text: bool) -> DesktopText {
        let rendering =
            rendering_from_gnome(self.font_rendering.as_deref(), self.antialiasing.as_deref())
                .or_else(|| {
                    self.fontconfig
                        .as_deref()
                        .and_then(rendering_from_fontconfig)
                })
                .unwrap_or(gpui_kit::TextRenderingMode::Grayscale);
        let text_scale = if scales_text {
            self.text_scale
                .map(crate::appearance::desktop_text_scale_value)
                .unwrap_or(1.0)
        } else {
            1.0
        };
        DesktopText {
            rendering,
            text_scale,
        }
    }
}

/// GNOME's `font-rendering` "automatic" delegates the choice to the toolkit;
/// GTK 4 then renders grayscale regardless of `font-antialiasing`, so the app
/// matches its neighbours. "manual" (and desktops without the newer key)
/// follows the low-level antialiasing preference. The toolkit cannot disable
/// antialiasing, so "none" is rendered grayscale.
#[cfg(any(target_os = "linux", test))]
pub(super) fn rendering_from_gnome(
    font_rendering: Option<&str>,
    antialiasing: Option<&str>,
) -> Option<gpui_kit::TextRenderingMode> {
    use gpui_kit::TextRenderingMode::{Grayscale, Subpixel};
    if font_rendering == Some("automatic") {
        return Some(Grayscale);
    }
    match antialiasing? {
        "rgba" => Some(Subpixel),
        "grayscale" | "none" => Some(Grayscale),
        _ => None,
    }
}

/// Parses `fc-match --format '%{antialias}|%{rgba}'`. `rgba` uses fontconfig's
/// constants: 0 unknown, 1 rgb, 2 bgr, 3 vrgb, 4 vbgr, 5 none. The toolkit
/// renders horizontal stripes only and takes their order from the compositor,
/// so every other layout is rendered grayscale.
#[cfg(any(target_os = "linux", test))]
pub(super) fn rendering_from_fontconfig(defaults: &str) -> Option<gpui_kit::TextRenderingMode> {
    use gpui_kit::TextRenderingMode::{Grayscale, Subpixel};
    let mut fields = defaults.trim().split('|');
    let antialias = match fields.next()?.trim() {
        "True" | "true" | "1" => true,
        "False" | "false" | "0" => false,
        _ => return None,
    };
    let rgba: u8 = fields.next()?.trim().parse().ok()?;
    Some(match (antialias, rgba) {
        (true, 1 | 2) => Subpixel,
        (true, 0 | 3 | 4 | 5) | (false, 0..=5) => Grayscale,
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
mod portal {
    use super::{DesktopText, Observed};
    use ashpd::desktop::settings::Settings;
    use futures::channel::mpsc::UnboundedSender;
    use futures::{Stream, StreamExt as _};
    use std::pin::Pin;
    use std::process::{Command, Stdio};

    const INTERFACE: &str = "org.gnome.desktop.interface";
    const FONT_RENDERING: &str = "font-rendering";
    const FONT_ANTIALIASING: &str = "font-antialiasing";
    const TEXT_SCALING_FACTOR: &str = "text-scaling-factor";

    enum Change {
        FontRendering(String),
        Antialiasing(String),
        TextScale(f64),
    }

    /// Sends one snapshot after the initial reads, then one per change until
    /// the receiver is dropped. Runs on the background executor; portal calls
    /// and the fontconfig query never touch the UI thread.
    pub(super) async fn observe(scales_text: bool, sender: UnboundedSender<DesktopText>) {
        let mut observed = Observed::default();
        let settings = Settings::new().await.ok();
        if let Some(settings) = &settings {
            observed.font_rendering = settings.read(INTERFACE, FONT_RENDERING).await.ok();
            observed.antialiasing = settings.read(INTERFACE, FONT_ANTIALIASING).await.ok();
            observed.text_scale = settings.read(INTERFACE, TEXT_SCALING_FACTOR).await.ok();
        }
        if observed.antialiasing.is_none() {
            observed.fontconfig = fontconfig_defaults();
        }
        if sender
            .unbounded_send(observed.resolve(scales_text))
            .is_err()
        {
            return;
        }
        let Some(settings) = settings else {
            return;
        };

        let mut streams: Vec<Pin<Box<dyn Stream<Item = Change> + Send>>> = Vec::new();
        if let Ok(changes) = settings
            .receive_setting_changed_with_args::<String>(INTERFACE, FONT_RENDERING)
            .await
        {
            streams.push(Box::pin(changes.filter_map(|value| {
                futures::future::ready(value.ok().map(Change::FontRendering))
            })));
        }
        if let Ok(changes) = settings
            .receive_setting_changed_with_args::<String>(INTERFACE, FONT_ANTIALIASING)
            .await
        {
            streams.push(Box::pin(changes.filter_map(|value| {
                futures::future::ready(value.ok().map(Change::Antialiasing))
            })));
        }
        if let Ok(changes) = settings
            .receive_setting_changed_with_args::<f64>(INTERFACE, TEXT_SCALING_FACTOR)
            .await
        {
            streams.push(Box::pin(changes.filter_map(|value| {
                futures::future::ready(value.ok().map(Change::TextScale))
            })));
        }
        let mut changes = futures::stream::select_all(streams);
        while let Some(change) = changes.next().await {
            match change {
                Change::FontRendering(value) => observed.font_rendering = Some(value),
                Change::Antialiasing(value) => observed.antialiasing = Some(value),
                Change::TextScale(value) => observed.text_scale = Some(value),
            }
            if sender
                .unbounded_send(observed.resolve(scales_text))
                .is_err()
            {
                return;
            }
        }
    }

    /// fontconfig is how KDE and other desktops publish their antialiasing
    /// choice. The `fontconfig` package already belongs to the runtime
    /// requirements; a missing tool simply leaves the fallback empty.
    fn fontconfig_defaults() -> Option<String> {
        let output = Command::new("fc-match")
            .args(["--format", "%{antialias}|%{rgba}", "sans-serif"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TextRenderingMode::{Grayscale, Subpixel};

    #[test]
    fn gnome_manual_mode_follows_the_antialiasing_key() {
        assert_eq!(
            rendering_from_gnome(Some("manual"), Some("rgba")),
            Some(Subpixel)
        );
        assert_eq!(
            rendering_from_gnome(Some("manual"), Some("grayscale")),
            Some(Grayscale)
        );
        assert_eq!(
            rendering_from_gnome(Some("manual"), Some("none")),
            Some(Grayscale)
        );
        // Desktops without the GNOME 47 key still publish the low-level one.
        assert_eq!(rendering_from_gnome(None, Some("rgba")), Some(Subpixel));
        assert_eq!(
            rendering_from_gnome(None, Some("grayscale")),
            Some(Grayscale)
        );
    }

    #[test]
    fn gnome_automatic_mode_renders_grayscale_like_gtk4() {
        assert_eq!(
            rendering_from_gnome(Some("automatic"), Some("rgba")),
            Some(Grayscale)
        );
        assert_eq!(
            rendering_from_gnome(Some("automatic"), None),
            Some(Grayscale)
        );
    }

    #[test]
    fn unknown_gnome_values_defer_to_the_next_source() {
        assert_eq!(rendering_from_gnome(None, None), None);
        assert_eq!(rendering_from_gnome(Some("manual"), None), None);
        assert_eq!(rendering_from_gnome(Some("manual"), Some("lcd-v")), None);
        assert_eq!(
            rendering_from_gnome(Some("later"), Some("rgba")),
            Some(Subpixel)
        );
    }

    #[test]
    fn fontconfig_defaults_map_horizontal_stripes_only() {
        assert_eq!(rendering_from_fontconfig("True|1\n"), Some(Subpixel));
        assert_eq!(rendering_from_fontconfig("True|2"), Some(Subpixel));
        assert_eq!(rendering_from_fontconfig("True|3"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig("True|4"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig("True|0"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig("True|5"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig("False|1"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig(""), None);
        assert_eq!(rendering_from_fontconfig("Maybe|1"), None);
        assert_eq!(rendering_from_fontconfig("True|rgb"), None);
        assert_eq!(rendering_from_fontconfig("True|9"), None);
    }

    #[test]
    fn resolution_prefers_desktop_then_fontconfig_then_grayscale() {
        let desktop = Observed {
            font_rendering: Some("manual".into()),
            antialiasing: Some("rgba".into()),
            text_scale: Some(1.25),
            fontconfig: Some("True|5".into()),
        };
        assert_eq!(
            desktop.resolve(true),
            DesktopText {
                rendering: Subpixel,
                text_scale: 1.25,
            }
        );
        let kde = Observed {
            fontconfig: Some("True|1".into()),
            ..Observed::default()
        };
        assert_eq!(kde.resolve(true).rendering, Subpixel);
        assert_eq!(Observed::default().resolve(true).rendering, Grayscale);
    }

    #[test]
    fn text_scaling_applies_on_wayland_only_and_stays_bounded() {
        let observed = Observed {
            text_scale: Some(1.5),
            ..Observed::default()
        };
        assert_eq!(observed.resolve(true).text_scale, 1.5);
        // The X11 backend already scales the window through Xft.dpi.
        assert_eq!(observed.resolve(false).text_scale, 1.0);
        let huge = Observed {
            text_scale: Some(40.0),
            ..Observed::default()
        };
        assert_eq!(huge.resolve(true).text_scale, 3.0);
        let broken = Observed {
            text_scale: Some(f64::NAN),
            ..Observed::default()
        };
        assert_eq!(broken.resolve(true).text_scale, 1.0);
        assert_eq!(Observed::default().resolve(true).text_scale, 1.0);
    }
}
