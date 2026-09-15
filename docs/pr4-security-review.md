# PR #4 security review

This review accounts for the **80 initially open CodeQL alerts** last observed on
main commit `c188020b065f7fcd40f1f53b12d2104fd972cf77`. Source-to-sink traces from
the main analyses for that commit were reviewed for every alert: Rust analysis
`1776210120` and Python analysis `1776141988`.

The inventory contains six `rust/command-line-injection`, 45
`rust/path-injection`, and 29 `py/path-injection` alerts. Review identified four
reproduced defects in installer and test code. Runtime watcher metadata and build
compiler selection were also reviewed.

**Disposition at this checkpoint:** 47 fixture alerts await analysis of the
changed code; 33 alerts have reviewed false-positive rationales with dismissals
not yet applied. This document does not establish a resolved dashboard, successful
new hosted checks, or merge readiness. Fresh analysis and final dispositions
remain part of PR validation.

## Reproduced defects and changes

All reproductions used disposable files, repositories or synthetic bundles.

| Defect and observed failure | Change | Regression |
| --- | --- | --- |
| A permissive umask created the watcher fixture directory with mode `0777`, allowing peer users to alter fixture descendants. | Create and retain a private `TempDir` with Unix mode `0700` before constructing descendants; preserve canonical paths for temporary-directory aliases. | `fixture_directory_is_private_and_resolves_temporary_directory_aliases` |
| Inherited `GIT_DIR`, `GIT_WORK_TREE` and `GIT_INDEX_FILE` redirected `git -C selected add` into a different repository. | Centralize fixture commands; remove inherited Git environment overrides and use fixed identity, hooks and template settings. | `fixture_git_cannot_redirect_writes_into_another_repository` verifies the unrelated index remains unchanged and the selected fixture receives the staged file. |
| Replacing an installed license directory with a symlink made rollback overwrite an unrelated `LICENSE` file. | `owned_target` rejects symlink components below the selected HOME/XDG roots before restoration changes any file. Explicitly selected root aliases remain supported. | `test_restore_rejects_symlink_parent_before_changing_any_file` preserves both the unrelated file and current executable; `test_user_selected_data_root_can_be_a_symlink` preserves supported root selection. |
| A wildcard icon payload copied an unchecksummed extra icon over an unrelated desktop icon, outside the recovery target list. | Enumerate only the fixed application icon names and sizes already required by `SHA256SUMS` and included in recovery targets. | `test_unlisted_icon_cannot_replace_an_unrelated_desktop_icon` verifies the unrelated icon remains unchanged. |

The Rust security fixes are confined to the `cfg(test)` module in
[`local_refresh.rs`](../crates/app/src/local_refresh.rs). Installer changes and
regressions are in [`install-linux.py`](../scripts/install-linux.py) and
[`test-install-linux.py`](../scripts/test-install-linux.py). No CodeQL rule or
file exclusion was introduced.

The installer runs with the invoking user's permissions. Its component checks
reject preexisting redirections; they do not establish an atomic filesystem
boundary against concurrent processes controlling the same account's directories.

## Original alert trust boundaries

These groups describe the actual reported sources and sinks. The four defects
above are tracked separately from whether a reported source grants unintended
path or command authority.

