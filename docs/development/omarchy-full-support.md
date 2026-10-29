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
