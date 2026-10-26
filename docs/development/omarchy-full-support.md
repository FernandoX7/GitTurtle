# Full Omarchy support

Owner goal, 2026-10-26: say publicly, to the Omarchy community and its maintainers, that
GitTurtle fully supports Omarchy, and make it a Git client Omarchy users want to use. This
plan lists what that claim needs, in order. The [Linux guide](../linux.md#arch-linux-and-omarchy)
and the [validation matrix](../validation.md) remain the record of what has been checked.

## What Omarchy users expect (research, 2026-10-26)

Checked against upstream sources on 2026-10-26. Omarchy moved from `basecamp/omarchy` to
[`omacom/omarchy`](https://github.com/omacom/omarchy) (default branch `quattro`). Stable
was v4.0.6 (2026-10-19), with v4.5.0-rc1 out since 2026-10-20. The development machine runs
the Asahi community port (`omarchy version`: 4.0.4-mac.1), so evidence from it does not
cover current upstream x86-64 Omarchy.

- **Live theme switching is the feature that gets celebrated.** Maintainer posts about
  Chromium, Ghostty, VS Code, OpenCode and Hermes all highlight retinting on
  `omarchy theme set`.
- **Other apps get theming through Omarchy itself.** T3 Code, Claude Code, Pi, Hermes,
  Helix, Obsidian and VS Code each have a template in `default/themed/*.tpl` plus an
  `omarchy-theme-set-<app>` sync script, added by PRs to Omarchy (none by the app's own
  team; for example [#8784](https://github.com/omacom/omarchy/pull/8784) for T3 Code).
  Maintainers have pushed back on carrying theming for optional apps
  ([#719](https://github.com/omacom/omarchy/pull/719)).
- **GitTurtle needs no Omarchy patch.** It reads `current/theme/colors.toml` itself and
  watches for switches ([themes spec](themes/spec.md#omarchy-theme)), as Omarchy's own Qt
  app Hype does. That covers every theme, including hand-written and installed ones.
- **No Git GUI ships.** Omarchy preinstalls lazygit and has no Git GUI in its Install
  menu. Discussion [#4067](https://github.com/omacom/omarchy/discussions/4067) asked for one.
- **Universal clipboard.** In 4.0.x, Super+C/V/X send Ctrl+C/V/X to normal windows
  (`default/hypr/bindings/clipboard.lua`); 4.5 adds Super+A → Ctrl+A.
- **Fonts.** `omarchy font set` writes fontconfig's `monospace` alias and fires the
  `font-set` hook. It sets no interface font.
- **Getting apps.** Users install from the AUR, from Omarchy's package repository
  ([`omacom/omarchy-pkgs`](https://github.com/omacom/omarchy-pkgs), which accepts community
  PRs), or from an Install-menu entry added by PR (`omarchy-menu.jsonc` plus an
  `omarchy-install-*` script).
- **Window rules** in `default/hypr/apps/*.lua` match on the Wayland app_id. GitTurtle's
  is `com.gitturtle.desktop`, the same as its desktop entry's `StartupWMClass`. Every window
  gets Omarchy's default opacity (0.985 active, 0.96 inactive) unless a rule removes it.

## Phases

1. **First launch matches the desktop.** Done on branch `claude/omarchy-defaults`. With
   no preference store, a launch on Omarchy selects the Omarchy theme and the desktop's
   monospace font, and saves made before any store exists keep them
   ([persistence contract](../../crates/app/docs/writes-and-persistence.md#preferences-and-commit-drafts)).
   Existing stores keep their theme (owner decision, 2026-10-26).
2. **Universal clipboard.** Check Super+C/V/X natively in text fields, the commit
   composer, diff and editor selections and the list copy action, and Ctrl+A as 4.5's
   Super+A sends it. Fix any surface that ignores them. First native pass, 2026-10-26, on
   the development machine (4.0.4-mac.1, AArch64, fcitx5 running):
   - **Text fields.** Ordinary Ctrl+V, Ctrl+A, Ctrl+C and Ctrl+X work in History search.
     Hyprland's `send_key_state`, which the Super bindings dispatch, pasted into that
     field once while its input-method connection was inactive. With fcitx5 active it
     pasted nothing, and Nautilus's GTK4 location bar behaved the same way. Synthetic
     dispatch cannot hold Super the way a person does: holding it with `wtype` left fcitx5
     ignoring dispatched keys until it was restarted. A physical Super+C/V/X check is
     still open.
   - **Lists.** Outside text fields, dispatched keys reach GitTurtle. Ctrl+C on a
     selected History commit or changed file copies nothing: the shortcut sheet lists
     "Copy Selected List Item" (Ctrl+C), but only Blame handles it. What a copied commit
     or file holds (full hash, short hash and subject, path) is an owner decision.
3. **Polish found on the way** (first-launch check, 2026-10-26):
   - In a half-width tiled window (845 px wide), the empty History-search message is
     clipped at the list's right edge ("Searc…").
   - Page Down does not scroll Settings.
   - Omarchy's default 0.985 active opacity lets text behind a floating window show
     through diffs. Browsers opt out with a rule (`opacity = "1.0 0.985"`). Decide
     whether to document a user rule or propose one upstream.
   - Omarchy scripts launch apps with `gtk-launch <id>`. `gtk-launch com.gitturtle.desktop`
     reports "no such application", because the desktop file is
     `com.gitturtle.desktop.desktop`; only the full file name works. Settle the
     application ID before distribution, since window rules match on it and changing it
     later breaks them.
4. **Distribution.** Publish versioned release bundles (x86-64 and AArch64, checksums,
   build identity). Resolve the two dependencies the packager flags for license review
   (`mac`, `ufbx`) before distributing. Then add an AUR package (`gitturtle-bin`) and
   later propose GitTurtle to `omacom/omarchy-pkgs`, so `omarchy pkg add gitturtle`
   works.
5. **Current upstream verification.** On x86-64 Omarchy v4.0.6 and the 4.5 release
   candidate:
   - install;
   - first launch;
   - live switching across all stock themes, light ones included;
   - `omarchy font set`;
   - the clipboard bindings;
   - the portal folder picker;
   - tiled and floating windows;
   - fractional scales;
   - readability at the inactive-window opacity.

   Record it in the validation matrix and update the README's support table.
6. **Launch kit.**
   - A short native recording of `omarchy theme set` cycling while GitTurtle retints
     beside the terminal.
   - A README section for Omarchy users, with an optional launch-or-focus binding for
     `~/.config/hypr/bindings.lua`.
   - The announcement text, claiming only what phase 4 recorded.
7. **Upstream, optional.** Once a package exists, propose an Install-menu entry
   (`omarchy-install-*` plus `omarchy-menu.jsonc`). GitTurtle needs no theme template.

The announcement waits for phases 1 to 5. Phase 7 depends on maintainers and is not
required for the claim.
