# Git core guidance

These instructions supplement the root agreements for `crates/git-core`. The [service notes](README.md) describe supported behavior, current budgets, and compatibility limits; consult the relevant section when changing a Git operation.

## Find the relevant operation

| Concern | Source | Regression fixtures |
| --- | --- | --- |
| Repository discovery, refs/worktrees, object reads, patches, local LFS | `src/lib.rs` | `tests/repository.rs`, `tests/local_directories.rs` |
| Bounded local-watch inclusion, tracked paths and ignore rules | `src/local_watch.rs` | `tests/local_directories.rs` |
| Captured document local-image paths, revision/index/worktree assets | `src/preview_assets.rs` | `tests/preview_assets.rs` |
| Streaming history, cancellable search and file history | `src/history.rs` | `tests/history.rs`, module unit tests |
| Revision comparisons and tracked-path search/quick open | `src/inspection.rs` | `tests/inspection.rs` |
| Passive process pipes, deadlines and cleanup | `src/process_io.rs`, `src/lib.rs`, `src/history.rs` | `src/lib.rs` and `src/history.rs` unit tests |
| Committed/working attribution and line history | `src/blame.rs` | `tests/blame.rs` |
| Status, previews, staging, commit, checkout, init/clone, fetch/pull/push | `src/work.rs` | `tests/workflow.rs` |
| Hunk/line staging, index snapshots and publication | `src/work.rs` | `tests/partial_staging.rs` |
| Merge/rebase, conflict resolution, Continue/Abort/Quit | `src/work/integration.rs` | `tests/integration.rs` |
| Interactive linear rebase and native message continuation | `src/work/interactive_rebase.rs` | `tests/interactive_rebase.rs` |
| Rewritten-series review and exact-lease publication | `src/work/rewrite_review.rs` | `tests/rewrite_review.rs`, module unit tests |
| Conflict block parsing, draft decisions and save-only resolution | `src/conflict_blocks.rs`, `src/work/integration.rs` | module unit tests, `tests/conflict_blocks.rs` |
| Amend/undo/revert/cherry-pick and stash | `src/work/recovery.rs` | `tests/recovery.rs` |
| Branch/upstream management and remote configuration | `src/work/branches.rs` | `tests/branches.rs` |
| Worktree creation/removal, reflog inspection and branch recovery | `src/work/worktrees.rs`, `src/work/reflog.rs` | `tests/worktrees_reflog.rs` |
| Named profile application, atomic identity and signing configuration | `src/work/profiles.rs` | `tests/profiles.rs`, `tests/signing.rs` |
| Explicit one-object LFS download and resolved text | `src/work/lfs_download.rs` | `tests/lfs_download.rs` |
| Tags and literal ignore rules | `src/work/tags.rs`, `src/work/ignore.rs` | `tests/tags_ignore.rs` |
| Operation cancellation, askpass, signing and redacted diagnostics | `src/work/authentication.rs`, `src/work/diagnostics.rs`, `src/work.rs` | `tests/authentication.rs`, `tests/signing.rs`, `tests/workflow.rs`, module unit tests |

Process deadlines and byte-input regressions also live in `src/lib.rs` and `src/work.rs` unit tests.

## Extending an operation

Keep an operation's preparation, captured plan, validation and execution together in its owning module. The existing `work` submodules provide this boundary for reviewed writes; expose their owned plans/results through core and route execution through `WriteCommand`. App navigation, queue scheduling and native types stay with the [app consumer](../app/AGENTS.md). A new presentation surface should reuse the same core operation and preservation guards.

Keep operation-specific ref, configuration and path guards beside that operation. Extract shared helpers when callers have the same semantics; passive reads and explicit writes must retain their different command policies. Changes to a shared helper need a review of its actual callers, including refusal and uncertain-outcome behavior.

## Command policies

- `git_command` and `bounded_output` in `src/lib.rs` isolate passive history/object reads. Plan preparation, status, configuration inspection, and attribution are also passive even when their code lives under `work`.
- `profile`, `remotes`, attributes, and management plans may inspect effective normal Git configuration. A shared write-policy command helper does not authorize helper execution, mutation, or network access during a read.
- `passive_status_command` disables configured clean/process filters as well as optional index refresh: even status can execute filters on dirty stat entries. Do not replace it with the normal write command builder.
- `normal_command` preserves configured identity, hooks, signing, filters, credentials, and SSH behavior for explicit operations while removing inherited repository/index targeting. Do not reuse the configuration-isolated history command builder for writes or add unsigned/hook-bypassing fallbacks.

## Contracts to preserve

Keep arguments structured and paths byte-safe through porcelain parsing, `StatusEntry::paths`, and literal NUL-delimited path input. Renames can require both paths; unstaging must handle an unborn HEAD and leave working bytes intact. Display text is never an operation target.

The shared `BatchReader` must discard its process after a failed or oversized request so unread protocol bytes cannot corrupt the next response. Preserve object type/size checks and explicit missing-object errors. Passive process cleanup must stop and join owned pipe threads even when a detached descendant holds a pipe; direct-child exit is not completion. Preserve nonblocking readiness and cancellation wakeups in `process_io`, including idle backpressure.

Ordinary history uses `history_traversal` and `HistoryTraversal::next_page`: one process walks captured tips without growing-prefix replay. Preserve bounded read-ahead and metadata accounting. A failed/cancelled traversal is invalidated and must visibly restart; dropping it closes its producer. `history_from_page` and `history_page` remain stateless compatibility APIs, with pinned and mutable scopes respectively. Refresh creates a new traversal and resolves branch/worktree identities again.

