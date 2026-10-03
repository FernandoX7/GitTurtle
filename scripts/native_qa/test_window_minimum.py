"""Lowering the window's WM_NORMAL_HINTS minimum: only the minimum changes, it is read back, and a launch records
the hints before and after it, or refuses with nothing resized; a launch without it never touches the hints."""

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

from native_qa import evidence, mutter, play, qa, scenario, session, stores, x11
from native_qa.test_scenario import spec
from native_qa.test_session import INFO

HAVE_PIL = importlib.util.find_spec("PIL") is not None
FLAGS = {name: 1 << bit for bit, name in enumerate(x11.SIZE_HINT_FLAGS)}
# What GPUI's X11 window sends for GitTurtle (`window_min_size` 1000x680, the GPU's texture limit as its maximum),
# with a gravity and the obsolete position and size words set, so the test sees every other field kept.
GPUI_HINTS = [FLAGS["PMinSize"] | FLAGS["PMaxSize"] | FLAGS["PWinGravity"], 7, 8, 9, 10, 1000, 680, 16384, 16384,
              0, 0, 0, 0, 0, 0, 0, 0, 10]


class HintsWindow:
    """The app's X window as far as its WM_NORMAL_HINTS go. `appears` reads find no property before it is set,
    as when GPUI has set _NET_WM_PID but not yet its size hints; `keeps` makes a write not take, as if the client
    wrote its own hints again; `changes` is every ChangeProperty."""

    def __init__(self, values=GPUI_HINTS, appears: int = 0, keeps: bool = False, property_type=x11.WM_SIZE_HINTS,
                 fmt: int = 32) -> None:
        self.values = None if values is None else list(values)
        self.appears, self.keeps = appears, keeps
        self.property_type, self.format = property_type, fmt
        self.reads = 0
        self.changes: list[tuple] = []

    def get_full_property(self, prop, prop_type):
        assert (prop, prop_type) == (x11.WM_NORMAL_HINTS, x11.WM_SIZE_HINTS)
        self.reads += 1
        if self.values is None or self.reads <= self.appears:
            return None
        return SimpleNamespace(property_type=self.property_type, format=self.format, value=list(self.values))

    def change_property(self, prop, prop_type, fmt, data) -> None:
        self.changes.append((prop, prop_type, fmt, list(data)))
        if not self.keeps:
            self.values = list(data)


class HintsDisplay:
    def __init__(self) -> None:
        self.syncs = 0

    def sync(self) -> None:
        self.syncs += 1


def driver(window: HintsWindow, backend: str = "xtest", log: list[str] | None = None) -> x11.Driver:
    log = [] if log is None else log
    if backend == "mutter":
        return mutter.MutterDriver(HintsDisplay(), window, log, remote=None)
    return x11.Driver(HintsDisplay(), window, log)


