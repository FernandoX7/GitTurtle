"""Exercise Claude Code hooks (protect_paths, reinject_task, stop_gate)."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


HOOKS = Path(__file__).resolve().parents[2] / ".claude/hooks"
PROTECT_HOOK = HOOKS / "protect_paths.py"
REINJECT_HOOK = HOOKS / "reinject_task.py"
STOP_GATE_HOOK = HOOKS / "stop_gate.py"
QUEUE = "docs/development/themes/tasks.json"


@unittest.skipUnless(os.name == "posix", "POSIX hook environment")
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
        return subprocess.run([sys.executable, str(PROTECT_HOOK)], input=json.dumps(payload),
                              capture_output=True, text=True, env=env)

    def test_active_task_queue_is_protected_during_a_guarded_session(self):
        for flag in ("GITTURTLE_PROTECT_PATHS", "GITTURTLE_LOOP"):
            with self.subTest(flag=flag):
                result = self.hook(QUEUE, **{flag: "1", "GITTURTLE_TASKS_PATH": QUEUE})
                self.assertEqual(result.returncode, 2)
                self.assertIn(QUEUE, result.stderr)

    def test_task_scope_outside_the_policy_paths_stays_editable(self):
        result = self.hook("crates/app/src/main.rs", GITTURTLE_PROTECT_PATHS="1", GITTURTLE_TASKS_PATH=QUEUE)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")

    def test_policy_and_guidance_paths_remain_protected(self):
        for relative in (".claude/hooks/protect_paths.py", "AGENTS.md",
                         "GEMINI.md", ".githooks/pre-commit", "scripts/dev/guards.py",
                         "scripts/gate.py", "deny.toml"):
            with self.subTest(relative=relative):
                result = self.hook(relative, GITTURTLE_PROTECT_PATHS="1", GITTURTLE_TASKS_PATH=QUEUE)
                self.assertEqual(result.returncode, 2)
                self.assertIn(relative, result.stderr)

    def test_interactive_sessions_are_unaffected(self):
        result = self.hook(QUEUE, GITTURTLE_TASKS_PATH=QUEUE)
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.hook("AGENTS.md")
        self.assertEqual(result.returncode, 0, result.stderr)


@unittest.skipUnless(os.name == "posix", "POSIX hook environment")
class ReinjectTaskTests(unittest.TestCase):
    def test_reinject_falls_back_to_state_md_git_status_and_local_target(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
            (root / "docs" / "development").mkdir(parents=True)
            (root / "docs" / "development" / "STATE.md").write_text("# Active State\nPR 1 active\n", encoding="utf-8")
            (root / ".local" / "target").mkdir(parents=True)
            env = {k: v for k, v in os.environ.items() if k not in ("GITTURTLE_TASK_CONTEXT", "CARGO_TARGET_DIR")}
            result = subprocess.run(
                [sys.executable, str(REINJECT_HOOK)],
                input=json.dumps({"cwd": str(root)}),
                capture_output=True, text=True, env=env, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(result.stdout)
            ctx = payload["hookSpecificOutput"]["additionalContext"]
            self.assertIn("PR 1 active", ctx)
            self.assertIn("Current git status -sb:", ctx)
            self.assertIn(str(root / ".local" / "target"), ctx)


@unittest.skipUnless(os.name == "posix", "POSIX hook environment")
class StopGateTests(unittest.TestCase):
    def test_stop_gate_detects_committed_rust_changes_uses_local_target_and_resets_counter_on_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            git_env = {
                **os.environ,
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_CONFIG_GLOBAL": "/dev/null",
                "GIT_AUTHOR_NAME": "Fixture",
                "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                "GIT_COMMITTER_NAME": "Fixture",
                "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
            }
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, env=git_env, check=True)
            (root / "crates" / "app" / "src").mkdir(parents=True)
            (root / "crates" / "app" / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
            subprocess.run(["git", "add", "."], cwd=root, env=git_env, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "init"], cwd=root, env=git_env, check=True)
            subprocess.run(["git", "checkout", "-q", "-b", "feat/test"], cwd=root, env=git_env, check=True)
            (root / "crates" / "app" / "src" / "main.rs").write_text("fn main() { /* edited */ }\n", encoding="utf-8")
            subprocess.run(["git", "add", "."], cwd=root, env=git_env, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "edit rust"], cwd=root, env=git_env, check=True)

            # Warm .local/target (worktree layout) and install a fake gate script that records env.
            (root / ".local" / "target" / "debug" / "deps").mkdir(parents=True)
            (root / ".local" / "target" / ".rustc_info.json").write_text("{}", encoding="utf-8")
            record = root / "gate-env.json"
            (root / "scripts").mkdir(parents=True)
            (root / "scripts" / "gate.py").write_text(
                "import json, os, pathlib, sys\n"
                f"pathlib.Path({str(record)!r}).write_text(json.dumps({{'toolchain': os.environ.get('RUSTUP_TOOLCHAIN'), 'target': os.environ.get('CARGO_TARGET_DIR')}}))\n"
                "sys.exit(0)\n",
                encoding="utf-8",
            )
            scratch = root / ".local" / "gate"
            scratch.mkdir(parents=True)
            counter = scratch / "stop-gate-attempts-s1"
            counter.write_text("1", encoding="utf-8")

            env = {k: v for k, v in os.environ.items() if k not in ("CARGO_TARGET_DIR", "GITTURTLE_SKIP_STOP_GATE")}
            env["RUSTUP_TOOLCHAIN"] = "stable"
            result = subprocess.run(
                [sys.executable, str(STOP_GATE_HOOK)],
                input=json.dumps({"cwd": str(root), "session_id": "s1"}),
                capture_output=True, text=True, env=env, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            seen = json.loads(record.read_text(encoding="utf-8"))
            self.assertIsNone(seen["toolchain"])
            self.assertEqual(seen["target"], str(root / ".local" / "target"))
            self.assertFalse(counter.exists(), "attempt counter must reset after a green gate")

            # Empty CARGO_TARGET_DIR="" must also resolve to .local/target instead of leaking "".
            record.unlink()
            env["CARGO_TARGET_DIR"] = ""
            result2 = subprocess.run(
                [sys.executable, str(STOP_GATE_HOOK)],
                input=json.dumps({"cwd": str(root), "session_id": "s1"}),
                capture_output=True, text=True, env=env, check=False,
            )
            self.assertEqual(result2.returncode, 0, result2.stderr)
            seen2 = json.loads(record.read_text(encoding="utf-8"))
            self.assertEqual(seen2["target"], str(root / ".local" / "target"))


if __name__ == "__main__":
    unittest.main()
