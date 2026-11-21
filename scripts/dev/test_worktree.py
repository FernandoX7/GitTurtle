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
            root = Path(tmp) / "repo"
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
            self.assertEqual(patched.call_args.args[0][:2], ["cp", "-Rc"])

    def test_gc_reports_and_prunes_only_unprotected_stale_entries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "repo"
            root.mkdir()
            init_fixture_repo(root)
            local = root / ".local"
            stale = local / "bench-old"
            fresh = local / "bench-new"
            protected = local / "target-base"
            for directory in (stale, fresh, protected):
                directory.mkdir(parents=True)
                (directory / "data.bin").write_bytes(b"x" * 1024)
            old_ts = os.path.getmtime(stale) - 30 * 86400
            os.utime(stale, (old_ts, old_ts))
            os.utime(protected, (old_ts, old_ts))

            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = worktree.main(["--root", str(root), "gc", "--days", "14"])
            self.assertEqual(rc, 0)
            self.assertIn("bench-old [stale]", buf.getvalue())
            self.assertTrue(stale.exists())

            buf2 = io.StringIO()
            with redirect_stdout(buf2):
                rc = worktree.main(["--root", str(root), "gc", "--days", "14", "--prune"])
            self.assertEqual(rc, 0)
            self.assertFalse(stale.exists())
            self.assertTrue(fresh.exists())
            self.assertTrue(protected.exists())


if __name__ == "__main__":
    unittest.main()
