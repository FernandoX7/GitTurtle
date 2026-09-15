from __future__ import annotations

import argparse
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile

SPEC = importlib.util.spec_from_file_location("ci_packages", Path(__file__).resolve().parents[1] / "packages.py")
packages = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(packages)
MAC_SPEC = importlib.util.spec_from_file_location("package_macos_diagnostics", packages.ROOT / "scripts/package-macos.py")
macos = importlib.util.module_from_spec(MAC_SPEC)
MAC_SPEC.loader.exec_module(macos)


def sha(data):
    return hashlib.sha256(data).hexdigest()


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def tar(self, entries):
        archive = self.root / "fixture.tar.gz"
        with tarfile.open(archive, "w:gz") as out:
            for name, content, kind in entries:
                member = tarfile.TarInfo(name)
                member.type = kind
                member.size = len(content) if kind == tarfile.REGTYPE else 0
                member.linkname = "../../outside"
                member.mode = 0o6755
                out.addfile(member, io.BytesIO(content) if kind == tarfile.REGTYPE else None)
        return archive

    def test_extract_preserves_bytes_executable_intent_not_special_bits(self):
        archive = self.tar([("bundle", b"", tarfile.DIRTYPE), ("bundle/bin", b"content", tarfile.REGTYPE)])
        packages.extract(archive, self.root / "extracted")
        binary = self.root / "extracted/bundle/bin"
        self.assertEqual(binary.read_bytes(), b"content")
        self.assertEqual(stat.S_IMODE(binary.stat().st_mode), 0o755)

    def test_tar_refuses_traversal_absolute_links_devices_duplicates(self):
        for name, kind in [("../outside", tarfile.REGTYPE), ("/tmp/outside", tarfile.REGTYPE),
                           ("bundle/link", tarfile.SYMTYPE), ("bundle/hard", tarfile.LNKTYPE),
                           ("bundle/device", tarfile.CHRTYPE), ("bundle/./file", tarfile.REGTYPE),
                           ("bundle\\file", tarfile.REGTYPE), ("bundle/line\nfile", tarfile.REGTYPE)]:
            with self.subTest(name=name, kind=kind):
                archive = self.tar([(name, b"bad", kind)])
                with self.assertRaises(packages.Error):
                    packages.extract(archive, self.root / "extracted")
                shutil.rmtree(self.root / "extracted")
        archive = self.tar([("bundle/A", b"a", tarfile.REGTYPE), ("bundle/a", b"b", tarfile.REGTYPE)])
        with self.assertRaises(packages.Error):
            packages.extract(archive, self.root / "extracted")

    def test_archive_count_expanded_size_and_member_size_bounds(self):
        archive = self.tar([("bundle/a", b"12345", tarfile.REGTYPE), ("bundle/b", b"67890", tarfile.REGTYPE)])
        for module, variable, bound in [(packages, "MAX_ENTRIES", 1), (packages, "MAX_TREE", 9),
                                        (packages.identity, "MAX_BINARY", archive.stat().st_size - 1)]:
            with self.subTest(variable=variable), patch.object(module, variable, bound):
                with self.assertRaises(packages.Error):
                    packages.extract(archive, self.root / "extracted")
                if (self.root / "extracted").exists():
                    shutil.rmtree(self.root / "extracted")

    def test_tar_reader_refuses_unbounded_read_before_decompressing(self):
        stream = Mock()
        reader = packages.TarReader(stream)
        for size in (-1, packages.MAX_LOG + 1, 2**40):
            with self.subTest(size=size), self.assertRaisesRegex(packages.Error, "metadata read"):
                reader.read(size)
        stream.read.assert_not_called()

    def test_truncated_pax_header_is_refused_without_unbounded_reads(self):
        archive = self.root / "huge-header.tar.gz"
        header = tarfile.TarInfo("pax")
        header.type = tarfile.XHDTYPE
        header.size = 2**40
        with gzip.open(archive, "wb") as out:
            out.write(header.tobuf(format=tarfile.GNU_FORMAT))
        original_read = gzip.GzipFile.read
        read_sizes = []

        def bounded_read(stream, size=-1):
            # Fail before a faulty reader could actually allocate a huge buffer.
            self.assertGreaterEqual(size, 0)
            self.assertLessEqual(size, packages.MAX_LOG)
            read_sizes.append(size)
            return original_read(stream, size)

        # Newer Python versions bound extended-header reads themselves and can
        # reject this truncated header before our oversized-read guard fires.
        # Either refusal is valid; an unbounded decompressor read is never valid.
        with patch.object(gzip.GzipFile, "read", bounded_read):
            with self.assertRaises((packages.Error, tarfile.ReadError)):
                packages.extract(archive, self.root / "extracted")
        self.assertTrue(read_sizes)
        self.assertEqual(list((self.root / "extracted").iterdir()), [])

    def test_extract_refuses_existing_output_and_symlink(self):
        archive = self.tar([("file", b"new", tarfile.REGTYPE)])
        output = self.root / "existing"
        output.mkdir()
        (output / "file").write_bytes(b"old")
        with self.assertRaises(FileExistsError):
            packages.extract(archive, output)
        self.assertEqual((output / "file").read_bytes(), b"old")
        link = self.root / "link"
        link.symlink_to(output, target_is_directory=True)
        with self.assertRaises(FileExistsError):
            packages.extract(archive, link)

    def test_zip_directory_is_bounded_before_zipfile_materializes_entries(self):
        archive = self.root / "huge-directory.zip"
        archive.write_bytes(struct.pack("<4s4H2IH", b"PK\x05\x06", 0, 0, 65535, 65535, 0, 0, 0))
        with patch.object(zipfile, "ZipFile", side_effect=AssertionError("must reject before opening")):
            with self.assertRaisesRegex(packages.Error, "metadata bound"):
                packages.extract(archive, self.root / "extracted")

    def test_zip64_locator_cannot_override_bounded_legacy_directory(self):
        for comment_size in (0, 65535):
            with self.subTest(comment_size=comment_size):
                archive = self.root / "zip64.zip"
                with zipfile.ZipFile(archive, "w") as out:
                    for index in range(4):
                        out.writestr(f"entry-{index}", b"fixture")
                raw = archive.read_bytes()
                eocd = struct.unpack("<4s4H2IH", raw[-22:])
                directory_size, directory_start = eocd[5:7]
                prefix = raw[:-22]
                record = struct.pack("<4sQHHIIQQQQ", b"PK\x06\x06", 44, 45, 45, 0, 0,
                                     4, 4, directory_size, directory_start)
                locator = struct.pack("<4sIQI", b"PK\x06\x07", 0, len(prefix), 1)
                legacy = struct.pack("<4s4H2IH", b"PK\x05\x06", 0, 0, 0, 0, 0, 0, comment_size)
                archive.write_bytes(prefix + record + locator + legacy + b"x" * comment_size)
                # Establish the fixture really is accepted by Python with four
                # effective entries despite the legacy zero count/size fields.
                with zipfile.ZipFile(archive) as source:
                    self.assertEqual(len(source.infolist()), 4)
                with patch.object(packages, "MAX_ENTRIES", 2), \
                        patch.object(zipfile, "ZipFile", side_effect=AssertionError("must refuse before allocation")):
                    with self.assertRaisesRegex(packages.Error, "ZIP64"):
                        packages.extract(archive, self.root / "extracted")
                shutil.rmtree(self.root / "extracted")

    def test_legacy_entry_count_cannot_understate_actual_directory(self):
        archive = self.root / "understated.zip"
        with zipfile.ZipFile(archive, "w") as out:
            for index in range(4):
                out.writestr(f"entry-{index}", b"fixture")
        raw = bytearray(archive.read_bytes())
        struct.pack_into("<HH", raw, len(raw) - 22 + 8, 0, 0)
        archive.write_bytes(raw)
        with zipfile.ZipFile(archive) as source:
            self.assertEqual(len(source.infolist()), 4)
        with patch.object(packages, "MAX_ENTRIES", 2), \
                patch.object(zipfile, "ZipFile", side_effect=AssertionError("must refuse before allocation")):
            with self.assertRaisesRegex(packages.Error, "entry bound"):
                packages.extract(archive, self.root / "extracted")

    def test_zip_preserves_bytes_and_rejects_link_duplicate_case_and_traversal(self):
        archive = self.root / "fixture.zip"
        for names in [["../outside"], ["A", "a"], ["link"], ["good"]]:
            with self.subTest(names=names):
                with zipfile.ZipFile(archive, "w") as out:
                    for name in names:
                        info = zipfile.ZipInfo(name)
                        info.external_attr = ((stat.S_IFLNK if name == "link" else stat.S_IFREG) | 0o755) << 16
                        out.writestr(info, b"content")
                if names == ["good"]:
                    packages.extract(archive, self.root / "extracted")
                    self.assertEqual((self.root / "extracted/good").read_bytes(), b"content")
                else:
                    with self.assertRaises(packages.Error):
                        packages.extract(archive, self.root / "extracted")
                shutil.rmtree(self.root / "extracted")


