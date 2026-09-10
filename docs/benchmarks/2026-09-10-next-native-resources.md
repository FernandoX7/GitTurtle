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

These steps were **pending at the end of Session A**. The interrupted session did not establish repeated native release of model/PDF replacement frames. [Session B below](#session-b-measured-188ec47-completed-review-and-window-cleanup) records the completed repeated cycles and image retirement, including its shorter settling intervals. RSS excludes GPU and driver allocations; resource plateaus plus source/unit-test retirement behavior are indirect evidence and cannot prove GPU allocation release. A new sustained native session must also revisit the crash, new-tab query isolation and deep graph continuity.

### Raw evidence

Final frozen copy: `/tmp/gitturtle-next-20260910/resource-analysis-preliminary-20260910T014619Z/`. The directory name retains the analysis script's preliminary naming, but this copy includes the final observer footer and crash trace. Raw files and analysis stayed outside observed repositories; retain them when archiving final milestone evidence.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `release-resources.jsonl` | 1,686,617 | `eb90f3663cb6d5c5b7ccaf0aef04b26fa356840bfa851530c895b39d7fb9148e` |
| `release-events.jsonl` | 989 | `9655bade8553837fa70c9d2954d13a92f217c7ae291ec2e93218154a62cb1419` |
| `release-trace.log` | 3,757 | `857b67ac4cb406ea6fbe389bc758341cd98bef3479abf5f350fbe54bcbfe4d37` |

Observer: `/tmp/gitturtle-next-20260910/observe-native.py`, SHA-256 `a72a1c57dd7aac48b8eaebca20ecc3951c968732765c4f87dfe6e269dfc3511d`. Analysis: `/tmp/gitturtle-next-20260910/analyze-preliminary.py`; derived `summary.json` is beside the frozen raw files. Observer footer: executable unchanged, 396 samples, zero collection errors, three imported markers, target unavailable after the abort.


## Session B: measured `188ec47`, completed review and window cleanup

The corrected release completed a coordinator-marked **21m43.822s mixed-review interval**, from `02:17:46.110641Z` to `02:39:29.933136Z`. Resource samples span **22m15.001s**. The coordinator then closed the actual final window; the trace recorded retirement of the remaining ten images and `image_window_closed retained_images=0`, followed by process disappearance. There are no panic/failure trace records, sampler errors, parser warnings or image-count inconsistencies in this session. This supplies sustained native and cleanup evidence for **this compiled source**; it does not establish that later corrections were exercised or installed.

Two further UI corrections—confirmation-dialog titles and GitHub PR-draft debounce—were pending after this run. Their eventual release identity and native checks belong in separate final packaging evidence. Session A above remains the failed earlier run.

### Build, observation and activity boundaries

| Item | Exercised value |
| --- | --- |
| Source | `188ec47199a0dc31f333fff6673d1d6afd7766f1` |
| App PID | `14206`; sole recognized GitTurtle process throughout observation |
| Packaged executable | `/private/tmp/gitturtle-next-20260910/FinalGitTurtle.app/Contents/MacOS/gitturtle` |
| Executable SHA-256, start/end verified | `f70097158c2b54b84c023ecee4c53e0916ea70791131de3d84602696a0bf5afe` |
| Packaged/raw executable UUID | `6B923D29-D3B9-3579-B79E-10939AE21C31` |
| Hardware / OS | Apple M4 Max, 16 logical CPUs, 128 GiB RAM; macOS 26.6.2, build 25G83 |
| First / last sample UTC | `02:17:46.418015Z` / `02:40:01.358136Z` |
| Observer footer UTC | `02:40:06.325624Z`, `target_unavailable`, executable unchanged |
| Sampling | 268 five-second samples; 45 fresh FD/background snapshots; 18 operator markers |

The process started at approximately `02:17:22Z`; the first sample was about 24 seconds after launch. The coordinator described a fresh process with warm filesystem caches and a restored captured PDF at Before 12 / After 20, linked at 125% zoom. The marked review includes startup, dialogs, tool/settling intervals and navigation, rather than twenty-one minutes of continuous input. The initial marker interval alone lasted 274.13 seconds and had essentially flat RSS. The coordinator reported compiler/container validation stopped before launch; no compiler appeared in the 45 fresh background snapshots. Ordinary desktop load remained: one-minute load average ranged 4.68–10.59, median 7.46.

Markers report the six-tab no-CLI startup, three PDF page 9–20 sweeps, three linked model camera cycles, captured commit/tab-switch handling, a 120,000-commit merge-heavy history window, search cancellation, eight-tab admission, alias deduplication, independent queries, missing-folder recovery and conflict-source inspection. No conflict resolution write is claimed. The deep graph was observed continuous at rows 5001–6000 while retaining the original inspected commit. These are coordinator native observations; the resource sampler does not itself inspect UI correctness.

The corrections in the action record are retained. Marker 3 initially overstated Find/Escape completion; marker 4 explicitly withdrew that claim after a stale tool state prevented the actions. The second PDF attempt initially entered invalid `920`, and the modal blocked underlying paging. Marker 14 records the explicit focus/select correction, successful page 9 entry and observed pages 10–20. That failed attempt is included in its elapsed interval and resource samples. The markers alone do not turn the withdrawn Find/Escape claim into a pass.

### Resource distribution and transient peak

| Metric | First → last | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Target RSS, MiB | 161.61 → 275.39 | 217.55 | 275.38 | **685.33** |
| Target five-second interval CPU, % of one logical CPU | — | 1.40 | 9.80 | 18.00 |
| Target threads | 14 → 16 | 16 | 21 | 23 |
| Fresh numeric FDs | 9 → 9 | 9 | 17 | 17 |
| Sampled Git descendants | 1 → 1 | 1 | 5 | 5 |
| All sampled descendants | 1 → 1 | 1 | 5 | 5 |

Target cumulative CPU increased 32.69 seconds over 1,335.001 seconds of samples, averaging 2.45% of one logical CPU. `ps`'s independently reported average CPU reached 35.3%; it is not the five-second interval metric. Sampler collection took median 64.22 ms, p95 117.64 ms and maximum 133.17 ms. The longest monotonic sample gap was 5.00544 seconds, with scheduling lag at most 8.22 ms. These show no sampler stall; they do not prove that every UI interaction was responsive.

The RSS peak is preserved, not hidden by the later plateau:

| UTC | Target RSS, MiB | Context |
| --- | ---: | --- |
| `02:34:56Z` | 257.73 | Tab workflow before the rise |
| `02:35:01Z` | 335.20 | Initial rise |
| `02:35:06Z` | 682.41 | Elevated plateau, continuing through `02:35:21Z` |
| `02:35:26Z` | **685.33** | Peak |
| `02:35:31Z` | 574.08 | First decrease |
| `02:35:41Z` | 262.66 | Returned near the preceding working level |

These samples lie between tab-workflow markers at `02:34:08.769577Z` and `02:35:51.532403Z`, trace bytes `[3847, 4086)`. The coordinator recalled Markdown companion-link loading followed by Escape/tab navigation near the peak. That supplies temporal context, not allocation attribution: there is no inner open/close marker or per-line trace timestamp to determine which action allocated or released the transient memory. The trace interval adds four registered images but records neither pixel sizes nor editor allocations.

There were three Git children throughout `02:35:01Z`–`02:35:41Z`; fresh FD counts before/during the rise were both 13. One child was replaced near the initial rise, while aggregate child RSS fell from about 133 MiB to 12.6 MiB. The target's sampled interval CPU reached 12.0% during the initial increase and 3.80% at peak RSS. The evidence does not support growing child-process accumulation as the cause of the target's rise. It also does not support a retained-memory leak claim from this transient. The 512 MiB tab allowance is a conservative retained-state admission estimate, **not an RSS cap**; it does not account for every transient allocation, allocator reserve or native framework allocation.

### Image lifetime and repeated preview cycles

The full trace contains **75 tracked images / 75 frames and 75 retired images / 75 frames** across 48 retirement batches. The registry reached at most 16 retained images and ended at zero. All frames in this exercised set were single-frame images; this run does not exercise multi-frame GIF retirement. Two initial images were registered before the first marker at byte 98, and are included in the totals.

Trace byte offsets are half-open intervals captured by the operator markers. They give ordering, not per-event wall-clock timestamps. Counts below describe the marked intervals; the action notes are required to interpret intervals whose phase labels were not updated at every step.

| Reported action interval | Trace bytes | Images tracked / retired | Retained before → after |
| --- | ---: | ---: | ---: |
| Initial PDF sweep/dialog interval | 98–1364 | 15 / 9 | 2 → 8 |
| Model cycle 1 | 1701–2655 | 12 / 12 | 10 → 10 |
| Model cycle 2, ending at deep-history marker | 2655–3233 | 8 / 8 | 10 → 10 |
| Conflict inspection plus reported model cycle 3 | 4086–4769 | 8 / 8 | 14 → 14 |
| Corrected actual PDF sweep 2 | 4769–6089 | 12 / 12 | 14 → 14 |
| Actual PDF sweep 3, while phase label still says `pdf-settled` | 6089–7409 | 12 / 12 | 14 → 14 |
| Explicit preview/tab closure | 7409–7508 | 0 / 4 | 14 → 10 |
| Projects quiet interval | 7508–7508 | 0 / 0 | 10 → 10 |
| Actual final window closure | 7508–7617 | 0 / 10 | 10 → **0** |

The two later PDF endpoint markers reported RSS 280,160 and 280,192 KiB: **273.594 and 273.625 MiB**, a 32 KiB difference at the same captured Before 12 / After 20 endpoint. Both sweeps retired as many images as they registered. The following 27.16-second marker interval contained no image-track/retire events and sampled RSS 273.594–273.625 MiB. Model replacement counts also balanced in the reported cycle intervals, but interleaved history and other views changed the retained working set; these are not equal-work isolated model RSS benchmarks. The third model settlement interval measured **24.49 seconds**, despite its next marker's approximate “about 30 seconds” wording.

After explicit closure of preview/history/operations/conflict tabs, one linked-worktree tab remained while Projects was shown. The final quiet interval was **33.903 seconds**, not a full minute. Its seven samples ranged 275.375–275.406 MiB, with 16–17 threads, one fresh FD observation of 9 and one Git child. Marker endpoint cumulative CPU increased 0.37 seconds over 33.903 seconds, averaging **1.09% of one logical CPU**. The first five-second CPU interval overlaps the preceding active cleanup; the marker endpoint calculation better isolates this quiet period.

The quiet interval produced no lifecycle events. Ten images remained legitimately referenced until final window closure. After the `02:40:03.836360Z` close-request marker, the trace logged `image_retire images=10 frames=10 retained_images=0` and then `image_window_closed retained_images=0`. These retirement records are emitted after actual GPUI `App::drop_image` calls. The coordinator's subsequent process check found neither the app nor its Git children remaining. The observer footer's `target_unavailable` is a disappearance classification, not an independently measured exit code; normal closure is established by the coordinator action and matching window-close trace, with no panic record.

This is direct native evidence that the exercised replacement and close paths invoked image retirement and emptied the registry. It does not measure completed GPU execution or GPU/driver resident bytes, and RSS need not return to launch levels while caches and the process are still alive. The originally proposed full sixty-second settled sample was not collected.

### All native callback timings

| Recorded metric | n | Median, ms | p95, ms | Maximum, ms |
| --- | ---: | ---: | ---: | ---: |
| Commit selection → changed-file callback | 9 | 30.763 | 51.545 | 51.545 |
| History file activation → preview callback | 2 | 4.232 | 5.266 | 5.266 |
| Older-history request → page callback | 11 | 7.862 | 10.243 | 10.243 |
| Working file activation → preview callback | 3 | 49.603 | **130.851** | **130.851** |

All **25 values**, including the 130.851 ms working-preview maximum, are retained in the durable trace. Small sample counts make p95 equal the maximum. The callback boundary is the same handler-to-matching-frame callback described for Session A; it excludes physical input delivery, OS presentation and completed GPU work. Superseded callbacks may be absent. There are no separately recorded worker-only durations, so no paired backend distribution or callback-minus-worker estimate is claimed. The differing actions, caches and small counts do not justify a speed comparison with Session A.

### Durable data and reproduction

The [sanitized evidence folder](2026-09-10-next-native-resources-data/188ec47/README.md) retains every resource sample as numeric CSV, exact event and trace logs, a structured summary, and file hashes. The resource projection removes private paths, executable command strings and background-process identities. The full original observer log and frozen analysis remain outside the repository at `/tmp/gitturtle-next-20260910/final-native-analysis-188ec47/`.

| Original frozen file | Bytes | SHA-256 |
| --- | ---: | --- |
| `resources.jsonl` | 1,149,193 | `0f8bb82aad00b4ec229f3c0470aab3ea44ad75eae148bd0ff1c39528ff71cb24` |
| `events.jsonl` | 11,982 | `b2027d43f2e93ba1ab27c371e57f1bc7a06002f675e90d358e750fbc70275f96` |
| `trace.log` | 7,617 | `a7ce9f1e435a948697ea0a05383f28744932dd2d9bc5a5c0340ce06275033335` |

The read-only analyzer verified PID, source and executable hash, copied stable complete files, reconciled the 268-sample footer, and reported zero malformed records, crossing trace boundaries or lifecycle-count inconsistencies. Preparation tests and the earlier failed session remain separate evidence. No application source changes, build, native interaction or process action was performed while producing this analysis.
