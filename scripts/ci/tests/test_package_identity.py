"""Package provenance/refusal fixtures; no native package evidence is claimed."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
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

    def test_probe_handles_success_nonzero_and_oversized_output(self):
        program = self.root / "probe"
        for code, expected in ((f"print({json.dumps(self.value)!r})", None),
                               ("raise SystemExit(7)", "failed"),
                               ("print('x' * 20000)", "output limit")):
            with self.subTest(code=code):
                program.write_text("#!/usr/bin/env python3\n" + code + "\n")
                program.chmod(0o755)
                if expected:
                    with self.assertRaisesRegex(identity.PackageError, expected):
                        identity.probe(program)
                else:
                    self.assertEqual(identity.probe(program), self.value)


if __name__ == "__main__":
    unittest.main()
