"""Exercise the Claude Code PreToolUse hook that protects policy paths."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


HOOK = Path(__file__).resolve().parents[2] / ".claude/hooks/protect_paths.py"
QUEUE = "docs/development/themes/tasks.json"


@unittest.skipUnless(os.name == "posix", "The controller supports macOS and Linux processes")
class ProtectPathsTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()

    def hook(self, relative: str, **environment: str) -> subprocess.CompletedProcess:
        """Run the hook on one edit; the target file need not exist."""
        payload = {"tool_name": "Edit", "tool_input": {"file_path": str(self.root / relative)},
                   "cwd": str(self.root)}
        env = {key: value for key, value in os.environ.items() if not key.startswith("GITTURTLE_")}
        env.update(environment)
        return subprocess.run([sys.executable, str(HOOK)], input=json.dumps(payload),
                              capture_output=True, text=True, env=env)

    def test_active_task_queue_is_protected_during_a_controller_session(self):
        result = self.hook(QUEUE, GITTURTLE_LOOP="1", GITTURTLE_TASKS_PATH=QUEUE)
        self.assertEqual(result.returncode, 2)
        self.assertIn(QUEUE, result.stderr)
        self.assertIn("task", result.stderr)

    def test_task_scope_outside_the_policy_paths_stays_editable(self):
        result = self.hook("crates/app/src/main.rs", GITTURTLE_LOOP="1", GITTURTLE_TASKS_PATH=QUEUE)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")

    def test_static_task_queue_and_guidance_remain_protected(self):
        for relative in ("docs/development/tasks.json", ".claude/hooks/protect_paths.py", "AGENTS.md"):
            with self.subTest(relative=relative):
                result = self.hook(relative, GITTURTLE_LOOP="1", GITTURTLE_TASKS_PATH=QUEUE)
                self.assertEqual(result.returncode, 2)
                self.assertIn(relative, result.stderr)

    def test_interactive_sessions_are_unaffected(self):
        result = self.hook(QUEUE, GITTURTLE_TASKS_PATH=QUEUE)
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.hook("docs/development/tasks.json")
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
