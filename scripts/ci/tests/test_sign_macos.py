"""Simulated signing/service responses, never evidence of Apple acceptance.

Real filesystem/subprocess assertions cover preservation and failure cleanup;
all macOS command results below are explicitly simulated on any host.
"""

from argparse import Namespace
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile


SCRIPT = Path(__file__).resolve().parents[2] / "release/sign-macos.py"
SPEC = importlib.util.spec_from_file_location("gitturtle_sign_macos", SCRIPT)
signing = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(signing)
SUBMISSION = "cc93306b-7c8c-4908-ba79-529969c77355"
TEAM = "01234ABCDE"
IDENTITY = "A" * 40


class SimulatedAppleCommands:
    """A fake protocol peer. No command in this class calls macOS or Apple."""

    def __init__(self, report, *, failure=None, identity=None, architecture="arm64",
                 response=None, log_changes=None, signature_changes=None,
                 entitlements=None, cancel=None, nonzero_response=None, corrupt_copy=False,
                 actual_cancel=False, imported_identity=None, submission_response=None):
        self.report = report
        self.calls = []
        self.failure = failure
        self.identity = identity
        self.architecture = architecture
        self.response = response
        self.log_changes = log_changes or {}
        self.signature_changes = signature_changes or {}
        self.entitlements = entitlements or {}
        self.cancel = cancel
        self.nonzero_response = nonzero_response
        self.corrupt_copy = corrupt_copy
        self.actual_cancel = actual_cancel
        self.imported_identity = imported_identity
        self.submission_response = submission_response
        self.private = None
        self.submitted = None

    def __call__(self, label, argv, timeout=60):
        self.calls.append((label, argv, timeout))
        event = {"step": label, "status": "passed"}
        self.report["commands"].append(event)
        if label in (self.failure or []):
            event["status"] = "failed"
            raise signing.SigningError(f"{label} simulated failure")
        if label == self.cancel:
            event["status"] = "cancelled"
            if self.actual_cancel:
                os.kill(os.getpid(), signal.SIGTERM)
            raise signing.Cancelled("Simulated cancellation")
        if label == "copy-trusted-app":
            source, target = map(Path, argv[-2:])
            self.private = target.parent
            shutil.copytree(source, target)
            if self.corrupt_copy:
                (target / "Contents/Resources/Assets.car").write_bytes(b"unexpected-replacement")
        elif label == "verify-compiled-identity":
            return json.dumps(self.identity or {
                "application": "GitTurtle", "source_revision": "b" * 40,
                "version": "0.1.0", "source_tree": "clean",
                "target": "aarch64-apple-darwin", "profile": "release",
            }).encode(), b""
        elif label == "verify-mach-o-architecture":
            return (self.architecture + "\n").encode(), b""
        elif label == "read-keychain-search-list":
            return b'    "/Users/test/Library/Keychains/login.keychain-db"\n', b""
        elif label == "create-signing-keychain":
            Path(argv[-1]).write_bytes(b"simulated-keychain")
        elif label == "delete-signing-keychain":
            Path(argv[-1]).unlink(missing_ok=True)
        elif label == "verify-imported-identity":
            if self.imported_identity is not None:
                return self.imported_identity, b""
            return f'1) {IDENTITY} "Developer ID Application: Fixture ({TEAM})"\n1 valid identities found\n'.encode(), b""
        elif label == "sign-app":
            executable = Path(argv[-1]) / "Contents/MacOS/gitturtle"
            executable.write_bytes(executable.read_bytes() + b"SIMULATED-SIGNATURE")
        elif label == "inspect-signature":
            fields = {"Identifier": "com.gitturtle.desktop", "TeamIdentifier": TEAM,
                      "Authority": f"Developer ID Application: Fixture ({TEAM})",
                      "Timestamp": "Sep 15, 2026 at 12:00:00 PM",
                      "CodeDirectory": "v=20500 size=500 flags=0x10000(runtime) hashes=10+2 location=embedded"}
            fields.update(self.signature_changes)
            return b"", "\n".join(f"{key}={value}" for key, value in fields.items() if value is not None).encode()
        elif label == "inspect-entitlements":
            return plistlib.dumps(self.entitlements), b""
        elif label in {"archive-notarization-input", "archive-stapled-app"}:
            app, output = map(Path, argv[-2:])
            with zipfile.ZipFile(output, "x") as archive:
                for path in app.rglob("*"):
                    if path.is_file():
                        archive.write(path, Path("GitTurtle.app") / path.relative_to(app))
            if label == "archive-notarization-input":
                self.submitted = output
        elif label == "submit-notarization":
            if self.nonzero_response == label:
                raise signing.CommandFailure(label, 1, json.dumps({"id": SUBMISSION}).encode())
            return json.dumps(self.submission_response or {"id": SUBMISSION}).encode(), b""
        elif label == "wait-for-notarization":
            saved = json.loads((self.submitted.parent / "signing-result.json").read_text())
            if saved["notarization"] != {"id": SUBMISSION, "status": "submitted"}:
                raise AssertionError("Submission identity must persist before waiting")
            data = json.dumps(self.response or {"id": SUBMISSION, "status": "Accepted"}).encode()
            if self.nonzero_response == label:
                raise signing.CommandFailure(label, 1, data)
            return data, b""
        elif label == "retrieve-notarization-log":
            data = {"jobId": SUBMISSION, "status": "Accepted", "statusCode": 0,
                    "sha256": signing.digest(self.submitted), "issues": None}
            data.update(self.log_changes)
            Path(argv[-1]).write_text(json.dumps(data))
        elif label == "staple-ticket":
            (Path(argv[-1]) / "Contents/CodeResources").write_bytes(b"SIMULATED-STAPLE")
        return b"", b""


class SigningSimulationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        # macOS's temporary directory can live beneath /var -> /private/var.
        # Resolve only our newly owned fixture; production must still reject
        # caller-supplied aliases before normalizing input/output paths.
        self.root = Path(self.temporary.name).resolve()
        self.app = self.root / "Input.app"
        resources = self.app / "Contents/Resources"
        resources.mkdir(parents=True)
        executable = self.app / "Contents/MacOS/gitturtle"
        executable.parent.mkdir()
        executable.write_bytes(b"\xcf\xfa\xed\xfeSIMULATED-MACHO")
        executable.chmod(0o755)
        for resource in ("AppIcon.icns", "Assets.car"):
            (resources / resource).write_bytes(b"SIMULATED-RESOURCE")
        plist = {"CFBundleIdentifier": "com.gitturtle.desktop", "CFBundleExecutable": "gitturtle",
                 "CFBundlePackageType": "APPL", "CFBundleShortVersionString": "0.1.0",
                 "CFBundleVersion": "0.1.0"}
        (self.app / "Contents/Info.plist").write_bytes(plistlib.dumps(plist))
        self.args = Namespace(app=self.app, output=self.root / "signed", source="b" * 40,
                              version="0.1.0", target="aarch64-apple-darwin",
                              executable_sha256=signing.digest(executable),
                              identity=IDENTITY, team_id=TEAM, notary_timeout_seconds=60)
        self.environ = dict(zip(signing.CREDENTIALS, (
            base64.b64encode(b"PRIVATE-IMPORT-BUNDLE").decode(), "SECRET-PASSWORD",
            base64.b64encode(b"-----BEGIN PRIVATE KEY-----\nSECRET-API-KEY\n-----END PRIVATE KEY-----").decode(),
            "123456ABCD", "1469eaa7-a7ef-4ae9-9385-c1735e4dd579")))
        self.commands = None
        self.behavior = {}

    def run_sign(self):
        def factory(report):
            self.commands = SimulatedAppleCommands(report, **self.behavior)
            return self.commands
        return signing.sign(self.args, command_factory=factory, environ=self.environ, platform="darwin")

    def report(self):
        return json.loads((self.args.output / "signing-result.json").read_text())

    def assert_failed_without_final(self):
        self.assertEqual(self.report()["status"], "failed")
        self.assertFalse(list(self.args.output.glob("*-notarized.zip")))
        self.assertFalse((self.args.output / ".final.zip.partial").exists())

    def assert_cleanup(self):
        labels = [call[0] for call in self.commands.calls]
        self.assertIn("restore-keychain-search-list", labels)
        self.assertIn("delete-signing-keychain", labels)
        self.assertFalse(self.commands.private.exists())

    def test_success_records_exact_final_bytes_but_retains_native_requirement(self):
        original = {str(path.relative_to(self.app)): path.read_bytes()
                    for path in self.app.rglob("*") if path.is_file()}
        report = self.run_sign()
        self.assertEqual(report["status"], "signed-notarized-stapled")
        self.assertEqual(report["downloaded_package_gatekeeper_and_launch"], "pending")
        archive = self.args.output / report["final_archive"]["name"]
        self.assertEqual(signing.digest(archive), report["final_archive"]["sha256"])
        self.assertNotEqual(report["submitted_archive_sha256"], report["final_archive"]["sha256"])
        with zipfile.ZipFile(archive) as shipped:
            self.assertIn("GitTurtle.app/Contents/CodeResources", shipped.namelist())
            binary = shipped.read("GitTurtle.app/Contents/MacOS/gitturtle")
            self.assertEqual(hashlib.sha256(binary).hexdigest(), report["post_staple_executable_sha256"])
        self.assertEqual(original, {str(path.relative_to(self.app)): path.read_bytes()
                                  for path in self.app.rglob("*") if path.is_file()})
        self.assertNotEqual(report["pre_sign_executable_sha256"], report["signed_executable_sha256"])
        self.assertEqual((self.args.output / "SHA256SUMS").read_text(),
                         f"{signing.digest(archive)}  {archive.name}\n")
        self.assert_cleanup()
        self.assertEqual(sorted(path.name for path in self.args.output.iterdir()),
                         sorted([archive.name, "SHA256SUMS", "signing-result.json"]))
        command = next(argv for label, argv, _ in self.commands.calls if label == "sign-app")
        self.assertNotIn("--deep", command)
        self.assertNotEqual(command[command.index("--sign") + 1], "-")
        self.assertNotIn("--preserve-metadata", command)

    def test_missing_credentials_fail_before_commands_and_output(self):
        for name in signing.CREDENTIALS:
            saved = self.environ.pop(name)
            with self.subTest(name=name), self.assertRaisesRegex(signing.SigningError, "missing"):
                self.run_sign()
            self.environ[name] = saved
        self.assertIsNone(self.commands)
        self.assertFalse(self.args.output.exists())

    def test_malformed_credentials_fail_closed(self):
        for name, value in ((signing.CREDENTIALS[0], "***"), (signing.CREDENTIALS[2], "YWJj"),
                            (signing.CREDENTIALS[3], "bad"), (signing.CREDENTIALS[4], "not-a-uuid")):
            original = self.environ[name]
            self.environ[name] = value
            with self.subTest(name=name), self.assertRaises(signing.SigningError):
                self.run_sign()
            self.environ[name] = original

    def test_wrong_pre_sign_digest_does_not_execute_the_input(self):
        self.args.executable_sha256 = "0" * 64
        with self.assertRaisesRegex(signing.SigningError, "digest mismatch"):
            self.run_sign()
        self.assertIsNone(self.commands)

    def test_copy_resource_mismatch_fails_before_credentials_or_execution(self):
        self.behavior = {"corrupt_copy": True}
        with self.assertRaisesRegex(signing.SigningError, "copied content changed"):
            self.run_sign()
        self.assertEqual([call[0] for call in self.commands.calls], ["copy-trusted-app"])
        self.assertFalse(self.commands.private.exists())
        self.assert_failed_without_final()

    def test_input_replaced_after_initial_digest_refuses_before_executable_invocation(self):
        snapshot = signing.content_snapshot
        executable = self.app / "Contents/MacOS/gitturtle"
        original_digest = self.args.executable_sha256
        replaced = False

        def replace_before_snapshot(app):
            nonlocal replaced
            if not replaced:
                executable.write_bytes(executable.read_bytes() + b"REPLACEMENT-AFTER-DIGEST")
                replaced = True
            return snapshot(app)

        with patch.object(signing, "content_snapshot", side_effect=replace_before_snapshot):
            with self.assertRaisesRegex(signing.SigningError, "Copied pre-sign executable digest mismatch"):
                self.run_sign()
        self.assertTrue(replaced)
        self.assertNotEqual(signing.digest(executable), original_digest)
        self.assertEqual(self.report()["pre_sign_executable_sha256"], original_digest)
        self.assertEqual([call[0] for call in self.commands.calls], ["copy-trusted-app"])
        self.assertFalse(self.commands.private.exists())
        self.assert_failed_without_final()

    def test_output_alias_inside_input_is_refused(self):
        self.args.output = self.root / "unused" / ".." / "Input.app" / "output"
        with self.assertRaisesRegex(signing.SigningError, "inside the input"):
            self.run_sign()
        self.assertFalse((self.app / "output").exists())

    def test_symlinked_input_and_output_ancestors_refuse_before_commands(self):
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        for field, name in (("app", "Input.app"), ("output", "signed")):
            with self.subTest(field=field), patch.object(self.args, field, alias / name):
                with self.assertRaisesRegex(signing.SigningError, "Symlinked"):
                    self.run_sign()
            self.assertIsNone(self.commands)
            self.assertFalse((self.root / "signed").exists())
            self.assertEqual(signing.digest(self.app / "Contents/MacOS/gitturtle"),
                             self.args.executable_sha256)

    def test_compiled_identity_and_architecture_must_match(self):
        base = {"application": "GitTurtle", "source_revision": "b" * 40,
                "version": "0.1.0", "source_tree": "clean", "profile": "release",
                "target": "aarch64-apple-darwin"}
        for key, value in (("source_revision", "c" * 40), ("source_tree", "modified"),
                           ("source_tree", "unknown"), ("profile", "debug"),
                           ("version", "0.2.0"), ("application", "Other"),
                           ("target", "x86_64-apple-darwin")):
            self.behavior = {"identity": {**base, key: value}}
            with self.subTest(key=key, value=value), self.assertRaisesRegex(signing.SigningError, "Compiled identity"):
                self.run_sign()
            self.assertNotIn("create-signing-keychain", [call[0] for call in self.commands.calls])
            shutil.rmtree(self.args.output)
        for value in ("x86_64", "arm64 x86_64", "", "arm64 garbage"):
            self.behavior = {"architecture": value}
            with self.subTest(architecture=value), self.assertRaisesRegex(signing.SigningError, "architecture"):
                self.run_sign()
            shutil.rmtree(self.args.output)

    def test_existing_or_dangling_output_is_never_replaced(self):
        self.args.output.mkdir()
        marker = self.args.output / "keep"
        marker.write_text("keep")
        with self.assertRaisesRegex(signing.SigningError, "already exists"):
            self.run_sign()
        self.assertEqual(marker.read_text(), "keep")
        marker.unlink()
        self.args.output.rmdir()
        self.args.output.symlink_to(self.root / "absent")
        with self.assertRaisesRegex(signing.SigningError, "already exists"):
            self.run_sign()
        self.assertTrue(self.args.output.is_symlink())

    def test_unsupported_nested_code_symlinks_and_special_files(self):
        resource = self.app / "Contents/Resources/extra"
        variants = ("symlink", "fifo", "script", "macho-without-executable-bit", "framework")
        for variant in variants:
            if variant == "symlink":
                resource.symlink_to(self.root / "unrelated")
            elif variant == "fifo":
                os.mkfifo(resource)
            elif variant == "script":
                resource.write_text("#!/bin/sh\n")
                resource.chmod(0o755)
            elif variant == "framework":
                resource = resource.with_suffix(".framework")
                resource.mkdir()
            else:
                resource.write_bytes(b"\xcf\xfa\xed\xfehidden-executable")
            with self.subTest(variant=variant), self.assertRaises(signing.SigningError):
                self.run_sign()
            self.assertIsNone(self.commands)
            if resource.is_dir():
                resource.rmdir()
            else:
                resource.unlink()

    def test_invalid_plist_or_release_options_refuse(self):
        for key, value in (("source", "HEAD"), ("version", "0.1.0-beta"),
                           ("identity", "Developer ID Application"), ("notary_timeout_seconds", 10000)):
            previous = getattr(self.args, key)
            setattr(self.args, key, value)
            with self.subTest(key=key), self.assertRaises(signing.SigningError):
                self.run_sign()
            setattr(self.args, key, previous)
        path = self.app / "Contents/Info.plist"
        value = plistlib.loads(path.read_bytes())
        value["CFBundleIdentifier"] = "other.application"
        path.write_bytes(plistlib.dumps(value))
        with self.assertRaisesRegex(signing.SigningError, "property list"):
            self.run_sign()

    def test_every_keychain_and_service_failure_cleans_and_never_succeeds(self):
        labels = ("create-signing-keychain", "configure-signing-keychain", "unlock-signing-keychain",
                  "import-signing-identity", "authorize-signing-key", "verify-imported-identity",
                  "sign-app", "verify-strict-signature", "inspect-signature", "inspect-entitlements",
                  "archive-notarization-input", "submit-notarization", "wait-for-notarization",
                  "retrieve-notarization-log", "staple-ticket", "validate-stapled-ticket", "archive-stapled-app")
        for label in labels:
            self.behavior = {"failure": [label]}
            with self.subTest(label=label), self.assertRaises(signing.SigningError):
                self.run_sign()
            self.assert_cleanup()
            self.assert_failed_without_final()
            report = self.report()
            if label in {"wait-for-notarization", "retrieve-notarization-log", "staple-ticket", "validate-stapled-ticket", "archive-stapled-app"}:
                self.assertEqual(report["notarization"]["id"], SUBMISSION)
                self.assertTrue((self.args.output / "notarization-input.zip").is_file())
            self.assertLessEqual([call[0] for call in self.commands.calls].count("submit-notarization"), 1)
            shutil.rmtree(self.args.output)

    def test_cancellation_during_signing_or_wait_cleans(self):
        for label in ("import-signing-identity", "sign-app", "wait-for-notarization"):
            self.behavior = {"cancel": label}
            with self.subTest(label=label), self.assertRaises(signing.Cancelled):
                self.run_sign()
            self.assert_cleanup()
            self.assert_failed_without_final()
            shutil.rmtree(self.args.output)

    def test_actual_sigterm_invokes_cleanup_without_apple_commands(self):
        self.behavior = {"cancel": "import-signing-identity", "actual_cancel": True}
        previous = signal.getsignal(signal.SIGTERM)
        with self.assertRaises(signing.Cancelled), signing.cancellation_handler():
            self.run_sign()
        self.assertEqual(signal.getsignal(signal.SIGTERM), previous)
        self.assert_cleanup()
        self.assert_failed_without_final()

    def test_cleanup_failure_withholds_final_archive_and_still_deletes_workspace(self):
        for label in ("restore-keychain-search-list", "delete-signing-keychain"):
            self.behavior = {"failure": [label]}
            with self.subTest(label=label), self.assertRaisesRegex(signing.SigningError, "cleanup failed"):
                self.run_sign()
            self.assert_cleanup()
            self.assert_failed_without_final()
            self.assertEqual(self.report()["keychain_cleanup"]["status"], "failed")
            shutil.rmtree(self.args.output)

    def test_notary_status_and_log_bind_to_this_submission_and_archive(self):
        variants = (
            {"response": {"id": SUBMISSION, "status": "In Progress"}},
            {"response": {"id": "138cf36c-2fe5-466b-b3b5-d0028b22b3a4", "status": "Accepted"}},
            {"response": {"status": "Accepted"}},
            {"response": {"id": SUBMISSION, "status": "Invalid"}, "log_changes": {"status": "Invalid", "statusCode": 4000}},
            {"log_changes": {"sha256": "0" * 64}},
            {"log_changes": {"jobId": "138cf36c-2fe5-466b-b3b5-d0028b22b3a4"}},
            {"log_changes": {"jobId": None}},
            {"log_changes": {"jobId": "not-a-uuid"}},
            {"log_changes": {"jobId": 17}},
            {"log_changes": {"statusCode": 4000}},
            {"log_changes": {"issues": [{"severity": "error", "code": 123}]}},
        )
        for changes in variants:
            self.behavior = changes
            with self.subTest(changes=changes), self.assertRaises(signing.SigningError):
                self.run_sign()
            self.assert_cleanup()
            self.assert_failed_without_final()
            self.assertNotIn("staple-ticket", [call[0] for call in self.commands.calls])
            shutil.rmtree(self.args.output)

    def test_equivalent_uuid_case_in_submission_wait_and_log_is_accepted(self):
        for changes in (
            {"log_changes": {"jobId": SUBMISSION.upper()}},
            {"submission_response": {"id": SUBMISSION.upper()},
             "response": {"id": SUBMISSION.upper(), "status": "Accepted"},
             "log_changes": {"jobId": SUBMISSION.upper()}},
        ):
            self.behavior = changes
            with self.subTest(changes=changes):
                try:
                    report = self.run_sign()
                    self.assertEqual(report["status"], "signed-notarized-stapled")
                    self.assertEqual(report["notarization"]["id"], SUBMISSION)
                    self.assertEqual(report["notarization"]["log"]["job_id"], SUBMISSION)
                    self.assert_cleanup()
                finally:
                    if self.args.output.exists():
                        shutil.rmtree(self.args.output)

    def test_nonzero_rejection_retains_validated_log_without_stapling(self):
        self.behavior = {"nonzero_response": "wait-for-notarization",
                         "response": {"id": SUBMISSION, "status": "Invalid"},
                         "log_changes": {"status": "Invalid", "statusCode": 4000}}
        with self.assertRaisesRegex(signing.SigningError, "did not accept"):
            self.run_sign()
        self.assertEqual(self.report()["notarization"]["log"]["status"], "Invalid")
        self.assertNotIn("staple-ticket", [call[0] for call in self.commands.calls])
        self.assert_cleanup()

    def test_nonzero_upload_with_id_is_preserved_as_uncertain_and_not_retried(self):
        self.behavior = {"nonzero_response": "submit-notarization"}
        with self.assertRaises(signing.CommandFailure):
            self.run_sign()
        self.assertEqual(self.report()["notarization"], {"id": SUBMISSION, "status": "submission-uncertain"})
        labels = [call[0] for call in self.commands.calls]
        self.assertNotIn("wait-for-notarization", labels)
        self.assertEqual(labels.count("submit-notarization"), 1)
        self.assert_cleanup()
        self.assert_failed_without_final()

    def test_nonzero_wait_cannot_succeed_even_with_accepted_body(self):
        self.behavior = {"nonzero_response": "wait-for-notarization"}
        with self.assertRaisesRegex(signing.SigningError, "wait failed"):
            self.run_sign()
        self.assert_failed_without_final()

    def test_imported_certificate_must_match_and_be_the_only_identity(self):
        matching = f'1) {IDENTITY} "Developer ID Application: Fixture ({TEAM})"\n'
        for value in ("", matching.replace(IDENTITY, "B" * 40), matching + matching,
                      matching.replace(TEAM, "ABCDE12345"), matching.replace("Developer ID Application", "Apple Development")):
            self.behavior = {"imported_identity": value.encode()}
            with self.subTest(value=value), self.assertRaisesRegex(signing.SigningError, "Imported identity"):
                self.run_sign()
            self.assert_cleanup()
            self.assert_failed_without_final()
            shutil.rmtree(self.args.output)

    def test_signature_team_timestamp_runtime_and_entitlements_are_required(self):
        variants = (
            {"signature_changes": {"TeamIdentifier": "OTHERTEAMX"}},
            {"signature_changes": {"Identifier": "other.app"}},
            {"signature_changes": {"Authority": f"Apple Development: Fixture ({TEAM})"}},
            {"signature_changes": {"Timestamp": None}},
            {"signature_changes": {"CodeDirectory": "v=20500 flags=0x0(none)"}},
            {"entitlements": {"com.apple.security.get-task-allow": True}},
            {"entitlements": {"com.apple.security.cs.allow-jit": True}},
        )
        for changes in variants:
            self.behavior = changes
            with self.subTest(changes=changes), self.assertRaises(signing.SigningError):
                self.run_sign()
            self.assert_cleanup()
            self.assert_failed_without_final()
            self.assertNotIn("submit-notarization", [call[0] for call in self.commands.calls])
            shutil.rmtree(self.args.output)

    def test_report_omits_raw_service_paths_messages_and_credentials(self):
        self.behavior = {"log_changes": {"issues": [{"severity": "warning", "code": 123,
                          "message": self.environ[signing.CREDENTIALS[1]],
                          "path": "/private/SENSITIVE-LOCATION/file"}]}}
        self.run_sign()
        report_text = (self.args.output / "signing-result.json").read_text()
        for secret in self.environ.values():
            self.assertNotIn(secret, report_text)
        self.assertNotIn("SENSITIVE-LOCATION", report_text)
        self.assertEqual(self.report()["notarization"]["log"]["issue_counts"], {"warning": 1, "error": 0})


