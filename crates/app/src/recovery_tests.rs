use super::*;
use ::core::prelude::v1::test;
#[test]
fn a_press_of_refresh_says_the_page_it_landed_on() {
    assert_eq!(spoken_page(0, None, 0), "No saved stashes");
    assert_eq!(spoken_page(0, None, 1), "1 saved stash");
    assert_eq!(spoken_page(0, None, 3), "3 saved stashes");
    // A page of one past the first, which once said "41 to 41".
    assert_eq!(spoken_page(40, None, 1), "Saved stash 41");
    assert_eq!(spoken_page(40, Some(80), 40), "Saved stashes 41 to 80");
    assert_eq!(spoken_page(0, Some(40), 40), "Saved stashes 1 to 40");
}
#[test]
fn amend_fields_preserve_description_whitespace_and_comment_lines() {
    let message = "Title\n\n  indented\n# literal description\n\n";
    let draft = split_message(message);
    assert_eq!(draft.title, "Title");
    assert_eq!(draft.description, "  indented\n# literal description\n\n");
    assert_eq!(draft.message(), message);
}
#[test]
fn legacy_single_newline_messages_split_without_losing_body_text() {
    assert_eq!(
        split_message("Title\nbody\n"),
        CommitDraft {
            title: "Title".into(),
            description: "body\n".into()
        }
    );
    assert_eq!(
        split_message("Only title"),
        CommitDraft {
            title: "Only title".into(),
            description: String::new()
        }
    );
}

#[test]
fn unchanged_amend_preserves_exact_legacy_message_and_edits_use_blank_separator() {
    let original = "Title\n  body\n# literal\n";
    let initial = split_message(original);
    assert_eq!(amended_message(original, &initial, &initial), original);
    let edited = CommitDraft {
        title: "Revised title".into(),
        description: initial.description.clone(),
    };
    assert_eq!(
        amended_message(original, &initial, &edited),
        "Revised title\n\n  body\n# literal\n"
    );
}

/// The stash inspector's unified patch keeps its decorations for the editor's life, and
/// its preview figure counts them along with the presentation they were built from.
#[gpui::test]
async fn stash_patch_preview_counts_its_decorations_and_presentation(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let fixture = tempfile::tempdir().unwrap();
    let repository = GitRepository::init(fixture.path().join("repository"), "main").unwrap();
    cx.update(gpui_kit::init);
    let captured: std::rc::Rc<std::cell::RefCell<Option<Entity<StashBrowser>>>> =
        Default::default();
    let output = captured.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let browser =
            cx.new(|cx| StashBrowser::new(WeakEntity::new_invalid(), repository, window, cx));
        *output.borrow_mut() = Some(browser.clone());
        gpui_kit::component::Root::new(browser, window, cx)
    });
    cx.run_until_parked();
    let browser = captured.borrow().as_ref().unwrap().clone();
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.content = Some(Arc::new(text::decorated_patch_content()));
            browser.text_mode = 0;
            browser.ensure_editor(window, cx);
            let Some(Content::Text { presentation, .. }) = browser.content.as_deref() else {
                unreachable!()
            };
            let presentation = presentation.retained_bytes();
            let decoration = browser.decoration.clone().expect("kept with the editor");
            let decorated = decoration.retained_bytes();
            assert!(decorated > 0, "a patch with changes has decorations");

            let with = browser.preview_retained_bytes();
            browser.decoration = None;
            let without = browser.preview_retained_bytes();
            assert_eq!(with - without, decorated);
            browser.content = None;
            assert!(without - browser.preview_retained_bytes() >= presentation);

            browser.decoration = Some(decoration);
            browser.clear_preview();
            assert!(browser.decoration.is_none());
            assert_eq!(browser.preview_retained_bytes(), 0);
        })
    });
}

#[test]
fn stash_list_navigation_bounds_empty_and_changed_pages() {
    assert_eq!(list_key_selection("down", None, 0), None);
    assert_eq!(list_key_selection("down", None, 4), Some(0));
    assert_eq!(list_key_selection("up", Some(0), 4), Some(0));
    assert_eq!(list_key_selection("down", Some(3), 4), Some(3));
    assert_eq!(list_key_selection("up", Some(39), 4), Some(3));
    assert_eq!(list_key_selection("end", None, 4), Some(3));
    assert_eq!(list_key_selection("home", Some(3), 4), Some(0));
    assert_eq!(list_key_selection("escape", Some(3), 4), None);
}

/// In a 461 × 490 half tile, at 13 and 18 pt, with a stash selected,
/// every Tab stop of the Stashes dialog draws its ring whole and in view:
/// the detail scrolls the control the keyboard moves onto into view with
/// its ring, as the dialogs' lists do. Its group, mode and restore
/// controls once stayed below the detail's fold, drawing no ring.
#[gpui::test]
async fn a_narrow_stash_detail_shows_every_stop_whole(cx: &mut gpui_kit::TestAppContext) {
    use crate::focus_ring::sweep;
    use crate::tags::tests::{git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
    git(
        repo.path(),
        &[
            "stash",
            "push",
            "--include-untracked",
            "-m",
            "kept for later",
        ],
    );
    let (app, cx) = window(cx, &repo);
    sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    let mut failures = Vec::new();
    for interface in sweep::SIZES {
        sweep::interface_size(cx, interface);
        // The saved stashes, then the selected stash's inspection, are
        // read on the browser's own thread, which its opener waits for
        // (`StashBrowser::lands`).
        sweep::open_dialog(&app, cx, "Stashes", |app, window, cx| {
            app.open_stash_browser(window, cx)
        })
        .await;
        let (browser, _) = app
            .read_with(cx, super::open_stash_list)
            .expect("Stashes is open");
        let row = cx
            .debug_bounds("saved-stash-row-0")
            .expect("the saved stash is drawn");
        // A click focuses the list; Home selects the stash from the
        // keyboard, as a person walking it with keys would.
        cx.simulate_click(row.center(), Modifiers::none());
        cx.simulate_keystrokes("home");
        StashBrowser::lands(&browser, cx);
        assert!(
            cx.debug_bounds("stash-area-0").is_some(),
            "the selected stash's inspection is drawn"
        );
        sweep::walk(&app, cx, "Stashes", interface, &mut failures);
        sweep::close_dialogs(cx, "Stashes", interface, &mut failures);
    }
    sweep::assert_whole(failures);
}

/// Stashes has one default button: with a stash selected, Review
/// restore… is the filled primary, and the footer's Close takes Cancel's
/// bordered style, the dialog's edge on its `subtle` fill, at Review
/// restore…'s 28 px, in a dark and a light palette. Close drew a second
/// filled primary beside it.
#[gpui::test]
async fn stashes_close_is_bordered_beside_the_filled_restore(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    use crate::views::tests::{painted_edges, painted_fills};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
    git(
        repo.path(),
        &[
            "stash",
            "push",
            "--include-untracked",
            "-m",
            "kept for later",
        ],
    );
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(1000.), px(680.)));
    draw(cx);
    // The saved stashes, then the selected stash's inspection, are read
    // on the browser's own threads.
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    landed(&app, cx, "saved-stash-row-0");
    cx.executor()
        .advance_clock(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    cx.simulate_keystrokes("home");
    landed(&app, cx, "restore-saved-stash");
    let palettes: Vec<_> = appearance::test_palettes()
        .into_iter()
        .filter(|(name, _)| ["Midnight", "Braden"].contains(&name.as_str()))
        .collect();
    assert_eq!(palettes.len(), 2);
    for (name, palette) in palettes {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        draw(cx);
        draw(cx);
        let accent = gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(palette.accent)));
        assert!(
            painted_fills(cx, "restore-saved-stash").contains(&accent),
            "{name}: Review restore… is the filled primary"
        );
        let close = "dialog-ok: Close";
        let fills = painted_fills(cx, close);
        assert!(
            !fills.contains(&accent),
            "{name}: Close is not a second filled primary: {fills:?}"
        );
        assert_eq!(
            painted_edges(cx, close),
            vec![gpui_kit::Hsla::from(rgb(palette.dialog_edge()))],
            "{name}: Close draws the dialog's edge, as Cancel does"
        );
        let (close, restore) = (
            cx.debug_bounds(close).expect("Close"),
            cx.debug_bounds("restore-saved-stash")
                .expect("Review restore…"),
        );
        assert!(
            (close.size.height - restore.size.height).abs() < px(0.01),
            "{name}: Close keeps the compact frame's height: {close:?} beside {restore:?}"
        );
    }
}

/// A merge's "Parent n" choices are equal, so the mainline dialog fills
/// none of them, and its Cancel, which only dismisses it, is bordered in
/// the dialog's edge (`tags::dialog_dismiss_footer`), in the bounds a
/// filled Cancel takes in the same dialog, at 13 and 18 pt, in Midnight
/// and Daylight. The dialog filled Cancel as though dismissing were the
/// merge's default.
#[gpui::test]
async fn the_mainline_dialog_fills_no_choice_and_borders_cancel(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, tagged_repository, window};
    use crate::views::tests::{painted_edges, painted_fills};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(1000.), px(680.)));
    draw(cx);
    let merge = Arc::new(CommitTarget {
        oid: "1".repeat(40),
        parents: vec!["2".repeat(40), "3".repeat(40)],
    });
    const CANCEL: &str = "dialog-ok: Cancel";
    let cancel = |cx: &mut VisualTestContext| cx.debug_bounds(CANCEL).expect("Cancel");
    let mut failures = Vec::new();
    for text in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        for theme in [
            appearance::ThemeChoice::Midnight,
            appearance::ThemeChoice::Daylight,
        ] {
            let palette = theme.palette();
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            let at = format!("{} at {text} pt", theme.label());
            let accent = gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(palette.accent)));
            // The same dialog's frame with the filled Cancel it had.
            cx.update(|window, cx| {
                window.open_alert_dialog(cx, |dialog, window, _| {
                    dialog
                        .title("Choose the mainline parent")
                        .width(appearance::dialog_width(px(560.), window))
                        .footer(crate::tags::dialog_footer("Cancel", None))
                })
            });
            draw(cx);
            assert!(
                painted_fills(cx, CANCEL).contains(&accent),
                "{at}: the reference Cancel is filled"
            );
            let filled = cancel(cx);
            cx.update(|window, cx| window.close_dialog(cx));
            draw(cx);

            let merge = merge.clone();
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.choose_commit_recovery(RecoveryKind::Revert, merge, window, cx)
                })
            });
            draw(cx);
            draw(cx);
            for parent in ["recovery-mainline-0", "recovery-mainline-1"] {
                let fills = painted_fills(cx, parent);
                if fills.contains(&accent) {
                    failures.push(format!("{at}: {parent} is filled: {fills:?}"));
                }
            }
            let fills = painted_fills(cx, CANCEL);
            if fills.contains(&accent) {
                failures.push(format!("{at}: Cancel is filled: {fills:?}"));
            }
            let edges = painted_edges(cx, CANCEL);
            if edges != [gpui_kit::Hsla::from(rgb(palette.dialog_edge()))] {
                failures.push(format!("{at}: Cancel's edges {edges:?}"));
            }
            // The two dialogs' bodies differ, so only their footers'
            // vertical places do.
            let bordered = cancel(cx);
            if bordered.size != filled.size || bordered.left() != filled.left() {
                failures.push(format!(
                    "{at}: Cancel {bordered:?}, where the filled one was {filled:?}"
                ));
            }
            cx.update(|window, cx| window.close_dialog(cx));
            draw(cx);
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// At 18 pt in a 1000 × 680 window the Stashes detail shows Restore
/// staged state, Drop… and Review restore… whole as a stash is selected,
/// without scrolling, and keeps them there, its footer, as a chosen
/// file's comparison takes its floor and the detail scrolls: they lay
/// below the scrolling detail's fold, as the empty comparison kept the
/// floor a file's takes. Until a file is chosen the comparison keeps its
/// prompt's height. Stacked in a 461 × 490 window, where the detail is a
/// few lines tall, the actions scroll with the rest.
#[gpui::test]
async fn the_stash_actions_stay_in_view_beside_the_list(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, scroll_down, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
    git(
        repo.path(),
        &[
            "stash",
            "push",
            "--include-untracked",
            "-m",
            "kept for later",
        ],
    );
    let (app, cx) = window(cx, &repo);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("no {selector}"))
    };
    let slack = px(0.5);
    for (width, height, pinned) in [(1000., 680., true), (461., 490., false)] {
        let at = format!("18 pt in {width} × {height}");
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
        landed(&app, cx, "saved-stash-row-0");
        let row = bounds(cx, "saved-stash-row-0");
        cx.simulate_click(row.center(), Modifiers::none());
        landed(&app, cx, "stash-actions");
        // Until a file is chosen, the comparison is its prompt's height.
        let comparison = bounds(cx, "stash-comparison");
        let floor = appearance::ui_size(STASH_CONTENT_FLOOR);
        // Layout snaps the floor to whole pixels.
        assert!(
            comparison.size.height < floor - px(1.),
            "{at}: the empty comparison {comparison:?} keeps a file's floor"
        );
        let browser = bounds(cx, "stash-browser");
        let detail = bounds(cx, "stash-detail");
        let viewport = cx.update(|window, _| window.viewport_size());
        let whole = |actions: Bounds<Pixels>| {
            actions.top() >= detail.top() - slack
                && actions.bottom() <= browser.bottom().min(viewport.height) + slack
        };
        let actions = bounds(cx, "stash-actions");
        assert_eq!(whole(actions), pinned, "{at}: the actions {actions:?}");
        if pinned {
            // The footer, below the detail's scrolling part.
            assert!(
                actions.top() >= detail.bottom(),
                "{at}: {actions:?} in {detail:?}"
            );
            let file = bounds(cx, "stash-file-0");
            cx.simulate_click(file.center(), Modifiers::none());
            landed(&app, cx, "stash-comparison");
            let comparison = bounds(cx, "stash-comparison");
            assert!(
                comparison.size.height >= floor - slack,
                "{at}: the chosen file's comparison {comparison:?}"
            );
        }
        // Scrolled to its end, the detail moves its actions only where
        // they are not its footer.
        let before = bounds(cx, "stash-actions");
        let delta = ScrollDelta::Pixels(point(px(0.), px(-10_000.)));
        let middle = bounds(cx, "stash-detail").center();
        scroll_down(cx, middle, delta);
        draw(cx);
        let after = bounds(cx, "stash-actions");
        assert_eq!(after == before, pinned, "{at}: {before:?}, then {after:?}");
        if pinned {
            assert!(whole(after), "{at}: the scrolled actions {after:?}");
        }
        cx.simulate_keystrokes("escape");
        draw(cx);
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// Stacked in a 461 × 490 window, the Stashes dialog's saved-work list
/// shows its first stash whole at 13 and 18 pt: its unscaled height left
/// the list 35.6 px at 18 pt, short of an 80 px row. Every line the
/// dialog draws lies whole inside its clip, and inside its pane, the
/// list's or the detail's, where the browser holds it, and the detail's
/// prompt is one whole line: at 18 pt the dialog's scrolling body cut
/// "Choose a stash… / unstaged and untracked files." mid-line, as the
/// browser asked for more height than the dialog's cap left it, and at
/// 13 pt the prompt left "files." alone on its last line.
#[gpui::test]
async fn a_narrow_stash_list_keeps_its_first_row_whole(cx: &mut gpui_kit::TestAppContext) {
    use crate::empty_state::tests::{marked_runs, text_mark};
    use crate::views::tests::{draw, fixture_git, history_script, styled_history_window};
    let (_fixture, app, cx) = styled_history_window(cx, history_script(1, 1).as_bytes(), |root| {
        root.text_bg(text_mark())
    })
    .await;
    let root = app.read_with(cx, |app, _| app.path.clone().unwrap());
    fixture_git(&root, &["config", "user.name", "Stash Fixture"]);
    fixture_git(&root, &["config", "user.email", "stash@example.invalid"]);
    std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
    fixture_git(&root, &["stash", "push", "-m", "kept for later"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    landed(&app, cx, "saved-stash-row-0");
    cx.executor()
        .advance_clock(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
    for interface in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let at = format!("{interface} pt in 461 × 490");
        let list = cx.debug_bounds("stash-list").expect("the saved-work list");
        let row = cx
            .debug_bounds("saved-stash-row-0")
            .expect("the saved stash's row");
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };
        assert!(
            row.top() >= list.top() - slack && row.bottom() <= list.bottom() + slack,
            "{at}: the row {row:?} is cut by the list {list:?}"
        );
        let [browser, panel, detail, prompt] = [
            "stash-browser",
            "stash-list-panel",
            "stash-detail",
            "stash-detail-placeholder",
        ]
        .map(|name| {
            cx.debug_bounds(name)
                .unwrap_or_else(|| panic!("{at}: no {name}"))
        });
        let viewport = cx.update(|window, _| window.viewport_size());
        let runs = marked_runs(cx, Bounds::new(Point::default(), viewport));
        for run in &runs {
            assert!(
                !run.cut,
                "{at}: the line {:?} crosses its clip {:?}",
                run.drawn, run.clip
            );
            assert!(
                !browser.contains(&run.drawn.center())
                    || inside(run.drawn, panel)
                    || inside(run.drawn, detail),
                "{at}: the line {:?} leaves its pane, the list {panel:?} or the detail {detail:?}",
                run.drawn
            );
        }
        // The browser ends above the dialog's last line, Close in its
        // footer, rather than past the fold of a body that scrolls it.
        let footer = runs
            .iter()
            .map(|run| run.drawn)
            .max_by(|a, b| a.top().partial_cmp(&b.top()).unwrap())
            .unwrap();
        assert!(
            browser.bottom() <= footer.top(),
            "{at}: the browser {browser:?} ends past the footer's line {footer:?}"
        );
        let lines = runs
            .iter()
            .filter(|run| prompt.contains(&run.drawn.center()))
            .count();
        assert!(
            lines == 1 && inside(prompt, detail),
            "{at}: the prompt {prompt:?} is {lines} drawn lines in the detail {detail:?}"
        );
    }
}

/// A stashed document's comparison, at the floor the Stashes dialog keeps
/// it (a scaled 120 px), says why a click on Open in default app opened
/// nothing on one whole line beneath its header's controls, its whole
/// answer in the line's tooltip, and the page beneath gives way below its
/// minimum for it: at 18 pt in a 1000 × 680 window, the detail scrolled
/// to the comparison, the room beneath the header held no line above that
/// minimum, so the line drew none and the click changed nothing on screen.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_stashed_document_says_why_on_one_whole_line(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, scroll_down, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pdf =
        |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
    let document = repo.path().join("document.pdf");
    std::fs::write(&document, pdf("first")).unwrap();
    git(repo.path(), &["add", "document.pdf"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add a document"]);
    std::fs::write(&document, pdf("second")).unwrap();
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(1000.), px(680.)));
    draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    landed(&app, cx, "saved-stash-row-0");
    cx.executor()
        .advance_clock(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-0");
    let file = cx.debug_bounds("stash-file-0").unwrap();
    cx.simulate_click(file.center(), Modifiers::none());
    landed(&app, cx, "system-preview-1");
    // The detail scrolls; at its end the comparison is in view.
    let middle = cx.debug_bounds("stash-detail").unwrap().center();
    scroll_down(cx, middle, ScrollDelta::Pixels(point(px(0.), px(-10_000.))));
    draw(cx);
    crate::system_preview::tests::a_failed_open_answers_on_one_line(
        cx,
        "the Stashes dialog at 18 pt in 1000 × 680",
        [
            "system-preview-1",
            "system-preview-1/note",
            "rich-sides/body",
        ],
        &["stash-comparison", "stash-detail"],
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A saved stash stays selected by its stash commit as the list is read
/// again: Refresh after a stash saved from a terminal, which renumbers
/// the list, keeps the stash that was selected selected one row down,
/// its saved changes read again and its row's node the same, by the same
/// name; Refresh after it was dropped selects nothing. Refresh cleared
/// the selection, and the row's node took the stash that moved into its
/// `stash@{n}`.
#[gpui::test]
async fn a_refresh_keeps_the_selected_stash_by_its_commit(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let notes = repo.path().join("notes.txt");
    for message in ["first", "second"] {
        std::fs::write(&notes, message).unwrap();
        git(
            repo.path(),
            &["stash", "push", "--include-untracked", "-m", message],
        );
    }
    let (app, cx) = window(cx, &repo);
    draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    landed(&app, cx, "saved-stash-row-1");
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("the Stashes dialog opens");
    cx.update(|window, cx| stashes.update(cx, |this, cx| this.select_stash(1, window, cx)));
    StashBrowser::lands(&stashes, cx);
    let selected = |cx: &mut VisualTestContext| {
        stashes.read_with(cx, |this, _| {
            let stash = this
                .selected
                .and_then(|index| this.page.as_ref()?.entries.get(index));
            (this.selected, stash.map(|stash| stash.oid.clone()))
        })
    };
    let (row, first) = selected(cx);
    assert_eq!(row, Some(1), "the first stash saved is selected");
    let oid = first.expect("the selected stash");
    // The accessibility node the stash's row writes, by the row's ID: its
    // stash commit, wherever its `stash@{n}` puts it.
    let id = stashes.read_with(cx, |this, _| {
        stash_row_id(&this.page.as_ref().expect("the list").entries[1]).to_string()
    });
    let node = |cx: &mut VisualTestContext| {
        crate::native_accessibility::written::forget();
        draw(cx);
        crate::native_accessibility::written::get(&id)
            .and_then(|written| written.node.label().map(str::to_owned))
    };
    let named = node(cx).unwrap_or_else(|| panic!("the stash's row writes its node {id}"));
    assert_eq!(
        named,
        stash_row_name(&stashes.read_with(cx, |this, _| {
            this.page.as_ref().expect("the list").entries[1].clone()
        })),
        "named by its message, date and commit"
    );
    let (date, commit) = stashes.read_with(cx, |this, _| {
        let stash = &this.page.as_ref().expect("the list").entries[1];
        (short_date(stash.timestamp), short_oid(&stash.oid))
    });
    assert!(
        named.contains(": first · ") && named.ends_with(&format!(" · {date} · {commit}")),
        "message · date · short commit: {named}"
    );
    // Another saved from a terminal, above it.
    std::fs::write(&notes, "third").unwrap();
    git(
        repo.path(),
        &["stash", "push", "--include-untracked", "-m", "third"],
    );
    cx.update(|window, cx| stashes.update(cx, |this, cx| this.load_page(0, window, cx)));
    StashBrowser::lands(&stashes, cx);
    let (row, kept) = selected(cx);
    assert_eq!(
        row,
        Some(2),
        "the stash moved one row down and stays selected"
    );
    assert_eq!(kept, Some(oid.clone()), "by its commit");
    let renamed = node(cx).unwrap_or_else(|| panic!("the stash's row keeps its node {id}"));
    assert_eq!(
        renamed, named,
        "the row's node stays with the stash, renumbered, by the same name"
    );
    assert!(
        stashes.read_with(cx, |this, _| this
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.stash.oid == oid)),
        "its saved changes are read again"
    );
    git(repo.path(), &["stash", "drop", "-q", "stash@{2}"]);
    cx.update(|window, cx| stashes.update(cx, |this, cx| this.load_page(0, window, cx)));
    StashBrowser::lands(&stashes, cx);
    assert_eq!(
        selected(cx),
        (None, None),
        "a dropped stash leaves nothing selected"
    );
}

/// Focus on the chosen stash's inspection, which a cleared snapshot
/// stops drawing, moves to the stash list, the nearest control of the
/// same list still drawn, and never stays on a control no frame draws.
/// With Drop… focused in the footer beside the list, Refresh leaves the
/// list focused while Git reads it and once it lands, its stash's row
/// selected and shown; focus on Refresh itself stays where it is; and
/// with the saved files focused, Refresh after a terminal dropped the
/// stash leaves the list focused with nothing selected. Focus stayed on
/// the saved files and on Drop…, which no frame drew while the list was
/// read, and with the stash gone, on nothing the keys could reach.
#[gpui::test]
async fn a_cleared_inspection_gives_its_focus_to_the_stash_list(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let notes = repo.path().join("notes.txt");
    for message in ["first", "second", "third"] {
        std::fs::write(&notes, message).unwrap();
        git(
            repo.path(),
            &["stash", "push", "--include-untracked", "-m", message],
        );
    }
    let (app, cx) = window(cx, &repo);
    // Beside the list, the restore actions are the detail's footer,
    // outside its scroll.
    cx.simulate_resize(size(px(1480.), px(980.)));
    draw(cx);
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    let (list, files) = stashes.read_with(cx, |this, _| {
        (this.stash_focus.clone(), this.file_focus.clone())
    });
    let on = |cx: &mut VisualTestContext, focus: &FocusHandle| {
        cx.update(|window, _| focus.is_focused(window))
    };
    let refresh = |cx: &mut VisualTestContext| {
        let held = stashes.read_with(cx, |this, _| this.metadata.hold());
        cx.update(|window, cx| stashes.update(cx, |this, cx| this.load_page(0, window, cx)));
        draw(cx);
        held
    };

    // Drop…, in the footer beside the list.
    tab_to(&stashes, "drop-saved-stash", cx);
    let held = refresh(cx);
    assert!(
        stashes.read_with(cx, |this, _| this.snapshot.is_none()),
        "the inspection is cleared while the list is read"
    );
    assert!(on(cx, &list), "the list takes focus while it is read");
    drop(held);
    StashBrowser::lands(&stashes, cx);
    assert!(on(cx, &list), "and keeps it once the list lands");
    assert_eq!(
        stashes.read_with(cx, |this, _| (this.selected, this.snapshot.is_some())),
        (Some(0), true),
        "its stash stays selected and inspected"
    );
    let (row, shown) = (
        cx.debug_bounds("saved-stash-row-0")
            .expect("its row is drawn"),
        cx.debug_bounds("stash-list").expect("the list is drawn"),
    );
    assert!(
        row.top() >= shown.top() - px(0.5) && row.bottom() <= shown.bottom() + px(0.5),
        "its row {row:?} is shown in the list {shown:?}"
    );

    // Focus outside the inspection, on Refresh, stays there.
    cx.simulate_keystrokes("shift-tab");
    draw(cx);
    let before = cx.update(|window, cx| window.focused(cx));
    let in_detail = cx.update(|window, cx| stashes.read(cx).detail.focused(window));
    assert!(
        before
            .as_ref()
            .is_some_and(|focus| *focus != list && *focus != files)
            && in_detail.is_none(),
        "Shift-Tab leaves the list for Refresh, outside the inspection"
    );
    let held = refresh(cx);
    drop(held);
    StashBrowser::lands(&stashes, cx);
    assert_eq!(
        cx.update(|window, cx| window.focused(cx)),
        before,
        "focus outside the inspection stays where it was"
    );

    // The saved files, with the stash dropped from a terminal.
    cx.update(|window, cx| window.focus(&files, cx));
    draw(cx);
    assert!(on(cx, &files), "the saved files hold focus");
    // A press on the inspection's text focuses nothing: the column that
    // tracks the inspection's focus never takes it.
    let name = cx
        .debug_bounds("stash-detail-name")
        .expect("the stash's name is drawn");
    cx.simulate_click(name.center(), Modifiers::none());
    draw(cx);
    assert!(
        on(cx, &files),
        "a press on the stash's name leaves focus on the saved files"
    );
    git(repo.path(), &["stash", "drop", "-q", "stash@{0}"]);
    let held = refresh(cx);
    assert!(
        on(cx, &list) && !on(cx, &files),
        "the list takes focus from the saved files while it is read"
    );
    drop(held);
    StashBrowser::lands(&stashes, cx);
    assert_eq!(
        stashes.read_with(cx, |this, _| (this.selected, this.snapshot.is_some())),
        (None, false),
        "the dropped stash leaves nothing selected"
    );
    assert!(on(cx, &list), "and the list keeps focus");
}

/// Refresh's answer lands though a stash is clicked while the list is
/// read: with a stash saved from a terminal since the list was read,
/// Refresh, its read held, then a click on the older stash's row, which
/// the list still draws, then the read let go. The list shows the three
/// stashes, and the stash clicked stays selected by its commit, one row
/// down, its saved changes shown, while a click on a stash the answer
/// no longer lists selects nothing, and one on a stash the answer lists
/// unchanged stays selected, its read landing rather than made again.
/// A stash chosen with Home while the list is read leaves the answer
/// to land as a click does. The click's read replaced the list's, so
/// Refresh's answer was dropped and the list kept the two stashes it
/// showed before.
#[gpui::test]
async fn refresh_lands_past_a_stash_clicked_while_the_list_is_read(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let notes = repo.path().join("notes.txt");
    for message in ["first", "second"] {
        std::fs::write(&notes, message).unwrap();
        git(
            repo.path(),
            &["stash", "push", "--include-untracked", "-m", message],
        );
    }
    let (app, cx) = window(cx, &repo);
    draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    landed(&app, cx, "saved-stash-row-1");
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("the Stashes dialog opens");
    let listed = |cx: &mut VisualTestContext| {
        stashes.read_with(cx, |this, _| {
            let page = this.page.as_ref().expect("the list");
            let names: Vec<_> = page
                .entries
                .iter()
                .map(|stash| stash.name.clone())
                .collect();
            let selected = this.selected.and_then(|index| page.entries.get(index));
            (
                names,
                this.selected,
                selected.map(|stash| stash.oid.clone()),
                this.snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.stash.oid.clone()),
            )
        })
    };
    let (names, _, _, _) = listed(cx);
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names[1].ends_with("first"), "{names:?}");
    let first = stashes.read_with(cx, |this, _| {
        this.page.as_ref().expect("the list").entries[1].oid.clone()
    });
    std::fs::write(&notes, "third").unwrap();
    git(
        repo.path(),
        &["stash", "push", "--include-untracked", "-m", "third"],
    );
    let held = stashes.read_with(cx, |this, _| this.metadata.hold());
    cx.update(|window, cx| stashes.update(cx, |this, cx| this.load_page(0, window, cx)));
    draw(cx);
    let row = cx
        .debug_bounds("saved-stash-row-1")
        .expect("the list still draws the older stash");
    cx.simulate_click(row.center(), Modifiers::none());
    drop(held);
    StashBrowser::lands(&stashes, cx);
    let (names, row, oid, shown) = listed(cx);
    assert_eq!(names.len(), 3, "Refresh's answer lands: {names:?}");
    assert!(names[0].ends_with("third"), "{names:?}");
    assert_eq!(row, Some(2), "the stash clicked moved one row down");
    assert_eq!(oid, Some(first.clone()), "and stays selected by its commit");
    assert_eq!(shown, Some(first.clone()), "its saved changes shown");
    assert!(
        cx.debug_bounds("saved-stash-row-2").is_some(),
        "its row is drawn"
    );

    // A click on a stash a terminal dropped meanwhile selects nothing
    // once the answer lands without it.
    git(repo.path(), &["stash", "drop", "-q", "stash@{0}"]);
    let held = stashes.read_with(cx, |this, _| this.metadata.hold());
    cx.update(|window, cx| stashes.update(cx, |this, cx| this.load_page(0, window, cx)));
    draw(cx);
    let row = cx
        .debug_bounds("saved-stash-row-0")
        .expect("the list still draws the dropped stash");
    cx.simulate_click(row.center(), Modifiers::none());
    drop(held);
    StashBrowser::lands(&stashes, cx);
    let (names, row, oid, shown) = listed(cx);
    assert_eq!(names.len(), 2, "Refresh's answer lands: {names:?}");
    assert_eq!(
        (row, oid, shown),
        (None, None, None),
        "a stash the answer no longer lists stays selected nowhere"
    );

    // A click on a stash the answer lists as the list drew it, by the
    // same `stash@{n}` and commit, stays selected, and its read, which
    // waited behind the list's, lands rather than being made again.
    let held = stashes.read_with(cx, |this, _| this.metadata.hold());
    cx.update(|window, cx| stashes.update(cx, |this, cx| this.load_page(0, window, cx)));
    draw(cx);
    let row = cx
        .debug_bounds("saved-stash-row-1")
        .expect("the list still draws the older stash");
    cx.simulate_click(row.center(), Modifiers::none());
    let clicked = stashes.read_with(cx, |this, _| this.generation);
    drop(held);
    StashBrowser::lands(&stashes, cx);
    let (names, row, oid, shown) = listed(cx);
    assert_eq!(names.len(), 2, "Refresh's answer lands: {names:?}");
    assert_eq!(row, Some(1), "the stash clicked stays selected");
    assert_eq!(oid, Some(first.clone()), "by its commit");
    assert_eq!(shown, Some(first), "its saved changes shown");
    assert_eq!(
        stashes.read_with(cx, |this, _| this.generation),
        clicked,
        "the click's read landed, never made again"
    );

    // A stash chosen from the keyboard while the list is read, with
    // another saved from a terminal since, leaves Refresh's answer to
    // land as a click does: Home on the rows the list still draws
    // chooses the newest, which the answer puts one row down.
    let second = stashes.read_with(cx, |this, _| {
        this.page.as_ref().expect("the list").entries[0].oid.clone()
    });
    std::fs::write(&notes, "fourth").unwrap();
    git(
        repo.path(),
        &["stash", "push", "--include-untracked", "-m", "fourth"],
    );
    let held = stashes.read_with(cx, |this, _| this.metadata.hold());
    cx.update(|window, cx| stashes.update(cx, |this, cx| this.load_page(0, window, cx)));
    draw(cx);
    cx.simulate_keystrokes("home");
    drop(held);
    StashBrowser::lands(&stashes, cx);
    let (names, row, oid, shown) = listed(cx);
    assert_eq!(names.len(), 3, "Refresh's answer lands: {names:?}");
    assert!(names[0].ends_with("fourth"), "{names:?}");
    assert_eq!(row, Some(1), "the stash chosen moved one row down");
    assert_eq!(
        oid,
        Some(second.clone()),
        "and stays selected by its commit"
    );
    assert_eq!(shown, Some(second), "its saved changes shown");
}

/// A stashed image's sides offer the open control Compare's image
/// captions offer, by the same rule, and it is one of the Stashes
/// detail's controls, as a stashed document's is: stacked beneath the
/// list in a 461 × 490 window at 13 and 18 pt, with focus on Before's
/// control and the detail scrolled to its top, away from the
/// comparison, Tab onto After's control shows it whole with its ring in
/// the detail's view in every frame drawn after the key, the first
/// included, its ring unclipped, and the detail holds focus. A click
/// that opens nothing says why on the line beneath After's caption.
/// Stashes once drew a stashed image's sides with no open control.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_stashed_image_side_offers_its_open_control_in_the_detail(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::system_preview::tests::{latest_note, png, queue, seen_as_svg};
    use crate::tags::tests::{content_mask, draw, holds, rendered};
    let fixture = tempfile::tempdir().unwrap();
    let picture = ("picture.png", png(Some(b"first")), png(Some(b"second")));
    let (browser, cx) = stashed(
        cx,
        fixture.path(),
        &[picture],
        (461., 490., 13),
        (0, "image-system-preview-1"),
    );
    let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
    let after = "image-system-preview-1";
    let slack = px(0.01);
    for size in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(size, 12, window, cx));
        draw(cx);
        let room = cx.read(appearance::button_ring_room);
        let whole = |control: Bounds<Pixels>, view: Bounds<Pixels>| {
            let ring = control.dilate(room);
            ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
        };
        cx.update(|window, cx| {
            detail
                .focus(("image-system-preview", 0_usize), cx)
                .focus(window, cx)
        });
        draw(cx);
        cx.update(|window, _| {
            detail.scroll().set_offset(point(px(0.), px(0.)));
            window.refresh();
        });
        draw(cx);
        assert!(
            !whole(rendered(cx, after), rendered(cx, "stash-detail")),
            "{size} pt: at its top the detail does not show After's open control whole"
        );
        detail.take_painted();
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(
            cx.update(|window, _| detail.focused(window)),
            Some(("image-system-preview", 1_usize).into()),
            "{size} pt: Tab moves from Before's open control onto After's"
        );
        assert!(
            cx.update(|window, cx| detail.holds_focus(window, cx)),
            "{size} pt: the detail holds focus"
        );
        // The key's change of input modality draws a frame before focus
        // moves, with Before's control focused in the comparison's left
        // half; the frames after it hold After's.
        let middle = rendered(cx, "stash-comparison").center().x;
        let frames: Vec<_> = detail
            .take_painted()
            .into_iter()
            .skip_while(|frame| frame.control.is_some_and(|control| control.left() < middle))
            .collect();
        assert!(!frames.is_empty(), "{size} pt: Tab paints the detail");
        for (index, frame) in frames.iter().enumerate() {
            let control = frame
                .control
                .unwrap_or_else(|| panic!("{size} pt: frame {index} draws After's control"));
            assert!(
                whole(control, frame.viewport),
                "{size} pt: frame {index} of {} paints After's control {control:?} with its \
                 ring outside the detail {:?}",
                frames.len(),
                frame.viewport
            );
        }
        let (ring, mask) = (rendered(cx, after).dilate(room), content_mask(cx, after));
        assert!(
            holds(mask, ring),
            "{size} pt: After's ring {ring:?} is clipped to {mask:?}"
        );
    }
    // A click that opens nothing says why beneath After's caption.
    queue(Err(seen_as_svg("a PNG image")));
    let center = rendered(cx, after).center();
    cx.simulate_click(center, Modifiers::none());
    cx.run_until_parked();
    draw(cx);
    let (answer, failed) = cx.read(latest_note).expect("a line says why");
    assert!(
        failed && answer.starts_with("Could not open After."),
        "the answer {answer:?}"
    );
    let line = rendered(cx, "image-system-preview-1/note");
    let (control, comparison) = (rendered(cx, after), rendered(cx, "stash-comparison"));
    assert!(
        line.top() >= control.bottom() - slack
            && line.left() >= comparison.center().x - slack
            && line.right() <= comparison.right() + slack,
        "the line {line:?} lies beneath After's control {control:?}, in After's half of \
         {comparison:?}"
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// Where a stashed image's comparison draws each side's open control,
/// its name and its image, from the comparison's top left corner.
#[cfg(target_os = "linux")]
struct StashedImage {
    opens: [Option<Bounds<Pixels>>; 2],
    names: [Bounds<Pixels>; 2],
    images: [Bounds<Pixels>; 2],
}

/// A stash of `picture.png` with `sides` in Stashes, once `present`, an
/// open control's selector, is drawn.
#[cfg(target_os = "linux")]
fn stashed_image<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    fixture: &std::path::Path,
    sides: (Option<Vec<u8>>, Option<Vec<u8>>),
    present: &'static str,
) -> &'a mut VisualTestContext {
    stashed_sides(
        cx,
        fixture,
        &[("picture.png", sides.0, sides.1)],
        (461., 490., 13),
        (0, present),
    )
    .1
}

