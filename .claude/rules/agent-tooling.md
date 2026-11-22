---
paths:
  - "scripts/dev/**"
  - "scripts/ci/**"
  - "scripts/gate.py"
  - "scripts/test_gate.py"
  - "scripts/test_claude_rm_hook.py"
  - "scripts/check-agent-guidance.py"
  - "scripts/check-docs-sync.py"
  - ".agents/**"
  - ".claude/**"
  - ".codex/**"
  - ".githooks/**"
---
# Development tooling conventions

Canonical skills live in `.agents/skills/` and canonical roles live in `.agents/roles/` with zero model or effort pinning (all roles inherit the active session's model and effort). After editing `.agents/roles/` or `.agents/skills/`, run `python3 scripts/dev/sync_adapters.py --write` to update generated `.claude/agents/`, `.codex/agents/`, and `.claude/skills/` symlinks.

Validate tooling changes with:
- `python3 scripts/check-agent-guidance.py`
- `python3 -B -m unittest discover -s scripts/dev -t scripts -p 'test_*.py'`
- `python3 -B -m unittest scripts/test_gate.py scripts/test_claude_rm_hook.py`
- `python3 -m unittest discover -s scripts/ci/tests -t scripts/ci/tests -p 'test_*.py'`
