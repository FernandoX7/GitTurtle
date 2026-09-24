"""Shared unittest fixtures for the controller's own tests.

`records.py` refuses a group- or other-writable record directory or file, and
the controller's `main()` sets a private umask for that reason. Tests call the
library directly, so a module that creates run state, journals or records
imports `setUpModule` and `tearDownModule` from here to hold the same
invariant on a host whose default umask is permissive, such as 002.
"""

from __future__ import annotations

import os

PRIVATE_UMASK = 0o077

_restore: list[int] = []


def setUpModule() -> None:
    _restore.append(os.umask(PRIVATE_UMASK))


def tearDownModule() -> None:
    if _restore:
        os.umask(_restore.pop())
