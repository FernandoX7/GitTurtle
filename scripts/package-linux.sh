#!/bin/bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build=true
distribution=false
executable=""
binary_explicit=false
identity_args=()
bundle=""
usage() {
  echo "Usage: scripts/package-linux.sh [--no-build [--binary PATH]] [--distribution] [--expected-revision SHA] [--expected-version VERSION] [--expected-sha256 SHA256] [output-directory]"
}
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --help) usage; exit 0 ;;
    --no-build) build=false; shift ;;
    --distribution) distribution=true; identity_args+=(--distribution); shift ;;
    --binary) executable="${2:?--binary needs a path}"; binary_explicit=true; shift 2 ;;
    --expected-revision|--expected-version|--expected-sha256)
      identity_args+=("$1" "${2:?identity option needs a value}"); shift 2 ;;
    --*) usage >&2; exit 1 ;;
    *)
      if [[ -n "$bundle" ]]; then usage >&2; exit 1; fi
      bundle="$1"; shift ;;
  esac
done
# The bundle always matches the packaging machine's architecture.
case "$(uname -s) $(uname -m)" in
  "Linux x86_64") target=x86_64-unknown-linux-gnu ;;
  "Linux aarch64") target=aarch64-unknown-linux-gnu ;;
  *) echo "This package requires Linux x86-64 or AArch64." >&2; exit 1 ;;
esac
if [[ "$binary_explicit" == true && "$build" == true ]]; then
  echo "--binary requires --no-build." >&2; exit 1
fi
executable="${executable:-$project_root/target/$target/release/gitturtle}"
bundle="${bundle:-$project_root/dist/gitturtle-linux-${target%%-*}}"
for output in "$bundle" "$bundle.tar.gz" "$bundle.tar.gz.sha256" "$bundle.tar.gz.manifest.json"; do
  if [[ -e "$output" || -L "$output" ]]; then
    echo "Output already exists: $output. Choose a new output directory." >&2; exit 1
  fi
done
for command in python3 cargo desktop-file-validate sha256sum tar readelf; do
  if ! command -v "$command" >/dev/null; then
    echo "Missing $command. Install the packaging prerequisites in docs/linux.md." >&2
    exit 1
  fi
done
if ! python3 -c 'from PIL import Image' 2>/dev/null; then
  echo "Install python3-pil (Ubuntu: sudo apt-get install python3-pil)." >&2
  exit 1
fi
# A SYSTEM_DEPS_ variable can make dav1d-sys git-clone dav1d and link that copy
# statically. GitTurtle links only the distribution's libdav1d.
if system_deps="$(compgen -e | grep '^SYSTEM_DEPS_')"; then
  echo "Unset these variables before packaging; GitTurtle links the system libdav1d: $(tr '\n' ' ' <<<"$system_deps")" >&2
  exit 1
fi
if [[ "$build" == true ]]; then
  if ! command -v pkg-config >/dev/null || ! pkg-config --exists fontconfig wayland-client xkbcommon-x11 x11-xcb openssl libzstd vulkan dav1d; then
    echo "Native development libraries are missing. Install all build dependencies in docs/linux.md; do not create local library symlinks." >&2
    exit 1
  fi
  echo "Building against libdav1d $(pkg-config --modversion dav1d) in $(pkg-config --variable=libdir dav1d)"
  # Force the packaged target, even if the user's Cargo configuration selects
  # another target. Never build one path and silently ship an older executable.
  (cd "$project_root" && cargo build --release --locked -p gitturtle --target "$target" --target-dir "$project_root/target")
fi
if [[ ! -x "$executable" ]]; then
  echo "Release executable missing. Run this script without --no-build." >&2
  exit 1
fi
# Build all payloads privately before exposing a completed output. A failed
# strict notice collection leaves no apparently verified bundle or archive.
parent="$(dirname "$bundle")"
mkdir -p "$parent"
parent="$(cd "$parent" && pwd)"
bundle_name="$(basename "$bundle")"
bundle="$parent/$bundle_name"
staging="$(mktemp -d "$parent/.gitturtle-package.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
staged_bundle="$staging/$bundle_name"
mkdir -p "$staged_bundle/bin" "$staged_bundle/icons"
if [[ -L "$executable" || ! -f "$executable" ]]; then
  echo "Expected a regular release executable, not a symlink." >&2; exit 1
