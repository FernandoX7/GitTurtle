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
  is `com.gitturtle.GitTurtle` (until 2026-10-26, `com.gitturtle.desktop`), the same as its
  desktop entry's name and `StartupWMClass`. Every window gets Omarchy's default opacity
  (0.985 active, 0.96 inactive) unless a rule removes it; Omarchy exempts browsers, video,
  games, password managers and virtual machines, not terminals or editors.

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
     ignoring dispatched keys until it was restarted. The owner's physical check,
     2026-10-26: Super+V pastes into text fields; nothing in the commit message could
     be selected or copied, because no read-only text in the app was selectable.
     Fixed: the commit message and its metadata select and copy like ordinary text,
     and list Copy works in History and the changed-file list
     ([validation entry](../validation.md#october-26-selectable-commit-text-the-new-application-id-and-omarchy-polish)).
     The owner confirmed copying and pasting with a physical keyboard on 2026-10-26.
     Working Changes and the file-history revision list followed on 2026-10-26
     ([validation entry](../validation.md#october-26-list-copy-in-working-changes-and-file-history-and-select-all-everywhere)).
   - **Lists.** Outside text fields, dispatched keys reach GitTurtle. Ctrl+C on a
     selected History commit or changed file copies nothing: the shortcut sheet lists
     "Copy Selected List Item" (Ctrl+C), but only Blame handles it. What a copied commit
     or file holds was settled on 2026-10-26: the full hash for a commit, the
     repository-relative path for a file.
3. **Polish found on the way** (first-launch check, 2026-10-26):
   - In a half-width tiled window (845 px wide), the empty History-search message was
     clipped at the list's right edge ("Searc…"). Fixed: it is centered in the visible
     part of the list.
   - Page Down did not scroll Settings. Fixed: Page Up, Page Down, Home and End scroll
     it.
   - Omarchy's default 0.985 active opacity lets text behind a floating window show
     through diffs. Decided 2026-10-26: keep the desktop's default, as terminals and
     editors do, and document an opt-out rule in the
     [Linux guide](../linux.md#arch-linux-and-omarchy).
   - Omarchy scripts launch apps with `gtk-launch <id>`, which failed for the old
     `com.gitturtle.desktop` ID because the desktop file was
     `com.gitturtle.desktop.desktop`. Decided 2026-10-26: the application ID is
     `com.gitturtle.GitTurtle` on every platform (Wayland app_id, X11 class, desktop
     file, macOS bundle identifier), reverse DNS on the project's domain as Flathub,
     GNOME and KDE expect. The installer replaces the old launcher and icons.
   - **Approved by the owner on 2026-10-26, all to be done before distribution:**
     1. List Copy in Working Changes and the file-history revision list. Done: the
        selected files' paths, one per line in list order, and the revision's full
        hash ([validation entry](../validation.md#october-26-list-copy-in-working-changes-and-file-history-and-select-all-everywhere)).
     2. Omarchy 4.5's Super+A (which sends Ctrl+A) does something sensible on every
        surface: text fields, the commit message, Working Changes, and the lists.
        Done, checked natively with no code change: text fields, the focused message
        and the Compare diff select all their text and Working Changes selects every
        visible row. Lists that hold one selection (History, changed files,
        revisions, Blame, navigation) and pages without a text field ignore it.
        Decided 2026-10-26: that is GTK's and macOS's behaviour for single-selection
        lists, and a list that cannot select more must not move focus or select
        something else.
     3. `omarchy font set` reaches GitTurtle's code text, checked natively and fixed if
        needed. Done: it reached code text only on the next focus; GitTurtle now
        follows the fontconfig directory and applies a change at once
        ([validation entry](../validation.md#october-26-omarchy-font-set-reaches-code-text-at-once)).
     4. Every stock Omarchy theme (22 on 4.0.4-mac.1, light and unusual ones included)
        reads well in History, Compare, Changes, Settings and dialogs; fix what does not.
        Done: eight fixes (`1adbdf4` to `7553ddb`) and two the re-sweep prompted (menu
        separators `9eb9a36`, the edge above the shadow `de4a8a2`), checked in all 22
        themes ([validation entry](../validation.md#october-27-every-stock-omarchy-theme-after-the-contrast-fixes)).
        Medium and heavier interface weights rendered as Regular on Linux, because
        `.SystemUIFont` is IBM Plex Sans there and it was not installed; the owner chose
        (2026-10-27) to fix it in the way best for Linux and the project, legally. Done:
        Linux builds embed IBM Plex Sans, `ad04939`
        ([validation entry](../validation.md#october-27-the-linux-interface-draws-its-real-weights)).
     5. Open in Editor uses the person's Omarchy editor (`omarchy launch editor`) when
        no external editor is set. Done, `bb98a57`
        ([validation entry](../validation.md#october-26-omarchy-editor-default-terminal-and-a-launch-or-focus-binding)).
     6. An Open in Terminal action for the repository, in the desktop's default
        terminal. Done, `bb98a57`, with the editor, terminal and file-manager
        handoffs added to Linux's Menu, where only the command palette had them.
     7. A README section for Omarchy users with a launch-or-focus key binding for
        `~/.config/hypr/bindings.lua`. Done: README's "On Omarchy" section.
     8. The 2026-10-01 queue in `tasks.json` that has not landed (for example
        `copy-actions-say-copied`, `switch-focus-ring-and-hover`,
        `switch-checked-knob-and-disabled-label`, `settings-focus-reveal-and-steady-switch`,
        `markdown-diff-fence-colours`, `review-file-preparation-lifecycle`,
        `narrow-large-text-compact-chrome`, `review-arrow-buttons-hover`,
        `menu-checked-items-accessible`, `builtin-light-hover-lift`); confirm each against
        `main` first. Checked 2026-10-26 against `origin/main` `a755d60`: landed are
        `history-vanished-scope-refresh`, `auth-prompt-platform-wording`,
        `editor-highlights-keep-syntax-readable`, `push-upstream-only-when-unset`,
        `pr-source-gutter-like-compare`, `decorations-in-retained-bounds`,
        `code-font-cold-launch-wait`, `code-font-blame-conflicts-review-gutter`,
        `narrow-large-text-minimums` and `history-columns-steady-after-toggle`; still open
        are the ten named above, plus `switch-focus-ring-and-hover` and
        `settings-focus-reveal-and-steady-switch`, which are in that list. The stock-theme
        sweep (item 4) found `switch-checked-knob-and-disabled-label`'s knob at 1.2–2.5:1
        in six themes. Done on this branch: `switch-checked-knob-and-disabled-label`
        (`1adbdf4`), `switch-focus-ring-and-hover` (`4401009`,
        [validation entry](../validation.md#october-27-switches-show-focus-and-hover)) and
        `settings-focus-reveal-and-steady-switch` (`959fe6b`,
        [validation entry](../validation.md#october-27-settings-keeps-keyboard-focus-in-view-and-switches-still)).
     9. Any further UI or UX improvement found along the way: the owner wants a
        first-class Omarchy app, good enough to be featured. Done so far: the Changes
        status bar counts the working tree instead of History's files, `8f2e8da`
        ([validation entry](../validation.md#october-27-the-changes-status-bar-counts-the-working-tree)),
        and the navigation's section labels are 11 px medium, `25433f7`. Owner
        decision, 2026-10-27: take the recommendation on every taste item from the
        theme sweep and the font reviews. Not started; each needs native evidence:
        - Raise the command palette onto the menus' raised surface and edge; today it
          is a dialog on the page background.
        - Light-theme disabled text from about 2:1 to about 3:1, with one disabled dim
          for Switch labels, Buttons and menus. Today there are two, though the theme
          comment beside the disabled foreground claims one.
        - The palette's highlight starts on the first enabled row.
        - Disabled primary buttons (Lumon's Open command, the identity button) and
          Retro-82's grey disabled Push take the neutral disabled fill.
        - Popup edges fitted to 1.4:1 rather than exactly 1.3, so antialiasing and
          fractional scales keep them above 1.3.
        - Settings' Git identity Name and Email labels medium, like the Projects
          titles beside them; disabled palette row titles Regular, so weight and
          colour both mark them; theme-card names Medium rather than SemiBold.
        - Reword the Changes empty state so "history." does not sit alone on a line
          at 640 px.
        - Kept as they are: Kanagawa's navy menus (the theme's own raised surface) and
          light-theme menus equal to the canvas (their edge and shadow carry them).
4. **Distribution.** Publish versioned release bundles (x86-64 and AArch64, checksums,
   build identity). Resolve the two dependencies the packager flags for license review
   (`mac`, `ufbx`) before distributing. Then add an AUR package (`gitturtle-bin`) and
   later propose GitTurtle to `omacom/omarchy-pkgs`, so `omarchy pkg add gitturtle`
   works. The owner also plans macOS downloads from gitturtle.com, signed with the
   owner's Developer ID ([release workflow](../releases.md#macos-developer-id-signing-component)
   covers signing and notarization), with automatic update checks. No updater exists
   yet, and an automatic check is a network action without user interaction, which the
   [product rules](../../AGENTS.md) forbid today. Owner decision, 2026-10-26: updates
   and update checks are a later feature, after the app itself is done; they are not
   part of this plan.
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

The announcement waits for phases 1 to 5. Owner decision, 2026-10-26: every Omarchy feature is finished before distribution (phase 4) starts. Phase 7 depends on maintainers and is not
required for the claim.
