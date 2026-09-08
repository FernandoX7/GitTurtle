//! Aligned source views prepared on the reader, with one linked vertical viewport.
use crate::{
    appearance::palette,
    diff_view::{self, LineNumbers},
    text::PatchPresentation,
};
use gpui_kit::{
    App, AppContext, ClipboardItem, Context, Entity, HighlightStyle, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, Styled, Subscription, Window,
    component::input::{Copy, EditorState, TextDecoration, TextDecorationCollection},
    div, point, px, rgb,
};
use std::{mem::size_of, ops::Range, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    old: Option<usize>,
    new: Option<usize>,
    changed: bool,
}

pub struct SplitSide {
    text: String,
    numbers: Arc<[LineNumbers]>,
    changes: Vec<Range<usize>>,
    // Only real source bytes participate in copying; alignment blanks do not.
    source_spans: Vec<Range<usize>>,
}

pub struct SplitPresentation {
    sides: [SplitSide; 2],
    first_change: usize,
}

impl SplitPresentation {
    pub fn prepare(old: &str, new: &str, patch: &PatchPresentation) -> Self {
        let sources = [
            old.split_inclusive('\n').collect::<Vec<_>>(),
            new.split_inclusive('\n').collect::<Vec<_>>(),
        ];
        let mut rows = Vec::new();
        let mut cursor = [0, 0];
        let mut removed = Vec::new();
        let mut added = Vec::new();
        let flush = |rows: &mut Vec<Row>, removed: &mut Vec<usize>, added: &mut Vec<usize>| {
            for i in 0..removed.len().max(added.len()) {
                rows.push(Row {
                    old: removed.get(i).copied(),
                    new: added.get(i).copied(),
                    changed: true,
                });
            }
            removed.clear();
            added.clear();
        };
        for numbers in patch.rows.iter() {
            let numbers = [
                numbers.old.map(|n| n as usize - 1),
                numbers.new.map(|n| n as usize - 1),
            ];
            if numbers == [None, None] {
                continue;
            }
            if numbers.iter().all(Option::is_some)
                || numbers
                    .iter()
                    .enumerate()
                    .any(|(side, number)| number.is_some_and(|n| n > cursor[side]))
            {
                flush(&mut rows, &mut removed, &mut added);
            }
            if removed.is_empty() && added.is_empty() {
                let gap = numbers
                    .iter()
                    .enumerate()
                    .filter_map(|(side, number)| number.map(|n| n.saturating_sub(cursor[side])))
                    .min()
                    .unwrap_or(0);
                for _ in 0..gap {
                    rows.push(Row {
                        old: (cursor[0] < sources[0].len()).then_some(cursor[0]),
                        new: (cursor[1] < sources[1].len()).then_some(cursor[1]),
                        changed: false,
                    });
                    cursor[0] += 1;
                    cursor[1] += 1;
                }
            }
            match numbers {
                [Some(old), Some(new)] => rows.push(Row {
                    old: Some(old),
                    new: Some(new),
                    changed: false,
                }),
                [Some(old), None] => removed.push(old),
                [None, Some(new)] => added.push(new),
                _ => unreachable!(),
            }
            for side in 0..2 {
                if let Some(number) = numbers[side] {
                    cursor[side] = number + 1;
                }
            }
        }
        flush(&mut rows, &mut removed, &mut added);
        while cursor[0] < sources[0].len() || cursor[1] < sources[1].len() {
            rows.push(Row {
                old: (cursor[0] < sources[0].len()).then_some(cursor[0]),
                new: (cursor[1] < sources[1].len()).then_some(cursor[1]),
                changed: false,
            });
            cursor[0] += 1;
            cursor[1] += 1;
        }
        Self {
            first_change: rows.iter().position(|row| row.changed).unwrap_or(0),
            sides: std::array::from_fn(|side| {
                let mut text = String::new();
                let mut numbers = Vec::new();
                let mut changes = Vec::new();
                let mut source_spans = Vec::new();
                for row in &rows {
                    let number = if side == 0 { row.old } else { row.new };
                    let start = text.len();
                    if let Some(number) = number {
                        if let Some(line) = sources[side].get(number) {
                            text.push_str(line);
                            source_spans.push(start..text.len());
                            if !line.ends_with('\n') {
                                text.push('\n');
                            }
                        }
                    } else {
                        text.push('\n');
                    }
                    if row.changed {
                        changes.push(start..text.len());
                    }
                    numbers.push(LineNumbers {
                        old: number.map(|n| n as u64 + 1),
                        new: None,
                    });
                }
                numbers.push(LineNumbers::default());
                SplitSide {
                    text,
                    numbers: numbers.into(),
                    changes,
                    source_spans,
                }
            }),
        }
    }

    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + self
                .sides
                .iter()
                .map(|side| {
                    side.text.capacity()
                        + std::mem::size_of_val(side.numbers.as_ref())
                        + 2 * size_of::<usize>()
                        + (side.changes.capacity() + side.source_spans.capacity())
                            * size_of::<Range<usize>>()
                })
                .sum::<usize>()
    }
}

