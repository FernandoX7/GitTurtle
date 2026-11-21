#!/usr/bin/env python3
"""Portable build concurrency slot and quiet-host lock for local builds.

Replaces `.local/slot.sh` without leaking lock file descriptors into child
processes (all lock descriptors are opened with O_CLOEXEC).

Usage:
    python3 scripts/dev/build_slot.py run -- cargo test --locked ...
    python3 scripts/dev/build_slot.py quiet-on
    python3 scripts/dev/build_slot.py quiet-off
    python3 scripts/dev/build_slot.py status
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

try:
    import fcntl
except ImportError:  # pragma: no cover - non-Unix fallback
    fcntl = None  # type: ignore[assignment]

DEFAULT_SLOTS = 3
QUIET_FILE = "quiet"


def default_slot_dir(root: Path | None = None) -> Path:
    if root is not None:
        return root / ".local" / "build-slots"
    probe = subprocess.run(
        ["git", "rev-parse", "--git-common-dir"],
        text=True,
        capture_output=True,
        check=False,
    )
    if probe.returncode == 0 and probe.stdout.strip():
        common = Path(probe.stdout.strip())
        if not common.is_absolute():
            common = (Path.cwd() / common).resolve()
        else:
            common = common.resolve()
        repo = common.parent if common.name == ".git" else Path.cwd().resolve()
        return repo / ".local" / "build-slots"
    return Path.cwd().resolve() / ".local" / "build-slots"


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be >= 1")
    return parsed


def try_acquire_slot(slot_dir: Path, slots: int) -> tuple[int, int] | None:
    """Attempt a non-blocking flock on slots 1..=slots; returns (slot_num, fd) or None."""
    if slots < 1:
        raise ValueError(f"slots must be >= 1, got {slots}")
    slot_dir.mkdir(parents=True, exist_ok=True)
    flags = os.O_CREAT | os.O_RDWR | getattr(os, "O_CLOEXEC", 0)
    for index in range(1, slots + 1):
        fd = os.open(slot_dir / str(index), flags, 0o600)
        try:
            os.set_inheritable(fd, False)
            if fcntl is None:  # pragma: no cover
                return index, fd
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return index, fd
        except BlockingIOError:
            os.close(fd)
        except BaseException:
            os.close(fd)
            raise
    return None


def acquire_slot(
    slot_dir: Path,
    *,
    slots: int = DEFAULT_SLOTS,
    poll_seconds: float = 2.0,
    quiet_seconds: float = 5.0,
    timeout_seconds: float | None = None,
) -> tuple[int, int]:
    deadline = None if timeout_seconds is None else time.monotonic() + timeout_seconds
    quiet_marker = slot_dir / QUIET_FILE
    while True:
        if quiet_marker.exists():
            if deadline is not None and time.monotonic() >= deadline:
                raise TimeoutError(f"host quiet lock still active: {quiet_marker}")
            sleep_for = quiet_seconds
            if deadline is not None:
                sleep_for = min(sleep_for, max(0.01, deadline - time.monotonic()))
            time.sleep(sleep_for)
            continue
        acquired = try_acquire_slot(slot_dir, slots)
        if acquired is not None:
            return acquired
        if deadline is not None and time.monotonic() >= deadline:
            raise TimeoutError(f"all {slots} build slots busy in {slot_dir}")
        sleep_for = poll_seconds
        if deadline is not None:
            sleep_for = min(sleep_for, max(0.01, deadline - time.monotonic()))
        time.sleep(sleep_for)


def release_slot(fd: int) -> None:
    try:
        if fcntl is not None:
            fcntl.flock(fd, fcntl.LOCK_UN)
    finally:
        os.close(fd)


def cmd_run(args: argparse.Namespace) -> int:
    command = list(args.command)
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        print("error: missing command to run", file=sys.stderr)
        return 2
    slot_dir = args.slot_dir or default_slot_dir()
    try:
        _slot, fd = acquire_slot(
            slot_dir,
            slots=args.slots,
            poll_seconds=args.poll_seconds,
            quiet_seconds=args.quiet_seconds,
            timeout_seconds=args.timeout,
        )
    except TimeoutError as error:
        print(f"error: {error}", file=sys.stderr)
        return 124
    try:
        completed = subprocess.run(command, check=False, close_fds=True)
        return completed.returncode
    finally:
        release_slot(fd)


def cmd_quiet_on(args: argparse.Namespace) -> int:
    slot_dir = args.slot_dir or default_slot_dir()
    slot_dir.mkdir(parents=True, exist_ok=True)
    marker = slot_dir / QUIET_FILE
    marker.write_text(f"pid={os.getpid()}\n", encoding="utf-8")
    print(f"QUIET_ON={marker}")
    return 0


def cmd_quiet_off(args: argparse.Namespace) -> int:
    slot_dir = args.slot_dir or default_slot_dir()
    marker = slot_dir / QUIET_FILE
    marker.unlink(missing_ok=True)
    print(f"QUIET_OFF={marker}")
    return 0


def cmd_status(args: argparse.Namespace) -> int:
    slot_dir = args.slot_dir or default_slot_dir()
    quiet = (slot_dir / QUIET_FILE).exists()
    busy = 0
    if slot_dir.is_dir():
        flags = os.O_CREAT | os.O_RDWR | getattr(os, "O_CLOEXEC", 0)
        for index in range(1, args.slots + 1):
            path = slot_dir / str(index)
            if not path.exists():
                continue
            fd = os.open(path, flags, 0o600)
            try:
                if fcntl is not None:
                    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    fcntl.flock(fd, fcntl.LOCK_UN)
            except BlockingIOError:
                busy += 1
            finally:
                os.close(fd)
    print(f"slots={busy}/{args.slots} quiet={'yes' if quiet else 'no'} dir={slot_dir}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Portable build concurrency slot manager.")
    parser.add_argument("--slot-dir", type=Path, default=None, help="Override slot lock directory")
    parser.add_argument("--slots", type=positive_int, default=DEFAULT_SLOTS, help="Maximum concurrent build slots")
    sub = parser.add_subparsers(dest="subcommand", required=True)

    p_run = sub.add_parser("run", help="Run a command while holding a build slot")
    p_run.add_argument("--poll-seconds", type=float, default=2.0)
    p_run.add_argument("--quiet-seconds", type=float, default=5.0)
    p_run.add_argument("--timeout", type=float, default=None)
    p_run.add_argument("command", nargs=argparse.REMAINDER)
    p_run.set_defaults(func=cmd_run)

    p_on = sub.add_parser("quiet-on", help="Hold new builds while benchmarks run")
    p_on.set_defaults(func=cmd_quiet_on)

    p_off = sub.add_parser("quiet-off", help="Release the quiet-host lock")
    p_off.set_defaults(func=cmd_quiet_off)

    p_status = sub.add_parser("status", help="Show active build slots and quiet state")
    p_status.set_defaults(func=cmd_status)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
