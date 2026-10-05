# Opening a large patch in Compare and in the stash inspector, X11, 2026-10-05

This is the release measurement for `decorations-in-retained-bounds`: candidate `c9e1222` against its same-session base `7323988`. It asks whether opening a large text patch got slower, in Compare and in the stash inspector, on a disposable fixture with two 26,672-line patches, on this Linux host under XWayland.

The raw per-open samples are in [`2026-10-05-decorations-in-retained-bounds-compare.csv`](2026-10-05-decorations-in-retained-bounds-compare.csv), [`2026-10-05-decorations-in-retained-bounds-compare-supplementary.csv`](2026-10-05-decorations-in-retained-bounds-compare-supplementary.csv) and [`2026-10-05-decorations-in-retained-bounds-stash.csv`](2026-10-05-decorations-in-retained-bounds-stash.csv). The supplementary block's per-CPU busy samples are in [`2026-10-05-decorations-in-retained-bounds-cpu-supplementary.txt`](2026-10-05-decorations-in-retained-bounds-cpu-supplementary.txt). The designation, both plans, host, builds, per-launch metadata and every statistic below are in [`2026-10-05-decorations-in-retained-bounds.json`](2026-10-05-decorations-in-retained-bounds.json).

The driver `compare_open_x11.py` (sha256 `19364090153d9358cd56ec52acf73c00e6247084e20b48063b2c75025f7d1cb6`) and the fixture generator `make_compare_fixture.py` (sha256 `6b44d5fb22b8e5e032efffd74695697f006b496863a305867a4c8369ee4a372e`) land beside this record in their own commit, under `2026-10-05-decorations-in-retained-bounds/`, because the controller's evidence commits hold no code. Both are the exact files that ran. The driver finds the repository from its own path when it sits there; `GITTURTLE_REPO` overrides that, and it was set for this run because the driver ran from outside the clone.

## What changed

- **Compare:** the tab's retained-bytes figure (`tab_retained_bytes`, `repository_tabs.rs`) now adds `PatchDecorations::retained_bytes` for Compare's unified patch. That figure is read only when switching tabs, against the 512 MiB bound, and the added term is O(1): it borrows the layer and multiplies two lengths. Compare's open path itself (`select_file`, `load_file`, `clear_preview`, `receive`, `ensure_editor`, `text::editor_with_decorations`) has no source change between the two builds. The candidate's parent is the base, and `git diff 7323988 c9e1222` is exactly the task's commit.
- **Stash and rewrite review:** the stash inspector (`recovery::StashBrowser`) and the rewritten-series review (`rewrite_review::SeriesBrowser`) now build their patch editor with `text::editor_with_decorations` and keep the decorations handle until the preview is cleared. Before, they called `text::editor`, which made the same call and dropped the handle at once. The editor is built the same way in both builds. The candidate stores an `Rc` that the base dropped, and frees the decoration list when the preview is cleared rather than right after it is built. Their new `preview_retained_bytes` is dead code outside tests.
- The new test helper `text::decorated_patch_content` is `#[cfg(test)]`.

So both open paths should be cost-neutral by construction. This record measures them.

## Setup

- **Builds**, designated in writing by the coordinator at 16:33 UTC on 2026-10-05, before any recorded launch:
  - Base `7323988ef9508c8ba68688babf9927827e70f543`, the run's accepted head, a clean release build: `gitturtle-base-7323988`, sha256 `4d14284730d495eb22e6325f4fbcdc3be36e555e194abb5822abe7255b7bba00`.
  - Candidate `c9e12222e2cf458a547de03be2277783038a69fb`, a clean release build: `gitturtle-cand-c9e1222`, sha256 `a79234e1d457b656ae9d238ad46e07718857e1fa52c05e8f85559ba6e983d7cb`.
  - `qa.py identity` checked the pair at 16:35:42 UTC, before the first launch. Both sha256 values match the designation. `--build-info` reports `source_tree clean`, `profile release`, `x86_64-unknown-linux-gnu`, rustc 1.99.0 (b940084d7 2026-09-28) and the revisions above. Neither was rebuilt.