class NoticeTests(unittest.TestCase):
    def test_only_exact_strict_result_and_documented_gaps_can_withhold(self):
        target = "x86_64-unknown-linux-gnu"
        value = dict(target=target, cargo_lock_sha256="a" * 64, dependencies=[{}], review_required=[])
        self.assertTrue(packages.notice_state(0, value, target, "a" * 64))
        for missing in (["mac-0.1.1"], ["ufbx-0.11.3", "mac-0.1.1"]):
            value["review_required"] = missing
            self.assertFalse(packages.notice_state(2, value, target, "a" * 64))
        for code, missing in [(1, ["mac-0.1.1"]), (0, ["mac-0.1.1"]), (2, []), (2, ["new-gap-1.0"]), (2, None)]:
            value["review_required"] = missing
            with self.subTest(code=code, missing=missing), self.assertRaises(packages.Error):
                packages.notice_state(code, value, target, "a" * 64)

    def test_strict_inventory_target_lock_and_dependency_failure(self):
        value = dict(target="x86_64-unknown-linux-gnu", cargo_lock_sha256="a" * 64,
                     dependencies=[{}], review_required=[])
        for field, wrong in [("target", "wrong"), ("cargo_lock_sha256", "b" * 64), ("dependencies", [])]:
            with self.subTest(field=field), self.assertRaises(packages.Error):
                packages.notice_state(0, {**value, field: wrong}, "x86_64-unknown-linux-gnu", "a" * 64)


class PayloadTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.stem = "GitTurtle-0.1.0-aaaaaaaaaaaa-linux-x86_64-release"
        self.payload = self.root / "source" / self.stem
        (self.payload / "licenses").mkdir(parents=True)
        header = bytearray(32)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = (62).to_bytes(2, "little")
        self.binary = bytes(header) + b"unit-test fixture, not an executable"
        (self.payload / "bin").mkdir()
        (self.payload / "bin/gitturtle").write_bytes(self.binary)
        self.compiled = dict(application="GitTurtle", source_revision="a" * 40, version="0.1.0",
                             target="x86_64-unknown-linux-gnu", profile="release", source_tree="clean",
                             rustc="rustc fixture", build_unix_seconds="1")
        self.inventory = dict(target=self.compiled["target"], cargo_lock_sha256="b" * 64, review_required=[],
                              dependencies=[dict(name="fixture", notices=[dict(path="fixture-LICENSE", sha256=sha(b"full notice"))])])
        for name, data in {"LICENSE": b"project notice", "THIRD_PARTY_NOTICES.md": b"index", "fixture-LICENSE": b"full notice"}.items():
            (self.payload / "licenses" / name).write_bytes(data)
        (self.payload / "licenses/dependencies.json").write_text(json.dumps(self.inventory))
        self.package = dict(format=2, application="GitTurtle", bundle_id="com.gitturtle.desktop",
                            target=self.compiled["target"], compiled_identity=self.compiled,
                            packaged_from_revision="a" * 40, packaging_tree_status="clean", release_built_by_packager=False,
                            binary_sha256=sha(self.binary), input_binary_sha256=sha(self.binary), cargo_lock_sha256="b" * 64,
                            license_inventory_sha256=packages.identity.digest(self.payload / "licenses/dependencies.json"),
                            license_review_required=[], distribution="complete-notices", signing="unsigned")
        self.rebuild()

    def tearDown(self):
        self.temporary.cleanup()

    def rebuild(self):
        info = self.payload / "build-info.json"
        info.write_text(json.dumps(self.package))
        archive = self.root / (self.stem + ".tar.gz")
        with tarfile.open(archive, "w:gz") as out:
            out.add(self.payload, arcname=self.stem)
        self.outer = packages.identity.archive_manifest(archive, info)
        manifest = Path(str(archive) + ".manifest.json")
        manifest.write_text(json.dumps(self.outer))
        Path(str(archive) + ".sha256").write_text(f"{packages.identity.digest(archive)}  {archive.name}\n")
        self.expected = dict(archive=archive.name, archive_sha256=packages.identity.digest(archive),
                             manifest_sha256=packages.identity.digest(manifest), target=self.compiled["target"],
                             revision="a" * 40, version="0.1.0", input_binary_sha256=sha(self.binary), cargo_lock_sha256="b" * 64)

    def test_roundtrip_checks_real_archive_and_explicit_cross_platform_no_probe(self):
        with patch.object(packages.identity, "probe", return_value=self.compiled) as probe:
            payload, binary, package = packages.verify_payload(self.root, self.expected, complete=True)
            self.assertEqual(binary.read_bytes(), self.binary)
            self.assertEqual(package, self.package)
            probe.assert_called_once_with(binary)
        shutil.rmtree(self.root / "extracted")
        with patch.object(packages.identity, "probe", side_effect=AssertionError("must not execute foreign binary")):
            packages.verify_payload(self.root, self.expected, complete=True, probe=False)

    def test_mutated_archive_manifest_and_checksum_fail_before_probe(self):
        for suffix in ("", ".manifest.json", ".sha256"):
            with self.subTest(suffix=suffix):
                path = self.root / (self.expected["archive"] + suffix)
                before = path.read_bytes()
                path.write_bytes(before + b"changed")
                with patch.object(packages.identity, "probe") as probe, self.assertRaises(packages.Error):
                    packages.verify_payload(self.root, self.expected, complete=True)
                probe.assert_not_called()
                path.write_bytes(before)

    def test_expected_revision_version_target_input_digest_and_lock_mismatch_fail(self):
        for key, value in [("revision", "c" * 40), ("version", "0.2.0"), ("target", "aarch64-apple-darwin"),
                           ("input_binary_sha256", "c" * 64), ("cargo_lock_sha256", "c" * 64)]:
            with self.subTest(key=key), self.assertRaises((packages.Error, OSError)):
                packages.verify_payload(self.root, {**self.expected, key: value}, complete=True, probe=False)
            shutil.rmtree(self.root / "extracted")

    def test_incomplete_notices_cannot_be_promoted(self):
        self.package["distribution"] = "development"
        self.rebuild()
        with self.assertRaises(packages.Error):
            packages.verify_payload(self.root, self.expected, complete=True, probe=False)

    def test_notice_payload_tampering_is_detected_even_when_archive_digest_updated(self):
        (self.payload / "licenses/fixture-LICENSE").write_bytes(b"short substituted notice")
        self.rebuild()
        with self.assertRaisesRegex(packages.Error, "license notice"):
            packages.verify_payload(self.root, self.expected, complete=True, probe=False)

    def test_missing_full_notice_is_not_complete(self):
        self.inventory["dependencies"][0]["notices"] = []
        (self.payload / "licenses/dependencies.json").write_text(json.dumps(self.inventory))
        self.package["license_inventory_sha256"] = packages.identity.digest(self.payload / "licenses/dependencies.json")
        self.rebuild()
        with self.assertRaisesRegex(packages.Error, "notices"):
            packages.verify_payload(self.root, self.expected, complete=True, probe=False)

    def test_runtime_identity_mismatch_fails(self):
        with patch.object(packages.identity, "probe", return_value={**self.compiled, "version": "0.2.0"}):
            with self.assertRaisesRegex(packages.Error, "executable identity"):
                packages.verify_payload(self.root, self.expected, complete=True)

    def test_standard_ci_verifier_rejects_distribution_signed_metadata(self):
        self.package["signing"] = "developer-id-notarized"
        self.rebuild()
        with self.assertRaisesRegex(packages.Error, "signing status"):
            packages.verify_payload(self.root, self.expected, complete=True, probe=False)

    def test_outer_inner_manifest_conflict_fails(self):
        path = self.root / (self.expected["archive"] + ".manifest.json")
        self.outer["package"]["compiled_identity"]["version"] = "0.2.0"
        path.write_text(json.dumps(self.outer))
        self.expected["manifest_sha256"] = packages.identity.digest(path)
        with self.assertRaisesRegex(packages.Error, "manifests disagree"):
            packages.verify_payload(self.root, self.expected, complete=True, probe=False)


