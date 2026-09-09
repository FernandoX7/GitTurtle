# Preview image lifetime follow-up — 2026-09-09

The completed repeated GIF/3MF/HEIC route accumulated 10,640 KiB of RSS between the first and last of five endpoints before image retirement, versus 192 KiB after the correction. The corrected endpoints span only 208 KiB. This targeted repeat no longer shows the earlier roughly 2.6 MiB increase on each transition. It supports the correction of the identified preview-retention path; it is not a lifetime leak guarantee, a process-memory cap or a speedup measurement.

This follows the [18-minute native session](2026-09-09-milestone-native.md), which observed an unattributed increase during two short repeats. That earlier report and its raw observations remain unchanged. This follow-up distinguishes the later installed reproduction from the corrected QA bundle.

## Exact artifacts and provenance

| Identity | Before: installed reproduction | After: corrected QA bundle |
| --- | --- | --- |
| Source | `66fe451c390a9073bcc2c9759cde181db30906dd` | `92ea02c6a9ecefefc7bf43b9fffa9ac516c99d82` |
| Exercised process | PID 97791, `/Applications/GitTurtle.app` | Sole native app PID 47946, `/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app` |
| Mach-O UUID, arm64 | `0271911C-6EFF-33CB-9345-F3E6EB1CEB56` | `0C794E6C-D59D-353C-B23C-31321BDD1245` |
| Package executable SHA-256 | `75bdc628bcf29d11ff2ec1c16b6c996b775e99b5703e4e49a5752965385bc99b` | `bd1af7e3e8dbf93c952a03ea00b820cd3f10f195a54f72ddfb36e4f56d4829ad` |
| Raw release executable SHA-256 | `83da0926b3bc082583cd6be5a949709b49481b11683515cb4b1cc15e2e9881a5` | `58bd433804d663a2103f8f85a6c7a6ef211f197c008d4c2290d95fc8be06a50d` |
| Collected input map | 399 inputs; `f989828dae8369510729ed948e2d955e7e41da841ac8a4fd441690e7a15cd078` | 400 inputs; `0ed342c63d4583ed2c9a316466462f9fcb8ea5cae98846cf2a71148101706477` |

Both build manifests record identical input maps before and after their build and successful formatting, complete workspace tests, strict all-target Clippy and release build. Both resource observers verified that their exact packaged executable remained unchanged through normal stop. The before/after input-map difference is limited to six app source files: the new `image_lifetime.rs`, its main/render hooks, and the GIF/static-image, image-comparison and rich-preview consumers. This analysis did not rebuild or run the app. The corrected result identifies the QA bundle; installation of a later package is separate evidence.

Hardware and OS were the same: Apple M4 Max, Mac16,5, 16 logical CPUs, 128 GiB RAM, macOS 26.6.2 (25G83), arm64. These were separate processes at different times, with different startup histories and uncontrolled desktop activity. Absolute starting RSS and CPU totals are therefore not an equal-state performance comparison.

## Repeated route and exclusions

The coordinator repeated Quick Open of `images/moving.gif`, `models/tetra.3mf` and `images/half-red-blue.heic` in the disposable `/private/tmp/gitturtle-expanded-native-20260909` worktree, using Back between source inspections. The retained fixture is HEAD `6d2a5f953fee3b43ee45a76323fd67df9ae20e31`, tree `08f8a78d3e617f874c2b74851aa57623a5ad19cc`. State captures for each completed corrected cycle are retained under `native-image-lifetime/cycle{6..10}-{moving-gif,tetra-3mf,half-red-blue-heic}.txt` beside the runtime logs. This is operator-paced native repetition of the same working-file route, not a synthetic allocation benchmark.

- The first installed sampler accidentally observed idle duplicate PID 97692 while the actual UI was PID 97791. All 18 rows in `installed-preview-resources.jsonl` and the associated wrong-process endpoints are excluded from workload inference. The original log and `installed-cycle-invalid.json` explanation are retained. Matching executable paths alone did not establish that the observed process owned the exercised window.
- The valid before comparison uses only the five endpoint rows for actual PID 97791 in `installed-warm-endpoints.jsonl`, cycles 6–10.
- Corrected cycles 1–2 are initial warming visits. Corrected attempts 3–5 were incomplete after the native automation freshness refusal and are explicitly marked `valid_complete_cycle=false`. Their endpoint rows and their resource samples remain visible; they are excluded from the completed-cycle comparison.
- Corrected cycles 6–10 each have `valid_complete_cycle=true`. They form the five completed endpoints compared below. The intervening invalid attempts are part of that process’s prior history, so this is not an identical cold-cache protocol.
- The fixed sampler stops at 18:52:35.983 UTC. Later PDF/native smoke is outside this resource observation and is not included in these statistics.

## Five completed endpoint rows

RSS is the exact `ps` value in KiB. CPU is cumulative process CPU seconds, not wall-clock latency. UTC timestamps are derived from the endpoint recorder’s `time_unix`.

