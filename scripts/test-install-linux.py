#!/usr/bin/env python3
"""Installer recovery tests; --bundle adds real-bundle checks in disposable homes."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("install-linux.py")
SPEC = importlib.util.spec_from_file_location("gitturtle_installer", SCRIPT)
installer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(installer)
BUNDLE = None


class BackupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="gitturtle-install-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / "home with spaces"
        self.data = self.root / "data with spaces"
        self.binary = self.home / ".local/bin/gitturtle"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"previous executable")
        self.binary.chmod(0o755)

    def test_restore_is_complete_and_preserves_unrelated_settings(self):
        settings = self.root / "settings.json"
        settings.write_text("retain this draft")
        metadata = self.data / "gitturtle/build-info.json"
        metadata.parent.mkdir(parents=True)
        metadata.write_text("old identity")
        new_notice = "gitturtle/licenses/new/LICENSE"
        targets = installer.installed_targets(self.home, self.data)
        targets.add(("data", new_notice))
        backup = installer.save_backup(targets, self.home, self.data)
        self.binary.write_bytes(b"new executable")
        metadata.write_text("new identity")
        notice = self.data / new_notice
        notice.parent.mkdir(parents=True)
        notice.write_text("new notice")
        installer.restore_backup(backup, self.home, self.data)
        self.assertEqual(self.binary.read_bytes(), b"previous executable")
        self.assertEqual(self.binary.stat().st_mode & 0o777, 0o755)
        self.assertEqual(metadata.read_text(), "old identity")
        self.assertFalse(notice.exists())
        self.assertEqual(settings.read_text(), "retain this draft")

    def test_corrupted_backup_is_rejected_before_any_restore(self):
        backup = installer.save_backup(installer.installed_targets(self.home, self.data), self.home, self.data)
        document = json.loads((backup / "manifest.json").read_text())
        stored = next(entry["stored"] for entry in document["files"] if entry["stored"])
        (backup / stored).write_bytes(b"changed")
        self.binary.write_bytes(b"keep current executable")
        with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
            installer.restore_backup(backup, self.home, self.data)
        self.assertEqual(self.binary.read_bytes(), b"keep current executable")

    def test_published_recovery_point_can_restore_an_interrupted_upgrade(self):
        backup = installer.save_backup(installer.installed_targets(self.home, self.data), self.home, self.data)
        installer.write_json({"backup": backup.name}, self.data / "gitturtle/previous-installation.json")
        self.binary.write_bytes(b"partially upgraded executable")
        with patch.object(installer, "refresh_desktop"):
            installer.rollback(self.home, self.data)
        self.assertEqual(self.binary.read_bytes(), b"previous executable")

    def test_backup_rejects_unowned_paths_and_symlink_destinations(self):
        with self.assertRaisesRegex(RuntimeError, "outside GitTurtle"):
            installer.owned_target("home", ".ssh/config", self.home, self.data)
        with self.assertRaisesRegex(RuntimeError, "Invalid path"):
            installer.owned_target("data", "gitturtle/licenses/../../other", self.home, self.data)
        self.binary.unlink()
        self.binary.symlink_to(self.root / "other")
        with self.assertRaisesRegex(RuntimeError, "symlink"):
            installer.save_backup(installer.installed_targets(self.home, self.data), self.home, self.data)

    def test_running_installed_executable_is_refused_without_stopping_it(self):
        shutil.copyfile(shutil.which("sleep"), self.binary)
        self.binary.chmod(0o755)
        process = subprocess.Popen([str(self.binary), "30"])
        try:
            with self.assertRaisesRegex(RuntimeError, "still running"):
                installer.refuse_running(self.binary)
            self.assertIsNone(process.poll())
        finally:
            # This is our disposable sleep fixture, never GitTurtle or Git.
            process.terminate()
            process.wait(timeout=5)


class RealBundleTests(unittest.TestCase):
    def setUp(self):
        if BUNDLE is None:
            self.skipTest("Supply --bundle to validate a checksummed native bundle")
        self.temporary = tempfile.TemporaryDirectory(prefix="gitturtle-real-install-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bundle = self.root / "extracted bundle"
        shutil.copytree(BUNDLE, self.bundle)
        self.home, self.data = self.root / "test home", self.root / "test data"
        self.config = self.root / "test config"
        self.config.mkdir()
        self.settings = self.config / "settings.json"
        self.settings.write_bytes(b"settings and commit drafts stay unchanged\n")
        self.environment = dict(os.environ, HOME=str(self.home), XDG_DATA_HOME=str(self.data),
                                XDG_CONFIG_HOME=str(self.config))
        self.binary = self.home / ".local/bin/gitturtle"

    def run_installer(self, script, *args):
        return subprocess.run([sys.executable, str(script), *args], env=self.environment,
                              text=True, capture_output=True, timeout=60)

    def test_install_upgrade_and_rollback_work_after_bundle_removal(self):
        expected = installer.digest_file(self.bundle / "bin/gitturtle")
        initial = self.run_installer(self.bundle / "install.py")
        self.assertEqual(initial.returncode, 0, initial.stderr)
        self.assertEqual(installer.digest_file(self.binary), expected)
        metadata = self.data / "gitturtle/build-info.json"
        self.assertTrue(metadata.is_file())
        self.assertTrue((self.data / "gitturtle/licenses/THIRD_PARTY_NOTICES.md").is_file())
        self.assertIn(str(self.binary), (self.data / f"applications/{installer.APP_ID}.desktop").read_text())
        # Distinguishable prior payload without launching any native application.
        self.binary.write_bytes(b"prior installed executable fixture")
        metadata.write_text('{"fixture": "previous build"}\n')
        upgrade = self.run_installer(self.bundle / "install.py")
        self.assertEqual(upgrade.returncode, 0, upgrade.stderr)
        self.assertEqual(installer.digest_file(self.binary), expected)
        shutil.rmtree(self.bundle)
        recovery = self.data / "gitturtle/install.py"
        rollback = self.run_installer(recovery, "--rollback")
        self.assertEqual(rollback.returncode, 0, rollback.stderr)
        self.assertEqual(self.binary.read_bytes(), b"prior installed executable fixture")
        self.assertEqual(json.loads(metadata.read_text()), {"fixture": "previous build"})
        redo = self.run_installer(recovery, "--rollback")
        self.assertEqual(redo.returncode, 0, redo.stderr)
        self.assertEqual(installer.digest_file(self.binary), expected)
        self.assertEqual(self.settings.read_bytes(), b"settings and commit drafts stay unchanged\n")

    def test_bad_bundle_checksum_leaves_existing_executable_intact(self):
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"preserved installation")
        (self.bundle / "icons/app-icon.png").write_bytes(b"corrupted bundle icon")
        result = self.run_installer(self.bundle / "install.py")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertEqual(self.binary.read_bytes(), b"preserved installation")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, help="Existing complete bundle with current installer and hashes")
    arguments, remaining = parser.parse_known_args()
    BUNDLE = arguments.bundle
    unittest.main(argv=[sys.argv[0], *remaining])
