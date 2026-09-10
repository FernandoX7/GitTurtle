# Preview decoder guidance

These instructions supplement the root agreements for `crates/preview`. The [support matrix](../../docs/file-previews.md) distinguishes rendered, source, metadata and external support; [document preview contracts](../../docs/document-previews.md) describe native PDF/text, Markdown resources and retained model behavior.

## Find the relevant decoder

| Concern | Source and focused fixtures |
| --- | --- |
| Raster/SVG decode, alpha and orientation | [src/lib.rs](src/lib.rs); [src/svg_limits.rs](src/svg_limits.rs) for SVG preflight |
| Composited GIF animation and budgets | [src/animation.rs](src/animation.rs) |
| Supplied-byte ImageIO/PDF pages and selected-page text | [src/native.rs](src/native.rs), [src/native/pdf_text.rs](src/native/pdf_text.rs) |
| Container metadata and transformed text | [src/metadata.rs](src/metadata.rs) |
| Static Mermaid | [src/mermaid.rs](src/mermaid.rs) |
| Mesh decoding and retained camera/raster views | [src/model3d.rs](src/model3d.rs), [src/model3d/camera.rs](src/model3d/camera.rs), [src/model3d/tests.rs](src/model3d/tests.rs) |
| GLB framing, embedded accessors and static scenes | [src/model3d/glb.rs](src/model3d/glb.rs); [GLB contract](../../docs/interactive-3d.md#glb-20-static-geometry), [GLB fixtures](tests/fixtures/models/glb/README.md) |
| STEP faceted solids, analytic primitives, instances and units | [src/model3d/step.rs](src/model3d/step.rs), [scene.rs](src/model3d/step/scene.rs), [primitives.rs](src/model3d/step/primitives.rs) |

Most tests are inline in the owning module. Checked-in binary fixtures and their provenance are documented in [tests/fixtures/README.md](tests/fixtures/README.md) and [model fixtures](tests/fixtures/models/README.md).

## Decoder contracts

- `decode_image` consumes supplied bytes. The filename is a format hint, never a path to open; raster magic takes precedence. `detect_lfs_pointer` returns metadata only. Verified local LFS resolution belongs to [Git core](../git-core/AGENTS.md), and missing sides must remain distinct from empty image bytes in the caller.
- Keep source-byte, source-dimension, decoded-allocation, and output-dimension limits independent. Reject unreasonable headers before decoding pixels and validate SVG complexity before rendering. Codec allocation limits are best effort, not a process-memory sandbox or wall-clock deadline. Run decoding on bounded background workers.
- `ImagePreview.rgba` is straight RGBA8. Preserve transparency, premultiply while resizing to prevent invisible colors bleeding into edges, then return straight alpha. Keep aspect ratio, never enlarge previews, and apply raster orientation before reporting original dimensions. `decode_image` retains first-frame GIF behavior; `animation::decode_gif` separately bounds composed frames, elapsed duration, aggregate source/output pixels, and cooperative decode time. GPUI's BGRA conversion belongs to [the app worker](../app/src/worker.rs).
- SVG previews are static and self-contained. Preserve explicit refusals for DTDs, linked or embedded image resources, nonfragment `href` values, and filters; disabling `resources_dir` alone does not prevent absolute-path reads. Bound `use` and marker expansion before usvg and font loading, independently from XML size/depth. Keep renderer-compatible namespace/fragment resolution and refuse marker CSS that the structural preflight cannot account for. Keep both image resolvers disabled and load system fonts once, only when text appears. Adding unsupported features requires bounded behavior and honest preview results.
- Native ImageIO, CoreGraphics and PDFKit references remain on one worker stack and consume supplied CFData only. Return owned pixels/text; never send native document references to the UI. PDF pages are capped independently from source bytes and pixel bounds, with cancellation between phases. [Selected-page text](src/native/pdf_text.rs) uses a scoped autorelease pool and caps returned UTF-8 independently at 256 KiB; absence and truncation stay explicit. Text-layer order may differ from visual layout; no OCR is requested. Generic container metadata never decompresses archives, installs fonts or executes document actions. Dedicated 3MF geometry decoding may decompress bounded selected XML members in memory, with no filesystem extraction. Decoded UTF-16 and PDF text are explicitly transformed source and never a Git patch.
- Mermaid retains exact literal source separately from its static diagram. Bound source/graph complexity before layout and generated SVG before rasterization. Refuse source configuration/actions/resources; do not call the renderer's filesystem/CLI helpers or restore its disabled disk font cache. Keep cancellation between phases and individual diagrams, with honest errors/caps and no claim of complete Mermaid compatibility.
- Mesh/CAD decoders never load repository-relative materials, textures, geometry references, scripts or caches. Bound expanded geometry and raster work independently of compressed input bytes and preserve finite coordinates, supported transforms and explicit units. STEP retains its finite supported geometry/placement contract; refuse unsupported geometry rather than inventing a bounding box or point cloud. Label tessellated analytic geometry, default poses and unknown physical scale honestly.
- `decode_geometry` returns immutable retained `ModelScene` geometry; interactive consumers render only the requested camera through `render_model` on a cancellable worker. Account for retained triangles as well as pixels. Initial linked cameras fit both revisions' union bounds so normalization cannot hide size or position changes; preserve explicit independent-camera inspection. `decode_model` retains fixed views for secondary static consumers. Views remain inspection aids rather than geometry edits or manufacturing validation.

## App icon consumer

[examples/render_icon.rs](examples/render_icon.rs) is a standalone decoder example for square images of at least 1024 pixels and preserves alpha. The production icon uses layered `assets/AppIcon.icon`; follow the [asset conventions](../../assets/icons/README.md) for its render and package pipeline. The decoder example does not produce the native appearance catalog.

## Verification

Run `cargo test --locked -p gitturtle-preview` or a relevant filter for decoder changes. Focus fixtures on output pixels/dimensions, supported format behavior, and refusal boundaries; use the root final checks for Rust integration. Guidance-only edits need link, symbol, and diff checks. Native QA applies when image presentation or the packaged icon changes, not to every decoder edit.
