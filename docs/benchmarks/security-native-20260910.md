# Security milestone native resource session — 2026-09-10

Status: **final**. One release native mixed session, PID 99696; the separate startup/duplicate attempt is excluded.

Source `b4440f132999ebc45a81b1baa31eda4594bddd66`; executable UUID `2475FAA0-21B2-398D-9F9C-A2D57C92543A`; SHA-256 `3ce5c6e541c1c7d459982c63ea497705511680f837ec78fb32ea5a8e1e21d020`.

Mixed markers: 2026-09-10T08:56:44.175944+00:00 → 2026-09-10T09:16:59.454686+00:00, 1215.279 seconds. Numerical sampling: 244 mixed samples; sample span 1214.968 seconds. Observer stop reason: `target_unavailable`.

Apple M4 Max, 16 logical CPUs, 128 GiB RAM, macOS 26.6.2 (25G83), arm64. Disposable repositories and an offline review provider; unrelated background applications remained active.

Mixed RSS: first 114.094 MiB, last 241.172 MiB, median 230.461 MiB, maximum 550.672 MiB. CPU consumed between first and last samples: 39.370 seconds. Peak sample: 2026-09-10T08:57:04.255083+00:00 in `mixed-start`.

| Measurement | n | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Process CPU, % of one core | 243 | 2.600 | 6.601 | 18.007 |
| Threads | 244 | 21.000 | 24.000 | 27.000 |
| Numeric FDs, fresh snapshots | 41 | 27.000 | 39.000 | 39.000 |
| Owned descendants | 244 | 5.000 | 8.000 | 11.000 |
| Owned descendant RSS, MiB | 244 | 20.891 | 140.203 | 147.844 |
| Sampler collection, ms | 244 | 62.958 | 117.747 | 126.780 |
| Sampler scheduling lag, ms | 244 | 4.310 | 5.541 | 7.964 |
| System load, fresh 1-minute readings | 41 | 7.445 | 10.096 | 11.569 |

## Marker phases

Phase duration comes from operator markers. RSS endpoints and CPU deltas use samples inside each phase; boundary tails are not reconstructed. FD values below use fresh snapshots only.

