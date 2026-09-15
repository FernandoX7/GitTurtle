# Local refresh reliability: reproduction and component evidence

This records filesystem-watcher and core-policy evidence from 2026-09-14. It does **not** measure latency or establish native UI behavior, frame pacing, an installed binary, or macOS coverage.

## Source and environment

The baseline watcher was extracted with `git show 7d18fef:crates/app/src/local_refresh.rs`. Its SHA-256 was `cafbf2c91697653041933f5553511fe5d1cbd30f834be4bd4558029b1073cc18`.

The initial correction landed in `d9f64c3`. The final source recorded here includes the parent-sentinel fix in commit `417b5e8d0c739ca3746d2cab379db58efa4d8ea9`:

| Input | SHA-256 |
| --- | --- |
| `crates/app/src/local_refresh.rs` | `e447fe8dcaf425cc47fb49d0d1c2153c391a867fd0ed16f86e33bb81fc6f1144` |
| `crates/git-core/src/local_watch.rs` | `bc0049f9144c8cd9f0689169e18b3f4db9561740ee8ebd025b3ca1905888ee02` |

Host: Linux `7.1.5-76070105-generic`, x86_64; Git 2.43.0; Rust 1.98.0 (`88d9e12ae`, 2026-08-18). The temporary harnesses used real `notify` 7.0.0/inotify and development builds. No cold-cache or hardware-isolation procedure was used.

## Reproduced causes

A read-only `os.walk(..., followlinks=False)` census of this checkout at the start of the work found 21,710 directories. These counts describe that observation, not a permanent fixture:

| Subtree | Directories |
| --- | ---: |
| `.local` | 17,328 |
| `target` | 3,727 |
| `vendor` | 345 |
| `.git` | 215 |

`git ls-files -z` returned 1,369 paths, occupying 67,551 output bytes. `git ls-files --others --exclude-standard -z` returned no paths. The checkout's ignore rules excluded `.local` and `target`, but the baseline watcher traversed them and applied its 16,384-directory limit before any Git-aware pruning.

Temporary binaries called core repository discovery, `git_directories()`, and the baseline/current `watch(WatchRoots { ... })` against the same checkout. Only passive discovery and native watch registration targeted the checkout. The observed outputs were:

```text
baseline watcher failed: Watch <checkout>: Automatic refresh reached the local directory-watch limit. Use Refresh to check all current changes.
Current watcher subscribed. Initial coverage report: None
```

`<checkout>` replaces the local path in the first output. The current harness checked `changes.next().now_or_never()` immediately after subscription; `None` means no initial coverage warning was queued, not that every possible later event was tested.

The missing-directory failure also had concrete unhandled paths: the baseline propagated errors from `symlink_metadata`, `read_dir`, directory-entry type inspection, and native registration while directories could disappear between those operations. Focused fixtures exercised a directory removed before registration, a directory replaced by a file, and real native notification delivery during 150 create/write/remove cycles. Those cases completed without race warnings after the fix.

A later regression reproduced a distinct failure in the initial correction: parent-directory recovery watches also received events for unrelated siblings. Directory creation/rename attempted to register those siblings using the worktree ignore policy, producing `Watch path is outside the selected worktree` and a false coverage warning. Creating and renaming a `linked-peer` beside the selected linked worktree, and a `build-peer` beside the common `.git` directory, reproduced the failure in both a direct registration test and real Linux notifications. Both tests failed before `417b5e8` and passed after it. They use disposable paths rather than unrelated user repositories.

## Behavior and bounds

Linux registers private/common administration roots and essential refs before spending the remaining watch budget on worktree directories. Git's index supplies byte-safe tracked ancestors. Nested `.gitignore`, common `info/exclude`, and effective global excludes determine pruning; tracked files inside ignored directories retain coverage. Directory symlinks are not traversed. Other platforms retain native recursive subscriptions.

Creation and rename events add affected subtrees. Parent-sentinel events are classified against the selected repository before filesystem inspection or policy evaluation; unrelated sibling directories are skipped, and worktree eligibility defensively rejects paths outside that root. Unrelated removals also skip cache pruning, while removal of a registered sentinel still releases its watches. Coalesced index changes add newly tracked ancestors and release formerly tracked directories that have become ignored. Ignore-file edits reconsider affected coverage; global excludes/configuration changes and replaced repository roots reconcile broader coverage. Removal releases watches and cached ignore matchers, including empty matchers and their source-byte accounting. Essential existing watches survive partial coverage failures.

There is no periodic tree poll. Freed registrations retry remembered omitted roots once; explicit Refresh or focus regain can retry degraded coverage. Ordinary quiet Git refreshes recheck root identity without repeatedly rebuilding an unchanged degraded watcher. Lost events permit one bounded reconciliation per continuous burst.

