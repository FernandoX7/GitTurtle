# Vendored dependency guidance

These dependencies are selected by the root [Cargo.toml](../Cargo.toml) `[patch.crates-io]` table and pinned through `Cargo.lock`. Read each crate's patch note and the consuming [app](../crates/app/AGENTS.md) or [preview](../crates/preview/AGENTS.md) guide before editing or reviewing. Each patch note owns exact upstream provenance, modified files, and removal conditions; keep it current with any local change.

| Dependency | Patch note & local purpose |
| --- | --- |
| `accesskit_atspi_common` | [GITTURTLE-PATCH.md](accesskit_atspi_common/GITTURTLE-PATCH.md) — Linux AT-SPI disabled states, modal-hidden page removal/restore, clip visibility/hit-test filtering, and bounded selectable-item cache retention |
| `accesskit_unix` | [GITTURTLE-PATCH.md](accesskit_unix/GITTURTLE-PATCH.md) — Upstream #819 AT-SPI `AddAccessible` / `RemoveAccessible` cache signal signatures (remove once GPUI Linux upgrades to `accesskit_unix >= 0.24.1`) |
| `gpui-base` | [GITTURTLE-PATCH.md](gpui-base/GITTURTLE-PATCH.md) — Accessibility (`DialogKind`, `TextRun` <=255-char splitting, disabled flags, `accepts_text_input`), dialog/input/Enter once-per-press semantics, popup/popover device-pixel clamping & bottom inset, blink-cursor stability, resizable fit, `ColorSwatch::track_focus`, `TextSelectionRegistration::with_scrollable`, and text decorations |
| `gpui-component` | [GITTURTLE-PATCH.md](gpui-component/GITTURTLE-PATCH.md) — Resolved theme/focus-ring/disabled tokens, menu keyboard/pointer/accessibility behavior, popup/dialog clamping, top-left alignment & shadow masking, weak overlay callbacks, color picker roving focus, diff syntax roles, `mono_font` empty-list guard, and `Icon::own_color` |
| `gpui-pre-linux` | [GITTURTLE-PATCH.md](gpui-pre-linux/GITTURTLE-PATCH.md) — Wayland fractional-scale `content_size` device-pixel rounding (the previous zed#63776 Wayland text-input backport is upstream in `0.3.12`) |
| `gpui-pre-macos` | [README.gitturtle.md](gpui-pre-macos/README.gitturtle.md) — macOS accessibility responder alignment on `native_view` with `objc2` (the previous `frame_waker` patch is upstream in `0.3.12` via `CAMetalDisplayLink`) |
| `gpui-pre-macros` | [GITTURTLE-PATCH.md](gpui-pre-macros/GITTURTLE-PATCH.md) — Test-only `#[gpui::test]` teardown calling `TestAppContext::quit()` so multi-seed runs release apps/threads |
| `mermaid-rs-renderer` | [GITTURTLE-PATCH.md](mermaid-rs-renderer/GITTURTLE-PATCH.md) — Disables renderer-owned disk font caching and isolates upstream test files |

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
