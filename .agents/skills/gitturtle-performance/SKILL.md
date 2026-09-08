---
name: gitturtle-performance
description: Review or improve GitTurtle's commit browsing latency, read-only Git operations, scheduling, caches, and content previews. Apply to this project's hot paths, not unrelated Rust changes.
---

# GitTurtle performance and read-only behavior

Use this workflow for the affected hot path. Start with the relevant code and existing evidence in [architecture](../../../docs/architecture.md) and [validation](../../../docs/validation.md); do not turn an ordinary change into an exhaustive audit.

Trace an interaction from selection through metadata, file list, selected-file content, and presentation. Preserve that order; expensive rename detection, aggregate statistics, syntax parsing, or unrelated blobs must not gate the first useful view.

In History, finish the changed-file list without eagerly preparing a preview. Enter Compare only on file activation. Back uses retained history and must not wait for an active preview; a late result may populate content but cannot switch modes or build an editor while hidden. Measure commit-file-list and file-preview frames separately.

Keep logical selection separate from rendered rows. Virtualizing a list must not break offscreen selection/copy, keyboard navigation, or graph continuity. Keep graph topology, patch gutter/decorations, and render-image pixel preparation on the worker; editor constructors consume prepared results. See [app contracts](../../../crates/app/AGENTS.md) for the relevant symbols.

Inspect `worker::PreviewKey` and `Content::bytes` when changing previews. Count retained buffers, patch metadata, and decoded CPU images. Separately inspect UI-held references, editor copies, and GPU texture lifetimes: the 32-entry/128 MiB cache is not a process-memory cap. Compressed image size is not its allocation cost.

Review `Worker::submit`, `Cancellation`, and `execute` at the worker boundary. A generation check only discards stale output; verify queued work is replaced or cancelled and concurrency remains bounded. Checkpoints are cooperative, not immediate preemption of a decoder or blocking read. Cache keys need repository identity, both object IDs, byte-safe paths, modes, and relevant comparison/rendering options. `RepositorySession` may reuse the canonical worktree's object reader while Refresh rereads mutable state. Keep linked worktrees distinct even when their objects are shared. Missing local LFS content must remain retryable.

Preserve one active request and one replaceable pending request, Git read deadlines, and worker panic recovery. Existing regression coverage lives in [worker tests](../../../crates/app/src/worker.rs), [Git fixtures](../../../crates/git-core/tests/repository.rs), and [decoder tests](../../../crates/preview/src/lib.rs); select the cases relevant to the change.

Verify read-only behavior with disposable Git fixtures that exercise roots, merges, byte-safe paths, worktrees, LFS/missing objects and configured helpers as relevant. No fixture command may target a user's repository. Read-only command flags are additional controls, not a complete proof; inspect actual operations and compare fixture state when changing the Git layer.

For performance claims, use release builds and repeatable navigation over the same immutable commit/file data. Record build revision, hardware, sample count, raw samples, median/tail/max, other load, and application versus filesystem cache conditions. Retain outliers; missing or superseded samples are not zero latency. Inspect resource growth under rapid selection when the change affects allocation or cancellation.

`gitturtle.commit_files_frame_ms` measures selection handling through the changed-file-list frame callback; `gitturtle.file_preview_frame_ms` measures activation handling through the prepared-preview callback. Neither includes pre-handler input delivery, OS presentation, or completed GPU execution. The status bar's content-read duration is worker work only. Historical `selection_frame_ms` includes eager previews and is not directly comparable. Save new records under `docs/benchmarks/` without rewriting previous measurements or attributing them to newer builds.

Prefer a narrow measured correction over another engine, persistent database, or broad cache without evidence. Once the affected behavior and required checks pass, report the result and concrete limits; repeat or broaden only for a new change, failure, or unresolved concern.
