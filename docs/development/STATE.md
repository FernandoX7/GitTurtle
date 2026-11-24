# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-23
- **Latest `main`:** `31d31af2` (PRs `#241`–`#253` merged; Quality green).
- **Active branch / PR:** `wt/binary-size-diet` (Pre-1.0 Polish Wave PR 1 of 4) — Release binary size diet & cold-start lean-out:
  - Tuned `[profile.release]` (`opt-level = "s"`, `lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"`) with `opt-level = 3` retained for UI framework, app, macOS Metal/CoreText, Linux wgpu/text-shaping, core, diff-model, preview, syntax runtime, and raster/image hot crates.
  - Added `-Wl,-x` for release `gitturtle` binaries in `crates/app/build.rs` to strip input local (`STB_LOCAL`) / non-global symbols while keeping `.symtab` and all global/weak symbols intact for `scripts/package-linux.sh` and crash backtraces.
  - Reduced Linux AArch64 release binary from `142.94 MiB` to `106.77 MiB` (`-25.3%`; `.tar.gz` `41.66 MiB` → `32.37 MiB`, `-22.3%`) with zero hot-path regressions verified on M2 Pro P-cores (`docs/benchmarks/2026-11-23-release-binary-size-diet.md`).
- **Not exercised locally:** Linux x86-64 (`ubuntu-24.04` `rust-lld` release link/packaging) and macOS ARM64 (`crates/app`/`crates/preview` compilation/packaging) are pending hosted `ubuntu-24.04` and `macos-15`/`macos-26` CI; live physical-desktop sessions (multi-monitor Wayland/Hyprland, X11, KDE, GNOME, macOS) and live Cloudflare Pages deployment (`wrangler` deploy) are held for owner.
- **Next concrete steps:**
  1. Merge PR 1 (`wt/binary-size-diet`) after 100% green hosted Quality CI, then proceed to PR 2 (Shift-click History range compare & Interactive Rebase drag-reorder).
  2. PR 3: Linux IME & Orca/AT-SPI accessibility parity hardening.
  3. PR 4: Current-build end-to-end GUI benchmark refresh, docs/website sync, and local release install.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive macOS desktop sanity on PR #248 (traffic lights, Dock lifecycle/reopen/recent, Finder drop, `gitturtle://open`, pinch/swipe, LaunchServices PATH, `QLPreviewPanel`, system accent, terminal presets, VoiceOver clipping).
- Interactive Omarchy/Linux desktop sanity on PR #249 (dark-desktop cold launch without light flash, Omarchy 3.x/4.x live theme swap, Hyprland 460x480 tiling & animations toggle, single-instance handoff + activation token, `PRIMARY` copy + middle-click paste, Wayland IME password privacy, KDE/GNOME accent & `kwinrc`/`kcmfonts`, Secret Service parent modal on KWallet/KeePassXC/gnome-keyring, Flatpak Bubblewrap + seccomp check).
- Ubuntu 24.04 x86-64 GNOME physical desktop pass.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` environment & `v*` tag protection, `v0.1.0` tag/GitHub Release publication, AUR push, `website/` Cloudflare Pages deploy, and Phase 5 opt-in auto-update design.
