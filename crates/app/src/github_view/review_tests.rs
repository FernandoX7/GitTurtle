use super::*;
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::{cell::RefCell, rc::Rc};

/// [`clear_of_lines`] with a pass over every line for each place it asks about, as it
/// stood, the reference [`Insides`] must agree with; it chooses among the places as
/// [`clear_of_lines`] does ([`stop`]).
fn clear_of_lines_by_pass(
    viewport: Bounds<Pixels>,
    edge: Pixels,
    (top, bottom): (Pixels, Pixels),
    lines: &[(Pixels, Pixels)],
    starts: &[Pixels],
    (first, last): (Pixels, Pixels),
) -> Pixels {
    let height = viewport.size.height;
    let slack = px(0.5);
    let within = px(0.01);
    let (top, bottom) = if bottom - top <= height {
        (top, bottom)
    } else if (edge - top).abs() <= slack {
        (top, top + height / 2.)
    } else {
        (edge + height / 2., edge + height)
    };
    let cuts = |at: Pixels| {
        lines
            .iter()
            .any(|(line_top, line_bottom)| at > *line_top + within && at < *line_bottom - within)
    };
    let lands = |at: Pixels| {
        at >= first - slack
            && at <= last + slack
            && at <= top + within
            && at + height >= bottom - within
            && !cuts(at)
            && !cuts(at + height)
    };
    if lands(edge) || edge <= first + slack {
        return edge;
    }
    stop(viewport, edge, lines, starts, (first, last), lands).unwrap_or(edge)
}

/// [`clear_of_lines`], which asks [`Insides`] whether an edge cuts a line, stops where a
/// pass over every line stopped, so its stops are the same, on lines that overlap, touch,
/// lie inside a frame, run past the view and stand apart, with parts shorter and taller
/// than the view, scrolling either way, near the content's ends and with no line at all.
#[test]
fn insides_stop_where_a_pass_over_every_line_stopped() {
    // A fixed linear congruential sequence, so every run checks the same cases.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = |below: u32| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as u32) % below
    };
    let mut moved = 0;
    for case in 0..4000 {
        let height = px(60. + next(240) as f32);
        let from = px(next(800) as f32 / 2.);
        let viewport = Bounds::new(point(px(0.), from), size(px(300.), height));
        let mut lines = Vec::new();
        let mut at = px(next(40) as f32);
        for _ in 0..next(40) {
            let tall = px(10. + next(60) as f32 / 2.);
            // Apart, touching or overlapping the line before, now and then within it.
            let gap = px(next(30) as f32 / 2.) - px(5.);
            at += gap.max(px(-tall.as_f32() + 0.5));
            lines.push((at, at + tall));
            at += tall;
        }
        // Now and then a frame around some of them, as a one-line control stands beside
        // a text that wraps, so a line can lie wholly inside another that starts before it.
        for _ in 0..next(3) {
            if lines.is_empty() {
                break;
            }
            let first = next(lines.len() as u32) as usize;
            let last = first + next((lines.len() - first) as u32) as usize;
            let (top, bottom) = (lines[first].0, lines[last].1);
            lines.push((
                top - px(next(8) as f32 / 2.),
                bottom + px(next(8) as f32 / 2.),
            ));
        }
        let starts: Vec<Pixels> = (0..next(12)).map(|_| px(next(1600) as f32 / 2.)).collect();
        let top = px(next(1600) as f32 / 2.);
        let part = (top, top + px(5. + next(400) as f32 / 2.));
        let first = px(next(60) as f32) - px(30.);
        let range = (first, first + px(next(1400) as f32 / 2.));
        let edge = px(next(1600) as f32 / 2.);
        let fast = clear_of_lines(viewport, edge, part, &lines, &starts, range);
        let pass = clear_of_lines_by_pass(viewport, edge, part, &lines, &starts, range);
        assert_eq!(
            fast, pass,
            "case {case}: {viewport:?}, edge {edge:?}, part {part:?}, lines {lines:?}, \
             starts {starts:?}, range {range:?}"
        );
        moved += usize::from(fast != edge);
    }
    // Hundreds of cases move the edge (930 of them), so the stops, not only the
    // landing, are compared.
    assert!(moved > 500, "{moved} of 4000 cases move the edge");
}
/// A reveal scrolling down from a view at 100..300 lands its top edge at 150, inside the
/// line at 140..160: it stops the least less that shows the line whole where the part it
/// shows stays whole, otherwise goes on until the line is out of view, and, where the
/// content ends before that, leaves the part whole where the reveal put it. A line its
/// bottom edge cuts is shown whole or left out of view the same way.
#[test]
fn a_reveal_stops_clear_of_the_lines_at_its_edges() {
    let viewport = Bounds::new(point(px(0.), px(100.)), size(px(300.), px(200.)));
    let top_line = [(px(140.), px(160.))];
    let range = (px(0.), px(1000.));
    let part = |bottom: f32| (px(300.), px(bottom));
    assert_eq!(
        clear_of_lines(
            viewport,
            px(150.),
            part(340.),
            &top_line,
            &[px(140.)],
            range
        ),
        px(140.),
        "a little less shows the line whole, the part still whole"
    );
    assert_eq!(
        clear_of_lines(
            viewport,
            px(150.),
            part(345.),
            &top_line,
            &[px(140.), px(160.)],
            range
        ),
        px(160.),
        "where less would cut the part, a little more takes the line out of view"
    );
    assert_eq!(
        clear_of_lines(
            viewport,
            px(150.),
            part(345.),
            &top_line,
            &[px(140.), px(160.)],
            (px(0.), px(150.))
        ),
        px(150.),
        "where the content ends, the part whole wins"
    );
    let bottom_line = [(px(340.), px(360.))];
    assert_eq!(
        clear_of_lines(
            viewport,
            px(150.),
            part(335.),
            &bottom_line,
            &[px(140.)],
            range
        ),
        px(140.),
        "a little less takes the line the bottom edge cut out of view"
    );
    assert_eq!(
        clear_of_lines(
            viewport,
            px(150.),
            part(345.),
            &bottom_line,
            &[px(140.), px(160.)],
            range
        ),
        px(160.),
        "a little more shows it whole"
    );
    // A part taller than the view, shown from its top at 150 as a pane that draws its own
    // ring is: it keeps the half of the view below its top, so where the content starts
    // just above it, with no block to stop at, the reveal stays where it landed. Where
    // the content goes on above, the view stops a little less, its bottom edge at the
    // line's top, or, where that would cut another line at its top edge, at the nearest
    // place clear of both, the part's top, its leading edge, still in view either way.
    let tall = (px(150.), px(500.));
    assert_eq!(
        clear_of_lines(
            viewport,
            px(150.),
            tall,
            &bottom_line,
            &[],
            (px(145.), px(1000.))
        ),
        px(150.),
        "with nowhere clear to stop, the tall part stays where it landed"
    );
    assert_eq!(
        clear_of_lines(viewport, px(150.), tall, &bottom_line, &[px(120.)], range),
        px(140.),
        "a little less takes the line out of view, the tall part's top still in view"
    );
    let above = [(px(135.), px(145.)), (px(340.), px(360.))];
    assert_eq!(
        clear_of_lines(viewport, px(150.), tall, &above, &[px(120.)], range),
        px(135.),
        "the nearest place clear of both lines, the line above's top, wins over the block \
         start farther up, the tall part's top still in view"
    );
    // Shift+Tab: a reveal scrolling up from a view at 400..600 lands at 190, its bottom
    // edge inside the line at 380..400, and less and more are mirrored: less stops lower.
    let below = Bounds::new(point(px(0.), px(400.)), size(px(300.), px(200.)));
    let line = [(px(380.), px(400.))];
    let starts = [px(170.), px(200.)];
    assert_eq!(
        clear_of_lines(below, px(190.), (px(200.), px(260.)), &line, &starts, range),
        px(200.),
        "scrolling up, a little less shows the line whole"
    );
    assert_eq!(
        clear_of_lines(below, px(190.), (px(190.), px(260.)), &line, &starts, range),
        px(180.),
        "scrolling up, where less would cut the part, a little more takes the line out, \
         the bottom edge at its top"
    );
}

/// Where no block start leaves every line whole or out of view, nor the bottom edge at a
/// line's top or foot, the reveal stops between two lines, its top edge at a line's top or
/// foot: a view at 100..300 scrolled down to 150, its top edge inside the first of three
/// lines at 145..175; the bottom edge at the top or foot of the line at 350..370 below them
/// would leave the top edge inside one of the three. The nearest place clear of every line
/// wins over a block start farther on (decision, 2026-11-05); otherwise the least less,
/// the paragraph's first line whole, and where that would cut the part, the least more,
/// past the line at the bottom edge. As macOS wraps the connection help at 18 pt in a
/// 461 × 490 window, no block start kept Connect whole with both edges clear, and Tab onto
/// it left Disconnect cut at the body's foot.
#[test]
fn where_no_block_start_clears_the_lines_the_edge_stops_between_two() {
    let viewport = Bounds::new(point(px(0.), px(100.)), size(px(300.), px(200.)));
    let lines = [
        (px(145.), px(155.)),
        (px(155.), px(165.)),
        (px(165.), px(175.)),
        (px(350.), px(370.)),
    ];
    let range = (px(0.), px(1000.));
    let part = |bottom: f32| (px(250.), px(bottom));
    assert_eq!(
        clear_of_lines(
            viewport,
            px(150.),
            part(330.),
            &lines,
            &[px(148.), px(240.)],
            range
        ),
        px(145.),
        "the nearest place clear of every line wins over a block start farther on"
    );
    assert_eq!(
        clear_of_lines(viewport, px(150.), part(330.), &lines, &[px(148.)], range),
        px(145.),
        "with no block start clear, the least less between two lines"
    );
    assert_eq!(
        clear_of_lines(viewport, px(150.), part(346.), &lines, &[px(148.)], range),
        px(175.),
        "where less would cut the part, the least more between two lines"
    );
}

/// Where no block start clears every line, the reveal stops where the view's bottom edge
/// lies at a line's top or foot before it stops between two lines: the connection pane at
/// 17 pt in a 440 × 490 window, as Linux lays it out, scrolled up by Shift+Tab onto Account
/// options, its body 271 px tall. The reveal lands at Account options' row, 142, its bottom
/// edge inside Connect; no block start keeps the control whole with both edges clear, and
/// no line's top or foot as the top edge, which once kept the landing where it cut Connect
/// (or, with the help's lines counted 25 px apart for 25.5, stopped at 91.5, the help's
/// fifth line cut by 2.5 px). The bottom edge at the foot of that line, 365, clears every
/// line, the top edge between Refresh and New pull request.
#[test]
fn where_no_block_start_clears_the_lines_the_bottom_edge_stops_at_one() {
    let viewport = Bounds::new(point(px(38.), px(396.)), size(px(364.), px(271.)));
    let help = (0..6).map(|line| {
        let top = px(237.5) + px(25.5) * line as f32;
        (top, top + px(25.5))
    });
    let lines: Vec<_> = [
        (px(12.5), px(46.5)),
        (px(55.), px(91.5)),
        (px(100.), px(136.5)),
        (px(145.), px(181.5)),
        (px(203.5), px(229.)),
    ]
    .into_iter()
    .chain(help)
    .chain([(px(399.), px(435.5)), (px(444.), px(480.5))])
    .collect();
    let ring = px(3.);
    let starts: Vec<_> = [12.5, 55., 100., 145., 203.5, 237.5, 399., 444.]
        .into_iter()
        .map(|start| px(start) - ring)
        .collect();
    let range = (px(9.5), px(798.5));
    let part = (px(142.), px(184.5));
    assert_eq!(
        clear_of_lines(viewport, px(142.), part, &lines, &starts, range),
        px(94.),
        "the bottom edge at the foot of the help's fifth line"
    );
    // Landed at the content's top, as Tab wrapping to the first part does, the reveal
    // keeps the box's top though its bottom edge cuts a line whose foot, 8.5 px lower,
    // would clear every line with the part's ring still whole: that place cuts the box
    // above the part, as it once cut the offline fixture's banner at 18 pt.
    let top = Bounds::new(point(px(204.), px(600.)), size(px(1072.), px(544.)));
    let banner = (px(133.), px(178.));
    let foot = [(px(649.5), px(676.5))];
    assert_eq!(
        clear_of_lines(top, px(124.), banner, &foot, &[], (px(124.), px(659.))),
        px(124.),
        "the wrap keeps the body's top"
    );
}

#[gpui::test]
fn patch_lines_use_the_code_font(cx: &mut TestAppContext) {
    let font = shaped_code_font(cx, |probe, cx| {
        patch_line(0, cx).child(probe).into_any_element()
    });
    assert_eq!(font.family, "Desktop Mono");
    assert_eq!(font.features.is_calt_enabled(), Some(false));
    assert!(font.features.tag_value_list().contains(&("liga".into(), 0)));
}

