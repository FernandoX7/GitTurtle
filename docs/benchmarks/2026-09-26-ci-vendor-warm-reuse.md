# CI vendored-crate warm reuse — September 26, 2026

Evidence for task `ci-vendor-warm-reuse`: keep the vendored GPUI/Mermaid path
packages' compiled outputs in the Rust cache and rewind their checkout times, so a
warm run no longer rebuilds them. The mechanism is described in
[docs/ci.md](../ci.md#vendored-path-packages). This record holds the **before**
baseline and the hosted runs **after** the change (commit `da12ee7`). Each figure
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

Against the hosted runs below: the size projections held (`v2` generation
4.11 GB, repository total 8.02 GB, Linux debug retained 6.21 GB), though the
added compressed size was 0.20 GB rather than 0.25 GB. The Linux test compile fell
104-111 s, inside its range; macOS fell 123-161 s, partly below 140-190 s. The
release binary started 39 s (Linux) and 29 s (macOS) earlier, so macOS fell short
of 35-50 s. Clippy compiled in 15-20 s against 22-41 s before; the before figure
is not per lane, so its 10-20 s projection is not checked precisely.

## After

Job durations in seconds; cache total is the repository total in bytes, against
the same 10 GB limit.

| Observation | Run | Tests · Linux | Tests · macOS | Release · Linux | Release · macOS | Vendor units fresh? | Cache total |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: |
| Pull request carrying the change (new key, cannot save), `048acce` | [36294670841](https://github.com/FernandoX7/GitTurtle/actions/runs/36294670841) | 908 | 1089 | 683 | 550 | n/a (cold) | 3,909,177,414 (`v1` only) |
| Same pull request after merging `main`, `423d0db` | [36295644929](https://github.com/FernandoX7/GitTurtle/actions/runs/36295644929) | 1244 | 1137 | 597 | 708 | n/a (cold) | 3,909,177,414 (`v1` only) |
| Cold after: first `main` push after merge, saves `gitturtle-rust-v2`, `da12ee7` | [36296766425](https://github.com/FernandoX7/GitTurtle/actions/runs/36296766425) | 1291 | 1484 | 513 | 711 | n/a (cold) | 8,019,440,142 |
| Warm after: pull request #48, `e20fc19` on `da12ee7` | [36298515464](https://github.com/FernandoX7/GitTurtle/actions/runs/36298515464) | 190 | 313 | 269 | 390 | yes, both platforms | 8,019,440,142 |
| Vendor-change probe (pull request #49, closed unmerged): one comment line in `vendor/gpui-component/src/lib.rs`, `8afc126` | [36298940285](https://github.com/FernandoX7/GitTurtle/actions/runs/36298940285) | 1245 | 1153 | 622 | 519 | no, rebuilt | 8,019,440,142 |

The cold after runs took 908-1484 s for tests and 513-711 s for release, against
1199-1416 s and 542-704 s before. Pull request runs never save, so the total only
changed at the seed.

### Cache inventory after the seed

Listed 2026-09-27T06:28Z: the four `v2` entries created by 36296766425 beside the
four `v1` entries above. Repository total **8,019,440,142 bytes of the 10 GB
limit**, of which `v2` is 4,110,262,728, 201,085,314 more than `v1`.

| Entry | Lane | Compressed bytes | Retained logical bytes | Retained vendor bytes | Limit bytes |
| --- | --- | ---: | ---: | ---: | ---: |
| 8168714842 | `gitturtle-rust-v2-debug-…-Linux-x64` | 1,416,921,943 | 6,209,553,703 | 333,347,882 | 6,442,450,944 |
| 8168754674 | `gitturtle-rust-v2-debug-…-Darwin-arm64` | 1,044,026,413 | 4,342,155,472 | 266,894,338 | 6,442,450,944 |
| 8168536199 | `gitturtle-rust-v2-release-…-Linux-x64` | 859,919,788 | 2,882,960,744 | 138,197,723 | 3,758,096,384 |
| 8168581559 | `gitturtle-rust-v2-release-…-Darwin-arm64` | 789,394,584 | 2,680,501,052 | 153,365,542 | 3,758,096,384 |

Retained bytes are from 36296766425's budget records, each with
`retained_vendor_packages: 4`, no eviction and `dropped_target: false`. Three lanes
grew by about their vendor bytes over the before figures; macOS debug grew
0.86 GB over the warm `v1` save and 0.44 GB over the `v1` seed's own record
(3,904,199,277), more than its 0.27 GB of vendored outputs. The `v1` entries were
kept because the total stays under the limit; they expire seven days after their
last access (2026-09-27T04:52Z at the latest).

### Restore and rebuilt units

The seed, warm and probe restore records all show `vendor_mtimes: 743` (files and
directories rewound).
The warm run restored the seed's keys (`key_prefix` `debug-bc7e75655f08…`,
`debug-3ebcfae8e630…`, `release-bfb686422e73…`, `release-0b5243ec8731…` for Linux
and macOS debug and release) with `hit=true`, and each job logged `full match:
true`. The probe computed new prefixes (`debug-e1585a1fc5fd…`,
`debug-5ea5f262fd00…`, `release-4e02de99cf13…`, `release-89395ebcd475…`),
reported `hit=false` and logged `No cache found`.

In the warm run the only dirty units were the three workspace members: 39 of 912
(Linux) and 39 of 810 (macOS) in the test step, 40 in Clippy and 5 of 925 or 816
in the release step, against 45/47 and 11/13 before. No vendor unit printed
`Compiling` or `Checking` in the test, Clippy or release logs on either platform,
so warm after passes. The probe rebuilt everything (0 fresh units in its test and
release steps; 776, 647, 635 and 558 `Compiling` lines, 5-7 of them vendored per
lane), so it passes too. Compile time and finish time within the step, in
seconds (fresh = not among the dirty units):

| Run | Lane | Compile total | `gpui-component` | `mermaid-rs-renderer` | `gpui-base` | `gpui-pre-macos` |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 36298515464 | Tests · Linux | 49.9 | fresh | fresh | fresh | — |
| 36298515464 | Tests · macOS | 73.5 | fresh | fresh | fresh | fresh |
| 36298515464 | Release · Linux | 193.2 | fresh | fresh | fresh | — |
| 36298515464 | Release · macOS | 264.5 | fresh | fresh | fresh | fresh |
| 36298940285 | Tests · Linux | 826.1 | 106.5 (ends 796.1) | 105.4 | 77.1 | — |
| 36298940285 | Tests · macOS | 716.9 | 97.9 (ends 672.2) | 94.0 | 68.8 | 11.6 |
| 36298940285 | Release · Linux | 565.8 | 41.3 (ends 327.6) | 30.4 | 33.5 | — |
| 36298940285 | Release · macOS | 447.1 | 42.8 (ends 278.1) | 29.5 | 26.8 | 5.4 |

The warm Clippy compile took 14.8 s (Linux) and 19.7 s (macOS).

### Result

One warm sample against the five before: Tests · Linux 190 s against 271-361 s,
Tests · macOS 313 s against 429-496 s, Release · Linux 269 s against 281-397 s,
and Release · macOS 390 s, inside its 308-525 s range. Pull request #48 changed
`crates/app`, as did the before pull request sample 36025813950; the members are
pruned from the cache and rebuild on every warm run, so both rebuilt the same
member units. The test compile fell from 154-234 s to 50-74 s. In the release
step the binary started 39 s (Linux) and 29 s (macOS) earlier than in
36283728303, but the binary's own compile varied more between the two samples
(241 to 184 s on Linux, 160 to 243 s on macOS), so the macOS release job shows no
gain in this sample.
