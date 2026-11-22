//! OKLab-based syntax role and editor highlight fitting.

use crate::custom;
use crate::palette::{
    LABEL_MARGIN, LABEL_RULE, MORE_CONTRAST_EDGE, MORE_CONTRAST_TEXT, Palette, toward_extreme_in,
};
use std::collections::BinaryHeap;

impl Palette {
    /// What [`Self::apply`] draws for this palette, decided once per
    /// application: the palette it stores, adjusted by
    /// [`Self::more_contrast_fit`] while `more_contrast` is on and itself
    /// otherwise, its syntax roles and its editor highlights, fitted from this
    /// palette as chosen under More contrast ([`EditorHighlights::fit`]).
    /// [`editor_highlights`] fits a test application's default palette
    /// through it too, so no path fits highlights another way.
    pub fn fitted(self, more_contrast: bool) -> (Self, SyntaxRoles, EditorHighlights) {
        let (palette, chosen) = if more_contrast {
            let (palette, chosen) = self.more_contrast_fit();
            (palette, Some(chosen))
        } else {
            (self, None)
        };
        let roles = palette.syntax_roles(more_contrast.then_some(self));
        (
            palette,
            roles,
            EditorHighlights::fit(palette, roles, chosen),
        )
    }

    /// Each syntax role's palette color fitted, measured once per application. With More
    /// contrast off, `chosen` is `None` and each is fitted by [`SyntaxTargets::fit`] at the
    /// text rule with the rasterization margin. Under More contrast, this palette is the one
    /// More contrast draws and `chosen` the palette as chosen, before it was adjusted: each is
    /// fitted at [`MORE_CONTRAST_TEXT`] in OKLab lightness, its hue kept, and never reads on
    /// a background worse than the role drawn off on `chosen` reads on `chosen`'s
    /// ([`SyntaxTargets::fit_in_lightness`]). Comments are secondary text and follow `muted`,
    /// which More contrast raises toward `text` by the secondary-text rule
    /// ([`Self::with_raised_floors`]), so they are fitted by [`SyntaxTargets::fit`] at the rule
    /// in force in both.
    ///
    /// Why `chosen`: where a line tint moved ([`Self::more_contrast_fit`]), the off fit on this
    /// palette is not the role drawn off, and a role held only to it could read worse on the
    /// editor background or the active line than the role drawn off does.
    pub fn syntax_roles(self, chosen: Option<Palette>) -> SyntaxRoles {
        let rule = if chosen.is_some() {
            MORE_CONTRAST_TEXT
        } else {
            LABEL_RULE + LABEL_MARGIN
        };
        let targets = SyntaxTargets::new(self, rule);
        let off = chosen.map(|chosen| SyntaxTargets::new(chosen, LABEL_RULE + LABEL_MARGIN));
        let fit = |color: u32| match &off {
            Some(off) => targets.fit_in_lightness(color, off),
            None => targets.fit(color),
        };
        SyntaxRoles {
            keyword: fit(self.renamed),
            string: fit(self.added),
            constant: fit(self.warning),
            type_: fit(self.modified),
            function: fit(self.hunk),
            markup: fit(self.accent),
            comment: targets.fit(self.muted),
            removed: fit(self.removed),
            // `text` reaches every target by definition, so fitting would return it unchanged.
            text: self.text,
        }
    }

    /// The editor highlights [`Self::apply`] fits for this palette, without an application.
    pub fn editor_highlights(self) -> EditorHighlights {
        EditorHighlights::fit(self, self.syntax_roles(None), None)
    }
}

/// The palette color each syntax role draws in once fitted: `keyword` from `renamed`, `string`
/// from `added`, `constant` from `warning`, `type_` from `modified`, `function` from `hunk`,
/// `markup` from `accent`, `comment` from `muted`, `removed` from `removed`, and `text` as it
/// is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxRoles {
    pub keyword: u32,
    pub string: u32,
    pub constant: u32,
    pub type_: u32,
    pub function: u32,
    pub markup: u32,
    pub comment: u32,
    pub removed: u32,
    pub text: u32,
}

impl SyntaxRoles {
    /// The colors a word tint carries: every role but `removed`, which only a diff file's
    /// removed lines draw in, and which a compared language's words never take.
    pub fn word_colors(self) -> [u32; 8] {
        [
            self.keyword,
            self.string,
            self.constant,
            self.type_,
            self.function,
            self.markup,
            self.comment,
            self.text,
        ]
    }

    /// Every color the editors draw syntax in: the eight fitted roles and `text`.
    pub fn colors(self) -> [u32; 9] {
        [
            self.keyword,
            self.string,
            self.constant,
            self.type_,
            self.function,
            self.markup,
            self.comment,
            self.removed,
            self.text,
        ]
    }
}

/// The backgrounds syntax colors are drawn on, measured once per palette application: the
/// editor background, which hunk header lines keep (their decoration sets no background), the
/// active line, and the added and removed line tints of the unified and split diffs.
struct SyntaxTargets {
    pub text: u32,
    /// Whether `text` reads at the text rule on every background.
    text_reads: bool,
    /// Whether `text` is lighter than the editor background, as in a dark palette: the side
    /// [`Self::fit_in_lightness`] moves a role toward.
    text_lighter: bool,
    /// Each background's luminance and the contrast a syntax color must reach on it: the
    /// fitting's rule (the text rule plus the rasterization margin, or More contrast's 7:1), or
    /// `text`'s own contrast where that is lower.
    backgrounds: [(f64, f64); 4],
}

impl SyntaxTargets {
    pub fn new(palette: Palette, rule: f64) -> Self {
        let text = custom::luminance(palette.text);
        let mut text_reads = true;
        let backgrounds = [
            palette.canvas,
            palette.panel,
            palette.added_background,
            palette.removed_background,
        ]
        .map(|background| {
            let background = custom::luminance(background);
            let text_contrast = custom::luminance_contrast(text, background);
            text_reads &= text_contrast >= LABEL_RULE;
            (background, rule.min(text_contrast))
        });
        Self {
            text: palette.text,
            text_reads,
            text_lighter: text > custom::luminance(palette.canvas),
            backgrounds,
        }
    }

    fn reads(&self, color: u32) -> bool {
        let color = custom::luminance(color);
        self.backgrounds
            .iter()
            .all(|&(background, minimum)| custom::luminance_contrast(color, background) >= minimum)
    }

    /// `color` as it is, when it reads on every background, or moved toward `text` by a step
    /// that reads, found by a binary search over 256ths of each sRGB channel; every step it
    /// accepts reads, so the result is at least 4.5:1 on each background. `text` itself reads
    /// everywhere, so the search always ends. A palette whose `text` is below the text rule on
    /// one of them, which only a custom theme with a readability warning can be, draws its
    /// syntax in `text`.
    pub fn fit(&self, color: u32) -> u32 {
        if !self.text_reads {
            return self.text;
        }
        if self.reads(color) {
            return color;
        }
        let toward_text = |step: u32| mix(color, self.text, step);
        // `high` always reads (at 256 steps the color is `text`) and `low` never does.
        let (mut low, mut high) = (0, 256);
        while high - low > 1 {
            let middle = (low + high) / 2;
            if self.reads(toward_text(middle)) {
                high = middle;
            } else {
                low = middle;
            }
        }
        toward_text(high)
    }

