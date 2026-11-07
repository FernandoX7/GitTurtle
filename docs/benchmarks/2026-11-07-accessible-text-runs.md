# Accessible text runs and repository confinement, 2026-11-07

Release measurements of two changes before they land: text fields writing their text to assistive technology as AccessKit text runs (FU29-C, `d108b064` … `c3ca4338`), and every Git command of an opened repository confined to it (FU26-E2, `8b586dd7` `c0a21056`).

## Host and procedure

The Asahi development host (AArch64, 12 cores, Linux 7.1), release test executables built with `cargo test --locked --release -p gitturtle --bin gitturtle --no-run`, each from its own revision in one checkout and target, one at a time. The measurement held the build slots quiet and started only after every running build had finished and the host's CPU was under 5 % busy over 2 s; base and candidate runs alternate. Warm caches; the GPUI test platform (layout, prepaint and paint; no GPU, compositor or display link). The tables give the median of the runs' medians, the range of the runs' medians, and the median of the runs' 95th percentiles.

## Find bar frame with no screen reader

`find_bench::find_bar_frame_release_benchmark` ([procedure](2026-11-03-compare-find-frame.md)), 200 samples a series, five runs each of `ec64f91c` (before FU29-C) and `8ac34dd0` (FU29-C's tip, the same tree as the landed chain). The test window has no assistive client, so this is the path every user without a screen reader takes. All 28 series moved by −0.5 to +0.8 %, every pair of run ranges overlapping: for example unified, Find open, 13 pt, keystroke 2,572 → 2,570 µs (p95 2,616 → 2,602); split, Find open, 13 pt, keystroke 3,260 → 3,269 µs (p95 3,331 → 3,373); unified, Find closed, 13 pt, redraw 1,842 → 1,849 µs. FU29-C costs nothing measurable without a screen reader.

## One frame's accessible text with a screen reader

`native_accessibility::text_runs_bench::text_runs_release_benchmark` (its doc comment has the command), 100 samples a series, three runs each of `2236058a` (FU29-C before its last fix pass) and `8ac34dd0`. A sample is one frame's accessible-text work on the UI thread up to the return of the AT-SPI adapter's update; `off` is the same frame with no screen reader.

| Field | Case | p50, µs | p95, µs |
| --- | --- | ---: | ---: |
| 64 KB textarea | redraw / caret / keystroke | 93 / 332 / 1,393 | 100 / 342 / 1,415 |
| 5 MB, 100,000 lines | first frame | 62,243 | 62,458 |
| 5 MB, 100,000 lines | redraw | 1,242 | 1,326 |
| 5 MB, 100,000 lines | caret / across a block | 7,988 / 8,441 | 8,110 / 9,251 |
| 5 MB, 100,000 lines | keystroke / line break | 68,805 / 69,271 | 69,368 / 70,141 |
| one 5 MB line | redraw / caret | 1,373 / 8,418 | 1,472 / 8,556 |
| one 5 MB line | keystroke | 72,679 | 73,390 |
| every field | `off`, any case | 0.1 | 0.2 |

(`8ac34dd0`.) The last fix pass moved no case by more than run-to-run noise except a caret move across a block in four 256 KB lines (+2.3 %, 2,350 → 2,404 µs, ranges disjoint) and the large cases' first frame, keystroke and line break (−0.5 to −0.8 %, disjoint).

Decision: a frame drawn again copies every kept run's text because GPUI and AccessKit own each node ([gpui-base patch note](../../vendor/gpui-base/GITTURTLE-PATCH.md)); in the largest field that copy is the redraw's 1.2-1.4 ms with a screen reader and nothing without one, under a 4 ms budget for work a screen reader adds to a frame, so vendoring GPUI to keep unchanged nodes is not worth it. A keystroke, a line break or the first frame in a multi-megabyte field segments the whole text again (62-73 ms with a screen reader), which is over a frame and is queued as its own change (segment only what the edit touched); a caret move there costs about 8 ms.

## Working read and stage

`worker::working_bench::working_read_and_stage_release_benchmark` (its doc comment has the command), 60 samples, five runs each of `966dc181` (before FU26-E2) and `c0a21056` (FU26-E2 landed). Changes' read 21.42 → 20.48 ms (ranges 20.76-22.74 and 19.43-20.62, disjoint; p95 23.96 → 23.63); staging one file 5.67 → 4.39 ms and unstaging it 6.78 → 5.76 ms (ranges overlapping; p95 6.82 → 6.88 and 10.49 → 7.76). FU26-E2's confinement makes the read and the write no slower.
