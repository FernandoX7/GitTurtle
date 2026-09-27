# CI tests with nextest — September 27, 2026

Evidence for task `ci-nextest-tests`: Quality's Rust debug jobs run unit and
integration tests with the pinned cargo-nextest 0.9.146 and the `ci` profile,
followed by a separate doctest step. The change is described in
[docs/ci.md](../ci.md#test-execution-with-nextest). Rows marked **TODO** are
filled from hosted runs; nothing in them is estimated. Each figure is one
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
| **TODO** before sample on the same base as the after runs | | | ubuntu-24.04 | TODO | TODO | TODO | TODO |
| **TODO** before sample on the same base as the after runs | | | macos-15 | TODO | TODO | TODO | TODO |

The 36164627195 rows come from jobs 108169412431 (Linux) and 108169412536 (macOS):
step timestamps from `gh run view`, and compile, harness and test counts from
`metrics.py cargo_timings` and `executed_tests.py list` over each raw job log.
The macOS count is lower because some tests are platform-conditional. A preview
test re-executes its own binary and prints its result line twice; the extracted
set counts it once.

## After: `cargo nextest run` and doctests (warm)

| Run | Event | Head | Platform | Install | Nextest step | Doctests step | Tests total | Compile | Harness | Executed / ignored |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| **TODO** | | | ubuntu-24.04 | TODO | TODO | TODO | TODO | TODO | TODO | TODO |
| **TODO** | | | macos-15 | TODO | TODO | TODO | TODO | TODO | TODO | TODO |

## Test-list comparison

| Platform | Before job | After job | Executed only before | Executed only after | Ignored only before | Ignored only after |
| --- | --- | --- | --- | --- | --- | --- |
| ubuntu-24.04 | **TODO** | **TODO** | TODO | TODO | TODO | TODO |
| macos-15 | **TODO** | **TODO** | TODO | TODO | TODO | TODO |

Acceptance needs every difference column empty on both platforms, and the
doctest step's `Doc-tests` sections counted on the after side.

## Failure readability

**TODO**: link a hosted or local failing-test log showing the immediate
`FAIL` line with the test's output and the non-zero step result, or state that
no failing run was produced.
