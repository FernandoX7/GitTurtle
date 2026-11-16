# Commit inspector reveal, release measurement

Measured 2026-11-09 for the reveal that keeps a commit's parent links in view as Tab reaches them (the commit inspector's message list now finishes a `FocusReveal` each frame). The bench is `inspector_bench::commit_inspector_frame_release_benchmark`, an opt-in `#[ignore]` release test.

**What it times.** One `Window::refresh` on the UI thread and every frame drawn as its effects flush, until the executor parks: render, layout, prepaint and paint of History with the newest commit selected and its metadata row painted. It runs on the test platform with no GPU, compositor or display link, so a change to the inspector is the difference between builds, not a share of the whole frame.

**Method.** BASE (`c08b2b2c`, the branch before the round, with the bench picked on) against the tip (`970e9c51`), both release builds, five alternating runs each with 300 samples after 30 warm-up frames, in a 1480 × 980 window, on the Asahi development host (AArch64), with `.local/build-slots/quiet` held and the build slots clear (host under 15 % busy over 2 s before starting). Filesystem and page cache warm.

| Parents | Build | Median of run p50 (µs) | Run p50s (µs) | Median of run p99 (µs) |
| --- | --- | --- | --- | --- |
| 1 | base | 1,880 | 1,882, 1,888, 1,874, 1,877, 1,880 | 2,045 |
| 1 | tip | 1,874 | 1,884, 1,868, 1,874, 1,875, 1,870 | 1,954 |
| 128 | base | 11,221 | 11,259, 11,530, 11,144, 11,181, 11,221 | 12,586 |
| 128 | tip | 11,461 | 11,946, 11,221, 11,461, 11,964, 11,228 | 12,688 |

**Result.** A normal commit is unchanged: the run medians overlap and the tip's is no higher. A 128-parent octopus merge, the bound the code enforces, costs about 2 % of the whole frame (240 µs at the median), and the run medians overlap. No speed gain is claimed.
