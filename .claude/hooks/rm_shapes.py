#!/usr/bin/env python3
"""PreToolUse hook: delegate `rm` shape validation to scripts/dev/guards.py."""

import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))

from dev.guards import rm_problem  # noqa: E402


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        return 0
    if data.get("tool_name") != "Bash":
        return 0
    command = (data.get("tool_input") or {}).get("command")
    if not isinstance(command, str) or "rm" not in command:
        return 0
    found = rm_problem(command)
    if found is None:
        return 0
    sys.stderr.write(
        f"{found}. Claude Code cannot resolve that target statically, so it would stop and wait for "
        "a person to approve the removal, whatever the permission mode. Run the removal again with "
        "the literal absolute path written out (for example `rm -rf /abs/run/cfg/*`), in a command "
        "that does not `cd` first and names no variable at the start of the target. Nothing was "
        "removed.\n"
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
