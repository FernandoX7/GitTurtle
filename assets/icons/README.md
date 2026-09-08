# GitTurtle icons

Original SVG artwork created for this project. No third-party icon assets or GitKraken artwork are included.

Control icons use a 24 × 24 view box with rounded 1.65-unit white strokes; the turtle mark uses a filled silhouette for clarity at small sizes. Render at 16–18 logical pixels for normal controls and use GPUI's SVG color to tint them. White makes raster-mask tinting reliable without depending on CSS `currentColor` interpretation. Keep aspect ratio and provide accessible labels on the containing control. The standalone turtle mark is in `../turtle-mark.svg`.

The app artwork is the first built-in ImageGen turtle, selected by the user on September 8, 2026, in `../app-icon.png`. Its original pixels are retained, with the prompt and source hash in `../app-icon.prompt.json`. The PNG preserves genuine alpha outside the rounded tile. `../app-icon.svg` and `../turtle-mark.svg` are simplified vector companions, not the source of the packaged app icon. Rebuild the packaged icon with:

```sh
cargo run --locked -p gitturtle-preview --example render_icon -- assets/app-icon.png .local/GitTurtle.iconset
iconutil --convert icns --output assets/AppIcon.icns .local/GitTurtle.iconset
```

Native controls embed only this icons directory; the large app artwork stays in the package resources.
