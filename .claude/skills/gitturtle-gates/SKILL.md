---
name: gitturtle-gates
description: Run and read GitTurtle's tiered quality gate (python3 scripts/gate.py fast|full) for Rust changes, including how to interpret .local/gate/report.md, known-failure allowlists and strict controller mode. Use before finishing any Rust change or when a gate is red.
---

# GitTurtle gates

`python3 scripts/gate.py fast` is the per-change gate: fmt, spelling, unused dependencies, `cargo check` for the workspace, and clippy plus tests for the crates that changed since the base (merge-base with `main`, or `--base <ref>`). It targets under three minutes on a warm target directory. `python3 scripts/gate.py full` is the per-candidate gate at workspace scope: the whole suite, doctests, clippy, doc lints, dependency licenses and advisories, snapshot verification, diff-scoped mutation testing, coverage on the non-UI crates in `--strict` mode, and the release build. The controller and the verifier run it with `--strict`, which turns missing optional tools and advisory-only stages into failures.

Exit codes: 0 green, 1 a stage failed, 3 a required tool is missing in strict mode, 4 usage error. On success the script prints one line; on failure it prints a compact report and writes `.local/gate/report.md` with the failing stage, the command, the exit code, the first error as `file:line:col`, the narrowest command that reproduces it, the last twenty lines of output, and a `Tests removed` count from the diff.

Read the report before touching code and reproduce with the `Next command` it names. Fix the root cause. Thresholds, `.config/nextest.toml`, `deny.toml`, snapshot baselines and the gate script itself are policy files: a change to them is a separately reviewed proposal, never part of making an attempt pass. A test that fails only on this host goes in `.local/gate/known-failures.txt`, one full test name per line, and is reported in the summary; nothing else excuses a red test.

Reuse the warm build: keep `CARGO_TARGET_DIR` pointing at the shared target directory when working from a worktree, and do not rerun a clean gate on unchanged code. Report the exact command and its result, including any `warn:` lines about skipped stages.
