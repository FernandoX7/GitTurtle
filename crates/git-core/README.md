# GitTurtle Git service

Blocking Rust APIs intended for bounded background workers. Immutable history/object inspection is separate from the explicit working-copy API in `src/work.rs`. Passive reads never mutate repositories or start network requests. The app submits writes to its serialized operation worker only after a user action.

- Local and remote-tracking branches, including the current branch of the opened worktree.
- Worktrees with byte-preserving paths and detached, locked, and prunable states.
- Git topological history with parents, messages, paging, and an optional fixed commit anchor.
- Literal repository-wide message, author-name and hash search with immutable scopes, bounded continuation and active process cancellation.
- File history follows renames along the first-parent lineage and retains exact before/after paths, blob IDs, deletions and actual merge parents.
- Root and selected-merge-parent comparisons, mode/type changes, and optional bounded rename detection.
- A shared persistent `git cat-file --batch-command` service for commit parents, object size queries, and raw file bytes.
- Unified text patches plus retained source bytes for split views, explicit binary/oversize/submodule states, and local LFS resolution.

For immutable inspection, Git arguments are fixed and supplied directly to `Command`. User/system Git configuration is excluded. Automatic lazy fetch, credentials, protocols, filesystem-monitor helpers, automatic maintenance, optional locks, and replacement objects are disabled. Tracked symlinks are read as Git blobs, never followed into the worktree. `git config` reads local LFS storage configuration. Passive status additionally enumerates filter keys to disable configured clean/process drivers: Git status can otherwise execute them when checking dirty stat entries. Status uses porcelain v2 with NUL-delimited byte-safe paths and disables optional index refresh locks. It reports submodule commit changes but leaves dirty file inspection inside each submodule to opening that repository. Filtered files use raw working bytes for inspection; this can conservatively report a change that an external clean filter would normalize away.

`search_history` matches literal Unicode-case-insensitive substrings in commit subject, description, author name and full hash. `HistoryScope::AllRefs` captures local reference tips and detached HEAD once; every continuation must use the returned `PinnedRefs` scope and `next_offset`. `FromCommit` searches one immutable ancestry. Refresh explicitly restarts the scope. A search page reports scanned commits and its stop reason: only `Exhausted` means there are no more matches to find. Scan, byte and time limits retain a continuation, including a retry cursor when no progress was possible. Cancellation signals the active Git process group and reaps pipe-holding descendants; it does not merely discard a late response.

`file_history` accepts an immutable full commit ID and one literal byte-safe repository-relative path. It uses bounded `--follow` rename detection on the first-parent lineage, so a merge represents changes introduced from side branches. Each row retains the real first parent plus all commit parents for other comparisons, and exact revision paths rather than the current filename. This explicit scope avoids presenting Git's nonlinear `--follow` limitations as complete ancestry. Pages retain the anchor and original path. Each page reads the prefix through its requested offset before slicing: using `--skip` with `--follow` can skip the rename that establishes an older path. The prefix remains bounded by 32 MiB and 15 seconds; a deep-page limit returns an actionable error to use an older revision as the anchor, not a false end of history. [Git history and rename-following semantics](https://git-scm.com/docs/git-log)

Local LFS lookup supports the common Git directory shared by linked worktrees and repository/worktree `lfs.storage` configuration. Relative storage is resolved against the common Git directory. Objects must match the pointer's size and SHA-256 digest; directory-relative `NOFOLLOW` opens prevent symlink escapes within the store. The resolver does not invoke Git LFS or extensions, does not create directories, and never downloads missing content. Global/system storage overrides and LFS reference-object stores are not searched. These are explicit compatibility limits. [Git LFS storage configuration](https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-config.adoc)

## Explicit everyday operations

`RepositoryStatus` separates each path's staged/unstaged state, untracked files, conflicts, current branch, HEAD, upstream counts, and in-progress Git operation. A staged rename retains both paths; `StatusEntry::paths()` supplies both for stage/unstage. `worktree_preview` compares HEAD/index or index/worktree with raw image/text bytes. Worktree file reads use descriptor-relative `NOFOLLOW` traversal; a stored symlink displays its target text. Working previews are snapshots and should be refreshed when another tool changes files.

