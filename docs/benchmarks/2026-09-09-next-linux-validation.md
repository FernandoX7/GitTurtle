# Linux validation of the next milestone checkpoint

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