    /// More contrast's fit for the syntax role drawn from `color`, with `off` the targets of
    /// the palette as chosen with More contrast off: `color` as it is where it reads on every
    /// background, and
    /// otherwise `color` moved in OKLab lightness toward `text`'s side, lighter in a dark
    /// palette and darker in a light one, its hue and chroma kept, or where sRGB cannot hold
    /// that chroma at that lightness the most of it sRGB holds in the same hue
    /// ([`linear_in_gamut`]), to the least step, in thousandths of lightness, that reads on
    /// every background, found by a binary search. It never moves past `text`'s lightness:
    /// where no step short of it reads, the role stops at `text`'s lightness in its own hue,
    /// and a color already level with `text` or past it does not move. Where what that gives
    /// reads on a background below the role drawn off (`off`'s fit of `color`) on the same
    /// background as chosen, the role drawn off stays, so no role reads worse than with More
    /// contrast off; the role drawn off is fitted only then. A palette whose `text` is below
    /// the text rule on a background, which only
    /// a custom theme with a readability warning can be, draws its syntax in `text`, as
    /// [`Self::fit`] does.
    ///
    /// Why lightness: moving toward `text` in sRGB spends chroma and turns the hue, so where
    /// `text` reads under 7:1 roles ended at `text` itself (Omarchy tokyo-night's keyword,
    /// function and markup), and short of that they greyed (Omarchy catppuccin's removed
    /// #f38ba8 drew #d9bedb); lightness is the channel hierarchy is built from, so contrast
    /// raised through it keeps each role's hue. Why the bound: syntax is never lighter than
    /// body text (darker in a light palette), the lightness form of the secondary-text rule;
    /// unbound, 29 roles in the shipped and Omarchy palettes passed `text`'s lightness, One
    /// Dark's keyword by 0.020.
    fn fit_in_lightness(&self, color: u32, off: &SyntaxTargets) -> u32 {
        if !self.text_reads {
            return self.text;
        }
        if self.reads(color) {
            return color;
        }
        let ([lightness, a, b], text) = (oklab(color), oklab(self.text)[0]);
        let toward = if self.text_lighter { 1. } else { -1. };
        let span = (text - lightness) * toward;
        let fitted = if span > 0. {
            let steps = (span * 1000.).ceil() as u32;
            let at = |step: u32| {
                let lightness = if step >= steps {
                    text
                } else {
                    lightness + toward * f64::from(step) / 1000.
                };
                from_linear(linear_in_gamut([lightness, a, b]))
            };
            if self.reads(at(steps)) {
                // `high` always reads and `low` never does (at 0 steps the color is `color`).
                let (mut low, mut high) = (0, steps);
                while high - low > 1 {
                    let middle = (low + high) / 2;
                    if self.reads(at(middle)) {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                at(high)
            } else {
                at(steps)
            }
        } else {
            color
        };
        let drawn_off = off.fit(color);
        let (fitted_luminance, off_luminance) =
            (custom::luminance(fitted), custom::luminance(drawn_off));
        // Both hold the editor background, the active line and the two line tints in order.
        let reads_as_off = self.backgrounds.iter().zip(&off.backgrounds).all(
            |(&(background, _), &(off_background, _))| {
                custom::luminance_contrast(fitted_luminance, background)
                    >= custom::luminance_contrast(off_luminance, off_background)
            },
        );
        if reads_as_off { fitted } else { drawn_off }
    }
}

/// `from` moved `step` 256ths of the way to `to` in each sRGB channel, rounded.
pub fn mix(from: u32, to: u32, step: u32) -> u32 {
    [16, 8, 0].into_iter().fold(0, |mixed, shift| {
        let (from, to) = ((from >> shift) & 0xff, (to >> shift) & 0xff);
        mixed | (((from * (256 - step) + to * step + 128) / 256) << shift)
    })
}

/// The colors the code editors draw behind text, fitted once per palette application by
/// [`EditorHighlights::fit`] and kept as a global beside the palette: the text selection, Find's
/// other and current matches, and the added and removed changed-word tints. Rows and lists keep
/// the palette's `selected`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorHighlights {
    pub selection: u32,
    pub find_match: u32,
    pub find_current: u32,
    pub added_word: u32,
    pub removed_word: u32,
    /// The 1 px rule an added word draws along its highlight box's bottom edge under More
    /// contrast ([`Self::word_rule`]), so a changed word is marked by more than colour; `None`
    /// with More contrast off.
    pub added_rule: Option<u32>,
    /// The removed word's rule, as [`Self::added_rule`].
    pub removed_rule: Option<u32>,
    /// The colours the unified patch draws its text in, as `text::theme_decorations` decorates
    /// it: added lines and their words, removed lines and their words, and hunk headers. With
    /// More contrast off these are the palette's `added`, `removed` and `hunk` as chosen, so a
    /// patch draws as it always has; under More contrast they are the syntax roles fitted from
    /// them, `string`, `removed` and `function` ([`SyntaxRoles`]), the colours Split, Before and
    /// After draw a patch's lines in, so one file never draws one change in two colours by view
    /// (decision, 2026-11-05: off, the Diff view kept its colours drawn off beside Split's
    /// raised roles, and a removed word read 4.82:1 in Omarchy tokyo-night).
    pub patch_text: PatchText,
}

/// The unified patch's text colours ([`EditorHighlights::patch_text`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchText {
    pub added: u32,
    pub removed: u32,
    pub hunk: u32,
}

/// How far every fitted editor highlight stands from each line background it is drawn on,
/// measured as ΔEOK, the Euclidean distance between the two colors in OKLab (the measure CSS
/// Color 4 names deltaEOK). [`EditorHighlights::fit`] keeps it for every highlight.
///
/// Why this measure: a highlight may only move in lightness until every color on it reads, and
/// where a palette's syntax is fitted to 4.75:1 on a line, a highlight lighter than that line
/// (toward the syntax) by the selected-row rule's 1.15:1 would leave that syntax at 4.13:1, so a
/// contrast ratio cannot both keep the syntax readable and the highlight apart. ΔEOK counts
/// lightness, chroma and hue together, so a highlight that keeps its seed's hue, or gains
/// chroma, stands apart from a line of nearly the same lightness, as a tinted selection over a
/// grey line visibly does.
///
/// Why this threshold: one just-noticeable difference in OKLab is about 0.02; 0.04 is two, and
/// sits just under the closest `selected` any built-in keeps from its canvas or active line on
/// origin/main (0.044, One Light's canvas and One Dark's active line), the distance the curated
/// palettes already accept as a visible selection. Origin's word tints stood at least 0.10 from
/// their lines, and its `selected` as little as 0.017 from Daylight's added lines.
pub const HIGHLIGHT_DISTANCE: f64 = 0.04;

/// The least gap in OKLab lightness a changed-word tint keeps from its line tint under More
/// contrast where the colors on it allow ([`EditorHighlights::lifted_word_tint`]).
///
/// Why lightness: a tint can stand [`HIGHLIGHT_DISTANCE`] from its line through hue and chroma
/// alone, and those are what colour-blind vision loses; the gap in lightness is what survives
/// it, so a rule met in OKLab distance could be met through chroma alone. With More contrast
/// off, Omarchy tokyo-night's added word stands 0.005 from its line in lightness, and 0.001 in
/// a protanopia model, so the changed word nearly vanishes. Why this gap: the same two
/// just-noticeable differences as [`HIGHLIGHT_DISTANCE`], here in lightness alone.
pub const WORD_LIGHTNESS_GAP: f64 = 0.04;

/// The least gap in OKLab lightness the highlights drawn beside one another keep under More
/// contrast, besides [`HIGHLIGHT_DISTANCE`] in ΔEOK: Find's current match from the selection,
/// and Find's other matches from the selection, the current match and the changed-word tints
/// ([`HighlightFit::apart_from`]), and a word stepped across its line from the line
/// ([`EditorHighlights::crossed_word_tint`]).
///
/// Why lightness: two highlights can stand [`HIGHLIGHT_DISTANCE`] apart through chroma alone,
/// which colour-blind vision loses (with More contrast off, One Dark's selection and current
/// match stand 0.044 apart and 0.0001 in lightness), and a distance met by saturation alone
/// failed a review before. Why this gap: one just-noticeable difference in OKLab, so the
/// lightness step alone is visible while hue and chroma carry the rest of the distance. The
/// five highlights share the lightness band in which every colour on them still reads, and two
/// differences between each pair exhausted it: Alucard's other matches found no colour that
/// reads and stands 0.04 in lightness from the selection, the current match and both word
/// tints, and fell back to the selection's, 0 apart from it, while other palettes' matches were
/// pushed to the band's edges (Kanagawa Lotus's other matches `#f2f6f8`, beside its canvas,
/// Rosé Pine Dawn's a saturated `#ffabff`; measured 2026-11-05).
pub const HIGHLIGHT_LIGHTNESS_GAP: f64 = 0.02;

