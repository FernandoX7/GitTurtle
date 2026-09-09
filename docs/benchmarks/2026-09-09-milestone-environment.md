# Milestone authentication and Linux environment checks

Executed September 9, 2026. These are local automated transport/signing checks and separate Linux container validation; they are not native prompt, hosted-provider or performance evidence.

## Local authentication and cryptographic signing

Host: Apple arm64, macOS 26.6.2 (25G83), Apple Git 2.50.1 (Apple Git-155), OpenSSH 10.3p1 / LibreSSL 3.3.6, GnuPG 2.5.22 / libgcrypt 1.12.3, Rust 1.98.0 (`88d9e12ae`, August 18, 2026).

The tests ran during integration leading to source checkpoint `043f562`. The authentication implementation and both fixture files were checked byte-identical against that checkpoint afterward. No actual user credential, private key, global Git configuration, Keychain item or hosted remote was used. The ordinary and optional signing tests generate their own keys, agents, known-hosts file, SSH server and OpenPGP home in disposable directories. Servers bind only `127.0.0.1`; agents/keyrings are stopped and fixture directories removed by the test harness.

| Exact command | Executed result | Evidence boundary |
| --- | --- | --- |
| `env GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null cargo test --locked -p gitturtle-core --test signing -- --include-ignored` | 3 passed; test execution 1.76 s | Real SSH-agent commit/tag signatures and hook execution; after deleting the private key file, signing still uses the isolated agent. Stopping that agent refuses further signed commits/tags without unsigned fallback and preserves HEAD/index/worktree. Optional OpenPGP signs/verifies real commits/tags in a disposable keyring. Optional loopback SSH fetch pins a generated host key, then refuses the same host when its known-hosts entry is removed, preserving refs and unrelated bytes. |
| `env GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null cargo test --locked -p gitturtle-core --test authentication` | 3 passed; test execution 1.22 s | Disposable HTTP helper Fetch/Pull, credential approval and expired-credential rejection, expected local refs/bytes, and refusal of secret-bearing clone URLs before destination creation. |
| `env GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null cargo test --locked -p gitturtle-core --lib work::authentication::tests` | 6 passed; test execution 0.19 s | Actual local Unix-socket askpass exchange, stale-answer refusal, secret redaction, configured askpass/helper precedence, socket cleanup, explicit cancellation/no replay and detached helper pipe cleanup. |

The durations above are Cargo's test-execution summaries, excluding compilation. They are not latency benchmarks.

Relevant SHA-256 input identities:

| File | SHA-256 |
| --- | --- |
| `crates/git-core/tests/signing.rs` | `5341fdb65b23bbaa50bcf3805aee1e427c47ff660ff1b85a5380a74cad40f771` |
| `crates/git-core/tests/authentication.rs` | `f1d72d1b344aecbaa90bc42fc5931e067cada1567d8979886178bc5aae84ee58` |
| `crates/git-core/src/work/authentication.rs` | `9f7e6c1621c88cb136e7e47904a9888c4e7900daedffe8a2ae2adedc1a910635` |

The installed Git exec directory contains `git-credential-osxkeychain`; `security list-keychains -d user` succeeded and identified a configured login Keychain. This establishes presence/configuration only. No `get`, `store`, `erase`, password retrieval, unlock or Keychain dialog was attempted. Actual Keychain unlock, real stored credentials, hardware-backed keys and native pinentry remain unverified.

## Linux execution environment

The macOS host initially had only the `aarch64-apple-darwin` Rust target. A read-only local Docker probe found a working Linux/aarch64 daemon. The existing Ubuntu 24.04 image was launched with `--network none --read-only --cap-drop ALL --security-opt no-new-privileges`; it reported Linux/aarch64 and no Rust, Cargo, C/C++ compiler, pkg-config or Git. No existing user container was modified.

A new task-owned container image installs the quality workflow's Linux build libraries plus Git, curl and CA certificates, then the exact Rust 1.98.0 minimal toolchain with rustfmt and Clippy. Public package/toolchain downloads are engineering dependencies; no source branch, artifact, release or hosted workflow was published. Base image: `ubuntu:24.04`, digest `sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54`. Built QA image configuration: `sha256:c63d4cfc51e03f777a363a0781ab97ae2f751e582e0ba0bcd9956304b255b9b4`.

The first source is an immutable `git archive 043f5628598aa5b59bc1190da9954ff843c6da28` snapshot, mounted read-only. Its Cargo registry and target directories are separate task-owned Docker volumes. Build/test CPU and memory are bounded to six CPUs and 12 GiB. Dependency fetch is separate from validation so validation runs with external networking disabled while allowing the tests' loopback fixtures. The container has Git 2.43.0 and Rust 1.98.0 for `aarch64-unknown-linux-gnu`.