class HintsTest(unittest.TestCase):
    def setUp(self) -> None:
        sleep = mock.patch("time.sleep")
        self.sleep = sleep.start()
        self.addCleanup(sleep.stop)

    def test_the_hints_read_by_name(self) -> None:
        read = x11.size_hints(GPUI_HINTS)
        self.assertEqual((read["min_size"], read["max_size"], read["flags"]),
                         ([1000, 680], [16384, 16384], ["PMinSize", "PMaxSize", "PWinGravity"]))
        self.assertEqual((read["fields"]["win_gravity"], read["fields"]["x"], read["values"]), (10, 7, GPUI_HINTS))
        # A pre-ICCCM client's 15 fields, with no minimum flag: nothing invented.
        old = x11.size_hints([FLAGS["PMaxSize"], 0, 0, 0, 0, 300, 200, 900, 900, 1, 1, 0, 0, 0, 0])
        self.assertEqual((old["min_size"], old["max_size"], "win_gravity" in old["fields"]), (None, [900, 900], False))

    def test_only_the_minimum_changes(self) -> None:
        lowered = x11.lowered_hints(GPUI_HINTS, 400, 420)
        self.assertEqual([i for i, (a, b) in enumerate(zip(GPUI_HINTS, lowered)) if a != b], list(x11.MIN_FIELDS))
        self.assertEqual(x11.size_hints(lowered)["min_size"], [400, 420])
        # The flag is raised where only the maximum was set.
        self.assertEqual(x11.lowered_hints([FLAGS["PMaxSize"], *GPUI_HINTS[1:]], 400, 420)[0],
                         FLAGS["PMaxSize"] | FLAGS["PMinSize"])

    def test_both_drivers_lower_the_minimum_and_read_it_back(self) -> None:
        for backend in ("xtest", "mutter"):
            with self.subTest(backend=backend):
                window, log = HintsWindow(), []
                record = driver(window, backend, log).lower_minimum(400, 420)
                applied = x11.lowered_hints(GPUI_HINTS, 400, 420)
                self.assertEqual(window.changes, [(x11.WM_NORMAL_HINTS, x11.WM_SIZE_HINTS, 32, applied)])
                self.assertEqual(window.values, applied)
                self.assertEqual(record["requested"], [400, 420])
                self.assertEqual((record["original"]["min_size"], record["applied"]["min_size"]),
                                 ([1000, 680], [400, 420]))
                self.assertEqual((record["original"]["max_size"], record["applied"]["max_size"]),
                                 ([16384, 16384], [16384, 16384]))
                self.assertEqual(record["applied"]["flags"], record["original"]["flags"])
                self.assertEqual(log, ["window minimum 1000x680 -> 400x420 (WM_NORMAL_HINTS, read back)"])

    def test_hints_set_after_the_window_is_found_are_waited_for(self) -> None:
        window = HintsWindow(appears=3)
        record = driver(window).lower_minimum(400, 420)
        self.assertEqual((record["original"]["min_size"], window.reads, len(window.changes)), ([1000, 680], 5, 1))
        self.assertEqual(self.sleep.call_count, 3)

    def test_refusals_change_nothing_or_say_the_minimum_did_not_take(self) -> None:
        no_minimum = [FLAGS["PMaxSize"], *GPUI_HINTS[1:]]
        cases = [(HintsWindow(values=None), "the window has no WM_NORMAL_HINTS after 1.0 s; nothing changed"),
                 (HintsWindow(values=no_minimum), r"sets no minimum size \(PMinSize\) after 1.0 s; nothing changed"),
                 (HintsWindow(fmt=8), "not WM_SIZE_HINTS of 32 bits"),
                 (HintsWindow(property_type=31), "of type 31"),
                 (HintsWindow(values=GPUI_HINTS[:6]), "6 fields, too few to hold a minimum size")]
        for window, match in cases:
            with self.subTest(match=match), self.assertRaisesRegex(SystemExit, f"refusing: cannot lower the window "
                                                                               f"minimum to 400x420: .*{match}"):
                driver(window).lower_minimum(400, 420, timeout=1.0)
            self.assertEqual(window.changes, [])
        # The record a caller logs keeps what was read and why it refused, whatever the refusal.
        record: dict = {}
        with self.assertRaises(SystemExit):
            driver(HintsWindow(fmt=8)).lower_minimum(400, 420, timeout=0, record=record)
        self.assertEqual(record["original"], dict(property_type=x11.WM_SIZE_HINTS, format=8, values=GPUI_HINTS))
        self.assertIn("not WM_SIZE_HINTS of 32 bits", record["refused"])
        kept, record = HintsWindow(keeps=True), {}
        with self.assertRaisesRegex(SystemExit, r"refusing: the window's WM_NORMAL_HINTS read back \[560, 7, 8, 9, "
                                                r"10, 1000, 680.*the window minimum did not take"):
            driver(kept).lower_minimum(400, 420, record=record)
        self.assertEqual(len(kept.changes), 1)
        self.assertEqual((record["requested"], record["original"]["min_size"], record["applied"]["min_size"],
                          record["read_back"]["min_size"]), ([400, 420], [1000, 680], [400, 420], [1000, 680]))
        self.assertIn("did not take", record["refused"])

    def test_a_minimum_that_lowers_nothing_refuses_with_nothing_written(self) -> None:
        for width, height in ((1000, 680), (1200, 420), (400, 700)):
            window, record = HintsWindow(), {}
            with self.subTest(size=(width, height)), \
                    self.assertRaisesRegex(SystemExit, rf"refusing: a window minimum of {width}x{height} is not lower "
                                                       r"than the app's 1000x680 \(larger in either dimension, or "
                                                       r"equal in both\); nothing changed"):
                driver(window).lower_minimum(width, height, record=record)
            self.assertEqual((window.changes, record["original"]["min_size"], "applied" in record),
                             ([], [1000, 680], False))
        # Lowering one dimension and keeping the other is a lower minimum.
        self.assertEqual(driver(HintsWindow()).lower_minimum(400, 680)["applied"]["min_size"], [400, 680])

    def test_the_attestation_line_counts_only_minimums_that_took(self) -> None:
        def launch(was, now, refused=None):
            record = dict(original=dict(min_size=was), applied=dict(min_size=now))
            return dict(window_minimum=dict(record, refused=refused) if refused else record)

        took = launch([1000, 680], [400, 420])
        self.assertEqual(evidence.minimum_line([took, took]), "window WM_NORMAL_HINTS minimum lowered from 1000x680 "
                                                              "to 400x420 in every launch, read back before the "
                                                              "first resize")
        self.assertEqual(evidence.minimum_line([took, launch([1000, 680], [450, 440]), {}]),
                         "window WM_NORMAL_HINTS minimum lowered from 1000x680 to 400x420 in 1 of 3 launches; from "
                         "1000x680 to 450x440 in 1 of 3 launches, read back before the first resize")
        self.assertIsNone(evidence.minimum_line([launch([1000, 680], [400, 420], refused="did not take"), {}]))


