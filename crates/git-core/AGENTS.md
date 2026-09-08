# Git core guidance

These instructions supplement the root agreements for `crates/git-core`. The [service notes](README.md) describe supported behavior, current budgets, and compatibility limits; consult the relevant section when changing a Git operation.

## Entry points and command policies

- `src/lib.rs`: immutable history/object reads, `git_command`, `bounded_output`, shared object reader, local LFS resolution, and `tests/repository.rs` fixtures.
- `src/work.rs`: `RepositoryStatus`, `ChangeArea`, `worktree_preview`, `WriteCommand`, init/clone, and `tests/workflow.rs` fixtures. Process timeout and byte-input regressions also live in this module's unit tests.
- `profile` and `remotes` read effective normal Git configuration; they are passive reads even though they share write-policy command helpers.
- `passive_status_command` disables configured clean/process filters as well as optional index refresh: even status can execute filters on dirty stat entries. Do not replace it with the normal write command builder.
- `normal_command` preserves configured identity, hooks, signing, filters, credentials, and SSH behavior for explicit operations while removing inherited repository/index targeting. Do not reuse the configuration-isolated history command builder for writes or add unsigned/hook-bypassing fallbacks.

## Contracts to preserve

Keep arguments structured and paths byte-safe through porcelain parsing, `StatusEntry::paths`, and literal NUL-delimited path input. Renames can require both paths; unstaging must handle an unborn HEAD and leave working bytes intact. Display text is never an operation target.

Staged previews compare HEAD to the index snapshot; unstaged previews compare the index snapshot to bounded raw working bytes. Working content has no immutable object ID: use side paths to distinguish absence, and report missing or oversized content explicitly. Preserve descriptor-relative no-follow traversal for working files and local LFS objects; symlinks display their stored target text.

Keep checkout's protection of local changes and other worktrees, fast-forward-only pull without rebase/autostash, and non-force push to one visible branch destination. Identity edits stay in repository config or existing private-worktree config. Init/clone preserve occupied destinations and partial results on failure. Changes to these semantics require a matching user-facing target and behavior, not just a new command flag.

`bounded_write_output` drains bounded stdout/stderr concurrently and includes pipe-holding descendants in its deadline. A nonzero exit, timeout, output overflow, or lost reply does not prove that nothing changed. Preserve uncertainty in errors; never replay automatically or attempt cleanup that discards partial work. The app's serialized operation executor owns dispatch and refresh after either success or failure. Local Refresh cannot establish a timed-out push's remote outcome; verifying remote refs is a separate explicit network action, or a direct state check in a local-remote fixture.

## Verification

Use disposable repositories and local bare remotes for mutation checks. For the changed operation, assert resulting HEAD/refs, index, and working bytes as relevant, including the unrelated work that must survive. Exercise relevant refusal or partial-result paths; a successful process exit alone does not establish correct Git semantics.

Use `cargo test --locked -p gitturtle-core --test repository` for passive compatibility fixtures, `cargo test --locked -p gitturtle-core --test workflow` for everyday operations, or a narrower test filter. Apply the root final checks when integrating Rust changes. Use the performance skill for passive-read or scheduling changes and native QA only when app interactions change. Do not require either skill for every core edit.
