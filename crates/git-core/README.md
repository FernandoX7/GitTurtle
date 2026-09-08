# GitTurtle Git service

Blocking Rust APIs intended for bounded background workers. Immutable history/object inspection is separate from the explicit working-copy API in `src/work.rs`. Passive reads never mutate repositories or start network requests. The app submits writes to its serialized operation worker only after a user action.

- Local and remote-tracking branches, including the current branch of the opened worktree.
- Worktrees with byte-preserving paths and detached, locked, and prunable states.
- Git topological history with parents, messages, paging, and an optional fixed commit anchor.
- Root and selected-merge-parent comparisons, mode/type changes, and optional bounded rename detection.
- A shared persistent `git cat-file --batch-command` service for commit parents, object size queries, and raw file bytes.
- Unified text patches plus retained source bytes for split views, explicit binary/oversize/submodule states, and local LFS resolution.

For immutable inspection, Git arguments are fixed and supplied directly to `Command`. User/system Git configuration is excluded. Automatic lazy fetch, credentials, protocols, filesystem-monitor helpers, automatic maintenance, optional locks, and replacement objects are disabled. Tracked symlinks are read as Git blobs, never followed into the worktree. `git config` reads local LFS storage configuration. Passive status additionally enumerates filter keys to disable configured clean/process drivers: Git status can otherwise execute them when checking dirty stat entries. Status uses porcelain v2 with NUL-delimited byte-safe paths and disables optional index refresh locks. It reports submodule commit changes but leaves dirty file inspection inside each submodule to opening that repository. Filtered files use raw working bytes for inspection; this can conservatively report a change that an external clean filter would normalize away.

Local LFS lookup supports the common Git directory shared by linked worktrees and repository/worktree `lfs.storage` configuration. Relative storage is resolved against the common Git directory. Objects must match the pointer's size and SHA-256 digest; directory-relative `NOFOLLOW` opens prevent symlink escapes within the store. The resolver does not invoke Git LFS or extensions, does not create directories, and never downloads missing content. Global/system storage overrides and LFS reference-object stores are not searched. These are explicit compatibility limits. [Git LFS storage configuration](https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-config.adoc)

## Explicit everyday operations

`RepositoryStatus` separates each path's staged/unstaged state, untracked files, conflicts, current branch, HEAD, upstream counts, and in-progress Git operation. A staged rename retains both paths; `StatusEntry::paths()` supplies both for stage/unstage. `worktree_preview` compares HEAD/index or index/worktree with raw image/text bytes. Worktree file reads use descriptor-relative `NOFOLLOW` traversal; a stored symlink displays its target text. Working previews are snapshots and should be refreshed when another tool changes files.

`WriteCommand` supports selected/all staging and unstaging, a staged-only commit with its message, local checkout, branch creation from HEAD or an explicit ref/OID, fetch, fast-forward-only pull, a normal push to explicit local/remote branch names, and identity updates. Unstaging changes only the index, including on an unborn branch; it does not undo working files. Checkout relies on Git's protection against overwriting local changes and another worktree's checked-out branch. Pull disables rebase and automatic stash; Git refuses divergence or overwriting local changes. Push disables force, mirror and implicit tag expansion, and rejects remotes with multiple push URLs because the visible destination would be ambiguous. It sets the pushed branch's upstream after success.

Real writes preserve normal Git configuration, clean/smudge filters, hooks and signing. A failing hook or signing helper is reported, never bypassed. `GitProfile` reads effective author identity and configured signing. Identity updates write repository config, or the current worktree's private config when the repository already enables `extensions.worktreeConfig`; global identity is never changed. If saving the email fails after the name was saved, the partial outcome is reported.

`GitRepository::init` and `clone_repository` accept only a fresh or empty destination in an existing parent folder. They preserve an occupied destination and do not remove partially created files after failure. Clone does not recurse into submodules. Explicit network operations use configured remotes, Git credentials and SSH configuration with terminal/askpass prompting disabled. Default SSH uses BatchMode and a connection timeout; custom SSH commands and credential/signing helpers remain subject to the operation deadline. Interactive authentication setup is performed outside this client; no credentials are stored by the core. Executable `ext` transport is disabled.

Every write runs once with bounded input/output. A timeout terminates and reaps its process group and reports that local or remote effects may already have happened. There is no automatic retry. The UI must refresh after success **or** failure and let the user inspect the result before another action. Configured hooks and filters are executable user configuration, and their own side effects are not transactional.

