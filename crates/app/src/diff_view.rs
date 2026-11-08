//! Unified patch presentation with a separate old/new line-number gutter.
//! The editor retains the exact patch, so selection, copying, and search never
//! include presentation-only numbers. No Git reads happen in this component.
//!
//! Where it offers a working diff's partial actions, its changed lines are
//! selected from the keyboard through the text's caret, never a Tab stop per
//! line: Space toggles the changed lines under the caret or the text
//! selection ([`DiffView::toggle_at_caret`]), the caret line's checkbox draws
//! a mark, and the view announces each toggle from its own live region.

use crate::{
    appearance::{CodeFont, palette},
    native_accessibility::LiveRegion as _,
    partial_view::{PartialActions, PartialRow},
    text::PatchPresentation,
};
use gitturtle_core::{ChangeArea, PartialDiff, PartialSelection};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Bounds, ClickEvent, ContentMask, Context, Entity, EventEmitter,
    FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Point,
    Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription, TextAlign,
    TextRun, Window, canvas,
    component::{
        Disableable, Sizable,
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        input::{EditorState, Position, RopeExt},
    },
    div, fill, point, px, relative, rgb, size,
};
use std::{
    cell::Cell,
    collections::BTreeSet,
    ops::Range,
    rc::Rc,
    sync::{Arc, OnceLock},
    time::Instant,
};

const FONT_SIZE: f32 = 12.;
/// The space beside a line number inside its gutter column, at the default code size; a
/// number ends this far inside its column's right edge.
pub(crate) const CELL_PADDING: f32 = 9.;
const ACTION_WIDTH: f32 = 28.;

/// Opt-in observations of existing input and paint work. This deliberately does
/// not request a frame: idle samples must not create the work being measured.
pub(crate) fn scroll_trace_enabled() -> bool {
    scroll_trace_start().is_some()
}

fn scroll_trace_start() -> Option<&'static Instant> {
    static START: OnceLock<Option<Instant>> = OnceLock::new();
    START
        .get_or_init(|| std::env::var_os("GITTURTLE_TRACE_SCROLL").map(|_| Instant::now()))
        .as_ref()
}

pub(crate) fn trace_scroll(kind: &str, details: std::fmt::Arguments<'_>) {
    if let Some(start) = scroll_trace_start() {
        eprintln!(
            "gitturtle.scroll kind={kind} time_us={} {details}",
            start.elapsed().as_micros()
        );
    }
}

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
    /// What the live region says of the latest Space toggle, with the
    /// count that names its node afresh for each, so each is announced
    /// ([`Self::render_announcement`]).
    announcement: Option<(usize, SharedString)>,
    announcements: usize,
    /// The selection bar's Stage / Unstage selected lines and Clear, the
    /// Tab stops after the text, whose press gives focus back to the text
    /// ([`Self::render_partial_selection`]).
    apply_focus: FocusHandle,
    clear_focus: FocusHandle,
    /// The room that bar took in the last frame, so the text keeps the
    /// caret's line whole above it in the frame it comes, goes or changes
    /// height ([`BarRoom::keep_caret_line`]).
    bar_room: Rc<BarRoom>,
    viewport: Viewport,
    pending_scroll: Rc<PendingScroll>,
    gutter_origin: Rc<Cell<Option<Point<Pixels>>>>,
    /// The other Split side's editor, which shares one Find bar with this
    /// view's, drawn across both sides, and which side this view is
    /// ([`Self::share_find_with`]).
    find_partner: Option<(Entity<EditorState>, crate::editor_find::Side)>,
    _subscription: Subscription,
}

impl EventEmitter<DiffViewEvent> for DiffView {}

impl DiffView {
    /// The patch editor's focus, which a scrolling pane around the view
    /// reveals when the keyboard moves onto it.
    pub fn focus_handle(&self, cx: &App) -> gpui_kit::FocusHandle {
        gpui_kit::Focusable::focus_handle(self.editor.read(cx), cx)
    }
    /// Whether the partial actions are suspended, as while a text review
    /// or a partial write runs.
    #[cfg(test)]
    pub(crate) fn partial_suspended(&self) -> bool {
        self.partial_busy
    }

    pub fn suspend_partial(&mut self, cx: &mut Context<Self>) {
        self.partial_busy = true;
        self.selection = LineSelection::default();
        self.announcement = None;
        cx.notify();
    }

    /// Whether the text holds focus while the per-line controls show, and
    /// then whether they unstage: Space then selects lines, which the status
    /// bar names (`GitTurtle::hints_for`).
    pub(crate) fn line_keys(&self, window: &Window, cx: &App) -> Option<bool> {
        let partial = self.partial.as_ref()?;
        self.focus_handle(cx)
            .is_focused(window)
            .then_some(partial.diff.area == ChangeArea::Staged)
    }

    /// The row the text's caret is on.
    fn caret_row(&self, cx: &App) -> usize {
        let state = self.editor.read(cx);
        state.text().offset_to_point(state.cursor()).row
    }

