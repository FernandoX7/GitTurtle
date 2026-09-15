//! Retained, bounded presentation of an already-loaded commit message.
//!
//! This is inspection state, moved with ReturnContext (including warm tabs),
//! not a second message store or a repository reader. Only a newly selected
//! immutable OID prepares text pieces; paints clone shared bounded pieces.
use crate::*;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::prelude::FluentBuilder;
use std::cell::RefCell;
use unicode_segmentation::UnicodeSegmentation;

const PIECE_BYTES: usize = 1024;
const PIECE_LINES: usize = 16;

struct Piece {
    text: SharedString,
    title: bool,
    body_start: bool,
}

struct Presentation {
    oid: String,
    pieces: Arc<Vec<Piece>>,
    scroll: ListState,
    focus: FocusHandle,
    scale: f32,
}

#[derive(Default)]
pub(super) struct State(RefCell<Option<Presentation>>);

impl State {
    pub(super) fn select(&self, oid: &str) {
        let mut state = self.0.borrow_mut();
        if state.as_ref().is_some_and(|state| state.oid != oid) {
            *state = None;
        }
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.0.borrow().as_ref().map_or(0, |state| {
            state.oid.capacity()
                + state.pieces.iter().map(|piece| piece.text.len()).sum::<usize>()
                // Reserve list tree/measurement bookkeeping as well as pieces.
                + state.pieces.capacity() * (std::mem::size_of::<Piece>() + 256)
        })
    }

    fn presentation(
        &self,
        commit: &Commit,
        cx: &mut App,
    ) -> (Arc<Vec<Piece>>, ListState, FocusHandle) {
        let mut retained = self.0.borrow_mut();
        let scale = appearance::ui_scale();
        if retained
            .as_ref()
            .is_none_or(|state| state.oid != commit.oid)
        {
            let mut pieces = Vec::new();
            append_pieces(&mut pieces, &commit.subject, true);
            append_pieces(&mut pieces, &commit.body, false);
            *retained = Some(Presentation {
                oid: commit.oid.clone(),
                scroll: ListState::new(pieces.len() + 1, ListAlignment::Top, px(100.))
                    .with_uniform_item_height(appearance::ui_size(24.)),
                pieces: Arc::new(pieces),
                focus: cx.focus_handle(),
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
        )
    }
}

// Prefer source line boundaries, then whitespace, then grapheme boundaries.
// Consecutive pieces continue a paragraph without extra vertical padding.
// A single maliciously large grapheme can itself exceed the shaping budget;
// split that exceptional cluster at UTF-8 boundaries. Literal copy never uses
// this display segmentation and remains byte-for-byte the loaded model.
fn append_pieces(pieces: &mut Vec<Piece>, mut text: &str, title: bool) {
    let mut first = true;
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
            text: display.to_owned().into(),
            title,
            body_start: !title && first,
        });
        first = false;
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

fn render_piece(piece: &Piece, index: usize, cx: &App) -> impl IntoElement {
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
        .child(piece.text.clone())
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
        let (pieces, scroll, focus) = self.inspector_message.presentation(commit, cx);
        let owner = cx.entity().downgrade();
        let oid = commit.oid.clone();
        let list = gpui_kit::list(scroll.clone(), move |index, _, cx| {
            if let Some(piece) = pieces.get(index) {
                return render_piece(piece, index, cx).into_any_element();
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
                        this.render_revision_metadata(this.file_history_entry().unwrap(), cx)
                    } else {
                        this.render_commit_metadata(this.message_commit(false).unwrap(), cx)
                    }
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        let copy_oid = commit.oid.clone();
        let hash = commit.oid.clone();
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
            .child(div()
                .id("commit-message-toolbar")
                .flex_shrink_0()
                .px_3().py_1()
                .flex().flex_wrap().items_center().gap_1()
                .child(button("copy-message", "Copy message", "copy", false).debug_selector(|| "copy-message".into())
                    .accessibility_label("Copy full commit message")
                    .tooltip("Copy the complete commit title and body")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(commit) = this.message_commit(file_history).filter(|commit| commit.oid == copy_oid) {
                            cx.write_to_clipboard(ClipboardItem::new_string(full_message(commit)));
                        }
                    })))
                .child(button("copy-commit", "Hash", "", false).debug_selector(|| "copy-commit".into())
                    .accessibility_label("Copy full commit hash")
                    .tooltip(format!("Copy full commit hash · {}", commit.oid))
                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(hash.clone()))))
            )
            .child(div()
                .id("commit-message-viewport").debug_selector(|| "commit-message-viewport".into())
                .role(Role::Document)
                .aria_label("Commit message. Use arrow keys, Page Up, Page Down, Home or End to scroll.")
                .tab_stop(true)
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| click_focus.focus(window, cx))
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
            append_pieces(&mut pieces, &source, true);
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
        append_pieces(&mut pieces, &source, false);
        let mut offset = 0;
        for piece in pieces {
            assert!(boundaries.contains(&offset));
            offset += piece.text.len();
        }
    }

    #[test]
    fn many_source_lines_keep_paragraph_spacing_with_bounded_shaping() {
        let source = (0..500)
            .map(|i| format!("Paragraph {i}: 東京.\n\n"))
            .collect::<String>();
        let mut pieces = Vec::new();
        append_pieces(&mut pieces, &source, false);
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
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("inspector-test")
                .debug_selector(|| "inspector-test".into())
                .w(px(self.width))
                .h(px(self.height))
                .child(self.app.update(cx, |app, cx| app.render_inspector(cx)))
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
                    text: text.to_owned().into(),
                    title: index == 0,
                    body_start: index == 1,
                };
                let element = render_piece(&piece, index, cx).into_element();
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
                let (_, scroll, _) = app.inspector_message.presentation(&app.commits[0], cx);
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
                let (_, restored, _) = app.inspector_message.presentation(&app.commits[0], cx);
                assert_eq!(
                    (
                        restored.logical_scroll_top().item_ix,
                        restored.logical_scroll_top().offset_in_item
                    ),
                    (before.item_ix, before.offset_in_item)
                );
                app.select_commit(1, window, cx);
                let (_, next, _) = app.inspector_message.presentation(&app.commits[1], cx);
                assert_eq!(next.logical_scroll_top().item_ix, 0);
                assert_eq!(next.logical_scroll_top().offset_in_item, px(0.));
                app.select_commit(0, window, cx);
                let (_, previous, _) = app.inspector_message.presentation(&app.commits[0], cx);
                assert_eq!(previous.logical_scroll_top().item_ix, 0);
            })
        });
    }
}
