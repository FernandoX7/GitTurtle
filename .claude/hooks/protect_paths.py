#!/usr/bin/env python3
"""PreToolUse hook: delegate protected-path checks to scripts/dev/guards.py."""

import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))

from dev.guards import active_queue, path_protection_reason  # noqa: E402


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
    reason = path_protection_reason(str(raw), cwd, queue=active_queue())
    if reason is None:
        return 0
    sys.stderr.write(
        f"protected path in a loop session: {reason}. Guidance, agent, skill, "
        "guard, task-policy and gate-configuration files cannot change during a "
        "loop attempt; return `blocked` with a concrete proposal instead.\n"
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