- **Host:** Intel Core i9-10900K, 10 cores and 20 threads, 61 GiB, NVIDIA GeForce RTX 3090 with the open kernel module 595.91.07. CPUs 0–9 are the physical cores and 10–19 their SMT siblings (CPU *n* pairs with *n* + 10).
- **Pinning:** every app launch ran as `taskset -c 0-7 BINARY FIXTURE`, and the driver checked affinity 0–7 and `/proc/PID/exe` for each one. The driver ran under `taskset -c 8,9`. Xwayland, gnome-shell and the app's GPU work were not pinned.
- **Power and frequency:** `intel_pstate` active, `powersave` governor, energy preference `performance`, turbo on, mains power (a desktop). Just after each Compare open's patch frame reached the window, the driver read the UI thread's last CPU and its `scaling_cur_freq`. The median was 5,282 to 5,296 MHz and the 5th percentile 5,000 to 5,100 MHz in every Compare cell, on CPUs 0–7. The stash mode did not record frequency (see Not measured).
- **Software:** Ubuntu 26.04.1 LTS, Linux 7.0.0-34-generic, GNOME Shell 50.1 (Wayland session), Xwayland 24.1.10 on `:0`. The output is 3840 × 2160 at 137.95 Hz, scale 1.
- **Quiet host:** the unattended loop was idle in an evidence wait and nothing was building. A tooling agent was allowed to run Python unit tests pinned to CPUs 12–19, which are the SMT siblings of the app's CPUs 2–7; this run did not observe whether it did. The coordinator's account, added afterwards: that agent did run the native-QA unit suite on CPUs 12–19 at intervals until about 16:53 UTC, and a read-only review of its change, not pinned, ran from 16:47 to 16:51 UTC, so both overlapped the main Compare block (16:44:14 to 16:58:31 UTC) and could account for its tail clustering; none of the coordinator's work ran during the supplementary block. The owner's own GitTurtle (PID 1448889) stayed open and was not touched. `qa.py display-check --display :0 --allow-pid 1448889` printed "display clear" at 16:35:46, 17:12:32 and 17:20:56 UTC. The one-minute load average was 0.08 at 16:35, 0.004 to 0.388 before each launch, and 0.40 at 17:20:56, just after the last launch.
- **Fixture:** `make_compare_fixture.py DEST` (default 20,000 lines), at `/tmp/gitturtle-evidence/decorations-in-retained-bounds/compare`. It runs with no system or global Git configuration, an empty template, fixed identities and fixed dates. Two builds gave the same IDs (Git 2.53.0).
  - The root commit adds `a-large.txt` and `b-large.txt`: 20,000 code-like ASCII lines each, 1,389,056 and 1,391,387 bytes, under Compare's 2 MiB per side.
  - HEAD `b6d97c5afc39efd5d63864394dbdbf64e9687d30` rewrites every third line of both files. Each file's patch has 26,672 lines in one hunk and 1,832,097 bytes (6,667 removed, 6,667 added).
  - `refs/stash` `6b3181b97246c689edaa4e05db1586df379d20a7` rewrites every fifth line of the root's text in both files, so each stashed patch against HEAD removes and adds 9,333 lines.
  - The worktree is clean. History lists the stash first. Each launch checked HEAD, status and index before and after, and all 36 were unchanged.
- **Launch:** through `scripts/native_qa`'s `Session`, as `qa.py launch --input mutter` does, with two changes: the argv adds the pinning, and `GITTURTLE_TRACE=1` is set, with the app's stderr read on a pipe and each line stamped on receipt. Each launch had:
  - a fresh HOME and XDG directories, the QA Git identity, and a generated store (Midnight, Follow system off, interface text 13 pt);
  - `WAYLAND_DISPLAY` and `GIT_*` removed, `DISPLAY=:0` and `GPUI_X11_SCALE_FACTOR=1`;
  - the window found by `_NET_WM_PID`, resized to 1480 × 800 (not fullscreen), a 5 s settle, activation and a pointer park.

  The app was stopped with SIGTERM to its PID, and all 36 launches exited on it.
