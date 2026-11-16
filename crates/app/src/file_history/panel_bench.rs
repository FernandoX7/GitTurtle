//! Opt-in release benchmark of File History's panel frames
//! ([`panel_frames_release_benchmark`]): a wheel notch over its revisions and a one-pixel
//! change of the window's width, each timed on the UI thread with the frame it draws.
//!
//! The driver uses only what the app has at `1abaa2de`, before File History decided its
//! header's, footer's and toolbar's forms ahead of layout, so the same module, picked onto a
//! later tree, measures one against the other. Build the test executable with
//! `cargo test --locked --release -p gitturtle --bin gitturtle --no-run` and run it pinned to
//! the performance cores, as `GITTURTLE_FILE_HISTORY_BENCH_OUTPUT=<file.json> <executable>
//! --ignored --exact file_history::panel_bench::panel_frames_release_benchmark
//! --test-threads 1`.

use crate::views::tests::{draw, history_script, history_window_from, hold_quiet_reads_for_test};
use crate::*;
use core::prelude::v1::test;
use std::time::Instant;

/// The fixture's commits, each rewriting the one file File History opens on.
const COMMITS: usize = 150;

/// Each surface's interface text size and window: a full window, and the two half tiles at
/// 18 pt, 461 px wide and 511 px wide, where the panel's toolbar and page controls change
/// form.
const SURFACES: [(u8, f32, f32); 3] = [(13, 1480., 980.), (18, 461., 490.), (18, 511., 490.)];

/// The revisions' list, which the wheel scrolls.
const LIST: &str = "file-revision-list";