class RecordingDriver:
    """What a session asks of its driver while sizing the window, in order."""

    def __init__(self) -> None:
        self.calls: list[tuple] = []

    def lower_minimum(self, width, height, record=None) -> dict:
        self.calls.append(("lower_minimum", width, height))
        record = {} if record is None else record
        record.update(requested=[width, height], original=x11.size_hints(GPUI_HINTS),
                      applied=x11.size_hints(x11.lowered_hints(GPUI_HINTS, width, height)))
        return record

    def resize(self, width, height) -> None:
        self.calls.append(("resize", width, height))


@unittest.skipUnless(shutil.which("git"), "no git")
class SessionTest(unittest.TestCase):
    def setUp(self) -> None:
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name).resolve()
        self.binary = self.root / "gitturtle"
        self.binary.write_text(f"#!/bin/sh\nprintf '%s\\n' '{json.dumps(INFO)}'\n")
        self.binary.chmod(0o755)
        subprocess.run(["git", "init", "-q", str(self.root / "fixture")], check=True)

    def session(self, name: str, **kwargs) -> session.Session:
        with contextlib.redirect_stderr(io.StringIO()):
            return session.Session(self.binary, self.root / "fixture", self.root / name, stores.store_text(),
                                   **kwargs)

    def test_the_minimum_is_lowered_before_the_first_resize_and_logged(self) -> None:
        run = self.session("run", width=461, height=490, window_minimum=(400, 420))
        run.driver = RecordingDriver()
        with contextlib.redirect_stdout(io.StringIO()) as out:
            run.size_window()
            run.close()
        self.assertEqual(run.driver.calls, [("lower_minimum", 400, 420), ("resize", 461, 490)])
        self.assertIn("window minimum lowered from 1000x680 to 400x420", out.getvalue())
        log = json.loads((self.root / "run" / "flow-log.json").read_text())
        self.assertEqual((log["header"]["window_minimum"], log["header"]["size"]), ([400, 420], [461, 490]))
        self.assertEqual((log["window_minimum"]["original"]["min_size"], log["window_minimum"]["applied"]["min_size"]),
                         ([1000, 680], [400, 420]))

    def test_without_it_the_hints_are_never_touched_or_logged(self) -> None:
        run = self.session("plain")
        run.driver = RecordingDriver()
        with contextlib.redirect_stdout(io.StringIO()):
            run.size_window()
            run.close()
        self.assertEqual(run.driver.calls, [("resize", 1000, 680)])
        log = json.loads((self.root / "plain" / "flow-log.json").read_text())
        self.assertNotIn("window_minimum", log)
        self.assertNotIn("window_minimum", log["header"])

    def test_a_minimum_that_does_not_take_refuses_before_any_resize_and_logs_what_it_read(self) -> None:
        run = self.session("refused", width=461, height=490, window_minimum=(400, 420))
        window = HintsWindow(keeps=True)
        run.driver = driver(window, log=run.log["input"])  # the real driver; resizing would fail on this fake
        with contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(SystemExit, "did not take"):
                run.size_window()
            run.close()
        log = json.loads((self.root / "refused" / "flow-log.json").read_text())
        minimum = log["window_minimum"]
        self.assertEqual((minimum["original"]["values"], minimum["read_back"]["min_size"]), (GPUI_HINTS, [1000, 680]))
        self.assertIn("the window minimum did not take", minimum["refused"])

    def test_the_launch_command_refuses_a_size_below_its_minimum(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()) as err:
            code = qa.main(["launch", "--binary", str(self.binary), "--fixture", str(self.root / "fixture"),
                            "--run-dir", str(self.root / "cli"), "--size", "461x490", "--window-minimum", "500x400"])
        self.assertEqual(code, 2)
        self.assertIn("--size 461x490 is below --window-minimum 500x400", err.getvalue())
        self.assertFalse((self.root / "cli").exists())


