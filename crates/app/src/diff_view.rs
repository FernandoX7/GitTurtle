//! Unified patch presentation with a separate old/new line-number gutter.
//! The editor retains the exact patch, so selection, copying, and search never
//! include presentation-only numbers. No Git reads happen in this component.

use crate::{BORDER, MINT as ADDED, MUTED, PANEL as BACKGROUND};
use gpui_kit::{
    App, AppContext, Bounds, ContentMask, Context, Entity, IntoElement, ParentElement, Pixels,
    Point, Render, SharedString, Styled, Subscription, TextAlign, TextRun, Window, canvas,
    component::{
        Theme,
        input::{Editor, EditorState},
    },
    div, fill, point, px, relative, rgb, size,
};
use std::{ops::Range, sync::Arc};

const REMOVED: u32 = 0xf29aa2;
const FONT_SIZE: f32 = 12.;
const CELL_PADDING: f32 = 9.;

pub struct DiffView {
    editor: Entity<EditorState>,
    rows: Arc<[LineNumbers]>,
    column_width: f32,
    viewport: Viewport,
    _subscription: Subscription,
}

/// Wrap an existing patch editor. The patch must be the unchanged text already
/// loaded into that editor. Folding/wrapping are disabled so each patch line has
/// exactly one gutter row; the editor remains responsible for input and copy.
pub fn new(
    editor: Entity<EditorState>,
    patch: &str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<DiffView> {
    editor.update(cx, |state, cx| {
        state.set_line_number(false, window, cx);
        state.set_soft_wrap(false, window, cx);
        state.set_folding(false, window, cx);
    });
    let rows: Arc<[LineNumbers]> = parse_rows(patch).into();
    let digits = rows
        .iter()
        .flat_map(|row| [row.old, row.new])
        .flatten()
        .map(|number| number.checked_ilog10().unwrap_or(0) + 1)
        .max()
        .unwrap_or(1)
        .max(3);
    let column_width = digits as f32 * FONT_SIZE * 0.65 + CELL_PADDING * 2.;
    cx.new(|cx| {
        let viewport = Viewport::read(editor.read(cx));
        let subscription = cx.observe(&editor, |this: &mut DiffView, editor, cx| {
            let next = Viewport::read(editor.read(cx));
            // Editor paint also emits notifications. Only geometry changes need
            // an ancestor redraw; avoid scheduling frames merely for repainting.
            if next != this.viewport {
                this.viewport = next;
                cx.notify();
            }
        });
        DiffView {
            editor,
            rows,
            column_width,
            viewport,
            _subscription: subscription,
        }
    })
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.clone();
        let rows = Arc::clone(&self.rows);
        let column_width = self.column_width;
        let width = column_width * 2.;
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .pl(px(width))
            .font_family(Theme::global(cx).mono_font_family.clone())
            .text_size(px(FONT_SIZE))
            // Paint the editor first. It records its current scrolled bounds
            // during paint; the later gutter canvas reads those same-frame
            // coordinates, avoiding a one-frame lag during wheel scrolling.
            .child(
                Editor::new(&self.editor)
                    .h(relative(1.))
                    .readonly(true)
                    .bordered(false)
                    .aria_label("Read-only unified patch")
                    .text_size(px(FONT_SIZE)),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        paint_gutter(&editor, &rows, column_width, bounds, window, cx);
                    },
                )
                .absolute()
                .left_0()
                .top_0()
                .w(px(width))
                .h_full(),
            )
    }
}

#[derive(Clone, PartialEq)]
struct Viewport {
    bounds: Option<Bounds<Pixels>>,
    offset: Point<Pixels>,
    line_height: Option<Pixels>,
    visible: Option<Range<usize>>,
}

impl Viewport {
    fn read(editor: &EditorState) -> Self {
        Self {
            bounds: editor.text_bounds(),
            offset: editor.scroll_offset(),
            line_height: editor.line_height(),
            visible: editor.visible_row_range(),
        }
    }
}