/// At every code size Settings offers, the line-number columns take Compare's width for
/// the file's widest number, scaled with the code size, and fit it in the code font: each
/// number ends [`crate::diff_view::CELL_PADDING`] inside its column's right edge, as
/// Compare's gutter paints it, without crossing the column's left edge, and the patch text
/// starts [`source_text_inset`] after both columns.
#[gpui::test]
fn line_numbers_fit_the_widest_number_at_every_code_size(cx: &mut TestAppContext) {
    struct Rows(model::PatchRow);
    impl Render for Rows {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let widest = self.0.old.max(self.0.new).unwrap_or(0);
            let column = px(
                crate::diff_view::line_number_column_width(u64::from(widest))
                    * appearance::code_scale(),
            );
            div()
                .w(px(900.))
                .child(patch_row(0, &self.0, column, patch_row_height(1.), cx))
        }
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::component::Theme::global_mut(cx).mono_font_family = "Desktop Mono".into();
    });
    let row = model::PatchRow {
        old: Some(99_999),
        new: Some(12_345),
        kind: b' ',
        text: "context".into(),
        hunk: 0,
    };
    let (_, cx) = cx.add_window_view(move |_, _| Rows(row));
    let measure = |code: u8, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            appearance::apply_text_sizes(13, code, window, cx);
            window.draw(cx).clear(cx);
        });
        let bounds = |selector, cx: &mut gpui_kit::VisualTestContext| {
            cx.debug_bounds(selector).expect(selector)
        };
        let line = bounds("github-patch-line-0", cx);
        let old = bounds("github-patch-old-0", cx);
        let new = bounds("github-patch-new-0", cx);
        let text = bounds("github-patch-text-0", cx);
        let scale = appearance::code_scale();
        let column = crate::diff_view::line_number_column_width(99_999) * scale;
        let padding = crate::diff_view::CELL_PADDING * scale;
        // The number as the column's text element shapes it.
        let number = cx.update(|window, cx| {
            let family = gpui_kit::component::Theme::global(cx)
                .mono_font_family
                .clone();
            let font = gpui_kit::Font {
                features: appearance::code_font_features_for(&family),
                ..gpui_kit::font(family)
            };
            let run = TextRun {
                len: "99999".len(),
                font,
                color: gpui_kit::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line("99999".into(), appearance::code_text(), &[run], None)
                .width
        });
        let at = |x: f32| line.left() + px(x);
        // Layout puts each edge on a device pixel, half a pixel at 2x.
        let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.51);
        assert!(
            old.size.width >= number,
            "{code} pt: {old:?} clips {number:?}"
        );
        assert!(
            old.left() >= line.left(),
            "{code} pt: {old:?} crosses {line:?}"
        );
        assert!(
            near(old.right(), at(column - padding)),
            "{code} pt: {old:?}"
        );
        assert!(
            new.left() >= at(column),
            "{code} pt: {new:?} crosses the old column"
        );
        assert!(
            near(new.right(), at(column * 2. - padding)),
            "{code} pt: {new:?}"
        );
        assert!(
            near(text.left(), at(column * 2.) + source_text_inset()),
            "{code} pt: {text:?} after {new:?}"
        );
    };
    for code in appearance::CODE_TEXT_RANGE {
        measure(code, cx);
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A text of the panel works its lines out again when the interface's text size changes
/// though its children keep their sizes ([`text_lines`]): a text in a label's size whose
/// one child is held at 200 × 60 px draws three lines 19.5 px apart at 13 pt, and two
/// 27 px apart at 18 pt, the lines a frame working its out afresh reports. Taken while
/// only the text and its children's sizes held, its lines kept their 13 pt height at
/// 18 pt, three of them. The test window's scale factor stays at 2, so the interface's
/// text size, which moves its text scale and the window's rem size, stands for every
/// scale a text's lines follow from ([`TextLines::scale`]).
#[gpui::test]
fn a_text_works_its_lines_out_again_at_another_text_size(cx: &mut TestAppContext) {
    struct Body(Blocks);
    impl Render for Body {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let blocks = self.0.clone();
            let lines = ElementId::from("held-text");
            div()
                .w(px(300.))
                .on_children_prepainted(move |_, _, _| blocks.finish())
                .child(self.0.collect_lines())
                // A label sets its text's size around the element that reports its lines.
                .child(
                    div().text_size(appearance::ui_text(12.)).child(
                        div()
                            .on_children_prepainted(move |children, window, cx| {
                                text_lines(&lines, children, window, cx)
                            })
                            .child(
                                div()
                                    .debug_selector(|| "held-child".into())
                                    .w(px(200.))
                                    .h(px(60.))
                                    .child("Held"),
                            ),
                    ),
                )
        }
    }
    cx.update(gpui_kit::init);
    let blocks = Blocks::default();
    let (_, cx) = cx.add_window_view({
        let blocks = blocks.clone();
        move |_, _| Body(blocks)
    });
    let draw = |interface: u8, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            appearance::apply_text_sizes(interface, 12, window, cx);
            window.draw(cx).clear(cx);
            blocks.lines(cx)
        })
    };
    let child = |cx: &mut gpui_kit::VisualTestContext| {
        cx.debug_bounds("held-child")
            .expect("the text's child")
            .size
    };
    let small = draw(13, cx);
    let held = child(cx);
    let (_, measured) = blocks.text_counts();
    let large = draw(18, cx);
    assert_eq!(child(cx), held, "the child keeps its size at 18 pt");
    assert_eq!(
        blocks.text_counts().1,
        measured + 1,
        "the text works its lines out again at 18 pt"
    );
    blocks.forget_texts();
    assert_eq!(
        draw(18, cx),
        large,
        "the lines it reports at 18 pt are those it works out afresh"
    );
    assert_eq!(
        (small.len(), large.len()),
        (3, 2),
        "three lines at 13 pt, {small:?}, and two at 18 pt, {large:?}"
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

#[gpui::test]
async fn native_review_keys_warm_context_and_destination_flush(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
            .arg(fixture.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap()
            .status
            .success()
    );
    crate::views::tests::fixture_identity(fixture.path());
    let repo = GitRepository::open(fixture.path()).unwrap();
    let panel_repo = repo.clone();
    let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
    let output = captured.clone();
    crate::views::tests::init_app(cx);
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
            app.page = AppPage::Repository;
            app.path = Some(repo.path().to_owned());
            app.repository = Some(repo);
            app
        });
        *output.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    cx.update(|_, cx| {
        gpui_kit::component::Theme::global_mut(cx).font_size = px(18.);
    });
    cx.simulate_resize(size(px(1000.), px(680.)));
    let app = captured.borrow().as_ref().unwrap().clone();
    let panel = cx.update(|window, cx| {
        cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
    });
    app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.executor().run_until_parked();
    let pull = model::fixture_pull(false);
    let captured_pull = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured_pull, 1, &OperationControl::default())
        .unwrap()
        .items;
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/native-review", window, cx)
            });
            panel.pull = Some(pull);
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.review.show_list = false;
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    cx.executor().run_until_parked();
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        assert!(panel.read(cx).review.file_focus.is_focused(window));
    });
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_file(1, window, cx);
            panel.select_review_file(0, window, cx);
        })
    });
    // File preparation runs on the GPUI background executor.
    cx.executor().run_until_parked();
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        assert_eq!(panel.read(cx).review.selected_file, Some(0));
        assert!(!panel.read(cx).review.preparing);
        assert!(!panel.read(cx).review.files[0].rows.is_empty());
        assert!(panel.read(cx).review.files[1].rows.is_empty());
        panel.update(cx, |panel, cx| {
            panel.select_review_line(3, false, window, cx)
        });
    });
    cx.simulate_keystrokes("shift-down enter");
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        let draft = panel.read(cx).review.composing.as_ref().unwrap();
        assert_eq!((draft.start_line, draft.line), (Some(2), 3));
        assert!(
            panel
                .read(cx)
                .review
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        );
        panel.update(cx, |panel, cx| {
            panel.review.input.update(cx, |input, cx| {
                input.set_value(" Exact unfinished text before navigation\\n", window, cx);
                cx.emit(InputEvent::Change);
            });
        });
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let composer = cx
        .debug_bounds("github-inline-composer")
        .expect("rendered inline composer");
    // With the content running past it, the body takes the room the dialog's height cap
    // leaves it in this window: the dialog is as tall as its cap, neither shorter nor
    // taller. Its height rather than its foot, since the dialog may still be sliding in.
    let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
    let viewport = scroll.bounds();
    assert!(
        scroll.max_offset().y > px(0.),
        "the content runs past the body"
    );
    let (dialog, _) = super::super::tests::dialog_frame(cx);
    let cap = cx.update(|window, _| appearance::dialog_max_height(window));
    assert!(
        (dialog.size.height - cap).abs() <= px(1.),
        "the dialog is {:?} tall, not its cap's {cap:?}",
        dialog.size.height
    );
    assert!(
        composer.size.height > px(100.) && composer.size.width > px(300.),
        "composer must retain useful native geometry"
    );
    // Its border and side padding together are the panel's text inset, as the
    // Conversations pane's are ([`super::bordered_pane`]), so its text starts there.
    let position = cx
        .debug_bounds("github-composer-position")
        .expect("the composer's position line");
    assert!(
        (position.left() - composer.left() - super::text_inset()).abs() <= px(0.25),
        "the inline composer's text {position:?} starts the panel's text inset inside \
         its frame {composer:?}"
    );
    assert!(
        composer.top() >= viewport.top() && composer.bottom() <= viewport.bottom(),
        "focused composer {composer:?} must be inside {viewport:?}"
    );
    cx.update(|window, cx| {
        panel
            .read(cx)
            .review
            .body_scroll
            .set_offset(point(px(0.), px(0.)));
        window.refresh();
    });
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    cx.read(|cx| {
        assert_eq!(
            panel.read(cx).review.body_scroll.offset().y,
            px(0.),
            "unchanged focus must not override manual scrolling"
        )
    });
    cx.update(|_, cx| assert!(panel.read(cx).deferred_draft.is_pending()));
    // This is a real InputEvent transition inside the typing quiet period.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/another-destination", window, cx);
                cx.emit(InputEvent::Change);
            })
        })
    });
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.executor().run_until_parked();
    let saved = app
        .read_with(cx, |app, _| app.preferences_writer.submit(drafts::load))
        .await
        .unwrap()
        .unwrap();
    let stored = saved
        .into_iter()
        .find_map(|draft| match draft {
            Draft::Review(review) if review.pull == captured_pull => Some(review),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        stored.composing.as_ref().unwrap().body,
        " Exact unfinished text before navigation\\n"
    );
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.pull = Some(model::fixture_pull(false));
            panel.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/native-review", window, cx)
            });
            panel.restore_review(Some(stored), window, cx);
            panel.review.section = Section::Review;
            panel.close(window, cx);
        })
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
    cx.executor().run_until_parked();
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        assert!(panel.read(cx).review.section == Section::Review);
        assert_eq!(
            panel.read(cx).review.input.read(cx).value(),
            " Exact unfinished text before navigation\\n"
        );
        assert!(
            panel
                .read(cx)
                .review
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        );
        panel.update(cx, |panel, cx| panel.close(window, cx));
    });
    let mut page = Client(UiTransport::Fixture { moved: false })
        .files(&captured_pull, 1, &OperationControl::default())
        .unwrap();
    let replacement = page.items.remove(1);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.closed = false;
            panel.review.section = Section::Files;
            panel.receive_file_page(page);
            panel.select_review_file(0, window, cx);
            panel.receive_file_page(github::Page {
                items: vec![replacement],
                page: 2,
                has_next: false,
            });
        })
    });
    // File preparation runs on the GPUI background executor.
    cx.executor().run_until_parked();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            assert_eq!(panel.review.file_page, 2);
            assert_eq!(panel.review.files[0].filename, "docs/review/界面-guide.md");
            assert!(panel.review.files[0].rows.is_empty());
            assert!(!panel.review.preparing);
            assert!(panel.review.selected_file.is_none());
            panel.close(window, cx);
        })
    });
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.executor().run_until_parked();
}
/// Source draws the supplied patch as plain text and takes the decorations the file's
/// background preparation made, read back through the handle Find masks: they reach the
/// editor whether Source opens before the preparation finishes or after it, equal Compare's
/// for the same text, follow a palette change and count in the retained bytes. A file
/// superseded before its preparation finished decorates nothing, neither its own Source
/// nor the next file's. `source_presentation_stops_at_compares_bounds` covers the bounds.
#[gpui::test]
async fn source_patch_takes_the_decorations_its_file_preparation_made(cx: &mut TestAppContext) {
    use crate::appearance::ThemeChoice;
    cx.executor().allow_parking();
    let fixture = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
            .arg(fixture.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap()
            .status
            .success()
    );
    crate::views::tests::fixture_identity(fixture.path());
    let repo = GitRepository::open(fixture.path()).unwrap();
    let panel_repo = repo.clone();
    let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
    let output = captured.clone();
    crate::views::tests::init_app(cx);
    cx.update(|cx| {
        ThemeChoice::ALL[0].apply(None, cx);
    });
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
            app.page = AppPage::Repository;
            app.path = Some(repo.path().to_owned());
            app.repository = Some(repo);
            app
        });
        *output.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = captured.borrow().as_ref().unwrap().clone();
    let panel = cx.update(|window, cx| {
        cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
    });
    // The panel's local read (remotes and saved drafts) runs on the operations executor;
    // settle it once so it cannot land during the steps below.
    app.read_with(cx, |app, _| app.operations.drain());
    cx.run_until_parked();
    // File preparation runs on the GPUI background executor.
    let settle = |cx: &mut gpui_kit::VisualTestContext| cx.run_until_parked();
    let patches = [
        "@@ -1,2 +1,2 @@\n context\n-old value\n+new value\n".to_owned(),
        "@@ -1 +1,2 @@\n-before\n+after\n+more\n".to_owned(),
    ];
    // Counts match each patch, so a prepared file has inline rows.
    let file = |name: &str, patch: &String, additions| PullFile {
        filename: name.into(),
        previous_filename: None,
        status: "modified".into(),
        additions,
        deletions: 1,
        patch: Some(patch.as_str().into()),
        rows: Vec::new(),
        unavailable: None,
    };
    let files = vec![
        file("src/a.rs", &patches[0], 1),
        file("src/b.rs", &patches[1], 2),
    ];
    // The decorations Compare's unified diff takes for the same text in the applied palette.
    let compare = |patch: &str, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            let presentation = text::PatchPresentation::prepare(patch);
            // The editor stays alive while `drawn` reads the ranges it holds.
            let (_editor, decorations) =
                text::editor_with_decorations(patch, "diff", Some(&presentation), window, cx);
            decorations
                .expect("a prepared patch is decorated")
                .drawn(cx)
        })
    };
    let source = |cx: &mut gpui_kit::VisualTestContext| {
        cx.read(|cx| panel.read(cx).review.source.clone().expect("Source editor"))
    };
    let has_view = |cx: &mut gpui_kit::VisualTestContext| {
        cx.read(|cx| panel.read(cx).review.source_view.is_some())
    };
    let decorations = |editor: &Entity<EditorState>, cx: &mut gpui_kit::VisualTestContext| {
        cx.read(|cx| editor_find::patch_layer(editor, cx).map(|layer| layer.drawn(cx)))
    };

    // Source opens before the file's preparation has finished: plain text at once.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.select_review_file(0, window, cx);
            panel.review_source(1, window, cx);
        })
    });
    let first = source(cx);
    assert_eq!(decorations(&first, cx), None);
    assert!(
        !has_view(cx),
        "Compare's gutter waits for the presentation prepared off the UI thread"
    );
    settle(cx);
    assert!(
        has_view(cx),
        "the prepared presentation gives Source Compare's gutter"
    );
    let expected = compare(&patches[0], cx);
    assert_eq!(
        decorations(&first, cx).as_ref(),
        Some(&expected),
        "the prepared decorations reach the open Source editor"
    );
    cx.read(|cx| assert_eq!(first.read(cx).value(), patches[0].as_str()));

    // The presentation and the decorations kept for the editor count toward the budget.
    cx.update(|_, cx| {
        panel.update(cx, |panel, cx| {
            let with = panel.retained_review_bytes(cx);
            let presentation = panel.review.presentation.take().expect("prepared");
            let kept = panel.review.source_decorations.take().expect("kept");
            let counted = presentation.retained_bytes() + kept.retained_bytes();
            assert_eq!(with - panel.retained_review_bytes(cx), counted);
            assert!(counted > 0);
            panel.review.presentation = Some(presentation);
            panel.review.source_decorations = Some(kept);
        })
    });

    // A palette change redecorates it as Compare would be.
    cx.update(|_, cx| ThemeChoice::ALL[1].apply(None, cx));
    cx.run_until_parked();
    let recolored = compare(&patches[0], cx);
    assert_ne!(recolored, expected);
    assert_eq!(decorations(&first, cx), Some(recolored.clone()));

    // A file superseded before its preparation finished decorates nothing: its Source is
    // plain text while it is selected, and its preparation's result never lands, neither
    // as its rows nor on the next file's Source, opened before either finished.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_file(1, window, cx);
            panel.review_source(1, window, cx);
        })
    });
    let superseded = source(cx);
    assert_eq!(decorations(&superseded, cx), None);
    assert!(!has_view(cx));
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_file(0, window, cx);
            panel.review_source(1, window, cx);
        })
    });
    let current = source(cx);
    settle(cx);
    assert_ne!(current, superseded);
    cx.read(|cx| assert!(panel.read(cx).review.files[1].rows.is_empty()));
    assert_eq!(decorations(&current, cx), Some(recolored));
    assert!(has_view(cx));

    // Source opened after its file's preparation finished takes the decorations at once.
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.select_review_file(1, window, cx)));
    settle(cx);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.review_source(1, window, cx)));
    let later = source(cx);
    cx.read(|cx| assert!(!panel.read(cx).review.files[1].rows.is_empty()));
    let expected = compare(&patches[1], cx);
    assert_eq!(decorations(&later, cx), Some(expected));
    assert!(has_view(cx));
}
/// Source decorations are prepared up to Compare's bounds and not one byte or one line
/// beyond. Lines are `\n` bytes, as git-core counts them for Compare, so a patch's last
/// line without one does not count.
#[test]
fn source_presentation_stops_at_compares_bounds() {
    use gitturtle_core::{MAX_DIFF_BYTES, MAX_DIFF_LINES};
    // Bytes: one hunk whose context line fills the patch, with no trailing newline.
    let header = "@@ -1 +1 @@\n ";
    let at_bytes = format!("{header}{}", "x".repeat(MAX_DIFF_BYTES - header.len()));
    assert_eq!(at_bytes.len(), MAX_DIFF_BYTES);
    assert!(source_presentation(&at_bytes).is_some());
    assert!(source_presentation(&format!("{at_bytes}x")).is_none());
    // Lines: MAX_DIFF_LINES newlines, the last context line unterminated.
    let at_lines = format!(
        "@@ -1,{0} +1,{0} @@\n{1} ",
        MAX_DIFF_LINES,
        " \n".repeat(MAX_DIFF_LINES - 1)
    );
    let newlines = |patch: &str| patch.bytes().filter(|byte| *byte == b'\n').count();
    assert_eq!(newlines(&at_lines), MAX_DIFF_LINES);
    assert!(at_lines.len() < MAX_DIFF_BYTES);
    assert!(source_presentation(&at_lines).is_some());
    let over_lines = format!("{at_lines}\n ");
    assert_eq!(newlines(&over_lines), MAX_DIFF_LINES + 1);
    assert!(source_presentation(&over_lines).is_none());
}