Primary references: [porcelain status](https://git-scm.com/docs/git-status), [staged restore](https://git-scm.com/docs/git-restore), [cached removal](https://git-scm.com/docs/git-rm), [commit and hooks](https://git-scm.com/docs/git-commit), [fast-forward-only pull](https://git-scm.com/docs/git-pull), [explicit push refspecs](https://git-scm.com/docs/git-push).

## Budgets and behavior

| Resource | Bound |
| --- | --- |
| Raw object or local LFS bytes | 64 MiB, additionally bounded by the caller for LFS |
| Text diff input | 2 MiB and 100,000 newline-delimited lines per side |
| Text diff algorithm | Patience, with a 250 ms algorithm deadline |
| Passive Git subprocess/object request | 15 seconds per request; timed-out processes are terminated and reaped |
| Explicit local / network operation | 90 / 180 seconds per command; timeout means the result may be partial or uncertain |
| Write input / captured output | 32 MiB path input; 64 KiB commit message; 4 MiB each stdout/stderr, pipes drained concurrently |
| Command output | 128 MiB stdout, 128 KiB stderr |
| Rename candidate limit | 1,000; rename detection is opt-in |

Image callers must separately bound decoded pixels and GPU allocations. A generation check or queue policy belongs to the UI worker; core methods do not cancel already-running calls when selection changes. Multiple calls may be needed for one interaction, so the per-request deadline is not an end-to-end interaction deadline. OS scheduling or uninterruptible filesystem I/O is outside these deadlines' guarantee.

`history_page` reads current refs and can shift if another tool changes them. Reset paging on refresh, or use `history_from_page` with an immutable commit anchor. Remote-tracking branches describe locally available state, with freshness controlled by an explicit fetch in this client or another tool. Missing promisor objects return an explicit local-unavailability error.

## Verification

```sh
cargo test -p gitturtle-core
cargo run --release -p gitturtle-core --example inspect -- /path/to/repository
```

Tests mutate only temporary repositories and local disposable remotes. The everyday workflow fixtures cover staged-only commits with later work preserved, unborn/rename unstaging, literal filenames, raw image/symlink previews, effective and private-worktree identity, hook/filter preservation, signing failure, checkout refusal, merge conflict state, fresh init/clone, fetch, fast-forward pull and non-force push. macOS filesystems reject non-UTF-8 filesystem names; parser/path-input tests preserve these bytes, immutable-tree fixtures cover their Git object representation, and the write fixture creates such files on Linux. The inspection tests cover empty/bare repositories, roots, merge parent selection, topological paging, branches and linked worktrees, non-UTF-8/newline paths constructed directly in Git trees, binary/text/mode/type changes, gitlinks, SHA-256 repositories, partial clones, local LFS integrity and symlink rejection, stalled-process cleanup, and repository-file snapshots with hostile configured helpers.

## Initial measurement, September 7, 2026

Hardware: Apple M4 Max; macOS 26.6.2; Apple Git 2.50.1; Rust 1.98.0; release profile. Data: the locally available `world-of-claudecraft` repository, 5,577 branches and 130 worktrees. Each run opened a new application-side repository handle, read 500 commits, and inspected the first changed file in each of 50 commits. The OS filesystem cache was not flushed; this is not a cold-disk measurement. No application-level content cache was used. No previews were unavailable in these samples.

| Operation | Sample A p50 | Sample A p95 | Sample B p50 | Sample B p95 |
| --- | --- | --- | --- | --- |
| Changed-file list | 13.31 ms | 18.57 ms | 28.06 ms | 66.70 ms |
| First-file text preview | 0.301 ms | 1.44 ms | 0.573 ms | 32.20 ms |
| File list + first text preview | Not recorded | Not recorded | 29.00 ms | 84.24 ms |

Sample A: open 25.06 ms; branch/worktree reads 112.20 ms; 500-commit history 59.72 ms. Sample B ran while the native workspace was compiling: open 92.91 ms; branch/worktree reads 222.28 ms; history 164.22 ms; combined maximum 131.47 ms. System load was not controlled or quantitatively sampled, so these are observations, not a controlled comparison or a performance guarantee.

The harness excludes input dispatch, the UI queue, rendering, image decode, syntax highlighting, and frame presentation. The combined metric covers backend file-list and text-preview work only. Native interaction latency and memory behavior require separate app measurements. Replacing the per-selection `rev-list` subprocess with raw commit reads removed one process startup; later work can investigate the remaining `diff-tree` subprocess only if integrated measurements justify it.
