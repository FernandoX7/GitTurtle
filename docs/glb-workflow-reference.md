# Independent GLB appearance and deformation evidence

The release decoder at preview source `4b19124`, integrated in `8908b35`, matches
62 sampled public poses and all their sampled unlit pixels. Complete geometry
differs by at most `1.819e-12` mm; all sampled RGBA channels match exactly. An
additional 41 authored equation samples agree with the maintained reference.
Two external/malformed texture cases preserve matching geometry while reporting
their appearance limitation. The [reference summary](benchmarks/2026-09-10-glb-workflow/reference-summary.json)
records exact input, executable, runtime and script hashes. Native interaction,
release timings and package identity are separate evidence.

## Maintained reference and supported assertions

[`reference-glb-workflow.mjs`](../scripts/reference-glb-workflow.mjs) uses
[Three.js GLTFLoader](https://threejs.org/docs/#GLTFLoader), `AnimationMixer`,
`Mesh.getVertexPosition`, `SkinnedMesh` and `Raycaster` from installed Three.js
0.185.1. The official meshoptimizer 0.25 JavaScript/WASM decoder handles embedded
compressed views; [its pinned provenance](../crates/preview/tests/fixtures/models/glb/meshopt-provenance.json)
already records immutable upstream URLs and hashes. sharp 0.35.3 independently
decodes captured PNG/JPEG pixels. Exact module hashes are in the
[runtime manifest](benchmarks/2026-09-10-glb-workflow/reference-runtime.json).
These are validation dependencies and are not bundled with GitTurtle.

No reference asset resource is fetched. `GLTFLoader.parseAsync` receives captured
GLB bytes, and a process-wide fetch gate accepts only temporary Node blob URLs
created from those bytes. Image URIs are rejected before loading; buffer URIs
cannot supply geometry. The [resource record](benchmarks/2026-09-10-glb-workflow/reference-resource-policy.json)
records embedded blob reads and zero external reads. The shader-independent
appearance evaluator combines Three.js material factors, ray intersections,
interpolated vertex colors, sampler wrapping and sharp's decoded nearest texels
in linear color space. It then applies glTF alpha behavior and sRGB output. It
does not claim a PBR renderer match, a GPU screenshot match, or validation of all
filter modes. Focused production tests cover the additional filtering behavior.

The [18 public fixtures](../crates/preview/tests/fixtures/models/glb/workflow/README.md)
include material-only and PNG-only revision pairs, JPEG, normalized vertex
colors, blend/mask alpha, three wrapping modes, skin inverse binds and ignored
mesh transforms, default morph weight precedence, combined skin plus morph,
and linear/step/cubic animation. Samples include authored default poses,
selected absolute timestamps, clip quartiles and exact ends. Clip results are
clamped and do not implicitly loop. Geometry exports contain every retained
triangle coordinate in little-endian f64, including scene placement and the
glTF-meters to Z-up-millimeters conversion. The
[public comparison](benchmarks/2026-09-10-glb-workflow/reference-public-comparison.json)
retains each tested pose and pixel, and the
[fallback comparison](benchmarks/2026-09-10-glb-workflow/reference-fallback-comparison.json)
records the two independently usable geometry cases.

## Real assets and quaternion precision

Four private captured assets and the public Cesium `RiggedSimple.glb` provide
263 matched poses across 32 clips, including 13–29 joints, compressed quantized
animation, uncompressed animation, hierarchy placement and up to 17,064
triangles. Only ordinal labels, hashes and comparison results are published;
original paths, source bytes, expanded coordinates and authored clip/node names
remain in private local evidence. The
[complete comparison records](benchmarks/2026-09-10-glb-workflow/reference-private-comparison.json)
use `0.01` mm absolute or `1e-6` relative tolerance. Maximum observed absolute
difference is `0.007286` mm on an asset about 121,621 mm wide in its authored pose.

This corpus comparison makes one explicit reference adjustment. Stock Three.js
retains length error in normalized-integer quaternion keys. Production
normalizes linear-interpolation endpoints and sampled rotations to unit length.
The untouched Three.js run was preserved privately; its largest difference was
`6.865482` mm. A separate `--normalize-rotation-keys` run applies maintained
`THREE.Quaternion.normalize` to loaded keys, then uses Three.js interpolation and
skinning unchanged. No captured asset is rewritten. That removes the length-error
effect; remaining differences reflect Three.js Float32 track dequantization and
interpolation versus production f64 arithmetic. Uncompressed `RiggedSimple` and
the uncompressed private selection also agree with unmodified Three.js within
`0.000146` mm. These are adjusted-reference geometry results, not a claim of
identical output from unmodified Three.js for every quantized animation.

Private geometry references remove appearance-only JSON fields in memory so
unsupported KTX2/WebP resources cannot prevent independent pose evaluation.
Geometry, skin, animation and captured BIN bytes remain unchanged; production
always receives each complete original GLB. The
[glTF transformation and animation contract](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
defines unit rotations, morph-before-skin evaluation and ignored skinned-mesh
placement. The procedural closed-form expectations provide a second check on
those semantics without sharing the production interpolation implementation.

## Current corpus and refusal audit

The current no-follow capture contains 1,379 regular GLBs totaling 180,048,188
bytes. Source-relative paths, hashes and source HEAD match the previous milestone;
31 ordinal labels differ because the current capture sorts path components.
The [integrity record](benchmarks/2026-09-10-glb-workflow/corpus-integrity.json)
also verifies every final source and captured hash. Source repositories were only
read, and no Git/LFS resource was fetched.

| Current outcome | Assets |
| --- | ---: |
| Supported retained geometry | 1,300 |
| Empty selected scenes | 60 |
| Malformed skin data | 18 |
| Bounded unsupported morph count | 1 |

76 of the 95 skinned assets are supported. 87 assets expose 842 supported clips.
Every supported default pose and every clip midpoint completed Front-view
360-pixel solid, 720-pixel solid and 720-pixel edges rendering: 2,142 sampled
scenes and 6,426 successful raster calls. These are actual release CPU outcomes,
not full-time animation sweeps or native latency measurements. 1,299 supported
assets disclose approximate appearance; 1,183 omit at least one unavailable or
unsupported texture. In particular, retained geometry support does not imply
faithful rendering of the corpus's KTX2 and WebP appearances.
The [per-input observations](benchmarks/2026-09-10-glb-workflow/corpus.jsonl) and
[scan/source manifest](benchmarks/2026-09-10-glb-workflow/corpus-summary.json)
keep decoder, appearance and requested-frame outcomes distinct.

The malformed category has three different causes, independently checked in the
[refusal audit](benchmarks/2026-09-10-glb-workflow/refusal-audit.jsonl):

- 16 assets declare a skeleton node that is not an ancestor of every joint,
  violating glTF section 5.28.2. Three.js ignores this declaration, so its ability
  to display these assets does not establish validity.
- One asset contains six normalized U8 vertices with all-zero joint weights.
  This is not quantization drift around a sum of one.
- One asset stores inverse-bind fourth rows ending in `1.0000001192092896`, one
  Float32 epsilon above the required affine `1`. This is a near-affine exporter
  precision issue, explicitly refused under the finite matrix contract; it should
  not be described as severe corruption.

The single morph corpus asset exceeds eight morph targets and is explicitly
bounded rather than partially rendered. Its input also declares more than
300,000 position elements and thousands of total skin joints; public fixtures
establish supported morph correctness independently of that refusal.

## Reproduction

Supply a local `node_modules` directory containing the recorded Three.js and sharp
versions and the pinned meshoptimizer decoder. The scripts do not install or
download dependencies. Keep private captures, inventories and exports outside
the checkout and outside watched source repositories.

```sh
cargo build --release --locked -p gitturtle-preview --example probe_glb_workflow
node scripts/reference-glb-workflow.mjs /tmp/glb-reference-runtime/node_modules /tmp/meshoptimizer-js/meshopt_decoder.js /tmp/glb-reference crates/preview/tests/fixtures/models/glb/workflow/*.glb > /tmp/glb-reference.jsonl
target/release/examples/probe_glb_workflow /tmp/glb-actual crates/preview/tests/fixtures/models/glb/workflow/*.glb > /tmp/glb-actual.jsonl
python3 scripts/compare-glb-workflow.py /tmp/glb-reference.jsonl /tmp/glb-actual.jsonl /tmp/glb-reference /tmp/glb-actual --authored crates/preview/tests/fixtures/models/glb/workflow/authored-expectations.json
python3 scripts/capture-glb-workflow-corpus.py /path/to/authorized/models /tmp/private-glb-captures
python3 scripts/scan-glb-workflow-corpus.py target/release/examples/probe_glb_workflow /tmp/private-glb-captures/inventory.json /tmp/private-glb-scan
python3 scripts/capture-glb-workflow-corpus.py --verify /tmp/private-glb-captures/inventory.json
```

For known private pose selections, add `--geometry-only
--normalize-rotation-keys` to the reference command and use `--absolute-mm .01`
when comparing. Preserve a separate run without quaternion adjustment to retain
the diagnostic difference. `audit-glb-workflow-refusals.cjs` accepts the pinned
decoder and explicit captured GLBs to reproduce the narrow ancestry, inverse-bind
and weight checks. The corpus probe exports no private triangles; selected
reference exports are opt-in through its ordinary mode.
