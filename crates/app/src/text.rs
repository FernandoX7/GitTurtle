use crate::appearance::palette;
use gpui_kit::{
    App, AppContext, Entity, FontWeight, HighlightStyle, Window,
    component::input::{EditorState, TextDecoration},
    rgb,
};
use std::{mem::size_of, ops::Range, sync::Arc};

/// Immutable patch metadata prepared by the repository worker and shared with
/// the editor/gutter. Keeping the literal patch separate preserves copy/search.
pub struct PatchPresentation {
    pub(crate) rows: Arc<[crate::diff_view::LineNumbers]>,
    pub(crate) column_width: f32,
    pub(crate) change_rows: Arc<[usize]>,
    ranges: Arc<[DiffRange]>,
}

impl PatchPresentation {
    pub fn prepare(patch: &str) -> Self {
        let (rows, column_width) = crate::diff_view::prepare_gutter(patch);
        let ranges = review_ranges(patch, &rows);
        let mut previous_changed = false;
        let change_rows = rows
            .iter()
            .zip(patch.split('\n'))
            .enumerate()
            .filter_map(|(row, (numbers, line))| {
                let changed = numbers.old.is_some() != numbers.new.is_some();
                let start = changed && !previous_changed;
                // A no-newline marker must not divide a replacement.
                if !line.starts_with('\\') {
                    previous_changed = changed;
                }
                start.then_some(row)
            })
            .collect::<Vec<_>>()
            .into();
        Self {
            rows,
            column_width,
            change_rows,
            ranges: ranges.into(),
        }
    }

    /// Retained metadata allocations, including the outer Arc payload and the
    /// three Arc reference-count headers; allocator bookkeeping is excluded.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + std::mem::size_of_val(self.rows.as_ref())
            + std::mem::size_of_val(self.ranges.as_ref())
            + std::mem::size_of_val(self.change_rows.as_ref())
            + 4 * 2 * size_of::<usize>()
    }
}

/// The toolkit's name for its plain-text language, which has no grammar.
const PLAIN_TEXT: &str = "text";
/// The toolkit's name for its unified-diff grammar.
const DIFF: &str = "diff";

