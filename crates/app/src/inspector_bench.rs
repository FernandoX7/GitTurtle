//! Opt-in release benchmark of a frame of History with the commit inspector
//! drawn ([`commit_inspector_frame_release_benchmark`]), for a commit with
//! one parent and for an octopus merge with 128, whose metadata's row the
//! message list paints in every frame.
//!
//! The driver uses only what the app had before the inspector's message took
//! a focus reveal (`0551e16a`), so the same module, picked onto that base,
//! measures it against a candidate.

use crate::views::tests::{history_script, history_window_from, settle};
use crate::*;
use core::prelude::v1::test;
use std::time::Instant;

/// The window's size throughout, which holds the inspector whole.
const WINDOW: (f32, f32) = (1_480., 980.);
/// The most parents the metadata lists, and the octopus merge's count.
const PARENTS: usize = 128;

/// A `git fast-import` stream of [`PARENTS`] root commits and their octopus
/// merge, the newest commit on `main`.
fn octopus_script() -> String {
    let mut input = String::new();
    for parent in 1..=PARENTS {
        let message = format!("parent {parent}");
        input.push_str(&format!(
            "commit refs/heads/parent-{parent}\nmark :{parent}\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\nM 100644 inline parent-{parent}.txt\ndata 5\nnote\n\n",
            1_700_000_000 + parent,
            message.len(),
        ));
    }
    let message = "octopus merge";
    input.push_str(&format!(
        "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\nfrom :1\n",
        1_700_000_000 + PARENTS + 1,
        message.len(),
    ));
    for parent in 2..=PARENTS {
        input.push_str(&format!("merge :{parent}\n"));
    }
    input + "M 100644 inline merged.txt\ndata 5\nnote\n\ndone\n"
}

/// One sample: a redraw of the window and every frame it draws as its
/// effects flush, in microseconds.
fn frame_us(cx: &mut VisualTestContext) -> f64 {
    let started = Instant::now();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    started.elapsed().as_secs_f64() * 1e6
}

/// Times a redraw of History with the newest commit selected, for one parent
/// and for 128: render, layout, prepaint and paint on the test platform, no
/// GPU, compositor or display link, with nothing changed but the frame.
#[gpui::test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_INSPECTOR_FRAME_BENCH_OUTPUT and run with --release --ignored"]
async fn commit_inspector_frame_release_benchmark(cx: &mut TestAppContext) {
    let output = std::env::var_os("GITTURTLE_INSPECTOR_FRAME_BENCH_OUTPUT")
        .expect("the runner supplies the result destination");
    let samples: usize = std::env::var("GITTURTLE_INSPECTOR_FRAME_BENCH_SAMPLES")
        .map(|count| count.parse().expect("a whole number of samples"))
        .unwrap_or(300);
    let release = !std::hint::black_box(cfg!(debug_assertions));
    assert!(samples > 0, "at least one sample");
    assert!(
        release || samples <= 10,
        "performance evidence requires --release; a debug build only checks the driver, at \
         ten samples or fewer"
    );
    let warmups = (samples / 10).max(1);
    let mut series = Vec::new();
    for (parents, script) in [(1, history_script(2, 1)), (PARENTS, octopus_script())] {
        let (_fixture, app, window) = history_window_from(&mut *cx, &script).await;
        window.simulate_resize(size(px(WINDOW.0), px(WINDOW.1)));
        window.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, window).await;
        let drawn = app.read_with(window, |app, _| {
            app.selected_commit
                .and_then(|index| app.commits.get(index))
                .map(|commit| commit.parents.len())
        });
        assert_eq!(drawn, Some(parents), "the selected commit's parents");
        assert!(
            window.debug_bounds("commit-parent-0").is_some(),
            "the metadata's row is painted"
        );
        for _ in 0..warmups {
            frame_us(window);
        }
        let us: Vec<f64> = (0..samples).map(|_| frame_us(window)).collect();
        series.push(serde_json::json!({ "parents": parents, "us": us }));
    }
    let result = serde_json::json!({
        "schema": 1,
        "profile": if release { "release" } else { "debug" },
        "samples": samples,
        "warmups": warmups,
        "window": [WINDOW.0, WINDOW.1],
        "boundary": "One `Window::refresh` on the UI thread and every frame drawn as its effects flush, until the executor parks: render, layout, prepaint and paint of History with the newest commit selected and its metadata's row painted, on the test platform, with no GPU, compositor or display link. The whole window's frame, so a change to the inspector is the difference between builds, not a share of it.",
        "series": series,
    });
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