`WriteCommand` supports selected/all staging and unstaging, a staged-only commit with its message, local checkout, branch creation from HEAD or an explicit ref/OID, fetch, fast-forward-only pull, a normal push to explicit local/remote branch names, and identity updates. Unstaging changes only the index, including on an unborn branch; it does not undo working files. Checkout relies on Git's protection against overwriting local changes and another worktree's checked-out branch. Pull disables rebase and automatic stash; Git refuses divergence or overwriting local changes. Push disables force, mirror and implicit tag expansion, and rejects remotes with multiple push URLs because the visible destination would be ambiguous. It sets the pushed branch's upstream after success.

Real writes preserve normal Git configuration, clean/smudge filters, hooks and signing. A failing hook or signing helper is reported, never bypassed. `GitProfile` reads effective author identity and configured signing. Identity updates write repository config, or the current worktree's private config when the repository already enables `extensions.worktreeConfig`; global identity is never changed. If saving the email fails after the name was saved, the partial outcome is reported.

Text working previews can include a `PartialDiff` snapshot for explicit hunk or changed-line staging and unstaging. `ApplyPartial` validates the selected repository, HEAD, attributes, index entry and working bytes under the real index lock, then atomically publishes a copied index. Unrelated index entries and all working files are preserved. Binary, oversized, filtered, normalized, renamed and mode/type-changing files retain whole-file operations. A selection that would ambiguously join unterminated lines is refused; select the complete replacement or hunk. Commit messages use verbatim cleanup to preserve description whitespace and comment lines.

`integration_plan` resolves and pins a local branch, remote-tracking branch, full commit ID, or configured upstream. Visible names resolve to precise full refs and refuse ambiguity; `upstream_integration_plan` resolves the current configured relationship. Explicit merge and rebase refuse stale plans and never autostash. Rebase also checks ignored paths against the target and each replayed commit before starting, including blocking parent paths. `operation_state` detects external merge, rebase, cherry-pick and revert operations; continuation snapshots include staged paths and the index identity. `same_operation` compares the operation without its changing index, allowing callers to retain conflict drafts across unrelated staging.

`conflict_preview` returns the base and both complete sides with branch-aware labels, including the reversed roles during rebase. Resolution can choose a side (including an absent side), save bounded manual text, or stage an externally edited file. Reads and manual writes never follow a stored symlink. Each resolution validates its raw source snapshot before using normal configured staging. Continue respects hooks/signing; bounded interactive-rebase state fingerprints detect pending message-editor steps that must finish externally. Merge/cherry-pick/revert Abort preserves disjoint unstaged and untracked work but refuses independent staged changes it cannot preserve. Rebase Abort uses a stricter dirty-work guard because Git may reset the checkout. Missing resolution provenance is treated conservatively. Quit keeps HEAD, index and working files while ending the operation.

`BranchCommand` uses prepared snapshots for tracking-branch creation, rename, safe deletion and upstream changes. Plans validate branch tips, configuration and linked-worktree occupancy again before writing. `rename_branch_plan` also validates the proposed name and destination namespace before a caller presents its review. `remote_configs` retains every URL, push URL and fetch refspec. Remote edits publish shared repository configuration atomically while preserving unrelated options; removal reports affected tracking relationships and preserves refs shared with another remote. Configuration inherited from another source must be changed at that source. These management operations are local and do not fetch.

`RecoveryCommand` uses prepared snapshots for amend, undo, revert, cherry-pick, and stash actions. `recovery_plan` captures the named local branch, HEAD, raw index identity, target commit and affected paths; execution refuses a stale or cross-repository plan. Amend preserves the exact UTF-8 message with verbatim cleanup and consumes the reviewed staged state while keeping later unstaged edits. Non-UTF-8 commit messages require an external tool to preserve them. Undo accepts only the current named local tip with one parent and uses a soft reset, preserving the exact index and working files. Root/merge commits and tips contained in known remote-tracking refs are refused, including containment learned after planning. This is local evidence of publication, not a guarantee about unfetched remotes. Revert/cherry-pick require an explicit one-based mainline for merge commits, preserve disjoint unstaged work, and protect tracked, untracked, ignored, and blocking ancestor paths before starting. Conflicts use the same integration inspection and Continue/Abort APIs.

