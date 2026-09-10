# Development agent guidance

The September 10, 2026 audit checked source through `8464987` against freshly fetched [GPT-6 Astra guidance](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra). It updates the September 8 audit for repository tabs, incremental history, GitHub collaboration, profiles, recovery/rewrite workflows, rich previews, accessibility and vendored patches. This concerns agents developing GitTurtle; the native client has no model integration. Product execution evidence remains tied to the source and build identified in [validation](validation.md).

## Instruction layout

| Source | Responsibility |
| --- | --- |
| [Root AGENTS.md](../AGENTS.md) | Shared product boundaries, routing, delegation ownership, model inheritance, commits and combined validation |
| [App guide](../crates/app/AGENTS.md) | Native module routing and shared state/lifetime contracts; conditional navigation, writes and presentation references |
| [Git core guide](../crates/git-core/AGENTS.md) | Operation modules and fixture families, passive versus explicit commands, captured targets, byte-safe paths and process outcomes |
| [Preview guide](../crates/preview/AGENTS.md) | Supplied-byte raster/document/diagram/model decoding, cancellation, resource restrictions and icon consumers |
| [Vendor guide](../vendor/AGENTS.md) | Local patch provenance, paired toolkit changes, consuming regressions and upstream retirement |
| [gitturtle-performance](../.agents/skills/gitturtle-performance/SKILL.md) | Affected scheduling/read/cache investigation; [measurement procedure](../.agents/skills/gitturtle-performance/references/measurement.md) for latency or resource claims |
| [gitturtle-native-qa](../.agents/skills/gitturtle-native-qa/SKILL.md) | Affected native workflows from the [validation matrix](validation.md#current-validation-guidance); conditional [bundle checks](../.agents/skills/gitturtle-native-qa/references/macos-package.md) |

[Codex instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md) follows the path from the repository root to the working directory. Tasks launched at the root therefore explicitly read the affected crate or vendor guide. Linked feature contracts are conditional references, loaded for the relevant work. The two skills remain in `.agents/skills` with normal implicit discovery and focused descriptions, following [official skill guidance](https://learn.chatgpt.com/docs/build-skills). A docs-only audit or pure core/decoder fixture change does not trigger native QA.

## Changes from this audit

- Updated app routes for the current modules, including nested GitHub transport/review/drafts, profile storage, repository identity and accessibility. Reused maintained feature contracts for those workflows instead of adding another instruction layer per feature.
- Expanded core routes for inspection, profiles/signing, worktrees/reflog, LFS downloads, rewrite review/publication and bounded pipe handling. Updated preview routes and model-camera/animation contracts to reflect the implemented decoder families.
- Added the vendor guide because the four dependency patches have distinct provenance, paired-change and validation requirements. Workspace tests cover consuming regressions but do not automatically execute the excluded packages' own test suites.
- Updated performance routing for history paging, retained tabs, secondary preview workers and image retirement. Added relevant measurement harnesses and native QA routing for the offline GitHub fixture and restoration of local app state.
- Removed the stale command-palette action count and refreshed its workflow categories. Replaced the earlier audit's obsolete module counts and feature inventory with this source-specific record.

## Model and agent choices

The [Astra guide](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra) recommends auditing conflicting instructions, unnecessary clarification pauses, delegation, writing style and excessive testing. The existing root agreements already cover these behaviors. They retain authorization and steering, reserve questions for material decisions, identify skill-induced blockers, assign bounded parallel work and stop after relevant validation. Product confirmation dialogs are product behavior; they do not add approval steps to authorized engineering work.

Keep the session's selected model and reasoning effort for workers. The model guide does not justify a repository-wide Ultra requirement or a silent model override. [Codex subagent documentation](https://learn.chatgpt.com/docs/agent-configuration/subagents) confirms inheritance when no override is configured and explains the coordination and token costs of parallel work. The project delegates by actual code ownership, with the coordinator responsible for shared integration, the index and final checks, and one owner operating the native app.

No persistent custom agents or project model overrides were present, and this audit adds none. The current specialization fits crate contracts and the two existing skills. Add a custom agent when recurring work needs a distinct context, tool or permission boundary; add a skill when a repeatable procedure cannot fit an existing contract or skill. Global configuration and unrelated installed skills were outside this repository audit.

## Validation and maintenance

Validation passed across 14 guidance documents: 154 local links/anchors, 125 literal crate paths, all 57 top-level app module routes, every core fixture route, three Cargo package names and the integrated diff. Both skills passed the bundled `quick_validate.py`; referenced fixture/package options were checked against their scripts. A separate routing review exercised README copy, model-camera/tab diagnosis, offline GitHub recovery and upstream toolkit replacement. It caught an overbroad shared-camera rule, now qualified to preserve explicit independent cameras. These checks establish instruction structure and correspondence to code; they are not a model-quality benchmark or a new product execution record. No Rust source or dependencies changed, so no builds, native sessions or benchmarks were rerun.

When implementation changes, update the closest contract and current validation row. Keep numeric limits in code and measured results in dated benchmark records. Recheck official model guidance when the development model changes or observed agent behavior warrants a correction; avoid adding a permanent rule for an isolated example.
