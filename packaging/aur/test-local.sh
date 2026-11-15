#!/bin/bash
# Build gitturtle-bin from a local release archive and check the built package.
#
#   packaging/aur/test-local.sh ARCHIVE [WORK-DIRECTORY]
#
# ARCHIVE is this machine's Linux archive: a downloaded release asset, or one
# made by scripts/package-linux.sh --distribution. A release's archive holds
# one directory named after it, GitTurtle-<version>-<commit, 12
# hex>-linux-<arch>-release, which package() enters. An archive packaged under
# another name, such as the packager's default gitturtle-linux-<arch>, holds a
# directory of that name: the copy of the PKGBUILD then gets a prepare() that
# renames the extracted directory to the release's name, and package() runs as
# it is written.
#
# Nothing is installed, downloaded or run as root. The PKGBUILD is copied to
# the work directory, where this architecture's source is pointed at the local
# archive; makepkg builds there without -s or -i; the archive's own installer
# runs with HOME set to a directory under the work directory; and the packaged
# executable is asked only for --build-info. The work directory is kept so the
# package can be inspected: remove it when done.
#
# That installer and that executable are ARCHIVE's own code. Pass an archive
# you built, or a download whose SHA-256 is the one in the release's
# SHA256SUMS; once the PKGBUILD carries real checksums, an archive that does
# not match is refused before anything in it runs.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
package_dir="$here/gitturtle-bin"
pkgbuild="$package_dir/PKGBUILD"
repository=https://github.com/FernandoX7/GitTurtle
# What the PKGBUILD carries until a release exists.
unreleased_commit=0000000000000000000000000000000000000000
unreleased_sum=not-released

fail() {
  echo "gitturtle-bin local test failed: $*" >&2
  exit 1
}
step() {
  echo "== $*"
}

if [[ "$#" -lt 1 || "$#" -gt 2 ]]; then
  echo "Usage: packaging/aur/test-local.sh ARCHIVE [WORK-DIRECTORY]" >&2
  exit 1
fi
machine="$(uname -m)"
if [[ "$(uname -s)" != Linux || ( "$machine" != x86_64 && "$machine" != aarch64 ) ]]; then
  fail "the package targets Linux x86_64 and aarch64."
fi
if [[ "$EUID" -eq 0 ]]; then
  fail "makepkg does not run as root; run this as an ordinary user."
fi
for tool in makepkg bsdtar sha256sum cmp desktop-file-validate python3; do
  command -v "$tool" >/dev/null || fail "missing $tool."
done
if [[ ! -f "$1" || -L "$1" ]]; then
  fail "expected a release archive file: $1"
fi
archive="$(realpath -e -- "$1")"
name="$(basename -- "$archive")"
if [[ ! "$name" =~ ^[A-Za-z0-9][A-Za-z0-9._+-]*\.tar\.gz$ ]]; then
  fail "unexpected archive name: $name"
fi

if [[ "$#" -eq 2 ]]; then
  if [[ -e "$2" || -L "$2" ]]; then
    fail "the work directory already exists: $2. Choose a new one."
  fi
  mkdir -p -- "$(dirname -- "$2")"
  mkdir -- "$2"
  work="$(cd -- "$2" && pwd)"
else
  work="$(mktemp -d "${TMPDIR:-/tmp}/gitturtle-bin-test.XXXXXX")"
fi
build="$work/build"
mkdir "$work/checked-in" "$build" "$work/out" "$work/archive" "$work/package" "$work/home" "$work/tmp"

# Every makepkg run stays inside the work directory, whatever makepkg.conf says.
run_makepkg() {
  (cd "$1" && shift && LC_ALL=C BUILDDIR="$work/makepkg" PKGDEST="$work/out" SRCDEST="$build" \
    SRCPKGDEST="$work/out" LOGDEST="$work/out" PKGEXT=.pkg.tar makepkg --nocolor "$@")
}
# The value of the PKGBUILD's single NAME= line.
assigned() {
  local lines
  lines="$(grep -E "^$1=" "$pkgbuild" || true)"
  if [[ -z "$lines" || "$lines" == *$'\n'* ]]; then
    fail "expected exactly one $1= line in the PKGBUILD."
  fi
  printf '%s\n' "${lines#*=}"
}

step "Archive $name"
# Read through stdin: sha256sum escapes a path that holds a backslash and marks the line.
sum="$(sha256sum < "$archive")"
sum="${sum%% *}"
bsdtar -tf "$archive" > "$work/archive-list.txt"
# The one directory the archive holds, whatever it is called.
top="$(sed -n '1s|/.*||p' "$work/archive-list.txt")"
if [[ ! "$top" =~ ^[A-Za-z0-9][A-Za-z0-9._+-]*$ ]]; then
  fail "unexpected directory name in the archive: $top"
