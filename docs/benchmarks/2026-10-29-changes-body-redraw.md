# Changes redraws with the body laid out for its own height, Wayland, 2026-10-29

This is the release measurement for `12b70431` (`fix(app): lay the Changes list and composer out for the body's own height`) against its parent `ea58f875`. The commit turns the Changes inspector's list/composer body into a `WorkingBody` element that builds and lays out the list and composer while it is prepainted, for the height it has in that frame. Before, a canvas measured the body during paint and, on any change of its height or width, called `notify` and deferred a `window.refresh()`. Now only a change of the short form or of the width asks for another frame (`workspace.rs`, the `short_body(layout.height) != short_body(size.height) || layout.width != size.width` check in `WorkingBody`'s prepaint). The question has two parts. Does a full redraw with Changes visible cost more or less per frame? How many frames do a Stage or Unstage and a body height change draw, before and after?

The data is in [`2026-10-29-changes-body-redraw/`](2026-10-29-changes-body-redraw/): per-window rows in `raw/windows.csv`, per-frame rows in `raw/frames.csv.gz`, statistics in `raw/summary.json` and `raw/summary-draw-incl-ambiguous.txt`, the run log `raw/runs.txt` and the load in `raw/load-per-launch.txt`; the scripts are in `tools/`. The per-launch Wayland logs and samples (`<launch>.wl.log`, `<launch>.marks.json`), the calibration and the trial launches stayed on the measuring host: they hold local paths and are large.

## Result

Frames drawn and the UI thread's CPU per interaction (nearest-rank percentiles; n is the number of writes or height steps over 6 launches per build):

| Cell | Build | Writes n | Frames per write p50 / p95 (min–max) | Woken frames per write p50 (max) | UI CPU per write p50 / p95 / max (ms) | Height steps n | Frames per step (count of steps) | UI CPU per step p50 / p95 / max (ms) |
| --- | --- | ---: | --- | --- | --- | ---: | --- | --- |
| Six files, 13 pt | `ea58f87` | 96 | 32 / 34 (29–35) | 4 (5) | 100.1 / 111.6 / 117.7 | 60 | 3 (56), 2 (2), 4 (2) | 21.3 / 30.9 / 37.3 |
| | `12b7043` | 96 | 32 / 34 (27–34) | 4 (5) | 100.2 / 107.5 / 117.4 | 60 | 2 (60) | 16.6 / 24.3 / 32.9 |
| Six files, 18 pt | `ea58f87` | 96 | 32 / 34 (30–35) | 4 (5) | 89.0 / 97.6 / 107.3 | 60 | 2 (60) | 18.3 / 23.7 / 24.1 |
| | `12b7043` | 96 | 33 / 34 (24–35) | 4 (5) | 89.4 / 96.6 / 107.6 | 60 | 2 (60) | 16.8 / 21.8 / 23.3 |
| 600 files, 13 pt | `ea58f87` | 96 | 32 / 34 (26–35) | 4 (5) | 99.6 / 111.0 / 121.0 | 60 | 3 (60) | 19.5 / 28.0 / 28.6 |
| | `12b7043` | 96 | 32 / 34 (29–34) | 4 (5) | 97.8 / 107.2 / 111.1 | 60 | 2 (60) | 17.2 / 24.4 / 37.6 |
| 600 files, 18 pt | `ea58f87` | 96 | 32 / 34 (30–35) | 4 (5) | 89.9 / 97.2 / 103.7 | 60 | 2 (60) | 15.9 / 23.5 / 26.7 |
| | `12b7043` | 96 | 32 / 34 (29–35) | 4 (4) | 87.7 / 95.8 / 102.1 | 60 | 2 (60) | 16.7 / 23.1 / 24.8 |

Every height step received exactly one `xdg_toplevel.configure`. Each write drew one frame for the press, about four frames woken by the write's results (the write, its status read, its rows and the quiet refresh after it; the last of them about 1.56 s after the press), and the rest as `done`-paced frames of the row's flight animation.

Per-frame draw (UI thread, `done` dispatch to the frame request; boundary below), over every `done`-paced frame of the write windows:

