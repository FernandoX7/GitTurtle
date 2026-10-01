from __future__ import annotations

import contextlib
import io
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from native_qa import mutter, qa

XWAYLAND_PORTAL = ["/usr/bin/Xwayland", ":0", "-rootless", "-noreset", "-accessx", "-core", "-auth",
                   "/run/user/1000/.mutter-Xwaylandauth.ABC", "-listenfd", "4", "-enable-ei-portal",
                   "-listenfd", "5", "-displayfd", "6", "-initfd", "7", "-byteswappedclients"]
XWAYLAND_PLAIN = [arg for arg in XWAYLAND_PORTAL if arg != "-enable-ei-portal"]
OTHERS = [["/usr/bin/gnome-shell"], ["/usr/bin/bash", "-c", "echo -enable-ei-portal"],
          ["/usr/bin/Xvfb", ":1", "-enable-ei-portal"]]
TYPES = SimpleNamespace(UInt32=int, Int32=int, Double=float, Boolean=bool)
KEYSYMS = {"comma": 0x2C, "Control_L": 0xFFE3, "Shift_L": 0xFFE1, "Escape": 0xFF1B, "a": 0x61, "b": 0x62,
           "slash": 0x2F}


class FakeSession:
    """A started org.gnome.Mutter.RemoteDesktop.Session that records every call."""

    def __init__(self) -> None:
        self.calls: list[tuple] = []

    def __getattr__(self, name):
        return lambda *args: self.calls.append((name, *args))


class FakeWindow:
    def __init__(self, wid: int, parent: "FakeWindow | None" = None) -> None:
        self.id, self.parent = wid, parent
        self.pointer = SimpleNamespace(same_screen=True, win_x=60, win_y=400)

    def query_tree(self):
        return SimpleNamespace(parent=self.parent)

    def get_geometry(self):
        return SimpleNamespace(width=1000, height=680)

    def query_pointer(self):
        return self.pointer

    def set_input_focus(self, *args) -> None:
        pass

    def configure(self, **kwargs) -> None:
        pass


class FakeDisplay:
    """X focus is wherever `focus` points; activation does not move it unless `activation_focuses`."""

    def __init__(self) -> None:
        self.root = FakeWindow(1)
        self.app = FakeWindow(100, self.root)
        self.app_child = FakeWindow(101, self.app)
        self.other = FakeWindow(200, self.root)
        self.focus = self.other

    def get_input_focus(self):
        return SimpleNamespace(focus=self.focus)

    def screen(self):
        return SimpleNamespace(root=self.root)

    def sync(self) -> None:
        pass


def keysym(name: str) -> int:
    return KEYSYMS.get(name, 0)


