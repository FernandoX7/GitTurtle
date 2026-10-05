# History scrolling after the exact-width change, X11, 2026-10-05

This is the release measurement for `history-columns-steady-after-toggle`: candidate `bc4726e` against its same-session base `d72b4c4`. It asks whether History's wheel scrolling got slower at 1480 × 800 or 461 × 490 on a disposable 3,000-commit repository like the one in the [September 30 record](2026-09-30-narrow-history-scroll.md), on this Linux host under XWayland.

Every raw per-notch sample is in [`2026-10-05-history-columns-steady-after-toggle.csv`](2026-10-05-history-columns-steady-after-toggle.csv). The host, the builds, the plan, per-launch metadata and every statistic below are in [`2026-10-05-history-columns-steady-after-toggle.json`](2026-10-05-history-columns-steady-after-toggle.json). The driver `history_scroll_x11.py` (sha256 `e45fd0302fbc633f618be716895b8c786d782590a2cbca38583792528f658583`) and the fixture generator `make_history_fixture.py` (sha256 `ff9b78988179682d7a76a1bba9c8793914e7640114799b3580e9de0a2fd95be2`) land beside this record in their own commit, under `2026-10-05-history-columns-steady-after-toggle/`, because the controller's evidence commits hold no code. The landed driver differs from the one that ran in one line only: it finds the repository from its own path instead of a local default (`GITTURTLE_REPO` still overrides it), sha256 `dc9a6aad795f4bfaaa28ac48eda11ed2446b1981c72cd520cc367cef3b60d287`.

## What changed

History's `on_prepaint` used to ignore a measured width within 1 px of the recorded one. Now it keeps the width exactly and notifies, which lays out the columns again, only when the width changes. The scope toolbar's compact threshold compares the exact width, not the width minus 1 px. The new `debug_selector` on each column header compiles to nothing in release builds: GPUI's no-op version applies without `test-support`, which `crates/app` enables only as a dev-dependency (resolver 3). Nothing else in render changed. Scrolling re-renders History on every notch. A width that differed between frames would therefore show up here as an extra notify and frame per notch, or as frames while idle.

## Setup

- **Builds**, designated in writing by the coordinator at 15:11 UTC on 2026-10-05, before any launch of this session:
  - Base `d72b4c489bbce93e3d0f42c6c592c37fc0f0b0f5`, the run's accepted head, a clean release build: `gitturtle-base-d72b4c4`, sha256 `caae2b3857c764d81fdabbb44d89683968cb56ffdbdad0d6c977fd01c3984fba`.
  - Candidate `bc4726eb434b76cd924b5ee70e24fafb4bf9ea0c`, a clean release build: `gitturtle-cand-bc4726e`, sha256 `3931e0b86f7fb40af1ce110b0274c868c469ec9841976ad99ff7f62fac857424`.
  - `qa.py identity` checked the pair at 15:13:41 UTC, before the first launch. Both sha256 values match the designation, and `--build-info` reports `source_tree clean`, `profile release`, `x86_64-unknown-linux-gnu`, rustc 1.99.0 and the revisions above.