| Cell | Build | Frames n | p50 / p95 / max (ms), paced (≤ 1 tick) | Launch p50s | p50 / p95 / p99 / max (ms), with 1–2-tick draws | Draws over one tick |
| --- | --- | ---: | --- | --- | --- | ---: |
| Six files, 13 pt | `ea58f87` | 2,561 | 2.720 / 5.223 / 8.328 | 2.60–2.80 | 2.724 / 5.305 / 8.034 / 15.482 | 12 |
| | `12b7043` | 2,581 | 2.691 / 5.138 / 8.299 | 2.65–2.76 | 2.695 / 5.232 / 7.739 / 9.508 | 11 |
| Six files, 18 pt | `ea58f87` | 2,606 | 2.369 / 4.777 / 8.174 | 2.32–2.50 | 2.371 / 4.857 / 7.650 / 16.245 | 8 |
| | `12b7043` | 2,642 | 2.312 / 4.816 / 8.302 | 2.24–2.43 | 2.314 / 4.865 / 7.829 / 11.074 | 7 |
| 600 files, 13 pt | `ea58f87` | 2,521 | 2.670 / 4.809 / 8.308 | 2.60–2.74 | 2.689 / 5.274 / 9.402 / 16.289 | 63 |
| | `12b7043` | 2,518 | 2.625 / 4.749 / 8.333 | 2.57–2.70 | 2.641 / 5.242 / 9.403 / 16.631 | 59 |
| 600 files, 18 pt | `ea58f87` | 2,585 | 2.339 / 4.435 / 8.251 | 2.26–2.41 | 2.350 / 4.929 / 9.087 / 16.319 | 48 |
| | `12b7043` | 2,581 | 2.296 / 4.343 / 8.320 | 2.23–2.37 | 2.305 / 4.885 / 8.883 / 11.165 | 51 |

Candidate minus base, with 95% two-level bootstrap intervals (launches, then samples; 4,000 resamples, seed 1):

| Cell | Draw Δp50 (ms) | Draw Δp95, with 1–2-tick draws (ms) | Draw Δp99, with 1–2-tick draws (ms) | UI CPU per write Δp50 (ms) | UI CPU per height step Δp50 (ms) |
| --- | --- | --- | --- | --- | --- |
| Six files, 13 pt | −0.029 [−0.100, +0.043] | −0.073 [−0.574, +0.287] | −0.295 [−0.778, +0.205] | +0.05 [−3.24, +2.43] | −4.64 [−5.74, −1.91] |
| Six files, 18 pt | −0.057 [−0.128, +0.016] | +0.008 [−0.368, +0.350] | +0.179 [−0.457, +0.568] | +0.35 [−2.02, +2.80] | −1.55 [−3.62, +0.15] |
| 600 files, 13 pt | −0.045 [−0.110, +0.020] | −0.032 [−0.508, +0.411] | +0.001 [−0.693, +0.754] | −1.73 [−3.48, +1.08] | −2.21 [−5.06, −0.87] |
| 600 files, 18 pt | −0.043 [−0.110, +0.018] | −0.044 [−0.373, +0.556] | −0.204 [−0.889, +0.418] | −2.16 [−4.22, +0.35] | +0.82 [−2.61, +2.76] |

## Verdict

