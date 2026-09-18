//! Native presentation choices shared by history, previews, and settings.

use gpui_kit::component::{Colorize, Theme, ThemeMode};
use gpui_kit::{App, Global, Pixels, Window, px, rgb};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU32, Ordering},
};

// Upstream values for the adapted built-in themes. Families whose themes have
// not landed yet are referenced only by the source tests.
#[cfg_attr(not(test), allow(dead_code))]
mod sources;

// The preference store reads the custom theme model; the theme editor and picker
// will consume the readability rules, token names and document format. Until
// they land, the tests are the only readers of most of them.
#[cfg_attr(not(test), allow(dead_code))]
pub mod custom;

pub const DEFAULT_INTERFACE_TEXT_SIZE: u8 = 13;
pub const DEFAULT_CODE_TEXT_SIZE: u8 = 12;
pub const INTERFACE_TEXT_RANGE: std::ops::RangeInclusive<u8> = 11..=18;
pub const CODE_TEXT_RANGE: std::ops::RangeInclusive<u8> = 10..=24;
/// Bounds for the desktop's own text scaling factor (GNOME "Large Text").
#[cfg(any(target_os = "linux", test))]
pub const DESKTOP_TEXT_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;

// One application appearance applies to every native window. Pixel helpers are
// also usable by canvas geometry and pure row-height consumers without a UI
// context. No preference writes or repository work happen through these reads.
#[cfg(not(test))]
static INTERFACE_TEXT_SIZE: AtomicU8 = AtomicU8::new(DEFAULT_INTERFACE_TEXT_SIZE);
#[cfg(not(test))]
static CODE_TEXT_SIZE: AtomicU8 = AtomicU8::new(DEFAULT_CODE_TEXT_SIZE);
// The desktop's text scaling factor multiplies both app sizes so the saved
// interface/code preferences keep their meaning across desktops. Only the
// Linux desktop bridge changes it; other platforms scale through the toolkit.
#[cfg(not(test))]
static DESKTOP_TEXT_SCALE: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

// Each GPUI test owns its application on one test thread. Keep its simulated
// appearance there too: changing the font size must not move another test's
// controls between measuring their bounds and dispatching a click. Production
// continues to share the atomic values across the application's native windows.
#[cfg(test)]
thread_local! {
    static INTERFACE_TEXT_SIZE: AtomicU8 = const { AtomicU8::new(DEFAULT_INTERFACE_TEXT_SIZE) };
    static CODE_TEXT_SIZE: AtomicU8 = const { AtomicU8::new(DEFAULT_CODE_TEXT_SIZE) };
    static DESKTOP_TEXT_SCALE: AtomicU32 = const { AtomicU32::new(1.0f32.to_bits()) };
}

fn with_text_sizes<R>(read: impl FnOnce(&AtomicU8, &AtomicU8, &AtomicU32) -> R) -> R {
    #[cfg(not(test))]
    {
        read(&INTERFACE_TEXT_SIZE, &CODE_TEXT_SIZE, &DESKTOP_TEXT_SCALE)
    }
    #[cfg(test)]
    {
        INTERFACE_TEXT_SIZE.with(|interface| {
            CODE_TEXT_SIZE
                .with(|code| DESKTOP_TEXT_SCALE.with(|desktop| read(interface, code, desktop)))
        })
    }
}

fn interface_text_size() -> u8 {
    with_text_sizes(|interface, _, _| interface.load(Ordering::Relaxed))
}

fn code_text_size() -> u8 {
    with_text_sizes(|_, code, _| code.load(Ordering::Relaxed))
}

/// Per-app notification for retained workspace viewports. Observers keep their
/// last applied factor so coalesced desktop updates are applied exactly once.
#[cfg(any(target_os = "linux", test))]
pub(super) struct DesktopTextScale(pub f32);
#[cfg(any(target_os = "linux", test))]
impl Global for DesktopTextScale {}

