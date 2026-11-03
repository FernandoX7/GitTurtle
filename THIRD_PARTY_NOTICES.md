# Third-party notices

GitTurtle-owned source is distributed under the [MIT license](LICENSE).
Third-party code, artwork, and fixtures retain their own terms. The root license
does not replace the notices described here.

## Toolkit and libraries

The locked GPUI Kit 0.6.0 stack and GPUI 0.3.4 backend use Apache-2.0, and its
AccessKit adapters MIT or Apache-2.0. Local patches preserve upstream licenses and document modifications:

| Vendored package | License | Modification record |
| --- | --- | --- |
| `accesskit_atspi_common` 0.19.1 | [MIT](vendor/accesskit_atspi_common/LICENSE-MIT) or [Apache-2.0](vendor/accesskit_atspi_common/LICENSE-APACHE), with [Chromium notices](vendor/accesskit_atspi_common/LICENSE.chromium) | [Patch](vendor/accesskit_atspi_common/GITTURTLE-PATCH.md) |
| `gpui-base` 0.6.0 | [Apache-2.0](vendor/gpui-base/LICENSE-APACHE) | [Patch](vendor/gpui-base/GITTURTLE-PATCH.md) |
| `gpui-component` 0.6.0 | [Apache-2.0](vendor/gpui-component/LICENSE-APACHE) | [Patch](vendor/gpui-component/GITTURTLE-PATCH.md) |
| `gpui-pre-linux` 0.3.4 | [Apache-2.0](vendor/gpui-pre-linux/LICENSE-APACHE) | [Patch](vendor/gpui-pre-linux/GITTURTLE-PATCH.md) |
| `gpui-pre-macos` 0.3.4 | [Apache-2.0](vendor/gpui-pre-macos/LICENSE-APACHE) | [Patch](vendor/gpui-pre-macos/README.gitturtle.md) |
| `gpui-pre-macros` 0.3.4 | [Apache-2.0](vendor/gpui-pre-macros/LICENSE-APACHE) | [Patch](vendor/gpui-pre-macros/GITTURTLE-PATCH.md) (test-only) |
| `mermaid-rs-renderer` 0.3.1 | [MIT](vendor/mermaid-rs-renderer/LICENSE) | [Patch](vendor/mermaid-rs-renderer/GITTURTLE-PATCH.md) |

Other direct and transitive dependencies are pinned in `Cargo.lock`. A package's
declared license expression may offer a choice of terms; bundled alternatives
do not change GitTurtle's license. Preserve embedded third-party notices as well
as a crate's top-level license when redistributing it. This matters for native
libraries such as meshoptimizer and ufbx, and for parser/renderer dependencies.
The collector preserves meshoptimizer's full MIT notice from its published C++
header, independently of the Rust wrapper's MIT/Apache license files.

The [notice collector](scripts/collect-third-party-licenses.py) follows runtime
and build dependencies of the application for the packaged target. It records
names, versions, declared licenses, authors, source URLs, and notice hashes in
`licenses/dependencies.json`, copies available license/notice files, includes
the local patch records, and includes the complete published sources of
MPL-2.0 dependencies in `licenses/sources/`. This currently includes `option-ext`
on Linux and `mp4parse` on Linux x86-64 and AArch64; target-specific source
records are authoritative. The traversal excludes
development dependency edges; Cargo workspace feature unification can still
conservatively include additional packages. This is a metadata inventory, not
a claim about every linked object. Test fixtures are not packaged.

Linux builds for x86-64 and AArch64 include the AVIF and JPEG 2000 decoders
for GitTurtle's confined preview helper; macOS builds include none of them and
decode both formats with ImageIO.