/// The stashed image's comparison as drawn at `text` pt in a `width` ×
/// `height` window.
#[cfg(target_os = "linux")]
fn stashed_image_at(
    cx: &mut VisualTestContext,
    (text, width, height): (u8, f32, f32),
) -> StashedImage {
    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
    cx.simulate_resize(size(px(width), px(height)));
    crate::tags::tests::draw(cx);
    let corner = cx
        .debug_bounds("stash-comparison")
        .expect("the comparison")
        .origin;
    let mut drawn = |selector: &'static str| {
        cx.debug_bounds(selector).map(|bounds| Bounds {
            origin: point(bounds.left() - corner.x, bounds.top() - corner.y),
            size: bounds.size,
        })
    };
    StashedImage {
        opens: ["image-system-preview-0", "image-system-preview-1"].map(&mut drawn),
        names: ["stash-image-name-0", "stash-image-name-1"]
            .map(|selector| drawn(selector).expect(selector)),
        images: ["stash-image-0", "stash-image-1"].map(|selector| drawn(selector).expect(selector)),
    }
}

/// A stashed image's open controls take one form and one place by
/// Compare's rule (`image_compare::open_place`), every caption counted
/// whether or not its side offers one, so an added or a deleted image's
/// one control stands where a changed image's control of its side
/// does. At 13 and 18 pt in 461 × 490 and 1000 × 680 windows, and at
/// each pair of widths a pixel apart, found between 461 and 1,480 px,
/// where a changed image's controls leave the names' line or become
/// their icons, the present side's control has the changed image's
/// bounds for its side, the absent side offers none, and each side's
/// image starts where the changed image's does, no shorter. Chosen from
/// the offering sides' captions alone, an added image's After control,
/// beside the shorter name, kept the form and the line a changed
/// image's two had left within a few pixels of each change.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_stashed_added_or_deleted_image_s_open_control_stands_where_a_changed_one_s_do(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::system_preview::tests::png;
    let (first, second) = (png(Some(b"first")), png(Some(b"second")));
    let slack = px(0.5);
    let fixture = tempfile::tempdir().unwrap();
    let mut sizes = vec![
        (13, 461., 490.),
        (18, 461., 490.),
        (13, 1000., 680.),
        (18, 1000., 680.),
    ];
    let changed = {
        let cx = stashed_image(
            cx,
            fixture.path(),
            (Some(first.clone()), Some(second.clone())),
            "image-system-preview-1",
        );
        // Where the controls leave the names' line, and where they
        // become their icons, as the window narrows.
        let inline = |drawn: &StashedImage| {
            drawn
                .opens
                .iter()
                .flatten()
                .all(|open| open.top() < drawn.names[0].bottom())
        };
        let labelled = |drawn: &StashedImage| {
            drawn
                .opens
                .iter()
                .flatten()
                .all(|open| open.size.width > open.size.height + slack)
        };
        for text in [13, 18] {
            for form in [&inline as &dyn Fn(&StashedImage) -> bool, &labelled] {
                let mut at = |width: f32| form(&stashed_image_at(cx, (text, width, 680.)));
                let (mut narrow, mut wide) = (461., 1480.);
                let narrowest = at(narrow);
                if narrowest == at(wide) {
                    continue;
                }
                while wide - narrow > 1. {
                    let middle = ((narrow + wide) / 2.).floor();
                    if at(middle) == narrowest {
                        narrow = middle;
                    } else {
                        wide = middle;
                    }
                }
                sizes.extend([(text, narrow, 680.), (text, wide, 680.)]);
            }
        }
        sizes
            .iter()
            .map(|&size| stashed_image_at(cx, size))
            .collect::<Vec<_>>()
    };
    assert!(
        sizes.len() > 4,
        "between 461 and 1,480 px a changed image's controls change form or line"
    );
    for (what, present, sides, selector) in [
        ("added", 1, (None, Some(second)), "image-system-preview-1"),
        ("deleted", 0, (Some(first), None), "image-system-preview-0"),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let cx = stashed_image(cx, fixture.path(), sides, selector);
        for ((text, width, height), changed) in sizes.iter().zip(&changed) {
            let one = stashed_image_at(cx, (*text, *width, *height));
            let at = format!("{what} at {text} pt in {width} × {height}");
            let control = one.opens[present]
                .unwrap_or_else(|| panic!("{at}: the present side offers its control"));
            assert!(
                one.opens[1 - present].is_none(),
                "{at}: the absent side offers none"
            );
            let theirs = changed.opens[present].expect("a changed image offers both");
            assert!(
                (control.left() - theirs.left()).abs() <= slack
                    && (control.top() - theirs.top()).abs() <= slack
                    && (control.size.width - theirs.size.width).abs() <= slack,
                "{at}: the control {control:?} stands where the changed image's {theirs:?} \
                 does, in its form"
            );
            for side in 0..2 {
                let (image, theirs) = (one.images[side], changed.images[side]);
                assert!(
                    (image.top() - theirs.top()).abs() <= slack
                        && image.size.height >= theirs.size.height - slack,
                    "{at}: side {side}'s image {image:?} starts where the changed image's \
                     {theirs:?} does, no shorter"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }
}

/// A stashed document's comparison takes its open controls' form from
/// its box's width in the frame it is drawn ([`crate::in_frame`]): every
/// frame drawn as the window narrows and widens between 461 and 1,481 px
/// at 13 and 18 pt lays the comparison out as the frame it settles in,
/// and the walk meets After's control labelled and as its icon. Built
/// from the box's width a frame before, the first frame after a resize
/// drew the form of the width before.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_stashed_comparison_takes_its_form_in_the_frame_it_is_drawn(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pdf =
        |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
    let document = repo.path().join("document.pdf");
    std::fs::write(&document, pdf("first")).unwrap();
    git(repo.path(), &["add", "document.pdf"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add a document"]);
    std::fs::write(&document, pdf("second")).unwrap();
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(1481.), px(980.)));
    draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    landed(&app, cx, "saved-stash-row-0");
    cx.executor()
        .advance_clock(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-0");
    let file = cx.debug_bounds("stash-file-0").unwrap();
    cx.simulate_click(file.center(), Modifiers::none());
    landed(&app, cx, "system-preview-1");
    let met = crate::system_preview::tests::open_controls_take_their_form_in_the_frame(
        cx,
        "the Stashes dialog",
        "stash-comparison",
    );
    assert_eq!(
        met.into_iter().collect::<Vec<_>>(),
        ["icon", "labelled"],
        "the walk meets both forms"
    );
}

/// The control `open` of a stashed side, its open control or another of
/// its header's, lies in the side's own header, `list` drawn as
/// `region`, and in the Stashes detail around it, `detail`
/// (`system_preview::SideHeader::within`). With the
/// detail scrolled to its top, away from the comparison, focus moves
/// onto the control: through the header's list with the region showing
/// the control, or, where `next` names the control after it, by
/// Shift+Tab from that control with the region scrolled to its end,
/// away from the open control, and the detail away from the region, so
/// both containers scroll. The detail then holds focus, and in every
/// frame painted after focus moves, the first included, the control lies
/// whole with its ring in the detail's view and in the region's.
#[cfg(target_os = "linux")]
fn assert_the_detail_reveals_an_open_control(
    cx: &mut VisualTestContext,
    at: &str,
    (detail, list): (
        &crate::focus_reveal::FocusReveal,
        &crate::focus_reveal::FocusReveal,
    ),
    region: &'static str,
    (id, open): &crate::focus_reveal::tests::Control,
    next: Option<&crate::focus_reveal::tests::Control>,
) {
    use crate::tags::tests::{draw, rendered};
    let room = cx.read(appearance::button_ring_room);
    let slack = px(0.01);
    let within = |ring: Bounds<Pixels>, view: Bounds<Pixels>| {
        ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
    };
    let whole = |cx: &mut VisualTestContext, container: &'static str| {
        within(rendered(cx, open).dilate(room), rendered(cx, container))
    };
    list.reveal_next_focus();
    let first = next.map_or(id, |(next, _)| next);
    cx.update(|window, cx| list.focus(first.clone(), cx).focus(window, cx));
    draw(cx);
    cx.update(|window, cx| {
        if next.is_none() {
            window.blur(cx);
        }
        detail.scroll().set_offset(point(px(0.), px(0.)));
        window.refresh();
    });
    draw(cx);
    if next.is_some() {
        // The region scrolls to its end, away from the control. The
        // detail's scroll above moved the region whole and revealed
        // nothing in it, so the order of the two scrolls does not
        // matter (`focus_reveal`'s `Observation`).
        cx.update(|window, _| {
            let end = list.scroll().max_offset().y;
            list.scroll().set_offset(point(px(0.), -end));
            window.refresh();
        });
        draw(cx);
        // The region's scroll moves the control, which it clips, so the
        // detail is away from the region itself.
        let away = !within(rendered(cx, region), rendered(cx, "stash-detail"));
        assert!(
            !whole(cx, region) && away,
            "{at}: {region} does not show {open} whole, and at its top the detail does not \
             show {region} whole"
        );
    } else {
        assert!(
            !whole(cx, "stash-detail") && whole(cx, region),
            "{at}: at its top, the detail does not show {open} whole, and {region} does"
        );
    }
    detail.take_painted();
    list.take_painted();
    if next.is_some() {
        cx.simulate_keystrokes("shift-tab");
    } else {
        detail.reveal_next_focus();
        list.reveal_next_focus();
        cx.update(|window, cx| list.focus(id.clone(), cx).focus(window, cx));
    }
    draw(cx);
    assert_eq!(
        cx.update(|window, _| list.focused(window)).as_ref(),
        Some(id),
        "{at}: focus is on {open}"
    );
    assert!(
        cx.update(|window, cx| detail.holds_focus(window, cx)),
        "{at}: the detail holds focus on {open}"
    );
    for (container, name) in [(detail, "stash-detail"), (list, region)] {
        let frames = container.take_painted();
        assert!(!frames.is_empty(), "{at}: {name} paints");
        for (index, frame) in frames.iter().enumerate() {
            let ring = frame
                .control
                .unwrap_or_else(|| panic!("{at}: frame {index} of {name} draws {open}"))
                .dilate(room);
            assert!(
                within(ring, frame.viewport),
                "{at}: frame {index} of {} paints {open} with its ring, {ring:?}, \
                 outside {name}, {:?}",
                frames.len(),
                frame.viewport
            );
        }
        assert!(whole(cx, name), "{at}: {name} shows {open} whole");
    }
}

/// Stashes, open on a stash of `files`, each committed with its first
/// bytes and stashed with its second, in a `width` × `height` window at
/// `text` pt, with the stash chosen and its file `chosen`, in path
/// order, once `drawn` is drawn.
#[cfg(target_os = "linux")]
fn stashed<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    fixture: &std::path::Path,
    files: &[(&str, Vec<u8>, Vec<u8>)],
    at: (f32, f32, u8),
    chosen: (usize, &'static str),
) -> (Entity<StashBrowser>, &'a mut VisualTestContext) {
    let files: Vec<_> = files
        .iter()
        .map(|(name, first, second)| (*name, Some(first.clone()), Some(second.clone())))
        .collect();
    stashed_sides(cx, fixture, &files, at, chosen)
}

/// A stashed file's name with its committed and its stashed bytes, each
/// absent where the file is not there ([`stashed_sides`]).
type StashedFile<'a> = (&'a str, Option<Vec<u8>>, Option<Vec<u8>>);

/// [`stashed`] for `files` that may be absent on either side: one
/// without first bytes is added to the index after the commit, so the
/// stash saves it added, and one without second bytes is deleted
/// from the working tree, so the stash saves it deleted.
fn stashed_sides<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    fixture: &std::path::Path,
    files: &[StashedFile<'_>],
    (width, height, text): (f32, f32, u8),
    (chosen, drawn): (usize, &'static str),
) -> (Entity<StashBrowser>, &'a mut VisualTestContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let repo = tagged_repository(fixture);
    for (name, first, _) in files {
        if let Some(first) = first {
            std::fs::write(repo.path().join(name), first).unwrap();
            git(repo.path(), &["add", name]);
        }
    }
    git(
        repo.path(),
        &["commit", "--quiet", "--allow-empty", "-m", "Add them"],
    );
    for (name, first, second) in files {
        let path = repo.path().join(name);
        match second {
            Some(second) => {
                std::fs::write(&path, second).unwrap();
                if first.is_none() {
                    git(repo.path(), &["add", name]);
                }
            }
            None => std::fs::remove_file(&path).unwrap(),
        }
    }
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
    cx.simulate_resize(size(px(width), px(height)));
    draw(cx);
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    landed(&app, cx, "saved-stash-row-0");
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-0");
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| browser.select_file(chosen, window, cx))
    });
    landed(&app, cx, drawn);
    (browser, cx)
}

/// Whether the last frame drew the saved stashes' keyboard ring: its
/// 1 px accent edge, whole on all four sides of the list.
fn stash_list_ringed(cx: &mut VisualTestContext) -> bool {
    let accent = cx.read(|cx| gpui_kit::Hsla::from(rgb(palette(cx).accent)));
    let list = cx.debug_bounds("stash-list").expect("the list is drawn");
    crate::focus_ring::painted_outlines(cx, accent)
        .into_iter()
        .find(|edge| {
            (edge.bounds.left() - list.left()).abs() <= px(1.)
                && (edge.bounds.top() - list.top()).abs() <= px(1.)
                && (edge.bounds.right() - list.right()).abs() <= px(1.)
                && (edge.bounds.bottom() - list.bottom()).abs() <= px(1.)
        })
        .is_some_and(|edge| edge.width == px(1.) && edge.whole)
}

/// Stashes open over `repo` with focus on History's list, its read not
/// yet landed, and that list, Escape's destination.
fn opened_from_history<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    repo: &GitRepository,
) -> (Entity<StashBrowser>, FocusHandle, &'a mut VisualTestContext) {
    use crate::tags::tests::{draw, window};
    let (app, cx) = window(cx, repo);
    cx.update(|_, cx| {
        crate::shortcuts::bind_keys(cx);
        appearance::ThemeChoice::Midnight.apply(None, cx);
    });
    draw(cx);
    let opener = app.read_with(cx, |app, _| app.focus.clone());
    cx.update(|window, cx| window.focus(&opener, cx));
    draw(cx);
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    (browser, opener, cx)
}

/// Closes Stashes with Escape and finds focus back on `opener`.
fn closes_back_to(opener: &FocusHandle, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("escape");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    crate::tags::tests::draw(cx);
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "Escape closes Stashes"
    );
    assert!(
        cx.update(|window, _| opener.is_focused(window)),
        "focus returns to the control that had it"
    );
}

/// Stashes opens with focus on its saved stashes, the first real stop,
/// the list's ring whole in the frame the dialog appears, and Escape
/// gives focus back to the control that had it. It once opened on the
/// dialog's shell, which draws nothing, so the keyboard started nowhere
/// visible.
#[gpui::test]
async fn stashes_opens_on_its_saved_stashes_and_closes_back_to_the_opener(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, tagged_repository};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    save_stash(&repo, "kept for later");
    let (browser, opener, cx) = opened_from_history(cx, &repo);
    let list = browser.read_with(cx, |this, _| this.stash_focus.clone());
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "the list holds focus as the dialog opens, before its first frame"
    );
    draw(cx);
    assert!(
        stash_list_ringed(cx),
        "the first frame draws the list's ring whole"
    );
    StashBrowser::lands(&browser, cx);
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "the saved stashes landing leaves focus on the list"
    );
    assert!(stash_list_ringed(cx), "and draws its ring whole");
    closes_back_to(&opener, cx);
}

/// Actions → Browse stashes…, then Escape: focus lands on the Actions
/// button with its ring drawn, whether the keyboard or the pointer opened
/// the menu. The item's handler runs on the menu, so Stashes recorded the
/// menu's focus as its opener and the menu went as the item finished:
/// Escape focused nothing, and no key reached anything until a click.
#[gpui::test]
async fn stashes_from_the_actions_menu_closes_back_to_actions(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, tagged_repository, window};
    use gpui_kit::Modifiers;
    const BROWSE: &str = "menu-item: Browse stashes… · MenuItem";
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    save_stash(&repo, "kept for later");
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| {
        crate::shortcuts::bind_keys(cx);
        crate::native_accessibility::bind_keys(cx);
        appearance::ThemeChoice::Midnight.apply(None, cx);
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    draw(cx);
    let actions = |cx: &mut VisualTestContext| {
        cx.debug_bounds("working-recovery-menu")
            .expect("the Actions menu")
    };
    let ringed = |cx: &mut VisualTestContext| {
        let button = actions(cx);
        !appearance::painted_button(cx, button).1.is_empty()
    };
    // A whole press: the keyboard's click waits for the key up.
    let press = |cx: &mut VisualTestContext, key: &str| {
        let keystroke =
            gpui::Keystroke::parse_with_os(key, crate::shortcuts::host_os()).expect("a keystroke");
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        draw(cx);
    };

    for by_key in [true, false] {
        let at = if by_key {
            "from the keyboard"
        } else {
            "from the pointer"
        };
        if by_key {
            for _ in 0..64 {
                if ringed(cx) {
                    break;
                }
                cx.update(|window, cx| window.focus_next(cx));
                draw(cx);
            }
            assert!(ringed(cx), "{at}: keyboard focus reaches Actions");
            press(cx, "enter");
            draw(cx);
            // The keyboard's cursor opens on the first action.
            press(cx, "enter");
        } else {
            // A Button refuses focus on mouse down, so focus stays on the
            // list while the pointer opens the menu.
            let list = app.read_with(cx, |app, _| app.focus.clone());
            cx.update(|window, cx| window.focus(&list, cx));
            draw(cx);
            assert!(!ringed(cx), "{at}: Actions holds no focus yet");
            let trigger = actions(cx).center();
            cx.simulate_click(trigger, Modifiers::default());
            draw(cx);
            let item = cx
                .debug_bounds(BROWSE)
                .expect("the menu offers Browse stashes…")
                .center();
            cx.simulate_click(item, Modifiers::default());
        }
        draw(cx);
        let browser = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .unwrap_or_else(|| panic!("{at}: Stashes opened"));
        StashBrowser::lands(&browser, cx);
        drop(browser);
        cx.simulate_keystrokes("escape");
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "{at}: Escape closes Stashes"
        );
        assert!(ringed(cx), "{at}: focus is on Actions, its ring drawn");
    }
}

/// With nothing saved, the read that opened Stashes moves focus from the
/// list, which holds no stash, to Refresh, the first control that can
/// take it, whose name carries the empty state's words, so that landing
/// reads them with no announcement of the automatic read's result.
#[gpui::test]
async fn stashes_with_nothing_saved_moves_focus_to_refresh_and_reads_its_empty_state(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, tagged_repository};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let (browser, opener, cx) = opened_from_history(cx, &repo);
    let (list, refresh) = browser.read_with(cx, |this, _| {
        (this.stash_focus.clone(), this.refresh_focus.clone())
    });
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "the list holds focus until the read has found what is saved"
    );
    StashBrowser::lands(&browser, cx);
    assert!(
        cx.update(|window, _| refresh.is_focused(window)),
        "nothing saved moves focus to Refresh, the first control that can take it"
    );
    draw(cx);
    assert_eq!(
        browser
            .read_with(cx, |this, _| this.refresh_name())
            .as_ref(),
        concat!(
            "Refresh saved stashes. ",
            "No saved stashes · Save one from the Actions menu in Changes"
        ),
        "Refresh's name carries the empty state's words as focus lands on it"
    );
    assert!(
        cx.debug_bounds("stash-list-empty-line").is_some(),
        "the list still shows the empty state's words"
    );
    // The empty list is no stop: Tab from Refresh goes on to the
    // dialog's Close, and round to Refresh, never to the list.
    cx.simulate_keystrokes("tab");
    draw(cx);
    assert!(
        cx.update(|window, _| !list.is_focused(window) && !refresh.is_focused(window)),
        "Tab moves from Refresh past the empty list"
    );
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "Tab stays inside Stashes"
    );
    assert!(
        !stash_list_ringed(cx),
        "no ring is drawn around the empty list"
    );
    cx.simulate_keystrokes("tab");
    draw(cx);
    assert!(
        cx.update(|window, _| refresh.is_focused(window)),
        "the next Tab comes round to Refresh"
    );
    assert!(
        crate::copy_feedback::tests::announced(cx).is_none(),
        "the automatic read that opened Stashes announces nothing"
    );
    closes_back_to(&opener, cx);
}

/// A saved file reads the same text as Compare, so a side whose Git LFS
/// object could not be used says so here too: the read no longer fails
/// for it, and only the pointer's text was left to say anything.
#[gpui::test]
async fn a_failing_lfs_side_says_why_in_stashes(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    let fixture = tempfile::tempdir().unwrap();
    let over_limit = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        "e".repeat(64),
        33 * 1024 * 1024
    );
    written::forget();
    let (_browser, cx) = stashed_sides(
        cx,
        fixture.path(),
        &[(
            "notes.txt",
            Some(over_limit.into_bytes()),
            Some(b"plain after\n".to_vec()),
        )],
        (1000., 680., 13),
        (0, "stash-lfs-failure"),
    );
    assert_eq!(
        written::get("stash-lfs-failure").map(|line| line.node.value().map(str::to_owned)),
        Some(Some(
            "Before: this Git LFS object is over the 32 MiB preview limit, so its pointer is \
             shown."
                .to_owned()
        )),
    );
    assert!(
        cx.debug_bounds("stash-lfs-failure")
            .is_some_and(|line| line.size.height > px(0.)),
        "the line is drawn"
    );
}

/// Stashes opened over `messages` saved in order, the last at the top of
/// the list, with the window's keys bound.
fn stashes_saved<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    fixture: &std::path::Path,
    messages: &[&str],
) -> (
    GitRepository,
    Entity<StashBrowser>,
    &'a mut VisualTestContext,
) {
    use crate::tags::tests::{draw, tagged_repository, window};
    let repo = tagged_repository(fixture);
    for message in messages {
        save_stash(&repo, message);
    }
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
    draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    landed(&app, cx, "saved-stash-row-0");
    cx.executor()
        .advance_clock(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
    let browser = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("the Stashes dialog opens");
    (repo, browser, cx)
}

/// Saves a stash named `message`, from outside the app, as a terminal does.
fn save_stash(repo: &GitRepository, message: &str) {
    std::fs::write(repo.path().join("notes.txt"), message).unwrap();
    crate::tags::tests::git(
        repo.path(),
        &["stash", "push", "--include-untracked", "-m", message],
    );
}

/// Presses the window's Refresh key.
fn press_refresh(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-r"
    } else {
        "ctrl-r"
    });
}

/// Selects `row` of the open Stashes and lets its inspection land.
fn select_row(browser: &Entity<StashBrowser>, row: usize, cx: &mut VisualTestContext) {
    cx.update(|window, cx| browser.update(cx, |this, cx| this.select_stash(row, window, cx)));
    StashBrowser::lands(browser, cx);
}

/// The row selected in the open Stashes and its stash's commit.
fn selected_stash(
    browser: &Entity<StashBrowser>,
    cx: &mut VisualTestContext,
) -> (Option<usize>, Option<String>) {
    browser.read_with(cx, |this, _| {
        let stash = this
            .selected
            .and_then(|index| this.page.as_ref()?.entries.get(index));
        (this.selected, stash.map(|stash| stash.oid.clone()))
    })
}

/// Stashes honours the window's Refresh key: with a stash dropped
/// elsewhere, below the one selected, the list reads again while the
/// selected stash, its inspection and focus inside it stay, through the
/// read and after it, and nothing looks busy meanwhile. The key reached
/// no one under the dialog.
#[gpui::test]
async fn the_refresh_key_keeps_the_stash_its_inspection_and_focus(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, git};
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second"]);
    select_row(&browser, 0, cx);
    let (row, oid) = selected_stash(&browser, cx);
    assert_eq!(row, Some(0));
    let files = browser.read_with(cx, |this, _| this.file_focus.clone());
    cx.update(|window, cx| window.focus(&files, cx));
    draw(cx);

    git(repo.path(), &["stash", "drop", "stash@{1}"]);
    let held = browser.read_with(cx, |this, _| this.metadata.hold());
    press_refresh(cx);
    draw(cx);
    browser.read_with(cx, |this, _| {
        assert!(this.list_task.is_some(), "the key reads the list");
        assert!(this.snapshot.is_some(), "the inspection stays meanwhile");
        assert!(this.reading().is_none(), "nothing looks busy meanwhile");
    });
    assert!(
        cx.update(|window, _| files.is_focused(window)),
        "focus stays in the inspection while the list is read"
    );
    drop(held);
    StashBrowser::lands(&browser, cx);

    browser.read_with(cx, |this, _| {
        assert_eq!(this.page.as_ref().map(|page| page.entries.len()), Some(1));
        assert!(this.snapshot.is_some(), "the inspection stays");
    });
    assert_eq!(selected_stash(&browser, cx), (Some(0), oid));
    assert!(
        cx.update(|window, _| files.is_focused(window)),
        "focus stays where it was"
    );
}

