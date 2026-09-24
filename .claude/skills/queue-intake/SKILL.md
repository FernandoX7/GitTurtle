---
name: queue-intake
description: Add or re-cut task contracts in docs/development/tasks.json or a separate queue file under docs/development/ for the unattended controller - single-session slices with three to five acceptance criteria, one-crate scope, profiles and a commit subject - and validate the graph. Planner use only, when the owner asks to queue work.
allowed-tools: Read, Grep, Glob, Edit, Write, Bash(python3 scripts/agent-loop.py validate*)
---

# Queue intake

Task contracts are the controller's acceptance rules; workers cannot edit them, so they must be complete and small before a run starts. Read `docs/development/task.schema.json` and the existing entries in `docs/development/tasks.json` first, and keep an existing task's wording immutable once a run has used it.

A good task fits one fresh session: one observable outcome, a scope of globs no wider than one crate plus its docs and tests, three to five acceptance criteria that each name a check someone can run or evidence someone can inspect, `depends_on` for real ordering only, the profiles that apply (`docs`, `tooling`, `rust`, `native`, `performance`, `package`, `vendor`) so the controller demands the matching attestations, and a Conventional Commit subject. Split a feature that needs more than five criteria or touches two crates into ordered tasks. Performance-sensitive and visible changes carry an explicit criterion for the release-mode measurement or the native evidence.

The queue is `docs/development/tasks.json`, or another file under `docs/development/` that the operator passes to `run --tasks <path>`; a fresh queue for an independent initiative belongs in its own file rather than mixed into the default one. Validate `docs/development/tasks.json` with `python3 scripts/agent-loop.py validate`; validate a separate file with `python3 scripts/agent-loop.py validate --tasks <path>`. Whichever file a run uses, the controller protects it for the whole run: it exports the active path as `GITTURTLE_TASKS_PATH`, and the `protect_paths.py` hook refuses edits to that path during the run regardless of which file it is, so contracts cannot be edited by workers no matter where they live.

Mark a task as hard in its description when you expect earlier attempts to fail, so the operator can route it to the strongest model. After editing, run the validate command for the file you changed and report the task count it prints; a validation error is fixed in the contract, never worked around.