- **Cost per frame: unchanged.** The whole-window redraw with Changes visible costs the same in both builds. Δp50 is −0.03 to −0.06 ms in every cell, with every interval including zero. That is smaller than the 0.13 to 0.20 ms spread of per-launch medians within one build, so it is noise, not a speedup. The tails also match: every Δp95 and Δp99 interval spans zero, and draws over one tick occur equally often in both builds (12/11, 8/7, 63/59, 48/51). Building the list and composer in `WorkingBody`'s prepaint adds no measurable cost on these fixtures.
- **Frames per Stage or Unstage: unchanged, 32 at p50 in both builds** (33 for the candidate at six files, 18 pt, from a distribution that overlaps the base's), with four woken frames per write in both. In these fixtures nothing above the body changes height during a write, so the base had no catch-up frame here for the change to remove. The 23 px line the commit describes was removed earlier by `541d974`, and this measurement does not recreate it.
- **Frames per body height change: one fewer at 13 pt.** At 13 pt the base drew 3 frames per 40 px height step (56 of 60 steps on six files, 60 of 60 on 600 files) and the candidate 2 (60 of 60 in both). The base's extra frame is `done`-paced and draws in 2.3 to 5.3 ms after the second frame's callback (118 steps), and its UI-thread CPU per step is higher by 4.6 ms [1.9, 5.7] on six files and 2.2 ms [0.9, 5.1] on 600 files. At 18 pt both builds drew 2 frames per step on every step, and the CPU intervals include zero. I didn't trace why the base asks for no third frame at 18 pt. One possibility, not checked, is that at 18 pt the body stays at its minimum height between 940 and 980 px, so the old canvas saw no change. The second frame that remains in both builds is consistent with the inspector-height canvas, which the diff keeps: it still notifies when the inspector's height changes. This too is inferred from the code, not traced.
- Read together: the change removes the catch-up frame where the body's height changes on its own, at no measurable per-frame cost. Ordinary Stage/Unstage writes draw the same number of frames as before.

## Setup

- **Host:** Apple M2 Pro (CPUs 0–3 efficiency at 2.42 GHz maximum, 4–11 performance at 3.26 GHz maximum), 31 GiB, Asahi Linux (Omarchy) on Linux 7.1.12-2-7-ARCH, `schedutil` governor, Hyprland 0.56.2, native Wayland. Display: built-in eDP-1, 3456 × 2160 at 120 Hz, scale 2, so one frame tick is 8.33 ms. Mesa 26.2.2 (`vulkan-asahi`), libwayland 1.26.0, Git 2.55.0.
- **Load:** the IDE hosting the session (`orca-ide`) used about one core throughout, and nothing compiled. One-minute load average 2.48–4.09 at launch start and 2.35–4.09 at launch end. Cores 4–11 were 68–86% idle over the 2 s before each launch (`raw/runs.txt`; per launch in `raw/load-per-launch.txt`). Pressure on the CPU (`/proc/pressure/cpu` some avg10) read 0.03 at the start. Window-start frequency of the UI thread's CPU: 3.264 GHz at p50, below 3.0 GHz on 254 of 672 base windows and 247 of 672 candidate windows.
- **Builds:** release executables built from clean trees (`--build-info`), each confirmed to embed its full revision (`strings <exe> | grep <sha>`):
  - Base `ea58f8758cc57236482d8d52c6ca16cc6d14dd75`, `source_tree` clean, aarch64-unknown-linux-gnu, release, rustc 1.99.0 (b940084d7 2026-09-28), build_unix_seconds 1793274349, sha256 `a5012ce8f4ec287dff10221e8e02762c8f21971424cbc17db863327a36231c2e`.
  - Candidate `12b70431e633c940a66539bdec754c18f7e07051`, `source_tree` clean, the same target, profile and rustc, build_unix_seconds 1793274423, sha256 `cd8810462e426942f621b822c9cdc82f186ed54db596e795ed231970a2a73b24`.
- **Fixtures** (`tools/make-fixtures.sh`, disposable, on disk under `fixtures/`, a fresh copy of the pristine repository per launch):
  - *Six files:* 40 commits of `README.md` (HEAD `821b953`), then six new files `alpha.txt`…`zeta.txt` with none staged. These are the six files of `motion::tests::six_files`, whose fixture also stages alpha and beta. Toggled file: `alpha.txt`.
  - *600 files:* one commit of 600 files `item-000.txt`…`item-599.txt` (HEAD `7dedc44`), all 600 edited, none staged. Toggled file: `item-000.txt`.
  - Nothing is staged so that the toggled file's Stage and Unstage controls are both reachable at 18 pt in 1480 × 980 without scrolling. At 18 pt with two files staged, the visible list ends at the Staged section.
- **Launch** (`tools/drive.py`): `taskset -c 4-11`, `NO_AT_BRIDGE=1`, `WAYLAND_DEBUG=1`. Empty absolute `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_STATE_HOME`/`XDG_CACHE_HOME` per launch, with fontconfig linked and preferences from the native-QA template: Midnight, follow-system off, interface text 13 or 18 pt, project pane off. The window floats at 1480 × 980, centred and forced opaque on workspace 9, and the app is in Changes (Ctrl+2).
- **Cache state:** warm. Each launch reads its fixture copy just after it was copied, and the executables were run repeatedly beforehand.
- **Order:** 6 rounds. Each round runs six files at 13 pt, six files at 18 pt, 600 files at 13 pt and 600 files at 18 pt, base then candidate in each cell, so each cell's launches alternate ABAB…ABAB (12 launches per cell, 48 in all, 16:02–17:23 UTC). No launch was dropped or repeated. After every launch the fixture's index had nothing staged, so every press hit its intended control.

## Method and boundaries

Per launch: one warm-up Stage/Unstage pair (not recorded), then 16 recorded writes alternating Stage and Unstage of the toggled file, then a 3 s idle window, 10 height steps (980 → 940 → 980 …, width fixed at 1480) and another 3 s idle window. For each write the pointer moves to the control, the driver waits 1.2 s (the hover animation and any tooltip settle), samples the UI thread, sends press and release through a virtual pointer, waits 3.0 s and samples again. For each height step it samples, runs `hyprctl dispatch` resize, waits 1.2 s and samples again, with the pointer parked on the status bar.

Neither build has a `GITTURTLE_TRACE` metric on this path (the only one it prints is `gitturtle.layout`, on viewport or settings changes). So every frame figure comes from the app's own `WAYLAND_DEBUG` log, identical for both builds, and every CPU figure from `/proc/<pid>/task/<pid>/schedstat`:

- **Frame:** one `wl_surface.attach` on the window's main surface (the one passed to `xdg_wm_base.get_xdg_surface`). This is Mesa's present of one GPUI draw. Neither build created popup or sub-surfaces in these launches.
- **Frames per write:** frames from libwayland's dispatch of the app's own `wl_pointer.button` press event to the driver's sample 3.0 s after the press began (windows 3.04 s). Each count includes the press frame, the frames woken by the write's results, and the row's flight animation. The animation is time-based, so how many frames it takes follows the 120 Hz tick and the draw time; counts are quantized by the tick and vary by a few frames between writes. In every write the last result frame landed 1.56 s after the press, well before the window ended. In 10 of 768 writes (6 base, 4 candidate) one more lone woken frame landed 1.98–3.04 s after the press, from a source I didn't identify. Both are counted. A frame later than the window would be missed.
- **Frames per height step:** frames between the driver's sample before the resize command and its sample 1.2 s later. Each window held exactly one `xdg_toplevel.configure`.
- **Per-frame draw** (`gap_ms` of `paced` frames in `frames.csv`): from libwayland's dispatch of a `wl_callback.done` for a callback requested on the main surface, to the app's next `wl_surface.frame` request on that surface. GPUI 0.3.4's Wayland window sends that request in its platform `draw`, after `Window::draw` returned and before the renderer encodes. The value therefore covers GPUI's frame handling (next-frame callbacks and the whole window's render, layout, prepaint and paint, with Changes visible), plus the debug print of the `done` line itself. It excludes renderer encoding, submission and Mesa's present (`present_ms`, request to attach, about 0.2 ms, recorded separately), GPU execution, compositor latching and presentation, input delivery, and the wait for the tick that delivers the `done`.

  The value starts at a tick, so the tick grid gives it no floor. Its classification has a ceiling, though. A `done` after which GPUI found nothing dirty parks the frame loop, and a task can wake it later. In the log, that looks just like a long draw. So a frame counts as `paced`, and its gap as a draw, only when the request follows the `done` within one tick (8.33 ms). Gaps of one to two ticks are `ambiguous`; they are reported in the second view and counted in "Draws over one tick". Longer gaps are `woken`, and no draw time is taken from them. The paced view's maximum is therefore bounded at 8.33 ms by construction, which is why the table also gives the view that includes the 1–2-tick draws. Neither view could show a draw longer than 16.67 ms. The observed gaps split into 2–5 ms draws, a handful at 5–11 ms, and parks of 16 ms and more, and the classification applies identically to both builds.
- **Press to first frame request** (`press_to_first_request_ms`): the press event's dispatch through the handler and the draw to the first frame request. p50 is 2.5–3.0 ms in every cell for both builds; p95 3.7–4.8 ms; max 6.2 ms.
- **UI CPU per write or step:** the main thread's schedstat run time between the driver's two samples. For a write that is from just before the press to 3.0 s after it, and it includes the write's handling, its results, every frame drawn and the libwayland debug formatting on that thread. The idle windows read 0.095–0.21 ms over 3 s and drew no frames, in every launch of both builds.

Statistics: pooled nearest-rank percentiles. Deltas use a two-level bootstrap: launches with replacement within each build, then samples within each drawn launch; 4,000 resamples, seed 1 (`tools/summary.py`).

## Outliers kept

- One base launch (`r3-ea58f87-six-18`) had two height steps whose first frame came 443 and 144 ms after the resize command. Their UI-thread CPU was ordinary (16.3 and 23.7 ms), so the delay is a wait, not work. They raise that cell's configure-to-first-request maximum to 437.5 ms and do not change its frame counts.
- The 10 lone late frames in write windows described above.

## Commands

```sh
tools/make-fixtures.sh
# calibration (accessibility bus on only for these launches, then set back to false):
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
tools/drive.py <exe> six|many 13|18 cal-<build>-<fixture>-<size> calibrate   # wrote tools/points.json's points
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b false
tools/run-all.sh 6        # 48 launches, then tools/analyse.py over all of them
python3 tools/summary.py  # tables and raw/summary.json
```

Calibration found the same points for both builds in every cell (13 pt: Stage (1455, 380), Unstage (1455, 336); 18 pt: (1446, 508) and (1446, 447), window-relative logical px; `raw/calibration/`). Two trial launches per build (`raw/trial/`) fixed the window lengths and are not part of the results.

## Limits

- One host, one display at 120 Hz, Midnight only, two fixtures. The writes here never change anything above the body's height. The cases the commit names, a feedback line coming or going above the body, were not recreated, so the frame that change removes during a write is not measured.
- Height steps of 40 px at 1480 px width only. The short form and width changes still ask for another frame by design and were not exercised.
- Frames are Mesa presents of the app's surface, not frames shown on the panel. The draw boundary excludes GPU work, compositing and presentation.
- `WAYLAND_DEBUG=1` adds libwayland's formatting to every message in both builds. It sits inside the UI CPU figures and adds only the `done` line's print to the draw boundary. No unlogged control run was made.
- The host was not idle: the IDE used about one core, and the load average stayed near 3. Alternating ABAB within each cell exposes both builds to the same drift.
- The per-launch logs were not versioned; the versioned CSV and summary files hold no local paths.
