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
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::prelude::FluentBuilder;
use std::cell::{Cell, RefCell};
use unicode_segmentation::UnicodeSegmentation;

const PIECE_BYTES: usize = 1024;
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
    focus: FocusHandle,
    selection: Selection,
    scale: f32,
}

#[derive(Default)]
pub(super) struct State(RefCell<Option<Presentation>>, Cell<Option<Toolbar>>);

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
    /// and the panel's 1 px bottom border is reserved with them. `pinned`: the
    /// window pins the inspector.
    pub(super) fn within(width: Pixels, pinned: bool, window: &Window) -> Self {
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
        let room = width - px(1.) - rem * 1.5;
        let gap = rem * 0.25;
        let padding = appearance::ui_size(2. * 10.);
        let square = appearance::ui_size(28.);
        let copy = padding + (icon + copy_label).max(copied);
        let hash = padding + (icon + hash_label).max(copied);
        let actions =
            (padding + label(&ACTIONS_LABEL, "Actions") + appearance::ui_size(6.) + rem * 0.875)
                .ceil();
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
            let focus = cx.focus_handle();
            *retained = Some(Presentation {
                oid: commit.oid.clone(),
                scroll: ListState::new(pieces.len() + 1, ListAlignment::Top, px(100.))
                    .with_uniform_item_height(appearance::ui_size(24.)),
                selection: Selection::new(pieces.clone(), message_len, focus.clone(), cx),
                pieces,
                focus,
                scale,
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

fn render_piece(piece: &Piece, index: usize, selection: &Selection, cx: &App) -> impl IntoElement {
    let colors = palette(cx);
    div()
        .id(("commit-message-piece", index))
        .role(Role::Label)
        // AccessKit adapters expose static label text through its value.
        .aria_value(piece.text.clone())
        .w_full()
        .min_w_0()
        .px_3()
        .pr(appearance::ui_size(20.))
        .when(index == 0, |row| row.pt_2())
        .when(piece.body_start && index > 0, |row| row.pt_3())
        .text_color(rgb(colors.text))
        .text_size(appearance::ui_text(if piece.title { 15. } else { 12. }))
        .line_height(relative(if piece.title { 1.35 } else { 1.5 }))
        .when(piece.title, |row| row.font_weight(FontWeight::MEDIUM))
        .min_h(appearance::ui_size(if piece.title { 20.25 } else { 18. }))
        .child(selection.piece(piece))
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

    pub(super) fn render_commit_message(
        &self,
        commit: &Commit,
        file_history: bool,
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
        let actions = (!file_history).then(|| {
            self.render_commit_recovery_menu(
                commit,
                form.is_none_or(|form| form.labelled_actions),
                cx,
            )
        });
        let (pieces, scroll, focus, selection) = self.inspector_message.presentation(commit, cx);
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
        .size_full();
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
            .role(Role::Group)
            .aria_label("Selected commit message and details")
            .min_w_0()
            .h(relative(0.45))
            .max_h(relative(0.45))
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
                    compact_control_name(copy_feedback::copy_button(hash_mark, "", format!("Hash · Copy full commit hash · {}", commit.oid), cx), "copy-commit", "Copy full commit hash")
                } else {
                    copy_feedback::copy_button(hash_mark, "Hash", format!("Copy full commit hash · {}", commit.oid), cx)
                        .accessibility_label("Copy full commit hash")
                }
                    .on_click(move |_, window, cx| copy_feedback::copy(window, cx, Some(hash_copy.clone()), "commit hash", hash.clone())))
                // At the trailing edge of the copy controls' last row, above
                // the scrolling message and metadata, so it stays in view.
                .children(actions)
            )
            .child(div()
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
                .relative().flex_1().min_h_0().min_w_0().overflow_hidden()
                .child(list)
                .child(selection.registrar(scroll.clone()))
                .child(Scrollbar::vertical(&scroll).id("commit-message-scrollbar").mode(ScrollbarMode::Always))
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
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let inspector = div()
                .id("inspector-test")
                .debug_selector(|| "inspector-test".into())
                .w(px(self.width))
                .h(px(self.height))
                .child(self.app.update(cx, |app, cx| {
                    app.render_inspector(crate::views::FilesHeader::WHOLE, cx)
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
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
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
        cx.update(|_, cx| {
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
                app.refresh_file_filter(cx);
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
        cx.update(|_, cx| {
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
                app.refresh_file_filter(cx);
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
