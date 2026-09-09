//! Unified patch presentation with a separate old/new line-number gutter.
//! The editor retains the exact patch, so selection, copying, and search never
//! include presentation-only numbers. No Git reads happen in this component.

use crate::{
    appearance::palette,
    partial_view::{PartialActions, PartialRow},
    text::PatchPresentation,
};
use gitturtle_core::{ChangeArea, PartialDiff, PartialSelection};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Bounds, ClickEvent, ContentMask, Context, Entity, EventEmitter,
    InteractiveElement, IntoElement, ParentElement, Pixels, Point, Render, SharedString, Styled,
    Subscription, TextAlign, TextRun, Window, canvas,
    component::{
        Disableable, Sizable, Theme,
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        input::EditorState,
    },
    div, fill, point, px, relative, rgb, size,
};
use std::{cell::Cell, collections::BTreeSet, ops::Range, rc::Rc, sync::Arc};

const FONT_SIZE: f32 = 12.;
const CELL_PADDING: f32 = 9.;
const ACTION_WIDTH: f32 = 28.;

pub enum DiffViewEvent {
    ApplyPartial {
        diff: Arc<PartialDiff>,
        selection: PartialSelection,
    },
}

pub struct DiffView {
    editor: Entity<EditorState>,
    rows: Arc<[LineNumbers]>,
    change_rows: Arc<[usize]>,
    change_index: Option<usize>,
    column_width: f32,
    single_column: bool,
    label: SharedString,
    partial: Option<Arc<PartialActions>>,
    selection: LineSelection,
    partial_busy: bool,
    viewport: Viewport,
    pending_scroll: Rc<PendingScroll>,
    gutter_origin: Rc<Cell<Option<Point<Pixels>>>>,
    _subscription: Subscription,
}

impl EventEmitter<DiffViewEvent> for DiffView {}

impl DiffView {
    pub fn suspend_partial(&mut self, cx: &mut Context<Self>) {
        self.partial_busy = true;
        self.selection = LineSelection::default();
        cx.notify();
    }

    pub fn navigate_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        let state = self.editor.read(cx);
        let visible = state.visible_row_range().map_or(0, |range| range.start);
        let Some(index) =
            crate::text_review::next_change(&self.change_rows, self.change_index, visible, forward)
        else {
            return;
        };
        let Some(height) = state.line_height() else {
            return;
        };
        self.change_index = Some(index);
        let offset = point(
            state.scroll_offset().x,
            -height * self.change_rows[index].saturating_sub(2) as f32,
        );
        self.editor
            .update(cx, |state, cx| state.set_scroll_offset(offset, cx));
        cx.notify();
    }

    pub fn find_header_height(&self, cx: &App) -> Pixels {
        crate::editor_find::panel_height(&self.editor, cx)
    }

    pub fn refresh(
        &mut self,
        presentation: &PatchPresentation,
        partial: Option<Arc<PartialActions>>,
        cx: &mut Context<Self>,
    ) {
        self.refresh_numbered(
            Arc::clone(&presentation.rows),
            presentation.column_width,
            cx,
        );
        self.change_rows = Arc::clone(&presentation.change_rows);
        self.change_index = None;
        self.partial = partial;
        self.selection = LineSelection::default();
        self.partial_busy = false;
    }

    pub fn refresh_numbered(
        &mut self,
        rows: Arc<[LineNumbers]>,
        column_width: f32,
        cx: &mut Context<Self>,
    ) {
        self.rows = rows;
        self.column_width = column_width;
        self.pending_scroll = Rc::default();
        self.gutter_origin = Rc::default();
        cx.notify();
    }
}

