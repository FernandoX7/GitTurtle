---
name: research
description: Verify a decision that depends on current facts (library versions, APIs, platform rules, tool behavior, pricing) against live primary sources and record a dated note with docs/development/research-template.md. Use before choosing or upgrading a dependency, adopting an API, or changing a development procedure.
---

# Research

Training data is not a source. Verify against live primary sources and write the result down where the next session will find it.

1. State the decision in one sentence, the user-visible outcome, and what would change the answer.
2. Check primary sources: official documentation, changelogs and releases, the crates.io or registry entry (`cargo info <crate>` or the registry page for version, MSRV and features), and the vendor's own page for platform or pricing facts. Prefer the newest stable version unless a documented incompatibility exists, and record the incompatibility when you deviate.
3. Write `docs/development/YYYY-MM-DD-<topic>.md` following `docs/development/research-template.md`: decision and scope, an evidence table with checked dates and versions, compatibility and alternatives, enforcement and acceptance, and the outcome once implemented. Name files and symbols with their revision; keep credentials, personal paths and proprietary content out.
4. Link the note from the contract it affects, and when it changes an upcoming task say so in `docs/development/HANDOFF.md` so the next session does not re-litigate it.
