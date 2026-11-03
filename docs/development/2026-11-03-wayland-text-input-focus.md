# Wayland text input only for a focused text field (2026-11-03)

## Decision and scope

- **Question.** On Linux under Wayland with an input method running (fcitx5 on Omarchy), a key held on a toolbar dropdown reopened its menu about 72 times in 2 s. QA-90 and QA-91 saw this natively, and it is recorded as a limitation in the `gpui-base` and `gpui-component` patch notes. The question is how to fix it at its cause.
- **Cause.** `gpui-pre-linux` 0.3.4 turns the Wayland text-input protocol (`zwp_text_input_v3`) on whenever the focused element has no input handler (`update_ime_enabled`, `src/linux/wayland/window.rs:1033-1057`, the `true` at `:1044-1046`). With text input on, the input method delivers each repeat as a fresh press (`is_held: false`, `wayland/client.rs:2008-2021`). GPUI's own repeat timer marks repeats held (`client.rs:1931`), but it never runs in that path.
- **Decision.** Vendor `gpui-pre-linux` 0.3.4 under `vendor/` through `[patch.crates-io]`, as `gpui-pre-macos` is vendored. Backport upstream Zed's one-line fix: with no focused handler, `update_ime_enabled` returns `false`. Text input then follows the focused text field. Fields keep the input method, because the kit's input registers a handler and accepts text input. Buttons, menus, lists and shortcuts get GPUI's own key events and repeats.
- **Outside this decision.** X11, where XIM creates its input context on every focus-in (`x11/client.rs:975-984`) and every press is `is_held: false` (`:1120`), stays a recorded limitation. So do repeats inside a text field, which is ordinary text entry, and an upgrade of the `gpui-pre` stack.

## Evidence

| Source | Checked date/version | Relevant finding | Limit |
| --- | --- | --- | --- |
| `gpui-pre-linux` 0.3.4 registry source, `wayland/window.rs:1033-1057`, `wayland/client.rs:536-570`, `:1872-1912`, `:1916-1931`, `:1992-1995`, `:2008-2021` | 2026-11-03, 0.3.4 (Zed snapshot `6916400`) | With no focused handler, text input is enabled. Dead keys and compose come from xkb compose, not from text input. A single-character commit with no handler arrives as a fresh key-down. The text-input Enter event re-enables text input until the next frame recomputes it. | Read from the source. Not yet observed under the patched build. |
| [zed-industries/zed#63776](https://github.com/zed-industries/zed/pull/63776), fixing [#63774](https://github.com/zed-industries/zed/issues/63774) | Merged 2026-09-07 | The no-handler fallback was changed from `true` to `false` because fcitx5 swallowed single-letter shortcuts outside text fields. The earlier `true` (PR #58237) kept the previous always-on behaviour; it was not a deliberate design choice. | The upstream issue concerns swallowed shortcuts. It does not mention key repeats. |
| `gpui-pre-linux` 0.3.5 and 0.3.11 on crates.io; Zed `main` `crates/gpui_linux/src/linux/wayland/window.rs` | 2026-11-03 | 0.3.5 (2026-09-14) is the first release with the fix, and 0.3.11 (Zed `f816e1f`) and `main` keep it. Each release pins the rest of the `gpui-pre` stack exactly (`=0.3.x`). | Taking the fix by upgrading means moving the whole stack and rebasing the macOS and macros patches. |
| `vendor/gpui-base/src/input/base/element.rs:2134`; `gpui-pre` `src/input.rs:103`, `window.rs:3079-3088` | 2026-11-03, this branch | The kit's input registers a handler while it is focused and accepts text input by default. When several handlers are registered, the last one wins. | — |

## Compatibility and alternatives

- **Size.** The crate has 27 Rust files (15,391 lines). Its licence is Apache-2.0 only, and the licence file is present. It has no build script, no `links` key and no proc macros. Vendoring it changes only the `source` and `checksum` lines of its `Cargo.lock` entry; the dependency set stays the same.
- **Alternatives considered.**
  - **An inert input handler in the app.** The app would register an input handler for focused non-text elements from a root canvas painted first, rejecting text input. That is about 50 Linux-only lines that depend on GPUI's paint order, and they become dead code after a stack upgrade.
  - **A timing filter in the kit's menu and popover.** fcitx5 sends repeats as release and press pairs, so only a timing guess could tell a repeat from a real press. It also would not stop the input method from swallowing letter shortcuts.
  - **Leave the limitation.** Default Omarchy keeps the reopening menu, and a CJK input-method user loses letter shortcuts outside text fields.
- **What would change the recommendation.** A native regression that the patch causes, for example fcitx5 Pinyin failing to compose in the commit message or the palette. A move to `gpui-pre` 0.3.5 or later would also change it: the vendored copy then goes away.

## Enforcement and acceptance

- **Owner.** A follow-ups round in `vendor/gpui-pre-linux/`: the backport, ideally as a small pure function with a unit test. It also adds a provenance note giving the registry checksum. The root `Cargo.toml` gets the patch entry, the exclude list and its comment. `vendor/AGENTS.md` gets a new row, and the Wayland input-method clause comes out of the `gpui-base` and `gpui-component` rows (X11 stays). `THIRD_PARTY_NOTICES.md` and both `GITTURTLE-PATCH.md` limitation passages are updated.
- **Checks.** `scripts/vendor-crate-test.sh vendor/gpui-pre-linux` runs the crate's tests and the new unit test. `cargo deny --locked check` runs because the lock changes. A security review compares the vendored tree with the registry `.crate` it came from: Cargo stops checking the checksum once the crate comes from a path.
- **Native evidence.** The GPUI test platform has no Wayland, so these checks run natively under `WAYLAND_DEBUG=1`:
  - Focus on a dropdown logs `zwp_text_input_v3.disable`, and a 2 s hold gives one press and one menu.
  - Focus in a field logs `enable`, and fcitx5 Pinyin composes in the commit message and the palette.
  - After Alt-Tab back with a button focused, text input stays disabled.

## Outcome

Pending: the follow-ups round implements this. Record the landed commit, the security verdict and the native evidence here.
