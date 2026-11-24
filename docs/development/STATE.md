# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-23
- **Latest `main`:** `4382ed07` (PR `#254` binary size diet + PR `#255` Shift-click history range compare & interactive rebase drag-reorder merged; Linux AArch64 release `106.77 MiB`).
- **Active branch / PR:** `wt/linux-ime-atspi-parity` (Pre-1.0 Polish Wave PR 3 of 4) — Linux IME & Orca/AT-SPI parity hardening:
  - Extended vendored `accesskit_atspi_common` to emit `State::Expandable` + `State::Expanded` (with `StateChanged(Expanded)` updates), `State::Busy`, `"level"` object attributes, and `RelationType::LabelledBy`.
  - Populated AccessKit root window bounds (`set_root_window_bounds`) and initial active focus state in vendored `gpui-pre-linux` (Wayland + X11), hardened `zwp_text_input_v3` double-buffered `Done` ordering, commit serial validation, clamped cursor rectangles, direct field-switch resets, non-editable Enter guard, and `HiddenText` password hint, and hardened X11 XIM parity (single reused IC, incremental `PreeditDraw`/`PreeditCaret`/`PreeditDone`, editable/non-password forwarding gate, clamped fractional-scale `SpotLocation`, and X11 `is_held` auto-repeat tracking).
  - Fixed masked `bounds_for_range` bullet-offset mapping, unmasked password `on_focus`/`toggle_masked`/`set_masked` content-type syncing, secret `PRIMARY` suppression, and blur preedit unmarking in `gpui-base`; fixed `PopupMenu` label `aria_value`, keyboard active-descendant focus, menu label, and scrollable `Role::ScrollView` clipping in `gpui-component`.
  - Fixed 0-based `position_in_set` + container `size_of_set` (History, Command Palette, Rewrite Review), wired `aria_active_descendant` on remaining composite listboxes, wired `LiveRegion` alerts/statuses, `clips_children` on remaining scrollable dialogs, `workspace_text` accessible value, `shortcuts::ENTER` parity, and extended `scripts/native_qa/a11y.py`.
- **Not exercised locally:** macOS ARM64 (`crates/app` compilation/UI) and Linux x86-64 are pending hosted CI; live interactive Orca + `fcitx5`/`IBus` Wayland/X11 sessions and macOS VoiceOver remain owner-driven physical desktop checks.
- **Next concrete steps:**
  1. Wait for PR 3 hosted CI (Linux x86-64/ARM64/Arch + macOS 15/26) to pass 100% and squash-merge to `main`.
  2. PR 4: Current-build end-to-end GUI benchmark refresh, docs/website sync, and final local release install.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive macOS desktop sanity on PR #248 (traffic lights, Dock lifecycle/reopen/recent, Finder drop, `gitturtle://open`, pinch/swipe, LaunchServices PATH, `QLPreviewPanel`, system accent, terminal presets, VoiceOver clipping).
- Interactive Omarchy/Linux desktop sanity on PR #249 (dark-desktop cold launch without light flash, Omarchy 3.x/4.x live theme swap, Hyprland 460x480 tiling & animations toggle, single-instance handoff + activation token, `PRIMARY` copy + middle-click paste, Wayland IME password privacy, KDE/GNOME accent & `kwinrc`/`kcmfonts`, Secret Service parent modal on KWallet/KeePassXC/gnome-keyring, Flatpak Bubblewrap + seccomp check).
- Ubuntu 24.04 x86-64 GNOME physical desktop pass.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` environment & `v*` tag protection, `v0.1.0` tag/GitHub Release publication, AUR push, `website/` Cloudflare Pages deploy, and Phase 5 opt-in auto-update design.
