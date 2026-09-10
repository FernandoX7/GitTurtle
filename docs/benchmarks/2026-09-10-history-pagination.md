# Incremental ordinary history, September 10, 2026

Ordinary history now keeps one captured topological Git stream and appends only new graph rows. Sequential Older actions no longer reread a growing prefix. The app retains a maximum of 5,000 visible rows / 64 MiB of commit and graph metadata, plus one selected commit outside the window. Previous/Newest can restore older captured windows by streaming and discarding bounded pages; this backward restore remains linear in depth.

The user-reported wide empty graph column came from using the maximum lane count across all loaded rows as a table minimum, even when those lanes were far below the visible rows. The automatic graph viewport is now limited to 25% of the history width (112–280 points), preserves deliberate wider settings, and uses shared horizontal lane navigation with original spacing. Parent/child lane colors and topology remain stable across page boundaries.

## Release backend comparison

Hardware: Apple M4 Max, 128 GiB RAM; macOS-26.6.2-arm64-arm-64bit-Mach-O; git version 2.50.1 (Apple Git-155); rustc 1.98.0 (88d9e12ae 2026-08-18). Source base `74d9ca11191a9fc44d55f7da25ee44df83d554af` plus the working source hashes in [raw provenance](2026-09-10-history-pagination-data/summary.json). The isolated release benchmark executable SHA-256 was `30472082c490063d86a6d53854510ea4772900a21749367e6b2d6b111808f8fb`. This identifies the backend harness, not the installed application.

Each synthetic fixture contains changing text trees, 17 authors and varied commit subjects/descriptions. Linear has 120,000 commits. Many-refs has 100,000 mainline commits, 1,000 independent 20-commit topic branches and 8,000 additional refs: 120,000 commits / 9,001 refs. Merge-heavy has 40,000 two-parent merges: 120,001 commits / 21 refs. No fixture commit-graph was generated.

The comparison visits the first 100,000 commits in 200 page steps, repeated three times with alternating algorithm order. One complete-prefix warmup precedes each fixture; the OS cache is warm and was not flushed. Each pass creates a new repository handle. Every returned 500-row segment is checked against the same SHA-256 of ordered commit IDs outside timing. Baseline calls the original `history(offset + 500)` API; at depths beyond the former native 10,000-row ceiling this measures the same algorithm extended to that depth, not an old-app interaction. Final calls the new captured traversal and returns only the next 500 rows.

Background load was uncontrolled while other milestone work continued. Raw load-average snapshots bracket every fixture, along with sample times and all outliers. There are 597 subsequent-page samples per algorithm/fixture; initial and terminal page summaries each have only three samples, so their p95 is the maximum.

| Fixture | Initial 500 p50, before → after | Subsequent page p50, before → after | Subsequent page p95, before → after | Total page work through 100k p50, before → after |
| --- | --- | --- | --- | --- |
| linear | 414.57 → 429.33 ms | 489.854 → 0.525 ms | 564.620 → 0.568 ms | 97.116 → 0.530 s |
| many-refs | 550.57 → 578.51 ms | 638.499 → 0.522 ms | 721.409 → 0.633 ms | 127.142 → 0.690 s |
| merge-heavy | 428.48 → 440.09 ms | 512.466 → 0.518 ms | 585.434 → 0.573 ms | 102.576 → 0.543 s |

Initial loading did not improve: the new path additionally captures current tips and still pays Git’s initial topological walk. The measured improvement applies to subsequent page work. The largest returned prefix retained 41.95–43.28 MB of commit metadata; final pages retained 0.210–0.217 MB each. These are owned metadata counts, not application/Git RSS or a GPU-memory cap. Git’s internal revision-walk memory remains independent of these application bounds.

| Fixture | Cancellation p50 / p95 / max | Changed-file selection p50 / p95 / max |
| --- | --- | --- |
| linear | 7.657 / 7.714 / 7.753 ms | 10.968 / 11.526 / 21.765 ms |
| many-refs | 6.605 / 6.668 / 6.682 ms | 10.753 / 12.011 / 23.033 ms |
| merge-heavy | 7.625 / 7.669 / 7.684 ms | 11.201 / 11.338 / 22.109 ms |

Cancellation has 20 attempts per fixture, signalling after 2 ms during an initial page and including process termination/reaping. All 60 attempts returned cancellation. Changed-file selection has 40 sequential, spaced commit targets per fixture; it measures core changed-file reads only and has no comparable before series. It establishes neither rapid native input performance nor displayed file-list latency.

## Graph preparation and scrolling model

The graph-only harness feeds the same 100,000 ordered commit/parent identities to an optimized extracted graph engine, with parsing outside timing and five passes. Prefix mode repeats bounded preflight/layout over the prefix; incremental mode carries only the lane frontier and color counter between 500-row pages. Both apply the 128-lane / 200,000-edge allowance; wide graphs honestly use node-only fallback. This excludes Git, GPUI, output destruction and native frames.

| Fixture | Subsequent graph page p50, prefix → incremental | p95, prefix → incremental |
| --- | --- | --- |
| linear | 6.893 → 0.070 ms | 13.163 → 0.077 ms |
| many-refs | 0.312 → 0.002 ms | 0.638 → 0.002 ms |
| merge-heavy | 8.912 → 0.091 ms | 16.752 → 0.101 ms |

The existing whole-layout harness confirms the underlying algorithm has comparable cost: a 20,000-row linear layout p50 was 1.623 ms before and 1.645 ms after; a 21-lane / 8,001-row layout was 4.241 and 4.305 ms. The gain comes from avoiding prefix replay, not a claim that each geometry row became faster. Sixty visible `Arc` row clones cost roughly 0.0001 ms per simulated frame; this is a cloning microbenchmark and excludes element construction, GPU execution and OS presentation. Raw samples are in [graph baseline data](2026-09-10-history-pagination-data/graph-baseline.json) and the fixture graph CSV/provenance files.

## Regression coverage and native boundary

- Core history fixtures: 14 passed, including pinned continuation after refs move, one-row merge pages, deferred metadata at the byte limit, and invalidated cancellation cursors. Five core history-process unit tests passed, including active descendant termination and dropping an idle backpressured producer.
- Native-model tests: 11 graph tests, four column tests, one history-window test and one worker continuation/restore test passed. The worker fixture compares exact commit/graph rows before and after a ref change and backward restore.
- Native app evidence belongs to the coordinator’s final current-build record. `history_page_frame_ms` now traces a page handler to a next-frame callback; it excludes OS presentation. These backend and algorithm measurements do not by themselves establish scroll smoothness, input latency, actual displayed results, GPU cleanup or Linux native behavior.

## Reproduction

Build the release `gitturtle-core` example `history_pagination_bench`, then run `scripts/bench-history-pagination.py --fixtures <disposable-directory> --output <separate-results-directory> --executable <release-example> --samples 3`. The script refuses results inside fixtures. Reuse the generated fixture with `--reuse`; benchmark passes are passive reads. Run `scripts/bench-history-graph.py --fixture <generated-fixture> --output <separate-graph.csv>` for each topology. `scripts/bench-graph-layout.py --baseline-ref 74d9ca11191a9fc44d55f7da25ee44df83d554af --output <separate-graph.json>` retains the earlier whole-layout comparison.

Raw backend [linear](2026-09-10-history-pagination-data/linear.csv), [many-ref](2026-09-10-history-pagination-data/many-refs.csv), [merge-heavy](2026-09-10-history-pagination-data/merge-heavy.csv) samples and [hardware/source/load provenance](2026-09-10-history-pagination-data/summary.json) are versioned together.
