#!/usr/bin/env python3
"""Installer recovery tests; --bundle adds real-bundle checks in disposable homes."""

import argparse
import contextlib
import copy
import fcntl
import io
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
PACKAGE_SPEC = importlib.util.spec_from_file_location("package_identity_fixture", SCRIPT.with_name("package-identity.py"))
package_identity = importlib.util.module_from_spec(PACKAGE_SPEC)
PACKAGE_SPEC.loader.exec_module(package_identity)
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

    def test_restore_rejects_symlink_parent_before_changing_any_file(self):
        notice = self.data / "gitturtle/licenses/dependency/LICENSE"
        notice.parent.mkdir(parents=True)
        notice.write_text("old license")
        backup = installer.save_backup(installer.installed_targets(self.home, self.data), self.home, self.data)
        external = self.root / "unrelated files"
        external.mkdir()
        victim = external / "LICENSE"
        victim.write_text("unrelated file")
        shutil.rmtree(notice.parent)
        notice.parent.symlink_to(external, target_is_directory=True)
        self.binary.write_bytes(b"keep current executable")

        with self.assertRaisesRegex(RuntimeError, "symlink"):
            installer.restore_backup(backup, self.home, self.data)

        self.assertEqual(victim.read_text(), "unrelated file")
        self.assertEqual(self.binary.read_bytes(), b"keep current executable")

    def test_user_selected_data_root_can_be_a_symlink(self):
        actual_data = self.root / "selected data storage"
        actual_data.mkdir()
        self.data.symlink_to(actual_data, target_is_directory=True)
        backup = installer.save_backup(installer.installed_targets(self.home, self.data), self.home, self.data)
        self.binary.write_bytes(b"new executable")
        installer.restore_backup(backup, self.home, self.data)
        self.assertEqual(self.binary.read_bytes(), b"previous executable")

    def test_data_path_accepts_absolute_user_selection_and_rejects_relative_path(self):
        with patch.dict(os.environ, {"XDG_DATA_HOME": str(self.data)}):
            self.assertEqual(installer.data_path(), self.data)
        with patch.dict(os.environ, {"XDG_DATA_HOME": "relative/data"}):
            with self.assertRaisesRegex(RuntimeError, "absolute path"):
                installer.data_path()

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

    def test_support_and_backup_symlinks_are_refused(self):
        external = self.root / "unrelated"
        external.mkdir()
        marker = external / "keep"
        marker.write_text("unchanged")
        self.data.mkdir()
        (self.data / "gitturtle").symlink_to(external, target_is_directory=True)
        with self.assertRaisesRegex(RuntimeError, "invalid installation directory"):
            installer.save_backup(installer.installed_targets(self.home, self.data), self.home, self.data)
        (self.data / "gitturtle").unlink()
        (self.data / "gitturtle").mkdir()
        (self.data / "gitturtle/install-backups").symlink_to(external, target_is_directory=True)
        with self.assertRaisesRegex(RuntimeError, "invalid installation directory"):
            installer.save_backup(installer.installed_targets(self.home, self.data), self.home, self.data)
        self.assertEqual(marker.read_text(), "unchanged")
        self.assertEqual(list(external.iterdir()), [marker])


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