| Exact initial command | Executed result at `043f562` |
| --- | --- |
| `cargo fetch --locked` | Passed; dependency preparation had network access. |
| `cargo fmt --all -- --check` | Passed, 1.32 s including container startup. |
| `cargo test --locked --offline --workspace` | Compiled and linked the native application and workspace test targets, then failed one profile signing test after 217.04 s. The app suite passed 181 tests with one ignored; the core unit suite passed 21. The failing assertion expected tag creation with a deleted SSH signing key to return an error. |
| `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | Passed, 82.24 s including container startup and compilation. |

The signing failure was reproduced directly in a disposable Git 2.43.0 repository: `git tag --sign --cleanup=verbatim --file=- -- must-fail HEAD`, configured with `gpg.format=ssh` and a nonexistent `user.signingKey`, returned zero and created an unsigned annotated tag while reporting a missing public key on stderr. This exposed an actual older-Git behavior; the initial workspace test gate did not pass.

Running the remaining suites separately exposed two diagnostic compatibility cases: a missing promisor object terminates Git 2.43's batch reader with lazy fetching disabled, and a stash application blocked by an existing index lock returns empty stdout/stderr. Both now provide useful diagnostics while preserving no-fetch and repository-state assertions. The Linux preview suite independently passed 19 tests; macOS-only decoder tests are excluded on Linux.

An immutable six-file compatibility overlay of `043f562` passed `cargo test --locked --offline -p gitturtle-core --no-fail-fast --test repository --test recovery --test profiles --test signing --test tags_ignore`: 60 passed, with two optional environment-dependent signing tests ignored. This exercised real SSH missing-key refusal, exact unsigned-tag rollback, concurrent replacement preservation, missing-object snapshots and stash lock preservation. A subsequent immutable tag-guard refinement passed `cargo test --locked --offline -p gitturtle-core --test tags_ignore signed_tag_postcondition`: two passed, including observed symbolic-reference replacement.

### Integrated Linux validation

The integrated source snapshot is `75daadcd7fa9b5a7f4a0d28f248cf053856b40d6`, again extracted using `git archive` and mounted read-only. The same isolated registry/target volumes retain compiled dependencies; no user repository, credential home or running application state is mounted. The final formatting/Clippy retry uses that snapshot with only `crates/app/src/settings.rs` replaced by its test-module relocation: SHA-256 `1d29cb20e8f28d8fd25be4013c412e933cee53b3580ef1cac71be7a536db12de`. The relocation moves the existing `cfg(test)` module after production items; executable behavior is unchanged.

| Exact command | Executed result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on `75daadc` and after the test-module relocation; final retry 1.25 s including container startup. |
| `cargo test --locked --offline --workspace --no-fail-fast` | Passed: 424 tests, zero failures, three ignored, 22.10 s including container startup and incremental compilation. App: 186 passed/one ignored; core unit: 21 passed; preview: 19 passed. The remaining passing tests are core integration fixtures. |
| `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | Passed after relocating the Settings test module, 3.32 s including container startup and incremental compilation. The initial `items_after_test_module` diagnostic is resolved. |
| `cargo build --release --locked --offline -p gitturtle` | Passed on `75daadc`, 131.35 s including container startup and compilation. |

The three ignored Linux tests are the manual performance fixture and optional OpenPGP/loopback-sshd environment tests. The latter two were explicitly executed successfully on macOS earlier in this record. These durations include compilation/container startup and are not application latency measurements.

The release executable SHA-256 is `2ccfe063599563c3f6ec19ec553c5055cc5884b8b9aa34928c38c328b6c17080`. `readelf -h` identifies an ELF64 little-endian AArch64 position-independent executable; `ldd` resolves every listed dynamic dependency in the QA image. The actual container kernel is `7.0.12-linuxkit` on aarch64, with Rust 1.98.0 / LLVM 22.1.8. No display server or native Linux window was started.

These runs establish Linux/aarch64 compilation and the explicitly listed checks. They do not exercise native window rendering or X11/Wayland integration, and they are distinct from the workflow's hosted Ubuntu runner.

## Hosted and native limits

`git remote` returned no configured source remote. No authorized hosted repository/destination was identified by this check, no hosted workflow was started, and no hosted run URL/result exists. The inspected quality workflow targets `macos-15` and `ubuntu-24.04`, uses read-only checkout credentials and has no publishing step; configuration is distinct from execution.

The coordinator owns native authentication dialogs and all macOS application QA. No VoiceOver, system appearance change, real-account authentication, actual Keychain interaction, external SSH/HTTP endpoint or user repository mutation occurred in these checks.
