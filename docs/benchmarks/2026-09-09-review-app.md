# App review preparation and path-filter measurements

Current release CPU costs for the production preparation/filter helpers. These measurements do not establish a speedup, native frame latency, queue responsiveness, or older-hardware performance.

[Raw JSON](2026-09-09-review-app.json) retains every warmup and sample, stable result dimensions, memory observations, complete compiled-input hashes, and hardware/load data. Reproduce with `python3 scripts/bench-app-review.py --output <raw.json>`; the runner creates and removes its disposable seed repository and builds a release test executable. The ignored test never launches GPUI.

## Execution and identity

- HEAD: `06c2e7cbca165342e3109615e7bd055136949b28` plus the exact source hashes in the JSON. The only benchmark hooks are compiled under `cfg(test)`.
- Compiled-input map SHA-256: `f3b7a597d137aad122305040d11aa093d559c33ac8eaa6f83497996204f7e866`; all 131 input hashes matched before build, after build and after sampling. Cargo.lock pins external dependencies; the map covers workspace manifests, Rust sources and embedded assets.
- Release test executable SHA-256: `b629ccda394122eb9d9dec9376c7489367e18e010c5166faa5357c5866876104`.
- UTC interval: 2026-09-09T05:10:44.503962+00:00–2026-09-09T05:10:48.372019+00:00; 3.804 seconds including disposable seed setup.
- Hardware/software: Apple M4 Max, Mac16,5, 16 logical CPUs, 128 GiB RAM; macOS-26.6.2-arm64-arm-64bit-Mach-O; rustc 1.98.0 (88d9e12ae 2026-08-18); git version 2.50.1 (Apple Git-155).
- Background load was not isolated. One-minute load averages were 19.99 before and 18.79 after. Process snapshots are in the JSON.

## Fixtures and boundaries

Deterministic synthetic 201-hunk Unicode text, mixed LF/CRLF, no final newline, 200 whitespace-only replacements; 50,000 synthetic path records cloned from a real untracked seed status with mixed staged/unstaged/untracked/conflict flags and 7,143 renamed paths.

Text sources have 20,000 lines per side and 201 change blocks: 200 adjacent substantive/whitespace-only replacement pairs and one final-line replacement. Mixed LF/CRLF endings and the missing final newline are preserved. The ordinary source and patch buffers, their immutable prepared presentation, and both 50,000-path indexes are created before timing. One separate patch contains a 24 KiB line on each side to exercise the bounded intraline fallback.

Historical indexes normalize both old/new paths; working indexes normalize current/original paths. The selective query is `module_017/` (500 matching entries), broad query `.rs` (50,000), absent query `missing_review_path` (zero), and renamed-history query `ancien_` (7,143). Working samples include matching, production staged/unstaged row construction, optional directory sorting, and total staged/conflict summaries. Source row indices remain operation identities. No selected-file content or Git status reads are timed.

Production in-memory preparation/matching/row helpers, including result allocation; excludes setup, fingerprints, result destruction, serial-queue scheduling, UI callbacks, editors, rendering and Git reads.

Inputs and cached indexes allocated before timing; three warmups per series; fresh output each sample. No filesystem cache flush or application preview cache.

Each series has three warmups and forty measured calls. All result dimensions/fingerprints matched within each series. Percentiles use nearest ranks; p99 equals the maximum with forty samples. The conventional median is also retained in JSON. No outlier was removed.

## Results

| Production helper series | p50 ms | p95 ms | p99 / max ms |
| --- | ---: | ---: | ---: |
| patch_intraline_201_hunks | 1.224 | 1.347 | 1.433 |
| split_intraline_20k_lines | 1.933 | 2.274 | 2.432 |
| intraline_24k_byte_line_fallback | 0.006 | 0.006 | 0.007 |
| review_hide_whitespace_context3 | 2.187 | 2.370 | 2.654 |
| review_expand_context48 | 4.410 | 4.824 | 5.180 |
| review_expand_context192 | 4.417 | 4.641 | 4.930 |
| review_hide_whitespace_context48 | 3.310 | 3.446 | 3.461 |
| history_prepare_index_50k | 6.636 | 6.797 | 6.865 |
| working_prepare_index_50k | 3.771 | 3.984 | 4.384 |
| history_cached_selective_50k | 0.682 | 0.727 | 0.780 |
| history_cached_all_50k | 0.271 | 0.304 | 0.407 |
| history_cached_absent_50k | 0.418 | 0.492 | 0.537 |
| history_cached_renamed_50k | 0.329 | 0.380 | 0.434 |
| working_cached_flat_selective_50k | 0.531 | 0.645 | 0.720 |
| working_cached_grouped_all_50k | 39.015 | 40.868 | 41.158 |
| working_cached_grouped_selective_50k | 0.716 | 0.877 | 0.976 |
| working_cached_grouped_absent_50k | 0.407 | 0.475 | 0.538 |

## Memory and limitations

Release-test process RSS from `ps` was 10,688 KiB before fixture construction, 75,264 KiB afterward, and 106,432 KiB after all samples. The already-built process peaked at 109,395,968 bytes (104.3 MiB) RSS and 100,057,616 bytes (95.4 MiB) physical footprint according to macOS `/usr/bin/time -l`; the complete resource output is retained in JSON. Compilation is excluded. Loaded libraries, synthetic fixtures, cached indexes and allocator-retained allocations are included. These are process observations, not editor/GPU measurements or a product memory cap.

The seed file and index presence were unchanged. Inputs and result validation warm caches; no filesystem flush was attempted. Samples exclude queue scheduling/cancellation latency, main-thread callbacks, editor construction, rendering and OS presentation, image decoding, native interaction and actual repository path discovery. Rapid-selection memory growth, Linux and other hardware remain outside this record.