class BundlePayloadTests(unittest.TestCase):
    """Synthetic payload tests; native probes and desktop commands are mocked."""
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="gitturtle-bundle-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bundle, self.home, self.data = (self.root / name for name in ("bundle", "home", "data"))
        self.binary = self.home / ".local/bin/gitturtle"
        self.names = {"bin/gitturtle", "install.py", "package_identity.py", "README.md", "build-info.json",
                      "icons/app-icon.png", "licenses/LICENSE", "licenses/THIRD_PARTY_NOTICES.md",
                      "licenses/dependencies.json"}
        self.names.update(f"icons/hicolor/{size}x{size}/apps/{installer.APP_ID}.png"
                          for size in installer.ICON_SIZES)
        for name in self.names:
            source = self.bundle / name
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_bytes(b"checksummed synthetic fixture payload")
        header = bytearray(32)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = (62).to_bytes(2, "little")
        (self.bundle / "bin/gitturtle").write_bytes(header)
        self.compiled = dict(application="GitTurtle", version="0.1.0", source_revision="a" * 40,
                             source_tree="clean", target="x86_64-unknown-linux-gnu", profile="release",
                             rustc="rustc 1.98.0", build_unix_seconds="1789492155")
        inventory = dict(target=self.compiled["target"], cargo_lock_sha256="b" * 64,
                         review_required=[], dependencies=[{"name": "fixture"}])
        (self.bundle / "licenses/dependencies.json").write_text(json.dumps(inventory))
        self.info = dict(format=2, application="GitTurtle", bundle_id=installer.APP_ID,
                         distribution="development", target=self.compiled["target"],
                         compiled_identity=self.compiled,
                         packaged_from_revision=self.compiled["source_revision"], packaging_tree_status="clean",
                         binary_sha256=installer.digest_file(self.bundle / "bin/gitturtle"),
                         input_binary_sha256=installer.digest_file(self.bundle / "bin/gitturtle"),
                         cargo_lock_sha256=inventory["cargo_lock_sha256"],
                         license_inventory_sha256=installer.digest_file(self.bundle / "licenses/dependencies.json"),
                         license_review_required=[], signing="unsigned")
        (self.bundle / "build-info.json").write_text(json.dumps(self.info))
        self.write_checksums()
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"preserved existing executable")
        self.unrelated = self.data / "unrelated/settings.json"
        self.unrelated.parent.mkdir(parents=True)
        self.unrelated.write_text("preserved unrelated settings and drafts")

    def write_checksums(self):
        (self.bundle / "SHA256SUMS").write_text("".join(
            f"{installer.digest_file(self.bundle / name)}  {name}\n" for name in sorted(self.names)))

    def native_doubles(self, *, compiled=None, dependencies=None):
        stack = contextlib.ExitStack()
        stack.enter_context(patch.object(installer, "__file__", str(self.bundle / "install.py")))
        stack.enter_context(patch.object(installer, "package_tools", return_value=package_identity))
        probe = stack.enter_context(patch.object(package_identity, "probe", return_value=compiled or self.compiled))
        stack.enter_context(patch.object(installer.shutil, "which", return_value="fixture-tool"))
        stack.enter_context(patch.object(installer.subprocess, "run", return_value=dependencies or
                                        subprocess.CompletedProcess([], 0, stdout="", stderr="")))
        stack.enter_context(patch.object(installer, "refresh_desktop"))
        stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        return stack, probe

    def assert_preserved(self):
        self.assertEqual(self.binary.read_bytes(), b"preserved existing executable")
        self.assertEqual(self.unrelated.read_text(), "preserved unrelated settings and drafts")

    def test_unlisted_icon_cannot_replace_an_unrelated_desktop_icon(self):
        extra = "icons/hicolor/48x48/apps/unrelated.png"
        (self.bundle / extra).write_bytes(b"unchecked extra icon")
        destination = self.data / extra
        destination.parent.mkdir(parents=True)
        destination.write_bytes(b"keep unrelated icon")
        context, _ = self.native_doubles()
        with context:
            installer.install_bundle(self.home, self.data)
        self.assertEqual(destination.read_bytes(), b"keep unrelated icon")
        self.assertEqual(self.binary.read_bytes(), (self.bundle / "bin/gitturtle").read_bytes())

    def test_checksum_mismatch_missing_required_and_duplicate_entries_do_not_probe_or_install(self):
        valid = (self.bundle / "SHA256SUMS").read_text()
        first = valid.splitlines()[0]
        variants = (valid.replace(first[:64], "f" * 64, 1),
                    "\n".join(line for line in valid.splitlines() if not line.endswith("  package_identity.py")),
                    valid + first + "\n")
        for checksum_text in variants:
            with self.subTest(checksum_text=checksum_text[:80]):
                (self.bundle / "SHA256SUMS").write_text(checksum_text)
                context, probe = self.native_doubles()
                with context, self.assertRaises(RuntimeError):
                    installer.install_bundle(self.home, self.data)
                probe.assert_not_called()
                self.assert_preserved()

    def test_checksum_paths_cannot_traverse_or_follow_symlinks(self):
        valid = (self.bundle / "SHA256SUMS").read_text()
        for name in ("../unrelated", "/absolute", "icons//file", "icons/./file", "icons\\file"):
            with self.subTest(name=name):
                (self.bundle / "SHA256SUMS").write_text(f"{'a' * 64}  {name}\n" + valid)
                context, probe = self.native_doubles()
                with context, self.assertRaisesRegex(RuntimeError, "Invalid bundle checksum entry"):
                    installer.install_bundle(self.home, self.data)
                probe.assert_not_called()
                self.assert_preserved()
        (self.bundle / "SHA256SUMS").write_text(valid)
        target = self.bundle / "icons/app-icon.png"
        target.unlink()
        target.symlink_to(self.unrelated)
        context, probe = self.native_doubles()
        with context, self.assertRaisesRegex(RuntimeError, "symlink"):
            installer.install_bundle(self.home, self.data)
        probe.assert_not_called()
        self.assert_preserved()

    def test_coherent_checksums_do_not_bypass_package_identity_validation(self):
        invalid = copy.deepcopy(self.info)
        invalid["compiled_identity"]["source_revision"] = "f" * 40
        (self.bundle / "build-info.json").write_text(json.dumps(invalid))
        self.write_checksums()
        context, probe = self.native_doubles()
        with context, self.assertRaisesRegex(package_identity.PackageError, "source_revision"):
            installer.install_bundle(self.home, self.data)
        probe.assert_not_called()
        self.assert_preserved()

    def test_unchecked_license_is_refused_before_installation(self):
        (self.bundle / "licenses/extra-notice.txt").write_text("unchecked")
        context, probe = self.native_doubles()
        with context, self.assertRaisesRegex(RuntimeError, "unchecked license"):
            installer.install_bundle(self.home, self.data)
        probe.assert_not_called()
        self.assert_preserved()

    def test_runtime_or_compiled_identity_failure_preserves_existing_installation(self):
        context, probe = self.native_doubles(dependencies=subprocess.CompletedProcess(
            [], 0, stdout="libmissing.so => not found", stderr=""))
        with context, self.assertRaisesRegex(RuntimeError, "Runtime library"):
            installer.install_bundle(self.home, self.data)
        probe.assert_not_called()
        self.assert_preserved()
        context, _ = self.native_doubles(compiled=dict(self.compiled, version="99.0.0"))
        with context, self.assertRaisesRegex(RuntimeError, "identity differs"):
            installer.install_bundle(self.home, self.data)
        self.assert_preserved()

    def test_partial_copy_failure_restores_installed_payload_and_keeps_recovery(self):
        metadata = self.data / "gitturtle/build-info.json"
        metadata.parent.mkdir(parents=True)
        metadata.write_text("previous build identity")
        launcher = self.data / f"applications/{installer.APP_ID}.desktop"
        launcher.parent.mkdir(parents=True)
        launcher.write_text("previous launcher")
        real_replace = installer.replace_file
        failed = False

        def fail_once(source, destination, mode):
            nonlocal failed
            if destination == launcher and not failed:
                failed = True
                raise OSError("fixture interrupted copy")
            return real_replace(source, destination, mode)

        context, _ = self.native_doubles()
        with context, patch.object(installer, "replace_file", side_effect=fail_once):
            with self.assertRaisesRegex(OSError, "fixture interrupted copy"):
                installer.install_bundle(self.home, self.data)
        self.assertTrue(failed)
        self.assert_preserved()
        self.assertEqual(metadata.read_text(), "previous build identity")
        self.assertEqual(launcher.read_text(), "previous launcher")
        marker = json.loads((self.data / "gitturtle/previous-installation.json").read_text())
        backup = self.data / "gitturtle/install-backups" / marker["backup"]
        installer.backup_entries(backup, self.home, self.data)
        self.assertFalse((self.data / "gitturtle/licenses/LICENSE").exists())


class InstallationLockTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="gitturtle-lock-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home, self.data = self.root / "home", self.root / "data"
        self.support = self.data / "gitturtle"
        self.support.mkdir(parents=True)

    def invoke(self):
        with (patch.object(installer.Path, "home", return_value=self.home),
              patch.object(installer, "data_path", return_value=self.data),
              patch.object(installer.platform, "system", return_value="Linux"),
              patch.object(installer.platform, "machine", return_value="x86_64"),
              patch.object(sys, "argv", ["install.py"]),
              patch.object(installer, "install_bundle") as install):
            try:
                installer.install()
            finally:
                install.assert_not_called()

    def test_concurrent_installation_is_refused_without_touching_payload(self):
        with (self.support / "install.lock").open("a") as held:
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(RuntimeError, "Another GitTurtle installation"):
                self.invoke()

    def test_symlink_lock_is_refused_without_touching_target(self):
        marker = self.root / "unrelated"
        marker.write_text("preserve")
        (self.support / "install.lock").symlink_to(marker)
        with self.assertRaisesRegex(RuntimeError, "Invalid installation lock"):
            self.invoke()
        self.assertEqual(marker.read_text(), "preserve")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, help="Existing complete bundle with current installer and hashes")
    arguments, remaining = parser.parse_known_args()
    BUNDLE = arguments.bundle
    unittest.main(argv=[sys.argv[0], *remaining])