| Resource | Bound |
| --- | --- |
| Queued native events / paths per event | 64 / 16 |
| Fingerprints | 4,096 |
| Linux registered directories | 16,384 |
| Enumeration pass | 200,000 entries or a cooperative two-second stop |
| Remembered omitted roots | 256 |
| Tracked paths | 100,000 |
| Tracked path/ancestor source bytes | 16 MiB |
| Cached ignore matchers | 16,384 |
| Individual / aggregate ignore-file source bytes | 1 MiB / 16 MiB |
| Event coalescing | 250 ms quiet period; two-second maximum burst delay |

These are component/input bounds, not a process RSS cap. Core subprocesses retain their existing passive-command deadlines and bounded output handling. A cooperative stop can finish its current filesystem/matcher operation before returning.

Warning ownership is separate from Git operation failures. Partial coverage uses a persistent footer status with Refresh and copyable Details; a recovered coverage report supersedes an older queued warning. Native presentation and interaction still require their own validation.

## Executed checks and reproduction

The following host checks passed during implementation:

```sh
cargo check --locked -p gitturtle --tests
cargo test --locked -p gitturtle-core --test local_directories
cargo clippy --locked -p gitturtle-core -p gitturtle --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Three core directory/policy tests initially passed together. The added temporary-cache regression subsequently passed on its own: 17,000 discarded directory matchers and repeated reads/evictions of a 1 MiB ignore file. Core fixtures also checked linked-worktree administration ownership, tracked files beneath ignored directories, nested rules, common/global exclusions, and unchanged index bytes after passive policy reads.

A normal host app-test link failed because the host lacked the unversioned `libxkbcommon-x11` development library. To exercise the watcher without GPUI, temporary crates outside the checkout loaded the actual `local_refresh.rs` using a Rust `#[path = "..."]` module and depended on the local core crate, `anyhow`, `futures`, and `notify = "=7.0.0"`. Commands used this form:

```sh
CARGO_TARGET_DIR=<checkout>/target cargo test --manifest-path <temporary-harness>/Cargo.toml --offline -- --test-threads=1
CARGO_TARGET_DIR=<checkout>/target cargo run --manifest-path <temporary-harness>/Cargo.toml --offline --bin watch-current -- <checkout>
```

Fourteen watcher tests passed together, including 16,386 ignored directories, constrained coverage preserving refs, cleanup recovery, ignore edits, force-added tracked ignored paths, burst delivery, fingerprint bounds, and real native notifications. Two later watcher regressions passed individually: private Git-root replacement with a completed index, and tracked-to-ignored directory release. These temporary harnesses resolved their own offline dependency lockfiles; they are supporting component evidence, not a substitute for the application's final locked suite.

The two sibling regressions subsequently passed together in 2.24 seconds in that harness. The registration test asserts zero enumerated sibling entries, an unchanged watch set and no diagnostic through create/rename/remove operations at both parent locations. The native test asserts no refresh delivery from sibling churn, then confirms worktree and private Git-metadata notifications without degradation. The existing private Git-root replacement regression also passed after the routing change.

The maintained application regressions can be run in the established complete Linux build environment with:

```sh
cargo test --locked -p gitturtle local_refresh::tests -- --test-threads=1
cargo test --locked -p gitturtle automatic_refresh::tests
cargo test --locked -p gitturtle-core --test local_directories
```

## Final locked validation after the sentinel correction

The coordinator ran the final combined checks in the established Ubuntu build root after `417b5e8`:

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

| Suite | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Application | 367 | 0 | 2 |
| Git core, unit and integration suites | 241 | 0 | 2 |
| Preview | 121 | 0 | 1 |
| Total | **729** | **0** | **5** |

All 18 `local_refresh::tests` and all four core `local_directories` tests passed, including both sibling regressions. The core and preview doc-test suites contained no tests. The preview suite separately spawns a one-test subprocess; its successful invocation is present in the log but excluded from the total above to avoid counting the same parent test twice. The five ignored tests cover installed `gh` isolation, an opt-in review benchmark, GnuPG signing, local `sshd` authentication and the SVG mutation probe. They are not reported as executed coverage. Clippy completed successfully with warnings denied.

The unchanged [final workspace test log](native-preview-20260914/final-workspace-tests.log) and [final Clippy log](native-preview-20260914/final-workspace-clippy.log) are retained with their SHA-256 hashes in the [evidence manifest](native-preview-20260914/manifest.json). These supersede the earlier check logs for final suite counts; they do not change the build identity of the earlier native scroll captures. No tests were rerun to update this document.

All mutation fixtures used disposable repositories. No native-window actions, package installation, compositor behavior, hosted CI, or unchecked platform are claimed by this report.
