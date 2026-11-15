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
        [validation entry](../validation.md#october-27-settings-keeps-keyboard-focus-in-view-and-switches-still))
        and `markdown-diff-fence-colours` (`fd551c6`, `278b822`,
        [validation entry](../validation.md#october-27-diff-fences-in-markdown-source-take-the-patch-colours))
        and `review-file-preparation-lifecycle` (`ff7fc3c`,
        [validation entry](../validation.md#october-27-pr-review-files-prepare-without-a-copy-and-resume-on-reopen)),
        `review-arrow-buttons-hover` (`2ab3348`) and `menu-checked-items-accessible` (`f57a514`,
        [validation entry](../validation.md#october-27-ghost-buttons-lift-on-hover-and-ticked-menu-items-are-check-items)),
        `builtin-light-hover-lift` (`63414e3`,
        [validation entry](../validation.md#october-27-light-palettes-hover-stands-off-subtle-and-pressed-ghost-buttons-take-selected)),
        and, awaiting native evidence, `narrow-large-text-compact-chrome` (`d1299fb`) and
        `copy-actions-say-copied` (`2133809`).
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
        Owner decision, 2026-10-27 evening: handle everything found during item 8 and take
        every recommendation, aiming for an incredibly beautiful UI and a wonderful user
        experience; do the Linux wording sweep first. Each item is one cohesive commit with
        native evidence:
        - Linux wording: no macOS store, shortcut or app name in text a Linux build shows
          ("Drafts stay on this Mac", the conflict tooltips' "Command-Option-Up/Down",
          `repository_access.rs`' "Command-O", and the GitHub Keychain messages wherever
          Linux can reach them); shortcut text from `shortcuts.rs` labels; audit with
          `grep -rnE '"[^"]*\b(Mac|macOS|Keychain|Finder|Command-|⌘)' crates/app/src`.
        - Disabled menu items announce themselves disabled (the kit's menu item needs a
          wrapper element that sets AccessKit's disabled flag, as gpui-base's Button does).
        - Escape closing the GitHub panel returns visible focus to what opened it.
        - The PR list row: title and second line share one left-aligned text column.
        - The review panel's Source editor reaches the file list's bottom: no half row cut,
          no empty dialog below it.
        - "Context 3 +" becomes a verb-led label, such as "More context (3 lines)", in the
          full row and the Options menu.
        - The narrow review caption never shows a lone "Origin…": below the width where it
          can show its words it leaves the row, its text kept in a tooltip or the menu.
        - The changed-file list keeps its focus ring while it has keyboard focus, even after
          the pointer moves.
        - Under a modal dialog the status bar shows the dialog's hints, or none, not the
          page's.
        - Braden and Rosé Pine Dawn take a slightly darker `selected`, so pressing a
          secondary control reads as a deeper press with margin over the pressed step.
        - Copy feedback: a Hash "Copied" never carries to another commit selected within
          the 1.5 s; check the dialog announcements with Orca; `copy_feedback.rs` in the
          app crate guide's routing table.
        - Compact chrome: the 18 pt composer heading reads "Commit to main" rather than
          "C…"; the breakpoint counts Compare's need at large code sizes.
        - Reflog: its text is not clipped at the left edge in a 461 px window.
        - Evidence still owed (capture with the window forced opaque): a hovered row beside
          the selected row in Braden and One Light, pressed Fetch in Braden and Rosé Pine
          Dawn, the Reflog's current entry, and the toolkit's own ghost controls.
        Found by the native reviews of 2026-10-27 late, handled the same way:
        - Linux wording: done, `5ce690b` and `69dc3de`
          ([validation entry](../validation.md#october-27-a-linux-build-names-linux-keys-and-offers-no-mac-only-action)).
          System preview is hidden on Linux, where it could only fail; whether Linux gets an
          `xdg-open` equivalent is the owner's call (not on this list). The rebase dialog's
          keyboard help is not on the accessibility bus.
        - Compact chrome (`d1299fb`): at 18 pt in 461 × 490 the Working Changes file list has no
          row and its focus ring collapses to a line; the commit title field, Actions and
          Refresh are hidden; the compact Changes toggle loses its count; the History and
          Changes toggles move between modes; inspector rows show lone ellipses, the Compare
          footer clips mid-character, Diff/Split/Before/After wrap, History's narrow column
          drops the subject, and the profile icon differs; the 13 pt composer is cramped and
          the inspector's left insets differ.
        - Copy feedback (`2133809`): start-align the copy cell, so Hash keeps its place at rest
          and Copied replaces the icon in place; key the mark by commit, path and side; check
          Hash at 18 pt in a half tile; frames of a copy while hovered, pressed, keyboard
          activated, from an icon-only button and repeated.
        - Item 9 wave 1 (`0aa3352`, `d5f7f9c`, `268dc23`, `5d543db`, `0a1e652`, `6ab4dc7`, natively
          checked): a menu label (the moved review caption, and the project pane's, profiles',
          branch actions' and model view's labels) draws in the disabled colour and is announced
          as a menu item, and the kit's first Down selects a leading label; (the popover was thought to drop its trigger's margin; it did not, and the narrow caption's width formula was the error, fixed in `c831033`); the narrow "More context" item's accessible name differs from the
          button's; `native-accessibility.md` still says "Context"; the Reflog's tooltip shows
          when nothing is cut, and its header, entries and filter start at three insets; Quick
          Open and Keyboard Shortcuts are not yet on the raised surface.
        Owner decision, 2026-10-27 late night: approved the coordinator's UX recommendations,
        each with its DESIGN.md change in the commit that implements it:
        - Narrow History keeps the subject: below a measured width the subject never leaves
          (floor about 120 px, then an ellipsis); References fold into the start of the subject
          cell as one pill (HEAD's branch, else the first ref, plus "+N"; the full list in the
          tooltip and accessible name); the graph takes a compact lane pitch. The saved column
          choice is untouched, as the rail leaves the saved navigation choice; widening restores
          the chosen columns. Not chosen: scrolling the table to the subject, hiding columns.
        - One responsive system: a single priority order for every page as width shrinks
          (content first, then actions become icons, then labels go, then secondary columns),
          replacing per-control breakpoints where they disagree, with a test that walks widths
          from 461 to 1,480 px at 13 and 18 pt and fails on cut or orphaned text.
        - A layout grid: shared inset tokens and a test that measures each panel's text start
          positions, so a panel's header, rows and fields share one left edge.
        - A focus-visibility sweep: a test that tabs through every page and dialog and asserts
          a visible, whole ring (all four sides) at every stop and never focus on nothing.
        - Feedback where the action happened, as Copied does: stage and unstage, fetch, pull and
          push finishing, branch creation and similar confirm on the control or row the user
          touched; the status bar keeps history, not confirmation.
        - Empty and edge states as designed screens: no working changes, no remote, no upstream,
          detached HEAD and similar each show one deliberate line with their next action.
        - Motion with intent: short, reduced-motion-aware transitions only where they explain a
          change (a row moving between Staged and Unstaged, the compact chrome switching).
        Owner decision, 2026-10-27 late night: Linux gets a System preview that opens an isolated
        read-only copy of the captured bytes in the desktop's default application (the
        OpenURI portal or `xdg-open`), after an independent security review of the candidate
        under [security review](security-review.md) and before integration.
        Found while implementing the late-night items (open): other lists still lose their ring
        on pointer movement through GPUI's `focus_visible` (rewrite review, discard and removal
        reviews, the GitHub panes, the model view; the focus sweep covers them); disabled
        primaries and selected default actions in dialogs sit on `canvas` at 1.03–1.05:1 with no
        edge in Rosé Pine, Graphite, Midnight, Braden and Ember; narrow History's tooltip covers
        the whole row (the pill alone would interrupt less), wide rows' accessible names omit
        their references, the selected lane is not scrolled into view after the window
        narrows, and History's rows draw 1 px right of its header; History shows no tags.
        Found by the native reviews of 2026-10-27 night (frames `.local/item8-2026-10-27/tools/qa/`):
        - Compact chrome follow-ups (`cf-3f23c62` → `cf-61653f0`; toggles held in place, the count
          badge, the Diff mode menu, Blame as an icon, whole paths and the unfilled profile icon
          verified): the short Working inspector shows only the "Staged 2" header, no file row (13
          and 18 pt); at 18 pt "Unstage all" runs past the window, "0/72 suggested", the summary
          placeholder and "Write a…" are cut, and the scrolled controls cut the filter's bottom
          border; the Working list's ring has no left side (all sizes); AT-SPI reports Directories,
          Stage, Unstage and the description outside what is drawn; the full chrome's History toggle
          moves 116 px between History and the other pages; Blame and the profile share one person
          glyph; taste: the 18 pt header's lone "3" chip, the badge without a cut-out, "Diff" text
          28 px right of the path; still unexercised: the breakpoint at large code sizes and the
          footer's truncation step.
        - Kit menus (`km-61653f0` → `km-47f9b06`, all six claims verified natively): a menu heading
          and an item differ only by colour (1.86:1 Midnight, 1.93:1 Braden; One Dark's and
          Solarized Dark's secondary text nearly equals their text), so draw headings in the
          section-label style and say so in DESIGN.md; the profile menu's "Apply a saved Git
          profile…" heading reads as a command (say "Saved Git profiles", "Current: …"); the
          Options caption beside a disabled Reset review reads as an option; the keyboard
          highlight in menus is 1.19:1 (Midnight) and 1.23:1 (Braden) against the surface;
          DESIGN.md's "current place checked" can add "and unavailable"; still owed: an AT-SPI dump
          with label and separator roles, focus after the first Down, a frame at `c831033`'s
          boundary width, Orca speech.
        - Polish (`pl-47f9b06` → `pl-a762727`, verified natively except the two noted): the
          selected Keyboard Shortcuts row's key chip edge falls to 1.01:1 (Midnight) and 1.24:1
          (Braden) on the selected fill; the palette's status bar says "Esc Close" beside a
          Cancel button while Quick Open says "Esc Cancel"; the Reflog and the rebase entry form
          open with no visible focus; the GitHub panel uses four left insets; at 640 × 680 the
          Changes list cuts the "Unstaged 1 / Stage all" header through its glyphs; AT-SPI reports
          Reflog entries below the dialog; still owed: the rebase status bar with its commit list
          loaded, and Reflog hovers over a cut and a whole entry.
        - Fix-ups (`fx-a762727` → `fx-2913b86`): Midnight's disabled Fetch, Pull and Push rest on
          `subtle` at 1.046:1 with no edge (DESIGN.md names them; wrap the toolbar actions in
          `appearance::keep_disabled_shape`); the review Source leaves a 39 px strip where a 14th
          36 px line fits; Settings' disabled Save editor has no shape; taste: the review box's edge
          moves 26 px between Changes and Source (Source could reserve the hint's row).
        - Narrow History (`nh-2913b86` → `nh-f5e70d3`): at 461 × 490, 13 pt, the pill (44–45 px)
          takes the floor and leaves the subject about four characters, against the owner's "the
          subject never leaves": measure the floor on the subject text and, below it, shrink the
          pill to a branch glyph plus count; a 15 px strip of cut Author letters at the viewport's
          edge (the layout's width is about 26 px under the drawn table, and the graph cell 12 px
          wider than computed); the list's ring has no right side at 461 and 640 px; HEAD reads
          "main +3" while wide badges put "+N" outside the chip (one treatment); still owed: the
          tooltip frame and the release History scroll measurement.
        - Menus after `16220ee`…`3e297d6` (`mc-f5e70d3` → `mc-3e297d6`; headings, profile rows,
          the keyboard cursor bar, the selected chip edge and the Escape verbs verified in Midnight,
          Braden, One Dark, Solarized Dark and Kanagawa): in One Dark and Solarized Dark the 12 px
          Medium headings outweigh the commands, and DESIGN.md gives section labels 11 px in one
          place and menu headings 12 px in another (use one size); the Options caption "Original
          Git diff", now a heading under a separator, reads as the title of an empty group (say
          "Showing the original Git diff"); Quick Open says "1 matches"; not captured: pointer hover
          without the bar, a submenu, "Saved Git profiles" with saved profiles, the profile menu in
          a 461 px window or with a long identity (its width caps at 500 px), a light palette whose
          menu equals its canvas, label and separator roles on the bus.
        - Linux System preview security reviews (2026-10-27 night): the first failed (F1 a malformed
          ZIP cost 25.6 s per side at Compare open and made the external-link verdict
          order-dependent; F2 `xdg-open` typed some copies differently from the content check; F3
          OpenDocument skipped the external-link check), all fixed; the second failed on office
          documents (a ZIP Unicode Path field hides a relationship part; Word field codes and Excel
          data connections reach outside the package without relationships; an OOXML/OpenDocument
          hybrid passes as Word) and on the portal typing PNG and Ogg copies by content (a PNG with an
          SVG marker opens in the browser as SVG). Coordinator's decision, failing closed: office
          documents get no Linux offer for now, and every portal request first checks GIO's content
          type against the kind. Deferred, for a later task: office documents with a bounded body
          scan (field codes, data connections, hybrids, extra-field names) and a release measurement
          at Compare open. The third and fourth reviews found media unsafe too (an SDP description
          in an MP3 or Matroska copy made mpv open RTP sockets; a media copy FFmpeg cannot open is
          retried through libarchive and a playlist inside it reaches the network), so Linux offers
          only raster images, PDF and plain text. Deferred: audio and video through a sandboxed or
          network-restricted handoff.
        - Working inspector after `67ce1fb`…`147c37c` (`wi-f5e70d3` → `wi-0aca11e`; one-row
          controls, list and first-row bounds, icon-only Unstage all, "0/72", "Commit" verified):
          the composer's "Description · optional" label is cut by the commit button and its entry
          is an undrawn Tab stop below the window (regression from `84761b2`); at 18 pt in 461 px
          "README.md" still reads "README…" beside its status tile; the filter placeholder reads
          "Filter wo"; the Actions trigger is a bare chevron; the repository tab clips its own close
          button at 18 pt in 461 px (pre-existing); not captured: the Actions menu's new items,
          the row context menu's Stage/Unstage, tooltips.
        - Narrow History after `c2ac707`…`3a3c56f` and fix-ups after `0e8ecd6`…`0aca11e`: at
          461 × 490, 13 pt the graph draws a third lane cut in half at its edge (regression); the
          floor figures in DESIGN.md are macOS's; the review Changes list's 13th row is 35 of 40 px;
          the hint's baseline sits 4.5 px above the page buttons'; Changes and Source use different
          insets; the pill tooltip is still to capture.
        Found by the native reviews of 2026-10-28 (frames `.local/item8-2026-10-27/tools/qa/{mn,wi2,hd,gr,nh3}-{0aca11e,48660d6}/`; [validation entries](../validation.md#october-28-menus-headings-at-11-px-the-original-diff-caption-as-state-1-match-and-menus-inside-a-narrow-window)):
        - Working inspector: the filter's accessible name follows its fitted placeholder; the short
          composer's leftover height lands between the title field and the commit action (20, 18
          and 33 px against a scaled 4 px); the labelled Actions trigger is 39 px tall in AT-SPI
          against the 28 px helper; the title placeholder reads "Summarize t…"; DESIGN.md's status
          tile sentence predates the bare glyph; narrow Compare at 18 pt cuts its path to "READ…".
        - Header: the History inspector's filter placeholder is cut mid-word at 18 pt; the wrapped
          13 pt header's second row starts with an empty slot in History; about 65 logical px of
          dead space after Back in the full header; the badge ring in the header's surface reads as
          a halo on the toggle group's fill (1.05:1, 1.13:1); taste: Blame's glyph can read as a
          list.
        - GitHub panel: Changes' code starts 10 logical px left of Source's; "100 files per page ·
          GitHub limit 3,000" starts off the panel's edge; in Files the path header and hint start
          at the box edge while the mode labels start inside; taste: Changes rows 20 against
          Source's 18 logical px, "Draft saved locally" ends off the right edge, unselected PR rows
          have no resting surface.
        - Narrow History: DESIGN.md says a 461 px window shows one lane where Linux draws two; at
          18 pt the graph drops a third lane the base drew whole; rows report AT-SPI bounds past the
          window; still owed: the pill hovered by itself, the compact pill's tag glyph and the root
          row's tooltip, the narrowing for `7f5c7b1`.
        - Menus: label and separator roles on the bus (the capture tool records items only), the
          807.75 px caption move, History search's "1 match", Quick Open's count on AT-SPI (Orca).
        - System preview: the image control's accessible name omits the type ("Open After in
          default app") where PDF and text name it ("…as a PDF document in the default app").
        Found by the native reviews of 2026-10-28 afternoon (frames `.local/item8-2026-10-27/tools/qa/{t2,f2,d2,g2}-{48660d6,04b6bc9}/`):
        - Regressions: the Working composer's leftover height falls below the composer, a 3.5–14 px
          band under the commit action (`cc52a97`/`403563e`); at 18 pt in 1480 × 800 History's
          metadata draws a scrollbar for about 16 px of overflow since Actions became 28 px
          (`eac91cd`); at 18 pt "GitHub limit 3,000" sits below the review body's fold (`5c32386`).
        - Accessibility: a Working row's menu opens only on a right press (no Menu key or
          Shift-F10 in `GitTurtleList`); at 13 pt in 461 px "Compare against parent 1" and
          "Selected commit actions" stay in the tree below the revision details' clip; History rows
          still report bounds past the window; the Reflog and rebase dialogs are announced as the
          fallback "Alert dialog", not by their titles.
        - Polish: History's search field is named by its placeholder, which its clip cuts at 18 pt
          ("…history, a"); at 18 pt in 461 px the tab name gives way ("com…") while Menu and
          Workspaces keep their labels, and the inspector heading reads "Change…" (drop the label,
          keep icon and count); at 18 pt in 1000 × 680 commit Actions sit below the metadata's fold;
          the review body has no scroll affordance when it overflows ("Recover drafts · 1" at 18 pt);
          the Changes hint wraps inside its last phrase at 18 pt; a 0.5 px teal fringe outside the
          compact badge's cut-out (1.23:1, 1.43:1); DESIGN.md's keyboard section still says narrow
          History keeps the whole lanes its room fits.
        - Taste: Copy message and Copy hash share one glyph; the profile button is a filled 50 px
          tile beside a 39 px unfilled Settings at 18 pt; review Changes rows 20 against Source's
          18 px; unselected PR rows have no resting surface.
        - Found by the layout grid (`c757a0e`): its `KNOWN` entries (inspector, narrow History,
          Working files and composer, GitHub panel, project hub and pane); Settings is not measured
          yet; the rebase (720 px) and Quick Open (700 px) dialogs draw past a 461 px window's left.
        - System preview at 461 × 490: the image control wraps under the file-status line, on the
          bus but not visible (regression from `7e0e472`/`1bab68d`); PDF side headers differ by 10
          and 14.5 px; one wrapped image caption drops the shared top; the image mode and zoom
          groups are cut at the column's edge (pre-existing).
        - Viewport: after one resize with no input the base kept its wide layout for 2.5 s
          (one candidate run did not, unexplained); a 180 px sidebar wraps its segments and the
          footer hint (pre-existing).
        - From the implementers: after a Stage the Working list moved down 41 px in the first frame
          (the post-write status above it); a running History search leaves the list 2 px tall at
          13 pt in 461 × 490; the GitHub panel's Name repository sits below its fold at 18 pt in
          461 × 490 with a cut ring.
        - Not captured yet: the composer's conflict note (`56c2d69`) on a conflicted fixture at
          18 pt in 461 px; a long draft status (`7b2adc1`); a branched fixture for narrow History's
          lanes at 18 pt; Back hovered, pressed and focused at 18 pt; the tab's close button focused
          at 18 pt in 461 px.
        Found by the native reviews of 2026-10-28 evening (frames `.local/item8-2026-10-27/tools/qa/{pre,prf,prg,prm,t2,f2,sp2,sp2m,sp2c}-{04b6bc9,00f0add}/`):
        - Regressions: at 13 pt in a 279 px inspector labelled Actions wraps onto its own row (the
          toolbar is sized from the saved inspector width, not the drawn one; `f07233b`); at
          461 × 490 a refusal paints over the image's two notes (`64d5d89`).
        - Empty and edge states: no state line is on the accessibility bus; at 18 pt in 461 px the
          detached line is not drawn, "No remote" reads "No r…", and a search with no match leaves
          the list 11 px tall; the Tags line starts 10 px right of the dialog's edge; Tags offers
          Create tag… with no commit; Add remote…'s cloud glyph beside Fetch's; Name repository's
          magnifier and its invisible focus move.
        - Feedback and motion: on Changes the next refresh replaces the outcome in the status line;
          Stage all confirms on a heading scrolled out of view; after Fetch a full-width
          "1 new commit" bar moves the page 35 px (pre-existing); before a row's flight the list is
          empty for a frame with "Filtering working paths…" pushing the header 21 px (the filter
          clears its rows before its worker answers); a still pointer ends over another row's
          control after a re-sort; the candidate's idle capture cadence was slower (measure idle
          CPU for continuous redraws); taste: a 1 px edge on the travelling row, "Fetched" centred.
        - Menus and inspector: a keyboard-opened menu highlights nothing until Down; the Working
          Actions menu covers the status bar at 461 × 490; "Ignore applies to untracked files"
          reads as one phrase; at 18 pt in 1,000 × 680 the metadata scrolls 13 px of padding; at
          18 pt the navigation's mode buttons wrap; "GitTurtle QA" is clipped in the inspector at
          18 pt.
        - System preview: notes cut to one line at 640 × 680 with room for two; the image stage is
          27 px at 13 pt and 0 at 18 pt in 461 × 490.
        - From implementers: every app dialog has the alert role; the per-row accessibility clip
          needs a release History scroll measurement; History's commit list has no Shift-F10 menu
          and the trees no Menu key; "Up to date" and tracking-branch creation still use the
          window notice.
        Found by the native reviews of 2026-10-28 night (frames `.local/item8-2026-10-27/tools/qa/{t2,f2,d2,g2,q3d,q3c,q3g,q4*,prg,prm}-{00f0add,91fb95e}/` and `{pre,prf,sp2*}-{91fb95e,7342c3e}/`; [validation entries](../validation.md#october-28-night-the-header-gives-way-by-icons-historys-search-is-named-by-its-label-and-rows-report-what-is-drawn)):
        - Regressions (fix first): at 18 pt in 461 × 490 the Stashes dialog cuts its "Choose a
          stash…" line mid-line (`dd922c9` with `cb7fa2e`); the keyboard commit menu ends 2 px above
          the status bar instead of 8 (`71ab8a4`).
        - Accessibility: Stashes' title, header and placeholder, the palette's title and hint, Quick
          Open's and Keyboard Shortcuts' hints and Projects' line are still unnamed on the bus; the
          inspector title reports bounds past its frame; the shortened Shortcuts list shows no
          scrollbar; Projects opens with no visible focus; in icon form the selected navigation
          mode is shown only by a 1.40:1 fill; the commit menu's edge lands on the graph lane.
        - Feedback: Stage all's "✓ Staged" is drawn in the disabled colour (3.19:1) while it
          confirms on an emptied group; the new-commit cue and Latest's dot stay through Pull and
          Push once the commit is on top, and the dot sits 1.5 px from Latest's arrow at 18 pt;
          a quick write flashes the busy chrome for under 200 ms and, since `541d974`, the write
          controls dim while rows are pending (delay the busy look); a still pointer ends over a
          moved row's control with its tooltip; the list's bottom jumps 23 px for one frame; a
          test should pin that a history notice ends no control's confirmation.
        - Empty states: the Working filter's no-match line is missing at 18 pt in 461 px and, at
          1,480 px, sits above the Changes header and pushes it 21 px (draw it inside the list
          with Clear filter); the detached compact bar shows two commit glyphs and never the
          word; the wide bar's detached picker uses a branch glyph; the compact "No remote"
          glyph is the cloud beside Fetch's.
        - System preview: at 18 pt in 461 × 490 the refusal band shows only "Could not / open
          After." and at 13 pt cuts its fourth line; the image stays hidden while it shows
          (decided: a short reason-first line, capped at two lines when a taller band would
          take the image's 96 px minimum); its surface spans both halves while its text wraps
          in After's; a sub-pixel image sliver and a doubled rule at its foot; the 100 px cap cuts
          a line at 640 × 680; the inspector title breaks before the file name's last letter at 18 pt; before
          any click at 461 × 490 the image gets 60–83 px (the give-way of the side dimensions).
        - Dialogs: Keyboard Shortcuts at 13 pt in 461 × 490 ends 14 px over the status bar (cap
          dialogs at the bar's top); at 18 pt the Reflog opens with its focused filter below the
          clip; Worktrees and Tags open with no visible focus; the Reflog's first-row ring is cut
          at its list's clip, it lists an undrawn row (HEAD@{6}) and has no scroll cue; Worktrees
          shows one of two rows with no cue; Quick Open's hint leaves "cancels" alone at 18 pt;
          Tags' placeholder is cut at 18 pt; Worktrees and Tags centre their rows' labels; the
          Midnight backdrop barely separates a dialog from History; right-click menus near the
          status bar can still overlap it (a kit margin); Stashes' hint leaves "files." alone.
        - GitHub panel: a reopened PR's focused Conversations pane loses its ring's top edge;
          opening a PR in a narrow panel shows only its title; revealing a conversation slices
          its author line; wrapping to the first stop leaves the body 6.5–10 px down; the
          file-page bound is cut 3 px at the fold at 18 pt; the right edge moves 19 px between
          section tabs at 13 pt; the scroll cue is 1.52:1 / 1.28:1; the fixture banner breaks
          inside a phrase; the range hint ends a line on "·"; the status bar is reported
          unclipped on the bus.
        - Inspector and Compare: Changed files rows report bounds past their list; P1's ring at
          18 pt in 1,000 × 680 is 1 px from the clip (the sweep runs only at 1,280 × 800); P1
          ends 0.5 px above the divider; about 60 px stays blank between P1 and Changed files at
          13 pt; Compare's ring and the selected file's accent bar read as one doubled stroke at
          13 pt in 461 px; the File actions "…" beside the Working pane's "…"; a start-elided long
          path keeps no directory; the status bar's timing is cut mid-number at 18 pt; History's
          changed-files filter empties its rows before its worker answers; a saved inspector
          width is stored unclamped; the width walk never draws the inspector narrower than its
          saved width.
        - Header and navigation: 183.5 px of the tab strip stays empty when both header controls
          are icons at 18 pt in 461 px (labels could give way one at a time); the branch filter's
          placeholder and Local/Remote/Worktrees at 18 pt (in a brief).
        - Driver gaps (tools, not product): a tap does not move keyboard focus (P1's ring),
          Return from History opened the commit actions menu instead of Compare, the 18 pt
          GitHub list in 461 px, the review text and Files in the candidate's walk.
        Found by the implementers and reviews of 2026-10-29 (owed native checks in
        `.local/item8-2026-10-27/native-checks-2026-10-29.md`):
        - Bugs: History's Changes body is measured a frame late, so any one-frame height change
          above it brings back the 23 px jump; while a write's busy look waits its 200 ms the write
          controls are announced enabled and an opened Actions menu lists enabled items (each
          refuses); the Targets inputs, History's Latest and "View commit", and dialogs still dim at
          once when a write starts; Quick Open's error line is not an alert.
        - Focus and selection: labelled navigation modes and the header's History/Changes toggles
          show selection by a 1.40:1 fill only (icon form now has a 2 px accent bar); Settings opens
          with focus on its root (in a brief); the worktree manager's details are not revealed by
          Tab (in a brief).
        - Edges and menus: the light palettes' dialog edge is 1.28:1 (Braden; needs a kit
          `dialog_edge` like `popover_edge`); the commit menu should end at the list's trailing
          inset rather than land on the graph lane; right-click menus near the status bar need the
          kit's margin; the Shortcuts scrollbar's canvas track on the raised picker needs a design
          look.
        - Motion (decision: keep DESIGN.md's instant re-layout and draw the travelling row
          beneath section headings, which are not rows): during a row's flight the Unstaged
          heading snaps and the travelling row covers it.
        - Tests: the focus sweep once counted History's selected-row bar beneath a dialog's
          backdrop as a cut ring (`891f109` moved the Stashes walk off History; root cause in a
          brief); wall-clock waits remain in `empty_state.rs`, `state_tests.rs`, `width_walk.rs`
          and the theme editor (`SerialExecutor::drain()` for `drain_preference_writer`).
        - macOS later: VoiceOver on dialog titles (now value only) and the dialog's own label.
        - From the native review of `gitturtle-7abe8b2` (frames `tools/qa/r*-{a79b83c,7abe8b2}/`,
          sheets 59–66): Escape on the tracking-branch review leaves 23 of 30 frame children
          "Unknown object" on AT-SPI (pre-existing; Confirm leaves none); a capped menu lights two
          rows after one wheel notch (`9c24a9a`); keyboard row menus end 7.0 px above the status bar
          where DESIGN.md asks 8, and a raised row menu covers its own row by 5.5 px (place it wholly
          above the cursor row; say so in DESIGN.md); at 18 pt in 461 px the key hints cut a fresh
          outcome ("Up to date with ori…": drop or shorten the hints while an outcome is fresh,
          and walk it in `width_walk`); the ahead/behind counts are cut to "↑ 0…"; the Stashes
          actions fall below the fold at 1000 × 680 18 pt (decided: pin them as the detail's
          footer at 1000 × 680 and larger, keep the comparison at its placeholder height until a
          file is chosen); a scroll stop cuts Review restore… in the 67 px detail at 461 × 490;
          taste: with no stash the 219 px list holds one line above a disabled Previous/More, the
          tracking-branch tooltip covers the path line, the author tooltip repeats a whole label.
        Found by the implementers and reviewers later on 2026-10-29:
        - Layout: a branch picker at its 280 px limit overflows the ~242 px group at 18 pt; a long
          file name wider than Compare's whole row at 461 px (the fixture's
          `RepositoryNavigationSidebar.tsx`) is still start-elided (recommended: let the path take
          a second line at narrow widths rather than cut the name); `compare_minimum` does not count
          the row File actions wraps onto; scroll containers stop mid-control (no whole-control
          snapping in GPUI's scroll); in very short windows a keyboard menu caps below DESIGN's four
          rows; a dropdown moved up by the status-bar inset can cover its trigger (keep it clear of
          its trigger); the raised pickers' own edge is unmeasured against the dimmed page.
        - GitHub panel: the Files page's bound is cut 3 px at the fold at rest at 18 pt (layout,
          not reveal); at 461 × 490 the Conversations pane is taller than the body, so its ring's
          bottom is below the fold; when the content cannot scroll further (Recover drafts at 13 pt
          461) a top edge can still cut a line; after a keyboard open focus stays on the pane below
          the fold until the next Tab.
        - Filters: the Working Changes filter shares the History filter's executor and is not
          coalesced.
        Found by the implementers on the evening of 2026-10-29:
        - Feedback: Tags, Stashes and the rebase form, open when a write starts, refuse their
          actions silently and never announce or look unavailable; the profile picker still dims
          at once while its own profile save runs; Compare's change arrows refuse during a write
          but their keyboard shortcuts still move.
        - Menus: a right-click menu near the status bar is moved up by the kit's clamp over the
          clicked row (button menus now open above their button instead); right-click menus on
          branch, worktree and working rows read the busy state once when they open and are not
          rebuilt when a write starts or ends (button menus now are); a button that moves while its
          menu is open, without a resize, keeps the rows it was given at open.
        - Tests: the test node recorder keeps no live-region properties, so no test pins an
          assertive announcement (GPUI applies live regions only while assistive technology is
          active, so the recorder needs them recorded where they are set); the theme editor's
          `finish_transfer` drains the writer while `an_import_the_full_preference_executor_refuses_changes_nothing`
          still holds it, so a pending import hangs the test instead of failing it; real-time
          waits remain in `recovery.rs` (Stashes tests, the focus sweep's `landed` loops),
          `workspace/confirmation_tests.rs` (`finish`) and the helper near `views.rs:17624`.
        - Accessibility: `repository_tabs.rs` `local-workspaces-error` is an alert that never sets
          an assertive live region, so it is probably not announced.
        From the native review of `gitturtle-62d10ae` against `-7abe8b2` (frames
        `.local/item8-2026-10-27/tools/qa/s*-{7abe8b2,62d10ae}/`, write-up `findings-q8-2026-10-29.md`):
        - Bugs: a quick Stage or Unstage draws the branch picker and profile button in the busy grey
          for one frame (~25 ms) while the write runs; under a still pointer the pressed control's
          tooltip ("Stage gamma.txt") stays over the row that moved beneath it until the pointer
          moves; Settings opened by Ctrl+, with an Omarchy theme at 18 pt in 461×490 focuses the
          Omarchy card below the fold (the button reveals it); after a one-match Shortcuts search the
          status reads "1 shortcuts"; at 18 pt in 461×490 the image gets 61 px and Before's Open
          control wraps onto its own line.
        - Native against test fonts: a 480 px inspector in a 1,404 px window draws 454.5 px with Actions
          as text (455.5 and an icon expected); long Compare paths read "…n/RepositoryNav.tsx" and
          "…omponents/views.rs" with File actions beside them, not below (as in the base).
        - Design review of the same frames (sheets 67–75 in `before-after/`, specs
          `before-after/specs-q8/`): the busy frame also turns the commit button's label to
          "Working…"; the absent side's "No image on this side" is gone from Before's caption at
          461 px (`5755f3d` gave it way as a dimensions line); only Before's open control wraps at
          18 pt, costing the image ~48 px; the one Shortcuts match is cut at the list's foot (the
          group heading is not counted in its one-row minimum; both builds; the thumb is correct);
          reopening the GitHub panel scrolls the offline notice and Move fixture head out of view
          where offset 0 would show the focused field whole (prefer offset 0 when the stop is whole
          there); at 18 pt in 461 px Settings' "Follow system appearance" heading wraps to three
          lines beside a switch with the same words (labels go). Corrected QA calls: the commit
          menu's visible edge is 13.5 px inside the list (the 1 px border was measured); the
          640×430 image under the band is drawn whole, as intended; Details… as text at rest is
          DESIGN.md's rule.
        - Later the same evening: keyboard row menus (Shift-F10, the Menu key) still read the busy
          state and a paused integration once and are not rebuilt when either changes (button and
          right-click menus now are; while paused its Revert looks available and a press is refused
          silently); the amend and stash forms reopening with a paused-integration refusal have no
          test; Create tag's "Review tag" belongs to its dialog and shows an error during a
          write rather than looking unavailable; `vendor/gpui-component/GITTURTLE-PATCH.md` still
          names the app's right-click menus as consumers of the popup-inset and context-menu-lifetime
          patches, which they no longer use (update the notes or retire the patches, vendor-scoped).
        - Layout, a frame late: Compare's toolbar (path actions, modes and zoom groups) and the
          footer's long-or-short line still choose from the width a frame before (at 18 pt the
          toolbar flips between 701 and 661 px a frame late); a jump from 461×560 to 980 px moves
          the image comparison 6.5 px between its first and settled frame.
        - Accessibility: at 18 pt in 461×490 the GitHub panel's offline status is reported on the
          bus with 0 height at the panel body's bottom edge (y 557 physical) rather than left out
          while it is scrolled out of view.
        - Tools: `scripts/native_qa/qa.py` cannot drive this host (needs python-xlib and XWayland
          with Mutter or XTest); a Hyprland backend (grim, hyprctl, the wlr virtual pointer, wtype)
          would let scenarios run here.
        - System preview: at 18 pt, side by side at a zoom other than Fit, the pan arrows' row
          pushes Before's control under the clip.
        - Worktree manager: the Create form's branch choices list overflows with no always-visible
          scrollbar (as before at 1480×980 18 pt); the worktree list's floor applies only with rows,
          so its "No worktrees match this filter." line may be squeezed (from the code, untested).
        From the native and design review of `gitturtle-9483bbd` against `-62d10ae` (2026-10-29 late
        night; frames `.local/item8-2026-10-27/tools/qa/u*-{62d10ae,9483bbd}/`, write-ups
        `findings-q9-2026-10-29.md` and `design-q9-2026-10-29.md`, sheets 76–85 in `before-after/`,
        specs `before-after/specs-q9/`; record in [validation](../validation.md)):
        - N1 (bug, a regression from `b9dc438` with `2f7f5e0`; being fixed in brief S): with no file
          watcher, after a paused merge has ended outside the app, Stashes' Review restore… is
          refused from the cached pause; the refusal is announced but its line is gone within 0.3 s,
          and Stashes closes with its selection lost (the base opened Restore stash, rightly). Fix:
          refuse from a cached pause only after the core has read it; if refused, keep Stashes and
          its selection and show the refusal, then the ended line, in the footer note beside Review
          restore….
        - N2 (bug, pre-existing; being fixed in brief R): while a write runs, Ctrl+2 and "Changes ·
          N" stay on History, yet the toggle looks and is announced available (`show_working` in
          `workspace.rs` closes inspections and then returns silently while `operation_busy`). Fix:
          allow the switch, since Changes is a read, or refuse it like the busy controls, with the
          guard placed before `close_inspections`.
        - N3 (bug, pre-existing; being fixed in brief S): at 461 × 490 the Stashes list is never a
          Tab stop, and its second stash is wholly clipped with no scroll cue while Previous and More
          are disabled. The one-row stacked list is DESIGN.md's choice, so "two whole rows" is not
          the fix: give the list a scrollbar, its count in the heading ("Saved work · 2"), and one
          Tab stop with arrows.
        - N4 (polish, pre-existing; being fixed in brief R): the Reflog takes 41 Tabs from a selected
          entry to Copy commit ID. Fix: make the entry list one Tab stop with arrows inside, and
          change DESIGN.md, which makes each entry a Tab stop.
        - N5 (polish): after the last row is removed under its open menu, focus returns to the list
          but the cursor restarts at the top rather than on the removed row's neighbour.
        - N6 (polish): at 18 pt in 461 × 490 both of a changed image's open controls drop beneath the
          names and the image keeps 94 of its 133 px minimum; about 47.5 px (not 55) remain beside
          "Before" for a 39 px icon and its gap. Fix: change the layout (the icons at the halves' top
          corners over the image, or in the toolbar row), which gives back the 29 px row.
        - N7 (bug, pre-existing; being fixed in System preview brief J): narrow model sides stack
          Fit, Reset, the zooms, triangle edges, Standard views and Cameras linked down to y 497 in a
          490 px window, leaving the model 2 px, and the text STL's "Copy source" is cut at the
          side's edge.
        - N8 (polish, was taste): after a one-match Shortcuts search at 18 pt in 461 × 490 the
          General heading's 58 px row (as tall as an entry) has its top cut by the list's edge.
          Fix: make the heading row its text plus the group gap (about 30 px at 18 pt) so heading
          and entry (about 88 px) fit the 99 px list, or else snap the scroll to the entry; do not
          hide the heading.
        - N9 (bug, raised from pre-existing, since Stage or Unstage without review could act on
          another file; being fixed in brief M, working-row menus keyed by path): a right-click
          menu open on a deleted non-last working row stays open over the next row.
        - N10 (polish, was taste; pre-existing): the paused-integration refusal always says "resolve
          conflicts", even for a merge with no unresolved files. Fix: word it by state with the
          banner's verbs ("Complete or abort the merge in progress before starting another recovery
          action"; "Resolve conflicts, then…" only with conflicted entries) in git-core's
          `crates/git-core/src/work/recovery.rs` and the app's `PAUSED_REFUSAL` in
          `crates/app/src/recovery.rs` together.
        - Design review's own findings: the worktree manager's "No worktrees match this filter." is
          drawn in primary text rather than muted and ends with a period unlike "No tags match this
          filter" (use `empty_state::EmptyState` or the muted colour, drop the period; polish,
          pre-existing); DESIGN.md's feedback section says "Added image" and "Deleted image" while its
          image comparison section and the build say "No image on this side", and with the footer's
          "Added" given way nothing says the file was added (pick one wording and update DESIGN.md;
          taste); a dropdown clears its button by 4.5 px and a right-click menu its row by 1.0 px (use
          one gap; taste); the ended line's placement is right, but its wording should name the
          operation and the form's own button: "The merge has ended. Choose Review stash to
          continue." (Review amend in the amend form).
        - Measurement: the slow write's first busy frame came about 250–270 ms after the press, at
          load 17 with frames 30–120 ms apart, which cannot be judged against the 200 ms delay;
          re-measure it on an idle host.
        Owner decision, 2026-10-28: Linux reaches feature parity with macOS where possible; macOS is
        checked later on a Mac. The gaps (README "Linux gaps", `docs/file-previews.md`,
        `docs/linux.md`), each a separate change with a dated research note on the dependency
        (license, maintenance, AArch64 and x86-64 builds; no GPL code), the architecture review, an
        independent security review before integration (decoders and credentials), a release
        measurement and native evidence:
        - PDF pages: page rendering, navigation, linked Before/After pages, Fit and zoom and the
          selected-page text alternative, as macOS has through PDFKit.
        - AVIF, HEIF/HEIC and JPEG 2000 images, which macOS decodes through ImageIO.
        - Pull-request review: store the GitHub account in the Secret Service (gnome-keyring runs
          on Omarchy) so the GitHub panel works with a live account on Linux.
        - The desktop's reduced-motion and contrast preferences read automatically (the XDG
          settings portal), as macOS reads its accessibility preferences.
        - Screen reader (Orca) and input-method coverage, established and recorded.
        - Office documents, audio and video for Open in default app, only through a sandboxed or
          network-restricted handoff that passes a security review (deferred on 2026-10-27).
        Research notes (2026-10-28): [GitHub account in the Secret Service](2026-10-28-linux-github-secret-service.md)
        (oo7, already in the Linux build) and [reduced motion and contrast from the settings
        portal](2026-10-28-linux-portal-motion-contrast.md) (extend the `desktop_text` observer); each
        lists the owner decisions it needs before implementation. Also [PDF pages](2026-10-28-linux-pdf-pages.md)
        (hayro in a limited helper process) and [AVIF, HEIF/HEIC and JPEG
        2000](2026-10-28-linux-avif-heif-jpeg2000.md) (hayro-jpeg2000; mp4parse with the system dav1d;
        HEIC deferred pending the owner's HEVC patent position).
        Owner decision, 2026-10-28 night: take the coordinator's recommendations for the best
        experience, security and engineering, and keep everything legal:
        - GitHub account: one Secret Service item through `oo7` (already in the Linux build, MIT),
          attributes mirroring macOS (`service=com.gitturtle.github.oauth`, `username=github.com`);
          stored reads never prompt; Connect may show the keyring's own unlock prompt; every failure
          fails closed, with no plaintext or file fallback. The interface says "stored in your
          desktop keyring" and never "encrypted"; `docs/linux.md` says a keyring without a password
          is kept unencrypted on disk and points to setting one. Native evidence uses the offline
          fixture; a live sign-in is done by the owner on their own account, and no token appears in
          logs, frames or files.
        - Motion and contrast: read the portal's `reduced-motion` and `contrast` and GNOME's
          `enable-animations`, a11y `reduced-motion` and `high-contrast` through the existing
          `desktop_text` observer; any source asking for reduced motion reduces it. More contrast is
          a derived palette adjustment that is never saved and applies to every palette: secondary
          text, line numbers and syntax at 7:1, dividers and edges at 3:1, a 1 px edge on resting
          controls, disabled labels at 4.5:1, nothing moving; macOS Increase Contrast takes the same
          path (checked later on a Mac). Settings gains "Follow the desktop" (default), On and Off
          for each, since Omarchy has no switch. Proposing that Omarchy's animations toggle set the
          GNOME key belongs to item 7 (optional).
        - Images: JPEG 2000 through `hayro-jpeg2000` (Apache-2.0 OR MIT); AVIF through `mp4parse`
          (MPL-2.0, used unmodified with its notice) and `dav1d` over the system libdav1d
          (BSD-2-Clause), with size, thread and delay limits before decoding. HEIF/HEIC is not
          decoded in the app: HEVC is patent-encumbered, libheif/libde265 are LGPL and Arch links
          them to GPL encoders, glycin brings LGPL GLib and GPL-linked loaders, and the pure-Rust
          decoders are weeks old; HEIC keeps Open in default app, revisited only for a royalty-free,
          permissively licensed path.
        - PDF pages: `hayro` (Apache-2.0 OR MIT) in GitTurtle's own helper process with memory and
          CPU limits, killed on cancel or deadline, after the architecture review accepts the helper
          and an independent security review decides its confinement.
        - Office documents, audio and video stay deferred, last, and only through a sandboxed handoff
          that passes a security review.
        - Legal rule for all of it: every dependency checked with the `research` skill and
          `cargo deny` against `deny.toml` (no GPL, AGPL or LGPL; MPL-2.0 only unmodified), license
          texts in `docs/licenses/` and the package notices, no patent-encumbered codec decoded in
          the app, bundled fonts and icons only under their licenses, and no copied GPL code.
        Owner decision, same evening: committed frames from before the interface-font change
        `ad04939` stand as records of their builds and are not retaken for later changes.
        Backlog, outside this plan (queue with the `queue-intake` skill): on a Mac, check
        VoiceOver's reading of check menu items and of copy announcements from dialogs.
   - **Release set (the owner's cut of 2026-11-15).** The owner decided on 2026-11-15 to wrap up phase 3 and release, building before the release only what meets the release bar: a wrong or unasked write, lost work, an exposed secret, a crash or hang, or a defect on Omarchy's main workflows with no workaround. A new finding enters this set only if it meets the bar; otherwise it joins the post-launch list below. A landed or finished item is removed from the set.
     - **PTR-A** (running): in Changes' Working list a control that arrives under a still pointer takes no pointer press until the pointer moves (decided 2026-11-10; QA-117, `qa/qa117/REPORT-a.md` "Findings outside the checks"). A second click 490 ms after the first, past GPUI's 400 ms double-click limit, hit the Stage all that had moved under the still pointer and staged every file: both handlers pass over a moved control only for a double-click (`crates/app/src/workspace.rs:6552`, `:6850`; `still_over_moved_rows`, `crates/app/src/motion.rs:303`). Committed as `186fb8ce` (not landed); its re-checks found the still state ended on any change of position, so a second click a pixel away still staged every file: PTR-A2 (running) counts the pointer as moved only once it is more than 5 logical pixels from where it came to rest, the platform's own click distance.
     - **PTR-B** (running): in the Diff view a pointer press on Stage hunk acts only on the hunk it began on (FU45-A8's security re-check 2026-11-15, F1, Low, pre-existing; `probes-FU45A8-sec/`). Stage hunk's element id is its row, so GPUI keeps a pending mouse-down across a re-read and the click checks only the row: pointer down on one hunk's Stage hunk, another hunk reverted outside and the quiet read landing, pointer up stages another hunk (`crates/app/src/diff_view.rs:1507`, `:1538-1539`, `:1687`); the reviewer's repair records the diff a pointer press began on and refuses the click once `refresh` ends the press or `land` changes the text. Committed as `5c42b58e` (not landed), the line checkboxes included; its re-checks left two routes, both older than it, for PTR-B2 (running): a press in the one frame between a read landing and the next frame acts on the new diff with the old frame's ids, and a double-click's second press that arrives after the first's write landed stages the hunk that took its place.
     - **Native QA.** Done on the release of `e9d55aaa` (QA-141 and QA-142, 2026-11-15, `qa/qa141/REPORT-a.md`, `qa/qa142/REPORT-a.md`; no finding that meets the bar): the idle-CPU reading (0 to 10 ms of CPU per 30 s on a resting Changes page, focused and unfocused, and on History: nothing redraws continuously, which closes the old "idle capture cadence" item), FU44-D's Settings identity on a marked and on a cold tab, FU45-A7's Stage hunk focus at four sizes and themes and its reasons while a write runs, FU44-F5's re-clone behind Changes, and FU38-A12's failed File History page by keyboard and with Orca. Left: native QA of PTR-A and PTR-B on a build that holds them (a second click in place and 1, 3 and 5 px away; a press held on Stage hunk across a re-read; a double-click on Stage hunk; an assistive Click).
   - **Post-launch list (the open polish queue, sorted against the release bar on 2026-11-15).** These wait until after the release; every entry was sorted against the bar on 2026-11-15 (the sorting tables are kept locally, not versioned). The queue's rules still hold: each landed item is removed; new findings are added here, not to HANDOFF; "verify" marks an item a reading could not settle, so its brief reproduces it first (every brief reproduces first). Order: what can write the wrong thing, lose work or expose a secret; user-visible defects and accessibility; layout consistency; macOS-only; test infrastructure and nits. Size S/M/L. Each item keeps a current `file:line` and the words that find its original, fuller entry in `git show 1e2ad7da:docs/development/omarchy-full-support.md` (lines 745 to 1105). The audit's DONE and STALE items, with commits and counts, are in HANDOFF's "Audit of everything left" sub-bullet.
     - **Write safety and secrets.** Ahead of everything else in this list, because a wrong write or a leaked token costs more than any cut control.
       - (M, X11, and fcitx5 on Wayland) "X11 key repeats unmarked", "X11 unpaired release", "held Enter on a Button": a held Enter on a focused Button presses on each X11 repeat (`vendor/gpui-pre-linux/src/linux/x11/client.rs:611`, `:1120`). One defect. Its reachable writes (a Button and a dialog's default action acting on Enter's key-down) are briefed as FU41-B4 (2026-11-10: they ask `enter_press_is_repeat`); what stays here is the client's own marking for other keys. Ranks after the Omarchy items of the next section, against the rule above, because Omarchy runs Wayland and only XWayland apps reach it: recorded 2026-11-08. FU41-B5's security review (2026-11-10, `probes-FU41B5-sec/`) adds two backend repairs this item owns: mark an X11 key press as held when its paired release was dropped as autorepeat (`vendor/gpui-pre-linux/src/linux/x11/client.rs:611-622`), and have `note_key_up` treat an Enter key-up in a window with no recorded press as released there, so a press in the same batch counts as a repeat (`vendor/gpui-base/src/dialog.rs:335-349`); together they close the last attempt a held Return can start after a press the window never saw (a native picker's confirmation, an input method's commit: probes E2, E3, F1), which the hub's and the theme editor's record-only guards (`theme_editor.rs:1980`) cannot see. The fcitx5 half reaches Omarchy's default Wayland session (FU41-B5C's security re-check, `probes-FU41B5C-sec/`: E2, and F3, the portal folder picker confirmed by a held Return followed by fcitx5's release-and-press pairs, each start one attempt). Owner decision, 2026-11-15: the fcitx5 half is post-launch, with one native check with a real composing engine if one is installed on the host, and comes back only if that check reproduces the write; none is installed (fcitx5 holds `keyboard-us` only, checked 2026-11-15), so the check has not run, and it runs first wherever an engine such as Mozc, Rime or Hangul is set up. Also (FU46-A security re-check 2026-11-11, pre-existing at base): an X11 Space repeat (release, press, release) arriving after a Tab presses the stop focus moved to, through GPUI's Space click on key-up (in probes Delete theme deleted the theme and a Git write was confirmed); X11 repeats only the last key pressed, so a Space held through a Tab should not repeat again: reproduce whether any real sequence produces it before fixing (`probes-FU46A-sec/`). Also (FU44-D9 security re-check 2026-11-14 night, pre-existing, `probes-FU44D9-sec/` P8): a key pressed on one control with focus then moved to another arms the newly focused control on an X11 release-and-press pair (a Return pressed on Settings' Reset, focus moved to Save, saved once at tip and base); no reviewed route moves focus onto a write control while a key is held, and an X11 key held in another app and repeated into the window counts as a fresh press (FU44-D9 code re-check).
       - (S, FU39-A security re-check 2026-11-10) A marked tab still offers what it cannot act on: the preview's LFS Download and text-review bar (`views.rs:3992`, `lfs_download.rs:320`). Every write they reach is refused before Git starts, so this is consistency, not safety: gate `lfs_download_targets` and the text-review bar on `marked_working`. Probes Q3 and Q4 in `.local/item8-2026-10-27/tools/briefs-q9/probes-FU39A5-sec/fu39a9/`.
       - (S, FU44-B security re-check 2026-11-10, pre-existing) Git messages the app reports: the failed read's Details… report is not redacted (Git echoes a config value, so a credential put in `status.showUntrackedFiles` reaches the report), and U+202E and the other bidi controls survive Git's control-character replacement into the banner and tooltips (`probes-FU44B-sec/probe_fu44b_sec.rs` B4). Decided 2026-11-10: the Details… report goes through the banner's credential redaction, and bidi controls (U+202A to U+202E, U+2066 to U+2069) are removed from every Git message the app shows or reports (reason: a report a person may paste into an issue must not carry echoed credentials, and an override can disguise a path or remote).
       - (S, FU45-A's report 2026-11-10 night) A quiet read that changes the shown diff while no partial write is pending replaces it through `automatic_refresh.rs`'s `replace_quiet_preview`, keeping the caret by byte offset, not by line, so an outside edit above the caret moves it to another line; remap it by line as `DiffView::land` does.
       - (S, QA-118 2026-11-10) The Diff view's staging: a "Selected changes staged" banner after each Stage pushes the whole layout down (decided: the result is said in place, through the status line and the live region, never a banner that moves the layout; reason: nothing moves under the person as a write lands); at the text's foot a cut row draws no checkbox yet AT-SPI lists it, with checkboxes for rows wholly below the text, and a press there moves focus to the window frame (decided: the accessibility tree lists only drawn checkboxes, and a press on a cut row reveals it whole as at the top edge); a resize that hides the focused "Review options" at 461 px leaves focus on the window frame (whole focus: the focused control is kept in view); Shift+Down twice from a line's start leaves the caret at column 1 so Space takes 3 lines while the block shows 2 (decided: the lines Space acts on are the lines the block draws); verify: Orca says nothing on caret moves in the diff text, exposed as disabled. Notes: `qa/qa118/REPORT-a.md`.
       - (S, FU41-B7 code re-check 2026-11-10; **FU41-B9**) Optional: `enter_press_began_on` at review.rs ~3042 is redundant, and `FieldReturn` (`tags.rs` ~346) belongs in a dialog-helper module. Notes: `probes-FU41B7-rev/`, `probes-FU41B7-sec/`.
       - (S, QA-117 2026-11-10, Working list; `qa/qa117/REPORT-a.md` "Findings outside the checks") A Stage press that acts although `DESIGN.md:399` says a press on rows still being replaced acts on nothing: a fast double-click on a row's Stage ran `git add delta.txt` twice when the second press arrived before the rows landed (2 of 3 at a 100 ms gap; harmless here, but a write that should not run). Decided 2026-11-10: a press on the Working list's rows or headings acts on nothing while the rows a write replaces have not landed (reason: a press must act on what the person aimed at). Tests: the second press timed before and after the landing. The entry's second half (a second click past the double-click limit pressing the Stage all that had moved under the still pointer) is PTR-A in the release set, whose rule, a control that arrives under a still pointer takes no press until the pointer moves, is expected to close this one too.
       - (S, RED-42902520118 2026-11-10) The preference writer's bounded queue refuses a save outright when a person toggles faster than a slow writer saves (Quality 42902520118's runner: "Could not save settings: Background executor is busy or unavailable", an 81 px notice under the pointer at 18 pt). Decided 2026-11-10: queued preference saves coalesce, the latest value replacing a waiting one, so a burst never refuses a save and the last state is what is written (reason: the person changed a setting, not a queue; only the final value matters). Where a save still fails (the writer stalled past every retry), its notice does not push the controls under the pointer: it is said in place, as the approved "feedback in place" principle asks, not inserted above the page (today 61 px at 13 pt, 103.5 px at 18 pt). Test with a stalled writer and a burst of toggles: no refusal, the last value saved once. Notes: `probes-RED42902520118/` (the round's worktree `agent-a38f1d5c…`).
       - (S, FU45-A8 security re-check 2026-11-15, F2, and PTR-B's re-checks the same day) One meaning of a still pointer on both surfaces: in the Diff view a lone later click in place on the Stage hunk that took the pressed one's place stages it at once, and the button that arrives under a still pointer shows its hover fill and, during a held press, the pressed look, where the Working list lights and presses nothing until the pointer moves more than 5 px. Decided 2026-11-15: the double-click's second press is built before release (PTR-B2, in the release set), and the lone click waits, since refusing a press on a lit button would be a new defect until the hover is suppressed too; repair: the Working list's radius and its unlit look for Stage hunk, Unstage hunk and the line checkboxes. Notes: `probes-FU45A8-sec/`, `probes-PTRB-sec/notes.md`, `probes-PTRB-rev/design-notes.md`, `probes-PTRA-rev/design-notes.md`.
       - (S, FU44-D10 and D11 security and code re-checks 2026-11-15, low, the same at `defc1072`) Settings' identity form after an outside change: with no mark, a Name typed and not saved survives a quiet read or Refresh that changes `user.email` (or both values) on disk, and Save writes the typed Name with the stale Email, since `identity_update` (`crates/app/src/settings.rs:212`) compares the pair, not each field (A4; repair: the identity form follows a read field by field; decided post-launch, reason: the write is the one pressed, on the values drawn, in the same repository, and it needs an outside configuration change during the edit); a Name typed before the mark is saved into a re-clone at the same path on a pressed Save (N3b); a carriage return in `user.name` is stripped by the input, so the read's Name counts as typing through the mark (A9b, `settings.rs:639`). Notes: `probes-FU44D10-sec/`, `probes-FU44D11-sec/notes.md`, `probes-FU44D11-rev/code-notes-fixup.md`.
     - **User-visible defects and accessibility.** The Omarchy claim depends on these; the keyboard and Orca routes come first.
       - (S, QA-135 2026-11-14, outside the checks; `qa/qa135/REPORT-a.md`) Focus stranded by a resize: with the Diff view's `Before` mode toggle focused at 18 pt 1,000 × 680, a resize to 461 × 490 hides it and Tab then does nothing (frame `q135-m18-20cfdfc/midnight-18-461x490-stranded-tab-3.png`). A control a resize hides must hand focus on (DESIGN.md's focus rules: focus never falls to nothing). FU44-F6 (2026-11-14) found the mechanism for a dropped refusal box: the window's focus ring keeps a strong copy of the focused handle, so a focused element no frame draws keeps focus and no key reaches a binding; check that first here, and whether a general repair (an element dropped while focused hands focus to a noted neighbour) belongs in one place.
       - (S, QA-137 2026-11-14 night, outside the checks; `qa/qa137/REPORT-a.md`) Quick source after a Fetch from a terminal kept "3 behind" through a refocus and Ctrl+R while History showed 4 at once: its ahead/behind count is read once and not refreshed with the History snapshot (verify: find which read Quick source's header takes the count from, and give it the snapshot's).
       - (S, QA-137 2026-11-14 night, outside the checks) Stashes: More on the last page leaves focus on More, now disabled, and Orca says nothing about the page that loaded; focus goes to the first new row (as a list's More does elsewhere, verify) and the load is announced politely.
       - (S, QA-137 2026-11-14 night, outside the checks) Ctrl+P during a Fetch does nothing and says nothing (the palette refused while busy?); the typed text and Return then went to History and opened a comparison. A refused shortcut says why (the busy reason in the status line) and the keys typed after it go nowhere a person did not aim them; reproduce first.
       - (S, QA-137 2026-11-14 night, outside the checks, accessibility) A focused parent link scrolled out of view by hand leaves the accessibility tree reporting only the window frame as focused (the link is not built off screen); a focused control keeps its node while focused, as the Diff view's Stage hunk now does (FU45-A7), or focus moves with an announcement.
       - (S, QA-137 2026-11-14 night, outside the checks, nit) During Ctrl+R in Quick source the Blame and Source buttons vanish for one frame (frame records in `qa/qa137/view/`): keep the toolbar's controls drawn while its re-read runs.
       - (S, QA-138 2026-11-14 night, outside the checks; `qa/qa138/REPORT-a.md`) GitHub panel: after a resize the focused reply composer settles at a different scroll position each time, sometimes leaving Review reply… below the body's foot; a resize keeps the focused control where it was, or whole by the least scroll (DESIGN.md's focus rules): reproduce with repeated resizes at 461 × 490 and 13/18 pt.
       - (S, QA-138 2026-11-14 night, outside the checks) Projects: pressing a refused recent row (eight tabs) moves it 52 px from under the pointer at 13 pt, and at 18 pt draws the page shifted for one frame, against the 2026-11-14 press rule (a pointer press scrolls only by the least amount that shows the pressed control whole, here none); reproduce first.
       - (S, QA-138 2026-11-14 night, outside the checks, accessibility) The Projects recent list's rows outside its view report positions as if on screen (AT-SPI extents), and Orca reads "List with 3 items" for the GitHub panel's Files list of 4: verify both against the drawn rows.
       - (S, QA-139 2026-11-14 night, part 16 check 2 FAIL; `qa/qa139/REPORT-a.md`) Blame at 13 pt 1480 × 973: with History of line open, the two Copy line-history commit hash buttons' focus rings have no left side (the stroke stops at the buttons' own left edge: the ring's room clipped by their container; frames `view/b13-ft3-4.png`, `b13-ft3-leftedge-x8.png`); 18 pt passes. Give the footer row the ring's room (`appearance::button_ring_room`) as the other cut rings were fixed.
       - (S, QA-139 2026-11-14 night, part 16 check 6 FAIL) Blame's heading at 18 pt 461 × 490 draws the name cut ("re…r.rs") while Back keeps its label, and through a width drag "renderer.rs" becomes "Blame · r….rs"; as Reload gets its label back the name is cut to "Blame · re…r.rs" at 820 px (18 pt) and 660 px (13 pt); the least form "r….rs" never appears at the narrowest pane. Decide the order against DESIGN.md's one responsive order and the Blame paragraph (the "Blame · " prefix, then the button labels, give way before the file name is cut; frames `view/h18-*.png`, `h13-*.png`), then fix with a width sweep.
       - (S, QA-139 2026-11-14 night, outside the checks, accessibility) A rejected push's banner's accessible name is Git's whole report, so a screen reader hears four progress lines before the error: name it by the error line (the banner's drawn line) and keep the report in Details; the compact Blame footer's accessible name lacks the detail DESIGN.md says it carries.
       - (S, QA-139 2026-11-14 night, outside the checks) At 18 pt: in the push's Details dialog Tab focuses Copy details while it sits out of view (the dialog's reveal); in Inspect rewritten files one frame hides the focused Back to series; History's Blame at 461 × 700 shows no action in its footer.
       - (S, FU44-F7 re-checks 2026-11-14 night, themes) More contrast and a custom theme: the removed tile's ring is checked never weaker with More contrast on only for the fitted Omarchy palettes (`omarchy::palette::tests::more_contrast_never_lowers_the_accent_on_the_removed_tile`); a custom theme is judged by `readability_issues` as saved, not as `Palette::with_more_contrast` moves it, so its accent need not lie beyond the moved tint: run the readability check on the adjusted palette when More contrast is on (or refuse the theme's save with the reason), with a test. Notes: `briefs-q9/probes-FU44F7-rev/`.
       - (S, FU41-B8H/B8I 2026-11-14 night, kit, user-visible) A pointer press on a kit text field's padding focuses the Input's wrapper (its own focus handle, `vendor/gpui-component/src/input/input.rs` ~523-530, ~588) rather than the input, so the field looks focused but typed text never reaches it (the project hub's folder name field at 18 pt: a press at y 111.5-117.5 or 141.5-145.5 focused FocusId 15, not the input's 4); a press anywhere in a field focuses its input. A kit fix with a security re-read (key routes into fields); every app field shares it. Notes: `briefs-q9/probes-FU41B8H/state.txt`, `probes-FU41B8I/`.
       - (S, QA-140 2026-11-14 night, part 16 check 9 FAIL, Orca; `qa/qa140/REPORT-a.md`) Projects at 18 pt 461 × 490, eight tabs: Return on a not-open recent row makes Orca speak the refusal twice, 52 ms apart, once as the announcement and again as the focused row's new name (`logs/orca-r18-12a46b4.log`): one of the two carries it (the row's name keeps the location; the refusal is the announcement), as DESIGN.md's announce-once rule.
       - (S, QA-140 2026-11-14 night, outside the checks) The Diff view's Stage N lines acts on Return's key-up, and focus moves to the text as the write starts and stays there after a failure, where DESIGN.md says a failed write leaves focus on the action or Clear: reproduce with the git shim holding the write and `.git` read-only, and check against FU45-A7/A8's landing focus rules before fixing.
       - (S, QA-140 2026-11-14 night, outside the checks) At 18 pt 461 × 490 Tab to the Projects search field and recent rows leaves them out of view with no ring visible (`view/r18-kf-1-3.png`): the Projects page's reveal; Blame's lineage-note button's expanded state is not exposed through AT-SPI (Orca hears it only in the name); opening the lineage note cuts the revision short and the line-history list draws no scrollbar.
       - (S, QA-135 2026-11-14, outside the checks) The Working preview's partial-staging reason has no accessible node (Orca cannot reach it), and the diff above it draws no row at that size.
       - (S, FU44-D7 design re-check 2026-11-14, pre-existing, kit) A disabled kit text field stays in the Tab order but draws no focus ring (`vendor/gpui-base/src/input/base/state.rs` ~708, ~1109; `vendor/gpui-component/src/input/input.rs` ~519, ~530): while a write runs, Settings' dimmed Name and Email (and any field disabled while busy) are invisible Tab stops. FU44-D8 takes them out of the Tab order on a marked tab only; decide the busy case (a write lasts seconds; moving focus off a field being typed in is worse), likely the ring drawn on a disabled field instead. FU44-D8's design re-check (2026-11-14 night, `briefs-q9/probes-FU44D8-rev/design-notes.md`): out of the Tab order still takes a click, since GPUI focuses a tracked element on mouse-down, so a click on a marked tab's dimmed field probably focuses it with no ring (`settings.rs` ~5411); one fix covers both (the field's focus tracking dropped while it is refused, or the ring drawn), with a click test. Notes: `briefs-q9/probes-FU44D7-rev/design-notes.md`.
       - (S, QA-136 2026-11-14, outside the checks; `qa/qa136/REPORT-a.md`) The status line during a write: a refusal note that expires gives the line back to the previous Fetch's "Up to date with origin · nothing new" while a new Fetch still runs; a cancelled read shows "Cancelling… Inspect the result bef…", cut short and in a write's uncertainty wording; the status-line hints stay the page's while focus is on Pull requests.
       - (S, FU38-A16 design re-check 2026-11-14, low; after FU44-F lands) A theme's Warning text is not checked on `removed_background`, which File History's failed-page notice sits on: `appearance/custom.rs` ~548-588 checks Warning only on the usual surfaces and `subtle` (FU44-F4 adds the accent's rule there). Add a text-contrast rule for Warning on `RemovedBackground`, with the Omarchy fitting keeping a theme that would fail it, as FU44-F4C decides for the accent. Notes: `briefs-q9/probes-FU38A16-rev/design-notes.md`.
       - (S, QA-108, first; FU39-C landed 2026-11-10 at `ebcbfdbf`: the review's Download control, the row's left edge, Split's strip; its follow-ups FU39-C4 to C6 are briefed) Rendered at 18 pt 461 × 490 has overlapping column headings and no room for the sentence (FU39-C's item 3, to brief as FU39-C7); FU39-C's item 6, its own small commits from QA-108's findings outside the checks (FU39-C7).
       - (S, QA-108) Outside the checks: a focused refused row at the foot loses its ring's bottom at the status bar; "Rewritten fcc4e36" is cut at the top in the inspecting mode at 18 pt 461 × 490; Orca reads a refusal twice on Enter (the announcement, then the row's new name); Details is not wrapped at 18 pt 461 × 490; the Blame body scrolls and cuts its Back row at 461 × 440 and below; the first refusal once moved a row by 125 lp at 13 pt 1480 × 973, no cause found.
       - (S, QA-109) Stashes' Ctrl+R and Refresh speak twice under Orca after a stash pushed outside: "4 saved stashes, stash@{2} selected", then the reselected row 12 ms later; one spoken result is the rule (`qa/qa109/readings/orca-s3.log`).
       - (S, QA-109) Outside the checks: at 18 pt a parent revealed by Tab keeps off-screen accessibility bounds for about 1 s while the frame shows it (verify); two pointer clicks on "Commit 1 file" with under 100 ms of hover did not commit where 250 ms did (verify: the input tool or the app). Ruled: a page of one stash's heading "Saved work · stash 41" stands (`recovery.rs:3581`: a bare number reads as a count), and the checklist's "Saved work · 41" was wrong.
       - (S, QA-110, briefed `FU41-B`) The theme editor's colour picker: after Enter or Space chooses a swatch, focus returns to the trigger but it draws its resting border, not the ring, until an unrelated redraw (3 of 3; Escape rings it at once). Outside the checks: a pointer-opened picker never focuses its trigger, so Escape returns to whichever trigger had focus; the popover's hex field, its four HSLA sliders and the popover itself have no accessible names; closing a picker makes Orca re-read the whole editor dialog with every readability warning; the popover is shifted up over its own trigger where there was room above; after Save or Cancel focus lands on "Follow system appearance", scrolled out of view; a shrink leaves the focused trigger out of view; in a light palette the 50 shades are barely set off from the white popover (`qa/qa110/REPORT-a.md`).
       - (S, FU39-A design review 2026-11-10, pre-existing) An accepted recapture clears History's file filter and Changes' filter typed during its history read (`forget_repository_state`, `main.rs:1473`; the Targets reset stays, security requires it). Also say in `navigation-and-refresh.md` that a could-not-check History Refresh keeps the header's branch and counts while History is empty (`shown_tab_unread`), as `c4fc6a2e` leaves the tab as it was.
       - (S, FU39-C4B 2026-11-10, pre-existing) At 18 pt 461 × 380 a failing LFS line at its least and the Download row already reach past the status bar (Split's room starts at y 366.5 against the bar's top at 344): the line and the row need a floor that leaves the comparison its room or gives way further.
       - (S, QA-114) Rendered Markdown at 18 pt 461 wide: the link button's text is cut at the column edge with no ellipsis ("Open li"), and at 461 × 490 the first document's link is neither drawn nor in the Tab order (each side draws only its stacked Read, Top and End controls: `qa/qa114/q114-d18-9bfca07/daylight-18-461x490-c4-rendered.png`; the overlapping headings are FU39-C's item 3); a link focused at 800 tall and no longer drawn after a shrink to 490 leaves focus on the window and Enter does nothing.
       - (S, QA-114) Outside the checks: at 461 × 360 a focused parent's pill and ring are cut at the bottom; at 18 pt a parent just brought in by Tab still reads "not-showing" in AT-SPI about a second later; in `over-corrupt.txt` Split after Down the previous line's "j" descender shows just inside the ring's top; at 13 pt 461 × 490 Split the failing LFS line shows half of its second line, faded and cut.
       - (S, QA-113) Conflict review at 18 pt 461 × 490 (Braden): Restore saved draft on a draft that keeps its markers draws it 48 device px higher for one frame, its top ring and first line cut (`qa/qa113/crops/bd18-b1-s3-s4-s5-body.png`; at 13 pt it rises 29 px in the review's first frame as the saved-draft line goes); typing the last marker away leaves the draft 49 device px higher for good, the caret's line above the body, and typing a marker in pushed it 30 px down for four frames with its bottom ring under the footer (`crops/bd18-b2-unmark-body-states.png`, `bd18-b2-mark-body-states.png`); "unresolved" breaks inside the word at 18 pt.
       - (S, QA-113; confirmed by QA-119 2026-11-11, check 2 FAIL, 3 of 3; **FU45-B**) Orca drops the conflict review's landed status: "Updating conflict blocks…" is spoken once and the landed status and "Recovery draft saved locally" (25 ms and 30 ms later) never, Orca logging "Ignoring object:announcement due to multiple instances in short time" (`qa/qa113/readings/orca-b4.log`); one spoken result needs the statuses spaced or merged. QA-115 (2026-11-10) found the same at 18 pt 461 × 490: the landed status follows 2 to 8 ms after "Updating conflict blocks…" and Orca discards it, and with the status line scrolled out of view typing announces nothing at all (`qa/qa115/readings/orca-b4.log`). QA-119: one keystroke in the draft sends "Updating conflict blocks…", "Saving recovery draft…", the block status and "Recovery draft saved locally · no file or index write" within about 40 ms, and Orca keeps only the first ("Ignoring object:announcement due to multiple instances in short time"). Decided 2026-11-11 (reason: one spoken result per action): a keystroke's intermediate states are not announced; only the landed sentence is, once; the restart label ("Saved draft matches this operation, index stages and working file.") is read on Tab. Evidence: `qa/qa119/readings/orca-c2.log`, `q119-c2-d69beff/`.
       - (S, QA-119 2026-11-11, outside the checks; `qa/qa119/REPORT-a.md`) Conflict review: the draft shows the top of the next code line inside its ring below its last whole line at every size (`crops/c1a-open-s5-draft.png`); Tab in the draft types text rather than moving focus (verify against the keyboard contract's way out of a text editor); with a block selected after Previous, Ctrl+Home does not move the caret, so typing replaces the block (verify). The conflict banner: a pointer sliding from one action icon to the next shows no tooltip within 3 s (from rest each shows by 1.5 s); between about 400 and 394 px its detail line is cut ("side · 3 unresolved…") before the actions wrap (DESIGN.md:269 names only the title's shortening); between 420 and 405 px the title reads "Merge in progress ·…", a separator before the ellipsis. Edit local commits: its sequence draws no scroll cue of its own while its 12 rows overflow; the dialog slides in about 89 device px on open (check against the motion rules). QA harness note: one wheel notch measured 174 device px here against QA-118's 126.
       - (S, QA-120 2026-11-11, check 7 FAIL) GitHub's Send confirmation: one wheel notch over the focused confirmation region scrolls the region's text and the panel's body in the same frame (18 pt 461 × 490 and 13 pt 461 × 491: #42, Reply on the first conversation, a long reply, Review reply…; `qa/qa120/REPORT-a.md`, `crops/c7-wheel1-states.png`, `crops/c7-13-461-wheel-states.png`); a wheel over a scroller that can still move scrolls that scroller only. Keys, Return (sends nothing) and the two-line floor pass.
       - (S, QA-120 2026-11-11, outside the checks) GitHub's reply keeps focus out of view: Tab from the reply text to Review reply… leaves the focused button outside the visible area at 18 pt and 13 pt, 461 wide (`qa/qa120/crops/c1-review-reply-focused-offview.png`); a reply restored from a saved draft opens with its text box focused out of view; Orca reads the confirmation region's name and destination, not the reply text (`readings/orca-c10.log`).
       - (S, QA-121 2026-11-11, outside the checks) GitHub's confirmation at fractional scales and across its row boundary: at XWayland 1.25 and 1.5 one pixel row of the body's text paints over the top line's divider (`qa/qa121/crops/c11-x125-divider-rows.png`, `c11-x15-361-ghost.png`); a resize across part 5 check 6's Buttons'-row boundary draws one wrong first frame (scale 2, 451 → 450: the body 8 px low with Send cut; scale 1, 450 → 452: the old two-row form for one frame).
       - (S, QA-122 2026-11-11, outside the checks; `qa/qa122/REPORT-a.md`) Part 6's neighbourhood: the first 699 → 700 crossing after the project list is turned on makes Orca read the list's selected row ("Open qa122-demo, /tmp/…") while focus stays on the page control (5 of 5 launches); the theme editor's colour picker Palette / HSLA tabs are never reached by Tab; once the held-back note expires Repository's status line reads "Settings saved" instead of the page's own status; under XWayland the window's right part moves about 1 px one or two frames after the 699/700 layout change.
       - (S, QA-123 2026-11-11, part 6 checks 7 and 11 FAIL; `qa/qa123/REPORT-a.md`) Orca: in Settings at 461 × 490 with focus on "Show the project list", Space makes Orca say "Hidden: widen the window to show the list beside the page" twice, once from the status bar's announcement and again after "pressed" from the switch's changed description (every launch; `logs/orca-q123-c7a-0ffd212.log`, `crops/c7-13-switch-before-on-off.png`): one spoken result per press; and a clean repository's Changes list focused by pointer or Tab is read only as "Working tree files, list box": "Working tree clean" is drawn but never said (`crops/c11-clean-list-focused.png`). Outside the checks: "Settings saved" stays in Repository's status line 30 s or more after a Settings write (as QA-122 saw).
       - (S, verify, QA-124 2026-11-11, outside the checks; `qa/qa124/REPORT-a.md`) GitHub: closing the panel with #42 open and nothing typed saves an empty Review draft, and Recover drafts then counts 1; a draft is kept only when it holds text. Also seen: at 13 pt, pressing Refresh moves the body 11 px; a held Button that closes wholly out of view gives its focus to the conversations group on reopen.
       - (S, QA-124 2026-11-11, part 7 checks 3 and 4 FAIL) GitHub panel reopen: (check 3) 13 pt, connection pane open, Copy sign-in command focused, closed, the window shrunk to 461 × 420, reopened by click or key: the first two frames show the body where it closed (field unringed, banner hidden) and the third jumps to the top (`qa/qa124/crops/c3-13-click-reopen-s3-s4-s5.png`, `c3-13-key-reopen-s1-s2-s3.png`); (check 4) 18 pt, #42 open, Reply or Open captured local comparison cut at the body's edge at close: on reopen the body nudges a second time in the second frame (Reply 3 px down, Open captured local comparison 4.5 px up; `crops/c4-18-reply-click-s4-s5-s8-dialog-aligned.png`, `c4-18-opencmp-click-s3-s4-s15-dialog-aligned.png`); Comment on selection and Recover drafts pass. Rule (FU30-D5b/c): the restored body is placed in the first painted frame, one move only.
       - (S, QA-125 2026-11-11, part 7 check 11 FAIL at 18 pt; `qa/qa125/REPORT-a.md`) GitHub Files' list reveal: 18 pt 461 × 490, #42, Files · 4 chosen by pointer, Shift+Tab to Review · 0, then Tab: the list is revealed whole with its ring but scrolled 68.5 px past the least scroll, bringing Previous · 1 · Next fully into view (`qa/qa125/crops/c11-18-before-after-tab.png`); 13 pt passes. Rule (FU28-A3): the reveal scrolls by the least amount and does not scroll the pager in. Also seen: at 13 pt a reopened body sat 34.5 px lower than where it closed when the body was at its end and the line under Refresh went away with a cancelled read; a single git call held past 15 s fails the panel's first read with "Could not read repository remotes and saved drafts."
       - (S, test infrastructure, QA-125 2026-11-11) The offline review fixture has one PR page (Next PR page always disabled), answers every read at once, and has no Refresh conversations / Next conversations route or cut PR row: QA-next part 7 checks 7, 8 (partly), 10 (cut row, held pager) and 11 (Previous/Next) are not producible; give the fixture a second PR page, a second conversations page and an opt-in delay per read.
       - (S, QA-126 2026-11-11, outside the checks; `qa/qa126/REPORT-a.md`, all six part 8 checks 2-7 pass) Rewritten-series review and Edit local commits: (a) at 18 pt 461 × 490, Check publication destination…'s answer "Choose a configured remote name" lands below the body's view, is not revealed and has no accessible node (`qa/qa126/crops/c3-18-check-focused-pending-after-scrolled.png`); (b) one wheel notch up over the failure box at its end scrolls the box and the body 174 device px in the same frame (`crops/c4-18-one-notch-up-chains.png`); (c) Return in Edit local commits' base field closes the form without loading, and the field opens with the caret before its old text, so typing prepends; (d) the palette does not find "Edit local commits", the dialog's own title.
       - (S, QA-127 2026-11-11, part 8 checks 8 and 11 FAIL; `qa/qa127/REPORT-a.md`) Rewritten-series review at 18 pt 461 × 490: (check 8) the modes row wraps in Midnight and Porcelain, Diagrams alone on a second line cut at the body's foot (route: review, Return on the Mermaid row, choose a file; `qa/qa127/crops/c8-18-461-wrap-midnight-dia-notes-switch-porcelain.png`); (check 11) a failed "Open in default app" on After, pressed at the body's foot, is announced by Orca with focus kept, but its line stays below the view with the body at its end, shown only in a tall window (`crops/c11-18-461-read-tab-open-before-after.png`, `c11-18-tall-1000x1050-line-shown.png`). Outside the checks: inspector file rows past the file box's view are not reachable by keyboard and a cut focused row is not revealed; one wheel notch over the series list also scrolls the body; the selected mode is not exposed to accessibility; Refresh series moves focus to the list.
       - (S, verify, QA-126 2026-11-11) Mermaid: a flowchart node named `Link` draws nothing (`flowchart TD / Link --> Regular`): the vendored parser reads a line starting `Link ` as a link directive (`vendor/mermaid-rs-renderer/src/parser.rs:6025`); a vendor fix (security re-check), or an upstream report.
       - (S, FU41-A14 2026-11-11, found by its implementer) While a right-click menu holds focus, Ctrl+Tab and Ctrl+W do nothing: the shortcut's binding never matches from inside that menu (the action dispatched directly reaches the app, and FU41-A14's fix then closes the menu). Two keystroke tests wait in `briefs-q9/probes-FU41A14/remainder.patch`. A window-level shortcut works from inside every menu, as it does from a button menu.
       - (S, verify, FU41-B9 2026-11-11, found by its implementer) A kit dialog whose multi-line field is opened by an Enter press can take that press's X11 repeats as new lines once another key has gone down in between (FU41-B9's N4 for the GitHub review comment, fixed there by an app-side interceptor in `github_view/review.rs` ~3134); a fix in the kit's keystroke interceptor would cover every such dialog (a vendor change: security re-check). Notes: `briefs-q9/probes-FU41B9/report.md`.
       - (S, FU38-A14 2026-11-11, SF2 of FU38-A13's code re-check; **FU47-A**, decided) At fractional scales the window's last device pixel is never painted: left from that entry, native QA at XWayland 1.25 and 1.5. Notes: FU38-A14's `.local/probes-FU38A14/`.
       - (S, QA-115) Outside the checks, conflict review: the status line is announced whenever it scrolls into view, even with no review running; after a pointer Accept current, accessibility focus falls to the window frame, and after Space on Accept incoming the button keeps focus while disabled; at 18 pt 461 × 490 the lower half of Accept current sits under the footer, so a press at its reported centre hits the footer's edge; the conflict sides' binary and absent messages are not in the accessibility tree (`qa/qa115/REPORT-a.md`).
       - (S, QA-113) Outside the checks: after a `chmod 000` root is restored the status line keeps "Read could not complete" and the commit selection is lost; a Refresh that first marks the tab, sent from Changes, lands on History; a pointer tap on the branch menu's "Edit local commits…" fell through to History; Edit local commits' base field starts as `HEAD~1`, so typing after a click appends to it. Not produced: 38B.5's error line and the 12-commit reverse turn, 38B.3 and 38B.5 at 13 pt, 38B.4 at 18 pt (next QA part).
       - (S, QA-112, first) A preview whose missing LFS side becomes local is not read again when the person re-opens the file on the same commit: `data.bin` (a pointer whose object is absent beside a binary blob) keeps "Binary or non-UTF-8 file changed…" after the object is copied into `.git/lfs/objects` and the file is opened again from History with the commit still selected, the status line still reading the first read's time (no new read); selecting another commit and coming back shows the object's text, so History keeps the commit's comparison rather than asking the worker FU40-C changed (`qa/qa112/REPORT-a.md` 40C.1; frames `q112-l13-89484e0/graphite-13-1480x973-f1-present.png`, `-f1b-present.png`). FU40-C's rule (a missing side is never cached) has to reach History's retained comparison too.
       - (S, QA-112) Outside the checks: hovering a project group's actions button shows the row's own tooltip, "Collapse this group", where a click opens the menu (`qa/qa112/crops/m13-c3-hover-button-direct.png`); "All history" lists the latest stash and its index commit, unlabelled, at the top of the graph; at 18 pt 461 × 490 Stashes shows one stash at a time; after New theme… opened by pointer, Escape returns focus to "Follow system appearance" (FU41-B3's item 5 owns the editor's return). The X11 half of part 18's FU37-A check 4 (a held Enter reopening no dropdown) is unverified: under XWayland no `wtype` key reaches the app and the host has no X11 key tool.
       - (S, QA-111) Working Changes at 18 pt 461 × 490: a selected row that lands at the list's foot after its file moves (a terminal `git reset`) has its descenders dimmed up to 21 % by the bottom fade ("Repository root"'s p and y), where the top fade stops 2 px short (`qa/qa111/crops/m18-c3-foot-x3.png`); the fade stops short of the selected row as it does of a focused ring. Outside the checks: with focus in Description, scrolling the list away from the selected row and back makes Orca read the list and move its reading position to that row, and Tab from Unstaged's Stage all to the commit title does the same, since the list reveals its selected row (keyboard focus stays put).
       - (S) Split's floor leaves out the Working mode's partial-staging reason, so with that reason shown Split can keep fewer than three rows (`crates/app/src/views.rs` `split_room`; found by FU39-B's code review; with FU39-C).
       - (S) The Rich fallback never resolves a Git LFS pointer side (`crates/app/src/worker.rs:2846`): beside a blob over 32 MiB a pointer whose object is local shows its text with no LFS caption ("Copy source" where the app's rule is "Copy pointer"), no Open and, with the object missing, no download, where beside a binary under the limit it resolves; route it through the local-store read `captured_model_side` uses. A pointer beside text over the 2 MiB text budget is never resolved either and offers no download (core returns empty sources there). Found by FU40-C and its review.
       - (S, nit, QA-107) A removed LFS object is not noticed while the file stays shown (Ctrl+R and the same row kept Rendered; a cold launch is right), under the rule that a quiet read keeps an unchanged immutable preview: reconsider if the store changing under a shown pointer proves common.
       - (S, verify natively) A reopen may nudge the GitHub body a second time after the frame-0 reveal, since the landing reads the last frame while the dialog slides (found by FU36-B's reviews).
       - (S) A restored tab on a filesystem without stable inode numbers (FAT, exFAT, some FUSE mounts) is marked unavailable at each launch, and an Open adopts it only until the next one (`crates/app/src/worker.rs:1225`, FU34-C): skip saving the inodes on those filesystems if it proves common.
       - (S, nit) A verified Git LFS object that is itself pointer text says "not in the local store", since `Literal::of` judges the resolved text (`crates/app/src/markdown_view.rs:320`; found by FU36-A2's reviews; only a hostile object reaches it).
       - (S) A Clone or Create at eight tabs writes the repository and then refuses the tab, where the limit could refuse first (found by FU35-C's review).
       - (S, nit) `stash_page` re-reads the whole reflog per page, where one core call could return a stash's position (`crates/app/src/recovery.rs:1836`).
       - (M) "Rings covered by a fill" and its every-stop check (`crates/app/src/tab_order_tests.rs:183`).
       - (S, verify) "remote manager's search": its field's and Add remote…'s rings were cut; no pin for them is left in `crates/app/src/tab_order_tests.rs`, so reproduce first.
       - (S) "no RemoveAccessible at close" (`vendor/accesskit_atspi_common/src/adapter.rs:706`). Orca keeps a closed window's nodes.
       - (S) "Working Changes filter shares" one executor with its reads (`crates/app/src/working_selection.rs:773`).
       - (L, verify) "Screen reader (Orca) and input-method coverage": no full record yet, `README.md:128` says not checked. The public claim names it, so the QA parts cover what they can.
       - (S each, verify) Reproduce first: "cold restore" header; "Ctrl+[ then Ctrl+Shift+P"; "explicit Refresh … focus" (`workspace.rs:2735`); "failed draft status wrapping" (`github_view.rs:1943`); "save errors invisible" (`project_pane.rs:169`); "Git actions one-frame lag"; "N2 stale banner"; `settings.rs` nested wheel (`:2017`); "Up to date" and the tracking-branch notice; "the Targets inputs" dim at once; "Clean list 'Working tree clean'"; "Two announcements after resize"; "Orca over profiles menu"; palette first row twice (`command_palette.rs:1260`, fail-safe kept).
       - (S, FU44-B code and design re-checks 2026-11-10; FU44-D's menus and entry points landed as FU44-D2 2026-11-11, its re-check findings in **FU44-D3**; the rest **FU44-E**, briefed) A marked tab: a recapture merging into the tab that shows its repository says so once, politely ("<name> was already open here; its unavailable tab is closed"). The not-read state (FU44-E): the unread look and its 200 ms timer are window-wide (a slow tab's timer dims a quick tab's bar; reset per tab and path); a quiet read's failure never reaches the picker (`automatic_refresh.rs` ~537-553); the picker's failure only in a pointer tooltip (give it as the description); the warm-restore clearing untested; History's Refresh refusing a forgetting recapture while a write runs (Open…'s words), and, decided 2026-11-10, an explicit Refresh failing as `CouldNotCheck` reading "Not read" on History as on Changes while a failing quiet read keeps the state with its banner (reason: one press reads the same from either page; a passive read does not change what the person was acting on). Notes: `probes-FU44B-rev/`, `probes-FU44A-rev/`.
       - (S, FU39-C6 re-checks 2026-11-10; **FU39-C11**, after FU39-C9) A review's download: a download ended by the person's Cancel (decided: "Cancelled download of <file>'s <side> object", normal colour, 1.5 s, said politely) is said on the footer but not yet in the row (FU39-C12's item 1); the footer status repeats its group's name; `DESIGN.md:342` "where the buttons keep their word"; with the queued `follow_download` reading "not ended" as success (it now also draws "Downloaded …"), Compare's re-read after a review's success then close, a hand-over test through `write_from`, the line's scroll on return, room-0 focus, the progress text's contrast, `DESIGN.md:350`'s values, the stale `views.rs` ~3955 comment. Notes: `probes-FU39C6-rev/`. Added 2026-11-10 night (FU39-C9B design re-check F3): where the line holds it, the row's progress keeps "Cancelling… Inspect the result before retrying." whole as a top rung of `progress_room`'s ladder, before "Download object" and "…" (at 1480 × 973 13 pt the press now cuts it with hundreds of px free).
       - (S, test infrastructure, verify) `tags::AN_END` is 0.5 px, a whole device pixel at scale 2, while its justification (GPUI rounding the reach to two decimals, `tags.rs` ~514-518) supports about 0.01: a slack under one device pixel (`0.5 / scale`) would make one device pixel of reach always draw a fade; it changes every cued list at scale ≥ 2, so it is its own round with its own sweep. Notes: `probes-FU38A11C-rev/code-notes.md`.
       - (S, test infrastructure, kit; left from FU41-B8B's re-checks 2026-11-10 night, the rest landed with FU41-B8D to B8I on 2026-11-14 night) The kit slider test reads the drawn node (`step` in `described_slider`'s debug selector, `vendor/gpui-base/src/slider.rs` ~567-587) rather than comparing the step with itself. Notes: `probes-FU41B8B-rev/`.
       - (S, accessibility, pre-existing, verify with Orca) On Linux no assistive technology can move the colour picker's HSLA sliders: AT-SPI's `SetValue` (`vendor/accesskit_atspi_common/src/node.rs` ~1726) has no handler in the slider, which takes only AccessKit Increment/Decrement (`slider.rs` ~533-552), and it takes no focus or key. Also (pre-existing, macOS, FU41-B8B security re-check): the slider's Increment and Decrement actions set its state without emitting a `SliderEvent` (`vendor/gpui-base/src/slider.rs` ~260-269, ~533-552), so VoiceOver's increment moves the slider while the colour, the hex field and the reported value stay, and a later drag picks up the moved state. Notes: `probes-FU41B8B-rev/code-notes.md` N2, `probes-FU41B8B-sec/notes.md`.
       - (S, FU43-A6 design re-check 2026-11-10 night; FU43-A10's item 2; FU43-A9 and FU43-A10 landed without it) The refused answer's side: record the Git action bar's controls (branch picker, counts, Fetch, Pull, Push, Commit, Targets and the compact bar's) and the Linux window controls (`repository_tabs.rs` ~3113-3126, ~3167, ~3301), and where both sides cover something prefer covering controls the running write disables over navigation that still works (decided; reason: a disabled control's label costs nothing during the write); a running write's progress label recorded or `github_view.rs` ~502's doc corrected; ~1072's "above the button where it fits". Notes: `probes-FU43A6-rev/`.
       - (S, FU43-A9 2026-11-11; FU43-A10 landed 2026-11-14 with (c)'s tooltip on another page fixed, without (a) and (b)) Pull requests and the header: (a) the queue's FU43-A9 entry's item 3 (the action bar's controls and the Linux window controls in the refused answer's side choice; `workspace.rs` ~6873, `repository_tabs.rs` or `window_chrome.rs`); (b) decided: the full header's labelled History and Changes carry the same tooltips as their compact glyphs (`views.rs` ~826-864; DESIGN.md:137); Notes: HANDOFF's FU43-A9 sub-bullet.
       - (S, verify natively, FU43-A7's report 2026-11-10 night) History's and Changes' header buttons never show a tooltip in the GPUI test harness, at 1280 × 800 or 461 × 490, idle or busy, where Settings and Pull requests do; the neighbour tests hover Settings instead. Check natively that both show their tooltips after the 500 ms delay (and wait the full delay after the Pull requests answer); a product bug if they do not.
       - (S, FU44-C4 code re-check 2026-11-10 night; FU44-F landed 2026-11-11 with the refused write's recapture; **FU44-F3**) Left from that entry, verify: Show latest's "selected inspector retained" set on a fresh capture (`history_updates.rs` ~477, pre-existing; FU44-F2's N1-N3 may have covered it). Notes: `probes-FU44C4-rev/code-notes.md`.
       - (S, FU41-A10 code re-check 2026-11-10, pre-existing) With Settings shown and nothing saved to return to, `inspection_opener` falls through to the focused Settings control.
       - (S, QA-116 2026-11-10, checks 13 and 14; `qa/qa116/REPORT-a.md`) The Working list's focus on a control the wheel took away: AT-SPI focus falls to the window frame ("0 frame | GitTurtle | focused") until a key brings a control back (W and S); and on the wheel back, at S, the edge goes at a notch where only 11 of the row's 122 device px are drawn and its Stage lies wholly above the list's top, so one frame shows neither edge nor ring (AT-SPI "Stage file-01.txt | focused,not-showing"). Decided 2026-11-10 (the ruling check 14 asked for): the edge stays until the focused control itself is drawn whole, a partly drawn row does not count (reason: whole focus, exactly one focus indicator in every frame); AT-SPI focus stays on the control while it is wheeled away, its node kept in the tree marked not showing (reason: a wheel is no focus move, so the screen reader says nothing and the keyboard acts from the control's place).
       - (S, QA-116 2026-11-10) The Working list's accessible list gives only its drawn rows as its size ("Working tree files List with N items", N 1 to 6 of 58 files), and at N repeats its name on every move between headings. Rule: rows carry their position and the list's whole size, so Orca says the whole count once.
       - (S, QA-116 2026-11-10, reproduce first) A held Enter on a Working row's Stage acts on the key's release under native Wayland, with no pressed look while held (under XWayland it landed during the hold); `DESIGN.md:381` says a Button is pressed once, as the key goes down.
       - (M, FU41-B6 2026-11-10, FU41-B's item 4) Closing a colour picker makes Orca re-read the whole theme editor, every readability warning included (`qa/qa110/readings/orca-c3.log` 21:07:56): GPUI lays deferred draws (every popover) out with the accessibility parent reset to the window root, so the popover's node is the dialog's sibling, and focus moving back into the dialog re-announces it. Decided 2026-11-10: fix it where it is, in GPUI (a deferred draw remembers the accessibility node it was deferred under and adds its nodes there), by vendoring `gpui-pre` 0.3.4 under the vendor guide's provenance rules with its own security review, or upstream; a kit-only workaround cannot move the node (reason: it reaches every popover, not only the picker). FU41-B6's notes: `probes-FU41B6/`.
       - (S, FU41-B6 re-checks 2026-11-10; **FU41-B8**) The theme editor: FU41-B's item 6 (the popover covering its own trigger; the light palettes' faint swatch borders). Notes: `probes-FU41B6-rev/`.
       - (S, FU41-B8 design re-check 2026-11-10, F3) The ring after a pointer close app-wide: kit Buttons ring on any focus (`DESIGN.md:290`), the theme card's on a key only (`:191`-`:193`), Columns rings after a pointer close (`:237`); decide one rule in `:290` (lean: the shared focus ring's, a key shows it and a pointer press ends it) or record why Columns differs. Notes: `probes-FU41B8-rev/`.
       - (S, FU41-B7 code re-check 2026-11-10, pre-existing) The GitHub panel's file list acts twice per held Return (`github_view/review.rs` ~3034-3039, ~3176: the press selects the first file and the first X11 repeat moves focus into the patch); navigation only. Settings' Retry save keys on the error text (`settings.rs` ~2982, `theme_editor.rs` ~9652), so it vanishes when another error replaces the line while the failed value is still held. Notes: `probes-FU41B7-rev/`.
       - (S, verify, FU44-C2 code re-check 2026-11-10, pre-existing) Refresh, then Changes before the snapshot lands, cancels it after Open cleared History's rows, so History may stay empty until another read. Notes: `probes-FU44C2-rev/`.
       - (S, verify, FU38-A11 code re-check 2026-11-10, pre-existing) The kit scrollbar's press and drag test against the unmasked track (`vendor/gpui-base/src/scrollbar.rs` ~1540, ~1594), so a bar partly outside a parent's clip could take a press aimed elsewhere. Notes: `probes-FU38A11-rev/`.
       - (S, verify, FU41-A11 design re-check 2026-11-10) Ordinary Compare's mode changes move no focus, so focus in the patch editor may be left on a view no longer drawn; Rendered's pane group and the rendered view's own stop are two Tab stops for one region. Notes: `probes-FU41A11-rev/`.
       - (S, QA-117 2026-11-10) The Working list: at 18 pt the double-click's second press, which the guard ignores, still draws the pressed fill on the control under it for three frames and keeps its tooltip away until the pointer leaves (a refused press shows no pressed look); a Stage write centred a selected row only cut at the list's top though the write did not move it (keep the least scroll); after a keyboard Stage Orca reads the landed row twice, and after the row menu's Stage at 461 px it first reads the row's old "Unstaged" state (one spoken result); a filter query over 4,096 bytes shows its error label unannounced (say it once, politely); `DESIGN.md`'s keyboard table still says Return in the file list opens the file in Compare, against `:399` ("Enter again writes nothing"): reconcile.
       - (S, FU41-B9's fixups re-checks 2026-11-11 night; **FU41-B10**, FU41-B9's last fix pass, briefed) Verify on a Mac: Command-Return repeats continuing mid-hold may now type newlines. Notes: `probes-FU41B9-rev/code-notes-2.md`, `probes-FU41B9-sec2/`.
       - (S, left from FU44-F with F2 re-checks 2026-11-11 night; the banner's box, Tab stop and closing sentence landed with FU44-F3 to F7 on 2026-11-14 night) The refusal hides the scope notice until dismissed (confirm natively); code nits: `read_all_history_in_place` does nothing while a write runs with the banner skipped (no route found), and `a_quiet_read_that_finds_the_scope_gone_keeps_the_history_it_shows` passes without its fix. Notes: `probes-FU44F2-rev/`.
       - (S, QA-128 2026-11-11 night, outside the checks; `qa/qa128/REPORT-a.md`, all six part 9 checks 3-8 pass) At 18 pt 461 × 490: (a) Split rendered Markdown draws "Before · Markdown" over "After · Markdown" and cuts the After heading at the right edge ("Markdo"; `crops/c94-*`); (b) Tab reaches both "Independent scroll" toggles below the panel's foot with no ring drawn (the known failure pinned at `tab_order_tests.rs` ~666-672, still present); (c) Blame's rows show line number, hash and author with no source text: the 44 px number and 180 px attribution cells (`blame.rs` ~1076, ~1085) cannot shrink, against DESIGN.md:255 "Blame always shows its source lines" (`crops/c95-n2-18-461x490-blame-three-rows.png`); (d) unverified: in the local document at 461 × 491 AT-SPI places the focused "Open link the guide" below the panel's foot.
       - (S, nit, FU38-A13 landing 2026-11-11 night; **FU38-A12**) `file_history.rs` ~328 (`Rows::regions`) still says "the header's rows stay whole before their labels go", which the exception made false (DESIGN.md and the crate doc now say "the labels never cost the header a row"); FU38-A15's design lows: the grid test at scales 1.25 and 1.75 too, the 480-wide sweep's skipped band; the simplification `self.labelled == Some(self.icons)`. Notes: `probes-FU38A15-rev/`.
       - (S, QA-129 2026-11-11 night, part 10 checks 1, 3 and 5 FAIL; `qa/qa129/REPORT-a.md`) Working Changes: (check 1) at 18 pt 461 × 980 the composer heading reads "C…" beside the "2 staged" badge, where DESIGN.md:265 says the icon, then the badge, leave first and names this window as one where only the branch truncates (`crops/c101-n2-18-461x980-heading-C-ellipsis.png`); (check 3) with the list focused and the body wheeled down to Commit, a watcher refresh that adds a row keeps the body where it was (Commit under the pointer, the list out of view, twice), where DESIGN.md:265 says the body returns to its top (`q129-p3-5c3c298/midnight-18-461x490-p3b-*.png`); (check 5) at 18 pt 430 × 490 the row draws "README…" with the glyph and folder already gone (`crops/c105-*`). Outside the checks: the commit description shows the top pixels of its fourth line at its foot (13 pt 640 × 680); after keyboard staging the list's top and bottom rows are cut through their text under the edge fade (18 pt 461 × 490); a focused list wheeled past reports `focused,not-showing`.
       - (S, QA-129 2026-11-11 night, part 9 check 9) With More contrast on, Porcelain's borders darken to #868B96 while the 1 px ring stays #3455A6: 2.05:1 against the border (Midnight 3.48:1; with More contrast off 4.67:1); DESIGN.md sets no ratio for 1 px rings; decide one (3:1 as Button and field rings have) and darken the ring with the borders.
       - (S, QA-130 2026-11-11 night, part 10 check 10 FAIL; `qa/qa130/REPORT-a.md`) Orca never says "scroll pane" for the Working body: AT-SPI exposes it as an unnamed "panel" and no node in the window has the scroll-pane role, against `08d0234f`'s "the one scroll view assistive technology reads" (nothing nested, so the "not two" half holds; `logs/orca-p10.log`). Outside the checks: one wheel notch over the Working list scrolls the list's rows and the whole body at once (174 + 174 device px), the list's rows staying offset after scrolling back (`crops/c106-*`); at 461 × 490 the path filter's error message is neither drawn nor in AT-SPI, the list empty with no reason (`crops/c107-*`); at 18 pt 461 × 835 the description shows the top half of a second line; the palette's placeholder at 18 pt 461 × 490 is cut mid-word (its last word cut after its fourth letter); after a failing background read the list keeps its old rows and the status bar says "10 working changes" beside "Changes unavailable"; Orca reads the Working list as "List with 1 item" for ten files (only drawn rows in the tree); a row's tooltip appears after a wheel notch moved the row away. Not producible: changing the text size while Keyboard Shortcuts help is open (it is modal and no key changes the size).
       - (S, FU41-A15 2026-11-11 night, found by its implementer) With a right-click menu open, Ctrl+Shift+P opens the palette but focus stays in the right-click menu, which takes focus back every frame it is drawn (`native_accessibility.rs` ~2292): typing reaches nothing, so no palette command (Move included) works from a right-click menu; also the Main Menu's own focus-out close (`platform_polish.rs` `toggle`) did not fire when a new tab moved focus away (covered now by the tab observer; check it elsewhere).
       - (S, test infrastructure, FU47-A 2026-11-11 night) `a_keyboard_menu_on_the_lowest_row_moves_nothing_at_any_scale` cannot take a third scale pass: by then History is scrolled 92 px and its "lowest row above the status bar" is a row cut 11 px by the list's own foot, which Shift+F10 rightly scrolls into view; pick the row against the list's bottom, then add 1.25.
       - (S, QA-131 2026-11-12, part 11 check 5 FAIL; `qa/qa131/REPORT-a.md`) Compare's Wipe with an image note at 13 pt 1×: while the GIF plays, the stage and both notes jump vertically by up to 8 device px frame to frame (still when paused; the notes' text and breaks unchanged), the movement starting above the stage (route: `qa131-media`, History, `anim.gif`, Compare, Wipe, Play GIF, columns 240 and 276; `q131-x1-e4a602f/play-offsets.txt`, `crops/c115-*`). Outside the checks: "Files · 4" by pointer focuses the changed-file list without revealing it, its first row cut at the body's foot; a file row's tooltip stays after the body scrolled from under the pointer; Quick Open's help writes "↑/↓" where the palette writes "↑ / ↓"; in a 680 px tall window the Wipe notes switch between one and two lines as the window narrows, since the GIF controls above them change height; the Overview's "Checks · …" line is a plain joined label, not laid out by phrase. Not producible here: a text-size change while the modal GitHub panel is drawn; a fractional-width share at 1×; the note's ellipsis by width (the comparison stops narrowing at 240 px).
       - (S, QA-132 2026-11-12, part 11 check 9 FAIL; `qa/qa132/REPORT-a.md`) Orca reads the GitHub panel's "Conversations · 3 unresolved on this page" and "Page 1 · 6 total conversations" twice in a row whenever focus lands on the conversations panel (launch `q132-o9`, PR #42, Return; `logs/orca-q132-o9-e50f965.log`); Quick Open's hint is never read: Orca reads the busy "Searching local tracked paths…" as the dialog opens and never the finished line (frame `q132-o9-e50f965/midnight-13-1480x973-o9-qo2.png`).
       - (S, QA-132 2026-11-12, outside the checks) The rewritten-series review at 18 pt 1480 × 973: the fourth file (`switch.mmd`) is neither drawn nor reachable by Tab or Down until a wheel notch reveals it.
       - (S, QA-132 2026-11-12, outside the checks) Orca: when the GitHub panel reopens on Files, it announces the file list 58 ms after the focused Threads toggle while focus stays on Threads; Amend last commit gives focus to the dialog itself and its branch line has no accessible node.
       - (S, QA-133 2026-11-14, part 12 check 4 FAIL; `qa/qa133/REPORT-a.md`) GitHub panel: pressing Refresh with the Review confirmation focused leaves focus on the bare window and Tab does nothing (5 of 5; frames `q133-g13-c0f7e9a/midnight-13-1480x935-stranded*.png`, `q133-g18-c0f7e9a/midnight-18-461x490-stranded.png`); in the one busy frame at 13 pt 1480 × 905 Send and Keep editing are cut at the body's foot (`crops/p124-13-1480x905-busy-frame-foot.png`); the check's first step cannot be produced with the offline fixture (Refresh answers at once and closes the confirmation).
       - (S, QA-134 2026-11-14, part 14 check 6 FAIL; `qa/qa134/REPORT-a.md`) Stashes: a Git LFS pointer whose object is missing shows only its pointer patch, with neither the shared sentence nor Download (`qa134-rw`, palette "stash", Browse stashes…, Down, `lfs-stash-miss.txt`; `crops/p146-18-1480-stash-missing-no-words.png`), where the series preview and Compare say why and offer Download.
       - (S, verify, QA-134 2026-11-14 outside the checks) History lists a stash's own commits above `main` in the disposable `qa134-rw` repository: reproduce whether a passive History read walks `refs/stash` (it should not show them as branch history); Diagrams' Tab order goes across the sides by control (both Open, then both Copy source) rather than down each side; "Copied" not seen about 1 s after Copy pointer (part 14 check 11(h) covers it).
       - (S, FU44-D10 security and design re-checks 2026-11-15, low) Held keys at a marked tab's way back: after a focus loss mid-hold a hold begun on the list acts at the way back (two reads, or two choosers on the rail; a regression on Refresh, no write; K5b); a refused press with a lost key-up swallows the next press once (K2); a held press that already reached the way back stays noted after the window loses focus, so the next press of that key is swallowed (design finding B). Notes: `probes-FU44D10-sec/notes.md`, `probes-FU44D10-rev/design-notes.md`.
       - (S, FU44-D11 re-checks and report 2026-11-15) Settings with no identity loaded: where the working status was read and only the identity's own read failed, Save and Reset say "Working tree not read · Refresh" though Fetch stays enabled (the code can tell the two apart, `work_status` set and `profile` none, and "Git identity not read · Refresh" would be the truthful reason); Return in a field with nothing loaded saves nothing, silently; a quiet read held only by the Settings page says "Reading the working tree…" until the person leaves Settings; the reason sits in the tooltip and accessible name, as the mark's does. Notes: `probes-FU44D11-rev/design-notes.md`, `probes-FU44D11-rev/code-notes-fixup.md`.
       - (S, FU45-A8 code and design re-checks 2026-11-15) Stage hunk: a kit dialog opened while Stage hunk holds focus gives focus back to the undrawn handle after a quiet read ended the press (no write; a click recovers); the review's own option buttons give no reason; a possible 2 px ring sliver at the text's edge (pre-existing); the pressed Stage hunk's ring is whole at 13 to 18 pt by reading, with scales 1.25 and 1.5 unswept. Notes: `probes-FU45A8-rev/code-notes.md`, `probes-FU45A8-rev/design-notes.md`.
       - (S, FU44-F5 code re-check 2026-11-15) After a re-clone, routes other than the ones FU44-F5 closed can leave History empty for the found repository; Refresh, or Changes and back, recovers every one. Also the Busy drop on the UI thread, decided post-launch with them. Notes: `probes-FU44F5C-rev/code-notes.md`.
       - (S, QA-141 2026-11-15; `qa/qa141/REPORT-a.md`, "Failures" and "What I saw outside the checks") Seen natively on the release of `e9d55aaa`, none a wrong write: Clear, dimmed while a partial write runs, shows no tooltip, though `cb4b167d` names it among the controls that give "Wait for the current Git operation to finish"; a tooltip does not follow the state under a resting pointer in Settings (Save repository identity kept "Reading the working tree…" for 1.4 to 23 s after the read landed and a save completed) nor on the Diff bar's action once a landing removed the bar; after an identity read ran into its 15 s limit the banner uses a write's words for a passive read ("Its result may have applied partially or remotely") and Settings stays on "Git identity has not been loaded yet." until it is left and opened again; on the first show of a tab not read since launch Push is drawn filled and the header reads "GitTurtle" for about 85 ms; on a marked tab a click gives the disabled Name field keyboard focus (nothing is typed or written); each Save repository identity leaves its earlier comment and two empty `[user]` headers in `.git/config` (Git reads them as one); a Stage hunk landing clears a line selection made in another hunk; the banner's leaving as a write starts and its arrival as it lands move the Working diff by 41 px at 13 pt under a resting pointer; a failed write's banner reads ".: File exists (os error 17)" where the cause is appended; at 18 pt 461 × 490 a Stage hunk cut by the viewport's top edge ignores a click until one wheel notch shows it whole.
       - (S, QA-142 2026-11-15; `qa/qa142/REPORT-a.md`) Seen natively on the release of `e9d55aaa`, nothing written in any of them: with a right-click menu open on a History row, Ctrl+Shift+P opens the palette under the menu, which keeps the keys; Escape closes only the menu, and Down, Tab, Escape and Ctrl+Shift+P then act on the page behind the open palette until a click in the palette (the Shift+F10 menu behaves; the app's CPU stays at rest, so there is no redraw loop); a failed File History page's scrolling message draws no scrollbar at 18 pt 461 × 420, though at 1,480 × 973 its thumb is drawn inside the ring; at that size a Return on Older page that fails leaves no focused node; a working file made unreadable keeps its stale diff through two quiet reads (70 s), and a Return on Stage hunk then stages nothing, shows "Preview unavailable" and gives focus to the file list.
     - **Layout consistency.** Visible and frequent, but no control is lost.
       - (S, QA-135 2026-11-14, part 14 check 11(c) FAIL) Oversize beside missing, Unified, 18 pt 461 × 490: the LFS line's text starts at x 108 and the Download button's icon at x 131, 31 px further in: the line and the row do not share one left edge (frames `q135-m13-20cfdfc/midnight-13-*-c-*.png`).
       - (S, QA-135 2026-11-14, part 14 check 11(g) FAIL at 461 × 490) A damaged image side's sentence is cut to "The local…" (`q135-mg-20cfdfc/midnight-18-461x490-g-png-461.png`); Rendered shows no content row and its side headers overlap (QA-133's entry, met again). Also: the damaged-side sentences on Rendered, image and model sides use the muted text colour, not the warning colour the check expects; Split draws no file footer.
       - (S, FU38-A16 code re-check 2026-11-14, nit) The LFS line's scroll cue is built with zero room inside its ringed region, so its track and fades run under the ring, as File History's message did before FU38-A17 (`lfs_download.rs` ~233; the tags list passes the room, `tags.rs` ~1557/1582). Notes: `briefs-q9/probes-FU38A16-rev/code-notes.md`.
       - (S, QA-132 2026-11-12, outside the checks) At 18 pt the series review's diagram pager splits, Previous ending one line and Next starting the next; the dialogs' hints are not one size (Quick Open's line 39 device px against 36 for the palette and Keyboard Shortcuts).
       - (S, QA-133 2026-11-14) Rendered Markdown's side headers overlap at 18 pt 461 × 490 ("Before · Markdown" runs over the After header; `qa/qa133/crops/p141-18-461x489-rendered-side-headers.png`); in Split the oversize line shows no scrollbar thumb; the comparison-mode toggles expose no pressed state to AT-SPI; the warning line's focus ring is 2 device px.
       - (S, QA-134 2026-11-14) At 18 pt Diagrams' side headers split the Previous/Next arrows across two lines in Compare and in the series preview (with QA-132's series pager split).
       - (S, nit) The local document's title at 18 pt in a half tile: "Captured local document · " keeps about 270 of 378 px and the path keeps 108 px, so `location_tail` elides even a short file name; where the last path component would be cut, drop the words and show the path alone (`crates/app/src/markdown_view.rs:1393`; found by FU37-D's re-check).
       - (S) Rendered Markdown's two headers wrap on their own, so near a wrap point the sides' first blocks start at different heights, and the header inset is 8 px against the blocks' 12 px (`crates/app/src/markdown_view.rs:1575`, `:1580`, `:1145`; found by FU36-A's design review).
       - (M) "Markdown's side headings overrun" (`crates/app/src/markdown_view.rs:1569`).
       - (S) "`ScrollCue` late thumb" and "Lists whose bar" have no fade (one fix, `crates/app/src/tags.rs:762`, `:456`).
       - (S) "From …" width, "`PanelLeast` reserves" in File history (`crates/app/src/file_history.rs:1170`, `:90`).
       - (S) "Refresh series" shrinks the list; "130 px file box" (`crates/app/src/rewrite_review.rs:614`, `:1666`).
       - (S) "`files_fit` reads last frame's top line"; title-and-mode-row height one frame (`crates/app/src/github_view/review.rs:3908`, `:3830`).
       - (S) "180–360 px range" (`crates/app/src/views.rs:8602`); "worktree manager's fixed 11 px" (`crates/app/src/worktrees.rs:38`); "Found by the layout grid", conversation cards (`crates/app/src/layout_grid.rs:424`).
       - (S each, verify) "PR row's held part is `None`"; "Description label" overrun; "scale 1.2 … padding"; "Rewritten-series" moves twice; "Rings under fills"; multi-line Input extra row (`vendor/gpui-base/src/input/base/element.rs:1186`); "History's Changes body is measured a frame late"; "rebase (720 px) and Quick Open (700 px)" past the left edge; "no scroll affordance"; `image_compare.rs:999` snapped width.
       - (verify) Older phase-3 findings the audit could only reach natively (18 pt and 461 px cuts, GitHub panel insets, header offsets, contrast samples, the "still owed" frame lists): the QA parts confirm or retire them.
       - (S, QA-116 2026-11-10) The Working list's focus edge: a 1-device-px neutral line sits just outside the accent on the edge's bottom and left at scale 2, and the selected row's 2 px marker merges with the edge's left side into one accent band. Decided 2026-11-10: the edge is drawn on the pane's outer line's own pixels, replacing it, with no neutral line beside it (reason: the rule is one 1 px accent on the pane's outer line); the selected row's marker keeps at least 1 lp clear of the edge (reason: two indicators that mean different things must read as two).
     - **macOS-only (a hosted run or a Mac).** Omarchy does not depend on these. All verify: Menlo's metrics (`main.rs:3018`); VoiceOver palette status (`command_palette.rs:860`), check items and copy announcements; the refused menu-bar item (`platform_polish.rs:437`); the AVIF caption (`worker.rs:2939`); Enter-then-Command and Command pressed mid-hold.
     - **Performance.** (M, verify) "hayro decode"; "grained-alpha peak"; the slow write's first busy frame. (M) "tags after history, one CPU" (`crates/app/src/worker.rs`). (S) "per-row clip scroll", verify; "`FocusReveal`: record path" (`crates/app/src/focus_reveal.rs:393`); "`confirming_with` clones `id`" (`crates/app/src/copy_feedback.rs:521`); screen-reader re-segmenting, M (`vendor/gpui-base/src/input/base/state.rs:591`).
     - **Test infrastructure and nits.**
       - (M) "real-time waits remain" (`crates/app/src/workspace/confirmation_tests.rs:56`, `crates/app/src/views.rs:36495`, `crates/app/src/theme_editor.rs:4964`).
       - (S, briefed `FU30-C4`) "theme editor sweep" split; a walk returning to a middle stop (`crates/app/src/focus_ring/sweep.rs:661`, `:274`).
       - (S) "list stop's ring"; "sweep of History and Working Changes"; "frame-0 test"; `cx.notify()` on a side press; "before any frame check"; "hold_quiet_reads direct"; `FIT_SLACK` (`crates/app/src/focus_ring/sweep.rs:96`, `:578`; `github_view.rs:10943`; `markdown_view.rs:1632`; `motion.rs:8989`; `editor_find.rs:1490`).
       - (S, nit) `repository_tabs::tests::quiet_reads_landed` drains only the reader's thread each round, so a quiet read waiting behind a status read on `operations` would end its 16 rounds early and panic (never hang); drain `operations` each round too (found by FU40-B's review).
       - (S, verify) `files_settles_in_a_frame_after_a_resize` hanging; "H4 (2)"; "pre-walk clone"; "History rows 21–520"; "Rerun QA-106e D6"; the older test-infrastructure list (FU20-GH4, FU17-E, FU18-D, FU12-D, M4, FU11-B, FU13-B/C, GIF, hue, FU14-B, AVIF fixture, benchmark records); "toolkit's own ghost controls" evidence (`docs/validation.md:838`).
       - (S, nits) "`views.rs` comments" (with the panel fix above); "Review at most 100" and shared timeout wording (`crates/git-core/src/work/rewrite_review.rs:183`, `work.rs:1977`); "old alias tab's mark"; "copied 0.45"; "Profiles: older failure's text" and profile header above failure (`crates/app/src/profiles.rs:339`, `:592`); `content_room` constants; "a shared fades-only helper" (`crates/app/src/tags.rs:648`); `github-collaboration.md:61` key-name rule; "no checked state" (`crates/app/src/platform_polish.rs:517`); single-line held Enter (`vendor/gpui-base/src/input/base/state.rs:1745`); `settings-themes.png` retaken (`README.md:39`); C2's crossed word (`crates/app/src/appearance.rs:3794`); a selector at `conversations.rs:669`, verify; FU20 (C) text cuts, L, deferred upstream; FU20 (D) taste, owner's call.
       - (S, test infrastructure, FU44-F5 code re-checks 2026-11-15) The clear on a status read's own error reply is untested (a mutation survives; worst case one extra re-read), and dropping only the clear on landing (`crates/app/src/history_updates.rs:522`) may go unnoticed (an extra read). Notes: `probes-FU44F5C-rev/code-notes.md`.
     - **Tooling and CI.** `qa.py` cannot launch here (python-xlib, `scripts/native_qa/x11.py:53`) and has no Hyprland backend, horizontal scroll or frame recorder (M); ΔEOK analysis; the `;` keysym (verify); per-channel tolerance (`analysis.py:32`); `runenv.py:323`; icu/json5 (`Cargo.lock:3726`, `:4058`); rust-cache (`.github/actions/setup-rust/action.yml:94`); LFS 3.8.1 on the runners and whether to warn before a clone needs LFS (decide with phase 4); the driver gaps, verify. `cargo-nextest`'s download failed with HTTP 504 on three of the runs of 2026-11-09 (Ubuntu, macOS, Arch), each needing a rerun of the failed jobs: fetch it from a cache or a second mirror (S).
   - **Audit status of phases 1 to 3 (2026-11-08, at `1e2ad7da`).** Phase 1 is done: first launch, its defaults and its documentation hold as written (`docs/linux.md`, `README.md:82`). Phase 2 is done: list Copy, the selectable message and the opt-out rule hold, and its sentence "only Blame handles it" is out of date (`crates/app/src/working_selection.rs:558`, `crates/app/src/file_history.rs:1726`, `crates/app/src/commit_message/selection.rs:512`). Phase 3's parts 3.3 to 3.7 are done (the `omarchy font set` route, IBM Plex, the theme fixes, the editor and terminal handoffs, the README section); 3.8's "awaiting native evidence" goes to the QA parts. Its older findings hold about 140 done, 3 stale (the entry that System preview is hidden on Linux, which now offers a passive open; the `local-workspaces-error` alert, now `refusal_line`; the commit menu's edge, measured later), 8 still open (moved into the queue above) and about 80 that only a native run can settle: the QA parts confirm or retire those, and about 65 entries were not judged (mostly pixel measurements recorded for past rounds). The plan's parity gaps stay open as written in `README.md:128`: office, audio and video previews (deferred), full Orca and input-method coverage (partly done). **Phases 4 to 7 need what only the owner can give, to be raised when reached and not before:** the license review of `mac` and `ufbx` (phase 4), an x86-64 Omarchy machine (phase 5), Developer ID signing and gitturtle.com (phase 4), and the live GitHub account check.
4. **Distribution.** Publish versioned release bundles (x86-64 and AArch64, checksums,
   build identity). Resolve the two dependencies the packager flags for license review
   (`mac`, `ufbx`) before distributing. Owner decision, 2026-11-15: `ufbx` 0.11.5 is
   distributed under its Unlicense option (it declares MIT OR Unlicense), which needs no
   attribution, and the native ufbx library's notice (`assets/ufbx-native-LICENSE`) still
   ships; `mac` 0.1.1 (declares MIT/Apache-2.0) ships the standard MIT text with
   "Copyright (c) Jonathan Reem" (its Cargo authors, repository `reem/rust-mac`) as its
   notice; both recorded as supplements in `scripts/collect-third-party-licenses.py` so the
   packager stops flagging them. When phase 4 starts the owner sees three things
   (recorded 2026-11-15): `block`, `objc_exception`, `leak` and `leaky-cow` have no license
   file on macOS and stay review-required; the owner's five-minute check that a live GitHub
   sign-in logs no token ("Pull-request review: store the GitHub account", raised at phase 5
   and moved here from the post-launch list); and the decision whether to turn on the Linux
   CI archive's upload, held since LIC-B by `HOSTED_UPLOAD` in `scripts/ci/packages.py` (the
   manually dispatched release workflow does not read it: with the notices complete its
   Linux build passes, and its `gitturtle-release` environment is not set up yet). Then add
   an AUR package (`gitturtle-bin`) and
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
