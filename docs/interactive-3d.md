# Interactive 3D comparison and finite CAD support

The September 2026 implementation retains supplied-byte model scenes for STL,
OBJ, FBX, GLB, 3MF and the STEP subset below. GLB scenes also retain bounded
appearance and supported animation evaluation data. `decode_geometry` consumes
supplied bytes;
`render_model` produces one requested orthographic frame on a background worker.
Camera updates do no parsing, geometry conversion or pixel allocation.

`ModelCamera::fit` takes the union of both revisions' bounds. Linked Before/After
cameras share target, orientation and span, preserving changes in size and
position. Independent cameras can inspect each version separately. Orbit, pan,
zoom, Fit, Reset and Isometric/Front/Back/Left/Right/Top/Bottom views operate on the
retained scene. Wireframe shows all triangle edges, including hidden edges and
triangulation diagonals. The orientation API exposes labelled X/Y/Z screen vectors
and colors so direction is available without color alone.

The native integration, runtime resource measurements and exercised build identity
belong to the current milestone record; decoder tests alone do not establish
native interaction or GPU cleanup.

Each side groups Fit/Reset, zoom and edges beside the standard-view menu and
camera-link toggle, wrapping as space requires. The menu retains all seven named
views and a checked current view; custom orbit is labelled Custom view. Geometry
counts and the applicable appearance/shading disclosure lead the scrollable details footer.
Updating status overlays the canvas without reserving an empty row. Unsupported
and missing sides use the same labelled comparison surface; errors are accessible,
bounded and scrollable, and a usable opposite side keeps its controls. Orientation
indicators appear only after a completed frame exists.

## Coordinates, units and source

3MF core build transforms and nested components are expanded before unit
conversion. Supported source units are micron, millimeter, centimeter, inch, foot
and meter; display coordinates use millimeters. FBX node placements are applied,
axes normalize to right-handed Z-up and declared units normalize to millimeters.
When FBX omits its unit setting, the FBX library's centimeter default is disclosed.
FBX remains a static default-pose mesh. OBJ and STL preserve source coordinates;
neither establishes physical units. A camera link between unknown and known units
does not establish physical equivalence.

All parsers remain supplied-byte operations. GLB can decode embedded PNG/JPEG
material textures from its captured BIN chunk. No external geometry, material,
texture, cache, script or repository-relative path is resolved. Exact source and
captured-byte identity remain with each original side. Neither retained triangles
nor rendered frames can become a staging patch.

<a id="glb-20-static-geometry"></a>

## GLB 2.0 appearance, deformation and animation

Case-insensitive `.glb` recognition enters the same native retained-model viewer.
The decoder validates the binary container and the independent glTF asset version;
JSON `.gltf` files are not part of this support. Indexed and non-indexed triangle
primitives, multiple primitives per mesh, shared mesh instances, nested nodes and
matrix or translation/rotation/scale placements retain their scene coordinates.
The declared default scene is selected; when omitted, scene zero is used and the
fallback is disclosed. Assets without scenes are refused. Other scenes are not
merged into an invented assembly.

glTF uses right-handed Y-up coordinates and meters. After composing node world
transforms, GitTurtle converts `[x, y, z]` to `[1000*x, -1000*z, 1000*y]` in its
right-handed Z-up/mm convention. This rotation preserves handedness and maps
glTF's front to the existing Front view. Each revision keeps its placement and
scale; only the shared camera fits the union of both bounds.

### Appearance and shading

`KHR_materials_unlit` uses the glTF unlit equation: linear `baseColorFactor` ×
linear `COLOR_0` × sampled base-color texture, with authored alpha mode and
sidedness. RGB texels are decoded from sRGB before filtering; alpha stays linear.
Linear RGB is converted to sRGB for display without tone mapping. VEC3 colors
have implicit alpha one; VEC4 colors retain alpha. FLOAT and normalized unsigned
BYTE/SHORT colors are supported. Material-only and texture-only changes therefore
remain visible with identical scene geometry and camera settings.

