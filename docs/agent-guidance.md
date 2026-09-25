# Development agent architecture

GitTurtle's agents develop the native Rust/GPUI client. They are development tooling; the client has no model integration. The [development workflow](development/README.md) is the operator guide, while product execution evidence remains tied to the source/build recorded in [validation](validation.md).

The shared guides, skills and review contracts apply across AI tools. The `.codex/agents` definitions and current controller adapter are an optional Codex integration. Contributors can use other tools or work directly from the [contribution guide](../CONTRIBUTING.md).

## Instruction layout

| Source | Responsibility |
| --- | --- |
| [Root AGENTS.md](../AGENTS.md) | Product boundaries, code routing, delegated ownership, model inheritance, atomic commits and combined validation |
| [App guide](../crates/app/AGENTS.md) | Native module routing, state/lifetime contracts and conditional feature references |
| [Git core guide](../crates/git-core/AGENTS.md) | Operation/fixture routing, passive and explicit commands, captured targets and Git outcomes |
| [Preview guide](../crates/preview/AGENTS.md) | Bounded supplied-byte decoding and presentation contracts |
| [Vendor guide](../vendor/AGENTS.md) | Patch provenance, paired toolkit changes and consumer regressions |
| [Feature-work skill](../.agents/skills/gitturtle-feature-work/SKILL.md) | Acceptance-driven implementation and controller-owned handoff |
| [Architecture review](development/architecture-review.md) | Cohesive boundaries, state ownership, compatibility and failure-path evidence for material changes |
| [Performance skill](../.agents/skills/gitturtle-performance/SKILL.md) | Affected scheduling/read/cache investigation and conditional measurement procedure |
| [Native-QA skill](../.agents/skills/gitturtle-native-qa/SKILL.md) | Affected real-app workflows, state restoration and conditional package checks |
| [Task specification](development/tasks.json) and [schema](development/task.schema.json) | Versioned feature outcomes, dependencies, scope and evidence requirements |
| [Controller](../scripts/agent-loop.py) | Isolated execution, fresh sessions, checks, evidence and acceptance state |

Tasks launched at the repository root explicitly read the affected crate/vendor guide because automatic instruction discovery varies by tool and working directory. Conditional references avoid loading unrelated subsystems. Keep feature progress in run state and dated evidence rather than adding changing task lists to AGENTS.md.

## Roles and acceptance

In coordinated agent work, the coordinator owns integration and commits. Four recurring roles have ready-to-use Codex definitions; their responsibilities can also be assigned in other tools:

- [Implementer](../.codex/agents/implementer.toml): one bounded feature, focused tests and a structured handoff.
- [Verifier](../.codex/agents/verifier.toml): an independent assessment of the identified candidate and its acceptance evidence.
- [Security reviewer](../.codex/agents/security-reviewer.toml): a separate, evidence-based assessment of the changed trust boundaries under the [security review contract](development/security-review.md).
- [Librarian](../.codex/agents/librarian.toml): assigned guidance, research and evidence reconciliation.

Architecture planning is a phase when uncertainty warrants it; crate ownership and existing skills provide domain specialization. One worker owns native UI/package interaction, exclusively and for a stated period: two sessions driving the same build in one desktop session invalidate each other's observations, so the desktop changes hands in writing. There is no mandatory agent count or an always-running design, research or performance persona.

The implementer and verifier also apply the architecture review when shared state or contracts change. The librarian reconciles module ownership, platform capabilities and validation routes with the actual source. The [September 15 maintainability audit](development/2026-09-15-maintainability-audit.md) records specific drift corrections and proposed implementation increments, including their evidence limits.

The controller snapshots approved task contracts and records attempt state outside them. Workers return results; they do not mark themselves passing or change grading rules. Every `AGENTS.md`, the agent/skill definitions and controller/task-policy files are protected during unattended attempts. Maintain them through explicitly scoped interactive work. Acceptance requires applicable deterministic checks, a separate verifier, conditional independent security review and every required external attestation for the same candidate. Security review follows general verification, binds the base/candidate/changed paths and evidence, and cannot pass with findings or material gaps. The coordinator keeps one consolidated report; the reviewer does not post comments or operate external services. Native, package, performance and vendor evidence remains explicit; missing coverage defers acceptance and dependent work.

The Codex role files omit `model` and `model_reasoning_effort`, preserving interactive inheritance. A separate unattended CLI process receives the operator's explicit `--model` and `--effort`; it cannot infer the desktop session's choice. The runner controls child configuration to avoid unrelated global overrides. No repository-wide Ultra setting is introduced.

Both reviewers request `sandbox_mode = "read-only"`. This is a default, not a universal guarantee: Codex reapplies live parent permission overrides when spawning a child. Review-only instructions and controller candidate-mutation checks remain necessary. The librarian's assigned-path restriction is likewise a role contract, not a filesystem access-control list. See [official subagent configuration and permission behavior](https://learn.chatgpt.com/docs/agent-configuration/subagents).

## Sustained work

Use durable task contracts, progress records and evidence for sustained work in the chosen tool. Fresh implementation and review sessions limit accumulated context. The optional Codex controller manages an authorized dependency queue; interactive Codex goals require an explicit user request. An ordinary feature request does not authorize creating a goal or automation.