/// The PR review's Source draws the fixture's `src/review/session.rs` patch as Compare draws
/// its unified patch: the same view around the editor, so the same old and new line-number
/// columns, gutter tint and decorations, and no line numbers or folds of the editor's own.
/// Its preparation stays off the UI thread and is cancelled on a file change and on close.
#[gpui::test]
async fn source_patch_draws_compares_gutter(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
            .arg(fixture.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap()
            .status
            .success()
    );
    crate::views::tests::fixture_identity(fixture.path());
    let repo = GitRepository::open(fixture.path()).unwrap();
    let panel_repo = repo.clone();
    let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
    let output = captured.clone();
    crate::views::tests::init_app(cx);
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
            app.page = AppPage::Repository;
            app.path = Some(repo.path().to_owned());
            app.repository = Some(repo);
            app
        });
        *output.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    cx.simulate_resize(size(px(1000.), px(680.)));
    let app = captured.borrow().as_ref().unwrap().clone();
    let panel = cx.update(|window, cx| {
        cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
    });
    app.read_with(cx, |app, _| app.operations.drain());
    cx.run_until_parked();
    let pull = model::fixture_pull(false);
    let captured_pull = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured_pull, 1, &OperationControl::default())
        .unwrap()
        .items;
    let index = files
        .iter()
        .position(|file| file.filename == "src/review/session.rs")
        .expect("fixture file");
    let other = (index + 1) % files.len();
    let patch = files[index].patch.clone().expect("fixture patch");
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.pull = Some(pull);
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.review.show_list = false;
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_file(index, window, cx);
            panel.review_source(1, window, cx);
        })
    });
    cx.run_until_parked();
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    // Compare's unified patch for the same text, built as Compare builds it, drawn where
    // Source draws its text: inside the frame's one-pixel border, above the inset that
    // keeps its lines whole, as tall as the editor, which runs past the clip at its last
    // whole line by its bottom padding.
    let clip = cx
        .debug_bounds("github-source-text")
        .expect("rendered Source text");
    let inner = Bounds::new(
        clip.origin,
        size(clip.size.width, clip.size.height + EDITOR_PADDING),
    );
    let presentation = text::PatchPresentation::prepare(&patch);
    let (compare, compare_decorations, compare_view) = cx.update(|window, cx| {
        let (editor, decorations) =
            text::editor_with_decorations(&patch, "diff", Some(&presentation), window, cx);
        let view = crate::diff_view::new(editor.clone(), &presentation, window, cx);
        (
            editor,
            decorations.expect("a prepared patch is decorated"),
            view,
        )
    });
    for _ in 0..3 {
        let view = compare_view.clone();
        cx.draw(inner.origin, inner.size, move |_, _| {
            div().w(inner.size.width).h(inner.size.height).child(view)
        });
        cx.run_until_parked();
    }
    let source = cx.read(|cx| panel.read(cx).review.source.clone().expect("Source editor"));
    // Gutter width (both number columns), where the editor's text starts within it (no
    // line numbers or fold markers of its own), its size and its line height.
    let geometry = |editor: &Entity<EditorState>, cx: &mut gpui_kit::VisualTestContext| {
        cx.read(|cx| {
            let state = editor.read(cx);
            let input = state.input_bounds();
            let text = state.text_bounds().expect("laid out");
            (
                input.origin.x - inner.origin.x,
                text.origin.x - input.origin.x,
                input.size,
                state.line_height().expect("laid out"),
            )
        })
    };
    let drawn = geometry(&source, cx);
    assert_eq!(
        drawn,
        geometry(&compare, cx),
        "Source draws Compare's gutter"
    );
    let columns = px(2. * presentation.column_width * crate::appearance::code_scale());
    // The editor's own inset follows the gutter, so the text starts past both columns.
    assert!(drawn.0 >= columns, "old and new number columns: {drawn:?}");
    cx.read(|cx| {
        assert_eq!(
            editor_find::patch_layer(&source, cx).map(|layer| layer.drawn(cx)),
            Some(compare_decorations.drawn(cx)),
            "Source tints its lines as Compare does"
        );
        let review = &panel.read(cx).review;
        let prepared = review.presentation.as_ref().expect("prepared");
        assert_eq!(
            prepared.rows, presentation.rows,
            "the gutter's numbers and tints"
        );
        assert_eq!(prepared.change_rows, presentation.change_rows);
        assert_eq!(prepared.column_width, presentation.column_width);
    });

    // A file change while Source waits for its presentation cancels it, and so does close:
    // nothing lands, so neither file's Source gains a gutter afterwards.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_file(other, window, cx);
            panel.review_source(1, window, cx);
            assert!(panel.review.preparing);
            assert!(panel.review.source_view.is_none());
            panel.select_review_file(index, window, cx);
            panel.review_source(1, window, cx);
            assert!(panel.review.source_view.is_none());
            panel.close(window, cx);
            assert!(!panel.review.preparing);
        })
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let review = &panel.read(cx).review;
        assert!(review.presentation.is_none());
        assert!(review.source_view.is_none());
        assert!(review.files.iter().all(|file| file.rows.is_empty()));
    });
}
#[gpui::test]
async fn warm_reopen_retries_interrupted_local_read_and_template_payload_evicts(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let fixture = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
            .arg(fixture.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(fixture.path())
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/gitturtle-fixture/held-read.git"
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    crate::views::tests::fixture_identity(fixture.path());
    let repo = GitRepository::open(fixture.path()).unwrap();
    let panel_repo = repo.clone();
    let observed: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
    let output = observed.clone();
    crate::views::tests::init_app(cx);
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
            app.page = AppPage::Repository;
            app.repository = Some(repo);
            app
        });
        *output.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = observed.borrow().as_ref().unwrap().clone();
    let stored = Draft::Pull(NewPull {
        repository: Repository::parse("gitturtle-fixture/held-read").unwrap(),
        title: "Recover this title".into(),
        body: "Exact stored recovery".into(),
        head: "feature".into(),
        base: "main".into(),
        draft: true,
        expected_head: None,
        expected_base: None,
    });
    let key = stored.key();
    app.read_with(cx, |app, _| {
        app.preferences_writer.submit(move || drafts::save(stored))
    })
    .await
    .unwrap()
    .unwrap();
    let (release, gate) = std::sync::mpsc::channel();
    let (started, running) = std::sync::mpsc::channel();
    let held = app.read_with(cx, |app, _| {
        app.operations.submit(move || {
            started.send(())?;
            gate.recv()?;
            Ok(())
        })
    });
    running.recv_timeout(Duration::from_secs(5)).unwrap();
    let panel = cx.update(|window, cx| {
        cx.new(|cx| {
            Panel::new(
                app.downgrade(),
                panel_repo.clone(),
                "main".into(),
                window,
                cx,
            )
        })
    });
    cx.executor().run_until_parked();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            assert!(panel.local_loading);
            assert!(!panel.local_loaded);
            panel.close(window, cx);
        })
    });
    release.send(()).unwrap();
    held.await.unwrap().unwrap();
    app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.executor().run_until_parked();
    cx.read(|cx| assert!(!panel.read(cx).local_loaded));
    let (release, gate) = std::sync::mpsc::channel();
    let (started, running) = std::sync::mpsc::channel();
    let held = app.read_with(cx, |app, _| {
        app.operations.submit(move || {
            started.send(())?;
            gate.recv()?;
            Ok(())
        })
    });
    running.recv_timeout(Duration::from_secs(5)).unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
    cx.executor().run_until_parked();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            assert!(panel.local_loading);
            panel.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/user-edited", window, cx)
            });
            panel.title.update(cx, |input, cx| {
                input.set_value("Unsaved edited title", window, cx)
            });
        })
    });
    release.send(()).unwrap();
    held.await.unwrap().unwrap();
    app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.executor().run_until_parked();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            assert!(panel.local_loaded);
            assert!(panel.saved.iter().any(|draft| draft.key() == key));
            assert_eq!(
                panel.destination.read(cx).value(),
                "gitturtle-fixture/user-edited"
            );
            assert_eq!(panel.title.read(cx).value(), "Unsaved edited title");
            panel.close(window, cx);
        })
    });
    // Each actual panel owns the maximum supported template payload. The
    // count limit alone would retain four; the payload limit must evict.
    cx.update(|window, cx| {
        let mut warm = WarmPanels::default();
        for index in 0..4 {
            let form = cx.new(|cx| {
                let mut panel = Panel::new(
                    app.downgrade(),
                    panel_repo.clone(),
                    "main".into(),
                    window,
                    cx,
                );
                panel.closed = true;
                panel.templates = (0..32)
                    .map(|n| github::Template {
                        path: format!(".github/PULL_REQUEST_TEMPLATE/{n}.md"),
                        revision: "1".repeat(40),
                        body: "x".repeat(github::MAX_TEXT),
                    })
                    .collect();
                panel.attempts.push(drafts::Attempt {
                    completed_reply_sha256: None,
                    destination: "fixture attempt".into(),
                    outcome: "Observed result".repeat(200),
                });
                panel
            });
            let bytes = form.read(cx).retained_review_bytes(cx);
            assert!(bytes > 8 * 1024 * 1024);
            warm.retain(WarmPanel {
                owner: app.entity_id(),
                worktree: std::path::PathBuf::from(format!("logical-fixture-{index}")),
                panel: form,
                bytes,
                unsaved: false,
            });
        }
        assert_eq!(warm.0.len(), 3);
        assert_eq!(
            warm.0[0].worktree,
            std::path::PathBuf::from("logical-fixture-1")
        );
        assert!(warm.0.iter().map(|entry| entry.bytes).sum::<usize>() <= 32 * 1024 * 1024);
    });
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.executor().run_until_parked();
}
/// In a 1480 × 980 window at 13 and 18 pt, Source fills its column down to the changed-file
/// list's bottom edge, as plain text while its file prepares and decorated once it has, and
/// shows whole lines: its text is a whole number of lines tall, and what a line does not
/// fill, less than a line, lies below the text inside the box. It counted its lines over the
/// box less the editor's padding above and below, though the bottom padding lies past its
/// clip, so 39 px at 2x lay below 13 lines of 36 px, room for a 14th.
#[gpui::test]
async fn source_reaches_the_file_lists_bottom_in_whole_lines(cx: &mut TestAppContext) {
    let (_fixture, app, panel, cx) = review_panel(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
    cx.simulate_resize(size(px(1480.), px(980.)));
    let pull = model::fixture_pull(false);
    let captured_pull = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured_pull, 1, &OperationControl::default())
        .unwrap()
        .items;
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.pull = Some(pull);
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.review.show_list = false;
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    settle_open(&app, cx);
    let canvas = midnight_canvas(cx);
    // What Source lays its lines out from: the height its box measured in the last frame
    // and the editor's line height.
    let measured = |cx: &App| {
        let review = &panel.read(cx).review;
        let line = review
            .source
            .as_ref()
            .and_then(|editor| editor.read(cx).line_height());
        (review.source_height.get(), line)
    };
    for (interface, code) in [(13, 12), (18, 15)] {
        cx.update(|window, cx| appearance::apply_text_sizes(interface, code, window, cx));
        for file in [0, 1] {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.select_review_file(file, window, cx);
                    panel.review_source(1, window, cx);
                })
            });
            for decorated in [false, true] {
                // Undecorated, the frames run nothing the file's preparation needs.
                if decorated {
                    cx.run_until_parked();
                }
                draw_settled(cx, measured);
                let at = format!("{interface} pt, file {file}, decorated {decorated}");
                assert_eq!(
                    cx.read(|cx| panel.read(cx).review.source_view.is_some()),
                    decorated,
                    "{at}"
                );
                let source = cx.debug_bounds("github-source-patch").expect("Source");
                let list = cx.debug_bounds("github-file-focus").expect("file list");
                assert!(
                    (source.bottom() - list.bottom()).abs() < px(0.01),
                    "{at}: Source {source:?} ends with the file list {list:?}"
                );
                let editor = cx.read(|cx| panel.read(cx).review.source.clone().unwrap());
                let (text, line) = cx.read(|cx| {
                    let editor = editor.read(cx);
                    (
                        editor.input_bounds(),
                        editor.line_height().expect("laid out"),
                    )
                });
                let lines = text.size.height / line;
                assert!(
                    lines >= 5. && (lines - lines.round()).abs() < 0.01,
                    "{at}: {text:?} is {lines} lines of {line:?}"
                );
                // Inside the 1 px border: the editor's bottom padding lies past the clip.
                let below = source.bottom() - px(1.) - text.bottom();
                assert!(
                    below > px(-0.01) && below < line,
                    "{at}: {below:?} below the text {text:?} in {source:?}"
                );
                // Nothing the box paints reaches below its last whole line, where the
                // top of a 14th line once showed, and the strip below takes the canvas
                // the text area paints.
                let clip = cx.debug_bounds("github-source-text").expect("Source text");
                assert!(
                    (clip.bottom() - text.bottom()).abs() < px(0.01),
                    "{at}: {clip:?} ends at the last whole line {text:?}"
                );
                let (fills, lowest) = painted_in(cx, source);
                assert_eq!(fills, vec![canvas], "{at}: the box's fill");
                assert!(
                    lowest <= clip.bottom() + px(0.01),
                    "{at}: paints down to {lowest:?}, below {clip:?}"
                );
            }
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.run_until_parked();
}
/// After the panel opens: its read of local settings and drafts, on the operations thread
/// `run_until_parked` cannot wait for, has landed, so the panel's content is final.
fn settle_open(app: &Entity<GitTurtle>, cx: &mut gpui_kit::VisualTestContext) {
    app.read_with(cx, |app, _| app.operations.drain());
    cx.run_until_parked();
}

