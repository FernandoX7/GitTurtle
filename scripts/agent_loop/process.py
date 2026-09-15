"""Bounded subprocess ownership and durable local records."""

from __future__ import annotations

import hashlib
import json
import math
import os
from pathlib import Path
import re
import select
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid
from contextlib import ExitStack
from dataclasses import asdict, dataclass
from typing import Callable

import fcntl

# Immutable controller snapshots also launch this file as a standalone script.
if __package__:
    from .records import (
        EnvironmentBlocked, LoopError, atomic_json, atomic_json_at,
        open_directory, read_json, read_json_at, validate_basename,
    )
else:
    from records import (
        EnvironmentBlocked, LoopError, atomic_json, atomic_json_at,
        open_directory, read_json, read_json_at, validate_basename,
    )


MAX_LAUNCH_BYTES = 256 * 1024
LAUNCH_SUFFIX = ".process-launch.json"
LAUNCH_RECORD = "state.process-launch.json"
LAUNCH_LOCK = "state.process-launch.lock"


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


@dataclass(frozen=True)
class Result:
    argv: tuple[str, ...]
    returncode: int
    elapsed: float
    stopped: str | None = None


def run_process(
    argv: list[str],
    cwd: Path,
    log: Path,
    timeout: float,
    *,
    stdin: str | None = None,
    env: dict[str, str] | None = None,
    stop: Callable[[], bool] = lambda: False,
    max_log_bytes: int = 32 * 1024 * 1024,
    stderr_path: Path | None = None,
) -> Result:
    """Own a worker through a watchdog that survives supervisor termination.

    Closing the private control pipe requests cleanup, including after SIGKILL
    of this supervisor. Only the watchdog signals its unreaped direct child's
    process group. Resume checks durable completion plus the ownership lock;
    recorded PIDs are diagnostic information, never recovery kill targets.
    """
    if not math.isfinite(timeout):
        raise LoopError("process timeout must be finite")
    if timeout <= 0:
        return Result(tuple(argv), -1, 0.0, "time budget exhausted")
    if not argv or any(not isinstance(argument, str) for argument in argv):
        raise LoopError("process arguments must be a nonempty string array")
    if type(max_log_bytes) is not int or max_log_bytes <= 0:
        raise LoopError("process log limit must be a positive integer")
    cwd, log = cwd.resolve(), log.absolute()
    started = time.monotonic()
    launch_id = uuid.uuid4().hex
    journal_name = ".launch-" + launch_id
    record_path = log.parent / journal_name / LAUNCH_RECORD
    record = {
        "version": 1, "launch_id": launch_id, "state": "starting",
        "argv": argv, "cwd": str(cwd), "supervisor_pid": os.getpid(),
        "started": started, "timeout": timeout, "max_log_bytes": max_log_bytes,
        "cleanup_confirmed": False,
    }
    if len(json.dumps(record).encode("utf-8")) > (MAX_LAUNCH_BYTES - 16384) // 2:
        raise LoopError("process launch arguments exceed the durable record limit")
    with ExitStack() as stack:
        logs = stack.enter_context(open_directory(log.parent, create=True))
        output = stack.enter_context(_open_output_at(logs, log.name))
        errors = output
        if stderr_path is not None and stderr_path.absolute() != log:
            error_directory = stack.enter_context(open_directory(stderr_path.parent, create=True))
            errors = stack.enter_context(_open_output_at(error_directory, stderr_path.name))
            if os.path.sameopenfile(output.fileno(), errors.fileno()):
                errors.close()
                errors = output
        # The watchdog receives a directory capability, never an arbitrary path.
        # Fixed record names cannot select a sibling or traverse a parent.
        os.mkdir(journal_name, 0o700, dir_fd=logs)
        journal = os.open(journal_name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
                          | os.O_CLOEXEC, dir_fd=logs)
        stack.callback(os.close, journal)
        os.fsync(logs)
        # Inherit an already-held lock so even a supervisor crash during launch
        # cannot expose an unlocked record while a watchdog is starting.
        ownership = stack.enter_context(_open_lock_at(journal, LAUNCH_LOCK, create=True))
        fcntl.flock(ownership.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        atomic_json_at(journal, LAUNCH_RECORD, record)
        prompt = stack.enter_context(tempfile.TemporaryFile())
        if stdin is not None:
            prompt.write(stdin.encode("utf-8"))
            prompt.seek(0)
        reader, writer = os.pipe()
        control = stack.enter_context(os.fdopen(writer, "wb", buffering=0))
        watchdog = None
        try:
            watchdog = subprocess.Popen(
                [sys.executable, "-B", str(Path(__file__).resolve()), "--watchdog",
                 str(journal), str(reader), str(prompt.fileno()), str(output.fileno()),
                 str(errors.fileno()), str(ownership.fileno())],
                cwd=cwd, stdin=subprocess.DEVNULL, stdout=errors, stderr=errors,
                env=env, start_new_session=True,
                pass_fds=tuple({journal, reader, prompt.fileno(), output.fileno(), errors.fileno(), ownership.fileno()}),
            )
        except OSError as error:
            record.update(state="complete", cleanup_confirmed=True,
                          spawn_error=f"cannot start process watchdog: {error}"[:4096],
                          result=asdict(Result(tuple(argv), -1, time.monotonic() - started,
                                               "watchdog unavailable")))
            atomic_json_at(journal, LAUNCH_RECORD, record)
            raise EnvironmentBlocked(f"cannot start process watchdog: {error}") from error
        finally:
            os.close(reader)
            # Closing this copy must not explicitly unlock the shared lock.
            ownership.close()
        try:
            while watchdog.poll() is None:
                if stop():
                    try:
                        os.write(control.fileno(), b"stop")
                    except BrokenPipeError:
                        pass
                    break
                time.sleep(0.05)
        finally:
            control.close()
            try:
                watchdog.wait(timeout=3)
            except subprocess.TimeoutExpired as error:
                raise EnvironmentBlocked(f"watchdog cleanup is still active; inspect {record_path}") from error
        completed = _read_launch_at(journal, LAUNCH_RECORD)
        result = _completed_result(completed, record_path)
        if completed.get("spawn_error") or completed.get("watchdog_error"):
            raise EnvironmentBlocked(str(completed.get("spawn_error") or completed.get("watchdog_error")))
        return result


def _lock_path(record: Path) -> Path:
    return record.with_suffix(".lock")


def _open_owned_file_at(directory: int, name: str, flags: int):
    validate_basename(name)
    fd = None
    try:
        fd = os.open(name, flags | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                     0o600, dir_fd=directory)
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                or info.st_uid != os.geteuid() or info.st_mode & 0o022):
            raise EnvironmentBlocked(f"expected an owned regular file: {name!r}")
        result = os.fdopen(fd, "r+b")
        fd = None
        return result
    except OSError as error:
        raise EnvironmentBlocked(f"cannot open process file {name!r}: {error}") from error
    finally:
        if fd is not None:
            os.close(fd)


