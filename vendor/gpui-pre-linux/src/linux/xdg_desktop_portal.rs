//! Provides a [calloop] event source from [XDG Desktop Portal] events
//!
//! This module uses the [ashpd] crate

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use ashpd::desktop::{
    Color,
    settings::{ColorScheme, Settings},
};
use calloop::channel::{Channel, Sender};
use calloop::{EventSource, Poll, PostAction, Readiness, Token, TokenFactory};
use futures::channel::oneshot;
use futures::future::{BoxFuture, select};
use parking_lot::Mutex;
use smol::stream::StreamExt;

use gpui::{BackgroundExecutor, Task, WindowAppearance};

const MAX_CONFIG_FILE_BYTES: u64 = 64 * 1024;

#[derive(Default)]
struct PortalSnapshot {
    source_started: bool,
    appearance_ready: bool,
    appearance: Option<WindowAppearance>,
    accent_color: Option<[f64; 3]>,
    waiters: Vec<oneshot::Sender<()>>,
}

fn portal_snapshot() -> &'static Mutex<PortalSnapshot> {
    static SNAPSHOT: OnceLock<Mutex<PortalSnapshot>> = OnceLock::new();
    SNAPSHOT.get_or_init(|| Mutex::new(PortalSnapshot::default()))
}

pub(crate) fn mark_initial_appearance_applied(appearance: WindowAppearance) {
    let mut state = portal_snapshot().lock();
    state.appearance = Some(appearance);
    state.appearance_ready = true;
    for waiter in state.waiters.drain(..) {
        let _ = waiter.send(());
    }
}

fn record_accent_color(color: Color) {
    let rgb = [color.red(), color.green(), color.blue()];
    let valid = rgb
        .iter()
        .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel));
    portal_snapshot().lock().accent_color = valid.then_some(rgb);
}

/// Waits at most `timeout` for the initial Linux desktop appearance (from the
/// XDG settings portal or non-portal GTK/KDE config fallback) to be applied to
/// the active display connection. Returns immediately in headless/test runs.
pub fn wait_initial_portal_appearance(
    timeout: Duration,
) -> BoxFuture<'static, Option<WindowAppearance>> {
    Box::pin(async move {
        let rx = {
            let mut state = portal_snapshot().lock();
            if !state.source_started || state.appearance_ready {
                return state.appearance;
            }
            let (tx, rx) = oneshot::channel();
            state.waiters.push(tx);
            rx
        };
        let timer = smol::Timer::after(timeout);
        futures::pin_mut!(rx, timer);
        let _ = select(rx, timer).await;
        let state = portal_snapshot().lock();
        state.appearance
    })
}

/// Returns the latest sRGB accent color (`[r, g, b]` in `0.0..=1.0`) reported by
/// `org.freedesktop.appearance accent-color`, if available.
pub fn portal_accent_color() -> Option<[f64; 3]> {
    portal_snapshot().lock().accent_color
}

pub enum Event {
    WindowAppearance(WindowAppearance),
    AccentColor,
    #[cfg_attr(feature = "x11", allow(dead_code))]
    CursorTheme(String),
    #[cfg_attr(feature = "x11", allow(dead_code))]
    CursorSize(u32),
    ButtonLayout(String),
}

/// Forwards desktop settings from the portal to the event loop.
///
/// Owns the task that listens to the portal, so removing the source from the loop cancels it and
/// releases its D-Bus subscriptions.
pub struct XDPEventSource {
    channel: Channel<Event>,
    _listener: Task<()>,
}

impl XDPEventSource {
    pub fn new(executor: &BackgroundExecutor) -> Self {
        {
            let mut state = portal_snapshot().lock();
            state.source_started = true;
            state.appearance_ready = false;
        }
        let (sender, channel) = calloop::channel::channel();
        let listener = executor.spawn(async move {
            if let Err(error) = listen(sender).await {
                log::debug!("stopped listening to desktop portal settings: {error:#}");
            }
        });
        Self {
            channel,
            _listener: listener,
        }
    }
}

