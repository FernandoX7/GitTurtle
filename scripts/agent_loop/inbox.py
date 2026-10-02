"""The run inbox: operator requests that reach a controller holding the run lock.

`note` and `attest` apply directly when the run lock is free. While a running
controller holds it, they validate what they can and queue the request here as
a private record named `<UTC>-<id>.json`, after copying any artifact it carries
beside it, so a visible request never names a missing file. The controller
applies or refuses each request under its lock at its next safe point, in name
order, and moves it to `done/` with an outcome record. The rules for each kind
live with the run state in runner.py; this module only stores and moves them.
"""

from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import os
from pathlib import Path
import re
import stat

from .records import LoopError, atomic_json_at, open_directory, read_json_at


INBOX = "inbox"
DONE = "done"
# A queue that nothing drains must not grow without bound.
MAX_PENDING = 64
RECENT_OUTCOMES = 10
REQUEST_ID = re.compile(r"[0-9a-f]{32}")
# A microsecond UTC stamp first, so name order is submission order.
REQUEST_NAME = re.compile(r"(\d{8}T\d{12}Z)-([0-9a-f]{32})\.json")
ARTIFACT_SUFFIX = ".evidence"
OUTCOME_SUFFIX = ".outcome.json"


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _open_supplied(source: Path, limit: int, what: str) -> tuple[int, os.stat_result]:
    """Open an operator-supplied file without following a final symlink."""
    try:
        fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    except OSError as error:
        raise LoopError(f"{what} must be a readable regular file, not a symlink: {source}") from error
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            raise LoopError(f"{what} must be a regular file, not a symlink, of at most {limit} bytes: {source}")
        return fd, info
    except BaseException:
        os.close(fd)
        raise


def _chunks(fd: int, limit: int, what: str):
    """Yield the file's bytes, refusing one that grows past `limit` while read."""
    total = 0
    while chunk := os.read(fd, min(1024 * 1024, limit + 1 - total)):
        total += len(chunk)
        if total > limit:
            raise LoopError(f"{what} grew past {limit} bytes while it was read")
        yield chunk


def read_supplied(source: Path, limit: int, what: str) -> bytes:
    fd, _ = _open_supplied(source, limit, what)
    try:
        return b"".join(_chunks(fd, limit, what))
    except OSError as error:
        raise LoopError(f"cannot read {what}: {error}") from error
    finally:
        os.close(fd)


def copy_supplied_at(source: Path, directory_fd: int, name: str, limit: int, what: str) -> str:
    """Copy `source` into a new private file in an opened directory; return its sha256."""
    fd, _ = _open_supplied(source, limit, what)
    hasher = hashlib.sha256()
    created = False
    try:
        target = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                         0o600, dir_fd=directory_fd)
        created = True
        with os.fdopen(target, "wb") as stream:
            for chunk in _chunks(fd, limit, what):
                hasher.update(chunk)
                stream.write(chunk)
            stream.flush()
            os.fsync(stream.fileno())
        os.fsync(directory_fd)
        created = False
        return hasher.hexdigest()
    except OSError as error:
        raise LoopError(f"cannot copy {what}: {error}") from error
    finally:
        os.close(fd)
        if created:
            try:
                os.unlink(name, dir_fd=directory_fd)
            except FileNotFoundError:
                pass


def copy_supplied(source: Path, destination: Path, limit: int, what: str) -> str:
    with open_directory(destination.parent, create=True) as directory:
        return copy_supplied_at(source, directory, destination.name, limit, what)


def _names(directory_fd: int) -> list[str]:
    return sorted(name for name in os.listdir(directory_fd) if REQUEST_NAME.fullmatch(name))


def pending(run: Path) -> list[str]:
    """Queued request names in the order they are applied."""
    if not os.path.lexists(run / INBOX):
        return []
    with open_directory(run / INBOX) as directory:
        return _names(directory)


def submit(run: Path, request: dict, evidence: Path | None, limit: int, what: str) -> str:
    """Queue one request, copying its artifact first; return the request's name."""
    if not isinstance(request.get("id"), str) or not REQUEST_ID.fullmatch(request["id"]):
        raise LoopError("an inbox request needs a 32-digit hexadecimal id")
    with open_directory(run / INBOX, create=True) as directory:
        if len(_names(directory)) >= MAX_PENDING:
            raise LoopError(f"the run inbox already holds {MAX_PENDING} pending requests; wait for the controller to apply them")
        stem = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ") + "-" + request["id"]
        request = dict(request)
        if evidence is not None:
            request["artifact"] = stem + ARTIFACT_SUFFIX
            request["sha256"] = copy_supplied_at(evidence, directory, request["artifact"], limit, what)
        atomic_json_at(directory, stem + ".json", request)
    return stem + ".json"


def read(run: Path, name: str) -> dict:
    """A queued request, checked to be the one its name identifies."""
    match = REQUEST_NAME.fullmatch(name)
    if not match:
        raise LoopError(f"not an inbox request name: {name!r}")
    with open_directory(run / INBOX) as directory:
        request = read_json_at(directory, name)
    if request.get("version") != 1 or request.get("id") != match[2]:
        raise LoopError("inbox request does not match its name or version")
    if "artifact" in request and request["artifact"] != name[: -len(".json")] + ARTIFACT_SUFFIX:
        raise LoopError("inbox request names an artifact other than its own")
    return request


def artifact(run: Path, request: dict) -> Path:
    return run / INBOX / request["artifact"]


def finish(run: Path, name: str, request: dict | None, outcome: str) -> None:
    """Record the outcome, then move the artifact and the request into `done/`.

    The request moves last, so a crash before it leaves the request pending and
    ingestion runs again; applying a request is idempotent by its id.
    """
    stem = name[: -len(".json")]
    request = request or {}
    record = {
        "version": 1, "request": name, "kind": request.get("kind"), "task": request.get("task"),
        "outcome": outcome, "ingested_at": _now(),
    }
    with open_directory(run / INBOX) as directory, open_directory(run / INBOX / DONE, create=True) as done:
        atomic_json_at(done, stem + OUTCOME_SUFFIX, record)
        try:
            for moved in (stem + ARTIFACT_SUFFIX, name):
                try:
                    os.replace(moved, moved, src_dir_fd=directory, dst_dir_fd=done)
                except FileNotFoundError:
                    if moved == name:
                        raise
            os.fsync(done)
            os.fsync(directory)
        except OSError as error:
            raise LoopError(f"cannot move inbox request {name!r} to done: {error}") from error


def summary(run: Path) -> dict:
    """Pending requests and the latest outcomes, for `status`."""
    result: dict[str, list] = {"pending": [], "recent": []}
    if not os.path.lexists(run / INBOX):
        return result
    with open_directory(run / INBOX) as directory:
        for name in _names(directory):
            try:
                request = read_json_at(directory, name)
                result["pending"].append({"request": name, "kind": request.get("kind"), "task": request.get("task"), "at": request.get("at")})
            except LoopError as error:
                result["pending"].append({"request": name, "error": str(error)})
    if not os.path.lexists(run / INBOX / DONE):
        return result
    with open_directory(run / INBOX / DONE) as done:
        outcomes = sorted(name for name in os.listdir(done) if name.endswith(OUTCOME_SUFFIX))
        for name in outcomes[-RECENT_OUTCOMES:]:
            try:
                result["recent"].append(read_json_at(done, name))
            except LoopError as error:
                result["recent"].append({"record": name, "error": str(error)})
    return result