impl EditorHighlights {
    /// Fit each highlight to this palette.
    ///
    /// Each starts from a seed: the selection and Find's other matches from `selected`, the
    /// current match from `selected` a quarter of the way to `accent`, and each word tint from its
    /// line tint a quarter of the way to its diff color, the tint origin/main drew. A seed is kept
    /// when every color drawn on it reads and it stands [`HIGHLIGHT_DISTANCE`] from every line
    /// background it sits on. Otherwise the highlight takes the candidate nearest the seed, by
    /// ΔEOK, that does both: candidates are the seed moved toward black in a dark palette or white
    /// in a light one in 256ths of each sRGB channel, and the seed's hue at more chroma
    /// ([`chroma_rows`]), which stands apart where lightness has no room left. A chroma
    /// candidate is ranked by the coordinates it was built at, before clipping to sRGB.
    ///
    /// A color reads when it reaches 4.75:1 on the highlight (the text rule plus the
    /// rasterization margin), or its own contrast on the line it is drawn on where that is
    /// lower, never below 4.5:1, as syntax fitting does; when no candidate does that and stands
    /// apart, the nearest at 4.5:1 that stands apart, and when none stands apart at all, the first
    /// step toward black or white at 4.5:1. The colors drawn on the selection and on Find's
    /// matches are `text` and every syntax role, drawn on the editor background, the active line
    /// and both line tints, and the unified patch's `added`, `removed` and `hunk`, drawn on their
    /// line tints and the editor background; a word tint carries `text`, every syntax role but
    /// `removed` ([`SyntaxRoles::word_colors`]) and its diff color, and sits on its line tint. The current match stands apart from the other
    /// matches too where it can; its accent underline marks it where it cannot.
    ///
    /// Under More contrast, `palette` is the adjusted palette, `roles` are fitted at
    /// [`MORE_CONTRAST_TEXT`] and `chosen` is the palette as chosen, and the selection, Find's
    /// current match and the word tints start from the ones `chosen` draws off, so no color
    /// reads worse on them and they stand from their lines on the same side, no nearer and with
    /// no less luminance contrast than with More contrast off, where the rules beside allow it:
    /// the selection and the current match raised ([`Self::raised`], [`Self::fit_raised`]), the
    /// current match and the other matches apart from the selection and from one another in
    /// lightness too, the other matches in a colour of their own, fitted by the text rule, and
    /// each word tint lifted from its line in lightness where it stands
    /// nearer than [`WORD_LIGHTNESS_GAP`] ([`Self::lifted_word_tint`]), or stepped beyond its
    /// line where it lies on the canvas side ([`Self::crossed_word_tint`]), which
    /// [`Palette::more_contrast_fit`] decides beside the line tints and hands in `chosen`. Off,
    /// `chosen` is `None`, and none of those rules runs.
    pub fn fit(palette: Palette, roles: SyntaxRoles, chosen: Option<Chosen>) -> Self {
        if let Some(chosen) = chosen {
            return Self::fit_raised(palette, roles, chosen);
        }
        let away = Self::away(palette);
        let lines = Self::lines(palette);
        let patch = Self::patch_colors(palette, roles, &lines);
        let fit = |seed: u32, drawn: &[(u32, &[u32])], lines: &[u32], tint: u32| {
            let fit = HighlightFit::new(drawn, lines);
            fit.search(seed, away, tint)
                .unwrap_or_else(|| fit.readable(seed, away))
        };
        let selection = fit(palette.selected, &patch, &lines, palette.accent);
        let find_match = selection;
        let current = mix(palette.selected, palette.accent, 64);
        let find_current = HighlightFit::new(&patch, &[&lines[..], &[find_match]].concat())
            .search(current, away, palette.accent)
            .unwrap_or_else(|| fit(current, &patch, &lines, palette.accent));
        Self {
            selection,
            find_match,
            find_current,
            added_word: Self::word_tint(palette, roles, palette.added, palette.added_background),
            removed_word: Self::word_tint(
                palette,
                roles,
                palette.removed,
                palette.removed_background,
            ),
            added_rule: None,
            removed_rule: None,
            patch_text: PatchText {
                added: palette.added,
                removed: palette.removed,
                hunk: palette.hunk,
            },
        }
    }

    /// [`Self::fit`] under More contrast. The selection and current match are raised from the
    /// ones `chosen` draws with More contrast off ([`Self::raised`]), or else are those: the
    /// selection from `selected`, and the current match from `selected` a quarter of the way to
    /// `accent`. Each word tint is the one `chosen` carries, the one drawn off lifted from its
    /// line where it stands faintest, or stepped beyond its line where it lies on the canvas side
    /// ([`Self::lifted_word_tint`], [`Self::crossed_word_tint`], [`Palette::more_contrast_fit`]).
    /// Fitted only to the text rule against the raised syntax, the selection and Find's matches
    /// settled lighter, nearer their seeds, and the colors on them read worse than off
    /// (Midnight's strings on the current match 6.18:1 where they read 6.63:1).
    ///
    /// The current match stands from the selection at least [`HIGHLIGHT_DISTANCE`] and
    /// [`HIGHLIGHT_LIGHTNESS_GAP`] in lightness, and at least as far as the off current match
    /// stands from it off wherever a candidate can ([`Self::current_beside`]). Held to its own
    /// distance from the raised
    /// selection, it found no candidate wherever the selection had moved, and settled beside the
    /// other matches (Omarchy alacritty-dark 0.040 to 0.024), or kept its off highlight beside a
    /// selection that had moved toward it (Omarchy ristretto 0.090 to 0.039).
    ///
    /// Find's other matches take a colour of their own, where with More contrast off they share
    /// the selection's: the candidate nearest that one that reads by the text rule as
    /// [`Self::fit`] fits and stands [`HIGHLIGHT_DISTANCE`] and [`HIGHLIGHT_LIGHTNESS_GAP`] from
    /// the selection, the current match and both word tints, and from the current match at least
    /// as far as the current match drawn off stands from them off, searched toward black or white
    /// as it searches and then toward the text, where a light palette's pale highlights have
    /// room left (searched one way only, seven light palettes found none), and then the same only
    /// [`HIGHLIGHT_DISTANCE`] from the current match. Where none fits, they keep the one drawn
    /// off, which reads, never the selection as More contrast draws it; since the one drawn off
    /// is the selection's colour drawn off, beside a selection that keeps that colour, as every
    /// palette's measured does, they still draw the selection's colour (decision, 2026-11-05
    /// evening; no shipped palette reaches it). They are not raised from the one drawn off
    /// as the selection is: in no built-in or Omarchy palette did a raised candidate stand so
    /// apart (none of the 45 [`Palette::more_contrast_fit`]'s tests read, 2026-11-05 evening);
    /// a custom theme in which one does takes the text-rule fit, which drew the same colour as
    /// the raised one in the 4 of 476 synthetic custom palettes where one stood apart (Midnight
    /// and Graphite with their text dimmed toward the canvas, or Graphite's selection taken to
    /// its canvas; 2026-11-05 night). Why: drawn in the selection's colour, they stood 0 from
    /// the selection in every palette, so a match could not be told from selected text
    /// (decision, 2026-11-05).
    fn fit_raised(palette: Palette, roles: SyntaxRoles, chosen: Chosen) -> Self {
        let Chosen {
            palette: chosen,
            off,
            words: [added_word, removed_word],
        } = chosen;
        let own_roles = chosen.syntax_roles(None);
        let (away, lines) = (Self::away(palette), Self::lines(palette));
        let patch = Self::patch_colors(palette, roles, &lines);
        let own: Vec<u32> = Self::patch_colors(chosen, own_roles, &Self::lines(chosen))
            .into_iter()
            .map(|(color, _)| color)
            .collect();
        let raised = |off: u32| Self::raised(&patch, &own, off, &lines);
        // A highlight fitted by the text rule where none is raised, under the colours drawn on
        // it as text under More contrast, `text` and every role on every line (the patch's
        // text is `string`, `removed` and `function`, [`PatchText`]): toward `away`, which
        // raises the text's contrast, and else toward the text, where a light palette's pale
        // highlights have room left.
        let as_text: Vec<(u32, &[u32])> = roles.colors().map(|color| (color, &lines[..])).into();
        let readable = |seed: u32, fit: &dyn Fn(HighlightFit) -> HighlightFit| {
            let fit = fit(HighlightFit::new(&as_text, &lines));
            fit.search(seed, away, palette.accent)
                .or_else(|| fit.search(seed, away ^ 0xffffff, palette.accent))
        };
        let selection = raised(off.selection)
            .raise(palette.selected, away, palette.accent)
            .unwrap_or(off.selection);
        let current = mix(palette.selected, palette.accent, 64);
        let (selection, find_current) = Self::current_beside(
            (off.selection, off.find_current),
            selection,
            |distance| {
                raised(off.find_current)
                    .apart_from(selection, distance)
                    .raise(current, away, palette.accent)
            },
            |selection, distance| readable(current, &|fit| fit.apart_from(selection, distance)),
        );
        // Find's other matches: apart from the selection, the current match, at least as far as
        // the current match drawn off stands from them, and the word tints.
        let from_current =
            oklab_distance(oklab(off.find_current), oklab(off.find_match)).max(HIGHLIGHT_DISTANCE);
        let beside = |fit: HighlightFit, from_current: f64| {
            fit.apart_from(selection, HIGHLIGHT_DISTANCE)
                .apart_from(find_current, from_current)
                .apart_from(added_word, HIGHLIGHT_DISTANCE)
                .apart_from(removed_word, HIGHLIGHT_DISTANCE)
        };
        let find_match = readable(off.find_match, &|fit| beside(fit, from_current))
            .or_else(|| readable(off.find_match, &|fit| beside(fit, HIGHLIGHT_DISTANCE)))
            .unwrap_or(off.find_match);
        Self {
            selection,
            find_match,
            find_current,
            added_word,
            removed_word,
            added_rule: Some(Self::word_rule(palette, palette.added, added_word)),
            removed_rule: Some(Self::word_rule(palette, palette.removed, removed_word)),
            patch_text: PatchText {
                added: roles.string,
                removed: roles.removed,
                hunk: roles.function,
            },
        }
    }

