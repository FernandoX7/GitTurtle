# Validation notes

This records the v0.1 validation performed on September 7, 2026. Native interaction was exercised on macOS. Linux is a portability target, not a validated release platform. These checks are local; no CI or notarized distribution is claimed.

## Automated and build checks

The completed workspace run passed **57 tests**, strict workspace Clippy, and a release build. Tests use disposable repositories for operations that create Git objects, refs, or worktrees.

```sh
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --release --locked -p gitturtle
```

Coverage includes repository roots and merge parents, branches and linked worktrees, unusual path bytes, binary/text/mode/type changes, SHA-256 repositories, missing partial-clone objects, local LFS integrity and symlink rejection, Git process deadlines, and repository-file snapshots with hostile configured helpers. Preview tests cover decoding limits, alpha handling, SVG resource rejection, and LFS pointer recognition. App tests exercise queue replacement, stale-work cancellation, cache identity and accounting, BGRA conversion, local LFS arrival, refreshed branch/worktree tips, and graph budgets.

A local macOS `.app` can be built with [the packaging script](../scripts/package-macos.sh). It is signed ad-hoc for local use, not notarized. The script's `--debug` option packages a debug build, while the default uses release; `--no-build` reuses the selected profile's existing executable.

## Native macOS checks

The application was opened against the locally available `world-of-claudecraft` repository containing **5,577 branches and 130 worktrees**, and against a generated demonstration repository.

| Area | Exercised behavior |
| --- | --- |
| Repository navigation | Large branch/worktree lists, local/remote branches, native folder picker, invalid-repository errors, and draggable history/sidebar dividers |
| Commit inspection | Changed-file selection, explicit merge-parent comparisons, locked-worktree display, and selected-commit preservation while expanding history |
| Text | Unified patch and Before/After views; typing did not edit the preview; text selection and copying worked |
| Images | Modified, added, and deleted PNGs; transparent pixels; linked zoom and vertical panning |
| Unavailable content | Binary file information and missing local LFS image messages |

Horizontal wheel panning did not visibly respond to the CUA test; full two-axis gesture behavior remains unverified.

These are manual checks of the current native build, not an exhaustive platform or accessibility certification. The [design document](design.md) includes intended behavior beyond the implemented surface.

To create a fresh disposable demonstration repository:

```sh
python3 scripts/create-demo-repo.py
cargo run --release --locked -p gitturtle -- .local/demo-repository
```

The generator also creates a linked worktree. It refuses nonempty destinations, including existing repositories. For another run, choose a fresh destination with `--output /path/to/empty-or-new-directory`; do not point it at a working repository.

## Timing methodology

Initial backend observations, hardware/toolchain details, and the reproducible inspection command are recorded in the [Git service benchmark notes](../crates/git-core/README.md#initial-measurement-september-7-2026). The backend harness measures Git service work. It excludes UI dispatch, queuing, image decode, editor preparation, rendering, and presentation. Filesystem caches were not flushed, so those observations are not cold-disk results.

The native app has a separate optional trace:

```sh
GITTURTLE_TRACE=1 target/release/gitturtle /path/to/repository
```

`gitturtle.selection_frame_ms` starts in the application selection handler. For a commit selection, it includes reading the changed-file list and preparing the chosen file preview; a direct file selection starts at that file's handler. The value is emitted at a GPUI frame-completion callback after the current preview is prepared, with a generation check to suppress superseded results. It does not measure input delivery before the handler, OS display presentation, or completed GPU execution. Interactions without a completed preview do not produce this sample.

The status bar's **content read** duration covers worker processing, including a content-cache lookup on a hit. It excludes time waiting for the worker and subsequent editor construction or frame work. Comparing this value directly with another client's click-to-visible delay would be misleading.

Native trace samples should be reported with the build profile, repository, selected content, sample count, system load, and cache conditions. No frame-latency or memory guarantee is established by the current checks.

## Initial optimized native measurements

On Apple M4 Max / macOS 26.6.2, the release build at `8b0799e` opened `world-of-claudecraft` with 500 loaded commits. We sent 25 Down actions and then 25 Up actions, observing the native accessibility state after each. The OS filesystem cache was not flushed; other desktop applications and development activity remained running. This is a small first measurement, not a comparative benchmark or latency guarantee.

| Completed preview samples | Count | Median | 95th percentile | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Navigation, excluding initial selection | 49 | 21.882 ms | 29.838 ms | 308.598 ms |
| Return traversal (subset above) | 25 | 21.910 ms | 27.176 ms | 27.216 ms |

The initial selection callback took 72.441 ms; that excludes repository discovery and history loading and is not application startup time. Fifty navigation actions emitted 49 completed-preview samples; absent samples are not counted as zero latency. The 308.598 ms outlier is retained, and this trace does not isolate its cause. The raw samples, method, and limits are in [the measurement record](benchmarks/2026-09-07-native.json).

Process RSS snapshots were about 142.3 MiB after the outbound traversal and 143.4 MiB after the return. These snapshots exclude GPU memory accounting and do not establish a memory-growth guarantee. The locally packaged app occupies about 30 MiB on disk.

## Known limits and follow-up

- The UI loads 500 commits initially and adds 500 per **Load more**, up to 10,000. It reloads the full prefix and restores the selected commit/preferred file when possible, centering the history selection. It does not retain an incremental history traversal cursor. Search covers loaded commits.
- Refresh is manual and resolves the selected scope's current tip. Another tool is responsible for fetches. Missing objects are reported without automatic downloads.
- Text uses a unified patch plus Before/After tabs. There is no aligned split view or dual old/new line-number gutter. The UI's initial file list omits rename detection, displaying additions and deletions instead.
- Images use a first-frame preview capped at a 1,600-pixel edge. Zoom percentages refer to that decoded preview; original dimensions and reduced preview dimensions are shown. SVG filters and embedded/external images are explicitly unsupported.
- Graph preparation allows at most 128 simultaneous lanes, 200,000 edges, and 200,000 parent entries. Above the budget, the UI explains why connections are hidden and shows isolated nodes instead of incomplete ancestry lines.
- The preview cache is limited to 32 entries and 128 MiB of retained CPU content allocations. UI-held references and GPU resources have separate lifetimes. Active decoder work stops at cooperative checkpoints; input and allocation limits are not a process sandbox or a hard end-to-end deadline.
- Native Linux builds and interactions, Linux packaging, automated CI, and notarized macOS distribution have not been validated or provided.
