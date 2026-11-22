# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-22
- **Latest green `main`:** `f30232be` (PR #241 merged green: Wave 0A safety, PDFKit `objc2`, virtualized-list panics, diff timeout, credential/bidi redaction, CI/tooling fixes).
- **Active branch / PR:** `feat/harness-ci-bug-sweep` (Roadmap PR 2 of 10 — Engineering harness v2, free-tier CI upgrades, and functional/UX/a11y bug sweep).
- **Completed in PR 2:**
  - Engineering harness v2: `scripts/dev/check_invariants.py` wired into `gate.py fast`, `.githooks/pre-push`, and CI `changes`; AST CLI smoke checks & ADR index validation in `scripts/dev/guidance.py`; widened `docs/doc-manifest.json` & `scripts/check-docs-sync.py`; offline reviewer eval fixtures (`scripts/dev/reviewer_evals/`); `worktree.py list|status|env` + `build_slot.held_slot` integration; deterministic P-core release benchmark driver `scripts/dev/bench_compare.py`.
  - Free-tier CI upgrades: weekly `schedule` routed to 6-job `cache_seed_plan` (`seed-only=true`, 2 macOS slots); `setup-rust` exact-hit `CI_RUST_CACHE_HIT=true` skips `finish` bound/save; `CARGO_PROFILE_TEST_DEBUG=line-tables-only` across debug/clippy/Arch legs (-32% local test compile); pinned offline `zizmor v1.32.0` in `changes`; stdlib CycloneDX 1.5 SBOM (`scripts/generate-sbom.py`) + pinned SLSA `actions/attest-build-provenance` in `release.yml` `publish`; updated `url` to `2.5.9` (`RUSTSEC-2026-0592`).
  - Core, data, copy, menu & LFS fixes: node-only `SearchHistory` (no spurious banner); `--exclude=refs/stash` on `AllRefs`; `About GitTurtle` in macOS menu bar; fixed Linux/shortcut copy leaks; cross-process file lock on `github-drafts.json`; coalesced preference writes; 8-tab pre-check before Clone/Create touches disk; deduplicated `[user]` config writes; `statfs` unstable-inode handling for FAT/exFAT/FUSE/SMB; LFS >32 MiB rich-fallback, pointer probing, refresh & stash display; quiet-refresh index-OID recheck and line-based caret restore.
  - Input, overlay, Mermaid, GitHub PR & a11y fixes: dismissed open `PointerMenu` on dialogs/Palette/page switches; weak `FocusRing` handles + Compare text-mode collapse & GitHub `Panel::refresh` focus transfer; visible/announced busy refusal on `Ctrl+P`/`Ctrl+Shift+C`; `Return` in rebase base field loads commits; shared clamped nested wheel scroll chaining; `gpui-base` Slider custom-range `SetValue` unmapping & 3D playback `SetValue`; `accesskit_atspi_common` `Drop` `RemoveAccessible` + teardown; Mermaid `Link`/`Click` node edge fix; GitHub PR soft-wrapped descriptions & no empty review draft on close.
- **Next concrete steps:**
  1. PR 3 — Runtime performance & memory quick wins (`paint_quad` straight graph lanes, in-place BGRA image swizzle, reduced `git` spawns, lazy Split/Settings, event-driven waits, `memfd_create` helper transfer) with P-core release benchmarks via `bench_compare.py`.
  2. PRs 4–10 — Architecture modularity & fuzz/Miri, large-repo scalability, daily-driver Git power, syntax/diff/GitHub v2/polish, macOS native craft, Omarchy/Linux craft + packaging prep, and vendor diet/website readiness.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12`, OS close/`Quit` refusal while a Git write runs, VoiceOver/Orca Slider `SetValue`, and `About GitTurtle` in macOS menu bar.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, `website/` Cloudflare deploy, and opt-in auto-update design.
