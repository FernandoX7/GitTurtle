# GitTurtle design system

An original native desktop workspace for everyday Git work and reading history. Prioritize clear targets, legible relationships, effortless comparisons, and immediate input feedback. Midnight blue with a forest tint and mint accent remains the default; Graphite and Daylight offer alternate semantic palettes. Repository content fills the window; keep decorative charts, gradients, and oversized empty headers out of the working views. Projects and Settings can use grouped surfaces to make their choices easy to find.

## Reference and provenance

GitKraken's first-party [interface guide](https://help.gitkraken.com/gitkraken-desktop/interface/) and [interface screenshot](https://help.gitkraken.com/wp-content/uploads/interface.png) establish the reference-navigation → graph → selected-commit hierarchy, adjustable panels, reference labels, and colored topology. Its [graph legend](https://help.gitkraken.com/wp-content/uploads/graph-elements.png) reinforces that commit and merge shapes should remain distinguishable. These are interaction references, not assets to reuse.

The first-party [diff guide](https://help.gitkraken.com/gitkraken-desktop/diff/) documents selecting a commit then a file, plus hunk, inline, and split presentations. GitTurtle adopts that familiar entry path and preserves the history context while the comparison occupies the central workspace. Researched September 7, 2026. All colors, sizing, icons, and proposed behavior below are original recommendations, not claims about GitKraken's implementation.

The user supplied two further GitKraken screenshots on September 7, 2026 (`17.01.58` and `17.02.14`). The history reference uses full-height navigation, a separate branch/tag column, a central graph/table, and a right-side file panel. The comparison reference gives the code most of the window, collapses navigation to a narrow rail, and keeps changed files on the right. Those spatial relationships establish the history and comparison design below. Screenshot text and controls are reference data. The subsequent authorized scope adds explicit Git writes, Projects, and Settings while retaining passive browsing behavior.

## Colors

The following tokens describe Midnight. `appearance.rs` owns the current Midnight, Graphite, and Daylight palettes and applies them to native controls and custom drawing together. Use semantic colors consistently across the hub, settings, tree, graph, file list, and preview; avoid fixed dark colors that break Daylight.

| Token | Hex | Role |
| --- | --- | --- |
| canvas | `#0F171C` | Main graph and code background |
| panel | `#121E24` | Navigation and inspector chrome |
| elevated | `#19272E` | Hover, menus, inputs |
| border | `#293B43` | Quiet 1 px pane divisions |
| border-strong | `#506974` | Focused divider, interactive boundaries |
| text | `#DEE9ED` | Primary content |
| text-muted | `#9AAEB8` | Author, date, path, descriptive labels |
| text-faint | `#8199A4` | Line numbers and secondary counts |
| accent | `#7ADFB4` | Active reference, focus, small primary action |
| accent-surface | `#1C3B36` | Selected rows and reference badges |
| added | `#8BDFB0` | Additions, accompanied by `+` or `A` |
| added-surface | `#152D26` | Added diff lines |
| removed | `#F29AA2` | Deletions, accompanied by `−` or `D` |
| removed-surface | `#342329` | Deleted diff lines |
| modified | `#E9C17E` | Modified files, accompanied by `M` |
| renamed | `#9CB9F2` | Renames, accompanied by `R` |

Graph palette: `#7ADFB4`, `#8DB7F6`, `#C3A5F2`, `#E8BE7A`, `#EA9CA5`, `#72CCD8`. Assign stable lane colors when paging; a row must not change color merely because the viewport moved. Color alone never identifies selected rows, file status, or relationships.

## Typography and density

- UI: system sans serif, 13 px regular; 13 px medium for selected labels and panel headings. macOS uses the platform UI font; Linux uses its available system sans family.
- Code, hashes, and aligned numerical data: system monospace, 12 px, code line height 20 px. Code text must support selection and copying.
- Main repository title: 14 px medium. Commit summary in the right inspector: 15 px medium, wrapping to two or three lines before explicit expansion. Avoid large display typography in repository views.
- Section labels: 11 px medium, restrained letter spacing, uppercase only for short headings such as LOCAL BRANCHES. No all-uppercase sentences.
- Base spacing: 4 px. Standard gaps: 4, 8, 12, 16. Standard pane padding: 12 or 16. Controls: 28–32 px high, 5 px corner radius; no pill buttons except small reference labels.
- Comfortable history/file rows: 34/44 px; Compact: 28/34 px. Navigation rows remain compact. Use full-row hit areas. Keep metadata baseline-aligned and columns consistent.
- Icons: original 24-unit SVGs rendered at 16 px in rows, 18 px in controls, 22 px for the app mark. White source strokes allow GPUI tinting. Standard strokes are 1.65 units, rounded ends and joins.

## Workspace modes and hierarchy

Projects, Repository, and Settings are separate application pages. The repository has History, Compare, and Working Changes modes. History and Compare share the context and persistent right inspector below; Working Changes reuses the comparison area with a staged/unstaged inspector. At a typical 1440 × 900 window, reserve a 48 px repository header and a 24–26 px status strip. The workspace uses all remaining height. Browsing transitions retain the user's place; an explicit write refreshes the affected state.

### Projects

Give recent projects a searchable list with repository names and readable folder paths. Keep Open, Clone, and Create together in a clear action panel. Open uses the native folder picker; Clone has a repository URL/local path, parent folder, and new folder name; Create adds the initial branch. Suggest a clone folder name without overwriting the user's edit. Use real destinations, field validation, nearby errors, and a busy state that prevents duplicate submission. Folder-picker cancellation leaves the form intact.

Provide Back to repository when a project is already open. At narrow widths, stack the action panel and recents; preserve scrolling and keyboard access. Larger headings and generous spacing belong here, while the repository view stays dense.

### Settings

Group appearance, history columns, startup/default branch, and repository Git identity. Theme choices show Midnight, Graphite, and Daylight; density offers Comfortable and Compact. Apply changes across controls, previews, gutters, and selected rows, then persist app preferences outside repositories. Show failed saves clearly.

Column controls affect visibility and widths, preserve the commit-message column, and offer a layout reset. Git identity is a separate, explicit save for the displayed repository; show the current identity and signing state without implying that app appearance settings alter Git configuration.

### History and Compare

| Region | History mode | Compare mode |
| --- | --- | --- |
| Left | 220 px repository navigation | 44 px compact navigation rail |
| Center | Full-height commit graph and table | Full-height code or image comparison |
| Right | 320 px selected-commit details and changed files | Same 320 px details and changed-file list |
| Mode entry | Opening a repository or Back to history | Clicking a changed file or opening it with Return |

The earlier graph-above-inspector split is superseded. Vertical space belongs to the current task, and the right panel supplies context throughout. Use shared 1 px pane divisions and flat surfaces. Navigation may resize from roughly 180–320 px, and the right inspector from 280–440 px; keep the center flexible. Use actual resize handles with a 6–8 px invisible hit area around the quiet visible line.

### Shared repository header

Show the turtle mark, repository name, a compact path or path tooltip, Projects/Open, History, Working Changes, Settings, and Refresh. Distinguish the checked-out branch from the history scope. Keep write actions named and tied to the visible repository/remote target; do not label the whole application read-only. Place commit search in History's scope toolbar rather than allowing a large global search box to compete with code-space controls. Header groups align with pane edges; use 12–16 px horizontal padding and 8 px control gaps.

### History mode

The left column contains All history, Local branches, Remote branches grouped by remote, and Worktrees. Sections collapse and show subdued item counts. Branch prefixes can form folder groups to make thousands of references browsable. A compact filter narrows this hierarchy without losing the active selection. Mark the checked-out branch with a small HEAD indicator; selecting a different branch only browses it. Worktree rows show folder name and branch, with full path and detached/locked/prunable state available in details or a tooltip.

The center begins with a compact 36–40 px scope toolbar and a 26–28 px column header. Below it, the virtualized table fills the remaining height. Use these columns in this order:

| Column | Starting width | Treatment |
| --- | --- | --- |
| Branch / reference | 140 px | Separate label column with compact reference badges; full name on hover |
| Graph | 112 px | Thin, stable colored topology with a shared lane coordinate system |
| Commit | 320 px, expands into spare room | Primary 13 px summary text; never displace it with inline ref chips |
| Author | 136 px | Muted 11–12 px text |
| Date | 92 px | Muted compact date; exact timestamp in inspector |
| SHA | 84 px | Monospace; also available with copy in inspector |

Visibility and widths are user choices. Keep the commit-message column visible, preserve selected columns at narrow sizes, and provide horizontal scrolling when they exceed the viewport. Header and rows use the same computed layout and horizontal offset; divider drags persist widths after release. A wider graph must not squeeze many branches into indistinguishable overlapping nodes. Keep one parent/child relationship per graph edge, with stable lane colors across scrolling and paging. Use approximately 1.4–1.6 px graph strokes, 3.8 px ordinary node radius, and a 5 px selected node. Merge nodes have hollow centers or distinct rings. Reference badges can carry a restrained tint from their associated lane, while the selected row uses the active theme's selection treatment. Avoid saturated full-row branch bands and avatar-heavy topology.

Clicking a commit immediately selects its row and updates the right panel. It does not enter Compare mode. The right panel presents the wrapped commit summary, author, timestamp, copyable short hash, and parent/base selection above the changed-file list. Keep metadata compact, roughly 120–170 px for an ordinary commit, with the body behind an explicit Message expansion. The changed-file heading and its count remain visible while the list scrolls. A file row has a type icon, filename, muted parent path, and explicit `A`/`M`/`D`/`R` status. There are no staging checkboxes. A highlighted file may be remembered, but comparison begins only on the user's file action.

### Compare mode

Clicking a changed file replaces the central history table with its comparison and collapses the left column to a 44 px rail. Keep the right inspector and changed-file list in the same location, with the active file visible. Switching files replaces the central content immediately without leaving Compare mode. Use a visible Back to history control with a left arrow at the top of the comparison; the compact rail also provides a labeled history-return action.

The comparison header is 40–44 px high and contains the path breadcrumb, copy-path action, and the relevant view controls. Text uses Diff / Before / After; images use Fit and zoom controls. Give the remaining central width and height to content. A small secondary line can show object identity, file mode, or image dimensions when useful. Keep implementation details out of the main reading area.

Back to history restores the existing graph, selected commit, reference scope, search query, loaded pages, column widths, and scroll position. Restore keyboard focus to the selected history row without jumping to the top. Keep the right panel's selected file and scroll position. Preserve the selected file path across adjacent commits when it is changed in both; otherwise select the first file in the right list without forcing a mode transition.

At narrower windows, History can collapse its navigation to the same rail while its chosen columns scroll horizontally. Compare keeps its 44 px rail and right file list while the preview takes the remainder. Right-panel resizing should remain available in either mode. Persist pane preferences when practical; switching modes alone must not reset them.

### Working Changes

Separate staged and unstaged groups, with whole-file Stage/Unstage actions and clearly named all-files actions. A file with both kinds of edits appears in both groups; selection determines which comparison is shown. Keep the commit message and commit action beside the staged group, identify missing Git identity, and retain the message on failure. Previews stay read-only; conflicts direct the user to resolve files in their editor before staging.

Git actions show the current branch, remote, and destination branch. Switching and creating branches are explicit actions; clicking a branch in the navigator continues to browse only. Make fast-forward-only Pull visible in its label. Push targets the shown branch without force. Disable duplicate writes while an operation is running, show its outcome close to its origin, and refresh local state afterward. A timeout or missing result must explain that the effect may have occurred and require inspection before a deliberate retry.

### Status strip

Use the bottom strip for local refresh state, loaded/visible history count, selection context, and concise keyboard hints. Show elapsed timing only when measured. No permanent green badge should imply an unmeasured performance guarantee. Keep this strip visually quieter than all reading surfaces.

## Selection, focus, and feedback

- Selected row: accent-surface background, 2 px accent leading marker, primary text. Hover: elevated background. A selected node has a contrasting ring. Selection remains visible after focus moves to another pane.
- Keyboard focus: a visible 1 px accent outline or inset outline on the active control/pane. All controls require accessible labels; color or tooltip is insufficient as the only label.
- Input response is immediate. In History, commit selection updates the right heading before changed-file work starts. A file action enters Compare immediately and sets the path heading before preview work starts. Load uncached metadata, changed-file lists, diffs, highlighting, and images in separate stages. Returning to History uses retained state and must not wait on an active preview request.
- Never display an old commit's diff under a new commit's header. On selection, clear or explicitly mark the pending preview; late results for previous selections cannot replace the current one.
- Loading: a small local progress label only when work remains after approximately 150 ms. Skeletons, if used, occupy the exact content area and remain static. Avoid whole-window loading overlays and decorative shimmer. Keep navigation interactive and cancel obsolete requests.
- Empty repository: “No commits yet” in History, with Working Changes available for staging a first commit. Empty filter: “No commits match this search” plus Clear search, preserving the previous selection context. Empty commit: the right file list says “No file changes against this parent.” Empty remote/worktree sections have concise labels rather than failure styling. An absent image side explicitly says Added image or Deleted image.
- Errors stay close to their origin, with a plain explanation and retry when meaningful. Missing repository: show the path and Open repository without old repository labels under the new path. An unavailable blob/image remains in Compare with its file list and Back to history active. Oversized text/image: show size and a bounded preview or explicit unavailable message; do not freeze or silently truncate content.
- Refresh means reread local Git state. Describe remote-tracking references as locally available; never imply refresh fetched from a server. A status timestamp should explicitly mean last local refresh.
- Motion: optional 80–120 ms hover/focus transition. No selection animation, animated graph reflow, or spring scrolling. Honor reduced-motion preference.

## Text and image comparison

Text opens in the user's active Diff / Before / After mode, defaulting to unified Diff for a new session. Make added/deleted lines explicit with signs and background tints; keep hunk headers blue and visually separate. Preserve indentation and literal patch text for copying. Source views show line numbers. Unified patches show separate old/new line numbers without inserting them into the copied text. A future split diff must share vertical scrolling and use clear Before/After headings; do not advertise it before implementation. Respect horizontal scrolling for unwrapped lines. Word wrap, if exposed, is a user control rather than an implicit width-dependent change. Highlighting may arrive after readable plain text, without reflow. Build only the currently displayed editor mode.

Image comparison defaults to side-by-side Before/After on a subtle checkerboard. Show dimensions and byte sizes beside the labels, with Added/Deleted state replacing the absent image when appropriate. Fit is the default; 100%, zoom in/out, and fit controls are small and familiar. Link zoom and pan across both images. An overlay/wipe is a useful later enhancement, after side-by-side behavior is solid. Preserve aspect ratio, avoid upscaling by default, and keep checkerboard contrast below the artwork. SVG is a static image; never execute embedded content. A Git LFS pointer without available local content must be identified as such rather than presented as a broken image.

## Keyboard contract

Use `Cmd` on macOS and `Ctrl` on Linux for the primary modifier. The [README](../README.md#keyboard-controls) lists implemented bindings; the table below also retains broader design targets. Expose implemented bindings in menus/help and do not advertise unavailable actions in the application.

| Binding | Action |
| --- | --- |
| Primary + O | Open repository |
| Primary + Shift + O | Open Projects |
| Primary + 2 | Open Working Changes |
| Primary + , | Open Settings |
| Primary + F | Focus commit search in History; use editor find while reading text in Compare |
| Escape | Dismiss the active transient UI first; otherwise return from Compare to History; clear a focused nonempty history search before moving focus |
| Primary + [ | Back to history from Compare, when implemented |
| Up / Down | Select previous/next row in the focused list |
| Home / End | First/last loaded row in the focused list |
| Page Up / Page Down | Move through the focused list by viewport |
| Left / Right | Collapse/expand a focused navigation section |
| Return | Open the highlighted file in Compare from either history or the file list |
| Tab / Shift + Tab | Traverse controls and panes in visual order |
| Primary + R | Refresh local repository state |
| Primary + C | Copy selected text; explicit copy buttons handle commit hash/path |
| Primary + B | Toggle expanded repository navigation in History, when implemented |
| Primary + / | Show shortcut help, when implemented |

The graph and file list must scroll the selected row into view during keyboard navigation. Search keystrokes should not trigger list navigation. Native text selection/copy behavior takes precedence in diff content. Escape in an editor find box closes that box before a subsequent Escape returns to History. Back to history preserves the history query; it is not a Clear search action. Primary + Q retains platform quit behavior.

## Review checklist

Choose checks for the affected surface. History/comparison checks use merges, long paths, non-ASCII text, large commits, and relevant image/missing-content cases. Exercise commit → file → another file → Back, including an active search and a scrolled graph. Verify focus, Escape precedence, stale-preview rejection, pane resizing, chosen-column alignment, and clipping at regular/narrow widths in each affected theme. Project, staging, commit, branch, and network tests use disposable repositories and local remotes; check exact destinations and preserve unrelated unstaged content. Settings checks include reopening, failed persistence, and identity scope. Record the build and observed results separately; a screenshot is not performance evidence.
