---
name: gitturtle-feature-work
description: Complete a coordinated GitTurtle feature or multi-step engineering task with explicit acceptance criteria, independent review, and clean handoff. Use for feature work and multi-file changes, not isolated copy edits.
---

# GitTurtle feature work

Read the task scope, [AGENTS.md](../../../AGENTS.md), and the affected crate or vendor guide before editing; do not load unrelated subsystems by default.

## Establish the acceptance boundary

- Define the observable behavior and choose the matching validation profile (core fixtures, GPUI tests, release micro-benchmarks, native QA, or package checks).
- For uncertain designs or new dependencies, verify primary sources with [gitturtle-research](../gitturtle-research/SKILL.md) and [research-template.md](../../../docs/development/research-template.md).
- For shared interfaces, retained state, persistence, scheduling, platform, or dependency changes, apply [architecture-review.md](../../../docs/development/architecture-review.md): identify the existing owner, trace producer and consumer together, and cover failure/recovery paths.
- Before copying behavior from another view or module, verify that the source actually implements the expected behavior.

## Implement, delegate & verify

- Create an isolated worktree with its own CoW-cloned `CARGO_TARGET_DIR` via `python3 scripts/dev/worktree.py new <name> [base]`.
- Delegate independent read-only investigations, reviews, or disjoint edits with explicit file ownership and validation expectations; the coordinator integrates shared edits and owns the Git index, commits, and final combined checks. Always inherit the session's model and effort unless the user requests otherwise.
- Run focused checks while iterating (`python3 scripts/gate.py fast`), then run independent reviews using the [review-candidate workflow](../../workflows/review-candidate.md) (`code-reviewer`, plus `security-reviewer` / `design-reviewer` / `performance-reviewer` where affected) before opening a PR.
- Use [gitturtle-performance](../gitturtle-performance/SKILL.md) for hot paths and [gitturtle-native-qa](../gitturtle-native-qa/SKILL.md) for UI/package changes. Never claim an unrun benchmark or unexercised platform passed, and never weaken tests or policy files to pass a gate.

## Close out cleanly

Follow [gitturtle-close-task](../gitturtle-close-task/SKILL.md): fold required doc/README updates into the same atomic PR, overwrite `docs/development/STATE.md` within its 60-line cap, commit with Conventional Commits and zero AI attribution, wait for green CI before squash-merging, and drop the worktree target once merged.
