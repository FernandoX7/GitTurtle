"""Unit tests for scripts/dev/worktree.py."""

from __future__ import annotations

import io
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import worktree  # noqa: E402


def git(cwd: Path, *args: str) -> None:
    env = os.environ.copy()
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    env["GIT_CONFIG_GLOBAL"] = "/dev/null"
    subprocess.run(
        [
            "git",
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            *args,
        ],
        cwd=cwd,
        env=env,
        check=True,
        capture_output=True,
    )


def init_fixture_repo(root: Path) -> None:
    git(root, "init", "-b", "main")
    (root / "README.md").write_text("hello\n", encoding="utf-8")
    (root / "docs" / "evidence").mkdir(parents=True)
    (root / "docs" / "evidence" / "shot.png").write_bytes(b"png")
    (root / "docs" / "benchmarks").mkdir(parents=True)
    (root / "docs" / "benchmarks" / "bench.json").write_text("{}\n", encoding="utf-8")
    (root / "crates" / "app").mkdir(parents=True)
    (root / "crates" / "app" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
    git(root, "add", ".")
    git(root, "commit", "-m", "init")


class WorktreeTests(unittest.TestCase):
    def test_validate_name_rejects_traversal_and_separators(self) -> None:
        self.assertEqual(worktree.validate_name("phase1-ci_01.a"), "phase1-ci_01.a")
        for bad in ("", "..", "a..b", "../x", "a/b", ".hidden", "-dash", "x" * 65):
            with self.assertRaises(worktree.WorktreeError):
                worktree.validate_name(bad)

    def test_new_sparse_and_full_and_rm(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = (Path(tmp) / "repo").resolve()
            root.mkdir()
            init_fixture_repo(root)
            (root / ".local" / "target-base").mkdir(parents=True)
            (root / ".local" / "target-base" / "stamp.txt").write_text("warm\n", encoding="utf-8")

            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = worktree.main(["--root", str(root), "new", "feat-a", "main"])
            self.assertEqual(rc, 0)
            out = buf.getvalue()
            wt = root / ".local" / "wt" / "feat-a"
            self.assertIn(f"WORKTREE={wt}", out)
            self.assertIn(f"CARGO_TARGET_DIR={wt / '.local' / 'target'}", out)
            self.assertTrue((wt / "README.md").exists())
            self.assertTrue((wt / "crates" / "app" / "main.rs").exists())
            self.assertFalse((wt / "docs" / "evidence" / "shot.png").exists())
            self.assertFalse((wt / "docs" / "benchmarks" / "bench.json").exists())
            self.assertTrue((wt / ".local" / "target").is_dir())
            # Simulate a populated CoW-cloned target dir even on non-reflink /tmp
            # (matches macOS APFS where stamp.txt is cloned into .local/target).
            (wt / ".local" / "target" / "stamp.txt").write_text("warm\n", encoding="utf-8")

            # Full checkout includes docs/evidence and docs/benchmarks.
            buf_full = io.StringIO()
            with redirect_stdout(buf_full):
                rc = worktree.main(["--root", str(root), "new", "feat-full", "--full", "--no-seed"])
            self.assertEqual(rc, 0)
            wt_full = root / ".local" / "wt" / "feat-full"
            self.assertTrue((wt_full / "docs" / "evidence" / "shot.png").exists())
            self.assertTrue((wt_full / "docs" / "benchmarks" / "bench.json").exists())
            self.assertIn("TARGET_SEED=empty", buf_full.getvalue())

            # Remove both worktrees and delete branches.
            for name in ("feat-a", "feat-full"):
                rc = worktree.main(["--root", str(root), "rm", name, "--delete-branch"])
                self.assertEqual(rc, 0)
                self.assertFalse((root / ".local" / "wt" / name).exists())

    def test_clone_target_cow_handles_fallback_without_copying_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "base"
            dst = Path(tmp) / "dst"
            src.mkdir()
            (src / "artifact").write_bytes(b"12345")
            failed = subprocess.CompletedProcess(args=["cp"], returncode=1, stdout="", stderr="unsupported")
            with mock.patch.object(worktree.subprocess, "run", return_value=failed) as patched:
                mode = worktree.clone_target_cow(src, dst, platform="linux")
            self.assertEqual(mode, "fallback-empty")
            self.assertEqual(patched.call_args.args[0][:3], ["cp", "-a", "--reflink=always"])
            self.assertTrue(dst.is_dir())
            self.assertFalse((dst / "artifact").exists())

            ok = subprocess.CompletedProcess(args=["cp"], returncode=0, stdout="", stderr="")
            with mock.patch.object(worktree.subprocess, "run", return_value=ok) as patched:
                mode = worktree.clone_target_cow(src, dst, platform="darwin")
            self.assertEqual(mode, "apfs-clone")
            self.assertEqual(patched.call_args.args[0][:2], ["cp", "-ac"])

    def test_rm_refuses_dirty_worktree_without_wiping_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = (Path(tmp) / "repo").resolve()
            root.mkdir()
            init_fixture_repo(root)
            self.assertEqual(worktree.main(["--root", str(root), "new", "dirty-wt", "--no-seed"]), 0)
            wt = root / ".local" / "wt" / "dirty-wt"
            sentinel = wt / ".local" / "target" / "keep.bin"
            sentinel.write_bytes(b"ok")
            (wt / "untracked.txt").write_text("dirty\n", encoding="utf-8")
            rc = worktree.main(["--root", str(root), "rm", "dirty-wt"])
            self.assertEqual(rc, 1)
            self.assertTrue(sentinel.exists(), "failed rm must not wipe .local/target")
            self.assertEqual(worktree.main(["--root", str(root), "rm", "dirty-wt", "--force", "--delete-branch"]), 0)
            self.assertFalse(wt.exists())

    def test_rm_deletes_branch_even_when_worktree_dir_was_manually_removed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = (Path(tmp) / "repo").resolve()
            root.mkdir()
            init_fixture_repo(root)
            self.assertEqual(
                worktree.main(["--root", str(root), "new", "gone-wt", "--branch", "fix/gone-wt", "--no-seed"]),
                0,
            )
            wt = root / ".local" / "wt" / "gone-wt"
            import shutil
            shutil.rmtree(wt)
            self.assertEqual(worktree.main(["--root", str(root), "rm", "gone-wt", "--delete-branch"]), 0)
            check = subprocess.run(
                ["git", "show-ref", "--verify", "--quiet", "refs/heads/fix/gone-wt"],
                cwd=root, check=False,
            )
            self.assertNotEqual(check.returncode, 0, "branch should be deleted even when worktree dir was already gone")

    def test_warm_release_builds_release_gitturtle_binary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = (Path(tmp) / "repo").resolve()
            root.mkdir()
            init_fixture_repo(root)
            ok = subprocess.CompletedProcess(args=["cargo"], returncode=0)
            with (
                mock.patch.object(worktree, "find_main_root", return_value=root),
                mock.patch.object(worktree.subprocess, "run", return_value=ok) as patched,
            ):
                rc = worktree.main(["--root", str(root), "warm", "--release"])
            self.assertEqual(rc, 0)
            self.assertEqual(
                patched.call_args.args[0],
                ["cargo", "build", "--locked", "--release", "-p", "gitturtle"],
            )

    def test_gc_reports_and_prunes_only_unprotected_stale_entries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = (Path(tmp) / "repo").resolve()
            root.mkdir()
            init_fixture_repo(root)
            local = root / ".local"
            stale = local / "bench-old"
            active_nested = local / "gate-active"
            fresh = local / "bench-new"
            protected = local / "target-base"
            for directory in (stale, active_nested, fresh, protected):
                directory.mkdir(parents=True)
                (directory / "data.bin").write_bytes(b"x" * 1024)
            old_ts = os.path.getmtime(stale) - 30 * 86400
            for path in (stale / "data.bin", stale, active_nested, protected / "data.bin", protected):
                os.utime(path, (old_ts, old_ts))
            # active_nested top-level dir is old, but its inner file is fresh: must NOT be pruned.

            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = worktree.main(["--root", str(root), "gc", "--days", "14"])
            self.assertEqual(rc, 0)
            self.assertIn("bench-old [stale]", buf.getvalue())
            self.assertNotIn("gate-active [stale]", buf.getvalue())
            self.assertTrue(stale.exists())

            buf2 = io.StringIO()
            with redirect_stdout(buf2):
                rc = worktree.main(["--root", str(root), "gc", "--days", "14", "--prune"])
            self.assertEqual(rc, 0)
            self.assertFalse(stale.exists())
            self.assertTrue(active_nested.exists())
            self.assertTrue(fresh.exists())
            self.assertTrue(protected.exists())

    def test_list_status_and_env_subcommands(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = (Path(tmp) / "repo").resolve()
            root.mkdir()
            init_fixture_repo(root)
            self.assertEqual(
                worktree.main(["--root", str(root), "new", "wt-info", "--branch", "feat/info", "--no-seed"]),
                0,
            )
            import json
            buf_list = io.StringIO()
            with redirect_stdout(buf_list):
                self.assertEqual(worktree.main(["--root", str(root), "list", "--json"]), 0)
            listed = json.loads(buf_list.getvalue())
            self.assertEqual([item["name"] for item in listed], ["wt-info"])
            self.assertEqual(listed[0]["branch"], "feat/info")

            buf_status = io.StringIO()
            with redirect_stdout(buf_status):
                self.assertEqual(worktree.main(["--root", str(root), "status", "wt-info", "--json"]), 0)
            st = json.loads(buf_status.getvalue())
            self.assertEqual(len(st["worktrees"]), 1)
            self.assertEqual(st["worktrees"][0]["name"], "wt-info")

            buf_env = io.StringIO()
            with redirect_stdout(buf_env):
                self.assertEqual(worktree.main(["--root", str(root), "env", "wt-info"]), 0)
            self.assertIn("export CARGO_TARGET_DIR=", buf_env.getvalue())
            self.assertIn("unset RUSTUP_TOOLCHAIN", buf_env.getvalue())


if __name__ == "__main__":
    unittest.main()
