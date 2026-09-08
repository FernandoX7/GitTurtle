#!/bin/bash
set -euo pipefail

# Render the same layered source used by macOS into static project previews.
project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
developer_dir="$(xcode-select -p)"
icon_tool="$developer_dir/../Applications/Icon Composer.app/Contents/Executables/ictool"
if [[ ! -x "$icon_tool" ]]; then
  echo "Select Xcode 26 or later to render Icon Composer artwork." >&2
  exit 1
fi
cd "$project_root"
mkdir -p assets/branding assets/icon-previews
render_icon() {
  "$icon_tool" assets/AppIcon.icon --export-image --output-file "$2" \
    --platform macOS --rendition "$1" --width "$3" --height "$3" --scale 1
}
render_icon Default assets/app-icon.png 1024
render_icon Default assets/branding/app-icon.png 128
render_icon Default assets/icon-previews/light.png 256
render_icon Dark assets/icon-previews/dark.png 256
render_icon ClearLight assets/icon-previews/clear-light.png 256
render_icon ClearDark assets/icon-previews/clear-dark.png 256

icon_build="$(mktemp -d "${TMPDIR:-/tmp}/gitturtle-icon.XXXXXX")"
trap 'rm -rf "$icon_build"' EXIT
xcrun actool assets/AppIcon.icon --compile "$icon_build" \
  --platform macosx --minimum-deployment-target 11.0 --app-icon AppIcon \
  --output-partial-info-plist "$icon_build/icon-info.plist" \
  --output-format human-readable-text --warnings --notices
install -m 644 "$icon_build/AppIcon.icns" assets/AppIcon.icns
echo "Rendered light, dark, and clear icons; rebuild the app for embedded branding changes."
