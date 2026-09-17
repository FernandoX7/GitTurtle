#!/usr/bin/env python3
"""PreToolUse hook: refuse edits to policy paths during controller sessions.

The unattended controller sets GITTURTLE_LOOP=1. In that mode every guidance,
agent, skill, controller, task-policy and gate-configuration path is protected
regardless of the task's scope globs; the controller's patch validation is the
backstop, this hook is the first line. Interactive sessions are unaffected.
"""
import json
import os
import sys
from pathlib import Path

PROTECTED_PREFIXES = (
    ".claude/",
    ".codex/",
    ".agents/",
    ".config/nextest.toml",
    "scripts/agent_loop/",
    "scripts/agent-loop.py",
    "scripts/check-agent-guidance.py",
    "scripts/gate.py",
    "docs/development/tasks.json",
    "docs/development/task.schema.json",
    "docs/development/security-review.md",
    "deny.toml",
    "clippy.toml",
    "rustfmt.toml",
)
PROTECTED_NAMES = ("AGENTS.md", "CLAUDE.md")


def relative_path(raw: str, cwd: Path) -> str:
    path = Path(raw)
    if not path.is_absolute():
        path = cwd / path
    try:
        return path.resolve().relative_to(cwd.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def is_protected(relative: str) -> bool:
    if Path(relative).name in PROTECTED_NAMES:
        return True
    if any(relative == p.rstrip("/") or relative.startswith(p) for p in PROTECTED_PREFIXES):
        return True
    parts = Path(relative).parts
    return "snapshots" in parts and relative.endswith((".snap", ".snap.new"))


def main() -> int:
    if os.environ.get("GITTURTLE_LOOP") != "1":
        return 0
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        return 0
    tool_input = data.get("tool_input") or {}
    raw = tool_input.get("file_path") or tool_input.get("notebook_path")
    if not raw:
        return 0
    cwd = Path(data.get("cwd") or os.getcwd())
    relative = relative_path(str(raw), cwd)
    if not is_protected(relative):
        return 0
    sys.stderr.write(
        f"protected path in a controller session: {relative}. Guidance, agent, skill, "
        "controller, task-policy and gate-configuration files cannot change during an "
        "unattended attempt; return `blocked` with a concrete proposal instead.\n"
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
