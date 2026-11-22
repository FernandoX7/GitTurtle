//! Pure, UI-framework-independent theme tokens, contrast/OKLab fitting,
//! custom themes, Omarchy palette parsing, and grapheme-safe text cutting.
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

#[cfg(any(test, feature = "test-support"))]
pub mod sources;
#[cfg(not(any(test, feature = "test-support")))]
#[expect(dead_code)]
mod sources;

#[cfg_attr(not(test), expect(dead_code))]
pub mod custom;

pub mod highlights;
pub mod omarchy;
pub mod palette;
pub mod text_cut;

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

pub use highlights::{
    Chosen, EditorHighlights, HIGHLIGHT_DISTANCE, HIGHLIGHT_LIGHTNESS_GAP, PatchText, SyntaxRoles,
    WORD_LIGHTNESS_GAP, oklab, oklab_distance,
};
pub use palette::{
    DIALOG_BACKDROP_MAX_PERCENT, DIALOG_EDGE, DIALOG_LIFT, DISABLED_EDGE, DISABLED_LABEL,
    DISABLED_LABEL_MARGIN, DISABLED_SHARE, Density, LABEL_MARGIN, LABEL_RULE, Labels,
    MORE_CONTRAST_DISABLED, MORE_CONTRAST_DISABLED_SHARE, MORE_CONTRAST_EDGE, MORE_CONTRAST_TEXT,
    POPUP_EDGE, Palette, REF_TINT_ALPHA, RgbaColor, SCROLL_THUMB, ThemeChoice, backdrop_over,
    composite, drawn_nearer, rgb_color,
};

pub const DEFAULT_INTERFACE_TEXT_SIZE: u8 = 13;
pub const DEFAULT_CODE_TEXT_SIZE: u8 = 12;
pub const INTERFACE_TEXT_RANGE: std::ops::RangeInclusive<u8> = 11..=18;
pub const CODE_TEXT_RANGE: std::ops::RangeInclusive<u8> = 10..=24;
/// Bounds for the desktop's own text scaling factor (GNOME "Large Text").
pub const DESKTOP_TEXT_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;

/// Custom project names outlive the ten recent entries, so they are bounded
/// separately. Reaching the limit refuses a new name instead of evicting one.
pub const MAX_PROJECT_NAME_BYTES: usize = 128;
/// Custom themes are user-authored, so reaching the limit refuses a new theme
/// instead of evicting one, and a stored section above it refuses later writes.
pub const MAX_CUSTOM_THEMES: usize = 32;

const DARK_COLORS: [u32; 6] = [0x7adfb4, 0x8db7f6, 0xc3a5f2, 0xe8be7a, 0xea9ca5, 0x72ccd8];
const LIGHT_COLORS: [u32; 6] = [0x146c53, 0x315da7, 0x7651aa, 0x8c5916, 0xa42d63, 0x156b7c];

/// Lane set for a palette; `Palette::readability_issues` judges the same colors.
pub fn palette_colors(palette: Palette) -> [u32; 6] {
    lane_colors(palette.is_light())
}

/// The lane set of a light or a dark palette.
pub fn lane_colors(light: bool) -> [u32; 6] {
    if light { LIGHT_COLORS } else { DARK_COLORS }
}

// One application appearance applies to every native window. Pixel helpers are
// also usable by canvas geometry and pure row-height consumers without a UI
// context. No preference writes or repository work happen through these reads.
#[cfg(not(any(test, feature = "test-support")))]
static INTERFACE_TEXT_SIZE: AtomicU8 = AtomicU8::new(DEFAULT_INTERFACE_TEXT_SIZE);
#[cfg(not(any(test, feature = "test-support")))]
static CODE_TEXT_SIZE: AtomicU8 = AtomicU8::new(DEFAULT_CODE_TEXT_SIZE);
#[cfg(not(any(test, feature = "test-support")))]
static DESKTOP_TEXT_SCALE: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

// Each GPUI test owns its application on one test thread. Keep its simulated
// appearance there too so changing font size in one test cannot move controls
// in another concurrent test.
#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static INTERFACE_TEXT_SIZE: AtomicU8 = const { AtomicU8::new(DEFAULT_INTERFACE_TEXT_SIZE) };
    static CODE_TEXT_SIZE: AtomicU8 = const { AtomicU8::new(DEFAULT_CODE_TEXT_SIZE) };
    static DESKTOP_TEXT_SCALE: AtomicU32 = const { AtomicU32::new(1.0f32.to_bits()) };
}

fn with_text_sizes<R>(read: impl FnOnce(&AtomicU8, &AtomicU8, &AtomicU32) -> R) -> R {
    #[cfg(not(any(test, feature = "test-support")))]
    {
        read(&INTERFACE_TEXT_SIZE, &CODE_TEXT_SIZE, &DESKTOP_TEXT_SCALE)
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        INTERFACE_TEXT_SIZE.with(|interface| {
            CODE_TEXT_SIZE
                .with(|code| DESKTOP_TEXT_SCALE.with(|desktop| read(interface, code, desktop)))
        })
    }
}

pub fn interface_text_size() -> u8 {
    with_text_sizes(|interface, _, _| interface.load(Ordering::Relaxed))
}

pub fn code_text_size() -> u8 {
    with_text_sizes(|_, code, _| code.load(Ordering::Relaxed))
}

pub fn store_text_sizes(interface: u8, code: u8) {
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
}

