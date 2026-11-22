use super::*;
use ::core::prelude::v1::test;

/// The series review's unified patch keeps its decorations for the editor's life, and its
/// preview figure counts them along with the presentation they were built from.
#[gpui::test]
async fn series_patch_preview_counts_its_decorations_and_presentation(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let fixture = tempfile::tempdir().unwrap();
    let repository = GitRepository::init(fixture.path().join("repository"), "main").unwrap();
    cx.update(gpui_kit::init);
    let captured: std::rc::Rc<std::cell::RefCell<Option<Entity<SeriesBrowser>>>> =
        Default::default();
    let output = captured.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                WeakEntity::new_invalid(),
                repository,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        *output.borrow_mut() = Some(browser.clone());
        gpui_kit::component::Root::new(browser, window, cx)
    });
    cx.run_until_parked();
    let browser = captured.borrow().as_ref().unwrap().clone();
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.content = Some(Arc::new(text::decorated_patch_content()));
            browser.text_mode = 0;
            browser.prepare_editor(window, cx);
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

            // A source mode replaces the patch editor and releases its decorations.
            browser.content = Some(Arc::new(text::decorated_patch_content()));
            browser.decoration = Some(decoration);
            browser.text_mode = 1;
            browser.prepare_editor(window, cx);
            assert!(browser.decoration.is_none());
            browser.clear_content();
            assert_eq!(browser.preview_retained_bytes(), 0);
        })
    });
}

/// An inspected commit's description, beside Back to series in a row
/// pulled before the dialog's text edge, takes the rest of that row and
/// wraps there, at 13 and 18 pt in a 461 × 490 window: a subject longer
/// than the room beside the Button wrapped onto a line of its own, which
/// started at the row's pulled left, 10 px before the text edge at 13 pt
/// and 14 px at 18 pt.
#[gpui::test]
async fn an_inspected_commits_description_wraps_beside_back_to_series(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, set_interface_text_size, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.inspecting = true;
            browser.inspect_label = format!(
                "Rewritten 0123456789 · {}",
                "Keep the subject of a rewritten commit whole ".repeat(3)
            );
            cx.notify();
        })
    });
    for interface in [13, 18] {
        set_interface_text_size(cx, &app, interface);
        let back = cx
            .debug_bounds("back-rewrite-series")
            .expect("Back to series");
        let label = cx
            .debug_bounds("rewrite-inspect-label")
            .expect("the inspected commit's description");
        assert!(
            label.left() >= back.right(),
            "at {interface} pt the description at {label:?} starts left of Back to series' \
             right side, {:?}",
            back.right()
        );
        assert!(
            label.size.height > back.size.height,
            "at {interface} pt the description wraps beside the Button: {label:?}"
        );
    }
}

/// The series review over a window of the tagged fixture at 18 pt in a
/// 461 × 490 window, inspecting its last commit, whose one changed file's
/// preview has landed in Patch: the fixture's directory, the window's
/// app, the review and the window.
fn inspected_commit(
    cx: &mut gpui_kit::TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    Entity<SeriesBrowser>,
    &mut gpui_kit::VisualTestContext,
) {
    use crate::focus_ring::sweep;
    use crate::tags::tests::{draw, git, set_interface_text_size, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let notes = repo.path().join("notes.txt");
    for (text, message) in [
        ("first\n", "Add notes"),
        ("second\nthird\n", "Rewrite notes"),
    ] {
        std::fs::write(&notes, text).unwrap();
        git(repo.path(), &["add", "notes.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", message]);
    }
    // A fresh repository keeps its branch as a loose reference.
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    let files = repo.changes_with_renames(head.trim(), 0).unwrap();
    let (app, cx) = window(cx, &repo);
    sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    set_interface_text_size(cx, &app, 18);
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.inspecting = true;
            browser.inspect_label = "Rewritten".into();
            browser.files = files;
            browser.file(0, window, cx);
        })
    });
    for _ in 0..400 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        draw(cx);
        if browser.read_with(cx, |browser, _| browser.content.is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        browser.read_with(cx, |browser, _| browser.content.is_some()),
        "the inspected file's preview lands"
    );
    draw(cx);
    (fixture, app, browser, cx)
}

/// What holds focus in the inspected commit, for a failure to name.
fn focus_name(browser: &Entity<SeriesBrowser>, cx: &mut gpui_kit::VisualTestContext) -> String {
    cx.update(|window, cx| {
        let browser = browser.read(cx);
        let Some(focused) = window.focused(cx) else {
            return "nothing".into();
        };
        if focused == browser.back {
            return "Back to series".into();
        }
        if browser.text_focus(cx).contains(&focused) {
            return "the preview's editor".into();
        }
        for (id, name) in [
            ("series-files", "the file list's own handle"),
            ("series-file-preview", "the preview's own handle"),
        ] {
            if focused == browser.body.focus(id, cx) {
                return name.into();
            }
        }
        format!("{focused:?}")
    })
}

/// The outlines the last frame painted in `color` of the palette, by
/// their bounds.
fn outlines_in(
    cx: &mut gpui_kit::VisualTestContext,
    color: impl Fn(&appearance::Palette) -> u32,
) -> Vec<Bounds<Pixels>> {
    let color: Hsla = cx.read(|cx| rgb(color(&palette(cx))).into());
    crate::focus_ring::painted_outlines(cx, color)
        .into_iter()
        .map(|outline| outline.bounds)
        .collect()
}

/// While a commit is inspected at 18 pt in a 461 × 490 window, its file
/// list and its preview lie past the body's fold: Tab and Shift+Tab onto
/// a file's row and into the preview reveal each, in Patch and in After
/// source, and every stop draws a whole ring inside the body, the editor,
/// which draws none of its own, on the preview's 1 px edge. Neither was
/// revealed, so Tab past the text modes landed below the fold with no
/// ring; the list cut its rows' rings on every side, After source ran
/// past the body's right edge, and the editor, focused by Tab, drew no
/// ring at all. A click in the editor focuses it unmarked, the preview's
/// edge in the border's colour, as a click leaves every list.
#[gpui::test]
async fn an_inspected_commits_files_and_preview_scroll_into_view(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::focus_ring::sweep;
    use crate::tags::tests::draw;
    let (_fixture, app, browser, cx) = inspected_commit(cx);
    let mut failures = Vec::new();
    for (mode, name) in [(0, "Patch"), (2, "After source")] {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = mode;
                browser.prepare_editor(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        for key in ["tab", "shift-tab"] {
            let mut reached = Vec::new();
            let mut edged = false;
            let mut walked = Vec::new();
            sweep::walk_each(
                &app,
                cx,
                key,
                &format!("the inspected commit's {name} by {key}"),
                18,
                &mut walked,
                &mut |cx, stop, rings| {
                    let (inside, text) = cx.update(|window, cx| {
                        let browser = browser.read(cx);
                        let inside = ["series-files", "series-file-preview"]
                            .into_iter()
                            .filter(|id| browser.body.focus(*id, cx).contains_focused(window, cx))
                            .collect::<Vec<_>>();
                        let text = browser
                            .text_focus(cx)
                            .iter()
                            .any(|focus| focus.is_focused(window));
                        (inside, text)
                    });
                    reached.extend(inside.iter().copied());
                    // The editor, which draws no ring of its own, rings
                    // the preview's 1 px edge, which the walk finds whole.
                    if text {
                        edged = true;
                        let preview = cx.debug_bounds("series-file-preview").expect("the preview");
                        let drawn: Vec<_> = rings.iter().map(|ring| ring.bounds).collect();
                        if !sweep::rings_edge(&drawn, preview) {
                            failures.push(format!(
                                "{stop}: the editor draws no ring on the preview's edge, \
                                 {preview:?}, only {drawn:?}"
                            ));
                        }
                    }
                },
            );
            failures.extend(walked);
            for id in ["series-files", "series-file-preview"] {
                if !reached.contains(&id) {
                    failures.push(format!(
                        "{name} by {key} never focused a control inside {id}"
                    ));
                }
            }
            if !edged {
                failures.push(format!(
                    "{name} by {key} never focused the preview's editor"
                ));
            }
        }
    }
    // Tab onto the source's editor reveals the preview; with the window
    // blurred, a click in the part of it the body shows focuses the
    // editor again, unmarked until a key.
    let editor_focused = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            browser
                .read(cx)
                .text_focus(cx)
                .iter()
                .any(|focus| focus.is_focused(window))
        })
    };
    for _ in 0..40 {
        if editor_focused(cx) {
            break;
        }
        cx.simulate_keystrokes("tab");
        draw(cx);
    }
    cx.update(|window, cx| window.blur(cx));
    draw(cx);
    let preview = cx.debug_bounds("series-file-preview").expect("the preview");
    let body = cx.debug_bounds("rewrite-review-body").expect("the body");
    let shown = preview.intersect(&body);
    let clicked = shown.size.height > px(20.);
    if clicked {
        cx.simulate_click(shown.center(), Modifiers::none());
        draw(cx);
    }
    let focused = clicked && editor_focused(cx);
    let preview = cx.debug_bounds("series-file-preview").expect("the preview");
    if !clicked {
        failures.push(format!(
            "Tab onto the source's editor leaves the preview, {preview:?}, outside the body, \
             {body:?}, for a click"
        ));
    } else if !focused {
        failures.push(format!(
            "a click in the source leaves focus on {}, not its editor",
            focus_name(&browser, cx)
        ));
    } else if sweep::rings_edge(&outlines_in(cx, |p| p.accent), preview) {
        failures.push(format!(
            "a click in the source rings the preview's edge, {preview:?}, in the accent"
        ));
    } else if !sweep::rings_edge(&outlines_in(cx, |p| p.border), preview) {
        failures.push(format!(
            "after a click in the source the preview's edge, {preview:?}, is not the border's \
             colour"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Review messages with both commits shows two editors side by side, at
/// 18 pt in a 461 × 490 window: Tab and Shift+Tab onto either ring that
/// message's text below its heading, never the other's or the whole
/// preview, as Split rings its focused side; one ring around the whole
/// preview for either told nothing as Tab moved between them. A click in
/// a message's text focuses its editor unmarked.
#[gpui::test]
async fn review_messages_ring_the_focused_messages_text(cx: &mut gpui_kit::TestAppContext) {
    use crate::focus_ring::sweep;
    use crate::tags::tests::draw;
    const TEXTS: [&str; 2] = ["series-message-0", "series-message-1"];
    let (_fixture, app, browser, cx) = inspected_commit(cx);
    // As Review messages opens them (`inspect_messages`).
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.clear_content();
            browser.back.focus(window, cx);
            browser.inspect_label = "Original and rewritten commit messages".into();
            browser.messages = Some(
                [
                    "Add notes\n\nThe first notes.",
                    "Rewrite notes\n\nThe second notes.",
                ]
                .map(|message| Some(text::editor(message, "text", None, window, cx))),
            );
            cx.notify();
        })
    });
    draw(cx);
    let editors = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            let browser = browser.read(cx);
            let messages = browser.messages.as_ref().expect("both messages");
            [0, 1].map(|side| {
                messages[side]
                    .as_ref()
                    .is_some_and(|editor| editor.read(cx).focus_handle(cx).is_focused(window))
            })
        })
    };
    let mut failures = Vec::new();
    for key in ["tab", "shift-tab"] {
        let mut reached = [false; 2];
        let mut walked = Vec::new();
        sweep::walk_each(
            &app,
            cx,
            key,
            &format!("the commit messages by {key}"),
            18,
            &mut walked,
            &mut |cx, stop, rings| {
                let focused = editors(cx);
                let drawn: Vec<_> = rings.iter().map(|ring| ring.bounds).collect();
                for side in 0..2 {
                    let text = cx.debug_bounds(TEXTS[side]).expect("the message's text");
                    let ringed = sweep::rings_edge(&drawn, text);
                    if focused[side] {
                        reached[side] = true;
                        if !ringed {
                            failures.push(format!(
                                "{stop}: message {side}'s editor draws no ring on its text's \
                                 edge, {text:?}, only {drawn:?}"
                            ));
                        }
                    } else if ringed {
                        failures.push(format!(
                            "{stop}: message {side}'s text rings without its editor's focus"
                        ));
                    }
                }
                let preview = cx.debug_bounds("series-file-preview").expect("the preview");
                if sweep::rings_edge(&drawn, preview) {
                    failures.push(format!("{stop}: the whole preview rings, {preview:?}"));
                }
            },
        );
        failures.extend(walked);
        for (side, reached) in reached.into_iter().enumerate() {
            if !reached {
                failures.push(format!("{key} never focused message {side}'s editor"));
            }
        }
    }
    let text = cx
        .debug_bounds(TEXTS[1])
        .expect("the rewritten message's text");
    cx.simulate_click(text.center(), Modifiers::none());
    draw(cx);
    let text = cx
        .debug_bounds(TEXTS[1])
        .expect("the rewritten message's text");
    if !editors(cx)[1] {
        failures.push("a click in the rewritten message leaves its editor unfocused".into());
    } else if sweep::rings_edge(&outlines_in(cx, |p| p.accent), text) {
        failures.push(format!(
            "a click in the rewritten message rings its text, {text:?}, in the accent"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A press on the inspected commit's file list or preview beside their
/// controls, as on the list's room below its rows or on the preview's
/// "Select a file…" line, at 13 pt in a 1280 × 800 window, leaves focus
/// where it was, and the next key shows its ring there: both track a
/// handle no Tab stops on, which such a press focused, so the next key
/// showed no ring anywhere.
#[gpui::test]
async fn a_press_beside_the_inspected_commits_controls_keeps_focus(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, set_interface_text_size};
    let (_fixture, app, browser, cx) = inspected_commit(cx);
    cx.simulate_resize(size(px(1280.), px(800.)));
    set_interface_text_size(cx, &app, 13);
    cx.update(|_, cx| browser.update(cx, |browser, _| browser.clear_content()));
    let back = cx.update(|window, cx| {
        let back = browser.read(cx).back.clone();
        back.focus(window, cx);
        app.read(cx).focus_ring.mark(window);
        back
    });
    draw(cx);
    let list = cx.debug_bounds("series-files").expect("the file list");
    let preview = cx.debug_bounds("series-file-preview").expect("the preview");
    let mut failures = Vec::new();
    for (place, at) in [
        (
            "the file list's room below its rows",
            point(list.center().x, list.bottom() - px(6.)),
        ),
        (
            "the preview's line",
            point(preview.left() + px(30.), preview.top() + px(20.)),
        ),
    ] {
        cx.simulate_click(at, Modifiers::none());
        draw(cx);
        cx.simulate_keystrokes("down");
        draw(cx);
        let (kept, ringed) = cx.update(|window, cx| {
            (
                back.is_focused(window),
                app.read(cx).focus_ring.shows(&back),
            )
        });
        if !kept || !ringed {
            failures.push(format!(
                "a press on {place} at {at:?} and a key leave focus on {}, its ring shown: \
                 {ringed}",
                focus_name(&browser, cx)
            ));
            // The next press starts from Back to series again.
            cx.update(|window, cx| back.focus(window, cx));
            draw(cx);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An inspected commit's document, in the series review's file box, which
/// keeps 100 px, says why a click on Open in default app opened nothing on
/// one whole line beneath its header's controls, inside the box and the
/// body's view, its whole answer in the line's tooltip, and the page
/// beneath gives way below its minimum for it, at 13 and 18 pt in a
/// 461 × 490 window: the room beneath the header held no line above that
/// minimum, so the line drew none and the click changed nothing on
/// screen. With the text modes' row kept above every preview, the line
/// lay 25.5 px below the view at 13 pt while the body stopped at the
/// window less 250 px, short of the room the dialog's cap leaves it.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn an_inspected_document_says_why_on_one_whole_line(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pdf =
        |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
    let document = repo.path().join("document.pdf");
    for (text, message) in [("first", "Add a document"), ("second", "Rewrite it")] {
        std::fs::write(&document, pdf(text)).unwrap();
        git(repo.path(), &["add", "document.pdf"]);
        git(repo.path(), &["commit", "--quiet", "-m", message]);
    }
    // A fresh repository keeps its branch as a loose reference.
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    let files = repo.changes_with_renames(head.trim(), 0).unwrap();
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    for text in [13, 18] {
        let at = format!("the series review at {text} pt in 461 × 490");
        size_to(&app, cx, text, 461., 490.);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        // The rewritten commit's files, as Inspect rewritten files lists
        // them, and its document chosen.
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files.clone();
                browser.file(0, window, cx);
            })
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("system-preview-1").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Where the document's controls lie below the body's view, a
        // person scrolls the body to them before pressing one.
        let body = cx.debug_bounds("rewrite-review-body").expect("the body");
        let control = crate::tags::tests::rendered(cx, "system-preview-1");
        if !lies_in(control, body) {
            crate::tags::tests::scroll_down(
                cx,
                body.origin + point(px(4.), px(4.)),
                gpui::ScrollDelta::Pixels(point(px(0.), -body.size.height)),
            );
        }
        crate::system_preview::tests::a_failed_open_answers_on_one_line(
            cx,
            &at,
            [
                "system-preview-1",
                "system-preview-1/note",
                "rich-sides/body",
            ],
            &["series-file-preview", "rewrite-review-body"],
        );
        cx.update(|window, cx| window.close_dialog(cx));
        draw(cx);
    }
}

/// A click on an inspected document's Open in default app whose control
/// a person scrolled to the body's foot, so only the controls' row of
/// the preview shows, answers where it is seen: the line saying why
/// nothing opened, which lands below the view, is revealed whole by the
/// body's least scroll, with its ring's room, and focus stays on the
/// control, at 18 pt in a 461 × 490 window, where the preview lies below
/// the view as the inspector opens. The line's bounds reached only
/// tests, and it answered out of sight.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn a_failed_open_at_the_bodys_foot_is_revealed(cx: &mut gpui_kit::TestAppContext) {
    use crate::system_preview::tests::{queue, seen_as_svg};
    use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pdf =
        |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
    let document = repo.path().join("document.pdf");
    for (text, message) in [("first", "Add a document"), ("second", "Rewrite it")] {
        std::fs::write(&document, pdf(text)).unwrap();
        git(repo.path(), &["add", "document.pdf"]);
        git(repo.path(), &["commit", "--quiet", "-m", message]);
    }
    // A fresh repository keeps its branch as a loose reference.
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    let files = repo.changes_with_renames(head.trim(), 0).unwrap();
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    {
        let text = 18;
        let at = format!("the series review at {text} pt in 461 × 490");
        size_to(&app, cx, text, 461., 490.);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files.clone();
                browser.file(0, window, cx);
            })
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("system-preview-1").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // The body scrolled until the controls' row, with its ring's
        // room, ends at the view's foot.
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let scroll = browser.read_with(cx, |browser, _| browser.body.scroll().clone());
        let control = rendered(cx, "system-preview-1");
        let body = rendered(cx, "rewrite-review-body");
        let offset = scroll.offset().y - (control.bottom() + room - body.bottom());
        assert!(
            offset <= px(0.) && -offset <= scroll.max_offset().y,
            "{at}: the body can scroll the control {control:?} to its foot {body:?}"
        );
        scroll.set_offset(point(px(0.), offset));
        draw(cx);
        let (control, body) = (
            rendered(cx, "system-preview-1"),
            rendered(cx, "rewrite-review-body"),
        );
        assert!(
            (control.bottom() + room - body.bottom()).abs() < px(0.5),
            "{at}: the control {control:?} ends the body's view {body:?}"
        );
        queue(Err(seen_as_svg("a PDF document")));
        cx.simulate_click(control.center(), Modifiers::none());
        let pressed = cx.update(|window, cx| window.focused(cx));
        cx.run_until_parked();
        draw(cx);
        let line = rendered(cx, "system-preview-1/note");
        let body = rendered(cx, "rewrite-review-body");
        assert!(
            line.top() >= body.top() + room - px(0.5)
                && line.bottom() + room <= body.bottom() + px(0.5),
            "{at}: the line {line:?} lies whole in the body's view {body:?}, with its \
             ring's room"
        );
        let control = rendered(cx, "system-preview-1");
        assert!(
            lies_in(control, body),
            "{at}: the control {control:?} stays in the body's view {body:?}"
        );
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            pressed,
            "{at}: focus stays where the press put it"
        );
        cx.update(|window, cx| window.close_dialog(cx));
        draw(cx);
    }
}

/// A model the series review shows, an inspected commit's 3D file,
/// focused from the keyboard keeps its canvas's accent ring while the
/// pointer moves over it and away, at 13 pt in a 1,480 × 980 window, as
/// Compare's does: the review's opener carries the window's focus ring
/// (`Opener::ringed_by`), which the canvas asks.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn an_inspected_models_canvas_keeps_its_ring_while_the_pointer_moves(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
    let shape = repo.path().join("shape.obj");
    for (scale, message) in [("1", "Add a model"), ("2", "Grow it")] {
        std::fs::write(&shape, model(scale)).unwrap();
        git(repo.path(), &["add", "shape.obj"]);
        git(repo.path(), &["commit", "--quiet", "-m", message]);
    }
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    let files = repo.changes_with_renames(head.trim(), 0).unwrap();
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(1480.), px(980.)));
    draw(cx);
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.inspecting = true;
            browser.inspect_label = "Rewritten".into();
            browser.files = files;
            browser.file(0, window, cx);
        })
    });
    for _ in 0..400 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        draw(cx);
        if cx.debug_bounds("model-canvas-1").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    crate::focus_ring::sweep::prepare(&app, cx);
    crate::model_view::tests::assert_the_canvas_keeps_its_ring(
        cx,
        "model-canvas-1",
        "the series review",
    );
}

/// An inspected document's comparison takes its open controls' form
/// from its box's width in the frame it is drawn ([`crate::in_frame`]):
/// every frame drawn as the window narrows and widens between 461 and
/// 1,481 px at 13 and 18 pt lays the comparison out as the frame it
/// settles in, and the walk meets After's control labelled and as its
/// icon. Built from the box's width a frame before, the first frame after
/// a resize drew the form of the width before.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn an_inspected_comparison_takes_its_form_in_the_frame_it_is_drawn(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pdf =
        |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
    let document = repo.path().join("document.pdf");
    for (text, message) in [("first", "Add a document"), ("second", "Rewrite it")] {
        std::fs::write(&document, pdf(text)).unwrap();
        git(repo.path(), &["add", "document.pdf"]);
        git(repo.path(), &["commit", "--quiet", "-m", message]);
    }
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    let files = repo.changes_with_renames(head.trim(), 0).unwrap();
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(1481.), px(980.)));
    draw(cx);
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.inspecting = true;
            browser.inspect_label = "Rewritten".into();
            browser.files = files;
            browser.file(0, window, cx);
        })
    });
    for _ in 0..400 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        draw(cx);
        if cx.debug_bounds("system-preview-1").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let met = crate::system_preview::tests::open_controls_take_their_form_in_the_frame(
        cx,
        "the series review",
        "rewrite-comparison",
    );
    assert_eq!(
        met.into_iter().collect::<Vec<_>>(),
        ["icon", "labelled"],
        "the walk meets both forms"
    );
}

/// A rewritten commit's 3D model and PDF, in the series review's file
/// box, which keeps 100 px at the interface size, keep each side's own
/// header at its floor, one control whole with the larger of its ring's
/// room and the header's padding above and below, beneath which the
/// model or page gives way, and Tab reveals each of the header's
/// controls whole with its ring there, in a 461 × 490 window, at 13 and
/// 18 pt. The floor once counted the ring's room and the padding
/// together, about 6 px more than either needed. The box once kept an
/// unscaled 100 px, as the error box beneath it did, which at 18 pt left
/// the model's side 44 px, less than its header's floor and rule, so the
/// header took the side and the box's clip cut its controls' rings.
#[cfg(target_os = "linux")]
#[gpui::test]
async fn an_inspected_side_header_keeps_its_floor_in_the_file_box(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::focus_reveal::tests::control;
    use crate::system_preview::tests::{Shown, assert_a_header_keeps_its_floor};
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pdf =
        |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
    let document = repo.path().join("document.pdf");
    let model = repo.path().join("shape.obj");
    for (text, scale, message) in [("first", "2", "Add them"), ("second", "1", "Rewrite them")] {
        std::fs::write(&document, pdf(text)).unwrap();
        std::fs::write(
            &model,
            format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n"),
        )
        .unwrap();
        git(repo.path(), &["add", "document.pdf", "shape.obj"]);
        git(repo.path(), &["commit", "--quiet", "-m", message]);
    }
    // A fresh repository keeps its branch as a loose reference.
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    let files = repo.changes_with_renames(head.trim(), 0).unwrap();
    let index = |path: &str| {
        files
            .iter()
            .position(|file| file.path() == std::path::Path::new(path))
            .expect("the commit changes it")
    };
    let (document, model) = (index("document.pdf"), index("shape.obj"));
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    cx.update(|_, cx| {
        browser.update(cx, |browser, _| {
            browser.inspecting = true;
            browser.inspect_label = "Rewritten".into();
            browser.files = files;
        })
    });
    let open = |cx: &mut gpui_kit::VisualTestContext, index: usize, selector: &'static str| {
        cx.update(|window, cx| browser.update(cx, |browser, cx| browser.file(index, window, cx)));
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds(selector).is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("{selector} never drawn");
    };
    let view = browser.entity_id();
    let mut cameras = vec![control("model-external", 1)];
    cameras.extend((5..10).map(|index| control("model-camera", index)));
    cameras.extend([control("model-views", 1), control("model-link", 1)]);
    open(cx, model, "model-side-1/header");
    for (text, floor) in [(13, 34.), (18, 48.)] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        draw(cx);
        assert_a_header_keeps_its_floor(
            cx,
            &format!("the model's After at {text} pt"),
            (view, "model-side-1"),
            (px(floor), Shown::Floor),
            &control("model-source", 1),
            &cameras,
        );
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    draw(cx);
    open(cx, document, "system-preview-1");
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
    // At 13 pt the narrowest form's run takes a line beneath its
    // heading; at 18 pt it wraps to a second, the "more" menu on its own.
    for (text, floor, shown) in [(13, 41., Shown::FloorOneLine), (18, 57., Shown::Floor)] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        draw(cx);
        for side in [0, 1] {
            // The narrowest header a narrow box's side takes, its run on
            // a line beneath its heading, which scrolls at its floor,
            // from the page number (Previous is unavailable on the first
            // page).
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
                "{name} at {text} pt"
            );
            assert_a_header_keeps_its_floor(
                cx,
                &format!("{name} at {text} pt"),
                (view, name),
                (px(floor), shown),
                &control("pdf-page-number", side),
                &controls,
            );
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The rewritten series' review, opened with the app's series keys on a
/// disposable repository of three commits whose last two a rebase's
/// start captured, as it captures them, before `rewrite` (Git argument
/// lists, run in order) rewrote them, its series read.
async fn rewritten_review<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    rewrite: &[&[&str]],
) -> (
    tempfile::TempDir,
    Entity<SeriesBrowser>,
    &'a mut gpui::VisualTestContext,
) {
    use crate::focus_ring::sweep::Opened as _;
    use crate::tags::tests::{draw, git};
    use gitturtle_core::InteractiveRebaseCommand;
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 2))
            .await;
    // The review opens at rest, so its geometry is read where it stays.
    cx.update(|_, cx| {
        crate::interactive_rebase::init(cx);
        cx.set_reduce_motion(true);
    });
    let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
    git(repo.path(), &["reset", "--quiet", "--hard", "main"]);
    let plan = repo.interactive_rebase_plan("main~2").unwrap();
    let steps = plan.steps();
    let command = WriteCommand::InteractiveRebase(Arc::new(InteractiveRebaseCommand::Start {
        plan,
        steps,
        acknowledge_published: false,
    }));
    let mut saved = app
        .update(cx, |app, _| app.prepare_rewrite_capture(&command))
        .expect("a rebase start captures its series");
    app.read_with(cx, |app, _| app.preferences_writer.drain());
    assert!(
        matches!(saved.try_recv(), Ok(Some(Ok(())))),
        "the series is captured"
    );
    for args in rewrite {
        git(repo.path(), args);
    }
    let review =
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_series_review(window, cx)));
    review.clone().finish(cx);
    let review = review.expect("the review opens");
    draw(cx);
    (fixture, review, cx)
}

/// A key-down of Enter, not marked held, with no key-up after it: a
/// fresh press, or under X11 each repeat of a held one; with `down`
/// false, its key-up.
fn enter(cx: &mut gpui::VisualTestContext, down: bool) {
    let keystroke = gpui::Keystroke::parse("enter").unwrap();
    if down {
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        });
    } else {
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
    }
    crate::tags::tests::draw(cx);
}

/// Return on a commit row of a rewritten series inspects that commit's
/// files, as Return on a History commit loads its files, and the review
/// stays open: the review dialog took the key for its default action,
/// which closed the review before the series list's own Return was
/// reached. Back to series takes the keys from the list the inspector
/// replaces, so Escape goes back to the series, keeping the row, and
/// closes the review from there: left on the list, which is no longer
/// drawn, focus sent Escape to the window's root, where the review never
/// saw it. Driven by the route a person takes: a
/// series captured before its rewrite, the review opened on it, the row
/// chosen with Down and Return and Escape dispatched as keys.
#[gpui::test]
async fn return_on_a_series_row_inspects_its_files_and_escape_closes_the_review(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::draw;
    // The newest commit reworded.
    let (_fixture, review, cx) = rewritten_review(
        cx,
        &[&[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "fixture notes 2, reworded",
        ]],
    )
    .await;
    let rows = review.read_with(cx, |review, _| {
        review.review.as_ref().map_or(0, |review| review.rows.len())
    });
    assert_eq!(rows, 2, "the rewritten series' two commits");
    assert!(
        cx.update(|window, cx| review.read(cx).focus.is_focused(window)),
        "the loaded series takes the keys"
    );

    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(review.read_with(cx, |review, _| review.selected), 1);
    cx.simulate_keystrokes("enter");
    review.read_with(cx, |review, _| review.worker.wait_idle());
    draw(cx);
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "Return leaves the review open"
    );
    let (inspecting, label, files) = review.read_with(cx, |review, _| {
        (
            review.inspecting,
            review.inspect_label.clone(),
            review
                .files
                .iter()
                .map(|file| file.path().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        )
    });
    assert!(inspecting, "Return inspects the selected commit");
    assert!(
        cx.update(|window, cx| review.read(cx).back.is_focused(window)),
        "Back to series takes the keys from the series list it replaces"
    );
    assert!(
        label.starts_with("Rewritten ") && label.ends_with("· fixture notes 2, reworded"),
        "the selected row's rewritten commit: {label:?}"
    );
    assert_eq!(files, ["notes.txt"], "its changed files are listed");
    assert!(
        cx.debug_bounds("series-files").is_some(),
        "the inspector draws its file list"
    );

    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "Escape in the inspector goes back to the series"
    );
    assert_eq!(
        review.read_with(cx, |review, _| (review.inspecting, review.selected)),
        (false, 1),
        "the series keeps its row"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "Escape on the series closes the review"
    );
}

