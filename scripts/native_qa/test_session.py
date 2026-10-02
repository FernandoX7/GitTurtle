from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

from native_qa import runenv, session, stores

QA = Path(__file__).resolve().parent / "qa.py"
INFO = {"application": "GitTurtle", "version": "0.1.0", "source_revision": "c" * 40, "source_tree": "clean",
        "target": "x86_64-unknown-linux-gnu", "profile": "debug", "rustc": "rustc 1.98.0",
        "build_unix_seconds": "1"}


@unittest.skipUnless(shutil.which("git"), "no git")
class SessionTest(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()
        self.binary = self.root / "gitturtle"
        self.binary.write_text(f"#!/bin/sh\nprintf '%s\\n' '{json.dumps(INFO)}'\n")
        self.binary.chmod(0o755)
        self.fixture = self.root / "fixture"
        subprocess.run(["git", "init", "-q", str(self.fixture)], check=True)

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def test_session_seeds_everything_and_records_an_unchanged_fixture(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()) as warned:
            run = session.Session(self.binary, self.fixture, self.root / "run", stores.store_text("porcelain"))
        self.assertIn("outside /tmp/gitturtle-evidence", warned.getvalue())
        header = run.log["header"]
        self.assertEqual(header["binary"]["build_info"], INFO)
        self.assertTrue(any("outside /tmp/gitturtle-evidence" in w for w in header["warnings"]))
        self.assertEqual(header["env"]["DISPLAY"], ":1")
        self.assertEqual(header["env"]["XDG_CONFIG_HOME"], str(self.root / "run" / "config"))
        self.assertNotIn("WAYLAND_DISPLAY", run.env)
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertIsNone(run.close())  # nothing was launched, so nothing is signalled
        log = json.loads((self.root / "run" / "flow-log.json").read_text())
        self.assertTrue(log["fixture_unchanged"])
        self.assertEqual(log["store_final"]["settings"]["theme"], "porcelain")

    @unittest.skipUnless(importlib.util.find_spec("PIL"), "Pillow is not installed")
    def test_scenario_steps_reach_the_driver_and_captures_park_unless_kept(self) -> None:
        from PIL import Image

        calls = []

        class FakeDriver:
            def __getattr__(self, name):
                return lambda *args, **kwargs: calls.append((name, args))

            def snap(self):
                calls.append(("snap", ()))
                return Image.new("RGB", (4, 4))

            def stable(self, timeout, quiet=0.5):
                return 0.5

        with contextlib.redirect_stderr(io.StringIO()):
            run = session.Session(self.binary, self.fixture, self.root / "run", stores.store_text())
        run.driver = FakeDriver()
        with contextlib.redirect_stdout(io.StringIO()):
            run.run([{"key": "comma", "mods": ["Control_L"]}, {"wheel": [60, 400], "steps": 18}, {"stable": 1},
                     {"capture": "rest"}, {"glide": [772, 410], "settle": 0}, {"press": 1},
                     {"capture": "pressed", "keep_pointer": True}, {"release": 1}])
        names = [name for name, _ in calls]
        self.assertEqual(names, ["key", "wheel", "park", "snap", "glide", "button", "snap", "button"])
        self.assertEqual(calls[1][1], (60, 400, 18))
        self.assertEqual([c["parked"] for c in run.log["captures"]], [True, False])
        self.assertTrue((self.root / "run" / "captures" / "pressed.png").is_file())
        with self.assertRaises(SystemExit):
            run.run([{"capture": "rest"}])  # a run never overwrites its own capture

    @unittest.skipUnless(importlib.util.find_spec("PIL"), "Pillow is not installed")
    def test_palette_marks_repeats_and_the_lock_check(self) -> None:
        from PIL import Image

        calls, locked = [], [False]

        class FakeDriver:
            """Each snap differs from the last unless `frozen`, so every wait_change sees a change; with `caret`,
            only one pixel of it changes, as a blinking caret does."""
            frozen = caret = False
            count = 0

            def __getattr__(self, name):
                return lambda *args, **kwargs: calls.append((name, args))

            def snap(self):
                if not self.frozen:
                    self.count += 1
                if self.caret:
                    image = Image.new("RGB", (4, 4))
                    image.putpixel((0, 0), (self.count % 256, 0, 0))
                    return image
                return Image.new("RGB", (4, 4), (self.count % 256, 0, 0))

            def stable(self, timeout, quiet=0.5):
                calls.append(("stable", (timeout, quiet)))
                return 0.5

            def wait_change(self, before, timeout=3.0):
                changed = self.snap()
                return (None if self.frozen else 0.01), changed

        with contextlib.redirect_stderr(io.StringIO()):
            run = session.Session(self.binary, self.fixture, self.root / "run", stores.store_text(),
                                  lock_check=lambda: locked[0])
        run.driver = FakeDriver()
        with contextlib.redirect_stdout(io.StringIO()):
            run.run([{"mark": "before"}, {"palette": "browse reflog"},
                     {"key": "Tab", "mods": ["Shift_L"], "repeat": 3, "await_change": 1.0, "wait_after": 0},
                     {"capture": "after", "stable_within": 2.0, "quiet": 0.7}])
        keys = [args for name, args in calls if name == "key"]
        self.assertEqual(keys[:2], [("p", ("Control_L", "Shift_L"), "command palette"),
                                    ("Return", (), "run 'browse reflog'")])
        self.assertEqual(keys[2:], [("Tab", ["Shift_L"], None)] * 3)
        self.assertIn(("type", ("browse reflog", "palette query")), calls)
        self.assertIn(("stable", (2.0, 0.7)), calls)
        self.assertEqual(sorted(run.frames), ["after", "before"])
        self.assertTrue((self.root / "run" / "marks" / "00-before.png").is_file())
        self.assertEqual(sum(1 for c in run.log["checks"] if c.get("check") == "key Tab first change"), 3)
        self.assertEqual(run.log["captures"][0]["stable_s"], 0.5)
        # A palette that never opens types nothing.
        run.driver.frozen = True
        calls.clear()
        with self.assertRaisesRegex(SystemExit, "did not open"):
            run.run([{"palette": "browse reflog"}])
        self.assertNotIn("type", [name for name, _ in calls])
        # Nor does one where only a caret blinks: the palette's modal dims the whole window.
        run.driver.frozen, run.driver.caret = False, True
        with self.assertRaisesRegex(SystemExit, "did not open"):  # waits the full 3 s
            run.run([{"palette": "browse reflog"}])
        self.assertNotIn("type", [name for name, _ in calls])
        run.driver.caret = False
        # Nothing is sent while the desktop is locked or its state is unknown.
        for state in (True, None):
            locked[0] = state
            calls.clear()
            with self.subTest(state=state), self.assertRaisesRegex(SystemExit, "nothing sent"):
                run.run([{"key": "Tab"}])
            self.assertEqual(calls, [])
        run.run([{"wait": 0}, {"stable": 0.1}])  # waiting sends nothing, so it needs no check

    def open_session(self, run_dir: Path | None = None, **kwargs) -> session.Session:
        with contextlib.redirect_stderr(io.StringIO()):
            return session.Session(self.binary, self.fixture, run_dir or self.root / "run", stores.store_text(),
                                   **kwargs)

    def test_read_only_paths_are_restored_when_the_launch_raises_after_the_step(self) -> None:
        class FailingDriver:
            def key(self, *args, **kwargs):
                raise RuntimeError("the app went away")

        run = self.open_session(lock_check=lambda: True)  # a locked desktop: the step sends no input, so it still runs
        themes = run.dirs.preferences.parent
        themes.chmod(0o755)
        before = session.fixture_state(self.fixture)
        run.driver = FailingDriver()
        with contextlib.redirect_stdout(io.StringIO()):
            try:
                run.run([{"read_only": "config/gitturtle", "note": "force a save error"}])
                self.assertEqual(themes.stat().st_mode & 0o7777, 0o500)
                run.lock_check = lambda: False
                with self.assertRaisesRegex(RuntimeError, "went away"):
                    run.run([{"key": "Return"}])
            finally:
                run.close()
        self.assertEqual(themes.stat().st_mode & 0o7777, 0o755)
        self.assertEqual(run.restore_failures, [])
        log = json.loads((self.root / "run" / "flow-log.json").read_text())
        self.assertEqual(log["read_only"], [dict(path="config/gitturtle", old_mode="0755", new_mode="0500",
                                                 at_utc=log["read_only"][0]["at_utc"], restored=True)])
        self.assertEqual(log["input"], ["read_only config/gitturtle: mode 0755 -> 0500  # force a save error",
                                        "restore config/gitturtle: mode 0500 -> 0755"])
        self.assertTrue(log["fixture_unchanged"])
        self.assertEqual(session.fixture_state(self.fixture), before)

    def test_sigterm_ends_the_launch_through_close_which_restores_the_modes(self) -> None:
        previous = signal.getsignal(signal.SIGTERM)
        run = self.open_session()
        run.dirs.preferences.parent.chmod(0o755)
        run.dirs.preferences.chmod(0o644)
        with contextlib.redirect_stdout(io.StringIO()):
            try:
                with self.assertRaisesRegex(SystemExit, "SIGTERM"):
                    run.run([{"read_only": "config/gitturtle/preferences.json"}, {"read_only": "config/gitturtle"}])
                    os.kill(os.getpid(), signal.SIGTERM)
                    time.sleep(5)
                self.assertEqual(signal.getsignal(signal.SIGTERM), signal.SIG_IGN)  # a second one cannot interrupt
            finally:
                run.close()
        self.assertEqual(signal.getsignal(signal.SIGTERM), previous)
        self.assertEqual(run.dirs.preferences.parent.stat().st_mode & 0o7777, 0o755)
        self.assertEqual(run.dirs.preferences.stat().st_mode & 0o7777, 0o644)
        restores = [line for line in run.log["input"] if line.startswith("restore")]
        self.assertEqual(restores, ["restore config/gitturtle: mode 0500 -> 0755",  # the last locked goes first
                                    "restore config/gitturtle/preferences.json: mode 0444 -> 0644"])

    def test_sigterm_just_after_the_mode_changed_waits_until_the_path_is_recorded(self) -> None:
        previous = signal.getsignal(signal.SIGTERM)
        run = self.open_session()
        themes = run.dirs.preferences.parent
        themes.chmod(0o755)
        real = os.fchmod

        def terminated(fd, mode):
            real(fd, mode)
            os.kill(os.getpid(), signal.SIGTERM)  # between the mode change and its record

        with contextlib.redirect_stdout(io.StringIO()):
            try:
                with mock.patch.object(session.runenv.os, "fchmod", terminated), \
                        self.assertRaisesRegex(SystemExit, "SIGTERM"):
                    run.run([{"read_only": "config/gitturtle"}])
                self.assertEqual([locked.path for locked, _ in run.locked], ["config/gitturtle"])
                self.assertEqual(themes.stat().st_mode & 0o7777, 0o500)
            finally:
                run.close()
        self.assertEqual(themes.stat().st_mode & 0o7777, 0o755)
        self.assertEqual(run.log["read_only"][0]["restored"], True)
        self.assertEqual(signal.getsignal(signal.SIGTERM), previous)
        self.assertNotIn(signal.SIGTERM, signal.pthread_sigmask(signal.SIG_BLOCK, ()))

    def test_sigterm_during_the_restore_waits_until_every_mode_is_back(self) -> None:
        received = []
        self.addCleanup(signal.signal, signal.SIGTERM,
                        signal.signal(signal.SIGTERM, lambda signum, frame: received.append(signum)))
        run = self.open_session()
        run.dirs.preferences.parent.chmod(0o755)
        run.dirs.preferences.chmod(0o644)
        real = session.runenv.restore_mode

        def terminated(locked):
            if not received:
                os.kill(os.getpid(), signal.SIGTERM)  # before the first of two restores
            real(locked)

        with contextlib.redirect_stdout(io.StringIO()):
            run.run([{"read_only": "config/gitturtle/preferences.json"}, {"read_only": "config/gitturtle"}])
            with mock.patch.object(session.runenv, "restore_mode", terminated):
                run.close()
        self.assertEqual(received, [signal.SIGTERM])  # delivered once, to the tool's own handler, afterwards
        self.assertEqual(run.dirs.preferences.parent.stat().st_mode & 0o7777, 0o755)
        self.assertEqual(run.dirs.preferences.stat().st_mode & 0o7777, 0o644)
        log = json.loads((self.root / "run" / "flow-log.json").read_text())
        self.assertEqual([entry["restored"] for entry in log["read_only"]], [True, True])
        self.assertNotIn(signal.SIGTERM, signal.pthread_sigmask(signal.SIG_BLOCK, ()))

    def test_a_failed_restore_is_reported(self) -> None:
        run = self.open_session()
        themes = run.dirs.preferences.parent
        with contextlib.redirect_stdout(io.StringIO()):
            run.run([{"read_only": "config/gitturtle"}])
        try:
            with mock.patch.object(session.runenv.os, "fchmod", side_effect=PermissionError(1, "Operation not permitted")), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as printed:
                run.close()
        finally:
            themes.chmod(0o755)
        self.assertIn("error: could not restore mode", printed.getvalue())
        self.assertEqual(len(run.restore_failures), 1)
        entry = json.loads((self.root / "run" / "flow-log.json").read_text())["read_only"][0]
        self.assertEqual((entry["restored"], entry["error"]), (False, "[Errno 1] Operation not permitted"))

    def test_read_only_refuses_a_path_in_the_fixture(self) -> None:
        run = self.open_session(run_dir=self.fixture / "run")  # a run directory inside the fixture's work tree
        themes = run.dirs.preferences.parent
        mode = themes.stat().st_mode
        with self.assertRaisesRegex(SystemExit, "overlaps the fixture"):
            run.run([{"read_only": "config/gitturtle"}])
        self.assertEqual(themes.stat().st_mode, mode)
        self.assertEqual(run.locked, [])

    def test_home_files_are_seeded_before_the_launch_and_their_digests_logged(self) -> None:
        files = {".local/state/omarchy/current/theme/colors.toml": b'accent = "#7aa2f7"\n',
                 ".local/state/omarchy/current/theme.name": b"tokyo-night\n"}
        run = self.open_session(home_files=files)
        home = run.dirs.paths["HOME"]
        for relative, data in files.items():
            self.assertEqual((home / relative).read_bytes(), data)
        digests = {path: stores.sha256(data) for path, data in sorted(files.items())}
        self.assertEqual(run.log["header"]["home_files"], digests)
        with contextlib.redirect_stdout(io.StringIO()):
            run.close()
        self.assertEqual(json.loads((self.root / "run" / "flow-log.json").read_text())["header"]["home_files"], digests)
        self.assertEqual(self.open_session(run_dir=self.root / "plain").log["header"]["home_files"], {})

    def test_reading_steps_send_nothing_and_keep_their_readings(self) -> None:
        focused = dict(name="Follow system appearance", role="toggle button", states=["focused", "focusable"],
                       depth=5)
        found = dict(pid=4242, seconds=0.1, application="GitTurtle", nodes=40, truncated=False, focused=focused,
                     all_focused=[focused])
        asked = []

        def read_focus(pid, within):
            asked.append((pid, within))
            return dict(found)

        run = self.open_session(lock_check=lambda: True)  # locked: a reading still runs, since it sends nothing
        run.driver = None  # any input would fail
        run.proc = mock.Mock(pid=4242)
        with mock.patch.object(session.a11y, "read_focus", read_focus), contextlib.redirect_stdout(io.StringIO()):
            run.run([{"atspi_focus": "switch", "within": 2.0, "note": "Switch focused"},
                     {"store_snapshot": "seeded", "quiet": 0.1, "within": 2.0}])
        self.assertEqual(asked, [(4242, 2.0)])
        self.assertEqual(sorted(run.readings), ["seeded", "switch"])
        switch = run.readings["switch"]
        self.assertEqual((switch["kind"], switch["note"], switch["focused"]), ("atspi_focus", "Switch focused", focused))
        seeded = run.readings["seeded"]
        self.assertEqual((seeded["kind"], seeded["path"], seeded["stable"]),
                         ("store_snapshot", "config/gitturtle/preferences.json", True))
        self.assertEqual(seeded["sha256"], stores.sha256(run.dirs.preferences.read_bytes()))
        self.assertEqual(seeded["json"]["settings"]["theme"], "midnight")
        with self.assertRaisesRegex(SystemExit, "reading switch was already taken"):
            run.run([{"store_snapshot": "switch"}])
        # No application on the bus, a store that never settles, or a path outside the XDG homes stops the launch.
        lost = dict(found, application=None, focused=None, all_focused=[], error="no AT-SPI application with pid 4242")
        with mock.patch.object(session.a11y, "read_focus", lambda pid, within: lost), \
                self.assertRaisesRegex(SystemExit, "atspi_focus gone: no AT-SPI application"):
            run.run([{"atspi_focus": "gone"}])
        self.assertIn("gone", run.readings)  # kept for the record all the same
        # AT-SPI that cannot even be initialised or read is the tool's failure: a refusal (inconclusive), not a
        # finding on the build.
        with mock.patch.object(session.a11y, "read_focus", side_effect=RuntimeError("atspi_init failed")), \
                self.assertRaisesRegex(runenv.Refusal, r"refusing: atspi_focus broken: AT-SPI could not be read "
                                                       r"\(RuntimeError\('atspi_init failed'\)\)"):
            run.run([{"atspi_focus": "broken"}])
        self.assertEqual((run.readings["broken"]["focused"], run.readings["broken"]["pid"]), (None, 4242))
        busy = {key: seeded[key] for key in ("waited_s", "quiet_s", "exists", "sha256", "bytes", "mtime_ns", "inode",
                                             "json")}
        with mock.patch.object(session.stores, "snapshot", lambda *args, **kwargs: dict(busy, stable=False)), \
                self.assertRaisesRegex(SystemExit, "still changing after 10.0 s"):
            run.run([{"store_snapshot": "busy"}])
        for path in ("home/.config/x", "/config/gitturtle/preferences.json", "config/../../x"):
            with self.subTest(path=path), self.assertRaisesRegex(SystemExit, "store_snapshot bad"):
                run.run([{"store_snapshot": "bad", "path": path}])
        run.proc = None
        with contextlib.redirect_stdout(io.StringIO()):
            run.close()
        log = json.loads((self.root / "run" / "flow-log.json").read_text())
        self.assertEqual([(r["label"], r["kind"]) for r in log["readings"]],
                         [("switch", "atspi_focus"), ("seeded", "store_snapshot"), ("gone", "atspi_focus"),
                          ("broken", "atspi_focus"), ("busy", "store_snapshot")])
        self.assertEqual(log["input"], [])

    def test_refusals_leave_no_run_directory(self) -> None:
        for kwargs in (dict(for_commit=True), dict(extra_env={"XDG_CONFIG_HOME": "/x"}),
                       dict(home_files={"../outside": b"x"}), dict(home_files={".gitconfig": b"[user]"})):
            with self.subTest(kwargs=kwargs), self.assertRaises(SystemExit):
                session.Session(self.binary, self.fixture, self.root / "run", b'{"version": 6}', **kwargs)
            self.assertFalse((self.root / "run").exists())

    def test_cli_refuses_a_relative_run_directory(self) -> None:
        result = subprocess.run([sys.executable, "-B", str(QA), "launch", "--binary", str(self.binary),
                                 "--fixture", str(self.fixture), "--run-dir", "relative-run"],
                                capture_output=True, text=True, cwd=self.root)
        self.assertEqual(result.returncode, 2, result.stdout)
        self.assertIn("refusing: run directory relative-run must be an absolute path", result.stderr)
        self.assertFalse((self.root / "relative-run").exists())


class ScenarioTest(unittest.TestCase):
    def test_default_and_validated_scenarios(self) -> None:
        self.assertEqual(session.load_scenario(None)[-1]["capture"], "00-launch")
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "s.json"
            path.write_text(json.dumps([{"key": "comma", "mods": ["Control_L"]}, {"capture": "rest"}, {"glide": [772, 410]},
                                        {"capture": "hover", "keep_pointer": True}]))
            self.assertEqual(len(session.load_scenario(path)), 4)
            for bad in ({"steps": []}, [{"key": "a", "glide": [1, 2]}], [{"hover": [1, 2]}], ["key"],
                        [{"wheel": [60, 400]}]):
                path.write_text(json.dumps(bad))
                with self.subTest(bad=bad), self.assertRaises(SystemExit):
                    session.load_scenario(path)
            path.write_text(json.dumps([{"read_only": "config/gitturtle", "note": "force a save error"},
                                        {"read_only": "data/gitturtle"}]))
            self.assertEqual(len(session.load_scenario(path)), 2)
            for bad in ("/config/gitturtle", "config/../home", "home/.config", "captures/x", "", None, 7):
                path.write_text(json.dumps([{"wait": 0}, {"read_only": bad}]))
                with self.subTest(bad=bad), self.assertRaisesRegex(SystemExit, r"scenario step 1 \(\$\[1\]\.read_only\)"):
                    session.load_scenario(path)
            path.write_text(json.dumps([{"atspi_focus": "switch"}, {"store_snapshot": "store"},
                                        {"store_snapshot": "themes", "path": "config/gitturtle/themes.json"}]))
            self.assertEqual(len(session.load_scenario(path)), 3)
            path.write_text(json.dumps([{"store_snapshot": "x", "path": "home/.gitconfig"}]))
            with self.assertRaisesRegex(SystemExit, r"scenario step 0 \(\$\[0\]\.path\)"):
                session.load_scenario(path)


class CommandLineTest(unittest.TestCase):
    def test_help_needs_no_optional_modules(self) -> None:
        probe = ("import runpy, sys\nsys.argv = ['qa.py', '--help']\n"
                 f"try:\n    runpy.run_path({str(QA)!r}, run_name='__main__')\n"
                 "except SystemExit:\n    pass\n"
                 "print(sorted({m.split('.')[0] for m in sys.modules} & {'PIL', 'Xlib', 'gi', 'dbus'}))")
        result = subprocess.run([sys.executable, "-B", "-c", probe], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("launch", result.stdout)
        self.assertTrue(result.stdout.rstrip().endswith("[]"), result.stdout)


if __name__ == "__main__":
    unittest.main()
