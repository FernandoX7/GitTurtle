# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-23
- **Latest `main`:** `5f74b4a0` (all 10 roadmap PRs `#241`–`#250` merged).
- **Active branch / PR:** `wt/polish-docs-website-ci` — Post-roadmap polish PR:
  - **Real Linux crash fixed (`0cd37524`):** Opening **Pull requests** in non-test Linux builds panicked with a re-entrant `GitTurtle` entity read in `github_view::Panel::new`; deferred the read into the spawned task and removed the `not(test)` blind spot.
  - **CI test flake fixed (`0cd37524`):** Replaced slow `python3` startup in `DetachedPipeHolder` (`crates/git-core/src/process_io.rs`) with native `setsid -f` + `/bin/sh` on Linux (and `python3 -S -I` on non-Linux Unix) + atomic PID marker writes so 300 ms deadline tests never race interpreter startup under CI CPU load.
  - **Fresh `2960 × 1946` screenshots & website update:** Recaptured 11 existing + 2 new (`history-filters.png`, `github-review.png`) clean `0cd37524` native Wayland `@2x` captures, regenerated lossless tour WebPs and `gitturtle-social.png`, updated `website/public/index.html`, `/performance/` (Wave 3A/3B tables), `/docs/history-and-diffs/`, `/docs/github/`, `/releases/`, and `website/asset-provenance.json`.
  - **Docs refreshed:** Synced `README.md`, `docs/user-guide.md`, `docs/file-previews.md` (exact 28 enabled tree-sitter grammars), `docs/validation/2026-11.md`, `docs/validation.md`, and `website/README.md`.
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
