# Next-milestone native resource observation — 2026-09-10

## Session A: completed observation, failed stability check

The release process was sampled for **32 minutes 55 seconds** before it aborted while the coordinator opened the GitHub panel from the command palette. This session does **not** pass the sustained native stability requirement. It provides resource and callback evidence for the exact earlier release below; fixes made after that source require a new release session.

The final trace records `cannot update gitturtle::GitTurtle while it is already being updated`, followed by a non-unwinding panic in native keyboard event handling and abort. The observer independently detected that its target PID became unavailable. No observer command stopped or signalled the app.

### Identity and procedure

| Item | Exercised value |
| --- | --- |
| Source declared by build coordinator | `0a0fb5a2f42379743baedf6d6ab0c98f38c1d49a` |
| Sole app PID | `85573` |
| Executable | `/private/tmp/gitturtle-next-20260910/ReleaseGitTurtle.app/Contents/MacOS/gitturtle` |
| Executable SHA-256, verified at start and end | `ec4bebba2f0b0b9c879d68ff108368d7bde070024f9b1b14782c3b12a0b30b5b` |
| Build UUID supplied by coordinator | `3DDFFCC7-B90D-3389-98A8-22EC75FB3F55` |
| Hardware | Apple M4 Max, Mac16,5, arm64, 16 logical CPUs, 128 GiB RAM |
| OS | macOS 26.6.2, build 25G83 |
| First / last resource sample, UTC | `2026-09-10T01:12:31.151759Z` / `2026-09-10T01:45:26.127922Z` |
| Observer end, UTC | `2026-09-10T01:45:31.102581Z`; reason `target_unavailable` |
| Resource / fresh FD-background samples | 396 / 66 |

The process was already approximately 41 seconds old at observation start. This is not a cold launch measurement. Application caches and filesystem caches were not flushed. The observer verified the target's process start identity, executable path, mapped executable inode/device, stable executable hash, and absence of a second GitTurtle process. It continued checking process/file identity and duplicate app presence. The duplicate guard uses executable naming and does not prove absence of arbitrarily renamed copies.

`observe-native.py` sampled target RSS, cumulative CPU, interval CPU, threads and descendant processes every five seconds; numeric descriptors and background processes every thirty seconds. Numeric FDs exclude `cwd`/`txt` mappings. Cached FD/background values are marked and are not counted as new measurements. Percentiles below use the median and nearest-rank p95; one fully occupied logical CPU is 100% interval CPU. Process snapshots are not atomic and can miss short-lived Git commands or resource peaks between samples.

The main repository and three 120,000-commit history fixtures were declared at observer start. Additional disposable Markdown, PDF, model and operation fixtures were selected by the coordinator during the native session. The observer did not identify the active repository from UI state. Logs were written outside observed repositories.

This was a loaded desktop: one-minute load average was 3.05–10.34 (median 5.27), the Virtualization framework VM process reached a reported 593% CPU, and a compiler appeared in six of 66 fresh background snapshots. The trace is not an isolated performance benchmark. Observer command collection took median 71.67 ms, p95 124.98 ms, maximum 192.68 ms; scheduling lag was at most 7.58 ms. There were zero sample collection errors.

### Resources

| Metric | First → last | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Target RSS, MiB | 365.69 → 269.23 | 251.30 | 266.14 | 365.69 |
| Target interval CPU, % of one logical CPU | — | 0.80 | 6.60 | 11.80 |
| Target threads | 13 → 19 | 20 | 22 | 23 |
| Fresh numeric FDs | 9 → 13 | 15 | 15 | 16 |
| Sampled Git descendants | 1 → 3 | 4 | 4 | 4 |
| All sampled descendants | 1 → 3 | 4 | 4 | 5 |

Cumulative target CPU increased from 0.89 to 37.93 seconds, a delta of 37.04 seconds across 1,975.004 seconds of samples (1.88% of one CPU on average). The independent `ps` CPU statistic is an OS-reported average, not the interval statistic above; its maximum was 21.3%.

