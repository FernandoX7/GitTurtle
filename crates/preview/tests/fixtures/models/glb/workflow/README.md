# Public GLB workflow fixtures

These 18 small GLBs were authored for GitTurtle and use the repository license.
They contain no third-party model geometry. The deterministic source is
[`create-glb-workflow-fixtures.py`](../../../../../../../scripts/create-glb-workflow-fixtures.py).
[`manifest.json`](manifest.json) records each exact file hash. The generator uses
Python's standard library, never downloads resources, and refuses an existing
destination. The 2×2 JPEG was encoded once from authored red/green/blue/white
pixels by sharp 0.35.3, quality 100, 4:4:4; its exact bytes are embedded in the
generator so reproduction needs no JPEG encoder.

| Fixture | Authored behavior |
| --- | --- |
| `material-before.glb`, `material-after.glb` | Identical geometry, UVs and indices; only the linear unlit base-color factor changes from red/orange to green. |
| `texture-before.glb`, `texture-after.glb` | Identical geometry, sampler and material; only captured 2×2 PNG texels and their encoded byte lengths change. |
| `texture-jpeg.glb` | Captured 2×2 JPEG; nearest unlit samples independently decoded by sharp. |
| `vertex-color.glb` | Normalized U8 RGBA vertex colors; color channels are linear. OPAQUE ignores vertex alpha for coverage. |
| `alpha-blend.glb`, `alpha-mask.glb` | Linear alpha blending over the declared preview background, and a threshold that distinguishes PNG alpha 127 from 128. |
| `double-sided.glb` | Explicit double-sided unlit panel. |
| `sampler-repeat.glb`, `sampler-mirror.glb`, `sampler-clamp.glb` | Identical texels and UV range 0–2.5 with the three core wrapping modes. |
| `skin-animated.glb` | Two-joint weighted skin, inverse bind matrices, scene translation, deliberately irrelevant mesh translation, animated joint translation and quaternion rotation. |
| `morph-animation.glb` | Node weight overrides mesh weight in the authored default pose; separate linear, cubic-spline and step morph clips. |
| `skin-morph-animation.glb` | Morph positions first, then evaluate the same animated weighted skin. |
| `trs-animation.glb` | Nested placement with linear translation/rotation/scale; separate cubic translation and step translation clips with differing durations. |
| `malformed-texture.glb` | PNG bytes declared as JPEG: retain geometry with an explicit appearance warning. |
| `external-texture.glb` | Deliberately forbidden image URL: retain geometry without opening the URL. |

All panels are XY planes in glTF meters with counterclockwise front faces toward
+Z. GitTurtle's Front view looks at that side after conversion to Z-up
millimeters. Only `KHR_materials_unlit` fixtures claim exact appearance; the
production decoder separately discloses its simplified core-PBR inspection.

[`authored-expectations.json`](authored-expectations.json) contains complete
expanded-vertex expectations derived from closed-form skin, morph, TRS, slerp,
and duration-scaled Hermite equations. These expectations do not read the GLB
parser or production implementation. The
[independent reference procedure](../../../../../../../docs/glb-workflow-reference.md)
compares these equations to Three.js and then compares every exported production
coordinate, not just bounds or triangle counts.

```sh
python3 scripts/create-glb-workflow-fixtures.py /tmp/glb-workflow-assets
python3 scripts/create-glb-workflow-fixtures.py /tmp/glb-workflow-history --repository > /tmp/glb-workflow-history-manifest.json
```

The disposable repository's two commits contain `material.glb` and `texture.glb`
appearance-only revision pairs. `animated.glb` compares a two-second skin clip
with morph clips of differing names and durations. An added uppercase `added.GLB`
contains TRS clips. The final unstaged `material.glb` modification exercises the
Working Changes path. Git identity, hooks and signing are isolated inside the
fixture. No source repository is modified; the complete manifest is also retained
inside the fixture's `.git` directory.
