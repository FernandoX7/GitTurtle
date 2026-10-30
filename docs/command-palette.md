# Menus, commands and keyboard shortcuts

Open the native palette with **Command-Shift-P** on macOS or **Control-Shift-P** on Linux, or choose **View → Command Palette** in the macOS menu bar or **Menu → Command Palette** in the Linux window. **Command-P / Control-P** remains Quick Open File. Focus enters the query after the dialog attaches, so typing immediately searches command names and synonyms. The highlight starts on the first command the context can run (the first row when none can) whenever the palette opens or the query changes, and Up/Down moves it one row at a time, onto unavailable rows too; Return opens its workflow; Escape or Cancel restores the previous focus. Opening or cancelling the palette does not change the underlying repository or selection.

`crates/app/src/command_palette.rs` owns the finite `COMMANDS` registry and one dispatcher into existing workflows. The Linux menu uses this same registry, availability checks and dispatcher, capturing the repository identity when it opens and rechecking it at activation. It includes Projects/History/Changes, repository tabs and local workspaces, repository opening, Quick Open, revision comparison, branch/worktree/tag management, GitHub collaboration, reflog, interactive rebase and rewritten-series review, File History, Blame, profiles, appearance, settings, recovery drafts, activity, local Refresh/search, graph lanes, sidebar, the project list pane and editor, terminal and file-manager handoff. No command directly stages, deletes, publishes or otherwise performs a destructive Git write: existing forms, captured reviews and operation checks remain responsible for those actions.