pub fn desktop_text_scale() -> f32 {
    with_text_sizes(|_, _, desktop| f32::from_bits(desktop.load(Ordering::Relaxed)))
}
/// Normalizes a reported factor: non-positive/non-finite values mean "no scaling".
#[cfg(any(target_os = "linux", test))]
pub fn desktop_text_scale_value(scale: impl Into<f64>) -> f32 {
    let scale = scale.into();
    if scale.is_finite() && scale > 0.0 {
        (scale as f32).clamp(
            *DESKTOP_TEXT_SCALE_RANGE.start(),
            *DESKTOP_TEXT_SCALE_RANGE.end(),
        )
    } else {
        1.0
    }
}
/// Stores the desktop factor and re-applies the text sizes of every open
/// window. Returns whether the factor changed.
#[cfg(target_os = "linux")]
pub fn set_desktop_text_scale(scale: f32, cx: &mut App) -> bool {
    let scale = desktop_text_scale_value(scale);
    if scale.to_bits() == desktop_text_scale().to_bits() {
        return false;
    }
    with_text_sizes(|_, _, desktop| desktop.store(scale.to_bits(), Ordering::Relaxed));
    cx.set_global(DesktopTextScale(scale));
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, cx| {
            apply_text_sizes(interface_text_size(), code_text_size(), window, cx)
        });
    }
    true
}

pub fn ui_scale() -> f32 {
    f32::from(interface_text_size()) / f32::from(DEFAULT_INTERFACE_TEXT_SIZE) * desktop_text_scale()
}
pub fn ui_size(base: f32) -> Pixels {
    px(base * ui_scale())
}
pub fn ui_text(base: f32) -> Pixels {
    ui_size(base)
}
pub fn code_text() -> Pixels {
    px(f32::from(code_text_size()) * desktop_text_scale())
}
pub fn code_scale() -> f32 {
    f32::from(code_text()) / f32::from(DEFAULT_CODE_TEXT_SIZE)
}

