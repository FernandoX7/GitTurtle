# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-23
- **Latest `main`:** `4c3c6112` (PRs `#241`–`#252` merged; Quality green).
- **Active branch / PR:** `wt/readme-marketing` — Concise, marketing-first `README.md` refinement:
  - Replaced "before/after speedup" framing with confident current-performance measurements in one scannable table.
  - Trimmed dense internal jargon in favor of crisp user-facing highlights, sleek 5-row lockfile-checked stack summary (15 marquee crates synced in `docs/doc-manifest.json`), and tighter section copy.
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
