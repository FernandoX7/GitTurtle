# GitTurtle design system

An original native desktop workspace for reading Git history. Prioritize legible relationships, effortless comparisons, and immediate input feedback. The visual personality is calm midnight blue with a forest tint, a crisp mint accent, and small geometric turtle details. Content fills the window; avoid dashboard cards, decorative charts, gradients, and oversized empty headers.

## Reference and provenance

GitKraken's first-party [interface guide](https://help.gitkraken.com/gitkraken-desktop/interface/) and [interface screenshot](https://help.gitkraken.com/wp-content/uploads/interface.png) establish the reference-navigation → graph → selected-commit hierarchy, adjustable panels, reference labels, and colored topology. Its [graph legend](https://help.gitkraken.com/wp-content/uploads/graph-elements.png) reinforces that commit and merge shapes should remain distinguishable. These are interaction references, not assets to reuse.

The first-party [diff guide](https://help.gitkraken.com/gitkraken-desktop/diff/) documents selecting a commit then a file, plus hunk, inline, and split presentations. GitTurtle adopts that familiar entry path and keeps the graph context available while inspecting a file. Researched September 7, 2026. All colors, sizing, icons, and proposed behavior below are original recommendations, not claims about GitKraken's implementation.

## Colors

Use these as semantic tokens; apply color consistently across tree, graph, file list, and preview.

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
- Main repository title: 14 px medium. Commit summary in the inspector: 15 px medium, wrapping. Avoid large display typography in repository views.
- Section labels: 11 px medium, restrained letter spacing, uppercase only for short headings such as LOCAL BRANCHES. No all-uppercase sentences.
- Base spacing: 4 px. Standard gaps: 4, 8, 12, 16. Standard pane padding: 12 or 16. Controls: 28–32 px high, 5 px corner radius; no pill buttons except small reference labels.
- Graph rows: 34 px. Navigation/file rows: 28–30 px. Use full-row hit areas. Keep metadata baseline-aligned and columns consistent.
- Icons: original 24-unit SVGs rendered at 16 px in rows, 18 px in controls, 22 px for the app mark. White source strokes allow GPUI tinting. Standard strokes are 1.65 units, rounded ends and joins.

## Workspace hierarchy

At a typical 1440 × 900 window, start with a 48 px repository header, a 220 px navigation pane, and a 26 px status strip. The remaining workspace has a graph above a lower inspector, separated by a draggable divider. Start with roughly 45% of available workspace height for history and 55% for the inspector. Inside the inspector, use a 250 px changed-file pane and a flexible preview. All panes use shared edges, not floating cards.

1. **Repository header:** turtle mark, repository name, path available on hover, open-repository control, compact commit search, and refresh. Search has a visible shortcut hint. Place a quiet Read only label near repository context; do not repeat it in every panel.
2. **Navigation:** All history, Local branches, Remote branches grouped by remote, and Worktrees. Each section collapses and has a subdued item count. Mark the checked-out branch with a small HEAD indicator; selecting a different branch only filters/browses. Worktree rows show the folder name and attached branch; expose the full path and detached/locked/prunable state in details or tooltip.
3. **History:** a small scope header followed by a virtualized graph/list. Columns contain topology, summary with compact reference labels, author, age, and short hash as space allows. The summary gets remaining width. Each graph node corresponds to a real commit; crossing lines should remain traceable. A merge node has a hollow center or distinct ring. Clicking a reference filters or focuses its history; it does not switch the checked-out branch.
4. **Inspector:** a compact selected-commit header gives summary, author, timestamp, hash with copy, and parent/base selection when relevant. Below it, the changed-file pane shows file type, filename, path, and explicit change status. The preview title uses a breadcrumb path and text/image mode controls. Expanded commit description can be revealed without occupying the main diff area permanently.
5. **Status:** repository load/refresh state, visible/loaded history count, current selection context, and a short keyboard hint when useful. Show elapsed timing only if it is measured. Do not invent a permanent green performance badge.

Keep pane widths and scroll positions stable between selections. Preserve the selected file path across adjacent commits when that path is changed in both; otherwise choose the first changed file. A ref filter is visibly labeled and removable. At smaller widths, collapse optional author/date columns before clipping the summary; allow hiding navigation and expanding the inspector. Remember layout per window or repository when persistence is available.

## Selection, focus, and feedback

- Selected row: accent-surface background, 2 px accent leading marker, primary text. Hover: elevated background. A selected node has a contrasting ring. Selection remains visible after focus moves to another pane.
- Keyboard focus: a visible 1 px accent outline or inset outline on the active control/pane. All controls require accessible labels; color or tooltip is insufficient as the only label.
- Input response is immediate. Set the row selection and commit/file heading before starting expensive work. Render uncached metadata, changed-file lists, diffs, highlighting, and images in separate stages.
- Never display an old commit's diff under a new commit's header. On selection, clear or explicitly mark the pending preview; late results for previous selections cannot replace the current one.
- Loading: a small local progress label only when work remains after approximately 150 ms. Skeletons, if used, occupy the exact content area and remain static. Avoid whole-window loading overlays and decorative shimmer. Keep navigation interactive and cancel obsolete requests.
- Empty repository: “No commits yet” in history, a brief description, and Open repository as the available action. Empty filter: “No commits match this search” plus Clear search. Empty commit: “No file changes against this parent.” No remote/worktree entries should show a concise empty section state, not imply failure.
- Errors stay close to their origin, with a plain explanation and retry when meaningful. Missing repository: show the path and Open repository. Unavailable blob/image: keep file navigation active. Oversized text/image: show size and a bounded preview or explicit unavailable message; do not freeze or silently truncate content.
- Refresh means reread local Git state. Describe remote-tracking references as locally available; never imply refresh fetched from a server. A status timestamp should explicitly mean last local refresh.
- Motion: optional 80–120 ms hover/focus transition. No selection animation, animated graph reflow, or spring scrolling. Honor reduced-motion preference.

## Text and image comparison

Text starts in unified diff view. Make added/deleted lines explicit with signs and background tints; keep hunk headers visually separate. Show old and new line numbers and preserve indentation. Split view shares vertical scrolling and has clear Before/After headings. Respect a horizontal scrollbar for unwrapped lines. Word wrap is a user control, not an implicit width-dependent change. Highlighting may arrive after readable plain text, without reflow.

Image comparison defaults to side-by-side Before/After on a subtle checkerboard. Show dimensions and byte sizes beside the labels, with Added/Deleted state replacing the absent image when appropriate. Fit is the default; 100%, zoom in/out, and fit controls are small and familiar. Link zoom and pan across both images. An overlay/wipe is a useful later enhancement, after side-by-side behavior is solid. Preserve aspect ratio, avoid upscaling by default, and keep checkerboard contrast below the artwork. SVG is a static image; never execute embedded content. A Git LFS pointer without available local content must be identified as such rather than presented as a broken image.

## Keyboard contract

Use `Cmd` on macOS and `Ctrl` on Linux for the primary modifier. Implemented bindings must be visible in a help menu or overlay; omit unavailable actions rather than advertising them.

| Binding | Action |
| --- | --- |
| Primary + O | Open repository |
| Primary + F | Focus commit search |
| Escape | Clear active search, dismiss transient UI, or return focus to its origin |
| Up / Down | Select previous/next row in the focused list |
| Home / End | First/last loaded row in the focused list |
| Page Up / Page Down | Move through the focused list by viewport |
| Left / Right | Collapse/expand a focused navigation section |
| Return | Inspect the selected row / focus its next pane |
| Tab / Shift + Tab | Traverse controls and panes in visual order |
| Primary + R | Refresh local repository state |
| Primary + C | Copy selected text; explicit copy buttons handle commit hash/path |
| Primary + B | Toggle repository navigation, when implemented |
| Primary + Shift + F | Expand/restore inspector, when implemented |
| Primary + / | Show shortcut help, when implemented |

The graph and file list must scroll the selected row into view during keyboard navigation. Search keystrokes should not trigger list navigation. Native text selection/copy behavior takes precedence in diff content. Primary + Q retains platform quit behavior.

## Review checklist

Validate with a real multi-branch repository, a merge, long paths, non-ASCII text, a large commit, text and images in the same commit, an added image, a deleted image, and a missing local LFS object. Inspect the live native app at regular and narrow window sizes. Verify selected state, keyboard focus, readable muted text, pane resizing, content clipping, and that rapidly changing commits never presents stale preview content. Performance observations must come from measured interactions; a smooth static screenshot is not evidence of responsiveness.