pub fn apply_text_sizes(interface: u8, code: u8, window: &mut Window, cx: &mut App) {
    with_text_sizes(|interface_size, code_size, _| {
        interface_size.store(
            interface.clamp(*INTERFACE_TEXT_RANGE.start(), *INTERFACE_TEXT_RANGE.end()),
            Ordering::Relaxed,
        );
        code_size.store(
            code.clamp(*CODE_TEXT_RANGE.start(), *CODE_TEXT_RANGE.end()),
            Ordering::Relaxed,
        );
    });
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
    Porcelain,
    Sandstone,
    DeepSea,
    Ember,
    SolarizedDark,
    SolarizedLight,
    OneDark,
    OneLight,
    RosePine,
    RosePineDawn,
    Dracula,
    Alucard,
    KanagawaWave,
    KanagawaLotus,
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
    pub const ALL: [Self; 20] = [
        Self::Midnight,
        Self::Daylight,
        Self::Graphite,
        Self::TokyoNight,
        Self::CatppuccinMocha,
        Self::Nord,
        Self::Porcelain,
        Self::Sandstone,
        Self::DeepSea,
        Self::Ember,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::OneDark,
        Self::OneLight,
        Self::RosePine,
        Self::RosePineDawn,
        Self::Dracula,
        Self::Alucard,
        Self::KanagawaWave,
        Self::KanagawaLotus,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Graphite => "Graphite",
            Self::Daylight => "Braden",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Nord => "Nord",
            Self::Porcelain => "Porcelain",
            Self::Sandstone => "Sandstone",
            Self::DeepSea => "Deep Sea",
            Self::Ember => "Ember",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::OneDark => "One Dark",
            Self::OneLight => "One Light",
            Self::RosePine => "Rosé Pine",
            Self::RosePineDawn => "Rosé Pine Dawn",
            Self::Dracula => "Dracula",
            Self::Alucard => "Alucard",
            Self::KanagawaWave => "Kanagawa Wave",
            Self::KanagawaLotus => "Kanagawa Lotus",
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
            Self::Porcelain => "Cool ivory · sapphire",
            Self::Sandstone => "Warm paper · terracotta",
            Self::DeepSea => "Ocean ink · turquoise",
            Self::Ember => "Smoked plum · apricot",
            Self::SolarizedDark => "Deep teal · azure",
            Self::SolarizedLight => "Warm cream · azure",
            Self::OneDark => "Soft charcoal · sky",
            Self::OneLight => "Clean paper · cobalt",
            Self::RosePine => "Dusky violet · rose",
            Self::RosePineDawn => "Blush paper · pine",
            Self::Dracula => "Night charcoal · purple",
            Self::Alucard => "Pale parchment · violet",
            Self::KanagawaWave => "Inky dusk · cornflower",
            Self::KanagawaLotus => "Rice paper · denim",
        }
    }

    pub fn is_light(self) -> bool {
        matches!(
            self,
            Self::Daylight
                | Self::Porcelain
                | Self::Sandstone
                | Self::SolarizedLight
                | Self::OneLight
                | Self::RosePineDawn
                | Self::Alucard
                | Self::KanagawaLotus
        )
    }

    pub fn palette(self) -> Palette {
        match self {
            // Original GitTurtle palettes, complete semantic surface sets.
            Self::Porcelain => Palette {
                canvas: 0xf6f7fc,
                panel: 0xffffff,
                subtle: 0xeff1f8,
                hover: 0xe5e9f4,
                border: 0xcbd3e4,
                text: 0x242e49,
                muted: 0x4d5b78,
                accent: 0x3455a6,
                accent_foreground: 0xffffff,
                accent_hover: 0x294790,
                accent_active: 0x203978,
                selected: 0xdfe6f6,
                added: 0x246448,
                removed: 0xa92d4e,
                modified: 0x795314,
                renamed: 0x6c459a,
                warning: 0x795314,
                added_background: 0xe3f0e9,
                removed_background: 0xf8e5ed,
                hunk: 0x3455a6,
                line_number: 0x5f6c86,
            },
            Self::Sandstone => Palette {
                canvas: 0xf8f3ea,
                panel: 0xfffcf6,
                subtle: 0xf0eade,
                hover: 0xeae1d3,
                border: 0xd4c6b5,
                text: 0x3b302b,
                muted: 0x635446,
                accent: 0x965034,
                accent_foreground: 0xffffff,
                accent_hover: 0x82432b,
                accent_active: 0x6e3723,
                selected: 0xeddfd0,
                added: 0x396241,
                removed: 0xa13243,
                modified: 0x755012,
                renamed: 0x794a84,
                warning: 0x755012,
                added_background: 0xe7efdc,
                removed_background: 0xf6e3dd,
                hunk: 0x365e8b,
                line_number: 0x70614f,
            },
            Self::DeepSea => Palette {
                canvas: 0x0d1c27,
                panel: 0x132735,
                subtle: 0x10222f,
                hover: 0x213b4b,
                border: 0x365366,
                text: 0xe4f2f7,
                muted: 0xb2c9d6,
                accent: 0x68dccb,
                accent_foreground: 0x072d2c,
                accent_hover: 0x91e9dc,
                accent_active: 0x58c9b9,
                selected: 0x21434c,
                added: 0x83d8ae,
                removed: 0xf7a0ad,
                modified: 0xe9ca8a,
                renamed: 0xc5b2f0,
                warning: 0xe9ca8a,
                added_background: 0x183c35,
                removed_background: 0x3b2c3b,
                hunk: 0x94c9f4,
                line_number: 0x9bb7c9,
            },
            Self::Ember => Palette {
                canvas: 0x201a22,
                panel: 0x2a222c,
                subtle: 0x251e27,
                hover: 0x3b303d,
                border: 0x514052,
                text: 0xf7ece5,
                muted: 0xd0bfc7,
                accent: 0xf2b38c,
                accent_foreground: 0x382119,
                accent_hover: 0xffcba6,
                accent_active: 0xe3a27a,
                selected: 0x48343d,
                added: 0xadd3a5,
                removed: 0xf2a2b2,
                modified: 0xe8c88b,
                renamed: 0xd1b0ef,
                warning: 0xe8c88b,
                added_background: 0x303b2e,
                removed_background: 0x472b37,
                hunk: 0xb4c7ee,
                line_number: 0xbda6b6,
            },
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
            // Families adapted from `sources`. Named constants are upstream
            // values used unchanged; hex literals marked "tuned" depart from
            // the upstream value for a readability rule and are listed in
            // DESIGN.md. Unmarked literals are derived surfaces the upstream
            // palette does not define (subtle, hover, selected, diff tiles,
            // accent states).
            Self::SolarizedDark => {
                use sources::solarized::{self as s, dark};
                Palette {
                    canvas: dark::CANVAS,
                    panel: dark::PANEL,
                    subtle: 0x01313d,
                    hover: 0x103c48,
                    selected: 0x0b4154,
                    border: s::BASE01,
                    // Tuned: base0 and base1 lightened for 4.5:1 on selected rows.
                    text: 0xb2bdbe,
                    muted: 0xa5b0b0,
                    // Tuned: blue lightened for 3:1 on selected and hovered rows.
                    accent: 0x2e93d9,
                    accent_foreground: 0x001e26,
                    accent_hover: 0x48a0de,
                    accent_active: 0x278ed6,
                    // Tuned: green, red and violet lightened for 3:1 on row
                    // surfaces, 4.5:1 in diff tiles and the canvas label.
                    added: 0x90a600,
                    removed: 0xe66c6a,
                    modified: s::YELLOW,
                    renamed: 0x8488cd,
                    warning: s::YELLOW,
                    added_background: 0x103830,
                    removed_background: 0x1a2c35,
                    // Tuned: cyan lightened for 4.5:1 on panels.
                    hunk: 0x2daba2,
                    // Tuned: base01 lightened for 4.5:1 in the gutter.
                    line_number: 0x879da5,
                }
            }
            Self::SolarizedLight => {
                use sources::solarized::{self as s, light};
                Palette {
                    canvas: light::CANVAS,
                    panel: light::PANEL,
                    subtle: 0xf6efdc,
                    hover: 0xe3dfcf,
                    selected: 0xd0dad5,
                    border: s::BASE1,
                    // Tuned: base00 and base01 darkened for 4.5:1 on selected rows.
                    text: 0x394549,
                    muted: 0x495b61,
                    // Tuned: blue darkened for a 4.5:1 white label and 3:1 on rows.
                    accent: 0x2178b6,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x1d6aa0,
                    accent_active: 0x1a5e8f,
                    // Tuned: every accent darkened for 3:1 on row surfaces,
                    // 4.5:1 in diff tiles and the canvas label on fills.
                    added: 0x5f6d00,
                    removed: 0xc62422,
                    modified: 0x8c6a00,
                    renamed: 0x656ac1,
                    warning: 0x8c6a00,
                    added_background: 0xece9c3,
                    removed_background: 0xfae2d1,
                    // Tuned: blue darkened for 4.5:1 on resting surfaces and 3:1 on rows.
                    hunk: 0x1d6ca2,
                    // Tuned: base01 darkened for 4.5:1 on panels.
                    line_number: 0x566b72,
                }
            }
            Self::OneDark => {
                use sources::one::dark as o;
                Palette {
                    canvas: o::BG,
                    panel: 0x2e333d,
                    subtle: 0x21252b,
                    hover: 0x333943,
                    selected: 0x323d52,
                    border: 0x4b5263,
                    text: o::MONO_1,
                    // Tuned: mono-2 lightened for 4.5:1 on every row surface.
                    muted: 0xadb2bb,
                    accent: o::BLUE,
                    accent_foreground: 0x1b2533,
                    accent_hover: 0x7dbdf2,
                    accent_active: 0x53a8ee,
                    added: o::GREEN,
                    // Tuned: red-1 lightened for 4.5:1 in its diff tile and under the canvas label.
                    removed: 0xe5858d,
                    modified: o::ORANGE_2,
                    renamed: o::PURPLE,
                    warning: o::ORANGE_2,
                    added_background: 0x353e3c,
                    removed_background: 0x3e343c,
                    hunk: o::CYAN,
                    // Tuned: mono-2 lightened for 4.5:1 in the gutter.
                    line_number: 0x979da8,
                }
            }
            Self::OneLight => {
                use sources::one::light as o;
                Palette {
                    canvas: o::BG,
                    panel: 0xffffff,
                    subtle: 0xf0f0f1,
                    hover: 0xf1f1f3,
                    selected: 0xe9edff,
                    border: 0xd3d3d6,
                    text: o::MONO_1,
                    // Tuned: mono-2 darkened for 4.5:1 on selected rows.
                    muted: 0x62656f,
                    // Tuned: blue darkened for a 4.5:1 white label.
                    accent: 0x2f6cf1,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x175bef,
                    accent_active: 0x0f52e3,
                    // Tuned: green darkened for 3:1 on rows and 4.5:1 in diff tiles.
                    added: 0x3b763a,
                    removed: o::RED_2,
                    modified: o::ORANGE_1,
                    renamed: o::PURPLE,
                    warning: o::ORANGE_1,
                    added_background: 0xe6efe5,
                    removed_background: 0xf6e7eb,
                    // Tuned: cyan darkened for 4.5:1 on subtle surfaces.
                    hunk: 0x0174a5,
                    line_number: o::MONO_2,
                }
            }
            Self::RosePine => {
                use sources::rose_pine::main as r;
                Palette {
                    canvas: r::BASE,
                    panel: r::SURFACE,
                    subtle: 0x16141f,
                    hover: r::OVERLAY,
                    selected: 0x2d2a45,
                    border: 0x403d52,
                    text: r::TEXT,
                    // Tuned: subtle lightened for 4.5:1 on selected rows and diff tiles.
                    muted: 0xa19db7,
                    accent: r::ROSE,
                    accent_foreground: 0x2a1d25,
                    accent_hover: 0xf3cfcd,
                    accent_active: 0xe2aeac,
                    added: r::FOAM,
                    removed: r::LOVE,
                    modified: r::GOLD,
                    renamed: r::IRIS,
                    warning: r::GOLD,
                    added_background: 0x1f2e36,
                    removed_background: 0x351f30,
                    hunk: r::IRIS,
                    line_number: r::SUBTLE,
                }
            }
            Self::RosePineDawn => {
                use sources::rose_pine::dawn as r;
                Palette {
                    canvas: r::BASE,
                    panel: r::SURFACE,
                    subtle: 0xf4ede4,
                    hover: r::OVERLAY,
                    selected: 0xe8dfe2,
                    border: 0xdfdad9,
                    text: r::TEXT,
                    // Tuned: subtle darkened for 4.5:1 on every row surface and diff tile.
                    muted: 0x5e5b73,
                    accent: r::PINE,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x225a70,
                    accent_active: 0x1d4d60,
                    // Tuned: foam, love, gold and iris darkened for 3:1 on row
                    // surfaces, 4.5:1 in diff tiles and the canvas label on fills.
                    added: 0x42717a,
                    removed: 0x985367,
                    modified: 0x986622,
                    renamed: 0x86719d,
                    warning: 0x986622,
                    added_background: 0xe4ecea,
                    removed_background: 0xf6e3e3,
                    hunk: r::PINE,
                    // Tuned: subtle darkened for 4.5:1 on canvas.
                    line_number: 0x716d89,
                }
            }
            Self::Dracula => {
                use sources::dracula::dark as d;
                Palette {
                    canvas: d::BACKGROUND,
                    panel: 0x2e303e,
                    subtle: 0x21222c,
                    hover: 0x383a4a,
                    selected: d::SELECTION,
                    border: 0x4a4d62,
                    text: d::FOREGROUND,
                    // Tuned: comment lightened for 4.5:1 on every row surface and diff tile.
                    muted: 0xb8bfd6,
                    accent: d::PURPLE,
                    accent_foreground: d::BACKGROUND,
                    accent_hover: 0xcfaefb,
                    accent_active: 0xb083f7,
                    added: d::GREEN,
                    // Tuned: red lightened for 3:1 on selected rows and 4.5:1 in its diff tile.
                    removed: 0xff6f6f,
                    modified: d::ORANGE,
                    renamed: d::PURPLE,
                    warning: d::ORANGE,
                    added_background: 0x2b4136,
                    removed_background: 0x472e3a,
                    hunk: d::CYAN,
                    // Tuned: comment lightened for 4.5:1 in the gutter.
                    line_number: 0x8b97bc,
                }
            }
            Self::Alucard => {
                use sources::dracula::alucard as a;
                Palette {
                    canvas: a::BACKGROUND,
                    panel: 0xfffdf5,
                    subtle: 0xf5f1e1,
                    hover: 0xefebdb,
                    selected: a::SELECTION,
                    border: 0xd9d4bf,
                    text: a::FOREGROUND,
                    // Tuned: comment darkened for 4.5:1 on selected and hovered selected rows.
                    muted: 0x59543e,
                    accent: a::PURPLE,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x563cb8,
                    accent_active: 0x4a32a2,
                    added: a::GREEN,
                    // Tuned: red darkened for 3:1 on hovered selected rows and 4.5:1 in its diff tile.
                    removed: 0xbf3728,
                    modified: a::ORANGE,
                    renamed: a::PURPLE,
                    warning: a::ORANGE,
                    added_background: 0xe3f0da,
                    removed_background: 0xfbe3dc,
                    hunk: a::CYAN,
                    line_number: a::COMMENT,
                }
            }
            Self::KanagawaWave => {
                use sources::kanagawa::wave as k;
                Palette {
                    canvas: k::SUMI_INK_3,
                    panel: k::SUMI_INK_4,
                    subtle: k::SUMI_INK_2,
                    hover: k::SUMI_INK_5,
                    // Tuned: waveBlue1 lightened to stay 1.15:1 apart from the panel.
                    selected: 0x24364e,
                    border: k::SUMI_INK_6,
                    text: k::FUJI_WHITE,
                    muted: k::OLD_WHITE,
                    accent: k::CRYSTAL_BLUE,
                    accent_foreground: k::SUMI_INK_3,
                    accent_hover: 0x94aee0,
                    accent_active: 0x6d8dce,
                    // Tuned: autumnGreen lightened for 4.5:1 in its diff tile.
                    added: 0x85a07a,
                    removed: k::PEACH_RED,
                    modified: k::AUTUMN_YELLOW,
                    renamed: k::ONI_VIOLET,
                    warning: k::RONIN_YELLOW,
                    added_background: k::WINTER_GREEN,
                    removed_background: k::WINTER_RED,
                    hunk: k::SPRING_BLUE,
                    // Tuned: sumiInk6 lightened for 4.5:1 in the gutter.
                    line_number: 0x9090a9,
                }
            }
            Self::KanagawaLotus => {
                use sources::kanagawa::lotus as k;
                Palette {
                    canvas: k::LOTUS_WHITE_3,
                    panel: 0xf7f3d1,
                    subtle: k::LOTUS_WHITE_2,
                    hover: k::LOTUS_WHITE_1,
                    selected: k::LOTUS_BLUE_1,
                    border: k::LOTUS_WHITE_0,
                    text: k::LOTUS_INK_1,
                    // Tuned: lotusGray2 darkened for 4.5:1 on every row surface and diff tile.
                    muted: 0x5a574d,
                    accent: k::LOTUS_BLUE_4,
                    accent_foreground: k::LOTUS_WHITE_3,
                    accent_hover: 0x435c89,
                    accent_active: 0x3a5077,
                    // Tuned: lotusGreen2, lotusRed2, lotusYellow3 and lotusOrange2 darkened for
                    // 3:1 on row surfaces, 4.5:1 in diff tiles and the canvas label on fills.
                    added: 0x4e6643,
                    removed: 0xa72428,
                    modified: 0x996900,
                    renamed: k::LOTUS_VIOLET_4,
                    warning: 0x9a5b00,
                    // Tuned: lotusGreen3 and lotusRed4 blended halfway to the canvas so text
                    // keeps 4.5:1 inside diff tiles.
                    added_background: 0xd4deb5,
                    removed_background: 0xe6c8a8,
                    // Tuned: lotusBlue4 darkened for 4.5:1 on subtle surfaces.
                    hunk: 0x476190,
                    // Tuned: lotusGray2 darkened for 4.5:1 on the canvas.
                    line_number: 0x6d6a5e,
                }
            }
        }
    }

    /// Apply this built-in theme through the one palette application path. The app
    /// applies a `ResolvedTheme`; test fixtures apply a built-in directly.
    #[cfg(test)]
    pub fn apply(self, window: Option<&mut Window>, cx: &mut App) {
        custom::ResolvedTheme::built_in(self).apply(window, cx);
    }
}

