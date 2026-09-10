# Next milestone Linux environment inventory

Read-only inventory on September 9, 2026, before the integrated source checkpoint. No current-milestone Linux build, test, Clippy, release executable or native desktop interaction is established by this inventory.

The Docker Desktop `desktop-linux` context is available. The existing task-owned image `gitturtle-linux-qa-20260909:043f562` remains present, with image ID `sha256:869f420928739dea3f57a1ee3dee459783f659de00e3045c57dcfe8efe2332f6`, Linux/arm64. This current inspected ID takes precedence over older image configuration IDs in the dated environment record.

A new ephemeral inventory container ran with `--network none --read-only --cap-drop ALL --security-opt no-new-privileges`, then exited and was removed. It reported Linux `7.0.12-linuxkit aarch64`, Rust `1.98.0 (88d9e12ae 2026-08-18)`, Cargo `1.98.0 (797e8a9bc 2026-08-05)`, Git `2.43.0`, xkbcommon `1.6.0`, OpenSSL `3.0.13`, and Vulkan `1.3.275`. The host has only the `aarch64-apple-darwin` Rust target; a container provides the existing Linux toolchain without changing host targets.

The earlier isolated cache volumes remain:

- `gitturtle-linux-20260909-043f562-33sa6mbc-cargo`
- `gitturtle-linux-20260909-043f562-33sa6mbc-target`

No existing user container or cache was modified. The image’s default `CARGO_HOME` is `/opt/cargo`, and `RUSTUP_HOME` is `/opt/rustup`.

## Prepared integrated validation

After the coordinator creates the final source checkpoint, use an immutable `git archive` snapshot, record its SHA-256, and mount it read-only at `/source`. Reuse only the task-owned volumes above as `/qa-cargo` and `/qa-target`; set `CARGO_HOME=/qa-cargo`, `CARGO_TARGET_DIR=/qa-target`, and working directory `/source`. Keep six CPUs and 12 GiB as in the previous environment record. The new GPUI test-support packages may require a public dependency-only `cargo fetch --locked` with network enabled before offline validation.

Then run separate bounded task containers with network disabled, all capabilities dropped, no-new-privileges, and a writable temporary directory for fixture tests:

```sh
cargo fmt --all -- --check
cargo test --locked --offline --workspace --no-fail-fast
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo build --release --locked --offline -p gitturtle
```

Verify the resulting ELF identity with `sha256sum`, `readelf -h`, and `ldd`, retaining logs and exact source/image identities. Separate compilation/test timing from application performance. A container pass cannot establish Linux desktop accessibility, window behavior, macOS codecs/Keychain, or hosted CI. No full gate was started during this inventory.