async fn listen(sender: Sender<Event>) -> anyhow::Result<()> {
    let settings = match Settings::new().await {
        Ok(settings) => settings,
        Err(error) => {
            let fallback = fallback_window_appearance().unwrap_or(WindowAppearance::Light);
            if sender.send(Event::WindowAppearance(fallback)).is_err() {
                mark_initial_appearance_applied(fallback);
            }
            if let Some(layout) = fallback_kde_button_layout() {
                let _ = sender.send(Event::ButtonLayout(layout));
            }
            return Err(error.into());
        }
    };

    let (color_scheme, accent_color) =
        futures::future::join(settings.color_scheme(), settings.accent_color()).await;
    if let Ok(accent) = accent_color {
        record_accent_color(accent);
    }

    let initial_appearance = match color_scheme {
        Ok(ColorScheme::PreferDark) => WindowAppearance::Dark,
        Ok(ColorScheme::PreferLight) => WindowAppearance::Light,
        Ok(ColorScheme::NoPreference) | Err(_) => {
            fallback_window_appearance().unwrap_or(WindowAppearance::Light)
        }
    };
    if sender.send(Event::WindowAppearance(initial_appearance)).is_err() {
        mark_initial_appearance_applied(initial_appearance);
        return Ok(());
    }

    if let Ok(initial_theme) = settings
        .read::<String>("org.gnome.desktop.interface", "cursor-theme")
        .await
    {
        sender.send(Event::CursorTheme(initial_theme))?;
    }

    // If u32 is used here, it throws invalid type error
    if let Ok(initial_size) = settings
        .read::<i32>("org.gnome.desktop.interface", "cursor-size")
        .await
    {
        sender.send(Event::CursorSize(initial_size as u32))?;
    }

    if let Ok(initial_layout) = settings
        .read::<String>("org.gnome.desktop.wm.preferences", "button-layout")
        .await
    {
        sender.send(Event::ButtonLayout(initial_layout))?;
    } else if let Some(kde_layout) = fallback_kde_button_layout() {
        sender.send(Event::ButtonLayout(kde_layout))?;
    }

    // Each listener runs until its stream or the channel closes. They're joined rather than
    // spawned, so that cancelling this future cancels them all.
    let mut listeners: Vec<BoxFuture<'_, anyhow::Result<()>>> = Vec::new();

    if let Ok(mut accent_changed) = settings.receive_accent_color_changed().await {
        let sender = sender.clone();
        listeners.push(Box::pin(async move {
            while let Some(color) = accent_changed.next().await {
                record_accent_color(color);
                sender.send(Event::AccentColor)?;
            }
            anyhow::Ok(())
        }));
    }

    if let Ok(mut cursor_theme_changed) = settings
        .receive_setting_changed_with_args("org.gnome.desktop.interface", "cursor-theme")
        .await
    {
        let sender = sender.clone();
        listeners.push(Box::pin(async move {
            while let Some(theme) = cursor_theme_changed.next().await {
                sender.send(Event::CursorTheme(theme?))?;
            }
            anyhow::Ok(())
        }));
    }

    if let Ok(mut cursor_size_changed) = settings
        .receive_setting_changed_with_args::<i32>("org.gnome.desktop.interface", "cursor-size")
        .await
    {
        let sender = sender.clone();
        listeners.push(Box::pin(async move {
            while let Some(size) = cursor_size_changed.next().await {
                sender.send(Event::CursorSize(size? as u32))?;
            }
            anyhow::Ok(())
        }));
    }

    if let Ok(mut button_layout_changed) = settings
        .receive_setting_changed_with_args("org.gnome.desktop.wm.preferences", "button-layout")
        .await
    {
        let sender = sender.clone();
        listeners.push(Box::pin(async move {
            while let Some(layout) = button_layout_changed.next().await {
                sender.send(Event::ButtonLayout(layout?))?;
            }
            anyhow::Ok(())
        }));
    }

    let mut appearance_changed = settings.receive_color_scheme_changed().await?;
    listeners.push(Box::pin(async move {
        while let Some(scheme) = appearance_changed.next().await {
            let appearance = match scheme {
                ColorScheme::PreferDark => WindowAppearance::Dark,
                ColorScheme::PreferLight => WindowAppearance::Light,
                ColorScheme::NoPreference => {
                    fallback_window_appearance().unwrap_or(WindowAppearance::Light)
                }
            };
            sender.send(Event::WindowAppearance(appearance))?;
        }
        anyhow::Ok(())
    }));

    for result in futures::future::join_all(listeners).await {
        result?;
    }
    Ok(())
}

