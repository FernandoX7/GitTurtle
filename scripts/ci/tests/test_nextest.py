"""Pinned cargo-nextest installation and the Quality tests-step policy; no network required."""

import contextlib
import importlib.util
import hashlib
import io
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib
import unittest
from unittest import mock
import urllib.error


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ci_tools", ROOT / "scripts/ci/tools.py")
tools = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tools)


def archive(members):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        for name, data, kind in members:
            info = tarfile.TarInfo(name)
            info.type = kind
            if kind == tarfile.SYMTYPE:
                info.linkname = "/bin/sh"
            else:
                info.size = len(data)
            tar.addfile(info, io.BytesIO(data) if kind != tarfile.SYMTYPE else None)
    return buffer.getvalue()


def rust_debug_steps():
    text = (ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
    job = text.split("\n  rust-debug:\n", 1)[1].split("\n  rust-release:\n", 1)[0]
    return job, job.split("\n      - ")[1:]


class InstallerTests(unittest.TestCase):
    def test_pins_are_exact_versioned_release_archives(self):
        self.assertRegex(tools.NEXTEST_VERSION, r"^\d+\.\d+\.\d+$")
        self.assertEqual(set(tools.NEXTEST_ARCHIVES), {("Linux", "X64"), ("macOS", "ARM64")})
        self.assertTrue(tools.NEXTEST_RELEASE.startswith("https://github.com/nextest-rs/nextest/releases/download/"))
        self.assertIn(f"cargo-nextest-{tools.NEXTEST_VERSION}/", tools.NEXTEST_RELEASE)
        for name, sha256 in tools.NEXTEST_ARCHIVES.values():
            self.assertTrue(name.startswith(f"cargo-nextest-{tools.NEXTEST_VERSION}-") and name.endswith(".tar.gz"))
            self.assertRegex(sha256, r"^[0-9a-f]{64}$")

    def test_verified_member_returns_the_executable_only_for_the_pinned_digest(self):
        data = archive([("cargo-nextest", b"binary", tarfile.REGTYPE)])
        digest = hashlib.sha256(data).hexdigest()
        self.assertEqual(tools.verified_member(data, digest, "cargo-nextest"), b"binary")
        with self.assertRaisesRegex(tools.ToolError, "checksum mismatch"):
            tools.verified_member(data + b"x", digest, "cargo-nextest")

    def test_missing_or_linked_member_is_refused(self):
        for members in ([("other", b"x", tarfile.REGTYPE)], [("cargo-nextest", b"", tarfile.SYMTYPE)]):
            data = archive(members)
            with self.subTest(members=members[0][0]), self.assertRaises(tools.ToolError):
                tools.verified_member(data, hashlib.sha256(data).hexdigest(), "cargo-nextest")
        with self.assertRaises(tools.ToolError):
            tools.verified_member(b"not a tarball", hashlib.sha256(b"not a tarball").hexdigest(), "cargo-nextest")

    def test_download_retries_transport_failures_but_not_oversized_bodies(self):
        response = mock.MagicMock()
        response.__enter__.return_value = response
        response.geturl.return_value = "https://release-assets.githubusercontent.com/archive"
        response.read.return_value = b"abc"
        failure = urllib.error.URLError("reset")
        with mock.patch.object(tools.urllib.request, "urlopen", side_effect=[failure, response]) as opened, \
                mock.patch.object(tools.time, "sleep"):
            self.assertEqual(tools.download("https://example.invalid/a", limit=3), b"abc")
        self.assertEqual(opened.call_count, 2)
        response.read.return_value = b"abcd"
        with mock.patch.object(tools.urllib.request, "urlopen", return_value=response) as opened:
            with self.assertRaisesRegex(tools.ToolError, "size limit"):
                tools.download("https://example.invalid/a", limit=3)
        self.assertEqual(opened.call_count, 1)
        with mock.patch.object(tools.urllib.request, "urlopen", side_effect=failure), \
                mock.patch.object(tools.time, "sleep"), self.assertRaisesRegex(tools.ToolError, "3 attempts"):
            tools.download("https://example.invalid/a")

    def install(self, directory, version_output, platform=("Linux", "X64"), returncode=0):
        data = archive([("cargo-nextest", b"#!/bin/sh\n", tarfile.REGTYPE)])
        pins = {platform: ("cargo-nextest-test.tar.gz", hashlib.sha256(data).hexdigest())}
        completed = subprocess.CompletedProcess([], returncode, stdout=version_output, stderr="")
        path_file = Path(directory) / "github-path"
        with mock.patch.object(tools, "NEXTEST_ARCHIVES", pins), \
                mock.patch.object(tools, "download", return_value=data), \
                mock.patch.object(tools.subprocess, "run", return_value=completed) as run, \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            status = tools.main(["install-nextest", "--directory", str(Path(directory) / "bin"),
                                 "--path-file", str(path_file), "--os", "Linux", "--arch", "X64"])
        return status, path_file, run

    def test_install_adds_the_directory_to_path_only_after_cargo_resolves_the_pin(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            status, path_file, run = self.install(directory, f"cargo-nextest {tools.NEXTEST_VERSION} (abc 2026-09-21)\n")
            self.assertEqual(status, 0)
            binary = Path(directory) / "bin/cargo-nextest"
            self.assertEqual(binary.read_bytes(), b"#!/bin/sh\n")
            self.assertTrue(binary.stat().st_mode & 0o111)
            self.assertEqual(path_file.read_text(), f"{binary.parent.resolve()}\n")
            self.assertEqual(run.call_args.args[0], ["cargo", "nextest", "--version"])
            self.assertTrue(run.call_args.kwargs["env"]["PATH"].startswith(str(binary.parent.resolve())))

    def test_a_different_resolved_version_or_platform_fails_without_touching_path(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            status, path_file, _ = self.install(directory, "cargo-nextest 0.9.100 (old)\n")
            self.assertEqual(status, 1)
            self.assertFalse(path_file.exists())
            status, path_file, _ = self.install(directory, "", returncode=101)
            self.assertEqual(status, 1)
            status, path_file, _ = self.install(directory, "", platform=("Linux", "ARM64"))
            self.assertEqual(status, 1)
            self.assertFalse(path_file.exists())


class WorkflowPolicyTests(unittest.TestCase):
    def test_tests_run_with_pinned_nextest_then_doctests_before_clippy_and_finish(self):
        job, steps = rust_debug_steps()
        index = {name: next(i for i, step in enumerate(steps) if marker in step) for name, marker in {
            "setup": "uses: ./.github/actions/setup-rust\n        with:\n          profile: debug",
            "install": "scripts/ci/tools.py install-nextest",
            "tests": "cargo nextest run",
            "doctests": "--name doctests ",
            "clippy": "cargo clippy",
            "finish": "phase: finish",
        }.items()}
        self.assertEqual(sorted(index, key=index.get), ["setup", "install", "tests", "doctests", "clippy", "finish"])
        self.assertTrue(steps[index["tests"]].rstrip().endswith(
            '--name tests --directory "$RUNNER_TEMP/ci-metrics" -- cargo nextest run --locked --workspace -P ci --no-fail-fast --timings'))
        # Default selection reuses the nextest build; the filter matches only
        # doctest names (`src/lib.rs - item (line 3)`), never a test path.
        self.assertTrue(steps[index["doctests"]].rstrip().endswith(
            '-- cargo test --locked --workspace --timings -- "(line "'))
        self.assertIn('--path-file "$GITHUB_PATH"', steps[index["install"]])
        # The only other Cargo test invocation in the job runs doctests.
        self.assertEqual(job.count("cargo test "), 1)
        for name in ("install", "tests", "doctests"):
            self.assertNotIn("\n        if:", steps[index[name]])
        self.assertNotIn("permissions:", job)

    def test_quality_keeps_read_only_permissions_and_no_third_party_installer(self):
        text = (ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
        blocks = re.findall(r"\n( *)permissions:\n((?:\1  .*\n)+)", text)
        self.assertTrue(blocks)
        for _, grants in blocks:
            self.assertEqual(grants.split(), ["contents:", "read"])
        self.assertNotRegex(text, r"uses: (?!actions/|\./)\S+")

    def test_ci_profile_reports_every_test_and_never_retries_or_stops_early(self):
        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text(encoding="utf-8"))["profile"]["ci"]
        self.assertEqual((config["fail-fast"], config["retries"]), (False, 0))
        self.assertEqual((config["failure-output"], config["status-level"]), ("immediate", "skip"))


if __name__ == "__main__":
    unittest.main()