@unittest.skipUnless(shutil.which("git"), "no git")
class LaunchRecordTest(unittest.TestCase):
    def launch(self, loaded: dict, variant: scenario.Variant) -> tuple[dict, dict]:
        """`play.launch` through a session that lowers the minimum it was given, as `Session.size_window` does;
        the launch record and the session's keyword arguments."""
        seen = {}

        class FakeSession:
            def __init__(self, binary, fixture, run_dir, store, **kwargs) -> None:
                seen.update(kwargs)
                self.log = dict(header=dict(started_utc="2026-10-03T10:00:00Z", ended_utc="2026-10-03T10:01:00Z"),
                                captures=[], fixture_unchanged=True)
                self.restore_failures = []
                self.minimum = kwargs.get("window_minimum")

            def launch(self) -> None:
                if self.minimum is not None:
                    self.log["window_minimum"] = RecordingDriver().lower_minimum(*self.minimum)

            def run(self, steps) -> None:
                pass

            def close(self) -> int:
                self.log["exit"] = -15
                return -15

        with tempfile.TemporaryDirectory() as scratch:
            fixture = Path(scratch) / "fixture"
            subprocess.run(["git", "init", "-q", str(fixture)], check=True)
            with mock.patch("native_qa.session.Session", FakeSession), contextlib.redirect_stdout(io.StringIO()):
                record = play.launch(loaded, "cand", variant, Path("/x/cand"), fixture, Path(scratch) / "run",
                                     dict(display=":1", settle=0, backend="mutter"))
        return record, seen

    def test_each_launch_opens_its_variants_window_and_records_the_hints(self) -> None:
        loaded = scenario.validate(spec(window=[461, 490], window_minimum=[400, 420], variants=[
            {"palette": "midnight"},
            {"palette": "porcelain", "window": [560, 600], "window_minimum": [500, 520]}],
            steps=[{"wait": 0}], analyses=[]), "f" * 64)
        midnight, porcelain = loaded["variants"]
        record, seen = self.launch(loaded, midnight)
        self.assertEqual((seen["width"], seen["height"], seen["window_minimum"]), (461, 490, (400, 420)))
        self.assertEqual(record["window_minimum"]["applied"]["min_size"], [400, 420])
        record, seen = self.launch(loaded, porcelain)
        self.assertEqual((seen["width"], seen["height"], seen["window_minimum"]), (560, 600, (500, 520)))
        self.assertEqual(record["window_minimum"]["original"]["min_size"], [1000, 680])
        plain, seen = self.launch(scenario.validate(spec(steps=[{"wait": 0}], analyses=[]), "f" * 64), midnight)
        self.assertEqual((seen["width"], seen["height"], seen["window_minimum"]), (1000, 680, None))
        self.assertNotIn("window_minimum", plain)


if __name__ == "__main__":
    unittest.main()
