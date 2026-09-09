# Content and layout contracts

Read this when changing or reviewing themes, geometry, content preparation, editor behavior, images or cache accounting. Source ownership is mapped in [app guidance](../AGENTS.md); visual intent is in [design](../../../DESIGN.md).

## Theme and retained layout

Use semantic appearance colors for native controls and custom content together. GPUI Kit 0.6 reads component backgrounds from resolved `ThemeTokens` and foregrounds from `ThemeColor`; after overriding colors, rebuild `theme.tokens` before `Theme::sync_base`. Updating only the app palette leaves native controls stale. Theme changes invalidate concrete editor decorations without rereading immutable content or eagerly rebuilding hidden editors.

Headers and rows share one `ColumnLayout` and horizontal offset. Align header/text/reference insets and graph node margins within allocated widths. Preserve chosen columns at narrow widths and keep the commit-message column visible. Keep `content_panels` and `history_panels` as app-owned `Entity<ResizableState>` values passed through `with_state`; stable element IDs alone do not retain sizes when page/mode changes remove a group. Observe the retained entities without constructing hidden repository views.

Interface and code text sizes are independent saved preferences. Use `appearance::ui_text` for UI labels and `ui_size` for fixed text-row heights; native rem-based controls follow the interface font. Source editors and custom gutters use `code_text` and `code_scale`. Density row heights include interface scaling. Size changes retain editor identities, selection and Find, and scale scroll offsets to preserve the logical viewport. Keep code and UI scaling separate when adding an editor, canvas gutter, or dialog. Revision, Quick Open, rebase, worktree, reflog and activity dialogs bound their scrolling body to the available window height, leaving their footer reachable at larger interface sizes. When measured Working geometry changes during painting, defer its redraw until painting finishes; unchanged bounds must not schedule another frame.

Pair file-status SVGs with a short label and semantic color; color alone must not distinguish New, Modified, Deleted, Renamed, Type or Conflict. Working rows use the status for their staged/unstaged area, with conflicts taking precedence and untracked files shown as New. Keep small controls/status icons tintable SVGs; packaged app artwork is a separate asset.

## Working composer

Size the composer from the list/composer body's laid-out available height after the header, Targets, feedback, and wrapping path/selection controls. Reserve several file rows, share a very short body between files and composer, keep the commit footer outside the scrolling fields, and let additional guidance/errors scroll without clipping the action. Header geometry must not depend on the measured body height. When bounds or density change, bring the selected working row into view using its current identity, not the previous geometry's pixel offset. Layout observers notify only on changed bounds.

## Prepared content

`worker::text_content` prepares `text::PatchPresentation` before delivery. `text::editor` and `diff_view::new` consume its shared rows, gutter width and decoration ranges without rescanning the patch. Partial controls use worker-validated changed-line identities beside the exact displayed hunk; clear stale selections when a snapshot changes. Keep literal patch text unchanged for copy/search; presentation-only numbers belong in the gutter. Preserve byte-indexed UTF-8 decorations and hunk-side numbers for empty sides, CRLF and no-newline markers.

`text_review` rebuilds optional review presentations from the retained original snapshot through the replaceable read worker. Whitespace filtering and context sizes 12/48/192 disable partial staging and explain that whole-file actions include all changes; Reset restores the original patch and captured partial IDs. Never feed review text back into staging or rebuild a filter from already filtered content. Review jobs check cancellation between bounded phases and during row preparation, stop at 4 MiB/100,000 source lines/4,096 hunks, and preserve the original Before/After bytes. Word highlighting uses a shared one-million-cell allowance and at most 256 tokens per line; long or expensive lines retain whole-line highlighting. Decorations are disjoint so Find can mask their backgrounds reliably. Previous/next change advances through prepared row identities without changing text selection.

Gutter scrolling follows the editor's actual text geometry and accumulates wheel input until the next paint; preserve horizontal offset and bounds. Patch wrapping/folding stays disabled; enabling either requires coordinated gutter layout and native checks. Check gutter and code-area scrolling after changes. Construct only the requested Diff, Split, Before or After editor. Split rows and copy spans are worker-prepared; synthetic alignment blanks never enter copied source text. Linked scroll setters apply during layout: ignore stale notifications until the requested offset is painted. Refresh mutable editors in place to preserve focus, search and viewport.

## Editor Find

Create patch/split styles through `editor_find::patch_decorations` and keep its handle through theme and mutable refreshes. The pinned renderer combines overlapping properties in hash order; collection creation order cannot guarantee Find precedence. Remove patch backgrounds only within painted Find ranges, retain source foreground/font styles, and identify the active match with an accent underline. Closing Find restores retained patch styles.

Reserve the empty Find collection without constructing an input/panel or replacing the source editor. Keep handles bounded with weak source ownership. Use the measured Find header height for gutter and split alignment.

## Images and cache accounting

Prepare render-image pixels on the worker. Before/After image sides share bounded pan state; active drags continue across app content and clear on release or preview replacement. Overlay/wipe composes existing images in one source coordinate system without copying pixels on drag. Preserve absent/error sides, original dimension ratios when sides are independently downsampled, and the distinction between source dimensions and decoded-preview zoom. Retained navigation copies exclude active drag gestures.

The preview cache counts retained source buffers, patch metadata and CPU image allocations. UI-held `Arc`s, editors and GPU textures may outlive eviction; its budget is not a total-memory cap. Changes to `Content` must keep `Content::bytes` accurate.

`rich_preview` presents independent captured Before/After metadata and bounded PDF pages prepared by the same worker. Page choices belong to retained content; Quick Open uses the source width. PDF/decoded-encoding views never acquire partial staging actions. System Quick Look receives an explicit, private, read-only copy of the captured bytes with a safe suffix; it cannot substitute the working file. Image previews retain captured bytes and optional literal SVG source for system inspection/copy, and the cache counts those buffers. See the finite [file support matrix](../../../docs/file-previews.md) for codecs, page caps, encoding and external-viewer limits.

Dialog children use the generic `rich_preview::render_comparison` helper with owned preview data and a weak owner handle. Rendering must not read or update the parent `GitTurtle` entity while its dialog layer is rendering; that reenters GPUI's active entity borrow. Parent updates belong in explicit action callbacks, and PDF page changes notify the rendering child.
