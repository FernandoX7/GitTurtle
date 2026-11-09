//! Retained, bounded presentation of an already-loaded commit message.
//!
//! This is inspection state, moved with ReturnContext (including warm tabs),
//! not a second message store or a repository reader. Only a newly selected
//! immutable OID prepares text pieces; paints clone shared bounded pieces.
//! The pieces and the metadata after them form one selectable document
//! ([`selection`]).
mod selection;

pub(crate) use selection::Selection;

use crate::views::{LineKey, cached_width, compact_control_name, line_widths};
use crate::*;
use gitturtle_core::FileHistoryEntry;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::prelude::FluentBuilder;
use std::cell::{Cell, RefCell};
use unicode_segmentation::UnicodeSegmentation;

const PIECE_BYTES: usize = 1024;
/// The most of the inspector's height History's and Compare's message panel
/// takes.
const MESSAGE_SHARE: f32 = 0.45;
const PIECE_LINES: usize = 16;

struct Piece {
    /// Byte offset of `text` in [`full_message`].
    start: usize,
    text: SharedString,
    title: bool,
    body_start: bool,
}

struct Presentation {
    oid: String,
    pieces: Arc<Vec<Piece>>,
    scroll: ListState,
    /// What reveals a parent's control, in the metadata the list's last row
    /// holds, as the keyboard reaches it ([`State::parent_reveal`]).
    reveal: crate::focus_reveal::FocusReveal,
    focus: FocusHandle,
    selection: Selection,
    scale: f32,
    /// The message and metadata's height as last measured
    /// (`GitTurtle::message_extent`), with the width, the bound, the font
    /// and the display scale it was measured for.
    extent: Option<Extent>,
}

/// A measured height of a presented message and its metadata.
///
/// Layout measures it on the display's device pixels, each line of text
/// rounded up to one and each padding on its whole one, so it is kept by
/// the display's scale factor too: kept by the width, the bound, the font
/// and the details alone, a window moved to a display of another scale at
/// the same logical width drew its panel from the other display's height.
struct Extent {
    width: Pixels,
    bound: Pixels,
    key: LineKey,
    /// The display's scale factor, as bits.
    scale: u32,
    details: Details,
    height: Pixels,
}

/// The metadata a measured [`Extent`] holds after the message: the
/// commit's own, as History and Compare show it, or a File History
/// revision's, its file's change against its first parent. File History
/// presents the same commit as History does, so one commit's extent once
/// measured in History would otherwise stand for its revision's details.
#[derive(PartialEq)]
enum Details {
    Commit,
    Revision(FileChange, Option<String>),
}

impl Details {
    fn of(revision: Option<&FileHistoryEntry>) -> Self {
        revision.map_or(Self::Commit, |entry| {
            Self::Revision(entry.change.clone(), entry.parent_oid.clone())
        })
    }

    /// The bytes the revision's paths, object IDs and parent hold.
    fn retained_bytes(&self) -> usize {
        let Self::Revision(change, parent) = self else {
            return 0;
        };
        let path = |path: &Option<std::path::PathBuf>| {
            path.as_ref().map_or(0, |path| path.as_os_str().len())
        };
        let text = |text: &Option<String>| text.as_ref().map_or(0, String::capacity);
        path(&change.old_path)
            + path(&change.new_path)
            + text(&change.old_oid)
            + text(&change.new_oid)
            + change.old_mode.capacity()
            + change.new_mode.capacity()
            + text(parent)
    }

    /// Whether these are `revision`'s details, or the commit's own where
    /// there is none; comparing allocates nothing.
    fn describe(&self, revision: Option<&FileHistoryEntry>) -> bool {
        match (self, revision) {
            (Self::Commit, None) => true,
            (Self::Revision(change, parent), Some(entry)) => {
                *change == entry.change && *parent == entry.parent_oid
            }
            _ => false,
        }
    }
}

/// The presented message, the toolbar's last layout and the message's one
/// focus handle, which every presentation of this inspection tracks, so a
/// reply that rebuilds the presentation, as another commit selected does,
/// leaves focus and its ring on the text and Tab moves on from it.
#[derive(Default)]
pub(super) struct State(
    RefCell<Option<Presentation>>,
    Cell<Option<Toolbar>>,
    std::cell::OnceCell<FocusHandle>,
    /// The focus handles of the parents' controls, one for each parent
    /// drawn so far, at most the 128 the metadata lists.
    RefCell<Vec<FocusHandle>>,
);

/// How the message toolbar lays out Copy message, Hash and, in History and
/// Compare, the selected commit's Actions in an inspector the window draws
/// `width` wide, and the height of the rows they take, which the message
/// panel reserves where the window pins the inspector, so the toolbar never
/// runs into the files below. Actions stands at the trailing edge of the copy
/// controls' last row, outside the scrolling message and metadata, so it
/// stays in view however far they scroll and they keep its row for their own
/// lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Toolbar {
    /// File History's toolbar, which has no Actions.
    plain: Form,
    /// History's and Compare's, with the commit's Actions.
    actions: Form,
    /// Whether the window pins the inspector, so the panel reserves the rows.
    /// An inspector at least its tight minimum wide keeps Copy message and
    /// Hash labelled and reserves nothing, as before.
    reserve: bool,
    /// The width inside the inspector's left border and the `px_3` that the
    /// toolbar and the metadata below it share: the room a line or a
    /// control of the metadata has, such as a parent's control.
    room: Pixels,
    /// The width inside the inspector's left border: the message's viewport
    /// and the width its pieces and metadata are laid out at.
    viewport: Pixels,
    /// The most the message panel takes, its share of the inspector's
    /// height, past which its message and metadata are not measured
    /// (`GitTurtle::message_extent`).
    cap: Pixels,
}

/// One layout of the toolbar's controls (DESIGN.md, "One responsive order":
/// actions become icons before the content gives way). Where the labelled
/// Copy message does not fit the row, Copy message and Hash keep only their
/// icons and share one row. Actions keeps its label and caret where they fit
/// on the copy controls' last row, otherwise only its more-actions icon there;
/// where not even that fits beside the labelled copy controls, they keep only
/// their icons too, so the three share one row rather than Actions taking a
/// row of the message's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Form {
    /// Copy message and Hash keep only their icons.
    compact: bool,
    /// Actions keeps its label and caret.
    labelled_actions: bool,
    /// The rows' height, with the panel's bottom border.
    height: Pixels,
}

thread_local! {
    static COPY_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static HASH_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static COPIED_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static ACTIONS_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
}

impl Toolbar {
    /// The toolbar of an inspector `width` wide, measured from the interface
    /// font at the interface size: the inspector's 1 px left border and the
    /// toolbar's `px_3`, then the shared copy buttons a quarter rem apart,
    /// each with 10 px of padding on either side of the wider of its content
    /// and the copied content, a 16 px check icon 6 px before "Copied", which
    /// `copy_feedback::copy_button` reserves. Copy message's content is a
    /// 16 px icon 6 px before its label, and Hash's the same icon before its
    /// label, narrower than the copied content; compact, each is a 28 px
    /// square of only the icon. Actions labelled is the helper's padding
    /// either side of its label, its 6 px gap and the caret, 0.875 rem
    /// square; otherwise a 28 px square. The controls wrap as the toolbar's
    /// `flex_wrap` wraps them, in rows a quarter rem apart inside its `py_1`,
    /// and the panel's 1 px bottom border is reserved with them. `tall`: the
    /// inspector's. `pinned`: the window pins the inspector.
    pub(super) fn within(width: Pixels, tall: Pixels, pinned: bool, window: &Window) -> Self {
        let key = LineKey::current(window);
        let label = |cache, text| {
            cached_width(cache, &key, || {
                line_widths(&[(text, appearance::ui_text(12.))], window)
            })
        };
        let copy_label = label(&COPY_LABEL, "Copy message");
        let hash_label = label(&HASH_LABEL, "Hash");
        let icon = appearance::ui_size(16. + 6.);
        let copied = icon + label(&COPIED_LABEL, copy_feedback::COPIED);
        let rem = window.rem_size();
        // The inspector and each control as the layout draws them, on whole
        // device pixels: the inspector's edges at the nearest, a control's
        // width rounded up. At 18 pt in a 455.4 px inspector, Copy message,
        // Hash and Actions fitted the row by 0.05 px unrounded, and Actions
        // wrapped below Hash once Copy message and Hash were drawn 178.5 and
        // 118.5 px wide.
        let scale = window.scale_factor();
        let drawn = |width: Pixels| px((f32::from(width) * scale).ceil() / scale);
        let width = px((f32::from(width) * scale).round() / scale);
        let room = width - px(1.) - rem * 1.5;
        let gap = rem * 0.25;
        let padding = appearance::ui_size(2. * 10.);
        let square = appearance::ui_size(28.);
        let copy = drawn(padding + (icon + copy_label).max(copied));
        let hash = drawn(padding + (icon + hash_label).max(copied));
        let actions = drawn(
            (padding + label(&ACTIONS_LABEL, "Actions") + appearance::ui_size(6.) + rem * 0.875)
                .ceil(),
        );
        // The rows `controls` wrap to.
        let rows = |controls: &[Pixels]| {
            let mut rows = 1.;
            let mut line = controls[0];
            for &control in &controls[1..] {
                if line + gap + control <= room {
                    line += gap + control;
                } else {
                    rows += 1.;
                    line = control;
                }
            }
            rows
        };
        let height = |rows: f32| square.ceil() * rows + gap * (rows - 1.) + rem * 0.5 + px(1.);
        let compact = copy > room;
        let (copy, hash) = if compact {
            (square, square)
        } else {
            (copy, hash)
        };
        let copy_rows = rows(&[copy, hash]);
        let plain = Form {
            compact,
            labelled_actions: false,
            height: height(copy_rows),
        };
        let actions = if rows(&[copy, hash, actions]) == copy_rows {
            Form {
                labelled_actions: true,
                ..plain
            }
        } else if rows(&[copy, hash, square]) == copy_rows {
            plain
        } else {
            Form {
                compact: true,
                labelled_actions: false,
                height: height(rows(&[square, square, square])),
            }
        };
        Self {
            plain,
            actions,
            reserve: pinned,
            room,
            viewport: width - px(1.),
            cap: tall * MESSAGE_SHARE,
        }
    }

    /// The form File History's toolbar takes, without Actions, or History's
    /// and Compare's, with it.
    fn form(&self, file_history: bool) -> Form {
        if file_history {
            self.plain
        } else {
            self.actions
        }
    }
}

impl State {
    /// The toolbar of the inspector the window draws next; `None`, as where
    /// no window lays the inspector out, keeps every control labelled and
    /// reserves nothing. The window sets it on every repository draw, before
    /// the inspector, so a state moved with an inspection carries nothing
    /// stale.
    pub(super) fn lay_out_toolbar(&self, toolbar: Option<Toolbar>) {
        self.1.set(toolbar);
    }

