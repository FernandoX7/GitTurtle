# Responsive layout and workspace chrome

Living specification for responsive chrome decay, edge alignment, narrow half-tile layouts, and Working Changes structure. Root summary: [DESIGN.md](../../DESIGN.md#one-responsive-order).

## One responsive order & in-frame sizing

- Every responsive choice (chrome mode, toolbar button label vs icon, menu collapse, footer yielding, placeholder fit) is decided **in the frame being drawn** from current container bounds (`in_frame::InFrame`), never from previous-frame scroll feedback or stale laid-out widths.
- When horizontal or vertical room tightens, surfaces degrade consistently:
  1. Multi-line supplementary notes collapse to one ellipsized line (full text in tooltip/a11y).
  2. Secondary metadata (image dimensions, timing tails on status lines) yields while remaining in tooltips and accessible descriptions.
  3. Action buttons drop text labels to `28 px` icon-only controls with explicit accessible names and tooltips (`HeaderControl`), preserving identical focus handles across transitions.
  4. Control groups fold into compact menus (`Comparison mode`, `Zoom and pan`, overflow actions) sized to their widest item so switching choices never shifts layout.
  5. Optional footers yield before primary workspace content drops below minimum readable floors (`>= 3` text rows, `>= 96 px` scaled image height, `>= 3` conflict-draft lines).

## One left edge

- Headings, search inputs, toolbars, lists, and action bars within each panel align to one vertical content edge (`PANE_INSET`).
- Transparent leading buttons and mode menus pull back by their horizontal padding so visible labels and icons align flush with headings and list text beneath them.
- Dialog sides reserve `max(16 px, dialog_side_room)` so leading ghost buttons and their `3 px` focus rings stay cleanly inside dialog bounds even in narrow `461 px` half-tiles.

## Tab strip, header, and compact chrome

- The repository tab strip is the window's top row and computes its height from snapped `28 px` tabs plus rounded-up device-pixel ring clearance above and below (`tab_strip_height`), never slicing a focused tab's outer ring at fractional scales (`1.25×`, `1.5×`, `1.75×`).
- In narrow windows, header chrome yields secondary labels by icons (keeping `Menu` labeled and compacting `Workspaces` to its icon) without creating extra stacked rows that steal vertical space from History or Compare.
- Long status messages drop optional timing tails before ellipsizing, keeping full status text in tooltips and assistive-technology nodes. Active writes replace the status text in place with their progress label and explicit `Cancel` control instead of inserting an extra header band.

## Working changes & narrow half-tiles

- The commit composer anchors at the foot of the Working Changes inspector with a fitted title placeholder (`PlaceholderFit`) and consistent `28 px`/`36 px` rhythm.
- During a write, the Working file list retains its visible rows until the post-write snapshot lands (no empty flash or shifting progress header).
- Single-row Stage/Unstage uses the `150 ms` eased flight and `5 px` still-pointer movement guard before re-arming hover, tooltip, or pointer activation on the newly arrived row control.
- Conflict-block review displays **Accept current**, **Accept incoming**, and **Accept both** above an 8-line result draft; on short/narrow half-tiles (`461 × 490`), the draft shrinks to a 3-line minimum inside a single vertically scrolling body while keeping source panels `>= 100 px` and scrolling focused controls cleanly into view with full ring clearance.
- Dialogs without explicit widths clamp to window width minus `2 × Theme::dialog_margin`, and nested dialogs reduce max height by their `16 px` layer offset.
