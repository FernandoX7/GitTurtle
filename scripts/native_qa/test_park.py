"""The pointer park in both input backends: never onto the app's window after keyboard input, nothing sent when the
pointer is already off it, every motion recorded, and a loud warning when a park cannot keep keyboard mode."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from native_qa import mutter, play, scenario, session, stores, x11
from native_qa.test_scenario import spec
from native_qa.test_session import INFO

HAVE_PIL = importlib.util.find_spec("PIL") is not None
FAKE_X = SimpleNamespace(MotionNotify=6, ButtonPress=4, ButtonRelease=5, KeyPress=2, KeyRelease=3,
                         RevertToParent=2, CurrentTime=0, Above=0, ZPixmap=2)
KEYSYMS = {"Tab": 0xFF09, "a": 0x61, "b": 0x62, "Shift_L": 0xFFE1, "Control_L": 0xFFE3, "Alt_L": 0xFFE9,
           "Super_L": 0xFFEB, "ISO_Level3_Shift": 0xFE03, "Mode_switch": 0xFF7E, "Num_Lock": 0xFF7F}
MODIFIERS = ("Shift_L", "Control_L", "Alt_L", "Super_L", "ISO_Level3_Shift", "Mode_switch", "Num_Lock")
FAKE_XK = SimpleNamespace(string_to_keysym=lambda name: KEYSYMS.get(name, 0))
TYPES = SimpleNamespace(UInt32=int, Int32=int, Double=float, Boolean=bool)
ROOT = (2304, 1440)
# Window root rectangles as (x, y, width, height), each with a point off it a park can reach.
WINDOWS = {"middle": (400, 300, 1000, 680), "bottom-right": (1304, 760, 1000, 680), "top-left": (0, 0, 1000, 680),
           "right half": (1304, 0, 1000, 1440)}


class Screen:
    """The X screen a driver acts on: the root's size, the app window's root rectangle and the true pointer.

    XTest warps the pointer and Mutter moves it by a relative motion kept on
    the screen; both put it straight at the motion's end, so `positions`,
    every point it took, is what the app could have seen.
    """

    def __init__(self, window=WINDOWS["middle"], root=ROOT, pointer=(700, 500)) -> None:
        self.ox, self.oy, self.width, self.height = window
        self.root = root
        self.pointer = pointer
        self.positions: list[tuple[int, int]] = []

    @property
    def rect(self) -> tuple[int, int, int, int]:
        return self.ox, self.oy, self.ox + self.width, self.oy + self.height

    def move_to(self, x, y) -> None:
        self.pointer = (min(max(int(x), 0), self.root[0] - 1), min(max(int(y), 0), self.root[1] - 1))
        self.positions.append(self.pointer)

    def on_window(self) -> list[tuple[int, int]]:
        return [point for point in self.positions if x11.inside(self.rect, *point)]


class FakeRoot:
    id = 1

    def __init__(self, screen: Screen) -> None:
        self.screen = screen

    def get_geometry(self):
        return SimpleNamespace(width=self.screen.root[0], height=self.screen.root[1])

    def query_pointer(self):
        return SimpleNamespace(root_x=self.screen.pointer[0], root_y=self.screen.pointer[1], same_screen=True)


class FakeApp:
    """The app's X window; XWayland reports the pointer's true position on it."""
    id = 100

    def __init__(self, screen: Screen) -> None:
        self.screen = screen

    def get_geometry(self):
        return SimpleNamespace(width=self.screen.width, height=self.screen.height)

    def translate_coords(self, root, x, y):
        return SimpleNamespace(x=x - self.screen.ox, y=y - self.screen.oy)

    def query_pointer(self):
        x, y = self.screen.pointer
        return SimpleNamespace(same_screen=True, win_x=x - self.screen.ox, win_y=y - self.screen.oy)

    def query_tree(self):
        return SimpleNamespace(parent=None)

    def get_image(self, x, y, width, height, fmt, mask):
        return SimpleNamespace(data=bytes(width * height * 4))


class FakeDisplay:
    """X input focus stays on the app; `sent` is every XTest event."""

    def __init__(self, screen: Screen) -> None:
        self.scr, self.root, self.app = screen, FakeRoot(screen), FakeApp(screen)
        self.sent: list[tuple] = []

    def screen(self):
        return SimpleNamespace(root=self.root)

    def sync(self) -> None:
        pass

    def get_input_focus(self):
        return SimpleNamespace(focus=self.app)

    def keysym_to_keycode(self, keysym) -> int:
        return 23

    def keycode_to_keysym(self, code, index) -> int:
        return 0xFF09


class FakeXTest:
    @staticmethod
    def fake_input(dsp, event, detail=0, x=None, y=None) -> None:
        dsp.sent.append((event, detail, x, y))
        if event == FAKE_X.MotionNotify:
            dsp.scr.move_to(x, y)


class FakeRemote:
    """A started RemoteDesktop session: a relative motion moves the screen's pointer; every call is kept."""

    def __init__(self, screen: Screen) -> None:
        self.screen, self.calls = screen, []

    def __getattr__(self, name):
        return lambda *args: self.calls.append((name, *args))

    def NotifyPointerMotionRelative(self, dx, dy) -> None:
        self.calls.append(("NotifyPointerMotionRelative", dx, dy))
        x, y = self.screen.pointer
        self.screen.move_to(x + dx, y + dy)


