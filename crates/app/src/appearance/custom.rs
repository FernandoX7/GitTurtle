//! Semantic palette tokens and the readability rules every palette is judged by.
//!
//! `Palette::readability_issues` is the single rule set: built-in palettes must return no issues
//! (asserted in tests), and custom palettes will show the same issues as editor warnings. The rows
//! and thresholds are the "Readability rules" table in `docs/development/themes/spec.md`.

use super::Palette;
use std::fmt;

/// One of the 21 semantic palette tokens, in the specification's group order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Canvas,
    Panel,
    Subtle,
    Hover,
    Border,
    Selected,
    Text,
    Muted,
    LineNumber,
    Accent,
    AccentForeground,
    AccentHover,
    AccentActive,
    Added,
    Removed,
    Modified,
    Renamed,
    Warning,
    AddedBackground,
    RemovedBackground,
    Hunk,
}

impl TokenKind {
    pub const ALL: [Self; 21] = [
        Self::Canvas,
        Self::Panel,
        Self::Subtle,
        Self::Hover,
        Self::Border,
        Self::Selected,
        Self::Text,
        Self::Muted,
        Self::LineNumber,
        Self::Accent,
        Self::AccentForeground,
        Self::AccentHover,
        Self::AccentActive,
        Self::Added,
        Self::Removed,
        Self::Modified,
        Self::Renamed,
        Self::Warning,
        Self::AddedBackground,
        Self::RemovedBackground,
        Self::Hunk,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Canvas => "Canvas",
            Self::Panel => "Panel",
            Self::Subtle => "Subtle surface",
            Self::Hover => "Hover",
            Self::Border => "Border",
            Self::Selected => "Selected",
            Self::Text => "Text",
            Self::Muted => "Muted text",
            Self::LineNumber => "Line numbers",
            Self::Accent => "Accent",
            Self::AccentForeground => "Accent label",
            Self::AccentHover => "Accent hover",
            Self::AccentActive => "Accent pressed",
            Self::Added => "Added",
            Self::Removed => "Removed",
            Self::Modified => "Modified",
            Self::Renamed => "Renamed",
            Self::Warning => "Warning",
            Self::AddedBackground => "Added background",
            Self::RemovedBackground => "Removed background",
            Self::Hunk => "Hunk and links",
        }
    }
}

impl Palette {
    pub fn token(self, kind: TokenKind) -> u32 {
        match kind {
            TokenKind::Canvas => self.canvas,
            TokenKind::Panel => self.panel,
            TokenKind::Subtle => self.subtle,
            TokenKind::Hover => self.hover,
            TokenKind::Border => self.border,
            TokenKind::Selected => self.selected,
            TokenKind::Text => self.text,
            TokenKind::Muted => self.muted,
            TokenKind::LineNumber => self.line_number,
            TokenKind::Accent => self.accent,
            TokenKind::AccentForeground => self.accent_foreground,
            TokenKind::AccentHover => self.accent_hover,
            TokenKind::AccentActive => self.accent_active,
            TokenKind::Added => self.added,
            TokenKind::Removed => self.removed,
            TokenKind::Modified => self.modified,
            TokenKind::Renamed => self.renamed,
            TokenKind::Warning => self.warning,
            TokenKind::AddedBackground => self.added_background,
            TokenKind::RemovedBackground => self.removed_background,
            TokenKind::Hunk => self.hunk,
        }
    }
}

/// WCAG relative luminance of an `0xrrggbb` color.
pub fn luminance(rgb: u32) -> f64 {
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

/// WCAG contrast ratio between two `0xrrggbb` colors, from 1 to 21.
pub fn contrast(a: u32, b: u32) -> f64 {
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The color being judged: a palette token or one of the palette's graph lane colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadabilityForeground {
    Token(TokenKind),
    /// Index into the six-color lane set chosen by `Palette::is_light`.
    Lane(usize),
}

/// The surface it is judged against: a palette token or the selected-row hover blend
/// (`Palette::row_hover(true)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadabilityBackground {
    Token(TokenKind),
    SelectedRowHover,
}

/// One pair that falls below its rule's minimum contrast ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReadabilityIssue {
    pub foreground: ReadabilityForeground,
    pub background: ReadabilityBackground,
    pub ratio: f64,
    pub minimum: f64,
}

