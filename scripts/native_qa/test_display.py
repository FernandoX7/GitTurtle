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
    "/venv/bin/python3 -u scripts/native_qa/qa.py scenario run docs/evidence/t/scenario.json --build base=/b",
    "python3 scripts/native_qa/qa.py recheck docs/evidence/t/scenario.json --exe /c --committed docs/evidence/t",
    "/usr/bin/python3 -B /work/tools/d3_run.py cand rows midnight",
    "python3.12 /work/b2/tools/drive_transfer.py --binary /b --output /o",
    "python3 -u capture_themes.py",
    "python3 portal_probe.py --binary /b",
]
NOT_DRIVERS = [
    "python3 scripts/native_qa/qa.py compare a.png b.png",
    "python3 scripts/native_qa/qa.py display-check",
    "python3 scripts/native_qa/qa.py scenario check docs/evidence/t/scenario.json",
    "python3 scripts/native_qa/qa.py scenario fixture docs/evidence/t/scenario.json",
    "python3 scripts/native_qa/qa.py attestation --bundle /b --task t",
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

        with mock.patch.object(display, "pgrep", fake_pgrep), mock.patch.object(display, "windows", windows), \
                mock.patch.object(display, "run_read", FakeReads()):
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

        with mock.patch.object(display, "pgrep", missing), mock.patch.object(display, "run_read", FakeReads()):
            self.assertEqual(display.check(":1")[0], 2)


UID = os.getuid()
UNLOCKED = {"gdbus": "(false,)\n", "show-user": "Display=8\n", "show-session": "Type=wayland\nLockedHint=no\n"}
# Every verb that would change the session rather than read it.
WRITE_VERBS = {"Lock", "SetActive", "SimulateUserActivity", "org.gnome.ScreenSaver.Lock",
               "org.gnome.ScreenSaver.SetActive", "org.gnome.ScreenSaver.SimulateUserActivity", "lock-session",
               "unlock-session", "lock-sessions", "unlock-sessions", "activate", "terminate-session", "kill-session",
               "terminate-user", "kill-user", "set-property", "--emit", "emit"}


class FakeReads:
    """Stands in for `display.run_read`: replies by command, records every argument array, never runs one."""

    def __init__(self, **replies) -> None:
        self.replies = {**UNLOCKED, **replies}
        self.calls: list[list[str]] = []

    def __call__(self, argv, env=None):
        self.calls.append(list(argv))
        reply = self.replies["gdbus" if argv[0] == "gdbus" else argv[1]]
        if isinstance(reply, Exception):
            raise reply
        return reply


class LockTest(unittest.TestCase):
    def run_check(self, reads, processes=None, windows=lambda name: []):
        processes = processes or {}
        with mock.patch.object(display, "pgrep", lambda pattern, exclude: processes.get(pattern, [])), \
                mock.patch.object(display, "windows", windows), mock.patch.object(display, "run_read", reads):
            return display.check(":1")

    def test_both_unlocked_keeps_the_clear_result(self) -> None:
        reads = FakeReads()
        status, lines = self.run_check(reads)
        self.assertEqual(status, 0)
        self.assertEqual(lines[-1], "display clear")
        self.assertIn("GNOME ScreenSaver: GetActive false (unlocked)", lines)
        self.assertIn("logind: LockedHint=no for session 8 (wayland) (unlocked)", lines)
        self.assertFalse(any(line.startswith(display.LOCKED_PREFIX) for line in lines))
        with mock.patch.object(display, "lock_check", lambda: (0, [])), \
                mock.patch.object(display, "pgrep", lambda pattern, exclude: []), \
                mock.patch.object(display, "windows", lambda name: []):
            before = display.check(":1")[1]
        self.assertEqual(lines[:len(before) - 1], before[:-1], "the existing lines are unchanged")

    def test_stubbed_argument_arrays(self) -> None:
        reads = FakeReads()
        self.run_check(reads)
        self.assertEqual(reads.calls, [
            ["gdbus", "call", "--session", "--dest", "org.gnome.ScreenSaver", "--object-path", "/org/gnome/ScreenSaver",
             "--method", "org.gnome.ScreenSaver.GetActive", "--timeout", "2"],
            ["loginctl", "show-user", str(UID), "-p", "Display"],
            ["loginctl", "show-session", "8", "-p", "Type", "-p", "LockedHint"],
        ])

    def test_screensaver_lock_fails_naming_its_source(self) -> None:
        status, lines = self.run_check(FakeReads(gdbus="(true,)\n"))
        self.assertEqual(status, 1)
        self.assertIn("LOCKED: GNOME ScreenSaver GetActive returned true", lines)
        self.assertEqual(lines[-1], "session LOCKED")

    def test_logind_lock_fails_naming_its_source(self) -> None:
        status, lines = self.run_check(FakeReads(**{"show-session": "Type=x11\nLockedHint=yes\n"}))
        self.assertEqual(status, 1)
        self.assertIn("LOCKED: logind LockedHint=yes for session 8 (x11)", lines)

    def test_one_source_reporting_a_lock_wins_over_an_unreadable_other(self) -> None:
        reads = FakeReads(gdbus=display.Unreadable("gdbus exited 1"), **{"show-session": "LockedHint=yes\n"})
        self.assertEqual(self.run_check(reads)[0], 1)
        reads = FakeReads(gdbus="(true,)\n", **{"show-user": display.Unreadable("FileNotFoundError: loginctl")})
        self.assertEqual(self.run_check(reads)[0], 1)

    def test_lock_and_foreign_process_are_both_reported(self) -> None:
        processes = {display.BINARY_PATTERN: [(4242, "/opt/gitturtle")]}
        status, lines = self.run_check(FakeReads(gdbus="(true,)\n"), processes)
        self.assertEqual((status, lines[-1]), (1, "FOREIGN processes or windows present; session LOCKED"))

    def test_one_readable_unlocked_source_is_enough(self) -> None:
        status, lines = self.run_check(FakeReads(gdbus=display.Unreadable("no bus")))
        self.assertEqual(status, 0)
        self.assertIn("GNOME ScreenSaver: unreadable (no bus)", lines)

    def assert_inconclusive(self, reads) -> None:
        status, lines = self.run_check(reads)
        self.assertEqual(status, 2)
        self.assertIn("session lock: INCONCLUSIVE (neither GNOME ScreenSaver nor logind could be read)", lines)
        self.assertEqual(lines[-1], "INCONCLUSIVE")
        # As with an unreadable display, a foreign process still fails rather than being inconclusive.
        processes = {display.DRIVER_PATTERN: [(7, "python3 d3_run.py")]}
        self.assertEqual(self.run_check(reads, processes)[0], 1)

    def test_no_session_bus_and_no_graphical_session_is_inconclusive(self) -> None:
        self.assert_inconclusive(FakeReads(gdbus=display.Unreadable("gdbus exited 1: Cannot autolaunch D-Bus"),
                                           **{"show-user": "Display=\n"}))

    def lock_commands(self, effect):
        """Replaces subprocess.run for gdbus and loginctl only; `date -u` still runs."""
        real = subprocess.run

        def run(argv, **kwargs):
            if argv[0] not in ("gdbus", "loginctl"):
                return real(argv, **kwargs)
            self.assertEqual(kwargs["timeout"], display.PROCESS_TIMEOUT_SECONDS)
            return effect(argv)

        return mock.patch.object(display.subprocess, "run", run)

    def test_missing_commands_are_inconclusive(self) -> None:
        def missing(argv):
            raise FileNotFoundError(2, "No such file or directory", argv[0])

        with self.lock_commands(missing):
            self.assert_inconclusive(display.run_read)

    def test_timeouts_are_inconclusive(self) -> None:
        def slow(argv):
            raise subprocess.TimeoutExpired(argv, display.PROCESS_TIMEOUT_SECONDS)

        with self.lock_commands(slow):
            self.assert_inconclusive(display.run_read)

    def test_unparseable_output_is_inconclusive(self) -> None:
        self.assert_inconclusive(FakeReads(gdbus="(uint32 1,)\n", **{"show-session": "Type=wayland\n"}))
        self.assert_inconclusive(FakeReads(gdbus="", **{"show-user": "Display=../x\n"}))
        self.assert_inconclusive(FakeReads(gdbus="(true)", **{"show-session": "LockedHint=maybe\n"}))

    def test_failing_command_is_unreadable(self) -> None:
        failed = subprocess.CompletedProcess([], 1, "", "Error: no such name\n")
        with self.lock_commands(lambda argv: failed):
            with self.assertRaisesRegex(display.Unreadable, "gdbus exited 1: Error: no such name"):
                display.run_read(display.screensaver_argv())

    def test_every_command_only_reads(self) -> None:
        argvs = []
        for reads in (FakeReads(), FakeReads(gdbus="(true,)\n", **{"show-session": "LockedHint=yes\n"}),
                      FakeReads(gdbus=display.Unreadable("x"), **{"show-user": "Display=\n"})):
            self.run_check(reads)
            argvs += reads.calls
        argvs += [display.screensaver_argv(), display.logind_user_argv(UID), display.logind_session_argv("8")]
        for argv in argvs:
            self.assertFalse(WRITE_VERBS & set(argv), argv)
            if argv[0] == "gdbus":
                self.assertEqual(argv[1], "call")
                self.assertEqual(argv[argv.index("--method") + 1], "org.gnome.ScreenSaver.GetActive")
            else:
                self.assertEqual(argv[0], "loginctl")
                self.assertIn(argv[1], {"show-user", "show-session"})

    def test_bus_fallback_only_adds_the_user_socket(self) -> None:
        with mock.patch.dict(os.environ, {"DBUS_SESSION_BUS_ADDRESS": "unix:path=/x"}):
            self.assertEqual(display.session_env()["DBUS_SESSION_BUS_ADDRESS"], "unix:path=/x")
        with mock.patch.dict(os.environ, {"DBUS_SESSION_BUS_ADDRESS": ""}):
            with mock.patch.object(display.Path, "exists", lambda path: True):
                self.assertEqual(display.session_env()["DBUS_SESSION_BUS_ADDRESS"], f"unix:path=/run/user/{UID}/bus")
            with mock.patch.object(display.Path, "exists", lambda path: False):
                self.assertEqual(display.session_env()["DBUS_SESSION_BUS_ADDRESS"], "")


if __name__ == "__main__":
    unittest.main()