| Cycle | Before UTC | Before RSS KiB | Before CPU s | Corrected UTC | Corrected RSS KiB | Corrected CPU s |
| --- | --- | ---: | ---: | --- | ---: | ---: |
| 6 | 2026-09-09T18:34:43.386928+00:00 | 191712 | 9.31 | 2026-09-09T18:50:56.593701+00:00 | 179888 | 6.73 |
| 7 | 2026-09-09T18:34:48.745586+00:00 | 194320 | 10.21 | 2026-09-09T18:51:03.695444+00:00 | 179968 | 7.78 |
| 8 | 2026-09-09T18:34:54.030012+00:00 | 197024 | 11.00 | 2026-09-09T18:51:10.832955+00:00 | 180016 | 8.86 |
| 9 | 2026-09-09T18:34:59.200017+00:00 | 199712 | 11.80 | 2026-09-09T18:51:18.032516+00:00 | 180096 | 9.89 |
| 10 | 2026-09-09T18:35:05.098016+00:00 | 202352 | 12.65 | 2026-09-09T18:51:25.169169+00:00 | 180080 | 10.92 |

From endpoint 6 through endpoint 10, before RSS increases by **10,640 KiB (10.391 MiB)**; corrected RSS increases by **192 KiB (0.188 MiB)**. These five endpoints contain four inter-endpoint transitions, not five measured allocation deltas. Before transition deltas are `2608, 2704, 2688, 2640` KiB; corrected deltas are `80, 48, 80, -16` KiB. The corrected endpoint range is 179,888–180,096 KiB, a 208 KiB band.

The before endpoint span is 21.711 seconds and cumulative CPU rises by 3.34 seconds. The corrected span is 28.575 seconds and CPU rises by 4.19 seconds. Pacing differs and includes automation/observation work. These values establish neither a latency reduction nor lower CPU cost; no speedup is claimed.

## Identified lifetime issue and correction

Working-tree Quick Open deliberately bypasses the immutable preview cache and prepares fresh captured content on each visit ([worker routing](../../crates/app/src/worker.rs)). Each prepared `RenderImage` gets a fresh GPUI image ID. In pinned `gpui-pre` 0.3.4, `Window::paint_image` stores atlas tiles keyed by image ID and frame; dropping its CPU `Arc` does not remove those tiles. The pinned Metal atlas retains tile/texture allocations until explicit removal. Its raw `img(RenderImage)` path also bypasses the resource image-cache retirement path. App Back previously released content owners without retiring these uploaded tiles.

The [new registry](../../crates/app/src/image_lifetime.rs) holds one cleanup reference per uploaded image. A sweep removes an entry only when that is the sole remaining `Arc`; `App::drop_image` then removes every frame from every window, including the currently updated window passed explicitly. History-cache, retained-inspection, dialog, canvas and other-window owners keep shared images alive. GIF/image comparison and static model/diagram/PDF painting register through this path.

Sweeps are coalesced per window and scheduled during actual rendering. The next callback follows the scheduling draw’s frame/element cleanup; if an older callback arrives before Back draws, Back re-arms one check for its own draw. A close observer defers cleanup until the window and released entities have dropped. Sweeps do not request redraws or schedule themselves. Five lifecycle tests cover repeated registration, shared owners, all GIF frames, delayed last-frame ownership, window close and repeated opens. The native run addresses the actual repeated-route behavior; the unit tests do not simulate a Metal device.

Existing preparation and ownership limits remain relevant:

| Layer | Limit or retained state |
| --- | --- |
| Immutable CPU preview cache | 128 MiB / 32 entries per worker; counts retained source buffers and decoded/render pixels. Working-tree Quick Open does not populate it. UI-held owners can outlive cache eviction. |
| Quick Open navigation | Four nested revision inspections; Back restores the captured context. Ordinary open/Back repetition does not add a retained history entry for every visit. |
| GIF preparation | At most 120 frames, 30 seconds, 16 million output pixels, 800-pixel edge, 32 MiB compressed input, plus source/decode limits and cooperative checks. The prepared content retains all BGRA frames, first-frame RGBA, timeline and captured bytes. |
| 3MF prepared view | Four 720×720 BGRA views: 8,294,400 retained pixel bytes (7.910 MiB) per prepared side, plus captured bytes and metadata. RGBA conversion, geometry/XML/depth buffers are bounded transient decoder work. No global model decoder cache was found. |
| HEIC prepared image | Source input/dimensions and requested output are bounded; this app requests a 1600-pixel preview edge. Retained content includes both RGBA and BGRA plus captured bytes. Native ImageIO references use scoped release and source caching is disabled. |
| Uploaded-image registry | One cleanup `Arc` per unique registered image, retired after other owners release it. It is not a new total-memory budget, and legitimate retained owners can keep allocations alive. |

The observed reduction in repeat growth is consistent with retiring the identified stale atlas entries. RSS alone cannot apportion bytes among atlas backing, allocator retention, framework caches and other allocations. No claim is made that every byte of the earlier 19 MiB increase was atlas storage.

## Full observation and final idle

