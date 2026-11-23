//! GPUI adapter over [`gitturtle_prefs`].

pub use gitturtle_prefs::*;

use crate::appearance::{ResolvedThemeExt as _, ThemeSelectionExt as _};
use gitturtle_theme::custom::{CustomTheme, ResolvedTheme};

/// GPUI-aware theme resolution extension for [`AppSettings`].
pub trait AppSettingsExt {
    /// The theme to apply on this desktop: the rule of [`AppSettings::resolve_selection`]
    /// for the selection the desktop honours (`ThemeSelectionExt::on_desktop`), with the
    /// desktop's Omarchy palette on Linux.
    fn effective_theme(&self, cx: &gpui_kit::App, custom_themes: &[CustomTheme]) -> ResolvedTheme;
}

impl AppSettingsExt for AppSettings {
    fn effective_theme(&self, cx: &gpui_kit::App, custom_themes: &[CustomTheme]) -> ResolvedTheme {
        let system_is_light = matches!(
            cx.window_appearance(),
            gpui_kit::WindowAppearance::Light | gpui_kit::WindowAppearance::VibrantLight
        );
        self.resolve_selection(self.theme.on_desktop(cx), system_is_light, custom_themes)
            .with_desktop(cx)
            .with_system_accent(self.system_accent, cx)
    }
}
