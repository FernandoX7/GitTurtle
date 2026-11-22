# Wave 3A Runtime Performance & Memory Quick Wins — Release Benchmark Record

- **Date:** 2026-11-22
- **Baseline:** `main` at `c35b267f913c5226eb9e4736f8a163df5383ba99`
- **Candidate branch:** `perf/wave3a-quick-wins`
- **Host:** `Linux-7.2.11-14-1-ARCH-aarch64-with-glibc2.43` (Apple M2 Pro, Omarchy Linux), pinned to P-cores `4-11` (`taskset -c 4-11`), quiet build slot active (`scripts/dev/build_slot.py quiet-on`)
- **Toolchain:** `rustc 1.100.0 (0ef1bc386 2026-11-09)`, `git version 2.56.0`, `--release` profile

## 1. Core Git Read & Partial-Staging Hot Paths (`gitturtle-core`)

Same-session alternating comparison (3 warmup rounds + 15 alternating rounds flipping baseline/candidate order each round) against a disposable 500-commit fixture repository with 4 configured remotes (including dotted remote names and an `insteadOf` rewrite rule) and a 420-line multi-commit file with two separated working-tree hunks.

- **Baseline binary SHA-256:** `75fb0511fa5f71782227bb6d1c3672000d49821ebab431465d38ee15086c168b`
- **Candidate binary SHA-256:** `16aebeee84cd3f657e9f16621a25016947b658336cbfada3b745c655d64d5dfa`

| Operation | Baseline median (IQR) | Candidate median (IQR) | Candidate p95 | Delta | Significance |
| --- | ---: | ---: | ---: | ---: | --- |
| `GitRepository::open` | 2.146 ms (0.324 ms) | 1.423 ms (0.189 ms) | 1.711 ms | **-33.7%** | Significant |
| `repo.status()` | 2.695 ms (0.519 ms) | 1.827 ms (0.296 ms) | 2.313 ms | **-32.2%** | Significant |
| `repo.remotes()` (4 remotes + `insteadOf`) | 6.513 ms (0.862 ms) | 0.774 ms (0.188 ms) | 1.004 ms | **-88.1%** | Significant |
| `repo.operation_state()` | 0.769 ms (0.148 ms) | 0.016 ms (0.002 ms) | 0.026 ms | **-97.9%** | Significant |
| `repo.local_watch_policy()` | 3.428 ms (0.558 ms) | 1.539 ms (0.153 ms) | 1.719 ms | **-55.1%** | Significant |
| `repo.blame(Working)` (420 lines) | 12.377 ms (2.852 ms) | 9.212 ms (1.561 ms) | 11.430 ms | **-25.6%** | Significant |
| `repo.history_traversal` (500 commits) | 3.409 ms (0.951 ms) | 3.474 ms (0.537 ms) | 4.674 ms | +1.9% | Within noise floor |
| `repo.execute(ApplyPartial)` (hunk stage) | 6.191 ms (0.799 ms) | 4.957 ms (0.581 ms) | 5.893 ms | **-19.9%** | Significant |

### Raw samples (ms, 15 rounds)

- `open_ms`:
  - baseline: `[2.627, 2.158, 2.034, 2.051, 2.397, 2.206, 1.805, 2.146, 2.132, 2.092, 2.128, 2.417, 1.949, 2.393, 2.580]`
  - candidate: `[1.370, 1.546, 1.683, 1.252, 1.432, 1.353, 1.335, 1.364, 1.417, 1.423, 1.711, 1.420, 1.495, 2.224, 1.565]`
- `status_ms`:
  - baseline: `[2.587, 2.381, 2.421, 2.978, 2.828, 2.600, 2.020, 3.101, 2.413, 2.873, 2.695, 2.430, 3.060, 2.910, 3.253]`
  - candidate: `[1.565, 2.098, 1.892, 1.390, 1.909, 1.827, 1.580, 1.743, 1.654, 1.699, 1.969, 2.313, 1.756, 2.336, 1.976]`
- `remotes_ms`:
  - baseline: `[5.876, 6.121, 6.133, 6.474, 5.824, 7.843, 6.513, 6.944, 6.012, 6.708, 6.176, 7.034, 6.714, 7.171, 7.215]`
  - candidate: `[0.876, 0.776, 0.930, 0.610, 0.744, 0.672, 0.626, 0.778, 0.675, 0.736, 0.774, 0.909, 0.758, 1.209, 1.004]`
- `op_state_ms`:
  - baseline: `[0.780, 0.763, 0.688, 0.830, 0.704, 1.054, 0.640, 0.856, 0.769, 0.794, 0.701, 0.622, 0.956, 0.881, 0.630]`
  - candidate: `[0.018, 0.016, 0.029, 0.015, 0.016, 0.013, 0.016, 0.017, 0.015, 0.017, 0.012, 0.013, 0.017, 0.026, 0.017]`
- `local_watch_ms`:
  - baseline: `[3.428, 2.995, 3.230, 3.087, 3.003, 3.400, 3.421, 3.145, 3.708, 4.007, 4.103, 3.543, 3.824, 3.783, 3.686]`
  - candidate: `[1.440, 1.586, 1.450, 1.402, 1.520, 1.719, 1.508, 1.562, 1.263, 1.647, 1.571, 1.539, 1.477, 1.666, 1.745]`