| Observation | Before, PID 97791 | Corrected, PID 47946 |
| --- | --- | --- |
| Header → normal stop, UTC | `2026-09-09T18:34:02.897589+00:00` → `2026-09-09T18:37:02.540754+00:00` | `2026-09-09T18:48:59.863025+00:00` → `2026-09-09T18:52:35.983226+00:00` |
| Successful samples / observer duration | 36 / 179.645 s | 44 / 216.123 s |
| Whole-observation RSS min / median / p95 / max, KiB | 188992 / 202224 / 202336 / 202352 | 142064 / 180008 / 554816 / 554896 |
| Whole-observation interval CPU median / p95 / max | 0.000% / 16.989% / 17.214% | 0.799% / 16.798% / 19.203% |
| Thread range; final | 11–16; 11 | 11–16; 11 |
| Fresh numeric FD observations | 6; all 9 | 8; all 9 |

The corrected run begins with **554,896 KiB RSS (541.891 MiB)** before the completed warm-cycle comparison; later samples fall as low as 142,064 KiB. Those large initial values are retained in the full-observation statistics and raw rows. Their cause is not established by this sampler, and the flat later endpoints must not be described as the process’s maximum or startup memory.

The Projects-idle marker is 18:51:47.830 UTC. Ten samples from 18:51:49.945 through 18:52:34.954 span 45.009 seconds. RSS narrows from 180,496 to 180,432 KiB (64 KiB range). The first interval includes the Projects transition at 5.001% CPU; the following nine interval readings are exactly 0%. Thread count settles to 11, both fresh FD observations are 9, and no sampler error occurs. This short idle observation is consistent with cleanup not creating an indefinite redraw loop; it does not establish behavior over hours or count native frame callbacks directly.

Both observers sample every 5 seconds and count numeric FDs every 30 seconds. No compiler process was detected in either valid run’s system snapshots, but ordinary desktop activity remained substantial. One-minute load ranges were 11.688–21.386 before and 11.263–26.456 after. Collection overhead median/p95/max was 73.262/126.396/128.082 ms before and 77.432/136.564/184.921 ms after. No outlier is removed. RSS is not a complete GPU/driver allocation measure; virtual size is reserved address space; short CPU/FD/thread spikes can fall between samples. The sample sets are small, sequential and not randomized.

## Raw evidence

Input copies were frozen at `2026-09-09T18:53:44.198064+00:00` under `/tmp/gitturtle-consistency-20260909/preview-lifetime-analysis-wyrtzhdz`. Full resource logs retain background/child-process observations; build files retain all collected input hashes. The invalid duplicate-process log is preserved for audit and excluded from the comparison. Relevant artifact hashes:

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `fixed-warm-resources.jsonl` | 156757 | `cd20990e30a5fe36dca1446485a812f27f2ae0f86bc39136a1b9ce2ae153a9c7` |
| `fixed-warm-endpoints.jsonl` | 1964 | `96113451cad9d17cbc7855e5d5f4bcca7c46f6f7392708f9bf5d01b0a9d68cad` |
| `fixed-warm-idle-start.json` | 33 | `b08a73a999bdc2dd9c881ccd921b76318fbd071c79a41bebe9672a656c5a86ab` |
| `installed-warm-resources.jsonl` | 133133 | `3737c0ee78d2d0271ebb42658ab2e133d662d57e91233098d4c26e7fa7804172` |
| `installed-warm-endpoints.jsonl` | 651 | `5420e8ac7ddbfe4c4c28a6206b33535b5bcb38c039dda8f693908b2511f53574` |
| `installed-preview-resources.jsonl` | 65867 | `a48c6c24d0b31ee5b1b0e03fc0391cc419315f129c57ef9ee82767220cd3c8b6` |
| `installed-cycle-invalid.json` | 293 | `01b64498a1e74f697110c82d9e0c7d63824cd76e9e433dbdc621af6f84ce3198` |
| `installed-cycle-endpoints.jsonl` | 782 | `3ea23691230bc1672a3180d9733cf9b37e149d611de483088083644cd03283ef` |
| `image-lifetime-build.json` | 99973 | `7ecd4f18c6ff7f66c54cf9592c17747bdd5b6ec388e641fbee8bf67cbd70e0a0` |
| `image-lifetime-package-identity.json` | 1037 | `61af591b202da25b956483068dc58cb01e6f3d9ba7b9d20be21cdec2dce36c41` |
| `final-build.json` | 99720 | `92d30dc40f8be978128e6f874645eb14474b5b15a77f517262807a96485b474c` |
| `final-package-identity.json` | 958 | `315cfa4e4db0ab14a4c105834aade8a4d2b9b89148bf28013d15d2a8e090e107` |
| `installed-identity.json` | 458 | `20af701dcb0adcb7e0d7ba8fd34782991c1ea100e3a1846391c6b51c09266331` |

<details>
<summary>Exact endpoint rows, including warming and incomplete attempts</summary>

Before: `installed-warm-endpoints.jsonl`

