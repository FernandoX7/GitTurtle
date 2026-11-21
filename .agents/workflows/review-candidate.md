# Review Candidate Workflow (Platform-Agnostic)

Run this two-phase protocol before opening a PR or merging a non-trivial candidate change, using any agent runtime (Claude Code, Codex, Gemini/Antigravity, Cursor, or human-led subagents).

## Inputs
- `base`: base Git ref (default `main`)
- `dir`: worktree path containing the candidate (default `.`)

## Phase 1 — Parallel Read-Only Lenses
Launch up to four independent read-only reviewers against `git -C <dir> diff <base>...HEAD` plus any uncommitted/untracked working-tree changes. Each lens reads the relevant crate or vendor `AGENTS.md` first and returns structured findings (`file`, `line`, `severity` in `blocker|major|minor`, `summary`, `failure_scenario`, `evidence`):

1. **Correctness** ([code-reviewer](../roles/code-reviewer.md)): contract violations, observable regressions, races, weakened tests, dropped repository-preservation/stale-target/generation guards, deleted-guard property audit.
2. **Design** ([design-reviewer](../roles/design-reviewer.md), when UI/theme/layout/a11y files changed): `DESIGN.md` compliance, resolved theme token contrast, narrow layouts, keyboard focus, accessibility labels, visual evidence tier.
3. **Performance** ([performance-reviewer](../roles/performance-reviewer.md), when hot paths changed): off-UI-thread discipline, virtualization, metadata-before-content, bounded queues, release-mode measurements and trace boundaries.
4. **Security** ([security-reviewer](../roles/security-reviewer.md), when code, Git commands, credentials, decoders, filesystem, dependencies, or CI/packaging changed): trust boundaries, redaction, decoder/resource bounds, symlink/path safety, CI supply chain.

## Phase 2 — Adversarial Skeptic Verification
Sort all findings by severity (`blocker`, `major`, `minor`) and inspect the top 8 with an independent read-only skeptic ([verifier](../roles/verifier.md)):
- For each finding, read the actual source, callers, and tests (or run one narrow test filter) to **try to refute** the claimed failure scenario.
- Default to `refuted: true` unless the failure path is confirmed from concrete code or command evidence.

## Output
Present one compact summary to the coordinator:
- **Confirmed findings** (severity, lens, `file:line`, failure scenario, why confirmed) — fix all blockers and majors in an atomic increment before opening or merging the PR.
- **Refuted findings** (one line each with refutation reason).
- **Open evidence gaps** (e.g., macOS/Wayland desktop QA or hosted CI lanes still pending).
