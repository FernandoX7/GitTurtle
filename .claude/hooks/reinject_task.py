#!/usr/bin/env python3
"""SessionStart(compact) hook: restore active workspace state after compaction.

If `GITTURTLE_TASK_CONTEXT` points at an explicit contract file, inject it.
Otherwise inject `docs/development/STATE.md` together with `git status -sb`
and the active worktree's isolated `CARGO_TARGET_DIR` (if present).
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys


def _git_status_summary(cwd: Path) -> str:
    try:
        completed = subprocess.run(
            ["git", "status", "-sb"],
            cwd=cwd,
            capture_output=True,
            text=True,
            errors="replace",
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    if completed.returncode != 0:
        return ""
    lines = completed.stdout.splitlines()[:40]
    return "\n".join(lines).strip()


def _detect_target_dir(cwd: Path) -> str:
    configured = os.environ.get("CARGO_TARGET_DIR")
    if configured:
        return configured
    local_target = cwd / ".local" / "target"
    if local_target.is_dir():
        return str(local_target)
    return ""


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        data = {}
    cwd = Path(data.get("cwd") or os.getcwd())

    parts: list[str] = []
    location = os.environ.get("GITTURTLE_TASK_CONTEXT")
    if location:
        path = Path(location)
        if path.is_file():
            parts.append("Task contract restored after compaction:\n" + path.read_text(encoding="utf-8", errors="replace")[:20000])
    else:
        state_file = cwd / "docs" / "development" / "STATE.md"
        if state_file.is_file():
            parts.append("Active workspace STATE.md:\n" + state_file.read_text(encoding="utf-8", errors="replace")[:8000])

    status = _git_status_summary(cwd)
    if status:
        parts.append("Current git status -sb:\n" + status)

    target_dir = _detect_target_dir(cwd)
    if target_dir:
        parts.append(f"Isolated CARGO_TARGET_DIR: {target_dir}")

    if not parts:
        return 0

    json.dump(
        {
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": "\n\n".join(parts),
            }
        },
        sys.stdout,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
