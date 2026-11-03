//! Opt-in release benchmark of a Compare frame with Find open
//! ([`find_bar_frame_release_benchmark`]). The find bar is laid out on
//! Compare's render path, in the unified patch across its line numbers and
//! in each Split side, so a frame of a long comparison with Find open pays
//! for it; the record and its procedure are in
//! `docs/benchmarks/2026-11-03-compare-find-frame.md`.
//!
//! The driver uses only what the app had before the bar chose its forms from
//! its width (`8e7998ac`), so the same test module measures that base and a
//! candidate.

use crate::in_frame::probe;
use crate::views::tests::{draw, history_window_from};
use crate::*;
use core::prelude::v1::test;
use std::time::Instant;

/// The comparison's lines before the change.
const LINES: usize = 5_000;
/// The file the newest commit changes, and nothing else.
const PATH: &str = "turtle.txt";
/// Find's query, which every line of both sides matches.
const QUERY: &str = "turtl";
/// The key typed into the field, which keeps every line matching.
const KEY: &str = "e";
/// The window's height throughout.
const HEIGHT: f32 = 980.;
/// The width of the redraw and keystroke frames.
const FIXED: f32 = 1_480.;

/// The resize steps of one layout at one text size: each alternates between
/// its two widths. `wrap_boundary` straddles the width below which the find
/// bar, open on [`QUERY`], wraps from one line to two in the build these were
/// chosen in (`b9db23b8`, on the test platform's text system on the Asahi
/// development host), so the bar's plan changes on every step; `one_form`
/// keeps the bar's forms (every control its icon) and its two lines on both
/// of its widths, so only the field's width changes. The rest of Compare's
/// chrome is the same at both widths of each pair and of both pairs. They
/// were read from the plan the bar drew at each step of this driver's own
/// sequence: the repository columns rescale from their sizes before
/// (`ResizableState::fit_container`), so the content column's width at a
/// window width depends on the widths visited earlier, and Split's sides
/// with it. Each series records the code editor's top at its two widths,
/// which moves with the bar's lines.
struct Steps {
    layout: &'static str,
    text: u8,
    wrap_boundary: [f32; 2],
    one_form: [f32; 2],
}

const STEPS: [Steps; 4] = [
    Steps {
        layout: "unified",
        text: 13,
        wrap_boundary: [600., 596.],
        one_form: [592., 588.],
    },
    Steps {
        layout: "unified",
        text: 18,
        wrap_boundary: [823., 819.],
        one_form: [815., 811.],
    },
    Steps {
        layout: "split",
        text: 13,
        wrap_boundary: [930., 925.],
        one_form: [920., 915.],
    },
    Steps {
        layout: "split",
        text: 18,
        wrap_boundary: [1263., 1258.],
        one_form: [1250., 1245.],
    },
];

/// A `git fast-import` stream of two commits on `main`: the first writes
/// [`LINES`] numbered lines to [`PATH`], the newest rewrites every 37th,
/// adds a line after every 211th and removes every 293rd, so both the
/// unified patch and Split have changes all through the file.
fn fixture_script() -> String {
    let before: String = (1..=LINES)
        .map(|line| format!("{line:05} the turtle walks along the shore\n"))
        .collect();
    let after: String = (1..=LINES)
        .filter(|line| line % 293 != 0)
        .map(|line| {
            let mut text = if line % 37 == 0 {
                format!("{line:05} the turtle rests on the warm sand\n")
            } else {
                format!("{line:05} the turtle walks along the shore\n")
            };
            if line % 211 == 0 {
                text.push_str(&format!("{line:05}b the turtle digs a nest\n"));
            }
            text
        })
        .collect();
    let mut input = String::new();
    for (commit, (message, notes)) in [("Walk the shore", before), ("Rest and dig", after)]
        .into_iter()
        .enumerate()
    {
        input.push_str(&format!(
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\nM 100644 inline {PATH}\ndata {}\n{notes}\n",
            1_700_000_000 + commit,
            message.len(),
            notes.len(),
        ));
    }
    input + "done\n"
}

/// The frames that laid Compare's preview out since the last call.
fn compare_frames() -> usize {
    probe::take()
        .iter()
        .filter(|frame| frame.region == "compare-preview")
        .count()
}

/// One sample: `act` and every frame drawn as its effects flush, until the
/// executor parks, then, apart, the frame callbacks it left delivered and
/// any frame they draw.
struct Timed {
    us: f64,
    frames: usize,
    next_frame_us: f64,
    next_frame_frames: usize,
}