impl fmt::Display for ReadabilityForeground {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Token(kind) => f.write_str(kind.label()),
            Self::Lane(index) => write!(f, "Graph lane {}", index + 1),
        }
    }
}

impl fmt::Display for ReadabilityBackground {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Token(kind) => f.write_str(kind.label()),
            Self::SelectedRowHover => f.write_str("Selected row hover"),
        }
    }
}

impl fmt::Display for ReadabilityIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Round down so a failing pair never displays as reaching its minimum.
        let ratio = (self.ratio * 10.).floor() / 10.;
        write!(
            f,
            "{} on {} {ratio:.1}:1, needs {}:1",
            self.foreground, self.background, self.minimum
        )
    }
}

const TEXT: f64 = 4.5;
const GRAPHIC: f64 = 3.0;
const SELECTED_SURFACE: f64 = 1.15;
const HOVER_SURFACE: f64 = 1.08;
const BORDER_SURFACE: f64 = 1.3;

impl Palette {
    /// Light palettes paint dark foregrounds on a bright canvas. This is the rule the graph has
    /// always used to pick its lane set; it stays integer-only because paint callbacks call it.
    pub fn is_light(self) -> bool {
        let brightness = |color: u32| {
            ((color >> 16) & 255) * 2126 + ((color >> 8) & 255) * 7152 + (color & 255) * 722
        };
        brightness(self.canvas) > brightness(self.text)
    }