/// A stash saved elsewhere renumbers the one selected: the key keeps it
/// selected by its commit one row down with its inspection and focus
/// inside it, nothing read again, and the inspection takes the entry the
/// list names it by now, which Drop… and Review restore… hand the core.
#[gpui::test]
async fn the_refresh_key_follows_a_renumbered_stash(cx: &mut gpui_kit::TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second"]);
    select_row(&browser, 0, cx);
    let (_, oid) = selected_stash(&browser, cx);
    let files = browser.read_with(cx, |this, _| this.file_focus.clone());
    cx.update(|window, cx| window.focus(&files, cx));
    crate::tags::tests::draw(cx);

    save_stash(&repo, "third");
    press_refresh(cx);
    StashBrowser::lands(&browser, cx);

    assert_eq!(selected_stash(&browser, cx), (Some(1), oid));
    browser.read_with(cx, |this, _| {
        let shown = this.snapshot.as_ref().expect("the inspection stays");
        let listed = &this.page.as_ref().expect("the list").entries[1];
        assert_eq!(shown.stash, *listed, "it takes the list's entry");
        assert_eq!(shown.stash.selector, "stash@{1}");
    });
    assert!(
        cx.update(|window, _| files.is_focused(window)),
        "focus stays in the inspection"
    );
}

/// The key reaches Stashes from where the dialog opens, its shell,
/// outside the browser's own controls, and leaves focus there.
#[gpui::test]
async fn the_refresh_key_reaches_stashes_from_the_dialog_itself(cx: &mut gpui_kit::TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first"]);
    let focused = cx.update(|window, cx| window.focused(cx));

    save_stash(&repo, "second");
    press_refresh(cx);
    StashBrowser::lands(&browser, cx);

    assert_eq!(
        browser.read_with(cx, |this, _| this.page.as_ref().map(|p| p.entries.len())),
        Some(2),
        "the saved stashes are read again"
    );
    assert_eq!(cx.update(|window, cx| window.focused(cx)), focused);
}

/// The key reads the page shown, so a stash selected on a later page
/// stays selected there; the Refresh control's first page would have
/// lost it.
#[gpui::test]
async fn the_refresh_key_reads_the_page_shown(cx: &mut gpui_kit::TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (_repo, browser, cx) = stashes_saved(cx, fixture.path(), &["one", "two", "three"]);
    cx.update(|window, cx| browser.update(cx, |this, cx| this.load_page(1, window, cx)));
    StashBrowser::lands(&browser, cx);
    select_row(&browser, 0, cx);
    let (_, oid) = selected_stash(&browser, cx);

    press_refresh(cx);
    StashBrowser::lands(&browser, cx);

    assert_eq!(browser.read_with(cx, |this, _| this.offset), 1);
    assert_eq!(selected_stash(&browser, cx), (Some(0), oid));
}

/// A page read again past the end of a list that has shrunk elsewhere
/// falls back to the first page rather than an empty list.
#[gpui::test]
async fn the_refresh_key_past_the_end_shows_the_first_page(cx: &mut gpui_kit::TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (_repo, browser, cx) = stashes_saved(cx, fixture.path(), &["one", "two"]);
    browser.update(cx, |this, _| this.offset = 5);

    press_refresh(cx);
    StashBrowser::lands(&browser, cx);

    browser.read_with(cx, |this, _| {
        assert_eq!(this.offset, 0);
        assert_eq!(this.page.as_ref().map(|page| page.entries.len()), Some(2));
    });
}

/// A stash a save elsewhere pushed onto the next page is followed there:
/// the quiet read keeps its inspection, focus inside it and a Review
/// restore… being prepared, on the page that now holds it. Only a stash
/// truly gone clears them: it was treated as gone from row 40 on, the
/// inspection cleared, focus moved to the list and the preparation
/// cancelled.
#[gpui::test]
async fn the_refresh_key_follows_a_stash_across_pages(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git};
    let fixture = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..STASH_PAGE_SIZE).map(|n| format!("stash {n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &names);
    // Row 39, the page's last and the oldest stash.
    select_row(&browser, STASH_PAGE_SIZE - 1, cx);
    let (_, oid) = selected_stash(&browser, cx);
    let files = browser.read_with(cx, |this, _| this.file_focus.clone());
    cx.update(|window, cx| window.focus(&files, cx));
    draw(cx);
    // Review restore…'s preparation, held on the operation executor.
    let app = browser
        .read_with(cx, |this, _| this.owner.clone())
        .upgrade()
        .expect("the window");
    let held = app.update(cx, |app, _| app.operations.hold());
    cx.update(|window, cx| browser.update(cx, |this, cx| this.restore(window, cx)));
    assert!(browser.read_with(cx, |this, _| this.restoring.is_some()));

    save_stash(&repo, "pushes it over");
    press_refresh(cx);
    StashBrowser::lands(&browser, cx);

    browser.read_with(cx, |this, _| {
        assert_eq!(this.offset, STASH_PAGE_SIZE, "the page that holds it now");
        assert!(this.snapshot.is_some(), "the inspection stays");
        assert_eq!(
            this.snapshot
                .as_ref()
                .map(|shown| shown.stash.selector.clone()),
            Some(format!("stash@{{{STASH_PAGE_SIZE}}}")),
            "it takes the name the list gives it now"
        );
        assert!(this.restoring.is_some(), "the preparation is not cancelled");
    });
    assert_eq!(selected_stash(&browser, cx), (Some(0), oid));
    assert!(
        cx.update(|window, _| files.is_focused(window)),
        "focus stays in the inspection"
    );
    draw(cx);
    assert_eq!(
        crate::copy_feedback::tests::announced(cx)
            .map(|(_, text)| text)
            .as_deref(),
        Some("Saved stash 41, stash@{40} selected"),
        "a page past the first holding one stash says that stash in words"
    );
    crate::native_accessibility::written::forget();
    draw(cx);
    assert_eq!(
        crate::native_accessibility::written::get("stash-list-heading")
            .and_then(|heading| heading.node.value().map(str::to_owned))
            .as_deref(),
        Some("Saved work · stash 41"),
        "the heading names that stash's place, not a range of one or a count"
    );

    // Truly gone, it is no longer selected.
    cx.update(|_, cx| browser.update(cx, |this, cx| this.cancel_restore(cx)));
    drop(held);
    git(
        repo.path(),
        &["stash", "drop", &format!("stash@{{{STASH_PAGE_SIZE}}}")],
    );
    press_refresh(cx);
    StashBrowser::lands(&browser, cx);
    browser.read_with(cx, |this, _| {
        assert_eq!(this.selected, None, "gone, nothing is selected");
        assert!(this.snapshot.is_none(), "its inspection goes");
    });
}

/// The selected stash stays in view as a stash saved elsewhere moves its
/// row, where the row was in view; a person who scrolled away from it is
/// not pulled back. The list moved its selection without scrolling, so a
/// row at the list's foot went below it.
#[gpui::test]
async fn the_refresh_key_keeps_the_selected_stash_in_view(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..30).map(|n| format!("stash {n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &names);
    draw(cx);
    let row_height = browser.read_with(cx, |this, _| {
        let state = this.stash_scroll.0.borrow();
        let size = state.last_item_size.expect("the list is laid out");
        size.contents.height / 30.
    });
    let last = browser.read_with(cx, |this, _| {
        (0..30)
            .take_while(|row| this.stash_row_in_view(*row))
            .last()
    });
    let last = last.expect("rows in view");
    assert!(last + 1 < 30, "the list scrolls");
    select_row(&browser, last, cx);
    draw(cx);
    let offset = |cx: &mut VisualTestContext| {
        browser.read_with(cx, |this, _| {
            this.stash_scroll.0.borrow().base_handle.offset().y
        })
    };
    assert_eq!(offset(cx), px(0.), "resting at the top");

    save_stash(&repo, "pushes it down");
    press_refresh(cx);
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(
        browser.read_with(cx, |this, _| this.selected),
        Some(last + 1),
        "the selection follows its stash"
    );
    assert!(
        browser.read_with(cx, |this, _| this.stash_row_in_view(last + 1)),
        "and the row stays in view"
    );
    assert!(offset(cx) < px(0.), "the list scrolled to keep it");

    // Scrolled away from the selected row, a refresh leaves the list alone.
    let away = point(px(0.), row_height * -3.);
    browser.update(cx, |this, _| {
        this.stash_scroll.0.borrow().base_handle.set_offset(away)
    });
    select_row(&browser, 0, cx);
    draw(cx);
    assert!(!browser.read_with(cx, |this, _| this.stash_row_in_view(0)));
    save_stash(&repo, "another");
    press_refresh(cx);
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(browser.read_with(cx, |this, _| this.selected), Some(1));
    assert_eq!(offset(cx), away.y, "not pulled back");
}

/// One press of Refresh speaks one result, politely, when its read has
/// landed, whatever the press changed: the list, the selected stash by
/// the name the list gives it now, or that it is no longer listed. A
/// read of the list no press asked for says nothing.
#[gpui::test]
async fn the_refresh_key_says_what_it_found_once(cx: &mut gpui_kit::TestAppContext) {
    use crate::copy_feedback::tests::announced;
    use crate::tags::tests::{draw, git};
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["one", "two", "three"]);
    select_row(&browser, 1, cx);
    draw(cx);

    press_refresh(cx);
    StashBrowser::lands(&browser, cx);
    draw(cx);
    let said = announced(cx).expect("the result is said");
    assert_eq!(said.1, "3 saved stashes, stash@{1} selected");
    draw(cx);
    assert_eq!(
        announced(cx).map(|said| said.0),
        Some(said.0),
        "once per landing"
    );

    // A read of the list no press asked for says nothing.
    cx.executor()
        .advance_clock(crate::copy_feedback::RESTORE_AFTER);
    draw(cx);
    cx.update(|window, cx| browser.update(cx, |this, cx| this.load_page(0, window, cx)));
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(announced(cx), None, "an automatic read is silent");

    // Gone, it says so.
    select_row(&browser, 1, cx);
    git(repo.path(), &["stash", "drop", "stash@{1}"]);
    press_refresh(cx);
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(
        announced(cx).map(|(_, text)| text).as_deref(),
        Some("Saved stash no longer listed")
    );
}

/// QA-109: with `stash@{1}` selected and a stash saved from a terminal,
/// the Refresh key, and the Refresh control alike, say one result: the
/// refresh's sentence, which names where the selected stash now stands.
/// The row keeps its node and its name as the selection follows it, so
/// the AT-SPI adapter reports no rename of it, which Orca 51.1 spoke 12
/// ms after the sentence ("On main: Stash 2: notes edit · stash@{2}").
#[gpui::test]
async fn a_refresh_that_renumbers_the_selected_stash_says_it_once_by_key(
    cx: &mut gpui_kit::TestAppContext,
) {
    refresh_that_renumbers_the_selected_stash(cx, "key").await;
}

/// As [`a_refresh_that_renumbers_the_selected_stash_says_it_once_by_key`],
/// with the dialog's Refresh control clicked.
#[gpui::test]
async fn a_refresh_that_renumbers_the_selected_stash_says_it_once_by_click(
    cx: &mut gpui_kit::TestAppContext,
) {
    refresh_that_renumbers_the_selected_stash(cx, "click").await;
}

/// A saved stash's message, which its row and the detail's title cut to
/// their width, is whole in a tooltip on each; neither had one.
#[gpui::test]
async fn a_stash_message_is_whole_in_its_tooltips(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, hovered_tooltips};
    let fixture = tempfile::tempdir().unwrap();
    // Past the detail's width in a 1,480 px window, where the title of
    // a message of about 75 characters stands whole.
    let long = "a saved stash whose message runs on well past what its row and title show, \
                and on past the detail's own width, with the files it saved, the base it was \
                saved on and the index beside it, in any window this test opens";
    let (_repo, browser, cx) = stashes_saved(cx, fixture.path(), &[long]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 0, cx);
    draw(cx);
    assert_eq!(
        hovered_tooltips(cx, "saved-stash-message-0"),
        1,
        "the row's message"
    );
    assert_eq!(
        hovered_tooltips(cx, "stash-detail-name"),
        1,
        "the detail's title"
    );
}

/// A stash row's message has a tooltip only where the row cuts it: the
/// same message, cut beside the detail in the 255 px list of a 1,480 ×
/// 980 window, is whole in the list stacked across a 461 × 490 window,
/// where a hover shows nothing, and the hover answers for the line laid
/// out last as the window changes. Every row's message had a tooltip,
/// repeating a message the row showed whole.
#[gpui::test]
async fn a_stash_rows_message_has_a_tooltip_only_where_the_row_cuts_it(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, hovered_tooltips};
    let fixture = tempfile::tempdir().unwrap();
    let message = "parser: keep each token's span";
    let (_repo, browser, cx) = stashes_saved(cx, fixture.path(), &[message]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 0, cx);
    draw(cx);
    assert_eq!(
        hovered_tooltips(cx, "saved-stash-message-0"),
        1,
        "cut beside the detail"
    );
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    assert_eq!(
        hovered_tooltips(cx, "saved-stash-message-0"),
        0,
        "whole across the stacked list"
    );
    cx.simulate_resize(size(px(1480.), px(980.)));
    draw(cx);
    assert_eq!(
        hovered_tooltips(cx, "saved-stash-message-0"),
        1,
        "and cut again beside the detail"
    );
}

/// The detail's title has a tooltip only where its room cuts it, as a
/// row's message does: a short message, whole in the title of a 1,480 ×
/// 980 window, shows nothing on a hover, and a long one, cut there,
/// shows its whole text. The title's tooltip once showed always,
/// repeating a title drawn whole.
#[gpui::test]
async fn the_stash_titles_tooltip_shows_only_where_the_title_is_cut(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, hovered_tooltips};
    let fixture = tempfile::tempdir().unwrap();
    // Past the detail's width in a 1,480 px window, where the title of
    // a message of about 75 characters stands whole.
    let long = "a saved stash whose message runs on well past what its row and title show, \
                and on past the detail's own width, with the files it saved, the base it was \
                saved on and the index beside it, in any window this test opens";
    let (_repo, browser, cx) = stashes_saved(cx, fixture.path(), &["short", long]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    // The newest stash, `long`, is listed first.
    select_row(&browser, 1, cx);
    draw(cx);
    assert_eq!(
        hovered_tooltips(cx, "stash-detail-name"),
        0,
        "a title drawn whole"
    );
    select_row(&browser, 0, cx);
    draw(cx);
    assert_eq!(
        hovered_tooltips(cx, "stash-detail-name"),
        1,
        "a title cut to its room"
    );
}

async fn refresh_that_renumbers_the_selected_stash(cx: &mut gpui_kit::TestAppContext, press: &str) {
    use crate::copy_feedback::tests::announced;
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["one", "two", "three"]);
    select_row(&browser, 1, cx);
    draw(cx);
    let (id, oid) = browser.read_with(cx, |this, _| {
        let stash = &this.page.as_ref().expect("the list").entries[1];
        assert_eq!(stash.selector, "stash@{1}");
        (stash_row_id(stash).to_string(), stash.oid.clone())
    });
    let name = |cx: &mut VisualTestContext| {
        written::forget();
        draw(cx);
        written::get(&id).and_then(|written| written.node.label().map(str::to_owned))
    };
    let before = name(cx).expect("the selected row writes its node");
    let mut heard = Heard::start(cx);

    save_stash(&repo, "four");
    if press == "key" {
        press_refresh(cx);
    } else {
        crate::copy_feedback::tests::click(cx, "refresh-stashes");
    }
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(
        selected_stash(&browser, cx),
        (Some(2), Some(oid)),
        "{press}: the selection follows its stash"
    );
    assert_eq!(
        announced(cx).map(|(_, text)| text).as_deref(),
        Some("4 saved stashes, stash@{2} selected"),
        "{press}: the refresh's sentence names where it stands"
    );
    assert_eq!(
        name(cx).as_deref(),
        Some(before.as_str()),
        "{press}: the renumbered row keeps its name"
    );
    let (line, identity) = browser.read_with(cx, |this, _| {
        let stash = &this.page.as_ref().expect("the list").entries[2];
        (
            format!(
                "saved-stash-line-2: stash@{{2}} · {} · {}",
                short_oid(&stash.oid),
                short_date(stash.timestamp)
            ),
            format!("stash@{{2}} · {} · base ", short_oid(&stash.oid)),
        )
    });
    assert!(
        cx.debug_bounds(line.clone().leak()).is_some(),
        "{press}: its row still draws where it stands: {line}"
    );
    let detail = crate::native_accessibility::written::get("stash-detail-identity")
        .and_then(|written| written.node.value().map(str::to_owned))
        .expect("the detail's identity line");
    assert!(
        detail.starts_with(&identity),
        "{press}: and the detail's line says it: {detail}"
    );
    if let Some(said) = heard.next(cx) {
        assert_eq!(
            said,
            vec!["4 saved stashes, stash@{2} selected".to_owned()],
            "{press}: AT-SPI announces the sentence once"
        );
    }
    if let Some(renamed) = heard.renamed_items() {
        assert_eq!(renamed, Vec::<String>::new(), "{press}: and renames no row");
    }
}

/// The Stashes Refresh control does what the key does: it reads the
/// page shown and keeps the selected stash, its inspection and focus
/// inside it. It read the first page and blanked the detail.
#[gpui::test]
async fn the_refresh_control_does_what_the_key_does(cx: &mut gpui_kit::TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (_repo, browser, cx) = stashes_saved(cx, fixture.path(), &["one", "two", "three"]);
    cx.update(|window, cx| browser.update(cx, |this, cx| this.load_page(1, window, cx)));
    StashBrowser::lands(&browser, cx);
    select_row(&browser, 0, cx);
    let (_, oid) = selected_stash(&browser, cx);
    let files = browser.read_with(cx, |this, _| this.file_focus.clone());
    cx.update(|window, cx| window.focus(&files, cx));
    crate::tags::tests::draw(cx);

    crate::copy_feedback::tests::click(cx, "refresh-stashes");
    StashBrowser::lands(&browser, cx);

    browser.read_with(cx, |this, _| {
        assert_eq!(this.offset, 1, "the page shown is read again");
        assert!(this.snapshot.is_some(), "the inspection stays");
    });
    assert_eq!(selected_stash(&browser, cx), (Some(0), oid));
    assert!(cx.update(|window, _| files.is_focused(window)));
}

/// The Refresh key under an open stash confirmation: `app`'s Stashes has
/// closed and its browser gone, so the key reads no list, as a stash
/// saved elsewhere would otherwise renumber the rows behind the alert.
#[track_caller]
fn refresh_key_stays_out(
    app: &Entity<GitTurtle>,
    repo: &GitRepository,
    cx: &mut VisualTestContext,
) {
    use crate::tags::tests::draw;
    draw(cx);
    assert!(
        cx.debug_bounds("operation-consequences").is_some(),
        "the confirmation is open"
    );
    let browser = app.read_with(cx, |app, _| app.recovery.browser.clone());
    assert!(
        browser.and_then(|browser| browser.upgrade()).is_none(),
        "Stashes closed before the confirmation opened, its browser with it"
    );
    save_stash(repo, "saved elsewhere");
    press_refresh(cx);
    draw(cx);
    assert!(
        !cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_open_stashes(window, cx))),
        "the key finds no Stashes to read"
    );
    assert!(
        cx.debug_bounds("operation-consequences").is_some(),
        "and the confirmation stays"
    );
    assert!(
        app.read_with(cx, |app, _| app.loading.is_none()),
        "and the page beneath reads nothing"
    );
}

/// Ctrl+R does nothing under Drop…'s confirmation, which Stashes closes
/// to open: the list cannot renumber behind its `stash@{n}`.
#[gpui::test]
async fn the_refresh_key_does_nothing_under_the_drop_confirmation(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second"]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 0, cx);
    draw(cx);
    let app = browser
        .read_with(cx, |this, _| this.owner.upgrade())
        .expect("the window");
    crate::copy_feedback::tests::click(cx, "drop-saved-stash");
    // The test's handle goes: in the app the dialog held the only one.
    drop(browser);
    refresh_key_stays_out(&app, &repo, cx);
}

/// QA-109: Drop…'s confirmation, which Stashes closes to open, cancelled
/// by Escape or by its Cancel, opens Stashes again as it was, the same
/// stash selected with its inspection, and focus where the person acted:
/// on Drop… pressed from the keyboard, ringed in the first frame, or on
/// the saved stashes a click on Drop… left it on. A stash saved
/// meanwhile from a terminal is read as Stashes opens again, the stash
/// followed where it now stands and nothing said. Confirmed, the stash
/// is dropped and Stashes stays closed, as before. Escape landed on
/// History, the selection gone.
#[gpui::test]
async fn cancelling_drop_returns_to_stashes_where_the_person_acted(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second", "third"]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 1, cx);
    draw(cx);
    let app = browser
        .read_with(cx, |this, _| this.owner.upgrade())
        .expect("the window");
    let oid = selected_stash(&browser, cx).1.expect("a stash selected");
    let open = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
    };
    let confirming =
        |cx: &mut VisualTestContext| cx.debug_bounds("operation-consequences").is_some();

    // From the keyboard: Drop… focused, Enter, then Escape.
    let drop_focus = cx.update(|_, cx| browser.read(cx).detail.focus("drop-saved-stash", cx));
    cx.update(|window, cx| window.focus(&drop_focus, cx));
    draw(cx);
    cx.simulate_keystrokes("enter");
    assert!(confirming(cx), "Enter on Drop… opens its confirmation");
    assert!(open(cx).is_none(), "Stashes closed for it");
    // The Enter comes up, as a press does.
    cx.simulate_event(gpui_kit::KeyUpEvent {
        keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
    });
    save_stash(&repo, "saved meanwhile");
    cx.simulate_keystrokes("escape");
    // The frame Escape draws.
    assert!(!confirming(cx), "Escape closes the confirmation");
    let again = open(cx).expect("Escape opens Stashes again");
    assert_eq!(again, browser, "on the browser it had");
    assert!(
        cx.update(|window, _| drop_focus.is_focused(window)),
        "focus is back on Drop…"
    );
    assert!(
        app.read_with(cx, |app, _| app.focus_ring.shows(&drop_focus)),
        "ringed in the first frame"
    );
    let button = cx.debug_bounds("drop-saved-stash").expect("Drop… is drawn");
    let room = cx.update(|window, _| window.viewport_size());
    assert!(
        button.left() >= px(0.)
            && button.top() >= px(0.)
            && button.right() <= room.width
            && button.bottom() <= room.height,
        "whole in the window {room:?}: {button:?}"
    );
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(
        selected_stash(&browser, cx),
        (Some(2), Some(oid.clone())),
        "the same stash stays selected where the terminal's save moved it"
    );
    assert!(
        cx.update(|window, _| drop_focus.is_focused(window)),
        "focus stays on Drop… as the read lands"
    );
    assert!(
        browser.read_with(cx, |this, _| this.snapshot.is_some()),
        "with its inspection"
    );
    assert_eq!(
        crate::copy_feedback::tests::announced(cx),
        None,
        "and nothing is said"
    );

    // By the pointer: the click leaves focus on the saved stashes, and
    // the confirmation's Cancel brings it back there.
    let list = browser.read_with(cx, |this, _| this.stash_focus.clone());
    cx.update(|window, cx| window.focus(&list, cx));
    draw(cx);
    crate::copy_feedback::tests::click(cx, "drop-saved-stash");
    assert!(confirming(cx), "a click on Drop… opens its confirmation");
    crate::copy_feedback::tests::click(cx, "dialog-cancel: Cancel");
    assert!(!confirming(cx), "Cancel closes the confirmation");
    assert_eq!(open(cx), Some(browser.clone()), "Stashes is open again");
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "focus is back on the saved stashes"
    );
    assert!(
        app.read_with(cx, |app, _| app.focus_ring.shows(&list)),
        "ringed in the first frame, as Stashes opens"
    );
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(selected_stash(&browser, cx), (Some(2), Some(oid.clone())));
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "focus stays on the saved stashes as the read lands"
    );

    // Confirmed: the stash is dropped and Stashes stays closed.
    crate::copy_feedback::tests::click(cx, "drop-saved-stash");
    assert!(confirming(cx));
    drop(browser);
    cx.simulate_keystrokes("enter");
    recovery_lands(&app, cx);
    assert!(!confirming(cx), "the confirmation closes");
    assert!(open(cx).is_none(), "and Stashes stays closed");
    let listed = repo.stash_list(0, STASH_PAGE_SIZE).expect("the stash list");
    assert!(
        !listed.entries.iter().any(|stash| stash.oid == oid),
        "the stash is dropped"
    );
}

/// Drop…'s confirmation cancelled the other ways: its Cancel pressed by a
/// held Enter after Drop… was pressed from the keyboard, and Escape after
/// a click on Drop…. Stashes opens again on the same stash, with focus on
/// Drop… for the key and on the saved stashes for the click; the held
/// Enter's repeats press nothing Stashes gives focus to, neither Drop…,
/// which would ask again, nor Review restore…. A stash dropped from a
/// terminal meanwhile is read as Stashes opens again, the selected stash
/// followed where it now stands.
#[gpui::test]
async fn cancelling_drop_by_a_held_enter_or_after_a_click_returns_to_stashes(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::draw;
    fn key_down(cx: &mut VisualTestContext, is_held: bool) {
        cx.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
            is_held,
            prefer_character_input: false,
        });
        draw(cx);
    }
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second", "third"]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 1, cx);
    draw(cx);
    let app = browser
        .read_with(cx, |this, _| this.owner.upgrade())
        .expect("the window");
    let oid = selected_stash(&browser, cx).1.expect("a stash selected");
    let open = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
    };
    let confirming =
        |cx: &mut VisualTestContext| cx.debug_bounds("operation-consequences").is_some();

    // From the keyboard: Drop…, then a held Enter on the Cancel.
    let drop_focus = cx.update(|_, cx| browser.read(cx).detail.focus("drop-saved-stash", cx));
    cx.update(|window, cx| window.focus(&drop_focus, cx));
    draw(cx);
    cx.simulate_keystrokes("enter");
    assert!(confirming(cx), "Enter on Drop… opens its confirmation");
    // The Enter that opened it comes up, as a press does.
    cx.simulate_event(gpui_kit::KeyUpEvent {
        keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
    });
    cx.simulate_keystrokes("tab");
    draw(cx);
    let cancel = cx.debug_bounds("dialog-cancel: Cancel").expect("Cancel");
    assert_eq!(
        focused_footer_button(cx, &["dialog-cancel: Cancel", "dialog-ok: Drop stash"]),
        Some("dialog-cancel: Cancel"),
        "Tab reaches Cancel"
    );
    // The stash sits first once its newer one is dropped elsewhere.
    let newest = repo
        .stash_list(0, STASH_PAGE_SIZE)
        .expect("the stash list")
        .entries[0]
        .selector
        .clone();
    crate::tags::tests::git(repo.path(), &["stash", "drop", "--quiet", &newest]);
    key_down(cx, false);
    assert!(
        !confirming(cx),
        "the held Enter's press cancels: {cancel:?}"
    );
    assert_eq!(open(cx), Some(browser.clone()), "Stashes opens again");
    assert!(
        cx.update(|window, _| drop_focus.is_focused(window)),
        "focus is back on Drop…"
    );
    key_down(cx, true);
    key_down(cx, true);
    cx.simulate_event(gpui_kit::KeyUpEvent {
        keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
    });
    draw(cx);
    assert!(
        !confirming(cx),
        "the held Enter's repeats press neither Drop… nor Review restore…"
    );
    assert_eq!(open(cx), Some(browser.clone()), "Stashes stays open");
    assert!(cx.update(|window, _| drop_focus.is_focused(window)));
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(
        selected_stash(&browser, cx),
        (Some(0), Some(oid.clone())),
        "the same stash stays selected where the terminal's drop moved it"
    );

    // By the pointer: a click on Drop…, then Escape.
    let list = browser.read_with(cx, |this, _| this.stash_focus.clone());
    cx.update(|window, cx| window.focus(&list, cx));
    draw(cx);
    crate::copy_feedback::tests::click(cx, "drop-saved-stash");
    assert!(confirming(cx), "a click on Drop… opens its confirmation");
    cx.simulate_keystrokes("escape");
    assert!(!confirming(cx), "Escape closes the confirmation");
    assert_eq!(open(cx), Some(browser.clone()), "Stashes is open again");
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "focus is back on the saved stashes"
    );
    assert!(
        app.read_with(cx, |app, _| app.focus_ring.shows(&list)),
        "ringed in the first frame"
    );
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(selected_stash(&browser, cx), (Some(0), Some(oid)));
}

/// Drop…'s confirmation cancelled after a terminal dropped that very
/// stash: Stashes opens again on its list as now read, nothing selected
/// and focus on the saved stashes, and says once "Saved stash no longer
/// listed", as Refresh says it. The selection was cleared with nothing
/// said. A press on the alert's backdrop cancels nothing: the kit's
/// alerts close only by a choice, Escape among them.
#[gpui::test]
async fn cancelling_drop_of_a_stash_gone_meanwhile_says_so_once(cx: &mut gpui_kit::TestAppContext) {
    cancel_drop_of_a_stash_gone_meanwhile(cx, "click").await;
}

/// As [`cancelling_drop_of_a_stash_gone_meanwhile_says_so_once`], with
/// Drop… pressed from the keyboard: focus starts on Drop…, which the
/// read that finds the stash gone no longer draws, and ends on the
/// saved stashes, the sentence said once.
#[gpui::test]
async fn cancelling_drop_of_a_stash_gone_meanwhile_by_key_says_so_once(
    cx: &mut gpui_kit::TestAppContext,
) {
    cancel_drop_of_a_stash_gone_meanwhile(cx, "key").await;
}

async fn cancel_drop_of_a_stash_gone_meanwhile(cx: &mut gpui_kit::TestAppContext, press: &str) {
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second", "third"]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 1, cx);
    draw(cx);
    let selector = browser.read_with(cx, |this, _| {
        this.page.as_ref().expect("the list").entries[1]
            .selector
            .clone()
    });
    let confirming =
        |cx: &mut VisualTestContext| cx.debug_bounds("operation-consequences").is_some();
    if press == "key" {
        let drop_focus = cx.update(|_, cx| browser.read(cx).detail.focus("drop-saved-stash", cx));
        cx.update(|window, cx| window.focus(&drop_focus, cx));
        draw(cx);
        assert!(
            cx.update(|window, _| drop_focus.is_focused(window)),
            "focus starts on Drop…"
        );
        cx.simulate_keystrokes("enter");
        assert!(confirming(cx), "Enter on Drop… opens its confirmation");
        // The Enter comes up, as a press does.
        cx.simulate_event(gpui_kit::KeyUpEvent {
            keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
        });
    } else {
        crate::copy_feedback::tests::click(cx, "drop-saved-stash");
        assert!(confirming(cx), "a click on Drop… opens its confirmation");
    }
    cx.simulate_click(
        gpui_kit::point(px(4.), px(600.)),
        gpui_kit::Modifiers::default(),
    );
    draw(cx);
    assert!(
        confirming(cx),
        "{press}: a press on the backdrop cancels nothing"
    );
    crate::tags::tests::git(repo.path(), &["stash", "drop", "--quiet", &selector]);
    let mut heard = Heard::start(cx);
    cx.simulate_keystrokes("escape");
    assert!(!confirming(cx), "{press}: Escape closes the confirmation");
    StashBrowser::lands(&browser, cx);
    let mut said = Vec::new();
    for _ in 0..3 {
        said.extend(heard.next(cx).unwrap_or_default());
    }
    assert_eq!(
        selected_stash(&browser, cx),
        (None, None),
        "{press}: the stash gone, nothing is selected"
    );
    let list = browser.read_with(cx, |this, _| this.stash_focus.clone());
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "{press}: focus ends on the saved stashes"
    );
    if cfg!(target_os = "linux") {
        assert_eq!(
            said.iter()
                .filter(|line| line.contains("Saved stash no longer listed"))
                .count(),
            1,
            "{press}: said once: {said:?}"
        );
    } else {
        StashBrowser::lands(&browser, cx);
        draw(cx);
        assert_eq!(
            crate::copy_feedback::tests::announced(cx).map(|(_, said)| said),
            Some("Saved stash no longer listed".to_owned()),
            "{press}"
        );
    }
}