/// Draws until a frame leaves `measured` as it found it. A review box lays its rows or
/// lines out from what its content measured in the last frame, and a frame whose layout
/// differs corrects the measure and draws again; the frame that corrects nothing showed
/// the box laid out from its own height. The dialog opens at rest (`review_panel`), so
/// nothing else moves the box between frames.
fn draw_settled<T: PartialEq + std::fmt::Debug>(
    cx: &mut gpui_kit::VisualTestContext,
    measured: impl Fn(&App) -> T,
) {
    let mut before = cx.read(&measured);
    for _ in 0..8 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let after = cx.read(&measured);
        if after == before {
            return;
        }
        before = after;
    }
    panic!("the review box is still measuring {before:?}");
}

/// Applies Midnight, whose kit input background (#181f2c over Midnight's surfaces) is
/// lighter than the canvas its editors' text areas paint (#10151f), and returns that canvas.
fn midnight_canvas(cx: &mut gpui_kit::VisualTestContext) -> gpui_kit::Background {
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    let (canvas, input) = cx.update(|_, cx| {
        (
            gpui_kit::Hsla::from(rgb(palette(cx).canvas)),
            gpui_kit::component::Theme::global(cx).input_background(),
        )
    });
    assert_ne!(
        input, canvas,
        "the strip's old fill differs from the canvas"
    );
    canvas.into()
}

/// What the last frame painted in `frame`, a box that clips its content: the fills of the
/// quads at its bounds, its own, and the lowest bottom, as clipped, of every other quad it
/// clips. A quad the box does not clip is not the box's, though it may lie inside it:
/// History's empty state draws its Open Changes button beneath the dialog, inside the
/// Changes and Source boxes at 13 pt once the dialog is at rest, its bottom 13.5 px below
/// Changes' last whole row and 1.5 px below Source's last whole line. Counted, it failed
/// both tests whenever the dialog's entrance had nearly finished before they read the
/// frame, which a loaded machine made likely; while the dialog was still sliding in, the
/// boxes lay above the button.
fn painted_in(
    cx: &mut gpui_kit::VisualTestContext,
    frame: Bounds<Pixels>,
) -> (Vec<gpui_kit::Background>, Pixels) {
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |bounds: Bounds<gpui_kit::ScaledPixels>| {
            let value = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
            Bounds::from_corners(
                point(value(bounds.left()), value(bounds.top())),
                point(value(bounds.right()), value(bounds.bottom())),
            )
        };
        let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.5);
        let (mut fills, mut lowest) = (Vec::new(), frame.top());
        for quad in window.painted_quads() {
            let drawn = logical(quad.bounds);
            if near(drawn.left(), frame.left())
                && near(drawn.top(), frame.top())
                && near(drawn.right(), frame.right())
                && near(drawn.bottom(), frame.bottom())
            {
                if !quad.background.is_transparent() {
                    fills.push(quad.background);
                }
                continue;
            }
            let clip = logical(quad.content_mask.bounds);
            let shown = drawn.intersect(&clip);
            if shown.size.height > px(0.)
                && shown.size.width > px(0.)
                && clip.left() >= frame.left() - px(0.5)
                && clip.right() <= frame.right() + px(0.5)
                && clip.top() >= frame.top() - px(0.5)
                && clip.bottom() <= frame.bottom() + px(0.5)
                && !quad.background.is_transparent()
            {
                lowest = lowest.max(shown.bottom());
            }
        }
        (fills, lowest)
    })
}

/// In a 1480 × 980 window at 13 and 18 pt, Changes' box ends at the changed-file list's
/// bottom edge, as Source's does, with its range hint below it, on the line of the file
/// list's page buttons. Its rows fill the box in whole rows, counted at the height they are
/// drawn: what a row does not fill, less than a row, lies below them inside the box, no row
/// runs past the rows' clip, and the box paints nothing below its last whole row and fills
/// that strip with the canvas a context row paints.
#[gpui::test]
async fn changes_reach_the_file_lists_bottom_in_whole_rows(cx: &mut TestAppContext) {
    let (_fixture, app, panel, cx) = review_panel(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
    cx.simulate_resize(size(px(1480.), px(980.)));
    let pull = model::fixture_pull(false);
    let captured_pull = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured_pull, 1, &OperationControl::default())
        .unwrap()
        .items;
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.pull = Some(pull);
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.review.show_list = false;
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    settle_open(&app, cx);
    let canvas = midnight_canvas(cx);
    // What Changes counts its rows over: the height its box measured in the last frame.
    let measured = |cx: &App| panel.read(cx).review.patch_height.get();
    for (interface, code) in [(13, 12), (18, 15)] {
        cx.update(|window, cx| appearance::apply_text_sizes(interface, code, window, cx));
        for file in [0, 1] {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.select_review_file(file, window, cx);
                    panel.review_source(0, window, cx);
                })
            });
            cx.run_until_parked();
            draw_settled(cx, measured);
            let at = format!("{interface} pt, file {file}");
            let list = cx.debug_bounds("github-file-focus").expect("file list");
            let patch = cx.debug_bounds("github-patch").expect("Changes");
            assert!(
                (patch.bottom() - list.bottom()).abs() < px(0.01),
                "{at}: Changes {patch:?} ends with the file list {list:?}"
            );
            let hint = cx.debug_bounds("github-range-hint").expect("range hint");
            assert!(
                hint.top() >= patch.bottom(),
                "{at}: the hint {hint:?} lies below the box {patch:?}"
            );
            // On the pager's line: the hint's first line, one of two at 18 pt, centres where
            // Previous centres its label, within the device pixel layout snaps it to. At the
            // top of its row it stood 4 px above.
            let previous = cx.debug_bounds("github-files-previous").expect("Previous");
            let line = appearance::ui_size(HINT_LINE);
            let lines = (hint.size.height / line).round();
            let first = hint.top() + hint.size.height / lines / 2.;
            assert!(
                lines >= 1. && (first - previous.center().y).abs() < px(0.5),
                "{at}: the hint {hint:?} centres its first line off Previous {previous:?}"
            );
            let rows = cx.debug_bounds("github-patch-rows").expect("rows");
            // The rows as drawn, each on whole device pixels (18 px at 12 pt): when rows
            // were 19.8 px, drawn 20 px tall at 2x, counting them at 19.8 px cut the 13th
            // row of the first file 2.6 px short.
            let row = cx
                .debug_bounds("github-patch-line-0")
                .expect("row")
                .size
                .height;
            let whole = rows.size.height / row;
            assert!(
                whole >= 5. && (rows.size.height - row * whole.round()).abs() < px(0.01),
                "{at}: {rows:?} is {whole} rows of {row:?}"
            );
            let drawn = (0..)
                .map_while(|index| cx.debug_bounds(format!("github-patch-line-{index}").leak()))
                .collect::<Vec<_>>();
            assert!(
                drawn.iter().all(|line| line.size.height == row),
                "{at}: {drawn:?}"
            );
            let last = drawn.last().expect("rows drawn");
            assert!(
                last.bottom() <= rows.bottom() + px(0.01),
                "{at}: the last row drawn {last:?} runs past the rows' clip {rows:?}"
            );
            // Inside the 1 px border.
            let below = patch.bottom() - px(1.) - rows.bottom();
            assert!(
                below > px(-0.01) && below < row,
                "{at}: {below:?} below the rows {rows:?} in {patch:?}"
            );
            let (fills, lowest) = painted_in(cx, patch);
            assert_eq!(fills, vec![canvas], "{at}: the box's fill");
            assert!(
                lowest <= rows.bottom() + px(0.01),
                "{at}: paints down to {lowest:?}, below {rows:?}"
            );
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.run_until_parked();
}