Core metallic/roughness materials use **base-color inspection with simplified
flat diffuse lighting**, identified in the native viewer and details. This is not
physically based material reproduction. Metallic/roughness values/maps, authored
normals, normal/occlusion/emissive maps, environment illumination, authored lights
and cameras are omitted. Known unsupported material extensions are named in the
details. Wireframe remains a geometry inspection view with neutral edge colors.

Embedded PNG/JPEG textures support the material's selected `TEXCOORD_n`, including
FLOAT and normalized unsigned BYTE/SHORT coordinates, with additional integer UV
layouts under required `KHR_mesh_quantization`. Image coordinates use the encoded
top row at V=0; image orientation and color profiles are ignored as required by
glTF. Samplers implement REPEAT, MIRRORED_REPEAT and CLAMP_TO_EDGE independently
on each axis, NEAREST/LINEAR magnification and all six core minification modes.
Missing sampler settings use repeat wrapping and linear/trilinear filtering.
Linear-light area-averaged mipmaps include non-power-of-two images. Orthographic
UV derivatives select mip levels; anisotropic filtering is not implemented.

OPAQUE ignores alpha. MASK discards samples below `alphaCutoff` (default 0.5)
without writing depth. BLEND sorts surviving fragments **at each pixel** and
composites back to front in linear light, including intersecting triangles; a
half-open shared-edge rule prevents duplicate transparent edge contributions.
This is bounded single-sample rasterization, without antialiasing or alpha-to-
coverage. Exceeding a transparency/raster cap returns a specific frame error;
wireframe can still inspect independently valid geometry. Single-sided materials
cull backfaces and double-sided materials retain them. Mirrored unskinned node
transforms correct the winding convention; skinned faces use their posed vertex
winding without reapplying the ignored mesh-node transform.

Only captured BIN image bytes are decoded. Resource URIs, including data URIs,
are never opened. Unsupported texture-coordinate extensions (including
`KHR_texture_transform`), absent image resources, malformed image headers,
mismatched MIME types, invalid appearance accessor ranges and appearance memory
limits omit the affected resource with a warning while retaining valid geometry,
base-color factors and other valid appearance inputs. Texture extensions can use
a valid core PNG/JPEG fallback with an explicit extension disclosure. Malformed
materials fall back to neutral double-sided geometry. Structural geometry errors,
unsupported geometry/visibility extensions, Draco, GPU instancing and unknown
required extensions still refuse the affected side.

### Default deformation and animation

The initial view evaluates authored node/mesh morph weights and supported skins
in the default pose. Morph POSITION deltas are applied before skinning. Node
weights override mesh weights; unspecified weights are zero. Joint world
transforms and inverse bind matrices produce weighted world-space positions;
missing inverse bind matrices mean identity. The skinned mesh node transform is
not applied a second time. JOINTS_0/WEIGHTS_0 and optional JOINTS_1/WEIGHTS_1 provide
up to eight influences. Integer weights are normalized; valid near-unit weight
sums are normalized before evaluation. Joints must belong to the selected scene
and share a root, with a declared skeleton ancestral to every joint. Invalid
skins or morph geometry are refused rather than displaying an undeformed stand-in.
Morph NORMAL/TANGENT data does not alter the disclosed flat inspection shading.

Supported animation clips target translation, rotation, scale and morph weights,
including animated joint hierarchies. STEP holds the left key, LINEAR interpolates
vectors/weights and uses shortest-path quaternion slerp, and CUBICSPLINE uses
interval-scaled Hermite tangents followed by quaternion normalization. Before the
first key and after the last key, channels hold their endpoint values. Nonfinite
values, invalid key order/layout, duplicate targets and unsupported channels are
explicit errors. A malformed or unsupported optional clip is unavailable with a
reason; default geometry and other valid clips remain available. Animation does
not target matrix-authored nodes or material properties. An animated zero scale
may collapse all triangles and correctly display background at that timestamp.

