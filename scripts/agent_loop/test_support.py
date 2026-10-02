"""Shared unittest fixtures for the controller's own tests.

`records.py` refuses a group- or other-writable record directory or file, and
the controller's `main()` sets a private umask for that reason. Tests call the
library directly, so a module that creates run state, journals or records
imports `setUpModule` and `tearDownModule` from here to hold the same
invariant on a host whose default umask is permissive, such as 002.

`GIT_CONFIG` is the global Git configuration of the disposable repositories
the real-Git tests create (`RunnerTests.setUp` points `GIT_CONFIG_GLOBAL` at
it). Since Git 2.47 every commit, fetch and merge starts `git maintenance run
--auto --detach`, whose daemonized child outlives the command, and since Git
2.54 its default geometric strategy can repack there: a pack or
multi-pack-index written after a test returns makes its TemporaryDirectory
cleanup fail with "Directory not empty". Automatic maintenance stays off, and
any automatic gc an older Git still runs stays in the foreground.
"""

from __future__ import annotations

import os

PRIVATE_UMASK = 0o077

GIT_CONFIG = (
    "[user]\n name = Loop Test\n email = loop@example.invalid\n"
    "[commit]\n gpgsign = false\n"
    "[maintenance]\n auto = false\n"
    "[gc]\n auto = 0\n autoDetach = false\n"
)

_restore: list[int] = []


def setUpModule() -> None:
    _restore.append(os.umask(PRIVATE_UMASK))


def tearDownModule() -> None:
    if _restore:
        os.umask(_restore.pop())
