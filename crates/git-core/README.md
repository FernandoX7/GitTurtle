# GitTurtle Git service

Blocking Rust read APIs intended for bounded background workers. The crate exposes no mutation, fetch, filter, hook, external-diff, checkout, status-refresh, or maintenance operation.

- Local and remote-tracking branches, including the current branch of the opened worktree.
- Worktrees with byte-preserving paths and detached, locked, and prunable states.
- Git topological history with parents, messages, paging, and an optional fixed commit anchor.
- Root and selected-merge-parent comparisons, mode/type changes, and optional bounded rename detection.
- A shared persistent `git cat-file --batch-command` service for commit parents, object size queries, and raw file bytes.
- Unified text patches plus retained source bytes for split views, explicit binary/oversize/submodule states, and local LFS resolution.

Git arguments are fixed and supplied directly to `Command`. User/system Git configuration is excluded. Automatic lazy fetch, credentials, protocols, filesystem-monitor helpers, automatic maintenance, optional locks, and replacement objects are disabled. Tracked symlinks are read as Git blobs, never followed into the worktree. `git config` is used only with `--get` to read local LFS storage configuration.

Local LFS lookup supports the common Git directory shared by linked worktrees and repository/worktree `lfs.storage` configuration. Relative storage is resolved against the common Git directory. Objects must match the pointer's size and SHA-256 digest; directory-relative `NOFOLLOW` opens prevent symlink escapes within the store. The resolver does not invoke Git LFS or extensions, does not create directories, and never downloads missing content. Global/system storage overrides and LFS reference-object stores are not searched. These are explicit compatibility limits. [Git LFS storage configuration](https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-config.adoc)

## Budgets and behavior

| Resource | Bound |
| --- | --- |
| Raw object or local LFS bytes | 64 MiB, additionally bounded by the caller for LFS |
| Text diff input | 2 MiB and 100,000 newline-delimited lines per side |
| Text diff algorithm | Patience, with a 250 ms algorithm deadline |
| Git subprocess/object request | 15 seconds per request; timed-out processes are terminated and reaped |
| Command output | 128 MiB stdout, 128 KiB stderr |
| Rename candidate limit | 1,000; rename detection is opt-in |

Image callers must separately bound decoded pixels and GPU allocations. A generation check or queue policy belongs to the UI worker; core methods do not cancel already-running calls when selection changes. Multiple calls may be needed for one interaction, so the per-request deadline is not an end-to-end interaction deadline. OS scheduling or uninterruptible filesystem I/O is outside these deadlines' guarantee.

`history_page` reads current refs and can shift if another tool changes them. Reset paging on refresh, or use `history_from_page` with an immutable commit anchor. Remote-tracking branches describe locally available state, with freshness controlled by another tool's fetch. Missing promisor objects return an explicit local-unavailability error.

## Verification

```sh
cargo test -p gitturtle-core
cargo run --release -p gitturtle-core --example inspect -- /path/to/repository
```

Tests mutate only temporary repositories. They cover empty/bare repositories, roots, merge parent selection, topological paging, branches and linked worktrees, non-UTF-8/newline paths constructed directly in Git trees, binary/text/mode/type changes, gitlinks, SHA-256 repositories, partial clones, local LFS integrity and symlink rejection, stalled-process cleanup, and repository-file snapshots with hostile configured helpers.

## Initial measurement, September 7, 2026

Hardware: Apple M4 Max; macOS 26.6.2; Apple Git 2.50.1; Rust 1.98.0; release profile. Data: the locally available `world-of-claudecraft` repository, 5,577 branches and 130 worktrees. Each run opened a new application-side repository handle, read 500 commits, and inspected the first changed file in each of 50 commits. The OS filesystem cache was not flushed; this is not a cold-disk measurement. No application-level content cache was used. No previews were unavailable in these samples.

| Operation | Sample A p50 | Sample A p95 | Sample B p50 | Sample B p95 |
| --- | --- | --- | --- | --- |
| Changed-file list | 13.31 ms | 18.57 ms | 28.06 ms | 66.70 ms |
| First-file text preview | 0.301 ms | 1.44 ms | 0.573 ms | 32.20 ms |
| File list + first text preview | Not recorded | Not recorded | 29.00 ms | 84.24 ms |

Sample A: open 25.06 ms; branch/worktree reads 112.20 ms; 500-commit history 59.72 ms. Sample B ran while the native workspace was compiling: open 92.91 ms; branch/worktree reads 222.28 ms; history 164.22 ms; combined maximum 131.47 ms. System load was not controlled or quantitatively sampled, so these are observations, not a controlled comparison or a performance guarantee.

The harness excludes input dispatch, the UI queue, rendering, image decode, syntax highlighting, and frame presentation. The combined metric covers backend file-list and text-preview work only. Native interaction latency and memory behavior require separate app measurements. Replacing the per-selection `rev-list` subprocess with raw commit reads removed one process startup; later work can investigate the remaining `diff-tree` subprocess only if integrated measurements justify it.
