# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-23
- **Latest `main`:** `61141b4d` (all 10 roadmap PRs `#241`–`#250` plus polish PR `#251` merged; Quality 29/29 green).
- **Active branch / PR:** `wt/readme-showcase` — README showcase overhaul & lockfile-backed dependency sync:
  - Upgraded `README.md` with above-the-fold release metrics strip, full Wave 3A & Wave 3B core/monorepo benchmark table beside end-to-end UI traces, 7-crate workspace architecture and security boundary table, and pinned Rust 1.100.0 + core library versions.
  - Registered all 26 `README.md` dependency mentions in `docs/doc-manifest.json` (`lockfile_crate_mentions`) so `scripts/check-docs-sync.py` validates them against `Cargo.lock` on every PR.
- **Not exercised locally:** macOS compilation/execution of `crates/app`/`crates/preview` (verified on hosted macOS CI), live physical-desktop sessions (multi-monitor Wayland/Hyprland, X11, KDE, GNOME, macOS), and live Cloudflare Pages deployment (`wrangler` deploy held for owner).
- **Next concrete steps (owner-driven):**
  1. Complete deferred physical desktop QA checks below.
  2. Deploy refreshed `website/public/` via `wrangler pages deploy` (`website/README.md`) when ready.
  3. Follow the owner `v0.1.0` launch runbook in `docs/releases.md` when ready to tag/release.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive macOS desktop sanity on PR #248 (traffic lights, Dock lifecycle/reopen/recent, Finder drop, `gitturtle://open`, pinch/swipe, LaunchServices PATH, `QLPreviewPanel`, system accent, terminal presets, VoiceOver clipping).
- Interactive Omarchy/Linux desktop sanity on PR #249 (dark-desktop cold launch without light flash, Omarchy 3.x/4.x live theme swap, Hyprland 460x480 tiling & animations toggle, single-instance handoff + activation token, `PRIMARY` copy + middle-click paste, Wayland IME password privacy, KDE/GNOME accent & `kwinrc`/`kcmfonts`, Secret Service parent modal on KWallet/KeePassXC/gnome-keyring, Flatpak Bubblewrap + seccomp check).
- Ubuntu 24.04 x86-64 GNOME physical desktop pass.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` environment & `v*` tag protection, `v0.1.0` tag/GitHub Release publication, AUR push, `website/` Cloudflare Pages deploy, and Phase 5 opt-in auto-update design.