    /// The rule a changed word draws along its highlight box's bottom edge under More contrast:
    /// its side's diff colour, `diff`, as applied, where that reads [`MORE_CONTRAST_EDGE`] on the
    /// word's tint, `word`, otherwise the first of 256 even steps from it toward black in a light
    /// palette or white in a dark one that does, as [`Palette::resting_edge`] is fitted.
    ///
    /// Why a rule: a changed line carries cues beyond colour, its `+` or `-` and Split's filler
    /// rows, and a changed word carried none; in colour-blind models some words nearly vanish
    /// (Solarized Dark's removed words), and WCAG 1.4.1 asks for a cue beyond colour. Why 3:1:
    /// the rule is a graphic, held to the non-text contrast every other edge under More contrast
    /// holds. Why every palette and both sides: marking only some would read as a difference in
    /// meaning.
    pub fn word_rule(palette: Palette, diff: u32, word: u32) -> u32 {
        let extreme = if palette.is_light() {
            0x000000
        } else {
            0xffffff
        };
        toward_extreme_in(256, diff, extreme, |rule| {
            custom::contrast(rule, word) >= MORE_CONTRAST_EDGE
        })
    }

    /// The selection and current match [`Self::fit_raised`] draws: `selection` is raised from
    /// `off_selection`, the selection drawn with More contrast off (or is it, where none is
    /// raised), `beside(distance)` raises the current match from `off_current`, the one drawn
    /// off, to stand at least `distance` and [`HIGHLIGHT_LIGHTNESS_GAP`] in lightness from
    /// `selection`, or is `None` where none does, and `fitted(selection, distance)` fits one by
    /// the text rule, as [`Self::fit`] fits, that stands `distance` and that gap from
    /// `selection`, or is `None`. The current match stands from the selection at least `apart`,
    /// as far as off and at least [`HIGHLIGHT_DISTANCE`], with that gap: raised beside the raised
    /// selection; or the off current match beside it, or beside the off selection, where it
    /// stands so; or else fitted beside the raised selection, or beside the off one, `apart` from
    /// it where a fitted candidate stands so, and where none does, only [`HIGHLIGHT_DISTANCE`]
    /// with the gap. Raised only as far as off where that was less, the current match stood 0.003
    /// from the selection in Sandstone, and 0.044 in One Dark by chroma alone, 0.0001 in
    /// lightness; fitted only [`HIGHLIGHT_DISTANCE`] from it, One Dark's `#104266` stood 0.041
    /// from its selection, nearer than the pair drawn off stand apart through chroma (0.044),
    /// where fitted as far as off it is `#13406d`, 0.045 from it.
    pub fn current_beside(
        (off_selection, off_current): (u32, u32),
        selection: u32,
        beside: impl Fn(f64) -> Option<u32>,
        fitted: impl Fn(u32, f64) -> Option<u32>,
    ) -> (u32, u32) {
        let apart =
            oklab_distance(oklab(off_current), oklab(off_selection)).max(HIGHLIGHT_DISTANCE);
        let stands = |current: u32, selection: u32| {
            let (current, selection) = (oklab(current), oklab(selection));
            oklab_distance(current, selection) >= apart
                && (current[0] - selection[0]).abs() >= HIGHLIGHT_LIGHTNESS_GAP
        };
        if let Some(current) = beside(apart) {
            (selection, current)
        } else if stands(off_current, selection) {
            (selection, off_current)
        } else if stands(off_current, off_selection) {
            (off_selection, off_current)
        } else {
            // As far as off where a fitted candidate stands so, else the floor; beside the raised
            // selection, else the off one (each tried once where they are the same).
            let distances = [apart, HIGHLIGHT_DISTANCE];
            let distances = &distances[..if apart > HIGHLIGHT_DISTANCE { 2 } else { 1 }];
            let selections = [selection, off_selection];
            let selections = &selections[..if selection != off_selection { 2 } else { 1 }];
            distances
                .iter()
                .flat_map(|&distance| selections.iter().map(move |&beside| (beside, distance)))
                .find_map(|(beside, distance)| {
                    fitted(beside, distance).map(|current| (beside, current))
                })
                .unwrap_or((off_selection, off_current))
        }
    }

    /// The fit that raises a highlight under More contrast from `off`, the same highlight fitted
    /// with More contrast off, so it reads and marks no worse: each color in `drawn`, with the
    /// lines it is drawn on, reaches [`MORE_CONTRAST_TEXT`], or its own lowest contrast on those
    /// lines where that is lower ([`drawn_target`]), and at least the contrast its counterpart
    /// in `own` reads on `off`, on a highlight that stands apart from each of `lines` as `off`
    /// does, on the same side, at least as far and with at least its luminance contrast
    /// ([`HighlightFit::at_least_as_apart_as`]).
    /// [`HighlightFit::raise`] finds the candidate nearest a seed that reaches all of that,
    /// searched as [`Self::fit`] searches, or `None`, and the caller keeps `off`.
    pub fn raised(drawn: &[(u32, &[u32])], own: &[u32], off: u32, lines: &[u32]) -> HighlightFit {
        let targets: Vec<_> = drawn
            .iter()
            .zip(own)
            .map(|(&(color, on), &own)| {
                let target =
                    drawn_target(color, on, MORE_CONTRAST_TEXT).max(custom::contrast(own, off));
                (color, target)
            })
            .collect();
        HighlightFit::targeted(&targets, lines).at_least_as_apart_as(off)
    }

    /// The colors drawn on the selection and Find's matches, each with the lines it is drawn on:
    /// `text` and every syntax role on every line of `lines` ([`Self::lines`]), and the patch's
    /// `added`, `removed` and `hunk` on their line tints and the editor background.
    pub fn patch_colors(
        palette: Palette,
        roles: SyntaxRoles,
        lines: &[u32; 4],
    ) -> Vec<(u32, &[u32])> {
        roles
            .colors()
            .map(|color| (color, &lines[..]))
            .into_iter()
            .chain([
                (palette.added, &lines[2..3]),
                (palette.removed, &lines[3..4]),
                (palette.hunk, &lines[..1]),
            ])
            .collect()
    }

    /// The side a highlight moves toward when its seed does not read: black in a dark palette,
    /// white in a light one.
    pub fn away(palette: Palette) -> u32 {
        if palette.is_light() {
            0xffffff
        } else {
            0x000000
        }
    }

    /// The backgrounds the editors draw lines on: the editor background, the active line and
    /// the added and removed line tints.
    pub fn lines(palette: Palette) -> [u32; 4] {
        [
            palette.canvas,
            palette.panel,
            palette.added_background,
            palette.removed_background,
        ]
    }