The palette floats on the menus' raised surface with their fitted edge (`Palette::raised` and `Palette::raised_edge`), its result list edged the same way and its resting rows on that surface, so it reads as the same layer as the menus in every built-in and Omarchy palette. Rows show the command, shortcut where assigned, and application/repository/selected-file scope. Disabled commands remain discoverable with a reason, their titles regular in the disabled color where an available command's title is medium. Availability distinguishes no repository, the Projects/Settings page, an active Git operation (which leaves Go to History, Go to Working Changes, Open Settings and appearance available, since switching pages only reads, as their shortcuts and the header's toggles do, along with activity, help and repository tabs; Go to Projects and every other command read “Wait for the current Git operation to finish”, or, while a request of the GitHub panel runs as the busy spell, that request's own reason, such as “Wait for the GitHub request to finish” or, for a PR's local comparison, “Wait for the pull request comparison to finish”), absent branch/file selection, open attribution/history, profile persistence and an existing integration. Activation rechecks both the repository identity and current availability. Highlighting a disabled row never dispatches its command. A claimed activation or cancellation cannot execute twice when an input Return and a dialog confirmation arrive together.

Results are capped at 40, with a 4,096-byte query limit; `COMMANDS` defines the current action set. Every query term must match the bounded command metadata; label matches rank before synonym-only matches. Virtualized rows and a viewport-constrained result surface scale with interface text size. The native input retains its editing shortcuts. Application navigation shortcuts are consumed while the palette is focused, and a palette is not opened over another active dialog or sheet.

The dialog renders owned context snapshots instead of borrowing its parent while that parent renders the dialog layer. Parent notifications refresh the snapshot; activation independently revalidates the live target. File scope includes only the visible workspace mode, so a retained working selection cannot replace the selected history file's label or target.

`cargo test --locked -p gitturtle command_palette::` verifies synonym/all-term matching and query bounds, contextual availability, changed-repository and hidden/busy workspace snapshots, bounded keyboard selection, and single-claim cancellation/activation; `command_palette::render_tests` checks the painted panel, list edge and row fills against the menus' surface and edge in every test palette, that the highlight starts on the first available command and Up/Down still reach unavailable rows, and that an unavailable title is regular; `every_picker_marks_its_keyboard_cursor_at_three_to_one` checks that the highlighted row of the palette, Quick Open and Keyboard Shortcuts, the keyboard's cursor, paints the `selected` fill and a 2 px accent leading marker at 3:1 or more against that fill and the raised surface in every test palette. `key_chips_keep_their_edge_on_the_selected_row` holds Keyboard Shortcuts' key chip edges at 1.4:1 or more against the selected fill on the highlighted row and against the raised surface on a resting one. `shortcuts::list_tests` reads every expectation from where the rows are drawn and from the list's own bounds, never from the layout the list scrolls by, so a wrong layout cannot set the expectations it is checked against. It checks that Keyboard Shortcuts lays its rows out again, each starting where the one above it ends, when a text size is chosen with it open, and that Up/Down leave its list still while the next entry lies whole in it, including after the wheel scrolled it, and centre one that does not or that its edge cuts by a few pixels; that Up onto a group's first entry centres it with the group's heading the wheel had cut; that a click scrolls the least that shows a group's first entry with its heading, and the least that shows an entry the list's foot cut; that a window resized under a list the wheel moved away from the selection keeps the wheel's scroll, clamped to the list's end, while one that would cut a selected entry the list showed whole brings it back whole, judged at the list's height to the pixel; that Down past the list's foot onto a group's first entry centres it with its heading, and Down to the last entry leaves it whole at the list's foot, at 13 and 18 pt; that End leaves the last entry selected and whole at the list's foot and Home the first selected with the first heading at its top, and Page Down and Page Up move by the entry rows the list holds whole, at 13 and 18 pt, also after the wheel moved the list away; and that presses delivered back to back end where they do with a frame drawn after each. Native QA must separately verify immediate typing, Up/Down/Return/Escape, mouse rows, disabled reasons, empty search, focus restoration from editable and inspection views, and transitions into existing dialogs. Check both densities, enlarged interface text, a narrow window, light/dark themes, and the preserved Quick Open shortcut. The current milestone record owns that native evidence.


## Platform menus

macOS keeps the native GitTurtle, File, Edit, View, Window and Help menus,
including Cocoa editing selectors, modifier symbols and system window controls.
Linux exposes a labeled hamburger **Menu** at the left of the repository-tab
strip on every page, above the repository sidebar. **F10** opens or closes it.
The menu groups repository opening, workspace navigation, the repository's
editor, terminal and file-manager handoff, command discovery, and Settings /
Keyboard Shortcuts / About GitTurtle. Commands unavailable in the
current context are disabled; the Command Palette explains their reasons.

This placement and compact grouping follow the current GNOME
[primary-menu guidance](https://developer.gnome.org/hig/patterns/controls/menus.html),
including its placement above a sidebar and standard settings/help items at the
end. Existing window controls handle closing; quitting remains available with
Ctrl+Q. GitTurtle retains its dense workspace rather than adding a second menu
bar to the window. The menu is native GPUI, with keyboard Up/Down, Return,
Escape, accessible roles and labels, and window-constrained scrolling. Closing
it restores its originating focus; activating a workflow restores that focus
before opening its dialog, so dismissing the workflow returns to the editor or
list rather than to a discarded menu.

## Keyboard Shortcuts

Use **Help → Keyboard Shortcuts** on macOS or **Menu → Show keyboard shortcuts**
on Linux, or search for “shortcuts” in the palette. The direct shortcut is
**Command-?** or **Ctrl+?** (Command/Ctrl plus Shift and `/` on a US keyboard).
GPUI normalizes shifted punctuation to the produced symbol on both platforms,
so the binding uses `?` and the menu, palette and reference show that same key. Help opens a focused search field and a
scrolling, grouped reference: General, Repository, Tabs and Windows, Text Review,
Lists and Selection, Text Editing, and Keyboard Navigation. Search matches all
terms against names, groups and key labels; it is bounded to 4,096 bytes.
Up/Down browses results, with a native status announcement for the selected
shortcut; as in the palette and Quick Open, the list scrolls only for an entry
not wholly in it, which it centres. A click selects the entry it lands on and
scrolls the least that shows it whole, as does a shorter window that would
cut a selected entry the list showed whole, or a text size chosen with help
open. Otherwise a window resized under the list keeps it where the wheel or
a key left it, clamped to its rows: a list the wheel moved away from the
selection stays there, where a resize once brought the selection back on
every frame.
Wherever the list brings a group's first entry into view it brings the
group's heading with it, where the list holds both: Up onto it brings back a
heading the wheel left cut, and Down past the list's foot one below it,
centring the two together, and a click, or a change of height that would
cut the entry, scrolls the least that shows both. Down to the last entry leaves it whole at the list's
foot. Home and End select the first and last entries and Page Up and Page
Down move the selection by as many entries as the list's height holds whole
rows, passing over headings, as a page in a macOS list does; each brings the
entry it reaches into view by Up's and Down's rules, so Home shows the first
group's heading at the list's top and End the last entry whole at its foot,
and each shows its entry even where the selection is already on it, as after
the wheel. The search field keeps focus and keeps Shift-Home and Shift-End;
the list takes the four keys from it, bound in the field's context inside the
dialog (`LIST_KEYS`), as macOS's single-line fields leave them to the view
beside them. The dialog's inline help names all six keys. While the field
holds focus, the status bar's hints keep “↑↓ Browse · Esc Close” first and
name “PgUp / PgDn Page · Home / End Ends” after them only where the status
line keeps its whole text beside them (`DialogKeys::key_hints`,
`KeyHints::spare`). Each press reads the
scroll the press before it, or the wheel, left, since a press sets the scroll
at once; the list's height it reads is current too, as GPUI draws a pending
frame before it dispatches a key. Escape, Return or Done dismisses help and restores the preceding
focus. It documents shortcuts; selecting a reference row does not run a command.

The section organization and F10 discovery follow GNOME's
[keyboard conventions](https://developer.gnome.org/hig/reference/keyboard.html).
The app-owned binding registry is `crates/app/src/shortcuts.rs`: startup bindings,
palette accelerators, the reference, and tab tooltips use the same metadata.
Native popup menus resolve those installed bindings through GPUI's keymap.
Labels use GPUI's platform formatter, so Linux displays Ctrl/Alt/Shift and macOS
displays its standard modifier symbols. Repository tab cycling uses Control-Tab
on both platforms. Open Repository advertises Command-O / Ctrl+O while the
existing Command-T / Ctrl+T alias remains available for opening a tab.

Focused text inputs keep GPUI's editing bindings. Linux Redo is Ctrl+Y in the
current toolkit, and Ctrl+H remains text Replace; application Hide bindings are
macOS-only. List-only Copy, Select All Working Changes and Search stay scoped to
lists. With the History or changed-file list focused, Copy Selected List Item
copies the selected row's full commit hash or repository path; with Working
Changes focused, every selected file's repository path, one per line in list
order and each file once; with File History's revision list focused, the
selected revision's full hash. Text that is selected, such as part of the commit
message, is copied instead. With Blame
focused, Copy acts on Blame's selected line. Select All Message Text is scoped
to the focused commit message. In the navigator, the project list, History's
commit list and the changed-file and Working Changes lists, Shift-F10, or on
Linux the Menu key, opens the cursor row's menu, the one a right-click opens;
the reference lists them under Keyboard Navigation. Repository
writes remain in their existing explicitly reviewed workflows.

`cargo test --locked -p gitturtle shortcuts::` checks binding uniqueness, both
platforms' key parsing, focused-list scope, platform labels, tab modifiers, and
bounded grouped search. Native acceptance additionally covers menu mouse/F10
opening, arrow navigation, disabled items without a repository, immediate help
search, Escape/Return/Done and focus restoration from an edited draft; compare
light/dark themes, narrow windows and enlarged interface text. These are checks
to run, not a claim that either platform's desktop has passed; the build-specific
[validation record](validation.md) owns the executed evidence.
