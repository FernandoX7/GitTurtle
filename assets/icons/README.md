# GitTurtle icons

Original SVG artwork created for this project. No third-party icon assets or GitKraken artwork are included.

Control icons use a 24 × 24 view box with rounded 1.65-unit white strokes. Render at 16–18 logical pixels for normal controls and use GPUI's SVG color to tint them. White makes raster-mask tinting reliable without depending on CSS `currentColor` interpretation. Keep aspect ratio and provide accessible labels on the containing control.

The canonical app icon is [AppIcon.icon](../AppIcon.icon/icon.json), an Icon Composer document with a transparent turtle foreground in `AppIcon.icon/Assets/turtle.png`. The foreground was extracted from the first built-in ImageGen turtle selected by the user on September 8, 2026. The original artwork is retained in Git at revision `9a2ae28`; [prompt metadata](../app-icon.prompt.json) records its provenance and the current derivatives.

The default appearance places the turtle over a full-bleed navy background; Dark uses the system charcoal background. Mono supplies clear light/dark and tinted appearances, with glass and specular effects enabled only for Mono. Let macOS apply the enclosure and corner mask: do not place another rounded tile behind the foreground. See Apple's [app icon guidance](https://developer.apple.com/design/human-interface-guidelines/app-icons) and [Icon Composer documentation](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer).

With Xcode 26 or later selected, regenerate the static default PNG, embedded 128-pixel branding PNG, four appearance previews, and fallback ICNS from the repository root:

```sh
./scripts/render-app-icon.sh
```

The [render script](../../scripts/render-app-icon.sh) writes `assets/app-icon.png`, `assets/branding/app-icon.png`, `assets/icon-previews/{light,dark,clear-light,clear-dark}.png`, and `assets/AppIcon.icns`. These are derived assets; edit the Icon Composer source first.

Native controls embed this icons directory. Repository and Projects headers and the collapsed sidebar share the embedded branding PNG, rendered at 24–32 logical pixels. GPUI loads and caches this small image asynchronously. Rebuild the executable after changing control SVGs or the embedded branding PNG.

The [package script](../../scripts/package-macos.sh) compiles `AppIcon.icon` with `actool` on every run, including `--no-build`. It ships `Assets.car` for native appearances and `AppIcon.icns` as the fallback, and merges the generated icon metadata into `Info.plist`. Raw artwork is excluded from the bundle. `--no-build` skips only the executable build; see [native QA](../../.agents/skills/gitturtle-native-qa/SKILL.md) for source identity and rendered-icon checks.
