//! Pointer and keyboard selection of the commit message and the metadata lines
//! after it, read as one document.
//!
//! The window's base `TextSelection` layer owns the gesture, its clearing,
//! auto-scroll and multi-click. This adapter registers the message viewport
//! as a single participant, so a selection keeps both endpoints after the
//! list stops painting their pieces. Each endpoint is a content key holding a
//! byte offset into the document, resolved against the runs the previous
//! frame painted. Copy slices the retained pieces between the two offsets and
//! puts back the line breaks the display draws between runs, so the clipboard
//! never depends on which pieces happen to be painted.
use super::Piece;
use crate::*;
use gpui_kit::base::{
    TextSelection, TextSelectionContentKey, TextSelectionEndpoint, TextSelectionEvent,
    TextSelectionHandle, TextSelectionRegistration, TextSelectionRun,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// The line breaks between the message and its first metadata line: the
/// metadata reads as a paragraph of its own.
const METADATA_BREAK: usize = 2;

/// The selection of one presented message, kept with its `Presentation`.
/// Clones share the participant and its model; dropping the presentation
/// drops the participant, which ends any selection it held.
#[derive(Clone)]
pub(crate) struct Selection {
    handle: TextSelectionHandle,
    model: Rc<RefCell<Model>>,
    _changes: Rc<Subscription>,
}

#[derive(Default)]
struct Model {
    pieces: Arc<Vec<Piece>>,
    message_len: usize,
    /// Metadata lines as displayed and copied, after the message.
    metadata: Vec<SharedString>,
    /// Runs painted in the current frame, and those of the last complete
    /// frame, which endpoint lookups use.
    painting: Vec<Painted>,
    painted: Vec<Painted>,
    /// The geometry registered with the window for `painted`.
    viewport: Bounds<Pixels>,
    scroll: Point<Pixels>,
    /// Document offsets of the gesture's endpoints.
    anchor: Option<usize>,
    cursor: Option<usize>,
    /// The window endpoints those offsets were last taken from; a snapshot
    /// repeating one leaves its offset as it is.
    seen_anchor: Option<TextSelectionEndpoint>,
    seen_cursor: Option<TextSelectionEndpoint>,
    /// Select All: the whole message, without its metadata.
    all: bool,
    /// Registered by the latest paint, and not since replaced or put away.
    live: bool,
    /// The view that paints the runs, refreshed when the selection changes.
    view: Option<EntityId>,
}

#[derive(Clone)]
struct Painted {
    start: usize,
    text: SharedString,
    layout: TextLayout,
    bounds: Bounds<Pixels>,
}

/// Takes the offset an endpoint's resolved key holds, unless the endpoint is
/// the one the offset already came from.
fn take_offset(
    endpoint: TextSelectionEndpoint,
    seen: &mut Option<TextSelectionEndpoint>,
    offset: &mut Option<usize>,
) {
    if *seen != Some(endpoint)
        && let Some(key) = endpoint.content_key()
    {
        (*seen, *offset) = (Some(endpoint), Some(key.value() as usize));
    }
}

impl Model {
    fn clear(&mut self) {
        self.clear_endpoints();
        self.all = false;
    }

    fn clear_endpoints(&mut self) {
        (self.anchor, self.cursor) = (None, None);
        (self.seen_anchor, self.seen_cursor) = (None, None);
    }

    fn metadata_start(&self) -> usize {
        if self.message_len == 0 {
            0
        } else {
            self.message_len + METADATA_BREAK
        }
    }

    /// Every run in reading order with its document offset.
    fn runs(&self) -> impl Iterator<Item = (usize, &str)> {
        let mut start = self.metadata_start();
        self.pieces
            .iter()
            .map(|piece| (piece.start, piece.text.as_ref()))
            .chain(self.metadata.iter().map(move |line| {
                let run = (start, line.as_ref());
                start += line.len() + 1;
                run
            }))
    }

    fn range(&self) -> Option<Range<usize>> {
        if self.all {
            return (self.message_len > 0).then_some(0..self.message_len);
        }
        let (anchor, cursor) = (self.anchor?, self.cursor?);
        (anchor != cursor).then(|| anchor.min(cursor)..anchor.max(cursor))
    }

    /// The part of the run at `start` that the selection covers, and whether
    /// the selection continues past the run's end (through the line break
    /// after it).
    fn run_range(&self, start: usize, len: usize) -> Option<(Range<usize>, bool)> {
        let range = self.range()?;
        let end = start + len;
        if range.end <= start || range.start > end {
            return None;
        }
        Some((
            range.start.max(start) - start..range.end.min(end) - start,
            range.end > end,
        ))
    }

    fn selected_text(&self) -> String {
        self.range()
            .filter(|_| self.live)
            .map(|range| self.text(range))
            .unwrap_or_default()
    }

    /// The document's bytes in `range`. Bytes between runs are line breaks:
    /// the newline a piece boundary leaves to the next row, the blank line
    /// between title and body, and the breaks before and between metadata
    /// lines.
    fn text(&self, range: Range<usize>) -> String {
        let mut text = String::with_capacity(range.len());
        let skip = self
            .pieces
            .partition_point(|piece| piece.start + piece.text.len() <= range.start);
        let mut previous = skip.checked_sub(1).map_or(0, |index| {
            self.pieces[index].start + self.pieces[index].text.len()
        });
        for (start, run) in self.runs().skip(skip) {
            let breaks = previous.max(range.start)..start.min(range.end);
            text.extend(std::iter::repeat_n('\n', breaks.len()));
            if start >= range.end {
                break;
            }
            let from = run.floor_char_boundary(range.start.saturating_sub(start).min(run.len()));
            let to = run.floor_char_boundary((range.end - start).min(run.len()));
            text.push_str(&run[from..to]);
            previous = start + run.len();
        }
        text
    }

    /// The document offset nearest a participant content point, which the
    /// window measured against the registered geometry.
    fn offset_at(&self, content: Point<Pixels>) -> Option<usize> {
        self.offset_at_window(content + self.viewport.origin + self.scroll)
    }

    /// The document offset nearest a window point in the painted runs. The
    /// point is held inside the viewport, so a drag past its edge selects to
    /// the visible text there while auto-scroll brings more into view.
    fn offset_at_window(&self, position: Point<Pixels>) -> Option<usize> {
        let within = |y: Pixels, bounds: Bounds<Pixels>| {
            y.max(bounds.top())
                .min((bounds.bottom() - px(1.)).max(bounds.top()))
        };
        let y = within(position.y, self.viewport);
        let distance = |bounds: Bounds<Pixels>| {
            f32::from((bounds.top() - y).max(y - bounds.bottom()).max(px(0.)))
        };
        let run = self
            .painted
            .iter()
            .min_by(|a, b| distance(a.bounds).total_cmp(&distance(b.bounds)))?;
        let index = run
            .layout
            .index_for_position(point(position.x, within(y, run.bounds)))
            .unwrap_or_else(|index| index);
        Some(run.start + run.text.floor_char_boundary(index.min(run.text.len())))
    }
}

impl Selection {
    pub(super) fn new(
        pieces: Arc<Vec<Piece>>,
        message_len: usize,
        focus: FocusHandle,
        cx: &mut App,
    ) -> Self {
        let handle = TextSelectionHandle::new("", cx);
        let model = Rc::new(RefCell::new(Model {
            pieces,
            message_len,
            ..Model::default()
        }));
        let participant = handle.entity_id();
        let changes = handle.subscribe(
            {
                let model = model.clone();
                move |event, cx| {
                    let TextSelectionEvent::SelectionChanged(snapshot) = event else {
                        return;
                    };
                    let ours =
                        |endpoint: TextSelectionEndpoint| endpoint.entity_id() == Some(participant);
                    let mut guard = model.borrow_mut();
                    let state = &mut *guard;
                    let before = (state.anchor, state.cursor);
                    match snapshot {
                        Some(snapshot)
                            if state.live && ours(snapshot.anchor()) && ours(snapshot.cursor()) =>
                        {
                            // An endpoint's key arrives when the window resolves
                            // it, just after the event that created it; keep the
                            // previous offset meanwhile. A repeated endpoint keeps
                            // the offset `register` may since have followed.
                            take_offset(
                                snapshot.anchor(),
                                &mut state.seen_anchor,
                                &mut state.anchor,
                            );
                            take_offset(
                                snapshot.cursor(),
                                &mut state.seen_cursor,
                                &mut state.cursor,
                            );
                        }
                        // Select All is participant-local; the window's own
                        // clearing reaches it through `clear_with`.
                        _ => state.clear_endpoints(),
                    }
                    let view = state
                        .view
                        .filter(|_| (state.anchor, state.cursor) != before);
                    drop(guard);
                    if let Some(view) = view {
                        cx.notify(view);
                    }
                }
            },
            cx,
        );
        handle.clear_with(
            {
                let model = model.clone();
                move |cx| {
                    // A click clears the selection but keeps the painted runs:
                    // the same press resolves its new anchor from them.
                    let mut state = model.borrow_mut();
                    state.clear();
                    let view = state.view;
                    drop(state);
                    if let Some(view) = view {
                        cx.notify(view);
                    }
                }
            },
            cx,
        );
        handle.copy_with(
            {
                let model = model.clone();
                move |_| model.borrow().selected_text()
            },
            cx,
        );
        handle.resolve_content_key_with(
            {
                let model = model.clone();
                move |point, _| {
                    model
                        .borrow()
                        .offset_at(point)
                        .map(|offset| TextSelectionContentKey::new(offset as u64))
                }
            },
            cx,
        );
        handle.focus_with(move |window, cx| focus.focus(window, cx), cx);
        Self {
            handle,
            model,
            _changes: Rc::new(changes),
        }
    }

    /// A selectable piece of the message.
    pub(super) fn piece(&self, piece: &Piece) -> Run {
        self.run(piece.start, piece.text.clone())
    }

    /// Selectable metadata lines, which follow the message in the document.
    /// The lines are retained for copying while they stay the same.
    pub(crate) fn metadata<const N: usize>(&self, lines: [SharedString; N]) -> [Run; N] {
        let mut start = {
            let mut model = self.model.borrow_mut();
            if model.metadata[..] != lines[..] {
                model.metadata = lines.to_vec();
            }
            model.metadata_start()
        };
        lines.map(|text| {
            let run = self.run(start, text);
            start += run.text.len() + 1;
            run
        })
    }

    fn run(&self, start: usize, text: SharedString) -> Run {
        Run {
            start,
            styled: StyledText::new(text.clone()),
            text,
            model: self.model.clone(),
        }
    }

    /// The viewport-sized element that registers the participant after the
    /// list paints, with the runs it painted.
    pub(crate) fn registrar(&self, scroll: ListState) -> impl IntoElement {
        let selection = self.clone();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                selection.register(
                    bounds,
                    hitbox,
                    scroll.scroll_px_offset_for_scrollbar(),
                    window,
                    cx,
                )
            },
        )
        .absolute()
        .inset_0()
    }

    fn register(
        &self,
        bounds: Bounds<Pixels>,
        hitbox: Hitbox,
        scroll: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let participant = self.handle.entity_id();
        let dragging = self.handle.snapshot(cx).is_some_and(|snapshot| {
            snapshot.is_selecting() && snapshot.cursor().entity_id() == Some(participant)
        });
        let (runs, followed) = {
            let mut model = self.model.borrow_mut();
            model.viewport = bounds;
            model.scroll = scroll;
            model.view = Some(window.current_view());
            model.live = true;
            model.painted = std::mem::take(&mut model.painting);
            // The window resolved a held drag's cursor against the previous
            // frame. When the list scrolls under a still pointer, as
            // auto-scroll does before a release without a move, follow the
            // pointer into the text this frame painted.
            let cursor = (dragging && model.cursor.is_some())
                .then(|| model.offset_at_window(window.mouse_position()))
                .flatten()
                .filter(|cursor| model.cursor != Some(*cursor));
            if cursor.is_some() {
                model.cursor = cursor;
            }
            let runs = model
                .painted
                .iter()
                .map(|run| TextSelectionRun::new(run.text.clone(), run.layout.clone(), run.bounds))
                .collect::<Vec<_>>();
            (runs, cursor.is_some())
        };
        self.handle.register(
            TextSelectionRegistration::new(hitbox, bounds)
                .with_scroll_offset(scroll)
                .with_text_bounds(runs.iter().map(TextSelectionRun::bounds).collect()),
            window,
            cx,
        );
        // The base resolves double- and triple-click word and line ranges
        // from these runs.
        self.handle.update_runs(&runs, cx);
        if followed {
            // This frame painted the previous cursor; a refresh during paint
            // is ignored.
            window.defer(cx, |window, _| window.refresh());
        }
    }

    /// Ends the selection of a presentation being replaced or put away into
    /// a return context or warm tab. Until its next frame the window still
    /// holds the participant, which now copies nothing, and the shaped runs
    /// kept for endpoint lookups are released here and in the participant.
    pub(super) fn detach(&self, cx: &mut App) {
        {
            let mut model = self.model.borrow_mut();
            model.clear();
            model.live = false;
            model.painting = Vec::new();
            model.painted = Vec::new();
        }
        self.handle.set_local_selection(false, cx);
        self.handle.update_runs(&[], cx);
    }

    /// Select All selects the whole message; copy then yields exactly what
    /// Copy message does.
    pub(crate) fn select_all(&self, window: &mut Window, cx: &mut App) {
        TextSelection::clear(window, cx);
        let view = {
            let mut model = self.model.borrow_mut();
            if model.message_len == 0 {
                return;
            }
            (model.anchor, model.cursor, model.all) = (None, None, true);
            model.view
        };
        self.handle.set_local_selection(true, cx);
        if let Some(view) = view {
            cx.notify(view);
        }
    }

    pub(crate) fn selected_text(&self) -> String {
        self.model.borrow().selected_text()
    }

    /// Copies a non-empty selection and reports whether it did.
    pub(crate) fn copy(&self, cx: &mut App) -> bool {
        let text = self.selected_text();
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    /// Metadata text and the painted runs' shaped lines, which the
    /// participant shares. A shaped line keeps at most a glyph (about 32
    /// bytes) per text byte; `detach` releases them.
    pub(crate) fn retained_bytes(&self) -> usize {
        let model = self.model.borrow();
        model.metadata.iter().map(|line| line.len()).sum::<usize>()
            + model
                .painted
                .iter()
                .chain(&model.painting)
                .map(|run| std::mem::size_of::<Painted>() + 256 + run.text.len() * 32)
                .sum::<usize>()
    }

    /// The window point of `offset` in the painted run that holds it, half a
    /// line below its glyph top.
    #[cfg(test)]
    pub(crate) fn point_at(&self, offset: usize) -> Option<Point<Pixels>> {
        let model = self.model.borrow();
        let run = model
            .painted
            .iter()
            .find(|run| (run.start..=run.start + run.text.len()).contains(&offset))?;
        let position = run.layout.position_for_index(offset - run.start)?;
        Some(point(
            position.x,
            position.y + run.layout.line_height() / 2.,
        ))
    }

    #[cfg(test)]
    pub(crate) fn offset_at_window(&self, position: Point<Pixels>) -> Option<usize> {
        self.model.borrow().offset_at_window(position)
    }

    #[cfg(test)]
    pub(crate) fn painted_starts(&self) -> Vec<usize> {
        self.model
            .borrow()
            .painted
            .iter()
            .map(|run| run.start)
            .collect()
    }
}