```jsonl
{"cycle": 6, "time_unix": 1788978883.386928, "ps": "97791 191712   0:09.31 /Applications/GitTurtle.app/Contents/MacOS/gitturtle"}
{"cycle": 7, "time_unix": 1788978888.745586, "ps": "97791 194320   0:10.21 /Applications/GitTurtle.app/Contents/MacOS/gitturtle"}
{"cycle": 8, "time_unix": 1788978894.030012, "ps": "97791 197024   0:11.00 /Applications/GitTurtle.app/Contents/MacOS/gitturtle"}
{"cycle": 9, "time_unix": 1788978899.200017, "ps": "97791 199712   0:11.80 /Applications/GitTurtle.app/Contents/MacOS/gitturtle"}
{"cycle": 10, "time_unix": 1788978905.098016, "ps": "97791 202352   0:12.65 /Applications/GitTurtle.app/Contents/MacOS/gitturtle"}
```

Corrected: `fixed-warm-endpoints.jsonl`

```jsonl
{"cycle": 1, "time_unix": 1788979785.66836, "ps": "47946 170320   0:03.29 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle", "valid_complete_cycle": true}
{"cycle": 2, "time_unix": 1788979797.151327, "ps": "47946 179344   0:04.34 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle", "valid_complete_cycle": true}
{"cycle": 3, "time_unix": 1788979811.375089, "ps": "47946 179488   0:05.27 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle", "valid_complete_cycle": false}
{"cycle": 4, "time_unix": 1788979811.8241038, "ps": "47946 179488   0:05.29 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle", "valid_complete_cycle": false}
{"cycle": 5, "time_unix": 1788979812.2283, "ps": "47946 179488   0:05.31 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle", "valid_complete_cycle": false}
{"cycle": 6, "valid_complete_cycle": true, "time_unix": 1788979856.5937011, "ps": "47946 179888   0:06.73 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle"}
{"cycle": 7, "valid_complete_cycle": true, "time_unix": 1788979863.6954439, "ps": "47946 179968   0:07.78 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle"}
{"cycle": 8, "valid_complete_cycle": true, "time_unix": 1788979870.832955, "ps": "47946 180016   0:08.86 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle"}
{"cycle": 9, "valid_complete_cycle": true, "time_unix": 1788979878.032516, "ps": "47946 180096   0:09.89 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle"}
{"cycle": 10, "valid_complete_cycle": true, "time_unix": 1788979885.169169, "ps": "47946 180080   0:10.92 /private/tmp/gitturtle-consistency-20260909/ImageLifetimeQA.app/Contents/MacOS/gitturtle"}
```

Excluded duplicate-process explanation:

```json
{
  "reason": "Direct CLI launch remained idle; native automation started a second process at the same installed path. Samples from PID97692 do not measure the exercised window. Excluded from workload inference.",
  "idle_pid": 97692,
  "native_pid": 97791,
  "time_unix": 1788978842.6038961
}
```

</details>

<details>
<summary>All 80 resource samples from the two valid processes</summary>

Original numeric precision is retained below. Empty interval CPU means no preceding sample. `fd_fresh=false` denotes a carried FD observation; every row’s original `errors` object was empty. The duplicate PID’s 18 excluded observations remain in its separate frozen log.