- `blame_ms`:
  - baseline: `[10.597, 11.950, 10.606, 14.838, 11.309, 12.377, 15.166, 11.130, 12.317, 13.139, 16.469, 15.163, 12.890, 13.306, 10.695]`
  - candidate: `[11.430, 8.516, 8.875, 9.148, 9.390, 8.554, 11.200, 9.212, 8.768, 10.009, 10.361, 8.367, 8.677, 11.897, 10.205]`
- `history_500_ms`:
  - baseline: `[3.714, 3.409, 3.061, 4.346, 2.670, 3.500, 5.070, 2.671, 3.296, 3.409, 5.333, 3.097, 3.491, 4.615, 2.464]`
  - candidate: `[3.244, 3.789, 2.857, 3.474, 4.674, 3.793, 4.660, 3.688, 3.336, 3.094, 3.263, 3.332, 2.665, 5.714, 3.639]`
- `apply_partial_ms`:
  - baseline: `[6.744, 6.130, 6.595, 6.754, 5.660, 6.474, 7.000, 4.981, 5.811, 6.123, 6.361, 5.080, 6.191, 6.335, 4.646]`
  - candidate: `[5.893, 5.282, 4.393, 4.957, 5.167, 4.671, 5.164, 4.811, 5.343, 4.793, 4.342, 4.864, 4.552, 5.803, 6.245]`

## 2. Linux Confined PDF Helper: Pipe Stdin vs Sealed `memfd_create`

Same-session alternating comparison on the candidate release `gitturtle` binary (`--gitturtle-pdf-helper`), 3 warmup + 15 alternating rounds on a 16 MiB (`16,773,748` bytes) valid PDF document, pinned to P-cores `4-11` under `quiet-on`.

- **Release binary SHA-256:** `416c19e886c31277254230292624bea813c8d51aef0d9bd845ccd05abf546692`

| Transport | Median (IQR) | p95 | Min / Max | Delta | Significance |
| --- | ---: | ---: | ---: | ---: | --- |
| Pipe stdin (baseline path) | 13.697 ms (0.997 ms) | 14.737 ms | 12.866 / 14.918 ms | — | — |
| Sealed `memfd_create` + `PROT_READ` `mmap` | 4.962 ms (0.491 ms) | 5.422 ms | 4.515 / 5.563 ms | **-63.8%** | Significant |

### Raw samples (ms, 15 rounds)

- `pipe`: `[14.737, 14.918, 13.657, 14.606, 13.210, 12.866, 13.518, 14.000, 12.971, 14.479, 13.697, 14.210, 13.218, 14.118, 13.477]`
- `sealed_memfd`: `[4.962, 5.367, 5.273, 5.563, 5.422, 4.909, 4.750, 4.946, 5.071, 4.706, 5.397, 5.151, 4.914, 4.559, 4.515]`

## 3. Structural Memory & Startup Entity Reductions (Verified by Unit/GPUI Tests)

- **Image preview CPU memory (`ImageSide` + in-place RGBA→BGRA swizzle):** Eliminates the duplicate full-resolution `Vec<u8>` RGBA pixel buffer on `ImageSide` (`ImageMetadata` replaces `ImagePreview`) and swizzles RGBA→BGRA in place across `worker`, `gif_playback`, `pdf_view`, `model_view`, `markdown_view`, and `rich_preview`. Verified by `worker::tests::render_pixels_are_bgra_in_place_without_retaining_duplicate_rgba` and `gif_playback::tests::prepares_all_bgra_frames_once_and_keeps_metadata_and_timing`.
- **Lazy `SplitPresentation`:** Unified/Before/After text previews defer `SplitPresentation::prepare` (`split: None`) until Split mode is chosen (`Job::PrepareSplit`), keeping `PreviewCache::bytes` exact across upgrades and evictions. Verified by `worker::tests::lazy_split_defers_allocation_until_requested_and_keeps_cache_bytes_exact` and `phrase_tests::lazy_split_loads_on_demand_and_cancels_when_returning_to_history`.
- **Lazy startup construction:** Launching directly into a repository defers Settings UI (`SettingsPage` + 4 `InputState` entities), theme picker card entities (`ThemePreviewBody` + `ThemeCardBody` + focus handles), and `ProjectHub` search/form `InputState` entities until first entry. Verified by `phrase_tests::repo_startup_defers_settings_theme_cards_and_project_hub_inputs`.
- **Commit graph rendering:** Straight vertical pass-through and incoming lane segments paint directly via `window.paint_quad(fill(...))` snapped to uniform device-pixel width with horizontal viewport culling instead of `PathBuilder::stroke` CPU tessellation. Verified by `graph::tests::straight_vertical_lanes_paint_as_masked_quads_and_skip_offscreen_or_filtered`.

## 4. Scope & Unexercised Platforms

- Measured locally on Linux ARM64 (Apple M2 Pro P-cores). Live macOS desktop GPU frame timing and macOS `waitid`/`poll` latency were not benchmarked on local macOS hardware; macOS compilation and full test suites are verified on GitHub Actions `macos-15` runners.