fi
install -m755 "$executable" "$staged_bundle/bin/gitturtle"
install -m755 "$project_root/scripts/install-linux.py" "$staged_bundle/install.py"
install -m644 "$project_root/scripts/package-identity.py" "$staged_bundle/package_identity.py"
install -m644 "$project_root/assets/app-icon.png" "$staged_bundle/icons/app-icon.png"
install -m644 "$project_root/docs/linux.md" "$staged_bundle/README.md"
license_args=()
if [[ "$distribution" == true ]]; then license_args+=(--require-complete); fi
python3 "$project_root/scripts/collect-third-party-licenses.py" \
  --target "$target" "${license_args[@]}" "$staged_bundle/licenses"
if [[ "$build" == true ]]; then identity_args+=(--built); fi
python3 "$project_root/scripts/package-identity.py" \
  --root "$project_root" --binary "$staged_bundle/bin/gitturtle" \
  --licenses "$staged_bundle/licenses" --target "$target" \
  --profile release --signing unsigned "${identity_args[@]}" \
  --output "$staged_bundle/build-info.json"
# Once the notices are complete and the executable's format, architecture and
# identity verified: every ELF file staged so far (the steps below add only
# text and images) is a dynamically linked executable, one with a PT_INTERP
# program header; the package ships no other ELF, such as a shared library or
# a static executable. Each executable links the distribution's libdav1d: it
# needs libdav1d.so.7, imports dav1d_* symbols and defines none (a static copy
# from dav1d-sys's internal build would), and carries no RPATH or RUNPATH that
# could load another copy. A miss names the file and the check.
check_dav1d_link() {
  local file="$1" name="${1#"$staged_bundle"/}" dynamic symbols imported defined
  if ! dynamic="$(LC_ALL=C readelf -d -W "$file" 2>&1)"; then
    echo "Packaged executable $name: readelf could not read its dynamic section." >&2; return 1
  fi
  if ! grep -Eq '\(NEEDED\)[[:space:]]+Shared library: \[libdav1d\.so\.7\]' <<<"$dynamic"; then
    echo "Packaged executable $name does not list NEEDED libdav1d.so.7." >&2; return 1
  fi
  if grep -Eq '\((RPATH|RUNPATH)\)' <<<"$dynamic"; then
    echo "Packaged executable $name sets DT_RPATH or DT_RUNPATH." >&2; return 1
  fi
  # One line per dav1d_* symbol: its table, whether it is defined, its name.
  # Ndx follows the visibility and any bracketed annotation such as
  # [VARIANT_PCS]; a version suffix is dropped from the name.
  if ! symbols="$(LC_ALL=C readelf -s -W "$file" | awk '
    /^Symbol table / { table = $3; gsub(/\047/, "", table); next }
    $1 ~ /^[0-9]+:$/ {
      for (i = 5; i <= NF && $i !~ /^(DEFAULT|PROTECTED|HIDDEN|INTERNAL)$/; i++) {}
      for (j = i + 1; j <= NF && $j ~ /^\[/; j++) {}
      name = $(j + 1); sub(/@.*/, "", name)
      if (name ~ /^dav1d_/) print table, ($j == "UND" ? "undefined" : "defined"), name
    }')"; then
    echo "Packaged executable $name: readelf could not read its symbol tables." >&2; return 1
  fi
  imported="$(grep -c '^\.dynsym undefined ' <<<"$symbols" || true)"
  if [[ "$imported" -eq 0 ]]; then
    echo "Packaged executable $name imports no dav1d_* symbol in .dynsym." >&2; return 1
  fi
  if defined="$(grep ' defined ' <<<"$symbols")"; then
    echo "Packaged executable $name defines dav1d_* symbols, a static copy of dav1d: $(tr '\n' ' ' <<<"$defined")" >&2
    return 1
  fi
  echo "Checked $name: NEEDED libdav1d.so.7, $imported dav1d_* symbols imported and none defined, no RPATH or RUNPATH"
}
elf_count=0
while IFS= read -r -d '' file; do
  if [[ "$(head -c 4 -- "$file" | od -An -tx1 | tr -d ' \n')" == 7f454c46 ]]; then
    name="${file#"$staged_bundle"/}"
    if ! headers="$(LC_ALL=C readelf -l -W "$file" 2>&1)"; then
      echo "Packaged ELF $name: readelf could not read its program headers." >&2; exit 1
    fi
    if ! grep -Eq '^[[:space:]]+INTERP[[:space:]]' <<<"$headers"; then
      echo "The package ships an ELF that is not an executable: $name has no PT_INTERP program header." >&2
      exit 1
    fi
    check_dav1d_link "$file" || exit 1
    elf_count=$((elf_count + 1))
  fi
done < <(find "$staged_bundle" -type f -print0)
if [[ "$elf_count" -eq 0 ]]; then
  echo "The package holds no ELF executable to check." >&2; exit 1
fi
python3 - "$project_root" "$staged_bundle" <<'PY'
import hashlib
from pathlib import Path
import sys
from PIL import Image

root, bundle = map(Path, sys.argv[1:3])
readme = bundle / "README.md"
readme.write_text(readme.read_text().replace(
    "](validation.md)", "](https://github.com/FernandoX7/GitTurtle/blob/main/docs/validation.md)"
).replace(
    "](file-previews.md)", "](https://github.com/FernandoX7/GitTurtle/blob/main/docs/file-previews.md)"
))
with Image.open(bundle / "icons/app-icon.png") as image:
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        path = bundle / f"icons/hicolor/{size}x{size}/apps/com.gitturtle.GitTurtle.png"
        path.parent.mkdir(parents=True, exist_ok=True)
        image.resize((size, size), Image.Resampling.LANCZOS).save(path)
files = sorted(path for path in bundle.rglob("*") if path.is_file())
(bundle / "SHA256SUMS").write_text("".join(
    f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(bundle)}\n" for path in files
))
PY
# Archive owner IDs are neutral. The manifest names the compiled source, version
# and target; a caller may use gitturtle-VERSION-TARGET-SHA as its output basename.
tar --sort=name --owner=0 --group=0 --numeric-owner -C "$staging" -czf "$staged_bundle.tar.gz" -- "$bundle_name"
(cd "$staging" && sha256sum -- "$bundle_name.tar.gz" > "$bundle_name.tar.gz.sha256")
python3 - "$project_root" "$staged_bundle" "$bundle" <<'PY_PACKAGE'
import importlib.util
import json
import os
from pathlib import Path
import sys