fn xdg_config_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Some(dir);
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|home| home.join(".config"))
}

fn read_bounded_text(path: &Path) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_FILE_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_CONFIG_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_CONFIG_FILE_BYTES).then_some(text)
}

fn fallback_window_appearance() -> Option<WindowAppearance> {
    if let Ok(gtk_theme) = std::env::var("GTK_THEME")
        && let Some(appearance) = parse_gtk_theme_env(&gtk_theme)
    {
        return Some(appearance);
    }
    let config = xdg_config_home()?;
    for rel in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        if let Some(text) = read_bounded_text(&config.join(rel))
            && let Some(appearance) = parse_gtk_settings_ini(&text)
        {
            return Some(appearance);
        }
    }
    if let Some(text) = read_bounded_text(&config.join("kdeglobals"))
        && let Some(appearance) = parse_kdeglobals_appearance(&text)
    {
        return Some(appearance);
    }
    None
}

fn parse_gtk_theme_env(value: &str) -> Option<WindowAppearance> {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.ends_with(":dark") || trimmed.ends_with("-dark") || trimmed == "dark" {
        Some(WindowAppearance::Dark)
    } else if trimmed.ends_with(":light") || trimmed.ends_with("-light") {
        Some(WindowAppearance::Light)
    } else {
        None
    }
}

fn parse_gtk_settings_ini(text: &str) -> Option<WindowAppearance> {
    let mut in_settings = false;
    let mut theme_fallback = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_settings = line.eq_ignore_ascii_case("[Settings]");
            continue;
        }
        if !in_settings {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.eq_ignore_ascii_case("gtk-application-prefer-dark-theme") {
            if matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            ) {
                return Some(WindowAppearance::Dark);
            }
            if matches!(
                value.to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            ) {
                return Some(WindowAppearance::Light);
            }
        } else if key.eq_ignore_ascii_case("gtk-theme-name") {
            theme_fallback = parse_gtk_theme_env(value);
        }
    }
    theme_fallback
}

fn parse_kdeglobals_appearance(text: &str) -> Option<WindowAppearance> {
    let mut section = "";
    let mut scheme_hint = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if section.eq_ignore_ascii_case("Colors:Window")
            && key.eq_ignore_ascii_case("BackgroundNormal")
        {
            let mut parts = value.split(',').map(|part| part.trim().parse::<u16>().ok());
            if let (Some(Some(r)), Some(Some(g)), Some(Some(b)), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
                && r <= 255
                && g <= 255
                && b <= 255
            {
                let luma = (299 * r as u32 + 587 * g as u32 + 114 * b as u32) / 1000;
                return Some(if luma < 128 {
                    WindowAppearance::Dark
                } else {
                    WindowAppearance::Light
                });
            }
        } else if section.eq_ignore_ascii_case("General") && key.eq_ignore_ascii_case("ColorScheme")
        {
            let lower = value.to_ascii_lowercase();
            if lower.contains("dark") {
                scheme_hint = Some(WindowAppearance::Dark);
            } else if lower.contains("light") {
                scheme_hint = Some(WindowAppearance::Light);
            }
        }
    }
    scheme_hint
}

fn fallback_kde_button_layout() -> Option<String> {
    let config = xdg_config_home()?;
    let text = read_bounded_text(&config.join("kwinrc"))?;
    parse_kwinrc_button_layout(&text)
}

