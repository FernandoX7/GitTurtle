# Meshopt GLB native validation — September 10, 2026

Embedded `EXT_meshopt_compression` now opens directly in the retained native
static-geometry pipeline. Current captured corpus coverage rose from **94 to
1,224 successful decodes out of 1,379 GLBs**; all 1,224 completed fitted 360-solid,
720-solid and 720-wireframe CPU renders. Of the 1,255 meshopt assets, 1,130 now
render. Remaining refusals are skins and selected scenes without triangles.
The [coverage, independent reference and release timing record](benchmarks/2026-09-10-meshopt.md)
contains complete sanitized observations, source identities and raw samples.

## Source and local package

Final native source: `936094197bf4925d145b1a15090d07ea4b92f1cf`.
Release/package UUID: `F735CCD2-266C-351B-8B5E-9FC4F7A1CE5A`.
Packaged executable SHA256:
`87c2478bb91f1f0e7615c8dc2b93b5859eec71838d081745c27369b15f985c59`.
Source and package hashes are recorded in
[the build manifest](benchmarks/2026-09-10-meshopt/native-build.json), including
tracked compiled-input hashes, release/package UUIDs, compiler and hardware.
Release builds used `cargo build --release --locked -p gitturtle --target-dir target`;
local bundles used `scripts/package-macos.sh --no-build`. The package passed plist
and strict ad-hoc signature verification. Host: Apple M4 Max, 128 GiB memory,
macOS 26.6.2 (25G83), Rust 1.98.0, arm64.

Initial meshopt native interaction used `ad39e20`. Later native findings produced
`5c6c244` (circular standard-view matching and translucent orientation background),
`71598ae` (single-source LFS action wording), `5201e24` (safe Quick Open cold
restoration) and `9360941` (shrinking the repository title at narrow widths).
Each was rebuilt and packaged before testing its changed behavior.
Decoder/reference/CPU benchmark source stays
`ad39e20`; those later changes do not alter preview decoding or CPU rasterization.
The final evidence commit adds documentation and validation tooling only.