/// Stashes opened again as Drop…'s confirmation is cancelled is drawn at
/// rest from its first frame, its title where a first open's entrance
/// ends, while a first open's first frame draws the title above that,
/// so the check tells the two apart. It slid in again as a new dialog;
/// read after frames that parking draws, the title stood at rest either
/// way.
#[gpui::test]
async fn a_cancelled_drop_reopens_stashes_at_rest(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (_repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second"]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    let app = browser
        .read_with(cx, |this, _| this.owner.upgrade())
        .expect("the window");
    let title = |cx: &mut VisualTestContext| {
        cx.debug_bounds("stashes-title")
            .expect("the title is drawn")
            .top()
    };
    let first_frame = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    };
    // Where the title stands once the entrance has ended.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    draw(cx);
    let rest = title(cx);

    // A first open, with motion: its first frame draws the title higher.
    cx.update(|window, cx| window.close_dialog(cx));
    draw(cx);
    drop(browser);
    cx.update(|_, cx| cx.set_reduce_motion(false));
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    first_frame(cx);
    assert!(
        title(cx) < rest,
        "a first open's first frame draws the title above its rest"
    );
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let browser = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes opens");
    StashBrowser::lands(&browser, cx);
    select_row(&browser, 1, cx);
    draw(cx);
    cx.update(|_, cx| cx.set_reduce_motion(false));
    crate::copy_feedback::tests::click(cx, "drop-saved-stash");
    assert!(
        cx.debug_bounds("operation-consequences").is_some(),
        "Drop… opens its confirmation"
    );

    // Escape, and the reopen's first frame, with nothing run between.
    cx.update(|window, cx| {
        window.dispatch_keystroke(gpui::Keystroke::parse("escape").unwrap(), cx);
    });
    first_frame(cx);
    assert!(
        cx.debug_bounds("operation-consequences").is_none(),
        "Escape closes the confirmation"
    );
    assert_eq!(title(cx), rest, "the reopen's first frame draws it at rest");
    cx.update(|_, cx| cx.set_reduce_motion(true));
    StashBrowser::lands(&browser, cx);
}

/// Drop… pressed again before the read a cancelled Drop… owes has been
/// made: the owed read goes with the close, as what it would say does,
/// so nothing reads the list under the second confirmation. It
/// outlived the second Drop… and ran under its alert as the read before
/// it landed.
#[gpui::test]
async fn a_second_drop_forgets_the_read_the_first_cancel_owed(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (_repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second"]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 1, cx);
    draw(cx);
    let confirming =
        |cx: &mut VisualTestContext| cx.debug_bounds("operation-consequences").is_some();
    crate::copy_feedback::tests::click(cx, "drop-saved-stash");
    assert!(confirming(cx), "Drop… opens its confirmation");
    // A read of the list that has not answered as Escape is pressed.
    let held = browser.read_with(cx, |this, _| this.metadata.hold());
    cx.update(|window, cx| browser.update(cx, |this, cx| this.refresh_saved_stashes(window, cx)));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!confirming(cx), "Escape closes the confirmation");
    assert!(
        browser.read_with(cx, |this, _| this.read_owed),
        "the reopened Stashes owes a read after the one still running"
    );
    crate::copy_feedback::tests::click(cx, "drop-saved-stash");
    assert!(confirming(cx), "Drop… opens its confirmation again");
    let reads = browser.read_with(cx, |this, _| this.list_generation);

    drop(held);
    for _ in 0..2 {
        browser.read_with(cx, |this, _| this.metadata.drain());
        draw(cx);
    }
    browser.read_with(cx, |this, _| {
        assert!(this.list_task.is_none(), "the running read has landed");
        assert_eq!(
            this.list_generation, reads,
            "and no read of the list starts under the confirmation"
        );
        assert!(!this.read_owed, "nothing is owed");
    });
    assert!(confirming(cx), "the confirmation stays");
}

/// Drop…'s confirmation cancelled while the list's read still runs, as
/// a Refresh pressed just before Drop… leaves it: Stashes lets that read
/// land, then reads again, so a stash saved from a terminal under the
/// confirmation is listed and the selected stash followed to where it
/// now stands. The reopen's read was skipped while another ran, and the
/// list showed the earlier read's answer.
#[gpui::test]
async fn cancelling_drop_during_a_read_reads_again_after_it(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second"]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 1, cx);
    draw(cx);
    let oid = selected_stash(&browser, cx).1.expect("a stash selected");
    crate::copy_feedback::tests::click(cx, "drop-saved-stash");
    assert!(
        cx.debug_bounds("operation-consequences").is_some(),
        "Drop… opens its confirmation"
    );
    // A read of the list answered on the browser's thread before the
    // terminal's save, its reply not yet handled as Escape is pressed.
    cx.update(|window, cx| browser.update(cx, |this, cx| this.refresh_saved_stashes(window, cx)));
    browser.read_with(cx, |this, _| this.metadata.drain());
    save_stash(&repo, "third");
    cx.simulate_keystrokes("escape");
    draw(cx);
    StashBrowser::lands(&browser, cx);
    StashBrowser::lands(&browser, cx);
    let listed = browser.read_with(cx, |this, _| {
        this.page.as_ref().expect("the list").entries.len()
    });
    assert_eq!(listed, 3, "the stash saved meanwhile is listed");
    assert_eq!(
        selected_stash(&browser, cx),
        (Some(2), Some(oid)),
        "the same stash stays selected where the save moved it"
    );
}

/// Ctrl+R does nothing under Review restore…'s confirmation, which opens
/// in place of Stashes once the core has prepared it.
#[gpui::test]
async fn the_refresh_key_does_nothing_under_the_restore_review(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first", "second"]);
    cx.simulate_resize(size(px(1480.), px(980.)));
    select_row(&browser, 0, cx);
    draw(cx);
    let app = browser
        .read_with(cx, |this, _| this.owner.upgrade())
        .expect("the window");
    crate::copy_feedback::tests::click(cx, "restore-saved-stash");
    recovery_lands(&app, cx);
    drop(browser);
    refresh_key_stays_out(&app, &repo, cx);
}

/// Ctrl+R stays with the dialog that holds focus. A rich preview's dialog
/// opens over a live Stashes, whose browser stays: with a PDF's Go to page
/// open the key reads no list behind it and says nothing over it, and
/// back in Stashes it works again.
#[gpui::test]
async fn the_refresh_key_stays_with_a_dialog_over_stashes(cx: &mut gpui_kit::TestAppContext) {
    use crate::copy_feedback::tests::announced;
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let (repo, browser, cx) = stashes_saved(cx, fixture.path(), &["first"]);
    cx.update(crate::pdf_view::tests::go_to_page);
    draw(cx);
    save_stash(&repo, "second");

    press_refresh(cx);
    draw(cx);
    browser.read_with(cx, |this, _| {
        assert!(this.list_task.is_none(), "the list behind it is not read");
        assert!(this.spoken_refresh.is_none(), "and no result is waiting");
    });
    StashBrowser::lands(&browser, cx);
    draw(cx);
    assert_eq!(
        browser.read_with(cx, |this, _| this.page.as_ref().map(|p| p.entries.len())),
        Some(1),
        "the rows behind it stay"
    );
    assert_eq!(announced(cx), None, "nothing is said over it");

    cx.update(|window, cx| window.close_dialog(cx));
    draw(cx);
    press_refresh(cx);
    StashBrowser::lands(&browser, cx);
    assert_eq!(
        browser.read_with(cx, |this, _| this.page.as_ref().map(|p| p.entries.len())),
        Some(2),
        "back in Stashes the key reads it"
    );
}

/// Lets every read the open Stashes has started land and draws it, then
/// fails at the caller's line unless `selector` is drawn: the saved
/// stashes and a chosen stash's inspection on its metadata executor,
/// and a chosen file's comparison on its preview worker, threads that
/// `run_until_parked` cannot wait for. Each wait holds the test clock
/// until its thread has answered every read asked of it, and the
/// replies land as the test draws. These tests polled instead, drawing,
/// advancing the clock and sleeping 5 ms until `selector` appeared, for
/// up to 22 s of wall-clock time, and a selector already drawn, as a
/// comparison's frame is before its file's read lands, ended the wait
/// with the read still running.
#[track_caller]
fn landed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, selector: &'static str) {
    let browser = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    let reading = |cx: &mut VisualTestContext| {
        browser.read_with(cx, |browser, _| {
            browser.reading().is_some() || browser.preview_task.is_some()
        })
    };
    crate::tags::tests::draw(cx);
    // A read that lands starts no other, so this ends within a round
    // for each kind of read.
    for _ in 0..3 {
        if !reading(cx) {
            break;
        }
        browser.read_with(cx, |browser, _| {
            browser.metadata.drain();
            browser.preview_worker.wait_idle();
        });
        crate::tags::tests::draw(cx);
    }
    assert!(!reading(cx), "Stashes' reads landed");
    assert!(
        cx.debug_bounds(selector).is_some(),
        "{selector} is drawn once Stashes' reads have landed"
    );
}

/// A stashed 3D model's header controls, the camera's, the views' and
/// the link, are the Stashes detail's controls too, as its open control
/// is: stacked beneath the list at 18 pt in a 461 × 490 window, with
/// the detail scrolled to its top, away from the comparison, focus moved
/// onto each shows it whole with its ring in the detail and in the
/// header's region from the first frame, the detail holding focus
/// ([`assert_the_detail_reveals_an_open_control`]). Only the open
/// control was the detail's, so focus on the others left the detail
/// where it was, the control below its fold.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_stashed_models_controls_are_the_details(cx: &mut gpui_kit::TestAppContext) {
    use crate::focus_reveal::tests::control;
    let fixture = tempfile::tempdir().unwrap();
    let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
    let shape = (
        "shape.obj",
        model("2").into_bytes(),
        model("1").into_bytes(),
    );
    let (browser, cx) = stashed(
        cx,
        fixture.path(),
        &[shape],
        (461., 490., 18),
        (0, "model-side-1/header"),
    );
    let view = browser.entity_id();
    let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
    let list = cx
        .update(|_, cx| crate::system_preview::side_header(view, "model-side-1", cx))
        .expect("After's header");
    for camera in [
        control("model-camera", 5),
        control("model-camera", 9),
        control("model-views", 1),
        control("model-link", 1),
    ] {
        assert_the_detail_reveals_an_open_control(
            cx,
            &format!("{} at 18 pt in 461 × 490", camera.1),
            (&detail, &list),
            "model-side-1/header",
            &camera,
            None,
        );
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// Focus on a stashed 3D model's canvas, which the comparison draws with
/// a focus handle of its own that the detail's reveal does not know,
/// moves to the stash list as Refresh clears the inspection, as it does
/// from any of the inspection's controls
/// ([`StashBrowser::release_inspection_focus`]). The release once asked
/// only the controls the detail and the footer listed, so focus stayed
/// on a canvas no frame drew again.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_cleared_inspection_takes_focus_off_a_stashed_models_canvas(
    cx: &mut gpui_kit::TestAppContext,
) {
    let fixture = tempfile::tempdir().unwrap();
    let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
    let shape = (
        "shape.obj",
        model("2").into_bytes(),
        model("1").into_bytes(),
    );
    let (browser, cx) = stashed(
        cx,
        fixture.path(),
        &[shape],
        (1480., 980., 13),
        (0, "model-canvas-1"),
    );
    let canvas = cx
        .debug_bounds("model-canvas-1")
        .expect("the model's canvas is drawn");
    cx.simulate_click(canvas.center(), Modifiers::none());
    crate::views::tests::draw(cx);
    let (list, files, detail) = browser.read_with(cx, |this, _| {
        (
            this.stash_focus.clone(),
            this.file_focus.clone(),
            this.detail.clone(),
        )
    });
    let (focused, listed) =
        cx.update(|window, cx| (window.focused(cx), detail.holds_focus(window, cx)));
    assert!(
        focused
            .as_ref()
            .is_some_and(|focus| *focus != list && *focus != files)
            && !listed,
        "the click focuses the canvas, a control the detail does not list"
    );
    let held = browser.read_with(cx, |this, _| this.metadata.hold());
    cx.update(|window, cx| browser.update(cx, |this, cx| this.load_page(0, window, cx)));
    crate::views::tests::draw(cx);
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "the list takes focus from the canvas while it is read"
    );
    drop(held);
    StashBrowser::lands(&browser, cx);
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "and keeps it once the list lands"
    );
}

/// A stashed 3D model's canvas, focused from the keyboard, keeps its
/// accent ring while the pointer moves over it and away, at 13 pt in a
/// 1,480 × 980 window, as Compare's does: the detail's opener carries the
/// window's focus ring (`Opener::ringed_by`), which the canvas asks.
/// Without it, the canvas drew no ring at all in Stashes.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_stashed_models_canvas_keeps_its_ring_while_the_pointer_moves(
    cx: &mut gpui_kit::TestAppContext,
) {
    let fixture = tempfile::tempdir().unwrap();
    let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
    let shape = (
        "shape.obj",
        model("2").into_bytes(),
        model("1").into_bytes(),
    );
    let (browser, cx) = stashed(
        cx,
        fixture.path(),
        &[shape],
        (1480., 980., 13),
        (0, "model-canvas-1"),
    );
    let app = browser
        .read_with(cx, |browser, _| browser.owner.upgrade())
        .expect("the window's app");
    crate::focus_ring::sweep::prepare(&app, cx);
    crate::model_view::tests::assert_the_canvas_keeps_its_ring(cx, "model-canvas-1", "Stashes");
}

