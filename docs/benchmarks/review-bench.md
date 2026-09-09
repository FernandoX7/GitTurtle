# Passive review backend benchmark

[`review_bench.rs`](../../crates/git-core/examples/review_bench.rs) measures synchronous core API calls in release mode. [`bench-review.py`](../../scripts/bench-review.py) creates a fresh disposable fixture, compiles the harness, records provenance and preserves every warmup/measured attempt in JSON. Neither the harness nor the timed calls perform a repository write or download. Fixture construction finishes before measurement.

Run from the project root, after other compilation and native QA have paused:

```sh
python3 scripts/bench-review.py --output /tmp/gitturtle-review-backend.json
```

The output must be a new file; existing attempts are never overwritten. The runner creates its own temporary fixture and has no option to mutate an existing repository. It builds using `cargo build --locked --release -p gitturtle-core --example review_bench`. Fixture setup uses ordinary Git fast-import, ref transactions, worktree creation and local configuration; Git's system/global configuration is excluded in this controlled fixture. No remote transport runs.

The fixture contains 12,005 tracked files, 61 commits on the main branch, a diverged comparison branch, a 50-commit linear review range, a remote-tracking containment warning, a binary change, a rename, a symlink and a Unicode path. A second worktree has four tracked edits, three untracked files and one ignored file. A single Git process performs 1,200 ref transactions for the bounded reflog corpus and returns HEAD to its original target before timing. The LFS pointer describes an absent local object and an empty local bare origin.

Each of thirteen series has three warmups and forty measured attempts:

- Endpoint comparison and changes since branching between the same resolved branch targets.
- Quick Open path discovery in the worktree and a pinned revision, including selective, empty-result and 500-result-boundary queries.
- In-memory conflict parsing for 256 and 2,048 diff3 blocks, with Unicode and CRLF. Source construction happens before timing.
- A 50-commit interactive-rebase plan, including local remote-tracking containment.
- Worktree discovery and detailed status of the selected linked worktree.
- The worktree-local HEAD reflog, bounded to 1,000 returned entries.
- A missing-LFS download review plan. Git LFS configuration inspection uses isolated temporary storage; no object is downloaded, checked out, smudged or staged.

The timer stops when the core API returns. Result fingerprints, validation, formatting and destruction run afterward. Raw attempts retain failures and outliers; failed attempts never count as zero latency. Result fingerprints must remain identical within each series. Percentiles use nearest ranks; with forty successful samples p99 equals the maximum. The separate median averages the two middle values.

One `GitRepository` and its object reader are retained. Setup and initial inventory checks warm filesystem data, and the three explicit warmups further warm the measured path. No filesystem cache flush or application content cache is involved. These are warm backend observations, not cold-launch or native-frame measurements.

The report records the source baseline, actual build HEAD, tracked diff hash, hashes of every compiled Git-core source and the harness, workspace/core manifests, Cargo.lock, the runner and release executable. New untracked source files are included in the compiled-input hash map even though Git's tracked diff omits them. Source hashes must match before compilation, afterward and after measurement.

Complete before/after fixture inventories compare entry types, modes, exact bytes, symlink targets and modification times, excluding access times. Their hashes cover both worktrees, common/private Git administration and the local bare remote. Any source drift, fixture drift, changed result or operation failure invalidates the attempt; its JSON remains available for diagnosis.

Accompany recorded measurements with OS/hardware, Git/LFS/Rust versions, load observations, exact fixture/result dimensions and the limitations relevant to the claim. These costs exclude app queues, rendering, editor creation, decoding, input delivery and displayed frames. They establish neither a historical speed improvement nor a responsiveness guarantee.