/// Build only the currently requested editor. Decorations change presentation,
/// while the editor receives one owned copy of the unmodified source text. A patch's
/// presentation is prepared off the UI thread by its caller's worker (`worker::text_content`
/// for Compare, recovery and rewrite; the pull request review's file preparation for its
/// Source patch), so this never scans the patch; a `"diff"` request without one draws plain
/// text with no colors.
pub fn editor(
    value: &str,
    language: &str,
    diff: Option<&PatchPresentation>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<EditorState> {
    editor_with_decorations(value, language, diff, window, cx).0
}

/// A patch is drawn as plain text, so its decorations are the only colors on it. A grammar's
/// captures would compete with them for the same text, and GPUI's highlight fold leaves either
/// color, by hash order rather than precedence.
pub fn editor_with_decorations(
    value: &str,
    language: &str,
    diff: Option<&PatchPresentation>,
    window: &mut Window,
    cx: &mut App,
) -> (
    Entity<EditorState>,
    Option<crate::editor_find::PatchDecorations>,
) {
    let language = if diff.is_some() || language == DIFF {
        PLAIN_TEXT
    } else {
        language
    };
    let editor = cx.new(|cx| {
        EditorState::new(window, cx)
            .language(language.to_owned())
            .line_number(diff.is_none())
            .folding(diff.is_none())
            .soft_wrap(false)
            .default_value(value.to_owned())
    });
    crate::editor_find::reserve_highlight_layer(&editor, cx);
    let collection = diff.map(|presentation| decorate(&editor, presentation, cx));
    (editor, collection)
}

/// Decorate a patch editor built without a presentation, once its caller has prepared one:
/// the pull request review's Source patch may open before its file's preparation finishes, and
/// then takes these decorations and `diff_view::new`'s gutter in place of the editor's own line
/// numbers. Keep the handle for the editor's life: Find removes the backgrounds under its
/// matches through it, and [`refresh_theme`] redecorates it.
pub fn decorate(
    editor: &Entity<EditorState>,
    presentation: &PatchPresentation,
    cx: &mut App,
) -> crate::editor_find::PatchDecorations {
    let decorations = theme_decorations(presentation, cx);
    crate::editor_find::patch_decorations(editor, decorations, cx)
}

/// Keep the native editor identity, find session, focus, selection, and scroll
/// while a local filesystem change supplies new prepared text.
pub fn refresh_editor(
    editor: &Entity<EditorState>,
    value: &str,
    decorations: Option<(&crate::editor_find::PatchDecorations, &PatchPresentation)>,
    window: &mut Window,
    cx: &mut App,
) {
    editor.update(cx, |state, cx| {
        let selection = state.selected_range();
        let offset = state.scroll_offset();
        state.set_value(value.to_owned(), window, cx);
        state.set_selected_range(selection, cx);
        state.set_scroll_offset(offset, cx);
    });
    if let Some((collection, presentation)) = decorations {
        refresh_theme(collection, presentation, cx);
    }
}

/// Preserve the logical viewport when its font changes. Editor entities,
/// selections and Find sessions remain intact; only deferred scroll geometry
/// changes to match the next layout's line/character dimensions.
pub fn rescale_editor(editor: &Entity<EditorState>, ratio: f32, cx: &mut App) {
    editor.update(cx, |state, cx| {
        state.rescale_scroll_offset(ratio, cx);
    });
}

pub fn refresh_theme(
    collection: &crate::editor_find::PatchDecorations,
    presentation: &PatchPresentation,
    cx: &mut App,
) {
    collection.set(theme_decorations(presentation, cx), cx);
}

/// The patch's decorations in the applied palette. Under More contrast a changed word also draws
/// a rule along its highlight box's bottom edge (`EditorHighlights::added_rule`), on a decoration
/// of its own over the word's range. Under Find's matches the patch layer splits a decoration
/// around each match, copying it whole and clearing one property on the pieces under it
/// (`editor_find`'s `split_under_matches`): where Find draws its matches with backgrounds, the
/// background (`without_match_backgrounds`), so the rule stays under the other matches and the
/// current match's underline, on Find's earlier collection, hides it; on an editor past the ones
/// that keep a Find collection of their own, where Find underlines every match without a
/// background, the rule (`without_match_rules`), so the patch keeps its backgrounds and no match
/// draws two strokes. Each run of adjacent changed lines of one side, a removed or an added
/// block, also fills its lines' whole rows in its line tint
/// (`TextDecoration::with_row_background`), on one decoration of its own without a background,
/// which Find keeps whole, so a hunk's changed lines read as one block with a straight right
/// edge and a blank changed line is tinted across its row, not under its `-` or `+` alone
/// (decision, 2026-11-05). Off, there is one decoration a prepared range and one a block. The
/// patch's text takes the fitted highlights' colours (`EditorHighlights::patch_text`): the
/// palette's diff and hunk colours off, and under More contrast the syntax roles fitted from
/// them, as Split draws a patch's lines.
fn theme_decorations(presentation: &PatchPresentation, cx: &App) -> Vec<TextDecoration> {
    let palette = palette(cx);
    let highlights = crate::appearance::editor_highlights(cx);
    // A range's line tint, which a block of adjacent ranges in one tint shares.
    let line_tint = |kind: Kind| match kind {
        Kind::Added | Kind::AddedWord => Some(palette.added_background),
        Kind::Removed | Kind::RemovedWord => Some(palette.removed_background),
        Kind::Hunk | Kind::Header => None,
    };
    // One decoration a range, one a block and one a changed word's rule, counted first so the
    // list is allocated once.
    let (mut extra, mut end) = (0, None);
    for decoration in presentation.ranges.iter() {
        let tint = line_tint(decoration.kind);
        extra +=
            usize::from(tint.is_some() && end != tint.map(|tint| (tint, decoration.range.start)))
                + usize::from(match decoration.kind {
                    Kind::AddedWord => highlights.added_rule.is_some(),
                    Kind::RemovedWord => highlights.removed_rule.is_some(),
                    _ => false,
                });
        end = tint.map(|tint| (tint, decoration.range.end));
    }
    let mut decorations = Vec::with_capacity(presentation.ranges.len() + extra);
    // The block of changed lines being gathered: its line tint and its bytes, line breaks
    // included, as `diff_ranges` gives them.
    let mut block: Option<(u32, Range<usize>)> = None;
    let fill = |block: Option<(u32, Range<usize>)>, decorations: &mut Vec<TextDecoration>| {
        if let Some((tint, range)) = block {
            decorations.push(
                TextDecoration::new(range, HighlightStyle::default())
                    .with_row_background(rgb(tint).into()),
            );
        }
    };
    for decoration in presentation.ranges.iter() {
        let tint = line_tint(decoration.kind);
        match (&mut block, tint) {
            (Some((current, range)), Some(tint))
                if *current == tint && range.end == decoration.range.start =>
            {
                range.end = decoration.range.end
            }
            _ => fill(
                std::mem::replace(
                    &mut block,
                    tint.map(|tint| (tint, decoration.range.clone())),
                ),
                &mut decorations,
            ),
        }
        let rule = match decoration.kind {
            Kind::AddedWord => highlights.added_rule,
            Kind::RemovedWord => highlights.removed_rule,
            _ => None,
        };
        let style = match decoration.kind {
            Kind::Added | Kind::AddedWord => HighlightStyle {
                color: Some(rgb(highlights.patch_text.added).into()),
                background_color: Some(
                    rgb(if decoration.kind == Kind::AddedWord {
                        highlights.added_word
                    } else {
                        palette.added_background
                    })
                    .into(),
                ),
                ..Default::default()
            },
            Kind::Removed | Kind::RemovedWord => HighlightStyle {
                color: Some(rgb(highlights.patch_text.removed).into()),
                background_color: Some(
                    rgb(if decoration.kind == Kind::RemovedWord {
                        highlights.removed_word
                    } else {
                        palette.removed_background
                    })
                    .into(),
                ),
                ..Default::default()
            },
            Kind::Hunk => HighlightStyle {
                color: Some(rgb(highlights.patch_text.hunk).into()),
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
            // No color of its own: a file header draws in the editor foreground, `text`,
            // which follows a theme change in the patch views that never redecorate.
            Kind::Header => HighlightStyle {
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
        };
        decorations.push(TextDecoration::new(decoration.range.clone(), style));
        if let Some(rule) = rule {
            decorations.push(
                TextDecoration::new(decoration.range.clone(), HighlightStyle::default())
                    .with_bottom_rule(rgb(rule).into()),
            );
        }
    }
    fill(block, &mut decorations);
    decorations
}

/// A changed word's highlight box as painted, its tint, and the rule colour, where it is one
/// asked for, of each quad inside its bottom edge ([`painted_word_rules`]).
#[cfg(test)]
pub(crate) type PaintedWord = (
    gpui_kit::Bounds<gpui_kit::ScaledPixels>,
    u32,
    Vec<Option<u32>>,
);

/// What the last frame painted for each changed word, read for the rule under More contrast:
/// each highlight box painted in one of the word `tints`, with its tint and the colour, where it
/// is one of `rules`, of each quad laid inside its bottom edge, a 1 px stroke tall on its device
/// pixels and as wide as the box; and how many quads of that height in one of `rules` lie along
/// no word's box, as under a whole changed line or a hunk header.
#[cfg(test)]
pub(crate) fn painted_word_rules(
    window: &Window,
    tints: &[u32],
    rules: &[u32],
) -> (Vec<PaintedWord>, usize) {
    let fill = |color: u32| gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(color)));
    let painted = |quad: &gpui_kit::Quad, colors: &[u32]| {
        colors
            .iter()
            .copied()
            .find(|&color| quad.background == fill(color))
    };
    let quads = window.painted_quads();
    // A 1 px stroke on its device pixels, halves toward zero, as GPUI snaps one.
    let stroke = (window.scale_factor() - 0.5).ceil().max(1.);
    let words = quads
        .iter()
        .filter_map(|quad| Some((quad.bounds, painted(quad, tints)?)))
        .collect::<Vec<_>>();
    let along = |word: &gpui_kit::Bounds<gpui_kit::ScaledPixels>, quad: &gpui_kit::Quad| {
        (
            quad.bounds.left(),
            quad.bounds.right(),
            quad.bounds.bottom(),
        ) == (word.left(), word.right(), word.bottom())
            && quad.bounds.size.height.0 == stroke
    };
    let stray = quads
        .iter()
        .filter(|quad| {
            quad.bounds.size.height.0 == stroke
                && painted(quad, rules).is_some()
                && !words.iter().any(|(word, _)| along(word, quad))
        })
        .count();
    let words = words
        .iter()
        .map(|&(word, tint)| {
            let rules = quads
                .iter()
                .filter(|quad| along(&word, quad))
                .map(|quad| painted(quad, rules))
                .collect();
            (word, tint, rules)
        })
        .collect();
    (words, stray)
}

/// A dense hunk for a test of the changed-word rule, its old source, new source and patch: four
/// short changed words a side on adjacent lines, a whole added line, a hunk header and unchanged
/// text.
#[cfg(test)]
pub(crate) const DENSE_HUNK: [&str; 3] = [
    "keep(a, b);\nlet x = old + one;\ncall(red, blue, 7);\n}\n",
    "keep(a, b);\nlet y = new + one;\ncall(green, blue, 8);\nwhole_added_line();\n}\n",
    "@@ -1,4 +1,5 @@ fn dense() {\n keep(a, b);\n-let x = old + one;\n-call(red, blue, 7);\n+let y = new + one;\n+call(green, blue, 8);\n+whole_added_line();\n }\n",
];

/// A hunk for a test of the line tint's extent, its old source, new source and patch: after an
/// unchanged line, a short changed line with a changed word, a blank changed line and a changed
/// line wider than any test view on each side, then a short added line and a blank one, so
/// Split's Before draws two filler rows beside them, and an unchanged line.
#[cfg(test)]
pub(crate) fn row_tint_hunk() -> [String; 3] {
    let long = |word: &str| format!("let long = [{}{word}];", "item, ".repeat(60));
    let (old, new) = (long("old"), long("new"));
    [
        format!("keep(a, b);\ncall(red, 7);\n\n{old}\ntail();\n"),
        format!("keep(a, b);\ncall(green, 7);\n\n{new}\nadded();\n\ntail();\n"),
        format!(
            "@@ -1,5 +1,7 @@ fn rows() {{\n keep(a, b);\n-call(red, 7);\n-\n-{old}\n+call(green, 7);\n+\n+{new}\n+added();\n+\n tail();\n"
        ),
    ]
}

/// What one editor row draws in a line tint, for [`assert_row_tints`].
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum RowTint {
    /// No line tint: an unchanged line, a hunk header or a filler row.
    Untinted,
    /// The tint from the text's left edge to the editor's visible right edge, the line's text
    /// ending inside the view.
    ToEdge(u32),
    /// The tint from the text's left edge to the editor's visible right edge or past it, the
    /// line's text running past the view.
    PastEdge(u32),
}

/// Assert what the last frame painted in the line `tints` across `editor`'s first rows, each as
/// `rows` expects it: each quad in a line tint starts and ends on row boundaries, and every row
/// it spans, one row or the adjacent rows the editor fills in one quad, is a listed row that
/// expects its tint; a tinted row carries a quad in its tint over its whole height from the
/// text's left edge, where the editor lays out each line's first glyph and a line's tint has
/// always started, to the editor's visible right edge (past it where the line runs past the
/// view), and no quad in its tint reaches further where the line ends inside the view; an
/// untinted row overlaps no quad in either tint. Only quads painting inside the editor count,
/// not Split's other side. Edges compare within half a device pixel, as GPUI rounds a quad's
/// edges to device pixels.
#[cfg(test)]
pub(crate) fn assert_row_tints(
    at: &str,
    window: &Window,
    editor: &EditorState,
    tints: [u32; 2],
    rows: &[RowTint],
) {
    let scale = window.scale_factor();
    let text = editor.text_bounds().expect("the editor is laid out");
    let line_height = editor
        .line_height()
        .expect("the editor is laid out")
        .as_f32()
        * scale;
    let left = (text.origin.x + gpui_kit::base::input::LINE_NUMBER_RIGHT_MARGIN).as_f32() * scale;
    let bounds = editor.input_bounds();
    let (start, right) = (
        bounds.left().as_f32() * scale,
        bounds.right().as_f32() * scale,
    );
    let (above, below) = (
        bounds.top().as_f32() * scale,
        bounds.bottom().as_f32() * scale,
    );
    let text_top = text.origin.y.as_f32() * scale;
    let fill = |color: u32| gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(color)));
    let near = |a: f32, b: f32| (a - b).abs() <= 0.51;
    let quads = window.painted_quads();
    // Every quad's and every row's failure, so one frame reports each thing it draws wrongly.
    let mut failures = Vec::new();
    // The row boundary an edge lies on, counted from the first row's top, if it lies on one.
    let boundary = |y: f32| {
        let row = ((y - text_top) / line_height).round();
        near(y, text_top + line_height * row).then_some(row as i64)
    };
    // Each quad in a line tint painting inside this editor (not beside it, as Split's other
    // side): from a row boundary to a later one, over listed rows that expect its tint.
    for quad in &quads {
        let Some(tint) = tints
            .iter()
            .copied()
            .find(|&tint| quad.background == fill(tint))
        else {
            continue;
        };
        let painted = quad.bounds.intersect(&quad.content_mask.bounds);
        if !(painted.top().0 < below - 0.51
            && painted.bottom().0 > above + 0.51
            && painted.left().0 < right - 0.51
            && painted.right().0 > start + 0.51)
        {
            continue;
        }
        let expects = |row: i64| {
            usize::try_from(row)
                .ok()
                .and_then(|row| rows.get(row))
                .is_some_and(|want| match *want {
                    RowTint::Untinted => false,
                    RowTint::ToEdge(want) | RowTint::PastEdge(want) => want == tint,
                })
        };
        match (
            boundary(quad.bounds.top().0),
            boundary(quad.bounds.bottom().0),
        ) {
            (Some(first), Some(end)) if first < end => {
                let strays = (first..end)
                    .filter(|&row| !expects(row))
                    .collect::<Vec<_>>();
                if !strays.is_empty() {
                    failures.push(format!(
                        "#{tint:06x}'s quad {:?} over rows {first} to {} spans rows {strays:?}, \
                         which are not listed rows expecting that tint",
                        quad.bounds,
                        end - 1
                    ));
                }
            }
            _ => failures.push(format!(
                "#{tint:06x}'s quad {:?} starts and ends on the boundaries of rows {line_height} \
                 tall from {text_top}",
                quad.bounds
            )),
        }
    }
    for (row, want) in rows.iter().enumerate() {
        let top = text_top + line_height * row as f32;
        let bottom = top + line_height;
        // Each quad in a line tint that paints over this row inside this editor (not beside
        // it, as Split's other side): its tint, its horizontal edges and whether it covers the
        // row's whole height.
        let drawn = quads
            .iter()
            .filter(|quad| {
                let painted = quad.bounds.intersect(&quad.content_mask.bounds);
                painted.top().0 < bottom - 0.51
                    && painted.bottom().0 > top + 0.51
                    && painted.left().0 < right - 0.51
                    && painted.right().0 > start + 0.51
            })
            .filter_map(|quad| {
                let tint = tints
                    .iter()
                    .copied()
                    .find(|&tint| quad.background == fill(tint))?;
                // Over the row's whole height, alone or with the rows of the run it starts or
                // ends, which the editor fills in one quad.
                let whole =
                    quad.bounds.top().0 <= top + 0.51 && quad.bounds.bottom().0 >= bottom - 0.51;
                Some((tint, quad.bounds.left().0, quad.bounds.right().0, whole))
            })
            .collect::<Vec<_>>();
        let (tint, past) = match *want {
            RowTint::Untinted => {
                if !drawn.is_empty() {
                    failures.push(format!("row {row} draws no line tint: {drawn:?}"));
                }
                continue;
            }
            RowTint::ToEdge(tint) => (tint, false),
            RowTint::PastEdge(tint) => (tint, true),
        };
        let mine = drawn
            .iter()
            .filter(|drawn| drawn.0 == tint)
            .collect::<Vec<_>>();
        if !mine.iter().any(|&&(_, from, to, whole)| {
            whole
                && near(from, left)
                && if past {
                    to >= right - 0.51
                } else {
                    near(to, right)
                }
        }) {
            failures.push(format!(
                "row {row} carries #{tint:06x} over its height from the text's left edge {left} \
                 to the editor's visible right edge {right}: {drawn:?}"
            ));
        }
        if !past && mine.iter().any(|&&(_, _, to, _)| to > right + 0.51) {
            failures.push(format!(
                "row {row}'s tint ends at the editor's visible right edge {right}: {drawn:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{at}:\n{}", failures.join("\n"));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Added,
    Removed,
    Hunk,
    AddedWord,
    RemovedWord,
    /// The `---` and `+++` file-header lines before the first hunk.
    Header,
}

#[derive(Debug, PartialEq, Eq)]
struct DiffRange {
    range: Range<usize>,
    kind: Kind,
}

/// One shared work allowance per file, never quadratic in unbounded line size.
/// Beyond the allowance we retain ordinary whole-line highlighting.
pub(crate) struct WordBudget {
    remaining: usize,
}

impl Default for WordBudget {
    fn default() -> Self {
        Self {
            remaining: 1_000_000,
        }
    }
}

pub(crate) fn word_changes(
    old: &str,
    new: &str,
    budget: &mut WordBudget,
) -> [Vec<Range<usize>>; 2] {
    if old.len().max(new.len()) > 16 * 1024 || budget.remaining == 0 {
        return Default::default();
    }
    fn tokens(text: &str) -> Option<Vec<Range<usize>>> {
        let mut result: Vec<Range<usize>> = Vec::new();
        let mut previous = None;
        for (offset, ch) in text.char_indices() {
            let kind = if ch.is_alphanumeric() || ch == '_' {
                0
            } else if ch.is_whitespace() {
                1
            } else {
                2
            };
            if previous == Some(kind) && kind != 2 {
                result.last_mut().unwrap().end = offset + ch.len_utf8();
            } else {
                if result.len() == 256 {
                    return None;
                }
                result.push(offset..offset + ch.len_utf8());
            }
            previous = Some(kind);
        }
        Some(result)
    }
    let (Some(a), Some(b)) = (tokens(old), tokens(new)) else {
        return Default::default();
    };
    let cells = (a.len() + 1) * (b.len() + 1);
    if cells > budget.remaining {
        budget.remaining = 0;
        return Default::default();
    }
    budget.remaining -= cells;
    let width = b.len() + 1;
    let mut lcs = vec![0u16; cells];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * width + j] = if old[a[i].clone()] == new[b[j].clone()] {
                1 + lcs[(i + 1) * width + j + 1]
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let mut result: [Vec<Range<usize>>; 2] = Default::default();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && old[a[i].clone()] == new[b[j].clone()] {
            i += 1;
            j += 1;
            continue;
        }
        let side = usize::from(
            i == a.len() || (j < b.len() && lcs[i * width + j + 1] > lcs[(i + 1) * width + j]),
        );
        let range = if side == 0 {
            let r = a[i].clone();
            i += 1;
            r
        } else {
            let r = b[j].clone();
            j += 1;
            r
        };
        if let Some(last) = result[side].last_mut()
            && last.end == range.start
        {
            last.end = range.end;
        } else {
            result[side].push(range);
        }
    }
    result
}

/// Styles are disjoint: the pinned editor does not give overlapping decoration
/// collections a stable precedence. Find can therefore mask these backgrounds.
fn review_ranges(patch: &str, rows: &[crate::diff_view::LineNumbers]) -> Vec<DiffRange> {
    let lines = patch.split_inclusive('\n').collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        offsets.push(offset);
        offset += line.len();
    }
    let mut words: Vec<DiffRange> = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut budget = WordBudget::default();
    let flush = |removed: &mut Vec<usize>,
                 added: &mut Vec<usize>,
                 words: &mut Vec<DiffRange>,
                 budget: &mut WordBudget| {
        for (&old, &new) in removed.iter().zip(added.iter()) {
            let ranges = word_changes(&lines[old][1..], &lines[new][1..], budget);
            for (side, ranges) in ranges.into_iter().enumerate() {
                let row = [old, new][side];
                for range in ranges {
                    words.push(DiffRange {
                        range: (offsets[row] + 1 + range.start)..(offsets[row] + 1 + range.end),
                        kind: if side == 0 {
                            Kind::RemovedWord
                        } else {
                            Kind::AddedWord
                        },
                    });
                }
            }
        }
        removed.clear();
        added.clear();
    };
    for (i, line) in lines.iter().enumerate() {
        let numbers = rows[i];
        match (numbers.old, numbers.new) {
            (Some(_), None) => removed.push(i),
            (None, Some(_)) => added.push(i),
            _ if !line.starts_with('\\') => {
                flush(&mut removed, &mut added, &mut words, &mut budget)
            }
            _ => {}
        }
    }
    flush(&mut removed, &mut added, &mut words, &mut budget);
    words.sort_unstable_by_key(|range| range.range.start);
    let mut result = Vec::new();
    let mut words = words.into_iter().peekable();
    for base in diff_ranges(patch) {
        let mut start = base.range.start;
        while let Some(word) = words.peek() {
            if word.range.start >= base.range.end {
                break;
            }
            let word = words.next().unwrap();
            if word.range.start < start {
                continue;
            }
            if start < word.range.start {
                result.push(DiffRange {
                    range: start..word.range.start,
                    kind: base.kind,
                });
            }
            start = word.range.end;
            result.push(word);
        }
        if start < base.range.end {
            result.push(DiffRange {
                range: start..base.range.end,
                kind: base.kind,
            });
        }
    }
    result
}

