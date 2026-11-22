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
        # As shipped macOS has no known gaps: legacy objc crates must fail if they return.
        for missing in (["block-0.1.6"], ["leaky-cow-0.1.1", "block-0.1.6", "leak-0.1.2", "objc_exception-0.1.2"]):
            value["review_required"] = missing
            with self.subTest(shipped_missing=missing), self.assertRaisesRegex(packages.Error, "new gaps"):
                packages.notice_state(2, value, target, "a" * 64)
        # Explicit documented gaps (when configured) still withhold only on exact exit-2 subsets.
        with patch.dict(packages.KNOWN_GAPS, {target: {"sample-gap-1.0", "sample-gap-2.0"}}):
            for missing in (["sample-gap-1.0"], ["sample-gap-2.0", "sample-gap-1.0"]):
                value["review_required"] = missing
                self.assertFalse(packages.notice_state(2, value, target, "a" * 64))
            for code, missing in [(1, ["sample-gap-1.0"]), (0, ["sample-gap-1.0"]), (2, []), (2, ["new-gap-1.0"]),
                                  (2, ["mac-0.1.1"]), (2, ["sample-gap-1.0", "ufbx-0.11.3"]), (2, None)]:
                value["review_required"] = missing
                with self.subTest(code=code, missing=missing), self.assertRaises(packages.Error):
                    packages.notice_state(code, value, target, "a" * 64)

    def test_linux_has_no_known_gap_so_any_missing_notice_fails(self):
        for target in ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"):
            value = dict(target=target, cargo_lock_sha256="a" * 64, dependencies=[{}], review_required=[])
            self.assertTrue(packages.notice_state(0, value, target, "a" * 64))
            for missing in (["mac-0.1.1"], ["ufbx-0.11.3"], ["block-0.1.6"]):
                value["review_required"] = missing
                with self.subTest(target=target, missing=missing), self.assertRaisesRegex(packages.Error, "new gaps"):
                    packages.notice_state(2, value, target, "a" * 64)

    def test_strict_inventory_target_lock_and_dependency_failure(self):
        value = dict(target="x86_64-unknown-linux-gnu", cargo_lock_sha256="a" * 64,
                     dependencies=[{}], review_required=[])
        for field, wrong in [("target", "wrong"), ("cargo_lock_sha256", "b" * 64), ("dependencies", [])]:
            with self.subTest(field=field), self.assertRaises(packages.Error):
                packages.notice_state(0, {**value, field: wrong}, "x86_64-unknown-linux-gnu", "a" * 64)