/// A stashed PDF's page arrows and page number are the Stashes detail's
/// controls too, as its open control is: stacked beneath the list at
/// 18 pt in a 461 × 490 window, with the detail scrolled to its top,
/// away from the comparison, focus moved onto each of either side's
/// shows it whole with its ring in the detail and in the header's region
/// from the first frame, the detail holding focus
/// ([`assert_the_detail_reveals_an_open_control`]). Only the open
/// control was the detail's, so focus on the others left the detail
/// where it was, the control below its fold.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_stashed_pdfs_page_controls_are_the_details(cx: &mut gpui_kit::TestAppContext) {
    use crate::focus_reveal::tests::control;
    use crate::tags::tests::draw;
    let fixture = tempfile::tempdir().unwrap();
    let pdf = |text: &str| {
        format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n").into_bytes()
    };
    let document = ("document.pdf", pdf("first"), pdf("second"));
    let (browser, cx) = stashed(
        cx,
        fixture.path(),
        &[document],
        (461., 490., 18),
        (0, "system-preview-1"),
    );
    // The second of three pages, so both arrows step.
    browser.update(cx, |browser, cx| {
        let Some(Content::Rich(preview)) = browser.content.as_deref() else {
            panic!("a changed PDF is a document comparison");
        };
        browser.content = Some(Arc::new(crate::pdf_view::tests::paged_at(preview, 3, 1)));
        cx.notify();
    });
    draw(cx);
    crate::pdf_view::tests::drain_pages();
    draw(cx);
    let view = browser.entity_id();
    let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
    for (side, name) in [(0, "pdf-side-0"), (1, "pdf-side-1")] {
        let list = cx
            .update(|_, cx| crate::system_preview::side_header(view, name, cx))
            .expect("the side's header");
        let region: &'static str = format!("{name}/header").leak();
        for page in ["pdf-back", "pdf-forward", "pdf-page-number"] {
            let page = control(page, side);
            assert_the_detail_reveals_an_open_control(
                cx,
                &format!("{} at 18 pt in 461 × 490", page.1),
                (&detail, &list),
                region,
                &page,
                None,
            );
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A side header's region keeps where the person scrolled it when the
/// Stashes detail around it scrolls after a key input: stacked beneath
/// the list at 18 pt in a 461 × 490 window, with Tab on one of a
/// stashed model's header controls and the region scrolled away from
/// it, scrolling the detail, which moves the region but changes neither
/// focus nor the region's size, leaves the region where it was: whether
/// the region shows the control cut or whole with its ring's room cut,
/// which a frame counts as shown and a resize would reveal. Judged by
/// where the region lay in the window, which the detail's scroll moves,
/// the region scrolled back to its focused control, since GPUI keeps
/// the keyboard as the last input through a trackpad's scroll.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_side_header_keeps_its_scroll_as_the_detail_scrolls(cx: &mut gpui_kit::TestAppContext) {
    use crate::focus_reveal::tests::control;
    use crate::tags::tests::{draw, rendered};
    let fixture = tempfile::tempdir().unwrap();
    let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
    let shape = (
        "shape.obj",
        model("2").into_bytes(),
        model("1").into_bytes(),
    );
    let (browser, cx) = stashed(
        cx,
        fixture.path(),
        &[shape],
        (461., 490., 18),
        (0, "model-side-1/header"),
    );
    let view = browser.entity_id();
    let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
    let list = cx
        .update(|_, cx| crate::system_preview::side_header(view, "model-side-1", cx))
        .expect("After's header");
    let region = "model-side-1/header";
    let room = cx.read(appearance::button_ring_room);
    let (before, (id, camera)) = (control("model-camera", 5), control("model-camera", 6));
    // Tab onto the camera's second control, from its first.
    list.reveal_next_focus();
    cx.update(|window, cx| list.focus(before.0.clone(), cx).focus(window, cx));
    draw(cx);
    cx.simulate_keystrokes("tab");
    draw(cx);
    assert_eq!(
        cx.update(|window, _| list.focused(window)),
        Some(id),
        "Tab moves onto {camera}"
    );
    let slack = px(0.01);
    let whole = |cx: &mut VisualTestContext| {
        let (ring, view) = (rendered(cx, camera).dilate(room), rendered(cx, region));
        ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
    };
    assert!(whole(cx), "Tab reveals {camera} in {region}");
    // The person scrolls the region away from the focused control.
    let end = list.scroll().max_offset().y;
    assert!(end > px(0.), "{region} scrolls at 18 pt in 461 × 490");
    for offset in [px(0.), -end] {
        cx.update(|window, _| {
            list.scroll().set_offset(point(px(0.), offset));
            window.refresh();
        });
        draw(cx);
        if !whole(cx) {
            break;
        }
    }
    assert!(!whole(cx), "{region} is scrolled away from {camera}");
    let away = list.scroll().offset();
    // The detail scrolls without a key or the pointer moving.
    let scroll_detail = |cx: &mut VisualTestContext| {
        let at = rendered(cx, region).origin;
        let shown = detail.scroll().offset().y;
        let reach = detail.scroll().max_offset().y;
        let step = if shown < -reach / 2. {
            px(20.)
        } else {
            px(-20.)
        };
        cx.update(|window, _| {
            detail.scroll().set_offset(point(px(0.), shown + step));
            window.refresh();
        });
        draw(cx);
        assert_ne!(
            rendered(cx, region).origin,
            at,
            "the detail's scroll moves {region}"
        );
        assert!(
            cx.update(|window, _| window.last_input_was_keyboard()),
            "the last input is still Tab"
        );
    };
    scroll_detail(cx);
    assert_eq!(
        list.scroll().offset(),
        away,
        "{region} stays where it was scrolled"
    );

    // Scrolled by less than its ring's room, the control still lies
    // whole in the region, so the frame counts it as shown, and only
    // the region's unchanged size keeps the detail's scroll from
    // revealing it: judged by where the region lay, that scroll moved
    // the region back by the rest of the ring's room.
    let (control, view) = (rendered(cx, camera), rendered(cx, region));
    let offset = list.scroll().offset().y;
    let near = [
        offset + (view.top() + room / 2. - control.top()),
        offset + (view.bottom() - room / 2. - control.bottom()),
    ]
    .into_iter()
    .find(|near| *near <= px(0.) && *near >= -end)
    .unwrap_or_else(|| panic!("{region} scrolls {camera} within its ring's room of an edge"));
    cx.update(|window, _| {
        list.scroll().set_offset(point(px(0.), near));
        window.refresh();
    });
    draw(cx);
    let (control, view) = (rendered(cx, camera), rendered(cx, region));
    assert!(
        control.top() + slack >= view.top()
            && control.bottom() <= view.bottom() + slack
            && !whole(cx),
        "{camera} {control:?} lies whole in {region} {view:?}, its ring's room cut"
    );
    let near = list.scroll().offset();
    scroll_detail(cx);
    assert_eq!(
        list.scroll().offset(),
        near,
        "{region} stays where it was scrolled, {camera} whole in it"
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A window shrunk until the Stashes detail cuts the control the
/// keyboard left focus on shows it whole again in the frames after the
/// resize: stacked beneath the list at 18 pt, with Tab on Review
/// restore… at the detail's foot in a 461 × 640 window and the pointer
/// since moved, as it is to drag the window's edge, the window shrunk
/// to 461 × 490 shows it whole with its ring. A control the person had
/// scrolled out of view is left there: only a cut the resize makes is
/// revealed. Judged only on a key being the last input, a resize after
/// the pointer moved left the focused control cut.
#[gpui::test]
async fn a_resize_that_cuts_the_focused_control_reveals_it(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "first\n").unwrap();
    git(repo.path(), &["add", "notes.txt"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
    std::fs::write(repo.path().join("notes.txt"), "second\n").unwrap();
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(461.), px(640.)));
    draw(cx);
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    landed(&app, cx, "saved-stash-row-0");
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-0");
    let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
    let room = cx.read(appearance::button_ring_room);
    let slack = px(0.01);
    let whole = |cx: &mut VisualTestContext| {
        let ring = rendered(cx, "restore-saved-stash").dilate(room);
        let view = rendered(cx, "stash-detail");
        ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
    };
    let pointer_moves = |cx: &mut VisualTestContext| {
        let list = rendered(cx, "stash-list").center();
        cx.simulate_mouse_move(list, None, Modifiers::default());
        draw(cx);
    };
    tab_to(&browser, "restore-saved-stash", cx);
    draw(cx);
    assert!(whole(cx), "Tab reveals Review restore…");

    // Scrolled out of view by the person, it stays out of view.
    pointer_moves(cx);
    cx.update(|window, _| {
        detail.scroll().set_offset(point(px(0.), px(0.)));
        window.refresh();
    });
    draw(cx);
    assert!(
        !whole(cx),
        "at its top the detail does not show Review restore… whole"
    );
    cx.simulate_resize(size(px(461.), px(600.)));
    draw(cx);
    assert_eq!(
        detail.scroll().offset().y,
        px(0.),
        "a resize leaves a control the person scrolled away from where it is"
    );

    // Shown whole, it is shown whole again once a resize cuts it.
    cx.simulate_keystrokes("shift-tab");
    draw(cx);
    cx.simulate_keystrokes("tab");
    draw(cx);
    assert_eq!(
        cx.update(|window, _| detail.focused(window)),
        Some("restore-saved-stash".into()),
        "Tab returns to Review restore…"
    );
    assert!(whole(cx), "Tab reveals Review restore… again");
    pointer_moves(cx);
    let foot = rendered(cx, "restore-saved-stash").bottom();
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    assert!(
        rendered(cx, "stash-detail").bottom() < foot,
        "the smaller window's detail ends above where Review restore… lay"
    );
    assert_eq!(
        cx.update(|window, _| detail.focused(window)),
        Some("restore-saved-stash".into()),
        "focus stays on Review restore…"
    );
    assert!(
        whole(cx),
        "the resize reveals Review restore… whole with its ring"
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A window whose edge is dragged keeps the focused control whole
/// through every step of the drag, not only its first: stacked beneath
/// the list at 18 pt, with Tab on Review restore… at the foot of the
/// Stashes detail and the pointer since moved, the window shrinks from
/// 461 × 640 to 461 × 490 in 15 px steps, each step's next resize laid
/// out before the frame that shows that step's reveal is drawn. Judged
/// at the offset its frame was painted at, before the reveal it
/// requested had applied, the step counted Review restore… as cut, so
/// the next resize revealed nothing and left it cut for the rest of the
/// drag.
///
/// GPUI's test build draws a dirty window as soon as its effects are
/// flushed, so a resize's reveal is always drawn before the next resize
/// arrives. Each step is sequenced by hand instead: one update resizes
/// the window and draws that frame, which requests its reveal, and
/// queues the next resize behind the reveal, so the next frame is laid
/// out at the smaller size with the reveal applied but never drawn at
/// the size it was made for. The test shows that the reveal holds
/// across such frames; it cannot show how a platform paces a real
/// drag's resizes against its frames, which a native drag settles.
#[gpui::test]
async fn a_dragged_window_keeps_the_focused_control_whole_at_every_step(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::focus_reveal::tests::Painted;
    use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "first\n").unwrap();
    git(repo.path(), &["add", "notes.txt"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
    std::fs::write(repo.path().join("notes.txt"), "second\n").unwrap();
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(461.), px(640.)));
    draw(cx);
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    landed(&app, cx, "saved-stash-row-0");
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-0");
    let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
    let room = cx.read(appearance::button_ring_room);
    let slack = px(0.01);
    let whole = move |control: Bounds<Pixels>, view: Bounds<Pixels>| {
        let ring = control.dilate(room);
        ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
    };
    let cut = |frame: &Painted| {
        !frame
            .control
            .is_some_and(|control| whole(control, frame.viewport))
    };
    tab_to(&browser, "restore-saved-stash", cx);
    draw(cx);
    assert!(
        whole(
            rendered(cx, "restore-saved-stash"),
            rendered(cx, "stash-detail")
        ),
        "Tab reveals Review restore…"
    );
    // The pointer moves, as it does to drag the window's edge.
    let list = rendered(cx, "stash-list").center();
    cx.simulate_mouse_move(list, None, Modifiers::default());
    draw(cx);
    let mut height: f32 = 640.;
    let mut held = 0;
    while height > 490. {
        let (step, next) = (height - 15., (height - 30.).max(490.));
        detail.take_painted();
        cx.update(|window, cx| {
            window.resize(size(px(461.), px(step)));
            window.bounds_changed(cx);
            window.draw(cx).clear(cx);
            window.defer(cx, move |window, cx| {
                window.resize(size(px(461.), px(next)));
                window.bounds_changed(cx);
            });
        });
        draw(cx);
        let frames = detail.take_painted();
        // The step's frame cut Review restore…, and the next resize cut
        // it again before the step's reveal was drawn.
        if let [stepped, resized, ..] = frames.as_slice()
            && cut(stepped)
            && cut(resized)
            && resized.viewport.size.height < stepped.viewport.size.height
        {
            held += 1;
        }
        assert_eq!(
            cx.update(|window, _| detail.focused(window)),
            Some("restore-saved-stash".into()),
            "at 461 × {next} focus stays on Review restore…"
        );
        assert!(
            whole(
                rendered(cx, "restore-saved-stash"),
                rendered(cx, "stash-detail")
            ),
            "dragged to 461 × {next}, the window shows Review restore… whole with its \
             ring: {frames:?}"
        );
        height = next;
    }
    assert!(
        held > 0,
        "a step's next resize cut Review restore… before the step's reveal was drawn"
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The Stashes detail's fades never lie over the ring of the control
/// focus is on, as every cue whose controls a reveal scrolls into view
/// (`tags::ScrollCue::revealing`): stacked beneath the list at 13 and
/// 18 pt, in a 461 px wide window at the height where the detail is too
/// short for a thumb that moves (461 × 490 at 18 pt), so it fades the
/// edges its content continues past, Tab through the detail's controls
/// reveals each, some at an edge where a whole fade would lie, and no
/// frame paints a fade band over the focused control's ring. The
/// detail's cue once took no notice of focus, and its fade lay over the
/// ring of a control the keyboard had just revealed at its edge.
#[gpui::test]
async fn the_stashes_detail_fades_stop_short_of_the_focused_ring(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, git, painted_fades, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "first\n").unwrap();
    git(repo.path(), &["add", "notes.txt"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
    std::fs::write(repo.path().join("notes.txt"), "second\n").unwrap();
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    landed(&app, cx, "saved-stash-row-0");
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-0");
    let file = cx.debug_bounds("stash-file-0").unwrap();
    cx.simulate_click(file.center(), Modifiers::none());
    landed(&app, cx, "stash-comparison");
    let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
    let slack = px(0.01);
    for text in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        let room = cx.read(appearance::button_ring_room);
        // The tallest window, from 461 × 490 down, whose detail fades.
        let floor = (0..20)
            .map(|step| 490. - 10. * step as f32)
            .find(|height| {
                cx.simulate_resize(size(px(461.), px(*height)));
                draw(cx);
                let list = detail.scroll().bounds();
                list.size.height - room * 2. < crate::tags::SHORTEST_SCROLL_TRACK
                    && detail.scroll().max_offset().y > px(0.)
            })
            .unwrap_or_else(|| panic!("{text} pt: the detail fades in a window 461 px wide"));
        let at = format!("{text} pt in 461 × {floor}");
        let fade = cx
            .update(|_, _| appearance::ui_size(8.))
            .min(detail.scroll().bounds().size.height / 4.);
        cx.update(|window, cx| {
            let list = browser.read(cx).stash_focus.clone();
            window.focus(&list, cx);
        });
        draw(cx);
        let mut at_edge = 0;
        for step in 0..16 {
            cx.simulate_keystrokes("tab");
            draw(cx);
            let Some(control) = cx.update(|window, cx| detail.focused_bounds(window, cx)) else {
                continue;
            };
            let ring = control.dilate(room);
            let list = detail.scroll().bounds();
            let scrolled = -detail.scroll().offset().y;
            let reach = detail.scroll().max_offset().y;
            if (scrolled > px(0.5) && ring.top() < list.top() + fade)
                || (scrolled < reach - px(0.5) && ring.bottom() > list.bottom() - fade)
            {
                at_edge += 1;
            }
            let (top, bottom) = painted_fades(cx, list);
            assert!(
                top.is_none_or(|top| {
                    list.top() + top <= ring.top() + slack || ring.bottom() <= list.top()
                }) && bottom.is_none_or(|bottom| {
                    list.bottom() - bottom >= ring.bottom() - slack || ring.top() >= list.bottom()
                }),
                "{at}, Tab {step}: the fades {top:?} and {bottom:?} of the detail {list:?} \
                 lie over the focused control's ring {ring:?}"
            );
        }
        assert!(
            at_edge > 0,
            "{at}: Tab reveals a control within a fade's height of an edge"
        );
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The saved files cue their overflow as the detail around them does
/// (`tags::ScrollCue`): at 13 and 18 pt in a 1000 × 680 window, each
/// size applied as Settings applies it (`assert_scroll_cue`,
/// `assert_no_scroll_cue` and the gutter's check below each call
/// `tags::tests::set_interface_text_size` for both), eleven
/// unstaged files overflow their box, which shows a scrollbar in a
/// gutter at the rows' right, its thumb following their scroll and
/// covering no row, and the one staged file fits, drawing none, its row
/// keeping the box's whole width. Eleven files once had only a row cut
/// at the box's foot to say more lay below. The files' scroll reaches
/// exactly into the cue's gutter (`ScrollCue::gutter`), so the track
/// runs along its right edge and each row ends where the gutter begins:
/// the list once copied the gutter's sum, which could drift from the
/// cue's.
#[gpui::test]
async fn the_saved_files_cue_their_overflow(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{
        assert_no_scroll_cue, assert_scroll_cue, draw, git, rendered, set_interface_text_size,
        tagged_repository, window,
    };
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let names: Vec<_> = (0..12)
        .map(|index| format!("file-{index:02}.txt"))
        .collect();
    for name in &names {
        std::fs::write(repo.path().join(name), "first\n").unwrap();
        git(repo.path(), &["add", name]);
    }
    git(repo.path(), &["commit", "--quiet", "-m", "Add them"]);
    for name in &names {
        std::fs::write(repo.path().join(name), "second\n").unwrap();
    }
    git(repo.path(), &["add", &names[0]]);
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(1000.), px(680.)));
    draw(cx);
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    landed(&app, cx, "saved-stash-row-0");
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-0");
    let scroll = browser.read_with(cx, |browser, _| {
        browser.file_scroll.0.borrow().base_handle.clone()
    });
    browser.update(cx, |browser, cx| {
        browser.choose_area(StashArea::Unstaged, cx)
    });
    draw(cx);
    assert_eq!(
        browser.read_with(cx, |browser, _| browser.files().len()),
        11,
        "eleven unstaged files"
    );
    assert_scroll_cue(
        cx,
        &app,
        &scroll,
        "stash-file-rows",
        px(0.),
        &["stash-file-0", "stash-file-1", "stash-file-2"],
    );
    let gutter = crate::tags::ScrollCue::gutter();
    for size in [13, 18] {
        set_interface_text_size(cx, &app, size);
        assert!(
            scroll.max_offset().y > px(0.),
            "at {size} pt the eleven files overflow their box"
        );
        let (list, rows) = (scroll.bounds(), rendered(cx, "stash-file-rows"));
        let row = rendered(cx, "stash-file-0");
        assert!(
            (list.right() - rows.right()).abs() < px(0.01)
                && (rows.right() - gutter - row.right()).abs() < px(0.01),
            "at {size} pt the files' scroll {list:?} spans the cue's {gutter:?} gutter \
             in {rows:?}, beside the row {row:?}"
        );
    }
    set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    browser.update(cx, |browser, cx| browser.choose_area(StashArea::Staged, cx));
    draw(cx);
    assert_no_scroll_cue(cx, &app, "stash-file-rows", px(0.), &["stash-file-0"]);
}

/// A stashed 3D model and PDF, at the floor the Stashes dialog keeps
/// its comparison (a scaled 120 px), keep each side's own header at its
/// floor, one control whole with the larger of its ring's room and the
/// header's padding above and below, beneath which the model or page
/// gives way, and Tab reveals each of the header's controls whole with
/// its ring there, at 13 and 18 pt in a 1000 × 560 window with the
/// detail scrolled to the comparison; the PDF's sides take the narrowest
/// form, which at 13 pt fits whole on one 34 px line beneath that floor
/// and is shown unscrolled. The floor once counted the ring's
/// room and the padding together, about 6 px more than either needed.
/// Each side's open control is the detail's too: with the detail
/// scrolled away from the comparison, stacked in a 461 × 490 window and,
/// for the model, in the wide one, focus moved onto it through its
/// header, and at 18 pt Shift+Tab onto it with the header's region
/// scrolled away from it, shows it whole in both from the first frame,
/// the detail holding focus
/// ([`assert_the_detail_reveals_an_open_control`]). The detail once
/// revealed it where the last frame drew it, before the region scrolled,
/// so that first frame painted it below the detail's edge.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_stashed_side_header_keeps_its_floor(cx: &mut gpui_kit::TestAppContext) {
    use crate::focus_reveal::tests::{Control, control};
    use crate::system_preview::tests::{Shown, assert_a_header_keeps_its_floor};
    use crate::tags::tests::{draw, git, scroll_down, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pdf =
        |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
    let document = repo.path().join("document.pdf");
    let model = repo.path().join("shape.obj");
    for (text, scale) in [("first", "2"), ("second", "1")] {
        std::fs::write(&document, pdf(text)).unwrap();
        std::fs::write(
            &model,
            format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n"),
        )
        .unwrap();
        if text == "first" {
            git(repo.path(), &["add", "document.pdf", "shape.obj"]);
            git(repo.path(), &["commit", "--quiet", "-m", "Add them"]);
        }
    }
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
    cx.simulate_resize(size(px(1000.), px(560.)));
    draw(cx);
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    landed(&app, cx, "saved-stash-row-0");
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-1");
    // The detail scrolls; at its end the comparison is in view.
    let to_the_comparison = |cx: &mut VisualTestContext| {
        let middle = cx.debug_bounds("stash-detail").unwrap().center();
        scroll_down(cx, middle, ScrollDelta::Pixels(point(px(0.), px(-10_000.))));
        draw(cx);
    };
    let view = browser.entity_id();
    let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
    // Each side's open control with the detail scrolled away from the
    // comparison, in each of `windows` where the detail's top leaves it
    // out of view: as the window is, and stacked in a narrow one. At
    // 18 pt, where the header's region scrolls, also by Shift+Tab from
    // `next` with the region scrolled away from it.
    let in_the_detail = |cx: &mut VisualTestContext,
                         text: u8,
                         windows: &[(f32, f32)],
                         name: &'static str,
                         (open, next): (&Control, &Control)| {
        let list = cx
            .update(|_, cx| crate::system_preview::side_header(view, name, cx))
            .expect("the side's header");
        let region: &'static str = format!("{name}/header").leak();
        for &(width, height) in windows {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{} in {width} × {height} at {text} pt", open.1);
            assert_the_detail_reveals_an_open_control(
                cx,
                &at,
                (&detail, &list),
                region,
                open,
                None,
            );
            if text == 18 {
                assert_the_detail_reveals_an_open_control(
                    cx,
                    &format!("Shift+Tab onto {at}"),
                    (&detail, &list),
                    region,
                    open,
                    Some(next),
                );
            }
        }
        cx.simulate_resize(size(px(1000.), px(560.)));
        draw(cx);
    };
    let mut cameras = vec![control("model-external", 1)];
    cameras.extend((5..10).map(|index| control("model-camera", index)));
    cameras.extend([control("model-views", 1), control("model-link", 1)]);
    // The stash's files in path order: the document, then the model.
    let file = cx.debug_bounds("stash-file-1").unwrap();
    cx.simulate_click(file.center(), Modifiers::none());
    landed(&app, cx, "model-side-1/header");
    for (text, floor) in [(13, 34.), (18, 48.)] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        draw(cx);
        to_the_comparison(cx);
        assert_a_header_keeps_its_floor(
            cx,
            &format!("the stashed model's After at {text} pt"),
            (view, "model-side-1"),
            (px(floor), Shown::Floor),
            &control("model-source", 1),
            &cameras,
        );
        let windows = [(1000., 560.), (461., 490.)];
        let open = (&cameras[0], &cameras[1]);
        in_the_detail(cx, text, &windows, "model-side-1", open);
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    draw(cx);
    let middle = cx.debug_bounds("stash-detail").unwrap().center();
    scroll_down(cx, middle, ScrollDelta::Pixels(point(px(0.), px(10_000.))));
    draw(cx);
    let file = cx.debug_bounds("stash-file-0").unwrap();
    cx.simulate_click(file.center(), Modifiers::none());
    landed(&app, cx, "system-preview-1");
    browser.update(cx, |browser, cx| {
        let Some(Content::Rich(preview)) = browser.content.as_deref() else {
            panic!("a changed PDF is a document comparison");
        };
        browser.content = Some(Arc::new(crate::pdf_view::tests::paged(preview)));
        cx.notify();
    });
    draw(cx);
    crate::pdf_view::tests::drain_pages();
    draw(cx);
    // At 13 pt the narrowest form, its heading and its run on one line a
    // quarter rem from its edges, 34 px, fits whole beneath the 41 px
    // floor; at 18 pt its run takes a line beneath its heading and it
    // scrolls at its floor.
    for (text, floor, shown) in [(13, 41., Shown::Whole), (18, 57., Shown::FloorOneLine)] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        draw(cx);
        to_the_comparison(cx);
        for side in [0, 1] {
            // The narrowest header a narrow box's side takes, from the
            // page number (Previous is unavailable on the first page).
            let controls = ["pdf-forward", "pdf-more"]
                .map(|name| control(name, side))
                .to_vec();
            let name = if side == 0 {
                "pdf-side-0"
            } else {
                "pdf-side-1"
            };
            assert_eq!(
                crate::pdf_view::DRAWN_FORMS.with_borrow(|drawn| drawn.get(&side).copied()),
                Some(crate::pdf_view::HeaderForm::Narrowest),
                "the stashed {name} at {text} pt"
            );
            assert_a_header_keeps_its_floor(
                cx,
                &format!("the stashed {name} at {text} pt"),
                (view, name),
                (px(floor), shown),
                &control("pdf-page-number", side),
                &controls,
            );
        }
        // The wide window's detail shows a PDF's open controls at its top.
        for (side, name) in [(0, "pdf-side-0"), (1, "pdf-side-1")] {
            let windows = [(461., 490.)];
            // The narrowest header's first control and the next.
            let open = (
                &control("pdf-page-number", side),
                &control("pdf-forward", side),
            );
            in_the_detail(cx, text, &windows, name, open);
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The side headers the Stashes dialog draws for a stashed 3D model go
/// with the dialog: their scroll and focus state, held by the view that
/// draws them, once stayed in an application-wide table after the
/// dialog closed, a model's two sides for every dialog opened.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_closed_stash_dialog_takes_its_side_headers(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let model = repo.path().join("shape.obj");
    std::fs::write(&model, b"v 0 0 0\nv 2 0 0\nv 0 2 0\nf 1 2 3\n").unwrap();
    git(repo.path(), &["add", "shape.obj"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add a model"]);
    std::fs::write(&model, b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    git(repo.path(), &["stash", "push", "-m", "kept for later"]);
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(1480.), px(980.)));
    draw(cx);
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    landed(&app, cx, "saved-stash-row-0");
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    landed(&app, cx, "stash-file-0");
    let file = cx.debug_bounds("stash-file-0").unwrap();
    cx.simulate_click(file.center(), Modifiers::none());
    landed(&app, cx, "model-side-1/header");
    let view = browser.entity_id();
    let held = |cx: &mut VisualTestContext| {
        ["model-side-0", "model-side-1"]
            .into_iter()
            .filter(|name| {
                cx.update(|_, cx| crate::system_preview::side_header(view, name, cx))
                    .is_some()
            })
            .count()
    };
    assert_eq!(held(cx), 2, "the dialog draws the model's two side headers");
    let weak = browser.downgrade();
    drop(browser);
    cx.update(|window, cx| window.close_dialog(cx));
    draw(cx);
    assert!(
        weak.upgrade().is_none(),
        "the closed dialog's view is released"
    );
    assert_eq!(held(cx), 0, "the closed dialog's side headers go with it");
}

/// Lets every quiet read the window has asked for land: each runs on
/// the window's reader, a thread `run_until_parked` cannot wait for.
fn quiet_reads_land(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
    for _ in 0..8 {
        if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
            return;
        }
        app.read_with(cx, |app, _| app.worker.wait_idle());
        cx.executor().run_until_parked();
    }
    panic!("a quiet read asked for never ran");
}

/// Reads the working state, as Refresh does, and lets it land: a merge
/// paused outside the app is the window's pause from then on.
async fn read_pause(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(app, cx).await;
    quiet_reads_land(app, cx);
    assert!(
        app.read_with(cx, |app, _| app.integration_state.is_some()),
        "the merge paused"
    );
}

/// Draws a frame without running the window's tasks, so a reply
/// waiting on the reader stays unread.
fn frame(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
}

/// The saved-work list is one Tab stop, in 461 × 490 and 1480 × 980
/// windows at 13 and 18 pt: Tab reaches it, it draws its keyboard ring
/// whole on its own edge, and Down moves the selection to the second of
/// two stashes, which the list scrolls whole into its view, clear of the
/// Previous/More pager beneath it. Its heading counts the stashes
/// ("Saved work · 2"), and while its rows overflow it, as the stacked
/// list's one row does in a half tile, it shows its scrollbar. The list
/// was never a Tab stop, its focus handle not one, and the stacked list
/// showed one stash with nothing saying a second lay below it, under the
/// pager.
#[gpui::test]
async fn the_stash_list_is_one_stop_that_counts_and_cues_its_stashes(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    for (name, text) in [("Index only", "first\n"), ("Layout experiment", "second\n")] {
        std::fs::write(repo.path().join("notes.txt"), text).unwrap();
        git(
            repo.path(),
            &["stash", "push", "--include-untracked", "-m", name],
        );
    }
    let (app, cx) = window(cx, &repo);
    crate::focus_ring::sweep::prepare(&app, cx);
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    let slack = px(0.5);
    for (width, height) in [(461., 490.), (1480., 980.)] {
        for interface in [13, 18] {
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
            landed(&app, cx, "saved-stash-row-0");
            let browser = app
                .read_with(cx, |app, _| app.recovery.browser.clone())
                .and_then(|browser| browser.upgrade())
                .expect("Stashes is open");
            assert_eq!(
                crate::native_accessibility::written::get("stash-list-heading")
                    .and_then(|heading| heading.node.value().map(str::to_owned)),
                Some("Saved work · 2".to_owned()),
                "{at}: the heading counts the stashes"
            );
            let list = cx.debug_bounds("stash-list").expect("the list");
            let pager = cx.debug_bounds("stash-pager").expect("the pager");
            let row = appearance::ui_size(58.);
            let overflows = row * 2. > list.size.height + slack;
            assert_eq!(
                overflows,
                width < 800.,
                "{at}: the stacked list holds one row, the list beside the detail both"
            );
            assert_eq!(
                cx.debug_bounds("saved-stashes-scrollbar").is_some(),
                overflows,
                "{at}: the scrollbar shows while the rows overflow {list:?}"
            );
            // Tab reaches the list.
            let stop = browser.read_with(cx, |browser, _| browser.stash_focus.clone());
            let mut reached = false;
            for _ in 0..12 {
                cx.simulate_keystrokes("tab");
                draw(cx);
                if cx.update(|window, _| stop.is_focused(window)) {
                    reached = true;
                    break;
                }
            }
            assert!(reached, "{at}: Tab reaches the saved-work list");
            let ring = crate::focus_ring::painted_outlines(cx, accent)
                .into_iter()
                .find(|outline| outline.bounds == list)
                .unwrap_or_else(|| panic!("{at}: the list {list:?} draws no ring"));
            assert!(ring.whole, "{at}: the list's ring {ring:?} is whole");
            // Down twice selects the second stash and shows it whole.
            cx.simulate_keystrokes("down");
            draw(cx);
            cx.simulate_keystrokes("down");
            draw(cx);
            assert_eq!(
                browser.read_with(cx, |browser, _| browser.selected),
                Some(1),
                "{at}: Down reaches the second stash"
            );
            let second = cx
                .debug_bounds("saved-stash-row-1")
                .expect("the second stash's row");
            assert!(
                second.top() >= list.top() - slack
                    && second.bottom() <= list.bottom() + slack
                    && second.bottom() <= pager.top() + slack,
                "{at}: the second stash {second:?} lies whole in the list {list:?}, above the pager {pager:?}"
            );
            assert!(
                cx.update(|window, _| stop.is_focused(window)),
                "{at}: the list keeps focus"
            );
            drop(browser);
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The saved files are one Tab stop, with the arrow keys inside, at 13
/// pt in a 1480 × 980 window and at 18 pt in a 461 × 490 one: Tab
/// reaches them, and they draw the keyboard ring whole on their own edge
/// in the detail's view; Down moves the selection to the first and then
/// the second of four files and End to the last, each whole in the list,
/// which keeps focus. Stacked beneath the saved-work list at 18 pt, where
/// the detail is about 60 px tall, they are no taller than it holds with
/// their ring. Their focus handle was no Tab stop, so Tab passed them by,
/// and at 85 px in that detail no reveal showed them whole: its clip cut
/// their ring.
#[gpui::test]
async fn the_saved_files_are_one_stop_whole_in_the_detail(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    for name in ["alpha.txt", "beta.txt", "delta.txt", "gamma.txt"] {
        std::fs::write(repo.path().join(name), format!("{name}\n")).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["stash", "push", "-m", "four files"]);
    let (app, cx) = window(cx, &repo);
    crate::focus_ring::sweep::prepare(&app, cx);
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    let slack = px(0.5);
    let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    };
    let bounds = |cx: &mut VisualTestContext, name: &'static str| {
        cx.debug_bounds(name).unwrap_or_else(|| panic!("no {name}"))
    };
    for (width, height, interface) in [(1480., 980., 13), (461., 490., 18)] {
        let at = format!("{interface} pt in {width} × {height}");
        cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        assert_eq!(
            stashes.read_with(cx, |stashes, _| (stashes.area, stashes.files().len())),
            (StashArea::Staged, 4),
            "{at}: four saved files"
        );
        // Tab reaches the saved files, from the stash list.
        let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
        let focused = |cx: &mut VisualTestContext| cx.update(|window, _| files.is_focused(window));
        let mut reached = false;
        for _ in 0..12 {
            cx.simulate_keystrokes("tab");
            draw(cx);
            if focused(cx) {
                reached = true;
                break;
            }
        }
        assert!(reached, "{at}: Tab reaches the saved files");
        draw(cx);
        let [list, view] = [bounds(cx, "stash-files"), bounds(cx, "stash-detail")];
        assert!(
            inside(list, view),
            "{at}: the saved files {list:?} lie whole in the detail's view {view:?}"
        );
        let ring = crate::focus_ring::painted_outlines(cx, accent)
            .into_iter()
            .find(|outline| outline.bounds == list)
            .unwrap_or_else(|| panic!("{at}: the saved files {list:?} draw no ring"));
        assert!(ring.whole, "{at}: their ring {ring:?} is whole");
        // The arrow keys move the selection inside them.
        for (key, index, row) in [
            ("down", 0, "stash-file-0"),
            ("down", 1, "stash-file-1"),
            ("end", 3, "stash-file-3"),
        ] {
            cx.simulate_keystrokes(key);
            draw(cx);
            assert_eq!(
                stashes.read_with(cx, |stashes, _| stashes.selected_file),
                Some(index),
                "{at}: {key} selects file {index}"
            );
            let [row, list] = [bounds(cx, row), bounds(cx, "stash-files")];
            assert!(
                inside(row, list),
                "{at}: {key} shows file {index} {row:?} whole in the list {list:?}"
            );
            assert!(focused(cx), "{at}: the saved files keep focus after {key}");
        }
        drop(stashes);
        cx.simulate_keystrokes("escape");
        draw(cx);
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The fixture of Stashes' Review restore… regressions: a repository
/// with two stashes on `main`, "Index only" and the newest, "Layout
/// experiment", and a branch `side` whose merge pauses cleanly, as the
/// native review's `dlg` fixture has. Returns its fixture Git.
fn two_stashes(repository: &std::path::Path) -> impl Fn(&[&str]) + '_ {
    let git = move |arguments: &[&str]| {
        let identity = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ];
        crate::views::tests::fixture_git(repository, &[&identity[..], arguments].concat())
    };
    git(&["reset", "-q", "--hard"]);
    git(&["checkout", "-q", "-b", "side"]);
    std::fs::write(repository.join("side.txt"), "side\n").unwrap();
    git(&["add", "side.txt"]);
    git(&["commit", "-q", "-m", "side"]);
    git(&["checkout", "-q", "main"]);
    for (name, text) in [("Index only", "first\n"), ("Layout experiment", "second\n")] {
        std::fs::write(repository.join("notes.txt"), text).unwrap();
        git(&["stash", "push", "-q", "-m", name]);
    }
    git
}

/// Lets a recovery action's preparation, which runs on the window's
/// operation executor, answer and draws its outcome.
fn recovery_lands(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
    app.read_with(cx, |app, _| app.operations.drain());
    cx.executor().run_until_parked();
    crate::views::tests::draw(cx);
}

/// Moves focus with Tab onto the open Stashes' control `id`, which the
/// stacked detail scrolls into view with its ring.
fn tab_to(browser: &Entity<StashBrowser>, id: &'static str, cx: &mut gpui::VisualTestContext) {
    for _ in 0..40 {
        cx.simulate_keystrokes("tab");
        crate::views::tests::draw(cx);
        let focused = cx.update(|window, cx| browser.read(cx).detail.focused(window));
        if focused == Some(id.into()) {
            return;
        }
    }
    panic!("Tab never reached {id}");
}

/// A whole press of Space on the focused control: GPUI's keyboard click
/// waits for the key up.
fn space(cx: &mut gpui::VisualTestContext) {
    let keystroke = gpui::Keystroke::parse("space").expect("a keystroke");
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
}

/// Moves focus with Tab onto the open Stashes' Review restore…, which
/// the stacked detail scrolls into view, and presses it with Space, as
/// the keyboard does: Enter would confirm the dialog, closing it.
fn press_review_restore(browser: &Entity<StashBrowser>, cx: &mut gpui::VisualTestContext) {
    tab_to(browser, "restore-saved-stash", cx);
    space(cx);
}

/// Every node an assistive technology reaches in the window's tree as
/// the next frame writes it, through AccessKit's filter, under the
/// dialog's own node: Stashes' footer note in the detail that clips it,
/// and the window's alert and announcement beside the dialog layer.
fn reached(cx: &mut gpui::VisualTestContext) -> Vec<crate::native_accessibility::Reachable> {
    use crate::native_accessibility::{reachable, written};
    written::forget();
    crate::views::tests::draw(cx);
    let (size, scale) =
        cx.update(|window, _| (window.viewport_size(), f64::from(window.scale_factor())));
    let edge = |value: Pixels| f64::from(f32::from(value)) * scale;
    let mut dialog = gpui::accesskit::Node::new(Role::Dialog);
    dialog.set_modal();
    let window = gpui::accesskit::Rect {
        x0: 0.,
        y0: 0.,
        x1: edge(size.width),
        y1: edge(size.height),
    };
    let tree = written::Assembly::default()
        .node("dialog", dialog, None)
        .everything(Some("dialog"))
        .tree(window, None);
    reachable(tree)
}

/// How many live regions in `found` say `text`, announced as `live`,
/// outside every node that clips its children, as the window's alert
/// and announcement are, wherever Stashes' detail has scrolled.
fn said_from_the_window(
    found: &[crate::native_accessibility::Reachable],
    text: &str,
    live: gpui::accesskit::Live,
) -> usize {
    found
        .iter()
        .filter(|node| {
            node.name.as_deref() == Some(text) && node.live == live && node.clip.is_none()
        })
        .count()
}

/// What the window's AT-SPI adapter announces as it is fed the
/// window's frames, as GPUI's Linux backend feeds it; no adapter runs
/// elsewhere.
struct Heard {
    #[cfg(target_os = "linux")]
    bus: crate::native_accessibility::atspi_state_tests::Bus,
}

impl Heard {
    /// The adapter as it starts on the window's next frame.
    fn start(cx: &mut gpui::VisualTestContext) -> Self {
        #[cfg(target_os = "linux")]
        {
            use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
            Self {
                bus: Bus::new(next_frame(cx)),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            crate::views::tests::draw(cx);
            Self {}
        }
    }

    /// Feeds the adapter the window's next frame and returns what it
    /// announced since the last on Linux, `None` elsewhere.
    fn next(&mut self, cx: &mut gpui::VisualTestContext) -> Option<Vec<String>> {
        #[cfg(target_os = "linux")]
        {
            use crate::native_accessibility::atspi_state_tests::next_frame;
            self.bus.update(next_frame(cx));
            Some(self.bus.announced())
        }
        #[cfg(not(target_os = "linux"))]
        {
            crate::views::tests::draw(cx);
            None
        }
    }

    /// The list items the adapter reported renamed in the frames fed
    /// since the last call, by the name each reads now, which Orca speaks
    /// for the item it presents, on Linux; `None` elsewhere.
    fn renamed_items(&mut self) -> Option<Vec<String>> {
        #[cfg(target_os = "linux")]
        {
            use accesskit_atspi_common::Role;
            Some(
                self.bus
                    .renamed()
                    .into_iter()
                    .filter(|(role, _)| *role == Role::ListItem)
                    .map(|(_, name)| name)
                    .collect(),
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
}

/// A stash whose read fails is said from the window's alert in the frame
/// that draws its line, but a failure for a stash the person moved off
/// before any frame drew it is taken back unheard, as Review restore…'s
/// refusal is: the older stash's commit gone, choosing it and then the
/// newest stash before a frame says nothing, and choosing it and waiting
/// says its failure. The failure was said whatever had been chosen since.
#[gpui::test]
async fn a_failed_read_left_before_it_shows_is_never_said(cx: &mut gpui::TestAppContext) {
    use crate::copy_feedback::tests::alerted;
    let (_fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let _git = two_stashes(&repository);
    cx.simulate_resize(size(px(1480.), px(980.)));
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    assert_eq!(alerted(cx), None, "nothing is said as Stashes opens");
    // The older stash's saved tree goes, which no other stash holds, so
    // reading it fails while the newest still reads.
    let commit = stashes.read_with(cx, |stashes, _| {
        stashes.page.as_ref().expect("the list").entries[1]
            .oid
            .clone()
    });
    let tree = std::process::Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args(["rev-parse", &format!("{commit}^{{tree}}")])
        .output()
        .unwrap();
    let oid = String::from_utf8(tree.stdout).unwrap().trim().to_owned();
    std::fs::remove_file(
        repository
            .join(".git/objects")
            .join(&oid[..2])
            .join(&oid[2..]),
    )
    .unwrap();
    let choose = |index: usize, cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            stashes.update(cx, |stashes, cx| stashes.select_stash(index, window, cx))
        });
    };
    // Failed, then left before a frame draws it, as a click handled
    // before the display's next frame is: the test window draws as soon
    // as the failure's update ends, so the choice is made from an
    // observer, which runs first.
    choose(1, cx);
    let chooses = cx.update(|window, cx| {
        window.observe(&stashes, cx, |stashes, window, cx| {
            let failed = stashes.read(cx);
            if failed.error.is_some() && failed.selected == Some(1) {
                stashes.update(cx, |stashes, cx| stashes.select_stash(0, window, cx));
            }
        })
    });
    stashes.read_with(cx, |stashes, _| stashes.metadata.drain());
    crate::views::tests::draw(cx);
    drop(chooses);
    StashBrowser::lands(&stashes, cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| (stashes.selected, stashes.error.clone())),
        (Some(0), None),
        "the newest stash is chosen and reads"
    );
    assert_eq!(
        alerted(cx),
        None,
        "a failure for the stash moved off is never said"
    );
    // Failed and shown: said with its line.
    choose(1, cx);
    StashBrowser::lands(&stashes, cx);
    let error = stashes
        .read_with(cx, |stashes, _| stashes.error.clone())
        .expect("the read fails again");
    let (_, said) = alerted(cx).expect("the failure shown is said");
    assert_eq!(said, error);
}

/// Stashes' footer buttons stand on the compact frame (`button_frame`),
/// as Drop… beside Review restore… does: Review restore…, Drop… and the
/// dialog's Close are each the frame's 28 px at the interface size, at 13
/// and 18 pt, beside the list and stacked beneath it. Review restore…, a
/// small kit Button, stood 19.5 px beside Drop…'s 28 at 13 pt, and Close,
/// the kit's own, 26 px.
#[gpui::test]
async fn the_footer_buttons_stand_on_the_compact_frame(cx: &mut gpui::TestAppContext) {
    let (_fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let _git = two_stashes(&repository);
    let mut failures = Vec::new();
    for (width, height) in [(1480., 980.), (461., 490.)] {
        cx.simulate_resize(size(px(width), px(height)));
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            open_selected_stash(&app, cx).await;
            crate::views::tests::draw(cx);
            let frame = appearance::ui_size(28.);
            for control in [
                "restore-saved-stash",
                "drop-saved-stash",
                "dialog-ok: Close",
            ] {
                let at = format!("{control} at {text} pt in {width} × {height}");
                match cx.debug_bounds(control) {
                    None => failures.push(format!("{at}: not drawn")),
                    // Snapped to the test window's 2× device pixels.
                    Some(drawn) if (drawn.size.height - frame).abs() > px(0.5) => {
                        failures.push(format!(
                            "{at}: {:?} tall, not the frame's {frame:?}",
                            drawn.size.height
                        ))
                    }
                    Some(_) => {}
                }
            }
            cx.update(|window, cx| window.close_all_dialogs(cx));
            crate::views::tests::draw(cx);
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Stacked beneath the list at 18 pt in a 461 × 490 window, the detail
/// holds Review restore… and a line of its note together before anything
/// scrolls: as Stashes opens, its view is already as tall as Review
/// restore…'s frame, the note's `gap_2` and line beneath it and the
/// ring's room at either edge, taken from the list panel's spare height
/// and the gap above the detail; the note lies `gap_2` beneath the
/// actions at 13 and 18 pt, as beside the list; and once Review
/// restore…, pressed from the keyboard with a merge paused, is refused,
/// the refusal lies on its own line beneath it, both whole in that view,
/// and only the detail has scrolled, the list and the gap above it where
/// they were drawn. The detail kept 57.5 px there, too short for the
/// frame and the line, and the note once lay `gap_1` beneath the actions
/// in every narrow window, 3.5 px higher at 13 pt than beside the list.
/// At 13 pt the list and the gap keep their heights.
#[gpui::test]
async fn the_stacked_detail_holds_review_restore_and_its_note(cx: &mut gpui::TestAppContext) {
    let (_fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    // Stashes otherwise slides in on a wall-clock animation, which moves
    // the whole dialog between the frames compared below.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    cx.simulate_resize(size(px(461.), px(490.)));
    let slack = px(0.5);
    for text in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        open_selected_stash(&app, cx).await;
        crate::views::tests::draw(cx);
        let browser = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        let scroll = browser.read_with(cx, |browser, _| browser.detail.scroll().clone());
        assert_eq!(
            scroll.offset(),
            Point::default(),
            "{text} pt: nothing scrolled"
        );
        let (room, line, rem) = cx.update(|window, cx| {
            (
                appearance::button_ring_room(cx),
                crate::image_compare::text_line(appearance::ui_text(10.), window),
                window.rem_size(),
            )
        });
        let [list, gap, view, actions, note] = [
            "stash-list-panel",
            "stash-stack-gap",
            "stash-detail",
            "stash-actions",
            "stash-restore-note",
        ]
        .map(|name| cx.debug_bounds(name).expect(name));
        let held = room * 2. + appearance::ui_size(28.) + rem * 0.5 + line;
        assert!(
            view.size.height >= held - slack,
            "{text} pt: the detail's view {view:?} holds Review restore… and its line, {held:?}"
        );
        let beneath = cx.update(|window, _| window.pixel_snap(rem * 0.5));
        assert!(
            (note.top() - actions.bottom() - beneath).abs() < slack,
            "{text} pt: the note {note:?} lies gap_2, {beneath:?}, beneath the actions \
             {actions:?}, as beside the list"
        );
        if text == 13 {
            assert!(
                (list.size.height - appearance::ui_size(140.)).abs() < slack
                    && (gap.size.height - rem * 0.75).abs() < slack,
                "13 pt: the list {list:?} and the gap {gap:?} keep their heights"
            );
        } else {
            assert!(
                list.size.height < appearance::ui_size(140.) - slack
                    && list.size.height >= StashBrowser::stacked_list_minimum() - slack
                    && gap.size.height >= rem * 0.25 - slack,
                "18 pt: the room comes from the list's spare height {list:?} and the gap {gap:?}"
            );
        }
        // Refused for the paused merge, from the keyboard: the refusal
        // takes the note's place, revealed with Review restore…. Only
        // the detail scrolls, as Tab brings Review restore… into its
        // view and the refusal follows: the dialog's content, the list
        // above it and the gap stay where they were drawn.
        press_review_restore(&browser, cx);
        recovery_lands(&app, cx);
        assert_eq!(
            ["stash-list-panel", "stash-stack-gap"].map(|name| cx.debug_bounds(name)),
            [Some(list), Some(gap)],
            "{text} pt: nothing but the detail scrolled"
        );
        let [restore, note, view] = [
            "restore-saved-stash",
            "stash-restore-refusal",
            "stash-detail",
        ]
        .map(|name| cx.debug_bounds(name).expect(name));
        assert!(
            note.top() >= restore.bottom(),
            "{text} pt: the refusal {note:?} lies on its own line beneath Review restore… {restore:?}"
        );
        assert!(
            restore.top() - room >= view.top() - slack
                && note.top() + line <= view.bottom() - room + slack,
            "{text} pt: Review restore… {restore:?} and the refusal's first line {note:?} \
             lie whole in the detail's view {view:?}"
        );
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
        git(&["merge", "--abort"]);
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// Review restore… answers only what Stashes still shows, with the
/// window's operations held so Git has not answered. Pressed on "Layout
/// experiment", then "Index only" chosen: the preparation is cancelled
/// at once, Review restore… can be pressed again, and once Git would
/// have answered no review of "Layout experiment" opens; pressed again,
/// it reviews "Index only". "Layout experiment" chosen again meanwhile,
/// by a click on its row, Up on the first row and Home: nothing is read
/// again and its review opens, where each read it again and cancelled
/// the preparation with nothing said. The list read again meanwhile, as
/// Refresh reads it: cancelled at once, and no review opens. Restore staged
/// state chosen meanwhile: no review of the restoration without it
/// opens, and pressed again the review restores it. Stashes closed with
/// Escape: its browser's release cancels, and nothing opens or is said.
/// An answer that outruns the
/// cancellations, here for a choice changed behind their back, is
/// dropped by the answer check: no review for the other Restore staged
/// state, no refusal under a selection moved on. Stacked at 18 pt in a
/// 461 × 490 window, with a merge paused: no refusal lands under the
/// stash chosen meanwhile, and a refusal that lands with another stash
/// chosen before a frame draws it is never heard and does not scroll
/// that stash's detail to its foot once its inspection lands, while one
/// a frame drew keeps its alert as another stash is chosen. Each
/// answered what had been asked once it landed: the old stash's review
/// opened, the refusal lay under the other stash, the review restored
/// without the staged state, and the review popped up after Stashes had
/// closed; the refusal's reveal, still due, scrolled the next stash's
/// detail to its foot; and the window's alert said the refusal no frame
/// had shown, for 1.5 s beside the stash chosen.
#[gpui::test]
async fn review_restore_answers_only_what_stashes_still_shows(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    let browser = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open")
    };
    let preparing = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |app, _| app.operation_busy == Some(PREPARING))
    };
    let pressable = || {
        crate::workspace::refusals::get("restore-saved-stash")
            .is_some_and(|refusal| !refusal.announced && !refusal.dimmed)
    };
    // The review open, by its explanation.
    let reviewed = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("operation-consequences").map(|_| {
            crate::native_accessibility::written::get("operation-consequences")
                .and_then(|text| text.node.value().map(str::to_owned))
                .unwrap_or_default()
        })
    };
    let close = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
    };
    cx.simulate_resize(size(px(1480.), px(980.)));

    // Another stash chosen while Git prepares.
    open_selected_stash(&app, cx).await;
    let stashes = browser(cx);
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    assert!(
        preparing(cx),
        "Git prepares Layout experiment's restoration"
    );
    let row = cx
        .debug_bounds("saved-stash-row-1")
        .expect("Index only's row");
    cx.simulate_click(row.center(), Modifiers::none());
    StashBrowser::lands(&stashes, cx);
    assert!(!preparing(cx), "choosing Index only cancels it");
    assert!(pressable(), "Review restore… can be pressed again at once");
    drop(held);
    recovery_lands(&app, cx);
    assert_eq!(reviewed(cx), None, "no review of Layout experiment opens");
    assert_eq!(
        stashes.read_with(cx, |stashes, _| stashes.selected),
        Some(1)
    );
    press_review_restore(&stashes, cx);
    recovery_lands(&app, cx);
    let review = reviewed(cx).expect("Index only's review opens");
    assert!(
        review.starts_with("Restore “On main: Index only”"),
        "{review}"
    );
    drop(stashes);
    close(cx);

    // The stash shown chosen again while Git prepares, by a click on its
    // row, then Up on the first row and Home: nothing is read again, the
    // preparation goes on and its review opens.
    open_selected_stash(&app, cx).await;
    let stashes = browser(cx);
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    let generation = stashes.read_with(cx, |stashes, _| stashes.generation);
    let row = cx
        .debug_bounds("saved-stash-row-0")
        .expect("Layout experiment's row");
    cx.simulate_click(row.center(), Modifiers::none());
    crate::views::tests::draw(cx);
    // The click chose the row, which focuses the list: one that missed
    // it, leaving focus on Review restore…, would change nothing too.
    let list = stashes.read_with(cx, |stashes, _| stashes.stash_focus.clone());
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "the click on Layout experiment's row focuses the list"
    );
    for key in ["up", "home"] {
        cx.simulate_keystrokes(key);
        crate::views::tests::draw(cx);
    }
    assert!(
        preparing(cx),
        "choosing Layout experiment again cancels nothing"
    );
    assert_eq!(
        stashes.read_with(cx, |stashes, _| (
            stashes.generation,
            stashes.metadata_task.is_none(),
            stashes.selected
        )),
        (generation, true, Some(0)),
        "nothing is read again"
    );
    drop(held);
    recovery_lands(&app, cx);
    let review = reviewed(cx).expect("Layout experiment's review opens");
    assert!(
        review.starts_with("Restore “On main: Layout experiment”"),
        "{review}"
    );
    drop(stashes);
    close(cx);

    // The list read again while Git prepares, as Refresh, Previous and
    // More read it.
    open_selected_stash(&app, cx).await;
    let stashes = browser(cx);
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    assert!(preparing(cx), "Git prepares the restoration");
    cx.update(|window, cx| stashes.update(cx, |stashes, cx| stashes.load_page(0, window, cx)));
    StashBrowser::lands(&stashes, cx);
    assert!(!preparing(cx), "reading the list again cancels it");
    drop(held);
    recovery_lands(&app, cx);
    assert_eq!(
        reviewed(cx),
        None,
        "no review opens once the list is read again"
    );
    drop(stashes);
    close(cx);

    // The other Restore staged state chosen while Git prepares.
    open_selected_stash(&app, cx).await;
    let stashes = browser(cx);
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    assert!(preparing(cx), "Git prepares the restoration");
    tab_to(&stashes, "stash-restore-index", cx);
    space(cx);
    crate::views::tests::draw(cx);
    assert!(stashes.read_with(cx, |stashes, _| stashes.restore_index));
    assert!(!preparing(cx), "choosing Restore staged state cancels it");
    assert!(pressable(), "Review restore… can be pressed again at once");
    drop(held);
    recovery_lands(&app, cx);
    assert_eq!(
        reviewed(cx),
        None,
        "no review without the staged state opens"
    );
    press_review_restore(&stashes, cx);
    recovery_lands(&app, cx);
    let review = reviewed(cx).expect("the review opens");
    assert!(
        review.contains("Restore the original staged state"),
        "{review}"
    );
    drop(stashes);
    close(cx);

    // Stashes closed while Git prepares, its dialog holding the
    // browser's only handle: the browser's release cancels.
    open_selected_stash(&app, cx).await;
    let stashes = browser(cx);
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    drop(stashes);
    cx.simulate_keystrokes("escape");
    crate::views::tests::draw(cx);
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "Escape closes Stashes"
    );
    assert!(!preparing(cx), "closing Stashes cancels it");
    drop(held);
    recovery_lands(&app, cx);
    assert_eq!(
        reviewed(cx),
        None,
        "no review opens once Stashes has closed"
    );
    assert_eq!(
        app.read_with(cx, |app, _| app.operation_error_text()),
        None,
        "and nothing is said"
    );

    // An answer that outruns the cancellations, for the other Restore
    // staged state chosen behind their back: the answer check drops it.
    open_selected_stash(&app, cx).await;
    let stashes = browser(cx);
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    stashes.update(cx, |stashes, _| {
        stashes.restore_index = !stashes.restore_index
    });
    assert!(preparing(cx), "nothing cancels the preparation");
    drop(held);
    recovery_lands(&app, cx);
    assert_eq!(
        reviewed(cx),
        None,
        "no review for the choice no longer shown"
    );
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "Stashes stays open"
    );
    drop(stashes);
    close(cx);

    // Stacked, with a merge paused: another stash chosen while Git
    // prepares, a refusal that outruns the cancellations, and one that
    // lands as another stash is chosen.
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    open_selected_stash(&app, cx).await;
    let stashes = browser(cx);
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    cx.update(|window, cx| stashes.update(cx, |stashes, cx| stashes.select_stash(1, window, cx)));
    StashBrowser::lands(&stashes, cx);
    assert!(!preparing(cx), "choosing Index only cancels it");
    drop(held);
    recovery_lands(&app, cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| (
            stashes.selected,
            stashes.restore_line.clone()
        )),
        (Some(1), None),
        "no refusal lands under Index only"
    );
    assert!(cx.debug_bounds("stash-restore-refusal").is_none());
    // The selection moved on behind the cancellations' back: the answer
    // check drops the refusal.
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    stashes.update(cx, |stashes, _| {
        stashes.generation = stashes.generation.wrapping_add(1)
    });
    assert!(preparing(cx), "nothing cancels the preparation");
    drop(held);
    recovery_lands(&app, cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
        None,
        "no refusal for the selection no longer shown"
    );
    // "Layout experiment" chosen as Index only's refusal lands, before a
    // frame draws it, as a click handled before the display's next
    // frame is: the test window draws as soon as the refusal's update
    // ends, so the choice is made from an observer, which runs first.
    let held = app.update(cx, |app, _| app.operations.hold());
    press_review_restore(&stashes, cx);
    let alerted = crate::copy_feedback::tests::alerted(cx).map(|(id, _)| id);
    let chooses = cx.update(|window, cx| {
        window.observe(&stashes, cx, |stashes, window, cx| {
            let refused = stashes.read(cx);
            if refused.restore_line.is_some() && refused.selected == Some(1) {
                stashes.update(cx, |stashes, cx| stashes.select_stash(0, window, cx));
            }
        })
    });
    drop(held);
    recovery_lands(&app, cx);
    drop(chooses);
    let refused = crate::copy_feedback::tests::alerted(cx)
        .filter(|(id, text)| Some(id) != alerted.as_ref() && text == PAUSED_REFUSAL);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| (
            stashes.selected,
            stashes.restore_line.clone()
        )),
        (Some(0), None),
        "Layout experiment chosen"
    );
    assert_eq!(
        refused, None,
        "Index only's refusal, which no frame drew, is never said"
    );
    StashBrowser::lands(&stashes, cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| stashes.detail.scroll().offset().y),
        px(0.),
        "Layout experiment's detail opens at its top"
    );
    // A refusal a frame has drawn has been said with it: choosing
    // another stash then leaves its alert to end in its own time.
    press_review_restore(&stashes, cx);
    recovery_lands(&app, cx);
    let said = crate::copy_feedback::tests::alerted(cx).filter(|(_, text)| text == PAUSED_REFUSAL);
    assert!(said.is_some(), "Layout experiment is refused");
    cx.update(|window, cx| stashes.update(cx, |stashes, cx| stashes.select_stash(1, window, cx)));
    StashBrowser::lands(&stashes, cx);
    assert_eq!(
        crate::copy_feedback::tests::alerted(cx),
        said,
        "the drawn refusal's alert stays as Index only is chosen"
    );
    git(&["merge", "--abort"]);
    drop(stashes);
    close(cx);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// Review restore…'s preparation cancelled, as another stash is chosen
/// or Stashes closes, runs the working read Refresh asked for while the
/// restoration was prepared, as a form's Cancel does
/// (`GitTurtle::cancel_recovery_preparation`): the preparation's end,
/// which would have read it, never runs. The read stayed noted, and the
/// working state stayed as read before it until something else ended.
#[gpui::test]
async fn a_refresh_asked_for_while_review_restore_prepares_runs_once_it_is_cancelled(
    cx: &mut gpui::TestAppContext,
) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let _git = two_stashes(&repository);
    cx.simulate_resize(size(px(1480.), px(980.)));
    let browser = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open")
    };
    let listed = |cx: &mut gpui::VisualTestContext, path: &str| {
        app.read_with(cx, |app, _| {
            app.work_status.as_ref().is_some_and(|status| {
                status
                    .entries
                    .iter()
                    .any(|entry| entry.path == std::path::Path::new(path))
            })
        })
    };
    for (index, cancel) in ["Index only chosen", "Stashes closed"]
        .into_iter()
        .enumerate()
    {
        open_selected_stash(&app, cx).await;
        let stashes = browser(cx);
        // Review restore…, held as a slow disk holds it, and Refresh.
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(PREPARING),
            "{cancel}: the restoration is prepared"
        );
        let later = format!("later-{index}.txt");
        std::fs::write(repository.join(&later), "later\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        assert!(
            app.read_with(cx, |app, _| app.working_read_deferred.is_some()),
            "{cancel}: Refresh waits for the preparation"
        );
        if index == 0 {
            let row = cx
                .debug_bounds("saved-stash-row-1")
                .expect("Index only's row");
            cx.simulate_click(row.center(), Modifiers::none());
            StashBrowser::lands(&stashes, cx);
            drop(stashes);
        } else {
            // The dialog holds the browser's only handle, whose release
            // cancels.
            drop(stashes);
            cx.simulate_keystrokes("escape");
            crate::views::tests::draw(cx);
        }
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_busy, None, "{cancel}: cancelled");
            assert_eq!(
                app.working_read_deferred, None,
                "{cancel}: the read Refresh asked for is taken up"
            );
            assert!(app.status_task.is_some(), "{cancel}: and runs");
        });
        drop(held);
        recovery_lands(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        assert!(
            listed(cx, &later),
            "{cancel}: Refresh read the working state"
        );
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
    }
    drop(fixture);
}

