"""Simulated release transport and fabricated package fixtures; no public writes,
Apple service calls, actual executable launches or native evidence are produced.
"""
from __future__ import annotations

import copy
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import sys
import tarfile
import tempfile
import types
import unittest
from unittest.mock import patch
import urllib.error
import zipfile

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("release_workflow_tests", ROOT / "scripts/release/workflow.py")
release = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = release
spec.loader.exec_module(release)
github = release.github
MAC, LINUX_X86, LINUX_ARM, LINUX_TARGETS = release.MAC, release.LINUX_X86, release.LINUX_ARM, release.LINUX_TARGETS
SHA = "a" * 40
TAG = "b" * 40
LOCK = "c" * 64
REQUIRED = ["Quality gate", "Rust formatting",
            "Rust tests · macos-15 · 1/2", "Rust tests · macos-15 · 2/2",
            "Rust tests · ubuntu-24.04 · 1/2", "Rust tests · ubuntu-24.04 · 2/2",
            "Rust Clippy · macos-15", "Rust Clippy · ubuntu-24.04",
            "Rust release · macos-26", "Rust release · ubuntu-24.04", "Rust release · ubuntu-24.04-arm"]
ARM_RELEASE_JOB = "Rust release · ubuntu-24.04-arm"


def context(platforms=LINUX_TARGETS, signed=False):
    value = {"format": 1, "identity": {"tag": "v0.1.0", "version": "0.1.0", "commit": SHA,
        "tag_object": TAG, "platforms": sorted(platforms), "cargo_lock_sha256": LOCK},
        "macos_signing": "notarized" if signed else "ad-hoc",
        "signing_identity": {"certificate_sha1": "D" * 40, "team_id": "EXAMPLE123" + "4"} if signed else None,
        "run_id": 123, "run_attempt": 1, "quality": {"run_id": 45, "run_attempt": 1,
        "url": f"https://github.com/{github.REPOSITORY}/actions/runs/45", "required_jobs": sorted(REQUIRED)}}
    value["approval_sha256"] = hashlib.sha256(github.canonical(release.approval_context(value))).hexdigest()
    return value


