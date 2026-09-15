#!/usr/bin/env python3
"""Disposable source fixtures with simulated Apple commands, never native evidence."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock
import zipfile

_spec = importlib.util.spec_from_file_location("package_macos", Path(__file__).with_name("package-macos.py"))
package = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(package)


class PackageFixtures(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="gitturtle-macos-source-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.project = self.root / "project"
        self.project.mkdir()
        self.git_env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.git_env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        (self.project / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0"\n')
        (self.project / "Cargo.lock").write_text("fixture lock\n")
        (self.project / ".gitignore").write_text("target/\n")
        icon = self.project / "assets/AppIcon.icon"
        icon.mkdir(parents=True)
        (icon / "icon.json").write_text('{"groups": []}')
        subprocess.run(["git", "init", "-q", str(self.project)], check=True, env=self.git_env)
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture")
        self.revision = self.git("rev-parse", "HEAD").strip()
        self.binary = self.project / f"target/{package.TARGET}/release/gitturtle"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"\xcf\xfa\xed\xfe" + (0x0100000C).to_bytes(4, "little") + b"\0" * 24 + b"fixture executable")
        self.binary.chmod(0o755)
        self.compiled = dict(application="GitTurtle", version="0.1.0", source_revision=self.revision,
                             source_tree="clean", target=package.TARGET, profile="release",
                             rustc="rustc 1.98.0 (fixture)", build_unix_seconds="1789490000")
        self.bundle = self.root / "output/GitTurtle.app"
        self.archive_dir = self.root / "archives"
        self.commands = []
        self.fail = None
        self.reviews = []
        self.generated = dict(CFBundleIconFile="AppIcon", CFBundleIconName="AppIcon")
        self.xcode = "Xcode 26.0\nBuild version FIXTURE\n"
        self.signature = "Executable=/private/fixture/GitTurtle.app/Contents/MacOS/gitturtle\nSignature=adhoc\nIdentifier=com.gitturtle.desktop\n"
        self.catalog = '[{"Name": "AppIcon", "AssetType": "Icon Stack"}]'
        self.probe = mock.patch.object(package.identity, "probe", side_effect=lambda path: dict(self.compiled))
        self.probe.start()
        self.addCleanup(self.probe.stop)
        self.run = mock.patch.object(package, "run", side_effect=self.fake_run)
        self.run.start()
        self.addCleanup(self.run.stop)

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.project), *args], text=True, env=self.git_env)

    def args(self, *extra):
        return package.parser().parse_args(["--no-build", "--archive-dir", str(self.archive_dir), *map(str, extra), str(self.bundle)])

    def invoke(self, *extra):
        with contextlib.redirect_stderr(io.StringIO()):
            return package.package(self.args(*extra), self.project)

    def fake_run(self, command, **kwargs):
        command = list(map(str, command))
        self.commands.append(command)
        if self.fail and self.fail(command):
            raise package.PackageError("simulated Apple/tool failure")
        if command == ["xcodebuild", "-version"]:
            return self.xcode
        if command == ["xcode-select", "-p"]:
            return "/Applications/Xcode.app/Contents/Developer\n"
        if command[:2] == ["xcrun", "--find"]:
            return "/fixture/tools/" + command[2] + "\n"
        if command[:3] in (["xcrun", "metal", "--version"], ["xcrun", "actool", "--version"]):
            return "Fixture tool 26.0\n"
        if len(command) > 1 and command[1].endswith("collect-third-party-licenses.py"):
            licenses = Path(command[-1])
            licenses.mkdir()
            (licenses / "LICENSE").write_text("fixture license")
            (licenses / "THIRD_PARTY_NOTICES.md").write_text("fixture notice")
            (licenses / "dependencies.json").write_text(json.dumps(dict(
                target=package.TARGET, cargo_lock_sha256=package.identity.digest(self.project / "Cargo.lock"),
                review_required=self.reviews, dependencies=[{"name": "fixture"}]
            )))
            if self.reviews:
                (licenses / "REVIEW_REQUIRED.md").write_text("fixture missing authoritative notice")
                if "--require-complete" in command:
                    raise package.PackageError("Complete notices required (simulated collector)")
            return ""
        if command[:2] == ["xcrun", "actool"]:
            output = Path(command[command.index("--compile") + 1])
            (output / "AppIcon.icns").write_bytes(b"fixture icns")
            (output / "Assets.car").write_bytes(b"fixture car")
            (output / "icon-info.plist").write_bytes(plistlib.dumps(self.generated))
            return ""
        if command[:3] == ["xcrun", "assetutil", "--info"]:
            return self.catalog
        if command[0] == "plutil":
            plistlib.loads(Path(command[-1]).read_bytes())
            return ""
        if command[:2] == ["codesign", "--force"]:
            app = Path(command[-1])
            with (app / "Contents/MacOS/gitturtle").open("ab") as stream:
                stream.write(b"simulated signature")
            return ""
        if command[:2] == ["codesign", "--verify"]:
            return ""
        if command[:2] == ["codesign", "--display"]:
            return self.signature
        if command[:2] == ["ditto", "-c"]:
            root, output = map(Path, command[-2:])
            with zipfile.ZipFile(output, "x") as zipped:
                for path in sorted(root.rglob("*")):
                    zipped.write(path, path.relative_to(root).as_posix())
            return ""
        if command[:2] == ["ditto", "-x"]:
            zipped, output = map(Path, command[-2:])
            with zipfile.ZipFile(zipped) as archive:
                archive.extractall(output)
                for entry in archive.infolist():
                    (output / entry.filename).chmod((entry.external_attr >> 16) & 0o777)
            return ""
        if command[0] == "ditto":
            shutil.copytree(command[1], command[2])
            return ""
        if command[0] == "cargo":
            return ""
        raise AssertionError(f"Unexpected command: {command}")

    def legacy_bundle(self):
        (self.bundle / "Contents/MacOS").mkdir(parents=True)
        (self.bundle / "Contents/Resources").mkdir()
        shutil.copy2(self.binary, self.bundle / "Contents/MacOS/gitturtle")
        info = dict(CFBundleIdentifier=package.identity.APP_ID, CFBundleExecutable="gitturtle", CFBundlePackageType="APPL")
        (self.bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
        return package.tree_digest(self.bundle)

    def assert_no_published_output(self):
        self.assertFalse(self.bundle.exists())
        if self.archive_dir.exists():
            self.assertFalse(any(path.name.startswith("GitTurtle-") for path in self.archive_dir.iterdir()))

    def test_complete_outputs_match_final_signed_bytes(self):
        original = package.identity.digest(self.binary)
        outputs = self.invoke("--expected-sha256", original)
        self.assertEqual(len(outputs), 5)
        sidecar = self.bundle.with_name("GitTurtle.app.build-info.json")
        value = package.identity.read_json(sidecar)
        self.assertEqual(value["input_binary_sha256"], original)
        self.assertNotEqual(value["binary_sha256"], original)
        self.assertEqual(value["binary_sha256"], package.identity.digest(self.bundle / "Contents/MacOS/gitturtle"))
        self.assertEqual(value["signing"], "ad-hoc")
        self.assertEqual(value["macos_package"]["notarization"], "not-performed")
        self.assertNotIn("/private/fixture", json.dumps(value))
        self.assertFalse((self.bundle / "Contents/Resources/build-info.json").exists())
        archive = self.archive_dir / package.archive_name("0.1.0", self.revision, "release")
        self.assertEqual(archive.with_name(archive.name + ".sha256").read_text(), f"{package.identity.digest(archive)}  {archive.name}\n")
        outer = json.loads(archive.with_name(archive.name + ".manifest.json").read_text())
        self.assertEqual(outer["archive_sha256"], package.identity.digest(archive))
        self.assertEqual(outer["package_manifest_sha256"], package.identity.digest(sidecar))
        with zipfile.ZipFile(archive) as zipped:
            self.assertEqual(zipped.read("build-info.json"), sidecar.read_bytes())
        self.assertEqual(package.identity.digest(self.binary), original)

    def test_legacy_matching_bundle_is_replaced_after_validation(self):
        old = self.legacy_bundle()
        self.invoke()
        self.assertNotEqual(package.tree_digest(self.bundle), old)

    def test_local_development_preserves_missing_notice_record(self):
        self.reviews = ["mac 0.1.1", "ufbx 0.11.3"]
        self.invoke()
        self.assertTrue((self.bundle / "Contents/Resources/licenses/REVIEW_REQUIRED.md").is_file())
        value = package.identity.read_json(self.bundle.with_name("GitTurtle.app.build-info.json"))
        self.assertEqual(value["distribution"], "development")
        self.assertEqual(value["license_review_required"], self.reviews)

    def test_strict_notice_failure_preserves_existing_bundle(self):
        old = self.legacy_bundle()
        self.reviews = ["mac 0.1.1"]
        with self.assertRaisesRegex(package.PackageError, "Complete notices"):
            self.invoke("--distribution")
        self.assertEqual(package.tree_digest(self.bundle), old)
        self.assertFalse(any(command[0] == "codesign" for command in self.commands))
        self.assertFalse(any(command[:2] == ["xcrun", "actool"] and "--compile" in command for command in self.commands))
        self.assertEqual(len(list(self.bundle.parent.glob(".gitturtle-package-*/failure.txt"))), 1)

    def test_incomplete_inventory_cannot_bypass_collector(self):
        self.reviews = ["mac 0.1.1"]
        original = self.fake_run
        def broken_collector(command, **kwargs):
            if len(command) > 1 and str(command[1]).endswith("collect-third-party-licenses.py"):
                command = [item for item in command if item != "--require-complete"]
            return original(command, **kwargs)
        self.run.stop()
        with mock.patch.object(package, "run", side_effect=broken_collector):
            with self.assertRaisesRegex(package.PackageError, "public distribution is blocked"):
                self.invoke("--distribution")
        self.assert_no_published_output()

    def test_successful_distribution_still_reports_ad_hoc(self):
        self.invoke("--distribution")
        value = package.identity.read_json(self.bundle.with_name("GitTurtle.app.build-info.json"))
        self.assertEqual(value["distribution"], "complete-notices")
        self.assertEqual(value["signing"], "ad-hoc")

    def test_identity_refusals_preserve_existing_bundle(self):
        old = self.legacy_bundle()
        for field, value in (("source_revision", "a" * 40), ("version", "0.2.0"),
                             ("target", "x86_64-apple-darwin"), ("profile", "debug"),
                             ("source_tree", "unknown"), ("application", "Other")):
            with self.subTest(field=field):
                original = self.compiled[field]
                self.compiled[field] = value
                with self.assertRaises(package.PackageError):
                    self.invoke()
                self.compiled[field] = original
                self.assertEqual(package.tree_digest(self.bundle), old)

    def test_missing_wrong_format_or_symlink_binary_refused(self):
        original = self.binary.read_bytes()
        self.binary.unlink()
        with self.assertRaises(FileNotFoundError):
            self.invoke()
        self.binary.write_bytes(b"ELF fixture")
        with self.assertRaisesRegex(package.PackageError, "format/architecture"):
            self.invoke()
        self.binary.unlink()
        other = self.root / "other"
        other.write_bytes(original)
        self.binary.symlink_to(other)
        with self.assertRaisesRegex(package.PackageError, "regular file"):
            self.invoke()
        self.assert_no_published_output()

    def test_expected_digest_and_revision_mismatch_refused(self):
        for options in (("--expected-sha256", "0" * 64), ("--expected-revision", "a" * 40),
                        ("--expected-version", "0.2.0"), ("--expected-sha256", "bad")):
            with self.subTest(options=options):
                with self.assertRaises(package.PackageError):
                    self.invoke(*options)
        self.assert_no_published_output()

    def test_modified_local_reuse_requires_captured_digest(self):
        self.compiled["source_tree"] = "modified"
        with self.assertRaisesRegex(package.PackageError, "explicit --expected-sha256"):
            self.invoke()
        self.invoke("--expected-sha256", package.identity.digest(self.binary))

    def test_distribution_rejects_modified_build_or_checkout(self):
        self.compiled["source_tree"] = "modified"
        with self.assertRaisesRegex(package.PackageError, "clean release build"):
            self.invoke("--distribution")
        self.compiled["source_tree"] = "clean"
        (self.project / "changed.txt").write_text("uncommitted")
        with self.assertRaisesRegex(package.PackageError, "clean packaging checkout"):
            self.invoke("--distribution")
        self.assert_no_published_output()

    def test_unrelated_and_symlink_outputs_preserved(self):
        self.bundle.parent.mkdir(parents=True)
        marker = self.root / "unrelated"
        marker.write_text("preserve")
        self.bundle.symlink_to(marker)
        with self.assertRaises(package.PackageError):
            self.invoke()
        self.assertTrue(self.bundle.is_symlink())
        self.assertEqual(marker.read_text(), "preserve")
        self.bundle.unlink()
        self.legacy_bundle()
        (self.bundle / "Contents/Resources/link").symlink_to(marker)
        with self.assertRaisesRegex(package.PackageError, "link or special"):
            self.invoke()
        self.assertEqual(marker.read_text(), "preserve")

    def test_archive_collision_including_dangling_sidecar_is_preserved(self):
        self.archive_dir.mkdir()
        archive = self.archive_dir / package.archive_name("0.1.0", self.revision, "release")
        sidecar = Path(str(archive) + ".sha256")
        sidecar.symlink_to(self.root / "missing")
        with self.assertRaisesRegex(package.PackageError, "Archive or archive sidecar exists"):
            self.invoke()
        self.assertTrue(sidecar.is_symlink())
        self.assert_no_published_output_except(sidecar)

    def assert_no_published_output_except(self, allowed):
        self.assertFalse(self.bundle.exists())
        self.assertEqual(list(self.archive_dir.iterdir()), [allowed])

    def test_existing_unrelated_manifest_refused(self):
        self.bundle.parent.mkdir()
        sidecar = self.bundle.with_name("GitTurtle.app.build-info.json")
        sidecar.write_text("unrelated")
        with self.assertRaisesRegex(package.PackageError, "unrelated existing package manifest"):
            self.invoke()
        self.assertEqual(sidecar.read_text(), "unrelated")

    def test_tool_failure_at_each_apple_stage_preserves_bundle(self):
        old = self.legacy_bundle()
        failures = [lambda c: c == ["xcrun", "--find", "metal"],
                    lambda c: c[:3] == ["xcrun", "metal", "--version"],
                    lambda c: c[:2] == ["xcrun", "actool"] and "--compile" in c,
                    lambda c: c[:3] == ["xcrun", "assetutil", "--info"],
                    lambda c: c[0] == "plutil", lambda c: c[:2] == ["codesign", "--force"],
                    lambda c: c[:2] == ["codesign", "--verify"],
                    lambda c: c[:2] == ["ditto", "-c"], lambda c: c[:2] == ["ditto", "-x"]]
        for index, failure in enumerate(failures):
            with self.subTest(stage=index):
                self.fail = failure
                with self.assertRaisesRegex(package.PackageError, "simulated"):
                    self.invoke()
                self.assertEqual(package.tree_digest(self.bundle), old)
        self.assertFalse(any(path.name.startswith("GitTurtle-") for path in self.archive_dir.iterdir()))

    def test_unsupported_xcode_and_missing_catalog_metadata_refused(self):
        self.xcode = "Xcode 16.4\n"
        with self.assertRaisesRegex(package.PackageError, "Xcode 26"):
            self.invoke()
        self.xcode = "Xcode 26.0\n"
        self.catalog = "[]"
        with self.assertRaisesRegex(package.PackageError, "catalog"):
            self.invoke()
        self.assert_no_published_output()

    def test_generated_metadata_cannot_override_identity(self):
        self.generated["CFBundleExecutable"] = "unexpected"
        with self.assertRaisesRegex(package.PackageError, "unexpected bundle metadata"):
            self.invoke()
        self.assert_no_published_output()

    def test_signature_metadata_is_verified(self):
        self.signature = "Signature=Developer ID\n"
        with self.assertRaisesRegex(package.PackageError, "ad-hoc signature"):
            self.invoke()
        self.assert_no_published_output()

    def test_signing_changed_identity_refused(self):
        calls = 0
        def probe(path):
            nonlocal calls
            calls += 1
            value = dict(self.compiled)
            if calls == 3:
                value["build_unix_seconds"] = "1"
            return value
        with mock.patch.object(package.identity, "probe", side_effect=probe):
            with self.assertRaisesRegex(package.PackageError, "identity changed"):
                self.invoke()
        self.assert_no_published_output()

    def test_build_passes_explicit_target_and_reads_matching_output(self):
        args = package.parser().parse_args([str(self.bundle)])
        package.package(args, self.project)
        command = next(command for command in self.commands if command[0] == "cargo")
        self.assertEqual(command[command.index("--target") + 1], package.TARGET)
        self.assertIn("--release", command)

    def test_local_debug_and_explicit_binary_remain_supported(self):
        self.compiled["profile"] = "debug"
        args = package.parser().parse_args(["--debug", "--no-build", "--binary", str(self.binary), str(self.bundle)])
        outputs = package.package(args, self.project)
        self.assertEqual(len(outputs), 2)
        self.assertEqual(package.identity.read_json(self.bundle.with_name("GitTurtle.app.build-info.json"))["compiled_identity"]["profile"], "debug")

    def test_no_build_option_contract(self):
        for options in (["--binary", str(self.binary)], ["--distribution"], ["--distribution", "--debug", "--archive-dir", str(self.archive_dir)]):
            with self.subTest(options=options):
                args = package.parser().parse_args([*options, str(self.bundle)])
                with self.assertRaises(package.PackageError):
                    package.package(args, self.project)

    def test_prerelease_version_refuses_unsupported_plist_mapping(self):
        (self.project / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0-beta.1"\n')
        with self.assertRaisesRegex(package.PackageError, "prerelease mapping"):
            self.invoke()

    def test_publication_failure_rolls_back_original_bundle(self):
        old = self.legacy_bundle()
        real_link = os.link
        def link(source, destination, *args, **kwargs):
            if str(destination).endswith(".zip.sha256"):
                raise OSError("simulated publication failure")
            return real_link(source, destination, *args, **kwargs)
        with mock.patch.object(package.os, "link", side_effect=link):
            with self.assertRaisesRegex(OSError, "publication failure"):
                self.invoke()
        self.assertEqual(package.tree_digest(self.bundle), old)
        self.assertFalse(self.bundle.with_name("GitTurtle.app.build-info.json").exists())
        self.assertFalse(any(path.name.startswith("GitTurtle-") for path in self.archive_dir.iterdir()))

    def test_concurrent_output_change_does_not_get_removed(self):
        old = self.legacy_bundle()
        real_run = self.fake_run
        def run(command, **kwargs):
            result = real_run(command, **kwargs)
            if list(map(str, command))[:2] == ["ditto", "-x"]:
                (self.bundle / "Contents/Resources/concurrent.txt").write_text("keep this")
            return result
        with mock.patch.object(package, "run", side_effect=run):
            with self.assertRaisesRegex(package.PackageError, "changed during packaging"):
                self.invoke()
        self.assertNotEqual(package.tree_digest(self.bundle), old)
        self.assertEqual((self.bundle / "Contents/Resources/concurrent.txt").read_text(), "keep this")

    def test_zip_traversal_and_duplicate_members_refused(self):
        for name in ("../escape", "/absolute", "GitTurtle.app/../escape", "other/file", "GitTurtle.app\\escape"):
            with self.subTest(name=name):
                path = self.root / "bad.zip"
                with zipfile.ZipFile(path, "w") as archive:
                    archive.writestr(name, "bad")
                with self.assertRaisesRegex(package.PackageError, "Unsafe or unexpected"):
                    package.inspect_zip(path)

    def test_app_change_during_backup_capture_restored_with_diagnostics(self):
        self.legacy_bundle()
        real_rename = package.rename_exclusive
        marker = self.bundle / "Contents/Resources/concurrent-user-file.txt"
        changed = False
        def rename(source, destination):
            nonlocal changed
            if source == self.bundle and not changed:
                marker.write_text("preserve concurrent app bytes")
                changed = True
            return real_rename(source, destination)
        with mock.patch.object(package, "rename_exclusive", side_effect=rename):
            with self.assertRaisesRegex(package.PackageError, "changed during backup capture"):
                self.invoke()
        self.assertTrue(changed)
        self.assertEqual(marker.read_text(), "preserve concurrent app bytes")
        self.assertEqual(len(list(self.bundle.parent.glob(".gitturtle-package-*/failure.txt"))), 1)
        self.assertFalse(self.bundle.with_name("GitTurtle.app.build-info.json").exists())
        self.assertFalse(any(path.name.startswith("GitTurtle-") for path in self.archive_dir.iterdir()))

    def test_sidecar_change_during_backup_capture_restores_app_and_sidecar(self):
        # Create a recognized current bundle/sidecar pair, then use a fresh
        # archive destination for the attempted replacement.
        self.invoke()
        self.archive_dir = self.root / "second-archives"
        old_app = package.tree_digest(self.bundle)
        sidecar = self.bundle.with_name("GitTurtle.app.build-info.json")
        new_bytes = sidecar.read_bytes() + b"\n"
        real_rename = package.rename_exclusive
        changed = False
        def rename(source, destination):
            nonlocal changed
            if source == sidecar and not changed:
                sidecar.write_bytes(new_bytes)
                changed = True
            return real_rename(source, destination)
        with mock.patch.object(package, "rename_exclusive", side_effect=rename):
            with self.assertRaisesRegex(package.PackageError, "changed during backup capture"):
                self.invoke()
        self.assertTrue(changed)
        self.assertEqual(package.tree_digest(self.bundle), old_app)
        self.assertEqual(sidecar.read_bytes(), new_bytes)
        self.assertEqual(len(list(self.bundle.parent.glob(".gitturtle-package-*/failure.txt"))), 1)
        self.assertFalse(any(path.name.startswith("GitTurtle-") for path in self.archive_dir.iterdir()))

    def test_captured_app_changed_during_publication_is_restored(self):
        self.legacy_bundle()
        real_link = os.link
        changed = False
        def link(source, destination, *args, **kwargs):
            nonlocal changed
            if str(destination).endswith(".zip.sha256") and not changed:
                backups = list(self.bundle.parent.glob(".gitturtle-package-*/previous-0"))
                self.assertEqual(len(backups), 1)
                (backups[0] / "Contents/Resources/concurrent-user-file.txt").write_text("preserve captured app bytes")
                changed = True
            return real_link(source, destination, *args, **kwargs)
        with mock.patch.object(package.os, "link", side_effect=link):
            with self.assertRaisesRegex(package.PackageError, "changed after backup capture"):
                self.invoke()
        self.assertTrue(changed)
        self.assertEqual((self.bundle / "Contents/Resources/concurrent-user-file.txt").read_text(), "preserve captured app bytes")
        self.assertFalse(self.bundle.with_name("GitTurtle.app.build-info.json").exists())
        self.assertFalse(any(path.name.startswith("GitTurtle-") for path in self.archive_dir.iterdir()))

    def test_exclusive_rename_preserves_existing_empty_directory(self):
        source, destination = self.root / "source", self.root / "destination"
        source.mkdir()
        destination.mkdir()
        with self.assertRaises(FileExistsError):
            package.rename_exclusive(source, destination)
        self.assertTrue(source.is_dir())
        self.assertTrue(destination.is_dir())

    def test_missing_icon_file_preserves_existing_bundle(self):
        old = self.legacy_bundle()
        real_run = self.fake_run
        def run(command, **kwargs):
            result = real_run(command, **kwargs)
            if list(map(str, command))[:2] == ["xcrun", "actool"] and "--compile" in command:
                (Path(command[command.index("--compile") + 1]) / "Assets.car").unlink()
            return result
        with mock.patch.object(package, "run", side_effect=run):
            with self.assertRaises(FileNotFoundError):
                self.invoke()
        self.assertEqual(package.tree_digest(self.bundle), old)

    def test_different_archived_bytes_fail_before_output_replacement(self):
        old = self.legacy_bundle()
        real_run = self.fake_run
        def run(command, **kwargs):
            result = real_run(command, **kwargs)
            if list(map(str, command))[:2] == ["ditto", "-x"]:
                (Path(command[-1]) / "GitTurtle.app/Contents/Resources/AppIcon.icns").write_bytes(b"corruption")
            return result
        with mock.patch.object(package, "run", side_effect=run):
            with self.assertRaisesRegex(package.PackageError, "Extracted archive differs"):
                self.invoke()
        self.assertEqual(package.tree_digest(self.bundle), old)

    def test_cancellation_preserves_existing_bundle_and_releases_lock(self):
        old = self.legacy_bundle()
        with mock.patch.object(package, "inspect_catalog", side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                self.invoke()
        self.assertEqual(package.tree_digest(self.bundle), old)
        self.assertFalse(self.bundle.with_name(".GitTurtle.app.package-lock").exists())


if __name__ == "__main__":
    unittest.main()