    /// The colors drawn on the changed-word tint for `diff` on its line tint `line`, each with
    /// the lines it is drawn on: `text` and every syntax role but `removed` on every line, and
    /// `diff` on its own.
    pub fn word_colors<'a>(
        roles: SyntaxRoles,
        lines: &'a [u32; 4],
        diff: u32,
        line: &'a [u32; 1],
    ) -> impl Iterator<Item = (u32, &'a [u32])> {
        roles
            .word_colors()
            .map(|color| (color, &lines[..]))
            .into_iter()
            .chain([(diff, &line[..])])
    }

    /// The seed of the changed-word tint for `diff` on its line tint `line`: the line tint a
    /// quarter of the way to `diff`, the tint origin/main drew.
    pub fn word_seed(diff: u32, line: u32) -> u32 {
        [16, 8, 0].into_iter().fold(0, |seed, shift| {
            let (line, diff) = ((line >> shift) & 0xff, (diff >> shift) & 0xff);
            seed | ((line * 3 + diff) / 4) << shift
        })
    }

    /// The changed-word tint for `diff` on its line tint `line`, fitted as [`Self::fit`]
    /// describes for a palette applied as it is.
    pub fn word_tint(palette: Palette, roles: SyntaxRoles, diff: u32, line: u32) -> u32 {
        let (lines, on_line) = (Self::lines(palette), [line]);
        let drawn: Vec<_> = Self::word_colors(roles, &lines, diff, &on_line).collect();
        let fit = HighlightFit::new(&drawn, &on_line);
        let (seed, away) = (Self::word_seed(diff, line), Self::away(palette));
        fit.search(seed, away, diff)
            .unwrap_or_else(|| fit.readable(seed, away))
    }

    /// Whether the selection and Find's matches, these highlights drawn with More contrast off,
    /// stand from `line`, a moved line tint, at least [`HIGHLIGHT_DISTANCE`] in OKLab, or as far
    /// as from `off_line`, the line drawn off, where that is less. A raised highlight stands from
    /// every line at least as far as the one drawn off ([`HighlightFit::at_least_as_apart_as`]),
    /// and where none is raised the one drawn off stays, so a line held to this keeps every
    /// highlight on it [`HIGHLIGHT_DISTANCE`] apart, as [`Self::fit`] keeps them off. A line
    /// moved toward the canvas, free of it, came within 0.034 of Omarchy miasma's selection,
    /// which stands 0.044 from the line drawn off.
    pub fn stay_apart_from(self, line: u32, off_line: u32) -> bool {
        let (line, off_line) = (oklab(line), oklab(off_line));
        [self.selection, self.find_current]
            .into_iter()
            .all(|highlight| {
                let highlight = oklab(highlight);
                let off_apart = oklab_distance(highlight, off_line);
                oklab_distance(highlight, line) >= HIGHLIGHT_DISTANCE.min(off_apart)
            })
    }

    /// The changed-word tint for `diff` on its line tint `line` under More contrast, for a word
    /// whose tint drawn with More contrast off, `off`, lies beyond `line` from the canvas
    /// ([`Palette::more_contrast_fit`]; a word on the line's canvas side steps across it instead,
    /// [`Self::crossed_word_tint`]): `off` where it stands at least [`WORD_LIGHTNESS_GAP`] from
    /// `line` in OKLab lightness, and otherwise `off` lifted away from `line`, on the side of it
    /// `off` lies, lighter or darker, until it does ([`lightness_lifts`]), or as far toward that
    /// as the colors drawn on it allow: each of `roles`, fitted at [`MORE_CONTRAST_TEXT`], but
    /// `removed`, and `diff` read on the lift at least the contrast their counterparts in
    /// `own`, the roles drawn off, read on `off`, and the lift stands from `line` at least as
    /// far as `off` does, in OKLab and in luminance on the same side, so the word is never
    /// marked more faintly. The lift stops at the step before the first on which some color
    /// reads worse than that, never past it, even where a step beyond would read well again;
    /// where the first step reads worse, `off` stays. A lift that runs out of lightness, where
    /// white or black stands nearer `line` than [`WORD_LIGHTNESS_GAP`], stops at the last step
    /// before it.
    ///
    /// More contrast lifts only the faintest tints because their lightness gap is what
    /// colour-blind vision keeps of them; a tint already that far keeps `off` exactly. A lift
    /// on the side of the line toward the text on it lowers that text's contrast, and `text`
    /// and the diff colors are not raised, so such a tint keeps `off`; one on the far side of
    /// its line from its text lifts, and the text reads better for it. Raised instead toward
    /// 7:1 for every color on it and held no nearer its line in luminance, a tint between its
    /// line and its text could only gain by moving toward the line, so it kept `off` as well.
    /// A word on its line's canvas side once lifted toward the canvas, held at half its line's
    /// gap from it, which left it on that side, short of the order canvas, line, word that the
    /// step across now always restores.
    pub fn lifted_word_tint(
        roles: SyntaxRoles,
        own: SyntaxRoles,
        off: u32,
        diff: u32,
        line: u32,
    ) -> u32 {
        let targets: Vec<_> = roles
            .word_colors()
            .into_iter()
            .zip(own.word_colors())
            .chain([(diff, diff)])
            .map(|(color, own)| (color, custom::contrast(own, off)))
            .collect();
        let reads = HighlightFit::targeted(&targets, &[]);
        lightness_lifts(off, line)
            .into_iter()
            .take_while(|&lift| reads.reading(lift) == Reading::Target)
            .last()
            .unwrap_or(off)
    }

    /// The changed-word tint under More contrast for a word whose tint does not stand beyond its
    /// line tint `line` from the canvas of `chosen`, the palette as chosen ([`stands_beyond`]):
    /// one drawn with More contrast off on the line's canvas side, or one lifted
    /// ([`Self::lifted_word_tint`]) and its line moved short of [`HIGHLIGHT_DISTANCE`] from it.
    /// It is the step beyond `line`
    /// ([`beyond_line`]), in the hue and chroma of the word's seed ([`Self::word_seed`]), else
    /// halfway to its diff color's, else the diff color's own, that
    /// stands at least [`HIGHLIGHT_DISTANCE`] from `line` and [`WORD_LIGHTNESS_GAP`] from it in
    /// OKLab lightness, or where the colors drawn on the word bind first, the last step before
    /// one on which some color would not read that stands apart: [`HIGHLIGHT_DISTANCE`] from
    /// `line` and [`HIGHLIGHT_LIGHTNESS_GAP`] beyond it in lightness. The
    /// colors are those drawn on the word as text under More contrast: `text` and every syntax
    /// role in `roles` but `removed`, as raised on the line, and `patch`, the patch text the Diff
    /// view draws on the word ([`PatchText`]); `diff`, the side's diff color, lends the seed its
    /// hue and is drawn on the word only as its rule, fitted to it afterward
    /// ([`Self::word_rule`]). Each reads [`MORE_CONTRAST_TEXT`], or its own contrast on `line`
    /// where that is lower; where no step that stands apart lets every color do so, each reads
    /// the text rule with its rasterization margin, or its own contrast on `line` where lower,
    /// and then the text rule alone. Where no step that stands so apart reads the text rule,
    /// the last step on which every color does that stands [`HIGHLIGHT_DISTANCE`] from `line`,
    /// beyond it but short of the lightness gap, since the text rule is the floor every color
    /// keeps (One Dark's added word, stepped 0.02 beyond its line, drew the raised keyword
    /// `#e090f7` at 4.39:1, and Kanagawa Lotus's removed word, before its text darkened on
    /// 2026-11-05, drew its strings at 4.49:1). With
    /// no step beyond `line` that both reads the text rule and stands [`HIGHLIGHT_DISTANCE`] from
    /// it, `off`, the word's tint drawn with More contrast off, which reads: the nearest step
    /// that stood apart, or the line's own colour, broke the text floor or drew the word as its
    /// line (decision, 2026-11-05 evening; no shipped palette reaches it).
    ///
    /// Why the seed's hue: the word tint is its line tint a quarter of the way to its diff color,
    /// the tint origin/main drew, and the one drawn off lay on the canvas side because it read
    /// better there. Why it may read worse than off: the text on a word lies on the line's far
    /// side from the canvas, so a word stepped across nears it; the order canvas, line, word is
    /// the decided rule (decision, 2026-11-05), and More contrast's 7:1 is kept wherever the step
    /// allows it. Held also to the diff color, which reads about 4.75:1 on its line by the off
    /// fit, a word stepped across had no room and fell under 4.5:1 (Braden's removed 4.42:1).
    /// Why the lightness gap: held to [`HIGHLIGHT_DISTANCE`] alone, a step stood apart through
    /// its seed's chroma with almost no lightness beyond its line (Solarized Dark's added word
    /// `#2d3500`, 0.0004 beyond), a mark colour-blind vision loses; one just-noticeable
    /// difference beyond, the order canvas, line, word holds in lightness too (decision,
    /// 2026-11-05 afternoon).
    pub fn crossed_word_tint(
        roles: SyntaxRoles,
        [diff, patch]: [u32; 2],
        (line, off): (u32, u32),
        chosen: Palette,
    ) -> u32 {
        let on_line = [line];
        let colors: Vec<u32> = roles.word_colors().into_iter().chain([patch]).collect();
        let seed = Self::word_seed(diff, line);
        // The seed's hue and chroma first, then halfway to the diff color's and the diff
        // color's own, which stand apart from the line with less lightness to spend.
        let rows = [seed, mix(seed, diff, 128), diff].map(|hue| beyond_line(hue, line, chosen));
        let line_lab = oklab(line);
        let from_line = |step: u32| oklab_distance(oklab(step), line_lab) >= HIGHLIGHT_DISTANCE;
        let apart = |step: u32| {
            from_line(step) && lightness_beyond(step, line, chosen) >= HIGHLIGHT_LIGHTNESS_GAP
        };
        // The last step of the first row on which every color reads `rule` that stands `apart`.
        let reading = |rule: f64, apart: &dyn Fn(u32) -> bool| {
            let targets: Vec<_> = colors
                .iter()
                .map(|&color| (color, drawn_target(color, &on_line, rule)))
                .collect();
            let reads = HighlightFit::targeted(&targets, &[]);
            rows.iter().find_map(|steps| {
                steps
                    .iter()
                    .copied()
                    .take_while(|&step| reads.reading(step) == Reading::Target)
                    .filter(|&step| apart(step))
                    .last()
            })
        };
        [MORE_CONTRAST_TEXT, LABEL_RULE + LABEL_MARGIN, LABEL_RULE]
            .into_iter()
            .find_map(|rule| reading(rule, &apart))
            .or_else(|| reading(LABEL_RULE, &from_line))
            .unwrap_or(off)
    }
}

/// Whether `word`, a changed-word tint, stands beyond `line`, its line tint, from the canvas of
/// `chosen` under More contrast: further from the canvas than `line` in OKLab lightness, on the
/// same side of it ([`lightness_beyond`]), and at least [`HIGHLIGHT_DISTANCE`] from `line`. The
/// order canvas, line, word is what makes a changed word read as emphasis of its line.
pub fn stands_beyond(word: u32, line: u32, chosen: Palette) -> bool {
    lightness_beyond(word, line, chosen) > 0.
        && oklab_distance(oklab(word), oklab(line)) >= HIGHLIGHT_DISTANCE
}

/// How far `color` lies beyond `line`, a line tint, from the canvas of `chosen` in OKLab
/// lightness ([`away_from_canvas`]): positive where it stands further from the canvas than
/// `line`, on the same side, and negative on the line's canvas side.
pub fn lightness_beyond(color: u32, line: u32, chosen: Palette) -> f64 {
    let line = oklab(line)[0];
    (oklab(color)[0] - line) * away_from_canvas(line, chosen.canvas, chosen.is_light())
}