/// Which of a dialog's footer Buttons, by their selectors, holds focus in
/// the last frame drawn: the one the ring painted tight around, the kit
/// Button's ring drawn only on a focused Button, within the ring's room.
fn focused_footer_button(
    cx: &mut gpui::VisualTestContext,
    buttons: &[&'static str],
) -> Option<&'static str> {
    let room = px(6.);
    let ringed: Vec<_> = buttons
        .iter()
        .copied()
        .filter(|selector| {
            let button = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            ring_around(cx, button).is_some_and(|ring| {
                button.left() - ring.bounds.left() <= room
                    && ring.bounds.right() - button.right() <= room
                    && button.top() - ring.bounds.top() <= room
                    && ring.bounds.bottom() - button.bottom() <= room
            })
        })
        .collect();
    assert!(ringed.len() <= 1, "one focused Button: {ringed:?}");
    ringed.first().copied()
}

/// The keyboard ring the last frame painted around `control`: the
/// tightest outline in the accent, at any opacity, that encloses it.
fn ring_around(
    cx: &mut gpui::VisualTestContext,
    control: Bounds<Pixels>,
) -> Option<crate::focus_ring::PaintedOutline> {
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    crate::focus_ring::painted_outlines_where(cx, move |color| {
        color.a > 0.
            && Hsla {
                a: accent.a,
                ..color
            } == accent
    })
    .into_iter()
    .filter(|ring| {
        ring.bounds.left() <= control.left()
            && ring.bounds.top() <= control.top()
            && ring.bounds.right() >= control.right()
            && ring.bounds.bottom() >= control.bottom()
    })
    .min_by(|a, b| {
        let area = |ring: &crate::focus_ring::PaintedOutline| {
            f32::from(ring.bounds.size.width) * f32::from(ring.bounds.size.height)
        };
        area(a).total_cmp(&area(b))
    })
}

/// Review restore… in Stashes reads the pause in the core rather than
/// refuse from the one the window read last. A merge aborted in a
/// terminal, unseen, no longer refuses it: Stashes gives way to the
/// Restore stash review, as before the window refused from its cache,
/// and the window reads its working state again. A merge that is paused
/// refuses it in place, in 461 × 490 and 1480 × 980 windows at 13 and
/// 18 pt: Stashes stays open with its stash selected, the footer note
/// beside Review restore… says why, whole in the detail's view and
/// beneath the control, a status that the window's alert says at once
/// on each refusal, and nowhere else, and once a read finds the merge
/// ended the note says so, which the window's announcement says once
/// speech pauses. Refused from the
/// window's cache, Stashes closed and the window's error that said why
/// went with the quiet read that found the merge ended, within 0.3 s
/// natively, so nothing showed and the selection was lost. Pressed from
/// the keyboard, Review restore… keeps focus and its ring whole as the
/// refusal lands, so Space presses it again; at 18 pt in the 461 × 490
/// window, where the detail is about 60 px tall, the refusal keeps one
/// line beneath it. The detail scrolled to show the whole two-line
/// refusal there and cut Review restore…'s ring. The refusal's node is
/// static text that reports the part of it the window draws, cut at the
/// detail's foot once the detail scrolls it half past there, where it
/// reported its whole layout.
#[gpui::test]
async fn review_restore_reads_the_pause_and_answers_in_stashes(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    // The app's palette, whose accent draws the keyboard ring.
    crate::focus_ring::sweep::prepare(&app, cx);
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    let browser = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
    };
    let slack = px(0.5);
    let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    };

    // Aborted unseen: the core accepts and the review opens.
    cx.simulate_resize(size(px(1480.), px(980.)));
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    read_pause(&app, cx).await;
    open_selected_stash(&app, cx).await;
    git(&["merge", "--abort"]);
    let stashes = browser(cx).expect("Stashes is open");
    press_review_restore(&stashes, cx);
    drop(stashes);
    recovery_lands(&app, cx);
    assert!(
        cx.debug_bounds("operation-consequences").is_some()
            && cx.debug_bounds("stash-browser").is_none(),
        "the Restore stash review opens in place of Stashes"
    );
    assert_eq!(app.read_with(cx, |app, _| app.operation_error_text()), None);
    quiet_reads_land(&app, cx);
    assert!(
        app.read_with(cx, |app, _| app.integration_state.is_none()),
        "the window reads the merge ended"
    );
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);

    // Paused: refused in place, and the pause's end said there.
    let mut last_alert = None;
    for (width, height) in [(461., 490.), (1480., 980.)] {
        for interface in [13, 18] {
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            // Unread: the core's refusal is the first the window hears
            // of the pause.
            git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
            open_selected_stash(&app, cx).await;
            let stashes = browser(cx).expect("Stashes is open");
            press_review_restore(&stashes, cx);
            recovery_lands(&app, cx);
            assert!(
                cx.update(|window, cx| window.has_active_dialog(cx))
                    && cx.debug_bounds("operation-consequences").is_none(),
                "{at}: Stashes stays open"
            );
            assert_eq!(
                stashes.read_with(cx, |stashes, _| (
                    stashes.selected,
                    stashes.snapshot.is_some(),
                    stashes.restore_line.clone()
                )),
                (Some(0), true, Some(PAUSED_REFUSAL.to_owned())),
                "{at}: the stash stays selected and refused"
            );
            let [line, restore, detail, browser_bounds] = [
                "stash-restore-refusal",
                "restore-saved-stash",
                "stash-detail-column",
                "stash-browser",
            ]
            .map(|name| {
                cx.debug_bounds(name)
                    .unwrap_or_else(|| panic!("{at}: no {name}"))
            });
            assert!(
                inside(line, detail) && inside(line, browser_bounds),
                "{at}: the refusal {line:?} lies whole in the detail {detail:?}"
            );
            assert!(
                line.top() >= restore.bottom() && line.top() - restore.bottom() < px(24.),
                "{at}: the refusal {line:?} lies beneath Review restore… {restore:?}"
            );
            // Review restore… keeps focus, whole with its ring.
            assert_eq!(
                cx.update(|window, cx| stashes.read(cx).detail.focused(window)),
                Some("restore-saved-stash".into()),
                "{at}: Review restore… keeps focus"
            );
            let ring = ring_around(cx, restore)
                .unwrap_or_else(|| panic!("{at}: Review restore… {restore:?} draws no ring"));
            assert!(ring.whole, "{at}: Review restore…'s ring {ring:?} is whole");
            let one = cx.update(|window, _| {
                crate::image_compare::text_line(appearance::ui_text(10.), window)
            });
            let lines = (f32::from(line.size.height) / f32::from(one)).round() as usize;
            if width < 800. && interface == 18 {
                assert_eq!(lines, 1, "{at}: the refusal {line:?} keeps one line");
            }
            let (alert, said) = crate::copy_feedback::tests::alerted(cx)
                .unwrap_or_else(|| panic!("{at}: the window's alert says the refusal"));
            assert!(
                said == PAUSED_REFUSAL && last_alert.as_ref() != Some(&alert),
                "{at}: the window's alert says the refusal at once, not {said:?}"
            );
            assert_eq!(
                crate::native_accessibility::written::announced_from("stash-restore-"),
                Vec::new(),
                "{at}: the note is no live region of its own"
            );
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_error_text()),
                None,
                "{at}: and nowhere else"
            );
            // Its node is static text that reports the part of it the
            // detail draws: all of it, revealed, and only what the
            // detail's foot leaves of it once the detail scrolls it half
            // past there, at 18 pt in the 461 × 490 window. It reported
            // its whole layout wherever the detail scrolled it.
            let scale = f64::from(cx.update(|window, _| window.scale_factor()));
            let physical = |edge: Pixels| f64::from(f32::from(edge)) * scale;
            let node = || {
                let written = crate::native_accessibility::written::get("stash-restore-refusal")
                    .unwrap_or_else(|| panic!("{at}: the refusal writes its node as static text"));
                assert_eq!(written.node.role(), Role::Status, "{at}");
                assert_eq!(written.node.label(), Some(PAUSED_REFUSAL), "{at}");
                written.node.bounds().expect("bounds")
            };
            let whole = node();
            assert!(
                (whole.y0 - physical(line.top())).abs() < 0.5
                    && (whole.y1 - physical(line.bottom())).abs() < 0.5,
                "{at}: the whole refusal {line:?} reports {whole:?}"
            );
            if width < 800. && interface == 18 {
                let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
                let kept = scroll.offset();
                let foot = cx
                    .debug_bounds("stash-detail")
                    .expect("the detail")
                    .bottom();
                let middle = line.top() + line.size.height / 2.;
                scroll.set_offset(point(kept.x, kept.y + (foot - middle)));
                crate::views::tests::draw(cx);
                let cut = cx
                    .debug_bounds("stash-restore-refusal")
                    .expect("the refusal");
                let foot = cx
                    .debug_bounds("stash-detail")
                    .expect("the detail")
                    .bottom();
                assert!(
                    cut.top() < foot && cut.bottom() > foot,
                    "{at}: the detail's foot {foot:?} cuts the refusal {cut:?}"
                );
                let drawn = node();
                assert!(
                    (drawn.y0 - physical(cut.top())).abs() < 0.5
                        && (drawn.y1 - physical(foot)).abs() < 0.5,
                    "{at}: the refusal {cut:?}, cut at {foot:?}, reports {drawn:?}"
                );
                scroll.set_offset(kept);
                crate::views::tests::draw(cx);
            }
            quiet_reads_land(&app, cx);
            assert!(
                app.read_with(cx, |app, _| app.integration_state.is_some()),
                "{at}: the refusal has the window read the pause"
            );
            // Pressed again where it is, the window's alert says it once
            // more, a fresh node in the same words.
            space(cx);
            recovery_lands(&app, cx);
            let again = crate::copy_feedback::tests::alerted(cx);
            assert!(
                again
                    .as_ref()
                    .is_some_and(|(id, said)| *id != alert && said == PAUSED_REFUSAL),
                "{at}: each refusal is said again, not {again:?} after {alert:?}"
            );
            last_alert = again.map(|(id, _)| id);
            // The merge ends; the next read says so in the note.
            git(&["merge", "--abort"]);
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            crate::views::tests::settle(&app, cx).await;
            quiet_reads_land(&app, cx);
            crate::views::tests::draw(cx);
            assert!(
                cx.debug_bounds("stash-restore-refusal").is_none(),
                "{at}: the refusal goes"
            );
            let ended = cx
                .debug_bounds("stash-restore-ended")
                .unwrap_or_else(|| panic!("{at}: the note says the merge ended"));
            let detail = cx.debug_bounds("stash-detail-column").unwrap();
            assert!(inside(ended, detail), "{at}: {ended:?} in {detail:?}");
            let restore = cx.debug_bounds("restore-saved-stash").unwrap();
            assert!(
                ring_around(cx, restore).is_some_and(|ring| ring.whole),
                "{at}: Review restore… {restore:?} keeps its ring whole"
            );
            assert_eq!(
                crate::copy_feedback::tests::announced(cx)
                    .map(|(_, said)| said)
                    .as_deref(),
                Some(PAUSE_ENDED),
                "{at}: the window's announcement says so, heard once speech pauses"
            );
            assert_eq!(
                crate::native_accessibility::written::announced_from("stash-restore-"),
                Vec::new(),
                "{at}: the note is no live region of its own"
            );
            drop(stashes);
            cx.update(|window, cx| window.close_all_dialogs(cx));
            crate::views::tests::draw(cx);
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// When Git itself fails to prepare Review restore…'s restoration, as a
/// status read of a damaged index does, Stashes stays open and its note
/// gives the first line of Git's report, in 1480 × 980 and 461 × 490
/// windows at 13 and 18 pt, whether the report runs past that line, as
/// a corrupt index's does, or is that one line, as a truncated index's
/// is: one or two lines, one where the stacked detail is about 60 px
/// tall (18 pt in 461 × 490), whole in what the window shows of the
/// detail, its scrolling view stacked and its column beside the list,
/// and not in the window's error. Review restore…, which the press left
/// focused, keeps focus, whole in that view with its ring. Details…
/// beside the note, whole there with its ring as Tab reaches it, opens
/// the whole report with Copy details above Stashes, and Escape closes
/// it back to Stashes with focus on Details… again, whole with its ring.
/// A report that opens with an empty line gives its first line that
/// says anything, and one that says nothing gives that Git could not
/// prepare the restoration, with no Details…. The stash shown, chosen
/// again after a terminal saved another stash, is not read again, and
/// Review restore…'s own plan finds the list changed. The note took
/// Git's whole report, its stderr line after line, unclamped beside the
/// list and with no Details… to read or copy it; a report of one line
/// had no Details…, so stacked at 18 pt its rest lay only in a tooltip
/// the keyboard never opens; a report that opened with an empty line, or
/// said nothing, showed an empty note; and a first line longer than an
/// accessible text carries reached the window's alert and the note's
/// name whole.
#[gpui::test]
async fn a_failed_restore_preparation_says_its_first_line_with_details(
    cx: &mut gpui::TestAppContext,
) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    // The app's palette, whose accent draws the keyboard ring.
    crate::focus_ring::sweep::prepare(&app, cx);
    let index = repository.join(".git").join("index");
    let kept = std::fs::read(&index).expect("the index");
    let slack = px(0.5);
    let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    };
    let note_label = || {
        crate::native_accessibility::written::get("stash-restore-refusal")
            .and_then(|note| note.node.label().map(str::to_owned))
    };
    // Git cannot read the status of a corrupt index, and says so on two
    // lines, or of a truncated one, and says so on one.
    let damaged: [(&str, &[u8], bool); 2] = [
        ("a corrupt index", &[b'x'; 128], true),
        ("a truncated index", b"DIRC", false),
    ];
    for (width, height) in [(1480., 980.), (461., 490.)] {
        for interface in [13, 18] {
            for (damage, bytes, more) in damaged {
                let at = format!("{damage} at {interface} pt in {width} × {height}");
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                open_selected_stash(&app, cx).await;
                let stashes = app
                    .read_with(cx, |app, _| app.recovery.browser.clone())
                    .and_then(|browser| browser.upgrade())
                    .expect("Stashes is open");
                std::fs::write(&index, bytes).unwrap();
                press_review_restore(&stashes, cx);
                recovery_lands(&app, cx);
                std::fs::write(&index, &kept).unwrap();
                let report = stashes
                    .read_with(cx, |stashes, _| stashes.restore_line.clone())
                    .unwrap_or_else(|| panic!("{at}: Review restore… is refused"));
                assert_eq!(
                    report.trim_end().lines().count() > 1,
                    more,
                    "{at}: Git's report: {report}"
                );
                assert_eq!(
                    note_label().as_deref(),
                    report.lines().next(),
                    "{at}: the note gives the report's first line"
                );
                assert_eq!(
                    crate::copy_feedback::tests::alerted(cx)
                        .map(|(_, said)| said)
                        .as_deref(),
                    report.lines().next(),
                    "{at}: the window's alert says that line, not the whole report"
                );
                assert_eq!(
                    app.read_with(cx, |app, _| app.operation_error_text()),
                    None,
                    "{at}: Stashes says it, not the window"
                );
                // What the window shows of the detail: stacked, the
                // detail's scrolling view, which the footer scrolls in;
                // beside the list, its column, whose footer lies below
                // the view.
                let stacked = width < 800.;
                let shown = if stacked {
                    "stash-detail"
                } else {
                    "stash-detail-column"
                };
                let [note, restore, view] = ["stash-restore-refusal", "restore-saved-stash", shown]
                    .map(|name| {
                        cx.debug_bounds(name)
                            .unwrap_or_else(|| panic!("{at}: no {name}"))
                    });
                let one = cx.update(|window, _| {
                    crate::image_compare::text_line(appearance::ui_text(10.), window)
                });
                let lines = (f32::from(note.size.height) / f32::from(one)).round() as usize;
                // One line where the stacked detail is about 60 px tall,
                // and two at most elsewhere.
                let keeps = if stacked && interface == 18 { 1 } else { 2 };
                assert!(
                    (1..=keeps).contains(&lines) && inside(note, view),
                    "{at}: the note {note:?} keeps {lines} lines, at most {keeps}, whole in {view:?}"
                );
                // Review restore…, which the press left focused, keeps
                // focus, whole in view with its ring, as the failure
                // lands: its row is taller than the paused refusal's,
                // with Details… beside the note.
                assert_eq!(
                    cx.update(|window, cx| stashes.read(cx).detail.focused(window)),
                    Some("restore-saved-stash".into()),
                    "{at}: Review restore… keeps focus"
                );
                let ring = ring_around(cx, restore)
                    .unwrap_or_else(|| panic!("{at}: Review restore… {restore:?} draws no ring"));
                assert!(
                    ring.whole && inside(restore, view),
                    "{at}: Review restore… {restore:?} and its ring {ring:?} are whole in {view:?}"
                );
                // Details… opens the whole report above Stashes, and
                // Escape closes it back to Stashes, focus on Details…
                // again, whole in view with its ring.
                let details_whole = |cx: &mut gpui::VisualTestContext, when: &str| {
                    assert_eq!(
                        cx.update(|window, cx| stashes.read(cx).detail.focused(window)),
                        Some("stash-restore-details".into()),
                        "{at}: Details… holds focus {when}"
                    );
                    let [details, view] = ["stash-restore-details", shown].map(|name| {
                        cx.debug_bounds(name)
                            .unwrap_or_else(|| panic!("{at}: no {name} {when}"))
                    });
                    let ring = ring_around(cx, details)
                        .unwrap_or_else(|| panic!("{at}: Details… draws no ring {when}"));
                    assert!(
                        ring.whole && inside(details, view),
                        "{at}: Details… {details:?} and its ring {ring:?} are whole in {view:?} {when}"
                    );
                };
                tab_to(&stashes, "stash-restore-details", cx);
                details_whole(cx, "as Tab reaches it");
                space(cx);
                crate::views::tests::draw(cx);
                assert!(
                    cx.debug_bounds("copy-operation-details").is_some(),
                    "{at}: Details… opens the report, with Copy details"
                );
                cx.simulate_keystrokes("escape");
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(300));
                crate::views::tests::draw(cx);
                assert!(
                    cx.debug_bounds("copy-operation-details").is_none()
                        && cx.update(|window, cx| window.has_active_dialog(cx))
                        && cx.debug_bounds("stash-browser").is_some(),
                    "{at}: Escape closes the report back to Stashes"
                );
                details_whole(cx, "once the report closes");
                drop(stashes);
                cx.update(|window, cx| window.close_all_dialogs(cx));
                crate::views::tests::draw(cx);
            }
        }
    }
    // A report that opens with an empty line gives the first line that
    // says anything, with Details… beside it.
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    let refuse = |message: &str, cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            stashes.update(cx, |stashes, cx| {
                stashes.refuse(message.into(), None, window, cx)
            })
        });
        crate::views::tests::draw(cx);
        crate::copy_feedback::tests::alerted(cx).map(|(_, said)| said)
    };
    let said = refuse(
        "\n  \nUnable to read working-copy status: fatal: index file corrupt\n",
        cx,
    );
    let first = "Unable to read working-copy status: fatal: index file corrupt";
    assert_eq!(note_label().as_deref(), Some(first));
    assert_eq!(said.as_deref(), Some(first), "the window's alert says it");
    assert!(cx.debug_bounds("stash-restore-details").is_some());
    // A report that says nothing still says that Git could not prepare
    // the restoration, with no Details… to open nothing.
    let said = refuse("\n  \n", cx);
    assert_eq!(
        note_label().as_deref(),
        Some(RESTORE_UNEXPLAINED),
        "a blank report gives the fallback line"
    );
    assert_eq!(
        said.as_deref(),
        Some(RESTORE_UNEXPLAINED),
        "the window's alert says it"
    );
    assert!(cx.debug_bounds("stash-restore-details").is_none());
    // A first line past what an accessible text carries, as one line of
    // Git's stderr can be, is said and named by at most 16 KiB of it,
    // ending in an ellipsis, alike: whole, the alert and the note carried
    // what core keeps of the stream, up to 4 MiB.
    let long = "Unable to read working-copy status: ".to_owned() + &"x".repeat(64 * 1024);
    let said = refuse(&format!("{long}\nmore\n"), cx).expect("the window's alert says it");
    assert!(
        said.len() <= 16 * 1024
            && said.ends_with('…')
            && long.starts_with(said.trim_end_matches('…')),
        "the window's alert says {} bytes of the {}-byte line",
        said.len(),
        long.len()
    );
    assert!(
        note_label().is_some_and(|named| named == said),
        "the note is named by the line the alert says"
    );
    assert!(cx.debug_bounds("stash-restore-details").is_some());
    drop(stashes);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    // Chosen again after a terminal saved another stash, the stash shown
    // is not read again: Review restore…'s own plan finds the list
    // changed, and the note says so.
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    std::fs::write(repository.join("notes.txt"), "third\n").unwrap();
    git(&["stash", "push", "-q", "-m", "Saved in a terminal"]);
    let row = cx
        .debug_bounds("saved-stash-row-0")
        .expect("Layout experiment's row");
    cx.simulate_click(row.center(), Modifiers::none());
    crate::views::tests::draw(cx);
    assert!(
        stashes.read_with(cx, |stashes, _| stashes.metadata_task.is_none()),
        "chosen again, the stash shown is not read again"
    );
    press_review_restore(&stashes, cx);
    recovery_lands(&app, cx);
    assert_eq!(
        note_label().as_deref(),
        Some(
            "The stash list changed. Refresh and review the selected stash again; no stash was removed."
        ),
        "Review restore…'s plan finds the list changed"
    );
    drop(stashes);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// Stacked at 18 pt in a 461 × 490 window, where the detail is about