fi
if ! awk -v prefix="$top/" 'index($0, prefix) != 1 { other = 1 } END { exit other }' "$work/archive-list.txt"; then
  fail "the archive holds more than one directory, $top/."
fi
bsdtar -xOf "$archive" "$top/build-info.json" > "$work/archive-build-info.json"
identity="$(python3 - "$work/archive-build-info.json" <<'PY'
import json
import re
import sys

info = json.load(open(sys.argv[1]))
compiled = info["compiled_identity"]
values = [compiled["version"], compiled["source_revision"], info["target"], info["bundle_id"]]
patterns = [r"[0-9]+\.[0-9]+\.[0-9]+", r"[0-9a-f]{40}", r"(x86_64|aarch64)-unknown-linux-gnu", r"[A-Za-z0-9.]+"]
if not all(isinstance(value, str) and re.fullmatch(pattern, value) for value, pattern in zip(values, patterns)):
    sys.exit("The archive's build-info.json does not identify a Linux release build.")
if info.get("distribution") != "complete-notices" or compiled.get("source_tree") != "clean":
    sys.exit("The archive is not a distribution package of a clean release build.")
print(*values)
PY
)"
read -r version revision target app_id <<<"$identity"
if [[ "$target" != "$machine-unknown-linux-gnu" ]]; then
  fail "the archive targets $target, but this machine builds the $machine package."
fi
# What the release workflow names this build's archive and its directory.
release_stem="GitTurtle-$version-${revision:0:12}-linux-$machine-release"
echo "version $version, source $revision, sha256 $sum"
if [[ "$top" == "$release_stem" ]]; then
  echo "holds $top/, a release archive's directory: package() enters it as written"
else
  echo "holds $top/, not a release archive's $release_stem/: the copy's prepare() renames it"
fi

step "Checked-in PKGBUILD and .SRCINFO"
cp -- "$pkgbuild" "$work/checked-in/PKGBUILD"
run_makepkg "$work/checked-in" --printsrcinfo > "$work/checked-in/.SRCINFO"
if ! cmp -s "$work/checked-in/.SRCINFO" "$package_dir/.SRCINFO"; then
  fail ".SRCINFO is not what makepkg --printsrcinfo prints for the PKGBUILD; regenerate it."
fi
pkgver="$(assigned pkgver)"
pkgrel="$(assigned pkgrel)"
checked_commit="$(assigned _commit)"
checked_sum="$(assigned "sha256sums_$machine")"
one_quoted_value="^\\('([^']+)'\\)\$"
if [[ ! "$checked_sum" =~ $one_quoted_value ]]; then
  fail "expected sha256sums_$machine=('...') on one line."
fi
checked_sum="${BASH_REMATCH[1]}"
if [[ "$pkgver" != "$version" ]]; then
  fail "pkgver is $pkgver, but the archive holds version $version."
fi
if [[ "$checked_commit" == "$unreleased_commit" ]]; then
  if [[ "$checked_sum" != "$unreleased_sum" ]]; then
    fail "_commit is the unreleased placeholder, but sha256sums_$machine is not."
  fi
  echo "not released yet: _commit and sha256sums_$machine are placeholders"
else
  if [[ "$checked_commit" != "$revision" ]]; then
    fail "_commit is $checked_commit, but the archive was built from $revision."
  fi
  if [[ "$checked_sum" != "$sum" ]]; then
    fail "sha256sums_$machine is not this archive's SHA-256, $sum."
  fi
  echo "released: _commit and sha256sums_$machine are this archive's"
fi

step "Source for $machine"
cp -- "$pkgbuild" "$build/PKGBUILD"
cp -- "$archive" "$build/$name"
sed -i "s/^_commit=.*/_commit=$revision/" "$build/PKGBUILD"
source_url="$(run_makepkg "$build" --printsrcinfo | sed -n "s/^[[:space:]]*source_$machine = //p")"
if [[ "$source_url" != "$repository/releases/download/v$version/$release_stem.tar.gz" ]]; then
  fail "with the archive's commit, source_$machine is $source_url, not this build's archive in the v$version release."
fi
echo "$source_url"
sed -i "s|^source_$machine=.*|source_$machine=('$name')|" "$build/PKGBUILD"
if [[ "$top" != "$release_stem" ]]; then
  cat >> "$build/PKGBUILD" <<EOF

