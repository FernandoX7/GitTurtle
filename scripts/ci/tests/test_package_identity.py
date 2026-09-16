"""Package provenance/refusal fixtures; no native package evidence is claimed."""
import contextlib
import copy
import errno
import io
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[2] / "package-identity.py"
SPEC = importlib.util.spec_from_file_location("package_identity", SCRIPT)
identity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(identity)
TARGET = "x86_64-unknown-linux-gnu"


class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.source.mkdir()
        (self.source / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0"\n')
        (self.source / "Cargo.lock").write_text('version = 4\n')
        for args in (["init", "-q"], ["add", "."],
                     ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                      "-c", "commit.gpgSign=false", "commit", "-qm", "fixture"]):
            subprocess.run(["git", "-C", str(self.source), *args], check=True, capture_output=True)
        self.revision = subprocess.check_output(["git", "-C", str(self.source), "rev-parse", "HEAD"], text=True).strip()
        self.value = dict(application="GitTurtle", version="0.1.0", source_revision=self.revision,
                          source_tree="clean", target=TARGET, profile="release",
                          rustc="rustc 1.98.0", build_unix_seconds="1789492155")
        self.binary = self.root / "gitturtle"
        header = bytearray(32)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = (62).to_bytes(2, "little")
        self.binary.write_bytes(header)
        self.binary.chmod(0o755)
        self.licenses = self.root / "licenses"
        self.licenses.mkdir()
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
            (self.licenses / name).write_text("Synthetic fixture notice\n")
        self.inventory = dict(target=TARGET, cargo_lock_sha256=identity.digest(self.source / "Cargo.lock"),
                              review_required=[], dependencies=[{"name": "fixture"}])
        self.write_inventory()

    def write_inventory(self):
        (self.licenses / "dependencies.json").write_text(json.dumps(self.inventory))

    def manifest(self, **kwargs):
        with mock.patch.object(identity, "probe", return_value=self.value):
            return identity.create_manifest(binary=self.binary, licenses=self.licenses,
                                            root=self.source, target=TARGET, **kwargs)

    def test_exact_identity_and_final_hash_are_preserved(self):
        result = self.manifest(distribution=True, expected_sha256=identity.digest(self.binary))
        self.assertEqual(result["compiled_identity"], self.value)
        self.assertEqual(result["binary_sha256"], identity.digest(self.binary))
        self.assertEqual(result["license_inventory_sha256"], identity.digest(self.licenses / "dependencies.json"))
        self.assertEqual(result["distribution"], "complete-notices")
        self.assertEqual(result["signing"], "unsigned")

    def test_wrong_compiled_fields_fail(self):
        original = copy.deepcopy(self.value)
        for key in ("application", "version", "source_revision", "target", "profile", "source_tree", "rustc", "build_unix_seconds"):
            with self.subTest(field=key):
                self.value = dict(original, **{key: "unavailable"})
                with self.assertRaises(identity.PackageError):
                    self.manifest()

    def test_stale_checkout_expected_version_and_digest_fail(self):
        for kwargs in ({"revision": "f" * 40}, {"version": "0.2.0"}, {"expected_sha256": "f" * 64}):
            with self.subTest(kwargs=kwargs), self.assertRaises(identity.PackageError):
                self.manifest(**kwargs)

    def test_distribution_requires_clean_source_and_checkout(self):
        self.value["source_tree"] = "modified"
        self.assertEqual(self.manifest()["distribution"], "development")
        with self.assertRaisesRegex(identity.PackageError, "clean release"):
            self.manifest(distribution=True)
        self.value["source_tree"] = "clean"
        (self.source / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0"\n# modified\n')
        with self.assertRaisesRegex(identity.PackageError, "clean packaging"):
            self.manifest(distribution=True)

    def test_notice_gaps_stay_visible_and_block_distribution(self):
        self.inventory["review_required"] = ["fixture-0.1.0"]
        self.write_inventory()
        with self.assertRaises(OSError):
            self.manifest()
        (self.licenses / "REVIEW_REQUIRED.md").write_text("Fixture unresolved\n")
        self.assertEqual(self.manifest()["license_review_required"], ["fixture-0.1.0"])
        with self.assertRaisesRegex(identity.PackageError, "public distribution"):
            self.manifest(distribution=True)

    def test_wrong_license_target_lock_or_inventory_fail(self):
        original = copy.deepcopy(self.inventory)
        for key, value in (("target", "aarch64-apple-darwin"), ("cargo_lock_sha256", "f" * 64),
                           ("review_required", None), ("dependencies", [])):
            with self.subTest(field=key):
                self.inventory = dict(original, **{key: value})
                self.write_inventory()
                with self.assertRaises(identity.PackageError):
                    self.manifest()

    def test_changed_binary_probe_fails(self):
        def change(_binary):
            with self.binary.open("ab") as stream:
                stream.write(b"changed")
            return self.value
        with mock.patch.object(identity, "probe", side_effect=change):
            with self.assertRaisesRegex(identity.PackageError, "changed while"):
                identity.create_manifest(binary=self.binary, licenses=self.licenses, root=self.source, target=TARGET)

    def test_wrong_architecture_and_symlink_fail(self):
        with self.assertRaises(identity.PackageError):
            identity.verify_architecture(self.binary, "aarch64-apple-darwin")
        link = self.root / "linked"
        link.symlink_to(self.binary)
        with self.assertRaises(identity.PackageError):
            identity.verify_architecture(link, TARGET)

    def test_duplicate_json_fields_fail(self):
        path = self.licenses / "dependencies.json"
        path.write_text('{"target":"expected","target":"wrong"}')
        with self.assertRaisesRegex(identity.PackageError, "Duplicate"):
            identity.read_json(path)

    def test_extracted_manifest_checks_compiled_identity_and_payload_hashes(self):
        valid = self.manifest()
        identity.validate_manifest(valid, self.binary, self.licenses)
        changes = (("format", 1), ("application", "Other"), ("bundle_id", "other.app"),
                   ("target", "aarch64-apple-darwin"), ("compiled_identity", None),
                   ("packaged_from_revision", "f" * 40), ("packaging_tree_status", None),
                   ("binary_sha256", "f" * 64), ("input_binary_sha256", None),
                   ("cargo_lock_sha256", "f" * 64), ("license_inventory_sha256", "f" * 64),
                   ("license_review_required", ["hidden gap"]), ("signing", "notarized"))
        for field, value in changes:
            with self.subTest(field=field), self.assertRaises(identity.PackageError):
                identity.validate_manifest(dict(valid, **{field: value}), self.binary, self.licenses)
        for field, value in (("version", "unknown"), ("profile", "debug"),
                             ("source_revision", "f" * 40), ("source_tree", "unknown")):
            changed = copy.deepcopy(valid)
            changed["compiled_identity"][field] = value
            with self.subTest(compiled_field=field), self.assertRaises(identity.PackageError):
                identity.validate_manifest(changed, self.binary, self.licenses)

    def test_extracted_manifest_cannot_hide_notice_gaps_or_modified_distribution(self):
        self.inventory["review_required"] = ["fixture-0.1.0"]
        self.write_inventory()
        (self.licenses / "REVIEW_REQUIRED.md").write_text("Synthetic unresolved notice")
        valid = self.manifest()
        identity.validate_manifest(valid, self.binary, self.licenses)
        forged = dict(valid, distribution="complete-notices")
        with self.assertRaisesRegex(identity.PackageError, "Incomplete notices"):
            identity.validate_manifest(forged, self.binary, self.licenses)
        self.inventory["review_required"] = []
        self.write_inventory()
        valid = self.manifest(distribution=True)
        for field in ("packaging_tree_status", "source_tree"):
            changed = copy.deepcopy(valid)
            if field == "source_tree":
                changed["compiled_identity"][field] = "modified"
            else:
                changed[field] = "modified"
            with self.subTest(field=field), self.assertRaises(identity.PackageError):
                identity.validate_manifest(changed, self.binary, self.licenses)

    def test_extracted_manifest_rejects_modified_license_bytes_and_missing_notices(self):
        valid = self.manifest()
        (self.licenses / "dependencies.json").write_text(json.dumps(self.inventory) + " ")
        with self.assertRaisesRegex(identity.PackageError, "License inventory"):
            identity.validate_manifest(valid, self.binary, self.licenses)
        self.write_inventory()
        (self.licenses / "LICENSE").unlink()
        with self.assertRaises(OSError):
            identity.validate_manifest(valid, self.binary, self.licenses)

    def test_cli_modified_reused_binary_requires_explicit_digest_pin(self):
        output = self.root / "build-info.json"
        original = copy.deepcopy(self.value)
        for modified_owner in ("compiled", "packaging"):
            with self.subTest(modified_owner=modified_owner):
                self.value = copy.deepcopy(original)
                if modified_owner == "compiled":
                    self.value["source_tree"] = "modified"
                else:
                    (self.source / "untracked-change.txt").write_text("changed source")
                args = [str(SCRIPT), "--root", str(self.source), "--binary", str(self.binary),
                        "--licenses", str(self.licenses), "--target", TARGET,
                        "--signing", "unsigned", "--output", str(output)]
                stderr = io.StringIO()
                with (mock.patch("sys.argv", args), mock.patch.object(identity, "probe", return_value=self.value),
                      contextlib.redirect_stderr(stderr), self.assertRaises(SystemExit) as failure):
                    identity.main()
                self.assertNotEqual(failure.exception.code, 0)
                self.assertIn("--expected-sha256", stderr.getvalue())
                self.assertFalse(output.exists())
                with (mock.patch("sys.argv", [*args, "--expected-sha256", identity.digest(self.binary)]),
                      mock.patch.object(identity, "probe", return_value=self.value)):
                    self.assertEqual(identity.main(), 0)
                self.assertEqual(identity.read_json(output)["binary_sha256"], identity.digest(self.binary))
                output.unlink()

    def test_cli_never_overwrites_existing_manifest(self):
        output = self.root / "build-info.json"
        output.write_text("unrelated existing output")
        args = [str(SCRIPT), "--root", str(self.source), "--binary", str(self.binary),
                "--licenses", str(self.licenses), "--target", TARGET,
                "--signing", "unsigned", "--output", str(output)]
        with (mock.patch("sys.argv", args), mock.patch.object(identity, "probe", return_value=self.value),
              contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit)):
            identity.main()
        self.assertEqual(output.read_text(), "unrelated existing output")

    def test_probe_deadline_covers_child_holding_output_pipe(self):
        program = self.root / "hanging-probe"
        descendant = self.root / "descendant.pid"
        program.write_text("#!/usr/bin/env python3\nimport os, time\nfrom pathlib import Path\n"
                           f"if os.fork() == 0:\n    Path({str(descendant)!r}).write_text(str(os.getpid()))\n"
                           "    time.sleep(30)\n"
                           "else:\n    print('{}')\n")
        program.chmod(0o755)
        before = time.monotonic()
        with self.assertRaisesRegex(identity.PackageError, "timed out"):
            identity.probe(program)
        self.assertLess(time.monotonic() - before, 8)
        deadline = time.monotonic() + 2
        while True:
            status = subprocess.run(["ps", "-o", "stat=", "-p", descendant.read_text()],
                                    capture_output=True, text=True, timeout=2)
            # An orphan can remain a zombie until the host reaps it, but no
            # live descendant may retain the probe's output pipe after cleanup.
            if status.returncode == 1 or status.stdout.strip().startswith("Z"):
                break
            self.assertEqual(status.returncode, 0, status.stderr)
            self.assertLess(time.monotonic(), deadline, "probe descendant survived cleanup")
            time.sleep(0.01)

    def test_json_file_limit_and_symlinks_fail_before_read(self):
        path = self.root / "oversized.json"
        with path.open("wb") as stream:
            stream.truncate(identity.MAX_JSON + 1)
        with self.assertRaisesRegex(identity.PackageError, "bounded regular"):
            identity.read_json(path)
        linked = self.root / "linked.json"
        linked.symlink_to(self.licenses / "dependencies.json")
        with self.assertRaisesRegex(identity.PackageError, "bounded regular"):
            identity.read_json(linked)

    def test_probe_handles_success_nonzero_and_oversized_output(self):
        program = self.root / "probe"
        for code, expected in ((f"print({json.dumps(self.value)!r})", None),
                               ("raise SystemExit(7)", "failed"),
                               ("print('x' * 20000)", "output limit"),
                               ("import sys; sys.stderr.write('x' * 20000)", "output limit"),
                               ("print('[]')", "JSON object"),
                               ("print('{\"same\":1,\"same\":2}')", "Duplicate")):
            with self.subTest(code=code):
                program.write_text("#!/usr/bin/env python3\n" + code + "\n")
                program.chmod(0o755)
                if expected:
                    with self.assertRaisesRegex(identity.PackageError, expected):
                        identity.probe(program)
                else:
                    self.assertEqual(identity.probe(program), self.value)

    def test_probe_cleanup_handles_darwin_zombie_group_race(self):
        program = self.root / "exiting-probe"
        program.write_text("#!/usr/bin/env python3\nimport os\nos.write(1, b'x' * 20000)\n")
        program.chmod(0o755)
        spawned = []
        actual_popen = subprocess.Popen
        actual_killpg = identity.os.killpg

        def spawn(command, **kwargs):
            process = actual_popen(command, **kwargs)
            spawned.append((command, process))
            return process

        def darwin_killpg(group, signum):
            probe = next(process for command, process in spawned if command[-1] == "--build-info")
            def state(pid):
                status = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)],
                                        capture_output=True, text=True, timeout=2)
                self.assertEqual(status.returncode, 0, status.stderr)
                return status.stdout.strip()
            # Force the observed ordering: output overflows, then the probe
            # exits without being reaped before the group signal is sent.
            deadline = time.monotonic() + 2
            while not state(probe.pid).startswith("Z"):
                self.assertLess(time.monotonic(), deadline, "probe did not exit")
                time.sleep(0.01)
            if state(group).startswith("Z"):
                raise PermissionError(errno.EPERM, "Darwin zombie-only process group")
            actual_killpg(group, signum)

        with (mock.patch.object(identity.subprocess, "Popen", side_effect=spawn),
              mock.patch.object(identity.os, "killpg", side_effect=darwin_killpg),
              self.assertRaisesRegex(identity.PackageError, "output limit")):
            identity.probe(program)
        self.assertTrue(all(process.returncode is not None for _, process in spawned))
        self.assertTrue(all(stream.closed for _, process in spawned
                            for stream in (process.stdin, process.stdout, process.stderr) if stream is not None))

    def test_probe_surfaces_live_group_permission_failure_and_releases_children(self):
        program = self.root / "live-probe"
        program.write_text("#!/usr/bin/env python3\nimport time\n"
                           "print('x' * 20000, flush=True)\ntime.sleep(30)\n")
        program.chmod(0o755)
        spawned = []
        actual_popen = subprocess.Popen

        def spawn(command, **kwargs):
            process = actual_popen(command, **kwargs)
            spawned.append(process)
            return process

        before = time.monotonic()
        with (mock.patch.object(identity.subprocess, "Popen", side_effect=spawn),
              mock.patch.object(identity.os, "killpg", side_effect=PermissionError(errno.EPERM, "live group")),
              self.assertRaisesRegex(PermissionError, "live group")):
            identity.probe(program)
        self.assertLess(time.monotonic() - before, 8)
        self.assertTrue(all(process.returncode is not None for process in spawned))
        self.assertTrue(all(stream.closed for process in spawned
                            for stream in (process.stdin, process.stdout, process.stderr) if stream is not None))

    def test_probe_spawn_failure_releases_guard(self):
        program = self.root / "unlaunchable-probe"
        program.write_text("#!/nonexistent/gitturtle-fixture-interpreter\n")
        program.chmod(0o755)
        spawned = []
        actual_popen = subprocess.Popen

        def spawn(command, **kwargs):
            process = actual_popen(command, **kwargs)
            spawned.append(process)
            return process

        with (mock.patch.object(identity.subprocess, "Popen", side_effect=spawn),
              self.assertRaises(FileNotFoundError)):
            identity.probe(program)
        self.assertTrue(spawned)
        self.assertTrue(all(process.returncode is not None for process in spawned))


if __name__ == "__main__":
    unittest.main()