/// A held Return on a series row inspects once. Under X11 a held key's
/// repeats arrive as fresh presses, not marked held: the first inspected
/// and moved focus to Back to series, the next pressed it, which focused
/// the list again, the next inspected again, so the review toggled and
/// started and cancelled a changed-files read with every repeat. Back to
/// series acts on a press of its own, once the held one comes up.
#[gpui::test]
async fn a_held_return_on_a_series_row_inspects_once(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let (_fixture, review, cx) = rewritten_review(
        cx,
        &[&[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "fixture notes 2, reworded",
        ]],
    )
    .await;
    enter(cx, true);
    let generation = review.read_with(cx, |review, _| review.generation);
    assert!(
        review.read_with(cx, |review, _| review.inspecting),
        "the press inspects"
    );
    for repeat in 1..=4 {
        enter(cx, true);
        assert_eq!(
            review.read_with(cx, |review, _| (review.inspecting, review.generation)),
            (true, generation),
            "repeat {repeat} of the held Return leaves the inspection as it is"
        );
    }
    enter(cx, false);
    review.read_with(cx, |review, _| review.worker.wait_idle());
    draw(cx);
    assert_eq!(
        review.read_with(cx, |review, _| review.files.len()),
        1,
        "the one read lands"
    );
    enter(cx, true);
    assert!(
        !review.read_with(cx, |review, _| review.inspecting),
        "a press of its own on Back to series goes back to the series"
    );
    enter(cx, true);
    assert!(
        !review.read_with(cx, |review, _| review.inspecting),
        "and its repeats on the series list inspect nothing"
    );
}

/// Return on a row whose commit the rewrite dropped, which has only its
/// original side, inspects that original commit's files: it inspected
/// only a rewritten side, so Return there did nothing.
#[gpui::test]
async fn return_on_a_dropped_commits_row_inspects_its_original(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    // The newest commit dropped.
    let (_fixture, review, cx) =
        rewritten_review(cx, &[&["reset", "--quiet", "--hard", "main~1"]]).await;
    let dropped = review.read_with(cx, |review, _| {
        review.review.as_ref().and_then(|review| {
            review
                .rows
                .iter()
                .position(|row| row.rewritten.is_none() && row.original.is_some())
        })
    });
    let dropped = dropped.expect("a row with only its original side");
    for _ in 0..dropped {
        cx.simulate_keystrokes("down");
    }
    draw(cx);
    assert_eq!(review.read_with(cx, |review, _| review.selected), dropped);
    cx.simulate_keystrokes("enter");
    review.read_with(cx, |review, _| review.worker.wait_idle());
    draw(cx);
    let (inspecting, label, files) = review.read_with(cx, |review, _| {
        (
            review.inspecting,
            review.inspect_label.clone(),
            review
                .files
                .iter()
                .map(|file| file.path().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        )
    });
    assert!(inspecting, "Return inspects the dropped commit");
    assert!(
        label.starts_with("Original ") && label.ends_with("· fixture notes 2"),
        "its original side: {label:?}"
    );
    assert_eq!(files, ["notes.txt"], "its changed files are listed");
}

/// A `git fast-import` stream on `main`: a base commit and three after
/// it, each adding a file of its own, so a rebase that drops the second
/// replays the third without a conflict. The third adds `large.txt`,
/// `large` bytes of numbered lines, where `large` is not zero.
fn series_script(large: usize) -> String {
    let mut input = String::new();
    for commit in 0..4 {
        let (path, content) = if commit == 3 && large > 0 {
            let mut content = String::new();
            let mut line = 0;
            while content.len() < large {
                content.push_str(&format!("line {line}\n"));
                line += 1;
            }
            ("large.txt".to_owned(), content)
        } else {
            (format!("file-{commit}.txt"), format!("file {commit}\n"))
        };
        let message = format!("series commit {commit}");
        input.push_str(&format!(
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\nM 100644 inline {path}\ndata {}\n{content}\n",
            1_700_000_000 + commit,
            message.len(),
            content.len(),
        ));
    }
    input + "done\n"
}

/// The app on [`series_script`]'s repository, checked out, with dialogs
/// opening at rest.
async fn series_window(
    cx: &mut gpui_kit::TestAppContext,
    large: usize,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    &mut gpui::VisualTestContext,
) {
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, series_script(large)).await;
    cx.update(|_, cx| {
        crate::interactive_rebase::init(cx);
        cx.set_reduce_motion(true);
    });
    let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
    crate::tags::tests::git(repo.path(), &["reset", "--quiet", "--hard", "main"]);
    crate::views::tests::settle(&app, cx).await;
    (fixture, app, cx)
}

/// `text` pt in a `width` × `height` window.
fn size_to(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
    text: u8,
    width: f32,
    height: f32,
) {
    cx.simulate_resize(size(px(width), px(height)));
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings.interface_text_size = text;
            appearance::apply_text_sizes(text, app.settings.code_text_size, window, cx);
            cx.notify();
        })
    });
    crate::tags::tests::draw(cx);
}

/// Runs a rebase through Edit local commits as a person does: the form
/// opened on `base`, Load commits pressed, the second commit dropped
/// with Down and D, Review rebase… pressed and the alert's Start rebase
/// pressed; returns once the rebase and the series' capture before it
/// have landed.
async fn rebase_dropping_the_second(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
    base: &str,
) {
    use crate::tags::tests::draw;
    let head = |cx: &mut gpui::VisualTestContext| {
        let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
        let output = std::process::Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    let before = head(cx);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_interactive_rebase(Some(base.into()), window, cx)
        })
    });
    draw(cx);
    let load = cx
        .debug_bounds("load-rebase-commits")
        .expect("Load commits");
    cx.simulate_click(load.center(), Modifiers::none());
    app.read_with(cx, |app, _| app.operations.drain());
    crate::views::tests::settle(app, cx).await;
    cx.simulate_keystrokes("down d");
    draw(cx);
    let review = cx
        .debug_bounds("review-interactive-rebase")
        .expect("Review rebase…");
    cx.simulate_click(review.center(), Modifiers::none());
    draw(cx);
    let start = cx
        .debug_bounds("dialog-ok: Start rebase")
        .expect("the alert's Start rebase");
    cx.simulate_click(start.center(), Modifiers::none());
    // The operation waits for the capture, saved on the preferences
    // thread, before Git starts.
    for _ in 0..4 {
        app.read_with(cx, |app, _| {
            app.preferences_writer.drain();
            app.operations.drain();
        });
        crate::views::tests::settle(app, cx).await;
    }
    assert!(
        app.read_with(cx, |app, _| app.operation_busy.is_none()),
        "the rebase has ended"
    );
    assert_ne!(head(cx), before, "the rebase rewrote main");
}

/// The review the palette's Review rewritten branch series… opens, its
/// read landed.
fn review_from_palette(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
) -> Entity<SeriesBrowser> {
    use crate::tags::tests::draw;
    let _ = take_opened();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
    draw(cx);
    cx.simulate_input("Review rewritten branch series");
    draw(cx);
    // Its Return goes down and comes up, as a person's does: the
    // review it opens takes no Return until that key-up.
    enter(cx, true);
    enter(cx, false);
    let review = take_opened().expect("the palette opens the review");
    lands(&review, cx);
    review
}

/// Lands the review's read.
fn lands(review: &Entity<SeriesBrowser>, cx: &mut gpui::VisualTestContext) {
    use crate::focus_ring::sweep::Opened as _;
    Some(review.clone()).finish(cx);
    crate::tags::tests::draw(cx);
}

/// Whether `inner` lies whole inside `outer`, to a quarter pixel.
fn lies_in(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    let slack = px(0.25);
    inner.left() >= outer.left() - slack
        && inner.top() >= outer.top() - slack
        && inner.right() <= outer.right() + slack
        && inner.bottom() <= outer.bottom() + slack
}

/// What the review says with no series to show: its line, its title
/// where it has one, and whether Edit local commits… stands beside it.
struct Says<'a> {
    line: &'a str,
    title: Option<&'a str>,
    edit: bool,
}

impl Says<'_> {
    /// The whole state, as its accessible description reads it.
    fn description(&self) -> String {
        self.title.map_or_else(
            || self.line.to_owned(),
            |title| format!("{title}. {}", self.line),
        )
    }
}

/// No rebase captured a series.
const UNCAPTURED: Says<'static> = Says {
    line: recovery_drafts::NO_CAPTURE,
    title: None,
    edit: true,
};

/// The review, with no series to show, says why in the list's place,
/// as `says` gives it: drawn as a designed empty state whole in the
/// list and in view once Tab moves onto the list, a failure's title
/// above its line, and read whole as the list's accessible description
/// and each disabled action's; Edit local commits… stands below the
/// line where no rebase captured a series, and Refresh series is
/// offered, Tab moving onto each in turn from the list. Returns with
/// Refresh series focused.
fn says_why(
    review: &Entity<SeriesBrowser>,
    cx: &mut gpui::VisualTestContext,
    says: &Says,
    at: &str,
) {
    let reason = says.line;
    let described = says.description();
    let described = described.as_str();
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    assert!(
        review.read_with(cx, |review, _| review
            .review
            .as_ref()
            .is_none_or(|review| review.rows.is_empty())),
        "{at}: no series to show"
    );
    // From the dialog's shell as it opens, Tab moves onto the list.
    if !cx.update(|window, cx| review.read(cx).focus.is_focused(window)) {
        cx.simulate_keystrokes("tab");
        draw(cx);
    }
    assert!(
        cx.update(|window, cx| review.read(cx).focus.is_focused(window)),
        "{at}: Tab moves onto the list"
    );
    assert_eq!(
        crate::empty_state::shown("series-empty").as_deref(),
        Some(reason),
        "{at}: the list's place says why"
    );
    let line = cx
        .debug_bounds("series-empty-line")
        .unwrap_or_else(|| panic!("{at}: the reason is drawn"));
    let list = cx
        .debug_bounds("series-focus-list")
        .unwrap_or_else(|| panic!("{at}: the list is drawn"));
    let body = cx
        .debug_bounds("rewrite-review-body")
        .unwrap_or_else(|| panic!("{at}: the body is drawn"));
    assert!(line.size.height > px(0.), "{at}: the reason has a line");
    assert!(
        lies_in(line, list),
        "{at}: the reason at {line:?} lies whole in the list, {list:?}"
    );
    assert!(
        lies_in(list, body),
        "{at}: the focused list at {list:?} lies whole in the body's view, {body:?}"
    );
    let title = cx.debug_bounds("series-empty-title");
    match says.title {
        Some(_) => {
            let title = title.unwrap_or_else(|| panic!("{at}: the failure's title is drawn"));
            assert!(
                lies_in(title, list) && title.bottom() <= line.top(),
                "{at}: the title at {title:?} lies in the list, {list:?}, above its line, \
                 {line:?}"
            );
            // Drawn as the app's title-and-explanation panes draw theirs
            // (`main::empty`): one 14 px line, its fixed `gap_2` above
            // its line.
            let (title_line, gap) = cx.update(|window, _| {
                let text = appearance::ui_text(crate::empty_state::TITLE_TEXT);
                (
                    window
                        .text_style()
                        .line_height
                        .to_pixels(text.into(), window.rem_size()),
                    window.rem_size() * 0.5,
                )
            });
            let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.5);
            assert!(
                near(title.size.height, title_line),
                "{at}: the title, {title:?}, is one 14 px line, {title_line:?} tall"
            );
            assert!(
                near(line.top() - title.bottom(), gap),
                "{at}: the title, {title:?}, stands {gap:?} above its line, {line:?}"
            );
        }
        None => assert!(title.is_none(), "{at}: the state has no title"),
    }
    let edit = cx.debug_bounds("series-edit-commits");
    assert_eq!(
        edit.is_some_and(|edit| lies_in(edit, list) && edit.top() >= line.bottom()),
        says.edit,
        "{at}: Edit local commits… stands below the line where no rebase captured a series: \
         {edit:?}"
    );
    let list_node = written::get("series-focus-list")
        .unwrap_or_else(|| panic!("{at}: the list's node is written"))
        .node;
    assert_eq!(
        list_node.description(),
        Some(described),
        "{at}: the list's accessible description is the reason"
    );
    for id in [
        "inspect-series-messages",
        "inspect-original-commit",
        "inspect-rewritten-commit",
        "inspect-publication-remote",
    ] {
        let node = written::get(id)
            .unwrap_or_else(|| panic!("{at}: {id}'s node is written"))
            .node;
        assert!(node.is_disabled(), "{at}: {id} waits for a series");
        assert_eq!(
            node.description(),
            Some(described),
            "{at}: {id} says why it is unavailable"
        );
    }
    if says.edit {
        let node = written::get("series-edit-commits")
            .unwrap_or_else(|| panic!("{at}: Edit local commits…' node is written"))
            .node;
        assert!(!node.is_disabled(), "{at}: Edit local commits… is offered");
        cx.simulate_keystrokes("tab");
        draw(cx);
        let edit_focus = cx.update(|_, cx| review.read(cx).body.focus("series-edit-commits", cx));
        assert!(
            cx.update(|window, _| edit_focus.is_focused(window)),
            "{at}: Tab moves from the list onto Edit local commits…"
        );
        // Its ring whole inside every clip around it, and in the body's
        // view.
        crate::tags::tests::assert_rings_whole(cx, &["series-edit-commits"]);
        let edit = cx
            .debug_bounds("series-edit-commits")
            .unwrap_or_else(|| panic!("{at}: Edit local commits… is drawn"));
        assert!(
            lies_in(edit, body),
            "{at}: Edit local commits… at {edit:?} lies in the body's view, {body:?}"
        );
    }
    let reload = written::get("reload-series")
        .unwrap_or_else(|| panic!("{at}: Refresh series' node is written"))
        .node;
    assert!(!reload.is_disabled(), "{at}: Refresh series is offered");
    assert_eq!(
        reload.description(),
        None,
        "{at}: Refresh series is offered"
    );
    cx.simulate_keystrokes("tab");
    draw(cx);
    assert!(
        cx.update(|window, cx| {
            let review = review.read(cx);
            review.body.focus("reload-series", cx).is_focused(window)
        }),
        "{at}: Tab moves from the list onto Refresh series"
    );
}

/// Presses Refresh series with Return, as [`says_why`] leaves it
/// focused, and lands the read it starts, which the review's thread
/// holds for a frame: while it reads, and once it has read, Refresh
/// series stays where it was, the state keeping its height. It keeps
/// focus, and Shift+Tab, past the Buttons that wait for a series and
/// Edit local commits… where `says` offers it, moves back onto the list.
fn refresh(
    review: &Entity<SeriesBrowser>,
    cx: &mut gpui::VisualTestContext,
    says: &Says,
    at: &str,
) {
    let read = review.read_with(cx, |review, _| review.generation);
    let place = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("reload-series")
            .unwrap_or_else(|| panic!("{at}: Refresh series is drawn"))
            .top()
    };
    let before = place(cx);
    let state = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("series-focus-list")
            .unwrap_or_else(|| panic!("{at}: the list is drawn"))
            .size
            .height
    };
    let kept = state(cx);
    let (release, held) = std::sync::mpsc::channel::<()>();
    review.read_with(cx, |review, _| {
        drop(review.metadata.submit(move || {
            let _ = held.recv();
            Ok(())
        }))
    });
    enter(cx, true);
    enter(cx, false);
    assert!(
        review.read_with(cx, |review, _| review.pending),
        "{at}: Return on Refresh series reads the capture again"
    );
    assert_eq!(
        crate::empty_state::shown("series-empty").as_deref(),
        Some(READING),
        "{at}: the list's place says the read is under way"
    );
    assert_eq!(
        state(cx),
        kept,
        "{at}: the state keeps the height it took at rest while it reads"
    );
    assert_eq!(
        place(cx),
        before,
        "{at}: Refresh series stays put while it reads"
    );
    drop(release);
    lands(review, cx);
    assert_ne!(
        review.read_with(cx, |review, _| review.generation),
        read,
        "{at}: Refresh series reads the capture again"
    );
    assert_eq!(
        place(cx),
        before,
        "{at}: Refresh series stays put once read"
    );
    cx.simulate_keystrokes(if says.edit {
        "shift-tab shift-tab"
    } else {
        "shift-tab"
    });
    crate::tags::tests::draw(cx);
}

/// The palette's Review rewritten branch series… in a repository with
/// no captured rebase says so in the list's place, at 13 pt in a 1,480 ×
/// 973 window and at 18 pt in a 461 × 490 half tile; QA-106e saw an
/// empty list, disabled Buttons and no text: the load's error was drawn
/// at the body's end in a scrolling box the body shrank to nothing.
/// Refresh series, pressed with Return, reads again and says so again,
/// moving nothing. Edit local commits…, offered beside the line, is
/// refused with the menu's reason while a write runs, and otherwise
/// opens the rebase form in the review's place, as the menu's item does.
#[gpui::test]
async fn the_palettes_review_says_why_it_has_no_series(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, app, cx) = series_window(cx, 0).await;
    // The app's focus ring, which `says_why` finds whole.
    crate::focus_ring::sweep::prepare(&app, cx);
    for (text, width, height) in [(13, 1480., 973.), (18, 461., 490.)] {
        let at = format!("{text} pt in {width} × {height}");
        size_to(&app, cx, text, width, height);
        let review = review_from_palette(&app, cx);
        says_why(&review, cx, &UNCAPTURED, &at);
        refresh(&review, cx, &UNCAPTURED, &at);
        says_why(&review, cx, &UNCAPTURED, &format!("{at}, refreshed"));
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "{at}: Escape closes the review"
        );
    }

    let _review = review_from_palette(&app, cx);
    app.update(cx, |app, _| app.operation_busy = Some("Fetching…"));
    draw(cx);
    let busy = app.read_with(cx, |app, _| app.busy_reason());
    let node = written::get("series-edit-commits")
        .expect("Edit local commits…' node is written")
        .node;
    assert!(node.is_disabled(), "a write refuses Edit local commits…");
    assert_eq!(
        node.description(),
        Some(busy),
        "Edit local commits… says why with the menu's reason"
    );
    app.update(cx, |app, _| app.operation_busy = None);
    draw(cx);
    // Tab moves onto the list, then onto the action, revealing it.
    cx.simulate_keystrokes("tab tab");
    draw(cx);
    enter(cx, true);
    enter(cx, false);
    assert!(
        cx.debug_bounds("load-rebase-commits").is_some()
            && cx.debug_bounds("series-focus-list").is_none(),
        "Edit local commits… opens the rebase form in the review's place"
    );
}

/// The palette's review opens with its first read holding the room of
/// the state it most likely lands in, no rebase captured, with Edit
/// local commits… below its line, at 18 pt in a 461 × 490 half tile:
/// where it lands there, the actions below the list stand where they
/// settle in every frame painted from open to settled (kept from nothing,
/// the reading line took one line, and the actions dropped as the state
/// landed); where it lands on the load's failure, a series past the
/// review's bound, they move once, as it lands.
#[gpui::test]
async fn the_actions_stand_still_from_open_to_settled(cx: &mut gpui_kit::TestAppContext) {
    // Where each painted frame from open to settled put the actions, and
    // where they settled; the review's read has landed.
    fn opened(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) -> (Entity<SeriesBrowser>, Vec<(Pixels, bool)>, Pixels) {
        size_to(app, cx, 18, 461., 490.);
        painted_actions::watch();
        let review = review_from_palette(app, cx);
        crate::tags::tests::draw(cx);
        let painted = painted_actions::take();
        assert!(
            review.read_with(cx, |review, _| !review.pending),
            "the read lands"
        );
        assert!(
            painted.iter().any(|(_, pending)| *pending),
            "a frame drew the read under way: {painted:?}"
        );
        let settled = cx
            .debug_bounds("inspect-series-messages")
            .expect("Review messages is drawn")
            .top();
        (review, painted, settled)
    }
    let moved = |a: Pixels, b: Pixels| (a - b).abs() >= px(0.5);
    let moves = |painted: &[(Pixels, bool)]| {
        painted
            .windows(2)
            .filter(|pair| moved(pair[0].0, pair[1].0))
            .count()
    };
    {
        let (_fixture, app, cx) = series_window(cx, 0).await;
        let (review, painted, settled) = opened(&app, cx);
        assert!(
            review.read_with(cx, |review, _| review.review.is_none()
                && review.error.is_none()),
            "no rebase captured a series"
        );
        assert_eq!(
            crate::empty_state::shown("series-empty").as_deref(),
            Some(recovery_drafts::NO_CAPTURE),
            "the likely state lands"
        );
        assert!(
            painted.iter().all(|(top, _)| !moved(*top, settled)),
            "every frame from open to settled puts the actions at {settled:?}: {painted:?}"
        );
    }
    // A listed series moves them twice: once as it lands, and by the
    // ring's room a frame later, as the body reveals the list its
    // landing focuses (queued).
    for (outcome, large) in [("the load's failure", 4_500_000)] {
        let (_fixture, app, cx) = series_window(cx, large).await;
        size_to(&app, cx, 13, 1480., 973.);
        rebase_dropping_the_second(&app, cx, "main~3").await;
        let (review, painted, settled) = opened(&app, cx);
        let (failed, listed) = review.read_with(cx, |review, _| {
            (
                review.error.is_some(),
                review
                    .review
                    .as_ref()
                    .is_some_and(|review| !review.rows.is_empty()),
            )
        });
        assert_eq!(
            (failed, listed),
            (large > 0, large == 0),
            "the read lands on {outcome}"
        );
        assert!(
            moves(&painted) <= 1,
            "{outcome}: the actions move at most once from open to settled: {painted:?}"
        );
        assert!(
            painted.last().is_some_and(|(top, _)| !moved(*top, settled)),
            "{outcome}: the last frame puts them where they settle, {settled:?}: {painted:?}"
        );
    }
}

/// A failed read of an inspected commit's changed files is said in the
/// file box whose read failed, in view without scrolling, at 13 pt in a
/// 1,480 × 973 window and at 18 pt in a 461 × 490 half tile: its title
/// and message inside the box and the body's view, the box and the
/// preview where the read left them, its message wrapped rather than cut
/// and announced at once, the preview's box not claiming the commit has
/// no files, and Back finding the series list and its actions where
/// they were. Drawn at the body's end, the failure lay below the view,
/// and the preview's box said the commit changed no files.
#[gpui::test]
async fn a_failed_changed_file_read_is_said_in_the_file_box(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let (_fixture, review, cx) = rewritten_review(
        cx,
        &[&[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "fixture notes 2, reworded",
        ]],
    )
    .await;
    let app = cx.update(|_, cx| review.read(cx).owner.upgrade().expect("the app"));
    // The rewritten side names commits Git cannot find, so the real read
    // fails on the worker.
    cx.update(|_, cx| {
        review.update(cx, |review, _| {
            let mut broken = (**review.review.as_ref().expect("a series")).clone();
            for commit in &mut broken.rewritten {
                commit.commit.oid = "1".repeat(40);
            }
            review.review = Some(Arc::new(broken));
        })
    });
    let bounds = |cx: &mut gpui::VisualTestContext, id: &'static str| {
        cx.debug_bounds(id)
            .unwrap_or_else(|| panic!("{id} is drawn"))
    };
    for (text, width, height) in [(13, 1480., 973.), (18, 461., 490.)] {
        let at = format!("{text} pt in {width} × {height}");
        size_to(&app, cx, text, width, height);
        let series = [bounds(cx, "series-focus-list"), bounds(cx, "reload-series")];
        cx.update(|window, cx| review.update(cx, |review, cx| review.inspect(false, window, cx)));
        // Asked and drawn in one frame, before the worker's reply lands.
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let boxes = |cx: &mut gpui::VisualTestContext| {
            ["series-files", "series-file-preview"].map(|id| bounds(cx, id))
        };
        let reading = boxes(cx);
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        let error = review
            .read_with(cx, |review, _| review.files_error.clone())
            .unwrap_or_else(|| panic!("{at}: the read fails"));
        let [files, preview] = boxes(cx);
        assert_eq!(files.origin, reading[0].origin, "{at}: the box stays put");
        assert!(
            files.size.height >= reading[0].size.height
                && files.size.height <= px(FILES_FAILURE_MOST),
            "{at}: the box keeps its resting height or grows within its bound: {files:?}"
        );
        if text == 18 {
            assert!(
                files.size.height > reading[0].size.height,
                "{at}: the failure's three wrapped lines grow the box: {files:?}"
            );
        }
        assert!(
            ((preview.top() - reading[1].top()) - (files.size.height - reading[0].size.height))
                .abs()
                <= px(1.),
            "{at}: only the box's growth moves the preview: {preview:?} from {:?}",
            reading[1]
        );
        assert_eq!(
            crate::empty_state::shown("series-files-failed").as_deref(),
            Some(error.as_str()),
            "{at}: the box says the read's error"
        );
        let body = bounds(cx, "rewrite-review-body");
        for id in ["series-files-failed-title", "series-files-failed-line"] {
            let drawn = bounds(cx, id);
            assert!(
                lies_in(drawn, files) && lies_in(drawn, body),
                "{at}: {id} at {drawn:?} lies in the box, {files:?}, and the body's view, \
                 {body:?}"
            );
        }
        assert!(
            cx.debug_bounds("series-review-error").is_none(),
            "{at}: said once, in the box"
        );
        assert_eq!(
            crate::empty_state::shown("series-preview-empty").as_deref(),
            Some(NO_FILE_TO_PREVIEW),
            "{at}: the preview's box claims no empty commit"
        );
        let (_, announced) =
            crate::copy_feedback::tests::alerted(cx).expect("the failure is announced");
        assert!(
            announced.starts_with(FILES_UNAVAILABLE) && announced.contains(error.as_str()),
            "{at}: announced at once: {announced:?}"
        );
        // Back finds the series list and its actions where the failure
        // left them, before any key has moved the body.
        cx.update(|window, cx| review.update(cx, |review, cx| review.back(window, cx)));
        draw(cx);
        assert_eq!(
            [bounds(cx, "series-focus-list"), bounds(cx, "reload-series"),],
            series,
            "{at}: the series list and its actions are where they were"
        );
        cx.update(|window, cx| review.update(cx, |review, cx| review.inspect(false, window, cx)));
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        assert!(
            review.read_with(cx, |review, _| review.files_error.is_some()),
            "{at}: the read fails again"
        );
        // A report longer than the box wraps inside it and stays whole,
        // scrolling where it is taller than the box.
        let long = format!(
            "fatal: {}",
            "could not read the changed files of this commit ".repeat(10)
        );
        cx.update(|_, cx| {
            review.update(cx, |review, cx| {
                review.files_error = Some(long.clone());
                cx.notify();
            })
        });
        draw(cx);
        let [files, _] = boxes(cx);
        assert!(
            files.origin == reading[0].origin && files.size.height <= px(FILES_FAILURE_MOST),
            "{at}: the box stays in its bound: {files:?}"
        );
        let line = bounds(cx, "series-files-failed-line");
        assert!(
            line.size.height >= crate::empty_state::line_text() * 2.
                && line.left() >= files.left()
                && line.right() <= files.right(),
            "{at}: wrapped inside the box, {files:?}, at {line:?}"
        );
        if text == 18 {
            let scrollbar = "series-files-failure-scrollbar";
            assert!(
                cx.debug_bounds(scrollbar).is_some(),
                "{at}: the box's scrollbar cues the failure's rest"
            );
            for _ in 0..4 {
                cx.simulate_event(gpui::ScrollEvent {
                    position: files.center(),
                    delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-300.))),
                    modifiers: Modifiers::default(),
                    touch_phase: gpui::TouchPhase::Moved,
                    source: Default::default(),
                });
                draw(cx);
            }
            let line = bounds(cx, "series-files-failed-line");
            assert!(
                line.bottom() <= files.bottom() + px(0.25),
                "{at}: scrolled to its end, the message ends at {:?} inside the box, \
                 {files:?}",
                line.bottom()
            );
            // Too long for the box, it is a stop that rings the box's edge.
            let focus = review.read_with(cx, |review, _| review.files_failure_focus.clone());
            for _ in 0..30 {
                cx.simulate_keystrokes("tab");
                draw(cx);
                if cx.update(|window, _| focus.is_focused(window)) {
                    break;
                }
            }
            assert!(
                cx.update(|window, _| focus.is_focused(window)),
                "{at}: Tab reaches the failure"
            );
            let accent: gpui::Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
            let edge = bounds(cx, "series-files");
            assert!(
                crate::focus_ring::painted_outlines(cx, accent)
                    .iter()
                    .any(|ring| ring.bounds == edge),
                "{at}: the box rings its own edge at {edge:?}"
            );
        }
        cx.update(|window, cx| review.update(cx, |review, cx| review.back(window, cx)));
        draw(cx);
    }
}

