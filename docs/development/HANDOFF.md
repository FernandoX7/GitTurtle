# Handoff

Keep the human section under 15 lines: what changed last, what is next, the exact commands that work, and known issues. Interactive sessions rewrite it when closing a task (see the `close-task` skill). The unattended controller writes its own generated handoff into the run directory and never edits this file.

- Last change: Claude Code support added beside the existing Codex configuration (`CLAUDE.md`, `.claude/`, `scripts/gate.py`, the `--tool claude` controller adapter).
- Next: pick the next ready task from `docs/development/tasks.json`, or run the planner to re-cut the queue into single-session tasks.
- Commands: `python3 scripts/gate.py fast` (changed crates, under three minutes warm), `python3 scripts/gate.py full` (workspace, before a candidate is accepted), `python3 scripts/check-agent-guidance.py`.
- Known issues: one git-core askpass test fails on hosts with Git 2.43 and passes in CI; list it in `.local/gate/known-failures.txt` locally (see `docs/validation.md`).
