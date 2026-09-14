# Menus, commands and keyboard shortcuts

Open the native palette with **Command-Shift-P** on macOS or **Control-Shift-P** on Linux, or choose **View → Command Palette** in the macOS menu bar or **Menu → Command Palette** in the Linux window. **Command-P / Control-P** remains Quick Open File. Focus enters the query after the dialog attaches, so typing immediately searches command names and synonyms. Up/Down moves the selected row; Return opens its workflow; Escape or Cancel restores the previous focus. Opening or cancelling the palette does not change the underlying repository or selection.

`crates/app/src/command_palette.rs` owns the finite `COMMANDS` registry and one dispatcher into existing workflows. The Linux menu uses this same registry, availability checks and dispatcher, capturing the repository identity when it opens and rechecking it at activation. It includes Projects/History/Changes, repository tabs and local workspaces, repository opening, Quick Open, revision comparison, branch/worktree/tag management, GitHub collaboration, reflog, interactive rebase and rewritten-series review, File History, Blame, profiles, appearance, settings, recovery drafts, activity, local Refresh/search, graph lanes, sidebar and external editor/file-manager handoff. No command directly stages, deletes, publishes or otherwise performs a destructive Git write: existing forms, captured reviews and operation checks remain responsible for those actions.

Rows show the command, shortcut where assigned, and application/repository/selected-file scope. Disabled commands remain discoverable with a reason. Availability distinguishes no repository, the Projects/Settings page, an active Git operation, absent branch/file selection, open attribution/history, profile persistence and an existing integration. Activation rechecks both the repository identity and current availability. Highlighting a disabled row never dispatches its command. A claimed activation or cancellation cannot execute twice when an input Return and a dialog confirmation arrive together.

Results are capped at 40, with a 4,096-byte query limit; `COMMANDS` defines the current action set. Every query term must match the bounded command metadata; label matches rank before synonym-only matches. Virtualized rows and a viewport-constrained result surface scale with interface text size. The native input retains its editing shortcuts. Application navigation shortcuts are consumed while the palette is focused, and a palette is not opened over another active dialog or sheet.

The dialog renders owned context snapshots instead of borrowing its parent while that parent renders the dialog layer. Parent notifications refresh the snapshot; activation independently revalidates the live target. File scope includes only the visible workspace mode, so a retained working selection cannot replace the selected history file's label or target.

`cargo test --locked -p gitturtle command_palette::` verifies synonym/all-term matching and query bounds, contextual availability, changed-repository and hidden/busy workspace snapshots, bounded keyboard selection, and single-claim cancellation/activation. Native QA must separately verify immediate typing, Up/Down/Return/Escape, mouse rows, disabled reasons, empty search, focus restoration from editable and inspection views, and transitions into existing dialogs. Check both densities, enlarged interface text, a narrow window, light/dark themes, and the preserved Quick Open shortcut. The current milestone record owns that native evidence.


## Platform menus

macOS keeps the native GitTurtle, File, Edit, View, Window and Help menus,
including Cocoa editing selectors, modifier symbols and system window controls.
Linux exposes a labeled hamburger **Menu** at the left of the repository-tab
strip on every page, above the repository sidebar. **F10** opens or closes it.
The menu groups repository opening, workspace navigation, command discovery,
and Settings / Keyboard Shortcuts / About GitTurtle. Commands unavailable in the
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
shortcut. Escape, Return or Done dismisses help and restores the preceding
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
lists. Repository writes remain in their existing explicitly reviewed workflows.

`cargo test --locked -p gitturtle shortcuts::` checks binding uniqueness, both
platforms' key parsing, focused-list scope, platform labels, tab modifiers, and
bounded grouped search. Native acceptance additionally covers menu mouse/F10
opening, arrow navigation, disabled items without a repository, immediate help
search, Escape/Return/Done and focus restoration from an edited draft; compare
light/dark themes, narrow windows and enlarged interface text. These are checks
to run, not a claim that either platform's desktop has passed; the build-specific
[validation record](validation.md) owns the executed evidence.
