//! Aligned source views prepared on the reader, with one linked vertical viewport.
use crate::{
    appearance::palette,
    diff_view::{self, LineNumbers},
    focus_ring::FocusRing,
    text::PatchPresentation,
};
use gpui_kit::{
    App, AppContext, ClipboardItem, Context, Entity, Focusable, HighlightStyle, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, Styled, Subscription, Window, canvas,
    component::input::{Copy, EditorState, TextDecoration},
    div, point, px, rgb,
};
use std::{mem::size_of, ops::Range, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) old: Option<usize>,
    pub(crate) new: Option<usize>,
    pub(crate) changed: bool,
}

pub struct SplitSide {
    text: String,
    numbers: Arc<[LineNumbers]>,
    changes: Vec<Range<usize>>,
    words: Vec<Range<usize>>,
    // Only real source bytes participate in copying; alignment blanks do not.
    source_spans: Vec<Range<usize>>,
}

pub struct SplitPresentation {
    sides: [SplitSide; 2],
    first_change: usize,
    change_rows: Arc<[usize]>,
}

impl SplitPresentation {
    pub fn prepare(old: &str, new: &str, patch: &PatchPresentation) -> Self {
        let sources = [
            old.split_inclusive('\n').collect::<Vec<_>>(),
            new.split_inclusive('\n').collect::<Vec<_>>(),
        ];
        let rows = align_rows(&sources, patch);
        Self::from_rows(&sources, &rows)
    }

    pub(crate) fn from_rows(sources: &[Vec<&str>; 2], rows: &[Row]) -> Self {
        let mut budget = crate::text::WordBudget::default();
        let word_rows = rows
            .iter()
            .map(|row| match (row.old, row.new) {
                (Some(old), Some(new)) if row.changed => crate::text::word_changes(
                    sources[0].get(old).unwrap_or(&""),
                    sources[1].get(new).unwrap_or(&""),
                    &mut budget,
                ),
                _ => Default::default(),
            })
            .collect::<Vec<_>>();
        let change_rows = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| (row.changed && (i == 0 || !rows[i - 1].changed)).then_some(i))
            .collect::<Vec<_>>()
            .into();
        Self {
            change_rows,
            first_change: rows.iter().position(|row| row.changed).unwrap_or(0),
            sides: std::array::from_fn(|side| {
                let mut text = String::new();
                let mut numbers = Vec::new();
                let mut changes = Vec::new();
                let mut words = Vec::new();
                let mut source_spans = Vec::new();
                for (row_index, row) in rows.iter().enumerate() {
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
                        words.extend(
                            word_rows[row_index][side]
                                .iter()
                                .map(|range| (start + range.start)..(start + range.end)),
                        );
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
                    words,
                    source_spans,
                }
            }),
        }
    }

    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + std::mem::size_of_val(self.change_rows.as_ref())
            + 2 * size_of::<usize>()
            + self
                .sides
                .iter()
                .map(|side| {
                    side.text.capacity()
                        + std::mem::size_of_val(side.numbers.as_ref())
                        + 2 * size_of::<usize>()
                        + (side.changes.capacity()
                            + side.words.capacity()
                            + side.source_spans.capacity())
                            * size_of::<Range<usize>>()
                })
                .sum::<usize>()
    }
}

pub(crate) fn align_rows(sources: &[Vec<&str>; 2], patch: &PatchPresentation) -> Vec<Row> {
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
    rows
}

/// Each side's changed-line tint, changed-word tint and, under More contrast, the rule a changed
/// word draws along its box's bottom edge, old side first, from the applied palette and its
/// fitted editor highlights.
fn side_tints(cx: &App) -> [(u32, u32, Option<u32>); 2] {
    let (colors, highlights) = (palette(cx), crate::appearance::editor_highlights(cx));
    [
        (
            colors.removed_background,
            highlights.removed_word,
            highlights.removed_rule,
        ),
        (
            colors.added_background,
            highlights.added_word,
            highlights.added_rule,
        ),
    ]
}

impl SplitSide {
    /// The side's changed lines in `line` and its emphasized words in `word`, each word with a
    /// `rule` along its box's bottom edge where there is one, on a decoration of its own, as
    /// `text::theme_decorations` gives a unified patch's words theirs. Each run of adjacent
    /// changed source lines also fills its lines' whole rows in `line`, on one decoration of
    /// its own, as a unified patch's changed blocks do, blank lines included; a filler row,
    /// which holds no source line, stays out of every run and draws nothing (decision,
    /// 2026-11-05).
    fn decorations(&self, (line, word, rule): (u32, u32, Option<u32>)) -> Vec<TextDecoration> {
        let style = HighlightStyle {
            background_color: Some(rgb(line).into()),
            ..Default::default()
        };
        let emphasized = HighlightStyle {
            background_color: Some(rgb(word).into()),
            ..Default::default()
        };
        let mut result = Vec::new();
        let mut words = self.words.iter().peekable();
        // A changed row holds a source line where a source span starts with it.
        let mut spans = self.source_spans.iter().peekable();
        let mut block: Option<Range<usize>> = None;
        let fill = |block: Option<Range<usize>>, result: &mut Vec<TextDecoration>| {
            if let Some(block) = block {
                result.push(
                    TextDecoration::new(block, HighlightStyle::default())
                        .with_row_background(rgb(line).into()),
                );
            }
        };
        for range in &self.changes {
            while spans.next_if(|span| span.start < range.start).is_some() {}
            let source = spans.peek().is_some_and(|span| span.start == range.start);
            match &mut block {
                Some(block) if source && block.end == range.start => block.end = range.end,
                _ => fill(
                    std::mem::replace(&mut block, source.then(|| range.clone())),
                    &mut result,
                ),
            }
            let mut cursor = range.start;
            while let Some(word) = words.peek() {
                if word.start >= range.end {
                    break;
                }
                let word = words.next().unwrap();
                if cursor < word.start {
                    result.push(TextDecoration::new(cursor..word.start, style));
                }
                result.push(TextDecoration::new(word.clone(), emphasized));
                if let Some(rule) = rule {
                    result.push(
                        TextDecoration::new(word.clone(), HighlightStyle::default())
                            .with_bottom_rule(rgb(rule).into()),
                    );
                }
                cursor = word.end;
            }
            if cursor < range.end {
                result.push(TextDecoration::new(cursor..range.end, style));
            }
        }
        fill(block, &mut result);
        result
    }

