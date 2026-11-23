"""Tests for scripts/native_qa/headless.py (cross-platform, stdlib + mocks only)."""

from __future__ import annotations

import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from native_qa import headless, runenv


class HeadlessTests(unittest.TestCase):
    def test_find_lavapipe_icd_prefers_host_architecture(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            icd_dir = Path(tmp)
            (icd_dir / "lvp_icd.x86_64.json").write_text("{}", encoding="utf-8")
            (icd_dir / "lvp_icd.aarch64.json").write_text("{}", encoding="utf-8")
            found_arm = headless.find_lavapipe_icd([icd_dir], machine="aarch64")
            found_x64 = headless.find_lavapipe_icd([icd_dir], machine="x86_64")
            self.assertEqual(found_arm, (icd_dir / "lvp_icd.aarch64.json").resolve())
            self.assertEqual(found_x64, (icd_dir / "lvp_icd.x86_64.json").resolve())

    def test_find_lavapipe_icd_skips_symlinks_and_symlinked_dirs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            real = root / "real.json"
            real.write_text("{}", encoding="utf-8")
            link = root / "lvp_icd.x86_64.json"
            link.symlink_to(real)
            self.assertIsNone(headless.find_lavapipe_icd([root], machine="x86_64"))

            real_dir = root / "real_dir"
            real_dir.mkdir()
            (real_dir / "lvp_icd.x86_64.json").write_text("{}", encoding="utf-8")
            link_dir = root / "link_dir"
            link_dir.symlink_to(real_dir)
            self.assertIsNone(headless.find_lavapipe_icd([link_dir], machine="x86_64"))

    def test_probe_headless_reports_missing_prerequisites_cleanly(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report = headless.probe_headless(
                icd_dirs=[Path(tmp)],
                which=lambda _name: None,
                machine="x86_64",
                sys_platform="linux",
            )
            self.assertFalse(report["ready"])
            self.assertEqual(report["backend"], headless.BACKEND_LABEL)
            self.assertEqual(len(report["issues"]), 2)

            (Path(tmp) / "lvp_icd.x86_64.json").write_text("{}", encoding="utf-8")
            ready = headless.probe_headless(
                icd_dirs=[Path(tmp)],
                which=lambda name: "/usr/bin/Xvfb" if name == "Xvfb" else None,
                machine="x86_64",
                sys_platform="linux",
            )
            self.assertTrue(ready["ready"])
            self.assertEqual(ready["issues"], [])

            mac = headless.probe_headless(
                icd_dirs=[Path(tmp)],
                which=lambda _name: "/usr/bin/Xvfb",
                machine="arm64",
                sys_platform="darwin",
            )
            self.assertFalse(mac["ready"])
            self.assertTrue(any("Linux-only" in issue for issue in mac["issues"]))

    def test_headless_env_sets_lavapipe_and_rejects_symlinks_zed_headless_and_reserved(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            icd = Path(tmp) / "lvp_icd.aarch64.json"
            icd.write_text("{}", encoding="utf-8")
            env = headless.headless_env(icd, {"GITTURTLE_LOG": "info"})
            self.assertEqual(env["VK_ICD_FILENAMES"], str(icd.resolve()))
            self.assertEqual(env["VK_DRIVER_FILES"], str(icd.resolve()))
            self.assertEqual(env["MESA_LOADER_DRIVER_OVERRIDE"], "llvmpipe")
            self.assertEqual(env["LIBGL_ALWAYS_SOFTWARE"], "1")
            self.assertNotIn("ZED_HEADLESS", env)

            icd_link = Path(tmp) / "link.json"
            icd_link.symlink_to(icd)
            with self.assertRaises(runenv.Refusal):
                headless.headless_env(icd_link)

            with self.assertRaises(runenv.Refusal):
                headless.headless_env(icd, {"ZED_HEADLESS": "1"})
            with self.assertRaises(runenv.Refusal):
                headless.headless_env(icd, {"DISPLAY": ":0"})

            # Inherited ZED_HEADLESS is stripped by runenv.launch_env.
            run_dir = Path(tmp) / "run"
            dirs = runenv.prepare(run_dir, b"{}", home=Path(tmp) / "op")
            launched = runenv.launch_env(
                dirs,
                base={"PATH": "/usr/bin", "ZED_HEADLESS": "1"},
                home=Path(tmp) / "op",
            )
            self.assertNotIn("ZED_HEADLESS", launched)

    def test_parse_displayfd_output(self) -> None:
        self.assertEqual(headless.parse_displayfd_output(b"42\n"), ":42")
        self.assertEqual(headless.parse_displayfd_output(b"0\n"), ":0")
        for bad in (b"", b":1\n", b"-1\n", b"abc\n", b"100000\n", b"\xff\n"):
            with self.subTest(bad=bad):
                with self.assertRaises(runenv.Refusal):
                    headless.parse_displayfd_output(bad)

    def test_xvfb_session_validates_geometry_reads_displayfd_and_terminates_cleanly(self) -> None:
        with self.assertRaises(runenv.Refusal):
            headless.XvfbSession(width=1280.5, height=800, xvfb_bin="/usr/bin/Xvfb")  # type: ignore[arg-type]

        class FakeProc:
            def __init__(self, argv, **_kwargs):
                self.argv = argv
                self._terminated = False
                fd_idx = argv.index("-displayfd") + 1
                os.write(int(argv[fd_idx]), b"77\n")

            def poll(self):
                return 0 if self._terminated else None

            def send_signal(self, sig):
                self._terminated = True

            def wait(self, timeout=None):
                self._terminated = True
                return 0

        created = []

        def fake_popen(argv, **kwargs):
            proc = FakeProc(argv, **kwargs)
            created.append(proc)
            return proc

        with mock.patch("subprocess.Popen", side_effect=fake_popen):
            with headless.XvfbSession(width=1280, height=800, xvfb_bin="/usr/bin/Xvfb") as dsp:
                self.assertEqual(dsp, ":77")
                self.assertIn("-nolisten", created[0].argv)
                self.assertIn("tcp", created[0].argv)
        self.assertTrue(created[0]._terminated)


if __name__ == "__main__":
    unittest.main()