- **Route, Compare (not recorded):** click HEAD's row in History, then `a-large.txt` in its changed files. This enters Compare with the file list focused. Wait for that patch's trace line, then park the pointer.
- **Route, stash (not recorded):** click Changes, its Actions menu, "Browse stashes…", the stash, then `a-large.txt`. This focuses the stash's file list. Wait until two screen grabs 1 s apart are identical, then park the pointer.
- **Input:** keys through `org.gnome.Mutter.RemoteDesktop` (`NotifyKeyboardKeysym`), never XTest, and no pointer input after the park. Down opens `b-large.txt` and Up opens `a-large.txt`, alternately. Each key selects the other file. In Compare that runs `select_file`, `load_file`, `clear_preview` (which drops the previous patch editor, its decorations and its content), a main-worker read and then `ensure_editor` on the UI thread. In the stash inspector the selection runs its own `clear_preview`, preview-worker read and `ensure_editor`. Per launch, one key every 700 ms:
  1. 1 cold open, `b-large.txt`'s first: a worker cache miss;
  2. 6 warm-up opens, not recorded;
  3. 40 recorded opens;
  4. 5 idle windows.

  700 ms is 96.6 refresh ticks, so the tick phase varies from open to open.
- **Cache state:**
  - Every recorded open is a hit in its worker's in-memory `PreviewCache` (128 MiB, 32 entries per worker), so it measures clearing the previous preview, applying cached prepared content, building the editor and its decorations, and drawing. The cold open, a miss that reads and prepares the patch on the worker, is reported separately.
  - Every launch had new XDG directories, so Mesa's shader cache and the app's stores started cold. The setup frames came first.
  - The kernel page cache was warm: the pilots had read the fixture and binaries minutes earlier.
- **Order:** both plans were fixed in writing before their first recorded launch:
  - Main plan, fixed at 16:44:09 UTC: Compare `B,C,C,B,C,B,B,C,B,C,C,B,C,B,B,C` (16 launches, 8 per build, 320 recorded opens per build), 16:44:14 to 16:58:31 UTC. Then stash `C,B,B,C,B,C,C,B,C,B,B,C` (12 launches, 6 per build, 240 recorded opens per build), 16:58:32 to 17:11:02 UTC.
  - Supplementary plan, fixed at 17:13:36 UTC, after the main analysis and before its first launch: Compare `C,B,B,C,C,B,B,C` (8 launches, 4 per build, 160 recorded opens per build), 17:13:41 to 17:20:50 UTC. A `/proc/stat` per-CPU sampler on CPU 9 ran alongside, every 0.5 s. Its reason is under Noise.
- **Pilots, not recorded:** 9 candidate launches from 16:36 to 16:44 UTC.
  - 6 short exploratory launches through plain `Session.launch`, unpinned, found the click points.
  - 3 driver pilots, pinned: one on a 6,000-line fixture, where a warm Compare open was about 17 ms to its frame and was scaled up for this record, and one per mode on the final fixture.

  None is in the results.

## Metrics and their boundaries

All stamps are the driver's `CLOCK_MONOTONIC`, taken when a call returned or when an X event or stderr line was read. The X events come from a second X connection: `DamageNotify` (raw rectangles) on the app's top-level window, and XI2 `RawKeyPress` on the root.

