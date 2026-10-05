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
HAVE_PIL = importlib.util.find_spec("PIL") is not None


class FakeClock:
    """Seconds that pass only when a fake grab, key or pause says so."""

    def __init__(self) -> None:
        self.now = 0.0

    def __call__(self) -> float:
        return self.now

    def advance(self, seconds: float) -> None:
        self.now += seconds


class ScriptedScreen:
    """A driver whose window shows scripted frames: press N of a key or of a pointer button starts script N, a
    list of (ms after the press, frame); before the first press the first script's first frame shows, and between
    presses the last one shown stays. Each grab takes `grab_ms` and returns the box's BGRX bytes as GetImage does;
    a key's send, or a button press, takes `send_ms`. `events` lists the input in order, with the grabs made
    before each. The pointer leaves its aimed point during the next `drifts` waits for a quiet region (so
    `on_window` fails until it is aimed again), and grab number `fail_at` raises, as a failed GetImage would."""

    def __init__(self, scripts, clock: FakeClock, size=(40, 30), grab_ms: float = 2.0, send_ms: float = 1.0,
                 drifts: int = 0, fail_at: int | None = None) -> None:
        self.scripts, self.clock, self.window = scripts, clock, size
        self.grab_ms, self.send_ms = grab_ms, send_ms
        self.drifts, self.fail_at = drifts, fail_at
        self.keys: list = []
        self.events: list = []
        self.presses = self.parked = self.grabs = 0
        self.pressed = None
        self.aimed = False
        self.shown = scripts[0][0][1]

    def on_window(self, x, y) -> bool:
        if self.aimed and self.drifts:  # the pointer moved while the probe waited for a quiet region
            self.drifts -= 1
            self.aimed = False
        return self.aimed

    def size(self):
        return self.window

    def park(self) -> None:
        self.parked += 1

    def press(self) -> None:
        self.presses += 1
        self.pressed = self.clock()
        self.clock.advance(self.send_ms / 1000)

    def key(self, name, mods=(), note=None, wait=0.0) -> None:
        self.keys.append((name, tuple(mods), note))
        self.events.append(("key", name, self.grabs))
        self.press()

    def aim(self, x, y, note=None, settle=0.8) -> None:
        self.events.append(("aim", x, y, self.grabs))
        self.aimed = True

    def button(self, down, number=1, note=None) -> None:
        self.events.append(("down" if down else "up", number, self.grabs))
        if down:
            self.press()

    def grab(self, box) -> bytes:
        self.clock.advance(self.grab_ms / 1000)
        self.grabs += 1
        if self.grabs == self.fail_at:
            raise RuntimeError("GetImage failed")
        if self.pressed is not None:
            since = (self.clock() - self.pressed) * 1000
            for at, frame in self.scripts[self.presses - 1]:
                if since >= at - 1e-6:
                    self.shown = frame
        return self.shown.crop(tuple(box)).tobytes("raw", "BGRX")


