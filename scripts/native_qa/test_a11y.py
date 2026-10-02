from __future__ import annotations

import contextlib
import io
import os
import signal
import sys
import time
import unittest
from types import SimpleNamespace
from unittest import mock

from native_qa import a11y, portal

TYPES = SimpleNamespace(Boolean=lambda value, variant_level=0: bool(value))


class FakeProxy:
    """org.a11y.Bus's object: `IsEnabled` as a value, with every call recorded and optional failures."""

    def __init__(self, enabled: bool = False) -> None:
        self.enabled, self.calls = enabled, []
        self.fail_get = self.fail_set = False
        self.ignore_set = False  # a Set that does not take, as when the property is locked down

    def Get(self, interface, name, dbus_interface=None):
        self.calls.append(("Get", interface, name, dbus_interface))
        if self.fail_get:
            raise RuntimeError("org.a11y.Bus is not running")
        return self.enabled

    def Set(self, interface, name, value, dbus_interface=None):
        self.calls.append(("Set", interface, name, value, dbus_interface))
        if self.fail_set:
            raise RuntimeError("access denied")
        if not self.ignore_set:
            self.enabled = value


def quiet():
    stack = contextlib.ExitStack()
    stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
    stack.enter_context(contextlib.redirect_stderr(io.StringIO()))
    return stack


class EnabledTest(unittest.TestCase):
    def setUp(self) -> None:
        self.proxy = FakeProxy(enabled=False)
        self.status = a11y.Status(self.proxy, TYPES)
        self.addCleanup(signal.signal, signal.SIGTERM, signal.getsignal(signal.SIGTERM))

    def test_set_for_the_block_then_restored_and_read_back(self) -> None:
        record = {}
        with quiet(), a11y.enabled(record, self.status):
            self.assertTrue(self.proxy.enabled)
            self.assertIsNot(signal.getsignal(signal.SIGTERM), signal.SIG_DFL)  # SIGTERM can no longer skip it
        self.assertFalse(self.proxy.enabled)
        self.assertEqual(record, dict(original=False, set=True, restored=False))
        self.assertEqual([call[0] for call in self.proxy.calls], ["Get", "Set", "Get", "Set", "Get"])
        self.assertEqual(self.proxy.calls[1], ("Set", a11y.STATUS, "IsEnabled", True, a11y.PROPERTIES))
        self.assertEqual(self.proxy.calls[0][1:], (a11y.STATUS, "IsEnabled", a11y.PROPERTIES))

    def test_an_exception_in_the_block_still_restores(self) -> None:
        record = {}
        with quiet(), self.assertRaisesRegex(RuntimeError, "the app went away"):
            with a11y.enabled(record, self.status):
                raise RuntimeError("the app went away")
        self.assertFalse(self.proxy.enabled)
        self.assertEqual((record["original"], record["set"], record["restored"]), (False, True, False))
        self.assertNotIn("restore_error", record)

    def test_an_original_true_stays_true(self) -> None:
        self.proxy.enabled = True
        record = {}
        with quiet(), a11y.enabled(record, self.status):
            pass
        self.assertEqual(record, dict(original=True, set=True, restored=True))

    def test_sigterm_in_the_block_only_marks_it_for_the_caller(self) -> None:
        previous = signal.getsignal(signal.SIGTERM)
        record = {}
        with quiet(), a11y.enabled(record, self.status):
            os.kill(os.getpid(), signal.SIGTERM)
            time.sleep(0.01)  # the handler has run by now, and raised nothing
            self.assertEqual((record.get("sigterm"), self.proxy.enabled), (True, True))
        self.assertEqual((record["restored"], self.proxy.enabled), (False, False))
        self.assertEqual(signal.getsignal(signal.SIGTERM), previous)

    def test_sigterm_after_the_block_before_the_restore_cannot_skip_it(self) -> None:
        # The signal lands after the block's last statement, before the generator resumes for the restore: the
        # window in which a handler that raised SystemExit used to skip the restore and leave IsEnabled true.
        real_exit = contextlib._GeneratorContextManager.__exit__

        def exit_after_a_signal(manager, *args):
            signal.getsignal(signal.SIGTERM)(signal.SIGTERM, None)
            return real_exit(manager, *args)

        record = {}
        with quiet(), mock.patch.object(contextlib._GeneratorContextManager, "__exit__", exit_after_a_signal):
            with a11y.enabled(record, self.status):
                self.assertTrue(self.proxy.enabled)
        self.assertEqual((record["sigterm"], record["restored"], self.proxy.enabled), (True, False, False))
        self.assertNotIn("restore_error", record)

    def test_sigterm_inside_the_set_or_the_restore_interrupts_neither(self) -> None:
        received = []
        signal.signal(signal.SIGTERM, lambda signum, frame: received.append(signum))
        real = self.proxy.Set

        def terminated(*args, **kwargs):
            os.kill(os.getpid(), signal.SIGTERM)  # handled at the next bytecode, inside the set or the restore
            real(*args, **kwargs)

        self.proxy.Set = terminated
        record = {}
        with quiet(), a11y.enabled(record, self.status):
            entered = True
        self.assertTrue(entered)
        self.assertEqual((record["set"], record["restored"], record["sigterm"]), (True, False, True))
        self.assertFalse(self.proxy.enabled)
        self.assertEqual(received, [])  # the tool's own handler took both, not the one installed before
        os.kill(os.getpid(), signal.SIGTERM)
        time.sleep(0.01)
        self.assertEqual(received, [signal.SIGTERM])  # and the one before is back afterwards

    def test_unreadable_status_refuses_with_nothing_changed(self) -> None:
        self.proxy.fail_get = True
        with quiet(), self.assertRaisesRegex(SystemExit, "IsEnabled cannot be read"):
            with a11y.enabled({}, self.status):
                self.fail("never entered")
        self.assertEqual([call[0] for call in self.proxy.calls], ["Get"])

    def test_a_set_that_does_not_take_refuses_the_block(self) -> None:
        self.proxy.ignore_set = True
        record = {}
        with quiet(), self.assertRaisesRegex(SystemExit, "read back False after setting it true"):
            with a11y.enabled(record, self.status):
                self.fail("never entered")
        self.assertEqual((record["original"], record["set"], record["restored"]), (False, False, False))

    def test_a_failed_restore_is_reported(self) -> None:
        record = {}
        with quiet(), a11y.enabled(record, self.status):
            self.proxy.fail_set = True  # the restore's Set raises
        self.assertEqual(record["restored"], True)  # read back as it really is
        self.assertTrue(record["restore_error"].startswith("org.a11y.Status IsEnabled could not write it back (access denied); restore it by hand: gdbus call"))
        self.assertTrue(record["restore_error"].endswith("IsEnabled '<false>'"))
        self.proxy.fail_set, self.proxy.enabled = False, False
        record = {}
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as errors, \
                a11y.enabled(record, self.status):
            self.proxy.ignore_set = True  # the restore's Set does not take
        self.assertTrue(record["restore_error"].startswith("org.a11y.Status IsEnabled read back True after restoring False; restore it by hand"))
        self.assertIn("error: org.a11y.Status IsEnabled read back True", errors.getvalue())


