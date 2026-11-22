# Development agent architecture

GitTurtle's agent configuration is strictly development tooling for the native Rust/GPUI client; the shipped application has no AI features. Product execution evidence remains tied to exact source and build identities in [validation.md](validation.md).

All durable rules, skills, roles, and executable checks are **platform-agnostic** and follow open standards (`AGENTS.md`, `.agents/skills/`, `.agents/roles/`, `.githooks/`, and stdlib Python scripts) so every coding agent and human contributor shares identical ground truth.

## Canonical layout & generated adapters

| Path | Role |
| --- | --- |
| [AGENTS.md](../AGENTS.md) | Concise root constitution (`<= 8 KB`): mission, invariants, commit/no-AI-attribution rules, CoW worktree isolation, routing table, validation |
| [CLAUDE.md](../CLAUDE.md), [GEMINI.md](../GEMINI.md), [copilot-instructions.md](../.github/copilot-instructions.md) | Ultra-thin tool shims (`<= 15` / `<= 10` lines) importing `AGENTS.md` with zero rule overrides |
| Crate & vendor `AGENTS.md` | Scoped subsystem contracts (`crates/app/`, `crates/git-core/`, `crates/preview/`, `vendor/`) |
| `.agents/skills/*/SKILL.md` | 13 canonical Open Agent Skills (progressive disclosure); symlinked 1:1 into `.claude/skills/` |
| `.agents/roles/*.md` | 8 canonical tool-neutral role contracts (`implementer`, `code-reviewer`, `security-reviewer`, `design-reviewer`, `performance-reviewer`, `native-qa`, `librarian`, `verifier`) with **zero model or effort pinning** |
| `.agents/workflows/review-candidate.md` | Portable two-phase candidate review protocol (4 parallel read-only lenses + adversarial skeptic verifier) |
| `.claude/agents/*.md`, `.codex/agents/*.toml` | Thin generated adapters produced by `python3 scripts/dev/sync_adapters.py --write` (verified automatically by `scripts/check-agent-guidance.py`) |
| `.githooks/{pre-commit,commit-msg,pre-push}` | Universal Git hooks (`git config core.hooksPath .githooks`) delegating to `scripts/dev/guards.py` and fast checks |
| `scripts/dev/` | Portable Python dev tooling: `worktree.py` (CoW reflink worktrees), `build_slot.py` (slots + benchmark quiet lock), `guards.py`, `guidance.py`, `sync_adapters.py` |
| [docs/development/STATE.md](development/STATE.md) | Overwritten active working state (`<= 60` lines, enforced by `check-agent-guidance.py`); replaces historical multi-megabyte handoff logs |

## Roles, delegation & model inheritance

- Every canonical role and generated adapter omits hardcoded `model` and `effort` overrides so delegated work inherits the caller's active session configuration unless the user explicitly asks otherwise (`scripts/check-agent-guidance.py` enforces this for both `.claude/agents/` and `.codex/agents/`).
- The coordinator owns integration, the Git index, commits, PR merges, and display handover. Exactly one owner drives the native GUI at a time, confirmed with literal `python3 scripts/native_qa/qa.py display-check` output.
- Review roles (`code-reviewer`, `security-reviewer`, `design-reviewer`, `performance-reviewer`, `verifier`) remain strictly read-only. Security review follows [docs/development/security-review.md](development/security-review.md); structural/interface changes follow [docs/development/architecture-review.md](development/architecture-review.md).

## Retired unattended controller (historical context)

The earlier standalone unattended controller (`scripts/agent_loop/`, `scripts/agent-loop.py`, `scripts/operator/`) was retired in November 2026 in favor of direct multi-agent coordinator sessions, isolated CoW worktrees (`scripts/dev/worktree.py`), universal `.githooks/`, and deterministic `scripts/gate.py` / `scripts/check-agent-guidance.py` checks. Historical decision notes remain in `docs/development/` for reference without claiming active controller execution.

## Validation

Run these fast checks (zero Rust build required) after changing guidance, skills, roles, hooks, or dev tooling:
- `python3 scripts/dev/sync_adapters.py --check`
- `python3 scripts/check-agent-guidance.py`
- `typos --format brief`
- `python3 scripts/ci/tooling_suites.py run --directory /tmp/gt-tooling-check`
