# Development agent guidance

The September 8, 2026 audit checked the project through `2073aeb`, including the design and performance changes in `b8b5799` and action feedback in `ca8d805`, against freshly fetched [GPT-6 Astra guidance](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra). It updates the September 7 audit for the larger native client. This concerns agents developing GitTurtle; the application itself has no model integration. Runtime and timing evidence remains tied to its exercised revision in [validation](validation.md).

## Instruction layout

| Source | Purpose |
| --- | --- |
| [Root AGENTS.md](../AGENTS.md) | Product boundaries, code routing, delegation and model inheritance, commits, and proportionate validation |
| [App AGENTS.md](../crates/app/AGENTS.md) | Native state transitions, operation targets, theme tokens, shared layout, prepared presentation, and resource lifetimes |
| [Git core AGENTS.md](../crates/git-core/AGENTS.md) | Passive versus normal Git policy, byte-safe paths, object-reader recovery, local LFS, and operation outcomes |
| [Preview AGENTS.md](../crates/preview/AGENTS.md) | Bounded decoding, alpha/orientation, SVG resource restrictions, and icon generation |
| [gitturtle-performance](../.agents/skills/gitturtle-performance/SKILL.md) | On-demand review of passive reads, scheduling, caches, previews, and measured performance |
| [gitturtle-native-qa](../.agents/skills/gitturtle-native-qa/SKILL.md) | On-demand native interaction and local package validation |

This follows OpenAI's [customization guidance](https://learn.chatgpt.com/docs/customization/overview): keep persistent rules small, place specialized contracts close to their code, and use skills for repeatable procedures. Codex's [instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md) walks from the repository root to the working directory. The root therefore explicitly routes work into each crate's instructions even when the task starts at the root. The two skills remain in the discoverable repository `.agents/skills` directory, with focused descriptions and references loaded as needed, consistent with [skill guidance](https://learn.chatgpt.com/docs/build-skills).

## Changes from this audit

- Added preview-local guidance for supplied-byte decoding, independent bounds, straight RGBA, alpha-aware resizing, orientation, and self-contained SVG handling. The icon pipeline now states that packaging copies ICNS and does not regenerate it.
- Updated app contracts for constructing only the active page, persistent Git actions with optional Targets fields, lazy bounded branch choices, and checkout callbacks bound to their opening repository. Preserved the distinction between browsing a branch's history and checking it out.
- Recorded the theme failure found during native work: GPUI Kit control backgrounds use resolved tokens, so updating colors also requires rebuilding tokens before syncing the base theme. Added semantic status labels/icons, aligned column insets, and stable inspector identity.
- Added core guidance for discarding a desynchronized shared object-reader process, paging from immutable OIDs versus mutable refs, and local LFS size/digest checks with retryable missing content.
- Updated the performance procedure for per-row graph cancellation, scratch reuse, lazy hidden pages, bounded menus, and the distinction between an isolated graph harness and native frame measurements.
- Updated native QA for actual button foreground/background colors, branch target and filter behavior, and selected icon resources. Artwork-only packaging can reuse a verified existing executable; embedded control SVGs require a rebuild. Documentation-only work still does not trigger Rust or native checks.

Existing contracts for accepted writes, uncertain outcomes, stale preview replies, canonical draft identity, granular preference merging, and literal patch copying remain applicable. The audit removes duplicated validation commands and replaces stale instructions rather than introducing another copy of the model prompt.

## Delegation and model choices

The [Astra guide](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra) calls for initiative within scope, clear instruction priority, useful delegation, concise communication, and testing calibrated to the change. Here that means completing authorized work, incorporating user steering, raising only material questions, and explaining the exact instruction if a skill causes a pause. Once relevant checks pass, additional review needs a concrete unresolved concern.

Use independent subagents for useful boundaries such as Git semantics/fixtures, worker/graph analysis, and native presentation. Give each worker an outcome, owned files or read-only scope, and expected evidence; have it return concise findings, changed files, checks, and unresolved issues. The coordinator integrates results and owns the Git index. One owner operates the shared desktop app and packages it. These boundaries match the code and reduce competing edits and duplicate expensive checks.

[OpenAI's subagent documentation](https://learn.chatgpt.com/docs/agent-configuration/subagents) describes model/effort inheritance and the additional token cost of parallel workers. Preserve the user's selected model and reasoning effort, including explicit cost limits. GPT-6 Astra is the model requested for this guidance; generic examples naming another model do not replace that choice. Do not force Ultra, a fixed worker count, or automatic effort changes. When the user requests a different configuration, use the host's supported model/effort combinations and assess task quality, latency, and cost. The Astra guide does not support `none`; its API migration advice for `none`/`minimal` starts at `low`, rather than silently increasing all work to maximum effort.

No persistent custom agent configurations exist in this repository. Task-specific delegation and the two skills cover the demonstrated workflows, so this audit adds no fixed personas, project model overrides, or third design skill. Design intent belongs in [design.md](design.md), implementation contracts beside the code, and rendered verification in native QA. A custom role is warranted later if recurring work requires a distinct tool, permission, or context boundary. No global configuration or installed skills were changed. API migration flags, async tools, and model routing are not application features to add to this native Git client.

## Validation and maintenance

Independent workers traced app, core/decoder, and skill contracts to current code. Review scenarios included changing a theme without refreshing button tokens; switching repositories with a branch menu open; cancelling graph layout; adding an image format; replacing only the packaged icon; and doing another documentation-only audit. These are instruction and code-contract reviews, not a model-quality benchmark or newly executed product workflows.

Validation resolved 54 local links across nine guidance/asset/validation documents, checked named code symbols and all three Cargo packages, and passed both skills through `quick_validate.py`. The integrated instructions passed a final review of documentation-only, bundle-resource, and embedded-asset routing, plus the diff check. The accompanying user-selected icon change has separate package evidence in [validation](validation.md); earlier Rust tests and timings were not rerun or attributed to this documentation revision.

Update the closest contract when implementation changes, keep changing limits in code and raw timings in benchmark records, and recheck the official model guide when the development model changes or observed agent behavior warrants a correction.