- **Key to patch frame** (`patch_ms`, Compare): from the driver's stamp just before the key-down D-Bus call to the receipt of the first `DamageNotify` read after the driver received the app's `gitturtle.file_preview_frame_ms` line for that open. It contains:
  - input routing;
  - the key handler and the previous preview's drop;
  - the cache hit's hop back to the UI thread;
  - `ensure_editor` (a 1.83 MB editor and its decorations);
  - any wait for GPUI's refresh tick;
  - that frame's layout, prepaint, paint and submission;
  - GPU execution and Xwayland's present into the window.

  It excludes Mutter's composite and scanout. GPUI's X11 backend draws only from a free-running timer at the RandR mode's period (`gpui-pre-linux` 0.3.4, unpatched, `x11/client.rs` `start_refresh_loop`), which is 7.25 ms at 137.95 Hz; input only marks the window dirty. When UI work overruns a tick, the overdue timer fires as soon as the UI thread returns to its event loop. So the value is at least input routing plus the UI work and the frame's CPU and GPU time, and at most that plus one tick of timer wait plus up to one more tick while Xwayland holds the present for Mutter's next frame callback. Here the UI work alone (about 20.5 ms to the patch frame) exceeds two ticks, so the grid adds jitter rather than setting the floor. In all 960 recorded Compare opens, the gap from the trace line's receipt to this damage was positive: 4.64 to 18.90 ms, 5.28 ms at p50. Where a loading frame came first, the driver's alternative rule, the first damage after send plus the trace value, picked that loading frame instead, in 2 to 7 opens per cell; `patch_ms` uses the receipt rule.
