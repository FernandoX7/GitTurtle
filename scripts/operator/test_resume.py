"""Tests for resume.sh on a copied operator tree with a fake tmux: no real session, no controller.

Run: python3 -B -m unittest discover -s scripts/operator -p 'test_*.py'
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

OPERATOR = Path(__file__).resolve().parent
SESSION = "gitturtle-loop"
MARKER = "[loop process exited]"

# Records each call as a JSON line; reports a session only when FAKE_TMUX_SESSION_LOG is set.
FAKE_TMUX = f"""#!{sys.executable}
import json, os, sys
with open(os.environ["FAKE_TMUX_CALLS"], "a", encoding="utf-8") as calls:
    calls.write(json.dumps(sys.argv[1:]) + "\\n")
session_log = os.environ.get("FAKE_TMUX_SESSION_LOG")
if sys.argv[1] == "has-session":
    sys.exit(0 if session_log else 1)
if sys.argv[1] == "show-options" and session_log:
    print(session_log)
"""


class ResumeCase(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.temp = Path(directory.name).resolve()
        self.root = self.temp / "root"
        operator = self.root / "scripts" / "operator"
        operator.mkdir(parents=True)
        for name in ("resume.sh", "lib.sh", "loop-session.sh"):
            shutil.copyfile(OPERATOR / name, operator / name)
        self.loop_session = operator / "loop-session.sh"
        self.root_controller = self.root / "scripts" / "agent-loop.py"
        self.root_controller.write_text("raise SystemExit('not run by these tests')\n")
        self.log_dir = self.root / ".local" / "agent-loop"
        self.log_dir.mkdir(parents=True)
        bin_dir = self.temp / "bin"
        bin_dir.mkdir()
        (bin_dir / "tmux").write_text(FAKE_TMUX)
        (bin_dir / "tmux").chmod(0o755)
        self.calls = self.temp / "tmux-calls.jsonl"
        self.env = {key: value for key, value in os.environ.items() if key != "FAKE_TMUX_SESSION_LOG"}
        self.env.update(PATH=f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}", FAKE_TMUX_CALLS=str(self.calls))

    def make_run(self, name: str = "20261002T073619Z-b621da2e", *, saved: bool) -> Path:
        run = self.log_dir / name
        run.mkdir()
        (run / "state.json").write_text("{}\n")
        if saved:
            controller = run / "controller" / "scripts" / "agent-loop.py"
            controller.parent.mkdir(parents=True)
            controller.write_text("raise SystemExit('not run by these tests')\n")
        return run

    def resume(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["bash", str(self.root / "scripts" / "operator" / "resume.sh"), *args],
                              cwd=self.temp, env=self.env, capture_output=True, text=True, timeout=30)

    def tmux_calls(self) -> list[list[str]]:
        if not self.calls.exists():
            return []
        return [json.loads(line) for line in self.calls.read_text().splitlines()]

    def launched(self) -> list[str]:
        """The command the new session runs: everything after tmux new-session's `--`."""
        starts = [call for call in self.tmux_calls() if call[0] == "new-session"]
        self.assertEqual(len(starts), 1, self.tmux_calls())
        start = starts[0]
        self.assertEqual(start[:start.index("--")], ["new-session", "-d", "-s", SESSION, "-c", str(self.root)])
        return start[start.index("--") + 1:]

    def assert_resumed(self, result: subprocess.CompletedProcess[str], controller: Path, run: Path,
                       args: list[str]) -> None:
        self.assertEqual(result.returncode, 0, result.stderr)
        log = result.stdout.strip()
        self.assertRegex(log, r"/console-\d{8}T\d{6}Z\.log$")
        self.assertEqual(Path(log).parent, self.log_dir)
        self.assertIn(f"controller: {controller}\n", result.stderr)
        self.assertEqual(self.launched(), ["bash", str(self.loop_session), log,
                                           "python3", str(controller), "resume", "--run", str(run), *args])

    def test_a_run_with_a_saved_controller_resumes_with_it(self) -> None:
        run = self.make_run(saved=True)
        result = self.resume(run.name, "--max-minutes", "480")
        self.assert_resumed(result, run / "controller" / "scripts" / "agent-loop.py", run, ["--max-minutes", "480"])

    def test_a_run_without_a_saved_controller_resumes_with_the_checkout(self) -> None:
        run = self.make_run(saved=False)
        result = self.resume(str(run), "--max-minutes", "480")
        self.assert_resumed(result, self.root_controller, run, ["--max-minutes", "480"])

    def test_resume_options_pass_through_unchanged(self) -> None:
        run = self.make_run(saved=True)
        args = ["--max-minutes", "480", "--max-tasks", "3", "--max-idle-minutes=5", "two words", "*", ""]
        result = self.resume(run.name, *args)
        self.assert_resumed(result, run / "controller" / "scripts" / "agent-loop.py", run, args)

    def test_a_symlinked_saved_controller_is_refused(self) -> None:
        run = self.make_run(saved=False)
        (run / "controller" / "scripts").mkdir(parents=True)
        (run / "controller" / "scripts" / "agent-loop.py").symlink_to(self.root_controller)
        result = self.resume(run.name)
        self.assertEqual(result.returncode, 1)
        self.assertIn("is a symlink", result.stderr)
        self.assertEqual(self.tmux_calls(), [])

    def test_a_finished_session_is_replaced(self) -> None:
        run = self.make_run(saved=True)
        finished = self.log_dir / "console-20261002T000000Z.log"
        finished.write_text(f"complete: done\n{MARKER}\n")
        self.env["FAKE_TMUX_SESSION_LOG"] = str(finished)
        result = self.resume(run.name)
        self.assertIn(["kill-session", "-t", f"={SESSION}"], self.tmux_calls())
        self.assert_resumed(result, run / "controller" / "scripts" / "agent-loop.py", run, [])

    def test_a_live_loop_is_refused(self) -> None:
        run = self.make_run(saved=True)
        alive = self.log_dir / "console-20261002T000000Z.log"
        alive.write_text("12:00:00Z building task attempt 1\n")
        self.env["FAKE_TMUX_SESSION_LOG"] = str(alive)
        result = self.resume(run.name)
        self.assertEqual(result.returncode, 1)
        self.assertIn("still running", result.stderr)
        self.assertNotIn("controller:", result.stderr)
        self.assertFalse([call for call in self.tmux_calls() if call[0] in ("new-session", "kill-session")])


if __name__ == "__main__":
    unittest.main()
