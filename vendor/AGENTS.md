# Vendored dependency guidance

These dependencies are selected by the root [Cargo.toml](../Cargo.toml) `[patch.crates-io]` table and pinned through `Cargo.lock`. Read each crate's patch note and the consuming [app](../crates/app/AGENTS.md) or [preview](../crates/preview/AGENTS.md) guide before editing or reviewing. Each patch note owns exact upstream provenance, modified files, and removal conditions; keep it current with any local change.

| Dependency | Upstream target & retirement trigger | Patch note & local purpose |
| --- | --- | --- |
| `accesskit_atspi_common` | `AccessKit/accesskit` (`platforms/atspi-common`); #788 drops at `>= 0.21`, others PR-ready | [GITTURTLE-PATCH.md](accesskit_atspi_common/GITTURTLE-PATCH.md) — Linux AT-SPI disabled states, `Expandable`/`Expanded`/`Busy` states, `"level"` attributes, `LabelledBy` relations, modal-hidden page removal/restore, clip visibility/hit-test filtering, `Drop` cache teardown, and bounded selectable-item cache retention |
| `accesskit_macos` | `AccessKit/accesskit` (`platforms/macos`); PR-ready clipping parity | [GITTURTLE-PATCH.md](accesskit_macos/GITTURTLE-PATCH.md) — macOS VoiceOver `accessibilityFrame` and `hit_test` clipping parity against `clips_children()` ancestors |
| `accesskit_unix` | `AccessKit/accesskit` (`platforms/unix`); auto-retire at `>= 0.24.1` (#819 upstream) | [GITTURTLE-PATCH.md](accesskit_unix/GITTURTLE-PATCH.md) — Upstream #819 AT-SPI `AddAccessible` / `RemoveAccessible` cache signal signatures |
| `gpui-base` | GPUI Kit (`crates/base`); retire per-patch on upstream upgrades | [GITTURTLE-PATCH.md](gpui-base/GITTURTLE-PATCH.md) — Accessibility (`DialogKind`, `TextRun` <=255-char splitting, disabled flags, `accepts_text_input`, Slider `SetValue`), dialog/input/Enter once-per-press semantics, Linux `PRIMARY` selection copy & middle-click paste, Linux IME `ContentType` hook, popup/popover device-pixel clamping & bottom inset, blink-cursor stability, resizable fit, `ColorSwatch::track_focus`, `TextSelectionRegistration::with_scrollable`, and text decorations |
| `gpui-component` | GPUI Kit (`crates/component`); retire per-patch on upstream upgrades | [GITTURTLE-PATCH.md](gpui-component/GITTURTLE-PATCH.md) — Resolved theme/focus-ring/disabled tokens, menu keyboard/pointer/accessibility behavior, Linux masked-input password IME content-type sync, popup/dialog clamping, top-left alignment & shadow masking, weak overlay callbacks, color picker roving focus, diff syntax roles, `mono_font` empty-list guard, and `Icon::own_color` |
| `gpui-pre-linux` | `zed-industries/zed` (`crates/gpui_linux`); PR-ready platform patches | [GITTURTLE-PATCH.md](gpui-pre-linux/GITTURTLE-PATCH.md) — Wayland fractional-scale `content_size` device-pixel rounding, clipboard `PRIMARY` cache invalidation, Wayland `zwp_text_input_v3` & X11 XIM IME protocol/password/focus/spot-location parity, Wayland/X11 AccessKit root window bounds & initial focus sync, dark-desktop startup wait & GTK/KDE appearance/button-layout/DPI fallbacks, portal accent color, Secret Service `WindowIdentifier` & `xdg_activation_v1` export, and Wayland-to-X11 startup fallback |
| `gpui-pre-macos` | `zed-industries/zed` (`crates/gpui_macos`); PR-ready 1-file fix | [GITTURTLE-PATCH.md](gpui-pre-macos/GITTURTLE-PATCH.md) — macOS accessibility responder alignment on `native_view` with `objc2` (the previous `frame_waker` patch is upstream in `0.3.12` via `CAMetalDisplayLink`) |
| `gpui-pre-macros` | `zed-industries/zed` (`crates/gpui_macros`); PR-ready 1-file test fix | [GITTURTLE-PATCH.md](gpui-pre-macros/GITTURTLE-PATCH.md) — Test-only `#[gpui::test]` teardown calling `TestAppContext::quit()` so multi-seed runs release apps/threads |
| `gpui-pre-wgpu` | `zed-industries/zed` (`crates/gpui_wgpu`); PR-ready 2-file fix | [GITTURTLE-PATCH.md](gpui-pre-wgpu/GITTURTLE-PATCH.md) — Bounded hardware GPU recovery (`MAX_HARDWARE_RECOVERY_ATTEMPTS = 3`) before software fallback (`llvmpipe`/Lavapipe) and `WGPU_BACKEND` selection within Vulkan/GL |
| `mermaid-rs-renderer` | `1jehuang/mermaid-rs-renderer`; PR-ready `disk-cache` feature + parser/test fixes | [GITTURTLE-PATCH.md](mermaid-rs-renderer/GITTURTLE-PATCH.md) — Disables renderer-owned disk font caching, fixes `Link`/`Click` node-ID flowchart edges, and isolates upstream test files |

## Patch discipline

- Keep product behavior in `crates/app` or `crates/preview` whenever the upstream API can express it.
- Record missing upstream behavior, consuming paths, provenance, modified files, and removal conditions in each crate's patch note. Preserve license notices and the matching GPUI Kit dependency set.
- On an upstream upgrade, audit every local patch against the new release and drop it once consuming regressions confirm upstream covers it. Remove paired `gpui-base`/`gpui-component` patches together.
- Note that editing pure documentation files under `vendor/` (`AGENTS.md`, `CLAUDE.md`, `GITTURTLE-PATCH.md`, `CHANGELOG.md`) does not bust Rust CI caches.

## Testing vendored crates

These packages are excluded workspace members. Run a vendored crate's own library tests through [scripts/vendor-crate-test.sh](../scripts/vendor-crate-test.sh) `vendor/<crate> [filter...]` (never `cargo test` inside `vendor/<crate>` directly): it builds an isolated copy under `.local/` against the root `[patch.crates-io]` table, `Cargo.lock`, and `upstream-files/` in `.local/target-vendor`, and refuses registry copies of patched crates.

Validate runtime behavior through consuming regressions:
- Toolkit controls, layout, focus, and dialogs: `crates/app` `#[gpui::test]` suites.
- Linux AT-SPI adapter: `crates/app` `native_accessibility::atspi_state_tests`.
- Mermaid integration: `crates/preview` Mermaid/SVG fixtures (never upstream CLI/filesystem examples).
- Patch-note or doc-only edits need link and diff checks without rebuilding the application.
