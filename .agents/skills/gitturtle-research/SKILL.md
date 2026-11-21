---
name: gitturtle-research
description: Verify decisions depending on current external facts (crate versions, APIs, platform behavior, tool versions, CI limits) against live primary sources and record a dated note with docs/development/research-template.md. Use before upgrading dependencies, adopting new APIs, or changing engineering workflows.
---

# Primary-source research

Training data is not a source. Verify against live primary sources and record durable facts where future sessions can rely on them.

1. State the decision in one sentence, the user-visible outcome, and what evidence would change the answer.
2. Inspect primary sources directly: official docs, release changelogs, registry metadata (`cargo info <crate>` or crates.io), and vendor documentation for OS/CI limits. Prefer the newest stable version unless a concrete incompatibility exists, and record that incompatibility when deviating.
3. Record non-trivial research in `docs/development/YYYY-MM-DD-<topic>.md` using [research-template.md](../../../docs/development/research-template.md): decision and scope, evidence table with checked dates and exact versions, compatibility and alternatives, acceptance criteria, and final outcome. Pin exact inspected commits/revisions and raw URLs; keep credentials, personal paths, and proprietary data out. When web search is unavailable, `gh api` (including `-H "Accept: application/vnd.github.raw"`) and `curl` against official endpoints are acceptable read-only primary-source reads.
4. Link the note from the relevant guide or ADR, and reflect any active next-step impact in [STATE.md](../../../docs/development/STATE.md) (strictly `<= 60` lines).
