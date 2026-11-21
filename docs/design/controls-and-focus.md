# Controls, focus, and motion

Living specification for GitTurtle's shared controls, dialog footers, focus rings, disabled states, and motion policy. Root summary: [DESIGN.md](../../DESIGN.md#shared-control-contract).

## Shared control geometry

- Compact buttons use `28 px` height (`28 × 28 px` square for icon-only buttons with `6 px` horizontal padding; `10 px` horizontal padding and `6 px` icon/label gap for labeled buttons).
- Named Git toolbar actions use `34 px` height; Working Changes' Commit action uses `36 px` height.
- Sibling controls in a row share one height (e.g., header profile picker matches `28 px` header controls).
- Shared corner radii are `7 px` for controls and rows, `12 px` for cards, panels, and dialogs.
- Hit areas, borders, and padding stay fixed across resting, hover, pressed, and focused states.
- Explicit app text sizes, icon sizes, and icon/label gaps take precedence over toolkit size defaults; bare `.small()` does not establish the `28 px` app helper contract.

## Primary vs secondary emphasis & dialog footers

- Every surface fills at most one primary action reflecting its main workflow (History fills Push; Working Changes fills Commit while Push renders as bordered secondary in the exact same bounds via `bordered_in_place` so switching views moves zero pixels).
- Dialog footers use the shared `28 px` compact frame, never bare toolkit `26 px` dialog buttons.
- Every dialog Cancel uses the consistent `1 px` bordered secondary style (`Theme::secondary_edge`, `>= 3:1` contrast). When a dialog body already contains a filled primary button (such as Stashes' `Review restore…`), a dismissing footer button (`Close`) uses bordered secondary styling rather than a second fill.
- One glyph represents one concept across the entire app: `remote.svg` for remotes, cloud exclusively for Fetch, and the commit glyph for detached HEAD.

## Focus rings and clip clearance

- Every focused interactive control (buttons, inputs, checkboxes, switches, repository tabs, swatches) paints a `2 px` full-accent ring `1 px` outside its bounds (`Theme::button_focus_ring`), snapped to whole device pixels across `1×`, `1.25×`, `1.5×`, `1.75×`, and `2×`.
- Clipping parents (tab strip, dialog lists, project recents, Quick Open, Keyboard Shortcuts, gutters) reserve `3 px` (rounded up to whole device pixels) inside their clips and restore layout spacing via outer margins so rings are never sliced on any side.
- Adjacent controls in the tab strip keep at least one ring clearance gap apart so a focused neighbor's ring is never overpainted by an adjacent selected tab.

## Disabled and switch states

- Disabled primary, secondary, and selected buttons rest on neutral `subtle` fill with a `1 px` `Theme::disabled_edge` border (`>= 1.5:1`), dimmed label (`Theme::disabled_label`), suppressed activation, and AccessKit disabled state.
- Disabled Select, Combobox, OTP Input, and DatePicker render unfaded using full-strength disabled field tokens rather than washed-out opacity.
- Checked switches use accent foreground on their thumb (`>= 3:1`); hovering an enabled unchecked switch (or its row) lifts its track by `Palette::switch_hover` (`>= 6/255`); disabled switches fade track and thumb cleanly and show neither hover nor focus ring.

## Motion and Reduce motion

- Motion exists solely to explain user-triggered layout transitions (`120–180 ms` ease-out, zero delay/spring/bounce/overshoot). First frames always lay out final positions; transitions only animate visual offset into place and never block input.
- **Animated:** single-row Stage/Unstage travel (`150 ms`, inside list clip beneath sticky headings, skipped if off-screen or bulk-triggered), toolkit dialog entrance (`250 ms` fade/slide, drawn immediately at rest when returning to an underlying dialog), and dropdown entrance (`150 ms`, `8 px` drop).
- **Never animated:** selection, hover, focus rings, scrolling, graph/list reflow, page/mode switches, loading labels, copy feedback, or compact-chrome breakpoint transitions.
- **Reduce motion:** Follows OS reduced-motion preference by default (Settings also allows explicit On/Off). When active, all transitions settle on frame 1 with zero extra frame requests, and animated GIFs and 3D model clips pause automatically.