/// While an inspected commit's changed files are read, its file box says
/// so, and while a chosen file's preview is read, the preview's box does,
/// each line inside the box being read and in the body's view, at 13 pt
/// in a 1,480 × 973 window; at 18 pt in a 461 × 490 half tile, where the
/// preview lies below the view, the chosen row says it, in its accessible
/// description too. The file box and the preview keep their place and
/// size as each read comes and goes, and the body's scroll never moves: a
/// choice that scrolled ~170 px slid the file list out from under the
/// pointer. Said in a line after the preview, the read lay below the view
/// in the half tile and took a line's height from the preview at each
/// read at 13 pt.
#[gpui::test]
async fn the_inspector_says_each_read_in_the_box_it_fills(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let (_fixture, review, cx) = rewritten_review(
        cx,
        &[&[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "fixture notes 2, reworded",
        ]],
    )
    .await;
    let app = cx.update(|_, cx| review.read(cx).owner.upgrade().expect("the app"));
    // Asked and drawn in one frame, before the worker's reply lands.
    let frame = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
    };
    let boxes = |cx: &mut gpui::VisualTestContext| {
        ["series-files", "series-file-preview"].map(|id| {
            cx.debug_bounds(id)
                .unwrap_or_else(|| panic!("{id} is drawn"))
        })
    };
    // The body's scroll, which no choice moves.
    let offset = |cx: &mut gpui::VisualTestContext| {
        review.read_with(cx, |review, _| review.body.scroll().offset())
    };
    // The fills the last frame painted in exactly the bounds of `id`.
    let fills = |cx: &mut gpui::VisualTestContext, id: &'static str| {
        let bounds = cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("{id} is drawn"));
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let near =
                |device: f32, logical: Pixels| (device / scale - f32::from(logical)).abs() < 0.5;
            window
                .painted_quads()
                .iter()
                .filter(|quad| {
                    near(quad.bounds.origin.x.as_f32(), bounds.left())
                        && near(quad.bounds.origin.y.as_f32(), bounds.top())
                        && near(quad.bounds.size.width.as_f32(), bounds.size.width)
                        && near(quad.bounds.size.height.as_f32(), bounds.size.height)
                })
                .map(|quad| quad.background)
                .collect::<Vec<_>>()
        })
    };
    // The state `id` says `line` inside the box `within` and in the
    // body's view.
    let says =
        |cx: &mut gpui::VisualTestContext, id: &str, line: &str, within: &'static str, at: &str| {
            assert_eq!(
                crate::empty_state::shown(id).as_deref(),
                Some(line),
                "{at}: {within} says its read"
            );
            let drawn = cx
                .debug_bounds(format!("{id}-line").leak())
                .unwrap_or_else(|| panic!("{at}: {id}'s line is drawn"));
            let within_bounds = cx.debug_bounds(within).expect("the box");
            let body = cx.debug_bounds("rewrite-review-body").expect("the body");
            assert!(drawn.size.height > px(0.), "{at}: {id} has a line");
            assert!(
                lies_in(drawn, within_bounds),
                "{at}: {id}'s line at {drawn:?} lies in {within}, {within_bounds:?}"
            );
            assert!(
                lies_in(drawn, body),
                "{at}: {id}'s line at {drawn:?} lies in the body's view, {body:?}"
            );
        };
    for (text, width, height) in [(13, 1480., 973.), (18, 461., 490.)] {
        let at = format!("{text} pt in {width} × {height}");
        size_to(&app, cx, text, width, height);
        cx.update(|window, cx| review.update(cx, |review, cx| review.inspect(false, window, cx)));
        frame(cx);
        let reading = boxes(cx);
        says(
            cx,
            "series-files-reading",
            READING_FILES,
            "series-files",
            &at,
        );
        assert!(
            cx.debug_bounds("series-preview-reading").is_none(),
            "{at}: the preview reads nothing yet"
        );
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        assert!(
            cx.debug_bounds("series-files-reading").is_none(),
            "{at}: the line goes once the files land"
        );
        assert_eq!(boxes(cx), reading, "{at}: nothing moves as the files land");
        // The preview's box says which file to choose, as a designed
        // empty state in the box.
        assert_eq!(
            crate::empty_state::shown("series-preview-empty").as_deref(),
            Some(CHOOSE_FILE),
            "{at}: the preview's box says which file to choose"
        );
        let prompt = cx
            .debug_bounds("series-preview-empty-line")
            .unwrap_or_else(|| panic!("{at}: the prompt is drawn"));
        assert!(
            lies_in(prompt, reading[1]),
            "{at}: the prompt at {prompt:?} lies in the preview's box, {:?}",
            reading[1]
        );
        let scrolled = offset(cx);
        cx.update(|window, cx| review.update(cx, |review, cx| review.file(0, window, cx)));
        frame(cx);
        assert_eq!(
            offset(cx),
            scrolled,
            "{at}: choosing a file scrolls nothing"
        );
        // In view in the wide window, the modes wait for the patch with
        // none drawn selected: Patch is painted as Before source is.
        if text == 13 {
            assert_eq!(
                fills(cx, "series-text-mode-0"),
                fills(cx, "series-text-mode-1"),
                "{at}: no mode is drawn selected while the modes wait"
            );
        }
        // In the half tile the preview lies wholly below the body's
        // view, so the chosen row says the read.
        let body = cx.debug_bounds("rewrite-review-body").expect("the body");
        let [files, preview] = boxes(cx);
        let hidden = text == 18;
        assert_eq!(
            preview.center().y > body.bottom(),
            hidden,
            "{at}: the preview, {preview:?}, against the body's view, {body:?}"
        );
        let row = cx.debug_bounds("series-file-reading");
        if hidden {
            let row = row.unwrap_or_else(|| panic!("{at}: the chosen row says its read"));
            assert!(
                lies_in(row, files) && lies_in(row, body),
                "{at}: the row's read at {row:?} lies in the file box, {files:?}, in view, \
                 {body:?}"
            );
            let node = crate::native_accessibility::written::get("series-file-0")
                .unwrap_or_else(|| panic!("{at}: the chosen row's node is written"))
                .node;
            assert_eq!(
                node.description(),
                Some(READING_PREVIEW),
                "{at}: the chosen row's accessible description says its read"
            );
        } else {
            says(
                cx,
                "series-preview-reading",
                READING_PREVIEW,
                "series-file-preview",
                &at,
            );
            assert!(
                row.is_none(),
                "{at}: the row says nothing where the preview's read is seen"
            );
        }
        assert_eq!(
            boxes(cx),
            reading,
            "{at}: nothing moves as the preview is read"
        );
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        assert!(
            review.read_with(cx, |review, _| matches!(
                review.content.as_deref(),
                Some(Content::Text { .. })
            )),
            "{at}: the file's patch lands"
        );
        assert!(
            cx.debug_bounds("series-preview-reading").is_none()
                && cx.debug_bounds("series-file-reading").is_none(),
            "{at}: the read goes once the preview lands"
        );
        assert_eq!(offset(cx), scrolled, "{at}: nor does its landing");
        if text == 13 {
            assert_ne!(
                fills(cx, "series-text-mode-0"),
                fills(cx, "series-text-mode-1"),
                "{at}: Patch is drawn selected once the patch lands"
            );
        }
        assert_eq!(
            boxes(cx),
            reading,
            "{at}: nothing moves as the preview lands"
        );
        cx.update(|window, cx| review.update(cx, |review, cx| review.back(window, cx)));
        draw(cx);
    }
}

/// The review's dialog as the last frame painted it: the smallest quad
/// edged on every side around the body, which holds no edged box of its
/// own around it.
fn dialog_frame(cx: &mut gpui::VisualTestContext) -> Bounds<Pixels> {
    let body = cx.debug_bounds("rewrite-review-body").expect("the body");
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| {
                let widths = quad.border_widths;
                [widths.top, widths.right, widths.bottom, widths.left]
                    .iter()
                    .all(|width| width.as_f32() > 0.)
            })
            .map(|quad| {
                Bounds::from_corners(
                    point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                    point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                )
            })
            .filter(|frame| {
                frame.top() < body.top()
                    && frame.bottom() > body.bottom()
                    && frame.left() < body.left()
                    && frame.right() > body.right()
            })
            .min_by(|a, b| a.size.height.partial_cmp(&b.size.height).unwrap())
            .expect("the dialog's frame")
    })
}

/// The review's body takes the room the dialog's height cap leaves it,
/// measured as the dialog lays out its title, footer and padding: an
/// inspector fills it, the dialog's foot on its cap's, and the series
/// running past it at 18 pt in a 461 × 490 window takes all of it too,
/// at 13 and 18 pt in that window and in a 1,480 × 800 one; the content
/// takes [`content_room`], which the series list keeps its rest from. A
/// window less 250 px once stood for that room, which the body stopped
/// at: at 13 pt in the 461 × 490 window it ended the dialog 53.5 px short
/// of its cap's foot.
#[gpui::test]
async fn the_body_takes_the_room_the_dialog_cap_leaves(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    lands(&browser, cx);
    let slack = px(0.5);
    for (text, width, height) in [
        (13, 461., 490.),
        (18, 461., 490.),
        (13, 1480., 800.),
        (18, 1480., 800.),
    ] {
        let at = format!("{text} pt in {width} × {height}");
        size_to(&app, cx, text, width, height);
        let (foot, room) = cx.update(|window, cx| {
            (
                crate::theme_editor::status_bar_top(window) - appearance::DIALOG_MARGIN,
                content_room(window, cx),
            )
        });
        for inspecting in [false, true] {
            let state = if inspecting {
                "inspecting"
            } else {
                "the series"
            };
            cx.update(|_, cx| {
                browser.update(cx, |browser, cx| {
                    browser.inspecting = inspecting;
                    cx.notify();
                })
            });
            // The inspector lays out its final room in the first frame
            // that draws it, and keeps it once everything has settled.
            let frames: &[&str] = if inspecting {
                &["in its first frame", "settled"]
            } else {
                &["settled"]
            };
            for &drawn in frames {
                if drawn == "settled" {
                    draw(cx);
                    draw(cx);
                } else {
                    cx.update(|window, cx| {
                        window.refresh();
                        window.draw(cx).clear(cx);
                    });
                }
                let state = format!("{state}, {drawn}");
                let frame = dialog_frame(cx);
                let content = cx
                    .debug_bounds("rewrite-review-content")
                    .expect("the review's content");
                let body = cx.debug_bounds("rewrite-review-body").expect("the body");
                let scrolls = browser.read_with(cx, |browser, _| {
                    browser.body.scroll().max_offset().y > px(0.)
                });
                if !inspecting && !scrolls {
                    assert!(
                        frame.bottom() <= foot + slack,
                        "{at}: {state} fits its dialog {frame:?} inside its cap's foot {foot:?}"
                    );
                    continue;
                }
                assert!(
                    (frame.bottom() - foot).abs() <= slack,
                    "{at}: {state}'s dialog {frame:?} ends at its cap's foot {foot:?}"
                );
                assert!(
                    (content.size.height - room).abs() <= slack,
                    "{at}: {state}'s content {content:?} takes the room the cap leaves, {room:?}"
                );
                assert!(
                    (body.bottom() - content.bottom()).abs() <= slack,
                    "{at}: {state}'s body {body:?} reaches the content's foot {content:?}"
                );
            }
        }
    }
}

/// A review of `rows` commit pairs that name no commit, opened in
/// `app`'s window, its read landed, with focus on its series.
fn listed_review(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
    rows: usize,
) -> Entity<SeriesBrowser> {
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    lands(&browser, cx);
    let row = gitturtle_core::SeriesRow {
        original: None,
        rewritten: None,
        change: gitturtle_core::SeriesChange::Changed,
        reordered: false,
        ambiguous: false,
    };
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.review = Some(Arc::new(RewriteReview {
                root: browser.repo.path().to_owned(),
                branch: "main".into(),
                base: String::new(),
                original_head: String::new(),
                rewritten_head: String::new(),
                original: Vec::new(),
                rewritten: Vec::new(),
                rows: vec![row; rows],
            }));
            browser.selected = 0;
            browser.focus.focus(window, cx);
            cx.notify();
        })
    });
    crate::tags::tests::draw(cx);
    browser
}

/// The series list is as tall as the whole rows the content's room
/// holds, inside its 1 px border, at least one, and scrolls by whole
/// rows, its scrollbar cueing the rest: with eight pairs at 13 and 18 pt
/// in a 461 × 490 window and in a 1,480 × 800 one, at the test display's
/// 2× and at 13 pt in the larger window at 1.5× and 1.75× too, every row
/// it draws lies whole inside it as Down selects each row in turn and Up
/// each one back. Cut at the rest of the body's room, at 13 pt in the
/// 461 × 490 window it showed one 70 px row and 13.5 px of the next, and
/// centred the selection between half rows; with a flat 2 px for its
/// border, which layout snaps to a device pixel on each side, it showed
/// a device pixel of the next row at 1.5× and cut one off the last at
/// 1.75×.
#[gpui::test]
async fn the_series_list_shows_whole_rows(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, rendered, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let rows = 8;
    let browser = listed_review(&app, cx, rows);
    let slack = px(0.05);
    for (text, width, height, scale) in [
        (13, 461., 490., 2.),
        (18, 461., 490., 2.),
        (13, 1480., 800., 2.),
        (18, 1480., 800., 2.),
        (13, 1480., 800., 1.5),
        (13, 1480., 800., 1.75),
    ] {
        let at = format!("{text} pt in {width} × {height} at {scale}×");
        size_to(&app, cx, text, width, height);
        // A resize takes the test display's 2×, so the scale follows it.
        cx.update(|window, _| window.set_scale_factor(scale));
        draw(cx);
        // The border as layout draws it, a device pixel at 1.5×.
        let border = cx.update(|window, _| window.pixel_snap(px(1.)));
        // From the top, as the review opens.
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.select(0, window, cx);
                browser
                    .rows_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(0.)));
            })
        });
        draw(cx);
        let row = cx.update(|window, _| series_row_height(window));
        let list = rendered(cx, "series-focus-list");
        let shown = (list.size.height - border * 2.) / row;
        assert!(
            shown >= 1. && (shown - shown.round()).abs() * f32::from(row) < 0.05,
            "{at}: the list {list:?} holds whole rows of {row:?} inside its border"
        );
        assert!(
            (shown.round() as usize) < rows,
            "{at}: the list shows {shown} of its {rows} rows, which overflow it"
        );
        assert!(
            cx.debug_bounds("rewrite-series-scrollbar").is_some(),
            "{at}: the list's scrollbar cues the rows past its foot"
        );
        let inside = Bounds::from_corners(
            list.origin + point(border, border),
            list.bottom_right() - point(border, border),
        );
        let mut walk = vec!["down"; rows - 1];
        walk.extend(vec!["up"; rows - 1]);
        for (step, key) in walk.into_iter().enumerate() {
            cx.simulate_keystrokes(key);
            draw(cx);
            let selected = browser.read_with(cx, |browser, _| browser.selected);
            let drawn: Vec<_> = (0..rows)
                .filter_map(|index| {
                    let selector: &'static str = format!("series-row-{index}").leak();
                    cx.debug_bounds(selector).map(|bounds| (index, bounds))
                })
                .filter(|(_, bounds)| bounds.intersects(&inside))
                .collect();
            assert!(
                drawn.iter().any(|(index, _)| *index == selected),
                "{at}: after {key} (step {step}) the selected row {selected} is drawn"
            );
            for (index, bounds) in drawn {
                assert!(
                    bounds.top() >= inside.top() - slack
                        && bounds.bottom() <= inside.bottom() + slack,
                    "{at}: after {key} (step {step}) row {index} {bounds:?} lies whole \
                     in the list {inside:?}"
                );
            }
        }
    }
}

/// The rows of `rows` the series list draws inside its border in the
/// last frame painted, each with whether it lies whole there.
fn rows_seen(cx: &mut gpui::VisualTestContext, rows: usize) -> Vec<(usize, bool)> {
    let border = cx.update(|window, _| window.pixel_snap(px(1.)));
    let list = crate::tags::tests::rendered(cx, "series-focus-list");
    let inside = Bounds::from_corners(
        list.origin + point(border, border),
        list.bottom_right() - point(border, border),
    );
    let slack = px(0.05);
    (0..rows)
        .filter_map(|index| {
            let selector: &'static str = format!("series-row-{index}").leak();
            let bounds = cx.debug_bounds(selector)?;
            let seen =
                bounds.top() < inside.bottom() - slack && bounds.bottom() > inside.top() + slack;
            let whole =
                bounds.top() >= inside.top() - slack && bounds.bottom() <= inside.bottom() + slack;
            seen.then_some((index, whole))
        })
        .collect()
}

/// Whether the series list draws row `row` whole inside its border, and
/// no other row, in the last frame painted.
fn shows_only(cx: &mut gpui::VisualTestContext, row: usize, rows: usize) -> bool {
    rows_seen(cx, rows) == [(row, true)]
}

/// The selected row stays whole in the series list, by the least scroll,
/// in the first frame after its rows change under it with no key: row 5
/// of eight, selected at 13 pt in a 1,480 × 800 window, where the list
/// shows five rows, row 5 at their foot, stays the row the list shows
/// as the window is tiled to a 461 × 490 half tile, where it shows one,
/// as the text grows to 18 pt there, and as the display's scale goes
/// from 2× to 1.25×, where a row's snapped height goes from 94 to
/// 94.4 px. Only a key revealed it: the half tile's list showed row 1
/// with row 5 selected, and Return inspected a row nobody saw.
#[gpui::test]
async fn the_selected_row_stays_whole_as_its_rows_change(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let rows = 8;
    let browser = listed_review(&app, cx, rows);
    size_to(&app, cx, 13, 1480., 800.);
    cx.update(|window, cx| browser.update(cx, |browser, cx| browser.select(5, window, cx)));
    draw(cx);
    let shown = cx.update(|window, cx| rows_shown(rows, window, cx));
    assert_eq!(
        rows_seen(cx, rows),
        (6 - shown..6).map(|row| (row, true)).collect::<Vec<_>>(),
        "the larger window's list shows its {shown} whole rows, row 5 at their foot"
    );
    assert!(
        shown > 1,
        "the larger window's list shows more than one row"
    );
    // One frame after each change, which no key follows.
    let frame = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
    };
    cx.simulate_resize(size(px(461.), px(490.)));
    frame(cx);
    assert!(
        shows_only(cx, 5, rows),
        "tiled to 461 × 490, the one-row list shows the selected row 5 whole"
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings.interface_text_size = 18;
            appearance::apply_text_sizes(18, app.settings.code_text_size, window, cx);
            cx.notify();
        })
    });
    frame(cx);
    assert!(
        shows_only(cx, 5, rows),
        "at 18 pt there, the list shows row 5 whole"
    );
    let before = cx.update(|window, _| series_row_height(window));
    cx.update(|window, _| window.set_scale_factor(1.25));
    frame(cx);
    let after = cx.update(|window, _| series_row_height(window));
    assert_ne!(before, after, "the scale changes a row's snapped height");
    assert!(
        shows_only(cx, 5, rows),
        "at 1.25×, the list shows row 5 whole, {after:?} tall"
    );
    assert_eq!(
        browser.read_with(cx, |browser, _| browser.selected),
        5,
        "nothing changed the selection"
    );
}

/// With more than one row shown, the series list keeps the row at its
/// top as a text size changes its rows' height, in the frame that
/// changes them: at row 2 of eight, which is selected, at 13 pt in a
/// 1,480 × 800 window, it is still at row 2 at 18 pt. Its offset is
/// carried over in rows; left in pixels it read as a row nearer the top
/// of the taller rows, and the list moved with no key.
#[gpui::test]
async fn the_series_list_keeps_its_row_as_a_text_size_changes_its_rows(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let rows = 8;
    let browser = listed_review(&app, cx, rows);
    size_to(&app, cx, 13, 1480., 800.);
    let before = cx.update(|window, _| series_row_height(window));
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.select(2, window, cx);
            browser
                .rows_scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.), -before * 2.));
        })
    });
    draw(cx);
    let from_row_2 = |shown: usize| (2..2 + shown).map(|row| (row, true)).collect::<Vec<_>>();
    let shown = cx.update(|window, cx| rows_shown(rows, window, cx));
    assert!(shown > 1, "the list shows several rows at 13 pt: {shown}");
    assert_eq!(
        rows_seen(cx, rows),
        from_row_2(shown),
        "the list is at row 2 at 13 pt"
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings.interface_text_size = 18;
            appearance::apply_text_sizes(18, app.settings.code_text_size, window, cx);
            cx.notify();
        })
    });
    // One frame, which no key follows.
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let after = cx.update(|window, _| series_row_height(window));
    assert_ne!(before, after, "the text size changes a row's height");
    let shown = cx.update(|window, cx| rows_shown(rows, window, cx));
    assert!(shown > 1, "the list shows several rows at 18 pt: {shown}");
    assert_eq!(
        rows_seen(cx, rows),
        from_row_2(shown),
        "at 18 pt, the list keeps row 2 at its top"
    );
    assert_eq!(
        browser.read_with(cx, |browser, _| browser.selected),
        2,
        "nothing changed the selection"
    );
}

/// The text modes keep their row for every preview, Diagrams among
/// them, refused with their reason where it has no text or no Mermaid
/// diagrams, so the preview's box keeps its place and size before a file
/// is chosen and as an image's, a text file's, the image's again,
/// another image's, a Mermaid file's, a document's and a notice's
/// previews are chosen and land, at 13 and 18 pt in a 461 × 490 half
/// tile and in a 1,480 × 800 window: the row left as an image landed and
/// came back with the next choice, moving the preview 34.5 px each way,
/// and Diagrams, there only for text with diagrams, wrapped the row at
/// 18 pt in the half tile. A notice's modes give its own words.
#[gpui::test]
async fn the_preview_keeps_its_box_between_images_and_text(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let png = crate::system_preview::tests::png;
    for (round, message) in [("first", "Add files"), ("second", "Change them")] {
        std::fs::write(repo.path().join("a.png"), png(Some(round.as_bytes()))).unwrap();
        std::fs::write(
            repo.path().join("b.png"),
            png(Some(format!("{round} b").as_bytes())),
        )
        .unwrap();
        std::fs::write(repo.path().join("notes.txt"), format!("{round}\n")).unwrap();
        std::fs::write(
            repo.path().join("diagram.mmd"),
            format!("flowchart LR\n  A[{round}] --> B\n"),
        )
        .unwrap();
        std::fs::write(
            repo.path().join("document.pdf"),
            format!("%PDF-1.4\n% {round}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n"),
        )
        .unwrap();
        // Past the text preview's line limit, which a notice says.
        std::fs::write(
            repo.path().join("long.txt"),
            format!("{round}\n").repeat(gitturtle_core::MAX_DIFF_LINES + 1),
        )
        .unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "--quiet", "-m", message]);
    }
    // A fresh repository keeps its branch as a loose reference.
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    let files = repo.changes_with_renames(head.trim(), 0).unwrap();
    let index = |path: &str| {
        files
            .iter()
            .position(|file| file.path() == std::path::Path::new(path))
            .unwrap_or_else(|| panic!("{path} changed"))
    };
    let (image, other, text_file) = (index("a.png"), index("b.png"), index("notes.txt"));
    let (mermaid, document, long) = (
        index("diagram.mmd"),
        index("document.pdf"),
        index("long.txt"),
    );
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    let frame = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
    };
    let preview = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("series-file-preview")
            .expect("the preview's box is drawn")
    };
    for (text, width, height) in [
        (13, 461., 490.),
        (18, 461., 490.),
        (13, 1480., 800.),
        (18, 1480., 800.),
    ] {
        let at = format!("{text} pt in {width} × {height}");
        size_to(&app, cx, text, width, height);
        cx.update(|_, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files.clone();
                browser.clear_content();
                cx.notify();
            })
        });
        draw(cx);
        let kept = preview(cx);
        for (name, chosen, kind) in [
            ("an image", image, "images"),
            ("a text file", text_file, "text"),
            ("the image again", image, "images"),
            ("another image", other, "images"),
            ("a Mermaid file", mermaid, "diagrams"),
            ("a document", document, "document"),
            ("a notice", long, "notice"),
        ] {
            let is_text = kind == "text" || kind == "diagrams";
            cx.update(|window, cx| {
                browser.update(cx, |browser, cx| browser.file(chosen, window, cx))
            });
            frame(cx);
            assert_eq!(preview(cx), kept, "{at}: choosing {name} moves nothing");
            browser.read_with(cx, |browser, _| browser.worker.wait_idle());
            draw(cx);
            let (landed, notice) =
                browser.read_with(cx, |browser, _| match browser.content.as_deref() {
                    Some(Content::Text {
                        diagrams: Some(_), ..
                    }) => (Some("diagrams"), None),
                    Some(Content::Text { .. }) => (Some("text"), None),
                    Some(Content::Images { .. }) => (Some("images"), None),
                    Some(Content::Rich(_)) => (Some("document"), None),
                    Some(Content::Notice(notice)) => (Some("notice"), Some(notice.clone())),
                    _ => (None, None),
                });
            assert_eq!(landed, Some(kind), "{at}: {name}'s preview lands");
            assert_eq!(preview(cx), kept, "{at}: {name}'s landing moves nothing");
            let mode = |index: usize| {
                crate::native_accessibility::written::get(&format!("series-text-mode-{index}"))
                    .unwrap_or_else(|| panic!("{at}: mode {index} is drawn for {name}"))
                    .node
            };
            let patch = mode(0);
            assert_eq!(
                patch.is_disabled(),
                !is_text,
                "{at}: Patch waits for a preview with text, for {name}"
            );
            if !is_text {
                assert_eq!(
                    patch.description(),
                    Some(notice.as_deref().unwrap_or(NO_TEXT)),
                    "{at}: Patch says why it waits, for {name}"
                );
            }
            let diagrams = mode(3);
            assert_eq!(
                diagrams.is_disabled(),
                kind != "diagrams",
                "{at}: Diagrams waits for text with diagrams, for {name}"
            );
            if kind != "diagrams" {
                assert_eq!(
                    diagrams.description(),
                    Some(notice.as_deref().unwrap_or(NOT_DIAGRAMS)),
                    "{at}: Diagrams says why it waits, for {name}"
                );
            }
        }
    }
}

/// A chosen file whose preview cannot be read, a link whose object is
/// gone, says why
/// in the preview's box, where the preview would draw, as a designed
/// empty state, and the text modes give the read's error as their
/// reason: the box went on asking for a file to be chosen with its file
/// chosen, the error drawn below it at the body's end, and the modes said
/// the file had no patch or source.
#[gpui::test]
async fn a_failed_preview_read_says_why_in_its_box(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
    git(repo.path(), &["add", "notes.txt"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
    // A fresh repository keeps its branch as a loose reference.
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    let mut files = repo.changes_with_renames(head.trim(), 0).unwrap();
    // A stored link, which the preview never renders, whose object is
    // gone: its read fails, where a missing regular file's side says so
    // in a preview of its own.
    files[0].new_mode = "120000".into();
    files[0].new_oid = Some("0123456789abcdef0123456789abcdef01234567".into());
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    lands(&browser, cx);
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.inspecting = true;
            browser.inspect_label = "Rewritten".into();
            browser.files = files.clone();
            browser.file(0, window, cx);
        })
    });
    browser.read_with(cx, |browser, _| browser.worker.wait_idle());
    draw(cx);
    let error = browser
        .read_with(cx, |browser, _| browser.preview_error.clone())
        .expect("the preview's read fails");
    assert_eq!(
        crate::empty_state::shown("series-preview-failed").as_deref(),
        Some(error.as_str()),
        "the preview's box says why its read failed"
    );
    let (said, kept) = (
        rendered(cx, "series-preview-failed"),
        rendered(cx, "series-file-preview"),
    );
    assert!(
        lies_in(said, kept),
        "the failure {said:?} is said in the preview's box {kept:?}"
    );
    // Under its title, as every failure keeps its title above its
    // explanation (DESIGN.md, "Empty and edge states"): it had none.
    let (title, line) = (
        cx.debug_bounds("series-preview-failed-title")
            .expect("the failure's title is drawn"),
        rendered(cx, "series-preview-failed-line"),
    );
    assert!(
        lies_in(title, kept) && title.bottom() <= line.top(),
        "the title {title:?} lies in the box {kept:?}, above the error {line:?}"
    );
    assert!(
        crate::native_accessibility::written::labels()
            .iter()
            .any(|(_, name)| name.as_deref() == Some("Preview unavailable")),
        "the title reads \"Preview unavailable\""
    );
    assert!(
        cx.debug_bounds("series-preview-empty").is_none(),
        "the box no longer asks for a file to be chosen"
    );
    assert!(
        cx.debug_bounds("series-review-error").is_none(),
        "the failure is not said again at the body's end"
    );
    for mode in [0, 1, 2, 3] {
        let node = crate::native_accessibility::written::get(&format!("series-text-mode-{mode}"))
            .unwrap_or_else(|| panic!("mode {mode} is drawn"))
            .node;
        assert!(node.is_disabled(), "mode {mode} waits for a preview");
        assert_eq!(
            node.description(),
            Some(error.as_str()),
            "mode {mode} gives the read's error as its reason"
        );
    }
    // A failure that fits draws no scrollbar and keeps no gutter at
    // either scale: laid out as tall as its scroller, and an overflow
    // being strictly past zero, a float's remainder would draw one, and
    // make it a Tab stop with nothing to scroll.
    for (scale, width, height) in [(1., 1480., 800.), (1.5, 1480., 800.), (1.5, 1481., 803.)] {
        let at = format!("at 13 pt in {width} × {height} at {scale}×");
        size_to(&app, cx, 13, width, height);
        // A resize takes the test display's 2×, so the scale follows it.
        cx.update(|window, _| window.set_scale_factor(scale));
        draw(cx);
        assert!(
            cx.debug_bounds("series-preview-failure-scrollbar")
                .is_none(),
            "{at}: a failure that fits draws no scrollbar"
        );
        assert_eq!(
            browser.read_with(cx, |browser, _| browser.failure_scroll.max_offset().y),
            px(0.),
            "{at}: a failure that fits has nothing to scroll, so keeps no gutter"
        );
    }
}

