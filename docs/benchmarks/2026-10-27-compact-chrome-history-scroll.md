# History scrolling after the compact chrome and narrow History, Wayland, 2026-10-27

This is the release measurement for two History changes on branch `claude/omarchy-support`: the compact chrome `d1299fb` against its parent's build `0dd91c0` (product code of `c7ad38f`), and narrow History (`f5e70d3`, finished by `c2ac707`, `b2f6ee3` and `3a3c56f`) as built at `0aca11e` against `2913b86`, its last build before narrow History. It asks whether History's wheel scrolling costs more UI-thread time at 1480 × 800 or 461 × 490 on the disposable 3,000-commit repository of the [October 5 record](2026-10-05-history-columns-steady-after-toggle.md).

Every per-notch sample of the quiet-host run, with the UI thread's CPU and that CPU's frequency, is in [`2026-10-27-compact-chrome-history-scroll.csv`](2026-10-27-compact-chrome-history-scroll.csv). The builds' checksums, both runs' statistics and the per-launch metadata are in [`2026-10-27-compact-chrome-history-scroll.json`](2026-10-27-compact-chrome-history-scroll.json).

## What changed

`d1299fb` decides once per window size whether the header and toolbars take their compact form; the chrome widths are measured once and cached. Narrow History folds References into a pill below a measured width, gives the graph a compact lane pitch, keeps a floor for the subject's own text and fits the table to its viewport. Its widths are recorded once per frame (`views::HistoryMetrics`) and text is measured only when the font, size or digit count changes; rows measure nothing (`narrow_history_scrolls_without_measuring_text` asserts that steady and scrolled frames lay out no text). `0aca11e` also carries the menu, Working Changes and review-panel changes made between them; none of them is on History's scroll path.

## Setup

- **Host:** Apple M2 Pro running Asahi Linux with Omarchy, Hyprland 0.56, native Wayland at scale 2. Each app launch pinned with `taskset` to CPUs 8–11, one performance cluster. AT-SPI off; notifications silenced; window forced opaque; empty XDG homes; Midnight at 13 pt.
- **Fixture:** `/tmp/gitturtle-evidence/history-scroll/history`, 3,000 commits and 121 references from the October 5 generator, HEAD `1e65ca0`, on tmpfs.
- **Builds:** clean release executables from a separate worktree (`--build-info` clean): `0dd91c0` sha256 `d8415e14…`, `d1299fb` `90743392…`, `2913b86` `03956daa…`, `0aca11e` `5b499cf9…`.
- **Method:** as on October 5, `history_scroll_wayland.py` sends wheel notches through a virtual pointer every 150 ms over a History row and reads the UI thread's CPU time per notch from `schedstat`: 20 warm-up notches, 20 idle windows, 120 recorded notches (30 down, 30 up, twice), 20 idle windows. Each notch also records the UI thread's CPU and that CPU's `scaling_cur_freq`. Statistics: pooled nearest-rank percentiles, and candidate-minus-base deltas with a two-level bootstrap (launches, then notches; 4,000 resamples, seed 1). The order was fixed before the first launch: the compact chrome cell at 1480 × 800 in BCCB CBBC BCCB CBBC (8 launches each), then narrow History at 1480 × 800 and at 461 × 490 in ABBA ABBA ABBA (6 launches each). Load average at launch start: 0.06–1.10; no builds or other agents were running.

## Results

UI-thread CPU per wheel notch, milliseconds:

| Cell | Build | Launches | p50 | p95 | p99 | Max | Launch p50s |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Compact chrome, 1480 × 800 | `0dd91c0` | 8 | 8.452 | 12.783 | 13.549 | 14.408 | 6.87–11.46 |
| | `d1299fb` | 8 | 7.384 | 12.701 | 13.193 | 13.894 | 6.20–8.41 |
| Narrow History, 1480 × 800 | `2913b86` | 6 | 8.518 | 13.249 | 13.682 | 14.212 | 6.74–11.98 |
| | `0aca11e` | 6 | 8.150 | 13.172 | 13.587 | 14.200 | 7.24–9.76 |
| Narrow History, 461 × 490 | `2913b86` | 6 | 6.090 | 8.946 | 14.621 | 15.988 | 4.82–7.73 |
| | `0aca11e` | 6 | 5.918 | 9.406 | 15.537 | 16.622 | 4.54–7.72 |

Candidate minus base, with 95% bootstrap intervals:

| Cell | Δp50 | Δp95 |
| --- | --- | --- |
| Compact chrome, 1480 × 800 | −1.068 [−2.054, −0.092] | −0.082 [−0.375, +0.198] |
| Narrow History, 1480 × 800 | −0.368 [−2.296, +1.265] | −0.077 [−0.330, +0.146] |
| Narrow History, 461 × 490 | −0.173 [−1.785, +1.312] | +0.460 [−2.210, +2.764] |

Idle windows read 0.000 ms at p50 in every cell. Notches ran on all four pinned CPUs in every cell at 702 MHz to 3.26 GHz (p50 1.19 GHz; 0.95 GHz for `2913b86` at 461 × 490), and per-launch medians spread by up to 5 ms within one build, which is why the intervals are wide.

## Reading

Neither change makes History's scrolling measurably costlier on the UI thread. The compact chrome's wide cell had first shown Δp50 +1.27 ms [+0.10, +2.23] on 2026-10-27 while worktree builds were running, with the base's launches bimodal; rerun on a quiet host and pinned to one cluster it reads −1.07 ms [−2.05, −0.09], so the first result was a confound of the busy host, and the quiet rerun gives no sign of a regression (its small negative delta is within the launch-to-launch spread and is not claimed as a speedup). The compact chrome's narrow cell from the first run stands: Δp50 −0.09 [−1.04, +0.66]. Narrow History's intervals at both sizes include zero, with p95 within 0.5 ms.

Limits: one host and one fixture; Midnight at 13 pt only; UI-thread CPU per notch, not frame presentation latency; the 461 × 490 cell's tails rest on six launches per build.
