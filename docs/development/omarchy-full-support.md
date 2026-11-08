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
   - **Open polish queue (consolidated 2026-11-05).** "The queues" from here on: this list
     replaces the per-session "Queue additions" sub-bullets in `HANDOFF.md` (every session
     bullet from 2026-10-27 to 2026-11-05 morning, checked against `a975f1e9` by read-only
     audits; the closed items and why are in HANDOFF's "Session 2026-11-05 midday" bullet).
     Each landed item is removed from it; new findings are added here, not to HANDOFF.
     Order: user-visible defects and accessibility, then layout consistency, then macOS-only,
     then test infrastructure and nits. Size S/M/L; "verify" marks an item the audit could not
     confirm in code, so its brief reproduces it first. Rounds run up to four implementers on
     disjoint files; a round's slices may change as files free up.
     - **Found on 2026-11-07 (native QA-106c on `ec64f91c`, and the morning's reviews; briefed rounds FU30-A
       conflict review draft, FU30-B Edit local commits, FU30-C lists out of the Tab order are in `briefs-q9/`):**
       (keyboard, M) the Diff view's per-line staging checkboxes are mouse-only (no Tab stop; a click does not
       focus them). (S) The Stashes dialog ignores Ctrl+R and window refocus after a stash is dropped from outside;
       the dropped stash stays listed until its own Refresh. (S) A cold restore briefly shows "GitTurtle · Detached
       HEAD · Profiles" in the header. (verify) Ctrl+[ then Ctrl+Shift+P in one input batch lost the palette in 2
       of 3 (may be the input tool's batching; reproduce with separate key events first). (S) Changes: a
       same-count change (one file moving from Unstaged to Staged) does not re-scroll the list to the selected row
       (AGENTS.md: keep selected files visible). (S, after FU29-A2 and FU29-C) the commit description's hint
       becomes its accessible description through the kit's `aria_description`. (S) File History: the header's and
       footer's own scrolls show no scroll cue; at 18 pt in a 461 px window the "From … · first-parent lineage"
       line is 61-80 px wide beside its copy button and wraps to ~135 px: give the text a minimum width and move
       the button beneath it below that. (S, security hardening) a symlink inside a stored path (`commondir` or
       `core.worktree` through a symlink) re-pointed after open is not re-resolved on the identity's fast path
       (FU26-E2 security re-check C6/S1, `probes-FU26E2-sec2/`; Git never writes either layout). (nit) An old
       alias tab's mark is cleared by an explicit Open only through the exact alias path (`find` matches bytes;
       accepted). (QA note) H4 (2)'s over-4-KiB refusal cannot be produced through a saved session (the loader
       cuts queries over 4,096 characters, `repository_tabs.rs:303`).
       (S) File History: with a failed page, `PanelLeast` reserves the message panel's whole 45 % share, so a
       short message can scroll needlessly (up to ~60 px at 18 pt in 461 × ~560-620): cap the share by the
       message panel's measured extent; share `commit_message::MESSAGE_SHARE` instead of the copied 0.45; the
       walk test's pre-walk clone of the failure region's reveal (FU29-A re-check 2). (S, allocation rule)
       `FocusReveal`: a record path taking one bounds value, so `Reveals::track` and other per-control reports
       stop allocating a `Vec` and cloning the reveal per button per frame (FU29-A3).
       (S) Recovery: the recovery form loads a commit message of up to 64 MiB into a Textarea
       (`recovery.rs:1243-1245`; `git-core/src/lib.rs:1121`) while submitting caps it at 64 KiB
       (`work/recovery.rs:885-888`): cap it when the form loads (FU29-C security review, 2026-11-07). (nit)
       Repository identity: a repository whose `core.worktree` points outside the folder holding `.git` is
       refused at open with "changed while it was opened", wrong for an unchanged layout: say its work tree
       lies outside the folder Git found it in (FU26-E2 security re-check round 4, L4). (defect, M; first)
       Rewritten-series review (Review rewritten branch series…) shows an empty list with no explanation,
       before and after a real rebase through the app: `recovery-drafts.json` holds the capture, Refresh
       series stays empty, and the "No captured interactive rebase is available…" error
       (`recovery_drafts.rs:326`) is never drawn (QA-106e D3/D5, `qa/qa106/REPORT-e.md`,
       `q106-D3series-dfd209f.png`; blocks D5). (S) In that dialog "Check publication destination…" is not
       pulled to the text edge (label ~56.5 px, frame at 42). (M) A click ~40 ms after Return closes a dialog
       is lost: focus returns to the opener's row (18 pt 1480 × 800; History → Manage current branch… → Tab to
       Done → Return → click another row; 250 ms later wins) (QA-106e D6). (S, keyboard) Tab leaves the open
       branch chooser while its popover stays drawn (13 pt 461 × 490) (QA-106e D4). (S) After a rebase the
       status bar and banner show Git's progress fragments ("Rebasing (1/3)Rebasing (2/3)…Successfully
       rebased…") instead of the outcome (QA-106e). (nit) Saved session: `lenient_git_directories` swallows
       JSON syntax errors in its first `Value::deserialize` step with `.ok()`; propagate that step with `?`
       and keep only `from_value` lenient (FU26-E2 re-check, harmless today). (nit) GitHub Files: with a
       single page of files both pager buttons are disabled and no stop reveals the page label (FU28-A
       re-check 3; holds at the walked sizes).
       (S) Changes: an explicit Refresh with a working row selected re-selects it and moves focus to the list
       (`workspace.rs` `select_working_row`), even from the commit title (FU29-A2b, pre-existing, 2026-11-07);
       at scale 1.2 the composer's 16 px padding is not device-aligned, so a one-line description snaps up to
       0.17 px short (pre-existing). (S) With the project pane open in a 461 x 490 window the repository
       page is laid out at its 520 px floor (`views.rs:8661` `repository_width`), so a merge banner's actions
       run to x 559-690, past the window's edge (FU30-A2, 2026-11-07): lay the page out in the width it has.
     - **Found on 2026-11-07 late afternoon (this session's reviews, implementers and release measurement; briefed rounds are in `briefs-q9/`):**
       (S) Rewritten-series review: Refresh series with a loaded series shrinks the list from up to six rows
       to one while it reads, so the controls below jump (`rewrite_review.rs` list height from the row count,
       `load` clears the review). (S, wording) The review's other bounds ("Review at most 100 commits per
       series", "Commit-series review exceeded its deadline") still read as "unavailable… original objects
       may have been removed" (`git-core/src/work/rewrite_review.rs` side context), and the shared timeout
       text (`work.rs` `bounded_write_output`) tells a passive read its result "may have applied partially or
       remotely". (S, shared) `ScrollCue`: the frame that reserves a gutter draws no bar, so the thumb comes
       one frame later (`tags.rs` `ScrollCue::list` flags from the last frame; seen in the conflict review
       and Edit local commits). (M, performance, screen reader) With a screen reader active, a keystroke, a line break or the first frame
       in a multi-megabyte text field segments the whole text again for its text runs: 62-73 ms in release in a
       5 MB editor (`docs/benchmarks/2026-11-07-accessible-text-runs.md`), and a caret move ~8 ms: segment only
       the lines an edit touched and keep the rest (4 ms budget for work a screen reader adds to a frame).
       (M, kit, keyboard; security re-check) The theme editor's colour picker (`vendor/gpui-component/src/color_picker.rs`):
       its 12 featured swatches repeat palette colours and share their keyed focus handles (`color-{hex}`), so Tab
       reaches 83 of its 112 stops and steps back loop, and a focused swatch paints no focus indicator (FU31-C2).
       Decided: the swatch grid is one Tab stop moved by the arrow keys (roving focus, the focused swatch ringed
       whole), the hex field the other; the featured row gets distinct ids.
       (S) The operation error banner draws the failure report's first line, so Git output whose progress
       ends in a newline before `error: …` (a hook printing `Checking (2/2)\n`) reads "Git operation failed:
       Checking (2/2)"; draw the first line of Git's text that starts with `error:` or `fatal:`, else the
       first line (DESIGN.md:273; FU31-C design re-check). (verify) Escape closing History's Columns panel
       (`main.rs` Escape) may leave focus on a removed control.
       (S, kit) Kit popovers and menus a key closes (Select, dropdown, context menu) keep hit-testing against
       the last drawn frame until the next draw, so a click right after the closing key can land on the gone
       popover (FU32-A code review; FU32-A fixed it for dialogs and sheets). (QA) Rerun QA-106e D6 exactly as
       recorded (`wtype -k Return`, a click ~40 ms later on a fixture with a stash, opener the stash row) on a
       build with FU32-A: if the click is still lost, the stash row's selection restore is the cause.
       (S) Edit local commits: when the form gets shorter (an error line goes, "Base edited" disappears) it draws
       one frame at the shorter sequence height before 220 px (`interactive_rebase.rs` `Fit`): reset the sequence
       to 220 px when those lines change. (S, shared) Lists whose bar sits in the dialog's side padding
       (Worktrees, Reflog, Edit local commits) draw no fade cue on a track shorter than 56 px (DESIGN.md:397):
       move the shared cue's fades there with the late-thumb fix. (S, verify) `settings.rs`'s nested wheel
       rule recomputes its step from the window's line height while the list uses the one captured at draw:
       a mouse-wheel line delta at the list's end may never reach the body (as FU30-B3's design re-check found).
       (M, focus) Rings covered by a fill painted after them (FU30-C2's `focus_ring::painted_over` run at every
       stop): a tab-strip Button's ring by its neighbour's fill on every page; Working Changes' header Buttons;
       Compare's toolbar Button; Compare's and File history's text ring by an 82 × 18 fill inside it; at 18 pt
       Buttons by neighbour fills and a 1 px fill. Extend the check to every stop once these are fixed.
       (S, security hardening) Repository identity: a folder above a tab's root re-pointed to another of the
       person's own repositories with the root's folder name is accepted at the next passive reopen (FU33-A
       security re-check S1, accepted residual): compare the Git directory's device and inode saved at capture
       (a same-filesystem move keeps them, so "moved and linked back" still reopens).
       (S) Rewritten-series review's inspector: a failed changed-file or preview read is drawn after the
       preview (below the view at 18 pt 461 × 490) while the boxes say "This commit has no changed files." or
       "Select a file…": say the failure in the box that was being read, through the titled `EmptyState`
       (FU31-B design re-check F3).
       (S) Conflict review: at 18 pt 461 × 490 Restore saved draft moves the draft 27 px as the block review
       lands ("Updating conflict blocks…" takes one line, the landed status two): keep the status's height
       through the review (FU30-A3).
       (S) Conflict review: in a 461 px window each side's no-reader message (no unresolved blocks left, a
       binary file, a side missing) wraps past the versions' 100 px minimum and the footer covers it (FU30-A3
       design re-check N1): the versions' minimum is the larger of 100 px and the sides' header and message,
       and the half-tile cut check stops excluding the versions.
       (S, verify) Theme editor: after Escape closes the colour picker at 18 pt 461 × 490 the trigger's ring is
       not drawn whole (whole at 13 pt 1480 × 973): reproduce whether it is clipped or drawn late (patch
       `briefs-q9/probes-FU31C2/agent-local/item6-with-trigger-ring.patch`).
       (S) Changes: at 18 pt in a 430-437 × 490 window the composer's "Description" label (101 px) runs up to
       8 px past its 93-100 px column (pre-existing; FU29-A2d3); a tall file list that overflows while the
       short body does not scroll shows no cue.
       (nit) Edit local commits' wheel rule: a resize and a wheel before the next prepaint are judged against
       the old reach, and a turn that changes direction within one frame is misrouted to the body: write the
       clamped offset back to the list's handle (FU30-B3 re-check).
       (M, focus, layout) At 18 pt in a 461 × 490 window Blame shows no source lines (their ring 243.5 × 2 px
       under a later border, FU30-C3's pin) and its four footer Buttons draw no ring; rendered Markdown's side
       headings overrun the comparison pane and cut two heading Buttons below its foot; the local document
       dialog's content is a fixed 65 % of the window, taller than its capped body, so its Markdown's ring is
       cut; its Exact source / Rendered toggles' rings, and the remote manager's search field and Add remote…,
       are cut on top, right and left at both sizes by the dialog body's clip, a first row flush with it
       (FU30-C2/C3's audits; pinned there as known failures). Decided fixes (FU30-C design re-checks,
       2026-11-07): Blame's footer (`blame.rs:578-581`, `flex_shrink_0` up to 260 px, an unwrapped row) becomes a
       shrinkable clipping region that reveals the focused control (`focus_reveal`, as `branch_actions.rs:1597`'s
       `content.control`), its row wraps and the lines keep three rows at least (acceptance: Blame's pins leave); the Markdown view shows one
       Link toggle above both sides (it sets one view-wide `linked`) and each side's header is one unwrapped row
       (the label truncates, the controls scroll sideways and reveal the focused one, as the PDF and 3D side
       headers do), then `blocks_room`'s whole-side ring fallback is removed and the blocks keep one line's
       height; a dialog's content keeps `appearance::button_ring_room` inside the body's clip above its first row and
       on its sides, through `pt(room)` with `px(room).mx(-room)` on the content wrapper as
       `branch-chooser-content` (`branch_actions.rs:1595`) does (a negative-margin row changes nothing: FU30-C3
       measured it; 4 px of top padding cleared all eight cuts), and the local document's Markdown is sized
       to the capped body (`flex_1().min_h_0()`) instead of 65 % of the window (`markdown_view.rs:1405`).
       (S, tests) The focus sweep: a list stop's ring is at least one row tall (a collapsed list passes
       today); a sweep of History and Working Changes in the 461 × 490 half tile at 18 pt; the rendered
       Markdown's `cx.notify()` on a side press (`markdown_view.rs:1632`) has no test, since no frame shows
       it yet (FU30-C3 reviews).
       (S, focus) Whatever a GitHub panel reopen focuses, a held control or the section's first stop, in a
       list, a field or the confirmation, the first painted frame draws it whole with its ring, and the body
       moves once: PR rows, the summary, the composers, the confirmation, the fallback onto the first stop
       and the line-clearing nudge keep the later reveal (FU30-D5 design review, 2026-11-07).
       (S) The project pane: a keyboard shortcut for Show or hide the project list (Primary+Shift+B beside
       Toggle Sidebar's Primary+B; the table, user guide, help, palette and menu); its save errors are
       invisible while it is held back or closed; its 180–360 px range and the page's 520 px floor do not
       scale with text size; View → Toggle Project List shows no checked state (FU32-D design review).
       (S) Rewritten-series review: a listed series moves the action row twice as the review opens (as the
       series lands, then a frame later as the body reveals the list the landing focuses): reveal in the
       landing's frame (FU31-B2).
     - **Found on 2026-11-07 evening (this session's reviews):**
       (S, focus) The branch and remote managers open with focus on the dialog shell, which draws nothing
       (DESIGN.md: a dialog opens on its first field): focus the search field when the prepared dialog shows
       (`branch_actions.rs:762`), with the remote manager's query row given ring room (`:1841`). (S, focus)
       Quick source opens focused on its information region, which is not a stop and draws no ring: open on the
       source text (a stop with a ring), and walk from the opening focus in its test. (S, focus) Rings covered
       by later fills (FU30-C2's fill check at list stops): Compare's and File history's text rings under an
       82 × 18 fill, and several Buttons': keep each fill 1 px inside the pane's edge or draw the ring last.
       (S, a11y) A clean Working Changes list is a stop: check with Orca that focusing it says "Working tree
       clean"; if only the list's name is read, give the list its empty line as its description.
       (M, focus, round FU29-A2e, decided) The Working list is one Tab stop, but each drawn row's Stage/Unstage
       and each heading's Stage all are stops too (`workspace.rs:4081`, `:5221`, `:5538`), so what Tab reaches
       depends on the scroll, and a partly drawn row's control takes focus with its ring cut: the list stays
       one stop; Tab from it reaches the selected row's Stage or Unstage, then the headings' controls, then the
       composer; every other row's control is `tab_stop(false)` (clickable, exposed, first in its row menu);
       the control that takes focus is revealed whole with its ring room in that frame (DESIGN.md:395's Reflog
       precedent; reconcile :395 and :296). (S, wording) A repository whose parent folder loses search
       permission is refused as "gone" by writes and as "Not a directory" (Other) by a direct open: `open` and
       `found_at_path` use `fs::metadata` and pass up any error but NotFound. (S) `rewrite_review::content_room`
       restates `github_view::dialog_chrome`'s constants: one shared `appearance` helper; the review's file box
       is a fixed 130 px that cuts its third row at 18 pt: whole rows, scaled with text. (S, a11y) The GitHub
       panel says two announcements after a resize while it was closed (FU30-D2's nit); the panel registers no
       `DialogKeys`, so its regions' keys are never announced (DESIGN.md:283).
       (S, round FU30-D8, decided) The GitHub confirmation lays its first frame out from the last frame's
       measures: a new panel's first long confirmation, and a width-only resize that rewraps its guidance or
       its buttons, draw one frame with the guidance in the wrong place and Send or Keep editing cut (DESIGN.md
       :321, :325): decide `inside` and the cap before layout from `panel_width(window)`, the guidance shaped
       at that width and the chrome from known sizes; tests: a long reply as a new panel's first confirmation
       at 18 pt 461 × 490, and 1480 → 461 at 13 pt with a long reply focused.
     - **Found by native QA-106a (2026-11-07, release `6fff51f5`; all on the previous build too unless said):**
       (keyboard, FU29-A) at 461 × 490 / 18 pt, Escape closing File History leaves nothing focused and Escape and
       Ctrl+Shift+P then do nothing until a click; Changes' Tab goes from Commit title to Main Menu, skipping the
       description and Commit. File History at 18 pt in 461 × 490: the message area is 15 px tall and draws nothing,
       the top row ("File history ← B") and Close are cut at the window's right edge. After an external checkout and
       Ctrl+R, History's refs and sidebar update one frame before the Git actions bar (one-frame lag; low). (verify)
       FU27-D's QA note expected "History rows 21–520" in the status bar after a restore past the end; it never shows.
       N2 (new with FU26-E: the stale "Git rev-parse failed" banner and a half-cut notice line beside the mark) is in
       FU26-E2's fixup pass. File History in windows shorter than 490 px at 18 pt can shrink its revisions to
       nothing (no scrolling fallback; FU29-A). A status line draws a two-line text whole, its first line rising
       above the 26 px bar (`views.rs:~9073`; FU29-B: draw the first line only). An unrelated watcher warning set
       before a tab is marked stays beside the mark (FU26-E2 report; low).
       Found by QA-106b (2026-11-07; also on the previous build): With eight tabs, Ctrl+T opens the Open Folder dialog and refuses only after a folder is
       chosen (`repository_tabs.rs:1453`). As History's read lands, Orca announces its list over an open profiles menu.
       A failed profile store read leaves the menu header "Profiles are unavailable or still loading" above the failure
       line. The palette's first row is still heard twice on open (QA-106b O2; a slow "go to hi" repeats the whole
       reading at "i": FU22-B's fail-safe, kept).
     - **Held Enter residuals (FU21-E landed 2026-11-06: a held Enter that opened a dialog reaches
       nothing in or behind it until its key-up, input-method repeats included; native QA-106 pending):**
       (macOS, verify) a Command pressed mid-hold clears the press's start, so if Cocoa keeps repeating
       Enter across the change the first plain repeat after Command's release reaches a field's or
       list's binding (never a confirmation; FU21-E review N3, security S8).
     - **Accessibility and polish found 2026-11-06 (FU22; FU24-C landed 2026-11-06: Keyboard Shortcuts' status
       says a spoken entry once, the tab strip and window controls keep a ring's room apart):** the palette's
       first row may be heard twice on open (QA-106 listens). Palette, pre-existing (FU22-B review): a key
       burst that ends on the reading it started from stays silent even where Orca last spoke another
       row; a click on an unavailable row leaves the status's reading stale (only the error alert is
       heard); held Down repeats under 100 ms apart leave the row they end on unheard (QA-94's quiet
       moves) (FU25-A, with the palette taking FU24-C's returned-row rule and one shared timing owner).
       FU28-D landed 2026-11-07 (a decorated exact-fit strip case; a field's caret no longer blinks unfocused after its
       context menu closes, only the code editor's completion and code-action menus holding a blur; nits below).
       FU27-A landed 2026-11-06 (one reason per group feeds the profiles menu's rebuild key and its rows; the open menu
       shows and drops Retry as the store fails and recovers); its review's nit (low): the "Profiles: …" line can keep an
       older failure's text while the menu stays open (only `load_profiles` replaces an error without clearing it, at
       startup and from Retry; probably unreachable); the `docs/profiles.md:3` Retry nit landed with FU28-D.
       FU27-C landed 2026-11-06 (the dialogs' ghost walk measures a Button's widest content, its held-back Copied
       included; Copy commit ID wraps beside its glyph in a column narrower than its content, the glyph beside the first
       line, Copied in place; one `select_newest` helper); its re-check's nit (low; the glyph tolerance one landed with
       FU28-D, now 0.25 px): `confirming_with` clones `id` for the test-only selector each frame in release
       (`copy_feedback.rs:524`).
       FU27-B landed 2026-11-06 (the GitHub connection pane's "Local worktree:" line keeps to one line, its path
       elided in the middle as the header's; the panel's stops all draw their rings, checked over 1,526 stops); its
       probe's two findings (Pulls at 17 pt in 461 × 490, stops 2 and 3 cutting a line at the body's edge; no stop
       showing `github-captured-identities` at 17 pt 461/440 px and 18 pt 480 px) landed with FU28-A on 2026-11-07.
     - **A Refresh moves a tab whose `.git` vanished to an enclosing repository (found by FU26-A, 2026-11-06; FU26-E):**
       `RepositorySession::open` rediscovers from the tab's path, so with an enclosing work tree (a dotfiles
       repository in the home directory) the tab silently shows, and later writes would target, that repository.
       Decided: a passive reopen that resolves to another root shows the tab unavailable, never switches.
       FU26-E landed 2026-11-06 (writes refused and tabs marked once Git finds another repository, or none, at the
       path; with none, `repository_access::explain`'s reasons on every surface; a quiet read that finds none marks the
       tab; release: +1.4 ms per Changes read and per write, one `rev-parse` each). Left: the hardening round FU26-E2
       (every command of an opened repository with `GIT_CEILING_DIRECTORIES` at the parent of the folder holding
       `.git`, closing the old handle's reads between `.git` vanishing and the mark, the Git directory captured,
       FU26-E's per-operation `rev-parse` removed; Show latest marking; a running quiet read's banner beside the mark;
       brief `FU26-E2.md`); an old session's subfolder-path tab reads "is gone" for good (message); `Path::is_dir()` is
       false on any metadata error, so a network-volume blip marks a tab until the next quiet read; a marked tab's
       explicit Refresh in Changes keeps the mark's old reason after Git starts finding another repository (low).
     - **Test infrastructure (2026-11-06; FU20-M and FU22-A (the PDF helper test's torn status read)
       landed, the testing skill names the recurring races; FU23-A in review: `564d0be6`'s cause was
       macOS's long `TMPDIR` wrapping the fixture path, and the paging test's seed caches):
       FU25-C landed 2026-11-06 (one test root per process, removed at exit, a killed process's root swept
       by the next run); its review's nits landed with FU28-C 2026-11-07. FU26-A landed 2026-11-06 (the
       long- and in-checkout-`TMPDIR` test causes; `narrow_history_keeps_paging_in_the_column`'s seed caches were
       already emptied by `59a5a3ef`).
     - **FU19 (B, C, D, G and E+H landed 2026-11-05 afternoon, D5 the same evening; A (with A2, A3) and C2 (with C3, C4) in flight; F waits for FU19-A's kit dialog file and FU20-A's profile dialogs):**
       - (A) Focus return: FU19-A, A2, A3 and A4 landed 2026-11-05 night (a closing dialog's
         focus returned once, never over a later click; a dialog opened as another closes keeps
         its return; Back's return forgotten on every way out of Settings; a dialog opened before
         Back's frame keeps focus; History's search held for a loading tab and run or dropped as
         its restore ends). FU19-A5 landed too (every restore end runs or drops the held search; a
         restore always ends, including a refused 4 KiB query and a write-refused search).
         FU27-D landed 2026-11-06 (a restore's unread page leaves paging on the shown history; a page appends only in the
         window's scope; a saved page past the end shows the last page; the opener on both pages is unreachable and PG5 was
         already fixed by `a9a62b4b`); its re-check's nits landed with FU28-D 2026-11-07.
       - (C2) Editor highlights' palette rules: FU19-C2 to C5 landed 2026-11-05 night (More
         contrast word tints beyond their lines, Find's other matches fitted by the text rule, the
         current match's off distance, fallbacks keeping Off colours; the hover/selected order in
         every built-in and Omarchy palette, with One Light, Kanagawa Wave and Kanagawa Lotus
         tuned and Lotus's body text a narrow exception). Left: a crossed word with no readable
         step may fall back nearer its moved line (no palette reaches it; low).
       - (F) Ghost Buttons in dialogs, M (decided 2026-11-05; FU19-F1 landed 2026-11-06: dialogs' side
         padding max(16 px, the ghost pull + ring room) rounded up to the device pixel, the ten-dialog test; FU19-F2 landed the same afternoon: ghost Buttons on the text edge at FU18-B's dialog sites, tall dialog bodies scrolling in the app's own view with every Tab stop revealed, the series review's inspecting mode and its preview's and messages' rings): dialogs' side padding max(16 px, label inset + ring room), 17 px at
         18 pt, and the ghost-Button rule applied at FU18-B's dialog sites (profiles, signing,
         Fetch rules…, Worktree created, Activity, LFS source, rewrite review, revision
         choices, About's Copy bug diagnostics); the 0.75 rem bars keep their frames (the ring
         wins); dialogs whose scroll views may cut a ring on the left (profiles, the profile
         form, Activity, the LFS form, the comparison form; verify); Reset column layout and
         Choose a project at a card's edge; the worktree manager's fixed 11 px text inset
         against the scaled label inset.
     - **FU20 (proposed):**
       - (GitHub Files: FU20-GH and FU20-GH4 landed 2026-11-05 night (the narrow body's fit, Source's ring, Overview's stop 12, editor rows whole, the body's reveal on return, the list fitted with no file selected, the mode row cache's tests; Changes' range hint may lie below the body's foot, decided); FU20-GH5 landed 2026-11-06 (a text's lines worked out once per change, allocation hygiene: no CPU change in release; the opt-in rest bench; the sweep at `REVEAL_SIZES`); FU20-GH6 landed 2026-11-06 (the PR list's first row and Changes' patch rings whole at 18 pt, a row a list cuts counted where the list shows it, the sweep reading lines as drawn, its found walks always-on; native check in QA-106); also soft-wrapped rows above an editor's top row uncounted (low), the title-and-mode-row height reused from another text size for one frame, and `files_settles_in_a_frame_after_a_resize` hanging under one mutation (test infra); decided 2026-11-05) the panel's Files section fits a narrow
         body: the changed-file list at most half the body's height, the patch or Source the
         rest (never fewer than five lines), the mode row wrapping inside the body's width,
         every ring whole (FU18-F's sweep at 461 × 490: the mode Buttons' rings reach x 449.5 /
         607 past the body's 423; at 18 pt the patch and Source have zero height under the fixed
         324 px `FILE_LIST_HEIGHT`; the list taller than the body); the sweep's never-whole lines analysed;
         the D+E review's bounded list span, scrolled-editor test and padding rows;
         `the_reveal_sweep_finds_every_line_whole_and_every_ring_drawn` always-on once green
         (`github_view*`, `focus_reveal.rs`).
       - (A) Overflow and scroll cues, M (FU20-A and FU20-A2 landed 2026-11-05 night: the branch
         chooser, the profile dialogs, the worktree manager, the conflict draft in whole code lines,
         the commit message's overflow cue inside its viewport; FU20-A3 landed (the branch picker's and branch dialogs'
         disabled reasons; the tracking-branch feedback already worked, now covered); FU20-A4 landed (the conflict review scrolling in a short
         window, decided; a focus the app moves for the person reveals its target)): the profiles menu's disabled items without a
         reason (verify, with FU19-F's `profiles.rs`);
       - (History, remaining; FU20-H landed 2026-11-05 night: Refresh's stash list past a stash chosen meanwhile, Reflog and Tags focus by row identity, the partial subscription item dropped (does not reproduce since `8408dec4`); the tracking-branch feedback covered by FU20-A3 (landed); left (FU22-D landed 2026-11-06: `FocusReveal` keeps handles only for drawn, focused or elsewhere-held controls); FU25-B landed 2026-11-06: a cleared stash inspection hands its focus to the stash list) S: `labelled_scope_toolbar_width`'s per-frame count string (FU20-P, in review).
       - (C) Text cuts: deferred to upstream (decided 2026-11-05 evening). GPUI's own `.truncate()`
          (`LineWrapper::truncate_line` in the unvendored core `gpui-pre` 0.3.4, `ceil_char_boundary`)
          cuts between characters for every overflow ellipsis (subjects, branches, tabs, paths); vendoring
          the 78,000-line core for a rare visual cut is not worth its upkeep and review surface. Revisit if
          `gpui-pre` is vendored for another reason or upstream cuts at clusters. The app's own cuts follow
          grapheme clusters (FU19-D5, landed).
        - (D) Small visible polish, S (FU20-D landed 2026-11-05 night: Settings' text fields named by their drawn labels; `LineKey` against a scale change did not reproduce: its widths are logical and snapped per frame at the current scale; FU22-C landed 2026-11-06: tabs keep a ring's room apart at 11 pt (the 1.5 padding was the shared outset rounding), a preview chosen again during its in-process decode takes it over (every other phase still stops on supersede, Back and close), popover reduce motion did not reproduce (guard test); `tab_whole_widths` is test-only): FU26-D landed 2026-11-06 (the caret already showed on focus; it is now held a whole blink phase after every key, edit, bound action, selection and composition; the GitHub destination field's ring was whole, now guarded);
         the 2026-10-28 taste items decided then (profile tile, review row
         pitch, PR rows' resting surface, the travelling row's edge, a centred "Fetched";
         verify each); the README's
         `settings-themes.png` retaken (with a native round).
     - **Later rounds (accessibility):** (FU19-E+H in flight with FU19-H2) AT-SPI's cache
       signals fixed at their root (AccessKit #819 backported) and the removed row kept until it
       leaves the bus; Orca's check in QA-105. The palette speaks its current row twice, plain then "· 1 of 1" (confirmed natively by QA-105); X11 key repeats unmarked (vendored `x11/client.rs`, verify; FU19-D5's fixups touch
       the Button side); at a window's close the adapter sends no `RemoveAccessible` for any node
       (upstream behaviour, libatspi keeps the proxies until the app exits; low).
     - **macOS-only (hosted run or a Mac):** Menlo's metrics for FU17-B's rule (hosted run
       `42074771762` passed its tests; native position unmeasured); VoiceOver never hears the
       palette's status; a refused menu-bar item looks enabled (`platform_polish.rs`); the
       animated AVIF caption on macOS (`native.rs`); VoiceOver's reading of check menu items
       and of copy announcements from dialogs (the backlog line above).
     - **Performance, S-M:** FU20-P2 landed 2026-11-05 night (`render_git_actions` builds its text only on a change); FU22-D landed 2026-11-06 (empty states build their line text only on a change); FU20-P landed 2026-11-05 night (the chrome's text and the scope toolbar's count built only on a change; release: no CPU change, allocation hygiene); FU25-D landed 2026-11-06 (the header's labels, tooltips, picker names and badge kept per input, the profile button's name and tooltip and `branch_actions.rs`'s picker label included; release: no CPU change, allocation hygiene; `tags.rs`'s read already ran only on a change, `bd8a67f5`); FU26-F landed 2026-11-06 (the shared Button helpers' icon paths and names kept per symbol, the header's line height read once per frame, the status bar's hints and Settings' Back tooltip built only on a change; release: −2.3 to −2.7 % UI-thread CPU per frame at rest); FU28-B landed 2026-11-07 (a dialog error line's state key and text, the project hub's icon paths and recent rows' text kept, the kit's Button key context built from its static name; release: −151 allocations per frame at rest on Projects, −27 to −31 on History and Changes, no CPU change; `empty_state.rs`'s cost was already gone); left:
       hayro's full-resolution decode profile and the grained-alpha peak on x86-64 (measure);
       read tags after the history page when only one CPU is available (+5.9 ms on one core,
       brief 10's follow-up in `c219a6f2`), M; a release scroll measurement for the per-row
       clip (brief 12).
     - **Kit input drawing (FU20-GH3, by reading):** a multi-line Input paints one row past its viewport into its padding (`vendor/gpui-base/src/input/base/state.rs:3416`, `element.rs:1187`), clipped at the padded box (GPUI 0.3.4's `overflow_mask` clips y while x is hidden: FU20-GH3's re-check corrected the earlier "no vertical clip" reading), so no row can draw over the border; a native look in QA-105 confirms, and only a row seen past the box would need a vendor fix. Also `conversations.rs:669` replaces a part selector (harmless).
      - **Dialog Enter nits (FU18-C re-checks; FU19-D5's X11 residuals: a repeat's unpaired release at a poll batch's end can start a fresh press; a held Enter on a Button that keeps focus presses on each X11 repeat; macOS Enter-then-Command may lose one Return):** the window's Enter mark goes stale when the
       release lands in another window (one ignored Enter; clear it on deactivation); a
       single-line field's own Enter binding still runs on held repeats (no repository write
       submits that way).
     - **Test infrastructure and nits (FU28-C landed 2026-11-07: the PDF helper tests wait on `Slots::waiting`, the
       hold's send no longer blocks on a full queue; the packager and confirmation-test items and SK1's nits were
       already fixed):** the GitHub Files title-and-mode-row height reused from another text size for one frame
       (FU20-GH4); `SerialExecutor::hold` delaying quiet jobs queued during the click (FU17-E);
       `github_view.rs`'s remaining test premises after FU18-D; tests calling
       `hold_quiet_reads(true)` directly and the other quiet-read wait helpers checked against
       FU12-E's window (`repository_tabs.rs:6807`, `motion.rs:8964`, `page_navigation.rs:920`,
       `recovery.rs`); FU12-D's test needing a Tab first; the M4 Tab test's first-frame scroll;
       FU11-B's redraw-at-rest nits; FU13-B's click test relying on GPUI's stray frame and its
       patch note; no press test for the dropped-reply release (FU13-C 1, `main.rs`);
       `State::reset` clearing the hold (`automatic_refresh.rs:90`), the GIF assertion and the
       hue order under 7:1 (FU10-C, M1, M2 N3); FU14-B's first-paint gating untested; FU8-D's
       wrap slack and `FIT_SLACK` only on a test fault; the AVIF-animation fixture not
       reproducible (Pillow timestamps); the benchmark records' kill-at-work low ends predating FU8fix (a note at the next measurement).
     - **Tooling and CI:** `qa.py` cannot launch on this host (python-xlib) and has no
       Hyprland backend, frame recorder or horizontal scroll (QA harness); the QA tooling
       items (ΔEOK analysis kind, the `;` keysym, per-channel tolerance, `runenv.py:323`
       measuring its path in characters); zizmor's `$/`,
       icu/json5 and rust-cache upkeep in CI; LFS 3.8.1 on the runner images and whether to
       warn before a clone needs LFS (decide with phase 4).
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
