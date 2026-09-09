# Self-contained model fixtures

These tiny tetrahedra were authored for GitTurtle in September 2026. Their four
points are `(0,0,0)`, `(2,0,0)`, `(0,3,0)`, `(0,0,4)`; no third-party artwork or
external model files are included. They follow the repository's license.

- `tetra.stl`: ASCII triangle mesh.
- `tetra.obj`: triangle mesh with a deliberately nonexistent absolute `mtllib`
  reference. Native decoding must never resolve this reference.
- `tetra.fbx`: ASCII FBX 7.4 geometry and a connected mesh node. The loader converts
  FBX axes to Z-up; material, animation and external resource loading are disabled.
- `tetra.3mf`: deflated core model XML and `_rels/.rels`, with deterministic ZIP
  member timestamps. Build items reference only the included model mesh.
- `tetra.step`: a faceted B-rep closed shell, with quoted punctuation and a comment
  containing a fake entity to exercise lexical parsing. Curved B-rep and assembly
  placement are intentionally outside this supported subset.

The decoder tests use the OBJ, FBX and STEP bytes directly and generate additional
binary/ASCII STL, transformed 3MF and refusal fixtures in memory.

For a reproducible pixel artifact from any supported file:

```sh
cargo run --locked -p gitturtle-preview --example render_model -- crates/preview/tests/fixtures/models/tetra.3mf /tmp/gitturtle-model-views
```

This writes four 720-pixel PNGs and prints geometry details. It is a decoder tool,
not evidence that the native app's controls or a packaged build passed. Native
cases should select each format through Quick Open and compare captured old/new
models with different geometry, switch the four named views independently, test
missing/invalid sides, and retain Back/Projects context while decoding.
