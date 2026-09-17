# Handoff

Keep the human section under 15 lines: what changed last, what is next, the exact commands that work, and known issues. Interactive sessions rewrite it when closing a task (see the `close-task` skill). The unattended controller writes its own generated handoff into the run directory and never edits this file.

- Last change: the themes plan (`7b2f906`: research note `docs/development/2026-09-17-theme-palettes.md`, specification `docs/development/themes/spec.md`, eleven-task queue `docs/development/themes/tasks.json`, pinned MIT notices) and the nine harness fixes below, each in its own commit. The default queue `docs/development/tasks.json` is untouched.
- Next: run the controller on the themes queue from this clean checkout:
  `python3 scripts/agent-loop.py run --tool claude --tasks docs/development/themes/tasks.json --model opus --effort high --hard-model fable --light-model sonnet --max-tasks 11 --max-attempts 3 --max-minutes 600 --session-minutes 90`
- While it runs: the first task needing an attestation is `themes-batch-solarized-one` (native); `themes-custom-model` → `themes-store-migration` continue meanwhile because they depend only on `themes-readability-rules`. Attest from a desktop session with `claude --agent native-qa` or the `performance-reviewer`, then `python3 scripts/agent-loop.py attest --run <dir> --task <id> --candidate <sha> --kind native|performance --evidence <path> --summary "<text>"` and `python3 scripts/agent-loop.py resume --run <dir>`.
- Hard tasks (`Hard.` in the description, routed to `--hard-model` after two failed attempts): `themes-store-migration`, `themes-editor`.
- Commands: `python3 scripts/agent-loop.py validate --tasks docs/development/themes/tasks.json`; `python3 scripts/gate.py fast`; `python3 scripts/gate.py full`; `python3 scripts/check-agent-guidance.py`; `python3 -m unittest discover -s scripts/agent_loop -t scripts -p 'test_*.py'` (188 tests, no umask workaround needed any more).
- Known issues: one git-core askpass test fails on hosts with Git 2.43 (list it in `.local/gate/known-failures.txt`, see `docs/validation.md`). An untracked Codex mirror from an earlier session (`.codex/agents/*.toml`, `.codex/config.toml`, a `hooks.json` with absolute worktree paths, and copies of the five Claude-only skills under `.agents/skills/`) was moved out of the tree to `.local/codex-mirror-2026-09-17/`; decide whether to delete it or rebuild it without absolute paths. `scripts/gate.py` still forces umask 077 for its controller-tests stage, now redundant but harmless.

## Harness fixes

Defects met while planning the themes work on 2026-09-17 with `claude --model fable --agent planner`, and their fixes. Keep this ledger until the themes run has used the fixed harness end to end; the next planning session judges whether each fix held.

| # | File(s) | Symptom | Fix |
| --- | --- | --- | --- |
| 1 | `.claude/agents/planner.md` | No Edit/Write and `permissionMode: plan`, yet the role must write notes, specs and the queue; writes went through Bash heredocs, bypassing the protect hook | `28a26c6`: Edit, Write, WebFetch and WebSearch; `permissionMode: default`; writes limited by prompt to `docs/`, the notices and the queue |
| 2 | `.claude/skills/queue-intake/SKILL.md` | `disable-model-invocation: true` while the planner is told to use the skill; default queue path hard-coded | `28a26c6`: model-invocable, Write allowed, separate queue files and `validate --tasks <path>` documented |
| 3 | `.claude/agents/planner.md`, `.claude/skills/research/SKILL.md` | Research skill demands live sources; planner had no web tools | `28a26c6`: web tools added; `gh api`/`curl` route documented |
| 4 | `scripts/agent_loop/{runner,claude,codex}.py`, `.claude/hooks/protect_paths.py` | Only the default queue was protected at edit time; the controller refused a queue edit only after the session | `30dea95`: adapters export `GITTURTLE_TASKS_PATH`; the hook blocks that path; hook and adapter tests |
| 5 | `.agents/skills/gitturtle-performance/references/measurement.md` | Metric catalog under a protected path; a task adding a trace metric could not document it | `1710b77`: catalog moved to `docs/benchmarks/metrics.md`; `themes-apply-trace` points at it |
| 6 | `_typos.toml` | Seven-character Git abbreviations failed the typos stage | `64d8f06`: hex tokens of 7–40 characters ignored as identifiers; bare words still reported |
| 7 | `.claude/agents/Explore.md`, `.claude/agents/planner.md`, `docs/development/claude-code.md` | Background Explore report surfaced only at the next turn boundary, truncated | `28a26c6`: planner waits for the report and asks bounded questions; Explore caps its report at 6,000 characters |
| 8 | Worktree | Untracked Codex mirror with absolute paths beside the committed Claude skills | Parked in `.local/codex-mirror-2026-09-17/` (ignored); decision pending, see known issues |
| 9 | `scripts/agent_loop/runner.py`, `scripts/agent_loop/test_support.py` and eight test modules | With this host's umask 002, 82 of 187 controller tests failed and a live run could refuse its own state | `070086f`: private umask in `main()`, shared test fixture, a CLI test under umask 002; 188 tests pass under 002, 022 and 077 |