| Phase | Marker duration, s | Samples | RSS first → last / max, MiB | CPU Δ, s | CPU p95, % | Threads max | FDs max | Children max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| mixed-start | 36.425 | 8 | 114.094 → 411.797 / 550.672 | 0.910 | 12.589 | 14 | 11 | 1 |
| markdown-diagrams | 36.097 | 7 | 413.828 → 140.938 / 413.828 | 0.740 | 3.601 | 17 | 11 | 3 |
| pdf | 36.363 | 7 | 150.188 → 168.312 / 168.375 | 0.650 | 3.202 | 16 | 15 | 2 |
| model | 31.826 | 7 | 168.469 → 195.141 / 195.141 | 0.710 | 6.602 | 18 | 19 | 3 |
| history120k | 69.395 | 13 | 196.578 → 197.469 / 197.484 | 1.490 | 5.603 | 23 | 27 | 6 |
| manyrefs | 32.271 | 7 | 197.453 → 207.609 / 207.609 | 1.100 | 4.602 | 23 | 31 | 6 |
| replacement-before | 33.089 | 7 | 207.641 → 207.688 / 207.703 | 0.440 | 3.402 | 21 | 31 | 6 |
| replacement-verified | 28.639 | 5 | 207.812 → 207.984 / 207.984 | 0.560 | 3.401 | 21 | 31 | 6 |
| adversarial | 87.980 | 18 | 208.266 → 222.125 / 222.125 | 2.320 | 9.595 | 23 | 35 | 7 |
| large-status | 35.813 | 7 | 222.266 → 222.656 / 222.703 | 0.650 | 3.000 | 23 | 35 | 7 |
| staged-draft | 49.739 | 10 | 222.828 → 224.062 / 224.062 | 1.830 | 6.398 | 27 | 39 | 11 |
| slow-fetch | 42.351 | 8 | 224.031 → 224.438 / 224.438 | 1.890 | 8.000 | 24 | 39 | 8 |
| offline-review | 57.954 | 12 | 224.891 → 225.781 / 225.797 | 2.900 | 7.403 | 24 | 39 | 8 |
| draft-navigation | 49.674 | 10 | 225.828 → 230.219 / 230.219 | 1.670 | 5.803 | 26 | 31 | 8 |
| linked-worktree | 18.901 | 4 | 230.219 → 230.234 / 230.234 | 0.300 | 2.800 | 26 | 35 | 7 |
| warm-cycle-1 | 45.282 | 9 | 230.250 → 230.688 / 230.688 | 1.490 | 15.198 | 24 | 19 | 5 |
| markdown-warm-repeat | 28.058 | 5 | 234.047 → 235.328 / 235.375 | 0.420 | 2.998 | 23 | — | 4 |
| markdown-settled | 40.980 | 9 | 235.328 → 235.375 / 235.375 | 0.780 | 2.600 | 22 | 23 | 4 |
| markdown-10cycles | 26.849 | 5 | 235.391 → 235.391 / 235.391 | 0.900 | 6.601 | 21 | 23 | 4 |
| settings | 64.814 | 13 | 236.125 → 237.625 / 237.625 | 0.780 | 4.400 | 21 | 23 | 4 |
| braden-compact | 107.795 | 22 | 238.188 → 239.641 / 239.656 | 3.280 | 4.200 | 22 | 27 | 5 |
| explicit-commit | 77.071 | 15 | 239.594 → 239.672 / 239.688 | 1.580 | 3.600 | 23 | 27 | 5 |
| warm-cycle-2 | 66.956 | 13 | 239.781 → 241.203 / 241.203 | 4.600 | 18.007 | 22 | 27 | 5 |
| settled-before-quit | 110.957 | 23 | 241.203 → 241.172 / 241.250 | 3.020 | 3.000 | 21 | 19 | 3 |

## Recorded callbacks

These are interaction-handler start to eligible next-frame callback measurements, including worker/scheduling delay. They exclude pre-handler input delivery and do not establish input-to-visible or GPU completion time. All outliers are retained. Nearest-rank p95 is used; small heterogeneous samples are not tail-latency benchmarks.

| Metric | n | Median, ms | p95, ms | Maximum, ms |
| --- | ---: | ---: | ---: | ---: |
| commit_files_frame_ms | 15 | 30.851 | 40.069 | 40.069 |
| file_preview_frame_ms | 34 | 7.911 | 261.551 | 829.004 |
| history_page_frame_ms | 1 | 5.441 | 5.441 | 5.441 |
| working_preview_frame_ms | 2 | 19.257 | 20.861 | 20.861 |

During the recorded ten alternating Markdown document cycles, 5 RSS samples ranged 235.391–235.391 MiB, with endpoints 235.391 → 235.391 MiB. The 20 file-preview callbacks ranged 2.063–133.323 ms (median 62.410 ms, p95 132.629 ms). This preserves the repeated preparation cost and shows only coarse sampled stability for these captured documents.