def x11_driver(screen: Screen, log: list[str] | None = None) -> x11.Driver:
    dsp = FakeDisplay(screen)
    return x11.Driver(dsp, dsp.app, [] if log is None else log)


def mutter_driver(screen: Screen, log: list[str] | None = None) -> mutter.MutterDriver:
    dsp = FakeDisplay(screen)
    return mutter.MutterDriver(dsp, dsp.app, [] if log is None else log,
                               mutter.RemoteDesktop(FakeRemote(screen), TYPES))


def motions_sent(driver) -> int:
    if isinstance(driver, mutter.MutterDriver):
        return sum(1 for call in driver.remote.session.calls if call[0] == "NotifyPointerMotionRelative")
    return sum(1 for event in driver.d.sent if event[0] == FAKE_X.MotionNotify)


class Patched(unittest.TestCase):
    def setUp(self) -> None:
        patches = [mock.patch.object(x11, "X", FAKE_X), mock.patch.object(x11, "XK", FAKE_XK),
                   mock.patch.object(x11, "xtest", FakeXTest), mock.patch.object(mutter, "keysym", FAKE_XK.string_to_keysym)]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        sleep = mock.patch("time.sleep")
        self.sleep = sleep.start()
        self.addCleanup(sleep.stop)

    def drivers(self, window=WINDOWS["middle"]):
        """Each backend's driver on its own screen showing `window`: (backend, screen, driver)."""
        for name, make in (("xtest", x11_driver), ("mutter", mutter_driver)):
            screen = Screen(window)
            yield name, screen, make(screen)


