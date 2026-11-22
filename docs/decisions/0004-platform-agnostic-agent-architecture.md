# ADR 0004: Platform-Agnostic Agent Architecture and CoW Worktrees

- **Date:** 2026-11-21
- **Status:** Accepted

## Context
Multi-session development accumulated 15,000 LOC of unused standalone controller code (`scripts/agent_loop/`, `scripts/operator/`), 4 MB of append-only handoff prose (`HANDOFF.md`), Claude-only skill/role copies with hardcoded models, and shared `CARGO_TARGET_DIR` collisions across worktrees.

## Decision
- Standardize on open, tool-neutral `AGENTS.md` (≤8 KB root), `.agents/skills/` (13 canonical skills), `.agents/roles/` (8 roles with zero model/effort pinning), and `.agents/workflows/`, generating thin `.claude/` and `.codex/` adapters deterministically via `scripts/dev/sync_adapters.py`.
- Retire `scripts/agent_loop/`, `scripts/agent-loop.py`, and `scripts/operator/` after moving `guidance.py` + tests to `scripts/dev/` and wiring `scripts/test_gate.py` and `scripts/test_claude_rm_hook.py` into CI.
- Enforce safety guards across all tools and humans via shared `scripts/dev/guards.py` and `.githooks/{pre-commit,commit-msg,pre-push}`.
- Replace `HANDOFF.md` with an overwritten `docs/development/STATE.md` (`<= 60` lines, enforced by `check-agent-guidance.py`), and mandate isolated per-worktree `CARGO_TARGET_DIR` seeded via Btrfs/APFS CoW reflinks (`scripts/dev/worktree.py`).

## Consequences
- Startup context overhead drops dramatically across every CLI/IDE agent (Claude, Codex, Gemini/Antigravity, Copilot, Cursor).
- Tooling CI runs faster while testing `gate.py`, `guards.py`, `worktree.py`, `sync_adapters.py`, and `guidance.py` on every change.
