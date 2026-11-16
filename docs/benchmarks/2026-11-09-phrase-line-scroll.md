# Phrase lines in the GitHub panel's body scroll, release measurement

Measured 2026-11-09 to decide whether `PhraseLine` (`crates/app/src/main.rs`) should keep its shaped lines in `PreparedPhrases` between frames. Today it keeps each run's measured width and lays its runs out again in every frame that draws it. The bench is `github_view::phrase_bench::body_scroll_release_benchmark`, an opt-in `#[ignore]` release test, written for this measurement (commit `5d8cfedc`, which lands with the next batch).

**What it times.** One `cx.update` that dispatches a wheel notch over the GitHub panel's body (`ScrollDelta::Lines(±3)`), and the draw that update flushes: the whole window's render, layout, prepaint and paint on the test platform, with no GPU, compositor or display link. The panel is open on the offline review fixture's Overview with the PR list shown, the one state whose scrolling body draws all four phrase lines (`github-page`, `github-conversations-heading`, `github-threads-page`, `github-passive-policy`), which make 26 run elements. The body scrolls to its foot and back, so every timed frame moves. The bench asserts that each timed frame drew once, moved the body and laid out all four lines.

**Method.** The head (`00d13fd7` plus the bench) against the same tree with `PhraseLine` drawn as one plain wrapping text with the same style and id (`.local/item8-2026-10-27/tools/briefs-q9/probes-PHRASE/plain-text.patch`, never committed), both release builds. Fifteen alternating runs each, 300 frames after 30 warm-up, pinned to cores 4 to 11, on the Asahi development host (AArch64), with `.local/build-slots/quiet` held, the slots clear and the host 2 % busy over 2 s before starting. UI-thread CPU time (`CLOCK_THREAD_CPUTIME_ID`) per frame.

| Surface | Build | Median of run medians (ms) | Run medians (ms), min to max | Median p99 (ms) |
| --- | --- | --- | --- | --- |
| 13 pt, 1480 × 800 | phrase lines | 6.332 | 6.317 to 6.417 | 6.480 |
| 13 pt, 1480 × 800 | plain text | 6.248 | 6.237 to 6.299 | 6.397 |
| 18 pt, 461 × 490 | phrase lines | 6.400 | 6.386 to 6.488 | 6.532 |
| 18 pt, 461 × 490 | plain text | 6.342 | 6.329 to 6.412 | 6.485 |

**Result.** At 13 pt the four phrase lines cost 84 µs more per frame than plain text, about 1.3 % of the whole frame, and the run medians are disjoint. At 18 pt in a half tile they cost 58 µs (0.9 %), and the run medians overlap. That difference covers everything the phrase lines do beyond one text each: 26 run elements laid out and painted as well as their shaping, so keeping shaped lines could save at most part of it, while adding state to invalidate on every font, size and width change. **Decision: shaped lines are not kept in `PreparedPhrases`; the item is closed.** Measure again if a scrolling surface comes to draw many more phrase lines than these four.