class PrepareTests(unittest.TestCase):
    def test_each_platform_packages_once_and_incomplete_notices_never_publish(self):
        for target in packages.TARGETS:
            for complete in (False, True):
                with self.subTest(target=target, complete=complete), tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    binary = root / "target" / target / "release/gitturtle"
                    binary.parent.mkdir(parents=True)
                    binary.write_bytes(b"fixture input")
                    args = argparse.Namespace(target=target, revision="a" * 40, directory=root / "work",
                                              event="pull_request", run_id="123", attempt="2",
                                              output=root / "github-output", summary=None, report=root / "report.json")
                    calls = []
                    def command(command, **kwargs):
                        command = list(map(str, command))
                        calls.append(command)
                        if "--require-complete" in command:
                            notices = Path(command[-1])
                            notices.mkdir()
                            (notices / "dependencies.json").write_text(json.dumps(dict(
                                target=target, cargo_lock_sha256="b" * 64, dependencies=[{}],
                                review_required=[] if complete else ["mac-0.1.1"])))
                            return (0 if complete else 2), ""
                        stem = f"GitTurtle-0.1.0-aaaaaaaaaaaa-{packages.TARGETS[target]}-release"
                        archive = root / "work/prepared" / (stem + (".tar.gz" if target.endswith("linux-gnu") else ".zip"))
                        for suffix in ("", ".sha256", ".manifest.json"):
                            Path(str(archive) + suffix).write_bytes(b"unit fixture")
                        return 0, ""
                    package = {"signing": "unsigned" if target.endswith("linux-gnu") else "ad-hoc"}
                    with patch.object(packages, "ROOT", root), \
                            patch.object(packages.identity, "expectations", return_value=("a" * 40, "0.1.0", "b" * 64, "clean")), \
                            patch.object(packages.identity, "probe", return_value={}), \
                            patch.object(packages.identity, "validate_identity"), \
                            patch.object(packages, "run", side_effect=command), \
                            patch.object(packages, "verify_payload", return_value=(root, binary, package)) as verify, \
                            patch.object(packages, "platform_checks"), patch("builtins.print"):
                        packages.prepare(args)
                    self.assertEqual(len(calls), 2)  # one strict collector, one no-build packager
                    self.assertIn("--no-build", calls[1])
                    self.assertEqual("--distribution" in calls[1], complete)
                    self.assertEqual(verify.call_args.kwargs["complete"], complete)
                    self.assertEqual((root / "work/publish").exists(), complete)
                    report = json.loads((root / "report.json").read_text())
                    self.assertIn("untrusted-pr-123-2", report["artifact_name"])
                    self.assertIs(report["release_input"], False)
                    self.assertEqual(report["hosted_transfer"], "not-performed")
                    self.assertEqual(report["native_smoke"], "not-performed")
                    self.assertEqual(report["upload_eligible"], complete)


