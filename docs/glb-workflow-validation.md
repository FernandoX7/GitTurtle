# Native GLB appearance and animation validation

Source `4b19124` adds bounded appearance, deformation and animation evaluation;
`8908b35` adds native playback and comparison controls. The packaged application
at that combined source passed the macOS workflow below. The support contract is
in [interactive 3D](interactive-3d.md#glb-20-appearance-deformation-and-animation),
with separate [independent reference and corpus evidence](glb-workflow-reference.md)
and [release measurements](benchmarks/2026-09-10-glb-workflow.md).

## Supported result

Captured GLBs render linear-light unlit base color, vertex color and embedded
PNG/JPEG, including sampler, alpha and sidedness behavior. Core PBR is explicitly
labelled **Base-color inspection** with simplified flat lighting. Metallic and
roughness response, authored lighting/normals and omitted texture extensions are
not presented as faithful appearance. Malformed or unsupported appearance can
leave useful geometry with a disclosure; malformed deformation does not silently
become undeformed geometry.

Supported default skins and morphs preserve authored placement and units.
Retained clips support node TRS and morph weights, STEP, LINEAR and CUBICSPLINE,
including animated joint transforms. Each side chooses its clip independently;
both share seconds, the longest selected duration loops, and a shorter clip holds
its endpoint. The authored default pose remains selectable. Seeking and clip
changes pause both sides. Camera state stays fixed during playback; Fit uses
displayed pose bounds and Reset restores the initial authored fit.

Source capture, decoding, pose evaluation, rasterization and render-image pixel
conversion stay off the UI thread. The existing single active/replaceable pending
model lane samples playback at up to 30 Hz without cancelling frames to chase the
clock. Hidden, replaced, inactive and closed previews pause and invalidate work.
The [finite allocation and work limits](interactive-3d.md#bounds-and-ownership) cover
textures, deformations, animation data and raster fragments independently of the
32-entry/128-MiB content cache and 512-MiB retained-tab allowance.

## Native macOS observations

One owner exercised the exact local `dist/GitTurtle.app`, launched with
`GITTURTLE_TRACE=1`, on Apple M4 Max / macOS 26.6.2. Only disposable repositories
were changed. The public two-commit repository is reproduced by
[`create-glb-workflow-fixtures.py`](../scripts/create-glb-workflow-fixtures.py).
Its first comparison used before `8dce999` and after `bfaa78b`. The separately
created demanding fixture contained captured private asset ordinal 0327, a
malformed After model and a missing local LFS pointer. Private source bytes and
paths are absent from the versioned evidence.

| Exercised interaction | Observed result |
| --- | --- |
| Material-only revision | Identical geometry appeared orange Before and green After; Working Changes showed the reversed pair from the unstaged material edit. |
| Embedded PNG-only revision | The four-color texture changed on identical geometry; both sides rendered their own captured image. |
| Default deformation and playback | The Before joint-bend skin and After morph retained independent geometry, clips and duration feedback. Play selected usable clips from Default pose. |
| Scrub and clip changes | Mouse dragging, keyboard End paused playback. A 2-second Before clip held a 1-second After clip at its endpoint. The cubic morph clip was selectable and evaluated at its exact endpoint. |
| Cameras | Linked zoom changed both sides; independent zoom changed only Before. Keyboard orbit preserved the shared pose. Fit used current deformed bounds; Reset and relinking remained usable. |
| Navigation | Rapid animated/material/texture activation followed by Back stayed in History. Tab switching paused demanding playback; returning retained the selected clip, camera and paused time. |
| Appearance and narrow layout | Ember at 13-point text and Braden at 13/15 points were inspected. The exact 1000×680 viewport kept playback, both clip selectors, wrapped camera controls and the file inspector reachable. |
| Availability | An added uppercase `.GLB` rendered After with an absent Before. A malformed After displayed its header error while valid Before stayed interactive. Missing LFS explicitly reported that no download was attempted. |
| Reduce Motion | Initial OFF → ON disabled Play and retained manual endpoint seeking. Restoring OFF did not resume playback. The system preference was restored. |
| Closure | Closing an actively playing tab stopped work. Closing the final native window while playing exited the process and retired all eight remaining images. |

The [native observations and raw trace](benchmarks/2026-09-10-glb-workflow/native-observations.json)
retain three-second quiet intervals for Settings, an inactive tab and closed tabs,
each with zero new model callbacks. Immutable preview cache entries can outlive
tab closure; that observation is distinct from the final window-close
`retained_images=0`. These bounded observations do not prove unlimited-duration
memory stability. Accessibility-tree labels, disabled states and keyboard actions
were checked; spoken VoiceOver was not exercised for this build. Other themes,
density combinations and Linux were not revalidated in this goal.

An early accessibility-driven Settings return retained the preceding displayed
frame while the OS window was inactive. Foreground keyboard interaction completed
the requested frame. Subsequent tab restoration delivered the paused pose. This
matches the explicit inactive-window rendering guard; it was not counted as an
active-window latency sample.

## Build identity, gates and restoration

Release and packaged executable UUIDs both equal
`996D52A5-7CF1-3CC5-BA91-928AF6AD0ECC` (arm64). Packaging verified the plist and
local ad-hoc signature; an additional deep/strict signature check passed. Signing
changes file bytes, so the [validation manifest](benchmarks/2026-09-10-glb-workflow/validation-manifest.json)
records both SHA-256 values, source Git objects, harness hashes and sanitized logs.
Compiled inputs remained unchanged after source `8908b35`; later commits contain
validation tooling and documentation.

`cargo fmt --all -- --check`, `cargo check --locked -p gitturtle`, focused tests,
`cargo test --locked --workspace`, and
`cargo clippy --locked --workspace --all-targets -- -D warnings` passed. The final
workspace run had **701 passed, five ignored**; its nested Mermaid subprocess
also passed and is not double-counted. Release app/examples and the local package
built successfully. Final tooling received formatting, syntax, deterministic
fixture, comparator failure-behavior and focused example Clippy checks. The
pre-existing `block 0.1.6` future-incompatibility notice remains a toolchain notice.

The installed application was never replaced. After closing the tested package,
all three saved application-data files were restored byte-for-byte and the
installed app was relaunched. Native inspection confirmed the original three
repository tabs, active repository, History commit and selected file. The original
theme/text preferences and Reduce Motion were restored. Source/corpus integrity
checks found no repository or captured-byte changes. There was no remote push,
publication, private asset upload, automatic LFS download or Linux work.

Remaining format limitations include approximate core PBR appearance, omitted
KTX2/WebP and external resources, no animated material parameters or cameras,
finite joint/morph/key/frame limits, and explicitly refused malformed skin data.
The corpus's oversized morph asset remains bounded; the public morph and combined
skin/morph reference fixtures establish correctness within the supported limit.