/// One selectable run: a message piece or a metadata line, laid out and
/// painted as plain text with its part of the selection behind it.
pub(crate) struct Run {
    start: usize,
    text: SharedString,
    styled: StyledText,
    model: Rc<RefCell<Model>>,
}

impl IntoElement for Run {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Run {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.styled.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled.layout().clone();
        let selected = self.model.borrow().run_range(self.start, self.text.len());
        if let Some((range, through_end)) = selected {
            // The fitted text selection of the applied theme, which keeps
            // text at 4.5:1 or more on it.
            let color = rgb(appearance::editor_highlights(cx).selection);
            if let (Some(start), Some(end)) = (
                layout.position_for_index(range.start),
                layout.position_for_index(range.end),
            ) {
                for quad in selection_quads(start, end, bounds, layout.line_height(), through_end) {
                    window.paint_quad(fill(quad, color));
                }
            }
        }
        self.styled
            .paint(id, inspector_id, bounds, &mut (), &mut (), window, cx);
        window.set_cursor_style(CursorStyle::IBeam, hitbox);
        self.model.borrow_mut().painting.push(Painted {
            start: self.start,
            text: self.text.clone(),
            layout,
            bounds,
        });
    }
}

/// The highlight from `start` to `end` in a run's `bounds`: the rest of the
/// first line, any whole lines between, then the last line up to `end`, or to
/// the right edge when the selection runs through the line break after it.
fn selection_quads(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    through_end: bool,
) -> Vec<Bounds<Pixels>> {
    let last_right = if through_end { bounds.right() } else { end.x };
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            point(last_right, end.y + line_height),
        )];
    }
    let mut quads = vec![Bounds::from_corners(
        start,
        point(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        quads.push(Bounds::from_corners(
            point(bounds.left(), start.y + line_height),
            point(bounds.right(), end.y),
        ));
    }
    quads.push(Bounds::from_corners(
        point(bounds.left(), end.y),
        point(last_right, end.y + line_height),
    ));
    quads
}

#[cfg(test)]
mod tests {
    use super::super::{append_pieces, full_message, tests::commit};
    use super::*;
    use core::prelude::v1::test;

    /// The model of a message, and the loaded message it must copy.
    fn document(subject: &str, body: &str) -> (Model, String) {
        let mut pieces = Vec::new();
        append_pieces(&mut pieces, subject, true, 0);
        let body_start = if subject.is_empty() {
            0
        } else {
            subject.len() + 2
        };
        append_pieces(&mut pieces, body, false, body_start);
        let message_len = pieces
            .last()
            .map_or(0, |piece| piece.start + piece.text.len());
        let model = Model {
            pieces: Arc::new(pieces),
            message_len,
            live: true,
            ..Model::default()
        };
        (model, full_message(&commit("a", subject, body)))
    }

    #[test]
    fn document_text_restores_every_byte_the_pieces_do_not_display() {
        let crlf = "Paragraph 東京.\r\n\r\n".repeat(2_000);
        let wide = "x".repeat(5_000);
        let lines = (0..300)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let breaks = "\n".repeat(100);
        let graphemes = "東京👩🏽‍💻e\u{301}".repeat(5_000);
        for (subject, body) in [
            ("Title 🐢", ""),
            ("", "Body only.\n\nSecond paragraph."),
            ("Title", crlf.as_str()),
            (wide.as_str(), lines.as_str()),
            ("Title", breaks.as_str()),
            (graphemes.as_str(), graphemes.as_str()),
        ] {
            let (mut model, expected) = document(subject, body);
            assert_eq!(model.message_len, expected.len());
            assert!(
                model.text(0..model.message_len) == expected,
                "{subject:.20?}"
            );
            let len = expected.len();
            for (from, to) in [(1, len / 2), (len / 3, len.saturating_sub(1)), (0, 1)] {
                let (from, to) = (
                    expected.floor_char_boundary(from),
                    expected.floor_char_boundary(to),
                );
                assert!(model.text(from..to) == expected[from..to], "{from}..{to}");
            }
            model.all = true;
            assert!(model.selected_text() == expected);
        }
    }

    #[test]
    fn metadata_follows_the_message_after_a_blank_line() {
        let (mut model, message) = document("Title", "Body");
        model.metadata = vec!["Author".into(), "Date · abc".into()];
        let end = model.metadata_start() + "Author\nDate".len();
        assert_eq!(
            model.text(2..end),
            format!("{}\n\nAuthor\nDate", &message[2..])
        );
        // Select All is the message alone.
        model.all = true;
        assert_eq!(model.selected_text(), message);
        // A presentation replaced or put away copies nothing.
        model.live = false;
        assert_eq!(model.selected_text(), "");

        let (mut empty, _) = document("", "");
        empty.metadata = vec!["Author".into(), "Date".into()];
        assert_eq!(empty.text(0.."Author\nDa".len()), "Author\nDa");
    }

    #[test]
    fn a_run_covers_its_part_and_the_break_the_selection_runs_through() {
        // "Title" is 0..5 and "Body" 7..11.
        let (mut model, _) = document("Title", "Body");
        (model.anchor, model.cursor) = (Some(9), Some(2));
        assert_eq!(model.run_range(0, 5), Some((2..5, true)));
        assert_eq!(model.run_range(7, 4), Some((0..2, false)));
        (model.anchor, model.cursor) = (Some(5), Some(8));
        assert_eq!(model.run_range(0, 5), Some((5..5, true)));
        (model.anchor, model.cursor) = (Some(0), Some(7));
        assert_eq!(model.run_range(7, 4), None);
        (model.anchor, model.cursor) = (Some(3), Some(3));
        assert_eq!(model.run_range(0, 5), None);
        assert_eq!(model.selected_text(), "");
    }

    #[test]
    fn a_highlight_through_a_line_break_reaches_the_right_edge() {
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(100.), px(60.)));
        let line = px(20.);
        let (start, end) = (point(px(40.), px(20.)), point(px(30.), px(60.)));
        assert_eq!(
            selection_quads(start, end, bounds, line, false),
            vec![
                Bounds::from_corners(start, point(px(110.), px(40.))),
                Bounds::from_corners(point(px(10.), px(40.)), point(px(110.), px(60.))),
                Bounds::from_corners(point(px(10.), px(60.)), point(px(30.), px(80.))),
            ]
        );
        assert_eq!(
            selection_quads(start, point(px(70.), px(20.)), bounds, line, true),
            vec![Bounds::from_corners(start, point(px(110.), px(40.)))]
        );
    }
}
