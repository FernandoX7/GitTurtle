"""Linux packager refusal fixtures; no app, archive, or desktop validation.

Run the real shell entry point and shared identity checks against disposable
source trees. Native tools/notice collection are explicit test doubles; tar is
an error sentinel so these tests never manufacture a distributable archive.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[2]


@unittest.skipUnless(platform.system() == "Linux" and platform.machine() == "x86_64",
                     "Linux packager entry point requires Linux x86-64")
class LinuxPackageRefusalTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="gitturtle-package-refusal-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source with spaces"
        self.scripts = self.source / "scripts"
        self.scripts.mkdir(parents=True)
        for name in ("package-linux.sh", "package-identity.py", "install-linux.py"):
            shutil.copyfile(SCRIPTS / name, self.scripts / name)
        for name in ("assets/app-icon.png", "docs/linux.md"):
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("Synthetic fixture resource; never distributed.\n")
        (self.source / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0"\n')
        (self.source / "Cargo.lock").write_text("version = 4\n")
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.events = self.root / "events"
        # The import check is all that can run: refused inputs never reach image
        # rendering. This avoids requiring Pillow in tooling-only test jobs.
        modules = self.root / "modules" / "PIL"
        modules.mkdir(parents=True)
        (modules / "__init__.py").write_text("Image = None\n")
        self.environment = dict(os.environ, PATH=f"{self.tools}:{os.environ['PATH']}",
                                PYTHONPATH=str(modules.parent), FIXTURE_EVENTS=str(self.events))
        self.tool("cargo", 'printf "cargo %s\\n" "$*" >> "$FIXTURE_EVENTS"\nexit 73')
        self.tool("desktop-file-validate", "exit 0")
        self.tool("pkg-config", "exit 0")
        self.tool("tar", 'printf "unexpected archive\\n" >> "$FIXTURE_EVENTS"\nexit 89')
        collector = '''import hashlib, json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ["FIXTURE_EVENTS"], "a") as stream:
    stream.write("collector " + json.dumps(args) + "\\n")
out = pathlib.Path(args[-1]); out.mkdir()
(out / "LICENSE").write_text("Synthetic fixture notice\\n")
(out / "THIRD_PARTY_NOTICES.md").write_text("Synthetic fixture notice\\n")
(out / "REVIEW_REQUIRED.md").write_text("Fixture missing upstream notice\\n")
root = pathlib.Path(__file__).resolve().parents[1]
(out / "dependencies.json").write_text(json.dumps(dict(
 target="x86_64-unknown-linux-gnu", cargo_lock_sha256=hashlib.sha256((root/"Cargo.lock").read_bytes()).hexdigest(),
 review_required=["fixture-0.1.0"], dependencies=[{"name":"fixture"}])))
if "--require-complete" in args:
    sys.stderr.write("Fixture unresolved notices block distribution\\n")
    raise SystemExit(12)
'''
        (self.scripts / "collect-third-party-licenses.py").write_text(collector)
        for args in (["init", "-q"], ["add", "."],
                     ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                      "-c", "commit.gpgSign=false", "commit", "-qm", "fixture"]):
            subprocess.run(["git", "-C", str(self.source), *args], check=True, capture_output=True)
        self.binary = self.root / "fixture executable"
        header = bytearray(32)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = (62).to_bytes(2, "little")
        self.binary.write_bytes(header)
        self.binary.chmod(0o755)
        self.output = self.root / "output with spaces" / "gitturtle"

    def tool(self, name, source):
        path = self.tools / name
        path.write_text("#!/bin/sh\n" + source + "\n")
        path.chmod(0o755)

    def run_package(self, *arguments, no_build=True, binary=None):
        args = ["bash", str(self.scripts / "package-linux.sh")]
        if no_build:
            args += ["--no-build", "--binary", str(binary or self.binary)]
        result = subprocess.run([*args, *arguments, str(self.output)], env=self.environment,
                                text=True, capture_output=True, timeout=15)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        return result

    def assert_no_output(self):
        for suffix in ("", ".tar.gz", ".tar.gz.sha256", ".tar.gz.manifest.json"):
            path = Path(str(self.output) + suffix)
            self.assertFalse(path.exists() or path.is_symlink(), path)
        if self.output.parent.exists():
            self.assertEqual(list(self.output.parent.iterdir()), [])
        if self.events.exists():
            self.assertNotIn("unexpected archive", self.events.read_text())

    def test_existing_outputs_and_dangling_symlinks_are_never_replaced(self):
        self.output.parent.mkdir()
        for suffix in ("", ".tar.gz", ".tar.gz.sha256", ".tar.gz.manifest.json"):
            for symbolic in (False, True):
                with self.subTest(suffix=suffix, symlink=symbolic):
                    collision = Path(str(self.output) + suffix)
                    if symbolic:
                        collision.symlink_to(self.root / "absent unrelated destination")
                    else:
                        collision.write_text("keep unrelated output\n")
                    result = self.run_package()
                    self.assertIn("Output already exists", result.stderr)
                    if symbolic:
                        self.assertTrue(collision.is_symlink())
                    else:
                        self.assertEqual(collision.read_text(), "keep unrelated output\n")
                    self.assertEqual(list(self.output.parent.iterdir()), [collision])
                    self.assertFalse(self.events.exists())
                    collision.unlink()

    def test_existing_directory_and_contents_are_preserved(self):
        self.output.mkdir(parents=True)
        marker = self.output / "unrelated.txt"
        marker.write_text("preserve")
        result = self.run_package()
        self.assertIn("Output already exists", result.stderr)
        self.assertEqual(marker.read_text(), "preserve")
        self.assertFalse(self.events.exists())

    def test_strict_notice_failure_removes_partial_stage(self):
        result = self.run_package("--distribution")
        self.assertIn("unresolved notices", result.stderr)
        events = self.events.read_text()
        self.assertIn("--require-complete", events)
        self.assertNotIn("cargo ", events)
        self.assert_no_output()

    def test_stale_expected_revision_version_and_hash_leave_no_output(self):
        for option, value, error in (("--expected-revision", "f" * 40, "Expected revision differs"),
                                     ("--expected-version", "99.0.0", "Expected version differs"),
                                     ("--expected-sha256", "f" * 64, "SHA-256 differs")):
            with self.subTest(option=option):
                result = self.run_package(option, value)
                self.assertIn(error, result.stderr)
                self.assert_no_output()

    def test_wrong_architecture_is_rejected_without_execution(self):
        self.binary.write_bytes(b"not an ELF executable\n")
        result = self.run_package()
        self.assertIn("format/architecture", result.stderr)
        self.assert_no_output()

    def test_symlink_or_missing_executable_is_refused(self):
        linked = self.root / "linked executable"
        linked.symlink_to(self.binary)
        result = self.run_package(binary=linked)
        self.assertIn("not a symlink", result.stderr)
        self.assert_no_output()
        result = self.run_package(binary=self.root / "missing executable")
        self.assertIn("Release executable missing", result.stderr)
        self.assert_no_output()

    def test_binary_option_requires_no_build_before_any_tool_runs(self):
        result = self.run_package("--binary", str(self.binary), no_build=False)
        self.assertIn("--binary requires --no-build", result.stderr)
        self.assertFalse(self.events.exists())
        self.assert_no_output()

    def test_build_failure_uses_explicit_supported_target_and_publishes_nothing(self):
        result = self.run_package(no_build=False)
        self.assertEqual(result.returncode, 73)
        command = self.events.read_text()
        self.assertIn("build --release --locked -p gitturtle --target x86_64-unknown-linux-gnu", command)
        self.assertIn(f"--target-dir {self.source}/target", command)
        self.assert_no_output()


if __name__ == "__main__":
    unittest.main()
