# Image comparison redraws laid out in the frame, Wayland, 2026-10-29

This is the release measurement for the System preview round, integrated as `c470e3be..81db2e39` on `claude/omarchy-support`, against `6660805d`, the parent of `c470e3be`. The measured difference `6660805d..81db2e39` therefore also contains `c470e3be` (`fix(app): write an open failure's short line from its typed category`: `system_preview.rs`, `system_preview/launch.rs`, `system_preview/store.rs`, `views.rs`). The round changes how an image comparison is laid out in every frame. The whole image preview, its toolbar included, is now built inside an `InFrame` element during prepaint, at the frame's own column width (`crates/app/src/in_frame.rs`; `views.rs` `render_preview`, about lines 2828–2844). The captions are laid out up to three times per frame: two throwaway `in_frame::measured_height` layouts, then the drawn one, plus `wrapped_lines`/`one_line_width` text measurement (`image_compare.rs`, about lines 760–790). The question: does an image comparison cost more per redrawn frame, and does it draw more frames, than in its parent?

The data is in [`2026-10-29-image-compare-redraw/`](2026-10-29-image-compare-redraw/): per-window rows in `raw/windows.csv`, per-frame rows in `raw/frames.csv.gz`, statistics in `raw/summary.json`, `raw/summary.txt` and `raw/tables.md` (the tables below), the run log `raw/runs.txt` and the load in `raw/load-per-launch.txt`. The scripts are in `tools/`. The per-launch Wayland logs and samples (`raw/<launch>.wl.log`, `raw/<launch>.marks.json`), the calibration (`raw/cal/`), the exploratory and trial launches (`raw/x*`, `raw/w-*`, `raw/t*`, `raw/trial/`) and the fixture copies stayed on the measuring host: they hold local paths and are large.

## Result

- **Cost per redrawn frame: higher by about 0.1–0.15 ms at 1480 px, 0.35 ms on the changed PNG at 461 × 490.** The candidate's pooled draw p50 is higher in all nine fixture × cell pools. The 95% interval excludes zero in six of them: +0.147 ms [+0.033, +0.222] on the changed PNG at 13 pt (5.23 → 5.37 ms, +2.8%); +0.156 [+0.074, +0.241] at 18 pt (4.23 → 4.38, +3.7%); +0.114 [+0.044, +0.239] on the added PNG at 18 pt; +0.138 [+0.059, +0.243] and +0.141 [+0.083, +0.208] during GIF playback at 13 and 18 pt; and +0.345 [+0.138, +0.537] on the changed PNG at 461 × 490 (2.30 → 2.64 ms, +15%, from only 48–53 draws per build). The other three pools, the added PNG at 13 pt and at 461 × 490 and the GIF at 461 × 490, also point up (+0.08, +0.08 and +0.13 ms), with intervals that include zero. The shift is no larger than the range of per-launch medians within one build (0.02–0.44 ms at 1480 px), but the launch medians shift as a whole. The two-level bootstrap resamples launches. The delta also holds within one frequency stratum: in windows that started at 3.0 GHz or more, +0.118 ms at 13 pt, +0.082 at 18 pt and +0.199 at 461 × 490; below 3.0 GHz, +0.169, +0.225 and +0.203. So the shift does not come from the candidate having had a few more low-frequency windows (330 of 896, against 287 of 896 for the base).
- **Tails: no change resolved.** The p95 and p99 intervals with 1–2-tick draws include zero in seven of nine pools for p95 and in all nine for p99. The exceptions are p95 on the added PNG at 18 pt (+0.92 [+0.04, +1.55]) and on the GIF at 18 pt (+0.26 [+0.01, +0.45]); of the more than 100 intervals in these tables, several would exclude zero by chance alone. The largest draw is 11.6 ms in the base and 12.3 ms in the candidate. Draws over one 8.33 ms tick: 74 base, 89 candidate, over 10,866 and 10,687 draws, with no consistent direction per cell (24/28, 14/21, 24/31 at 13 pt; 7/3, 0/1, 5/5 at 18 pt; 0/0 at 461 × 490).
- **Frames per interaction: unchanged, except one fewer when narrowing to 461 × 490.** Pan drags, wheel notches, zoom and mode choices, GIF playback and pauses draw the same numbers of frames and draws (every Δp50 interval includes zero; drags −2 frames [−11, +6], GIF playback −3 [−10, +1] per 4 s). Width steps at 1480 ↔ 1000 draw 1 frame narrowing and 3 widening in both builds, on every step. Narrowing from 700 to 461 px, where the toolbar's buttons become menus, the base drew 3 frames on all 32 steps and the candidate 2 on all 32. The UI thread's CPU for that step fell by 1.5 ms [−2.5, −0.8] (11.5 → 10.0 ms p50), and its last frame came 8 ms sooner at p50 (42.8 → 34.8 ms after the command). Widening 461 → 700 drew 2 frames in both.
- **UI-thread CPU per interaction follows the draw.** Most intervals include zero. Two exclude it, both where many draws accumulate: a 200% pan drag at 18 pt costs +5.7 ms [+0.9, +9.5] on 121 ms (+4.7%; 19 draws × 0.19 ms ≈ 3.6 ms), and 4 s of GIF playback at 18 pt costs +38 ms [+7, +64] on 1,564 ms (+2.4%; 351 draws × 0.14 ms ≈ 49 ms). A zoom choice at 461 × 490 costs +1.7 ms [+0.4, +2.7] on 15.1 ms (5 draws × 0.35 ms ≈ 1.7 ms).
- **Behaviour at 461 × 490, which these numbers do not price.** In the base, the PNG comparison draws no image at 461 × 490, even at Fit: the stage gets no room, and the footer shows where the image would be. The candidate draws the images at Fit. Zoomed, the candidate's toolbar adds a row of pan arrows and its stage gives way. With the GIF, both builds draw no image at 461 × 490: the GIF controls band fills the column, so Play GIF draws 2 frames and no animation in either build. Drag and wheel on the image were therefore measured only at 1480 px. At 461 × 490 the per-frame figures come from zoom and mode choices and width steps, and in the PNG cells the candidate's frames there include images the base did not draw.