# Added by test-local.sh: this archive's directory is not named as a release's.
prepare() {
  mv "\$srcdir/$top" "\$srcdir/$release_stem"
}
EOF
fi
if [[ "$checked_sum" == "$unreleased_sum" ]]; then
  if refusal="$(run_makepkg "$build" --verifysource 2>&1)"; then
    fail "makepkg accepted the placeholder checksum."
  fi
  if ! grep -q 'did not pass the validity check' <<<"$refusal"; then
    printf '%s\n' "$refusal" >&2
    fail "makepkg stopped before it compared the placeholder checksum."
  fi
  echo "makepkg refuses the placeholder checksum"
fi
sed -i "s/^sha256sums_$machine=.*/sha256sums_$machine=('$sum')/" "$build/PKGBUILD"
pointed="$(run_makepkg "$build" --printsrcinfo | sed -n "s/^[[:space:]]*\(source\|sha256sums\)_$machine = //p" | tr '\n' ' ')"
if [[ "$pointed" != "$name $sum " ]]; then
  fail "could not point source_$machine at the local archive; the PKGBUILD's layout changed."
fi

step "makepkg"
if ! run_makepkg "$build" > "$work/makepkg.log" 2>&1; then
  tail -n 30 "$work/makepkg.log" >&2
  fail "makepkg did not build the package (log: $work/makepkg.log)."
fi
package="$work/out/gitturtle-bin-$version-$pkgrel-$machine.pkg.tar"
if [[ ! -f "$package" ]]; then
  fail "makepkg did not write $package."
fi
bsdtar -tf "$package" > "$work/package-list.txt"
bsdtar -xpf "$package" -C "$work/package"
echo "$package"

step "The archive's installer, into $work/home"
bsdtar -xpf "$archive" -C "$work/archive"
if ! env -u XDG_DATA_HOME HOME="$work/home" TMPDIR="$work/tmp" \
    python3 "$work/archive/$top/install.py" > "$work/install.log" 2>&1; then
  tail -n 30 "$work/install.log" >&2
  fail "the archive's installer failed in the scratch HOME."
fi

step "Built package"
python3 - "$work" "$top" "$package" "$app_id" <<'PY'
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import subprocess
import sys
import tarfile

work, top, built, app_id = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3]), sys.argv[4]
pkgname = "gitturtle-bin"
archive, package, home = work / "archive" / top, work / "package", work / "home"
data = home / ".local/share"
info = json.loads((archive / "build-info.json").read_text())


def fail(message):
    sys.exit("gitturtle-bin local test failed: " + message)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def tree(root, suffix=""):
    """Each file below root by relative path, with its SHA-256."""
    found = {}
    for path in sorted(root.rglob("*")):
        mode = path.lstat().st_mode
        if stat.S_ISREG(mode):
            if path.name.endswith(suffix):
                found[path.relative_to(root).as_posix()] = digest(path)
        elif not stat.S_ISDIR(mode):
            fail(f"{path} is neither a file nor a directory.")
    return found


def same(what, expected, actual):
    if expected != actual:
        missing, extra = sorted(set(expected) - set(actual)), sorted(set(actual) - set(expected))
        changed = sorted(key for key in set(expected) & set(actual)
                         if isinstance(expected, dict) and expected[key] != actual[key])
        fail(f"{what} differ. Missing: {missing[:10]}. Unexpected: {extra[:10]}. Changed: {changed[:10]}.")


# 1. The file list is exactly what the archive's own listing calls for.
listed = (work / "archive-list.txt").read_text().splitlines()


def below(directory):
    prefix = f"{top}/{directory}/"
    return [name[len(prefix):] for name in listed if name.startswith(prefix) and not name.endswith("/")]


files = {"usr/bin/gitturtle", f"usr/share/applications/{app_id}.desktop",
         f"usr/share/doc/{pkgname}/README.md",
         *(f"usr/share/icons/hicolor/{name}" for name in below("icons/hicolor")),
         *(f"usr/share/licenses/{pkgname}/{name}" for name in below("licenses"))}
directories = {f"{parent}/" for name in files for parent in PurePosixPath(name).parents if parent.name}
expected = sorted({".BUILDINFO", ".MTREE", ".PKGINFO"} | files | directories)
same("The package's file list and the expected one", expected,
     sorted((work / "package-list.txt").read_text().splitlines()))
with tarfile.open(built) as members:
    for member in members:
        name = member.name.rstrip("/")
        if name.startswith("."):
            continue
        wanted = 0o755 if member.isdir() or name == "usr/bin/gitturtle" else 0o644
        if not (member.isdir() or member.isreg()) or member.mode != wanted or (member.uid, member.gid) != (0, 0):
            fail(f"{name} has type {member.type!r}, mode {member.mode:o}, owner {member.uid}:{member.gid}.")
