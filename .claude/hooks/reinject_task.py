#!/usr/bin/env python3
"""SessionStart(compact) hook: put the task contract back after compaction.

The controller points GITTURTLE_TASK_CONTEXT at a file with the task contract
and evidence path. Interactive sessions without that variable get nothing.
"""
import json
import os
import sys
from pathlib import Path


def main() -> int:
    location = os.environ.get("GITTURTLE_TASK_CONTEXT")
    if not location:
        return 0
    path = Path(location)
    if not path.is_file():
        return 0
    text = path.read_text(encoding="utf-8", errors="replace")[:20000]
    json.dump(
        {
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": "Task contract restored after compaction:\n" + text,
            }
        },
        sys.stdout,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