class LocalSubprocessTests(unittest.TestCase):
    def test_no_credentials_or_loader_environment_reaches_commands(self):
        report = {"commands": []}
        with patch.dict(os.environ, {"GITTURTLE_SIGNING_P12_PASSWORD": "secret", "DYLD_INSERT_LIBRARIES": "bad", "PYTHONPATH": "bad"}):
            command = signing.Commands(report)
            output, _ = command("environment-fixture", [sys.executable, "-I", "-c", "import os,json;print(json.dumps(dict(os.environ)))"])
        environment = json.loads(output)
        self.assertNotIn("GITTURTLE_SIGNING_P12_PASSWORD", environment)
        self.assertNotIn("DYLD_INSERT_LIBRARIES", environment)
        self.assertNotIn("PYTHONPATH", environment)

    def test_failure_output_and_arguments_are_not_logged(self):
        report = {"commands": []}
        command = signing.Commands(report)
        with self.assertRaises(signing.SigningError):
            command("failure-fixture", [sys.executable, "-I", "-c", "import sys;print('SECRET');sys.exit(4)"])
        self.assertNotIn("SECRET", json.dumps(report))
        self.assertEqual(report["commands"][0]["exit_code"], 4)

    def test_oversized_and_timed_out_commands_are_stopped(self):
        for source, timeout in (("import sys;sys.stdout.write('x'*3000000)", 5),
                                ("import time;time.sleep(10)", 0.15)):
            report = {"commands": []}
            command = signing.Commands(report)
            with self.subTest(source=source), self.assertRaises(signing.SigningError):
                command("bounded-fixture", [sys.executable, "-I", "-c", source], timeout)
            self.assertEqual(report["commands"][0]["status"], "failed")
            self.assertLess(report["commands"][0]["elapsed_seconds"], 5)

    def test_duplicate_identity_keys_are_rejected(self):
        with self.assertRaises(signing.SigningError):
            signing.read_json(b'{"application":"Other","application":"GitTurtle"}', "identity")

    def test_sigterm_stops_a_real_child_and_restores_signal_handler(self):
        source = f"""
import importlib.util, json, sys
spec = importlib.util.spec_from_file_location('signing', {str(SCRIPT)!r})
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)
report = {{'commands': []}}
try:
    with signing.cancellation_handler():
        signing.Commands(report)('cancel-fixture', [sys.executable, '-I', '-c',
            'import os,signal,time;os.kill(os.getppid(),signal.SIGTERM);time.sleep(20)'])
except signing.Cancelled:
    print(json.dumps(report))
else:
    raise SystemExit(2)
"""
        result = subprocess.run([sys.executable, "-I", "-c", source], capture_output=True, check=True, timeout=5)
        report = json.loads(result.stdout)
        self.assertEqual(report["commands"][0]["status"], "failed")
        self.assertLess(report["commands"][0]["elapsed_seconds"], 5)


if __name__ == "__main__":
    unittest.main()