No installation, release publication, notarization, hosted CI, Linux build or
Linux native UI execution is implied. The existing installed application was
preserved. The current [support matrix](file-previews.md) and
[GLB contract](interactive-3d.md#glb-20-static-geometry) define support; the
[earlier GLB validation](glb-preview-validation.md) remains dated evidence.

## Baseline and changes driven by native inspection

The pre-meshopt baseline reviewed the public arch comparison in local package
UUID `D76A4494-C572-3410-B81A-1197D8D7DAE3`, executable SHA256
`aceea22dad21f6aed8c2f710f3a9b3d8c99fda654e41d0080d0228d75e58c4a9`.
Its source identity was not established by a clean build in this task, so these
baseline observations apply to that artifact. Commit selection stayed in History,
explicit file activation entered Compare, and the linked camera fitted both
revisions' union. Unsupported compressed files refused explicitly.

The [baseline narrow screenshot](screenshots/meshopt-preview/baseline-light-narrow.png)
records the pre-change layout. At the minimum window size with 16-point interface
text, the old toolbar/status
layout left about 165 pixels of canvas height and buried the geometry disclosure.
The updated grouped controls wrap onto two rows at that size, remove the empty
status row, and place the geometry summary first in a scrollable footer. The
canvas gained about 40 pixels in the matched light/Compact comparison. At the exact
minimum width, the repository title now shrinks within its existing bounds so
Settings does not wrap onto a separate row. Supported models need no conversion
or compression toggle. Wide layouts keep controls on
one row. Keyboard focus remains visible and controls retain descriptive labels.

Failed sides now keep a plain primary message and secondary format detail, their
actual file type, and useful captured-original actions. Absent files remain
separate from errors. A missing/corrupt local LFS side retains its exact pointer
and explicit download action while its independently verified opposite model
remains interactive. Pointer bytes are not offered as a model to system preview.
Quick Open uses “Download Source LFS” consistently with its Source panel.

The first-frame raster failure now says that no completed view exists, and has
no misleading orientation indicator. A later failure keeps the last completed
frame and says so. Wireframe recovered the deliberately overlapping fixture.
The orientation background is translucent so a narrow canvas does not hide
geometry under an opaque square. Circular yaw matching preserves standard-view
labels across normalized camera bookmarks: −45° and 315° remain Isometric.

Quick Open is retained with its exact scope through warm navigation. Cold restart
returns to the underlying History selection instead of interpreting a working
source with no blob identity as a fabricated Added change. Both an older invalid
saved bookmark and a newly saved Quick Source were exercised. Valid captured
History comparisons and independent camera bookmarks still restore. The
[restart contract](repository-tabs.md) records this deliberate scope.

## Native workflows exercised

The public fixture generator is
[`create-meshopt-preview-fixtures.py`](../scripts/create-meshopt-preview-fixtures.py).
The main Before/After commits are `f54f7add0b0abc30a71ab5820fdd4713eef5f7e4`
and `a83a25b6100c4baed55e40a93b726232a1b1d8bc`. A separately added valid raster
baseline and failing revision are `a51b86e59f6d72dc7c040f8c79949079d36fc663`
and `c59cd8af86d1dd6961f9563031ad1a380ee6e6be`. The latter is fixture HEAD;
main workflow checks explicitly selected the earlier commit.

| Workflow | Observed native result |
| --- | --- |
| History and activation | Selecting commits loads changed paths and stays in History. Activating a GLB enters Compare directly. Back retains commit/file context. |
| Revision placement | Both compressed arches have 36 triangles. Before bounds are `[-1200,-300,0]` to `[1200,300,2400]` mm; After bounds `[-300,-450,0]` to `[3300,450,3600]`. Initial linked target `[1050,0,1800]`, span `6532.5866` mm preserves their relative scale and displacement. |
| Camera controls | Pointer orbit and wheel zoom; keyboard orbit, Shift-arrow pan, plus/minus, Fit and Reset; all seven standard views; solid/wireframe and orientation indicators were exercised. |
| Linked and independent | Linked edits update both views. Unlinking changes only the active side. Before custom yaw/span and After isometric union pose survived Back, Settings and tab switches. Restart preserves equivalent camera positions and the corrected standard-view label. |
| Working Changes | Modified compressed arch opens from unstaged files. A History `raster` query does not hide a working `assembly` selection. Local Refresh returns to the captured comparison with selection/query intact; Back restores the prior History query and commit. No staging or writes were invoked. |
| Missing and malformed | Added uppercase `.GLB` has an absent Before; deleted compressed Avocado has an absent After. A malformed After leaves the valid Before interactive. Unsupported Draco, external-buffer and invalid meshopt stream messages name their actual limitations. |
| Local LFS | Available Before plus missing After stays split with interactive geometry, pointer and download action. Corrupt Quick Open source reports local verification failure without downloading. Worker tests additionally prove exact pointers, independent side permutations and retry after local availability changes. |
| Raster bounds | Valid Before plus 1,000 coincident After triangles produces a first-frame solid-raster refusal. Edges renders both; returning to solid retains the completed After wireframe with an explicit status. |
| Dense and rapid navigation | The 65,536-triangle compressed torus opens directly. Repeated pointer orbit/pan/zoom, wireframe and six tab-away/back cycles completed. Immediate Escape after opening returns to History; later completion does not reopen Compare. Rapid private-file selection followed by a repository switch did not publish a private model into the public tab. |
| Existing formats | Uncompressed interleaved GLB box remains a 12-triangle model with millimeter units. OBJ arch remains interactive with physical units explicitly unknown. |
| Private historical pairs | Three independently verified real pairs rendered at 544, 12,000 and 12,410 triangles per side. The last uses the authored static pose with animation playback explicitly omitted. Linked pan/orbit/zoom and rapid switching were exercised. All private captures, provenance and screenshots remain outside this checkout. |

Editing a History query to exclude the selected file deliberately clears that
selection under the existing [filter contract](../crates/app/docs/navigation-and-refresh.md#search-and-retained-inspections).
Separate History/Working queries and retained navigation were the isolation check.
Shift-pointer-drag was not separately synthesized by this automation interface;
keyboard pan was exercised. No claim of exhaustive gestures or all themes is made.

## Visual evidence

Only procedural or permissively licensed public fixture content appears in the
versioned [screenshots](screenshots/meshopt-preview/). Images were inspected in the
actual GPUI application, not only through accessibility output. The representative
appearance matrix includes Braden light/Compact with 16-point text and Midnight
dark/Comfortable with 13-point text, each wide and minimum-size. The trace records a 1480×981 wide viewport, the exact 1000×680 minimum viewport,
and a 1728×1052 maximized dark viewport. Capture tools scale large images;
[screenshot metadata](benchmarks/2026-09-10-meshopt/screenshots.json) records
image pixel dimensions, source increments and hashes. The final four appearance
captures and dense-model image use the final source; other screenshots identify
the earlier source on which their unchanged workflow was exercised.

- [Light wide](screenshots/meshopt-preview/union-light-wide.png), [light narrow with enlarged text](screenshots/meshopt-preview/union-light-narrow-large-text.png).
- [Dark wide](screenshots/meshopt-preview/union-dark-wide.png), [dark narrow](screenshots/meshopt-preview/union-dark-narrow.png).
- [One-sided failure](screenshots/meshopt-preview/one-sided-failure.png), [available/missing LFS](screenshots/meshopt-preview/lfs-one-sided.png), [corrupt Source LFS](screenshots/meshopt-preview/lfs-corrupt-source.png).
- [Deleted Avocado](screenshots/meshopt-preview/deleted-avocado.png), [Working Changes](screenshots/meshopt-preview/working-comparison.png), [dense model](screenshots/meshopt-preview/dense-native.png).
- [First-frame raster refusal](screenshots/meshopt-preview/raster-first-frame-failure.png), [retained frame after failure](screenshots/meshopt-preview/raster-retained-frame.png), [unsupported Draco](screenshots/meshopt-preview/unsupported-draco.png).

## Correctness, bounds and checks

The adapter supports ATTRIBUTES, TRIANGLES and INDICES modes and their permitted
NONE/OCTAHEDRAL/QUATERNION/EXPONENTIAL filters. Tests cover strict bitstream versions,
quantization, strides, sparse/shared views, fallback descriptors, transformed
instances, finite positions, index wrapping, arithmetic/ranges, output and instance
amplification, cancellation and retained-resource accounting. An independent
review caught invalid exponential-filter exponents producing finite values;
validation now checks the specification's signed `[-100,100]` domain before filtering.

The [contract](interactive-3d.md#glb-20-static-geometry) records independently bounded
compressed input, decoded views, temporary indices, retained geometry and raster
work. Native whole-stream decoder calls are bounded but not interruptible;
cooperative checks surround them and run through chunked filter/index work.
Captured BIN bytes are the only decoder input. No referenced URL or filesystem
resource is opened, and passive previews never fetch Git/LFS objects.

Formatting, targeted app/preview checks and final locked workspace tests/strict
all-target Clippy passed: **675 workspace tests passed, five existing ignores**.
The isolated Mermaid child also reports one pass within its parent test; it is
not counted twice. The [check manifest](benchmarks/2026-09-10-meshopt/checks.json)
records counts, commands and log hashes. The existing `block 0.1.6` future-incompatibility notice remains;
there were no new Clippy warnings. Local arm64 release/package checks passed.

## Performance and evidence limits

The [release measurements](benchmarks/2026-09-10-meshopt.md#release-timing-series)
retain first attempts, three warmups and 100 measured samples for nine identified
inputs. Dense torus p99 is 2.957 ms decode / 6.320 ms 720-solid CPU render;
private 33-node assembly p99 is 1.072 / 14.466 ms. These are supplied-byte CPU costs,
not an application speedup or end-to-end interaction latency.

[Native observations](benchmarks/2026-09-10-meshopt/native-observations.json) and
[36 raw completed-frame samples](benchmarks/2026-09-10-meshopt/native-frame-callbacks.tsv)
are separate from those benchmarks. Final-package dense interaction produced
6.625–15.234 ms callbacks (median 7.1595 ms). A fresh final-package window containing
the dense model retired its one image and reported zero retained images when
closed. These are bounded observations, not an application-wide leak conclusion. The callback spans request dispatch through the
next GPUI frame callback after current-result publication. It includes render lane,
pixel conversion and UI delivery; it excludes source decode, preceding input
handling, GPU completion and OS presentation. Superseded results are omitted.
Process RSS samples and renderer retirement counters do not establish GPU peak
memory or prove absence of leaks.

## Restoration and remaining limits

Both disposable repositories' final refs, index hashes, working bytes and local
LFS hashes matched their manifests. The original private source's current GLB
hashes and HEAD matched its passive inventory. Genuine application files were
backed up only after quitting the initial app; fixture state never replaced that
backup. The [restoration record](benchmarks/2026-09-10-meshopt/restoration.json) confirms
preferences, repository session and activity restored byte-for-byte before launch
and remained semantically identical afterward. The genuine three tabs, original
active repository/selected commit, saved drafts, Ember theme, Comfortable density,
13-point UI and 12-point code text were verified after relaunch. No fixture tab
remains. The installed executable hash is unchanged; the local tested bundle is
left running in the restored genuine session.

Static geometry remains the intended limit: no material/texture appearance,
animation playback, skin/morph evaluation, Draco or GPU instancing. Unsupported
geometry/visibility behavior and absent external resources refuse explicitly.
JSON glTF is not decoded. Core fixtures, native macOS observations, independent
reference comparisons and CPU timings are distinct evidence; none implies an
unchecked platform or arbitrary glTF asset will work.
