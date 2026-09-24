"""Crash/reconciliation tests using disposable process trees, never Codex."""

from __future__ import annotations

import errno
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from agent_loop import process
from agent_loop.process import EnvironmentBlocked, LoopError, reconcile_processes, run_process
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


@unittest.skipUnless(os.name == "posix", "Watchdog ownership supports macOS and Linux")
class ProcessRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def wait_for(self, condition, message, timeout=5):
        deadline = time.monotonic() + timeout
        while not condition():
            if time.monotonic() >= deadline:
                self.fail(message)
            time.sleep(0.02)

    def inactive(self, pid):
        result = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)],
                                capture_output=True, text=True, timeout=2, check=False)
        state = result.stdout.strip()
        return not state or state.startswith("Z")

    def test_supervisor_sigkill_stops_workers_before_resume_can_retry(self):
        # Copy the module to exercise the same standalone watchdog entry point
        # used by immutable controller snapshots, with no installed package.
        module = self.root / "snapshot/scripts/agent_loop"
        module.mkdir(parents=True)
        (module / "__init__.py").write_text("")
        for name in ("process.py", "records.py"):
            shutil.copyfile(Path(process.__file__).with_name(name), module / name)
        ready = self.root / "ready"
        pid_file = self.root / "worker-pids.json"
        leaf_code = (
            "import os, signal, time; from pathlib import Path; "
            "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
            f"Path({str(ready)!r}).write_text(str(os.getpid())); time.sleep(15)"
        )
        worker_code = (
            "import json, os, subprocess, sys, time; from pathlib import Path; "
            f"child = subprocess.Popen([sys.executable, '-c', {leaf_code!r}]); "
            f"Path({str(pid_file)!r}).write_text(json.dumps([os.getpid(), child.pid])); "
            "time.sleep(15)"
        )
        supervisor_code = (
            "import sys; from pathlib import Path; "
            f"sys.path.insert(0, {str(module.parent)!r}); "
            "from agent_loop.process import run_process; "
            f"run_process([sys.executable, '-c', {worker_code!r}], "
            f"Path({str(self.root)!r}), Path({str(self.root / 'attempt/worker.log')!r}), 10)"
        )
        supervisor = subprocess.Popen([sys.executable, "-B", "-c", supervisor_code],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        def close_supervisor():
            if supervisor.poll() is None:
                supervisor.terminate()
            supervisor.wait(timeout=5)

        self.addCleanup(close_supervisor)
        self.wait_for(ready.exists, "fixture worker never became ready")
        leader, leaf = json.loads(pid_file.read_text())
        self.assertFalse(self.inactive(leaf))
        supervisor.kill()
        supervisor.wait(timeout=5)
        records = reconcile_processes(self.root, timeout=3)
        self.assertEqual(len(records), 1)
        self.assertEqual(records[0]["result"]["stopped"], "supervisor exited")
        self.assertTrue(records[0]["cleanup_confirmed"])
        for pid in (leader, leaf, records[0]["guard_pid"]):
            self.wait_for(lambda pid=pid: self.inactive(pid), f"worker {pid} survived supervisor death")

    def record(self, *, complete):
        path = self.root / ("fixture" + process.LAUNCH_SUFFIX)
        record = {
            "version": 1, "launch_id": "a" * 32,
            "state": "complete" if complete else "running",
            "cleanup_confirmed": complete,
            # A PID that could already have been reused must never be signaled.
            "worker_pid": os.getpid(),
        }
        if complete:
            record["result"] = {"argv": ["fixture"], "returncode": 0,
                                "elapsed": 0.1, "stopped": None}
        process.atomic_json(path, record)
        path.with_suffix(".lock").touch()
        return path

    def test_incomplete_unlocked_record_blocks_without_signaling_saved_pids(self):
        path = self.record(complete=False)
        before = path.read_bytes()
        with patch.object(process.os, "kill") as kill, patch.object(process.os, "killpg") as killpg:
            with self.assertRaisesRegex(EnvironmentBlocked, "cleanup is unconfirmed"):
                reconcile_processes(self.root, timeout=0)
            kill.assert_not_called()
            killpg.assert_not_called()
        self.assertEqual(path.read_bytes(), before)

    def test_resume_waits_for_lock_even_if_completion_was_already_written(self):
        path = self.record(complete=True)
        with path.with_suffix(".lock").open("r+b") as lock:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            with self.assertRaisesRegex(EnvironmentBlocked, "watchdog still owns"):
                reconcile_processes(self.root, timeout=0.05)
        self.assertEqual(len(reconcile_processes(self.root, timeout=0)), 1)

    def test_attempt_named_build_is_still_checked_for_live_processes(self):
        path = self.record(complete=False)
        attempt = self.root / "attempts/build/1"
        attempt.mkdir(parents=True)
        path.with_suffix(".lock").rename(attempt / path.with_suffix(".lock").name)
        path.rename(attempt / path.name)
        with self.assertRaisesRegex(EnvironmentBlocked, "cleanup is unconfirmed"):
            reconcile_processes(self.root, timeout=0)

    def test_exact_source_and_cache_trees_are_excluded_from_journal_scan(self):
        path = self.record(complete=False)
        for relative in ("build", "accepted", "controller", "attempts/build/1/repo"):
            directory = self.root / relative
            directory.mkdir(parents=True)
            shutil.copyfile(path, directory / path.name)
            shutil.copyfile(path.with_suffix(".lock"), directory / path.with_suffix(".lock").name)
        path.unlink()
        path.with_suffix(".lock").unlink()
        self.assertEqual(reconcile_processes(self.root), [])

    def test_unavailable_executable_is_environment_blocked_with_confirmed_cleanup(self):
        with self.assertRaisesRegex(EnvironmentBlocked, "cannot start"):
            run_process([str(self.root / "missing-program")], self.root, self.root / "missing.log", 5)
        records = reconcile_processes(self.root)
        self.assertEqual(len(records), 1)
        self.assertTrue(records[0]["cleanup_confirmed"])
        self.assertEqual(records[0]["result"]["stopped"], "executable unavailable")

    def test_failed_watchdog_spawn_and_record_write_close_both_control_descriptors(self):
        descriptors = []
        original_pipe, original_write = os.pipe, process.atomic_json_at

        def capture_pipe():
            pair = original_pipe()
            descriptors.extend(pair)
            return pair

        def fail_completion_write(directory, name, record):
            if record["state"] == "complete":
                raise LoopError("fixture completion write failed")
            original_write(directory, name, record)

        with (patch.object(process.os, "pipe", side_effect=capture_pipe),
              patch.object(process, "atomic_json_at", side_effect=fail_completion_write),
              patch.object(process.subprocess, "Popen", side_effect=OSError("fixture spawn failure"))):
            with self.assertRaisesRegex(LoopError, "fixture completion write failed"):
                run_process(["fixture-never-started"], self.root, self.root / "failed.log", 5)
        self.assertEqual(len(descriptors), 2)
        for fd in descriptors:
            with self.subTest(fd=fd), self.assertRaises(OSError) as raised:
                os.fstat(fd)
            self.assertEqual(raised.exception.errno, errno.EBADF)
        with self.assertRaisesRegex(EnvironmentBlocked, "cleanup is unconfirmed"):
            reconcile_processes(self.root)

    def test_callback_exception_still_closes_control_pipe_and_confirms_cleanup(self):
        def fail_stop():
            raise RuntimeError("fixture callback failure")

        with self.assertRaisesRegex(RuntimeError, "fixture callback failure"):
            run_process([sys.executable, "-c", "import time; time.sleep(15)"],
                        self.root, self.root / "callback.log", 5, stop=fail_stop)
        records = reconcile_processes(self.root)
        self.assertEqual(records[0]["result"]["stopped"], "supervisor exited")
        self.assertTrue(records[0]["cleanup_confirmed"])

    def test_separate_stderr_preserves_capture_and_combined_log_limit(self):
        stdout, stderr = self.root / "stdout.log", self.root / "stderr.log"
        result = run_process(
            [sys.executable, "-c", "import sys; print('answer'); print('diagnostic', file=sys.stderr)"],
            self.root, stdout, 5, stderr_path=stderr,
        )
        self.assertEqual(result.returncode, 0)
        self.assertIsNone(result.stopped)
        self.assertEqual(stdout.read_text(), "answer\n")
        self.assertEqual(stderr.read_text(), "diagnostic\n")
        result = run_process(
            [sys.executable, "-c", "import sys; sys.stdout.write('o'*100); sys.stderr.write('e'*100)"],
            self.root, stdout, 5, stderr_path=stderr, max_log_bytes=128,
        )
        self.assertEqual(result.stopped, "log limit exceeded")
        self.assertEqual(stdout.read_bytes(), b"o" * 100)
        self.assertEqual(stderr.read_bytes(), b"e" * 28)
        self.assertEqual(len(reconcile_processes(self.root)), 2)

    def test_oversized_launch_record_is_not_treated_as_completed(self):
        path = self.record(complete=True)
        path.write_text(" " * (process.MAX_LAUNCH_BYTES + 1))
        with self.assertRaisesRegex(EnvironmentBlocked, "size limit"):
            reconcile_processes(self.root)

    def test_directory_alias_for_shared_output_preserves_both_streams_and_limit(self):
        logs = self.root / "logs"
        logs.mkdir()
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        output = logs / "combined.log"
        result = run_process(
            [sys.executable, "-c", "import os; os.write(1, b'answer\\n'); os.write(2, b'diagnostic\\n')"],
            self.root, output, 5, stderr_path=alias / "logs" / output.name, max_log_bytes=18,
        )
        self.assertEqual(result.returncode, 0)
        self.assertIsNone(result.stopped)
        self.assertEqual(output.read_bytes(), b"answer\ndiagnostic\n")


if __name__ == "__main__":
    unittest.main()