- **Host:** Intel Core i9-10900K, 10 cores and 20 threads, 61 GiB, NVIDIA GeForce RTX 3090 with the open kernel module 595.91.07. The app's renderer adapter was not logged. CPUs 0–9 are the physical cores and 10–19 their SMT siblings.
- **Pinning:** every app launch ran as `taskset -c 0-7 BINARY FIXTURE`, and the driver checked affinity 0–7 and `/proc/PID/exe` for each one. The driver ran under `taskset -c 8,9`. Xwayland and gnome-shell were not pinned. A read-only design review may have run on CPUs 8–19 at the same time; it reads images and never launched the app.
- **Power and frequency:** `intel_pstate` active, `powersave` governor, energy preference `performance`, turbo on, mains power (a desktop). Just after each notch's frame reached the window, the driver read the UI thread's last CPU and its `scaling_cur_freq`. Median 5,294 to 5,300 MHz, 5th percentile 5,092 to 5,100 MHz, maximum 5,326 MHz, the same in all four cells, on CPUs 0–7 in every cell.
- **Software:** Ubuntu 26.04.1 LTS, Linux 7.0.0-34-generic, GNOME Shell 50.1 (Wayland session), Xwayland 24.1.10 on `:0`. The output is 3840 × 2160 at 137.95 Hz, scale 1.
- **Quiet host:** the controller loop was paused and nothing was building. The owner's own GitTurtle (PID 1448889) stayed open and was not touched. `qa.py display-check --allow-pid 1448889` printed "display clear" at 15:13:42 and 15:42:48 UTC. The one-minute load average was 0.12 at the start, 0.017 to 0.359 before each launch, and 0.07 at the end.
- **Fixture:** `/tmp/gitturtle-evidence/history-columns-steady-after-toggle/history`, built by `make_history_fixture.py DEST`. It has 3,000 commits, 142 of them merges: a root commit, then 142 blocks of a 4-commit side branch beside a 16-commit mainline run, each ending in a merge, and 17 linear commits on top. It has 121 branches (`main` and 120 `topic/NNN` at the last 120 side tips). It was written by `git fast-import` with fixed identities and dates and no system or global configuration. HEAD is `1e65ca0e77c98afe79699f13168fba8951872549`, and a second build gave the same HEAD (Git 2.53.0). History opens on "All history 1–500". Each launch checked HEAD, status and index before and after, and all 32 were unchanged.
- **Launch:** through `scripts/native_qa`'s `Session`, as `qa.py launch --input mutter` does, with only the argv changed to add the pinning. Each launch had a fresh HOME and XDG directories, the QA Git identity, and a generated store: Midnight, Follow system off, interface text 13 pt. `WAYLAND_DISPLAY` and `GIT_*` were removed, `DISPLAY=:0` and `GPUI_X11_SCALE_FACTOR=1`. The window was found by `_NET_WM_PID` and resized. For 461 × 490, its `WM_NORMAL_HINTS` minimum was first lowered from 1000 × 680 to 400 × 420 and read back. Then came a 5 s settle, activation and a pointer park. Mutter placed the window; it was not fullscreen. The app was stopped with SIGTERM to its PID, and all 32 exited on it.
- **Cache state:** every launch had new XDG directories, so Mesa's shader cache and the app's own caches started cold in every launch; the warm-up notches came first. The kernel page cache was warm: the fixture and binaries had been read by the pilots minutes earlier.
- **Windows:** at 1480 × 800 both builds draw the labelled scope toolbar, and their first frames were pixel-identical in the pilots. At 461 × 490, scale 1, both draw the compact toolbar, and their first frames differed only in the status bar's timing digit. The History list is then a column of about 175 px beside the inspector.
- **Input:** through `org.gnome.Mutter.RemoteDesktop`, never XTest. Each notch is one `NotifyPointerAxisDiscrete` step (+1 down, −1 up). The pointer was glided onto a History row and confirmed there by Xwayland before each phase: at (444, 480) in the wide window and (138, 382) in the narrow one. The lock state was checked before each phase. Per launch:
  1. 20 warm-up notches (10 down, 10 up), not recorded;
  2. 20 idle windows;
  3. 120 recorded notches (30 down, 30 up, twice);
  4. 20 idle windows.
  There was one notch or idle window every 150 ms, and no keys were sent. In the narrow window a vertical notch also scrolls the table sideways, in both builds, as on September 30.
- **Order:** the plan was fixed in writing at 15:21:11 UTC, before the first recorded launch. It ran wide BCCB, narrow CBBC, wide CBBC, narrow BCCB, and the four again: 32 launches, 8 per build per size, 960 recorded notches per cell. The run went from 15:21:18 to 15:41:42 UTC.
- **Pilots, not recorded:** 5 launches from 15:16:12 to 15:21:02 UTC checked the pointer point, the scrolling and the timing source. The first stopped on a driver bug after the window opened. None is in the results.

## Metrics and their boundaries

All stamps are the driver's `CLOCK_MONOTONIC`, taken when a call returned or an X event was read from a second X connection.

- **Send to frame** (`damage_ms`): from the driver's stamp just before the D-Bus `NotifyPointerAxisDiscrete` call to the receipt of the first X `DamageNotify` (raw rectangles) on the app's top-level window after it. It contains four parts:
  - input routing, from Mutter through its Wayland pointer to Xwayland and the app;
  - the wait for GPUI's next frame tick;
  - the UI thread's frame: wheel dispatch, the root render with `render_history` and the scope toolbar, layout, prepaint with History's `on_prepaint`, paint and the renderer's submission;
  - GPU execution and Xwayland's present into the window.

  It excludes Mutter's composite and scanout. Every notch was handled while idle, but GPUI's X11 backend only draws from a free-running refresh timer at the RandR mode's period (`gpui-pre-linux` 0.3.4, `x11/client.rs` `start_refresh_loop`). That is 7.25 ms at 137.95 Hz, and input only marks the window dirty. Xwayland can also hold a present until Mutter's next frame callback, up to another 7.25 ms. So the value is at least input routing plus the frame's own CPU and GPU time, and at most that plus about two ticks. A notch every 150 ms is not a whole number of ticks, so the tick phase varies uniformly from notch to notch. The tick wait therefore adds the same noise to both builds, with a spread of about 7 ms. With 960 samples per cell it limits this metric's p50 resolution to about ±0.4 ms.