class RequireModulesTest(unittest.TestCase):
    def test_missing_modules_refuse_and_are_named(self) -> None:
        with mock.patch.object(portal, "_atspi", side_effect=ImportError("No module named 'gi'")), \
                mock.patch.dict(sys.modules, {"dbus": None}), \
                self.assertRaisesRegex(SystemExit, r"refusing: \"atspi\": true needs gi with Atspi 2\.0 \(PyGObject; "
                                                   r"No module named 'gi'\) and dbus \(dbus-python; .*nothing launched"):
            a11y.require_modules()
        with mock.patch.object(portal, "_atspi", side_effect=ValueError("Namespace Atspi not available")), \
                mock.patch.dict(sys.modules, {"dbus": SimpleNamespace()}), \
                self.assertRaisesRegex(SystemExit, r"needs gi with Atspi 2\.0 \(PyGObject; Namespace Atspi not "
                                                   r"available\); nothing launched"):
            a11y.require_modules()
        with mock.patch.object(portal, "_atspi", lambda: SimpleNamespace()), \
                mock.patch.dict(sys.modules, {"dbus": SimpleNamespace()}):
            a11y.require_modules()  # both present: nothing refused


class Node:
    """An AT-SPI accessible with a process id, as the registry's applications report one."""

    def __init__(self, name: str = "", role: str = "panel", states=(), children=(), pid: int | None = None) -> None:
        self.name, self.role, self.states, self.children, self.pid = name, role, set(states), list(children), pid
        self.cleared = 0

    def get_name(self) -> str:
        return self.name

    def get_role_name(self) -> str:
        return self.role

    def get_child_count(self) -> int:
        return len(self.children)

    def get_child_at_index(self, index: int):
        return self.children[index]

    def get_state_set(self):
        return SimpleNamespace(contains=lambda state: state in self.states)

    def get_process_id(self) -> int:
        if self.pid is None:
            raise RuntimeError("no connection")
        return self.pid

    def clear_cache(self) -> None:
        self.cleared += 1


