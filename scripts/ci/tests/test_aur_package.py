"""The AUR package's PKGBUILD and .SRCINFO, compared as checked in; no package is built.

makepkg exists only on Arch, so a plain bash sources the PKGBUILD (it holds
assignments and package(), nothing that runs when sourced) and prints what it
assigns, which is compared with the .SRCINFO that makepkg wrote. Where makepkg
is installed the .SRCINFO is also printed again and compared byte for byte.
packaging/aur/test-local.sh builds and checks the package itself, on Arch.

The release asset name is pinned here, GitTurtle-<version>-<commit, 12
hex>-<label>-release.tar.gz, because the release workflow builds it inside a
function. The label is read from scripts/ci/packages.py where that names the
target, and the workflow's source is checked for the same stem.
"""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
PACKAGE = ROOT / "packaging/aur/gitturtle-bin"
PKGBUILD, SRCINFO = PACKAGE / "PKGBUILD", PACKAGE / ".SRCINFO"
REPOSITORY = "https://github.com/FernandoX7/GitTurtle"
# The asset label of each package architecture's release target.
LABELS = {"x86_64": "linux-x86_64", "aarch64": "linux-aarch64"}
# What the PKGBUILD carries until a release exists: makepkg refuses both.
UNRELEASED_COMMIT, UNRELEASED_SUM = "0" * 40, "not-released"
SCALARS = ("pkgver", "pkgrel", "epoch", "pkgdesc", "url", "install", "changelog")
ARRAYS = ("arch", "license", "groups", "depends", "optdepends", "makedepends", "checkdepends",
          "provides", "conflicts", "replaces", "backup", "options", "noextract", "validpgpkeys",
          "source", "md5sums", "sha1sums", "sha224sums", "sha256sums", "sha384sums", "sha512sums",
          "b2sums", "cksums")
# Prints NAME<tab>VALUE for each scalar that is set and each array element,
# with every architecture's variant of each array. Works in bash 3.2.
DUMP = r"""
source "$1" || exit 1
printf '%s\t%s\n' pkgname "$pkgname" _commit "$_commit"
for name in $2; do
  if [ -n "${!name-}" ]; then printf '%s\t%s\n' "$name" "${!name}"; fi
done
for base in $3; do
  for name in "$base" "${base}_x86_64" "${base}_aarch64"; do
    elements="$name[@]"
    for value in "${!elements}"; do printf '%s\t%s\n' "$name" "$value"; done
  done
done
"""


def load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def assigned() -> dict[str, list[str]]:
    """What the PKGBUILD assigns, as a bash that sources it sees it."""
    printed = subprocess.run(
        ["bash", "--noprofile", "--norc", "-c", DUMP, "bash", str(PKGBUILD), " ".join(SCALARS), " ".join(ARRAYS)],
        env={"PATH": os.environ.get("PATH", os.defpath)}, capture_output=True, text=True, timeout=30, check=True)
    values: dict[str, list[str]] = {}
    for line in printed.stdout.splitlines():
        name, value = line.split("\t", 1)
        values.setdefault(name, []).append(value)
    return values


def recorded() -> tuple[dict[str, list[str]], list[str]]:
    """The .SRCINFO's pkgbase section and the package names after it."""
    sections = SRCINFO.read_text().strip().split("\n\n")
    values: dict[str, list[str]] = {}
    for line in sections[0].splitlines():
        name, value = line.strip().split(" = ", 1)
        values.setdefault(name, []).append(value)
    return values, [section.strip() for section in sections[1:]]


