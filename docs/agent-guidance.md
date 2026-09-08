# Development agent guidance

The September 7, 2026 follow-up audit checked the project through `7eef2c3`, including the final application changes in `55e7f14`, against freshly fetched [GPT-6 Astra model guidance](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra). It covers development instructions for the expanded everyday Git workflow, Projects, Settings, themes, and configurable columns. The original audit covered `c1808d4`; this review updates that setup for the current code. Native behavior and measured timings remain tied to their exercised revisions in [validation](validation.md).

## Instruction layout

| Source | Purpose |
| --- | --- |
| [Root AGENTS.md](../AGENTS.md) | Product boundaries, authorized scope, code routing, delegation, commits, and proportionate validation |
| [App AGENTS.md](../crates/app/AGENTS.md) | Native state transitions, explicit-operation routing, settings/columns, prepared presentation, and resource lifetimes |
| [Git core AGENTS.md](../crates/git-core/AGENTS.md) | Passive versus normal Git command policy, byte-safe working previews, operation outcomes, and semantic fixture checks |
| [gitturtle-performance](../.agents/skills/gitturtle-performance/SKILL.md) | On-demand review of passive reads, scheduling, caches, previews, and performance evidence |
| [gitturtle-native-qa](../.agents/skills/gitturtle-native-qa/SKILL.md) | On-demand project/working/settings interaction and local package validation |

This arrangement follows OpenAI's [customization guidance](https://learn.chatgpt.com/docs/customization/overview): keep persistent project rules small, place specialized contracts near their code, and load repeatable procedures as skills when relevant. The root explicitly routes app and Git-core work to their nested instructions so a task started from the repository root can find them. See [instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md) for Codex's directory-based loading behavior.

## Astra guidance applied here

The official model guide emphasizes initiative within scope, clear instruction priority, deliberate delegation, concise communication, and testing calibrated to the change. The audit translates these into project decisions:

- Distinguish passive inspection from user-triggered writes. The original read-only product boundary has been superseded by the user's everyday Git scope; development tests still mutate only disposable fixtures, and normal GitTurtle engineering commits remain authorized.
- Preserve the active objective and completed work when the user steers or resumes a task. Raise material decisions after preparing authorized, reviewable work.
- Assign bounded parallel work with explicit ownership and useful evidence. Keep Git-index integration and the shared native app under a single owner.
- Give agents current entry points instead of requiring every document to be loaded. Keep replaceable preview reads, accepted explicit operations, and app preference persistence distinct; describe their contracts beside the code.
- Distinguish app, Git-core, and decoder tests; use native checks for rendered behavior. Documentation edits validate instructions and links without triggering a release build. Stop after relevant checks pass unless a concrete concern remains.
- Keep historical benchmarks tied to their measured revisions and metrics. A small local sample is not a guarantee or a controlled before/after comparison.

These are project applications of the source guidance, not a verbatim model prompt or evidence that model quality has been benchmarked.

## Findings from the expanded-code audit

- Updated the root code map for working state, operation execution, project/settings forms, persistence, themes, columns, and Git workflow fixtures. Narrowed the performance-skill trigger to its actual scope.
- Added local Git-core instructions for the two command policies, literal paths, mutable previews, retained Git hooks/signing/filter behavior, and uncertain outcomes. Local Refresh cannot verify whether a timed-out push reached its remote.
- Corrected the blanket “Back performs no Git request” rule: Compare retains its file list, while returning from Working after history invalidation can reload changed-file metadata. Added root/list focus ownership, page guards for hidden editors, working-preview invalidation, canonical draft identity, and granular preference merging.
- Updated the performance skill to cover the working-preview trace and reproducible HEAD/index/worktree state. Cache limits are looked up in code instead of copied into instructions.
- Updated native QA for Settings/Projects return focus, working refresh races, draft identity, and uncertain pushes. A current development build is sufficient for ordinary interaction checks; release measurements and fresh packaging remain tied to those requests.

The two existing skills remain focused and useful. The new core instructions supply the missing durable contract without adding a third workflow skill or fixed agent persona.

## Delegation and model settings

Use task-specific subagents when there is independent work: Git/fixture review, worker/cache analysis, or native design review are useful boundaries. Give a worker the requested outcome, relevant paths, ownership, and checks; ask it to return changed files, evidence, and unresolved issues. Reviews can remain read-only while the coordinator edits. Avoid workers concurrently manipulating the shared Git index or desktop app.

There are no persistent role configurations to migrate. The current crate boundaries and focused skills supply the reusable guidance; fixed personas, a second orchestration framework, or mandatory multi-agent review on every edit would add maintenance without a demonstrated need. Add a custom role later if a repeated task needs distinct tools, permissions, or context that these skills do not provide.

GPT-6 Astra is the requested development model. Keep the user's selected model and reasoning settings in the host rather than installing project overrides that silently replace them. This audit adds no `.codex/config.toml`, changes no global skills/settings, and introduces no model calls into GitTurtle. The guide's API migration parameters concern API-backed software; this native Git client has no such integration.

## Maintaining the guidance

Revise the closest contract when implementation changes, update skill routing when workflows change, and remove superseded instructions rather than accumulating duplicate rules. Keep changing limits and benchmark results in code and validation records; check them at use time. Recheck the official model guide when upgrading the development model or when observed behavior warrants a prompting change.

The follow-up used independent code-contract and scenario reviews. The scenarios covered Settings/Escape focus, stale working previews after staging, uncertain push retries, a docs-only Astra audit, and another image format. They exposed routing and verification gaps before revision; they were instruction reviews, not executed product tests or a model-quality benchmark. Validation checked the six guidance files, 29 local links, Cargo package/command paths, 24 key code symbols, both skill frontmatters, and the final diff. No Rust suite, native checks, or benchmarks were rerun for these documentation changes. Runtime and performance evidence belongs in the validation record with its exercised revision.