def solid(colour, size=(40, 30)):
    from PIL import Image

    return Image.new("RGB", size, colour)


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class ProbePressTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        # Built here, not in the class body, so the module imports where Pillow is missing.
        cls.A, cls.B, cls.C = solid((10, 10, 10)), solid((20, 20, 20)), solid((30, 30, 30))

    def press(self, script, quiet=0.051, timeout=0.5, cap=session.PROBE_FRAME_CAP, budget=session.PROBE_BYTE_BUDGET):
        clock = FakeClock()
        screen = ScriptedScreen([script], clock)
        return session.probe_press(lambda: screen.grab((0, 0, 40, 30)), lambda: screen.key("Tab"), quiet, timeout,
                                   cap, clock, budget=budget)

    def test_every_distinct_frame_is_kept_with_its_timing(self) -> None:
        # Grabs return every 2 ms; the key's send takes 1 ms, so grab k after it returns at 2k ms, 2k + 1 after the
        # key went down: B shows from grab 4 (at 9 ms after the key), C from grab 7 (at 15 ms).
        record, raw, gaps = self.press([(0, self.A), (9, self.B), (15, self.C)])
        self.assertEqual([(f["index"], f["ms"], f["previous_ms"], f["last_ms"], f["grabs"]) for f in record["frames"]],
                         [(0, -1.0, None, 6.0, 4), (1, 8.0, 6.0, 12.0, 3), (2, 14.0, 12.0, 66.0, 27)])
        self.assertEqual(raw, [frame.tobytes("raw", "BGRX") for frame in (self.A, self.B, self.C)])
        self.assertEqual((record["ended"], record["changed"], record["settled"]), ("quiet", True, True))
        self.assertEqual((record["send_ms"], record["first_grab_ms"], record["first_change_ms"],
                          record["last_change_ms"], record["end_ms"], record["grabs"]),
                         (1.0, 2.0, 8.0, 14.0, 66.0, 33))
        self.assertEqual(record["interval_ms"], dict(min=2.0, median=2.0, max=2.0))
        self.assertEqual(record["resolution_ms"], 3.0)  # the grab before the key to the first after it
        self.assertEqual(len(gaps), 32)

    def test_a_press_that_changes_nothing_or_never_settles(self) -> None:
        record, raw, _ = self.press([(0, self.A)], timeout=0.02)
        self.assertEqual((record["ended"], record["changed"], record["settled"], record["first_change_ms"]),
                         ("timeout", False, False, None))
        self.assertEqual((len(raw), record["grabs"], record["frames"][0]["grabs"]), (1, 10, 11))
        flicker = [(at, (self.A, self.B)[index % 2]) for index, at in enumerate(range(0, 400, 4))]
        record, raw, _ = self.press(flicker, timeout=0.1)
        self.assertEqual((record["ended"], record["settled"]), ("timeout", False))
        record, raw, _ = self.press(flicker, cap=3)
        self.assertEqual((record["ended"], record["truncated"], len(record["frames"]), len(raw)),
                         ("frame-cap", True, 4, 4))
        # Raw frames are also held to a byte budget: 40x30 BGRX frames are 4,800 bytes, so 3 reach 14,000.
        record, raw, _ = self.press(flicker, budget=14_000)
        self.assertEqual((record["ended"], record["truncated"], record["bytes"], len(raw)),
                         ("byte-budget", True, 14_400, 3))
        record, _, _ = self.press([(0, self.A), (9, self.B)])
        self.assertEqual((record["truncated"], record["bytes"]), (False, 9_600))

    def test_waiting_for_a_quiet_region(self) -> None:
        clock = FakeClock()
        screen = ScriptedScreen([[(0, self.A)]], clock)
        grab = lambda: screen.grab((0, 0, 40, 30))  # noqa: E731
        # From the first grab's return: 5 more, 12 ms apart. A timeout equal to `quiet` still settles.
        self.assertEqual(session.wait_quiet(grab, 1.0, 0.05, clock, clock.advance), 0.06)
        self.assertEqual(session.wait_quiet(grab, 0.05, 0.05, clock, clock.advance), 0.06)
        frames_ = iter([self.A, self.B] * 200)
        self.assertIsNone(session.wait_quiet(lambda: next(frames_).tobytes(), 0.5, 0.05, clock, clock.advance))


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

    @unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
    def test_a_probe_keeps_every_frame_after_each_press_under_probes(self) -> None:
        from PIL import Image

        clock = FakeClock()
        a, b, c = solid((10, 10, 10)), solid((20, 20, 20)), solid((30, 30, 30))
        b.putpixel((5, 5), (200, 0, 0))
        screen = ScriptedScreen([[(0, a), (9, b), (15, c)], [(0, c), (5, a)]], clock)
        locks = []
        run = self.open_session(lock_check=lambda: locks.append("checked") or False)
        run.driver, run.clock, run.pause = screen, clock, clock.advance
        with contextlib.redirect_stdout(io.StringIO()) as printed:
            run.run([{"probe": "rows", "send": "Tab", "mods": ["Shift_L"], "repeat": 2, "region": [2, 3, 22, 13],
                      "quiet": 0.051, "timeout": 0.5, "stable_within": 1.0, "note": "down the list"}])
        directory = self.root / "run" / "probes" / "rows"
        record = json.loads((directory / "probe.json").read_text())
        self.assertEqual(record, run.log["probes"][0])  # flow-log.json carries the same record
        self.assertEqual((record["region"], record["key"], record["mods"], record["parked"]),
                         ([2, 3, 22, 13], "Tab", ["Shift_L"], True))
        self.assertEqual(screen.keys, [("Tab", ("Shift_L",), "down the list")] * 2)
        self.assertEqual((screen.parked, locks), (1, ["checked"] * 3))  # the step's check, then one per press
        first, second = record["presses"]
        self.assertEqual([(f["file"], f["ms"]) for f in first["frames"]],
                         [("001-00.png", -1.0), ("001-01.png", 8.0), ("001-02.png", 14.0)])
        self.assertEqual([(f["file"], f["ms"]) for f in second["frames"]], [("002-00.png", -1.0), ("002-01.png", 4.0)])
        self.assertEqual(sorted(path.name for path in directory.iterdir()),
                         ["001-00.png", "001-01.png", "001-02.png", "002-00.png", "002-01.png", "probe.json"])
        with Image.open(directory / "001-01.png") as saved:
            self.assertEqual(saved.convert("RGB").tobytes(), b.crop((2, 3, 22, 13)).tobytes())
        self.assertEqual(first["frames"][1]["sha256"], session.identity.sha256_file(directory / "001-01.png"))
        self.assertEqual((record["frames"], record["grabs"], record["resolution_ms"]),
                         (3, first["grabs"] + second["grabs"], 3.0))
        self.assertEqual(record["interval_ms"], dict(min=2.0, median=2.0, max=2.0))
        self.assertIsNotNone(record["settled_before_s"])
        self.assertIn("probe rows: 2 press(es), 3 frames after them", printed.getvalue())
        self.assertEqual(run.log["captures"], [])  # nothing a probe grabs is a capture, so nothing is committed

    @unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
    def test_a_probe_stops_at_a_locked_desktop_and_refuses_a_region_outside_the_window(self) -> None:
        clock = FakeClock()
        a, b = solid((10, 10, 10)), solid((20, 20, 20))
        screen = ScriptedScreen([[(0, a), (5, b)], [(0, b), (5, a)]], clock)
        states = iter([False, False, True])
        run = self.open_session(lock_check=lambda: next(states))
        run.driver, run.clock, run.pause = screen, clock, clock.advance
        step = {"probe": "rows", "send": "Tab", "repeat": 2, "quiet": 0.051, "timeout": 0.5, "keep_pointer": True}
        with contextlib.redirect_stdout(io.StringIO()), \
                self.assertRaisesRegex(SystemExit, "locked before probe rows press 2; nothing sent"):
            run.run([step])
        self.assertEqual((len(screen.keys), screen.parked), (1, 0))
        record = json.loads((run.dirs.root / "probes" / "rows" / "probe.json").read_text())
        self.assertEqual((len(record["presses"]), record["region"]), (1, [0, 0, 40, 30]))  # the press it made
        run.lock_check = lambda: False
        with self.assertRaisesRegex(SystemExit, r"probe wide: region \[0, 0, 41, 30\] is not inside the 40x30 "
                                                r"window; nothing sent"):
            run.run([{"probe": "wide", "send": "Tab", "region": [0, 0, 41, 30]}])
        self.assertFalse((run.dirs.root / "probes" / "wide").exists())
        with self.assertRaisesRegex(SystemExit, "probe rows already exists in this run"):
            run.run([step])
        self.assertEqual(len(screen.keys), 1)

    @unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
    def test_a_click_probe_aims_presses_and_releases_elsewhere_after_its_frames(self) -> None:
        clock = FakeClock()
        a, b, c = solid((10, 10, 10)), solid((20, 20, 20)), solid((30, 30, 30))
        screen = ScriptedScreen([[(0, a), (7, b)], [(0, b), (7, c)]], clock)
        locks = []
        run = self.open_session(lock_check=lambda: locks.append("checked") or False)
        run.driver, run.clock, run.pause = screen, clock, clock.advance
        with contextlib.redirect_stdout(io.StringIO()):
            run.run([{"probe": "row", "click_at": [10, 8], "release_at": [30, 2], "repeat": 2, "region": [0, 4, 40, 30],
                      "quiet": 0.051, "timeout": 0.5, "stable_within": 0.1, "note": "press a partly visible row"}])
        kinds = [event[0] for event in screen.events]
        self.assertEqual(kinds, ["aim", "down", "aim", "up"] * 2)  # the release only once the frames are grabbed
        self.assertEqual([e[1:3] for e in screen.events if e[0] == "aim"], [(10, 8), (30, 2)] * 2)
        down, up = screen.events[1], screen.events[3]
        self.assertGreater(up[2] - down[2], 20)  # the press's grabs ran between them
        self.assertEqual((screen.parked, screen.keys, len(locks)), (0, [], 7))  # the step, then 3 checks a press
        record = run.log["probes"][0]
        self.assertEqual((record["key"], record["click_at"], record["release_at"], record["parked"]),
                         (None, [10, 8], [30, 2], False))
        self.assertEqual([(p["released"], p["frames"][1]["ms"]) for p in record["presses"]], [(True, 6.0), (True, 6.0)])
        self.assertTrue(all(p["settled_before_s"] is not None for p in record["presses"]))
        # Without release_at the button goes down and up together, as the press the probe times.
        clock2 = FakeClock()
        screen2 = ScriptedScreen([[(0, a), (7, b)]], clock2)
        run.driver, run.clock, run.pause = screen2, clock2, clock2.advance
        with contextlib.redirect_stdout(io.StringIO()):
            run.probe("field", click_at=(10, 8), quiet=0.051, timeout=0.5, stable_within=0.1)
        self.assertEqual([e[0] for e in screen2.events], ["aim", "down", "up"])
        self.assertEqual(screen2.events[1][2], screen2.events[2][2])  # no grab between them
        self.assertNotIn("released", run.log["probes"][1]["presses"][0])

    @unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
    def test_a_click_probe_refuses_bad_input_and_records_a_release_it_could_not_send(self) -> None:
        clock = FakeClock()
        a, b = solid((10, 10, 10)), solid((20, 20, 20))
        screen = ScriptedScreen([[(0, a), (5, b)]], clock)
        states = iter([False, False, False])  # the step, the pointer move and the press; locked from the release on
        run = self.open_session(lock_check=lambda: next(states, True))
        run.driver, run.clock, run.pause = screen, clock, clock.advance
        for kwargs, match in ((dict(send="Tab", click_at=(1, 1)), "exactly one of them"), (dict(), "exactly one"),
                              (dict(send="Tab", release_at=(1, 1)), "needs \"click_at\""),
                              (dict(click_at=(40, 1)), r"point \[40, 1\] is not inside the 40x30 window")):
            with self.subTest(kwargs=kwargs), self.assertRaisesRegex(SystemExit, match):
                run.probe("x", **kwargs)
        self.assertEqual(screen.events, [])
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as printed, \
                self.assertRaisesRegex(SystemExit, "locked before probe row release 1"):
            run.run([{"probe": "row", "click_at": [10, 8], "release_at": [30, 2], "quiet": 0.051, "timeout": 0.5,
                      "stable_within": 0.1}])
        self.assertEqual([e[0] for e in screen.events], ["aim", "down"])
        record = run.log["probes"][0]
        self.assertIs(record["presses"][0]["released"], False)
        self.assertEqual((record["owed_release"]["press"], record["owed_release"]["released"]), (1, False))
        self.assertIn("error: probe row: the button pressed at [10, 8] for press 1 was never released",
                      printed.getvalue())

    @unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
    def test_a_click_probe_checks_the_pointer_again_immediately_before_the_press(self) -> None:
        a, b = solid((10, 10, 10)), solid((20, 20, 20))
        step = {"probe": "row", "click_at": [10, 8], "quiet": 0.051, "timeout": 0.5, "stable_within": 0.1}
        # The pointer moves while the region settles: aimed again once, then pressed where it was aimed.
        clock = FakeClock()
        screen = ScriptedScreen([[(0, a), (5, b)]], clock, drifts=1)
        locks = []
        run = self.open_session(lock_check=lambda: locks.append("checked") or False)
        run.driver, run.clock, run.pause = screen, clock, clock.advance
        with contextlib.redirect_stdout(io.StringIO()):
            run.run([step])
        self.assertEqual([e[0] for e in screen.events], ["aim", "aim", "down", "up"])
        self.assertTrue(run.log["probes"][0]["presses"][0]["reaimed"])
        self.assertEqual(len(locks), 5)  # the step, then pointer and press checks for each of the two attempts
        self.assertIn("probe row: the pointer left [10, 8] before press 1; aiming again", run.log["input"])
        # It moves again after the second aim: refused, and no button was sent.
        clock = FakeClock()
        screen = ScriptedScreen([[(0, a), (5, b)]], clock, drifts=2)
        run.driver, run.clock, run.pause = screen, clock, clock.advance
        with contextlib.redirect_stdout(io.StringIO()), \
                self.assertRaisesRegex(SystemExit, r"the pointer left \[10, 8\] on the app window again before press "
                                                   r"1; no button sent"):
            run.run([dict(step, probe="row-2")])
        self.assertEqual([e[0] for e in screen.events], ["aim", "aim"])

    @unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
    def test_a_button_held_when_a_grab_fails_is_released_or_recorded(self) -> None:
        a, b = solid((10, 10, 10)), solid((20, 20, 20))
        step = {"probe": "row", "click_at": [10, 8], "release_at": [30, 2], "quiet": 0.051, "timeout": 0.5,
                "stable_within": 0.1}
        clock = FakeClock()
        screen = ScriptedScreen([[(0, a), (5, b)]], clock, fail_at=12)  # after the down, before any press is kept
        run = self.open_session(lock_check=lambda: False)
        run.driver, run.clock, run.pause = screen, clock, clock.advance
        with contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(RuntimeError, "GetImage failed"):
            run.run([step])
        self.assertEqual([e[0] for e in screen.events], ["aim", "down", "aim", "up"])
        record = json.loads((run.dirs.root / "probes" / "row" / "probe.json").read_text())
        self.assertEqual((record["presses"], record["owed_release"]), ([], dict(press=1, released=True)))
        # When the release cannot be sent either, the record and stderr say so.
        clock = FakeClock()
        screen = ScriptedScreen([[(0, a), (5, b)]], clock, fail_at=12)
        states = iter([False, False, False])
        run.lock_check = lambda: next(states, True)
        run.driver, run.clock, run.pause = screen, clock, clock.advance
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as printed, \
                self.assertRaisesRegex(RuntimeError, "GetImage failed"):
            run.run([dict(step, probe="row-2")])
        self.assertEqual([e[0] for e in screen.events], ["aim", "down"])
        owed = run.log["probes"][1]["owed_release"]
        self.assertEqual((owed["press"], owed["released"]), (1, False))
        self.assertIn("locked before probe row-2 release 1", owed["error"])
        self.assertIn("for press 1 was never released", printed.getvalue())

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

    def test_app_config_files_are_seeded_beside_the_store_and_their_digests_logged(self) -> None:
        files = {"recovery-drafts.json": b'{"version": 1, "drafts": []}', "activity.json": b"[]"}
        run = self.open_session(app_config_files=files)
        for name, data in files.items():
            self.assertEqual((run.dirs.preferences.parent / name).read_bytes(), data)
        self.assertEqual(json.loads(run.dirs.preferences.read_bytes())["settings"]["theme"], "midnight")
        digests = {name: stores.sha256(data) for name, data in sorted(files.items())}
        self.assertEqual(run.log["header"]["app_config_files"], digests)
        with contextlib.redirect_stdout(io.StringIO()):
            run.close()
        log = json.loads((self.root / "run" / "flow-log.json").read_text())
        self.assertEqual(log["header"]["app_config_files"], digests)
        self.assertEqual(self.open_session(run_dir=self.root / "plain").log["header"]["app_config_files"], {})

    def test_cli_launch_seeds_app_config_files_from_paths(self) -> None:
        from native_qa import mutter, qa

        drafts = self.root / "drafts.json"
        drafts.write_bytes(b'{"version": 1, "drafts": []}')
        run_dir = self.root / "run"
        seen = []

        def captured(*args, **kwargs):
            seen.append(kwargs)
            raise runenv.Refusal("stopped by the test before anything is created")

        argv = ["launch", "--binary", str(self.binary), "--fixture", str(self.fixture), "--run-dir", str(run_dir),
                "--input", "xtest"]
        with mock.patch.object(mutter, "process_argvs", return_value=[]), \
                contextlib.redirect_stderr(io.StringIO()) as err:
            with mock.patch.object(session, "Session", captured):
                self.assertEqual(qa.main([*argv, "--app-config", f"recovery-drafts.json={drafts}"]), 2)
            self.assertEqual(seen[0]["app_config_files"], {"recovery-drafts.json": drafts.read_bytes()})
            for extra, reason in (
                    (["--app-config", f"preferences.json={drafts}"], "'preferences.json' is the preference store"),
                    (["--app-config", f"a/b={drafts}"], "must be a plain file name"),
                    (["--app-config", f"x={drafts}", "--app-config", f"x={drafts}"], "--app-config x is given twice"),
                    (["--app-config", f"x={self.root / 'absent'}"], "--app-config x: cannot read")):
                with self.subTest(reason=reason):
                    self.assertEqual(qa.main([*argv, *extra]), 2)
                    self.assertIn(reason, err.getvalue())
                    self.assertFalse(run_dir.exists())

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
                       dict(home_files={"../outside": b"x"}), dict(home_files={".gitconfig": b"[user]"}),
                       dict(app_config_files={"preferences.json": b"{}"}), dict(app_config_files={"../x": b"x"})):
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
                        [{"wheel": [60, 400]}], [{"probe": "rows"}], [{"probe": "rows", "key": "Tab"}],
                        [{"probe": "rows", "send": "Tab", "click_at": [1, 2]}],
                        [{"probe": "rows", "send": "Tab", "release_at": [1, 2]}]):
                path.write_text(json.dumps(bad))
                with self.subTest(bad=bad), self.assertRaises(SystemExit):
                    session.load_scenario(path)
            path.write_text(json.dumps([{"read_only": "config/gitturtle", "note": "force a save error"},
                                        {"read_only": "data/gitturtle"}, {"probe": "rows", "send": "Tab"},
                                        {"probe": "row", "click_at": [5, 5], "release_at": [5, 1]}]))
            self.assertEqual(len(session.load_scenario(path)), 4)
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