pub fn desktop_text_scale() -> f32 {
    with_text_sizes(|_, _, desktop| f32::from_bits(desktop.load(Ordering::Relaxed)))
}

pub fn store_desktop_text_scale(scale: f32) -> bool {
    let scale = desktop_text_scale_value(scale);
    if scale.to_bits() == desktop_text_scale().to_bits() {
        return false;
    }
    with_text_sizes(|_, _, desktop| desktop.store(scale.to_bits(), Ordering::Relaxed));
    true
}

/// Normalizes a reported factor: non-positive/non-finite values mean "no scaling".
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

pub fn ui_scale() -> f32 {
    f32::from(interface_text_size()) / f32::from(DEFAULT_INTERFACE_TEXT_SIZE) * desktop_text_scale()
}

/// Every palette a theme-wide test judges: each built-in by its label and the
/// palette each embedded Omarchy fixture maps to, as `Omarchy <file name>`.
#[cfg(any(test, feature = "test-support"))]
pub fn test_palettes() -> Vec<(String, Palette)> {
    let built_in = ThemeChoice::ALL
        .into_iter()
        .map(|choice| (choice.label().to_owned(), choice.palette()));
    #[cfg(target_os = "linux")]
    let omarchy = omarchy::fixture_palettes()
        .into_iter()
        .map(|(name, palette)| (format!("Omarchy {name}"), palette));
    #[cfg(not(target_os = "linux"))]
    let omarchy = std::iter::empty();
    built_in.chain(omarchy).collect()
}

#[cfg(test)]
pub(crate) fn assert_pure_syntax_colors_read(
    cases: impl IntoIterator<Item = (String, Palette, bool)>,
) {
    let mut below = Vec::new();
    for (name, palette, _) in cases {
        let roles = palette.syntax_roles(None);
        let backgrounds = [
            ("editor background", palette.canvas),
            ("active line", palette.panel),
            ("added lines", palette.added_background),
            ("removed lines", palette.removed_background),
        ];
        for (i, color) in roles.colors().into_iter().enumerate() {
            for (surface, background) in backgrounds {
                let ratio = custom::contrast(color, background);
                if ratio < 4.5 {
                    below.push(format!(
                        "{name}: role[{i}] #{color:06x} on {surface} #{background:06x} {ratio:.2}:1"
                    ));
                }
            }
        }
    }
    assert!(
        below.is_empty(),
        "below 4.5:1:
{}",
        below.join(
            "
"
        )
    );
}

#[cfg(test)]
pub(crate) fn assert_pure_editor_highlights_read(
    cases: impl IntoIterator<Item = (String, Palette, bool)>,
) -> Vec<String> {
    let mut below = Vec::new();
    let mut close = Vec::new();
    let mut current_like_others = Vec::new();
    for (name, palette, _) in cases {
        let roles = palette.syntax_roles(None);
        let highlights = palette.editor_highlights();
        let all_syntax = roles.colors();
        let word_syntax = roles.word_colors();
        let lines = [
            ("editor background", palette.canvas),
            ("active line", palette.panel),
            ("added lines", palette.added_background),
            ("removed lines", palette.removed_background),
        ];
        for (h_name, h_color) in [
            ("selection", highlights.selection),
            ("find_match", highlights.find_match),
            ("find_current", highlights.find_current),
        ] {
            for &color in all_syntax.iter().chain(&[
                highlights.patch_text.added,
                highlights.patch_text.removed,
                highlights.patch_text.hunk,
            ]) {
                let ratio = custom::contrast(color, h_color);
                if ratio < 4.5 {
                    below.push(format!(
                        "{name}: #{color:06x} on {h_name} #{h_color:06x} {ratio:.2}:1"
                    ));
                }
            }
            for (l_name, l_color) in lines {
                let dist = oklab_distance(oklab(h_color), oklab(l_color));
                if dist + 1e-9 < HIGHLIGHT_DISTANCE {
                    close.push(format!(
                        "{name}: {h_name} #{h_color:06x} on {l_name} #{l_color:06x} {dist:.4}"
                    ));
                }
            }
        }
        for (w_name, w_color, line_color, diff_color) in [
            (
                "added_word",
                highlights.added_word,
                palette.added_background,
                highlights.patch_text.added,
            ),
            (
                "removed_word",
                highlights.removed_word,
                palette.removed_background,
                highlights.patch_text.removed,
            ),
        ] {
            for &color in word_syntax.iter().chain(&[diff_color]) {
                let ratio = custom::contrast(color, w_color);
                if ratio < 4.5 {
                    below.push(format!(
                        "{name}: #{color:06x} on {w_name} #{w_color:06x} {ratio:.2}:1"
                    ));
                }
            }
            let dist = oklab_distance(oklab(w_color), oklab(line_color));
            if dist + 1e-9 < HIGHLIGHT_DISTANCE {
                close.push(format!(
                    "{name}: {w_name} #{w_color:06x} on line #{line_color:06x} {dist:.4}"
                ));
            }
        }
        if oklab_distance(oklab(highlights.find_current), oklab(highlights.find_match)) + 1e-9
            < HIGHLIGHT_DISTANCE
        {
            current_like_others.push(name);
        }
    }
    assert!(
        below.is_empty(),
        "below 4.5:1:
{}",
        below.join(
            "
"
        )
    );
    assert!(
        close.is_empty(),
        "too close:
{}",
        close.join(
            "
"
        )
    );
    current_like_others
}