The native viewer starts at Default pose. Each side selects its own clip; Play
selects the first positive-duration clip when neither side has a playable
selection. Playback shares seconds across Before/After: shorter clips hold their
last pose until the longest selected clip loops. Clip selection and timeline
scrubbing pause both sides. The timeline supports pointer/keyboard scrubbing,
Start and 1/30-second stepping, and shows comparison time/duration alongside each
side's displayed pose time. Exact endpoint poses remain available by scrubbing.
Reduce Motion disables automatic playback while preserving clip selection,
scrubbing and stepping.

Evaluation and raster preparation run on the cancellable model worker. Playback
samples at most 30 times per second and skips elapsed samples when rendering is
slower; it does not cancel every in-flight frame to chase the clock. Playing,
scrubbing and dragging use 360-pixel frames, settling at 720 pixels. Hidden,
replaced, closed and paused previews request no animation loop. Late generations
cannot overwrite a newer selection or reopen Compare. Cameras remain independent
of pose evaluation; initial linked cameras fit default union bounds, and explicit
Fit uses the currently displayed pose bounds. Camera bookmarks do not resume
playback after navigation or restart.

The parser consumes the captured JSON and BIN slices only. Buffer URIs cannot
supply geometry, image URIs are never opened, and opening a preview does not fetch
Git/LFS objects. Verified locally present LFS content enters through the existing
captured-byte path. Each original side keeps its bytes, absence and independent
error state.

`EXT_meshopt_compression` opens directly through this same pipeline. Selected
embedded buffer views are decompressed once and reused by ordinary and sparse
accessors. ATTRIBUTES, TRIANGLES and INDICES modes support their legal NONE,
OCTAHEDRAL, QUATERNION and EXPONENTIAL filters. Quantized accessors still require
`KHR_mesh_quantization`; decoding does not undo the author's quantization or
change transforms, placement or physical units. The same bounded decompressed
views supply supported appearance, skin, morph and animation accessors when selected.

Compressed ranges must refer to the captured BIN buffer. Required-extension
placeholder buffers are accepted at indices above zero with checked fallback
ranges; fallback descriptors may name resources that are never opened. Marked
fallback buffers cannot provide ordinary or compressed data. Supported compressed
views always use their compressed payload; malformed streams never silently fall
back to another representation. Newer bitstream versions outside the EXT
specification are refused explicitly.

POSITION supports core non-normalized FLOAT/VEC3 and signed/unsigned BYTE/SHORT
VEC3 only with required `KHR_mesh_quantization`, including normalized integer
values. Triangle indices are non-normalized unsigned BYTE/SHORT/INT SCALAR;
primitive-restart sentinels are refused. Interleaved positions follow checked
four-byte alignment and legal strides; indices remain tightly packed. Sparse
position/index overrides support a base view or an implicit zero base, with
strictly increasing in-range override indices. Other primitive modes, including
strips, fans, lines and points, are refused instead of skipped. Authored normals
do not affect flat inspection shading; supported UVs and vertex colors follow
the appearance contract above.

The finite validator checks container boundaries, versions, references, tree
structure and layouts needed by the selected geometry and supported resources.
Appearance and animation resource validation is isolated so an optional resource
failure can preserve useful default geometry. It is not a complete glTF validator.

### Specification and dependencies

