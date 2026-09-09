# Development agent guidance

The September 8, 2026 audit checked the repository through `303f78a` against freshly fetched [GPT-6 Astra guidance](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra), including the newer authentication, attribution, staging, recovery, search, tags, image comparison, and macOS work. This concerns agents developing GitTurtle; the native client has no model integration. Product verification remains tied to the source and build identified in [validation](validation.md).

## Instruction layout

| Source | Responsibility |
| --- | --- |
| [Root AGENTS.md](../AGENTS.md) | Shared product boundaries, code routing, agent ownership, model inheritance, commits, and final validation |
| [App AGENTS.md](../crates/app/AGENTS.md) | Native module routing and cross-cutting state/lifetime contracts; links to feature-specific contracts loaded for the affected work |
| [Git core AGENTS.md](../crates/git-core/AGENTS.md) | Current operation modules and fixture suites, passive/read versus explicit/write policy, captured plans, byte-safe paths, cancellation, and uncertain outcomes |
| [Preview AGENTS.md](../crates/preview/AGENTS.md) | Supplied-byte decoding, allocation limits, orientation/alpha, SVG restrictions, and icon consumers |
| [gitturtle-performance](../.agents/skills/gitturtle-performance/SKILL.md) | Investigation of the affected read/scheduling/cache path; [measurement procedure](../.agents/skills/gitturtle-performance/references/measurement.md) only for timing or resource claims |
| [gitturtle-native-qa](../.agents/skills/gitturtle-native-qa/SKILL.md) | Native input/presentation checks selected from the [current workflow matrix](validation.md#current-validation-guidance); [package procedure](../.agents/skills/gitturtle-native-qa/references/macos-package.md) when validating a bundle |

[Codex instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md) follows the path from the repository root to the working directory. A task launched at the root must therefore explicitly read the affected crate's guide. The app's linked contracts are conditional references, not automatically discovered instruction files. Shared rules remain in the root; feature details belong in the nearest guide or its references.

The two project skills remain in `.agents/skills`, with normal implicit discovery. Their descriptions identify the relevant work, and their entrypoints link conditional procedures. This applies the [official skill guidance](https://learn.chatgpt.com/docs/build-skills) on focused triggers and progressive disclosure. A pure core fixture change, a decoder-only edit, or an instruction audit should not activate native QA merely because the repository contains a native app.

## What changed

- Replaced the root's older file list with routes to maintained crate guides and the current workflow/CI sources. Reviews now follow the same local-contract routing as edits, including across the core model, worker result, and native consumer.
- Reduced the app's always-loaded instructions from 2,155 to 495 words, with routes for all 32 source modules. Navigation/refresh, writes/persistence, and presentation details remain available through explicit topic links.
- Expanded core routing beyond `lib.rs`, `work.rs`, and two fixture suites to cover every current service and integration-test family. Added prepared-plan revalidation, pinned search/attribution semantics, private/shared Git directories, transient authentication, explicit cancellation, and uncertainty after writes.
- Updated performance guidance for the shared history/search/attribution worker, quiet-refresh deferral, separate stash readers, and the difference between obsolete-read cancellation and explicit operation Cancel. Measurement guidance distinguishes backend and native frame evidence.
- Updated native QA for current staging/recovery/authentication/search/blame/tag/ignore workflows through the maintained validation matrix, and for split diff/Find and Overlay/Wipe input. Moved bundle identity and packaging steps into their own reference. Corrected stale design text that still described the implemented Overlay/Wipe modes as future work.
- Retained the preview guide after checking its decoder and layered icon consumers. Removed the obsolete audit claim that packaging merely copies ICNS: the current package script compiles the layered icon even with `--no-build`.

## Astra and delegation choices

The [Astra model guide](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra) identifies sensitivity to conflicting instructions, unnecessary clarification pauses, under-delegation, verbose writing, and excessive verification as behaviors to tune. The project uses short scope and ownership rules: finish authorized work, incorporate steering, ask only material questions, identify the exact instruction behind a skill-induced pause, and stop once the relevant checks pass. Product confirmation dialogs remain product behavior; they do not require agents to ask again before routine authorized engineering work.

Use the selected session model and effort for workers. Astra is the requested model for this audit; examples naming an older model in general documentation do not change that target. Preserve explicit user cost and effort choices. The guide says Astra does not support `none` and recommends starting at `low` when migrating an API workload from `none`/`minimal`; that is not a reason to override this session or impose Ultra on every task.

[Codex subagent guidance](https://learn.chatgpt.com/docs/agent-configuration/subagents) supports bounded parallel exploration and implementation, while noting the added token cost and coordination needed for concurrent edits. Useful GitTurtle divisions follow actual dependencies: core semantics and fixtures, read-worker/resource analysis, and native state/presentation. Delegate independent work with owned files or read-only scope; return concise findings with source references and checks. Integrate shared types and cross-crate call sites sequentially. The coordinator owns the index, commits, and combined validation; one owner operates the shared native app and packages it.

No persistent custom agents or project model overrides were present, and this audit adds none. No demonstrated workflow needs a separately configured tool or permission boundary; task-specific ownership plus the two skills provides the necessary specialization. Add a custom agent when recurring work demonstrates a distinct context, tool, or permission requirement. Add a skill for a repeatable procedure that does not already belong in a crate contract or an existing skill. Global configuration and unrelated installed skills are outside this repository audit.

## Validation and maintenance

Validation passed for 85 local links/anchors across 12 guidance files, 67 crate source/test paths, all 32 app module routes, and the three Cargo package names. Both skills passed the skill-creator's `quick_validate.py`; script options and the integrated diff were checked. An independent routing review exercised README copy, authentication Cancel, retained inspection focus, packaged icon, and Overlay latency scenarios. It found the stale design sentence corrected above. These checks validate instruction structure and code correspondence; they are not a model-quality benchmark or a new product execution record. No Rust source or dependencies changed, so the audit does not rerun builds, Rust tests, native interaction, or benchmarks.

When implementation changes, update its closest contract and current validation row. Keep numeric limits in code and measured results in dated benchmark records. Recheck the official model guide when the development model changes or observed agent behavior warrants a correction; avoid adding a permanent rule for an isolated example.
