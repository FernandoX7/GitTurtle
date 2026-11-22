"""Tests for shared repository safety guards and Git hooks."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))

from dev import guards  # noqa: E402


class GuardsTests(unittest.TestCase):
    def test_staged_paths_reject_snap_new_and_loop_protected_paths(self):
        self.assertTrue(guards.check_staged_paths(["crates/app/src/snapshots/a.snap.new"]))
        # Deleting an accidental .snap.new is allowed when only all_touched includes it.
        self.assertEqual(guards.check_staged_paths([], all_touched=["crates/app/src/snapshots/a.snap.new"]), [])
        # Deleting or renaming away a protected path in loop mode must still be rejected.
        self.assertTrue(guards.check_staged_paths([], all_touched=["AGENTS.md"], loop_mode=True))
        self.assertEqual(guards.check_staged_paths(["AGENTS.md"], loop_mode=False), [])
        self.assertTrue(guards.check_staged_paths(["AGENTS.md"], loop_mode=True))
        self.assertTrue(guards.check_staged_paths(["GEMINI.md"], loop_mode=True))
        self.assertTrue(guards.check_staged_paths([".githooks/pre-commit"], loop_mode=True))
        self.assertTrue(guards.check_staged_paths(["scripts/dev/__init__.py"], loop_mode=True))
        self.assertTrue(guards.check_staged_paths(["docs/development/q.json"], loop_mode=True, queue="docs/development/q.json"))
        self.assertEqual(guards.check_staged_paths(["crates/app/src/main.rs"], loop_mode=True), [])

    def test_staged_diff_detects_conflict_and_probe_markers_and_ignores_hunk_spoofing(self):
        conflict = "\n".join([
            "diff --git a/docs/notes.md b/docs/notes.md",
            "+++ b/docs/notes.md",
            "@@ -1,0 +1,2 @@",
            "+" + "<" * 7 + " HEAD",
            "+text",
        ])
        self.assertTrue(any("merge conflict marker" in item for item in guards.check_staged_diff(conflict)))

        # Valid 7-char Markdown Setext H1 heading must not false-positive.
        setext = "\n".join([
            "diff --git a/docs/notes.md b/docs/notes.md",
            "+++ b/docs/notes.md",
            "@@ -1,0 +1,2 @@",
            "+Summary",
            "+=======",
        ])
        self.assertEqual(guards.check_staged_diff(setext), [])

        # An added line starting with `++ b/scripts/dev/guards.py` inside a hunk must not spoof current_path.
        spoof = "\n".join([
            "diff --git a/crates/app/src/main.rs b/crates/app/src/main.rs",
            "+++ b/crates/app/src/main.rs",
            "@@ -10,0 +11,3 @@",
            "+++ b/scripts/dev/guards.py",
            "+// " + "MUTATION" + ": spoofed check",
            "+let x = " + "Q9_" + "PROBE;",
        ])
        found = guards.check_staged_diff(spoof)
        self.assertEqual(len(found), 2)
        self.assertTrue(all(item.startswith("crates/app/src/main.rs:") for item in found))

    def test_commit_message_rejects_ai_trailers_and_strips_verbose_diff(self):
        self.assertEqual(guards.check_commit_message("feat(app): refine layout\n"), [])
        self.assertTrue(guards.check_commit_message("feat(app): x\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n"))
        self.assertTrue(guards.check_commit_message("fix: y\n\nCo-Authored-By: Gemini <a@b.test>\n"))
        self.assertTrue(guards.check_commit_message("fix: z\n\nGenerated with [Claude Code]\n"))
        verbose = (
            "test(guards): update parser\n\n"
            + guards.SCISSORS_LINE
            + "\n+Co-Authored-By: Claude <noreply@anthropic.com>\n"
        )
        self.assertEqual(guards.check_commit_message(verbose), [])

    def test_path_protection_handles_subdirectory_cwd_and_symlink_escapes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
            sub = root / "crates" / "app"
            sub.mkdir(parents=True)
            self.assertIsNotNone(guards.path_protection_reason("../../AGENTS.md", sub))
            self.assertIsNotNone(guards.path_protection_reason(str(root / "scripts/dev/__init__.py"), sub))
            self.assertIsNone(guards.path_protection_reason("src/main.rs", sub))
            escape_link = root / "crates" / "app" / "escape"
            escape_link.symlink_to("/tmp")
            self.assertIsNotNone(guards.path_protection_reason(str(escape_link / "outside.txt"), sub))

    def test_rm_problem_rejects_unresolvable_shapes(self):
        self.assertIsNotNone(guards.rm_problem("cd run && rm -rf out/*"))
        self.assertIsNotNone(guards.rm_problem('rm -rf "$DIR"'))
        self.assertIsNone(guards.rm_problem("rm -rf /tmp/safe-dir/*"))

    @unittest.skipUnless(os.name == "posix", "POSIX executable hooks")
    def test_githooks_are_executable_and_run_commit_msg_guard(self):
        for name in ("pre-commit", "commit-msg", "pre-push"):
            hook = ROOT / ".githooks" / name
            self.assertTrue(hook.is_file(), f"missing {hook}")
            self.assertTrue(os.access(hook, os.X_OK), f"not executable: {hook}")
        with tempfile.TemporaryDirectory() as tmp:
            msg = Path(tmp) / "COMMIT_EDITMSG"
            msg.write_text("chore: ok\n", encoding="utf-8")
            ok = subprocess.run([str(ROOT / ".githooks/commit-msg"), str(msg)], cwd=ROOT, capture_output=True, text=True)
            self.assertEqual(ok.returncode, 0, ok.stderr)
            msg.write_text("chore: bad\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n", encoding="utf-8")
            bad = subprocess.run([str(ROOT / ".githooks/commit-msg"), str(msg)], cwd=ROOT, capture_output=True, text=True)
            self.assertEqual(bad.returncode, 1)


if __name__ == "__main__":
    unittest.main()