    /// Space in the focused text while the per-line controls show: it
    /// toggles the changed lines under the caret or the text selection
    /// ([`Self::toggle_at_caret`]), once per press, and types nothing; the
    /// read-only text took it for a space it refused. Elsewhere, as in Find's
    /// field, the key goes on.
    fn line_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "space"
            || event.keystroke.modifiers.modified()
            || self.partial.is_none()
            || !self.focus_handle(cx).is_focused(window)
        {
            return;
        }
        cx.stop_propagation();
        if !event.is_held {
            self.toggle_at_caret(cx);
        }
    }

    /// Toggles every changed line from the text selection's first row to its
    /// last, the caret's row alone where nothing is selected: all selected
    /// where any is not, else all cleared, as clicks on their checkboxes
    /// would, the caret's line becoming the anchor a Shift+click extends
    /// from. Context, hunk headers and the file's header select nothing, and
    /// a hunk's lines are reached by selecting them. The live region says
    /// what changed.
    fn toggle_at_caret(&mut self, cx: &mut Context<Self>) {
        if self.partial_busy {
            return;
        }
        let Some(partial) = self.partial.clone() else {
            return;
        };
        let state = self.editor.read(cx);
        let range = state.selected_range();
        let text = state.text();
        let rows = text.offset_to_point(range.start).row..=text.offset_to_point(range.end).row;
        let caret = text.offset_to_point(state.cursor()).row;
        let changes: Vec<_> = rows
            .filter_map(|row| match partial.rows.get(row)? {
                PartialRow::Change { id, added, line } => Some((row, *id, *added, *line)),
                _ => None,
            })
            .collect();
        let Some(&(_, last, added, line)) = changes.last() else {
            return;
        };
        let select = changes
            .iter()
            .any(|(_, id, ..)| !self.selection.ids.contains(id));
        for (_, id, ..) in &changes {
            if select {
                self.selection.ids.insert(*id);
            } else {
                self.selection.ids.remove(id);
            }
        }
        self.selection.anchor = Some(
            changes
                .iter()
                .find(|(row, ..)| *row == caret)
                .map_or(last, |(_, id, ..)| *id),
        );
        let action = self.action().to_lowercase();
        let state = if select {
            "selected"
        } else {
            "no longer selected"
        };
        let said = match &changes[..] {
            [_] => format!(
                "{} line {line} {state} to {action}",
                if added { "Added" } else { "Removed" }
            ),
            many => format!(
                "{} {state} to {action}",
                crate::counted(many.len(), "line", "lines")
            ),
        };
        self.announcements = self.announcements.wrapping_add(1);
        self.announcement = Some((self.announcements, said.into()));
        cx.notify();
    }

    /// The partial actions' verb: Unstage in the staged area, else Stage.
    fn action(&self) -> &'static str {
        if self
            .partial
            .as_ref()
            .is_some_and(|partial| partial.diff.area == ChangeArea::Staged)
        {
            "Unstage"
        } else {
            "Stage"
        }
    }

    /// The polite status node saying the latest toggle, out of flow in a
    /// 1 px box at the view's top-left, over the gutter's corner where no
    /// control lies, its text its name and its value, as AT-SPI reads the
    /// one and macOS the other. Each toggle draws a node of its own, which
    /// screen readers announce as it appears, as the window's copy
    /// announcement does (`copy_feedback::announcement`).
    fn render_announcement(&self) -> Option<AnyElement> {
        let (said, text) = self.announcement.clone()?;
        Some(
            crate::native_accessibility::recorded(
                div()
                    .id(SharedString::from(format!(
                        "partial-line-announcement-{said}"
                    )))
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(px(1.))
                    .overflow_hidden()
                    .role(Role::Status)
                    .polite()
                    .aria_label(text.clone())
                    .aria_value(text),
            )
            .into_any_element(),
        )
    }

    /// Gives focus back to the text after a press of the selection bar's
    /// control holding `focus`, which goes with the selection: focus never
    /// rests on a control that is gone. A pointer press elsewhere leaves
    /// focus where it is.
    fn return_focus(&self, focus: &FocusHandle, window: &mut Window, cx: &mut App) {
        if focus.is_focused(window) {
            window.focus(&self.focus_handle(cx), cx);
        }
    }

    /// Moves the caret to the start of `row`, focusing the text, as a click
    /// on that row's checkbox does: Space goes on from there. The text's
    /// offset shows the line's start first, as the move scrolls it to: a
    /// selection bar the click brings scrolls the text on from that offset
    /// ([`BarRoom::keep_caret_line`]), in place of the move's own scroll.
    fn move_caret_to(&self, row: usize, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |state, cx| {
            state.set_scroll_offset(point(px(0.), state.scroll_offset().y), cx);
            state.set_cursor_position(Position::new(row as u32, 0), window, cx)
        });
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

    /// Shares one Find bar with `partner`'s editor, as Split's sides do, this
    /// view being `side`: the split view draws it across both, so this view
    /// draws none ([`crate::editor_find::Editor::sharing_find_with`]).
    pub fn share_find_with(
        &mut self,
        partner: Entity<EditorState>,
        side: crate::editor_find::Side,
    ) {
        self.find_partner = Some((partner, side));
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
        self.announcement = None;
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
        // The gutter keeps the origin it last painted at: the partial
        // actions are placed from it ([`Self::render_partial_controls`]).
        // Forgotten here, a view refreshed while hidden, as a text review
        // released as Quick Open's inspection covers it, drew
        // its first frame back with each action above its row (8 px at the
        // default size), where it stayed until something else drew, and the
        // pointer's arrival drew it in place, from under the pointer: Stage
        // looked enabled and its first press started nothing.
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
            announcement: None,
            announcements: 0,
            apply_focus: cx.focus_handle(),
            clear_focus: cx.focus_handle(),
            bar_room: Rc::default(),
            viewport,
            pending_scroll: Rc::default(),
            gutter_origin: Rc::default(),
            find_partner: None,
            _subscription: subscription,
        }
    })
}

