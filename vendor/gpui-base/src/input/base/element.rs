use crate::input::{InputExtras as _, InputModeKind};
use gpui::Corners;
use gpui::Half;
use gpui::{
    AnyElement, App, Bounds, Edges, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId,
};
use gpui::{
    HighlightStyle, Hitbox, HitboxBehavior, Hsla, InteractiveElement, IntoElement, LayoutId,
    MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement as _, Path, Pixels, Point, Position,
    ShapedLine, SharedString, Size, Style, Styled as _, TextAlign, TextRun, TextStyle,
    UnderlineStyle, Window, fill, point, px, relative, size,
};
use ropey::Rope;
use smallvec::SmallVec;
use std::{ops::Range, rc::Rc};

use crate::{
    Scrollbar,
    input::{RopeExt as _, blink_cursor::CURSOR_WIDTH, display_map::LineLayout},
};

use super::{
    InputBaseState, TextDecoration,
    layout::{LastLayout, WhitespaceIndicators},
    mode::LayoutMode,
};

fn diagnostic_highlight_style(
    severity: crate::input::DiagnosticSeverity,
    colors: crate::input::DiagnosticColors,
) -> HighlightStyle {
    let color = match severity {
        crate::input::DiagnosticSeverity::Error => colors.error,
        crate::input::DiagnosticSeverity::Warning => colors.warning,
        crate::input::DiagnosticSeverity::Info => colors.info,
        crate::input::DiagnosticSeverity::Hint => colors.hint,
    };
    HighlightStyle {
        underline: Some(UnderlineStyle {
            color: Some(color),
            thickness: px(1.),
            wavy: true,
        }),
        ..Default::default()
    }
}

const BOTTOM_MARGIN_ROWS: usize = 3;
pub(super) const RIGHT_MARGIN: Pixels = px(10.);
/// The space between the line numbers and a line's text. A code editor keeps it
/// before its text without line numbers too, inside its padding, so a caller
/// that draws its own gutter beside one can start text where the editor does.
pub const LINE_NUMBER_RIGHT_MARGIN: Pixels = px(10.);
const FOLD_ICON_WIDTH: Pixels = px(14.);
const FOLD_ICON_HITBOX_WIDTH: Pixels = px(18.);
const MAX_HIGHLIGHT_LINE_LENGTH: usize = 10_000;
const FOLD_CHEVRON_RIGHT_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>"#;
const FOLD_CHEVRON_DOWN_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>"#;

fn compose_decorations(
    mut styles: Vec<(Range<usize>, HighlightStyle)>,
    decorations: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    visible_byte_range: Range<usize>,
) -> Option<Vec<(Range<usize>, HighlightStyle)>> {
    let mut visible_decorations = decorations
        .into_iter()
        .filter_map(|(range, style)| {
            let range =
                range.start.max(visible_byte_range.start)..range.end.min(visible_byte_range.end);
            (!range.is_empty()).then_some((range, style))
        })
        .peekable();

    if visible_decorations.peek().is_none() {
        return (!styles.is_empty()).then_some(styles);
    }
    if styles.is_empty() {
        styles.push((visible_byte_range.clone(), HighlightStyle::default()));
    }

    // Decorations are application-authored overrides and must win over syntax
    // and semantic highlighting when both set the same style property.
    Some(gpui::combine_highlights(styles, visible_decorations).collect())
}

/// Compose the collections' styles, and (a GitTurtle patch) put their row
/// backgrounds that reach the visible lines in `rows` ([`read_row_background`])
/// in the same pass, so a frame walks each decoration once.
fn compose_decoration_collections<'a>(
    mut styles: Vec<(Range<usize>, HighlightStyle)>,
    collections: impl IntoIterator<Item = &'a [TextDecoration]>,
    visible_byte_range: Range<usize>,
    rows: &mut RowBackgrounds,
) -> Option<Vec<(Range<usize>, HighlightStyle)>> {
    // Apply collections in reverse creation order so the first collection is
    // composed last and retains its documented precedence.
    let collections = collections.into_iter().collect::<Vec<_>>();
    for (index, decorations) in collections.into_iter().enumerate().rev() {
        styles = compose_decorations(
            styles,
            decorations.iter().map(|decoration| {
                read_row_background(decoration, index, &visible_byte_range, rows);
                (decoration.range.clone(), decoration.style)
            }),
            visible_byte_range.clone(),
        )
        .unwrap_or_default();
    }

    (!styles.is_empty()).then_some(styles)
}

/// A decoration's row background ([`TextDecoration::row_background`]) that
/// reaches the visible lines: its absolute byte range, its colour and its
/// collection's index, the first collection's fill taking a line where
/// several reach it.
type RowBackground = (Range<usize>, Hsla, usize);

/// The row backgrounds a frame reads, inline for the few blocks a view
/// shows, so a frame that reads them allocates nothing for them.
type RowBackgrounds = SmallVec<[RowBackground; 8]>;

/// Put `decoration`'s row background in `rows` where its range reaches the
/// visible lines, whose bytes `visible` holds to the last one's end: a range
/// reaches a line where it covers some of its text or its line break, so a
/// range over only the last visible line's break, as over a blank line, is
/// read too, though the styles' visible range leaves that byte out.
fn read_row_background(
    decoration: &TextDecoration,
    index: usize,
    visible: &Range<usize>,
    rows: &mut RowBackgrounds,
) {
    if let Some(color) = decoration.row_background
        && decoration.range.start <= visible.end
        && decoration.range.end > visible.start
    {
        rows.push((decoration.range.clone(), color, index));
    }
}

/// The row background to paint on the buffer line whose text spans
/// `line_start..line_end`, its line break at `line_end`: the first
/// collection's among the `rows` reaching it, and within one collection the
/// one starting first. `rows` ascend by start, the first ending past
/// `line_start`.
fn row_background_at(rows: &[RowBackground], line_start: usize, line_end: usize) -> Option<Hsla> {
    rows.iter()
        .take_while(|(range, ..)| range.start <= line_end)
        .filter(|(range, ..)| range.end > line_start)
        .min_by_key(|(_, _, index)| *index)
        .map(|(_, color, _)| *color)
}

/// Compose the decoration layers: upstream's composition where no decoration
/// carries a bottom rule (`ruled`, kept as the decorations are set, so such a
/// frame does no rule work and leaves `rules` empty), and
/// [`compose_ruled_decoration_collections`] where one does. Either puts the
/// row backgrounds reaching the visible lines in `rows`, ascending by start.
fn compose_decoration_layers(
    styles: Vec<(Range<usize>, HighlightStyle)>,
    collections: Vec<&[TextDecoration]>,
    ruled: bool,
    visible_byte_range: Range<usize>,
    rules: &mut Vec<(Range<usize>, Hsla)>,
    rows: &mut RowBackgrounds,
) -> Option<Vec<(Range<usize>, HighlightStyle)>> {
    rows.clear();
    let styles = if ruled {
        compose_ruled_decoration_collections(styles, collections, visible_byte_range, rules, rows)
    } else {
        compose_decoration_collections(styles, collections, visible_byte_range, rows)
    };
    rows.sort_unstable_by_key(|(range, _, index)| (range.start, *index));
    styles
}

/// Compose the collections' styles as [`compose_decoration_collections`]
/// does, and put their visible bottom rules ([`TextDecoration::bottom_rule`])
/// in `rules`, ascending by start. A rule is left out where an earlier
/// collection draws an underline, as GitTurtle's Find marks its current
/// match, so two strokes never run one above the other; under an earlier
/// collection's background alone it is kept. The rules are read in the pass
/// that composes the styles, so a frame walks each decoration once, and each
/// rule is checked only against the underlines that can reach it.
fn compose_ruled_decoration_collections<'a>(
    mut styles: Vec<(Range<usize>, HighlightStyle)>,
    collections: impl IntoIterator<Item = &'a [TextDecoration]>,
    visible_byte_range: Range<usize>,
    rules: &mut Vec<(Range<usize>, Hsla)>,
    rows: &mut RowBackgrounds,
) -> Option<Vec<(Range<usize>, HighlightStyle)>> {
    let visible = |range: &Range<usize>| {
        let range =
            range.start.max(visible_byte_range.start)..range.end.min(visible_byte_range.end);
        (!range.is_empty()).then_some(range)
    };
    // Each visible rule with its collection's index, and the visible
    // underlines of the collections before a ruled one, with theirs.
    let mut ruled: Vec<(usize, Range<usize>, Hsla)> = Vec::new();
    let mut underlines: Vec<(usize, Range<usize>)> = Vec::new();
    // Apply collections in reverse creation order so the first collection is
    // composed last and retains its documented precedence.
    let collections = collections.into_iter().collect::<Vec<_>>();
    for (index, decorations) in collections.into_iter().enumerate().rev() {
        // Only a later collection's rule can be hidden by this one's underlines.
        let hides = !ruled.is_empty();
        styles = compose_decorations(
            styles,
            decorations.iter().map(|decoration| {
                read_row_background(decoration, index, &visible_byte_range, rows);
                if let Some(color) = decoration.bottom_rule
                    && let Some(range) = visible(&decoration.range)
                {
                    ruled.push((index, range, color));
                }
                if hides
                    && decoration.style.underline.is_some()
                    && let Some(range) = visible(&decoration.range)
                {
                    underlines.push((index, range));
                }
                (decoration.range.clone(), decoration.style)
            }),
            visible_byte_range.clone(),
        )
        .unwrap_or_default();
    }

    rules.clear();
    underlines.sort_unstable_by_key(|(_, underline)| underline.start);
    // The furthest any of the first underlines reaches, so a rule passes over
    // every underline that ends before it with one search.
    let reach = underlines
        .iter()
        .scan(0, |reach, (_, underline)| {
            *reach = underline.end.max(*reach);
            Some(*reach)
        })
        .collect::<Vec<_>>();
    for (index, range, color) in ruled {
        let first = reach.partition_point(|&end| end <= range.start);
        let mut start = range.start;
        for (earlier, underline) in underlines[first..]
            .iter()
            .take_while(|(_, underline)| underline.start < range.end)
        {
            if *earlier >= index || underline.end <= start {
                continue;
            }
            if start < underline.start {
                rules.push((start..underline.start, color));
            }
            start = underline.end;
        }
        if start < range.end {
            rules.push((start..range.end, color));
        }
    }
    rules.sort_by_key(|(range, _)| range.start);

    (!styles.is_empty()).then_some(styles)
}

/// For each of `indices`, the x of the first of `glyphs`, in the order the
/// background pass walks them, whose byte index is at least that index: the
/// glyph at which that pass starts or ends a run's box. One walk answers them
/// all, whatever the glyphs' order (a right-to-left run's indices fall): the
/// indices are answered in ascending order, each by the first glyph that
/// reaches it, so a glyph whose index falls below one walked before reaches
/// only indices that glyph already answered. `None` where no glyph does.
fn first_glyphs_at(
    glyphs: impl IntoIterator<Item = (usize, Pixels)>,
    indices: &[usize],
) -> SmallVec<[Option<Pixels>; 8]> {
    let mut order = (0..indices.len()).collect::<SmallVec<[usize; 8]>>();
    order.sort_unstable_by_key(|&slot| indices[slot]);
    let mut found = SmallVec::from_elem(None, indices.len());
    let mut next = 0;
    for (index, x) in glyphs {
        if next == order.len() {
            break;
        }
        while next < order.len() && indices[order[next]] <= index {
            found[order[next]] = Some(x);
            next += 1;
        }
    }
    found
}

/// Paint the bottom rules ([`TextDecoration::bottom_rule`]) on one buffer
/// line, whose first byte is `line_start`, laid out at `origin` as the
/// background pass lays it out. `rules` hold absolute byte ranges ascending by
/// start, the first ending past `line_start`.
///
/// Each rule spans the box `paint_background` fills for its range on each
/// visual line, from the range's first glyph to the glyph after it, or the
/// line's end: the glyphs are walked as that pass walks them, so the rule's
/// edges and the box's are the same floats and land on the same device
/// pixels. Adjacent rules of one colour join, as adjacent backgrounds of one
/// colour fill one box. The rule takes the box's bottom edge on its device
/// pixel and a 1 px stroke's device pixels above it, so it stands inside the
/// box on whole device pixels at every scale. A range without width paints
/// nothing, as its box has none.
#[allow(clippy::too_many_arguments)]
fn paint_bottom_rules(
    line: &LineLayout,
    line_start: usize,
    origin: Point<Pixels>,
    line_height: Pixels,
    text_align: TextAlign,
    align_width: Option<Pixels>,
    rules: &[(Range<usize>, Hsla)],
    window: &mut Window,
) {
    let line_end = line_start
        + line
            .wrapped_lines
            .iter()
            .map(|line| line.len)
            .sum::<usize>();
    if rules
        .first()
        .is_none_or(|(range, _)| range.start >= line_end)
    {
        return;
    }
    let scale = window.scale_factor();
    let stroke = round_half_toward_zero(scale).max(1.);
    let mut start = line_start;
    for (ix, shaped) in line.wrapped_lines.iter().enumerate() {
        let end = start + shaped.len;
        let indent = if ix == 0 { px(0.) } else { line.wrap_indent };
        let pos = origin + point(indent, ix * line_height);
        // This visual line's ranges, local to it, joined where they touch in one colour.
        let mut local: SmallVec<[(Range<usize>, Hsla); 4]> = SmallVec::new();
        for (range, color) in rules
            .iter()
            .take_while(|(range, _)| range.start < end)
            .filter(|(range, _)| range.end > start)
        {
            let range = range.start.max(start) - start..range.end.min(end) - start;
            match local.last_mut() {
                Some((last, last_color)) if last.end == range.start && last_color == color => {
                    last.end = range.end
                }
                _ => local.push((range, *color)),
            }
        }
        start = end;
        if local.is_empty() {
            continue;
        }
        // The glyph walk of the background pass: each glyph's x, accumulated.
        let layout: &gpui::LineLayout = shaped;
        let align_width = align_width.unwrap_or(layout.width);
        let mut x = match text_align {
            TextAlign::Left => pos.x,
            TextAlign::Center => (pos.x * 2.0 + align_width - layout.width) / 2.0,
            TextAlign::Right => pos.x + align_width - layout.width,
        };
        let mut previous = px(0.);
        let glyphs = layout
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter())
            .map(|glyph| {
                x += glyph.position.x - previous;
                previous = glyph.position.x;
                (glyph.index, x)
            });
        let edges = local
            .iter()
            .flat_map(|(range, _)| [range.start, range.end])
            .collect::<SmallVec<[usize; 8]>>();
        let edges = first_glyphs_at(glyphs, &edges);
        let bottom = round_half_toward_zero((pos.y + line_height).as_f32() * scale);
        let (top, bottom) = (px((bottom - stroke) / scale), px(bottom / scale));
        let quads = local
            .into_iter()
            .enumerate()
            .filter_map(|(rule, (_, color))| {
                let left = edges[2 * rule]?;
                let right = edges[2 * rule + 1].unwrap_or(pos.x + layout.width);
                (right > left).then(|| (left, right, color))
            })
            .collect::<SmallVec<[(Pixels, Pixels, Hsla); 4]>>();
        let Some(&(left, right, _)) = quads.first() else {
            continue;
        };
        let (left, right) = quads.iter().fold((left, right), |(left, right), quad| {
            (left.min(quad.0), right.max(quad.1))
        });
        // One layer a visual line, as the background pass paints its boxes,
        // so the scene orders the line's rules together, over its boxes and
        // under the guides, selections and text painted after.
        let layer = Bounds::from_corners(point(left, top), point(right, bottom));
        window.paint_layer(layer, |window| {
            for (left, right, color) in quads {
                window.paint_quad(fill(
                    Bounds::from_corners(point(left, top), point(right, bottom)),
                    color,
                ));
            }
        });
    }
}

