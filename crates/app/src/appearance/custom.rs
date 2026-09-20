//! Semantic palette tokens, custom themes, and the readability rules every palette is judged by.
//!
//! `Palette::readability_issues` is the single rule set: built-in palettes must return no issues
//! (asserted in tests), and custom palettes will show the same issues as editor warnings. The rows
//! and thresholds are the "Readability rules" table in `docs/development/themes/spec.md`.
//!
//! A custom theme is a named palette derived from a built-in base. `ThemeSelection` names either
//! kind, `ResolvedTheme` carries the palette that is applied, and the export document is the
//! bounded JSON format from the specification's "Import and export" section.

use super::{Palette, ThemeChoice};
use crate::preferences::{MAX_CUSTOM_THEMES, MAX_PROJECT_NAME_BYTES};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

    /// One sentence naming where the token is painted.
    pub fn description(self) -> &'static str {
        match self {
            Self::Canvas => "The editor, history and window background.",
            Self::Panel => "Sidebars, headers, popovers and the inspector.",
            Self::Subtle => "Secondary buttons, tab bars and alternating rows.",
            Self::Hover => "Rows and controls under the pointer.",
            Self::Border => "Dividers, input outlines and scrollbar thumbs.",
            Self::Selected => "The selected row and pressed controls.",
            Self::Text => "Body text and primary labels.",
            Self::Muted => "Secondary labels, metadata and inactive tabs.",
            Self::LineNumber => "Editor gutters and inspector coordinates.",
            Self::Accent => "Primary buttons, focus rings, the caret and active list borders.",
            Self::AccentForeground => "Labels on primary buttons.",
            Self::AccentHover => "Primary buttons under the pointer.",
            Self::AccentActive => "Primary buttons while pressed.",
            Self::Added => "Added files, lines and success messages.",
            Self::Removed => "Removed files, lines and destructive actions.",
            Self::Modified => "Modified files.",
            Self::Renamed => "Renamed and copied files.",
            Self::Warning => "Conflicts and warning messages.",
            Self::AddedBackground => "The background of added diff lines.",
            Self::RemovedBackground => "The background of removed diff lines.",
            Self::Hunk => "Hunk headers, links and informational messages.",
        }
    }

    pub fn group(self) -> TokenGroup {
        match self {
            Self::Canvas
            | Self::Panel
            | Self::Subtle
            | Self::Hover
            | Self::Border
            | Self::Selected => TokenGroup::Surfaces,
            Self::Text | Self::Muted | Self::LineNumber => TokenGroup::Text,
            Self::Accent | Self::AccentForeground | Self::AccentHover | Self::AccentActive => {
                TokenGroup::Accent
            }
            Self::Added | Self::Removed | Self::Modified | Self::Renamed | Self::Warning => {
                TokenGroup::Status
            }
            Self::AddedBackground | Self::RemovedBackground | Self::Hunk => TokenGroup::Diff,
        }
    }

    /// The snake_case key of this token in a theme document; the `Palette` field name.
    pub fn key(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::Panel => "panel",
            Self::Subtle => "subtle",
            Self::Hover => "hover",
            Self::Border => "border",
            Self::Selected => "selected",
            Self::Text => "text",
            Self::Muted => "muted",
            Self::LineNumber => "line_number",
            Self::Accent => "accent",
            Self::AccentForeground => "accent_foreground",
            Self::AccentHover => "accent_hover",
            Self::AccentActive => "accent_active",
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Modified => "modified",
            Self::Renamed => "renamed",
            Self::Warning => "warning",
            Self::AddedBackground => "added_background",
            Self::RemovedBackground => "removed_background",
            Self::Hunk => "hunk",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }
}

/// The editor's token sections, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenGroup {
    Surfaces,
    Text,
    Accent,
    Status,
    Diff,
}

impl TokenGroup {
    pub const ALL: [Self; 5] = [
        Self::Surfaces,
        Self::Text,
        Self::Accent,
        Self::Status,
        Self::Diff,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Surfaces => "Surfaces",
            Self::Text => "Text",
            Self::Accent => "Accent",
            Self::Status => "Status",
            Self::Diff => "Diff",
        }
    }
}

impl Palette {
    pub fn get(self, kind: TokenKind) -> u32 {
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

    pub fn set(&mut self, kind: TokenKind, color: u32) {
        let color = color & 0xff_ffff;
        let field = match kind {
            TokenKind::Canvas => &mut self.canvas,
            TokenKind::Panel => &mut self.panel,
            TokenKind::Subtle => &mut self.subtle,
            TokenKind::Hover => &mut self.hover,
            TokenKind::Border => &mut self.border,
            TokenKind::Selected => &mut self.selected,
            TokenKind::Text => &mut self.text,
            TokenKind::Muted => &mut self.muted,
            TokenKind::LineNumber => &mut self.line_number,
            TokenKind::Accent => &mut self.accent,
            TokenKind::AccentForeground => &mut self.accent_foreground,
            TokenKind::AccentHover => &mut self.accent_hover,
            TokenKind::AccentActive => &mut self.accent_active,
            TokenKind::Added => &mut self.added,
            TokenKind::Removed => &mut self.removed,
            TokenKind::Modified => &mut self.modified,
            TokenKind::Renamed => &mut self.renamed,
            TokenKind::Warning => &mut self.warning,
            TokenKind::AddedBackground => &mut self.added_background,
            TokenKind::RemovedBackground => &mut self.removed_background,
            TokenKind::Hunk => &mut self.hunk,
        };
        *field = color;
    }
}

/// `0xrrggbb` as lowercase `#rrggbb`.
pub fn format_hex(color: u32) -> String {
    format!("#{:06x}", color & 0xff_ffff)
}

/// Parse `#rrggbb` or `rrggbb` in either case. Anything else, including surrounding spaces,
/// shorthand and alpha, is refused.
pub fn parse_hex(value: &str) -> Option<u32> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
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
            ReadabilityBackground::Token(kind) => self.get(kind),
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
                            self.get(foreground),
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

/// Theme documents larger than this are refused before parsing.
pub const MAX_THEME_DOCUMENT_BYTES: usize = 64 * 1024;
pub const THEME_DOCUMENT_FORMAT: &str = "gitturtle-theme";
pub const THEME_DOCUMENT_VERSION: u64 = 1;

/// The longest run of document-derived text a refusal or notice may quote.
///
/// A theme document is supplied bytes: a key, a token value or a base name can
/// be as long as the 64 KiB the reader accepts. Quoting one unclamped would put
/// tens of thousands of characters in the Your themes card and push everything
/// below it off screen, so every fragment taken from a document is clamped here
/// rather than where it is rendered. 64 characters is several times the longest
/// legitimate fragment (`removed_background` is 18 characters, the longest base
/// key 16, a quoted token value 9).
pub const MAX_MESSAGE_FRAGMENT_CHARS: usize = 64;

/// One document-derived fragment, clamped for a message on a character
/// boundary and marked with an ellipsis when anything was dropped. Theme
/// names are bounded at 128 bytes, not 64 characters, so the import notice
/// clamps the two it interpolates through this as well: two full-length names
/// make that sentence about 316 characters, past the card's two lines.
pub fn fragment(text: &str) -> std::borrow::Cow<'_, str> {
    fragment_within(text, MAX_MESSAGE_FRAGMENT_CHARS)
}

