# Tokens and palettes

Living specification for GitTurtle's color tokens, 20 built-in palettes, live Omarchy theme adaptation, More contrast fitting, and custom theme editor. Root summary: [DESIGN.md](../../DESIGN.md#semantic-palette-ownership).

## Built-in palettes and system appearance

- [`crates/theme/src/palette.rs`](../../crates/theme/src/palette.rs) (bridged to GPUI in [`crates/app/src/appearance.rs`](../../crates/app/src/appearance.rs)) owns 20 built-in palettes (`Midnight` default dark, `Braden` original light with `daylight` storage ID, `Graphite`, `Tokyo Night`, `Catppuccin Mocha`, `Nord`, `Solarized Dark/Light`, `One Dark/Light`, `Rosé Pine/Dawn`, `Dracula/Alucard`, `Kanagawa Wave/Lotus`, `Porcelain`, `Sandstone`, `Deep Sea`, `Ember`).
- Upstream palette origins live in [`crates/theme/src/sources.rs`](../../crates/theme/src/sources.rs) and [`crates/app/tests/fixtures/2026-09-17-theme-palettes.md`](../../crates/app/tests/fixtures/2026-09-17-theme-palettes.md).
- **Follow system** selects `Braden` in light mode and the saved dark palette (`Midnight` fallback) in dark mode. It never overrides Linux Omarchy live theme sync.
- Applying a theme updates `ThemeTokens` and `ThemeColor` together before `Theme::sync_base`, keeping native controls, custom canvas drawing, and code editors synchronized. Appearance, density, and text sizes stay global across Git profiles.

## Surface ladder and readability floors

- **Hover vs selection invariant:** In every built-in and Omarchy palette (with More contrast on or off), `hover` stands closer to `canvas`, `panel`, and `subtle` than `selected` in OKLab lightness, ΔEOK, and luminance contrast, moving away from `canvas` (darker in light palettes, lighter in dark palettes).
- **Secondary button lift:** `hover` holds `>= 1.08:1` off `subtle` and `panel`; pressed states step at least `6/255` in one channel from `hover` on every surface.
- **Text & glyph floors:** Body text and secondary (`muted`) text hold `>= 4.5:1` on `canvas`, `panel`, `subtle`, `hover`, `selected`, hovered-selected rows, and diff tiles (`text` always strictly >= `muted`). Status glyphs and accent focus rings hold `>= 3:1` across resting and row surfaces.
- **Syntax & highlight fitting:** Keywords take `renamed`, strings `added`, numbers/constants `warning`, types `modified`, functions `hunk`, tags/properties/links `accent`, comments `muted`, variables/operators `text`. Each syntax role mixes toward `text` until reaching `4.75:1` (`4.5:1` minimum) on canvas, panel, and diff line tints. Selection, Find matches, and changed-word highlights are fitted from `selected`/`accent`/tints to stay `>= 0.04 ΔEOK` from their line while preserving `>= 4.5:1` contrast for all syntax colors.
- **Floating surfaces & edges:** Menus, popovers, tooltips, and pickers sit on the lightest page surface with a `1 px` edge holding `>= 1.4:1` (`>= 1.3:1` rendered after antialiasing), painted above shadows. Dialog backdrops dim pages by the smallest opacity (`20–60%` dark, `5–60%` light) keeping dialogs `>= 1.05:1` lighter than dimmed surfaces; dialog borders and Cancel edges hold `>= 3:1` against both the dialog and dimmed page.
- **Disabled tokens:** Disabled filled controls keep a `1 px` edge at `>= 1.5:1` off page surfaces and their own fill (`Theme::disabled_edge`). Disabled labels sit at half strength (or first step up to `75%` reaching `3:1`, capped at `<= 60%` of enabled secondary text contrast).
- **Scrollbar thumbs:** `Palette::scrollbar_thumb` holds `>= 2:1` against `canvas`, `panel`, `subtle`, and raised surfaces at rest, stepping to `>= 4.5:1` when hovered.

## More contrast transform

`Palette::with_more_contrast()` derives raised contrast at apply time without mutating saved themes:
- Steps `muted` and `line_number` toward `text` to `7:1` on resting surfaces (`4.5:1` on momentary hover surfaces).
- Steps `border` toward `text` to `3:1` on `canvas`, `panel`, `subtle`, and raised surfaces.
- Raises syntax fitting floor from `4.75:1` to `7:1`, disabled labels from `3:1` to `4.5:1` (`<= 75%` of adjusted secondary text), and popup/disabled/resting edges to `3:1`.
- Adds a `1 px` bottom rule on changed-word diff spans (omitted under existing underlines) so word diffs remain unmistakable without relying on fill tint alone.

## Custom theme editor

- Custom themes store explicit semantic tokens and show live advisory contrast diagnostics against the same readability rules without silently rewriting saved user drafts.
- The color picker palette forms one roving-focus Tab stop navigated with arrow keys; each swatch exposes its color name and a whole focus ring, returning focus cleanly to its trigger on commit or `Escape`.
- Hex input is labeled `"Hex value"`, and HSLA sliders announce their channel name, drawn numeric value, and step units.
