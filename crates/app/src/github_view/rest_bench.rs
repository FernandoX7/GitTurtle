//! Opt-in release benchmark of the GitHub panel's frame at rest
//! ([`panel_at_rest_release_benchmark`]). Every frame the panel draws, each
//! of its texts reports the lines it draws to the body collecting them
//! (`review::text_lines`), so a frame with nothing changed pays for that
//! text work as much as one that moved.
//!
//! The driver uses only what the panel had before its texts kept their lines
//! from one frame to the next (`64631542`), so the same test module measures
//! that base and a candidate. Build the test executable with
//! `cargo test --locked --release -p gitturtle --bin gitturtle --no-run`
//! and run it pinned to the performance cores, as
//! `GITTURTLE_GITHUB_REST_BENCH_OUTPUT=<file.json> <executable> --ignored --exact
//! github_view::rest_bench::panel_at_rest_release_benchmark --test-threads 1`.

use super::*;
use core::prelude::v1::test;

/// The UI thread's frame at rest in the GitHub panel, for a release build:
/// the panel open on the offline review fixture (`GITTURTLE_GITHUB_FIXTURE=review`'s
/// data, loaded as the panel's tests load it: its two PRs listed, the first
/// open, signed in as the fixture's reviewer, with the first page of its
/// conversations and its files), on the PR's Overview and on its Files with
/// the first file's patch open, each in a 1,480 × 800 window and a 461 × 490
/// half tile at 13 pt, with quiet reads held so nothing it shows changes.
/// Each frame times one `panel.update` that notifies the panel, as a caret's
/// blink or a hover does, from before the call to its return: the effects it
/// flushes draw the window it dirtied, once (the test app's `flush_effects`),
/// with the root render, the repository page under the dialog and the
/// panel's, layout, prepaint and paint on the test platform, with no GPU,
/// compositor or display link. `cpu_ms` is the UI thread's CPU time
/// (`CLOCK_THREAD_CPUTIME_ID`) and `wall_ms` the elapsed time, each with its
/// median, p95 and p99 (nearest rank); `frames` counts the repository
/// columns each timed update's draw laid out, which the benchmark asserts is
/// one. Frames per series come from `GITTURTLE_GITHUB_REST_BENCH_FRAMES` (300
/// unless set), after a tenth as many untimed; a debug build runs only as a
/// check of the driver, at ten or fewer.
#[gpui::test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_GITHUB_REST_BENCH_OUTPUT and run with --release --ignored"]
async fn panel_at_rest_release_benchmark(cx: &mut TestAppContext) {
    let output = std::env::var_os("GITTURTLE_GITHUB_REST_BENCH_OUTPUT")
        .expect("the runner supplies the result destination");
    let frames: usize = std::env::var("GITTURTLE_GITHUB_REST_BENCH_FRAMES")
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
            panel.review.show_list = false;
            panel.review.section = Section::Overview;
            panel.receive_threads(threads, None);
            panel.review.files = files;
        })
    });
    settle(&app, cx).await;

    crate::in_frame::probe::watch();
    let mut series = Vec::new();
    for (section, surface, shown) in [
        (Section::Overview, "overview", "github-thread-section"),
        (Section::Files, "files", "github-patch-rows"),
    ] {
        for window_size in [size(px(1480.), px(800.)), size(px(461.), px(490.))] {
            crate::focus_ring::sweep::interface_size(cx, 13);
            cx.simulate_resize(window_size);
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.review.section = section;
                    if section == Section::Files {
                        panel.select_review_file(0, window, cx);
                    }
                    panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                    panel.focus_visible_section(window, cx);
                    cx.notify();
                })
            });
            settle(&app, cx).await;
            let at = format!("{surface} in {window_size:?}");
            assert!(
                cx.debug_bounds(shown).is_some(),
                "{at}: the panel draws {shown}"
            );
            let mut cpu_ms = Vec::new();
            let mut wall_ms = Vec::new();
            let mut drawn = Vec::new();
            for frame in 0..warmup + frames {
                cx.update(|window, cx| window.simulate_next_frame(cx));
                cx.run_until_parked();
                repository_frames();
                // The update's effects draw the window it dirtied, once,
                // before it returns (the test app's `flush_effects`).
                let (cpu, wall) = (thread_cpu_ms(), std::time::Instant::now());
                panel.update(cx, |_, cx| cx.notify());
                let (cpu, wall) = (thread_cpu_ms() - cpu, wall.elapsed().as_secs_f64() * 1000.);
                cx.run_until_parked();
                if frame >= warmup {
                    cpu_ms.push(cpu);
                    wall_ms.push(wall);
                    drawn.push(repository_frames());
                }
            }
            assert!(
                drawn.iter().all(|&count| count == 1),
                "{at}: every timed update draws the window once: {drawn:?}"
            );
            series.push(serde_json::json!({
                "surface": surface,
                "text_pt": 13,
                "window": [f32::from(window_size.width), f32::from(window_size.height)],
                "cpu_ms_summary": summary(&cpu_ms),
                "wall_ms_summary": summary(&wall_ms),
                "cpu_ms": cpu_ms,
                "wall_ms": wall_ms,
                "frames": drawn,
            }));
        }
    }
    crate::in_frame::probe::stop();
    let result = serde_json::json!({
        "schema": 1,
        "profile": if release { "release" } else { "debug" },
        "frames": frames,
        "warmup_frames": warmup,
        "fixture": {
            "pull": pull.number,
            "conversations": conversations,
            "files": file_count,
        },
        "boundary": "One `panel.update` on the UI thread that notifies the GitHub panel with nothing it shows changed, from before the call to its return: the notification and the effects it flushes, which draw the window it dirtied once (the root render, the repository page under the dialog and the panel, layout, prepaint and paint on the test platform; no GPU, compositor or display link), quiet reads held. `cpu_ms` is the UI thread's CPU time across the call, `wall_ms` its elapsed time; `frames` counts the repository columns that draw laid out. The frame callbacks before it and any work it leaves queued are delivered untimed.",
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
