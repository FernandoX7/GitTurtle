#!/bin/bash
set -euo pipefail

# Build a local, ad-hoc-signed app bundle. Distribution notarization and a
# Developer ID identity belong in a separate release workflow.
project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build=true
profile=release
if [[ "${1:-}" == "--debug" ]]; then
  profile=debug
  shift
fi
if [[ "${1:-}" == "--no-build" ]]; then
  build=false
  shift
fi
if [[ "${1:-}" == "--help" || "$#" -gt 1 ]]; then
  echo "Usage: scripts/package-macos.sh [--debug] [--no-build] [output/GitTurtle.app]"
  exit 0
fi
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS app bundles must be packaged on macOS." >&2
  exit 1
fi

bundle="${1:-$project_root/dist/GitTurtle.app}"
if [[ "$bundle" != *.app ]]; then
  echo "The output path must end in .app." >&2
  exit 1
fi
cd "$project_root"
if [[ "$build" == true ]]; then
  if [[ "$profile" == release ]]; then
    cargo build --release --locked -p gitturtle --target-dir "$project_root/target"
  else
    cargo build --locked -p gitturtle --target-dir "$project_root/target"
  fi
fi
executable="$project_root/target/$profile/gitturtle"
if [[ ! -x "$executable" ]]; then
  echo "Executable missing. Run this script without --no-build." >&2
  exit 1
fi
if [[ -e "$bundle" ]]; then
  identifier="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Info.plist" 2>/dev/null || true)"
  if [[ "$identifier" != "com.gitturtle.desktop" ]]; then
    echo "Refusing to replace an existing path that is not a GitTurtle bundle: $bundle" >&2
    exit 1
  fi
fi
package_id="$(cargo pkgid -p gitturtle)"
version="${package_id##*@}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][A-Za-z0-9.-]+)?$ ]]; then
  echo "Could not read the GitTurtle package version." >&2
  exit 1
fi

if ! xcrun --find actool >/dev/null 2>&1; then
  echo "Xcode 26 or later is required to compile the native icon appearances." >&2
  exit 1
fi
icon_build="$(mktemp -d "${TMPDIR:-/tmp}/gitturtle-icon.XXXXXX")"
trap 'rm -rf "$icon_build"' EXIT
xcrun actool "$project_root/assets/AppIcon.icon" \
  --compile "$icon_build" --platform macosx --minimum-deployment-target 11.0 \
  --app-icon AppIcon --output-partial-info-plist "$icon_build/icon-info.plist" \
  --output-format human-readable-text --warnings --notices

mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
install -m 755 "$executable" "$bundle/Contents/MacOS/gitturtle"
# UI artwork is embedded; ship only the compiled macOS icon resources.
rm -rf "$bundle/Contents/Resources/assets"
install -m 644 "$icon_build/AppIcon.icns" "$bundle/Contents/Resources/AppIcon.icns"
install -m 644 "$icon_build/Assets.car" "$bundle/Contents/Resources/Assets.car"
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>GitTurtle</string>
  <key>CFBundleDisplayName</key><string>GitTurtle</string>
  <key>CFBundleExecutable</key><string>gitturtle</string>
  <key>CFBundleIdentifier</key><string>com.gitturtle.desktop</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
/usr/libexec/PlistBuddy -c "Merge $icon_build/icon-info.plist" "$bundle/Contents/Info.plist"
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --deep --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
echo "Built $bundle"
echo "Open with: open \"$bundle\""
