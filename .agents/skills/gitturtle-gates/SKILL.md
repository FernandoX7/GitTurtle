---
name: gitturtle-gates
description: Run and interpret GitTurtle's tiered quality gate (python3 scripts/gate.py fast|full), .local/gate/report.md, CI Git isolation, per-worktree CARGO_TARGET_DIR discipline, and strict mode. Use before finishing any Rust or tooling change or when a gate fails.
---

# GitTurtle quality gates

- **`python3 scripts/gate.py fast`** (per-change gate, <3 min warm): formatting, spelling (`typos`), unused dependencies (`cargo-machete`), workspace `cargo clippy --locked --workspace --all-targets -- -D warnings`, tests for crates changed since `--base <ref>` (default `GITTURTLE_GATE_BASE` or merge-base with `main`), and `ITERATIONS=20` on changed `#[gpui::test]` functions (up to 40 tests; run manually if >40). Root/vendor/manifest/lockfile/toolchain edits widen tests to `--workspace`. Non-Rust changes run workspace `cargo check`.
- **`python3 scripts/gate.py full`** (pre-PR/candidate gate): runs all build-free checks first — `format`, `typos`, `machete`, `insta` (`.snap.new` hygiene), `privacy`, `deny` (`cargo deny --locked`, never mutating `Cargo.lock`), `audit`, `guidance` (`scripts/check-agent-guidance.py`), `dev-tooling-tests` (`scripts/dev`), and `gate-tests` (`scripts/test_gate.py`, `scripts/test_claude_rm_hook.py`) — followed by `clippy`, workspace tests, GPUI iterations, doctests, doc lints, diff-scoped `mutants`, and the release build.
- **Linux -> macOS core cross-check:** On Linux hosts whenever `crates/git-core` changes, also run `env -u RUSTUP_TOOLCHAIN cargo clippy --locked -p gitturtle-core --target aarch64-apple-darwin -- -D warnings` (when the target is installed) to catch macOS `cfg(target_os = "macos")` compile/lint regressions before pushing.

## Target directories & toolchain isolation

- **Never share a mutable `CARGO_TARGET_DIR` across worktrees** (concurrently or sequentially). Cargo hashes path packages relative to workspace root and relies on mtimes, so sharing `target/` silently swaps binaries, tests, and vendor patches.
- Create every worktree with `python3 scripts/dev/worktree.py new <name> [base]`, which seeds an isolated `.local/wt/<name>/.local/target` in <0.5 s via Btrfs/APFS CoW reflinks from `.local/target-base`. Prefix Cargo commands with `env -u RUSTUP_TOOLCHAIN CARGO_TARGET_DIR=<worktree>/.local/target` when the host exports `RUSTUP_TOOLCHAIN`.
- Do not rerun a clean gate on unchanged code.

## Interpreting results & Git isolation

- Without `--strict`, missing optional tools warn and advisory stages (`machete`, `doc`, `deny`, `audit`, `mutants`) warn. `--strict` fails on missing tools/advisories and runs `cargo-llvm-cov` when `GITTURTLE_GATE_COVERAGE_MIN` is set.
- Exit codes: `0` green, `1` stage failure, `3` missing required tool in strict mode, `4` usage error. On failure, read `.local/gate/report.md` first and reproduce with its `Next command`.
- Fix the root cause. Policy files (`.config/nextest.toml`, `deny.toml`, `clippy.toml`, `rustfmt.toml`, snapshots, `scripts/gate.py`) are never weakened to turn a failure green. There is no known-failures allowlist.
- Every test stage runs under CI Git isolation (`GIT_CONFIG_NOSYSTEM=1`, empty `GIT_CONFIG_GLOBAL`, unset `GIT_ASKPASS`/`SSH_ASKPASS`/`SSH_ASKPASS_REQUIRE`/`GIT_CONFIG_PARAMETERS`/`GIT_CONFIG_COUNT`). If a test passes under `gate.py` but fails under plain `cargo test`, isolate it inside the test itself.
- When matching Cargo logs, match `Finished` or `^error` (not bare `error`, which matches crate names like `thiserror`).