impl SplitSide {
    fn decorations(&self, color: u32) -> Vec<TextDecoration> {
        let style = HighlightStyle {
            background_color: Some(rgb(color).into()),
            ..Default::default()
        };
        self.changes
            .iter()
            .map(|range| TextDecoration::new(range.clone(), style))
            .collect()
    }

    fn source_selection(&self, selection: Range<usize>) -> String {
        let mut text = String::new();
        for span in &self.source_spans {
            let start = selection.start.max(span.start);
            let end = selection.end.min(span.end);
            if start < end {
                text.push_str(&self.text[start..end]);
            }
            if span.start >= selection.end {
                break;
            }
        }
        text
    }
}

pub struct SplitView {
    presentation: Arc<SplitPresentation>,
    editors: [Entity<EditorState>; 2],
    views: [Entity<diff_view::DiffView>; 2],
    decorations: [TextDecorationCollection; 2],
    search_heights: [Pixels; 2],
    scroll: LinkedScroll,
    initial_row: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

/// Editor scroll setters defer application until layout. A notification from
/// the setter still contains the old offset and must never bounce it back.
#[derive(Default)]
struct LinkedScroll {
    applied: [Pixels; 2],
    pending: [Option<Pixels>; 2],
}

impl LinkedScroll {
    fn observe(&mut self, side: usize, next: Pixels) -> Option<(usize, Pixels)> {
        if self.pending[side] == Some(next) {
            self.pending[side] = None;
            self.applied[side] = next;
            return None;
        }
        if self.applied[side] == next {
            return None;
        }
        self.applied[side] = next;
        self.pending[side] = None;
        let other = 1 - side;
        if self.applied[other] == next && self.pending[other].is_none() {
            return None;
        }
        self.pending[other] = Some(next);
        Some((other, next))
    }
}

pub fn new(
    presentation: Arc<SplitPresentation>,
    language: &str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<SplitView> {
    let colors = palette(cx);
    let editors: [_; 2] = std::array::from_fn(|side| {
        cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.to_owned())
                .line_number(false)
                .folding(false)
                .soft_wrap(false)
                .default_value(presentation.sides[side].text.clone())
        })
    });
    let decorations = std::array::from_fn(|side| {
        let color = if side == 0 {
            colors.removed_background
        } else {
            colors.added_background
        };
        editors[side].update(cx, |editor, cx| {
            editor.create_decorations_collection(presentation.sides[side].decorations(color), cx)
        })
    });
    let views = std::array::from_fn(|side| {
        let numbers = Arc::clone(&presentation.sides[side].numbers);
        let digits = numbers
            .iter()
            .filter_map(|n| n.old)
            .max()
            .unwrap_or(1)
            .ilog10()
            + 1;
        diff_view::new_numbered(
            editors[side].clone(),
            numbers,
            digits.max(3) as f32 * 7.8 + 18.,
            true,
            if side == 0 {
                "Read-only Before source"
            } else {
                "Read-only After source"
            },
            window,
            cx,
        )
    });
    cx.new(|cx| {
        let mut subscriptions = Vec::new();
        for (side, editor) in editors.iter().enumerate() {
            subscriptions.push(cx.observe(editor, move |this: &mut SplitView, editor, cx| {
                if let (Some(row), Some(height)) = (this.initial_row, editor.read(cx).line_height())
                {
                    this.initial_row = None;
                    if row > 0 {
                        let y = -height * row as f32;
                        this.scroll.pending = [Some(y); 2];
                        for editor in &this.editors {
                            editor.update(cx, |editor, cx| {
                                editor.set_scroll_offset(point(px(0.), y), cx)
                            });
                        }
                        cx.notify();
                        return;
                    }
                }
                let next = editor.read(cx).scroll_offset();
                if let Some((other, y)) = this.scroll.observe(side, next.y) {
                    let target = point(this.editors[other].read(cx).scroll_offset().x, y);
                    this.editors[other]
                        .update(cx, |editor, cx| editor.set_scroll_offset(target, cx));
                    cx.notify();
                }
            }));
        }
        for (side, view) in views.iter().enumerate() {
            subscriptions.push(cx.observe(view, move |this: &mut SplitView, view, cx| {
                let height = view.read(cx).find_header_height(cx);
                if (this.search_heights[side] - height).abs() > px(0.5) {
                    this.search_heights[side] = height;
                    cx.notify();
                }
            }));
        }
        SplitView {
            initial_row: Some(presentation.first_change.saturating_sub(3)),
            presentation,
            editors,
            views,
            decorations,
            search_heights: [px(0.); 2],
            scroll: LinkedScroll::default(),
            _subscriptions: subscriptions,
        }
    })
}