- **Raw key to patch frame** (`app_patch_ms`): the same end, from the receipt of the key's XI2 `RawKeyPress` on the root, that is Xwayland dispatching it to X clients. It excludes Mutter's part of input routing, `raw_ms`, which was 1.06 to 1.09 ms at p50 in every cell. The grid bounds above apply unchanged.
- **App trace** (`trace_ms`): the app's own `gitturtle.file_preview_frame_ms`, from `select_file` entry to the window's next-frame callback after the prepared preview was applied ([metrics](metrics.md)). That callback runs at the start of the tick, before the frame's layout and paint. So the value is at least the UI work before the callback and at most that plus one 7.25 ms tick. It excludes input delivery, the frame that draws the patch, and presentation.
- **UI CPU to patch frame** (`cpu_to_patch_ms`, Compare): the app's main thread's CPU time from `/proc/PID/task/PID/schedstat`, from the read just before the send to the read just after the patch frame's `DamageNotify`. It covers the open's whole UI-thread work, including the previous preview's drop and the frame that draws the patch, plus about three idle timer callbacks. It excludes other threads and GPU time. The tick grid does not enter it, except that an open which drew a loading frame first also pays for that frame. This is the metric that resolves code cost.
- **UI CPU per open** (`cpu_ms`): the same thread over the whole 700 ms window, from the read just before this send to the read just before the next. It adds the idle refresh callbacks of the rest of the window, which cost 1.20 to 1.32 ms per 700 ms idle window at p50 in every cell. For the stash inspector, which prints no trace, this is the CPU metric.
- **Process CPU per open** (`proc_cpu_ms`): the sum over all of the app's threads, over the same window. It includes the worker's cache lookup and the renderer's threads.
- **Key to last frame** (`last_ms_settle`, the stash's latency): from the send to the receipt of the last `DamageNotify` in the 700 ms window. The idle windows of both builds drew no frame, and nothing else changes on screen, so the last frame is the one that completes the open. In 97% of stash opens it was the only frame. Its tick-grid bounds are those of `patch_ms`. `app_last_ms` is the same end from the raw key.
- **Frames per open:** damage bursts in the window. Events more than 1 ms apart count as separate frames.

Percentiles are by nearest rank. Δ is the candidate's pooled statistic minus the base's. Its 95% interval comes from a two-level bootstrap: 4,000 resamples, drawing launches with replacement within each build and then opens with replacement within each drawn launch. Launch-to-launch spread is therefore in the interval.

## Results

Milliseconds, recorded warm opens only, every launch pinned to CPUs 0–7.

### Compare, main plan (8 launches and 320 opens per build)

| Metric | base p50 / p95 / max | candidate p50 / p95 / max | Δp50 [95%] | Δp95 [95%] | Per-launch p50, base; candidate |
| --- | --- | --- | --- | --- | --- |
| Key to patch frame | 25.541 / 26.618 / 44.540 | 25.551 / 27.213 / 119.494 | +0.010 [−0.172, +0.190] | +0.595 [−0.422, +11.914] | 25.343–25.697; 25.320–25.747 |
| Raw key to patch frame | 24.447 / 25.424 / 27.040 | 24.433 / 25.747 / 37.751 | −0.014 [−0.151, +0.176] | +0.323 [−0.333, +1.529] | 24.246–24.686; 24.292–24.679 |
| App trace | 17.019 / 17.718 / 20.573 | 17.009 / 18.460 / 29.851 | −0.010 [−0.090, +0.122] | +0.742 [−0.117, +1.476] | 16.805–17.206; 16.903–17.198 |
| UI CPU to patch frame | 20.486 / 21.489 / 23.851 | 20.490 / 22.395 / 35.205 | +0.004 [−0.115, +0.158] | +0.906 [+0.014, +2.051] | 20.344–20.677; 20.356–20.719 |
| UI CPU per open | 21.741 / 22.804 / 26.840 | 21.752 / 23.857 / 36.667 | +0.011 [−0.139, +0.189] | +1.053 [−0.248, +3.326] | 21.536–21.996; 21.613–22.087 |
| Process CPU per open | 22.743 / 23.823 / 28.348 | 22.769 / 24.749 / 37.588 | +0.026 [−0.174, +0.242] | +0.926 [−0.352, +3.079] | 22.511–23.033; 22.579–23.011 |

Frames: base 313 opens with one frame, 1 with two and 6 with three; candidate 312 with one and 8 with three. In the three-frame opens a refresh tick fell between the key and the preview, so a loading frame came first. No idle window drew a frame. Every recorded open had exactly one trace line and a patch frame.

### Compare, supplementary plan (4 launches and 160 opens per build)

| Metric | base p50 / p95 / max | candidate p50 / p95 / max | Δp50 [95%] | Δp95 [95%] | Per-launch p50, base; candidate |
| --- | --- | --- | --- | --- | --- |
| Key to patch frame | 25.558 / 26.566 / 27.397 | 25.442 / 26.643 / 51.763 | −0.116 [−0.460, +0.157] | +0.077 [−0.595, +1.095] | 25.251–25.875; 25.093–25.679 |
| Raw key to patch frame | 24.518 / 25.458 / 26.199 | 24.356 / 25.386 / 36.953 | −0.162 [−0.448, +0.162] | −0.072 [−0.615, +0.421] | 24.271–24.723; 24.013–24.703 |
| App trace | 16.903 / 17.442 / 18.208 | 16.889 / 17.376 / 28.892 | −0.014 [−0.136, +0.134] | −0.066 [−0.423, +0.299] | 16.822–16.923; 16.763–16.956 |
| UI CPU to patch frame | 20.391 / 21.160 / 22.206 | 20.295 / 20.919 / 33.671 | −0.096 [−0.227, +0.007] | −0.241 [−0.593, +0.831] | 20.340–20.471; 20.175–20.352 |
| UI CPU per open | 21.625 / 22.713 / 26.130 | 21.527 / 22.355 / 35.171 | −0.098 [−0.200, +0.000] | −0.358 [−2.862, +1.454] | 21.578–21.689; 21.428–21.563 |
| Process CPU per open | 22.607 / 23.588 / 27.298 | 22.603 / 23.512 / 36.026 | −0.004 [−0.173, +0.160] | −0.076 [−2.464, +1.683] | 22.491–22.747; 22.521–22.651 |

Frames: base 154 opens with one frame and 6 with three; candidate 158 with one and 2 with three. No idle window drew a frame.

### Stash inspector (6 launches and 240 opens per build)

| Metric | base p50 / p95 / max | candidate p50 / p95 / max | Δp50 [95%] | Δp95 [95%] | Per-launch p50, base; candidate |
| --- | --- | --- | --- | --- | --- |
| Key to last frame | 26.926 / 29.595 / 55.469 | 26.945 / 28.054 / 41.524 | +0.019 [−0.213, +0.312] | −1.541 [−10.310, +9.323] | 26.705–27.127; 26.580–27.342 |
| Raw key to last frame | 25.850 / 27.618 / 38.390 | 25.952 / 26.936 / 39.361 | +0.102 [−0.229, +0.325] | −0.682 [−10.215, +9.284] | 25.721–26.019; 25.471–26.228 |
| UI CPU per open | 22.627 / 24.106 / 28.364 | 22.644 / 23.377 / 28.060 | +0.017 [−0.133, +0.193] | −0.729 [−3.800, +3.125] | 22.429–22.844; 22.461–22.779 |
| Process CPU per open | 23.660 / 25.345 / 30.088 | 23.681 / 24.633 / 29.569 | +0.021 [−0.144, +0.128] | −0.712 [−3.592, +3.094] | 23.550–24.007; 23.551–23.784 |

Frames: base 232 opens with one frame and 8 with three; candidate 232 with one, 1 with two and 7 with three. No idle window drew a frame. The stash's p95 intervals are wide because its tail is those three-frame opens. Each drew a loading frame 9 to 10 ms after the key and the patch at 34 to 41 ms, with about 5 ms more UI CPU, and they fall unevenly across launches in both builds (base launch 2 had 3 of its 40).

### Cold opens (one per launch, a worker cache miss)

| Path | base p50 / max | candidate p50 / max |
| --- | --- | --- |
| Compare, key to patch frame (8 per build) | 304.166 / 307.698 | 302.374 / 307.496 |
| Compare, UI CPU per open | 27.952 / 29.866 | 27.507 / 28.392 |
| Stash, key to last frame (6 per build) | 313.373 / 316.248 | 314.602 / 318.344 |
| Stash, UI CPU per open | 30.065 / 30.814 | 29.329 / 29.696 |

The cold open's extra time is the worker's read and preparation of the 1.8 MB patch, about 275 ms of process CPU off the UI thread in both builds. The UI thread pays 6 to 7 ms more than for a warm open. With 6 to 8 samples per build this is a coarse check, and it shows no difference.

### Noise

- **Medians:** in every block and metric the p50 difference is within ±0.17 ms, and its interval contains zero. The intervals reach about ±0.25 ms for CPU and ±0.2 to 0.46 ms for latency, so a per-open cost of more than about 0.25 ms, roughly 1% of the open, would have shown. The per-launch medians spread 0.1 to 0.8 ms within each build, more than any p50 difference.
- **The one interval that excludes zero:** in the main Compare block, the UI CPU to the patch frame at p95 is +0.906 ms [+0.014, +2.051]. That is one of 16 intervals in that block, and it clears zero by 0.014 ms. The shift is not spread across launches as a code cost on this deterministic path would be. It sits in candidate launches 4, 7 and 10 (per-launch p95 22.47, 22.38 and 33.25 ms), while candidate launches 9, 12 and 15 are among the lowest of all 16 (20.65 to 21.11 ms; launch 15 is the lowest). A launch-level permutation test on the per-launch p95 values of all 16 launches gives p = 0.12, and 0.31 for the latency's p95. The supplementary block, planned before its first launch to check this, reverses it: Δp95 −0.241 ms [−0.593, +0.831]. In the stash block the candidate's p95 values are the lower ones. The code path is unchanged. So this is noise from launch to launch, not a regression.
- **Isolated slow opens:** four candidate Compare opens took 33.3 to 35.2 ms of UI CPU to the patch frame, against a p50 of 20.5. Three were in main launch 10 (opens 17, 18 and 33, all on CPU 4) and one in supplementary launch 7 (open 12, on CPU 5). In each:
  - `trace_ms` was 28.4 to 29.9 ms against about 17, so the extra time came before the frame callback;
  - frequency was 5,000 to 5,299 MHz;
  - run-queue wait was at most 0.02 ms;
  - the slice count was normal, and one frame was drawn.

  So the UI thread was running, at full clock, about 1.6 times slower for one open. That is consistent with contention on its physical core or for shared cache. No base open went past 23.9 ms. Over 480 opens per build that is 4 against 0, and Fisher's exact test gives p = 0.12 (two-sided). The sampler showed little activity on CPUs 10–19 during the supplementary block: 6 of its 857 half-second samples had any of them above 20% busy. At that resolution it cannot rule out a burst of a few tens of milliseconds on the SMT sibling (CPU 14 or 15). The cause is not identified. It does not come from a changed function, because the Compare open path is identical in source.
- **Latency maxima:** every Compare open over 40 ms to its patch frame (2 base, 6 candidate) was input routing. In each, Xwayland's raw key came 17.9 to 93.7 ms after the send, and the D-Bus call itself returned 16 to 93 ms late, while that open's UI CPU was normal (20.1 to 24.9 ms). The same holds for the stash's 55.5 ms base maximum (raw key at 30.4 ms). The raw-key metrics exclude this.

## Verdict

No regression beyond noise on either path. Opening a 26,672-line, 1.8 MB patch from the preview cache is equal in both builds at p50:

- in Compare, 25.54 against 25.55 ms to the patch frame (Δ +0.010 ms [−0.172, +0.190]) and 20.49 ms of UI CPU in both (Δ +0.004 ms [−0.115, +0.158]);
- in the stash inspector, 26.93 against 26.95 ms to its last frame (Δ +0.019 ms [−0.213, +0.312]) and 22.63 against 22.64 ms of UI CPU (Δ +0.017 ms [−0.133, +0.193]).

At p95, every difference lies inside the launch-to-launch noise. The main Compare block's +0.9 ms UI-CPU p95 clusters in three candidate launches, is not significant at the launch level and reverses in the supplementary block. The frame counts, cold opens and idle frames agree. Grounds for the change being neutral by construction: Compare's open path has no source change, `tab_retained_bytes` runs only on tab switches and gained an O(1) term, and the stash inspector keeps an `Rc` it used to drop. No fix is needed. One thing is left open: the four isolated 33 to 35 ms candidate Compare opens against none in the base (p = 0.12). A follow-up that wants to close it should sample the app cores' SMT siblings per open, not every 0.5 s.

## Commands

From the repository root, with the QA virtual environment, on the X11 display `:0` under GNOME, with `BENCH=docs/benchmarks/2026-10-05-decorations-in-retained-bounds`:

```sh
python3 $BENCH/make_compare_fixture.py /tmp/gitturtle-evidence/decorations-in-retained-bounds/compare
taskset -c 8,9 .local/qa-venv/bin/python3 $BENCH/compare_open_x11.py run compare BASE CAND FIXTURE compare.json B,C,C,B,C,B,B,C,B,C,C,B,C,B,B,C
taskset -c 8,9 .local/qa-venv/bin/python3 $BENCH/compare_open_x11.py run stash BASE CAND FIXTURE stash.json C,B,B,C,B,C,C,B,C,B,B,C
taskset -c 8,9 .local/qa-venv/bin/python3 $BENCH/compare_open_x11.py run compare BASE CAND FIXTURE compare-supp.json C,B,B,C,C,B,B,C
.local/qa-venv/bin/python3 $BENCH/compare_open_x11.py analyze OUT.json
.local/qa-venv/bin/python3 $BENCH/compare_open_x11.py export OUT.json RECORD.csv RUN.json
```

`analyze` ran with its default of 4,000 bootstrap resamples, pinned off the app's CPUs after the launches had ended. The combined JSON joins the three exported run files with the designation and plans. They carry the binaries', fixture's and run directories' basenames in place of their local paths. The supplementary block's CPU sampler was a 20-line `/proc/stat` reader, not landed; its output is the `.log` file.

## Not measured

- **The rewritten-series review (`rewrite_review::SeriesBrowser`).** Its change is the same as the stash inspector's: it keeps a handle it used to drop. It needs a remote-tracking rewrite fixture to reach, and it was left out.
- **Tab switching, where `tab_retained_bytes` runs.** Its added term is O(1) by inspection; no switch was timed.
- **Retained memory.** The change alters accounting, not allocation; this record times opens only.
- **Frequency and core in the stash mode.** The driver reads them at the patch frame, which needs a trace line the stash inspector does not print.
- **Other cases:** Split, Before and After views; patches larger than these or over the 2 MiB per-side limit; a cold page cache; a warm shader cache.
- **Other platforms:** GPU execution time, Mutter's composite and scanout, Wayland, Hyprland, fractional scales, macOS and another host.