impl Render for DiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The caret's row while the text holds focus: its checkbox draws
        // the caret's mark ([`Self::render_partial_controls`]).
        let caret = self.line_keys(window, cx).map(|_| self.caret_row(cx));
        let editor = self.editor.clone();
        let scroll_editor = self.editor.clone();
        let pending_scroll = Rc::clone(&self.pending_scroll);
        let painted_scroll = Rc::clone(&self.pending_scroll);
        let gutter_origin = Rc::clone(&self.gutter_origin);
        let rows = Arc::clone(&self.rows);
        let trace_label = self.label.clone();
        let row_count = rows.len();
        let column_width = self.column_width * crate::appearance::code_scale();
        let single_column = self.single_column;
        let action_width = if self.partial.is_some() {
            f32::from(crate::appearance::ui_size(ACTION_WIDTH))
        } else {
            0.
        };
        let width = action_width + column_width * if single_column { 1. } else { 2. };
        let controls = self.render_partial_controls(width, caret, cx);
        let find_partner = self.find_partner.clone();
        // The gutter and the partial actions stand beside the text under
        // Find's bar, which spans them (`editor_find::Editor::beside`), so a
        // narrow side's bar keeps the line numbers' width as well. The editor
        // paints first: it records its current scrolled bounds during paint,
        // and the later gutter canvas reads those same-frame coordinates,
        // avoiding a one-frame lag during wheel scrolling.
        let gutter = div()
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
                    let bottom_padding = (text_bounds.size.height / 2.).max(line_height * 3.);
                    let content_height = (line_height * row_count as f32 + bottom_padding)
                        .max(text_bounds.size.height);
                    let min_y = (state.input_bounds().size.height - content_height).min(px(0.));
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
                        // The partial actions are placed from this origin
                        // as the view renders, before this paint, so a paint
                        // that finds it moved, the first one or one after the
                        // view was laid out anew, asks for the frame that
                        // draws them in place.
                        if gutter_origin.replace(Some(bounds.origin)) != Some(bounds.origin)
                            && action_width > 0.
                        {
                            window.request_animation_frame();
                        }
                        #[cfg(test)]
                        painted_rows::record(&editor, cx);
                        if action_width > 0. {
                            window.paint_quad(fill(bounds, rgb(palette(cx).panel)));
                        }
                        let numbered_bounds = Bounds::new(
                            point(bounds.origin.x + px(action_width), bounds.origin.y),
                            size(bounds.size.width - px(action_width), bounds.size.height),
                        );
                        if scroll_trace_enabled() {
                            trace_editor_paint(&editor, &trace_label, bounds, window, cx);
                        }
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
            .children(controls);
        let editor = crate::editor_find::Editor::new(&self.editor)
            .h(relative(1.))
            .readonly(true)
            .bordered(false)
            .aria_label(self.label.clone())
            .text_size(crate::appearance::code_text());
        let editor = match &find_partner {
            // Split's headings give their row to the shared bar, which names
            // and marks the side it searches instead.
            Some((partner, side)) => editor.sharing_find_with(partner, *side),
            None => editor,
        };
        let content = div()
            .size_full()
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .code_font(cx)
            .text_size(crate::appearance::code_text())
            .when(self.partial.is_some(), |el| {
                // Prepainted before the text, once the frame is laid out.
                let (room, state) = (Rc::clone(&self.bar_room), self.editor.clone());
                el.child(
                    canvas(
                        move |column, _, cx| room.keep_caret_line(column, &state, cx),
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .child(editor.beside(px(width), vec![gutter.into_any_element()]));
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .when(self.partial.is_some(), |el| {
                // The view's foot, recorded before the text column reads it.
                let room = Rc::clone(&self.bar_room);
                el.on_key_down(cx.listener(Self::line_key)).child(
                    canvas(
                        move |view, _, _| room.foot.set(Some(view.bottom())),
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .child(content)
            .when(!self.selection.ids.is_empty(), |el| {
                el.child(self.render_partial_selection(cx))
            })
            .children(self.render_announcement())
    }
}

fn trace_editor_paint(
    editor: &Entity<EditorState>,
    label: &SharedString,
    gutter_bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &App,
) {
    let state = editor.read(cx);
    let input_bounds = state.input_bounds();
    trace_scroll(
        "editor_paint",
        format_args!(
            "window={:?} editor={:?} label={label:?} scale={} offset={:?} text={:?} input={input_bounds:?} line_height={:?} visible={:?}",
            window.window_handle().window_id(),
            editor.entity_id(),
            window.scale_factor(),
            state.scroll_offset(),
            state.text_bounds(),
            state.line_height(),
            state.visible_row_range(),
        ),
    );
    let editor = editor.clone();
    let label = label.clone();
    // Capture sees events before the editor's ordinary bubble listener can
    // consume them. The observer never changes focus, offsets or propagation.
    window.on_mouse_event(move |event: &gpui_kit::ScrollWheelEvent, phase, _, cx| {
        if phase != gpui_kit::DispatchPhase::Capture
            || (!input_bounds.contains(&event.position) && !gutter_bounds.contains(&event.position))
        {
            return;
        }
        let state = editor.read(cx);
        let Some(height) = state.line_height() else {
            return;
        };
        trace_scroll(
            "input",
            format_args!(
                "editor={:?} label={label:?} area={} delta={:?} before={:?}",
                editor.entity_id(),
                if gutter_bounds.contains(&event.position) {
                    "gutter"
                } else {
                    "code"
                },
                event.delta.pixel_delta(height),
                state.scroll_offset(),
            ),
        );
    });
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

    /// The per-line checkboxes and each hunk's Stage hunk, over the gutter
    /// beside the text's visible rows, none of them a Tab stop: a long diff
    /// would make hundreds, and its lines are selected from the text
    /// ([`Self::toggle_at_caret`]). The checkbox on the `caret` row, where
    /// the text holds focus, draws the caret's mark.
    fn render_partial_controls(
        &self,
        width: f32,
        caret: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(partial) = &self.partial else {
            return Vec::new();
        };
        let accent = palette(cx).accent;
        let state = self.editor.read(cx);
        let (Some(bounds), Some(line_height), Some(visible)) = (
            state.text_bounds(),
            state.line_height(),
            state.visible_row_range(),
        ) else {
            return Vec::new();
        };
        // Placed from where the gutter last painted. Before its first paint
        // they fall back to the text's origin, which lies below the gutter's
        // (8 px at the default size), and that paint, finding the origin
        // moved, asks for the frame that draws them in place.
        let input = state.input_bounds();
        let origin = self.gutter_origin.get().unwrap_or(input.origin);
        let top = bounds.origin.y - input.origin.y;
        let action = self.action();
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
                                    .debug_selector(move || format!("partial-hunk-{row}"))
                                    .tab_stop(false)
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
                                        "{action} all {} in this hunk",
                                        crate::counted(ids.len(), "changed line", "changed lines")
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
                    // The caret's mark: a 2 px bar in the ring's accent at
                    // the checkbox's leading side, as tall as its box, a
                    // mark rather than a ring, since focus stays in the
                    // text. Out of flow, it moves nothing.
                    let mark = (caret == Some(row)).then(|| {
                        div()
                            .debug_selector(|| "partial-line-caret".into())
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(-5.))
                            .w(px(2.))
                            .bg(rgb(accent))
                    });
                    elements.push(
                        div()
                            .debug_selector(move || format!("partial-line-{row}"))
                            .absolute()
                            .top(y)
                            .left(px(5.))
                            .w(crate::appearance::ui_size(ACTION_WIDTH - 6.))
                            .h(line_height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div().relative().flex().children(mark).child(
                                    Checkbox::new(("partial-line", id))
                                        .tab_stop(false)
                                        .small()
                                        .checked(self.selection.ids.contains(&id))
                                        .disabled(self.partial_busy)
                                        .accessibility_label(format!(
                                            "Select {} line {line} for {}",
                                            if *added { "added" } else { "removed" },
                                            action.to_lowercase()
                                        ))
                                        .tooltip(format!(
                                            "Select this changed line. {} selects a range.",
                                            crate::shortcuts::SHIFT_CLICK
                                        ))
                                        .on_click(cx.listener(move |this, checked, window, cx| {
                                            if this.partial_busy {
                                                return;
                                            }
                                            this.selection.toggle(
                                                id,
                                                *checked,
                                                window.modifiers().shift,
                                            );
                                            // The checkbox says what its click did.
                                            this.announcement = None;
                                            this.move_caret_to(row, window, cx);
                                            cx.notify();
                                        })),
                                ),
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

    /// The bar under the text while lines are selected, right-aligned:
    /// Stage (or Unstage) with the selection's count, "Stage 3 lines", the
    /// next Tab stop after the text, and Clear. Either press, from the
    /// keyboard, gives focus back to the text, as the bar goes with the
    /// selection ([`Self::return_focus`]). The count stands in the action:
    /// at 18 pt in a 461 × 490 window the Diff view is 243 px wide and 151
    /// px tall, where a count beside "Stage selected lines" and Clear ran
    /// the action past its edge, its ring cut, and wrapped below whole they
    /// took the text's room. A bar still too narrow for both, as for a
    /// three-digit count there, wraps Clear below, whole.
    fn render_partial_selection(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let action = self.action();
        let count = self.selection.ids.len();
        let lines = crate::counted(count, "line", "lines");
        div()
            .debug_selector(|| "partial-selection-bar".into())
            .flex_shrink_0()
            .px_3()
            .py_2()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap_2()
            .bg(rgb(palette.panel))
            .border_t_1()
            .border_color(rgb(palette.border))
            .child(
                Button::new("apply-partial-selection")
                    .debug_selector(|| "apply-partial-selection".into())
                    .track_focus(&self.apply_focus)
                    .small()
                    .primary()
                    .label(format!("{action} {lines}"))
                    .accessibility_label(format!(
                        "{action} {count} selected {}",
                        if count == 1 { "line" } else { "lines" }
                    ))
                    .disabled(self.partial_busy)
                    .tooltip("For a replacement, select both the removed and added lines.")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.return_focus(&this.apply_focus.clone(), window, cx);
                        this.apply_partial(this.selection.ids.iter().copied().collect(), cx)
                    })),
            )
            .child(
                Button::new("clear-partial-selection")
                    .debug_selector(|| "clear-partial-selection".into())
                    .track_focus(&self.clear_focus)
                    .small()
                    .label("Clear")
                    .disabled(self.partial_busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.return_focus(&this.clear_focus.clone(), window, cx);
                        this.selection = LineSelection::default();
                        this.announcement = None;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

/// The room the selection bar takes under the text, read as each frame is
/// prepainted, after layout and before the text: the view records its foot,
/// then the text's column, above the bar, compares the bar with the last
/// frame's ([`Self::keep_caret_line`]).
#[derive(Default)]
struct BarRoom {
    /// The view's foot in this frame.
    foot: Cell<Option<Pixels>>,
    /// The bar's height and the text column's bounds in the last frame.
    last: Cell<Option<(Pixels, Bounds<Pixels>)>>,
}

impl BarRoom {
    /// Where the bar has come, gone or changed height since the last frame,
    /// keeps the caret's line, if the text showed any of it, whole in the
    /// text's new height, scrolling by the least, as the text reveals a
    /// moved caret: the text lays out this same frame at that offset, so no
    /// frame draws the bar over that line. The text keeps its top under
    /// Find's bar and ends where its `column` does. In the half tile (18 pt,
    /// 461 × 490) the bar the first selected line brought covered the
    /// text's last rows, the caret's among them, until the caret moved.
    fn keep_caret_line(&self, column: Bounds<Pixels>, editor: &Entity<EditorState>, cx: &mut App) {
        let Some(foot) = self.foot.get() else {
            return;
        };
        let bar = foot - column.bottom();
        let Some((was, last)) = self.last.replace(Some((bar, column))) else {
            return;
        };
        if was == bar {
            return;
        }
        editor.update(cx, |state, cx| {
            let Some(height) = state.line_height() else {
                return;
            };
            let text = state.input_bounds();
            let room =
                text.size.height + (column.bottom() - last.bottom()) - (column.top() - last.top());
            let offset = state.scroll_offset();
            let row = state.text().offset_to_point(state.cursor()).row;
            // The line's top below the text's.
            let line = offset.y + height * row as f32;
            if line >= text.size.height || line + height <= px(0.) || height > room {
                return;
            }
            let by = if line + height > room {
                line + height - room
            } else if line < px(0.) {
                line
            } else {
                return;
            };
            state.set_scroll_offset(point(offset.x, offset.y - by), cx);
        });
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
    let labels = crate::appearance::labels(cx);
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
                (Some(_), None) => (labels.removed, Some(palette.removed_background)),
                (None, Some(_)) => (labels.added, Some(palette.added_background)),
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
    let widest = rows
        .iter()
        .flat_map(|row| [row.old, row.new])
        .flatten()
        .max()
        .unwrap_or(0);
    (rows, line_number_column_width(widest))
}

/// The width at the default code size of a line-number column that holds `widest`, at least
/// three digits, with [`CELL_PADDING`] on either side. Compare's gutter and the GitHub
/// review's Changes rows share it, so a patch's numbers stand at one place in both.
pub(crate) fn line_number_column_width(widest: u64) -> f32 {
    let digits = (widest.checked_ilog10().unwrap_or(0) + 1).max(3);
    digits as f32 * FONT_SIZE * 0.65 + CELL_PADDING * 2.
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

/// Test-only: where each painted frame put each patch view's rows, as the
/// gutter, painted after its editor, reads them. A test window draws a
/// frame a layout asks for inside the update that asked, so bounds read
/// after it show only the last frame; this record keeps each.
#[cfg(test)]
pub(crate) mod painted_rows {
    use super::*;
    use std::cell::RefCell;

    /// A painted view's editor and the top of its first row.
    pub(crate) type Painted = (gpui_kit::EntityId, Option<Pixels>);

    /// A painted view's editor, the top of its first row and the region
    /// its text showed, in one frame.
    pub(crate) struct Frame {
        pub(crate) editor: gpui_kit::EntityId,
        pub(crate) top: Option<Pixels>,
        pub(crate) text: Bounds<Pixels>,
    }

    thread_local! {
        static ROWS: RefCell<Option<Vec<Frame>>> = const { RefCell::new(None) };
    }

    /// Starts recording, forgetting anything recorded before.
    pub(crate) fn watch() {
        ROWS.with_borrow_mut(|rows| *rows = Some(Vec::new()));
    }

    /// Each painted view's editor and the top of its first row, its text's
    /// scrolled origin, in paint order since [`watch`] or the last call;
    /// recording goes on.
    pub(crate) fn take() -> Vec<Painted> {
        take_frames()
            .into_iter()
            .map(|frame| (frame.editor, frame.top))
            .collect()
    }

    /// Each painted view's [`Frame`], in paint order since [`watch`] or the
    /// last call; recording goes on.
    pub(crate) fn take_frames() -> Vec<Frame> {
        ROWS.with_borrow_mut(|rows| rows.as_mut().map(std::mem::take).unwrap_or_default())
    }

    /// Stops recording.
    pub(crate) fn stop() {
        ROWS.with_borrow_mut(|rows| *rows = None);
    }

    pub(super) fn record(editor: &Entity<EditorState>, cx: &App) {
        ROWS.with_borrow_mut(|rows| {
            if let Some(rows) = rows {
                let state = editor.read(cx);
                rows.push(Frame {
                    editor: editor.entity_id(),
                    top: state.text_bounds().map(|bounds| bounds.origin.y),
                    text: state.input_bounds(),
                });
            }
        });
    }
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

    /// Keyboard staging by line in Working's diff (decided 2026-11-07):
    /// the text's caret and selection reach the per-line checkboxes, at QA's
    /// full window at 13 pt and its half tile at 18 pt.
    mod keyboard {
        use super::*;
        use crate::gpui;
        use crate::native_accessibility::written;
        use crate::views::tests::{fixture_git, history_window_from, settle};
        use gpui::{Modifiers, VisualTestContext};

        /// The windows and interface text sizes each test walks.
        const SIZES: [(f32, f32, u8); 2] = [(1480., 973., 13), (461., 490., 18)];

        /// No line selected.
        const NONE: [usize; 0] = [];

        /// Working Changes showing notes.txt's unstaged diff, its notes 4 and
        /// 5 replaced by "four" and "five": two removed and two added lines
        /// between context lines, in Midnight with reduced motion.
        async fn working(
            cx: &mut gpui::TestAppContext,
        ) -> (
            tempfile::TempDir,
            Entity<crate::GitTurtle>,
            &mut VisualTestContext,
        ) {
            let (fixture, app, cx) =
                history_window_from(cx, &crate::views::tests::history_script(1, 12)).await;
            crate::focus_ring::sweep::prepare(&app, cx);
            let root = fixture.path().join("narrow");
            fixture_git(&root, &["reset", "--hard", "--quiet"]);
            let edited: String = (1..=12)
                .map(|line| match line {
                    4 => "four\n".to_owned(),
                    5 => "five\n".to_owned(),
                    _ => format!("note {line}\n"),
                })
                .collect();
            std::fs::write(root.join("notes.txt"), edited).unwrap();
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
            settle(&app, cx).await;
            crate::path_filter::filters_land(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.text_mode = crate::TextMode::Unified;
                    app.select_working(0, ChangeArea::Unstaged, window, cx);
                })
            });
            settle(&app, cx).await;
            (fixture, app, cx)
        }

        /// The shown diff at one of [`SIZES`].
        struct Shown {
            editor: Entity<EditorState>,
            view: Entity<DiffView>,
            rows: Arc<[PartialRow]>,
            at: String,
        }

        impl Shown {
            /// The diff drawn in a `width` × `height` window at `interface`
            /// pt, with no line selected or announced, its text focused.
            fn at(
                app: &Entity<crate::GitTurtle>,
                cx: &mut VisualTestContext,
                (width, height, interface): (f32, f32, u8),
            ) -> Self {
                cx.simulate_resize(size(px(width), px(height)));
                crate::focus_ring::sweep::interface_size(cx, interface);
                let (editor, view, rows) = app.read_with(cx, |app, _| {
                    let Some(crate::Content::Text {
                        partial: Some(partial),
                        ..
                    }) = app.content.as_deref()
                    else {
                        panic!("the diff offers its partial actions");
                    };
                    (
                        app.patch_editor.clone().expect("the patch's editor"),
                        app.patch_view.clone().expect("the Diff view"),
                        Arc::clone(&partial.rows),
                    )
                });
                view.update(cx, |view, cx| {
                    view.selection = LineSelection::default();
                    view.announcement = None;
                    cx.notify();
                });
                let shown = Self {
                    editor,
                    view,
                    rows,
                    at: format!("{width} × {height} at {interface} pt"),
                };
                shown.caret_to(cx, shown.first());
                shown
            }

            /// The first changed line's row, removed note 4.
            fn first(&self) -> usize {
                self.rows
                    .iter()
                    .position(|row| matches!(row, PartialRow::Change { .. }))
                    .expect("a changed line")
            }

            /// The context line before it, note 3.
            fn context(&self) -> usize {
                let row = self.first() - 1;
                assert_eq!(self.rows[row], PartialRow::None, "{}", self.at);
                row
            }

            /// The selection ID of `row`'s changed line.
            fn id(&self, row: usize) -> usize {
                let PartialRow::Change { id, .. } = self.rows[row] else {
                    panic!("row {row} is no changed line");
                };
                id
            }

            /// Moves the caret to the start of `row`, focusing the text.
            fn caret_to(&self, cx: &mut VisualTestContext, row: usize) {
                cx.update(|window, cx| {
                    self.editor.update(cx, |state, cx| {
                        state.set_cursor_position(Position::new(row as u32, 0), window, cx)
                    })
                });
                crate::tags::tests::draw(cx);
            }

            /// Presses and releases each of `keys` in turn, as a keyboard
            /// does, a key's text reaching the focused field where nothing
            /// took the key: GPUI clicks a focused Button as Space or Enter
            /// is released.
            fn press(&self, cx: &mut VisualTestContext, keys: &str) {
                for key in keys.split(' ') {
                    cx.simulate_keystrokes(key);
                    let keystroke = gpui::Keystroke::parse(key).unwrap();
                    cx.simulate_event(gpui::KeyUpEvent { keystroke });
                }
                crate::tags::tests::draw(cx);
            }

            fn selected(&self, cx: &mut VisualTestContext) -> Vec<usize> {
                self.view
                    .read_with(cx, |view, _| view.selection.ids.iter().copied().collect())
            }

            fn caret(&self, cx: &mut VisualTestContext) -> usize {
                self.view.read_with(cx, |view, cx| view.caret_row(cx))
            }

            fn text_focused(&self, cx: &mut VisualTestContext) -> bool {
                cx.update(|window, cx| self.view.read(cx).focus_handle(cx).is_focused(window))
            }

            /// Scrolls the text so `row` is its last whole row, and
            /// `sideways` to the right, as far as the text scrolls: where
            /// the whole diff fits, it stays put.
            fn to_foot(&self, cx: &mut VisualTestContext, row: usize, sideways: Pixels) {
                self.editor.update(cx, |state, cx| {
                    let height = state.line_height().expect("the text is laid out");
                    let text = state.input_bounds().size.height;
                    let y = (text - height * (row + 1) as f32).min(px(0.));
                    state.set_scroll_offset(point(-sideways, y), cx);
                });
                crate::tags::tests::draw(cx);
            }

            /// The text's line height and the region it shows, which ends
            /// where the selection bar, if any, begins.
            fn text(&self, cx: &mut VisualTestContext) -> (Pixels, Bounds<Pixels>) {
                let (height, text) = self.editor.read_with(cx, |state, _| {
                    let height = state.line_height().expect("the text is laid out");
                    (height, state.input_bounds())
                });
                if let Some(bar) = cx.debug_bounds("partial-selection-bar") {
                    assert!(
                        text.bottom() <= bar.top() + px(0.01),
                        "{}: the text {text:?} ends above the bar {bar:?}",
                        self.at
                    );
                }
                (height, text)
            }

            /// The top of the caret's line where the latest frame painted
            /// the text.
            fn caret_line(&self, cx: &mut VisualTestContext) -> Pixels {
                let row = self.caret(cx);
                let (height, _) = self.text(cx);
                let top = self.editor.read_with(cx, |state, _| {
                    state.text_bounds().expect("the text is painted").origin.y
                });
                top + height * row as f32
            }

            /// Does `act` and asserts that every frame painted after it
            /// draws the caret's line whole in the text it shows, `when`.
            /// Returns the top of that line in the first frame whose text
            /// ends where it now does, above the selection bar where it
            /// shows: a frame the text asked for before `act` can come
            /// first.
            fn caret_line_after(
                &self,
                cx: &mut VisualTestContext,
                when: &str,
                act: impl FnOnce(&mut VisualTestContext),
            ) -> Pixels {
                painted_rows::watch();
                act(cx);
                let frames = painted_rows::take_frames();
                painted_rows::stop();
                let (height, now) = self.text(cx);
                let row = self.caret(cx);
                let mut first = None;
                for frame in frames
                    .iter()
                    .filter(|frame| frame.editor == self.editor.entity_id())
                {
                    let top = frame.top.expect("the text is painted");
                    let line = top + height * row as f32;
                    let text = frame.text;
                    assert!(
                        line >= text.top() - px(0.01) && line + height <= text.bottom() + px(0.01),
                        "{}: {when}: a frame draws the caret's line {line:?} to {:?} \
                         outside its text {text:?}",
                        self.at,
                        line + height
                    );
                    if first.is_none() && text == now {
                        first = Some(line);
                    }
                }
                first.unwrap_or_else(|| {
                    panic!("{}: {when}: a frame draws the text {now:?}", self.at)
                })
            }

            /// Asserts that the caret's line, its top at `line`, lies whole
            /// in the text, above the selection bar where it shows.
            fn assert_whole(&self, cx: &mut VisualTestContext, line: Pixels, when: &str) {
                let (height, text) = self.text(cx);
                assert!(
                    line >= text.top() - px(0.01) && line + height <= text.bottom() + px(0.01),
                    "{}: {when}: the caret's line {line:?} to {:?} lies whole in the text {text:?}",
                    self.at,
                    line + height
                );
            }
        }

        /// The view's live regions the last frame drew, by element ID with
        /// what each says: a polite status whose name is its value.
        fn said() -> Vec<(String, String)> {
            written::announced_from("partial-line-announcement")
                .into_iter()
                .map(|(id, announced)| {
                    assert_eq!(announced.live, gpui::accesskit::Live::Polite, "{id}");
                    let node = written::get(&id).expect("the region's node").node;
                    assert_eq!(node.role(), gpui::accesskit::Role::Status, "{id}");
                    assert_eq!(
                        node.label(),
                        node.value(),
                        "{id}: AT-SPI reads the name, macOS the value"
                    );
                    let text = node.label().expect("a name").to_owned();
                    (id, text)
                })
                .collect()
        }

        /// Space on a changed line selects it for staging, and Space again
        /// clears it, each announced from the view's live region in a node
        /// of its own, as a click on its checkbox selects it; Space on a
        /// context line selects nothing, types nothing and says nothing,
        /// and a held Space's repeats do nothing. The view takes the key,
        /// which goes on as no text. The status bar names Space
        /// while the text holds focus. In the staged area the lines are
        /// selected to unstage, and the status bar and the live region say
        /// so. Before this, Space in the read-only text did nothing and no
        /// key reached the checkboxes.
        #[gpui::test]
        async fn space_selects_the_caret_line_and_space_again_clears_it(
            cx: &mut gpui::TestAppContext,
        ) {
            let (fixture, app, cx) = working(cx).await;
            for size in SIZES {
                let shown = Shown::at(&app, cx, size);
                let at = shown.at.clone();
                let first = shown.first();
                let text = shown.editor.read_with(cx, |state, _| state.value());
                shown.caret_to(cx, shown.context());
                shown.press(cx, "space");
                assert_eq!(
                    shown.selected(cx),
                    NONE,
                    "{at}: a context line selects nothing"
                );
                assert_eq!(said(), [], "{at}: and says nothing");
                shown.caret_to(cx, first);
                shown.press(cx, "space");
                assert_eq!(shown.selected(cx), [shown.id(first)], "{at}");
                let [(selected, said_selected)] = &said()[..] else {
                    panic!("{at}: one announcement: {:?}", said());
                };
                assert_eq!(said_selected, "Removed line 4 selected to stage", "{at}");
                assert!(
                    cx.debug_bounds("apply-partial-selection").is_some(),
                    "{at}: the selection bar shows"
                );
                // A held Space's repeats leave the line selected, unsaid.
                cx.simulate_event(gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("space").unwrap(),
                    is_held: true,
                    prefer_character_input: false,
                });
                crate::tags::tests::draw(cx);
                assert_eq!(shown.selected(cx), [shown.id(first)], "{at}: a repeat");
                assert_eq!(
                    said()[..],
                    [(selected.clone(), said_selected.clone())],
                    "{at}"
                );
                let hints = cx
                    .update(|window, cx| app.update(cx, |app, cx| app.key_hints(window, cx)))
                    .expect("the page's hints");
                assert!(
                    hints.keys.contains(&(
                        crate::shortcuts::key_label("space"),
                        "Select lines to stage"
                    )),
                    "{at}: the status bar names Space: {:?}",
                    hints.keys
                );
                // The view takes the key, so the platform sends it on as no
                // text, nor to an input method.
                let space = gpui::Keystroke::parse("space").unwrap();
                let taken = cx.update(|window, cx| {
                    let down = gpui::KeyDownEvent {
                        keystroke: space.clone(),
                        is_held: false,
                        prefer_character_input: false,
                    };
                    !window
                        .dispatch_event(gpui::PlatformInput::KeyDown(down), cx)
                        .propagate
                });
                assert!(taken, "{at}: Space goes on from the view");
                cx.simulate_event(gpui::KeyUpEvent { keystroke: space });
                crate::tags::tests::draw(cx);
                assert_eq!(shown.selected(cx), NONE, "{at}: Space again clears it");
                let [(cleared, said_cleared)] = &said()[..] else {
                    panic!("{at}: one announcement: {:?}", said());
                };
                assert_ne!(cleared, selected, "{at}: a node of its own");
                assert_eq!(
                    said_cleared, "Removed line 4 no longer selected to stage",
                    "{at}"
                );
                assert_eq!(
                    shown.editor.read_with(cx, |state, _| state.value()),
                    text,
                    "{at}: Space types nothing"
                );
                // Focus elsewhere, Space and the hint are the page's again.
                let list = app.read_with(cx, |app, _| app.file_focus.clone());
                cx.update(|window, cx| window.focus(&list, cx));
                crate::tags::tests::draw(cx);
                let hints = cx
                    .update(|window, cx| app.update(cx, |app, cx| app.key_hints(window, cx)))
                    .expect("the page's hints");
                assert!(
                    !hints
                        .keys
                        .iter()
                        .any(|(_, action)| *action == "Select lines to stage"),
                    "{at}: {:?}",
                    hints.keys
                );
            }
            // The same notes staged: Space selects lines to unstage.
            fixture_git(&fixture.path().join("narrow"), &["add", "notes.txt"]);
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            settle(&app, cx).await;
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.select_working(0, ChangeArea::Staged, window, cx)
                })
            });
            settle(&app, cx).await;
            let shown = Shown::at(&app, cx, SIZES[1]);
            shown.press(cx, "space");
            assert_eq!(shown.selected(cx), [shown.id(shown.first())], "staged");
            let [(_, text)] = &said()[..] else {
                panic!("staged: one announcement: {:?}", said());
            };
            assert_eq!(text, "Removed line 4 selected to unstage");
            let hints = cx
                .update(|window, cx| app.update(cx, |app, cx| app.key_hints(window, cx)))
                .expect("the page's hints");
            assert!(
                hints.keys.contains(&(
                    crate::shortcuts::key_label("space"),
                    "Select lines to unstage"
                )),
                "staged: the status bar names Space: {:?}",
                hints.keys
            );
        }

        /// Shift+Down twice from a changed line extends the text selection
        /// over three changed lines, and Space selects all three, then
        /// clears all three; where some of the lines a selection covers are
        /// selected and some not, Space selects them all.
        #[gpui::test]
        async fn shift_down_twice_then_space_selects_three_lines(cx: &mut gpui::TestAppContext) {
            let (_fixture, app, cx) = working(cx).await;
            for size in SIZES {
                let shown = Shown::at(&app, cx, size);
                let at = shown.at.clone();
                let first = shown.first();
                let three: Vec<_> = (first..first + 3).map(|row| shown.id(row)).collect();
                shown.press(cx, "shift-down shift-down");
                assert_eq!(shown.caret(cx), first + 2, "{at}");
                shown.press(cx, "space");
                assert_eq!(shown.selected(cx), three, "{at}");
                let [(_, text)] = &said()[..] else {
                    panic!("{at}: one announcement: {:?}", said());
                };
                assert_eq!(text, "3 lines selected to stage", "{at}");
                shown.press(cx, "space");
                assert_eq!(shown.selected(cx), NONE, "{at}: all three cleared");
                let [(_, text)] = &said()[..] else {
                    panic!("{at}: one announcement: {:?}", said());
                };
                assert_eq!(text, "3 lines no longer selected to stage", "{at}");
                // The middle line alone, then all three: any unselected
                // selects them all.
                shown.caret_to(cx, first + 1);
                shown.press(cx, "space");
                assert_eq!(shown.selected(cx), [three[1]], "{at}");
                shown.caret_to(cx, first);
                shown.press(cx, "shift-down shift-down space");
                assert_eq!(shown.selected(cx), three, "{at}");
            }
        }

        /// The selection bar never covers the caret's line: with the caret
        /// on the text's last whole row, the frame that draws the bar Space
        /// brings scrolls the text by the least that keeps that line whole
        /// above it, as the text reveals a moved caret, and the frame that
        /// takes the bar away leaves the line where it is, whole. A click on
        /// the checkbox of the text's last whole row, the text scrolled
        /// sideways, brings the bar with the line whole above it and its
        /// start shown, where the caret goes. In the half tile the bar once
        /// covered the text's last rows, the caret's among them, until the
        /// caret moved.
        #[gpui::test]
        async fn the_selection_bar_never_covers_the_caret_line(cx: &mut gpui::TestAppContext) {
            let (_fixture, app, cx) = working(cx).await;
            for size in SIZES {
                let shown = Shown::at(&app, cx, size);
                let at = shown.at.clone();
                // The last changed line, added "five".
                let row = shown.first() + 3;
                shown.caret_to(cx, row);
                shown.to_foot(cx, row, px(0.));
                let before = shown.caret_line(cx);
                shown.assert_whole(cx, before, "before Space");
                let (height, text) = shown.text(cx);
                let line =
                    shown.caret_line_after(cx, "the bar comes", |cx| shown.press(cx, "space"));
                assert_eq!(shown.selected(cx), [shown.id(row)], "{at}");
                shown.assert_whole(cx, line, "in the frame that draws the bar");
                let (_, shorter) = shown.text(cx);
                assert!(
                    shorter.bottom() < text.bottom(),
                    "{at}: the bar takes the text's foot"
                );
                // The least scroll: the line moves up only as far as the
                // bar would cover it.
                let least = (before + height - shorter.bottom()).max(px(0.));
                assert!(
                    (f32::from(before - least - line)).abs() < 0.01,
                    "{at}: the caret's line moved from {before:?} to {line:?}, not by the least, {least:?}"
                );
                let after =
                    shown.caret_line_after(cx, "the bar goes", |cx| shown.press(cx, "space"));
                assert_eq!(shown.selected(cx), NONE, "{at}");
                assert_eq!(cx.debug_bounds("partial-selection-bar"), None, "{at}");
                shown.assert_whole(cx, after, "in the frame that takes the bar away");
                assert!(
                    (f32::from(after - line)).abs() < 0.01,
                    "{at}: the caret's line stays where it was, at {line:?}, not {after:?}"
                );
                // A click on the foot row's checkbox, the caret elsewhere.
                shown.caret_to(cx, shown.context());
                shown.to_foot(cx, row, px(40.));
                let cell = cx
                    .debug_bounds(format!("partial-line-{row}").leak())
                    .expect("the row's checkbox");
                cx.simulate_mouse_down(
                    cell.center(),
                    gpui::MouseButton::Left,
                    Modifiers::default(),
                );
                let line = shown.caret_line_after(cx, "a click's bar comes", |cx| {
                    cx.simulate_mouse_up(
                        cell.center(),
                        gpui::MouseButton::Left,
                        Modifiers::default(),
                    );
                    crate::tags::tests::draw(cx);
                });
                assert_eq!(shown.selected(cx), [shown.id(row)], "{at}");
                assert_eq!(shown.caret(cx), row, "{at}: the caret moved to the click");
                shown.assert_whole(cx, line, "in the frame a click's bar comes");
                assert_eq!(
                    shown
                        .editor
                        .read_with(cx, |state, _| state.scroll_offset().x),
                    px(0.),
                    "{at}: the text shows the caret line's start"
                );
            }
        }

        /// With lines selected, Tab from the text moves to Stage selected
        /// lines, its ring whole, then to Clear; Clear's press from the
        /// keyboard clears the selection and gives focus back to the text.
        /// The per-line checkboxes and the hunk's Stage hunk are no Tab
        /// stops.
        #[gpui::test]
        async fn the_selected_lines_action_is_the_next_tab_stop_with_its_ring_whole(
            cx: &mut gpui::TestAppContext,
        ) {
            let (_fixture, app, cx) = working(cx).await;
            let accent: gpui::Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
            for size in SIZES {
                let shown = Shown::at(&app, cx, size);
                let at = shown.at.clone();
                shown.press(cx, "space");
                let (apply, clear) = shown.view.read_with(cx, |view, _| {
                    (view.apply_focus.clone(), view.clear_focus.clone())
                });
                for (focus, selector) in [
                    (&apply, "apply-partial-selection"),
                    (&clear, "clear-partial-selection"),
                ] {
                    shown.press(cx, "tab");
                    assert!(
                        cx.update(|window, _| focus.is_focused(window)),
                        "{at}: Tab reaches {selector}, focus on {:?}",
                        cx.update(|window, cx| window.focused(cx))
                    );
                    let button = cx.debug_bounds(selector).expect("the button is drawn");
                    let rings: Vec<_> = crate::focus_ring::painted_outlines(cx, accent)
                        .into_iter()
                        .filter(|ring| {
                            ring.bounds.left() < button.left()
                                && ring.bounds.top() < button.top()
                                && ring.bounds.right() > button.right()
                                && ring.bounds.bottom() > button.bottom()
                        })
                        .collect();
                    let [ring] = &rings[..] else {
                        panic!("{at}: one ring around {selector} {button:?}: {rings:?}");
                    };
                    assert!(ring.whole, "{at}: {selector}'s ring is cut: {ring:?}");
                }
                shown.press(cx, "space");
                assert_eq!(
                    shown.selected(cx),
                    NONE,
                    "{at}: Clear cleared the selection"
                );
                assert!(shown.text_focused(cx), "{at}: focus is back in the text");
            }
        }

        /// The checkbox on the caret's line draws a 2 px accent mark at its
        /// leading side while the text holds focus, which follows the caret
        /// from line to line and leaves a context line, and a blurred text,
        /// unmarked. A click on a checkbox selects its line and moves the
        /// caret, and with it the mark, there. The selection bar that Space
        /// or the click brings leaves the line, and its mark, whole above it.
        #[gpui::test]
        async fn the_caret_lines_checkbox_mark_follows_the_caret(cx: &mut gpui::TestAppContext) {
            let (_fixture, app, cx) = working(cx).await;
            let accent =
                cx.read(|cx| gpui_kit::Background::from(gpui::Hsla::from(rgb(palette(cx).accent))));
            for size in SIZES {
                let shown = Shown::at(&app, cx, size);
                let at = shown.at.clone();
                let first = shown.first();
                let marked = |cx: &mut VisualTestContext, row: usize| {
                    let mark = cx
                        .debug_bounds("partial-line-caret")
                        .unwrap_or_else(|| panic!("{at}: row {row}'s checkbox is marked"));
                    let cell = cx
                        .debug_bounds(format!("partial-line-{row}").leak())
                        .expect("the row's checkbox");
                    assert!(
                        mark.top() >= cell.top() && mark.bottom() <= cell.bottom(),
                        "{at}: the mark {mark:?} lies on row {row}'s checkbox {cell:?}"
                    );
                    assert_eq!(mark.size.width, px(2.), "{at}");
                    assert!(
                        mark.left() >= cell.left(),
                        "{at}: inside the gutter's column"
                    );
                    let painted = cx.update(|window, _| {
                        let scale = window.scale_factor();
                        window.painted_quads().into_iter().any(|quad| {
                            quad.background == accent
                                && (quad.bounds.origin.x.as_f32() / scale - f32::from(mark.left()))
                                    .abs()
                                    < 0.5
                                && (quad.bounds.origin.y.as_f32() / scale - f32::from(mark.top()))
                                    .abs()
                                    < 0.5
                        })
                    });
                    assert!(painted, "{at}: the mark is painted in the ring's accent");
                    let (_, text) = shown.text(cx);
                    assert!(
                        mark.top() >= text.top() && mark.bottom() <= text.bottom(),
                        "{at}: the mark {mark:?} shows whole in the text {text:?}"
                    );
                };
                marked(cx, first);
                // Space where the caret starts, the half tile's last whole
                // row, brings the bar.
                shown.press(cx, "space");
                assert_eq!(shown.selected(cx), [shown.id(first)], "{at}");
                marked(cx, first);
                shown.press(cx, "space");
                assert_eq!(shown.selected(cx), NONE, "{at}");
                shown.press(cx, "down");
                marked(cx, first + 1);
                shown.caret_to(cx, shown.context());
                assert_eq!(
                    cx.debug_bounds("partial-line-caret"),
                    None,
                    "{at}: no mark on a context line"
                );
                // A click on a checkbox moves the caret, and the mark, there:
                // the second changed line, at the half tile's foot.
                let cell = cx
                    .debug_bounds(format!("partial-line-{}", first + 1).leak())
                    .expect("the row's checkbox");
                cx.simulate_click(cell.center(), Modifiers::default());
                crate::tags::tests::draw(cx);
                assert_eq!(shown.selected(cx), [shown.id(first + 1)], "{at}");
                assert_eq!(
                    shown.caret(cx),
                    first + 1,
                    "{at}: the caret moved to the click"
                );
                assert!(shown.text_focused(cx), "{at}");
                marked(cx, first + 1);
                cx.update(|window, cx| window.blur(cx));
                crate::tags::tests::draw(cx);
                assert_eq!(
                    cx.debug_bounds("partial-line-caret"),
                    None,
                    "{at}: no mark unfocused"
                );
            }
        }
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