    /// Every pair in the readability rules that falls below its minimum, grouped by rule.
    /// Empty for every built-in palette; advisory for custom palettes.
    pub fn readability_issues(self) -> Vec<ReadabilityIssue> {
        use ReadabilityBackground::SelectedRowHover;
        use TokenKind::*;

        const SURFACES: [ReadabilityBackground; 6] = [
            ReadabilityBackground::Token(Canvas),
            ReadabilityBackground::Token(Panel),
            ReadabilityBackground::Token(Subtle),
            ReadabilityBackground::Token(Hover),
            ReadabilityBackground::Token(Selected),
            SelectedRowHover,
        ];
        let token = ReadabilityBackground::Token;
        let background_color = |background: ReadabilityBackground| match background {
            ReadabilityBackground::Token(kind) => self.token(kind),
            SelectedRowHover => self.row_hover(true),
        };

        let mut issues = Vec::new();
        let mut check = |foreground: ReadabilityForeground,
                         color: u32,
                         background: ReadabilityBackground,
                         minimum: f64| {
            let ratio = contrast(color, background_color(background));
            if ratio < minimum {
                issues.push(ReadabilityIssue {
                    foreground,
                    background,
                    ratio,
                    minimum,
                });
            }
        };
        let mut rule =
            |foregrounds: &[TokenKind], backgrounds: &[ReadabilityBackground], minimum: f64| {
                for &foreground in foregrounds {
                    for &background in backgrounds {
                        check(
                            ReadabilityForeground::Token(foreground),
                            self.token(foreground),
                            background,
                            minimum,
                        );
                    }
                }
            };

        let diff_backgrounds = [token(AddedBackground), token(RemovedBackground)];
        // Body and secondary text.
        rule(&[Text, Muted], &SURFACES, TEXT);
        rule(&[Text, Muted], &diff_backgrounds, TEXT);
        // Status icons.
        rule(&[Added, Removed, Modified, Renamed], &SURFACES, GRAPHIC);
        // Diff content.
        rule(&[Added], &[token(AddedBackground)], TEXT);
        rule(&[Removed], &[token(RemovedBackground)], TEXT);
        // Primary button label.
        rule(
            &[AccentForeground],
            &[token(Accent), token(AccentHover), token(AccentActive)],
            TEXT,
        );
        // Focus ring, caret and list active border.
        rule(&[Accent], &SURFACES, GRAPHIC);
        // Selection, hover and dividers remain distinguishable surfaces.
        rule(&[Selected], &[token(Panel)], SELECTED_SURFACE);
        rule(&[Hover], &[token(Panel)], HOVER_SURFACE);
        rule(&[Border], &[token(Panel)], BORDER_SURFACE);
        // Gutter and inspector coordinates.
        rule(&[LineNumber], &[token(Canvas), token(Panel)], TEXT);
        // Links, info and hunk headers.
        rule(&[Hunk], &[token(Canvas), token(Panel), token(Subtle)], TEXT);
        rule(
            &[Hunk],
            &[token(Hover), token(Selected), SelectedRowHover],
            GRAPHIC,
        );
        // `configure` paints canvas as the success/danger/warning/info foreground.
        rule(
            &[Canvas],
            &[token(Added), token(Removed), token(Warning), token(Hunk)],
            TEXT,
        );
        // Status icons inside diff tiles.
        rule(&[Modified, Renamed], &diff_backgrounds, GRAPHIC);
        // Conflict and warning glyphs.
        rule(&[Warning], &SURFACES, GRAPHIC);

        // Graph lanes stay identifiable on every row state.
        for (index, color) in crate::graph::palette_colors(self).into_iter().enumerate() {
            for background in [token(Canvas), token(Panel), token(Selected), token(Hover)] {
                check(
                    ReadabilityForeground::Lane(index),
                    color,
                    background,
                    GRAPHIC,
                );
            }
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::ThemeChoice;
    use ReadabilityBackground::SelectedRowHover;
    use TokenKind::*;

    type Pair = (ReadabilityForeground, ReadabilityBackground, f64);

    fn expected(
        palette: Palette,
        foreground: ReadabilityForeground,
        background: ReadabilityBackground,
        minimum: f64,
    ) -> ReadabilityIssue {
        let foreground_color = match foreground {
            ReadabilityForeground::Token(kind) => palette.token(kind),
            ReadabilityForeground::Lane(index) => crate::graph::palette_colors(palette)[index],
        };
        let background_color = match background {
            ReadabilityBackground::Token(kind) => palette.token(kind),
            SelectedRowHover => palette.row_hover(true),
        };
        let ratio = contrast(foreground_color, background_color);
        assert!(ratio < minimum, "{foreground} on {background} must fail");
        ReadabilityIssue {
            foreground,
            background,
            ratio,
            minimum,
        }
    }

    /// Each case changes a built-in palette so that exactly one row of the rules table fails,
    /// and lists every pair of that row that should be reported.
    #[test]
    fn each_rule_names_foreground_background_ratio_and_minimum() {
        use ReadabilityBackground::Token as On;
        use ReadabilityForeground::{Lane, Token as Fg};

        let midnight = ThemeChoice::Midnight.palette();
        let porcelain = ThemeChoice::Porcelain.palette();
        let daylight = ThemeChoice::Daylight.palette();
        let cases: Vec<(&str, Palette, Vec<Pair>)> = vec![
            (
                "body and secondary text",
                Palette {
                    muted: midnight.line_number,
                    ..midnight
                },
                vec![
                    (Fg(Muted), On(Selected), 4.5),
                    (Fg(Muted), SelectedRowHover, 4.5),
                ],
            ),
            (
                "status icons",
                Palette {
                    modified: 0xe623b1,
                    ..midnight
                },
                vec![
                    (Fg(Modified), On(Selected), 3.),
                    (Fg(Modified), SelectedRowHover, 3.),
                ],
            ),
            (
                "diff content",
                Palette {
                    added: 0x829868,
                    ..midnight
                },
                vec![(Fg(Added), On(AddedBackground), 4.5)],
            ),
            (
                "primary button label",
                Palette {
                    accent_foreground: midnight.accent,
                    ..midnight
                },
                vec![
                    (Fg(AccentForeground), On(Accent), 4.5),
                    (Fg(AccentForeground), On(AccentHover), 4.5),
                    (Fg(AccentForeground), On(AccentActive), 4.5),
                ],
            ),
            (
                "focus ring",
                Palette {
                    accent: 0x7f7f7f,
                    accent_hover: 0x7f7f7f,
                    accent_active: 0x7f7f7f,
                    accent_foreground: 0x000000,
                    ..porcelain
                },
                vec![(Fg(Accent), SelectedRowHover, 3.)],
            ),
            (
                "graph lanes",
                Palette {
                    muted: porcelain.text,
                    hover: 0xafafaf,
                    ..porcelain
                },
                [0, 1, 2, 3, 5]
                    .into_iter()
                    .map(|lane| (Lane(lane), On(Hover), 3.))
                    .collect(),
            ),
            (
                "selected surface",
                Palette {
                    selected: midnight.panel,
                    ..midnight
                },
                vec![(Fg(Selected), On(Panel), 1.15)],
            ),
            (
                "hover surface",
                Palette {
                    hover: midnight.panel,
                    ..midnight
                },
                vec![(Fg(Hover), On(Panel), 1.08)],
            ),
            (
                "divider",
                Palette {
                    border: midnight.panel,
                    ..midnight
                },
                vec![(Fg(Border), On(Panel), 1.3)],
            ),
            (
                "gutter and inspector coordinates",
                Palette {
                    line_number: midnight.hover,
                    ..midnight
                },
                vec![
                    (Fg(LineNumber), On(Canvas), 4.5),
                    (Fg(LineNumber), On(Panel), 4.5),
                ],
            ),
            (
                "links, info and hunk headers",
                Palette {
                    hunk: 0xe623b1,
                    ..midnight
                },
                vec![
                    (Fg(Hunk), On(Panel), 4.5),
                    (Fg(Hunk), On(Subtle), 4.5),
                    (Fg(Hunk), On(Selected), 3.),
                    (Fg(Hunk), SelectedRowHover, 3.),
                ],
            ),
            (
                "status control labels",
                Palette {
                    warning: 0x6b7f32,
                    ..daylight
                },
                vec![(Fg(Canvas), On(Warning), 4.5)],
            ),
            (
                "status icons inside diff tiles",
                Palette {
                    added_background: 0xd8d8d8,
                    modified: 0x7b7b7b,
                    ..porcelain
                },
                vec![(Fg(Modified), On(AddedBackground), 3.)],
            ),
            (
                "warning glyphs",
                Palette {
                    warning: 0xe623b1,
                    ..midnight
                },
                vec![
                    (Fg(Warning), On(Selected), 3.),
                    (Fg(Warning), SelectedRowHover, 3.),
                ],
            ),
        ];
        for (rule, palette, pairs) in cases {
            let expected: Vec<_> = pairs
                .into_iter()
                .map(|(foreground, background, minimum)| {
                    expected(palette, foreground, background, minimum)
                })
                .collect();
            assert_eq!(palette.readability_issues(), expected, "{rule}");
        }
    }

    #[test]
    fn issues_describe_both_colors_the_ratio_and_the_minimum() {
        let palette = Palette {
            muted: ThemeChoice::Midnight.palette().line_number,
            ..ThemeChoice::Midnight.palette()
        };
        let issues = palette.readability_issues();
        assert_eq!(
            issues[0].to_string(),
            "Muted text on Selected 4.2:1, needs 4.5:1"
        );
        let lane = ReadabilityIssue {
            foreground: ReadabilityForeground::Lane(2),
            background: SelectedRowHover,
            ratio: 2.99,
            minimum: 3.,
        };
        // A failing ratio is never rounded up to its minimum.
        assert_eq!(
            lane.to_string(),
            "Graph lane 3 on Selected row hover 2.9:1, needs 3:1"
        );
    }

    #[test]
    fn tokens_cover_every_palette_field_once() {
        let palette = ThemeChoice::Midnight.palette();
        let mut seen = std::collections::HashSet::new();
        for kind in TokenKind::ALL {
            assert!(seen.insert(kind));
            let mut changed = palette;
            // Each token reads its own field: a unique value shows up only there.
            match kind {
                Canvas => changed.canvas = 0x123456,
                Panel => changed.panel = 0x123456,
                Subtle => changed.subtle = 0x123456,
                Hover => changed.hover = 0x123456,
                Border => changed.border = 0x123456,
                Selected => changed.selected = 0x123456,
                Text => changed.text = 0x123456,
                Muted => changed.muted = 0x123456,
                LineNumber => changed.line_number = 0x123456,
                Accent => changed.accent = 0x123456,
                AccentForeground => changed.accent_foreground = 0x123456,
                AccentHover => changed.accent_hover = 0x123456,
                AccentActive => changed.accent_active = 0x123456,
                Added => changed.added = 0x123456,
                Removed => changed.removed = 0x123456,
                Modified => changed.modified = 0x123456,
                Renamed => changed.renamed = 0x123456,
                Warning => changed.warning = 0x123456,
                AddedBackground => changed.added_background = 0x123456,
                RemovedBackground => changed.removed_background = 0x123456,
                Hunk => changed.hunk = 0x123456,
            }
            for other in TokenKind::ALL {
                assert_eq!(
                    changed.token(other) == 0x123456,
                    other == kind,
                    "{kind:?} / {other:?}"
                );
            }
        }
    }
}
