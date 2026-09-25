from __future__ import annotations

import os
import re
import shutil
import subprocess
import unittest
from unittest import mock

from native_qa import display

BINARIES = [
    "/tmp/gitturtle-evidence/bin/gitturtle /tmp/gitturtle-evidence/theme-fixture",
    "gitturtle-base-ff06709 /tmp/gitturtle-evidence/theme-fixture",
    "/opt/GitTurtle/GitTurtle",
    "./target/debug/gitturtle --build-info",
]
NOT_BINARIES = [
    "vim /work/GitTurtle/README.md",
    "cargo run --locked -p gitturtle -- /tmp/fixture",
    "/work/GitTurtle/target/debug/build/gitturtle-1a2b/build-script-build",
    "bash -c pgrep -af gitturtle",
    "rustc --crate-name gitturtle crates/app/src/main.rs",
]
DRIVERS = [
    "python3 scripts/native_qa/qa.py launch --binary /b --fixture /f --run-dir /r",
    "/usr/bin/python3 -B /work/tools/d3_run.py cand rows midnight",
    "python3.12 /work/b2/tools/drive_transfer.py --binary /b --output /o",
    "python3 -u capture_themes.py",
    "python3 portal_probe.py --binary /b",
]
NOT_DRIVERS = [
    "python3 scripts/native_qa/qa.py compare a.png b.png",
    "python3 scripts/native_qa/qa.py display-check",
    "python3 -m unittest discover -s scripts/native_qa -t scripts",
    "less drive_transfer.py",
    "python3 scripts/agent-loop.py run",
]


class PatternTest(unittest.TestCase):
    def test_binary_pattern_is_anchored_on_the_command(self) -> None:
        pattern = re.compile(display.BINARY_PATTERN)
        for line in BINARIES:
            self.assertTrue(pattern.search(line), line)
        for line in NOT_BINARIES:
            self.assertFalse(pattern.search(line), line)

    def test_driver_pattern_matches_launchers_not_other_tooling(self) -> None:
        pattern = re.compile(display.DRIVER_PATTERN)
        for line in DRIVERS:
            self.assertTrue(pattern.search(line), line)
        for line in NOT_DRIVERS:
            self.assertFalse(pattern.search(line), line)

    @unittest.skipUnless(shutil.which("pgrep"), "no pgrep")
    def test_patterns_are_valid_for_pgrep(self) -> None:
        for pattern in (display.BINARY_PATTERN, display.DRIVER_PATTERN):
            result = subprocess.run(["pgrep", "-af", pattern], capture_output=True, text=True)
            self.assertIn(result.returncode, (0, 1), result.stderr)


class CheckTest(unittest.TestCase):
    def test_parse_pgrep_excludes_this_process_tree(self) -> None:
        text = "101 /x/gitturtle /tmp/f\n202 python3 scripts/native_qa/qa.py launch\n\nnoise\n"
        self.assertEqual(display.parse_pgrep(text, {202}), [(101, "/x/gitturtle /tmp/f")])
        self.assertIn(os.getpid(), display.ancestors())

    def run_check(self, processes, windows, allow=frozenset()):
        def fake_pgrep(pattern, exclude):
            return processes.get(pattern, [])

        with mock.patch.object(display, "pgrep", fake_pgrep), mock.patch.object(display, "windows", windows):
            return display.check(":1", set(allow))

    def test_clear_display(self) -> None:
        status, lines = self.run_check({}, lambda name: [])
        self.assertEqual(status, 0)
        self.assertTrue(lines[0].startswith("$ date -u\n"))
        self.assertIn("(nothing)", lines)
        self.assertIn("(none)", lines)

    def test_foreign_process_or_window_fails_unless_allowed(self) -> None:
        processes = {display.BINARY_PATTERN: [(4242, "/opt/gitturtle")]}
        window = [dict(id=0x3a00004, wm_class=["gitturtle", "GitTurtle"], pid=4242, width=1000, height=680)]
        self.assertEqual(self.run_check(processes, lambda name: [])[0], 1)
        self.assertEqual(self.run_check({}, lambda name: window)[0], 1)
        status, lines = self.run_check(processes, lambda name: window, allow={4242})
        self.assertEqual(status, 0)
        self.assertIn("4242 /opt/gitturtle  (allowed)", lines)

    def test_unreadable_display_is_inconclusive(self) -> None:
        def broken(name):
            raise ConnectionRefusedError("no display")

        self.assertEqual(self.run_check({}, broken)[0], 2)
        processes = {display.DRIVER_PATTERN: [(7, "python3 d3_run.py")]}
        self.assertEqual(self.run_check(processes, broken)[0], 1)

    def test_missing_pgrep_is_inconclusive(self) -> None:
        def missing(pattern, exclude):
            raise FileNotFoundError("pgrep")

        with mock.patch.object(display, "pgrep", missing):
            self.assertEqual(display.check(":1")[0], 2)


if __name__ == "__main__":
    unittest.main()
