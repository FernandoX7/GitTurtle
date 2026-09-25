---
name: gitturtle-gates
description: Run and read GitTurtle's tiered quality gate (python3 scripts/gate.py fast|full) for Rust changes, including how to interpret .local/gate/report.md, known-failure allowlists and strict mode. Use before finishing any Rust change or when a gate is red.
---

# GitTurtle gates

`python3 scripts/gate.py fast` is the per-change gate: fmt, spelling, unused dependencies, `cargo check` for the workspace, clippy plus tests for the crates that changed since the base (`--base <ref>`, else `GITTURTLE_GATE_BASE`, else the merge-base with `main`), and 20 iterations of the GPUI tests in changed app sources. Rust-affecting paths outside a crate (such as `vendor/`, manifests, the lockfile, toolchain and lint configuration) widen it to the workspace; with no Rust change it stops after the check. It targets under three minutes on a warm target directory. `python3 scripts/gate.py full` is the per-candidate gate: the same stages at workspace scope (`--changed-only` keeps the changed crates), then doctests, doc lints, snapshot hygiene, dependency licenses and advisories, diff-scoped mutation testing, the release build, `scripts/check-agent-guidance.py` and the controller suite under umask 077 (the controller refuses group- or other-writable records).

Without `--strict` a missing optional tool skips its stage with a `warn:` line and the advisory stages (unused dependencies, doc lints, licenses, advisories, mutation testing) only warn. `--strict` turns both into failures and adds the coverage stage: line coverage of `gitturtle-core` and `gitturtle-preview`, which runs only when `cargo-llvm-cov` is installed and `GITTURTLE_GATE_COVERAGE_MIN` sets the minimum percentage. The unattended controller runs its own profile checks rather than this script.

Exit codes: 0 green, 1 a stage failed, 3 a required tool is missing in strict mode, 4 usage error. On success the script prints one line; on failure it prints a compact report and writes `.local/gate/report.md` with the failing stage, the command, the exit code, the first error as `file:line:col`, the narrowest command that reproduces it, the last twenty lines of output, and a `Tests removed` count from the diff.

Read the report before touching code and reproduce with the `Next command` it names. Fix the root cause. Thresholds, `.config/nextest.toml`, `deny.toml`, snapshot baselines and the gate script itself are policy files: a change to them is a separately reviewed proposal, never part of making an attempt pass. A test that fails only on this host goes in `.local/gate/known-failures.txt` (or `--known-failures FILE`), one full test name per line; a test stage whose only failures are listed passes with a `warn:` and the summary reports them. Nothing else excuses a red test.

Reuse the warm build: keep `CARGO_TARGET_DIR` pointing at the shared target directory when working from a worktree, and do not rerun a clean gate on unchanged code. Report the exact command and its result, including any `warn:` lines about skipped stages.