pub fn new_partial(
    editor: Entity<EditorState>,
    presentation: &PatchPresentation,
    partial: Arc<PartialActions>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<DiffView> {
    let view = new(editor, presentation, window, cx);
    view.update(cx, |view, _| view.partial = Some(partial));
    view
}

/// Wrap an existing patch editor with metadata prepared for its unchanged text.
/// Folding/wrapping are disabled so each patch line has exactly one gutter row;
/// the editor remains responsible for input and copy. No patch parsing occurs.
pub fn new(
    editor: Entity<EditorState>,
    presentation: &PatchPresentation,
    window: &mut Window,
    cx: &mut App,
) -> Entity<DiffView> {
    let view = new_numbered(
        editor,
        Arc::clone(&presentation.rows),
        presentation.column_width,
        false,
        "Read-only unified patch",
        window,
        cx,
    );
    view.update(cx, |view, _| {
        view.change_rows = Arc::clone(&presentation.change_rows)
    });
    view
}

pub fn new_numbered(
    editor: Entity<EditorState>,
    rows: Arc<[LineNumbers]>,
    column_width: f32,
    single_column: bool,
    label: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<DiffView> {
    editor.update(cx, |state, cx| {
        state.set_line_number(false, window, cx);
        state.set_soft_wrap(false, window, cx);
        state.set_folding(false, window, cx);
    });
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
            change_rows: Arc::from([]),
            change_index: None,
            column_width,
            single_column,
            label: label.into(),
            partial: None,
            selection: LineSelection::default(),
            partial_busy: false,
            viewport,
            pending_scroll: Rc::default(),
            gutter_origin: Rc::default(),
            _subscription: subscription,
        }
    })
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.clone();
        let scroll_editor = self.editor.clone();
        let pending_scroll = Rc::clone(&self.pending_scroll);
        let painted_scroll = Rc::clone(&self.pending_scroll);
        let gutter_origin = Rc::clone(&self.gutter_origin);
        let rows = Arc::clone(&self.rows);
        let row_count = rows.len();
        let column_width = self.column_width * crate::appearance::code_scale();
        let single_column = self.single_column;
        let action_width = if self.partial.is_some() {
            f32::from(crate::appearance::ui_size(ACTION_WIDTH))
        } else {
            0.
        };
        let width = action_width + column_width * if single_column { 1. } else { 2. };
        let controls = self.render_partial_controls(width, cx);
        let content = div()
            .size_full()
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .pl(px(width))
            .font_family(Theme::global(cx).mono_font_family.clone())
            .text_size(crate::appearance::code_text())
            // Paint the editor first. It records its current scrolled bounds
            // during paint; the later gutter canvas reads those same-frame
            // coordinates, avoiding a one-frame lag during wheel scrolling.
            .child(
                crate::editor_find::Editor::new(&self.editor)
                    .h(relative(1.))
                    .readonly(true)
                    .bordered(false)
                    .aria_label(self.label.clone())
                    .text_size(crate::appearance::code_text()),
            )
            .child(
                div()
                    .id("diff-line-gutter")
                    .absolute()
                    .left_0()
                    .top_0()
                    .w(px(width))
                    .h_full()
                    .on_scroll_wheel(move |event, _, cx| {
                        scroll_editor.update(cx, |state, cx| {
                            let (Some(line_height), Some(text_bounds)) =
                                (state.line_height(), state.text_bounds())
                            else {
                                return;
                            };
                            let delta = event.delta.pixel_delta(line_height);
                            let old_offset = state.scroll_offset();
                            // Match Editor's default trailing space; bound the deferred
                            // scroll before layout, which applies its own final clamp.
                            let bottom_padding =
                                (text_bounds.size.height / 2.).max(line_height * 3.);
                            let content_height = (line_height * row_count as f32 + bottom_padding)
                                .max(text_bounds.size.height);
                            let min_y =
                                (state.input_bounds().size.height - content_height).min(px(0.));
                            if let Some(next) = pending_scroll.advance(old_offset, delta.y, min_y) {
                                // The gutter stays fixed horizontally, including when
                                // a trackpad gesture includes both axes.
                                state.set_scroll_offset(next, cx);
                                cx.stop_propagation();
                            }
                        });
                    })
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, cx| {
                                // Editor has painted and applied its deferred offset.
                                // Notifications alone do not mark that boundary.
                                painted_scroll.clear();
                                gutter_origin.set(Some(bounds.origin));
                                if action_width > 0. {
                                    window.paint_quad(fill(bounds, rgb(palette(cx).panel)));
                                }
                                let numbered_bounds = Bounds::new(
                                    point(bounds.origin.x + px(action_width), bounds.origin.y),
                                    size(bounds.size.width - px(action_width), bounds.size.height),
                                );
                                paint_gutter(
                                    &editor,
                                    &rows,
                                    column_width,
                                    single_column,
                                    numbered_bounds,
                                    window,
                                    cx,
                                );
                            },
                        )
                        .size_full(),
                    )
                    .children(controls),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(content)
            .when(!self.selection.ids.is_empty(), |el| {
                el.child(self.render_partial_selection(cx))
            })
    }
}

impl DiffView {
    fn apply_partial(&mut self, ids: Vec<usize>, cx: &mut Context<Self>) {
        if self.partial_busy || ids.is_empty() {
            return;
        }
        let Some(partial) = &self.partial else {
            return;
        };
        let diff = Arc::clone(&partial.diff);
        self.partial_busy = true;
        cx.emit(DiffViewEvent::ApplyPartial {
            diff,
            selection: PartialSelection::Lines(ids),
        });
        cx.notify();
    }

