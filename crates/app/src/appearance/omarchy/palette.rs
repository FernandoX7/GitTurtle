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

use crate::appearance::custom::{contrast, luminance};
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
/// Diff tiles tint the canvas with this much of the status color.
const DIFF_TINT_DARK: f64 = 0.14;
const DIFF_TINT_LIGHT: f64 = 0.10;
/// Red and green count as distinguishable when both keep at least this OKLCH chroma after
/// fitting and their OKLCH hues are at least this many degrees apart.
pub(crate) const DIFF_MIN_CHROMA: f64 = 0.05;
pub(crate) const DIFF_MIN_HUE: f64 = 60.;

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
    // Lift a surface off the panel toward the foreground side by `ratio`.
    let lift = |color: u32, panel: u32, ratio: f64| {
        let steps = |candidate: u32, ratio: f64| judge.clears(candidate, &[(&[panel], ratio)], 0.);
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
    let subtle = settle(
        colors
            .lighter_background
            .unwrap_or_else(|| mix(canvas, colors.foreground, 0.06)),
    );
    let selected = lift(
        settle(
            colors
                .selection
                .unwrap_or_else(|| mix(canvas, colors.accent, 0.25)),
        ),
        panel,
        1.15,
    );
    let hover = lift(settle(mix(canvas, selected, 0.5)), panel, 1.08);
    let border = lift(
        mix(
            canvas,
            colors
                .muted
                .unwrap_or_else(|| mix(canvas, colors.foreground, 0.3)),
            0.7,
        ),
        panel,
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
    // The pressed button's layer stands apart from its hover on every surface: lift the
    // selected row further while it keeps the lanes, then bring hover back toward the panel
    // while it keeps its own step.
    let foreground_side = if judge.dark { 1. } else { 0. };
    for _ in 0..128 {
        if palette.pressed_step() >= PRESSED_TARGET {
            break;
        }
        let (h, s, l) = to_hsl(palette.selected);
        let lifted = from_hsl(h, s, l + (foreground_side - l) / 32.);
        if lifted != palette.selected && within(lifted, 1.) {
            palette.selected = lifted;
            (palette.accent, palette.accent_foreground) = accent_for(palette, palette.accent);
            continue;
        }
        let (h, s, l) = to_hsl(palette.hover);
        let lowered = from_hsl(h, s, l + (1. - foreground_side - l) / 32.);
        if lowered == palette.hover
            || !judge.clears(lowered, &[(&[panel], 1.08 + SURFACE_MARGIN)], 0.)
        {
            break;
        }
        palette.hover = lowered;
        (palette.accent, palette.accent_foreground) = accent_for(palette, palette.accent);
    }
    (palette.accent_hover, palette.accent_active) =
        accent_states(palette.accent, palette.accent_foreground);
    let (hover, selected) = (palette.hover, palette.selected);
    let row = palette.row_hover(true);
    let surfaces = [canvas, panel, subtle, hover, selected, row];

    // Status colors: icons on every surface, and a label in `canvas` on their fills.
    let on_canvas = [canvas];
    let status = |color: u32, extra: &[(&[u32], f64)]| {
        let mut rules: Vec<(&[u32], f64)> = vec![(&surfaces, 3.0), (&on_canvas, 4.5)];
        rules.extend_from_slice(extra);
        judge.tune(color, &rules, CONTRAST_MARGIN)
    };
    let red = status(colors.red, &[]);
    let green = status(colors.green, &[]);
    let (added, removed) = if distinguishable(red, green) {
        (green, red)
    } else {
        let own = if judge.dark {
            ThemeChoice::Midnight.palette()
        } else {
            ThemeChoice::Daylight.palette()
        };
        (status(own.added, &[]), status(own.removed, &[]))
    };
    let tint = if judge.dark {
        DIFF_TINT_DARK
    } else {
        DIFF_TINT_LIGHT
    };
    let added_background = settle(mix(canvas, added, tint));
    let removed_background = settle(mix(canvas, removed, tint));
    let tiles = [added_background, removed_background];
    palette.added_background = added_background;
    palette.removed_background = removed_background;
    palette.added = status(added, &[(&[added_background], 4.5)]);
    palette.removed = status(removed, &[(&[removed_background], 4.5)]);
    palette.modified = judge.tune(
        colors.yellow,
        &[(&surfaces, 3.0), (&tiles, 3.0)],
        CONTRAST_MARGIN,
    );
    palette.renamed = judge.tune(
        colors.magenta,
        &[(&surfaces, 3.0), (&tiles, 3.0)],
        CONTRAST_MARGIN,
    );
    palette.warning = status(colors.orange.unwrap_or(colors.yellow), &[(&[subtle], 4.5)]);
    palette.hunk = judge.tune(
        colors.blue,
        &[
            (&[canvas, panel, subtle], 4.5),
            (&[hover, selected, row], 3.0),
        ],
        CONTRAST_MARGIN,
    );

    // Text, then secondary text and line numbers lifted toward it.
    let text_rules: [(&[u32], f64); 2] = [(&surfaces, 4.5), (&tiles, 4.5)];
    let text = judge.tune(colors.foreground, &text_rules, CONTRAST_MARGIN);
    palette.text = text;
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

/// Whether a theme's fitted red and green are far enough apart to mark removed and added
/// lines: both keep `DIFF_MIN_CHROMA` of OKLCH chroma and their hues are `DIFF_MIN_HUE`
/// degrees apart.
pub(crate) fn distinguishable(red: u32, green: u32) -> bool {
    let (red_chroma, red_hue) = oklch(red);
    let (green_chroma, green_hue) = oklch(green);
    let apart = (red_hue - green_hue).abs() % 360.;
    red_chroma.min(green_chroma) >= DIFF_MIN_CHROMA && apart.min(360. - apart) >= DIFF_MIN_HUE
}

/// OKLCH chroma and hue in degrees of an `0xrrggbb` color.
fn oklch(color: u32) -> (f64, f64) {
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
    let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
    let b = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
    (a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.))
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
    /// background, selection, accent, status colors and foreground, as GitTurtle's own
    /// Tokyo Night keeps the same upstream hues; its dim foreground (2.9:1) is lifted toward
    /// the foreground only until secondary text reads on every surface.
    #[test]
    fn a_passing_theme_keeps_its_own_colors() {
        let colors = bundled("tokyo-night");
        let palette = map(&colors).palette;
        assert_eq!(palette.canvas, colors.background);
        assert_eq!(Some(palette.panel), colors.dark_background);
        assert_eq!(Some(palette.subtle), colors.lighter_background);
        assert_eq!(Some(palette.selected), colors.selection);
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
    /// readability finding, whose lightness matches its mode. The fit itself (before the
    /// built-in fallback) must carry at least 97% of them.
    #[test]
    fn arbitrary_themes_always_map_to_a_readable_palette() {
        let mut inputs = Inputs(0x9e37_79b9_7f4a_7c15);
        let total = 600;
        let mut fitted = 0;
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
            if fit(&colors).readability_issues().is_empty() {
                fitted += 1;
            }
        }
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
