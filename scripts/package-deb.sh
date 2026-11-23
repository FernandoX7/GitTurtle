#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/package-deb.sh --bundle <linux-bundle-dir> [--output <path.deb>]

Repackages a verified GitTurtle Linux release bundle (produced by
scripts/package-linux.sh) into a Debian/Ubuntu .deb package using dpkg-deb.
EOF
}

bundle=""
output=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle)
      [[ $# -ge 2 ]] || { echo "--bundle requires a directory path." >&2; exit 1; }
      bundle="$2"
      shift 2
      ;;
    --output)
      [[ $# -ge 2 ]] || { echo "--output requires a file path." >&2; exit 1; }
      output="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$bundle" ]]; then
  echo "Missing --bundle <linux-bundle-dir>." >&2
  usage >&2
  exit 1
fi

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "Missing dpkg-deb. Install dpkg on Ubuntu/Debian (or dpkg on Arch) before building a .deb." >&2
  exit 1
fi

if [[ ! -d "$bundle" ]]; then
  echo "Bundle directory does not exist: $bundle" >&2
  exit 1
fi
bundle="$(cd "$bundle" && pwd)"

for required in "bin/gitturtle" "build-info.json" "README.md" "licenses/LICENSE"; do
  if [[ ! -f "$bundle/$required" || -L "$bundle/$required" ]]; then
    echo "Bundle is missing required regular file: $required" >&2
    exit 1
  fi
done

if [[ ! -d "$bundle/icons/hicolor" ]]; then
  echo "Bundle is missing icons/hicolor directory." >&2
  exit 1
fi

meta_out="$(python3 - "$bundle/build-info.json" <<'PY'
import json, re, sys
data = json.load(open(sys.argv[1], encoding="utf-8"))
version = data.get("version", "").strip()
target = data.get("target", "").strip()
if not version or not target:
    raise SystemExit("build-info.json is missing version or target")
if not re.fullmatch(r"[0-9][A-Za-z0-9.+~:-]*", version):
    raise SystemExit(f"Invalid Debian package version: {version!r}")
arch_map = {
    "x86_64-unknown-linux-gnu": "amd64",
    "aarch64-unknown-linux-gnu": "arm64",
}
if target not in arch_map:
    raise SystemExit(f"Unsupported Linux target for .deb packaging: {target}")
print(version)
print(arch_map[target])
PY
)"
version="${meta_out%%$'\n'*}"
deb_arch="${meta_out#*$'\n'}"
if [[ -z "$version" || -z "$deb_arch" || "$version" == "$meta_out" ]]; then
  echo "Failed to parse version and architecture from build-info.json." >&2
  exit 1
fi

if [[ -z "$output" ]]; then
  output="$(dirname "$bundle")/gitturtle_${version}_${deb_arch}.deb"
fi

if [[ -e "$output" || -L "$output" ]]; then
  echo "Output already exists: $output" >&2
  exit 1
fi

out_parent="$(dirname "$output")"
mkdir -p "$out_parent"
out_parent="$(cd "$out_parent" && pwd)"
output="$out_parent/$(basename "$output")"

staging="$(mktemp -d "$out_parent/.gitturtle-deb.XXXXXX")"
trap 'rm -rf "$staging"' EXIT

pkg="$staging/pkg"
app_id="com.gitturtle.GitTurtle"
mkdir -p \
  "$pkg/DEBIAN" \
  "$pkg/usr/bin" \
  "$pkg/usr/share/applications" \
  "$pkg/usr/share/doc/gitturtle/licenses"

install -m755 "$bundle/bin/gitturtle" "$pkg/usr/bin/gitturtle"
install -m644 "$bundle/README.md" "$pkg/usr/share/doc/gitturtle/README.md"
install -m644 "$bundle/licenses/LICENSE" "$pkg/usr/share/doc/gitturtle/copyright"

while IFS= read -r -d '' rel; do
  rel="${rel#./}"
  dest="$pkg/usr/share/doc/gitturtle/licenses/$rel"
  mkdir -p "$(dirname "$dest")"
  install -m644 "$bundle/licenses/$rel" "$dest"
done < <(cd "$bundle/licenses" && find . -type f -print0)

for size in 16 24 32 48 64 128 256 512; do
  icon_src="$bundle/icons/hicolor/${size}x${size}/apps/${app_id}.png"
  if [[ ! -f "$icon_src" || -L "$icon_src" ]]; then
    echo "Bundle is missing regular icon file: icons/hicolor/${size}x${size}/apps/${app_id}.png" >&2
    exit 1
  fi
  mkdir -p "$pkg/usr/share/icons/hicolor/${size}x${size}/apps"
  install -m644 "$icon_src" "$pkg/usr/share/icons/hicolor/${size}x${size}/apps/${app_id}.png"
done

cat > "$pkg/usr/share/applications/${app_id}.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=GitTurtle
Comment=Browse Git history and manage local repositories
Exec=gitturtle %f
Icon=${app_id}
Terminal=false
Categories=Development;RevisionControl;
Keywords=Git;VersionControl;VCS;Repository;Commit;Diff;Branch;PR;Merge;
SingleMainWindow=true
StartupWMClass=${app_id}
EOF
chmod 644 "$pkg/usr/share/applications/${app_id}.desktop"

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$pkg/usr/share/applications/${app_id}.desktop"
fi

installed_kb="$(du -sk "$pkg/usr" | awk '{print $1}')"

cat > "$pkg/DEBIAN/control" <<EOF
Package: gitturtle
Version: ${version}
Section: vcs
Priority: optional
Architecture: ${deb_arch}
Installed-Size: ${installed_kb}
Maintainer: GitTurtle Contributors <gitturtle@users.noreply.github.com>
Homepage: https://github.com/FernandoX7/GitTurtle
Depends: libc6, libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-egl1, libvulkan1, libegl1, git, hicolor-icon-theme
Recommends: libdav1d7, xdg-desktop-portal, xdg-utils, libglib2.0-0t64 | libglib2.0-0, fontconfig, openssh-client
Suggests: git-lfs, gh
Description: Fast native Git client for history inspection and everyday Git work
 GitTurtle is a fast native Rust/GPUI Git client built for clear history
 inspection, diff review, and everyday repository work.
EOF
chmod 644 "$pkg/DEBIAN/control"

cat > "$pkg/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
EOF
chmod 755 "$pkg/DEBIAN/postinst"

cat > "$pkg/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
EOF
chmod 755 "$pkg/DEBIAN/postrm"

staged_deb="$staging/gitturtle.deb"
dpkg-deb --build --root-owner-group "$pkg" "$staged_deb"
mv "$staged_deb" "$output"
echo "Packaged $output"