class PlanTest(unittest.TestCase):
    def test_the_park_target_is_off_the_window_or_none(self) -> None:
        for name, (x, y, width, height) in WINDOWS.items():
            with self.subTest(window=name):
                rect = (x, y, x + width, y + height)
                target = x11.park_target(rect, ROOT)
                self.assertFalse(x11.inside(rect, *target))
                self.assertTrue(0 < target[0] < ROOT[0] - 1 and 0 < target[1] < ROOT[1] - 1)
        self.assertEqual(x11.park_target((400, 300, 1400, 980), ROOT), (1520, 1100))  # below and right
        self.assertEqual(x11.park_target((1304, 760, 2304, 1440), ROOT), (1184, 640))  # above and left
        self.assertIsNone(x11.park_target((0, 0, *ROOT), ROOT))  # the window covers the screen

    def test_the_plan_enters_the_window_only_to_clear_a_hover(self) -> None:
        rect = (400, 300, 1400, 980)
        self.assertEqual(x11.plan_park(rect, ROOT, (700, 500), hover_pending=False),
                         [((1520, 1100), "off the window, 120 px past it")])
        self.assertEqual(x11.plan_park(rect, ROOT, (2000, 100), hover_pending=False), [])  # already off it
        self.assertEqual(x11.plan_park(rect, ROOT, None, hover_pending=False)[-1][0], (1520, 1100))
        cleared = x11.plan_park(rect, ROOT, (2000, 100), hover_pending=True)
        self.assertEqual([point for point, _ in cleared], [(404, 976), (1520, 1100)])
        self.assertIn("mouse mode already", cleared[0][1])
        self.assertEqual(x11.plan_park((0, 0, *ROOT), ROOT, (700, 500), hover_pending=False), [])

    def test_modifier_keysyms_are_the_ones_gpui_sends_no_key_down_for(self) -> None:
        for sym in (0xFFE1, 0xFFE3, 0xFFE9, 0xFFEB, 0xFFEE, 0xFE01, 0xFE03, 0xFE13, 0xFF7E, 0xFF7F):
            self.assertTrue(x11.modifier_keysym(sym), hex(sym))
        for sym in (0xFFE0, 0xFFEF, 0xFE00, 0xFE14, 0xFF09, 0xFF0D, 0xFF1B, 0x61, 0x20):
            self.assertFalse(x11.modifier_keysym(sym), hex(sym))

    def test_the_anchor_corner_is_off_the_window_and_never_the_hot_corner(self) -> None:
        cases = {"middle": ((1, 1), (2303, 1439)), "bottom-right": ((1, -1), (2303, 0)),
                 "right half": ((-1, 1), (0, 1439))}
        for name, expected in cases.items():
            x, y, width, height = WINDOWS[name]
            with self.subTest(window=name):
                self.assertEqual(mutter.anchor_corner((x, y, x + width, y + height), ROOT), expected)
        self.assertEqual(mutter.anchor_corner((0, 0, *ROOT), ROOT), ((1, 1), (2303, 1439)))  # no corner is off it
        self.assertNotIn((-1, -1), mutter.ANCHORS)