| Trace line | Marker phase | Metric | Milliseconds |
| ---: | --- | --- | ---: |
| 2 | before-mixed-start | commit_files_frame_ms | 32.307 |
| 3 | mixed-start | file_preview_frame_ms | 829.004 |
| 8 | markdown-diagrams | file_preview_frame_ms | 5.865 |
| 10 | markdown-diagrams | commit_files_frame_ms | 28.352 |
| 11 | markdown-diagrams | commit_files_frame_ms | 40.069 |
| 12 | markdown-diagrams | file_preview_frame_ms | 6.626 |
| 19 | pdf | commit_files_frame_ms | 36.417 |
| 20 | model | file_preview_frame_ms | 261.551 |
| 32 | history120k | commit_files_frame_ms | 33.822 |
| 33 | history120k | history_page_frame_ms | 5.441 |
| 34 | history120k | commit_files_frame_ms | 17.491 |
| 35 | history120k | file_preview_frame_ms | 1.558 |
| 36 | manyrefs | commit_files_frame_ms | 29.763 |
| 37 | manyrefs | commit_files_frame_ms | 17.545 |
| 38 | manyrefs | commit_files_frame_ms | 30.851 |
| 39 | manyrefs | file_preview_frame_ms | 8.053 |
| 40 | replacement-before | commit_files_frame_ms | 32.954 |
| 41 | replacement-before | file_preview_frame_ms | 3.068 |
| 42 | replacement-verified | commit_files_frame_ms | 17.735 |
| 43 | replacement-verified | commit_files_frame_ms | 35.383 |
| 44 | adversarial | file_preview_frame_ms | 17.765 |
| 45 | adversarial | commit_files_frame_ms | 24.786 |
| 46 | adversarial | file_preview_frame_ms | 7.184 |
| 47 | adversarial | file_preview_frame_ms | 3.901 |
| 48 | large-status | working_preview_frame_ms | 20.861 |
| 49 | large-status | working_preview_frame_ms | 17.652 |
| 50 | staged-draft | commit_files_frame_ms | 35.235 |
| 52 | draft-navigation | commit_files_frame_ms | 34.182 |
| 53 | draft-navigation | file_preview_frame_ms | 3.592 |
| 54 | warm-cycle-1 | file_preview_frame_ms | 135.050 |
| 59 | markdown-settled | file_preview_frame_ms | 4.045 |
| 61 | markdown-10cycles | file_preview_frame_ms | 133.323 |
| 64 | markdown-10cycles | file_preview_frame_ms | 7.787 |
| 66 | markdown-10cycles | file_preview_frame_ms | 116.837 |
| 69 | markdown-10cycles | file_preview_frame_ms | 8.034 |
| 71 | markdown-10cycles | file_preview_frame_ms | 121.414 |
| 74 | markdown-10cycles | file_preview_frame_ms | 2.196 |
| 76 | markdown-10cycles | file_preview_frame_ms | 124.826 |
| 79 | markdown-10cycles | file_preview_frame_ms | 4.602 |
| 81 | markdown-10cycles | file_preview_frame_ms | 116.785 |
| 84 | markdown-10cycles | file_preview_frame_ms | 6.943 |
| 86 | markdown-10cycles | file_preview_frame_ms | 124.851 |
| 89 | markdown-10cycles | file_preview_frame_ms | 7.787 |
| 91 | markdown-10cycles | file_preview_frame_ms | 132.629 |
| 94 | markdown-10cycles | file_preview_frame_ms | 2.063 |
| 96 | markdown-10cycles | file_preview_frame_ms | 124.982 |
| 99 | markdown-10cycles | file_preview_frame_ms | 2.856 |
| 101 | markdown-10cycles | file_preview_frame_ms | 124.364 |
| 104 | markdown-10cycles | file_preview_frame_ms | 5.253 |
| 106 | markdown-10cycles | file_preview_frame_ms | 120.499 |
| 109 | markdown-10cycles | file_preview_frame_ms | 4.894 |
| 114 | braden-compact | file_preview_frame_ms | 125.132 |
| 117 | braden-compact | commit_files_frame_ms | 30.450 |

## Separate warm VM snapshot

At 2026-09-10T09:13:02.336155+00:00, vmmap reported physical footprint 487.4M and process-lifetime peak 805.1M. These are vmmap display units and accounting, separate from ps RSS; no matched baseline or allocation attribution is claimed.

| Region | Virtual | Resident | Dirty |
| --- | ---: | ---: | ---: |
| MALLOC_SMALL | 127.1M | 90.8M | 90.4M |
| MALLOC_LARGE (empty) | 4064K | 4064K | 4064K |
| IOAccelerator (graphics) | 46.3M | 43.5M | 43.5M |
| IOSurface | 67.9M | 67.6M | 67.6M |
| owned unmapped (graphics) | 262.0M | 258.0M | 258.0M |

