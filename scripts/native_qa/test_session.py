from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from native_qa import session, stores

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

            def stable(self, timeout):
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

    def test_refusals_leave_no_run_directory(self) -> None:
        for kwargs in (dict(for_commit=True), dict(extra_env={"XDG_CONFIG_HOME": "/x"})):
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
