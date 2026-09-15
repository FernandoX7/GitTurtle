---
name: gitturtle-feature-work
description: Complete a coordinated GitTurtle feature or queued development task with explicit acceptance evidence and a controller-owned handoff. Use for feature contracts and runner work, not isolated copy edits.
---

# GitTurtle feature work

Read the assigned task contract and the [development workflow](../../../docs/development/README.md). Keep its outcome, owned paths, dependencies and evidence requirements fixed during an attempt. Read the affected guide from [root routing](../../../AGENTS.md#find-the-relevant-code); do not load every subsystem by default.

## Establish the acceptance boundary

Describe the observable result and choose the existing validation profile for the changed behavior. A core operation, integrated native workflow, measured improvement and verified package promise different outcomes. If the supplied contract omits a material acceptance requirement, return a concrete correction to the coordinator before dependent work; continue independent authorized work.

For an uncertain design or new dependency, use the [research template](../../../docs/development/research-template.md) to record the decision, current primary sources, compatibility and enforcement. Planning is conditional; ordinary implementation does not require an architecture agent or a research document.

When changing shared interfaces, retained state, persistence, scheduling or platform/dependency boundaries, use the [architecture review](../../../docs/development/architecture-review.md). Identify the existing owner, affected producer/consumer path and failure behavior before extending it. Include any necessary lifecycle, compatibility or resource acceptance in the task contract; a visible feature alone does not establish those properties. Keep concrete structural follow-ups distinct from reproduced defects and from the authorized feature queue.

## Implement and verify

Use bounded code ownership for independent workers. The coordinator integrates shared changes and owns the index; one owner operates the native app and package. Preserve the session's chosen model and effort.

Run focused checks while iterating and the [required combined gates](../../../AGENTS.md#validation) at integration. Use [performance](../gitturtle-performance/SKILL.md) and [native QA](../gitturtle-native-qa/SKILL.md) only for their affected paths. Check real Git results in disposable fixtures when semantics change. Do not add tests that repeat instruction wording or implementation details.

Give the independent verifier the task, candidate identity, diff and command/evidence paths. Evidence from an unchanged candidate can be reused; repeat checks for changed inputs, failures or concrete unresolved concerns. A missing native session or external attestation leaves the requirement open. It is not an implementation failure to repair blindly, and a reviewer cannot substitute a text verdict for required evidence.

## Hand off once

Return the changed behavior, intended files, actual checks/results, evidence identity and remaining requirements. In an unattended session, use the controller's supplied result schema and make no Git commits. A `ready` patch may still need planned external attestations after candidate creation; report those pending without claiming task acceptance. Use `blocked` when missing information or capability prevents preparing the patch itself. The controller owns candidate commits, acceptance state and retry bookkeeping. In interactive work, the coordinator creates the atomic Conventional Commit following [project commit discipline](../../../CONTRIBUTING.md#commit-cohesive-changes).

Workers never update their own pass status or weaken specifications, gates or agent policy during an attempt. The unattended controller protects every `AGENTS.md`, agent/skill definitions and controller/task-policy files; changes to them use an explicitly scoped interactive review. Preserve a failed candidate and its evidence for diagnosis. Use the runner's stop/resume procedure rather than resetting a user's checkout. Accepted candidates move by local fetch and fast-forward within private clones; source-branch integration remains the coordinator's work. Finish when the contract and relevant checks are satisfied; do not add another review cycle without a concrete concern.
