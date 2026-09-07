---
name: gitturtle-performance
description: Review or improve GitTurtle's commit browsing latency, read-only Git operations, scheduling, caches, and content previews. Apply to this project's hot paths, not unrelated Rust changes.
---

# GitTurtle performance and read-only behavior

Trace an interaction from selection through metadata, file list, selected-file content, and presentation. Preserve that order; expensive rename detection, aggregate statistics, syntax parsing, or unrelated blobs must not gate the first useful view.

In History, finish the changed-file list without eagerly preparing a preview. Enter Compare only on file activation. Back uses retained history and must not wait for an active preview; a late result may populate content but cannot switch modes or build an editor while hidden. Measure commit-file-list and file-preview frames separately.

Keep logical selection separate from rendered rows. Virtualizing a list must not break offscreen selection/copy, keyboard navigation, or graph continuity. Image budgets must count inflated pixels, CPU copies, and GPU textures rather than compressed file sizes.

Review cancellation at the worker boundary. A generation check only discards stale output; verify queued work is replaced or cancelled and concurrency remains bounded. Cache keys need repository identity, both object IDs and relevant comparison/rendering options. Keep mutable ref/worktree state separate from immutable content.

Verify read-only behavior with disposable Git fixtures that exercise roots, merges, byte-safe paths, worktrees, LFS/missing objects and configured helpers as relevant. No fixture command may target a user's repository. Read-only command flags are additional controls, not a complete proof; inspect actual operations and compare fixture state when changing the Git layer.

For performance claims, use release builds and repeatable navigation over the same data. Separate cold application/cache conditions, record time to first useful result and tail latency, and inspect resource growth under rapid selection. Prefer a narrow measured correction over adding another engine, persistent database, or broad cache without evidence.
