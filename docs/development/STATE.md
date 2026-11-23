# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-22
- **Latest `main`:** `1638cc0b` (PRs #241 Wave 0A through #247 Wave 1B/1C/1D merged green).
- **Active branch / PR:** `feat/wave2a-macos-craft` (Roadmap PR 8 of 10 — First-class macOS native craft, unsigned-ready, Wave 2A).
- **Completed in PR 8:**
  - **macOS Lifecycle, Multi-Window & Chrome (`crates/prefs`, `crates/app`, `scripts/package-macos.py`):** Resident-in-Dock lifecycle when last window closes, `on_reopen`, `on_open_urls` (`file://` and `gitturtle://open?path=...` passive local directory opens), deduplicated `set_dock_menu` + `add_recent_document`, `tabbing_identifier`, shared app-global `SerialExecutor` preferences writer, persisted `WindowGeometry` (bounds/display/maximized/fullscreen), integrated transparent titlebar with centered traffic lights & out-of-flow drag/double-click backdrop, live `<repo> — <branch>` window title & `set_document_path` proxy path, complete native Menu Bar (`New Window`, `New Tab`, `Open Recent` + `Clear Menu`, `Close Tab`, `Find` submenu, `Enter/Exit Full Screen`, `Bring All to Front`), `ExternalPaths` folder drop, `PinchEvent` zoom (Image Compare/PDF/3D) + Back swipe gesture, and enriched `Info.plist` (`LSApplicationCategoryType`, `CFBundleDocumentTypes`, `CFBundleURLTypes`).
  - **System Integration, Credentials, Quick Look, Accent & VoiceOver (`crates/git-core`, `crates/github`, `crates/theme`, `crates/app`, `vendor/accesskit_macos`):** Centralized per-command LaunchServices `PATH` hygiene (`/opt/homebrew/bin`, `/usr/local/bin`, `/usr/local/MacGPG2/bin`); modern macOS Data Protection Keychain (`OSX_10_15`, local-only) with graceful `-34018` fallback to legacy login Keychain for unsigned/ad-hoc builds; in-process `QuickLookUI` `QLPreviewPanel` controller with 3-path temp cleanup; WCAG/OKLab-fitted `NSColor.controlAccentColor` live accent option; macOS installed terminal presets + Alacritty/Kitty/iTerm2 launch flags + expanded monospace font filter; vendored `accesskit_macos 0.26.3` off-screen `accessibilityFrame` and `hit_test` clipping parity.
- **Not exercised locally:** macOS compilation/execution of `crates/app`, `crates/preview`, and `vendor/accesskit_macos`, and live macOS desktop runtime (`aarch64-apple-darwin` verified locally via cross-clippy on 5 headless crates with and without `test-support`; hosted macOS CI verifies full macOS build and tests on PR push).
- **Next concrete steps:**
  1. PR 9 — First-class Omarchy & general Linux native craft + packaging prep (Waves 2B + 2C).
  2. PR 10 — Vendored patch diet, automated visual QA, and pre-release website readiness (Wave 4C + Wave 6 prep).

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive macOS desktop sanity on PR 8: transparent titlebar traffic lights + drag/double-click, resident Dock lifecycle + reopen + Dock menu + `Open Recent`, multi-window + geometry restore, Finder/Dock folder drop & `gitturtle://open`, trackpad pinch zoom & swipe back, LaunchServices Homebrew `git`/`git-lfs`/`gpg`, in-process `QLPreviewPanel` Spacebar/close, `Use macOS accent color` live switch, installed terminal presets, and VoiceOver clipped focus box / hit-test parity.
- Interactive Omarchy & macOS desktop sanity on PRs 1–7 (`gpui-kit 0.10.0`, OS close/`Quit` refusal, Slider `SetValue`, soft-wrap gutter, dark Mermaid, CSV/JSON/PDF search, bisect, submodules, custom commands, GitHub PR v2).
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, `website/` Cloudflare deploy, and opt-in auto-update design.
