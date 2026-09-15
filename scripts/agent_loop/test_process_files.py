"""Process logs must be safe to open before any truncation or worker launch."""

from __future__ import annotations

import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest

from agent_loop import process


@unittest.skipUnless(os.name == "posix", "Process file ownership supports macOS and Linux")
class ProcessFileSecurityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def assert_output_refused(self, stream, kind):
        directory = self.root / f"{stream}-{kind}"
        directory.mkdir(mode=0o700)
        logs = directory / "logs"
        logs.mkdir(mode=0o700)
        external = directory / "outside-data"
        external.write_bytes(b"existing unrelated bytes must survive\n")
        before = external.read_bytes()
        stdout, stderr = logs / "stdout.log", logs / "stderr.log"
        unsafe = stdout if stream == "stdout" else stderr
        if kind == "symlink":
            unsafe.symlink_to(external)
        elif kind == "hardlink":
            os.link(external, unsafe)
        elif kind == "fifo":
            os.mkfifo(unsafe, mode=0o600)
        elif kind == "writable":
            unsafe.write_bytes(before)
            unsafe.chmod(0o660)
        else:
            self.fail(f"unknown fixture type: {kind}")
        original = unsafe.lstat()
        marker = directory / "worker-started"
        worker_code = f"from pathlib import Path; Path({str(marker)!r}).write_text('started')"
        supervisor_code = f"""
import sys
from pathlib import Path
sys.path.insert(0, {str(Path(process.__file__).resolve().parent.parent)!r})
from agent_loop.process import EnvironmentBlocked, run_process
try:
    run_process([sys.executable, '-B', '-c', {worker_code!r}],
                Path({str(directory)!r}), Path({str(stdout)!r}), 2,
                stderr_path=Path({str(stderr)!r}))
except EnvironmentBlocked:
    raise SystemExit(78)
raise SystemExit(0)
"""
        # An accidentally blocking FIFO open is bounded by the supervisor's
        # timeout; it cannot hang the whole development-tooling test suite.
        result = subprocess.run([sys.executable, "-B", "-c", supervisor_code],
                                cwd=directory, capture_output=True, text=True,
                                timeout=6, check=False)
        self.assertEqual(result.returncode, 78, result.stdout + result.stderr)
        self.assertFalse(marker.exists(), "a worker started despite unsafe output storage")
        self.assertEqual(external.read_bytes(), before)
        remaining = unsafe.lstat()
        self.assertEqual((remaining.st_dev, remaining.st_ino),
                         (original.st_dev, original.st_ino))
        if kind == "fifo":
            self.assertTrue(stat.S_ISFIFO(remaining.st_mode))
        else:
            self.assertEqual(unsafe.read_bytes(), before)

    def test_symlink_outputs_are_refused_before_target_truncation_or_launch(self):
        for stream in ("stdout", "stderr"):
            with self.subTest(stream=stream):
                self.assert_output_refused(stream, "symlink")

    def test_hardlinked_outputs_are_refused_before_alias_truncation_or_launch(self):
        for stream in ("stdout", "stderr"):
            with self.subTest(stream=stream):
                self.assert_output_refused(stream, "hardlink")

    def test_fifo_outputs_are_refused_without_blocking_or_launch(self):
        for stream in ("stdout", "stderr"):
            with self.subTest(stream=stream):
                self.assert_output_refused(stream, "fifo")

    def test_group_writable_outputs_are_refused_before_truncation_or_launch(self):
        for stream in ("stdout", "stderr"):
            with self.subTest(stream=stream):
                self.assert_output_refused(stream, "writable")


if __name__ == "__main__":
    unittest.main()
