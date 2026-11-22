# Fuzzing Harnesses (`cargo-fuzz` / `libFuzzer`)

This directory is excluded from the root Cargo workspace so stable `cargo test --workspace` never requires a nightly toolchain.

## Targets

- `preview_parsers`: exercises `gitturtle-preview` sniffers, metadata inspection, LFS pointer detection, declared dimensions, and image/SVG decoding on untrusted bytes.
- `model3d_parsers`: exercises `gitturtle-preview::model3d::decode_geometry` across `.stl`, `.obj`, `.fbx`, `.glb`, `.3mf`, and `.step` payloads.
- `git_parsers`: exercises `gitturtle-core` conflict-block parsing and resolution on arbitrary UTF-8 inputs.
- `secret_redaction`: verifies `redact_diagnostic` and `strip_bidi_controls` never leak URL credential tokens or retain bidi override controls (`OperationControl::redact` and `ProgressScanner::feed` are exercised by the in-crate `proptest` suite in `crates/git-core`).

## Running locally (requires nightly + `cargo-fuzz`)

```sh
cargo +nightly fuzz run preview_parsers -- -max_total_time=60
cargo +nightly fuzz run model3d_parsers -- -max_total_time=60
cargo +nightly fuzz run git_parsers -- -max_total_time=60
cargo +nightly fuzz run secret_redaction -- -max_total_time=60
```

Deterministic property-based equivalents (`proptest`) run automatically in `cargo test --workspace` on stable Rust without nightly.