/// `fragment` with an explicit allowance: `allowance` characters, then an
/// ellipsis when anything was dropped.
fn fragment_within(text: &str, allowance: usize) -> std::borrow::Cow<'_, str> {
    match text.char_indices().nth(allowance) {
        Some((end, _)) => std::borrow::Cow::Owned(format!("{}…", &text[..end])),
        None => std::borrow::Cow::Borrowed(text),
    }
}

/// The most characters an import notice may hold, so it fits the two lines
/// the Your themes card clamps it to.
///
/// Measured on the narrower of the two evidence windows: at 1440x900 the
/// settings column is 732 px, and a notice whose two 64-character names were
/// runs of "a" was cut at the end of its second line after 248 characters
/// (the long-name re-check recorded under "September 19 Theme export and
/// import" in `docs/validation.md`). 238 keeps ten characters under that, and is also the
/// smallest budget at which no notice of one or two clauses is clamped below
/// `MAX_MESSAGE_FRAGMENT_CHARS`: only the three-clause notice, a renamed
/// import whose base was also replaced, shortens its fragments further. The
/// budget is characters, not pixels, so a name of nothing but wide glyphs can
/// still run a little past it; the clamp on the card is the backstop for that.
pub const MAX_NOTICE_CHARS: usize = 238;

/// The fewest characters a notice fragment keeps, however many clauses share
/// the budget, so a quoted name is still recognisable.
pub const MIN_MESSAGE_FRAGMENT_CHARS: usize = 8;

/// The most characters the import notice quotes of an unknown base, below the
/// `MAX_MESSAGE_FRAGMENT_CHARS` its names keep.
///
/// A base key has no spaces, so the quoted base is one unbreakable token: when
/// it does not fit after "The base theme" on the first line it wraps whole,
/// and the second line then has to hold it and its fixed 78-character tail
/// ("” is not available in this version of GitTurtle; Midnight is used as
/// the base."). Measured at 1440x900 the second line of this notice holds
/// about 144 characters (the same re-check's unknown-base capture): a 64-character base
/// with its ellipsis and quotes is 64 + 2 + 78 = 144, exactly at the edge,
/// where the capture ends the line mid-word at "Midnight is used a"; 48 + 3 +
/// 78 = 129 leaves a margin. The longest legitimate base key is 16 characters
/// (`future_theme` is 12), so a real base is always quoted whole.
pub const MAX_BASE_FRAGMENT_CHARS: usize = 48;
const _: () = assert!(
    MIN_MESSAGE_FRAGMENT_CHARS <= MAX_BASE_FRAGMENT_CHARS
        && MAX_BASE_FRAGMENT_CHARS < MAX_MESSAGE_FRAGMENT_CHARS
);

/// The sentence the Your themes card shows after an import: the stored name,
/// then why it differs from the document's when it does, then which built-in
/// replaced an unknown base when one did. The quoted strings share what
/// `MAX_NOTICE_CHARS` leaves after the fixed text, so the notice fits its two
/// lines whichever clauses it carries and a base substitution is always told.
/// The share is dealt shortest first: a string within its share is quoted
/// whole and hands the rest on, and only a string past its share is cut, to
/// at most `MAX_MESSAGE_FRAGMENT_CHARS` for a name and
/// `MAX_BASE_FRAGMENT_CHARS` for the base, and never below
/// `MIN_MESSAGE_FRAGMENT_CHARS`, with an ellipsis in the reserved last place.
pub fn import_notice(
    name: &str,
    document_name: &str,
    unknown_base: Option<&str>,
    base: ThemeChoice,
) -> String {
    // Each clause: the text before the quoted string, the string, the text
    // after it, and the most characters the string may keep.
    let mut clauses: Vec<(&str, &str, String, usize)> =
        vec![("Imported “", name, "”.".into(), MAX_MESSAGE_FRAGMENT_CHARS)];
    if name != document_name {
        clauses.push((
            " A theme named “",
            document_name,
            "” already exists, so the imported one was renamed.".into(),
            MAX_MESSAGE_FRAGMENT_CHARS,
        ));
    }
    if let Some(unknown_base) = unknown_base {
        clauses.push((
            " The base theme “",
            unknown_base,
            format!(
                "” is not available in this version of GitTurtle; {} is used as the base.",
                base.label()
            ),
            MAX_BASE_FRAGMENT_CHARS,
        ));
    }
    let fixed: usize = clauses
        .iter()
        .map(|(before, _, after, _)| before.chars().count() + after.chars().count())
        .sum();

    let mut order: Vec<usize> = (0..clauses.len()).collect();
    order.sort_by_key(|&index| clauses[index].1.chars().count());
    let mut allowances = vec![0; clauses.len()];
    let mut remaining = MAX_NOTICE_CHARS.saturating_sub(fixed);
    for (dealt, &index) in order.iter().enumerate() {
        let share = remaining / (clauses.len() - dealt);
        let characters = clauses[index].1.chars().count();
        let most = clauses[index].3;
        allowances[index] = if characters <= share.min(most) {
            characters
        } else {
            // The ellipsis takes the share's last place.
            share
                .saturating_sub(1)
                .clamp(MIN_MESSAGE_FRAGMENT_CHARS, most)
        };
        remaining = remaining
            .saturating_sub(allowances[index] + usize::from(characters > allowances[index]));
    }
    clauses
        .iter()
        .zip(allowances)
        .map(|((before, quoted, after, _), allowance)| {
            format!("{before}{}{after}", fragment_within(quoted, allowance))
        })
        .collect()
}

/// A user-authored palette derived from a built-in base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomTheme {
    /// Unique within the store; the next id is the largest existing id plus one.
    pub id: u32,
    pub name: String,
    pub base: ThemeChoice,
    pub palette: Palette,
}

