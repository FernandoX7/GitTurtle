# Commit inspector parents under a long message, release measurement

Measured 2026-11-09 for the change that lets Tab reach a commit's parent links while a long message keeps the metadata row out of the list's drawn range: while the row is undrawn, a stand-in in the message's viewport tracks each parent's focus handle. The bench is `inspector_bench::commit_inspector_frame_release_benchmark`, an opt-in `#[ignore]` release test, with a third series added for the row lying below what the list draws. The [reveal measurement](2026-11-09-commit-inspector-reveal.md) of the same morning used the same bench before that series existed.

**What it times.** One `Window::refresh` on the UI thread and every frame drawn as its effects flush, until the executor parks: render, layout, prepaint and paint of History with the newest commit selected, with its metadata row painted or, in the third series, undrawn below a long message. It runs on the test platform with no GPU, compositor or display link, so a change to the inspector is the difference between builds, not a share of the whole frame.

**Method.** BASE (`1a4cf8df`, the branch before the round, with the bench commit `b6442096` picked on) against the tip (`714f5430`, whose tree the landed chain keeps), both release builds, five alternating runs each with 300 samples after 30 warm-up frames, in a 1480 × 980 window, on the Asahi development host (AArch64), with `.local/build-slots/quiet` held, the build slots clear and the host 7 % busy over 2 s before starting. Filesystem and page cache warm.

| Parents | Row | Build | Median of run p50 (µs) | Run p50s (µs) | Median of run p99 (µs) |
| --- | --- | --- | --- | --- | --- |
| 1 | drawn | base | 1,877 | 1,877, 1,877, 1,882, 1,874, 1,895 | 1,949 |
| 1 | drawn | tip | 1,887 | 1,892, 1,896, 1,885, 1,887, 1,886 | 1,933 |
| 128 | drawn | base | 11,268 | 11,250, 11,253, 11,274, 11,268, 11,325 | 12,454 |
| 128 | drawn | tip | 11,366 | 11,263, 11,345, 11,408, 11,427, 11,366 | 12,397 |
| 128 | undrawn | base | 8,131 | 8,126, 8,093, 8,131, 8,134, 8,208 | 8,862 |
| 128 | undrawn | tip | 8,532 | 8,545, 8,480, 8,518, 8,727, 8,532 | 9,218 |

**Result.** With the row drawn, a normal commit and the 128-parent bound are unchanged: the run medians overlap. With the row undrawn, the 128-parent merge costs 401 µs more per frame at the median (about 5 %), and the run medians are disjoint (base at most 8,208, tip at least 8,480): that is the stand-ins' cost, one per parent, and it buys the parents' Tab stops while the row is out of view. The case needs both a message longer than the list's view and an octopus merge at the bound the code enforces; a commit with one or two parents keeps one or two stand-ins. Accepted for keyboard reach, with no speed claim.