def _open_lock_at(directory: int, name: str, *, create: bool = False):
    return _open_owned_file_at(directory, name, os.O_RDWR | (os.O_CREAT | os.O_EXCL if create else 0))


def _open_output_at(directory: int, name: str):
    output = _open_owned_file_at(directory, name, os.O_RDWR | os.O_CREAT)
    try:
        # Check the descriptor before truncation, including hardlink refusal.
        os.ftruncate(output.fileno(), 0)
        return output
    except BaseException:
        output.close()
        raise


def _read_launch_at(directory: int, name: str) -> dict:
    try:
        record = read_json_at(directory, name, max_bytes=MAX_LAUNCH_BYTES)
        if record.get("version") != 1 or not isinstance(record.get("launch_id"), str):
            raise LoopError("unknown launch record format")
        return record
    except (OSError, LoopError) as error:
        raise EnvironmentBlocked(f"cannot establish process state from {name!r}: {error}") from error


def _completed_result(record: dict, path: Path) -> Result:
    result = record.get("result")
    if (record.get("state") != "complete" or record.get("cleanup_confirmed") is not True
            or not isinstance(result, dict)):
        raise EnvironmentBlocked(f"process cleanup is unconfirmed; preserve and inspect {path}")
    argv = result.get("argv")
    elapsed = result.get("elapsed")
    if (not isinstance(argv, list) or not argv or not all(isinstance(item, str) for item in argv)
            or type(result.get("returncode")) is not int
            or not isinstance(elapsed, (int, float)) or not math.isfinite(elapsed) or elapsed < 0
            or (result.get("stopped") is not None and not isinstance(result["stopped"], str))):
        raise EnvironmentBlocked(f"invalid completed process record: {path}")
    return Result(tuple(argv), result["returncode"], float(elapsed), result.get("stopped"))


