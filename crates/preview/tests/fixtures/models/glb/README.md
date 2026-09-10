# GLB fixtures and provenance

These files exercise captured-byte static geometry inspection. They are not
application assets and must not be packaged with the executable. GLB appearance
is intentionally omitted: all supported models render with GitTurtle's neutral
surface shading, independent of their stored materials, textures and normals.

## Synthetic comparison and refusals

The seven lowercase files were authored for GitTurtle in September 2026 and follow
the repository license. Their deterministic source is
[`scripts/create-glb-preview-fixtures.py`](../../../../../../scripts/create-glb-preview-fixtures.py).
The generator uses only Python's standard library and never downloads resources.

| File | Expected behavior |
| --- | --- |
| `assembly-before.glb` | An arch made from three instances of one cube mesh. Each cube has two indexed triangle primitives; the beam has a nested parent translation. 36 triangles in the selected scene. |
| `assembly-after.glb` | The same arch, scaled by 1.5 and translated +1.5 meters on X at the root. These changes must remain visible with initial linked Before/After cameras. |
| `external-buffer.glb` | A buffer URI points to `https://example.invalid/do-not-fetch.bin`. Refuse the external buffer without network or filesystem resource access. |
| `skinned.glb` | A mesh node references a skin. Refuse unsupported skin evaluation. |
| `morph-target.glb` | A primitive contains a position morph target. Refuse unsupported morph evaluation. |
| `compressed.glb` | Required `KHR_draco_mesh_compression`; refuse unsupported compression. |
| `truncated.glb` | Thirteen bytes removed from the valid Before container. Report malformed/truncated input. |

The arch is authored in glTF's right-handed Y-up coordinates and meters. Applying
`(x, y, z) -> (1000*x, -1000*z, 1000*y)` preserves handedness and converts it to
GitTurtle's Z-up millimeters. Expected retained scene bounds, with floating-point
tolerance for composed transforms:

| Scene | Minimum (mm) | Maximum (mm) |
| --- | --- | --- |
| Before | `[-1200, -300, 0]` | `[1200, 300, 2400]` |
| After | `[-300, -450, 0]` | `[3300, 450, 3600]` |
| Linked-camera union | `[-1200, -450, 0]` | `[3300, 450, 3600]` |

Create a new disposable repository outside the checkout:

```sh
python3 scripts/create-glb-preview-fixtures.py /tmp/gitturtle-glb-fixture > /tmp/gitturtle-glb-fixture-manifest.json
```

The first commit contains the original GLB and a matching existing-format OBJ
arch. HEAD moves and scales both, adds `added.GLB`, deletes `deleted.glb` and adds
the refusal cases. `deleted.glb` contains the official BoxInterleaved bytes so
Git's rename detection keeps it distinct from the added truncated arch. The
manifest includes the resulting history name/status list with rename detection.
The unstaged area has a changed `assembly.glb`, a deleted
`working-deleted.glb` and an untracked `working-added.GLB`. Official checked-in GLBs
are copied into the first commit for Quick Open. `dense-torus.glb` is generated
only inside the disposable repository: 32,768 float positions and 65,536 indexed
triangles, with 32-bit indices, for geometry expansion and raster measurements.
Its major radius is 1 meter and minor radius 0.35 meters; it has no textures.
The printed manifest records revision IDs, working status, file sizes and SHA256.
Git configuration and hooks are isolated from the user's configuration.

## Official Khronos samples