fn paint_gutter(
    editor: &Entity<EditorState>,
    rows: &[LineNumbers],
    column_width: f32,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    window.paint_quad(fill(bounds, rgb(BACKGROUND)));
    for x in [column_width, column_width * 2. - 1.] {
        window.paint_quad(fill(
            Bounds::new(
                point(bounds.origin.x + px(x), bounds.origin.y),
                size(px(1.), bounds.size.height),
            ),
            rgb(BORDER),
        ));
    }
    let state = editor.read(cx);
    let (Some(text_bounds), Some(line_height), Some(visible)) = (
        state.text_bounds(),
        state.line_height(),
        state.visible_row_range(),
    ) else {
        return;
    };
    let input_bounds = state.input_bounds();
    let top = bounds.origin.y.max(input_bounds.origin.y);
    let bottom = (bounds.origin.y + bounds.size.height)
        .min(input_bounds.origin.y + input_bounds.size.height);
    if bottom <= top {
        return;
    }
    let mask = ContentMask {
        bounds: Bounds::new(
            point(bounds.origin.x, top),
            size(bounds.size.width, bottom - top),
        ),
    };
    let visible = visible.start.min(rows.len())..visible.end.min(rows.len());
    let font = window.text_style().font();
    window.with_content_mask(Some(mask), |window| {
        for row in visible {
            // GPUI's public text_bounds already includes the applied scroll
            // offset. Adding scroll_offset again would double-scroll the gutter.
            let y = text_bounds.origin.y + line_height * row as f32;
            if y + line_height <= top || y >= bottom {
                continue;
            }
            let numbers = rows[row];
            let (color, background) = match (numbers.old, numbers.new) {
                (Some(_), None) => (REMOVED, Some(0x342329)),
                (None, Some(_)) => (ADDED, Some(0x152d26)),
                _ => (MUTED, None),
            };
            if let Some(background) = background {
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.origin.x, y),
                        size(bounds.size.width - px(1.), line_height),
                    ),
                    rgb(background),
                ));
            }
            for (column, number) in [numbers.old, numbers.new].into_iter().enumerate() {
                let Some(number) = number else {
                    continue;
                };
                let label: SharedString = number.to_string().into();
                let run = TextRun {
                    len: label.len(),
                    font: font.clone(),
                    color: rgb(color).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window
                    .text_system()
                    .shape_line(label, px(FONT_SIZE), &[run], None);
                let right = bounds.origin.x + px(column_width * (column + 1) as f32 - CELL_PADDING);
                let _ = line.paint(
                    point(right - line.width(), y),
                    line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        }
    });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LineNumbers {
    old: Option<u64>,
    new: Option<u64>,
}

#[derive(Clone, Copy)]
struct HunkSide {
    next: u64,
    remaining: u64,
}

impl HunkSide {
    fn consume(&mut self) -> Option<u64> {
        if self.remaining == 0 {
            return None;
        }
        let number = self.next;
        self.remaining -= 1;
        self.next = self.next.saturating_add(1);
        Some(number)
    }
}

/// One entry per LF-delimited editor row, including the empty final row. CRLF
/// stays untouched and is treated as one line, matching GPUI's LF line metric.
fn parse_rows(patch: &str) -> Vec<LineNumbers> {
    let mut hunk: Option<(HunkSide, HunkSide)> = None;
    patch
        .split('\n')
        .map(|line| {
            if let Some(sides) = parse_hunk(line) {
                hunk = Some(sides);
                return LineNumbers::default();
            }
            let Some((old, new)) = hunk.as_mut() else {
                return LineNumbers::default();
            };
            let numbers = match line.as_bytes().first() {
                Some(b'-') if old.remaining > 0 => LineNumbers {
                    old: old.consume(),
                    new: None,
                },
                Some(b'+') if new.remaining > 0 => LineNumbers {
                    old: None,
                    new: new.consume(),
                },
                Some(b' ') if old.remaining > 0 && new.remaining > 0 => LineNumbers {
                    old: old.consume(),
                    new: new.consume(),
                },
                Some(b'\\') if line.trim_end_matches('\r') == "\\ No newline at end of file" => {
                    LineNumbers::default()
                }
                _ => {
                    hunk = None;
                    return LineNumbers::default();
                }
            };
            if hunk
                .as_ref()
                .is_some_and(|(old, new)| old.remaining == 0 && new.remaining == 0)
            {
                hunk = None;
            }
            numbers
        })
        .collect()
}