    /// The room a line or a control of the metadata has in the inspector the
    /// window draws next (`Toolbar::room`); `None` where no window lays the
    /// inspector out.
    pub(super) fn details_room(&self) -> Option<Pixels> {
        self.1.get().map(|toolbar| toolbar.room)
    }

    /// The least height File History's message panel keeps in the inspector
    /// the window draws next: its toolbar and the message's first line
    /// (`GitTurtle::render_commit_message`); `None` where no window lays
    /// the inspector out.
    pub(super) fn revision_message_least(&self) -> Option<Pixels> {
        self.1
            .get()
            .map(|toolbar| toolbar.plain.height + first_line())
    }

    /// The height the presented message and metadata were last measured at
    /// for `width`, `bound` and `key` on a display of `scale`, with
    /// `revision`'s details or the commit's own
    /// (`GitTurtle::message_extent`).
    fn extent(
        &self,
        width: Pixels,
        bound: Pixels,
        key: &LineKey,
        scale: f32,
        revision: Option<&FileHistoryEntry>,
    ) -> Option<Pixels> {
        let state = self.0.borrow();
        let extent = state.as_ref()?.extent.as_ref()?;
        (extent.width == width
            && extent.bound == bound
            && extent.key == *key
            && extent.scale == scale.to_bits()
            && extent.details.describe(revision))
        .then_some(extent.height)
    }

    fn remember_extent(
        &self,
        width: Pixels,
        bound: Pixels,
        key: LineKey,
        scale: f32,
        revision: Option<&FileHistoryEntry>,
        height: Pixels,
    ) {
        if let Some(state) = self.0.borrow_mut().as_mut() {
            state.extent = Some(Extent {
                width,
                bound,
                key,
                scale: scale.to_bits(),
                details: Details::of(revision),
                height,
            });
        }
    }

    /// Drops a presentation of another OID, ending its text selection at once.
    pub(super) fn select(&self, oid: &str, cx: &mut App) {
        let mut state = self.0.borrow_mut();
        if state.as_ref().is_some_and(|state| state.oid != oid)
            && let Some(previous) = state.take()
        {
            previous.selection.detach(cx);
        }
    }

    /// Moves the presentation out, as into a return context or warm tab: its
    /// text selection ends and its painted runs are released.
    pub(super) fn take(&mut self, cx: &mut App) -> Self {
        self.detach(cx);
        std::mem::take(self)
    }

    /// Replaces the presentation, ending the text selection of the one it had.
    pub(super) fn replace(&mut self, next: Self, cx: &mut App) {
        self.detach(cx);
        *self = next;
    }

    fn detach(&self, cx: &mut App) {
        let selection = self
            .0
            .borrow()
            .as_ref()
            .map(|state| state.selection.clone());
        if let Some(selection) = selection {
            selection.detach(cx);
        }
    }

    /// The presented message's focus handle, which its text tracks.
    #[cfg(test)]
    pub(crate) fn focus(&self) -> Option<FocusHandle> {
        self.0.borrow().as_ref().map(|state| state.focus.clone())
    }

    /// What scrolls a parent's control of the presented commit into the
    /// message's view, whole with its ring's room, as the keyboard reaches
    /// it (DESIGN.md: whole focus). Kept with the presentation, so each
    /// commit's metadata scrolls for itself.
    pub(super) fn parent_reveal(&self) -> Option<crate::focus_reveal::FocusReveal> {
        self.0.borrow().as_ref().map(|state| state.reveal.clone())
    }

    /// The focus handle the control of parent `index` tracks, the same one
    /// for every commit the inspector shows, as the message's own focus is,
    /// so focus on a parent's control survives the presentation's being
    /// replaced.
    pub(super) fn parent_focus(&self, index: usize, cx: &mut App) -> FocusHandle {
        let mut parents = self.3.borrow_mut();
        while parents.len() <= index {
            parents.push(cx.focus_handle());
        }
        parents[index].clone()
    }

    /// The presented message and metadata's height as last measured
    /// (`GitTurtle::message_extent`).
    #[cfg(test)]
    pub(crate) fn measured(&self) -> Option<Pixels> {
        let state = self.0.borrow();
        state.as_ref()?.extent.as_ref().map(|extent| extent.height)
    }

    /// How far the presented message and metadata run past their viewport,
    /// as the list last measured them: zero where they fit, so the scrollbar
    /// draws nothing.
    #[cfg(test)]
    pub(crate) fn overflow(&self) -> Option<Pixels> {
        self.0
            .borrow()
            .as_ref()
            .map(|state| state.scroll.max_offset_for_scrollbar().y)
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.0.borrow().as_ref().map_or(0, |state| {
            state.oid.capacity()
                + state.pieces.iter().map(|piece| piece.text.len()).sum::<usize>()
                // Reserve list tree/measurement bookkeeping as well as pieces.
                + state.pieces.capacity() * (std::mem::size_of::<Piece>() + 256)
                + state.selection.retained_bytes()
                + state
                    .extent
                    .as_ref()
                    .map_or(0, |extent| extent.details.retained_bytes())
        })
    }

    /// Copies the presented message's selected text, byte for byte, and
    /// reports whether there was any.
    pub(super) fn copy_selection(&self, cx: &mut App) -> bool {
        let selection = self
            .0
            .borrow()
            .as_ref()
            .map(|state| state.selection.clone());
        selection.is_some_and(|selection| selection.copy(cx))
    }

    fn presentation(
        &self,
        commit: &Commit,
        cx: &mut App,
    ) -> (Arc<Vec<Piece>>, ListState, FocusHandle, Selection) {
        let mut retained = self.0.borrow_mut();
        let scale = appearance::ui_scale();
        if retained
            .as_ref()
            .is_none_or(|state| state.oid != commit.oid)
        {
            if let Some(previous) = retained.take() {
                previous.selection.detach(cx);
            }
            let mut pieces = Vec::new();
            append_pieces(&mut pieces, &commit.subject, true, 0);
            let body = if commit.subject.is_empty() {
                0
            } else {
                commit.subject.len() + 2
            };
            append_pieces(&mut pieces, &commit.body, false, body);
            // The last piece always keeps its text's end.
            let message_len = pieces
                .last()
                .map_or(0, |piece| piece.start + piece.text.len());
            let pieces = Arc::new(pieces);
            // The text is a Tab stop: GPUI ignores the `tab_stop` of the div
            // that tracks this handle, so the handle carries it.
            let focus = self
                .2
                .get_or_init(|| cx.focus_handle().tab_stop(true))
                .clone();
            let scroll = ListState::new(pieces.len() + 1, ListAlignment::Top, px(100.))
                .with_uniform_item_height(appearance::ui_size(24.));
            *retained = Some(Presentation {
                oid: commit.oid.clone(),
                reveal: crate::focus_reveal::FocusReveal::for_list(
                    scroll.clone(),
                    crate::focus_reveal::Trigger::Keyboard,
                ),
                scroll,
                selection: Selection::new(pieces.clone(), message_len, focus.clone(), cx),
                pieces,
                focus,
                scale,
                extent: None,
            });
        }
        let state = retained.as_mut().unwrap();
        if state.scale != scale {
            state.scroll.remeasure();
            state.scale = scale;
        }
        (
            state.pieces.clone(),
            state.scroll.clone(),
            state.focus.clone(),
            state.selection.clone(),
        )
    }
}

// Prefer source line boundaries, then whitespace, then grapheme boundaries.
// Consecutive pieces continue a paragraph without extra vertical padding.
// A single maliciously large grapheme can itself exceed the shaping budget;
// split that exceptional cluster at UTF-8 boundaries. Literal copy never uses
// this display segmentation and remains byte-for-byte the loaded model: each
// piece records its offset in the full message, and copying a selection puts
// back the newlines no piece displays. `base` is `text`'s offset there.
fn append_pieces(pieces: &mut Vec<Piece>, mut text: &str, title: bool, base: usize) {
    let mut first = true;
    let mut start = base;
    while !text.is_empty() {
        let limit = text.floor_char_boundary(PIECE_BYTES.min(text.len()));
        let prefix = &text[..limit];
        let line_end = prefix
            .match_indices('\n')
            .take(PIECE_LINES)
            .last()
            .map(|(ix, _)| ix + 1);
        let end = if limit == text.len()
            && prefix.bytes().filter(|byte| *byte == b'\n').count() < PIECE_LINES
        {
            limit
        } else if let Some(end) = line_end {
            end
        } else {
            let preferred = prefix
                .char_indices()
                .rev()
                .find(|(_, ch)| ch.is_whitespace())
                .map(|(ix, ch)| ix + ch.len_utf8())
                .filter(|end| *end >= limit / 2)
                .unwrap_or(limit);
            prefix
                .grapheme_indices(true)
                .take_while(|(ix, _)| *ix <= preferred)
                .map(|(ix, _)| ix)
                .last()
                .filter(|end| *end > 0)
                .unwrap_or(limit)
        };
        let piece = &text[..end];
        // The following row supplies this line break; leaving it on both rows
        // would add an empty visual line at every virtualization boundary.
        let display = if end < text.len() {
            piece.strip_suffix('\n').unwrap_or(piece)
        } else {
            piece
        };
        pieces.push(Piece {
            start,
            text: display.to_owned().into(),
            title,
            body_start: !title && first,
        });
        first = false;
        start += end;
        text = &text[end..];
    }
}

pub(super) fn full_message(commit: &Commit) -> String {
    if commit.body.is_empty() {
        commit.subject.clone()
    } else if commit.subject.is_empty() {
        commit.body.clone()
    } else {
        format!("{}\n\n{}", commit.subject, commit.body)
    }
}

/// Single-line navigation labels never shape or announce a megabyte title.
/// The inspector and explicit message copy remain complete.
pub(super) fn summary(text: &str) -> String {
    let end = text.floor_char_boundary(512.min(text.len()));
    if end == text.len() {
        text.to_owned()
    } else {
        let end = text[..end]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index);
        format!("{}…", &text[..end])
    }
}

/// The room a message piece keeps clear at its right for the viewport's
/// scrollbar: the bar's own width, which is the same at every interface
/// size, and a scaled 4 px between it and the text, 20 px at the default
/// size. A scaled 20 px gave the bar 11.7 px more than it takes at 18 pt,
/// and in a 461 × 490 window broke the title "Add polyglot.png" before the
/// file name's last letter.
fn scrollbar_reserve() -> Pixels {
    Scrollbar::width() + appearance::ui_size(4.)
}