The high first RSS sample fell by 200.06 MiB at the ten-second sample. From the resulting minimum of 165.63 MiB, later mixed activity increased RSS to 269.23 MiB. The largest later five-second rise was 16.16 MiB at `01:22:31Z`. The workload changes and retained previews prevent attributing these increases to a leak or one subsystem.

| Elapsed observation window | RSS range, MiB | Interval CPU median / p95 | Sampled Git children |
| --- | ---: | ---: | ---: |
| 0–5 min | 165.63–365.69 | 0.80% / 3.20% | 1–2 |
| 5–10 min | 198.52–205.89 | 2.40% / 4.20% | 1–2 |
| 10–15 min | 221.84–250.17 | 0.80% / 6.20% | 1–4 |
| 15–20 min | 250.05–252.97 | 1.00% / 9.01% | 4 |
| 20–25 min | 252.88–252.94 | 0.60% / 0.80% | 4 |
| 25–30 min | 252.88–258.89 | 2.20% / 6.60% | 4 |
| 30–32m55s | 258.75–269.23 | 0.80% / 4.40% | 3–4 |

There is a clear five-minute RSS plateau at 20–25 minutes and later growth with further actions. The final 55-second sample window ranged from 268.80 to 269.23 MiB, with median interval CPU 0.80%, 18–19 threads, 13 fresh FDs and three Git children. These plateaus establish bounded sampled behavior over those intervals, not successful repeated cleanup or a process-memory cap. At the last sample the three Git children each reported 0.0% `ps` CPU. A passive process check at `01:47:18Z` found none of their recorded PIDs (`27445`, `31831`, `85840`) remaining after the app abort.

### Native handler-to-callback timings

| Recorded metric | n | Median, ms | p95, ms | Maximum, ms |
| --- | ---: | ---: | ---: | ---: |
| Commit selection → changed-file callback | 14 | 16.533 | 37.966 | 37.966 |
| History file activation → preview callback | 4 | 12.874 | **33,375.705** | **33,375.705** |
| Working file activation → preview callback | 2 | 26.292 | 34.843 | 34.843 |
| Older-history request → page callback | 11 | 16.080 | 17.517 | 17.517 |

All 31 recorded values, including the 33.376-second preview outlier, are included. With these small sample counts, p95 often equals the maximum. The coordinator reported inactive-app/tool timing around the long callback; the trace itself has no per-record timestamps, request identity, focus state or cache state, so it cannot determine how much was decoding, queuing, inactive-window callback delay, or another cause. Do not label it a proven 33-second UI hang or remove it from the distribution.

The [trace implementation](../../crates/app/src/main.rs) starts an `Instant` in the interaction handler and emits after prepared content has been received and a matching generation/mode reaches `on_next_frame`. These values include intervening worker/scheduling work when present. They exclude pre-handler input delivery, OS presentation and completed GPU execution; they are not measured physical input-to-visible-display latencies. Generation/mode checks can suppress superseded callbacks. The log cannot count missing or cancelled interactions.

There are **no worker-only duration records** in this trace. Worker durations appear separately in the status UI but cannot be reconstructed from these callback lines or subtracted without paired records. The [large-history release benchmark](2026-09-10-history-pagination.md) has independent backend measurements; those samples are not part of this native distribution. The deep-history graph-edge defect observed in this release is also independent of its short page callback times.

### Activity evidence and limits

The append-only marker file contains three coordinator assertions:

- `01:14:12Z`: Markdown linked scroll, captured image versions, source keyboard navigation, and an explicit remaining spoken-VoiceOver/manual-navigation check.
- `01:20:01Z`: PDF page 20, extracted text copy, rejected invalid page, before/after page positions, zoom and tab restoration; a focused-input Escape defect remained.
- `01:26:00Z`: STEP orbit/pan/zoom, independent view and edge controls, mapped geometry counts, cancellation/Back behavior, tabs/workspace actions and preserved commit draft with a disposable tracked-file commit. Busy-switch overlap still needed checking.

