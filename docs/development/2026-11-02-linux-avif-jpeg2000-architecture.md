# Linux AVIF and JPEG 2000 images: architecture review (2026-11-02)

The [architecture review](architecture-review.md) of the second Linux parity item in [full Omarchy support](omarchy-full-support.md) (item 9): decoding AVIF and JPEG 2000 images on Linux, which macOS decodes through ImageIO, with the crates the owner chose on 2026-10-28 after the [research note](2026-10-28-linux-avif-heif-jpeg2000.md): `hayro-jpeg2000` for JPEG 2000; `mp4parse` (unmodified) and `dav1d` over the system libdav1d for AVIF, with size, thread and delay limits before decoding. HEIF/HEIC stays out by owner decision. Base revision: `1284f791`. Verdict: **both formats decode only inside GitTurtle's confined helper process**, the one built for [PDF pages](2026-11-02-linux-pdf-pages-architecture.md), extended with an image request: one helper per image, single-threaded, never in the app process. macOS keeps ImageIO. The independent [security review](security-review.md) decides the confinement questions in section 3 before implementation.

## 1. Dependency refresh (checked 2026-11-02)

Sources: the crates.io API and the published archives from `static.crates.io` (each SHA-256 equals the API's checksum; read as data, never built), the GitHub API for the repositories, the VideoLAN GitLab API for dav1d tags and sources, the archlinux.org package API, packages.ubuntu.com and the Ubuntu CVE tracker, the NVD API, `google/oss-fuzz`, and on the AArch64 Asahi development host `pacman -Qi`/`-Ql`, `readelf` and `cargo tree --locked --offline`.

| Item | 2026-11-02 |
| --- | --- |
| `hayro-jpeg2000` 0.4.1 (2026-10-04, `Apache-2.0 OR MIT`, MSRV 1.92, archive `122c5f83…`) | Still the latest. Already in GitTurtle's Linux build through `hayro` 0.8.0 (`hayro-syntax`), built with `std` and `simd`: `hayro-syntax`'s `unsafe` feature turns `simd` on, so the SIMD path through `fearless_simd` already runs in the PDF helper. The crate itself is `#![forbid(unsafe_code)]` (`lib.rs:68`). Pure Rust on AArch64 and x86-64, no threads (`rayon` is a dev-dependency only). The `image` feature adds `impl image::ImageDecoder for Image` with ICC and CMYK conversion through `moxcms` (`integration.rs:12-16`); `image` and `moxcms` 0.8.1 are already locked. `DecodeSettings` has `resolve_palette_indices`, `strict` and `target_resolution` (`lib.rs:110-140`). No OSS-Fuzz project; the repository has fuzz targets. Issues: [#1549](https://github.com/LaurenzV/hayro/issues/1549) (no decoded-memory budget) and its fix [#1558](https://github.com/LaurenzV/hayro/pull/1558) are open. A 2026-10-22 comment on #1558 shows the budget misses the tile vector: #1549's codestream changed to 16383 × 16383 with 1 × 1 tiles stays under 2^28 samples and still asks for 25.8 GB at `j2c/tile.rs:146` (OpenJPEG refuses more than 65,535 tiles); three fuzzed PDFs peak at 7.1–63 GiB even when decoded at 256 px. [#1649](https://github.com/LaurenzV/hayro/issues/1649) (palette panics) is open with fix [#1679](https://github.com/LaurenzV/hayro/pull/1679) unmerged (updated 2026-10-28). New: [#1680](https://github.com/LaurenzV/hayro/issues/1680) (2026-10-29: the first tile's multiple-component-transform setting applies to every tile, so wrong colours, not memory). Merged on `main` after 0.4.1 and unreleased: [#1409](https://github.com/LaurenzV/hayro/pull/1409) (three panics when decoding with `target_resolution`: subsampled components, empty resolution levels, tile-parts that reduce the level count), [#1405](https://github.com/LaurenzV/hayro/pull/1405) (reduced or subsampled images one column too wide), #1411 (truncated codestreams), #1403 (YCbCr for raw codestreams with subsampled chroma), #1516 (report ICC profiles instead of converting), and SIMD speedups #1624, #1627, #1628. `main` is `04a801de` (2026-11-01). |
| `mp4parse` 0.17.0 (2023-05-29, MPL-2.0, archive `63a35203…`) | Still the latest on crates.io. No `unsafe` in `src/` (the C API is the separate `mp4parse_capi`). `read_avif` returns an `AvifContext` with the primary and alpha coded data, `premultiplied_alpha`, `ispe`, `nclx` and ICC `colr`, `irot`, `imir`, `pasp`, the `avis` track as `sequence`, and `unsupported_features` (`A1lx`, `A1op`, `Clap`, `Grid`, `Ipro`, `Lsel`; `lib.rs:278-311`, `:1567-1737`, `:2462`); allocations are fallible (`fallible_collections` 0.4.9); a few `assert!` invariants in property association (`lib.rs:3256-3281`) can only fire if the parser's own bookkeeping is wrong. Repository `master` is `d3a9abfc` (2026-10-27) and carries three years of unreleased fixes, among them `f2d96807` (2026-02-14, an integer overflow in composition times), `73423e14` (`nclx` boxes without the range byte) and the `a1lx`/`lsel` handling of 2026-07 to 2026-09; there is no tag after `v0.12.0`. OSS-Fuzz has `mp4parse-rust`. Pure Rust on both architectures. |
| `dav1d` 0.11.1 (2025-11-25, MIT, archive `3ee89cb8…`) | Unchanged. `Settings` has setters for `n_threads`, `max_frame_delay`, `frame_size_limit`, `apply_grain`, `operating_point`, `all_layers`, `strict_std_compliance`, `output_invisible_frames`, `inloop_filters` and `decode_frame_type`, but none for the logger; `Decoder::with_settings_and_allocator` takes a picture allocator. 49 `unsafe` occurrences in `src/lib.rs` (the FFI wrapper). `master` is still `03477a36` (2025-11-25). Open: [#126](https://github.com/rust-av/dav1d-rs/issues/126) (2026-10-21): `dav1d-sys` swaps the types of `initial_display_delay` and `idc`, so `Dav1dSequenceHeader` is 744 bytes in Rust and 808 in C (dav1d 1.4.1 and 1.5.4); `dav1d_parse_sequence_header` overwrites the stack and every field after `operating_points` is read at the wrong offset. dav1d-rs's `Picture` accessors read only `hbd`, `pri`, `trc`, `mtrx`, `color_range` and `chr` (`lib.rs:95-223`), which precede `operating_points` (`headers.h:216-233`), so they read correctly. [#123](https://github.com/rust-av/dav1d-rs/pull/123) (open PR): U and V plane slices of a monochrome picture may point at unset memory. #70 and #74 affect only multithreaded decoding; #125 proposes `system-deps` 9. |
| `dav1d-sys` 0.8.3 (2025-05-28, MIT, archive `c3c91aea…`) | Unchanged. Links the system library through `system-deps` ^7 (7.0.8; `system-deps` 9.0.1 shipped 2026-10-22) and pkg-config `dav1d >= 1.3.0`. Only when `SYSTEM_DEPS_DAV1D_BUILD_INTERNAL` is set does its `build.rs` `git clone` dav1d **1.5.0** and build it static with meson. |
| System libdav1d | dav1d 1.5.4 (2026-07-14) is still the latest tag; `master` moved from `c6f51c88` to `4e3ab21f` (2026-10-29). BSD-2-Clause. Arch x86-64 `dav1d` 1.5.4-1 (2026-07-15) and Arch Linux ARM 1.5.4-1 (local; required by `chromium`, `ffmpeg` and `librsvg`): shared library, headers and `dav1d.pc`, no static archive; `libdav1d.so.7` needs only libc, and has `DT_INIT` plus two `.init_array` entries while dav1d's `lib.c`, `cpu.c`, `mem.c` and `log.c` declare no constructor. Ubuntu 24.04: `libdav1d-dev` and runtime `libdav1d7` 1.4.1-1build1 in universe, no newer update; the Ubuntu tracker marks noble "Not affected" by CVE-2024-1580. NVD still lists two CVEs: CVE-2023-32570 (a thread race before 1.2.0) and CVE-2024-1580 (integer overflow, "upgrade past 1.4.0"). OSS-Fuzz has `dav1d`. Hand-written assembly for AArch64 (NEON, SVE) and x86-64 (SSE to AVX-512). At 1.5.4, `n_threads` 0 means the logical processors (`src/lib.c:122-125`), `frame_size_limit` 0 means unlimited (`dav1d.h:84`), and with `n_threads` 1 and `max_frame_delay` 1 no thread is created (`lib.c:227`, `:276-282`). `dav1d_open` still calls `pthread_once` for CPU detection, by `getauxval(AT_HWCAP)` on AArch64 Linux (`src/arm/cpu.c:46-47`) and `cpuid` on x86-64, and `dlsym(RTLD_DEFAULT, "__pthread_get_minstack")` for a thread stack size it never uses (`lib.c:92-104`, `:161`). The default logger is `vfprintf(stderr)` (`log.c:40-43`); memory comes from `malloc` (`mem.c:104-105`). Flatpak runtimes: not relevant (GitTurtle ships no Flatpak) and not checked. |

**Alternatives (one verdict each).**

| Option | 2026-11-02 | Verdict |
| --- | --- | --- |
| `image` 0.25.10 `avif-native` | Unchanged (latest, 2026-03-10). Decodes in the constructor with dav1d's defaults (every core, unlimited frame size) before limits apply; no orientation. | Rejected: unbounded before the limits, no `irot`/`imir`. |
| `rav1d` 1.1.0 (2025-05-07, BSD-2-Clause) | `main` `2e7216c2` (2026-10-27), no release since; a c2rust-derived port with wide `unsafe`; no OSS-Fuzz project. | Rejected for now; revisit only if a bundle must not depend on a system codec. |
| `re_rav1d` 0.1.3 (2024-10-15) | Rerun's fork of `rav1d`; no release in two years. | Rejected: stale. |
| `avif-decode` 3.0.0 (2026-09-20, BSD-3-Clause) | `rav1d` + `avif-parse` + `yuv`; threads up to 32, no frame-size limit. | Rejected: no size limit, threads, inherits `rav1d`. |
| `avif-parse` 2.1.0 (2026-03-27, MPL-2.0) | Maintained; parses the AV1 sequence header; refuses grids (`lib.rs:930`); no `irot`, `imir` or `clap`. | Rejected as the container parser (no orientation, so no parity); the fallback if `mp4parse`'s staleness becomes a blocker. |
| `zune-*` | No AVIF or JPEG 2000 decoder in the family: `zune-heic` 0.5.0 is HEVC only, `zune-isobmff` 0.5.0 (first release 2026-10-11) parses boxes. | Not applicable. |
| `jpeg2k` 0.10.1 + `openjpeg-sys` 1.0.12 | Unchanged; the vendored OpenJPEG is 2.5.3 and lacks the CVE-2025-54874 fix. | Rejected: C, vendored copy behind a CVE fix. |
| `openjp2` 0.6.1 | Unchanged; OpenJPEG translated to Rust, 482 `unsafe`. | Rejected. |
| System `libopenjp2` 2.5.4 (Arch x86-64, 2025-09-26) | 104 historical CVEs; would need a custom `-sys` crate. | Rejected: `hayro-jpeg2000` is memory-safe and already in the helper. |
| `pdfluent-jpeg2000` 0.4.0, `ps-hayro-jpeg2000` 0.5.0, `dicom-toolkit-jpeg2000` 0.5.0 | Forks of `hayro-jpeg2000`, all first published in 2026. | Rejected: follow upstream. |

**Changed since 2026-10-28.** dav1d `master` moved (no release). `hayro-jpeg2000`: #1680 opened; the #1558 tile-vector gap and the unreleased `main` fixes (#1405, #1409 and others) are recorded here for the first time. `dav1d-rs` #126 opened 2026-10-21 and was not in the earlier note. `system-deps` 9.0.1 shipped (dav1d-sys stays on 7). Ubuntu 24.04's libdav1d is 1.4.1 in universe and not affected by CVE-2024-1580. Nothing else changed: same crate versions and checksums, no new CVE or RustSec advisory.

**cargo-deny (0.20.2, query only).** Two throwaway crates in `.local/` with a copy of [`deny.toml`](../../deny.toml): the chosen set (`mp4parse =0.17.0`, `dav1d =0.11.1`, `hayro-jpeg2000 =0.4.1` with `std` and `image`) and the alternatives (`image` with `avif-native`, `avif-decode`, `avif-parse`, `re_rav1d`, `jpeg2k`, `openjpeg-sys`, `openjp2`) both report "advisories ok, bans ok, licenses ok, sources ok"; `deny.toml` needs no change. cargo-deny sees crate licenses only; libdav1d (BSD-2-Clause) is recorded by hand. Against today's lock the chosen set adds about a dozen packages (`mp4parse`, `bitreader`, `fallible_collections` 0.4.9 with `hashbrown` 0.13, `dav1d`, `dav1d-sys`, `av-data`, `byte-slice-cast`, `system-deps` 7 with `cfg-expr`, `target-lexicon`, `version-compare`); slice A1 records the exact count.

## 2. Review

**Today's path.** `decode_image` (`crates/preview/src/lib.rs:244-264`) sends AVIF (`ftyp` brands, `metadata.rs:11-39`) and JP2/J2K (signatures, `metadata.rs:56-64`) to ImageIO on macOS (`native.rs:213-299`: bounded JP2 dimensions, a no-cache thumbnail at the requested edge with orientation applied, labelled "first image · macOS codec") and to `bail!("{format} rendering is not available on this platform")` elsewhere (`lib.rs:263`). `jpeg2000.rs` builds only for macOS and tests (`lib.rs:18`). Bounds (`lib.rs:117-121`, `:280-303`): 32 MiB input, 32,768-pixel edge, 32,000,000 source pixels, 256 MiB decoded, 4,096 output edge. The app decodes on each window's single reader thread (`worker.rs:634-686`: one active read, one replaceable pending request, a panic caught at `:670`): `working_image_side` (`:2188`) tries GIF playback, then `decode_image(&bytes, name, PREVIEW_EDGE)` at 1,600 px (`:25`, `:2245`), then `render_image`. Markdown's local images decode at 1,000 px under a 16 MiB pixel budget (`markdown_view.rs:433`). On Linux an AVIF or JPEG 2000 side shows that sentence beside Open in default app (`system_preview/passive.rs:15`, `:79-88`).

**Decode location: the confined helper, for both formats.** Measured or justified per format:
- *AVIF.* libdav1d is C and assembly parsing hostile bytes, with two CVEs and, in its Rust binding, a known struct-layout mismatch (#126). In the app process a memory-safety bug reaches every window's memory (repository content and any credential the app holds) and a crash ends the app. The helper already has the confinement, limits, kill and reap.
- *JPEG 2000.* `hayro-jpeg2000` is memory-safe, but it has no memory budget (#1549, the 25.8 GB tile vector, 7–63 GiB peaks at 256 px), and Rust aborts on allocation failure, which in the app process ends GitTurtle. `decode` cannot be cancelled, so in the app process it would hold the window's only reader (History, search and every preview) until it finished. GitTurtle's preflight below closes the known cases, not the class. In the helper, `RLIMIT_DATA` turns an allocation blow-up into a helper abort, the client's kill bounds the time, and cancelling becomes a kill within 25 ms. `hayro-jpeg2000` already runs there for PDF JPX streams, so JPEG 2000 brings no new crate into the helper.
- *Cost, measured.* The release executable `gitturtle-8b916c5` (helper and client present; `strings` finds its commit once) was started as a helper by a Python driver (empty environment, `/` as working directory, its own pid as the parent). An exit at the version check (exec, loading and pre-main work): p50 1.7 ms, p95 2.3, max 3.2. A full confined round trip for a 64 × 64 one-page PDF: p50 2.0 ms, p95 2.8, max 3.9 (n = 40, load 5–6, warm page cache). A 33 MB request (the largest input): p50 19.6 ms, p95 23.3 (n = 20, load 13), helper peak RSS 147,200 KiB. Against expected decode times of a few hundred milliseconds for a 12 MP image, a single-image helper costs under 5% at typical sizes and about 20 ms at the largest. Section 5 repeats this on an idle host.

Decision: on Linux nothing decodes AVIF or JPEG 2000 in the app process. `decode_image` keeps refusing them there, as `decode_pdf_page` does, so a caller that bypasses the helper gets a refusal, not an unconfined decode.

**One helper, extended.** Same entry (`--gitturtle-pdf-helper <version> <parent pid>`, `main.rs:2451`), setup order, allow-list, resource limits, 10 s deadline and the two slots (`pdf_helper.rs:448-453`), which images and PDF pages share. The helper reads a 4-byte magic and dispatches before allocating: `GTPq` (PDF page, unchanged) or a new `GTIq` image frame. `PROTOCOL_VERSION` becomes 2 because the helper accepts a new frame. The flag and the process name `gitturtle-pdf` stay: both are internal, the argv grammar is reviewed and tested exactly, and a rename is a separate refactor if it ever confuses.
- *Request:* magic, version (u32), format (u8: 1 AVIF, 2 JP2, 3 J2K), output edge (u32, 1..=1,600), length (u32, ≤ 32 MiB), then the bytes. The helper reads the validated length, sniffs the bytes itself and refuses a format that disagrees with the request.
- *Reply:* magic, version, status, then an error kind (u8 + u32 detail) or: format echo, flags (u16), original and output size (u32 each), parse, decode and convert times in µs (metrics only), then exactly width × height × 4 straight RGBA bytes. The client requires the echo to match, only known flags, an original size that passes `check_dimensions`, an output size equal to `scaled_dimensions(original, edge)`, a length computed with checked arithmetic, no trailing bytes and exit status 0, and allocates only after those checks. The helper's image edge is capped at the app's 1,600 (Markdown asks 1,000), so the reply cap is 10,240,000 bytes plus the header.

**Owners and interfaces.** Linux x86-64 and AArch64 only, like `pdf_hayro`:
- Preview: `src/jpeg2000.rs` builds for the helper targets too and gains the checks below; `src/jpeg2000_hayro.rs` (new) decodes through `hayro-jpeg2000`'s `Image` as an `image::ImageDecoder`; `src/avif.rs` (new) parses with `mp4parse` and decodes with `dav1d`; `src/image_helper.rs` (new) holds the image frame and its serving; `pdf_helper::serve` dispatches on the magic and stays the one public way into hayro, `hayro-jpeg2000`, `mp4parse` and `dav1d`. `lib.rs` gains `confined_image_format(bytes)` (AVIF, JP2 and J2K, never HEIF), the single routing decision.
- App: `src/pdf_helper.rs` turns the reply cap (`MAX_REPLY_BYTES`, `:836`) into a parameter and exposes one crate-private request runner, PDF wording unchanged. `src/image_helper.rs` (new) has `decode(bytes, name, edge, check)`: the helper for `confined_image_format`, `gitturtle_preview::decode_image` for everything else and on other targets, with its own `Failure` wording. The call sites are `worker.rs:2245` and `markdown_view.rs:433`; no other app code calls `decode_image`.
- Git core: unchanged; sides are captured as today.

**Bounds before allocation: AVIF** (in the helper, in this order):
1. Input ≤ 32 MiB and `iso_image_format` is AVIF; then `mp4parse::read_avif` with `ParseStrictness::Normal` (any error: unreadable file).
2. `unsupported_features`: `Grid` is refused explicitly, since `mp4parse` cannot expose the tiles; `Ipro` (protected items) is refused; with `A1op`, `Lsel` or `A1lx`, operating point 0 is decoded; with `Clap` the image is decoded without the crop and the caption says so.
3. A primary item is required (MIAF; AVIF 1.2 §6.3 notes that a sequence file still has an image item); its `ispe` must pass `check_dimensions`.
4. An alpha item (`auxl`) must have the primary's `ispe`.
5. dav1d `Settings`: `n_threads` 1, `max_frame_delay` 1, `frame_size_limit` = `ispe` width × height (≤ 32,000,000), `all_layers` false, `operating_point` 0, `apply_grain` true, `output_invisible_frames` false; one decoder per item, data sent once, a bounded `get_picture` loop.
6. After decoding: picture size equal to `ispe`, layout I400/I420/I422/I444 and 8, 10 or 12 bits, else refused. dav1d's own buffers (planes at 2 bytes per sample above 8 bits, a film-grain copy, an alpha picture) are bounded by `RLIMIT_DATA`.
7. One frame only, whatever the file holds (below); output through `scaled_dimensions` and the existing premultiplied resize.

**Bounds before allocation: JPEG 2000:**
1. Signature; for JP2, brand `jp2 ` or a compatibility list containing `jp2 ` (JPX that is not JP2-compatible is refused on Linux); exactly one `jp2h`, then one `jp2c`, and neither kind after it (`hayro-jpeg2000` 0.4.1 reads every top-level box and decodes the last of each; bytes that frame no box stop its walk, so they are refused before the `jp2c` and end the preflight's walk after it); `ihdr` and SIZ agree and both pass `check_dimensions` (existing `jpeg2000::dimensions`).
2. SIZ: 1–4 components, precision ≤ 16 bits, tiles `ceil(w / tw) × ceil(h / th)` ≤ 65,535 (SOT's tile index is 16 bits; covers the 25.8 GB tile vector).
3. `pclr`: 1–1,024 entries, 1–4 columns, depth ≤ 16 bits, with a `cmap` of 1–4 entries, each naming a SIZ component, mapping type 0 or 1 and an existing column (covers #1649; `resolve_palette_indices` makes one full-size plane per entry). A palette turns reduced resolution off, so its budget is at full size.
4. Resolution: target `scaled_dimensions(size, edge)`; reduced resolution only when every component samples 1 × 1 and there is no palette (0.4.1 panics on subsampled components at reduced resolution, #1409, fixed only on `main`), otherwise full resolution.
5. Budget before `decode`, from the size `Image::new` reports: width × height × (components + `cmap` entries with a palette, or twice the output planes with a `cdef` when that is more, since 0.4.1 clones them to reorder; with a first `colr` that converts through a profile, `profile_planes`: the output planes and their 8-bit copy stay alive while a refused profile decodes the image again) × 4 bytes (f32 samples) plus the largest tile-component at that resolution × 4 bytes ≤ 256 MiB (#1558's formula; step 2 covers its gap). The size `Image::new` reports must equal GitTurtle's own size for the full image or a level it may skip, checked before decoding; `total_bytes()` and the output buffer derive from that size, so no later check of either can disagree.
6. `decode` under `catch_unwind`; code-block, precinct and packet structures are not budgeted by anyone: `RLIMIT_DATA` and the deadline cover them.

**Colour, depth, alpha and orientation.**
- AVIF: the YUV matrix comes from an `nclx` `colr` if present, else from the sequence header through dav1d-rs's accessors (fields before `operating_points` only); unspecified means BT.601; full or limited range; identity (GBR) for 4:4:4; 4:0:0 is grey and never touches U or V (#123); chroma by nearest sample (the output is downscaled anyway); 10 and 12 bits to 8 by `(v × 255 + max / 2) / max`; the alpha item's range honoured; `prem` alpha unpremultiplied to straight RGBA. An ICC profile, or `nclx` primaries other than BT.709/sRGB (Display P3, BT.2020), is converted to sRGB with `moxcms` (already locked, `BSD-3-Clause OR Apache-2.0`), as ImageIO matches to its RGB space. PQ and HLG are shown without tone mapping and the caption says so. A gain-map file shows its base image; `tmap` is ignored. Orientation follows MIAF: `clap` (not applied, disclosed), then `irot` (anticlockwise quarter turns), then `imir`; the original size is reported after the transform, as the decoder contract requires.
- JPEG 2000: the `image` integration converts ICC, CMYK, sYCC and grey to RGB(A)8 with `moxcms`; alpha from `cdef`; 16-bit samples to 8 by the `image` conversion; JP2 has no orientation.

**Threads.** The helper stays single-threaded and the filter keeps killing `clone`/`clone3`: dav1d with `n_threads` 1 and `max_frame_delay` 1 creates no thread, and `hayro-jpeg2000`, the conversion and the resize run on the calling thread (`image`'s `rayon` feature only adds parallel iterators, which the resize does not use). The cost is dav1d's tile and frame threading on large stills; section 5 measures it. If a 12 MP AVIF misses its budget, the next step is a security-reviewed allowance for dav1d threads, never an in-process decode.

**Animation: the first frame only.** An AVIF sequence shows its primary item, captioned as the first frame of an animation when `AvifContext::sequence` is present; a sequence without a primary item is refused. Reasons: parity (ImageIO's thumbnail at index 0 is the first image, and neither platform plays AVIF today; GIF playback is GIF-only); cost (playback would decode up to 120 inter-predicted frames single-threaded in the helper, seconds per file, and return up to 16 M output pixels, about 64 MB, six times the image cap); and a review tool compares stills. Playback would be a later item for both platforms with its own budget.

**Cancellation and stale results.** The client runs the caller's check at least every 25 ms: the reader's generation check (a newer request) or Markdown's check kills and reaps the helper, and `GitTurtle`'s request generations and page guards still drop late results, so a late image never reopens Compare. Image decoding thus becomes cancellable, which ImageIO in the app process is not. Both sides still decode one after the other on the reader; two helpers at once would need a second thread, decided by measurement.

**Retained bytes and cache.** Unchanged: the helper returns an owned `ImagePreview`; `Content::bytes` counts its RGBA and the captured bytes as for any image; warm tabs, retained inspections and bookmarks gain nothing. The helper's memory is transient: at most two helpers (1 GiB data limit each), shared with PDF pages.

**Platforms.** Dependencies under the helper's target `cfg` (Linux x86-64 and AArch64); macOS keeps ImageIO, proved by an empty `cargo tree --target aarch64-apple-darwin -i dav1d` (and `-i mp4parse`) and later on a Mac; other targets keep today's refusal. libdav1d is linked dynamically through pkg-config into the app executable (`NEEDED libdav1d.so.7`) and called only in the helper. It becomes a runtime package (Arch `dav1d`, Ubuntu `libdav1d7`) and a build package (`libdav1d-dev`). Packaging never sets `SYSTEM_DEPS_DAV1D_BUILD_INTERNAL`. The helper refuses a libdav1d older than 1.4.1 (Ubuntu 24.04's version, past CVE-2024-1580), read from `dav1d_version()`, as GitTurtle refuses Git LFS before 3.8.1.

**Wording on Linux.** Error kinds map to fixed sentences; the helper sends none. Document errors reuse the existing bound sentences ("Image exceeds the 32 MiB input limit", the edge and pixel limits, "Decoded image exceeds the 256 MiB limit") and add: "Cannot read AVIF; corrupt or unsupported file"; "AVIF image grids are not decoded on Linux yet"; "This AVIF animation has no still image"; "The AVIF image size disagrees with its header"; "AVIF decoding needs libdav1d 1.4.1 or later"; "Cannot read JPEG 2000; corrupt or unsupported codestream"; "This JPEG 2000 image declares more than 65,535 tiles"; "JPEG 2000 images with more than four components are not decoded on Linux"; "JPEG 2000 samples deeper than 16 bits are not decoded on Linux"; "This JPX file is not JP2-compatible and is not decoded on Linux". Helper failures: "The image took longer than 10 seconds to decode and was stopped."; "The image decoder stopped on this image."; "The image decoder returned an unreadable result."; "The image decoder could not start.". HEIF: "HEIF / HEIC images are not decoded on Linux." with Open in default app still offered. Captions as macOS's without "macOS codec" ("AVIF · first image", "JPEG 2000 / JP2 · first image"), plus a flag where it applies (first frame of an animation, crop not applied, HDR not tone-mapped); final words in A4's design review.

**Failure and recovery.** Each failure is that side's error; the other side and the rest of the view are unaffected. No automatic retry (a hostile file would burn the CPU again). Open in default app stays offered for AVIF and JPEG 2000 sides (`system_preview::offer` unchanged). A decoder panic becomes "stopped" through the helper's `catch_unwind`; the reader's own panic guard stays.

**Tests and fixtures.** `assert_jpeg2000_preview` (`lib.rs`, Linux branch) keeps expecting `decode_image`'s in-process refusal; new preview tests drive `serve` in-process as PDF-T1's did; app tests drive the real binary under the filter as `tests/pdf_helper.rs` does; the macOS ImageIO tests (`native.rs`) and the passive-type tests (`passive.rs`) stay unchanged. The fixture README gains each new fixture's generator, tool version and SHA-256 (section 5).

## 3. Confinement questions for the security review

1. **One process for pages and images.** Is extending the existing helper (same argv grammar, setup order, filter, limits, 10 s deadline, two shared slots) with a second frame chosen by its magic acceptable, or should images get their own mode and limits? Recommended: the same process.
2. **The allow-list and libdav1d.** At 1.5.4, `dav1d_open` calls `pthread_once` (futex), detects CPU features by `getauxval` (AArch64) or `cpuid` (x86-64), calls `pthread_attr_init` and `dlsym(RTLD_DEFAULT, "__pthread_get_minstack")` even with one thread, and allocates with `malloc`; its default logger writes to fd 2 (`/dev/null`). Does anything here need a call the filter refuses? Proposed evidence: SIGSYS-free fixture runs on both architectures and an `strace -f` list from a host that has `strace`. Should the logger instead be removed through raw `dav1d-sys` calls (more `unsafe` in GitTurtle), or is `write(2)` enough?
3. **Threads.** `clone`/`clone3` stay killed with `n_threads` 1 and `max_frame_delay` 1, and `RLIMIT_NPROC` 0 also refuses threads. If the measurement later asks for dav1d's threads, what must that request prove (`CLONE_THREAD` only, a thread cap, `RLIMIT_NPROC` raised, `RLIMIT_CPU` counting every thread)?
4. **Load-time code.** libdav1d is linked into the app executable though only the helper calls it, so its `DT_INIT` and two `.init_array` entries (Arch build; none from dav1d's own sources) run in both processes before confinement. Acceptable, or should the helper `dlopen` it before the filter, keeping it out of the app's start at the cost of a hand-written loader?
5. **Binding layout.** `dav1d-sys` 0.8.3's `Dav1dSequenceHeader` is 744 bytes against C's 808 (#126). GitTurtle would read only `hbd`, `pri`, `trc`, `mtrx`, `color_range` and `chr` and never call `dav1d_parse_sequence_header`. Is a guard required (a test that no other `seq_hdr` field or that function is used), plus a `sizeof`/`offsetof` probe of `Dav1dSettings` and `Dav1dPicture` against the installed header on both architectures? `frame_size_limit` lives in `Dav1dSettings`, so a silent mismatch would disable it; proposed behavioural proof: a fixture whose AV1 frame header exceeds its `ispe` is refused with peak RSS under 64 MiB.
6. **Monochrome planes.** dav1d-rs may build U and V slices from unset pointers for I400 (#123). Require that the converter never asks for them, with a 4:0:0 fixture.
7. **Library provenance.** Refuse libdav1d below 1.4.1 from `dav1d_version()` in the helper, or rely on packages? Should packaging check that the executable's `NEEDED` lists `libdav1d.so.7` and that no `SYSTEM_DEPS_*` variable reached the build (the internal build `git clone`s and links dav1d 1.5.0 statically)?
8. **Memory.** Keep `RLIMIT_DATA` 1 GiB for the largest accepted AVIF (32 MP, 12-bit 4:4:4, alpha, film grain: about 192 MB of planes, a grain copy, a 64 MB alpha picture, 128 MB of RGBA and dav1d's frame state) and let anything larger abort as "stopped", or lower the AVIF pixel cap? Proposed: keep, and decide on section 5's peak.
9. **`mp4parse` age.** 0.17.0 (2023) lacks three years of fixes on `master` (the composition-time overflow `f2d96807`, `colr` and `a1lx` parsing), and `read_avif` also parses the `moov` of an `avis` file. It runs only in the helper under `catch_unwind`, and `deny.toml` refuses git dependencies. Is crates.io 0.17.0 acceptable on those terms?
10. **`hayro-jpeg2000` 0.4.1.** Are GitTurtle's preflights (tiles ≤ 65,535, 1–4 components, ≤ 16 bits, the `pclr`/`cmap` checks, the sample budget, reduced resolution only without subsampling or palette) plus the helper enough, given #1549, the tile vector, #1649 and #1409? #1680 gives wrong colours, not a memory or time risk.
11. **Reply.** Cap 10,240,000 bytes plus the header for edges up to 1,600; exact `scaled_dimensions` match, format echo, known flags, checked length, no trailing bytes; timings only as metrics; helper bytes never in wording, logs or labels. Anything to add?
12. **Request.** The helper reads the validated length (≤ 32 MiB), then sniffs. Should it sniff the first 4,096 bytes and refuse a mismatch before reading the rest?
13. **x86-64 evidence.** dav1d's x86-64 assembly differs from its AArch64 code, so the image tests need an executed ubuntu-24.04 run (libdav1d 1.4.1) under the filter, as PDF-T3 needed; a configured job is not evidence.

## 4. Slices (in order, disjoint files)

Prerequisite: PDF-T3 landed (the client enabled on both architectures). A2 is the largest; it mirrors PDF-T1 and may go to the hard implementer.

- **A1** `build(deps): add the AVIF and JPEG 2000 decoders for the Linux helper`: `crates/preview/Cargo.toml`, `Cargo.lock`, `docs/licenses/` (inventory, texts, supplements if a crate lacks its notice), `THIRD_PARTY_NOTICES.md` (`mp4parse` MPL-2.0 used unmodified with its source in `licenses/sources/`; libdav1d BSD-2-Clause as a dynamically linked system library), `.github/actions/setup-rust/action.yml` (`libdav1d-dev`, `pkg-config --exists dav1d`), `.github/actions/setup-rust/cache.py` (`dav1d` among the keyed Arch packages), `.github/workflows/quality.yml` (Arch `dav1d` in `pacman -Su`, `pacman -Q` and `pkg-config --exists`), `scripts/package-linux.sh` (`dav1d` in the pkg-config check); `deny.toml` unchanged. Acceptance: (1) `mp4parse =0.17.0`, `dav1d =0.11.1` and `hayro-jpeg2000 =0.4.1` (`std`, `image`) only under the helper's target `cfg`, and `cargo tree --target aarch64-apple-darwin -i dav1d` and `-i mp4parse` are empty; (2) `cargo deny check` passes on the workspace and the added-package count is recorded; (3) the collector's Linux output includes `mp4parse`'s complete source and the notices, and `THIRD_PARTY_NOTICES.md` names libdav1d; (4) executed hosted runs of the Ubuntu 24.04 and Arch jobs build with the packages, with no `SYSTEM_DEPS_*` variable set.
- **A2** `feat(preview): decode AVIF and JPEG 2000 images for the confined helper`: `crates/preview/src/{lib.rs, jpeg2000.rs, jpeg2000_hayro.rs, avif.rs, image_helper.rs, pdf_helper.rs}`, `crates/preview/AGENTS.md`, `crates/preview/tests/fixtures/` (new AVIF and JPEG 2000 fixtures and `README.md`), `scripts/create-image-codec-fixtures.py` (new). Acceptance: (1) through `serve`, the generated fixtures give the expected size, orientation and pixels within lossy tolerance: AVIF 8, 10 and 12 bits, 4:0:0, 4:2:0 and 4:4:4, straight and premultiplied alpha, `irot`/`imir`, film grain, a Display P3 profile, an animation's first frame; JP2/J2K RGB, 16-bit grey, alpha, ICC, palette, subsampled at full resolution and large at reduced resolution; (2) every bound in section 2 refuses before allocating, each hostile fixture citing its issue (#1549, the 1 × 1-tile file, #1649, frame larger than `ispe`, alpha size mismatch, grid, a sequence without a primary item, non-JP2-compatible JPX), and a decoder panic becomes an error kind; (3) `read_image_reply` refuses a wrong echo, unknown flags, a size other than `scaled_dimensions`, short, long and trailing frames; every error kind round-trips; PDF frames are unchanged and `PROTOCOL_VERSION` is 2; (4) `decode_image` still refuses AVIF and JPEG 2000 in-process on Linux, HEIF has its new sentence, and the macOS tests are unchanged; (5) the converter never reads U or V for I400, and nothing reads a `seq_hdr` field after `operating_points` or calls `dav1d_parse_sequence_header`.
- **A3** `feat(app): decode AVIF and JPEG 2000 in the confined helper on Linux` (after the security review): `crates/app/src/pdf_helper.rs` (the cap as a parameter, one crate-private runner), `crates/app/src/image_helper.rs` (new), the module line in `crates/app/src/main.rs`, `crates/app/tests/image_helper.rs` (new). Acceptance: (1) the real binary round-trips an AVIF and a JP2 with `/proc` showing the decided confinement (`Seccomp: 2`, `NoNewPrivs: 1`, `Threads: 1`), and no fixture ever ends in SIGSYS, on AArch64 and in an executed x86-64 run; (2) every hostile fixture ends in its error kind or "stopped" within the deadline plus 1 s, the frame-larger-than-`ispe` fixture with a recorded peak RSS; (3) with shell fakes: cancellation leaves no child within 100 ms; deadline, oversized reply, wrong echo, size mismatch, trailing bytes, early exit and start failure each give their own sentence with no zombie; (4) images and PDF pages share the two slots, a waiting request still cancels, and the PDF tests pass unchanged; (5) the executable's `NEEDED` includes `libdav1d.so.7` and contains no static copy of dav1d.
- **A4** `feat(app): show AVIF and JPEG 2000 images on Linux`: `crates/app/src/worker.rs`, `crates/app/src/markdown_view.rs`, `crates/app/AGENTS.md`, `crates/app/docs/content-and-layout.md`, `README.md` ("Linux gaps"), `docs/file-previews.md`, `docs/linux.md` (packages, platform limits, the seccomp and libdav1d requirements), `crates/app/tests/image_decode_bench.rs` (new, `#[ignore]` release), `docs/benchmarks/` (new record). Acceptance: (1) both call sites go through `image_helper::decode` and macOS behaviour is unchanged; (2) a result arriving after another selection, Back or a tab switch is dropped and its helper killed (`ps` shows none), and Compare never reopens; (3) wording, captions and Open in default app as section 2, with a design review of the captions and error states; (4) the release measurement within section 5's budgets and the native evidence recorded against the exact build.

## 5. Measurement and native evidence

**Fixtures.** Checked-in fixtures are generated from deterministic pixels (the half-red-blue pattern, gradients, a ramp of greys) by `scripts/create-image-codec-fixtures.py`, each with its command, tool version and SHA-256 in `crates/preview/tests/fixtures/README.md`. Tools on the development host today: Pillow 13.0.0 (AVIF through libavif, JPEG 2000 through OpenJPEG), ffmpeg 9.0.1 with libaom 3.15.0 (10- and 12-bit, 4:4:4, 4:0:0, film grain, sequences), OpenJPEG 2.5.4 `opj_compress` (tiles, subsampling, levels), libheif's `heif-enc`, ImageMagick 7.1.2.31 and libvips 8.18.6. The existing macOS-made `half-red-blue.avif`, `.jp2` and `.j2k` stay as cross-platform fixtures. Shapes no encoder writes (`prem`, `pclr`/`cmap`, ICC `colr` in JP2, grids, a frame larger than `ispe`, a sequence without a primary item) and the hostile inputs are built in test code, each citing its source: #1549's 97-byte codestream, the 16383 × 16383 1 × 1-tile variant from #1558, #1649's empty `cmap` and 32-bit column, five components, 17-bit samples, JPX without `jp2 ` compatibility, `ispe` of 40000 × 1, a 64 × 64 `ispe` over a 16384 × 16384 AV1 frame header, a 32 × 32 alpha under a 64 × 64 primary, a truncated OBU and a missing `ispe`.

**Public samples, local only** (in a disposable `.local/bench-images-<date>/` with SHA-256, never committed): the [AOM `av1-avif` test files](https://github.com/AOMediaCodec/av1-avif/tree/master/testFiles) (repository BSD-2-Clause, with folder licenses: Link-U CC BY-SA 4.0, Netflix CC Attribution-NonCommercial-NoDerivatives 4.0, Xiph CC BY-SA 3.0; Microsoft's files state rights only in their Exif): `Microsoft/Summer_in_Tomsk_720p_5x4_grid.avif` (grid refusal), `Microsoft/Ronda_rotate90.avif` (`irot`), `Microsoft/Chimera_10bit_cropped_to_1920x1008_with_HDR_metadata.avif` (HDR caption), `Netflix/avis/alpha_video.avif` (an animation with alpha), `Xiph/abandoned_filmgrain.avif` and `Xiph/tiger_3layer_3res.avif` (spatial layers), and the Link-U `fox` set (every depth, subsampling and odd size). JPEG 2000 measurement files are generated with `opj_compress`.

**Release measurement** (A4's `#[ignore]` release test writing JSON to an environment-named file; the executable's commit confirmed with `strings`; AArch64 Asahi host, and x86-64 where a host is available; the host idle): a 4032 × 3024 8-bit 4:2:0 AVIF; the same at 10 bits with alpha; a 30 MP 12-bit 4:4:4 AVIF with alpha and film grain (the largest accepted); a 6000 × 4000 lossy JP2 at reduced resolution; the same lossless; a 6000 × 4000 JP2 with no decomposition levels (full resolution, the slowest accepted); the 64 × 64 raw J2K; and the same images as PNG and JPEG through `decode_image` in-process for comparison. Per request: spawn, input write, parse, decode, conversion and resize, reply read and total; at least 20 runs, first after start and warm; p50, p95 and max; helper peak memory (`wait4` `ru_maxrss`); kill-to-reap time; executable size and `NEEDED` before and after; UI frame times while a 2 s JP2 decodes. Proposed budgets (warm, p95): 12 MP 8-bit AVIF at most 350 ms; 12 MP 10-bit with alpha at most 600 ms; lossy 24 MP JP2 at reduced resolution at most 400 ms; the slowest accepted file at most 3 s (30% of the deadline); helper start and exit at most 10 ms; the largest accepted peak at most 512 MiB (half of `RLIMIT_DATA`); no change for PNG and JPEG, which never reach the helper. A 12 MP AVIF over budget reopens question 3 (threads).

**Native evidence** (Omarchy on Hyprland/Wayland; Midnight and Daylight; 13 and 18 pt; scale 1 and fractional): a disposable repository with an added AVIF (Before absent), a modified AVIF that changes size (side by side, Overlay, Wipe, zoom and linked pan), an added and a modified JP2 and a raw J2K, a portrait AVIF stored with `irot`, an AVIF with alpha, an animated AVIF (first frame and its caption), a grid AVIF (its sentence and Open in default app), the #1549 codestream and the 1 × 1-tile file (their sentences, the app responsive, `ps` showing no helper afterwards), a slow legitimate JP2 with Back pressed mid-decode (Compare stays closed, no helper left), a HEIC (its sentence and Open in default app) and a Markdown file embedding an AVIF; AT-SPI names of the sides; `/proc/<pid>/status` of an app-started image helper. Later on a Mac: the same AVIF and JPEG 2000 files through ImageIO, unchanged.

## 6. Unknowns and decisions

**Unknowns.**
- Whether the animated AVIFs written by libavif, ffmpeg and Pillow always carry a primary item (the fixtures will show).
- Which item is primary in libavif's gain-map files: if it is the `tmap`, `mp4parse` refuses the file ("Image item type is neither 'av01' nor 'grid'", `lib.rs:2806`) and the person sees "Cannot read AVIF"; to check with a gain-map fixture.
- dav1d's single-thread time on this host and on x86-64, which decides question 3.
- The largest accepted file's peak memory against 1 GiB.
- `hayro-jpeg2000`'s colour fidelity against ImageIO (ICC, CMYK, sYCC) and its speed on large lossless files.
- When `hayro-jpeg2000` releases #1409, #1558 and #1679 (reduced resolution for subsampled images, an upstream budget), when `dav1d-rs` fixes #126 and #123, and whether `mp4parse` publishes again; each needs the research skill, and `hayro-jpeg2000` must move with `hayro`.
- Whether a default Ubuntu 24.04 desktop already has `libdav1d7` (not checked), and Ubuntu's arm64 package (only the noble page was read).

**Decisions made here** (delegated to the coordinator, chosen for a fast, beautiful and secure app):
1. Both formats decode in the confined helper and nowhere else on Linux: libdav1d is C, `hayro-jpeg2000` has no memory budget or cancellation, and the measured cost is about 2 ms per image.
2. Extend the one helper rather than add a second: one confinement to review and test; the flag and process name stay; `PROTOCOL_VERSION` 2.
3. Single-threaded dav1d with the filter unchanged; threads only after a measurement and a security review.
4. Animated AVIF shows its first frame, captioned; playback is a later item for both platforms.
5. Grids are refused with a sentence and Open in default app (`mp4parse` cannot expose the tiles; ImageIO shows them, a documented parity gap); `clap` is not applied and HDR is not tone-mapped, each disclosed in the caption; a gain-map file shows its base image.
6. Colour goes to sRGB through `moxcms` (already in the build), as ImageIO matches colour on macOS.
7. JPX decodes only when JP2-compatible; reduced-resolution JPEG 2000 only without subsampling or a palette until `hayro-jpeg2000` ships #1409.
8. The helper's image edge is capped at 1,600, keeping the reply under 10.3 MB.
9. `hayro-jpeg2000` stays at the 0.4.1 that `hayro` uses, adding only its `image` feature (`simd` is already on).
10. libdav1d is linked dynamically through pkg-config, never built internally, with a 1.4.1 floor checked in the helper.
11. Checked-in fixtures are generated; public samples serve only local measurement and native evidence.
12. HEIF/HEIC keeps Open in default app with the sentence "HEIF / HEIC images are not decoded on Linux.".

## 7. Security review of the design (2026-11-02)

This is an independent [security review](security-review.md) of this design at `9454e246`, done before any code. It holds the design to the [PDF helper's review](2026-11-02-linux-pdf-pages-architecture.md#6-security-review-of-the-design-2026-11-02) and to that review's implementation (`crates/app/src/pdf_helper.rs`, `crates/preview/src/pdf_helper.rs`, `crates/app/tests/pdf_helper.rs`). Verdict: **acceptable with the corrections and requirements below**, which A1–A4 build and prove.

The confinement argument carries over unchanged. Whatever runs in the helper (libdav1d's C and assembly, dav1d-rs's 49 `unsafe` sites, `mp4parse`, `hayro-jpeg2000`) gets:
- 10 s of time and 1 GiB of data;
- fd 0 to read, and fd 1 and `/dev/null` to write;
- no file, socket, process or signal call;
- a reply the client checks before it allocates anything.

The only helper-controlled bytes that reach the interface are the pixels of an image the attacker already wrote.

No finding blocks the design; the open facts at the end are evidence the slices produce. This is not a certification, and each slice's diff gets its own review. The research archives and dav1d 1.5.4 sources were read as data, never built.

**Corrections to sections 1–5.**
- C1: #126's root cause also reaches `Dav1dPicture`.
  - dav1d 1.5.4 (`picture.h:84-104`) puts `size_t n_itut_t35` right after `itut_t35` and before `reserved[4]`. `dav1d-sys` 0.8.3 (`src/lib.rs:536-556`) puts `n_itut_t34` after `itut_t35_ref`.
  - The sizes are equal, so `dav1d_get_picture` writing into dav1d-rs's zeroed value overflows nothing. In Rust, though, `frame_hdr_ref` to `itut_t35_ref` and `n_itut_t34` each sit one word off.
  - dav1d-rs 0.11.1 reads only `seq_hdr`, `data`, `stride`, `p`, `m`, `content_light`, `mastering_display` and `allocator_data`. These all come before the shift or at the matching tail offset, and dav1d-rs hands the struct back to C unchanged, so this is harmless today.
  - The guard is therefore "only fields whose offsets a probe proves" (requirements 6 and 8), not "nothing after `operating_points`".
- C2: The tile count is formula B-5 over the reference grid.
  - The count is `ceil((Xsiz − XTOsiz) / XTsiz) × ceil((Ysiz − YTOsiz) / YTsiz)`, exactly as `hayro-jpeg2000` allocates it (`j2c/codestream.rs:495-514`). The vector is built in `decode` (`j2c/decode.rs:38`, `tile.rs:137-140`).
  - The design's `ceil(w / tw) × ceil(h / th)` undercounts by up to one row and one column.
  - hayro enforces B-3 and B-4 (`codestream.rs:545-571`) but not `XOsiz < Xsiz` or `YOsiz < Ysiz`: `image_width` subtracts without a check (`:517-521`). GitTurtle's own SIZ parse must refuse both.
- C3: #1409 fixed three reduced-resolution panics, and "1 × 1 sampling, no palette" avoids only the subsampled one.
  - Empty resolution levels, and tile-parts that lower the level count, are still reachable in 0.4.1.
  - These are safe-Rust panics that the helper turns into an error kind; the preflight does not close them.
- C4: Images add no new crate to the helper, but they do add new code paths:
  - palette resolution (`resolve_palette_indices`), which hayro's `JPXDecode` never requests (this is why T2 could not reach #1649);
  - the `image` integration (`integration.rs`);
  - `moxcms` for JP2 ICC and CMYK.
- C5: With its `image` feature, `hayro-jpeg2000` 0.4.1 exports `register_decoding_hook` (`integration.rs:320-343`).
  - It installs a process-wide `image` hook that decodes JP2/J2K through `Jp2Decoder` with default settings for any guessed-format decode.
  - Called anywhere, it would put an unbounded JPEG 2000 decoder behind every `image` reader in that process (requirement 7).
- C6: The premise of question 3 needs two corrections.
  - `RLIMIT_NPROC` counts every task of the real user (and root is exempt), so it can never cap one helper's threads.
  - The request's magic cannot choose a filter, because the filter is installed before the first byte is read.
- C7: The libdav1d floor checks a version string, not a patch level.
  - Distributions backport fixes without changing `dav1d_version()`.
  - `NEEDED libdav1d.so.7` already implies 1.3.0 or later: that is the soname and `dav1d-sys`'s own pkg-config floor.
  - Keep the floor as a fail-closed policy, not as a CVE control.
  - dav1d-rs does not wrap `dav1d_version()`, so reading it needs one `unsafe` call through `dav1d-sys` (requirement 9).
- C8: The logger cannot be removed through dav1d-rs, because the only field of `Settings` is private (`dav1d-0.11.1/src/lib.rs:328-330`). Removing it would mean GitTurtle calling `dav1d_open` itself over `dav1d-sys`, which moves the binding's layout risk and many `unsafe` lines into GitTurtle. The default logger stays (question 2).
- C9: dav1d-rs's `Picture::bit_depth()` returns `p.bpc`, which is 8, 10 or 12, not "8 or 16" as its documentation says (`lib.rs:940-947`). Samples take two bytes whenever the value is above 8.

**Answers to section 3.**

1. **One process for pages and images.** Yes. Keep the same entry, argv grammar, setup order, `ALLOWED`, `LIMITS`, alarm, 10 s deadline and the two `HELPERS` slots, on two conditions:
   - the allow-list stays identical for both frames (question 2 shows it can);
   - the measured peak of the largest AVIF fits half of `RLIMIT_DATA` (question 8).

   Each helper serves one request in a fresh address space, so a hostile image cannot affect a later page. Confinement is fixed before the magic is read, so a frame cannot weaken it.

   If images ever need anything else (threads, more data), they get their own argv-selected mode with its own filter and review; the request bytes never select confinement. Sharing the slots keeps the worst case at 2 × 1 GiB. Requirement 17 bounds the one new way to hold a slot for a long time (Markdown).

2. **The allow-list and libdav1d.** At 1.5.4, with `n_threads` 1 and `max_frame_delay` 1, nothing on the paths read needs a call outside the allow-list:
   - `pthread_once` (`lib.c:142`) is uncontended and at most wakes waiters with `futex`.
   - CPU detection reads glibc's saved auxiliary vector (`arm/cpu.c:46-47`) or runs `cpuid` and `xgetbv`; neither is a syscall.
   - `dav1d_init_thread` is empty on Linux (`thread.h:144`).
   - `pthread_attr_init`, `setstacksize` and `destroy`, and the `dlsym(RTLD_DEFAULT, "__pthread_get_minstack")` lookup (`lib.c:100`), only search objects that are already loaded.
   - `dav1d_num_logical_processors` (which calls `sched_getaffinity`) runs only when `n_threads` is 0 (`lib.c:122`). `pthread_create` runs only when there are several task contexts (`lib.c:282`).
   - Memory comes from `malloc` and `posix_memalign` on the main arena: `brk`, anonymous read-write `mmap`, `munmap`, `mremap` and `madvise`, all allowed.
   - The default logger is `vfprintf(stderr)` (`log.c:43`). glibc's stderr is unbuffered, so this is one `write` on fd 2, which the filter already allows for Rust's allocation-failure message.

   The allow-list does not grow. If a settings mismatch left `n_threads` at 0, or reached `pthread_create`, the helper would die by SIGSYS; it would never run unconfined.

   The only build-dependent path found: a libdav1d built without `getauxval` reads `/proc/cpuinfo` (`arm/cpu.c:164`), which the filter kills. That fails closed, and running the tests against each supported package covers it.

   Keep the logger writing to `/dev/null` rather than removing it (C8). The proof is SIGSYS-free real-binary runs on both architectures, including a fixture that makes dav1d log (requirement 13). An `strace -f` list is welcome where `strace` exists but is not required, because the filter fails closed.

3. **Threads.** If threads are ever requested, the request must prove all of the following:
   - It uses a separate argv mode, so PDF requests never run under it.
   - `clone` is allowed only with glibc's exact thread flag set, checked on argument 0. `clone3` is answered with `ENOSYS` so glibc falls back to `clone`: its argument is a pointer the filter cannot read, and this needs a new `SECCOMP_RET_ERRNO` action.
   - Each extra per-thread call is argued on its own: `set_robust_list`, `rseq`, and `prctl` from `dav1d_set_thread_name` (`thread.h:150-153`), limited to `PR_SET_NAME`.
   - `n_threads` and `max_frame_delay` are set explicitly and never to 0 (0 calls `sched_getaffinity`).
   - `RLIMIT_NPROC` is lifted, since it cannot cap (C6); the thread cap comes from dav1d's settings.
   - `RLIMIT_CPU` counts every thread, so it is either scaled by the thread count or left below the wall-clock deadline as the bound.
   - Thread stacks (1 MiB plus glibc's minimum, `lib.c:161`) and per-thread arenas are measured inside `RLIMIT_DATA`.
   - The speed-up is measured against section 5's budget.

   dav1d's earlier thread race (CVE-2023-32570) is a reminder that this needs its own review.

4. **Load-time code.** Keep the dynamic link (`NEEDED libdav1d.so.7`); do not `dlopen`.
   - Every call that depends on the input runs only in the helper, either way.
   - What runs early in the app is load-time code that does not depend on the input, from a package root installed. It is in the same trust class as the 141 `.init_array` entries the PDF review already accepted, and dav1d's own sources declare no constructor.
   - A `dlopen` loader would have to drop dav1d-rs, which links its symbols at build time. That would put a hand-written FFI layer, with the #126 and C1 layout risk, into GitTurtle, only to confine code an attacker cannot reach.
   - The helper's empty environment resolves the same system library.

   Conditions:
   - requirement 15 records the init entries of each build;
   - requirement 6 keeps every dav1d call behind `serve`;
   - `docs/linux.md` says libdav1d is now a hard runtime dependency, and the app does not start without it (requirement 19).

5. **Binding layout.** A guard is required, and it is wider than proposed (C1). It has three parts:
   - Structure (requirements 6–8): no `unsafe` and only listed dav1d-rs calls in the decoding modules, and never `dav1d_parse_sequence_header`.
   - An ABI probe of every struct that crosses the boundary, on both architectures and both supported libdav1d builds (requirement 8).
   - A behavioural proof of `frame_size_limit`: a 64 × 64 `ispe` over a 16384 × 16384 frame header ends in its error kind, not "stopped", with the helper's `ru_maxrss` under 64 MiB (requirements 12 and 13).

6. **Monochrome planes.** Required, and it covers more than grey images: libavif encodes alpha as 4:0:0 AV1, so every alpha AVIF decodes an I400 picture. Requirement 10 sets the rule:
   - the converter matches `pixel_layout()` first;
   - it reads only Y for I400, and for every alpha item whatever its layout;
   - it uses only dav1d's default allocator.

   Fixtures: 4:0:0 at 8, 10 and 12 bits, and alpha at each depth.

7. **Library provenance.** Both checks are needed.
   - The packages carry the dependency.
   - The helper refuses a libdav1d older than 1.4.1, read from `dav1d_version()` with a strict parse. It takes the leading decimal `major.minor.patch`, refuses anything else, and runs before `mp4parse`, with its fixed sentence (C7, requirement 9).
   - Packaging and CI check that `NEEDED` lists `libdav1d.so.7` and that the executable defines no `dav1d_*` symbol (no static copy).
   - They also check that no `SYSTEM_DEPS_*` variable is set. The internal build in `dav1d-sys`'s `build.rs` would `git clone` dav1d 1.5.0 and link it statically.
   - They record `pkg-config --modversion dav1d` and its `libdir` (requirements 3 and 15).

8. **Memory.** Keep `RLIMIT_DATA` at 1 GiB for images, and do not raise it in this mode.
   - If section 5's largest accepted AVIF peaks above 512 MiB (half the limit, as the PDF review required), lower the AVIF pixel cap in the helper, or convert straight into the scaled output instead of a full-size RGBA buffer (128 MB at 32 MP). Record which (requirement 18).
   - Anything larger aborts as "stopped". That costs availability, not confinement.

9. **`mp4parse` age.** Acceptable on those terms:
   - crates.io `=0.17.0`, unmodified, with no features and `ParseStrictness::Normal`;
   - no `unsafe` in `src/` (none in `lib.rs`, `boxes.rs`, `macros.rs` or `unstable.rs`);
   - fallible allocation;
   - helper only, under `catch_unwind`.

   Release builds do not check integer overflow (the workspace's `[profile.release]` leaves it off), so overflows like the one fixed in `f2d96807` wrap to wrong values inside safe code. GitTurtle checks every value it uses: `ispe`, the alpha `ispe`, and item lengths within the input. Parsing the `moov` of an `avis` file in `read_avif` is accepted surface.

   If a RustSec advisory appears with no crates.io fix, switch to `avif-parse` (MPL-2.0), as section 1 records. `deny.toml` keeps refusing git dependencies.

10. **`hayro-jpeg2000` 0.4.1.** The preflights plus the helper are enough for safety.
    - The crate forbids `unsafe` (`lib.rs:68`).
    - In the helper, a panic becomes an error kind, a memory blow-up becomes an abort within 1 GiB, and a slow file is killed at 10 s.
    - The preflights give honest errors and avoid most aborts; they do not close the whole class (C3).

    Requirement 11 adds:
    - tile counting by B-5 (C2) and the offset checks;
    - decoding only through `Image::new` with GitTurtle's settings;
    - checking the decoded size and length before use (#1405 decodes one column too wide);
    - fixtures for #1409's other two panics.

    #1680 affects colour only.

11. **Reply.** Add the following:
    - Image replies get their own magic (`GTIr`), and only the image client accepts it.
    - The cap is computed from the request's edge: prefix + header + edge² × 4, so 4,000,000 pixel bytes for Markdown's 1,000, not the 1,600 maximum.
    - Image error kinds form a closed set, each with detail 0 (no version or count on the wire).
    - A kind or flag that the echoed format cannot produce (for example an AVIF-only flag on JP2) makes the reply unreadable.
    - The `scaled_dimensions` check uses the client's own requested edge.
    - Captions and wording are built from the format echo and the flags only.
    - There is no in-process decode after any helper failure, "could not start" included (requirement 14).

12. **Request.** Yes:
    - read `min(length, 4,096)` bytes first, the same bound `iso_image_format` already uses for `ftyp` (`metadata.rs:16`);
    - run `confined_image_format` on them and refuse a mismatch;
    - then allocate the body and copy the prefix in, as the PDF frame does with `%PDF-` (`pdf_helper.rs:229-245`).

    This is not a security boundary: the client routed with the same function, and 32 MiB fits within the limit. But it keeps "refuse before allocating" uniform, at no cost.

13. **x86-64 evidence.** Yes. A3 is not accepted without an executed hosted ubuntu-24.04 run (x86-64, libdav1d 1.4.1, the floor) of `tests/image_helper.rs` and `tests/pdf_helper.rs` under the filter, with the ABI probe run there too.
    - Record the runner's CPU flags, because dav1d chooses its SSE, AVX2 or AVX-512 code at run time.
    - Run the Arch x86-64 job (1.5.4) as well if it runs tests.
    - The PDF filter's own x86-64 evidence is already met (hosted run `41480657885`, recorded in the handoff), so this asks only for the image paths.
    - A configured job is not evidence.

**Requirements for A1** (dependencies).

1. **Scope.**
   - `mp4parse =0.17.0`, `dav1d =0.11.1` and `hayro-jpeg2000 =0.4.1` are declared only under the helper's target `cfg`.
   - `hayro-jpeg2000` gets exactly the features `std`, `simd` and `image` (not `logging`); `mp4parse` gets none.
   - `cargo tree -i` shows `gitturtle-preview` as the only dependent of `dav1d`, `dav1d-sys`, `mp4parse` and `hayro-jpeg2000`, and the app crate names none of them.
   - `cargo tree -e features -i image` is unchanged and has no `avif` or `avif-native` feature, so `image` can never decode AVIF in either process.

2. **Licences under the owner's rule** (`omarchy-full-support.md:737-740`).
   - `mp4parse` (MPL-2.0) is used unmodified: no `[patch]`, vendoring or edits.
     - Its lockfile checksum equals the crates.io archive's (`63a35203…`).
     - The complete source of that archive is in `docs/licenses/sources/` with its SHA-256, and `THIRD_PARTY_NOTICES.md` says where it is (MPL-2.0 §3.2(a)).
     - No code from `mp4parse` or `avif-parse` is copied into a GitTurtle file.
   - `dav1d` and `dav1d-sys`: MIT notices.
   - libdav1d (BSD-2-Clause) is recorded by hand as a system library that GitTurtle links but does not ship; if a bundle ever ships it, its notice ships too.
   - `hayro-jpeg2000` (`Apache-2.0 OR MIT`), `moxcms` and `image` are already in the inventory.
   - `cargo deny check` passes and lists the exact added packages, build-only ones included. No GPL, AGPL or LGPL.
   - Codec rule: only AV1 (AOMedia's royalty-free licence) and JPEG 2000 Part 1 are decoded.
     - HEVC cannot be decoded: `read_avif` refuses items that are neither `av01` nor `grid` (`mp4parse` `src/lib.rs:607`), and dav1d decodes only AV1.
     - JPX that is not JP2-compatible is refused.

3. **Provenance.**
   - `libdav1d-dev` and `dav1d` come only from the distributions' signed repositories.
   - The CI steps and `scripts/package-linux.sh` fail if any `SYSTEM_DEPS_` variable is set.
   - They log `pkg-config --modversion dav1d` and `pkg-config --variable=libdir dav1d`.

**Requirements for A2** (decoders, request frame, `serve`).

4. **Request order.** Every refusal happens before the body is allocated, in this order:
   - the 4-byte magic, then that frame's fixed header;
   - the version;
   - the format (1–3);
   - the edge (1..=1,600);
   - the length (≤ 32 MiB);
   - the prefix sniff (question 12), which must agree with the format.

   HEIF is never confined. An unknown magic gets a fixed failure frame.

5. **dav1d settings**, on a fresh `Settings::new()`, exactly:
   - `n_threads` 1 and `max_frame_delay` 1;
   - `frame_size_limit` = `ispe` width × height;
   - `all_layers` false and `operating_point` 0;
   - `apply_grain` true and `output_invisible_frames` false.

   Use one decoder per item. The send/get loop follows dav1d-rs's contract: after `Again`, call `send_pending_data`, never `send_data` again (it asserts). The loop has a fixed iteration bound and ends in an error kind.

6. **Structure.**
   - `avif.rs`, `jpeg2000_hayro.rs` and `image_helper.rs` are private, and each declares `#![forbid(unsafe_code)]`.
   - No dav1d, `mp4parse` or `hayro-jpeg2000` type appears in the crate's public API, and `serve` is the only way in.
   - `decode_image` keeps refusing AVIF, JP2 and J2K on Linux, with a test.
   - dav1d-rs calls are limited to:
     - `Settings::new` and the setters in requirement 5;
     - `Decoder::with_settings`, `send_data`, `send_pending_data` and `get_picture`;
     - `Picture::width`, `height`, `pixel_layout`, `bits_per_component`, `bit_depth`, `stride`, `plane`, `matrix_coefficients`, `color_range`, `color_primaries` and `transfer_characteristic`.
   - Never use a custom picture allocator (`with_settings_and_allocator`, `with_allocator`). It runs Rust callbacks under C frames, and `Plane`'s length (`stride × height`, `lib.rs:882-888`) is only backed by dav1d's default allocation.
   - Never call `content_light`, `mastering_display` or anything else outside the list. The crate guide records the list.

7. **JPEG 2000 entry and forbidden methods.**
   - Decode only through `Image::new(bytes, &settings)` and `decode` (or `Image` as an `ImageDecoder`), with GitTurtle's `DecodeSettings`.
   - Never use `Jp2Decoder` or `register_decoding_hook` (C5).
   - A workspace `clippy.toml` `disallowed-methods` list (enforced by the gate's `-D warnings`), or an equivalent source test, refuses:
     - `register_decoding_hook` in `hayro-jpeg2000`;
     - `image::hooks::register_decoding_hook` and `image::hooks::register_format_detection_hook`;
     - `dav1d::Decoder::with_settings_and_allocator` and `dav1d::Decoder::with_allocator`;
     - `dav1d_sys::dav1d_parse_sequence_header`.

8. **ABI probe**, checked in. For example, a script that compiles a C file against the installed `dav1d/*.h` and compares it with `size_of` and `offset_of!` from a test-only `dav1d-sys =0.8.3` dev-dependency, or an equivalent. It requires:
   - Equal sizes for every struct C writes into Rust memory:
     - `Dav1dSettings`, written by `dav1d_default_settings`;
     - `Dav1dData`, `Dav1dDataProps` and `Dav1dUserData`, written by `dav1d_data_wrap` and `dav1d_send_data`;
     - `Dav1dPicture`, written by `dav1d_get_picture`.
   - Equal offsets for:
     - every `Dav1dSettings` field;
     - `Dav1dPicture`'s `seq_hdr`, `frame_hdr`, `data`, `stride`, `p`, `m`, `content_light`, `mastering_display`, `itut_t35`, `ref` and `allocator_data`;
     - every field of `Dav1dPictureParameters`;
     - `Dav1dSequenceHeader`'s `layout`, `pri`, `trc`, `mtrx`, `chr`, `hbd` and `color_range`.
   - #126 and C1 are recorded as the expected mismatches.
   - Any other mismatch, or any change of the `dav1d` or `dav1d-sys` version or the libdav1d soname, blocks until reviewed.

9. **Version floor.**
   - One function calls `dav1d_sys::dav1d_version()` and copies the static string. It is the only `unsafe` in the new preview code.
   - The parse takes the leading decimal `major.minor.patch`. `1.5.4` and `1.4.1-0-g…` pass; `1.4.0` and anything that does not parse are refused.
   - The parser has unit tests.
   - The refusal is a fixed kind, returned before `mp4parse` runs.
   - `dav1d-sys` becomes a direct dependency (`=0.8.3`) for this one call only.

10. **AVIF conversion.**
    - The picture size must equal `ispe` for both items before any plane is touched.
    - The layout must be I400, I420, I422 or I444, at 8, 10 or 12 bits.
    - Samples take two bytes above 8 bits (C9).
    - `stride` must be at least width × bytes per sample, and the plane length at least `stride × (rows − 1) + width × bytes per sample`; otherwise the result is an error kind.
    - I400 pictures and every alpha item read Y only.
    - Indexing is safe Rust only.

11. **JPEG 2000 preflight.** The checks run before `decode`; the tile vector is allocated there, not in `Image::new`.
    - GitTurtle's own SIZ parse refuses `XOsiz ≥ Xsiz` or `YOsiz ≥ Ysiz`.
    - Tiles are counted by B-5 with checked arithmetic, at most 65,535.
    - 1–4 components, at most 16 bits each.
    - The `pclr` and `cmap` checks, and the budget, apply.
    - Reduced resolution is used only with 1 × 1 sampling and no palette.
    - Before decoding, the width and height `Image::new` reports must equal GitTurtle's own computation for the chosen resolution; `total_bytes()` derives from them.
    - The output is resized to exactly `scaled_dimensions(original, edge)`, with `original` taken from SIZ.

12. **Fixtures through `serve`.** Everything in A2's acceptance list, plus:
    - #1409's empty-resolution-level panic and its level-lowering tile-part panic, at reduced resolution with 1 × 1 sampling (each ends in an error kind);
    - one real `hayro-jpeg2000` panic that passes the preflight, kept for A3;
    - the frame-larger-than-`ispe` file, ending in its error kind;
    - `read_image_reply` refusing everything listed in question 11.

**Requirements for A3** (helper wiring and client).

13. **Confinement unchanged, proven under the real filter.**
    - `ALLOWED`, `LIMITS`, `ALARM_SECONDS`, `DEADLINE`, the slot count, the argv grammar and the setup order stay byte-for-byte the same. A diff that touches them is outside A3 and needs its own review.
    - Under the real filter, on AArch64 and in the x86-64 run:
      - every benign fixture exits 0;
      - `/proc/<pid>/status` of an image helper shows `Seccomp: 2`, `NoNewPrivs: 1` and `Threads: 1`;
      - started by the test with stderr on a pipe, a fixture that makes dav1d log delivers its message there, which proves the logger's `write` passes the filter;
      - the frame-larger-than-`ispe` fixture ends in its error kind, with the helper's `ru_maxrss` under 64 MiB, recorded;
      - the 4:0:0, alpha, film-grain and 12-bit fixtures decode;
      - the #1549 and 1 × 1-tile files end in their error kinds;
      - A2's real `hayro-jpeg2000` panic ends in its error kind;
      - a truncated OBU is refused;
      - each run ends within the deadline plus 1 s, and no test ever sees SIGSYS.

14. **Client.**
    - One crate-private runner is shared with PDF pages.
    - The reply cap is a per-request parameter (question 11).
    - Image replies are classified the way PDF replies are: success needs exit status 0 and a valid frame, and signals and exit codes map as today.
    - `image_helper::decode` never calls `decode_image` for a confined format, whatever the failure.

15. **Build evidence**, for each tested build (Arch 1.5.4 on AArch64, Ubuntu 1.4.1 on x86-64):
    - `readelf -d` lists `libdav1d.so.7`;
    - `readelf --dyn-syms` shows every `dav1d_*` symbol as undefined;
    - libdav1d's `DT_INIT` and `.init_array` entries are resolved to symbols. The expected result is toolchain stubs such as `_init` and `frame_dummy`; an entry in dav1d's own code reopens question 4;
    - the helper's `VmSize` at confinement is still at most 512 MiB (the existing assertion).

16. **Shell fakes.** Everything in the design's acceptance items (3) and (4), plus four cases that each give "unreadable" and leave no zombie:
    - a PDF reply to an image request;
    - an image reply to a PDF request;
    - an AVIF-only flag on a JP2 echo;
    - a reply one byte over the cap for its edge.

**Requirements for A4** (app view).

17. **Markdown bounds its helper time per document.**
    - After one confined image of a document ends in a timeout or "stopped", the document's remaining confined images are not sent to a helper and show a fixed sentence. An alternative is an aggregate 10 s helper budget per document; A4's review decides which.
    - Without this, a committed Markdown file with 32 local images (`MAX_PREVIEW_ASSETS`, `crates/git-core/src/preview_assets.rs:66`), each reaching the deadline, holds a slot and a core for about 320 s while it is open.
    - The 16 MiB pixel budget (`markdown_view.rs:416-447`) counts only pixels that come back.
    - `Slots::acquire` (`pdf_helper.rs:542-556`) does not serve requests in arrival order, so two such windows can keep PDF pages and image sides waiting.
    - Test it with a fake helper that sleeps past a short deadline.

18. **Peak memory.** The release measurement records the largest accepted AVIF's peak against 512 MiB, and question 8's action if it is above.

19. **`docs/linux.md`** states:
    - seccomp is required;
    - libdav1d is a hard runtime dependency, with the 1.4.1 floor (a version check, C7);
    - HEIF and HEIC are not decoded.

**Evidence each slice must produce.**
- A1:
  - requirement 1's `cargo tree` output, for Linux and for `aarch64-apple-darwin`;
  - `cargo deny check` with the list of added packages;
  - the collector output and the notices;
  - executed Ubuntu 24.04 and Arch runs, with package versions logged and no `SYSTEM_DEPS_*` variable set.
- A2:
  - the tests for requirements 4–12 on AArch64;
  - the ABI probe's output on AArch64 (Arch 1.5.4);
  - `cargo test -p gitturtle-preview`, and clippy with the disallowed list.
- A3:
  - the real-binary and shell-fake tests on AArch64 and in an executed ubuntu-24.04 x86-64 run, with CPU flags recorded (and the Asahi host's lack of SVE noted);
  - the ABI probe on x86-64 (Ubuntu 1.4.1);
  - requirement 15's `readelf` records;
  - the frame-larger-than-`ispe` `ru_maxrss`.
  - Recorded 2026-11-03: quality run [`41738961057`](https://github.com/FernandoX7/GitTurtle/actions/runs/41738961057) on `99814b1f`, Ubuntu 24.04 x86-64 tests job: the real helper under the filter reads `Seccomp: 2`, `NoNewPrivs: 1`, `Threads: 1`; libdav1d 1.4.1 logged "Frame size 16384x16384 exceeds limit 4096" with a helper peak of 23,720 KiB (`VmHWM` and `ru_maxrss`); the ABI probe matched `dav1d-sys` with the installed headers (`Dav1dSettings.frame_size_limit` at offset 20) and printed the CPU flags; the executable lists NEEDED `libdav1d.so.7`, and the `.init_array` step printed each entry's relocation and target. That run's packaging job refused a correct executable over a FILE symbol named for `dav1d_sys`'s codegen unit; `d7ca36d1` fixed the check, and run [`41745637784`](https://github.com/FernandoX7/GitTurtle/actions/runs/41745637784) on `d7ca36d1` is green on all 12 jobs, so A3 is accepted. AArch64 evidence: the fast gates of the A3 rounds on the Asahi host.
- A4:
  - requirement 17's test;
  - the release measurement;
  - native evidence, including `/proc/<pid>/status` of an image helper started by the app, and `ps` showing no helper left after Back.

**Open facts, closed by that evidence and not blocking.**
- The C layouts of `Dav1dData`, `Dav1dDataProps` and `Dav1dUserData`, whose headers were not in the research set, and Ubuntu 1.4.1's layouts in general (requirement 8).
- Which fixture makes dav1d log at 1.4.1 and 1.5.4: `obu.c` was not read. Requirement 13's stderr pipe shows it either way.
- What libdav1d's two `.init_array` entries are (requirement 15).
- The largest AVIF's peak memory (requirement 18).

**Residual risks.**
- A memory-safety bug can still occur in libdav1d's C and assembly, in dav1d-rs's `unsafe`, or in the SIMD crates. Inside the helper it gets 10 s, 1 GiB, the allow-list's kernel surface and a checked reply. The attacker controls the displayed pixels, as the file's author already does, and can keep one slot busy.
- The system libdav1d is patched at the distribution's pace (Ubuntu's copy is in universe). The floor cannot see backports or their absence.
- libdav1d's load-time code runs unconfined in the app and in every helper. It is trusted and does not depend on the input.
- libdav1d is now required for GitTurtle to start at all.
- Some assembly paths go unexercised. The Asahi host has no SVE, and a runner without AVX-512 never runs that path. A libdav1d built without `getauxval` would fail closed as "stopped".
- The `dav1d-sys` layouts are written by hand (#126, C1). Any version bump, or a libdav1d soname change, reopens the probe.
- `mp4parse` has unreleased fixes. `hayro-jpeg2000` still has #1549, the gap in #1558, #1409, #1649 and #1680 open. Their panics and aborts stay in the helper.
- Two helpers can briefly hold 2 GiB.
- Under requirement 17's first option, up to 32 legitimate but slow images in one document still run one after another.
- A third-party pool asserts AV1 patents. Whether that falls under "no patent-encumbered codec" is the owner's legal judgement, already taken for AVIF on 2026-10-28; this review makes no legal finding.