/// 60 px tall, the note that a merge has ended scrolls the detail only
/// while nothing else in it holds focus. Review restore… refused while
/// the merge pauses, focus moves to the saved files, as a click on one
/// moves it, and the merge is aborted in a terminal: once a read finds
/// it ended, the note says so, and the detail stays where it is, with
/// the saved files focused and whole in its view; the note below its
/// fold, out of AccessKit's reach, is said once by the window's
/// announcement, through AccessKit's filter and, on Linux, the AT-SPI
/// adapter, and scrolled to, it is a status in reach that no one hears
/// again. A polite note of its own there was left out by the filter
/// and heard by no one. Refused again with
/// focus on the saved-work list instead, outside the detail, and the
/// detail scrolled to its top: the end reveals Review restore… and the
/// note whole beneath it. The detail jumped to Review restore… and the note
/// on any read that found the merge ended, scrolling the focused saved
/// files out of view.
#[gpui::test]
async fn the_end_of_a_pause_leaves_focus_in_view_in_stashes(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    let slack = px(0.5);
    let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    };
    let ended = |stashes: &Entity<StashBrowser>, cx: &mut gpui::VisualTestContext| {
        stashes.read_with(cx, |stashes, _| {
            stashes.restore_line.is_none() && stashes.pause_ended.is_some()
        }) && cx.debug_bounds("stash-restore-ended").is_some()
    };

    // Refused while the merge pauses, unread, which the refusal has the
    // window read.
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    press_review_restore(&stashes, cx);
    recovery_lands(&app, cx);
    quiet_reads_land(&app, cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
        Some(PAUSED_REFUSAL.to_owned())
    );
    assert!(app.read_with(cx, |app, _| app.integration_state.is_some()));
    // Focus moves to the saved files, as a click on one moves it, and
    // they scroll into the detail's view.
    let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
    cx.update(|window, cx| window.focus(&files, cx));
    crate::views::tests::draw(cx);
    let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
    let kept = scroll.offset();
    let in_view = |cx: &mut gpui::VisualTestContext| {
        let [files, view] =
            ["stash-files", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
        // Whole in view, no taller than the detail shows.
        (inside(files, view), files, view)
    };
    let (shown, files_at, view) = in_view(cx);
    assert!(
        shown,
        "the saved files {files_at:?} are whole in view {view:?} at {kept:?}"
    );
    let mut heard = Heard::start(cx);
    // The merge ends in a terminal; the next read says so in the note.
    git(&["merge", "--abort"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    quiet_reads_land(&app, cx);
    crate::views::tests::draw(cx);
    assert!(ended(&stashes, cx), "the note says the merge ended");
    assert_eq!(
        scroll.offset(),
        kept,
        "the detail stays where the keyboard left it"
    );
    assert!(
        cx.update(|window, _| files.is_focused(window)) && in_view(cx).0,
        "the saved files keep focus, whole in view"
    );
    // Below the fold, the note is out of reach; the window's
    // announcement says it, once, as the read lands.
    let found = reached(cx);
    assert!(
        !found
            .iter()
            .any(|node| node.clip.is_some() && node.name.as_deref() == Some(PAUSE_ENDED)),
        "the note below the fold is out of reach: {found:?}"
    );
    assert_eq!(
        said_from_the_window(&found, PAUSE_ENDED, gpui::accesskit::Live::Polite),
        1,
        "the window's announcement says the merge ended, in reach: {found:?}"
    );
    assert_eq!(
        crate::native_accessibility::written::announced_from("stash-restore-"),
        Vec::new(),
        "the note is no live region of its own"
    );
    if let Some(heard) = heard.next(cx) {
        assert_eq!(
            heard.iter().filter(|said| *said == PAUSE_ENDED).count(),
            1,
            "AT-SPI announces the end once as the read lands: {heard:?}"
        );
    }
    // Scrolled to the note, it is in reach, read where it lies, and
    // nothing says it again.
    scroll.set_offset(point(kept.x, -scroll.max_offset().y));
    let found = reached(cx);
    assert!(
        found.iter().any(|node| node.role == Role::Status
            && node.clip.is_some()
            && node.live == gpui::accesskit::Live::Off
            && node.name.as_deref() == Some(PAUSE_ENDED)),
        "the note scrolled into view is a status in reach: {found:?}"
    );
    if let Some(heard) = heard.next(cx) {
        assert!(
            !heard.iter().any(|said| said == PAUSE_ENDED),
            "AT-SPI says the end once, not again on scroll: {heard:?}"
        );
    }

    // Refused again, the keyboard on the saved-work list, outside the
    // detail, and the detail at its top.
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    read_pause(&app, cx).await;
    press_review_restore(&stashes, cx);
    recovery_lands(&app, cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
        Some(PAUSED_REFUSAL.to_owned())
    );
    let list = stashes.read_with(cx, |stashes, _| stashes.stash_focus.clone());
    cx.update(|window, cx| window.focus(&list, cx));
    scroll.set_offset(Point::default());
    crate::views::tests::draw(cx);
    let restore = cx
        .debug_bounds("restore-saved-stash")
        .expect("Review restore…");
    let view = cx.debug_bounds("stash-detail").expect("the detail");
    assert!(
        !inside(restore, view),
        "Review restore… {restore:?} lies below the detail's view {view:?}"
    );
    git(&["merge", "--abort"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    quiet_reads_land(&app, cx);
    crate::views::tests::draw(cx);
    assert!(ended(&stashes, cx), "the note says the merge ended");
    let [note, restore, view] = ["stash-restore-ended", "restore-saved-stash", "stash-detail"]
        .map(|name| cx.debug_bounds(name).expect(name));
    assert!(
        inside(restore, view) && inside(note, view),
        "Review restore… {restore:?} and the note {note:?} are revealed in {view:?}"
    );
    assert!(
        cx.update(|window, _| list.is_focused(window)),
        "the saved-work list keeps focus"
    );
    drop(stashes);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// Stacked at 18 pt in a 461 × 490 window, where the detail is about
/// 60 px tall, a click on Review restore… shows its answer wherever
/// focus lies. Tab leaves focus on the saved files, the detail is
/// scrolled to show Review restore… at its foot with the note below its
/// fold, and a click on Review restore… while a merge pauses, which
/// focuses no Button and so leaves focus on the saved files, is
/// refused: the detail scrolls to Review restore… and the refusal
/// beneath it, whole in its view. The note's reveal scrolled only while
/// Review restore… or nothing in the detail held focus, as only the end
/// of a pause should, so the click's refusal lay below the fold with
/// nothing in view saying anything.
#[gpui::test]
async fn a_clicked_review_restore_shows_its_answer_wherever_focus_lies(
    cx: &mut gpui::TestAppContext,
) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    let slack = px(0.5);
    let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    };
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");

    // Tab leaves focus on the saved files.
    let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
    let on_files =
        |cx: &mut gpui::VisualTestContext| cx.update(|window, _| files.is_focused(window));
    for _ in 0..12 {
        cx.simulate_keystrokes("tab");
        crate::views::tests::draw(cx);
        if on_files(cx) {
            break;
        }
    }
    assert!(on_files(cx), "Tab reaches the saved files");
    // The detail scrolled to show Review restore… at its foot, with the
    // note below its fold.
    let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
    let room = cx.read(appearance::button_ring_room);
    let [restore, view] =
        ["restore-saved-stash", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
    let offset = scroll.offset();
    scroll.set_offset(point(
        offset.x,
        (offset.y + view.bottom() - room - restore.bottom()).clamp(-scroll.max_offset().y, px(0.)),
    ));
    crate::views::tests::draw(cx);
    let [restore, note, view] = ["restore-saved-stash", "stash-restore-note", "stash-detail"]
        .map(|name| cx.debug_bounds(name).expect(name));
    assert!(
        inside(restore, view) && note.top() >= view.bottom() - slack,
        "Review restore… {restore:?} lies in the detail's view {view:?}, the note {note:?} below it"
    );
    let kept = scroll.offset();

    // A click on Review restore… leaves focus on the saved files.
    cx.simulate_click(restore.center(), Modifiers::none());
    assert!(on_files(cx), "the click leaves focus on the saved files");
    recovery_lands(&app, cx);
    quiet_reads_land(&app, cx);
    crate::views::tests::draw(cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
        Some(PAUSED_REFUSAL.to_owned()),
        "the click is refused"
    );
    let [note, restore, view] = [
        "stash-restore-refusal",
        "restore-saved-stash",
        "stash-detail",
    ]
    .map(|name| cx.debug_bounds(name).expect(name));
    assert!(
        inside(note, view) && inside(restore, view),
        "Review restore… {restore:?} and the refusal {note:?} are revealed in {view:?}, \
         scrolled from {kept:?} to {:?}",
        scroll.offset()
    );
    assert!(on_files(cx), "the saved files keep focus");
    git(&["merge", "--abort"]);
    drop(stashes);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// Stacked at 18 pt in a 461 × 490 window, where the detail is about
/// 60 px tall, the answer to a press of Review restore… leaves the
/// control the keyboard reached while Git prepared it in view. With a
/// merge paused and the window's operations held, so Git has not
/// answered, a click on Review restore…, focus on the saved-work list,
/// then Tab onto the saved files, which the detail reveals whole: once
/// the refusal lands, the detail stays where it is, the saved files
/// focused and whole in its view, and the note below its fold, out of
/// AccessKit's reach, is said once by the window's alert, through
/// AccessKit's filter and, on Linux, the AT-SPI adapter; scrolled to
/// the note, the detail brings it into reach, a status no one hears
/// again. Pressed with Space instead, then Shift+Tab back onto the
/// saved files: the same. The press's answer scrolled the detail to
/// Review restore… and the note wherever focus lay, taking the saved
/// files out of view; then, left where it was, the note was an alert
/// that AccessKit's filter left out below the fold, heard by no one.
#[gpui::test]
async fn a_late_restore_answer_leaves_the_control_tabbed_to_in_view(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    let slack = px(0.5);
    let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    };
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
    let list = stashes.read_with(cx, |stashes, _| stashes.stash_focus.clone());
    let on_files =
        |cx: &mut gpui::VisualTestContext| cx.update(|window, _| files.is_focused(window));
    let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
    let files_in_view = |cx: &mut gpui::VisualTestContext| {
        let [files, view] =
            ["stash-files", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
        (inside(files, view), files, view)
    };
    // Moves focus onto the saved files with `key`, which the detail
    // reveals whole.
    let move_to_files = |key: &str, cx: &mut gpui::VisualTestContext| {
        for _ in 0..12 {
            cx.simulate_keystrokes(key);
            crate::views::tests::draw(cx);
            if on_files(cx) {
                break;
            }
        }
        assert!(on_files(cx), "{key} reaches the saved files");
        let (shown, files_at, view) = files_in_view(cx);
        assert!(
            shown,
            "the saved files {files_at:?} are revealed whole in {view:?}"
        );
    };

    for press in ["click", "space"] {
        let held = if press == "click" {
            // Focus on the saved-work list, outside the detail, and
            // Review restore… scrolled into the detail's view.
            assert!(
                cx.update(|window, _| list.is_focused(window)),
                "the saved-work list holds focus"
            );
            let [restore, view] = ["restore-saved-stash", "stash-detail"]
                .map(|name| cx.debug_bounds(name).expect(name));
            let room = cx.read(appearance::button_ring_room);
            let offset = scroll.offset();
            scroll.set_offset(point(
                offset.x,
                (offset.y + view.bottom() - room - restore.bottom())
                    .clamp(-scroll.max_offset().y, px(0.)),
            ));
            crate::views::tests::draw(cx);
            let restore = cx
                .debug_bounds("restore-saved-stash")
                .expect("Review restore…");
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.simulate_click(restore.center(), Modifiers::none());
            held
        } else {
            tab_to(&stashes, "restore-saved-stash", cx);
            let held = app.update(cx, |app, _| app.operations.hold());
            space(cx);
            held
        };
        crate::views::tests::draw(cx);
        assert!(
            stashes.read_with(cx, |stashes, cx| stashes.preparing(cx)),
            "{press}: Git prepares the restoration"
        );
        // The keyboard moves on to the saved files while Git prepares.
        move_to_files(if press == "click" { "tab" } else { "shift-tab" }, cx);
        let kept = scroll.offset();
        let mut heard = Heard::start(cx);

        drop(held);
        recovery_lands(&app, cx);
        quiet_reads_land(&app, cx);
        crate::views::tests::draw(cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
            Some(PAUSED_REFUSAL.to_owned()),
            "{press}: the press is refused"
        );
        let (shown, files_at, view) = files_in_view(cx);
        assert!(
            on_files(cx) && shown && scroll.offset() == kept,
            "{press}: the saved files {files_at:?} keep focus, whole in {view:?}, \
             the detail kept at {kept:?}, not {:?}",
            scroll.offset()
        );
        // Below the fold, the note is out of reach; the window's alert
        // says it, once, as it lands.
        let found = reached(cx);
        assert!(
            !found
                .iter()
                .any(|node| node.clip.is_some() && node.name.as_deref() == Some(PAUSED_REFUSAL)),
            "{press}: the note below the fold is out of reach: {found:?}"
        );
        assert_eq!(
            said_from_the_window(&found, PAUSED_REFUSAL, gpui::accesskit::Live::Assertive),
            1,
            "{press}: the window's alert says the refusal, in reach: {found:?}"
        );
        assert_eq!(
            crate::native_accessibility::written::announced_from("stash-restore-"),
            Vec::new(),
            "{press}: the note is no live region of its own"
        );
        if let Some(heard) = heard.next(cx) {
            assert_eq!(
                heard.iter().filter(|said| *said == PAUSED_REFUSAL).count(),
                1,
                "{press}: AT-SPI announces the refusal once as it lands: {heard:?}"
            );
        }
        // Scrolled to the note, it is in reach, read where it lies, and
        // nothing says it again.
        scroll.set_offset(point(kept.x, -scroll.max_offset().y));
        let found = reached(cx);
        assert!(
            found.iter().any(|node| node.role == Role::Status
                && node.clip.is_some()
                && node.live == gpui::accesskit::Live::Off
                && node.name.as_deref() == Some(PAUSED_REFUSAL)),
            "{press}: the note scrolled into view is a status in reach: {found:?}"
        );
        if let Some(heard) = heard.next(cx) {
            assert!(
                !heard.iter().any(|said| said == PAUSED_REFUSAL),
                "{press}: AT-SPI says the refusal once, not again on scroll: {heard:?}"
            );
        }
        scroll.set_offset(kept);
        crate::views::tests::draw(cx);
    }
    git(&["merge", "--abort"]);
    drop(stashes);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// The reveal of a press's answer expires with the frame that draws it.
/// At 18 pt in a 1480 × 980 window, Stashes beside the list, the saved
/// files focused, a click on Review restore… while a merge pauses is
/// refused in the footer, always in view. The window narrowed to 461 ×
/// 490 afterwards, the detail stacked beneath the list is not scrolled
/// to the refusal; Tab and Shift+Tab back onto the saved files reveal
/// them whole, and once a read finds the merge ended the note says so
/// with the saved files still focused and whole in view.
///
/// A refactor guard, not a regression test: the code before Stashes
/// took the reveal as it renders passed it too, since a frame beside
/// the list cleared the reveal and every stacked frame's footer spent
/// it, so no frame it could reach left one due. It holds that take
/// (`NoteReveal::due_in`) to its promise for every frame that draws
/// Stashes: a press's reveal kept past the frame beside the list would
/// scroll the first stacked frame to Review restore… and the note,
/// whatever focus had done since, and win over any end of the pause
/// before that frame.
#[gpui::test]
async fn a_press_answer_beside_the_list_expires_with_its_frame(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(1480.), px(980.)));
    let slack = px(0.5);
    let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    };
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
    let on_files =
        |cx: &mut gpui::VisualTestContext| cx.update(|window, _| files.is_focused(window));
    // Focus on the saved files, as a click on one moves it.
    cx.update(|window, cx| window.focus(&files, cx));
    crate::views::tests::draw(cx);

    // Beside the list, a click on Review restore… in the footer, which
    // leaves focus on the saved files, is refused there.
    let restore = cx
        .debug_bounds("restore-saved-stash")
        .expect("Review restore…");
    cx.simulate_click(restore.center(), Modifiers::none());
    recovery_lands(&app, cx);
    quiet_reads_land(&app, cx);
    crate::views::tests::draw(cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
        Some(PAUSED_REFUSAL.to_owned()),
        "the click is refused"
    );
    assert!(
        cx.debug_bounds("stash-restore-refusal").is_some() && on_files(cx),
        "the refusal is in the footer, the saved files focused"
    );

    // Narrowed, the detail stacks beneath the list and is not scrolled
    // to the refusal answered beside it.
    cx.simulate_resize(size(px(461.), px(490.)));
    for _ in 0..3 {
        crate::views::tests::draw(cx);
    }
    let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
    let [note, view] =
        ["stash-restore-refusal", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
    assert!(
        on_files(cx) && !inside(note, view),
        "stacked, the saved files keep focus and the refusal {note:?} is not revealed \
         in {view:?} at {:?}",
        scroll.offset()
    );
    // The keyboard moves off the saved files and back, which the detail
    // reveals whole.
    for key in ["tab", "shift-tab"] {
        cx.simulate_keystrokes(key);
        crate::views::tests::draw(cx);
    }
    let files_in_view = |cx: &mut gpui::VisualTestContext| {
        let [files, view] =
            ["stash-files", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
        (inside(files, view), files, view)
    };
    let (shown, files_at, view) = files_in_view(cx);
    assert!(
        on_files(cx) && shown,
        "the saved files {files_at:?} are focused, whole in {view:?}"
    );
    let kept = scroll.offset();

    // The merge ends in a terminal; the next read says so in the note.
    git(&["merge", "--abort"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    quiet_reads_land(&app, cx);
    crate::views::tests::draw(cx);
    assert!(
        stashes.read_with(cx, |stashes, _| stashes.pause_ended.is_some())
            && cx.debug_bounds("stash-restore-ended").is_some(),
        "the note says the merge ended"
    );
    let (shown, files_at, view) = files_in_view(cx);
    assert!(
        on_files(cx) && shown && scroll.offset() == kept,
        "the saved files {files_at:?} keep focus, whole in {view:?}, the detail kept at \
         {kept:?}, not {:?}",
        scroll.offset()
    );
    drop(stashes);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// While Git prepares Review restore…'s restoration, Review restore…
/// says so in place, at 13 pt in a 1480 × 980 window and at 18 pt in a
/// 461 × 490 one, where it lies in the stacked detail: pressed from the
/// keyboard, its name says it is preparing at once, and once the
/// preparation looks busy it reads "Preparing…", keeping focus, whole
/// with its ring, and its width, so Drop… and the footer stay where they
/// were. Once Git answers, the review opens in Stashes' place. At rest
/// its label is drawn as Drop…'s beside it, as large and as heavy, in
/// the primary label colour. Nothing in Stashes said Git was preparing:
/// the window's progress label that said so lay beneath the dialog's
/// backdrop; and the label took the kit's small size, 11.375 px beside
/// Drop…'s 12 px at 13 pt.
#[gpui::test]
async fn review_restore_says_it_is_preparing_in_place(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    // The app's palette, whose accent draws the keyboard ring.
    crate::focus_ring::sweep::prepare(&app, cx);
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let _git = two_stashes(&repository);
    // Review restore…'s accessibility node as the window would write it
    // now, built inside a draw as `tags::tests` builds Review tag's.
    let name = |stashes: &Entity<StashBrowser>, cx: &mut gpui::VisualTestContext| {
        let stashes = stashes.clone();
        crate::views::tests::view_node(cx, move |window, cx| {
            use gpui::Element as _;
            let detail = stashes.read(cx).detail.clone();
            let focus = detail.focus("restore-saved-stash", cx);
            let button = stashes.read(cx).review_restore(&focus, cx);
            let button = button.base(window, cx);
            let element = RenderOnce::render(button, window, cx).into_element();
            let mut node =
                gpui::accesskit::Node::new(element.a11y_role().expect("Review restore…'s role"));
            element.write_a11y_info(&mut node);
            node
        })
        .label()
        .map(str::to_owned)
    };
    let footer = |cx: &mut gpui::VisualTestContext| {
        ["restore-saved-stash", "drop-saved-stash", "stash-actions"]
            .map(|name| cx.debug_bounds(name).unwrap_or_else(|| panic!("no {name}")))
    };
    let label = |cx: &mut gpui::VisualTestContext| match (
        cx.debug_bounds("restore-saved-stash/rest").is_some(),
        cx.debug_bounds("restore-saved-stash/preparing").is_some(),
    ) {
        (true, false) => "Review restore…",
        (false, true) => RESTORE_PREPARING,
        drawn => panic!("Review restore… draws one label, not {drawn:?}"),
    };
    for (width, height, interface) in [(1480., 980., 13), (461., 490., 18)] {
        let at = format!("{interface} pt in {width} × {height}");
        cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        tab_to(&stashes, "restore-saved-stash", cx);
        let at_rest = footer(cx);
        assert_eq!(label(cx), "Review restore…", "{at}");
        assert_eq!(
            name(&stashes, cx).as_deref(),
            Some("Review restore…"),
            "{at}"
        );
        // At rest its label is drawn as Drop…'s beside it, the compact
        // helper's: as large and as heavy, in the kit's primary label
        // colour.
        let text = |key: &str| {
            (
                appearance::drawn_size(key),
                appearance::drawn_weight(key),
                appearance::drawn_color(key),
            )
        };
        let (size, weight, color) = text("restore-saved-stash/rest/text");
        let (drop_size, drop_weight, _) = text("drop-saved-stash/text");
        assert!(
            size.is_some() && size == drop_size,
            "{at}: Review restore…'s label, {size:?}, is as large as Drop…'s, {drop_size:?}"
        );
        assert!(
            weight.is_some() && weight == drop_weight,
            "{at}: Review restore…'s label, {weight:?}, is as heavy as Drop…'s, {drop_weight:?}"
        );
        let primary =
            cx.read(|cx| gpui_kit::component::Theme::global(cx).button_primary_foreground);
        assert_eq!(
            color,
            Some(primary),
            "{at}: Review restore…'s label is in the primary label colour"
        );

        let held = app.update(cx, |app, _| app.operations.hold());
        space(cx);
        crate::views::tests::draw(cx);
        assert!(
            stashes.read_with(cx, |stashes, cx| stashes.preparing(cx)),
            "{at}: Git prepares the restoration"
        );
        // Heard at once; the look waits as the busy look does.
        assert_eq!(
            name(&stashes, cx).as_deref(),
            Some(RESTORE_PREPARING_NAME),
            "{at}: its name says it is preparing at once"
        );
        assert_eq!(label(cx), "Review restore…", "{at}: the look waits");
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        crate::views::tests::draw(cx);
        assert_eq!(
            label(cx),
            RESTORE_PREPARING,
            "{at}: it reads Preparing… once the preparation looks busy"
        );
        let preparing = footer(cx);
        for ((before, now), name) in
            at_rest
                .iter()
                .zip(&preparing)
                .zip(["Review restore…", "Drop…", "the footer"])
        {
            assert_eq!(before, now, "{at}: {name} keeps its place and width");
        }
        assert_eq!(
            cx.update(|window, cx| stashes.read(cx).detail.focused(window)),
            Some("restore-saved-stash".into()),
            "{at}: Review restore… keeps focus"
        );
        let ring = ring_around(cx, preparing[0])
            .unwrap_or_else(|| panic!("{at}: Review restore… draws no ring"));
        assert!(ring.whole, "{at}: Review restore…'s ring {ring:?} is whole");

        drop(held);
        recovery_lands(&app, cx);
        assert!(
            cx.debug_bounds("operation-consequences").is_some()
                && cx.debug_bounds("stash-browser").is_none(),
            "{at}: the review opens in place of Stashes"
        );
        drop(stashes);
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// Stacked at 18 pt in a 461 × 490 window, where the detail is about
/// 60 px tall, a stashed document's open control is one of the detail's
/// controls. Review restore… refused while a merge pauses, Shift+Tab
/// onto After's Open in default app scrolls it into the detail's view
/// with its ring, and once a read finds the merge ended the note says
/// so and the detail stays where it is, the control focused and whole
/// in view. Unknown to the detail, the control was not revealed, and the
/// note's reveal, which scrolls only while nothing else in the detail
/// holds focus, found none there and scrolled it away.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_focused_open_control_stays_in_view_as_a_pause_ends(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    // The app's palette, whose accent draws the keyboard ring.
    crate::focus_ring::sweep::prepare(&app, cx);
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let git = two_stashes(&repository);
    // The newest stash saves a document's second version.
    let pdf =
        |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
    let document = repository.join("document.pdf");
    std::fs::write(&document, pdf("first")).unwrap();
    git(&["add", "document.pdf"]);
    git(&["commit", "-q", "-m", "Add a document"]);
    std::fs::write(&document, pdf("second")).unwrap();
    git(&["stash", "push", "-q", "-m", "Document"]);
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    let slack = px(0.5);
    let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    };

    // Refused while the merge pauses, unread, which the refusal has the
    // window read, with the document chosen.
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    cx.update(|window, cx| stashes.update(cx, |stashes, cx| stashes.select_file(0, window, cx)));
    landed(&app, cx, "system-preview-1");
    press_review_restore(&stashes, cx);
    recovery_lands(&app, cx);
    quiet_reads_land(&app, cx);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
        Some(PAUSED_REFUSAL.to_owned())
    );
    assert!(app.read_with(cx, |app, _| app.integration_state.is_some()));

    // Shift+Tab back onto After's open control, which scrolls into the
    // detail's view with its ring.
    let detail = stashes.read_with(cx, |stashes, _| stashes.detail.clone());
    let open = cx.update(|_, cx| detail.focus(("system-preview", 1_usize), cx));
    let mut reached = false;
    for _ in 0..12 {
        cx.simulate_keystrokes("shift-tab");
        crate::views::tests::draw(cx);
        if cx.update(|window, _| open.is_focused(window)) {
            reached = true;
            break;
        }
    }
    assert!(reached, "Shift+Tab reaches After's open control");
    crate::views::tests::draw(cx);
    let room = cx.read(appearance::button_ring_room);
    let in_view = |cx: &mut gpui::VisualTestContext| {
        let [control, view] =
            ["system-preview-1", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
        (inside(control.dilate(room), view), control, view)
    };
    let (shown, control, view) = in_view(cx);
    assert!(
        shown,
        "the open control {control:?} and its ring lie in the detail's view {view:?}"
    );
    let ring = ring_around(cx, control)
        .unwrap_or_else(|| panic!("the open control {control:?} draws no ring"));
    assert!(ring.whole, "its ring {ring:?} is whole");
    let kept = detail.scroll().offset();

    // The merge ends in a terminal; the next read says so in the note.
    git(&["merge", "--abort"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    quiet_reads_land(&app, cx);
    crate::views::tests::draw(cx);
    assert!(
        stashes.read_with(cx, |stashes, _| stashes.restore_line.is_none()
            && stashes.pause_ended.is_some())
            && cx.debug_bounds("stash-restore-ended").is_some(),
        "the note says the merge ended"
    );
    assert_eq!(
        detail.scroll().offset(),
        kept,
        "the detail stays where the keyboard left it"
    );
    let (shown, control, view) = in_view(cx);
    assert!(
        cx.update(|window, _| open.is_focused(window)) && shown,
        "the open control {control:?} keeps focus in the detail's view {view:?}"
    );
    drop(stashes);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    drop(fixture);
}

/// A recovery action refuses while an integration is paused from the
/// pause the window read last, which a change outside the app can end
/// unseen, as it does here: the test's local watch reports nothing,
/// like one whose read has not landed yet, or a retained tab's restored
/// pause. Review stash in an open Save changes to stash… form is
/// refused in the core's words and asks for a quiet read of the working
/// state; once it lands and finds the merge ended, the form's line says
/// so instead of refusing. Revert, as a keyboard's commit menu offers
/// it, is refused in the window's error and asks the same, and that
/// error ends with the merge. Nothing read the pause again: each try was
/// refused from the same stale pause until some other read landed, and
/// the form went on refusing after it.
#[gpui::test]
async fn a_paused_refusal_reads_the_pause_again_and_gives_way_once_it_ends(
    cx: &mut gpui::TestAppContext,
) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let identity = [
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
    ];
    let git = |arguments: &[&str]| {
        crate::views::tests::fixture_git(&repository, &[&identity[..], arguments].concat())
    };
    git(&["reset", "-q", "--hard"]);
    git(&["checkout", "-q", "-b", "side"]);
    std::fs::write(repository.join("side.txt"), "side\n").unwrap();
    git(&["add", "side.txt"]);
    git(&["commit", "-q", "-m", "side"]);
    git(&["checkout", "-q", "main"]);
    let paused = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |app, _| app.integration_state.is_some())
    };
    let asked = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |app, _| app.automatic.quiet_read_requested())
    };

    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    read_pause(&app, cx).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stash_form(window, cx)));
    crate::views::tests::draw(cx);
    let form = app.read_with(cx, |app, _| app.recovery.stash.clone().expect("the form"));
    cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            form.name
                .update(cx, |name, cx| name.set_value("paused work", window, cx))
        })
    });
    // The merge ends in a terminal, unseen.
    git(&["merge", "--abort"]);
    // Review stash, as its button asks, with nothing run after it.
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
            cx,
        )
    });
    frame(cx);
    assert_eq!(
        form.read_with(cx, |form, _| form.error.clone()),
        Some(PAUSED_REFUSAL.into()),
        "Review stash is refused from the pause read last"
    );
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "the form reopens"
    );
    assert!(
        cx.debug_bounds("recovery-form-error").is_some(),
        "the form draws the refusal"
    );
    assert!(asked(cx), "the refusal asks for a quiet read");
    assert!(paused(cx), "the read has not landed");

    quiet_reads_land(&app, cx);
    frame(cx);
    assert!(!paused(cx), "the read finds the merge ended");
    assert_eq!(
        form.read_with(cx, |form, _| (form.error.clone(), form.pause_ended)),
        (None, Some(Some(OperationKind::Merge))),
        "the refusal gives way"
    );
    assert!(
        cx.debug_bounds("recovery-form-error").is_none(),
        "the form draws no refusal"
    );
    assert!(
        cx.debug_bounds("recovery-form-note").is_some(),
        "the form says the merge ended"
    );
    assert_eq!(
        crate::native_accessibility::written::announced("recovery-form-note")
            .map(|announced| announced.live),
        Some(gpui::accesskit::Live::Polite),
        "a status, heard once speech pauses"
    );
    cx.update(|window, cx| window.close_dialog(cx));
    crate::views::tests::draw(cx);

    // Revert, as Enter on a keyboard's commit menu chooses it.
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    read_pause(&app, cx).await;
    git(&["merge", "--abort"]);
    let commit = app.read_with(cx, |app, _| commit_target(&app.commits[0]));
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.choose_commit_recovery(RecoveryKind::Revert, commit, window, cx)
        })
    });
    assert_eq!(
        app.read_with(cx, |app, _| app.operation_error_text()),
        Some(PAUSED_REFUSAL.into()),
        "Revert is refused in the window's error"
    );
    assert!(asked(cx), "the refusal asks for a quiet read");
    quiet_reads_land(&app, cx);
    assert!(!paused(cx), "the read finds the merge ended again");
    assert_eq!(
        app.read_with(cx, |app, _| app.operation_error_text()),
        None,
        "the window's refusal ends with the merge"
    );
    drop(fixture);
}