/// A review in `app`'s window, its read landed, inspecting `files` as
/// a commit's, with none chosen.
fn inspecting(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
    files: &[FileChange],
) -> Entity<SeriesBrowser> {
    let browser = cx.update(|window, cx| {
        let repo = app
            .read(cx)
            .repository
            .clone()
            .expect("the window's repository");
        let browser = cx.new(|cx| {
            SeriesBrowser::new(
                app.downgrade(),
                repo,
                "origin".into(),
                "main".into(),
                window,
                cx,
            )
        });
        open_series_dialog(browser.clone(), window, cx);
        browser
    });
    lands(&browser, cx);
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.inspecting = true;
            browser.inspect_label = "Rewritten".into();
            browser.files = files.to_vec();
            cx.notify();
        })
    });
    crate::tags::tests::draw(cx);
    browser
}

/// Chooses the inspected commit's file `index` and lands its preview.
fn choose(browser: &Entity<SeriesBrowser>, cx: &mut gpui::VisualTestContext, index: usize) {
    cx.update(|window, cx| browser.update(cx, |browser, cx| browser.file(index, window, cx)));
    browser.read_with(cx, |browser, _| browser.worker.wait_idle());
    crate::tags::tests::draw(cx);
}

/// The changed files of `repo`'s last commit, and a lookup of each by
/// its path.
fn last_commit_files(repo: &GitRepository) -> Vec<FileChange> {
    // A fresh repository keeps its branch as a loose reference.
    let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
    repo.changes_with_renames(head.trim(), 0).unwrap()
}

fn file_index(files: &[FileChange], path: &str) -> usize {
    files
        .iter()
        .position(|file| file.path() == std::path::Path::new(path))
        .unwrap_or_else(|| panic!("{path} changed"))
}

/// The text modes the last frame drew selected: those that paint a fill
/// over their bounds at rest, with the pointer away, which a ghost mode
/// at rest does not.
fn selected_modes(cx: &mut gpui::VisualTestContext) -> Vec<usize> {
    (0..4)
        .filter(|mode| {
            let key = TEXT_MODES[*mode].1;
            !crate::views::tests::painted_fills(cx, key).is_empty()
        })
        .collect()
}

/// Diagrams, chosen on a Mermaid file, gives way while it cannot apply:
/// on `notes.txt` the modes draw Patch, the file's default text mode as
/// Compare's Unified is, selected, and the box its patch, while the
/// person's choice is kept, so the Mermaid file chosen again draws
/// Diagrams selected and its diagrams. Left as the drawn mode, it drew
/// no mode selected, while the row refused Diagrams, and the box the
/// bare "No Mermaid diagrams on this file; use Source.".
#[gpui::test]
async fn diagrams_gives_way_to_the_patch_on_a_file_without_them(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
    std::fs::write(repo.path().join("diagram.mmd"), "flowchart LR\n  A --> B\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &["commit", "--quiet", "-m", "Add notes and a diagram"],
    );
    let files = last_commit_files(&repo);
    let (mermaid, notes) = (
        file_index(&files, "diagram.mmd"),
        file_index(&files, "notes.txt"),
    );
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    size_to(&app, cx, 13, 1480., 800.);
    let browser = inspecting(&app, cx, &files);
    choose(&browser, cx, mermaid);
    let diagrams = cx.debug_bounds(TEXT_MODES[3].1).expect("Diagrams is drawn");
    cx.simulate_click(diagrams.center(), Modifiers::none());
    cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
    draw(cx);
    assert_eq!(
        selected_modes(cx),
        [3],
        "Diagrams is chosen and drawn selected"
    );
    choose(&browser, cx, notes);
    assert_eq!(
        browser.read_with(cx, |browser, _| browser.text_mode),
        3,
        "the person's choice of Diagrams is kept"
    );
    assert_eq!(
        selected_modes(cx),
        [0],
        "notes.txt, with no diagrams, draws Patch selected"
    );
    let (patch, text) = cx.update(|_, cx| {
        let browser = browser.read(cx);
        (
            browser.patch.is_some(),
            browser
                .editor
                .as_ref()
                .map(|editor| editor.read(cx).value().to_string()),
        )
    });
    assert!(patch, "the box draws notes.txt's patch");
    assert!(
        text.is_some_and(|text| text.contains("+notes")),
        "the patch is notes.txt's"
    );
    choose(&browser, cx, mermaid);
    assert_eq!(
        selected_modes(cx),
        [3],
        "the Mermaid file chosen again draws Diagrams selected"
    );
    assert!(
        browser.read_with(cx, |browser, _| browser.patch.is_none()),
        "and its diagrams, not a patch"
    );
}

/// A preview failure taller than the preview's box, a five-line error
/// at 18 pt in a 461 × 490 half tile, scrolls inside the box with the
/// shared scroll cue rather than being cut: at its top the state starts
/// whole in the box, and scrolled by the wheel to its end, the error's
/// last line ends in it. Centred in a box that clips, it lost its first
/// and last lines' edges.
#[gpui::test]
async fn a_long_preview_failure_scrolls_in_its_box(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
    git(repo.path(), &["add", "notes.txt"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
    let files = last_commit_files(&repo);
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    size_to(&app, cx, 18, 461., 490.);
    let browser = inspecting(&app, cx, &files);
    let error = (1..=5)
        .map(|line| format!("Line {line} of the read's error"))
        .collect::<Vec<_>>()
        .join("\n");
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.selected_file = Some(0);
            browser.preview_error = Some(error.clone());
            cx.notify();
        })
    });
    draw(cx);
    let slack = px(0.25);
    let kept = rendered(cx, "series-file-preview");
    let line = rendered(cx, "series-preview-failed-line");
    assert!(
        line.size.height > kept.size.height / 2.,
        "the error's five lines, {line:?}, fill most of the box, {kept:?}"
    );
    let top = cx
        .debug_bounds("series-preview-failed-title")
        .unwrap_or(line)
        .top();
    assert!(
        top >= kept.top() - slack,
        "at its top, the failure starts at {top:?} inside the box, {kept:?}"
    );
    assert!(
        cx.debug_bounds("series-preview-failure-scrollbar")
            .is_some(),
        "the box's scrollbar cues the failure's rest"
    );
    // The box lies wholly below the body's fold here, so the wheel
    // first brings it into the body's view, as a person's does: over
    // the box's place while the body hides it, it reaches no scroller.
    let wheel = |at: gpui::Point<Pixels>, cx: &mut gpui::VisualTestContext| {
        cx.simulate_event(gpui::ScrollEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-300.))),
            modifiers: Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
            source: Default::default(),
        });
        draw(cx);
    };
    let body = rendered(cx, "rewrite-review-body");
    wheel(body.center(), cx);
    let (kept, body) = (
        rendered(cx, "series-file-preview"),
        rendered(cx, "rewrite-review-body"),
    );
    assert!(
        body.contains(&kept.center()),
        "the wheel brought the box, {kept:?}, into the body's view, {body:?}"
    );
    for _ in 0..3 {
        wheel(kept.center(), cx);
    }
    let (kept, line) = (
        rendered(cx, "series-file-preview"),
        rendered(cx, "series-preview-failed-line"),
    );
    assert!(
        line.bottom() <= kept.bottom() + slack,
        "scrolled to its end, the error's last line ends at {:?} inside the box, {kept:?}",
        line.bottom()
    );
}

/// `app`'s review inspecting the last commit of `repo`, its first file
/// chosen and its read failed with `error`.
fn failed_read(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
    repo: &GitRepository,
    error: &str,
) -> Entity<SeriesBrowser> {
    let browser = inspecting(app, cx, &last_commit_files(repo));
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.selected_file = Some(0);
            browser.preview_error = Some(error.into());
            cx.notify();
        })
    });
    crate::tags::tests::draw(cx);
    browser
}

/// A repository in `path` whose last commit adds `notes.txt`.
fn committed_notes(path: &std::path::Path) -> GitRepository {
    use crate::tags::tests::{git, tagged_repository};
    let repo = tagged_repository(path);
    std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
    git(repo.path(), &["add", "notes.txt"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
    repo
}

/// Whether the failure in the preview's box holds focus.
fn failure_focused(browser: &Entity<SeriesBrowser>, cx: &mut gpui::VisualTestContext) -> bool {
    cx.update(|window, cx| browser.read(cx).failure_focus.is_focused(window))
}

/// A failure taller than the preview's box, a five-line error at 18 pt
/// in a 461 × 490 half tile, is a Tab stop while it overflows, by Tab
/// and by Shift+Tab: focus on it rings the box's own 1 px edge whole and
/// brings the box, which lies below the body's fold, into view; Up and
/// Down scroll it a line at a time, to its ends and no further, and move
/// nothing else. Made to fit by a resize while it holds focus, it keeps
/// focus and its ring until the next Tab, which leaves it for good. It
/// scrolled only by the pointer: the keyboard could not reach its last
/// lines.
#[gpui::test]
async fn a_long_preview_failure_is_a_tab_stop_that_up_and_down_scroll(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::focus_ring::sweep;
    use crate::tags::tests::{draw, rendered, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = committed_notes(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    size_to(&app, cx, 18, 461., 490.);
    let error = (1..=5)
        .map(|line| format!("Line {line} of the read's error"))
        .collect::<Vec<_>>()
        .join("\n");
    let browser = failed_read(&app, cx, &repo, &error);
    let mut failures = Vec::new();
    for key in ["tab", "shift-tab"] {
        let mut reached = false;
        let mut walked = Vec::new();
        sweep::walk_each(
            &app,
            cx,
            key,
            &format!("the long failure by {key}"),
            18,
            &mut walked,
            &mut |cx, stop, rings| {
                if !failure_focused(&browser, cx) {
                    return;
                }
                reached = true;
                let (kept, body) = (
                    rendered(cx, "series-file-preview"),
                    rendered(cx, "rewrite-review-body"),
                );
                let drawn: Vec<_> = rings.iter().map(|ring| ring.bounds).collect();
                if !sweep::rings_edge(&drawn, kept) {
                    failures.push(format!(
                        "{stop}: the failure draws no ring on the box's edge, {kept:?}, only \
                         {drawn:?}"
                    ));
                }
                if !lies_in(kept, body) {
                    failures.push(format!(
                        "{stop}: the box, {kept:?}, is not revealed in the body, {body:?}"
                    ));
                }
            },
        );
        failures.extend(walked);
        if !reached {
            failures.push(format!("{key} never focused the long failure"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for _ in 0..40 {
        if failure_focused(&browser, cx) {
            break;
        }
        cx.simulate_keystrokes("tab");
        draw(cx);
    }
    assert!(failure_focused(&browser, cx), "Tab reaches the failure");
    let scrolled = |cx: &mut gpui::VisualTestContext| {
        browser.read_with(cx, |browser, _| {
            (
                -browser.failure_scroll.offset().y,
                browser.failure_scroll.max_offset().y,
            )
        })
    };
    let body_at = |cx: &mut gpui::VisualTestContext| {
        browser.read_with(cx, |browser, _| browser.body.scroll().offset())
    };
    let press = |key: &str, cx: &mut gpui::VisualTestContext| {
        cx.simulate_keystrokes(key);
        draw(cx);
    };
    let (at, end) = scrolled(cx);
    assert_eq!(at, px(0.), "the failure shows from its top");
    assert!(end > px(0.), "the failure overflows the box");
    // While it overflows it is a group named for what it holds: the Tab
    // stop's name and role, which assistive technology reads on reaching
    // it. The title below draws the same words, so a search for the name
    // alone finds that.
    crate::native_accessibility::written::forget();
    draw(cx);
    let group = crate::native_accessibility::written::get("series-preview-failure")
        .expect("the failure writes a node while it overflows")
        .node;
    assert_eq!(group.role(), Role::Group, "the failure is a group");
    assert_eq!(
        group.label(),
        Some(PREVIEW_UNAVAILABLE),
        "the group is named for the failure"
    );
    let line = cx.update(|window, _| crate::empty_state::block_height(1, false, window));
    let body = body_at(cx);
    press("down", cx);
    assert_eq!(scrolled(cx).0, line.min(end), "Down scrolls it a line");
    for _ in 0..40 {
        press("down", cx);
    }
    assert_eq!(scrolled(cx).0, end, "Down stops at its end");
    let (kept, last) = (
        rendered(cx, "series-file-preview"),
        rendered(cx, "series-preview-failed-line"),
    );
    assert!(
        last.bottom() <= kept.bottom() + px(0.25),
        "at its end, the error's last line ends at {:?} inside the box, {kept:?}",
        last.bottom()
    );
    press("up", cx);
    assert_eq!(
        scrolled(cx).0,
        (end - line).max(px(0.)),
        "Up scrolls it back a line"
    );
    assert_eq!(body_at(cx), body, "Up and Down move only the failure");
    assert!(failure_focused(&browser, cx), "Up and Down keep focus");
    // A resize that makes it fit leaves focus and the ring where they
    // are, until the next Tab.
    size_to(&app, cx, 13, 1480., 800.);
    assert_eq!(scrolled(cx).1, px(0.), "the failure fits the larger box");
    assert!(
        failure_focused(&browser, cx),
        "made to fit, it keeps focus until the next Tab"
    );
    assert!(
        cx.update(|window, _| window
            .context_stack()
            .iter()
            .any(|context| context.contains("Dialog"))),
        "made to fit, it is still drawn as what holds focus, so keys still reach the dialog"
    );
    let kept = rendered(cx, "series-file-preview");
    assert!(
        sweep::rings_edge(&outlines_in(cx, |p| p.accent), kept),
        "made to fit, it keeps its ring on the box's edge, {kept:?}"
    );
    press("tab", cx);
    assert!(
        !failure_focused(&browser, cx),
        "the next Tab leaves the failure that fits"
    );
    let mut walked = Vec::new();
    sweep::walk_each(
        &app,
        cx,
        "tab",
        "the failure once it fits",
        13,
        &mut walked,
        &mut |cx, stop, _| {
            assert!(
                !failure_focused(&browser, cx),
                "{stop}: a failure that fits is no Tab stop"
            );
        },
    );
    assert!(walked.is_empty(), "{}", walked.join("\n"));
}

/// A failure that fits the preview's box is no Tab stop, by Tab or by
/// Shift+Tab, and a press on it takes focus from nothing: there is
/// nothing in it for a key to reach.
#[gpui::test]
async fn a_short_preview_failure_is_no_tab_stop(cx: &mut gpui_kit::TestAppContext) {
    use crate::focus_ring::sweep;
    use crate::tags::tests::{draw, rendered, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = committed_notes(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    size_to(&app, cx, 13, 1480., 800.);
    let browser = failed_read(&app, cx, &repo, "The read's error");
    for key in ["tab", "shift-tab"] {
        let mut walked = Vec::new();
        sweep::walk_each(
            &app,
            cx,
            key,
            &format!("the short failure by {key}"),
            13,
            &mut walked,
            &mut |cx, stop, _| {
                assert!(
                    !failure_focused(&browser, cx),
                    "{stop}: a failure that fits is no Tab stop"
                );
            },
        );
        assert!(walked.is_empty(), "{}", walked.join("\n"));
    }
    let held = cx.update(|window, cx| window.focused(cx));
    let line = rendered(cx, "series-preview-failed-line");
    cx.simulate_click(line.center(), Modifiers::none());
    draw(cx);
    assert!(
        !failure_focused(&browser, cx),
        "a press on a failure that fits does not focus it"
    );
    assert_eq!(
        cx.update(|window, cx| window.focused(cx)),
        held,
        "a press on a failure that fits leaves focus where it was"
    );
}

/// The series' preview reads the same text as Compare, so a side whose
/// Git LFS object could not be used says so here too: the read no longer
/// fails for it, and only the pointer's text was left to say anything.
#[gpui::test]
async fn a_failing_lfs_side_says_why_in_the_series_preview(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    use crate::tags::tests::{git, tagged_repository, window};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let over_limit = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        "e".repeat(64),
        33 * 1024 * 1024
    );
    std::fs::write(repo.path().join("notes.txt"), &over_limit).unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "Before"]);
    std::fs::write(repo.path().join("notes.txt"), "plain after\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "After"]);
    let files = last_commit_files(&repo);
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let browser = inspecting(&app, cx, &files);
    written::forget();
    choose(&browser, cx, file_index(&files, "notes.txt"));
    assert_eq!(
        written::get("series-lfs-failure").map(|line| line.node.value().map(str::to_owned)),
        Some(Some(
            "Before: this Git LFS object is over the 32 MiB preview limit, so its pointer is \
             shown."
                .to_owned()
        )),
    );
    assert!(
        cx.debug_bounds("series-lfs-failure")
            .is_some_and(|line| line.size.height > px(0.)),
        "the line is drawn"
    );
}

/// A side whose Git LFS object is missing offers its download in the
/// review as in Compare, Download Before… alone where only the old
/// object is missing, in Diagrams and beneath a failing After's line in
/// Patch, its frame on the line's text edge at 13 and 18 pt; a press
/// opens the download's form over the review, and as the write it asks
/// for ends the file's preview is read again. The review said the object
/// was missing and offered nothing.
#[gpui::test]
async fn the_review_offers_a_missing_side_s_download(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    use crate::tags::tests::{git, tagged_repository, window};
    use sha2::{Digest, Sha256};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let store = |text: &str, bytes: &str| {
        let oid = gitturtle_core::lower_hex(Sha256::digest(text));
        let directory = repo
            .path()
            .join(".git/lfs/objects")
            .join(&oid[..2])
            .join(&oid[2..4]);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(&oid), bytes).unwrap();
    };
    let pointer = |text: &str| {
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            gitturtle_core::lower_hex(Sha256::digest(text)),
            text.len()
        )
    };
    let (diagram, damaged) = ("flowchart LR\n  A --> B\n", "flowchart LR\n  C --> D\n");
    store(diagram, diagram);
    store(damaged, &"x".repeat(damaged.len()));
    let gone = |name: &str| pointer(&format!("flowchart LR\n  {name} --> Gone\n"));
    let sides = [
        ("flow.mmd", gone("One"), pointer(diagram)),
        ("damaged.mmd", gone("Two"), pointer(damaged)),
    ];
    for (path, before, _) in &sides {
        std::fs::write(repo.path().join(path), before).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "Before"]);
    for (path, _, after) in &sides {
        std::fs::write(repo.path().join(path), after).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "After"]);
    let files = last_commit_files(&repo);
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(1480.), px(973.)));
    let browser = inspecting(&app, cx, &files);
    let mode = |mode: usize, cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = mode;
                browser.prepare_editor(window, cx);
                cx.notify();
            })
        });
        crate::tags::tests::draw(cx);
    };
    // Diagrams, only the old object missing: Download Before… alone.
    choose(&browser, cx, file_index(&files, "flow.mmd"));
    mode(3, cx);
    let button = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Before offers its download");
    assert!(
        cx.debug_bounds("download-lfs-side-1").is_none(),
        "After, whose object is local, offers none"
    );
    assert_eq!(
        written::get("download-lfs-side-0").and_then(|node| node.node.label().map(str::to_owned)),
        Some("Download Before…".to_owned()),
        "the review names the download as Compare does"
    );
    // Patch, beneath After's failure line: the frame on its text edge.
    choose(&browser, cx, file_index(&files, "damaged.mmd"));
    mode(0, cx);
    for text in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        crate::tags::tests::draw(cx);
        let line = cx
            .debug_bounds("series-lfs-failure-region")
            .expect("After's failure line");
        let button = cx
            .debug_bounds("download-lfs-side-0")
            .expect("Before offers its download beneath the line");
        let edge = cx.update(|window, _| window.rem_size() * 0.75);
        assert!(
            (button.left() - (line.left() + edge)).abs() <= px(0.25)
                && button.top() >= line.bottom() - px(0.25),
            "{text} pt: {button:?} stands its frame on the line's text edge, {edge:?} \
             into {line:?}, beneath it"
        );
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    // Back to Diagrams' file: a press opens the form over the review.
    choose(&browser, cx, file_index(&files, "flow.mmd"));
    mode(3, cx);
    let _ = button;
    let button = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Before offers its download");
    written::forget();
    cx.simulate_click(button.center(), Modifiers::none());
    crate::tags::tests::draw(cx);
    assert!(
        written::get("lfs-download-file")
            .and_then(|node| node.node.value().map(str::to_owned))
            .is_some_and(|file| file.contains("flow.mmd")),
        "the download's form opens for the file's object"
    );
    assert!(
        browser.read_with(cx, |browser, _| browser.asking.is_some()),
        "the review records the download it asked for"
    );
    // The write runs and ends: the file's preview is read again.
    // By its showing: a comparison read again may land at the address
    // the one it replaces was freed from, as with seed 2.
    let shown = |cx: &mut gpui::VisualTestContext| {
        browser.read_with(cx, |browser, _| {
            browser.content.is_some().then_some(browser.showing)
        })
    };
    let before = shown(cx);
    cx.update(|window, cx| window.close_dialog(cx));
    let me = crate::lfs_download::Asker::Review(browser.entity_id());
    cx.update(|window, cx| app.update(cx, |app, cx| app.simulate_lfs_download(me, window, cx)));
    crate::tags::tests::draw(cx);
    assert_eq!(shown(cx), before, "nothing is read while the write runs");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.simulate_lfs_download_end(None, window, cx)
        })
    });
    browser.read_with(cx, |browser, _| browser.worker.wait_idle());
    crate::tags::tests::draw(cx);
    assert!(
        shown(cx).is_some() && shown(cx) != before,
        "the file's preview is read again as the write ends"
    );
    assert!(
        browser.read_with(cx, |browser, _| browser.lfs_download.is_none()),
        "and the review follows it no further"
    );
}

/// A Git LFS fixture whose `flow.mmd` lost only its old object, its new
/// object in the local store, with the changed files of its last commit.
fn missing_before() -> (tempfile::TempDir, GitRepository, Vec<FileChange>) {
    missing_before_with(&[])
}

/// [`missing_before`], its last commit also adding each of `others`, a
/// text file, so the review can show another file of that commit.
fn missing_before_with(others: &[&str]) -> (tempfile::TempDir, GitRepository, Vec<FileChange>) {
    missing_before_of(&["flow.mmd"], others)
}

/// [`missing_before_with`] where each of `lost` is a pointer like
/// `flow.mmd`'s, each having lost its own old object.
fn missing_before_of(
    lost: &[&str],
    others: &[&str],
) -> (tempfile::TempDir, GitRepository, Vec<FileChange>) {
    use crate::tags::tests::{git, tagged_repository};
    use sha2::{Digest, Sha256};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pointer = |text: &str| {
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            gitturtle_core::lower_hex(Sha256::digest(text)),
            text.len()
        )
    };
    let diagram = "flowchart LR\n  A --> B\n";
    let oid = gitturtle_core::lower_hex(Sha256::digest(diagram));
    let directory = repo
        .path()
        .join(".git/lfs/objects")
        .join(&oid[..2])
        .join(&oid[2..4]);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(&oid), diagram).unwrap();
    for (index, name) in lost.iter().enumerate() {
        let gone = format!("flowchart LR\n  One --> Gone{}\n", " again".repeat(index));
        std::fs::write(repo.path().join(name), pointer(&gone)).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "Before"]);
    for name in lost {
        std::fs::write(repo.path().join(name), pointer(diagram)).unwrap();
    }
    for other in others {
        std::fs::write(repo.path().join(other), "Notes\n").unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "After"]);
    let files = last_commit_files(&repo);
    (fixture, repo, files)
}

/// The review of `app`'s repository inspecting `files`, `flow.mmd`
/// chosen in Diagrams, which offers Download Before….
fn reviewing_download(
    app: &Entity<GitTurtle>,
    cx: &mut gpui::VisualTestContext,
    files: &[FileChange],
) -> Entity<SeriesBrowser> {
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(1480.), px(973.)));
    let browser = inspecting(app, cx, files);
    choose(&browser, cx, file_index(files, "flow.mmd"));
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.text_mode = 3;
            browser.prepare_editor(window, cx);
            cx.notify();
        })
    });
    crate::tags::tests::draw(cx);
    browser
}

/// Whether `which` of `browser`'s focus handles holds focus.
fn holds_focus(
    browser: &Entity<SeriesBrowser>,
    cx: &mut gpui::VisualTestContext,
    which: fn(&SeriesBrowser) -> &FocusHandle,
) -> bool {
    cx.update(|window, cx| which(browser.read(cx)).is_focused(window))
}

/// The review covers the operation bar, so the download it asked for
/// says its progress, offers its Cancel and says how it failed in the
/// review's own Download row: once the write looks busy, where the
/// pressed Download stood, the row keeping its height and place, focus
/// moving to Cancel; then a failure's words, announced at once, and the
/// Download back with focus. The review showed none of them, while its
/// confirmation said the download could be cancelled from the operation
/// bar, which it covered.
#[gpui::test]
async fn the_review_s_download_says_its_progress_cancel_and_outcome(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::native_accessibility::written;
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    let pressed = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered");
    // Pressed from the keyboard, as a mouse press takes no focus.
    cx.update(|window, cx| browser.read(cx).download_focus[0].clone().focus(window, cx));
    crate::tags::tests::draw(cx);
    enter(cx, true);
    enter(cx, false);
    assert!(
        cx.debug_bounds("lfs-download-file").is_some(),
        "Enter on Download Before… opens its form"
    );
    // Its form gives way to its confirmation, whose write runs for the
    // review, and focus comes back to the Download pressed.
    cx.update(|window, cx| window.close_dialog(cx));
    crate::tags::tests::draw(cx);
    assert!(
        holds_focus(&browser, cx, |browser| &browser.download_focus[0]),
        "focus is back on Download Before…"
    );
    let me = crate::lfs_download::Asker::Review(browser.entity_id());
    cx.update(|window, cx| app.update(cx, |app, cx| app.simulate_lfs_download(me, window, cx)));
    crate::tags::tests::draw(cx);
    assert!(
        cx.debug_bounds("lfs-download-progress").is_none()
            && cx.debug_bounds("download-lfs-side-0").is_some(),
        "through the busy delay the row stands as it was"
    );
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    crate::tags::tests::draw(cx);
    assert!(
        cx.debug_bounds("download-lfs-side-0").is_none(),
        "the pressed Download gives its place"
    );
    let progress = cx
        .debug_bounds("lfs-download-progress")
        .expect("the window's progress stands where it stood");
    let cancel = cx
        .debug_bounds("cancel-lfs-download")
        .expect("beside Cancel download");
    assert!(
        (progress.left() - pressed.left()).abs() <= px(0.5),
        "where the Download stood: {progress:?}, {pressed:?}"
    );
    assert!(
        (cancel.top() - pressed.top()).abs() <= px(0.5)
            && (cancel.size.height - pressed.size.height).abs() <= px(0.5),
        "the row keeps its height and place: {cancel:?}, {pressed:?}"
    );
    assert_eq!(
        written::get("lfs-download-progress")
            .and_then(|node| node.node.label().map(str::to_owned))
            .as_deref(),
        Some("Download object"),
        "it says the operation's busy label"
    );
    assert!(
        holds_focus(&browser, cx, |browser| &browser.download_cancel),
        "focus moves from the vanished Download to Cancel"
    );
    // Unpressed, as a download the person cancelled is no failure
    // (`a_download_the_person_cancels_ends_as_no_failure`).
    let words = "Git LFS could not reach origin";
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.simulate_lfs_download_end(Some(words), window, cx)
        })
    });
    crate::tags::tests::draw(cx);
    cx.debug_bounds("lfs-download-failure")
        .expect("the failure is said in the row");
    assert_eq!(
        written::get("lfs-download-failure")
            .and_then(|node| node.node.value().map(str::to_owned))
            .as_deref(),
        Some(words),
        "in its own words"
    );
    assert!(
        cx.debug_bounds("download-lfs-side-0").is_some()
            && holds_focus(&browser, cx, |browser| &browser.download_focus[0]),
        "Download Before… returns, with focus"
    );
    let (_, alerted) = crate::copy_feedback::tests::alerted(cx).expect("the failure is announced");
    assert_eq!(alerted, words, "at once");
}

/// The review's download runs for the review through its own form and
/// confirmation, whose Review download records who asked: driven from
/// the keyboard, Tab onto Download Before… and Enter, then the form's
/// Review download and the confirmation's Download object, the write
/// runs for the review, the confirmation says where its Cancel stands,
/// focus stays on the Download through the busy delay and moves to
/// Cancel as the progress shows. Every other test said who asked
/// itself, so with the form's record gone the write ran for Compare,
/// whose header the review covers, and every test stayed green.
#[gpui::test]
async fn the_review_s_form_and_confirmation_run_its_download(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    // The write waits, as a transfer would, while the test looks.
    let held = app.read_with(cx, |app, _| app.operations.hold());
    for _ in 0..40 {
        if holds_focus(&browser, cx, |browser| &browser.download_focus[0]) {
            break;
        }
        cx.simulate_keystrokes("tab");
        crate::tags::tests::draw(cx);
    }
    assert!(
        holds_focus(&browser, cx, |browser| &browser.download_focus[0]),
        "Tab reaches Download Before…"
    );
    enter(cx, true);
    enter(cx, false);
    assert!(
        cx.debug_bounds("lfs-download-file").is_some(),
        "Enter on Download Before… opens its form"
    );
    app.update(cx, |app, cx| app.stand_in_plan("origin", cx));
    crate::tags::tests::draw(cx);
    let review = cx
        .debug_bounds("dialog-ok: Review download")
        .expect("the form offers Review download");
    cx.simulate_click(review.center(), Modifiers::none());
    crate::tags::tests::draw(cx);
    let consequences = written::get("operation-consequences")
        .and_then(|node| node.node.value().or(node.node.label()).map(str::to_owned))
        .expect("the confirmation says the download's consequences");
    assert!(
        consequences
            .contains("While it runs, Cancel download stands in the review's Download row."),
        "it says where the review's Cancel stands: {consequences}"
    );
    let confirm = cx
        .debug_bounds("dialog-ok: Download object")
        .expect("the confirmation offers Download object");
    cx.simulate_click(confirm.center(), Modifiers::none());
    crate::tags::tests::draw(cx);
    let me = crate::lfs_download::Asker::Review(browser.entity_id());
    assert_eq!(
        app.read_with(cx, |app, _| app.lfs_downloads.running()),
        Some(me),
        "the write runs for the review"
    );
    assert!(
        cx.debug_bounds("lfs-download-progress").is_none()
            && holds_focus(&browser, cx, |browser| &browser.download_focus[0]),
        "through the busy delay focus is back on Download Before…"
    );
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    crate::tags::tests::draw(cx);
    assert!(
        cx.debug_bounds("lfs-download-progress").is_some()
            && cx.debug_bounds("cancel-lfs-download").is_some(),
        "the progress and Cancel download stand in the review's row"
    );
    assert!(
        holds_focus(&browser, cx, |browser| &browser.download_cancel),
        "focus moves to Cancel download as the progress shows"
    );
    // The stand-in's write refuses as it runs, before any command.
    drop(held);
    app.read_with(cx, |app, _| app.operations.drain());
    crate::tags::tests::draw(cx);
    assert!(
        app.read_with(cx, |app, _| app.lfs_downloads.running())
            .is_none(),
        "the write ends"
    );
}

