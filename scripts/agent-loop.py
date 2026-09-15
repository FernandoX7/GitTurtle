#!/usr/bin/env python3
"""Operate GitTurtle's local development queue (Python 3.11+)."""

import sys

sys.dont_write_bytecode = True
if sys.version_info < (3, 11):
    raise SystemExit("agent-loop requires Python 3.11 or newer")

from agent_loop.runner import main

if __name__ == "__main__":
    raise SystemExit(main())
