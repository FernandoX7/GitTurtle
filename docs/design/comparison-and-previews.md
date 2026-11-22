# Comparison and previews

Living specification for text diffs, partial line/hunk staging, the shared Find bar, image comparison, and rich document/media previews. Root summary: [DESIGN.md](../../DESIGN.md#text-and-image-comparison).

## Text comparison (Diff, Split, Before, After)

- Compare defaults to unified **Diff** for a new session and groups **Diff / Split / Before / After** in one compact mode control. Working comparisons explicitly identify **Staged** or **Unstaged** beside the path.
- Changed-line tints span the editor's entire visible row width (including blank changed lines) at every horizontal scroll offset; unchanged lines, hunk headers, and Split alignment filler rows stay untinted. Changed-word highlights stay tight to the changed characters (plus a `1 px` bottom rule under More contrast).
- `---`/`+++` headers render in medium body text; unified patches use semantic diff roles (`added`, `removed`, `hunk`) rather than raw language syntax coloring.
- Line numbers and blank Split alignment rows are never copied into clipboard text. Mode switches preserve selection, scroll viewport, and partial-staging line selections; only a newly landed working snapshot clears stale line selections.
- Build only the active editor mode; syntax highlighting arrives asynchronously without reflow.

## Partial line and hunk staging

- Line staging uses the diff editor's caret and text selection (`Space` toggles the changed line under the caret or every changed line drawn selected; selections ending at column 0 of a line exclude that line) rather than adding hundreds of per-line Tab stops.
- A `2 px` leading accent mark tracks the caret's row checkbox while the diff holds focus. Every toggle announces cleanly via a polite live region, and the action bar (`Stage N lines` / `Unstage N lines` + `Clear`) sits immediately after the diff in Tab order and returns focus to the diff when activated.
- Partial writes update the diff in place, preserving caret row (or nearest remaining changed line), scroll offset, and focus without flashing `"Choose a file"` or dropping focus to the window frame.
- Pointer presses on `Stage hunk`, `Unstage hunk`, or checkboxes bind strictly to the diff revision drawn in that frame: if a new diff lands before release or on the second press of a double-click, the stale press does nothing.
- Narrow bars collapse `Clear` to a `28 px` close-icon button before wrapping, and scroll the diff just enough so the action bar never covers the caret line.

## Shared Find bar

- Pressing Find (`Cmd+F`/`Ctrl+F`) anywhere in Compare opens Find on the active text comparison (never leaving Compare for History search when viewing images, PDFs, or notices). Opening or refocusing selects the full query anchored from start so the caret stays visible in narrow fields.
- The bar spans full comparison width aligned to `PANE_INSET` using `28 px` compact controls. In **Split**, one shared bar with one query spans both sides' heading row, indicating the searched side via a `1 px` accent bottom rule on that half and contextual placeholder (`"Find in Before"` / `"Find in After"`).
- Responsive decay keeps one row as long as the field's 8-digit floor plus controls fit, collapsing Previous/Next labels to chevrons, Match case to `"Aa"` checkbox, and Close to its icon before wrapping whole controls onto a second row.
- `Escape` from the field or any Find control closes Find and restores focus to the searched editor. Compare's footer yields while Find is open if needed to keep `>= 3` whole text rows visible.

## Image comparison

- Defaults to side-by-side on a quiet checkerboard with Fit zoom and linked pan/zoom; supports **Overlay** (opacity) and **Wipe** (divider) with keyboard controls.
- Absent sides clearly state both absence and cause (`"No image on this side"` + `"The file was added"`/`"deleted"`, collapsing cleanly to `"No image: file added"`/`"deleted"` in tight columns with full text in tooltips/a11y).
- Mode, Zoom, and Pan toolbars size menus to their widest option on whole device pixels so changing a zoom or mode never reflows rows or pushes captions down.
- Labels yield in deterministic order (`notes` second lines -> dimensions -> Compare footer -> notes) before shrinking the image area below its `96 px` scaled minimum, keeping hidden metadata in accessible descriptions and tooltips.

## Documents, media, and exact source

- PDF pages, Markdown/Mermaid diagrams, animated GIFs, and 3D/CAD models (GLB, STEP, STL, OBJ, 3MF) always keep exact literal source inspectable alongside rendered previews.
- All decoding and rendering stay off the UI thread with independent byte, dimension, page, frame, and memory bounds (using the seccomp-confined helper on Linux for PDF, AVIF, and JPEG 2000).
- Hidden tabs, closed views, and Reduce motion pause GIF playback and 3D animation immediately and release GPU atlas textures once owners drop.
- See [docs/file-previews.md](../file-previews.md) and [docs/document-previews.md](../document-previews.md) for format support and resource caps.