impl SplitView {
    pub fn refresh_theme(&mut self, cx: &mut Context<Self>) {
        let colors = palette(cx);
        for (side, color) in [colors.removed_background, colors.added_background]
            .into_iter()
            .enumerate()
        {
            self.decorations[side].set(self.presentation.sides[side].decorations(color), cx);
        }
        cx.notify();
    }

    /// Update a mutable file while retaining editor focus, find state, selection
    /// and viewport. Text and alignment metadata were prepared on the worker.
    pub fn refresh(
        &mut self,
        presentation: Arc<SplitPresentation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let colors = palette(cx);
        self.initial_row = None;
        for side in 0..2 {
            crate::text::refresh_editor(
                &self.editors[side],
                &presentation.sides[side].text,
                None,
                window,
                cx,
            );
            let color = if side == 0 {
                colors.removed_background
            } else {
                colors.added_background
            };
            self.decorations[side].set(presentation.sides[side].decorations(color), cx);
            let numbers = Arc::clone(&presentation.sides[side].numbers);
            let digits = numbers
                .iter()
                .filter_map(|n| n.old)
                .max()
                .unwrap_or(1)
                .ilog10()
                + 1;
            self.views[side].update(cx, |view, cx| {
                view.refresh_numbered(numbers, digits.max(3) as f32 * 7.8 + 18., cx)
            });
        }
        self.presentation = presentation;
        cx.notify();
    }
}

