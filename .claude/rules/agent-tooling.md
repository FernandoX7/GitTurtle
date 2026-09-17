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

The controller, the gate and the hooks are stdlib Python 3 with focused unit tests: `python3 -m unittest discover -s scripts/agent_loop -t scripts -p 'test_*.py'` for the controller, `python3 -m unittest scripts/test_gate.py` for the gate, and `python3 scripts/check-agent-guidance.py` for every guidance, agent, skill, rule and hook file. Product Rust gates apply only when product code also changes.

These paths are protected during unattended attempts regardless of task scope. The Codex configuration under `.codex/` and `.agents/` is shared with other tools: leave it unchanged unless explicitly asked, keep the `.claude/skills` symlinks pointing at the shared skills, and keep every Claude-specific instruction out of `AGENTS.md`. Agent files pin a model only for delegated review, QA and exploration roles; main-session roles inherit the launched model.