The coordinator subsequently reported deep graph edges disappearing after moving beyond the retained window, a new-tab search query leak, and the GitHub-panel crash that ended the run. The new graph/query/UI fixes were not in source `0a0fb5a` and are not validated by this session. The markers have no start/end phase fields; all observer sample phases are null. The 32m55s resource span is established independently, but the three markers alone do not prove twenty minutes of continuously active mixed review. Consult the coordinator's native action evidence for that sequence. Successful automated checks remain separate evidence from this failed native stability run.

### Bounded cleanup follow-up for the next release

Inspection of the exercised resource implementation explains what a useful native cleanup cycle must test:

- [Model rendering](../../crates/app/src/model_view.rs) has one global render lane and one replaceable pending pair. Each document retains its scene and current raster frame, with a reservation for current/replacement frames. Camera changes replace that frame; hiding pauses work through cancellation generations. The global lane can keep an idle thread after first use.
- [PDF navigation](../../crates/app/src/pdf_view.rs) holds at most four pages and 16 MiB per side, including page text and pixels. Navigating beyond four distinct pages exercises eviction, not merely cache hits.
- [Image retirement](../../crates/app/src/image_lifetime.rs) retains one cleanup reference per image. After a real draw, including Back/Projects, it calls `drop_image` only when ordinary cache/view/frame/dialog owners have released their references. Window closure has a deferred cleanup path. The registry does not create an idle polling loop.
- [The worker cache](../../crates/app/src/worker.rs) intentionally retains up to 128 MiB / 32 immutable previews across repository changes. Warm tabs and retained inspections also own content. Closing a tab therefore does not imply a return to cold RSS. Shared `GitRepository` clones can retain idle object batch readers; releasing ordinary history closes its traversal, not every shared repository object reader.

For the next build, record markers before/after three identical short orbit/pan/zoom/fit cycles on the same model pair, allowing 30–60 seconds to settle after each. Then repeat a fixed PDF sweep across more than four pages on each side three times, again with settled intervals. Close the model/PDF tabs and any retained source dialogs, show Projects so a real image-free frame runs cleanup, and collect sixty seconds of settled samples. Compare successive post-warm plateaus, idle CPU, threads, FDs and Git children. Preserve a completed preview while testing a replaced frame so live-image correctness is exercised alongside retirement.

These steps remain **pending** for a corrected final candidate. The interrupted session did not establish repeated native release of model/PDF replacement frames. RSS excludes GPU and driver allocations; resource plateaus plus source/unit-test retirement behavior are indirect evidence and cannot prove GPU allocation release. A new sustained native session must also revisit the crash, new-tab query isolation and deep graph continuity.

### Raw evidence

Final frozen copy: `/tmp/gitturtle-next-20260910/resource-analysis-preliminary-20260910T014619Z/`. The directory name retains the analysis script's preliminary naming, but this copy includes the final observer footer and crash trace. Raw files and analysis stayed outside observed repositories; retain them when archiving final milestone evidence.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `release-resources.jsonl` | 1,686,617 | `eb90f3663cb6d5c5b7ccaf0aef04b26fa356840bfa851530c895b39d7fb9148e` |
| `release-events.jsonl` | 989 | `9655bade8553837fa70c9d2954d13a92f217c7ae291ec2e93218154a62cb1419` |
| `release-trace.log` | 3,757 | `857b67ac4cb406ea6fbe389bc758341cd98bef3479abf5f350fbe54bcbfe4d37` |

Observer: `/tmp/gitturtle-next-20260910/observe-native.py`, SHA-256 `a72a1c57dd7aac48b8eaebca20ecc3951c968732765c4f87dfe6e269dfc3511d`. Analysis: `/tmp/gitturtle-next-20260910/analyze-preliminary.py`; derived `summary.json` is beside the frozen raw files. Observer footer: executable unchanged, 396 samples, zero collection errors, three imported markers, target unavailable after the abort.
