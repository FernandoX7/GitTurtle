# Maintainability audit — September 15, 2026

Reviewed product source at `dcf8bd271638a8d834168a96ac20138c9131cb6d` and its existing agent/skill contracts. This audit updates guidance and identifies concrete engineering follow-ups. Source inspection establishes the ownership and routing findings below; it does not establish new runtime, performance or release results.

## Existing foundations

The workspace separates Git operations, supplied-byte preview decoding and native presentation. Typed writes, captured targets, bounded workers, explicit unsupported states and disposable Git fixtures already provide useful boundaries. Existing feature state types and scoped contracts should remain the starting points for growth.

Three recurring agent roles remain sufficient for the maintained workflow: implementation, independent verification and documentation reconciliation. Their updated instructions apply the [architecture review](architecture-review.md) to shared state, interfaces, persistence, scheduling, platforms and dependencies. Architecture quality requires implementation and evidence at those boundaries; role configuration alone cannot establish it.

## Guidance corrections

- Update the product overview for current document/3D decoding, Linux integration, and the distinction between window-owned executors, shared renderers and application observers.
- Route current app modules, core watch policy, GLB submodules and toolkit patches from their owning guides. Keep current behavior separate from dated validation evidence.
- Make lifetime, compatibility, error recovery and resource ownership part of feature definition and independent review where affected.
- Keep native/package QA conditional on the actual platform and candidate. Preserve the performance skill's distinction between measured paths and unmeasured claims.
- Extend the guidance checker to cover product architecture and scoped crate contracts. Wire the existing [installer recovery suite](../../scripts/test-install-linux.py), including real-bundle cases, into the [Linux quality job](../../.github/workflows/quality.yml); its prior single-install check did not execute those regressions. This configures the next hosted run and is not evidence of an executed Linux pass.

## Proposed implementation increments

These are source-grounded follow-ups, not completed fixes or entries in the active feature queue. Recheck the identified source before starting an increment.

| Priority and finding | Evidence and consequence | Bounded next increment and acceptance |
| --- | --- | --- |
| Release correctness: macOS executable selection | [package-macos.sh](../../scripts/package-macos.sh) builds with a fixed target directory but no explicit target, then copies `target/<profile>/gitturtle`. A configured build target changes Cargo's artifact path; an older host executable can remain at the copied path. This is a source-review risk, not a reproduced packaged failure. | Make build target and artifact selection agree, and verify embedded build identity. Add a disposable script fixture with a conflicting Cargo target and stale executable, then exercise the intended macOS package/resource/signature path. [Linux packaging](../../scripts/package-linux.sh) already pins its target. |
| Maintainability: retained workspace ownership | [GitTurtle](../../crates/app/src/main.rs) owns substantial history, working-copy and presentation state. [WarmTab](../../crates/app/src/repository_tabs.rs), capture/restore and retention accounting manually transfer related fields. A new feature must update several lifecycle paths consistently. This is a design pressure, not proof of a current retention bug. | Extract one cohesive state owner with capture/restore/reset and accounting at the same boundary. Preserve existing warm/cold-tab, inspection, focus and viewport behavior with its current tests and affected native checks. Avoid a whole-app rewrite or moving fields solely to reduce file length. |
| Maintainability: worker policy and content preparation | [worker.rs](../../crates/app/src/worker.rs) combines scheduling/session/cache ownership with dispatch and nontext/LFS/text preparation. Its `execute`, `supplied_nontext_content`, `nontext_history_content` and text helpers span several consumers. | Extract a coherent preparation boundary when that path next changes, keeping queue/cancellation/session ownership explicit. Exercise immutable history, mutable working content, absent sides, LFS and resource accounting through the real worker. Preserve the read/write separation; do not introduce another executor to make the file smaller. |
| Responsiveness: object-reader contention | [GitRepository::read_object](../../crates/git-core/src/lib.rs) holds its shared reader mutex across an object request. [Core deadlines](../../crates/git-core/README.md#budgets-and-behavior) do not establish an end-to-end interaction deadline; waiting for another caller and multi-read preparation matter. This is a known boundary to measure, not a demonstrated regression. | Profile a representative competing-reader interaction before changing ownership. If it violates the task's latency requirement, bound or cancel the waiting path while preserving cat-file protocol integrity, repository identity and passive-read semantics. Record queue wait separately from read/decode time. |

The packaging inference follows Cargo's documented [build target configuration](https://doc.rust-lang.org/cargo/reference/config.html#buildtarget) and [target-specific output layout](https://doc.rust-lang.org/cargo/reference/build-cache.html), checked September 15, 2026. Package identity remains necessary even when compilation exits successfully.

Independent routing scenarios also found an outdated GLB summary in [document previews](../document-previews.md), corrected here. Current camera bookmarks do not serialize animation clip/time selection; cold playback-state restoration would be new feature scope. The scenarios confirmed that a README sentence change requires neither architecture planning nor native/performance work.

## Validation scope

Validate this guidance through local links/anchors, current source symbols and commands, agent TOML and skill metadata, plus independent routing scenarios. Run the development-tool suite for the checker change and parse the edited workflow/its shell commands. No product Rust, dependency or packaging implementation changes are made by this audit, so it does not require a native build or rerunning the Rust suite. Future implementation increments use their affected core, native, performance and package contracts; this record must not be presented as evidence that those increments passed.