/// The UI thread's frames of File History's panel, for a release build: the window on a
/// fixture of [`COMMITS`] commits that each rewrite `notes.txt`, in History with the newest
/// selected, File History open on that file with its first page of revisions landed (a
/// hundred, so the list scrolls at every surface) and quiet reads held so nothing it shows
/// changes, in Midnight with reduce motion, at each of [`SURFACES`].
///
/// Two series per surface. `wheel`: each timed frame dispatches one wheel notch
/// (`ScrollDelta::Lines(±3)`, as GPUI's X11 and Wayland backends deliver one) over the
/// visible middle of the revisions' list, down until the list reaches its foot and then up to
/// its top and so on, so every frame moves the list; the benchmark asserts that it did.
/// `width`: each timed frame resizes the window one pixel wider or back (`w`, `w + 1`, `w`,
/// …) at the surface's height. Each frame is one call on the UI thread, `cx.update` that
/// dispatches the wheel event or `simulate_resize`, from before the call to its return, which
/// includes the effects it flushes, drawing the window it dirtied (the test app's
/// `flush_effects`): the root render, the repository page with File History in its inspector
/// and the revision's comparison beside it, layout, prepaint and paint on the test platform,
/// with no GPU, compositor or display link. `cpu_ms` is the UI thread's CPU time
/// (`CLOCK_THREAD_CPUTIME_ID`) and `wall_ms` the elapsed time, each with its median, p95 and
/// p99 (nearest rank). Per frame, `draws` counts the repository columns laid out by the call
/// and the work it left queued, which the benchmark asserts is one in every step, warm-up
/// included, and `line_layouts` the lines the app's measured texts asked the text system to
/// lay out (`views::LINE_LAYOUTS`) over the same span.
///
/// Timed frames per series come from `GITTURTLE_FILE_HISTORY_BENCH_FRAMES` (300 unless set),
/// after `GITTURTLE_FILE_HISTORY_BENCH_WARMUP` untimed ones (30 unless set); a debug build
/// runs only as a check of the driver, at twenty timed frames or fewer.
/// `GITTURTLE_FILE_HISTORY_BENCH_SURFACES`, a comma-separated list of surface names
/// (`13pt-1480x980`, `18pt-461x490`, `18pt-511x490`), runs only those: at `1abaa2de` the
/// window at 18 pt in 511 × 490 never finishes its first draw, its panel's scroll cue asking
/// for another frame in every frame (fixed by `3d5e1e0f`), so a run there names the other
/// two.
#[gpui::test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_FILE_HISTORY_BENCH_OUTPUT and run with --release --ignored"]
async fn panel_frames_release_benchmark(cx: &mut TestAppContext) {
    let output = std::env::var_os("GITTURTLE_FILE_HISTORY_BENCH_OUTPUT")
        .expect("GITTURTLE_FILE_HISTORY_BENCH_OUTPUT names the result's destination");
    let count = |name: &str, default: usize| {
        std::env::var(name).map_or(default, |count| {
            count
                .parse()
                .unwrap_or_else(|_| panic!("{name} is a whole number of frames"))
        })
    };
    let frames = count("GITTURTLE_FILE_HISTORY_BENCH_FRAMES", 300);
    let warmup = count("GITTURTLE_FILE_HISTORY_BENCH_WARMUP", 30);
    let release = !std::hint::black_box(cfg!(debug_assertions));
    assert!(frames > 0, "at least one timed frame");
    assert!(
        release || frames <= 20,
        "performance evidence requires --release; a debug build only checks the driver, at \
         twenty frames or fewer"
    );
    let (commit, commit_from) = match std::env::var("GITTURTLE_BUILD_COMMIT") {
        Ok(commit) if !commit.is_empty() => (Some(commit), Some("GITTURTLE_BUILD_COMMIT")),
        _ if crate::build_info::REVISION != "unknown" => (
            Some(crate::build_info::REVISION.to_owned()),
            Some("build_info"),
        ),
        _ => (None, None),
    };
    let named = std::env::var("GITTURTLE_FILE_HISTORY_BENCH_SURFACES")
        .ok()
        .filter(|names| !names.trim().is_empty());
    let surfaces: Vec<(String, u8, f32, f32)> = SURFACES
        .into_iter()
        .map(|(text_pt, width, height)| {
            (
                format!("{text_pt}pt-{width}x{height}"),
                text_pt,
                width,
                height,
            )
        })
        .filter(|(surface, ..)| {
            named
                .as_deref()
                .is_none_or(|names| names.split(',').any(|name| name.trim() == surface))
        })
        .collect();
    assert!(
        !surfaces.is_empty(),
        "GITTURTLE_FILE_HISTORY_BENCH_SURFACES names one or more of 13pt-1480x980, \
         18pt-461x490 and 18pt-511x490"
    );
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
    let mean = |counts: &[usize]| counts.iter().sum::<usize>() as f64 / counts.len() as f64;
    let repository_frames = || {
        crate::in_frame::probe::take()
            .iter()
            .filter(|frame| frame.region == "repository-columns")
            .count()
    };
    let line_layouts = || crate::views::LINE_LAYOUTS.with(std::cell::Cell::get);

    let (_fixture, app, cx) = history_window_from(cx, history_script(COMMITS, 2)).await;
    cx.update(|_, cx| {
        crate::appearance::ThemeChoice::Midnight.apply(None, cx);
        cx.set_reduce_motion(true);
    });
    hold_quiet_reads_for_test(&app, cx);
    super::tests::open_landed(&app, cx).await;
    let revisions = app.read_with(cx, |app, _| {
        app.file_history
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.page.as_ref())
            .map_or(0, |page| page.entries.len())
    });
    assert!(revisions > 20, "File History lists {revisions} revisions");
    let scroll = app.read_with(cx, |app, _| {
        app.file_history.scroll.0.borrow().base_handle.clone()
    });
    // Back at the list's top, with nothing left to draw.
    let to_top = |cx: &mut VisualTestContext| {
        scroll.set_offset(point(px(0.), px(0.)));
        cx.update(|_, cx| app.update(cx, |_, cx| cx.notify()));
        draw(cx);
    };

    crate::in_frame::probe::watch();
    let mut series = Vec::new();
    for (surface, text_pt, width, height) in surfaces.iter().cloned() {
        cx.update(|window, cx| crate::appearance::apply_text_sizes(text_pt, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        to_top(cx);
        let at = format!("{text_pt} pt in {width} × {height}");
        assert!(
            app.read_with(cx, |app, _| app.file_history.is_active()),
            "{at}: File History stays open"
        );
        let list = cx.debug_bounds(LIST).expect("the revisions' list");
        let panel = cx
            .debug_bounds("file-history-panel")
            .expect("File History's panel");
        let viewport = cx.update(|window, _| window.viewport_size());
        let shown = list
            .intersect(&panel)
            .intersect(&Bounds::new(point(px(0.), px(0.)), viewport));
        assert!(
            shown.size.height > px(4.) && shown.size.width > px(4.),
            "{at}: part of the list {list:?} lies in view in the panel {panel:?}"
        );
        let step = cx.update(|window, _| window.line_height()) * 3.;
        let travel = scroll.max_offset().y;
        assert!(
            travel > step * 2.,
            "{at}: the list scrolls more than two wheel notches: {travel:?} for {step:?}"
        );

        // The wheel over the list, down to its foot and back.
        let position = shown.center();
        let mut down = true;
        let (mut cpu_ms, mut wall_ms, mut draws, mut layouts, mut offsets) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
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
            let laid = line_layouts();
            // The update's effects draw the window the scroll dirtied before it returns (the
            // test app's `flush_effects`).
            let (cpu, wall) = (thread_cpu_ms(), Instant::now());
            cx.update(|window, cx| {
                window.dispatch_event(event, cx);
            });
            let (cpu, wall) = (thread_cpu_ms() - cpu, wall.elapsed().as_secs_f64() * 1000.);
            let after = scroll.offset().y;
            cx.run_until_parked();
            let drawn = repository_frames();
            let laid = line_layouts() - laid;
            assert!(
                after != before,
                "{at}: wheel frame {frame}'s notch moves the list from {before:?}"
            );
            assert_eq!(drawn, 1, "{at}: wheel frame {frame} draws the window once");
            if frame >= warmup {
                cpu_ms.push(cpu);
                wall_ms.push(wall);
                draws.push(drawn);
                layouts.push(laid);
                offsets.push(f32::from(after));
            }
        }
        series.push(serde_json::json!({
            "surface": surface,
            "series": "wheel",
            "text_pt": text_pt,
            "window": [width, height],
            "frames": cpu_ms.len(),
            "cpu_ms_summary": summary(&cpu_ms),
            "wall_ms_summary": summary(&wall_ms),
            "mean_draws": mean(&draws),
            "mean_line_layouts": mean(&layouts),
            "list_px": [f32::from(list.size.width), f32::from(list.size.height)],
            "list_travel_px": f32::from(travel),
            "wheel_step_px": f32::from(step),
            "cpu_ms": cpu_ms,
            "wall_ms": wall_ms,
            "draws": draws,
            "line_layouts": layouts,
            "offset_px": offsets,
        }));

        // The window one pixel wider and back, from the list's top.
        to_top(cx);
        let (mut cpu_ms, mut wall_ms, mut draws, mut layouts, mut widths) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for frame in 0..warmup + frames {
            cx.update(|window, cx| window.simulate_next_frame(cx));
            cx.run_until_parked();
            repository_frames();
            let to = if frame.is_multiple_of(2) {
                width + 1.
            } else {
                width
            };
            let laid = line_layouts();
            // The resize's window update draws the window it dirtied before it returns.
            let (cpu, wall) = (thread_cpu_ms(), Instant::now());
            cx.simulate_resize(size(px(to), px(height)));
            let (cpu, wall) = (thread_cpu_ms() - cpu, wall.elapsed().as_secs_f64() * 1000.);
            cx.run_until_parked();
            let drawn = repository_frames();
            let laid = line_layouts() - laid;
            assert_eq!(
                cx.update(|window, _| window.viewport_size().width),
                px(to),
                "{at}: width frame {frame} resizes the window"
            );
            assert_eq!(drawn, 1, "{at}: width frame {frame} draws the window once");
            if frame >= warmup {
                cpu_ms.push(cpu);
                wall_ms.push(wall);
                draws.push(drawn);
                layouts.push(laid);
                widths.push(to);
            }
        }
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        series.push(serde_json::json!({
            "surface": surface,
            "series": "width",
            "text_pt": text_pt,
            "window": [width, height],
            "frames": cpu_ms.len(),
            "cpu_ms_summary": summary(&cpu_ms),
            "wall_ms_summary": summary(&wall_ms),
            "mean_draws": mean(&draws),
            "mean_line_layouts": mean(&layouts),
            "cpu_ms": cpu_ms,
            "wall_ms": wall_ms,
            "draws": draws,
            "line_layouts": layouts,
            "width_px": widths,
        }));
    }
    crate::in_frame::probe::stop();
    cx.update(|window, cx| {
        cx.set_reduce_motion(false);
        crate::appearance::apply_text_sizes(13, 12, window, cx);
    });
    let result = serde_json::json!({
        "schema": 1,
        "benchmark": "file_history::panel_bench::panel_frames_release_benchmark",
        "profile": if release { "release" } else { "debug" },
        "frames": frames,
        "warmup_frames": warmup,
        "surfaces": surfaces.iter().map(|(surface, ..)| surface).collect::<Vec<_>>(),
        "source": {
            "commit": commit,
            "from": commit_from,
            "build_revision": crate::build_info::REVISION,
            "build_tree": crate::build_info::TREE,
        },
        "fixture": {
            "commits": COMMITS,
            "path": "notes.txt",
            "revisions": revisions,
        },
        "boundary": "One call on the UI thread, from before it to its return: `cx.update` that dispatches a wheel notch (`ScrollDelta::Lines(±3)`) over File History's revisions, which moves them (`wheel`), or `simulate_resize` one pixel wider or back (`width`), with the effects it flushes, which draw the window it dirtied (the root render, the repository page with File History in its inspector and the revision's comparison beside it, layout, prepaint and paint on the test platform; no GPU, compositor or display link), quiet reads held, in Midnight with reduce motion. `cpu_ms` is the UI thread's CPU time across the call, `wall_ms` its elapsed time; `draws` counts the repository columns laid out by the call and the work it left queued, asserted one, and `line_layouts` the lines `views::LINE_LAYOUTS` counted over that span. The frame callbacks before it and the work it leaves queued run untimed.",
        "series": series,
    });
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
