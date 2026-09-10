# Interactive 3D comparison and finite CAD support

The September 2026 implementation retains immutable triangles for STL, OBJ, FBX,
3MF and the STEP subset below. `decode_geometry` consumes supplied bytes;
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

## Coordinates, units and source

3MF core build transforms and nested components are expanded before unit
conversion. Supported source units are micron, millimeter, centimeter, inch, foot
and meter; display coordinates use millimeters. FBX node placements are applied,
axes normalize to right-handed Z-up and declared units normalize to millimeters.
When FBX omits its unit setting, the FBX library's centimeter default is disclosed.
FBX remains a static default-pose mesh. OBJ and STL preserve source coordinates;
neither establishes physical units. A camera link between unknown and known units
does not establish physical equivalence.

All parsers remain supplied-byte operations. No geometry, material, texture,
external cache, script or repository-relative path is resolved. Exact source and
captured-byte identity remain with each original side. Neither retained triangles
nor rendered frames can become a staging patch.

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

Frames have a 64–720 pixel edge, a separate 64-million raster-sample budget and
cooperative cancellation per triangle and scanline block. Wireframe clips lines
before sampling, including at extreme zoom. A solid 720-pixel frame allocates about
2.1 MB RGBA plus 4.1 MB depth; it does not retain a second projected geometry copy.
`ModelScene::retained_bytes` accounts for coordinates and metadata. Pixel conversion
and GPU lifetime tracking stay in the app worker/presentation layer. The legacy
`decode_model` wrapper still prepares four fixed views for existing static consumers;
interactive consumers use `decode_geometry` to avoid that work.

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
