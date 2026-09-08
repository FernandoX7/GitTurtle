use crate::appearance::palette;
use gpui_kit::{
    App, AppContext, Entity, FontWeight, HighlightStyle, Window,
    component::input::{EditorState, TextDecoration, TextDecorationCollection},
    rgb,
};
use std::{mem::size_of, ops::Range, sync::Arc};

/// Immutable patch metadata prepared by the repository worker and shared with
/// the editor/gutter. Keeping the literal patch separate preserves copy/search.
pub struct PatchPresentation {
    pub(crate) rows: Arc<[crate::diff_view::LineNumbers]>,
    pub(crate) column_width: f32,
    ranges: Arc<[DiffRange]>,
}

impl PatchPresentation {
    pub fn prepare(patch: &str) -> Self {
        let (rows, column_width) = crate::diff_view::prepare_gutter(patch);
        Self {
            rows,
            column_width,
            ranges: diff_ranges(patch).into(),
        }
    }

    /// Retained metadata allocations, including the outer Arc payload and the
    /// three Arc reference-count headers; allocator bookkeeping is excluded.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + std::mem::size_of_val(self.rows.as_ref())
            + std::mem::size_of_val(self.ranges.as_ref())
            + 3 * 2 * size_of::<usize>()
    }
}

/// Build only the currently requested editor. Decorations change presentation,
/// while the editor receives one owned copy of the unmodified source text.
pub fn editor(
    value: &str,
    language: &str,
    diff: Option<&PatchPresentation>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<EditorState> {
    editor_with_decorations(value, language, diff, window, cx).0
}

pub fn editor_with_decorations(
    value: &str,
    language: &str,
    diff: Option<&PatchPresentation>,
    window: &mut Window,
    cx: &mut App,
) -> (Entity<EditorState>, Option<TextDecorationCollection>) {
    let editor = cx.new(|cx| {
        EditorState::new(window, cx)
            .language(language.to_owned())
            .line_number(diff.is_none())
            .folding(diff.is_none())
            .soft_wrap(false)
            .default_value(value.to_owned())
    });
    crate::editor_find::reserve_highlight_layer(&editor, cx);
    let collection = diff.map(|presentation| {
        let decorations = theme_decorations(presentation, cx);
        // Find reserves the first collection; patch colors remain underneath it.
        editor.update(cx, |state, cx| {
            state.create_decorations_collection(decorations, cx)
        })
    });
    (editor, collection)
}

/// Keep the native editor identity, find session, focus, selection, and scroll
/// while a local filesystem change supplies new prepared text.
pub fn refresh_editor(
    editor: &Entity<EditorState>,
    value: &str,
    decorations: Option<(&TextDecorationCollection, &PatchPresentation)>,
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

pub fn refresh_theme(
    collection: &TextDecorationCollection,
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
                Kind::Added => HighlightStyle {
                    color: Some(rgb(palette.added).into()),
                    background_color: Some(rgb(palette.added_background).into()),
                    ..Default::default()
                },
                Kind::Removed => HighlightStyle {
                    color: Some(rgb(palette.removed).into()),
                    background_color: Some(rgb(palette.removed_background).into()),
                    ..Default::default()
                },
                Kind::Hunk => HighlightStyle {
                    color: Some(rgb(palette.hunk).into()),
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
}

#[derive(Debug, PartialEq, Eq)]
struct DiffRange {
    range: Range<usize>,
    kind: Kind,
}

/// Track unified hunk counts so `---` / `+++` file headers are not marked as
/// changes, while real changed source beginning with those characters is.
/// Byte offsets include existing line endings and are valid UTF-8 boundaries.
fn diff_ranges(value: &str) -> Vec<DiffRange> {
    let mut result: Vec<DiffRange> = Vec::new();
    let mut remaining: Option<(usize, usize)> = None;
    let mut offset = 0;
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
        assert_eq!(presentation.ranges.as_ref(), diff_ranges(patch));
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
                (Kind::Hunk, "@@ -1,2 +1,2 @@ function\r\n"),
                (Kind::Removed, "-héllo 🐢\r\n-adiós\r\n"),
                (Kind::Added, "+你好\r\n+再见"),
            ]
        );
    }

    #[test]
    fn file_headers_are_neutral_but_similar_changed_content_is_colored() {
        let patch = "--- a/file\n+++ b/file\n@@ -1,2 +1,2 @@\n--- removed code\n unchanged\n+++ added code\n--- a/next\n+++ b/next\n";
        assert_eq!(
            decorated(patch),
            vec![
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
}