/// The direction in OKLab lightness that leads from the canvas past a line of lightness
/// `line`: `1.` where the line is lighter than `canvas`, `-1.` where darker, and where level,
/// toward the text, lighter in a dark palette and darker in a light one (`light`).
fn away_from_canvas(line: f64, canvas: u32, light: bool) -> f64 {
    match line.total_cmp(&oklab(canvas)[0]) {
        std::cmp::Ordering::Greater => 1.,
        std::cmp::Ordering::Less => -1.,
        std::cmp::Ordering::Equal if light => -1.,
        std::cmp::Ordering::Equal => 1.,
    }
}

/// The steps beyond `line` from the canvas of `chosen` ([`away_from_canvas`]), nearest first: OKLab
/// lightness moves from `line`'s a thousandth at a time, in `seed`'s hue and chroma, or where
/// sRGB cannot hold that chroma at that lightness, the most of it sRGB holds, in the same hue.
/// Each step is rounded to sRGB and listed only where it stands further beyond `line` in
/// lightness than the last one listed; the last is the first that stands at least
/// [`HIGHLIGHT_DISTANCE`] from `line` and [`WORD_LIGHTNESS_GAP`] from it in lightness, or the
/// last before lightness runs out.
pub fn beyond_line(seed: u32, line: u32, chosen: Palette) -> Vec<u32> {
    let ([_, a, b], line_lab) = (oklab(seed), oklab(line));
    let direction = away_from_canvas(line_lab[0], chosen.canvas, chosen.is_light());
    let beyond = |lab: [f64; 3]| (lab[0] - line_lab[0]) * direction;
    let (mut steps, mut last) = (Vec::new(), 0.);
    for step in 1..=1000 {
        let lab = [line_lab[0] + direction * f64::from(step) / 1000., a, b];
        if !(0. ..=1.).contains(&lab[0]) {
            break;
        }
        let color = from_linear(linear_in_gamut(lab));
        let color_lab = oklab(color);
        if beyond(color_lab) <= last {
            continue;
        }
        last = beyond(color_lab);
        steps.push(color);
        if last >= WORD_LIGHTNESS_GAP && oklab_distance(color_lab, line_lab) >= HIGHLIGHT_DISTANCE {
            break;
        }
    }
    steps
}

/// What [`EditorHighlights::fit`] fits from under More contrast, from
/// [`Palette::more_contrast_fit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chosen {
    /// The palette as chosen.
    pub(crate) palette: Palette,
    /// Its editor highlights with More contrast off, which the selection and Find's matches are
    /// raised from.
    pub(crate) off: EditorHighlights,
    /// The added and removed changed-word tints under More contrast, decided beside the line
    /// tints.
    pub words: [u32; 2],
}

/// The steps that lift `tint` away from `line` in OKLab lightness, on the side of it `tint`
/// lies, nearest first, until the lightness gap reaches [`WORD_LIGHTNESS_GAP`]: lightness moves
/// a thousandth at a time, keeping `tint`'s hue and chroma, or where sRGB cannot hold that
/// chroma at that lightness, the most of it sRGB holds, in the same hue. Each step is rounded to
/// sRGB and listed only where it stands further from `line` in lightness than the last one
/// listed, and at least as far from it as `tint` in OKLab and in luminance contrast, on `tint`'s
/// side in luminance, so no step marks the word more faintly than `tint` (rounding, or chroma
/// the gamut takes, can leave a step nearer in OKLab than `tint`); the last is the first whose
/// gap reaches [`WORD_LIGHTNESS_GAP`], or, where white or black stands nearer `line` than that,
/// the last step before lightness runs out. Empty where `tint` already stands that far or is
/// level with `line`, and where it lies on one side of `line` in lightness and on the other in
/// luminance, as a saturated red can, since every step away in lightness then nears `line` in
/// luminance or crosses it.
pub fn lightness_lifts(tint: u32, line: u32) -> Vec<u32> {
    use std::cmp::Ordering;
    let ([lightness, a, b], line_lab) = (oklab(tint), oklab(line));
    let gap = |color: u32| (oklab(color)[0] - line_lab[0]).abs();
    let line_luminance = custom::luminance(line);
    let side = |color: u32| custom::luminance(color).total_cmp(&line_luminance);
    let (apart, contrast) = (
        oklab_distance(oklab(tint), line_lab),
        custom::contrast(tint, line),
    );
    let direction = match (lightness.total_cmp(&line_lab[0]), side(tint)) {
        (Ordering::Equal, _) => return Vec::new(),
        (lighter, luminance) if luminance.is_ne() && luminance != lighter => return Vec::new(),
        (Ordering::Greater, _) => 1.,
        (Ordering::Less, _) => -1.,
    };
    let (mut lifts, mut last) = (Vec::new(), tint);
    for step in 1..=1000 {
        if gap(last) >= WORD_LIGHTNESS_GAP {
            break;
        }
        let lab = [lightness + direction * f64::from(step) / 1000., a, b];
        if !(0. ..=1.).contains(&lab[0]) {
            break;
        }
        let lift = from_linear(linear_in_gamut(lab));
        if gap(lift) > gap(last)
            && oklab_distance(oklab(lift), line_lab) >= apart
            && custom::contrast(lift, line) >= contrast
            && (side(tint).is_eq() || side(lift) == side(tint))
        {
            last = lift;
            lifts.push(lift);
        }
    }
    lifts
}

/// The steps that move the line tint `line` away from `word`, the changed-word tint drawn on it,
/// in OKLab lightness, on the line's far side from the word, nearest first, until the word stands
/// [`WORD_LIGHTNESS_GAP`] from it: lightness moves a thousandth at a time, keeping `line`'s hue
/// and chroma, or where sRGB cannot hold that chroma at that lightness, the most of it sRGB
/// holds, in the same hue. Each step is rounded to sRGB and listed only where it stands further
/// from `word` in lightness than the last one listed, and `word` stands from it at least as far
/// as `off_word` stands from `off_line`, the pair drawn with More contrast off, in OKLab and in
/// luminance contrast, on that pair's side in luminance. The steps end before the first that
/// stands nearer `canvas` in lightness than [`WORD_LIGHTNESS_GAP`], or than `line` does where
/// `line` already stands nearer, so such a line never moves toward the canvas, and at the first
/// whose gap reaches [`WORD_LIGHTNESS_GAP`]. Empty where `word` already stands that far or is
/// level with `line`, and where it lies on one side of `line` in lightness and on the other in
/// luminance, since every step away in lightness then nears `word` in luminance or crosses it.
///
/// Why the canvas floor: the line tint is itself the mark of an added or removed line, so it
/// keeps from the editor background the two just-noticeable differences a word keeps from it.
pub fn line_moves(line: u32, word: u32, (off_word, off_line): (u32, u32), canvas: u32) -> Vec<u32> {
    use std::cmp::Ordering;
    let ([lightness, a, b], word_lab) = (oklab(line), oklab(word));
    let gap = |color: u32| (oklab(color)[0] - word_lab[0]).abs();
    let canvas = oklab(canvas)[0];
    let from_canvas = |color: u32| (oklab(color)[0] - canvas).abs();
    let floor = WORD_LIGHTNESS_GAP.min(from_canvas(line));
    let word_luminance = custom::luminance(word);
    let side = |color: u32| word_luminance.total_cmp(&custom::luminance(color));
    let off_side = custom::luminance(off_word).total_cmp(&custom::luminance(off_line));
    let (apart, contrast) = (
        oklab_distance(oklab(off_word), oklab(off_line)),
        custom::contrast(off_word, off_line),
    );
    let direction = match (word_lab[0].total_cmp(&lightness), side(line)) {
        (Ordering::Equal, _) => return Vec::new(),
        (lighter, luminance) if luminance.is_ne() && luminance != lighter => return Vec::new(),
        (Ordering::Greater, _) => -1.,
        (Ordering::Less, _) => 1.,
    };
    let (mut moves, mut last) = (Vec::new(), line);
    for step in 1..=1000 {
        if gap(last) >= WORD_LIGHTNESS_GAP {
            break;
        }
        let lab = [lightness + direction * f64::from(step) / 1000., a, b];
        if !(0. ..=1.).contains(&lab[0]) {
            break;
        }
        let moved = from_linear(linear_in_gamut(lab));
        if from_canvas(moved) < floor {
            break;
        }
        if gap(moved) > gap(last)
            && oklab_distance(word_lab, oklab(moved)) >= apart
            && custom::contrast(word, moved) >= contrast
            && (off_side.is_eq() || side(moved) == off_side)
        {
            last = moved;
            moves.push(moved);
        }
    }
    moves
}