/// GPUI's device-pixel rounding, half toward zero, as `paint_quad` snaps edges.
fn round_half_toward_zero(value: f32) -> f32 {
    (value.abs() - 0.5).ceil().copysign(value)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct EditorScrollbarLayout {
    bounds: Bounds<Pixels>,
    scroll_size: Size<Pixels>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EditorScrollbarSnapshot {
    layout: EditorScrollbarLayout,
    cursor_scroll_offset: Point<Pixels>,
    soft_wrap: bool,
}

impl EditorScrollbarSnapshot {
    fn new<M: InputModeKind>(
        input_bounds: Bounds<Pixels>,
        last_layout: &LastLayout,
        scroll_size: Size<Pixels>,
        cursor_scroll_offset: Point<Pixels>,
        state: &InputBaseState<M>,
    ) -> Self {
        Self {
            layout: EditorScrollbarLayout::new(
                input_bounds,
                last_layout.line_number_width,
                scroll_size,
                state.editor_paddings,
            ),
            cursor_scroll_offset,
            soft_wrap: state.soft_wrap,
        }
    }
}

impl EditorScrollbarLayout {
    fn new(
        input_bounds: Bounds<Pixels>,
        line_number_width: Pixels,
        scroll_size: Size<Pixels>,
        paddings: Edges<Pixels>,
    ) -> Self {
        let left = if line_number_width == px(0.) {
            px(0.)
        } else {
            paddings.left + line_number_width - LINE_NUMBER_RIGHT_MARGIN
        };

        Self {
            bounds: Bounds::new(
                point(
                    input_bounds.origin.x + left,
                    input_bounds.origin.y - paddings.top,
                ),
                size(
                    input_bounds.size.width - left + paddings.right,
                    input_bounds.size.height + paddings.top + paddings.bottom,
                ),
            ),
            scroll_size: size(
                scroll_size.width - left + paddings.right + RIGHT_MARGIN,
                scroll_size.height,
            ),
        }
    }
}

pub(super) struct EditorScrollbar<M: InputModeKind> {
    state: Entity<InputBaseState<M>>,
}

impl<M: InputModeKind> EditorScrollbar<M> {
    pub(super) fn new(state: Entity<InputBaseState<M>>) -> Self {
        Self { state }
    }
}

impl<M: InputModeKind> IntoElement for EditorScrollbar<M> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<M: InputModeKind> Element for EditorScrollbar<M> {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        Some("editor-scrollbar".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();

        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = self.state.read(cx);
        let Some(snapshot) = state.editor_scrollbar_snapshot.get() else {
            return None;
        };
        let scroll_handle = state.scroll_handle.clone();

        if scroll_handle.offset() != snapshot.cursor_scroll_offset {
            scroll_handle.set_offset(snapshot.cursor_scroll_offset);
        }

        let mut scrollbar = if !snapshot.soft_wrap {
            Scrollbar::new(&scroll_handle)
        } else {
            Scrollbar::vertical(&scroll_handle)
        }
        .viewport_bounds(snapshot.layout.bounds)
        .scroll_size(snapshot.layout.scroll_size)
        .into_any_element();

        scrollbar.prepaint_as_root(
            snapshot.layout.bounds.origin,
            snapshot.layout.bounds.size.into(),
            window,
            cx,
        );
        Some(scrollbar)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(scrollbar) = prepaint.as_mut() {
            scrollbar.paint(window, cx);
        }
    }
}

fn clamp_auto_grow_vertical_scroll_offset(
    mode: &LayoutMode,
    scroll_top: Pixels,
    scroll_height: Pixels,
    input_height: Pixels,
) -> Pixels {
    if mode.is_auto_grow() {
        scroll_top.clamp((input_height - scroll_height).min(px(0.)), px(0.))
    } else {
        scroll_top
    }
}

fn editor_gutter_bounds(
    input_bounds: Bounds<Pixels>,
    line_number_width: Pixels,
    ghost_lines_height: Pixels,
    paddings: Edges<Pixels>,
) -> Bounds<Pixels> {
    Bounds {
        origin: point(
            input_bounds.origin.x - paddings.left,
            input_bounds.origin.y - paddings.top,
        ),
        size: size(
            line_number_width + paddings.left,
            input_bounds.size.height + ghost_lines_height + paddings.top + paddings.bottom,
        ),
    }
}

use super::MASK_CHAR;

/// Convert a byte offset in the original text to a byte offset in the masked display string.
///
/// The masked string consists of `MASK_CHAR` repeated once per character in the original text.
/// Since `MASK_CHAR` may be multi-byte in UTF-8, the byte offset in the masked string is
/// `char_index * MASK_CHAR.len_utf8()`.
fn masked_display_offset(text: &Rope, original_offset: usize) -> usize {
    text.offset_to_char_index(original_offset) * MASK_CHAR.len_utf8()
}

/// Move the IME marked range (tracked against the original text) into the display text
/// coordinate space, so that a run boundary can't land inside a multi-byte `MASK_CHAR`
/// and panic text shaping on a non-char-boundary slice.
fn ime_marked_display_range(
    text: &Rope,
    marked_range: Option<Range<usize>>,
    masked: bool,
) -> Option<Range<usize>> {
    let marked = marked_range?;
    if masked {
        Some(masked_display_offset(text, marked.start)..masked_display_offset(text, marked.end))
    } else {
        Some(marked)
    }
}

/// Minimum pixel padding the cursor is kept clear of the viewport's
/// top/bottom edges before auto-scroll engages. Backs
/// [`InputBaseState::cursor_surrounding_lines`].
///
/// Auto-grow uses one line. Otherwise `None` falls back to the historical
/// heuristic ([`BOTTOM_MARGIN_ROWS`] lines, or one line on small
/// viewports); `Some(n)` uses `n` lines. The result is saturated against
/// half the viewport so an oversized override can't invert the
/// top/bottom thresholds into a scroll feedback loop.
pub(super) fn cursor_surrounding_padding(
    is_auto_grow: bool,
    override_lines: Option<usize>,
    visible_lines: usize,
    line_height: Pixels,
) -> Pixels {
    if is_auto_grow {
        return line_height;
    }
    let raw = match override_lines {
        Some(lines) => lines as f32 * line_height,
        None => {
            if visible_lines < BOTTOM_MARGIN_ROWS * 8 {
                line_height
            } else {
                BOTTOM_MARGIN_ROWS * line_height
            }
        }
    };
    // Saturate against half the viewport so top + bottom margins can coexist.
    let viewport_half = (visible_lines as f32 * line_height).half();
    raw.min(viewport_half)
}

/// Pixel height of the empty area below the last line in the editor's
/// scrollable region. Backs [`InputBaseState::scroll_beyond_last_line`].
///
/// `0` outside code-editor mode. Inside it, `None` is half the viewport
/// (floored at [`BOTTOM_MARGIN_ROWS`] line-heights); `Some(n)` is exactly
/// `n` line-heights.
fn empty_bottom_height(
    is_code_editor: bool,
    override_rows: Option<usize>,
    viewport_height: Pixels,
    line_height: Pixels,
) -> Pixels {
    if !is_code_editor {
        return px(0.);
    }
    match override_rows {
        Some(rows) => rows as f32 * line_height,
        None => viewport_height.half().max(BOTTOM_MARGIN_ROWS * line_height),
    }
}

/// Layout information for fold icons.
struct FoldIconLayout {
    /// Hitbox for the line number area (used for hover detection)
    line_number_hitbox: Hitbox,
    /// List of (display_row, is_folded, icon_element) pairs for each fold candidate
    icons: Vec<(usize, bool, gpui::AnyElement)>,
}

pub(super) struct TextElement<M: InputModeKind> {
    pub(crate) state: Entity<InputBaseState<M>>,
    placeholder: SharedString,
}

impl<M: InputModeKind> TextElement<M> {
    pub(super) fn new(state: Entity<InputBaseState<M>>) -> Self {
        Self {
            state,
            placeholder: SharedString::default(),
        }
    }

    /// Set the placeholder text of the input field.
    pub(super) fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    fn paint_mouse_listeners(&mut self, window: &mut Window, _: &mut App) {
        window.on_mouse_event({
            let state = self.state.clone();

            move |event: &MouseMoveEvent, _, window, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    state.update(cx, |state, cx| {
                        state.on_drag_move(event, window, cx);
                    });
                }
            }
        });

        window.on_mouse_event({
            let state = self.state.clone();
            move |_: &MouseUpEvent, phase, _, cx| {
                if !phase.bubble() {
                    return;
                }

                // Stop auto-scroll when mouse up, and also stop selecting.
                state.update(cx, |state, _| {
                    state.auto_scroll.stop();
                    state.selecting = false;
                });
            }
        });
    }

    /// Returns the:
    ///
    /// - cursor bounds
    /// - scroll offset
    /// - current row index (No only the visible lines, but all lines)
    ///
    /// This method also will update for track scroll to cursor.
    fn layout_cursor(
        &self,
        last_layout: &LastLayout,
        bounds: &mut Bounds<Pixels>,
        scroll_size: Size<Pixels>,
        _: &mut Window,
        cx: &mut App,
    ) -> (Option<Bounds<Pixels>>, Point<Pixels>, Option<usize>) {
        let state = self.state.read(cx);

        let line_height = last_layout.line_height;
        let visible_range = &last_layout.visible_range;
        let lines = &last_layout.lines;
        let line_number_width = last_layout.line_number_width;

        let mut selected_range = state.selected_range;

        if let Some(ime_marked_range) = &state.ime_marked_range {
            selected_range = (ime_marked_range.end..ime_marked_range.end).into();
        }
        let is_selected_all = selected_range.len() == state.text.len();

        let mut cursor = state.cursor();
        // Buffer rows from the raw (pre-mask) offsets, used to locate the cursor line.
        let cursor_row = state.text.offset_to_point(cursor).row;
        let sel_start_row = state.text.offset_to_point(selected_range.start).row;
        let sel_end_row = state.text.offset_to_point(selected_range.end).row;
        if state.masked {
            selected_range.start = masked_display_offset(&state.text, selected_range.start);
            selected_range.end = masked_display_offset(&state.text, selected_range.end);
            cursor = masked_display_offset(&state.text, cursor);
        }

        let mut scroll_offset = state.scroll_handle.offset();

        // Padding kept between the cursor and the viewport's top/bottom
        // edges, used by the auto-scroll-into-view computation below.
        let top_bottom_margin = cursor_surrounding_padding(
            state.mode.is_auto_grow(),
            state.cursor_surrounding_lines,
            visible_range.len(),
            line_height,
        );

        // Resolve a cursor or selection endpoint to a content-space position.
        let visible_buffer_lines = &last_layout.visible_buffer_lines;
        let caret_for = |row: usize, offset: usize, affinity: bool| -> Point<Pixels> {
            // y of the top of buffer line `row` in content space.
            let top = line_height * state.display_map.buffer_line_to_display_row(row);
            let line_origin = point(px(0.), top);

            if let Some(vi) = visible_buffer_lines.iter().position(|&bl| bl == row) {
                let line = &lines[vi];
                let line_start = last_layout.visible_line_byte_offsets[vi];
                let local = offset.saturating_sub(line_start);
                if let Some(pos) = line.position_for_index(local, last_layout, affinity) {
                    return line_origin + pos;
                }
            }
            line_origin
        };

        let current_row = Some(cursor_row);
        let cursor_pos = caret_for(cursor_row, cursor, state.cursor_line_end_affinity);
        let cursor_start = caret_for(sel_start_row, selected_range.start, false);
        let cursor_end = caret_for(sel_end_row, selected_range.end, false);

        let cursor_bounds = {
            let selection_changed = state.last_selected_range != Some(selected_range);
            let auto_scrolling = state.auto_scroll.is_active();
            if selection_changed && !is_selected_all {
                // For Right alignment use 0 margin: cursor is clamped to bounds separately,
                // so we never scroll the text for cursor-at-edge, avoiding a first-click jump.
                let safety_margin = match last_layout.text_align {
                    TextAlign::Left => RIGHT_MARGIN,
                    TextAlign::Right => px(0.),
                    TextAlign::Center => CURSOR_WIDTH,
                };

                scroll_offset.x = if scroll_offset.x + cursor_pos.x
                    > (bounds.size.width - line_number_width - safety_margin)
                {
                    // cursor is out of right
                    bounds.size.width - line_number_width - safety_margin - cursor_pos.x
                } else if scroll_offset.x + cursor_pos.x < px(0.) {
                    // cursor is out of left
                    scroll_offset.x - cursor_pos.x
                } else {
                    scroll_offset.x
                };

                // Vertical cursor-follow is suppressed while auto-scroll manages the y axis,
                // to prevent fighting the background scroll task.
                if !auto_scrolling {
                    // If we change the scroll_offset.y, GPUI will render and trigger the next run loop.
                    // So, here we just adjust offset by `line_height` for move smooth.
                    scroll_offset.y = if scroll_offset.y + cursor_pos.y
                        > bounds.size.height - top_bottom_margin
                    {
                        // cursor is out of bottom
                        scroll_offset.y - line_height
                    } else if scroll_offset.y + cursor_pos.y < top_bottom_margin {
                        // cursor is out of top
                        (scroll_offset.y + line_height).min(px(0.))
                    } else {
                        scroll_offset.y
                    };
                }

                // For selection to move scroll
                if state.selection_reversed {
                    if scroll_offset.x + cursor_start.x < px(0.) {
                        // selection start is out of left
                        scroll_offset.x = -cursor_start.x;
                    }
                    if !auto_scrolling && scroll_offset.y + cursor_start.y < px(0.) {
                        // selection start is out of top
                        scroll_offset.y = -cursor_start.y;
                    }
                } else {
                    // TODO: Consider to remove this part,
                    // maybe is not necessary (But selection_reversed is needed).
                    if scroll_offset.x + cursor_end.x <= px(0.) {
                        // selection end is out of left
                        scroll_offset.x = -cursor_end.x;
                    }
                    if !auto_scrolling && scroll_offset.y + cursor_end.y <= px(0.) {
                        // selection end is out of top
                        scroll_offset.y = -cursor_end.y;
                    }
                }
            }

            // cursor bounds
            let cursor_height = 0.85 * line_height;

            // Match the caret to the deferred scroll target (applied below) that
            // the text paints at; otherwise the caret follows the cursor-scroll
            // while the text uses the deferred offset, flashing it mid-field.
            let cursor_scroll_x = state
                .deferred_scroll_offset
                .map(|offset| offset.x)
                .unwrap_or(scroll_offset.x);

            // For Right alignment, clamp cursor within the right edge of bounds so it
            // stays visible without having to shift the text via scroll_offset.
            let cursor_x = bounds.left() + cursor_pos.x + line_number_width + cursor_scroll_x;
            let cursor_x = if last_layout.text_align == TextAlign::Right {
                cursor_x.min(bounds.right() - CURSOR_WIDTH)
            } else {
                cursor_x
            };
            Some(Bounds::new(
                point(
                    cursor_x,
                    bounds.top() + cursor_pos.y + ((line_height - cursor_height) / 2.),
                ),
                size(CURSOR_WIDTH, cursor_height),
            ))
        };

        if let Some(deferred_scroll_offset) = state.deferred_scroll_offset {
            scroll_offset = deferred_scroll_offset;
        }
        scroll_offset.y = clamp_auto_grow_vertical_scroll_offset(
            &state.mode,
            scroll_offset.y,
            scroll_size.height,
            bounds.size.height,
        );

        bounds.origin = bounds.origin + scroll_offset;

        (cursor_bounds, scroll_offset, current_row)
    }

    /// Layout the match range to a Path.
    pub(crate) fn layout_match_range(
        range: Range<usize>,
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
    ) -> Option<Path<Pixels>> {
        if range.is_empty() {
            return None;
        }

        if range.start < last_layout.visible_range_offset.start
            || range.end > last_layout.visible_range_offset.end
        {
            return None;
        }

        let line_height = last_layout.line_height;
        let visible_top = last_layout.visible_top;
        let lines = &last_layout.lines;
        let line_number_width = last_layout.line_number_width;

        let start_ix = range.start;
        let end_ix = range.end;

        // Start from visible_top (which already accounts for all lines before visible range)
        let mut offset_y = visible_top;
        let mut line_corners = vec![];

        // Iterate only over visible (non-hidden) buffer lines
        for (prev_lines_offset, line) in last_layout
            .visible_line_byte_offsets
            .iter()
            .zip(lines.iter())
        {
            let prev_lines_offset = *prev_lines_offset;
            let line_size = line.size(line_height);
            let line_wrap_width = line_size.width;

            let line_origin = point(px(0.), offset_y);

            let line_cursor_start = line.position_for_index(
                start_ix.saturating_sub(prev_lines_offset),
                last_layout,
                false,
            );
            // The end of a range closes the row it lands on: a range ending exactly on a soft
            // wrap boundary highlights to the end of that row instead of opening a zero-width
            // sliver at the start of the next one.
            let line_cursor_end = line.position_for_index(
                end_ix.saturating_sub(prev_lines_offset),
                last_layout,
                true,
            );

            if line_cursor_start.is_some() || line_cursor_end.is_some() {
                let start = line_cursor_start
                    .unwrap_or_else(|| line.position_for_index(0, last_layout, false).unwrap());

                let end = line_cursor_end.unwrap_or_else(|| {
                    line.position_for_index(line.len(), last_layout, false)
                        .unwrap()
                });

                // Split the selection into multiple items
                let wrapped_lines =
                    (end.y / line_height).ceil() as usize - (start.y / line_height).ceil() as usize;

                let mut end_x = end.x;
                if wrapped_lines > 0 {
                    end_x = line_wrap_width;
                }

                // Ensure at least 6px width for the selection for empty lines.
                end_x = end_x.max(start.x + px(6.));

                line_corners.push(Corners {
                    top_left: line_origin + point(start.x, start.y),
                    top_right: line_origin + point(end_x, start.y),
                    bottom_left: line_origin + point(start.x, start.y + line_height),
                    bottom_right: line_origin + point(end_x, start.y + line_height),
                });

                // wrapped lines
                for i in 1..=wrapped_lines {
                    let indent = line.wrap_indent;
                    let start = point(indent, start.y + i as f32 * line_height);
                    let mut end = point(end.x, end.y + i as f32 * line_height);
                    if i < wrapped_lines {
                        end.x = line_size.width;
                    }

                    line_corners.push(Corners {
                        top_left: line_origin + point(start.x, start.y),
                        top_right: line_origin + point(end.x, start.y),
                        bottom_left: line_origin + point(start.x, start.y + line_height),
                        bottom_right: line_origin + point(end.x, start.y + line_height),
                    });
                }
            }

            if line_cursor_start.is_some() && line_cursor_end.is_some() {
                break;
            }

            offset_y += line_size.height;
        }

        let mut points = vec![];
        if line_corners.is_empty() {
            return None;
        }

        // Fix corners to make sure the left to right direction
        for corners in &mut line_corners {
            if corners.top_left.x > corners.top_right.x {
                std::mem::swap(&mut corners.top_left, &mut corners.top_right);
                std::mem::swap(&mut corners.bottom_left, &mut corners.bottom_right);
            }
        }

        for corners in &line_corners {
            points.push(corners.top_right);
            points.push(corners.bottom_right);
            points.push(corners.bottom_left);
        }

        let mut rev_line_corners = line_corners.iter().rev().peekable();
        while let Some(corners) = rev_line_corners.next() {
            points.push(corners.top_left);
            if let Some(next) = rev_line_corners.peek() {
                if next.top_left.x != corners.top_left.x {
                    points.push(point(next.top_left.x, corners.top_left.y));
                }
            }
        }

        // print_points_as_svg_path(&line_corners, &points);

        let path_origin = bounds.origin + point(line_number_width, px(0.));
        let first_p = *points.get(0).unwrap();
        let mut builder = gpui::PathBuilder::fill();
        builder.move_to(path_origin + first_p);
        for p in points.iter().skip(1) {
            builder.line_to(path_origin + *p);
        }

        builder.build().ok()
    }

    fn layout_search_matches(
        &self,
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
        cx: &mut App,
    ) -> Vec<(Path<Pixels>, bool)> {
        let state = self.state.read(cx);
        if !state.search_session.open {
            return vec![];
        }
        let ranges = state.search_session.matcher.matched_ranges();
        let current_match_ix = state.search_session.matcher.current_match_index();

        let mut paths = Vec::with_capacity(ranges.as_ref().len());
        for (index, range) in ranges.as_ref().iter().enumerate() {
            if let Some(path) = Self::layout_match_range(range.clone(), last_layout, bounds) {
                paths.push((path, current_match_ix == index));
            }
        }

        paths
    }

    fn layout_hover_highlight(
        &self,
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
        cx: &mut App,
    ) -> Option<Path<Pixels>> {
        let state = self.state.read(cx);
        let Some(symbol_range) = state.extras.hover_symbol_range() else {
            return None;
        };

        Self::layout_match_range(symbol_range, last_layout, bounds)
    }

    fn layout_document_colors(
        &self,
        document_colors: &[(Range<usize>, Hsla)],
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
        _cx: &mut App,
    ) -> Vec<(Path<Pixels>, Hsla)> {
        let mut paths = vec![];
        for (range, color) in document_colors.iter() {
            if let Some(path) = Self::layout_match_range(range.clone(), last_layout, bounds) {
                paths.push((path, *color));
            }
        }

        paths
    }

    fn layout_selections(
        &self,
        last_layout: &LastLayout,
        bounds: &mut Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Path<Pixels>> {
        let state = self.state.read(cx);
        if !state.focus_handle.is_focused(window) {
            return None;
        }

        let mut selected_range = state.selected_range;
        if let Some(ime_marked_range) = &state.ime_marked_range {
            if !ime_marked_range.is_empty() {
                selected_range = (ime_marked_range.end..ime_marked_range.end).into();
            }
        }
        if selected_range.is_empty() {
            return None;
        }

        if state.masked {
            selected_range.start = masked_display_offset(&state.text, selected_range.start);
            selected_range.end = masked_display_offset(&state.text, selected_range.end);
        }

        let (start_ix, end_ix) = if selected_range.start < selected_range.end {
            (selected_range.start, selected_range.end)
        } else {
            (selected_range.end, selected_range.start)
        };

        let range = start_ix.max(last_layout.visible_range_offset.start)
            ..end_ix.min(last_layout.visible_range_offset.end);

        Self::layout_match_range(range, &last_layout, bounds)
    }

    /// Calculate the visible range of lines in the viewport.
    ///
    /// Returns
    ///
    /// - visible_range: The visible range is based on unwrapped lines (Zero based).
    /// - visible_buffer_lines: Indices of non-hidden buffer lines within the visible range.
    /// - visible_top: The top position of the first visible line in the scroll viewport.
    fn calculate_visible_range(
        &self,
        state: &InputBaseState<M>,
        line_height: Pixels,
        input_height: Pixels,
    ) -> (Range<usize>, Vec<usize>, Pixels) {
        // Add extra rows to avoid showing empty space when scroll to bottom.
        let extra_rows = 1;
        if state.is_single_line() {
            return (0..1, vec![0], px(0.));
        }

        let total_lines = state.display_map.wrap_row_count();
        let display_count = state.display_map.display_row_count();
        let buffer_line_count = state.display_map.buffer_line_count();
        if display_count == 0 || buffer_line_count == 0 {
            return (0..0, Vec::new(), px(0.));
        }

        let mut scroll_top = if let Some(deferred_scroll_offset) = state.deferred_scroll_offset {
            deferred_scroll_offset.y
        } else {
            state.scroll_handle.offset().y
        };
        scroll_top = clamp_auto_grow_vertical_scroll_offset(
            &state.mode,
            scroll_top,
            line_height * total_lines,
            input_height,
        );

        // Display rows are uniformly `line_height` tall, so the visible window maps
        // directly to a display-row range.
        let viewport_top = (-scroll_top).max(px(0.));
        let viewport_bottom = viewport_top + input_height;
        let line_height_f = f32::from(line_height);
        let first_display =
            ((f32::from(viewport_top) / line_height_f).floor() as usize).min(display_count - 1);
        let last_display =
            ((f32::from(viewport_bottom) / line_height_f).ceil() as usize).min(display_count - 1);

        let start_line = state.display_map.display_row_to_buffer_line(first_display);
        let end_line = state.display_map.display_row_to_buffer_line(last_display);

        // y of the top of the first visible buffer line (in content space).
        let visible_top = match state
            .display_map
            .buffer_line_to_display_row_range(start_line)
        {
            Some(range) => line_height * range.start,
            None => line_height * first_display,
        };

        let visible_range = start_line..(end_line + 1 + extra_rows).min(buffer_line_count);

        // Collect non-hidden buffer lines within the visible range
        let mut visible_buffer_lines = Vec::with_capacity(visible_range.len());
        for ix in visible_range.clone() {
            if state.display_map.visible_wrap_row_count_for_buffer_line(ix) > 0 {
                visible_buffer_lines.push(ix);
            }
        }

        (visible_range, visible_buffer_lines, visible_top)
    }

    /// Return (line_number_width, line_number_len)
    fn layout_line_numbers(
        state: &InputBaseState<M>,
        text: &Rope,
        font_size: Pixels,
        style: &TextStyle,
        window: &mut Window,
    ) -> (Pixels, usize) {
        let total_lines = text.lines_len();
        // One extra column beyond the widest line number, so right-aligned
        // numbers keep a gap from the left edge.
        let line_number_len = total_lines.max(1).ilog10() as usize + 2;

        let mut line_number_width = if state.mode.line_number() {
            let empty_line_number = window.text_system().shape_line(
                "+".repeat(line_number_len).into(),
                font_size,
                &[TextRun {
                    len: line_number_len,
                    font: style.font(),
                    color: gpui::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            );

            empty_line_number.width + LINE_NUMBER_RIGHT_MARGIN
        } else if state.is_code_editor() {
            LINE_NUMBER_RIGHT_MARGIN
        } else {
            px(0.)
        };

        if state.mode.is_folding() {
            // Add extra space for fold icons
            line_number_width += FOLD_ICON_HITBOX_WIDTH
        }

        (line_number_width, line_number_len)
    }

    /// Layout shaped lines for whitespace indicators (space and tab).
    ///
    /// Returns `WhitespaceIndicators` with shaped lines for space and tab characters.
    fn layout_whitespace_indicators(
        state: &InputBaseState<M>,
        text_size: Pixels,
        style: &TextStyle,
        window: &mut Window,
        _cx: &App,
    ) -> Option<WhitespaceIndicators> {
        if !state.show_whitespaces {
            return None;
        }

        let invisible_color = state
            .editor_style
            .editor_invisible
            .unwrap_or(state.editor_style.muted_foreground);

        let space_font_size = text_size.half();
        let tab_font_size = text_size;

        let space_text = SharedString::new_static("•");
        let space = window.text_system().shape_line(
            space_text.clone(),
            space_font_size,
            &[TextRun {
                len: space_text.len(),
                font: style.font(),
                color: invisible_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );

        let tab_text = SharedString::new_static("→");
        let tab = window.text_system().shape_line(
            tab_text.clone(),
            tab_font_size,
            &[TextRun {
                len: tab_text.len(),
                font: style.font(),
                color: invisible_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );

        Some(WhitespaceIndicators { space, tab })
    }

    /// Compute inline completion ghost lines for rendering.
    ///
    /// Returns (first_line, ghost_lines) where:
    /// - first_line: Shaped text for the first line (goes after cursor on same line)
    /// - ghost_lines: Shaped lines for subsequent lines (shift content down)
    fn layout_inline_completion(
        state: &InputBaseState<M>,
        visible_range: &Range<usize>,
        font_size: Pixels,
        window: &mut Window,
        _cx: &App,
    ) -> (Option<ShapedLine>, Vec<ShapedLine>) {
        // Must be focused to show inline completion
        if !state.focus_handle.is_focused(window) {
            return (None, vec![]);
        }

        let Some(completion_item) = state.extras.inline_completion_item() else {
            return (None, vec![]);
        };

        // Get cursor row from cursor position
        let cursor_row = state.cursor_position().line as usize;

        // Only show if cursor row is visible
        if cursor_row < visible_range.start || cursor_row >= visible_range.end {
            return (None, vec![]);
        }

        let completion_text = &completion_item.insert_text;
        let completion_color = state.editor_style.muted_foreground.opacity(0.5);

        let text_style = window.text_style();
        let font = text_style.font();

        let lines: Vec<&str> = completion_text.split('\n').collect();
        if lines.is_empty() {
            return (None, vec![]);
        }

        // Shape first line (goes after cursor)
        let first_text: SharedString = lines[0].to_string().into();
        let first_line = if !first_text.is_empty() {
            let first_run = TextRun {
                len: first_text.len(),
                font: font.clone(),
                color: completion_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            Some(
                window
                    .text_system()
                    .shape_line(first_text, font_size, &[first_run], None),
            )
        } else {
            None
        };

        // Shape ghost lines (lines 2+ that shift content down)
        let ghost_lines: Vec<ShapedLine> = lines[1..]
            .iter()
            .map(|line_text| {
                let text: SharedString = line_text.to_string().into();
                let len = text.len().max(1); // Ensure at least 1 for empty lines
                let run = TextRun {
                    len,
                    font: font.clone(),
                    color: completion_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                // Use space for empty lines so they take up height
                let shaped_text = if text.is_empty() { " ".into() } else { text };
                window
                    .text_system()
                    .shape_line(shaped_text, font_size, &[run], None)
            })
            .collect();

        (first_line, ghost_lines)
    }

    /// Return (line_number_width, line_number_len)
    /// Layout fold icon hitboxes during prepaint phase.
    ///
    /// This creates hitboxes for the fold icon area, positioned to the right of line numbers.
    /// Icons are created and prepainted here to avoid panics.
    fn layout_fold_icons(
        &self,
        origin_x: Pixels,
        bounds: &Bounds<Pixels>,
        last_layout: &LastLayout,
        window: &mut Window,
        cx: &mut App,
    ) -> FoldIconLayout {
        // First pass: collect fold information from state
        struct FoldInfo {
            buffer_line: usize,
            is_folded: bool,
            display_row: usize,
            offset_y: Pixels,
        }

        let line_number_hitbox = window.insert_hitbox(
            Bounds::new(
                point(origin_x, bounds.origin.y + last_layout.visible_top),
                size(last_layout.line_number_width, bounds.size.height),
            ),
            HitboxBehavior::Normal,
        );

        let mut icon_layout = FoldIconLayout {
            line_number_hitbox,
            icons: vec![],
        };

        let fold_infos: Vec<FoldInfo> = {
            let state = self.state.read(cx);
            if !state.mode.is_folding() {
                return icon_layout;
            }

            let mut infos = Vec::with_capacity(last_layout.visible_buffer_lines.len());
            let mut offset_y = last_layout.visible_top;

            for (line, &buffer_line) in last_layout
                .lines
                .iter()
                .zip(last_layout.visible_buffer_lines.iter())
            {
                if state.display_map.is_fold_candidate(buffer_line) {
                    let is_folded = state.display_map.is_folded_at(buffer_line);
                    infos.push(FoldInfo {
                        buffer_line,
                        is_folded,
                        display_row: buffer_line,
                        offset_y,
                    });
                }

                offset_y += line.wrapped_lines.len() * last_layout.line_height;
            }

            infos
        }; // state is dropped here

        // Second pass: create and prepaint icons
        let line_height = last_layout.line_height;
        let line_number_width =
            last_layout.line_number_width - LINE_NUMBER_RIGHT_MARGIN - FOLD_ICON_HITBOX_WIDTH;
        let icon_relative_pos = point(
            (FOLD_ICON_HITBOX_WIDTH - FOLD_ICON_WIDTH).half(),
            (line_height - FOLD_ICON_WIDTH).half(),
        );

        for (ix, info) in fold_infos.iter().enumerate() {
            // Position fold icon to the right of line numbers.
            // Use origin_x (unscrolled) so icons stay fixed in the gutter during horizontal scroll.
            let fold_icon_bounds = Bounds::new(
                point(
                    origin_x + icon_relative_pos.x + line_number_width,
                    bounds.origin.y + icon_relative_pos.y + info.offset_y,
                ),
                size(FOLD_ICON_HITBOX_WIDTH, line_height),
            );

            // Create and prepaint icon
            let child = self
                .state
                .read(cx)
                .editor_style
                .fold_icon_renderer
                .as_ref()
                .map(|render| render(ix, info.is_folded))
                .unwrap_or_else(|| {
                    let (path, svg) = if info.is_folded {
                        ("input-fold-chevron-right", FOLD_CHEVRON_RIGHT_SVG)
                    } else {
                        ("input-fold-chevron-down", FOLD_CHEVRON_DOWN_SVG)
                    };
                    gpui::canvas(
                        |_, _, _| {},
                        move |bounds, _, window, cx| {
                            let color = window.text_style().color;
                            let _ = window.paint_svg(
                                bounds,
                                path.into(),
                                Some(svg),
                                gpui::TransformationMatrix::default(),
                                color,
                                cx,
                            );
                        },
                    )
                    .size(FOLD_ICON_WIDTH)
                    .into_any_element()
                });
            let mut icon = gpui::div()
                .id(("fold", ix))
                .size(FOLD_ICON_WIDTH)
                .child(child)
                .on_mouse_down(MouseButton::Left, {
                    let state = self.state.clone();
                    let buffer_line = info.buffer_line;
                    move |_, _: &mut Window, cx: &mut App| {
                        cx.stop_propagation();

                        state.update(cx, |state, cx| {
                            state.display_map.toggle_fold(buffer_line);
                            cx.notify();
                        });
                    }
                })
                .into_any_element();

            icon.prepaint_as_root(
                fold_icon_bounds.origin,
                fold_icon_bounds.size.into(),
                window,
                cx,
            );

            icon_layout
                .icons
                .push((info.display_row, info.is_folded, icon));
        }

        icon_layout
    }

    /// Paint fold icons using prepaint hitboxes.
    ///
    /// This handles:
    /// - Rendering fold icons (chevron-right for folded, chevron-down for expanded)
    /// - Mouse click handling to toggle fold state
    /// - Cursor style changes on hover
    /// - Only show icon on hover or for current line
    fn paint_fold_icons(
        &mut self,
        fold_icon_layout: &mut FoldIconLayout,
        current_row: Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let is_hovered = fold_icon_layout.line_number_hitbox.is_hovered(window);
        for (display_row, is_folded, icon) in fold_icon_layout.icons.iter_mut() {
            let is_current_line = current_row == Some(*display_row);

            if !is_hovered && !is_current_line && !*is_folded {
                continue;
            }

            icon.paint(window, cx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_lines(
        state: &InputBaseState<M>,
        display_text: &Rope,
        last_layout: &LastLayout,
        font_size: Pixels,
        runs: &[TextRun],
        bg_segments: &[(Range<usize>, Hsla)],
        whitespace_indicators: Option<WhitespaceIndicators>,
        window: &mut Window,
    ) -> Vec<LineLayout> {
        let is_single_line = state.is_single_line();

        if is_single_line {
            let text: SharedString = display_text.to_string().into();
            let aligned_runs = align_runs_to_char_boundaries(&text, runs);
            let line_runs = aligned_runs.as_deref().unwrap_or(runs);
            let shaped_line = window
                .text_system()
                .shape_line(text, font_size, line_runs, None);

            let line_layout = LineLayout::new()
                .lines(smallvec::smallvec![shaped_line])
                .with_background(has_background(line_runs))
                .with_whitespaces(whitespace_indicators);
            return vec![line_layout];
        }

        // Empty to use placeholder, the placeholder is not in the wrapper map.
        if state.text.len() == 0 {
            let placeholder_text = display_text.to_string();
            let mut placeholder_lines = SmallVec::new();
            let mut line_has_background = false;

            for (line, line_runs) in placeholder_line_runs(&placeholder_text, runs) {
                let shaped_line = window.text_system().shape_line(
                    line.to_string().into(),
                    font_size,
                    &line_runs,
                    None,
                );
                line_has_background |= has_background(&line_runs);
                placeholder_lines.push(shaped_line);
            }

            // Keep placeholder lines in a single layout to stay parallel with visible_* metadata.
            let line_layout = LineLayout::new()
                .lines(placeholder_lines)
                .with_background(line_has_background)
                .with_whitespaces(whitespace_indicators);
            return vec![line_layout];
        }

        let mut lines = Vec::with_capacity(last_layout.visible_buffer_lines.len());
        // run_offset tracks position in the runs vec coordinate space (only visible line bytes).
        // This is separate from the visible_text offset because runs from highlight_lines
        // only cover visible (non-folded) lines.
        let mut run_offset = 0;

        for (vi, &buffer_line) in last_layout.visible_buffer_lines.iter().enumerate() {
            let line_text: String = display_text.slice_line(buffer_line).into();
            let line_item = state
                .display_map
                .line(buffer_line)
                .expect("line should exists in wrapper");

            debug_assert_eq!(line_item.len(), line_text.len());

            let mut wrapped_lines: SmallVec<[ShapedLine; 1]> = SmallVec::with_capacity(1);
            let mut line_has_background = false;

            for range in &line_item.wrapped_lines {
                let line_runs = runs_for_range(runs, run_offset, &range);
                let line_runs = if bg_segments.is_empty() {
                    line_runs
                } else {
                    split_runs_by_bg_segments(
                        last_layout.visible_line_byte_offsets[vi] + (range.start),
                        &line_runs,
                        bg_segments,
                    )
                };

                let sub_line: SharedString = line_text[range.clone()].to_string().into();
                let line_runs =
                    align_runs_to_char_boundaries(&sub_line, &line_runs).unwrap_or(line_runs);
                let shaped_line = window
                    .text_system()
                    .shape_line(sub_line, font_size, &line_runs, None);

                line_has_background |= has_background(&line_runs);
                wrapped_lines.push(shaped_line);
            }

            // Use the first visual line's indentation width for continuation lines.
            let wrap_indent = if line_item.indent > 0 && wrapped_lines.len() > 1 {
                let indent_byte_len = line_text
                    .char_indices()
                    .nth(line_item.indent as usize)
                    .map(|(ix, _)| ix)
                    .unwrap_or(line_text.len());
                wrapped_lines[0].x_for_index(indent_byte_len)
            } else {
                px(0.)
            };

            let line_layout = LineLayout::new()
                .lines(wrapped_lines)
                .wrap_indent(wrap_indent)
                .with_background(line_has_background)
                .with_whitespaces(whitespace_indicators.clone());
            lines.push(line_layout);

            // +1 for the `\n`
            run_offset += line_text.len() + 1;
        }

        lines
    }

    /// First usize is the offset of skipped. The decorations' visible bottom
    /// rules go in `rules` ([`compose_decoration_collections`]) and the row
    /// backgrounds reaching the visible lines in `rows`, none while the input
    /// is masked.
    fn highlight_lines(
        &mut self,
        visible_buffer_lines: &[usize],
        _visible_top: Pixels,
        visible_byte_range: Range<usize>,
        rules: &mut Vec<(Range<usize>, Hsla)>,
        rows: &mut RowBackgrounds,
        cx: &mut App,
    ) -> Option<Vec<(Range<usize>, HighlightStyle)>> {
        let state = self.state.read(cx);
        let text = &state.text;
        let is_multi_line = state.is_multi_line();

        let (mut highlighter, diagnostics) = match &state.mode {
            LayoutMode::CodeEditor {
                highlighter,
                diagnostics,
                ..
            } => (highlighter.borrow_mut(), diagnostics),
            _ => {
                return (!state.masked)
                    .then(|| {
                        compose_decoration_layers(
                            Vec::new(),
                            state.extras.decoration_layers(),
                            state.extras.has_bottom_rules(),
                            visible_byte_range,
                            rules,
                            rows,
                        )
                    })
                    .flatten();
            }
        };
        let Some(highlighter) = highlighter.as_mut() else {
            return (!state.masked)
                .then(|| {
                    compose_decoration_layers(
                        Vec::new(),
                        state.extras.decoration_layers(),
                        state.extras.has_bottom_rules(),
                        visible_byte_range,
                        rules,
                        rows,
                    )
                })
                .flatten();
        };

        let mut styles = Vec::with_capacity(visible_buffer_lines.len());
        // Byte ranges of the flushed line groups; the composed styles are
        // clipped to these at the end so the resulting runs cover exactly the
        // visible (non-folded) lines' bytes.
        let mut group_ranges: Vec<Range<usize>> = Vec::new();

        // Helper to flush a contiguous range of lines. These ranges are disjoint,
        // so appending avoids repeatedly cloning and recombining prior styles.
        let mut flush_range =
            |start_line: usize, end_line: usize, skip: bool, styles: &mut Vec<_>| {
                let byte_start = text.line_start_offset(start_line);
                let byte_end = if is_multi_line {
                    // +1 for `\n`
                    text.line_start_offset(end_line + 1)
                } else {
                    text.line_end_offset(end_line)
                };
                let range_styles = if skip {
                    vec![(byte_start..byte_end, HighlightStyle::default())]
                } else {
                    highlighter.styles(
                        &(byte_start..byte_end),
                        state.editor_style.highlight_styles.as_ref(),
                    )
                };

                group_ranges.push(byte_start..byte_end);
                styles.extend(range_styles);
            };

        // Group contiguous visible lines into ranges and call styles() once per range
        let mut visible_iter = visible_buffer_lines.iter().peekable();
        let mut range_start: Option<usize> = None;

        while let Some(&line) = visible_iter.next() {
            // Check if this line is too long for highlighting
            let line_len = text.slice_line(line).len();
            if line_len > MAX_HIGHLIGHT_LINE_LENGTH {
                // Flush any accumulated range first
                if let Some(start) = range_start.take() {
                    flush_range(start, line - 1, false, &mut styles);
                }

                flush_range(line, line, true, &mut styles);
                continue;
            }

            range_start.get_or_insert(line);

            // Check if next line is contiguous, if so keep accumulating
            if visible_iter
                .peek()
                .map(|&&next| next == line + 1)
                .unwrap_or(false)
            {
                continue;
            }

            // Flush the contiguous range
            let start_line = range_start.take().unwrap();
            flush_range(start_line, line, false, &mut styles);
        }

        let diagnostic_styles: Vec<_> = diagnostics
            .range(visible_byte_range.clone())
            .map(|entry| {
                (
                    entry.range.clone(),
                    diagnostic_highlight_style(entry.severity, state.editor_style.diagnostics),
                )
            })
            .collect();

        // Range semantic tokens, resolved from the LSP provider's cached
        // result through the active highlight theme so it shares the same
        // colour vocabulary as the tree-sitter path. Empty Vec when no
        // provider is set, so `combine_highlights` short-circuits.
        let custom_styles = state.extras.semantic_token_styles(
            text,
            &visible_byte_range,
            state.editor_style.highlight_styles.as_ref(),
        );

        // hover definition style
        if let Some(hover_style) = M::hover_definition_style(self.state.read(cx), cx) {
            styles.push(hover_style);
        }

        // Compose tree-sitter, semantic, application, then diagnostic styles.
        styles = gpui::combine_highlights(custom_styles, styles).collect();
        if !state.masked {
            styles = compose_decoration_layers(
                styles,
                state.extras.decoration_layers(),
                state.extras.has_bottom_rules(),
                visible_byte_range.clone(),
                rules,
                rows,
            )
            .unwrap_or_default();
        }
        styles = gpui::combine_highlights(diagnostic_styles, styles).collect();

        // Some sources reach outside the flushed groups — a diagnostic
        // straddling the viewport edge, or any range inside a folded region —
        // which would inject extra bytes into the runs coordinate space and
        // shift every later run boundary (see `layout_lines`). Clip the
        // composed styles back to the groups.
        styles = clip_styles_to_ranges(styles, &group_ranges);

        Some(styles)
    }
}

pub(super) struct PrepaintState {
    /// The lines of entire lines.
    last_layout: LastLayout,
    /// The lines only contains the visible lines in the viewport, based on `visible_range`.
    ///
    /// The child is the soft lines.
    line_numbers: Option<Vec<SmallVec<[ShapedLine; 1]>>>,
    /// Size of the scrollable area by entire lines.
    scroll_size: Size<Pixels>,
    cursor_bounds: Option<Bounds<Pixels>>,
    cursor_scroll_offset: Point<Pixels>,
    /// row index (zero based), no wrap, same line as the cursor.
    current_row: Option<usize>,
    selection_path: Option<Path<Pixels>>,
    hover_highlight_path: Option<Path<Pixels>>,
    search_match_paths: Vec<(Path<Pixels>, bool)>,
    document_color_paths: Vec<(Path<Pixels>, Hsla)>,
    hover_definition_hitbox: Option<Hitbox>,
    indent_guides_path: Option<Path<Pixels>>,
    bounds: Bounds<Pixels>,
    /// Fold icon layout data
    fold_icon_layout: FoldIconLayout,
    // Inline completion rendering data
    /// Shaped ghost lines to paint after cursor row (completion lines 2+)
    ghost_lines: Vec<ShapedLine>,
    /// First line of inline completion (painted after cursor on same line)
    ghost_first_line: Option<ShapedLine>,
    ghost_lines_height: Pixels,
    /// The decorations' visible bottom rules, absolute byte ranges ascending
    /// ([`compose_decoration_collections`]), painted with the backgrounds.
    bottom_rules: Vec<(Range<usize>, Hsla)>,
    /// The row backgrounds reaching the visible lines, ascending by start
    /// ([`read_row_background`]), painted first in the background pass.
    row_backgrounds: RowBackgrounds,
}

impl PrepaintState {
    /// Returns cursor bounds adjusted for scroll offset, if available.
    fn cursor_bounds_with_scroll(&self) -> Option<Bounds<Pixels>> {
        self.cursor_bounds.map(|mut bounds| {
            bounds.origin.y += self.cursor_scroll_offset.y;
            bounds
        })
    }
}

impl<M: InputModeKind> IntoElement for TextElement<M> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// A debug function to print points as SVG path.
#[allow(unused)]
fn print_points_as_svg_path(
    line_corners: &Vec<gpui::Corners<Pixels>>,
    points: &Vec<Point<Pixels>>,
) {
    for corners in line_corners {
        println!(
            "tl: ({}, {}), tr: ({}, {}), bl: ({}, {}), br: ({}, {})",
            corners.top_left.as_f32() as i32,
            corners.top_left.as_f32() as i32,
            corners.top_right.as_f32() as i32,
            corners.top_right.as_f32() as i32,
            corners.bottom_left.as_f32() as i32,
            corners.bottom_left.as_f32() as i32,
            corners.bottom_right.as_f32() as i32,
            corners.bottom_right.as_f32() as i32,
        );
    }

    if points.len() > 0 {
        println!(
            "M{},{}",
            points[0].x.as_f32() as i32,
            points[0].y.as_f32() as i32
        );
        for p in points.iter().skip(1) {
            println!("L{},{}", p.x.as_f32() as i32, p.y.as_f32() as i32);
        }
    }
}
impl<M: InputModeKind> Element for TextElement<M> {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let line_height = window.line_height();
        if self.state.read(cx).deferred_scroll_row.is_some() {
            self.state
                .update(cx, |state, _| state.resolve_scroll_line_height(line_height));
        }
        let state = self.state.read(cx);

        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if state.is_multi_line() {
            style.flex_grow = 1.0;
            style.size.height = relative(1.).into();
            if state.mode.is_auto_grow() {
                // Auto grow to let height match to rows, but not exceed max rows.
                let rows = state.mode.max_rows().min(state.mode.rows());
                style.min_size.height = (rows * line_height).into();
            } else {
                style.min_size.height = line_height.into();
            }
        } else {
            // For single-line inputs, the minimum height should be the line height
            style.size.height = line_height.into();
        };

        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let style = window.text_style();
        let font = style.font();
        let text_size = style.font_size.to_pixels(window.rem_size());

        self.state.update(cx, |state, cx| {
            state.display_map.set_font(font, text_size, cx);
            state.display_map.ensure_text_prepared(&state.text, cx);
        });

        // GitTurtle patch: a placeholder fitted to the width the text is laid
        // out at in this frame, kept as the field's own
        // (`InputBaseState::set_placeholder_fit`).
        let placeholder = match self.state.read(cx).placeholder_fit.clone() {
            Some(fit) => {
                let fitted = (fit.fit)(bounds.size.width, window);
                self.state.update(cx, |state, _| {
                    if state.placeholder != fitted {
                        state.placeholder = fitted.clone();
                    }
                });
                fitted
            }
            None => self.placeholder.clone(),
        };

        let state = self.state.read(cx);
        let multi_line = state.is_multi_line();
        let text = state.text.clone();
        let is_empty = text.len() == 0;

        let text_style = window.text_style();
        let disabled = state.disabled;
        let dim = |color: Hsla| if disabled { color.opacity(0.5) } else { color };
        let fg = dim(text_style.color);
        let (display_text, text_color) = if is_empty {
            (
                &Rope::from(placeholder.as_str()),
                dim(state.editor_style.muted_foreground),
            )
        } else if state.masked {
            (
                &Rope::from(MASK_CHAR.to_string().repeat(text.chars().count())),
                fg,
            )
        } else {
            (&text, fg)
        };

        // Calculate the width of the line numbers
        let (line_number_width, line_number_len) =
            Self::layout_line_numbers(&state, &text, text_size, &text_style, window);

        let mut bounds = bounds;
        let wrap_width = if multi_line && state.soft_wrap {
            Some(bounds.size.width - line_number_width - RIGHT_MARGIN)
        } else {
            None
        };

        let wrapping_indent = state.wrapping_indent;
        let wrap_width_changed = state
            .last_layout
            .as_ref()
            .map(|l| l.wrap_width != wrap_width)
            .unwrap_or(true);

        let wrapping_indent_changed = state
            .last_layout
            .as_ref()
            .map(|l| l.wrapping_indent != wrapping_indent)
            .unwrap_or(true);

        if wrap_width_changed || wrapping_indent_changed {
            self.state.update(cx, |state, cx| {
                state.display_map.on_layout_changed(wrap_width, cx);
                state.display_map.set_wrapping_indent(wrapping_indent, cx);
            });
        }

        let state = self.state.read(cx);
        let line_height = window.line_height();

        let (visible_range, visible_buffer_lines, visible_top) =
            self.calculate_visible_range(&state, line_height, bounds.size.height);
        let visible_start_offset = state.text.line_start_offset(visible_range.start);
        let visible_end_offset = state
            .text
            .line_end_offset(visible_range.end.saturating_sub(1));

        let mut bottom_rules = Vec::new();
        let mut row_backgrounds = RowBackgrounds::new();
        let highlight_styles = self.highlight_lines(
            &visible_buffer_lines,
            visible_top,
            visible_start_offset..visible_end_offset,
            &mut bottom_rules,
            &mut row_backgrounds,
            cx,
        );

        let state = self.state.read(cx);

        let visible_line_byte_offsets: Vec<usize> = visible_buffer_lines
            .iter()
            .map(|&bl| state.text.line_start_offset(bl))
            .collect();

        // For password input (masked: true), convert byte offsets to masked display byte offsets so that
        // layout_match_range and position_for_index work in the correct coordinate space.
        let (visible_line_byte_offsets, visible_range_offset) = if state.masked {
            let offsets = visible_line_byte_offsets
                .iter()
                .map(|&o| masked_display_offset(&text, o))
                .collect();
            let range_offset = masked_display_offset(&text, visible_start_offset)
                ..masked_display_offset(&text, visible_end_offset);
            (offsets, range_offset)
        } else {
            (
                visible_line_byte_offsets,
                visible_start_offset..visible_end_offset,
            )
        };

        let mut last_layout = LastLayout {
            visible_range,
            visible_buffer_lines,
            visible_line_byte_offsets,
            visible_top,
            visible_range_offset,
            line_height,
            wrap_width,
            wrapping_indent,
            line_number_width,
            lines: Rc::new(vec![]),
            cursor_bounds: None,
            text_align: state.text_align,
            content_width: bounds.size.width,
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let marked_run = TextRun {
            len: 0,
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(text_color),
                wavy: false,
            }),
            strikethrough: None,
        };

        let ime_marked_range = ime_marked_display_range(
            &text,
            state.ime_marked_range.as_ref().map(|m| m.start..m.end),
            state.masked,
        );

        let runs = if let (false, Some(highlight_styles)) = (is_empty, highlight_styles) {
            let mut runs = Vec::with_capacity(highlight_styles.len() + 2);

            for (range, style) in &highlight_styles {
                let mut run = text_style.clone().highlight(*style).to_run(range.len());
                if disabled {
                    run.color = run.color.opacity(0.5);
                }

                runs.extend(split_run_for_ime_underline(
                    run,
                    range.clone(),
                    ime_marked_range.clone(),
                    marked_run.underline,
                ));
            }
            runs
        } else {
            split_run_for_ime_underline(
                run,
                0..display_text.len(),
                ime_marked_range,
                marked_run.underline,
            )
            .into_vec()
        };

        let document_colors = state
            .extras
            .document_color_swatches(&text, &last_layout.visible_range);

        // Create shaped lines for whitespace indicators before layout
        let whitespace_indicators =
            Self::layout_whitespace_indicators(&state, text_size, &text_style, window, cx);

        let lines = Self::layout_lines(
            &state,
            &display_text,
            &last_layout,
            text_size,
            &runs,
            &document_colors,
            whitespace_indicators,
            window,
        );

        let mut longest_line_width = wrap_width.unwrap_or(px(0.));
        // 1. Single line
        // 2. Multi-line with soft wrap disabled.
        if state.is_single_line() || !state.soft_wrap {
            let longest_row = state.display_map.longest_row();
            let longest_line: SharedString = state.text.slice_line(longest_row).to_string().into();
            longest_line_width = window
                .text_system()
                .shape_line(
                    longest_line.clone(),
                    text_size,
                    &[TextRun {
                        len: longest_line.len(),
                        font: style.font(),
                        color: gpui::black(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    wrap_width,
                )
                .width;
        }
        last_layout.lines = Rc::new(lines);

        let (ghost_first_line, ghost_lines) = Self::layout_inline_completion(
            state,
            &last_layout.visible_range,
            text_size,
            window,
            cx,
        );
        let ghost_line_count = ghost_lines.len();
        let ghost_lines_height = ghost_line_count as f32 * line_height;

        let total_wrapped_lines = state.display_map.wrap_row_count();
        let empty_bottom_height = empty_bottom_height(
            state.is_code_editor(),
            state.scroll_beyond_last_line,
            bounds.size.height,
            line_height,
        );

        // Empty bottom and ghost lines both describe extra height past the
        // last content row, so take the max rather than summing — summing
        // left a band of empty space the cursor could never reach.
        let mut scroll_size = size(
            if longest_line_width + line_number_width + RIGHT_MARGIN > bounds.size.width {
                longest_line_width + line_number_width + RIGHT_MARGIN
            } else {
                longest_line_width
            },
            (total_wrapped_lines as f32 * line_height
                + empty_bottom_height.max(ghost_lines_height))
            .max(bounds.size.height),
        );

        // TODO: should be add some gap to right, to convenient to focus on boundary position
        if last_layout.text_align == TextAlign::Right || last_layout.text_align == TextAlign::Center
        {
            scroll_size.width = longest_line_width + line_number_width;
        }

        // `position_for_index` for example
        //
        // #### text
        //
        // Hello 世界，this is GPUI component.
        // The GPUI Component is a collection of UI components for
        // GPUI framework, including Button, Input, Checkbox, Radio,
        // Dropdown, Tab, and more...
        //
        // wrap_width: 444px, line_height: 20px
        //
        // #### lines[0]
        //
        // | index | pos              | line |
        // |-------|------------------|------|
        // | 5     | (37 px, 0.0)     | 0    |
        // | 38    | (261.7 px, 20.0) | 0    |
        // | 40    | None             | -    |
        //
        // #### lines[1]
        //
        // | index | position              | line |
        // |-------|-----------------------|------|
        // | 5     | (43.578125 px, 0.0)   | 0    |
        // | 56    | (422.21094 px, 0.0)   | 0    |
        // | 57    | (11.6328125 px, 20.0) | 1    |
        // | 114   | (429.85938 px, 20.0)  | 1    |
        // | 115   | (11.3125 px, 40.0)    | 2    |

        // Calculate the scroll offset to keep the cursor in view

        // Save the unscrolled x before layout_cursor modifies bounds.origin with scroll_offset.
        // Fold icons and their hitboxes must use this value so they stay fixed in the gutter
        // regardless of horizontal scroll position.
        let input_bounds = bounds;
        let original_x = bounds.origin.x;

        let (cursor_bounds, cursor_scroll_offset, current_row) =
            self.layout_cursor(&last_layout, &mut bounds, scroll_size, window, cx);
        last_layout.cursor_bounds = cursor_bounds;

        let search_match_paths = self.layout_search_matches(&last_layout, &mut bounds, cx);
        let selection_path = self.layout_selections(&last_layout, &mut bounds, window, cx);
        let hover_highlight_path = self.layout_hover_highlight(&last_layout, &mut bounds, cx);
        let document_color_paths =
            self.layout_document_colors(&document_colors, &last_layout, &bounds, cx);

        let state = self.state.read(cx);
        let line_numbers = if state.mode.line_number() {
            let mut line_numbers = Vec::with_capacity(last_layout.visible_buffer_lines.len());
            let other_line_runs = vec![TextRun {
                len: line_number_len,
                font: style.font(),
                color: state.editor_style.muted_foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            let current_line_runs = vec![TextRun {
                len: line_number_len,
                font: style.font(),
                color: state.editor_style.foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];

            // build line numbers
            for (line, &buffer_line) in last_layout
                .lines
                .iter()
                .zip(last_layout.visible_buffer_lines.iter())
            {
                let line_no: SharedString =
                    format!("{:>width$}", buffer_line + 1, width = line_number_len).into();

                let runs = if current_row == Some(buffer_line) {
                    &current_line_runs
                } else {
                    &other_line_runs
                };

                let mut sub_lines: SmallVec<[ShapedLine; 1]> = SmallVec::new();
                sub_lines.push(
                    window
                        .text_system()
                        .shape_line(line_no, text_size, &runs, None),
                );
                for _ in 0..line.wrapped_lines.len().saturating_sub(1) {
                    sub_lines.push(ShapedLine::default());
                }
                line_numbers.push(sub_lines);
            }
            Some(line_numbers)
        } else {
            None
        };

        let hover_definition_hitbox = M::hover_definition_hitbox(state, window, cx);
        let indent_guides_path =
            self.layout_indent_guides(state, &bounds, &last_layout, &text_style, window);
        state
            .editor_scrollbar_snapshot
            .set(Some(EditorScrollbarSnapshot::new(
                input_bounds,
                &last_layout,
                scroll_size,
                cursor_scroll_offset,
                state,
            )));
        let fold_icon_layout =
            self.layout_fold_icons(original_x, &bounds, &last_layout, window, cx);

        PrepaintState {
            bounds,
            last_layout,
            scroll_size,
            line_numbers,
            cursor_bounds,
            cursor_scroll_offset,
            current_row,
            selection_path,
            search_match_paths,
            hover_highlight_path,
            hover_definition_hitbox,
            document_color_paths,
            indent_guides_path,
            fold_icon_layout,
            ghost_first_line,
            ghost_lines,
            ghost_lines_height,
            bottom_rules,
            row_backgrounds,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        input_bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, show_cursor, disabled, selected_range, editor_style, editor_paddings) = {
            let state = self.state.read(cx);
            (
                state.focus_handle.clone(),
                state.show_cursor(window, cx),
                state.disabled,
                state.selected_range,
                state.editor_style.clone(),
                state.editor_paddings,
            )
        };
        let focused = focus_handle.is_focused(window);
        let bounds = prepaint.bounds;
        let text_align = prepaint.last_layout.text_align;

        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.state.clone()),
            cx,
        );

        // Paint multi line text
        let line_height = window.line_height();
        let origin = bounds.origin;

        let invisible_top_padding = prepaint.last_layout.visible_top;
        let active_line_color = editor_style
            .editor_active_line
            .map(|color| if disabled { color.opacity(0.5) } else { color });
        let editor_background = if disabled {
            editor_style.background.opacity(0.5)
        } else {
            editor_style.background
        };

        // Paint active line
        let mut offset_y = px(0.);
        if let Some(line_numbers) = prepaint.line_numbers.as_ref() {
            offset_y += invisible_top_padding;

            // Each item is the normal lines.
            for (lines, &buffer_line) in line_numbers
                .iter()
                .zip(prepaint.last_layout.visible_buffer_lines.iter())
            {
                let is_active = prepaint.current_row == Some(buffer_line);
                let p = point(input_bounds.origin.x, origin.y + offset_y);
                let height = line_height * lines.len() as f32;
                // Paint the current line background
                if is_active {
                    if let Some(bg_color) = active_line_color {
                        window.paint_quad(fill(
                            Bounds::new(p, size(bounds.size.width, height)),
                            bg_color,
                        ));
                    }
                }
                offset_y += height;
            }
        }

        // Keep scrollbar offset always be positive，Start from the left position
        let scroll_offset = if text_align == TextAlign::Right {
            (prepaint.scroll_size.width - prepaint.bounds.size.width).max(px(0.))
        } else if text_align == TextAlign::Center {
            (prepaint.scroll_size.width - prepaint.bounds.size.width)
                .half()
                .max(px(0.))
        } else {
            px(0.)
        };

        // Paint glyph backgrounds
        //
        // gpui's `paint` only draws glyphs, underlines, and strikethroughs, so decoration
        // backgrounds need their own pass. It runs ahead of the indent guides, selections,
        // and text so a highlighted range sits under them instead of covering them. The
        // decorations' bottom rules are painted in it, over their line's backgrounds, and
        // their row backgrounds first, under them.
        let mut offset_y = invisible_top_padding;
        // The first rule that may reach the line: the lines ascend, and the
        // rules ascend by start. A frame without rules paints as upstream.
        let rules = &prepaint.bottom_rules;
        let mut rules_from = 0;
        // The first row background that may reach the line, likewise; a
        // frame without one paints as upstream.
        let row_backgrounds = &prepaint.row_backgrounds;
        let mut rows_from = 0;
        // The visible lines before this index lie in the run of filled lines
        // already painted, whose first line began it.
        let mut run_until = 0;
        let layout = &prepaint.last_layout;
        // A visible line's fill, the first row background that may reach it
        // found from `from` on: its colour, and its right edge, the visible
        // right edge or the line's end where that is further.
        let fill_at = |index: usize, from: &mut usize, x: Pixels| {
            let (line, line_start) = (&layout.lines[index], layout.visible_line_byte_offsets[index]);
            while row_backgrounds
                .get(*from)
                .is_some_and(|(range, ..)| range.end <= line_start)
            {
                *from += 1;
            }
            let line_end = line_start
                + line
                    .wrapped_lines
                    .iter()
                    .map(|line| line.len)
                    .sum::<usize>();
            row_background_at(&row_backgrounds[*from..], line_start, line_end).map(|color| {
                (
                    color,
                    input_bounds
                        .right()
                        .max(x + line.size(line_height).width),
                )
            })
        };
        // Ghost lines after the cursor's row open a gap no fill crosses.
        let gap_after = |buffer_line: usize| {
            Some(buffer_line) == prepaint.current_row && prepaint.ghost_lines_height > px(0.)
        };
        for (index, ((line, &buffer_line), &line_start)) in layout
            .lines
            .iter()
            .zip(layout.visible_buffer_lines.iter())
            .zip(layout.visible_line_byte_offsets.iter())
            .enumerate()
        {
            let p = point(
                origin.x + layout.line_number_width + scroll_offset,
                origin.y + offset_y,
            );

            if !row_backgrounds.is_empty() {
                if index >= run_until
                    && let Some((color, right)) = fill_at(index, &mut rows_from, p.x)
                {
                    // The run of adjacent lines filled in this colour to this
                    // right edge, as one quad from the text area's left edge,
                    // over each of their visual rows: the pixels the lines'
                    // own fills would paint, painted before any of the run's
                    // backgrounds.
                    let mut bottom = p.y + line.size(line_height).height;
                    let (mut end, mut from) = (index + 1, rows_from);
                    let mut gap = gap_after(buffer_line);
                    while !gap
                        && end < layout.lines.len()
                        && fill_at(end, &mut from, p.x) == Some((color, right))
                    {
                        bottom += layout.lines[end].size(line_height).height;
                        gap = gap_after(layout.visible_buffer_lines[end]);
                        end += 1;
                    }
                    run_until = end;
                    window.paint_quad(fill(
                        Bounds::from_corners(p, point(right, bottom)),
                        color,
                    ));
                }
                // The active line, painted before this pass, again over the
                // fill, as it shows past the text on a row without one.
                if index < run_until
                    && prepaint.line_numbers.is_some()
                    && prepaint.current_row == Some(buffer_line)
                    && let Some(active) = active_line_color
                {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(input_bounds.origin.x, p.y),
                            size(bounds.size.width, line.size(line_height).height),
                        ),
                        active,
                    ));
                }
            }
            line.paint_background(
                p,
                line_height,
                text_align,
                Some(prepaint.last_layout.content_width),
                window,
                cx,
            );
            if !rules.is_empty() {
                while rules
                    .get(rules_from)
                    .is_some_and(|(range, _)| range.end <= line_start)
                {
                    rules_from += 1;
                }
                if rules_from < rules.len() {
                    paint_bottom_rules(
                        line,
                        line_start,
                        p,
                        line_height,
                        text_align,
                        Some(prepaint.last_layout.content_width),
                        &rules[rules_from..],
                        window,
                    );
                }
            }

            offset_y += line.size(line_height).height;

            // Ghost lines shift every later line down.
            if Some(buffer_line) == prepaint.current_row {
                offset_y += prepaint.ghost_lines_height;
            }
        }

        // Paint indent guides
        if let Some(path) = prepaint.indent_guides_path.take() {
            window.paint_path(path, editor_style.border.opacity(0.85));
        }

        // Paint selections
        if window.is_window_active() {
            let secondary_selection = Hsla {
                s: 0.1,
                ..editor_style.selection
            };
            for (path, is_active) in prepaint.search_match_paths.iter() {
                window.paint_path(path.clone(), secondary_selection);

                if *is_active {
                    window.paint_path(path.clone(), editor_style.selection);
                }
            }

            if let Some(path) = prepaint.selection_path.take() {
                window.paint_path(path, editor_style.selection);
            }

            // Paint hover highlight
            if let Some(path) = prepaint.hover_highlight_path.take() {
                window.paint_path(path, secondary_selection);
            }
        }

        // Paint document colors
        for (path, color) in prepaint.document_color_paths.iter() {
            let color = if disabled { color.opacity(0.5) } else { *color };
            window.paint_path(path.clone(), color);
        }

        // Paint text with inline completion ghost line support
        let mut offset_y = invisible_top_padding;
        let ghost_lines = &prepaint.ghost_lines;
        let has_ghost_lines = !ghost_lines.is_empty();

        // Track the y-position of the cursor row for positioning the first line suffix
        let mut cursor_row_y = None;

        for (line, &buffer_line) in prepaint
            .last_layout
            .lines
            .iter()
            .zip(prepaint.last_layout.visible_buffer_lines.iter())
        {
            let row = buffer_line;
            let line_y = origin.y + offset_y;
            let p = point(
                origin.x + prepaint.last_layout.line_number_width + (scroll_offset),
                line_y,
            );

            // Paint the actual line
            _ = line.paint(
                p,
                line_height,
                text_align,
                Some(prepaint.last_layout.content_width),
                window,
                cx,
            );
            offset_y += line.size(line_height).height;

            if Some(row) == prepaint.current_row {
                cursor_row_y = Some(line_y);
            }

            // After the cursor row, paint ghost lines (which shifts subsequent content down)
            if has_ghost_lines && Some(row) == prepaint.current_row {
                let ghost_x = origin.x + prepaint.last_layout.line_number_width;

                for ghost_line in ghost_lines {
                    let ghost_p = point(ghost_x, origin.y + offset_y);

                    // Paint semi-transparent background for ghost line
                    let ghost_bounds = Bounds::new(
                        ghost_p,
                        size(
                            bounds.size.width - prepaint.last_layout.line_number_width,
                            line_height,
                        ),
                    );
                    window.paint_quad(fill(ghost_bounds, editor_background));

                    // Paint ghost line text
                    _ = ghost_line.paint(
                        ghost_p,
                        line_height,
                        text_align,
                        Some(prepaint.last_layout.content_width),
                        window,
                        cx,
                    );
                    offset_y += line_height;
                }
            }
        }

        // Paint blinking cursor
        if focused && show_cursor {
            if let Some(cursor_bounds) = prepaint.cursor_bounds_with_scroll() {
                window.paint_quad(fill(cursor_bounds, editor_style.caret));
            }
        }

        // Paint line numbers
        let mut offset_y = px(0.);
        if let Some(line_numbers) = prepaint.line_numbers.as_ref() {
            offset_y += invisible_top_padding;

            // The gutter is a fixed overlay above horizontally scrolling text.
            // Always give it an opaque editor background when the theme does not
            // provide a dedicated gutter color, and cover the complete gutter so
            // text cannot show through its right-side spacing/fold-icon area.
            let gutter_bg = editor_style
                .editor_gutter_background
                .unwrap_or(editor_background);
            let gutter_bounds = editor_gutter_bounds(
                input_bounds,
                prepaint.last_layout.line_number_width,
                prepaint.ghost_lines_height,
                editor_paddings,
            );
            window.paint_quad(fill(gutter_bounds, gutter_bg));

            // Each item is the normal lines.
            for (lines, &buffer_line) in line_numbers
                .iter()
                .zip(prepaint.last_layout.visible_buffer_lines.iter())
            {
                let p = point(input_bounds.origin.x, origin.y + offset_y);
                let is_active = prepaint.current_row == Some(buffer_line);

                let height = line_height * lines.len() as f32;
                // paint active line number background
                if is_active {
                    if let Some(bg_color) = active_line_color {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(gutter_bounds.origin.x, p.y),
                                size(gutter_bounds.size.width, height),
                            ),
                            bg_color,
                        ));
                    }
                }

                for line in lines {
                    _ = line.paint(p, line_height, TextAlign::Left, None, window, cx);
                    offset_y += line_height;
                }

                // Add ghost line height after cursor row for line numbers alignment
                if !prepaint.ghost_lines.is_empty() && prepaint.current_row == Some(buffer_line) {
                    offset_y += prepaint.ghost_lines_height;
                }
            }
        }

        // Paint fold icons (only visible on hover or for current line)
        self.paint_fold_icons(
            &mut prepaint.fold_icon_layout,
            prepaint.current_row,
            window,
            cx,
        );

        self.state.update(cx, |state, cx| {
            state.last_layout = Some(prepaint.last_layout.clone());
            state.last_bounds = Some(bounds);
            state.last_cursor = Some(state.cursor());
            state.set_input_bounds(input_bounds, cx);
            state.last_selected_range = Some(selected_range);
            state.scroll_size = prepaint.scroll_size;
            state.update_scroll_offset(Some(prepaint.cursor_scroll_offset), cx);
            state.deferred_scroll_offset = None;
            state.deferred_scroll_row = None;

            cx.notify();
        });

        if let Some(hitbox) = prepaint.hover_definition_hitbox.as_ref() {
            window.set_cursor_style(gpui::CursorStyle::PointingHand, &hitbox);
        }

        // Paint inline completion first line suffix (after cursor on same line)
        if focused {
            if let Some(first_line) = &prepaint.ghost_first_line {
                if let (Some(cursor_bounds), Some(cursor_row_y)) =
                    (prepaint.cursor_bounds_with_scroll(), cursor_row_y)
                {
                    let first_line_x = cursor_bounds.origin.x + cursor_bounds.size.width;
                    let p = point(first_line_x, cursor_row_y);

                    // Paint background to cover any existing text
                    let bg_bounds = Bounds::new(p, size(first_line.width + px(4.), line_height));
                    window.paint_quad(fill(bg_bounds, editor_background));

                    // Paint first line completion text
                    _ = first_line.paint(p, line_height, text_align, None, window, cx);
                }
            }
        }

        self.paint_mouse_listeners(window, cx);
    }
}

/// Split placeholder text into display lines and trim runs to each line.
fn placeholder_line_runs<'a>(
    display_text: &'a str,
    runs: &[TextRun],
) -> Vec<(&'a str, Vec<TextRun>)> {
    let mut result = Vec::new();
    let mut line_offset = 0;

    for line in display_text.split('\n') {
        let line_runs = runs_for_range(runs, line_offset, &(0..line.len()));
        debug_assert_eq!(
            line_runs.iter().map(|run| run.len).sum::<usize>(),
            line.len()
        );
        result.push((line, line_runs));
        // Advance in the whole-placeholder coordinate space, including the separator.
        line_offset += line.len() + 1;
    }

    result
}

/// Get the runs for the given range.
///
/// The range is the byte range of the wrapped line.
pub(super) fn runs_for_range(
    runs: &[TextRun],
    line_offset: usize,
    range: &Range<usize>,
) -> Vec<TextRun> {
    let mut result = vec![];
    let range = (line_offset + range.start)..(line_offset + range.end);
    let mut cursor = 0;

    for run in runs {
        let run_start = cursor;
        let run_end = cursor + run.len;

        if run_end <= range.start {
            cursor = run_end;
            continue;
        }

        if run_start >= range.end {
            break;
        }

        let start = range.start.max(run_start) - run_start;
        let end = range.end.min(run_end) - run_start;
        let len = end - start;

        if len > 0 {
            result.push(TextRun { len, ..run.clone() });
        }

        cursor = run_end;
    }

    result
}

/// Clip sorted `styles` to the sorted, disjoint `ranges`, splitting styles
/// that span several ranges and dropping coverage in the gaps between them.
fn clip_styles_to_ranges(
    styles: Vec<(Range<usize>, HighlightStyle)>,
    ranges: &[Range<usize>],
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut result = Vec::with_capacity(styles.len());
    let mut range_ix = 0;

    for (style_range, style) in styles {
        while range_ix < ranges.len() && ranges[range_ix].end <= style_range.start {
            range_ix += 1;
        }

        let mut ix = range_ix;
        while ix < ranges.len() && ranges[ix].start < style_range.end {
            let start = style_range.start.max(ranges[ix].start);
            let end = style_range.end.min(ranges[ix].end);
            if start < end {
                result.push((start..end, style));
            }
            ix += 1;
        }
    }

    result
}

/// Snap run boundaries to char boundaries of `text` and cap them at its
/// length. Style ranges are byte offsets that can drift off char boundaries
/// (a stale syntax tree, a range in the wrong coordinate space) and the
/// platform text system panics when a run splits a multi-byte character.
///
/// Returns `None` when the runs are already aligned (the common case).
pub(super) fn align_runs_to_char_boundaries(text: &str, runs: &[TextRun]) -> Option<Vec<TextRun>> {
    let mut end = 0;
    let aligned = runs.iter().all(|run| {
        end += run.len;
        end <= text.len() && text.is_char_boundary(end)
    });
    if aligned {
        return None;
    }

    let mut result = Vec::with_capacity(runs.len());
    let mut cursor = 0;
    let mut raw_end = 0;
    for run in runs {
        raw_end = (raw_end + run.len).min(text.len());
        let mut end = raw_end;
        while !text.is_char_boundary(end) {
            end += 1;
        }
        if end > cursor {
            result.push(TextRun {
                len: end - cursor,
                ..run.clone()
            });
            cursor = end;
        }
    }

    Some(result)
}

fn split_run_for_ime_underline(
    run: TextRun,
    run_range: Range<usize>,
    marked_range: Option<Range<usize>>,
    marked_underline: Option<UnderlineStyle>,
) -> SmallVec<[TextRun; 3]> {
    if run.len == 0 {
        return SmallVec::new();
    }

    let Some(marked) = marked_range else {
        return [run].into_iter().collect();
    };

    let intersection_start = run_range.start.max(marked.start);
    let intersection_end = run_range.end.min(marked.end);
    if intersection_start >= intersection_end {
        return [run].into_iter().collect();
    }

    [
        TextRun {
            len: intersection_start - run_range.start,
            ..run.clone()
        },
        TextRun {
            len: intersection_end - intersection_start,
            underline: marked_underline,
            ..run.clone()
        },
        TextRun {
            len: run_range.end - intersection_end,
            ..run
        },
    ]
    .into_iter()
    .filter(|run| run.len > 0)
    .collect()
}

/// Whether any of these runs paints a glyph background, used to skip the background
/// paint pass for lines without highlights.
fn has_background(runs: &[TextRun]) -> bool {
    runs.iter().any(|run| run.background_color.is_some())
}

fn split_runs_by_bg_segments(
    start_offset: usize,
    runs: &[TextRun],
    bg_segments: &[(Range<usize>, Hsla)],
) -> Vec<TextRun> {
    let mut result = vec![];

    let mut cursor = start_offset;
    for run in runs {
        let mut run_start = cursor;
        let run_end = cursor + run.len;

        for (bg_range, bg_color) in bg_segments {
            if run_end <= bg_range.start || run_start >= bg_range.end {
                continue;
            }

            // Overlap exists
            if run_start < bg_range.start {
                // Add the part before the background range
                result.push(TextRun {
                    len: bg_range.start - run_start,
                    ..run.clone()
                });
            }

            // Add the overlapping part with background color
            let overlap_start = run_start.max(bg_range.start);
            let overlap_end = run_end.min(bg_range.end);
            let text_color = if bg_color.l >= 0.5 {
                gpui::black()
            } else {
                gpui::white()
            };

            let run_len = overlap_end.saturating_sub(overlap_start);
            if run_len > 0 {
                result.push(TextRun {
                    len: run_len,
                    color: text_color,
                    ..run.clone()
                });

                cursor = bg_range.end;
                run_start = cursor;
            }
        }

        if run_end > cursor {
            // Add the part after the background range
            result.push(TextRun {
                len: run_end - cursor,
                ..run.clone()
            });
        }

        cursor = run_end;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_decorations_include_unstyled_gaps() {
        let decoration = HighlightStyle {
            background_color: Some(gpui::red()),
            ..Default::default()
        };
        let styles = compose_decorations(Vec::new(), [(2..5, decoration)], 0..10).unwrap();

        assert_eq!(
            styles
                .iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            vec![0..2, 2..5, 5..10]
        );
        assert_eq!(styles[0].1, HighlightStyle::default());
        assert_eq!(styles[1].1.background_color, Some(gpui::red()));
        assert_eq!(styles[2].1, HighlightStyle::default());
    }

    #[test]
    fn test_first_decoration_collection_has_precedence() {
        let first = [TextDecoration::new(
            0..4,
            HighlightStyle {
                background_color: Some(gpui::red()),
                ..Default::default()
            },
        )];
        let second = [TextDecoration::new(
            0..4,
            HighlightStyle {
                background_color: Some(gpui::blue()),
                ..Default::default()
            },
        )];

        let styles =
            compose_decoration_collections(Vec::new(), [&first[..], &second[..]], 0..4, &mut RowBackgrounds::new())
                .unwrap();

        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].1.background_color, Some(gpui::red()));
    }

    /// A bottom rule is read where it is visible, and an earlier collection's
    /// underline hides it, as Find's current match does; an earlier
    /// collection's background alone, as Find's other matches, and an
    /// underline in its own collection or a later one do not. Underlines
    /// reaching across several rules, and one ending inside another, hide
    /// each rule only where they lie over it. The styles are composed as
    /// without the rules.
    #[test]
    fn bottom_rules_are_hidden_only_by_an_earlier_collections_underline() {
        let tint = |color| HighlightStyle {
            background_color: Some(color),
            ..Default::default()
        };
        let underlined = |color| HighlightStyle {
            underline: Some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.),
                wavy: false,
            }),
            ..tint(color)
        };
        let (rule, other, third) = (gpui::green(), gpui::yellow(), gpui::white());
        let find = [
            TextDecoration::new(0..3, underlined(gpui::red())),
            TextDecoration::new(4..6, underlined(gpui::red())),
            TextDecoration::new(7..8, tint(gpui::red())),
            TextDecoration::new(25..40, underlined(gpui::red())),
        ];
        let patch = [
            TextDecoration::new(2..9, tint(gpui::blue())),
            TextDecoration::new(2..9, HighlightStyle::default()).with_bottom_rule(rule),
            TextDecoration::new(12..20, tint(gpui::blue())).with_bottom_rule(other),
            TextDecoration::new(22..30, tint(gpui::blue())).with_bottom_rule(third),
            TextDecoration::new(32..36, tint(gpui::blue())).with_bottom_rule(third),
        ];
        let later = [TextDecoration::new(13..15, underlined(gpui::red()))];
        let plain = patch.clone().map(|decoration| TextDecoration {
            bottom_rule: None,
            ..decoration
        });

        let mut rules = vec![(0..1, gpui::black())];
        let styles = compose_ruled_decoration_collections(
            Vec::new(),
            [&find[..], &patch[..], &later[..]],
            0..34,
            &mut rules,
            &mut RowBackgrounds::new(),
        );
        assert_eq!(
            rules,
            [(3..4, rule), (6..9, rule), (12..20, other), (22..25, third)],
            "hidden under the earlier underlines, kept under the earlier background alone and \
             the later underline, and clipped to the visible bytes"
        );
        assert_eq!(
            styles,
            compose_decoration_collections(
                Vec::new(),
                [&find[..], &plain[..], &later[..]],
                0..34,
                &mut RowBackgrounds::new()
            ),
            "rules leave the styles upstream's composition gives"
        );
        let mut none = vec![(0..1, gpui::black())];
        compose_decoration_layers(
            Vec::new(),
            vec![&find[..], &plain[..], &later[..]],
            false,
            0..34,
            &mut none,
            &mut RowBackgrounds::new(),
        );
        assert_eq!(
            none,
            [(0..1, gpui::black())],
            "a frame without rules does no rule work"
        );
    }

    /// The single walk that finds where each rule's box starts and ends
    /// answers as the first-match search over the glyphs in walk order does,
    /// for glyphs ascending, shuffled and descending (as a right-to-left run
    /// lays them out), at indices before, between, on and past every glyph.
    #[test]
    fn first_glyphs_at_answers_as_a_search_in_walk_order() {
        let ascending = (0..40).map(|index| index * 3).collect::<Vec<usize>>();
        let mut shuffled = ascending.clone();
        // A fixed permutation: a stride coprime with the length.
        for (slot, index) in shuffled.iter_mut().enumerate() {
            *index = ascending[(slot * 17) % ascending.len()];
        }
        let descending = ascending.iter().rev().copied().collect::<Vec<_>>();
        let queries = (0..125).rev().chain([0, 60, 60, 200]).collect::<Vec<_>>();
        for (order, indices) in [
            ("ascending", ascending),
            ("shuffled", shuffled),
            ("descending", descending),
        ] {
            let glyphs = indices
                .iter()
                .enumerate()
                .map(|(slot, &index)| (index, px(slot as f32 * 7.5)))
                .collect::<Vec<_>>();
            let found = first_glyphs_at(glyphs.iter().copied(), &queries);
            for (query, found) in queries.iter().zip(found) {
                let searched = glyphs
                    .iter()
                    .find(|(index, _)| index >= query)
                    .map(|(_, x)| *x);
                assert_eq!(found, searched, "{order} glyphs at byte {query}");
            }
        }
    }

    /// A decoration's bottom rule is painted inside its box's bottom edge,
    /// as wide as the box and a 1 px stroke tall on whole device pixels,
    /// whether it rides on the decoration that sets the background or on a
    /// decoration of its own over the same range; a decoration without one
    /// paints exactly what it painted before the field existed.
    #[gpui::test]
    fn a_bottom_rule_is_painted_inside_its_boxs_bottom_edge(cx: &mut gpui::TestAppContext) {
        use crate::input::{EditorMode, EditorState};
        use gpui::{AppContext as _, Background, Context, IntoElement, Render, div};

        struct Root(Entity<InputBaseState<EditorMode>>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size_full().child(self.0.clone())
            }
        }

        let mut editor = None;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.set_global(crate::theme::Theme::default());
                crate::input::init(cx);
                let state = cx.new(|cx| {
                    EditorState::new(window, cx).default_value("first line\nsecond line\nthird\n")
                });
                editor = Some(state.clone());
                cx.new(|_| Root(state))
            })
            .unwrap()
        });
        let editor = editor.unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);

        let (tint, own, shared) = (gpui::blue(), gpui::green(), gpui::red());
        let background = HighlightStyle {
            background_color: Some(tint),
            ..Default::default()
        };
        // "first" without a rule, "second" with a rule of its own over the
        // same range, and "line" on the second line with the rule on its
        // background decoration.
        let ruled = vec![
            TextDecoration::new(0..5, background),
            TextDecoration::new(11..17, background),
            TextDecoration::new(11..17, HighlightStyle::default()).with_bottom_rule(own),
            TextDecoration::new(18..22, background).with_bottom_rule(shared),
        ];
        let plain = ruled
            .iter()
            .cloned()
            .map(|decoration| TextDecoration {
                bottom_rule: None,
                ..decoration
            })
            .collect::<Vec<_>>();
        let collection = cx.update(|_, cx| {
            editor.update(cx, |state, cx| {
                state.create_decorations_collection(ruled.clone(), cx)
            })
        });
        let painted = |decorations: &[TextDecoration], cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| collection.set(decorations.to_vec(), cx));
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                window
                    .painted_quads()
                    .into_iter()
                    .map(|quad| (quad.bounds, quad.background))
                    .collect::<Vec<_>>()
            })
        };

        for scale in [2., 1., 1.2, 1.25, 1.5] {
            cx.update(|window, _| window.set_scale_factor(scale));
            let with_rules = painted(&ruled, cx);
            let without = painted(&plain, cx);
            let boxes = without
                .iter()
                .filter(|(_, fill)| *fill == Background::from(tint))
                .map(|(bounds, _)| *bounds)
                .collect::<Vec<_>>();
            assert_eq!(boxes.len(), 3, "scale {scale}: one box a decorated word");
            // 1 px on its device pixels, halves toward zero, as GPUI snaps a stroke.
            let stroke: f32 = if scale < 2. { 1. } else { 2. };
            for (word, color) in [(1, own), (2, shared)] {
                let tinted = boxes[word];
                let rules = with_rules
                    .iter()
                    .filter(|(_, fill)| *fill == Background::from(color))
                    .map(|(bounds, _)| *bounds)
                    .collect::<Vec<_>>();
                assert_eq!(rules.len(), 1, "scale {scale}: word {word} has one rule");
                let rule = rules[0];
                assert_eq!(
                    (rule.left(), rule.right(), rule.bottom()),
                    (tinted.left(), tinted.right(), tinted.bottom()),
                    "scale {scale}: word {word}'s rule spans its box {tinted:?} along its bottom edge"
                );
                assert_eq!(
                    rule.size.height.0, stroke,
                    "scale {scale}: word {word}'s rule is a 1 px stroke on whole device pixels"
                );
                assert_eq!(rule.top().0.fract(), 0., "scale {scale}: on a device pixel");
                // The scene draws in this order: the rule over its box.
                let drawn = |bounds: Bounds<gpui::ScaledPixels>, fill: Background| {
                    with_rules
                        .iter()
                        .position(|quad| *quad == (bounds, fill))
                        .expect("painted")
                };
                assert!(
                    drawn(rule, Background::from(color)) > drawn(tinted, Background::from(tint)),
                    "scale {scale}: word {word}'s rule is drawn over its box"
                );
            }
            let mut rest = with_rules
                .into_iter()
                .filter(|(_, fill)| ![own, shared].map(Background::from).contains(fill))
                .collect::<Vec<_>>();
            let mut without = without;
            let order = |quads: &mut Vec<(Bounds<gpui::ScaledPixels>, Background)>| {
                quads.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")))
            };
            order(&mut rest);
            order(&mut without);
            assert_eq!(
                rest, without,
                "scale {scale}: the rules add their quads and nothing else"
            );
        }
    }

    /// A range that soft wrapping lays over several visual lines draws its
    /// bottom rule under each of its boxes, one a visual line.
    #[gpui::test]
    fn a_wrapped_range_draws_its_bottom_rule_under_each_visual_line(cx: &mut gpui::TestAppContext) {
        use crate::input::{EditorMode, EditorState};
        use gpui::{AppContext as _, Background, Context, IntoElement, Render, Styled as _, div};

        struct Root(Entity<InputBaseState<EditorMode>>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(160.)).h(px(400.)).child(self.0.clone())
            }
        }

        let text = "alpha beta gamma delta epsilon zeta eta theta";
        let mut editor = None;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.set_global(crate::theme::Theme::default());
                crate::input::init(cx);
                let state = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .soft_wrap(true)
                        .default_value(text)
                });
                editor = Some(state.clone());
                cx.new(|_| Root(state))
            })
            .unwrap()
        });
        let editor = editor.unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let (tint, rule) = (gpui::blue(), gpui::green());
        let background = HighlightStyle {
            background_color: Some(tint),
            ..Default::default()
        };
        cx.update(|_, cx| {
            editor.update(cx, |state, cx| {
                state.create_decorations_collection(
                    vec![TextDecoration::new(0..text.len(), background).with_bottom_rule(rule)],
                    cx,
                )
            })
        });
        let quads = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.painted_quads()
        });
        let painted = |color| {
            quads
                .iter()
                .filter(|quad| quad.background == Background::from(color))
                .map(|quad| quad.bounds)
                .collect::<Vec<_>>()
        };
        let (boxes, rules) = (painted(tint), painted(rule));
        assert!(boxes.len() > 1, "the range wraps: {boxes:?}");
        assert_eq!(rules.len(), boxes.len(), "a rule a visual line");
        for (tinted, rule) in boxes.iter().zip(&rules) {
            assert_eq!(
                (rule.left(), rule.right(), rule.bottom()),
                (tinted.left(), tinted.right(), tinted.bottom()),
                "the rule spans its visual line's box {tinted:?} along its bottom edge"
            );
            assert_eq!(rule.size.height.0, 2., "a 1 px stroke at scale 2");
        }
    }

    /// Row backgrounds are read where their range reaches the visible lines,
    /// one over only the last visible line's break included, in either
    /// composition, ascending by start, and leave the styles as composed
    /// without them; a line takes the first collection's fill, and within a
    /// collection the one starting first.
    #[test]
    fn row_backgrounds_are_read_where_they_reach_the_visible_lines() {
        let tint = HighlightStyle {
            background_color: Some(gpui::red()),
            ..Default::default()
        };
        let (first, second, third) = (gpui::green(), gpui::blue(), gpui::yellow());
        // Visible bytes 10..30: before them, across their start, inside,
        // over only the last line's break at 30, and after it.
        let earlier = [TextDecoration::new(20..24, tint).with_row_background(first)];
        let later = [
            TextDecoration::new(0..10, tint).with_row_background(second),
            TextDecoration::new(30..31, HighlightStyle::default()).with_row_background(second),
            TextDecoration::new(5..12, tint).with_row_background(third),
            TextDecoration::new(14..26, HighlightStyle::default()).with_row_background(second),
            TextDecoration::new(31..40, tint).with_row_background(second),
            TextDecoration::new(16..18, tint).with_bottom_rule(gpui::white()),
        ];
        let plain = |decorations: &[TextDecoration]| {
            decorations
                .iter()
                .cloned()
                .map(|decoration| TextDecoration {
                    row_background: None,
                    ..decoration
                })
                .collect::<Vec<_>>()
        };
        let (earlier_plain, later_plain) = (plain(&earlier), plain(&later));
        for ruled in [false, true] {
            let mut rows = RowBackgrounds::from_vec(vec![(0..1, gpui::black(), 9)]);
            let styles = compose_decoration_layers(
                Vec::new(),
                vec![&earlier[..], &later[..]],
                ruled,
                10..30,
                &mut Vec::new(),
                &mut rows,
            );
            assert_eq!(
                rows.as_slice(),
                [
                    (5..12, third, 1),
                    (14..26, second, 1),
                    (20..24, first, 0),
                    (30..31, second, 1)
                ],
                "ruled {ruled}: the fills reaching bytes 10 to 30 or the break at 30, by start"
            );
            let mut none = RowBackgrounds::new();
            assert_eq!(
                styles,
                compose_decoration_layers(
                    Vec::new(),
                    vec![&earlier_plain[..], &later_plain[..]],
                    ruled,
                    10..30,
                    &mut Vec::new(),
                    &mut none,
                ),
                "ruled {ruled}: the fills leave the styles as composed without them"
            );
            assert!(none.is_empty(), "ruled {ruled}: no fill without the field");
            // A line's text and its break, and the fill painted on it.
            let at = |start, end| row_background_at(&rows, start, end);
            assert_eq!(at(10, 11), Some(third), "a fill over the line's text");
            assert_eq!(at(12, 14), Some(second), "a fill over the line's break alone");
            assert_eq!(at(11, 15), Some(third), "the collection's fill starting first");
            assert_eq!(at(19, 25), Some(first), "the first collection's fill");
            assert_eq!(at(30, 30), Some(second), "a blank line's break");
            assert_eq!(at(26, 29), None, "no fill reaching the line");
        }
    }

    /// A row background fills the whole row of each line its range reaches,
    /// from the text area's left edge, where a background on the line's first
    /// byte starts, to the editor's visible right edge, a blank line's row
    /// included, in one quad with the adjacent line that ends at the same
    /// edge, a line wider than the view to its end, and the row ends at the
    /// visible right edge scrolled sideways too; it is painted under the
    /// line's decoration backgrounds, and a decoration without one paints
    /// exactly what it painted before the field existed.
    #[gpui::test]
    fn a_row_background_fills_its_lines_rows_to_the_visible_right_edge(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::input::{EditorMode, EditorState};
        use gpui::{AppContext as _, Background, Context, IntoElement, Render, Styled as _, div};

        struct Root(Entity<InputBaseState<EditorMode>>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(400.)).h(px(300.)).child(self.0.clone())
            }
        }

        let long = "wide ".repeat(40);
        let text = format!("short\n\n{long}\nplain\n");
        let mut editor = None;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.set_global(crate::theme::Theme::default());
                crate::input::init(cx);
                let state = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .soft_wrap(false)
                        .default_value(text.clone())
                });
                editor = Some(state.clone());
                cx.new(|_| Root(state))
            })
            .unwrap()
        });
        let editor = editor.unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);

        let (tint, row) = (gpui::blue(), gpui::green());
        let background = HighlightStyle {
            background_color: Some(tint),
            ..Default::default()
        };
        // "short" tinted, and the first three lines filled, the long line's
        // break included; "plain" neither.
        let filled = vec![
            TextDecoration::new(0..5, background),
            TextDecoration::new(0..8 + long.len(), HighlightStyle::default())
                .with_row_background(row),
        ];
        let plain = filled
            .iter()
            .cloned()
            .map(|decoration| TextDecoration {
                row_background: None,
                ..decoration
            })
            .collect::<Vec<_>>();
        let collection = cx.update(|_, cx| {
            editor.update(cx, |state, cx| {
                state.create_decorations_collection(filled.clone(), cx)
            })
        });
        let painted = |decorations: &[TextDecoration], cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| collection.set(decorations.to_vec(), cx));
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                window
                    .painted_quads()
                    .into_iter()
                    .map(|quad| (quad.bounds, quad.background))
                    .collect::<Vec<_>>()
            })
        };

        for (scale, x) in [(2., 0.), (1.25, 0.), (2., -60.)] {
            cx.update(|window, cx| {
                window.set_scale_factor(scale);
                editor.update(cx, |state, cx| {
                    state.set_scroll_offset(point(px(x), px(0.)), cx)
                });
                window.draw(cx).clear(cx);
            });
            let at = format!("scale {scale}, scrolled {x} px");
            let with_rows = painted(&filled, cx);
            let without = painted(&plain, cx);
            let visible_right =
                cx.update(|_, cx| editor.read(cx).input_bounds().right().as_f32() * scale);
            let boxes = without
                .iter()
                .filter(|(_, fill)| *fill == Background::from(tint))
                .map(|(bounds, _)| *bounds)
                .collect::<Vec<_>>();
            assert_eq!(boxes.len(), 1, "{at}: one box under \"short\"");
            let mut rows = with_rows
                .iter()
                .filter(|(_, fill)| *fill == Background::from(row))
                .map(|(bounds, _)| *bounds)
                .collect::<Vec<_>>();
            rows.sort_by(|a, b| a.top().0.total_cmp(&b.top().0));
            // The short and blank lines end at the same edge and share one
            // quad; the long line's reaches its own end.
            assert_eq!(rows.len(), 2, "{at}: a quad a run of rows: {rows:?}");
            let height = boxes[0].size.height.0;
            for (run, (filled, lines)) in rows.iter().zip([2., 1.]).enumerate() {
                assert_eq!(
                    filled.left(),
                    boxes[0].left(),
                    "{at}: run {run}'s rows start where \"short\"'s box does"
                );
                // As tall as its lines within the device pixel each edge is
                // rounded to, the first starting with "short"'s box.
                assert!(
                    filled.top() == if run == 0 { boxes[0].top() } else { rows[0].bottom() }
                        && (filled.size.height.0 - height * lines).abs() <= 1.,
                    "{at}: run {run}'s quad {filled:?} covers its {lines} lines' rows: {rows:?}"
                );
            }
            assert!(
                (rows[0].right().0 - visible_right).abs() <= 0.5,
                "{at}: the short and blank lines' rows {:?} end at the visible right edge {visible_right}",
                rows[0]
            );
            assert!(
                rows[1].right().0 > visible_right + 0.5,
                "{at}: the long line's row {:?} reaches its end past the edge {visible_right}",
                rows[1]
            );
            let drawn = |quad: (Bounds<gpui::ScaledPixels>, Background)| {
                with_rows.iter().position(|painted| *painted == quad).unwrap()
            };
            assert!(
                drawn((rows[0], Background::from(row))) < drawn((boxes[0], Background::from(tint))),
                "{at}: the row is painted under the line's background"
            );
            let mut rest = with_rows
                .into_iter()
                .filter(|(_, fill)| *fill != Background::from(row))
                .collect::<Vec<_>>();
            let mut without = without;
            let order = |quads: &mut Vec<(Bounds<gpui::ScaledPixels>, Background)>| {
                quads.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")))
            };
            order(&mut rest);
            order(&mut without);
            assert_eq!(rest, without, "{at}: the rows add their quads and nothing else");
        }
    }

    /// A soft-wrapped line's row background covers each of its visual rows
    /// to the visible right edge, in one fill.
    #[gpui::test]
    fn a_wrapped_line_s_row_background_covers_each_visual_row(cx: &mut gpui::TestAppContext) {
        use crate::input::{EditorMode, EditorState};
        use gpui::{AppContext as _, Background, Context, IntoElement, Render, Styled as _, div};

        struct Root(Entity<InputBaseState<EditorMode>>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(160.)).h(px(400.)).child(self.0.clone())
            }
        }

        let text = "alpha beta gamma delta epsilon zeta eta theta";
        let mut editor = None;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.set_global(crate::theme::Theme::default());
                crate::input::init(cx);
                let state = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .soft_wrap(true)
                        .default_value(text)
                });
                editor = Some(state.clone());
                cx.new(|_| Root(state))
            })
            .unwrap()
        });
        let editor = editor.unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let (tint, row) = (gpui::blue(), gpui::green());
        let background = HighlightStyle {
            background_color: Some(tint),
            ..Default::default()
        };
        cx.update(|_, cx| {
            editor.update(cx, |state, cx| {
                state.create_decorations_collection(
                    vec![TextDecoration::new(0..text.len(), background).with_row_background(row)],
                    cx,
                )
            })
        });
        let quads = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.painted_quads()
        });
        let painted = |color| {
            quads
                .iter()
                .filter(|quad| quad.background == Background::from(color))
                .map(|quad| quad.bounds)
                .collect::<Vec<_>>()
        };
        let (boxes, rows) = (painted(tint), painted(row));
        let right = cx.update(|_, cx| editor.read(cx).input_bounds().right().as_f32() * 2.);
        assert!(boxes.len() > 1, "the line wraps: {boxes:?}");
        assert_eq!(rows.len(), 1, "one fill for the line: {rows:?}");
        let (first, last) = (boxes[0], boxes[boxes.len() - 1]);
        assert_eq!(
            (rows[0].left(), rows[0].top(), rows[0].bottom()),
            (first.left(), first.top(), last.bottom()),
            "the fill covers every visual row from the text's left edge"
        );
        assert!(
            (rows[0].right().0 - right).abs() <= 0.5,
            "the fill {rows:?} reaches the visible right edge {right}"
        );
    }

    /// With line numbers on, the active line is painted again over a row
    /// background on its row, so its colour shows past the text there as on
    /// a row without one, whether the caret is on the run's first line or a
    /// later one; the other filled row, in the same quad, keeps the fill.
    #[gpui::test]
    fn the_active_line_shows_over_a_row_background(cx: &mut gpui::TestAppContext) {
        use crate::input::{EditorMode, EditorState, InputEditorStyle};
        use gpui::{AppContext as _, Background, Context, IntoElement, Render, Styled as _, div};

        struct Root(Entity<InputBaseState<EditorMode>>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(400.)).h(px(300.)).child(self.0.clone())
            }
        }

        let (active, row) = (gpui::yellow(), gpui::green());
        let text = "short\nnext\n";
        let mut editor = None;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.set_global(crate::theme::Theme::default());
                crate::input::init(cx);
                let state = cx.new(|cx| {
                    let mut state = EditorState::new(window, cx)
                        .line_number(true)
                        .soft_wrap(false)
                        .default_value(text);
                    state.set_editor_style(InputEditorStyle {
                        editor_active_line: Some(active),
                        ..Default::default()
                    });
                    state
                });
                editor = Some(state.clone());
                cx.new(|_| Root(state))
            })
            .unwrap()
        });
        let editor = editor.unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        // Both lines filled, in one run.
        cx.update(|_, cx| {
            editor.update(cx, |state, cx| {
                state.create_decorations_collection(
                    vec![
                        TextDecoration::new(0..text.len(), HighlightStyle::default())
                            .with_row_background(row),
                    ],
                    cx,
                )
            })
        });
        // The caret on the run's first line, then on its second.
        for caret in [0, 1] {
            let quads = cx.update(|window, cx| {
                editor.update(cx, |state, cx| {
                    state.set_cursor_position(lsp_types::Position::new(caret, 0), window, cx)
                });
                window.draw(cx).clear(cx);
                window.painted_quads()
            });
            let painted = |color| {
                quads
                    .iter()
                    .enumerate()
                    .filter(|(_, quad)| quad.background == Background::from(color))
                    .map(|(order, quad)| (order, quad.bounds))
                    .collect::<Vec<_>>()
            };
            let (fills, lines) = (painted(row), painted(active));
            assert_eq!(fills.len(), 1, "caret on line {caret}: one fill for both lines: {fills:?}");
            let (order, fill) = fills[0];
            // Across the fill: the active line, not the gutter's line number.
            let across = |line: &Bounds<gpui::ScaledPixels>| {
                line.left() <= fill.left() && line.right() >= fill.right()
            };
            // The active row, as the active line painted before the pass marks it.
            let active_row = lines
                .iter()
                .find(|(drawn, line)| *drawn < order && across(line))
                .map(|(_, line)| *line)
                .expect("the active line is painted before the background pass");
            assert!(
                (active_row.top() == fill.top()) == (caret == 0)
                    && (active_row.bottom() == fill.bottom()) == (caret == 1),
                "the active row {active_row:?} is the fill's {fill:?} row {caret}"
            );
            assert!(
                lines.iter().any(|(drawn, line)| *drawn > order
                    && across(line)
                    && (line.top(), line.bottom()) == (active_row.top(), active_row.bottom())),
                "caret on line {caret}: the active line {lines:?} is painted over the fill \
                 {fill:?} on its row"
            );
            assert!(
                lines
                    .iter()
                    .filter(|(drawn, _)| *drawn > order)
                    .all(|(_, line)| (line.top(), line.bottom())
                        == (active_row.top(), active_row.bottom())),
                "caret on line {caret}: no active line over the other line's row: {lines:?}"
            );
        }
    }

    /// Adjacent lines filled in one colour to one edge are painted as one
    /// quad over all their rows: three such lines one quad, and an unfilled
    /// line between two runs two; a line filled in another colour right
    /// after a run takes a quad of its own.
    #[gpui::test]
    fn adjacent_filled_lines_paint_one_quad_a_run(cx: &mut gpui::TestAppContext) {
        use crate::input::{EditorMode, EditorState};
        use gpui::{AppContext as _, Background, Context, IntoElement, Render, Styled as _, div};

        struct Root(Entity<InputBaseState<EditorMode>>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(400.)).h(px(300.)).child(self.0.clone())
            }
        }

        let text = "a1\na2\na3\nplain\nb1\nb2\nc1\n";
        let mut editor = None;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.set_global(crate::theme::Theme::default());
                crate::input::init(cx);
                let state = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .soft_wrap(false)
                        .default_value(text)
                });
                editor = Some(state.clone());
                cx.new(|_| Root(state))
            })
            .unwrap()
        });
        let editor = editor.unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let (row, other) = (gpui::green(), gpui::blue());
        // "a1" to "a3" and "b1" to "b2" in one colour, "c1" in another.
        cx.update(|_, cx| {
            editor.update(cx, |state, cx| {
                state.create_decorations_collection(
                    vec![
                        TextDecoration::new(0..9, HighlightStyle::default())
                            .with_row_background(row),
                        TextDecoration::new(15..21, HighlightStyle::default())
                            .with_row_background(row),
                        TextDecoration::new(21..24, HighlightStyle::default())
                            .with_row_background(other),
                    ],
                    cx,
                )
            })
        });
        let quads = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.painted_quads()
        });
        let line_height = cx.update(|_, cx| editor.read(cx).line_height().unwrap().as_f32() * 2.);
        let painted = |color| {
            let mut painted = quads
                .iter()
                .filter(|quad| quad.background == Background::from(color))
                .map(|quad| quad.bounds)
                .collect::<Vec<_>>();
            painted.sort_by(|a, b| a.top().0.total_cmp(&b.top().0));
            painted
        };
        let (runs, others) = (painted(row), painted(other));
        assert_eq!(runs.len(), 2, "a quad a run of filled lines: {runs:?}");
        assert_eq!(others.len(), 1, "another colour's line its own quad: {others:?}");
        let rows = |quad: &Bounds<gpui::ScaledPixels>| quad.size.height.0 / line_height;
        assert!(
            (rows(&runs[0]) - 3.).abs() < 0.05 && (rows(&runs[1]) - 2.).abs() < 0.05,
            "the runs cover three rows and two: {runs:?} at {line_height} a row"
        );
        assert!(
            (runs[1].top().0 - runs[0].bottom().0 - line_height).abs() < 0.05,
            "the unfilled line lies between the runs: {runs:?}"
        );
        assert!(
            others[0].top() == runs[1].bottom() && (rows(&others[0]) - 1.).abs() < 0.05,
            "\"c1\" is filled on its own row under the second run: {others:?}, {runs:?}"
        );
    }

    /// The ghost lines an inline completion opens below the cursor's row, a
    /// filled line between lines filled in the same colour to the same edge,
    /// divide their run: the lines down to the cursor's take one quad and the
    /// lines after the ghost lines another, which do not touch, the gap
    /// between them the ghost lines' height.
    #[gpui::test]
    fn ghost_lines_divide_a_run_of_filled_lines(cx: &mut gpui::TestAppContext) {
        use crate::input::{EditorMode, EditorState};
        use gpui::{AppContext as _, Background, Context, IntoElement, Render, Styled as _, div};

        struct Root(Entity<InputBaseState<EditorMode>>);
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(400.)).h(px(300.)).child(self.0.clone())
            }
        }

        let text = "a1\na2\na3\na4\n";
        let mut editor = None;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.set_global(crate::theme::Theme::default());
                crate::input::init(cx);
                let state = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .soft_wrap(false)
                        .default_value(text)
                });
                editor = Some(state.clone());
                cx.new(|_| Root(state))
            })
            .unwrap()
        });
        let editor = editor.unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let row = gpui::green();
        // Every line filled in one colour; the caret, focused, on "a2", and
        // a completion there of three lines, two of them ghost lines below.
        let ghost_lines = 2.;
        cx.update(|window, cx| {
            editor.update(cx, |state, cx| {
                state.create_decorations_collection(
                    vec![
                        TextDecoration::new(0..text.len(), HighlightStyle::default())
                            .with_row_background(row),
                    ],
                    cx,
                );
                state.set_cursor_position(lsp_types::Position::new(1, 2), window, cx);
                state.extras.inline_completion.item = Some(lsp_types::InlineCompletionItem {
                    insert_text: "x\ny\nz".into(),
                    filter_text: None,
                    range: None,
                    command: None,
                    insert_text_format: None,
                });
            })
        });
        let quads = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.painted_quads()
        });
        let line_height = cx.update(|_, cx| editor.read(cx).line_height().unwrap().as_f32() * 2.);
        let mut runs = quads
            .iter()
            .filter(|quad| quad.background == Background::from(row))
            .map(|quad| quad.bounds)
            .collect::<Vec<_>>();
        runs.sort_by(|a, b| a.top().0.total_cmp(&b.top().0));
        assert_eq!(runs.len(), 2, "a quad each side of the ghost lines: {runs:?}");
        let rows = |quad: &Bounds<gpui::ScaledPixels>| quad.size.height.0 / line_height;
        assert!(
            (rows(&runs[0]) - 2.).abs() < 0.05 && (rows(&runs[1]) - 2.).abs() < 0.05,
            "\"a1\" and the cursor's \"a2\" one quad, \"a3\" and \"a4\" the other: {runs:?} at \
             {line_height} a row"
        );
        assert!(
            runs[0].bottom() < runs[1].top()
                && (runs[1].top().0 - runs[0].bottom().0 - ghost_lines * line_height).abs() < 0.05,
            "the quads lie the ghost lines' height, {ghost_lines} rows, apart: {runs:?}"
        );
    }

    #[test]
    fn test_decorations_override_syntax_highlights() {
        let syntax = vec![(
            0..8,
            HighlightStyle {
                color: Some(gpui::blue()),
                ..Default::default()
            },
        )];
        let decoration = HighlightStyle {
            color: Some(gpui::red()),
            font_style: Some(gpui::FontStyle::Italic),
            ..Default::default()
        };

        let styles = compose_decorations(syntax, [(2..6, decoration)], 0..8).unwrap();

        assert_eq!(styles[1].0, 2..6);
        assert_eq!(styles[1].1.color, Some(gpui::red()));
        assert_eq!(styles[1].1.font_style, Some(gpui::FontStyle::Italic));
    }

    #[test]
    fn test_editor_scrollbar_layout_uses_current_scroll_size() {
        let input_bounds = Bounds::new(point(px(10.), px(20.)), size(px(300.), px(80.)));
        let paddings = Edges {
            top: px(2.),
            right: px(3.),
            bottom: px(5.),
            left: px(7.),
        };

        let layout =
            EditorScrollbarLayout::new(input_bounds, px(40.), size(px(1000.), px(200.)), paddings);

        assert_eq!(
            layout.bounds,
            Bounds::new(point(px(47.), px(18.)), size(px(266.), px(87.)))
        );
        assert_eq!(layout.scroll_size, size(px(976.), px(200.)));

        let layout_without_gutter =
            EditorScrollbarLayout::new(input_bounds, px(0.), size(px(500.), px(120.)), paddings);

        assert_eq!(
            layout_without_gutter.bounds,
            Bounds::new(point(px(10.), px(18.)), size(px(303.), px(87.)))
        );
        assert_eq!(layout_without_gutter.scroll_size, size(px(513.), px(120.)));
    }

    #[test]
    fn test_editor_gutter_covers_the_complete_fixed_column() {
        let input_bounds = Bounds::new(point(px(10.), px(20.)), size(px(300.), px(80.)));

        assert_eq!(
            editor_gutter_bounds(
                input_bounds,
                px(48.),
                px(16.),
                Edges {
                    top: px(2.),
                    right: px(3.),
                    bottom: px(5.),
                    left: px(7.),
                },
            ),
            Bounds::new(point(px(3.), px(18.)), size(px(55.), px(103.)))
        );
    }

    #[test]
    fn test_auto_grow_scroll_offset_is_clamped_to_current_viewport() {
        let mode = LayoutMode::auto_grow(3, 8);

        assert_eq!(
            clamp_auto_grow_vertical_scroll_offset(&mode, px(-260.), px(340.), px(160.)),
            px(-180.)
        );
        assert_eq!(
            clamp_auto_grow_vertical_scroll_offset(&mode, px(-40.), px(340.), px(160.)),
            px(-40.)
        );
        assert_eq!(
            clamp_auto_grow_vertical_scroll_offset(&mode, px(20.), px(340.), px(160.)),
            px(0.)
        );

        let plain_text = LayoutMode::plain_text();
        assert_eq!(
            clamp_auto_grow_vertical_scroll_offset(&plain_text, px(-260.), px(340.), px(160.)),
            px(-260.)
        );
    }

    #[test]
    fn test_runs_for_range() {
        let run = TextRun {
            len: 0,
            font: gpui::font(".SystemUIFont"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        // use hello this-is-test
        let runs = vec![
            // use
            TextRun {
                len: 3,
                ..run.clone()
            },
            // \s
            TextRun {
                len: 1,
                ..run.clone()
            },
            // hello
            TextRun {
                len: 5,
                ..run.clone()
            },
            // \s
            TextRun {
                len: 1,
                ..run.clone()
            },
            // this-is-test
            TextRun {
                len: 12,
                ..run.clone()
            },
        ];

        #[track_caller]
        fn assert_runs(actual: Vec<TextRun>, expected: &[usize]) {
            let left = actual.iter().map(|run| run.len).collect::<Vec<_>>();
            assert_eq!(left, expected);
        }

        assert_runs(runs_for_range(&runs, 0, &(0..0)), &[]);
        assert_runs(runs_for_range(&runs, 0, &(0..100)), &[3, 1, 5, 1, 12]);

        assert_runs(runs_for_range(&runs, 0, &(0..6)), &[3, 1, 2]);
        assert_runs(runs_for_range(&runs, 0, &(1..6)), &[2, 1, 2]);
        assert_runs(runs_for_range(&runs, 0, &(3..10)), &[1, 5, 1]);
        assert_runs(runs_for_range(&runs, 0, &(5..8)), &[3]);
        assert_runs(runs_for_range(&runs, 3, &(0..3)), &[1, 2]);
        assert_runs(runs_for_range(&runs, 3, &(2..10)), &[4, 1, 3]);
        assert_runs(runs_for_range(&runs, 9, &(0..8)), &[1, 7]);
    }

    #[test]
    fn test_split_runs_preserve_ime_underline_across_highlight_boundaries() {
        let underline = UnderlineStyle {
            thickness: px(1.),
            color: Some(gpui::black()),
            wavy: false,
        };

        let runs = [0..4, 4..10]
            .into_iter()
            .flat_map(|range| {
                split_run_for_ime_underline(
                    TextStyle::default().to_run(range.len()),
                    range,
                    Some(2..7),
                    Some(underline),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            runs.iter()
                .map(|run| (run.len, run.underline.is_some()))
                .collect::<Vec<_>>(),
            vec![(2, false), (2, true), (3, true), (3, false)]
        );
    }

    #[test]
    fn test_split_run_applies_ime_underline_without_highlighting() {
        let underline = UnderlineStyle {
            thickness: px(1.),
            color: Some(gpui::black()),
            wavy: false,
        };

        let runs = split_run_for_ime_underline(
            TextStyle::default().to_run(10),
            0..10,
            Some(2..7),
            Some(underline),
        );

        assert_eq!(
            runs.iter()
                .map(|run| (run.len, run.underline.is_some()))
                .collect::<Vec<_>>(),
            vec![(2, false), (5, true), (3, false)]
        );
        assert!(
            split_run_for_ime_underline(TextStyle::default().to_run(0), 0..0, None, None)
                .is_empty()
        );
    }

    #[test]
    fn test_masked_ime_underline_splits_on_mask_char_boundaries() {
        let underline = UnderlineStyle {
            thickness: px(1.),
            color: Some(gpui::black()),
            wavy: false,
        };
        let text = Rope::from("abcdef");
        let mask_len = MASK_CHAR.len_utf8();

        assert_eq!(
            ime_marked_display_range(&text, Some(4..6), false),
            Some(4..6)
        );
        assert_eq!(ime_marked_display_range(&text, None, true), None);
        assert_eq!(
            ime_marked_display_range(&text, Some(4..6), true),
            Some(4 * mask_len..6 * mask_len)
        );

        let display_text = MASK_CHAR.to_string().repeat(text.chars().count());
        let runs = split_run_for_ime_underline(
            TextStyle::default().to_run(display_text.len()),
            0..display_text.len(),
            ime_marked_display_range(&text, Some(4..6), true),
            Some(underline),
        );

        let mut offset = 0;
        for run in &runs {
            assert!(display_text.is_char_boundary(offset));
            offset += run.len;
        }
        assert_eq!(offset, display_text.len());
    }

    #[test]
    fn test_placeholder_line_runs() {
        let run = TextRun {
            len: 0,
            font: gpui::font(".SystemUIFont"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let runs = vec![
            TextRun {
                len: 2,
                ..run.clone()
            },
            TextRun {
                len: 2,
                ..run.clone()
            },
            TextRun { len: 1, ..run },
        ];

        let placeholder_runs = placeholder_line_runs("ab\n\nc", &runs);

        let lines = placeholder_runs
            .iter()
            .map(|(line, _)| *line)
            .collect::<Vec<_>>();
        assert_eq!(lines, vec!["ab", "", "c"]);

        let run_lengths = placeholder_runs
            .iter()
            .map(|(_, line_runs)| line_runs.iter().map(|run| run.len).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(run_lengths, vec![vec![2], vec![], vec![1]]);
    }

    #[test]
    fn test_align_runs_to_char_boundaries() {
        let run = TextRun {
            len: 0,
            font: gpui::font(".SystemUIFont"),
            color: gpui::blue(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = |lens: &[usize]| {
            lens.iter()
                .map(|&len| TextRun { len, ..run.clone() })
                .collect::<Vec<_>>()
        };
        let lens = |runs: &[TextRun]| runs.iter().map(|run| run.len).collect::<Vec<_>>();

        // "你好，世界" is 15 bytes; boundaries at 0, 3, 6, 9, 12, 15.
        let text = "你好，世界";

        // Aligned runs pass through untouched.
        assert_eq!(align_runs_to_char_boundaries(text, &runs(&[3, 12])), None);

        // A mid-char boundary (4) snaps up to the next boundary (6).
        let aligned = align_runs_to_char_boundaries(text, &runs(&[4, 11])).unwrap();
        assert_eq!(lens(&aligned), vec![6, 9]);

        // Several boundaries inside the same char collapse into one run.
        let aligned = align_runs_to_char_boundaries(text, &runs(&[1, 1, 13])).unwrap();
        assert_eq!(lens(&aligned), vec![3, 12]);

        // Overshooting runs are capped at the text length.
        let aligned = align_runs_to_char_boundaries(text, &runs(&[3, 20])).unwrap();
        assert_eq!(lens(&aligned), vec![3, 12]);

        // Undershooting runs keep the shortfall (the tail is left unshaped,
        // matching `runs_for_range` clipping).
        assert_eq!(align_runs_to_char_boundaries(text, &runs(&[3, 3])), None);
    }

    #[test]
    fn test_clip_styles_to_ranges() {
        let style = HighlightStyle::default();
        let styles = vec![(0..4, style), (4..10, style), (12..20, style)];

        // Ranges with a gap (a folded region) drop the gap coverage and split
        // styles that span a range edge.
        let clipped = clip_styles_to_ranges(styles.clone(), &[2..6, 14..18]);
        assert_eq!(
            clipped
                .iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            vec![2..4, 4..6, 14..18]
        );

        // Styles fully inside the ranges are unchanged.
        let clipped = clip_styles_to_ranges(styles.clone(), &[0..20]);
        assert_eq!(
            clipped
                .iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            vec![0..4, 4..10, 12..20]
        );

        // A single style spanning several ranges splits into one piece per range.
        let clipped = clip_styles_to_ranges(vec![(0..30, style)], &[5..10, 20..25]);
        assert_eq!(
            clipped
                .iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            vec![5..10, 20..25]
        );

        assert!(clip_styles_to_ranges(styles, &[]).is_empty());
    }

    #[test]
    fn test_split_runs_by_bg_segments() {
        let run = TextRun {
            len: 0,
            font: gpui::font(".SystemUIFont"),
            color: gpui::blue(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let runs = vec![
            TextRun {
                len: 5,
                ..run.clone()
            },
            TextRun {
                len: 7,
                ..run.clone()
            },
            TextRun {
                len: 24,
                ..run.clone()
            },
        ];

        let bg_segments = vec![(8..12, gpui::red()), (12..18, gpui::blue())];
        let result = split_runs_by_bg_segments(5, &runs, &bg_segments);
        assert_eq!(
            result.iter().map(|run| run.len).collect::<Vec<_>>(),
            vec![3, 2, 2, 5, 1, 23]
        );
        assert_eq!(result[0].color, gpui::blue());
        assert_eq!(result[1].color, gpui::black());
        assert_eq!(result[2].color, gpui::black());
        assert_eq!(result[3].color, gpui::black());
        assert_eq!(result[4].color, gpui::black());
        assert_eq!(result[5].color, gpui::blue());
    }

    #[test]
    fn test_empty_bottom_height_outside_code_editor() {
        // Single-line / plain-text / auto-grow modes never reserve empty
        // bottom space, regardless of any override.
        for override_rows in [None, Some(0), Some(3), Some(99)] {
            assert_eq!(
                empty_bottom_height(false, override_rows, px(800.), px(20.)),
                px(0.),
            );
        }
    }

    #[test]
    fn test_empty_bottom_height_code_editor_default() {
        // `None`: roughly half the viewport, floored at
        // `BOTTOM_MARGIN_ROWS * line_height` so the empty area never
        // collapses to "less than a few lines" on tiny viewports.
        let line_height = px(20.);

        // Viewport much taller than the floor → half-viewport wins.
        assert_eq!(
            empty_bottom_height(true, None, px(800.), line_height),
            px(400.),
        );

        // Viewport shorter than 2 × floor → floor wins.
        let floor = BOTTOM_MARGIN_ROWS * line_height;
        assert_eq!(empty_bottom_height(true, None, px(40.), line_height), floor);
    }

    #[test]
    fn test_empty_bottom_height_explicit_row_count() {
        // `Some(n)`: exactly `n` line-heights. Caller fully controls
        // the trailing empty space; viewport size doesn't amplify it.
        let line_height = px(20.);

        for rows in [0_usize, 1, 3, 8, 64] {
            let expected = rows as f32 * line_height;
            assert_eq!(
                empty_bottom_height(true, Some(rows), px(800.), line_height),
                expected,
            );
            // Tiny viewport: still exactly `n × line_height`, no floor
            // applied when caller supplied an explicit count.
            assert_eq!(
                empty_bottom_height(true, Some(rows), px(20.), line_height),
                expected,
            );
        }
    }

    #[test]
    fn test_cursor_surrounding_padding_auto_grow() {
        // Auto-grow inputs always pad by one line, regardless of any
        // override or visible-lines count.
        let line_height = px(20.);
        for override_lines in [None, Some(0), Some(3), Some(99)] {
            for visible_lines in [0_usize, 1, 8, 64] {
                assert_eq!(
                    cursor_surrounding_padding(true, override_lines, visible_lines, line_height,),
                    line_height,
                );
            }
        }
    }

    #[test]
    fn test_cursor_surrounding_padding_default() {
        // `None`: historical heuristic — `BOTTOM_MARGIN_ROWS` for normal
        // viewports, falls back to one line on small viewports (less
        // than `BOTTOM_MARGIN_ROWS × 8` rows tall).
        let line_height = px(20.);

        // Small viewport → 1-line fallback.
        let small = BOTTOM_MARGIN_ROWS * 8 - 1;
        assert_eq!(
            cursor_surrounding_padding(false, None, small, line_height),
            line_height,
        );

        // Boundary at `BOTTOM_MARGIN_ROWS × 8` flips to the full margin.
        let boundary = BOTTOM_MARGIN_ROWS * 8;
        assert_eq!(
            cursor_surrounding_padding(false, None, boundary, line_height),
            BOTTOM_MARGIN_ROWS * line_height,
        );

        // Comfortably-large viewport.
        assert_eq!(
            cursor_surrounding_padding(false, None, 100, line_height),
            BOTTOM_MARGIN_ROWS * line_height,
        );
    }

    #[test]
    fn test_cursor_surrounding_padding_explicit() {
        // `Some(n)`: exactly `n × line_height` when the viewport has
        // room for it; saturated against half the viewport when it
        // doesn't.
        let line_height = px(20.);

        for lines in [0_usize, 1, 2, 5, 50] {
            let raw = lines as f32 * line_height;
            for visible_lines in [0_usize, 1, 8, 100] {
                let viewport_half = (visible_lines as f32 * line_height).half();
                assert_eq!(
                    cursor_surrounding_padding(false, Some(lines), visible_lines, line_height,),
                    raw.min(viewport_half),
                );
            }
        }
    }

    #[test]
    fn test_cursor_surrounding_padding_saturates_against_viewport() {
        // An aggressive override on a small viewport must not produce a
        // padding larger than half the visible region — otherwise the
        // bottom-edge auto-scroll-into-view threshold sinks below the
        // top-edge threshold and the per-frame scroll adjustment loses
        // a stable fixed point.
        let line_height = px(20.);

        // Override much larger than viewport → clamped to half.
        let visible_lines = 10;
        let viewport_half = (visible_lines as f32 * line_height).half();
        assert_eq!(
            cursor_surrounding_padding(false, Some(50), visible_lines, line_height),
            viewport_half,
        );

        // Override that fits → returned unchanged.
        let visible_lines = 40;
        assert_eq!(
            cursor_surrounding_padding(false, Some(3), visible_lines, line_height),
            3.0 * line_height,
        );

        // Default heuristic still saturates if BOTTOM_MARGIN_ROWS would
        // exceed the half-viewport bound (only possible at extreme
        // sizes — kept for defensive completeness).
        let visible_lines = BOTTOM_MARGIN_ROWS * 8;
        let half = (visible_lines as f32 * line_height).half();
        let raw = BOTTOM_MARGIN_ROWS * line_height;
        assert_eq!(
            cursor_surrounding_padding(false, None, visible_lines, line_height),
            raw.min(half),
        );
    }
}