class PayloadFixture:
    """A real Linux archive with its sidecars under `self.root`, and the record a job expects of it."""

    # The Linux bundle under test: its target, the label in its name and its ELF machine (EM_X86_64).
    TARGET, LABEL, ELF_MACHINE = "x86_64-unknown-linux-gnu", "linux-x86_64", 62

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.stem = f"GitTurtle-0.1.0-aaaaaaaaaaaa-{self.LABEL}-release"
        self.payload = self.root / "source" / self.stem
        (self.payload / "licenses").mkdir(parents=True)
        header = bytearray(32)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = self.ELF_MACHINE.to_bytes(2, "little")
        self.binary = bytes(header) + b"unit-test fixture, not an executable"
        (self.payload / "bin").mkdir()
        (self.payload / "bin/gitturtle").write_bytes(self.binary)
        self.compiled = dict(application="GitTurtle", source_revision="a" * 40, version="0.1.0",
                             target=self.TARGET, profile="release", source_tree="clean",
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


class PayloadTests(PayloadFixture, unittest.TestCase):
    def test_roundtrip_checks_real_archive_and_explicit_cross_platform_no_probe(self):
        with patch.object(packages.identity, "probe", return_value=self.compiled) as probe:
            payload, binary, package = packages.verify_payload(self.root, self.expected, complete=True)
            # A Linux bundle: one root directory named as the archive, the executable under bin/.
            self.assertEqual(payload, self.root / "extracted" / self.stem)
            self.assertEqual(binary, payload / "bin/gitturtle")
            self.assertEqual(binary.read_bytes(), self.binary)
            self.assertEqual(package, self.package)
            self.assertEqual(package["target"], self.TARGET)
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


class ArmLinuxPayloadTests(PayloadTests):
    """Every payload check again for the AArch64 Linux bundle (EM_AARCH64), which is a Linux bundle too."""
    TARGET, LABEL, ELF_MACHINE = "aarch64-unknown-linux-gnu", "linux-aarch64", 183


class PlatformCheckTests(unittest.TestCase):
    def test_both_linux_targets_get_the_install_checks_of_a_linux_bundle(self):
        self.assertEqual(packages.identity.LINUX_TARGETS, ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"))
        for target in packages.identity.LINUX_TARGETS:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                payload = directory / "bundle"
                package = {"target": target, "compiled_identity": {"version": "0.1.0"}}
                tools = []
                def command(command, **kwargs):
                    names = [Path(str(argument)).name for argument in command]
                    tools.append(names[1] if names[0] == Path(os.sys.executable).name else names[0])
                    if kwargs.get("allowed") == (1,):
                        return 1, "GitTurtle needs a Wayland or X11 desktop session"
                    return 0, ""
                with patch.object(packages, "run", side_effect=command), \
                        patch.object(packages.identity, "digest", return_value="d" * 64), \
                        patch.object(packages.identity, "probe", return_value=package["compiled_identity"]):
                    packages.platform_checks(payload, payload / "bin/gitturtle", package, directory)
                # The installer fixtures, an installation, its desktop entry and libraries, and a no-display start.
                self.assertEqual(tools, ["test-install-linux.py", "install.py", "desktop-file-validate", "ldd",
                                         "gitturtle"])


class PrepareTests(unittest.TestCase):
    LINUX, ARM, MAC = "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "aarch64-apple-darwin"
    LINUX_LABELS = {LINUX: "linux-x86_64", ARM: "linux-aarch64"}
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
                f"`{report['artifact_name']}`, kept 7 days. The later job `Package transfer` downloads and verifies "
                "that artifact: this job's green does not say that it did. "
                "It is an early-development CI build, not a release.\n")

    @staticmethod
    def uploaded_record_sha256(root):
        """The SHA-256 of the provenance file as `prepare` left it for the upload."""
        return sha((root / "work/publish/ci-provenance.json").read_bytes())

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
                         {self.LINUX: "linux", self.ARM: "linux", self.MAC: "macos"})
        self.assertEqual(set(packages.PLATFORM_NAMES), self.EVERYBODY)
        for undeclared in ("riscv64gc-unknown-linux-gnu", "x86_64-apple-darwin", "x86_64-pc-windows-msvc", "linux", ""):
            with self.subTest(target=undeclared), self.assertRaisesRegex(packages.Error, "no declared package platform"):
                packages.platform_of(undeclared)

    def test_both_linux_architectures_are_declared_under_their_own_labels(self):
        # The label is in the archive's and the artifact's names, and the release assets read it too.
        self.assertEqual(packages.TARGETS, {**self.LINUX_LABELS, self.MAC: "macos-arm64"})
        self.assertEqual(len(set(packages.TARGETS.values())), len(packages.TARGETS))
        # Every Linux target the packager can build is declared here, and nothing else is Linux.
        self.assertEqual({target for target in packages.TARGETS if packages.platform_of(target) == "linux"},
                         set(packages.identity.LINUX_TARGETS))
        self.assertEqual(set(packages.KNOWN_GAPS), set(packages.TARGETS))

    def test_as_shipped_complete_linux_notices_upload_the_verified_archive(self):
        cases = [(target, event, trust) for target in self.LINUX_LABELS
                 for event, trust in (("pull_request", "untrusted-pr"), ("push", "ci-build"),
                                      ("workflow_dispatch", "ci-build"))]
        names = set()
        for target, event, trust in cases:
            with self.subTest(target=target, event=event):
                root, calls, verify, checks = self.prepare(target, [], event=event)
                # The upload follows the same strict collection, complete-notice package and checks.
                self.assertEqual(len(calls), 2)
                self.assertIn("--require-complete", calls[0])
                self.assertEqual(calls[0][calls[0].index("--target") + 1], target)
                self.assertEqual(calls[1][0], "ROOT/scripts/package-linux.sh")
                self.assertIn("--distribution", calls[1])
                self.assertIs(verify.call_args.kwargs["complete"], True)
                checks.assert_called_once()
                report, output, summary = self.left_behind(root)
                self.assertIs(report["upload_eligible"], True)
                self.assertIs(report["release_input"], False)
                self.assertEqual(report["target"], target)
                self.assertEqual(report["license_review_required"], [])
                self.assertEqual(report["local_archive_checks"], "passed")
                self.assertEqual(report["hosted_transfer"], "not-performed")
                # A pull request's artifact, a fork's included, says so in its name.
                stem = f"GitTurtle-0.1.0-aaaaaaaaaaaa-{self.LINUX_LABELS[target]}-release"
                self.assertEqual(report["archive"], stem + ".tar.gz")
                self.assertEqual(report["artifact_name"], f"{stem}-{trust}-123-2")
                names.add(report["artifact_name"])
                # The answer and the name for this job's own steps, then what the later job reads under this
                # leg's label alone: the same answer and the SHA-256 of the record's bytes as uploaded.
                label, digest = self.LINUX_LABELS[target], self.uploaded_record_sha256(root)
                self.assertEqual(output, f"eligible=true\nartifact-name={report['artifact_name']}\n"
                                         f"{label}-eligible=true\n{label}-provenance-sha256={digest}\n")
                self.assertRegex(digest, r"\A[0-9a-f]{64}\Z")
                self.assertEqual((root / "work/publish/ci-provenance.json").read_bytes(),
                                 (root / "work/prepared.json").read_bytes())
                self.assertTrue(summary.startswith(f"### {self.LINUX_LABELS[target]} CI package\n"), summary)
                self.assertTrue(summary.endswith(self.uploaded(report)), summary)
                self.assertNotIn("Download unavailable", summary)
                # The upload directory holds the three archive files and the record, and nothing else.
                published = {report["archive"] + suffix for suffix in ("", ".sha256", ".manifest.json")}
                self.assertEqual({path.name for path in (root / "work/publish").iterdir()},
                                 published | {"ci-provenance.json"})
                self.assertEqual(json.loads((root / "work/publish/ci-provenance.json").read_text()), report)
                self.assertEqual(json.loads((root / "work/prepared.json").read_text()), report)
        # In one run the two Linux legs share the event, run and attempt: only the label keeps their artifacts apart.
        self.assertEqual(len(names), 4)

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
        # A held leg hands on its answer under its label and no digest: there is no uploaded record.
        self.assertEqual(output, f"eligible=false\nartifact-name={report['artifact_name']}\nmacos-arm64-eligible=false\n")
        self.assertFalse((root / "work/publish").exists())
        self.assertTrue(summary.endswith(self.held("macOS")), summary)
        self.assertNotIn("uploads the archive", summary)
        self.assertNotIn("incomplete", summary)
        # The download verifier refuses the held record even when it is handed that record's own digest
        # and identity, so nothing can stand in for the upload.
        self.assertEqual(json.loads((root / "work/prepared.json").read_text()), report)
        shutil.copyfile(root / "work/prepared.json", root / "work/prepared/ci-provenance.json")
        refused = argparse.Namespace(directory=root / "work/prepared", target=self.MAC, revision="a" * 40,
                                     expected_sha256=sha((root / "work/prepared.json").read_bytes()),
                                     run_id="123", attempt="2", artifact_id="1", report=root / "transfer.json",
                                     summary=None, output=root / "transfer-output")
        with self.assertRaisesRegex(packages.Error, "upload-eligible CI package"):
            packages.verify(refused)
        self.assertFalse((root / "transfer.json").exists())
        self.assertFalse((root / "transfer-output").exists())

    def test_its_entry_in_the_set_alone_separates_a_held_platform_from_an_uploading_one(self):
        # Each platform's shipped state beside the other one: Linux unlisted, macOS listed.
        for target, platform, unlisted, listed in ((self.LINUX, "Linux", self.NOBODY, None),
                                                   (self.ARM, "Linux", self.NOBODY, None),
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
                # Both answers under the leg's label too; only the upload hands on a digest, of its own record.
                label, named = packages.TARGETS[target], f"artifact-name={report['artifact_name']}\n"
                self.assertEqual(output, f"eligible=true\n{named}{label}-eligible=true\n"
                                         f"{label}-provenance-sha256={self.uploaded_record_sha256(root)}\n")
                self.assertEqual(held_output, f"eligible=false\n{named}{label}-eligible=false\n")
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
        for target, uploads in ((target, uploads) for target in self.LINUX_LABELS
                                for uploads in (None, self.NOBODY, self.EVERYBODY)):
            for missing in (["mac-0.1.1"], ["new-gap-1.0"]):
                with self.subTest(target=target, uploads=uploads, missing=missing):
                    root, calls, verify, checks = self.prepare(target, missing, uploads=uploads, refusal="new gaps")
                    self.assertEqual(len(calls), 1)  # the strict collector; nothing is packaged
                    self.assertIn("--require-complete", calls[0])
                    verify.assert_not_called()
                    checks.assert_not_called()
                    for withheld in ("work/prepared", "work/publish", "work/prepared.json", "report.json",
                                     "github-output", "summary.md"):
                        self.assertFalse((root / withheld).exists(), withheld)

    def test_macos_has_no_known_gaps_and_rejects_any_missing_notice(self):
        self.assertEqual(packages.KNOWN_GAPS[self.MAC], set())
        for missing in (["block-0.1.6"], ["objc_exception-0.1.2"], ["new-crate-1.0.0"]):
            with self.subTest(missing=missing):
                root, calls, verify, checks = self.prepare(self.MAC, missing, refusal="new gaps")
                self.assertEqual(len(calls), 1)
                verify.assert_not_called()
                checks.assert_not_called()
                self.assertFalse((root / "work/publish").exists())

    def test_schedule_event_verifies_package_locally_without_uploading(self):
        for target, label in self.LINUX_LABELS.items():
            with self.subTest(target=target):
                root, calls, verify, _ = self.prepare(target, [], event="schedule")
                self.assertEqual(len(calls), 2)
                self.assertIn("--distribution", calls[1])
                self.assertIs(verify.call_args.kwargs["complete"], True)
                self.assertFalse((root / "work/publish").exists())
                report, output, summary = self.left_behind(root)
                self.assertEqual(report["event"], "schedule")
                self.assertIs(report["upload_eligible"], False)
                self.assertEqual(
                    output,
                    f"eligible=false\nartifact-name={report['artifact_name']}\n{label}-eligible=false\n",
                )
                self.assertIn("Scheduled cache-seed runs do not upload CI package artifacts", summary)


LINUX_LABELS = ("linux-x86_64", "linux-aarch64")
ARTIFACT, DIGEST = "14598682882", "5e" * 32
# What neither the hand-off nor the plan may take for an artifact ID. The pinned download action reads
# its input with parseInt and splits it at commas, so each of these would name another artifact, several
# or, when empty, every artifact of the run.
NOT_AN_ID = ("", "0", "012", "12abc", "abc", "1234567890123456", " 12", "12 ", "12\n", "+12", "-12", "1.0", "1e3",
             "12,13", "0x1f", "１２", "1２", "1٢٣")


class HandOffTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.output = Path(temporary.name) / "github-output"

    def hand_off(self, target, artifact):
        with patch("builtins.print") as printed:
            packages.hand_off(argparse.Namespace(target=target, artifact_id=artifact, output=self.output))
        return printed

    def test_the_uploaded_id_is_written_under_the_legs_own_label_alone(self):
        for target, label in zip(packages.identity.LINUX_TARGETS, LINUX_LABELS):
            for artifact in ("1", ARTIFACT, "9" * 15):
                with self.subTest(target=target, artifact=artifact):
                    self.output.write_text("eligible=true\n")
                    printed = self.hand_off(target, artifact)
                    # Appended to what the job's earlier steps wrote, and nothing for the other label.
                    self.assertEqual(self.output.read_text(), f"eligible=true\n{label}-artifact-id={artifact}\n")
                    printed.assert_called_once_with(f"{label}: the uploaded artifact's ID is handed to the later job, "
                                                    "which downloads and verifies it")

    def test_anything_but_a_plain_id_is_refused_without_being_written_or_printed(self):
        for artifact in NOT_AN_ID:
            with self.subTest(artifact=artifact):
                with self.assertRaises(packages.Error) as refused:
                    self.hand_off("aarch64-unknown-linux-gnu", artifact)
                self.assertEqual(str(refused.exception),
                                 "linux-aarch64: the upload returned no artifact ID of the expected form")
                self.assertFalse(self.output.exists())


class TransferPlanTests(unittest.TestCase):
    X86, ARM = packages.identity.LINUX_TARGETS
    MALFORMED = "The release job's outputs are missing or malformed"

    @staticmethod
    def handed(label="linux-x86_64", eligible="true", artifact=ARTIFACT, digest=DIGEST):
        """A release job's outputs with one label's three; None leaves a name unset, as an empty output is."""
        named = {"eligible": eligible, "artifact-id": artifact, "provenance-sha256": digest}
        return {f"{label}-{name}": value for name, value in named.items() if value is not None}

    def refusal(self, outputs, target=X86):
        raw = outputs if isinstance(outputs, str) else json.dumps(outputs)
        with self.assertRaises(packages.Error) as refused:
            packages.transfer_plan(raw, target)
        return str(refused.exception)

    def test_an_uploaded_package_is_planned_from_its_own_labels_three_outputs(self):
        both = {**self.handed(), **self.handed("linux-aarch64", artifact="77", digest="0f" * 32)}
        self.assertEqual(packages.transfer_plan(json.dumps(both), self.X86), (ARTIFACT, DIGEST))
        self.assertEqual(packages.transfer_plan(json.dumps(both), self.ARM), ("77", "0f" * 32))
        for artifact in ("1", "9" * 15):
            self.assertEqual(packages.transfer_plan(json.dumps(self.handed(artifact=artifact)), self.X86), (artifact, DIGEST))
        # A leg judges its own label alone: the other leg's job refuses the other label's values.
        broken = {**self.handed(), **self.handed("linux-aarch64", artifact="12abc", digest="")}
        self.assertEqual(packages.transfer_plan(json.dumps(broken), self.X86), (ARTIFACT, DIGEST))
        self.assertEqual(self.refusal(broken, self.ARM), "linux-aarch64: the release leg's artifact ID is missing or malformed")

    def test_a_held_package_is_planned_only_when_nothing_else_was_handed_on(self):
        for empty in (None, ""):
            with self.subTest(empty=empty):
                held = self.handed(eligible="false", artifact=empty, digest=empty)
                self.assertIsNone(packages.transfer_plan(json.dumps(held), self.X86))
        for artifact, digest in ((ARTIFACT, None), (None, DIGEST), (ARTIFACT, DIGEST), ("0", ""), ("", "x")):
            with self.subTest(artifact=artifact, digest=digest):
                self.assertEqual(self.refusal(self.handed(eligible="false", artifact=artifact, digest=digest)),
                                 "linux-x86_64: the release leg held its package, yet handed on an artifact ID or a "
                                 "provenance SHA-256")

    def test_an_upload_without_a_plain_id_and_a_whole_digest_is_refused_by_name(self):
        for artifact in (None, *NOT_AN_ID):
            with self.subTest(artifact=artifact):
                self.assertEqual(self.refusal(self.handed(artifact=artifact)),
                                 "linux-x86_64: the release leg's artifact ID is missing or malformed")
        for digest in (None, "", "5e" * 31, "5e" * 31 + "5", "5e" * 32 + "5", "5E" * 32, "5g" * 32, "5e" * 32 + "\n",
                       " " + "5e" * 32, "sha256:" + "5e" * 32):
            with self.subTest(digest=digest):
                self.assertEqual(self.refusal(self.handed(digest=digest)),
                                 "linux-x86_64: the release leg's provenance SHA-256 is missing or malformed")

    def test_an_answer_that_is_neither_true_nor_false_is_refused_whatever_came_with_it(self):
        for eligible in (None, "", "True", "TRUE", "yes", "1", " true", "true\n", "false ", "null"):
            for rest in ({}, {"artifact": None, "digest": None}):
                with self.subTest(eligible=eligible, rest=rest):
                    self.assertEqual(self.refusal(self.handed(eligible=eligible, **rest)),
                                     "linux-x86_64: the release leg's `eligible` answer is missing or is neither `true` "
                                     "nor `false`")
        # The other leg's complete outputs do not stand in for this leg's.
        self.assertIn("`eligible` answer is missing", self.refusal(self.handed("linux-aarch64")))

    def test_outputs_that_are_not_one_object_of_strings_are_refused(self):
        duplicate = '{"linux-x86_64-eligible": "false", "linux-x86_64-eligible": "true"}'
        for raw in ("", "null", "[]", '"true"', "{", "true", "7", duplicate,
                    json.dumps({**self.handed(), "linux-x86_64-eligible": True}),
                    json.dumps({**self.handed(), "linux-x86_64-artifact-id": 14598682882}),
                    json.dumps({**self.handed(), "other": {"nested": "value"}}),
                    json.dumps({**self.handed(), "padding": "x" * packages.MAX_OUTPUTS})):
            with self.subTest(raw=raw[:60]):
                self.assertEqual(self.refusal(raw), self.MALFORMED)

    def test_outputs_are_read_at_the_bound_and_refused_one_character_past_it(self):
        room = packages.MAX_OUTPUTS - len(json.dumps({**self.handed(), "padding": ""}))
        at_the_bound = json.dumps({**self.handed(), "padding": "x" * room})
        self.assertEqual(len(at_the_bound), packages.MAX_OUTPUTS)
        self.assertEqual(packages.transfer_plan(at_the_bound, self.X86), (ARTIFACT, DIGEST))
        # The same object with one space before its closing brace.
        self.assertEqual(self.refusal(at_the_bound[:-1] + " }"), self.MALFORMED)

    def test_no_refusal_repeats_a_value_it_was_handed(self):
        marker = "VALUE-7f3a"
        for outputs in (self.handed(artifact=marker), self.handed(digest=marker), self.handed(eligible=marker),
                        self.handed(eligible="false", artifact=marker), {marker: 1}, marker,
                        f'{{"{marker}": "a", "{marker}": "b"}}'):
            with self.subTest(outputs=outputs):
                self.assertNotIn(marker, self.refusal(outputs))


class PlanStepTests(unittest.TestCase):
    """`plan` as the later job's first step: the outputs the next steps read, or a failed step that writes none."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.output, self.summary = self.root / "github-output", self.root / "summary.md"

    def plan(self, outputs, target="x86_64-unknown-linux-gnu"):
        arguments = argparse.Namespace(target=target, output=self.output, summary=self.summary)
        with patch.dict(os.environ, {packages.RELEASE_OUTPUTS: json.dumps(outputs)}), patch("builtins.print") as printed:
            packages.plan(arguments)
        return printed.call_args.args[0]

    def test_an_upload_gives_the_validated_id_and_digest_to_the_next_steps_and_no_verdict(self):
        printed = self.plan(TransferPlanTests.handed())
        self.assertEqual(self.output.read_text(), f"verify=true\nartifact-id={ARTIFACT}\nprovenance-sha256={DIGEST}\n")
        self.assertFalse(self.summary.exists())
        self.assertEqual(printed, "linux-x86_64: The release leg handed on an artifact ID and a provenance SHA-256 of the "
                                  "expected form; this job downloads that artifact once and verifies it.")

    def test_a_held_package_skips_the_next_steps_and_is_recorded_as_held_under_its_label(self):
        for target, label in zip(packages.identity.LINUX_TARGETS, LINUX_LABELS):
            with self.subTest(target=target):
                self.output.unlink(missing_ok=True)
                self.summary.unlink(missing_ok=True)
                printed = self.plan(TransferPlanTests.handed(label, eligible="false", artifact=None, digest=None), target)
                self.assertEqual(self.output.read_text(), f"verify=false\n{label}-transfer=held\n")
                sentence = ("The release leg held this package: nothing was uploaded, so nothing is downloaded or "
                            "verified here.")
                self.assertEqual(self.summary.read_text(), f"### {label} package transfer\n\n{sentence}\n")
                self.assertEqual(printed, f"{label}: {sentence}")

    def test_a_refusal_fails_the_step_by_name_and_writes_no_output(self):
        script = packages.ROOT / "scripts/ci/packages.py"
        cases = [(TransferPlanTests.handed(artifact="12abc"), "linux-x86_64: the release leg's artifact ID is missing or malformed"),
                 (TransferPlanTests.handed(digest="5e" * 31), "linux-x86_64: the release leg's provenance SHA-256 is missing or malformed"),
                 (TransferPlanTests.handed(eligible="false"), "linux-x86_64: the release leg held its package, yet handed on an "
                                                              "artifact ID or a provenance SHA-256"),
                 ({}, "linux-x86_64: the release leg's `eligible` answer is missing or is neither `true` nor `false`"),
                 (None, "The release job's outputs are missing or malformed")]
        for outputs, message in cases:
            with self.subTest(message=message):
                status, diagnostic = packages.run(
                    [os.sys.executable, "-B", script, "plan", "--target", "x86_64-unknown-linux-gnu",
                     "--output", self.output, "--summary", self.summary],
                    env={packages.RELEASE_OUTPUTS: json.dumps(outputs)}, allowed=(1,))
                self.assertEqual((status, diagnostic), (1, f"CI package verification failed: {message}\n"))
                self.assertFalse(self.output.exists())
                self.assertFalse(self.summary.exists())
        # The variable itself is required: an unset one is not an empty set of outputs.
        status, diagnostic = packages.run([os.sys.executable, "-B", script, "plan", "--target", "x86_64-unknown-linux-gnu",
                                           "--output", self.output], env={}, allowed=(1,))
        self.assertEqual(diagnostic, "CI package verification failed: The release job's outputs are missing or malformed\n")
        self.assertFalse(self.output.exists())


class DownloadFailedTests(unittest.TestCase):
    WORDS = ("The package artifact was not returned intact at this job's one download attempt, or it is past its 7 days "
             "of retention. A re-run is not the remedy: this is the owner's question of the later job against no "
             "read-back in Quality.")

    def test_the_step_prints_fixed_words_as_one_annotation_and_names_nothing(self):
        self.assertEqual(packages.DOWNLOAD_FAILED, self.WORDS)
        # One line, and nothing of a run in it: no address, and no figure but the retention.
        self.assertNotIn("\n", self.WORDS)
        self.assertEqual(re.findall(r"[0-9]+|https?:|/|`|\$", self.WORDS), [str(packages.UPLOAD_RETENTION_DAYS)])
        with tempfile.TemporaryDirectory() as temporary:
            summary = Path(temporary) / "summary.md"
            summary.write_text("earlier\n")
            script = packages.ROOT / "scripts/ci/packages.py"
            status, printed = packages.run([os.sys.executable, "-B", script, "download-failed", "--summary", summary])
            self.assertEqual((status, printed), (0, f"::error::{self.WORDS}\n"))
            self.assertEqual(summary.read_text(), f"earlier\n\n**{self.WORDS}**\n")
            # It takes nothing to print: no artifact, run or path option exists.
            for option in ("--artifact-id", "--run-id", "--directory", "--output"):
                with self.subTest(option=option):
                    status, _ = packages.run([os.sys.executable, "-B", script, "download-failed", option, "1"], allowed=(2,))
                    self.assertEqual(status, 2)


class TransferFixture(PayloadFixture):
    """A download directory with the four files a release leg uploaded, and what that leg handed on."""

    RUN, UPLOADED_IN, VERIFIED_IN = "123", "1", "2"

    def setUp(self):
        super().setUp()
        self.download = self.root / "download"
        self.download.mkdir()
        self.names = [self.expected["archive"] + suffix for suffix in ("", ".sha256", ".manifest.json")]
        for name in self.names:
            shutil.copyfile(self.root / name, self.download / name)
        self.record = dict(format=1, **self.expected, upload_eligible=True, license_review_required=[],
                           artifact_name=f"{self.stem}-ci-build-{self.RUN}-{self.UPLOADED_IN}", event="push",
                           run_id=self.RUN, run_attempt=self.UPLOADED_IN, release_input=False,
                           native_smoke="not-performed", hosted_transfer="not-performed",
                           local_archive_checks="passed", signing="unsigned")
        self.report, self.summary, self.output = (self.root / "reports/package-transfer.json", self.root / "summary.md",
                                                  self.root / "github-output")
        self.upload(self.record)

    def upload(self, record):
        """Put a record in the download as its release leg would have written it, and hand on its SHA-256."""
        raw = (json.dumps(record, indent=2) + "\n").encode()
        (self.download / "ci-provenance.json").write_bytes(raw)
        self.handed = sha(raw)

    def arguments(self, **changed):
        return argparse.Namespace(**{**dict(
            directory=self.download, expected_sha256=self.handed, target=self.TARGET, revision="a" * 40, run_id=self.RUN,
            attempt=self.VERIFIED_IN, artifact_id=ARTIFACT, report=self.report, summary=self.summary, output=self.output),
            **changed})

    def verify(self, refusal=None, error=packages.Error, checks=None, **changed):
        """Run `verify` with only the executable and the install checks stood in; a refusal leaves no word."""
        with patch.object(packages.identity, "probe", return_value=self.compiled), \
                patch.object(packages, "platform_checks", side_effect=checks) as installed:
            if refusal is None:
                packages.verify(self.arguments(**changed))
            else:
                with self.assertRaisesRegex(error, refusal):
                    packages.verify(self.arguments(**changed))
                self.assertFalse(self.output.exists(), "a refused transfer wrote its output")
        return installed

    def again(self):
        """Leave the download as the action left it, and no trace of an earlier verification."""
        shutil.rmtree(self.download / "extracted", ignore_errors=True)
        shutil.rmtree(self.download / "install-home", ignore_errors=True)
        shutil.rmtree(self.root / "reports", ignore_errors=True)
        for path in (self.summary, self.output):
            if path.is_dir():
                path.rmdir()
            path.unlink(missing_ok=True)


class TransferVerifyTests(TransferFixture, unittest.TestCase):
    def test_a_bound_record_gets_the_full_checks_then_the_report_the_summary_and_last_the_word(self):
        seen = []
        installed = self.verify(checks=lambda *_: seen.append((self.report.exists(), self.output.exists())))
        # The same payload and install checks as `prepare` ran, on the downloaded bytes.
        payload = self.download / "extracted" / self.stem
        installed.assert_called_once_with(payload, payload / "bin/gitturtle", self.package, self.download)
        self.assertEqual(seen, [(False, False)])
        # The report is the uploaded record with the transfer's own fields: which bytes, which ID, which attempts.
        self.assertEqual(json.loads(self.report.read_text()),
                         {**self.record, "hosted_transfer": "passed", "artifact_id": ARTIFACT, "native_smoke": "not-performed",
                          "provenance_sha256": self.handed, "verified_run_attempt": self.VERIFIED_IN})
        # The later job's summary starts with this: the heading a held package's plan writes too.
        self.assertEqual(self.summary.read_text(),
                         f"### {self.LABEL} package transfer\n\n"
                         f"Actual Actions artifact `{ARTIFACT}`, uploaded by attempt 1 of this run, downloaded once and "
                         f"verified in attempt 2: `{self.record['archive_sha256']}`. Package checks passed; native desktop "
                         "acceptance remains separate.\n")
        self.assertEqual(self.output.read_text(), f"{self.LABEL}-transfer=verified\n")

    def test_the_record_is_hashed_before_it_is_read_and_one_changed_byte_refuses_it(self):
        refusal = "Downloaded CI provenance differs from what the release leg handed on"
        path = self.download / "ci-provenance.json"
        intact = path.read_bytes()
        flipped = bytearray(intact)
        flipped[len(flipped) // 2] ^= 1
        # A changed byte, a record that would not even parse, another leg's digest, an empty file.
        for raw, handed in ((bytes(flipped), self.handed), (b"{not a record", self.handed), (intact, "0" * 64),
                            (intact, sha(intact + b"\n")), (b"", self.handed)):
            with self.subTest(raw=raw[:20], handed=handed[:8]):
                self.again()
                path.write_bytes(raw)
                with patch.object(packages.json, "loads", side_effect=AssertionError("read before it was bound")), \
                        patch.object(packages, "verify_payload", side_effect=AssertionError("checked before it was bound")):
                    installed = self.verify(refusal, expected_sha256=handed)
                installed.assert_not_called()
                self.assertFalse(self.report.exists())
        # Bound and still unreadable: its own failure, and no word either.
        self.again()
        path.write_bytes(b"{not a record")
        self.verify("Expecting property name", error=ValueError, expected_sha256=sha(b"{not a record"))
        for raw in (b"[]", b'"record"'):
            self.again()
            path.write_bytes(raw)
            self.verify("Expected a JSON object", expected_sha256=sha(raw))
        # Only a bounded regular file is read at all.
        self.again()
        path.unlink()
        path.symlink_to(self.root / (self.expected["archive"] + ".manifest.json"))
        self.verify("Expected a bounded regular file", expected_sha256=self.expected["manifest_sha256"])
        path.unlink()
        path.write_bytes(intact)
        with patch.object(packages.identity, "MAX_JSON", len(intact) - 1):
            self.verify("Expected a bounded regular file")

    def test_a_record_of_another_target_revision_or_run_or_of_a_later_attempt_is_refused(self):
        other = next(target for target in packages.identity.LINUX_TARGETS if target != self.TARGET)
        cases = [("target", other, "for another target than this job's"),
                 ("target", "aarch64-apple-darwin", "for another target than this job's"),
                 ("revision", "c" * 40, "for another revision than this job's"),
                 ("revision", "a" * 39, "for another revision than this job's"),
                 ("run_id", "124", "from another run than this job's"),
                 ("run_id", 123, "from another run than this job's"),
                 ("run_attempt", "3", "from no numbered attempt or from a later one than this job's"),
                 ("run_attempt", "10", "from no numbered attempt or from a later one than this job's"),
                 ("run_attempt", 1, "from no numbered attempt or from a later one than this job's"),
                 ("run_attempt", "", "from no numbered attempt or from a later one than this job's"),
                 ("run_attempt", "1 ", "from no numbered attempt or from a later one than this job's"),
                 ("upload_eligible", False, "upload-eligible CI package"), ("upload_eligible", "true", "upload-eligible CI package"),
                 ("release_input", True, "upload-eligible CI package"), ("format", 2, "upload-eligible CI package")]
        for key, value, refusal in cases:
            with self.subTest(key=key, value=value):
                self.again()
                # The leg handed on this very record's digest: only what the record says refuses it.
                self.upload({**self.record, key: value})
                with patch.object(packages, "verify_payload", side_effect=AssertionError("checked before it was bound")):
                    self.verify(refusal).assert_not_called()
                self.assertFalse(self.report.exists())
        for key in ("target", "revision", "run_id", "run_attempt"):
            with self.subTest(missing=key):
                self.again()
                self.upload({name: value for name, value in self.record.items() if name != key})
                self.verify("Downloaded CI provenance is ")
        # The same from the job's side: its own target, revision, run and attempt are what the record must name.
        self.upload(self.record)
        for changed, refusal in (({"target": other}, "another target"), ({"revision": "c" * 40}, "another revision"),
                                 ({"run_id": "124"}, "another run"), ({"attempt": "0"}, "a later one than this job's")):
            with self.subTest(job=changed):
                self.again()
                self.verify(refusal, **changed)

    def test_an_upload_of_this_attempt_or_an_earlier_one_is_accepted(self):
        # A re-run of the later job alone reads the release leg's earlier attempt; the report names both.
        for uploaded, verifying in (("1", "1"), ("1", "2"), ("2", "2"), ("2", "10"), ("9", "10")):
            with self.subTest(uploaded=uploaded, verifying=verifying):
                self.again()
                self.upload({**self.record, "run_attempt": uploaded})
                self.verify(attempt=verifying)
                report = json.loads(self.report.read_text())
                self.assertEqual((report["run_attempt"], report["verified_run_attempt"]), (uploaded, verifying))
                self.assertIn(f"uploaded by attempt {uploaded} of this run, downloaded once and verified in attempt "
                              f"{verifying}: ", self.summary.read_text())
                self.assertEqual(self.output.read_text(), f"{self.LABEL}-transfer=verified\n")

    def test_the_word_is_written_only_after_every_check_the_report_and_the_summary(self):
        archive = self.download / self.expected["archive"]

        def tampered():
            archive.write_bytes(archive.read_bytes() + b"changed")

        def restored():
            shutil.copyfile(self.root / archive.name, archive)

        # What fails, how the download is prepared for it, and what leaves the download as it was.
        cases = [("the archive's bytes", tampered, restored, {}, packages.Error, "differs from the prepared bytes"),
                 ("an extra file", lambda: (self.download / "other.txt").write_text("x"),
                  lambda: (self.download / "other.txt").unlink(), {}, packages.Error, "missing or unexpected files"),
                 ("a missing sidecar", lambda: (self.download / self.names[1]).unlink(),
                  lambda: shutil.copyfile(self.root / self.names[1], self.download / self.names[1]), {}, packages.Error,
                  "missing or unexpected files"),
                 ("the install checks", None, None, {"checks": packages.Error("Installed package has unresolved native libraries")},
                  packages.Error, "unresolved native libraries"),
                 ("the report", lambda: (self.report.parent.mkdir(), self.report.write_text("{}")), None, {}, FileExistsError, ""),
                 ("the summary", lambda: self.summary.mkdir(), None, {}, IsADirectoryError, "")]
        for name, prepare, restore, options, error, refusal in cases:
            with self.subTest(failing=name):
                self.again()
                if prepare:
                    prepare()
                self.verify(refusal, error=error, **options)
                if restore:
                    restore()
        # The summary's failure came after the report was written: a report alone is not the verdict.
        self.assertTrue(self.report.exists())
        # Nothing above was the fixture's fault: the untouched download is verified.
        self.again()
        self.verify()
        self.assertEqual(self.output.read_text(), f"{self.LABEL}-transfer=verified\n")

    def test_malformed_job_arguments_are_refused_before_the_download_is_read(self):
        cases = [({"artifact_id": artifact}, "Expected uploaded artifact ID") for artifact in NOT_AN_ID]
        cases += [({"expected_sha256": digest}, "Expected the SHA-256 of the release leg's provenance")
                  for digest in ("", self.handed[:63], self.handed.upper(), self.handed + "0", self.handed + "\n")]
        cases += [(changed, "Expected numeric GitHub run identity")
                  for changed in ({"run_id": ""}, {"run_id": "12a"}, {"attempt": ""}, {"attempt": "x"}, {"attempt": "-1"})]
        for changed, refusal in cases:
            with self.subTest(changed=changed), \
                    patch.object(packages, "bound_provenance", side_effect=AssertionError("read the download")):
                self.verify(refusal, **changed)

    def test_the_command_fails_by_itself_on_a_download_directory_the_action_did_not_fill(self):
        # The one download step fails the job itself; this is what stands behind it. The verify command
        # exits non-zero when the directory is absent, empty or partly filled, and records no transfer.
        (self.root / "empty").mkdir()
        (self.root / "partial").mkdir()
        shutil.copyfile(self.download / "ci-provenance.json", self.root / "partial/ci-provenance.json")
        for directory in ("absent", "empty", "partial"):
            with self.subTest(directory=directory):
                status, diagnostic = packages.run(
                    [os.sys.executable, "-B", packages.ROOT / "scripts/ci/packages.py", "verify",
                     "--directory", self.root / directory, "--expected-sha256", self.handed, "--target", self.TARGET,
                     "--revision", "a" * 40, "--run-id", self.RUN, "--attempt", self.VERIFIED_IN, "--artifact-id", ARTIFACT,
                     "--report", self.report, "--output", self.output], allowed=(1,))
                self.assertEqual(status, 1)
                self.assertTrue(diagnostic.startswith("CI package verification failed: "), diagnostic)
                self.assertFalse(self.report.exists())
                self.assertFalse(self.output.exists())


class ArmTransferVerifyTests(TransferVerifyTests):
    """Every binding again for the AArch64 Linux leg, whose word carries its own label."""
    TARGET, LABEL, ELF_MACHINE = "aarch64-unknown-linux-gnu", "linux-aarch64", 183


class TransferRouteTests(PayloadFixture, unittest.TestCase):
    """One package from `prepare` to the later job's word; only the tools and the executable are stood in."""

    def release_leg(self, work, output, uploads=packages.HOSTED_UPLOAD_PLATFORMS):
        """`prepare` over the fixture's real archive and, as the workflow does on its answer alone, the
        hand-off of the ID the upload returned. Returns everything the leg's steps wrote as outputs."""
        binary = self.root / "target" / self.TARGET / "release/gitturtle"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(self.binary)

        def tool(command, **_):
            command = list(map(str, command))
            if "--require-complete" in command:
                notices = Path(command[-1])
                notices.mkdir()
                (notices / "dependencies.json").write_text(json.dumps(dict(
                    target=self.TARGET, cargo_lock_sha256="b" * 64, dependencies=[{}], review_required=[])))
                return 0, ""
            # The packager: the archive and its two sidecars beside the path it was given.
            for suffix in ("", ".sha256", ".manifest.json"):
                shutil.copyfile(self.root / (self.stem + ".tar.gz" + suffix), Path(command[-1] + ".tar.gz" + suffix))
            return 0, ""

        prepare = argparse.Namespace(target=self.TARGET, revision="a" * 40, directory=work, event="pull_request",
                                     run_id="123", attempt="1", output=output, summary=None, report=None)
        with patch.object(packages, "ROOT", self.root), patch.object(packages, "run", side_effect=tool), \
                patch.object(packages, "HOSTED_UPLOAD_PLATFORMS", uploads), \
                patch.object(packages.identity, "expectations", return_value=("a" * 40, "0.1.0", "b" * 64, "clean")), \
                patch.object(packages.identity, "probe", return_value=self.compiled), \
                patch.object(packages, "platform_checks"), patch("builtins.print"):
            packages.prepare(prepare)
            if "eligible=true\n" in output.read_text():
                packages.hand_off(argparse.Namespace(target=self.TARGET, artifact_id=ARTIFACT, output=output))
        return dict(line.split("=", 1) for line in output.read_text().splitlines())

    def later_job(self, needs, download, **changed):
        """The plan, then `verify` on a download with what the plan gave it; returns the job's outputs."""
        planned, output = self.root / "plan-output", self.root / "transfer-output"
        with patch.dict(os.environ, {packages.RELEASE_OUTPUTS: json.dumps(needs)}), patch("builtins.print"):
            packages.plan(argparse.Namespace(target=self.TARGET, output=planned, summary=None))
        plan = dict(line.split("=", 1) for line in planned.read_text().splitlines())
        planned.unlink()
        if plan["verify"] != "true":
            return plan
        arguments = argparse.Namespace(**{**dict(
            directory=download, expected_sha256=plan["provenance-sha256"], target=self.TARGET, revision="a" * 40,
            run_id="123", attempt="1", artifact_id=plan["artifact-id"], report=self.root / "transfer.json", summary=None,
            output=output), **changed})
        with patch.object(packages.identity, "probe", return_value=self.compiled), patch.object(packages, "platform_checks"):
            packages.verify(arguments)
        return dict(line.split("=", 1) for line in output.read_text().splitlines())

    def test_what_the_release_leg_writes_is_what_the_plan_accepts_and_verify_binds(self):
        work = self.root / "work"
        written = self.release_leg(work, self.root / "release-output")
        # The job declares its label's three outputs; `needs` shows the later job exactly these.
        needs = {name: value for name, value in written.items() if name.startswith(f"{self.LABEL}-")}
        self.assertEqual(needs, {f"{self.LABEL}-eligible": "true", f"{self.LABEL}-artifact-id": ARTIFACT,
                                 f"{self.LABEL}-provenance-sha256": sha((work / "publish/ci-provenance.json").read_bytes())})
        # The download: the four files the leg uploaded, byte for byte.
        shutil.copytree(work / "publish", self.root / "download")
        self.assertEqual(self.later_job(needs, self.root / "download"), {f"{self.LABEL}-transfer": "verified"})
        report = json.loads((self.root / "transfer.json").read_text())
        self.assertEqual((report["hosted_transfer"], report["artifact_id"], report["provenance_sha256"]),
                         ("passed", ARTIFACT, needs[f"{self.LABEL}-provenance-sha256"]))
        # The uploaded record itself still says what was true when it was written.
        self.assertEqual(json.loads((work / "publish/ci-provenance.json").read_text())["hosted_transfer"], "not-performed")
        # One changed byte of the downloaded record, and the same hand-off refuses it.
        shutil.copytree(work / "publish", self.root / "changed")
        record = self.root / "changed/ci-provenance.json"
        raw = bytearray(record.read_bytes())
        raw[raw.index(b"not-performed")] ^= 1
        record.write_bytes(raw)
        (self.root / "transfer.json").unlink()
        (self.root / "transfer-output").unlink()
        with self.assertRaisesRegex(packages.Error, "differs from what the release leg handed on"):
            self.later_job(needs, self.root / "changed")
        self.assertFalse((self.root / "transfer.json").exists())
        self.assertFalse((self.root / "transfer-output").exists())

    def test_a_leg_whose_platform_is_held_ends_as_held_without_a_download(self):
        # No platform uploads: `prepare` holds the package, and neither the upload nor the hand-off runs.
        work = self.root / "work"
        written = self.release_leg(work, self.root / "release-output", uploads=frozenset())
        needs = {name: value for name, value in written.items() if name.startswith(f"{self.LABEL}-")}
        self.assertEqual(needs, {f"{self.LABEL}-eligible": "false"})
        self.assertFalse((work / "publish").exists())
        self.assertEqual(self.later_job(needs, self.root / "absent"), {"verify": "false", f"{self.LABEL}-transfer": "held"})


class ArmTransferRouteTests(TransferRouteTests):
    TARGET, LABEL, ELF_MACHINE = "aarch64-unknown-linux-gnu", "linux-aarch64", 183


class QualityUploadTests(unittest.TestCase):
    """What quality.yml does with `prepare`'s answer: the release job's upload and hand-off, and the later job
    that downloads the upload once and verifies it. Read as text (no YAML dependency); comments are not read."""

    ELIGIBLE = "steps.package.outputs.eligible == 'true'"
    PLANNED = "steps.plan.outputs.verify == 'true'"
    # A step's own `outcome` is its result; `failure()` lets a step run once an earlier one has failed.
    DOWNLOAD_FAILED = "failure() && steps.download.outcome == 'failure'"
    CHECKOUT = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1"
    UPLOAD = "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2"
    DOWNLOAD = "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2"
    RELEASE_LEGS = [("macos-26", "aarch64-apple-darwin"), ("ubuntu-24.04", "x86_64-unknown-linux-gnu"),
                    ("ubuntu-24.04-arm", "aarch64-unknown-linux-gnu")]
    TRANSFER_STEPS = ["uses: checkout", "Read what the release leg handed on and refuse anything malformed",
                      "Install the runtime libraries and the desktop-entry tools",
                      "Download the release leg's exact artifact from Actions", "Say what a failed download is",
                      "Verify downloaded bytes and installation", "Summarize CI measurements",
                      "Preserve CI measurements and sanitized diagnostics"]

    def job(self, name):
        """A job's text without its comments, and its steps: each one's own keys, then the lines of its
        `with:`, `env:` and `run:` blocks."""
        text = (packages.ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
        blocks = re.split(r"^  ([a-z][a-z0-9-]*):\n", text.split("\njobs:\n", 1)[1], flags=re.M)
        job = "\n".join(line for line in dict(zip(blocks[1::2], blocks[2::2]))[name].splitlines()
                        if not line.lstrip().startswith("#"))
        steps = []
        for step in ("\n" + job.split("\n    steps:\n", 1)[1]).split("\n      - ")[1:]:
            lines = ("        " + step).splitlines()
            own = dict(re.fullmatch(r"        ([a-z-]+):(?: (.*))?", line).groups() for line in lines if line[8:9].strip())
            steps.append((own, [line.strip() for line in lines if line.startswith("          ")]))
        return job, steps

    def scripts(self, name):
        """The shell text of each of a job's `run` steps: the one line, or the block under `run: |`."""
        found = []
        for own, lines in self.job(name)[1]:
            if "run" in own:
                # A step's `env:` block precedes its script; its entries are `NAME: value`.
                block = [line for line in lines if not re.match(r"[A-Z][A-Z0-9_]*: ", line)]
                found.append(own["run"] if own["run"] != "|" else "\n".join(block))
        return found

    @staticmethod
    def named(steps):
        return [own.get("name", "uses: " + own.get("uses", "").split("@")[0].split("/")[-1]) for own, _ in steps]

    @staticmethod
    def uploading():
        """Each target whose platform uploads its package, with its label: the later job's legs."""
        return {target: label for target, label in packages.TARGETS.items()
                if packages.platform_of(target) in packages.HOSTED_UPLOAD_PLATFORMS}

    @staticmethod
    def outputs(job):
        """A job's declared outputs, in its own order."""
        block = re.search(r"^    outputs:\n((?:      .*\n)+)", job, re.M).group(1)
        return [tuple(line.strip().split(": ", 1)) for line in block.splitlines()]

    def test_only_prepare_decides_the_upload_and_the_hand_off_and_the_summary_states_the_real_retention(self):
        release, steps = self.job("rust-release")
        gated = [(own, lines) for own, lines in steps if "outputs.eligible" in own.get("if", "")]
        self.assertEqual(release.count("eligible =="), len(gated))
        # The upload, then the hand-off of its ID: nothing else in the job reads the answer.
        self.assertEqual([own["name"] for own, _ in gated], ["Upload verified early-development CI package",
                                                             "Hand the uploaded artifact's ID to the later job"])
        self.assertEqual([own["if"] for own, _ in gated], [self.ELIGIBLE, self.ELIGIBLE])
        upload, inputs = gated[0]
        self.assertEqual((upload["id"], upload["uses"]), ("uploaded", self.UPLOAD))
        # Only the directory `prepare` fills for an eligible package is ever uploaded as a package.
        self.assertEqual(inputs, ["name: ${{ steps.package.outputs.artifact-name }}",
                                  "path: ${{ runner.temp }}/ci-package/publish/*",
                                  f"retention-days: {packages.UPLOAD_RETENTION_DAYS}", "compression-level: 0",
                                  "if-no-files-found: error", "include-hidden-files: false"])
        self.assertEqual(packages.UPLOAD_RETENTION_DAYS, 7)
        # No run-time input reaches the decision: neither job reads a workflow input or a repository variable.
        for name in ("rust-release", "package-transfer"):
            for override in ("inputs.", "vars.", "github.event.inputs"):
                self.assertNotIn(override, self.job(name)[0])
        package, = (lines for own, lines in steps if own.get("id") == "package")
        self.assertEqual(sorted(set(re.findall(r"--[a-z-]+", " ".join(package).split("packages.py prepare", 1)[1]))),
                         ["--attempt", "--directory", "--event", "--output", "--report", "--revision", "--run-id",
                          "--summary", "--target"])

    def test_every_declared_target_has_one_release_leg_on_its_own_architecture(self):
        release, steps = self.job("rust-release")
        self.assertIn("\n    name: Rust release · ${{ matrix.os }}\n    runs-on: ${{ matrix.os }}\n", "\n" + release)
        legs = re.findall(r"^          - os: (\S+)\n            target: (\S+)$", release, re.M)
        self.assertEqual(legs, self.RELEASE_LEGS)
        # The names the release preflight requires of a Quality run.
        self.assertEqual([f"Rust release · {runner}" for runner, _ in legs],
                         ["Rust release · macos-26", "Rust release · ubuntu-24.04", "Rust release · ubuntu-24.04-arm"])
        # One leg per declared target: none is packaged twice and none is left out.
        self.assertEqual(sorted(target for _, target in legs), sorted(packages.TARGETS))
        # A failed leg keeps the others' results, and no leg waits for another.
        self.assertIn("\n      fail-fast: false\n", release)
        self.assertEqual(re.findall(r"^      max-parallel: (\d+)$", release, re.M), [str(len(legs))])
        # Each leg installs, builds, packages and hands on its own matrix target, and bounds that target's cache.
        self.assertEqual(release.count("PACKAGE_TARGET: ${{ matrix.target }}"), 4)
        self.assertEqual(re.findall(r"^          target: (.*)$", release, re.M), ["${{ matrix.target }}"])
        # Nothing but the Xcode selection depends on the runner, so the AArch64 Linux leg skips no step that
        # the x86-64 one runs: the same setup, build, strict notices, package, upload and hand-off.
        self.assertEqual(sorted({own["if"] for own, _ in steps if "if" in own}),
                         ["always()", "runner.os == 'macOS'", self.ELIGIBLE])
        # Neither job tells its legs apart in an expression: a per-target value is written by packages.py
        # under the target's label, from the target the leg passes it.
        selectors = ("runner.arch ==", "runner.arch !=", "matrix.os ==", "matrix.os !=", "matrix.target ==",
                     "matrix.target !=", "contains(matrix.", "startsWith(matrix.")
        for name, barred in (("rust-release", selectors), ("package-transfer", (*selectors, "runner.os ==", "runner.os !="))):
            for selector in barred:
                self.assertNotIn(selector, self.job(name)[0], name)
        # The two Linux legs share a job id and an OS: only the architecture keeps their diagnostics apart.
        self.assertEqual(re.findall(r"^          name: (ci-diagnostics-.*)$", release, re.M),
                         ["ci-diagnostics-${{ github.job }}-${{ runner.os }}-${{ runner.arch }}-"
                          "${{ github.run_id }}-${{ github.run_attempt }}"])

    def test_a_release_leg_hands_on_its_own_labels_three_outputs_through_the_helper_alone(self):
        release, steps = self.job("rust-release")
        labels = list(self.uploading().values())
        self.assertEqual(labels, ["linux-x86_64", "linux-aarch64"])
        # Six outputs: an answer and a provenance digest from `prepare`, an artifact ID from the hand-off.
        self.assertEqual(self.outputs(release), [
            (f"{label}-{name}", f"${{{{ steps.{step}.outputs.{label}-{name} }}}}") for label in labels
            for name, step in (("eligible", "package"), ("provenance-sha256", "package"), ("artifact-id", "handoff"))])
        # The hand-off directly follows the upload and passes the uploaded ID through the environment.
        names = self.named(steps)
        upload = names.index("Upload verified early-development CI package")
        own, lines = steps[upload + 1]
        self.assertEqual(own, {"name": "Hand the uploaded artifact's ID to the later job", "id": "handoff",
                               "if": self.ELIGIBLE, "env": None,
                               "run": 'python3 scripts/ci/packages.py hand-off --target "$PACKAGE_TARGET" '
                                      '--artifact-id "$PACKAGE_ARTIFACT_ID" --output "$GITHUB_OUTPUT"'})
        self.assertEqual(lines, ["PACKAGE_TARGET: ${{ matrix.target }}",
                                 "PACKAGE_ARTIFACT_ID: ${{ steps.uploaded.outputs.artifact-id }}"])
        # No step's script holds an expression: every value reaches a command through `env`.
        for name, count in (("rust-release", 7), ("package-transfer", 5)):
            scripts = self.scripts(name)
            self.assertEqual(len(scripts), count, name)
            for script in scripts:
                self.assertNotIn("${{", script)
                self.assertRegex(script, r"\A(?:python3 scripts/|printf |rustup |mkdir )")
        self.assertEqual([own["id"] for own, _ in steps if "id" in own], ["package", "uploaded", "handoff"])

    def test_the_later_job_is_the_uploading_release_legs_and_nothing_it_does_not_need(self):
        transfer, steps = self.job("package-transfer")
        # Everything above its outputs: it needs the release job, follows the product lane as that job does,
        # runs each leg on the label in its name, and its matrix is two `include` entries and nothing else.
        self.assertEqual(transfer.split("    outputs:\n", 1)[0],
                         "    needs: [changes, rust-release]\n"
                         "    if: needs.changes.outputs.product == 'true' && needs.changes.outputs.seed-only != 'true'\n"
                         "    name: Package transfer · ${{ matrix.os }}\n    runs-on: ${{ matrix.os }}\n"
                         "    timeout-minutes: 30\n    strategy:\n      fail-fast: false\n      matrix:\n        include:\n"
                         "          - os: ubuntu-24.04\n            target: x86_64-unknown-linux-gnu\n"
                         "          - os: ubuntu-24.04-arm\n            target: aarch64-unknown-linux-gnu\n")
        # A leg for each release leg whose platform uploads its package, on the label that built it.
        legs = re.findall(r"^          - os: (\S+)\n            target: (\S+)$", transfer, re.M)
        self.assertEqual(legs, [(runner, target) for runner, target in self.RELEASE_LEGS if target in self.uploading()])
        self.assertEqual(self.RELEASE_LEGS, re.findall(r"^          - os: (\S+)\n            target: (\S+)$",
                                                       self.job("rust-release")[0], re.M))
        # Its word under each label: the verification's, else the plan's for a held package.
        self.assertEqual(self.outputs(transfer), [
            (f"{label}-transfer", f"${{{{ steps.verify.outputs.{label}-transfer || steps.plan.outputs.{label}-transfer }}}}")
            for label in self.uploading().values()])
        # The inherited read-only token, no credential, no cache, no setup action, no container, no job-wide variable.
        for absent in ("permissions:", "secrets", "token", "cache", "./.github/actions/", "container:", "\n    env:\n",
                       "max-parallel:", "defaults:"):
            self.assertNotIn(absent, transfer, absent)
        self.assertEqual([own["uses"] for own, _ in steps if "uses" in own], [self.CHECKOUT, self.DOWNLOAD, self.UPLOAD])
        self.assertEqual(steps[0], ({"uses": self.CHECKOUT, "with": None}, ["persist-credentials: false"]))

    def test_the_later_jobs_steps_follow_the_plan_which_alone_reads_what_was_handed_on(self):
        transfer, steps = self.job("package-transfer")
        self.assertEqual(self.named(steps), self.TRANSFER_STEPS)
        self.assertEqual([own.get("id") for own, _ in steps], [None, "plan", None, "download", None, "verify", None, None])
        # The installation, the download and the verification run on the plan's answer; the last two always.
        self.assertEqual([own.get("if") for own, _ in steps],
                         [None, None, self.PLANNED, self.PLANNED, self.DOWNLOAD_FAILED, self.PLANNED, "always()", "always()"])
        for function, count in (("failure()", 1), ("always()", 2), ("cancelled()", 0), ("success()", 0)):
            self.assertEqual(transfer.count(function), count, function)
        # The plan is the first step after the checkout, so it refuses before the download action runs. It
        # reads the release job's outputs and its own target through the environment and nothing else.
        own, lines = steps[1]
        self.assertEqual(own, {"name": self.TRANSFER_STEPS[1], "id": "plan", "env": None,
                               "run": 'python3 scripts/ci/packages.py plan --target "$PACKAGE_TARGET" '
                                      '--output "$GITHUB_OUTPUT" --summary "$GITHUB_STEP_SUMMARY"'})
        self.assertEqual(lines, [f"{packages.RELEASE_OUTPUTS}: ${{{{ toJSON(needs.rust-release.outputs) }}}}",
                                 "PACKAGE_TARGET: ${{ matrix.target }}"])
        # No other step reads the release job: what the later steps take is the plan's validated output.
        body = transfer.split("\n    steps:\n", 1)[1]
        self.assertEqual(body.count("needs."), 1)
        self.assertEqual(sorted(set(re.findall(r"steps\.([a-z]+)\.outputs\.([a-z0-9-]+)", body))),
                         [("plan", "artifact-id"), ("plan", "provenance-sha256"), ("plan", "verify")])

    def test_the_artifact_is_downloaded_once_by_the_validated_id_and_no_failure_is_tolerated_or_waited_on(self):
        workflow = (packages.ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
        (release, release_steps), (transfer, steps) = self.job("rust-release"), self.job("package-transfer")
        downloads = [(own, inputs) for own, inputs in steps if "download-artifact" in own.get("uses", "")]
        (own, inputs), = downloads
        self.assertEqual(own, {"name": self.TRANSFER_STEPS[3], "id": "download", "if": self.PLANNED, "uses": self.DOWNLOAD,
                               "with": None})
        # One exact ID, the plan's, and a refused digest mismatch; no name, pattern, run or token input.
        self.assertEqual(inputs, ["artifact-ids: ${{ steps.plan.outputs.artifact-id }}",
                                  "path: ${{ runner.temp }}/ci-package-download", "merge-multiple: true",
                                  "digest-mismatch: error"])
        # The release job downloads nothing, and nothing else in the workflow downloads an artifact.
        self.assertEqual([own for own, _ in release_steps if "download-artifact" in own.get("uses", "")], [])
        self.assertEqual(len(re.findall(r"^[^#\n]*download-artifact", workflow, re.M)), 1)
        # In the two jobs together nothing tolerates a failure, waits or repeats a step.
        for name, job, read in (("rust-release", release, release_steps), ("package-transfer", transfer, steps)):
            with self.subTest(job=name):
                self.assertNotIn("continue-on-error", job)
                self.assertNotRegex(job, r"\bsleep\b")
                named = [own["name"] for own, _ in read if "name" in own]
                self.assertEqual(len(named), len(set(named)))
                self.assertEqual([step for step in named if re.search(r"attempt|again|wait", step, re.I)], [])
        self.assertEqual([own["name"] for own, _ in release_steps if "name" in own][-5:],
                         ["Upload verified early-development CI package", "Hand the uploaded artifact's ID to the later job",
                          "Prepare bounded successful-main cache save", "Summarize CI measurements",
                          "Preserve CI measurements and sanitized diagnostics"])

    def test_a_failed_download_alone_runs_the_step_that_says_what_it_is(self):
        _, steps = self.job("package-transfer")
        names = self.named(steps)
        own, lines = steps[names.index("Say what a failed download is")]
        # Directly after the download, on that step's own failure: not after a refused plan or a failed installation,
        # which skip the download, and not when a later step fails.
        self.assertEqual(names.index("Say what a failed download is"), names.index(self.TRANSFER_STEPS[3]) + 1)
        self.assertEqual(own, {"name": "Say what a failed download is", "if": self.DOWNLOAD_FAILED,
                               "run": 'python3 scripts/ci/packages.py download-failed --summary "$GITHUB_STEP_SUMMARY"'})
        self.assertEqual(lines, [])
        self.assertEqual(steps[names.index(self.TRANSFER_STEPS[3])][0]["id"], "download")

    def test_the_full_verification_runs_in_the_later_job_under_the_collector_with_the_jobs_own_identity(self):
        (release, release_steps), (transfer, steps) = self.job("rust-release"), self.job("package-transfer")
        own, lines = steps[self.named(steps).index("Verify downloaded bytes and installation")]
        self.assertEqual(own, {"name": "Verify downloaded bytes and installation", "id": "verify", "if": self.PLANNED,
                               "env": None, "run": "|"})
        # The job's own target, commit, run and attempt, and the plan's validated ID and digest: no token.
        self.assertEqual(lines, [
            "PACKAGE_TARGET: ${{ matrix.target }}", "PACKAGE_REVISION: ${{ github.sha }}",
            "PACKAGE_RUN_ID: ${{ github.run_id }}", "PACKAGE_ATTEMPT: ${{ github.run_attempt }}",
            "PACKAGE_ARTIFACT_ID: ${{ steps.plan.outputs.artifact-id }}",
            "PACKAGE_PROVENANCE_SHA256: ${{ steps.plan.outputs.provenance-sha256 }}",
            'python3 scripts/ci/metrics.py measure --name artifact-download-checks --directory "$RUNNER_TEMP/ci-metrics" -- \\',
            'python3 scripts/ci/packages.py verify --directory "$RUNNER_TEMP/ci-package-download" \\',
            '--expected-sha256 "$PACKAGE_PROVENANCE_SHA256" --target "$PACKAGE_TARGET" --revision "$PACKAGE_REVISION" \\',
            '--run-id "$PACKAGE_RUN_ID" --attempt "$PACKAGE_ATTEMPT" --artifact-id "$PACKAGE_ARTIFACT_ID" \\',
            '--report "$RUNNER_TEMP/ci-metrics/reports/package-transfer.json" --summary "$GITHUB_STEP_SUMMARY" \\',
            '--output "$GITHUB_OUTPUT"'])
        # The directory is the one the download fills, and the release job neither verifies nor measures a download.
        self.assertIn("path: ${{ runner.temp }}/ci-package-download", steps[3][1])
        for moved in ("packages.py verify", "artifact-download-checks", "package-transfer.json", "ci-package-download"):
            self.assertNotIn(moved, release, moved)
            self.assertIn(moved, transfer, moved)
        # The summary and the diagnostics upload are the release job's own two last steps, word for word; the
        # artifact's name holds the job and the architecture, which keep the two legs' and the two jobs' apart.
        self.assertEqual(steps[-2:], release_steps[-2:])
        self.assertEqual(steps[-1][1][0], "name: ci-diagnostics-${{ github.job }}-${{ runner.os }}-${{ runner.arch }}-"
                                          "${{ github.run_id }}-${{ github.run_attempt }}")
        self.assertEqual([own.get("if") for own, _ in steps[-2:]], ["always()", "always()"])


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