def reconcile_processes(directory: Path, timeout: float = 3.0) -> list[dict]:
    """Wait for recorded watchdogs; refuse any unconfirmed cleanup before retry.

    Keep records and locks beside durable logs under this run directory, outside
    its root build/accepted/controller trees and attempts/<task>/<attempt>/repo
    source checkouts. A watchdog independently killed before recording completion
    requires inspection; this function never signals a saved PID.
    """
    if not math.isfinite(timeout) or timeout < 0:
        raise LoopError("reconciliation timeout must be finite and nonnegative")
    directory = directory.resolve()
    deadline = time.monotonic() + timeout
    records = []
    paths = []
    for parent, children, files in os.walk(directory, followlinks=False):
        relative = Path(parent).relative_to(directory).parts
        excluded = {".git"}
        if not relative:
            excluded.update({"build", "accepted", "controller"})
        elif len(relative) == 3 and relative[0] == "attempts":
            excluded.add("repo")
        # A task ID such as build is valid. Prune only exact source/cache
        # locations in the run layout, never those names at arbitrary depths.
        children[:] = [name for name in children if name not in excluded
                       and not (Path(parent) / name).is_symlink()]
        paths.extend(Path(parent) / name for name in files if name.endswith(LAUNCH_SUFFIX))
    for path in sorted(paths):
        with open_directory(path.parent) as journal, _open_lock_at(journal, _lock_path(path).name) as ownership:
            while True:
                try:
                    fcntl.flock(ownership.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= deadline:
                        raise EnvironmentBlocked(f"process watchdog still owns {path}; wait for cleanup")
                    time.sleep(0.05)
            record = _read_launch_at(journal, path.name)
            _completed_result(record, path)
            records.append(record | {"record_path": str(path)})
    return records


def _control_status(reader: int) -> str | None:
    if not select.select([reader], [], [], 0)[0]:
        return None
    return "stop requested" if os.read(reader, 32) else "supervisor exited"


def _exited_unreaped(child: subprocess.Popen) -> bool:
    # Keep the leader's PID reserved until every group signal is complete. A
    # poll()/wait() before killpg would permit PID/group reuse on natural exit.
    return os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None


def _ignore_term() -> None:
    # Used only by the standalone, single-threaded watchdog's guard child.
    # The real command is spawned separately and retains normal TERM behavior.
    signal.signal(signal.SIGTERM, signal.SIG_IGN)


def _cleanup_child(child: subprocess.Popen, guard: subprocess.Popen | None) -> None:
    try:
        os.killpg(child.pid, signal.SIGTERM)
        deadline = time.monotonic() + 1
        while not _exited_unreaped(child) and time.monotonic() < deadline:
            time.sleep(0.02)
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    child.wait()
    if guard:
        guard.wait()


def _verify_journal(directory: int, ownership: int) -> None:
    """Bind the inherited held lock to this directory's fixed journal slot."""
    folder = os.fstat(directory)
    if (not stat.S_ISDIR(folder.st_mode) or folder.st_uid != os.geteuid()
            or folder.st_mode & 0o022):
        raise EnvironmentBlocked("watchdog journal must be an owned private directory")
    held = os.fstat(ownership)
    named = os.stat(LAUNCH_LOCK, dir_fd=directory, follow_symlinks=False)
    if (not stat.S_ISREG(held.st_mode) or not stat.S_ISREG(named.st_mode)
            or held.st_nlink != 1 or named.st_nlink != 1
            or held.st_uid != os.geteuid() or held.st_mode & 0o022
            or (held.st_dev, held.st_ino) != (named.st_dev, named.st_ino)):
        raise EnvironmentBlocked("watchdog ownership lock does not match its journal")
    # The supervisor acquired this same open-file-description lock before spawn.
    # Another holder or a substituted lock must never authorize record writes.
    fcntl.flock(ownership, fcntl.LOCK_EX | fcntl.LOCK_NB)


def _verify_channels(directory: int, reader: int, prompt: int, output: int,
                     errors: int, ownership: int) -> None:
    if len({directory, reader, prompt, ownership, output}) != 5 or errors in {
            directory, reader, prompt, ownership}:
        raise EnvironmentBlocked("watchdog descriptors must have distinct roles")
    if (not stat.S_ISFIFO(os.fstat(reader).st_mode)
            or fcntl.fcntl(reader, fcntl.F_GETFL) & os.O_ACCMODE != os.O_RDONLY):
        raise EnvironmentBlocked("watchdog control descriptor must be a read pipe")
    lock = os.fstat(ownership)
    for fd in {prompt, output, errors}:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode)
                or (info.st_dev, info.st_ino) == (lock.st_dev, lock.st_ino)):
            raise EnvironmentBlocked("watchdog I/O descriptors must be regular files distinct from its lock")
    if fcntl.fcntl(prompt, fcntl.F_GETFL) & os.O_ACCMODE == os.O_WRONLY:
        raise EnvironmentBlocked("watchdog prompt descriptor is not readable")
    if any(fcntl.fcntl(fd, fcntl.F_GETFL) & os.O_ACCMODE == os.O_RDONLY
           for fd in {output, errors}):
        raise EnvironmentBlocked("watchdog output descriptor is not writable")