Search continuations retain the returned pinned scope and scanned offset; a budget stop with no matches is not exhaustion. File history retains its immutable anchor, exact revision paths and real merge parents along the first-parent lineage. Working blame uses raw working bytes relative to HEAD, including staged and unstaged edits as uncommitted; preserve explicit shallow-history and truncated-result indicators. Cancellation must stop and reap the active read process, not only discard its reply.

Revision comparison pins both resolved commits and keeps endpoint comparison distinct from changes since one unambiguous merge base. Tracked-path search retains raw paths, explicit truncation and its returned revision scope; working scope includes tracked deletions and conflicts. Missing objects never permit substituting current working content.

`LocalWatchPolicy` supplies bounded inclusion rules from the selected index, nested ignore files and effective shared/global excludes. Preserve tracked paths beneath ignored directories, lazy ignore invalidation and cache accounting when directories disappear. The app owns watch subscriptions, coalescing and refresh scheduling; watch coverage limits must remain visible rather than silently implying complete coverage. Local events authorize passive reads only.

Staged previews compare HEAD to the index snapshot; unstaged previews compare the index snapshot to bounded raw working bytes. Working content has no immutable object ID: use side paths to distinguish absence, and report missing or oversized content explicitly. Preserve descriptor-relative no-follow traversal for working files and local LFS objects; symlinks display their stored target text.

`local_lfs_object` accepts only locally present content that matches the pointer's size and SHA-256 digest within both the caller's limit and the core blob limit. Distinguish a missing object from corrupt or unsafe content. LFS storage is mutable, so a missing object is not a permanent property of the pointer's immutable Git blob. Decode limits remain the preview crate's responsibility.

An [explicit LFS download](../../docs/lfs-previews.md) captures the pointer, repository, endpoint/configuration and storage, then transfers only the selected object without checkout or index changes. Plan preparation redirects `git lfs env` storage into a private temporary directory because that inspection creates directories. Working pointers use private temporary Git objects; preserve post-transfer size/digest verification and disabled transfer retries.

Keep checkout's protection of local changes and other worktrees, fast-forward-only pull without rebase/autostash, and ordinary non-force push to one visible branch destination. [Profile and identity edits](../../docs/profiles.md) publish all keys atomically under the configuration lock, stay in repository config or existing private-worktree config, and preserve inherited signing requirements. Init/clone preserve occupied destinations and partial results on failure. Changes to these semantics require a matching user-facing target and behavior, not just a new command flag.

Prepared commands must revalidate the selected repository and the operation's captured refs, index, working bytes, configuration, or reflog before writing. Do not replace a stale plan silently with a new target. Partial staging publishes under the real index lock and preserves unrelated entries; conflict and recovery operations must retain their operation-specific preservation guards. Shared refs/configuration/stashes and private worktree HEAD/index/operation state have different ownership; use `git_directories` to resolve private and common administration directories.

Worktree details inspect only the selected worktree. Creation rechecks destination and branch occupancy; ordinary removal refuses changed, untracked, ignored, locked, main, current or missing worktrees and never forces cleanup. Explicit force removal passes one `--force` to delete dirty content and unfinished operation state; it still refuses locked, main, current or missing worktrees, held lock files, initialized submodules and nested repositories, and never passes the second force that overrides a worktree lock. Reflog recovery revalidates the captured entry and creates a new branch without checkout/reset; missing objects and expired entries remain explicit.

[Rewritten-series review](../../docs/rewritten-series.md) is local and bounded; preserve ambiguous correspondence and labelled subject-only fallback. Publication inspection is a separate explicit network action. `PublishRewrite` targets one captured push URL/ref/OID with an exact expected remote OID lease; remote movement requires a new series review. Never broaden the lease or silently fall back to force.

`bounded_write_output` drains bounded stdout/stderr concurrently and includes pipe-holding descendants in its deadline. `OperationControl` supplies explicit cancellation and transient authentication through `run_controlled`; losing a UI reply is not cancellation. Preserve configured credential helpers and SSH, keep headless calls noninteractive, and redact secrets in prompts, progress, and diagnostics. See [authentication behavior](../../docs/authentication.md).

A nonzero exit, cancellation, timeout, output overflow, or lost reply does not prove that nothing changed. Preserve uncertainty in errors; never replay automatically or attempt cleanup that discards partial work. The app's serialized operation executor owns dispatch and refresh after either success or failure. Local Refresh cannot establish a timed-out push's remote outcome; verifying remote refs is a separate explicit network action, or a direct state check in a local-remote fixture.

## Verification

Use disposable repositories and local bare remotes for mutation checks. For the changed operation, assert resulting HEAD/refs, index, and working bytes as relevant, including the unrelated work that must survive. Exercise relevant refusal or partial-result paths; a successful process exit alone does not establish correct Git semantics.

Choose the fixture above with `cargo test --locked -p gitturtle-core --test <fixture> [filter]`; use `cargo test --locked -p gitturtle-core --lib [filter]` for module unit tests. Changes to command builders, parsing, process control, or shared write guards can affect multiple operation families, so cover the relevant callers. Apply the root final checks when integrating Rust changes. Use the performance skill for passive-read or scheduling changes and native QA only when app interactions change. Do not require either skill for every core edit.