class CommandTests(unittest.TestCase):
    def test_failure_excerpts_preserve_first_cause_and_final_status_within_bound(self):
        for module, error, name in [(packages, packages.Error, Path(os.sys.executable).name),
                                    (macos, macos.PackageError, os.sys.executable)]:
            for size in (100, 8192, 8193, 25000):
                first, last = "FIRST_CAUSE\n", "\nFINAL_CAUSE\n"
                diagnostic = first + "x" * (size - len(first) - len(last)) + last
                with self.subTest(module=module.__name__, size=size):
                    with self.assertRaises(error) as raised:
                        module.run([os.sys.executable, "-c",
                                    f"import sys;sys.stderr.write({diagnostic!r});sys.exit(17)"])
                    message = str(raised.exception)
                    prefix = f"{name} failed (17): "
                    self.assertTrue(message.startswith(prefix))
                    excerpt = message[len(prefix):]
                    if len(diagnostic) <= 8192:
                        self.assertEqual(excerpt, diagnostic)
                    else:
                        self.assertLessEqual(len(excerpt), 8192)
                        self.assertTrue(excerpt.startswith(first))
                        self.assertTrue(excerpt.endswith(last))
                        self.assertIn("[diagnostic output truncated]", excerpt)

    def test_nested_wrappers_retain_original_cause_and_both_exit_statuses(self):
        child = "import sys;sys.stderr.write('ORIGINAL_CAUSE\\n' + 'x' * 25000 + '\\nFINAL_CAUSE\\n');sys.exit(17)"
        wrapper = (
            "import importlib.util,sys\n"
            f"spec=importlib.util.spec_from_file_location('package_macos', {str(packages.ROOT / 'scripts/package-macos.py')!r})\n"
            "module=importlib.util.module_from_spec(spec)\n"
            "spec.loader.exec_module(module)\n"
            "try:\n"
            f"    module.run([sys.executable, '-c', {child!r}])\n"
            "except module.PackageError as error:\n"
            "    print(error, file=sys.stderr)\n"
            "    sys.exit(23)\n"
        )
        with self.assertRaises(packages.Error) as raised:
            packages.run([os.sys.executable, "-c", wrapper])
        message = str(raised.exception)
        prefix = f"{Path(os.sys.executable).name} failed (23): "
        self.assertTrue(message.startswith(prefix))
        self.assertIn(f"{os.sys.executable} failed (17): ORIGINAL_CAUSE\n", message)
        self.assertTrue(message.endswith("\nFINAL_CAUSE\n\n"))
        self.assertIn("[diagnostic output truncated]", message)
        self.assertLessEqual(len(message[len(prefix):]), 8192)

    def test_success_output_and_allowed_exit_are_unchanged(self):
        command = [os.sys.executable, "-c", "import sys;sys.stdout.write('complete success output')"]
        self.assertEqual(macos.run(command), "complete success output")
        self.assertEqual(packages.run(command), (0, "complete success output"))
        self.assertEqual(packages.run([os.sys.executable, "-c", "print('allowed');raise SystemExit(2)"],
                                      allowed=(0, 2)), (2, "allowed\n"))

    def test_timeout_and_output_limits_are_failures(self):
        with self.assertRaises(packages.Error):
            packages.run([os.sys.executable, "-c", "import time;time.sleep(10)"], timeout=0.01)
        with patch.object(packages, "MAX_LOG", 100), self.assertRaises(packages.Error):
            packages.run([os.sys.executable, "-c", "print('x' * 1000)"])

    def test_unexpected_command_exit_is_not_notice_blocker(self):
        with self.assertRaises(packages.Error):
            packages.run([os.sys.executable, "-c", "raise SystemExit(1)"], allowed=(0, 2))


if __name__ == "__main__":
    unittest.main()