/// In a 1480 × 980 window, Changes' range hint takes one line at 13 pt and wraps at
/// 18 pt, breaking only between its phrases. It broke inside one: here inside the second,
/// natively inside the last, leaving "comments" alone on a line.
#[gpui::test]
async fn the_range_hint_wraps_between_its_phrases(cx: &mut TestAppContext) {
    use gpui_kit::Styled as _;
    let (_fixture, app, panel, cx) =
        review_panel_styled(cx, |root| root.text_bg(crate::width_walk::text_mark()));
    cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
    cx.simulate_resize(size(px(1480.), px(980.)));
    let pull = model::fixture_pull(false);
    let captured_pull = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured_pull, 1, &OperationControl::default())
        .unwrap()
        .items;
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.pull = Some(pull);
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.review.show_list = false;
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    settle_open(&app, cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_file(0, window, cx);
            panel.review_source(0, window, cx);
        })
    });
    cx.run_until_parked();
    let measured = |cx: &App| panel.read(cx).review.patch_height.get();
    for (interface, wraps) in [(13, false), (18, true)] {
        cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
        draw_settled(cx, measured);
        let at = format!("{interface} pt");
        // The panel's body scrolls the whole hint into view; at 18 pt its last line lies
        // below the body's fold, above the dialog's footer.
        let (hint, body) = (
            cx.debug_bounds("github-range-hint").expect("range hint"),
            cx.debug_bounds("github-panel").expect("panel's body"),
        );
        // Inside the room the body keeps for focus rings.
        let below = hint.bottom() - (body.bottom() - px(4.));
        if below > px(0.) {
            cx.read(|cx| {
                let scroll = &panel.read(cx).review.body_scroll;
                let offset = scroll.offset();
                scroll.set_offset(point(offset.x, offset.y - below));
            });
            draw_settled(cx, measured);
        }
        let lines = super::super::tests::assert_phrases_whole(
            cx,
            "github-range-hint",
            &range_hint(),
            appearance::ui_text(12.),
            &at,
        );
        assert_eq!(lines > 1, wraps, "{at}: the hint draws {lines} lines");
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.run_until_parked();
}

/// In a 1480 × 980 window at 13 and 18 pt, the review box's bottom edge stays where it is
/// between Changes and Source, level with the changed-file list's bottom: Changes' range
/// hint took the box's last 26 px only in Changes, so switching modes moved the edge. The
/// hint lies below the box in Changes and is not drawn in Source.
#[gpui::test]
async fn switching_modes_keeps_the_review_box_edge(cx: &mut TestAppContext) {
    let (_fixture, app, panel, cx) = review_panel(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
    cx.simulate_resize(size(px(1480.), px(980.)));
    let pull = model::fixture_pull(false);
    let captured_pull = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured_pull, 1, &OperationControl::default())
        .unwrap()
        .items;
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.pull = Some(pull);
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.review.show_list = false;
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    cx.run_until_parked();
    for (interface, code) in [(13, 12), (18, 15)] {
        cx.update(|window, cx| appearance::apply_text_sizes(interface, code, window, cx));
        for file in [0, 1] {
            let mut edges = Vec::new();
            for (mode, selector) in [(0, "github-patch"), (1, "github-source-patch")] {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.select_review_file(file, window, cx);
                        panel.review_source(mode, window, cx);
                    })
                });
                cx.run_until_parked();
                for _ in 0..3 {
                    cx.update(|window, cx| window.draw(cx).clear(cx));
                }
                let at = format!("{interface} pt, file {file}, {selector}");
                let shown = cx.debug_bounds(selector).expect(selector);
                let list = cx.debug_bounds("github-file-focus").expect("file list");
                let hint = cx.debug_bounds("github-range-hint");
                match mode {
                    0 => assert!(
                        hint.is_some_and(|hint| hint.top() >= shown.bottom()),
                        "{at}: the hint {hint:?} lies below the box {shown:?}"
                    ),
                    _ => assert_eq!(hint, None, "{at}"),
                }
                // From the list's top: the panel's body may scroll between the modes.
                edges.push((shown.bottom() - list.top(), list.size.height));
            }
            assert!(
                edges
                    .iter()
                    .all(|&(edge, list)| (edge - list).abs() < px(0.01)),
                "{interface} pt, file {file}: Changes and Source end {edges:?} below the \
                 list's top (box, list)"
            );
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.run_until_parked();
}

/// In a 1480 × 980 window at 13 and 18 pt, Changes and decorated Source start their text at
/// one place in the box they share: the first row and the first line one inset below the
/// box's top, the editor's top padding, the old and new line numbers right-aligned where
/// Compare's gutter paints Source's, and the code text where the editor draws its first
/// character; and each Changes row is as tall as one of Source's lines, where it was 20 px
/// beside Source's 18 px at 12 pt. Changes started flush with the box, its numbers left-aligned in columns that
/// kept 36 px, so at 15 pt code its text began 12.5 px before Source's; then, measured
/// against the editor's text area rather than its drawn text, 10 px before it.
#[gpui::test]
async fn changes_and_source_start_their_text_at_one_place(cx: &mut TestAppContext) {
    let (_fixture, app, panel, cx) = review_panel(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
    cx.simulate_resize(size(px(1480.), px(980.)));
    let pull = model::fixture_pull(false);
    let captured_pull = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured_pull, 1, &OperationControl::default())
        .unwrap()
        .items;
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.pull = Some(pull);
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.review.show_list = false;
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    cx.run_until_parked();
    let show = |file: usize, mode: usize, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.select_review_file(file, window, cx);
                panel.review_source(mode, window, cx);
            })
        });
        cx.run_until_parked();
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    };
    for (interface, code) in [(13, 12), (18, 15)] {
        cx.update(|window, cx| appearance::apply_text_sizes(interface, code, window, cx));
        for file in [0, 1] {
            let at = format!("{interface} pt, file {file}");
            show(file, 0, cx);
            // The first row with both line numbers: the hunk's header has none.
            let numbered = cx.read(|cx| {
                panel.read(cx).review.files[file]
                    .rows
                    .iter()
                    .position(|row| row.old.is_some() && row.new.is_some())
                    .expect("a context row")
            });
            let bounds = |selector: String, cx: &mut gpui_kit::VisualTestContext| {
                cx.debug_bounds(selector.clone().leak())
                    .unwrap_or_else(|| panic!("{selector}"))
            };
            let patch = bounds("github-patch".into(), cx);
            let first = bounds("github-patch-line-0".into(), cx);
            let text = bounds(format!("github-patch-text-{numbered}"), cx);
            let old = bounds(format!("github-patch-old-{numbered}"), cx);
            let new = bounds(format!("github-patch-new-{numbered}"), cx);
            let changes = (
                first.top() - patch.top(),
                text.left() - patch.left(),
                old.right() - patch.left(),
                new.right() - patch.left(),
            );

            show(file, 1, cx);
            assert!(
                cx.read(|cx| panel.read(cx).review.source_view.is_some()),
                "{at}: Source draws Compare's gutter"
            );
            let source = cx.debug_bounds("github-source-patch").expect("Source");
            let editor = cx.read(|cx| panel.read(cx).review.source.clone().unwrap());
            // Where the editor draws the first line's first character: past the margin
            // its code editor keeps inside the text area, not at the text area's edge.
            let (input, drawn, line) = cx.read(|cx| {
                let editor = editor.read(cx);
                (
                    editor.input_bounds(),
                    editor.range_to_bounds(&(0..0)).expect("laid out"),
                    editor.line_height().expect("laid out"),
                )
            });
            // One row height: a Changes row is one of Source's lines, within the device
            // pixel layout puts a row's edge on.
            assert!(
                (first.size.height - line).abs() < px(0.51),
                "{at}: a Changes row {first:?} beside Source's {line:?} line"
            );
            // Compare's gutter paints each number this far inside its column's right edge,
            // its columns starting inside the box's 1 px border.
            let scale = appearance::code_scale();
            let column = cx.read(|cx| {
                panel
                    .read(cx)
                    .review
                    .presentation
                    .as_ref()
                    .unwrap()
                    .column_width
            }) * scale;
            let padding = crate::diff_view::CELL_PADDING * scale;
            let painted = |columns: f32| px(1. + column * columns - padding);
            let shown = (
                input.top() - source.top(),
                drawn.left() - source.left(),
                painted(1.),
                painted(2.),
            );
            assert_eq!(
                changes.0,
                px(1.) + EDITOR_PADDING,
                "{at}: the first row's inset"
            );
            assert!(
                (changes.0 - shown.0).abs() < px(0.01) && (changes.1 - shown.1).abs() < px(0.01),
                "{at}: Changes starts its text at {changes:?}, Source at {shown:?} \
                 (top, text x, old and new numbers' right edges)"
            );
            // Within the device pixel layout puts each edge on.
            assert!(
                (changes.2 - shown.2).abs() < px(0.51) && (changes.3 - shown.3).abs() < px(0.51),
                "{at}: Changes' numbers end at {changes:?}, Source's at {shown:?} \
                 (top, text x, old and new numbers' right edges)"
            );
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.run_until_parked();
}

/// A repository page and a review panel for it, built as the views above build them, with
/// the panel's local read settled. Dialogs open at rest, as with reduced motion: the
/// kit's entrance slides a dialog down over 250 ms of real time, so where a frame placed
/// the panel depended on how long the machine took to draw it.
fn review_panel(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    Entity<Panel>,
    &mut gpui_kit::VisualTestContext,
) {
    review_panel_styled(cx, |root| root)
}

/// [`review_panel`] with the window's root styled by `style`.
fn review_panel_styled(
    cx: &mut TestAppContext,
    style: impl FnOnce(Root) -> Root + 'static,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    Entity<Panel>,
    &mut gpui_kit::VisualTestContext,
) {
    cx.executor().allow_parking();
    let fixture = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
            .arg(fixture.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap()
            .status
            .success()
    );
    crate::views::tests::fixture_identity(fixture.path());
    let repo = GitRepository::open(fixture.path()).unwrap();
    let panel_repo = repo.clone();
    let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
    let output = captured.clone();
    crate::views::tests::init_app(cx);
    cx.update(|cx| {
        cx.set_reduce_motion(true);
    });
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
            app.page = AppPage::Repository;
            app.path = Some(repo.path().to_owned());
            app.repository = Some(repo);
            app
        });
        *output.borrow_mut() = Some(app.clone());
        style(Root::new(app, window, cx))
    });
    let app = captured.borrow().as_ref().unwrap().clone();
    let panel = cx.update(|window, cx| {
        cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
    });
    app.read_with(cx, |app, _| app.operations.drain());
    cx.run_until_parked();
    (fixture, app, panel, cx)
}
/// Return on a review line answers once per press, under X11 too, through each of a held
/// key's repeats (`REPEATS`): a press that began on the changed-file list, which gives the
/// patch focus, begins no comment there; with no line chosen a press is refused once, and
/// after the key-up a new press is refused again; on a chosen line a press opens its comment
/// with focus in the comment's text, which the repeats leave empty. Begun on every Enter
/// key-down, the comment opened from the list's press, a refusal was said on each repeat
/// and each repeat typed a new line into the comment just opened.
#[gpui::test]
async fn a_held_return_on_a_review_line_answers_once_per_press(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::{REPEATS, enter};
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    let draw = line_draw;
    let seen = |cx: &mut gpui_kit::VisualTestContext| line_seen(&panel, cx);
    let hold = |cx: &mut gpui_kit::VisualTestContext, at: &str| {
        let held = seen(cx);
        for (repeat, events) in REPEATS {
            for down in events {
                enter(cx, *down);
            }
            draw(cx);
            assert_eq!(seen(cx), held, "{at}, {repeat}: nothing more is answered");
        }
        enter(cx, None);
        draw(cx);
    };

    // Begun on the changed-file list, with a line chosen.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_line(3, false, window, cx);
            panel.review.file_focus.focus(window, cx);
        })
    });
    draw(cx);
    let before = seen(cx);
    enter(cx, Some(false));
    draw(cx);
    assert!(
        cx.update(|window, cx| panel.read(cx).review.line_focus.is_focused(window)),
        "the list's Return gives the patch focus"
    );
    assert_eq!(seen(cx), before, "the list's Return begins no comment");
    hold(cx, "begun on the list");

    // No line chosen: refused once per press.
    for press in ["the first press", "a new press after the key-up"] {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.anchor = None;
                panel.review.selected_row = None;
                panel.review.line_focus.focus(window, cx);
            })
        });
        draw(cx);
        let refusals = seen(cx).0;
        enter(cx, Some(false));
        draw(cx);
        assert_eq!(seen(cx).0, refusals + 1, "no line, {press}: refused once");
        hold(cx, &format!("no line, {press}"));
    }

    // A chosen line: one comment, its text untouched by the repeats.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_line(3, false, window, cx);
            panel.review.line_focus.focus(window, cx);
        })
    });
    draw(cx);
    enter(cx, Some(false));
    draw(cx);
    let (_, composing, text) = seen(cx);
    assert!(composing && text.is_empty(), "a line: its comment opens");
    assert!(
        cx.update(|window, cx| {
            panel
                .read(cx)
                .review
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }),
        "a line: focus moves to the comment's text"
    );
    hold(cx, "a line");
}

