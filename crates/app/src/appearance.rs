//! Native presentation choices shared by history, previews, and settings.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Window, px, rgb};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    Graphite,
    Daylight,
    #[default]
    #[serde(other)]
    Midnight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub canvas: u32,
    pub panel: u32,
    pub hover: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub selected: u32,
    pub added: u32,
    pub removed: u32,
    pub added_background: u32,
    pub removed_background: u32,
    pub hunk: u32,
    pub line_number: u32,
}

impl Global for Palette {}

pub fn palette(cx: &App) -> Palette {
    cx.try_global::<Palette>()
        .copied()
        .unwrap_or_else(|| ThemeChoice::default().palette())
}

impl ThemeChoice {
    pub const ALL: [Self; 3] = [Self::Midnight, Self::Graphite, Self::Daylight];

    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Graphite => "Graphite",
            Self::Daylight => "Daylight",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Midnight => Palette {
                canvas: 0x0f171c,
                panel: 0x121e24,
                hover: 0x19272e,
                border: 0x293b43,
                text: 0xdee9ed,
                muted: 0x9aaeb8,
                accent: 0x7adfb4,
                selected: 0x1c3b36,
                added: 0x7adfb4,
                removed: 0xf29aa2,
                added_background: 0x152d26,
                removed_background: 0x342329,
                hunk: 0x8db7f6,
                line_number: 0x8199a4,
            },
            Self::Graphite => Palette {
                canvas: 0x18191d,
                panel: 0x202126,
                hover: 0x2b2c33,
                border: 0x3b3d47,
                text: 0xefeff4,
                muted: 0xa8a9b8,
                accent: 0xb7a5ff,
                selected: 0x38324e,
                added: 0x91ddb3,
                removed: 0xf5a0ac,
                added_background: 0x22352c,
                removed_background: 0x3e2830,
                hunk: 0xa8bfff,
                line_number: 0x9b9daa,
            },
            Self::Daylight => Palette {
                canvas: 0xf7f9fb,
                panel: 0xffffff,
                hover: 0xe9eef3,
                border: 0xcbd5df,
                text: 0x1f2d3a,
                muted: 0x526575,
                accent: 0x146c53,
                selected: 0xd4ebe2,
                added: 0x176440,
                removed: 0xa42d43,
                added_background: 0xe1f2e8,
                removed_background: 0xfbe4e8,
                hunk: 0x315da7,
                line_number: 0x5c7182,
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
        {
            let theme = Theme::global_mut(cx);
            theme.colors.background = rgb(palette.canvas).into();
            theme.colors.foreground = rgb(palette.text).into();
            theme.colors.muted = rgb(palette.hover).into();
            theme.colors.muted_foreground = rgb(palette.muted).into();
            theme.colors.primary = rgb(palette.accent).into();
            theme.colors.primary_foreground = rgb(palette.panel).into();
            theme.colors.primary_hover = rgb(palette.accent).into();
            theme.colors.primary_active = rgb(palette.accent).into();
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
            theme.colors.button_primary_foreground = rgb(palette.panel).into();
            theme.colors.button_primary_hover = rgb(palette.accent).into();
            theme.colors.button_primary_active = rgb(palette.accent).into();
            theme.colors.list = rgb(palette.panel).into();
            theme.colors.list_head = rgb(palette.panel).into();
            theme.colors.list_hover = rgb(palette.hover).into();
            theme.colors.list_active = rgb(palette.selected).into();
            theme.colors.popover = rgb(palette.panel).into();
            theme.colors.popover_foreground = rgb(palette.text).into();
            theme.colors.sidebar = rgb(palette.panel).into();
            theme.colors.sidebar_foreground = rgb(palette.text).into();
            theme.colors.sidebar_border = rgb(palette.border).into();
            theme.colors.ring = rgb(palette.accent).into();
            theme.colors.caret = rgb(palette.accent).into();
            theme.colors.link = rgb(palette.hunk).into();
            let syntax = Arc::make_mut(&mut theme.highlight_theme);
            syntax.style.editor_background = Some(rgb(palette.canvas).into());
            syntax.style.editor_gutter_background = Some(rgb(palette.canvas).into());
            syntax.style.editor_active_line = Some(rgb(palette.panel).into());
            syntax.style.editor_line_number = Some(rgb(palette.line_number).into());
            syntax.style.editor_foreground = Some(rgb(palette.text).into());
            theme.font_size = px(13.);
            theme.mono_font_size = px(12.);
            theme.radius = px(5.);
        }
        cx.set_global(palette);
        Theme::sync_base(cx);
        if let Some(window) = window {
            window.refresh();
        }
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
        match self {
            Self::Comfortable => 34.,
            Self::Compact => 28.,
        }
    }

    pub fn file_row_height(self) -> f32 {
        match self {
            Self::Comfortable => 44.,
            Self::Compact => 34.,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            for background in [palette.canvas, palette.panel, palette.selected] {
                assert!(
                    contrast(palette.text, background) >= 4.5,
                    "{choice:?} primary text"
                );
                assert!(
                    contrast(palette.muted, background) >= 4.5,
                    "{choice:?} secondary text"
                );
            }
            assert!(contrast(palette.added, palette.added_background) >= 4.5);
            assert!(contrast(palette.removed, palette.removed_background) >= 4.5);
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