- **Raw input to frame** (`app_ms`): the same end, from the receipt of the notch's first XI2 raw event (`RawButtonPress` or `RawMotion`) on the root window instead. That is Xwayland dispatching the notch to X clients, the app included. It excludes Mutter's part of input routing, which was 0.83 ms at p50 in all four cells. The tick-grid bounds above apply unchanged.
- **UI-thread CPU per notch** (`cpu_ms`): the app's main thread's CPU time from `/proc/PID/task/PID/schedstat`, between the reads just before this notch's send and just before the next one, 150 ms later. It contains everything the UI thread did for the notch: event handling, render, layout, prepaint, paint and the renderer's submission. It also contains the 20 or so idle refresh-timer callbacks in those 150 ms, 0.27 ms in an idle window of either build. It excludes other threads, GPU execution and presentation. The tick grid does not enter it, so it is the metric that resolves code cost. Run delay on the same thread (`wait_ms`) was 0.002 to 0.003 ms at p50 and at most 1.02 ms, so scheduling did not shape the CPU figures.
- **Frames per notch:** the damage bursts on the window in each 150 ms window; events more than 1 ms apart count as separate frames. Idle windows count frames drawn without input.

Percentiles are by nearest rank. Δ is the candidate's pooled statistic minus the base's. Its 95% interval comes from a two-level bootstrap: 4,000 resamples, drawing launches with replacement within each build and then notches with replacement within each drawn launch, so launch-to-launch spread is in the interval.

## Results

Milliseconds, 8 launches and 960 recorded notches per cell, all pinned to CPUs 0–7.

| Size, metric | base p50 / p95 / p99 / max | candidate p50 / p95 / p99 / max | Δp50 [95%] | Δp95 [95%] | Per-launch p50, base; candidate |
| --- | --- | --- | --- | --- | --- |
| 1480 × 800, UI CPU | 5.277 / 5.730 / 6.180 / 6.726 | 5.295 / 5.730 / 6.164 / 6.877 | +0.018 [−0.042, +0.081] | 0.000 [−0.202, +0.158] | 5.177–5.400; 5.230–5.359 |
| 1480 × 800, send to frame | 11.830 / 15.285 / 16.139 / 34.628 | 11.975 / 15.221 / 15.842 / 39.330 | +0.145 [−0.264, +0.548] | −0.064 [−0.365, +0.131] | 11.475–12.031; 11.559–12.151 |
| 1480 × 800, raw input to frame | 10.914 / 14.343 / 15.014 / 16.894 | 11.068 / 14.321 / 14.841 / 15.731 | +0.154 [−0.302, +0.561] | −0.022 [−0.268, +0.134] | 10.578–11.208; 10.695–11.247 |
| 461 × 490, UI CPU | 3.357 / 5.990 / 6.584 / 7.296 | 3.356 / 5.967 / 6.586 / 7.585 | −0.001 [−0.100, +0.097] | −0.023 [−0.306, +0.381] | 3.230–3.520; 3.230–3.578 |
| 461 × 490, UI CPU, one-frame notches (896/896) | 3.340 / 3.746 / 4.048 / 4.696 | 3.340 / 3.750 / 4.086 / 4.589 | 0.000 [−0.092, +0.089] | +0.004 [−0.136, +0.122] | 3.207–3.478; 3.200–3.569 |
| 461 × 490, send to frame | 9.079 / 12.622 / 13.303 / 19.219 | 9.205 / 12.657 / 13.269 / 43.586 | +0.126 [−0.294, +0.454] | +0.035 [−0.274, +0.313] | 8.795–9.391; 8.877–9.324 |
| 461 × 490, raw input to frame | 8.261 / 11.673 / 12.156 / 13.078 | 8.359 / 11.754 / 12.341 / 13.024 | +0.098 [−0.277, +0.463] | +0.081 [−0.179, +0.288] | 7.962–8.394; 8.028–8.498 |

The minima show where the tick grid puts the floor. Send to frame was at least 7.02 (base) and 7.45 ms (candidate) wide, and 4.78 and 4.54 ms narrow, against UI CPU minima of 4.61 and 4.54 ms wide and 2.94 and 2.88 ms narrow.

### Frames per notch and idle frames

