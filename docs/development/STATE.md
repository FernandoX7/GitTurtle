# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-23
- **Latest `main`:** `b398399f` (PRs #241 through #249 merged after 100% green PR CI across every lane).
- **Active branch / PR:** `chore/wave4c-6-release-prep` (Roadmap PR 10 of 10 — Vendored patch diet, automated visual QA, and pre-release website/launch readiness, Wave 4C + Wave 6 prep).
- **Completed across Roadmap PRs 1–10:**
  - **PRs #241–#242 (Waves 0A/0B/0C & 5):** Active-write close/quit guard, safe `objc2` PDFKit bindings, UI-thread panic/redaction/bidi fixes, `check_invariants.py`, CLI doc-smoke checks, reviewer eval fixtures, 6-job weekly cache-seed plan, `zizmor`, CycloneDX 1.5 SBOM + SLSA provenance in `release.yml`, and core/a11y bug sweep.
  - **PRs #243–#245 (Waves 3A/3B & 4A/4B):** Straight pass-through graph quad rendering, cached text shaping, 4 headless domain crates (`gitturtle-theme`, `gitturtle-diff-model`, `gitturtle-github`, `gitturtle-prefs`), typed `CoreError`/`PreviewError`, `proptest` + `cargo-fuzz` targets, and large-repo pagination/search scalability.
  - **PRs #246–#247 (Waves 1A–1D):** Clutter-free commit/branch/stash context menus, collapsible search filter chips, pull/merge strategy dialogs, interactive rebase, contextual bisect banner, intra-line word diffs, GitHub PR v2 (merge, draft/ready, thread resolve), and visual/keyboard polish.
  - **PRs #248–#249 (Waves 2A–2C):** First-class macOS lifecycle/Dock/Finder/`gitturtle://open`/Quick Look/Keychain/VoiceOver clipping parity; first-class Omarchy 3.x/4.x & Linux desktop craft (single-instance handoff, `PRIMARY` clipboard, Wayland IME password privacy, dark-startup wait, dynamic `libdav1d.so.7`, seccomp 3D helpers, GPU recovery, `.deb`/Flatpak prep).
  - **PR 10 (Waves 4C & 6 prep):** Reverted dead `WindowExt::hide_tooltip` (`vendor/gpui-component/src/window_ext.rs`), deleted obsolete `Positioner::corner_following` and `Slider::aria_label` shims, standardized `vendor/gpui-pre-macos/GITTURTLE-PATCH.md`, added missing `accesskit_macos 0.26.3` notice to `THIRD_PARTY_NOTICES.md`, refreshed all 10 vendor patch notes with archive SHA-256s & removal triggers, added offline `scripts/dev/vendor_patch_audit.py` (wired into `gate.py`, `quality.yml`, `.githooks/pre-push`), added canonical GitHub Releases URL allowlist (`website/check.py`), added `website/visual_qa.py` + refreshed `website/guides/`, added sparse-checkout guards + opt-in Linux `Xvfb`+Lavapipe & macOS `osascript`/`screencapture` QA hooks (`scripts/native_qa/`), hardened GPUI test `settle()` helpers (`views_tests.rs`, `worktrees/tests.rs`) to drain worker/operations executors while holding `TestClock`, and updated `docs/public-launch.md` & `docs/releases.md` with the owner `v0.1.0` launch runbook.
- **Not exercised locally:** macOS compilation/execution of `crates/app`/`crates/preview` (verified on hosted macOS CI), live desktop Wayland/X11/Hyprland/KDE/GNOME/macOS sessions, and optional Playwright Chromium browser QA (`website/visual_qa.py --browser`).
- **Next concrete steps (owner-driven):**
  1. Complete deferred physical desktop QA checks below.
  2. Follow the owner `v0.1.0` launch runbook in `docs/releases.md` when ready to tag/release/deploy.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive macOS desktop sanity on PR #248 (traffic lights, Dock lifecycle/reopen/recent, Finder drop, `gitturtle://open`, pinch/swipe, LaunchServices PATH, `QLPreviewPanel`, system accent, terminal presets, VoiceOver clipping).
- Interactive Omarchy/Linux desktop sanity on PR #249 (dark-desktop cold launch without light flash, Omarchy 3.x/4.x live theme swap, Hyprland 460x480 tiling & animations toggle, single-instance handoff + activation token, `PRIMARY` copy + middle-click paste, Wayland IME password privacy, KDE/GNOME accent & `kwinrc`/`kcmfonts`, Secret Service parent modal on KWallet/KeePassXC/gnome-keyring, Flatpak Bubblewrap + seccomp check).
- Ubuntu 24.04 x86-64 GNOME physical desktop pass.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` environment & `v*` tag protection, `v0.1.0` tag/GitHub Release publication, AUR push, `website/` Cloudflare Pages deploy, and Phase 5 opt-in auto-update design.