class DriverParkTest(Patched):
    def test_a_park_after_keys_never_moves_the_pointer_onto_the_window(self) -> None:
        for window in WINDOWS:
            for backend, screen, driver in self.drivers(WINDOWS[window]):
                with self.subTest(window=window, backend=backend):
                    driver.move(100, 100, "onto the window: mouse mode, with a hover there")
                    driver.key("Tab")
                    screen.positions.clear()
                    record = driver.park()
                    self.assertEqual(screen.on_window(), [])
                    self.assertEqual(len(screen.positions), 1)  # straight off: one motion
                    self.assertEqual((record["keyboard_mode"], record["entered_window"], record["off_window"]),
                                     (True, False, True))
                    self.assertEqual(record["start"], [screen.ox + 100, screen.oy + 100])
                    self.assertEqual(tuple(record["at"]), screen.pointer)
                    self.assertEqual((driver.keyboard_mode, driver.keyboard_mode_lost), (True, False))

    def test_a_park_with_the_pointer_already_off_the_window_sends_nothing(self) -> None:
        for backend, screen, driver in self.drivers():
            with self.subTest(backend=backend):
                driver.move(100, 100)
                driver.park()  # mouse mode: through the corner, then off
                driver.key("Tab")
                for _ in range(2):  # after a key, and again
                    sent, positions = motions_sent(driver), len(screen.positions)
                    self.sleep.reset_mock()
                    record = driver.park()
                    self.assertEqual((motions_sent(driver), len(screen.positions)), (sent, positions))
                    self.assertEqual((record["motions"], record["entered_window"], record["off_window"]),
                                     ([], False, True))
                    self.sleep.assert_called_once_with(0.8)  # the settle a capture after input still relies on
                    self.assertIn("park: nothing sent; the pointer is already off the window", driver.log[-1])

    def test_a_park_after_pointer_input_clears_the_hover_through_the_corner(self) -> None:
        for backend, screen, driver in self.drivers():
            with self.subTest(backend=backend):
                driver.move(100, 100)
                screen.positions.clear()
                record = driver.park()
                corner = (screen.ox + 4, screen.oy + screen.height - 4)
                self.assertEqual(screen.positions[0], corner)
                self.assertEqual(screen.on_window(), [corner])
                self.assertEqual([motion["inside"] for motion in record["motions"]], [True, False])
                self.assertEqual((record["keyboard_mode"], record["entered_window"], record["keyboard_mode_lost"]),
                                 (False, True, False))
                self.assertFalse(driver.hover_pending)

    def test_every_motion_a_park_sends_is_recorded(self) -> None:
        for backend, screen, driver in self.drivers():
            with self.subTest(backend=backend):
                driver.move(100, 100)
                screen.positions.clear()
                before, sent = len(driver.log), motions_sent(driver)
                record = driver.park()
                self.assertEqual([tuple(motion["root"]) for motion in record["motions"]], screen.positions)
                self.assertEqual(len(record["motions"]), motions_sent(driver) - sent)
                lines = [line for line in driver.log[before:] if line.startswith("park motion")]
                self.assertEqual(len(lines), len(record["motions"]))
                for motion, line in zip(record["motions"], lines):
                    x, y = motion["root"]
                    self.assertEqual(motion["window"], [x - screen.ox, y - screen.oy])
                    self.assertEqual(motion["inside"], x11.inside(screen.rect, x, y))
                    self.assertIn(f"root ({x},{y}), window ({x - screen.ox},{y - screen.oy}), "
                                  f"{'INSIDE' if motion['inside'] else 'off'} the window  # {motion['why']}", line)
                self.assertEqual(driver.log[-1], f"park off-window at root ({screen.pointer[0]},{screen.pointer[1]})")

    def test_input_sets_the_mode_a_park_relies_on(self) -> None:
        for backend, screen, driver in self.drivers():
            with self.subTest(backend=backend):
                mode = lambda: (driver.keyboard_mode, driver.hover_pending)  # noqa: E731
                self.assertEqual(mode(), (False, True))  # at launch: mouse mode, hover unknown
                driver.key("Tab")
                self.assertEqual(mode(), (True, False))
                driver.move(-50, 10)  # off the window: the app sees no motion
                self.assertEqual(mode(), (True, False))
                driver.move(100, 100)
                self.assertEqual(mode(), (False, True))
                driver.type("ab")
                self.assertEqual(mode(), (True, False))
                driver.button(True)  # a press on the window
                self.assertEqual(mode(), (False, True))
                driver.key("Tab")
                driver.button(False)  # a release changes nothing
                self.assertEqual(mode(), (True, False))

    def test_a_move_that_moves_nothing_changes_no_mode(self) -> None:
        for backend, screen, driver in self.drivers():
            with self.subTest(backend=backend):
                driver.move(100, 100)
                driver.key("Tab")
                sent = motions_sent(driver)
                driver.move(100, 100)  # the pointer is already there
                self.assertEqual(motions_sent(driver), sent)
                self.assertEqual((driver.keyboard_mode, driver.hover_pending), (True, False))
                driver.move(101, 100)
                self.assertEqual(motions_sent(driver), sent + 1)
                self.assertEqual((driver.keyboard_mode, driver.hover_pending), (False, True))

    def test_the_mode_follows_where_a_move_ends_not_where_it_was_aimed(self) -> None:
        for backend, screen, driver in self.drivers(WINDOWS["right half"]):
            with self.subTest(backend=backend):
                driver.move(-100, 100)  # left of the window
                driver.key("Tab")
                driver.move(1200, 100)  # aimed past the screen's right edge, which keeps the pointer on the window
                self.assertTrue(x11.inside(screen.rect, *screen.pointer))
                self.assertEqual((driver.keyboard_mode, driver.hover_pending), (False, True))
                if backend == "mutter":
                    self.assertEqual(driver.known, screen.pointer)  # tracked where Mutter keeps it

    def test_a_modifier_alone_leaves_the_mode_and_its_hover_to_clear(self) -> None:
        for backend, screen, driver in self.drivers():
            with self.subTest(backend=backend):
                driver.move(100, 100)
                for name in MODIFIERS:
                    driver.key(name)
                self.assertEqual((driver.keyboard_mode, driver.hover_pending), (False, True))
                screen.positions.clear()
                record = driver.park()
                self.assertEqual(screen.on_window(), [(screen.ox + 4, screen.oy + screen.height - 4)])
                self.assertFalse(record["keyboard_mode"])
                driver.key("a", ["Control_L"])  # a chord: its non-modifier key gets a KeyDown
                self.assertEqual((driver.keyboard_mode, driver.hover_pending), (True, False))

    def test_a_park_from_the_margin_itself_sends_only_the_move_off(self) -> None:
        for backend, screen, driver in self.drivers():
            with self.subTest(backend=backend):
                driver.move(4, screen.height - 4)  # mouse mode, resting where a park clears the hover
                screen.positions.clear()
                record = driver.park()
                self.assertEqual(screen.on_window(), [])
                self.assertEqual([motion["inside"] for motion in record["motions"]], [False])
                self.assertFalse(driver.hover_pending)
                self.assertTrue(any("park: the pointer already rests at root" in line for line in driver.log))

    def test_mutter_anchors_an_unknown_position_in_a_corner_off_the_window(self) -> None:
        for window, corner in (("middle", (2303, 1439)), ("bottom-right", (2303, 0)), ("right half", (0, 1439))):
            screen = Screen(WINDOWS[window], pointer=(1500, 700))
            driver = mutter_driver(screen)
            driver.keys_sent(KEYSYMS["Tab"])  # keyboard input, and the position is still unknown
            with self.subTest(window=window):
                record = driver.park()
                self.assertIsNone(record["start"])
                self.assertEqual(screen.on_window(), [])
                self.assertEqual(record["motions"][0]["root"], list(corner))
                self.assertIn("anchor", record["motions"][0]["why"])
                first = driver.remote.session.calls[0]
                self.assertEqual(first[0], "NotifyPointerMotionRelative")
                self.assertEqual(abs(first[1]), abs(first[2]))
                self.assertFalse(first[1] < 0 and first[2] < 0)  # never towards the top-left hot corner
                self.assertTrue(record["off_window"])