`stash_create_plan` captures the included tracked/index work and optional untracked files; raw bytes, modes, index and path-list changes invalidate the plan. Ignored files remain outside the saved work. `stash_list` returns bounded pages whose entries carry a full bounded reflog identity. `stash_snapshot` compares separate base, index, worktree, and optional untracked trees. Apply validates the selected entry, optionally restores staged state, and always keeps the stash on success, conflict, or failure. Drop is a separate explicit command; changes anywhere in the captured stash reflog invalidate it, even when another entry has the same object ID, name, and timestamp. Shared stash storage does not change the selected worktree for create or apply.

`GitRepository::init` and `clone_repository` accept only a fresh or empty destination in an existing parent folder. They preserve an occupied destination and do not remove partially created files after failure. Clone does not recurse into submodules. Explicit network operations use configured remotes, Git credentials and SSH configuration with terminal/askpass prompting disabled. Default SSH uses BatchMode and a connection timeout; custom SSH commands and credential/signing helpers remain subject to the operation deadline. Interactive authentication setup is performed outside this client; no credentials are stored by the core. Executable `ext` transport is disabled.

Every write runs once with bounded input/output. A timeout terminates and reaps its process group and reports that local or remote effects may already have happened. There is no automatic retry. The UI must refresh after success **or** failure and let the user inspect the result before another action. Configured hooks and filters are executable user configuration, and their own side effects are not transactional.

Failure reports preserve Git's output and supplement recognized SSH, host verification, credential-helper, authentication, signing, conflict and divergence failures with next steps. Generic commit failures direct users to inspect configured hooks, identity and signing without asserting which failed. Guidance never changes configuration or disables verification. Disposable configured-SSH and local HTTP challenge fixtures cover this reporting; they do not verify a real hosting provider's sign-in flow.

