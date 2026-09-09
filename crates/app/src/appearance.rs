//! Native presentation choices shared by history, previews, and settings.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Pixels, Window, px, rgb};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

pub const DEFAULT_INTERFACE_TEXT_SIZE: u8 = 13;
pub const DEFAULT_CODE_TEXT_SIZE: u8 = 12;
pub const INTERFACE_TEXT_RANGE: std::ops::RangeInclusive<u8> = 11..=18;
pub const CODE_TEXT_RANGE: std::ops::RangeInclusive<u8> = 10..=24;

// One application appearance applies to every native window. Pixel helpers are
// also usable by canvas geometry and pure row-height consumers without a UI
// context. No preference writes or repository work happen through these reads.
static INTERFACE_TEXT_SIZE: AtomicU8 = AtomicU8::new(DEFAULT_INTERFACE_TEXT_SIZE);
static CODE_TEXT_SIZE: AtomicU8 = AtomicU8::new(DEFAULT_CODE_TEXT_SIZE);

pub fn ui_scale() -> f32 {
    f32::from(INTERFACE_TEXT_SIZE.load(Ordering::Relaxed)) / f32::from(DEFAULT_INTERFACE_TEXT_SIZE)
}
pub fn ui_size(base: f32) -> Pixels {
    px(base * ui_scale())
}
pub fn ui_text(base: f32) -> Pixels {
    ui_size(base)
}
pub fn code_text() -> Pixels {
    px(f32::from(CODE_TEXT_SIZE.load(Ordering::Relaxed)))
}
pub fn code_scale() -> f32 {
    f32::from(code_text()) / f32::from(DEFAULT_CODE_TEXT_SIZE)
}

pub fn apply_text_sizes(interface: u8, code: u8, window: &mut Window, cx: &mut App) {
    INTERFACE_TEXT_SIZE.store(
        interface.clamp(*INTERFACE_TEXT_RANGE.start(), *INTERFACE_TEXT_RANGE.end()),
        Ordering::Relaxed,
    );
    CODE_TEXT_SIZE.store(
        code.clamp(*CODE_TEXT_RANGE.start(), *CODE_TEXT_RANGE.end()),
        Ordering::Relaxed,
    );
    let theme = Theme::global_mut(cx);
    theme.font_size = ui_text(13.);
    theme.mono_font_size = code_text();
    Theme::sync_base(cx);
    // Root uses font_size for rem geometry, so native control padding and
    // heights grow together with explicit app text and custom canvas rows.
    window.set_rem_size(ui_text(13.));
    window.refresh();
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    Graphite,
    Daylight,
    TokyoNight,
    CatppuccinMocha,
    Nord,
    #[default]
    #[serde(other)]
    Midnight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub canvas: u32,
    pub panel: u32,
    pub subtle: u32,
    pub hover: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_foreground: u32,
    pub accent_hover: u32,
    pub accent_active: u32,
    pub selected: u32,
    pub added: u32,
    pub removed: u32,
    pub modified: u32,
    pub renamed: u32,
    pub warning: u32,
    pub added_background: u32,
    pub removed_background: u32,
    pub hunk: u32,
    pub line_number: u32,
}

impl Global for Palette {}

impl Palette {
    /// Preserve the selection while giving a selected, clickable row feedback.
    pub fn row_hover(self, selected: bool) -> u32 {
        if !selected {
            return self.hover;
        }
        let mut color = 0;
        for shift in [0, 8, 16] {
            let base = (self.selected >> shift) & 0xff;
            let accent = (self.accent >> shift) & 0xff;
            color |= ((base * 93 + accent * 7) / 100) << shift;
        }
        color
    }
}

pub fn palette(cx: &App) -> Palette {
    cx.try_global::<Palette>()
        .copied()
        .unwrap_or_else(|| ThemeChoice::default().palette())
}

