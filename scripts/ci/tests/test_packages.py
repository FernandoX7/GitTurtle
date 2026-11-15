from __future__ import annotations

import argparse
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
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
        target = "aarch64-apple-darwin"
        value = dict(target=target, cargo_lock_sha256="a" * 64, dependencies=[{}], review_required=[])
        self.assertTrue(packages.notice_state(0, value, target, "a" * 64))
        for missing in (["block-0.1.6"], ["leaky-cow-0.1.1", "block-0.1.6", "leak-0.1.2", "objc_exception-0.1.2"]):
            value["review_required"] = missing
            self.assertFalse(packages.notice_state(2, value, target, "a" * 64))
        # The resolved `mac` and the long-replaced `ufbx` 0.11.3 would be new gaps again.
        for code, missing in [(1, ["block-0.1.6"]), (0, ["block-0.1.6"]), (2, []), (2, ["new-gap-1.0"]),
                              (2, ["mac-0.1.1"]), (2, ["block-0.1.6", "ufbx-0.11.3"]), (2, None)]:
            value["review_required"] = missing
            with self.subTest(code=code, missing=missing), self.assertRaises(packages.Error):
                packages.notice_state(code, value, target, "a" * 64)

    def test_linux_has_no_known_gap_so_any_missing_notice_fails(self):
        target = "x86_64-unknown-linux-gnu"
        value = dict(target=target, cargo_lock_sha256="a" * 64, dependencies=[{}], review_required=[])
        self.assertTrue(packages.notice_state(0, value, target, "a" * 64))
        for missing in (["mac-0.1.1"], ["ufbx-0.11.3"], ["block-0.1.6"]):
            value["review_required"] = missing
            with self.subTest(missing=missing), self.assertRaises(packages.Error):
                packages.notice_state(2, value, target, "a" * 64)

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
        self.package = dict(format=2, application="GitTurtle", bundle_id="com.gitturtle.GitTurtle",
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
    LINUX, MAC = "x86_64-unknown-linux-gnu", "aarch64-apple-darwin"
    NOBODY, EVERYBODY = frozenset(), frozenset({"linux", "macos"})
    MAC_HELD = "the macOS upload is held until macOS distribution is decided"
    INCOMPLETE = ("**Download unavailable: C0 license notices are incomplete; C3 hosted transfer remains open.** "
                  "The development archive is withheld from public upload. Verified strict refusal: "
                  "`block-0.1.6`, `leak-0.1.2`, `leaky-cow-0.1.1`, `objc_exception-0.1.2`.")

    @staticmethod
    def held(platform):
        return (f"Complete notices verified. **Download unavailable: the {platform} upload is held until {platform} "
                "distribution is decided; C3 hosted transfer remains open.** The verified complete-notice archive "
                "stays in this job and is not uploaded.\n")

    @staticmethod
    def uploaded(report):
        return ("Complete notices verified. This job uploads the archive next as the Actions artifact "
                f"`{report['artifact_name']}`, kept 7 days, then downloads and verifies that artifact. "
                "It is an early-development CI build, not a release.\n")

    def prepare(self, target, missing, *, uploads=None, refusal=None, event="pull_request"):
        """Run `prepare` over stand-in tools; `missing` is what strict collection reports.

        `uploads` replaces the set of uploading platforms for this run; None leaves it as shipped.
        """
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        binary = root / "target" / target / "release/gitturtle"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"fixture input")
        args = argparse.Namespace(target=target, revision="a" * 40, directory=root / "work",
                                  event=event, run_id="123", attempt="2", output=root / "github-output",
                                  summary=root / "summary.md", report=root / "report.json")
        calls = []
        def command(command, **kwargs):
            command = list(map(str, command))
            calls.append([argument.replace(str(root), "ROOT") for argument in command])
            if "--require-complete" in command:
                notices = Path(command[-1])
                notices.mkdir()
                (notices / "dependencies.json").write_text(json.dumps(dict(
                    target=target, cargo_lock_sha256="b" * 64, dependencies=[{}], review_required=missing)))
                return (2 if missing else 0), ""
            stem = f"GitTurtle-0.1.0-aaaaaaaaaaaa-{packages.TARGETS[target]}-release"
            archive = root / "work/prepared" / (stem + (".tar.gz" if target.endswith("linux-gnu") else ".zip"))
            for suffix in ("", ".sha256", ".manifest.json"):
                Path(str(archive) + suffix).write_bytes(b"unit fixture" + suffix.encode())
            return 0, ""
        package = {"signing": "unsigned" if target.endswith("linux-gnu") else "ad-hoc"}
        with patch.object(packages, "HOSTED_UPLOAD_PLATFORMS",
                          packages.HOSTED_UPLOAD_PLATFORMS if uploads is None else uploads), \
                patch.object(packages, "ROOT", root), \
                patch.object(packages.identity, "expectations", return_value=("a" * 40, "0.1.0", "b" * 64, "clean")), \
                patch.object(packages.identity, "probe", return_value={}), \
                patch.object(packages.identity, "validate_identity"), \
                patch.object(packages, "run", side_effect=command), \
                patch.object(packages, "verify_payload", return_value=(root, binary, package)) as verify, \
                patch.object(packages, "platform_checks") as checks, patch("builtins.print"):
            if refusal:
                with self.assertRaisesRegex(packages.Error, refusal):
                    packages.prepare(args)
            else:
                packages.prepare(args)
        return root, calls, verify, checks

    def left_behind(self, root):
        """What a finished `prepare` leaves for the workflow and for its readers."""
        return (json.loads((root / "report.json").read_text()), (root / "github-output").read_text(),
                (root / "summary.md").read_text())

    def test_each_platform_packages_once_and_incomplete_notices_never_publish(self):
        # The upload path on every platform, which only a listed platform reaches: all are listed here.
        for target in packages.TARGETS:
            # A target with no known gap left has no development path: any gap fails it.
            known = sorted(packages.KNOWN_GAPS[target])[:1]
            for complete in (False, True):
                with self.subTest(target=target, complete=complete):
                    if not complete and not known:
                        root, calls, verify, _ = self.prepare(target, ["new-gap-1.0"], uploads=self.EVERYBODY,
                                                              refusal="new gaps")
                        self.assertEqual(len(calls), 1)  # the strict collector; nothing is packaged
                        verify.assert_not_called()
                        for withheld in ("work/prepared", "work/publish", "report.json", "github-output"):
                            self.assertFalse((root / withheld).exists(), withheld)
                        continue
                    root, calls, verify, _ = self.prepare(target, [] if complete else known, uploads=self.EVERYBODY)
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

    def test_the_shipped_upload_set_is_linux_alone(self):
        self.assertEqual(packages.HOSTED_UPLOAD_PLATFORMS, frozenset({"linux"}))
        # Every declared target has exactly one platform, so none can upload unlisted.
        self.assertEqual({target: packages.platform_of(target) for target in packages.TARGETS},
                         {self.LINUX: "linux", self.MAC: "macos"})
        self.assertEqual(set(packages.PLATFORM_NAMES), self.EVERYBODY)
        # A target the packager knows but this helper does not declare has no platform here either.
        self.assertIn("aarch64-unknown-linux-gnu", packages.identity.LINUX_TARGETS)
        for undeclared in ("aarch64-unknown-linux-gnu", "riscv64gc-unknown-linux-gnu", "x86_64-apple-darwin",
                           "x86_64-pc-windows-msvc", "linux", ""):
            with self.subTest(target=undeclared), self.assertRaisesRegex(packages.Error, "no declared package platform"):
                packages.platform_of(undeclared)

    def test_as_shipped_complete_linux_notices_upload_the_verified_archive(self):
        for event, trust in (("pull_request", "untrusted-pr"), ("push", "ci-build"), ("workflow_dispatch", "ci-build")):
            with self.subTest(event=event):
                root, calls, verify, checks = self.prepare(self.LINUX, [], event=event)
                # The upload follows the same strict collection, complete-notice package and checks.
                self.assertEqual(len(calls), 2)
                self.assertIn("--require-complete", calls[0])
                self.assertIn("--distribution", calls[1])
                self.assertIs(verify.call_args.kwargs["complete"], True)
                checks.assert_called_once()
                report, output, summary = self.left_behind(root)
                self.assertIs(report["upload_eligible"], True)
                self.assertIs(report["release_input"], False)
                self.assertEqual(report["license_review_required"], [])
                self.assertEqual(report["local_archive_checks"], "passed")
                self.assertEqual(report["hosted_transfer"], "not-performed")
                # A pull request's artifact, a fork's included, says so in its name.
                self.assertEqual(report["artifact_name"],
                                 f"GitTurtle-0.1.0-aaaaaaaaaaaa-linux-x86_64-release-{trust}-123-2")
                self.assertEqual(output, f"eligible=true\nartifact-name={report['artifact_name']}\n")
                self.assertTrue(summary.endswith(self.uploaded(report)), summary)
                self.assertNotIn("Download unavailable", summary)
                # The upload directory holds the three archive files and the record, and nothing else.
                published = {report["archive"] + suffix for suffix in ("", ".sha256", ".manifest.json")}
                self.assertEqual({path.name for path in (root / "work/publish").iterdir()},
                                 published | {"ci-provenance.json"})
                self.assertEqual(json.loads((root / "work/publish/ci-provenance.json").read_text()), report)
                self.assertEqual(json.loads((root / "work/prepared.json").read_text()), report)

    def test_as_shipped_macos_stays_held_even_with_complete_notices(self):
        # macOS's notices are not complete today; this is the day they are.
        root, calls, verify, checks = self.prepare(self.MAC, [])
        # The hold skips neither the strict collection nor the complete-notice package.
        self.assertEqual(len(calls), 2)
        self.assertIn("--require-complete", calls[0])
        self.assertIn("--distribution", calls[1])
        self.assertIs(verify.call_args.kwargs["complete"], True)
        checks.assert_called_once()
        report, output, summary = self.left_behind(root)
        self.assertIs(report["upload_eligible"], False)
        self.assertEqual(report["license_review_required"], [])
        self.assertEqual(report["local_archive_checks"], "passed")
        self.assertEqual(report["hosted_transfer"], "not-performed")
        self.assertEqual(output, f"eligible=false\nartifact-name={report['artifact_name']}\n")
        self.assertFalse((root / "work/publish").exists())
        self.assertTrue(summary.endswith(self.held("macOS")), summary)
        self.assertNotIn("uploads the archive", summary)
        self.assertNotIn("incomplete", summary)
        # The download verifier refuses the held record, so nothing can stand in for the upload.
        self.assertEqual(json.loads((root / "work/prepared.json").read_text()), report)
        refused = argparse.Namespace(expected=root / "work/prepared.json", directory=root / "work/prepared",
                                     artifact_id="1", report=root / "transfer.json", summary=None)
        with self.assertRaisesRegex(packages.Error, "upload-eligible CI package"):
            packages.verify(refused)
        self.assertFalse((root / "transfer.json").exists())

    def test_its_entry_in_the_set_alone_separates_a_held_platform_from_an_uploading_one(self):
        # Each platform's shipped state beside the other one: Linux unlisted, macOS listed.
        for target, platform, unlisted, listed in ((self.LINUX, "Linux", self.NOBODY, None),
                                                   (self.MAC, "macOS", None, self.EVERYBODY)):
            with self.subTest(target=target):
                held_root, held_calls, held_verify, _ = self.prepare(target, [], uploads=unlisted)
                root, calls, verify, _ = self.prepare(target, [], uploads=listed)
                # Same collection, same package, same verification.
                self.assertEqual(calls, held_calls)
                self.assertEqual(verify.call_args.kwargs, held_verify.call_args.kwargs)
                held_report, held_output, held_summary = self.left_behind(held_root)
                report, output, summary = self.left_behind(root)
                self.assertEqual(report.keys(), held_report.keys())
                self.assertEqual({key for key in report if report[key] != held_report[key]}, {"upload_eligible"})
                self.assertIs(report["upload_eligible"], True)
                self.assertEqual(output, f"eligible=true\nartifact-name={report['artifact_name']}\n")
                self.assertEqual(held_output, output.replace("eligible=true", "eligible=false"))
                self.assertTrue(summary.endswith(self.uploaded(report)), summary)
                self.assertEqual(held_summary.removesuffix(self.held(platform)),
                                 summary.removesuffix(self.uploaded(report)))
                # On disk only the upload's own directory differs: the three archive files and the record.
                published = {report["archive"] + suffix for suffix in ("", ".sha256", ".manifest.json")}
                self.assertEqual({path.name for path in (root / "work/publish").iterdir()},
                                 published | {"ci-provenance.json"})
                for name in published:
                    self.assertEqual((root / "work/publish" / name).read_bytes(),
                                     (root / "work/prepared" / name).read_bytes())
                self.assertEqual(json.loads((root / "work/publish/ci-provenance.json").read_text()), report)
                def outside_publish(base):
                    return {str(path.relative_to(base)) for path in base.rglob("*") if "publish" not in path.parts}
                self.assertEqual(outside_publish(held_root), outside_publish(root))
                self.assertFalse((held_root / "work/publish").exists())

    def test_missing_linux_notice_fails_whatever_the_upload_set_lists(self):
        for uploads in (None, self.NOBODY, self.EVERYBODY):
            for missing in (["mac-0.1.1"], ["new-gap-1.0"]):
                with self.subTest(uploads=uploads, missing=missing):
                    root, calls, verify, checks = self.prepare(self.LINUX, missing, uploads=uploads, refusal="new gaps")
                    self.assertEqual(len(calls), 1)  # the strict collector; nothing is packaged
                    self.assertIn("--require-complete", calls[0])
                    verify.assert_not_called()
                    checks.assert_not_called()
                    for withheld in ("work/prepared", "work/publish", "work/prepared.json", "report.json",
                                     "github-output", "summary.md"):
                        self.assertFalse((root / withheld).exists(), withheld)

    def test_macos_known_gaps_withhold_the_archive_whatever_the_upload_set_lists(self):
        gaps = sorted(packages.KNOWN_GAPS[self.MAC])
        self.assertEqual(gaps, ["block-0.1.6", "leak-0.1.2", "leaky-cow-0.1.1", "objc_exception-0.1.2"])
        seen = []
        # As shipped the summary also says that completing the notices would not start an upload.
        for uploads, ending in ((None, f"{self.INCOMPLETE} Separately, {self.MAC_HELD}.\n"),
                                (self.EVERYBODY, f"{self.INCOMPLETE}\n")):
            with self.subTest(uploads=uploads):
                root, calls, verify, _ = self.prepare(self.MAC, gaps, uploads=uploads)
                self.assertEqual(len(calls), 2)
                self.assertNotIn("--distribution", calls[1])
                self.assertIs(verify.call_args.kwargs["complete"], False)
                self.assertFalse((root / "work/publish").exists())
                report, output, summary = self.left_behind(root)
                self.assertIs(report["upload_eligible"], False)
                self.assertEqual(report["license_review_required"], gaps)
                self.assertEqual(output, f"eligible=false\nartifact-name={report['artifact_name']}\n")
                self.assertTrue(summary.endswith(ending), summary)
                self.assertNotIn("Complete notices verified", summary)
                seen.append((calls, report, output, summary.removesuffix(ending)))
        self.assertEqual(seen[0], seen[1])


