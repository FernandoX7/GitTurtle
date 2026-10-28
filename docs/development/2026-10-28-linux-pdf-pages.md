# Linux PDF pages (2026-10-28)

Research for the owner decision of 2026-10-28 in [full Omarchy support](omarchy-full-support.md) (item 9: Linux reaches feature parity with macOS where possible), using the [research template](research-template.md). Base revision: `c219a6f`.

## Decision and scope

- **Question:** how a Linux build renders captured PDF pages with the behavior macOS has through CoreGraphics and PDFKit: one selected page at a time, Previous/Next and direct page entry, linked or independent Before/After pages, Fit and 25–400% zoom over a bounded raster, and the selected page's "Read page text" alternative ([contract](../document-previews.md#pdf-pages-and-text)).
- **User-visible outcome:** a PDF on Linux opens in the same native page view as on macOS instead of metadata and an unavailable result. Only the decoder changes. Page navigation, linking, Fit, zoom and the four-page, 16 MiB per-side cache in [`pdf_view.rs`](../../crates/app/src/pdf_view.rs) are already platform-neutral.
- **Recommendation:** render with **`hayro` 0.8.0** (its `hayro-syntax` and `hayro-interpret` crates, plus `hayro-jpeg2000`, `hayro-jbig2` and `hayro-ccitt` for image filters). Build the text alternative on `hayro-interpret`'s `Device` trait. Run each page render and text extraction in a **GitTurtle helper process** (the app re-executing itself, as it already does for the askpass helper) under address-space and CPU limits. Cancellation or a deadline kills the helper.
  - **Why hayro:** it is the only option that is permissively licensed, pure Rust (`forbid(unsafe_code)` in the renderer and interpreter), builds the same way on AArch64 and x86-64, and is actively maintained.
  - **Why a helper process:** hayro has open issues where a few hostile bytes abort the process or overflow the stack, and it has no cancellation hook. In-process, any of these would take down the whole app.
- **What would change the answer:** hayro gaining a decoded-memory budget, cooperative cancellation and parser fuzzing, which would make in-process rendering defensible. Or a PDFium build with an established sandbox, if fidelity on real repository PDFs turns out to matter more than the helper's cost.
- **Outside this decision:** printing, forms, annotations editing, search, OCR, macOS, AVIF/HEIF/JPEG 2000 rasters ([separate note](2026-10-28-linux-avif-heif-jpeg2000.md); it recommends the same `hayro-jpeg2000` crate), and pdf.js, which needs a webview and is excluded.

## Evidence