    fn source_selection(&self, selection: Range<usize>) -> String {
        let mut text = String::new();
        for span in &self.source_spans {
            let mut start = selection.start.max(span.start);
            let mut end = selection.end.min(span.end);
            while start < end && !self.text.is_char_boundary(start) {
                start += 1;
            }
            while end > start && !self.text.is_char_boundary(end) {
                end -= 1;
            }
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
    decorations: [crate::editor_find::PatchDecorations; 2],
    scroll: LinkedScroll,
    initial_row: Option<usize>,
    change_index: Option<usize>,
    /// The window's keyboard focus ring, which a side's text rings its edge
    /// with while its editor holds focus.
    focus_ring: FocusRing,
    _subscriptions: Vec<Subscription>,
}

/// Track accepted viewport changes and acknowledgments independently from
/// selection. Laid-out editors publish linked offsets immediately; cold editors
/// may still acknowledge their initial request after layout.
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
    focus_ring: FocusRing,
    window: &mut Window,
    cx: &mut App,
) -> Entity<SplitView> {
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
    for editor in &editors {
        crate::editor_find::reserve_highlight_layer(editor, cx);
    }
    let tints = side_tints(cx);
    let decorations = std::array::from_fn(|side| {
        crate::editor_find::patch_decorations(
            &editors[side],
            presentation.sides[side].decorations(tints[side]),
            cx,
        )
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
                    if diff_view::scroll_trace_enabled() {
                        diff_view::trace_scroll(
                            "split_link",
                            format_args!(
                                "source={:?} target={:?} applied={:?} requested={target:?}",
                                this.editors[side].entity_id(),
                                this.editors[other].entity_id(),
                                this.editors[other].read(cx).scroll_offset(),
                            ),
                        );
                    }
                    this.editors[other]
                        .update(cx, |editor, cx| editor.set_scroll_offset(target, cx));
                    cx.notify();
                }
            }));
        }
        // The sides share one Find bar, which `render` draws across both
        // (`editor_find::Editor::sharing_find_with`).
        for (side, view) in views.iter().enumerate() {
            let partner = editors[1 - side].clone();
            let side = if side == 0 {
                crate::editor_find::Side::Before
            } else {
                crate::editor_find::Side::After
            };
            view.update(cx, |view, _| view.share_find_with(partner, side));
        }
        SplitView {
            initial_row: Some(presentation.first_change.saturating_sub(3)),
            change_index: None,
            presentation,
            editors,
            views,
            decorations,
            scroll: LinkedScroll::default(),
            focus_ring,
            _subscriptions: subscriptions,
        }
    })
}

impl SplitView {
    /// The Before and After sides' editors.
    pub(crate) fn editors(&self) -> &[Entity<EditorState>; 2] {
        &self.editors
    }

    pub fn rescale_code(&mut self, ratio: f32, cx: &mut Context<Self>) {
        // An editor that has never painted still owns its initial logical
        // change row; the first measured layout will resolve that request.
        self.scroll.pending =
            std::array::from_fn(|side| Some(self.editors[side].read(cx).scroll_offset().y * ratio));
        for editor in &self.editors {
            crate::text::rescale_editor(editor, ratio, cx);
        }
        cx.notify();
    }