/// A piece of the inspector's message: static text read by its text, whose
/// node reports the part the message's viewport draws, where the title ran
/// past the viewport's foot at 18 pt in a 461 px window, over the Changed
/// files heading below. Its text keeps clear of the viewport's scrollbar
/// ([`scrollbar_reserve`]).
/// The message's first line as [`render_piece`] lays it out, the title's,
/// with the half rem above it, rounded up to a whole pixel: the least
/// height File History's message viewport keeps.
fn first_line() -> Pixels {
    let line = (appearance::ui_text(15.) * 1.35).max(appearance::ui_size(20.25));
    // The rem is the interface's 13 pt (`appearance::apply_text_sizes`).
    (appearance::ui_size(13. * 0.5) + line).ceil()
}

fn render_piece(piece: &Piece, index: usize, selection: &Selection, cx: &App) -> impl IntoElement {
    let colors = palette(cx);
    crate::native_accessibility::static_text(
        div()
            .id(("commit-message-piece", index))
            .debug_selector(move || format!("commit-message-piece-{index}")),
        piece.text.clone(),
    )
    .w_full()
    .min_w_0()
    .px_3()
    .pr(scrollbar_reserve())
    .when(index == 0, |row| row.pt_2())
    .when(piece.body_start && index > 0, |row| row.pt_3())
    .text_color(rgb(colors.text))
    .text_size(appearance::ui_text(if piece.title { 15. } else { 12. }))
    .line_height(relative(if piece.title { 1.35 } else { 1.5 }))
    .when(piece.title, |row| row.font_weight(FontWeight::MEDIUM))
    .min_h(appearance::ui_size(if piece.title { 20.25 } else { 18. }))
    .child(selection.piece(piece))
}

/// The message viewport's overflow cue, built as the frame paints, after
/// the list has laid out in it (`in_frame`), as a scrolling list's is
/// (`tags::ScrollCue::list`): along a track a thumb moves on
/// ([`crate::tags::SHORTEST_SCROLL_TRACK`]) the always-visible scrollbar,
/// which draws nothing where the message fits; along a shorter track a
/// soft fade at each edge the message continues past
/// ([`message_fades`]); and in a sliver shorter than one fade, nothing. An
/// inspector whose 45% cap lies just past the toolbar row it reserves
/// leaves the viewport that sliver, about 112 px tall at 18 pt, and the
/// bar's track, in the canvas colour, drew a 16 × 2 px strip on the
/// panel's bottom border; somewhat taller, up to 233 px at 18 pt, it
/// leaves the viewport 4 to 55 px, where the kit's 48 px thumb cannot
/// move. Built from the list's bounds in this frame, the cue follows a
/// resize or a cap that shrinks the viewport in the frame that shrinks it.
/// Tests find the bar, while it is drawn, by its id.
fn message_cue(scroll: ListState, cx: &App) -> impl IntoElement {
    let surface: Hsla = rgb(palette(cx).panel).into();
    div().absolute().size_full().flex().flex_col().child(
        crate::in_frame::in_frame(move |_, _, _| {
            if scroll.viewport_bounds().size.height < crate::tags::SHORTEST_SCROLL_TRACK {
                return message_fades(scroll, surface).into_any_element();
            }
            crate::in_frame::probed(
                div()
                    .debug_selector(|| "commit-message-scrollbar".into())
                    .size_full()
                    .child(
                        Scrollbar::vertical(&scroll)
                            .id("commit-message-scrollbar")
                            .mode(ScrollbarMode::Always),
                    ),
                "scrollbar",
            )
            .into_any_element()
        })
        .named("commit-message-scrollbar"),
    )
}

/// The fades over a message viewport too short for a thumb that moves
/// ([`message_cue`]): at each edge the message continues past, a band as
/// wide as the viewport and as tall as a scrolling list's fade
/// ([`crate::tags::EDGE_FADE`]), at most a quarter of the viewport,
/// opaque at the edge in the inspector's panel colour and clear inward, so
/// the cut lines dissolve into the surface; none at an end the message is
/// scrolled to, none where it fits, and none in a viewport shorter than
/// one fade, where a band would be a pixel or two. It reads the list's
/// offset and reach as it paints, after the list clamped them in the same
/// frame, so a wheel or a resize moves the fades in the frame that scrolls
/// it, and it inserts no hitbox, so a wheel over a fade scrolls the
/// message.
fn message_fades(scroll: ListState, surface: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let list = scroll.viewport_bounds();
            let full = appearance::ui_size(crate::tags::EDGE_FADE);
            if list.size.height < full {
                return;
            }
            let fade = full.min(list.size.height / 4.);
            // Half a pixel either way is an end, as a scrolling list's.
            let slack = px(0.5);
            let scrolled = -scroll.scroll_px_offset_for_scrollbar().y;
            let band = |angle: f32| {
                linear_gradient(
                    angle,
                    linear_color_stop(surface.opacity(0.), 0.),
                    linear_color_stop(surface, 1.),
                )
            };
            if scrolled > slack {
                // Toward the top, opaque at the viewport's top edge.
                let top = Bounds::new(list.origin, size(list.size.width, fade));
                window.paint_quad(fill(top, band(0.)));
            }
            if scrolled < scroll.max_offset_for_scrollbar().y - slack {
                // Toward the bottom, opaque at the viewport's bottom edge.
                let bottom = Bounds::new(
                    point(list.left(), list.bottom() - fade),
                    size(list.size.width, fade),
                );
                window.paint_quad(fill(bottom, band(180.)));
            }
        },
    )
    .absolute()
    .size_full()
}

impl GitTurtle {
    fn message_commit(&self, file_history: bool) -> Option<&Commit> {
        if file_history {
            self.file_history_entry().map(|entry| &entry.commit)
        } else if !self.file_history.is_active() && !self.revision_inspection.is_active() {
            self.selected_commit
                .and_then(|index| self.commits.get(index))
        } else {
            None
        }
    }

