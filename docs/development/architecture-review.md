# Architecture review

Use this when a feature or refactor changes shared interfaces, state ownership, persistence, scheduling, platform behavior or dependencies. For a local change within an established contract, apply that contract directly. The coordinator and existing implementer/verifier roles own this review; a separate agent or design document is optional.

The [product architecture](../architecture.md) describes current behavior. This guide explains how to extend it while keeping changes understandable and independently testable.

## Choose the owner before adding behavior

Trace the affected user action through its captured target, background work, owned result and visible outcome. For a Git write, include preparation, revalidation at execution, and the state shown after a partial or uncertain result. Locate the closest existing implementation before introducing another path.

| Responsibility | Existing owner | Review boundary |
| --- | --- | --- |
| Git identity, reads, prepared writes and repository preservation | [Git core](../../crates/git-core/AGENTS.md) | Keep authoritative target checks here even when the UI already validates a form. No GPUI dependency. |
| Parsing and rendering supplied document/model bytes | [Preview](../../crates/preview/AGENTS.md) | Return owned data with explicit limits and unsupported cases; filenames do not grant filesystem or network access. |
| Selection, presentation and task lifetime | [App](../../crates/app/AGENTS.md) | Views consume prepared models. Extend the correct read, write or renderer owner instead of starting a parallel execution path in a click/render callback. |
| GitHub collaboration | [Provider contract](../github-collaboration.md) | Provider identity, credentials, local drafts and ambiguous remote outcomes remain distinct from Git operations. |
| Saved user state | [Persistence contract](../../crates/app/docs/writes-and-persistence.md#preferences-and-commit-drafts) | Name the store, format, writer scope and recovery behavior. Atomic replacement alone does not serialize concurrent writers. |
| Toolkit/platform adaptation | [Vendor guide](../../vendor/AGENTS.md), [Linux runbook](../linux.md) | Keep platform-specific behavior behind its existing boundary, with explicit fallback and patch provenance. |

Keep APIs narrow enough that callers cannot bypass the invariant they are meant to preserve. Group state that changes together and put its transitions beside it. A new module should own a cohesive responsibility and a useful interface; moving another `impl GitTurtle` into a file does not by itself reduce shared-state coupling. Introduce a trait, generic layer or crate when it resolves an actual dependency or substitution need.

## Make lifetime and resource ownership explicit

For new retained state, identify whether it belongs to a window, repository, worktree, tab, inspection or durable store. Specify what happens on replacement, cancellation, tab switch, close, cold restore and repository replacement where applicable. In particular, adding a field to [GitTurtle](../../crates/app/src/main.rs) can also affect [warm-tab capture/restore](../../crates/app/src/repository_tabs.rs), inspection retention and byte accounting.

For asynchronous work, name the executor, concurrency/queue bound, captured identity and stale-result check. Cancellation must account for underlying work; dropping a receiver only stops observing its reply. Accepted writes have different semantics and must survive view changes. The [app ownership map](../../crates/app/AGENTS.md) distinguishes window-owned executors from shared renderer lanes and application observers.

Account for all retained owners and expansion stages when adding caches or formats: input bytes, decoded models, UI references and GPU resources have different lifetimes. Derive limits from the actual contract and measurements. Keep overload, unsupported data and partial results explicit. Use the [performance procedure](../../.agents/skills/gitturtle-performance/SKILL.md) when the changed path warrants it.

## Preserve compatibility and recoverability

For a persisted-format change, inspect existing defaults/version handling and the real load/save consumers. Cover applicable older, missing, malformed and oversized records; keep unreadable user state available for recovery. Decide how concurrent updates and shutdown are handled before claiming durable saves. Credentials belong in the existing credential boundary, and diagnostics/evidence must remain sanitized.

For a dependency or platform change, record the concrete capability needed, existing alternatives, supported targets, licensing and local patches. Use the [decision research template](research-template.md) when the tradeoff needs a durable explanation. Preserve a clear unsupported state on other platforms and tie package claims to the actual compiled artifact.

## Review the change, including its failure paths

The verifier assesses both the requested outcome and the affected architecture contracts. A task can satisfy its visible acceptance steps while introducing a second write path, losing a retained state owner or removing a refusal guard; report that concrete regression with its source and consequence.

Choose evidence for the boundary changed:

- Pure models/parsers: observable outputs, representative edge inputs and refusal boundaries.
- Cross-module behavior: exercise the real producer and consumer, including relevant stale results, cancellation, overload or state restoration.
- Git mutations: disposable repository fixtures asserting intended effects and unrelated state preservation, as routed by the core guide.
- Native behavior, performance and packaging: the existing conditional skills and current validation matrix; a compile or structural test cannot substitute for those observations.

Tests should expose the contract to a future implementation. Refactoring tests may characterize behavior that must survive; correct known wrong behavior against the intended contract. Do not equate a source-text assertion, line-count threshold, coverage percentage or new interface with architectural quality.

## Improve structure in reviewable increments

Extract a cohesive responsibility when the affected code requires unrelated state to change together, repeats a lifecycle protocol, or makes the behavior hard to test through a stable interface. Explain which dependency or invariant the extraction improves. Keep independently reviewable structural and behavioral changes in atomic Conventional Commits; a necessary small extraction can accompany its feature.

Remove superseded paths and update their callers, contracts and evidence routes. For material work left outside the task, record the source, consequence, proposed next increment and acceptance evidence. Distinguish a reproduced defect from a code-review risk or a design opportunity. The [September 15 audit](2026-09-15-maintainability-audit.md) is a dated starting point, not an automatically authorized queue or a release certification.
