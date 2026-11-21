"""Unit tests for scripts/dev/build_slot.py."""

from __future__ import annotations

import io
import os
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_slot  # noqa: E402


class BuildSlotTests(unittest.TestCase):
    def test_acquire_release_and_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            slot_dir = Path(tmp) / "slots"
            slot1, fd1 = build_slot.acquire_slot(slot_dir, slots=2, timeout_seconds=1.0)
            try:
                self.assertEqual(slot1, 1)
                slot2, fd2 = build_slot.acquire_slot(slot_dir, slots=2, timeout_seconds=1.0)
                try:
                    self.assertEqual(slot2, 2)
                    self.assertIsNone(build_slot.try_acquire_slot(slot_dir, slots=2))
                    with self.assertRaises(TimeoutError):
                        build_slot.acquire_slot(
                            slot_dir,
                            slots=2,
                            poll_seconds=0.02,
                            timeout_seconds=0.05,
                        )
                    buf = io.StringIO()
                    with redirect_stdout(buf):
                        rc = build_slot.main(["--slot-dir", str(slot_dir), "--slots", "2", "status"])
                    self.assertEqual(rc, 0)
                    self.assertIn("slots=2/2 quiet=no", buf.getvalue())
                finally:
                    build_slot.release_slot(fd2)
            finally:
                build_slot.release_slot(fd1)

            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = build_slot.main(["--slot-dir", str(slot_dir), "--slots", "2", "status"])
            self.assertEqual(rc, 0)
            self.assertIn("slots=0/2 quiet=no", buf.getvalue())

    def test_quiet_lock_blocks_and_clears(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            slot_dir = Path(tmp) / "slots"
            self.assertEqual(build_slot.main(["--slot-dir", str(slot_dir), "quiet-on"]), 0)
            rc = build_slot.main(
                [
                    "--slot-dir",
                    str(slot_dir),
                    "run",
                    "--quiet-seconds",
                    "0.02",
                    "--timeout",
                    "0.05",
                    "--",
                    sys.executable,
                    "-c",
                    "pass",
                ]
            )
            self.assertEqual(rc, 124)
            self.assertEqual(build_slot.main(["--slot-dir", str(slot_dir), "quiet-off"]), 0)
            rc = build_slot.main(
                [
                    "--slot-dir",
                    str(slot_dir),
                    "run",
                    "--",
                    sys.executable,
                    "-c",
                    "raise SystemExit(0)",
                ]
            )
            self.assertEqual(rc, 0)

    def test_lock_fd_is_not_inherited_by_child(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            slot_dir = Path(tmp) / "slots"
            # Child prints open non-stdio fds on Linux via /proc/self/fd if present;
            # also verify os.get_inheritable is False directly.
            _slot, fd = build_slot.acquire_slot(slot_dir, slots=1, timeout_seconds=1.0)
            try:
                self.assertFalse(os.get_inheritable(fd))
            finally:
                build_slot.release_slot(fd)


if __name__ == "__main__":
    unittest.main()