    pub fn navigate_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        let state = self.editors[0].read(cx);
        let visible = state.visible_row_range().map_or(0, |range| range.start);
        let Some(index) = crate::text_review::next_change(
            &self.presentation.change_rows,
            self.change_index,
            visible,
            forward,
        ) else {
            return;
        };
        let Some(height) = state.line_height() else {
            return;
        };
        self.change_index = Some(index);
        self.initial_row = None;
        let y = -height * self.presentation.change_rows[index].saturating_sub(2) as f32;
        self.scroll.pending = [Some(y); 2];
        for editor in &self.editors {
            let x = editor.read(cx).scroll_offset().x;
            editor.update(cx, |state, cx| state.set_scroll_offset(point(x, y), cx));
        }
        cx.notify();
    }

    pub fn refresh_theme(&mut self, cx: &mut Context<Self>) {
        for (side, tints) in side_tints(cx).into_iter().enumerate() {
            self.decorations[side].set(self.presentation.sides[side].decorations(tints), cx);
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
        self.initial_row = None;
        self.change_index = None;
        for (side, tints) in side_tints(cx).into_iter().enumerate() {
            crate::text::refresh_editor(
                &self.editors[side],
                &presentation.sides[side].text,
                None,
                window,
                cx,
            );
            self.decorations[side].set(presentation.sides[side].decorations(tints), cx);
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
        let trace_editors = diff_view::scroll_trace_enabled().then(|| self.editors.clone());
        // A side whose editor holds keyboard focus rings its text's edge,
        // over its lines, so none of them moves.
        let mut edges: [_; 2] = std::array::from_fn(|side| {
            self.focus_ring
                .edge(&self.editors[side].read(cx).focus_handle(cx), rgb(p.accent))
        });
        // The sides' one Find bar, open on either, stands across both in
        // their headings' row, so their rows start level under it and a
        // narrow window's bar takes the comparison's width, as the unified
        // patch's does, and the headings' height: on a side of its own it
        // took four lines in a 461 × 490 window and left no row in view.
        let shared_bar = crate::editor_find::shared_bar(&self.editors, cx);
        let headings = shared_bar.is_none().then(|| {
            div().flex().children((0..2).map(|side| {
                div()
                    .flex_1()
                    .min_w_0()
                    .h(crate::appearance::ui_size(30.))
                    .px_3()
                    .flex()
                    .items_center()
                    .border_r_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.panel))
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(if side == 0 { "Before" } else { "After" })
            }))
        });
        let sides = div().flex_1().min_h_0().flex().children((0..2).map(|side| {
            let editor = self.editors[side].clone();
            let presentation = Arc::clone(&self.presentation);
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .border_r_1()
                .border_color(rgb(p.border))
                .capture_action(move |_: &Copy, window, cx| {
                    if !editor.read(cx).focus_handle(cx).is_focused(window) {
                        return;
                    }
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
                        .debug_selector(move || format!("split-text-{side}"))
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .flex()
                        .flex_col()
                        .child(div().flex_1().min_h_0().child(self.views[side].clone()))
                        .children(edges[side].take()),
                )
        }));
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .children(headings)
            .children(shared_bar)
            .child(sides)
            .children(trace_editors.map(|editors| {
            // Painted after both panes, so one record contains their actual
            // same-frame geometry. The transparent observer has no hitbox and
            // never requests another frame or changes either editor.
            canvas(
                |_, _, _| (),
                move |_, _, window, cx| {
                    let before = editors[0].read(cx);
                    let after = editors[1].read(cx);
                    diff_view::trace_scroll(
                        "split_paint",
                        format_args!(
                            "window={:?} editors={:?} before_offset={:?} after_offset={:?} before_text={:?} after_text={:?} before_visible={:?} after_visible={:?}",
                            window.window_handle().window_id(),
                            [editors[0].entity_id(), editors[1].entity_id()],
                            before.scroll_offset(),
                            after.scroll_offset(),
                            before.text_bounds(),
                            after.text_bounds(),
                            before.visible_row_range(),
                            after.visible_row_range(),
                        ),
                    );
                },
            ).absolute().size_full()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit as gpui;

    struct FontProbe {
        split: Entity<SplitView>,
        font_size: Pixels,
    }
    impl Render for FontProbe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            // Exercise the real SplitView subscriptions and Editor layout with
            // a per-window font override; process-wide appearance stays intact
            // for other tests running in parallel.
            div()
                .w(px(900.))
                .h(px(400.))
                .flex()
                .children(self.split.read(cx).editors.iter().map(|editor| {
                    gpui::component::input::Editor::new(editor)
                        .readonly(true)
                        .text_size(self.font_size)
                        .w(px(450.))
                        .h_full()
                }))
        }
    }

    fn long_presentation() -> Arc<SplitPresentation> {
        let lines = vec!["source row\n"; 800];
        let rows = (0..800)
            .map(|row| Row {
                old: Some(row),
                new: Some(row),
                changed: row == 549,
            })
            .collect::<Vec<_>>();
        Arc::new(SplitPresentation::from_rows(&[lines.clone(), lines], &rows))
    }

    fn notify_measured_editors(split: &Entity<SplitView>, cx: &mut gpui::VisualTestContext) {
        // GPUI suppresses observer notifications during paint. A subsequent
        // editor event (for example focus or selection) exposes the measured
        // viewport to SplitView's production subscriptions.
        cx.update(|_, cx| {
            for editor in split.read(cx).editors.clone() {
                editor.update(cx, |_, cx| cx.notify());
            }
        });
        cx.cx.update(|_| {});
        cx.executor().run_until_parked();
    }

    #[gpui::test]
    fn fractional_split_layout_preserves_both_measured_viewports(cx: &mut gpui::TestAppContext) {
        cx.update(gpui::init);
        let (probe, cx) = cx.add_window_view(|window, cx| FontProbe {
            split: new(
                long_presentation(),
                "text",
                FocusRing::default(),
                window,
                cx,
            ),
            font_size: px(12.),
        });
        let split = cx.read(|cx| probe.read(cx).split.clone());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            notify_measured_editors(&split, cx);
        }
        cx.read(|cx| {
            for editor in &split.read(cx).editors {
                assert_eq!(editor.read(cx).line_height(), Some(px(18.)));
                assert_eq!(editor.read(cx).visible_row_range().unwrap().start, 546);
            }
        });
        for (font, ratio, height) in [(15., 1.25, 23.), (12., 0.8, 18.)] {
            cx.update(|_, cx| {
                probe.update(cx, |probe, cx| {
                    probe.font_size = px(font);
                    split.update(cx, |split, cx| split.rescale_code(ratio, cx));
                    cx.notify();
                });
            });
            cx.cx.update(|_| {});
            // Include each paint and the following observer acknowledgments,
            // not just a final stabilized state after linked corrections.
            for _ in 0..3 {
                cx.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    for editor in &split.read(cx).editors {
                        let editor = editor.read(cx);
                        assert_eq!(editor.line_height(), Some(px(height)));
                        assert_eq!(editor.scroll_offset().y, px(-546. * height));
                        assert_eq!(editor.visible_row_range().unwrap().start, 546);
                    }
                });
                notify_measured_editors(&split, cx);
                cx.read(|cx| {
                    let split = split.read(cx);
                    assert_eq!(split.scroll.applied, [px(-546. * height); 2]);
                    assert_eq!(split.scroll.pending, [None; 2]);
                });
            }
        }
    }

    #[gpui::test]
    fn scale_change_before_first_paint_keeps_the_split_change_row(cx: &mut gpui::TestAppContext) {
        cx.update(gpui::init);
        let (probe, cx) = cx.add_window_view(move |window, cx| {
            let split = new(
                long_presentation(),
                "text",
                FocusRing::default(),
                window,
                cx,
            );
            split.update(cx, |split, cx| {
                assert_eq!(split.initial_row, Some(546));
                assert!(
                    split
                        .editors
                        .iter()
                        .all(|editor| editor.read(cx).line_height().is_none())
                );
                split.rescale_code(1.25, cx);
                assert_eq!(split.initial_row, Some(546));
            });
            FontProbe {
                split,
                font_size: px(15.),
            }
        });
        let split = cx.read(|cx| probe.read(cx).split.clone());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            notify_measured_editors(&split, cx);
        }
        cx.read(|cx| {
            let split = split.read(cx);
            assert!(split.initial_row.is_none());
            for editor in &split.editors {
                let editor = editor.read(cx);
                assert_eq!(editor.line_height(), Some(px(23.)));
                assert_eq!(editor.scroll_offset().y, px(-23. * 546.));
                assert_eq!(editor.visible_row_range().unwrap().start, 546);
            }
            assert_eq!(split.scroll.applied, [px(-23. * 546.); 2]);
            assert_eq!(split.scroll.pending, [None; 2]);
        });
    }

    /// Under More contrast each side's emphasized words in Split draw a rule inside their
    /// highlight box's bottom edge, a 1 px stroke as wide as the box, in the side's fitted
    /// colour, removed on Before and added on After, at 13 and 18 pt, in a dark palette, one
    /// whose removed words nearly vanish in colour-blind models, and a light one; no other quad
    /// of a rule's colour and height is painted, so a whole changed line and unchanged text draw
    /// none. Off, no word draws one, and each side's decorations carry no rule.
    #[gpui::test]
    fn more_contrast_rules_each_emphasized_word_in_split(cx: &mut gpui::TestAppContext) {
        use crate::appearance::{
            ThemeChoice, apply_text_sizes, editor_highlights, set_more_contrast,
        };

        struct Root(Entity<SplitView>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(1200.)).h(px(600.)).child(self.0.clone())
            }
        }

        let [before, after, patch] = crate::text::DENSE_HUNK;
        let presentation = Arc::new(prepare(before, after, patch));
        let words = presentation.sides.each_ref().map(|side| side.words.len());
        assert_eq!(words, [4, 4], "four changed words a side");
        cx.update(gpui::init);
        let (root, cx) = cx.add_window_view(|window, cx| {
            Root(new(
                Arc::clone(&presentation),
                "text",
                FocusRing::default(),
                window,
                cx,
            ))
        });
        let split = cx.read(|cx| root.read(cx).0.clone());
        for choice in [
            ThemeChoice::Midnight,
            ThemeChoice::SolarizedDark,
            ThemeChoice::Daylight,
        ] {
            for code in [13, 18] {
                for more in [true, false] {
                    let at = format!("{choice:?} at {code} pt, More contrast {more}");
                    let highlights = cx.update(|window, cx| {
                        set_more_contrast(more, cx);
                        choice.apply(Some(&mut *window), cx);
                        apply_text_sizes(13, code, window, cx);
                        split.update(cx, |split, cx| split.refresh_theme(cx));
                        editor_highlights(cx)
                    });
                    cx.update(|window, cx| window.draw(cx).clear(cx));
                    // Old side first: removed words on Before, added on After.
                    let sides = [
                        (highlights.removed_word, highlights.removed_rule),
                        (highlights.added_word, highlights.added_rule),
                    ];
                    let rules = sides.iter().filter_map(|side| side.1).collect::<Vec<_>>();
                    let (drawn, stray) = cx.update(|window, _| {
                        crate::text::painted_word_rules(window, &sides.map(|side| side.0), &rules)
                    });
                    assert_eq!(stray, 0, "{at}: a rule off every emphasized word's box");
                    for (side, (tint, rule)) in sides.into_iter().enumerate() {
                        let boxes = drawn
                            .iter()
                            .filter(|(_, drawn, _)| *drawn == tint)
                            .collect::<Vec<_>>();
                        assert_eq!(boxes.len(), words[side], "{at}: side {side}'s word boxes");
                        let want = rule.map(|rule| vec![Some(rule)]).unwrap_or_default();
                        for (bounds, _, along) in boxes {
                            assert_eq!(
                                along, &want,
                                "{at}: side {side}'s word box {bounds:?} draws {want:?} inside its bottom edge"
                            );
                        }
                    }
                    if !more {
                        let tints = cx.read(side_tints);
                        assert!(
                            (0..2)
                                .flat_map(|side| presentation.sides[side].decorations(tints[side]))
                                .all(|decoration| decoration.bottom_rule.is_none()),
                            "{at}: no decoration has a rule"
                        );
                    }
                }
            }
        }
    }

    /// Each side of Split carries a changed line's tint across its editor's whole row, so a
    /// replacement reads as one block a side with a straight right edge (decision, 2026-11-05):
    /// a short changed line, a blank one, which drew no tint at all, and one wider than the view
    /// carry it from the text's left edge, where it always started, to the side's visible right
    /// edge, also scrolled to the far right, where the long lines' text ends short of that edge;
    /// at 13 and 18 pt, More contrast off and on, in Midnight and in Tokyo Night, whose More
    /// contrast moves its added line tint. Unchanged lines and Before's filler rows beside the
    /// extra added lines draw no line tint, and each emphasized word drawn keeps its box, with
    /// its rule under More contrast.
    #[gpui::test]
    fn a_changed_line_s_tint_spans_its_row_on_each_split_side(cx: &mut gpui::TestAppContext) {
        use crate::appearance::{
            ThemeChoice, apply_text_sizes, editor_highlights, set_more_contrast,
        };
        use crate::text::RowTint;

        struct Root(Entity<SplitView>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(1200.)).h(px(600.)).child(self.0.clone())
            }
        }

        let [before, after, patch] = crate::text::row_tint_hunk();
        let presentation = Arc::new(prepare(&before, &after, &patch));
        assert_eq!(
            numbers(&presentation, 0),
            [
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                None,
                None,
                Some(5),
                None
            ],
            "Before pads the extra added lines with two filler rows"
        );
        cx.update(gpui::init);
        let (root, cx) = cx.add_window_view(|window, cx| {
            Root(new(
                Arc::clone(&presentation),
                "text",
                FocusRing::default(),
                window,
                cx,
            ))
        });
        let split = cx.read(|cx| root.read(cx).0.clone());
        let editors = cx.read(|cx| split.read(cx).editors.clone());
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
                            split.update(cx, |split, cx| split.refresh_theme(cx));
                            (palette(cx), editor_highlights(cx))
                        });
                        moved |= colors.added_background != chosen;
                        let x = if far_right { -100_000. } else { 0. };
                        for _ in 0..2 {
                            cx.update(|window, cx| {
                                for editor in &editors {
                                    editor.update(cx, |state, cx| {
                                        state.set_scroll_offset(point(px(x), px(0.)), cx)
                                    });
                                }
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
                        let sides = [
                            [
                                RowTint::Untinted,
                                RowTint::ToEdge(removed),
                                RowTint::ToEdge(removed),
                                long(removed),
                                RowTint::Untinted,
                                RowTint::Untinted,
                                RowTint::Untinted,
                            ],
                            [
                                RowTint::Untinted,
                                RowTint::ToEdge(added),
                                RowTint::ToEdge(added),
                                long(added),
                                RowTint::ToEdge(added),
                                RowTint::ToEdge(added),
                                RowTint::Untinted,
                            ],
                        ];
                        for (side, rows) in sides.iter().enumerate() {
                            cx.update(|window, cx| {
                                crate::text::assert_row_tints(
                                    &format!("{at}, side {side}"),
                                    window,
                                    editors[side].read(cx),
                                    [removed, added],
                                    rows,
                                )
                            });
                        }
                        let words = [
                            (highlights.removed_word, highlights.removed_rule),
                            (highlights.added_word, highlights.added_rule),
                        ];
                        let rules = words.iter().filter_map(|word| word.1).collect::<Vec<_>>();
                        let (drawn, stray) = cx.update(|window, _| {
                            crate::text::painted_word_rules(
                                window,
                                &words.map(|word| word.0),
                                &rules,
                            )
                        });
                        assert_eq!(stray, 0, "{at}: a rule off every emphasized word's box");
                        for (bounds, tint, along) in drawn {
                            let rule = words[usize::from(tint == highlights.added_word)].1;
                            let want = rule.map(|rule| vec![Some(rule)]).unwrap_or_default();
                            assert_eq!(along, want, "{at}: the word box {bounds:?}'s rule");
                        }
                    }
                }
            }
        }
        assert!(moved, "More contrast moves Tokyo Night's added line tint");
    }

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

    /// Scrolling a dense diff, for a release build: 2,400 replaced lines,
    /// each with four changed words a side, so every visible line carries a
    /// line decoration and word decorations, in the Diff view and in Split,
    /// at 13 and 18 pt, in Midnight, with More contrast off and on. After
    /// one untimed pass, each of sixty steps of 800 px down the text times
    /// the scroll from the call that sets the offset (Split's first side,
    /// which its observer mirrors to the second) through the frame drawn
    /// after it: the scroll's effects, then layout, prepaint and paint of the
    /// whole window on the test platform, with no GPU, compositor or display
    /// link, and with the test platform's text system, which places glyphs
    /// without shaping a font. The last step must show row 1,000 or later on
    /// both sides, so the steps move the text. Then the same steps over many
    /// one-line changed blocks (`diff_blocks` and `split_blocks`), the slowest
    /// input for the changed lines' row fills: 1,600 unchanged lines, each
    /// followed by a removed and an added line, every other added line blank.
    /// Then the security review's
    /// crafted worst case: 37 replaced pairs of 16,384-byte lines, each with
    /// 81 changed words a side, the most pairs one file's word budget gives
    /// words, in the Diff view at 13 pt with More contrast off and on, Find
    /// closed and then open on `.`, which matches every changed word's
    /// separator, sixty steps between the top and 600 px down. It names no More contrast API beyond the setting, so it measures
    /// the same steps on a build without the changed-word rule.
    #[gpui::test]
    #[ignore = "Opt-in release benchmark: set GITTURTLE_DIFF_SCROLL_BENCH_OUTPUT and run with --release --ignored"]
    fn diff_scroll_release_benchmark(cx: &mut gpui::TestAppContext) {
        use crate::appearance::{ThemeChoice, apply_text_sizes, set_more_contrast};
        const LINES: usize = 2_400;
        const STEPS: usize = 60;
        const STEP: f32 = 800.;
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "performance evidence requires --release"
        );
        let output = std::env::var_os("GITTURTLE_DIFF_SCROLL_BENCH_OUTPUT")
            .expect("the runner supplies the result destination");

        struct Root {
            split: bool,
            diff: Entity<diff_view::DiffView>,
            split_view: Entity<SplitView>,
            crafted: Option<Entity<diff_view::DiffView>>,
        }
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let view = if let Some(crafted) = &self.crafted {
                    crafted.clone().into_any_element()
                } else if self.split {
                    self.split_view.clone().into_any_element()
                } else {
                    self.diff.clone().into_any_element()
                };
                div().w(px(1200.)).h(px(900.)).child(view)
            }
        }

        let line = |index: usize, names: [&str; 4]| {
            format!(
                "call({}{index}, keep, {}{index}, same, {}{index}, still, {}{index});\n",
                names[0], names[1], names[2], names[3]
            )
        };
        let before: String = (0..LINES).map(|i| line(i, ["a", "b", "c", "d"])).collect();
        let after: String = (0..LINES).map(|i| line(i, ["w", "x", "y", "z"])).collect();
        let mut patch = format!("@@ -1,{LINES} +1,{LINES} @@\n");
        for text in before.split_inclusive('\n') {
            patch.push('-');
            patch.push_str(text);
        }
        for text in after.split_inclusive('\n') {
            patch.push('+');
            patch.push_str(text);
        }
        let presentation = PatchPresentation::prepare(&patch);
        let split_presentation =
            Arc::new(SplitPresentation::prepare(&before, &after, &presentation));
        cx.update(gpui::init);
        let mut handles = None;
        let (root, cx) = cx.add_window_view(|window, cx| {
            ThemeChoice::Midnight.apply(Some(&mut *window), cx);
            let (editor, decorations) = crate::text::editor_with_decorations(
                &patch,
                "diff",
                Some(&presentation),
                window,
                cx,
            );
            let diff = diff_view::new(editor.clone(), &presentation, window, cx);
            let split_view = new(
                Arc::clone(&split_presentation),
                "text",
                FocusRing::default(),
                window,
                cx,
            );
            handles = Some((editor, decorations.expect("a prepared patch is decorated")));
            Root {
                split: false,
                diff,
                split_view,
                crafted: None,
            }
        });
        let (diff_editor, decorations) = handles.expect("the window was built");
        let draw =
            |cx: &mut gpui::VisualTestContext| cx.update(|window, cx| window.draw(cx).clear(cx));
        let mut series = Vec::new();
        for split in [false, true] {
            for text in [13, 18] {
                for more in [false, true] {
                    let editor = cx.update(|window, cx| {
                        set_more_contrast(more, cx);
                        ThemeChoice::Midnight.apply(Some(&mut *window), cx);
                        apply_text_sizes(13, text, window, cx);
                        crate::text::refresh_theme(&decorations, &presentation, cx);
                        let split_view = root.read(cx).split_view.clone();
                        split_view.update(cx, |view, cx| view.refresh_theme(cx));
                        root.update(cx, |root, cx| {
                            root.split = split;
                            cx.notify();
                        });
                        if split {
                            split_view.read(cx).editors[0].clone()
                        } else {
                            diff_editor.clone()
                        }
                    });
                    let mut step_ms = Vec::new();
                    for pass in 0..2 {
                        cx.update(|_, cx| {
                            editor.update(cx, |state, cx| {
                                state.set_scroll_offset(point(px(0.), px(0.)), cx)
                            })
                        });
                        draw(cx);
                        draw(cx);
                        for step in 1..=STEPS {
                            let offset = point(px(0.), px(-STEP * step as f32));
                            let started = std::time::Instant::now();
                            cx.update(|_, cx| {
                                editor.update(cx, |state, cx| state.set_scroll_offset(offset, cx))
                            });
                            draw(cx);
                            let elapsed = started.elapsed().as_secs_f64() * 1000.;
                            if pass == 1 {
                                step_ms.push(elapsed);
                            }
                        }
                    }
                    // The steps moved the text, on both of Split's sides.
                    let rows = cx.read(|cx| {
                        let split_view = root.read(cx).split_view.read(cx);
                        let first = |editor: &Entity<EditorState>| {
                            editor.read(cx).visible_row_range().map(|rows| rows.start)
                        };
                        if split {
                            [first(&split_view.editors[0]), first(&split_view.editors[1])]
                        } else {
                            [first(&editor); 2]
                        }
                    });
                    assert!(
                        rows[0].is_some_and(|row| row >= 1_000) && rows[0] == rows[1],
                        "split {split}, {text} pt: the last step shows rows {rows:?}"
                    );
                    let mut sorted = step_ms.clone();
                    sorted.sort_by(f64::total_cmp);
                    let at = |share: f64| {
                        sorted[((share * sorted.len() as f64).ceil() as usize)
                            .clamp(1, sorted.len())
                            - 1]
                    };
                    series.push(serde_json::json!({
                        "surface": if split { "split" } else { "diff" },
                        "text_pt": text,
                        "more_contrast": more,
                        "find_open": false,
                        "step_ms": step_ms,
                        "median_ms": at(0.5),
                        "p95_ms": at(0.95),
                        "max_ms": sorted[sorted.len() - 1],
                    }));
                }
            }
        }
        // Many one-line blocks, the slowest input for the changed lines' row
        // fills: 1,600 unchanged lines, each followed by one removed line and
        // one added line, every other added line blank, so the Diff view
        // draws a one-line removed block and a one-line added block between
        // each pair of unchanged lines, and each Split side a one-line block
        // every other row. The views take the main ones' place in the root,
        // whose series are already recorded, and run the same steps.
        const BLOCK_PAIRS: usize = 1_600;
        let block_before: String = (0..BLOCK_PAIRS)
            .map(|i| format!("keep{i}();\nold{i}();\n"))
            .collect();
        let added = |i: usize| {
            if i.is_multiple_of(2) {
                format!("new{i}();\n")
            } else {
                "\n".to_owned()
            }
        };
        let block_after: String = (0..BLOCK_PAIRS)
            .map(|i| format!("keep{i}();\n{}", added(i)))
            .collect();
        let mut block_patch = format!("@@ -1,{0} +1,{0} @@\n", 2 * BLOCK_PAIRS);
        for i in 0..BLOCK_PAIRS {
            block_patch.push_str(&format!(" keep{i}();\n-old{i}();\n+{}", added(i)));
        }
        let block_presentation = PatchPresentation::prepare(&block_patch);
        let block_split = Arc::new(SplitPresentation::prepare(
            &block_before,
            &block_after,
            &block_presentation,
        ));
        let (block_editor, block_decorations) = cx.update(|window, cx| {
            let (editor, decorations) = crate::text::editor_with_decorations(
                &block_patch,
                "diff",
                Some(&block_presentation),
                window,
                cx,
            );
            let diff = diff_view::new(editor.clone(), &block_presentation, window, cx);
            let split_view = new(
                Arc::clone(&block_split),
                "text",
                FocusRing::default(),
                window,
                cx,
            );
            root.update(cx, |root, cx| {
                root.diff = diff;
                root.split_view = split_view;
                cx.notify();
            });
            (editor, decorations.expect("a prepared patch is decorated"))
        });
        for split in [false, true] {
            for text in [13, 18] {
                for more in [false, true] {
                    let editor = cx.update(|window, cx| {
                        set_more_contrast(more, cx);
                        ThemeChoice::Midnight.apply(Some(&mut *window), cx);
                        apply_text_sizes(13, text, window, cx);
                        crate::text::refresh_theme(&block_decorations, &block_presentation, cx);
                        let split_view = root.read(cx).split_view.clone();
                        split_view.update(cx, |view, cx| view.refresh_theme(cx));
                        root.update(cx, |root, cx| {
                            root.split = split;
                            cx.notify();
                        });
                        if split {
                            split_view.read(cx).editors[0].clone()
                        } else {
                            block_editor.clone()
                        }
                    });
                    let mut step_ms = Vec::new();
                    for pass in 0..2 {
                        cx.update(|_, cx| {
                            editor.update(cx, |state, cx| {
                                state.set_scroll_offset(point(px(0.), px(0.)), cx)
                            })
                        });
                        draw(cx);
                        draw(cx);
                        for step in 1..=STEPS {
                            let offset = point(px(0.), px(-STEP * step as f32));
                            let started = std::time::Instant::now();
                            cx.update(|_, cx| {
                                editor.update(cx, |state, cx| state.set_scroll_offset(offset, cx))
                            });
                            draw(cx);
                            let elapsed = started.elapsed().as_secs_f64() * 1000.;
                            if pass == 1 {
                                step_ms.push(elapsed);
                            }
                        }
                    }
                    // The steps moved the text, on both of Split's sides.
                    let rows = cx.read(|cx| {
                        let split_view = root.read(cx).split_view.read(cx);
                        let first = |editor: &Entity<EditorState>| {
                            editor.read(cx).visible_row_range().map(|rows| rows.start)
                        };
                        if split {
                            [first(&split_view.editors[0]), first(&split_view.editors[1])]
                        } else {
                            [first(&editor); 2]
                        }
                    });
                    assert!(
                        rows[0].is_some_and(|row| row >= 1_000) && rows[0] == rows[1],
                        "blocks, split {split}, {text} pt: the last step shows rows {rows:?}"
                    );
                    let mut sorted = step_ms.clone();
                    sorted.sort_by(f64::total_cmp);
                    let at = |share: f64| {
                        sorted[((share * sorted.len() as f64).ceil() as usize)
                            .clamp(1, sorted.len())
                            - 1]
                    };
                    series.push(serde_json::json!({
                        "surface": if split { "split_blocks" } else { "diff_blocks" },
                        "text_pt": text,
                        "more_contrast": more,
                        "find_open": false,
                        "step_ms": step_ms,
                        "median_ms": at(0.5),
                        "p95_ms": at(0.95),
                        "max_ms": sorted[sorted.len() - 1],
                    }));
                }
            }
        }
        // The crafted worst case: 37 pairs of 16,384-byte lines with 81
        // changed words a side, the most pairs one file's word budget gives
        // words (`text::WordBudget`), as the security review's probe made them.
        const CRAFTED_PAIRS: usize = 37;
        const CRAFTED_WORDS: usize = 81;
        const CRAFTED_STEP: f32 = 600.;
        let shared = "A".repeat(16 * 1024 - 2 * CRAFTED_WORDS - 2);
        let mut crafted_patch = format!("@@ -1,{CRAFTED_PAIRS} +1,{CRAFTED_PAIRS} @@\n");
        for (sign, word) in [('-', "x."), ('+', "y.")] {
            for _ in 0..CRAFTED_PAIRS {
                crafted_patch.push(sign);
                crafted_patch.push_str(&shared);
                crafted_patch.push_str(&word.repeat(CRAFTED_WORDS));
                crafted_patch.push('\n');
            }
        }
        let crafted = PatchPresentation::prepare(&crafted_patch);
        let (crafted_editor, crafted_decorations) = cx.update(|window, cx| {
            let (editor, decorations) = crate::text::editor_with_decorations(
                &crafted_patch,
                "diff",
                Some(&crafted),
                window,
                cx,
            );
            let view = diff_view::new(editor.clone(), &crafted, window, cx);
            root.update(cx, |root, cx| {
                root.crafted = Some(view);
                cx.notify();
            });
            (editor, decorations.expect("a prepared patch is decorated"))
        });
        draw(cx);
        for find in [false, true] {
            if find {
                let focus = cx.update(|_, cx| crafted_editor.focus_handle(cx));
                cx.update(|window, cx| window.focus(&focus, cx));
                cx.dispatch_action(gpui::component::input::Search);
                cx.simulate_input(".");
                cx.run_until_parked();
            }
            for more in [false, true] {
                cx.update(|window, cx| {
                    set_more_contrast(more, cx);
                    ThemeChoice::Midnight.apply(Some(&mut *window), cx);
                    apply_text_sizes(13, 13, window, cx);
                    crate::text::refresh_theme(&crafted_decorations, &crafted, cx);
                });
                let mut step_ms = Vec::new();
                for pass in 0..2 {
                    cx.update(|_, cx| {
                        crafted_editor.update(cx, |state, cx| {
                            state.set_scroll_offset(point(px(0.), px(0.)), cx)
                        })
                    });
                    draw(cx);
                    draw(cx);
                    for step in 1..=STEPS {
                        let y = if step % 2 == 1 { -CRAFTED_STEP } else { 0. };
                        let started = std::time::Instant::now();
                        cx.update(|_, cx| {
                            crafted_editor.update(cx, |state, cx| {
                                state.set_scroll_offset(point(px(0.), px(y)), cx)
                            })
                        });
                        draw(cx);
                        let elapsed = started.elapsed().as_secs_f64() * 1000.;
                        if pass == 1 {
                            step_ms.push(elapsed);
                        }
                        if pass == 1 && step == 1 {
                            let row = cx.read(|cx| {
                                crafted_editor
                                    .read(cx)
                                    .visible_row_range()
                                    .map(|rows| rows.start)
                            });
                            assert!(
                                row.is_some_and(|row| row > 0),
                                "find {find}: a step down moves the crafted text, at row {row:?}"
                            );
                        }
                    }
                }
                let mut sorted = step_ms.clone();
                sorted.sort_by(f64::total_cmp);
                let at = |share: f64| {
                    sorted
                        [((share * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len()) - 1]
                };
                series.push(serde_json::json!({
                    "surface": "diff_crafted",
                    "text_pt": 13,
                    "more_contrast": more,
                    "find_open": find,
                    "step_ms": step_ms,
                    "median_ms": at(0.5),
                    "p95_ms": at(0.95),
                    "max_ms": sorted[sorted.len() - 1],
                }));
            }
        }
        let result = serde_json::json!({
            "schema": 1,
            "palette": "Midnight",
            "changed_lines": LINES,
            "changed_words_per_line_a_side": 4,
            "steps": STEPS,
            "step_px": STEP,
            "window": [1200, 900],
            "crafted": {
                "pairs": CRAFTED_PAIRS,
                "line_bytes": 16 * 1024 - 1,
                "changed_words_per_line_a_side": CRAFTED_WORDS,
                "step_px": CRAFTED_STEP,
                "find_query": ".",
            },
            "blocks": {
                "pairs": BLOCK_PAIRS,
                "shape": "an unchanged line, then one removed line and one added line, every other added line blank: one-line changed blocks between unchanged lines",
                "steps": STEPS,
                "step_px": STEP,
            },
            "warmup_passes": 1,
            "boundary": "One scroll step on the UI thread: from before the call that sets the editor's scroll offset (Split's first side, mirrored to the second by its observer as the update's effects flush) through the return of the next `window.draw`: the scroll's effects, layout, prepaint and paint of the whole window on the test platform (no GPU, compositor or display link; the test text system places glyphs without shaping a font).",
            "series": series,
        });
        std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        cx.update(|window, cx| apply_text_sizes(13, 12, window, cx));
    }
}