impl ThemeChoice {
    pub const ALL: [Self; 6] = [
        Self::Midnight,
        Self::Daylight,
        Self::Graphite,
        Self::TokyoNight,
        Self::CatppuccinMocha,
        Self::Nord,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Graphite => "Graphite",
            Self::Daylight => "Daylight",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Nord => "Nord",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Midnight => "Deep slate · mint",
            Self::Daylight => "Soft white · evergreen",
            Self::Graphite => "Warm charcoal · lilac",
            Self::TokyoNight => "City blues · neon",
            Self::CatppuccinMocha => "Cozy pastels · mauve",
            Self::Nord => "Arctic blue · frost",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Midnight => Palette {
                canvas: 0x10151f,
                panel: 0x171e2b,
                subtle: 0x131a25,
                hover: 0x222c3c,
                border: 0x2b3749,
                text: 0xe8eef7,
                muted: 0xa4b1c5,
                accent: 0x75e0bb,
                accent_foreground: 0x0a241d,
                accent_hover: 0x99edcf,
                accent_active: 0x5ccca6,
                selected: 0x223b3b,
                added: 0x75e0bb,
                removed: 0xff95a8,
                modified: 0xe8bc79,
                renamed: 0xc1a5f5,
                warning: 0xe8bc79,
                added_background: 0x19322d,
                removed_background: 0x382531,
                hunk: 0x95baff,
                line_number: 0x899bb5,
            },
            Self::Graphite => Palette {
                canvas: 0x18191d,
                panel: 0x202126,
                subtle: 0x1c1d22,
                hover: 0x2b2c33,
                border: 0x3b3d47,
                text: 0xefeff4,
                muted: 0xa8a9b8,
                accent: 0xb7a5ff,
                accent_foreground: 0x211936,
                accent_hover: 0xcabaff,
                accent_active: 0xa994f0,
                selected: 0x38324e,
                added: 0x91ddb3,
                removed: 0xf5a0ac,
                modified: 0xe8c58c,
                renamed: 0xb7a5ff,
                warning: 0xe8c58c,
                added_background: 0x22352c,
                removed_background: 0x3e2830,
                hunk: 0xa8bfff,
                line_number: 0x9b9daa,
            },
            Self::Daylight => Palette {
                canvas: 0xf4f6fa,
                panel: 0xffffff,
                subtle: 0xedf1f7,
                hover: 0xe7ecf3,
                border: 0xd2dce8,
                text: 0x253247,
                muted: 0x526279,
                accent: 0x08755d,
                accent_foreground: 0xffffff,
                accent_hover: 0x06654f,
                accent_active: 0x055440,
                selected: 0xdaece6,
                added: 0x146744,
                removed: 0xb53351,
                modified: 0x875a14,
                renamed: 0x7850b4,
                warning: 0x875a14,
                added_background: 0xe2f1e9,
                removed_background: 0xf9e5eb,
                hunk: 0x3764ae,
                line_number: 0x61728a,
            },
            // Base hues follow the original palette; secondary text and status
            // surfaces are tuned for readable, small native UI labels.
            // https://github.com/tokyo-night/tokyo-night-vscode-theme
            Self::TokyoNight => Palette {
                canvas: 0x1a1b26,
                panel: 0x202231,
                subtle: 0x16161e,
                hover: 0x292e42,
                border: 0x373e59,
                text: 0xc0caf5,
                muted: 0xa9b1d6,
                accent: 0x7aa2f7,
                accent_foreground: 0x151c30,
                accent_hover: 0x99b9ff,
                accent_active: 0x7299e8,
                selected: 0x2c3552,
                added: 0x9ece6a,
                removed: 0xf7768e,
                modified: 0xe0af68,
                renamed: 0xbb9af7,
                warning: 0xe0af68,
                added_background: 0x26362c,
                removed_background: 0x3c2638,
                hunk: 0x7dcfff,
                line_number: 0x8997bd,
            },
            // https://catppuccin.com/palette/#mocha
            Self::CatppuccinMocha => Palette {
                canvas: 0x1e1e2e,
                panel: 0x242436,
                subtle: 0x181825,
                hover: 0x313244,
                border: 0x45475a,
                text: 0xcdd6f4,
                muted: 0xa6adc8,
                accent: 0xcba6f7,
                accent_foreground: 0x251b33,
                accent_hover: 0xddc0ff,
                accent_active: 0xba97e8,
                selected: 0x36324b,
                added: 0xa6e3a1,
                removed: 0xf38ba8,
                modified: 0xf9e2af,
                renamed: 0xcba6f7,
                warning: 0xf9e2af,
                added_background: 0x28392f,
                removed_background: 0x3e293d,
                hunk: 0x89b4fa,
                line_number: 0x9399b2,
            },
            // https://www.nordtheme.com/docs/colors-and-palettes
            Self::Nord => Palette {
                canvas: 0x2e3440,
                panel: 0x343c4b,
                subtle: 0x292f3b,
                hover: 0x3b4252,
                border: 0x4c566a,
                text: 0xeceff4,
                muted: 0xc0c9d8,
                accent: 0x88c0d0,
                accent_foreground: 0x243039,
                accent_hover: 0xa1d3df,
                accent_active: 0x80b5c7,
                selected: 0x3b485c,
                added: 0xa3be8c,
                removed: 0xe89aa3,
                modified: 0xebcb8b,
                renamed: 0xd0acd0,
                warning: 0xebcb8b,
                added_background: 0x303e39,
                removed_background: 0x45333f,
                hunk: 0x88c0d0,
                line_number: 0xa7b4c9,
            },
        }
    }

    /// Apply native controls and editor defaults together. Call after GPUI Kit
    /// initialization; callers invalidate/rebuild existing custom decorations.
    pub fn apply(self, window: Option<&mut Window>, cx: &mut App) {
        let palette = self.palette();
        Theme::change(
            if self == Self::Daylight {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            },
            None,
            cx,
        );
        self.configure(Theme::global_mut(cx));
        cx.set_global(palette);
        Theme::sync_base(cx);
        if let Some(window) = window {
            window.refresh();
        }
    }

    fn configure(self, theme: &mut Theme) {
        let palette = self.palette();
        theme.colors.background = rgb(palette.canvas).into();
        theme.colors.foreground = rgb(palette.text).into();
        theme.colors.muted = rgb(palette.hover).into();
        theme.colors.muted_foreground = rgb(palette.muted).into();
        theme.colors.primary = rgb(palette.accent).into();
        theme.colors.primary_foreground = rgb(palette.accent_foreground).into();
        theme.colors.primary_hover = rgb(palette.accent_hover).into();
        theme.colors.primary_active = rgb(palette.accent_active).into();
        theme.colors.border = rgb(palette.border).into();
        theme.colors.input = rgb(palette.border).into();
        theme.colors.selection = rgb(palette.selected).into();
        theme.colors.accent = rgb(palette.hover).into();
        theme.colors.accent_foreground = rgb(palette.text).into();
        theme.colors.button = rgb(palette.panel).into();
        theme.colors.button_hover = rgb(palette.hover).into();
        theme.colors.button_active = rgb(palette.selected).into();
        theme.colors.button_foreground = rgb(palette.text).into();
        theme.colors.button_primary = rgb(palette.accent).into();
        theme.colors.button_primary_foreground = rgb(palette.accent_foreground).into();
        theme.colors.button_primary_hover = rgb(palette.accent_hover).into();
        theme.colors.button_primary_active = rgb(palette.accent_active).into();
        theme.colors.secondary = rgb(palette.subtle).into();
        theme.colors.secondary_foreground = rgb(palette.text).into();
        theme.colors.secondary_hover = rgb(palette.hover).into();
        theme.colors.secondary_active = rgb(palette.selected).into();
        theme.colors.button_secondary = rgb(palette.subtle).into();
        theme.colors.button_secondary_foreground = rgb(palette.text).into();
        theme.colors.button_secondary_hover = rgb(palette.hover).into();
        theme.colors.button_secondary_active = rgb(palette.selected).into();
        theme.colors.list = rgb(palette.panel).into();
        theme.colors.list_head = rgb(palette.panel).into();
        theme.colors.list_hover = rgb(palette.hover).into();
        theme.colors.list_active = rgb(palette.selected).into();
        theme.colors.list_active_border = rgb(palette.accent).into();
        theme.colors.list_even = rgb(palette.subtle).into();
        theme.colors.popover = rgb(palette.panel).into();
        theme.colors.popover_foreground = rgb(palette.text).into();
        theme.colors.sidebar = rgb(palette.panel).into();
        theme.colors.sidebar_foreground = rgb(palette.text).into();
        theme.colors.sidebar_border = rgb(palette.border).into();
        theme.colors.sidebar_accent = rgb(palette.selected).into();
        theme.colors.sidebar_accent_foreground = rgb(palette.text).into();
        theme.colors.sidebar_primary = rgb(palette.accent).into();
        theme.colors.sidebar_primary_foreground = rgb(palette.accent_foreground).into();
        theme.colors.tab = rgb(palette.subtle).into();
        theme.colors.tab_foreground = rgb(palette.muted).into();
        theme.colors.tab_active = rgb(palette.panel).into();
        theme.colors.tab_active_foreground = rgb(palette.text).into();
        theme.colors.tab_bar = rgb(palette.subtle).into();
        theme.colors.tab_bar_segmented = rgb(palette.canvas).into();
        theme.colors.table = rgb(palette.canvas).into();
        theme.colors.table_head = rgb(palette.panel).into();
        theme.colors.table_head_foreground = rgb(palette.muted).into();
        theme.colors.table_hover = rgb(palette.hover).into();
        theme.colors.table_active = rgb(palette.selected).into();
        theme.colors.table_active_border = rgb(palette.accent).into();
        theme.colors.table_row_border = rgb(palette.border).into();
        theme.colors.table_even = rgb(palette.subtle).into();
        theme.colors.title_bar = rgb(palette.panel).into();
        theme.colors.title_bar_border = rgb(palette.border).into();
        theme.colors.status_bar = rgb(palette.subtle).into();
        theme.colors.status_bar_border = rgb(palette.border).into();
        theme.colors.window_border = rgb(palette.border).into();
        theme.colors.scrollbar = rgb(palette.canvas).into();
        theme.colors.scrollbar_thumb = rgb(palette.border).into();
        theme.colors.scrollbar_thumb_hover = rgb(palette.muted).into();
        theme.colors.switch = rgb(palette.border).into();
        theme.colors.switch_thumb = rgb(palette.text).into();
        theme.colors.progress_bar = rgb(palette.accent).into();
        theme.colors.skeleton = rgb(palette.hover).into();
        theme.colors.ring = rgb(palette.accent).into();
        theme.colors.caret = rgb(palette.accent).into();
        theme.colors.link = rgb(palette.hunk).into();
        theme.colors.link_hover = rgb(palette.accent).into();
        theme.colors.link_active = rgb(palette.accent).into();
        theme.colors.danger = rgb(palette.removed).into();
        theme.colors.danger_foreground = rgb(palette.canvas).into();
        theme.colors.success = rgb(palette.added).into();
        theme.colors.success_foreground = rgb(palette.canvas).into();
        theme.colors.warning = rgb(palette.warning).into();
        theme.colors.warning_foreground = rgb(palette.canvas).into();
        theme.colors.info = rgb(palette.hunk).into();
        theme.colors.info_foreground = rgb(palette.canvas).into();
        let syntax = Arc::make_mut(&mut theme.highlight_theme);
        syntax.style.editor_background = Some(rgb(palette.canvas).into());
        syntax.style.editor_gutter_background = Some(rgb(palette.canvas).into());
        syntax.style.editor_active_line = Some(rgb(palette.panel).into());
        syntax.style.editor_line_number = Some(rgb(palette.line_number).into());
        syntax.style.editor_foreground = Some(rgb(palette.text).into());
        theme.font_size = ui_text(13.);
        theme.mono_font_size = code_text();
        theme.radius = px(7.);
        theme.radius_lg = px(12.);
        // GPUI Kit 0.6 paints component backgrounds from resolved ThemeTokens,
        // while foregrounds still read ThemeColor. Sync both representations
        // before Theme::sync_base propagates them to native controls.
        theme.tokens = (&theme.colors).into();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    #[serde(other)]
    Comfortable,
}

