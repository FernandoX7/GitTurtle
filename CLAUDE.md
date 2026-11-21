@AGENTS.md

Claude Code notes:
- Project hooks live in `.claude/settings.json` and `.claude/hooks/` (delegating to `scripts/dev/guards.py` and `scripts/gate.py`).
- Canonical skills live in `.agents/skills/` (symlinked into `.claude/skills/`) and canonical roles live in `.agents/roles/` (synced into `.claude/agents/` via `python3 scripts/dev/sync_adapters.py --write`).
- Do not override `AGENTS.md` rules here; edit `AGENTS.md`, crate `AGENTS.md`, or `.agents/` directly.