/// Track unified hunk counts so `---` / `+++` file headers are not marked as
/// changes, while real changed source beginning with those characters is.
/// The headers before the first hunk are marked as headers; other lines outside
/// hunks stay neutral. Byte offsets include existing line endings and are valid
/// UTF-8 boundaries.
fn diff_ranges(value: &str) -> Vec<DiffRange> {
    let mut result: Vec<DiffRange> = Vec::new();
    let mut remaining: Option<(usize, usize)> = None;
    let mut offset = 0;
    let first_hunk = value
        .split_inclusive('\n')
        .try_fold(0, |start, line| match hunk_counts(line) {
            Some(_) => Err(start),
            None => Ok(start + line.len()),
        })
        .err();
    for line in value.split_inclusive('\n') {
        let kind = if let Some(counts) = hunk_counts(line) {
            remaining = Some(counts);
            Some(Kind::Hunk)
        } else if let Some((old, new)) = remaining.as_mut() {
            match line.as_bytes().first() {
                Some(b'+') if *new > 0 => {
                    *new -= 1;
                    Some(Kind::Added)
                }
                Some(b'-') if *old > 0 => {
                    *old -= 1;
                    Some(Kind::Removed)
                }
                Some(b' ') if *old > 0 && *new > 0 => {
                    *old -= 1;
                    *new -= 1;
                    None
                }
                Some(b'\\') => None, // "No newline at end of file" marker.
                _ => {
                    remaining = None;
                    None
                }
            }
        } else if first_hunk.is_some_and(|first| offset < first)
            && (line.starts_with("--- ") || line.starts_with("+++ "))
        {
            Some(Kind::Header)
        } else {
            None
        };
        if remaining == Some((0, 0)) {
            remaining = None;
        }
        let end = offset + line.len();
        if let Some(kind) = kind {
            if let Some(previous) = result.last_mut()
                && previous.kind == kind
                && previous.range.end == offset
            {
                previous.range.end = end;
            } else {
                result.push(DiffRange {
                    range: offset..end,
                    kind,
                });
            }
        }
        offset = end;
    }
    result
}

