"""Exercise the Claude Code PreToolUse hook that refuses unresolvable `rm` shapes."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import unittest


HOOK = Path(__file__).resolve().parents[1] / ".claude/hooks/rm_shapes.py"


class RmShapesTests(unittest.TestCase):
    def hook(self, command: str, tool: str = "Bash") -> subprocess.CompletedProcess:
        payload = {"tool_name": tool, "tool_input": {"command": command}, "cwd": "/repo"}
        return subprocess.run([sys.executable, str(HOOK)], input=json.dumps(payload),
                              capture_output=True, text=True)

    def test_relative_glob_after_a_directory_change_is_refused(self):
        for command in (
            'cd /repo/.local/qa/qa133; ls cfg/; { echo "# removed $(date -u +%T)"; ls cfg/; } > removed.txt; rm -rf cfg/*; ls cfg/ | wc -l',
            "cd run && rm -rf -- build/*.o",
            "(cd run\nrm -r cfg/?)",
            "pushd run; if true; then rm -rf out/*; fi",
        ):
            with self.subTest(command=command):
                result = self.hook(command)
                self.assertEqual(result.returncode, 2)
                self.assertIn("relative glob", result.stderr)
                self.assertIn("literal absolute path", result.stderr)

    def test_expansion_that_can_become_a_top_level_path_is_refused(self):
        for command in ("rm -f $E/*.png", 'rm -rf "$DIR"', "rm -rf ${OUT}/", "E=x; rm -rf $(pwd)/cfg", "sudo rm -r `pwd`"):
            with self.subTest(command=command):
                result = self.hook(command)
                self.assertEqual(result.returncode, 2)
                self.assertIn("shell expansion", result.stderr)

    def test_resolvable_removals_pass(self):
        for command in (
            "rm -rf /repo/.local/qa/qa133/cfg/*",
            "ls cfg/; rm -rf cfg/*",
            "cd run && rm -rf cfg",
            "cd run && rm -f notes.txt && cargo fmt --all",
            "rm -rf $HOME/.cache/gitturtle/thumbs",
            "cd run; echo 'rm -rf cfg/*' >> notes.txt",
            "cd run && git rm -r --cached cfg/*",
            "cargo test --locked -p gitturtle confirm_rm",
        ):
            with self.subTest(command=command):
                result = self.hook(command)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")

    def test_unreadable_commands_and_other_tools_pass_through(self):
        for command in ("cd run; cat <<EOF > clean.sh\nrm -rf cfg/*\nEOF", "cd run; rm -rf 'cfg/*"):
            with self.subTest(command=command):
                self.assertEqual(self.hook(command).returncode, 0)
        self.assertEqual(self.hook("cd run; rm -rf cfg/*", tool="Edit").returncode, 0)
        empty = subprocess.run([sys.executable, str(HOOK)], input="", capture_output=True, text=True)
        self.assertEqual(empty.returncode, 0)


if __name__ == "__main__":
    unittest.main()
