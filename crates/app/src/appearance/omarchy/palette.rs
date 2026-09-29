//! Omarchy's `colors.toml`, parsed and mapped onto GitTurtle's semantic palette.
//!
//! Lines are read the way Omarchy's own `omarchy-theme-color` reads them, and the keys GitTurtle
//! uses are resolved through that script's alias cascade, so a theme generated from an
//! `alacritty.toml` (ANSI `color0`…`color15` and no named colors or mode) or written with the
//! legacy short names resolves as Omarchy renders it. The mode follows the same precedence.
//! Input that is too large, not UTF-8 or without a required color is refused with a reason.
//!
//! The mapping follows the plan in `docs/development/themes/spec.md#omarchy-theme`: each token
//! starts from the theme's own color and is then fitted to the readability rules, moving its
//! lightness toward the mode's extreme only as far as a rule needs. A palette that still has a
//! finding after the fit (only contrived input gets there) is replaced by the mode's default
//! built-in, so the applied palette never breaks a rule.

use crate::appearance::custom::{PRESSED_STEP, contrast, luminance};
use crate::appearance::{Palette, ThemeChoice};
use std::collections::HashMap;

/// Theme files larger than this are refused without reading past it.
pub(crate) const MAX_COLORS_BYTES: usize = 16 * 1024;

/// Whether a theme paints light text on dark surfaces or the reverse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Light,
    Dark,
}

/// The colors of a theme the mapping reads, resolved through Omarchy's aliases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Colors {
    pub mode: Mode,
    pub background: u32,
    pub foreground: u32,
    pub accent: u32,
    pub red: u32,
    pub green: u32,
    pub yellow: u32,
    pub blue: u32,
    pub magenta: u32,
    /// An optional color is `None` only when the value it resolves to is not `#rrggbb`.
    pub selection: Option<u32>,
    pub muted: Option<u32>,
    pub dark_background: Option<u32>,
    pub lighter_background: Option<u32>,
    pub dark_foreground: Option<u32>,
    pub orange: Option<u32>,
}

/// The required colors and the keys `omarchy-theme-color` tries for each, in its order: the
/// canonical name, the legacy short name, then the ANSI color it aliases.
const REQUIRED: [(&str, &[&str]); 8] = [
    ("background", &["background", "bg", "color0"]),
    ("foreground", &["foreground", "fg", "color7"]),
    ("accent", &["accent"]),
    ("red", &["red", "color1"]),
    ("green", &["green", "color2"]),
    ("yellow", &["yellow", "color3"]),
    ("blue", &["blue", "color4"]),
    ("magenta", &["magenta", "color5", "purple"]),
];

/// The keys of one `colors.toml` as `omarchy-theme-color` reads them: the last value of each,
/// where a key counts only with a non-empty value.
struct Keys<'a>(HashMap<String, &'a str>);

impl<'a> Keys<'a> {
    /// Read each line as `omarchy-theme-color` does: split at the first `=`, drop quotes and
    /// spaces from the key and skip it when empty, a comment or not `[A-Za-z0-9_-]`, take a
    /// value between its first two quotes (or trimmed, unquoted), and skip a value with a
    /// character outside Omarchy's set. Unlike that script, keys after a `[table]` header
    /// belong to the table, as in TOML.
    fn read(text: &'a str) -> Self {
        let mut keys = HashMap::new();
        let mut in_table = false;
        for line in text.lines() {
            if line.trim_start().starts_with('[') {
                in_table = true;
                continue;
            }
            let (key, value) = line.split_once('=').unwrap_or((line, ""));
            let key: String = key
                .chars()
                .filter(|character| !matches!(character, '"' | '\'' | ' '))
                .collect();
            if in_table
                || key.is_empty()
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                continue;
            }
            let value = match value.find(['"', '\'']) {
                Some(open) => {
                    let rest = &value[open + 1..];
                    &rest[..rest.find(['"', '\'']).unwrap_or(rest.len())]
                }
                // Bash's `[:space:]`.
                None => value.trim_matches(|character: char| {
                    character.is_ascii_whitespace() || character == '\x0b'
                }),
            };
            if value.chars().all(|character| {
                character.is_ascii_alphanumeric() || "#(),._+/% -".contains(character)
            }) {
                keys.insert(key, value);
            }
        }
        Self(keys)
    }

    /// The first of `names` with a value, and the key that gave it.
    fn first(&self, names: &[&'static str]) -> Option<(&'static str, &'a str)> {
        names.iter().find_map(|&name| {
            self.0
                .get(name)
                .filter(|value| !value.is_empty())
                .map(|&value| (name, value))
        })
    }
}