## Verdict

The candidate costs a small, consistent amount more per redrawn image-comparison frame: about 0.1–0.15 ms on 4–5.4 ms draws (2–4%) at 1480 × 980, and up to 0.35 ms at 461 × 490. It draws no more frames for any interaction measured, and one fewer when the width crosses the toolbar's menu threshold at 461 px. On this host the extra cost keeps p50 draws at 2.6–5.4 ms within the 8.33 ms tick, and the tails show no resolved change. In frames per second this is neutral: a pan drag draws as many frames as before, and GIF playback's frame count is unchanged within noise. As CPU, it is a measurable regression of a few percent on every image redraw, visible as UI-thread CPU only where draws accumulate (drag at 18 pt, playback).

Likely cause (inferred from the diff, not profiled). The work added per frame is the `InFrame` rebuild of the whole preview at prepaint (`in_frame.rs`, `InFrame::prepaint`: a separate `layout_as_root` of the toolbar, captions and stage), plus the two throwaway caption layouts and the text measurement in `image_compare.rs` (the `Beneath { names: measured_height(..), dimensions: measured_height(..), .. }` block and the `wrapped_lines`/`one_line_width` calls above it). During a pan drag or GIF playback, none of these inputs change from frame to frame: width, text size, captions and answers are the same. The smallest fix to try is to cache the caption-fit inputs (`names`/`dimensions` heights, `kept_wraps`, the note `wraps` and the band lines) in the comparison's state, keyed by column width, text size and the caption contents. A frame whose key is unchanged would then skip the two `measured_height` layouts and the text shaping. Whether that recovers the 0.1–0.35 ms needs a profile or a rerun of this protocol; the `InFrame` layout itself may account for part of it. Product source was not changed.

## Setup

