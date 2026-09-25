---
paths:
  - "scripts/agent_loop/**"
  - "scripts/agent-loop.py"
  - "scripts/gate.py"
  - "scripts/test_gate.py"
  - "scripts/check-agent-guidance.py"
  - ".claude/**"
---
# Development tooling conventions

The controller, the gate and the hooks are stdlib Python 3 with focused unit tests: `umask 022 && python3 -m unittest discover -s scripts/agent_loop -t scripts -p 'test_*.py'` for the controller (it refuses group- or other-writable records, so a umask of 002 fails the suite), `python3 -m unittest scripts/test_gate.py` for the gate, and `python3 scripts/check-agent-guidance.py` for every guidance, agent, skill, rule and hook file. Product Rust gates apply only when product code also changes.

These paths are protected during unattended attempts regardless of task scope. The Codex configuration under `.codex/` and `.agents/` is shared with other tools: leave it unchanged unless explicitly asked, keep the `.claude/skills` symlinks pointing at the shared skills, and keep every Claude-specific instruction out of `AGENTS.md`. Agent files pin a model only for delegated review, verifier, exploration and librarian roles; main-session roles inherit the launched model.

Sessions are bounded so that none of them runs long enough to degrade: agents other than the interactive `planner` and `native-qa` cap `maxTurns`, the controller caps `--max-turns` and `--session-minutes`, and each attempt gets a fresh session rather than a growing one. Keep it that way — raising a cap to finish a task in one session trades a bounded risk for an unbounded one. An interactive coordinator has no such cap, so it applies the session-length rule in `CLAUDE.md` itself.