Primary references: [porcelain status](https://git-scm.com/docs/git-status), [staged restore](https://git-scm.com/docs/git-restore), [cached removal](https://git-scm.com/docs/git-rm), [commit and hooks](https://git-scm.com/docs/git-commit), [fast-forward-only pull](https://git-scm.com/docs/git-pull), [explicit push refspecs](https://git-scm.com/docs/git-push).

## Budgets and behavior

| Resource | Bound |
| --- | --- |
| Raw object or local LFS bytes | 64 MiB, additionally bounded by the caller for LFS |
| Text diff input | 2 MiB and 100,000 newline-delimited lines per side |
| Text diff algorithm | Patience, with a 250 ms algorithm deadline |
| Passive Git subprocess/object request | 15 seconds per request; timed-out processes are terminated and reaped |
| History search | 1–500 results per page; 4 KiB query; 50,000 scanned commits, 64 MiB output or 15 seconds per call |
| Search reference snapshot | 16,384 tips and approximately 1 MiB; continued pages use the captured tips |
| History metadata | 2 MiB per commit; file-history pages additionally cap output at 32 MiB |
| File-history input / page | 16 KiB literal relative path; 1–500 rows; bounded prefix replay retains rename lineage |
| Conflict content / manual resolution | 64 MiB per raw side; 2 MiB UTF-8 manual input; interactive-rebase metadata up to 1 MiB per file |
| Rebase collision preflight | 32 MiB path output, 100,000 candidate paths, 15-second filesystem inspection deadline |
| Stash enumeration | 1–500 entries per page; complete reflog snapshot up to 16 MiB or 50,000 records |
| Stash-create raw work snapshot | 64 MiB per file and 256 MiB in total |
| Explicit local / network operation | 90 / 180 seconds per command; timeout means the result may be partial or uncertain |
| Write input / captured output | 32 MiB path input; 64 KiB commit message; 4 MiB each stdout/stderr, pipes drained concurrently |
| Command output | 128 MiB stdout, 128 KiB stderr |
| Rename candidate limit | 1,000; the app requests detection for immutable changed-file lists and file history |

Image callers must separately bound decoded pixels and GPU allocations. A generation check or queue policy belongs to the UI worker. Search and file-history calls accept `HistoryCancellation`; other core reads retain their individual deadlines rather than interrupting an already-running call on selection changes. Multiple calls may be needed for one interaction, so the per-request deadline is not an end-to-end interaction deadline. OS scheduling or uninterruptible filesystem I/O is outside these deadlines' guarantee.

`history_page` reads current refs and can shift if another tool changes them. Reset paging on refresh, or use `history_from_page` with an immutable commit anchor. Remote-tracking branches describe locally available state, with freshness controlled by an explicit fetch in this client or another tool. Missing promisor objects return an explicit local-unavailability error.

## Verification

```sh
cargo test --locked -p gitturtle-core
cargo run --locked --release -p gitturtle-core --example inspect -- /path/to/repository
```

Tests mutate only temporary repositories and local disposable remotes. The everyday workflow fixtures cover staged-only commits with later work preserved, unborn/rename unstaging, literal filenames, raw image/symlink previews, effective and private-worktree identity, hook/filter preservation, signing failure, checkout refusal, merge conflict state, fresh init/clone, fetch, fast-forward pull and non-force push. macOS filesystems reject non-UTF-8 filesystem names; parser/path-input tests preserve these bytes, immutable-tree fixtures cover their Git object representation, and the write fixture creates such files on Linux. The inspection tests cover empty/bare repositories, roots, merge parent selection, topological paging, branches and linked worktrees, non-UTF-8/newline paths constructed directly in Git trees, binary/text/mode/type changes, gitlinks, SHA-256 repositories, partial clones, local LFS integrity and symlink rejection, stalled-process cleanup, and repository-file snapshots with hostile configured helpers.

Dedicated partial-staging, integration and branch-management fixtures cover stale snapshots, mixed staged/unstaged work, missing final newlines, filtered and SHA-256 indexes, linked worktrees, manual and binary conflicts, external sequencers and pending message edits, abort preservation, ignored-path collisions, invalid rename destinations, tracking relationships, atomic remote edits and inherited configuration.

History fixtures cover matches beyond loaded pages, literal Unicode/hash search, immutable continuation after refs move, scan limits, unborn/detached HEAD, annotated tags including non-commit targets, rename boundaries across pages, deletion and merge paths, byte-safe filenames, and active process cleanup. Recovery fixtures cover exact amend messages and index/worktree preservation, hooks/signing failures, published/stale undo refusal, merge-parent choices, conflict Continue/Abort, ignored blocking paths, stash inspection/apply/drop and duplicate reflog identities, and linked-worktree isolation. These backend fixtures do not establish native interaction or platform coverage.

## Initial measurement, September 7, 2026

Hardware: Apple M4 Max; macOS 26.6.2; Apple Git 2.50.1; Rust 1.98.0; release profile. Data: the locally available `world-of-claudecraft` repository, 5,577 branches and 130 worktrees. Each run opened a new application-side repository handle, read 500 commits, and inspected the first changed file in each of 50 commits. The OS filesystem cache was not flushed; this is not a cold-disk measurement. No application-level content cache was used. No previews were unavailable in these samples.

| Operation | Sample A p50 | Sample A p95 | Sample B p50 | Sample B p95 |
| --- | --- | --- | --- | --- |
| Changed-file list | 13.31 ms | 18.57 ms | 28.06 ms | 66.70 ms |
| First-file text preview | 0.301 ms | 1.44 ms | 0.573 ms | 32.20 ms |
| File list + first text preview | Not recorded | Not recorded | 29.00 ms | 84.24 ms |

Sample A: open 25.06 ms; branch/worktree reads 112.20 ms; 500-commit history 59.72 ms. Sample B ran while the native workspace was compiling: open 92.91 ms; branch/worktree reads 222.28 ms; history 164.22 ms; combined maximum 131.47 ms. System load was not controlled or quantitatively sampled, so these are observations, not a controlled comparison or a performance guarantee.

The harness excludes input dispatch, the UI queue, rendering, image decode, syntax highlighting, and frame presentation. The combined metric covers backend file-list and text-preview work only. Native interaction latency and memory behavior require separate app measurements. Replacing the per-selection `rev-list` subprocess with raw commit reads removed one process startup; later work can investigate the remaining `diff-tree` subprocess only if integrated measurements justify it.
