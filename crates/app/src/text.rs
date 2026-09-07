use gpui_kit::{
    App, AppContext, Entity, FontWeight, HighlightStyle, Window,
    component::input::{EditorState, TextDecoration},
    rgb,
};
use std::ops::Range;

/// Build only the currently requested editor. Decorations change presentation,
/// while the editor receives one owned copy of the unmodified source text.
pub fn editor(
    value: &str,
    language: &str,
    diff: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<EditorState> {
    cx.new(|cx| {
        let mut state = EditorState::new(window, cx)
            .language(language.to_owned())
            .line_number(!diff)
            .folding(!diff)
            .soft_wrap(false)
            .default_value(value.to_owned());
        if diff {
            let decorations = diff_ranges(value)
                .into_iter()
                .map(|decoration| {
                    let style = match decoration.kind {
                        Kind::Added => HighlightStyle {
                            color: Some(rgb(0x7adfb4).into()),
                            background_color: Some(rgb(0x152d26).into()),
                            ..Default::default()
                        },
                        Kind::Removed => HighlightStyle {
                            color: Some(rgb(0xf29aa2).into()),
                            background_color: Some(rgb(0x342329).into()),
                            ..Default::default()
                        },
                        Kind::Hunk => HighlightStyle {
                            color: Some(rgb(0x8db7f6).into()),
                            font_weight: Some(FontWeight::MEDIUM),
                            ..Default::default()
                        },
                    };
                    TextDecoration::new(decoration.range, style)
                })
                .collect();
            // Collections are retained by EditorState, not by the returned
            // handle, and disappear with this editor when selection changes.
            state.create_decorations_collection(decorations, cx);
        }
        state
    })
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