    fn render_partial_controls(&self, width: f32, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(partial) = &self.partial else {
            return Vec::new();
        };
        let state = self.editor.read(cx);
        let (Some(bounds), Some(line_height), Some(visible)) = (
            state.text_bounds(),
            state.line_height(),
            state.visible_row_range(),
        ) else {
            return Vec::new();
        };
        let input = state.input_bounds();
        let top = bounds.origin.y - input.origin.y;
        let origin = self.gutter_origin.get().unwrap_or(input.origin);
        let action = if partial.diff.area == ChangeArea::Staged {
            "Unstage"
        } else {
            "Stage"
        };
        let mut elements = Vec::new();
        for row in visible.start.min(partial.rows.len())..visible.end.min(partial.rows.len()) {
            let y = top + line_height * row as f32;
            match &partial.rows[row] {
                PartialRow::None => {}
                PartialRow::Hunk(ids) => {
                    let ids = Arc::clone(ids);
                    elements.push(
                        div()
                            .absolute()
                            .top(y + px(1.))
                            .left(px(2.))
                            .w(px(width - 4.))
                            .h(line_height - px(2.))
                            .child(
                                Button::new(("partial-hunk", row))
                                    .small()
                                    .w_full()
                                    .h(line_height - px(2.))
                                    .px_1()
                                    .rounded(px(3.))
                                    .text_size(crate::appearance::ui_text(10.))
                                    .label(format!("{action} hunk"))
                                    .accessibility_label(format!(
                                        "{action} hunk at patch line {}",
                                        row + 1
                                    ))
                                    .disabled(self.partial_busy)
                                    .tooltip(format!(
                                        "{action} all {} changed lines in this hunk",
                                        ids.len()
                                    ))
                                    .on_click(cx.listener(move |this, event, _, cx| {
                                        if this.click_matches_row(event, row, cx) {
                                            this.apply_partial(ids.to_vec(), cx);
                                        }
                                    })),
                            )
                            .into_any_element(),
                    );
                }
                PartialRow::Change { id, added, line } => {
                    let id = *id;
                    elements.push(
                        div()
                            .absolute()
                            .top(y)
                            .left(px(5.))
                            .w(crate::appearance::ui_size(ACTION_WIDTH - 6.))
                            .h(line_height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Checkbox::new(("partial-line", id))
                                    .small()
                                    .checked(self.selection.ids.contains(&id))
                                    .disabled(self.partial_busy)
                                    .accessibility_label(format!(
                                        "Select {} line {line} for {}",
                                        if *added { "added" } else { "removed" },
                                        action.to_lowercase()
                                    ))
                                    .tooltip(
                                        "Select this changed line. Shift-click selects a range.",
                                    )
                                    .on_click(cx.listener(move |this, checked, window, cx| {
                                        if this.partial_busy {
                                            return;
                                        }
                                        this.selection.toggle(
                                            id,
                                            *checked,
                                            window.modifiers().shift,
                                        );
                                        cx.notify();
                                    })),
                            )
                            .into_any_element(),
                    );
                }
            }
        }
        // Match the input's clipping region, including when editor find moves
        // the first visible source line down inside the outer comparison.
        vec![
            div()
                .absolute()
                .left_0()
                .top(input.origin.y - origin.y)
                .w(px(width))
                .h(input.size.height)
                .overflow_hidden()
                .children(elements)
                .into_any_element(),
        ]
    }

    fn click_matches_row(&self, event: &ClickEvent, row: usize, cx: &App) -> bool {
        if matches!(event, ClickEvent::Keyboard(_)) {
            return true;
        }
        let state = self.editor.read(cx);
        let (Some(bounds), Some(height)) = (state.text_bounds(), state.line_height()) else {
            return false;
        };
        let y = event.position().y;
        let top = bounds.origin.y + height * row as f32;
        // Scroll offsets are applied while painting the editor. Reject a click
        // delivered while a control from the previous geometry is relocating.
        y >= top && y < top + height
    }

    fn render_partial_selection(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let action = if self
            .partial
            .as_ref()
            .is_some_and(|partial| partial.diff.area == ChangeArea::Staged)
        {
            "Unstage"
        } else {
            "Stage"
        };
        let count = self.selection.ids.len();
        div()
            .flex_shrink_0()
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .bg(rgb(palette.panel))
            .border_t_1()
            .border_color(rgb(palette.border))
            .child(
                div()
                    .flex_1()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(palette.muted))
                    .child(format!(
                        "{count} {} selected",
                        if count == 1 { "line" } else { "lines" }
                    )),
            )
            .child(
                Button::new("clear-partial-selection")
                    .small()
                    .label("Clear")
                    .disabled(self.partial_busy)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.selection = LineSelection::default();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("apply-partial-selection")
                    .small()
                    .primary()
                    .label(format!("{action} selected lines"))
                    .disabled(self.partial_busy)
                    .tooltip("For a replacement, select both the removed and added lines.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.apply_partial(this.selection.ids.iter().copied().collect(), cx)
                    })),
            )
            .into_any_element()
    }
}

#[derive(Default)]
struct LineSelection {
    ids: BTreeSet<usize>,
    anchor: Option<usize>,
}

