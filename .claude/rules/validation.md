# Validation

Run `python3 scripts/gate.py fast` before finishing any Rust change; `python3 scripts/gate.py full` is the per-candidate gate. The [`gitturtle-gates`](../skills/gitturtle-gates/SKILL.md) skill describes their stages, strict mode, known failures and the red-gate report. Fix the root cause, never the threshold; only a host-specific entry in `.local/gate/known-failures.txt` excuses a red test.

`AGENTS.md` sets the performance-measurement and evidence rules; the native-qa and performance-reviewer roles collect the native, package, performance and vendor evidence.
