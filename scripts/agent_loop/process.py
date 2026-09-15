"""Bounded subprocess ownership and durable local records."""

from __future__ import annotations

import hashlib
import json
import math
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import time
import uuid
from contextlib import ExitStack
from dataclasses import asdict, dataclass
from typing import Callable

import fcntl


class LoopError(RuntimeError):
    """An actionable runner failure; candidates and evidence must be retained."""


class EnvironmentBlocked(LoopError):
    """Execution or cleanup could not be established; do not retry blindly."""


MAX_LAUNCH_BYTES = 256 * 1024
LAUNCH_SUFFIX = ".process-launch.json"


def atomic_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".write-", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def read_json(path: Path) -> dict:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 8_000_000:
        raise LoopError(f"invalid or oversized record: {path}")
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (ValueError, UnicodeError) as error:
        raise LoopError(f"invalid JSON record: {path}") from error
    if not isinstance(data, dict):
        raise LoopError(f"expected an object: {path}")
    return data


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
    cwd, log = cwd.resolve(), log.resolve()
    log.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    launch_id = uuid.uuid4().hex
    record_path = log.with_name(f".{log.name}.{launch_id}{LAUNCH_SUFFIX}")
    record = {
        "version": 1, "launch_id": launch_id, "state": "starting",
        "argv": argv, "cwd": str(cwd), "supervisor_pid": os.getpid(),
        "started": started, "timeout": timeout, "max_log_bytes": max_log_bytes,
        "cleanup_confirmed": False,
    }
    if len(json.dumps(record).encode("utf-8")) > (MAX_LAUNCH_BYTES - 16384) // 2:
        raise LoopError("process launch arguments exceed the durable record limit")
    with ExitStack() as stack:
        # Inherit an already-held lock so even a supervisor crash during launch
        # cannot expose an unlocked record while a watchdog is starting.
        ownership = stack.enter_context(_open_lock(_lock_path(record_path), create=True))
        fcntl.flock(ownership.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        atomic_json(record_path, record)
        prompt = stack.enter_context(tempfile.TemporaryFile())
        output = stack.enter_context(log.open("wb"))
        errors = output
        if stderr_path is not None and stderr_path.resolve() != log:
            stderr_path.parent.mkdir(parents=True, exist_ok=True)
            errors = stack.enter_context(stderr_path.open("wb"))
        if stdin is not None:
            prompt.write(stdin.encode("utf-8"))
            prompt.seek(0)
        reader, writer = os.pipe()
        watchdog = None
        try:
            watchdog = subprocess.Popen(
                [sys.executable, "-B", str(Path(__file__).resolve()), "--watchdog",
                 str(record_path), str(reader), str(prompt.fileno()), str(output.fileno()),
                 str(errors.fileno()), str(ownership.fileno())],
                cwd=cwd, stdin=subprocess.DEVNULL, stdout=errors, stderr=errors,
                env=env, start_new_session=True,
                pass_fds=tuple({reader, prompt.fileno(), output.fileno(), errors.fileno(), ownership.fileno()}),
            )
        except OSError as error:
            record.update(state="complete", cleanup_confirmed=True,
                          spawn_error=f"cannot start process watchdog: {error}"[:4096],
                          result=asdict(Result(tuple(argv), -1, time.monotonic() - started,
                                               "watchdog unavailable")))
            atomic_json(record_path, record)
            os.close(writer)
            raise EnvironmentBlocked(f"cannot start process watchdog: {error}") from error
        finally:
            os.close(reader)
            # Closing this copy must not explicitly unlock the shared lock.
            ownership.close()
        try:
            while watchdog.poll() is None:
                if stop():
                    try:
                        os.write(writer, b"stop")
                    except BrokenPipeError:
                        pass
                    break
                time.sleep(0.05)
        finally:
            os.close(writer)
            try:
                watchdog.wait(timeout=3)
            except subprocess.TimeoutExpired as error:
                raise EnvironmentBlocked(f"watchdog cleanup is still active; inspect {record_path}") from error
        completed = _read_launch(record_path)
        result = _completed_result(completed, record_path)
        if completed.get("spawn_error") or completed.get("watchdog_error"):
            raise EnvironmentBlocked(str(completed.get("spawn_error") or completed.get("watchdog_error")))
        return result


def _lock_path(record: Path) -> Path:
    return record.with_suffix(".lock")


def _open_lock(path: Path, *, create: bool = False):
    flags = os.O_RDWR | os.O_NOFOLLOW | (os.O_CREAT if create else 0)
    try:
        return os.fdopen(os.open(path, flags, 0o600), "r+b")
    except OSError as error:
        raise EnvironmentBlocked(f"cannot inspect process ownership lock {path}: {error}") from error


def _read_launch(path: Path) -> dict:
    try:
        if path.stat().st_size > MAX_LAUNCH_BYTES:
            raise LoopError("launch record exceeds size limit")
        record = read_json(path)
        if record.get("version") != 1 or not isinstance(record.get("launch_id"), str):
            raise LoopError("unknown launch record format")
        return record
    except (OSError, LoopError) as error:
        raise EnvironmentBlocked(f"cannot establish process state from {path}: {error}") from error


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
        with _open_lock(_lock_path(path)) as ownership:
            while True:
                try:
                    fcntl.flock(ownership.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= deadline:
                        raise EnvironmentBlocked(f"process watchdog still owns {path}; wait for cleanup")
                    time.sleep(0.05)
            record = _read_launch(path)
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


def _watchdog(record_path: Path, reader: int, prompt: int, output: int,
              errors: int, ownership: int) -> int:
    record = _read_launch(record_path)
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
            atomic_json(record_path, record)
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
        atomic_json(record_path, record)
        # The parent owns another reference only until Popen returns. Keep this
        # one until the cleanup result is durable; never give it to the worker.
        os.close(ownership)
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 8 or sys.argv[1] != "--watchdog":
        raise SystemExit("process.py is an internal watchdog entry point")
    raise SystemExit(_watchdog(Path(sys.argv[2]), *(int(value) for value in sys.argv[3:])))