fn timed(cx: &mut VisualTestContext, act: impl FnOnce(&mut VisualTestContext)) -> Timed {
    compare_frames();
    let started = Instant::now();
    act(cx);
    cx.run_until_parked();
    let us = started.elapsed().as_secs_f64() * 1e6;
    let frames = compare_frames();
    let started = Instant::now();
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
    let next_frame_us = started.elapsed().as_secs_f64() * 1e6;
    Timed {
        us,
        frames,
        next_frame_us,
        next_frame_frames: compare_frames(),
    }
}

/// The samples of one series, after its untimed warm-ups.
#[derive(Default)]
struct Series {
    widths: Vec<f32>,
    us: Vec<f64>,
    next_frame_us: Vec<f64>,
    frames: Vec<usize>,
    next_frame_frames: Vec<usize>,
}

impl Series {
    fn push(&mut self, width: f32, sample: Timed) {
        self.widths.push(width);
        self.us.push(sample.us);
        self.next_frame_us.push(sample.next_frame_us);
        self.frames.push(sample.frames);
        self.next_frame_frames.push(sample.next_frame_frames);
    }

    fn json(self, labels: serde_json::Value) -> serde_json::Value {
        let mut series = labels;
        let fields = series.as_object_mut().expect("labels are an object");
        fields.insert("widths".into(), serde_json::json!(self.widths));
        fields.insert("us".into(), serde_json::json!(self.us));
        fields.insert(
            "next_frame_us".into(),
            serde_json::json!(self.next_frame_us),
        );
        fields.insert("frames".into(), serde_json::json!(self.frames));
        fields.insert(
            "next_frame_frames".into(),
            serde_json::json!(self.next_frame_frames),
        );
        series
    }
}

/// Shows the comparison in `mode` and returns the editor Find opens over:
/// the unified patch, or Split's first side.
fn show(
    app: &Entity<GitTurtle>,
    mode: TextMode,
    cx: &mut VisualTestContext,
) -> Entity<EditorState> {
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.text_mode = mode;
            app.ensure_editor(window, cx);
            cx.notify();
        })
    });
    draw(cx);
    cx.update(|_, cx| {
        let app = app.read(cx);
        match mode {
            TextMode::Split => app
                .split_view
                .as_ref()
                .expect("the split view")
                .read(cx)
                .editors()[0]
                .clone(),
            _ => app.patch_editor.clone().expect("the patch editor"),
        }
    })
}

