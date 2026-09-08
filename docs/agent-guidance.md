# Development agent guidance

The original September 7, 2026 audit covered the implementation through `c1808d4` against the [GPT-6 Astra model guidance](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra). The later authorized product scope adds everyday Git writes, Projects, Settings, themes, and configurable columns; the guidance below now routes those features while retaining the audit's instruction design. Application behavior and validation evidence remain in their existing documents. This update is not a new model audit or a claim that the expanded native workflows have passed validation.

## Instruction layout

| Source | Purpose |
| --- | --- |
| [Root AGENTS.md](../AGENTS.md) | Product boundaries, authorized scope, code routing, delegation, commits, and proportionate validation |
| [App AGENTS.md](../crates/app/AGENTS.md) | Native state transitions, explicit-operation routing, settings/columns, prepared presentation, and resource lifetimes |
| [gitturtle-performance](../.agents/skills/gitturtle-performance/SKILL.md) | On-demand review of passive reads, scheduling, caches, previews, and performance evidence |
| [gitturtle-native-qa](../.agents/skills/gitturtle-native-qa/SKILL.md) | On-demand project/working/settings interaction and local package validation |

This arrangement follows OpenAI's [customization guidance](https://learn.chatgpt.com/docs/customization/overview): keep persistent project rules small, place specialized contracts near their code, and load repeatable procedures as skills when relevant. The root explicitly routes app work to its nested instructions so a task started from the repository root can find them. See [instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md) for Codex's directory-based loading behavior.

## Astra guidance applied here

The official model guide emphasizes initiative within scope, clear instruction priority, deliberate delegation, concise communication, and testing calibrated to the change. The audit translates these into project decisions:

- Distinguish passive inspection from user-triggered writes. The original read-only product boundary has been superseded by the user's everyday Git scope; development tests still mutate only disposable fixtures, and normal GitTurtle engineering commits remain authorized.
- Preserve the active objective and completed work when the user steers or resumes a task. Raise material decisions after preparing authorized, reviewable work.
- Assign bounded parallel work with explicit ownership and useful evidence. Keep Git-index integration and the shared native app under a single owner.
- Give agents current entry points instead of requiring every document to be loaded. Keep replaceable preview reads, accepted explicit operations, and app preference persistence distinct; describe their contracts beside the code.
- Distinguish app, Git-core, and decoder tests; use native checks for rendered behavior. Documentation edits validate instructions and links without triggering a release build. Stop after relevant checks pass unless a concrete concern remains.
- Keep historical benchmarks tied to their measured revisions and metrics. A small local sample is not a guarantee or a controlled before/after comparison.

These are project applications of the source guidance, not a verbatim model prompt or evidence that model quality has been benchmarked.

## Delegation and model settings

Use task-specific subagents when there is independent work: Git/fixture review, worker/cache analysis, or native design review are useful boundaries. Give a worker the requested outcome, relevant paths, ownership, and checks; ask it to return changed files, evidence, and unresolved issues. Reviews can remain read-only while the coordinator edits. Avoid workers concurrently manipulating the shared Git index or desktop app.

There are no persistent role configurations to migrate. The current crate boundaries and focused skills supply the reusable guidance; fixed personas, a second orchestration framework, or mandatory multi-agent review on every edit would add maintenance without a demonstrated need. Add a custom role later if a repeated task needs distinct tools, permissions, or context that these skills do not provide.

GPT-6 Astra is the requested development model. Keep the user's selected model and reasoning settings in the host rather than installing project overrides that silently replace them. This audit adds no `.codex/config.toml`, changes no global skills/settings, and introduces no model calls into GitTurtle. The guide's API migration parameters concern API-backed software; this native Git client has no such integration.

## Maintaining the guidance

Revise the closest contract when implementation changes, update skill routing when workflows change, and remove superseded instructions rather than accumulating duplicate rules. Keep changing limits and benchmark results in code and validation records; check them at use time. Recheck the official model guide when upgrading the development model or when observed behavior warrants a prompting change.

The original audit checked file/symbol references, Cargo package names, local Markdown links, and both skill frontmatters. An independent code review checked the architectural fit; a separate scenario review checked task routing and validation choices. Those historical checks validate that instruction setup, not the new workflows, native runtime behavior, or model performance. Current implementation checks belong in the validation record with their exercised revision.