Implementation was checked against the current official
[Khronos glTF 2.0 specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html),
[extension registry](https://github.com/KhronosGroup/glTF/blob/main/extensions/README.md)
and [mesh quantization extension](https://github.com/KhronosGroup/glTF/blob/main/extensions/2.0/Khronos/KHR_mesh_quantization/README.md)
on September 10, 2026.

The adapter uses `serde` and `serde_json`, already present in the workspace lock,
with bounded preflight and checked local accessor readers. Both use
[MIT OR Apache-2.0 licensing](https://github.com/serde-rs/json/blob/master/Cargo.toml).
The portable Rust [`gltf`/`gltf-json` 1.4.1 candidate](https://github.com/gltf-rs/gltf/blob/master/Cargo.toml)
has the same dual license. Its optional import/resource helpers are unnecessary
here, and its [utility accessor readers](https://github.com/gltf-rs/gltf/blob/master/src/accessor/util.rs)
assume validated data; a separate allocation/range/cancellation boundary would
still be required. Keeping the narrow adapter exposes those checks without adding
image or filesystem import behavior. No native glTF library or new renderer is
bundled.

The meshopt adapter uses pinned [`meshopt` 0.6.2](https://crates.io/crates/meshopt/0.6.2),
whose Rust wrapper is MIT OR Apache-2.0 and whose bundled meshoptimizer 0.25 decoder
is MIT. Its C++11 implementation targets the platform toolchain on macOS and
Linux and supports native SIMD; only supplied byte pointers enter its decoder
and filter calls. The [official extension specification](https://github.com/KhronosGroup/glTF/blob/main/extensions/2.0/Vendor/EXT_meshopt_compression/README.md)
defines modes, filters, fallback buffers and supported bitstreams. The native
decoder does not allocate its output or expose an interruption callback: the
adapter owns checked allocations and checks cancellation immediately before and
after each bounded whole-stream call. Filters and index conversion have additional
block checkpoints. This is cooperative cancellation, not a hard deadline.

The [published wrapper's build](https://github.com/gwihlidal/meshopt-rs/blob/585fe7f5120df13f494bcde7134d8b115fc28213/build.rs)
uses checked-in bindings and the existing C++ toolchain. The evaluated
[pure Rust alternative](https://github.com/yzsolt/meshopt-rs) has MIT licensing,
an older mostly-0.22 implementation and no SIMD or cancellation callback. The
upstream-aligned native decoder was selected for broader current stream coverage
and its maintained reference implementation. Platform source support is separate
from the executed build evidence in the milestone validation record.

## Implemented STEP subset

This is **not general STEP B-rep support**. Files exported as conventional trimmed
`ADVANCED_FACE`/`MANIFOLD_SOLID_BREP` surfaces remain unsupported, even when the
shape happens to look spherical or cylindrical. Accepted curved geometry is
evaluated from analytic CSG entities rather than inferred from faceted examples.

| Area | Implemented contract |
| --- | --- |
| Faceted topology | `FACETED_BREP` → `CLOSED_SHELL` → `FACE` or planar `FACE_SURFACE`, one `FACE_OUTER_BOUND`/`FACE_BOUND`, `POLY_LOOP`, 3D `CARTESIAN_POINT`. Concave planar polygons use bounded ear clipping. No face holes. |
| Analytic solids | A `CSG_SOLID` containing one `SPHERE`, `RIGHT_CIRCULAR_CYLINDER`, `TORUS` or `BLOCK`. Sphere uses its Cartesian center; cylinder/torus use `AXIS1_PLACEMENT`; block uses `AXIS2_PLACEMENT_3D`. Positive finite dimensions are required; torus major radius exceeds minor radius. |
| Curved fidelity | Sphere: 64 longitude segments × 32 latitude bands, 3,968 triangles. Cylinder: 64 angular segments with planar end caps, 256 triangles. Torus: 64 major-ring × 32 tube segments, 4,096 triangles. Block: 12 exact planar triangles. This fixed visual approximation has no user-selected CAD tolerance, surface healing or manufacturing assurance. |
| Mapped instances | `SHAPE_REPRESENTATION`, `FACETED_BREP_SHAPE_REPRESENTATION`, `CSG_SHAPE_REPRESENTATION`, `MAPPED_ITEM` and `REPRESENTATION_MAP`; the target is `CARTESIAN_TRANSFORMATION_OPERATOR_3D`, the mapping origin is `AXIS2_PLACEMENT_3D`. Nested mapping composes target × inverse origin; unplaced definitions are excluded. Positive uniform scale, orthogonal axes, rotation, reflection and translation are supported. |
| Length units | Units are read from each shape representation's `GLOBAL_UNIT_ASSIGNED_CONTEXT`. Complex `LENGTH_UNIT`/`NAMED_UNIT` with `SI_UNIT` supports meter and milli/centi/deci/micro/nano/kilo prefixes. `CONVERSION_BASED_UNIT` supports bounded positive `LENGTH_MEASURE_WITH_UNIT`/`MEASURE_WITH_UNIT` chains to an accepted length unit. Known units convert to mm. Missing units stay explicitly unknown. Mixed or inconsistent representation units are refused. |
| Explicitly unsupported | Trimmed curved B-rep/NURBS, boolean CSG trees, cones, face holes/voids, nonuniform transformations, external references, product-relationship assemblies (`CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`, relationship transformations), PMI, material/color rendering, topology repair and semantic manufacturing validation. |

STEP parsing accepts one supplied text exchange, respects strings/comments and
rejects duplicate IDs, missing references, unsupported complex geometry, mapped
cycles, invalid axes and unreasonable coordinates. Unsupported geometry returns a
side-specific error; it is not replaced with a point cloud, bounding box or partial
assembly.

## Bounds and ownership

Input is at most 32 MiB (STEP 4 MiB). Expanded output is at most 100,000 triangles
(7.2 MB of retained f64 coordinates per scene), 300,000 source vertices and 4,096
objects/instance visits. STEP has at most 40,000 records, 32 KiB per record,
32 syntax/mapping levels, 256 points per faceted polygon and four million polygon
candidate tests. Unit conversion chains stop at eight levels. Existing 3MF ZIP,
XML and decompression limits remain independent of output geometry.

GLB additionally limits JSON to 4 MiB, 250,000 structural tokens, 32 nesting
levels, 16,384 entries per array/object and 64 KiB per string. Duplicate decoded
object keys are refused before typed deserialization. The container has at most
128 chunks. Nodes, meshes, scenes, primitives and selected instance visits each
have a 4,096 cap; node depth is 32. Each accessor has at most 300,000 elements;
cumulative decoded positions and indices are independently limited to 300,000
each, and accessor work including sparse overrides to 900,000 elements. Both
cached source triangles and expanded instance triangles are capped at 100,000.
Unused meshes are not expanded. These limits bound geometry separately from
JSON, decoded appearance/deformation data, retained frames and raster samples.
Parsing and expansion have cooperative checkpoints; bounded library calls are
not a hard deadline or a process-memory sandbox.

Meshopt independently caps each selected view at 16 MiB compressed input, 16 MiB
decompressed output and 300,000 elements. Cumulative touched compressed ranges and
aligned decompressed view storage each stop at 32 MiB. A shared view is decoded
once; temporary view storage is released when geometry preparation completes and
is not retained in `ModelScene`. Index decoding uses an additional temporary u32
array of at most 1.2 MB to detect values that would overflow a two-byte index.
These limits supplement the original captured-input, accessor, triangle,
instance, retained-frame and raster limits rather than replacing them.

Appearance independently caps each decoded image at 4,194,304 pixels and a 4,096
pixel edge; the codec allocation allowance is 32 MiB. Unique linear RGBA32F texture
images and all generated mip levels share a 96 MiB retained budget. Samplers
referencing the same image share storage. Appearance attribute decoding has a
16 MiB cumulative work allowance, including repeated selected source reads;
expanded per-triangle colors/UVs/material indices are separately bounded by the
100,000-triangle cap (about 8 MiB). Up to 32 distinct appearance diagnostics are
retained. Failed image decodes are not repeatedly attempted through other sampler
references. Image codec calls are bounded but not interruptible internally;
cooperative checks surround decoding and run through conversion and mip creation.

Deformation limits are 256 joints per skin, 1,024 joints total, eight morph targets
and eight skin influences per vertex. Decoding allows four million scalar
components and 64 MiB of retained evaluation data, independently of textures and
expanded scene triangles. A requested frame allows eight million charged
vertex/morph/influence operations. Animation allows 64 clips, 256 channels and
samplers per clip, 100,000 cumulative channel keyframes, one million cumulative
animation scalar components (including timestamps/tangents), and timestamp values
within 0–86,400 seconds. Sources without valid animation clips release evaluation
data after preparing the default scene. Animated frames share immutable appearance
and evaluator storage and retain only their current evaluated triangles/bounds.
The application does not cache every animation frame.

Frames have a 64–720 pixel edge, an independent 64-million candidate raster-sample
budget and cooperative cancellation per triangle and scanline block. Wireframe
clips lines before sampling, including at extreme zoom. A solid 720-pixel frame
allocates about 2.1 MB RGBA plus 4.1 MB depth. BLEND additionally allows two million
fragments (about 64 MB), 32 visible transparent layers per pixel, and pixel head
and linear-color buffers (about 8.3 MB at 720 pixels); it explicitly refuses
excess overlap. Temporary evaluated geometry and per-frame vertex/joint arrays
are bounded separately from these pixels and retained textures.

`ModelScene::retained_bytes` uses precomputed appearance/evaluator accounting and
counts current geometry, metadata and winding overrides. Shared storage can be
conservatively counted more than once across retained consumers. Pixel conversion
and GPU lifetime tracking remain in the app worker/presentation layer. The legacy
`decode_model` wrapper prepares four fixed views; its GLB views use the same
appearance renderer with a separate 64-million sample cap per view. Interactive
consumers use `decode_geometry` and render only the requested view. These are
allocation/work bounds, not a process-memory sandbox or hard wall-clock deadline.

## Maintained CAD kernel investigation

The maintained Rust candidate is Truck. Its upstream STEP importer includes
analytic/NURBS surface entities and product-assembly tables; its upstream tests
exercise cone, cylinder, sphere and torus B-rep tessellation. `truck-stepio` and
`truck-meshalgo` declare Apache-2.0 licensing. See the upstream
[STEP entity/import source](https://github.com/ricosjp/truck/blob/master/truck-stepio/src/in/mod.rs),
[curved tessellation tests](https://github.com/ricosjp/truck/blob/master/truck-stepio/tests/input/tessellate_shape.rs)
and [package license declaration](https://github.com/ricosjp/truck/blob/master/truck-stepio/Cargo.toml).

Truck's public tessellation operation takes a tolerance and returns allocated
geometry; it exposes no cancellation callback or output allocation budget. Its
generic shape tessellation collects surfaces before GitTurtle can enforce its
triangle cap. This is an integration constraint found in the
[upstream tessellation API](https://github.com/ricosjp/truck/blob/master/truck-meshalgo/src/tessellation/mod.rs),
not evidence that Truck is unsuitable for every application. A full integration
needs a bounded adapter or separately constrained helper, plus real trimmed-face,
assembly and unit fixtures. It is not introduced as an unbounded in-process parser
in this milestone's passive preview path.

Open CASCADE is the established alternative for STEP and product assemblies. Its
[STEP/XDE guide](https://github.com/Open-Cascade-SAS/OCCT/blob/master/dox/user_guides/step/step.md)
describes geometry and assembly translation; its
[LGPL-2.1 license](https://github.com/Open-Cascade-SAS/OCCT/blob/master/LICENSE_LGPL_21.txt)
and [additional exception](https://github.com/Open-Cascade-SAS/OCCT/blob/master/OCCT_LGPL_EXCEPTION.txt)
require a deliberate native linking, notices/source/relinking and packaging path.
No OCCT binary or binding is bundled here. Neither candidate is labelled integrated.

The implemented analytic subset is maintained in GitTurtle and adds no dependency.
Its entity interpretation was checked against STEP Tools' schema documentation for
[sphere](https://www.steptools.com/stds/stp_aim/html/t_sphere.html),
[cylinder](https://www.steptools.com/stds/stp_aim/html/t_right_circular_cylinder.html),
[torus](https://www.steptools.com/stds/stp_aim/html/t_torus.html),
[mapped item](https://www.steptools.com/stds/stp_aim/html/t_mapped_item.html),
[representation map](https://www.steptools.com/stds/stp_aim/html/t_representation_map.html)
and [3D transformation](https://www.steptools.com/stds/stp_aim/html/t_cartesian_transformation_operator_3d.html).

Behavioral fixtures verify changed revision scale, camera projection/pan/orbit,
wireframe, mm conversion, exact analytic radial coordinates, approximate enclosed
sphere volume, transformed primitive bounds, nested mapped instances, consistent
unit contexts, cancellation, cycles and amplification limits. They establish the
finite implemented contract, not general CAD interoperability.