def _starting_record(record: dict) -> None:
    def finite(value):
        return type(value) in (int, float) and math.isfinite(value)

    argv, cwd = record.get("argv"), record.get("cwd")
    if (not re.fullmatch(r"[0-9a-f]{32}", record.get("launch_id", ""))
            or record.get("state") != "starting" or record.get("cleanup_confirmed") is not False
            or not isinstance(argv, list) or not argv
            or any(not isinstance(value, str) or "\0" in value for value in argv)
            or not isinstance(cwd, str) or not os.path.isabs(cwd) or "\0" in cwd
            or not finite(record.get("started")) or not 0 <= record["started"] <= time.monotonic()
            or not finite(record.get("timeout")) or record["timeout"] <= 0
            or type(record.get("max_log_bytes")) is not int or record["max_log_bytes"] <= 0):
        raise EnvironmentBlocked("invalid or already-started watchdog launch record")


def _write_launch(directory: int, ownership: int, record: dict) -> None:
    _verify_journal(directory, ownership)
    atomic_json_at(directory, LAUNCH_RECORD, record)


def _watchdog(directory: int, reader: int, prompt: int, output: int,
              errors: int, ownership: int) -> int:
    _verify_journal(directory, ownership)
    _verify_channels(directory, reader, prompt, output, errors, ownership)
    record = _read_launch_at(directory, LAUNCH_RECORD)
    _starting_record(record)
    started = record["started"]
    stopped = None
    child = None
    guard = None
    failure = None
    termination = []
    for signum in (signal.SIGTERM, signal.SIGINT):
        signal.signal(signum, lambda number, frame: termination.append(number))
    output_fds = list(dict.fromkeys([output, errors]))
    try:
        stopped = _control_status(reader)
        if not stopped:
            try:
                child = subprocess.Popen(record["argv"], cwd=record["cwd"], stdin=prompt,
                                         stdout=output, stderr=errors, process_group=0,
                                         close_fds=True)
            except OSError as error:
                record["spawn_error"] = f"cannot start {record['argv'][0]}: {error}"[:4096]
                stopped = "executable unavailable"
        if child:
            # Darwin returns EPERM when killpg sees only unreaped zombies. A
            # live guard makes both group signals well-defined while the real
            # leader's unreaped PID prevents reuse. Both children stay in this
            # watchdog's session, so the guard can join the worker's group.
            guard = subprocess.Popen(
                [sys.executable, "-B", "-c", "import time; time.sleep(86400)"],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                process_group=child.pid, preexec_fn=_ignore_term, close_fds=True,
            )
            record.update(state="running", watchdog_pid=os.getpid(), worker_pid=child.pid,
                          guard_pid=guard.pid)
            _write_launch(directory, ownership, record)
            while True:
                stopped = _control_status(reader)
                if not stopped and termination:
                    stopped = "watchdog termination requested"
                if not stopped and time.monotonic() - started >= record["timeout"]:
                    stopped = "deadline exceeded"
                if not stopped and sum(os.fstat(fd).st_size for fd in output_fds) > record["max_log_bytes"]:
                    stopped = "log limit exceeded"
                if stopped or _exited_unreaped(child):
                    break
                time.sleep(0.05)
    except BaseException as error:
        failure = f"process watchdog failed: {type(error).__name__}: {error}"[:4096]
        stopped = "watchdog failure"
    finally:
        if child:
            _cleanup_child(child, guard)
        remaining = record["max_log_bytes"]
        for fd in output_fds:
            size = os.fstat(fd).st_size
            if size > remaining:
                stopped = stopped or "log limit exceeded"
                os.ftruncate(fd, remaining)
            remaining = max(0, remaining - size)
        result = Result(tuple(record["argv"]), child.returncode if child else -1,
                        time.monotonic() - started, stopped)
        record.update(state="complete", cleanup_confirmed=True, result=asdict(result))
        if failure:
            record["watchdog_error"] = failure
        _write_launch(directory, ownership, record)
        # The parent owns another reference only until Popen returns. Keep this
        # one until the cleanup result is durable; never give it to the worker.
        os.close(ownership)
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 8 or sys.argv[1] != "--watchdog":
        raise SystemExit("process.py is an internal watchdog entry point")
    # No command-line value is ever used as a filesystem path or record name.
    if any(not value.isascii() or not value.isdecimal() or int(value) > 2**31 - 1
           for value in sys.argv[2:]):
        raise SystemExit("watchdog requires inherited descriptor numbers")
    try:
        raise SystemExit(_watchdog(*(int(value) for value in sys.argv[2:])))
    except (OSError, ValueError, LoopError) as error:
        raise SystemExit(f"watchdog refused or could not complete: {error}") from error
