"""Tests for scripts/native_qa/macos.py (runs cleanly on both Linux and macOS)."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from native_qa import macos, runenv


class MacosQaTests(unittest.TestCase):
    def test_probe_macos_rejects_non_darwin_and_reports_darwin_readiness(self) -> None:
        linux_report = macos.probe_macos(sys_platform="linux")
        self.assertFalse(linux_report["ready"])
        self.assertTrue(any("darwin" in issue for issue in linux_report["issues"]))

        def ok_run(argv, **_kwargs):
            return subprocess.CompletedProcess(argv, 0, stdout="System Events\n", stderr="")

        ready = macos.probe_macos(
            sys_platform="darwin",
            which=lambda name: f"/usr/bin/{name}",
            run=ok_run,
        )
        self.assertTrue(ready["ready"])
        self.assertTrue(ready["system_events_accessible"])
        self.assertEqual(ready["backend"], macos.BACKEND_LABEL)

        def denied_run(argv, **_kwargs):
            return subprocess.CompletedProcess(argv, 1, stdout="", stderr="Not authorized to send Apple events")

        denied = macos.probe_macos(
            sys_platform="darwin",
            which=lambda name: f"/usr/bin/{name}",
            run=denied_run,
        )
        self.assertFalse(denied["ready"])
        self.assertFalse(denied["system_events_accessible"])
        self.assertTrue(any("Not authorized" in issue for issue in denied["issues"]))

    def test_prepare_and_launch_env_isolate_home_and_reject_operator_library_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            fake_operator = root / "operator"
            (fake_operator / "Library").mkdir(parents=True)

            # Must refuse inside operator ~/Library BEFORE creating any directories or files.
            bad_run = fake_operator / "Library" / "qa-run"
            with self.assertRaises(runenv.Refusal):
                macos.prepare_macos_run_dir(bad_run, b"{}", home=fake_operator)
            self.assertFalse(bad_run.exists())

            run_dir = root / "run"
            prefs = b'{"version":1,"settings":{}}'
            paths = macos.prepare_macos_run_dir(run_dir, prefs, home=fake_operator)
            self.assertEqual(
                paths["preferences"],
                (run_dir / "home" / "Library" / "Application Support" / "GitTurtle" / "preferences.json").resolve(),
            )
            self.assertEqual(paths["preferences"].read_bytes(), prefs)
            self.assertTrue((paths["home"] / ".gitconfig").is_file())

            env = macos.macos_launch_env(
                paths,
                base={
                    "PATH": "/usr/bin",
                    "GIT_DIR": "/evil",
                    "XDG_CONFIG_HOME": "/evil",
                    "ZED_HEADLESS": "1",
                    "HOME": str(fake_operator),
                },
                extra={"GITTURTLE_LOG": "info"},
                home=fake_operator,
            )
            self.assertEqual(env["HOME"], str(paths["home"]))
            self.assertNotIn("GIT_DIR", env)
            self.assertNotIn("XDG_CONFIG_HOME", env)
            self.assertNotIn("ZED_HEADLESS", env)
            self.assertEqual(env["GITTURTLE_LOG"], "info")

            with self.assertRaises(runenv.Refusal):
                macos.macos_launch_env(paths, extra={"HOME": "/tmp/else"}, home=fake_operator)

    def test_validate_capture_name_blocks_traversal_and_absolute_paths(self) -> None:
        self.assertEqual(macos.validate_capture_name("00-launch"), "00-launch")
        for bad in ("", ".", "..", "../escape", "sub/dir", "a\\b", "/Users/op/Desktop/evil", ".hidden", "-flag", "bad\0name"):
            with self.subTest(bad=bad):
                with self.assertRaises(runenv.Refusal):
                    macos.validate_capture_name(bad)

    def test_jxa_script_and_query_windows_are_strictly_pid_scoped(self) -> None:
        script = macos.jxa_window_info_script(4242)
        self.assertIn("unixId: 4242", script)
        self.assertNotIn('byName("GitTurtle")', script)

        class EvilInt(int):
            def __str__(self) -> str:
                return "1}); evil(); ({"

        for bad_pid in (0, -5, True, "42", EvilInt(42)):  # type: ignore[arg-type]
            with self.subTest(bad_pid=bad_pid):
                with self.assertRaises(runenv.Refusal):
                    macos.jxa_window_info_script(bad_pid)

        captured_argv = []

        def fake_run(argv, **_kwargs):
            captured_argv.append(argv)
            payload = json.dumps([{"title": "GitTurtle", "position": [0, 0], "size": [1200, 800], "role": "AXWindow"}])
            return subprocess.CompletedProcess(argv, 0, stdout=payload, stderr="")

        windows = macos.query_windows(4242, run=fake_run)
        self.assertEqual(captured_argv[0][:3], ["/usr/bin/osascript", "-l", "JavaScript"])
        self.assertEqual(len(windows), 1)
        self.assertEqual(windows[0]["title"], "GitTurtle")

    def test_capture_window_argv_and_execution_validate_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp).resolve() / "00-launch.png"
            argv = macos.capture_window_argv(9876, out)
            self.assertEqual(argv, ["/usr/bin/screencapture", "-x", "-o", "-l", "9876", str(out)])

            with self.assertRaises(runenv.Refusal):
                macos.capture_window_argv(0, out)
            with self.assertRaises(runenv.Refusal):
                macos.capture_window_argv(9876, Path("relative.png"))
            with self.assertRaises(runenv.Refusal):
                macos.capture_window_argv(9876, Path(tmp).resolve() / "bad.jpg")

            def fake_capture(cmd, **_kwargs):
                Path(cmd[-1]).write_bytes(b"\x89PNG\r\n\x1a\n")
                return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

            written = macos.capture_window(9876, out, run=fake_capture)
            self.assertEqual(written, out)
            self.assertTrue(out.is_file())
            # Refuses overwriting an existing file.
            with self.assertRaises(runenv.Refusal):
                macos.capture_window_argv(9876, out)


if __name__ == "__main__":
    unittest.main()
