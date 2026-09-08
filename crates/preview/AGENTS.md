# Image preview guidance

These instructions supplement the root agreements for `crates/preview`. Decoder code and focused fixtures live in [src/lib.rs](src/lib.rs); [examples/render_icon.rs](examples/render_icon.rs) builds macOS iconset PNGs through the same decoder.

## Decoder contracts

- `decode_image` consumes supplied bytes. The filename is a format hint, never a path to open; raster magic takes precedence. `detect_lfs_pointer` returns metadata only. Verified local LFS resolution belongs to [Git core](../git-core/AGENTS.md), and missing sides must remain distinct from empty image bytes in the caller.
- Keep source-byte, source-dimension, decoded-allocation, and output-dimension limits independent. Reject unreasonable headers before decoding pixels and validate SVG complexity before rendering. Codec allocation limits are best effort, not a process-memory sandbox or wall-clock deadline. Run decoding on bounded background workers.
- `ImagePreview.rgba` is straight RGBA8. Preserve transparency, premultiply while resizing to prevent invisible colors bleeding into edges, then return straight alpha. Keep aspect ratio, never enlarge previews, apply raster orientation before reporting original dimensions, and retain GIF's first-frame behavior. GPUI's BGRA conversion belongs to [the app worker](../app/src/worker.rs).
- SVG previews are static and self-contained. Preserve explicit refusals for DTDs, linked or embedded image resources, nonfragment `href` values, and filters; disabling `resources_dir` alone does not prevent absolute-path reads. Keep both image resolvers disabled and load system fonts once, only when text appears. Adding unsupported features requires bounded behavior and honest preview results.

## App icon pipeline

The production app icon uses the layered `assets/AppIcon.icon` source and Xcode 26 or later. Follow the [asset conventions](../../assets/icons/README.md) and [render script](../../scripts/render-app-icon.sh) for static previews, embedded branding, and the fallback ICNS. `render_icon` remains a standalone decoder example that accepts square images of at least 1024 pixels and preserves alpha; it does not produce the native appearance catalog.

The [package script](../../scripts/package-macos.sh) compiles the layered source on every run, including `--no-build`, and ships the generated appearance catalog, fallback ICNS, and icon metadata. After an icon change, inspect small and large rendered sizes and verify the packaged appearances using the [native QA procedure](../../.agents/skills/gitturtle-native-qa/SKILL.md). Keep artwork regeneration scoped to asset changes.

## Verification

Run `cargo test --locked -p gitturtle-preview` or a relevant filter for decoder changes. Focus fixtures on output pixels/dimensions, supported format behavior, and refusal boundaries; use the root final checks for Rust integration. Guidance-only edits need link, symbol, and diff checks. Native QA applies when image presentation or the packaged icon changes, not to every decoder edit.