class FocusTest(unittest.TestCase):
    def setUp(self) -> None:
        self.switch = Node("Follow system appearance", "toggle button",
                           states=("focused", "focusable", "enabled", "sensitive", "showing", "visible"))
        self.card = Node("Omarchy theme", "button", states=("focusable", "enabled"))
        settings = Node("Settings", "panel", children=[Node(children=[self.card, Node(children=[self.switch])])])
        self.frame = Node("GitTurtle", "frame", states=("focused", "active"), children=[settings])
        self.app = Node("GitTurtle", "application", children=[self.frame], pid=4242)
        # Another GitTurtle (the operator's own) and an application that cannot say its pid.
        self.other = Node("GitTurtle", "application", pid=99,
                          children=[Node("GitTurtle", "frame", children=[Node("x", "button", states=("focused",))])])
        self.desktop = Node(children=[Node("broken"), self.other, self.app])
        fake = SimpleNamespace(get_desktop=lambda index: self.desktop, set_timeout=lambda *args: None,
                               init=lambda: None, StateType=SimpleNamespace(**{s.upper(): s for s in a11y.STATES}))
        for patch in (mock.patch.object(portal, "_atspi", lambda: fake), mock.patch("time.sleep")):
            patch.start()
            self.addCleanup(patch.stop)

    def test_the_application_is_found_by_pid_and_its_deepest_focused_node_reported(self) -> None:
        self.assertIs(a11y.application_for(4242), self.app)
        self.assertIs(a11y.application_for(99), self.other)
        self.assertIsNone(a11y.application_for(7))
        reading = a11y.read_focus(4242, within=1)
        self.assertEqual(reading["focused"], dict(name="Follow system appearance", role="toggle button",
                                                  states=["focused", "focusable", "enabled", "sensitive", "showing",
                                                          "visible"], depth=5))
        self.assertEqual([(n["name"], n["depth"]) for n in reading["all_focused"]],
                         [("GitTurtle", 1), ("Follow system appearance", 5)])
        self.assertEqual((reading["pid"], reading["application"], reading["nodes"], reading["truncated"]),
                         (4242, "GitTurtle", 7, False))
        self.assertNotIn("error", reading)
        self.assertEqual(self.app.cleared, 1)

    def test_a_switch_that_is_on_reports_pressed(self) -> None:
        self.switch.states |= {"pressed"}
        self.switch.states -= {"enabled", "sensitive"}
        reading = a11y.read_focus(4242, within=1)
        self.assertEqual(reading["focused"]["states"], ["focused", "focusable", "pressed", "showing", "visible"])

    def test_only_the_window_focused_and_nothing_focused(self) -> None:
        self.switch.states.discard("focused")
        self.assertEqual(a11y.read_focus(4242, within=1)["focused"]["name"], "GitTurtle")
        self.frame.states.discard("focused")
        clock = iter(range(100))
        reading = a11y.read_focus(4242, within=3, clock=lambda: next(clock), sleep=lambda seconds: None)
        self.assertIsNone(reading["focused"])
        self.assertEqual(reading["all_focused"], [])
        self.assertNotIn("error", reading)  # the application is there; it reports no focus

    def test_an_application_that_never_registers(self) -> None:
        clock = iter(range(100))
        reading = a11y.read_focus(31337, within=3, clock=lambda: next(clock), sleep=lambda seconds: None)
        self.assertIsNone(reading["application"])
        self.assertIn("no AT-SPI application with pid 31337 within 3 s", reading["error"])

    def test_the_application_is_waited_for(self) -> None:
        self.desktop.children.remove(self.app)
        polls = []

        def sleep(seconds):
            polls.append(seconds)
            if len(polls) == 2:
                self.desktop.children.append(self.app)  # it registers after two polls

        reading = a11y.read_focus(4242, within=60, poll=0.25, sleep=sleep)
        self.assertEqual(polls, [0.25, 0.25])
        self.assertEqual(reading["focused"]["name"], "Follow system appearance")

    def test_the_search_is_bounded(self) -> None:
        with mock.patch.object(a11y, "NODES", 3):
            found, count, truncated = a11y.focused_nodes(self.app)
        self.assertEqual((count, truncated), (3, True))
        self.assertEqual([n["name"] for n in found], ["GitTurtle"])


if __name__ == "__main__":
    unittest.main()
