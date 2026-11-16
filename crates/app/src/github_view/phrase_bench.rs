//! Opt-in release benchmark of the GitHub panel's body scroll
//! ([`body_scroll_release_benchmark`]), with the panel's lines of " · "-separated phrases
//! (`crate::PhraseLine`, drawn through `super::phrases`) in view: each wheel step lays every
//! one of them out again, from what it kept from the frame before.
//!
//! The driver uses only what the panel has at `00d13fd7`, and reads the phrase lines through
//! their wrappers' selectors, which a `PhraseLine` drawn as plain text keeps, so the same test
//! module measures `PhraseLine` against plain text. Build the test executable with
//! `cargo test --locked --release -p gitturtle --bin gitturtle --no-run` and run it pinned to
//! the performance cores, as
//! `GITTURTLE_GITHUB_SCROLL_BENCH_OUTPUT=<file.json> <executable> --ignored --exact
//! github_view::phrase_bench::body_scroll_release_benchmark --test-threads 1`.

use super::*;
use core::prelude::v1::test;

/// The panel's texts drawn as a `PhraseLine` in its body on the PR's Overview with the PR
/// list shown, by their selector: the list's page, the conversations' heading and page, and
/// the drafts' policy.
const PHRASE_LINES: [&str; 4] = [
    "github-page",
    "github-conversations-heading",
    "github-threads-page",
    "github-passive-policy",
];

