# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-23
- **Latest `main`:** `f74ca679` (PR `#254` binary size diet merged; Linux AArch64 release `142.94 MiB` → `106.77 MiB`, `-25.3%`; Quality green).
- **Active branch / PR:** `wt/ergonomics-range-rebase` (Pre-1.0 Polish Wave PR 2 of 4) — Everyday ergonomics:
  - Added Shift-click and `Shift+Up`/`Shift+Down` History commit range compare (`HistoryRange` on `RepositoryWorkspace`), wired passively through `Job::CompareRevisions` (`ComparisonMode::Endpoints`) with generation/OID validation, calm inspector header (`Comparing <old>..<new>`, commit count, `Swap`, `Clear`/`Esc`), Compare + Back range retention, search retention, and AccessKit row/header labels.
  - Added native GPUI drag-to-reorder in Interactive Rebase (`RebaseForm`) with 2 px accented drop indicator, drag preview respecting reduced motion, live edge auto-scroll, Escape-to-cancel mid-drag without closing or mutating, and live `validate_steps` enforcement rejecting leading `Squash`/`Fixup` drops with an accessible inline notice.
  - Added deterministic unit + `#[gpui::test]` suites (`history_range_tests.rs`, `interactive_rebase_drag_tests.rs`) and synced `docs/user-guide.md` and `docs/interactive-rebase.md`.
- **Not exercised locally:** macOS ARM64 (`crates/app` compilation/UI) and Linux x86-64 are pending hosted `macos-15`/`macos-26` and `ubuntu-24.04` CI (locally verified on Linux AArch64 headless `#[gpui::test]`; physical Wayland/X11, macOS, VoiceOver, and Orca sessions remain owner-driven).
- **Next concrete steps:**
  1. Finish PR 2 gates/reviews/hosted CI and squash-merge to `main`.
  2. PR 3: Linux IME & Orca/AT-SPI accessibility parity hardening.
  3. PR 4: Current-build end-to-end GUI benchmark refresh, docs/website sync, and local release install.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive macOS desktop sanity on PR #248 (traffic lights, Dock lifecycle/reopen/recent, Finder drop, `gitturtle://open`, pinch/swipe, LaunchServices PATH, `QLPreviewPanel`, system accent, terminal presets, VoiceOver clipping).
- Interactive Omarchy/Linux desktop sanity on PR #249 (dark-desktop cold launch without light flash, Omarchy 3.x/4.x live theme swap, Hyprland 460x480 tiling & animations toggle, single-instance handoff + activation token, `PRIMARY` copy + middle-click paste, Wayland IME password privacy, KDE/GNOME accent & `kwinrc`/`kcmfonts`, Secret Service parent modal on KWallet/KeePassXC/gnome-keyring, Flatpak Bubblewrap + seccomp check).
- Ubuntu 24.04 x86-64 GNOME physical desktop pass.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` environment & `v*` tag protection, `v0.1.0` tag/GitHub Release publication, AUR push, `website/` Cloudflare Pages deploy, and Phase 5 opt-in auto-update design.