root, stage, destination = map(Path, sys.argv[1:])
spec = importlib.util.spec_from_file_location("package_identity", root / "scripts/package-identity.py")
identity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(identity)
archive = Path(str(stage) + ".tar.gz")
outer = Path(str(archive) + ".manifest.json")
outer.write_text(json.dumps(identity.archive_manifest(archive, stage / "build-info.json"), indent=2) + "\n")
outputs = [(Path(str(stage) + suffix), Path(str(destination) + suffix))
           for suffix in (".tar.gz", ".tar.gz.sha256", ".tar.gz.manifest.json")]
# Files use exclusive hard-link creation, so even a concurrent collision cannot
# overwrite unrelated output. Reserve the bundle directory exclusively as well.
published = []
reserved = False
try:
    destination.mkdir()
    reserved = True
    for source, target in outputs:
        os.link(source, target)
        published.append(target)
    os.replace(stage, destination)  # destination is our own empty reservation
    reserved = False
except BaseException:
    for target in reversed(published):
        target.unlink()
    if reserved:
        destination.rmdir()
    raise
PY_PACKAGE
echo "Built $bundle.tar.gz ($([[ "$distribution" == true ]] && echo complete-notices || echo development))"
echo "Extract anywhere, then run: python3 \"$bundle/install.py\""