impl Render for SplitView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let search_heights = self.search_heights;
        let reserved_height = search_heights[0].max(search_heights[1]);
        div().size_full().flex().children((0..2).map(|side| {
            let editor = self.editors[side].clone();
            let padding = reserved_height - search_heights[side];
            let presentation = Arc::clone(&self.presentation);
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .border_r_1()
                .border_color(rgb(p.border))
                .capture_action(move |_: &Copy, _, cx| {
                    let selection = editor.read(cx).selected_range();
                    if !selection.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            presentation.sides[side].source_selection(selection),
                        ));
                        cx.stop_propagation();
                    }
                })
                .child(
                    div()
                        .h(px(30.))
                        .px_3()
                        .flex()
                        .items_center()
                        .bg(rgb(p.panel))
                        .text_size(px(11.))
                        .text_color(rgb(p.muted))
                        .child(if side == 0 { "Before" } else { "After" }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .flex()
                        .flex_col()
                        .pt(padding)
                        .child(div().flex_1().min_h_0().child(self.views[side].clone())),
                )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linked_scroll_ignores_deferred_notifications_and_follows_bursts_and_reversal() {
        let mut scroll = LinkedScroll::default();
        assert_eq!(scroll.observe(0, px(-40.)), Some((1, px(-40.))));
        for _ in 0..4 {
            assert_eq!(scroll.observe(1, px(0.)), None);
        }
        assert_eq!(scroll.observe(0, px(-60.)), Some((1, px(-60.))));
        assert_eq!(scroll.observe(1, px(0.)), None);
        assert_eq!(scroll.observe(1, px(-60.)), None);
        assert_eq!(scroll.observe(1, px(-20.)), Some((0, px(-20.))));
        assert_eq!(scroll.observe(0, px(-60.)), None);
        assert_eq!(scroll.observe(0, px(-20.)), None);
        assert_eq!(scroll.applied, [px(-20.); 2]);
        assert_eq!(scroll.pending, [None; 2]);
    }
    fn prepare(old: &str, new: &str, patch: &str) -> SplitPresentation {
        SplitPresentation::prepare(old, new, &PatchPresentation::prepare(patch))
    }
    fn numbers(p: &SplitPresentation, side: usize) -> Vec<Option<u64>> {
        p.sides[side].numbers.iter().map(|row| row.old).collect()
    }
    #[test]
    fn pairs_replacements_and_pads_extra_lines_without_corrupting_copy() {
        let p = prepare(
            "one\nold\ntail\n",
            "one\nnew\nextra\ntail\n",
            "@@ -1,3 +1,4 @@\n one\n-old\n+new\n+extra\n tail\n",
        );
        assert_eq!(numbers(&p, 0), [Some(1), Some(2), None, Some(3), None]);
        assert_eq!(numbers(&p, 1), [Some(1), Some(2), Some(3), Some(4), None]);
        assert_eq!(
            p.sides[0].source_selection(0..p.sides[0].text.len()),
            "one\nold\ntail\n"
        );
        assert_eq!(p.sides[0].source_selection(4..10), "old\nt");
    }
    #[test]
    fn handles_absent_sides_crlf_unicode_and_missing_final_newline() {
        let p = prepare(
            "",
            "你好\r\n🐢",
            "@@ -0,0 +1,2 @@\n+你好\r\n+🐢\n\\ No newline at end of file\n",
        );
        assert_eq!(numbers(&p, 0), [None, None, None]);
        assert_eq!(p.sides[0].source_selection(0..usize::MAX), "");
        assert_eq!(p.sides[1].source_selection(0..usize::MAX), "你好\r\n🐢");
        let p = prepare(
            "gone",
            "",
            "@@ -1 +0,0 @@\n-gone\n\\ No newline at end of file\n",
        );
        assert_eq!(p.sides[0].source_selection(0..usize::MAX), "gone");
        assert_eq!(numbers(&p, 1), [None, None]);
    }
    #[test]
    fn retains_unchanged_regions_between_hunks_and_after_last_hunk() {
        let old = (1..=30).map(|i| format!("line {i}\n")).collect::<String>();
        let new = old
            .replace("line 8\n", "changed 8\n")
            .replace("line 24\n", "changed 24\n");
        let p = prepare(
            &old,
            &new,
            "@@ -8 +8 @@\n-line 8\n+changed 8\n@@ -24 +24 @@\n-line 24\n+changed 24\n",
        );
        assert_eq!(p.sides[0].source_selection(0..usize::MAX), old);
        assert_eq!(p.sides[1].source_selection(0..usize::MAX), new);
        assert_eq!(
            numbers(&p, 0),
            (1..=30).map(Some).chain([None]).collect::<Vec<_>>()
        );
    }
}
