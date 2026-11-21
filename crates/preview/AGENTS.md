# Preview decoder guidance

These instructions supplement root [AGENTS.md](../../AGENTS.md) for `crates/preview`. See [file-previews.md](../../docs/file-previews.md) for the support matrix and [document-previews.md](../../docs/document-previews.md) for native PDF/text, Markdown resources, and retained 3D model contracts.

## Find the relevant decoder

| Concern | Source and focused fixtures |
| --- | --- |
| Raster/SVG decode, alpha, and orientation | [src/lib.rs](src/lib.rs); [src/svg_limits.rs](src/svg_limits.rs) for SVG preflight |
| Composited GIF animation and budgets | [src/animation.rs](src/animation.rs) |
| Supplied-byte ImageIO/PDF pages and text (macOS) | [src/native.rs](src/native.rs), [src/jpeg2000.rs](src/jpeg2000.rs) (unconfined JP2/J2K box/SIZ dimension reader on both platforms), [src/native/pdf_text.rs](src/native/pdf_text.rs) |
| Linux confined PDF helper (hayro) | [src/pdf_hayro.rs](src/pdf_hayro.rs), [src/pdf_helper.rs](src/pdf_helper.rs); [architecture](../../docs/development/2026-11-02-linux-pdf-pages-architecture.md) |
| Linux confined AVIF & JPEG 2000 helper | [src/image_helper.rs](src/image_helper.rs), [src/avif.rs](src/avif.rs), [src/jpeg2000_hayro.rs](src/jpeg2000_hayro.rs), [src/jpeg2000.rs](src/jpeg2000.rs); [fixtures](tests/fixtures/README.md); [architecture](../../docs/development/2026-11-02-linux-avif-jpeg2000-architecture.md) |
| Container signatures and transformed text | [src/metadata.rs](src/metadata.rs) |
| Static Mermaid | [src/mermaid.rs](src/mermaid.rs) |
| Mesh decoding and retained camera/raster views | [src/model3d.rs](src/model3d.rs), [src/model3d/camera.rs](src/model3d/camera.rs), [src/model3d/tests.rs](src/model3d/tests.rs) |
| GLB framing, appearance, meshopt, deformation | [src/model3d/glb.rs](src/model3d/glb.rs), [appearance.rs](src/model3d/glb/appearance.rs), [compression.rs](src/model3d/glb/compression.rs), [deformation.rs](src/model3d/glb/deformation.rs); [GLB contract](../../docs/interactive-3d.md#glb-20-appearance-deformation-and-animation) |
| STEP faceted solids and analytic primitives | [src/model3d/step.rs](src/model3d/step.rs), [scene.rs](src/model3d/step/scene.rs), [primitives.rs](src/model3d/step/primitives.rs) |

Most tests live beside their decoder module. Binary fixtures and provenance are tracked in [tests/fixtures/README.md](tests/fixtures/README.md) and [tests/fixtures/models/README.md](tests/fixtures/models/README.md).

## Extending a decoder

Keep format parsing, validation, and optional resource handling inside the owning decoder module. Public decode results carry owned data and explicit unsupported, absent, or truncated states; decoders never hold repository handles, app state, or native UI types. `crates/git-core` captures repository bytes and local LFS assets; `crates/app` owns scheduling, cancellation generations, and cache retention.

## Decoder contracts

### General raster, GIF, SVG, and container metadata

- `decode_image` consumes supplied bytes only (filename is a format hint, never a path to open; magic bytes take precedence). `detect_lfs_pointer` returns metadata only; verified local LFS resolution belongs to [crates/git-core/AGENTS.md](../git-core/AGENTS.md).
- Keep source-byte, source-dimension, decoded-allocation, and output-dimension limits independent. Reject unreasonable headers before pixel allocation; run decoding on bounded background workers.
- `ImagePreview.rgba` is straight RGBA8 (premultiply during resize, return straight alpha, preserve aspect ratio, never enlarge, apply raster orientation before reporting original dimensions). `animation::decode_gif` bounds composed frames, duration, aggregate pixels, and cooperative time. BGRA conversion belongs to [crates/app/src/worker.rs](../app/src/worker.rs).
- SVG previews are static and self-contained: refuse DTDs, external/embedded image resources, nonfragment `href`, and filters; bound `use` and marker expansion before `usvg`; keep both image resolvers disabled and load system fonts once only when text appears.
- Generic container metadata never decompresses archives, installs fonts, or triggers document actions. `metadata::container` inspects leading magic without file names for Linux default-app handoff (deliberately omitting OOXML/OpenDocument bodies; see [file-previews.md](../../docs/file-previews.md#background-work-and-resource-lifetime)). Decoded UTF-16 and PDF text are explicitly transformed source, never a Git patch.

### macOS native ImageIO / PDFKit

- Keep ImageIO, CoreGraphics, and PDFKit references on one worker stack consuming supplied `CFData` only; return owned pixels/text.
- Cap PDF pages independently from source bytes and pixel bounds, with cancellation between phases. [src/native/pdf_text.rs](src/native/pdf_text.rs) uses a scoped autorelease pool and caps UTF-8 independently at 256 KiB with explicit absence/truncation (no OCR).

### Linux confined PDF helper (`pdf_hayro` / `pdf_helper`)

- On Linux x86-64 and AArch64, `hayro` renders PDF pages only inside the seccomp-confined helper via `pdf_helper::serve`; `decode_pdf_page` and `decode_pdf_page_text` keep refusing in-process on Linux.
- Preserve the macOS page contract (crop box, rotation, white background, 1,000 px max edge from `render_dimensions()` + `check_dimensions`, no annotations/actions) and the 256 KiB UTF-8 text cap.
- Validate request size, page bound, and `%PDF-` magic before allocating; `read_reply` checks every field against bounds and refuses trailing bytes. Error replies use fixed kinds only; bump `PROTOCOL_VERSION` on any wire-frame change.

### Linux confined AVIF & JPEG 2000 helper (`image_helper` / `avif` / `jpeg2000_hayro`)

- Confined routing: `confined_image_format` is the single router (AVIF, JP2, J2K; never HEIF); `decode_image` keeps refusing all three (and HEIF separately) in-process on Linux.
- Safety & version floor: decoding modules carry `#![forbid(unsafe_code)]`; `libdav1d_version` in `src/lib.rs` is the sole `unsafe` call, rejecting `libdav1d < 1.4.1` before `mp4parse` runs. Request framing validates header, version, format, edge (`1..=1600`), length, and 4,096-byte prefix sniff before allocating the body.
- `dav1d` contract: fresh `Settings` (`n_threads = 1`, `max_frame_delay = 1`, `frame_size_limit = ispe` area, operating point 0, layers off, film grain on, invisible frames off), one decoder per item, bounded send/get loop, primary decoded and freed before alpha so at most one picture lives beside RGBA output.
- Color profiles (`moxcms`): bounded RGB ICC profile (`<= 4 MiB`) converts RGBA to sRGB in place via `image_helper` (`moxcms` is confined to `image_helper.rs`, enforced by the source test in `avif.rs`); malformed, oversized, or non-RGB profiles draw without conversion and flag `"color profile not applied"`.
- JPEG 2000 preflight & budget: validate JP2 box structure, `ihdr`/SIZ agreement, origins, `<= 65,535` tiles, `1..=4` components (`<= 16` bits), unsigned `pclr` columns, `cmap`/`cdef`, and 256 MiB plane budget (`profile_planes` / `cmyk_planes`), selecting the deepest resolution level within budget via `jpeg2000_hayro::open`.
- Source-boundary test (`avif.rs`): verifies the exact pinned `dav1d` (`=0.11.1`), `dav1d-sys` (`=0.8.3`), and `moxcms` (`=0.8.1`) crates.io dependencies in `Cargo.toml`/`Cargo.lock`, enforces the `dav1d-rs` call allowlist, forbids `include*`/`#[path]`/`macro_rules!` and build scripts in `crates/preview`, and checks ABI + CPU feature flags via the libdav1d ABI probe. Fixtures come from `scripts/create-image-codec-fixtures.py`.

### Mermaid and 3D / CAD (`model3d`)

- Mermaid preserves exact literal source separately from the static diagram, bounds source/graph/SVG complexity, refuses source config/actions/external resources, and keeps renderer disk font caching disabled.
- Mesh/CAD decoders never load external repository materials, textures, scripts, or caches. STEP preserves finite supported geometry and refuses unsupported geometry honestly.
- GLB separates structural geometry failures (invalid skins/morphs must fail) from optional appearance/animation fallbacks (explicit disclosure). Embedded glTF textures preserve encoded orientation/color rather than standalone raster orientation.
- `decode_geometry` returns immutable retained `ModelScene` data; interactive views evaluate poses via `ModelScene::evaluate_animation` and render via `render_model` with `ModelScene::retained_bytes` accounting. Initial linked cameras fit both revisions' union bounds.

## App icon

- [examples/render_icon.rs](examples/render_icon.rs) is a standalone square-image decoder example; production icons use `assets/AppIcon.icon` ([assets/icons/README.md](../../assets/icons/README.md)).

## Verification

- Run `cargo test --locked -p gitturtle-preview` (or focused module filters) for decoder changes, plus workspace combined checks before integration. Native QA is required only when UI presentation or the packaged icon changes.
