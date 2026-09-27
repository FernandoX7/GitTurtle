# Validation

Run `python3 scripts/gate.py fast` before finishing any Rust change; `python3 scripts/gate.py full` is the per-candidate gate. The [`gitturtle-gates`](../skills/gitturtle-gates/SKILL.md) skill describes their stages, strict mode, Git isolation and the red-gate report. Fix the root cause, never the threshold; nothing excuses a red test, and the gate keeps no allowlist of known failures.

`AGENTS.md` sets the performance-measurement and evidence rules; the native-qa and performance-reviewer roles collect the native, package, performance and vendor evidence.
