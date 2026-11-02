# Columns laid out in the frame: scrolling, Compare redraws, width steps and Changes, Wayland, 2026-10-29

This is the release measurement for four commits integrated on `claude/omarchy-support`, `5f7b078c..5e128852`: `0639b0ec` (`fix(app): choose Compare's toolbar and footer in the frame they are drawn`), `5618236d` (`fix(app): lay Compare's columns out at a new width in its first frame`), `7e41fe4c` (tests only) and `5e128852` (`fix(app): decide the header and status bar from the body's laid-out size`). The base is `5f7b078c`, the parent of `0639b0ec`. Every frame now builds the whole Compare preview, text included, inside an `in_frame::InFrame` at the column's width (`views.rs` `render_preview`). It builds the content column and the inspector in an `InFrame` that first fits their panel group to its width (`ResizableState::fit_container`, a vendored `gpui-base` addition), and History's navigation and table the same way (`render_history_columns`). With the project pane shown, the pane and the page are built in the body's `InFrame`. One page width, `repository_width`, is derived each frame from `body_size`/`laid_out_size`, and the header and the status bar choose their forms from `body_size`. The questions:

1. Does a History frame cost more, or does History draw more frames, while scrolling with the wheel and with a held arrow key?
2. The same for Compare: a text diff, and an image comparison (pan drag, wheel, mode and zoom choices).
3. Width changes, the point of the change: how many frames does a width step draw, is the state it leaves the settled one, and what do those frames cost?
4. What does a Changes list redraw cost in absolute terms, in both builds? It is on record because an earlier change keys each working row by its path, allocated in every frame.

The data is in [`2026-10-29-columns-in-frame/`](2026-10-29-columns-in-frame/). Per-window rows are in `raw/windows.csv` and per-frame rows in `raw/frames.csv.gz`. The statistics are in `raw/summary.json` and `raw/summary.txt`, the tables below in `raw/tables.md`, the run log in `raw/runs.txt`, the load in `raw/load-per-launch.txt`, and the base designation and the fixed order in `raw/designation.txt`. The Changes cells have their own `raw/changes/`, and the scripts are in `tools/`. The per-launch Wayland logs, the samples and the settle-check screenshots stayed on the measuring host, because they hold local paths and are large. So did the fixture copies, the trial launches (`trial-*`) and the exploratory launches (`x/`).

## Result

- **(1) History scroll: no change, in cost or in frames.** A wheel notch draws one frame in both builds (1 in all 240 notches per build at 13 pt; 1, or 2 on 13 to 16 notches, at 18 pt). The input-led cost of that frame, from the axis event to the frame request, does not move. Base against candidate, p50: 4.98 / 4.93 ms at 1480 × 980, 13 pt, Δ −0.05 ms [−0.38, +0.39]. At 18 pt: 4.35 / 4.28, Δ −0.07 [−0.29, +0.15]. At 461 × 490, 13 pt: 3.06 / 3.04, Δ −0.02 [−0.27, +0.10]. At 18 pt: 2.84 / 2.81, Δ −0.03 [−0.19, +0.16]. Every Δp95 and Δp99 interval includes zero. The UI-thread CPU per notch changes by −0.06 to −0.01 ms, every interval spanning zero. Holding Down or Up for 1 s (key repeat at 40/s) draws 58 / 58, 61 / 63, 64 / 64 and 64 / 64 frames per window. Its paced draws cost 4.30 / 4.27, 3.68 / 3.61, 2.59 / 2.62 and 2.46 / 2.41 ms at p50, with every interval spanning zero (−0.07 to +0.03 ms at p50).
- **(2) Compare redraw: no resolved change.** Text diff, paced draws at p50: 3.46 / 3.48 ms (Δ +0.02 [−0.23, +0.37]) at 13 pt, 3.43 / 3.33 (Δ −0.09 [−0.38, +0.23]) at 18 pt, and 2.27 / 2.26 (Δ −0.01 [−0.12, +0.12]) at 461 × 490. A text wheel notch draws 1 frame, at 4.24 / 4.22 and 4.08 / 4.07 ms input-led. Image comparison, paced draws: 6.43 / 6.24 (Δ −0.19 [−0.74, +0.28]) and 5.40 / 5.25 (Δ −0.16 [−0.49, +0.18]). Pan drags at 200% draw 126 / 125 and 126 / 128 frames, and cost 157.5 / 159.2 and 140.8 / 140.1 ms of UI CPU. Wheel notches, zoom choices and mode choices draw the same frames in both builds. These intervals are 0.3 to 0.9 ms wide. They cannot resolve an effect the size of the 0.10–0.15 ms that the [image-compare record](2026-10-29-image-compare-redraw.md) found for building the image preview in an `InFrame`. The text preview's move into one is therefore shown to cost less than about 0.3 ms per frame, not shown to cost nothing.
- **(3) Width steps: the candidate always ends settled, and Compare pays one extra frame for it.** After each step, the driver compares the screen 1.5 s later with the screen after an input. That is the *settle check* described below.
  - **Compare at 1480 ↔ 1000, base: one frame per step, never settled.** The base draws exactly one frame per step and leaves the comparison's columns unsettled on all 32 Compare steps per text size. The next input then redraws them at other widths: 417,595 or 46,020 device pixels change, depending on the direction, on the text diff at 13 pt, and 289,897 or 450,526 on the image comparison.
  - **Compare at 1480 ↔ 1000, candidate: two frames per step, always settled.** The candidate draws 2 frames on every step, and the input changes nothing on any of its 32 steps.
  - **The cost of the second frame.** It comes one or two ticks after the first, which follows the configure (last frame 37.6–46.2 ms after the command at p50, against 20.4–31.9 ms). It costs 3.6–5.3 ms at p50. Compare's UI CPU per step rises by +0.6 to +7.2 ms at p50, positive in all eight Compare rows at 1480 px with every interval including zero (for example, image 1480 → 1000 at 13 pt: 17.3 → 23.1 ms, +5.8 [−1.9, +11.3]).
  - **History at 1480 ↔ 1000: fewer frames.** The base draws 3 frames narrowing and 4 widening at 13 pt, 3 and 3 at 18 pt. The candidate draws 2 on every step. Both are settled. The candidate's last frame comes 14–38 ms sooner, and its UI CPU per step falls: −14.2 ms [−18.0, −4.6] widening at 13 pt, the other three History rows −5 to −10 ms with intervals including zero.
  - **461 ↔ 980 (tiled, at the app's default inspector width).** History draws 2 frames per step in both builds, and the image comparison 2 in both. Text Compare narrowing to 461 draws 3 frames in the base and 2 in the candidate. Every narrow step ends settled in both builds.
  - **First frame unobserved.** Whether the candidate's *first* frame at a new width is already the settled one is not observable with this method. The second frame comes from GPUI's paced frame loop (a `wl_callback.done`, then a draw), so something marked the window dirty during or after the first draw.
- **(4) Changes list redraw, absolute.** 600 changed files at 1480 × 980, Stage/Unstage writes. Paced draws in the write windows, base / candidate, p50 / p95 / p99: 2.98 / 5.31 / 8.09 against 2.88 / 5.15 / 6.92 ms at 13 pt, and 2.59 / 4.84 / 7.13 against 2.53 / 4.68 / 7.26 ms at 18 pt. The deltas: Δp50 −0.10 [−0.21, +0.06] and −0.07 [−0.18, +0.05]. Each write draws 25 (23–26) frames at 13 pt, and 25 or 26 (24–27) at 18 pt. Its UI CPU is 86.3 / 86.7 ms at 13 pt and 76.6 / 78.2 ms at 18 pt. These are two launches per build and size, so they are figures on record, not a graded comparison.

## Verdict

No regression in per-frame cost. History scrolling, by wheel and by held keys, Compare's text and image redraws, and the Changes list cost the same per frame and draw the same frames in both builds, within intervals of about ±0.2–0.5 ms at p50. Noise exceeds any difference at that scale. The 0.1 ms effects that an earlier record resolved are below this run's resolution.

On width steps the change does what it claims for the end state. Every candidate step leaves the screen in its settled state, while the base left every Compare step at 1480 ↔ 1000 unsettled until the next input. The candidate also removes one or two frames from each History step. It adds one frame to every Compare step at 1480 ↔ 1000: one more draw of 3.6–5.3 ms, and a UI CPU rise per step that points up in every row without resolving (+0.6 to +7.2 ms). The cost is small and buys a correct screen, but the second frame may be redundant. If the candidate's first frame is already the settled one, as the commit's tests assert of the layout (`a_resize_lays_each_panel_group_out_where_it_settles`), the second frame redraws the same thing.

Likely cause of the extra frame, inferred from the diff and not traced. `fit_container` (vendor `gpui-base/src/resizable/mod.rs`) calls `adjust_to_container_size`, which ends in `cx.notify()`. The candidate calls it from inside each `InFrame` build closure, during prepaint, for the content columns (`render_repository`), History's columns and the workspace columns. A notify on the panel state while the window draws would leave the window dirty for the next tick. That fits the evidence: one paced frame one or two ticks after the configure frame, on every step and every surface, whereas the base drew Compare steps in one frame. The smallest fix to try is a `fit_container` that rescales the sizes without notifying: split the arithmetic out of `adjust_to_container_size` and notify only from the group's own post-layout path. The caller builds the group from those sizes in the same frame. Then rerun this protocol's width steps and check two things: one frame per Compare step, and a settle-check difference of 0. If the second frame remains, trace which entity invalidated the window. Product source was not changed.

## Setup

- **Host:** Apple M2 Pro (CPUs 0–3 efficiency at 2.42 GHz maximum, 4–11 performance at 3.26 GHz maximum), 31 GiB, Asahi Linux (Omarchy) on Linux 7.1.12-2-7-ARCH, `schedutil` governor, Hyprland 0.56.2, native Wayland. Display: built-in eDP-1, 3456 × 2160 at 120 Hz, scale 2, so one frame tick is 8.33 ms. Mesa 26.2.2 (`vulkan-asahi`), libwayland 1.26.0, Git 2.55.0. Keyboard repeat from the seat: 40/s after 250 ms.
- **Quiet host and load:** a build hold (`.local/build-slots/quiet`) was in place. Before every launch the run waited until `pgrep -x rustc` and `pgrep cargo` found nothing, and recorded `builds 0` for all 40 launches (`raw/load-per-launch.txt`). AT-SPI was off, `IsEnabled false`, read before every launch, and notifications were silenced. The IDE hosting the session (`orca-ide`) used about one core throughout. Seven orphaned `fc-match` test stubs, left by earlier test runs, also ran throughout: each is a shell loop polling a file every 5 ms at about 1.5% of a core, on CPUs that included 4–11. One-minute load average: 2.16–3.64 at launch starts, 2.16–4.67 at ends. Cores 4–11 were 75–87% idle (median 86%) over the 2 s before each launch. Window-start frequency of the UI thread's CPU: 2.40 GHz at p50 in both builds, below 3.0 GHz in 1,120 of 1,592 base windows and 1,083 of 1,592 candidate windows. The run ran from 02:14:02 to 03:35:44 UTC on 2026-10-30 (the evening of 2026-10-29 locally).
- **Builds:** release executables from clean trees (`--build-info`), each embedding its own full revision and not the other's (`strings <exe> | grep <sha>`). The base was designated in writing before the first measured launch:
  - Base `5f7b078ca1b0281ffac42a93449563616556f02d`, `source_tree` clean, aarch64-unknown-linux-gnu, release, rustc 1.99.0 (b940084d7 2026-09-28), build_unix_seconds 1793319492, sha256 `4d95805f538b62efd57f3c20ab9f49e89e5f118f1096899864aa2e52e2f3a362`.
  - Candidate `5e128852bd46e79fd0f6b7fbd91259cdca40f887`, `source_tree` clean, the same target, profile and rustc, build_unix_seconds 1793324508, sha256 `e581ae28a6462695c756a14b488d824bd01d6b8cb7bb9ac8aaa2959af11a1114`.
- **Fixtures** (`tools/make-fixtures.sh`: deterministic, disposable, a fresh copy of the pristine repository per launch, no hooks, remotes or signing):
  - *cols*: the 3,000-commit, 121-reference History fixture of the [October 5 record](2026-10-05-history-columns-steady-after-toggle.md) (`make_history_fixture.py`), then three commits. "Add photo and long text" adds `photo.png`, 1600 × 1000, and `long.txt`, 3,000 lines. "Change photo" (HEAD~1, `7d2a777`) changes `photo.png`. "Edit long text" (HEAD, `27503ee`) edits 60 lines of `long.txt`, which makes 60 hunks.
  - *many*: the Changes fixture of the [Changes record](2026-10-29-changes-body-redraw.md), 600 tracked files all edited and none staged (601 rows).
- **Cells:** *1480 × 980 at 13 pt* and *at 18 pt*: floating, centred, forced opaque, on workspace 9. *461 × 490 at 13 pt* and *at 18 pt*: tiled beside one `foot` filler and above another, because tiling ignores the app's 1000 × 680 minimum. At 461 × 490, 13 pt, only History was run. Changes ran at 1480 × 980, 13 and 18 pt.
- **Launch** (`tools/common.py`, from the image-compare record): `taskset -c 4-11`, `NO_AT_BRIDGE=1`, `WAYLAND_DEBUG=1`, and empty absolute `XDG_*` homes per launch. Fontconfig is linked, and preferences come from the native-QA template: Midnight, follow-system off, interface text at 13 or 18 pt, project pane off. The launch homes were deleted afterwards.
- **Cache state:** warm. Each launch reads a fixture copy made just before it, and the executables had been run before. Every launch is a new process, so its preview cache starts empty.
- **Order:** fixed in writing before the first measured launch (`raw/designation.txt`). Four rounds. Each round runs the cells 1480 × 980 at 13 pt, 1480 × 980 at 18 pt, 461 × 490 at 13 pt and 461 × 490 at 18 pt with both builds: base first in rounds 1 and 3, candidate first in rounds 2 and 4. That gives 4 launches per build and cell, 32 in all. In rounds 1 and 2, the Changes cells followed in the same order: 2 launches per build and size, 8 in all. No launch failed, was dropped or was repeated. Four trial launches (`trial-*`) and exploratory launches checked the driver and are not results.

## Method and boundaries

Per launch (`tools/drive.py`). The accessibility bus stayed off, so Compare is entered with the keyboard: Enter twice from History, Escape twice to go back. Buttons are found by OCR, `tesseract` on a screenshot run on CPUs 0–3 just before each click and outside every measured window.

- **History, HEAD selected:**
  - Idle for 2 s.
  - 10 unrecorded wheel notches over a History row, then 60 recorded notches (30 down, 30 up), one per 150 ms window, as in [`history_scroll_wayland.py`](2026-10-27-compact-chrome-history-scroll.md).
  - Down held for 1 s, then 1 s more; Up the same; twice each. Then Home.
  - Four width steps.
- **Text Compare** on `long.txt` (not at 461 × 490, 13 pt):
  - 20 notches (10 down, 10 up) in 0.5 s windows.
  - At 1480 px: Split, then Diff.
  - Four width steps.
- **Image Compare** on `photo.png`:
  - At 1480 px: zoom 100%, then 200%; four pan drags at 200% (a 160 px stroke through the Before side, as in the image-compare record); six wheel notches in 1.0 s windows; Overlay, one drag in Overlay, then Side by side.
  - Four width steps.
  - At 461 × 490 the toolbars are menus and the stage is small, so only the width steps were run there.
- **Idle** for 2 s.

A width step goes 1480 → 1000 → 1480… or 461 → 980 → 461…. Each step parks the pointer on the status bar, samples, resizes, waits 1.5 s and samples again. Each step got exactly one `xdg_toplevel.configure`. The **settle check** runs after each step, outside its window. It takes screenshot A. The pointer then visits the Menu button for 0.6 s (a hover, which is an input that redraws) and returns to the status bar for 0.8 s, and the check takes screenshot B. It counts device pixels that differ by more than 24 grey levels, masking the pointer. The compositor draws the pointer into the screenshots, and it can sit in another window-relative place after a floating resize. A step whose count is 0 is *final without further input*. The check shows the state the step left on screen, not the content of each frame drawn during it.

Neither build prints a `GITTURTLE_TRACE` metric on these paths. Frames come from the app's own `WAYLAND_DEBUG` log, and CPU from `/proc/<pid>/task/<pid>/schedstat` (`tools/analyse.py`, `tools/summary.py`, `tools/tables.py`; Changes: `tools/changes-drive.py`, the [Changes record](2026-10-29-changes-body-redraw.md)'s `analyse.py` and `tools/changes-summary.py`). The boundaries:

