# GLB preview validation — September 10, 2026

This record covers native GLB static geometry and the shared model-control layout.
The [support contract](interactive-3d.md#glb-20-static-geometry) defines the supported
subset and independent limits; [fixture provenance](../crates/preview/tests/fixtures/models/glb/README.md)
identifies synthetic and permissively licensed inputs.

## Source and artifact identity

Integrated model implementation: `999bcd9851741f44d53f97e4656ac87b892b7d7f`.
The working tree was clean when building this release. `cargo build --release --locked -p gitturtle
--target-dir target` and `scripts/package-macos.sh --no-build` produced the local
arm64 `dist/GitTurtle.app`, with matching release/package Mach-O UUID
`44B5E8F1-0BD8-39D6-851C-3A4AAF858606`. Packaged executable SHA256:
`edbf3e6801484a8b416c8b9468c2e707b35dcf423fda22cfb88e1e6798b8b033`.
The package script verified its local ad-hoc signature. No release publication,
notarization, installation or hosted build is implied.

Host: Apple M4 Max, 128 GiB memory, macOS 26.6.2 (25G83), Rust 1.98.0.

## Baseline and corrections

Before implementation, the installed native app was reviewed using a disposable
OBJ tetrahedron comparison. Its executable SHA256 was
`3ce5c6e541c1c7d459982c63ea497705511680f837ec78fb32ea5a8e1e21d020`; its historical
compiled source identity was not established, so this baseline applies only to
that artifact. Commit selection stayed in History; file activation entered model
comparison. Initial union fitting preserved a doubled X extent. Front and zoom
changed both cameras, while unlinking allowed an independent Before orientation.

An initial GLB release exposed two actionable presentation issues. At a
1044×698 viewport with 16-point interface text, wrapping all seven standard-view
buttons and long camera-link labels left a very shallow model canvas. Standard
views now live in a checked native menu beside a shorter link toggle, with compact
header spacing, a short pointer hint and a scrollable details footer. The loading
accessibility status now says Preparing until a first frame exists. Invalid and
unsupported model sides use the same accessible model comparison surface, keeping
errors separate from the generic filename-sniffer fallback.

## Automated checks

- `cargo fmt --all -- --check` passed.
- `cargo check --locked -p gitturtle` passed during targeted integration.
- GLB app checks passed for exact historical/working captures, missing sides,
  independent errors, retryable local LFS resolution, cache accounting and initial
  union camera fitting. Existing model-view/source-retention checks also passed.
- Preview tests: 85 passed, one existing SVG probe ignored; 26 GLB tests cover
  transforms, instances, scene choice, units, accessors, sparse/quantized layouts,
  container truncations, external resources, extensions, deformation refusals,
  independent amplification budgets and cooperative cancellation.
- The model integration workspace run passed 648 tests, with five existing
  ignored tests. The later tightened primitive-restart assertion passed its
  focused rerun. After the native filter correction, the final combined run
  passed 649 tests, with the same five ignored tests.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` passed.
  Cargo still reports the existing dependency `block 0.1.6` future-incompatibility
  notice; it is not a new GLB warning.

One final-suite attempt hit the existing three-second native askpass test deadline
with a broken pipe. That unchanged test passed its focused rerun in 0.02 seconds;
the complete workspace rerun then passed. Both attempts remain in the local QA
logs. No authentication code was changed for this milestone.

The independent specification audit reproduced and corrected arbitrary
`extras.extensions` metadata being mistaken for extension declarations, legal
matrix accessor trailing-padding cases, and unchecked references in unused
primitive metadata. Its six reproduction cases now have the expected outcomes.
Eight captured authorized local models match an independent Python reference
exactly for triangle counts and all 48 transformed bound coordinates; four
compressed cases refuse explicitly. Those private captures and their provenance
remain outside the repository.

## Native workflow evidence

Final native fixture history: Before
`86fd3e86a312165b609d5adc90411c3b43daebee`, After
`04b2ae53f7113d7da006c264aa52fafb0572b30a`. The generator records working bytes,
index SHA256 and rename-aware statuses; deletion is independent of the truncated
file fixture. The Before/After arch has 36 triangles per side and union bounds
`[-1200,-450,0]` to `[3300,450,3600]` mm.

The package identified above exercised these cases:

| Native case | Observed result |
| --- | --- |
| History selection and union fit | Commit selection stayed in History. Explicit activation opened two 36-triangle GLB models. Both cameras targeted `[1050,0,1800]` mm at span `6532.5866` mm; the displaced/larger After arch remained visibly displaced/larger. |
| Pointer and keyboard cameras | Pointer orbit and wheel zoom worked in the initial GLB package. The identified package verified keyboard orbit, Shift-arrow pan, Fit/Reset, edges, and checked standard-view menu activation by pointer and keyboard. Linked cameras changed together; independent orbit changed only Before and showed Custom. |
| Retained navigation | Back retained the commit/file context and model camera state. Settings preserved comparison state. A rapid model replacement followed by Back stayed in History, and switching repository tabs restored each comparison without cross-repository content. |
| Missing and unsupported sides | History deletion displayed a 12-triangle Before and absent After. Working addition recognized uppercase `.GLB` with absent Before; deletion displayed absent After; modified working bytes reversed the arch's scale/placement change. Draco, meshopt and external geometry errors were readable in the model surface. |
| Appearance and layout | Braden/Compact at 16-point text was inspected at 1480×980 and the 1000×680 minimum viewport. The narrow model canvas remained usable at approximately 162 pixels high, with a checked view menu and scrollable details. Ember/Comfortable at 13-point text was inspected in a wider window. |
| Representative public asset | The 8.1 MB Khronos Avocado opened as 682 triangles in neutral flat shading, with appearance/animation omissions explicitly described. Its physical extent was `42.5618 × 27.6180 × 62.8958` mm. |
| Authorized local corpus | A real historical pair showed 6,271 triangles on both sides and identical placement/bounds despite changed texture encoding. A 13,686-triangle assembly opened; an `EXT_meshopt_compression` asset refused explicitly. Only captured blobs in a separate disposable repository were used for these native checks; private screenshots remain outside this repository. |

The first GLB native package, used for pointer orbit/wheel and the initial layout
diagnosis, had UUID `214EEA95-1D40-332F-BDC5-B3E74F3BB270`. The final model
controls were then checked in the identified package above. Shift-pointer-drag was not reliably emitted by the
automation tool; pan was verified with Shift-arrow input. Spoken VoiceOver,
native Linux, hosted CI, GPU timing and long-duration memory growth were not
tested. Accessible names/states and visual screenshots were inspected together.

Native testing also found a retained History path query could clear a completed
Working Changes preview. The final correction keeps the two filter scopes
independent, cancels stale History filter results, and reapplies the retained
query on return to History. Its GPUI regression and final package check are
recorded with the final correction identity below.

Representative unaltered native screenshots contain only authored/permissive
fixtures: [wide union](screenshots/glb-preview/union-light-wide.jpg),
[minimum size with enlarged text](screenshots/glb-preview/union-light-narrow-large-text.jpg),
[unsupported side](screenshots/glb-preview/unsupported-light-narrow.jpg), and
[Avocado in Ember](screenshots/glb-preview/avocado-dark-wide.jpg).

## Final correction and state restoration

The filter correction is commit `25627c103d5d5a43cdaaf90a9d3e8a74c1659927`.
Formatting, the 649-test workspace run, strict all-target workspace Clippy and
release compilation passed on these Rust inputs. Only validation documentation
and evidence remained uncommitted during the build. The corrected local package
and release executable share Mach-O UUID
`D76A4494-C572-3410-B81A-1197D8D7DAE3`; the ad-hoc signed package executable SHA256
is `aceea22dad21f6aed8c2f710f3a9b3d8c99fda654e41d0080d0228d75e58c4a9`.
Strict package signature verification passed. Signing changes executable bytes,
so the unsigned release binary is identified through its matching UUID.

In that exact package, History retained the query `external`, while Working
Changes filtered to `added`. Activating `working-added.GLB` showed absent Before
and a 36-triangle After; Refresh retained the correct model. Back restored the
History query and its external-buffer row. The [corrected working preview](screenshots/glb-preview/working-added-dark.jpg)
records that native result. The earlier geometry, model-control and benchmark
evidence remains tied to `999bcd9`; the final correction changes filter/navigation
state only.

After quitting the fixture session, all three original application-data files
were restored byte-for-byte from the pre-test backup. Relaunching the corrected
local bundle restored the three genuine tabs in order, the original active tab
and selected commit, Ember/Comfortable with 13-point interface and 12-point code
text, saved drafts and activity. Settings/drafts, tab order/selection and activity
were independently compared after relaunch. The installed application remained
unchanged at its baseline SHA256; no install or system-setting changes occurred.
The disposable fixture's index, refs and every captured working-file hash still
matched its manifest after the final check.

## Release measurements

See [raw samples and decode/render measurements](benchmarks/2026-09-10-glb.md).
These are supplied-byte CPU measurements with resident input and fresh geometry
decode, not native input-to-display or GPU timing.