/// A recovery action refused from the pause the window read names the
/// paused operation and how to end it, in the core's words: a commit's
/// Revert and Save changes to stash…'s Review stash say so for a
/// cherry-pick and for a revert stopped on a conflict outside the app.
/// Every pause once read "Finish the current operation and resolve
/// conflicts before starting another recovery action", whatever had
/// paused. Once it ends, the form's line names the operation that ended
/// and the form's own button; it once read "The paused operation has
/// ended. Review again to continue.", naming neither.
#[gpui::test]
async fn a_paused_refusal_names_the_paused_operation(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let identity = [
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "core.editor=true",
    ];
    let arguments = |rest: &[&'static str]| -> Vec<&'static str> { [&identity[..], rest].concat() };
    let git = |args: &[&str]| crate::views::tests::fixture_git(&repository, args);
    let commit = |text: &'static str| {
        std::fs::write(repository.join("conflict.txt"), text).unwrap();
        git(&arguments(&["add", "conflict.txt"]));
        git(&arguments(&["commit", "-q", "-m", text.trim()]));
    };
    git(&["reset", "-q", "--hard"]);
    git(&["checkout", "-q", "-b", "side"]);
    commit("side\n");
    git(&["checkout", "-q", "main"]);
    commit("one\n");
    commit("two\n");
    for (stop, end, kind, said, ended) in [
        (
            ["cherry-pick", "side"],
            "cherry-pick",
            gitturtle_core::OperationKind::CherryPick,
            "A cherry-pick is paused. Continue or abort it before another recovery action.",
            "The cherry-pick has ended. Choose Review stash to continue.",
        ),
        (
            ["revert", "HEAD~1"],
            "revert",
            gitturtle_core::OperationKind::Revert,
            "A revert is paused. Continue or abort it before another recovery action.",
            "The revert has ended. Choose Review stash to continue.",
        ),
    ] {
        let stopped = crate::views::tests::fixture_git_output(&repository, &arguments(&stop));
        assert!(!stopped.status.success(), "{kind:?} stops on a conflict");
        read_pause(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app
                .integration_state
                .as_ref()
                .map(|state| state.kind)),
            Some(kind)
        );
        let target = app.read_with(cx, |app, _| commit_target(&app.commits[0]));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.choose_commit_recovery(RecoveryKind::Revert, target, window, cx)
            })
        });
        frame(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error_text())
                .as_deref(),
            Some(said),
            "{kind:?}: the window's error names it"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stash_form(window, cx)));
        crate::views::tests::draw(cx);
        let form = app.read_with(cx, |app, _| app.recovery.stash.clone().expect("the form"));
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.name
                    .update(cx, |name, cx| name.set_value("paused work", window, cx))
            })
        });
        cx.update(|window, cx| {
            window.dispatch_action(
                Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
                cx,
            )
        });
        frame(cx);
        assert_eq!(
            form.read_with(cx, |form, _| form.error.clone()).as_deref(),
            Some(said),
            "{kind:?}: Review stash is refused in the same words"
        );
        git(&[end, "--abort"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        crate::views::tests::draw(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error_text()),
            None,
            "{kind:?}: the refusal ends with the pause"
        );
        assert_eq!(
            form.read_with(cx, |form, _| (form.error.clone(), form.pause_ended)),
            (None, Some(Some(kind))),
            "{kind:?}: the form's refusal gives way"
        );
        let note = crate::native_accessibility::written::get("recovery-form-note")
            .unwrap_or_else(|| panic!("{kind:?}: the form's note"));
        assert_eq!(
            note.node.label(),
            Some(ended),
            "{kind:?}: the note names the operation and Review stash"
        );
        cx.update(|window, cx| window.close_dialog(cx));
        crate::views::tests::draw(cx);
    }
    drop(fixture);
}

/// The end of a paused integration ends the window's refusal for it, but
/// not one for conflicts left with no operation, which that end does not
/// resolve, as the forms already keep theirs; the window once cleared
/// both.
#[gpui::test]
async fn an_ended_pause_keeps_a_refusal_for_conflicts_alone(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(2, 1))
            .await;
    for (operation, kept) in [(None, true), (Some(OperationKind::Merge), false)] {
        let refusal = gitturtle_core::paused_recovery_refusal(operation);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.operation_error = Some(refusal.into());
                app.integration_ended(window, cx);
            })
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error_text()),
            kept.then(|| refusal.to_owned()),
            "{operation:?}"
        );
    }
    drop(fixture);
}

/// The recovery forms a paused integration refuses.
#[derive(Clone, Copy, Debug)]
enum PausedForm {
    /// Amend last commit…, its title edited.
    Amend,
    /// Save changes to stash…, named.
    Stash,
}

/// What `form` holds: why its review was refused, if it was, and the
/// field it was given.
fn paused_form(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
    form: PausedForm,
) -> (Option<String>, String) {
    app.read_with(cx, |app, cx| match form {
        PausedForm::Amend => {
            let form = app
                .recovery
                .amend
                .as_ref()
                .expect("the amend form")
                .read(cx);
            let fields = form.fields.as_ref().expect("the amend fields");
            (
                form.error.clone(),
                fields.title.read(cx).value().to_string(),
            )
        }
        PausedForm::Stash => {
            let form = app
                .recovery
                .stash
                .as_ref()
                .expect("the stash form")
                .read(cx);
            (form.error.clone(), form.name.read(cx).value().to_string())
        }
    })
}

/// A window on a checked-out repository whose tip's message is exactly
/// `message`, with Amend last commit's form opened on it.
async fn amend_form_on<'a>(
    cx: &'a mut gpui::TestAppContext,
    message: &str,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    &'a mut gpui::VisualTestContext,
) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let identity = [
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
    ];
    let git = |arguments: &[&str]| {
        crate::views::tests::fixture_git(&repository, &[&identity[..], arguments].concat())
    };
    git(&["reset", "-q", "--hard"]);
    let file = fixture.path().join("message");
    std::fs::write(&file, message).unwrap();
    git(&[
        "commit",
        "-q",
        "--allow-empty",
        "--cleanup=verbatim",
        "-F",
        file.to_str().unwrap(),
    ]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    let head = app.read_with(cx, |app, _| {
        app.work_status
            .as_ref()
            .and_then(|status| status.head.clone())
            .expect("HEAD")
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.prepare_commit_recovery(RecoveryKind::Amend, head, None, window, cx)
        })
    });
    let read = app.update(cx, |app, _| app.recovery.task.take());
    read.expect("the amend plan is read").await;
    crate::views::tests::draw(cx);
    (fixture, app, cx)
}

const AMEND_TITLE: &str = "Title\n\n";

/// A message over what Amend can submit is not loaded: the form says so
/// where the fields would be, and its only action closes it, so nothing
/// offered can fail. Enter is that action, and starts no review.
#[gpui::test]
async fn a_message_past_the_commit_limit_is_refused_in_place_of_the_editor(
    cx: &mut gpui::TestAppContext,
) {
    let message = format!(
        "{AMEND_TITLE}{}",
        "x".repeat(MAX_RECOVERY_MESSAGE_BYTES + 1 - AMEND_TITLE.len())
    );
    let (_fixture, app, cx) = amend_form_on(cx, &message).await;
    let form = app.read_with(cx, |app, _| app.recovery.amend.clone().expect("the form"));
    assert!(
        form.read_with(cx, |form, _| form.fields.is_none()),
        "no editor holds any of the message"
    );
    assert_eq!(
        crate::native_accessibility::written::get("amend-message-too-long")
            .and_then(|said| said.node.value().map(str::to_owned))
            .as_deref(),
        Some("This commit's message is over 64 KiB, too long to edit here"),
    );
    assert!(
        cx.debug_bounds("amend-message-too-long").is_some(),
        "the notice stands in the form"
    );
    assert!(
        cx.debug_bounds("dialog-ok: Review amend").is_none(),
        "no review is offered that would fail"
    );
    assert!(
        cx.debug_bounds("dialog-ok: Close").is_some(),
        "its one action closes it"
    );
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
            cx,
        )
    });
    crate::views::tests::draw(cx);
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "Enter closes it"
    );
    assert!(
        app.read_with(cx, |app, _| app.recovery.task.is_none()),
        "and asks for no review"
    );
}

/// A message at the limit is loaded whole, byte for byte, and can be
/// reviewed as any other.
#[gpui::test]
async fn a_message_at_the_commit_limit_loads_whole(cx: &mut gpui::TestAppContext) {
    let message = format!(
        "{AMEND_TITLE}{}",
        "x".repeat(MAX_RECOVERY_MESSAGE_BYTES - AMEND_TITLE.len())
    );
    let (_fixture, app, cx) = amend_form_on(cx, &message).await;
    let form = app.read_with(cx, |app, _| app.recovery.amend.clone().expect("the form"));
    assert_eq!(
        form.read_with(cx, |form, cx| form.message(cx)).as_deref(),
        Some(message.as_str()),
        "the whole message is the form's, unedited"
    );
    let description = app
        .read_with(cx, amend_description)
        .expect("its description field");
    assert_eq!(
        description.read_with(cx, |field, _| field.value().len()),
        MAX_RECOVERY_MESSAGE_BYTES - AMEND_TITLE.len(),
        "the description field holds all of it"
    );
    assert!(
        cx.debug_bounds("amend-message-too-long").is_none(),
        "no notice stands in its place"
    );
    assert!(
        cx.debug_bounds("dialog-ok: Review amend").is_some(),
        "its review is offered"
    );
}

/// Opens `form` at rest and fills its field with `value`, then pauses a
/// merge outside the app and reads it, as the local watcher would, and
/// asks for the form's review, as its button does: the review is
/// refused in the core's words without reading a plan, and the form
/// opens again with that refusal beneath its fields and the value kept,
/// asking for a quiet read of the pause. The refusal is announced at
/// once, and asked for again while still paused, the form opens again
/// with it already there and announces it again by a fresh node.
async fn reopened_refused_while_paused(cx: &mut gpui::TestAppContext, form: PausedForm) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let identity = [
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
    ];
    let git = |arguments: &[&str]| {
        crate::views::tests::fixture_git(&repository, &[&identity[..], arguments].concat())
    };
    git(&["reset", "-q", "--hard"]);
    git(&["checkout", "-q", "-b", "side"]);
    std::fs::write(repository.join("side.txt"), "side\n").unwrap();
    git(&["add", "side.txt"]);
    git(&["commit", "-q", "-m", "side"]);
    git(&["checkout", "-q", "main"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    let dialog_open =
        |cx: &mut gpui::VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));

    // Opened at rest, its field filled.
    let value = match form {
        PausedForm::Amend => {
            let head = app.read_with(cx, |app, _| {
                app.work_status
                    .as_ref()
                    .and_then(|status| status.head.clone())
                    .expect("HEAD")
            });
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.prepare_commit_recovery(RecoveryKind::Amend, head, None, window, cx)
                })
            });
            let read = app.update(cx, |app, _| app.recovery.task.take());
            read.expect("the amend plan is read").await;
            crate::views::tests::draw(cx);
            let form = app.read_with(cx, |app, _| app.recovery.amend.clone().expect("the form"));
            cx.update(|window, cx| {
                form.update(cx, |form, cx| {
                    let fields = form.fields.as_ref().expect("the amend fields");
                    fields.title.update(cx, |title, cx| {
                        title.set_value("Reworded while paused", window, cx)
                    })
                })
            });
            "Reworded while paused"
        }
        PausedForm::Stash => {
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_stash_form(window, cx)));
            crate::views::tests::draw(cx);
            let form = app.read_with(cx, |app, _| app.recovery.stash.clone().expect("the form"));
            cx.update(|window, cx| {
                form.update(cx, |form, cx| {
                    form.name
                        .update(cx, |name, cx| name.set_value("paused work", window, cx))
                })
            });
            "paused work"
        }
    };
    crate::views::tests::draw(cx);
    assert!(dialog_open(cx), "{form:?}: the form opened");
    assert_eq!(
        paused_form(&app, cx, form),
        (None, value.into()),
        "{form:?}: at rest"
    );

    // A merge pauses, and the window reads it.
    git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
    read_pause(&app, cx).await;
    assert!(
        dialog_open(cx),
        "{form:?}: the form stays open as the pause is read"
    );
    // Review, as its button asks.
    let nothing = app.update(cx, |app, _| app.operations.hold());
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
            cx,
        )
    });
    frame(cx);
    assert_eq!(
        paused_form(&app, cx, form),
        (Some(PAUSED_REFUSAL.into()), value.into()),
        "{form:?}: refused in the core's words, its field kept"
    );
    assert!(dialog_open(cx), "{form:?}: the form opens again");
    assert!(
        cx.debug_bounds("recovery-form-error").is_some(),
        "{form:?}: the form draws its refusal"
    );
    assert_eq!(
        app.read_with(cx, |app, _| (
            app.operation_busy,
            app.recovery.task.is_some()
        )),
        (None, false),
        "{form:?}: no plan is read"
    );
    assert!(
        app.read_with(cx, |app, _| app.automatic.quiet_read_requested()),
        "{form:?}: the refusal asks for a quiet read of the pause"
    );
    // Announced at once, as when it first appears in an open form.
    let first = refusal_announced(form);
    quiet_reads_land(&app, cx);
    assert!(
        app.read_with(cx, |app, _| app.integration_state.is_some()),
        "{form:?}: the read finds the merge still paused"
    );

    // Review again, still paused: the form opens again with the same
    // refusal already there, announced again by a fresh node.
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
            cx,
        )
    });
    frame(cx);
    assert_eq!(
        paused_form(&app, cx, form),
        (Some(PAUSED_REFUSAL.into()), value.into()),
        "{form:?}: refused again"
    );
    assert!(dialog_open(cx), "{form:?}: the form opens again once more");
    let second = refusal_announced(form);
    assert_ne!(
        first, second,
        "{form:?}: the refusal it opens with is a fresh node, announced again"
    );
    drop(nothing);
    quiet_reads_land(&app, cx);
    drop(fixture);
}

/// Why a recovery action refuses while conflicts remain with no
/// operation paused (`gitturtle_core::paused_recovery_refusal`).
const CONFLICTS_REFUSAL: &str = gitturtle_core::paused_recovery_refusal(None);

/// Saves an edit of notes.txt in a stash and commits another over it,
/// so that applying the stash outside the app ([`conflict_alone`])
/// leaves notes.txt conflicted.
fn clashing_stash(repository: &std::path::Path) {
    let git = |arguments: &[&str]| {
        let identity = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ];
        crate::views::tests::fixture_git(repository, &[&identity[..], arguments].concat())
    };
    std::fs::write(repository.join("notes.txt"), "stashed\n").unwrap();
    git(&["stash", "push", "-q", "-m", "Clash"]);
    std::fs::write(repository.join("notes.txt"), "committed\n").unwrap();
    git(&["commit", "-q", "-am", "Clash"]);
}

/// Applies the stash `clashing_stash` saved outside the app, which
/// stops with notes.txt conflicted and no operation paused, as a
/// conflicted stash restoration leaves it.
fn conflict_alone(repository: &std::path::Path, stash: &str) {
    let applied =
        crate::views::tests::fixture_git_output(repository, &["stash", "apply", "-q", stash]);
    assert!(!applied.status.success(), "the stash clashes");
    let unmerged = crate::views::tests::fixture_git_output(repository, &["ls-files", "-u"]);
    assert!(!unmerged.stdout.is_empty(), "notes.txt is conflicted");
    assert!(
        !repository.join(".git").join("MERGE_HEAD").exists(),
        "no merge is paused"
    );
}

/// Resolves notes.txt outside the app and has the window read it
/// quietly, as the local watcher would.
fn conflict_resolved(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
    repository: &std::path::Path,
) {
    std::fs::write(repository.join("notes.txt"), "resolved\n").unwrap();
    crate::views::tests::fixture_git(repository, &["add", "notes.txt"]);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.queue_automatic_refresh(
                local_refresh::LocalChange {
                    worktree: true,
                    ..Default::default()
                },
                window,
                cx,
            )
        })
    });
    quiet_reads_land(app, cx);
    frame(cx);
    let unmerged = crate::views::tests::fixture_git_output(repository, &["ls-files", "-u"]);
    assert!(unmerged.stdout.is_empty(), "notes.txt is resolved");
}

/// Opens `form` at rest and fills its field, then has a stash restored
/// outside the app leave notes.txt conflicted with no operation paused,
/// and asks for the form's review: the core refuses it for conflicts
/// alone, and the form opens again with that refusal. It stays while a
/// read finds the conflict left and gives way in the frame a read finds
/// none, as the window's error does, to the line that says none remain,
/// naming the form's button, a status heard once speech pauses
/// (`GitTurtle::conflicts_ended`). A refusal that names a paused merge
/// stays through such a read. The form's refusal once stayed after the
/// conflicts were resolved, though the window's error had gone.
async fn refused_for_conflicts_alone(cx: &mut gpui::TestAppContext, form: PausedForm) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    crate::views::tests::fixture_git(&repository, &["reset", "-q", "--hard"]);
    clashing_stash(&repository);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    let review = match form {
        PausedForm::Amend => {
            let head = app.read_with(cx, |app, _| {
                app.work_status
                    .as_ref()
                    .and_then(|status| status.head.clone())
                    .expect("HEAD")
            });
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.prepare_commit_recovery(RecoveryKind::Amend, head, None, window, cx)
                })
            });
            let read = app.update(cx, |app, _| app.recovery.task.take());
            read.expect("the amend plan is read").await;
            REVIEW_AMEND
        }
        PausedForm::Stash => {
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_stash_form(window, cx)));
            crate::views::tests::draw(cx);
            let form = app.read_with(cx, |app, _| app.recovery.stash.clone().expect("the form"));
            cx.update(|window, cx| {
                form.update(cx, |form, cx| {
                    form.name
                        .update(cx, |name, cx| name.set_value("conflicted work", window, cx))
                })
            });
            REVIEW_STASH
        }
    };
    crate::views::tests::draw(cx);
    let ended = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |app, cx| match form {
            PausedForm::Amend => app.recovery.amend.as_ref().map(|f| f.read(cx).pause_ended),
            PausedForm::Stash => app.recovery.stash.as_ref().map(|f| f.read(cx).pause_ended),
        })
        .expect("the form")
    };

    conflict_alone(&repository, "stash@{0}");
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
            cx,
        )
    });
    recovery_lands(&app, cx);
    assert_eq!(
        paused_form(&app, cx, form).0.as_deref(),
        Some(CONFLICTS_REFUSAL),
        "{form:?}: refused in the core's words for conflicts alone"
    );
    assert!(
        cx.debug_bounds("recovery-form-error").is_some(),
        "{form:?}: the form draws its refusal"
    );
    // The read the refusal asks for finds the conflict left.
    quiet_reads_land(&app, cx);
    frame(cx);
    assert!(app.read_with(cx, |app, _| app.integration_state.is_none()));
    assert_eq!(
        paused_form(&app, cx, form).0.as_deref(),
        Some(CONFLICTS_REFUSAL),
        "{form:?}: the refusal stays while the conflict does"
    );

    conflict_resolved(&app, cx, &repository);
    assert_eq!(
        (paused_form(&app, cx, form).0, ended(cx)),
        (None, Some(None)),
        "{form:?}: the refusal gives way in the frame the read finds no conflict"
    );
    assert!(
        cx.debug_bounds("recovery-form-error").is_none(),
        "{form:?}: the form draws no refusal"
    );
    let line = format!("No conflicts remain. Choose {review} to continue.");
    let note = crate::native_accessibility::written::get("recovery-form-note")
        .unwrap_or_else(|| panic!("{form:?}: the form's note"));
    assert_eq!(
        note.node.label(),
        Some(line.as_str()),
        "{form:?}: says none remain"
    );
    assert_eq!(
        crate::native_accessibility::written::announced("recovery-form-note")
            .map(|announced| announced.live),
        Some(gpui::accesskit::Live::Polite),
        "{form:?}: a status, heard once speech pauses"
    );

    // A refusal that names a paused merge stays through a read that
    // finds no conflict and no pause.
    app.update(cx, |app, cx| match form {
        PausedForm::Amend => {
            app.recovery
                .amend
                .clone()
                .expect("the form")
                .update(cx, |form, cx| {
                    form.error = Some(PAUSED_REFUSAL.into());
                    cx.notify();
                })
        }
        PausedForm::Stash => {
            app.recovery
                .stash
                .clone()
                .expect("the form")
                .update(cx, |form, cx| {
                    form.error = Some(PAUSED_REFUSAL.into());
                    cx.notify();
                })
        }
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    frame(cx);
    assert_eq!(
        paused_form(&app, cx, form).0.as_deref(),
        Some(PAUSED_REFUSAL),
        "{form:?}: a refusal naming a paused merge stays"
    );
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    drop(fixture);
}

/// Amend last commit…'s refusal for conflicts alone gives way once a
/// read finds them resolved.
#[gpui::test]
async fn the_amend_forms_refusal_for_conflicts_goes_as_they_are_resolved(
    cx: &mut gpui::TestAppContext,
) {
    refused_for_conflicts_alone(cx, PausedForm::Amend).await;
}

/// Save changes to stash…'s refusal for conflicts alone gives way once
/// a read finds them resolved.
#[gpui::test]
async fn the_stash_forms_refusal_for_conflicts_goes_as_they_are_resolved(
    cx: &mut gpui::TestAppContext,
) {
    refused_for_conflicts_alone(cx, PausedForm::Stash).await;
}

/// A refusal for conflicts alone stays through a read asked for before
/// it was raised, which may have read the working state before the
/// conflict came: a read asked for, and made, before a stash is
/// restored outside the app, held while Save changes to stash… and a
/// commit's Revert are refused for the conflict it left, then landing
/// with no conflict, leaves the form's line and the window's error
/// standing. The read the refusals ask for finds the conflict left, and
/// both go once a later read finds it resolved. A read from before the
/// refusal once replaced them with "No conflicts remain" while the
/// conflict stood (`GitTurtle::conflicts_ended`).
#[gpui::test]
async fn a_read_from_before_a_refusal_for_conflicts_keeps_it(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    crate::views::tests::fixture_git(&repository, &["reset", "-q", "--hard"]);
    clashing_stash(&repository);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stash_form(window, cx)));
    crate::views::tests::draw(cx);
    let form = app.read_with(cx, |app, _| app.recovery.stash.clone().expect("the form"));
    cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            form.name
                .update(cx, |name, cx| name.set_value("conflicted work", window, cx))
        })
    });
    crate::views::tests::draw(cx);
    let refusals = |cx: &mut gpui::VisualTestContext| {
        (
            form.read_with(cx, |form, _| (form.error.clone(), form.pause_ended)),
            app.read_with(cx, |app, _| app.operation_error_text()),
        )
    };
    let standing = (
        (Some(CONFLICTS_REFUSAL.to_owned()), None),
        Some(CONFLICTS_REFUSAL.to_owned()),
    );

    // The read held across the refusals, asked for and made before the
    // stash is restored.
    let (requested, held) = app.update(cx, |app, _| {
        let requested = app.request_status_read();
        let repo = app.repository.clone().expect("the repository");
        (
            requested,
            crate::worker::WorkingState::read(&repo).expect("the working state"),
        )
    });
    assert!(
        !held.status.entries.iter().any(|entry| entry.conflicted),
        "the held read finds no conflict"
    );
    conflict_alone(&repository, "stash@{0}");
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
            cx,
        )
    });
    recovery_lands(&app, cx);
    let head = app.read_with(cx, |app, _| {
        app.work_status
            .as_ref()
            .and_then(|status| status.head.clone())
            .expect("HEAD")
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.prepare_commit_recovery(RecoveryKind::Revert, head, None, window, cx)
        })
    });
    recovery_lands(&app, cx);
    assert_eq!(refusals(cx), standing, "both refused for conflicts alone");

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_worktree_state(held, true, false, requested, window, cx)
        })
    });
    frame(cx);
    assert_eq!(
        refusals(cx),
        standing,
        "the read from before the refusals leaves them standing"
    );
    assert!(
        cx.debug_bounds("recovery-form-error").is_some(),
        "the form draws its refusal"
    );

    // The read the refusals asked for finds the conflict left.
    quiet_reads_land(&app, cx);
    frame(cx);
    assert_eq!(refusals(cx), standing, "they stay while the conflict does");

    conflict_resolved(&app, cx, &repository);
    assert_eq!(
        refusals(cx),
        ((None, Some(None)), None),
        "both go in the frame a later read finds no conflict"
    );
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    drop(fixture);
}

/// Stashes' Review restore…, refused for a conflict a restoration
/// outside the app left with no operation paused, keeps that refusal in
/// its note while a read finds the conflict left, and in the frame a
/// read finds none the note says none remain, naming Review restore…,
/// which the window's announcement says once (`conflicts_ended`), as
/// at the end of a pause. The refusal once stayed until another stash
/// was chosen, saying conflicts remained after they were resolved.
#[gpui::test]
async fn stashes_refusal_for_conflicts_goes_as_they_are_resolved(cx: &mut gpui::TestAppContext) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
            .await;
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let _git = two_stashes(&repository);
    clashing_stash(&repository);
    conflict_alone(&repository, "stash@{0}");
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    assert!(app.read_with(cx, |app, _| app.integration_state.is_none()));
    open_selected_stash(&app, cx).await;
    let stashes = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("Stashes is open");
    press_review_restore(&stashes, cx);
    recovery_lands(&app, cx);
    quiet_reads_land(&app, cx);
    crate::views::tests::draw(cx);
    assert_eq!(
        stashes
            .read_with(cx, |stashes, _| stashes.restore_line.clone())
            .as_deref(),
        Some(CONFLICTS_REFUSAL),
        "refused for conflicts alone, which a read finds left"
    );
    let mut heard = Heard::start(cx);
    conflict_resolved(&app, cx, &repository);
    crate::views::tests::draw(cx);
    let line = pause_ended_line(None, REVIEW_RESTORE);
    assert_eq!(
        stashes.read_with(cx, |stashes, _| (
            stashes.restore_line.clone(),
            stashes.pause_ended
        )),
        (None, Some(None)),
        "the refusal gives way in the frame the read finds no conflict"
    );
    assert!(
        cx.debug_bounds("stash-restore-ended").is_some(),
        "the note says none remain"
    );
    let found = reached(cx);
    assert_eq!(
        said_from_the_window(&found, &line, gpui::accesskit::Live::Polite),
        1,
        "the window's announcement says none remain: {found:?}"
    );
    if let Some(heard) = heard.next(cx) {
        assert!(
            heard.iter().filter(|said| **said == *line).count() == 1,
            "AT-SPI announces it once: {heard:?}"
        );
    }
    drop(stashes);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::views::tests::draw(cx);
    drop(fixture);
}

/// The node that announces the open recovery form's refusal: one, at
/// once.
fn refusal_announced(form: PausedForm) -> String {
    let mut announced =
        crate::native_accessibility::written::announced_from("recovery-form-error-");
    assert_eq!(
        announced.len(),
        1,
        "{form:?}: one refusal announced: {announced:?}"
    );
    let (node, how) = announced.pop().expect("the refusal");
    assert_eq!(
        how,
        crate::native_accessibility::written::Announced::ASSERTIVE,
        "{form:?}: the refusal is announced at once"
    );
    // Its name, which AT-SPI announces, and its value, which macOS does.
    let written = crate::native_accessibility::written::get(&node).expect("the refusal's node");
    assert_eq!(
        (written.node.label(), written.node.value()),
        (Some(PAUSED_REFUSAL), Some(PAUSED_REFUSAL)),
        "{form:?}: the refusal is its node's name and value"
    );
    node
}

/// Amend last commit…, opened at rest, is refused while a merge is
/// paused and opens again with the refusal and the reworded title.
#[gpui::test]
async fn the_amend_form_reopens_refused_while_paused(cx: &mut gpui::TestAppContext) {
    reopened_refused_while_paused(cx, PausedForm::Amend).await;
}

/// Save changes to stash…, opened at rest, is refused while a merge is
/// paused and opens again with the refusal and the stash's name.
#[gpui::test]
async fn the_stash_form_reopens_refused_while_paused(cx: &mut gpui::TestAppContext) {
    reopened_refused_while_paused(cx, PausedForm::Stash).await;
}