class UnavoidableTest(Patched):
    def test_a_park_that_must_enter_the_window_after_keys_is_flagged(self) -> None:
        screen = Screen((0, 0, *ROOT))  # the window covers the screen
        driver = mutter_driver(screen)
        driver.keys_sent(KEYSYMS["Tab"])  # keyboard input; the position is unknown, so the park anchors on the window
        record = driver.park()
        self.assertEqual((record["entered_window"], record["keyboard_mode"], record["keyboard_mode_lost"],
                          record["off_window"]), (True, True, True, False))
        self.assertTrue(driver.keyboard_mode_lost)
        self.assertIn("every allowed corner is on the window", record["motions"][0]["why"])
        self.assertTrue(any(line.startswith("WARNING: that motion took the app out of keyboard mode")
                            for line in driver.log))
        driver.key("Tab")  # keyboard mode again
        self.assertFalse(driver.keyboard_mode_lost)
        self.assertEqual(driver.park()["motions"], [])  # still nowhere off the window, and nothing to clear

    def test_xtest_leaves_the_pointer_on_a_covering_window_after_keys(self) -> None:
        screen = Screen((0, 0, *ROOT))
        driver = x11_driver(screen)
        driver.key("Tab")
        record = driver.park()
        self.assertEqual((record["motions"], record["entered_window"], record["off_window"]), ([], False, False))
        self.assertFalse(driver.keyboard_mode_lost)
        self.assertIn("no point is off the window", driver.log[-1])