impl custom::ResolvedTheme {
    /// Apply the resolved built-in or custom palette through the one application path.
    pub fn apply(self, window: Option<&mut Window>, cx: &mut App) {
        self.palette.apply(self.is_light, window, cx);
    }
}

impl Palette {
    /// Apply native controls and editor defaults together. Built-in and custom themes both use
    /// this path. Call after GPUI Kit initialization; callers invalidate/rebuild existing custom
    /// decorations.
    pub fn apply(self, is_light: bool, window: Option<&mut Window>, cx: &mut App) {
        Theme::change(
            if is_light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            },
            None,
            cx,
        );
        self.configure(is_light, Theme::global_mut(cx));
        cx.set_global(self);
        Theme::sync_base(cx);
        if let Some(window) = window {
            window.refresh();
        }
    }

    fn configure(self, is_light: bool, theme: &mut Theme) {
        let palette = self;
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
        // Danger buttons have their own tokens; the general danger color
        // alone leaves the toolkit's low-contrast default button untouched.
        let danger: gpui_kit::Hsla = rgb(palette.removed).into();
        theme.colors.button_danger = danger;
        theme.colors.button_danger_foreground = rgb(palette.canvas).into();
        theme.colors.button_danger_hover = if is_light {
            danger.darken(0.05)
        } else {
            danger.lighten(0.05)
        };
        theme.colors.button_danger_active = if is_light {
            danger.darken(0.1)
        } else {
            danger.lighten(0.1)
        };
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
        self.history_row_height_at_scale(ui_scale())
    }
    pub(super) fn history_row_height_at_scale(self, scale: f32) -> f32 {
        scale
            * match self {
                Self::Comfortable => 34.,
                Self::Compact => 28.,
            }
    }

    pub fn file_row_height(self) -> f32 {
        self.file_row_height_at_scale(ui_scale())
    }
    pub(super) fn file_row_height_at_scale(self, scale: f32) -> f32 {
        scale
            * match self {
                Self::Comfortable => 44.,
                Self::Compact => 34.,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::custom::contrast;
    use super::*;
    use gpui_kit::{Background, Hsla};

    #[test]
    fn parallel_test_applications_keep_their_own_text_geometry() {
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for (interface, code, desktop) in [(13, 12, 1.0f32), (18, 24, 1.5f32)] {
                let barrier = &barrier;
                scope.spawn(move || {
                    with_text_sizes(|interface_size, code_size, desktop_scale| {
                        interface_size.store(interface, Ordering::Relaxed);
                        code_size.store(code, Ordering::Relaxed);
                        desktop_scale.store(desktop.to_bits(), Ordering::Relaxed);
                    });
                    // Both applications have set their sizes before either
                    // consumes geometry, exposing shared global storage reliably.
                    barrier.wait();
                    assert_eq!(ui_text(13.), px(f32::from(interface) * desktop));
                    assert_eq!(code_text(), px(f32::from(code) * desktop));
                });
            }
        });
    }

    #[test]
    fn desktop_text_scale_values_are_bounded_and_finite() {
        assert_eq!(desktop_text_scale_value(1.0), 1.0);
        assert_eq!(desktop_text_scale_value(1.25), 1.25);
        assert_eq!(desktop_text_scale_value(0.1), 0.5);
        assert_eq!(desktop_text_scale_value(12.0), 3.0);
        assert_eq!(desktop_text_scale_value(0.0), 1.0);
        assert_eq!(desktop_text_scale_value(-1.0), 1.0);
        assert_eq!(desktop_text_scale_value(f64::NAN), 1.0);
        assert_eq!(desktop_text_scale_value(f64::INFINITY), 1.0);
        // This test application begins with the default desktop factor.
        assert_eq!(desktop_text_scale(), 1.0);
    }

    #[test]
    fn palettes_keep_text_and_diff_content_readable_in_each_theme() {
        for choice in ThemeChoice::ALL {
            let issues = choice.palette().readability_issues();
            assert!(
                issues.is_empty(),
                "{choice:?} breaks readability rules: {}",
                issues
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
    }

    #[test]
    fn palette_lightness_matches_each_built_in_choice() {
        for choice in ThemeChoice::ALL {
            assert_eq!(choice.is_light(), choice.palette().is_light(), "{choice:?}");
        }
    }

    #[test]
    fn theme_switch_updates_resolved_component_backgrounds_with_foregrounds() {
        // Reuse one theme to cover dark/light and dark/dark switches. Component
        // buttons read token backgrounds but legacy foreground colors, so a
        // palette-only assertion cannot catch a stale white primary button.
        // A custom palette, built from a base with two edited tokens, takes the same path.
        let mut edited = ThemeChoice::Nord.palette();
        edited.set(custom::TokenKind::Accent, 0x1f6f5c);
        edited.set(custom::TokenKind::Removed, 0xff9aa2);
        let custom = custom::CustomTheme {
            id: 1,
            name: "Edited Nord".into(),
            base: ThemeChoice::Nord,
            palette: edited,
        };
        let cases = ThemeChoice::ALL
            .into_iter()
            .map(|choice| (format!("{choice:?}"), choice.palette(), choice.is_light()))
            .chain([(custom.name.clone(), custom.palette, custom.is_light())]);
        let mut theme = Theme::default();
        for (choice, palette, is_light) in cases {
            palette.configure(is_light, &mut theme);
            let foreground: Hsla = rgb(palette.accent_foreground).into();
            assert_eq!(theme.colors.button_primary_foreground, foreground);
            for token in [
                theme.tokens.button_danger,
                theme.tokens.button_danger_hover,
                theme.tokens.button_danger_active,
            ] {
                let foreground = u32::from(theme.colors.button_danger_foreground.to_rgb()) >> 8;
                let background = u32::from(token.color.to_rgb()) >> 8;
                assert!(
                    contrast(foreground, background) >= 4.5,
                    "{choice} destructive action label must remain readable"
                );
                assert_eq!(token.background, Background::from(token.color));
            }
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
                assert_eq!(token.color, color, "{choice} resolved color");
                assert_eq!(
                    token.background,
                    Background::from(color),
                    "{choice} renderable background"
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

    #[test]
    fn adapted_family_themes_keep_their_storage_names_labels_and_lightness() {
        assert_eq!(ThemeChoice::ALL.len(), 20);
        for (choice, stored, label, light) in [
            (
                ThemeChoice::SolarizedDark,
                "solarized_dark",
                "Solarized Dark",
                false,
            ),
            (
                ThemeChoice::SolarizedLight,
                "solarized_light",
                "Solarized Light",
                true,
            ),
            (ThemeChoice::OneDark, "one_dark", "One Dark", false),
            (ThemeChoice::OneLight, "one_light", "One Light", true),
            (ThemeChoice::RosePine, "rose_pine", "Rosé Pine", false),
            (
                ThemeChoice::RosePineDawn,
                "rose_pine_dawn",
                "Rosé Pine Dawn",
                true,
            ),
            (ThemeChoice::Dracula, "dracula", "Dracula", false),
            (ThemeChoice::Alucard, "alucard", "Alucard", true),
            (
                ThemeChoice::KanagawaWave,
                "kanagawa_wave",
                "Kanagawa Wave",
                false,
            ),
            (
                ThemeChoice::KanagawaLotus,
                "kanagawa_lotus",
                "Kanagawa Lotus",
                true,
            ),
        ] {
            assert!(ThemeChoice::ALL.contains(&choice));
            assert_eq!(serde_json::to_value(choice).unwrap(), stored);
            assert_eq!(
                serde_json::from_value::<ThemeChoice>(stored.into()).unwrap(),
                choice
            );
            assert_eq!(choice.label(), label);
            assert_eq!(choice.is_light(), light);
            // Two words around a middle dot, like "Deep slate · mint".
            let (surface, accent) = choice.description().split_once(" · ").unwrap();
            assert_eq!(surface.split(' ').count(), 2, "{choice:?}");
            assert_eq!(accent.split(' ').count(), 1, "{choice:?}");
        }
    }

    #[test]
    fn saved_daylight_remains_braden_without_changing_its_storage_or_light_mapping() {
        let saved =
            r#"{"theme":"daylight","follow_system":false,"density":"compact","code_text_size":19}"#;
        let mut settings: crate::preferences::AppSettings = serde_json::from_str(saved).unwrap();
        assert_eq!(
            settings.theme,
            custom::ThemeSelection::BuiltIn(ThemeChoice::Daylight)
        );
        assert_eq!(ThemeChoice::Daylight.label(), "Braden");
        assert_eq!(
            serde_json::to_value(&settings).unwrap()["theme"],
            "daylight"
        );
        assert_eq!(settings.density, Density::Compact);
        assert_eq!(settings.code_text_size, 19);
        let resolved = |settings: &crate::preferences::AppSettings, appearance| {
            settings.resolved_theme(appearance, &[]).selection
        };
        let built_in = custom::ThemeSelection::BuiltIn;
        assert_eq!(
            resolved(&settings, gpui_kit::WindowAppearance::Dark),
            built_in(ThemeChoice::Daylight)
        );
        settings.follow_system = true;
        for light in [
            ThemeChoice::Daylight,
            ThemeChoice::Porcelain,
            ThemeChoice::Sandstone,
            ThemeChoice::SolarizedLight,
            ThemeChoice::OneLight,
            ThemeChoice::RosePineDawn,
            ThemeChoice::Alucard,
            ThemeChoice::KanagawaLotus,
        ] {
            settings.theme = built_in(light);
            assert_eq!(
                resolved(&settings, gpui_kit::WindowAppearance::Light),
                built_in(ThemeChoice::Daylight)
            );
            assert_eq!(
                resolved(&settings, gpui_kit::WindowAppearance::Dark),
                built_in(ThemeChoice::Midnight)
            );
        }
    }
}
