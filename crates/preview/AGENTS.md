# Image preview guidance

These instructions supplement the root agreements for `crates/preview`. Decoder code and focused fixtures live in [src/lib.rs](src/lib.rs); [examples/render_icon.rs](examples/render_icon.rs) builds macOS iconset PNGs through the same decoder.

## Decoder contracts

- `decode_image` consumes supplied bytes. The filename is a format hint, never a path to open; raster magic takes precedence. `detect_lfs_pointer` returns metadata only. Verified local LFS resolution belongs to [Git core](../git-core/AGENTS.md), and missing sides must remain distinct from empty image bytes in the caller.
- Keep source-byte, source-dimension, decoded-allocation, and output-dimension limits independent. Reject unreasonable headers before decoding pixels and validate SVG complexity before rendering. Codec allocation limits are best effort, not a process-memory sandbox or wall-clock deadline. Run decoding on bounded background workers.
- `ImagePreview.rgba` is straight RGBA8. Preserve transparency, premultiply while resizing to prevent invisible colors bleeding into edges, then return straight alpha. Keep aspect ratio, never enlarge previews, apply raster orientation before reporting original dimensions, and retain GIF's first-frame behavior. GPUI's BGRA conversion belongs to [the app worker](../app/src/worker.rs).
- SVG previews are static and self-contained. Preserve explicit refusals for DTDs, linked or embedded image resources, nonfragment `href` values, and filters; disabling `resources_dir` alone does not prevent absolute-path reads. Keep both image resolvers disabled and load system fonts once, only when text appears. Adding unsupported features requires bounded behavior and honest preview results.

## App icon pipeline

Use the selected project asset as the source for `render_icon`; it accepts the decoder's supported formats and requires square dimensions of at least 1024 pixels. Preserve its alpha. Generate the complete iconset in a disposable output directory, then use macOS `iconutil` to produce `assets/AppIcon.icns`. Keep the selected source in the workspace; app artwork and small monochrome UI marks may have different sources.

The [package script](../../scripts/package-macos.sh) copies the existing ICNS; it does not regenerate it. After an icon change, inspect small and large rendered sizes and verify the packaged icon using the [native QA procedure](../../.agents/skills/gitturtle-native-qa/SKILL.md). Keep artwork regeneration scoped to asset changes.

## Verification

Run `cargo test --locked -p gitturtle-preview` or a relevant filter for decoder changes. Focus fixtures on output pixels/dimensions, supported format behavior, and refusal boundaries; use the root final checks for Rust integration. Guidance-only edits need link, symbol, and diff checks. Native QA applies when image presentation or the packaged icon changes, not to every decoder edit.
