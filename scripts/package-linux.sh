#!/bin/bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build=true
executable="$project_root/target/x86_64-unknown-linux-gnu/release/gitturtle"
if [[ "${1:-}" == "--no-build" ]]; then
  build=false
  shift
  if [[ "${1:-}" == "--binary" ]]; then
    executable="${2:?--binary needs the path to the intended release executable}"
    shift 2
  fi
fi
if [[ "${1:-}" == "--help" ]]; then
  echo "Usage: scripts/package-linux.sh [--no-build [--binary PATH]] [output-directory]"
  exit 0
fi
if [[ "$#" -gt 1 || "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  echo "Usage: scripts/package-linux.sh [--no-build [--binary PATH]] [output-directory] (Linux x86-64)" >&2
  exit 1
fi
for command in python3 desktop-file-validate sha256sum tar; do
  if ! command -v "$command" >/dev/null; then
    echo "Missing $command. Install the packaging prerequisites in docs/linux.md." >&2
    exit 1
  fi
done
if ! python3 -c 'from PIL import Image' 2>/dev/null; then
  echo "Install python3-pil (Ubuntu: sudo apt-get install python3-pil)." >&2
  exit 1
fi
if [[ "$build" == true ]]; then
  if ! command -v pkg-config >/dev/null || ! pkg-config --exists fontconfig wayland-client xkbcommon-x11 x11-xcb openssl libzstd vulkan; then
    echo "Native development libraries are missing. Install all build dependencies in docs/linux.md; do not create local library symlinks." >&2
    exit 1
  fi
  # Force the packaged target, even if the user's Cargo configuration selects
  # another target. Never build one path and silently ship an older executable.
  (cd "$project_root" && cargo build --release --locked -p gitturtle --target x86_64-unknown-linux-gnu --target-dir "$project_root/target")
fi
if [[ ! -x "$executable" ]]; then
  echo "Release executable missing. Run this script without --no-build." >&2
  exit 1
fi
bundle="${1:-$project_root/dist/gitturtle-linux-x86_64}"
if [[ -e "$bundle" || -e "$bundle.tar.gz" ]]; then
  echo "Output already exists: $bundle (or its .tar.gz). Choose a new output directory." >&2
  exit 1
fi
mkdir -p "$bundle/bin" "$bundle/icons"
bundle="$(cd "$bundle" && pwd)"
install -m755 "$executable" "$bundle/bin/gitturtle"
install -m755 "$project_root/scripts/install-linux.py" "$bundle/install.py"
install -m644 "$project_root/assets/app-icon.png" "$bundle/icons/app-icon.png"
install -m644 "$project_root/docs/linux.md" "$bundle/README.md"
python3 - "$project_root" "$bundle" "$build" <<'PY'
import hashlib
import json
from pathlib import Path
import subprocess
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
        path = bundle / f"icons/hicolor/{size}x{size}/apps/com.gitturtle.desktop.png"
        path.parent.mkdir(parents=True, exist_ok=True)
        image.resize((size, size), Image.Resampling.LANCZOS).save(path)
def git(*args):
    result = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)
    return result.stdout.strip() if result.returncode == 0 else "unavailable"
info = {
    "target": "x86_64-unknown-linux-gnu",
    "packaged_from_revision": git("rev-parse", "HEAD"),
    "packaging_tree_status": git("status", "--porcelain"),
    "release_built_by_packager": sys.argv[3] == "true",
    "binary_sha256": hashlib.sha256((bundle / "bin/gitturtle").read_bytes()).hexdigest(),
    "note": "With --no-build, verify the reused executable's source identity separately.",
}
(bundle / "build-info.json").write_text(json.dumps(info, indent=2) + "\n")
files = sorted(path for path in bundle.rglob("*") if path.is_file())
(bundle / "SHA256SUMS").write_text("".join(
    f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(bundle)}\n" for path in files
))
PY
# Neutral archive ownership avoids leaking the builder's account IDs and lets
# single-UID user namespaces extract normally without unmapped-owner failures.
tar --owner=0 --group=0 --numeric-owner -C "$(dirname "$bundle")" -czf "$bundle.tar.gz" "$(basename "$bundle")"
(cd "$(dirname "$bundle")" && sha256sum "$(basename "$bundle").tar.gz" > "$(basename "$bundle").tar.gz.sha256")
echo "Built $bundle.tar.gz"
echo "Extract anywhere, then run: python3 \"$bundle/install.py\""