class QualityUploadTests(unittest.TestCase):
    """What quality.yml does with `prepare`'s answer; read as text (no YAML dependency)."""

    def steps(self):
        text = (packages.ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
        job = re.split(r"^  ([a-z][a-z0-9-]*):\n", text.split("\njobs:\n", 1)[1], flags=re.M)
        release = dict(zip(job[1::2], job[2::2]))["rust-release"]
        return release, release.split("\n    steps:\n", 1)[1].split("\n      - ")

    def test_only_prepare_decides_the_upload_and_its_summary_states_the_real_retention(self):
        release, steps = self.steps()
        gated = [step for step in steps if "outputs.eligible" in step]
        self.assertEqual(release.count("eligible =="), len(gated))
        self.assertEqual([step.splitlines()[0] for step in gated],
                         ["name: Upload verified early-development CI package",
                          "name: Download this exact uploaded artifact from Actions",
                          "name: Verify downloaded bytes and installation"])
        for step in gated:
            with self.subTest(step=step.splitlines()[0]):
                self.assertEqual(re.findall(r"^ +if: (.*)$", step, re.M), ["steps.package.outputs.eligible == 'true'"])
        upload = gated[0]
        self.assertIn("\n          name: ${{ steps.package.outputs.artifact-name }}\n", upload)
        # Only the directory `prepare` fills for an eligible package is ever uploaded as a package.
        self.assertEqual(re.findall(r"^ +path: (.*)$", upload, re.M), ["${{ runner.temp }}/ci-package/publish/*"])
        self.assertEqual(re.findall(r"^ +retention-days: (.*)$", upload, re.M), [str(packages.UPLOAD_RETENTION_DAYS)])
        self.assertEqual(packages.UPLOAD_RETENTION_DAYS, 7)
        # No run-time input reaches the decision: the job reads no workflow input or repository variable.
        for override in ("inputs.", "vars.", "github.event.inputs"):
            self.assertNotIn(override, release)
        package = next(step for step in steps if "\n        id: package\n" in step)
        self.assertEqual(sorted(set(re.findall(r"--[a-z-]+", package.split("packages.py prepare", 1)[1]))),
                         ["--attempt", "--directory", "--event", "--output", "--report", "--revision", "--run-id",
                          "--summary", "--target"])


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