```csv
run,pid,timestamp_utc,elapsed_seconds,rss_kib,vsize_kib,cpu_time_seconds,cpu_percent_ps,cpu_percent_interval,thread_count,numeric_fds,fd_fresh,fd_sample_elapsed_seconds,direct_children,descendants,load_1m,load_5m,load_15m,collection_ms,schedule_lag_seconds,activity
before,97791,2026-09-09T18:34:03.016796+00:00,0.000588541995966807,188992,435985072,8.41,0.0,,11,9,true,0.000588541995966807,1,1,17.23291015625,16.03173828125,14.482421875,118.75333299394697,0.000588541995966807,
before,97791,2026-09-09T18:34:07.968645+00:00,5.002087375003612,188992,435985072,8.41,0.0,0.0,11,9,false,0.000588541995966807,1,1,16.4931640625,15.89794921875,14.4443359375,69.13649998023175,0.002087375003611669,
before,97791,2026-09-09T18:34:12.977482+00:00,10.004637042002287,188992,435985072,8.41,0.0,0.0,11,9,false,0.000588541995966807,1,1,15.65283203125,15.7333984375,14.39453125,75.46299998648465,0.004637042002286762,
before,97791,2026-09-09T18:34:17.966405+00:00,15.003987874981249,188992,435985072,8.41,0.0,0.0,11,9,false,0.000588541995966807,1,1,14.95947265625,15.58837890625,14.35107421875,65.07229202543385,0.003987874981248751,
before,97791,2026-09-09T18:34:22.971420+00:00,20.00403491698671,188992,435985072,8.41,0.0,0.0,11,9,false,0.000588541995966807,1,1,14.88232421875,15.5615234375,14.3486328125,70.07629101281054,0.004034916986711323,
before,97791,2026-09-09T18:34:28.001106+00:00,25.00591212499421,188992,435985072,8.41,0.0,0.0,11,9,false,0.000588541995966807,1,1,14.0107421875,15.369140625,14.28759765625,97.91833299095742,0.005912124994210899,
before,97791,2026-09-09T18:34:33.027984+00:00,30.00435033297981,188992,435985072,8.41,0.0,0.0,11,9,true,30.00435033297981,1,1,13.369140625,15.21337890625,14.23876953125,126.39554199995473,0.0043503329798113555,
before,97791,2026-09-09T18:34:37.979914+00:00,35.00518195799668,188992,435985072,8.41,0.0,0.0,11,9,false,30.00435033297981,1,1,12.69873046875,15.04345703125,14.1845703125,77.5313749909401,0.005181957996683195,
before,97791,2026-09-09T18:34:42.984717+00:00,40.001105667004595,191712,435990896,9.27,25.4,17.214033882250334,14,9,false,30.00435033297981,1,1,11.921875,14.84326171875,14.11865234375,86.44945800187998,0.0011056670045945793,
before,97791,2026-09-09T18:34:47.968989+00:00,45.00431608298095,194288,435996704,10.12,32.3,16.98909158978727,14,9,false,30.00435033297981,1,1,11.6875,14.74609375,14.08837890625,67.5432090065442,0.004316082980949432,
before,97791,2026-09-09T18:34:52.964972+00:00,50.00509512500139,197008,436001952,10.89,17.9,15.397600924373217,16,9,false,30.00435033297981,1,1,13.87451171875,15.1484375,14.23388671875,62.78283300343901,0.005095125001389533,
before,97791,2026-09-09T18:34:57.967560+00:00,55.003803500003414,199680,436003840,11.56,7.2,13.40346244943182,15,9,false,30.00435033297981,1,1,13.40380859375,15.029296875,14.197265625,66.69895799132064,0.0038035000034142286,
before,97791,2026-09-09T18:35:03.023574+00:00,60.003960000001825,202336,436007968,12.33,30.9,15.399517995091639,15,9,true,60.003960000001825,1,1,15.373046875,15.41064453125,14.33642578125,122.59483299567364,0.003960000001825392,
before,97791,2026-09-09T18:35:07.971596+00:00,65.00375591698685,202352,436007968,12.65,0.0,6.400261236921981,15,9,false,60.003960000001825,1,1,15.5830078125,15.45361328125,14.357421875,70.85491600446403,0.0037559169868472964,
before,97791,2026-09-09T18:35:12.997389+00:00,70.00515120799537,202224,436006288,12.65,0.0,0.0,12,9,false,60.003960000001825,1,1,14.6552734375,15.26318359375,14.29638671875,95.28683399548754,0.005151207995368168,
before,97791,2026-09-09T18:35:17.975272+00:00,75.00515099999029,202224,436006272,12.65,0.0,0.0,12,9,false,60.003960000001825,1,1,13.8818359375,15.0927734375,14.24169921875,73.21537501411512,0.0051509999902918935,
before,97791,2026-09-09T18:35:22.993132+00:00,80.00524929200765,202224,436006272,12.65,0.0,0.0,12,9,false,60.003960000001825,1,1,14.85205078125,15.2734375,14.310546875,91.00587497232482,0.005249292007647455,
before,97791,2026-09-09T18:35:27.960368+00:00,85.00104604198714,202288,436006832,12.65,0.0,0.0,13,9,false,60.003960000001825,1,1,14.54345703125,15.2021484375,14.291015625,62.48179101385176,0.001046041987137869,
before,97791,2026-09-09T18:35:33.030026+00:00,90.00514175000717,202224,436005712,12.65,0.0,0.0,11,9,true,90.00514175000717,1,1,13.69873046875,15.01611328125,14.23046875,128.0821249820292,0.005141750007169321,
before,97791,2026-09-09T18:35:37.968793+00:00,95.00235958298435,202224,436005712,12.65,0.0,0.0,11,9,false,90.00514175000717,1,1,12.921875,14.8330078125,14.17041015625,69.6652920159977,0.002359582984354347,
before,97791,2026-09-09T18:35:42.971054+00:00,100.00449999998091,202224,436005712,12.65,0.0,0.0,11,9,false,90.00514175000717,1,1,15.490234375,15.33349609375,14.3505859375,69.82287502614781,0.004499999980907887,
before,97791,2026-09-09T18:35:47.983636+00:00,105.00104800000554,202224,436005712,12.65,0.0,0.0,11,9,false,90.00514175000717,1,1,14.56982421875,15.14501953125,14.28955078125,85.89524999842979,0.0010480000055395067,
before,97791,2026-09-09T18:35:52.970957+00:00,110.00099541698,202224,436005712,12.65,0.0,0.0,11,9,false,90.00514175000717,1,1,14.12353515625,15.04296875,14.25830078125,73.30866600386798,0.000995416980003938,
before,97791,2026-09-09T18:35:57.968800+00:00,115.00403449998703,202224,436005712,12.65,0.0,0.0,11,9,false,90.00514175000717,1,1,13.712890625,14.9423828125,14.22705078125,68.14695801585913,0.004034499987028539,
before,97791,2026-09-09T18:36:03.022740+00:00,120.00468170800013,202224,436005712,12.65,0.0,0.0,11,9,true,120.00468170800013,1,1,13.25537109375,14.82666015625,14.1904296875,121.47570899105631,0.004681708000134677,
before,97791,2026-09-09T18:36:07.981772+00:00,125.00060779199703,202224,436005712,12.65,0.0,0.0,11,9,false,120.00468170800013,1,1,12.67431640625,14.6796875,14.14208984375,84.61966598406434,0.0006077919970266521,
before,97791,2026-09-09T18:36:12.971580+00:00,130.00097987498157,202224,436005712,12.65,0.0,0.0,11,9,false,120.00468170800013,1,1,11.9794921875,14.501953125,14.08251953125,74.09004200599156,0.0009798749815672636,
before,97791,2026-09-09T18:36:17.961120+00:00,135.0003620829957,202224,436005712,12.65,0.0,0.0,11,9,false,120.00468170800013,1,1,13.02197265625,14.67578125,14.146484375,64.2845839902293,0.0003620829957071692,
before,97791,2026-09-09T18:36:22.967675+00:00,140.00416129198857,202224,436005712,12.65,0.0,0.0,11,9,false,120.00468170800013,1,1,13.66064453125,14.78076171875,14.1865234375,67.07325001480058,0.004161291988566518,
before,97791,2026-09-09T18:36:27.967947+00:00,145.0059756669798,202224,436005712,12.65,0.0,0.0,11,9,false,120.00468170800013,1,1,16.08984375,15.265625,14.36083984375,65.57187502039596,0.005975666979793459,
before,97791,2026-09-09T18:36:33.018038+00:00,150.00397099999827,202224,436005712,12.65,0.0,0.0,11,9,true,150.00397099999827,1,1,19.76611328125,16.041015625,14.6396484375,117.70100001012906,0.003970999998273328,
before,97791,2026-09-09T18:36:37.972460+00:00,155.00468758298666,202224,436005712,12.65,0.0,0.0,11,9,false,150.00397099999827,1,1,21.38623046875,16.4384765625,14.7880859375,71.44137501018122,0.004687582986662164,
before,97791,2026-09-09T18:36:42.970241+00:00,160.00307708300534,202224,436005712,12.65,0.0,0.0,11,9,false,150.00397099999827,1,1,20.23388671875,16.28173828125,14.7421875,70.87412499822676,0.0030770830053370446,
before,97791,2026-09-09T18:36:47.966074+00:00,165.00093212499633,202224,436005712,12.65,0.0,0.0,11,9,false,150.00397099999827,1,1,18.853515625,16.06103515625,14.67333984375,68.88537498889491,0.0009321249963250011,
before,97791,2026-09-09T18:36:52.980664+00:00,170.00512037498993,202224,436005712,12.65,0.0,0.0,11,9,false,150.00397099999827,1,1,17.50390625,15.8271484375,14.5986328125,79.32962500490248,0.005120374989928678,
before,97791,2026-09-09T18:36:57.992935+00:00,175.0021886669856,202224,436005712,12.65,0.0,0.0,11,9,false,150.00397099999827,1,1,16.58251953125,15.66357421875,14.5478515625,94.56412500003353,0.0021886669856030494,
corrected,47946,2026-09-09T18:49:00.000733+00:00,0.0004964999970979989,554896,436190000,1.02,3.6,,12,9,true,0.0004964999970979989,1,1,14.8779296875,16.392578125,15.0771484375,137.45429200935178,0.0004964999970979989,
corrected,47946,2026-09-09T18:49:04.945831+00:00,5.004576125007588,554816,436189424,1.02,0.0,0.0,11,9,false,0.0004964999970979989,1,1,14.0869140625,16.203125,15.01806640625,78.52216699393466,0.004576125007588416,
corrected,47946,2026-09-09T18:49:09.937702+00:00,10.0033616249857,554816,436189424,1.02,0.0,0.0,11,9,false,0.0004964999970979989,1,1,13.43896484375,16.03369140625,14.96484375,71.66595800663345,0.003361624985700473,
corrected,47946,2026-09-09T18:49:14.953122+00:00,15.004180957999779,320224,435953856,1.02,0.0,0.0,11,9,false,0.0004964999970979989,1,1,15.96630859375,16.51416015625,15.140625,86.32129200850613,0.0041809579997789115,
corrected,47946,2026-09-09T18:49:19.931073+00:00,20.00430604198482,320224,435953856,1.02,0.0,0.0,11,9,false,0.0004964999970979989,1,1,14.767578125,16.25634765625,15.0576171875,64.20675001572818,0.004306041984818876,
corrected,47946,2026-09-09T18:49:24.936931+00:00,25.002896207995946,320224,435953856,1.04,0.0,0.40011281853018993,11,9,false,0.0004964999970979989,1,1,14.4658203125,16.1689453125,15.03369140625,71.5371250116732,0.0028962079959455878,
corrected,47946,2026-09-09T18:49:29.988210+00:00,30.00404445800814,320224,435953856,1.04,0.0,0.0,11,9,true,30.00404445800814,1,1,14.26806640625,16.099609375,15.015625,121.72170900157653,0.004044458008138463,
corrected,47946,2026-09-09T18:49:34.934819+00:00,35.004272416990716,142064,435768464,1.28,14.5,4.799781169353608,12,9,false,30.00404445800814,1,1,14.56689453125,16.130859375,15.03271484375,68.16279099439271,0.00427241699071601,
corrected,47946,2026-09-09T18:49:39.938761+00:00,40.00348437498906,161312,435774752,2.24,24.3,19.20302655829739,15,9,false,30.00404445800814,1,1,13.640625,15.91259765625,14.9619140625,72.95279201935045,0.0034843749890569597,
corrected,47946,2026-09-09T18:49:44.936787+00:00,45.00383537498419,170192,435776432,3.17,28.5,18.5986943716732,15,9,false,30.00404445800814,1,1,13.1083984375,15.76416015625,14.9150390625,70.68533301935531,0.0038353749841917306,
corrected,47946,2026-09-09T18:49:49.937377+00:00,50.003652791987406,181104,435778912,3.95,15.3,15.600569679752743,15,9,false,30.00404445800814,1,1,12.298828125,15.55224609375,14.84521484375,71.5137500083074,0.00365279198740609,
corrected,47946,2026-09-09T18:49:54.946800+00:00,55.00437783298548,178784,435774224,4.06,2.0,2.1996810282143593,13,9,false,30.00404445800814,1,1,11.6337890625,15.3603515625,14.78125,80.2717920159921,0.00437783298548311,
corrected,47946,2026-09-09T18:49:59.999324+00:00,60.00390579199302,179344,435774784,4.38,0.8,6.400604269518355,15,9,true,60.00390579199302,1,1,11.2626953125,15.22119140625,14.7353515625,133.32970801275223,0.0039057919930201024,
corrected,47946,2026-09-09T18:50:04.946471+00:00,65.00460891699186,179344,435776464,4.93,35.3,10.998453342501268,15,9,false,60.00390579199302,1,1,11.8818359375,15.28369140625,14.76025390625,79.83137501287274,0.004608916991855949,
corrected,47946,2026-09-09T18:50:09.936470+00:00,70.00499733298784,179424,435774208,5.26,0.9,6.599487330711091,14,9,false,60.00390579199302,1,1,12.29150390625,15.31201171875,14.77294921875,69.49841699679382,0.004997332987841219,
corrected,47946,2026-09-09T18:50:14.944119+00:00,75.00503749999916,179440,435774208,5.33,0.8,1.3999887533271849,14,9,false,60.00390579199302,1,1,14.35009765625,15.6884765625,14.90869140625,77.16491699102335,0.00503749999916181,
corrected,47946,2026-09-09T18:50:19.940100+00:00,80.00409437500639,179424,435774208,5.41,0.5,1.6003018569354526,16,9,false,60.00390579199302,2,2,14.88232421875,15.7763671875,14.9443359375,74.15016699815169,0.004094375006388873,
corrected,47946,2026-09-09T18:50:24.967581+00:00,85.00830591699923,179440,435774208,5.5,0.7,1.7984851208779802,14,9,false,60.00390579199302,1,1,14.8115234375,15.74658203125,14.9384765625,97.47904099640436,0.008305916999233887,
corrected,47946,2026-09-09T18:50:29.993530+00:00,90.00361691700527,179584,435774304,5.61,1.2,2.2020650966449797,14,9,true,90.00361691700527,1,1,14.826171875,15.73388671875,14.9384765625,128.17166600143537,0.003616917005274445,
corrected,47946,2026-09-09T18:50:34.930736+00:00,95.00168254200253,179488,435773184,5.66,0.7,1.0003870247307387,12,9,false,90.00361691700527,1,1,14.119140625,15.572265625,14.8857421875,67.37312499899417,0.0016825420025270432,
corrected,47946,2026-09-09T18:50:39.942204+00:00,100.0051111250068,179424,435773088,5.7,0.7,0.799451802627355,12,9,false,90.00361691700527,1,1,13.548828125,15.4296875,14.83935546875,75.47250000061467,0.005111125006806105,
corrected,47946,2026-09-09T18:50:44.938863+00:00,105.00513691699598,179392,435772528,5.72,0.0,0.3999979366515012,11,9,false,90.00361691700527,1,1,13.10400390625,15.30615234375,14.798828125,72.16166600119323,0.0051369169959798455,
corrected,47946,2026-09-09T18:50:49.951204+00:00,110.00509170800797,179408,435772528,5.73,4.9,0.200001808375885,11,9,false,90.00361691700527,1,1,12.21435546875,15.0849609375,14.7236328125,84.60741699673235,0.0050917080079670995,
corrected,47946,2026-09-09T18:50:54.937243+00:00,115.00441354198847,179824,435775888,6.48,10.3,15.002034774041414,14,9,false,90.00361691700527,1,1,13.47802734375,15.29931640625,14.80126953125,71.3813750189729,0.00441354198846966,
corrected,47946,2026-09-09T18:51:00.002014+00:00,120.00406166698667,179952,435776464,7.18,13.8,14.000985319346867,15,9,true,120.00406166698667,1,1,14.240234375,15.4267578125,14.84912109375,136.56370801618323,0.004061666986672208,
corrected,47946,2026-09-09T18:51:04.936105+00:00,125.00338124998962,179936,435776464,7.95,22.3,15.402095969577609,15,9,false,120.00406166698667,1,1,16.142578125,15.80126953125,14.984375,71.39283302240074,0.0033812499896157533,
corrected,47946,2026-09-09T18:51:09.936056+00:00,130.00383254198823,179968,435775904,8.79,25.6,16.798483795734803,14,9,false,120.00406166698667,1,1,15.810546875,15.73779296875,14.966796875,70.9497080242727,0.0038325419882312417,
corrected,47946,2026-09-09T18:51:14.943240+00:00,135.0019521669892,180080,435776464,9.48,14.1,13.8051917874988,15,9,false,120.00406166698667,1,1,15.66552734375,15.70849609375,14.9609375,80.07795800222084,0.0019521669892128557,
corrected,47946,2026-09-09T18:51:19.940043+00:00,140.00401379200048,180080,435776464,10.17,13.5,13.794312260166238,15,9,false,120.00406166698667,1,1,15.21142578125,15.61328125,14.931640625,74.88150001154281,0.0040137920004781336,
corrected,47946,2026-09-09T18:51:24.950169+00:00,145.00470083300024,180080,435774224,10.91,7.7,14.797966638041308,14,9,false,120.00406166698667,1,1,15.6748046875,15.70263671875,14.966796875,84.37054199748673,0.00470083300024271,
corrected,47946,2026-09-09T18:51:29.995073+00:00,150.00509329198394,180000,435773104,10.93,0.0,0.39996860574548715,12,9,true,150.00509329198394,1,1,16.58154296875,15.89013671875,15.037109375,128.94304102519527,0.005093291983939707,
corrected,47946,2026-09-09T18:51:34.945577+00:00,155.00485570798628,180000,435773088,10.93,0.0,0.0,12,9,false,150.00509329198394,1,1,15.89404296875,15.7587890625,14.99560546875,79.74537499831058,0.004855707986280322,
corrected,47946,2026-09-09T18:51:39.942581+00:00,160.0039628329978,180000,435773088,10.93,0.0,0.0,12,9,false,150.00509329198394,1,1,22.30859375,17.0908203125,15.47021484375,77.6995419873856,0.003962832997785881,
corrected,47946,2026-09-09T18:51:44.943667+00:00,165.004964791995,180016,435773648,10.93,0.0,0.0,13,9,false,150.00509329198394,1,1,22.36376953125,17.1884765625,15.51416015625,77.84400001401082,0.004964791995007545,
corrected,47946,2026-09-09T18:51:49.944989+00:00,170.0043017079879,180496,435774144,11.18,0.0,5.000663171954844,12,9,false,150.00509329198394,1,1,21.85400390625,17.16845703125,15.5166015625,79.91612501791678,0.004301707987906411,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:51:54.939047+00:00,175.0042614999984,180448,435773584,11.18,0.0,0.0,11,9,false,150.00509329198394,1,1,22.826171875,17.44775390625,15.62451171875,74.04891701298766,0.004261499998392537,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:51:59.988373+00:00,180.0049133329885,180448,435773584,11.18,0.0,0.0,11,9,true,180.0049133329885,1,1,24.841796875,17.95458984375,15.81396484375,122.78475001221523,0.004913332988508046,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:52:04.937432+00:00,185.0050808750093,180448,435773584,11.18,0.0,0.0,11,9,false,180.0049133329885,1,1,26.45556640625,18.4033203125,15.98486328125,71.73362499452196,0.005080875009298325,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:52:09.937172+00:00,190.00433345799684,180448,435773584,11.18,0.0,0.0,11,9,false,180.0049133329885,1,1,24.8173828125,18.197265625,15.92626953125,72.27720899390988,0.004333457996835932,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:52:14.936575+00:00,195.00453945799381,180448,435773584,11.18,0.0,0.0,11,9,false,180.0049133329885,1,1,23.31005859375,17.99462890625,15.86767578125,71.53462499263696,0.004539457993814722,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:52:19.929904+00:00,200.00141024999903,180448,435773584,11.18,0.0,0.0,11,9,false,180.0049133329885,1,1,21.84375,17.77880859375,15.8037109375,68.0507920042146,0.0014102499990258366,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:52:24.941582+00:00,205.0033862919954,180432,435773584,11.18,0.0,0.0,11,9,false,180.0049133329885,1,1,20.89501953125,17.6494140625,15.76953125,77.81512499786913,0.0033862919954117388,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:52:30.049245+00:00,210.00400233300752,180432,435773584,11.18,0.0,0.0,11,9,true,210.00400233300752,1,1,20.0224609375,17.52197265625,15.7353515625,184.92083399905823,0.004002333007520065,Projects idle after complete corrected cycles6–10
corrected,47946,2026-09-09T18:52:34.953631+00:00,215.00418687501224,180432,435773584,11.18,0.0,0.0,11,9,false,210.00400233300752,1,1,19.9404296875,17.54638671875,15.75439453125,89.202374976594,0.004186875012237579,Projects idle after complete corrected cycles6–10
```

</details>