fn hunk_counts(line: &str) -> Option<(usize, usize)> {
    if !line.starts_with("@@ ") {
        return None;
    }
    let mut fields = line.split_ascii_whitespace();
    if fields.next()? != "@@" {
        return None;
    }
    let old = range_count(fields.next()?.strip_prefix('-')?)?;
    let new = range_count(fields.next()?.strip_prefix('+')?)?;
    if fields.next()? != "@@" {
        return None;
    }
    Some((old, new))
}

fn range_count(range: &str) -> Option<usize> {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    start.parse::<usize>().ok()?;
    count.parse().ok()
}

/// A prepared text comparison whose unified patch has additions, removals and context, as
/// the worker hands it to Compare, recovery and rewrite review.
#[cfg(test)]
pub fn decorated_patch_content() -> crate::worker::Content {
    let patch = "diff --git a/notes.txt b/notes.txt\n--- a/notes.txt\n+++ b/notes.txt\n\
                 @@ -1,3 +1,3 @@\n context\n-old line\n+new line\n tail\n"
        .to_owned();
    let old = "context\nold line\ntail\n".to_owned();
    let new = "context\nnew line\ntail\n".to_owned();
    let presentation = Arc::new(PatchPresentation::prepare(&patch));
    let split = Arc::new(crate::split_diff::SplitPresentation::prepare(
        &old,
        &new,
        &presentation,
    ));
    crate::worker::Content::Text {
        diagrams: None,
        markdown: None,
        patch,
        old,
        new,
        presentation,
        split,
        partial: None,
        partial_unavailable: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_highlights_isolate_multiple_unicode_tokens_and_keep_find_ranges_disjoint() {
        let old = "let café = 🐢 + old_value;\r\n";
        let new = "let café = 🦀 + new_value;\r\n";
        let spans = word_changes(old, new, &mut WordBudget::default());
        assert_eq!(
            spans[0].iter().map(|r| &old[r.clone()]).collect::<Vec<_>>(),
            ["🐢", "old_value"]
        );
        assert_eq!(
            spans[1].iter().map(|r| &new[r.clone()]).collect::<Vec<_>>(),
            ["🦀", "new_value"]
        );
        let patch = format!("@@ -1 +1 @@\n-{old}+{new}");
        let presentation = PatchPresentation::prepare(&patch);
        assert!(
            presentation
                .ranges
                .windows(2)
                .all(|pair| pair[0].range.end <= pair[1].range.start)
        );
        assert!(
            presentation
                .ranges
                .iter()
                .all(|r| patch.is_char_boundary(r.range.start)
                    && patch.is_char_boundary(r.range.end))
        );
        assert_eq!(
            presentation
                .ranges
                .iter()
                .filter(|r| matches!(r.kind, Kind::AddedWord | Kind::RemovedWord))
                .count(),
            4
        );
    }

    #[test]
    fn word_work_is_bounded_for_long_lines_and_many_tokens() {
        let long = "🐢".repeat(5000);
        assert_eq!(
            word_changes(&long, "short", &mut WordBudget::default()),
            [Vec::<Range<usize>>::new(), Vec::<Range<usize>>::new()]
        );
        let tokens = "a + ".repeat(300);
        assert_eq!(
            word_changes(&tokens, "short", &mut WordBudget::default()),
            [Vec::<Range<usize>>::new(), Vec::<Range<usize>>::new()]
        );
        let mut budget = WordBudget { remaining: 2 };
        assert_eq!(
            word_changes("many words", "more words", &mut budget),
            [Vec::<Range<usize>>::new(), Vec::<Range<usize>>::new()]
        );
        assert_eq!(budget.remaining, 0);
    }

    #[test]
    fn navigation_has_separate_hunks_but_no_newline_markers_do_not_split_replacements() {
        let p = PatchPresentation::prepare(
            "@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n@@ -8 +8 @@\n-last\n+next\n",
        );
        assert_eq!(p.change_rows.as_ref(), &[1, 6]);
    }

    #[test]
    fn prepared_metadata_keeps_utf8_rows_ranges_and_worker_safe_ownership() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PatchPresentation>();
        let patch = "@@ -9999 +10000 @@\r\n-héllo 🐢\r\n+你好\r\n";
        let presentation = PatchPresentation::prepare(patch);
        assert_eq!(presentation.rows.len(), 4);
        assert_eq!(presentation.rows[1].old, Some(9999));
        assert_eq!(presentation.rows[1].new, None);
        assert_eq!(presentation.rows[2].old, None);
        assert_eq!(presentation.rows[2].new, Some(10000));
        assert_eq!(presentation.rows[3].old, None);
        assert_eq!(presentation.rows[3].new, None);
        assert!(
            presentation
                .ranges
                .iter()
                .all(|range| patch.is_char_boundary(range.range.start)
                    && patch.is_char_boundary(range.range.end))
        );
        assert_eq!(presentation.change_rows.as_ref(), &[1]);
        assert!(presentation.column_width > PatchPresentation::prepare("").column_width);

        // Extra neutral rows require storage even though they add no highlights.
        let empty = PatchPresentation::prepare("");
        let neutral = PatchPresentation::prepare(&"\n".repeat(128));
        assert_eq!(
            neutral.retained_bytes() - empty.retained_bytes(),
            128 * size_of::<crate::diff_view::LineNumbers>()
        );
    }

    fn decorated(value: &str) -> Vec<(Kind, &str)> {
        diff_ranges(value)
            .into_iter()
            .map(|range| {
                assert!(value.is_char_boundary(range.range.start));
                assert!(value.is_char_boundary(range.range.end));
                (range.kind, &value[range.range])
            })
            .collect()
    }

    #[test]
    fn context_that_looks_like_a_hunk_keeps_following_changes() {
        let patch = "@@ -1,2 +1,2 @@\n @@ -99 +100 @@\n-old\n+new\n";
        assert_eq!(
            decorated(patch),
            vec![
                (Kind::Hunk, "@@ -1,2 +1,2 @@\n"),
                (Kind::Removed, "-old\n"),
                (Kind::Added, "+new\n")
            ]
        );
    }

    #[test]
    fn utf8_changes_group_without_modifying_patch_or_line_endings() {
        let patch = "diff --git a/你好 b/你好\r\n--- a/你好\r\n+++ b/你好\r\n@@ -1,2 +1,2 @@ function\r\n-héllo 🐢\r\n-adiós\r\n+你好\r\n+再见";
        assert_eq!(
            decorated(patch),
            vec![
                (Kind::Header, "--- a/你好\r\n+++ b/你好\r\n"),
                (Kind::Hunk, "@@ -1,2 +1,2 @@ function\r\n"),
                (Kind::Removed, "-héllo 🐢\r\n-adiós\r\n"),
                (Kind::Added, "+你好\r\n+再见"),
            ]
        );
    }

    #[test]
    fn file_headers_are_headers_but_similar_changed_content_is_colored() {
        let patch = "--- a/file\n+++ b/file\n@@ -1,2 +1,2 @@\n--- removed code\n unchanged\n+++ added code\n--- a/next\n+++ b/next\n";
        // Only the headers before the first hunk are headers; later ones stay neutral.
        assert_eq!(
            decorated(patch),
            vec![
                (Kind::Header, "--- a/file\n+++ b/file\n"),
                (Kind::Hunk, "@@ -1,2 +1,2 @@\n"),
                (Kind::Removed, "--- removed code\n"),
                (Kind::Added, "+++ added code\n"),
            ]
        );
    }

    #[test]
    fn empty_sides_multiple_hunks_and_no_newline_markers_keep_exact_ranges() {
        let patch = "@@ -0,0 +1,2 @@\n+first\n+\n@@ -4 +6 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
        assert_eq!(
            decorated(patch),
            vec![
                (Kind::Hunk, "@@ -0,0 +1,2 @@\n"),
                (Kind::Added, "+first\n+\n"),
                (Kind::Hunk, "@@ -4 +6 @@\n"),
                (Kind::Removed, "-old\n"),
                (Kind::Added, "+new\n"),
            ]
        );
        assert!(diff_ranges("+++ file\n--- file\n+not a hunk\n@@ broken @@\n").is_empty());
    }

    /// What the unified patch editor draws on each line under every built-in palette: the
    /// syntax styles of the language the editor asks its highlighter for, in the applied
    /// highlight theme, with the patch decorations composed over them as the kit's input
    /// element composes them (`combine_highlights`, semantic styles first and decorations
    /// last), and where neither sets a color, the editor foreground (the theme's `foreground`).
    /// On origin/main the editor asked for the diff grammar: file headers drew its keyword and
    /// string colors, and hunk headers and changed lines the decoration's color or the
    /// grammar's, as GPUI's unordered fold left them (the themes spec, "Diff and syntax
    /// colors"). File headers also draw in each palette's text under decorations prepared in
    /// another, as the recovery and rewrite patches, which never redecorate, keep theirs.
    #[gpui_kit::test]
    fn patch_headers_and_changes_draw_palette_colors_in_every_built_in(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::appearance::ThemeChoice;
        use gpui_kit::{
            Context, IntoElement, ParentElement, Render, Styled,
            base::input::InputHighlighterFactory,
            combine_highlights,
            component::{
                Rope, Theme,
                highlighter::SyntaxHighlighter,
                input::{Editor, EditorState},
            },
            div, px,
        };
        use std::{cell::RefCell, rc::Rc};

        struct Probe([Entity<EditorState>; 2]);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(480.)).h(px(400.)).children(
                    self.0
                        .iter()
                        .map(|editor| Editor::new(editor).w_full().h(px(200.))),
                )
            }
        }

        let patch = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@ fn main() {\n fn main() {\n-    let old = \"value\";\n+    let new = \"value\";\n }\n@@ -10,2 +10,2 @@\n-a\n+b\n@@ -20 +20 @@\n-c\n+d\n";
        let presentation = PatchPresentation::prepare(patch);
        cx.update(gpui_kit::init);
        // The language each editor's highlighter is built for, as the editor asks: the
        // decorated patch, and the same patch asked for as "diff" without a presentation.
        let asked = [(); 2].map(|_| Rc::new(RefCell::new(Vec::<String>::new())));
        let factory = |asked: &Rc<RefCell<Vec<String>>>| -> InputHighlighterFactory {
            let record = asked.clone();
            Rc::new(move |language: &str| {
                record.borrow_mut().push(language.to_owned());
                None
            })
        };
        let factories = [factory(&asked[0]), factory(&asked[1])];
        let (_, cx) = cx.add_window_view(|window, cx| {
            let decorated = editor(patch, "diff", Some(&presentation), window, cx);
            let (plain, decorations) = editor_with_decorations(patch, "diff", None, window, cx);
            assert!(
                decorations.is_none(),
                "an unprepared patch has no decorations"
            );
            let [first, second] = factories;
            decorated.update(cx, |state, cx| state.set_highlighter_factory(first, cx));
            plain.update(cx, |state, cx| state.set_highlighter_factory(second, cx));
            Probe([decorated, plain])
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let language = asked[0]
            .borrow()
            .first()
            .cloned()
            .expect("the editor asked");
        let hex =
            |color: Option<gpui_kit::Hsla>| color.map(|color| serde_json::to_value(color).unwrap());

        // Decorations prepared in the first built-in and never refreshed.
        let kept = cx.update(|_, cx| {
            ThemeChoice::ALL[0].apply(None, cx);
            theme_decorations(&presentation, cx)
        });
        cx.update(|_, cx| {
            for choice in ThemeChoice::ALL {
                choice.apply(None, cx);
                let palette = palette(cx);
                let theme = Theme::global(cx).highlight_theme.clone();
                let foreground = Some(Theme::global(cx).foreground);
                // The contrast test reads the same highlight theme without an application.
                assert_eq!(
                    theme,
                    palette.configured_theme(choice.is_light()).highlight_theme,
                    "{choice:?}"
                );
                let mut highlighter = SyntaxHighlighter::new(&language);
                highlighter.update(None, &Rope::from(patch), None);
                let syntax = highlighter.styles(&(0..patch.len()), theme.as_ref());
                let styles = combine_highlights(Vec::new(), syntax).collect::<Vec<_>>();
                let compose = |decorations: &[TextDecoration]| {
                    combine_highlights(
                        styles.clone(),
                        decorations
                            .iter()
                            .map(|decoration| (decoration.range.clone(), decoration.style)),
                    )
                    .collect::<Vec<_>>()
                };
                let drawn = compose(&theme_decorations(&presentation, cx));
                let stale = compose(&kept);

                let color = |token: u32| Some(rgb(token).into());
                let mut start = 0;
                for line in patch.split_inclusive('\n') {
                    let range = start..start + line.len();
                    start = range.end;
                    let header = line.starts_with("--- a/") || line.starts_with("+++ b/");
                    let expected = if line.starts_with("@@") {
                        // Hunk lines draw on the editor background (canvas).
                        (color(palette.hunk), Some(FontWeight::MEDIUM), true)
                    } else if header {
                        (color(palette.text), Some(FontWeight::MEDIUM), true)
                    } else if line.starts_with('-') {
                        (color(palette.removed), None, false)
                    } else if line.starts_with('+') {
                        (color(palette.added), None, false)
                    } else {
                        (color(palette.text), None, true)
                    };
                    let segments = drawn.iter().map(|segment| (segment, "prepared now"));
                    let segments = segments.chain(
                        stale
                            .iter()
                            .filter(|_| header)
                            .map(|segment| (segment, "prepared in the first built-in")),
                    );
                    for ((segment, style), decorations) in segments {
                        if segment.end <= range.start || segment.start >= range.end {
                            continue;
                        }
                        let (want_color, weight, no_background) = expected;
                        let drawn_color = style.color.or(foreground);
                        assert!(
                            drawn_color == want_color
                                && style.font_weight == weight
                                && (!no_background || style.background_color.is_none()),
                            "{choice:?} draws {:?} of {:?} in {:?} at {:?} on {:?} under \
                             decorations {decorations}, wanted {:?} at {weight:?}",
                            &patch[segment.start.max(range.start)..segment.end.min(range.end)],
                            line.trim_end(),
                            hex(drawn_color),
                            style.font_weight,
                            hex(style.background_color),
                            hex(want_color),
                        );
                    }
                }
            }
        });
        // Both patches are drawn as the kit's plain text, which has no grammar to color them.
        assert_eq!(language, "text");
        let unprepared = asked[1].borrow().first().cloned();
        assert_eq!(unprepared.as_deref(), Some("text"));
    }

    /// Under More contrast every changed word in the Diff view draws a rule inside its highlight
    /// box's bottom edge, a 1 px stroke as wide as the box, in its side's fitted colour, at 13
    /// and 18 pt, in a dark palette, one whose removed words nearly vanish in colour-blind
    /// models, and a light one; no other quad of a rule's colour and height is painted, so a
    /// whole changed line, the hunk header and unchanged text draw none. Off, no word draws one,
    /// and the decorations are one a prepared range and one filling each changed block's rows,
    /// none with a rule, as before the rule existed.
    #[gpui_kit::test]
    fn more_contrast_rules_each_changed_word_in_the_diff_view(cx: &mut gpui_kit::TestAppContext) {
        use crate::appearance::{
            ThemeChoice, apply_text_sizes, editor_highlights, set_more_contrast,
        };
        use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, div, px};

        struct Root(
            Entity<crate::diff_view::DiffView>,
            crate::editor_find::PatchDecorations,
        );
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(900.)).h(px(600.)).child(self.0.clone())
            }
        }

        let patch = DENSE_HUNK[2];
        let presentation = PatchPresentation::prepare(patch);
        let words = presentation
            .ranges
            .iter()
            .filter(|range| matches!(range.kind, Kind::AddedWord | Kind::RemovedWord))
            .count();
        assert_eq!(words, 8, "four changed words a side");
        cx.update(gpui_kit::init);
        let (root, cx) = cx.add_window_view(|window, cx| {
            let (editor, decorations) =
                editor_with_decorations(patch, "diff", Some(&presentation), window, cx);
            let view = crate::diff_view::new(editor, &presentation, window, cx);
            Root(view, decorations.expect("a prepared patch is decorated"))
        });
        for choice in [
            ThemeChoice::Midnight,
            ThemeChoice::SolarizedDark,
            ThemeChoice::Daylight,
        ] {
            for code in [13, 18] {
                for more in [true, false] {
                    let at = format!("{choice:?} at {code} pt, More contrast {more}");
                    let (highlights, decorations) = cx.update(|window, cx| {
                        set_more_contrast(more, cx);
                        choice.apply(Some(&mut *window), cx);
                        apply_text_sizes(13, code, window, cx);
                        let decorations = root.read(cx).1.clone();
                        refresh_theme(&decorations, &presentation, cx);
                        (editor_highlights(cx), theme_decorations(&presentation, cx))
                    });
                    cx.update(|window, cx| window.draw(cx).clear(cx));
                    let tints = [highlights.added_word, highlights.removed_word];
                    let rules = [highlights.added_rule, highlights.removed_rule];
                    let (drawn, stray) = cx.update(|window, _| {
                        painted_word_rules(
                            window,
                            &tints,
                            &rules.iter().flatten().copied().collect::<Vec<_>>(),
                        )
                    });
                    assert_eq!(drawn.len(), words, "{at}: a box a changed word");
                    assert_eq!(stray, 0, "{at}: a rule off every changed word's box");
                    for (bounds, tint, along) in drawn {
                        let rule = rules[usize::from(tint != highlights.added_word)];
                        let want = rule.map(|rule| vec![Some(rule)]).unwrap_or_default();
                        assert_eq!(
                            along, want,
                            "{at}: the word box {bounds:?} in #{tint:06x} draws {want:?} inside its bottom edge"
                        );
                    }
                    if !more {
                        assert_eq!(rules, [None, None], "{at}");
                        // One a prepared range, and one filling each changed block's rows.
                        assert_eq!(decorations.len(), presentation.ranges.len() + 2, "{at}");
                        assert!(
                            decorations
                                .iter()
                                .all(|decoration| decoration.bottom_rule.is_none()),
                            "{at}: no decoration has a rule"
                        );
                    }
                }
            }
        }
    }

    /// Under More contrast, Find over changed words: a changed word's rule stays under Find's
    /// other matches, which take the word's tint, so a matched changed word still reads as
    /// changed, and gives way under its current match, whose accent underline would run about
    /// 2.6 px above it. "re" matches the removed "red" and the added "green".
    #[gpui_kit::test]
    fn find_hides_a_changed_word_s_rule_only_under_its_current_match(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::appearance::{
            ThemeChoice, apply_text_sizes, editor_highlights, set_more_contrast,
        };
        use gpui_kit::{Context, Focusable, IntoElement, ParentElement, Render, Styled, div, px};

        /// The view, and the decorations' handle kept for the window's life, as Compare keeps it.
        struct Root {
            view: Entity<crate::diff_view::DiffView>,
            _decorations: crate::editor_find::PatchDecorations,
        }
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(900.)).h(px(600.)).child(self.view.clone())
            }
        }

        let patch = DENSE_HUNK[2];
        let presentation = PatchPresentation::prepare(patch);
        cx.update(gpui_kit::init);
        let mut patch_editor = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            set_more_contrast(true, cx);
            ThemeChoice::Midnight.apply(Some(&mut *window), cx);
            apply_text_sizes(13, 13, window, cx);
            let (editor, decorations) =
                editor_with_decorations(patch, "diff", Some(&presentation), window, cx);
            patch_editor = Some(editor.clone());
            Root {
                view: crate::diff_view::new(editor, &presentation, window, cx),
                _decorations: decorations.expect("a prepared patch is decorated"),
            }
        });
        let editor = patch_editor.expect("the patch editor");
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let focus = cx.update(|_, cx| editor.focus_handle(cx));
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.dispatch_action(gpui_kit::component::input::Search);
        cx.simulate_input("re");
        cx.run_until_parked();
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        let highlights = cx.update(|_, cx| editor_highlights(cx));
        let fill = |color: u32| gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(color)));
        let rules = [highlights.added_rule, highlights.removed_rule]
            .map(|rule| fill(rule.expect("More contrast is on")));
        let (quads, stroke) = cx.update(|window, _| {
            let stroke = (window.scale_factor() - 0.5).ceil().max(1.);
            (window.painted_quads(), stroke)
        });
        let painted = |color: u32| {
            quads
                .iter()
                .filter(|quad| quad.background == fill(color))
                .map(|quad| quad.bounds)
                .collect::<Vec<_>>()
        };
        let (current, others) = (
            painted(highlights.find_current),
            painted(highlights.find_match),
        );
        assert_eq!(
            (current.len(), others.len()),
            (1, 1),
            "one current match and one other: {current:?}, {others:?}"
        );
        let ruled = quads
            .iter()
            .filter(|quad| quad.bounds.size.height.0 == stroke && rules.contains(&quad.background))
            .map(|quad| quad.bounds)
            .collect::<Vec<_>>();
        let (current, other) = (current[0], others[0]);
        assert!(
            ruled.iter().any(|rule| rule.bottom() == other.bottom()
                && rule.left() <= other.left()
                && rule.right() >= other.right()),
            "the other match {other:?} keeps its changed word's rule: {ruled:?}"
        );
        assert!(
            !ruled.iter().any(|rule| rule.bottom() == current.bottom()
                && rule.left() < current.right()
                && rule.right() > current.left()),
            "the current match {current:?} hides its changed word's rule: {ruled:?}"
        );
    }

    /// A changed line's tint spans the editor's whole row in the Diff view, so a hunk's changed
    /// lines read as one block with a straight right edge (decision, 2026-11-05): a short changed
    /// line, a blank one, whose tint lay under its `-` or `+` alone, and one wider than the view
    /// carry their tint from the text's left edge, where it always started, to the editor's
    /// visible right edge, also scrolled to the far right, where the long lines' text ends short
    /// of that edge; at 13 and 18 pt, More contrast off and on, in Midnight and in Tokyo Night,
    /// whose More contrast moves its added line tint. The `---` and `+++` file header lines, the
    /// hunk header and unchanged lines draw no line tint, and each changed word drawn keeps its
    /// box, with its rule under More contrast.
    #[gpui_kit::test]
    fn a_changed_line_s_tint_spans_its_row_in_the_diff_view(cx: &mut gpui_kit::TestAppContext) {
        use crate::appearance::{
            ThemeChoice, apply_text_sizes, editor_highlights, set_more_contrast,
        };
        use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, div, point, px};

        struct Root(
            Entity<crate::diff_view::DiffView>,
            crate::editor_find::PatchDecorations,
        );
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(900.)).h(px(600.)).child(self.0.clone())
            }
        }

        // The file header lines before the hunk read as body text, untinted.
        let patch = format!("--- a/x\n+++ b/x\n{}", row_tint_hunk()[2]);
        let presentation = PatchPresentation::prepare(&patch);
        cx.update(gpui_kit::init);
        let mut patch_editor = None;
        let (root, cx) = cx.add_window_view(|window, cx| {
            let (editor, decorations) =
                editor_with_decorations(&patch, "diff", Some(&presentation), window, cx);
            patch_editor = Some(editor.clone());
            let view = crate::diff_view::new(editor, &presentation, window, cx);
            Root(view, decorations.expect("a prepared patch is decorated"))
        });
        let editor = patch_editor.expect("the patch editor");
        let mut moved = false;
        for choice in [ThemeChoice::Midnight, ThemeChoice::TokyoNight] {
            let chosen = cx.update(|window, cx| {
                set_more_contrast(false, cx);
                choice.apply(Some(&mut *window), cx);
                palette(cx).added_background
            });
            for code in [13, 18] {
                for more in [false, true] {
                    for far_right in [false, true] {
                        let at = format!(
                            "{choice:?} at {code} pt, More contrast {more}, scrolled to the {}",
                            if far_right { "far right" } else { "start" }
                        );
                        let (colors, highlights) = cx.update(|window, cx| {
                            set_more_contrast(more, cx);
                            choice.apply(Some(&mut *window), cx);
                            apply_text_sizes(13, code, window, cx);
                            let decorations = root.read(cx).1.clone();
                            refresh_theme(&decorations, &presentation, cx);
                            (palette(cx), editor_highlights(cx))
                        });
                        moved |= colors.added_background != chosen;
                        let x = if far_right { -100_000. } else { 0. };
                        for _ in 0..2 {
                            cx.update(|window, cx| {
                                editor.update(cx, |state, cx| {
                                    state.set_scroll_offset(point(px(x), px(0.)), cx)
                                });
                                window.draw(cx).clear(cx);
                            });
                        }
                        let (removed, added) = (colors.removed_background, colors.added_background);
                        // Scrolled to the far right, the long lines end short of the edge.
                        let long = |tint| {
                            if far_right {
                                RowTint::ToEdge(tint)
                            } else {
                                RowTint::PastEdge(tint)
                            }
                        };
                        let rows = [
                            RowTint::Untinted,
                            RowTint::Untinted,
                            RowTint::Untinted,
                            RowTint::Untinted,
                            RowTint::ToEdge(removed),
                            RowTint::ToEdge(removed),
                            long(removed),
                            RowTint::ToEdge(added),
                            RowTint::ToEdge(added),
                            long(added),
                            RowTint::ToEdge(added),
                            RowTint::ToEdge(added),
                            RowTint::Untinted,
                        ];
                        cx.update(|window, cx| {
                            assert_row_tints(&at, window, editor.read(cx), [removed, added], &rows)
                        });
                        let tints = [highlights.added_word, highlights.removed_word];
                        let rules = [highlights.added_rule, highlights.removed_rule];
                        let (drawn, stray) = cx.update(|window, _| {
                            painted_word_rules(
                                window,
                                &tints,
                                &rules.iter().flatten().copied().collect::<Vec<_>>(),
                            )
                        });
                        assert_eq!(stray, 0, "{at}: a rule off every changed word's box");
                        for (bounds, tint, along) in drawn {
                            let rule = rules[usize::from(tint != highlights.added_word)];
                            let want = rule.map(|rule| vec![Some(rule)]).unwrap_or_default();
                            assert_eq!(along, want, "{at}: the word box {bounds:?}'s rule");
                        }
                    }
                }
            }
        }
        assert!(moved, "More contrast moves Tokyo Night's added line tint");
    }

    /// With no reserved Find layer, as on an editor made past `editor_find`'s retained bound,
    /// Find underlines every match and draws no background, on a collection made after the
    /// patch's, whose underlines the kit lets hide no rule: under More contrast a matched
    /// changed word's rule gives way under the match, so two strokes never run one above the
    /// other, and stays along the rest of the word and under every unmatched changed word,
    /// each word keeping its tint. "re" matches the removed "red" and the added "green".
    #[gpui_kit::test]
    fn find_without_a_reserved_layer_hides_every_matched_word_s_rule(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::appearance::{
            ThemeChoice, apply_text_sizes, editor_highlights, set_more_contrast,
        };
        use gpui_kit::{Context, Focusable, IntoElement, ParentElement, Render, Styled, div, px};

        /// The view, the decorations' handle kept for the window's life, as Compare keeps it,
        /// and the editors that took every reserved Find layer first.
        struct Root {
            view: Entity<crate::diff_view::DiffView>,
            _decorations: crate::editor_find::PatchDecorations,
            _reserving: Vec<Entity<EditorState>>,
        }
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(900.)).h(px(600.)).child(self.view.clone())
            }
        }

        let patch = DENSE_HUNK[2];
        let presentation = PatchPresentation::prepare(patch);
        cx.update(gpui_kit::init);
        let mut patch_editor = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            set_more_contrast(true, cx);
            ThemeChoice::Midnight.apply(Some(&mut *window), cx);
            apply_text_sizes(13, 13, window, cx);
            let reserving = (0..64)
                .map(|_| {
                    let editor = cx.new(|cx| EditorState::new(window, cx));
                    crate::editor_find::reserve_highlight_layer(&editor, cx);
                    editor
                })
                .collect();
            let (editor, decorations) =
                editor_with_decorations(patch, "diff", Some(&presentation), window, cx);
            patch_editor = Some(editor.clone());
            Root {
                view: crate::diff_view::new(editor, &presentation, window, cx),
                _decorations: decorations.expect("a prepared patch is decorated"),
                _reserving: reserving,
            }
        });
        let editor = patch_editor.expect("the patch editor");
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let focus = cx.update(|_, cx| editor.focus_handle(cx));
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.dispatch_action(gpui_kit::component::input::Search);
        cx.simulate_input("re");
        cx.run_until_parked();
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        let highlights = cx.update(|_, cx| editor_highlights(cx));
        let tints = [highlights.added_word, highlights.removed_word];
        let rules = [highlights.added_rule, highlights.removed_rule]
            .map(|rule| rule.expect("More contrast is on"));
        let (drawn, stray) = cx.update(|window, _| painted_word_rules(window, &tints, &rules));
        let backgrounds = cx.update(|window, _| {
            let fill = |color: u32| gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(color)));
            window
                .painted_quads()
                .iter()
                .filter(|quad| {
                    [highlights.find_match, highlights.find_current]
                        .iter()
                        .any(|&color| quad.background == fill(color))
                })
                .count()
        });
        assert_eq!(backgrounds, 0, "Find draws no background without its layer");
        assert_eq!(
            drawn.len(),
            8,
            "every changed word keeps its tint: {drawn:?}"
        );
        let whole = drawn
            .iter()
            .filter(|(_, _, along)| !along.is_empty())
            .count();
        assert_eq!(
            whole, 6,
            "the six unmatched changed words keep their whole rule: {drawn:?}"
        );
        assert_eq!(
            stray, 3,
            "\"red\" keeps its rule under \"d\" and \"green\" under \"g\" and \"en\", none under \
             a match: {drawn:?}"
        );
    }
}