class SelectionTest(unittest.TestCase):
    def test_ei_portal_xwayland_selects_mutter_by_default(self) -> None:
        self.assertEqual(mutter.choose_input(None, [*OTHERS, XWAYLAND_PORTAL]), "mutter")
        self.assertEqual(mutter.choose_input("mutter", [XWAYLAND_PORTAL]), "mutter")

    def test_other_hosts_default_to_xtest(self) -> None:
        for argvs in ([], OTHERS, [XWAYLAND_PLAIN, *OTHERS]):
            with self.subTest(argvs=argvs):
                self.assertFalse(mutter.ei_portal_host(argvs))
                self.assertEqual(mutter.choose_input(None, argvs), "xtest")
        self.assertEqual(mutter.choose_input("mutter", [XWAYLAND_PLAIN]), "mutter")

    def test_xtest_is_refused_on_an_ei_portal_host(self) -> None:
        with self.assertRaises(mutter.InputRefused) as refused:
            mutter.choose_input("xtest", [XWAYLAND_PLAIN, XWAYLAND_PORTAL])
        self.assertIn("-enable-ei-portal", str(refused.exception.code))
        with self.assertRaises(ValueError):
            mutter.choose_input("uinput", [])

    def test_process_list_is_read_from_a_proc_tree(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            proc = Path(scratch)
            for pid, argv in ((10, XWAYLAND_PORTAL), (11, OTHERS[0]), (12, [])):
                (proc / str(pid)).mkdir()
                (proc / str(pid) / "cmdline").write_bytes(b"".join(arg.encode() + b"\0" for arg in argv))
            (proc / "self").mkdir()
            (proc / "13").mkdir()  # an exited process: no cmdline
            argvs = mutter.process_argvs(proc)
            self.assertCountEqual(argvs, [XWAYLAND_PORTAL, OTHERS[0]])
            self.assertTrue(mutter.ei_portal_host(argvs))
            self.assertEqual(mutter.process_argvs(proc / "absent"), [])

    @unittest.skipUnless(shutil.which("git"), "no git")
    def test_launch_refuses_xtest_before_creating_the_run_directory(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch).resolve()
            subprocess.run(["git", "init", "-q", str(root / "fixture")], check=True)
            run_dir = root / "run"
            argv = ["launch", "--binary", sys.executable, "--fixture", str(root / "fixture"), "--run-dir", str(run_dir),
                    "--input", "xtest"]
            with mock.patch.object(mutter, "process_argvs", return_value=[XWAYLAND_PORTAL]), \
                    contextlib.redirect_stderr(io.StringIO()) as err:
                self.assertEqual(qa.main(argv), 2)
            self.assertIn("-enable-ei-portal", err.getvalue())
            self.assertFalse(run_dir.exists())


class GuardTest(unittest.TestCase):
    def setUp(self) -> None:
        patches = [mock.patch.object(mutter, "keysym", keysym), mock.patch("time.sleep")]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        self.session = FakeSession()
        self.dsp = FakeDisplay()
        self.log: list[str] = []
        self.driver = mutter.MutterDriver(self.dsp, self.dsp.app, self.log, mutter.RemoteDesktop(self.session, TYPES))
        self.driver.activate = lambda: None  # a re-activation that does not move X focus

    def sent(self, name: str) -> list[tuple]:
        return [call[1:] for call in self.session.calls if call[0] == name]

    def test_keys_are_refused_unless_x_focus_is_on_the_app(self) -> None:
        for focus in (self.dsp.other, None, 1):  # another window, no focus, PointerRoot
            self.dsp.focus = focus
            with self.subTest(focus=focus), self.assertRaises(mutter.InputRefused):
                self.driver.key("comma", ["Control_L"])
            with self.subTest(focus=focus), self.assertRaises(mutter.InputRefused):
                self.driver.type("ab")
        self.assertEqual(self.session.calls, [])
        self.assertTrue(any("guard x-focus" in line for line in self.log))

    def test_keys_reach_the_app_when_its_window_or_a_child_has_focus(self) -> None:
        self.dsp.focus = self.dsp.app_child
        self.driver.key("comma", ["Control_L"])
        self.assertEqual(self.sent("NotifyKeyboardKeysym"),
                         [(0xFFE3, True), (0x2C, True), (0x2C, False), (0xFFE3, False)])
        self.dsp.focus = self.dsp.app
        self.driver.type("a/")
        self.assertEqual(self.sent("NotifyKeyboardKeysym")[4:], [(0x61, True), (0x61, False), (0x2F, True), (0x2F, False)])

    def test_activation_that_takes_focus_lets_the_key_through(self) -> None:
        self.driver.activate = lambda: setattr(self.dsp, "focus", self.dsp.app)
        self.driver.key("Escape")
        self.assertEqual(self.sent("NotifyKeyboardKeysym"), [(0xFF1B, True), (0xFF1B, False)])

    def test_typing_stops_when_focus_leaves_the_app(self) -> None:
        self.dsp.focus = self.dsp.app
        checks = iter([True, False])  # the guard, then the check before the second character
        self.driver.app_focused = lambda: next(checks)
        with self.assertRaises(mutter.InputRefused):
            self.driver.type("aba")
        self.assertEqual(self.sent("NotifyKeyboardKeysym"), [(0x61, True), (0x61, False)])

    def test_first_wheel_click_of_each_batch_is_compensated(self) -> None:
        self.driver.glide = lambda *args, **kwargs: None
        self.driver.wheel(60, 400, 3)
        self.assertEqual(self.sent("NotifyPointerAxisDiscrete"), [(0, 1)] * 4)
        self.driver.wheel(60, 400, -2)
        self.assertEqual(self.sent("NotifyPointerAxisDiscrete")[4:], [(0, -1)] * 3)
        self.assertIn("4 clicks sent", self.log[0])

    def test_wheel_and_click_are_refused_off_the_window(self) -> None:
        self.driver.glide = lambda *args, **kwargs: None
        self.dsp.app.pointer = SimpleNamespace(same_screen=True, win_x=61, win_y=400)
        with self.assertRaises(mutter.InputRefused):
            self.driver.wheel(60, 400, 3)
        with self.assertRaises(mutter.InputRefused):
            self.driver.click(60, 400)
        self.assertEqual(self.session.calls, [])


if __name__ == "__main__":
    unittest.main()
