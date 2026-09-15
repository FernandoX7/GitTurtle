"""Bounded JSON records accessed through pinned, private directory handles.

Path wrappers are for controller-selected locations. They do not grant authority
to an untrusted path: a child process should inherit an already-open directory.
Ancestor aliases (including macOS /var) are resolved before no-follow traversal;
the final directory and record are never followed through symlinks. Once open,
renaming or replacing an ancestor cannot redirect an operation. These checks do
not isolate hostile processes with the same user identity and directory access.
"""

from __future__ import annotations

from contextlib import contextmanager
import json
import os
from pathlib import Path
import secrets
import stat
from typing import Iterator


class LoopError(RuntimeError):
    """An actionable runner failure; candidates and evidence must be retained."""


class EnvironmentBlocked(LoopError):
    """Execution or cleanup could not be established; do not retry blindly."""


MAX_RECORD_BYTES = 8_000_000


def validate_basename(name: str) -> str:
    if (not isinstance(name, str) or not name or name in (".", "..")
            or any(character in name for character in ("/", "\\", "\0"))):
        raise LoopError("record name must be a single filename")
    return name


def _check_directory(fd: int, *, final: bool) -> None:
    info = os.fstat(fd)
    if not stat.S_ISDIR(info.st_mode):
        raise LoopError("record directory handle is not a directory")
    if info.st_uid not in ({os.geteuid()} if final else {0, os.geteuid()}):
        raise LoopError("record directory has an unexpected owner")
    writable = info.st_mode & (stat.S_IWGRP | stat.S_IWOTH)
    if writable and (final or not info.st_mode & stat.S_ISVTX):
        raise LoopError("record directory is writable by other users")


@contextmanager
def open_directory(path: Path, *, create: bool = False) -> Iterator[int]:
    """Open an owned directory, creating missing components privately if asked."""
    # Only ancestors may be canonicalized: resolving path itself would accept a
    # symlink for the final directory. Traverse the resulting path by descriptor
    # so a concurrent symlink replacement after resolution fails closed.
    path = Path(os.path.abspath(path))
    try:
        canonical = path.parent.resolve() / path.name
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        current = os.open(canonical.anchor, flags)
        try:
            _check_directory(current, final=not canonical.parts[1:])
            for index, component in enumerate(canonical.parts[1:]):
                try:
                    child = os.open(component, flags, dir_fd=current)
                except FileNotFoundError:
                    if not create:
                        raise
                    try:
                        os.mkdir(component, 0o700, dir_fd=current)
                    except FileExistsError:
                        pass
                    child = os.open(component, flags, dir_fd=current)
                os.close(current)
                current = child
                _check_directory(current, final=index == len(canonical.parts) - 2)
            yield current
        finally:
            os.close(current)
    except (OSError, RuntimeError) as error:
        if isinstance(error, LoopError):
            raise
        raise LoopError(f"cannot access record directory {path}: {error}") from error


def _open_record(directory_fd: int, name: str) -> tuple[int, os.stat_result]:
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                 dir_fd=directory_fd)
    try:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                or info.st_uid != os.geteuid()
                or info.st_mode & (stat.S_IWGRP | stat.S_IWOTH)):
            raise LoopError(f"record must be an owned, singly-linked regular file: {name!r}")
        return fd, info
    except BaseException:
        os.close(fd)
        raise


def read_json_at(directory_fd: int, name: str, *, max_bytes: int = MAX_RECORD_BYTES) -> dict:
    """Read and validate through one descriptor, including a growing-file bound."""
    validate_basename(name)
    if type(max_bytes) is not int or max_bytes <= 0:
        raise LoopError("record byte limit must be a positive integer")
    try:
        _check_directory(directory_fd, final=True)
        fd, info = _open_record(directory_fd, name)
        try:
            if info.st_size > max_bytes:
                raise LoopError(f"record exceeds size limit: {name!r}")
            data = bytearray()
            while True:
                chunk = os.read(fd, min(64 * 1024, max_bytes + 1 - len(data)))
                if not chunk:
                    break
                data.extend(chunk)
                if len(data) > max_bytes:
                    raise LoopError(f"record exceeds size limit: {name!r}")
        finally:
            os.close(fd)
        value = json.loads(data.decode("utf-8"))
        if not isinstance(value, dict):
            raise LoopError(f"expected an object record: {name!r}")
        return value
    except (ValueError, UnicodeError, OSError, RecursionError) as error:
        raise LoopError(f"cannot read JSON record {name!r}: {error}") from error


def _target_identity(directory_fd: int, name: str) -> tuple[int, int] | None:
    try:
        fd, info = _open_record(directory_fd, name)
    except FileNotFoundError:
        return None
    try:
        return info.st_dev, info.st_ino
    finally:
        os.close(fd)


def atomic_json_at(directory_fd: int, name: str, value: object) -> None:
    """Durably replace one record without reconstructing a filesystem path.

    Replacement never follows the old leaf. The identity recheck rejects observable
    changes before rename; it is not a compare-and-swap against a hostile same-user
    writer. All writes and cleanup remain inside the pinned directory even then.
    """
    validate_basename(name)
    temporary = ".write-" + secrets.token_hex(16)
    fd = None
    created = False
    try:
        _check_directory(directory_fd, final=True)
        original = _target_identity(directory_fd, name)
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
                     | os.O_CLOEXEC, 0o600, dir_fd=directory_fd)
        created = True
        with os.fdopen(fd, "wb") as stream:
            fd = None
            size = 0
            encoder = json.JSONEncoder(indent=2, sort_keys=True)
            for chunk in encoder.iterencode(value):
                encoded = chunk.encode("utf-8")
                size += len(encoded)
                if size + 1 > MAX_RECORD_BYTES:
                    raise LoopError(f"record exceeds size limit: {name!r}")
                stream.write(encoded)
            stream.write(b"\n")
            stream.flush()
            os.fsync(stream.fileno())
        if _target_identity(directory_fd, name) != original:
            raise LoopError(f"record changed during replacement: {name!r}")
        os.replace(temporary, name, src_dir_fd=directory_fd, dst_dir_fd=directory_fd)
        created = False
        os.fsync(directory_fd)
    except (OSError, TypeError, ValueError, UnicodeError, RecursionError) as error:
        raise LoopError(f"cannot write JSON record {name!r}: {error}") from error
    finally:
        if fd is not None:
            os.close(fd)
        if created:
            try:
                os.unlink(temporary, dir_fd=directory_fd)
            except FileNotFoundError:
                pass


def read_json(path: Path, *, max_bytes: int = MAX_RECORD_BYTES) -> dict:
    validate_basename(path.name)
    with open_directory(path.parent) as directory:
        return read_json_at(directory, path.name, max_bytes=max_bytes)


def atomic_json(path: Path, value: object) -> None:
    validate_basename(path.name)
    with open_directory(path.parent, create=True) as directory:
        atomic_json_at(directory, path.name, value)