/// Opens Find over `editor`, as Primary+F there does, and types [`QUERY`].
fn open_find(editor: &Entity<EditorState>, cx: &mut VisualTestContext) {
    let focus = cx.update(|_, cx| editor.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.dispatch_action(gpui_kit::component::input::Search);
    draw(cx);
    cx.simulate_input(QUERY);
    draw(cx);
}

/// Draws until the window rests, untimed: until no frame callback is left,
/// which a newly shown Split leaves for a dozen frames at 13 pt as its sides
/// first lay out, or for at most [`REST_FRAMES`] frames.
fn rest(cx: &mut VisualTestContext) {
    draw(cx);
    for _ in 0..REST_FRAMES {
        let callbacks = cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        if callbacks == 0 {
            break;
        }
    }
}

/// The most frames [`rest`] delivers.
const REST_FRAMES: usize = 100;

/// Resizes the window to `width` and draws until it rests, untimed.
fn rest_at(width: f32, cx: &mut VisualTestContext) {
    cx.simulate_resize(size(px(width), px(HEIGHT)));
    rest(cx);
}

/// Where the code editor last drawn starts, below the find bar where Find is
/// open, at `width`: the bar's lines show in it.
fn editor_top(width: f32, cx: &mut VisualTestContext) -> f32 {
    rest_at(width, cx);
    let bounds = cx
        .debug_bounds("code-editor")
        .expect("the code editor is drawn");
    f32::from(bounds.top())
}

/// The UI thread's handling of a Compare frame of a 5,000-line text file
/// changed all through, with Find open on a query every line matches and
/// without it, for a release build. Unified first, then Split with Find on
/// its first side, each at 13 and 18 pt, in a window [`HEIGHT`] tall. Each
/// series times, after its untimed warm-ups:
///
/// - `redraw`: the whole window redrawn at [`FIXED`] px (`Window::refresh`);
/// - `resize_wrap_boundary`: one `simulate_resize` step between two widths
///   either side of the find bar's one-line/wrap boundary ([`STEPS`]);
/// - `resize_one_form`: one step between two widths that keep the bar's
///   forms and lines;
/// - `keystroke`, with Find open: [`KEY`] typed into the field at [`FIXED`]
///   px, the query searched and the frame that shows it; it is erased again
///   untimed before the next sample.
///
/// Each sample is the action and every frame drawn as its effects flush
/// (layout, prepaint and paint on the test platform; no GPU, compositor or
/// display link), until the executor parks, in `us`. The frame callbacks it
/// leaves are delivered apart, timed in `next_frame_us`; the preview's frames
/// drawn in each are counted in `frames` and `next_frame_frames`. Fixture
/// setup, opening Compare and Find and every warm-up are untimed. Samples
/// per series come from `GITTURTLE_FIND_FRAME_BENCH_SAMPLES` (200 unless
/// set); a debug build runs only as a check of the driver, at ten or fewer.
#[gpui::test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_FIND_FRAME_BENCH_OUTPUT and run with --release --ignored"]
async fn find_bar_frame_release_benchmark(cx: &mut TestAppContext) {
    let output = std::env::var_os("GITTURTLE_FIND_FRAME_BENCH_OUTPUT")
        .expect("the runner supplies the result destination");
    let samples: usize = std::env::var("GITTURTLE_FIND_FRAME_BENCH_SAMPLES")
        .map(|count| count.parse().expect("a whole number of samples"))
        .unwrap_or(200);
    let release = !std::hint::black_box(cfg!(debug_assertions));
    assert!(samples > 0, "at least one sample");
    assert!(
        release || samples <= 10,
        "performance evidence requires --release; a debug build only checks the driver, at \
         ten samples or fewer"
    );
    let warmups = (samples / 10).max(1);
    let (_fixture, app, cx) = history_window_from(cx, fixture_script()).await;
    rest_at(FIXED, cx);
    let index = app.read_with(cx, |app, _| {
        app.files
            .iter()
            .position(|file| file.path() == std::path::Path::new(PATH))
            .expect("the newest commit changes the file")
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(index, window, cx)));
    crate::views::tests::settle(&app, cx).await;
    probe::watch();
    let mut series = Vec::new();
    for (layout, mode) in [("unified", TextMode::Unified), ("split", TextMode::Split)] {
        let editor = show(&app, mode, cx);
        for find in ["closed", "open"] {
            if find == "open" {
                rest_at(FIXED, cx);
                open_find(&editor, cx);
            }
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                rest_at(FIXED, cx);
                let labels = |case: &str| {
                    serde_json::json!({
                        "layout": layout,
                        "find": find,
                        "text_pt": text,
                        "case": case,
                    })
                };
                let mut redraw = Series::default();
                for sample in 0..warmups + samples {
                    let timed = timed(cx, |cx| cx.update(|window, _| window.refresh()));
                    if sample >= warmups {
                        redraw.push(FIXED, timed);
                    }
                }
                series.push(redraw.json(labels("redraw")));
                let steps = STEPS
                    .iter()
                    .find(|steps| steps.layout == layout && steps.text == text)
                    .expect("widths for each layout and text size");
                for (case, pair) in [
                    ("resize_wrap_boundary", steps.wrap_boundary),
                    ("resize_one_form", steps.one_form),
                ] {
                    let tops = pair.map(|width| editor_top(width, cx));
                    rest_at(pair[0], cx);
                    let mut resize = Series::default();
                    for sample in 0..warmups + samples {
                        let width = pair[(sample + 1) % 2];
                        let timed = timed(cx, |cx| {
                            cx.simulate_resize(size(px(width), px(HEIGHT)));
                        });
                        if sample >= warmups {
                            resize.push(width, timed);
                        }
                    }
                    let mut labelled = labels(case);
                    labelled["pair"] = serde_json::json!(pair);
                    labelled["code_editor_top"] = serde_json::json!(tops);
                    series.push(resize.json(labelled));
                }
                if find == "open" {
                    rest_at(FIXED, cx);
                    let mut keystroke = Series::default();
                    for sample in 0..warmups + samples {
                        let timed = timed(cx, |cx| cx.simulate_input(KEY));
                        cx.simulate_keystrokes("backspace");
                        rest(cx);
                        if sample >= warmups {
                            keystroke.push(FIXED, timed);
                        }
                    }
                    series.push(keystroke.json(labels("keystroke")));
                }
            }
            cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
            if find == "open" {
                cx.simulate_keystrokes("escape");
                draw(cx);
            }
        }
    }
    probe::stop();
    let result = serde_json::json!({
        "schema": 1,
        "profile": if release { "release" } else { "debug" },
        "samples": samples,
        "warmups": warmups,
        "window_height": HEIGHT,
        "fixed_width": FIXED,
        "fixture": {
            "path": PATH,
            "lines_before": LINES,
            "change": "every 37th line rewritten, a line added after every 211th, every 293rd removed",
        },
        "query": QUERY,
        "key": KEY,
        "boundary": "One action on the UI thread and every frame drawn as its effects flush (layout, prepaint and paint on the test platform; no GPU, compositor or display link), until the executor parks: `Window::refresh` (redraw), one `simulate_resize` (resize steps) or one key typed into the find field (keystroke). `next_frame_us` times delivering the frame callbacks the action left, and any frame they draw, separately. `frames` count the frames that laid Compare's preview out.",
        "series": series,
    });
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