fn parse_kwinrc_button_layout(text: &str) -> Option<String> {
    let mut in_decoration = false;
    let mut left: Option<&str> = None;
    let mut right: Option<&str> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_decoration = line.eq_ignore_ascii_case("[org.kde.kdecoration2]");
            continue;
        }
        if !in_decoration {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "ButtonsOnLeft" => left = Some(value.trim()),
            "ButtonsOnRight" => right = Some(value.trim()),
            _ => {}
        }
    }
    if left.is_none() && right.is_none() {
        return None;
    }
    let map_side = |codes: &str| -> String {
        codes
            .chars()
            .filter_map(|ch| match ch {
                'M' => Some("menu"),
                'I' => Some("minimize"),
                'A' => Some("maximize"),
                'X' => Some("close"),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    // KConfig omits a side when it matches KDE's defaults (`MS` on the left, `HIA__X` on the right),
    // while an explicitly cleared side writes an empty value (`ButtonsOnRight=`).
    let left_buttons = map_side(left.unwrap_or("MS"));
    let right_buttons = map_side(right.unwrap_or("HIA__X"));
    if left_buttons.is_empty() && right_buttons.is_empty() {
        return None;
    }
    Some(format!("{left_buttons}:{right_buttons}"))
}

impl EventSource for XDPEventSource {
    type Event = Event;
    type Metadata = ();
    type Ret = ();
    type Error = anyhow::Error;

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        token: Token,
        mut callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        self.channel.process_events(readiness, token, |evt, _| {
            if let calloop::channel::Event::Msg(msg) = evt {
                (callback)(msg, &mut ())
            }
        })?;

        Ok(PostAction::Continue)
    }

    fn register(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        self.channel.register(poll, token_factory)?;

        Ok(())
    }

    fn reregister(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        self.channel.reregister(poll, token_factory)?;

        Ok(())
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        self.channel.unregister(poll)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gtk_theme_env_and_settings_ini() {
        assert_eq!(
            parse_gtk_theme_env("Adwaita:dark"),
            Some(WindowAppearance::Dark)
        );
        assert_eq!(
            parse_gtk_theme_env("Breeze-Dark"),
            Some(WindowAppearance::Dark)
        );
        assert_eq!(
            parse_gtk_theme_env("Adwaita:light"),
            Some(WindowAppearance::Light)
        );
        assert_eq!(parse_gtk_theme_env("Adwaita"), None);

        assert_eq!(
            parse_gtk_settings_ini("[Settings]\ngtk-application-prefer-dark-theme=1\n"),
            Some(WindowAppearance::Dark)
        );
        assert_eq!(
            parse_gtk_settings_ini("[Settings]\ngtk-application-prefer-dark-theme = false\n"),
            Some(WindowAppearance::Light)
        );
        assert_eq!(
            parse_gtk_settings_ini("[Settings]\ngtk-theme-name=Arc-Dark\n"),
            Some(WindowAppearance::Dark)
        );
    }

    #[test]
    fn parses_kdeglobals_appearance_and_kwinrc_buttons() {
        assert_eq!(
            parse_kdeglobals_appearance("[Colors:Window]\nBackgroundNormal=30,33,39\n"),
            Some(WindowAppearance::Dark)
        );
        assert_eq!(
            parse_kdeglobals_appearance("[Colors:Window]\nBackgroundNormal=239,240,241\n"),
            Some(WindowAppearance::Light)
        );
        assert_eq!(
            parse_kdeglobals_appearance("[General]\nColorScheme=BreezeDark\n"),
            Some(WindowAppearance::Dark)
        );
        assert_eq!(
            parse_kwinrc_button_layout(
                "[org.kde.kdecoration2]\nButtonsOnLeft=MS\nButtonsOnRight=FIAX\n"
            ),
            Some("menu:minimize,maximize,close".to_string())
        );
        assert_eq!(
            parse_kwinrc_button_layout(
                "[org.kde.kdecoration2]\nButtonsOnLeft=XIA\nButtonsOnRight=\n"
            ),
            Some("close,minimize,maximize:".to_string())
        );
        // Single-side customization preserves KDE's default buttons on the omitted side.
        assert_eq!(
            parse_kwinrc_button_layout("[org.kde.kdecoration2]\nButtonsOnLeft=M\n"),
            Some("menu:minimize,maximize,close".to_string())
        );
        assert_eq!(
            parse_kwinrc_button_layout("[org.kde.kdecoration2]\nButtonsOnRight=X\n"),
            Some("menu:close".to_string())
        );
        assert_eq!(parse_kwinrc_button_layout("[Compositing]\nEnabled=true\n"), None);
    }
}
