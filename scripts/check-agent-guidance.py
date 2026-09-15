#!/usr/bin/env python3
"""Validate maintained development guidance without building GitTurtle."""

import sys

if sys.version_info < (3, 11):
    raise SystemExit("check-agent-guidance.py requires Python 3.11 or newer")

from agent_loop.guidance import main


if __name__ == "__main__":
    raise SystemExit(main())