/// Which theme the user chose. A built-in serializes as its bare existing string (`"nord"`),
/// a custom theme as `{"custom": 7}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SelectionRepr", into = "SelectionRepr")]
pub enum ThemeSelection {
    BuiltIn(ThemeChoice),
    Custom(u32),
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomReference {
    custom: u32,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
enum SelectionRepr {
    Custom(CustomReference),
    BuiltIn(ThemeChoice),
}

impl From<SelectionRepr> for ThemeSelection {
    fn from(repr: SelectionRepr) -> Self {
        match repr {
            SelectionRepr::Custom(reference) => Self::Custom(reference.custom),
            SelectionRepr::BuiltIn(choice) => Self::BuiltIn(choice),
        }
    }
}

impl From<ThemeSelection> for SelectionRepr {
    fn from(selection: ThemeSelection) -> Self {
        match selection {
            ThemeSelection::Custom(custom) => Self::Custom(CustomReference { custom }),
            ThemeSelection::BuiltIn(choice) => Self::BuiltIn(choice),
        }
    }
}

impl Default for ThemeSelection {
    fn default() -> Self {
        Self::BuiltIn(ThemeChoice::default())
    }
}

impl From<ThemeChoice> for ThemeSelection {
    fn from(choice: ThemeChoice) -> Self {
        Self::BuiltIn(choice)
    }
}

impl ThemeSelection {
    /// The palette this selection names. A custom id missing from `customs` resolves to the
    /// default theme, which is then also the resolved selection.
    pub fn resolve(self, customs: &[CustomTheme]) -> ResolvedTheme {
        match self {
            Self::BuiltIn(choice) => ResolvedTheme::built_in(choice),
            Self::Custom(id) => customs.iter().find(|theme| theme.id == id).map_or_else(
                || ResolvedTheme::built_in(ThemeChoice::default()),
                ResolvedTheme::custom,
            ),
        }
    }
}

/// The theme that is actually applied, after follow-system rules and missing custom themes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedTheme {
    pub selection: ThemeSelection,
    pub palette: Palette,
    pub is_light: bool,
}

impl ResolvedTheme {
    pub fn built_in(choice: ThemeChoice) -> Self {
        Self {
            selection: ThemeSelection::BuiltIn(choice),
            palette: choice.palette(),
            is_light: choice.is_light(),
        }
    }

    pub fn custom(theme: &CustomTheme) -> Self {
        Self {
            selection: ThemeSelection::Custom(theme.id),
            palette: theme.palette,
            is_light: theme.is_light(),
        }
    }
}

/// The id for a new custom theme: one more than the largest existing id, or `None` when the
/// id space is exhausted.
pub fn next_custom_theme_id(customs: &[CustomTheme]) -> Option<u32> {
    customs
        .iter()
        .map(|theme| theme.id)
        .max()
        .map_or(Some(1), |largest| largest.checked_add(1))
}

/// The one-line, bounded name rules shared with project names (`validate_project_name`).
fn validate_name_text(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name for this theme.".into());
    }
    if name.len() > MAX_PROJECT_NAME_BYTES {
        return Err(format!(
            "Use a theme name of at most {MAX_PROJECT_NAME_BYTES} bytes."
        ));
    }
    if name
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
    {
        return Err("Use a theme name on a single line, without control characters.".into());
    }
    Ok(name)
}

/// Check a custom theme name: the project-name rules, never a built-in label, and unique
/// among `customs` ignoring case. `editing` is the id of the theme being renamed, which may
/// keep its own name.
pub fn validate_theme_name(
    name: &str,
    customs: &[CustomTheme],
    editing: Option<u32>,
) -> Result<(), String> {
    let name = validate_name_text(name)?;
    let folded = name.to_lowercase();
    if let Some(choice) = ThemeChoice::ALL
        .into_iter()
        .find(|choice| choice.label().to_lowercase() == folded)
    {
        return Err(format!(
            "“{}” is a built-in theme. Choose another name.",
            choice.label()
        ));
    }
    if customs
        .iter()
        .any(|theme| Some(theme.id) != editing && theme.name.trim().to_lowercase() == folded)
    {
        return Err(format!("Another theme is already named “{name}”."));
    }
    Ok(())
}

/// The name an imported theme takes in a store that already holds `customs`: its own when the
/// store accepts it, then "<name> (imported)", "<name> (imported) (2)", … as the specification's
/// "Import and export" section describes. A stem that would pass the name's byte limit with its
/// suffix is shortened on a character boundary first.
pub fn import_name(name: &str, customs: &[CustomTheme]) -> Result<String, String> {
    let name = validate_name_text(name)?;
    let candidate = |suffix: &str| {
        let budget = MAX_PROJECT_NAME_BYTES.saturating_sub(suffix.len());
        let mut end = budget.min(name.len());
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{suffix}", name[..end].trim_end())
    };
    std::iter::once(String::new())
        .chain((1..=MAX_CUSTOM_THEMES + 1).map(|attempt| {
            if attempt == 1 {
                " (imported)".to_owned()
            } else {
                format!(" (imported) ({attempt})")
            }
        }))
        .map(|suffix| candidate(&suffix))
        .find(|candidate| validate_theme_name(candidate, customs, None).is_ok())
        .ok_or_else(|| {
            format!("No free name is left for “{name}”. Rename or delete a theme and import again.")
        })
}

/// The storage key of a built-in theme, as in the preference store and theme documents.
fn built_in_key(choice: ThemeChoice) -> String {
    match serde_json::to_value(choice) {
        Ok(Value::String(key)) => key,
        _ => unreachable!("theme choices serialize as strings"),
    }
}

/// The built-in stored under `key`, or `None` for a key this build does not know. Unlike
/// `ThemeChoice`'s own deserializer, an unknown key is not read as Midnight.
pub fn built_in_from_key(key: &str) -> Option<ThemeChoice> {
    ThemeChoice::ALL
        .into_iter()
        .find(|choice| built_in_key(*choice) == key)
}

/// The base that stands in for one this build does not know, such as a built-in added by a
/// newer release: Braden for a light palette and Midnight for a dark one.
pub fn fallback_base(palette: Palette) -> ThemeChoice {
    if palette.is_light() {
        ThemeChoice::Daylight
    } else {
        ThemeChoice::Midnight
    }
}

/// A parsed theme document, before the store assigns an id and resolves name collisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedTheme {
    /// The document's name, trimmed and checked against the one-line name rules only.
    pub name: String,
    pub base: ThemeChoice,
    pub palette: Palette,
    /// The document's base key when this build does not know it; `base` then holds the
    /// Midnight or Braden fallback, and `import_notice` says so.
    pub unknown_base: Option<String>,
}

impl ImportedTheme {
    pub fn into_theme(self, id: u32) -> CustomTheme {
        CustomTheme {
            id,
            name: self.name,
            base: self.base,
            palette: self.palette,
        }
    }
}

struct DocumentTokens(Palette);

impl Serialize for DocumentTokens {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(TokenKind::ALL.len()))?;
        for kind in TokenKind::ALL {
            map.serialize_entry(kind.key(), &format_hex(self.0.get(kind)))?;
        }
        map.end()
    }
}

#[derive(Serialize)]
struct DocumentOut<'a> {
    format: &'static str,
    version: u64,
    name: &'a str,
    base: ThemeChoice,
    tokens: DocumentTokens,
}

/// A JSON object's members in document order, duplicates included, so the reader can refuse
/// repeated keys instead of silently keeping the last one.
struct Members<V>(Vec<(String, V)>);

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Members<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<V>(std::marker::PhantomData<V>);
        impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<V> {
            type Value = Members<V>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                while let Some(member) = access.next_entry()? {
                    members.push(member);
                }
                Ok(Members(members))
            }
        }
        deserializer.deserialize_map(Visitor(std::marker::PhantomData))
    }
}

/// A top-level member: `tokens` keeps its own members so repeated tokens are visible too.
#[derive(Deserialize)]
#[serde(untagged)]
enum Member {
    Object(Members<Value>),
    Other(Value),
}

const DOCUMENT_KEYS: [&str; 5] = ["format", "version", "name", "base", "tokens"];

