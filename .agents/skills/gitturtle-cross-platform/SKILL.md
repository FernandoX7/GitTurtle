---
name: gitturtle-cross-platform
description: macOS vs Linux cross-platform parity matrix for GitTurtle — windowing, GPU, keyboard/IME, accessibility, credentials, packaging, cross-clippy checks, and platform copy rules. Use whenever touching platform-conditional code, shortcuts, credentials, packaging, or UI copy.
---

# macOS and Linux cross-platform parity

GitTurtle ships macOS-first while keeping Rust/GPUI cleanly portable to Linux (Arch/Omarchy x86-64 & AArch64, Ubuntu 24.04 x86-64 & ARM64).

## Platform matrix

| Subsystem | macOS (Apple Silicon) | Linux (Arch/Omarchy & Ubuntu) |
| --- | --- | --- |
| Window / GPU | AppKit / Metal (`objc2`, `gpui-pre-macos`) | Wayland (`zwp_text_input_v3`, fractional scale) & X11/XWayland (`gpui-pre-linux`, `gpui-pre-wgpu`) |
| Keyboard / Input | `Cmd` primary modifier; withheld `Cmd` key-up handling | `Ctrl` primary modifier; X11 synthetic key-repeat guard; `fcitx5` held-key; Hyprland/Omarchy CSD & `F10` |
| Accessibility | VoiceOver (`accesskit_macos`) | Orca / AT-SPI (`accesskit_unix` + `accesskit_atspi_common`) |
| Credentials & OS | Keychain, Quick Look, `/opt/homebrew/bin/gh` | Secret Service (`oo7`, note unlocked/unencrypted keyring warning), `ashpd` OpenURI, `gio`/`xdg-mime` polyglot checks |
| Decoders | In-process ImageIO, PDFKit, `ufbx`, `meshopt`, `resvg` | Seccomp-confined Linux helper for image/PDF/AVIF/JP2 (`gitturtle-preview`); `ufbx`/`meshopt` currently in-process |
| Packaging | `.app` bundle, Icon Composer, Developer ID signing & notarization | Release tarballs, `scripts/install-linux.py`, AUR `PKGBUILD` |

## Cross-platform engineering rules

1. **Cross-check `git-core` for macOS from Linux:** Whenever `crates/git-core` changes on a Linux host, run `env -u RUSTUP_TOOLCHAIN cargo clippy --locked -p gitturtle-core --target aarch64-apple-darwin -- -D warnings` (when the target is installed) before pushing to catch `cfg(target_os = "macos")` regressions immediately.
2. **Symlinked `TMPDIR` parity:** macOS maps `/var` -> `/private/var`. Always canonicalize paths consistently in tests and production comparisons, and verify path-keyed logic under a symlinked `TMPDIR` on Linux.
3. **Filesystem & `st_dev` traps:** `st_dev` is unstable across Btrfs subvolumes and boots; never rely on `st_dev` alone (prefer inode with documented FAT/exFAT/FUSE bounds). Always check `is_file()` before opening files inside `.git/` so a hostile FIFO cannot hang a reader.
4. **Platform copy discipline:** Never hardcode unguarded `"Mac"`, `"Keychain"`, `"Finder"`, or `"Cmd+"` in shared UI strings; route platform-specific labels, keycaps, and secure-store names through centralized platform constants.
5. **Explicit platform tagging:** State clearly in PR descriptions and handoffs which platforms and display backends were actually exercised versus unverified (e.g., `"Verified on Linux XWayland + hosted macos-15 CI | Unverified on live macOS desktop"`). Never claim an unexercised platform passed native QA.