/// The OKLab lightness of `color` as protanopia and deuteranopia see it, in that order: the
/// color through Machado, Oliveira and Fernandes' model (2009, severity 1, on linear sRGB), the
/// model the design reviews measured, rounded to sRGB.
pub fn colour_blind_lightness(color: u32) -> [f64; 2] {
    const MODELS: [[[f64; 3]; 3]; 2] = [
        [
            [0.152_286, 1.052_583, -0.204_868],
            [0.114_503, 0.786_281, 0.099_216],
            [-0.003_882, -0.048_116, 1.051_998],
        ],
        [
            [0.367_322, 0.860_646, -0.227_968],
            [0.280_085, 0.672_501, 0.047_413],
            [-0.011_820, 0.042_940, 0.968_881],
        ],
    ];
    let linear = [16, 8, 0].map(|shift| custom::linear_channel((color >> shift) & 0xff));
    MODELS.map(|model| {
        let seen = from_linear(model.map(|row| {
            let channel: f64 = row.iter().zip(linear).map(|(m, c)| m * c).sum();
            channel.clamp(0., 1.)
        }));
        oklab(seen)[0]
    })
}

/// Whether `word`, a changed-word tint, stands from `line`, a moved line tint, in OKLab
/// lightness at least as far as from `off_line`, the line drawn off, as protanopia and as
/// deuteranopia see the three ([`colour_blind_lightness`]).
///
/// Why: the word's lightness gap is what colour-blind vision keeps of it, so a line moved for
/// it must not narrow that gap there. Moved by normal vision alone, red-ish words, which read
/// darker in a protanopia model than their lines where they read lighter in normal vision, drew
/// nearer their lines there (Omarchy kanagawa's removed word 0.020 to 0.018 from its line,
/// alacritty-dark's 0.016 to 0.013, catppuccin-latte's added word crossing its line).
pub fn keeps_colour_blind_gaps(word: u32, line: u32, off_line: u32) -> bool {
    let [word, line, off_line] = [word, line, off_line].map(colour_blind_lightness);
    (0..2).all(|model| (word[model] - line[model]).abs() >= (word[model] - off_line[model]).abs())
}

/// The linear-light sRGB channels of OKLab coordinates, with their chroma lowered, hue kept,
/// as little as sRGB needs to hold them.
pub fn linear_in_gamut([lightness, a, b]: [f64; 3]) -> [f64; 3] {
    let at = |share: f64| linear_from_oklab_unclipped([lightness, a * share, b * share]);
    let holds = |linear: [f64; 3]| linear.iter().all(|&channel| (0. ..=1.).contains(&channel));
    let mut share = 1.;
    if !holds(at(share)) {
        let (mut held, mut lost) = (0., 1.);
        for _ in 0..24 {
            let middle = (held + lost) / 2.;
            if holds(at(middle)) {
                held = middle;
            } else {
                lost = middle;
            }
        }
        share = held;
    }
    at(share).map(|channel| channel.clamp(0., 1.))
}

/// The contrast a color drawn on a highlight should reach there: `rule`, or the color's own
/// lowest contrast on the lines it is drawn on where that is lower, never below the text rule.
pub fn drawn_target(color: u32, on: &[u32], rule: f64) -> f64 {
    let color = custom::luminance(color);
    on.iter()
        .map(|&line| custom::luminance_contrast(color, custom::luminance(line)))
        .fold(rule, f64::min)
        .max(LABEL_RULE)
}

/// The colors one highlight carries and sits on, measured once for its fitting.
pub struct HighlightFit {
    /// Each color drawn on the highlight: its luminance and the contrast it should reach there,
    /// 4.75:1 or its own lowest contrast on the lines it is drawn on, never below 4.5:1 (a
    /// highlight under More contrast sets its own, [`EditorHighlights::raised`]).
    drawn: Vec<(f64, f64)>,
    /// Each background the highlight sits on, and each highlight drawn beside it
    /// ([`Self::apart_from`]), with how it stands apart from it.
    pub lines: Vec<Apart>,
}

/// How a highlight stands apart from one color: a background it sits on, or another highlight
/// drawn beside it ([`HighlightFit::apart_from`]).
pub struct Apart {
    /// The color's OKLab coordinates.
    pub oklab: [f64; 3],
    /// The color's relative luminance.
    pub luminance: f64,
    /// The distance, in ΔEOK, the highlight stands from it at least: [`HIGHLIGHT_DISTANCE`], or
    /// more for a raised highlight ([`HighlightFit::at_least_as_apart_as`]).
    pub distance: f64,
    /// The gap in OKLab lightness the highlight keeps from it at least: none from a background,
    /// and [`HIGHLIGHT_LIGHTNESS_GAP`] from another highlight drawn beside it under More contrast
    /// ([`HighlightFit::apart_from`]), so that distance never comes from chroma alone.
    pub lightness: f64,
    /// The side of it, in OKLab lightness, the highlight keeps, lighter (`Greater`) or darker
    /// (`Less`), or `None` for either: a raised highlight keeps the side of the one drawn with
    /// More contrast off ([`HighlightFit::at_least_as_apart_as`]).
    pub side: Option<std::cmp::Ordering>,
    /// The relative luminance the highlight keeps beyond, with the side of the color's it lies
    /// on: at least it where that side is lighter (`Greater`), at most it where darker (`Less`),
    /// or `None` for no bound. A raised highlight keeps the luminance of the one drawn with More
    /// contrast off, so its luminance contrast with the color is at least the off one's, on the
    /// same side ([`HighlightFit::at_least_as_apart_as`]).
    pub beyond: Option<(std::cmp::Ordering, f64)>,
}

/// How a candidate highlight reads under the colors drawn on it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reading {
    /// Some color misses the text rule.
    Unreadable,
    /// Every color reaches the text rule, but not its target.
    Rule,
    /// Every color reaches its target.
    Target,
}

impl HighlightFit {
    /// Each color in `drawn` with the lines it is drawn on, to reach the text rule plus the
    /// rasterization margin on the highlight ([`drawn_target`]).
    pub fn new(drawn: &[(u32, &[u32])], lines: &[u32]) -> Self {
        let targets: Vec<_> = drawn
            .iter()
            .map(|&(color, on)| (color, drawn_target(color, on, LABEL_RULE + LABEL_MARGIN)))
            .collect();
        Self::targeted(&targets, lines)
    }

    /// Each color in `drawn` with the contrast it should reach on the highlight.
    pub fn targeted(drawn: &[(u32, f64)], lines: &[u32]) -> Self {
        Self {
            drawn: drawn
                .iter()
                .map(|&(color, target)| (custom::luminance(color), target))
                .collect(),
            lines: lines
                .iter()
                .map(|&line| Apart {
                    oklab: oklab(line),
                    luminance: custom::luminance(line),
                    distance: HIGHLIGHT_DISTANCE,
                    lightness: 0.,
                    side: None,
                    beyond: None,
                })
                .collect(),
        }
    }

    fn reading(&self, color: u32) -> Reading {
        let color = custom::luminance(color);
        self.drawn
            .iter()
            .map(|&(drawn, target)| {
                let contrast = custom::luminance_contrast(drawn, color);
                if contrast >= target {
                    Reading::Target
                } else if contrast >= LABEL_RULE {
                    Reading::Rule
                } else {
                    Reading::Unreadable
                }
            })
            .min()
            .unwrap_or(Reading::Target)
    }

    /// Whether a color of this luminance before rounding to 8 bits could read at the text rule
    /// once rounded: rounding the three channels moves a contrast ratio by under 2%, so a margin
    /// of 3% of the rule keeps every candidate that can.
    fn may_read(&self, luminance: f64) -> bool {
        self.drawn
            .iter()
            .all(|&(drawn, _)| custom::luminance_contrast(drawn, luminance) >= LABEL_RULE * 0.97)
    }

    /// This fit for a highlight that stands apart from each line as `off` does: on the same
    /// side of it in OKLab lightness, lighter or darker, and at least as far from it, never
    /// nearer than [`HIGHLIGHT_DISTANCE`], and with at least `off`'s luminance contrast with it,
    /// on the same side in luminance. Under More contrast a raised highlight never marks its
    /// text more faintly than the highlight drawn with it off, and never with the other polarity:
    /// a changed-word tint that turned darker than its lighter line tint read as a gap in the
    /// line (Midnight's added words, #0f1d19 on #19322d), and a selection that turned near black
    /// lost its hue (Midnight's, #0b3e3e to #000001). Its lightness against the line is the mark
    /// that survives colour-blind vision, so distance gained through saturation alone does not
    /// stand in for it: held only to the distance, every raised word tint took its line's
    /// luminance, 1.00 to 1.01:1, where the ones drawn off stand 1.08 to 1.37:1 from theirs, and
    /// in a protanopia and deuteranopia model Midnight's removed word turned darker than its line
    /// and Porcelain's added word fell from 0.067 to 0.018 from it.
    pub fn at_least_as_apart_as(mut self, off: u32) -> Self {
        let (off, off_luminance) = (oklab(off), custom::luminance(off));
        for apart in &mut self.lines {
            apart.distance = apart.distance.max(oklab_distance(off, apart.oklab));
            apart.side = match off[0].total_cmp(&apart.oklab[0]) {
                std::cmp::Ordering::Equal => None,
                side => Some(side),
            };
            apart.beyond = match off_luminance.total_cmp(&apart.luminance) {
                std::cmp::Ordering::Equal => None,
                side => Some((side, off_luminance)),
            };
        }
        self
    }