Captured unchanged from
[KhronosGroup/glTF-Sample-Assets](https://github.com/KhronosGroup/glTF-Sample-Assets/tree/90d7ede14c7e280af263824604b427a1ca02cb66)
on September 10, 2026, at revision
`90d7ede14c7e280af263824604b427a1ca02cb66`.
[`provenance.json`](provenance.json) records immutable original GLB, metadata and
license URLs, attribution, sizes and SHA256. Individual upstream license notices
are preserved next to the files. No GLB bytes were modified.

| Asset | Attribution and license | Size | Purpose |
| --- | --- | ---: | --- |
| [BoxInterleaved.glb](https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/90d7ede14c7e280af263824604b427a1ca02cb66/Models/BoxInterleaved/glTF-Binary/BoxInterleaved.glb) | © 2017 Cesium, [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/legalcode); [upstream notice](BoxInterleaved.LICENSE.md) | 1,632 bytes | 12 triangles, interleaved position/normal attributes and node transforms. |
| [Avocado.glb](https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/90d7ede14c7e280af263824604b427a1ca02cb66/Models/Avocado/glTF-Binary/Avocado.glb) | Microsoft, 2017, [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/legalcode); [upstream notice](Avocado.LICENSE.md) | 8,110,040 bytes | 682 triangles and embedded appearance data; a larger input with modest geometry. Texture bytes are never decoded by the model preview. |
| [RiggedSimple.glb](https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/90d7ede14c7e280af263824604b427a1ca02cb66/Models/RiggedSimple/glTF-Binary/RiggedSimple.glb) | © 2017 Cesium, [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/legalcode); [upstream notice](RiggedSimple.LICENSE.md) | 15,104 bytes | Representative skin and animation. Static inspection must explicitly refuse skin evaluation instead of displaying unevaluated geometry. |

The copied upstream metadocumentation is CC-BY-4.0 as declared in each notice.
The above attribution and license links apply to those notices as well. No
endorsement by the creators or Khronos is implied.

| GLB | SHA256 |
| --- | --- |
| BoxInterleaved | `b2ae631f118f1d13f829cdf9d9dc0fe7cb582de20b8c51d17f81f77a1cbf290c` |
| Avocado | `ccc9c3ce56423720b09399c2351537207cd5a65f859f9e6e2f30922762f3abd4` |
| RiggedSimple | `3a79dabb67bb0cd598a18d08b954d9d357c27c30672f82ef5d3f4e7fe6ca3401` |

## Meshopt fixtures and independent codec reference

`meshopt-arch-before.glb` and `meshopt-arch-after.glb` are lossless compressed
derivatives of the authored assembly pair above. They retain the same 36
triangles, shared mesh instances, nested transforms and bounds. Their shared
index view uses TRIANGLES; FLOAT positions use ATTRIBUTES with NONE. Both require
`EXT_meshopt_compression` and use its absent fallback-buffer convention.

`meshopt-avocado.glb` is a geometry-only derivative of Microsoft's CC0 Avocado
above: 682 triangles with the original positions, indices, node placement and
meter scale. Texture/material/normal/UV payloads were removed, then position and
index views were compressed losslessly. The original license and attribution
still apply; it is a modified asset, not an unchanged upstream sample.

[`meshopt-provenance.json`](meshopt-provenance.json) records every source/output
hash, encoder/decoder module hash, immutable upstream URL, runtime version and
decoded buffer expectation. The fixture encoder
[`encode-meshopt-glb.cjs`](../../../../../../scripts/encode-meshopt-glb.cjs)
uses the official MIT-licensed [meshoptimizer 0.25 JavaScript/WASM modules](https://github.com/zeux/meshoptimizer/tree/6daea4695c48338363b08022d2fb15deaef6ac09/js),
separate from GitTurtle's Rust/FFI adapter. It verifies POSITION bytes exactly and
each decoded triangle's cyclic vertex identity and winding before writing a GLB.
It does not quantize or reorder geometry. Fixture generators never download
resources; supply the two pinned upstream modules locally. Module hashes are
checked before loading. These modules are fixture tooling and are not packaged
with GitTurtle.

```sh
node scripts/encode-meshopt-glb.cjs /tmp/meshoptimizer-js crates/preview/tests/fixtures/models/glb/assembly-before.glb /tmp/meshopt-arch-before.glb
python3 scripts/create-meshopt-preview-fixtures.py /tmp/gitturtle-meshopt-fixture --upstream-js /tmp/meshoptimizer-js > /tmp/gitturtle-meshopt-manifest.json
```

The disposable History/Working fixture includes the changed arch, added/deleted
compressed models, a useful Before with malformed After, invalid compressed
bytes, and existing deformation/Draco/external-buffer refusals. It also includes
compressed and uncompressed versions of the same dense 65,536-triangle torus
and Avocado, with preserved placement. Local LFS cases provide exact compressed
bytes, an absent object, a corrupt object and a comparison with only one locally
resolvable side; no LFS commands or downloads are used. Its stdout manifest
records refs/index/working hashes and local LFS-object hashes. Keep that manifest
outside the watched fixture. The generator refuses an existing destination.

[`probe_models`](../../../../examples/probe_models.rs) reports actual decoding
and independent 360-pixel solid, 720-pixel solid and 720-pixel wireframe outcomes
for explicitly supplied files. Each outcome remains distinct: decoding success
does not imply every requested raster fits the raster budget. Its optional
`--triangles-directory` export writes the complete retained triangle sequence as
little-endian f64 coordinates for an independent geometry comparison. Store
private inputs, path manifests, exports and results outside this repository.

```sh
cargo build --release --locked -p gitturtle-preview --example probe_models
target/release/examples/probe_models /tmp/gitturtle-meshopt-fixture/assembly.glb
```

## Release measurement harness

[`bench_model`](../../../../examples/bench_model.rs) measures `decode_geometry`
separately from three `render_model` calls: a 360-pixel solid frame, a 720-pixel
solid frame and a 720-pixel all-edge wireframe. The harness requires release mode:

```sh
cargo build --release --locked -p gitturtle-preview --example bench_model
target/release/examples/bench_model crates/preview/tests/fixtures/models/glb/Avocado.glb 30 3 > /tmp/gitturtle-avocado-bench.tsv
target/release/examples/bench_model /tmp/gitturtle-glb-fixture/dense-torus.glb 30 3 > /tmp/gitturtle-torus-bench.tsv
```

Input bytes are read once before timing and remain resident. Each row decodes a
fresh retained scene without an application cache and allocates a fresh frame
for each render. One first-attempt row and all warmups are retained separately
from measured rows. Every measured row is preserved; nearest-rank p50, p95 and
maximum summarize only measured rows. File I/O, camera fitting, output destruction,
app scheduling, native interaction, GPUI, GPU upload and OS presentation are
outside these CPU timings. The first attempt is not a cold-filesystem benchmark.
`scene_retained_bytes` is scene accounting, not process peak memory.

Record the source revision plus dirty diff identity, executable SHA256, compiler,
hardware/OS, other system load and fixture SHA256 alongside the raw TSV. Keep
records outside the watched disposable repository. Report these as backend
measurements, separate from actual native interaction evidence.
