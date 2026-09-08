# Liquid Glass investigation — September 8, 2026

The milestone retains an opaque navigation treatment. The native prototype exposed a concrete compositing problem with the pinned GPUI renderer, so it is not presented as a finished Liquid Glass feature. No performance benefit or compatibility result follows from this investigation.

## Native prototype and observed failure

The prototype instantiated the real macOS 26 `NSGlassEffectView` through runtime class lookup, sized it to the 56-point header and inserted it below other subviews of GPUI's native view. It enabled a transparent window and removed the app header's own opaque fill. After activating the app, the material sampled and blurred the GPUI toolbar labels themselves. [The captured prototype](evidence/macos-milestone/glass-prototype-obscured-toolbar.jpg) records this failed integration. The native material's inactive and accessibility fallbacks did not resolve the active-window compositing problem.

`NSWindowOrderingMode::Below` orders sibling **subviews**; it cannot insert a child behind selected drawing primitives in its parent's Metal layer. GPUI draws the header controls, history, editors and comparisons into that same native surface. Moving the material into the parent as a sibling could avoid sampling the toolbar's parent layer, but it would still fail the supported Liquid Glass content relationship described below.

## Supported AppKit relationship

Apple says custom glass should own its foreground controls through `NSGlassEffectView.contentView`, and advises against positioning a glass view behind a sibling content view. This gives AppKit the correct relationship for adaptive appearance and legibility. [WWDC25: Build an AppKit app with the new design, Glass chapter](https://developer.apple.com/videos/play/wwdc2025/310/?time=1050)

The API documentation only guarantees the intended glass/content ordering for `contentView`; arbitrary subviews have no corresponding guarantee. [NSGlassEffectView.contentView](https://developer.apple.com/documentation/appkit/nsglasseffectview/contentview)

The locally inspected pinned sources establish the limitation:

- `gpui-pre-macos 0.3.4`, `src/window.rs:1012–1034` creates one window-sized `GPUIView` and renderer; lines 1104–1119 attach that layer-backed view to the window. The exposed raw handle points to this view, not a separate toolbar surface.
- `gpui-component 0.6.0`, `src/root.rs:582–599` paints a full-window theme background before the app view. Making the app's header transparent alone therefore cannot reveal material below that surface.
- `objc2-app-kit 0.3.2`, generated `NSGlassEffectView` bindings include the same documented `contentView` ordering contract.

A header-sized glass `contentView` cannot own only the header pixels of this full-window GPUI surface. Wrapping the entire surface would change the material's geometry and sampling to the whole window; masking that wrapper also clips its foreground content. Those are not a verified, confined navigation treatment. A sound implementation needs a distinct native toolbar/control hierarchy or independently composited GPUI toolbar surface, with coordinated layout, event routing, focus and accessibility. That is beyond a bounded appearance adjustment and was not introduced in this milestone.

## Finished fallback and coverage

The failed prototype is replaced by the existing opaque theme surfaces. Light/dark themes, inactive windows, older macOS versions and Linux retain that dependable path. The fallback introduces no glass animation, background sampling or transparency dependency. Native `NSVisualEffectView` vibrancy and a simulated glass treatment were not substituted or labelled Liquid Glass. There is no measured glass rendering cost, older-Mac result, older-macOS result or Linux-native result to claim.
