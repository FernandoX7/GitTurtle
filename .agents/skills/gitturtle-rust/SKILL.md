---
name: gitturtle-rust
description: Rust 1.100 / Edition 2024 engineering standards for GitTurtle — workspace lints, unsafe and unwrap boundaries, expect attributes, rustix/objc2, test-module splitting, and typed crate-boundary errors. Use for any Rust implementation or refactoring task.
---

# GitTurtle Rust standards

## Toolchain & workspace lints

- Respect `rust-toolchain.toml` (`env -u RUSTUP_TOOLCHAIN` when a host exports `RUSTUP_TOOLCHAIN=stable`). All workspace crates inherit `[workspace.package]` (`version`, `edition`, `rust-version`) and `[workspace.lints]` (`[lints] workspace = true`).
- `[workspace.lints]` enforces `unsafe_op_in_unsafe_fn = "deny"`, `undocumented_unsafe_blocks = "deny"`, `allow_attributes = "warn"`, `dbg_macro = "deny"`, `todo = "deny"`, `unimplemented = "deny"`, and curated readability lints (`explicit_iter_loop`, `inefficient_to_string`, `manual_let_else`, `uninlined_format_args`).
- Use `#[expect(...)]` instead of `#[allow(...)]` so stale suppressions fail automatically. Use `#[allow(...)]` paired with `#[expect(clippy::allow_attributes)]` only for genuine cross-target items (e.g., helpers compiled under both `bin` and integration tests).

## Safety & panic boundaries

- **`crates/git-core`**: `#![forbid(unsafe_code)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used))]`. Keep passive reads on fixed argument arrays, raw objects, no optional locks, and bounded memory; use `rustix` for safe POSIX wrappers rather than raw `libc` FFI.
- **`crates/preview`**: Keep `unsafe` confined to explicit documented FFI boundaries (`crates/preview/src/lib.rs` `libdav1d_version` and native OS bridges); never add production `.unwrap()` on untrusted byte slices. Prefer `objc2` bindings over raw `objc_msgSend` transmutes on macOS.
- **`crates/app`**: Avoid UI-thread panics — use `f32::total_cmp` instead of `partial_cmp().unwrap()`, recover poisoned mutexes deliberately (`unwrap_or_else(|e| e.into_inner())` where invariant-safe), and keep all Git/parsing/decoding/layout-prep work off the UI thread.

## Module & error structure

- **Sibling test files:** When a module's test suite grows beyond ~300 lines, place tests in a sibling `*_tests.rs` file (`#[cfg(test)] mod tests;`) rather than growing inline monoliths (avoid adding to already-large inline test sections in `views.rs`, `github_view.rs`, or `repository_tabs.rs`).
- **Typed crate-boundary errors:** Expose structured error enums at `git-core` and `preview` boundaries so `crates/app` matches variants instead of parsing formatted error strings.
- **Persistence schema compatibility:** Adding a defaulted field older builds safely ignore does not bump store version; incompatible schema changes bump version, keep older files readable, test both versions, and reread-before-merge on save.
- **Git test isolation:** Every test spawning Git must clear inherited `GIT_ASKPASS`/`SSH_ASKPASS` and pass empty system/global config (`GIT_CONFIG_NOSYSTEM=1`, empty `GIT_CONFIG_GLOBAL`). Never leave blocking/FIFO child threads detached at test exit.
