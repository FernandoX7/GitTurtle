# Everyday Git backend measurements — September 8, 2026

These release measurements record current synchronous Git-core costs on a large disposable working fixture and the GitTurtle project. On the 996-entry working fixture, passive status had a 51.914 ms p50 and 53.538 ms p95; a complete 653-commit search had a 29.458 ms p50 and 31.441 ms p95. There is no baseline run or speed-improvement claim.

Raw attempts, result fingerprints, selected object IDs, pinned search tips, and stop conditions are preserved in the [rich-fixture JSON](2026-09-08-everyday-rich-core.json) and [project JSON](2026-09-08-everyday-project-core.json). The [harness reproduction guide](everyday-bench.md) describes its arguments and measurement boundary.

## Build and conditions

- Hardware: Mac16,5, Apple M4 Max, 128 GiB RAM; 16 available logical processors reported by the harness.
- Software: macOS 26.6.2, Rust 1.98.0 (`88d9e12ae`, 2026-08-18), Git 2.50.1 (Apple Git-155), aarch64 release build with debug assertions disabled.
- Build: `cargo build --locked --release -p gitturtle-core --example everyday_bench` at `082182108652a553948ba24b6b3499df09c399a9`. The core implementation and harness were unchanged from `a0c12543b23c21dd832eef9d8bd0f21fab9822f9`; this identifies the measured code, not a later app package.
- Sampling: two warmups followed by 20 measured calls per eligible operation, serially. History samples allowed at most two pages of 100 entries. The run budget was 180 seconds; neither run stopped for budget or reported a failed attempt.
- Cache: one retained `GitRepository` and object reader; no app content cache. OS caches were not flushed or otherwise controlled. Setup read status, pinned refs, and selected changed-file metadata; checksums outside the timed interval also warmed memory caches. Treat these as warm repeated-call observations, not cold-start measurements.
- Other load: no compilation, tests, or native interaction ran during measurement. A debug GitTurtle app was open but idle on another fixture, and other desktop apps remained open. OS background activity was not quantitatively sampled, so a quiescent system is not assumed.

The tables use nearest-rank p50 and p95 over the 20 successful measured samples, rounded to three decimal places. Warmups are excluded; all measured outliers remain in the raw data. With 20 samples, p95 is the nineteenth sorted sample, and maximum is the twentieth.

## Rich working fixture

The disposable fixture had 653 reachable commits, 91 local branches, and 92 distinct pinned search tips. Selected HEAD was `bcf41e2d24ff5582a145be5d34ebbdca774c95b4`, compared with parent index 0. Its changed-file list contained 1,043 entries with rename detection, including 40 renames; disabling detection represented those renames as separate additions/deletions and returned 1,083 entries.

Working status contained 996 distinct entries, with 330 staged-side entries, 668 unstaged-side entries, and two untracked files. Side counts overlap for files with both staged and further unstaged changes, and untracked files are included in the unstaged count. There were no conflicts. Staged and unstaged preview samples both selected `src/precise-commit-review.ts`: HEAD/index/worktree sizes were 16,197 / 16,221 / 16,251 bytes, with three partial-staging hunks and six changed lines in each comparison. Timings include reading and preparing core partial metadata, without applying any staging action.

| Core operation | p50 (ms) | p95 (ms) | Maximum (ms) |
| --- | ---: | ---: | ---: |
| Passive status, 996 entries | 51.914 | 53.538 | 61.143 |
| Staged text preview and partial metadata | 15.739 | 16.249 | 16.289 |
| Unstaged text preview and partial metadata | 13.724 | 16.115 | 16.205 |
| Pinned search, 653 commits scanned | 29.458 | 31.441 | 31.929 |
| File history, two pages / 200 revisions | 73.551 | 74.981 | 75.850 |
| Changed files without rename detection, 1,083 entries | 17.357 | 18.673 | 18.916 |
| Changed files with bounded rename detection, 1,043 entries | 18.772 | 20.509 | 22.828 |

Search used the literal query `RICH-QA-DEEP-SEARCH-ANCHOR`, found one match, and exhausted all 653 commits in one page. File history used `src/precise-commit-review.ts` and returned 200 revisions over two calls, with `next_offset: 200` and `Page full`: it was **not exhaustive**. This selected path is distinct from the renamed notes lineage in the fixture, so the history timing does not isolate crossing that rename boundary. The operation still uses the core's bounded rename-following, first-parent traversal and prefix replay for pagination.

The local fixture manifest is retained at `.local/rich-native-qa-20260908T172155Z-artifacts/manifest.json` (unversioned), with generator SHA-256 `fddc483a0f37711873764da1c7f01b941cf23d7909d6a27cae1b449417b1d8c9`. Fixture creation was finished before measurement. A separate before/after preservation check found all 1,236 snapshotted tracked-worktree and Git-metadata entries unchanged in bytes and modification time after the run. That check excluded immutable object storage and access times. The harness also reported unchanged initial/final status and stable results in every series.

## GitTurtle project

The project was measured at HEAD `082182108652a553948ba24b6b3499df09c399a9`, again using parent index 0. Its selected commit had nine changed files. Initial and final working status reported nine unstaged-side entries, including three untracked files, with no staged changes or conflicts. The staged-preview series was explicitly skipped; it is not a zero-time sample.

| Core operation | p50 (ms) | p95 (ms) | Maximum (ms) |
| --- | ---: | ---: | ---: |
| Passive status, nine entries | 42.428 | 45.253 | 45.337 |
| Staged preview | Skipped | Skipped | Skipped |
| Unstaged text preview and partial metadata | 14.052 | 16.351 | 16.578 |
| Pinned search, 38 commits scanned | 14.228 | 14.979 | 15.091 |
| File history, one page / two revisions | 23.018 | 23.640 | 23.785 |
| Changed files without rename detection, nine entries | 14.685 | 15.298 | 16.040 |
| Changed files with bounded rename detection, nine entries | 13.522 | 14.784 | 16.010 |

