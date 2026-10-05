# Editor highlights: theme application cost, X11, 2026-10-05

This is the release measurement for task `editor-highlights-keep-syntax-readable` (run `20261002T173706Z-3663d7c4`): candidate `3761488` against its same-session base `e5afb22`. The change adds `EditorHighlights::fit` (`crates/app/src/appearance.rs`), which `Palette::apply` runs once per application beside the syntax roles. It fits the editors' selection, Find's two match backgrounds and the two changed-word tints. The question is what that adds to a theme switch, measured against the theme-apply budget in the [themes specification](../development/themes/spec.md#performance): median ≤ 8 ms and p95 ≤ 16 ms for `gitturtle.theme_apply_frame_ms`.

Three layers were measured:

- **Built-in switches in the running app.** These are Settings picker clicks across all twenty built-in palettes, with the spec's budget fixture (History at 1–1000, a 2,000-line Split comparison retained behind Settings). They give `gitturtle.theme_apply_frame_ms` and UI-thread CPU per switch.
- **Omarchy switches in the running app.** These alternate Tokyo Night and Catppuccin Latte through `omarchy-theme-set`'s writes, with the same Split comparison on screen. They give `gitturtle.omarchy_apply_frame_ms` and UI-thread CPU per switch.
- **The apply work alone,** in an optimized harness: `Palette::apply`'s work on the kit theme, for the twenty built-ins and the two Omarchy themes. A supplementary run adds two adversarial custom drafts. This layer gives the per-apply cost that bounds the theme editor's drag path.

The raw per-switch samples are in [`…-builtin.csv`](2026-10-05-editor-highlights-keep-syntax-readable-builtin.csv) and [`…-omarchy.csv`](2026-10-05-editor-highlights-keep-syntax-readable-omarchy.csv). The harness's per-process statistics and every first-call sample are in [`…-harness.csv`](2026-10-05-editor-highlights-keep-syntax-readable-harness.csv). The designation, plan, host, builds, per-launch metadata, bootstrap intervals and per-palette cells are in [the JSON](2026-10-05-editor-highlights-keep-syntax-readable.json). The written plan and each launch's driver summary line are in [`…-driver-log.txt`](2026-10-05-editor-highlights-keep-syntax-readable-driver-log.txt).

## Result

**Within budget, with an added cost that is real and small.** On the candidate, `theme_apply_frame_ms` measured median **3.860 ms** (≤ 8: yes) and p95 **7.079 ms** (≤ 16: yes), max 7.897 ms, over 320 built-in switches. The base measured 3.699 / 7.055 / 7.684 ms. On Omarchy switches, `omarchy_apply_frame_ms` measured 3.788 / 7.109 / 7.853 ms on the candidate and 3.643 / 7.150 / 7.858 ms on the base. Both bounds are decided by the frame-tick boundary, not by this code (see [Boundaries](#what-the-values-measure)).

The fitting costs **0.05 to 0.58 ms of UI-thread time per application**, depending on the palette:

- Harness, steady-state p50: Deep Sea +0.046 ms; Sandstone +0.577 ms. The light palettes cost the most, and the Omarchy Catppuccin Latte mapping +0.354 ms.
- In the app, the per-palette deltas of UI CPU to the new theme's frame track the harness's with slope 1.01 (r = 0.75 over the twenty built-ins). So the in-app increase is the fitting, not an artefact of the run.

| Path | Metric | base p50 / p95 / max | candidate p50 / p95 / max | Δp50 [95%] | Δp95 [95%] | Per-launch p50, base; candidate |
| --- | --- | --- | --- | --- | --- | --- |
| Built-in, 320 per build | `theme_apply_frame_ms` | 3.699 / 7.055 / 7.684 | 3.860 / 7.079 / 7.897 | +0.161 [−0.341, +0.601] | +0.024 [−0.388, +0.397] | 3.579–3.752; 3.413–4.047 |
| Built-in | Release to new-theme frame | 16.386 / 19.867 / 20.510 | 16.283 / 19.480 / 20.987 | −0.103 [−0.829, +0.452] | −0.387 [−0.793, +0.177] | 16.253–16.651; 15.876–16.485 |
| Built-in | UI CPU to new-theme frame | 10.281 / 10.736 / 11.200 | 10.417 / 10.902 / 12.265 | +0.136 [−0.017, +0.242] | +0.166 [+0.045, +0.307] | 10.209–10.356; 10.269–10.556 |
| Built-in | UI CPU, release to +150 ms | 10.552 / 11.035 / 11.443 | 10.683 / 11.196 / 12.616 | +0.131 [−0.010, +0.265] | +0.161 [+0.054, +0.291] | 10.478–10.635; 10.549–10.878 |
| Built-in | UI CPU, whole 1.2 s window | 95.145 / 138.958 / 147.409 | 96.192 / 139.679 / 144.075 | +1.047 [−0.440, +2.887] | +0.721 [−2.023, +2.130] | 93.923–95.926; 94.709–97.133 |
| Omarchy, 160 per build | `omarchy_apply_frame_ms` | 3.643 / 7.150 / 7.858 | 3.788 / 7.109 / 7.853 | +0.145 [−0.963, +0.870] | −0.041 [−0.613, +0.480] | 3.408–4.038; 3.257–3.945 |
| Omarchy | First write to new-theme frame | 263.809 / 267.585 / 270.120 | 263.561 / 267.280 / 268.376 | −0.248 [−1.152, +0.516] | −0.305 [−1.017, +0.452] | 263.647–263.886; 262.957–263.832 |
| Omarchy | UI CPU to new-theme frame | 7.173 / 7.688 / 8.538 | 7.461 / 7.793 / 8.527 | **+0.288 [+0.202, +0.356]** | +0.105 [−0.291, +0.575] | 7.138–7.194; 7.372–7.485 |
| Omarchy | UI CPU, whole 1.5 s window | 9.399 / 10.181 / 10.866 | 9.752 / 10.380 / 11.017 | +0.353 [+0.192, +0.490] | +0.199 [−0.283, +0.714] | 9.388–9.431; 9.541–9.894 |
| Omarchy, control | `omarchy_reread_ms` (background executor) | 0.195 / 0.481 / 0.557 | 0.085 / 0.120 / 0.148 | −0.110 [−0.359, −0.067] | −0.361 [−0.397, −0.347] | 0.155–0.195; 0.071–0.085 |
| Idle, 5 per launch | UI CPU per 1.2 s / 1.5 s window | 2.118 / 2.345 / 2.387; 2.676 / 2.843 / 2.844 | 2.241 / 3.381 / 3.748; 2.708 / 2.986 / 3.582 | — | — | — |

All values are in ms, pooled from recorded switches only, with every launch pinned to CPUs 0–7. The Omarchy reread is faster in the candidate. The likely cause in the diff is `custom::luminance`, which now reads a 256-entry linear-light table instead of calling `powf` per channel, and Omarchy's mapping measures luminance throughout. That reread runs off the UI thread.

**Per palette, UI CPU to the new-theme frame** (16 recorded switches per palette and build; with 16 samples, the nearest-rank p95 is the maximum), beside the harness's added apply cost:

| Palette | base p50 / max | candidate p50 / max | Δp50, app | Δp50, harness |
| --- | --- | --- | ---: | ---: |
| Midnight | 10.250 / 10.684 | 10.288 / 10.920 | +0.038 | +0.060 |
| Daylight | 10.132 / 10.860 | 10.672 / 11.228 | +0.540 | +0.312 |
| Graphite | 10.379 / 10.754 | 10.328 / 10.544 | −0.051 | +0.065 |
| Tokyo Night | 10.292 / 11.085 | 10.306 / 10.902 | +0.014 | +0.088 |
| Catppuccin Mocha | 10.317 / 11.195 | 10.102 / 10.778 | −0.215 | +0.074 |
| Nord | 10.204 / 10.587 | 10.409 / 10.800 | +0.205 | +0.080 |
| Porcelain | 10.142 / 10.628 | 10.322 / 10.896 | +0.180 | +0.127 |
| Sandstone | 10.443 / 10.812 | 10.684 / 11.219 | +0.241 | +0.577 |
| Deep Sea | 10.157 / 10.714 | 10.135 / 10.614 | −0.022 | +0.046 |
| Ember | 10.380 / 10.793 | 10.174 / 10.805 | −0.206 | +0.053 |
| Solarized Dark | 10.177 / 10.653 | 10.288 / 10.778 | +0.111 | +0.131 |
| Solarized Light | 10.229 / 10.455 | 10.396 / 11.023 | +0.167 | +0.242 |
| One Dark | 10.344 / 10.668 | 10.361 / 10.747 | +0.017 | +0.097 |
| One Light | 10.217 / 10.600 | 10.829 / 11.164 | +0.612 | +0.542 |
| Rosé Pine | 10.356 / 10.941 | 10.238 / 10.889 | −0.118 | +0.096 |
| Rosé Pine Dawn | 10.258 / 10.844 | 10.637 / 10.984 | +0.379 | +0.367 |
| Dracula | 10.243 / 10.768 | 10.427 / 10.776 | +0.184 | +0.119 |
| Alucard | 10.417 / 10.766 | 10.596 / 12.265 | +0.179 | +0.333 |
| Kanagawa Wave | 10.280 / 10.917 | 10.364 / 10.637 | +0.084 | +0.109 |
| Kanagawa Lotus | 10.225 / 11.200 | 10.372 / 10.665 | +0.147 | +0.164 |
| Omarchy Tokyo Night (80 per build) | 7.196 / 8.058 | 7.284 / 8.189 | +0.088 | +0.085 |
| Omarchy Catppuccin Latte (80 per build) | 7.169 / 8.538 | 7.580 / 8.527 | +0.411 | +0.354 |

**Harness, `Palette::apply`'s work on the kit theme**, in µs. Steady state pools 10,000 calls per palette and build from 10 processes. The first call is one call in each of 5 fresh processes.

| Palette | base p50 / p95 / max | candidate p50 / p95 / max | Δp50 | fit alone, p50 / p95 | process medians, base; candidate | first call p50, base / candidate |
| --- | --- | --- | ---: | --- | --- | --- |
| Midnight | 13.87 / 14.56 / 60.84 | 73.90 / 76.80 / 149.45 | +60.0 | 59.75 / 61.40 | 13.76–14.23; 72.98–76.74 | 46.15 / 116.01 |
| Daylight | 14.72 / 15.17 / 95.70 | 326.77 / 337.35 / 470.43 | +312.1 | 311.08 / 329.02 | 14.55–15.14; 322.93–327.72 | 44.68 / 368.61 |
| Graphite | 13.82 / 14.27 / 50.95 | 78.87 / 80.62 / 160.66 | +65.1 | 64.69 / 66.27 | 13.71–14.20; 77.54–79.38 | 43.47 / 113.46 |
| Tokyo Night | 13.88 / 14.26 / 60.08 | 102.05 / 105.34 / 240.13 | +88.2 | 87.38 / 91.25 | 13.72–14.23; 100.57–102.63 | 44.28 / 136.55 |
| Catppuccin Mocha | 13.84 / 14.27 / 33.49 | 88.23 / 92.50 / 167.00 | +74.4 | 73.94 / 78.28 | 13.74–14.25; 86.97–92.31 | 47.26 / 127.09 |
| Nord | 13.89 / 14.38 / 53.59 | 93.96 / 96.77 / 181.42 | +80.1 | 79.58 / 81.38 | 13.75–14.34; 92.89–94.82 | 44.34 / 129.36 |
| Porcelain | 14.20 / 14.70 / 36.27 | 141.38 / 144.83 / 222.55 | +127.2 | 127.42 / 129.81 | 14.05–14.68; 140.03–143.34 | 44.40 / 180.44 |
| Sandstone | 14.51 / 14.99 / 43.03 | 591.82 / 627.75 / 1002.01 | +577.3 | 577.09 / 610.39 | 14.34–14.96; 589.08–596.35 | 44.07 / 646.85 |
| Deep Sea | 13.89 / 14.41 / 51.66 | 60.24 / 61.84 / 119.64 | +46.4 | 46.62 / 47.99 | 13.78–14.36; 59.74–61.31 | 45.49 / 94.43 |
| Ember | 14.02 / 14.53 / 51.78 | 66.67 / 69.17 / 168.78 | +52.7 | 52.70 / 55.15 | 13.87–14.50; 66.26–67.95 | 44.50 / 102.92 |
| Solarized Dark | 16.32 / 16.90 / 57.70 | 147.15 / 153.68 / 284.21 | +130.8 | 132.68 / 135.00 | 16.19–16.60; 146.16–153.49 | 46.86 / 184.54 |
| Solarized Light | 16.05 / 16.37 / 43.47 | 258.21 / 268.73 / 481.00 | +242.2 | 242.55 / 253.77 | 15.92–16.35; 256.47–260.17 | 46.81 / 303.94 |
| One Dark | 15.28 / 15.54 / 45.01 | 112.33 / 114.51 / 213.59 | +97.1 | 97.59 / 99.91 | 15.16–15.50; 111.70–113.89 | 45.52 / 150.55 |
| One Light | 15.89 / 16.51 / 68.51 | 558.04 / 582.70 / 970.67 | +542.2 | 545.06 / 576.67 | 15.74–16.23; 555.88–562.61 | 48.12 / 611.33 |
| Rosé Pine | 13.93 / 14.19 / 114.50 | 110.41 / 112.52 / 212.79 | +96.5 | 95.89 / 97.56 | 13.79–14.15; 109.48–111.63 | 43.65 / 144.96 |
| Rosé Pine Dawn | 15.74 / 16.16 / 65.80 | 382.42 / 398.65 / 698.71 | +366.7 | 367.67 / 384.72 | 15.59–16.13; 380.35–385.26 | 46.53 / 433.66 |
| Dracula | 14.95 / 15.54 / 55.04 | 133.89 / 140.57 / 290.86 | +118.9 | 118.82 / 125.94 | 14.85–15.52; 132.94–140.13 | 45.05 / 170.84 |
| Alucard | 15.47 / 16.08 / 61.05 | 348.22 / 364.23 / 637.85 | +332.8 | 333.72 / 349.03 | 15.32–16.06; 346.31–350.92 | 47.22 / 392.47 |
| Kanagawa Wave | 14.46 / 15.05 / 53.40 | 123.82 / 130.04 / 235.63 | +109.4 | 108.89 / 111.83 | 14.34–15.02; 123.14–129.86 | 43.89 / 162.65 |
| Kanagawa Lotus | 16.87 / 17.60 / 61.93 | 180.47 / 192.61 / 358.36 | +163.6 | 165.27 / 168.61 | 16.70–17.58; 179.46–192.02 | 46.04 / 217.29 |
| Omarchy Tokyo Night (mapped) | 14.34 / 14.92 / 49.69 | 99.58 / 101.73 / 197.43 | +85.2 | 85.25 / 87.43 | 14.21–14.89; 99.04–101.13 | 45.05 / 136.06 |
| Omarchy Catppuccin Latte (mapped) | 16.25 / 16.64 / 51.75 | 370.64 / 392.08 / 631.21 | +354.4 | 356.12 / 362.62 | 16.12–16.61; 368.15–384.15 | 44.89 / 411.51 |

The supplementary harness run used 4 processes and 4,000 calls per palette and build. Its two adversarial drafts are Midnight with `text` `#3A4250` and Porcelain with `text` `#D8DBE4`. In both, `text` misses the text rule on its own editor background, so syntax falls back to `text` and no highlight candidate reads. They added +478.0 µs (base 13.26, candidate 491.25 p50, 518.78 p95, 702.12 max) and +492.9 µs (13.59; 506.44 / 527.31 / 656.69). In the same run, Sandstone measured 592.01 µs and Midnight 73.11 µs, within 1 µs of the main run. So a failing draft costs less than Sandstone. The heaviest of the 24 palettes measured is a curated light built-in.

## Verdict

- **Budget: PASS.** On the candidate, `theme_apply_frame_ms` measured median 3.860 ms (≤ 8) and p95 7.079 ms (≤ 16), max 7.897 ms, over 320 switches covering every built-in palette 16 times. `omarchy_apply_frame_ms` measured 3.788 / 7.109 / 7.853 ms over 160 Omarchy switches. No switch was missing its frame, every switch printed exactly one apply line (and, on Omarchy, exactly one reread), the store named the expected palette after every built-in switch, and no other trace line printed.
  - These bounds are decided by the frame-tick boundary, and the base sits at the same place (3.699 / 7.055 ms). The differences of +0.161 and +0.145 ms at p50 lie inside intervals of about ±0.5 to ±0.9 ms that come from tick phase.
- **Code cost: +0.05 to +0.58 ms of UI-thread CPU per application, real and within the budget's slack.** UI CPU to the new-theme frame rose by 0.136 ms at p50 over all built-ins (interval [−0.017, +0.242]; the harness's mean over the twenty is 0.184 ms) and by 0.288 ms on Omarchy switches ([+0.202, +0.356]; the launch medians do not overlap). Per palette it follows the harness: One Light +0.612 ms in the app against +0.542 in the harness, Rosé Pine Dawn +0.379 against +0.367, Catppuccin Latte +0.411 against +0.354. The dark built-ins add less than about 0.13 ms, and their in-app deltas (−0.2 to +0.2 ms) are inside the per-palette noise of 16 samples.
- **Theme editor drag path** ([DESIGN.md](../../DESIGN.md), live preview: a picker drag applies up to once per frame). This path was not measured natively. The harness bounds what each application adds: at most 0.58 ms at p50 and 0.63 ms at p95 in steady state (Sandstone), 0.65 ms on a first call in a fresh process, and about 0.49 ms for drafts that fail the text rule. Against this host's 7.249 ms frame period, that is under 9% of a frame for the heaviest light palette and about 1% for most dark ones. The search is bounded by construction: at most 257 lightness steps and 20 chroma rows of 65 steps per highlight, with candidates screened before rounding. But no draft was built to maximise it.
- **No fix is needed for the budget.** If the drag path ever needs the time back, the smallest change is to fit once per drag frame only when a token that the fitting reads has changed, or to cache `EditorHighlights` by palette. Both are untested suggestions.

## What the values measure

All stamps are the driver's `CLOCK_MONOTONIC`, taken when a call returned or when an X event or stderr line was read. GPUI's X11 backend draws only from a free-running timer at the RandR mode's period: 137.95 Hz on this output, a 7.249 ms tick (`gpui-pre-linux` 0.3.4, `x11/client.rs` `start_refresh_loop`). Next-frame callbacks run at that tick, before the frame's layout and paint.

- **`theme_apply_frame_ms`**, the budget metric. It runs from `choose_theme` entry (`settings.rs`) through `apply_appearance` (where `Palette::apply` now fits the editor highlights), the store save submission and `trace_next_frame` to the window's next-frame callback.
  - It excludes input delivery, the layout and paint of the frame that shows the new theme, presentation and GPU time.
  - By construction it lies between the handler's own cost and that cost plus one 7.249 ms tick. With the tick phase uniform, the median sits near the handler plus 3.6 ms and the p95 near the handler plus 6.9 ms.
  - The 8 ms median bound therefore cannot fail unless the handler alone approaches 4.4 ms, and the 16 ms p95 bound not unless it approaches 9 ms. A 16 ms criterion on this metric is decided by the boundary, not by the code.
  - Every recorded sample here was under 7.9 ms, under one tick plus 0.65 ms.
- **`omarchy_apply_frame_ms`.** It runs from the arrival of the reread's result on the UI thread, through `apply_appearance`, to the next-frame callback. It excludes the 250 ms quiet period, the reread itself (`omarchy_reread_ms`, on the background executor, which serves as the control) and the frame's draw. The same one-tick bounds apply.
- **Release to new-theme frame** (built-in) and **first write to new-theme frame** (Omarchy). These run from the stamp just before the button-release D-Bus call, or just before the first of `omarchy-theme-set`'s writes, to the first `DamageNotify` on the app's top-level window read after the driver received the apply trace line.
  - They contain input routing, the handler, any wait for the tick, the frame's layout, paint and submission, GPU execution and Xwayland's present into the window. They exclude Mutter's composite and scanout.
  - At least one tick boundary and up to one more (Xwayland's frame callback) can enter. The UI work here (about 10 ms) exceeds a tick, so the grid adds jitter rather than setting the floor.
  - The Omarchy value also contains the 250 ms quiet period, the inotify delivery and the reread.
- **UI CPU to new-theme frame** (`cpu_to_frame_ms`) resolves code cost. It is the app main thread's run time from `/proc/PID/task/PID/schedstat`, from the read just before the release or first write to the read just after that frame's `DamageNotify`.
  - It covers the handler, `Palette::apply`, the restyle of the retained Split editors, the frame that draws the new theme and, for Omarchy, about 35 idle tick callbacks during the quiet period.
  - It excludes other threads and GPU time. The tick grid does not enter it.
- **UI CPU, release to +150 ms** (built-in) is the same thread's run time from just before the release to 150 ms after it, the window of the [2026-09-23 picker record](2026-09-23-theme-picker-switching.md). The pointer is still on the card then, and no tooltip can show before release plus 250 ms.
- **UI CPU, whole window** runs from the read before this switch to the read before the next. On the built-in path it adds the pointer leaving the card and the picker's hover transitions, 17 to 39 frames per window in both builds. That draw dominates the value and varies with the launch, so this metric does not resolve the change.
- **Harness `apply`** is one call of what `Palette::apply` does on the kit theme, timed with `Instant` around it:
  - base: `configure(is_light, theme)`;
  - candidate: `syntax_roles()`, `EditorHighlights::fit(palette, roles)` and `configure(is_light, roles, highlights, theme)`.

  The theme's highlight theme is the mode's shared default `Arc`, built before the clock starts, so `Arc::make_mut` clones it as after `Theme::change`. The call excludes `Theme::change`, the control button, `set_global`, `Theme::sync_base`, the editor restyle, `notify` and every frame. `fit alone` is `EditorHighlights::fit(palette, palette.syntax_roles())`.

Percentiles are by nearest rank. Δ is the candidate's pooled statistic minus the base's. Its 95% interval is a two-level bootstrap: 4,000 resamples, drawing launches with replacement within each build and then switches with replacement within each drawn launch, so launch-to-launch spread is in the interval.

## Setup

- **Builds**, designated in writing by the coordinator at 19:29 UTC on 2026-10-05, before the first recorded launch. The designation reads: "the base for every recorded launch is `.local/evidence/shared-base/gitturtle-base-e5afb22`, the release build of `e5afb228d53ebb4bf226cc8802c11ad7288ce737` (the run's accepted_head), sha256 `cb4ba80ab60961ef677b88ed26aa6e82a029a9825a1eab38442f52b1ff942b0d`, built 18:33–18:35 UTC today in its own fresh CARGO_TARGET_DIR. The candidate is `.local/evidence/editor-highlights-keep-syntax-readable/gitturtle-cand-3761488`, release build of `376148846a4431cfd2da88ac5cfd3f945a2a85b5`, sha256 `06ba761b949293ed6891541c59719ed9765adff424274dbae9065321e7a75fff`, built 18:31–18:33 UTC. `qa.py identity` already accepted the pair."
  - The driver read `--build-info` and the sha256 of both builds at the start of each block. Both are clean release builds for `x86_64-unknown-linux-gnu`, with rustc 1.99.0. Neither was rebuilt. The candidate's parent is the base.
- **Plan**, fixed in writing at 19:48:53 UTC, before the first recorded launch:
  - built-in launches in the order `B,C,C,B,B,C,C,B`;
  - Omarchy launches in the order `C,B,B,C,C,B,B,C`;
  - then the harness.

  The first attempt at 19:48:57 stopped on an error in the driver's run metadata before launching anything. The driver was fixed and its new sha256 appended to the plan at 19:49:19. The plan itself was unchanged.
- **Blocks:** built-in 19:49:24 to 20:06:53 UTC, Omarchy 20:07:03 to 20:19:46 UTC, harness 20:20:05 to 20:21:54 UTC, supplementary harness 20:28:08 to 20:29:01 UTC.
- **Host:**
  - Intel Core i9-10900K, 10 cores and 20 threads (CPU *n* pairs with *n* + 10), 61 GiB, NVIDIA GeForce RTX 3090 with the open kernel module 595.91.07.
  - Ubuntu 26.04.1 LTS, Linux 7.0.0-34-generic, GNOME Shell 50.1 (Wayland session), Xwayland 24.1.10 on `:0`, output 3840 × 2160 at 137.95 Hz, scale 1.
- **Pinning:**
  - Every app launch ran as `taskset -c 0-7 BINARY FIXTURE`, and the driver checked affinity 0–7 and `/proc/PID/exe` for each one.
  - The driver ran under `taskset -c 8,9`. Xwayland, gnome-shell and the GPU were not pinned.
  - The harness ran on CPU 2 alone, with no app running.
- **Frequency:** `intel_pstate`, `powersave` governor, energy preference `performance`, turbo on, mains power. At each new-theme frame the driver read the UI thread's CPU and its `scaling_cur_freq`:
  - built-in: p50 5,300 MHz in both builds, p5 5,100 (base) and 5,104 (candidate);
  - Omarchy: p50 5,298 and 5,300, p5 5,098 and 5,187;
  - all on CPUs 0–7.
- **Quiet host:**
  - The unattended loop was idle, waiting for this evidence, and no build ran during either native block.
  - A read-only design-review agent was allowed to run on CPUs 8, 9, 18 and 19 with no app launches. CPUs 8 and 9 were shared with the driver. Whether it ran during the blocks was not observed.
  - No other GitTurtle process ran.
  - The one-minute load average before each launch was 0.00 to 0.39.
  - The supplementary harness started 7 s after its executables finished building, when the load average still read 10.96 from that build. Its Midnight and Sandstone medians agree with the main run's within 1 µs.
- **Fixture:** `make_theme_fixture.py DEST`, the spec's budget fixture, at `/tmp/gitturtle-evidence/editor-highlights-perf/demo`, HEAD `79cc1067d345bf3230f657e19c44aed73ad3b583`.
  - It is the `scripts/create-demo-repo.py` repository plus 1,200 linear commits from `git fast-import`, with no system or global Git configuration and fixed identities and dates. A second generation gave the same HEAD.
  - The newest commit changes 23 lines of the 2,000-line `src/generated/large-module.ts`. History lists the demo's newer `experiment` commit first and this HEAD second.
  - Each launch checked HEAD, status and index before and after, and all 16 were unchanged.
- **Launch:** through `scripts/native_qa`'s `Session`, as `qa.py launch --input mutter` does, with two changes: the argv adds the pinning, and `GITTURTLE_TRACE=1` is set, with stderr read on a pipe and each line stamped on receipt. Each launch had:
  - a fresh HOME and XDG directories, the QA Git identity, and a generated store with Follow system off and interface text 13 pt: Midnight for built-in launches, `omarchy` for Omarchy launches;
  - `WAYLAND_DISPLAY` and `GIT_*` removed, `DISPLAY=:0` and `GPUI_X11_SCALE_FACTOR=1`;
  - the window found by `_NET_WM_PID`, resized to 1480 × 1100, a 5 s settle, activation and a pointer park.

  Each app was stopped with SIGTERM to its PID, and all 16 exited on it.
- **Setup per launch, not recorded:** Older (History 1–1000, waiting for `history_page_frame_ms`), HEAD's row, `large-module.ts` in its changed files (Compare, waiting for `file_preview_frame_ms`), then Split.
  - Built-in launches then open Settings through its toolbar button, which keeps the Split comparison behind it. All twenty built-in cards are on screen without scrolling at 1480 × 1100.
  - Omarchy launches stay on the Split comparison.
- **Built-in input**, through `org.gnome.Mutter.RemoteDesktop`, never XTest. Every 1.2 s:
  - one relative motion onto the target card's preview;
  - the button pressed 150 ms later and released 100 ms after that (the release is t_send);
  - at t_send + 200 ms, one motion off the window's right edge.

  Tooltips show 500 ms after hover starts, so none showed. Cards were clicked in `ThemeChoice::ALL` order from Midnight: 4 warm-up switches, then 80 recorded (four full cycles from Porcelain, so 4 per palette per launch and 16 per palette per build), then 5 idle windows. The store was read at the end of each window.
- **Omarchy input:** every 1.5 s the driver performed `omarchy-theme-set`'s writes in the HOME's `current/`: stage `next-theme/` with the other theme's `colors.toml` (the app's own `tests/fixtures/omarchy/` files), remove `theme/`, rename `next-theme/` to `theme/`, then write `theme.name`. The themes alternated Catppuccin Latte and Tokyo Night: 4 warm-up, 40 recorded (20 per theme), 5 idle windows.
- **Cache state:**
  - The kernel page cache was warm: pilots minutes earlier had read the fixture and both executables.
  - Every launch had new XDG directories, so Mesa's shader cache and the app's stores started cold. Setup and 4 warm-up switches came before the recorded ones.
  - Harness: steady state after 200 untimed calls per palette per process, plus first calls in fresh processes.
- **Harness build:** each tree was exported with `git archive` at its revision. The harness module (`bench/harness/common.rs.in` plus `base.rs.in` or `cand.rs.in`) was appended to `crates/app/src/appearance.rs`, and the cases (`cases.rs.in`) to `crates/app/src/appearance/omarchy.rs`. Each was built with `cargo test --release --locked -p gitturtle --bins --no-run`, which uses the release profile (thin LTO, 8 codegen units, opt-level 3), in its own target directory.
  - Builds finished at 19:38 and 19:45 UTC, before the first recorded launch. The candidate's rustc crashed twice with this host's known SIGSEGV in LLVM before building on the third try.
  - The executables' sha256 values are `3f697452…` (base) and `086be967…` (candidate). The targets were deleted.
  - The supplementary run's executables (`64879f18…`, `460448a9…`) were built after both native blocks, from the same files with only the two stress cases appended to the case list.
- **Pilots, not recorded:** 3 exploratory launches for geometry and 5 driver pilots of the candidate: two stopped during setup on driver errors, one tried a keyboard route that selected no card, then one built-in and one Omarchy pilot. They ran from 19:36 to 19:48 UTC, partly while the harness was building. None is in the results.

## Commands

From the repository root, with the QA virtual environment, on `:0` under GNOME, with `BENCH` the directory holding the driver:

```sh
python3 $BENCH/make_theme_fixture.py /tmp/gitturtle-evidence/editor-highlights-perf/demo
taskset -c 8,9 .local/qa-venv/bin/python3 $BENCH/theme_switch_x11.py run builtin BASE CAND FIXTURE builtin.json B,C,C,B,B,C,C,B
taskset -c 8,9 .local/qa-venv/bin/python3 $BENCH/theme_switch_x11.py run omarchy BASE CAND FIXTURE omarchy.json C,B,B,C,C,B,B,C
$BENCH/harness/build.sh CLONE e5afb228d53ebb4bf226cc8802c11ad7288ce737 base SRC_BASE TARGET_BASE
$BENCH/harness/build.sh CLONE 376148846a4431cfd2da88ac5cfd3f945a2a85b5 cand SRC_CAND TARGET_CAND
$BENCH/harness/run.sh BASE_TEST_EXE CAND_TEST_EXE harness.jsonl 2 10 5
.local/qa-venv/bin/python3 $BENCH/theme_switch_x11.py analyze builtin.json   # and omarchy.json
.local/qa-venv/bin/python3 $BENCH/theme_switch_x11.py export builtin.json RECORD-builtin.csv builtin-meta.json
python3 $BENCH/harness/analyze.py harness.jsonl harness-summary.json
```

The driver, fixture generator and harness files land in their own commit, because evidence commits hold no code:

- driver `theme_switch_x11.py`: sha256 `f3228ad7…` took the samples. The landed copy, `052b1543…`, differs only in `analyze` skipping a per-palette metric that the Omarchy mode does not produce.
- fixture generator `make_theme_fixture.py`: `a469554b…`.

The harness's full per-call samples (`harness.jsonl`, sha256 `b488aadf…`; stress `bb84aade…`) and the drivers' raw run files are retained outside the repository with the evidence. The CSV keeps each process's n, min, p50, p95 and max and every first-call sample.

## Not measured

- **The theme editor's live preview** (`theme_edit_frame_ms`, and a colour-picker drag that applies once per frame). The per-apply cost above bounds what the change adds there. No edit or drag was timed.
- **The spec's two custom themes in the switching cycle.** The picker cycle covered the twenty built-ins only. Custom palettes enter through the harness's two stress drafts.
- **Presentation and GPU time,** a cold page cache, a warm shader cache, Wayland, macOS and any other host.