/// The review's Download row in QA's window, 18 pt in 461 × 490 at
/// scales 2 and 1.2, gives way in Compare's order: beneath a failing
/// After's line in Patch, the line gives way first, the row keeps its
/// height, its sentence in the button's tooltip and description, and the
/// comparison keeps one whole row inside the preview's box, whether the
/// row rests, its download runs or it failed. A failure's words stand
/// on the button's line and end in an ellipsis, whole in their tooltip,
/// so a failure landing never grows the row: wrapped, they took a line
/// of their own, about 25 px at 18 pt, as they landed. With both sides'
/// objects missing, the buttons' word gives way, Cancel download's with
/// it, and the two share a line in every state, the sentence given way,
/// their whole names their accessible names, the comparison keeping a
/// whole row (kept "Cancel download", whose running form left them no
/// shared line, they stacked with their word and the comparison's least
/// row ended 1.5 px past the box's foot). Where they share one with
/// their word, at
/// 18 pt in a 1,000 × 680 window, the sentence a measurement stands
/// beside them stands on their line, ending inside the row.
#[gpui::test]
async fn the_review_s_download_row_keeps_its_height_and_a_row_at_18_pt(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::{draw, git, tagged_repository, window};
    use sha2::{Digest, Sha256};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let pointer = |text: &str| {
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            gitturtle_core::lower_hex(Sha256::digest(text)),
            text.len()
        )
    };
    // After's object stored with the wrong bytes: damaged.
    let damaged = "flowchart LR\n  C --> D\n";
    let oid = gitturtle_core::lower_hex(Sha256::digest(damaged));
    let directory = repo
        .path()
        .join(".git/lfs/objects")
        .join(&oid[..2])
        .join(&oid[2..4]);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(&oid), "x".repeat(damaged.len())).unwrap();
    let gone = |name: &str| pointer(&format!("flowchart LR\n  {name} --> Gone\n"));
    let sides = [
        ("damaged.mmd", gone("One"), pointer(damaged)),
        ("both.mmd", gone("Two"), gone("Three")),
    ];
    for (path, before, _) in &sides {
        std::fs::write(repo.path().join(path), before).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "Before"]);
    for (path, _, after) in &sides {
        std::fs::write(repo.path().join(path), after).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "After"]);
    let files = last_commit_files(&repo);
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    let browser = inspecting(&app, cx, &files);
    let patch = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
    };
    // The preview lies below the body's fold here: its own handle,
    // focused, reveals it, as Tab into it does.
    let reveal = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            let body = &browser.read(cx).body;
            body.reveal_next_focus();
            body.focus("series-file-preview", cx).focus(window, cx);
        });
        draw(cx);
        draw(cx);
    };
    // The row's bounds, its first button's, and the comparison's,
    // checked to stand in order inside the preview's box.
    let drawn = |at: &str, cx: &mut gpui::VisualTestContext| {
        let device = cx.update(|window, _| px(1. / window.scale_factor()));
        let floor = cx.update(|window, _| crate::views::comparison_floor(1., window));
        let preview = cx
            .debug_bounds("series-file-preview")
            .expect("the preview's box");
        let button = cx
            .debug_bounds("download-lfs-side-0")
            .or_else(|| cx.debug_bounds("cancel-lfs-download"))
            .expect("the row's first control");
        let body = cx
            .debug_bounds("lfs-comparison")
            .expect("the comparison beneath the row");
        if let Some(line) = cx.debug_bounds("series-lfs-failure-region") {
            assert!(
                line.top() >= preview.top() - device / 2.
                    && line.bottom() <= button.top() + device / 2.,
                "{at}: the line, {line:?}, stands above the row, {button:?}, in {preview:?}"
            );
        }
        assert!(
            button.bottom() <= body.top() + device / 2.
                && body.size.height >= floor - device / 2.
                && body.bottom() <= preview.bottom() + device / 2.,
            "{at}: beneath {button:?} the comparison, {body:?}, keeps a whole row, \
             {floor:?}, inside {preview:?}"
        );
        (button, body, device)
    };
    choose(&browser, cx, file_index(&files, "damaged.mmd"));
    patch(cx);
    for scale in [2.0f32, 1.2] {
        cx.update(|window, _| window.set_scale_factor(scale));
        reveal(cx);
        drawn(&format!("damaged.mmd at scale {scale}"), cx);
        assert!(
            cx.debug_bounds("lfs-preview-download-explanation")
                .is_none(),
            "scale {scale}: beside the line the sentence moves to the button"
        );
    }
    cx.update(|window, _| window.set_scale_factor(2.));
    reveal(cx);
    let (pressed, rest, device) = drawn("resting", cx);
    // Running: the progress and Cancel where Download stood.
    start_download(&app, &browser, cx, pressed.center());
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    draw(cx);
    draw(cx);
    let (cancel, body, _) = drawn("running", cx);
    cx.debug_bounds("lfs-download-progress")
        .expect("the progress stands in the row");
    assert!(
        (cancel.top() - pressed.top()).abs() <= device / 2.
            && (body.top() - rest.top()).abs() <= device / 2.,
        "running, the row keeps its height: {cancel:?} for {pressed:?}, {body:?} for {rest:?}"
    );
    // Failed: words longer than the row ends on the button's line.
    let words = "Git LFS could not reach origin: the remote refused the batch \
                 request for this object after three attempts";
    end_download(&app, &browser, cx, Some(words));
    draw(cx);
    let (button, body, _) = drawn("failed", cx);
    let failure = cx
        .debug_bounds("lfs-download-failure")
        .expect("the failure is said in the row");
    // After the button, which keeps the place where it was pressed.
    assert!(
        (failure.center().y - button.center().y).abs() <= device / 2.
            && failure.left() >= button.right() - device / 2.
            && (button.left() - pressed.left()).abs() <= device / 2.,
        "the failure's words, {failure:?}, stand after the button on its line, \
         {button:?}, which stays where it was pressed, {pressed:?}"
    );
    assert!(
        (button.top() - pressed.top()).abs() <= device / 2.
            && (body.top() - rest.top()).abs() <= device / 2.,
        "a failure landing keeps the row's height: {button:?} for {pressed:?}, {body:?} \
         for {rest:?}"
    );
    // Both sides missing: the review keeps each side's widest state, its
    // Download or the running form, so in the preview's 358 px at scale
    // 2 the buttons' word gives way, and with it Cancel download's, the
    // progress and a failure's words giving way first, and the two share
    // a line in every state, their whole names their accessible names
    // (`both_sides_keep_their_lines_as_either_runs_or_fails_at_18_pt`):
    // beneath them the comparison keeps a whole row inside the box.
    use crate::native_accessibility::written;
    let both = |fits: Option<bool>, cx: &mut gpui::VisualTestContext| {
        choose(&browser, cx, file_index(&files, "both.mmd"));
        crate::lfs_download::SENTENCE_FITS.set(fits);
        patch(cx);
        reveal(cx);
        let sentence = cx.debug_bounds("lfs-preview-download-explanation");
        let before = cx.debug_bounds("download-lfs-side-0").expect("Before");
        let after = cx.debug_bounds("download-lfs-side-1").expect("After");
        crate::lfs_download::SENTENCE_FITS.set(None);
        let names = ["download-lfs-side-0", "download-lfs-side-1"]
            .map(|id| written::get(id).and_then(|node| node.node.label().map(str::to_owned)));
        (sentence, before, after, names)
    };
    let (sentence, before, after, names) = both(None, cx);
    let inset = cx.update(|window, _| window.pixel_snap(window.rem_size() * 0.75));
    let preview = cx
        .debug_bounds("series-file-preview")
        .expect("the preview's box");
    assert!(
        sentence.is_none()
            && (after.center().y - before.center().y).abs() <= device / 2.
            && before.right() <= after.left() + device / 2.
            && after.right() <= preview.right() - inset + device / 2.,
        "the sentence, {sentence:?}, gives way and {before:?} and {after:?} share a line \
         inside {preview:?}"
    );
    assert_eq!(
        names,
        ["Download Before…", "Download After…"].map(|name| Some(name.to_owned())),
        "the whole names stay their accessible names"
    );
    let floor = cx.update(|window, _| crate::views::comparison_floor(1., window));
    let body = cx
        .debug_bounds("lfs-comparison")
        .expect("the comparison beneath the row");
    assert!(
        after.bottom() <= body.top() + device / 2.
            && body.size.height >= floor - device / 2.
            && body.bottom() <= preview.bottom() + device / 2.,
        "beneath {after:?} the comparison, {body:?}, keeps a whole row, {floor:?}, inside \
         {preview:?}"
    );
    // Where they share a line with their word, at 18 pt in a 1,000 ×
    // 680 window, the sentence a measurement stands beside them stands
    // there on their line, ending inside the row.
    cx.simulate_resize(size(px(1000.), px(680.)));
    let (sentence, before, after, _) = both(Some(true), cx);
    let preview = cx
        .debug_bounds("series-file-preview")
        .expect("the preview's box");
    let sentence = sentence.expect("the sentence is stood beside the buttons");
    assert!(
        sentence.left() >= preview.left() + inset - device / 2.
            && sentence.right() <= before.left() + device / 2.
            && before.right() <= after.left() + device / 2.
            && after.right() <= preview.right() - inset + device / 2.
            && (before.center().y - sentence.center().y).abs() <= device / 2.
            && (after.center().y - sentence.center().y).abs() <= device / 2.,
        "{sentence:?} stands beside {before:?} and {after:?} on one line, inside {preview:?}"
    );
    drawn("both sides at 1,000 × 680", cx);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A repository whose last commit changes `both.mmd`, a Git LFS pointer
/// whose objects on both sides are missing from the local store, and
/// that commit's changed files.
fn missing_both() -> (tempfile::TempDir, GitRepository, Vec<FileChange>) {
    missing_both_with(&[])
}

/// [`missing_both`], its last commit also adding each of `others`, a
/// text file, so the review can show another file of that commit.
fn missing_both_with(others: &[&str]) -> (tempfile::TempDir, GitRepository, Vec<FileChange>) {
    use crate::tags::tests::{git, tagged_repository};
    use sha2::{Digest, Sha256};
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    for (message, name) in [("Before", "Two"), ("After", "Three")] {
        let text = format!("flowchart LR\n  {name} --> Gone\n");
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            gitturtle_core::lower_hex(Sha256::digest(&text)),
            text.len()
        );
        std::fs::write(repo.path().join("both.mmd"), pointer).unwrap();
        if message == "After" {
            for other in others {
                std::fs::write(repo.path().join(other), "Notes\n").unwrap();
            }
        }
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "--quiet", "-m", message]);
    }
    let files = last_commit_files(&repo);
    (fixture, repo, files)
}

/// The review's two Download buttons share a line only where they would
/// in every state its row takes: each side in its widest state, its
/// Download or its running form, one side running at a time, and a
/// failure's words kept beside them (`lfs_download::buttons_share_line`).
/// With both sides' objects missing at 18 pt in QA's 461 × 490 window at
/// scale 2, the comparison's row takes the buttons' word, and with it
/// Cancel download's, which reads "Cancel" (its accessible name and
/// tooltip saying what it cancels), the progress and a failure's words
/// giving way first: so the two sides share one line at rest, as either
/// side's download runs and as it fails, Cancel and the other Download
/// apart, and the comparison keeps a whole row inside the preview's box
/// throughout. Pressed, Cancel reads "Cancelling…", as wide as before,
/// and no longer cancels. In a 600 × 490 window the progress stays whole
/// and the two share a line in every state.
/// Kept "Cancel download", whose running form left the two no shared
/// line, the two stacked in every state and the comparison's row ended
/// 1.5 px past the box's foot; before that, decided from the Downloads
/// alone, Before's running form drew Cancel over After… by 7 px.
#[gpui::test]
async fn both_sides_keep_their_lines_as_either_runs_or_fails_at_18_pt(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_both();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.update(|window, _| window.set_scale_factor(2.));
    let browser = inspecting(&app, cx, &files);
    let device = cx.update(|window, _| px(1. / window.scale_factor()));
    let floor = cx.update(|window, _| crate::views::comparison_floor(1., window));
    let whole = cx.update(|window, _| {
        crate::views::laid_out_text_width(
            crate::views::line_widths(&[("Download object", appearance::ui_text(12.))], window),
            window,
        )
    });
    // Patch, its preview revealed below the body's fold by its own
    // handle, focused, as Tab into it does.
    let shown = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let body = &browser.body;
                body.reveal_next_focus();
                body.focus("series-file-preview", cx).focus(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
    };
    let apart = |a: Bounds<Pixels>, b: Bounds<Pixels>| {
        a.right() <= b.left() + device / 2.
            || b.right() <= a.left() + device / 2.
            || a.bottom() <= b.top() + device / 2.
            || b.bottom() <= a.top() + device / 2.
    };
    let one_line = |parts: &[Bounds<Pixels>]| {
        parts
            .iter()
            .all(|part| (part.center().y - parts[0].center().y).abs() <= device / 2.)
    };
    // The comparison beneath the row keeps a whole row inside the box.
    let comparison = |at: &str, cx: &mut gpui::VisualTestContext| {
        let preview = cx
            .debug_bounds("series-file-preview")
            .expect("the preview's box");
        let body = cx
            .debug_bounds("lfs-comparison")
            .expect("the comparison beneath the row");
        assert!(
            body.size.height >= floor - device / 2.
                && body.bottom() <= preview.bottom() + device / 2.,
            "{at}: the comparison, {body:?}, keeps a whole row, {floor:?}, inside \
             {preview:?}"
        );
        body
    };
    let label = || {
        crate::views::COMPACT_LABELS
            .with(|labels| labels.borrow().get("cancel-lfs-download").cloned())
    };
    let words = "Git LFS could not reach origin: the remote refused the batch request";
    for width in [461., 600.] {
        cx.simulate_resize(size(px(width), px(490.)));
        choose(&browser, cx, file_index(&files, "both.mmd"));
        shown(cx);
        let downloads = |cx: &mut gpui::VisualTestContext| {
            ["download-lfs-side-0", "download-lfs-side-1"].map(|id| cx.debug_bounds(id))
        };
        let rest = downloads(cx).map(|side| side.expect("both sides offer a download"));
        let body = comparison(&format!("{width} px wide, at rest"), cx);
        let shares = one_line(&rest);
        assert!(shares, "at {width} px the two share a line: {rest:?}");
        for side in [0, 1] {
            let at = format!("{width} px wide, side {side}");
            // Where it stands now: a failure's words before the two on
            // their shared line move them along it.
            let pressed = downloads(cx)[side].expect("the side's Download");
            start_download(&app, &browser, cx, pressed.center());
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            shown(cx);
            let progress = cx
                .debug_bounds("lfs-download-progress")
                .expect("the progress stands in the row");
            let cancel = cx
                .debug_bounds("cancel-lfs-download")
                .expect("beside Cancel download");
            let other = downloads(cx)[1 - side].expect("the other side's Download");
            assert!(
                progress.size.width > Pixels::ZERO
                    && apart(progress, cancel)
                    && apart(cancel, other)
                    && apart(progress, other)
                    && (!shares || one_line(&[progress, cancel, other])),
                "{at}: the progress, {progress:?}, Cancel, {cancel:?}, and the other \
                 Download, {other:?}, stand apart, on one line where the two share it"
            );
            if width > 461. {
                assert!(
                    progress.size.width >= whole - device / 2.,
                    "{at}: the progress, {progress:?}, is whole, {whole:?}"
                );
            } else {
                let name = ["Before", "After"][side];
                assert_eq!(
                    (
                        label().as_ref().map(|label| label.to_string()),
                        written::get("cancel-lfs-download")
                            .and_then(|node| node.node.label().map(str::to_owned)),
                    ),
                    (
                        Some("Cancel".to_owned()),
                        Some(format!("Cancel download of both.mmd's {name} object")),
                    ),
                    "{at}: Cancel's word gives way with the Downloads', its name whole"
                );
            }
            let running = comparison(&format!("{at}, running"), cx);
            assert!(
                (running.top() - body.top()).abs() <= device / 2.
                    && (other.top() - rest[1 - side].top()).abs() <= device / 2.,
                "{at}: running, the row keeps its height and lines: {running:?} for \
                 {body:?}, {other:?} for {:?}",
                rest[1 - side]
            );
            if width == 461. && side == 0 {
                // Pressed, it reads "Cancelling…" at its width and no
                // longer cancels.
                cx.simulate_click(cancel.center(), Modifiers::none());
                shown(cx);
                assert_eq!(
                    (
                        label().as_ref().map(|label| label.to_string()),
                        cx.debug_bounds("cancel-lfs-download"),
                        written::get("cancel-lfs-download").map(|node| node.node.is_disabled()),
                    ),
                    (Some("Cancelling…".to_owned()), Some(cancel), Some(true)),
                    "{at}: pressed, Cancel reads Cancelling… where it stood, unpressable"
                );
                // Ended by that press, the download is no failure: the
                // row is back at rest, both Downloads where they stood.
                // Started again, it fails below.
                end_download(&app, &browser, cx, Some(words));
                shown(cx);
                let back = downloads(cx).map(|side| side.expect("both Downloads are back"));
                assert!(
                    cx.debug_bounds("lfs-download-failure").is_none()
                        && crate::lfs_download::drawn_glyph(true, side).as_deref()
                            != Some("icons/warning.svg")
                        && back.iter().zip(&rest).all(|(back, rest)| {
                            (back.left() - rest.left()).abs() <= device / 2.
                                && (back.top() - rest.top()).abs() <= device / 2.
                        }),
                    "{at}: cancelled, the row is at rest, {back:?}, as it stood, {rest:?}"
                );
                start_download(&app, &browser, cx, pressed.center());
                cx.executor()
                    .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
                shown(cx);
            }
            end_download(&app, &browser, cx, Some(words));
            shown(cx);
            // The failed Download's warning glyph, and its words where
            // they keep "Failed…".
            let sides = downloads(cx).map(|side| side.expect("both Downloads are back"));
            assert_eq!(
                crate::lfs_download::drawn_glyph(true, side).as_deref(),
                Some("icons/warning.svg"),
                "{at}: the failed Download draws the warning glyph"
            );
            if let Some(failure) = cx.debug_bounds("lfs-download-failure") {
                assert!(
                    failure.size.width > Pixels::ZERO
                        && sides.iter().all(|side| apart(failure, *side))
                        && (!shares || one_line(&[failure, sides[0], sides[1]])),
                    "{at}: the failure's words, {failure:?}, stand beside {sides:?}"
                );
            }
            let ended = comparison(&format!("{at}, failed"), cx);
            assert!(
                (ended.top() - body.top()).abs() <= device / 2.
                    && sides
                        .iter()
                        .zip(&rest)
                        .all(|(side, rest)| (side.top() - rest.top()).abs() <= device / 2.),
                "{at}: failed, the row keeps its height and lines: {ended:?} for {body:?}, \
                 {sides:?} for {rest:?}"
            );
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A failure's words stand on the line of the side that failed, where
/// the person pressed: with both sides' objects missing at 18 pt in a
/// 461 × 680 window, where the two Downloads keep their word and stack,
/// After…'s failure stands after Download After…, which keeps the place
/// where it was pressed, and while it is drawn that Download's
/// accessible description says it ("Failed: …"), Before… keeping the
/// row's sentence. The words stood beside the first line's Download
/// Before…, which had not failed, and then before Download After…,
/// which moved right by their width as it failed, its ring with it.
#[gpui::test]
async fn a_failure_stands_beside_the_download_that_failed(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_both();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(461.), px(680.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.update(|window, _| window.set_scale_factor(2.));
    let browser = inspecting(&app, cx, &files);
    let device = cx.update(|window, _| px(1. / window.scale_factor()));
    choose(&browser, cx, file_index(&files, "both.mmd"));
    let shown = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let body = &browser.body;
                body.reveal_next_focus();
                body.focus("series-file-preview", cx).focus(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
    };
    shown(cx);
    let downloads = |cx: &mut gpui::VisualTestContext| {
        ["download-lfs-side-0", "download-lfs-side-1"]
            .map(|id| cx.debug_bounds(id).unwrap_or_else(|| panic!("{id}")))
    };
    let [before, pressed] = downloads(cx);
    assert!(
        pressed.top() >= before.bottom() - device / 2.,
        "the two stack: {before:?}, {pressed:?}"
    );
    start_download(&app, &browser, cx, pressed.center());
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    shown(cx);
    let words = "Git LFS could not reach origin";
    end_download(&app, &browser, cx, Some(words));
    shown(cx);
    let failure = cx
        .debug_bounds("lfs-download-failure")
        .expect("the failure is said in the row");
    let [before, after] = downloads(cx);
    assert!(
        (failure.center().y - after.center().y).abs() <= device / 2.
            && failure.left() >= after.right() - device / 2.,
        "the words, {failure:?}, stand after Download After…, {after:?}, not \
         {before:?}"
    );
    assert!(
        (after.left() - pressed.left()).abs() <= device / 2.,
        "Download After…, {after:?}, keeps the place where it was pressed, {pressed:?}"
    );
    let described =
        |id: &str| written::get(id).and_then(|node| node.node.description().map(str::to_owned));
    assert_eq!(
        [
            described("download-lfs-side-0"),
            described("download-lfs-side-1")
        ],
        [
            Some(crate::lfs_download::DOWNLOAD_EXPLANATION.to_owned()),
            Some(format!("Failed: {words}")),
        ],
        "the Download that failed says the words it stands beside"
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The glyphs both sides' Downloads draw where only side `failed`'s
/// failure stands: its warning glyph, the other's pull glyph.
fn glyphs_with(failed: Option<usize>) -> [Option<String>; 2] {
    [0, 1].map(|side| {
        Some(if Some(side) == failed {
            "icons/warning.svg".to_owned()
        } else {
            "icons/pull.svg".to_owned()
        })
    })
}

/// A failure never shrinks to a bare ellipsis (decision, 2026-11-10):
/// with both sides' objects missing at 18 pt in QA's 461 × 490 window at
/// scale 2, where the two Downloads share a line without their word, a
/// long failure of After… leaves its words no room for "After:
/// Failed…", so nothing is drawn for them: Download After…'s warning
/// glyph, in place of its pull glyph, is the last thing standing, the
/// whole words in its tooltip and accessible description, said nowhere
/// else on screen. At 13 pt in a 1480 × 973 window the words are drawn,
/// naming the side that failed, as two sides on one line would not tell
/// which did. The words were drawn as "…".
#[gpui::test]
async fn a_failure_never_shrinks_to_a_bare_ellipsis(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_both();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.update(|window, _| window.set_scale_factor(2.));
    let browser = inspecting(&app, cx, &files);
    choose(&browser, cx, file_index(&files, "both.mmd"));
    let shown = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let body = &browser.body;
                body.reveal_next_focus();
                body.focus("series-file-preview", cx).focus(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
    };
    shown(cx);
    let after = cx
        .debug_bounds("download-lfs-side-1")
        .expect("Download After… is offered");
    start_download(&app, &browser, cx, after.center());
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    shown(cx);
    let words = "Git LFS could not reach origin: the remote refused the batch request \
                 for this object because its credentials have expired";
    end_download(&app, &browser, cx, Some(words));
    shown(cx);
    assert!(
        cx.debug_bounds("lfs-download-failure").is_none(),
        "the words, without room for \"After: Failed…\", are not drawn"
    );
    assert_eq!(
        [0, 1].map(|side| crate::lfs_download::drawn_glyph(true, side)),
        glyphs_with(Some(1)),
        "Download After…'s warning glyph stands for them"
    );
    assert!(
        hover_shows(
            cx,
            "download-lfs-side-1",
            &format!("Download After… · Failed: {words}")
        ),
        "its tooltip says the whole words"
    );
    assert_eq!(
        written::get("download-lfs-side-1")
            .and_then(|node| node.node.description().map(str::to_owned)),
        Some(format!("Failed: {words}")),
        "Download After…'s accessible description says them"
    );
    // The test platform sets scale 2 on every resize: the scale after it.
    cx.simulate_resize(size(px(1480.), px(973.)));
    cx.update(|window, _| window.set_scale_factor(1.));
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    assert_eq!(cx.update(|window, _| window.scale_factor()), 1.);
    shown(cx);
    let failure = cx
        .debug_bounds("lfs-download-failure")
        .expect("the words are drawn");
    // They take the sentence's place at its width, which keeps the side's
    // name and "Failed…".
    let device = cx.update(|window, _| px(1. / window.scale_factor()));
    let [slot, least] = cx.update(|window, _| {
        [crate::lfs_download::DOWNLOAD_EXPLANATION, "After: Failed…"].map(|text| {
            crate::views::laid_out_text_width(
                crate::views::line_widths(&[(text, appearance::ui_text(12.))], window),
                window,
            )
        })
    });
    assert!(
        (failure.size.width - slot).abs() <= device / 2.
            && failure.size.width >= least - device / 2.,
        "at 13 pt in a 1480 × 973 window the words, {failure:?}, stand in the sentence's \
         place, {slot:?} wide, at least {least:?}"
    );
    assert_eq!(
        written::get("lfs-download-failure").and_then(|node| node.node.value().map(str::to_owned)),
        Some(format!("After: {words}")),
        "beside both sides' Downloads the words name the side that failed"
    );
}

/// A failure never moves a Download, and the failed side's own Download
/// says which side failed (decisions, 2026-11-10): with both sides'
/// objects missing, at 18 pt in QA's 461 × 490 window at scale 2, where
/// the two share a line without their word and a long failure's words
/// have no room, and at 13 pt in a 1480 × 973 window, where the sentence
/// leads the line at rest and the words take its place at its width, a
/// failure of After… and then one of Before… leave both Downloads where
/// they stood at rest, the failed side's Download drawing the warning
/// glyph in place of its pull glyph, the other its pull glyph; once the
/// failure clears, as the person presses Download Before… again, its
/// glyph is the pull glyph again. Put before them, the failure's icon
/// moved both right by its width and the gap, at 13 pt the long words
/// pushed them to the row's end, and then, after them, a Before…
/// failure's icon stood after Download After… and read as After…'s.
#[gpui::test]
async fn a_failure_moves_neither_download_and_marks_its_own(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_both();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(1480.), px(973.)));
    let initial = cx.update(|window, _| window.scale_factor());
    let browser = inspecting(&app, cx, &files);
    choose(&browser, cx, file_index(&files, "both.mmd"));
    let shown = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let body = &browser.body;
                body.reveal_next_focus();
                body.focus("series-file-preview", cx).focus(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
    };
    let downloads = |cx: &mut gpui::VisualTestContext| {
        ["download-lfs-side-0", "download-lfs-side-1"]
            .map(|id| cx.debug_bounds(id).unwrap_or_else(|| panic!("{id}")))
    };
    let kept =
        |rest: &[gpui::Bounds<Pixels>; 2], now: &[gpui::Bounds<Pixels>; 2], device: Pixels| {
            rest.iter()
                .zip(now)
                .all(|(rest, now)| (rest.left() - now.left()).abs() <= device / 2.)
        };
    let narrow = |cx: &mut gpui::VisualTestContext| {
        cx.simulate_resize(size(px(461.), px(490.)));
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.update(|window, _| window.set_scale_factor(2.));
    };
    let wide = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, _| window.set_scale_factor(initial));
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1480.), px(973.)));
    };
    shown(cx);
    assert!(
        cx.debug_bounds("lfs-preview-download-explanation")
            .is_some(),
        "at 13 pt in a 1480 × 973 window the sentence leads the line"
    );
    let wide_rest = downloads(cx);
    narrow(cx);
    shown(cx);
    let narrow_rest = downloads(cx);
    assert!(
        (narrow_rest[0].center().y - narrow_rest[1].center().y).abs()
            <= cx.update(|window, _| px(1. / window.scale_factor())),
        "at 18 pt the two share a line: {narrow_rest:?}"
    );
    assert_eq!(
        [0, 1].map(|side| crate::lfs_download::drawn_glyph(true, side)),
        glyphs_with(None)
    );
    let words = "Git LFS could not reach origin: the remote refused the batch request \
                 for this object because its credentials have expired";
    for side in [1, 0] {
        start_download(&app, &browser, cx, narrow_rest[side].center());
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        shown(cx);
        end_download(&app, &browser, cx, Some(words));
        shown(cx);
        let device = cx.update(|window, _| px(1. / window.scale_factor()));
        let failed = downloads(cx);
        assert!(
            kept(&narrow_rest, &failed, device)
                && cx.debug_bounds("lfs-download-failure").is_none(),
            "side {side} at 18 pt: both Downloads, {failed:?}, keep their places at rest, \
             {narrow_rest:?}, and the words have no room"
        );
        assert_eq!(
            [0, 1].map(|side| crate::lfs_download::drawn_glyph(true, side)),
            glyphs_with(Some(side)),
            "side {side} at 18 pt: the failed Download draws the warning glyph"
        );
        wide(cx);
        shown(cx);
        let device = cx.update(|window, _| px(1. / window.scale_factor()));
        let failure = cx
            .debug_bounds("lfs-download-failure")
            .expect("the words are drawn");
        let failed = downloads(cx);
        assert!(
            kept(&wide_rest, &failed, device) && failure.right() <= failed[0].left() + device / 2.,
            "side {side} at 13 pt: the words, {failure:?}, take the sentence's place, and \
             both Downloads, {failed:?}, keep their places at rest, {wide_rest:?}"
        );
        assert_eq!(
            [0, 1].map(|side| crate::lfs_download::drawn_glyph(true, side)),
            glyphs_with(Some(side)),
            "side {side} at 13 pt: the failed Download draws the warning glyph"
        );
        narrow(cx);
        shown(cx);
    }
    // Pressed again, Download Before… opens its form: the failure clears
    // and the row rests.
    cx.simulate_click(narrow_rest[0].center(), Modifiers::none());
    draw(cx);
    shown(cx);
    assert!(
        browser.read_with(cx, |browser, _| {
            browser.lfs_download.is_none() && browser.asking.is_some()
        }),
        "the press asks for a download again"
    );
    assert_eq!(
        [0, 1].map(|side| crate::lfs_download::drawn_glyph(true, side)),
        glyphs_with(None),
        "once the failure clears, Download Before… draws its pull glyph again"
    );
    cx.update(|window, cx| window.close_dialog(cx));
    wide(cx);
}

