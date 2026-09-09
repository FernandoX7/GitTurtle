# Review milestone backend measurements — September 9, 2026 UTC

All thirteen passive review paths completed forty measured release calls with stable results and no fixture changes. Selective Quick Open worktree search had a 22.289 ms p95 over 12,005 tracked entries. Reviewing a fifty-commit rebase range had a 252.654 ms p95. These are current warm backend costs under substantial background load; there is no historical speedup or native-responsiveness claim.

[The raw JSON](2026-09-09-review-backend.json) retains all 559 attempts: 39 warmups and 520 measured calls, with no failures. It includes fingerprints, result dimensions, source/build identities, hardware/load observations and preservation checks. [The procedure](review-bench.md), [Rust harness](../../crates/git-core/examples/review_bench.rs) and [fixture/report runner](../../scripts/bench-review.py) provide reproduction instructions.

## Source and execution

- Baseline: `e85883bbf42284d0cd695c602b3d3ad265cfbb49`. Build HEAD: `75cdb8a055c9eb2e7569873e5a57fef9b1321e8f`, plus the milestone's working changes. This baseline identifies source history; no baseline timing run was performed.
- SHA-256 over the 21 compiled input hashes: `64c1199dd6535b10f3a5aa3d4fa1c1e503be41bfcd6ad94d3b9bfeafca841506`. The inputs include all Git-core Rust sources, the harness, workspace/core manifests and Cargo.lock. The map was identical before compilation, after compilation and after measurement, including seven new untracked source/harness files.
- The tracked diff from the baseline was 11,909 bytes with SHA-256 `877a79aa686265bdcc0fa1f866d0c30e9a6edb95f7fde2d6037fc84cee68cbeb`. Git's tracked diff omits new untracked files; the complete input map above includes their contents.
- Harness SHA-256: `535aa6a9af53d4e0e53ddece205d3317be39483a5732caa3a2514d95decbbbdc`. Runner SHA-256: `e804a0a4288c72c31a2d169356b7564aabb213dae293f52659204a845868a464`.
- Build: `cargo build --locked --release -p gitturtle-core --example review_bench`. It completed before the timed run. Executable SHA-256: `dab764fddb8b1bcaa615e211a1b1817c3d0bdf84484cf0e72db553fba5edf6c8`.
- Run: September 9, 04:41:49–04:42:37 UTC (September 8, 22:41–22:42 MDT), 47.787 seconds. Every series used three warmups followed by forty measured attempts.
- Hardware/software: Apple M4 Max, Mac16,5, 16 logical processors, 128 GiB memory; macOS 26.6.2 / 25G83 arm64; Git 2.50.1 / Apple Git-155; Git LFS 3.8.0; Rust 1.98.0 (`88d9e12ae`); Cargo 1.98.0.
- Load was not isolated: one-minute load averaged 34.26 before and 29.82 after. Recorded samples show multiple active Node processes and WindowServer. These measurements do not represent an otherwise idle machine.

## Results

Percentiles use nearest ranks; p99 equals the maximum with forty samples. The JSON also retains the conventional median, which averages the middle two samples. Timings below are milliseconds, rounded to three decimals. No successful outlier was removed.