class AurPackageTests(unittest.TestCase):
    maxDiff = None

    @classmethod
    def setUpClass(cls):
        cls.text = PKGBUILD.read_text()
        cls.values = assigned()
        cls.commit = cls.values["_commit"][0]
        cls.version = cls.values["pkgver"][0]

    def test_srcinfo_agrees_with_the_pkgbuild(self):
        values, packages = recorded()
        expected = {name: value for name, value in self.values.items() if name not in ("pkgname", "_commit")}
        expected["pkgbase"] = self.values["pkgname"]
        self.assertEqual(values, expected)
        self.assertEqual(packages, ["pkgname = gitturtle-bin"])
        self.assertEqual(values["pkgbase"], ["gitturtle-bin"])
        self.assertEqual(values["arch"], list(LABELS))
        self.assertEqual((values["provides"], values["conflicts"], values["license"], values["url"]),
                         (["gitturtle"], ["gitturtle"], ["MIT"], [REPOSITORY]))

    def test_sources_are_the_release_assets_of_each_architecture(self):
        self.assertRegex(self.commit, r"\A[0-9a-f]{40}\Z")
        self.assertNotIn("source", self.values, "every source is an architecture's release archive")
        targets = load("aur_ci_packages", ROOT / "scripts/ci/packages.py").TARGETS
        for architecture, label in LABELS.items():
            with self.subTest(architecture=architecture):
                # The CI helper names both Linux targets, and its label is the pinned one.
                self.assertEqual(targets[f"{architecture}-unknown-linux-gnu"], label)
                self.assertEqual(self.values[f"source_{architecture}"], [
                    f"{REPOSITORY}/releases/download/v{self.version}/"
                    f"GitTurtle-{self.version}-{self.commit[:12]}-{label}-release.tar.gz"])
                # package() enters the archive's one directory, named by makepkg's CARCH.
                self.assertEqual(label, f"linux-{architecture}")
        self.assertIn('cd "$srcdir/$_asset-linux-$CARCH-release"', self.text)
        workflow = (ROOT / "scripts/release/workflow.py").read_text()
        self.assertRegex(workflow, r"GitTurtle-\{[^{}]*version[^{}]*\}-\{[^{}]*commit[^{}]*\[:12\]\}-[^\n]*\n?"
                                   r"[^\n]*\{packages\.TARGETS\[[^{}]*\]\}-release",
                         "the release workflow no longer names its archives by this stem")

    def test_checksums_are_real_or_refused_and_never_skipped(self):
        sums = {name: values for name, values in self.values.items() if name.endswith("sums") or "sums_" in name}
        self.assertEqual(sorted(sums), [f"sha256sums_{architecture}" for architecture in sorted(LABELS)])
        for name, values in sums.items():
            with self.subTest(name=name):
                self.assertEqual(len(values), len(self.values[name.replace("sha256sums", "source")]))
                for value in values:
                    self.assertNotEqual(value.upper(), "SKIP")
                    if self.commit == UNRELEASED_COMMIT:
                        # No release yet: a value makepkg refuses, not a hash that looks real.
                        self.assertEqual(value, UNRELEASED_SUM)
                    else:
                        self.assertRegex(value, r"\A[0-9a-f]{64}\Z")

    def test_package_reads_only_the_extracted_archive(self):
        self.assertEqual(re.findall(r"^([A-Za-z_][A-Za-z0-9_]*) *\(\)", self.text, re.M), ["package"])
        self.assertNotIn("install", self.values, "no .install scriptlet")
        code = [line.split(" #")[0] for line in self.text.splitlines() if not line.lstrip().startswith("#")]
        self.assertEqual([line for line in code if "://" in line], [f"url='{REPOSITORY}'"])
        body = "\n".join(code[code.index("package() {"):])
        self.assertIsNone(re.search(r"\b(curl|wget|git|ssh|scp|rsync|nc|sudo|pacman|python3?|eval)\b", body))
        self.assertNotIn("$HOME", body)

    def test_desktop_entry_and_icons_use_the_application_id(self):
        installer = load("aur_installer", ROOT / "scripts/install-linux.py")
        self.assertEqual(re.findall(r"^ *local _id=(\S+)$", self.text, re.M), [installer.APP_ID])
        self.assertEqual(re.findall(r"^ *for _size in ([0-9 ]+); do$", self.text, re.M),
                         [" ".join(map(str, installer.ICON_SIZES))])
        self.assertIn('"$pkgdir/usr/share/applications/$_id.desktop"', self.text)
        self.assertIn("\nSingleMainWindow=true\n", self.text)
        self.assertIn("\nKeywords=Git;VersionControl;VCS;Repository;Commit;Diff;Branch;PR;Merge;\n", self.text)
        self.assertIn("\nStartupWMClass=$_id\n", self.text)
        # The launcher shows the icon only when the window's Wayland app ID names the desktop file.
        window_ids = {match for path in (ROOT / "crates/app/src").rglob("*.rs")
                      for match in re.findall(r'\bapp_id: Some\("([^"]+)"', path.read_text(encoding="utf-8"))}
        self.assertEqual(window_ids, {installer.APP_ID})

    @unittest.skipUnless(shutil.which("makepkg") and os.geteuid() != 0,
                         "makepkg is not installed, or refuses to run as root; the comparison above still runs")
    def test_makepkg_prints_the_checked_in_srcinfo(self):
        with tempfile.TemporaryDirectory() as temporary:
            shutil.copyfile(PKGBUILD, Path(temporary) / "PKGBUILD")
            printed = subprocess.run(["makepkg", "--printsrcinfo"], cwd=temporary, capture_output=True,
                                     env={**os.environ, "LC_ALL": "C"}, timeout=60, check=True).stdout
        self.assertEqual(printed, SRCINFO.read_bytes())


if __name__ == "__main__":
    unittest.main()