/// Parse a theme's `colors.toml`, resolving it through Omarchy's aliases. `light_marker` says
/// whether a regular `light.mode` file sits beside it, which Omarchy reads as a light theme
/// when the file names no mode. The error is a short reason for the Settings card.
pub(crate) fn parse(bytes: &[u8], light_marker: bool) -> Result<Colors, String> {
    if bytes.len() > MAX_COLORS_BYTES {
        return Err(format!(
            "colors.toml is larger than {} KiB",
            MAX_COLORS_BYTES / 1024
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "colors.toml is not UTF-8 text")?;
    let keys = Keys::read(text.strip_prefix('\u{feff}').unwrap_or(text));
    let mut required = [0; REQUIRED.len()];
    for ((name, names), color) in REQUIRED.iter().zip(&mut required) {
        let article = if name.starts_with(['a', 'e', 'i', 'o', 'u']) {
            "an"
        } else {
            "a"
        };
        let (key, value) = keys.first(names).ok_or_else(|| match names {
            [_] => format!("colors.toml is missing {article} {name} color"),
            [aliases @ .., last] => format!(
                "colors.toml is missing {article} {name} color ({} or {last})",
                aliases.join(", ")
            ),
            [] => unreachable!("every required color has a key"),
        })?;
        *color = hex(value).ok_or_else(|| format!("colors.toml's {key} is not a #rrggbb color"))?;
    }
    let [
        background,
        foreground,
        accent,
        red,
        green,
        yellow,
        blue,
        magenta,
    ] = required;
    // `omarchy-theme-color` resolves these after the required ones; `color0` has become the
    // background by then.
    let color = |names: &[&'static str], then: Option<u32>| match keys.first(names) {
        Some((_, value)) => hex(value),
        None => then,
    };
    let dark_foreground = color(&["dark_foreground", "dark_fg", "color8"], Some(foreground));
    let mode = match keys.first(&["mode", "theme_type"]) {
        // Omarchy's consumers compare the mode with "light" exactly.
        Some((_, "light")) => Mode::Light,
        Some(_) => Mode::Dark,
        None if light_marker => Mode::Light,
        None => {
            let sum = [16, 8, 0]
                .into_iter()
                .map(|shift| (background >> shift) & 0xff)
                .sum::<u32>();
            if sum > 382 { Mode::Light } else { Mode::Dark }
        }
    };
    Ok(Colors {
        mode,
        background,
        foreground,
        accent,
        red,
        green,
        yellow,
        blue,
        magenta,
        selection: color(
            &["selection", "selection_background", "color8"],
            Some(background),
        ),
        muted: color(&["muted", "color8"], dark_foreground),
        dark_background: color(
            &["dark_background", "dark_bg"],
            Some(mix(background, BLACK, 0.25)),
        ),
        lighter_background: color(&["lighter_background", "lighter_bg"], Some(background)),
        dark_foreground,
        orange: color(&["orange"], Some(yellow)),
    })
}

/// `#rrggbb` in either case; anything else, including shorthand and alpha, is not a color.
fn hex(value: &str) -> Option<u32> {
    let digits = value.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

/// A theme's palette and whether it is light, as `ResolvedTheme` applies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mapped {
    pub palette: Palette,
    pub is_light: bool,
}

/// The palette a theme applies: the fitted palette, or the mode's default built-in when
/// the fit still leaves a readability finding.
pub(crate) fn map(colors: &Colors) -> Mapped {
    let fitted = fit(colors);
    let palette = if fitted.readability_issues().is_empty() {
        fitted
    } else {
        match colors.mode {
            Mode::Light => ThemeChoice::Daylight.palette(),
            Mode::Dark => ThemeChoice::Midnight.palette(),
        }
    };
    Mapped {
        palette,
        is_light: palette.is_light(),
    }
}

const BLACK: u32 = 0x000000;
const WHITE: u32 = 0xffffff;

/// Margins a tuned token keeps above its rule (`DESIGN.md`, semantic palette ownership): 0.25
/// for text and glyph rules, and a little for the surface steps.
const CONTRAST_MARGIN: f64 = 0.25;
const SURFACE_MARGIN: f64 = 0.02;
/// The graph lanes' own minimum plus a margin: every row surface keeps it against each lane.
const LANE_TARGET: f64 = 3.1;
/// How far below the lane limit (dark) or above it (light) the panel must stay, as a contrast
/// ratio, so the selected row, hover and pressed layers can still step away from it.
const PANEL_ROOM: f64 = 1.3;
/// The pressed-step rule's 6, plus a margin.
const PRESSED_TARGET: u32 = 8;
/// The most rounds the pressed step takes: every lightness step of the selected row, and as
/// many of hover's.
const PRESS_ROUNDS: u32 = 2 * LIGHTNESS_STEPS;
/// Diff tiles tint the canvas with this much of the status color.
const DIFF_TINT_DARK: f64 = 0.14;
const DIFF_TINT_LIGHT: f64 = 0.10;
/// Red and green count as distinguishable when both keep at least this OKLCH chroma after
/// fitting and their OKLCH hues are at least this many degrees apart.
pub(crate) const DIFF_MIN_CHROMA: f64 = 0.05;
pub(crate) const DIFF_MIN_HUE: f64 = 60.;
/// Modified and renamed keep the theme's yellow and magenta only with `DIFF_MIN_CHROMA` of
/// chroma and at least this many degrees of OKLCH hue from added, removed and each other.
/// Among the bundled themes the pairs that read as one status are 26.5° apart or closer
/// (Catppuccin's pink renamed beside its rose removed; Hackerman's and Osaka Jade's modified
/// beside added at 7° and 5°); 30° also takes Everforest's two dusty pinks at 29.5°, and the
/// closest pair kept is Flexoki Light's modified and added at 31°.
pub(crate) const STATUS_MIN_HUE: f64 = 30.;
/// The OKLCH hues a hunk header keeps the theme's `blue` in: GitTurtle's own hunk colors
/// span 188° (Solarized's cyan) to 305° (Rosé Pine's iris). Among the bundled themes the
/// blues run from Gruvbox's aqua at 179° to Ethereal's periwinkle at 280°, and the colors
/// that are no blue sit at 163° (Osaka Jade's green) and below.
const HUNK_HUES: std::ops::RangeInclusive<f64> = 175.0..=310.0;
/// The OKLCH hues Modified keeps the theme's `yellow` in, from orange to yellow: GitTurtle's
/// own Modified colors span 48.5° (Alucard's orange) to 86.7° (Solarized Dark's yellow), the
/// bundled themes' span 51.8° (Miasma's tan) to 88.3° (Flexoki Light's yellow), and pure
/// yellow sits at 110°. Outside it lie Matte Black's red at 27.5°, yellow-greens from about
/// 118°, Osaka Jade's green at 147° and the blues Lumon and Lupine name `yellow`.
const MODIFIED_HUES: std::ops::RangeInclusive<f64> = 40.0..=115.0;
/// The OKLCH hues the warning keeps the theme's `orange` (or `yellow`) in, from orange-red to
/// yellow: GitTurtle's own warnings span 48.5° (Alucard's orange) to 86.7° (Solarized Dark's
/// yellow), and the bundled themes' oranges run down to Catppuccin Latte's `#A4391E` and Tokyo
/// Night's `#EB927B` at 35°. Below it lie pure red at 29°, Catppuccin's salmon `#F6B6AB` at
/// 29.8° and Matte Black's red at 21.6°; above it, as for Modified, the yellow-greens and
/// Hackerman's green at 157°, and beyond them the blues Lumon and Lupine name `orange`.
const WARNING_HUES: std::ops::RangeInclusive<f64> = 32.0..=115.0;
/// The OKLCH hues renamed keeps the theme's `magenta` in, from blue-violet round to
/// magenta-pink: GitTurtle's own renamed colors span 278.5° (Solarized Light's violet) to
/// 328.8° (One Light's magenta), and the bundled themes' run from Ristretto's lavender at 283°
/// to Flexoki Light's magenta at 353°. Pure blue sits at 264°, Hackerman's periwinkle
/// `#86A7DF` at 261° and Lumon's sky blue `#8BC9EB` at 233°; past 360° pinks turn into the
/// roses and reds that mark removed lines.
const RENAMED_HUES: std::ops::RangeInclusive<f64> = 270.0..=360.0;
/// A `lighter_background` this close to the background is flat, not a lighter surface: the
/// bundled themes that mean one keep at least 1.045:1 (Lupine), and those without one repeat
/// the background exactly (Last Horizon, Solitude, and `color0` in a generated theme).
const FLAT_SURFACE: f64 = 1.03;
/// The furthest subtle lifts off the panel and the selected row off the canvas, so grouped
/// surfaces do not compete with the selected row and graph lanes keep their contrast on it.
/// GitTurtle's own Braden, Tokyo Night and Catppuccin Mocha lift subtle 1.13:1 to 1.15:1 off
/// the panel, and its Tokyo Night, Catppuccin Mocha, Nord and Graphite lift the selected row
/// 1.34:1 to 1.45:1 off the canvas. The selected row's 1.15:1 step off the panel and the
/// pressed-step rule still win.
const SUBTLE_CAP: f64 = 1.15;
const SELECTED_CAP: f64 = 1.45;
/// The least subtle stands off the canvas, and off the panel where it can, toward the
/// foreground: the step the flat-surface fallback gives, and the hover's step off the panel.
/// It wins over `SUBTLE_CAP`.
const SUBTLE_FLOOR: f64 = 1.08;
/// The least hover stands off the panel (the readability rule) and off subtle, where
/// secondary buttons rest, toward the foreground. The lane limit and the pressed step bring
/// subtle back under it rather than hover onto subtle.
const HOVER_STEP: f64 = 1.08;

/// The rules' foreground and background contrast checks, with the side of the surface the
/// foreground must be on: lighter in a dark palette, darker in a light one.
#[derive(Clone, Copy)]
struct Judge {
    dark: bool,
}

impl Judge {
    fn clears(self, color: u32, rules: &[(&[u32], f64)], margin: f64) -> bool {
        let color_luminance = luminance(color);
        rules.iter().all(|(surfaces, minimum)| {
            surfaces.iter().all(|&surface| {
                let surface_luminance = luminance(surface);
                (if self.dark {
                    color_luminance > surface_luminance
                } else {
                    color_luminance < surface_luminance
                }) && contrast(color, surface) >= minimum + margin
            })
        })
    }

    /// Moves `color` toward the foreground extreme until it clears `rules`: not at all when
    /// it already does, otherwise to the first lightness that clears them with the margin, or
    /// failing that without it.
    fn tune(self, color: u32, rules: &[(&[u32], f64)], margin: f64) -> u32 {
        if self.clears(color, rules, 0.) {
            return color;
        }
        let extreme = self.foreground_extreme();
        let tuned = toward(color, extreme, |candidate| {
            self.clears(candidate, rules, margin)
        });
        if self.clears(tuned, rules, margin) {
            tuned
        } else {
            toward(color, extreme, |candidate| {
                self.clears(candidate, rules, 0.)
            })
        }
    }

    fn foreground_extreme(self) -> u32 {
        if self.dark { WHITE } else { BLACK }
    }

    fn background_extreme(self) -> u32 {
        if self.dark { BLACK } else { WHITE }
    }
}

/// The fitted palette for `colors`, before the readability check.
pub(crate) fn fit(colors: &Colors) -> Palette {
    let judge = Judge {
        dark: colors.mode == Mode::Dark,
    };
    let lanes = crate::graph::lane_colors(!judge.dark);
    // Every row surface keeps the lanes at `LANE_TARGET`: in a dark palette a surface is at
    // most this luminance, in a light one at least.
    let lane_limit = if judge.dark {
        let darkest = lanes.iter().map(|&lane| luminance(lane)).fold(1., f64::min);
        (darkest + 0.05) / LANE_TARGET - 0.05
    } else {
        let lightest = lanes.iter().map(|&lane| luminance(lane)).fold(0., f64::max);
        (lightest + 0.05) * LANE_TARGET - 0.05
    };
    let within = |color: u32, ratio: f64| {
        let room = if judge.dark {
            (luminance(color) + 0.05) * ratio - 0.05
        } else {
            (luminance(color) + 0.05) / ratio - 0.05
        };
        if judge.dark {
            room <= lane_limit
        } else {
            room >= lane_limit
        }
    };
    let background_extreme = judge.background_extreme();
    let settle = |color: u32| toward(color, background_extreme, |candidate| within(candidate, 1.));
    // Lift a surface off each of `references` toward the foreground side by `ratio`.
    let lift = |color: u32, references: &[u32], ratio: f64| {
        let steps =
            |candidate: u32, ratio: f64| judge.clears(candidate, &[(references, ratio)], 0.);
        if steps(color, ratio) {
            return color;
        }
        toward(color, judge.foreground_extreme(), |candidate| {
            steps(candidate, ratio + SURFACE_MARGIN)
        })
    };

    // Surfaces.
    let canvas = settle(colors.background);
    let panel = toward(
        colors
            .dark_background
            .unwrap_or_else(|| mix(canvas, BLACK, 0.12)),
        background_extreme,
        |candidate| within(candidate, PANEL_ROOM),
    );
    // Bring a surface lifted further than `ratio` off `reference` back toward it, in lightness
    // only: the mirror of `lift`.
    let cap = |color: u32, reference: u32, ratio: f64| {
        toward(color, background_extreme, |candidate| {
            !judge.clears(candidate, &[(&[reference], ratio)], 0.)
        })
    };
    let subtle = cap(
        settle(
            colors
                .lighter_background
                .filter(|&lighter| contrast(lighter, colors.background) >= FLAT_SURFACE)
                .unwrap_or_else(|| mix(canvas, colors.foreground, 0.06)),
        ),
        panel,
        SUBTLE_CAP,
    );
    // The floor, which wins over the cap: subtle stands `SUBTLE_FLOOR` off the canvas toward
    // the foreground, where a canvas already well off the panel left the capped subtle at or
    // under it (Nord, Everforest, Gruvbox), and off the panel as well, where a subtle between
    // canvas and panel all but vanished on it (Rosé Pine, Catppuccin Latte). Either step is
    // taken only while subtle stays within the lanes' limit and leaves the selected row, which
    // lifts at most `SELECTED_CAP` off the canvas, the same step beyond it: a panel far off
    // the canvas (a generated light theme's, 1.32:1) would otherwise put the grouped surfaces
    // beside the selected row.
    let floored = |references: &[u32]| {
        let clears = |candidate: u32, margin: f64| {
            judge.clears(candidate, &[(references, SUBTLE_FLOOR)], margin)
        };
        if clears(subtle, 0.) {
            return Some(subtle);
        }
        let lifted = toward(subtle, judge.foreground_extreme(), |candidate| {
            clears(candidate, SURFACE_MARGIN)
        });
        let room = SELECTED_CAP / SUBTLE_FLOOR;
        (clears(lifted, SURFACE_MARGIN)
            && within(lifted, 1.)
            && !judge.clears(lifted, &[(&[canvas], room)], 0.))
        .then_some(lifted)
    };
    let subtle = floored(&[canvas, panel])
        .or_else(|| floored(&[canvas]))
        .unwrap_or(subtle);
    // The selected row's own step off the panel wins over the cap.
    let selected = lift(
        cap(
            settle(
                colors
                    .selection
                    .unwrap_or_else(|| mix(canvas, colors.accent, 0.25)),
            ),
            canvas,
            SELECTED_CAP,
        ),
        &[panel],
        1.15,
    );
    // Subtle brought back toward the background until `hover` stands its step off it.
    let under = |subtle: u32, hover: u32| {
        let step = HOVER_STEP + SURFACE_MARGIN;
        toward(subtle, background_extreme, |candidate| {
            judge.clears(hover, &[(&[candidate], step)], 0.)
        })
    };
    // Hover stands its step off the panel and off subtle, where a secondary button rests. The
    // lane limit wins: where it holds hover under that step, subtle comes back under hover.
    let lifted = lift(
        settle(mix(canvas, selected, 0.5)),
        &[panel, subtle],
        HOVER_STEP,
    );
    let hover = settle(lifted);
    let subtle = if hover == lifted {
        subtle
    } else {
        under(subtle, hover)
    };
    let border = lift(
        mix(
            canvas,
            colors
                .muted
                .unwrap_or_else(|| mix(canvas, colors.foreground, 0.3)),
            0.7,
        ),
        &[panel],
        1.3,
    );
    let mut palette = Palette {
        canvas,
        panel,
        subtle,
        hover,
        border,
        selected,
        ..if judge.dark {
            ThemeChoice::Midnight.palette()
        } else {
            ThemeChoice::Daylight.palette()
        }
    };

    // The accent, whose hovered selected row (`row_hover(true)`) blends it into `selected`.
    let fit_accent = |palette: Palette, accent: u32| {
        let accepts = |candidate: u32, margin: f64| {
            let row = Palette {
                accent: candidate,
                ..palette
            }
            .row_hover(true);
            judge.clears(
                candidate,
                &[(
                    &[
                        palette.canvas,
                        palette.panel,
                        palette.subtle,
                        palette.hover,
                        palette.selected,
                        row,
                    ],
                    3.0,
                )],
                margin,
            )
        };
        if accepts(accent, 0.) {
            return accent;
        }
        let tuned = toward(accent, judge.foreground_extreme(), |candidate| {
            accepts(candidate, CONTRAST_MARGIN)
        });
        if accepts(tuned, CONTRAST_MARGIN) {
            tuned
        } else {
            toward(accent, judge.foreground_extreme(), |candidate| {
                accepts(candidate, 0.)
            })
        }
    };
    // The primary button's label reads on the accent; finding one may move the accent away
    // from the surfaces, which only raises its contrast there.
    let accent_for = |palette: Palette, accent: u32| {
        accent_label(judge, fit_accent(palette, accent), palette.canvas)
    };
    (palette.accent, palette.accent_foreground) = accent_for(palette, colors.accent);
    // The pressed button's layer stands apart from its hover on every surface, with the
    // selected row beyond hover. Each round takes the first move open, in this order: lift the
    // selected row while it keeps the lanes and its cap; bring hover back toward the panel
    // while it keeps its step off the panel, with subtle coming back under it while subtle
    // keeps its floor. Only while the pressed step is below its rule do the cap and then the
    // floor give way: lift the selected row past its cap, then bring hover back with subtle
    // under it past its floor. Hover never loses its step off subtle.
    let foreground_side = if judge.dark { 1. } else { 0. };
    let floor: Vec<u32> = [canvas, panel]
        .into_iter()
        .filter(|&reference| judge.clears(subtle, &[(&[reference], SUBTLE_FLOOR)], 0.))
        .collect();
    // Hover a step back toward the panel, keeping its step off the panel, with subtle
    // brought back under it; `floored` keeps subtle on its floor too.
    let lower_hover = |palette: Palette, floored: bool| {
        let (h, s, l) = to_hsl(palette.hover);
        let lowered = from_hsl(h, s, l + (1. - foreground_side - l) / 32.);
        let subtle = under(palette.subtle, lowered);
        (lowered != palette.hover
            && judge.clears(lowered, &[(&[panel], HOVER_STEP + SURFACE_MARGIN)], 0.)
            && !(floored && !judge.clears(subtle, &[(&floor, SUBTLE_FLOOR)], 0.)))
        .then_some(Palette {
            hover: lowered,
            subtle,
            ..palette
        })
    };
    // The selected row lifts in the fitter's lightness steps, counted from where it starts so
    // that rounding does not drift its hue: the next step that changes it, while it keeps the
    // lanes and, when `capped`, its cap.
    let start = to_hsl(palette.selected);
    let raise = |from: u32, selected: u32, capped: bool| {
        let (h, s, l) = start;
        (from + 1..=LIGHTNESS_STEPS)
            .map(|step| {
                let amount = f64::from(step) / f64::from(LIGHTNESS_STEPS);
                (step, from_hsl(h, s, l + (foreground_side - l) * amount))
            })
            .find(|&(_, candidate)| candidate != selected)
            .filter(|&(_, candidate)| {
                within(candidate, 1.)
                    && !(capped && judge.clears(candidate, &[(&[canvas], SELECTED_CAP)], 0.))
            })
    };
    let in_order =
        |palette: &Palette| judge.clears(palette.selected, &[(&[palette.hover], 1.)], 0.);
    let mut raised = 0;
    // The first round that met the pressed target, whether or not in order.
    let mut first = None;
    for _ in 0..PRESS_ROUNDS {
        let pressed = palette.pressed_step();
        if pressed >= PRESSED_TARGET {
            first.get_or_insert(palette);
            if in_order(&palette) {
                break;
            }
        }
        let short = pressed < PRESSED_STEP;
        if let Some((step, selected)) = raise(raised, palette.selected, true) {
            (raised, palette.selected) = (step, selected);
        } else if let Some(next) = lower_hover(palette, true) {
            palette = next;
        } else if let Some((step, selected)) = short
            .then(|| raise(raised, palette.selected, false))
            .flatten()
        {
            (raised, palette.selected) = (step, selected);
        } else if let Some(next) = short.then(|| lower_hover(palette, false)).flatten() {
            palette = next;
        } else {
            break;
        }
        (palette.accent, palette.accent_foreground) = accent_for(palette, palette.accent);
    }
    // The order is kept only where it also meets the rule. Otherwise the first round that met
    // the target stands, as the same moves without the order would have stopped there, and
    // the selected row may then stand on hover's side in lightness, apart from it in a
    // channel.
    let unordered = first.unwrap_or(palette);
    if !(in_order(&palette) && palette.pressed_step() >= PRESSED_STEP)
        && unordered.pressed_step() >= PRESSED_STEP
    {
        palette = unordered;
    }
    (palette.accent_hover, palette.accent_active) =
        accent_states(palette.accent, palette.accent_foreground);
    let (subtle, hover, selected) = (palette.subtle, palette.hover, palette.selected);
    let row = palette.row_hover(true);
    let surfaces = [canvas, panel, subtle, hover, selected, row];

    // Status colors: icons on every surface, and a label in `canvas` on their fills.
    let on_canvas = [canvas];
    let status = |color: u32, extra: &[(&[u32], f64)]| {
        let mut rules: Vec<(&[u32], f64)> = vec![(&surfaces, 3.0), (&on_canvas, 4.5)];
        rules.extend_from_slice(extra);
        judge.tune(color, &rules, CONTRAST_MARGIN)
    };
    // GitTurtle's own colors for the mode: Braden's in light, Midnight's in dark.
    let own = if judge.dark {
        ThemeChoice::Midnight.palette()
    } else {
        ThemeChoice::Daylight.palette()
    };
    let tint = if judge.dark {
        DIFF_TINT_DARK
    } else {
        DIFF_TINT_LIGHT
    };
    // The four file statuses from added and removed: the diff tiles and the colors on them,
    // then modified and renamed as `yellow` and `magenta` where they lie in their hue windows
    // (amber, violet) and stand apart from the others, and otherwise GitTurtle's own.
    let statuses = |added: u32, removed: u32, yellow: u32, magenta: u32| {
        let added_background = settle(mix(canvas, added, tint));
        let removed_background = settle(mix(canvas, removed, tint));
        let tiles = [added_background, removed_background];
        let added = status(added, &[(&[added_background], 4.5)]);
        let removed = status(removed, &[(&[removed_background], 4.5)]);
        let icon =
            |color: u32| judge.tune(color, &[(&surfaces, 3.0), (&tiles, 3.0)], CONTRAST_MARGIN);
        let theirs = icon(yellow);
        let modified =
            if MODIFIED_HUES.contains(&oklch(theirs).1) && apart(theirs, &[added, removed]) {
                theirs
            } else {
                icon(own.modified)
            };
        let theirs = icon(magenta);
        let renamed = if RENAMED_HUES.contains(&oklch(theirs).1)
            && apart(theirs, &[added, removed, modified])
        {
            theirs
        } else {
            icon(own.renamed)
        };
        (tiles, [added, removed, modified, renamed])
    };
    let red = status(colors.red, &[]);
    let green = status(colors.green, &[]);
    let (own_added, own_removed) = (status(own.added, &[]), status(own.removed, &[]));
    // The first set whose four statuses stand apart, or all four of GitTurtle's own.
    let ([added_background, removed_background], [added, removed, modified, renamed]) = status_set(
        statuses,
        [green, red, colors.yellow, colors.magenta],
        [own_added, own_removed, own.modified, own.renamed],
    );
    palette.added_background = added_background;
    palette.removed_background = removed_background;
    palette.added = added;
    palette.removed = removed;
    palette.modified = modified;
    palette.renamed = renamed;
    let tiles = [added_background, removed_background];
    // A grey warning is no warning, nor is one outside `WARNING_HUES` (Matte Black's red,
    // Lupine's blue): it takes GitTurtle's, which shares modified's amber.
    let warning = |color: u32| status(color, &[(&[subtle], 4.5)]);
    let theirs = warning(colors.orange.unwrap_or(colors.yellow));
    let (chroma, hue) = oklch(theirs);
    palette.warning = if chroma >= DIFF_MIN_CHROMA && WARNING_HUES.contains(&hue) {
        theirs
    } else {
        warning(own.warning)
    };

    // Text, then secondary text and line numbers lifted toward it.
    let text_rules: [(&[u32], f64); 2] = [(&surfaces, 4.5), (&tiles, 4.5)];
    let text = judge.tune(colors.foreground, &text_rules, CONTRAST_MARGIN);
    palette.text = text;
    // Hunk headers stay blue and apart from the code text: a `blue` without chroma, outside
    // `HUNK_HUES` (Matte Black's orange, Osaka Jade's green), or one that differs from the
    // text only in lightness, takes GitTurtle's. GitTurtle's can sit beside a blue text too,
    // closer than the theme's blue; the header then keeps whichever of the two blues stands
    // farther from the text.
    let hunk = |color: u32| {
        judge.tune(
            color,
            &[
                (&[canvas, panel, subtle], 4.5),
                (&[hover, selected, row], 3.0),
            ],
            CONTRAST_MARGIN,
        )
    };
    let apart_from_text = |color: u32| tint_distance(color, text) >= DIFF_MIN_CHROMA;
    let theirs = hunk(colors.blue);
    let blue = oklch(theirs).0 >= DIFF_MIN_CHROMA && HUNK_HUES.contains(&oklch(theirs).1);
    palette.hunk = if blue && apart_from_text(theirs) {
        theirs
    } else {
        let gitturtles = hunk(own.hunk);
        if !blue
            || apart_from_text(gitturtles)
            || tint_distance(gitturtles, text) >= tint_distance(theirs, text)
        {
            gitturtles
        } else {
            theirs
        }
    };
    let dim = colors
        .dark_foreground
        .unwrap_or_else(|| mix(colors.foreground, canvas, 0.4));
    // The first blend toward text that clears the rules with the margin, else without it;
    // text itself always clears them.
    let toward_text = |rules: &[(&[u32], f64)]| {
        if judge.clears(dim, rules, 0.) {
            return dim;
        }
        let blend = |margin: f64| {
            (1..=64)
                .map(|step| mix(dim, text, f64::from(step) / 64.))
                .find(|&candidate| judge.clears(candidate, rules, margin))
        };
        blend(CONTRAST_MARGIN).or_else(|| blend(0.)).unwrap_or(text)
    };
    let muted = toward_text(&text_rules);
    // Secondary text never reads above body text.
    palette.muted = if contrast(muted, canvas) > contrast(text, canvas) {
        text
    } else {
        muted
    };
    palette.line_number = toward_text(&[(&[canvas, panel], 4.5)]);
    palette
}

/// The primary button's label: the canvas color when it reads on the accent, otherwise black
/// or white. When neither reads with the margin, the accent moves away from the surfaces (which
/// only raises its own contrast there) until the canvas side's extreme does.
fn accent_label(judge: Judge, accent: u32, canvas: u32) -> (u32, u32) {
    let minimum = 4.5 + CONTRAST_MARGIN;
    let reads = |label: u32, accent: u32| contrast(label, accent) >= minimum;
    for label in [
        canvas,
        judge.background_extreme(),
        judge.foreground_extreme(),
    ] {
        if reads(label, accent) {
            return (accent, label);
        }
    }
    let label = judge.background_extreme();
    let accent = toward(accent, judge.foreground_extreme(), |candidate| {
        reads(label, candidate)
    });
    (accent, label)
}

/// Hover and pressed shades of the accent: hover a step away from the label, pressed a step
/// toward it when the label still reads there, and otherwise further away than hover.
fn accent_states(accent: u32, label: u32) -> (u32, u32) {
    let (h, s, l) = to_hsl(accent);
    let away = if luminance(label) < luminance(accent) {
        1.
    } else {
        -1.
    };
    let shade = |amount: f64| from_hsl(h, s, (l + away * amount).clamp(0., 1.));
    let hover = shade(0.06);
    let active = [-0.06, -0.03]
        .into_iter()
        .map(shade)
        .find(|&candidate| contrast(label, candidate) >= 4.5 + CONTRAST_MARGIN)
        .unwrap_or_else(|| shade(0.12));
    (hover, active)
}

/// The diff tiles and four file statuses `statuses` fits from an added, removed, yellow and
/// magenta, given the theme's and GitTurtle's colors in status order (added and removed
/// already fitted). The first set whose four statuses stand apart: the theme's red and green
/// where they are distinguishable, then GitTurtle's (when GitTurtle's own modified or renamed
/// would sit beside the theme's, as Matte Black's amber added beside GitTurtle's amber
/// modified), each with the theme's yellow and magenta where `statuses` keeps them. A replacement
/// is not checked against the color kept beside it, so when neither set stands apart, all
/// four are GitTurtle's own. Since modified and renamed keep only amber and violet, no known
/// theme reaches that step: a replacement violet renamed can no longer land on a kept
/// modified.
fn status_set(
    statuses: impl Fn(u32, u32, u32, u32) -> ([u32; 2], [u32; 4]),
    [green, red, yellow, magenta]: [u32; 4],
    [own_added, own_removed, own_modified, own_renamed]: [u32; 4],
) -> ([u32; 2], [u32; 4]) {
    distinguishable(red, green)
        .then(|| statuses(green, red, yellow, magenta))
        .into_iter()
        .chain(std::iter::once_with(|| {
            statuses(own_added, own_removed, yellow, magenta)
        }))
        .find(|&(_, set)| stand_apart(set))
        .unwrap_or_else(|| statuses(own_added, own_removed, own_modified, own_renamed))
}

/// Whether a theme's fitted red and green are far enough apart to mark removed and added
/// lines: both keep `DIFF_MIN_CHROMA` of OKLCH chroma and their hues are `DIFF_MIN_HUE`
/// degrees apart.
pub(crate) fn distinguishable(red: u32, green: u32) -> bool {
    oklch(red).0.min(oklch(green).0) >= DIFF_MIN_CHROMA && hue_distance(red, green) >= DIFF_MIN_HUE
}

/// Whether `color` reads as a status of its own beside `others`: it keeps `DIFF_MIN_CHROMA`
/// of chroma and lies at least `STATUS_MIN_HUE` degrees of hue from each of them.
fn apart(color: u32, others: &[u32]) -> bool {
    oklch(color).0 >= DIFF_MIN_CHROMA
        && others
            .iter()
            .all(|&other| hue_distance(color, other) >= STATUS_MIN_HUE)
}

/// Whether four statuses (added, removed, modified, renamed) stand pairwise apart.
fn stand_apart([added, removed, modified, renamed]: [u32; 4]) -> bool {
    apart(added, &[removed, modified, renamed])
        && apart(removed, &[modified, renamed])
        && apart(modified, &[renamed])
}

/// The angle between two colors' OKLCH hues, in degrees.
fn hue_distance(a: u32, b: u32) -> f64 {
    let apart = (oklch(a).1 - oklch(b).1).abs() % 360.;
    apart.min(360. - apart)
}

/// How far apart two colors are in hue and chroma alone: their distance in OKLab's a–b
/// plane, which leaves lightness out. Against a grey it is the color's chroma, so it asks of
/// a hunk header what `DIFF_MIN_CHROMA` asks of a grey: that it reads as a color of its own
/// beside the code text, not as the text a shade lighter or darker.
fn tint_distance(a: u32, b: u32) -> f64 {
    let (_, a1, b1) = oklab(a);
    let (_, a2, b2) = oklab(b);
    (a1 - a2).hypot(b1 - b2)
}

/// OKLCH chroma and hue in degrees of an `0xrrggbb` color.
fn oklch(color: u32) -> (f64, f64) {
    let (_, a, b) = oklab(color);
    (a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.))
}

/// OKLab lightness, a and b of an `0xrrggbb` color.
fn oklab(color: u32) -> (f64, f64, f64) {
    let linear = |shift: u32| {
        let value = f64::from((color >> shift) & 255) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(16), linear(8), linear(0));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    (
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    )
}

/// `from` blended toward `to` by `amount` in each 8-bit channel.
fn mix(from: u32, to: u32, amount: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |color, shift| {
        let a = f64::from((from >> shift) & 255);
        let b = f64::from((to >> shift) & 255);
        color | (((a + (b - a) * amount).round().clamp(0., 255.) as u32) << shift)
    })
}