| Source | Checked date/version | Relevant finding | Limit |
| --- | --- | --- | --- |
| GitTurtle [`crates/preview/src/lib.rs`](../../crates/preview/src/lib.rs), [`native.rs`](../../crates/preview/src/native.rs), [`native/pdf_text.rs`](../../crates/preview/src/native/pdf_text.rs) | 2026-10-28; `c219a6f` | `decode_pdf_page` and `decode_pdf_page_text` `bail!` on every non-macOS target. Shared bounds that a Linux backend must keep: `MAX_INPUT_BYTES` 32 MiB, `%PDF-` header, `MAX_PDF_PAGES` 100,000, `PDF_PREVIEW_EDGE` 1,000 px, `MAX_PDF_PAGE_TEXT_BYTES` 256 KiB, shared `check_dimensions`. macOS uses the crop box, the page rotation and a white fill (`CGPDFPageGetBoxRect(page, 1)`, `CGPDFPageGetDrawingTransform`). It refuses locked documents (`CGPDFDocumentIsEncrypted` and not `IsUnlocked`) and checks cancellation between phases. | Contract source, not a Linux measurement. |
| GitTurtle [`pdf_view.rs`](../../crates/app/src/pdf_view.rs), [`rich_preview.rs`](../../crates/app/src/rich_preview.rs) | 2026-10-28; `c219a6f` | Pages render on one `SerialExecutor` lane (`gitturtle-pdf-pages`) with `CACHE_PAGES` 4 and `CACHE_BYTES` 16 MiB. The initial preparation calls `decode_pdf` (`rich_preview.rs:133`). The only platform branch in the PDF view is the System preview control's accessible label (`pdf_view.rs:488`). | The Linux error and metadata paths in the consumer still have to be traced at implementation. |
| GitTurtle [`crates/git-core/src/work/authentication.rs`](../../crates/git-core/src/work/authentication.rs) `configure_askpass` | 2026-10-28; `c219a6f` | The shipped executable already doubles as a helper (`GIT_ASKPASS`/`SSH_ASKPASS` point at `current_exe()`). That is a precedent for a re-executed decode helper. | Askpass is not a decoder sandbox. |
| crates.io API: `hayro`, `hayro-syntax`, `hayro-interpret` 0.8.0; `hayro-jpeg2000` 0.4.1; `hayro-jbig2` 0.3.1; `hayro-ccitt` 0.4.0; `hayro-cmap` 0.1.0 | 2026-10-28; published 2026-10-04 (cmap 2026-04-12) | All `Apache-2.0 OR MIT`, MSRV 1.92 (GitTurtle pins 1.99). Archive SHA-256: `hayro` `4675ff5a…`, `hayro-interpret` `ac9df074…`, `hayro-syntax` `6cdc818d…`, `hayro-jpeg2000` `122c5f83…`. `hayro` depends on `vello_cpu` ^0.3, `kurbo` ^0.13, `pic-scale` ^0.7.9, `fearless_simd` ^1, and `image` ^0.25 (png only). `hayro-interpret` depends on `skrifa` ^0.46 and `moxcms` ^0.8.1. | Registry metadata; no build was made. |
| [hayro repository](https://github.com/LaurenzV/hayro), tag `hayro-v0.8.0` = `ea9c81dc`; `main` at `8558a8a5` (2026-10-26) | 2026-10-28 | README: "An experimental, work-in-progress PDF interpreter and renderer". It handles the 1,400+ PDFs in its suite (largely scraped from the PDFBox and pdf.js regression suites). It does not yet support knockout groups or non-embedded CID fonts, and "performance … has not been a focus". 202 commits on `main` since 2026-09-28; 39 contributors; 826 stars. CI runs only on `ubuntu-latest`. NOTICE: some code adapted from PDFBox and pdf.js (Apache-2.0) and from the `png` crate. | "Experimental" is the author's label. |
| `hayro*` 0.8.0 sources (published archives) | 2026-10-28 | `#![forbid(unsafe_code)]` in `hayro`, `hayro-interpret` and `hayro-jpeg2000`. `hayro-interpret` enables `hayro-syntax`'s `unsafe` feature (flate2 on `zlib-rs`, `memchr`, SIMD image decoding). API: `Pdf::new` → `pages()`, `page.render_dimensions()`, `hayro::render(page, &RenderCache, &InterpreterSettings, &RenderSettings, &PixmapSettings { x_scale, y_scale, bg_color })`. `InterpreterSettings` holds `font_resolver`, `cmap_resolver`, `warning_sink` and `render_annotations`. Nesting limits: `MAX_NESTED_INTERPRETATION_DEPTH` 50 and `MAX_OBJECT_NESTING_DEPTH` 64. Flate streams are inflated with `read_to_end` and no output cap (`hayro-syntax/src/filter/lzw_flate.rs:14–27`). No cancellation or memory-budget parameter exists anywhere in the API. | Read, not executed. |
| Same sources: encryption and text | 2026-10-28 | `Pdf::new` calls `new_with_password(data, "")` and returns `LoadPdfError::Decryption`, so documents with an empty user password should open and real passwords fail explicitly. The crate docs still list password-protected PDFs as unsupported, which suggests the docs lag the code. Text: `Device::draw_glyph_run` passes positioned glyphs, and `Glyph::as_unicode()` resolves ToUnicode, then the Adobe Glyph List, then `uniXXXX` names. Its own docs call it "still somewhat experimental". The `hayro-interpret/examples/extract_html.rs` example shows per-glyph extraction and leaves word and line merging to the caller. | Encryption and text quality need fixtures. |
| Same sources: bundled data | 2026-10-28 | The default `embed-fonts` feature embeds Foxit standard-font substitutes extracted from PDFium (BSD-3-Clause, `assets/LICENSE_FOXIT`), plus a CMYK profile (CC0-1.0) and `LAB.icc`. The default `embed-cmaps` feature embeds Adobe's predefined CMaps (BSD-3-Clause, `hayro-cmap/assets/LICENSE.txt`), decompressed with `brotli` 9.1.1 (`BSD-3-Clause AND MIT`). | These notices must be added to the license inventory. |
| hayro issues | 2026-10-28 state | **Open:** [#1549](https://github.com/LaurenzV/hayro/issues/1549) (JPEG 2000 has no decoded-memory budget; a 97-byte codestream reached about 95 GB RSS and aborts under a memory cap); [#1588](https://github.com/LaurenzV/hayro/issues/1588) (1-, 2- and 4-bit images are unpacked to their declared size, e.g. 20000 × 20000 from a 1-byte stream); [#1610](https://github.com/LaurenzV/hayro/issues/1610) (inline-image scanning recurses; time doubles per image and deep chains overflow the stack); [#1052](https://github.com/LaurenzV/hayro/issues/1052) (no cooperative cancellation); [#1649](https://github.com/LaurenzV/hayro/issues/1649) (JPEG 2000 panics); #1550 and #1598 (JBIG2 memory and time). **Closed:** #1347 and #1509 (stack overflows on nested arrays and object streams). | Issue reports; this note did not reproduce them. |
| hayro fuzzing | 2026-10-28; `hayro-fuzz` at `ea9c81dc` and `8558a8a5` | Fuzz targets exist only for CCITT, JBIG2 and JPEG 2000. There is no target for `Pdf::new`, interpretation or rendering. No `projects/hayro` in OSS-Fuzz (`google/oss-fuzz` at `fd3ab601`). Several open issues above came from third-party fuzzing. | — |
| Performance reports | 2026-10-28 | [#1613](https://github.com/LaurenzV/hayro/issues/1613) (third party, hayro 0.8.0 against MuPDF 1.28.5, warm renders at 2×, Apple M2 Pro): text-heavy first pages took 3.2–10.2 ms against MuPDF's 0.76–3.7 ms. Image-heavy pages were faster than MuPDF (scanned JPEG 2000: 5.0 ms against 10.0 ms). The cause is no glyph-raster cache. The maintainer ([#1358](https://github.com/LaurenzV/hayro/issues/1358)) says hayro is slower on complex shadings and glyph-heavy pages and that time is limited, but the project is not being abandoned. | Not GitTurtle's hardware, fixtures or edge. |
| Adoption | 2026-10-28 | crates.io reverse dependencies: `hayro` 56 (including `gpui-pdf`), `hayro-syntax` 12 (including `typst-library`), `hayro-jpeg2000` 29 (including GNOME's `glycin-image-rs`). | Popularity is not a security review. |
| crates.io `pdfium-render` 0.9.4; README at `master` `ea2961cb` | 2026-10-28; published 2026-09-06 | `MIT OR Apache-2.0`. Default features: `pdfium_latest`, `image_latest`, `thread_safe` (a global mutex, since PDFium is not thread-safe). Binds a `libpdfium` at run time through `libloading`, or statically with `static`. It "will not build Pdfium for you". Its 0.9.4 notes say it "fixes two double-free errors" in the wrapper's own font and page-object handling. | Wrapper only. |
| [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) release `chromium/8125` | 2026-10-26 release; checked 2026-10-28 | Ships `pdfium-linux-arm64.tgz` (SHA-256 `8b8818f2…`, `libpdfium.so` 8.1 MB) and `pdfium-linux-x64.tgz` (`e1c3ce6e…`), weekly. `args.gn`: `pdf_enable_v8 = false`, `pdf_enable_xfa = false`. Bundled licenses: PDFium (BSD-3-Clause), FreeType (FTL), ICU (Unicode-3.0), libjpeg-turbo (IJG/BSD), OpenJPEG (BSD-2-Clause), lcms (MIT), libpng, zlib, abseil and highway (Apache-2.0), AGG 2.3, HarfBuzz, simdutf, fast_float, dragonbox and llvm-libc. Build scripts are MIT, from one maintainer. | A third-party binary; GitTurtle would have to trust or reproduce it. |
| PDFium `main` `6876f741` (2026-10-28) | 2026-10-28 | BSD-3-Clause `LICENSE`. `testing/fuzzers` holds 35 `*_fuzzer.cc` targets, run in Chromium's own infrastructure. NVD keyword "pdfium": 137 CVEs, 25 published in 2026, e.g. CVE-2026-118345 (use after free, 2026-10-27) and CVE-2026-110155 (buffer overflow). Their descriptions say code execution is "inside the sandbox", i.e. Chrome's renderer sandbox. | Chrome-reported severities assume Chrome's sandbox. |
| Arch Linux: official and AUR search for `pdfium` | 2026-10-28 | No official package except `deepin-pdfium` (a Deepin fork). AUR has `libpdfium-nojs` and `pdfium-binaries`. | Not checked on Arch Linux ARM's own mirrors. |
| `lopdf` 0.46.0 (MIT), `pdf` 0.10.0 (MIT), `pdf-rs/pdf_render` | 2026-10-28 | `lopdf` parses and edits PDFs but has no renderer. OSV lists RUSTSEC-2026-0187, a stack overflow on deeply nested objects. `pdf` 0.10.0 (2026-03-02) is a parser. Its renderer `pdf_render` is not on crates.io and its repository was last pushed 2026-03-02. GitTurtle's `deny.toml` sets `unknown-git = "deny"`. | — |
| Other crates.io renderers: `pdf-render` 1.2.0 (pdfluent), `pdfboss-render` 2.16.1 | 2026-10-28 | Both are permissive, both are new in 2026 (`pdfboss-render` has 55 versions since 2026-07-25), and neither has the adoption record above. | Not evaluated beyond metadata. |
| [glycin README](https://gitlab.gnome.org/GNOME/glycin) at `main` `8dae8600` | 2026-10-28 | GNOME's image loaders run in `bwrap` with `setrlimit` memory limits and a seccomp allow-list, and receive bytes over a Unix socket. It is a Linux precedent for decoding untrusted bytes in a confined helper. | Image loaders, not a PDF renderer. |

## Compatibility and alternatives

Pinned stack: Rust 1.99.0, `image` 0.25.10, `kurbo` 0.11.3 and 0.13.1, `skrifa` 0.40.0 and 0.44.0, `moxcms` 0.8.1, `zune-jpeg` 0.5.15 (all in `Cargo.lock` at `c219a6f`). hayro reuses `kurbo` 0.13, `moxcms` 0.8, `zune-jpeg` 0.5 and `image` 0.25. It adds a third `skrifa`/`read-fonts` pair (^0.46) plus `vello_cpu`/`vello_common` 0.3, `fearless_simd` 1, `pic-scale` 0.7 and `brotli`. Every license is already on `deny.toml`'s allow list, and a Linux-only target dependency leaves macOS unchanged. The duplicated font crates and the embedded fonts and CMaps cost binary size, which the release measurement should record.

| Option | License tree | Maintenance | Fidelity limits | Text alternative | Bounding hooks | Security posture |
| --- | --- | --- | --- | --- | --- | --- |
| **hayro 0.8.0 (recommended)** | Apache-2.0 OR MIT. Embedded data: BSD-3-Clause (Foxit fonts, Adobe CMaps), CC0-1.0 (CMYK profile). Dependencies are MIT, Apache-2.0, BSD-3-Clause and Zlib. | Very active (202 commits in 30 days), one lead author, 39 contributors, frequent minor releases with API changes. | Author-documented: knockout groups, non-embedded CID fonts, CID fonts with non-identity encodings, partial annotations. Crate docs (possibly stale) also list blending/isolation and color-key masking. CMYK and ICC go through `moxcms`. JBIG2, CCITT, JPX and DCT are decoded in Rust. | Custom `Device` over glyph runs with `as_unicode()` (experimental). GitTurtle must merge glyphs into words and lines itself. | Rendering scale is chosen by the caller, so the 1,000-px edge is enforced before rasterizing. Nesting is capped at 50/64. No byte budget, no cancellation, and uncapped flate output. | Memory-safe Rust, but known aborts (allocation failure, stack overflow) and slow paths from tiny inputs. Only the image codecs are fuzzed. |
| `pdfium-render` 0.9.4 + PDFium `chromium/8125` | Wrapper MIT OR Apache-2.0. PDFium BSD-3-Clause plus the bundled list above (FTL for FreeType, which is dual FTL/GPLv2; GitTurtle would ship it under FTL). | PDFium is Chromium's, updated continuously. Binaries come weekly from one third-party builder. | Best fidelity of the options (Chrome's viewer). V8 and XFA are off in the chosen build. | Mature `FPDFText` API (`PdfPageText`). | Render target size is set by the caller. No memory budget or cancellation inside a call. | C++ with frequent memory-safety CVEs (25 in 2026). Only acceptable in a confined helper with seccomp/landlock, since Chrome's severities assume its sandbox. Adds a native `.so` to ship and update on both architectures; not in Arch's repositories. |
| `lopdf` or `pdf` + a rasteriser | MIT | Active parsers, but no maintained Rust renderer on crates.io. | GitTurtle would be writing a PDF renderer. | `pdf-extract` (MIT, on `lopdf`) exists. | — | `lopdf` has RUSTSEC-2026-0187. Not viable for parity. |
| MuPDF, Poppler | AGPL-3.0, GPL | — | — | — | — | Excluded by the no-GPL rule. |
| pdf.js | Apache-2.0 | — | — | — | — | Excluded: needs a webview. |

**Why a helper process.** Each of hayro's open failure modes ends in a bounded error inside a helper with `RLIMIT_AS`, `RLIMIT_CPU` and a wall-clock kill:

- an inflated flate stream (up to roughly 1,000 times the 32 MiB input);
- #1549 (JPEG 2000 allocating about 95 GB);
- #1588 (bit-packed images unpacked to their declared size);
- #1610 (stack overflow);
- #1052 (no cancellation).

In-process, the same inputs abort GitTurtle or pressure the whole desktop. `catch_unwind` only covers panics such as #1649.

The helper also gives Linux a property macOS lacks: the [contract](../document-previews.md#pdf-pages-and-text) says native calls "cannot be interrupted mid-call" and form no "hard wall-clock deadline or process-memory sandbox", whereas killing the helper is both.

The cost is process start-up plus re-parsing per request. Either one helper per request, or a helper kept alive per open document (with hayro's `RenderCache`) and killed on cancellation, generation change or idle. The measurement below should decide between the two. Seccomp or Landlock confinement on top of the rlimits is a [security review](security-review.md) decision; the helper needs no filesystem or network access after it starts.

**In-process hayro** is the fallback if the [architecture review](architecture-review.md) rejects a helper. It must then document the abort risks above, and it should wait for upstream fixes to #1549, #1588 and #1610.

**Could change the recommendation:**

- upstream adds a decoded-memory budget and cancellation (#1052) and fuzzes the parser;
- fixture fidelity on real repository PDFs (CJK, forms, transparency) is poor enough to justify PDFium's confinement cost;
- the owner prefers vendor fidelity over a Rust-only decoder.

## Enforcement and acceptance

- **Owners:** `crates/preview` gets a Linux backend behind the existing `decode_pdf_page` and `decode_pdf_page_text`, keeping every bound in `lib.rs`. `crates/app` gets the helper launch, protocol, limits and kill on the existing render lane, and any Linux wording that still describes PDFs as unavailable. Update [`docs/document-previews.md`](../document-previews.md), [`docs/file-previews.md`](../file-previews.md), [`crates/preview/AGENTS.md`](../../crates/preview/AGENTS.md), `THIRD_PARTY_NOTICES.md` and `docs/licenses/dependencies.json` (Foxit, Adobe CMap and CC0 notices). The architecture review applies (a new process boundary and scheduling), and the [security review](security-review.md) must pass before integration (a decoder of untrusted bytes).
- **Behavior to keep:**
  - crop box, rotation and white background;
  - 1,000-px output edge, computed from `render_dimensions()` before rasterizing;
  - 1–100,000 pages and an explicit locked/corrupt result;
  - 256 KiB text cap with explicit truncation at a character boundary, and explicit absence;
  - no annotations' actions, scripts or links;
  - cancellation checked between parse, render, text and conversion;
  - no native or document handle crossing to the UI.
  
  Text from a custom `Device` must say, as on macOS, that reading order can differ from layout.
- **Tests:**
  - preview fixtures for orientation, crop box, bounds, empty-password encryption, a real password (refused), text present, absent and truncated, and CMYK/JPX/JBIG2 pages;
  - hostile fixtures built from the #1549, #1588 and #1610 reproductions, each ending in an explicit error within the helper's limits while the app stays responsive;
  - a helper test that cancellation kills the child and that a late result is dropped by the generation check.
- **Release measurement** (release build on the AArch64 Asahi host and an x86-64 Linux host; record fixture, hardware, cold/warm state and tail latency):
  - per page at the 1,000-px edge: helper start, parse, render, RGBA return and text extraction;
  - cold first page and warm next page, p50/p95/max over at least 20 runs;
  - a fixture set with text-heavy, CJK/CID, scanned JPX, JBIG2, CMYK, transparency and a 300-page document (first, middle and last page);
  - helper peak RSS (`getrusage`) and time from cancellation to the child's exit;
  - executable size delta;
  - the same measurement for a retained per-document helper, to choose between the two helper modes.
- **Native evidence:** Previous/Next, direct entry, linked and independent sides with different page counts, Fit and zoom, "Read page text", and an absent side, on Omarchy (Wayland), following the [validation matrix](../validation.md#current-validation-guidance).
- **Remaining uncertainty:** real-world fidelity of hayro on the owner's PDFs; whether its text output reads acceptably without layout analysis; helper cost per page.

## Outcome

Not implemented. Pending the owner's go-ahead and the architecture review of the helper process.