    /// This fit for a highlight that also stands at least `distance` from `color`, another
    /// highlight drawn beside it, on either side of it, and at least [`HIGHLIGHT_LIGHTNESS_GAP`]
    /// from it in OKLab lightness: under More contrast, Find's current match from the selection,
    /// and Find's other matches from the selection, the current match and the changed-word tints.
    pub fn apart_from(mut self, color: u32, distance: f64) -> Self {
        self.lines.push(Apart {
            oklab: oklab(color),
            luminance: custom::luminance(color),
            distance,
            lightness: HIGHLIGHT_LIGHTNESS_GAP,
            side: None,
            beyond: None,
        });
        self
    }

    /// The candidate nearest `seed` that reaches every target and stands apart, searched as
    /// [`Self::search`] searches, `tint` lending its hue, or `None` where none does.
    pub fn raise(&self, seed: u32, away: u32, tint: u32) -> Option<u32> {
        self.search(seed, away, tint)
            .filter(|&highlight| self.reading(highlight) == Reading::Target)
    }

    /// Whether `color` stands from every line at least its distance ([`HIGHLIGHT_DISTANCE`]
    /// unless [`Self::at_least_as_apart_as`] raised it) and its lightness gap, on its side where
    /// it keeps one, and beyond the luminance it keeps there, if any.
    fn distinct(&self, color: u32) -> bool {
        let (lab, luminance) = (oklab(color), custom::luminance(color));
        self.lines.iter().all(|apart| {
            oklab_distance(lab, apart.oklab) >= apart.distance
                && (lab[0] - apart.oklab[0]).abs() >= apart.lightness
                && apart
                    .side
                    .is_none_or(|side| lab[0].total_cmp(&apart.oklab[0]) == side)
                && apart.beyond.is_none_or(|(side, bound)| match side {
                    std::cmp::Ordering::Greater => luminance >= bound,
                    _ => luminance <= bound,
                })
        })
    }

    /// `seed` fitted as [`EditorHighlights::fit`] describes, when a candidate both reads and
    /// stands apart; `tint` lends its hue to a seed too grey to have one of its own.
    pub fn search(&self, seed: u32, away: u32, tint: u32) -> Option<u32> {
        let origin = oklab(seed);
        if self.reading(seed) == Reading::Target && self.distinct(seed) {
            return Some(seed);
        }
        // Candidates in order of the distance from the seed each was built at, merged from rows
        // along which that distance grows: the steps toward `away`, by their own distance, and
        // each chroma row of [`chroma_rows`], by its unclipped coordinates. Only candidates up to
        // the first that fits are built. A chroma candidate's luminance before rounding to 8 bits
        // screens it, so only those near enough to read are rounded and measured. The first that
        // reaches its targets and stands apart wins; the first at the rule that stands apart
        // stands in when none does.
        let end = oklab(away)[0];
        let rows = chroma_rows(seed, tint);
        let at = |row: usize, step: u32| match row.checked_sub(1) {
            None => {
                let color = mix(seed, away, step);
                (
                    oklab_distance(oklab(color), origin),
                    Candidate::Color(color),
                )
            }
            Some(chroma) => {
                let lightness = origin[0] + (end - origin[0]) * f64::from(step) / 64.;
                let lab = [lightness, rows[chroma][0], rows[chroma][1]];
                (oklab_distance(lab, origin), Candidate::Lab(lab))
            }
        };
        let next = |row: usize, step: u32| {
            (step <= if row == 0 { 256 } else { 64 }).then(|| {
                let (distance, candidate) = at(row, step);
                Nearest {
                    distance,
                    row,
                    step,
                    candidate,
                }
            })
        };
        let mut heap = (0..=rows.len())
            .filter_map(|row| next(row, 0))
            .collect::<BinaryHeap<_>>();
        let mut fallback = None;
        while let Some(Nearest {
            row,
            step,
            candidate,
            ..
        }) = heap.pop()
        {
            heap.extend(next(row, step + 1));
            let color = match candidate {
                Candidate::Color(color) => color,
                Candidate::Lab(lab) => {
                    let linear = linear_from_oklab(lab);
                    if !self.may_read(linear_luminance(linear)) {
                        continue;
                    }
                    from_linear(linear)
                }
            };
            match self.reading(color) {
                Reading::Target if self.distinct(color) => return Some(color),
                Reading::Rule if fallback.is_none() && self.distinct(color) => {
                    fallback = Some(color);
                }
                _ => {}
            }
        }
        fallback
    }

    /// The first step from `seed` toward `away` that reads at the text rule, for a palette where
    /// no candidate also stands apart.
    fn readable(&self, seed: u32, away: u32) -> u32 {
        toward_away(seed, away)
            .find(|&color| self.reading(color) != Reading::Unreadable)
            .unwrap_or(away)
    }
}

/// `seed` and each 256th of the way from it to `away`.
fn toward_away(seed: u32, away: u32) -> impl Iterator<Item = u32> {
    (0..=256).map(move |step| mix(seed, away, step))
}

/// Below this OKLab chroma a color is treated as grey, with no hue of its own to keep.
pub const GREY_CHROMA: f64 = 0.01;

/// A highlight candidate: an sRGB color, or OKLab coordinates the search rounds to one, clipped
/// to sRGB, only when it reaches them.
enum Candidate {
    Color(u32),
    Lab([f64; 3]),
}

/// A candidate on the search's heap, which pops the nearest to the seed first (then the
/// earliest row and step, so the order is deterministic).
struct Nearest {
    distance: f64,
    row: usize,
    step: u32,
    candidate: Candidate,
}

impl Nearest {
    fn key(&self) -> (f64, usize, u32) {
        (self.distance, self.row, self.step)
    }
}

impl PartialEq for Nearest {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Nearest {}

impl PartialOrd for Nearest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nearest {
    /// Reversed, so the max-heap pops the nearest.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let ((d, row, step), (other_d, other_row, other_step)) = (self.key(), other.key());
        other_d
            .total_cmp(&d)
            .then(other_row.cmp(&row))
            .then(other_step.cmp(&step))
    }
}

/// The opponent axes of each chroma row a highlight that cannot stand apart by lightness alone
/// searches: `seed`'s chroma raised by 0.01 at a time up to 0.1 more, in `seed`'s hue and in
/// `tint`'s (a grey has no hue; with neither, blue). Along a row the lightness moves from
/// `seed`'s toward `away`'s in 64ths; coordinates outside sRGB are clipped when converted, and the
/// fitting measures the clipped color.
fn chroma_rows(seed: u32, tint: u32) -> Vec<[f64; 2]> {
    let [_, a, b] = oklab(seed);
    let chroma = a.hypot(b);
    let hue = |color: u32| {
        let [_, a, b] = oklab(color);
        let chroma = a.hypot(b);
        (chroma >= GREY_CHROMA).then(|| [a / chroma, b / chroma])
    };
    let mut hues = [hue(seed), hue(tint)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    hues.dedup();
    if hues.is_empty() {
        hues.push([-0.105, -0.995]);
    }
    hues.into_iter()
        .flat_map(|hue| {
            (1..=10).map(move |raise| {
                let chroma = chroma + f64::from(raise) * 0.01;
                [chroma * hue[0], chroma * hue[1]]
            })
        })
        .collect()
}

/// OKLab coordinates (lightness and the two opponent axes) of an `0xrrggbb` color.
pub fn oklab(color: u32) -> [f64; 3] {
    let [r, g, b] = [16, 8, 0].map(|shift| custom::linear_channel((color >> shift) & 0xff));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// The linear-light sRGB channels of OKLab coordinates, each clipped to 0..=1.
fn linear_from_oklab(lab: [f64; 3]) -> [f64; 3] {
    linear_from_oklab_unclipped(lab).map(|channel| channel.clamp(0., 1.))
}

/// The linear-light sRGB channels of OKLab coordinates, outside 0..=1 where sRGB cannot show
/// them.
fn linear_from_oklab_unclipped([lightness, a, b]: [f64; 3]) -> [f64; 3] {
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ]
}

/// The WCAG relative luminance of linear-light sRGB channels.
fn linear_luminance([r, g, b]: [f64; 3]) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The `0xrrggbb` color whose channels are nearest linear-light values.
pub fn from_linear(linear: [f64; 3]) -> u32 {
    linear
        .into_iter()
        .zip([16, 8, 0])
        .fold(0, |color, (channel, shift)| {
            color | custom::encode_linear(channel) << shift
        })
}

/// ΔEOK: the Euclidean distance between two OKLab colors.
pub fn oklab_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}
