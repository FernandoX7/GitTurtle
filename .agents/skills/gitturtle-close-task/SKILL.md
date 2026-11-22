---
name: gitturtle-close-task
description: Close out a GitTurtle task cleanly — confirm scope, run gates and doc-sync checks, record evidence, overwrite STATE.md (<= 60 lines), and create atomic Conventional Commits with zero AI attribution. Use before opening a PR or ending a work session.
---

# Close a task

Complete these checks in order before opening a PR or handing off:

1. **Scope:** Exactly one cohesive concern per commit/PR. Split unrelated work or revert accidental edits; never edit policy files or weaken tests to force green.
2. **Gates:** Run `python3 scripts/gate.py fast` (and `full` for shared-interface, persistence, scheduling, platform, or dependency changes), `python3 scripts/check-agent-guidance.py`, `typos --format brief`, and `python3 scripts/check-docs-sync.py --changed-since origin/main` (when present). Fold required README/docs/website updates into the same PR rather than opening a separate follow-up docs PR.
3. **Evidence:** Tie every acceptance criterion to the exact command and observed result. Report unexercised platforms or pending desktop QA explicitly; never claim unrun benchmarks or checks passed, and never commit `"pending"` placeholders in `docs/validation.md`.
4. **State (`docs/development/STATE.md`):** Overwrite (never append!) [STATE.md](../../../docs/development/STATE.md) within the enforced **60-line cap**: current branch/PR, latest green `main` commit, active blockers, next 3 concrete tasks, and any open manual desktop checks. Graduate durable lessons into `.agents/skills/` or `docs/decisions/` instead of accumulating session logs.
5. **Atomic Conventional Commit:** Stage explicit intended paths, inspect `git diff --cached`, and commit with a Conventional Commit message (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`) explaining *why*. **Never include AI mentions or `Co-Authored-By` AI trailers** in commits, PR titles, PR bodies, or docs. Never bypass hooks with `--no-verify` or amend published history.
6. **Worktree cleanup:** After a PR is squash-merged and `main` is updated, remove the worktree and its `.local/target` immediately via `python3 scripts/dev/worktree.py rm <name> --delete-branch`. If stopping partially, leave the working tree green and record the exact resume point in `STATE.md`.