- **Frame:** one `wl_surface.attach` on the window's main surface: Mesa's present of one GPUI frame. A frame whose request follows its `done` within 0.5 ms is a present without a draw.
- **Paced draw** (`gap_ms` of `paced` frames): from libwayland's dispatch of a `wl_callback.done` on the main surface to the app's next `wl_surface.frame` request on it, when that request comes 0.5 ms to one 8.33 ms tick after the `done`.
  - *What it covers:* GPUI's frame handling, including next-frame callbacks and the whole window's request_layout, layout, prepaint (with the `InFrame` builds and fits) and paint, plus the debug print of the `done` line.
  - *What it excludes:* renderer encoding, submission and present, GPU execution, compositing, input delivery, and the wait for the tick.
  - *Tick grid:* the value starts at a tick, so the grid puts no floor under it. The ceiling is one tick by construction, two ticks in the view "with 1–2-tick draws". A draw longer than two ticks cannot be seen.
- **Input-led draw and input to first frame request:** from libwayland's dispatch of the input event to the first frame request on the main surface. The input event is the app's own pointer press, its first `wl_pointer` axis event (wheel), a `wl_keyboard.key` press (a held key's repeats arrive from the seat as presses), or `xdg_toplevel.configure` (width steps).
  - *What it covers:* the handler and the whole draw. GPUI draws for an input without waiting for a frame callback, so where no frame loop is running there is no tick floor.
  - *Wheel notches:* in 1,893 of 1,920 History notches and all 96 image notches the first frame followed the axis event with no `done` in between. In 27 History notches and 104 of 480 text notches a frame loop was already running, a scroll or scrollbar animation, and the frame waited for the next `done`. Those values include up to one tick of waiting.
  - *Width steps:* the first frame followed the configure directly in 319 of 320 steps. The exception is one text step at 461 × 490 that began inside a running scroll animation.
- **Last frame after the command** (width steps): from the driver's sample before `hyprctl`'s resize to the last attach in the window. It includes the compositor's handling of the command. The candidate's second frame follows a `done`, so its value cannot fall below the configure frame plus the wait for the next tick.
- **Frames per interaction:** main-surface attaches in the window. A window starts at the app's own first press, axis or key event, or at the driver's sample for width steps and idle. It ends at the driver's end sample.
- **UI CPU per interaction:** the main thread's schedstat run time between the driver's two samples. It includes every frame drawn, worker results handled on that thread and the libwayland debug formatting. Idle windows read 0.05–0.15 ms over 2 s and drew no frames, in every launch of both builds.

Statistics: pooled nearest-rank percentiles. Deltas are candidate minus base, with a two-level bootstrap: launches with replacement within each build, then samples within each drawn launch; 4,000 resamples, seed 1; the interval runs from the 2.5th to the 97.5th percentile.

## Tables

### Per-frame draw by surface

| Cell | Surface | Build | Paced draws n | Paced p50 / p95 / max (ms) | Launch p50s | With 1–2-tick draws p50 / p95 / p99 / max (ms) | Over one tick | Input-led draws n | Input-led p50 / p95 / p99 / max (ms) |
| --- | --- | --- | ---: | --- | --- | --- | ---: | ---: | --- |
| 1480 × 980, 13 pt | History | `5f7b078` | 600 | 4.299 / 6.523 / 8.002 | 4.18–4.43 | 4.321 / 6.619 / 7.680 / 10.249 | 3 | 350 | 4.006 / 6.406 / 8.633 / 12.205 |
|  |  | `5e12885` | 563 | 4.271 / 6.481 / 8.089 | 3.92–4.44 | 4.277 / 6.510 / 7.948 / 9.707 | 3 | 367 | 4.030 / 6.148 / 8.682 / 10.502 |
|  | Text Compare | `5f7b078` | 248 | 3.463 / 5.644 / 7.330 | 3.37–3.56 | 3.473 / 5.654 / 6.741 / 8.658 | 1 | 8 | 4.007 / 5.929 / 5.929 / 5.929 |
|  |  | `5e12885` | 261 | 3.481 / 5.740 / 7.493 | 3.36–3.90 | 3.482 / 5.790 / 7.493 / 10.632 | 2 | 8 | 3.942 / 6.519 / 6.519 / 6.519 |
|  | Image Compare | `5f7b078` | 253 | 6.431 / 8.117 / 8.332 | 6.12–6.76 | 6.999 / 10.531 / 11.738 / 12.742 | 83 | 36 | 7.018 / 10.733 / 14.504 / 14.504 |
|  |  | `5e12885` | 285 | 6.240 / 8.081 / 8.324 | 5.88–6.38 | 6.833 / 10.313 / 12.294 / 12.978 | 77 | 36 | 7.047 / 13.146 / 13.990 / 13.990 |
| 1480 × 980, 18 pt | History | `5f7b078` | 563 | 3.681 / 5.833 / 7.305 | 3.62–3.77 | 3.689 / 5.900 / 7.129 / 10.141 | 2 | 434 | 3.458 / 5.472 / 6.818 / 10.717 |
|  |  | `5e12885` | 542 | 3.614 / 5.978 / 8.017 | 3.38–3.77 | 3.622 / 6.037 / 7.604 / 9.126 | 3 | 451 | 3.428 / 5.252 / 7.001 / 13.821 |
|  | Text Compare | `5f7b078` | 248 | 3.428 / 5.899 / 8.304 | 3.15–3.74 | 3.428 / 5.899 / 7.377 / 8.304 | 0 | 8 | 4.178 / 5.690 / 5.690 / 5.690 |
|  |  | `5e12885` | 264 | 3.334 / 5.102 / 7.731 | 3.17–3.48 | 3.337 / 5.164 / 7.518 / 8.621 | 1 | 8 | 3.823 / 5.684 / 5.684 / 5.684 |
|  | Image Compare | `5f7b078` | 346 | 5.402 / 7.584 / 8.258 | 5.14–5.75 | 5.448 / 8.955 / 10.684 / 11.955 | 21 | 36 | 5.864 / 8.277 / 8.447 / 8.447 |
|  |  | `5e12885` | 354 | 5.246 / 7.474 / 8.142 | 5.13–5.31 | 5.356 / 9.065 / 10.929 / 12.613 | 28 | 36 | 5.326 / 8.920 / 10.490 / 10.490 |
| 461 × 490, 13 pt | History | `5f7b078` | 512 | 2.594 / 4.408 / 8.029 | 2.56–2.63 | 2.594 / 4.408 / 5.903 / 8.029 | 0 | 512 | 2.515 / 3.699 / 4.851 / 9.077 |
|  |  | `5e12885` | 512 | 2.623 / 4.419 / 6.175 | 2.59–2.62 | 2.623 / 4.419 / 5.798 / 6.175 | 0 | 512 | 2.517 / 3.993 / 5.390 / 6.375 |
| 461 × 490, 18 pt | History | `5f7b078` | 520 | 2.464 / 4.173 / 7.599 | 2.40–2.56 | 2.464 / 4.173 / 5.478 / 7.599 | 0 | 512 | 2.363 / 3.618 / 5.014 / 6.496 |
|  |  | `5e12885` | 519 | 2.409 / 4.195 / 7.235 | 2.38–2.44 | 2.409 / 4.195 / 5.399 / 7.235 | 0 | 512 | 2.296 / 3.672 / 4.947 / 5.949 |
|  | Text Compare | `5f7b078` | 435 | 2.274 / 3.662 / 6.279 | 2.21–2.42 | 2.274 / 3.696 / 4.745 / 8.503 | 1 | 0 | — / — / — / — |
|  |  | `5e12885` | 428 | 2.261 / 3.685 / 7.822 | 2.23–2.32 | 2.261 / 3.685 / 4.965 / 7.822 | 0 | 0 | — / — / — / — |
|  | Image Compare | `5f7b078` | 16 | 3.104 / 3.685 / 3.685 | 2.80–3.14 | 3.104 / 3.685 / 3.685 / 3.685 | 0 | 0 | — / — / — / — |
|  |  | `5e12885` | 16 | 3.036 / 4.226 / 4.226 | 2.38–3.43 | 3.036 / 4.226 / 4.226 / 4.226 | 0 | 0 | — / — / — / — |

### Candidate minus base, per-frame draw

| Cell | Surface | Paced Δp50 (ms) | Δp95, with 1–2-tick draws | Δp99, with 1–2-tick draws | Input-led Δp50 | Input-led Δp95 | Input-led Δp99 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1480 × 980, 13 pt | History | -0.028 [-0.336, +0.197] | -0.109 [-0.690, +0.591] | +0.268 [-1.335, +1.671] | +0.024 [-0.200, +0.188] | -0.258 [-1.128, +0.535] | +0.049 [-4.527, +2.316] |
| 1480 × 980, 13 pt | Text Compare | +0.018 [-0.232, +0.368] | +0.136 [-0.782, +0.899] | +0.752 [-1.538, +4.252] | -0.065 [-1.906, +2.014] | +0.590 [-1.485, +1.778] | +0.590 [-1.485, +1.778] |
| 1480 × 980, 13 pt | Image Compare | -0.191 [-0.737, +0.275] | -0.218 [-1.253, +0.951] | +0.556 [-1.411, +1.645] | +0.029 [-1.144, +1.685] | +2.413 [-5.678, +4.792] | -0.514 [-3.963, +4.766] |
| 1480 × 980, 18 pt | History | -0.067 [-0.273, +0.113] | +0.137 [-0.664, +1.075] | +0.475 [-2.184, +1.790] | -0.030 [-0.219, +0.150] | -0.220 [-0.725, +0.538] | +0.183 [-1.740, +1.273] |
| 1480 × 980, 18 pt | Text Compare | -0.094 [-0.377, +0.233] | -0.735 [-1.641, +1.323] | +0.141 [-1.939, +2.022] | -0.355 [-1.442, +1.835] | -0.006 [-0.902, +1.455] | -0.006 [-0.902, +1.425] |
| 1480 × 980, 18 pt | Image Compare | -0.156 [-0.493, +0.180] | +0.110 [-1.588, +1.929] | +0.245 [-2.132, +2.276] | -0.538 [-1.199, +0.371] | +0.643 [-1.493, +3.363] | +2.043 [-0.787, +3.363] |
| 461 × 490, 13 pt | History | +0.029 [-0.054, +0.102] | +0.011 [-0.713, +0.613] | -0.105 [-1.224, +0.389] | +0.002 [-0.182, +0.145] | +0.294 [-0.382, +0.982] | +0.539 [-1.109, +1.752] |
| 461 × 490, 18 pt | History | -0.055 [-0.155, +0.030] | +0.022 [-0.779, +0.695] | -0.079 [-1.156, +0.883] | -0.067 [-0.170, +0.027] | +0.054 [-0.453, +0.550] | -0.067 [-1.415, +1.029] |
| 461 × 490, 18 pt | Text Compare | -0.013 [-0.124, +0.118] | -0.011 [-0.798, +0.476] | +0.220 [-2.372, +1.532] | — | — | — |
| 461 × 490, 18 pt | Image Compare | -0.068 [-0.664, +0.630] | +0.541 [-0.198, +0.877] | +0.541 [-0.198, +0.877] | — | — | — |

### Frames, draws and UI-thread CPU per interaction

| Cell | Interaction | Build | n | Frames p50 / p95 (min–max) | UI CPU p50 / p95 / max (ms) | Input to first frame request p50 / p95 / p99 / max (ms) | Δ UI CPU p50 (ms) | Δ input to first request p50 (ms) |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- |
| 1480 × 980, 13 pt | History wheel notch | `5f7b078` | 240 | 1 / 1 (1–1) | 6.0 / 9.5 / 13.3 | 4.98 / 8.12 / 9.39 / 11.93 |  |  |
|  |  | `5e12885` | 240 | 1 / 1 (1–1) | 6.0 / 10.1 / 15.8 | 4.93 / 8.43 / 12.15 / 13.50 | -0.1 [-0.4, +0.5] | -0.05 [-0.38, +0.39] |
|  | History held arrow, 1 s | `5f7b078` | 16 | 58 / 63 (53–63) | 309.9 / 325.0 / 325.0 | 5.33 / 12.21 / 12.21 / 12.21 |  |  |
|  |  | `5e12885` | 16 | 58 / 62 (54–62) | 306.4 / 318.2 / 318.2 | 5.13 / 10.50 / 10.50 / 10.50 | -3.5 [-15.4, +17.5] | -0.20 [-2.69, +2.74] |
|  | Text wheel notch | `5f7b078` | 80 | 1 / 2 (1–31) | 5.3 / 14.0 / 130.2 | 4.24 / 14.84 / 19.77 / 19.77 |  |  |
|  |  | `5e12885` | 80 | 1 / 2 (1–29) | 5.2 / 18.2 / 117.5 | 4.22 / 14.52 / 17.76 / 17.76 | -0.2 [-1.0, +0.6] | -0.01 [-0.30, +0.26] |
|  | Text mode (Split, Diff) | `5f7b078` | 8 | 2 / 33 (2–33) | 16.4 / 154.8 / 154.8 | 4.01 / 5.93 / 5.93 / 5.93 |  |  |
|  |  | `5e12885` | 8 | 2 / 32 (2–32) | 16.0 / 143.8 / 143.8 | 3.94 / 6.52 / 6.52 / 6.52 | -0.4 [-125.1, +127.2] | -0.07 [-1.91, +2.01] |
|  | Image zoom (100%, 200%) | `5f7b078` | 8 | 2 / 15 (2–15) | 20.7 / 106.6 / 106.6 | 6.50 / 10.73 / 10.73 / 10.73 |  |  |
|  |  | `5e12885` | 8 | 2 / 14 (2–14) | 16.0 / 116.1 / 116.1 | 7.04 / 8.32 / 8.32 / 8.32 | -4.7 [-89.9, +93.3] | +0.54 [-2.19, +1.15] |
|  | Image pan drag at 200% | `5f7b078` | 16 | 126 / 133 (106–133) | 157.5 / 179.9 / 179.9 | 6.51 / 14.50 / 14.50 / 14.50 |  |  |
|  |  | `5e12885` | 16 | 125 / 133 (120–133) | 159.2 / 169.5 / 169.5 | 6.77 / 13.99 / 13.99 / 13.99 | +1.7 [-8.9, +8.2] | +0.26 [-1.85, +2.09] |
|  | Image wheel notch at 200% | `5f7b078` | 24 | 1 / 1 (1–1) | 8.5 / 12.1 / 14.6 | 7.88 / 11.10 / 14.03 / 14.03 |  |  |
|  |  | `5e12885` | 24 | 1 / 1 (1–1) | 8.4 / 14.6 / 15.6 | 7.75 / 13.55 / 15.06 / 15.06 | -0.2 [-1.8, +2.5] | -0.12 [-1.64, +2.28] |
|  | Image mode (Overlay, Side by side) | `5f7b078` | 8 | 2 / 2 (2–2) | 16.1 / 20.1 / 20.1 | 7.78 / 10.46 / 10.46 / 10.46 |  |  |
|  |  | `5e12885` | 8 | 2 / 2 (2–2) | 16.8 / 20.4 / 20.4 | 7.27 / 11.23 / 11.23 / 11.23 | +0.6 [-3.4, +4.5] | -0.51 [-2.55, +2.77] |
|  | Image pan drag, Overlay | `5f7b078` | 4 | 122 / 127 (121–127) | 157.3 / 168.4 / 168.4 | 7.86 / 9.22 / 9.22 / 9.22 |  |  |
|  |  | `5e12885` | 4 | 118 / 127 (107–127) | 155.2 / 169.0 / 169.0 | 8.27 / 10.54 / 10.54 / 10.54 | -2.2 [-13.8, +11.7] | +0.41 [-0.95, +2.68] |
| 1480 × 980, 18 pt | History wheel notch | `5f7b078` | 240 | 1 / 2 (1–2) | 5.3 / 10.3 / 12.9 | 4.35 / 8.68 / 17.27 / 19.09 |  |  |
|  |  | `5e12885` | 240 | 1 / 2 (1–2) | 5.3 / 10.0 / 17.6 | 4.28 / 7.17 / 16.10 / 17.34 | -0.0 [-0.4, +0.4] | -0.07 [-0.29, +0.15] |
|  | History held arrow, 1 s | `5f7b078` | 16 | 61 / 64 (56–64) | 271.6 / 312.8 / 312.8 | 3.92 / 10.72 / 10.72 / 10.72 |  |  |
|  |  | `5e12885` | 16 | 63 / 64 (58–64) | 276.7 / 305.3 / 305.3 | 3.96 / 13.82 / 13.82 / 13.82 | +5.0 [-6.5, +13.8] | +0.04 [-1.76, +0.88] |
|  | Text wheel notch | `5f7b078` | 80 | 1 / 2 (1–29) | 5.5 / 15.4 / 128.3 | 4.08 / 15.78 / 17.70 / 17.70 |  |  |
|  |  | `5e12885` | 80 | 1 / 2 (1–32) | 5.1 / 13.7 / 117.9 | 4.07 / 14.10 / 16.75 / 16.75 | -0.4 [-1.6, +1.2] | -0.01 [-0.50, +0.65] |
|  | Text mode (Split, Diff) | `5f7b078` | 8 | 2 / 33 (2–33) | 14.0 / 132.0 / 132.0 | 4.18 / 5.69 / 5.69 / 5.69 |  |  |
|  |  | `5e12885` | 8 | 2 / 32 (2–32) | 14.3 / 133.6 / 133.6 | 3.82 / 5.68 / 5.68 / 5.68 | +0.3 [-117.3, +119.0] | -0.35 [-1.44, +1.89] |
|  | Image zoom (100%, 200%) | `5f7b078` | 8 | 2 / 16 (2–16) | 14.1 / 98.9 / 98.9 | 5.53 / 8.11 / 8.11 / 8.11 |  |  |
|  |  | `5e12885` | 8 | 2 / 15 (2–15) | 17.8 / 102.2 / 102.2 | 5.09 / 10.49 / 10.49 / 10.49 | +3.7 [-79.8, +78.3] | -0.44 [-2.04, +1.22] |
|  | Image pan drag at 200% | `5f7b078` | 16 | 126 / 132 (115–132) | 140.8 / 153.5 / 153.5 | 5.97 / 8.45 / 8.45 / 8.45 |  |  |
|  |  | `5e12885` | 16 | 128 / 134 (110–134) | 140.1 / 151.1 / 151.1 | 5.41 / 8.92 / 8.92 / 8.92 | -0.7 [-10.6, +5.6] | -0.56 [-1.28, +1.95] |
|  | Image wheel notch at 200% | `5f7b078` | 24 | 1 / 1 (1–1) | 7.3 / 14.1 / 14.5 | 6.52 / 11.40 / 12.93 / 12.93 |  |  |
|  |  | `5e12885` | 24 | 1 / 1 (1–1) | 6.8 / 12.0 / 12.2 | 6.23 / 11.04 / 11.34 / 11.34 | -0.5 [-1.8, +1.5] | -0.28 [-1.80, +1.41] |
|  | Image mode (Overlay, Side by side) | `5f7b078` | 8 | 2 / 2 (2–2) | 12.3 / 16.0 / 16.0 | 5.69 / 8.28 / 8.28 / 8.28 |  |  |
|  |  | `5e12885` | 8 | 2 / 2 (2–2) | 11.2 / 18.6 / 18.6 | 4.91 / 6.62 / 6.62 / 6.62 | -1.1 [-4.8, +5.9] | -0.77 [-2.01, +0.28] |
|  | Image pan drag, Overlay | `5f7b078` | 4 | 129 / 134 (126–134) | 133.8 / 139.9 / 139.9 | 4.28 / 6.94 / 6.94 / 6.94 |  |  |
|  |  | `5e12885` | 4 | 125 / 130 (124–130) | 134.8 / 142.2 / 142.2 | 5.19 / 6.78 / 6.78 / 6.78 | +0.9 [-10.5, +14.6] | +0.91 [-1.75, +2.50] |
| 461 × 490, 13 pt | History wheel notch | `5f7b078` | 240 | 1 / 1 (1–1) | 3.6 / 6.3 / 9.8 | 3.06 / 5.36 / 7.45 / 8.69 |  |  |
|  |  | `5e12885` | 240 | 1 / 1 (1–1) | 3.6 / 5.5 / 9.3 | 3.04 / 4.71 / 7.11 / 8.14 | -0.0 [-0.3, +0.1] | -0.02 [-0.27, +0.10] |
|  | History held arrow, 1 s | `5f7b078` | 16 | 64 / 64 (64–64) | 204.0 / 233.7 / 233.7 | 2.91 / 9.08 / 9.08 / 9.08 |  |  |
|  |  | `5e12885` | 16 | 64 / 64 (64–64) | 210.0 / 243.8 / 243.8 | 2.87 / 6.38 / 6.38 / 6.38 | +6.0 [-18.3, +19.3] | -0.04 [-0.69, +1.99] |
| 461 × 490, 18 pt | History wheel notch | `5f7b078` | 240 | 1 / 2 (1–2) | 3.4 / 6.5 / 10.1 | 2.84 / 5.12 / 6.45 / 7.21 |  |  |
|  |  | `5e12885` | 240 | 1 / 2 (1–2) | 3.3 / 6.2 / 8.8 | 2.81 / 4.56 / 6.72 / 7.29 | -0.1 [-0.2, +0.1] | -0.03 [-0.19, +0.16] |
|  | History held arrow, 1 s | `5f7b078` | 16 | 64 / 64 (64–64) | 193.9 / 231.2 / 231.2 | 2.78 / 6.50 / 6.50 / 6.50 |  |  |
|  |  | `5e12885` | 16 | 64 / 64 (64–64) | 194.6 / 216.3 / 216.3 | 2.99 / 5.45 / 5.45 / 5.45 | +0.7 [-13.8, +12.9] | +0.21 [-1.25, +0.72] |
|  | Text wheel notch | `5f7b078` | 80 | 1 / 2 (1–38) | 4.3 / 10.8 / 110.0 | 3.19 / 7.90 / 11.73 / 11.73 |  |  |
|  |  | `5e12885` | 80 | 1 / 2 (1–38) | 4.5 / 8.1 / 107.3 | 3.33 / 6.69 / 9.79 / 9.79 | +0.2 [-0.7, +0.7] | +0.14 [-0.36, +0.50] |

### Width steps

| Cell | Surface | Step | Build | n | Frames (count of steps) | Final without further input (steps) | UI CPU p50 / p95 / max (ms) | Configure to first frame request p50 / max (ms) | Last frame after the command p50 / max (ms) | Δ UI CPU p50 (ms) |
| --- | --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- |
| 1480 × 980, 13 pt | History | 1480 → 1000 | `5f7b078` | 8 | 3 (8) | 8 of 8 | 33.0 / 51.2 / 51.2 | 21.9 / 44.0 | 57.9 / 79.6 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 22.8 / 30.2 / 30.2 | 21.4 / 27.3 | 43.0 / 52.1 | -10.2 [-15.2, +2.2] |
|  |  | 1000 → 1480 | `5f7b078` | 8 | 4 (8) | 8 of 8 | 38.8 / 56.1 / 56.1 | 22.7 / 37.3 | 84.9 / 104.2 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 24.6 / 36.1 / 36.1 | 23.8 / 33.9 | 46.5 / 62.6 | -14.2 [-18.0, -4.6] |
|  | Text Compare | 1480 → 1000 | `5f7b078` | 8 | 1 (8) | 0 of 8 | 15.9 / 23.2 / 23.2 | 20.2 / 26.1 | 21.2 / 27.8 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 18.3 / 28.8 / 28.8 | 20.0 / 27.2 | 37.6 / 45.1 | +2.4 [-1.2, +10.7] |
|  |  | 1000 → 1480 | `5f7b078` | 8 | 1 (8) | 0 of 8 | 21.6 / 27.6 / 27.6 | 25.0 / 31.0 | 26.7 / 32.0 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 23.4 / 30.4 / 30.4 | 23.2 / 27.4 | 42.0 / 47.8 | +1.8 [-4.2, +7.3] |
|  | Image Compare | 1480 → 1000 | `5f7b078` | 8 | 1 (8) | 0 of 8 | 17.3 / 33.0 / 33.0 | 19.9 / 34.5 | 20.7 / 36.0 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 23.1 / 32.3 / 32.3 | 20.4 / 29.0 | 42.6 / 50.1 | +5.8 [-1.9, +11.3] |
|  |  | 1000 → 1480 | `5f7b078` | 8 | 1 (8) | 0 of 8 | 26.3 / 32.2 / 32.2 | 30.9 / 35.1 | 31.9 / 35.8 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 26.9 / 56.2 / 56.2 | 25.5 / 46.0 | 45.6 / 75.0 | +0.6 [-4.4, +20.2] |
| 1480 × 980, 18 pt | History | 1480 → 1000 | `5f7b078` | 8 | 3 (8) | 8 of 8 | 27.7 / 49.4 / 49.4 | 21.0 / 42.9 | 58.5 / 81.1 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 22.6 / 33.9 / 33.9 | 20.4 / 31.5 | 44.8 / 53.0 | -5.1 [-15.1, +5.7] |
|  |  | 1000 → 1480 | `5f7b078` | 8 | 3 (8) | 8 of 8 | 29.5 / 35.4 / 35.4 | 23.8 / 30.0 | 60.8 / 73.9 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 22.5 / 32.6 / 32.6 | 21.2 / 29.4 | 43.5 / 57.4 | -7.0 [-12.1, +2.8] |
|  | Text Compare | 1480 → 1000 | `5f7b078` | 8 | 1 (8) | 0 of 8 | 18.3 / 28.3 / 28.3 | 20.9 / 30.7 | 21.8 / 32.2 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 20.2 / 28.4 / 28.4 | 19.8 / 25.6 | 38.8 / 46.6 | +1.9 [-4.3, +9.1] |
|  |  | 1000 → 1480 | `5f7b078` | 8 | 1 (8) | 0 of 8 | 19.1 / 25.4 / 25.4 | 21.6 / 28.2 | 22.4 / 29.4 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 20.6 / 40.5 / 40.5 | 20.2 / 40.3 | 41.5 / 61.0 | +1.4 [-1.0, +12.7] |
|  | Image Compare | 1480 → 1000 | `5f7b078` | 8 | 1 (8) | 0 of 8 | 15.3 / 22.8 / 22.8 | 19.9 / 27.5 | 20.4 / 28.2 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 20.1 / 27.9 / 27.9 | 18.6 / 23.5 | 39.1 / 41.8 | +4.8 [-0.6, +10.0] |
|  |  | 1000 → 1480 | `5f7b078` | 8 | 1 (8) | 0 of 8 | 19.9 / 30.1 / 30.1 | 25.4 / 32.5 | 26.1 / 33.3 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 27.1 / 34.9 / 34.9 | 26.4 / 33.8 | 46.2 / 57.9 | +7.2 [-0.9, +12.8] |
| 461 × 490, 13 pt | History | 461 → 980 | `5f7b078` | 8 | 2 (8) | 8 of 8 | 22.8 / 35.6 / 35.6 | 31.2 / 38.9 | 48.6 / 63.1 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 19.4 / 35.2 / 35.2 | 29.0 / 38.1 | 47.3 / 59.3 | -3.5 [-9.5, +4.6] |
|  |  | 980 → 461 | `5f7b078` | 8 | 2 (8) | 8 of 8 | 11.1 / 12.6 / 12.6 | 19.9 / 24.5 | 29.7 / 38.5 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 11.7 / 14.8 / 14.8 | 20.3 / 26.9 | 34.3 / 40.3 | +0.6 [-0.9, +2.3] |
| 461 × 490, 18 pt | History | 461 → 980 | `5f7b078` | 8 | 2 (8) | 8 of 8 | 17.4 / 24.1 / 24.1 | 27.3 / 35.9 | 43.8 / 54.9 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 16.6 / 25.1 / 25.1 | 23.2 / 32.5 | 41.3 / 52.0 | -0.8 [-5.5, +1.6] |
|  |  | 980 → 461 | `5f7b078` | 8 | 2 (8) | 8 of 8 | 10.9 / 16.7 / 16.7 | 21.9 / 25.1 | 34.7 / 43.9 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 10.3 / 12.8 / 12.8 | 18.3 / 21.9 | 27.5 / 33.6 | -0.6 [-5.3, +1.0] |
|  | Text Compare | 461 → 980 | `5f7b078` | 8 | 2 (4), 63 (1), 64 (3) | 8 of 8 | 19.9 / 202.8 / 202.8 | 25.3 / 38.1 | 45.0 / 1008.1 |  |
|  |  |  | `5e12885` | 8 | 2 (4), 62 (1), 63 (1), 64 (2) | 8 of 8 | 20.8 / 196.6 / 196.6 | 26.2 / 29.2 | 43.4 / 1006.6 | +1.0 [-172.8, +175.3] |
|  |  | 980 → 461 | `5f7b078` | 8 | 3 (8) | 8 of 8 | 14.7 / 21.5 / 21.5 | 18.5 / 25.9 | 40.8 / 46.8 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 14.2 / 19.7 / 19.7 | 23.4 / 29.8 | 35.2 / 41.7 | -0.5 [-4.9, +1.0] |
|  | Image Compare | 461 → 980 | `5f7b078` | 8 | 2 (8) | 8 of 8 | 13.4 / 17.9 / 17.9 | 18.7 / 32.9 | 39.2 / 47.4 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 14.3 / 15.5 / 15.5 | 24.2 / 35.9 | 41.0 / 52.5 | +0.9 [-2.7, +2.2] |
|  |  | 980 → 461 | `5f7b078` | 8 | 2 (8) | 8 of 8 | 11.4 / 16.4 / 16.4 | 19.7 / 34.0 | 33.3 / 46.2 |  |
|  |  |  | `5e12885` | 8 | 2 (8) | 8 of 8 | 10.9 / 16.4 / 16.4 | 20.2 / 27.8 | 27.1 / 39.9 | -0.5 [-4.2, +2.3] |

### Changes list, 600 files, 1480 × 980 (absolute)

Writes alternate Stage and Unstage of `item-000.txt`, 10 per launch after one unrecorded pair, with 3.0 s windows. The draws are the paced frames of the write windows: mostly the row's flight animation and the woken frames of the write's results.

| Size | Build | Launches | Writes | Frames per write p50 (min–max) | Paced draws n | Paced p50 / p95 / p99 / max (ms) | Launch p50s | With 1–2-tick draws p95 / p99 / max (ms) | UI CPU per write p50 / p95 / max (ms) | Draw Δp50 (ms) |
| --- | --- | ---: | ---: | --- | ---: | --- | --- | --- | --- | --- |
| 13 pt | `5f7b078` | 2 | 20 | 25 (24–26) | 408 | 2.981 / 5.306 / 8.090 / 8.254 | 2.97–2.99 | 6.005 / 9.493 / 11.366 | 86.3 / 93.2 / 93.2 | |
| | `5e12885` | 2 | 20 | 25 (23–26) | 403 | 2.878 / 5.153 / 6.924 / 8.009 | 2.82–2.93 | 6.924 / 10.055 / 11.568 | 86.7 / 96.0 / 96.0 | −0.103 [−0.206, +0.060] |
| 18 pt | `5f7b078` | 2 | 20 | 25 (24–27) | 415 | 2.594 / 4.837 / 7.126 / 8.187 | 2.58–2.62 | 6.451 / 9.366 / 10.917 | 76.6 / 82.5 / 82.5 | |
| | `5e12885` | 2 | 20 | 26 (24–27) | 416 | 2.526 / 4.680 / 7.261 / 8.307 | 2.49–2.56 | 6.078 / 9.997 / 10.928 | 78.2 / 85.2 / 85.2 | −0.068 [−0.181, +0.052] |

Each write woke 3 frames. Four 40 px height steps per launch drew 2 frames each in both builds.

## Outliers kept

- Every frame and window is kept.
- The first text width step after the wheel phase at 461 × 490 drew 62–64 frames in both builds (4 of 8 steps per build). A scrollbar or scroll animation was still running; it is included in that row's p95 (UI CPU 197–203 ms).
- One text wheel window per launch drew 27–32 frames at 1480 px and 38 at 461 × 490, in both builds: the scroll's own animation.
- The first zoom choice of each launch (Fit → 100%) drew 12–16 frames in both builds, as in the image-compare record.
- Text mode Split drew 29–33 frames in both builds, Diff 2.

## Commands

```sh
tools/make-fixtures.sh                       # fixtures/pristine/{cols,many}
tools/run-all.sh 1 4                         # 32 Compare/History launches and 8 Changes launches, ABBA by round
python3 tools/analyse.py r*-*                # raw/windows.csv, raw/frames.csv (then gzip)
python3 tools/summary.py > raw/summary.txt; python3 tools/tables.py > raw/tables.md
# Changes cells: the Changes record's analyse.py, then tools/changes-summary.py over raw/changes/
```

The tools ran from a working directory outside the repository. The versioned copies differ from what ran only in path lines: `run-all.sh` finds the executables through `GITTURTLE_RELEASES`, and `changes-drive.py` is the [Changes record](2026-10-29-changes-body-redraw.md)'s `drive.py`, which finds the native-QA helpers through `GITTURTLE_QA_TOOLS` where the copy that ran named them by absolute path. `run-all.sh` still calls the Changes driver at `changes/tools/drive.py`, its location in that working directory.

## Limits

- One host and one display at 120 Hz, Midnight only, warm cache, 4 launches per build and cell (2 for Changes). The project pane was off, so the body's own `InFrame` (`workspace-columns`) was not exercised.
- The p50 intervals are 0.2–0.9 ms wide. A per-frame change of 0.1–0.2 ms, the size the image-compare record resolved, would not be detected here.
- The settle check reads the screen after a step, not each frame. Whether the candidate's first frame at a new width already matches its second was not observed. The base's unsettled Compare steps appeared at the app's default inspector width. `5618236d`'s own reproduction with an inspector dragged to 400 px was not run.
- At 461 × 490 only the image comparison's width steps were measured, its toolbars being menus; the text diff got wheel notches and width steps there. Text mode, zoom and mode choices ran only at 1480 px.
- The UI thread ran below 3.0 GHz in about 70% of windows in both builds, and launches were pinned to all eight performance cores (4–11), not to one cluster. The host was not idle: the IDE used about a core, and seven orphaned `fc-match` polling loops ran throughout. Alternating the builds within each cell exposes both to the same drift.
- `WAYLAND_DEBUG=1` adds libwayland's formatting to every message in both builds. It is inside the UI CPU figures and adds only the `done` line's print to the draw boundary.
- The cause of the candidate's second frame on a width step is inferred from the diff and was not traced.

## Rerun 2026-10-30: width steps of `aeab555`

This reruns only the width steps above against `aeab555a`, the I line: `3e9aa590` (`fix(app): draw each Compare width step in one frame`), `876a1b6d` (a file-list scroll still waiting to apply wins over the list's resize reveal) and the vendored `eaa93062` (an open popup follows its trigger in the frame that moves it). The base is this record's candidate, `5e128852`. The question is whether Compare's extra frame per width step is gone, and whether every step still ends settled. The data is in [`2026-10-29-columns-in-frame/rerun-2026-10-30/`](2026-10-29-columns-in-frame/rerun-2026-10-30/): per-window rows in `raw/windows.csv`, per-frame rows in `raw/frames.csv.gz`, the statistics in `raw/summary.json` and `raw/summary.txt`, the tables below in `raw/tables.md`, the run log in `raw/runs.txt`, the load in `raw/load-per-launch.txt`, and the designation, the fixed order and the display checks in `raw/designation.txt`. The scripts are in `tools/`.

### Result

- **Compare's extra frame is gone at 1480 ↔ 1000.** The base draws 2 frames on every one of its 64 Compare steps there (32 text, 32 image, 13 and 18 pt). The candidate draws 1 frame on every one of its 64. The removed frame is the base's paced second frame: 4.3–6.7 ms at p50 per row, and 10.2–16.5 ms at p95 counting the draws requested one to two ticks after their `done`.
- **Every step still ends settled, in both builds.** The settle check found 0 changed pixels on all 160 steps per build: History, text and image Compare at 1480 ↔ 1000, History at 461 ↔ 980 at both sizes, and text and image Compare at 461 ↔ 980, 18 pt.
- **Last frame after the command, Compare at 1480 ↔ 1000.** The candidate is at 25.0–44.8 ms at p50 against the base's 44.8–73.1 ms, and at 40.3–61.7 ms at p95 against 73.3–96.8 ms. Δp50 runs from −9.7 to −41.9 ms. Seven of the eight intervals exclude zero; image 1000 → 1480 at 13 pt does not (−9.7 [−46.6, +16.8]).
- **UI CPU per Compare step at 1480 ↔ 1000.** The candidate is at 19.2–39.4 ms at p50 against the base's 24.3–48.9 ms. Δp50 is negative in seven of eight rows, from −27.1 to −4.6 ms, and +0.2 in the eighth. Only text 1480 → 1000 at 13 pt resolves: −27.1 [−37.9, −4.7]. The other intervals are 33–60 ms wide and include zero.
- **The first frame did not get cheaper.** Configure to first frame request moves both ways between builds, row by row: 18.7–39.7 ms at p50 in the candidate against 19.0–38.3 ms in the base. The saving is the second frame and the tick wait before it.
- **Unchanged: History, and every step at 461 ↔ 980.** History draws 2 frames on every step in both builds, at both widths and sizes, as `3e9aa590`'s message says it still does for its table's columns. The tiled 461 ↔ 980 steps of text and image Compare at 18 pt also draw 2 frames on all 16 steps per surface in **both** builds. Their second frame is paced (a `wl_callback.done` and then a draw) in 27 of 32 candidate steps. The commit's one-frame claim therefore holds for Compare at 1480 ↔ 1000, not for the narrow tiled steps. That is not a regression: the base draws the same frames there, as `5e12885` did in the 2026-10-29 run. The cause of that second frame was not traced. History's 1000 → 1480 row at 13 pt reads +21.2 ms UI CPU and +14.3 ms to the last frame at p50. Both intervals include zero ([−18.0, +35.2], [−18.2, +40.6]), and the same row at 18 pt moves the other way (−9.1, −11.3).
- **This record's inferred cause was wrong.** The Verdict above attributed the second frame to `fit_container`'s `cx.notify()`. `3e9aa590`'s message reports a different source, the changed-file list's viewport probe deferring a window refresh as it painted, plus a popup that asked for a frame. This run measures the result and does not trace either cause.

### Verdict

`aeab555` removes the extra frame from every Compare width step at 1480 ↔ 1000 and keeps every step settled. The last frame comes 10–42 ms sooner at p50, resolved in seven of eight rows, and the UI CPU per step points down without resolving outside one row. There is no regression: History, and every step at 461 ↔ 980, draw the same frames in both builds and stay settled. Step-to-step noise is large: the bootstrap intervals of the per-step deltas are 21–70 ms wide, so a change in UI CPU or last frame smaller than about 10–35 ms, depending on the row, would not be detected. Narrow tiled Compare steps still draw 2 frames in both builds, so "each Compare width step in one frame" is true here only for the 1480 ↔ 1000 steps.

### Setup

- **Host:** as above, the Apple M2 Pro running Asahi Linux (Omarchy), on Linux 7.1.12-2-7-ARCH, Hyprland, native Wayland. Display: eDP-1, 120 Hz, scale 2, one tick 8.33 ms.
- **Quiet host and load:** the build hold (`.local/build-slots/quiet`) was in place from 14:42:44 to 15:21:43 UTC on 2026-10-30, 39 minutes. A workspace `clippy`/`check` that was already running finished before the first launch at 14:43:08. Every launch waited until `pgrep -x rustc` and `pgrep cargo` found nothing, and all 32 recorded `builds 0`.
  - AT-SPI was off, `IsEnabled false`, read before every launch.
  - Notifications were silenced with the shell's do-not-disturb (`omarchy-shell notifications setDnd on`; it was off before and was restored to off afterwards). A notification card that was already on screen stayed in the top-right corner, over part of the window's header at 1480 px, in every launch of both builds.
  - The orphaned `fc-match` loops of the first run were gone.
  - One-minute load average: 0.43–8.76 at launch starts (8.76 at the first launch, while the builds' load decayed; at most 3.05 from the third launch on), and 0.43–3.05 at ends.
  - Cores 4–11 were 74–98% idle (median 95%) over the 2 s before each launch.
  - Window-start frequency of the UI thread's CPU: 1.70 GHz at p50 in both builds, below 3.0 GHz in 139 of 160 base and 130 of 160 candidate step windows. That is lower than in the first run (2.40 GHz), so absolute times are not comparable with the tables above. Only the same-session base is.
- **Builds:** release executables from clean trees (`--build-info`), each embedding its own full revision and not the other's (`strings <exe> | grep <sha>`: 2 matches against 0, and 1 against 0). The base was designated in writing before the first measured launch:
  - Base `5e128852bd46e79fd0f6b7fbd91259cdca40f887`, sha256 `e581ae28a6462695c756a14b488d824bd01d6b8cb7bb9ac8aaa2959af11a1114`, the executable this record measured as its candidate.
  - Candidate `aeab555a2805617f1d23226ac972477d0b3e442a`, `source_tree` clean, aarch64-unknown-linux-gnu, release, rustc 1.99.0 (b940084d7 2026-09-28), build_unix_seconds 1793370923, sha256 `819bf2d618fc2a418bcaf1fe2720e486539f63f7b9a06646866502d15108518b`.
- **Fixture:** *cols* from `tools/make-fixtures.sh`, regenerated for this run with the same HEAD `27503ee` and HEAD~1 `7d2a777`, and copied fresh for each launch.
- **Cells and launch:** the same four cells, launch settings, preferences (Midnight, 13 or 18 pt, project pane off), workspace 9 and empty absolute `XDG_*` homes per launch. The launch homes and fixture copies were deleted afterwards.
- **Cache state:** warm. The executables and fixture had run before, and every launch is a new process with an empty preview cache.
- **Order:** fixed in writing before the first measured launch: `tools/run-all.sh 1 4`. Four rounds of cells L13, L18, S13 and S18, both builds per cell, base first in rounds 1 and 3 and candidate first in rounds 2 and 4. That gives 4 launches per build and cell, 32 in all. No launch failed, was dropped or was repeated. One trial launch of the candidate (`trial-aeab555-L13`) timed the driver. It ran before the hold, while builds were running, and is not a result.

### Method and boundaries

`tools/drive.py BIN CELL LABEL steps` adds a width-steps-only mode to this record's driver. The run goes:

1. Idle for 2 s, then Home (unrecorded).
2. Four History steps.
3. Enter twice into text Compare on `long.txt`, then four steps.
4. Escape twice, Down, then Enter twice into image Compare on `photo.png`, then four steps.
5. Idle for 2 s.

There is no wheel, held-key, text-mode, zoom, pan or image-mode phase. The text diff therefore stays in its default Diff mode, which is also where the first run's mode choices ended. The image stays at Fit, where the first run had stepped it at 200% in Side by side. Without a wheel phase, no step began inside a scroll animation. That was the cause of the first run's 62–64-frame outliers at 461 × 490. S13 runs History only, as before. Each step and its settle check are unchanged. The step parks the pointer on the status bar, samples, sends one `hyprctl` resize, waits 1.5 s and samples. Each step got exactly one `xdg_toplevel.configure`, in all 320 step windows.

The boundaries:

- **Frames per step:** main-surface `wl_surface.attach`es between the driver's sample before the resize command and its sample 1.5 s later.
- **Last frame after the command:** from the driver's sample, taken just before it runs `hyprctl`'s resize, to the last main-surface attach in the window.
  - *What it covers:* the `hyprctl` process, the compositor's handling, configure delivery, the app's configure handling and every draw of the step, up to Mesa's attach.
  - *What it excludes:* GPU execution, compositing and scan-out after the attach.
  - *Tick grid:* GPUI draws a configure without waiting for a frame callback, so a one-frame step has no tick floor. A second frame follows a `wl_callback.done`, so a two-frame step cannot end sooner than the first attach plus the wait for the next tick (up to 8.33 ms) plus the second draw. The candidate's Compare saving is that term, and a two-frame build cannot reach a one-frame build's value on this metric by construction.
- **Configure to first frame request:** from libwayland's dispatch of `xdg_toplevel.configure` to the first `wl_surface.frame` request on the main surface. It covers the configure handler and the whole first draw (request_layout, layout, prepaint with the `InFrame` builds, paint). It excludes present, the GPU and compositing. It starts from an event, not a tick, so the grid puts no floor under it. Note that the first run's table column "Configure to first frame request" is computed by `summary.py` from the driver's sample, as is this run's "Command to first frame request". The first run's column therefore spans the command to the first request, not the configure.
- **Paced draw:** as above, from a main-surface `wl_callback.done` to the next frame request when that comes 0.5 ms to one tick later. The view "with 1–2-tick draws" adds requests one to two ticks after the `done`. Its ceiling is two ticks by construction.
- **UI CPU per step:** the main thread's schedstat run time between the driver's two samples. It includes every draw, the configure handling and libwayland's debug formatting. The idle windows read 0.03–0.22 ms over 2 s with no frames, in every launch of both builds.
- **Settle check:** as above. A count of 0 means the step left the final screen without further input. It does not show what each frame drew.

Statistics are as above: pooled nearest-rank percentiles, n = 8 steps per build, cell, surface and direction (2 per launch), and a two-level bootstrap of candidate minus base (4,000 resamples, seed 1, 95% interval).

### Width steps

| Cell | Surface | Step | Build | n | Frames p50 / max | Frames (count of steps) | Final without further input | UI CPU p50 / p95 / max (ms) | Configure to first frame request p50 / p95 (ms) | Last frame after the command p50 / p95 / max (ms) | Δ UI CPU p50 (ms) | Δ last frame p50 (ms) |
| --- | --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | --- | --- |
| 1480 × 980, 13 pt | History | 1480 → 1000 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 32.4 / 61.3 / 61.3 | 24.3 / 52.6 | 53.9 / 80.4 / 80.4 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 28.4 / 78.9 / 78.9 | 21.5 / 69.3 | 43.5 / 97.0 / 97.0 | -4.0 [-29.8, +25.7] | -10.4 [-36.5, +24.3] |
|  |  | 1000 → 1480 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 39.8 / 64.7 / 64.7 | 29.4 / 50.1 | 70.5 / 88.6 / 88.6 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 61.0 / 64.8 / 64.8 | 47.0 / 50.7 | 84.8 / 92.2 / 92.2 | +21.2 [-18.0, +35.2] | +14.3 [-18.2, +40.6] |
|  | Text Compare | 1480 → 1000 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 46.3 / 57.7 / 57.7 | 35.0 / 45.9 | 66.9 / 82.8 / 82.8 |  |  |
|  |  |  | `aeab555` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 19.2 / 41.2 / 41.2 | 18.7 / 40.3 | 25.0 / 45.5 / 45.5 | -27.1 [-37.9, -4.7] | -41.9 [-51.8, -16.4] |
|  |  | 1000 → 1480 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 48.9 / 58.5 / 58.5 | 38.3 / 46.5 | 68.4 / 80.8 / 80.8 |  |  |
|  |  |  | `aeab555` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 26.9 / 47.3 / 47.3 | 26.5 / 47.5 | 32.7 / 53.2 / 53.2 | -22.0 [-29.8, +17.6] | -35.7 [-45.9, -0.1] |
|  | Image Compare | 1480 → 1000 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 47.1 / 61.8 / 61.8 | 34.6 / 45.3 | 67.7 / 88.1 / 88.1 |  |  |
|  |  |  | `aeab555` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 33.1 / 45.3 / 45.3 | 32.0 / 45.8 | 36.1 / 51.3 / 51.3 | -14.0 [-43.9, +4.0] | -31.6 [-57.1, -12.4] |
|  |  | 1000 → 1480 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 39.2 / 72.7 / 72.7 | 25.8 / 56.8 | 54.5 / 96.8 / 96.8 |  |  |
|  |  |  | `aeab555` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 39.4 / 55.0 / 55.0 | 39.7 / 55.4 | 44.8 / 60.4 / 60.4 | +0.2 [-29.1, +31.3] | -9.7 [-46.6, +16.8] |
| 1480 × 980, 18 pt | History | 1480 → 1000 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 39.6 / 65.0 / 65.0 | 27.7 / 58.0 | 57.3 / 88.0 / 88.0 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 43.2 / 55.3 / 55.3 | 32.9 / 47.4 | 63.5 / 74.2 / 74.2 | +3.6 [-35.1, +15.9] | +6.2 [-35.6, +23.6] |
|  |  | 1000 → 1480 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 36.3 / 59.5 / 59.5 | 27.2 / 46.9 | 61.6 / 80.5 / 80.5 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 27.2 / 60.2 / 60.2 | 22.2 / 47.9 | 50.2 / 91.1 / 91.1 | -9.1 [-34.0, +22.0] | -11.3 [-34.1, +27.8] |
|  | Text Compare | 1480 → 1000 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 41.7 / 55.1 / 55.1 | 31.4 / 43.3 | 58.2 / 73.3 / 73.3 |  |  |
|  |  |  | `aeab555` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 34.3 / 39.1 / 39.1 | 34.4 / 38.4 | 41.4 / 44.6 / 44.6 | -7.3 [-20.7, +12.0] | -16.9 [-36.2, -3.2] |
|  |  | 1000 → 1480 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 37.5 / 57.9 / 57.9 | 29.0 / 47.1 | 58.1 / 77.9 / 77.9 |  |  |
|  |  |  | `aeab555` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 23.1 / 44.6 / 44.6 | 22.8 / 44.7 | 29.7 / 49.9 / 49.9 | -14.4 [-33.6, +5.8] | -28.4 [-45.8, -11.9] |
|  | Image Compare | 1480 → 1000 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 24.3 / 55.0 / 55.0 | 19.0 / 41.7 | 44.8 / 73.5 / 73.5 |  |  |
|  |  |  | `aeab555` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 19.6 / 36.8 / 36.8 | 18.7 / 36.6 | 26.0 / 40.3 / 40.3 | -4.6 [-36.1, +11.0] | -18.8 [-48.6, -4.9] |
|  |  | 1000 → 1480 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 48.8 / 65.8 / 65.8 | 37.4 / 53.3 | 73.1 / 88.9 / 88.9 |  |  |
|  |  |  | `aeab555` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 31.3 / 57.7 / 57.7 | 30.2 / 55.8 | 36.9 / 61.7 / 61.7 | -17.5 [-37.9, +5.3] | -36.2 [-54.2, -9.9] |
| 461 × 490, 13 pt | History | 461 → 980 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 38.8 / 73.5 / 73.5 | 34.0 / 60.3 | 61.4 / 93.7 / 93.7 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 35.6 / 77.7 / 77.7 | 27.6 / 61.9 | 62.4 / 98.2 / 98.2 | -3.2 [-31.5, +38.3] | +1.0 [-23.0, +36.5] |
|  |  | 980 → 461 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 15.5 / 32.9 / 32.9 | 21.5 / 38.6 | 45.0 / 66.2 / 66.2 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 17.8 / 33.3 / 33.3 | 18.1 / 28.9 | 39.1 / 57.7 / 57.7 | +2.3 [-14.5, +18.7] | -5.9 [-22.1, +22.2] |
| 461 × 490, 18 pt | History | 461 → 980 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 38.5 / 72.1 / 72.1 | 45.8 / 60.9 | 74.6 / 96.9 / 96.9 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 32.8 / 42.8 / 42.8 | 30.0 / 40.1 | 63.8 / 73.6 / 73.6 | -5.7 [-38.8, +10.7] | -10.8 [-28.6, +9.1] |
|  |  | 980 → 461 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 16.4 / 30.9 / 30.9 | 18.4 / 35.7 | 41.5 / 64.9 / 64.9 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 14.9 / 22.3 / 22.3 | 14.1 / 20.1 | 40.9 / 50.7 / 50.7 | -1.5 [-15.2, +6.0] | -0.6 [-15.9, +8.7] |
|  | Text Compare | 461 → 980 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 24.8 / 53.7 / 53.7 | 20.3 / 51.5 | 43.3 / 89.0 / 89.0 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 29.5 / 46.8 / 46.8 | 30.1 / 43.5 | 59.6 / 81.7 / 81.7 | +4.7 [-34.9, +22.6] | +16.3 [-36.0, +34.4] |
|  |  | 980 → 461 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 15.4 / 32.2 / 32.2 | 19.3 / 35.7 | 44.8 / 67.6 / 67.6 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 14.1 / 28.1 / 28.1 | 16.1 / 30.8 | 38.8 / 62.1 / 62.1 | -1.3 [-17.4, +15.0] | -6.0 [-21.0, +10.6] |
|  | Image Compare | 461 → 980 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 24.1 / 52.1 / 52.1 | 25.9 / 43.8 | 55.2 / 87.1 / 87.1 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 21.9 / 43.4 / 43.4 | 26.1 / 37.0 | 45.5 / 70.3 / 70.3 | -2.2 [-15.2, +10.3] | -9.7 [-23.5, +21.1] |
|  |  | 980 → 461 | `5e12885` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 16.2 / 23.7 / 23.7 | 19.6 / 26.2 | 40.2 / 55.9 / 55.9 |  |  |
|  |  |  | `aeab555` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 12.5 / 33.2 / 33.2 | 12.5 / 39.6 | 37.1 / 62.9 / 62.9 | -3.7 [-11.7, +18.0] | -3.1 [-21.1, +21.6] |

The per-row command-to-first-request figures and the paced draws inside the step windows are in `raw/tables.md`.

### Limits

- The run covers one host, one display at 120 Hz, Midnight only and a warm cache. It has 4 launches per build and cell, which is 8 steps per row, and 16 Compare steps per surface at 461 × 490, 18 pt only.
- The UI thread ran at 1.70 GHz at p50, below 3.0 GHz in about 84% of step windows. Launches were pinned to all eight performance cores (4–11).
- The first launch started at a load of 8.76, while the builds' load decayed. Its cell (L13, base, round 1) is one of four base launches in that cell.
- The image comparison was stepped at Fit, not at 200%, and the text diff in its default mode. The first run's mode, zoom and scroll states before the steps were not reproduced.
- The settle check reads the screen after a step, not each frame. A notification card covered part of the window's header at 1480 px in both builds, and a difference under it would not be counted.
- The cause of the second frame on narrow tiled Compare steps, in both builds, was not traced.
- Product source was not changed.

### Commands

```sh
tools/make-fixtures.sh                       # fixtures/pristine/cols (and many, unused)
tools/run-all.sh 1 4                         # 32 width-step launches, base first in odd rounds
python3 tools/analyse.py r*-*                # raw/windows.csv, raw/frames.csv (then gzip)
python3 tools/summary.py > raw/summary.txt; python3 tools/tables.py > raw/tables.md
```

As in the first run, the tools ran from a working directory outside the repository (`.local/bench-columns-rerun-2026-10-30`), with `GITTURTLE_RELEASES` and `GITTURTLE_QA_TOOLS` set to the release and native-QA tool directories. The versioned copies are byte-identical to what ran.

## Rerun 2026-11-01: WS1 and the rounds since

This measures `a50eae8e` (the branch head: WS1, NT3, NT4, PC3, WS2, WS3 and the vendored menu fixes, 60 commits after the base) against `2f0ac97f`, the head before WS1 landed. It has four parts: the width steps of the 2026-10-30 rerun; the three `#[ignore]` release benchmarks those rounds added; the full interaction protocol of the first run at 1480 × 980, 13 pt, which covers PC1's image-comparison redraws and PC2's commit selection; and a live resize with a refused write's hook report on screen, for NT4. Every launch of the last two parts also counted the UI thread's heap allocation calls, for NT3's and CH2's allocation removals. The data is in `.local/bench-ws1-2026-11-01/` on the measuring host (file list under Commands).

### Result

- **WS1's claim holds: every History width step now draws one frame.** The base draws 2 frames on every one of its 96 History and narrow-Compare steps, and 1 on each of its 64 Compare steps at 1480 ↔ 1000. The candidate draws 1 frame on all 160 of its steps. That covers History at 1480 ↔ 1000 and 461 ↔ 980, at 13 and 18 pt, and text and image Compare at both widths. The narrow tiled Compare steps, 2 frames in both builds in the 2026-10-30 rerun, are 1 frame in the candidate too. Every one of the 320 steps ended settled (settle check 0) and got exactly one configure.
  - *Where the base drew two frames:* the last frame comes sooner in the candidate in all 14 of those rows at p50, by 3.4 to 35.9 ms. Four intervals exclude zero: History 1000 → 1480 at 13 pt, −24.1 [−50.6, −2.2] ms, and at 18 pt, −35.7 [−52.0, −9.7]; History 980 → 461 at 13 pt, −9.1 [−25.1, −2.4], and at 18 pt, −8.3 [−16.9, −0.2]. The removed frame was the base's paced second draw: 4.3–7.8 ms at p50 per row, and up to 13.0 ms counting draws one to two ticks after their `done`.
  - *The single frame does more work.* On the narrow steps its configure-to-first-request p50 is higher than the base's first frame: 980 → 461, 29.4 against 20.1 ms (History, 13 pt), 33.0 against 25.0 (History, 18 pt), 24.6 against 17.4 (text) and 25.6 against 17.3 (image). That is the work the base split over two frames, now in one. UI CPU per step moved −23.5 to +11.1 ms at p50, with no interval excluding zero.
  - *Compare at 1480 ↔ 1000, one frame in both builds:* no resolved change in last frame or UI CPU (Δp50 −14.1 to +8.8 ms, every interval including zero).
- **Benchmarks, candidate only.** Every timed width step drew 1 frame and left no frame for the next callback, in all 2,400 steps: History, text and image Compare at 13 and 18 pt, 5 rounds. The handler p50 was 1.80–3.47 ms, p95 1.83–4.42 ms and max 4.72 ms. Every one of 800 warm-tab switches drew 1 frame with nothing left over, at a handler p50 of 3.03–3.51 ms and a max of 4.08 ms. **`quiet_landing_release_benchmark` does not run at the head.** Its `5000_rows` series panics before it writes any output (`views.rs:14084`, "5000_rows: the rows shown", left 500, right 5000), in 2 of 2 runs, so it has no result.
- **PC1 and PC2 interactions: no resolved change.** Paced-draw Δp50: History +0.09 [−0.85, +0.85] ms, text Compare −0.26 [−1.26, +0.77], image Compare +0.08 [−0.65, +0.50].
  - *PC2's commit selection.* A held arrow key in History draws its input-led frames at 4.94 against 5.03 ms p50 (Δ +0.09 [−0.43, +0.91]). Its UI CPU per 1 s hold is 408.5 against 413.9 ms (+5.4 [−47.2, +70.1]).
  - *PC1's caption cache.* Pan drags at 200% cost 235.1 against 231.2 ms of UI CPU (−3.9 [−22.3, +28.3]) and draw 109–128 frames per drag in both builds.
  - *Neither commit is in this comparison.* PC1 (`845c646f`, `aa9483f6`) and PC2 (`802ccec1`) landed before `2f0ac97`. These rows show that the rounds since did not regress them; they do not measure PC1's or PC2's own effect.
- **NT3 and CH2 allocations: no visible difference per frame.** A History wheel-notch frame makes 11,506 main-thread allocation calls in the base and 11,504 in the candidate (Δ −2 [−39, +35]). A text notch makes 8,960 against 8,953 calls per window (Δ −7 [−38, +44]), and an image notch 8,492 against 8,500 (Δ +8 [−4, +15]). NT3's once-per-frame `LineKey` read is in the candidate. It removes at most a few calls per frame, below this noise of about ±40 calls. CH2's tab-label buffer (`dac88888`, `667859ed`) predates both builds. Compare width-step windows make 5–10 more calls in the candidate, some resolved but tiny: image 1480 → 1000, +9 [+8, +10]; text 1000 → 1480, +5 [+5, +6]. History width-step windows make about 10,800 fewer, because one frame replaced two.
- **NT4: no regression; the report never reaches the Create tag form's error line.** The `reference-transaction` hook refuses the write only after the form has closed. `TagForm` sets its error line only from a failed plan preparation (`tags.rs:1189`). The write's failure goes to the window-wide notice (`integration.rs:58-63`), which shows "Git operation failed: remote: declined …" (one line, cut) or "…: remote: hook line 1 declined", with Details… and Dismiss. The live resizes therefore measure that notice.
  - *Frames:* the candidate draws 1 frame per 7-configure resize in all 144 resizes; the base draws 2, and 4 in 4 resizes.
  - *Allocation calls:* 8,500–9,100 fewer per resize in the candidate, resolved in every row, which is the base's extra frame.
  - *The report's own cost* (notice shown minus dismissed) is +239 / +250 calls per resize in the base and +145 / +156 in the candidate for the 16 KiB line; for 550 lines it is +153 / +155 and +73 / +79.
  - *Time:* UI CPU per resize moves −31.9 to +9.1 ms at p50, no interval excluding zero. The last frame is sooner in all 8 rows, by 4.2 to 54.7 ms; two are resolved: 550 lines, 600 → 461, −40.7 [−62.5, −3.1]; dismissed, 461 → 600, −54.7 [−69.9, −12.7].
  - *Not measurable here:* the error line NT4 changed (`tag-preparation-error`) cannot show a hook's report in either build. A preparation error is short (at most a 4,096-byte target), so the 16 KiB and 550-line case exists only in `a_long_report_is_measured_in_bounded_work_through_a_live_resize`.

### Verdict

No regression. WS1 does what it claims: every width step on every surface, at both widths and both text sizes, draws one frame and ends settled. Where the base drew two frames, the last frame now comes 3–36 ms sooner at p50, resolved in 4 of 14 rows. The 2026-10-30 rerun's remaining two-frame case, narrow tiled Compare, is gone too. WS2's warm-tab switch draws one frame in all 800 timed switches. The interaction protocol, the PC1 and PC2 rows and the allocation counts show no resolved change in per-frame cost (intervals about ±0.5–1.3 ms at p50 on paced draws, about ±40 allocation calls per frame). One benchmark finding needs the owner: `quiet_landing_release_benchmark` panics at the head in its `5000_rows` series, so WS3's landing timing has no release figure. A likely cause, inferred and not traced: `9a8d3574` itself, the commit that added the benchmark, made a landing that finds the repository unchanged skip rereading the history. Raising `app.limit` and rescanning would then no longer install 5,000 rows. The smallest fix is in the test: make the `5000_rows` case install its rows through a path that reads the history again, such as an explicit refresh or a changed tip, before it asserts the row count. Product source was not changed.

### Setup

- **Host:** as above, the Apple M2 Pro running Asahi Linux (Omarchy) on Linux 7.1.12-2-7-ARCH, `schedutil`, Hyprland, native Wayland. Display: eDP-1, 3456 × 2160 at 120 Hz, scale 2; one tick is 8.33 ms.
- **Quiet host:** the build hold (`.local/build-slots/quiet`) was in place from 04:50:56 to 11:22:56 UTC on 2026-11-02, including an idle gap from 05:31:24 to 10:20:25 while the session was suspended; no launch ran in the gap.
  - Every launch and benchmark run waited until `pgrep -x rustc` and `pgrep cargo` found nothing, and all recorded `builds 0`.
  - AT-SPI was off, `IsEnabled false`, read before every launch. The Orca screen reader was not running (no `orca.pid`).
  - Notifications were silenced with `omarchy-shell notifications setDnd on`; DND was off before and was restored to off at 11:22:56.
  - The IDE hosting the session (two `orca-ide` processes) used about 5% of a core each.
- **Load and frequency:**
  - *Width steps* (32 launches, 04:52:46–05:31:24): one-minute load 0.10–0.75 at starts and ends; cores 4–11 were 97–99% idle (median 98%) over the 2 s before each launch. The UI thread's window-start frequency was 1.45 GHz at p50 in both builds, below 3.0 GHz in 157 of 160 base and 158 of 160 candidate step windows.
  - *Benchmarks* (10:20:25–10:24:03): load 0.85–2.00; cores 4–11 87–97% idle.
  - *Interaction phase* (24 launches, 10:36:55–11:22:46): load 0.74–1.60; cores 4–11 83–98% idle (median 94%). Window-start frequency 1.19 GHz at p50 in both builds, below 3.0 GHz in 582 of 596 base and 558 of 596 candidate windows.
  - Absolute times are therefore not comparable with earlier sessions; only the same-session base is.
- **Builds:** release executables from clean trees (`--build-info`: release, aarch64-unknown-linux-gnu, rustc 1.99.0 (b940084d7 2026-09-28)). Each embeds its own full revision and not the other's: `strings | grep -c`, 2 against 0 for each. The base was designated in writing before the first measured launch (`raw/designation.txt`).
  - Base `2f0ac97fa50fe397fe5df2a5d967cdc4a7698cab`, build_unix_seconds 1793586803, sha256 `9a717cae3692f25324f64cf0179137955e72afeb3e5bebfa68a864e438a759a4`.
  - Candidate `a50eae8e6bd651264d66f1112fa3bf30358f59bd`, build_unix_seconds 1793594696, sha256 `ac0fc3086b81773999092b2e442e2a000524cf6fd7fde51e066adfca000fd747`.
  - Candidate test binary (`cargo test --locked --release -p gitturtle --bin gitturtle --no-run` at `a50eae8e`): sha256 `c38ca313625f3d123bd89ef2152317140cae59651bf76ae13cb6ae97e8ee1c59`, `strings` 3 matches of `a50eae8e…` and 0 of `2f0ac97f…`.
- **Fixtures** (disposable; a fresh copy per launch):
  - *cols*: as above, HEAD `27503ee`, HEAD~1 `7d2a777`, 3,003 commits, 121 references, copied from the 2026-10-30 rerun's pristine copy.
  - *hook1* and *hook550* (`tools2/make-hook-fixtures.sh`): 20 commits each. Their `.git/hooks/reference-transaction` drains stdin, and in the `prepared` state writes a report to stderr and exits 1. The report is 16,371 bytes on one line ("remote: declined " × 963, as NT4's test) or 550 lines of "remote: hook line N declined" (16,942 bytes).
- **Cache state:** warm. The executables and fixtures had run before, and every launch is a new process with an empty preview cache.
- **Order:** fixed in writing before each phase's first launch.
  - *Width steps:* `tools/run-all.sh 1 4`, as the 2026-10-30 rerun: four rounds of cells L13, L18, S13 and S18, both builds per cell, base first in rounds 1 and 3. 32 launches.
  - *Benchmarks:* `tools2/run-bench.sh`, rounds 1–5. Each round ran width steps, warm-tab switch and, in round 1 only, quiet landing.
  - *Interaction phase:* `tools2/run-phase3.sh 1 4`. Each round ran the full driver at L13, then the hook driver on hook1 and on hook550, with both builds, base first in rounds 1 and 3. 8 full and 16 hook launches.
  - No measured launch failed, was dropped or was repeated. Not results: one width-step trial of the candidate (`trial-a50eae8-L13`), four hook trials of the candidate while the driver's form navigation was fixed, and one of the base.

### Method and boundaries

- **Width steps:** `tools/` is the 2026-10-30 rerun's `tools/` with the builds renamed (`run-all.sh`, `summary.py`, `tables.py`). The protocol, the settle check and every boundary are as in that rerun.
  - *Last frame after the command* spans from the driver's sample before `hyprctl`'s resize to the last main-surface attach. GPUI draws a configure without waiting for a callback, so a one-frame step has no tick floor. A two-frame step cannot end before its first attach plus the wait for the next tick (up to 8.33 ms) plus the second draw. The base's two-frame rows are bounded below that way by construction, and the candidate's saving on them includes that term.
  - *Configure to first frame request* spans from libwayland's dispatch of `xdg_toplevel.configure` to the first `wl_surface.frame` request. It starts at an event, so the tick grid puts no floor under it.
- **Benchmarks:** the tests' own boundaries, as their JSON records them.
  - *Width steps:* one `simulate_resize` on the UI thread, from before the call to its return, frames included, on the test platform (no GPU, compositor or display link).
  - *Warm-tab switch:* one `switch_repository_tab`, with the reader and the executors held, so no reply is timed.
  - `next_frame_ms` times the callbacks a step or switch left, separately.
  - Run as `GITTURTLE_<NAME>_BENCH_OUTPUT=<out.json> taskset -c 4-11 testbin-a50eae8 --ignored --exact views::tests::<name>_release_benchmark`.
  - No base figure exists for these. WS1, WS2 and WS3 added the tests, and they call code the base does not have, such as `take_history_width`, `switch_warm_tab`, `repository_frames` and `many_refs_script`, none of which exist at `2f0ac97`, so the base product code cannot run them.
- **Interaction phase, full driver** (`tools2/drive.py`, unchanged from the first run's driver): History wheel, held arrows and width steps; text wheel, Split/Diff and width steps; image zoom, pan drags, wheel, modes and width steps. 1480 × 980, 13 pt only. The boundaries are the first run's.
  - *Paced draw:* from a main-surface `wl_callback.done` to the next frame request, when that comes 0.5 ms to one tick later. It starts at a tick, so the grid puts no floor under it. The ceiling is one tick, or two in the view with 1–2-tick draws.
  - *Input-led draw:* from the input event to the first frame request.
  - With the UI thread at about 1.2 GHz, 227–272 History draws and 93–103 image draws per build fell one to two ticks after their `done`. The "with 1–2-tick draws" columns carry those.
- **Allocation counter** (`tools/alloccount/alloccount.c`, preloaded with `LD_PRELOAD` on every launch of the interaction phase):
  - *What it counts:* `malloc`, `calloc`, `realloc`, `memalign`, `aligned_alloc` and `posix_memalign` calls, and their requested bytes, made on the process's main thread (tid = pid). The counts go into a shared page that the driver reads at each sample. It forwards to glibc's `__libc_*`. The app uses the system allocator; the only `#[global_allocator]` in the tree is in a vendor bench.
  - *What a window spans:* from the driver's start sample to its end sample, every frame and every handled reply included.
  - *Exclusions:* frees and other threads.
  - *Side effects:* the preload's environment reaches the app's child Git processes, which count into their own pages and add the same small cost in both builds. The `/dev/shm` pages were deleted afterwards. libwayland's debug formatting (`WAYLAND_DEBUG=1`) allocates in both builds and is inside every count.
- **Hook driver** (`tools2/drive-hook.py`): 600 × 680, tiled beside one `foot` filler and above another, 13 pt.
  - *Getting the report on screen:* Ctrl+Shift+P, "tags", Enter. Tab and Space reach Create tag… (Enter would choose the dialog's Done). A click in the name field (found by OCR), "v1", Lightweight (OCR), Enter, then Enter on the confirmation runs the write, and the hook refuses it.
  - *Live resize:* 12 resizes with the notice shown, alternating 600 → 461 and 461 → 600. Each is seven relative `hyprctl` resizes of the tiled window in 20 px steps, sent back to back as a drag sends them, each delivered as one configure (all 288 resizes got 7). The window runs from the driver's sample before the first command to its sample 1.0 s after the last.
  - *Control:* Dismiss (OCR), then 6 more resizes without the notice.
  - *Last frame after the first command:* it includes seven `hyprctl` round trips, the compositor and every draw. One frame per resize means GPUI drew once for the burst, so the value is bounded below by the burst's delivery, not by the tick grid.
- **Statistics:** pooled nearest-rank percentiles. Deltas are candidate minus base with the two-level bootstrap: launches with replacement within each build, then windows within each drawn launch; 4,000 resamples, seed 1, 95% interval.

### Width steps

| Cell | Surface | Step | Build | n | Frames p50 / max | Frames (count of steps) | Final without further input | UI CPU p50 / p95 / max (ms) | Command to first frame request p50 / max (ms) | Configure to first frame request p50 / p95 (ms) | Last frame after the command p50 / p95 / max (ms) | Δ UI CPU p50 (ms) | Δ last frame p50 (ms) |
| --- | --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1480 × 980, 13 pt | History | 1480 → 1000 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 51.3 / 75.9 / 75.9 | 42.8 / 75.5 | 37.2 / 70.6 | 78.1 / 94.7 / 94.7 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 38.9 / 68.9 / 68.9 | 41.8 / 72.3 | 38.5 / 69.5 | 43.3 / 73.6 / 73.6 | -12.4 [-45.1, +29.9] | -34.9 [-63.6, +19.7] |
|  |  | 1000 → 1480 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 41.4 / 60.2 / 60.2 | 34.8 / 55.2 | 30.6 / 50.0 | 71.9 / 88.4 / 88.4 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 41.4 / 49.7 / 49.7 | 46.7 / 54.8 | 41.0 / 49.8 | 47.9 / 56.2 / 56.2 | +0.0 [-30.4, +16.4] | -24.1 [-50.6, -2.2] |
|  | Text Compare | 1480 → 1000 | `2f0ac97` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 37.8 / 43.7 / 43.7 | 42.1 / 49.6 | 38.7 / 43.8 | 43.8 / 50.8 / 50.8 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 24.5 / 46.6 / 46.6 | 29.4 / 51.9 | 23.7 / 46.0 | 30.9 / 53.1 / 53.1 | -13.2 [-25.7, +13.5] | -12.9 [-23.9, +12.3] |
|  |  | 1000 → 1480 | `2f0ac97` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 41.9 / 46.7 / 46.7 | 45.1 / 52.0 | 41.2 / 46.6 | 46.6 / 53.4 / 53.4 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 28.0 / 41.1 / 41.1 | 31.7 / 45.9 | 28.0 / 41.3 | 32.5 / 47.8 / 47.8 | -13.9 [-24.2, +9.8] | -14.1 [-28.6, +7.9] |
|  | Image Compare | 1480 → 1000 | `2f0ac97` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 27.9 / 44.9 / 44.9 | 32.4 / 48.6 | 27.6 / 44.9 | 33.4 / 49.9 / 49.9 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 28.4 / 44.9 / 44.9 | 33.6 / 50.1 | 28.4 / 45.5 | 34.4 / 51.5 / 51.5 | +0.6 [-19.1, +19.1] | +1.0 [-19.1, +18.8] |
|  |  | 1000 → 1480 | `2f0ac97` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 47.5 / 59.4 / 59.4 | 49.8 / 63.0 | 46.9 / 59.7 | 51.1 / 64.1 / 64.1 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 35.5 / 59.9 / 59.9 | 41.0 / 65.5 | 35.3 / 59.5 | 42.1 / 66.8 / 66.8 | -12.0 [-22.5, +24.8] | -9.0 [-23.6, +25.3] |
| 1480 × 980, 18 pt | History | 1480 → 1000 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 47.7 / 61.3 / 61.3 | 42.5 / 60.3 | 36.8 / 56.5 | 67.1 / 89.1 / 89.1 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 24.2 / 64.5 / 64.5 | 30.0 / 67.7 | 24.2 / 64.3 | 31.2 / 69.1 / 69.1 | -23.5 [-33.5, +14.9] | -35.9 [-54.5, +3.1] |
|  |  | 1000 → 1480 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 40.6 / 58.9 / 58.9 | 34.3 / 51.8 | 31.0 / 46.2 | 64.0 / 80.4 / 80.4 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 22.0 / 49.3 / 49.3 | 27.4 / 52.6 | 22.2 / 48.4 | 28.3 / 54.1 / 54.1 | -18.6 [-32.3, +8.7] | -35.7 [-52.0, -9.7] |
|  | Text Compare | 1480 → 1000 | `2f0ac97` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 29.7 / 42.5 / 42.5 | 31.8 / 45.0 | 28.7 / 41.8 | 33.2 / 46.6 / 46.6 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 25.7 / 42.6 / 42.6 | 28.2 / 47.4 | 25.0 / 42.4 | 29.5 / 49.1 / 49.1 | -4.0 [-18.3, +18.6] | -3.7 [-21.3, +18.5] |
|  |  | 1000 → 1480 | `2f0ac97` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 32.7 / 48.0 / 48.0 | 37.2 / 52.8 | 31.6 / 47.8 | 39.0 / 54.5 / 54.5 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 43.7 / 48.0 / 48.0 | 46.3 / 54.2 | 43.1 / 47.4 | 47.8 / 56.0 / 56.0 | +11.1 [-14.8, +23.8] | +8.8 [-14.5, +24.6] |
|  | Image Compare | 1480 → 1000 | `2f0ac97` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 26.4 / 41.6 / 41.6 | 30.3 / 46.9 | 25.6 / 41.2 | 31.5 / 48.6 / 48.6 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 25.2 / 40.6 / 40.6 | 31.3 / 44.4 | 25.4 / 41.0 | 32.2 / 46.0 / 46.0 | -1.2 [-23.1, +11.6] | +0.7 [-22.1, +12.7] |
|  |  | 1000 → 1480 | `2f0ac97` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 34.7 / 57.2 / 57.2 | 39.6 / 60.8 | 33.9 / 57.6 | 41.0 / 62.1 / 62.1 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 45.9 / 57.7 / 57.7 | 48.0 / 63.8 | 44.9 / 58.0 | 49.3 / 65.0 / 65.0 | +11.1 [-8.9, +29.0] | +8.3 [-11.1, +31.3] |
| 461 × 490, 13 pt | History | 461 → 980 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 46.3 / 77.2 / 77.2 | 55.5 / 70.7 | 48.5 / 61.4 | 79.1 / 101.1 / 101.1 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 45.0 / 64.0 / 64.0 | 53.0 / 67.1 | 43.5 / 57.6 | 63.3 / 75.0 / 75.0 | -1.3 [-25.3, +17.5] | -15.8 [-33.7, +16.9] |
|  |  | 980 → 461 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 25.7 / 32.8 / 32.8 | 29.3 / 44.3 | 20.1 / 34.9 | 49.9 / 64.2 / 64.2 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 21.7 / 23.7 / 23.7 | 40.0 / 44.6 | 29.4 / 36.5 | 40.8 / 45.7 / 45.7 | -4.0 [-20.9, +1.8] | -9.1 [-25.1, -2.4] |
| 461 × 490, 18 pt | History | 461 → 980 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 45.4 / 67.7 / 67.7 | 55.1 / 61.2 | 42.3 / 54.4 | 84.5 / 88.0 / 88.0 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 38.7 / 72.3 / 72.3 | 56.2 / 72.3 | 46.2 / 65.1 | 66.0 / 81.0 / 81.0 | -6.6 [-21.5, +29.5] | -18.4 [-39.0, +18.0] |
|  |  | 980 → 461 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 25.7 / 31.3 / 31.3 | 32.2 / 39.5 | 25.0 / 30.8 | 49.2 / 58.0 / 58.0 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 22.5 / 23.0 / 23.0 | 39.7 / 45.4 | 33.0 / 36.0 | 40.9 / 46.8 / 46.8 | -3.2 [-8.3, +7.1] | -8.3 [-16.9, -0.2] |
|  | Text Compare | 461 → 980 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 34.4 / 55.6 / 55.6 | 48.5 / 61.5 | 39.8 / 53.6 | 67.9 / 91.0 / 91.0 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 33.4 / 45.7 / 45.7 | 48.2 / 64.4 | 41.2 / 54.3 | 56.4 / 77.0 / 77.0 | -1.0 [-24.3, +16.4] | -11.5 [-37.4, +6.0] |
|  |  | 980 → 461 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 16.8 / 33.1 / 33.1 | 28.7 / 45.3 | 17.4 / 38.4 | 45.1 / 64.3 / 64.3 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 15.6 / 24.9 / 24.9 | 34.0 / 46.5 | 24.6 / 37.9 | 34.9 / 47.6 / 47.6 | -1.2 [-16.3, +5.2] | -10.2 [-26.1, +2.3] |
|  | Image Compare | 461 → 980 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 22.6 / 41.6 / 41.6 | 31.9 / 49.1 | 25.4 / 38.6 | 52.2 / 72.2 / 72.2 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 14.9 / 34.0 / 34.0 | 33.3 / 62.5 | 23.4 / 51.4 | 33.8 / 63.5 / 63.5 | -7.7 [-19.7, +7.8] | -18.4 [-40.1, +3.6] |
|  |  | 980 → 461 | `2f0ac97` | 8 | 2 / 2 | 2 (8) | 8 of 8 | 18.4 / 30.0 / 30.0 | 24.7 / 43.0 | 17.3 / 33.6 | 39.1 / 57.9 / 57.9 |  |  |
|  |  |  | `a50eae8` | 8 | 1 / 1 | 1 (8) | 8 of 8 | 20.6 / 25.1 / 25.1 | 35.1 / 45.5 | 25.6 / 36.4 | 35.8 / 46.3 / 46.3 | +2.3 [-11.7, +6.4] | -3.4 [-27.1, +5.8] |

### Release benchmarks (candidate only)

| Benchmark | Series | Rounds | n | Handler p50 / p95 / p99 / max (ms) | Round p50s (ms) | Frames per step | Frames left for the next callback |
| --- | --- | ---: | ---: | --- | --- | --- | --- |
| `width_steps_release_benchmark` | History, 13 pt | 5 | 400 | 2.148 / 2.178 / 2.210 / 2.350 | 2.142–2.155 | 1 (400) | 0 (400) |
|  | Text Compare, 13 pt | 5 | 400 | 2.000 / 2.035 / 2.048 / 2.117 | 1.995–2.002 | 1 (400) | 0 (400) |
|  | Image Compare, 13 pt | 5 | 400 | 3.466 / 4.422 / 4.552 / 4.718 | 3.419–3.597 | 1 (400) | 0 (400) |
|  | History, 18 pt | 5 | 400 | 1.797 / 1.995 / 2.005 / 2.021 | 1.793–1.806 | 1 (400) | 0 (400) |
|  | Text Compare, 18 pt | 5 | 400 | 1.804 / 1.830 / 1.838 / 1.850 | 1.802–1.806 | 1 (400) | 0 (400) |
|  | Image Compare, 18 pt | 5 | 400 | 2.615 / 3.373 / 3.426 / 3.451 | 2.613–2.615 | 1 (400) | 0 (400) |
| `warm_tab_switch_release_benchmark` | 461 × 490, 13 pt | 5 | 200 | 3.201 / 3.355 / 3.390 / 3.432 | 2.861–3.253 | 1 (200) | 0 (200) |
|  | 1480 × 980, 13 pt | 5 | 200 | 3.506 / 3.921 / 3.966 / 4.084 | 3.474–3.553 | 1 (200) | 0 (200) |
|  | 461 × 490, 18 pt | 5 | 200 | 3.031 / 3.382 / 3.582 / 3.590 | 2.727–3.047 | 1 (200) | 0 (200) |
|  | 1480 × 980, 18 pt | 5 | 200 | 3.388 / 3.766 / 3.828 / 3.909 | 3.289–3.476 | 1 (200) | 0 (200) |
| `quiet_landing_release_benchmark` | `500_rows_followed`, `5000_rows` | — | — | not measured: panics in `5000_rows` before writing output (2 of 2 runs) | — | — | — |

`next_frame_ms` was at most 0.004 ms in every series: callback delivery with no frame drawn.

### Interaction phase

#### Per-frame draw, 1480 × 980, 13 pt

| Surface | Build | Paced draws n | Paced p50 / p95 / max (ms) | With 1–2-tick draws p95 / p99 (ms) | Over one tick | Input-led draws n | Input-led p50 / p95 / max (ms) | Paced Δp50 | Δp95 with 1–2-tick draws | Input-led Δp50 |
| --- | --- | ---: | --- | --- | ---: | ---: | --- | --- | --- | --- |
| History | `2f0ac97` | 324 | 5.708 / 8.040 / 8.329 | 12.981 / 13.595 | 227 | 221 | 4.935 / 9.969 / 13.522 |  | | |
|  | `a50eae8` | 263 | 5.794 / 8.062 / 8.286 | 13.170 / 13.850 | 272 | 191 | 5.028 / 10.200 / 19.303 | +0.086 [-0.846, +0.849] | +0.189 [-1.669, +2.478] | +0.093 [-0.425, +0.906] |
| Text Compare | `2f0ac97` | 68 | 5.315 / 7.535 / 7.963 | 11.559 / 14.595 | 97 | 8 | 6.742 / 13.070 / 13.070 |  | | |
|  | `a50eae8` | 101 | 5.056 / 7.926 / 8.325 | 11.564 / 12.307 | 75 | 8 | 4.815 / 11.054 / 11.054 | -0.259 [-1.261, +0.771] | +0.005 [-1.363, +0.413] | -1.927 [-6.837, +5.520] |
| Image Compare | `2f0ac97` | 50 | 7.032 / 7.908 / 8.157 | 15.351 / 16.172 | 103 | 36 | 7.747 / 19.886 / 20.829 |  | | |
|  | `a50eae8` | 45 | 7.115 / 7.744 / 8.272 | 15.138 / 15.812 | 93 | 36 | 8.886 / 20.904 / 20.949 | +0.083 [-0.651, +0.496] | -0.213 [-0.905, +0.399] | +1.139 [-4.002, +4.589] |

#### Frames, UI CPU, input to first frame request and main-thread allocations per interaction, 1480 × 980, 13 pt

| Interaction | Build | n | Frames (count of windows) | UI CPU p50 / p95 / max (ms) | Input to first frame request p50 / p95 (ms) | Allocation calls per window p50 / p95 | Allocation calls per frame p50 | Δ UI CPU p50 (ms) | Δ first request p50 (ms) | Δ calls per window p50 |
| --- | --- | ---: | --- | --- | --- | --- | --- | --- | --- | --- |
| History wheel notch | `2f0ac97` | 240 | 1 (240) | 10.1 / 15.8 / 19.0 | 8.70 / 13.75 | 11506 / 11642 | 11506 |  | | |
|  | `a50eae8` | 240 | 1 (240) | 11.8 / 15.9 / 18.0 | 9.87 / 13.88 | 11504 / 11638 | 11504 | +1.7 [-5.0, +5.3] | +1.17 [-4.61, +4.76] | -2 [-39, +35] |
| History held arrow, 1 s (commit selection) | `2f0ac97` | 16 | 43 (1), 44 (1), 45 (2), 46 (1), 47 (3), 48 (1), 49 (1), 50 (3), 51 (1), 53 (1), 55 (1) | 408.5 / 461.5 / 461.5 | 5.49 / 12.89 | 553116 / 638064 | 11669 |  | | |
|  | `a50eae8` | 16 | 43 (1), 44 (2), 45 (1), 46 (4), 47 (2), 48 (3), 51 (2), 100 (1) | 413.9 / 472.4 / 472.4 | 5.42 / 19.30 | 537772 / 593053 | 11670 | +5.4 [-47.2, +73.7] | -0.06 [-3.67, +5.15] | -15344 [-58080, +21306] |
| Text wheel notch | `2f0ac97` | 80 | 1 (62), 2 (14), 18 (1), 19 (1), 20 (2) | 7.8 / 23.7 / 181.0 | 5.84 / 11.60 | 8960 / 16878 | 8919 |  | | |
|  | `a50eae8` | 80 | 1 (60), 2 (16), 19 (1), 20 (1), 21 (1), 24 (1) | 9.3 / 24.4 / 182.1 | 6.19 / 11.44 | 8953 / 16894 | 8745 | +1.5 [-2.2, +4.3] | +0.35 [-0.95, +2.50] | -7 [-38, +44] |
| Text mode (Split, Diff) | `2f0ac97` | 8 | 2 (4), 19 (2), 20 (1), 24 (1) | 20.5 / 223.6 / 223.6 | 6.74 / 13.07 | 29296 / 231194 | 9881 |  | | |
|  | `a50eae8` | 8 | 2 (4), 20 (1), 21 (1), 23 (1), 24 (1) | 23.8 / 214.4 / 214.4 | 4.82 / 11.05 | 29311 / 231364 | 9828 | +3.4 [-199.4, +191.8] | -1.93 [-7.04, +5.52] | +15 [-167109, +193367] |
| Image zoom (100%, 200%) | `2f0ac97` | 8 | 2 (8) | 25.0 / 39.9 / 39.9 | 12.07 / 18.17 | 16831 / 16921 | 8416 |  | | |
|  | `a50eae8` | 8 | 2 (8) | 24.4 / 35.7 / 35.7 | 7.11 / 15.29 | 16845 / 16944 | 8422 | -0.7 [-13.2, +12.3] | -4.96 [-7.07, +5.13] | +14 [-76, +104] |
| Image pan drag at 200% | `2f0ac97` | 16 | 110 (1), 115 (1), 117 (1), 121 (2), 122 (2), 124 (2), 125 (2), 126 (3), 128 (2) | 235.1 / 256.0 / 256.0 | 9.38 / 20.83 | 155904 / 166715 | 1278 |  | | |
|  | `a50eae8` | 16 | 109 (1), 111 (1), 112 (1), 115 (1), 116 (2), 120 (3), 121 (1), 122 (1), 124 (1), 125 (2), 126 (1), 127 (1) | 231.2 / 263.2 / 263.2 | 10.29 / 20.95 | 145619 / 163243 | 1227 | -3.9 [-22.1, +28.3] | +0.92 [-6.95, +6.91] | -10285 [-21475, +2417] |
| Image wheel notch at 200% | `2f0ac97` | 24 | 1 (24) | 17.9 / 22.0 / 22.1 | 17.37 / 20.92 | 8492 / 8509 | 8492 |  | | |
|  | `a50eae8` | 24 | 1 (24) | 11.2 / 22.2 / 22.7 | 10.52 / 21.03 | 8500 / 8516 | 8500 | -6.7 [-11.6, +3.2] | -6.85 [-11.03, +2.88] | +8 [-4, +15] |
| Image mode (Overlay, Side by side) | `2f0ac97` | 8 | 2 (8) | 22.4 / 35.9 / 35.9 | 7.01 / 13.13 | 17189 / 17433 | 8594 |  | | |
|  | `a50eae8` | 8 | 2 (8) | 19.8 / 43.0 / 43.0 | 8.55 / 20.90 | 17202 / 17447 | 8601 | -2.6 [-11.3, +11.1] | +1.54 [-0.24, +11.87] | +13 [-231, +258] |
| Image pan drag, Overlay | `2f0ac97` | 4 | 118 (1), 123 (1), 126 (2) | 211.5 / 262.5 / 262.5 | 7.30 / 7.75 | 149323 / 177585 | 1265 |  | | |
|  | `a50eae8` | 4 | 119 (2), 131 (2) | 182.4 / 235.3 / 235.3 | 7.76 / 10.53 | 166390 / 204639 | 1372 | -29.1 [-80.1, +23.8] | +0.46 [-0.12, +3.23] | +17067 [-19478, +55316] |

#### Live resizes with a refused write's report on screen, 600 ↔ 461 × 680 tiled, 13 pt

| Report | Phase | Step | Build | n | Frames (count of resizes) | UI CPU p50 / p95 / max (ms) | Last frame after the first command p50 / p95 (ms) | Allocation calls per resize p50 / p95 | Allocated MiB per resize p50 | Δ UI CPU p50 (ms) | Δ last frame p50 (ms) | Δ calls p50 |
| --- | --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | --- | --- |
| 16 KiB, one line | notice shown | 600 → 461 | `2f0ac97` | 24 | 2 (24) | 78.8 / 114.4 / 117.1 | 145.6 / 182.6 | 38029 / 38633 | 48.8 |  | | |
|  |  |  | `a50eae8` | 24 | 1 (24) | 77.1 / 110.0 / 110.4 | 127.0 / 172.9 | 29417 / 30115 | 43.9 | -1.7 [-25.2, +27.5] | -18.6 [-49.7, +22.2] | -8612 [-8613, -8603] |
|  |  | 461 → 600 | `2f0ac97` | 24 | 2 (22), 4 (2) | 92.6 / 119.3 / 119.6 | 154.6 / 188.3 | 39307 / 57523 | 49.8 |  | | |
|  |  |  | `a50eae8` | 24 | 1 (24) | 78.2 / 110.1 / 116.1 | 135.2 / 170.2 | 30242 / 30270 | 44.5 | -14.4 [-39.6, +14.5] | -19.4 [-52.8, +9.0] | -9065 [-9067, -9054] |
|  | dismissed (control) | 600 → 461 | `2f0ac97` | 12 | 2 (12) | 72.7 / 116.6 / 116.6 | 146.9 / 208.7 | 37790 / 37791 | 48.5 |  | | |
|  |  |  | `a50eae8` | 12 | 1 (12) | 79.9 / 108.7 / 108.7 | 127.4 / 186.5 | 29272 / 29373 | 43.8 | +7.1 [-38.8, +36.6] | -19.5 [-52.3, +42.1] | -8518 [-8525, -8422] |
|  |  | 461 → 600 | `2f0ac97` | 12 | 2 (12) | 94.7 / 119.3 / 119.3 | 170.8 / 189.1 | 39057 / 39058 | 49.5 |  | | |
|  |  |  | `a50eae8` | 12 | 1 (12) | 62.8 / 97.7 / 97.7 | 116.1 / 145.6 | 30086 / 30089 | 44.3 | -31.9 [-63.0, +2.5] | -54.7 [-69.9, -12.7] | -8971 [-8979, -8960] |
| 550 lines | notice shown | 600 → 461 | `2f0ac97` | 24 | 2 (24) | 91.3 / 115.2 / 117.0 | 166.7 / 193.0 | 37940 / 38547 | 48.8 |  | | |
|  |  |  | `a50eae8` | 24 | 1 (24) | 65.5 / 102.5 / 108.8 | 126.0 / 175.4 | 29348 / 30050 | 43.9 | -25.8 [-37.6, +1.6] | -40.7 [-62.5, -3.1] | -8592 [-8600, -8588] |
|  |  | 461 → 600 | `2f0ac97` | 24 | 2 (22), 4 (2) | 79.9 / 112.9 / 113.5 | 136.7 / 182.4 | 39210 / 57041 | 49.7 |  | | |
|  |  |  | `a50eae8` | 24 | 1 (24) | 76.4 / 102.3 / 106.9 | 132.6 / 156.5 | 30165 / 30193 | 44.4 | -3.5 [-22.2, +21.6] | -4.2 [-35.9, +22.7] | -9045 [-9047, -9035] |
|  | dismissed (control) | 600 → 461 | `2f0ac97` | 12 | 2 (12) | 83.1 / 111.7 / 111.7 | 149.4 / 202.0 | 37787 / 37790 | 48.5 |  | | |
|  |  |  | `a50eae8` | 12 | 1 (12) | 83.0 / 102.4 / 102.4 | 141.6 / 192.5 | 29275 / 29371 | 43.8 | -0.2 [-30.6, +22.1] | -7.9 [-41.7, +7.6] | -8512 [-8516, -8425] |
|  |  | 461 → 600 | `2f0ac97` | 12 | 2 (12) | 78.9 / 110.3 / 110.3 | 139.9 / 180.0 | 39055 / 39058 | 49.5 |  | | |
|  |  |  | `a50eae8` | 12 | 1 (12) | 88.0 / 111.4 / 111.4 | 134.1 / 171.3 | 30086 / 30089 | 44.3 | +9.1 [-36.3, +36.6] | -5.8 [-59.7, +42.9] | -8969 [-8979, -8959] |

### Outliers kept

- Every frame and window is kept.
- One candidate held-arrow window (`f2-a50eae8-L13`, Up, the fourth hold) drew 100 frames, with its last frame 1,384.7 ms after the key. A scroll animation was still running at the end of the hold. It is that row's frame maximum and p95 UI CPU (456.2 ms).
- Text Split drew 19–24 frames in both builds, and Diff 2. One text wheel window per launch drew 18–24 frames in both builds: the scroll's own animation.
- In 2 of 24 base resizes per report at 461 → 600, 4 frames were drawn with the notice shown; the candidate drew 1 in all of them.

### Limits

- One host, one display at 120 Hz, Midnight only, warm cache. 4 launches per build and cell for the width steps, 4 per build for the full protocol (L13 only), and 4 per build and report for the hook resizes.
- The UI thread ran at 1.2–1.5 GHz at p50, below 3.0 GHz in 94–99% of windows in both builds. Launches were pinned to all eight performance cores (4–11).
- The session was suspended for 4 h 49 min between the width steps and the rest. The benchmarks and the interaction phase therefore ran in a later window than the width steps, each phase comparing both builds within itself.
- PC1, PC2 and CH2 landed before the base, so their rows check for regressions since and do not measure those commits. NT3's allocation saving is below this run's noise of about ±40 calls per frame. Its own counting-allocator test is the finer evidence.
- NT4's `tag-preparation-error` line was not exercised with a long report, because no user path puts a hook's report there. The live resizes measure the window-wide notice that does show it. A native resize burst of seven `hyprctl` commands is not a pointer drag on a border, though both deliver a configure per step.
- `quiet_landing_release_benchmark` has no result. No base figure exists for any of the three benchmarks.
- The allocation counter counts calls, not frees or peak heap, and only the main thread.
- The per-launch Wayland logs, marks, screenshots and fixture copies stayed on the measuring host. Screenshots hold local paths and were not privacy-checked for publication.
- Product source was not changed.

### Commands

```sh
cd .local/bench-ws1-2026-11-01
# width steps (tools/ = the 2026-10-30 rerun's tools with base 2f0ac97 / candidate a50eae8)
GITTURTLE_RELEASES=$PWD/../item8-2026-10-27 tools/run-all.sh 1 4
python3 tools/analyse.py r*-*; gzip raw/frames.csv
python3 tools/summary.py > raw/summary.txt; python3 tools/tables.py > raw/tables.md
# benchmarks (round 1 with quiet landing, rounds 2-5 without it)
BENCH_QL=1 tools2/run-bench.sh 1; tools2/run-bench.sh 2 ... 5; python3 tools2/bench-summary.py > raw/bench/summary.txt
# interaction phase (ALLOCCOUNT=1 preloads tools/alloccount/alloccount.so, built with gcc -O2 -shared -fPIC)
tools2/make-hook-fixtures.sh
GITTURTLE_RELEASES=$PWD/../item8-2026-10-27 tools2/run-phase3.sh 1 4
ANALYSE_OUT=raw/phase3 python3 tools2/analyse.py f*-* h*-*
SUMMARY_RAW=raw/phase3 python3 tools2/summary.py > raw/phase3/summary.txt
python3 tools2/phase3-summary.py raw/phase3 > raw/phase3/phase3-table.md; python3 tools2/section-tables.py > raw/phase3/section-tables.md
```

The run used `run-bench.sh` with the round as its argument. `BENCH_QL` was a later edit that put round 1's quiet-landing run behind a switch; round 1 ran it unconditionally.