    /// The height the selected commit's message and metadata take in the
    /// inspector the window draws next, laid out at its width as the list
    /// lays them out, and one inset after the last row, as wide as the
    /// metadata's inset above its first: the height the message's viewport
    /// takes where they fit, so Changed files follows the last parent at the
    /// inspector's rhythm rather than below the rest of a fixed 45%, which
    /// left about 60 px blank under P1 at 13 pt. File History's panel, which
    /// holds `revision`'s details after the message rather than the commit's,
    /// shrinks to them alike, so its revisions follow their paths at that
    /// rhythm. Measured once for each commit, details, width, font and
    /// display scale (`Extent`), and only as far as the panel's cap, past
    /// which nothing more shows, so a long message shapes only what can be
    /// drawn: in File History, whose panel takes its share of the room
    /// between the header and the footer, as far as the inspector's share,
    /// which holds it. `None` where no window lays the inspector out.
    pub(super) fn message_extent(
        &self,
        commit: &Commit,
        revision: Option<&FileHistoryEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Pixels> {
        let toolbar = self.inspector_message.1.get()?;
        let (width, bound) = (toolbar.viewport, toolbar.cap);
        let (key, scale) = (LineKey::current(window), window.scale_factor());
        let (pieces, _, _, selection) = self.inspector_message.presentation(commit, cx);
        if let Some(height) = self
            .inspector_message
            .extent(width, bound, &key, scale, revision)
        {
            return Some(height);
        }
        let mut height = px(0.);
        for (index, piece) in pieces.iter().enumerate() {
            if height > bound {
                break;
            }
            let row = render_piece(piece, index, &selection, cx).into_any_element();
            height += crate::in_frame::measured_height(row, width, window, cx);
        }
        if height <= bound {
            let metadata = match revision {
                Some(entry) => self.render_revision_metadata(entry, &selection, cx),
                None => self.render_commit_metadata(commit, &selection, window, cx),
            };
            height += crate::in_frame::measured_height(metadata, width, window, cx);
        }
        let height = height + window.rem_size() * 0.75;
        self.inspector_message
            .remember_extent(width, bound, key, scale, revision, height);
        Some(height)
    }

    /// The selected commit's message panel; `extent`, where the window lays
    /// the inspector out, is the height its message and metadata take
    /// (`message_extent`), to which the panel shrinks below its cap.
    pub(super) fn render_commit_message(
        &self,
        commit: &Commit,
        file_history: bool,
        extent: Option<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        // File History has no Actions; History and Compare lay out its form.
        let toolbar = self.inspector_message.1.get();
        let form = toolbar.map(|toolbar| toolbar.form(file_history));
        let compact = form.is_some_and(|form| form.compact);
        let reserved = toolbar
            .filter(|toolbar| toolbar.reserve)
            .and(form)
            .map(|form| form.height);
        // File History's panel keeps its toolbar and the message's first
        // line whole (DESIGN.md's responsive order: content stays whole
        // first), whatever its share of a short panel leaves: at 18 pt in a
        // 461 × 490 window that share left the viewport 15 px below the
        // toolbar, drawn empty.
        let reserved = match form.filter(|_| file_history) {
            Some(form) => Some(reserved.unwrap_or_default().max(form.height + first_line())),
            None => reserved,
        };
        let actions = (!file_history).then(|| {
            self.render_commit_recovery_menu(
                commit,
                form.is_none_or(|form| form.labelled_actions),
                cx,
            )
        });
        let (pieces, scroll, focus, selection) = self.inspector_message.presentation(commit, cx);
        // A parent's control the keyboard has just moved onto scrolls into
        // the list's view in this frame, before the list lays out, its
        // state being out of reach while it does.
        let reveal = self.inspector_message.parent_reveal();
        if let Some(reveal) = &reveal {
            reveal.reveal(window, cx);
        }
        let room = appearance::button_ring_room(cx);
        let owner = cx.entity().downgrade();
        let oid = commit.oid.clone();
        let rows = selection.clone();
        let list = gpui_kit::list(scroll.clone(), move |index, window, cx| {
            if let Some(piece) = pieces.get(index) {
                return render_piece(piece, index, &rows, cx).into_any_element();
            }
            owner
                .update(cx, |this, cx| {
                    if this
                        .message_commit(file_history)
                        .is_none_or(|commit| commit.oid != oid)
                    {
                        return div().into_any_element();
                    }
                    if file_history {
                        this.render_revision_metadata(this.file_history_entry().unwrap(), &rows, cx)
                    } else {
                        this.render_commit_metadata(
                            this.message_commit(false).unwrap(),
                            &rows,
                            window,
                            cx,
                        )
                    }
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full()
        // The metadata ends at its last row, so scrolled to its end the
        // last parent's control would sit on the list's edge with its ring
        // cut: the room the reveal keeps below it, which a message that fits
        // never reaches, the metadata keeping its inset below that row.
        .when(!file_history, |list| list.pb(room));
        let copy_selection = selection.clone();
        let select_all = selection.clone();
        let copy_oid = commit.oid.clone();
        let hash = commit.oid.clone();
        // Keyed by the commit, so another commit's buttons, drawn with the
        // same ids in the same place, are at rest.
        let message_mark = copy_feedback::Mark::new("copy-message", &commit.oid);
        let hash_mark = copy_feedback::Mark::new("copy-commit", &commit.oid);
        let message_copy = message_mark.clone();
        let hash_copy = hash_mark.clone();
        let key_scroll = scroll.clone();
        let key_owner = cx.entity().downgrade();
        let click_focus = focus.clone();
        div()
            .id("commit-message-panel")
            .debug_selector(|| "commit-message-panel".into())
            .role(Role::Group)
            .aria_label("Selected commit message and details")
            .min_w_0()
            // As tall as the message and metadata where they fit, at most
            // 45% of the inspector; their viewport gives way below.
            .when(extent.is_none(), |panel| panel.h(relative(MESSAGE_SHARE)))
            .max_h(relative(MESSAGE_SHARE))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(rgb(colors.border))
            .when_some(reserved, |panel, height| panel.min_h(height))
            .child(div()
                .id("commit-message-toolbar")
                .debug_selector(|| "commit-message-toolbar".into())
                .flex_shrink_0()
                .px_3().py_1()
                .flex().flex_wrap().items_center().gap_1()
                .child(if compact {
                    compact_control_name(copy_feedback::copy_button(message_mark, "", "Copy message · Copy the complete commit title and body", cx), "copy-message", "Copy full commit message")
                } else {
                    copy_feedback::copy_button(message_mark, "Copy message", "Copy the complete commit title and body", cx)
                        .accessibility_label("Copy full commit message")
                }
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let message = this.message_commit(file_history).filter(|commit| commit.oid == copy_oid).map(full_message);
                        if let Some(message) = message {
                            copy_feedback::copy(window, cx, Some(message_copy.clone()), "commit message", message);
                        }
                    })))
                // Compact beside the compact Copy message, so the two keep
                // one row.
                .child(if compact {
                    compact_control_name(copy_feedback::copy_hash_button(hash_mark, "", format!("Hash · Copy full commit hash · {}", commit.oid), cx), "copy-commit", "Copy full commit hash")
                } else {
                    copy_feedback::copy_hash_button(hash_mark, "Hash", format!("Copy full commit hash · {}", commit.oid), cx)
                        .accessibility_label("Copy full commit hash")
                }
                    .on_click(move |_, window, cx| copy_feedback::copy(window, cx, Some(hash_copy.clone()), "commit hash", hash.clone())))
                // At the trailing edge of the copy controls' last row, above
                // the scrolling message and metadata, so it stays in view.
                .children(actions)
            )
            .child(div()
                .when_some(reveal, |viewport, reveal| viewport.on_children_prepainted(reveal.observe()))
                .id("commit-message-viewport").debug_selector(|| "commit-message-viewport".into())
                .role(Role::Document)
                .aria_label("Commit message. Use arrow keys, Page Up, Page Down, Home or End to scroll.")
                .tab_stop(true)
                .key_context("GitTurtleText")
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| click_focus.focus(window, cx))
                // Selected text copies byte for byte; without a selection,
                // Copy continues to the window's handlers.
                .on_action(move |_: &gpui_kit::component::input::Copy, _, cx| {
                    if !copy_selection.copy(cx) { cx.propagate(); }
                })
                .on_action(move |_: &gpui_kit::component::input::SelectAll, window, cx| select_all.select_all(window, cx))
                .on_key_down(move |event, _, cx| {
                    let modifiers = &event.keystroke.modifiers;
                    if modifiers.platform || modifiers.control || modifiers.alt { return; }
                    let mut offset = key_scroll.scroll_px_offset_for_scrollbar();
                    let page = key_scroll.viewport_bounds().size.height * 0.85;
                    match event.keystroke.key.as_str() {
                        "up" => offset.y += appearance::ui_size(24.),
                        "down" => offset.y -= appearance::ui_size(24.),
                        "pageup" => offset.y += page,
                        "pagedown" | "space" => offset.y -= page,
                        "home" => { key_scroll.scroll_to(ListOffset { item_ix: 0, offset_in_item: px(0.) }); let _ = key_owner.update(cx, |_, cx| cx.notify()); cx.stop_propagation(); return; }
                        "end" => { key_scroll.scroll_to_end(); let _ = key_owner.update(cx, |_, cx| cx.notify()); cx.stop_propagation(); return; }
                        _ => return,
                    };
                    key_scroll.set_offset_from_scrollbar(offset);
                    let _ = key_owner.update(cx, |_, cx| cx.notify());
                    cx.stop_propagation();
                })
                .relative().min_h_0().min_w_0().overflow_hidden()
                .map(|viewport| match extent {
                    // Its inset gives way first, so details that fit still
                    // draw no scrollbar.
                    Some(height) => viewport.h(height).flex_shrink(1.),
                    None => viewport.flex_1(),
                })
                .child(list)
                .child(selection.registrar(scroll.clone()))
                .child(message_cue(scroll.clone(), cx))
                // Its keyboard ring on its own edge, inside its clip, over
                // the text, so none of it moves.
                .children(self.focus_ring.edge(&focus, rgb(colors.accent)))
            )
            .into_any_element()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::rc::Rc;

    pub(crate) fn commit(oid: &str, subject: &str, body: &str) -> Commit {
        Commit {
            oid: oid.repeat(40),
            parents: vec!["f".repeat(40)],
            author: "Author 東京".into(),
            timestamp: 0,
            subject: subject.into(),
            body: body.into(),
        }
    }

    #[test]
    fn copy_uses_complete_loaded_message_without_empty_body_separator() {
        for (subject, body, expected) in [
            ("Title 🐢", "", "Title 🐢"),
            (
                "Title 🐢",
                "Paragraph one.\n\n  Keep indentation.\nSigned-off-by: A",
                "Title 🐢\n\nParagraph one.\n\n  Keep indentation.\nSigned-off-by: A",
            ),
            ("", "", ""),
        ] {
            assert_eq!(full_message(&commit("a", subject, body)), expected);
        }
    }

    #[test]
    fn giant_titles_paragraphs_and_graphemes_have_bounded_pieces() {
        for source in [
            "x".repeat(1_600_000),
            "東京👩🏽‍💻e\u{301}".repeat(50_000),
            format!("x{}", "\u{301}".repeat(800_000)),
        ] {
            let mut pieces = Vec::new();
            append_pieces(&mut pieces, &source, true, 0);
            assert!(pieces.len() > 1);
            assert!(pieces.iter().all(|piece| piece.text.len() <= PIECE_BYTES));
            assert_eq!(
                pieces
                    .iter()
                    .map(|piece| piece.text.as_ref())
                    .collect::<String>(),
                source
            );
        }
        let source = "東京👩🏽‍💻e\u{301}".repeat(500);
        let boundaries = source
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .collect::<HashSet<_>>();
        let mut pieces = Vec::new();
        append_pieces(&mut pieces, &source, false, 0);
        let mut offset = 0;
        for piece in pieces {
            assert!(boundaries.contains(&offset));
            assert_eq!(piece.start, offset);
            offset += piece.text.len();
        }
    }

    #[test]
    fn many_source_lines_keep_paragraph_spacing_with_bounded_shaping() {
        let source = (0..500)
            .map(|i| format!("Paragraph {i}: 東京.\n\n"))
            .collect::<String>();
        let mut pieces = Vec::new();
        append_pieces(&mut pieces, &source, false, 0);
        assert!(pieces.iter().all(|piece| piece.text.len() <= PIECE_BYTES));
        assert!(
            pieces.iter().all(
                |piece| piece.text.bytes().filter(|byte| *byte == b'\n').count() <= PIECE_LINES
            )
        );
        assert_eq!(
            pieces
                .iter()
                .map(|piece| piece.text.as_ref())
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
    }

    pub(crate) struct Probe {
        pub(crate) app: Entity<GitTurtle>,
        width: f32,
        height: f32,
        /// Also render the History list, left of the inspector.
        history: bool,
        /// Lay the message toolbar out for the inspector, pinned, as the
        /// window does before it draws one, so the panel shrinks to its
        /// measured message (`GitTurtle::message_extent`).
        toolbar: bool,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if self.toolbar {
                let (width, height) = (px(self.width), px(self.height));
                self.app.update(cx, |app, _| {
                    app.inspector_message
                        .lay_out_toolbar(Some(Toolbar::within(width, height, true, window)));
                });
            }
            let inspector = div()
                .id("inspector-test")
                .debug_selector(|| "inspector-test".into())
                .w(px(self.width))
                .h(px(self.height))
                .child(self.app.update(cx, |app, cx| {
                    app.render_inspector(crate::views::FilesHeader::WHOLE, window, cx)
                }));
            if !self.history {
                return inspector.into_any_element();
            }
            div()
                .flex()
                .child(
                    div().w(px(640.)).h(px(self.height)).child(
                        self.app
                            .update(cx, |app, cx| app.render_history(window, cx)),
                    ),
                )
                .child(inspector)
                .into_any_element()
        }
    }

    pub(crate) fn fixture(
        cx: &mut TestAppContext,
        commits: Vec<Commit>,
    ) -> (Entity<Probe>, Entity<GitTurtle>, &mut VisualTestContext) {
        crate::views::tests::init_app(cx);
        let app_holder = Rc::new(RefCell::new(None));
        let probe_holder = Rc::new(RefCell::new(None));
        let captured_app = app_holder.clone();
        let captured_probe = probe_holder.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                // This fixture runs synchronous GPUI layout tests, but startup
                // profile loading uses the real preferences worker. Finish its
                // FIFO before GPUI first polls the foreground reply; otherwise
                // a worker-thread wake races the deterministic test scheduler.
                futures::executor::block_on(app.preferences_writer.submit(|| Ok(())))
                    .expect("startup preferences worker replied")
                    .expect("startup preferences queue drained");
                app.page = AppPage::Repository;
                app.commits = commits;
                app.selected_commit = Some(0);
                app
            });
            let probe = cx.new(|_| Probe {
                app: app.clone(),
                width: 280.,
                height: 520.,
                history: false,
                toolbar: false,
            });
            *captured_app.borrow_mut() = Some(app);
            *captured_probe.borrow_mut() = Some(probe.clone());
            Root::new(probe, window, cx)
        });
        let app = app_holder.borrow().clone().unwrap();
        let probe = probe_holder.borrow().clone().unwrap();
        draw(cx);
        (probe, app, cx)
    }

    pub(crate) fn draw(cx: &mut VisualTestContext) {
        for _ in 0..3 {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
    }

    #[gpui::test]
    fn rendered_message_labels_expose_text_to_native_accessibility(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            for (index, text) in ["Title 東京 🐢", "First paragraph.\n\n  Keep indentation."]
                .into_iter()
                .enumerate()
            {
                let piece = Piece {
                    start: 0,
                    text: text.to_owned().into(),
                    title: index == 0,
                    body_start: index == 1,
                };
                let focus = cx.focus_handle();
                let selection = Selection::new(Arc::new(Vec::new()), 0, focus, cx);
                let element = render_piece(&piece, index, &selection, cx).into_element();
                let mut node = gpui::accesskit::Node::new(element.a11y_role().unwrap());
                element.write_a11y_info(&mut node);
                assert_eq!(node.role(), Role::Label);
                // AccessKit's native adapters read a Label's text from value.
                // A label-only property renders visibly but has no spoken name.
                assert_eq!(node.value(), Some(text));
            }
        });
    }

    #[gpui::test]
    fn rendered_narrow_inspector_reserves_files_and_copies_offscreen_text(cx: &mut TestAppContext) {
        let body = "Paragraph 東京 🐢.\n\n".repeat(10_000);
        let mut selected = commit("a", &"Long title ".repeat(200), &body);
        selected.parents = (0..128).map(|index| format!("{index:040x}")).collect();
        let expected = full_message(&selected);
        let (_, app, cx) = fixture(cx, vec![selected]);
        for font in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(font, 12, window, cx));
            draw(cx);
            let inspector = cx.debug_bounds("inspector-test").unwrap();
            let files = cx.debug_bounds("commit-inspector-files").unwrap();
            let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
            assert!(
                files.size.height >= inspector.size.height * 0.5,
                "{files:?} / {inspector:?}"
            );
            assert!(viewport.size.height > px(80.));
            for id in ["copy-message", "copy-commit"] {
                let button = cx.debug_bounds(id).unwrap();
                assert!(button.left() >= inspector.left() && button.right() <= inspector.right());
            }
            cx.simulate_click(viewport.center(), Modifiers::default());
            cx.simulate_keystrokes("pagedown");
            draw(cx);
            cx.read(|cx| {
                let retained = app.read(cx).inspector_message.0.borrow();
                let position = retained.as_ref().unwrap().scroll.logical_scroll_top();
                assert!(position.item_ix > 0 || position.offset_in_item > px(0.));
            });
            cx.simulate_keystrokes("home");
            draw(cx);
            cx.read(|cx| {
                let retained = app.read(cx).inspector_message.0.borrow();
                let position = retained.as_ref().unwrap().scroll.logical_scroll_top();
                assert_eq!((position.item_ix, position.offset_in_item), (0, px(0.)));
            });
            cx.update(|_, cx| {
                app.update(cx, |app, _| {
                    let state = app.inspector_message.0.borrow();
                    let state = state.as_ref().unwrap();
                    assert!(state.pieces.len() > 100);
                    assert!(
                        state
                            .scroll
                            .bounds_for_item(state.pieces.len() / 2)
                            .is_none(),
                        "offscreen text should not be laid out on entry"
                    );
                    state.scroll.scroll_to_end();
                })
            });
            draw(cx);
            let button = cx.debug_bounds("copy-message").unwrap();
            cx.simulate_click(button.center(), Modifiers::default());
            cx.read(|cx| {
                assert!(
                    cx.read_from_clipboard().unwrap().text().unwrap() == expected,
                    "copy must preserve the complete loaded message"
                )
            });
            cx.update(|_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    "before keyboard activation".into(),
                ))
            });
            // Native buttons intentionally preserve pointer focus. Use the
            // actual tab-stop traversal to focus the first toolbar control.
            cx.update(|window, cx| {
                window.blur(cx);
                window.focus_next(cx);
                window.draw(cx).clear(cx);
            });
            // Native activation is completed on key release; the convenience
            // simulate_keystrokes helper only dispatches key-down events.
            let keystroke = Keystroke::parse("enter").unwrap();
            cx.simulate_event(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            cx.simulate_event(KeyUpEvent { keystroke });
            cx.read(|cx| {
                assert!(
                    cx.read_from_clipboard().unwrap().text().unwrap() == expected,
                    "copy must preserve the complete loaded message"
                )
            });
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    #[gpui::test]
    fn changed_file_selection_stays_visible_after_message_pane_and_density_resize(
        cx: &mut TestAppContext,
    ) {
        let (probe, app, cx) = fixture(
            cx,
            vec![commit(
                "a",
                "Selected commit",
                &"Long paragraph. ".repeat(5000),
            )],
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.files = (0..100)
                    .map(|index| FileChange {
                        old_path: Some(format!("file-{index}.txt").into()),
                        new_path: Some(format!("file-{index}.txt").into()),
                        old_oid: Some("a".repeat(40)),
                        new_oid: Some("b".repeat(40)),
                        old_mode: "100644".into(),
                        new_mode: "100644".into(),
                        status: gitturtle_core::ChangeStatus::Modified,
                    })
                    .collect();
                app.selected_file = Some(80);
                app.refresh_file_filter(window, cx);
                app.file_scroll.scroll_to_item(80, ScrollStrategy::Bottom);
            })
        });
        draw(cx);
        for (height, font, density) in [
            (480., 18, appearance::Density::Compact),
            (640., 13, appearance::Density::Comfortable),
        ] {
            cx.update(|window, cx| {
                appearance::apply_text_sizes(font, 12, window, cx);
                probe.update(cx, |probe, cx| {
                    probe.height = height;
                    cx.notify();
                });
                app.update(cx, |app, cx| {
                    app.settings.density = density;
                    cx.notify();
                });
            });
            draw(cx);
            let selected = cx
                .debug_bounds("selected-changed-file")
                .expect("selected row remains rendered");
            cx.read(|cx| {
                let app = app.read(cx);
                assert_eq!(app.selected_file, Some(80));
                let viewport = app.file_scroll.0.borrow().base_handle.bounds();
                assert!(
                    selected.top() >= viewport.top() - px(1.)
                        && selected.bottom() <= viewport.bottom() + px(1.),
                    "selected row {selected:?} outside {viewport:?}"
                );
            });
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    #[gpui::test]
    fn inspection_transfer_and_selection_keep_the_right_message_viewport(cx: &mut TestAppContext) {
        let (_, app, cx) = fixture(
            cx,
            vec![
                commit("a", "First", &"Line\n\n".repeat(300)),
                commit("b", "Second", "Body"),
            ],
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let (_, scroll, _, _) = app.inspector_message.presentation(&app.commits[0], cx);
                scroll.scroll_to(ListOffset {
                    item_ix: 5,
                    offset_in_item: px(20.),
                });
                let before = scroll.logical_scroll_top();
                app.mode = WorkspaceMode::Compare;
                app.back_to_history(window, cx);
                assert_eq!(
                    (
                        scroll.logical_scroll_top().item_ix,
                        scroll.logical_scroll_top().offset_in_item
                    ),
                    (before.item_ix, before.offset_in_item)
                );
                let retained = app.take_inspection_context(window, cx);
                assert_eq!(app.inspector_message.retained_bytes(), 0);
                app.inspector_message.presentation(&app.commits[1], cx);
                app.restore_inspection_context(retained, window, cx);
                let (_, restored, _, _) = app.inspector_message.presentation(&app.commits[0], cx);
                assert_eq!(
                    (
                        restored.logical_scroll_top().item_ix,
                        restored.logical_scroll_top().offset_in_item
                    ),
                    (before.item_ix, before.offset_in_item)
                );
                app.select_commit(1, window, cx);
                let (_, next, _, _) = app.inspector_message.presentation(&app.commits[1], cx);
                assert_eq!(next.logical_scroll_top().item_ix, 0);
                assert_eq!(next.logical_scroll_top().offset_in_item, px(0.));
                app.select_commit(0, window, cx);
                let (_, previous, _, _) = app.inspector_message.presentation(&app.commits[0], cx);
                assert_eq!(previous.logical_scroll_top().item_ix, 0);
            })
        });
    }

    /// A kept measurement of one commit's message stands only for the
    /// details it measured with, at the same width, bound and text: the
    /// commit's own, as History measures them, never stand for a revision
    /// of it, whose details add its paths, nor one revision's for another
    /// change or parent. No window path reaches this yet, since File
    /// History opens with no measurement (`take_inspection_context`) and a
    /// file's lineage lists each commit once, so its rendered regression
    /// (`file_history::tests::file_revisions_follow_the_revisions_paths`)
    /// passes with this key gone; the key keeps one presentation that
    /// measured either from answering for the other.
    #[gpui::test]
    fn a_kept_measurement_stands_only_for_the_details_it_measured(cx: &mut TestAppContext) {
        let (_, app, cx) = fixture(cx, vec![commit("a", "First", "Body")]);
        let revision = |path: &str, parent: &str| FileHistoryEntry {
            commit: commit("a", "First", "Body"),
            change: gitturtle_core::FileChange {
                old_path: Some(path.into()),
                new_path: Some(path.into()),
                old_oid: Some("c".repeat(40)),
                new_oid: Some("d".repeat(40)),
                status: gitturtle_core::ChangeStatus::Modified,
                old_mode: "100644".into(),
                new_mode: "100644".into(),
            },
            parent_oid: Some(parent.repeat(40)),
        };
        let (notes, other_path, other_parent) = (
            revision("notes.txt", "f"),
            revision("other.txt", "f"),
            revision("notes.txt", "e"),
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let state = &app.inspector_message;
                state.presentation(&app.commits[0], cx);
                let key = LineKey::current(window);
                let (width, bound, height) = (px(300.), px(400.), px(120.));
                state.remember_extent(width, bound, key.clone(), 2., None, height);
                assert_eq!(state.extent(width, bound, &key, 2., None), Some(height));
                assert_eq!(
                    state.extent(width, bound, &key, 2., Some(&notes)),
                    None,
                    "the commit's own details stand for no revision of it"
                );
                state.remember_extent(width, bound, key.clone(), 2., Some(&notes), height);
                assert_eq!(
                    state.extent(width, bound, &key, 2., Some(&notes)),
                    Some(height)
                );
                for (other, what) in [
                    (None, "the commit's own details"),
                    (Some(&other_path), "another path"),
                    (Some(&other_parent), "another parent"),
                ] {
                    assert_eq!(
                        state.extent(width, bound, &key, 2., other),
                        None,
                        "a revision's details stand not for {what}"
                    );
                }
            })
        });
    }

    /// A kept measurement of a message's height stands only for the display
    /// scale it was measured at, which layout measures it on: the pinned
    /// inspector's panel, 280 × 600 px, drawn at scale 2 and then at 1.2
    /// after a resize, its width, cap, font and details as they were, is as
    /// tall as a panel first drawn at 1.2, and so is its viewport, which is
    /// the height measured at 1.2 on that display's device pixels. Kept
    /// without the scale, a window moved to a display of another scale at
    /// the same logical width drew its panel from the other display's
    /// height. History's own window cannot hold the cap across those two
    /// scales: its chrome, each part on its device pixels, leaves the
    /// inspector a third of a pixel taller at 1.2.
    #[gpui::test]
    fn a_kept_measurement_stands_only_for_the_scale_it_was_measured_at(cx: &mut TestAppContext) {
        let body = "First paragraph of the body.\n\nSecond paragraph.";
        let (probe, app, cx) = fixture(cx, vec![commit("a", "Selected title", body)]);
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            probe.update(cx, |probe, cx| {
                probe.height = 600.;
                probe.toolbar = true;
                cx.notify();
            });
        });
        // The toolbar laid out for the inspector, the panel's and its
        // viewport's drawn heights and the height measured, as drawn now.
        let read = |cx: &mut VisualTestContext| {
            draw(cx);
            let toolbar = cx
                .read(|cx| app.read(cx).inspector_message.1.get())
                .expect("the window lays the inspector out");
            let panel = cx.debug_bounds("commit-message-panel").unwrap();
            let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
            let measured = cx
                .read(|cx| app.read(cx).inspector_message.measured())
                .expect("the message measured");
            (
                (toolbar.viewport, toolbar.cap),
                panel.size.height,
                viewport.size.height,
                measured,
            )
        };
        // A resize takes the test display's own scale, 2, and draws there.
        cx.simulate_resize(size(px(1000.), px(680.)));
        let at_two = read(cx);
        // Moved to a display of scale 1.2, set after the resize.
        cx.simulate_resize(size(px(1000.), px(680.)));
        cx.update(|window, _| window.set_scale_factor(1.2));
        let moved = read(cx);
        // A panel first drawn at 1.2: the presentation dropped there, as
        // another commit's selection drops it, with no resize to draw it at
        // 2 first, is measured anew at 1.2.
        cx.update(|_, cx| app.update(cx, |app, cx| app.inspector_message.select("", cx)));
        let fresh = read(cx);
        cx.update(|window, _| window.set_scale_factor(2.));
        assert_eq!(
            at_two.0, fresh.0,
            "the inspector is as wide and as tall at both scales"
        );
        assert_ne!(
            at_two.3, fresh.3,
            "the message measures another height at each scale"
        );
        assert_eq!(
            moved.3, fresh.3,
            "moved from scale 2 to 1.2, the panel is measured at 1.2: {at_two:?}"
        );
        assert_eq!(
            (moved.1, moved.2),
            (fresh.1, fresh.2),
            "moved from scale 2 to 1.2, the panel and its viewport are drawn as a panel \
             first drawn at 1.2"
        );
        assert!(
            (fresh.2 - fresh.3).abs() < px(1. / 1.2),
            "the viewport {:?} is drawn as tall as the message measured, {:?}",
            fresh.2,
            fresh.3
        );
    }

    pub(crate) fn message_selection(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) -> Selection {
        cx.read(|cx| {
            let state = app.read(cx).inspector_message.0.borrow();
            state.as_ref().unwrap().selection.clone()
        })
    }

    /// Each piece's document offset and displayed length.
    fn piece_spans(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Vec<(usize, usize)> {
        cx.read(|cx| {
            let state = app.read(cx).inspector_message.0.borrow();
            let state = state.as_ref().unwrap();
            state
                .pieces
                .iter()
                .map(|piece| (piece.start, piece.text.len()))
                .collect()
        })
    }

    fn message_focused(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> bool {
        let focus = cx.read(|cx| {
            let state = app.read(cx).inspector_message.0.borrow();
            state.as_ref().unwrap().focus.clone()
        });
        cx.update(|window, _| focus.is_focused(window))
    }

    fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        draw(cx);
    }

    /// The platform's copy chord through the keymap, then the clipboard; a
    /// chord that copies nothing leaves the sentinel.
    pub(crate) fn copy_chord(cx: &mut VisualTestContext) -> String {
        cx.update(|_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string("nothing copied".into()))
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-c"
        } else {
            "ctrl-c"
        });
        cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap())
    }

    fn has_text_selection(cx: &mut VisualTestContext) -> bool {
        cx.update(gpui_kit::base::TextSelection::has_selection)
    }

    #[gpui::test]
    fn dragging_within_a_piece_copies_exactly_the_selected_text(cx: &mut TestAppContext) {
        let selected = commit("a", "Alpha bravo 東京 charlie", "Body paragraph.");
        let message = full_message(&selected);
        let (_, app, cx) = fixture(cx, vec![selected]);
        let selection = message_selection(&app, cx);
        let start = message.find("bravo").unwrap();
        let end = message.find(" charlie").unwrap();
        drag(
            cx,
            selection.point_at(start).unwrap(),
            selection.point_at(end).unwrap(),
        );
        assert!(has_text_selection(cx));
        // The gesture focuses the message, whose Copy handler takes the chord.
        assert!(message_focused(&app, cx));
        assert_eq!(copy_chord(cx), &message[start..end]);

        // Double-click selects the word under the pointer.
        let word = selection
            .point_at(message.find("charlie").unwrap() + 2)
            .unwrap();
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: word,
            modifiers: Modifiers::default(),
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: word,
            modifiers: Modifiers::default(),
            click_count: 2,
        });
        draw(cx);
        assert_eq!(copy_chord(cx), "charlie");
    }

    #[gpui::test]
    fn dragging_across_pieces_and_into_metadata_keeps_every_line_break(cx: &mut TestAppContext) {
        let body = (0..40)
            .map(|line| format!("Line {line:02} 東京\n"))
            .collect::<String>();
        let selected = commit("a", "Title 🐢", body.trim_end());
        let message = full_message(&selected);
        let (probe, app, cx) = fixture(cx, vec![selected]);
        cx.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.height = 2_400.;
                cx.notify();
            })
        });
        draw(cx);
        let selection = message_selection(&app, cx);
        let pieces = piece_spans(&app, cx);
        assert!(pieces.len() >= 4, "{pieces:?}");
        let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
        let point = |offset| {
            let point = selection.point_at(offset).unwrap();
            assert!(
                viewport.contains(&point),
                "{offset} at {point:?} outside {viewport:?}"
            );
            point
        };

        // Across a virtualization boundary, whose newline neither piece shows,
        // dragged upward.
        let (first, second) = (pieces[1], pieces[2]);
        assert_eq!(first.0 + first.1 + 1, second.0);
        // Each line ends in "東京", six bytes.
        let (start, end) = (first.0 + first.1 - 6, second.0 + 5);
        drag(cx, point(end), point(start));
        let copied = copy_chord(cx);
        assert_eq!(copied, &message[start..end]);
        assert!(copied.contains('\n'));

        // From the title into the body keeps the blank line between them.
        let end = pieces[1].0 + 4;
        drag(cx, point(2), point(end));
        assert_eq!(copy_chord(cx), &message[2..end]);

        // From the body into the metadata after the message.
        let date = cx.read(|cx| {
            format!(
                "{} · {}",
                full_date(app.read(cx).commits[0].timestamp),
                short_oid(&app.read(cx).commits[0].oid)
            )
        });
        let author = commit_message_author();
        let metadata = message.len() + 2;
        let start = message.len() - 3;
        let end = metadata + author.len() + 1 + 5;
        drag(cx, point(start), point(end));
        assert_eq!(
            copy_chord(cx),
            format!("{}\n\n{author}\n{}", &message[start..], &date[..5])
        );
    }

    fn commit_message_author() -> String {
        summary(&commit("a", "", "").author)
    }

    #[gpui::test]
    fn select_all_copies_the_complete_loaded_message(cx: &mut TestAppContext) {
        let body = "Paragraph 東京 🐢.\n\n  Keep indentation.\r\n".repeat(3_000);
        let selected = commit("a", &"Long title ".repeat(200), &body);
        let expected = full_message(&selected);
        let (_, app, cx) = fixture(cx, vec![selected]);
        cx.update(|_, cx| shortcuts::bind_keys(cx));
        let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
        cx.simulate_click(viewport.center(), Modifiers::default());
        assert!(message_focused(&app, cx));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        draw(cx);
        assert!(has_text_selection(cx));
        let copied = copy_chord(cx);
        assert!(
            copied == expected,
            "select all copied {} of {} bytes",
            copied.len(),
            expected.len()
        );
        // The visible pieces paint the selection without shaping the rest.
        cx.read(|cx| {
            let state = app.read(cx).inspector_message.0.borrow();
            let state = state.as_ref().unwrap();
            assert!(
                state
                    .scroll
                    .bounds_for_item(state.pieces.len() / 2)
                    .is_none()
            );
        });
    }

    #[gpui::test]
    fn a_selection_copies_pieces_scrolled_out_of_view(cx: &mut TestAppContext) {
        let body = (0..2_000)
            .map(|line| format!("Line {line} 東京 🐢\n"))
            .collect::<String>();
        let selected = commit("a", "Title", &body);
        let message = full_message(&selected);
        let (_, app, cx) = fixture(cx, vec![selected]);
        let selection = message_selection(&app, cx);
        let pieces = piece_spans(&app, cx);
        let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
        let start = pieces[1].0 + 3;
        let from = selection.point_at(start).unwrap();
        assert!(viewport.contains(&from));
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            from + point(px(20.), px(0.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        // With the button held, wheel steps scroll far past the anchor, as
        // auto-scroll's synthesized steps do. The list measures the rows it
        // reaches each frame.
        for _ in 0..40 {
            cx.simulate_event(ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            });
            draw(cx);
        }
        let painted = selection.painted_starts();
        assert!(!painted.contains(&pieces[1].0), "{painted:?}");
        // A painted line well inside the viewport, clear of auto-scroll's edges.
        let end = message
            .match_indices('\n')
            .map(|(index, _)| index + 1 + "Line ".len())
            .find(|&offset| {
                selection.point_at(offset).is_some_and(|point| {
                    point.y > viewport.top() + px(40.) && point.y < viewport.bottom() - px(40.)
                })
            })
            .unwrap();
        let to = selection.point_at(end).unwrap();
        cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        draw(cx);
        let copied = copy_chord(cx);
        assert!(copied == message[start..end], "copied {copied:?}");
        let skipped = pieces
            .iter()
            .filter(|(piece, _)| (start..end).contains(piece) && !painted.contains(piece))
            .count();
        assert!(
            skipped > 10,
            "{skipped} pieces between the endpoints were never painted"
        );
    }

    #[gpui::test]
    fn dragging_past_the_viewport_auto_scrolls_and_extends_the_selection(cx: &mut TestAppContext) {
        let body = (0..2_000)
            .map(|line| format!("Line {line} 東京\n"))
            .collect::<String>();
        let selected = commit("a", "Title", &body);
        let message = full_message(&selected);
        let (_, app, cx) = fixture(cx, vec![selected]);
        let selection = message_selection(&app, cx);
        let pieces = piece_spans(&app, cx);
        let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
        let start = pieces[1].0 + 1;
        let from = selection.point_at(start).unwrap();
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        let below = point(from.x, viewport.bottom() + px(30.));
        cx.simulate_mouse_move(below, Some(MouseButton::Left), Modifiers::default());
        for _ in 0..40 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            cx.run_until_parked();
            draw(cx);
        }
        // Released without a move after the last scroll step.
        cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::default());
        draw(cx);
        let top = cx.read(|cx| {
            let state = app.read(cx).inspector_message.0.borrow();
            state.as_ref().unwrap().scroll.logical_scroll_top().item_ix
        });
        assert!(top > 2, "auto-scroll moved the list to piece {top}");
        // The selection ends at the text under the pointer in the frame shown
        // at release, not where the previous frame had it.
        let pointer = cx.update(|window, _| window.mouse_position());
        let end = selection.offset_at_window(pointer).unwrap();
        assert!(end > pieces[2].0 + pieces[2].1, "{end}");
        let copied = copy_chord(cx);
        assert!(
            copied == message[start..end],
            "copied {} bytes, expected {}: ends {:?}",
            copied.len(),
            end - start,
            &copied[copied.floor_char_boundary(copied.len().saturating_sub(12))..]
        );
    }

    #[gpui::test]
    fn another_commit_or_a_click_elsewhere_clears_the_selection(cx: &mut TestAppContext) {
        let first = commit("a", "First commit title", "First body.");
        let message = full_message(&first);
        let (_, app, cx) = fixture(cx, vec![first, commit("b", "Second", "Body")]);
        let selection = message_selection(&app, cx);
        let select = |cx: &mut VisualTestContext| {
            drag(
                cx,
                selection.point_at(0).unwrap(),
                selection
                    .point_at(message.find(" commit").unwrap())
                    .unwrap(),
            );
            assert!(has_text_selection(cx));
            assert_eq!(selection.selected_text(), "First");
        };
        select(cx);
        let files = cx.debug_bounds("commit-inspector-files").unwrap();
        cx.simulate_click(files.center(), Modifiers::default());
        draw(cx);
        assert!(!has_text_selection(cx));
        assert_eq!(selection.selected_text(), "");

        select(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        draw(cx);
        assert!(!has_text_selection(cx));
        cx.update(|window, cx| {
            assert_eq!(gpui_kit::base::TextSelection::selected_text(window, cx), "");
            app.update(cx, |app, cx| {
                assert!(!app.inspector_message.copy_selection(cx))
            });
        });
    }

    /// The window keeps a replaced or put-away presentation's participant
    /// until its next frame; Copy in that gap must not return its text.
    #[gpui::test]
    fn copy_before_the_next_frame_never_returns_a_replaced_or_retained_message(
        cx: &mut TestAppContext,
    ) {
        let (first, second) = (
            commit("a", "First commit title", "First body."),
            commit("b", "Second commit title", "Second body."),
        );
        let (first_message, second_message) = (full_message(&first), full_message(&second));
        let (probe, app, cx) = fixture(cx, vec![first, second]);
        cx.update(|_, cx| {
            shortcuts::bind_keys(cx);
            app.update(cx, |app, _| {
                app.graph = crate::graph::layout(&app.commits, || Ok::<_, ()>(())).unwrap();
                app.visible = (0..app.commits.len()).collect();
            });
            probe.update(cx, |probe, cx| {
                probe.history = true;
                cx.notify();
            });
        });
        draw(cx);
        let select_title_word = |cx: &mut VisualTestContext, message: &str| {
            let selection = message_selection(&app, cx);
            let end = message.find(' ').unwrap();
            drag(
                cx,
                selection.point_at(0).unwrap(),
                selection.point_at(end).unwrap(),
            );
            assert_eq!(selection.selected_text(), &message[..end]);
            selection
        };
        let focus_history = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let focus = app.read(cx).focus.clone();
                window.focus(&focus, cx);
            })
        };

        // Tests draw dirty windows when an update's effects flush, so each
        // change and its Copy share one update, as a key press after Down
        // shares the interval before the next frame.
        let chord = Keystroke::parse(if cfg!(target_os = "macos") {
            "cmd-c"
        } else {
            "ctrl-c"
        })
        .unwrap();
        let sentinel = || ClipboardItem::new_string("nothing copied".into());
        let clipboard = |cx: &mut VisualTestContext| {
            cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap())
        };

        // Down in History, then Copy before a frame shows the new commit.
        let replaced = select_title_word(cx, &first_message);
        focus_history(cx);
        cx.update(|window, cx| {
            cx.write_to_clipboard(sentinel());
            app.update(cx, |app, cx| app.select_commit(1, window, cx));
            assert_eq!(replaced.selected_text(), "");
            assert!(replaced.painted_starts().is_empty());
            window.dispatch_keystroke(chord.clone(), cx);
        });
        assert_eq!(clipboard(cx), "b".repeat(40));

        // Put away into a return context, as Compare and warm tabs do: the
        // participant keeps only its metadata lines.
        let retained = select_title_word(cx, &second_message);
        let painted_bytes = retained.retained_bytes();
        assert!(!retained.painted_starts().is_empty());
        focus_history(cx);
        let context = cx.update(|window, cx| {
            cx.write_to_clipboard(sentinel());
            let context = app.update(cx, |app, cx| app.take_inspection_context(window, cx));
            assert_eq!(retained.selected_text(), "");
            assert!(retained.painted_starts().is_empty());
            window.dispatch_keystroke(chord.clone(), cx);
            context
        });
        assert_eq!(clipboard(cx), "b".repeat(40));
        let date_line = cx.read(|cx| {
            let commit = &app.read(cx).commits[1];
            format!(
                "{} · {}",
                full_date(commit.timestamp),
                short_oid(&commit.oid)
            )
        });
        let metadata = commit_message_author().len() + date_line.len();
        assert!(retained.retained_bytes() < painted_bytes);
        assert_eq!(retained.retained_bytes(), metadata);

        // Restored, it paints and selects again.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.restore_inspection_context(context, window, cx)
            })
        });
        draw(cx);
        assert!(!retained.painted_starts().is_empty());
        select_title_word(cx, &second_message);
        assert_eq!(copy_chord(cx), "Second");
    }

    #[gpui::test]
    fn list_copy_takes_the_row_identity_unless_text_is_selected(cx: &mut TestAppContext) {
        let selected = commit("a", "Selected title", "Body.");
        let message = full_message(&selected);
        let (probe, app, cx) = fixture(cx, vec![selected, commit("b", "Older", "")]);
        let path = "dir/東京 notes.txt";
        cx.update(|window, cx| {
            shortcuts::bind_keys(cx);
            app.update(cx, |app, cx| {
                app.graph = crate::graph::layout(&app.commits, || Ok::<_, ()>(())).unwrap();
                app.visible = (0..app.commits.len()).collect();
                app.files = ["first.txt", path]
                    .into_iter()
                    .map(|path| FileChange {
                        old_path: Some(path.into()),
                        new_path: Some(path.into()),
                        old_oid: Some("a".repeat(40)),
                        new_oid: Some("b".repeat(40)),
                        old_mode: "100644".into(),
                        new_mode: "100644".into(),
                        status: gitturtle_core::ChangeStatus::Modified,
                    })
                    .collect();
                app.selected_file = Some(1);
                app.refresh_file_filter(window, cx);
            });
            probe.update(cx, |probe, cx| {
                probe.history = true;
                cx.notify();
            });
        });
        draw(cx);
        let focus = |cx: &mut VisualTestContext, files: bool| {
            cx.update(|window, cx| {
                let app = app.read(cx);
                let focus = if files {
                    app.file_focus.clone()
                } else {
                    app.focus.clone()
                };
                window.focus(&focus, cx);
            });
            draw(cx);
        };

        focus(cx, false);
        assert_eq!(copy_chord(cx), "a".repeat(40));
        focus(cx, true);
        assert_eq!(copy_chord(cx), path);

        // Selected message text wins over the focused row.
        let selection = message_selection(&app, cx);
        let end = message.find(" title").unwrap();
        drag(
            cx,
            selection.point_at(0).unwrap(),
            selection.point_at(end).unwrap(),
        );
        for files in [false, true] {
            focus(cx, files);
            assert_eq!(copy_chord(cx), &message[..end]);
        }
        cx.update(gpui_kit::base::TextSelection::clear);
        focus(cx, false);
        assert_eq!(copy_chord(cx), "a".repeat(40));
    }

    /// The selection is the applied theme's fitted text selection; message
    /// text, and the muted metadata drawn on it, read on it in every built-in.
    #[test]
    fn selection_highlight_keeps_message_and_metadata_text_readable() {
        use crate::appearance::{ThemeChoice, custom::contrast};
        for choice in ThemeChoice::ALL {
            let palette = choice.palette();
            let selection = palette.editor_highlights().selection;
            let (text, muted) = (
                contrast(palette.text, selection),
                contrast(palette.muted, selection),
            );
            assert!(text >= 4.5, "{choice:?}: text {text:.2}:1");
            assert!(muted >= 4.5, "{choice:?}: metadata {muted:.2}:1");
        }
    }

    /// The quads the last frame painted on `area` as wide as a scrollbar's
    /// track: the track's own fill and edge, filled with the canvas in a
    /// palette, or clear without one.
    fn tracks_on(cx: &mut VisualTestContext, area: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let logical = |value: ScaledPixels| px(value.as_f32() / scale);
            window
                .painted_quads()
                .into_iter()
                .map(|quad| {
                    let (origin, extent) = (quad.bounds.origin, quad.bounds.size);
                    Bounds::new(
                        point(logical(origin.x), logical(origin.y)),
                        size(logical(extent.width), logical(extent.height)),
                    )
                })
                .filter(|quad| {
                    (quad.size.width - Scrollbar::width()).abs() < px(0.5) && quad.intersects(&area)
                })
                .collect()
        })
    }

    /// The edge fades the last frame painted over the message viewport at
    /// `viewport`: the height of the one along its top and of the one along
    /// its bottom, each as wide as the viewport, in the inspector's panel
    /// colour, opaque at its edge and clear inward.
    fn fades_on(
        cx: &mut VisualTestContext,
        viewport: Bounds<Pixels>,
    ) -> (Option<Pixels>, Option<Pixels>) {
        cx.update(|window, cx| {
            let surface: Hsla = rgb(palette(cx).panel).into();
            let toward = |angle: f32| {
                linear_gradient(
                    angle,
                    linear_color_stop(surface.opacity(0.), 0.),
                    linear_color_stop(surface, 1.),
                )
            };
            let (up, down) = (toward(0.), toward(180.));
            let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.5);
            let scale = window.scale_factor();
            let logical = |value: ScaledPixels| px(value.as_f32() / scale);
            let (mut top, mut bottom) = (None, None);
            for quad in window.painted_quads() {
                let (origin, extent) = (quad.bounds.origin, quad.bounds.size);
                let band = Bounds::new(
                    point(logical(origin.x), logical(origin.y)),
                    size(logical(extent.width), logical(extent.height)),
                );
                if !near(band.left(), viewport.left()) || !near(band.right(), viewport.right()) {
                    continue;
                }
                if quad.background == up && near(band.top(), viewport.top()) {
                    assert!(top.is_none(), "one fade along {viewport:?}'s top");
                    top = Some(band.size.height);
                } else if quad.background == down && near(band.bottom(), viewport.bottom()) {
                    assert!(bottom.is_none(), "one fade along {viewport:?}'s bottom");
                    bottom = Some(band.size.height);
                }
            }
            (top, bottom)
        })
    }

    /// The cue the message viewport drew in the last frame.
    #[derive(Debug, PartialEq)]
    enum Cue {
        Nothing,
        /// The heights of the fades along its top and its bottom.
        Fades(Option<Pixels>, Option<Pixels>),
        Bar,
    }

    /// The cue the message viewport drew in the last frame, checked
    /// against the rule for its height and its overflow: the always-visible
    /// bar, inside the viewport and the viewport's height, along a track a
    /// thumb moves on, where the message overflows (the bar's element is
    /// there, painting nothing, where it fits); otherwise no bar element
    /// and, where the message overflows a viewport at least one fade tall,
    /// a fade at the edge it continues past, a quarter of the viewport at
    /// most; nothing in a shorter sliver. Returns the cue and the viewport.
    fn drawn_cue(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        at: &str,
    ) -> (Cue, Bounds<Pixels>) {
        let panel = cx.debug_bounds("commit-message-panel").unwrap();
        let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
        let overflow = cx
            .read(|cx| app.read(cx).inspector_message.overflow())
            .unwrap();
        let tracks = tracks_on(cx, panel);
        let fades = fades_on(cx, viewport);
        let height = viewport.size.height;
        let thumb = height >= crate::tags::SHORTEST_SCROLL_TRACK;
        assert_eq!(
            cx.debug_bounds("commit-message-scrollbar").is_some(),
            thumb,
            "{at}: a bar element only along a track a thumb moves on, {viewport:?}"
        );
        for track in &tracks {
            assert_eq!(
                (track.top(), track.bottom(), track.right()),
                (viewport.top(), viewport.bottom(), viewport.right()),
                "{at}: {track:?} runs inside {viewport:?}"
            );
        }
        let fade = appearance::ui_size(crate::tags::EDGE_FADE);
        let cue = if !tracks.is_empty() {
            assert_eq!(fades, (None, None), "{at}: no fade beside the bar");
            Cue::Bar
        } else if fades != (None, None) {
            for band in [fades.0, fades.1].into_iter().flatten() {
                assert!(
                    (band - fade.min(height / 4.)).abs() < px(0.5),
                    "{at}: a fade {band:?} in {viewport:?}"
                );
            }
            Cue::Fades(fades.0, fades.1)
        } else {
            Cue::Nothing
        };
        let expected = if overflow <= px(0.) || (!thumb && height < fade) {
            "nothing"
        } else if thumb {
            "bar"
        } else {
            "fades"
        };
        let drawn = match cue {
            Cue::Nothing => "nothing",
            Cue::Fades(..) => "fades",
            Cue::Bar => "bar",
        };
        assert_eq!(
            drawn, expected,
            "{at}: {viewport:?} overflowing by {overflow:?}"
        );
        (cue, viewport)
    }

    /// The message cues its overflow inside its viewport, and as the rule
    /// for that viewport's height has it: an inspector whose 45% cap lies
    /// just past the toolbar row it reserves, 112 or 118 px tall at 18 pt,
    /// leaves the viewport a sliver 1.5 or 4 px tall over the long message,
    /// and its track drew a strip that tall and 16 px wide on the panel's
    /// bottom border; it draws nothing there. In 160 and 200 px inspectors
    /// the viewport, 23 and 41 px tall, is too short for the kit's 48 px
    /// thumb to move, and the message, which showed one or two cut lines
    /// with no cue, fades its bottom edge at its top and its top edge alone
    /// at its end; a 520 px inspector draws the bar the viewport's full
    /// height. In a History window at 18 pt in 461 × 490, where the message
    /// overflows a viewport a thumb moves along, at 13 pt in 1,480 × 800,
    /// where it fits, and in File History at 18 pt in 461 × 490, whose
    /// panel takes 45% of the shorter room between its header and footer,
    /// the cue follows the same rule: there the 15 px viewport fades its
    /// bottom edge, where its track drew 16 × 15 px.
    #[gpui::test]
    async fn the_message_cues_its_overflow_inside_its_viewport(cx: &mut TestAppContext) {
        let body = "A body long enough to run past the panel's cap. ".repeat(12);
        let (probe, app, short) = fixture(cx, vec![commit("a", "Selected title", &body)]);
        short.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        let scroll = short.read(|cx| {
            let state = app.read(cx).inspector_message.0.borrow();
            state
                .as_ref()
                .expect("the message is presented")
                .scroll
                .clone()
        });
        for (height, fades) in [
            (112., false),
            (118., false),
            (160., true),
            (200., true),
            (520., false),
        ] {
            probe.update(short, |probe, cx| {
                (probe.width, probe.height, probe.toolbar) = (156., height, true);
                cx.notify();
            });
            scroll.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            draw(short);
            let at = format!("a {height} px inspector at 18 pt");
            let (cue, viewport) = drawn_cue(short, &app, &at);
            if height < 150. {
                assert!(viewport.size.height > px(0.), "{at}: a sliver {viewport:?}");
                assert_eq!(cue, Cue::Nothing, "{at}: no strip");
            } else if fades {
                assert!(
                    (px(20.)..px(50.)).contains(&viewport.size.height),
                    "{at}: {viewport:?}"
                );
                assert!(
                    matches!(cue, Cue::Fades(None, Some(_))),
                    "{at}: at its top the message fades its bottom edge alone, not {cue:?}"
                );
                scroll.scroll_to_end();
                draw(short);
                let (cue, _) = drawn_cue(short, &app, &format!("{at}, scrolled to the end"));
                assert!(
                    matches!(cue, Cue::Fades(Some(_), None)),
                    "{at}: at its end the message fades its top edge alone, not {cue:?}"
                );
            } else {
                assert_eq!(cue, Cue::Bar, "{at}");
            }
        }
        short.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));

        let (_fixture, app, cx) = crate::views::tests::history_window_from(
            cx,
            crate::views::tests::history_script(2, 40),
        )
        .await;
        for (text, width, height, cue) in
            [(18, 461., 490., Cue::Bar), (13, 1480., 800., Cue::Nothing)]
        {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("History at {text} pt in {width} × {height}");
            assert_eq!(drawn_cue(cx, &app, &at).0, cue, "{at}");
        }
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        crate::file_history::tests::open_landed(&app, cx).await;
        draw(cx);
        // A viewport shorter than the shortest track, the message's first
        // line, over the overflowing message, where a 16 px track drew its
        // thumb with no room to move.
        let (cue, viewport) = drawn_cue(cx, &app, "File History at 18 pt in 461 × 490");
        assert!(
            viewport.size.height < crate::tags::SHORTEST_SCROLL_TRACK
                && matches!(cue, Cue::Fades(None, Some(_))),
            "File History at 18 pt in 461 × 490: {cue:?} in {viewport:?}"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// In an 18 pt 461 × 490 half tile, where Copy message keeps only its
    /// icon, Hash does too, and the commit's Actions its more-actions icon:
    /// three 28 px squares on one toolbar row, Actions at its trailing edge,
    /// which is the row the panel reserves, so the commit title starts one
    /// row below the toolbar's top. Hash keeps its name, and copied it shows
    /// only the check in its own bounds and is announced.
    #[gpui::test]
    fn compact_hash_keeps_the_copy_toolbar_one_row(cx: &mut TestAppContext) {
        use crate::copy_feedback::{
            RESTORE_AFTER,
            tests::{Shows, announced, click, clipboard, history, shows, wait},
        };
        let (app, cx) = history(cx, vec![commit("a", "Selected title", "Body.")]);
        crate::views::COMPACT_NAMES.with(|names| names.borrow_mut().clear());
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let square = appearance::ui_size(28.).ceil();
        let message = cx.debug_bounds("copy-message").unwrap();
        let hash = cx.debug_bounds("copy-commit").unwrap();
        let toolbar = cx.debug_bounds("commit-message-toolbar").unwrap();
        let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
        let rem = cx.update(|window, _| window.rem_size());
        assert_eq!(message.size.width, square, "{message:?}");
        assert_eq!(hash.size, message.size, "{hash:?}");
        assert_eq!(hash.top(), message.top(), "one row: {message:?} {hash:?}");
        assert!(message.right() < hash.left() && hash.right() <= toolbar.right());
        let actions = cx.debug_bounds("commit-recovery-menu").unwrap();
        assert_eq!(actions.size, message.size, "{actions:?}");
        assert_eq!(actions.top(), message.top(), "one row: {actions:?}");
        assert!(
            hash.right() < actions.left()
                && (actions.right() - (toolbar.right() - rem * 0.75)).abs() < px(0.5),
            "Actions at the trailing edge: {actions:?} in {toolbar:?}"
        );
        assert!(cx.debug_bounds("commit-recovery-more").is_some());
        assert_eq!(
            toolbar.size.height,
            message.size.height + rem * 0.5,
            "{toolbar:?}"
        );
        assert_eq!(viewport.top(), toolbar.bottom());
        // The reservation is that row and the panel's 1 px bottom border.
        let reserved = cx
            .read(|cx| app.read(cx).inspector_message.1.get())
            .expect("the window lays the toolbar out");
        assert!(reserved.reserve, "the window pins the inspector");
        let form = reserved.form(false);
        assert!(form.compact && !form.labelled_actions, "{form:?}");
        assert!(
            (form.height - (toolbar.size.height + px(1.))).abs() < px(0.5),
            "{form:?} for {toolbar:?}"
        );
        assert_eq!(
            crate::views::COMPACT_NAMES.with(|names| names.borrow().get("copy-commit").copied()),
            Some("Copy full commit hash")
        );

        click(cx, "copy-commit");
        assert_eq!(clipboard(cx), "a".repeat(40));
        assert_eq!(shows(cx, "copy-commit"), Shows::Copied);
        // The 16 px icon, as layout rounds it, and no "Copied".
        let check = cx.debug_bounds("copy-commit/copied").unwrap();
        let icon = appearance::ui_size(16.);
        assert!(
            (check.size.width - icon).abs() < px(1.),
            "only the check: {check:?}"
        );
        assert_eq!(cx.debug_bounds("copy-commit"), Some(hash));
        assert_eq!(cx.debug_bounds("commit-message-toolbar"), Some(toolbar));
        assert_eq!(announced(cx).unwrap().1, "Copied commit hash");
        wait(cx, RESTORE_AFTER);
        assert_eq!(shows(cx, "copy-commit"), Shows::Rest);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }
}