/// Keys, checked for repeats, unknown names and missing names, in `expected` order.
fn check_keys<V>(
    members: &[(String, V)],
    expected: impl Iterator<Item = &'static str> + Clone,
    what: &str,
) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for (key, _) in members {
        if !seen.insert(key.as_str()) {
            return Err(format!(
                "The theme file repeats the {what} “{}”.",
                fragment(key)
            ));
        }
        if !expected.clone().any(|known| known == key) {
            return Err(format!(
                "The theme file has an unknown {what} “{}”.",
                fragment(key)
            ));
        }
    }
    if let Some(missing) = expected.into_iter().find(|key| !seen.contains(key)) {
        return Err(format!("The theme file is missing the {what} “{missing}”."));
    }
    Ok(())
}

impl CustomTheme {
    /// A new theme whose tokens start as the base's palette.
    pub fn from_base(id: u32, name: impl Into<String>, base: ThemeChoice) -> Self {
        Self {
            id,
            name: name.into(),
            base,
            palette: base.palette(),
        }
    }

    pub fn is_light(&self) -> bool {
        self.palette.is_light()
    }

    /// The file name the export dialog suggests: the theme's name as a lowercase ASCII slug with
    /// the document's double extension, and `theme` when the name has no usable characters.
    pub fn suggested_file_name(&self) -> String {
        let mut slug = String::new();
        for character in self.name.chars() {
            if character.is_ascii_alphanumeric() {
                slug.push(character.to_ascii_lowercase());
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }
        let slug = slug.trim_matches('-');
        let slug = if slug.is_empty() { "theme" } else { slug };
        format!("{slug}.{THEME_DOCUMENT_FORMAT}.json")
    }

    /// The export document: exactly `format`, `version`, `name`, `base` and the 21 `tokens` as
    /// lowercase `#rrggbb`, in specification order, pretty-printed with a final newline.
    pub fn to_document(&self) -> Vec<u8> {
        let document = DocumentOut {
            format: THEME_DOCUMENT_FORMAT,
            version: THEME_DOCUMENT_VERSION,
            name: &self.name,
            base: self.base,
            tokens: DocumentTokens(self.palette),
        };
        let mut bytes =
            serde_json::to_vec_pretty(&document).expect("theme documents always serialize");
        bytes.push(b'\n');
        bytes
    }

    /// Parse and validate a theme document. Each refusal names the problem. The format and
    /// version are checked before the keys so a newer document reports its version first.
    pub fn from_document(bytes: &[u8]) -> Result<ImportedTheme, String> {
        const LIMIT_KIB: usize = MAX_THEME_DOCUMENT_BYTES / 1024;
        if bytes.len() > MAX_THEME_DOCUMENT_BYTES {
            return Err(format!(
                "This theme file is larger than {LIMIT_KIB} KiB, the most GitTurtle imports."
            ));
        }
        let Members(members) =
            serde_json::from_slice::<Members<Member>>(bytes).map_err(|error| {
                format!(
                    "This file is not a GitTurtle theme: {}.",
                    fragment(&error.to_string())
                )
            })?;
        let value = |key: &str| {
            members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, member)| member)
        };
        let string = |key: &str| match value(key) {
            Some(Member::Other(Value::String(text))) => Some(text.as_str()),
            _ => None,
        };

        if string("format") != Some(THEME_DOCUMENT_FORMAT) {
            return Err(format!(
                "This file is not a GitTurtle theme: “format” must be “{THEME_DOCUMENT_FORMAT}”."
            ));
        }
        match value("version") {
            Some(Member::Other(Value::Number(number))) => match number.as_u64() {
                Some(THEME_DOCUMENT_VERSION) => {}
                Some(version) if version > THEME_DOCUMENT_VERSION => {
                    return Err(format!(
                        "This theme was exported by a newer GitTurtle (format version {version}). Update GitTurtle to import it."
                    ));
                }
                _ => {
                    return Err(format!(
                        "The theme file has an unsupported “version”; expected {THEME_DOCUMENT_VERSION}."
                    ));
                }
            },
            Some(_) => {
                return Err(format!(
                    "The theme file has an unsupported “version”; expected {THEME_DOCUMENT_VERSION}."
                ));
            }
            None => return Err("The theme file is missing the key “version”.".into()),
        }
        check_keys(&members, DOCUMENT_KEYS.into_iter(), "key")?;

        let name = string("name").ok_or("The theme file’s “name” must be a string.")?;
        let name = validate_name_text(name)?.to_owned();
        let base = string("base").ok_or("The theme file’s “base” must be a string.")?;
        let Some(Member::Object(Members(tokens))) = value("tokens") else {
            return Err("The theme file’s “tokens” must be an object.".into());
        };
        check_keys(
            tokens,
            TokenKind::ALL.into_iter().map(TokenKind::key),
            "token",
        )?;

        let mut palette = ThemeChoice::default().palette();
        for (key, value) in tokens {
            let color = value
                .as_str()
                .filter(|text| text.starts_with('#'))
                .and_then(parse_hex)
                .ok_or_else(|| {
                    format!(
                        "The token “{}” must be a #rrggbb color, not {}.",
                        fragment(key),
                        fragment(&value.to_string())
                    )
                })?;
            let kind = TokenKind::from_key(key).expect("token keys were checked");
            palette.set(kind, color);
        }

