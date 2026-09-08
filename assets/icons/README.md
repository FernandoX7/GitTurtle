# GitTurtle icons

Original SVG artwork created for this project. No third-party icon assets or GitKraken artwork are included.

Control icons use a 24 × 24 view box with rounded 1.65-unit white strokes. Render at 16–18 logical pixels for normal controls and use GPUI's SVG color to tint them. White makes raster-mask tinting reliable without depending on CSS `currentColor` interpretation. Keep aspect ratio and provide accessible labels on the containing control.

The app artwork is the first built-in ImageGen turtle, selected by the user on September 8, 2026, in `../app-icon.png`. Its original pixels are retained, with the prompt and source hash in `../app-icon.prompt.json`. The PNG preserves genuine alpha outside the rounded tile. Both the macOS icon and the in-app branding use this same source; obsolete vector turtle variants have been removed. Rebuild both derived assets with:

```sh
cargo run --locked -p gitturtle-preview --example render_icon -- assets/app-icon.png .local/GitTurtle.iconset
iconutil --convert icns --output assets/AppIcon.icns .local/GitTurtle.iconset
mkdir -p assets/branding
cp .local/GitTurtle.iconset/icon_128x128.png assets/branding/app-icon.png
```

Native controls embed this icons directory. Repository and Projects headers and the collapsed sidebar share the embedded 128-pixel branding PNG, rendered at 24–32 logical pixels. GPUI loads and caches this small image asynchronously. The full-size artwork stays in the package resources. Rebuild the executable after changing the embedded branding PNG.
