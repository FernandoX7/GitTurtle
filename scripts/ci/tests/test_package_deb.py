"""Tests for scripts/package-deb.sh using a hermetic dpkg-deb test double."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "scripts/package-deb.sh"
ICON_SIZES = (16, 24, 32, 48, 64, 128, 256, 512)
APP_ID = "com.gitturtle.GitTurtle"


def create_fixture_bundle(base: Path, target: str = "x86_64-unknown-linux-gnu", version: str = "0.1.0") -> Path:
    bundle = base / "bundle"
    (bundle / "bin").mkdir(parents=True)
    (bundle / "licenses").mkdir(parents=True)
    binary = bundle / "bin/gitturtle"
    binary.write_bytes(b"\x7fELFfake-binary")
    binary.chmod(0o755)
    (bundle / "README.md").write_text("# GitTurtle Linux\n", encoding="utf-8")
    (bundle / "licenses/LICENSE").write_text("MIT License\n", encoding="utf-8")
    (bundle / "licenses/THIRD_PARTY_NOTICES.md").write_text("# Notices\n", encoding="utf-8")
    (bundle / "build-info.json").write_text(
        json.dumps({"application": "GitTurtle", "version": version, "target": target}),
        encoding="utf-8",
    )
    for size in ICON_SIZES:
        icon_dir = bundle / f"icons/hicolor/{size}x{size}/apps"
        icon_dir.mkdir(parents=True)
        (icon_dir / f"{APP_ID}.png").write_bytes(b"\x89PNG\r\n\x1a\n")
    return bundle


def install_fake_dpkg_deb(bin_dir: Path, capture_dir: Path) -> None:
    stub = bin_dir / "dpkg-deb"
    stub.write_text(
        f"""#!/usr/bin/env python3
import shutil, sys
from pathlib import Path

args = sys.argv[1:]
capture = Path({str(capture_dir)!r})
(capture / "argv.txt").write_text(" ".join(args) + "\\n", encoding="utf-8")
pkg_dir = Path(args[-2])
out_file = Path(args[-1])
shutil.copytree(pkg_dir, capture / "staged", dirs_exist_ok=True)
out_file.write_bytes(b"!<arch>\\ndebian-binary   ")
""",
        encoding="utf-8",
    )
    stub.chmod(0o755)


class PackageDebTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gitturtle-deb-test-")
        self.root = Path(self.temp.name)
        self.fake_bin = self.root / "fake-bin"
        self.fake_bin.mkdir()
        self.capture = self.root / "capture"
        self.capture.mkdir()
        install_fake_dpkg_deb(self.fake_bin, self.capture)
        self.env = {
            **os.environ,
            "PATH": f"{self.fake_bin}{os.pathsep}{os.environ.get('PATH', os.defpath)}",
        }

    def tearDown(self):
        self.temp.cleanup()

    def run_deb(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(SCRIPT), *args],
            env=self.env,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_packages_amd64_and_arm64_without_hard_libdav1d_dependency(self):
        for target, deb_arch in [
            ("x86_64-unknown-linux-gnu", "amd64"),
            ("aarch64-unknown-linux-gnu", "arm64"),
        ]:
            with self.subTest(target=target):
                sub = self.root / deb_arch
                sub.mkdir()
                bundle = create_fixture_bundle(sub, target=target, version="0.1.0")
                out = sub / f"gitturtle_0.1.0_{deb_arch}.deb"
                result = self.run_deb("--bundle", str(bundle), "--output", str(out))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(out.is_file())
                argv = (self.capture / "argv.txt").read_text(encoding="utf-8")
                self.assertIn("--build --root-owner-group", argv)

                staged = self.capture / "staged"
                control = (staged / "DEBIAN/control").read_text(encoding="utf-8")
                self.assertIn(f"Architecture: {deb_arch}\n", control)
                self.assertIn("Version: 0.1.0\n", control)
                depends_line = next(line for line in control.splitlines() if line.startswith("Depends:"))
                recommends_line = next(line for line in control.splitlines() if line.startswith("Recommends:"))
                self.assertNotIn("libdav1d", depends_line)
                self.assertIn("libdav1d7", recommends_line)
                self.assertNotIn("libdav1d8", recommends_line)

                desktop = (staged / f"usr/share/applications/{APP_ID}.desktop").read_text(encoding="utf-8")
                self.assertIn("SingleMainWindow=true\n", desktop)
                self.assertIn("Keywords=Git;VersionControl;VCS;Repository;Commit;Diff;Branch;PR;Merge;\n", desktop)

                for hook in ("postinst", "postrm"):
                    mode = stat.S_IMODE((staged / "DEBIAN" / hook).stat().st_mode)
                    self.assertEqual(mode, 0o755, hook)
                self.assertTrue((staged / "usr/share/doc/gitturtle/copyright").is_file())
                self.assertTrue((staged / "usr/share/doc/gitturtle/licenses/THIRD_PARTY_NOTICES.md").is_file())
                for size in ICON_SIZES:
                    self.assertTrue(
                        (staged / f"usr/share/icons/hicolor/{size}x{size}/apps/{APP_ID}.png").is_file()
                    )

    def test_refuses_existing_output_and_unsupported_target(self):
        bundle = create_fixture_bundle(self.root, target="x86_64-unknown-linux-gnu")
        existing = self.root / "existing.deb"
        existing.write_bytes(b"keep")
        result = self.run_deb("--bundle", str(bundle), "--output", str(existing))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Output already exists", result.stderr)
        self.assertEqual(existing.read_bytes(), b"keep")

        bad_bundle = create_fixture_bundle(self.root / "bad", target="riscv64gc-unknown-linux-gnu")
        result = self.run_deb("--bundle", str(bad_bundle))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Unsupported Linux target", result.stderr)

    def test_refuses_missing_dpkg_deb(self):
        bundle = create_fixture_bundle(self.root / "nodpkg")
        empty_bin = self.root / "empty-bin"
        empty_bin.mkdir()
        bash = shutil.which("bash")
        self.assertIsNotNone(bash, "bash must be available to run package-deb.sh")
        env = {**os.environ, "PATH": str(empty_bin)}
        result = subprocess.run(
            [bash, str(SCRIPT), "--bundle", str(bundle)],
            env=env,
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing dpkg-deb", result.stderr)


if __name__ == "__main__":
    unittest.main()
