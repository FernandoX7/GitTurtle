# CI tests with nextest — September 27, 2026

Evidence for task `ci-nextest-tests`: Quality's Rust debug jobs run unit and
integration tests with the pinned cargo-nextest 0.9.146 and the `ci` profile,
followed by a separate doctest step. The change is described in
[docs/ci.md](../ci.md#test-execution-with-nextest). Every row comes
from hosted runs; nothing in them is estimated. Each figure is one
observation, not a distribution.

## Measurement boundary

- **Tests step**: the step's `startedAt` to `completedAt` from
  `gh run view <run> --json jobs`. Before: `Locked workspace tests and doctests`
  (compilation, unit and integration tests, doctests). After: `Locked workspace
  tests with nextest` plus `Locked workspace doctests`, reported separately and as
  their sum. `Install pinned, checksum-verified cargo-nextest` is reported on its own.
- **Harness**: `test_harness_seconds` from the step's `tests.json` or
  `doctests.json` in the `ci-diagnostics-*` artifact, or `metrics.py
  cargo_timings` over the step's log. Before, it sums libtest's per-binary
  `finished in` times; after, it is nextest's `Summary` wall-clock duration.
- **Compile**: `cargo_compilation_seconds` from the same record (Cargo `Finished` lines).
- **Warm**: the setup action's restore record shows an exact cache hit
  (`rust-cache-restore.json`), for both before and after samples.
- **Test lists**: `python3 scripts/ci/executed_tests.py compare --before <cargo test
  job log> --after <nextest job log>` per platform, using
  `gh api repos/FernandoX7/GitTurtle/actions/jobs/<job-id>/logs`.

## Before: `cargo test --locked --workspace --timings` (warm)

Durations in seconds.

| Run | Event | Head | Platform | Tests step | Compile | Harness | Executed / ignored |
| --- | --- | --- | --- | ---: | ---: | ---: | --- |
| [36164627195](https://github.com/FernandoX7/GitTurtle/actions/runs/36164627195) | push `main` | `92118f9` | ubuntu-24.04 | 215 | 154 | 59.39 | 958 / 5 |
| [36164627195](https://github.com/FernandoX7/GitTurtle/actions/runs/36164627195) | push `main` | `92118f9` | macos-15 | 383 | 234 | 146.10 | 952 / 5 |
| [36298515464](https://github.com/FernandoX7/GitTurtle/actions/runs/36298515464) | pull request #48 | `e20fc19` | ubuntu-24.04 | 111 | 49.86 | 59.75 | 959 / 5 |
| [36298515464](https://github.com/FernandoX7/GitTurtle/actions/runs/36298515464) | pull request #48 | `e20fc19` | macos-15 | 234 | 73.00 | 158.61 | 953 / 5 |
| [36299802049](https://github.com/FernandoX7/GitTurtle/actions/runs/36299802049) | pull request #48 | `4df94c3` | ubuntu-24.04 | 108 | 48.89 | 57.72 | 959 / 5 |
| [36299802049](https://github.com/FernandoX7/GitTurtle/actions/runs/36299802049) | pull request #48 | `4df94c3` | macos-15 | 224 | 71.00 | 149.69 | 953 / 5 |
| [36300198277](https://github.com/FernandoX7/GitTurtle/actions/runs/36300198277) | push `main` | `87aed41` | ubuntu-24.04 | 91 | 37.94 | 52.08 | 959 / 5 |
| [36300198277](https://github.com/FernandoX7/GitTurtle/actions/runs/36300198277) | push `main` | `87aed41` | macos-15 | 143 | 47.47 | 94.29 | 953 / 5 |

The 36164627195 rows come from jobs 108169412431 (Linux) and 108169412536 (macOS):
step timestamps from `gh run view`, and compile, harness and test counts from
`metrics.py cargo_timings` and `executed_tests.py list` over each raw job log.
The two #48 pull-request runs are the like-for-like warm samples: pull-request
runs restore `main`'s cache, compile the three workspace crates and run the same
tests as #50, whose base `87aed41` is #48's squash. `main` run 36300198277 on
`87aed41` itself is the test-list reference; its runners were faster (Linux
compiled in 38 s against 49 s), so it is not used for the speed comparison.
The macOS count is lower because some tests are platform-conditional. A preview
test re-executes its own binary and prints its result line twice; the extracted
set counts it once.

## After: `cargo nextest run` and doctests (warm)

| Run | Event | Head | Platform | Install | Nextest step | Doctests step | Tests total | Compile | Harness | Executed / ignored |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| [36300227546](https://github.com/FernandoX7/GitTurtle/actions/runs/36300227546) | pull request | `c9ab3fd` | ubuntu-24.04 | 1 | 108 | 108 ¹ | 216 | 47.60 + 107.00 | 56.64 | 959 / 5 |
| [36300227546](https://github.com/FernandoX7/GitTurtle/actions/runs/36300227546) | pull request | `c9ab3fd` | macos-15 | 1 | 183 | 3 | 186 | 55.55 + 1.17 | 119.33 | 953 / 5 |
| [36321021870](https://github.com/FernandoX7/GitTurtle/actions/runs/36321021870) | pull request | `ca15131` | ubuntu-24.04 | 1 | 106 | 33 ² | 139 | 49.94 + 31.79 | 52.70 | 959 / 5 |
| [36321021870](https://github.com/FernandoX7/GitTurtle/actions/runs/36321021870) | pull request | `ca15131` | macos-15 | 1 | 195 | 76 ² | 271 | 58.75 + 72.00 | 127.28 | 953 / 5 |
| [36321657988](https://github.com/FernandoX7/GitTurtle/actions/runs/36321657988) attempt 1 | pull request | `fc4916c` | ubuntu-24.04 | 1 | 113 | 2 | 115 | 50.53 + 0.56 | 58.19 | 959 / 5 |
| [36321657988](https://github.com/FernandoX7/GitTurtle/actions/runs/36321657988) attempt 1 | pull request | `fc4916c` | macos-15 | 1 | 215 | 5 | 220 | 62.00 + 2.40 | 141.83 | 953 / 5 |
| [36321657988](https://github.com/FernandoX7/GitTurtle/actions/runs/36321657988) attempt 2 | pull request | `fc4916c` | ubuntu-24.04 | 1 | 109 | 1 | 110 | 48.84 + 0.49 | 56.01 | 959 / 5 |
| [36321657988](https://github.com/FernandoX7/GitTurtle/actions/runs/36321657988) attempt 2 | pull request | `fc4916c` | macos-15 | 1 | 126 | 3 | 129 | 38.54 + 1.58 | 80.91 | 953 / 5 |

Compile is the nextest step's plus the doctest step's Cargo `Finished` time. Every
sample, before and after, restored its platform's debug cache with a full match
(keys `debug-bc7e7565…` on Linux and `debug-3ebcfae8…` on macOS). Attempt 2 of
36321657988 is a rerun of the same commit for a fourth sample.

1. `cargo test --doc` selects library targets only. On Linux the app's bin-only
   dependencies then leave the unit graph, `quote`, `syn` and the proc-macro
   crates are no longer shared between host and target, and Cargo rebuilt about
   30 of them with build-override debug information. Fixed by `f5179f5`: the step
   keeps the default selection and filters on the doctest name suffix.
2. The step recompiled the app: `crates/app/build.rs` watches `.git/packed-refs`,
   which the shallow checkout lacks, and Cargo reruns a build script whose watched
   file is missing. Fixed by `fc4916c` (`git pack-refs --all --no-prune`).

## Result

With both fixes (36321657988, two attempts), tests plus doctests took 115 s and
110 s on Linux against 111 s and 108 s before, and 220 s and 129 s on macOS
against 234 s and 224 s. The nextest step alone took 108, 106, 113 and 109 s on
Linux and 183, 195, 215 and 126 s on macOS over all four samples.

- **Linux is flat within run-to-run noise.** Compile (48.8-50.5 s against 48.9-49.9 s)
  and harness (52.7-58.2 s against 57.7-59.8 s) match. nextest adds about 3 s of
  step time beyond compile and harness (listing the 24 test binaries), and the
  doctest step 1-2 s. The two before samples already differ by 3 s, and a `main`
  run of the same code took 91 s.
- **macOS is faster.** Its harness fell from 150-159 s to 81-142 s: the runner
  has 3 CPUs, `cargo test` ran the binaries one after another, and nextest keeps
  all three busy (summed per-test time 425 s over a 141.8 s run in attempt 1).
- **Where the time goes** (36321657988 attempt 1, nextest per-test durations): on
  Linux, 4 CPUs run at a parallelism of 3.97. The app binary's 548 GPUI tests sum
  to 123 of 231 s with a median of 14 ms each, so per-process startup does not
  dominate; the longest test, `theme_editor::tests::the_side_column_shows_a_scrollbar_only_while_it_overflows`
  (12.5 s), bounds the tail. On macOS the Git-spawning core tests dominate
  (`worktrees_reflog` 58 s over 26 tests, 2.2 s each against 0.5 s on Linux), and
  the app binary sums to 113 s with a median of 24 ms. Core count, not GPUI
  startup, sets the harness time on both.

## Test-list comparison

| Platform | Before job | After job | Executed only before | Executed only after | Ignored only before | Ignored only after |
| --- | --- | --- | --- | --- | --- | --- |
| ubuntu-24.04 | 108566379644 (36300198277) | 108566456164, 108624822672, 108626626510, 108627927492 | none | none | none | none |
| macos-15 | 108566379585 (36300198277) | 108566456175, 108624822716, 108626626505, 108627927426 | none | none | none | none |

Each after job was compared with `executed_tests.py compare` against the `main`
job of the same code: `identical`, 959 executed and 5 ignored on Linux, 953 and 5
on macOS. The workspace has no doctests yet (`Doc-tests gitturtle_core` and
`gitturtle_preview` each run 0), so the doctest sections add nothing to either side.

Acceptance needs every difference column empty on both platforms, and the
doctest step's `Doc-tests` sections counted on the after side.

## Failure readability

No hosted failing run was produced. Locally, a disposable crate with one passing,
one failing and one ignored test, run with this repository's `.config/nextest.toml`
(`cargo nextest run -P ci --no-fail-fast`, cargo-nextest 0.9.145, one release
before the pinned 0.9.146), printed `SKIP` and `PASS` lines, then the `FAIL` line
immediately followed by the test's captured stdout (its own `println!` line) and
stderr (the assertion message with both values). The `Summary` repeated the
failing test, `error: test run failed` followed, and the command exited 100,
which fails the step under `bash -e`.