- `mp4parse` 0.17.0 (MPL-2.0) reads AVIF containers. GitTurtle uses the
  crates.io release unmodified. The collector places its complete source in
  each package, as MPL-2.0 section 3.2(a) asks: it generates
  `licenses/sources/mp4parse-0.17.0.tar.gz` from Cargo's unpacked copy of the
  crates.io archive whose SHA-256 is
  `63a35203d3c6ce92d5251c77520acb2e57108c88728695aa883f70023624c570`, the
  checksum `Cargo.lock` pins; the generated file's own SHA-256 differs. The
  source is also available from crates.io and
  [the upstream repository](https://github.com/mozilla/mp4parse-rust).
- `dav1d` 0.11.1 and `dav1d-sys` 0.8.3 (MIT) bind libdav1d, VideoLAN's AV1
  decoder (BSD-2-Clause, copyright VideoLAN and the dav1d authors).
  libdav1d is a dynamically linked system library: builds use the
  distribution's package (Arch `dav1d`, Ubuntu `libdav1d7`) through
  pkg-config, never a copy built from source, and GitTurtle packages do not
  ship it. A package that ever bundles libdav1d must ship its notice too.
- `hayro-jpeg2000` 0.4.1 (`Apache-2.0 OR MIT`) decodes JPEG 2000; it already
  reached Linux builds through hayro and now adds its `image` integration.
- `moxcms` 0.8.1 (`BSD-3-Clause OR Apache-2.0`, with its own license files)
  converts an AVIF's embedded ICC profile to sRGB. It already reached every
  build through `image`, with the same features; on these Linux targets the
  AVIF decoder also calls it directly.

Some published crates omit their license files. [Supplemental notices](docs/licenses/README.md)
preserve inspected upstream texts with immutable source URLs and hashes. Five
wrappers/libraries still require attribution review: `block`, `objc_exception`,
`mac`, `leak` and `leaky-cow`. Their Cargo manifests
declare licenses; a declaration alone does not establish that every required
notice has been collected. The [native ufbx notice](docs/licenses/assets/ufbx-native-LICENSE)
is retained separately. The target-specific `REVIEW_REQUIRED.md` lists only the
affected packages actually selected for that package.
The [Linux notice review](docs/licenses/linux-notice-review.md) ties `mac` 0.1.1
to its exact upstream source and records the remaining attribution question and
prepared maintainer follow-up, and how the Rust `ufbx` wrapper's notice was
resolved by its 0.11.5 release.

Local development packages include this report and remain unpublished. Before
a public binary release, resolve those notices and run the collector with
`--require-complete`; a nonzero result blocks that release. Regenerate against
the same lockfile and target as the executable. Review any new license, bundled
asset, native-library source, or source-availability requirement when updating
dependencies. System Git and dynamically linked system libraries remain supplied
by the OS/user; distribution of those components would require a separate review.

## Artwork, themes, and fonts

- GitTurtle control SVGs are original project artwork. The turtle icon and its
  derivatives have [recorded generation provenance](assets/icons/README.md) and
  [prompt metadata](assets/app-icon.prompt.json). GitKraken screenshots referenced
  in the design are external references; its artwork is not bundled.
- The toolkit's fallback icon bundle embeds Lucide artwork, including icons
  derived from Feather. Preserve its [ISC and MIT notices](docs/licenses/assets/lucide-LICENSE).
  This is separate from GitTurtle's own control SVGs.
- Tokyo Night, Catppuccin Mocha, and Nord are palette adaptations with adjusted
  native UI colors. Their upstream MIT notices are preserved for
  [Tokyo Night](docs/licenses/assets/tokyo-night-LICENSE.txt),
  [Catppuccin](docs/licenses/assets/catppuccin-LICENSE), and
  [Nord](docs/licenses/assets/nord-license). These credits do not imply endorsement.
- Solarized (Dark and Light), One (Atom's One Dark and One Light), Rosé Pine
  (Rosé Pine and Rosé Pine Dawn), Dracula (Dracula and its Alucard light variant),
  and Kanagawa (Kanagawa Wave and Kanagawa Lotus) are shipped palette adaptations
  with adjusted native UI colors, selected in the
  [palette research note](docs/development/2026-09-17-theme-palettes.md). Each is
  MIT; the copyright and permission notices are preserved for
  [Solarized](docs/licenses/assets/solarized-LICENSE),
  [One](docs/licenses/assets/atom-one-LICENSE.md),
  [Rosé Pine](docs/licenses/assets/rose-pine-LICENSE),
  [Dracula](docs/licenses/assets/dracula-LICENSE), and
  [Kanagawa](docs/licenses/assets/kanagawa-LICENSE), pinned to their inspected
  revisions in `docs/licenses/assets/sources.json`. One Light's license at the
  inspected `atom/one-light-syntax` revision is byte-identical to One Dark's, so
  the One notice covers both. The upstream values are recorded, with each
  repository, file and revision, in
  [`crates/app/src/appearance/sources.rs`](crates/app/src/appearance/sources.rs).
  MIT asks for nothing beyond those notices; theme names are used descriptively
  and imply no endorsement.
- Linux builds embed DejaVu Sans Mono 2.37 (Regular, Bold, Oblique and Bold
  Oblique, unmodified) as the code font, with its [provenance and hashes](assets/fonts/dejavu-sans-mono/README.md).
  Preserve its Bitstream Vera, public-domain DejaVu and Arev
  [notice](docs/licenses/assets/dejavu-fonts-LICENSE). They also embed IBM Plex
  Sans 3.005 (Regular, Medium, SemiBold, Bold, Italic and Bold Italic, unmodified)
  as the interface font, with its [provenance and hashes](assets/fonts/ibm-plex-sans/README.md),
  under the SIL Open Font License 1.1 with IBM's Reserved Font Name "Plex".
  Preserve its [notice](docs/licenses/assets/ibm-plex-sans-LICENSE.txt), keep the
  faces unmodified and unrenamed, and do not sell them by themselves. macOS builds
  embed no font and use installed system fonts. Linux builds for x86-64 and
  AArch64 also link the hayro PDF renderer (`Apache-2.0 OR MIT`), which embeds Foxit's standard-font
  substitutes extracted from PDFium (BSD-3-Clause, `assets/LICENSE_FOXIT` in
  `hayro-interpret`), Adobe's predefined CMaps (BSD-3-Clause, `assets/LICENSE.txt`
  in `hayro-cmap`) and CC0-1.0 ICC profiles. hayro's `NOTICE.md` records code
  adapted from PDFBox, pdf.js and the png crate under Apache-2.0. The collector
  copies the notices the crates ship, and the [supplements](docs/licenses/README.md)
  add `NOTICE.md` and the CMYK profile's CC0 text, which the published crates
  omit or name differently. macOS builds do not include hayro. Font files
  present in decoder/text dependency test directories are not application
  assets. Preserve their own notices if separately redistributing those test
  corpora.

## Test fixtures and screenshots

Generated image and model fixtures have
[documented provenance](crates/preview/tests/fixtures/README.md). The Khronos
GLB samples retain their [per-file notices and immutable source hashes](crates/preview/tests/fixtures/models/glb/README.md):
BoxInterleaved and RiggedSimple are © 2017 Cesium, CC-BY-4.0; Avocado is Microsoft,
2017, CC0-1.0. Their copied documentation is CC-BY-4.0. The meshopt Avocado fixture
is a modified geometry-only derivative, as its provenance records explain.
The 22 bundled-theme [Omarchy fixtures](crates/app/tests/fixtures/omarchy/README.md)
are unmodified copies of Omarchy's bundled `colors.toml` files, MIT licensed,
Copyright (c) David Heinemeier Hansson; their README carries the license text.
The two `alacritty-*.toml` fixtures beside them are GitTurtle's own.

These fixtures are source/test assets and are not copied into application
packages. Keep attribution with any redistributed fixture or screenshot that
depicts it. The README's launch screenshots use GitTurtle's generated disposable
demo repository. Historical validation screenshots keep their dated evidence
context and require the [privacy review](docs/public-launch.md) before publication.
