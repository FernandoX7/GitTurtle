# Theme application trace baseline, September 18, 2026

This records the first release baseline for [`gitturtle.theme_apply_frame_ms`](metrics.md) (task `themes-apply-trace`, criterion `baseline-recorded`) against the budget in the [themes specification](../development/themes/spec.md#performance): median ≤ 8 ms, p95 ≤ 16 ms. The raw data, including every sample, per-launch summaries and the supplementary observations below, is in [2026-09-18-theme-apply.json](2026-09-18-theme-apply.json).

**Result.** 60 switches, one recorded launch: median **3.827 ms**, p95 **7.669 ms**, max **8.065 ms** (min 0.494 ms, mean 4.001 ms). The budget holds for the metric as implemented. The metric's end point, however, precedes the layout and paint of the frame that shows the new theme; see [What the value measures](#what-the-value-measures) before using it as the instrument for later editor and picker tasks.

## Exercised build and environment

`--build-info` of the exercised executable:

```json
{
  "application": "GitTurtle",
  "version": "0.1.0",
  "source_revision": "79242444c6b07c3b8430e67e713e2c57f940848e",
  "source_tree": "clean",
  "target": "x86_64-unknown-linux-gnu",
  "profile": "release",
  "rustc": "rustc 1.98.0 (88d9e12ae 2026-08-18)",
  "build_unix_seconds": "1789767942"
}
```

The executable SHA-256 was `6f84cf3802267305b2988291b00693267ecd89079275e658413eb8612ad694af`. This record applies to that executable only.

Host: Intel Core Ultra 9 275HX (24 threads), 188 GiB RAM, Pop!_OS 24.04 LTS, Linux `7.1.5-76070105-generic`, Git 2.43.0. The app ran as an X11 client of XWayland (`DISPLAY=:1`, `WAYLAND_DISPLAY` unset) on GNOME Shell 46.0 Wayland. The eDP-1 panel is 3840×2400 at 119.98 Hz with a Mutter scale of 2.0. The app used `GPUI_X11_SCALE_FACTOR=2` at its default 1480×980 logical window (2960×1960 physical), which fits the screen and was not resized.

Rendering ran on the Intel Graphics (ARL) iGPU (i915, Mesa 26.1.6). Its DRM client accumulated 10.97 s of render-engine time and 245 MB resident during the recorded launch. The NVIDIA RTX 5090 Laptop GPU (driver 595.84) was opened only while adapters were enumerated. The app does not log the wgpu backend.

The machine was on AC, with the `powersave` governor and `balance_performance` EPP. The owner's desktop session stayed active (shell, browser, editors), so background load was uncontrolled. Load average was 0.46/0.77/1.08 before the recorded launch and 0.56/0.64/0.93 after it.

Cache state: warm. An exploratory launch, two 3-switch smoke launches and a discarded full 60-switch warm-up launch of the same executable on the same fixture preceded the recorded launch, and caches were not dropped. The OS page cache held the executable and fixture, and the Mesa shader cache was warm. Each launch used a fresh process and a fresh, absolute scratch preference store, seeded as `{version 5, theme midnight}` and migrated by the app to version 6. No cold-cache run was made.

## Fixture and state before the first switch

The fixture is the `scripts/create-demo-repo.py` repository (7 commits) plus 1,200 linear commits appended with `git fast-import` by the evidence generator `make_fixture.py`. That gives 1,207 commits on `main` with HEAD `c8f20cfeb3f846168abbdcde270983c42bb27e71`. The newest commit ("Rebalance the generated ledger module") changes 23 lines of the 2,000-line `src/generated/large-module.ts`.

Each fixture precondition was checked in every launch:

| Precondition | How it was verified |
| --- | --- |
| At least 1,000 commits loaded | Page 1 (500 rows) loaded on open. The explicit Older control appended page 2, printing `gitturtle.history_page_frame_ms=8.235`, and the History header then read "1–1000". Older pages load only through that control, not by scrolling the list. |
| 2,000-line comparison in Split | Row 1 (`c8f20cf`) is selected on open. Activating `large-module.ts` in Changed files printed `gitturtle.file_preview_frame_ms=10.418`; the Split segment was then chosen and captured with both panes showing. |
| Comparison retained behind Settings | Settings opened through Menu → Open Settings. After the 60 switches, Back showed the same Split comparison in the new theme with **no** new `file_preview_frame_ms` line, so nothing was re-requested. A second Back showed History still at "1–1000" with row 1 selected. |

The fixture's HEAD, commit count and clean status were unchanged after all launches.

## Procedure

The driver made 60 real XTest pointer clicks on theme cards in Settings. The sequence followed `ThemeChoice::ALL` (`crates/app/src/appearance.rs`) starting from Midnight: ALL[1] … ALL[19], ALL[0], three full cycles, so every click changed the selection. Each click was a pointer move, 150 ms, press, 150 ms hold, release, and at least 1.2 s without input preceded every click. Light cards were clicked with the page unscrolled and dark cards after 10 wheel steps (630 logical px), where all 12 dark cards are wholly visible.

Every switch was verified before the next one:

- **Before the click:** the target card's top border lay at its expected row in the previous theme's border colour.
- **After the click:**
  - that border was 100% the new theme's accent;
  - the previously selected card, when on screen, had lost its accent;
  - the scratch store named the new theme;
  - exactly one new `gitturtle.theme_apply_frame_ms=` line had appeared.

The launch would have aborted on any failure; none occurred. It produced 60 lines for 60 clicks, in click order. Checkpoint screenshots after switches 1, 20, 40 and 60 show the expected card selected.

Custom themes are not in the cycle. The specification names "every built-in and two custom themes", but custom themes have no Settings UI yet and `choose_theme` traces built-ins only. This task's contract (60 switches cycling `ThemeChoice::ALL`) governs here.

Percentiles use the nearest-rank method: p95 of 60 samples is the 57th smallest.

## Samples

Values are in milliseconds, with the switch number in parentheses. Rows follow the cycle order, and each row is the switch *to* that theme.

| Target | Cycle 1 | Cycle 2 | Cycle 3 |
| --- | ---: | ---: | ---: |
| Daylight | 5.747 (#1) | 4.946 (#21) | 3.718 (#41) |
| Graphite | 0.941 (#2) | 0.681 (#22) | 7.558 (#42) |
| TokyoNight | 3.471 (#3) | 7.669 (#23) | 5.931 (#43) |
| CatppuccinMocha | 0.494 (#4) | 5.719 (#24) | 3.564 (#44) |
| Nord | 5.826 (#5) | 3.658 (#25) | 0.596 (#45) |
| Porcelain | 2.940 (#6) | 6.949 (#26) | 3.636 (#46) |
| Sandstone | 1.508 (#7) | 5.205 (#27) | 0.675 (#47) |
| DeepSea | 5.031 (#8) | 2.201 (#28) | 5.476 (#48) |
| Ember | 3.920 (#9) | 0.987 (#29) | 2.260 (#49) |
| SolarizedDark | 2.026 (#10) | 6.862 (#30) | 7.800 (#50) |
| SolarizedLight | 6.698 (#11) | 3.977 (#31) | 4.961 (#51) |
| OneDark | 2.926 (#12) | 1.432 (#32) | 0.600 (#52) |
| OneLight | 7.314 (#13) | 7.348 (#33) | 4.486 (#53) |
| RosePine | 4.703 (#14) | 3.686 (#34) | 0.515 (#54) |
| RosePineDawn | 2.265 (#15) | 0.547 (#35) | 1.787 (#55) |
| Dracula | 8.065 (#16) | 6.510 (#36) | 5.357 (#56) |
| Alucard | 3.459 (#17) | 4.468 (#37) | 2.551 (#57) |
| KanagawaWave | 0.522 (#18) | 2.475 (#38) | 7.861 (#58) |
| KanagawaLotus | 5.787 (#19) | 7.308 (#39) | 4.830 (#59) |
| Midnight | 3.734 (#20) | 4.528 (#40) | 1.366 (#60) |

| n | min | median | mean | p90 | p95 | max | Budget |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 60 | 0.494 | 3.827 | 4.001 | 7.314 | 7.669 | 8.065 | median ≤ 8 and p95 ≤ 16: holds |

Other launches of the same procedure, which are not the recorded baseline, had medians of 4.008 ms (warm-up), 2.789 ms (control), 3.857 ms (pixel polling) and 3.519 ms (20-switch bursts). Their p95 ranged from 6.861 to 7.438 ms and their max from 7.891 to 8.128 ms. This spread between launches is sampling noise from the refresh-phase mechanism below, not a difference in work.

## What the value measures

The trace starts at `choose_theme` entry and prints from a `Window::on_next_frame` callback.

- **The callback runs before the frame is drawn.** In gpui-pre 0.3.4 (`window.rs`, lines 1762–1789), `request_frame` runs pending next-frame callbacks first, then `window.draw()` and `present()`. In all 60 recorded switches, the `gitturtle.layout` line printed at the start of the root render arrived after the theme line (median 0.047 ms later). The printed value therefore ends **before** the layout and paint of the frame that shows the new theme. The [metric definition](metrics.md) in candidate `7924244` says the value includes "the layout and paint of that frame"; that statement is not accurate for this GPUI version. The catalog entry was corrected in `76a8fba`: [`gitturtle.theme_apply_frame_ms`](metrics.md) now ends at the next-frame callback, as the existing `*_frame_ms` entries do, and no longer claims that frame's layout and paint.
- **The value is mostly the wait for the next refresh tick.** On X11, GPUI drives frames from a fixed-cadence timer at the display rate (`gpui-pre-linux` 0.3.4, `x11/client.rs`, lines 1985–2017; 8.334 ms here), while pointer input is dispatched as it arrives. In the recorded launch, every trace line arrived on that timer's grid (phase standard deviation 0.108 ms, largest deviation 0.507 ms), handler entries were spread across the whole period (0.32–8.33 ms), and no sample exceeded one period. The value is the handler, which the 0.494 ms minimum sample bounds from above, plus the wait for the next tick. Its median is set by the display rate and input phase, not by theme-application work. By the same mechanism, a 60 Hz display would be expected to yield a median near 8.3 ms from the wait alone. This is inferred, not measured.
- **Input delivery was negligible.** The time from the XTest release (after `XSync`) to the line's arrival on the stderr pipe exceeded the printed value by a median of 0.134 ms (max 0.269 ms).

## Supplementary observations

These come from separate launches of the same executable, fixture, window and procedure. They do not attribute cost to functions, because `perf` is not installed on this host.

**UI-thread CPU per switch.** This is the main thread's `/proc/<pid>/task/<pid>/schedstat` delta from just before the release to 150 ms after it. The control launch also measured three other 150 ms windows on the same Settings page, giving four (min / median / p95 / max, ms):

| Window | UI-thread CPU (ms) |
| --- | ---: |
| Idle | 0.44 / 0.73 / 1.19 / 1.28 |
| Hover repaint (pointer onto a card) | 10.31 / 13.88 / 17.74 / 18.68 |
| Press repaint | 12.10 / 13.58 / 18.12 / 20.03 |
| Theme switch | 18.41 / 25.72 / 29.00 / 36.52 |

The recorded launch agrees: 17.91 / 26.43 / 28.90 / 30.90 ms per switch.

**Two renders per switch.** A 20-switch launch sampled UI-thread CPU about every 1 ms after each release.

- **Separable (13 of 20):**
  - a first burst of 12.97 ms median CPU (handler plus the first frame after the callback), ending a median 19.35 ms after the release;
  - then a second full render starting a median 22.93 ms after the release, costing 10.35 ms median CPU (range 7.43–14.63).
- **Merged (7 of 20):** the two renders formed a single burst of 26.16 ms median CPU.

The first frame costs about the same as an ordinary hover or press repaint of this page at 2×.

The second render's timing is consistent with the preference-save completion. `save_preferences` (`crates/app/src/settings.rs`) always calls `cx.notify()` when its off-thread write finishes. That write is `sync_all`, rename, then a directory `sync_all` (`crates/app/src/preferences.rs`). A reproduction of that sequence on the same ext4 NVMe filesystem took 16.54 ms median (p95 17.15 ms, 40 repetitions), mostly the file `fsync` (13.48 ms). That places the completion after the first frame in every case. This is an inference from timing, not a verified attribution.

**Release to visible pixels.** In a separate launch, the driver polled a 2-row strip of the target card's border after each release until it showed the new accent. Median 47.93 ms, p95 59.94 ms, max 72.23 ms, min 28.08 ms (n = 60). The trace in the same launch read median 3.857 ms. This figure includes presentation through XWayland, and the polling itself loads the X server during the frame. Treat it as an order of magnitude, not a budget figure.

## Limitations

- The value excludes the frame that shows the new theme, and on this display its distribution is dominated by refresh phase. It therefore does not test the specification's intent of one 60 Hz frame. Deciding the instrument's boundary is left to the owner; the evidence is above.
- Linux XWayland on GNOME Wayland only: not native Wayland and not macOS. Input is synthetic XTest; the display is 120 Hz at 2× scale.
- One recorded launch of 60 samples, with warm caches and uncontrolled desktop load.
- Custom themes were not exercised (see [Procedure](#procedure)).

## Reproduction

The evidence generator (`make_fixture.py`), the driver (`drive_theme_apply.py`, with `launch.py`, `analyze.py` and `build_record.py` beside it) and per-launch data are retained outside the repository in the themes evidence directory (`perf-theme-apply/runs/`). Per-launch data comprises `stderr.log`, `stderr-timed.log`, `samples.json` and screenshots. Script digests are in the JSON.

Every launch used an absolute, freshly created `XDG_CONFIG_HOME` and `GITTURTLE_TRACE=1`. The driver terminated only the process it launched. After the recorded launch, the driver gained supplementary-only options (the idle, hover and press CPU controls, `--pixels` and `--bursts`). The recorded launch ran the same navigation, click timing and verification path without the control `/proc` reads.