fn parse_hunk(line: &str) -> Option<(HunkSide, HunkSide)> {
    // A context line can itself contain header-looking source after its leading
    // space. Only a marker in column zero starts a new unified hunk.
    if !line.starts_with("@@ ") {
        return None;
    }
    let mut fields = line.split_ascii_whitespace();
    if fields.next()? != "@@" {
        return None;
    }
    let old = parse_side(fields.next()?.strip_prefix('-')?)?;
    let new = parse_side(fields.next()?.strip_prefix('+')?)?;
    if fields.next()? != "@@" {
        return None;
    }
    Some((old, new))
}

fn parse_side(range: &str) -> Option<HunkSide> {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    let next: u64 = start.parse().ok()?;
    let remaining: u64 = count.parse().ok()?;
    if remaining > 0 {
        if next == 0 {
            return None;
        }
        next.checked_add(remaining - 1)?;
    }
    Some(HunkSide { next, remaining })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(old: Option<u64>, new: Option<u64>) -> LineNumbers {
        LineNumbers { old, new }
    }

    #[test]
    fn utf8_crlf_patch_keeps_exact_text_and_corresponding_line_numbers() {
        let patch = "--- a/你好\r\n+++ b/你好\r\n@@ -7,3 +9,3 @@ fn 🐢\r\n context\r\n-héllo\r\n+你好\r\n tail\r\n";
        let original = patch.to_owned();
        assert_eq!(
            parse_rows(patch),
            vec![
                n(None, None),
                n(None, None),
                n(None, None),
                n(Some(7), Some(9)),
                n(Some(8), None),
                n(None, Some(10)),
                n(Some(9), Some(11)),
                n(None, None),
            ]
        );
        assert_eq!(patch, original);
    }

    #[test]
    fn roots_deletions_multiple_hunks_and_no_newline_markers_do_not_shift_rows() {
        let patch = "@@ -0,0 +1,2 @@\n+first\n+second\n\\ No newline at end of file\n@@ -5,2 +0,0 @@\n-one\n-two\n@@ -10 +20 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file";
        assert_eq!(
            parse_rows(patch),
            vec![
                n(None, None),
                n(None, Some(1)),
                n(None, Some(2)),
                n(None, None),
                n(None, None),
                n(Some(5), None),
                n(Some(6), None),
                n(None, None),
                n(Some(10), None),
                n(None, None),
                n(None, Some(20)),
                n(None, None),
            ]
        );
    }

    #[test]
    fn headers_and_malformed_hunks_are_blank_but_header_like_source_counts() {
        let patch = "--- a/file\n+++ b/file\n@@ -1 +1 @@\n--- source\n+++ source\n@@ broken @@\n+untrusted\n@@ -0 +1 @@\n+bad zero\n@@ -18446744073709551615,2 +1 @@\n-overflow";
        let rows = parse_rows(patch);
        assert_eq!(rows[3], n(Some(1), None));
        assert_eq!(rows[4], n(None, Some(1)));
        assert!(
            rows.iter()
                .enumerate()
                .all(|(i, row)| [3, 4].contains(&i) || *row == n(None, None))
        );
        assert_eq!(parse_rows(""), vec![n(None, None)]);
    }

    #[test]
    fn huge_line_numbers_and_empty_hunks_never_overflow_or_invent_lines() {
        let patch = "@@ -18446744073709551615 +0,0 @@\n-last\n@@ -0,0 +0,0 @@\n+outside hunk\n";
        assert_eq!(
            parse_rows(patch),
            vec![
                n(None, None),
                n(Some(u64::MAX), None),
                n(None, None),
                n(None, None),
                n(None, None)
            ]
        );
    }

    #[test]
    fn header_looking_context_is_counted_as_source() {
        let patch = "@@ -4,2 +8,2 @@\n @@ -99 +100 @@\n unchanged\n";
        assert_eq!(
            parse_rows(patch),
            vec![
                n(None, None),
                n(Some(4), Some(8)),
                n(Some(5), Some(9)),
                n(None, None),
            ]
        );
    }
}