impl Density {
    pub const ALL: [Self; 2] = [Self::Comfortable, Self::Compact];

    pub fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "Comfortable",
            Self::Compact => "Compact",
        }
    }

    pub fn history_row_height(self) -> f32 {
        ui_scale()
            * match self {
                Self::Comfortable => 34.,
                Self::Compact => 28.,
            }
    }

    pub fn file_row_height(self) -> f32 {
        ui_scale()
            * match self {
                Self::Comfortable => 44.,
                Self::Compact => 34.,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Background, Hsla};

    fn luminance(rgb: u32) -> f64 {
        let linear = |channel: u32| {
            let value = f64::from(channel) / 255.;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear((rgb >> 16) & 255)
            + 0.7152 * linear((rgb >> 8) & 255)
            + 0.0722 * linear(rgb & 255)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn palettes_keep_text_and_diff_content_readable_in_each_theme() {
        for choice in ThemeChoice::ALL {
            let palette = choice.palette();
            for background in [
                palette.canvas,
                palette.panel,
                palette.subtle,
                palette.hover,
                palette.selected,
                palette.row_hover(true),
            ] {
                assert!(
                    contrast(palette.text, background) >= 4.5,
                    "{choice:?} primary text"
                );
                assert!(
                    contrast(palette.muted, background) >= 4.5,
                    "{choice:?} secondary text"
                );
                for status in [
                    palette.added,
                    palette.removed,
                    palette.modified,
                    palette.renamed,
                ] {
                    assert!(contrast(status, background) >= 3., "{choice:?} status icon");
                }
            }
            assert!(contrast(palette.added, palette.added_background) >= 4.5);
            assert!(contrast(palette.removed, palette.removed_background) >= 4.5);
            for background in [palette.accent, palette.accent_hover, palette.accent_active] {
                assert!(
                    contrast(palette.accent_foreground, background) >= 4.5,
                    "{choice:?} action button label"
                );
            }
        }
    }

    #[test]
    fn theme_switch_updates_resolved_component_backgrounds_with_foregrounds() {
        // Reuse one theme to cover dark/light and dark/dark switches. Component
        // buttons read token backgrounds but legacy foreground colors, so a
        // palette-only assertion cannot catch a stale white primary button.
        let mut theme = Theme::default();
        for choice in ThemeChoice::ALL {
            choice.configure(&mut theme);
            let palette = choice.palette();
            let foreground: Hsla = rgb(palette.accent_foreground).into();
            assert_eq!(theme.colors.button_primary_foreground, foreground);
            for (token, expected) in [
                (theme.tokens.button_primary, palette.accent),
                (theme.tokens.button_primary_hover, palette.accent_hover),
                (theme.tokens.button_primary_active, palette.accent_active),
                (theme.tokens.button_secondary, palette.subtle),
                (theme.tokens.button_secondary_hover, palette.hover),
                (theme.tokens.button_secondary_active, palette.selected),
                (theme.tokens.list_active, palette.selected),
                (theme.tokens.popover, palette.panel),
                (theme.tokens.scrollbar_thumb, palette.border),
            ] {
                let color: Hsla = rgb(expected).into();
                assert_eq!(token.color, color, "{choice:?} resolved color");
                assert_eq!(
                    token.background,
                    Background::from(color),
                    "{choice:?} renderable background"
                );
            }
        }
    }

    #[test]
    fn appearance_choices_round_trip_and_unknown_choices_have_safe_defaults() {
        for choice in ThemeChoice::ALL {
            let encoded = serde_json::to_string(&choice).unwrap();
            assert_eq!(
                serde_json::from_str::<ThemeChoice>(&encoded).unwrap(),
                choice
            );
        }
        assert_eq!(
            serde_json::from_str::<ThemeChoice>("\"future-theme\"").unwrap(),
            ThemeChoice::Midnight
        );
        assert_eq!(
            serde_json::from_str::<Density>("\"future-density\"").unwrap(),
            Density::Comfortable
        );
        assert!(Density::Compact.history_row_height() < Density::Comfortable.history_row_height());
        assert!(Density::Compact.file_row_height() < Density::Comfortable.file_row_height());
    }
}