The controller uses private local clones with independent Git metadata and removed origin; it does not reset, stage or commit in the caller's checkout. It creates atomic Conventional Commits in attempt clones, then fetches accepted candidates locally and fast-forwards its private accepted branch. It does not integrate into the source branch, push or publish. Attempts, sessions, elapsed time and optional reported-output limits bound each run, including the security stage. Interrupted security review retains successful general verification and gates; unchanged successful reviews are reused after input and evidence validation. New policy applies to new runs, while saved runs retain their original controller snapshot. Stop/resume and external evidence registration are explicit operator commands; installing this architecture does not start an automation.

Changed-path inference adds conservative native evidence requirements for app Rust and native toolkit patches, and package evidence for assets/package scripts. A pending candidate becomes stale when unrelated accepted work advances its base; it must be rebuilt and verified against the new base. The [runbook](development/README.md#native-and-external-attestations) explains how to avoid collecting evidence for a superseded candidate. When a criterion requires its evidence inside the commit it validates, the evidence commit is itself what advances the base, so those candidates are rebuilt by design and are gated one at a time; the runbook records [that order](development/README.md#evidence-gated-by-its-own-commit).

See the [runbook](development/README.md) for command semantics, recovery and platform limits. Controller tests establish orchestration behavior only. Neither a valid configuration nor a short test proves unattended hours of successful product implementation.

## Security process change — September 15, 2026

The repository owner explicitly changed the initiative scope from CodeQL automation to a dedicated security reviewer. The [security review contract](development/security-review.md) records its evidence threshold, conditional routing, limitations and controller enforcement. The original queue remains immutable; its superseded CodeQL clauses are not represented as passed. Product validation and external evidence requirements remain independent of this process change.

## Research decisions — September 15, 2026

The audit started from GitTurtle source `470755bfd6d014eb6eb5fd8a7c7987cca0cf4e98`. It compared the existing contracts and local Wildhearth agent/skill/controller implementation with current official guidance. The inspected Codex CLI was 0.153.4; capability checks and command help are version-specific evidence, not a permanent minimum-version claim.

| Evidence | Adopted decision |
| --- | --- |
| [GPT-6 Astra guidance](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra), checked September 15 | Preserve authorized scope and steering; remove conflicting instructions; delegate useful bounded work; calibrate testing and concise reporting. Retain these existing agreements without copying a model prompt template. |
| [Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents), checked September 15 | Use standalone project TOML definitions with required identity/instructions; omit model/effort overrides and document live permission precedence. |
| Existing GitTurtle scoped guides and native/performance skills | Keep code-specific contracts and affected-workflow verification; add one feature handoff procedure instead of duplicating checklists. |
| Wildhearth's implementation/verifier sessions and durable run artifacts | Transfer fresh feature sessions, independent acceptance and persisted evidence; keep the controller outside model-owned grading. |
| Wildhearth source review of queue selection, cleanup, permissions and verdict parsing | Validate the dependency graph, select eligible work, isolate Git metadata, bind structured results to candidate identity and preserve interruptions for explicit reconciliation. |

Wildhearth's game content, naming/IP, simulation and translation roles do not map to GitTurtle's current work. A prompt-cache rationale does not justify preserving stale guidance. GitTurtle uses its existing Rust/Python tooling and native contracts rather than importing a game-specific toolchain or a graph framework.

Use the [research template](development/research-template.md) for future decisions that need primary sources and enforceable acceptance. Recheck official guidance when the model, CLI or observed behavior changes; do not invent a permanent rule for one failure.

## Historical audit — September 10, 2026

The prior audit checked source through `8464987` and updated routing for repository tabs, incremental history, GitHub collaboration, profiles, recovery/rewrite, rich previews, accessibility and vendor patches. At that time no custom agents or development controller were present; its decision to retain only two skills described that earlier scope.

That audit recorded successful checks across 14 guidance documents: local links/anchors, literal crate paths, module and fixture routes, package names and both skill validators. An independent routing review exercised README copy, model-camera/tab diagnosis, offline GitHub recovery and toolkit replacement. These are historical instruction-structure checks, not a model-quality benchmark or product execution record. Rust source/dependencies were unchanged, so it ran no native sessions or benchmarks.

## Maintenance

Keep durable rules here and in the scoped guides, numeric product limits in code, and measured results in dated evidence. Update the closest contract when implementation changes. Validate changed guidance links, commands, TOML and skill frontmatter; validate controller behavior with its focused tests. Do not rerun native/Rust gates merely because development documentation changed.

## Claude Code configuration — September 17, 2026

Claude Code support was added as a second optional integration beside the Codex definitions, without changing them: `CLAUDE.md` files import the guides above, roles live in `.claude/agents/`, path-scoped conventions in `.claude/rules/`, shared skills are symlinked into `.claude/skills/`, and enforcement hooks live in `.claude/settings.json`. The controller accepts `--tool claude` with Codex as the unchanged default, and `scripts/gate.py` provides a fast and a full gate for every tool. Delegated review, verifier and exploration roles pin a model alias so planning sessions on a larger model do not spend its quota on delegated work; main-session roles inherit the launched model, preserving the inheritance agreement above. The [usage guide](development/claude-code.md) and the [decision record](development/2026-09-17-claude-code-support.md) carry the details and the dated evidence.
