# Third-party notices

GitTurtle-owned source is distributed under the [MIT license](LICENSE).
Third-party code, artwork, and fixtures retain their own terms. The root license
does not replace the notices described here.

## Toolkit and libraries

The locked GPUI Kit 0.6.0 stack and GPUI 0.3.4 backend use Apache-2.0. Local
patches preserve upstream licenses and document modifications:

| Vendored package | License | Modification record |
| --- | --- | --- |
| `gpui-base` 0.6.0 | [Apache-2.0](vendor/gpui-base/LICENSE-APACHE) | [Patch](vendor/gpui-base/GITTURTLE-PATCH.md) |
| `gpui-component` 0.6.0 | [Apache-2.0](vendor/gpui-component/LICENSE-APACHE) | [Patch](vendor/gpui-component/GITTURTLE-PATCH.md) |
| `gpui-pre-macos` 0.3.4 | [Apache-2.0](vendor/gpui-pre-macos/LICENSE-APACHE) | [Patch](vendor/gpui-pre-macos/README.gitturtle.md) |
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
on Linux; target-specific source records are authoritative. The traversal excludes
development dependency edges; Cargo workspace feature unification can still
conservatively include additional packages. This is a metadata inventory, not
a claim about every linked object. Test fixtures are not packaged.

Some published crates omit their license files. [Supplemental notices](docs/licenses/README.md)
preserve inspected upstream texts with immutable source URLs and hashes. Six
wrappers/libraries still require attribution review: `block`, `objc_exception`,
`mac`, `leak`, `leaky-cow`, and the Rust `ufbx` wrapper. Their Cargo manifests
declare licenses; a declaration alone does not establish that every required
notice has been collected. The [native ufbx notice](docs/licenses/assets/ufbx-native-LICENSE)
is retained separately. The target-specific `REVIEW_REQUIRED.md` lists only the
affected packages actually selected for that package.
The [Linux notice review](docs/licenses/linux-notice-review.md) ties `mac` 0.1.1
and `ufbx` 0.11.3 to their exact upstream source and records the remaining
attribution questions and prepared maintainer follow-ups.

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
- No font files are bundled by GitTurtle's assets or the selected GPUI icon asset
  bundle. Text uses installed system fonts. Font files present in decoder/text
  dependency test directories are not application assets. Preserve their own
  notices if separately redistributing those test corpora or adding bundled fonts.

## Test fixtures and screenshots

Generated image and model fixtures have
[documented provenance](crates/preview/tests/fixtures/README.md). The Khronos
GLB samples retain their [per-file notices and immutable source hashes](crates/preview/tests/fixtures/models/glb/README.md):
BoxInterleaved and RiggedSimple are © 2017 Cesium, CC-BY-4.0; Avocado is Microsoft,
2017, CC0-1.0. Their copied documentation is CC-BY-4.0. The meshopt Avocado fixture
is a modified geometry-only derivative, as its provenance records explain.

These fixtures are source/test assets and are not copied into application
packages. Keep attribution with any redistributed fixture or screenshot that
depicts it. The README's launch screenshots use GitTurtle's generated disposable
demo repository. Historical validation screenshots keep their dated evidence
context and require the [privacy review](docs/public-launch.md) before publication.