/// [`review_panel`] open on Files with the fixture pull's files, the first one's rows
/// prepared, as the review line's Return tests start.
fn review_line_panel(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    Entity<Panel>,
    &mut gpui_kit::VisualTestContext,
) {
    let (fixture, app, panel, cx) = review_panel(cx);
    let pull = model::fixture_pull(false);
    let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured, 1, &OperationControl::default())
        .unwrap()
        .items;
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.pull = Some(pull);
            panel.review.files = files;
            panel.review.section = Section::Files;
            panel.review.show_list = false;
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    cx.executor().run_until_parked();
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.select_review_file(0, window, cx)));
    cx.executor().run_until_parked();
    (fixture, app, panel, cx)
}
fn line_draw(cx: &mut gpui_kit::VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}
/// The refusals the panel said, whether a comment is open, and its text.
fn line_seen(panel: &Entity<Panel>, cx: &mut gpui_kit::VisualTestContext) -> (usize, bool, String) {
    panel.read_with(cx, |panel, cx| {
        (
            panel.presses.error,
            panel.review.composing.is_some(),
            panel.review.input.read(cx).value().to_string(),
        )
    })
}
/// Gives the patch focus, on `line` or with no line chosen.
fn focus_line(panel: &Entity<Panel>, line: Option<usize>, cx: &mut gpui_kit::VisualTestContext) {
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            match line {
                Some(line) => panel.select_review_line(line, false, window, cx),
                None => {
                    panel.review.anchor = None;
                    panel.review.selected_row = None;
                }
            }
            panel.review.line_focus.focus(window, cx);
        })
    });
    line_draw(cx);
}
fn composer_focused(panel: &Entity<Panel>, cx: &mut gpui_kit::VisualTestContext) -> bool {
    cx.update(|window, cx| {
        panel
            .read(cx)
            .review
            .input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    })
}

/// A Return the window never saw go down begins nothing on a review line as its held
/// repeats arrive, marked held, as on macOS and in GPUI's own Wayland repeat: one held
/// down in another window or application as the patch takes focus, with no line chosen
/// and with one. After the key-up a fresh press begins one comment, focus in its text,
/// and the key's repeats (`REPEATS`), under X11 too, begin no other and type nothing
/// there. The line took Return through its `ReviewAccept` binding, which carries no held
/// mark, so the first held repeat looked like a fresh press: with no line it was refused,
/// and on a line it began a local draft (FU41-B7 code re-check, S1).
#[gpui::test]
async fn a_return_held_as_the_patch_takes_focus_begins_nothing(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::{REPEATS, enter};
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    for (case, line) in [("no line chosen", None), ("a line chosen", Some(3))] {
        focus_line(&panel, line, cx);
        let before = line_seen(&panel, cx);
        for repeat in 0..3 {
            enter(cx, Some(true));
            line_draw(cx);
            assert_eq!(
                line_seen(&panel, cx),
                before,
                "{case}, held repeat {repeat}: nothing begins or is refused"
            );
        }
        enter(cx, None);
        line_draw(cx);
        assert_eq!(line_seen(&panel, cx), before, "{case}, the key-up");
    }
    enter(cx, Some(false));
    line_draw(cx);
    let (refusals, composing, text) = line_seen(&panel, cx);
    assert!(
        composing && text.is_empty(),
        "a fresh press begins its comment"
    );
    assert!(
        composer_focused(&panel, cx),
        "focus moves to the comment's text"
    );
    for (repeat, events) in REPEATS {
        for down in events {
            enter(cx, *down);
        }
        line_draw(cx);
        assert_eq!(
            line_seen(&panel, cx),
            (refusals, true, String::new()),
            "{repeat}: nothing more begins or is typed"
        );
    }
    enter(cx, None);
    line_draw(cx);
}

/// Under X11, which marks no repeat held, another key going down while the Return that
/// began a comment is still down leaves that press's later repeats out of the comment's
/// text, as a dialog an Enter opened takes none until the key comes up
/// (`dialog_opened`); after the key-up a fresh press types its line there. The toolkit
/// counts an Enter after another key as a fresh press, so the comment's text, whose own
/// Enter binding runs before the dialog's wait for the key-up, took the next repeat and
/// typed a line (FU41-B7 code re-check, N4).
#[gpui::test]
async fn another_key_during_the_return_that_began_a_comment_types_no_line(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::enter;
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    focus_line(&panel, Some(3), cx);
    enter(cx, Some(false));
    line_draw(cx);
    let (refusals, composing, text) = line_seen(&panel, cx);
    assert!(
        composing && text.is_empty() && composer_focused(&panel, cx),
        "the press begins its comment, focus in its text"
    );
    tap(cx, "left");
    line_draw(cx);
    for repeat in 0..2 {
        enter(cx, Some(false));
        line_draw(cx);
        assert_eq!(
            line_seen(&panel, cx),
            (refusals, true, String::new()),
            "X11 repeat {repeat} after another key: nothing is typed"
        );
    }
    // A modified Enter is the comment's own: after another key, which the toolkit's
    // record ends, Shift-Return types its line, though a Shift held into the repeats
    // is the held key's own and types none.
    key(cx, "shift-enter", Some(false));
    line_draw(cx);
    assert_eq!(
        line_seen(&panel, cx),
        (refusals, true, String::new()),
        "a repeat with Shift held types nothing"
    );
    tap(cx, "left");
    key(cx, "shift-enter", Some(false));
    line_draw(cx);
    assert_eq!(
        line_seen(&panel, cx),
        (refusals, true, "\n".to_string()),
        "Shift-Return after another key types its line"
    );
    key(cx, "shift-enter", None);
    line_draw(cx);
    enter(cx, Some(false));
    line_draw(cx);
    assert_eq!(
        line_seen(&panel, cx),
        (refusals, true, "\n\n".to_string()),
        "a fresh press after the key-up types its line"
    );
    enter(cx, None);
    line_draw(cx);
}

/// One key event as the platform delivers it, with no idle pass after it: `stroke` going
/// down, marked held or not, or with `None` its key-up.
fn key(cx: &mut gpui_kit::VisualTestContext, stroke: &str, down: Option<bool>) {
    let keystroke =
        gpui::Keystroke::parse_with_os(stroke, crate::shortcuts::host_os()).expect("a keystroke");
    let input = match down {
        Some(is_held) => gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
            keystroke,
            is_held,
            prefer_character_input: false,
        }),
        None => gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }),
    };
    cx.update(|window, cx| window.dispatch_event(input, cx));
}
/// `stroke` going down and coming up.
fn tap(cx: &mut gpui_kit::VisualTestContext, stroke: &str) {
    key(cx, stroke, Some(false));
    key(cx, stroke, None);
}
/// Gives the comment's text focus.
fn focus_composer(panel: &Entity<Panel>, cx: &mut gpui_kit::VisualTestContext) {
    cx.update(|window, cx| {
        panel
            .read(cx)
            .review
            .input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx)
    });
    line_draw(cx);
}

/// Under X11 an unpaired release mid-hold, a repeat's release that ends one input batch
/// whose press arrives in the next before the application runs a task, leaves the Return
/// that began a comment down, as the toolkit's record of it stays: after another key the
/// held key's next repeat types nothing into the comment, and after the key-up a fresh
/// press types its line. Ended at once by any Enter key-up, the press let that repeat
/// type a new line (FU41-B9 code re-check, should-fix 1).
#[gpui::test]
async fn an_unpaired_release_mid_hold_keeps_the_return_off_the_comment(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::enter;
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    focus_line(&panel, Some(3), cx);
    enter(cx, Some(false));
    line_draw(cx);
    let (refusals, composing, text) = line_seen(&panel, cx);
    assert!(
        composing && text.is_empty() && composer_focused(&panel, cx),
        "the press begins its comment, focus in its text"
    );
    enter(cx, None);
    enter(cx, Some(false));
    line_draw(cx);
    tap(cx, "left");
    line_draw(cx);
    enter(cx, Some(false));
    line_draw(cx);
    assert_eq!(
        line_seen(&panel, cx),
        (refusals, true, String::new()),
        "the repeat after another key types nothing"
    );
    enter(cx, None);
    line_draw(cx);
    enter(cx, Some(false));
    line_draw(cx);
    assert_eq!(
        line_seen(&panel, cx),
        (refusals, true, "\n".to_string()),
        "a fresh press after the key-up types its line"
    );
    enter(cx, None);
    line_draw(cx);
}

/// The Return that began a comment ends wherever its key comes up, so the next fresh
/// Return in the comment types its line: on a control elsewhere in the panel, the Pull
/// requests toggle above Files; in Review, which draws the same comment, reached with the
/// pointer during the hold; and outside the panel, closed during the hold by Back to
/// repository or by Escape, then opened again. Cleared only by a key-up inside Files, the
/// press swallowed each of those Returns (FU41-B9 re-checks: code should-fix 2, security
/// L1).
#[gpui::test]
async fn the_return_that_began_a_comment_ends_wherever_its_key_comes_up(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::enter;
    let (_fixture, app, panel, cx) = review_line_panel(cx);
    let mut typed = String::new();
    for case in [
        "elsewhere in the panel",
        "in Review",
        "Back to repository",
        "Escape",
    ] {
        focus_line(&panel, Some(3), cx);
        enter(cx, Some(false));
        line_draw(cx);
        assert!(
            composer_focused(&panel, cx),
            "{case}: the press gives the comment focus"
        );
        match case {
            "elsewhere in the panel" => {
                let toggle = panel
                    .read_with(cx, |panel, _| panel.review.buttons.kept("github-back-list"))
                    .expect("Pull requests is drawn");
                cx.update(|window, cx| toggle.focus(window, cx));
            }
            "in Review" => cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.change_section(Section::Review, window, cx)
                })
            }),
            "Back to repository" => crate::copy_feedback::tests::click(cx, "github-close"),
            _ => tap(cx, "escape"),
        }
        line_draw(cx);
        enter(cx, None);
        line_draw(cx);
        if matches!(case, "Back to repository" | "Escape") {
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{case}: the panel closes"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
            cx.executor().run_until_parked();
            line_draw(cx);
        }
        focus_composer(&panel, cx);
        enter(cx, Some(false));
        line_draw(cx);
        typed.push('\n');
        assert_eq!(
            line_seen(&panel, cx).2,
            typed,
            "{case}: the next fresh Return types its line"
        );
        enter(cx, None);
        line_draw(cx);
        if case == "in Review" {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.change_section(Section::Files, window, cx)
                })
            });
            line_draw(cx);
        }
    }
}

