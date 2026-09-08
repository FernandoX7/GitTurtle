# Read-only everyday backend benchmark

`crates/git-core/examples/everyday_bench.rs` measures synchronous core calls. It does not stage, commit, update configuration, refresh the index, invoke network operations, or create a fixture. Run it against an existing disposable fixture or a repository authorized for passive inspection.

Build and run after other compilation and native interaction checks have finished:

```sh
cargo build --locked --release -p gitturtle-core --example everyday_bench
./target/release/examples/everyday_bench /path/to/fixture \
  --file src/example.rs --query fix --scope all \
  --warmup 2 --samples 20 --pages 2 --page-size 100 \
  --budget-seconds 300 --label mixed-staging-and-renames \
  --build-label 'SOURCE_REVISION; dirty patch identity if applicable' \
  > /tmp/gitturtle-everyday-bench.json
```

Keep output outside the measured repository so recording the report cannot create working-status events. Stdout contains one JSON document; progress goes to stderr. Preserve the raw report, including errors and outliers. This document supplies reproduction instructions, not measured results.

For a representative disposable fixture, prepare a realistic history with merges and renames, plus a tracked text file containing both staged and unstaged edits. Finish fixture preparation before starting the harness and keep it unchanged throughout the run. Also measure a realistic existing repository when authorized; clean repositories correctly skip working-preview operations. Do not manufacture changes in an existing user repository for coverage.

The optional `--file` is one literal repository-relative path, including leading dashes or unusual filename bytes supported by the OS. Without it, each working area chooses its first eligible nonconflicted status entry. File history chooses the first eligible changed working path, or the first changed path of the selected commit if the working tree is clean. The report records the actual selected paths; an untracked path can legitimately have no committed history.

`--commit FULL_OID` selects the immutable commit for changed-file and file-history reads; the default is the initial HEAD. `--parent N` selects the parent for changed-file comparisons (default 0). File history always follows its documented first-parent lineage through renames. `--scope head` restricts search to the initial HEAD; the default `all` captures local refs once, including locally available remote-tracking refs and detached HEAD. It never fetches. Every search sample starts from those same pinned tips and follows returned scan offsets within that sample. The JSON includes those tips, the query and continuation/stop details.

Measured operations are:

- `status`: the passive porcelain status snapshot.
- `staged_worktree_preview` and `unstaged_worktree_preview`: selected core preview, including raw source reads, text diff and partial-staging metadata when supported. Selection uses the initial status outside the timed interval. Missing areas, conflicts and bare worktrees are reported as skipped; binary, oversized or filtered previews report their actual result and partial-action availability.
- `pinned_history_search_pages`: up to the configured number of bounded repository-wide search pages from the same pinned scope.
- `file_history_follow_first_parent_pages`: bounded rename-following history from the selected immutable commit and literal path. The core does not expose how many commits this Git operation scanned, so `search_scanned` is null for this series.
- `changed_files` and `changed_files_with_renames`: the selected commit/parent's metadata, with and without explicit bounded rename detection.

Each series contains raw warmup and measured attempts. Successful measured attempts alone contribute to nearest-rank p50, p95 and maximum; failed calls retain their elapsed time and error instead of becoming zero-latency samples. A budget stop during pagination is an incomplete failed attempt. A normal search page, scan, byte or time limit is recorded as a bounded result with a continuation and must not be described as exhaustive history. Inspect returned page/entry/scanned counts when comparing samples.

One `GitRepository` and its object reader are retained for the run. There is no application content cache and no filesystem cache flush. Setup reads initial status, captures search tips and reads the selected changed-file list, so even zero explicit warmups are not a cold-disk experiment. Checksums and result summaries run after the timer stops; their reads can warm memory caches between samples. Timing excludes result destruction, UI queues, application patch/split presentation preparation, image decoding, editor construction and native frame presentation. Working previews include core partial metadata, but do not measure applying a patch.

The harness caps warmups at 10, samples at 100, pages per sample at 4 and entries per page at 500. A configurable run budget (default 180 seconds, maximum 1,800) is checked before each sample and between history pages. Active core calls retain their own input, output and process deadlines; the run budget is not a hard wall-clock interruption of an active filesystem read. Final status inspection still runs after a budget stop.

Review `status_unchanged`, each series' `completed`/`result_changed`, raw errors and result fingerprints before comparing timings. Status equality cannot detect every edit that leaves Git status unchanged. Selected preview results include raw-side digests, object IDs, modes and partial metadata counts to expose drift during that series. Keep the fixture stable and report any concurrent activity; these checks do not establish atomic isolation of the entire repository.

For a published result, accompany the JSON with the exact source/build identity (and patch identity for an uncommitted build), release command, hardware/RAM, OS version, Git/Rust versions, other system load, fixture preparation and cache conditions. The harness records package version, target OS/architecture, debug-assertion setting, available parallelism, timestamps, initial/final status fingerprints and HEAD, without collecting hostnames, environment variables, remote URLs or commit messages. `--build-label` is supplied by the operator and is not independently verified build provenance.

Developer checks, without a release measurement:

```sh
cargo check --locked -p gitturtle-core --example everyday_bench
cargo test --locked -p gitturtle-core --example everyday_bench
cargo clippy --locked -p gitturtle-core --example everyday_bench -- -D warnings
```
