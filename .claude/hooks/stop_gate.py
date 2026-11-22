#!/usr/bin/env python3
"""Stop/SubagentStop hook: run the fast gate when Rust files changed.

Exit 2 blocks the turn with the gate report so red checks are fixed before
claiming completion. Gives up after two consecutive failures per session and
resets the counter once the gate passes. A cold target directory, a missing
tool, or a non-Rust change never blocks and never starts a long build.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys

MAX_ATTEMPTS = 2
GATE_TIMEOUT = 150
RUST_SUFFIXES = (".rs", "Cargo.toml", "Cargo.lock")
RUST_ROOTS = ("crates/", "vendor/", "Cargo.toml", "Cargo.lock")


def _is_rust_path(path: str) -> bool:
    return path.startswith(RUST_ROOTS) and path.endswith(RUST_SUFFIXES)


def rust_changes(cwd: Path) -> bool:
    try:
        status = subprocess.run(
            ["git", "status", "--porcelain", "--untracked-files=all"],
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        ).stdout
    except (OSError, subprocess.SubprocessError):
        return False
    for line in status.splitlines():
        path = line[3:].strip().split(" -> ")[-1]
        if _is_rust_path(path):
            return True
    # Also detect committed Rust edits on a feature branch ahead of main/origin/main.
    for base in ("main", "origin/main"):
        try:
            diff = subprocess.run(
                ["git", "diff", "--name-only", f"{base}...HEAD"],
                cwd=cwd,
                capture_output=True,
                text=True,
                timeout=20,
                check=False,
            )
        except (OSError, subprocess.SubprocessError):
            break
        if diff.returncode == 0:
            if any(_is_rust_path(line.strip()) for line in diff.stdout.splitlines() if line.strip()):
                return True
            break
    return False


def resolve_target_dir(cwd: Path) -> Path:
    configured = os.environ.get("CARGO_TARGET_DIR")
    if configured:
        return Path(configured)
    worktree_target = cwd / ".local" / "target"
    if worktree_target.is_dir():
        return worktree_target
    return cwd / "target"


def target_is_warm(cwd: Path) -> bool:
    target = resolve_target_dir(cwd)
    return (target / "debug" / "deps").is_dir() and (target / ".rustc_info.json").exists()


def attempt_counter(data: dict, cwd: Path) -> Path:
    base = data.get("scratchpad_dir") or str(cwd / ".local" / "gate")
    directory = Path(base)
    directory.mkdir(parents=True, exist_ok=True)
    return directory / f"stop-gate-attempts-{data.get('session_id', 'session')}"


def main() -> int:
    if os.environ.get("GITTURTLE_SKIP_STOP_GATE") == "1":
        return 0
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        data = {}
    cwd = Path(data.get("cwd") or os.getcwd())
    gate = cwd / "scripts" / "gate.py"
    if not gate.exists() or not rust_changes(cwd):
        return 0
    if not target_is_warm(cwd):
        sys.stderr.write(
            "stop gate: the Cargo target directory is cold, so the hook did not build. "
            "Run `python3 scripts/gate.py fast` yourself before calling the change done.\n"
        )
        return 0
    counter = attempt_counter(data, cwd)
    attempts = int(counter.read_text().strip() or 0) if counter.exists() else 0
    if attempts >= MAX_ATTEMPTS:
        return 0
    env = dict(os.environ)
    env.pop("RUSTUP_TOOLCHAIN", None)
    if not env.get("CARGO_TARGET_DIR"):
        env["CARGO_TARGET_DIR"] = str(resolve_target_dir(cwd))
    try:
        result = subprocess.run(
            [sys.executable, str(gate), "fast", "--quiet"],
            cwd=cwd,
            env=env,
            capture_output=True,
            text=True,
            timeout=GATE_TIMEOUT,
            check=False,
        )
    except subprocess.TimeoutExpired:
        counter.write_text(str(attempts + 1))
        sys.stderr.write(
            "stop gate: the fast gate did not finish in time; run `python3 scripts/gate.py fast` "
            "yourself before finishing.\n"
        )
        return 0
    except OSError as error:
        sys.stderr.write(f"stop gate: could not run the fast gate: {error}\n")
        return 0
    if result.returncode == 0:
        counter.unlink(missing_ok=True)
        return 0
    if result.returncode != 1:
        sys.stderr.write((result.stdout or "")[-2000:] + (result.stderr or "")[-1000:])
        return 0
    counter.write_text(str(attempts + 1))
    report = (result.stdout or "").strip()
    sys.stderr.write(
        "The fast gate is red. Fix the failing stage, rerun `python3 scripts/gate.py fast`, "
        "and only then finish.\n\n" + report[-6000:] + "\n"
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