/// A lone missing side's failure takes the sentence's place too
/// (decision, 2026-11-10): at 13 pt in a 1480 × 973 window, where the
/// sentence leads Download Before…'s line at rest, a failure's words
/// stand in its place at its width, and the Download keeps the place
/// where it was pressed. At their own width before it, the words moved
/// it 137 px left as the download failed.
#[gpui::test]
async fn a_lone_side_s_failure_keeps_its_download_in_place(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    let sentence = cx
        .debug_bounds("lfs-preview-download-explanation")
        .expect("the sentence leads the line");
    let pressed = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered");
    start_download(&app, &browser, cx, pressed.center());
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    draw(cx);
    end_download(&app, &browser, cx, Some("Git LFS could not reach origin"));
    draw(cx);
    let device = cx.update(|window, _| px(1. / window.scale_factor()));
    let failure = cx
        .debug_bounds("lfs-download-failure")
        .expect("the words are drawn");
    let button = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is back");
    assert!(
        (button.left() - pressed.left()).abs() <= device / 2.
            && (failure.left() - sentence.left()).abs() <= device / 2.
            && (failure.size.width - sentence.size.width).abs() <= device / 2.,
        "the words, {failure:?}, take the sentence's place, {sentence:?}, and Download \
         Before…, {button:?}, keeps where it was pressed, {pressed:?}"
    );
}

/// A failure's mark is the failed Download's own glyph, and every
/// tooltip says what is drawn (decisions, 2026-11-10): with both sides'
/// objects missing and After…'s download failed, at 18 pt in QA's
/// 461 × 490 window at scale 2, where the two share a line without their
/// word and the words have no room, Download After… draws
/// `icons/warning.svg` in place of its pull glyph and its tooltip names it
/// with the failure's whole words; at 13 pt in a 1480 × 973 window the
/// words drawn on that shared line say themselves with the side's name in
/// theirs; at 18 pt in a 461 × 680 window, where the two stack, the words
/// beside Download After… say themselves without the name, as drawn, and
/// Download After…'s tooltip still names it with them. A warning icon of
/// its own once stood after the Downloads, read as the last one's, and
/// the failed Download's tooltip still gave the sentence.
#[gpui::test]
async fn a_failure_s_glyph_and_tooltips_say_what_is_drawn(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_both();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let initial = cx.update(|window, _| window.scale_factor());
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.update(|window, _| window.set_scale_factor(2.));
    let browser = inspecting(&app, cx, &files);
    choose(&browser, cx, file_index(&files, "both.mmd"));
    let shown = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let body = &browser.body;
                body.reveal_next_focus();
                body.focus("series-file-preview", cx).focus(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
    };
    shown(cx);
    let after = cx
        .debug_bounds("download-lfs-side-1")
        .expect("Download After… is offered");
    start_download(&app, &browser, cx, after.center());
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    shown(cx);
    let words = "Git LFS could not reach origin: the remote refused the batch request \
                 for this object because its credentials have expired";
    end_download(&app, &browser, cx, Some(words));
    shown(cx);
    let failed = format!("Download After… · Failed: {words}");
    let named = format!("After: {words}");
    assert!(
        cx.debug_bounds("lfs-download-failure").is_none(),
        "the words have no room on the shared line"
    );
    assert_eq!(
        [0, 1].map(|side| crate::lfs_download::drawn_glyph(true, side)),
        glyphs_with(Some(1)),
        "Download After… draws the warning glyph"
    );
    assert!(
        hover_shows(cx, "download-lfs-side-1", &failed),
        "on the shared line Download After…'s tooltip names it with the failure"
    );
    cx.update(|window, _| window.set_scale_factor(initial));
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    cx.simulate_resize(size(px(1480.), px(973.)));
    shown(cx);
    assert!(
        hover_shows(cx, "lfs-download-failure", &named)
            && hover_shows(cx, "download-lfs-side-1", &failed),
        "at 13 pt the drawn words and Download After… say the same"
    );
    cx.simulate_resize(size(px(461.), px(680.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.update(|window, _| window.set_scale_factor(2.));
    shown(cx);
    let [before, after] = ["download-lfs-side-0", "download-lfs-side-1"]
        .map(|id| cx.debug_bounds(id).unwrap_or_else(|| panic!("{id}")));
    assert!(
        after.top() >= before.bottom() - px(0.25),
        "the two stack: {before:?}, {after:?}"
    );
    assert_eq!(
        [0, 1].map(|side| crate::lfs_download::drawn_glyph(true, side)),
        glyphs_with(Some(1)),
        "stacked, Download After… draws the warning glyph"
    );
    assert!(
        hover_shows(cx, "lfs-download-failure", words)
            && hover_shows(cx, "download-lfs-side-1", &failed),
        "stacked, the words say themselves as drawn, without the side, and Download \
         After… names itself with them"
    );
    cx.update(|window, _| window.set_scale_factor(initial));
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// Neither a press nor a progress update moves Cancel download or the
/// other side's Download (decision, 2026-11-10): with both sides'
/// objects missing, at 18 pt in a 600 × 490 window at scale 2 and at
/// 13 pt in a 1480 × 973 window, Before…'s running form keeps its
/// progress the room the row reserved for it, so Cancel keeps its bounds
/// and After… its place before the press, after it and after a long
/// progress update, the progress ending in an ellipsis with its whole
/// text in its tooltip and accessible name; and on the dialog's footer,
/// while the review shows another file, Cancel keeps its bounds through
/// the same steps. The progress grew into the line's spare room as the
/// press turned it to "Cancelling… Inspect the result before
/// retrying.", pushing Cancel, and After… with it, right.
#[gpui::test]
async fn cancel_and_the_other_download_stand_still_as_the_progress_grows(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_both_with(&["notes.txt"]);
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let initial = cx.update(|window, _| window.scale_factor());
    let browser = inspecting(&app, cx, &files);
    let shown = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let body = &browser.body;
                body.reveal_next_focus();
                body.focus("series-file-preview", cx).focus(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
    };
    let long = "Receiving objects: 50% (1/2), 1.20 MiB | 3.40 MiB/s, and a long report \
                from Git LFS that runs well past the room the line keeps for it";
    // Cancel's bounds and, in the row, After…'s left edge.
    let placed = |cx: &mut gpui::VisualTestContext| {
        (
            cx.debug_bounds("cancel-lfs-download")
                .expect("Cancel download is drawn"),
            cx.debug_bounds("download-lfs-side-1")
                .map(|after| after.left()),
        )
    };
    for (width, height, text, scale) in [(600., 490., 18, 2.), (1480., 973., 13, initial)] {
        let at = format!("{width} × {height} at {text} pt");
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        cx.update(|window, _| window.set_scale_factor(scale));
        for footer in [false, true] {
            let at = format!("{at}, {}", if footer { "footer" } else { "row" });
            choose(&browser, cx, file_index(&files, "both.mmd"));
            shown(cx);
            let before = cx
                .debug_bounds("download-lfs-side-0")
                .expect("Download Before… is offered");
            start_download(&app, &browser, cx, before.center());
            if footer {
                choose(&browser, cx, file_index(&files, "notes.txt"));
            }
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            shown(cx);
            let resting = placed(cx);
            assert_eq!(
                resting.1.is_some(),
                !footer,
                "{at}: After… is drawn beside Before…'s running form in the row only"
            );
            cx.simulate_click(resting.0.center(), Modifiers::none());
            shown(cx);
            assert!(
                app.read_with(cx, |app, _| app.operation_cancelling()),
                "{at}: the press cancels the write"
            );
            assert_eq!(placed(cx), resting, "{at}: the press moves nothing");
            crate::authentication::CANCELLING_WORDS.set(Some(long));
            shown(cx);
            let updated = placed(cx);
            let said = written::get("lfs-download-progress")
                .and_then(|node| node.node.label().map(str::to_owned));
            let tooltip = !footer && hover_shows(cx, "lfs-download-progress", long);
            crate::authentication::CANCELLING_WORDS.set(None);
            assert_eq!(updated, resting, "{at}: a long progress moves nothing");
            if !footer {
                assert_eq!(
                    (said.as_deref(), tooltip),
                    (Some(long), true),
                    "{at}: the progress says its whole text in its accessible name \
                     and its tooltip"
                );
            }
            end_download(&app, &browser, cx, Some("Cancelled"));
        }
    }
    cx.update(|window, _| window.set_scale_factor(initial));
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The footer's progress keeps a room of its own as the row's does
/// (decisions, 2026-11-10 and 2026-11-11): the narrower of what its line
/// leaves beside Cancel download and its name with the first words its
/// progress says once pressed, "Downloading both.mmd's Before object ·
/// Cancelling…", measured once for the download's object. At 13 pt and
/// scale 1 in a 1480 × 973 window the room is that wide, so before any
/// press the blank between the name, all the line says, and Cancel is
/// at most "· Cancelling…" and the line's gap; Cancel stands a gap after
/// the room and keeps its bounds through a long Git LFS progress before
/// any press, the press and "Cancelling…", nothing measured again; once
/// pressed the text draws those words whole, as GPUI cuts the sentence
/// at the text's drawn width, the rest of it cut, whole in its tooltip
/// and accessible name; the text's tooltip shows over its words, never
/// over the blank room after shorter words or the footer past Cancel; a
/// text size changed while it runs measures them again, once. At 18 pt
/// in 600 × 490 and 461 × 490 windows, as before, the room takes what
/// the line leaves and Cancel stands at its end. Taking all the line
/// left, the text drew Cancel at the line's end, far from its words in
/// the wide window, so it read as a pair with Back to repository, and
/// the text's tooltip showed over the blank footer between them; kept
/// for the whole cancelling sentence, the room still stood Cancel far
/// from the name before any press.
#[gpui::test]
async fn the_footer_s_progress_keeps_its_room_with_cancel_beside_it(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::lfs_download::FOOTER_MEASURES;
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_both_with(&["notes.txt"]);
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let initial = cx.update(|window, _| window.scale_factor());
    let browser = inspecting(&app, cx, &files);
    let shown = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let body = &browser.body;
                body.reveal_next_focus();
                body.focus("series-file-preview", cx).focus(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
    };
    let name = "Downloading both.mmd's Before object";
    let kept = format!("{name} · Cancelling…");
    let cancelling = format!("{name} · {}", crate::authentication::CANCELLING_PROGRESS);
    // The kept words and what they keep past the name as layout places
    // them, the line's gap and a device pixel.
    let measure = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, _| {
            let placed = |text: &str| {
                crate::views::laid_out_text_width(
                    crate::views::line_widths(&[(text, appearance::ui_text(12.))], window),
                    window,
                )
            };
            (
                placed(&kept),
                placed(" · Cancelling…"),
                window.pixel_snap(window.rem_size() * 0.5),
                px(1. / window.scale_factor()),
            )
        })
    };
    // What GPUI's cut of `text` draws in a text `width` wide, as the
    // progress's text element cuts it.
    let cut = |cx: &mut gpui::VisualTestContext, text: &str, width: Pixels| {
        cx.update(|window, _| {
            let style = window.text_style();
            window
                .text_system()
                .line_wrapper(style.font(), appearance::ui_text(12.))
                .truncate_line(
                    SharedString::from(text.to_owned()),
                    width,
                    "…",
                    &[style.to_run(text.len())],
                    gpui::TruncateFrom::End,
                )
                .0
                .to_string()
        })
    };
    let long = "Receiving objects: 50% (1/2), 1.20 MiB | 3.40 MiB/s, and a long report \
                from Git LFS that runs well past the room the line keeps for it";
    let said = format!("{name} · {long}");
    for (width, height, text, scale) in [
        (1480., 973., 13, 1.),
        (600., 490., 18, 2.),
        (461., 490., 18, 2.),
    ] {
        let at = format!("{width} × {height} at {text} pt");
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        // After the resize, which sets the test window's scale back.
        cx.update(|window, _| window.set_scale_factor(scale));
        assert_eq!(
            cx.update(|window, _| window.scale_factor()),
            scale,
            "{at}: the window draws at {scale}"
        );
        choose(&browser, cx, file_index(&files, "both.mmd"));
        shown(cx);
        let before = cx
            .debug_bounds("download-lfs-side-0")
            .expect("Download Before… is offered");
        start_download(&app, &browser, cx, before.center());
        choose(&browser, cx, file_index(&files, "notes.txt"));
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        shown(cx);
        let (reserved, beyond, gap, device) = measure(cx);
        let [line, room, progress, rest] = [
            "series-download-status",
            "lfs-download-progress-room",
            "lfs-download-progress",
            "cancel-lfs-download",
        ]
        .map(|id| {
            cx.debug_bounds(id)
                .unwrap_or_else(|| panic!("{at}: the footer draws {id}"))
        });
        // The room keeps the words, past them by a device pixel at most,
        // where GPUI's cut keeps them whole.
        let keeps = |room: Bounds<Pixels>, reserved: Pixels, device: Pixels| {
            room.size.width >= reserved - device / 2.
                && room.size.width <= reserved + device + device / 2.
        };
        let wide = width > 1000.;
        if wide {
            assert!(
                keeps(room, reserved, device)
                    && (rest.left() - (room.right() + gap)).abs() <= device / 2.
                    && rest.right() < line.right() - device,
                "{at}: the progress's room, {room:?}, keeps {reserved:?}, and Cancel, \
                 {rest:?}, stands a gap after it, short of the line's end, {line:?}"
            );
            // Before any press the line says the name alone: Cancel
            // stands at most "· Cancelling…" and the gap from it, and the
            // device pixel the room may lie past those words.
            assert!(
                rest.left() - progress.right() <= beyond + gap + device,
                "{at}: the blank between the name, {progress:?}, and Cancel, {rest:?}, \
                 is at most {beyond:?} and the gap, {gap:?}"
            );
            // The words before any press, the name alone, are shorter
            // than the room: the text, and its tooltip, take them only,
            // and neither the blank room after them nor the footer past
            // Cancel shows it.
            let between = point((progress.right() + rest.left()) / 2., line.center().y);
            let past = point((rest.right() + line.right()) / 2., line.center().y);
            assert!(
                progress.right() < between.x - px(4.) && line.right() - rest.right() >= px(4.),
                "{at}: the text, {progress:?}, leaves the room, {room:?}, blank before \
                 Cancel, {rest:?}, and the line, {line:?}, has room past Cancel"
            );
            let shows = [between, past].map(|at| tooltip_at(cx, at, name));
            assert_eq!(
                (hover_shows(cx, "lfs-download-progress", name), shows),
                (true, [false, false]),
                "{at}: the text's tooltip says its words over them alone, none over the \
                 blank room after them, {between:?}, or past Cancel, {past:?}"
            );
        } else {
            assert!(
                (rest.right() - line.right()).abs() <= device / 2.
                    && (room.left() - line.left()).abs() <= device / 2.
                    && (rest.left() - (room.right() + gap)).abs() <= device / 2.
                    && room.size.width < reserved,
                "{at}: the progress's room, {room:?}, takes what the line, {line:?}, \
                 leaves beside Cancel, {rest:?}, at its end"
            );
        }
        FOOTER_MEASURES.set(0);
        crate::authentication::PROGRESS_WORDS.set(Some(long));
        shown(cx);
        let updated = cx.debug_bounds("cancel-lfs-download");
        let named = written::get("lfs-download-progress")
            .and_then(|node| node.node.label().map(str::to_owned));
        // A long progress fills the room, ending in an ellipsis inside
        // it, its tooltip and accessible name saying it whole.
        let filled = cx.debug_bounds("lfs-download-progress");
        let tooltip = hover_shows(cx, "lfs-download-progress", &said);
        crate::authentication::PROGRESS_WORDS.set(None);
        assert_eq!(
            updated,
            Some(rest),
            "{at}: a long progress before any press moves nothing"
        );
        assert!(
            filled.is_some_and(|filled| {
                filled.left() >= room.left() - device / 2.
                    && filled.right() <= room.right() + device / 2.
            }),
            "{at}: a long progress, {filled:?}, stays inside its room, {room:?}"
        );
        assert_eq!(
            (tooltip, named.as_deref()),
            (true, Some(said.as_str())),
            "{at}: the text's tooltip and accessible name say its words whole"
        );
        cx.simulate_click(rest.center(), Modifiers::none());
        shown(cx);
        if wide {
            // Once pressed, the name with "· Cancelling…" is drawn whole
            // inside the room, as GPUI cuts the sentence at the text's
            // drawn width, the rest of it cut, and the tooltip and the
            // accessible name say it whole.
            let drawn = cx
                .debug_bounds("lfs-download-progress")
                .expect("the progress is drawn");
            let shows = cut(cx, &cancelling, drawn.size.width);
            let named = written::get("lfs-download-progress")
                .and_then(|node| node.node.label().map(str::to_owned));
            assert!(
                drawn.size.width >= reserved - device / 2.
                    && drawn.left() >= room.left() - device / 2.
                    && drawn.right() <= room.right() + device / 2.,
                "{at}: the cancelling text, {drawn:?}, holds {reserved:?} inside its room, \
                 {room:?}"
            );
            assert_eq!(
                (
                    shows.as_str(),
                    hover_shows(cx, "lfs-download-progress", &cancelling),
                    named.as_deref(),
                ),
                (kept.as_str(), true, Some(cancelling.as_str())),
                "{at}: the text draws its name and \"Cancelling…\" whole and cuts the \
                 rest, saying it whole in its tooltip and accessible name"
            );
        }
        // Its tooltip is its name in every form once pressed.
        let pressed = crate::views::CONTROL_TOOLTIPS
            .with(|tooltips| tooltips.borrow().get("cancel-lfs-download").cloned());
        assert!(
            app.read_with(cx, |app, _| app.operation_cancelling())
                && pressed.as_deref() == Some("Cancelling download of both.mmd's Before object"),
            "{at}: the press cancels the write: {pressed:?}"
        );
        assert_eq!(
            cx.debug_bounds("cancel-lfs-download"),
            Some(rest),
            "{at}: the press and \"Cancelling…\" move nothing"
        );
        assert_eq!(
            FOOTER_MEASURES.get(),
            0,
            "{at}: the line's kept words are measured once for its object"
        );
        if wide {
            // A text size changed while it runs: the words are measured
            // again, once, and the room follows them; a smaller size, so
            // they stay whole.
            cx.update(|window, cx| appearance::apply_text_sizes(12, 12, window, cx));
            shown(cx);
            shown(cx);
            let (reserved, _, _, device) = measure(cx);
            let room = cx
                .debug_bounds("lfs-download-progress-room")
                .expect("the progress's room");
            assert!(
                FOOTER_MEASURES.get() == 1 && keeps(room, reserved, device),
                "{at}: at 12 pt the words are measured again, once ({}), and the \
                 progress's room, {room:?}, keeps {reserved:?}",
                FOOTER_MEASURES.get()
            );
        }
        end_download(&app, &browser, cx, Some("Cancelled"));
    }
    cx.update(|window, _| window.set_scale_factor(initial));
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// Whether Cancel download was pressed is read from the window's write,
/// never from the words its progress then reads: with those words
/// changed, a press still turns the review's Cancel to "Cancelling…",
/// no longer pressable, and the window says it was pressed. It was read
/// from the progress starting "Cancelling", so other words there left it
/// reading "Cancel download" and pressable.
#[gpui::test]
async fn a_pressed_cancel_is_read_from_the_write_not_its_words(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    start_download(&app, &browser, cx, at);
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    crate::tags::tests::draw(cx);
    let cancel = cx
        .debug_bounds("cancel-lfs-download")
        .expect("Cancel download stands in the row");
    let name = || {
        written::get("cancel-lfs-download").and_then(|node| node.node.label().map(str::to_owned))
    };
    assert_eq!(
        name().as_deref(),
        Some("Cancel download of flow.mmd's Before object"),
        "before the press it is named for what it cancels"
    );
    crate::authentication::CANCELLING_WORDS.set(Some("Stopping the transfer…"));
    cx.simulate_click(cancel.center(), Modifiers::none());
    crate::tags::tests::draw(cx);
    let pressed = (
        crate::views::COMPACT_LABELS
            .with(|labels| labels.borrow().get("cancel-lfs-download").cloned())
            .map(|label| label.to_string()),
        written::get("cancel-lfs-download").map(|node| node.node.is_disabled()),
        name(),
        app.read_with(cx, |app, _| {
            (app.operation_cancelling(), app.operation_progress())
        }),
    );
    crate::authentication::CANCELLING_WORDS.set(None);
    assert_eq!(
        pressed,
        (
            Some("Cancelling…".to_owned()),
            Some(true),
            Some("Cancelling download of flow.mmd's Before object".to_owned()),
            (true, Some("Stopping the transfer…".to_owned())),
        ),
        "pressed, whatever the progress says, Cancel reads Cancelling…, unpressable, and \
         its name says it is cancelling"
    );
    end_download(&app, &browser, cx, Some("Cancelled"));
}

/// Cancel download cancels the window's write only while it is still
/// the download the review asked for, of the object its Cancel names: a
/// click on the review's Cancel delivered before the frame that takes it
/// away, once its download has ended and Compare's or another review's
/// runs, cancels nothing. It cancelled whatever write ran.
#[gpui::test]
async fn a_cancel_left_from_an_ended_download_cancels_no_other(cx: &mut gpui_kit::TestAppContext) {
    use crate::lfs_download::Asker;
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    for other in [Asker::Compare, Asker::Review(app.entity_id())] {
        let at = cx
            .debug_bounds("download-lfs-side-0")
            .expect("Download Before… is offered")
            .center();
        start_download(&app, &browser, cx, at);
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        crate::tags::tests::draw(cx);
        let cancel = cx
            .debug_bounds("cancel-lfs-download")
            .expect("Cancel download stands in the row");
        // The review's download ends and another starts, and a click
        // lands on its Cancel before a frame takes it away: the update
        // that ends a write draws the next frame as it returns.
        let [down, up] = click_at(cancel.center(), 1);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.simulate_lfs_download_end(Some("Git LFS could not reach origin"), window, cx);
                app.simulate_lfs_download(other, window, cx);
            });
            window.dispatch_event(down, cx);
            window.dispatch_event(up, cx);
        });
        crate::tags::tests::draw(cx);
        assert!(
            !app.read_with(cx, |app, _| app.operation_cancelling()),
            "a click on the review's Cancel cancels none of {other:?}'s download"
        );
        // The other download ends, landing the read its end asks for:
        // read before it did, with the reader slow, the row had not
        // come back to offer Download Before….
        end_download(&app, &browser, cx, None);
    }
}

/// On the dialog's footer, where at 18 pt in QA's 461 × 490 window
/// Cancel download gives way to its icon, its tooltip leads with the
/// label the icon stands for, as the header's compact Cancel's does:
/// "Cancel · Cancel download of flow.mmd's Before object", and once
/// pressed its accessible name alone, "Cancelling download of flow.mmd's
/// Before object", which says it is cancelling, once (decision,
/// 2026-11-10). It said only what it cancels, pressed or not, and then
/// "Cancelling… · Cancelling download of …".
#[gpui::test]
async fn the_footer_s_cancel_icon_says_its_label_in_its_tooltip(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_before_with(&["notes.txt"]);
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    start_download(&app, &browser, cx, at);
    choose(&browser, cx, file_index(&files, "notes.txt"));
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.update(|window, _| window.set_scale_factor(2.));
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    draw(cx);
    draw(cx);
    let said = || {
        (
            crate::native_accessibility::written::get("cancel-lfs-download")
                .and_then(|node| node.node.label().map(str::to_owned)),
            crate::views::COMPACT_LABELS
                .with(|labels| labels.borrow().get("cancel-lfs-download").cloned())
                .map(|label| label.to_string()),
            crate::views::CONTROL_TOOLTIPS.with(|tooltips| {
                tooltips
                    .borrow()
                    .get(&SharedString::from("cancel-lfs-download"))
                    .cloned()
            }),
        )
    };
    let name = "Cancel download of flow.mmd's Before object";
    assert_eq!(
        said(),
        (
            Some(name.to_owned()),
            Some(String::new()),
            Some(format!("Cancel · {name}"))
        ),
        "the footer's Cancel is its icon, its tooltip leading with its label"
    );
    let cancel = cx
        .debug_bounds("cancel-lfs-download")
        .expect("Cancel download stands on the footer");
    cx.simulate_click(cancel.center(), Modifiers::none());
    draw(cx);
    let name = "Cancelling download of flow.mmd's Before object";
    assert_eq!(
        said(),
        (
            Some(name.to_owned()),
            Some(String::new()),
            Some(name.to_owned())
        ),
        "pressed, its name says it is cancelling, and its tooltip is that name alone"
    );
    end_download(&app, &browser, cx, None);
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The comparison the review shows, by its identity.
/// By its showing, which each read starts: a comparison read again may
/// land at the address the one it replaces was freed from.
fn shown_key(
    browser: &Entity<SeriesBrowser>,
    cx: &mut gpui::VisualTestContext,
) -> Option<crate::system_preview::Showing> {
    browser.read_with(cx, |browser, _| {
        browser.content.is_some().then_some(browser.showing)
    })
}

/// Presses the review's Download Before… at `at`, its form giving way
/// to its confirmation, and starts the write that confirmation asks
/// for, for the review.
fn start_download(
    app: &Entity<GitTurtle>,
    browser: &Entity<SeriesBrowser>,
    cx: &mut gpui::VisualTestContext,
    at: Point<Pixels>,
) {
    cx.simulate_click(at, Modifiers::none());
    crate::tags::tests::draw(cx);
    cx.update(|window, cx| window.close_dialog(cx));
    let me = crate::lfs_download::Asker::Review(browser.entity_id());
    cx.update(|window, cx| app.update(cx, |app, cx| app.simulate_lfs_download(me, window, cx)));
    crate::tags::tests::draw(cx);
}

/// Ends the review's download, failing with `failure` or succeeding,
/// and lands any read its end asks for.
fn end_download(
    app: &Entity<GitTurtle>,
    browser: &Entity<SeriesBrowser>,
    cx: &mut gpui::VisualTestContext,
    failure: Option<&str>,
) {
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.simulate_lfs_download_end(failure, window, cx)
        })
    });
    browser.read_with(cx, |browser, _| browser.worker.wait_idle());
    crate::tags::tests::draw(cx);
}

/// One caller's start never erases another's outcome (decision,
/// 2026-11-10): where the review's download fails in the same update in
/// which Compare's download, or another review's, starts, the review
/// keeps the failure, its row saying the words beside Download Before…,
/// and reads nothing again, through the other download's run and end.
/// The start cleared the window's record of how the review's download
/// ended before the review read it, so the review took the failure for a
/// success, read its file again, its row giving way to the read, and the
/// failure was lost. Meanwhile the failed Download's warning glyph
/// dims with its button while the other starts and runs, the window's
/// busy look carried over from the review's own write, in the kit's
/// disabled colour as its label is, and at rest once the other ended is
/// the warning colour again (decision, 2026-11-10 night: set on the
/// glyph, the warning colour outlasted the button's disabled look, so
/// the dimmed Download read half enabled); and the failed Download keeps
/// its width at rest, within half a device pixel. The glyph's colour is
/// read from the icon the Download is handed (`lfs_download::noted_glyph`):
/// read from the colour meant for it, a glyph drawn in another passed.
#[gpui::test]
async fn another_caller_s_start_keeps_the_review_s_failure(cx: &mut gpui_kit::TestAppContext) {
    use crate::lfs_download::Asker;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    let browser = reviewing_download(&app, cx, &files);
    let words = "Git LFS could not reach origin";
    let (warning, dimmed, device) = cx.update(|window, cx| {
        (
            gpui::Hsla::from(rgb(appearance::labels(cx).warning)),
            gpui_kit::component::Theme::global(cx).disabled_foreground(),
            px(1. / window.scale_factor()),
        )
    });
    assert_ne!(warning, dimmed, "the two colours differ");
    let rest = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered");
    for other in [Asker::Compare, Asker::Review(app.entity_id())] {
        let at = cx
            .debug_bounds("download-lfs-side-0")
            .expect("Download Before… is offered")
            .center();
        start_download(&app, &browser, cx, at);
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        draw(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.simulate_lfs_download_end(Some(words), window, cx);
                app.simulate_lfs_download(other, window, cx);
            })
        });
        let kept = |at: &str, glyph: gpui::Hsla, cx: &mut gpui::VisualTestContext| {
            browser.read_with(cx, |browser, _| browser.worker.wait_idle());
            draw(cx);
            assert_eq!(
                (
                    crate::lfs_download::drawn_glyph(true, 0).as_deref(),
                    crate::lfs_download::drawn_glyph_color(true, 0),
                ),
                (Some("icons/warning.svg"), Some(glyph)),
                "{other:?}, {at}: Download Before… draws the warning glyph, dimmed with \
                 it only while it is"
            );
            let failed = cx
                .debug_bounds("download-lfs-side-0")
                .expect("Download Before… is back");
            assert!(
                (failed.size.width - rest.size.width).abs() <= device / 2.,
                "{other:?}, {at}: the failed Download, {failed:?}, keeps its width, {rest:?}"
            );
            let failure = browser.read_with(cx, |browser, _| {
                browser
                    .lfs_download
                    .as_ref()
                    .and_then(|asked| asked.failure.clone())
            });
            assert_eq!(
                failure.as_deref(),
                Some(words),
                "{other:?}, {at}: the review keeps its failure"
            );
            assert!(
                cx.debug_bounds("download-lfs-side-0").is_some()
                    && cx.debug_bounds("lfs-download-failure").is_some()
                    && cx.debug_bounds("series-preview-reading").is_none(),
                "{other:?}, {at}: the row says it beside Download Before…, and nothing is \
                 read again"
            );
        };
        kept("as the other starts", dimmed, cx);
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        kept("as the other runs", dimmed, cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.simulate_lfs_download_end(None, window, cx)
            })
        });
        cx.run_until_parked();
        kept("once the other ended", warning, cx);
    }
}