@unittest.skipUnless(HAVE_PIL and shutil.which("git"), "Pillow or git is missing")
class SessionParkTest(Patched):
    def setUp(self) -> None:
        super().setUp()
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name).resolve()
        binary = self.root / "gitturtle"
        binary.write_text(f"#!/bin/sh\nprintf '%s\\n' '{json.dumps(INFO)}'\n")
        binary.chmod(0o755)
        subprocess.run(["git", "init", "-q", str(self.root / "fixture")], check=True)
        with contextlib.redirect_stderr(io.StringIO()):
            self.run_ = session.Session(binary, self.root / "fixture", self.root / "run", stores.store_text())

    def test_captures_after_keys_keep_keyboard_mode_and_every_park_is_logged(self) -> None:
        screen = Screen()
        self.run_.driver = x11_driver(screen, self.run_.log["input"])
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as err:
            self.run_.run([{"click": [100, 100]}, {"key": "Tab"}])
            screen.positions.clear()
            self.run_.run([{"capture": "focus"}, {"capture": "again"}, {"park": True},
                           {"mark": "end", "park_first": True}])
            self.run_.close()
        self.assertEqual(screen.on_window(), [])
        self.assertNotIn("WARNING", err.getvalue())
        log = json.loads((self.root / "run" / "flow-log.json").read_text())
        parks = log["parks"]
        self.assertEqual([park["why"] for park in parks], ["capture focus", "capture again", "park step", "mark end"])
        self.assertEqual([len(park["motions"]) for park in parks], [1, 0, 0, 0])
        self.assertTrue(all(park["keyboard_mode"] and not park["entered_window"] for park in parks))
        self.assertEqual([(c["parked"], c["park_entered_window"], "warning" in c) for c in log["captures"]],
                         [(True, False, False)] * 2)
        self.assertTrue(any(line.startswith("park motion to root") for line in log["input"]))

    def test_a_capture_after_a_park_left_keyboard_mode_warns(self) -> None:
        screen = Screen((0, 0, *ROOT))  # nowhere off the window
        self.run_.driver = mutter_driver(screen, self.run_.log["input"])
        self.run_.driver.keys_sent(KEYSYMS["Tab"])  # keyboard input before the pointer's position is known
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as err:
            self.run_.run([{"capture": "focus"}, {"key": "Tab"}, {"capture": "after-key"},
                           {"capture": "hover", "keep_pointer": True}])
        first, second, kept = self.run_.log["captures"]
        self.assertEqual((first["park_entered_window"], first["warning"]), (True, session.KEYBOARD_MODE_LOST))
        self.assertNotIn("warning", second)  # the key put the app back in keyboard mode
        self.assertEqual((kept["parked"], kept["park_entered_window"], "warning" in kept), (False, False, False))
        self.assertIn("WARNING: the park for capture focus moved the pointer onto the app's window", err.getvalue())
        self.assertIn(f"WARNING: capture focus.png: {session.KEYBOARD_MODE_LOST}", err.getvalue())


@unittest.skipUnless(shutil.which("git"), "no git")
class RunWarningTest(unittest.TestCase):
    def test_a_launch_lists_its_capture_warnings_and_the_run_repeats_them(self) -> None:
        class FakeSession:
            def __init__(self, binary, fixture, run_dir, store, **kwargs) -> None:
                self.log = dict(header=dict(started_utc="2026-10-02T10:00:00Z", ended_utc="2026-10-02T10:01:00Z"),
                                captures=[dict(capture="rest.png"),
                                          dict(capture="focus.png", warning=session.KEYBOARD_MODE_LOST)],
                                fixture_unchanged=True)
                self.restore_failures = []

            def launch(self) -> None:
                pass

            def run(self, steps) -> None:
                pass

            def close(self) -> int:
                self.log["exit"] = -15
                return -15

        loaded = scenario.validate(spec(steps=[{"wait": 0}], analyses=[]), "f" * 64)
        with tempfile.TemporaryDirectory() as scratch:
            fixture = Path(scratch) / "fixture"
            subprocess.run(["git", "init", "-q", str(fixture)], check=True)
            with mock.patch("native_qa.session.Session", FakeSession), contextlib.redirect_stdout(io.StringIO()):
                record = play.launch(loaded, "cand", loaded["variants"][0], Path("/x/cand"), fixture,
                                     Path(scratch) / "run", dict(display=":1", settle=0, backend="mutter"))
        self.assertIsNone(record["error"])  # a warning never fails the launch
        self.assertEqual(record["warnings"], [f"capture focus.png: {session.KEYBOARD_MODE_LOST}"])
        run = dict(launches=[record, dict(role="base", variant="midnight", error="refused before launch")])
        with contextlib.redirect_stdout(io.StringIO()) as out:
            play.warn(run)
        self.assertEqual(run["warnings"], [f"cand midnight: capture focus.png: {session.KEYBOARD_MODE_LOST}"])
        self.assertIn(f"WARNING: cand midnight: capture focus.png: {session.KEYBOARD_MODE_LOST}", out.getvalue())


if __name__ == "__main__":
    unittest.main()
