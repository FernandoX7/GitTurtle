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
        self.assertEqual(guards.check_staged_paths(["AGENTS.md"], loop_mode=False), [])
        self.assertTrue(guards.check_staged_paths(["AGENTS.md"], loop_mode=True))
        self.assertTrue(guards.check_staged_paths(["GEMINI.md"], loop_mode=True))
        self.assertTrue(guards.check_staged_paths([".githooks/pre-commit"], loop_mode=True))
        self.assertTrue(guards.check_staged_paths(["docs/development/q.json"], loop_mode=True, queue="docs/development/q.json"))
        self.assertEqual(guards.check_staged_paths(["crates/app/src/main.rs"], loop_mode=True), [])

    def test_staged_diff_detects_conflict_and_probe_markers(self):
        conflict = "\n".join([
            "+++ b/docs/notes.md",
            "@@ -1,0 +1,2 @@",
            "+" + "<" * 7 + " HEAD",
            "+text",
        ])
        self.assertTrue(any("merge conflict marker" in item for item in guards.check_staged_diff(conflict)))

        probe = "\n".join([
            "+++ b/crates/app/src/main.rs",
            "@@ -10,0 +11,2 @@",
            "+// " + "MUTATION" + ": temporary check",
            "+let x = " + "Q9_" + "PROBE;",
        ])
        found = guards.check_staged_diff(probe)
        self.assertEqual(len(found), 2)

        self_file = "\n".join([
            "+++ b/scripts/dev/test_guards.py",
            "@@ -1,0 +1,1 @@",
            "+// " + "MUTATION",
        ])
        self.assertEqual(guards.check_staged_diff(self_file), [])

    def test_commit_message_rejects_ai_trailers(self):
        self.assertEqual(guards.check_commit_message("feat(app): refine layout\n"), [])
        self.assertTrue(guards.check_commit_message("feat(app): x\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n"))
        self.assertTrue(guards.check_commit_message("fix: y\n\nCo-Authored-By: Gemini <a@b.test>\n"))

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