/// A press the window refuses while the review's download runs, as in
/// its busy delay, where the Download still stands, records nothing, so
/// the download's end still reads the file again: the press once
/// recorded a fresh request over the one in flight, whose end then read
/// nothing.
#[gpui::test]
async fn a_refused_press_keeps_the_download_in_flight(cx: &mut gpui_kit::TestAppContext) {
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    start_download(&app, &browser, cx, at);
    let forms = crate::lfs_download::FORMS_OPENED.get();
    cx.simulate_click(at, Modifiers::none());
    crate::tags::tests::draw(cx);
    assert_eq!(
        crate::lfs_download::FORMS_OPENED.get(),
        forms,
        "the window refuses a press while the download runs"
    );
    let before = shown_key(&browser, cx);
    end_download(&app, &browser, cx, None);
    assert!(
        shown_key(&browser, cx).is_some() && shown_key(&browser, cx) != before,
        "the download's end reads the file again"
    );
}

/// The download's end reads its file again only while the review shows
/// that file of the commit it was asked from: after Back and another
/// commit, whose list has a file at the same place, the end reads
/// nothing and moves no focus. Matched by its place in the list alone,
/// it read the other commit's file.
#[gpui::test]
async fn a_late_download_end_reads_only_the_file_it_was_asked_from(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    browser.update(cx, |browser, _| browser.inspected = Some("asked".into()));
    crate::tags::tests::draw(cx);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    start_download(&app, &browser, cx, at);
    cx.update(|window, cx| browser.update(cx, |browser, cx| browser.back(window, cx)));
    crate::tags::tests::draw(cx);
    browser.update(cx, |browser, cx| {
        browser.inspecting = true;
        browser.inspected = Some("another".into());
        browser.files = files.clone();
        cx.notify();
    });
    choose(&browser, cx, file_index(&files, "flow.mmd"));
    let focused = cx.update(|window, cx| window.focused(cx));
    let before = shown_key(&browser, cx);
    end_download(&app, &browser, cx, None);
    assert_eq!(
        shown_key(&browser, cx),
        before,
        "another commit's file at the same place is not read again"
    );
    assert_eq!(
        cx.update(|window, cx| window.focused(cx)),
        focused,
        "and focus stays"
    );
}

/// Whether the review's preview holds focus.
fn preview_focused(browser: &Entity<SeriesBrowser>, cx: &mut gpui::VisualTestContext) -> bool {
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser
                .body
                .focus("series-file-preview", cx)
                .is_focused(window)
        })
    })
}

/// Focus moves onto Cancel download only where the review draws it:
/// every file's row tracks the same handles, so after Back and another
/// commit, whose file's Download held focus as the download asked from
/// the first looked busy, focus moved onto a Cancel no row drew. Its
/// success there reads nothing and moves no focus, said only where its
/// line stood, on the dialog's footer.
#[gpui::test]
async fn focus_moves_only_onto_a_cancel_the_review_draws(cx: &mut gpui_kit::TestAppContext) {
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    browser.update(cx, |browser, _| browser.inspected = Some("asked".into()));
    crate::tags::tests::draw(cx);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    start_download(&app, &browser, cx, at);
    cx.update(|window, cx| browser.update(cx, |browser, cx| browser.back(window, cx)));
    crate::tags::tests::draw(cx);
    browser.update(cx, |browser, cx| {
        browser.inspecting = true;
        browser.inspected = Some("another".into());
        browser.files = files.clone();
        cx.notify();
    });
    choose(&browser, cx, file_index(&files, "flow.mmd"));
    cx.update(|window, cx| browser.read(cx).download_focus[0].clone().focus(window, cx));
    crate::tags::tests::draw(cx);
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    crate::tags::tests::draw(cx);
    app.update(cx, |_, cx| cx.notify());
    crate::tags::tests::draw(cx);
    assert!(
        cx.debug_bounds("download-lfs-side-0").is_some()
            && holds_focus(&browser, cx, |browser| &browser.download_focus[0])
            && !holds_focus(&browser, cx, |browser| &browser.download_cancel),
        "focus stays on the other commit's Download, which is drawn"
    );
    end_download(&app, &browser, cx, None);
    assert!(
        holds_focus(&browser, cx, |browser| &browser.download_focus[0]),
        "the success elsewhere moves no focus"
    );
    assert_eq!(
        crate::copy_feedback::tests::announced(cx).map(|(_, said)| said),
        Some("Downloaded flow.mmd's Before object".to_owned()),
        "and is said where its line stood, naming the file it read nothing of"
    );
}

/// A success moves focus from the side's Download or Cancel, which the
/// read again takes away, to the preview, also where the write ends
/// inside the busy delay with the Download still drawn and focused, and
/// says once, politely, what it downloaded: the window's notice says
/// it under the review, hidden from assistive technology while the
/// review is open. A quick success left focus on the vanished Download.
#[gpui::test]
async fn a_quick_success_moves_focus_to_the_preview_and_says_so(cx: &mut gpui_kit::TestAppContext) {
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    start_download(&app, &browser, cx, at);
    cx.update(|window, cx| browser.read(cx).download_focus[0].clone().focus(window, cx));
    crate::tags::tests::draw(cx);
    assert!(
        cx.debug_bounds("lfs-download-progress").is_none(),
        "inside the busy delay the Download still stands"
    );
    end_download(&app, &browser, cx, None);
    assert!(
        preview_focused(&browser, cx),
        "focus moves from the Download the read again takes away to the preview"
    );
    let (_, announced) =
        crate::copy_feedback::tests::announced(cx).expect("the success is announced");
    assert_eq!(
        announced, "Downloaded flow.mmd's Before object",
        "once, politely"
    );
}

/// Starts the review's download of `flow.mmd`'s Before object from its
/// Download, focused, and lets its write look busy, Cancel download
/// taking focus in the row.
fn running_with_cancel_focused(
    app: &Entity<GitTurtle>,
    browser: &Entity<SeriesBrowser>,
    cx: &mut gpui::VisualTestContext,
) {
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    start_download(app, browser, cx, at);
    cx.update(|window, cx| browser.read(cx).download_focus[0].clone().focus(window, cx));
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    crate::tags::tests::draw(cx);
    app.update(cx, |_, cx| cx.notify());
    crate::tags::tests::draw(cx);
    assert!(
        cx.debug_bounds("lfs-download-progress").is_some()
            && holds_focus(browser, cx, |browser| &browser.download_cancel),
        "the progress stands in the row, Cancel download focused"
    );
}

/// The download stays in view while the review shows another file: the
/// dialog's footer says the progress and offers Cancel download, focus
/// on Cancel staying on it as it moves there. A failure that lands
/// meanwhile is announced at once, naming what failed, focus going from
/// the vanished Cancel to Back to series, never the window's root, and
/// kept: the footer says it in its warning words, "Couldn't download
/// flow.mmd's Before object: …", until the person comes back to the
/// file, where the row says it, and only leaving that file again drops
/// it. The line vanished, nothing on screen said the download failed,
/// and the alert said the words alone. The review covers the operation bar, so it said
/// nothing of a download that ran for a file it no longer showed, and
/// the failure that landed meanwhile was dropped unsaid.
#[gpui::test]
async fn the_review_keeps_its_download_in_view_while_it_shows_another_file(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::native_accessibility::written;
    let (_fixture, repo, files) = missing_before_with(&["notes.txt"]);
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    running_with_cancel_focused(&app, &browser, cx);
    assert!(
        cx.debug_bounds("series-download-status").is_none(),
        "the row says it while the review shows its file"
    );
    choose(&browser, cx, file_index(&files, "notes.txt"));
    let status = cx
        .debug_bounds("series-download-status")
        .expect("the dialog's footer says the download");
    let progress = cx
        .debug_bounds("lfs-download-progress")
        .expect("its progress");
    let cancel = cx
        .debug_bounds("cancel-lfs-download")
        .expect("and its Cancel download");
    let [body, close] = ["rewrite-review-body", "close-series-review"]
        .map(|id| cx.debug_bounds(id).unwrap_or_else(|| panic!("{id}")));
    assert!(
        cx.debug_bounds("download-lfs-side-0").is_none()
            && status.contains(&progress.center())
            && status.contains(&cancel.center())
            && status.top() >= body.bottom()
            && cancel.right() <= close.left()
            && (cancel.center().y - close.center().y).abs() <= px(0.5),
        "the progress, {progress:?}, and Cancel, {cancel:?}, stand on the footer, \
         {status:?}, below the body, {body:?}, beside {close:?}"
    );
    assert!(
        holds_focus(&browser, cx, |browser| &browser.download_cancel),
        "focus stays on Cancel download where it moved"
    );
    let words = "Git LFS could not reach origin";
    end_download(&app, &browser, cx, Some(words));
    let failed = format!("Couldn't download flow.mmd's Before object: {words}");
    let footer = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("series-download-failure")?;
        written::get("series-download-failure")
            .and_then(|node| node.node.value().map(str::to_owned))
    };
    assert!(
        cx.debug_bounds("cancel-lfs-download").is_none()
            && footer(cx).as_deref() == Some(failed.as_str()),
        "the download's end takes Cancel away, the footer saying what failed"
    );
    let alert = crate::copy_feedback::tests::alerted(cx).expect("the failure is announced");
    assert_eq!(alert.1, failed, "at once, naming what failed");
    assert!(
        holds_focus(&browser, cx, |browser| &browser.back),
        "focus goes from the vanished Cancel to Back to series"
    );
    let said = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("lfs-download-failure")?;
        written::get("lfs-download-failure").and_then(|node| node.node.value().map(str::to_owned))
    };
    choose(&browser, cx, file_index(&files, "flow.mmd"));
    assert_eq!(
        (said(cx).as_deref(), footer(cx)),
        (Some(words), None),
        "back on its file, the row says the failure kept for it, the footer no more"
    );
    assert_eq!(
        crate::copy_feedback::tests::alerted(cx),
        Some(alert),
        "and it is announced once, as it landed"
    );
    choose(&browser, cx, file_index(&files, "notes.txt"));
    choose(&browser, cx, file_index(&files, "flow.mmd"));
    assert!(
        said(cx).is_none() && cx.debug_bounds("download-lfs-side-0").is_some(),
        "once said, leaving the file drops it"
    );
}

/// Back on its file while its download runs, the person's file read
/// again keeps the line on the dialog's footer away, as the row will say
/// the download once the read lands, so the line does not come and go;
/// but a read that ends without a preview, failed or interrupted,
/// leaves no row to say it, so the footer says the progress and offers
/// Cancel download again, focus on Cancel coming back with it. Counted
/// as the row's whenever the file had no preview, the download's
/// progress and Cancel were drawn nowhere after such a read.
#[gpui::test]
async fn a_read_that_fails_on_return_leaves_the_download_on_the_footer(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_before_with(&["notes.txt"]);
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    running_with_cancel_focused(&app, &browser, cx);
    choose(&browser, cx, file_index(&files, "notes.txt"));
    cx.debug_bounds("series-download-status")
        .expect("the dialog's footer says the download");
    // Back on its file, whose read waits behind a slower one.
    let held = browser.read_with(cx, |browser, _| browser.worker.hold());
    let flow = file_index(&files, "flow.mmd");
    cx.update(|window, cx| browser.update(cx, |browser, cx| browser.file(flow, window, cx)));
    draw(cx);
    assert!(
        cx.debug_bounds("series-download-status").is_none()
            && cx.debug_bounds("lfs-download-progress").is_none(),
        "while its file is read again, the line stays away for the row"
    );
    // The read is dropped unanswered, as an interrupted one is.
    browser.read_with(cx, |browser, _| browser.worker.cancel());
    drop(held);
    browser.read_with(cx, |browser, _| browser.worker.wait_idle());
    draw(cx);
    assert!(
        browser.read_with(cx, |browser, _| browser.content.is_none()
            && !browser.reading_preview()
            && browser.preview_error.is_some()),
        "the read ended without a preview"
    );
    let status = cx
        .debug_bounds("series-download-status")
        .expect("the dialog's footer says the download again");
    let [progress, cancel] = ["lfs-download-progress", "cancel-lfs-download"]
        .map(|id| cx.debug_bounds(id).unwrap_or_else(|| panic!("{id}")));
    assert!(
        status.contains(&progress.center())
            && status.contains(&cancel.center())
            && cx.debug_bounds("download-lfs-side-0").is_none(),
        "its progress, {progress:?}, and Cancel, {cancel:?}, stand on the footer, {status:?}"
    );
    assert!(
        holds_focus(&browser, cx, |browser| &browser.download_cancel),
        "focus on Cancel download comes back with it"
    );
    end_download(&app, &browser, cx, None);
}

/// A failure kept for its file while the review shows another stays
/// kept as the person opens another file's download form and cancels
/// it, the footer still saying it: only a download that starts takes
/// its place, and then its file's row is at rest. Recorded as the form
/// merely opened, the other file's request took the failure's place,
/// so the footer stopped saying it and, back on its file, the person
/// never learnt that its download had failed.
#[gpui::test]
async fn another_file_s_form_keeps_the_failure_until_its_download_starts(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_before_of(&["flow.mmd", "second.mmd"], &[]);
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    running_with_cancel_focused(&app, &browser, cx);
    choose(&browser, cx, file_index(&files, "second.mmd"));
    let words = "Git LFS could not reach origin";
    end_download(&app, &browser, cx, Some(words));
    let failed = format!("Couldn't download flow.mmd's Before object: {words}");
    let footer = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("series-download-failure")?;
        written::get("series-download-failure")
            .and_then(|node| node.node.value().map(str::to_owned))
    };
    assert_eq!(
        footer(cx).as_deref(),
        Some(failed.as_str()),
        "the footer keeps the failure while the review shows another file"
    );
    // second.mmd's Download Before… opens its form, which is cancelled.
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("second.mmd offers Download Before…")
        .center();
    let forms = crate::lfs_download::FORMS_OPENED.get();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(
        crate::lfs_download::FORMS_OPENED.get(),
        forms + 1,
        "its form opens"
    );
    cx.update(|window, cx| window.close_dialog(cx));
    draw(cx);
    assert_eq!(
        footer(cx).as_deref(),
        Some(failed.as_str()),
        "a form opened and cancelled leaves the failure kept"
    );
    // Pressed again, its write starts, which takes the failure's place.
    start_download(&app, &browser, cx, at);
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    draw(cx);
    assert!(
        footer(cx).is_none() && cx.debug_bounds("lfs-download-progress").is_some(),
        "a download that starts replaces the failure, its row saying its progress"
    );
    choose(&browser, cx, file_index(&files, "flow.mmd"));
    assert!(
        cx.debug_bounds("lfs-download-failure").is_none()
            && cx.debug_bounds("download-lfs-side-0").is_some()
            && cx.debug_bounds("series-download-status").is_some(),
        "back on flow.mmd, its row is at rest"
    );
    assert_eq!(
        written::get("series-download-line")
            .and_then(|node| node.node.label().map(str::to_owned))
            .as_deref(),
        Some("Downloading second.mmd's Before object"),
        "and the footer says the download that runs"
    );
    end_download(&app, &browser, cx, None);
}

/// A download ended by the person's own Cancel is no failure (decided
/// 2026-11-10: feedback in place, and a person's own Cancel is not an
/// error). Where the row draws it, the row comes back at rest, its
/// Download Before… where it was pressed, with focus and its pull
/// glyph, and no words beside it; where the review shows another file,
/// the line on the dialog's footer says "Cancelled download of
/// flow.mmd's Before object" for 1.5 s in the normal text colour, never
/// the warning colour, focus going from the vanished Cancel to Back to
/// series; either way it is announced once, politely, never as an
/// alert, and nothing is kept for the person's return. Reported as a
/// failure, its warning words interrupted as an alert and, away from
/// the file, stayed on the footer until the person returned to it.
#[gpui::test]
async fn a_download_the_person_cancels_ends_as_no_failure(cx: &mut gpui_kit::TestAppContext) {
    use crate::copy_feedback::tests::{alerted, announced, click, wait};
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_before_with(&["notes.txt"]);
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    let browser = reviewing_download(&app, cx, &files);
    let said = "Cancelled download of flow.mmd's Before object";
    // Cancelled in the row, where Cancel download holds focus.
    running_with_cancel_focused(&app, &browser, cx);
    click(cx, "cancel-lfs-download");
    draw(cx);
    assert!(
        app.read_with(cx, |app, _| app.operation_cancelling()),
        "Cancel download cancels the window's write"
    );
    let alert = alerted(cx);
    end_download(&app, &browser, cx, Some("Git LFS was stopped"));
    assert!(
        cx.debug_bounds("lfs-download-failure").is_none()
            && cx.debug_bounds("download-lfs-side-0").is_some()
            && crate::lfs_download::drawn_glyph(true, 0).as_deref() != Some("icons/warning.svg")
            && cx.debug_bounds("series-download-status").is_none(),
        "the row comes back at rest, nothing beside its Download"
    );
    assert!(
        holds_focus(&browser, cx, |browser| &browser.download_focus[0]),
        "focus goes from the vanished Cancel to the Download back where it was pressed"
    );
    assert_eq!(alerted(cx), alert, "never as an alert");
    assert_eq!(
        announced(cx).map(|(_, said)| said).as_deref(),
        Some(said),
        "said once, politely"
    );
    wait(cx, crate::copy_feedback::RESTORE_AFTER);
    draw(cx);
    // Cancelled on the dialog's footer, the review showing another file.
    running_with_cancel_focused(&app, &browser, cx);
    choose(&browser, cx, file_index(&files, "notes.txt"));
    click(cx, "cancel-lfs-download");
    draw(cx);
    let alert = alerted(cx);
    end_download(&app, &browser, cx, Some("Git LFS was stopped"));
    let footer = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("series-download-cancelled")?;
        written::get("series-download-cancelled")
            .and_then(|node| node.node.value().map(str::to_owned))
    };
    assert!(
        footer(cx).as_deref() == Some(said)
            && cx.debug_bounds("series-download-failure").is_none()
            && cx.debug_bounds("cancel-lfs-download").is_none(),
        "the footer says the download was cancelled where its line stood"
    );
    let (warning, normal) = cx.update(|_, cx| {
        (
            gpui::Hsla::from(rgb(appearance::labels(cx).warning)),
            gpui_kit::component::Theme::global(cx).foreground,
        )
    });
    assert_ne!(warning, normal, "the two colours differ");
    assert_eq!(
        ENDED_LINE_COLOR.get(),
        Some(normal),
        "in the normal text colour"
    );
    assert!(
        holds_focus(&browser, cx, |browser| &browser.back),
        "focus goes from the vanished Cancel to Back to series"
    );
    assert_eq!(alerted(cx), alert, "never as an alert");
    assert_eq!(
        announced(cx).map(|(_, said)| said).as_deref(),
        Some(said),
        "said once, politely"
    );
    wait(cx, crate::copy_feedback::RESTORE_AFTER);
    draw(cx);
    assert!(
        footer(cx).is_none() && cx.debug_bounds("series-download-status").is_none(),
        "for 1.5 s"
    );
    choose(&browser, cx, file_index(&files, "flow.mmd"));
    assert!(
        cx.debug_bounds("lfs-download-failure").is_none()
            && cx.debug_bounds("download-lfs-side-0").is_some(),
        "nothing is kept for the person's return: the row is at rest"
    );
}

/// While the review shows another file, its download's line stands on
/// the leading side of the dialog's footer, beside Back to repository,
/// never in the scrolling body: at 18 pt in QA's 461 × 490 window at
/// scale 2, with the body scrolled to its end, the line and its Cancel
/// are whole inside the dialog and the window, and nothing in the body
/// moves as the line comes with the download's busy look and goes as it
/// ends. At the top of the body, the line scrolled away with it,
/// overflowed a body the series list's rows do not count, and moved
/// the content by about 50 px as it came and went. It names what
/// downloads, "Downloading flow.mmd's Before object", as its group's
/// name too, and its Cancel what it cancels: it said "Download object".
#[gpui::test]
async fn the_review_s_download_line_stands_on_its_footer(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, repo, files) = missing_before_with(&["notes.txt"]);
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    start_download(&app, &browser, cx, at);
    choose(&browser, cx, file_index(&files, "notes.txt"));
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.update(|window, _| window.set_scale_factor(2.));
    draw(cx);
    draw(cx);
    let placed = |cx: &mut gpui::VisualTestContext| {
        ["back-rewrite-series", "series-files", "series-file-preview"]
            .map(|id| cx.debug_bounds(id).unwrap_or_else(|| panic!("{id}")))
    };
    let resting = placed(cx);
    assert!(
        cx.debug_bounds("series-download-status").is_none(),
        "through the busy delay no line stands"
    );
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    draw(cx);
    draw(cx);
    cx.debug_bounds("series-download-status")
        .expect("the line stands as the write looks busy");
    assert_eq!(
        placed(cx),
        resting,
        "the line coming moves nothing in the body"
    );
    // The body scrolled to its end, past the file list.
    cx.update(|_, cx| {
        let scroll = browser.read(cx).body.scroll().clone();
        scroll.set_offset(point(px(0.), -scroll.max_offset().y));
    });
    draw(cx);
    draw(cx);
    let frame = dialog_frame(cx);
    let window = Bounds::new(point(px(0.), px(0.)), size(px(461.), px(490.)));
    let [status, progress, cancel, close] = [
        "series-download-status",
        "lfs-download-progress",
        "cancel-lfs-download",
        "close-series-review",
    ]
    .map(|id| cx.debug_bounds(id).unwrap_or_else(|| panic!("{id}")));
    let inside = |part: Bounds<Pixels>, outer: Bounds<Pixels>| {
        part.left() >= outer.left()
            && part.right() <= outer.right()
            && part.top() >= outer.top()
            && part.bottom() <= outer.bottom()
    };
    assert!(
        progress.size.width > Pixels::ZERO
            && progress.right() <= cancel.left()
            && cancel.right() <= close.left()
            && inside(cancel, status)
            && inside(status, frame)
            && inside(frame, window),
        "the line's progress, {progress:?}, and Cancel, {cancel:?}, stand whole on the \
         footer, {status:?}, before {close:?}, inside the dialog, {frame:?}, and the window"
    );
    let said = |id: &str| written::get(id).and_then(|node| node.node.label().map(str::to_owned));
    assert_eq!(
        [
            said("series-download-line"),
            said("lfs-download-progress"),
            said("cancel-lfs-download"),
        ],
        [
            "Downloading flow.mmd's Before object",
            "Downloading flow.mmd's Before object",
            "Cancel download of flow.mmd's Before object",
        ]
        .map(|said| Some(said.to_owned())),
        "the line, its text and its Cancel name what downloads"
    );
    cx.update(|_, cx| {
        browser
            .read(cx)
            .body
            .scroll()
            .set_offset(point(px(0.), px(0.)));
    });
    draw(cx);
    end_download(&app, &browser, cx, None);
    draw(cx);
    assert!(
        cx.debug_bounds("series-download-status").is_none(),
        "the line goes as the download ends"
    );
    assert_eq!(
        placed(cx),
        resting,
        "and its going moves nothing in the body"
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A download that ends while the review shows the series, Back to
/// series having left the commit, with Cancel download on the dialog's
/// footer focused, moves focus from the vanished Cancel to the series
/// list, never the window's root, and reads nothing. Its success is said
/// where the line stood: "Downloaded flow.mmd's Before object" with a
/// check for 1.5 s, announced once, politely. The line vanished and
/// nothing said the download had succeeded.
#[gpui::test]
async fn a_download_ending_over_the_series_moves_focus_to_the_list(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    running_with_cancel_focused(&app, &browser, cx);
    cx.update(|window, cx| browser.update(cx, |browser, cx| browser.back(window, cx)));
    crate::tags::tests::draw(cx);
    cx.debug_bounds("series-download-status")
        .expect("the dialog's footer says the download over the series");
    cx.update(|window, cx| browser.read(cx).download_cancel.clone().focus(window, cx));
    crate::tags::tests::draw(cx);
    let before = shown_key(&browser, cx);
    end_download(&app, &browser, cx, None);
    assert!(
        cx.debug_bounds("cancel-lfs-download").is_none()
            && holds_focus(&browser, cx, |browser| &browser.focus),
        "focus goes from the vanished Cancel to the series list"
    );
    assert_eq!(shown_key(&browser, cx), before, "and nothing is read again");
    let done = "Downloaded flow.mmd's Before object";
    let said = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("series-download-done")?;
        crate::native_accessibility::written::get("series-download-done")
            .and_then(|node| node.node.value().map(str::to_owned))
    };
    assert_eq!(
        said(cx).as_deref(),
        Some(done),
        "the footer says the success"
    );
    assert_eq!(
        crate::copy_feedback::tests::announced(cx).map(|(_, said)| said),
        Some(done.to_owned()),
        "once, politely"
    );
    crate::copy_feedback::tests::wait(cx, crate::copy_feedback::RESTORE_AFTER);
    crate::tags::tests::draw(cx);
    assert!(
        said(cx).is_none()
            && cx.debug_bounds("series-download-status").is_none()
            && crate::copy_feedback::tests::announced(cx).is_none(),
        "for 1.5 s"
    );
}

/// A press acts only while the review is current, as Publish does: the
/// form takes the window's repository, so once the window had moved
/// on, to Settings here, a press opened a form for whatever it showed.
#[gpui::test]
async fn a_press_after_the_window_moves_on_opens_no_form(cx: &mut gpui_kit::TestAppContext) {
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let _browser = reviewing_download(&app, cx, &files);
    app.update(cx, |app, cx| {
        app.page = AppPage::Settings;
        cx.notify();
    });
    crate::tags::tests::draw(cx);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("the review still draws Download Before…")
        .center();
    let forms = crate::lfs_download::FORMS_OPENED.get();
    cx.simulate_click(at, Modifiers::none());
    crate::tags::tests::draw(cx);
    assert_eq!(
        crate::lfs_download::FORMS_OPENED.get(),
        forms,
        "a review the window moved on from opens no form"
    );
}

/// Only the caller that asked reads its preview again as the download
/// ends: a download the review asked for captures none of Compare's
/// preview, whose unchanged file it once read again too, while one
/// Compare asked for captures Compare's.
#[gpui::test]
async fn only_the_caller_that_asked_reads_again(cx: &mut gpui_kit::TestAppContext) {
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    app.update(cx, |app, _| {
        app.files = files.clone();
        app.selected_file = Some(0);
    });
    for (asker, captures) in [
        (
            crate::lfs_download::Asker::Review(browser.entity_id()),
            false,
        ),
        (crate::lfs_download::Asker::Compare, true),
    ] {
        let captured = cx.update(|window, cx| {
            app.update(cx, |app, cx| app.simulate_lfs_download(asker, window, cx))
        });
        assert_eq!(
            captured.is_some(),
            captures,
            "{asker:?} captures Compare's preview only where Compare asked"
        );
        end_download(&app, &browser, cx, None);
    }
}

/// A review closed or released while its download runs hands it over
/// to Compare: as the write began, Compare's preview was kept where
/// Compare showed the object it fetches, and the end Compare now owns
/// reads that preview again, so Compare never keeps "not downloaded"
/// after a success. While the review is open its download reads none of
/// Compare's preview, and Compare showing another object keeps none.
/// Left to a closed review, the end reached no one.
#[gpui::test]
async fn a_review_closed_while_its_download_runs_hands_it_to_compare(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::lfs_download::Asker;
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let flow = file_index(&files, "flow.mmd");
    let browser = reviewing_download(&app, cx, &files);
    let target = browser.read_with(cx, |browser, _| {
        let content = browser.content.as_deref().expect("flow.mmd's preview");
        crate::lfs_download::preview_targets(content, &files[flow])
            .into_iter()
            .find_map(|(side, target)| (side == 0).then_some(target))
            .expect("Before's missing object")
    });
    // Compare shows flow.mmd of the same commit.
    app.update(cx, |app, _| {
        app.files = files.clone();
        app.selected_file = Some(flow);
    });
    let begin = |browser: &Entity<SeriesBrowser>, cx: &mut gpui::VisualTestContext| {
        let me = Asker::Review(browser.entity_id());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.simulate_lfs_download_of(me, Some(&target), window, cx)
            })
        })
    };
    let end = |kept, cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.simulate_lfs_download_end_with(None, kept, window, cx)
            })
        })
    };
    let kept = begin(&browser, cx);
    assert!(
        kept.is_some(),
        "Compare's preview is kept where it shows the object"
    );
    assert!(
        end(kept, cx).is_none(),
        "the review open, its end reads none of it"
    );
    let other = files[flow].new_oid.clone();
    app.update(cx, |app, _| app.files[flow].old_oid = other);
    assert!(
        begin(&browser, cx).is_none(),
        "Compare showing another object keeps nothing"
    );
    end(None, cx);
    app.update(cx, |app, _| app.files = files.clone());
    // Closed while it runs.
    let kept = begin(&browser, cx);
    cx.update(|window, cx| browser.update(cx, |browser, cx| browser.close(window, cx)));
    assert_eq!(
        app.read_with(cx, |app, _| app.lfs_downloads.running()),
        Some(Asker::Compare),
        "the closed review hands its download to Compare"
    );
    assert!(
        kept.is_some() && end(kept.clone(), cx) == kept,
        "whose end reads Compare's preview again"
    );
    // Released while it runs, its dialog closed by the window.
    let browser = reviewing_download(&app, cx, &files);
    let kept = begin(&browser, cx);
    cx.update(|window, cx| window.close_dialog(cx));
    let weak = browser.downgrade();
    drop(browser);
    // The last frame holds the dialog's content until the next.
    crate::tags::tests::draw(cx);
    cx.run_until_parked();
    assert!(weak.upgrade().is_none(), "the review is released");
    assert_eq!(
        app.read_with(cx, |app, _| app.lfs_downloads.running()),
        Some(Asker::Compare),
        "a released review hands its download to Compare"
    );
    assert!(kept.is_some() && end(kept.clone(), cx) == kept);
}

