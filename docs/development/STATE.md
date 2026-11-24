# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-24
- **Latest `main`:** `07a1492d` — Pre-1.0 Polish Wave PRs 1–3 merged (`#254` release binary size diet `142.94 -> 106.83 MiB` / `-25.3%`; `#255` Shift-click history range compare & interactive rebase drag-reorder; `#256` Linux Wayland/X11 IME & AT-SPI accessibility parity + macOS `waitid`/spawn hardening).
- **Active branch / PR:** `wt/pre10-bench-docs-sync` (Pre-1.0 Polish Wave PR 4 of 4) — Current-build `07a1492d` release benchmark refresh + README/docs/website sync:
  - Measured clean `07a1492d` `--release` builds on Apple M2 Pro P-cores (`4-11`, `quiet-on`) across packaged binary/tarball footprint, `everyday_bench`, 120k-commit `history_pagination_bench`, worker open/working/stage, 20k-line/50k-path `review_bench`, and headless GPUI UI-thread & accessibility benchmarks (`docs/benchmarks/2026-11-24-pre10-release-benchmarks.md`).
  - Published companion raw records (`website/public/performance/records/pre10-release-summary.txt`, `pre10-linear-history-pages.csv`), rebuilt static guides (`website/build_guides.py`), and refreshed `README.md`, `website/guides/`, `docs/validation.md`, and `docs/validation/2026-11.md`.
- **Not exercised locally:** macOS ARM64 (`crates/app` UI) and Linux x86-64 binaries are verified by hosted CI; live interactive Orca + `fcitx5`/`IBus` Wayland/X11 sessions, Ubuntu 24.04 GNOME desktop checks, and physical macOS VoiceOver/UI checks remain owner-driven desktop passes.
- **Next concrete steps (owner-driven launch actions after PR 4 merges & local release install completes):**
  1. Complete owner physical desktop sanity passes (macOS Apple Silicon + VoiceOver; Linux Omarchy Orca + `fcitx5`/`IBus`; Ubuntu 24.04 GNOME).
  2. Deploy rebuilt `website/public/` to Cloudflare Pages (`wrangler pages deploy`) when ready.
  3. Configure macOS Developer ID signing & Apple notarization (`gitturtle-macos-signing`, `HOSTED_UPLOAD_PLATFORMS` flip), create `gitturtle-release` environment & `v*` tag protection, and tag/publish the first public release & AUR package.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive macOS desktop sanity on PR #248 + PR #255/#256 (traffic lights, Dock, Finder drop, `gitturtle://open`, Quick Look, Shift-click range compare, rebase drag-reorder, VoiceOver).
- Interactive Omarchy/Linux desktop sanity on PR #249 + PR #255/#256 (live Omarchy theme/font sync, Hyprland 460x480 tiling, single-instance handoff, `PRIMARY` paste, live Orca + `fcitx5`/`IBus` Wayland/X11 IME & password privacy, Shift-click range compare, rebase drag-reorder).
- Ubuntu 24.04 x86-64 GNOME physical desktop pass.
- Held by owner (out of scope): macOS signing/notarization, `website/` Cloudflare Pages deployment, `v0.1.0`/`v1.0.0` GitHub Release tagging & AUR push, and Phase 5 opt-in auto-update design.
