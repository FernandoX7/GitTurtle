"""Exercise watchdog filesystem authority through its standalone CLI."""

from __future__ import annotations

from contextlib import ExitStack
import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

from agent_loop import process


@unittest.skipUnless(os.name == "posix", "Watchdog ownership supports macOS and Linux")
class WatchdogSecurityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.directory = self.root / "private-records"
        self.directory.mkdir(mode=0o700)
        self.launch_id = "a" * 32
        self.name = "state.process-launch.json"
        self.record = self.directory / self.name
        self.lock = self.record.with_suffix(".lock")
        self.lock.touch(mode=0o600)
        self.write_record(self.record)

    def write_record(self, path, **changes):
        record = {
            "version": 1,
            "launch_id": self.launch_id,
            "state": "starting",
            "argv": [str(self.root / "worker-must-not-start")],
            "cwd": str(self.root),
            "supervisor_pid": os.getpid(),
            "started": time.monotonic(),
            "timeout": 5,
            "max_log_bytes": 4096,
            "cleanup_confirmed": False,
        }
        record.update(changes)
        path.write_text(json.dumps(record), encoding="utf-8")
        path.chmod(0o600)

    def invoke(self, *, directory_fd=None, ownership_fd=None, lock_path=None,
               directory_argument=None, extra_name=None, control_closed=True):
        # EOF makes even a valid launch stop before executing its fixture argv.
        # All descriptors are real inherited capabilities, as in run_process.
        with ExitStack() as stack:
            if directory_fd is None:
                directory_fd = os.open(self.directory, os.O_RDONLY | os.O_DIRECTORY)
                stack.callback(os.close, directory_fd)
            if ownership_fd is None:
                ownership = stack.enter_context((lock_path or self.lock).open("r+b"))
                ownership_fd = ownership.fileno()
                fcntl.flock(ownership_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            prompt = stack.enter_context(tempfile.TemporaryFile())
            output = stack.enter_context(tempfile.TemporaryFile())
            errors = stack.enter_context(tempfile.TemporaryFile())
            reader, writer = os.pipe()
            stack.callback(os.close, reader)
            if control_closed:
                os.close(writer)
                writer = None
            inherited = {directory_fd, reader, prompt.fileno(), output.fileno(),
                         errors.fileno(), ownership_fd}
            arguments = [sys.executable, "-B", str(Path(process.__file__).resolve()),
                         "--watchdog"]
            arguments.append(str(directory_fd) if directory_argument is None else directory_argument)
            if extra_name is not None:
                arguments.append(extra_name)
            arguments.extend(str(fd) for fd in [reader, prompt.fileno(), output.fileno(),
                                               errors.fileno(), ownership_fd])
            watchdog = subprocess.Popen(arguments, cwd=self.directory, pass_fds=tuple(inherited),
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        text=True, start_new_session=True)
            try:
                stdout, stderr = watchdog.communicate(timeout=5)
                return subprocess.CompletedProcess(arguments, watchdog.returncode, stdout, stderr)
            finally:
                if writer is not None:
                    os.close(writer)
                if watchdog.poll() is None:
                    # Closing control requests owned cleanup even if an assertion
                    # or fixture failure interrupts this test's supervision.
                    watchdog.terminate()
                    try:
                        watchdog.communicate(timeout=4)
                    except subprocess.TimeoutExpired:
                        watchdog.kill()
                        watchdog.communicate(timeout=2)

    def assert_refused(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def assert_stopped(self, path):
        record = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(record["launch_id"], self.launch_id)
        self.assertEqual(record["state"], "complete")
        self.assertIs(record["cleanup_confirmed"], True)
        self.assertEqual(record["result"]["stopped"], "supervisor exited")
        self.assertEqual(record["result"]["returncode"], -1)
        self.assertNotIn("worker_pid", record)
        self.assertNotIn("spawn_error", record)

    def test_valid_directory_record_and_lock_can_confirm_supervisor_exit(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_stopped(self.record)

    def test_legacy_external_path_cannot_rewrite_with_unrelated_lock(self):
        external = self.root / "unrelated.json"
        self.write_record(external)
        before = external.read_bytes()
        self.assert_refused(self.invoke(directory_argument=str(external)))
        self.assertEqual(external.read_bytes(), before)

    def test_directory_argument_rejects_paths_and_traversal(self):
        external = self.root / self.name
        for name in (str(external), "../" + self.name, "..\\" + self.name,
                     "./" + self.name, "nested/../" + self.name):
            with self.subTest(name=name):
                self.write_record(external)
                self.write_record(self.record)
                before = external.read_bytes()
                self.assert_refused(self.invoke(directory_argument=name))
                self.assertEqual(external.read_bytes(), before)
                self.assertEqual(json.loads(self.record.read_text())["state"], "starting")

    def test_extra_filename_cannot_select_another_local_record(self):
        unrelated = self.directory / "important.json"
        self.write_record(unrelated)
        before = unrelated.read_bytes()
        self.assert_refused(self.invoke(extra_name=unrelated.name))
        self.assertEqual(unrelated.read_bytes(), before)

    def test_malformed_record_launch_id_cannot_be_treated_as_owned(self):
        self.write_record(self.record, launch_id="../not-a-launch")
        before = self.record.read_bytes()
        self.assert_refused(self.invoke())
        self.assertEqual(self.record.read_bytes(), before)

    def test_running_or_complete_launch_cannot_be_replayed(self):
        for state in ("running", "complete"):
            with self.subTest(state=state):
                marker = self.root / f"replayed-{state}"
                worker_code = f"from pathlib import Path; Path({str(marker)!r}).touch()"
                self.write_record(self.record, state=state, cleanup_confirmed=state == "complete",
                                  argv=[sys.executable, "-B", "-c", worker_code])
                before = self.record.read_bytes()
                self.assert_refused(self.invoke(control_closed=False))
                self.assertEqual(self.record.read_bytes(), before)
                self.assertFalse(marker.exists(), "replayed launch started another worker")

    def test_inherited_lock_must_match_the_records_lock_inode(self):
        other_lock = self.directory / "unrelated.lock"
        other_lock.touch(mode=0o600)
        before = self.record.read_bytes()
        self.assert_refused(self.invoke(lock_path=other_lock))
        self.assertEqual(self.record.read_bytes(), before)
        self.assertEqual(other_lock.read_bytes(), b"")

    def test_replaced_lock_cannot_borrow_ownership_from_an_old_descriptor(self):
        with self.lock.open("r+b") as ownership:
            fcntl.flock(ownership.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            retired_lock = self.directory / "retired.lock"
            self.lock.rename(retired_lock)
            self.lock.touch(mode=0o600)
            before = self.record.read_bytes()
            self.assert_refused(self.invoke(ownership_fd=ownership.fileno()))
            self.assertEqual(self.record.read_bytes(), before)
            self.assertEqual(self.lock.read_bytes(), b"")
            self.assertEqual(retired_lock.read_bytes(), b"")

    def test_symlink_lock_is_not_accepted_even_when_descriptor_matches_target(self):
        external = self.root / "unrelated.lock"
        self.lock.rename(external)
        self.lock.symlink_to(external)
        before = self.record.read_bytes()
        self.assert_refused(self.invoke(lock_path=external))
        self.assertEqual(self.record.read_bytes(), before)
        self.assertTrue(self.lock.is_symlink())
        self.assertEqual(external.read_bytes(), b"")

    def test_nonregular_ownership_descriptor_is_refused_without_record_changes(self):
        reader, writer = os.pipe()
        try:
            before = self.record.read_bytes()
            self.assert_refused(self.invoke(ownership_fd=reader))
            self.assertEqual(self.record.read_bytes(), before)
        finally:
            os.close(reader)
            os.close(writer)

    def test_multiply_linked_lock_cannot_claim_exclusive_record_ownership(self):
        alias = self.root / "another-records.lock"
        os.link(self.lock, alias)
        before = self.record.read_bytes()
        self.assert_refused(self.invoke())
        self.assertEqual(self.record.read_bytes(), before)
        self.assertEqual(alias.read_bytes(), b"")

    def test_other_users_cannot_be_given_write_access_to_record_directory(self):
        self.directory.chmod(0o770)
        before = self.record.read_bytes()
        self.assert_refused(self.invoke())
        self.assertEqual(self.record.read_bytes(), before)

    def test_other_users_cannot_be_given_write_access_to_ownership_lock(self):
        self.lock.chmod(0o660)
        before = self.record.read_bytes()
        self.assert_refused(self.invoke())
        self.assertEqual(self.record.read_bytes(), before)

    def test_regular_file_cannot_be_used_as_directory_authority(self):
        with self.record.open("rb") as ordinary_file:
            before = self.record.read_bytes()
            self.assert_refused(self.invoke(directory_fd=ordinary_file.fileno()))
            self.assertEqual(self.record.read_bytes(), before)

    def test_symlink_record_never_reads_or_replaces_the_external_target(self):
        external = self.root / "unrelated.json"
        self.record.rename(external)
        self.record.symlink_to(external)
        before = external.read_bytes()
        self.assert_refused(self.invoke())
        self.assertEqual(external.read_bytes(), before)
        self.assertTrue(self.record.is_symlink())

    def test_fifo_record_is_refused_promptly_without_waiting_for_a_writer(self):
        self.record.unlink()
        os.mkfifo(self.record, mode=0o600)
        self.assert_refused(self.invoke())

    def test_directory_rename_and_replacement_symlink_cannot_redirect_completion(self):
        external = self.root / "external-records"
        external.mkdir(mode=0o700)
        external_record = external / self.name
        self.write_record(external_record)
        before = external_record.read_bytes()
        pinned = os.open(self.directory, os.O_RDONLY | os.O_DIRECTORY)
        try:
            moved = self.root / "moved-private-records"
            self.directory.rename(moved)
            self.directory.symlink_to(external, target_is_directory=True)
            result = self.invoke(directory_fd=pinned, lock_path=moved / self.lock.name)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assert_stopped(moved / self.name)
            self.assertEqual(external_record.read_bytes(), before)
            self.assertEqual(sorted(path.name for path in external.iterdir()), [self.name])
            self.assertTrue(self.directory.is_symlink())
        finally:
            os.close(pinned)

    def test_completion_refuses_replaced_record_and_preserves_outside_bytes(self):
        external = self.root / "important-outside-data.json"
        external.write_bytes(b"outside data must survive\n")
        before = external.read_bytes()
        marker = self.root / "swapped-by-worker"
        worker_code = f"""
import json
import os
from pathlib import Path
import time

record = Path({str(self.record)!r})
deadline = time.monotonic() + 3
while time.monotonic() < deadline:
    if json.loads(record.read_text())["state"] == "running":
        break
    time.sleep(0.01)
else:
    raise SystemExit("watchdog never published running state")
record.unlink()
record.symlink_to({str(external)!r})
Path({str(marker)!r}).write_text(str(os.getpid()))
"""
        self.write_record(self.record, argv=[sys.executable, "-B", "-c", worker_code])
        result = self.invoke(control_closed=False)
        self.assertTrue(marker.is_file(), result.stdout + result.stderr)
        self.assert_refused(result)
        self.assertEqual(external.read_bytes(), before)
        self.assertTrue(self.record.is_symlink())
        pid = int(marker.read_text())
        status = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)],
                                capture_output=True, text=True, timeout=2, check=False)
        self.assertFalse(status.stdout.strip(), "fixture worker survived failed completion")


if __name__ == "__main__":
    unittest.main()
