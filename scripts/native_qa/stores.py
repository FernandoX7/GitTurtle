"""Preference stores seeded into a launch's XDG_CONFIG_HOME before the app starts, and read back during it.

A generated store selects a built-in theme with Follow system off, so the
desktop's colour scheme cannot change the frame, and can list projects.
Stores with custom themes are supplied as files and copied byte for byte:
their tokens are resolved from the exercised source, which this module does
not mirror. `snapshot` reads a store the app writes, once it has stopped
changing (a `store_snapshot` step).
"""

from __future__ import annotations

import hashlib
import json
import os
import stat
import time
from pathlib import Path

# `STORE_VERSION` in crates/app/src/preferences.rs; test_runenv's StoreTest checks it.
STORE_VERSION = 6
# The preference store, relative to a launch's run directory (`runenv.RunDirs.preferences`).
PREFERENCES = "config/gitturtle/preferences.json"
SNAPSHOT_QUIET = 1.0    # seconds a store must stay unchanged before a snapshot reads it
SNAPSHOT_WITHIN = 10.0  # seconds a snapshot waits for that
SNAPSHOT_POLL = 0.05


def parse_project(spec: str) -> tuple[str, str | None]:
    """`PATH` or `PATH=NAME`; the path must be absolute so the store never depends on the cwd."""
    path, _, name = spec.partition("=")
    if not Path(path).is_absolute():
        raise SystemExit(f"refusing: project path {path!r} must be absolute")
    return path, (name or None)


def store_text(theme: str = "midnight", projects: list[str] = (), follow_system: bool = False,
               settings: dict | None = None) -> bytes:
    store: dict = {"version": STORE_VERSION, "settings": {"theme": theme, "follow_system": follow_system}}
    store["settings"].update(settings or {})
    parsed = [parse_project(spec) for spec in projects]
    if parsed:
        store["recent_repositories"] = [path for path, _ in parsed]
        store["project_library"] = [{"project": {"path": path}} for path, _ in parsed]
        names = [{"path": path, "name": name} for path, name in parsed if name]
        if names:
            store["project_names"] = names
    return json.dumps(store).encode()


def load_file(path: Path) -> bytes:
    """A supplied store, validated as JSON with a version but otherwise unchanged."""
    data = Path(path).read_bytes()
    try:
        version = json.loads(data).get("version")
    except (ValueError, AttributeError) as error:
        raise SystemExit(f"refusing: {path} is not a JSON preference store ({error})")
    if not isinstance(version, int):
        raise SystemExit(f"refusing: {path} has no integer version")
    return data


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


# ---------- snapshots ----------
def read_state(root: Path, relative: str) -> tuple[bytes, int, int] | None:
    """The bytes, mtime and inode of the regular file `root/relative`; None while it or a directory on the way is
    absent. Each part is opened relative to the one before without following a link, so a symbolic link anywhere
    below `root`, or a final entry that is not a regular file, raises OSError instead of being read."""
    parts = relative.split("/")
    if relative.startswith("/") or any(part in ("", ".", "..") for part in parts):
        raise OSError(f"{relative!r} is not a relative path without empty, . or .. parts")
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
    try:
        for depth, part in enumerate(parts[:-1]):
            try:
                child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            except FileNotFoundError:
                return None
            except OSError as error:
                raise OSError(f"{'/'.join(parts[:depth + 1])} is not a plain directory ({error})") from None
            os.close(fd)
            fd = child
        try:
            info = os.stat(parts[-1], dir_fd=fd, follow_symlinks=False)
        except FileNotFoundError:
            return None
        if not stat.S_ISREG(info.st_mode):
            raise OSError(f"{relative} is not a regular file")
        try:
            store = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
        except FileNotFoundError:
            return None  # replaced between the two calls: the next poll reads the new file
    finally:
        os.close(fd)
    try:
        info = os.fstat(store)
        chunks = []
        while chunk := os.read(store, 1 << 16):
            chunks.append(chunk)
    finally:
        os.close(store)
    return b"".join(chunks), info.st_mtime_ns, info.st_ino


def snapshot(root: Path, relative: str, quiet: float = SNAPSHOT_QUIET, within: float = SNAPSHOT_WITHIN,
             poll: float = SNAPSHOT_POLL, clock=time.monotonic, sleep=time.sleep) -> dict:
    """`root/relative` once its bytes, mtime and inode have not changed for `quiet` seconds, waiting up to `within`.

    Returns whether it settled, how long that took, and its existence, sha256,
    size, mtime, inode and parsed JSON (None with `json_error` when it is not
    JSON). A store that never settles is returned as read last, `stable`
    false. A symbolic link anywhere below `root` on the way, or a final entry
    that is not a regular file, raises OSError (`read_state`).
    """
    start = clock()
    current, since = read_state(root, relative), start
    while True:
        now = clock()
        if now - since >= quiet:
            stable = True
            break
        if now - start >= within:
            stable = False
            break
        sleep(poll)
        later = read_state(root, relative)
        if later != current:
            current, since = later, clock()
    entry = dict(stable=stable, waited_s=round(clock() - start, 3), quiet_s=quiet, exists=current is not None,
                 sha256=None, bytes=None, mtime_ns=None, inode=None, json=None)
    if current is not None:
        data, mtime, inode = current
        entry.update(sha256=sha256(data), bytes=len(data), mtime_ns=mtime, inode=inode)
        try:
            entry["json"] = json.loads(data)
        except ValueError as error:
            entry["json_error"] = str(error)
    return entry