/// Steps of lightness between a color and black or white.
const LIGHTNESS_STEPS: u32 = 256;

/// The first color from `color` toward `extreme` (black or white) that `accepts`, changing
/// only its HSL lightness so hue and saturation stay; the extreme itself when none does.
fn toward(color: u32, extreme: u32, accepts: impl Fn(u32) -> bool) -> u32 {
    if accepts(color) {
        return color;
    }
    let (h, s, l) = to_hsl(color);
    let target = if extreme == WHITE { 1. } else { 0. };
    (1..=LIGHTNESS_STEPS)
        .map(|step| {
            from_hsl(
                h,
                s,
                l + (target - l) * f64::from(step) / f64::from(LIGHTNESS_STEPS),
            )
        })
        .find(|&candidate| accepts(candidate))
        .unwrap_or(extreme)
}

fn to_hsl(color: u32) -> (f64, f64, f64) {
    let channel = |shift: u32| f64::from((color >> shift) & 255) / 255.;
    let (r, g, b) = (channel(16), channel(8), channel(0));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.;
    let d = max - min;
    if d == 0. {
        return (0., 0., l);
    }
    let s = if l > 0.5 {
        d / (2. - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6. } else { 0. }
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h / 6., s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> u32 {
    let l = l.clamp(0., 1.);
    let channels = if s == 0. {
        [l; 3]
    } else {
        let q = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
        let p = 2. * l - q;
        let hue = |t: f64| {
            let t = t.rem_euclid(1.);
            if t < 1. / 6. {
                p + (q - p) * 6. * t
            } else if t < 0.5 {
                q
            } else if t < 2. / 3. {
                p + (q - p) * (2. / 3. - t) * 6.
            } else {
                p
            }
        };
        [hue(h + 1. / 3.), hue(h), hue(h - 1. / 3.)]
    };
    channels.into_iter().fold(0, |color, channel| {
        (color << 8) | (channel * 255.).round().clamp(0., 255.) as u32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every theme Omarchy 4.0.4 bundles, with its declared mode.
    const BUNDLED: [(&str, &[u8], Mode); 22] = {
        macro_rules! theme {
            ($name:literal, $mode:ident) => {
                (
                    $name,
                    include_bytes!(concat!("../../../tests/fixtures/omarchy/", $name, ".toml")),
                    Mode::$mode,
                )
            };
        }
        [
            theme!("catppuccin", Dark),
            theme!("catppuccin-latte", Light),
            theme!("ethereal", Dark),
            theme!("everforest", Dark),
            theme!("flexoki-light", Light),
            theme!("gruvbox", Dark),
            theme!("hackerman", Dark),
            theme!("kanagawa", Dark),
            theme!("last-horizon", Dark),
            theme!("lumon", Dark),
            theme!("lupine", Light),
            theme!("matte-black", Dark),
            theme!("miasma", Dark),
            theme!("nord", Dark),
            theme!("osaka-jade", Dark),
            theme!("retro-82", Dark),
            theme!("ristretto", Dark),
            theme!("rose-pine", Light),
            theme!("solitude", Dark),
            theme!("tokyo-night", Dark),
            theme!("vantablack", Dark),
            theme!("white", Light),
        ]
    };

    fn bundled(name: &str) -> Colors {
        let (_, bytes, _) = BUNDLED
            .iter()
            .find(|(theme, _, _)| *theme == name)
            .expect("a bundled theme");
        parse(bytes, false).expect("bundled themes parse")
    }

    fn own(mode: Mode) -> Palette {
        match mode {
            Mode::Light => ThemeChoice::Daylight.palette(),
            Mode::Dark => ThemeChoice::Midnight.palette(),
        }
    }

    /// All 22 bundled themes parse, keep the mode they declare, and fit without a single
    /// readability finding, so none of them needs the built-in fallback.
    #[test]
    fn every_bundled_theme_maps_in_its_own_mode_without_findings() {
        for (name, bytes, declared) in BUNDLED {
            let colors = parse(bytes, false).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(colors.mode, declared, "{name}");
            let fitted = fit(&colors);
            assert_eq!(
                fitted.readability_issues(),
                Vec::new(),
                "{name} fits every readability rule"
            );
            let mapped = map(&colors);
            assert_eq!(mapped.palette, fitted, "{name} applies its own fit");
            assert_eq!(mapped.is_light, declared == Mode::Light, "{name}");
            assert_eq!(mapped.palette.is_light(), mapped.is_light, "{name}");
        }
    }

    /// Values that already pass stay as the theme wrote them: Tokyo Night keeps its
    /// background, accent, status colors and foreground, as GitTurtle's own Tokyo Night keeps
    /// the same upstream hues; its dim foreground (2.9:1) is lifted toward the foreground only
    /// until secondary text reads on every surface. Its lighter background, 1.26:1 off the
    /// panel, is brought back toward the panel in lightness only, as far as the subtle floor
    /// off its canvas allows. Its selection, 1.27:1 off the canvas, is lifted in lightness
    /// only, for the pressed step beyond a hover that stands its step off subtle.
    #[test]
    fn a_passing_theme_keeps_its_own_colors() {
        let colors = bundled("tokyo-night");
        let palette = map(&colors).palette;
        assert_eq!(palette.canvas, colors.background);
        assert_eq!(Some(palette.panel), colors.dark_background);
        let lighter = colors.lighter_background.unwrap();
        assert!(contrast(lighter, palette.panel) > SUBTLE_CAP + 0.1);
        assert!(contrast(palette.subtle, palette.panel) < contrast(lighter, palette.panel));
        let floor = SUBTLE_FLOOR + SURFACE_MARGIN;
        let off_canvas = contrast(palette.subtle, palette.canvas);
        assert!(
            (floor..floor + 0.02).contains(&off_canvas),
            "{off_canvas:.3}"
        );
        let (hue, saturation, _) = to_hsl(palette.subtle);
        let (theirs, their_saturation, _) = to_hsl(lighter);
        assert!((hue - theirs).abs() < 0.01 && (saturation - their_saturation).abs() < 0.05);
        let selection = colors.selection.unwrap();
        let (hue, saturation, _) = to_hsl(palette.selected);
        let (theirs, their_saturation, _) = to_hsl(selection);
        assert!((hue - theirs).abs() < 0.01 && (saturation - their_saturation).abs() < 0.05);
        assert!(contrast(palette.selected, palette.canvas) > contrast(selection, palette.canvas));
        assert!(contrast(palette.selected, palette.canvas) < SELECTED_CAP);
        assert_eq!(palette.accent, colors.accent);
        assert_eq!(palette.accent, ThemeChoice::TokyoNight.palette().accent);
        assert_eq!(palette.text, colors.foreground);
        assert_eq!(palette.added, colors.green);
        assert_eq!(palette.removed, colors.red);
        assert_eq!(palette.modified, colors.yellow);
        assert_eq!(palette.renamed, colors.magenta);
        assert_eq!(Some(palette.warning), colors.orange);
        assert_eq!(palette.hunk, colors.blue);
        assert_eq!(palette.accent_foreground, colors.background);
        let dim = colors.dark_foreground.unwrap();
        assert!(contrast(dim, palette.canvas) < 4.5);
        assert!(contrast(palette.muted, palette.canvas) > contrast(dim, palette.canvas));
        assert!(contrast(palette.muted, palette.canvas) <= contrast(palette.text, palette.canvas));
    }

    /// Diff colors keep the theme's red and green where they are distinguishable (OKLCH
    /// chroma ≥ `DIFF_MIN_CHROMA` for both, hues ≥ `DIFF_MIN_HUE` apart after fitting), and
    /// otherwise take GitTurtle's own added and removed hues for the mode, fitted to the
    /// theme's surfaces: Hackerman's green "red", White's greys, and a synthetic theme whose
    /// red and green differ by one step in each channel.
    #[test]
    fn indistinguishable_red_and_green_mark_diffs_in_gitturtle_hues() {
        let tokyo = bundled("tokyo-night");
        assert!(distinguishable(tokyo.red, tokyo.green));
        for (name, colors) in [
            ("hackerman", bundled("hackerman")),
            ("white", bundled("white")),
            ("vantablack", bundled("vantablack")),
            ("lumon", bundled("lumon")),
            (
                "synthetic",
                Colors {
                    green: tokyo.red ^ 0x010101,
                    ..tokyo
                },
            ),
        ] {
            let palette = fit(&colors);
            assert!(
                !distinguishable(colors.red, colors.green),
                "{name}'s red and green look alike"
            );
            assert_eq!(palette.readability_issues(), Vec::new(), "{name}");
            let own = own(colors.mode);
            for (token, color, gitturtle) in [
                ("added", palette.added, own.added),
                ("removed", palette.removed, own.removed),
            ] {
                let apart = (oklch(color).1 - oklch(gitturtle).1).abs();
                assert!(
                    apart.min(360. - apart) < 8.,
                    "{name}'s {token} {color:06x} keeps GitTurtle's hue ({gitturtle:06x})"
                );
            }
            assert!(distinguishable(palette.removed, palette.added), "{name}");
        }
        // Low chroma alone is enough: two greys far apart in hue angle still look alike.
        assert!(!distinguishable(0x7f7f80, 0x80807f));
        // Hue alone too: a vivid red beside a vivid orange.
        assert!(!distinguishable(0xe03030, 0xe07020));
        assert!(distinguishable(0xe03030, 0x30b050));
    }

    /// Every bundled theme and the two generated from an `alacritty.toml`, by file name.
    fn fixtures() -> Vec<(&'static str, Colors)> {
        let generated = [
            (
                "alacritty-dark",
                &include_bytes!("../../../tests/fixtures/omarchy/alacritty-dark.toml")[..],
            ),
            (
                "alacritty-light",
                &include_bytes!("../../../tests/fixtures/omarchy/alacritty-light.toml")[..],
            ),
        ];
        BUNDLED
            .iter()
            .map(|(name, bytes, _)| (*name, *bytes))
            .chain(generated)
            .map(|(name, bytes)| (name, parse(bytes, false).unwrap()))
            .collect()
    }

    /// Modified and renamed stand apart from added, removed and each other in every fixture:
    /// the theme's yellow and magenta where they keep `DIFF_MIN_CHROMA` of chroma and
    /// `STATUS_MIN_HUE` degrees of hue from the others, GitTurtle's own otherwise. Where
    /// GitTurtle's own would sit beside the theme's red and green, the diff takes GitTurtle's
    /// hues too (Matte Black's amber added; Miasma's tan removed, 0.043 chroma once fitted).
    /// The themes that take each fallback are named, and a grey warning takes GitTurtle's.
    #[test]
    fn status_colors_stand_apart_or_take_gitturtles_own() {
        let mut took = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for (name, colors) in fixtures() {
            let palette = fit(&colors);
            let own = own(colors.mode);
            let statuses = [
                palette.added,
                palette.removed,
                palette.modified,
                palette.renamed,
            ];
            for (index, &status) in statuses.iter().enumerate() {
                assert!(
                    apart(status, &statuses[index + 1..]),
                    "{name}: {statuses:06x?} stand apart"
                );
            }
            // Which it took: whichever of the theme's color and GitTurtle's it is nearer.
            let gitturtles = |color: u32, theirs: u32, gitturtle: u32| {
                hue_distance(color, gitturtle) < hue_distance(color, theirs)
            };
            for (list, taken) in took.iter_mut().zip([
                gitturtles(palette.modified, colors.yellow, own.modified),
                gitturtles(palette.renamed, colors.magenta, own.renamed),
                gitturtles(palette.removed, colors.red, own.removed),
                oklch(colors.orange.unwrap_or(colors.yellow)).0 < DIFF_MIN_CHROMA,
            ]) {
                if taken {
                    list.push(name);
                }
            }
            assert!(
                oklch(palette.warning).0 >= DIFF_MIN_CHROMA,
                "{name}'s warning"
            );
        }
        assert_eq!(
            took,
            [
                vec![
                    "hackerman",
                    "last-horizon",
                    "lumon",
                    "lupine",
                    "matte-black",
                    "osaka-jade",
                    "retro-82",
                    "solitude",
                    "vantablack",
                    "white",
                ],
                vec![
                    "catppuccin",
                    "everforest",
                    "gruvbox",
                    "hackerman",
                    "last-horizon",
                    "lumon",
                    "matte-black",
                    "miasma",
                    "retro-82",
                    "solitude",
                    "vantablack",
                    "white",
                ],
                // Red and green that look alike (the diff rule), and then Matte Black and
                // Miasma, whose own statuses cannot stand apart beside them.
                vec![
                    "ethereal",
                    "hackerman",
                    "last-horizon",
                    "lumon",
                    "lupine",
                    "matte-black",
                    "miasma",
                    "solitude",
                    "vantablack",
                    "white",
                ],
                vec!["last-horizon", "solitude", "vantablack", "white"],
            ],
            "modified, renamed, the diff and warning"
        );
    }

    /// When neither the theme's set nor GitTurtle's added and removed with the theme's yellow
    /// and magenta stands apart, the statuses are all four of the GitTurtle colors it is given,
    /// here with a dark theme's colors and a light one's. No fixture or generated theme reaches
    /// that step through `fit` (see `status_set`), so the choice is given a set fitter that
    /// keeps the colors it is given, and a yellow equal to the theme's magenta fails both
    /// earlier sets.
    #[test]
    fn statuses_that_never_stand_apart_take_all_four_of_gitturtles_own() {
        for name in ["tokyo-night", "catppuccin-latte"] {
            let colors = bundled(name);
            let own = own(colors.mode);
            let gitturtles = [own.added, own.removed, own.modified, own.renamed];
            let theirs = [colors.green, colors.red, colors.magenta, colors.magenta];
            assert!(distinguishable(colors.red, colors.green), "{name}");
            let tried = std::cell::RefCell::new(Vec::new());
            let (tiles, statuses) = status_set(
                |added, removed, yellow, magenta| {
                    let set = [added, removed, yellow, magenta];
                    tried.borrow_mut().push(set);
                    ([added, removed], set)
                },
                theirs,
                gitturtles,
            );
            assert_eq!(statuses, gitturtles, "{name} takes GitTurtle's own four");
            assert_eq!(tiles, [own.added, own.removed], "{name}");
            let second = [own.added, own.removed, colors.magenta, colors.magenta];
            assert_eq!(
                tried.into_inner(),
                [theirs, second, gitturtles],
                "{name} tries the theme's set, then GitTurtle's diff with its yellow and magenta"
            );
        }
    }

    fn statuses(palette: &Palette) -> [u32; 4] {
        [
            palette.added,
            palette.removed,
            palette.modified,
            palette.renamed,
        ]
    }

    /// GitTurtle's own four statuses for the mode, fitted to `colors`' surfaces: the fit of
    /// the same theme with GitTurtle's colors as its red, green, yellow and magenta.
    fn gitturtles_statuses(colors: &Colors) -> [u32; 4] {
        let own = own(colors.mode);
        statuses(&fit(&Colors {
            red: own.removed,
            green: own.added,
            yellow: own.modified,
            magenta: own.renamed,
            ..*colors
        }))
    }

    /// The warning keeps the theme's `orange` (its `yellow` when it has none) only inside
    /// `WARNING_HUES`, from orange-red to yellow, and renamed keeps its `magenta` only inside
    /// `RENAMED_HUES`, from blue-violet round to magenta-pink; otherwise each takes GitTurtle's
    /// own. Among the fixtures whose color has a hue of its own, the warning window leaves out
    /// Catppuccin's salmon (29.8°), Hackerman's green, Lumon's and Lupine's blues and Matte
    /// Black's red, and the renamed window Hackerman's periwinkle and Lumon's sky blue besides
    /// the colors the 30° rule already replaced. A Tokyo Night whose orange and magenta are a
    /// sky blue, which the 30° rule alone would keep, takes GitTurtle's for both.
    #[test]
    fn warning_and_renamed_keep_their_hues_or_take_gitturtles_own() {
        let mut outside = [Vec::new(), Vec::new()];
        for (name, colors) in fixtures() {
            let palette = fit(&colors);
            let own = own(colors.mode);
            for (list, (hues, color, theirs, gitturtles)) in outside.iter_mut().zip([
                (
                    WARNING_HUES,
                    palette.warning,
                    colors.orange.unwrap_or(colors.yellow),
                    own.warning,
                ),
                (RENAMED_HUES, palette.renamed, colors.magenta, own.renamed),
            ]) {
                assert!(
                    hues.contains(&oklch(color).1),
                    "{name}'s {color:06x} outside {hues:?}"
                );
                let (chroma, hue) = oklch(theirs);
                if chroma >= DIFF_MIN_CHROMA && !hues.contains(&hue) {
                    assert!(
                        hue_distance(color, gitturtles) < hue_distance(color, theirs),
                        "{name} takes GitTurtle's {gitturtles:06x}"
                    );
                    list.push(name);
                }
            }
        }
        assert_eq!(
            outside,
            [
                vec!["catppuccin", "hackerman", "lumon", "lupine", "matte-black"],
                vec![
                    "gruvbox",
                    "hackerman",
                    "lumon",
                    "matte-black",
                    "miasma",
                    "retro-82"
                ],
            ],
            "warning, renamed"
        );
        let tokyo = bundled("tokyo-night");
        let sky = 0x7dcfff;
        let palette = fit(&Colors {
            orange: Some(sky),
            magenta: sky,
            ..tokyo
        });
        assert!(apart(
            sky,
            &[palette.added, palette.removed, palette.modified]
        ));
        let gitturtles = fit(&Colors {
            orange: Some(own(Mode::Dark).warning),
            magenta: own(Mode::Dark).renamed,
            ..tokyo
        });
        assert_eq!(
            (palette.warning, palette.renamed),
            (gitturtles.warning, gitturtles.renamed)
        );
        assert_ne!(palette.renamed, sky);
        assert_eq!(palette.readability_issues(), Vec::new());
    }

    /// A replacement is never checked against the theme color kept beside it. Before Modified
    /// was held to amber, a Tokyo Night whose yellow and magenta are both GitTurtle's violet
    /// renamed `#C1A5F5` kept that violet as modified and took GitTurtle's renamed, the same
    /// violet, beside it, and only the last step, all four statuses GitTurtle's own, set them
    /// apart. The violet yellow now takes GitTurtle's amber and the violet stays renamed, with
    /// the theme's red and green kept.
    #[test]
    fn a_violet_yellow_leaves_modified_and_renamed_apart() {
        let tokyo = bundled("tokyo-night");
        let violet = own(Mode::Dark).renamed;
        let colors = Colors {
            yellow: violet,
            magenta: violet,
            ..tokyo
        };
        let palette = fit(&colors);
        assert!(
            stand_apart(statuses(&palette)),
            "{:06x?}",
            statuses(&palette)
        );
        let amber = fit(&Colors {
            yellow: own(Mode::Dark).modified,
            ..colors
        });
        assert_eq!(palette.modified, amber.modified);
        assert_eq!(palette.renamed, violet);
        assert_eq!((palette.added, palette.removed), (tokyo.green, tokyo.red));
        assert_eq!(palette.readability_issues(), Vec::new());
    }

    /// A `lighter_background` within `FLAT_SURFACE` of the background is no surface of its
    /// own: Last Horizon and Solitude repeat their background, and a generated theme's
    /// `color0` is its background, so each takes the canvas 6% toward the foreground and its
    /// grouped surfaces show. Lupine's, 1.045:1 off its background, is not flat.
    #[test]
    fn a_flat_lighter_background_counts_as_absent() {
        for (name, colors) in fixtures() {
            let lighter = colors.lighter_background.unwrap();
            let palette = fit(&colors);
            let flat = contrast(lighter, colors.background) < FLAT_SURFACE;
            assert_eq!(
                flat,
                [
                    "alacritty-dark",
                    "alacritty-light",
                    "last-horizon",
                    "solitude"
                ]
                .contains(&name),
                "{name}"
            );
            if flat {
                assert!(
                    contrast(palette.subtle, palette.canvas) >= 1.04,
                    "{name}'s subtle {:06x} stands off its canvas {:06x}",
                    palette.subtle,
                    palette.canvas
                );
            }
        }
    }

    /// Hunk headers stay blue and apart from the code text: White's, Vantablack's,
    /// Solitude's and Last Horizon's blue is a grey, and Nord's and Rosé Pine's keep 0.045 and
    /// 0.049 chroma once fitted, so each takes GitTurtle's hunk color. Matte Black's `blue` is
    /// an orange (its accent), Ristretto's a salmon beside its removed, Osaka Jade's a green
    /// beside its added and Miasma's an olive beside its text: outside `HUNK_HUES`, they take
    /// it too, while Everforest's and Retro-82's teal and Gruvbox's aqua (179°) are kept.
    /// Beside a blue text GitTurtle's can be as close
    /// as the theme's: the header keeps whichever stands farther from the text, the theme's
    /// `#6F9AF7` 0.026 from `#8FB0FF` over GitTurtle's `#95BAFF` at 0.015, and GitTurtle's
    /// over a blue that is the text itself.
    #[test]
    fn hunk_headers_stay_blue_and_apart_from_the_text() {
        let mut took = Vec::new();
        for (name, colors) in fixtures() {
            let palette = fit(&colors);
            assert!(oklch(palette.hunk).0 >= DIFF_MIN_CHROMA, "{name}");
            assert!(HUNK_HUES.contains(&oklch(palette.hunk).1), "{name}");
            assert!(
                tint_distance(palette.hunk, palette.text) >= DIFF_MIN_CHROMA,
                "{name}"
            );
            let own = own(colors.mode).hunk;
            if hue_distance(palette.hunk, own) < hue_distance(palette.hunk, colors.blue) {
                took.push(name);
            }
        }
        assert_eq!(
            took,
            [
                "last-horizon",
                "matte-black",
                "miasma",
                "nord",
                "osaka-jade",
                "ristretto",
                "rose-pine",
                "solitude",
                "vantablack",
                "white"
            ]
        );
        let blue_text = Colors {
            foreground: 0x8fb0ff,
            blue: 0x6f9af7,
            ..bundled("tokyo-night")
        };
        let gitturtles = fit(&Colors {
            blue: own(Mode::Dark).hunk,
            ..blue_text
        })
        .hunk;
        let palette = fit(&blue_text);
        assert_eq!(palette.text, blue_text.foreground);
        let from_text = |color: u32| tint_distance(color, palette.text);
        assert!(oklch(blue_text.blue).0 >= DIFF_MIN_CHROMA);
        assert!(
            from_text(gitturtles) < from_text(blue_text.blue)
                && from_text(blue_text.blue) < DIFF_MIN_CHROMA,
            "GitTurtle's {:.3} and the theme's {:.3} both sit beside the text",
            from_text(gitturtles),
            from_text(blue_text.blue)
        );
        assert_eq!(palette.hunk, blue_text.blue, "the theme's, the farther");
        assert_eq!(from_text(palette.hunk), from_text(blue_text.blue));
        let the_text = Colors {
            blue: blue_text.foreground,
            ..blue_text
        };
        let palette = fit(&the_text);
        assert_eq!(palette.hunk, gitturtles, "GitTurtle's, the farther");
        assert!(from_text(palette.hunk) > from_text(the_text.blue));
    }

    /// Subtle surfaces lift at most `SUBTLE_CAP` off the panel unless the subtle floor off the
    /// canvas needs more, and the selected row at most `SELECTED_CAP` off the canvas unless its
    /// own step off the panel or the pressed step needs more, brought back in lightness only:
    /// White's subtle and selected were both `#c0c0c0` (1.67:1 off its panel, 1.82:1 off its
    /// canvas) and Last Horizon's selected row 2.45:1 off its canvas. On those rows every graph
    /// lane keeps at least 4:1. The pressed step passes the cap only below its rule: Lupine's
    /// panel sits 1.13:1 off its canvas, which leaves subtle's floor, hover's step and the
    /// rule's 6 no room under the cap.
    #[test]
    fn subtle_and_selected_surfaces_lift_no_further_than_the_caps() {
        let mut pressed = Vec::new();
        for (name, colors) in fixtures() {
            let palette = fit(&colors);
            // A lift is toward the foreground: lighter in a dark palette, darker in a light
            // one. A generated light theme's subtle sits between its canvas and its much
            // darker panel, on the other side, and is not a lift.
            let judge = Judge {
                dark: colors.mode == Mode::Dark,
            };
            let lifted = |surface: u32, from: u32, cap: f64| {
                judge.clears(surface, &[(&[from], cap + 0.005)], 0.)
            };
            // The floor wins, lifting no further than it needs: one lightness step past its
            // margin.
            assert!(
                !lifted(palette.subtle, palette.panel, SUBTLE_CAP)
                    || !lifted(
                        palette.subtle,
                        palette.canvas,
                        SUBTLE_FLOOR + SURFACE_MARGIN + 0.02
                    ),
                "{name}"
            );
            // The selected row's own step off the panel wins: a generated light theme's
            // panel is dark enough that 1.15:1 below it is 1.55:1 below its canvas. So does
            // the pressed step: brought back to the cap, the selected row would stand too
            // close to hover.
            if lifted(palette.selected, palette.canvas, SELECTED_CAP)
                && lifted(
                    palette.selected,
                    palette.panel,
                    1.15 + SURFACE_MARGIN + 0.01,
                )
            {
                let capped = toward(palette.selected, judge.background_extreme(), |candidate| {
                    !judge.clears(candidate, &[(&[palette.canvas], SELECTED_CAP)], 0.)
                });
                let at_the_cap = Palette {
                    selected: capped,
                    ..palette
                };
                assert!(at_the_cap.pressed_step() < PRESSED_STEP, "{name}");
                pressed.push(name);
            }
        }
        assert_eq!(pressed, ["lupine"]);
        for name in ["white", "last-horizon"] {
            let palette = fit(&bundled(name));
            let lanes = crate::graph::lane_colors(palette.is_light());
            let weakest = lanes
                .iter()
                .map(|&lane| contrast(lane, palette.selected))
                .fold(f64::INFINITY, f64::min);
            assert!(
                weakest >= 4.,
                "{name}'s lanes on its selected row: {weakest:.2}"
            );
        }
    }

    /// Subtle stands at least `SUBTLE_FLOOR` off the canvas toward the foreground in every
    /// fixture. The cap alone left it at or under the canvas where the canvas sits well off
    /// the panel: Nord's and Everforest's 1.057:1 and 1.062:1 below it, Gruvbox's and
    /// Ristretto's 1.008:1 and 1.013:1 above. It stands as far off the panel where that leaves
    /// the selected row room, as for Rosé Pine's (1.023:1 lighter than its panel) and
    /// Catppuccin Latte's (1.041:1). A generated light theme's panel sits 1.32:1 below its
    /// canvas, where that step would put subtle beside the selected row; its subtle stays on
    /// the canvas side, clearly lighter than the panel.
    #[test]
    fn subtle_stands_off_the_canvas_and_the_panel() {
        for (name, colors) in fixtures() {
            let palette = fit(&colors);
            let judge = Judge {
                dark: colors.mode == Mode::Dark,
            };
            let off =
                |reference: u32| judge.clears(palette.subtle, &[(&[reference], SUBTLE_FLOOR)], 0.);
            assert!(
                off(palette.canvas),
                "{name}'s subtle {:06x} on its canvas {:06x}",
                palette.subtle,
                palette.canvas
            );
            assert_eq!(off(palette.panel), name != "alacritty-light", "{name}");
            assert_eq!(palette.readability_issues(), Vec::new(), "{name}");
        }
        let generated = fixtures()
            .into_iter()
            .find_map(|(name, colors)| (name == "alacritty-light").then(|| fit(&colors)))
            .unwrap();
        assert!(contrast(generated.subtle, generated.panel) >= SUBTLE_FLOOR);
        assert!(contrast(generated.selected, generated.subtle) >= SUBTLE_FLOOR);
    }

    /// Secondary buttons rest on subtle and hover to `hover`, so in every fixture hover stands
    /// at least `HOVER_STEP` off subtle toward the foreground, as it does off the panel, and
    /// the selected row, the pressed layer, stands beyond hover. The subtle floor had lifted
    /// subtle onto hover: Tokyo Night's hover `#222534` stood 1.011:1 off its `#212434`
    /// subtle, Lupine's was its subtle, and White's, Vantablack's and Flexoki Light's sat
    /// behind it. The pressed step keeps its rule everywhere and its margin where the cap and
    /// subtle's floor leave room: Catppuccin Latte and Lupine stop at the rule's 6.
    #[test]
    fn hover_stands_off_subtle_and_the_panel() {
        let mut at_the_rule = Vec::new();
        for (name, colors) in fixtures() {
            let palette = fit(&colors);
            let judge = Judge {
                dark: colors.mode == Mode::Dark,
            };
            for (surface, reference) in [("subtle", palette.subtle), ("panel", palette.panel)] {
                assert!(
                    judge.clears(palette.hover, &[(&[reference], HOVER_STEP)], 0.),
                    "{name}'s hover {:06x} on its {surface} {reference:06x}",
                    palette.hover
                );
            }
            assert!(
                judge.clears(palette.selected, &[(&[palette.hover], 1.)], 0.),
                "{name}'s selected row {:06x} beyond its hover {:06x}",
                palette.selected,
                palette.hover
            );
            assert!(palette.pressed_step() >= PRESSED_STEP, "{name}");
            if palette.pressed_step() < PRESSED_TARGET {
                at_the_rule.push(name);
            }
            assert_eq!(palette.readability_issues(), Vec::new(), "{name}");
        }
        assert_eq!(at_the_rule, ["catppuccin-latte", "lupine"]);
    }

    /// Modified keeps the theme's `yellow` only inside `MODIFIED_HUES`, from orange to yellow,
    /// and otherwise takes GitTurtle's amber. Among the fixtures whose yellow has a hue of its
    /// own, Lumon's `#6FA4C9` (240°) and Lupine's `#026FDE` (256°) are blues, Hackerman's a
    /// teal, Matte Black's a red and Osaka Jade's a green. A Tokyo Night whose yellow is its
    /// blue, which would otherwise stand apart from its red and green, takes GitTurtle's too.
    #[test]
    fn modified_stays_amber_or_takes_gitturtles_own() {
        let mut outside = Vec::new();
        for (name, colors) in fixtures() {
            let palette = fit(&colors);
            assert!(
                MODIFIED_HUES.contains(&oklch(palette.modified).1),
                "{name}'s modified {:06x}",
                palette.modified
            );
            let (chroma, hue) = oklch(colors.yellow);
            if chroma >= DIFF_MIN_CHROMA && !MODIFIED_HUES.contains(&hue) {
                let gitturtles = own(colors.mode).modified;
                assert!(
                    hue_distance(palette.modified, gitturtles)
                        < hue_distance(palette.modified, colors.yellow),
                    "{name} takes GitTurtle's modified"
                );
                outside.push(name);
            }
        }
        assert_eq!(
            outside,
            ["hackerman", "lumon", "lupine", "matte-black", "osaka-jade"]
        );
        let tokyo = bundled("tokyo-night");
        let palette = fit(&Colors {
            yellow: tokyo.blue,
            ..tokyo
        });
        assert!(apart(tokyo.blue, &[palette.added, palette.removed]));
        let gitturtles = fit(&Colors {
            yellow: own(Mode::Dark).modified,
            ..tokyo
        });
        assert_eq!(palette.modified, gitturtles.modified);
        assert_eq!(palette.readability_issues(), Vec::new());
    }

    /// A small xorshift generator, so the property test needs no new dependency and every run
    /// sees the same inputs.
    struct Inputs(u64);

    impl Inputs {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn color(&mut self) -> u32 {
            (self.next() & 0xff_ffff) as u32
        }

        fn optional(&mut self) -> Option<u32> {
            (!self.next().is_multiple_of(4)).then(|| self.color())
        }
    }

    /// 600 arbitrary themes, a fifth of them with foreground equal to background and a fifth
    /// with red and green one step apart, in every mode: each maps to a palette with no
    /// readability finding, whose lightness matches its mode, its fit's four statuses stand
    /// pairwise apart or are GitTurtle's own, and its fit's hover stands its step off subtle
    /// and the panel. The fit itself (before the built-in fallback) must carry at least 97%
    /// of them. Subtle's floor holds only where it can, and the count of themes whose subtle
    /// stands under it is pinned: of the 600, 500 never take the floor off the canvas (their
    /// canvas sits at the lane limit, or the step would leave the selected row no room), 22
    /// lose it where the lane limit holds hover under its step off subtle, and 2 where the
    /// pressed step falls below its rule; off the panel, 59 never take it and 7 lose it to the
    /// pressed step's rule. Measured when the counts were set; the pressed step had taken it
    /// from 8 and 19 themes while it also gave way for the margin and the order.
    #[test]
    fn arbitrary_themes_always_map_to_a_readable_palette() {
        let mut inputs = Inputs(0x9e37_79b9_7f4a_7c15);
        let total = 600;
        let mut fitted = 0;
        let mut under_the_floor = [0; 2];
        for case in 0..total {
            let mut colors = Colors {
                mode: if inputs.next().is_multiple_of(2) {
                    Mode::Light
                } else {
                    Mode::Dark
                },
                background: inputs.color(),
                foreground: inputs.color(),
                accent: inputs.color(),
                red: inputs.color(),
                green: inputs.color(),
                yellow: inputs.color(),
                blue: inputs.color(),
                magenta: inputs.color(),
                selection: inputs.optional(),
                muted: inputs.optional(),
                dark_background: inputs.optional(),
                lighter_background: inputs.optional(),
                dark_foreground: inputs.optional(),
                orange: inputs.optional(),
            };
            match case % 5 {
                0 => colors.foreground = colors.background,
                1 => colors.green = colors.red ^ 0x010101,
                _ => {}
            }
            let mapped = map(&colors);
            assert_eq!(
                mapped.palette.readability_issues(),
                Vec::new(),
                "case {case}: {colors:x?}"
            );
            assert_eq!(mapped.is_light, colors.mode == Mode::Light, "case {case}");
            assert_eq!(mapped.palette.is_light(), mapped.is_light, "case {case}");
            let fit = fit(&colors);
            assert!(
                stand_apart(statuses(&fit)) || statuses(&fit) == gitturtles_statuses(&colors),
                "case {case}: {:06x?}",
                statuses(&fit)
            );
            let judge = Judge {
                dark: colors.mode == Mode::Dark,
            };
            assert!(
                judge.clears(fit.hover, &[(&[fit.subtle, fit.panel], HOVER_STEP)], 0.),
                "case {case}: hover {:06x} on subtle {:06x} or the panel {:06x}",
                fit.hover,
                fit.subtle,
                fit.panel
            );
            for (count, reference) in under_the_floor.iter_mut().zip([fit.canvas, fit.panel]) {
                if !judge.clears(fit.subtle, &[(&[reference], SUBTLE_FLOOR)], 0.) {
                    *count += 1;
                }
            }
            if fit.readability_issues().is_empty() {
                fitted += 1;
            }
        }
        assert_eq!(under_the_floor, [524, 66], "off the canvas, off the panel");
        assert!(
            fitted * 100 >= total * 97,
            "the fit carried {fitted} of {total} themes"
        );
    }

    fn tokyo_night_source() -> String {
        let (_, bytes, _) = BUNDLED[19];
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// Each required color is refused by name when neither it nor an alias Omarchy tries has a
    /// value, and by the key that gave it when that value is not `#rrggbb`. A value Omarchy
    /// would skip for its characters, or an empty one, leaves the aliases to decide.
    #[test]
    fn a_missing_or_malformed_required_color_is_refused_by_name() {
        let source = tokyo_night_source();
        for (key, names) in REQUIRED {
            let article = if key == "accent" { "an" } else { "a" };
            let missing = match names {
                [_] => format!("colors.toml is missing {article} {key} color"),
                _ => format!(
                    "colors.toml is missing {article} {key} color ({} or {})",
                    names[..names.len() - 1].join(", "),
                    names[names.len() - 1]
                ),
            };
            let edit = |value: Option<&str>| {
                source
                    .lines()
                    .filter_map(|line| match value {
                        _ if !line.starts_with(&format!("{key} ")) => Some(line.to_owned()),
                        Some(value) => Some(format!("{key} = \"{value}\"")),
                        None => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            assert_eq!(
                parse(edit(None).as_bytes(), false),
                Err(missing.clone()),
                "{key}"
            );
            for skipped in ["", "#1a1b26;", "#1a1b26\\"] {
                assert_eq!(
                    parse(edit(Some(skipped)).as_bytes(), false),
                    Err(missing.clone()),
                    "{key} = {skipped}"
                );
            }
            for malformed in ["#1a1b2", "#1a1b2g", "1a1b26", "#1a1b26ff", "rgb(1, 2, 3)"] {
                assert_eq!(
                    parse(edit(Some(malformed)).as_bytes(), false),
                    Err(format!("colors.toml's {key} is not a #rrggbb color")),
                    "{key} = {malformed}"
                );
            }
        }
    }

    /// A theme with only `alacritty.toml` gets the `colors.toml` that
    /// `omarchy-theme-colors-from-alacritty` writes: accent, selection, background, foreground
    /// and `color0`…`color15`, with no named colors and no mode. Built here by hand in that
    /// shape (the script itself is never run), a dark and a light one resolve as Omarchy
    /// resolves them and fit without a single readability finding.
    #[test]
    fn a_theme_generated_from_alacritty_resolves_through_omarchys_aliases() {
        for (bytes, mode) in [
            (
                &include_bytes!("../../../tests/fixtures/omarchy/alacritty-dark.toml")[..],
                Mode::Dark,
            ),
            (
                &include_bytes!("../../../tests/fixtures/omarchy/alacritty-light.toml")[..],
                Mode::Light,
            ),
        ] {
            let text = std::str::from_utf8(bytes).unwrap();
            let file = |key: &str| {
                text.lines()
                    .find_map(|line| line.strip_prefix(&format!("{key} = \"#")))
                    .map(|value| u32::from_str_radix(&value[..6], 16).unwrap())
                    .unwrap_or_else(|| panic!("the generated file has {key}"))
            };
            let colors = parse(bytes, false).unwrap();
            assert_eq!(
                colors,
                Colors {
                    // By the background's channel sum, as neither a mode nor light.mode says.
                    mode,
                    background: file("background"),
                    foreground: file("foreground"),
                    accent: file("accent"),
                    red: file("color1"),
                    green: file("color2"),
                    yellow: file("color3"),
                    blue: file("color4"),
                    magenta: file("color5"),
                    selection: Some(file("selection")),
                    muted: Some(file("color8")),
                    dark_background: Some(mix(file("background"), BLACK, 0.25)),
                    lighter_background: Some(file("color0")),
                    dark_foreground: Some(file("color8")),
                    orange: Some(file("color3")),
                }
            );
            assert_eq!(fit(&colors).readability_issues(), Vec::new(), "{mode:?}");
            assert_eq!(map(&colors).is_light, mode == Mode::Light);
        }
    }

    /// The rest of `omarchy-theme-color`'s cascade for the keys GitTurtle reads, and its mode
    /// precedence: `mode`, then `theme_type`, then a `light.mode` file, then the background's
    /// channel sum above 382, with only the exact value "light" meaning light.
    #[test]
    fn omarchys_aliases_and_mode_precedence_decide_as_omarchy_does() {
        let theme = |lines: &str, marker: bool| {
            parse(format!("accent = \"#7aa2f7\"\n{lines}").as_bytes(), marker)
        };
        let legacy = theme(
            "bg = \"#101010\"\nfg = \"#e0e0e0\"\ndark_bg = \"#080808\"\n\
             lighter_bg = \"#202020\"\ndark_fg = \"#808080\"\ncolor1 = \"#e05050\"\n\
             color2 = \"#50e050\"\ncolor3 = \"#e0e050\"\ncolor4 = \"#5050e0\"\n\
             purple = \"#b050e0\"\nselection_background = \"#303030\"\n",
            false,
        )
        .unwrap();
        assert_eq!(
            (legacy.background, legacy.foreground, legacy.magenta),
            (0x101010, 0xe0e0e0, 0xb050e0)
        );
        assert_eq!(legacy.dark_background, Some(0x080808));
        assert_eq!(legacy.lighter_background, Some(0x202020));
        assert_eq!(legacy.dark_foreground, Some(0x808080));
        assert_eq!(
            legacy.muted,
            Some(0x808080),
            "muted falls back to dark_foreground"
        );
        assert_eq!(legacy.selection, Some(0x303030));
        assert_eq!(legacy.orange, Some(0xe0e050), "orange falls back to yellow");

        // Canonical names win over their aliases; color5 over purple.
        let both = theme(
            "background = \"#111111\"\nbg = \"#222222\"\ncolor0 = \"#333333\"\n\
             foreground = \"#dddddd\"\nred = \"#e05050\"\ncolor1 = \"#ff0000\"\n\
             green = \"#50e050\"\nyellow = \"#e0e050\"\nblue = \"#5050e0\"\n\
             color5 = \"#c050c0\"\npurple = \"#b050e0\"\ncolor8 = \"#707070\"\n",
            false,
        )
        .unwrap();
        assert_eq!(
            (both.background, both.red, both.magenta),
            (0x111111, 0xe05050, 0xc050c0)
        );
        // color0 is the background by then, and color8 serves selection, muted and dim text.
        assert_eq!(both.lighter_background, Some(0x111111));
        assert_eq!(both.selection, Some(0x707070));
        assert_eq!(both.muted, Some(0x707070));
        assert_eq!(both.dark_foreground, Some(0x707070));
        assert_eq!(
            both.dark_background,
            Some(0x0d0d0d),
            "25% toward black, rounded"
        );

        // A value Omarchy keeps blocks its aliases even when it is not a color; one it skips
        // for its characters, or an empty one, does not.
        let base = "background = \"#111111\"\nforeground = \"#dddddd\"\ngreen = \"#50e050\"\n\
                    yellow = \"#e0e050\"\nblue = \"#5050e0\"\nmagenta = \"#c050c0\"\n\
                    color1 = \"#e05050\"\n";
        assert_eq!(
            theme(&format!("{base}red = \"rgb(1, 2, 3)\"\n"), false),
            Err("colors.toml's red is not a #rrggbb color".into())
        );
        for skipped in ["red = \"#ff0000;\"", "red = \"\"", "# red = \"#ff0000\""] {
            assert_eq!(
                theme(&format!("{base}{skipped}\n"), false).unwrap().red,
                0xe05050
            );
        }
        // An unquoted value and a quoted key are read as Omarchy reads them.
        let loose = theme(
            &format!("{base}red = #ff0000\n\"muted\" = '#404040'\n"),
            false,
        )
        .unwrap();
        assert_eq!((loose.red, loose.muted), (0xff0000, Some(0x404040)));

        let mode =
            |lines: &str, marker: bool| theme(&format!("{base}{lines}"), marker).unwrap().mode;
        assert_eq!(mode("mode = \"light\"\n", false), Mode::Light);
        assert_eq!(
            mode("mode = \"Light\"\n", true),
            Mode::Dark,
            "only \"light\" is light"
        );
        assert_eq!(
            mode("mode = \"dark\"\ntheme_type = \"light\"\n", true),
            Mode::Dark
        );
        assert_eq!(mode("theme_type = \"light\"\n", false), Mode::Light);
        assert_eq!(
            mode("mode = \"\"\n", true),
            Mode::Light,
            "light.mode, when nothing names one"
        );
        assert_eq!(mode("", false), Mode::Dark);
        for (background, expected) in [("#808080", Mode::Light), ("#7f7f7f", Mode::Dark)] {
            let lines = base.replace("#111111", background);
            assert_eq!(theme(&lines, false).unwrap().mode, expected, "{background}");
        }
    }

    /// Oversized, non-UTF-8 and binary input and a file whose colors all sit in a table are
    /// refused with a reason; comments, blank lines, either case, missing spaces, trailing
    /// comments and values that are not colors are accepted or ignored.
    #[test]
    fn the_parser_reads_only_flat_theme_colors() {
        let source = tokyo_night_source();
        let mut oversized = source.clone().into_bytes();
        oversized.resize(MAX_COLORS_BYTES + 1, b'\n');
        assert_eq!(
            parse(&oversized, false),
            Err("colors.toml is larger than 16 KiB".into())
        );
        let mut at_limit = source.clone().into_bytes();
        at_limit.resize(MAX_COLORS_BYTES, b'\n');
        assert!(parse(&at_limit, false).is_ok());

        let mut invalid = source.clone().into_bytes();
        invalid.extend_from_slice(b"\nname = \"\xff\xfe\"\n");
        assert_eq!(
            parse(&invalid, false),
            Err("colors.toml is not UTF-8 text".into())
        );
        let binary = [
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0xc3,
        ];
        assert_eq!(
            parse(&binary, false),
            Err("colors.toml is not UTF-8 text".into())
        );

        let table = format!("[colors]\n{source}");
        assert_eq!(
            parse(table.as_bytes(), false),
            Err("colors.toml is missing a background color (background, bg or color0)".into())
        );
        // A table after the theme's own keys does not hide them.
        let trailing = format!("{source}\n[extra]\naccent = \"#000000\"\n");
        assert_eq!(parse(trailing.as_bytes(), false).unwrap().accent, 0x7aa2f7);

        let written = "\u{feff}# A theme\n\nmode = \"Dark\"\nbackground=\"#1A1B26\"\n\
            foreground = '#a9b1d6'\naccent = \"rgba(26a269ee) rgba(2ec27eee) 45deg\"\n\
            accent = \"#7aa2f7\" # the accent\nred = \"#f7768e\"\ngreen = \"#9ece6a\"\n\
            yellow = \"#e0af68\"\nblue = \"#7aa2f7\"\nmagenta = \"#ad8ee6\"\n\
            orange = \"#eb927\"\nhyprland_active_border = \"rgba(26a269ee) rgba(2ec27eee) 45deg\"\n\
            active_tab_background = \"#6fb8e3\"\nnot a key\ncyan = \"#449dab\" trailing\n";
        let colors = parse(written.as_bytes(), false).unwrap();
        assert_eq!(colors.mode, Mode::Dark);
        assert_eq!(colors.background, 0x1a1b26);
        assert_eq!(colors.foreground, 0xa9b1d6);
        assert_eq!(colors.accent, 0x7aa2f7);
        assert_eq!(
            colors.orange, None,
            "a malformed optional color is not used"
        );
        assert_eq!(colors.selection, Some(colors.background));
        // Without a usable orange the warning takes yellow.
        let palette = map(&colors).palette;
        assert_eq!(palette.warning, colors.yellow);
        assert_eq!(palette.readability_issues(), Vec::new());

        // A mode that is not "light" is dark, whatever the background.
        let edited = source.replace("mode = \"dark\"", "mode = \"dim\"");
        assert_eq!(parse(edited.as_bytes(), false).unwrap().mode, Mode::Dark);
    }
}