/// The Return that began a comment, on a line or on Comment on selection, stays held off
/// it as the panel moves to Review during the hold, where the same comment is drawn and
/// takes focus: under X11 the held key's next repeat after another key types nothing
/// there, and after the key-up a fresh press types its line. Ended at once by the section
/// change, the press let that repeat type a new line (FU41-B9 fix-pass re-checks, S1).
#[gpui::test]
async fn a_return_held_into_review_types_nothing_into_its_comment(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::enter;
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    for opener in ["a line", "Comment on selection"] {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.discard_composer(window, cx);
                panel.change_section(Section::Files, window, cx);
            })
        });
        line_draw(cx);
        focus_line(&panel, Some(3), cx);
        if opener == "Comment on selection" {
            let button = panel
                .read_with(cx, |panel, _| {
                    panel.review.buttons.kept("github-inline-start")
                })
                .expect("Comment on selection is drawn");
            cx.update(|window, cx| button.focus(window, cx));
            line_draw(cx);
        }
        enter(cx, Some(false));
        line_draw(cx);
        let (refusals, composing, text) = line_seen(&panel, cx);
        assert!(
            composing && text.is_empty() && composer_focused(&panel, cx),
            "{opener}: the press opens the comment, focus in its text"
        );
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.change_section(Section::Review, window, cx)
            })
        });
        line_draw(cx);
        assert!(
            composer_focused(&panel, cx),
            "{opener}: Review gives the comment focus"
        );
        tap(cx, "left");
        line_draw(cx);
        enter(cx, Some(false));
        line_draw(cx);
        assert_eq!(
            line_seen(&panel, cx),
            (refusals, true, String::new()),
            "{opener}: the X11 repeat after another key types nothing"
        );
        enter(cx, None);
        line_draw(cx);
        enter(cx, Some(false));
        line_draw(cx);
        assert_eq!(
            line_seen(&panel, cx),
            (refusals, true, "\n".to_string()),
            "{opener}: a fresh press after the key-up types its line"
        );
        enter(cx, None);
        line_draw(cx);
    }
}

/// Return on a focused Button that opens a comment, Comment on selection or Edit, opens
/// it once and types nothing into it while the key is held, through X11 repeats, a
/// release and its press, and repeats marked held (`REPEATS`); after the key-up a fresh
/// press in the comment types its line. The Button opened the comment as the key went
/// down, and each repeat typed a new line into the comment it gave focus to (decided
/// 2026-11-11: a Return that opens a composer never types into it).
#[gpui::test]
async fn a_return_on_a_comment_opener_types_nothing_into_its_comment(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::{REPEATS, enter};
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    focus_line(&panel, Some(3), cx);
    for (opener, id, opened) in [
        (
            "Comment on selection",
            ElementId::from("github-inline-start"),
            "",
        ),
        (
            "Edit",
            ElementId::from(("github-edit-inline", 0usize)),
            "Looks right",
        ),
    ] {
        if opener == "Edit" {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel
                        .review
                        .input
                        .update(cx, |input, cx| input.set_value(opened, window, cx));
                    panel.add_comment(window, cx);
                    panel.change_section(Section::Review, window, cx);
                })
            });
            line_draw(cx);
            assert!(
                panel.read_with(cx, |panel, _| panel.review.composing.is_none()
                    && panel.review.comments.len() == 1),
                "the comment is added to the review"
            );
        }
        let button = panel
            .read_with(cx, |panel, _| panel.review.buttons.kept(id))
            .unwrap_or_else(|| panic!("{opener} is drawn"));
        cx.update(|window, cx| button.focus(window, cx));
        line_draw(cx);
        enter(cx, Some(false));
        line_draw(cx);
        let (refusals, composing, text) = line_seen(&panel, cx);
        assert!(
            composing && text == opened && composer_focused(&panel, cx),
            "{opener}: the press opens the comment, focus in its text"
        );
        for (repeat, events) in REPEATS {
            for down in events {
                enter(cx, *down);
            }
            line_draw(cx);
            assert_eq!(
                line_seen(&panel, cx),
                (refusals, true, opened.to_string()),
                "{opener}, {repeat}: nothing is typed"
            );
        }
        enter(cx, None);
        line_draw(cx);
        enter(cx, Some(false));
        line_draw(cx);
        // Typed at the caret, which a comment opened by Edit puts at its start.
        let text = line_seen(&panel, cx).2;
        assert!(
            text.matches('\n').count() == 1 && text.replace('\n', "") == opened,
            "{opener}: a fresh press after the key-up types its line: {text:?}"
        );
        enter(cx, None);
        line_draw(cx);
    }
}

/// After the Return that opened or focused an empty composer, `text` reading that
/// composer: each of the held key's repeats (`REPEATS`), and under X11 an unpaired release
/// with its press, then another key and the next repeat, type nothing there; after the
/// key-up a fresh press types its line.
fn return_held_off(
    cx: &mut gpui_kit::VisualTestContext,
    at: &str,
    text: impl Fn(&mut gpui_kit::VisualTestContext) -> String,
) {
    use crate::branch_actions::tests::{REPEATS, enter};
    for (repeat, events) in REPEATS {
        for down in events {
            enter(cx, *down);
        }
        line_draw(cx);
        assert_eq!(text(cx), "", "{at}, {repeat}: nothing is typed");
    }
    enter(cx, None);
    enter(cx, Some(false));
    line_draw(cx);
    tap(cx, "left");
    line_draw(cx);
    enter(cx, Some(false));
    line_draw(cx);
    assert_eq!(
        text(cx),
        "",
        "{at}: the X11 repeat after an unpaired release and another key types nothing"
    );
    enter(cx, None);
    line_draw(cx);
    enter(cx, Some(false));
    line_draw(cx);
    assert_eq!(
        text(cx),
        "\n",
        "{at}: a fresh press after the key-up types its line"
    );
    enter(cx, None);
    line_draw(cx);
}

/// Return on the Review tab, which gives focus to the review body, or to the comment open
/// on a line, takes the panel to Review and types nothing into that composer while the
/// key is held ([`return_held_off`]); after the key-up a fresh press types its line. The
/// tab took the press as the key went down, and each of its repeats typed a new line into
/// the composer it had given focus to (FU41-B9 fix-pass re-checks, S2; decided
/// 2026-11-11: a Return that opens or focuses a composer never types into it).
#[gpui::test]
async fn a_return_on_the_review_tab_types_nothing_into_its_composer(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::enter;
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    for composer in ["the review body", "the comment"] {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.change_section(Section::Files, window, cx);
                if composer == "the comment" {
                    panel.select_review_line(3, false, window, cx);
                    panel.begin_comment(window, cx);
                }
            })
        });
        line_draw(cx);
        let tab = panel
            .read_with(cx, |panel, _| {
                panel.review.buttons.kept(("github-section", 2usize))
            })
            .expect("the Review tab is drawn");
        cx.update(|window, cx| tab.focus(window, cx));
        line_draw(cx);
        enter(cx, Some(false));
        line_draw(cx);
        let text = |cx: &mut gpui_kit::VisualTestContext| {
            panel.read_with(cx, |panel, cx| {
                let input = if composer == "the comment" {
                    &panel.review.input
                } else {
                    &panel.body
                };
                input.read(cx).value().to_string()
            })
        };
        let focused = cx.update(|window, cx| {
            let panel = panel.read(cx);
            let input = if composer == "the comment" {
                &panel.review.input
            } else {
                &panel.body
            };
            panel.review.section == Section::Review
                && input.read(cx).focus_handle(cx).is_focused(window)
        });
        assert!(
            focused && text(cx).is_empty(),
            "{composer}: the press shows Review, focus in its empty text"
        );
        return_held_off(cx, composer, text);
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel
                    .body
                    .update(cx, |body, cx| body.set_value("", window, cx));
            })
        });
    }
}

/// Return on a conversation's Reply opens its reply, focus in the reply's text, and types
/// nothing there while the key is held ([`return_held_off`]); after the key-up a fresh
/// press types its line. Reply opened the reply as the key went down, and each of its
/// repeats typed a new line there (FU41-B9 fix-pass re-checks, S2; decided 2026-11-11: a
/// Return that opens or focuses a composer never types into it).
#[gpui::test]
async fn a_return_on_reply_types_nothing_into_its_reply(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::enter;
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    receive_fixture_threads(&panel, cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.change_section(Section::Overview, window, cx)
        })
    });
    line_draw(cx);
    let reply = panel
        .read_with(cx, |panel, _| {
            panel.review.buttons.kept(("github-thread-reply", 0usize))
        })
        .expect("Reply is drawn");
    cx.update(|window, cx| reply.focus(window, cx));
    line_draw(cx);
    enter(cx, Some(false));
    line_draw(cx);
    let text = |cx: &mut gpui_kit::VisualTestContext| {
        panel.read_with(cx, |panel, cx| {
            panel.conversations.input.read(cx).value().to_string()
        })
    };
    let opened = cx.update(|window, cx| {
        let panel = panel.read(cx);
        panel.conversations.active.is_some()
            && panel
                .conversations
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    });
    assert!(
        opened && text(cx).is_empty(),
        "the press opens the reply, focus in its empty text"
    );
    return_held_off(cx, "Reply", text);
}

/// The fixture pull's conversations received for the offline reviewer, so Reply can open
/// the first one's reply.
fn receive_fixture_threads(panel: &Entity<Panel>, cx: &mut gpui_kit::VisualTestContext) {
    let identity = model::fixture_pull(false)
        .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let page = Client(UiTransport::Fixture { moved: false })
        .review_threads(&identity, None, &OperationControl::default())
        .unwrap();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/native-review", window, cx)
            });
            panel.account = Some("offline-reviewer".into());
            panel.receive_threads(page, None);
        })
    });
    line_draw(cx);
}

/// Return on `route`, a Button used in Review whose action leaves focus in the empty
/// review body: Add to review and Discard this comment, beside the comment open on a line,
/// a collected comment's Remove, an open reply's Save and close reply, and a
/// confirmation's Keep editing. The press acts
/// once and types nothing into the body while the key is held ([`return_held_off`]);
/// after the key-up a fresh press types its line. Each Button took the press as the key
/// went down, and each of its repeats typed a new line into the body (decided 2026-11-11:
/// a Return that opens or focuses a composer never types into it).
fn a_return_left_in_the_review_body(cx: &mut TestAppContext, route: &str) {
    use crate::branch_actions::tests::enter;
    let (_fixture, _app, panel, cx) = review_line_panel(cx);
    let button: ElementId = match route {
        "Add to review" => "github-add-comment".into(),
        "Discard this comment" => "github-discard-composer".into(),
        "Remove" => ("github-remove-inline", 0usize).into(),
        "Keep editing" => "github-edit-submission".into(),
        _ => "github-close-reply".into(),
    };
    if route == "Save and close reply" {
        receive_fixture_threads(&panel, cx);
    }
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            if route == "Save and close reply" {
                panel.change_section(Section::Review, window, cx);
                let target = panel.conversations.threads[0].target.clone();
                panel.begin_reply(target, window, cx);
                return;
            }
            if route == "Keep editing" {
                panel.change_section(Section::Review, window, cx);
                let repository = Repository::parse("gitturtle-fixture/native-review").unwrap();
                panel.confirm = Some(Confirmation {
                    action: Action::Comment {
                        pull: model::fixture_pull(false).capture(repository),
                        body: "Looks right".into(),
                    },
                    account: "offline-reviewer".into(),
                });
                panel.focus_confirmation(window, cx);
                return;
            }
            panel.select_review_line(3, false, window, cx);
            panel.begin_comment(window, cx);
            panel
                .review
                .input
                .update(cx, |input, cx| input.set_value("Looks right", window, cx));
            if route == "Remove" {
                panel.add_comment(window, cx);
            }
            panel.change_section(Section::Review, window, cx);
        })
    });
    line_draw(cx);
    let focus = panel
        .read_with(cx, |panel, _| panel.review.buttons.kept(button))
        .unwrap_or_else(|| panic!("{route} is drawn"));
    cx.update(|window, cx| focus.focus(window, cx));
    line_draw(cx);
    enter(cx, Some(false));
    line_draw(cx);
    let (acted, focused) = cx.update(|window, cx| {
        let panel = panel.read(cx);
        let acted = match route {
            "Add to review" => panel.review.comments.len() == 1,
            "Save and close reply" => panel.conversations.active.is_none(),
            "Keep editing" => panel.confirm.is_none(),
            _ => panel.review.comments.is_empty(),
        } && panel.review.composing.is_none();
        (
            acted,
            panel.body.read(cx).focus_handle(cx).is_focused(window),
        )
    });
    assert!(
        acted && focused,
        "{route}: the press acts, focus in the review body"
    );
    let text = |cx: &mut gpui_kit::VisualTestContext| {
        panel.read_with(cx, |panel, cx| panel.body.read(cx).value().to_string())
    };
    return_held_off(cx, route, text);
}
#[gpui::test]
async fn a_return_on_add_to_review_types_nothing_into_the_review_body(cx: &mut TestAppContext) {
    a_return_left_in_the_review_body(cx, "Add to review");
}
#[gpui::test]
async fn a_return_on_discard_this_comment_types_nothing_into_the_review_body(
    cx: &mut TestAppContext,
) {
    a_return_left_in_the_review_body(cx, "Discard this comment");
}
#[gpui::test]
async fn a_return_on_remove_types_nothing_into_the_review_body(cx: &mut TestAppContext) {
    a_return_left_in_the_review_body(cx, "Remove");
}
#[gpui::test]
async fn a_return_on_keep_editing_types_nothing_into_the_review_body(cx: &mut TestAppContext) {
    a_return_left_in_the_review_body(cx, "Keep editing");
}
#[gpui::test]
async fn a_return_on_save_and_close_reply_types_nothing_into_the_review_body(
    cx: &mut TestAppContext,
) {
    a_return_left_in_the_review_body(cx, "Save and close reply");
}