| Synchronous backend call | p50 | p95 | p99 | Maximum | Observed result |
| --- | ---: | ---: | ---: | ---: | --- |
| Endpoint comparison | 53.823 | 78.094 | 114.766 | 114.766 | 804 changed files, including rename/binary metadata |
| Changes since branching | 69.545 | 83.324 | 87.075 | 87.075 | One merge base; 802 changed files |
| Quick Open, selective worktree query | 18.955 | 22.289 | 26.011 | 26.011 | 100 matches; all 12,005 entries scanned |
| Quick Open, selective revision query | 53.561 | 71.831 | 84.069 | 84.069 | 100 matches; all 12,005 entries scanned |
| Quick Open, absent revision query | 51.041 | 60.367 | 61.626 | 61.626 | Zero matches; all 12,005 entries scanned |
| Quick Open, bounded worktree query | 18.254 | 28.987 | 37.768 | 37.768 | 500 matches after 505 entries; explicitly truncated |
| Parse 256 conflict blocks | 0.030 | 0.055 | 0.064 | 0.064 | 47,872 bytes; Unicode/CRLF diff3 text |
| Parse 2,048 conflict blocks | 0.229 | 0.337 | 0.358 | 0.358 | 382,976 bytes; Unicode/CRLF diff3 text |
| Prepare interactive rebase | 180.936 | 252.654 | 348.990 | 348.990 | 50 linear commits; one containing remote-tracking ref |
| Worktree discovery | 24.096 | 28.755 | 40.001 | 40.001 | Two registered worktrees |
| Selected linked-worktree details | 284.087 | 336.439 | 358.117 | 358.117 | Seven changed/untracked files; one ignored file |
| HEAD reflog read | 34.263 | 37.916 | 40.501 | 40.501 | 1,000 entries; explicitly truncated |
| Missing-LFS download plan | 257.687 | 311.747 | 347.456 | 347.456 | One 35,840-byte missing object; no download |

Selective path queries use `module_001`, the empty-result query uses `missing-review-match`, and the bounded query uses `module_`. The revision scope is pinned to the main fixture tip. Path timings include discovery and filtering, but not opening the selected file, constructing its editor, or showing File History/Blame.

Comparison targets are the same two diverged local branches in both modes. Endpoint comparison includes the Before branch's independent changes, while changes since branching compares their common ancestor with After. Rebase preparation reads its separate exclusive base and fifty linear main-branch commits. No rebase or worktree mutation occurs inside the measurement.

## Fixture, caches and preservation

The deterministic fixture has 12,005 tracked files, 61 main-branch commits, a diverged comparison branch, a renamed Rust file, binary content, a stored symlink and a Unicode path. Its main HEAD is `bb1c3680ebc3d8cc144874b347c047906868b2cf`; comparison Before is `4dcfc4a21255d3e75c1e41ab23f395fb8ccce227`, the merge base is `25a4064d45f7538b0dc767e91044cc1daec428b4`, and the rebase base is `cc0fcf215993d18198c218f8666da945a13942d5`. A linked worktree has four tracked edits, three untracked files and one ignored file. Git's system/global configuration is excluded for the fixture; its explicit local configuration is retained.

The runner uses ordinary Git ref transactions to create the reflog corpus and returns HEAD to the same tip before timing. The LFS pointer refers to deliberately absent content and an empty local bare origin. LFS preparation inspects configuration using isolated temporary storage; it never fetches, checks out, smudges or stages the object.

One `GitRepository` and object reader are retained. Fixture setup, initial status and complete inventories warm filesystem data. There is no filesystem flush and no application content cache. In-memory conflict documents are created before timing. Validation, fingerprints and result destruction occur outside each timer and may warm memory between attempts.

Before/after inventories matched across all 24,240 entries: 24,070 files, 168 directories and two symlinks. The comparison covers type, mode, content, symlink target and modification time, excluding access time. Both inventory hashes were `8e7240bcf3d4b65c70b32f3a9ed91f30a52a2e3776f16a1a5a24969b6300d7ee`. They cover both worktrees, common/private Git metadata and the local bare remote. Initial/final status and every per-series result fingerprint also matched.

The harness passed `cargo clippy --locked -p gitturtle-core --example review_bench -- -D warnings`; the runner's syntax and disposable fixture dimensions were checked before the recorded run. The release command above completed successfully.

These observations exclude app scheduling, native input/frame delivery, rendering, editor construction, image decoding, mutation and cancellation latency, transport, memory-growth analysis, Linux and older hardware. They describe the recorded compiled inputs, not an unspecified later app package. A 40-sample warm run under uncontrolled load cannot establish a general latency guarantee.