/// Delivers `events` to the window with no frame between them, as a
/// double-click's clicks arrive before the first one's frame
/// (`dialog_close_tests::deliver_before_a_frame`).
fn deliver_before_a_frame(cx: &mut gpui::VisualTestContext, events: Vec<PlatformInput>) {
    const EFFECT_ROUNDS: usize = 32;
    fn deliver(
        window: AnyWindowHandle,
        mut events: std::vec::IntoIter<PlatformInput>,
        rounds: usize,
        cx: &mut App,
    ) {
        if rounds > 0 {
            cx.defer(move |cx| deliver(window, events, rounds - 1, cx));
            return;
        }
        let Some(event) = events.next() else {
            return;
        };
        window
            .update(cx, |_, window, cx| window.dispatch_event(event, cx))
            .expect("the window is open");
        cx.defer(move |cx| deliver(window, events, EFFECT_ROUNDS, cx));
    }
    cx.update(|window, cx| {
        let handle = window.window_handle();
        let mut events = events.into_iter();
        if let Some(first) = events.next() {
            window.dispatch_event(first, cx);
        }
        cx.defer(move |cx| deliver(handle, events, EFFECT_ROUNDS, cx));
    });
    cx.run_until_parked();
}

/// A left click's press and release at `at`, the `count`th of a run.
fn click_at(at: Point<Pixels>, count: usize) -> [PlatformInput; 2] {
    [
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: count,
            first_mouse: false,
        }),
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: count,
        }),
    ]
}

/// A second press finds the download's form open and does nothing, in
/// the review and Compare alike: a double-click whose clicks arrived
/// before any frame opened two identical forms. A held Enter, whose
/// X11 repeats arrive as fresh presses, opens one too.
#[gpui::test]
async fn a_second_press_finds_the_form_open(cx: &mut gpui_kit::TestAppContext) {
    let (_fixture, repo, files) = missing_before();
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    let browser = reviewing_download(&app, cx, &files);
    let at = cx
        .debug_bounds("download-lfs-side-0")
        .expect("Download Before… is offered")
        .center();
    let forms = crate::lfs_download::FORMS_OPENED.get();
    let [down, up] = click_at(at, 1);
    let [again, released] = click_at(at, 2);
    deliver_before_a_frame(cx, vec![down, up, again, released]);
    crate::tags::tests::draw(cx);
    assert_eq!(
        crate::lfs_download::FORMS_OPENED.get() - forms,
        1,
        "a double-click before any frame opens one form"
    );
    let target = browser.read_with(cx, |browser, _| {
        let content = browser.content.as_deref().expect("the preview landed");
        crate::lfs_download::preview_targets(content, &files[file_index(&files, "flow.mmd")])
            .remove(0)
            .1
    });
    let opened = cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_lfs_download(target, crate::lfs_download::Asker::Compare, window, cx)
        })
    });
    assert!(!opened, "Compare's press finds the form open too");
    // Closed, it gives focus back to Download Before…, where a held
    // Enter opens one form.
    cx.update(|window, cx| window.close_dialog(cx));
    crate::tags::tests::draw(cx);
    cx.update(|window, cx| browser.read(cx).download_focus[0].clone().focus(window, cx));
    crate::tags::tests::draw(cx);
    let forms = crate::lfs_download::FORMS_OPENED.get();
    for _ in 0..7 {
        enter(cx, true);
    }
    enter(cx, false);
    assert_eq!(
        crate::lfs_download::FORMS_OPENED.get() - forms,
        1,
        "a held Enter opens one form"
    );
}

/// Each Git LFS side stands alone: the side whose object is in the local
/// store shows it and draws its diagrams, and only the side whose object
/// is missing stays its pointer, says so and offers a download. With only
/// the old object missing, Diagrams said the file's new object was not in
/// the local store, which was false.
#[gpui::test]
async fn diagrams_reads_each_lfs_side_on_its_own(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{git, tagged_repository, window};
    use sha2::{Digest, Sha256};
    const POINTER_SIDE: &str = "This side's Git LFS object is not in the local store, and no \
                                download was attempted; its stored text is below.";
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    // `text`'s pointer, with its object in the local store if `local`.
    let pointer = |text: &str, local: bool| {
        let oid = gitturtle_core::lower_hex(Sha256::digest(text));
        if local {
            let directory = repo
                .path()
                .join(".git/lfs/objects")
                .join(&oid[..2])
                .join(&oid[2..4]);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(&oid), text).unwrap();
        }
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            text.len()
        )
    };
    let diagram = "flowchart LR\n  A --> B\n";
    let blank = " \n";
    let gone = |name: &str| pointer(&format!("flowchart LR\n  {name} --> Gone\n"), false);
    let binary = "\0binary\n";
    // Local, and failing verification: the right size, the wrong bytes.
    let damaged = "flowchart LR\n  Damaged --> Object\n";
    let corrupt = pointer(damaged, true);
    let oid = gitturtle_core::lower_hex(Sha256::digest(damaged));
    std::fs::write(
        repo.path()
            .join(".git/lfs/objects")
            .join(&oid[..2])
            .join(&oid[2..4])
            .join(&oid),
        "x".repeat(damaged.len()),
    )
    .unwrap();
    // Past the limit a download or preview takes, so never read.
    let over_limit = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        "e".repeat(64),
        33 * 1024 * 1024
    );
    let sides = [
        ("old-gone.mmd", gone("One"), pointer(diagram, true)),
        ("old-gone-blank.mmd", gone("Two"), pointer(blank, true)),
        ("new-gone.mmd", pointer(diagram, true), gone("Three")),
        ("new-gone-blank.mmd", pointer(blank, true), gone("Four")),
        ("new-binary.mmd", gone("Five"), pointer(binary, true)),
        (
            "new-oversize.mmd",
            gone("Six"),
            pointer(&"x\n".repeat(1_100_000), true),
        ),
        ("new-corrupt.mmd", gone("Seven"), corrupt),
        ("new-over-limit.mmd", gone("Eight"), over_limit),
        ("old-binary.mmd", pointer(binary, true), gone("Nine")),
    ];
    for (path, before, _) in &sides {
        std::fs::write(repo.path().join(path), before).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "Before"]);
    for (path, _, after) in &sides {
        std::fs::write(repo.path().join(path), after).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "After"]);
    let files = last_commit_files(&repo);
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let browser = inspecting(&app, cx, &files);
    for (name, path, kept) in [
        ("only the old object missing", "old-gone.mmd", 0),
        ("only the new object missing", "new-gone.mmd", 1),
    ] {
        let index = file_index(&files, path);
        choose(&browser, cx, index);
        let (literal, resolved, targets) = browser.read_with(cx, |browser, _| {
            let content = browser.content.as_deref().expect("the preview landed");
            let Content::Text {
                diagrams: Some(diagrams),
                ..
            } = content
            else {
                panic!("{name}: the side with its object draws");
            };
            let (literal, resolved) = if kept == 0 {
                (diagrams.old.clone(), diagrams.new.clone())
            } else {
                (diagrams.new.clone(), diagrams.old.clone())
            };
            let targets = crate::lfs_download::preview_targets(content, &files[index])
                .into_iter()
                .map(|(side, _)| side)
                .collect::<Vec<_>>();
            (literal, resolved, targets)
        });
        assert!(
            resolved.pages.len() == 1 && resolved.pages[0].render.is_some(),
            "{name}: the side with its object draws its diagram"
        );
        assert_eq!(
            literal.metadata.format, "Git LFS pointer",
            "{name}: the other side is its pointer"
        );
        assert_eq!(
            literal.metadata.details,
            [POINTER_SIDE],
            "{name}: the words name that side alone"
        );
        assert_eq!(targets, [kept], "{name}: only that side offers a download");
    }
    // A local object is never called missing or offered for download,
    // whichever side the reason judges.
    for (name, path, reason, missing) in [
        (
            "the old object missing beside a blank new file",
            "old-gone-blank.mmd",
            NO_DIAGRAMS,
            0,
        ),
        (
            "the new object missing beside a blank old file",
            "new-gone-blank.mmd",
            NO_LFS_OBJECT,
            1,
        ),
        (
            "the old object missing beside a binary new object",
            "new-binary.mmd",
            NO_LFS_TEXT,
            0,
        ),
        (
            "the old object missing beside an oversize new object",
            "new-oversize.mmd",
            NO_LFS_TEXT,
            0,
        ),
        (
            "the old object missing beside a corrupt new object",
            "new-corrupt.mmd",
            NO_LFS_TEXT,
            0,
        ),
        (
            "the old object missing beside a new object past the limit",
            "new-over-limit.mmd",
            NO_LFS_TEXT,
            0,
        ),
        (
            "the new object missing beside a binary old object",
            "old-binary.mmd",
            NO_LFS_OBJECT,
            1,
        ),
    ] {
        let index = file_index(&files, path);
        choose(&browser, cx, index);
        let targets = browser.read_with(cx, |browser, _| {
            let content = browser.content.as_deref().expect("the preview landed");
            assert!(
                matches!(content, Content::Text { diagrams: None, .. }),
                "{name}: no side draws"
            );
            crate::lfs_download::preview_targets(content, &files[index])
                .into_iter()
                .map(|(side, _)| side)
                .collect::<Vec<_>>()
        });
        let node = crate::native_accessibility::written::get(TEXT_MODES[3].1)
            .unwrap_or_else(|| panic!("Diagrams is drawn for {name}"))
            .node;
        assert_eq!(
            node.description(),
            Some(reason),
            "Diagrams says why, {name}"
        );
        assert_eq!(
            targets,
            [missing],
            "{name}: only the missing side offers a download"
        );
    }
}

/// Diagrams says why a file shows none: a Mermaid file stored as a
/// link, that Diagrams never follows a link; one whose Git LFS object is
/// not in the local store, that; and a file that is not Mermaid or
/// Markdown, what Diagrams shows. "This file has no Mermaid diagrams."
/// stood for all three, though the link's target or the object could
/// hold diagrams. The reason judges the side Diagrams would draw, the
/// one after the change or, for a file it removed, the one before: a
/// Mermaid file changed from a link to a blank regular file once read as
/// stored as a link.
///
/// Only the literal side stays literal. Where one side of the change is
/// a link or a Git LFS pointer and the other holds Mermaid text, the
/// other draws its diagram, Diagrams opens and the literal side says
/// what it is: an `.mmd` going from a link or a pointer to real Mermaid
/// text read "This file has no Mermaid diagrams.", which was untrue.
#[gpui::test]
async fn diagrams_says_why_a_file_shows_none(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
    use std::os::unix::fs::symlink;
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let diagram = "flowchart LR\n  A --> B\n";
    // A Git LFS pointer whose object the local store does not hold.
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
        "a".repeat(64)
    );
    symlink("notes.txt", repo.path().join("was-link.mmd")).unwrap();
    symlink("notes.txt", repo.path().join("gone-link.mmd")).unwrap();
    symlink("notes.txt", repo.path().join("link-to-blank.mmd")).unwrap();
    std::fs::write(repo.path().join("now-link.mmd"), diagram).unwrap();
    std::fs::write(repo.path().join("was-pointer.mmd"), &pointer).unwrap();
    std::fs::write(repo.path().join("pointer-to-blank.mmd"), &pointer).unwrap();
    std::fs::write(repo.path().join("now-pointer.mmd"), diagram).unwrap();
    std::fs::write(repo.path().join("blank-to-link.mmd"), " \n").unwrap();
    std::fs::write(repo.path().join("blank-to-pointer.mmd"), " \n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add the sides"]);
    std::fs::remove_file(repo.path().join("blank-to-link.mmd")).unwrap();
    symlink("notes.txt", repo.path().join("blank-to-link.mmd")).unwrap();
    std::fs::write(repo.path().join("blank-to-pointer.mmd"), &pointer).unwrap();
    std::fs::remove_file(repo.path().join("was-link.mmd")).unwrap();
    std::fs::write(repo.path().join("was-link.mmd"), diagram).unwrap();
    std::fs::remove_file(repo.path().join("gone-link.mmd")).unwrap();
    std::fs::remove_file(repo.path().join("link-to-blank.mmd")).unwrap();
    std::fs::write(repo.path().join("link-to-blank.mmd"), " \n").unwrap();
    std::fs::remove_file(repo.path().join("now-link.mmd")).unwrap();
    symlink("notes.txt", repo.path().join("now-link.mmd")).unwrap();
    std::fs::write(repo.path().join("was-pointer.mmd"), diagram).unwrap();
    std::fs::write(repo.path().join("pointer-to-blank.mmd"), " \n").unwrap();
    std::fs::write(repo.path().join("now-pointer.mmd"), &pointer).unwrap();
    std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
    std::fs::write(repo.path().join("blank.mmd"), " \n\n").unwrap();
    std::fs::write(repo.path().join("link.mmd"), "flowchart LR\n  A --> B\n").unwrap();
    std::fs::write(repo.path().join("missing.mmd"), &pointer).unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "Add files"]);
    let mut files = last_commit_files(&repo);
    let (link, missing, notes) = (
        file_index(&files, "link.mmd"),
        file_index(&files, "missing.mmd"),
        file_index(&files, "notes.txt"),
    );
    let blank = file_index(&files, "blank.mmd");
    let (was_link, now_link, gone_link) = (
        file_index(&files, "was-link.mmd"),
        file_index(&files, "now-link.mmd"),
        file_index(&files, "gone-link.mmd"),
    );
    let (was_pointer, now_pointer) = (
        file_index(&files, "was-pointer.mmd"),
        file_index(&files, "now-pointer.mmd"),
    );
    let (link_to_blank, pointer_to_blank) = (
        file_index(&files, "link-to-blank.mmd"),
        file_index(&files, "pointer-to-blank.mmd"),
    );
    let (blank_to_link, blank_to_pointer) = (
        file_index(&files, "blank-to-link.mmd"),
        file_index(&files, "blank-to-pointer.mmd"),
    );
    // Stored as a link, whose text names its target.
    files[link].new_mode = "120000".into();
    let (app, cx) = window(cx, &repo);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let browser = inspecting(&app, cx, &files);
    for (name, index, reason) in [
        ("a Mermaid file stored as a link", link, LINKED),
        (
            "a Mermaid file whose LFS object is missing",
            missing,
            NO_LFS_OBJECT,
        ),
        ("a text file", notes, NOT_DIAGRAMS),
        ("a blank Mermaid file", blank, NO_DIAGRAMS),
        ("a link changed to a blank file", link_to_blank, NO_DIAGRAMS),
        (
            "a pointer changed to a blank file",
            pointer_to_blank,
            NO_DIAGRAMS,
        ),
        ("a link the commit removed", gone_link, LINKED),
        ("a blank file changed to a link", blank_to_link, LINKED),
        (
            "a blank file changed to a pointer",
            blank_to_pointer,
            NO_LFS_OBJECT,
        ),
    ] {
        choose(&browser, cx, index);
        assert!(
            browser.read_with(cx, |browser, _| matches!(
                browser.content.as_deref(),
                Some(Content::Text { diagrams: None, .. })
            )),
            "{name}'s text lands, with no diagrams"
        );
        let node = crate::native_accessibility::written::get(TEXT_MODES[3].1)
            .unwrap_or_else(|| panic!("Diagrams is drawn for {name}"))
            .node;
        assert!(node.is_disabled(), "Diagrams waits, for {name}");
        assert_eq!(
            node.description(),
            Some(reason),
            "Diagrams says why, for {name}"
        );
    }
    const LINK_SIDE: &str = "This side is stored as a link, which Diagrams never follows; \
                             its stored text is below.";
    const POINTER_SIDE: &str = "This side's Git LFS object is not in the local store, and no \
                                download was attempted; its stored text is below.";
    for (name, index, literal, format, says) in [
        (
            "a link changed to a regular file",
            was_link,
            0,
            "Symbolic link",
            LINK_SIDE,
        ),
        (
            "a pointer changed to a regular file",
            was_pointer,
            0,
            "Git LFS pointer",
            POINTER_SIDE,
        ),
        (
            "a regular file changed to a link",
            now_link,
            1,
            "Symbolic link",
            LINK_SIDE,
        ),
        (
            "a regular file changed to a pointer",
            now_pointer,
            1,
            "Git LFS pointer",
            POINTER_SIDE,
        ),
    ] {
        choose(&browser, cx, index);
        let (kept, drawn) = browser.read_with(cx, |browser, _| {
            let Some(Content::Text {
                diagrams: Some(diagrams),
                ..
            }) = browser.content.as_deref()
            else {
                panic!("{name}: the regular side draws its diagram");
            };
            if literal == 0 {
                (diagrams.old.clone(), diagrams.new.clone())
            } else {
                (diagrams.new.clone(), diagrams.old.clone())
            }
        });
        assert!(
            drawn.pages.len() == 1
                && drawn.pages[0].render.is_some()
                && drawn.pages[0].error.is_none(),
            "{name}: the regular side draws one diagram"
        );
        assert!(
            kept.pages.is_empty() && kept.page_count == 0,
            "{name}: the literal side is never decoded"
        );
        assert!(
            kept.captured.is_none() && kept.system_preview.is_none(),
            "{name}: the literal side offers nothing to open"
        );
        assert_eq!(
            kept.metadata.details,
            [says],
            "{name}: the literal side says what it is"
        );
        cx.update(|_, cx| {
            browser.update(cx, |browser, cx| {
                browser.text_mode = 3;
                cx.notify();
            })
        });
        draw(cx);
        let node = crate::native_accessibility::written::get(TEXT_MODES[3].1)
            .unwrap_or_else(|| panic!("Diagrams is drawn for {name}"))
            .node;
        assert!(!node.is_disabled(), "Diagrams opens, for {name}");
        rendered(cx, format!("rich-side-format-{literal}/{format}").leak());
        let reason =
            crate::native_accessibility::written::get(&format!("rich-side-detail-{literal}-0"))
                .unwrap_or_else(|| panic!("{name}: the literal side's reason is written"))
                .node;
        assert_eq!(
            reason.role(),
            Role::Label,
            "{name}: the reason is static text"
        );
        assert_eq!(
            reason.value(),
            Some(says),
            "{name}: assistive technology reads the reason"
        );
        let copy = if format == "Git LFS pointer" {
            "Copy pointer"
        } else {
            "Copy source"
        };
        let side = if literal == 0 { "Before" } else { "After" };
        let control =
            crate::native_accessibility::written::get(&format!("copy-decoded-source-{literal}"))
                .unwrap_or_else(|| panic!("{name}: the literal side's copy control is drawn"))
                .node;
        assert_eq!(
            control.label(),
            Some(format!("{side}: {copy}").as_str()),
            "{name}: the copy control names what it copies"
        );
    }
}

/// Whether the pointer resting at `at` shows the progress's tooltip
/// saying `text`; the pointer then leaves the window until it has gone.
fn tooltip_at(cx: &mut gpui::VisualTestContext, at: Point<Pixels>, text: &str) -> bool {
    use crate::tags::tests::draw;
    const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(600);
    cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
    draw(cx);
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.executor().advance_clock(SHOW_DELAY);
    draw(cx);
    let shown = cx
        .debug_bounds(format!("lfs-download-progress/tooltip/{text}").leak())
        .is_some();
    cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
    cx.executor().advance_clock(SHOW_DELAY);
    draw(cx);
    shown
}

/// Whether the pointer resting on action `id` shows the tooltip `text`,
/// as the window draws it under the pointer; the pointer then leaves the
/// window until the tooltip has gone. It leaves first, so an action it
/// rests on already, as after a press, is entered again.
fn hover_shows(cx: &mut gpui::VisualTestContext, id: &'static str, text: &str) -> bool {
    use crate::tags::tests::draw;
    const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(600);
    cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
    draw(cx);
    let action = cx.debug_bounds(id).expect("the action is drawn");
    cx.simulate_mouse_move(action.center(), None, Modifiers::default());
    cx.executor().advance_clock(SHOW_DELAY);
    draw(cx);
    let shown = cx
        .debug_bounds(format!("{id}/tooltip/{text}").leak())
        .is_some();
    cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
    cx.executor().advance_clock(SHOW_DELAY);
    draw(cx);
    shown
}

/// Mid-merge, where the palette refuses its Edit local commits… and the
/// branch menu leaves it out, the review's Edit local commits…, beside
/// the line saying no rebase captured a series, is refused with the
/// palette's reason, in its tooltip and its accessible description, and
/// a press opens nothing: it was offered, refused only while a write
/// ran. Once the review's repository is no longer the one shown, it says
/// so, where it said to wait for a Git operation.
#[gpui::test]
async fn edit_local_commits_waits_for_a_merge_under_way(cx: &mut gpui_kit::TestAppContext) {
    use crate::native_accessibility::written;
    use crate::tags::tests::draw;
    let (_fixture, app, cx) = series_window(cx, 0).await;
    size_to(&app, cx, 13, 1480., 973.);
    let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
    let git = |args: &[&str]| {
        let identity = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ];
        crate::tags::tests::git(repo.path(), &[&identity[..], args].concat());
    };
    git(&["checkout", "--quiet", "-b", "side", "main~1"]);
    std::fs::write(repo.path().join("side.txt"), "side\n").unwrap();
    git(&["add", "side.txt"]);
    git(&["commit", "--quiet", "-m", "side"]);
    git(&["checkout", "--quiet", "main"]);
    git(&["merge", "--quiet", "--no-commit", "--no-ff", "side"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    assert!(
        app.read_with(cx, |app, _| app.integration_state.is_some()),
        "a merge is under way"
    );
    let reason = cx
        .update(|window, cx| {
            app.read(cx)
                .menu_command_reason(crate::command_palette::CommandId::Rebase, window, cx)
        })
        .expect("the palette refuses Edit local commits… mid-merge");
    let _review = review_from_palette(&app, cx);
    assert_eq!(
        crate::empty_state::shown("series-empty").as_deref(),
        Some(recovery_drafts::NO_CAPTURE),
        "the review has no captured series to show"
    );
    let node = written::get("series-edit-commits")
        .expect("Edit local commits…' node is written")
        .node;
    assert!(node.is_disabled(), "the merge refuses Edit local commits…");
    assert_eq!(
        node.description(),
        Some(reason),
        "Edit local commits… says why with the palette's reason"
    );
    assert!(
        hover_shows(cx, "series-edit-commits", reason),
        "its tooltip says why"
    );
    let edit = cx
        .debug_bounds("series-edit-commits")
        .expect("Edit local commits… is drawn");
    cx.simulate_click(edit.center(), Modifiers::none());
    draw(cx);
    assert!(
        cx.debug_bounds("load-rebase-commits").is_none()
            && cx.debug_bounds("series-focus-list").is_some(),
        "a press on the refused Edit local commits… opens nothing"
    );
    // The repository no longer the one shown.
    app.update(cx, |app, cx| {
        app.page = AppPage::Settings;
        cx.notify();
    });
    draw(cx);
    let node = written::get("series-edit-commits")
        .expect("Edit local commits…' node is written")
        .node;
    assert!(
        node.is_disabled(),
        "a repository no longer shown refuses it"
    );
    assert_eq!(
        node.description(),
        Some(NOT_SHOWN),
        "Edit local commits… says its repository is no longer shown"
    );
}

/// With a capture present, Check publication destination… is offered,
/// and Tab onto it at 18 pt in a 461 × 490 window reveals it with its
/// ring whole, below the publication fields.
#[gpui::test]
async fn check_publication_destination_draws_its_ring_whole(cx: &mut gpui_kit::TestAppContext) {
    use crate::tags::tests::draw;
    let (_fixture, review, cx) = rewritten_review(
        cx,
        &[&[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "fixture notes 2, reworded",
        ]],
    )
    .await;
    let app = cx.update(|_, cx| review.read(cx).owner.upgrade().expect("the app"));
    crate::focus_ring::sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    draw(cx);
    let check = cx.update(|_, cx| review.read(cx).body.focus("inspect-publication-remote", cx));
    for _ in 0..12 {
        if cx.update(|window, _| check.is_focused(window)) {
            break;
        }
        cx.simulate_keystrokes("tab");
        draw(cx);
    }
    assert!(
        cx.update(|window, _| check.is_focused(window)),
        "Tab reaches Check publication destination…"
    );
    crate::tags::tests::assert_rings_whole(cx, &["inspect-publication-remote"]);
    let body = cx
        .debug_bounds("rewrite-review-body")
        .expect("the body is drawn");
    let control = cx
        .debug_bounds("inspect-publication-remote")
        .expect("Check publication destination… is drawn");
    assert!(
        lies_in(control, body),
        "Check publication destination… at {control:?} is revealed in the body, {body:?}"
    );
}

/// A rebase run through Edit local commits' Start rebase is reviewed
/// from the palette, as QA-106e reviewed one: its capture is found and
/// the review lists its series, and Refresh series lists it again,
/// selecting its first row and showing it whole at 18 pt in a 461 × 490
/// half tile, where the list shows one row: with the last row selected
/// by Down before it, the reload selected row 0 and went on showing the
/// last.
#[gpui::test]
async fn a_rebase_started_in_edit_local_commits_is_reviewed_from_the_palette(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_fixture, app, cx) = series_window(cx, 0).await;
    size_to(&app, cx, 13, 1480., 973.);
    rebase_dropping_the_second(&app, cx, "main~3").await;
    size_to(&app, cx, 18, 461., 490.);
    let review = review_from_palette(&app, cx);
    let listed = |cx: &mut gpui::VisualTestContext| {
        review.read_with(cx, |review, _| {
            (
                review.error.clone(),
                review.review.as_ref().map(|review| {
                    review
                        .rows
                        .iter()
                        .map(|row| row.change.label())
                        .collect::<Vec<_>>()
                }),
            )
        })
    };
    let (error, rows) = listed(cx);
    assert_eq!(error, None, "the captured series is read");
    let rows = rows.expect("the review lists the captured series");
    assert_eq!(rows.len(), 3, "its three original commits: {rows:?}");
    assert!(
        rows.contains(&"Dropped / unmatched"),
        "the dropped commit is listed: {rows:?}"
    );
    assert!(
        cx.debug_bounds("series-empty").is_none(),
        "a listed series draws no empty state"
    );
    cx.simulate_keystrokes("down down");
    crate::tags::tests::draw(cx);
    assert!(
        shows_only(cx, 2, rows.len()),
        "Down selects the last row and shows it alone"
    );
    // Refresh series, reached by Tab, which reveals it below the half
    // tile's fold, and pressed with Return: a click at its place there
    // reached nothing, so the series was never read again.
    let reload = cx.update(|_, cx| review.read(cx).body.focus("reload-series", cx));
    for _ in 0..6 {
        if cx.update(|window, _| reload.is_focused(window)) {
            break;
        }
        cx.simulate_keystrokes("tab");
        crate::tags::tests::draw(cx);
    }
    assert!(
        cx.update(|window, _| reload.is_focused(window)),
        "Tab moves onto Refresh series"
    );
    let read = review.read_with(cx, |review, _| review.generation);
    enter(cx, true);
    enter(cx, false);
    lands(&review, cx);
    assert_ne!(
        review.read_with(cx, |review, _| review.generation),
        read,
        "Refresh series reads the series again"
    );
    assert_eq!(
        listed(cx).1.map(|rows| rows.len()),
        Some(3),
        "Refresh series lists it again"
    );
    assert_eq!(
        review.read_with(cx, |review, _| review.selected),
        0,
        "the reload selects the series' first row"
    );
    assert!(
        shows_only(cx, 0, rows.len()),
        "the list shows the selected first row whole"
    );
}

/// QA-106e's route: the palette's review before any capture, then a
/// rebase through Edit local commits' Start rebase, the review opened
/// again and Refresh series pressed. Its series holds a commit whose
/// patch is past the per-command bound on what a read keeps (4 MiB), as
/// QA-106e's two 30 MB images were, so the capture is found but the
/// bounded review refuses the series: the review says why in the list's
/// place, where it drew an empty list with no text.
#[gpui::test]
async fn a_series_past_the_review_bound_says_why_after_its_rebase(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_fixture, app, cx) = series_window(cx, 4_500_000).await;
    // The app's focus ring, which `says_why` finds whole.
    crate::focus_ring::sweep::prepare(&app, cx);
    size_to(&app, cx, 18, 461., 490.);
    let review = review_from_palette(&app, cx);
    says_why(&review, cx, &UNCAPTURED, "before the rebase");
    cx.simulate_keystrokes("escape");
    crate::tags::tests::draw(cx);
    size_to(&app, cx, 13, 1480., 973.);
    rebase_dropping_the_second(&app, cx, "main~3").await;
    size_to(&app, cx, 18, 461., 490.);
    let review = review_from_palette(&app, cx);
    let error = review
        .read_with(cx, |review, _| review.error.clone())
        .expect("the bounded review refuses the series");
    assert_eq!(
        error,
        "This series is too large to review here: its patches pass the review's 4 MiB read \
         limit. Compare it with `git range-diff`.",
        "the capture is found and its series is past the bound"
    );
    let says = Says {
        line: &error,
        title: Some(UNREVIEWABLE),
        edit: false,
    };
    says_why(&review, cx, &says, "after the rebase");
    refresh(&review, cx, &says, "after the rebase");
    says_why(&review, cx, &says, "after Refresh series");
}