/// 8,000 deletions and 8,000 additions of 520 bytes, just under the 8 MiB response bound:
/// 16,001 inline rows, inside the 20,000-row limit, and above Compare's bounds, so no
/// presentation is prepared.
const LARGE_LINES: usize = 8_000;
fn large_file(name: &str, fill: char) -> PullFile {
    let mut patch = format!("@@ -1,{LARGE_LINES} +1,{LARGE_LINES} @@\n");
    for marker in ['-', '+'] {
        for line in 0..LARGE_LINES {
            patch.push(marker);
            patch.push_str(&format!("{line:08} "));
            patch.extend(std::iter::repeat_n(fill, 510));
            patch.push('\n');
        }
    }
    assert!(patch.len() <= github::MAX_RESPONSE && patch.len() > 8_000_000);
    PullFile {
        filename: name.into(),
        previous_filename: None,
        status: "modified".into(),
        additions: LARGE_LINES as u32,
        deletions: LARGE_LINES as u32,
        patch: Some(patch.into()),
        rows: Vec::new(),
        unavailable: None,
    }
}
/// Selecting a file hands its preparation the patch the view holds, not a copy made on
/// the UI thread, and the prepared file keeps that allocation.
#[gpui::test]
async fn selecting_a_review_file_shares_its_patch_with_the_preparation(cx: &mut TestAppContext) {
    let (_fixture, app, panel, cx) = review_panel(cx);
    let file = large_file("src/large.rs", 'a');
    // Compared, never read: the address of the bytes the view holds.
    let held = file.patch.as_deref().expect("a patch").as_ptr();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.review.files = vec![file];
            panel.review.section = Section::Files;
            panel.select_review_file(0, window, cx);
        })
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let review = &panel.read(cx).review;
        assert!(!review.preparing);
        assert_eq!(review.files[0].rows.len(), 2 * LARGE_LINES + 1);
        assert_eq!(
            review.files[0].patch.as_deref().expect("a patch").as_ptr(),
            held,
            "the preparation took the view's allocation"
        );
    });
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.run_until_parked();
}
/// Closing the panel cancels the selected file's pending preparation; reopening resumes it
/// without the file being chosen again, in Changes and in Source, which keeps its editor
/// and takes the decorations Compare would draw. A preparation that failed is not repeated.
#[gpui::test]
async fn reopening_resumes_an_interrupted_file_preparation(cx: &mut TestAppContext) {
    let (_fixture, app, panel, cx) = review_panel(cx);
    let patches = [
        "@@ -1,2 +1,2 @@\n context\n-old value\n+new value\n",
        "@@ -1 +1,2 @@\n-before\n+after\n+more\n",
    ];
    let file = |name: &str, patch: &str, additions| PullFile {
        filename: name.into(),
        previous_filename: None,
        status: "modified".into(),
        additions,
        deletions: 1,
        patch: Some(patch.into()),
        rows: Vec::new(),
        unavailable: None,
    };
    cx.update(|_, cx| {
        panel.update(cx, |panel, _| {
            panel.review.files = vec![
                file("src/a.rs", patches[0], 1),
                file("src/b.rs", patches[1], 2),
                // A path the preparation refuses.
                file("../outside.rs", patches[0], 1),
            ];
            panel.review.section = Section::Files;
        })
    });
    let close_and_reopen = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        // The cancelled preparation's result, if any, has nowhere to land.
        cx.run_until_parked();
        cx.read(|cx| {
            let review = &panel.read(cx).review;
            assert!(!review.preparing);
            assert!(review.files.iter().all(|file| file.rows.is_empty()));
            assert!(review.presentation.is_none());
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
    };

    // Changes: the rows and presentation arrive after reopening.
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.select_review_file(0, window, cx)));
    close_and_reopen(cx);
    cx.read(|cx| {
        let review = &panel.read(cx).review;
        assert_eq!((review.selected_file, review.mode), (Some(0), 0));
        assert!(!review.preparing);
        assert_eq!(review.files[0].rows.len(), 4);
        assert!(review.presentation.is_some());
    });

    // Source: the editor opened before the close is kept and decorated, with Compare's
    // gutter, as Compare decorates the same text.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.select_review_file(1, window, cx);
            panel.review_source(1, window, cx);
        })
    });
    let source = cx.read(|cx| panel.read(cx).review.source.clone().expect("Source editor"));
    close_and_reopen(cx);
    let expected = cx.update(|window, cx| {
        let presentation = text::PatchPresentation::prepare(patches[1]);
        let (_editor, decorations) =
            text::editor_with_decorations(patches[1], "diff", Some(&presentation), window, cx);
        decorations
            .expect("a prepared patch is decorated")
            .drawn(cx)
    });
    cx.read(|cx| {
        let review = &panel.read(cx).review;
        assert_eq!((review.selected_file, review.mode), (Some(1), 1));
        assert_eq!(review.files[1].rows.len(), 4);
        assert_eq!(
            review.source.as_ref(),
            Some(&source),
            "Source keeps its editor"
        );
        assert!(
            review.source_view.is_some(),
            "Source takes Compare's gutter"
        );
        assert_eq!(
            editor_find::patch_layer(&source, cx).map(|layer| layer.drawn(cx)),
            Some(expected)
        );
    });

    // A failed preparation reports once; reopening does not run it again.
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.select_review_file(2, window, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        panel.update(cx, |panel, _| {
            assert!(panel.error.take().is_some(), "the refused path is reported");
        })
    });
    close_and_reopen(cx);
    cx.read(|cx| {
        let panel = panel.read(cx);
        assert_eq!(panel.review.selected_file, Some(2));
        assert_eq!(panel.error, None, "the failed preparation is not repeated");
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.run_until_parked();
}
/// The UI-thread time of `select_review_file` between two files whose patches are just
/// under the 8 MiB response bound: after the previous file finished preparing, and while it
/// is still preparing. Only the handler is timed; preparation runs untimed between samples.
#[gpui::test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_REVIEW_SELECT_BENCH_OUTPUT and run with --release --ignored"]
async fn select_review_file_release_benchmark(cx: &mut TestAppContext) {
    const WARMUPS: usize = 5;
    const SAMPLES: usize = 60;
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "performance evidence requires --release"
    );
    let output = std::env::var_os("GITTURTLE_REVIEW_SELECT_BENCH_OUTPUT")
        .expect("the runner supplies the result destination");
    let (_fixture, app, panel, cx) = review_panel(cx);
    let first = large_file("src/first.rs", 'a');
    let patch_bytes = first.patch.as_deref().map_or(0, str::len);
    cx.update(|_, cx| {
        panel.update(cx, |panel, _| {
            panel.review.files = vec![first, large_file("src/second.rs", 'b')];
            panel.review.section = Section::Files;
        })
    });
    let select = |index: usize, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let started = std::time::Instant::now();
                panel.select_review_file(index, window, cx);
                started.elapsed().as_secs_f64() * 1000.
            })
        })
    };
    let mut series = Vec::new();
    for (name, prepared) in [("after_preparation", true), ("while_preparing", false)] {
        select(0, cx);
        cx.run_until_parked();
        let mut samples = Vec::new();
        for attempt in 0..WARMUPS + SAMPLES {
            let index = (attempt + 1) % 2;
            let milliseconds = select(index, cx);
            if prepared {
                cx.run_until_parked();
                cx.read(|cx| {
                    let review = &panel.read(cx).review;
                    assert!(!review.preparing);
                    assert_eq!(review.files[index].rows.len(), 2 * LARGE_LINES + 1);
                });
            } else {
                cx.read(|cx| assert!(panel.read(cx).review.preparing));
            }
            if attempt >= WARMUPS {
                samples.push(milliseconds);
            }
        }
        cx.run_until_parked();
        series.push(serde_json::json!({"name": name, "milliseconds": samples}));
    }
    let result = serde_json::json!({
        "schema": 1,
        "warmups_per_series": WARMUPS,
        "samples_per_series": SAMPLES,
        "patch_bytes": patch_bytes,
        "rows_per_file": 2 * LARGE_LINES + 1,
        "boundary": "One `select_review_file` call on the UI thread, from before the call to its return; excludes GPUI's effect flush, rendering and the background preparation.",
        "series": series,
    });
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.run_until_parked();
}

/// The focus sweep (`focus_ring::sweep`) over the panel on the offline
/// fixture, at the default and the largest interface size: its pull
/// request list, then a pull request's Overview with the first page of
/// its conversations, as the native review loads it, its Files with the
/// first file's patch open, and its Review. Tab shows every stop whole:
/// the conversations pane fits inside the body, and the conversation list
/// scrolls to the actions Tab moves onto. Review's first stop once drew
/// no ring at 18 pt, with a frame drawn before the walk too: a long draft
/// status pushed Review submission… past the panel's right edge, so the
/// product, not the walk, was at fault
/// (`github_view::tests::the_draft_status_gives_way_to_review_submission`).
#[gpui::test]
async fn the_panel_draws_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    use crate::focus_ring::sweep;
    let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
    sweep::prepare(&app, cx);
    let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
    let panel = cx.update(|window, cx| {
        cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
    });
    app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
        .await
        .unwrap()
        .unwrap();
    cx.executor().run_until_parked();
    let pull = model::fixture_pull(false);
    let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let files = Client(UiTransport::Fixture { moved: false })
        .files(&captured, 1, &OperationControl::default())
        .unwrap()
        .items;
    let threads = Client(UiTransport::Fixture { moved: false })
        .review_threads(&captured, None, &OperationControl::default())
        .unwrap();
    assert!(
        !threads.items.is_empty(),
        "the fixture's first page of conversations"
    );
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/native-review", window, cx)
            });
            panel.rows = vec![pull.clone()];
            panel.review.fixture = true;
            panel.account = Some("offline-reviewer".into());
            panel.receive_threads(threads, None);
            panel.close(window, cx);
        });
        app.update(cx, |app, cx| app.open_github(window, cx));
    });
    cx.executor().run_until_parked();
    let mut failures = Vec::new();
    for interface in sweep::SIZES {
        sweep::interface_size(cx, interface);
        // Each change of view moves focus as the panel's own transitions
        // do, so no walk starts on a pane the panel no longer draws.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = None;
                panel.review.show_list = true;
                panel.focus_visible_section(window, cx);
                cx.notify();
            })
        });
        cx.executor().run_until_parked();
        sweep::walk(
            &app,
            cx,
            "the GitHub panel's list",
            interface,
            &mut failures,
        );
        for (section, name) in [
            (Section::Overview, "the GitHub panel's Overview"),
            (Section::Files, "the GitHub panel's Files"),
            (Section::Review, "the GitHub panel's Review"),
        ] {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.pull = Some(pull.clone());
                    panel.review.files = files.clone();
                    panel.review.section = section;
                    panel.review.show_list = false;
                    if section == Section::Files {
                        panel.select_review_file(0, window, cx);
                    }
                    panel.focus_visible_section(window, cx);
                    cx.notify();
                })
            });
            cx.executor().run_until_parked();
            sweep::walk(&app, cx, name, interface, &mut failures);
        }
    }
    sweep::assert_whole(failures);
}

/// Changes' range hint, laid out in a box swept from 461 px down to its widest run at 13
/// and 18 pt, breaks only between whole words: no line starts with a phrase's "·" and no
/// key name breaks across two lines, its lines read as its text does inside the box, and
/// its accessible text is the whole hint. Where only a phrase's trailing space ran past the
/// line, the "·" before it wrapped onto a line of its own, and where the second phrase
/// wrapped, GPUI broke "Shift+Down" before its "+" (review of FU30-D2).
#[gpui::test]
fn the_range_hint_keeps_its_key_names_and_separators_whole(cx: &mut TestAppContext) {
    let keys = [
        shortcuts::SHIFT_CLICK.to_owned(),
        shortcuts::key_label("shift-up"),
        shortcuts::key_label("shift-down"),
        shortcuts::ENTER.to_owned(),
    ];
    let runs: Vec<SharedString> = crate::phrase_runs(&range_hint())
        .into_iter()
        .flat_map(|phrase| phrase.runs)
        .collect();
    crate::phrase_tests::sweep(
        cx,
        ("github-range-hint", "github-range-hint"),
        &range_hint(),
        &keys,
        move |window| crate::phrase_tests::widest(&runs, appearance::ui_text(12.), window),
        || super::super::phrases("github-range-hint", range_hint()).into_any_element(),
    );
}