| Size | Recorded notches without a frame | Notches with two frames | Idle windows with a frame (of 320) | Idle UI CPU per 150 ms, p50 |
| --- | --- | --- | --- | --- |
| 1480 × 800, base | 0 | 0 | 0 | 0.265 |
| 1480 × 800, candidate | 0 | 0 | 0 | 0.278 |
| 461 × 490, base | 0 | 64 | 16 | 0.275 |
| 461 × 490, candidate | 0 | 64 | 16 | 0.272 |

Wide, every notch drew exactly one frame in both builds, and nothing was drawn while idle. Narrow, both builds drew a second frame on the same eight notches of every launch (indexes 13, 24, 43, 54, 73, 84, 103 and 114, at the same scroll positions in each cycle). Both also drew one frame in idle windows 7 and 11 after the warm-up of every launch. These frames predate the change, since the base draws them identically. They cost about 3 ms of UI CPU each and make the narrow UI CPU p95 about 6 ms in both builds, so the one-frame row above shows the per-frame cost without them. They were not investigated further. No notch drew a frame in one build and not the other, and no idle window did either. So the candidate's exact width comparison caused no extra notify, layout pass or frame while scrolling or idle at either size. The wide idle CPU is 0.013 ms higher in the candidate. That is within the spread of the per-launch medians (base 0.253–0.293, candidate 0.265–0.347), and no frame was drawn in those windows.

### Noise

- **UI CPU:** the per-launch medians spread by 0.13 to 0.35 ms within each build, more than any p50 difference. Every interval spans zero and lies within about ±0.1 ms at p50, so a per-notch cost above about 0.1 ms would have shown. The means agree as well: 5.309 against 5.324 ms wide, 3.564 against 3.549 ms narrow.
- **Send to frame:** its p50 is 0.13 to 0.15 ms later in the candidate at both sizes. That is a quarter to a third of the ±0.4 ms interval, and the per-launch medians spread 0.45 to 0.6 ms within each build. The difference of launch means (+0.107 wide, +0.052 narrow) is under 1.5 standard errors with launch-mean standard deviations of 0.12 to 0.16 ms. The UI CPU of the same notches does not differ, and the frequency was the same in all four cells. So the tick grid's noise exceeds this difference, and it is not taken as a regression.
- **Maxima:** every send-to-frame sample over 20 ms (4 base and 4 candidate wide, 2 candidate narrow) was input routing. In each, Xwayland's raw event came 8 to 33 ms after the send, and in most of them the D-Bus call itself returned 9 to 31 ms late, while that notch's UI CPU was normal (3.5 to 5.7 ms). The same kind occurs in both builds. Raw input to frame, which excludes it, has maxima of 13.0 to 16.9 ms in all four cells.

## Verdict

No regression beyond noise at either size. The UI thread's CPU per scroll notch is equal within about 0.1 ms at p50 and within the interval at p95: wide Δp50 +0.018 ms [−0.042, +0.081], narrow −0.001 ms [−0.100, +0.097]. Frames per notch and idle frames are identical in both builds, so the exact-width `on_prepaint` adds no layout, notify or frame. End-to-end send-to-frame latency is bounded by GPUI's 137.95 Hz refresh timer and Xwayland's presentation, and its differences (+0.13 to +0.15 ms at p50, −0.06 to +0.04 ms at p95) are well inside its ±0.3 to 0.5 ms intervals.

## Commands

From the repository root, with the QA virtual environment, on the X11 display `:0` under GNOME:

```sh
python3 make_history_fixture.py /tmp/gitturtle-evidence/history-columns-steady-after-toggle/history
taskset -c 8,9 .local/qa-venv/bin/python3 history_scroll_x11.py run BASE CAND FIXTURE OUT.json PLAN
.local/qa-venv/bin/python3 history_scroll_x11.py analyze OUT.json
.local/qa-venv/bin/python3 history_scroll_x11.py export OUT.json RECORD.csv RECORD.json
```

`PLAN` is the 32-launch order above, also kept in the results file. `analyze` ran with `BOOT_N=4000`. The exported JSON has the binaries' and run directories' basenames in place of their local paths.

## Not measured

- The navigation toggle itself (Ctrl+B twice) and window resizes, the paths where the width changes and History lays out again. This record covers steady scrolling only.
- Wayland, Hyprland, fractional scale factors (the task's 1.25 case is in its native evidence), macOS, and another host. The narrow window here is at scale 1, where September 30's was at scale 2 on Hyprland, so the figures are not comparable with that record, only the base-against-candidate result.
- The commit-file-list and selection frames, Compare and tooltips.
- GPU execution time, Mutter's composite and scanout, a warm shader cache and a cold page cache.
- Pages other than the first, "All history 1–500".
- The cause of the narrow two-frame notches and the post-warm-up idle frames, which both builds draw alike.