print(f"file list: {len(files)} files in {len(directories)} directories, as expected; "
      "modes 755 for the executable and directories, 644 otherwise; owner root")
for name in sorted(files):
    if "/licenses/" not in name:
        print("  /" + name)
notices = [name for name in files if "/licenses/" in name]
print(f"  /usr/share/licenses/{pkgname}/ ({len(notices)} files, LICENSE among them)")

# 2. The executable is the archive's, byte for byte, and names the same source.
binary = package / "usr/bin/gitturtle"
if not digest(binary) == digest(archive / "bin/gitturtle") == info["binary_sha256"]:
    fail("the packaged executable is not the archive's executable.")
probe = subprocess.run([str(binary), "--build-info"], capture_output=True, text=True, timeout=30,
                       env={"PATH": os.environ["PATH"], "HOME": str(home)})
if probe.returncode != 0 or json.loads(probe.stdout) != info["compiled_identity"]:
    fail("the packaged executable's --build-info is not the archive's compiled identity.")
revision = info["compiled_identity"]["source_revision"]
print(f"executable: the archive's bytes ({info['binary_sha256']}); --build-info source revision {revision}")
if app_id.encode() not in binary.read_bytes():
    fail(f"the executable does not carry the application ID {app_id}.")

# 3. The desktop entry is valid and is the installer's, paths apart.
entry = package / f"usr/share/applications/{app_id}.desktop"
checked = subprocess.run(["desktop-file-validate", str(entry)], capture_output=True, text=True)
if checked.returncode != 0 or checked.stdout.strip() or checked.stderr.strip():
    fail("desktop-file-validate: " + (checked.stdout + checked.stderr).strip())
print(f"desktop-file-validate: passed for {entry.name}")


def pairs(path):
    lines = path.read_text().splitlines()
    if lines[0] != "[Desktop Entry]" or any("=" not in line for line in lines[1:]):
        fail(f"{path} is not a single-group desktop entry.")
    return [tuple(line.split("=", 1)) for line in lines[1:]]


same("The installed desktop entries", [entry.name],
     sorted(path.name for path in (data / "applications").glob("*.desktop")))
installed, packaged = pairs(data / "applications" / entry.name), pairs(entry)
same("The desktop entry's keys", [key for key, _ in installed], [key for key, _ in packaged])
plain = {"Exec", "Icon"}
same("The desktop entry's values", [pair for pair in installed if pair[0] not in plain],
     [pair for pair in packaged if pair[0] not in plain])
installed, packaged = dict(installed), dict(packaged)
program = f'"{home / ".local/bin/gitturtle"}"'
if not installed["Exec"].startswith(program):
    fail("the installer's Exec does not start with its installed executable.")
if packaged["Exec"] != binary.name + installed["Exec"][len(program):]:
    fail(f"Exec is {packaged['Exec']!r}; the installer's arguments are {installed['Exec'][len(program):]!r}.")
if packaged["Icon"] != Path(installed["Icon"]).stem or packaged["Icon"] != app_id:
    fail(f"Icon is {packaged['Icon']!r}; the installer's icon is {installed['Icon']!r}.")
if packaged["StartupWMClass"] != app_id:
    fail("StartupWMClass is not the application ID.")
print(f"desktop entry: the installer's {len(installed) - len(plain)} other keys, same order and values; "
      f"Exec={packaged['Exec']}, Icon={packaged['Icon']}; the executable carries that application ID")

# 4. The icons, notices, executable and README are the installer's and the archive's.
same("The installer's hicolor icons and the package's", tree(data / "icons/hicolor", ".png"),
     tree(package / "usr/share/icons/hicolor"))
same("The installer's notices and the package's", tree(data / "gitturtle/licenses"),
     tree(package / f"usr/share/licenses/{pkgname}"))
if digest(home / ".local/bin/gitturtle") != digest(binary):
    fail("the installer's executable is not the package's.")
if digest(package / f"usr/share/doc/{pkgname}/README.md") != digest(archive / "README.md"):
    fail("the packaged README is not the archive's.")
if digest(package / f"usr/share/licenses/{pkgname}/LICENSE") != digest(archive / "licenses/LICENSE"):
    fail("the packaged LICENSE is not the archive's.")
print(f"installer parity: {len(tree(package / 'usr/share/icons/hicolor'))} hicolor icons and "
      f"{len(notices)} notices byte for byte, and the same executable")
PY

if command -v namcap >/dev/null; then
  step "namcap (reported, not judged)"
  namcap "$build/PKGBUILD" "$package" || true
else
  echo "namcap is not installed; skipped"
fi
echo "gitturtle-bin local test passed: $name -> $(basename -- "$package"), source $revision (work directory: $work)"
