# Decision research template

Use this template when uncertainty about an interface, dependency, platform behavior or development procedure materially affects a task. Save completed notes with a descriptive date and title; link them from the relevant contract. Routine fixes do not need a research note.

## Decision and scope

- Question to resolve and user-visible outcome.
- Current recommendation and why it fits GitTurtle.
- What is outside this decision.

## Evidence

| Source | Checked date/version | Relevant finding | Limit |
| --- | --- | --- | --- |
| Primary documentation, source, or local command record | Date and revision/version | What was observed | What it does not establish |

Link directly to current primary sources. For code, name the file/symbol and revision. Record actual command results separately from proposed checks. Do not put credentials, personal paths or proprietary repository content into public notes.

## Compatibility and alternatives

- Relevant Rust, Git, GPUI, OS or Codex versions and existing pinned dependencies.
- Alternatives and their concrete tradeoffs.
- Existing contracts, licensing/provenance or resource limits affected.
- Evidence that could change the recommendation.

## Enforcement and acceptance

- Implementation owners and affected contracts.
- Existing or necessary regression, native, measurement or package evidence.
- Where the requirement is enforced: source guard, validator, fixture, CI job or operator procedure.
- Remaining uncertainty, required capability and the next check that can resolve it.

## Outcome

Record the adopted decision and source/evidence identity after implementation. Preserve earlier observations as dated evidence; do not relabel them as validation of newer code.
