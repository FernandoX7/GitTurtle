# Git core guidance

These instructions supplement the root agreements for `crates/git-core`. The [service notes](README.md) describe supported behavior, current budgets, and compatibility limits; consult the relevant section when changing a Git operation.

## Find the relevant operation

| Concern | Source | Regression fixtures |
| --- | --- | --- |
| Repository discovery, refs/worktrees, object reads, patches, local LFS | `src/lib.rs` | `tests/repository.rs`, `tests/local_directories.rs` |
| Cancellable search and file history | `src/history.rs` | `tests/history.rs` |
| Committed/working attribution and line history | `src/blame.rs` | `tests/blame.rs` |
| Status, previews, staging, commit, checkout, init/clone, fetch/pull/push | `src/work.rs` | `tests/workflow.rs` |
| Hunk/line staging, index snapshots and publication | `src/work.rs` | `tests/partial_staging.rs` |
| Merge/rebase, conflict resolution, Continue/Abort/Quit | `src/work/integration.rs` | `tests/integration.rs` |
| Interactive linear rebase and native message continuation | `src/work/interactive_rebase.rs` | `tests/interactive_rebase.rs` |
| Conflict block parsing, draft decisions and save-only resolution | `src/conflict_blocks.rs`, `src/work/integration.rs` | module unit tests, `tests/conflict_blocks.rs` |
| Amend/undo/revert/cherry-pick and stash | `src/work/recovery.rs` | `tests/recovery.rs` |
| Branch/upstream management and remote configuration | `src/work/branches.rs` | `tests/branches.rs` |
| Tags and literal ignore rules | `src/work/tags.rs`, `src/work/ignore.rs` | `tests/tags_ignore.rs` |
| Operation cancellation, askpass, redacted diagnostics | `src/work/authentication.rs`, `src/work/diagnostics.rs`, `src/work.rs` | `tests/authentication.rs`, `tests/workflow.rs`, module unit tests |

Process deadlines and byte-input regressions also live in `src/lib.rs` and `src/work.rs` unit tests.

## Command policies

- `git_command` and `bounded_output` in `src/lib.rs` isolate passive history/object reads. Plan preparation, status, configuration inspection, and attribution are also passive even when their code lives under `work`.
- `profile`, `remotes`, attributes, and management plans may inspect effective normal Git configuration. A shared write-policy command helper does not authorize helper execution, mutation, or network access during a read.
- `passive_status_command` disables configured clean/process filters as well as optional index refresh: even status can execute filters on dirty stat entries. Do not replace it with the normal write command builder.
- `normal_command` preserves configured identity, hooks, signing, filters, credentials, and SSH behavior for explicit operations while removing inherited repository/index targeting. Do not reuse the configuration-isolated history command builder for writes or add unsigned/hook-bypassing fallbacks.

## Contracts to preserve

Keep arguments structured and paths byte-safe through porcelain parsing, `StatusEntry::paths`, and literal NUL-delimited path input. Renames can require both paths; unstaging must handle an unborn HEAD and leave working bytes intact. Display text is never an operation target.

The shared `BatchReader` must discard its process after a failed or oversized request so unread protocol bytes cannot corrupt the next response. Preserve object type/size checks and explicit missing-object errors. `history_from_page` pins paging to an immutable OID; `history_page` follows mutable refs. Resolve branch/worktree identities again when refreshing rather than treating a retained tip as current.

Search continuations retain the returned pinned scope and scanned offset; a budget stop with no matches is not exhaustion. File history retains its immutable anchor, exact revision paths and real merge parents along the first-parent lineage. Working blame uses raw working bytes relative to HEAD, including staged and unstaged edits as uncommitted; preserve explicit shallow-history and truncated-result indicators. Cancellation must stop and reap the active read process, not only discard its reply.

Staged previews compare HEAD to the index snapshot; unstaged previews compare the index snapshot to bounded raw working bytes. Working content has no immutable object ID: use side paths to distinguish absence, and report missing or oversized content explicitly. Preserve descriptor-relative no-follow traversal for working files and local LFS objects; symlinks display their stored target text.

`local_lfs_object` accepts only locally present content that matches the pointer's size and SHA-256 digest within both the caller's limit and the core blob limit. Distinguish a missing object from corrupt or unsafe content. LFS storage is mutable, so a missing object is not a permanent property of the pointer's immutable Git blob. Decode limits remain the preview crate's responsibility.

Keep checkout's protection of local changes and other worktrees, fast-forward-only pull without rebase/autostash, and non-force push to one visible branch destination. Identity edits stay in repository config or existing private-worktree config. Init/clone preserve occupied destinations and partial results on failure. Changes to these semantics require a matching user-facing target and behavior, not just a new command flag.

Prepared commands must revalidate the selected repository and the operation's captured refs, index, working bytes, configuration, or reflog before writing. Do not replace a stale plan silently with a new target. Partial staging publishes under the real index lock and preserves unrelated entries; conflict and recovery operations must retain their operation-specific preservation guards. Shared refs/configuration/stashes and private worktree HEAD/index/operation state have different ownership; use `git_directories` to resolve private and common administration directories.

`bounded_write_output` drains bounded stdout/stderr concurrently and includes pipe-holding descendants in its deadline. `OperationControl` supplies explicit cancellation and transient authentication through `run_controlled`; losing a UI reply is not cancellation. Preserve configured credential helpers and SSH, keep headless calls noninteractive, and redact secrets in prompts, progress, and diagnostics. See [authentication behavior](../../docs/authentication.md).

A nonzero exit, cancellation, timeout, output overflow, or lost reply does not prove that nothing changed. Preserve uncertainty in errors; never replay automatically or attempt cleanup that discards partial work. The app's serialized operation executor owns dispatch and refresh after either success or failure. Local Refresh cannot establish a timed-out push's remote outcome; verifying remote refs is a separate explicit network action, or a direct state check in a local-remote fixture.

## Verification

Use disposable repositories and local bare remotes for mutation checks. For the changed operation, assert resulting HEAD/refs, index, and working bytes as relevant, including the unrelated work that must survive. Exercise relevant refusal or partial-result paths; a successful process exit alone does not establish correct Git semantics.

Choose the fixture above with `cargo test --locked -p gitturtle-core --test <fixture> [filter]`; use `cargo test --locked -p gitturtle-core --lib [filter]` for module unit tests. Changes to command builders, parsing, process control, or shared write guards can affect multiple operation families, so cover the relevant callers. Apply the root final checks when integrating Rust changes. Use the performance skill for passive-read or scheduling changes and native QA only when app interactions change. Do not require either skill for every core edit.