        let (base, unknown_base) = match built_in_from_key(base) {
            Some(choice) => (choice, None),
            None => (fallback_base(palette), Some(base.to_owned())),
        };
        Ok(ImportedTheme {
            name,
            base,
            palette,
            unknown_base,
        })
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
            ReadabilityForeground::Token(kind) => palette.get(kind),
            ReadabilityForeground::Lane(index) => crate::graph::palette_colors(palette)[index],
        };
        let background_color = match background {
            ReadabilityBackground::Token(kind) => palette.get(kind),
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
    fn tokens_cover_every_palette_field_once_in_group_order() {
        let palette = ThemeChoice::Midnight.palette();
        let mut seen = std::collections::HashSet::new();
        for kind in TokenKind::ALL {
            assert!(seen.insert(kind));
            // Each token writes and reads only its own field.
            let mut changed = palette;
            changed.set(kind, 0x123456);
            assert_eq!(changed.get(kind), 0x123456, "{kind:?}");
            for other in TokenKind::ALL {
                assert_eq!(
                    changed.get(other) == 0x123456,
                    other == kind,
                    "{kind:?} / {other:?}"
                );
            }
            changed.set(kind, palette.get(kind));
            assert_eq!(changed, palette, "{kind:?} round trip");
            assert_eq!(TokenKind::from_key(kind.key()), Some(kind));
            assert!(!kind.label().is_empty());
            let description = kind.description();
            assert!(
                description.ends_with('.') && description.matches('.').count() == 1,
                "{kind:?} has one sentence"
            );
        }
        // Every field of `Palette` is a token: building one from tokens alone is complete.
        let Palette {
            canvas,
            panel,
            subtle,
            hover,
            border,
            text,
            muted,
            accent,
            accent_foreground,
            accent_hover,
            accent_active,
            selected,
            added,
            removed,
            modified,
            renamed,
            warning,
            added_background,
            removed_background,
            hunk,
            line_number,
        } = palette;
        assert_eq!(
            TokenKind::ALL.map(|kind| palette.get(kind)),
            [
                canvas,
                panel,
                subtle,
                hover,
                border,
                selected,
                text,
                muted,
                line_number,
                accent,
                accent_foreground,
                accent_hover,
                accent_active,
                added,
                removed,
                modified,
                renamed,
                warning,
                added_background,
                removed_background,
                hunk,
            ]
        );
        // Groups are contiguous and in the specification's order.
        let groups: Vec<_> = TokenKind::ALL.map(TokenKind::group).into_iter().collect();
        let mut order = groups.clone();
        order.dedup();
        assert_eq!(order, TokenGroup::ALL);
        assert_eq!(
            TokenGroup::ALL.map(|group| groups.iter().filter(|g| **g == group).count()),
            [6, 3, 4, 5, 3]
        );
        assert_eq!(
            TokenGroup::ALL.map(TokenGroup::label),
            ["Surfaces", "Text", "Accent", "Status", "Diff"]
        );
    }

    #[test]
    fn hex_colors_accept_either_case_with_or_without_the_hash() {
        assert_eq!(format_hex(0x0a1b2c), "#0a1b2c");
        assert_eq!(format_hex(0), "#000000");
        for value in ["#0A1B2C", "#0a1b2c", "0a1B2c", "0A1B2C"] {
            assert_eq!(parse_hex(value), Some(0x0a1b2c), "{value}");
        }
        for value in [
            "", "#", "#abc", "#0a1b2c3", "#0a1b2g", " #0a1b2c", "##0a1b2c", "+0a1b2c",
        ] {
            assert_eq!(parse_hex(value), None, "{value:?}");
        }
        let mut palette = ThemeChoice::Nord.palette();
        palette.set(Canvas, 0xff12_3456);
        assert_eq!(palette.canvas, 0x123456, "only 24-bit colors are stored");
    }

    fn sunset() -> CustomTheme {
        let mut theme = CustomTheme::from_base(7, "Sunset", ThemeChoice::Nord);
        theme.palette.set(Accent, 0xd08770);
        theme.palette.set(Hunk, 0xebcb8b);
        theme
    }

    #[test]
    fn documents_round_trip_in_the_specified_shape() {
        let theme = sunset();
        let bytes = theme.to_document();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.starts_with(
            "{\n  \"format\": \"gitturtle-theme\",\n  \"version\": 1,\n  \"name\": \"Sunset\",\n  \"base\": \"nord\",\n  \"tokens\": {\n    \"canvas\": \"#"
        ));
        assert!(text.ends_with("}\n"));
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        let tokens = document["tokens"].as_object().unwrap();
        assert_eq!(document.as_object().unwrap().len(), 5);
        assert_eq!(tokens.len(), 21);
        for kind in TokenKind::ALL {
            assert_eq!(tokens[kind.key()], format_hex(theme.palette.get(kind)));
        }
        // Tokens are written in the specification's order.
        let positions: Vec<_> = TokenKind::ALL
            .map(|kind| text.find(&format!("\"{}\":", kind.key())).unwrap())
            .into_iter()
            .collect();
        assert!(positions.is_sorted());

        let imported = CustomTheme::from_document(&bytes).unwrap();
        assert_eq!(imported.unknown_base, None);
        assert_eq!(imported.into_theme(7), theme);
    }

    fn document_with(edit: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Vec<u8> {
        let mut document: Value = serde_json::from_slice(&sunset().to_document()).unwrap();
        edit(document.as_object_mut().unwrap());
        serde_json::to_vec(&document).unwrap()
    }

    fn tokens(
        document: &mut serde_json::Map<String, Value>,
    ) -> &mut serde_json::Map<String, Value> {
        document.get_mut("tokens").unwrap().as_object_mut().unwrap()
    }

    #[test]
    fn documents_accept_hex_in_either_case_and_trim_the_name() {
        let bytes = document_with(|document| {
            tokens(document).insert("canvas".into(), "#ABCDEF".into());
            tokens(document).insert("panel".into(), "#aBcDeF".into());
            document.insert("name".into(), "  Sunset  ".into());
        });
        let imported = CustomTheme::from_document(&bytes).unwrap();
        assert_eq!(imported.palette.canvas, 0xabcdef);
        assert_eq!(imported.palette.panel, 0xabcdef);
        assert_eq!(imported.name, "Sunset");
    }

    #[test]
    fn documents_refuse_each_problem_with_a_message_naming_it() {
        let refused = |bytes: &[u8]| CustomTheme::from_document(bytes).unwrap_err();
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            (
                "missing token",
                document_with(|document| {
                    tokens(document).remove("hunk");
                }),
                "The theme file is missing the token “hunk”.",
            ),
            (
                "extra token",
                document_with(|document| {
                    tokens(document).insert("sparkle".into(), "#ffffff".into());
                }),
                "The theme file has an unknown token “sparkle”.",
            ),
            (
                "unknown top-level key",
                document_with(|document| {
                    document.insert("author".into(), "someone".into());
                }),
                "The theme file has an unknown key “author”.",
            ),
            (
                "missing top-level key",
                document_with(|document| {
                    document.remove("base");
                }),
                "The theme file is missing the key “base”.",
            ),
            (
                "newer version",
                document_with(|document| {
                    document.insert("version".into(), 2.into());
                    // A newer document may carry keys this version does not know.
                    document.insert("variables".into(), Value::Null);
                }),
                "This theme was exported by a newer GitTurtle (format version 2). Update GitTurtle to import it.",
            ),
            (
                "version zero",
                document_with(|document| {
                    document.insert("version".into(), 0.into());
                }),
                "The theme file has an unsupported “version”; expected 1.",
            ),
            (
                "text version",
                document_with(|document| {
                    document.insert("version".into(), "1".into());
                }),
                "The theme file has an unsupported “version”; expected 1.",
            ),
            (
                "other format",
                document_with(|document| {
                    document.insert("format".into(), "vscode-theme".into());
                }),
                "This file is not a GitTurtle theme: “format” must be “gitturtle-theme”.",
            ),
            (
                "non-hex value",
                document_with(|document| {
                    tokens(document).insert("muted".into(), "#12345g".into());
                }),
                "The token “muted” must be a #rrggbb color, not \"#12345g\".",
            ),
            (
                "hex without hash",
                document_with(|document| {
                    tokens(document).insert("muted".into(), "123456".into());
                }),
                "The token “muted” must be a #rrggbb color, not \"123456\".",
            ),
            (
                "number value",
                document_with(|document| {
                    tokens(document).insert("muted".into(), 1193046.into());
                }),
                "The token “muted” must be a #rrggbb color, not 1193046.",
            ),
            (
                "tokens not an object",
                document_with(|document| {
                    document.insert("tokens".into(), Value::Array(Vec::new()));
                }),
                "The theme file’s “tokens” must be an object.",
            ),
            (
                "empty name",
                document_with(|document| {
                    document.insert("name".into(), " ".into());
                }),
                "Enter a name for this theme.",
            ),
            (
                "multi-line name",
                document_with(|document| {
                    document.insert("name".into(), "Sun\nset".into());
                }),
                "Use a theme name on a single line, without control characters.",
            ),
        ];
        for (case, bytes, message) in cases {
            assert_eq!(refused(&bytes), message, "{case}");
        }

        let text = String::from_utf8(sunset().to_document()).unwrap();
        let repeated_token = text.replacen(
            "\"canvas\": \"#",
            "\"canvas\": \"#000000\", \"canvas\": \"#",
            1,
        );
        assert_eq!(
            refused(repeated_token.as_bytes()),
            "The theme file repeats the token “canvas”."
        );
        let repeated_key = text.replacen(
            "\"name\": \"Sunset\"",
            "\"name\": \"A\", \"name\": \"B\"",
            1,
        );
        assert_eq!(
            refused(repeated_key.as_bytes()),
            "The theme file repeats the key “name”."
        );
        assert!(refused(b"not json").starts_with("This file is not a GitTurtle theme: "));
        assert!(refused(b"[]").starts_with("This file is not a GitTurtle theme: "));

        // A document over 64 KiB is refused before parsing, even when otherwise valid.
        let mut padded = sunset().to_document();
        padded.resize(65 * 1024, b' ');
        assert_eq!(
            refused(&padded),
            "This theme file is larger than 64 KiB, the most GitTurtle imports."
        );
        padded.truncate(MAX_THEME_DOCUMENT_BYTES);
        assert!(CustomTheme::from_document(&padded).is_ok());
    }

    #[test]
    fn documents_with_an_unknown_base_fall_back_by_lightness_with_a_notice() {
        let dark = document_with(|document| {
            document.insert("base".into(), "future_theme".into());
        });
        let imported = CustomTheme::from_document(&dark).unwrap();
        assert_eq!(imported.base, ThemeChoice::Midnight);
        assert_eq!(imported.palette, sunset().palette, "tokens are kept");
        assert_eq!(imported.unknown_base.as_deref(), Some("future_theme"));
        assert_eq!(
            import_notice("Sunset", "Sunset", Some("future_theme"), imported.base),
            "Imported “Sunset”. The base theme “future_theme” is not available in this version of GitTurtle; Midnight is used as the base."
        );

        let mut light = CustomTheme::from_base(1, "Paper", ThemeChoice::Porcelain).to_document();
        light = String::from_utf8(light)
            .unwrap()
            .replace("\"porcelain\"", "\"parchment\"")
            .into_bytes();
        let imported = CustomTheme::from_document(&light).unwrap();
        assert_eq!(imported.base, ThemeChoice::Daylight);
        assert_eq!(imported.unknown_base.as_deref(), Some("parchment"));
        assert!(
            import_notice("Paper", "Paper", Some("parchment"), imported.base)
                .contains("Braden is used as the base")
        );

        for choice in ThemeChoice::ALL {
            let theme = CustomTheme::from_base(1, "Copy", choice);
            let imported = CustomTheme::from_document(&theme.to_document()).unwrap();
            assert_eq!(imported.base, choice);
            assert_eq!(imported.unknown_base, None);
        }
    }

    /// A document is supplied bytes, so a key, a value or a base name in it can
    /// be as long as the reader accepts. Every message that quotes one clamps
    /// it, so nothing downstream — the card's text or its aria_label — carries
    /// an unbounded string.
    #[test]
    fn messages_clamp_the_text_they_quote_from_a_document() {
        const LONG: usize = 4096;
        let clamped = |text: &str| {
            format!(
                "{}\u{2026}",
                text.chars()
                    .take(MAX_MESSAGE_FRAGMENT_CHARS)
                    .collect::<String>()
            )
        };
        let long_key = "k".repeat(LONG);
        let long_value = "v".repeat(LONG);
        let long_base = "b".repeat(LONG);

        let cases: Vec<(&str, Vec<u8>, String)> = vec![
            (
                "an unknown key",
                document_with(|document| {
                    document.insert(long_key.clone(), Value::String("x".into()));
                }),
                format!(
                    "The theme file has an unknown key “{}”.",
                    clamped(&long_key)
                ),
            ),
            (
                "an unknown token",
                document_with(|document| {
                    tokens(document).insert(long_key.clone(), Value::String("#ffffff".into()));
                }),
                format!(
                    "The theme file has an unknown token “{}”.",
                    clamped(&long_key)
                ),
            ),
            (
                "a token value",
                document_with(|document| {
                    tokens(document).insert("canvas".into(), Value::String(long_value.clone()));
                }),
                format!(
                    "The token “canvas” must be a #rrggbb color, not {}.",
                    clamped(&format!("\"{long_value}\""))
                ),
            ),
        ];
        for (what, bytes, expected) in cases {
            assert_eq!(
                CustomTheme::from_document(&bytes).unwrap_err(),
                expected,
                "{what}"
            );
        }

        // A repeat keeps its own message, which a `serde_json::Value`
        // object cannot express. An unknown name is reported before a
        // repeat, so this repeats a known key; the clamp there guards the
        // same way.
        let repeated =
            "{\"format\":\"gitturtle-theme\",\"version\":1,\"name\":\"S\",\"name\":\"S\"}";
        assert_eq!(
            CustomTheme::from_document(repeated.as_bytes()).unwrap_err(),
            "The theme file repeats the key “name”."
        );

        // A parser refusal quotes the JSON parser's own message, which is
        // free to name the input it choked on; clamped, it stays a sentence.
        let refusal = CustomTheme::from_document(
            &b"\""
                .iter()
                .copied()
                .chain(long_value.bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap_err();
        assert!(
            refusal.chars().count() <= MAX_MESSAGE_FRAGMENT_CHARS + 40,
            "{refusal}"
        );

        // The unknown base reaches a notice rather than a refusal. It is
        // carried whole and clamped where the notice is built, to the base's
        // own `MAX_BASE_FRAGMENT_CHARS`; the theme still imports.
        let imported = CustomTheme::from_document(&document_with(|document| {
            document.insert("base".into(), Value::String(long_base.clone()));
        }))
        .unwrap();
        assert_eq!(imported.base, ThemeChoice::Midnight);
        assert_eq!(imported.palette, sunset().palette);
        assert_eq!(imported.unknown_base.as_deref(), Some(long_base.as_str()));
        assert_eq!(
            import_notice("Sunset", "Sunset", Some(&long_base), imported.base),
            format!(
                "Imported “Sunset”. The base theme “{}…” is not available in this version of GitTurtle; Midnight is used as the base.",
                long_base
                    .chars()
                    .take(MAX_BASE_FRAGMENT_CHARS)
                    .collect::<String>()
            )
        );

        // A fragment at the bound is quoted whole, with no ellipsis.
        let exact = "e".repeat(MAX_MESSAGE_FRAGMENT_CHARS);
        assert_eq!(
            CustomTheme::from_document(&document_with(|document| {
                document.insert(exact.clone(), Value::String("x".into()));
            }))
            .unwrap_err(),
            format!("The theme file has an unknown key “{exact}”.")
        );
    }

    /// The card clamps the notice to two lines and marks a cut only at the
    /// render site, so the sentence has to fit `MAX_NOTICE_CHARS` whichever
    /// clauses it carries. A renamed import whose base was also replaced quotes three
    /// document strings; with names at the 128-byte bound and a long unknown
    /// base each is shortened so the base clause, the one that tells the
    /// user a substitution happened and which built-in stood in, is reached.
    #[test]
    fn a_three_clause_notice_fits_its_budget_and_still_names_the_base() {
        let name = format!("{} (imported)", "n".repeat(117));
        let document_name = "n".repeat(128);
        let unknown_base = "u".repeat(128);
        let quoted = |notice: &str| {
            notice
                .split('“')
                .skip(1)
                .map(|rest| rest.split('”').next().unwrap().to_owned())
                .collect::<Vec<_>>()
        };

        for base in [ThemeChoice::Midnight, ThemeChoice::Daylight] {
            let notice = import_notice(&name, &document_name, Some(&unknown_base), base);
            assert!(
                notice.chars().count() <= MAX_NOTICE_CHARS,
                "{} characters: {notice}",
                notice.chars().count()
            );
            assert!(
                notice.ends_with(&format!(
                    "” is not available in this version of GitTurtle; {} is used as the base.",
                    base.label()
                )),
                "{notice}"
            );
            assert!(notice.contains("already exists, so the imported one was renamed."));
            let fragments = quoted(&notice);
            assert_eq!(fragments.len(), 3, "{notice}");
            for fragment in &fragments {
                let kept = fragment
                    .strip_suffix('…')
                    .expect("every long fragment is cut");
                let characters = kept.chars().count();
                assert!(
                    (MIN_MESSAGE_FRAGMENT_CHARS..=MAX_MESSAGE_FRAGMENT_CHARS).contains(&characters),
                    "{fragment}"
                );
            }
            assert!(name.starts_with(&fragments[0][..fragments[0].len() - '…'.len_utf8()]));
        }

        // The budget is only felt with three clauses: alone, or with one
        // other, a name keeps the full `MAX_MESSAGE_FRAGMENT_CHARS` and the
        // base its own `MAX_BASE_FRAGMENT_CHARS`, so every notice the
        // evidence captures renders as before.
        let clamped_to =
            |text: &str, most: usize| format!("{}…", text.chars().take(most).collect::<String>());
        let clamped = |text: &str| clamped_to(text, MAX_MESSAGE_FRAGMENT_CHARS);
        assert_eq!(
            import_notice(&document_name, &document_name, None, ThemeChoice::Nord),
            format!("Imported “{}”.", clamped(&document_name))
        );
        assert_eq!(
            import_notice(&name, &document_name, None, ThemeChoice::Nord),
            format!(
                "Imported “{}”. A theme named “{}” already exists, so the imported one was renamed.",
                clamped(&name),
                clamped(&document_name)
            )
        );
        assert_eq!(
            import_notice(
                &document_name,
                &document_name,
                Some(&unknown_base),
                ThemeChoice::Midnight
            ),
            format!(
                "Imported “{}”. The base theme “{}” is not available in this version of GitTurtle; Midnight is used as the base.",
                clamped(&document_name),
                clamped_to(&unknown_base, MAX_BASE_FRAGMENT_CHARS)
            )
        );
        // A short string is quoted whole however many clauses there are, and
        // hands its unused share to a long one beside it.
        let notice = import_notice(
            "Aurora Light (imported)",
            "Aurora Light",
            Some(&unknown_base),
            ThemeChoice::Daylight,
        );
        assert!(
            notice.starts_with(
                "Imported “Aurora Light (imported)”. A theme named “Aurora Light” already exists, so the imported one was renamed. The base theme “uuuuuuuuuuuuuuuuuuuu"
            ),
            "{notice}"
        );
        assert!(
            notice.ends_with(
                "u…” is not available in this version of GitTurtle; Braden is used as the base."
            ),
            "{notice}"
        );
        assert_eq!(
            notice.chars().count(),
            MAX_NOTICE_CHARS,
            "the long base takes every character the short names left: {notice}"
        );
        assert_eq!(
            import_notice(
                "Aurora Light (imported)",
                "Aurora Light",
                Some("aurora-9"),
                ThemeChoice::Daylight
            ),
            "Imported “Aurora Light (imported)”. A theme named “Aurora Light” already exists, so the imported one was renamed. The base theme “aurora-9” is not available in this version of GitTurtle; Braden is used as the base."
        );
    }

    /// The quoted base is one unbreakable token that wraps whole, so it is
    /// capped below the names to fit one line with its fixed tail; the
    /// budget alone cannot see where the lines break.
    #[test]
    fn a_two_clause_notice_keeps_its_base_fragment_on_one_line() {
        let name = "n".repeat(128);
        let unknown_base = "u".repeat(128);
        let notice = import_notice(&name, &name, Some(&unknown_base), ThemeChoice::Midnight);
        assert!(
            notice.chars().count() <= MAX_NOTICE_CHARS,
            "{} characters: {notice}",
            notice.chars().count()
        );
        assert!(
            notice.ends_with("; Midnight is used as the base."),
            "{notice}"
        );
        let base_fragment = notice
            .split(" The base theme “")
            .nth(1)
            .and_then(|rest| rest.split('”').next())
            .expect("the notice quotes the base");
        let kept = base_fragment
            .strip_suffix('…')
            .expect("a 128-byte base is cut");
        assert!(
            kept.chars().count() <= MAX_BASE_FRAGMENT_CHARS,
            "{} characters: {base_fragment}",
            kept.chars().count()
        );
        assert!(unknown_base.starts_with(kept));
        // The name beside it still keeps the full name allowance.
        assert!(
            notice.starts_with(&format!(
                "Imported “{}…”.",
                "n".repeat(MAX_MESSAGE_FRAGMENT_CHARS)
            )),
            "{notice}"
        );
        // A real base key is far shorter than the cap and is quoted whole.
        assert_eq!(
            import_notice(
                "Aurora",
                "Aurora",
                Some("future_theme"),
                ThemeChoice::Midnight
            ),
            "Imported “Aurora”. The base theme “future_theme” is not available in this version of GitTurtle; Midnight is used as the base."
        );
    }

    #[test]
    fn selections_keep_built_ins_as_bare_strings_and_customs_as_objects() {
        for choice in ThemeChoice::ALL {
            let selection = ThemeSelection::from(choice);
            let encoded = serde_json::to_value(selection).unwrap();
            assert_eq!(encoded, serde_json::to_value(choice).unwrap());
            assert!(encoded.is_string());
            assert_eq!(
                serde_json::from_value::<ThemeSelection>(encoded).unwrap(),
                selection
            );
        }
        assert_eq!(
            serde_json::to_string(&ThemeSelection::BuiltIn(ThemeChoice::Nord)).unwrap(),
            r#""nord""#
        );
        assert_eq!(
            serde_json::to_string(&ThemeSelection::Custom(7)).unwrap(),
            r#"{"custom":7}"#
        );
        assert_eq!(
            serde_json::from_str::<ThemeSelection>(r#"{"custom": 7}"#).unwrap(),
            ThemeSelection::Custom(7)
        );
        // Unknown built-ins keep the existing safe default.
        assert_eq!(
            serde_json::from_str::<ThemeSelection>(r#""future-theme""#).unwrap(),
            ThemeSelection::BuiltIn(ThemeChoice::Midnight)
        );
        for malformed in [
            r#"{"custom": -1}"#,
            r#"{"custom": "7"}"#,
            r#"{"custom": 7, "name": "x"}"#,
            r#"{}"#,
            "7",
        ] {
            assert!(
                serde_json::from_str::<ThemeSelection>(malformed).is_err(),
                "{malformed}"
            );
        }
        assert_eq!(
            ThemeSelection::default(),
            ThemeSelection::BuiltIn(ThemeChoice::default())
        );
    }

    #[test]
    fn selections_resolve_to_their_palette_or_the_default_when_missing() {
        let theme = sunset();
        let customs = [theme.clone()];
        let resolved = ThemeSelection::Custom(7).resolve(&customs);
        assert_eq!(resolved.selection, ThemeSelection::Custom(7));
        assert_eq!(resolved.palette, theme.palette);
        assert!(!resolved.is_light);

        let missing = ThemeSelection::Custom(99).resolve(&customs);
        assert_eq!(missing, ResolvedTheme::built_in(ThemeChoice::default()));

        let braden = ThemeSelection::BuiltIn(ThemeChoice::Daylight).resolve(&customs);
        assert_eq!(braden.palette, ThemeChoice::Daylight.palette());
        assert!(braden.is_light);

        let light = CustomTheme::from_base(8, "Paper", ThemeChoice::Porcelain);
        assert!(ResolvedTheme::custom(&light).is_light);
    }

    #[test]
    fn new_theme_ids_follow_the_largest_existing_id() {
        assert_eq!(next_custom_theme_id(&[]), Some(1));
        let mut themes = vec![
            CustomTheme::from_base(3, "A", ThemeChoice::Nord),
            CustomTheme::from_base(9, "B", ThemeChoice::Nord),
        ];
        assert_eq!(next_custom_theme_id(&themes), Some(10));
        themes[1].id = u32::MAX;
        assert_eq!(next_custom_theme_id(&themes), None);
    }

    #[test]
    fn theme_names_follow_project_rules_and_stay_unique() {
        let customs = [
            sunset(),
            CustomTheme::from_base(8, "Paper", ThemeChoice::Porcelain),
        ];
        assert_eq!(validate_theme_name("Dusk", &customs, None), Ok(()));
        assert_eq!(validate_theme_name("  Dusk  ", &customs, None), Ok(()));
        assert_eq!(
            validate_theme_name(" \t ", &customs, None),
            Err("Enter a name for this theme.".into())
        );
        assert_eq!(
            validate_theme_name(&"a".repeat(128), &customs, None),
            Ok(())
        );
        assert_eq!(
            validate_theme_name(&"a".repeat(129), &customs, None),
            Err("Use a theme name of at most 128 bytes.".into())
        );
        // The byte bound applies after trimming.
        assert_eq!(
            validate_theme_name(&format!("  {}  ", "a".repeat(128)), &customs, None),
            Ok(())
        );
        for name in ["Sun\nset", "Sun\tset", "Sun\u{2028}set"] {
            assert_eq!(
                validate_theme_name(name, &customs, None),
                Err("Use a theme name on a single line, without control characters.".into()),
                "{name:?}"
            );
        }
        // Unique among custom themes, ignoring case, except for the theme being renamed.
        assert_eq!(
            validate_theme_name("SUNSET", &customs, None),
            Err("Another theme is already named “SUNSET”.".into())
        );
        assert_eq!(validate_theme_name("sunset", &customs, Some(7)), Ok(()));
        assert_eq!(
            validate_theme_name("paper", &customs, Some(7)),
            Err("Another theme is already named “paper”.".into())
        );
        // Never a built-in label, in any case, even while renaming.
        for choice in ThemeChoice::ALL {
            let expected = Err(format!(
                "“{}” is a built-in theme. Choose another name.",
                choice.label()
            ));
            assert_eq!(validate_theme_name(choice.label(), &[], None), expected);
            assert_eq!(
                validate_theme_name(&choice.label().to_uppercase(), &customs, Some(7)),
                expected
            );
        }
        // The storage key of Braden is not its label.
        assert_eq!(validate_theme_name("Daylight", &[], None), Ok(()));
    }

    #[test]
    fn imported_names_keep_their_own_name_or_take_the_imported_suffix() {
        // A free name is kept, whatever the store already holds.
        assert_eq!(import_name("Sunset", &[]), Ok("Sunset".into()));
        assert_eq!(import_name("  Sunset  ", &[]), Ok("Sunset".into()));

        let mut customs = vec![sunset()];
        assert_eq!(
            import_name("SUNSET", &customs),
            Ok("SUNSET (imported)".into())
        );
        // A built-in label is taken, as the store never accepts one.
        assert_eq!(import_name("Nord", &customs), Ok("Nord (imported)".into()));

        // Repeated imports number the suffix.
        customs.push(CustomTheme::from_base(
            8,
            "Sunset (imported)",
            ThemeChoice::Nord,
        ));
        assert_eq!(
            import_name("Sunset", &customs),
            Ok("Sunset (imported) (2)".into())
        );
        customs.push(CustomTheme::from_base(
            9,
            "Sunset (imported) (2)",
            ThemeChoice::Nord,
        ));
        assert_eq!(
            import_name("Sunset", &customs),
            Ok("Sunset (imported) (3)".into())
        );

        // A name at the byte bound is shortened on a character boundary so the suffix fits.
        let long = format!("{}é", "a".repeat(126));
        assert_eq!(long.len(), MAX_PROJECT_NAME_BYTES);
        customs.push(CustomTheme::from_base(10, long.clone(), ThemeChoice::Nord));
        let taken = import_name(&long, &customs).unwrap();
        assert_eq!(taken, format!("{} (imported)", "a".repeat(117)));
        assert!(taken.len() <= MAX_PROJECT_NAME_BYTES);
        assert_eq!(validate_theme_name(&taken, &customs, None), Ok(()));

        // A document name the store could never accept is refused, not repaired.
        assert_eq!(
            import_name(" \t ", &customs),
            Err("Enter a name for this theme.".into())
        );
    }

    #[test]
    fn suggested_file_names_slug_the_theme_name() {
        let named =
            |name: &str| CustomTheme::from_base(1, name, ThemeChoice::Nord).suggested_file_name();
        assert_eq!(named("Sunset"), "sunset.gitturtle-theme.json");
        assert_eq!(named("Deep Sea 2"), "deep-sea-2.gitturtle-theme.json");
        assert_eq!(named("  Rosé //Pine  "), "ros-pine.gitturtle-theme.json");
        // A name with nothing usable still has a file name, and no path separator survives.
        assert_eq!(named("…"), "theme.gitturtle-theme.json");
        assert_eq!(named("../etc"), "etc.gitturtle-theme.json");
    }
}