/// The UI thread's frame of a wheel step over the GitHub panel's body, for a release build:
/// the panel open on the offline review fixture (`GITTURTLE_GITHUB_FIXTURE=review`'s data,
/// loaded as the panel's tests load it: its two PRs listed, the first open, signed in as the
/// fixture's reviewer, with the first page of its conversations and its files) on the PR's
/// Overview with the PR list shown, the state whose body holds the most phrase lines (the
/// four of [`PHRASE_LINES`]), at 13 pt in a 1,480 × 800 window and at 18 pt in a 461 × 490
/// half tile, with quiet reads held so nothing it shows changes. Each timed frame dispatches
/// one wheel notch (three lines, as GPUI's X11 and Wayland backends deliver one) over the
/// body's left padding, down until the body reaches its foot and then up to its top and so
/// on, so every frame moves the body and draws: one `cx.update` that dispatches the event,
/// from before the call to its return, which includes the effects it flushes, drawing the
/// window the scroll dirtied, once (the test app's `flush_effects`): the root render, the
/// repository page under the dialog and the panel's, layout, prepaint and paint on the test
/// platform, with no GPU, compositor or display link. `cpu_ms` is the UI thread's CPU time
/// (`CLOCK_THREAD_CPUTIME_ID`) and `wall_ms` the elapsed time, each with its median, p95
/// and p99 (nearest rank). Per timed frame, `frames` counts the repository columns its draw
/// laid out, `phrase_lines` the phrase lines it laid out (wrappers of [`PHRASE_LINES`] in
/// the frame's debug bounds) and `phrase_runs` the run elements a `PhraseLine` lays out for
/// them (`{id}-{phrase}-{run}`; none where they draw as plain text); the benchmark asserts
/// one repository frame, all four phrase lines and a moved body in each. Frames per series
/// come from `GITTURTLE_GITHUB_SCROLL_BENCH_FRAMES` (300 unless set), after a tenth as many
/// untimed; a debug build runs only as a check of the driver, at ten or fewer.
#[gpui::test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_GITHUB_SCROLL_BENCH_OUTPUT and run with --release --ignored"]
async fn body_scroll_release_benchmark(cx: &mut TestAppContext) {
    let output = std::env::var_os("GITTURTLE_GITHUB_SCROLL_BENCH_OUTPUT")
        .expect("the runner supplies the result destination");
    let frames: usize = std::env::var("GITTURTLE_GITHUB_SCROLL_BENCH_FRAMES")
        .map(|count| count.parse().expect("a whole number of frames"))
        .unwrap_or(300);
    let release = !std::hint::black_box(cfg!(debug_assertions));
    assert!(frames > 0, "at least one frame");
    assert!(
        release || frames <= 10,
        "performance evidence requires --release; a debug build only checks the driver, at \
         ten frames or fewer"
    );
    let warmup = (frames / 10).max(1);
    let thread_cpu_ms = || {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: writes into a local.
        assert_eq!(
            unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) },
            0
        );
        time.tv_sec as f64 * 1000. + time.tv_nsec as f64 / 1_000_000.
    };
    let summary = |samples: &[f64]| {
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let rank = |quantile: f64| {
            let rank = (quantile * sorted.len() as f64).ceil() as usize;
            sorted[rank.clamp(1, sorted.len()) - 1]
        };
        serde_json::json!({ "median": rank(0.5), "p95": rank(0.95), "p99": rank(0.99) })
    };
    let repository_frames = || {
        crate::in_frame::probe::take()
            .iter()
            .filter(|frame| frame.region == "repository-columns")
            .count()
    };

    let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
    crate::focus_ring::sweep::prepare(&app, cx);
    crate::views::tests::hold_quiet_reads_for_test(&app, cx);
    let pull = github::review::fixture_pull(false);
    let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
    let mut client = Client(UiTransport::Fixture { moved: false });
    let threads = client
        .review_threads(&captured, None, &OperationControl::default())
        .unwrap();
    let files = client
        .files(&captured, 1, &OperationControl::default())
        .unwrap()
        .items;
    assert!(
        !threads.items.is_empty() && !files.is_empty(),
        "the fixture's conversations and files"
    );
    let (conversations, file_count) = (threads.items.len(), files.len());
    let panel = cx.update(|window, cx| {
        open_offline_fixture_with(&app, window, cx, move |panel| {
            panel.review.show_list = true;
            panel.review.section = Section::Overview;
            panel.receive_threads(threads, None);
            panel.review.files = files;
        })
    });
    settle(&app, cx).await;
    let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
    let pull_rows = panel.read_with(cx, |panel, _| panel.rows.len());

    crate::in_frame::probe::watch();
    let mut series = Vec::new();
    for (text_pt, window_size) in [
        (13, size(px(1480.), px(800.))),
        (18, size(px(461.), px(490.))),
    ] {
        crate::focus_ring::sweep::interface_size(cx, text_pt);
        cx.simulate_resize(window_size);
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.show_list = true;
                panel.review.section = Section::Overview;
                panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                panel.focus_visible_section(window, cx);
                cx.notify();
            })
        });
        settle(&app, cx).await;
        let at = format!("{text_pt} pt in {window_size:?}");
        let body = cx.debug_bounds("github-panel").expect("the panel's body");
        let step = cx.update(|window, _| window.line_height()) * 3.;
        let travel = scroll.max_offset().y;
        assert!(
            travel > step * 2.,
            "{at}: the body scrolls more than two wheel notches: {travel:?} for {step:?}"
        );
        for id in PHRASE_LINES {
            assert!(cx.debug_bounds(id).is_some(), "{at}: the body draws {id}");
        }
        // Each phrase line's run elements as `PhraseLine` names them, found once, where it
        // draws them.
        let mut runs: Vec<&'static str> = Vec::new();
        for id in PHRASE_LINES {
            for phrase in 0.. {
                let first: &'static str = format!("{id}-{phrase}-0").leak();
                if cx.debug_bounds(first).is_none() {
                    break;
                }
                runs.push(first);
                for run in 1.. {
                    let next: &'static str = format!("{id}-{phrase}-{run}").leak();
                    if cx.debug_bounds(next).is_none() {
                        break;
                    }
                    runs.push(next);
                }
            }
        }
        // Over the body's left padding, beside its parts, so no list or editor inside it
        // takes the wheel.
        let position = body.origin + point(px(2.), body.size.height / 2.);
        let mut down = true;
        let mut cpu_ms = Vec::new();
        let mut wall_ms = Vec::new();
        let mut drawn = Vec::new();
        let mut phrase_lines = Vec::new();
        let mut phrase_runs = Vec::new();
        let mut offsets = Vec::new();
        for frame in 0..warmup + frames {
            cx.update(|window, cx| window.simulate_next_frame(cx));
            cx.run_until_parked();
            repository_frames();
            let before = scroll.offset().y;
            if down && before <= -scroll.max_offset().y + px(0.5) {
                down = false;
            } else if !down && before >= px(-0.5) {
                down = true;
            }
            let event = PlatformInput::ScrollWheel(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Lines(point(0., if down { -3. } else { 3. })),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            });
            // The update's effects draw the window the scroll dirtied, once, before it
            // returns (the test app's `flush_effects`).
            let (cpu, wall) = (thread_cpu_ms(), std::time::Instant::now());
            cx.update(|window, cx| {
                window.dispatch_event(event, cx);
            });
            let (cpu, wall) = (thread_cpu_ms() - cpu, wall.elapsed().as_secs_f64() * 1000.);
            let after = scroll.offset().y;
            let lines = PHRASE_LINES
                .iter()
                .filter(|id| cx.debug_bounds(id).is_some())
                .count();
            let laid = runs
                .iter()
                .filter(|id| cx.debug_bounds(id).is_some())
                .count();
            cx.run_until_parked();
            assert!(
                after != before,
                "{at}: frame {frame}'s wheel notch moves the body from {before:?}"
            );
            if frame >= warmup {
                cpu_ms.push(cpu);
                wall_ms.push(wall);
                drawn.push(repository_frames());
                phrase_lines.push(lines);
                phrase_runs.push(laid);
                offsets.push(f32::from(after));
            } else {
                repository_frames();
            }
        }
        assert!(
            drawn.iter().all(|&count| count == 1),
            "{at}: every timed wheel notch draws the window once: {drawn:?}"
        );
        assert!(
            phrase_lines
                .iter()
                .all(|&count| count == PHRASE_LINES.len()),
            "{at}: every timed frame lays out the body's phrase lines: {phrase_lines:?}"
        );
        series.push(serde_json::json!({
            "surface": "overview-with-list",
            "text_pt": text_pt,
            "window": [f32::from(window_size.width), f32::from(window_size.height)],
            "body_travel_px": f32::from(travel),
            "wheel_step_px": f32::from(step),
            "phrase_run_elements": runs.len(),
            "cpu_ms_summary": summary(&cpu_ms),
            "wall_ms_summary": summary(&wall_ms),
            "cpu_ms": cpu_ms,
            "wall_ms": wall_ms,
            "frames": drawn,
            "phrase_lines": phrase_lines,
            "phrase_runs": phrase_runs,
            "offset_px": offsets,
        }));
    }
    crate::in_frame::probe::stop();
    let result = serde_json::json!({
        "schema": 1,
        "profile": if release { "release" } else { "debug" },
        "frames": frames,
        "warmup_frames": warmup,
        "fixture": {
            "pull": pull.number,
            "pull_rows": pull_rows,
            "conversations": conversations,
            "files": file_count,
            "phrase_lines": PHRASE_LINES,
        },
        "boundary": "One `cx.update` on the UI thread that dispatches a wheel notch (three lines) over the GitHub panel's body, which moves it, from before the call to its return: the dispatch and the effects it flushes, which draw the window the scroll dirtied once (the root render, the repository page under the dialog and the panel, layout, prepaint and paint on the test platform; no GPU, compositor or display link), quiet reads held. `cpu_ms` is the UI thread's CPU time across the call, `wall_ms` its elapsed time; `frames` counts the repository columns that draw laid out, `phrase_lines` the body's phrase lines it laid out and `phrase_runs` the run elements a `PhraseLine` drew for them. The frame callbacks before it and any work it leaves queued are delivered untimed.",
        "series": series,
    });
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}

/// Lets the panel's reads and the app's writers land, then draws.
async fn settle(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let (operations_done, preferences_done) = app.read_with(cx, |app, _| {
        (
            app.operations.submit_read(|| Ok(())),
            app.preferences_writer.submit_read(|| Ok(())),
        )
    });
    operations_done.await.unwrap().unwrap();
    preferences_done.await.unwrap().unwrap();
    cx.executor().run_until_parked();
    for _ in 0..3 {
        crate::width_walk::frame(cx);
    }
}
