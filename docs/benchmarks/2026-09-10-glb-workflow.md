# GLB workflow release measurements — September 10, 2026

On Apple M4 Max (16 logical CPUs, 128 GiB), macOS 26.6.2, Rust 1.98.0,
release preview source `4b19124` integrated in `8908b35` produced the following
supplied-byte results. The [timing manifest](2026-09-10-glb-workflow/timing-manifest.json)
records exact source, harness, executable and input hashes, hardware/load, phase
boundaries and raw filenames. These measurements establish current costs; they
do not claim a speedup or performance on another platform.

## CPU phases and process memory

Each of ten inputs preserved one first attempt, three warmups and 100 measured
rows. Input bytes were already resident; every row decoded a fresh scene. The
first attempt is not a cold-filesystem measurement. Animation samples cycle
through the selected clip at 60 equal intervals, including endpoints. Percentiles
below use nearest rank over the 100 measured rows; no outliers were discarded.

All times below are **p99 milliseconds**. First usable CPU is the per-row sum of
decode and static 720-pixel rendering. It excludes intervening camera fit and
allocation destruction, file I/O, app scheduling, pixel conversion, GPU upload
and OS presentation. Evaluation and rendering are separately timed.

| Fixture | Triangles | Decode | First usable CPU | Pose evaluation | 360 render | 720 render | Peak RSS range (MiB) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| material-unlit | 2 | 0.066 | 2.940 | — | 0.933 | 2.928 | 15.30–15.31 |
| texture-png | 2 | 0.081 | 2.799 | — | 0.946 | 3.259 | 15.47–17.50 |
| texture-jpeg | 2 | 0.117 | 2.830 | — | 0.725 | 2.709 | 15.56–17.66 |
| textured-avocado | 682 | 64.214 | 70.259 | — | 1.881 | 6.750 | 138.97–140.52 |
| skin-animation | 2 | 0.132 | 2.826 | 0.011 | 0.699 | 2.789 | 15.50–17.48 |
| morph-animation | 2 | 0.095 | 2.997 | 0.014 | 0.912 | 3.555 | 15.41–17.42 |
| skin-morph-animation | 2 | 0.127 | 2.939 | 0.010 | 0.785 | 2.678 | 15.48–17.50 |
| rigged-simple | 188 | 0.115 | 2.826 | 0.022 | 1.236 | 2.729 | 15.67–17.66 |
| dense-torus | 65,536 | 2.944 | 8.842 | — | 3.390 | 6.513 | 32.47–32.48 |
| private-asset-0327 | 17,064 | 3.392 | 8.782 | 0.874 | 2.582 | 5.607 | 30.83–32.44 |

The 2048² embedded-PNG Avocado scene retains about 85.44 MiB of scene storage,
primarily linear texture mipmaps. Its process RSS peaks at 138.97–140.52 MiB.
The demanding private captured asset has 17,064 triangles and ten clips; its
per-row evaluation plus 720 render sum is 6.310 ms at p99 (these are summed
phases, not a single end-to-end timer). No-animation evaluation columns are
omitted because those rows only measure branch/timer overhead.

Each RSS range comes from three separate `/usr/bin/time -l` invocations of the
same release example, each with one first and one measured row. Darwin reports
maximum resident set size in bytes. Three usage-only baseline processes consumed
8.625–8.672 MiB; those values are preserved and are not subtracted. Scene and pose
byte estimates share immutable allocations and must not be added together. RSS
includes loader, allocator, input and transient processing allocations, excludes
separate GPU accounting, and is not an application-wide memory cap.

The maintained public Avocado and RiggedSimple inputs and procedural dense torus
are identified by provenance in the manifest. Private asset 0327 remains a local
read-only capture and is identified only by ordinal and hash. Ordinary desktop
load and native QA could overlap; release builds and corpus scans had finished.

## Packaged native frame callbacks

The exact arm64 package UUID `996D52A5-7CF1-3CC5-BA91-928AF6AD0ECC` exercised
the native [workflow](../glb-workflow-validation.md). The captured 17,064-triangle
asset rendered one After side, with the absent Before shown independently. The
playback interval kept all 807 recorded 360-pixel callbacks:

| Native request-to-frame-callback interval | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Demanding captured animation, milliseconds | 6.883 | 14.884 | 15.300 |

This timer starts at model request dispatch and includes lane wait, pose/raster
work, render-image pixel conversion and UI delivery to the GPUI next-frame
callback. It excludes input delivery, captured-source loading/decoding and
completed GPU or OS presentation. Callbacks superseded before delivery are not
logged, so this is not an input-latency, presented-frame-rate or dropped-frame
metric. The [raw native trace](2026-09-10-glb-workflow/native-trace.txt) and
[observations](2026-09-10-glb-workflow/native-observations.json) record the exact
selected line interval and every callback value.

Twenty RSS snapshots over approximately ten seconds of demanding playback were
stable at 206,448 KiB (201.61 MiB), including the previous native interaction and
cache history. Each three-second observation with Settings visible, a different
tab active, or all tabs closed recorded zero new model callbacks. Cached immutable
previews remained after closing tabs; final window closure retired every remaining
image (`retained_images=0`) and exited the process. This short session cannot
establish long-running memory stability.

## Reproduction

```sh
cargo build --release --locked -p gitturtle-preview --example bench_model_animation
target/release/examples/bench_model_animation INPUT.glb 100 3 0 > /tmp/glb-timing.tsv
/usr/bin/time -l target/release/examples/bench_model_animation INPUT.glb 1 0 0 > /tmp/glb-rss-timing.tsv 2> /tmp/glb-rss.txt
cargo build --release --locked -p gitturtle --target-dir target
scripts/package-macos.sh --no-build
GITTURTLE_TRACE=1 dist/GitTurtle.app/Contents/MacOS/gitturtle /tmp/disposable-glb-repository
```

Run the RSS command in three fresh processes per input and retain the first and
warmup timing rows. Use the public fixture generator described in the workflow
report for native revision comparisons. Capture trace line offsets around Play
and Pause/tab transitions; keep the measurement boundary separate from source
loading and GPU completion. The [validation manifest](2026-09-10-glb-workflow/validation-manifest.json)
ties logs and source objects to the exercised release/package. No installation,
Linux work, hosted CI, notarization or release publication is implied.