| Group | Count | Reviewed boundary and validation |
| --- | ---: | --- |
| **F — fixture paths and commands** | 47 | The original source is `temp_dir()`, followed by an exclusively created generated leaf, canonicalization, and fixed or numeric descendants. Forty-two sinks access fixture paths. Five command sinks use literal `git` and structured arguments: canonical absolute paths are values for `-C` or `worktree add`, so they cannot introduce an option or choose an executable. The permission and inherited-environment defects above required fixes despite this limited reported input. These alerts await fresh analysis. |
| **W — watcher metadata** | 3 | `watch()` requires absolute roots, canonicalizes them and verifies directories. Registration inspects metadata and rejects symlink directories; event handling classifies repository scope before relevance checks. Alerts 338 and 366 also propagate the result of `normalize()`'s `fs::metadata` validation as a source, although that result never becomes path bytes. Alert 339's reported flows originate in fixture callers. The three sinks read metadata for registration, relevance or availability; they perform no content read, repository write or subprocess execution. |
| **C — Cargo compiler** | 1 | `build.rs` executes the Cargo-selected `RUSTC` with the fixed argument `--version`. Compiler selection is trusted build configuration already executed by Cargo to compile the crate. There is no application repository input or shell parsing. This is intentional executable selection, as documented by [Cargo](https://doc.rust-lang.org/cargo/reference/environment-variables.html). |
| **I — installer root** | 28 | The standalone installer obtains `XDG_DATA_HOME` from its invoking user and requires an absolute root. Operations use fixed application suffixes, generated temporary names or validated recovery entries. Recovery restricts owned destinations, rejects traversal and symlinks, bounds manifest/marker reads, and checks stored-file containment and hashes before restoration. Selecting a local installation root is an intended capability; the separately reproduced component and payload defects were corrected. |
| **L — benchmark log** | 1 | The analyst supplies local log filenames as command-line arguments. The script reads those selected logs and prints numeric summaries, with no filesystem writes, command execution or service requester. |

The installer entries below distinguish source reads, replacement destinations,
cleanup, ownership checks, backup validation and recovery state. They are not a
blanket exemption for installer code or environment variables.

## Validation completed for these fixes

- `cargo test --locked -p gitturtle local_refresh::tests -- --test-threads=4`
  passed **20 tests** in a local Ubuntu environment, including both new fixture
  isolation regressions and existing filesystem notification fixtures.
- `python3 scripts/test-install-linux.py` completed **9 passing tests and 2
  skipped tests**. The skipped tests require an explicit real bundle.
- `git diff --check` passed for the changed security files.

These results establish targeted local validation. Full workspace checks,
real-bundle installation, native UI behavior, macOS execution and fresh hosted
security analysis are separate validation items.

## Initial alert inventory

Locations below refer to analyzed main commit `c188020`, before these fixes;
line numbers may differ in the PR. Distinct IDs on the same line can identify
separate source or destination arguments.

- **Pending**: recheck after analysis of the fixture hardening.
- **FP review**: source-to-sink review supports a false-positive dismissal;
  dismissal has not been applied at this checkpoint.

| Alert | Original path:line | Boundary / operation | Disposition |
| --- | --- | --- | --- |
| 292 | `docs/benchmarks/native-preview-20260914/analyze-scroll.py:14` | L: selected log read | FP review |
| 293 | `scripts/install-linux.py:51` | I: replacement source read | FP review |
| 294 | `scripts/install-linux.py:56` | I: generated replacement source | FP review |
| 295 | `scripts/install-linux.py:56` | I: owned replacement destination | FP review |
| 296 | `scripts/install-linux.py:58` | I: temporary-file cleanup | FP review |
| 297 | `scripts/install-linux.py:62` | I: validated source hash read | FP review |
| 298 | `scripts/install-linux.py:124` | I: destination type check | FP review |
| 299 | `scripts/install-linux.py:122` | I: destination symlink check | FP review |
| 300 | `scripts/install-linux.py:124` | I: destination existence check | FP review |
| 301 | `scripts/install-linux.py:134` | I: license-tree existence check | FP review |
| 302 | `scripts/install-linux.py:135` | I: installed-license enumeration | FP review |
| 303 | `scripts/install-linux.py:142` | I: backup-root creation | FP review |
| 304 | `scripts/install-linux.py:143` | I: generated backup directory | FP review |
| 305 | `scripts/install-linux.py:149` | I: backup-source existence check | FP review |
| 306 | `scripts/install-linux.py:151` | I: backup-source permissions | FP review |
| 307 | `scripts/install-linux.py:158` | I: failed-backup cleanup | FP review |
| 308 | `scripts/install-linux.py:164` | I: manifest symlink check | FP review |
| 309 | `scripts/install-linux.py:164` | I: manifest size check | FP review |
| 310 | `scripts/install-linux.py:166` | I: bounded manifest read | FP review |
| 311 | `scripts/install-linux.py:178` | I: stored-source symlink check | FP review |
| 312 | `scripts/install-linux.py:178` | I: stored-source containment | FP review |
| 313 | `scripts/install-linux.py:178` | I: backup containment anchor | FP review |
| 314 | `scripts/install-linux.py:210` | I: recovery-marker type check | FP review |
| 315 | `scripts/install-linux.py:212` | I: recovery-marker symlink check | FP review |
| 316 | `scripts/install-linux.py:212` | I: recovery-marker size check | FP review |
| 317 | `scripts/install-linux.py:214` | I: bounded recovery-marker read | FP review |
| 318 | `scripts/install-linux.py:248` | I: application-data creation | FP review |
| 319 | `scripts/install-linux.py:249` | I: fixed installation lock | FP review |
| 320 | `scripts/install-linux.py:334` | I: unused-backup cleanup | FP review |
| 322 | `crates/app/src/local_refresh.rs:1458` | F: fixture file write | Pending |
| 323 | `crates/app/src/local_refresh.rs:1148` | F: fixture file write | Pending |
| 324 | `crates/app/src/local_refresh.rs:1439` | F: fixture file write | Pending |
| 325 | `crates/app/src/local_refresh.rs:1481` | F: fixture file write | Pending |
| 326 | `crates/app/src/local_refresh.rs:1606` | F: fixture file write | Pending |
| 327 | `crates/app/src/local_refresh.rs:1607` | F: fixture file write | Pending |
| 328 | `crates/app/src/local_refresh.rs:1639` | F: fixture file write | Pending |
| 329 | `crates/app/src/local_refresh.rs:1644` | F: fixture file write | Pending |
| 330 | `crates/app/src/local_refresh.rs:1658` | F: fixture file write | Pending |
| 331 | `crates/app/src/local_refresh.rs:1659` | F: fixture file write | Pending |
| 332 | `crates/app/src/local_refresh.rs:1713` | F: fixture file write | Pending |
| 333 | `crates/app/src/local_refresh.rs:1714` | F: fixture file write | Pending |
| 334 | `crates/app/src/local_refresh.rs:1513` | F: fixture file write | Pending |
| 335 | `crates/app/src/local_refresh.rs:1567` | F: fixture file write | Pending |
| 336 | `crates/app/src/local_refresh.rs:1571` | F: fixture file write | Pending |
| 337 | `crates/app/src/local_refresh.rs:1574` | F: fixture file write | Pending |
| 338 | `crates/app/src/local_refresh.rs:711` | W: registration metadata | FP review |
| 339 | `crates/app/src/local_refresh.rs:750` | W: event relevance metadata | FP review |
| 340 | `crates/app/src/local_refresh.rs:1443` | F: fixture rename | Pending |
| 341 | `crates/app/src/local_refresh.rs:1678` | F: fixture rename | Pending |
| 342 | `crates/app/src/local_refresh.rs:1402` | F: fixture rename | Pending |
| 343 | `crates/app/src/local_refresh.rs:1671` | F: fixture rename | Pending |
| 344 | `crates/app/src/local_refresh.rs:1402` | F: fixture rename | Pending |
| 345 | `crates/app/src/local_refresh.rs:1443` | F: fixture rename | Pending |
| 346 | `crates/app/src/local_refresh.rs:1671` | F: fixture rename | Pending |
| 347 | `crates/app/src/local_refresh.rs:1678` | F: fixture rename | Pending |
| 348 | `crates/app/src/local_refresh.rs:1486` | F: fixture directory creation | Pending |
| 349 | `crates/app/src/local_refresh.rs:1479` | F: fixture directory creation | Pending |
| 350 | `crates/app/src/local_refresh.rs:1113` | F: fixture directory creation | Pending |
| 351 | `crates/app/src/local_refresh.rs:1394` | F: fixture directory creation | Pending |
| 352 | `crates/app/src/local_refresh.rs:1438` | F: fixture directory creation | Pending |
| 353 | `crates/app/src/local_refresh.rs:1510` | F: fixture directory creation | Pending |
| 354 | `crates/app/src/local_refresh.rs:1526` | F: fixture directory creation | Pending |
| 355 | `crates/app/src/local_refresh.rs:1565` | F: fixture directory creation | Pending |
| 356 | `crates/app/src/local_refresh.rs:1605` | F: fixture directory creation | Pending |
| 357 | `crates/app/src/local_refresh.rs:1638` | F: fixture directory creation | Pending |
| 358 | `crates/app/src/local_refresh.rs:1657` | F: fixture directory creation | Pending |
| 359 | `crates/app/src/local_refresh.rs:1712` | F: fixture directory creation | Pending |
| 360 | `crates/app/src/local_refresh.rs:1445` | F: fixture rename | Pending |
| 361 | `crates/app/src/local_refresh.rs:1640` | F: fixture cleanup | Pending |
| 362 | `crates/app/src/local_refresh.rs:1417` | F: fixture rename | Pending |
| 363 | `crates/app/src/local_refresh.rs:1511` | F: fixture cleanup | Pending |
| 364 | `crates/app/src/local_refresh.rs:1542` | F: fixture cleanup | Pending |
| 365 | `crates/app/src/local_refresh.rs:1744` | F: fixture file check | Pending |
| 366 | `crates/app/src/local_refresh.rs:936` | W: root availability metadata | FP review |
| 367 | `crates/app/build.rs:66` | C: selected compiler executable | FP review |
| 368 | `crates/app/src/local_refresh.rs:1117` | F: Git -C directory argument | Pending |
| 369 | `crates/app/src/local_refresh.rs:1613` | F: Git -C directory argument | Pending |
| 370 | `crates/app/src/local_refresh.rs:1665` | F: Git -C directory argument | Pending |
| 371 | `crates/app/src/local_refresh.rs:1718` | F: Git -C directory argument | Pending |
| 372 | `crates/app/src/local_refresh.rs:1128` | F: absolute worktree argument | Pending |
