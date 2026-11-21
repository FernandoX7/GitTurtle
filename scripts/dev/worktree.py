#!/usr/bin/env python3
"""Btrfs/APFS Copy-on-Write worktree and Cargo target manager for GitTurtle.

Commands:
    python3 scripts/dev/worktree.py new <name> [base] [--branch BRANCH | --detach] [--full] [--no-seed]
    python3 scripts/dev/worktree.py warm [--release]
    python3 scripts/dev/worktree.py rm <name> [--delete-branch] [--force]
    python3 scripts/dev/worktree.py gc [--days 14] [--prune]
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
SPARSE_PATTERNS = ("/*", "!/docs/evidence/", "!/docs/benchmarks/")
PROTECTED_LOCAL_ENTRIES = frozenset({"target-base", "privacy", "build-slots", "wt"})


class WorktreeError(RuntimeError):
    """Raised on invalid arguments or failed git/filesystem operations."""


def validate_name(name: str) -> str:
    if not NAME_RE.fullmatch(name) or ".." in name:
        raise WorktreeError(
            f"invalid worktree name {name!r}: use 1-64 ASCII letters, digits, '.', '_', or '-' without '..'"
        )
    return name


def run_git(cwd: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        ["git", *args],
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}"
        raise WorktreeError(f"git {' '.join(args)} failed: {detail}")
    return result


def find_main_root(start: Path | None = None) -> Path:
    cwd = (start or Path.cwd()).resolve()
    common = run_git(cwd, "rev-parse", "--git-common-dir").stdout.strip()
    common_path = Path(common)
    if not common_path.is_absolute():
        common_path = (cwd / common_path).resolve()
    else:
        common_path = common_path.resolve()
    if common_path.name == ".git":
        return common_path.parent
    top = run_git(cwd, "rev-parse", "--show-toplevel").stdout.strip()
    return Path(top).resolve()


def validate_ref_arg(label: str, value: str) -> str:
    if not value or value.startswith("-") or any(c in value for c in "\r\n\0"):
        raise WorktreeError(f"invalid {label} {value!r}")
    return value


def clone_target_cow(src: Path, dst: Path, *, platform: str | None = None) -> str:
    """Seed dst from src using CoW reflinks when supported; never does a full byte copy."""
    plat = platform or sys.platform
    if not src.is_dir():
        dst.mkdir(parents=True, exist_ok=True)
        return "empty"
    if dst.exists():
        shutil.rmtree(dst)
    dst.parent.mkdir(parents=True, exist_ok=True)
    if plat == "darwin":
        cmd = ["cp", "-ac", str(src), str(dst)]
        mode = "apfs-clone"
    else:
        cmd = ["cp", "-a", "--reflink=always", str(src), str(dst)]
        mode = "reflink"
    result = subprocess.run(cmd, text=True, capture_output=True, check=False)
    if result.returncode == 0:
        return mode
    if dst.exists():
        shutil.rmtree(dst, ignore_errors=True)
    dst.mkdir(parents=True, exist_ok=True)
    return "fallback-empty"


def configure_sparse_checkout(worktree: Path) -> None:
    run_git(worktree, "sparse-checkout", "init", "--no-cone")
    run_git(worktree, "sparse-checkout", "set", "--no-cone", *SPARSE_PATTERNS)


def cmd_new(args: argparse.Namespace) -> int:
    root = find_main_root(args.root)
    name = validate_name(args.name)
    wt_root = root / ".local" / "wt"
    wt_path = wt_root / name
    if wt_path.exists():
        raise WorktreeError(f"worktree path already exists: {wt_path}")
    wt_root.mkdir(parents=True, exist_ok=True)

    base_explicit = args.base is not None
    base = validate_ref_arg("base", args.base) if base_explicit else None
    if base is None:
        probe = run_git(root, "rev-parse", "--verify", "--quiet", "main^{commit}", check=False)
        base = "main" if probe.returncode == 0 else "HEAD"

    add_args = ["worktree", "add"]
    if not args.full:
        add_args.append("--no-checkout")
    if args.detach:
        add_args.extend(["--detach", "--", str(wt_path), base])
    else:
        branch = validate_ref_arg("branch", args.branch) if args.branch else f"wt/{name}"
        exists = run_git(root, "show-ref", "--verify", "--quiet", f"refs/heads/{branch}", check=False)
        if exists.returncode == 0:
            if base_explicit:
                raise WorktreeError(
                    f"branch {branch!r} already exists; omit base {base!r} to check it out as-is"
                )
            add_args.extend(["--", str(wt_path), branch])
        else:
            add_args.extend(["-b", branch, "--", str(wt_path), base])

    run_git(root, *add_args)
    try:
        if not args.full:
            configure_sparse_checkout(wt_path)
            run_git(wt_path, "checkout")
    except Exception:
        run_git(root, "worktree", "remove", "--force", str(wt_path), check=False)
        raise

    target_dir = wt_path / ".local" / "target"
    if args.no_seed:
        target_dir.mkdir(parents=True, exist_ok=True)
        seed_mode = "empty"
    else:
        seed_mode = clone_target_cow(root / ".local" / "target-base", target_dir)

    print(f"WORKTREE={wt_path}")
    print(f"CARGO_TARGET_DIR={target_dir}")
    print(f"TARGET_SEED={seed_mode}")
    return 0


def cmd_warm(args: argparse.Namespace) -> int:
    root = find_main_root(args.root)
    target_base = root / ".local" / "target-base"
    target_base.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.pop("RUSTUP_TOOLCHAIN", None)
    env["CARGO_TARGET_DIR"] = str(target_base)
    cmd = ["cargo", "test", "--locked", "--workspace", "--no-run"]
    if args.release:
        cmd.append("--release")
    result = subprocess.run(cmd, cwd=root, env=env, check=False)
    if result.returncode != 0:
        raise WorktreeError(f"{' '.join(cmd)} failed with exit {result.returncode}")
    print(f"WARMED_TARGET={target_base}")
    return 0


def cmd_rm(args: argparse.Namespace) -> int:
    root = find_main_root(args.root)
    name = validate_name(args.name)
    wt_path = root / ".local" / "wt" / name
    branch: str | None = None
    if wt_path.exists() or wt_path.is_symlink():
        if wt_path.is_symlink():
            raise WorktreeError(f"refusing to remove symlinked worktree path: {wt_path}")
        if not args.force:
            dirty = run_git(wt_path, "status", "--porcelain", "--untracked-files=normal")
            if dirty.stdout.strip():
                raise WorktreeError(
                    f"worktree {wt_path} has uncommitted or untracked files; pass --force to remove"
                )
        head = run_git(wt_path, "symbolic-ref", "--short", "HEAD", check=False)
        if head.returncode == 0 and head.stdout.strip():
            branch = head.stdout.strip()
        run_git(root, "worktree", "unlock", "--", str(wt_path), check=False)
        target_dir = wt_path / ".local" / "target"
        if target_dir.is_symlink():
            raise WorktreeError(f"refusing to remove symlinked target path: {target_dir}")
        if target_dir.exists():
            shutil.rmtree(target_dir)
        rm_args = ["worktree", "remove"]
        if args.force:
            rm_args.append("--force")
        rm_args.extend(["--", str(wt_path)])
        run_git(root, *rm_args)
    run_git(root, "worktree", "prune")
    if args.delete_branch and branch and branch != "main":
        flag = "-D" if args.force else "-d"
        run_git(root, "branch", flag, "--", branch)
    print(f"REMOVED={wt_path}")
    return 0


def dir_stats(path: Path) -> tuple[int, float]:
    """Return (apparent_bytes, latest_mtime) without following symlinks."""
    try:
        st = path.lstat()
    except OSError:
        return 0, time.time()
    if path.is_symlink() or path.is_file():
        return st.st_size, st.st_mtime
    total = 0
    latest = st.st_mtime
    for root_dir, dirs, files in os.walk(path, followlinks=False):
        root_path = Path(root_dir)
        try:
            latest = max(latest, root_path.lstat().st_mtime)
        except OSError:
            pass
        for name in files:
            try:
                fst = (root_path / name).lstat()
                total += fst.st_size
                latest = max(latest, fst.st_mtime)
            except OSError:
                pass
        for name in dirs:
            dpath = root_path / name
            if dpath.is_symlink():
                try:
                    dst = dpath.lstat()
                    total += dst.st_size
                    latest = max(latest, dst.st_mtime)
                except OSError:
                    pass
    return total, latest


def format_bytes(size: int) -> str:
    units = ("B", "KiB", "MiB", "GiB", "TiB")
    value = float(size)
    for unit in units:
        if value < 1024.0 or unit == units[-1]:
            return f"{value:.1f} {unit}" if unit != "B" else f"{int(value)} B"
        value /= 1024.0
    return f"{size} B"


def cmd_gc(args: argparse.Namespace) -> int:
    root = find_main_root(args.root)
    local_dir = root / ".local"
    if not local_dir.is_dir():
        print(".local does not exist")
        return 0
    now = time.time()
    cutoff = now - args.days * 86400
    entries: list[tuple[int, float, Path, bool]] = []
    for child in sorted(local_dir.iterdir()):
        size, mtime = dir_stats(child)
        prunable = child.name not in PROTECTED_LOCAL_ENTRIES and mtime < cutoff
        entries.append((size, mtime, child, prunable))
    entries.sort(key=lambda item: item[0], reverse=True)
    total = sum(item[0] for item in entries)
    print(f".local total (apparent): {format_bytes(total)}")
    pruned_bytes = 0
    for size, mtime, child, prunable in entries:
        age_days = max(0.0, (now - mtime) / 86400.0)
        tag = " [stale]" if prunable else ""
        print(f"  {format_bytes(size):>10}  {age_days:5.1f}d  {child.name}{tag}")
        if args.prune and prunable:
            if child.is_dir() and not child.is_symlink():
                shutil.rmtree(child)
            else:
                child.unlink(missing_ok=True)
            pruned_bytes += size
    if args.prune:
        print(f"Pruned: {format_bytes(pruned_bytes)}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="CoW worktree and target manager for GitTurtle.")
    parser.add_argument("--root", type=Path, default=None, help="Repository root override")
    sub = parser.add_subparsers(dest="command", required=True)

    p_new = sub.add_parser("new", help="Create a CoW-seeded worktree under .local/wt/<name>")
    p_new.add_argument("name", help="Worktree directory name under .local/wt/")
    p_new.add_argument("base", nargs="?", default=None, help="Base commit/branch (default: main or HEAD)")
    group = p_new.add_mutually_exclusive_group()
    group.add_argument("--branch", help="Branch name to create or check out (default: wt/<name>)")
    group.add_argument("--detach", action="store_true", help="Create a detached HEAD worktree")
    p_new.add_argument("--full", action="store_true", help="Disable sparse checkout (include docs/evidence & benchmarks)")
    p_new.add_argument("--no-seed", action="store_true", help="Do not CoW-seed from .local/target-base")
    p_new.set_defaults(func=cmd_new)

    p_warm = sub.add_parser("warm", help="Incrementally warm .local/target-base")
    p_warm.add_argument("--release", action="store_true", help="Also warm release test artifacts")
    p_warm.set_defaults(func=cmd_warm)

    p_rm = sub.add_parser("rm", help="Remove a worktree under .local/wt/<name> and its target")
    p_rm.add_argument("name", help="Worktree directory name under .local/wt/")
    p_rm.add_argument("--delete-branch", action="store_true", help="Delete the worktree's local branch")
    p_rm.add_argument("--force", action="store_true", help="Force removal of dirty worktree / unmerged branch")
    p_rm.set_defaults(func=cmd_rm)

    p_gc = sub.add_parser("gc", help="Report .local/ disk usage and optionally prune stale scratch dirs")
    p_gc.add_argument("--days", type=float, default=14.0, help="Staleness threshold in days (default: 14)")
    p_gc.add_argument("--prune", action="store_true", help="Delete non-protected stale .local/ entries")
    p_gc.set_defaults(func=cmd_gc)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except WorktreeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
