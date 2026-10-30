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
