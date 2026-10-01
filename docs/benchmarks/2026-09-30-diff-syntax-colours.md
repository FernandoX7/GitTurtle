# Diff syntax colours: theme application cost, 2026-09-30

This is the release measurement for task `diff-syntax-colours-from-palette`. The task makes `Palette::configure` (`crates/app/src/appearance.rs`) fit the editor syntax roles for every palette. `Palette::syntax_colors` fits seven role colors with `SyntaxTargets::fit` and rebuilds the toolkit's 42 syntax styles through a serde round trip (`restyle`).

Two layers were measured against a base from the same session:

- **`configure` alone**, in an optimized harness, for Midnight, Daylight, the Omarchy Tokyo Night mapping, two custom themes and all twenty built-ins.
- **Theme application in the running app**, for Omarchy switches between Tokyo Night and Catppuccin Latte, through [`gitturtle.omarchy_apply_frame_ms`](metrics.md) and UI-thread CPU per switch.

Raw data, per-launch samples and digests are in [the JSON](2026-09-30-diff-syntax-colours.json).

## Result

`configure` costs about 35 to 42 µs more per call in steady state, and about 46 µs more on a first call in a fresh process. In the app, an application costs about 0.13 ms more at p50 and 0.15 ms more at p95. Both are far inside the [themes budget](../development/themes/spec.md#performance) of 8 ms median and 16 ms p95.

**`configure_us`**: one call, in µs. Steady state pools 20,000 calls per palette from 10 processes. First call is one call in each of 40 fresh processes.

| Palette | Roles fitted | Base p50 / p95 / max | Candidate p50 / p95 / max | Δ p50 | First call, base p50 / p95 / max | First call, candidate p50 / p95 / max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Midnight (dark built-in) | 0 | 4.75 / 4.89 / 56.8 | 40.09 / 42.04 / 128.7 | +35.3 | 12.29 / 15.09 / 15.85 | 58.53 / 72.35 / 98.27 |
| Daylight (light built-in) | 1 | 4.75 / 4.89 / 897.5 | 41.97 / 44.28 / 283.8 | +37.2 | 12.08 / 14.81 / 15.71 | 60.06 / 71.45 / 100.50 |
| Omarchy Tokyo Night (mapped) | 1 | 4.75 / 4.89 / 54.8 | 41.00 / 43.09 / 274.4 | +36.2 | 12.71 / 14.88 / 14.95 | 58.67 / 63.00 / 101.20 |
| Custom "Faint statuses" (Porcelain, faint keyword and type) | 2 | 4.75 / 4.89 / 80.1 | 43.02 / 45.19 / 910.6 | +38.3 | 12.29 / 15.16 / 16.34 | 61.60 / 67.33 / 103.36 |
| Custom "Midnight copy" | 0 | 4.75 / 4.89 / 52.4 | 40.02 / 41.98 / 123.5 | +35.3 | 12.22 / 14.88 / 15.71 | 57.27 / 95.89 / 106.79 |
| Kanagawa Lotus (slowest built-in) | 5 | 4.89 / 5.31 / 65.2 | 46.65 / 52.38 / 154.1 | +41.8 | — | — |

Across all twenty built-ins, the p50 ranges from 4.75 to 5.24 µs on the base and from 39.60 to 46.65 µs on the candidate. Process medians stay within about 1 µs of each other for each palette, so noise is about 1 µs against a difference of 35 µs. The timer floor is 1.40 µs at p50. The maxima in both builds are single preemptions.

`syntax_colors` alone takes 36.7 µs at p50 on Midnight and 39.0 µs on the custom theme. Most of the added cost is the 42 serde round trips; the fitting itself adds up to about 7 µs for five fitted roles.

**In the app**: Omarchy switches, eight launches in the order B C C B B C C B, with 40 switches per launch. Values are in ms.

| Path | Metric | Build | n | p50 | p95 | max |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| Switch to Tokyo Night | `omarchy_apply_frame_ms` | base | 80 | 0.463 | 0.624 | 1.579 |
| Switch to Tokyo Night | `omarchy_apply_frame_ms` | candidate | 80 | 0.588 | 0.749 | 0.769 |
| Switch to Catppuccin Latte | `omarchy_apply_frame_ms` | base | 80 | 0.453 | 0.537 | 0.551 |
| Switch to Catppuccin Latte | `omarchy_apply_frame_ms` | candidate | 80 | 0.605 | 0.708 | 0.752 |
| Both | `omarchy_apply_frame_ms` | base | 160 | 0.463 | 0.589 | 1.579 |
| Both | `omarchy_apply_frame_ms` | candidate | 160 | 0.592 | 0.741 | 0.769 |
| Both | UI-thread CPU per switch | base | 160 | 25.985 | 30.406 | 32.556 |
| Both | UI-thread CPU per switch | candidate | 160 | 23.744 | 29.927 | 32.433 |
| Idle control, 1.2 s | UI-thread CPU | base / candidate | 48 / 48 | 0.112 / 0.116 | 0.166 / 0.169 | 0.204 / 0.171 |
| Control: reread (unchanged code) | `omarchy_reread_ms` | base / candidate | 160 / 160 | 0.882 / 0.876 | 2.565 / 2.553 | 2.613 / 2.667 |

- **Application.** The launch medians do not overlap: base 0.437 to 0.489 ms, candidate 0.565 to 0.638 ms. The in-app delta is therefore real, at about +0.13 ms at p50 and +0.15 ms at p95. It is three to four times the harness's steady-state delta, which is what a cold call amid other work and different inlining would give. That explanation is inferred, not attributed. The reread runs on the background executor and contains no `configure`; it is unchanged, which serves as the control. Every switch printed exactly one application and no other trace line.
- **UI-thread CPU per switch.** Both builds have two modes: about 12 ms (37 and 42 of 160 switches) and about 26 ms (123 and 118). Launch medians spread from 19.6 to 26.5 ms. That noise is about 50 times the 0.13 ms delta, so this metric cannot show the change. The candidate's lower median is mode mixing, not a speedup.

## Budget

The budget in the [themes specification](../development/themes/spec.md#performance) is stated for `theme_apply_frame_ms`, the picker switch: median ≤ 8 ms and p95 ≤ 16 ms. That path was not measured natively here (see [Not measured](#not-measured)). It runs the same `apply_appearance` and therefore the same single `configure` per window. On this host's earlier record that switch measured p50 0.251 ms and p95 1.403 ms ([2026-09-28](2026-09-28-omarchy-theme.md), Midnight). On the X11 baseline host it measured p50 3.600 ms and p95 7.691 ms ([2026-09-23](2026-09-23-theme-picker-switching.md)). An added 0.05 to 0.15 ms leaves both bounds with several milliseconds to spare. The theme editor's live preview also applies on every edit, so its `theme_edit_frame_ms` gains the same 0.05 to 0.15 ms against its 26 ms p95 bound. That is within the bound's roughly 3 ms launch-to-launch spread, and it was not measured here.

## What the values measure

- **`configure_us`** is one call of `Palette::configure(is_light, &mut Theme)`, timed with `Instant` immediately before and after. The theme's highlight theme is the toolkit's shared default `Arc` for the mode, so `Arc::make_mut` clones it, as it does after `Theme::change` in the app. The value excludes `Theme::change`, the control button, `set_global`, `Theme::sync_base`, editor restyling, `notify` and every frame. No frame tick enters it.
- **`omarchy_apply_frame_ms`** starts when the reread's result arrives on the UI thread. It runs through `apply_appearance`, where `Palette::apply` calls `configure` once for the one window, and ends at that window's next-frame callback. It excludes the 250 ms quiet period, the reread (printed separately as `omarchy_reread_ms`), and the layout and paint of that frame.
  - On a presenting output, the callback follows the platform's frame request. The value then cannot fall below the handler plus the wait for the next frame, which is up to 16.7 ms at 60 Hz. A p95 criterion near one period on such an output would be decided by the boundary, not by the code.
  - In this run the output was DPMS-off and no value exceeded 1.6 ms, so the handler, not the frame grid, set every value. The 2026-09-28 run on a presenting 60 Hz output gave similar figures (0.510 / 0.582 ms), which is a consistency check only.
- **UI-thread CPU per switch** is the main thread's `/proc/<pid>/task/<pid>/schedstat` run time from just before the theme write to 1.2 s after it. It covers the quiet period, the application and the draws that follow. The idle control uses identical windows with no write.

## Setup

- **Builds.**
  - Base: `ca7b826`, release, clean tree, sha256 `4c2cb18e…`. It is the candidate's merge base. `origin/main` (`ec1d7d8`) differs from it only in `crates/preview/src/animation.rs` and the `git-core` tests, and the candidate does not contain those changes either.
  - Candidate: `e71ed69`, release, clean tree, sha256 `95eef802…`.
  - Both were designated in writing at 02:54:41Z, before the first native launch.
- **Harness.** One optimized build of each variant: rustc 1.98.0 at `-C opt-level=3 -C codegen-units=8`, linked against the release rlibs of the candidate's app build.
  - The appearance sources are the unmodified files at each revision. An identical hook module is appended to each to call `configure`, and crate-level stubs supply `desktop_text`, `preferences`, `graph` and `trace_enabled` with the source's values.
  - The harness has no cross-crate thin LTO, unlike the app.
- **Host.**
  - CPU: AMD 3020e, two cores with one thread each, 1.2 GHz base with boost to about 2.6 GHz, L2 1 MiB, L3 4 MiB. Memory: 5.7 GiB.
  - Frequency: `schedutil` governor (acpi-cpufreq), boost on, on AC. CPU 1 ran at 2.2 to 2.6 GHz at the start of each launch.
  - Software: Omarchy 4.0.4 on Linux 7.2.5-3-omarchy, Hyprland 0.56.2 on native Wayland.
  - Pinning: the harness and the app ran on CPU 1 (`taskset -c 1`), the driver on CPU 0.
  - Load: nothing else ran. Load average was 0.64 to 0.80 during the harness and 1.06 falling to 0.14 across the native launches.
- **Output.** A temporary headless Hyprland output, 1400 × 2100 at 60 Hz and scale 1, on workspace 9. The window was tiled at 1376 × 2050, and the output was DPMS-off on every recorded switch.
- **Cache state.** The page cache was warm for both executables, the rlibs and the fixture.
  - Harness: steady state after 200 warm-up rounds per process, plus one first call in each fresh process.
  - Native: every launch was a fresh process with fresh HOME and XDG directories, so Mesa and app caches started cold, followed by four unrecorded warm-up switches.
- **Fixture.**
  - Repository: `scripts/create-demo-repo.py`'s repository at `/tmp/gitturtle-evidence/syntax-perf-11/demo`, HEAD `52f471a`. HEAD and status were unchanged after the run.
  - Omarchy state: a fake `current/` per launch, seeded with tokyo-night and switched with `omarchy-theme-set`'s writes, in its order.
  - Store: version 6, theme `omarchy`, and one custom theme.
  - Input: none was sent; each app was stopped with SIGTERM by PID.

## Not measured

- **The picker switch** (`theme_apply_frame_ms`) for Midnight, Daylight and the custom theme. Hyprland refused keyboard focus to the app's window for the whole session, and the terminal kept it. An open Omarchy menu is the likely holder, but that was not verified. These palettes are measured in the harness only.
- **Presentation.** Every output went DPMS-off within seconds of the headless output's creation. Both builds ran in that state, so the comparison is like-with-like, but nothing here measures presentation.
- **The theme editor's edit path.** It applies on every edit and was not measured, and neither was a retained comparison behind Settings.
- **Other platforms and caches.** There was no X11 or XWayland run, no macOS run and no cold page cache, and only this two-core host was used.

## Reproduction

The harness, driver, logs and raw samples are retained outside the repository in `/tmp/gitturtle-evidence/syntax-perf-11/tools/`:

- `bench/` holds the harness: `build.sh` and `run.sh` for building and running it, and `analyze.py` for the summary, with the raw samples in `results.jsonl`.
- `native/` holds the native driver: `omarchy_ab.py BCCBBCCB` with `lib.py` and `launch.sh`, and `analyze_native.py` for the summary.

The native run is `/tmp/gitturtle-evidence/runs/syntax-perf-omarchy-BCCBBCCB-1790823845/all.json`. `/tmp` does not survive a reboot.
