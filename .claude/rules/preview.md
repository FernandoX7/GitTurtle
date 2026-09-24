---
paths:
  - "crates/preview/**"
---
# Preview decoder conventions

The crate guide beside this code owns the decoder contracts; this rule adds what Claude needs to test and review here.

- Decoders consume supplied bytes only. The filename is a format hint, never a path to open; decoders never touch the filesystem, repository handles, app state or native UI types, and results carry owned data with explicit unsupported, absent or truncated states.
- Keep source-byte, source-dimension, decoded-allocation and output-dimension limits independent, reject unreasonable headers before decoding pixels, and keep cancellation between phases.
- Run `cargo test --locked -p gitturtle-preview [filter]`; most tests are inline in the owning module and binary fixtures are documented in `tests/fixtures/README.md`. New fixtures need recorded provenance. Decoding-bound changes are measured with the performance skill on a release build.
