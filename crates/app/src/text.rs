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
/// the pull request review's Source patch keeps the editor's own line numbers and may open
/// before its file's preparation finishes. Keep the handle for the editor's life: Find removes
/// the backgrounds under its matches through it, and [`refresh_theme`] redecorates it.
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

fn theme_decorations(presentation: &PatchPresentation, cx: &App) -> Vec<TextDecoration> {
    let palette = palette(cx);
    presentation
        .ranges
        .iter()
        .map(|decoration| {
            let style = match decoration.kind {
                Kind::Added | Kind::AddedWord => HighlightStyle {
                    color: Some(rgb(palette.added).into()),
                    background_color: Some(
                        rgb(if decoration.kind == Kind::AddedWord {
                            strong_tint(palette.added_background, palette.added)
                        } else {
                            palette.added_background
                        })
                        .into(),
                    ),
                    ..Default::default()
                },
                Kind::Removed | Kind::RemovedWord => HighlightStyle {
                    color: Some(rgb(palette.removed).into()),
                    background_color: Some(
                        rgb(if decoration.kind == Kind::RemovedWord {
                            strong_tint(palette.removed_background, palette.removed)
                        } else {
                            palette.removed_background
                        })
                        .into(),
                    ),
                    ..Default::default()
                },
                Kind::Hunk => HighlightStyle {
                    color: Some(rgb(palette.hunk).into()),
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
            TextDecoration::new(decoration.range.clone(), style)
        })
        .collect()
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

pub(crate) fn strong_tint(background: u32, foreground: u32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |value, shift| {
        let channel = (((background >> shift) & 255) * 3 + ((foreground >> shift) & 255)) / 4;
        value | channel << shift
    })
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
}
