# Linux validation of the next milestone checkpoint

Latest completed gate: **`58f09ff690e0dbd863a8c29d6699640248d0a136` passed Linux/aarch64 formatting, all 533 workspace tests, strict Clippy and the release build** (three opt-in tests ignored). [Final candidate evidence](#final-candidate-58f09ff) follows the preserved earlier checkpoint records below.

Executed September 9, 2026 against commit `0a0fb5a2f42379743baedf6d6ab0c98f38c1d49a`. Linux/aarch64 formatting, workspace tests, strict Clippy and release gates passed without source changes or an overlay.

## Source and environment

The source was exported with `git archive --format=tar 0a0fb5a2f42379743baedf6d6ab0c98f38c1d49a`; archive SHA-256 is `135b414ad3ada7d22012bfa5eaee231f7ee263b2ddba9626aea616bb496596cb`. `git get-tar-commit-id` independently confirmed its commit identity. Extracted files were mounted read-only at `/source`. Results apply to this checkpoint, including GitHub collaboration, repository tabs/history retention, PDF/Markdown/model work and the disabled/Tab/focus-trap accessibility patches; they do not establish coverage of subsequent edits.

The existing Ubuntu 24.04/aarch64 image was `gitturtle-linux-qa-20260909:043f562`, inspected ID `sha256:869f420928739dea3f57a1ee3dee459783f659de00e3045c57dcfe8efe2332f6`. The container reported Linux `7.0.12-linuxkit aarch64`, Rust `1.98.0 (88d9e12ae 2026-08-18)`, Cargo `1.98.0 (797e8a9bc 2026-08-05)` and Git `2.43.0`.

Each gate used a separate ephemeral container with a read-only root filesystem, all capabilities dropped, no-new-privileges, six CPUs, 12 GiB, and a writable 4 GiB `/tmp`. Only the existing task-owned cargo/target volumes were writable, at `/qa-cargo` and `/qa-target`. `CARGO_HOME=/qa-cargo`, `CARGO_TARGET_DIR=/qa-target`, `GIT_CONFIG_NOSYSTEM=1`, and `GIT_CONFIG_GLOBAL=/dev/null` isolated the build and fixture environment. Locked public dependency preparation had network access; all later gates used `--network none`. No existing user container or repository was changed.

## Executed gates

| Exact command inside the container | Result | Wall time |
| --- | --- | --- |
| `cargo fetch --locked` | Passed | 0.774 s |
| `cargo fmt --all -- --check` | Passed | 1.517 s |
| `cargo test --locked --offline --workspace --no-fail-fast` | 522 unique tests passed; zero failed; three ignored | 72.857 s |
| `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | Passed | 17.026 s |
| `cargo build --release --locked --offline -p gitturtle` | Passed | 70.100 s |

Times include container startup and compilation using retained caches. They are not application latency measurements or performance comparisons.

The total is **522 unique passing tests**: app 245, core unit 23, core integration 209, preview 45. The Mermaid isolation fixture emits a one-test child summary; it is counted once. Doc-test suites contained zero tests. The three ignored tests were the opt-in release CPU benchmark, optional OpenPGP signing fixture and optional loopback-sshd fixture; none was run in this Linux session.

The application suite includes rendered-node disabled/selected-Tab and focus-trap dialog regressions, navigator traversal, reduced-motion GIF state, GitHub mock/process boundaries and tab/history persistence. These are automated model/toolkit tests; generated accessibility metadata is separate from actual assistive-technology interaction.

## Release identity and retained evidence

Release executable SHA-256: `0d82d022d59a24479d72c025458cbe85fecd396db1505f1f1166bf7c057c2f8a`. `readelf -h /qa-target/release/gitturtle` identifies an ELF64 little-endian AArch64 position-independent executable. `ldd /qa-target/release/gitturtle` resolves all listed shared libraries in the QA image. Identity checks completed successfully in 0.173 seconds.

Local evidence is retained at `/tmp/gitturtle-linux-0a0fb5a-of0a3kk5`: immutable source archive, `run.py`, exact Docker argument arrays, per-stage timing/exit JSON, full fetch/format/tests/Clippy/release/identity logs, parsed `test-counts.json`, `source-info.json` and aggregate `manifest.json`. The manifest records SHA-256 values for all logs. Key log hashes:

| Log | SHA-256 |
| --- | --- |
| `tests.log` | `143c9890b5cb55e7f2270bdddec445eaf5e3c311f55374bba6f8903dda565ac1` |
| `clippy.log` | `90a136c20d279ab5f864828953a76b6a38668ea95b95c8203eed7b023cd670cd` |
| `release.log` | `27c3c6bfbc28d250b2168f0144662a9de8d22dff6310481c18fe5f0a7c0919b5` |

All named task containers exited and were removed; the final task-name-filtered container listing was empty. Build caches remain available.

No Linux desktop session, native Linux window, Linux screen reader, hosted CI or hosted GitHub account action was exercised. macOS PDF codecs, Keychain authorization, actual VoiceOver, display settings and packaging remain separate coordinator evidence. Earlier [Linux environment records](2026-09-09-milestone-environment.md) and [this checkpoint’s preparation inventory](2026-09-09-next-linux-inventory.md) retain their own scope.

## Integrated accessibility candidate a7139cd

The same isolated Linux/aarch64 environment executed all gates against `a7139cd45dbd5ec98004bf52770a65d3c974b3fc`, exported with `git archive --format=tar a7139cd45dbd5ec98004bf52770a65d3c974b3fc`. Archive SHA-256: `8721dc18cedf84d380b1c129fd0e391e9ba04093e5b9e736542b6e3f35ae1474`; the tar commit header independently matched. The source mount stayed read-only, with no overlay. This candidate includes the exact gpui-component 0.6.0 input focus-owner patch and additional dialog/graph fixes.

Formatting passed in 1.453 s, strict workspace Clippy passed in 9.044 s, and the release build passed in 70.446 s. Locked dependency preparation passed in 0.485 s. The workspace test command above **failed** in 44.885 s: 527 unique tests passed, two failed, and three remained ignored. The app suite reported 250 passed, two failed and one ignored; all core and preview suites passed. These failures prevent treating this candidate as a passing combined gate:

- `native_accessibility::dialog_tests::focused_reader_closes_find_before_the_dialog` used the literal macOS `cmd-f` shortcut on Linux, so Find did not open before Escape reached the dialog. The first-Escape assertion failed.
- `profiles::store::tests::restart_assignments_edit_delete_and_stale_saves_preserve_other_profiles` received an advisory-lock-busy error at the final sequential delete, after the expected stale-delete rejection. The coordinator observed the same profile failure in the macOS combined suite; it requires a source correction and combined recheck.

Release SHA-256: `d089280baecf1baf901e890f561d9ebbc05d0a1ad0eb785de356be575620190b`. Identity inspection passed in 0.155 s: ELF64 AArch64 PIE, all listed shared libraries resolved. This successful build does not negate the test failures.

Full source archive, exact Docker argument arrays, stage logs, parsed test identities, per-stage exit/timing records and SHA-256 manifest are retained under `/tmp/gitturtle-linux-a7139cd-0bz0w1ib`. All named task containers were removed. No native Linux, hosted CI or hosted GitHub account action was exercised. The seven new passing/failing app tests are counted by distinct suite/test identity, including the child Mermaid isolation summary only once.

| Candidate log | SHA-256 |
| --- | --- |
| `tests.log` | `9195ef7cac9674ba21374c4657bf24ff4678bdf226d4640117929901b6081e35` |
| `clippy.log` | `ab0815d64984f76cde43c15f0dd91094e96689d24ec049ac66719e75e8a39efe` |
| `release.log` | `719264fee81f367e4e3c6c30b08f16fc416718846e67c3523f327a181181be55` |

## Corrected workflow candidate f9fb53e

The immutable `f9fb53e52809bbce5d2b73af2e844bfb5def99ef` archive (SHA-256 `7233f3aec57bc1d9cf0366a8cc37c076958d3954ecc63d393403af3470b5a0cb`) used the same isolated commands and environment. Fetch passed in 0.446 s, formatting in 1.500 s, strict workspace Clippy in 5.679 s, and release in 70.326 s. Identity inspection passed in 0.180 s; the ELF64 AArch64 executable hash is `db7df205687cf14288a86d5c7fead9872e851f657ba2210a56d57486356b6fa1`, with all shared libraries resolved.

The workspace test gate **failed** in 26.712 s because the app test process aborted. The new full-app GitHub opening regression performed real serial-executor I/O while GPUI's test scheduler still forbade foreign-thread wakes. Its background reply triggered the scheduler assertion; the ensuing task cleanup aborted the process. The earlier profile-lock and portable-Find regressions passed, and core/preview suites finished successfully. Although 529 distinct passing test lines were observed across the process output, the app suite did not produce a normal result; this is not a completed passing test total. The required correction is confined to the integration test's supported GPUI real-I/O mode and draining both serial executors before teardown; production scheduling is unchanged.

Evidence is retained in `/tmp/gitturtle-linux-f9fb53e-o5fsgcaq`, including archive, full logs, exact Docker argv, stage records, and manifest with the incomplete app-suite limitation. All task containers exited and were removed before further native performance work. `tests.log` SHA-256: `f0fe5df9aed6547ea387b46ec59381b141c681fc58f88f12dd919221e1f197c0`. No native Linux or hosted execution claim is made.

## Final candidate 58f09ff

All applicable Linux/aarch64 gates passed against immutable commit `58f09ff690e0dbd863a8c29d6699640248d0a136`. The source was exported with `git archive --format=tar 58f09ff690e0dbd863a8c29d6699640248d0a136`; archive SHA-256: `4991781fa6ded754910d7680c90ccdc827bdd24d9a570b469a5f3ca203909a1e`. The tar commit header independently matched. The same image ID, read-only source/root, task-owned caches, CPU/memory limits and network isolation described above were used, with no source overlay or version changes.

| Exact command inside the container | Result | Wall time |
| --- | --- | --- |
| `cargo fetch --locked` | Passed | 0.442 s |
| `cargo fmt --all -- --check` | Passed | 1.584 s |
| `cargo test --locked --offline --workspace --no-fail-fast` | 533 unique passed; zero failed; three ignored | 24.900 s |
| `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | Passed | 5.992 s |
| `cargo build --release --locked --offline -p gitturtle` | Passed | 79.696 s |

The complete passing total is **533**: app 256, core unit 23, core integration 209 and preview 45. The Mermaid child summary is counted once, and both doc-test suites contain zero tests. The three existing opt-in CPU/OpenPGP/loopback-sshd tests remain ignored. The earlier profile-lock, portable-Find and full-app GitHub opening regressions all passed in this combined run. The macOS-only NSWorkspace notification observer is not compiled or exercised on Linux; macOS adapter/Keychain/codec and actual assistive-technology evidence remain separate.

Release SHA-256: `60668b0cd2aa1772ba39b8bc4bb3d78667be19bb25cbf05967067725a718b87b`. Identity checks passed in 0.232 s: ELF64 little-endian AArch64 PIE, with every listed shared library resolved. This candidate's release command ran successfully against its own archive; the recorded hash identifies that exact output. Compilation times include container overhead and warm caches, and are not application latency or performance measurements.

Evidence is retained at `/tmp/gitturtle-linux-58f09ff-qxvwi9j1`: source archive, `source-info.json`, `run.py`, exact Docker argv and stage records, complete logs, parsed test identities and aggregate `manifest.json`. All named task containers exited and were removed. No Docker validation workload remains running; only task-owned caches remain. No Linux desktop window, Linux screen reader, hosted CI or hosted GitHub action was exercised.

| Final log | SHA-256 |
| --- | --- |
| `tests.log` | `0a8e81b5d8ce2acc31ccb447ae492b04f2fbc2385f84b152123091c83b085fe0` |
| `clippy.log` | `b9dc09eb9e043c16e188f5eb7c4a437bb1d0c3310d52b1aa704972f339492c30` |
| `release.log` | `7416be5cf424b8889b22de18789ba464193df7dfb9a0028ef39a16311d98863f` |