impl LineSelection {
    fn toggle(&mut self, id: usize, checked: bool, extend: bool) {
        let anchor = self.anchor.filter(|_| extend).unwrap_or(id);
        for id in anchor.min(id)..=anchor.max(id) {
            if checked {
                self.ids.insert(id);
            } else {
                self.ids.remove(&id);
            }
        }
        self.anchor = Some(id);
    }
}

#[derive(Default)]
struct PendingScroll(Cell<Option<Point<Pixels>>>);

impl PendingScroll {
    fn advance(
        &self,
        applied: Point<Pixels>,
        delta_y: Pixels,
        min_y: Pixels,
    ) -> Option<Point<Pixels>> {
        // Editor's public setter defers until layout, so wheel bursts must
        // accumulate against the pending position instead of the last frame.
        let previous_y = self.0.get().map_or(applied.y, |offset| offset.y);
        let next_y = (previous_y + delta_y).clamp(min_y, px(0.));
        if next_y == previous_y {
            return None;
        }
        let next = point(applied.x, next_y);
        self.0.set(Some(next));
        Some(next)
    }

    fn clear(&self) {
        self.0.set(None);
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
    single_column: bool,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let palette = palette(cx);
    window.paint_quad(fill(bounds, rgb(palette.panel)));
    for x in [column_width - 1., column_width * 2. - 1.]
        .into_iter()
        .take(if single_column { 1 } else { 2 })
    {
        window.paint_quad(fill(
            Bounds::new(
                point(bounds.origin.x + px(x), bounds.origin.y),
                size(px(1.), bounds.size.height),
            ),
            rgb(palette.border),
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
                _ if single_column => (palette.muted, None),
                (Some(_), None) => (palette.removed, Some(palette.removed_background)),
                (None, Some(_)) => (palette.added, Some(palette.added_background)),
                _ => (palette.line_number, None),
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
            for (column, number) in [numbers.old, numbers.new]
                .into_iter()
                .take(if single_column { 1 } else { 2 })
                .enumerate()
            {
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
                let line = window.text_system().shape_line(
                    label,
                    crate::appearance::code_text(),
                    &[run],
                    None,
                );
                let right = bounds.origin.x
                    + px(column_width * (column + 1) as f32
                        - CELL_PADDING * crate::appearance::code_scale());
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
pub(crate) struct LineNumbers {
    pub(crate) old: Option<u64>,
    pub(crate) new: Option<u64>,
}

/// Pure worker-side preparation; the UI constructor only shares these results.
pub(crate) fn prepare_gutter(patch: &str) -> (Arc<[LineNumbers]>, f32) {
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
    (rows, column_width)
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

    #[test]
    fn changed_line_selection_supports_disjoint_edits_and_shift_ranges() {
        let mut selection = LineSelection::default();
        selection.toggle(2, true, false);
        selection.toggle(5, true, true);
        assert_eq!(
            selection.ids.iter().copied().collect::<Vec<_>>(),
            [2, 3, 4, 5]
        );
        selection.toggle(10, true, false);
        assert_eq!(
            selection.ids.iter().copied().collect::<Vec<_>>(),
            [2, 3, 4, 5, 10]
        );
        selection.toggle(8, false, true);
        assert_eq!(
            selection.ids.iter().copied().collect::<Vec<_>>(),
            [2, 3, 4, 5]
        );
        selection.toggle(3, false, false);
        assert_eq!(selection.ids.iter().copied().collect::<Vec<_>>(), [2, 4, 5]);
    }

    fn n(old: Option<u64>, new: Option<u64>) -> LineNumbers {
        LineNumbers { old, new }
    }

    #[test]
    fn wheel_burst_accumulates_until_paint_and_can_reverse_to_applied_position() {
        let pending = PendingScroll::default();
        // Every event in this burst reads the same last-painted editor offset.
        let applied = point(px(-40.), px(0.));
        let min_y = px(-100.);
        assert_eq!(
            pending.advance(applied, px(-10.), min_y),
            Some(point(px(-40.), px(-10.)))
        );
        assert_eq!(
            pending.advance(applied, px(-15.), min_y),
            Some(point(px(-40.), px(-25.)))
        );
        assert_eq!(pending.advance(applied, px(25.), min_y), Some(applied));
        assert_eq!(
            pending.advance(applied, px(-1000.), min_y),
            Some(point(px(-40.), min_y))
        );
        assert_eq!(pending.advance(applied, px(-1.), min_y), None);
        assert_eq!(
            pending.advance(applied, px(10.), min_y),
            Some(point(px(-40.), px(-90.)))
        );

        pending.clear();
        // A later frame may have a different offset after Editor's own clamp
        // or another input path. The next gesture starts from that actual state.
        assert_eq!(
            pending.advance(point(px(-8.), px(-60.)), px(10.), min_y),
            Some(point(px(-8.), px(-50.)))
        );
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