Malloc-zone summary: 151341 allocations, 65.5M allocated bytes, 97.3M resident and 30.0M fragmentation as reported by vmmap. This snapshot does not identify individual allocation sites or retained GPU owners.

## Shutdown and fixture integrity

The operator ended the mixed session with normal Quit. The observer then reported `target_unavailable`. At 2026-09-10T09:17:22.549958+00:00, the independent cleanup record found 0 GitTurtle processes and 0 remaining PIDs among 26 descendants previously observed during the session. Sampling can miss short-lived or unobserved descendants.

Before Quit, all 7 recorded passive fixtures retained the same HEAD and status: markdown, pdf, model, history-120k, many-refs, everyday, linked. Genuine held application state was unchanged: True. These are the specific recorded checks; restoration and final installed identity are verified separately.

## Cleanup and evidence limits

Image trace: 44 track records / 44 frames; 15 retire records / 34 images / 34 frames; maximum 12 retained images. Last lifecycle record retained 10 images at trace line 116; no later zero-retention trace is present. Process-exit cleanup is separate from proving each image or GPU allocation retired before Quit.

Observer quality: 0 samples with errors; 0 truncated descendant lists; all process-start identities match: True; sole target application in every sample: True; final executable unchanged: True.

Fresh background category maxima: `{"compilers": 0, "credential_helpers": 0, "docker_named_processes": 10, "git": 9, "quick_look": 9, "signing": 0, "ssh": 2}`. Remaining system activity prevents an uncontended timing comparison.

Slow local fetch cancellation record: refs unchanged True, index unchanged True, named slow helpers remaining 0. Other retained Git readers are counted separately; this record alone does not establish application shutdown cleanup.

Explicit disposable native commit record: `{"changed_path_count": 1, "head": "73a9ccbcc0227dacc91f487c3bc0c8820ddac10f", "message_sha256": "41dd2af7f39647704cd9cf5a5117a0a747829cccbe9413c42c5cbed05962ec9d", "only_expected_fixture_file": true, "staged_paths_after": 0, "unicode_qa_markers_present": true, "untracked_count": 1499}`. The coordinator owns the corresponding interaction and Git-state verification.

The [numerical report](security-native-20260910.json) preserves every sanitized resource sample, every callback and lifecycle record, phase metrics, input hashes and the complete interpretation limits. Private raw paths, accessibility text and marker notes are omitted.

- Five-second ps/lsof samples miss shorter resource spikes and child processes; snapshots are not atomic.
- RSS excludes GPU/driver allocations and is not live heap; no leak or allocation-site claim follows from an RSS peak.
- FD and background readings are fresh every thirty seconds; carried values remain in the numeric projection but are excluded from those summary distributions.
- CPU interval uses successive cumulative process CPU deltas; one core is 100%. The first sample of a phase can straddle the previous phase.
- Per-phase CPU seconds are last minus first sample; unobserved boundary tails are excluded.
- Callbacks measure handler interaction start through an eligible next-frame callback, including worker/scheduling delay; not input delivery, GPU completion, or input-to-visible latency.
- Trace phase assignment uses operator byte-offset markers, not unavailable per-line timestamps. The pre-mixed prefix is retained separately and excluded from mixed callback summaries.
- Before/After document sides are captured revisions, not a matched performance baseline. Cold/warm and distinct fixture timings do not establish a speedup.
- Image retirement records show released CPU ownership and requested GPUI image retirement; they do not measure GPU resource bytes or prove all resources released before process exit.
- System background activity is present; no uncontended-machine claim. Docker process names do not establish active container workload.
- Private raw evidence contains local paths and accessibility text; durable output preserves numerical samples and hashes without copying raw notes or identities.
- This observes the release candidate bundle. Final installation and restored-state verification are separate coordinator evidence.
- Native Linux UI, live authenticated GitHub and hosted CI are outside this native session.