- **Host:** Apple M2 Pro (CPUs 0–3 efficiency at 2.42 GHz maximum, 4–11 performance at 3.26 GHz maximum), 31 GiB, Asahi Linux (Omarchy) on Linux 7.1.12-2-7-ARCH, `schedutil` governor, Hyprland 0.56.2, native Wayland. Display: built-in eDP-1, 3456 × 2160 at 120 Hz, scale 2, so one frame tick is 8.33 ms. Mesa 26.2.2 (`vulkan-asahi`), libwayland 1.26.0, Git 2.55.0.
- **Load:** the IDE hosting the session (`orca-ide`) used about one core throughout. A build hold (`.local/build-slots/quiet`) was in place, and no `rustc` or `cargo` process ran at any launch start (`raw/runs.txt`, `builds 0` on all 72). One-minute load average 2.24–4.08 at launch starts and ends. Cores 4–11 were 72–79% idle (median 76%) over the 2 s before each launch. Window-start frequency of the UI thread's CPU: 3.264 GHz at p50 in both builds, below 3.0 GHz in 287 of 896 base windows and 330 of 896 candidate windows (stratified above). One more load overlapped the runs: from 19:15 UTC until after the last launch (20:18), a single-threaded preliminary analysis of round 1 (`summary.py`, a version that scanned every row per key) ran pinned to efficiency cores 0–3. It shared no core with the app (4–11) but did share the memory system, and both builds ran under it in alternation.
- **Builds:** release executables built from clean trees (`--build-info`), each confirmed to embed its full revision (`strings <exe> | grep <sha>`; neither embeds the other's), base designated before measuring:
  - Base `6660805d2de5bd16a6137176444a3aed591a523a`, `source_tree` clean, aarch64-unknown-linux-gnu, release, rustc 1.99.0 (b940084d7 2026-09-28), build_unix_seconds 1793296598, sha256 `ea04c341889649f52d2d67addc963c806f521915c625c7ebba7e19da0c103faf`.
  - Candidate `81db2e39358513771a57d6545916b8658081cbed`, `source_tree` clean, the same target, profile and rustc, build_unix_seconds 1793296679, sha256 `1cdb0edb0cdaea709775b8a327d711a04260b8730ed8f660ae6cd47f1c8758d9`.
- **Fixtures** (`tools/make-fixtures.sh` with `tools/make-images.py`, deterministic, disposable, a fresh copy of the pristine repository per launch). Each has 10 commits of `README.md`, then:
  - *PNG changed* (`png`, HEAD `c142c57`): `photo.png` added, then changed (1600 × 1000 RGB, 86,519 → 87,856 bytes). Both sides drawn.
  - *PNG added* (`add`, HEAD `efcebc7`): HEAD adds `photo.png` (the same 1600 × 1000 image). Before absent.
  - *GIF changed* (`gif`, HEAD `73062c1`): `anim.gif` added, then changed (640 × 400, 24 frames of 50 ms, both sides animated).
- **Cells:** *1480 × 980 at 13 pt* and *1480 × 980 at 18 pt*: the window floats, centred and forced opaque on workspace 9. *461 × 490 at 18 pt*: the window is tiled beside one `foot` filler and above another, because tiling ignores the app's 1000 × 680 minimum window size (`main.rs`, `window_min_size`), and is resized to exactly 461 × 490.
- **Launch** (`tools/common.py`, `tools/drive.py`): `taskset -c 4-11`, `NO_AT_BRIDGE=1`, `WAYLAND_DEBUG=1`. Empty absolute `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_STATE_HOME`/`XDG_CACHE_HOME` per launch, with fontconfig linked and preferences from the native-QA template: Midnight, follow-system off, interface text 13 or 18 pt, project pane off. History opens with HEAD selected. The driver clicks the changed file to enter Compare, and checks a screenshot against the calibration's (mean grey difference under 12 of 255; every launch opened on the first click, differences 0.0–0.21).
- **Cache state:** warm. Each launch reads its fixture copy just after it was copied, and the executables had been run repeatedly beforehand. Each launch is a new process, so its in-memory preview cache starts empty and it decodes the images afresh.
- **Order:** 4 rounds, 18:44:56–20:18:40 UTC. Each round runs every cell (1480 × 980 at 13 pt, at 18 pt, 461 × 490 at 18 pt) × fixture (PNG changed, PNG added, GIF changed), base then candidate, so each cell's launches alternate ABAB…ABAB (8 launches per cell × fixture, 72 in all). No launch failed, was dropped or was repeated.

## Method and boundaries

Per launch (`tools/drive.py`'s SEQUENCE). Idle 2 s. Then zoom choices, Fit → 100% → 50% → Fit → 200% and once more (at 461 × 490: 100% → 50% → 200% → Fit; the GIF: 100% → Fit twice), each as: pointer on the target, 1.2 s, sample, press and release, 1.5 s, sample. At 461 × 490, a zoom or mode choice opens its menu and picks the item, two clicks 0.6 s apart. Then, on the PNGs at 1480 px only, at 200%: six pan drags, each pressing at one end of a 160 px horizontal stroke through the left (changed) or After (added) side, with 40 moves of 4 px about 7 ms apart, release, 1.0 s; then ten wheel notches over the image, alternating down and up, 1.0 s each (the wheel pans; zoom has no wheel binding). Then Overlay, two pan drags in Overlay, Side by side. Then width steps 1480 → 1000 → 1480 … (3 each way on the PNGs, 2 on the GIF) at 1480 px, or 700 ↔ 461 at 461 × 490, each 1.5 s with the pointer parked on the status bar. Then idle 2 s. The GIF launches instead run Play GIF (4.0 s window) and Pause GIF (1.5 s) three times, after the zoom choices (at 1480 px) and before the width steps. Click targets were calibrated per build, cell and fixture with the accessibility bus on (`tools/calibrate-all.sh`, which resolves each target by its accessible name just before clicking it, in sequence). Measured launches replay those points. Both builds calibrated to identical points in every cell. The accessibility bus was set back to `IsEnabled false` after calibrating and read back as `false` before the measured rounds.

Neither build prints a `GITTURTLE_TRACE` metric on this path. Every frame figure comes from the app's own `WAYLAND_DEBUG` log, identical for both builds, and every CPU figure from `/proc/<pid>/task/<pid>/schedstat` (`tools/analyse.py`, `tools/summary.py`, `tools/tables.py`):

- **Frame:** one `wl_surface.attach` on the window's main surface (the one passed to `xdg_wm_base.get_xdg_surface`). This is Mesa's present of one GPUI frame. Menus' popup surfaces are not counted.
- **Draw** and **present without a draw.** After a pan drag's release, both builds keep answering every `wl_callback.done` with a `wl_surface.frame` request within 0.056 ms and an attach about 0.2 ms later. That is too soon for `Window::draw` to have run, so GPUI presented again without drawing. This went on at 120 Hz until the pointer next moved, which in this protocol is the end of the drag's window: about 109 such presents per drag in both builds, untraced. A frame whose request follows its `done` within 0.5 ms is therefore counted as a `present`, a frame but not a draw, and gives no draw time. Over all 49,515 done-triggered gaps, every gap was either at most 0.056 ms or at least 1.735 ms, so the 0.5 ms split is not sensitive.
- **Per-frame draw** (`gap_ms` of `paced` frames, the draw tables): from libwayland's dispatch of a `wl_callback.done` for a callback requested on the main surface, to the app's next `wl_surface.frame` request on that surface, when that request comes 0.5 ms to one tick (8.33 ms) after the `done`. GPUI 0.3.4's Wayland window sends that request in its platform `draw`, after `Window::draw` returned and before the renderer encodes. The value therefore covers GPUI's frame handling, including next-frame callbacks and the whole window's request_layout, layout, prepaint (with the candidate's `InFrame` build and its caption measurements) and paint, plus the debug print of the `done` line. It excludes renderer encoding, submission and Mesa's present (request to attach, `present_ms`, about 0.2 ms), GPU execution, compositor latching and presentation, input delivery, and the wait for the tick that delivers the `done`. The value starts at a tick, so the tick grid puts no floor under it. Its classification has a ceiling. A gap of one to two ticks (8.33–16.67 ms) is `ambiguous`, since a parked frame loop woken by a task looks the same in the log. Those gaps are reported only in the view "with 1–2-tick draws" and in "Draws over one tick". Longer gaps are `woken` and give no draw time. The paced view's maximum is therefore at most 8.33 ms by construction, and neither view could show a draw longer than 16.67 ms. The classification applies identically to both builds. Pooled draws include every interaction but idle: zoom, mode, drag, wheel, playback, pause and width steps.
- **Input to first frame request** (`first_request_ms`): from libwayland's dispatch of the app's own press (zoom, mode, drag, play, pause), first axis event (wheel) or `xdg_toplevel.configure` (width step), through the handler and the draw, to the first frame request on the main surface. On this platform GPUI draws for an input event without waiting for a frame callback. In 896 of the 944 press windows (all but 48 GIF pauses), the first frame followed the press with no `done` in between, and so did all 320 wheel frames. Those values have no tick floor. A pause pressed during playback lands inside the running frame loop, so its value can include up to one tick of waiting.
- **Frames and draws per interaction:** main-surface attaches (and those that were draws) from the window's start to the driver's sample at its end. The start is the app's own first press dispatch for clicks and drags, the first axis dispatch for the wheel, and the driver's sample before the resize command for width steps. Windows end 1.5 s after a click, 1.0 s after a drag's release or a wheel notch, 4.0 s after Play GIF and 1.5 s after a width command. A frame later than the window would be missed. In every width step the last frame came within 74 ms of the command.
- **UI CPU per interaction:** the main thread's schedstat run time between the driver's two samples. For a click that is from just before the press to the window's end. It includes the handler, every frame drawn or presented, the worker results' handling on that thread, and the libwayland debug formatting. The idle windows read 0.04–0.18 ms over 2 s and drew no frames, in every launch of both builds.

Statistics: pooled nearest-rank percentiles. Deltas use a two-level bootstrap: launches with replacement within each build, then samples within each drawn launch; 4,000 resamples, seed 1; the interval is the 2.5th to 97.5th percentile.

## Tables

### Per-frame draw, every interaction pooled

| Cell | Fixture | Build | Draws n | p50 / p95 / max (ms), paced | Launch p50s | p50 / p95 / p99 / max (ms), with 1–2-tick draws | Draws over one tick |
| --- | --- | --- | ---: | --- | --- | --- | ---: |
| 1480 × 980, 13 pt | PNG changed | `6660805` | 563 | 5.226 / 7.428 / 8.322 | 5.20–5.29 | 5.264 / 8.070 / 9.608 / 11.015 | 24 |
|  |  | `81db2e3` | 551 | 5.373 / 7.639 / 8.318 | 5.29–5.42 | 5.401 / 8.318 / 9.804 / 10.829 | 28 |
|  | PNG added | `6660805` | 591 | 5.089 / 7.348 / 8.323 | 5.03–5.22 | 5.097 / 7.719 / 9.177 / 10.134 | 14 |
|  |  | `81db2e3` | 576 | 5.169 / 7.420 / 8.310 | 5.14–5.29 | 5.197 / 8.048 / 9.327 / 11.383 | 21 |
|  | GIF changed | `6660805` | 3,933 | 4.840 / 6.990 / 8.317 | 4.80–4.90 | 4.844 / 7.053 / 8.026 / 11.550 | 24 |
|  |  | `81db2e3` | 3,865 | 4.978 / 7.163 / 8.332 | 4.90–5.12 | 4.984 / 7.268 / 8.237 / 10.085 | 31 |
| 1480 × 980, 18 pt | PNG changed | `6660805` | 614 | 4.227 / 6.327 / 8.283 | 4.21–4.29 | 4.241 / 6.541 / 8.447 / 10.417 | 7 |
|  |  | `81db2e3` | 622 | 4.383 / 6.476 / 8.024 | 4.34–4.46 | 4.383 / 6.633 / 7.773 / 10.983 | 3 |
|  | PNG added | `6660805` | 626 | 4.128 / 5.850 / 7.905 | 4.12–4.14 | 4.128 / 5.850 / 7.273 / 7.905 | 0 |
|  |  | `81db2e3` | 616 | 4.242 / 6.707 / 8.145 | 4.14–4.58 | 4.247 / 6.774 / 7.707 / 8.653 | 1 |
|  | GIF changed | `6660805` | 4,406 | 3.863 / 5.541 / 8.169 | 3.81–3.89 | 3.864 / 5.550 / 6.657 / 10.421 | 5 |
|  |  | `81db2e3` | 4,344 | 4.004 / 5.788 / 7.836 | 3.93–4.08 | 4.005 / 5.806 / 6.939 / 12.276 | 5 |
| 461 × 490, 18 pt | PNG changed | `6660805` | 53 | 2.298 / 3.273 / 3.549 | 2.23–2.34 | 2.298 / 3.273 / 3.549 / 3.549 | 0 |
|  |  | `81db2e3` | 48 | 2.643 / 3.619 / 4.884 | 2.58–2.69 | 2.643 / 3.619 / 4.884 / 4.884 | 0 |
|  | PNG added | `6660805` | 56 | 2.325 / 3.623 / 4.655 | 2.20–2.44 | 2.325 / 3.623 / 4.655 / 4.655 | 0 |
|  |  | `81db2e3` | 49 | 2.402 / 3.648 / 4.743 | 2.32–2.51 | 2.402 / 3.648 / 4.743 / 4.743 | 0 |
|  | GIF changed | `6660805` | 24 | 2.386 / 2.986 / 3.049 | 2.37–2.45 | 2.386 / 2.986 / 3.049 / 3.049 | 0 |
|  |  | `81db2e3` | 16 | 2.518 / 2.865 / 2.865 | 2.42–2.54 | 2.518 / 2.865 / 2.865 / 2.865 | 0 |

### Candidate minus base, per-frame draw

| Cell | Fixture | Draw Δp50 (ms) | Δp95, with 1–2-tick draws (ms) | Δp99, with 1–2-tick draws (ms) |
| --- | --- | --- | --- | --- |
| 1480 × 980, 13 pt | PNG changed | +0.147 [+0.033, +0.222] | +0.248 [-0.604, +0.978] | +0.196 [-1.342, +1.179] |
| 1480 × 980, 13 pt | PNG added | +0.080 [-0.020, +0.203] | +0.329 [-0.511, +0.922] | +0.150 [-0.562, +2.284] |
| 1480 × 980, 13 pt | GIF changed | +0.138 [+0.059, +0.243] | +0.215 [-0.031, +0.530] | +0.211 [-0.250, +0.504] |
| 1480 × 980, 18 pt | PNG changed | +0.156 [+0.074, +0.241] | +0.092 [-0.912, +0.823] | -0.674 [-2.173, +0.398] |
| 1480 × 980, 18 pt | PNG added | +0.114 [+0.044, +0.239] | +0.924 [+0.039, +1.548] | +0.434 [-0.435, +1.101] |
| 1480 × 980, 18 pt | GIF changed | +0.141 [+0.083, +0.208] | +0.256 [+0.014, +0.445] | +0.282 [-0.099, +0.637] |
| 461 × 490, 18 pt | PNG changed | +0.345 [+0.138, +0.537] | +0.346 [-0.161, +2.048] | +1.335 [-0.006, +1.834] |
| 461 × 490, 18 pt | PNG added | +0.077 [-0.052, +0.249] | +0.025 [-1.439, +1.678] | +0.088 [-1.385, +1.447] |
| 461 × 490, 18 pt | GIF changed | +0.132 [-0.009, +0.298] | -0.121 [-0.452, +0.310] | -0.184 [-0.460, +0.226] |

### Frames, draws and UI-thread CPU per interaction

| Cell | Interaction | Build | n | Frames p50 / p95 (min–max) | Draws p50 (min–max) | UI CPU p50 / p95 / max (ms) | Input to first frame request p50 / p95 / max (ms) | Δ UI CPU p50 (ms) | Δ draw p50 (ms) |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- |
| 1480 × 980, 13 pt | Pan drag at 200%, PNG changed | `6660805` | 24 | 128 / 132 (116–132) | 18 (17–20) | 137.1 / 151.0 / 154.7 | 5.15 / 8.33 / 9.01 |  |  |
|  |  | `81db2e3` | 24 | 126 / 133 (98–134) | 18 (16–20) | 138.0 / 150.7 / 156.1 | 5.74 / 7.66 / 7.77 | +0.9 [-7.9, +8.7] | +0.161 [+0.007, +0.305] |
| 1480 × 980, 13 pt | Pan drag at 200%, Overlay | `6660805` | 8 | 127 / 132 (114–132) | 18 (17–18) | 143.2 / 147.1 / 147.1 | 5.26 / 6.89 / 6.89 |  |  |
|  |  | `81db2e3` | 8 | 127 / 133 (116–133) | 17 (16–18) | 138.7 / 164.8 / 164.8 | 5.76 / 9.09 / 9.09 | -4.4 [-11.5, +10.9] | +0.079 [-0.418, +0.484] |
| 1480 × 980, 13 pt | Wheel notch at 200% (pans) | `6660805` | 40 | 1 / 1 (1–1) | 1 (1–1) | 5.7 / 8.4 / 10.1 | 5.16 / 7.74 / 9.39 |  |  |
|  |  | `81db2e3` | 40 | 1 / 1 (1–1) | 1 (1–1) | 6.0 / 9.3 / 10.0 | 5.51 / 8.60 / 9.40 | +0.3 [-0.2, +2.2] | — |
| 1480 × 980, 13 pt | Zoom choice | `6660805` | 32 | 2 / 15 (2–15) | 2 (2–15) | 12.6 / 91.1 / 92.0 | 5.51 / 7.39 / 7.84 |  |  |
|  |  | `81db2e3` | 32 | 2 / 15 (2–16) | 2 (2–16) | 14.2 / 93.1 / 101.4 | 5.74 / 8.36 / 8.42 | +1.6 [-0.8, +3.1] | +0.172 [-0.505, +1.257] |
| 1480 × 980, 13 pt | Mode change | `6660805` | 8 | 2 / 2 (2–2) | 2 (2–2) | 13.5 / 16.2 / 16.2 | 5.43 / 9.78 / 9.78 |  |  |
|  |  | `81db2e3` | 8 | 2 / 2 (2–2) | 2 (2–2) | 11.6 / 16.1 / 16.1 | 5.39 / 7.74 / 7.74 | -1.9 [-4.2, +2.1] | — |
| 1480 × 980, 13 pt | Pan drag at 200%, PNG added | `6660805` | 24 | 128 / 132 (111–136) | 19 (18–20) | 134.8 / 146.8 / 148.0 | 5.06 / 8.90 / 9.82 |  |  |
|  |  | `81db2e3` | 24 | 126 / 132 (113–132) | 18 (17–20) | 138.9 / 150.4 / 153.1 | 5.24 / 8.38 / 8.61 | +4.1 [-3.4, +8.1] | +0.097 [-0.078, +0.258] |
| 1480 × 980, 13 pt | GIF playback, 4 s | `6660805` | 12 | 317 / 325 (311–325) | 317 (311–325) | 1763.7 / 1819.4 / 1819.4 | 4.84 / 7.95 / 7.95 |  |  |
|  |  | `81db2e3` | 12 | 314 / 318 (304–318) | 314 (304–318) | 1773.7 / 1830.5 / 1830.5 | 5.15 / 10.68 / 10.68 | +10.0 [-10.1, +62.6] | +0.138 [+0.051, +0.239] |
| 1480 × 980, 13 pt | GIF pause | `6660805` | 12 | 4 / 5 (4–5) | 4 (4–5) | 25.0 / 31.9 / 31.9 | 7.09 / 12.37 / 12.37 |  |  |
|  |  | `81db2e3` | 12 | 4 / 4 (4–4) | 4 (4–4) | 25.9 / 32.6 / 32.6 | 12.04 / 13.64 / 13.64 | +0.9 [-4.8, +5.6] | +0.439 [-0.010, +1.291] |
| 1480 × 980, 18 pt | Pan drag at 200%, PNG changed | `6660805` | 24 | 130 / 134 (110–134) | 19 (18–20) | 121.4 / 132.3 / 136.3 | 4.69 / 7.90 / 9.06 |  |  |
|  |  | `81db2e3` | 24 | 128 / 133 (118–135) | 19 (18–20) | 127.0 / 135.6 / 137.5 | 4.78 / 6.76 / 7.14 | +5.7 [+0.9, +9.5] | +0.189 [+0.105, +0.280] |
| 1480 × 980, 18 pt | Pan drag at 200%, Overlay | `6660805` | 8 | 130 / 134 (123–134) | 19 (18–20) | 118.1 / 120.9 / 120.9 | 4.09 / 5.65 / 5.65 |  |  |
|  |  | `81db2e3` | 8 | 128 / 132 (125–132) | 19 (18–20) | 119.3 / 128.8 / 128.8 | 5.68 / 7.20 / 7.20 | +1.2 [-5.0, +8.8] | +0.144 [-0.063, +0.485] |
| 1480 × 980, 18 pt | Wheel notch at 200% (pans) | `6660805` | 40 | 1 / 1 (1–1) | 1 (1–1) | 5.0 / 8.2 / 9.2 | 4.44 / 7.65 / 8.54 |  |  |
|  |  | `81db2e3` | 40 | 1 / 1 (1–1) | 1 (1–1) | 5.0 / 8.5 / 12.6 | 4.46 / 7.78 / 11.87 | -0.0 [-1.0, +2.0] | — |
| 1480 × 980, 18 pt | Zoom choice | `6660805` | 32 | 2 / 16 (2–17) | 2 (2–17) | 10.9 / 83.6 / 84.0 | 4.52 / 7.09 / 8.33 |  |  |
|  |  | `81db2e3` | 32 | 2 / 16 (2–16) | 2 (2–16) | 10.6 / 80.1 / 82.8 | 4.46 / 7.17 / 7.70 | -0.3 [-2.2, +1.8] | -0.079 [-1.142, +0.367] |
| 1480 × 980, 18 pt | Mode change | `6660805` | 8 | 2 / 2 (2–2) | 2 (2–2) | 9.1 / 15.7 / 15.7 | 4.11 / 8.23 / 8.23 |  |  |
|  |  | `81db2e3` | 8 | 2 / 2 (2–2) | 2 (2–2) | 10.6 / 13.0 / 13.0 | 4.47 / 6.17 / 6.17 | +1.5 [-2.1, +3.8] | — |
| 1480 × 980, 18 pt | Pan drag at 200%, PNG added | `6660805` | 24 | 130 / 134 (118–135) | 19 (18–20) | 117.3 / 126.5 / 133.1 | 4.59 / 6.07 / 6.77 |  |  |
|  |  | `81db2e3` | 24 | 128 / 133 (120–134) | 19 (18–20) | 120.1 / 135.1 / 139.7 | 4.75 / 6.37 / 7.04 | +2.8 [-2.5, +9.0] | +0.142 [+0.066, +0.346] |
| 1480 × 980, 18 pt | GIF playback, 4 s | `6660805` | 12 | 354 / 365 (349–365) | 354 (349–365) | 1563.9 / 1604.0 / 1604.0 | 3.94 / 5.66 / 5.66 |  |  |
|  |  | `81db2e3` | 12 | 351 / 354 (344–354) | 351 (344–354) | 1602.0 / 1650.5 / 1650.5 | 4.13 / 5.64 / 5.64 | +38.2 [+6.9, +63.5] | +0.141 [+0.081, +0.212] |
| 1480 × 980, 18 pt | GIF pause | `6660805` | 12 | 5 / 5 (4–5) | 5 (4–5) | 21.2 / 23.3 / 23.3 | 9.06 / 13.02 / 13.02 |  |  |
|  |  | `81db2e3` | 12 | 4 / 5 (4–5) | 4 (4–5) | 22.0 / 27.8 / 27.8 | 9.95 / 15.02 / 15.02 | +0.8 [-0.8, +3.6] | +0.181 [-0.039, +0.423] |
| 461 × 490, 18 pt | Zoom choice | `6660805` | 32 | 5 / 5 (5–5) | 5 (5–5) | 15.1 / 17.7 / 18.3 | 2.61 / 3.56 / 4.90 |  |  |
|  |  | `81db2e3` | 32 | 5 / 5 (5–5) | 5 (5–5) | 16.8 / 20.4 / 21.6 | 2.87 / 4.18 / 4.43 | +1.7 [+0.4, +2.7] | +0.362 [-0.518, +1.069] |
| 461 × 490, 18 pt | Mode change | `6660805` | 8 | 5 / 5 (5–5) | 5 (5–5) | 14.7 / 16.5 / 16.5 | 2.67 / 3.87 / 3.87 |  |  |
|  |  | `81db2e3` | 8 | 5 / 5 (5–5) | 5 (5–5) | 15.6 / 17.6 / 17.6 | 2.79 / 3.67 / 3.67 | +0.9 [-0.8, +2.6] | +0.299 [-0.308, +0.594] |
| 461 × 490, 18 pt | GIF playback, 4 s | `6660805` | 12 | 2 / 2 (2–2) | 2 (2–2) | 5.5 / 9.3 / 9.3 | 2.39 / 5.12 / 5.12 |  |  |
|  |  | `81db2e3` | 12 | 2 / 2 (2–2) | 2 (2–2) | 6.7 / 8.0 / 8.0 | 2.61 / 3.59 / 3.59 | +1.1 [+0.3, +1.5] | — |
| 461 × 490, 18 pt | GIF pause | `6660805` | 12 | 2 / 2 (2–2) | 2 (2–2) | 5.6 / 8.4 / 8.4 | 2.46 / 4.11 / 4.11 |  |  |
|  |  | `81db2e3` | 12 | 2 / 2 (2–2) | 2 (2–2) | 6.8 / 8.0 / 8.0 | 2.70 / 3.90 / 3.90 | +1.2 [-0.7, +1.8] | — |

### Width steps

| Cell | Step | Build | n | Frames (count of steps) | UI CPU p50 / p95 / max (ms) | Configure to first frame request p50 / p95 / max (ms) | Last frame after the command p50 / max (ms) | Δ UI CPU p50 (ms) |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- |
| 1480 × 980, 13 pt | 1480 → 1000 | `6660805` | 32 | 1 (32) | 12.8 / 18.3 / 18.7 | 17.3 / 21.3 / 22.8 | 17.9 / 23.3 |  |
|  |  | `81db2e3` | 32 | 1 (32) | 14.0 / 19.9 / 21.2 | 17.7 / 24.6 / 26.4 | 18.1 / 27.1 | +1.1 [-0.4, +3.8] |
|  | 1000 → 1480 | `6660805` | 32 | 3 (32) | 27.9 / 36.7 / 38.6 | 20.2 / 27.4 / 29.3 | 58.0 / 71.6 |  |
|  |  | `81db2e3` | 32 | 3 (32) | 28.3 / 38.5 / 40.4 | 19.4 / 30.1 / 30.8 | 58.5 / 72.9 | +0.4 [-2.6, +3.8] |
| 1480 × 980, 18 pt | 1480 → 1000 | `6660805` | 32 | 1 (32) | 13.2 / 19.3 / 19.6 | 16.5 / 22.0 / 24.6 | 17.0 / 25.1 |  |
|  |  | `81db2e3` | 32 | 1 (32) | 12.5 / 20.0 / 20.7 | 16.8 / 22.2 / 24.1 | 17.5 / 25.4 | -0.7 [-1.9, +2.8] |
|  | 1000 → 1480 | `6660805` | 32 | 3 (32) | 25.3 / 36.7 / 39.4 | 20.2 / 26.7 / 27.6 | 55.1 / 66.7 |  |
|  |  | `81db2e3` | 32 | 3 (32) | 26.1 / 34.9 / 37.2 | 20.1 / 27.8 / 29.9 | 56.0 / 73.8 | +0.8 [-1.8, +3.5] |
| 461 × 490, 18 pt | 700 → 461 | `6660805` | 32 | 3 (32) | 11.5 / 14.7 / 16.1 | 22.2 / 38.4 / 39.1 | 42.8 / 59.2 |  |
|  |  | `81db2e3` | 32 | 2 (32) | 10.0 / 12.6 / 12.9 | 25.5 / 35.5 / 36.4 | 34.8 / 48.1 | -1.5 [-2.5, -0.8] |
|  | 461 → 700 | `6660805` | 32 | 2 (32) | 10.6 / 15.4 / 15.8 | 22.1 / 34.2 / 37.6 | 34.7 / 46.3 |  |
|  |  | `81db2e3` | 32 | 2 (32) | 12.7 / 18.5 / 19.5 | 21.6 / 33.8 / 35.6 | 34.4 / 47.8 | +2.1 [-0.6, +3.7] |

## Outliers kept

- Every frame and window is kept. The two longest draws (12.3 ms candidate, 11.6 ms base) are GIF-cell 1–2-tick gaps, counted in the "with 1–2-tick draws" view.
- At 1480 px, the first zoom choice of each launch (Fit → 100%) draws 13–17 frames in both builds, against 2–3 for later choices. The cause is untraced (plausibly the first 100% texture of the launch). It is included in the zoom rows and their p95 (15–16). At 461 × 490 a choice draws 5–6 frames, because it opens and closes a menu.
- The post-release presents without a draw described above (both builds).

## Commands

```sh
tools/make-fixtures.sh
# calibration (accessibility bus on only for these launches, then set back to false and read back):
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
tools/calibrate-all.sh                 # raw/cal/<build>-<fixture>-<cell>.json
python3 tools/regeometry.py            # drag centre and stroke, bounded to the stage
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b false
tools/run-all.sh 1 4                   # 72 launches, ABAB within each cell and fixture
python3 tools/analyse.py r1-... r4-... # raw/windows.csv, raw/frames.csv (then gzip)
python3 tools/summary.py > raw/summary.txt; python3 tools/tables.py > raw/tables.md
```

Two trial launches (`raw/trial/`, one per build, PNG changed at 1480 × 980, 13 pt) checked the pipeline and are not part of the results. Their windows showed the presents without a draw that led to the `present` class.

## Limits

- One host, one display at 120 Hz, Midnight only, three small fixtures, warm cache, 4 launches per build in each cell × fixture.
- Drag and wheel ran only at 1480 × 980: at 461 × 490 the base draws no image to drag. GIF playback animated only at 1480 × 980: at 461 × 490 neither build draws the GIF. The 461 × 490 per-frame figures rest on 16–56 draws per build and fixture.
- The width steps at 13 pt (1480 ↔ 1000) cross no toolbar threshold: the zoom group stays on one row down to 1000 px. At 18 pt the zoom group wraps below the modes between 1150 and 1000 px, and at 490 px tall both groups are menus at 461 and 560 px and buttons at 700 px. Floating windows cannot go below the app's 1000 × 680 minimum, so 461 × 490 is a tiled window.
- The cause is inferred from the diff and was not profiled. The record does not show how the 0.1–0.35 ms divides between the `InFrame` layout and the caption measurements.
- Frames are Mesa presents of the app's surface, not frames shown on the panel. The draw boundary excludes GPU work, compositing and presentation. The extra cost is on the UI thread; GPU work was not measured.
- `WAYLAND_DEBUG=1` adds libwayland's formatting to every message in both builds. It sits inside the UI CPU figures and adds only the `done` line's print to the draw boundary. No unlogged control run was made.
- The host was not idle: the IDE used about one core, the load average stayed near 3, and a preliminary analysis ran on the efficiency cores from 19:15 to past the end. Alternating ABAB within each cell exposes both builds to the same drift.
- The per-launch logs were not versioned. The CSV, summary and table files hold no local paths.