The unstaged preview selected `crates/app/src/split_diff.rs`, comparing 18,660 index bytes with 18,808 working bytes and preparing two partial hunks with seven changed lines. Search pinned four tips, used `native`, and returned 15 matches after exhausting 38 commits. File history used the same selected path and exhausted its two revisions in one page. Both changed-file variants returned the same nine entries; their separate timing distributions do not establish that rename detection is faster.

The project report was written to `/tmp` outside the measured repository, then copied into `docs/benchmarks` after completion. Initial/final status matched, and every measured series completed with stable result fingerprints. This establishes stability of the captured status and selected results, not a complete byte-for-byte snapshot of the project.

## Interpretation boundary

Timing ends when the synchronous core API returns. It excludes preview-selection status lookup, result checksums and validation, result destruction, app worker queueing, graph and patch/split presentation preparation, editor construction, image decoding, GPUI frames, OS display presentation, and GPU completion. Setup and checksums are recorded or performed outside each operation's timed interval. The rich run took 5.419 seconds overall with 340.713 ms setup; the project run took 2.965 seconds with 165.575 ms setup.

These observations describe the selected local data and warm-cache conditions. They do not measure native click-to-display latency, staging/commit/recovery writes, watcher behavior, cancellation latency, long-history limit handling, resource growth, or another platform. File-history scan counts are unavailable from this API; its result counts must not be mistaken for scanned-commit counts. The raw continuation and stop fields remain necessary when interpreting bounded history work.

## Native release measurements — `f68bd20`

A separate native run used the release package built from `f68bd2070d4c71eec00af02cb7f67b1be107e728`, with executable UUID `83F6445B-F09F-3AAC-A138-BA72E7D77CAF` and SHA-256 `672615cd826e9c7001e5ef774c08b90d350ee7d6935857668c336b7e315049b1`. The machine and software versions match the hardware/toolchain listed above. A fresh app process opened only the rich fixture at `bcf41e2d24ff5582a145be5d34ebbdca774c95b4`, using Daylight and Comfortable density. This run has a different measurement boundary and build from the backend series above.

The [native raw record](2026-09-08-everyday-native.json) preserves every sample, exact input targets, excluded preparation callbacks, source byte ranges, and trace hashes. Extraction used the completed start/end offsets in `.local/native-release-phases.json`, not any subsequent appends to `.local/everyday-release-native.trace`. Each measured series contains twenty deliberate inputs and twenty completed callbacks. Nearest-rank statistics retain all measured outliers; there was no separate warmup series.

| Native interaction | Samples | p50 (ms) | p95 (ms) | Maximum (ms) |
| --- | ---: | ---: | ---: | ---: |
| History selection to changed-file callback | 20 | 21.241 | 45.238 | 62.794 |
| Immutable text-file activation to prepared-preview callback | 20 | 4.898 | 7.428 | 13.216 |
| Staged text activation to prepared-preview callback | 20 | 21.838 | 21.947 | 22.043 |
| Unstaged text activation to prepared-preview callback | 20 | 23.248 | 24.800 | 24.838 |

History navigation used ten Down actions from `bcf41e2` through guidance revision 631 (`99aac553addb52e67e8f499278e08d8aa0f31dba`), then ten Up actions back to the initial commit. The 38.690 ms click on the already selected initial row was excluded and retained separately. The measured traversal produced twenty changed-file callbacks and **zero file-preview callbacks**. The startup callback before this phase was also excluded.

Immutable-file navigation began with `fixtures/review/additions/新增-000-comparison-notes.md`, whose initial activation was outside the phase. Ten Down actions reached `新增-010-comparison-notes.md`; ten Up actions returned to `新增-000`. All twenty measured previews were added text files in unified Diff mode. Return selections could reuse the 32-entry immutable preview cache; the cache was not cleared between visits.

Staged navigation began at `fixtures/review/additions/新增-002-comparison-notes.md`, excluded its initial activation, then used ten Down and ten Up actions. The staged list contained 330 entries; its first eleven paths advance in steps of three from `新增-002` through `新增-032`. Exact paths and their twenty-sample order are recorded in the JSON, recovered from passive index metadata after timing. All measured staged previews used text and unified Diff mode.

Unstaged measurement alternated ten pairs between `fixtures/review/batch-09/renamed-component-guidance/日本語-über-Δ/0969-long-filename-with-selection-focus-hover-and-disabled-state-review.md` and `src/precise-commit-review.ts`, beginning with the former and ending with the latter. The precise file retained three unstaged partial hunks. Preparation and initial activations were outside the measured phase. Both staged and unstaged previews bypass the immutable cache.

The four phases ran in the order listed, without another build, test, backend benchmark, or native interaction during timing beyond the recorded inputs. Other desktop apps remained open, OS background activity was uncontrolled, and filesystem caches were not flushed. No Git writes occurred. A post-phase read-only comparison verified the full 1,236-entry baseline inventory—1,215 regular files and 21 stored symlinks—with identical kinds, byte sizes, SHA-256 values, and modification times. It excluded immutable `.git/objects`, access times, and directory metadata; the raw record identifies the baseline and its digest.

These metrics start in the application handler and end at the generation- and mode-checked GPUI callback after changed-file or preview preparation. They exclude input delivery before the handler, OS display presentation, and completed GPU execution. Working-preview samples also exclude a preceding status refresh or Git write. The small uncontrolled sample establishes neither a speedup nor a latency guarantee; it does not measure image decoding, writes, watcher/cancellation latency, resource growth, or Linux behavior.
