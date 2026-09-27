# CI vendored-crate warm reuse — September 26, 2026

Evidence for task `ci-vendor-warm-reuse`: keep the vendored GPUI/Mermaid path
packages' compiled outputs in the Rust cache and rewind their checkout times, so a
warm run no longer rebuilds them. The mechanism is described in
[docs/ci.md](../ci.md#vendored-path-packages). This record holds the **before**
baseline; the after rows are placeholders until the hosted runs exist. Each figure
is one observation, not a distribution.

## Measurement boundary

- **Job duration**: the job's `startedAt` to `completedAt` from
  `gh run view <run> --json jobs`, including setup, restore, tests or packaging and
  the post-job cache action.
- **Compile**: the `Total time` of the step's `cargo --timings` report in the run's
  `ci-diagnostics-*` artifact (`tests-timing.txt` or `release-timing.txt`).
- **Vendor units**: `gpui-component` (three units), `gpui-base`,
  `mermaid-rs-renderer`, macOS `gpui-pre-macos`, and the registry crates that
  depend on them (`gpui-kit`, macOS `gpui-pre-platform`).
- **Cache sizes**: the cache service's `sizeInBytes` from `gh cache list`; the
  repository total from `gh api repos/FernandoX7/GitTurtle/actions/cache/usage`,
  against the 10 GB repository limit.
- **Retained bytes**: the helper's pre-registration logical payload from
  `rust-cache-budget.json` (not a compressed size).

## Before: warm runs (exact cache hit)

Job durations in seconds.

| Run | Event | Head | Tests · Linux | Tests · macOS | Release · Linux | Release · macOS |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| [36283728303](https://github.com/FernandoX7/GitTurtle/actions/runs/36283728303) | push `main` | `81b150d` | 308 | 429 | 364 | 308 |
| [36173968561](https://github.com/FernandoX7/GitTurtle/actions/runs/36173968561) | push `main` | `827cee1` | 271 | 471 | 328 | 392 |
| [36164627195](https://github.com/FernandoX7/GitTurtle/actions/runs/36164627195) | push `main` | `92118f9` | 361 | 496 | 281 | 329 |
| [36161885923](https://github.com/FernandoX7/GitTurtle/actions/runs/36161885923) | push `main` | `ac7a3a8` | 316 | 490 | 364 | 525 |
| [36025813950](https://github.com/FernandoX7/GitTurtle/actions/runs/36025813950) | pull request | `e1f7fe3` | 299 | 450 | 397 | 485 |

On every warm run 45 (Linux) or 47 (macOS) units rebuilt in the test step and
11 or 13 in the release step: the vendor units above and the three workspace
members. Vendor unit compile time and finish time within the step, in seconds:

| Run | Lane | Compile total | `gpui-component` | `mermaid-rs-renderer` | `gpui-base` | `gpui-pre-macos` |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 36283728303 | Tests · Linux | 161.1 | 116.2 (ends 129.0) | 82.0 | 68.9 | — |
| 36283728303 | Tests · macOS | 196.7 | 128.6 (ends 152.0) | 76.8 | 65.5 | 11.7 |
| 36164627195 | Tests · Linux | 154.0 | 110.7 (ends 122.5) | 85.5 | 72.2 | — |
| 36164627195 | Tests · macOS | 234.2 | 157.6 (ends 194.0) | 105.2 | 92.2 | 17.0 |
| 36025813950 | Tests · Linux | 160.9 | 117.7 (ends 130.7) | 80.0 | 83.6 | — |
| 36025813950 | Tests · macOS | 217.8 | 144.8 (ends 171.0) | 95.2 | 83.6 | 11.8 |
| 36283728303 | Release · Linux | 289.5 | 36.2 (ends 48.7) | 32.8 | 23.2 | — |
| 36283728303 | Release · macOS | 210.7 | 30.3 (ends 51.0) | 27.5 | 23.9 | 5.8 |

Clippy rebuilt the same units in check mode; its whole compile took 22-41 s.

## Before: cold runs (cache miss)

| Run | Event | Head | Tests · Linux | Tests · macOS | Release · Linux | Release · macOS |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| [36009569684](https://github.com/FernandoX7/GitTurtle/actions/runs/36009569684) | push `main` (seeded the current entries) | `72916b0` | 1261 | 1416 | 704 | 542 |
| [35965873026](https://github.com/FernandoX7/GitTurtle/actions/runs/35965873026) | pull request | `7042c54` | 1209 | 1199 | 639 | 564 |

## Before: cache inventory

Listed 2026-09-27T01:14Z: four entries, all on `refs/heads/main`, created by
36009569684 and still read by every warm run. Repository total
**3,909,177,414 bytes of the 10 GB limit**.

| Entry | Lane | Compressed bytes | Retained logical bytes | Limit bytes |
| --- | --- | ---: | ---: | ---: |
| 8074320648 | `gitturtle-rust-v1-debug-…-Linux-x64` | 1,358,962,960 | 5,875,158,754 | 6,442,450,944 |
| 8074438753 | `gitturtle-rust-v1-debug-…-Darwin-arm64` | 981,228,359 | 3,477,504,715 | 6,442,450,944 |
| 8073897489 | `gitturtle-rust-v1-release-…-Linux-x64` | 820,825,879 | 2,744,004,230 | 3,758,096,384 |
| 8073776085 | `gitturtle-rust-v1-release-…-Darwin-arm64` | 748,160,216 | 2,526,422,857 | 3,758,096,384 |

Retained logical bytes are from 36283728303's budget records (main, key unchanged
since the seed).

## Projection (not measured)

- **Size**: the vendored rlib/rmeta files measure about 0.33-0.36 GB per debug
  lane (test and check builds) and 0.14 GB per release lane on a local Linux
  build. At the observed 0.23-0.30 archive-to-logical ratios that is about
  0.25 GB compressed per generation, so a `v2` generation of about 4.15 GB and
  about 8.1 GB while `v1` remains. Linux debug retained bytes rise to about
  6.2 GB of its 6.44 GB limit.
- **Time**: replaying each step's `--timings` dependency graph with the vendor
  units at zero cost suggests about 100-115 s off the Linux test compile,
  140-190 s off macOS, 10-20 s off Clippy and 35-50 s off each release build. The
  replay assumes unlimited parallelism, so the real saving will be smaller.

## After (TODO)

| Observation | Run | Tests · Linux | Tests · macOS | Release · Linux | Release · macOS | Vendor units fresh? | Cache total |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: |
| Pull request carrying the change (new key, cannot save) | TODO | TODO | TODO | TODO | TODO | n/a (cold) | TODO |
| Cold after: first `main` push after merge, saves `gitturtle-rust-v2` | TODO | TODO | TODO | TODO | TODO | n/a (cold) | TODO |
| Warm after: next pull request that routes to the Rust jobs | TODO | TODO | TODO | TODO | TODO | TODO | TODO |
| Vendor-change probe (never merged): one-line vendor edit | TODO | TODO | TODO | TODO | TODO | expected rebuilt | TODO |

For each after run, record the four `v2` entries' compressed sizes, the budget
records' `retained_bytes` and `retained_vendor_bytes`, the restore records'
`key_prefix` and `vendor_mtimes`, and the rebuilt units. Warm after passes when
none of the vendor units above print `Compiling` in the test, Clippy or release
logs on either platform. The probe passes when its `key_prefix` differs from the
warm run's, its restore reports `hit=false`, and the vendor units compile. The
`v1` entries are left to expire after seven days without access; delete them
earlier only if the repository total approaches its limit.
