# GitTurtle patch to gpui-pre-linux 0.3.4

This directory contains the exact crates.io `gpui-pre-linux` 0.3.4 source under Apache-2.0, the version the matching GPUI Kit 0.6.0 stack locks. `LICENSE-APACHE` is byte-for-byte preserved. The registry archive (`gpui-pre-linux-0.3.4.crate`) has SHA-256 `8fd1d912c039641f0236b1701b7871bfddfb4926ffd1915b3958b73d03177250`, the checksum the workspace lockfile recorded for it; the package metadata names Zed's `crates/gpui_linux` at `69164008341295ad481bb11c0334a712ca8c23e3` (`https://github.com/zed-industries/zed`). `Cargo.toml` is the published normalized manifest and `Cargo.toml.orig` the original one; dependency requirements are unchanged and the workspace lockfile pins their versions. The registry-only `.cargo-ok` and the package lockfile are omitted. Apart from this note, `src/linux/wayland/window.rs` is the only file that differs from the archive.

Cargo stops checking the archive checksum once the crate comes from a path, so compare the tree with the archive it came from when reviewing a change here:

```sh
mkdir -p .local/gpui-pre-linux-archive
tar xzf ~/.cargo/registry/cache/index.crates.io-*/gpui-pre-linux-0.3.4.crate -C .local/gpui-pre-linux-archive
diff -r --exclude=Cargo.lock --exclude=GITTURTLE-PATCH.md .local/gpui-pre-linux-archive/gpui-pre-linux-0.3.4 vendor/gpui-pre-linux
```

The crate is the Linux and FreeBSD platform backend; its dependencies are all under `cfg(any(target_os = "linux", target_os = "freebsd"))`, so the macOS build graph does not reach it.

## Change: Wayland text input only while a text field is focused

This is a runtime change to the Wayland backend, a backport of upstream [zed-industries/zed#63776](https://github.com/zed-industries/zed/pull/63776) (fixing [#63774](https://github.com/zed-industries/zed/issues/63774)), first released in `gpui-pre` 0.3.5. The X11 backend, the headless backend and the rest of the crate are unchanged. The decision and its evidence are in [the 2026-11-03 decision record](../../docs/development/2026-11-03-wayland-text-input-focus.md).

`WaylandWindowStatePtr::update_ime_enabled`, which runs after every frame of an active window, decides whether the window keeps the text-input protocol (`zwp_text_input_v3`) on. With a focused input handler it asks the handler whether it accepts text input; with none, 0.3.4 answered `true`, so text input stayed on for every button, menu, list and shortcut. With text input on, an input method such as fcitx5, as on a default Omarchy, takes the keyboard: it delivers a held key as a release and a fresh press at the seat's repeat rate, each `is_held: false`, so GPUI's own repeat timer (`wayland/client.rs`), which marks repeats held, never runs, and it swallows single-letter shortcuts outside text fields. A key held on a toolbar dropdown reopened its menu about 72 times in 2 s (native probes QA-90 and QA-91).

The decision now lives in a pure function, `text_input_enabled(Option<bool>)`, given `None` when no handler is focused and the handler's answer otherwise: text input is on only for `Some(true)`, a focused handler that accepts text input. The kit's text input registers a handler while it is focused and accepts text input while editable, so the commit message, the palette and other fields keep the input method, with composition and candidate windows; read-only text, buttons, menus, lists and shortcuts get GPUI's own key events and held repeats. Dead keys and compose sequences come from xkb compose, not from text input, so they still work outside a text field. `update_ime_enabled` still only enables or disables the protocol when the answer changes.

A known transient, left as upstream has it: the text-input `Enter` event (`wayland/client.rs`, the `zwp_text_input_v3::Event::Enter` arm), sent when the surface gains keyboard focus, enables text input unconditionally, and it stays on until the window's next frame calls `update_ime_enabled` again, which turns it off when no text field is focused. Keys pressed in that gap, before the first frame after the window regains focus, still reach the input method.

Test: `linux::wayland::window::text_input_tests::text_input_follows_a_focused_text_field` checks the truth table (no handler: off; a handler accepting text input: on; a handler not accepting it: off), run with `scripts/vendor-crate-test.sh vendor/gpui-pre-linux`; it fails when the no-handler answer goes back to `true`. The GPUI test platform has no Wayland, so the protocol itself is checked natively under `WAYLAND_DEBUG=1` against the integrated build: focus on a dropdown logs `zwp_text_input_v3.disable` and a 2 s held Enter gives one press and one menu; focus in a field logs `enable` and fcitx5 Pinyin composes in the commit message and the palette; after Alt-Tab back with a button focused, text input stays disabled.

Remove this patch, the `[patch.crates-io]` entry, the workspace exclusion, the `THIRD_PARTY_NOTICES.md` row and the `vendor/AGENTS.md` row on an upgrade to `gpui-pre` 0.3.5 or later, which carries the upstream fix.