def environment(ctx):
    return {"GITHUB_REPOSITORY": github.REPOSITORY, "GITHUB_EVENT_NAME": "workflow_dispatch",
            "GITHUB_REF": "refs/heads/main", "GITHUB_SHA": SHA, "GITHUB_WORKFLOW_SHA": SHA,
            "GITHUB_WORKFLOW_REF": github.REPOSITORY + "/.github/workflows/release.yml@refs/heads/main",
            "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "1", "RELEASE_CONTEXT": json.dumps(ctx)}


class FakeGitHub:
    """The state machine uses real request/response boundaries with an in-memory
    remote. No fake success replaces a required real hosted/package attestation.
    """
    def __init__(self):
        self.releases = []
        self.assets = []
        self.contents = {}
        self.calls = []
        self.main = SHA
        self.tag = TAG
        self.peeled = SHA
        self.quality = {"head_sha": SHA, "head_branch": "main", "event": "push",
            "path": ".github/workflows/quality.yml", "repository": {"full_name": github.REPOSITORY},
            "head_repository": {"full_name": github.REPOSITORY}, "status": "completed",
            "conclusion": "success", "run_attempt": 1}
        self.jobs = [{"name": name, "head_sha": SHA, "status": "completed", "conclusion": "success"}
                     for name in REQUIRED]
        self.fail = None

    def request(self, method, route, value=None):
        self.calls.append((method, route, copy.deepcopy(value)))
        if self.fail == "create" and method == "POST":
            raise github.Error("Simulated ambiguous create")
        if method == "GET":
            if route.endswith("/git/ref/heads/main"):
                return {"object": {"type": "commit", "sha": self.main}}
            if "/git/ref/tags/" in route:
                return {"object": {"type": "tag", "sha": self.tag}}
            if "/git/tags/" in route:
                return {"sha": self.tag, "object": {"type": "commit", "sha": self.peeled}}
            if route.endswith("/actions/runs/45"):
                return copy.deepcopy(self.quality)
            if route.endswith("/releases/99"):
                return copy.deepcopy(self.releases[0])
        if method == "POST" and route.endswith("/releases"):
            result = {**value, "id": 99}
            self.releases.append(result)
            return copy.deepcopy(result)
        if method == "PATCH":
            self.releases[0].update(value)
            if self.fail == "publish":
                raise github.Error("Simulated lost publication reply")
            return copy.deepcopy(self.releases[0])
        raise AssertionError((method, route))

    def pages(self, route, key=None):
        self.calls.append(("LIST", route, key))
        if "/jobs" in route:
            return copy.deepcopy(self.jobs)
        return copy.deepcopy(self.assets if route.endswith("/assets") else self.releases)

    def upload(self, release_id, path):
        self.calls.append(("UPLOAD", path.name, release_id))
        if self.fail == "upload":
            raise github.Error("Simulated uncertain upload")
        item = {"name": path.name, "id": len(self.assets) + 1, "size": path.stat().st_size, "state": "uploaded"}
        self.assets.append(item)
        self.contents[item["id"]] = path.read_bytes()
        return copy.deepcopy(item)

    def download_digest(self, item):
        return hashlib.sha256(self.contents[item["id"]]).hexdigest()


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gitturtle-release-fixture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.ctx = context()
        self.inputs = self.root / "inputs"
        self.inputs.mkdir()
        self.build_index = 0

    def executable(self, target):
        if target in LINUX_TARGETS:
            machine = release.package_id.ELF_MACHINES[target].to_bytes(2, "little")
            return b"\x7fELF\x02\x01" + b"\0" * 12 + machine + b"\0" * 12
        return bytes.fromhex("cffaedfe0c000001") + b"\0" * 24

    def archive(self, directory, target, *, name=None, executable=None, gaps=False, signing=None):
        """Write one fabricated archive, its checksum and its manifest into directory."""
        linux = target in LINUX_TARGETS
        self.build_index += 1
        tree = self.root / ("tree" + str(self.build_index))
        tree.mkdir()
        suffix = ".tar.gz" if linux else ".zip"
        name = name or f"GitTurtle-0.1.0-{SHA[:12]}-{release.packages.TARGETS[target]}-release{suffix}"
        root = tree / (name.removesuffix(suffix) if linux else "GitTurtle.app")
        root.mkdir()
        binary = root / ("bin/gitturtle" if linux else "Contents/MacOS/gitturtle")
        binary.parent.mkdir(parents=True)
        binary.write_bytes(executable or self.executable(target))
        binary.chmod(0o755)
        licenses = root / ("licenses" if linux else "Contents/Resources/licenses")
        licenses.mkdir(parents=True)
        (licenses / "LICENSE").write_text("Fabricated test license\n")
        (licenses / "THIRD_PARTY_NOTICES.md").write_text("Fixture full notices\n")
        (licenses / "dependency.txt").write_text("Entire fixture dependency notice\n")
        inventory = {"target": target, "cargo_lock_sha256": LOCK, "review_required": ["missing-1"] if gaps else [],
            "dependencies": [{"name": "fixture", "version": "1", "notices": [{"path": "dependency.txt",
                "sha256": release.package_id.digest(licenses / "dependency.txt")}]}]}
        release.write_json(licenses / "dependencies.json", inventory)
        if gaps:
            (licenses / "REVIEW_REQUIRED.md").write_text("Fixture gap\n")
        package = {"format": 2, "application": "GitTurtle", "bundle_id": "com.gitturtle.GitTurtle", "target": target,
            "packaged_from_revision": SHA, "packaging_tree_status": "clean", "release_built_by_packager": False,
            "binary_sha256": release.package_id.digest(binary), "input_binary_sha256": release.package_id.digest(binary),
            "compiled_identity": {"application": "GitTurtle", "source_revision": SHA, "version": "0.1.0",
                "target": target, "profile": "release", "source_tree": "clean", "rustc": "rustc fixture",
                "build_unix_seconds": "1"}, "cargo_lock_sha256": LOCK,
            "license_inventory_sha256": release.package_id.digest(licenses / "dependencies.json"),
            "license_review_required": inventory["review_required"], "distribution": "complete-notices",
            "signing": signing or ("unsigned" if linux else "ad-hoc")}
        info = (root if linux else tree) / "build-info.json"
        release.write_json(info, package)
        archive = directory / name
        if linux:
            with tarfile.open(archive, "w:gz") as stream:
                stream.add(root, arcname=root.name)
        else:
            with zipfile.ZipFile(archive, "w") as stream:
                for path in tree.rglob("*"):
                    if path.is_file():
                        stream.write(path, path.relative_to(tree))
        release.write_json(Path(str(archive) + ".manifest.json"), release.package_id.archive_manifest(archive, info))
        Path(str(archive) + ".sha256").write_text(f"{release.package_id.digest(archive)}  {name}\n")
        return archive, package, binary

    def package(self, target=LINUX_X86, *, ctx=None, name=None, gaps=False, signing=None):
        ctx = ctx or self.ctx
        signed = signing == "developer-id-notarized"
        directory = self.inputs / (("release-signed-" if signed else "release-package-") + target + "-123-1")
        directory.mkdir()
        archive, package, binary = self.archive(directory, target, name=name, gaps=gaps, signing=signing)
        expected = release.expected_package(directory, ctx, target)
        receipt = {"format": 1, "context": ctx, "expected": expected,
            "producer": "signed-release-build" if signed else "fresh-release-build", "platform_checks": "passed",
            "native_interaction": "fixture-only", "signing": package["signing"]}
        if signed:
            report = self.signing_report(ctx, package)
            report["release_archive"] = github.file_record(archive)
            release.write_json(directory / "signing-result.json", report)
            receipt["signing_report_sha256"] = release.package_id.digest(directory / "signing-result.json")
        release.write_json(directory / "receipt.json", receipt)
        return directory, package, binary

    def linux(self, **fault):
        """Both archives of a Linux release; a requested fault goes into the x86-64 one."""
        self.package(LINUX_ARM)
        return self.package(LINUX_X86, **fault)

    def fresh_inputs(self, name):
        self.inputs = self.root / ("inputs-" + name)
        self.inputs.mkdir()

    def signing_report(self, ctx, package):
        executable = package["binary_sha256"]
        keys = ("application", "source_revision", "version", "target", "source_tree", "profile")
        return {"schema_version": 1, "status": "signed-notarized-stapled", "source_revision": SHA,
            "version": "0.1.0", "target": MAC, "staple_validation": "passed", "credential_workspace_cleanup": "passed",
            "keychain_cleanup": {"status": "passed"}, "compiled_identity": {k: package["compiled_identity"][k] for k in keys},
            "signature": {"type": "Developer ID Application", **ctx["signing_identity"], "hardened_runtime": True,
                          "secure_timestamp": True, "entitlements": {}},
            "pre_sign_executable_sha256": executable, "signed_executable_sha256": executable,
            "post_staple_executable_sha256": executable, "submitted_archive_sha256": "e" * 64,
            "notarization": {"id": "f" * 32, "status": "Accepted", "log": {"job_id": "f" * 32,
                "status": "Accepted", "status_code": 0, "sha256": "e" * 64, "issue_counts": {"error": 0}}}}

    def assemble(self, ctx=None, name="assembled"):
        output = self.root / name
        with patch.object(release, "local_identity"):
            release.assemble(types.SimpleNamespace(directory=self.inputs, output=output), ctx or self.ctx)
        return output

    def mutate_receipt(self, directory, mutate):
        path = directory / "receipt.json"
        value = json.loads(path.read_text())
        mutate(value)
        path.write_bytes(github.canonical(value))


class Packages(Fixture):
    def test_linux_archive_assembly_verifies_actual_bytes_without_executing_them(self):
        directory, package, binary = self.linux()
        with patch.object(release.package_id, "probe", side_effect=AssertionError("cross-platform assembly executed a binary")):
            output = self.assemble()
        value = json.loads((output / "release-manifest.json").read_bytes())
        self.assertEqual(value["context"], self.ctx)
        self.assertEqual(value["platforms"][0]["signing"], "unsigned")
        self.assertIn("complete target-specific license", (output / "RELEASE_NOTES.md").read_text())
        for item in value["assets"]:
            self.assertEqual(github.file_record(output / item["name"]), item)

    def test_one_linux_archive_without_the_other_fails_assembly(self):
        self.ctx = context(release.PLATFORMS["linux"])
        for present in LINUX_TARGETS:
            with self.subTest(present=present):
                self.fresh_inputs(present)
                self.package(present)
                with self.assertRaises(FileNotFoundError):
                    self.assemble(name="assembled-" + present)
                self.assertFalse((self.root / ("assembled-" + present) / "release-manifest.json").exists())

    def test_archive_that_no_selection_promised_fails_assembly(self):
        for selection, extra in (("linux", MAC), ("macos", LINUX_ARM), ("macos", LINUX_X86)):
            with self.subTest(selection=selection, extra=extra):
                self.ctx = context(release.PLATFORMS[selection])
                self.fresh_inputs(selection + "-" + extra)
                for target in (*release.PLATFORMS[selection], extra):
                    self.package(target)
                output = "assembled-" + selection + "-" + extra
                with self.assertRaisesRegex(github.Error, "Unexpected platform artifacts"):
                    self.assemble(name=output)
                self.assertFalse((self.root / output / "release-manifest.json").exists())

    def test_linux_package_under_another_or_no_targets_name_is_refused(self):
        # One archive must never stand in for the other: its name is all that
        # tells the two Linux downloads apart.
        arm, x86 = (f"GitTurtle-0.1.0-{SHA[:12]}-linux-{machine}-release.tar.gz" for machine in ("aarch64", "x86_64"))
        undeclared = f"GitTurtle-0.1.0-{SHA[:12]}-linux-riscv64-release.tar.gz"
        for case, names in (("x86-64 under the AArch64 name", {LINUX_ARM: arm, LINUX_X86: arm}),
                            ("both names swapped", {LINUX_ARM: x86, LINUX_X86: arm}),
                            ("an undeclared label", {LINUX_ARM: arm, LINUX_X86: undeclared})):
            with self.subTest(case=case):
                self.fresh_inputs(case.replace(" ", "-"))
                for target, name in names.items():
                    self.package(target, name=name)
                output = "assembled-" + case.replace(" ", "-")
                with self.assertRaisesRegex(github.Error, "not named for its target"):
                    self.assemble(name=output)
                self.assertFalse((self.root / output / "release-manifest.json").exists())

    def test_two_packages_with_one_asset_name_are_refused_not_overwritten(self):
        # Reachable only if two targets ever derive one name: here, one shared label.
        with patch.dict(release.packages.TARGETS, {LINUX_ARM: release.packages.TARGETS[LINUX_X86]}):
            self.linux()
            with self.assertRaisesRegex(github.Error, "same release asset name"):
                self.assemble()
        self.assertFalse((self.root / "assembled" / "release-manifest.json").exists())

    def test_notarized_archive_keeps_the_signing_name_but_never_another_targets(self):
        self.ctx = context((MAC,), signed=True)
        self.package(MAC, signing="developer-id-notarized", name=f"GitTurtle-0.1.0-{MAC}-{SHA[:12]}-notarized.zip")
        self.assertTrue((self.assemble() / f"GitTurtle-0.1.0-{MAC}-{SHA[:12]}-notarized.zip").is_file())
        self.fresh_inputs("linux-name")
        self.package(MAC, signing="developer-id-notarized", name=f"GitTurtle-0.1.0-{SHA[:12]}-linux-aarch64-release.tar.gz")
        with self.assertRaisesRegex(github.Error, "not named for its target"):
            self.assemble(name="assembled-linux-name")

    def test_missing_promised_macos_is_a_failure(self):
        self.ctx = context((*LINUX_TARGETS, MAC))
        self.linux()
        with self.assertRaises((OSError, github.Error)):
            self.assemble()

    def test_explicit_macos_ad_hoc_is_labelled_without_notarization_claim(self):
        self.ctx = context((MAC,))
        self.package(MAC)
        output = self.assemble()
        value = json.loads((output / "release-manifest.json").read_bytes())
        self.assertEqual(value["platforms"][0]["signing"], "ad-hoc")
        self.assertIn("not Developer ID signed or notarized", (output / "RELEASE_NOTES.md").read_text())

    def test_signed_fixture_requires_report_and_actual_final_manifest_digests(self):
        self.ctx = context((MAC,), signed=True)
        self.package(MAC, signing="developer-id-notarized")
        output = self.assemble()
        self.assertTrue((output / "macos-signing-result.json").is_file())
        self.assertEqual(json.loads((output / "release-manifest.json").read_bytes())["platforms"][0]["signing"],
                         "developer-id-notarized")

    def test_incomplete_licenses_block_assembly(self):
        self.linux(gaps=True)
        with self.assertRaises(release.package_id.PackageError):
            self.assemble()

    def test_changed_archive_fails_even_if_the_outer_manifest_still_exists(self):
        directory, _, _ = self.linux()
        archive = next(directory.glob("*.tar.gz"))
        archive.write_bytes(archive.read_bytes() + b"changed")
        with self.assertRaises(release.package_id.PackageError):
            self.assemble()

    def test_mixed_source_receipt_is_refused(self):
        directory, _, _ = self.linux()
        self.mutate_receipt(directory, lambda value: value["expected"].update(revision="d" * 40))
        with self.assertRaises(github.Error):
            self.assemble()

    def test_wrong_run_receipt_is_refused(self):
        directory, _, _ = self.linux()
        self.mutate_receipt(directory, lambda value: value["context"].update(run_id=456))
        with self.assertRaises(github.Error):
            self.assemble()

    def test_pr_artifact_cannot_masquerade_as_release_producer(self):
        directory, _, _ = self.linux()
        self.mutate_receipt(directory, lambda value: value.update(producer="pull_request"))
        with self.assertRaises(github.Error):
            self.assemble()

    def test_unexpected_extra_asset_is_refused(self):
        directory, _, _ = self.linux()
        (directory / "unexpected.txt").write_text("No wildcard publication")
        with self.assertRaises(github.Error):
            self.assemble()

    def test_archive_name_cannot_escape_download_directory(self):
        directory, _, _ = self.linux()
        self.mutate_receipt(directory, lambda value: value["expected"].update(archive="../another.tar.gz"))
        with self.assertRaises(github.Error):
            self.assemble()

    def test_missing_platform_check_or_unsigned_fallback_is_refused(self):
        directory, _, _ = self.linux()
        self.mutate_receipt(directory, lambda value: value.update(platform_checks="skipped"))
        with self.assertRaises(github.Error):
            self.assemble()

    def test_unsafe_archive_entry_is_refused_before_publication(self):
        directory, _, _ = self.linux()
        archive = next(directory.glob("*.tar.gz"))
        with tarfile.open(archive, "w:gz") as output:
            item = tarfile.TarInfo("../escape")
            item.size = 1
            output.addfile(item, io.BytesIO(b"x"))
        value = json.loads((directory / "receipt.json").read_bytes())
        value["expected"]["archive_sha256"] = release.package_id.digest(archive)
        outer_path = Path(str(archive) + ".manifest.json")
        outer = json.loads(outer_path.read_bytes())
        outer.update(archive_sha256=value["expected"]["archive_sha256"], archive_bytes=archive.stat().st_size)
        outer_path.write_bytes(github.canonical(outer))
        value["expected"]["manifest_sha256"] = release.package_id.digest(outer_path)
        (directory / "receipt.json").write_bytes(github.canonical(value))
        Path(str(archive) + ".sha256").write_text(f"{outer['archive_sha256']}  {archive.name}\n")
        with self.assertRaises(release.package_id.PackageError):
            self.assemble()
        self.assertFalse((directory / "escape").exists())

    def test_notarization_failure_cleanup_identity_and_certificate_are_required(self):
        ctx = context((MAC,), signed=True)
        _, package, _ = self.package(MAC, ctx=ctx, signing="developer-id-notarized")
        report = self.signing_report(ctx, package)
        mutations = [lambda r: r.update(status="failed"), lambda r: r.update(staple_validation="failed"),
            lambda r: r.update(credential_workspace_cleanup="failed"),
            lambda r: r["keychain_cleanup"].update(status="failed"),
            lambda r: r["notarization"].update(status="Rejected"),
            lambda r: r["notarization"]["log"].update(sha256="0" * 64),
            lambda r: r["signature"].update(certificate_sha1="0" * 40),
            lambda r: r["signature"].update(entitlements={"com.apple.security.get-task-allow": True}),
            lambda r: r.update(post_staple_executable_sha256="0" * 64)]
        for mutate in mutations:
            with self.subTest(mutation=mutations.index(mutate)):
                changed = copy.deepcopy(report)
                mutate(changed)
                with self.assertRaises(github.Error):
                    release.signing_report(changed, ctx, package)

    def test_finish_signing_preserves_signed_entries_and_hashes_the_repacked_archive(self):
        self.ctx = context((MAC,), signed=True)
        directory, original, binary = self.package(MAC, ctx=self.ctx)
        helper = self.root / "signing"
        helper.mkdir()
        app = binary.parents[2]
        signed_binary = binary.read_bytes() + b"simulated signature"
        binary.write_bytes(signed_binary)
        signed_package = {**original, "binary_sha256": release.package_id.digest(binary)}
        report = self.signing_report(self.ctx, signed_package)
        report["pre_sign_executable_sha256"] = original["binary_sha256"]
        archive = helper / "GitTurtle-0.1.0-aarch64-apple-darwin-aaaaaaaaaaaa-notarized.zip"
        with zipfile.ZipFile(archive, "w") as stream:
            for path in app.rglob("*"):
                if path.is_file():
                    stream.write(path, Path("GitTurtle.app") / path.relative_to(app))
            stream.writestr("__MACOSX/._GitTurtle.app", b"simulated AppleDouble metadata")
        report["final_archive"] = github.file_record(archive)
        release.write_json(helper / "signing-result.json", report)
        calls = []

        def native(command, **kwargs):
            calls.append([str(item) for item in command])
            if str(command[0]) == "/usr/bin/ditto":
                # Explicitly simulated native extraction for an already bounded
                # fabricated fixture; actual Apple signature/staple tests remain C4.
                with zipfile.ZipFile(command[3]) as stream:
                    for item in stream.infolist():
                        path = Path(command[4]) / item.filename
                        path.parent.mkdir(parents=True, exist_ok=True)
                        if not item.is_dir():
                            path.write_bytes(stream.read(item))
                            path.chmod(0o755 if (item.external_attr >> 16) & 0o111 else 0o644)
            return 0, "simulated"

        output = self.root / "signed-upload"
        args = types.SimpleNamespace(directory=directory, signing=helper, output=output)
        with patch.object(release, "local_identity"), patch.object(release.packages, "run", side_effect=native), \
                patch.object(release.package_id, "probe", return_value=original["compiled_identity"]):
            release.finish_signing(args, self.ctx)
        final = output / archive.name
        self.assertNotEqual(release.package_id.digest(final), report["final_archive"]["sha256"])
        final_report = json.loads((output / "signing-result.json").read_bytes())
        self.assertEqual(final_report["release_archive"], github.file_record(final))
        self.assertEqual(final_report["final_archive"], report["final_archive"])
        with zipfile.ZipFile(final) as stream:
            self.assertEqual(stream.read("GitTurtle.app/Contents/MacOS/gitturtle"), signed_binary)
            self.assertEqual(stream.read("__MACOSX/._GitTurtle.app"), b"simulated AppleDouble metadata")
            self.assertEqual(json.loads(stream.read("build-info.json"))["signing"], "developer-id-notarized")
        self.assertEqual(calls[-2][0], "codesign")
        self.assertEqual(calls[-1][:3], ["xcrun", "stapler", "validate"])
        self.assertEqual(json.loads((helper / "signing-result.json").read_bytes()), report)


class Preparation(Fixture):
    def prepare(self, target):
        """The real prepare step around a fabricated compiled executable and packager."""
        source = self.root / ("source-" + target)
        compiled = source / "target" / target / "release/gitturtle"
        compiled.parent.mkdir(parents=True)
        compiled.write_bytes(self.executable(target))
        directory = self.root / ("job-" + target)
        calls, built = [], []

        def packager(command, **_):
            # Each real packager builds for its own platform only; the Linux one
            # builds for the machine it runs on, which is this leg's runner.
            command = [str(item) for item in command]
            calls.append(command)
            executable = Path(command[command.index("--binary") + 1]).read_bytes()
            if Path(command[0]).name == "package-linux.sh":
                bundle = Path(command[-1])
                built.append(self.archive(bundle.parent, target, name=bundle.name + ".tar.gz", executable=executable)[1])
            else:
                archives = Path(command[command.index("--archive-dir") + 1])
                built.append(self.archive(archives, MAC, executable=executable)[1])
            return 0, ""

        with patch.object(release, "ROOT", source), patch.object(release, "local_identity"), \
                patch.object(release.packages, "run", side_effect=packager), \
                patch.object(release.packages, "platform_checks") as checks, \
                patch.object(release.package_id, "probe", side_effect=lambda binary: built[-1]["compiled_identity"]):
            release.prepare(types.SimpleNamespace(target=target, directory=directory), self.ctx)
        return directory / "upload", calls, checks

    def test_linux_release_is_two_archives_that_differ_only_by_target(self):
        self.ctx = context(release.PLATFORMS["linux"])
        archives = {LINUX_ARM: f"GitTurtle-0.1.0-{SHA[:12]}-linux-aarch64-release.tar.gz",
                    LINUX_X86: f"GitTurtle-0.1.0-{SHA[:12]}-linux-x86_64-release.tar.gz"}
        for target, archive in archives.items():
            upload, calls, checks = self.prepare(target)
            self.assertEqual([Path(call[0]).name for call in calls], ["package-linux.sh"])
            self.assertEqual(sorted(p.name for p in upload.iterdir()),
                             sorted([archive, archive + ".manifest.json", archive + ".sha256", "receipt.json"]))
            self.assertEqual(checks.call_args.args[2]["target"], target)
            shutil.copytree(upload, self.inputs / f"release-package-{target}-123-1")
        output = self.assemble()
        self.assertEqual(sorted(p.name for p in output.iterdir()), sorted(
            [name + suffix for name in archives.values() for suffix in ("", ".manifest.json", ".sha256")]
            + ["RELEASE_NOTES.md", "SHA256SUMS", "release-manifest.json"]))
        value = json.loads((output / "release-manifest.json").read_bytes())
        self.assertEqual([(item["target"], item["expected"]["archive"], item["signing"]) for item in value["platforms"]],
                         [(target, archive, "unsigned") for target, archive in archives.items()])
        notes, sums = (output / "RELEASE_NOTES.md").read_text(), (output / "SHA256SUMS").read_text()
        for target, archive in archives.items():
            self.assertIn(f"- `{archive}` — `{target}`, unsigned.\n", notes)
            self.assertIn(f"{release.package_id.digest(output / archive)}  {archive}\n", sums)

    def test_macos_keeps_its_own_packager(self):
        self.ctx = context((MAC,))
        upload, calls, checks = self.prepare(MAC)
        self.assertEqual([Path(call[0]).name for call in calls], ["package-macos.sh"])
        self.assertEqual(calls[0][calls[0].index("--archive-dir") + 1], str(upload))
        self.assertEqual(checks.call_args.args[2]["target"], MAC)


class XcodeSelection(unittest.TestCase):
    def test_selects_verified_build_even_when_newer_or_default_tools_exist(self):
        with tempfile.TemporaryDirectory() as directory:
            applications = Path(directory)
            for name in ("Xcode.app", "Xcode_26.2.app", "Xcode_26.3.app", "Xcode_26.6.app"):
                (applications / name / "Contents/Developer").mkdir(parents=True)
            output = applications / "job-environment"
            with patch.object(release.packages, "run", return_value=(0, "Xcode 26.3\nBuild version 17C529\n")) as probe:
                release.select_xcode(applications=applications, environ={"GITHUB_ENV": str(output)})
            selected = str(applications / "Xcode_26.3.app/Contents/Developer")
            self.assertEqual(output.read_text(), "DEVELOPER_DIR=" + selected + "\n")
            probe.assert_called_once_with(["/usr/bin/xcodebuild", "-version"],
                env={"GITHUB_ENV": str(output), "DEVELOPER_DIR": selected}, allowed=tuple(range(256)), timeout=20)

    def test_missing_pinned_installation_does_not_probe_other_versions(self):
        with tempfile.TemporaryDirectory() as directory:
            applications = Path(directory)
            for name in ("Xcode.app", "Xcode_26.2.app", "Xcode_26.6.app"):
                (applications / name / "Contents/Developer").mkdir(parents=True)
            with patch.object(release.packages, "run") as probe, self.assertRaisesRegex(github.Error, "no fallback"):
                release.select_xcode(applications=applications, environ={"GITHUB_ENV": str(applications / "output")})
            probe.assert_not_called()
            self.assertFalse((applications / "output").exists())

    def test_failed_mismatched_or_ambiguous_identity_never_exports_the_path(self):
        with tempfile.TemporaryDirectory() as directory:
            applications = Path(directory)
            (applications / "Xcode_26.3.app/Contents/Developer").mkdir(parents=True)
            output = applications / "output"
            output.write_text("EXISTING=value\n")
            for result in ((1, "Xcode 26.3\nBuild version 17C529\n"),
                           (0, "Xcode 26.6\nBuild version 17F113\n"),
                           (0, "Xcode 26.3\nBuild version changed\n"),
                           (0, "Xcode 26.3\n"), (0, "unexpected"),
                           (0, "Xcode 26.3\nBuild version 17C529\nXcode 26.6\n")):
                with self.subTest(result=result), patch.object(release.packages, "run", return_value=result), self.assertRaisesRegex(github.Error, "no fallback"):
                    release.select_xcode(applications=applications, environ={"GITHUB_ENV": str(applications / "output")})
                self.assertEqual(output.read_text(), "EXISTING=value\n")

    def test_quality_cli_selects_tools_without_a_release_context(self):
        with tempfile.TemporaryDirectory() as directory:
            applications = Path(directory)
            (applications / "Xcode_26.3.app/Contents/Developer").mkdir(parents=True)
            output = applications / "output"
            select = release.select_xcode
            with patch.dict(os.environ, {"GITHUB_ENV": str(output)}, clear=True), \
                    patch.object(sys, "argv", ["workflow.py", "select-xcode"]), \
                    patch.object(release, "select_xcode", side_effect=lambda: select(applications=applications)), \
                    patch.object(release, "context_from_env") as context_probe, \
                    patch.object(release.packages, "run", return_value=(0, "Xcode 26.3\nBuild version 17C529\n")):
                self.assertEqual(release.main(), 0)
            context_probe.assert_not_called()
            self.assertEqual(output.read_text(), "DEVELOPER_DIR=" + str(applications / "Xcode_26.3.app/Contents/Developer") + "\n")

    def test_other_cli_operations_still_refuse_absent_or_bad_release_context(self):
        operations = [*(("prepare", ["--target", target, "--directory", "unused"]) for target in (MAC, *LINUX_TARGETS)),
                      ("signing-input", ["--directory", "unused"]),
                      ("finish-signing", ["--directory", "unused", "--signing", "unused", "--output", "unused"]),
                      ("assemble", ["--directory", "unused", "--output", "unused"]),
                      ("activate-signing", []),
                      ("publish", ["--directory", "unused", "--report", "unused"])]
        for name, args in operations:
            for context in (None, "{}"):
                env = {} if context is None else {"RELEASE_CONTEXT": context}
                with self.subTest(command=name, context=context), patch.dict(os.environ, env, clear=True), \
                        patch.object(sys, "argv", ["workflow.py", name, *args]), \
                        patch.object(release, "api") as api, \
                        patch.object(release, "local_identity") as source, \
                        contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(release.main(), 2)
                api.assert_not_called()
                source.assert_not_called()



class Trust(unittest.TestCase):
    def preflight(self, selection, output, api=None):
        """The real preflight against the in-memory remote; the local source check
        is replaced by one that keeps exactly the platforms preflight asked for.
        """
        env = {**environment(context()), "GITHUB_OUTPUT": str(output),
               "RELEASE_PLATFORMS": selection, "RELEASE_MACOS_SIGNING": "ad-hoc",
               "RELEASE_QUALITY_RUN": "45", "RELEASE_PUBLISH": "false",
               "RELEASE_TAG": "v0.1.0", "RELEASE_VERSION": "0.1.0",
               "RELEASE_COMMIT": SHA, "RELEASE_TAG_OBJECT": TAG}

        def selected(_root, *, platforms, **_):
            return release.identity.ReleaseIdentity("v0.1.0", TAG, SHA, "0.1.0", tuple(sorted(platforms)), LOCK)

        with patch.dict(os.environ, env, clear=True), patch.object(release, "api", return_value=api or FakeGitHub()), \
                patch.object(release.identity, "verify_release_identity", side_effect=selected), \
                contextlib.redirect_stdout(io.StringIO()):
            release.preflight(None)
        return dict(line.split("=", 1) for line in output.read_text().splitlines())

    def test_preflight_emits_macos26_build_matrix_and_keeps_linux_independent(self):
        x86, arm, mac = ("x86_64-unknown-linux-gnu", "ubuntu-24.04"), ("aarch64-unknown-linux-gnu", "ubuntu-24.04-arm"), \
            ("aarch64-apple-darwin", "macos-26")
        for selection, legs in (("linux", [x86, arm]), ("macos", [mac]), ("linux,macos", [x86, arm, mac])):
            with self.subTest(selection=selection), tempfile.TemporaryDirectory() as directory:
                outputs = self.preflight(selection, Path(directory) / "job-output")
                self.assertEqual(json.loads(outputs["matrix"]),
                                 {"include": [{"target": target, "os": runner} for target, runner in legs]})
                self.assertEqual(outputs["linux"], str("linux" in selection).lower())
                self.assertEqual(outputs["macos"], str("macos" in selection).lower())
                ctx = json.loads(outputs["context"])
                self.assertEqual(ctx["identity"]["platforms"], sorted(target for target, _ in legs))
                self.assertEqual(release.context_from_env(environment(ctx)), ctx)

    def test_no_selection_names_one_linux_target(self):
        options = re.search(r"^      platforms:\n(?:        .*\n)*?        options: \[(.*)\]$",
                            (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8"), re.M)
        self.assertEqual(["".join(option) for option in re.findall(r"'([^']*)'|([^,'\s]+)", options.group(1))],
                         ["linux", "macos", "linux,macos"])
        self.assertEqual(list(release.PLATFORMS), ["linux", "macos", "linux,macos"])
        for selection in (LINUX_X86, LINUX_ARM, "linux-x86_64", "linux-aarch64", "linux,linux", "macos,linux", ""):
            with self.subTest(selection=selection), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "job-output"
                with self.assertRaisesRegex(github.Error, "explicit supported platform set"):
                    self.preflight(selection, output)
                self.assertFalse(output.exists())

    def test_context_promises_only_a_set_a_dispatch_can_select(self):
        for platforms in ((LINUX_X86,), (LINUX_ARM,), (LINUX_X86, MAC), (LINUX_ARM, MAC)):
            with self.subTest(platforms=platforms), self.assertRaisesRegex(github.Error, "no selection offers"):
                release.context_from_env(environment(context(platforms)))
        for platforms in release.PLATFORMS.values():
            with self.subTest(platforms=platforms):
                ctx = context(platforms)
                self.assertEqual(release.context_from_env(environment(ctx)), ctx)

    def test_preflight_refuses_quality_without_a_successful_arm_linux_release_job(self):
        # "missing" is every Quality run from before Quality built this target.
        for selection in release.PLATFORMS:
            for variant in ("missing", "skipped", "failure", "cancelled", "duplicate", "other-source"):
                with self.subTest(selection=selection, variant=variant), tempfile.TemporaryDirectory() as directory:
                    api = FakeGitHub()
                    job = next(item for item in api.jobs if item["name"] == ARM_RELEASE_JOB)
                    if variant == "missing":
                        api.jobs.remove(job)
                    elif variant == "duplicate":
                        api.jobs.append(copy.deepcopy(job))
                    elif variant == "other-source":
                        job["head_sha"] = "d" * 40
                    else:
                        job["conclusion"] = variant
                    output = Path(directory) / "job-output"
                    with self.assertRaisesRegex(github.Error, ARM_RELEASE_JOB):
                        self.preflight(selection, output, api)
                    self.assertFalse(output.exists())

    def test_only_exact_main_dispatch_and_workflow_revision_are_accepted(self):
        ctx = context()
        good = environment(ctx)
        self.assertEqual(release.context_from_env(good), ctx)
        for key, value in {"GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": "someone/fork",
            "GITHUB_REF": "refs/tags/v0.1.0", "GITHUB_SHA": "d" * 40,
            "GITHUB_WORKFLOW_SHA": "d" * 40, "GITHUB_WORKFLOW_REF": github.REPOSITORY + "/.github/workflows/other.yml@refs/heads/main",
            "GITHUB_RUN_ID": "124", "GITHUB_RUN_ATTEMPT": "2"}.items():
            with self.subTest(key=key), self.assertRaises(github.Error):
                release.context_from_env({**good, key: value})

    def test_approval_context_must_match_the_exact_selected_inputs(self):
        ctx = context()
        ctx["identity"]["version"] = "1.2.3"
        with self.assertRaises(github.Error):
            release.context_from_env(environment(ctx))

    def test_protected_environment_requires_the_specific_approval_digest(self):
        ctx = context()
        for env in ({}, {"GITTURTLE_RELEASE_CONTEXT": "enabled"}, {"GITTURTLE_RELEASE_CONTEXT": "0" * 64}):
            with self.assertRaises(github.Error):
                github.require_activation(ctx, env, "GITTURTLE_RELEASE_CONTEXT")
        github.require_activation(ctx, {"GITTURTLE_RELEASE_CONTEXT": ctx["approval_sha256"]}, "GITTURTLE_RELEASE_CONTEXT")

    def test_remote_tag_object_and_peeled_source_must_both_match(self):
        api = FakeGitHub()
        github.remote_identity(api, context()["identity"])
        for field in ("main", "tag", "peeled"):
            with self.subTest(field=field), self.assertRaises(github.Error):
                api = FakeGitHub()
                setattr(api, field, "d" * 40)
                github.remote_identity(api, context()["identity"])

    def test_quality_requires_trusted_source_and_all_actual_platform_jobs(self):
        api = FakeGitHub()
        self.assertEqual(github.quality_evidence(api, SHA, 45), context()["quality"])
        mutations = [lambda a: a.quality.update(event="pull_request"),
            lambda a: a.quality.update(head_sha="d" * 40), lambda a: a.quality.update(path="other.yml"),
            lambda a: a.quality.update(head_repository={"full_name": "fork/repo"}),
            lambda a: a.jobs.pop(), lambda a: a.jobs[2].update(conclusion="skipped"),
            lambda a: a.jobs[0].update(conclusion="failure"), lambda a: a.jobs.append(a.jobs[0])]
        for mutate in mutations:
            with self.subTest(mutation=mutations.index(mutate)), self.assertRaises(github.Error):
                api = FakeGitHub()
                mutate(api)
                github.quality_evidence(api, SHA, 45)

    def test_required_jobs_are_names_the_quality_workflow_produces(self):
        # A renamed Quality job would otherwise leave every release preflight
        # requiring a job that no longer runs.
        body = (ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8").split("\njobs:\n", 1)[1]
        produced = set()
        for block in re.split(r"^  [a-z][a-z0-9-]*:\n", body, flags=re.M)[1:]:
            name = re.search(r"^    name: (.+)$", block, re.M)
            if not name:
                continue
            # Every matrix axis in the name, each an inline list or, for the
            # release legs, one `include` entry per value.
            names = [name.group(1)]
            for axis in dict.fromkeys(re.findall(r"\$\{\{ matrix\.([a-z-]+) \}\}", name.group(1))):
                listed = re.search(rf"^        {axis}: \[(.*)\]$", block, re.M)
                values = ([value.strip() for value in listed.group(1).split(",")] if listed
                          else re.findall(rf"^          - {axis}: (\S+)$", block, re.M))
                self.assertTrue(values, name.group(1))
                names = [item.replace(f"${{{{ matrix.{axis} }}}}", value) for item in names for value in values]
            self.assertNotIn("${{", "".join(names))
            produced.update(names)
        required = set(github.quality_evidence(FakeGitHub(), SHA, 45)["required_jobs"])
        self.assertLessEqual(required, produced)
        # Both shards of each required platform, so no release counts half a suite.
        tests = sorted(name for name in produced if name.startswith("Rust tests · ") and "archlinux" not in name)
        self.assertEqual(tests, sorted(name for name in required if name.startswith("Rust tests · ")))
        self.assertEqual(len(tests), 4)

    def test_every_test_shard_is_required_and_no_unsharded_job_stands_in_for_one(self):
        shards = [name for name in REQUIRED if name.startswith("Rust tests · ")]
        self.assertEqual(shards, ["Rust tests · macos-15 · 1/2", "Rust tests · macos-15 · 2/2",
                                  "Rust tests · ubuntu-24.04 · 1/2", "Rust tests · ubuntu-24.04 · 2/2"])
        for shard in shards:
            for variant in ("missing", "duplicate", "unsharded", "skipped", "failure", "cancelled"):
                with self.subTest(shard=shard, variant=variant):
                    api = FakeGitHub()
                    job = next(item for item in api.jobs if item["name"] == shard)
                    if variant == "missing":
                        api.jobs.remove(job)
                    elif variant == "duplicate":
                        api.jobs.append(copy.deepcopy(job))
                    elif variant == "unsharded":
                        # The name every run from before the split gave the whole platform's job.
                        job["name"] = shard.rsplit(" · ", 1)[0]
                    else:
                        job["conclusion"] = variant
                    with self.assertRaisesRegex(github.Error, re.escape(shard)):
                        github.quality_evidence(api, SHA, 45)

    def test_macos26_package_job_cannot_be_replaced_by_old_or_unsuccessful_checks(self):
        for variant in ("old-runner", "missing", "duplicate", "skipped", "failure", "cancelled"):
            with self.subTest(variant=variant):
                api = FakeGitHub()
                job = next(item for item in api.jobs if item["name"] == "Rust release · macos-26")
                if variant == "old-runner":
                    job["name"] = "Rust release · macos-15"
                elif variant == "missing":
                    api.jobs.remove(job)
                elif variant == "duplicate":
                    api.jobs.append(copy.deepcopy(job))
                else:
                    job["conclusion"] = variant
                with self.assertRaisesRegex(github.Error, "Rust release · macos-26"):
                    github.quality_evidence(api, SHA, 45)

    def test_changed_quality_attempt_requires_new_context(self):
        api = FakeGitHub()
        api.quality["run_attempt"] = 2
        with self.assertRaises(github.Error):
            github.verify_checks(api, context())


class Publication(Fixture):
    # Reuse real archive assembly as publication input.
    def assembled(self):
        self.linux()
        return self.assemble()

    def publish(self, api, output, name="result.json"):
        return github.publish(api, self.ctx, output, self.root / name)

    def test_publish_creates_draft_verifies_bytes_and_then_publishes(self):
        output = self.assembled()
        api = FakeGitHub()
        result = self.publish(api, output)
        self.assertEqual(result["status"], "published")
        self.assertFalse(api.releases[0]["draft"])
        self.assertEqual(sum(call[0] == "POST" for call in api.calls), 1)
        self.assertEqual(sum(call[0] == "PATCH" for call in api.calls), 1)
        self.assertTrue(all(op["state"] == "verified" for op in result["operations"]))

    def test_linux_publication_uploads_both_archives_with_their_checksums_and_manifests(self):
        output = self.assembled()
        api = FakeGitHub()
        self.publish(api, output)
        archives = [f"GitTurtle-0.1.0-{SHA[:12]}-linux-{machine}-release.tar.gz" for machine in ("aarch64", "x86_64")]
        self.assertEqual(sorted(item["name"] for item in api.assets), sorted(
            [name + suffix for name in archives for suffix in ("", ".manifest.json", ".sha256")]
            + ["RELEASE_NOTES.md", "SHA256SUMS", "release-manifest.json"]))

    def test_identical_retry_reads_all_assets_without_any_remote_write(self):
        output = self.assembled()
        api = FakeGitHub()
        self.publish(api, output)
        api.calls.clear()
        result = self.publish(api, output, "retry.json")
        self.assertEqual(result["status"], "already-published-identical")
        self.assertFalse(any(call[0] in ("POST", "PATCH", "UPLOAD", "DELETE") for call in api.calls))

    def test_different_published_bytes_are_never_overwritten(self):
        output = self.assembled()
        api = FakeGitHub()
        self.publish(api, output)
        api.contents[1] += b"tampered"
        api.calls.clear()
        with self.assertRaises(github.Error):
            self.publish(api, output, "collision.json")
        self.assertFalse(any(call[0] in ("POST", "PATCH", "UPLOAD", "DELETE") for call in api.calls))

    def test_partial_draft_is_not_automatically_resumed(self):
        output = self.assembled()
        api = FakeGitHub()
        api.fail = "upload"
        with self.assertRaises(github.Error):
            self.publish(api, output)
        report = json.loads((self.root / "result.json").read_bytes())
        self.assertEqual(report["status"], "upload-uncertain")
        self.assertTrue(report["inspection_required"])
        self.assertEqual(len(api.releases), 1)
        api.fail = None
        api.calls.clear()
        with self.assertRaisesRegex(github.Error, "partial release"):
            self.publish(api, output, "retry.json")
        self.assertFalse(any(call[0] in ("POST", "PATCH", "UPLOAD", "DELETE") for call in api.calls))

    def test_uncertain_create_and_publish_are_recorded_without_retry(self):
        output = self.assembled()
        for failure, state in (("create", "creating-draft-uncertain"), ("publish", "publishing-uncertain")):
            with self.subTest(failure=failure):
                api = FakeGitHub()
                api.fail = failure
                with self.assertRaises(github.Error):
                    self.publish(api, output, failure + ".json")
                report = json.loads((self.root / (failure + ".json")).read_bytes())
                self.assertEqual(report["status"], state)
                self.assertTrue(report["inspection_required"])
                self.assertEqual(sum(call[0] == "POST" for call in api.calls), 1)
                self.assertLessEqual(sum(call[0] == "PATCH" for call in api.calls), 1)
                self.assertFalse(any(call[0] == "DELETE" for call in api.calls))

    def test_modified_local_asset_fails_before_any_network_operation(self):
        output = self.assembled()
        (output / "RELEASE_NOTES.md").write_text("Changed after review")
        api = FakeGitHub()
        with self.assertRaises(github.Error):
            self.publish(api, output)
        self.assertEqual(api.calls, [])

    def test_incomplete_existing_asset_set_and_metadata_mismatch_are_refused(self):
        output = self.assembled()
        api = FakeGitHub()
        self.publish(api, output)
        api.assets.pop()
        api.calls.clear()
        with self.assertRaises(github.Error):
            self.publish(api, output, "missing.json")
        self.assertFalse(any(call[0] in ("POST", "PATCH", "UPLOAD") for call in api.calls))
        api.releases[0]["target_commitish"] = "other"
        with self.assertRaises(github.Error):
            self.publish(api, output, "metadata.json")

    def test_moved_remote_tag_prevents_all_publication_writes(self):
        output = self.assembled()
        api = FakeGitHub()
        api.tag = "d" * 40
        with self.assertRaises(github.Error):
            self.publish(api, output)
        self.assertFalse(any(call[0] in ("POST", "PATCH", "UPLOAD") for call in api.calls))


class Transport(unittest.TestCase):
    def test_download_redirect_does_not_forward_token(self):
        api = github.GitHub("secret-test-token")
        calls = []
        data = b"actual downloaded fixture"

        def open_request(request, timeout):
            calls.append(request)
            if len(calls) == 1:
                raise urllib.error.HTTPError(request.full_url, 302, "redirect",
                    {"Location": "https://release-assets.githubusercontent.com/path?signature=opaque"}, None)
            return io.BytesIO(data)

        api.opener.open = open_request
        self.assertEqual(api.download_digest({"id": 1, "size": len(data)}), hashlib.sha256(data).hexdigest())
        self.assertEqual(calls[0].get_header("Authorization"), "Bearer secret-test-token")
        self.assertIsNone(calls[1].get_header("Authorization"))

    def test_download_refuses_untrusted_redirect_and_wrong_length(self):
        api = github.GitHub("secret-test-token")
        for location in ("http://release-assets.githubusercontent.com/path", "https://example.invalid/file",
                         "https://release-assets.githubusercontent.com.evil.invalid/path"):
            with self.subTest(location=location):
                api.opener.open = lambda req, timeout: (_ for _ in ()).throw(urllib.error.HTTPError(req.full_url, 302,
                    "redirect", {"Location": location}, None))
                with self.assertRaises(github.Error):
                    api.download_digest({"id": 1, "size": 3})
        api.opener.open = lambda req, timeout: io.BytesIO(b"short")
        with self.assertRaises(github.Error):
            api.download_digest({"id": 1, "size": 9})

    def test_failure_message_never_includes_response_or_token(self):
        api = github.GitHub("secret-test-token")
        api.opener.open = lambda req, timeout: (_ for _ in ()).throw(urllib.error.HTTPError(req.full_url, 403,
            "body echoes secret-test-token", {}, io.BytesIO(b"private service body")))
        with self.assertRaises(github.Error) as failure:
            api.request("POST", "/repos/FernandoX7/GitTurtle/releases", {})
        self.assertNotIn("secret-test-token", str(failure.exception))
        self.assertNotIn("private", str(failure.exception))


if __name__ == "__main__":
    unittest.main()
