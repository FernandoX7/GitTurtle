# Custom themes — specification (September 17, 2026)

Status: implemented. All 13 tasks in [`tasks.json`](tasks.json) landed on `claude/themes` (pull request #23), and its follow-ups are done or moved to the development backlog ([`../tasks.json`](../tasks.json), [follow-ups.md](follow-ups.md)). Palette selection and provenance: [the research note](../2026-09-17-theme-palettes.md). Design intent: [`DESIGN.md` §Semantic palette ownership](../../../DESIGN.md#semantic-palette-ownership). The [architecture review](../architecture-review.md) applies: this changes a shared interface (`ThemeChoice` becomes one case of `ThemeSelection`), a persistent store (preferences version 5 → 6) and a hot path (theme application).

## Observable outcome

In Settings the user picks any built-in theme as a base, edits the 21 semantic tokens with a live preview, names the theme, saves it, and chooses it from the same picker as the built-ins. A custom theme can be exported to a JSON file, imported from one, and deleted. Every theme, built-in or custom, is judged by one readability rule set: built-ins must pass it in tests; custom themes show its findings as warnings. Custom themes persist in the preference store and survive restarts, profile changes and unrelated preference saves. Switching themes re-colors the window without re-preparing content or dropping a frame.

## Owners

| Concern | Owner |
| --- | --- |
| Semantic tokens (`Palette`), built-ins (`ThemeChoice`), upstream values, readability rules, custom model and document format | `crates/app/src/appearance.rs` with submodules `appearance/sources.rs` (values from the research note) and `appearance/custom.rs` (`TokenKind`, `CustomTheme`, `ThemeSelection`, `ResolvedTheme`, `readability_issues`, export/import document) |
| Store version 6, `custom_themes` section, `save_custom_themes`, `resolved_theme` | `crates/app/src/preferences.rs` |
| Picker, "Your themes" card | `crates/app/src/settings.rs` |
| Editor form state, import and export flows | new `crates/app/src/theme_editor.rs`, a feature `State` owner like `profiles.rs`; `GitTurtle` composes it |
| The single application path (`apply_appearance` → `Palette::apply` → `configure` → `Theme::sync_base` → decoration refresh) | `crates/app/src/platform_polish.rs`, `appearance.rs` |
| Lane color set selection | `crates/app/src/graph.rs` via `Palette::is_light` |
| The Omarchy theme (Linux): reading, parsing, mapping, fitting and following the desktop's theme | `crates/app/src/appearance/omarchy.rs` (read, state, watch) and `appearance/omarchy/palette.rs` (parse, map, fit); see [Omarchy theme](#omarchy-theme) |

No new crates. Nothing here touches `git-core` or `preview`.

## Data model

```rust
pub struct Palette { /* unchanged: 21 u32 tokens */ }
pub enum TokenKind { Canvas, Panel, Subtle, Hover, Border, Selected,          // Surfaces
                     Text, Muted, LineNumber,                                  // Text
                     Accent, AccentForeground, AccentHover, AccentActive,      // Accent
                     Added, Removed, Modified, Renamed, Warning,               // Status
                     AddedBackground, RemovedBackground, Hunk }                // Diff
// TokenKind::ALL is in this order; each has label(), description(), group().
pub struct CustomTheme { pub id: u32, pub name: String, pub base: ThemeChoice, pub palette: Palette }
pub enum ThemeSelection { BuiltIn(ThemeChoice), Custom(u32) }
pub struct ResolvedTheme { pub selection: ThemeSelection, pub palette: Palette, pub is_light: bool }
```

- `Palette::is_light()` uses the rule `graph::palette_colors` already applies (canvas brighter than text); `graph.rs` calls it, and a test asserts every built-in's explicit `is_light` agrees.
- `Palette::apply(is_light, window, cx)` is the one application path; `ThemeChoice::apply` delegates to it.
- Bounds: at most 32 custom themes per store; ids are unique `u32`, next id = max + 1 (the project-group convention); names follow the project-name rules (`preferences::validate_project_name`: one line, 1–128 bytes after trimming) and are additionally unique case-insensitively among custom themes and never equal to a built-in label.
- `ThemeSelection` serde: a built-in serializes as today's bare string (`"nord"`); a custom theme as `{"custom": 7}`. An older app reading a custom selection through `#[serde(other)]` would see Midnight, but it refuses version 6 before that matters.

## Readability rules

`Palette::readability_issues() -> Vec<ReadabilityIssue>` where an issue names the foreground (a `TokenKind` or graph lane index), the background (a `TokenKind` or the selected-row hover blend), what the rule measures (a contrast ratio, or the pressed-step rule's channel step), the measured value and the minimum. The contrast function moves out of the test module. Rules, with the six surfaces being canvas, panel, subtle, hover, selected and `row_hover(true)`:

| Foreground | Background | Minimum | Why |
| --- | --- | --- | --- |
| text, muted | six surfaces; added/removed backgrounds | 4.5 | body and secondary text (existing) |
| added, removed, modified, renamed | six surfaces | 3.0 | status icons (existing) |
| added / removed | added / removed background | 4.5 | diff content (existing) |
| accent_foreground | accent, accent_hover, accent_active | 4.5 | primary button label (existing) |
| accent | six surfaces | 3.0 | focus ring, caret, list active border (new) |
| each of the six lane colors of the palette's set | canvas, panel, selected, hover | 3.0 | graph lanes stay identifiable on every row state (new) |
| selected vs panel; hover vs panel; border vs panel | — | 1.15; 1.08; 1.3 | selection, hover and dividers stay distinguishable surfaces (new) |
| line_number | canvas, panel | 4.5 | gutter and inspector coordinates (new) |
| hunk | canvas, panel, subtle / hover, selected, row hover | 4.5 / 3.0 | links, info and hunk headers (new) |
| canvas | added, removed, warning, hunk | 4.5 | `configure` uses canvas as the success/danger/warning/info foreground (new) |
| warning | subtle | 4.5 | failure messages in the warning color on grouped cards, such as Your themes (added 2026-09-23) |
| modified, renamed | added/removed backgrounds | 3.0 | status icons inside diff tiles (new) |
| warning | six surfaces | 3.0 | conflict and warning glyphs (new) |
| selected, as the shared button's pressed layer | hover, as its hover layer, both composited over each of the six surfaces | a step of 6 in one channel | a pressed button stays distinct from its hover (added 2026-09-27) |

All ten current palettes pass every row with margin (computed on 2026-09-17 from `appearance.rs` and `graph.rs`: lowest values are Nord hover/panel 1.10, Nord selected/panel 1.20, Daylight line_number 4.53). Built-in tests assert the list is empty for `ThemeChoice::ALL`. For custom themes the list is advisory.

The accent row measures the token, which a focus ring reaches only where it paints the token at full opacity. The toolkit's ring is 3 px of accent at half opacity (2.23:1 on Porcelain's subtle surface), so the shared `button` helper adds a 1 px band of full accent inside its edge (`appearance::control_focus_edge`). `shared_button_focus_ring_clears_the_graphic_rule_in_every_state` composites what a focused helper paints and requires 3.0 on the six surfaces at rest, selected and selected-and-hovered, and on canvas, panel and subtle while hovered or pressed, holding those two states to 2.5 on the row surfaces (added 2026-09-27). Primary and danger variants are outside it: the band lies on their own fill, and their focus stays the toolkit ring.

The warning-message row was added on 2026-09-23, when five built-ins were below it (Kanagawa Lotus 3.95, Rosé Pine Dawn 4.25, One Light 4.27, Solarized Dark 4.34, Solarized Light 4.38); their tuned warning colors now clear it by the 0.25 margin [`DESIGN.md`](../../../DESIGN.md#semantic-palette-ownership) sets for tuned tokens, and [`contrast-margins.md`](contrast-margins.md) records the values. A saved custom theme copied from one of those five before the change keeps its old warning color and now reports one more warning.

The pressed-step row was added on 2026-09-27 (`themes-press-distinct`). The shared button paints its hover and pressed states as the `hover` and `selected` layers of `Palette::control_fill`, so pressed stands only as far from hover as those layers do. The rule composites both layers over each of the six surfaces, takes the largest difference in one 8-bit channel on each, and asks the smallest of the six to be at least 6; the warning reads "Selected against Hover differs by 3 of 255 in one channel, needs 6". It measures a channel step rather than a contrast ratio because a change of hue at the same lightness is a visible press: Kanagawa Lotus's `selected` is 52 apart from its `hover` at almost the same luminance (1.314:1 and 1.321:1 on its panel).

The task proposed a step of 8 and a second clause, `selected` at least as far from `panel` as `hover` in the lift direction. Measured on the twenty built-ins with this rule's composited measure, 8 would also retune Catppuccin Mocha and Nord, which hold 6 over their selected rows, and Deep Sea, which holds 7 (between the bare tokens they are 7, 7 and 8); the lift clause would reject Kanagawa Lotus and Kanagawa Wave, whose pressed layers are at least 50 and 17 apart on every surface. For the built-ins, the shared button test already asserts that the pressed layer lifts every surface in hover's direction by at least 1.12:1, so a pressed state cannot fall back toward the surface. A custom theme is not checked for direction: one whose `selected` sits on the other side of `panel` from `hover` gets no warning as long as the step and the 1.15:1 selected-surface row hold, which keeps an inset selection a legitimate choice. The rule is therefore the largest step every palette outside the two tuned ones keeps, with no lift clause:

- Sandstone was 3 apart on every surface and Porcelain 6 over its panel but 4 over the hovered selected row. Both original palettes now lean `selected` toward their accent's hue, Sandstone `#EDDFD0` → `#F2DCD0` (terracotta) and Porcelain `#DFE6F6` → `#DCE6F6` (sapphire), which puts pressed at least 8 from hover on every surface, the proposal's value. Their selected rows still lift further than hover (1.288:1 and 1.258:1 on the panel, against 1.265:1 and 1.214:1) and their text keeps its margins; the lowest is Porcelain's secondary text on the hovered selected row, 4.89:1, from 4.92:1.
- Kanagawa Wave meets the rule unchanged and is not exempted. Its `selected`, upstream waveBlue1 lightened only as far as the panel rule, lifts less than `hover` (1.153:1 against 1.194:1 on the panel), so its pressed button steps by hue rather than by a further lift; tuning it lighter would move a token the rule does not require. `appearance::custom::tests::pressed_buttons_stay_apart_from_hover_on_every_surface` records this beside the two shipped values it rejects.

A saved custom theme copied from Sandstone or Porcelain before the change keeps its old `selected` and now reports one more warning. The rule does not cover the margin limit found beside it: Solarized Dark's button label is 4.575:1 on the pressed layer over the hovered selected row, above 4.5 but inside the 0.25 margin, and reaching the margin would move its label off `text`.

## Store migration (version 5 → 6)

- `StoredPreferences` gains `custom_themes: Vec<StoredCustomTheme>` (default empty) beside `project_library`; `AppSettings::theme` becomes `ThemeSelection`. A version-6 file whose selection is a built-in is byte-compatible with version 5 for the `settings` object.
- Loading accepts `1..=6`. Files below 6 load with an empty custom section; loading never writes. The custom section is user-authored data: a malformed token, duplicate id, duplicate name, invalid name or more than 32 entries makes `load_from` fail, so later saves refuse rather than drop the section — the project-name policy. A selection naming a missing custom id resolves to the default theme without rewriting the file, and the picker shows the default as selected.
- Saving writes `"version": 6`. `Preferences::save_custom_themes(&[CustomTheme])` rereads the disk store, replaces the section and saves atomically on the preference executor; `save_settings` keeps carrying the disk's custom section. An older app reading a version-6 file refuses it, starts with defaults and never overwrites it — the existing unsupported-version contract.
- `AppSettings::resolved_theme(appearance, custom_themes) -> ResolvedTheme`. Not following the system: the selection's palette. Following the system: Light appearance selects Braden; Dark appearance keeps the selection when its palette is dark (built-in or custom) and otherwise Midnight — the existing rule, applied to custom palettes through `is_light`.
- Tests: a complete version-5 fixture (theme, recents, drafts, names, library) loads identically with no customs and no write; a version-6 fixture with two customs and a custom selection round-trips byte-for-byte; each refusal above leaves the original bytes; a missing custom id resolves without a write; the existing `saved_daylight_remains_braden…` test still sees `"daylight"` as a bare string.

## Editor (visual contract)

Settings gains a **Your themes** card directly under the theme picker, using the existing grouped-surface card style: a title, a one-line description, a list of custom themes (30 px rows: a five-token swatch strip — canvas, panel, accent, added, removed — the name in primary text, the base name in muted text, a warning glyph with a count when `readability_issues` is non-empty) with compact Edit…, Export… and Delete… actions, and **New theme…** and **Import…** buttons using the shared 28 px compact `button` helper. Actions carry specific accessible names ("Edit Sunset theme"). The list is virtualized (`uniform_list`, as History) and is `min(rows, 8) × 30 px` tall: with up to eight themes it is exactly the stacked rows, with more it scrolls inside the card with the toolkit scrollbar, every row's actions stay tab stops in order, and the row holding focus is scrolled into view (owner decision of 2026-09-20 from the `themes-draw-cost` diagnosis: 32 unvirtualized rows cost 6 to 8.7 ms of every Settings and dialog frame).

The editor is an alert dialog like the profile editor, titled **New theme** or **Edit theme**:

- Header row: **Name** input; **Base** dropdown listing the twenty built-ins by label (changing the base after edits asks before replacing the draft tokens).
- Body, two columns at ≥ 1,060 px window width, stacked below that: left, the token groups Surfaces, Text, Accent, Status, Diff, each with a muted 12 px group label and rows of `label · description · 16 px swatch · hex Input · ColorPicker`; right, the preview card (`theme_preview` generalized to a `Palette`) above the warnings list. Row height 30 px (the navigator geometry); the hex field is monospace, seven characters wide, accepts `#rrggbb` or `rrggbb` in either case.
- Footer: **Reset to base** (secondary), **Cancel** (secondary), **Save** (primary). The body scrolls within the window height so the footer stays reachable at large text sizes.
- Live preview: every valid edit applies the draft through `Palette::apply`, coalesced to one application per frame with `on_next_frame`. The whole window, including the dialog, follows the draft. Cancel, Escape, or closing the dialog re-applies the saved selection.
- Warnings: recomputed on each valid edit; each line names both tokens, the ratio and the minimum ("Muted text on Selected 3.9:1, needs 4.5:1"; the pressed-step rule gives its step instead, "Selected against Hover differs by 3 of 255 in one channel, needs 6"), and the affected row shows a warning glyph. Warnings never block Save; a saved theme with warnings keeps the glyph on its list row and picker card.
- Validation: an invalid hex value marks the field, keeps the last valid draft and disables Save until every field is valid; a name that breaks `validate_theme_name` shows its message beside the field and disables Save.
- Keyboard: focus is contained; order is Name, Base, rows in group order, footer; Escape cancels; Return in a hex field commits it.
- Delete asks for confirmation naming the theme. Deleting the active theme selects and applies its base.

## Import and export

- Export uses `prompt_for_new_path` with the suggested name `<slug>.gitturtle-theme.json`, writes on a background executor with temp-and-rename, and reports the written path inline. Import uses `prompt_for_paths` (files only, single), reads with the bounded store reader (regular file, no final-component symlink, at most 64 KiB), parses and validates off the UI thread, then saves through `save_custom_themes`. The imported theme is added and highlighted in the list, not applied.
- Document (exact keys, no others; `tokens` has exactly the 21 `TokenKind` names in snake_case):

```json
{"format": "gitturtle-theme", "version": 1, "name": "Sunset", "base": "solarized_dark",
 "tokens": {"canvas": "#002b36", "panel": "#073642", "...": "..."}}
```

- Rules: `format` must be `gitturtle-theme`; `version` 1 (a greater version reports "exported by a newer GitTurtle"); an unknown top-level or token key, a missing token, a non-hex value or a document over 64 KiB is refused with a message naming the problem; a `base` the app does not know becomes Midnight or Braden by `is_light` with a notice; a name collision becomes "<name> (imported)", then "(2)", "(3)"; the 32-theme bound refuses the import with the bound. Cancelling either dialog is quiet; a Linux portal failure gets the folder picker's guidance.

## Failure and recovery

| Situation | Behavior |
| --- | --- |
| Save fails (I/O, refused store) | Dialog stays open with the error beside Save; the draft and the applied preview are kept; retry allowed. |
| Store refuses because the custom section is malformed | Every preference save reports it (existing policy); the message names the store and asks the user to repair or remove it. |
| Selection points at a deleted or missing custom theme | Default theme applies; the file is not rewritten until the user chooses a theme. |
| Import file invalid | Message in the card; nothing saved. |
| Export destination unwritable | Error inline; no partial file (temp-and-rename). |
| Editor open while a preference save from elsewhere lands | The editor keeps its draft; the list re-reads after its own save only (pending-save count, as the project pane does). |
| Window closes with the editor open | The saved selection is re-applied on the next launch because nothing unsaved reached the store. |

## Performance

Hot path: `apply_appearance` (theme switch from the picker, and every live-preview edit). Budget, measured from the switch or edit handler to the next frame callback with a new `gitturtle.theme_apply_frame_ms` trace under `GITTURTLE_TRACE`: median ≤ 8 ms, p95 ≤ 16 ms (one 60 Hz frame), zero worker submissions, editor entity and Find state retained. Release build, fixture: the `scripts/create-demo-repo.py` repository plus one page of at least 1,000 commits loaded and a split comparison of a 2,000-line file retained; 60 switches cycling every built-in and two custom themes; record host, cache state and raw samples under `docs/benchmarks/`. Import parsing is bounded by the 64 KiB limit and runs off the UI thread; store saves are unchanged in cost.

The two paths end at different frames. A switch applies inside its handler, so its trace ends at the next frame callback, before that frame is drawn ([baseline](../../benchmarks/2026-09-18-theme-apply.md)); that value is dominated by the wait for the platform frame tick and cannot see draw cost, so UI-thread CPU per switch is the switch number that moves with the code, and the callback bound stands as a non-regression. An early editor build measured edits at median 45 ms and p95 56.6 ms, dominated by a whole-window draw of Settings plus the dialog (about 20 ms at 2x). By owner decision on 2026-09-19, `themes-editor` records the edit cost without gating on it, and `themes-draw-cost` owns the edit budget in the application path.

By owner decision on 2026-09-20, after the diagnosis of candidate `58b6226` in run `20260919T143547Z-8e458d6a`, the edit budget is re-stated at a new boundary because the old one could not be met: the edit trace ended at a second next-frame callback, callbacks run only on the platform frame tick (an 8.334 ms grid on the 120 Hz Linux X11 host), so the value was at least the keystroke's phase plus one period even for a free frame, and a ten-sample p95 could pass 16 ms with probability 0.43 at zero cost. The edit metric is now `gitturtle.theme_edit_frame_ms`: from entry to the token row's change handler, before the value is parsed, to the paint of a trace element deferred above the dialog layer so that it is the last element `Window::draw` paints in the first frame drawn after the handler applied the draft, on either draw path (frame tick or the synchronous draw in `Window::dispatch_key_event`). It contains the parse, readability recompute, `apply_appearance` and the whole window's layout, prepaint and paint; it excludes callback waits, frame-finish bookkeeping, present (0.95 ms measured, no vsync block) and input delivery. Budget (owner decision of 2026-09-20, re-cut to 26 ms on 2026-09-21): p95 ≤ 26 ms over 60 pooled edits in one recorded launch designated in writing before it runs, the p95 being the 57th of the 60 sorted samples rather than a ten-sample nearest rank, which is that set's maximum; the first dialog's ten edits are still reported as a designated set but carry no bound. Measured on a store seeded with 32 custom themes (the bound) with the editor opened by Edit… on the first row; `themes-draw-cost` owns it. The measured draw of that frame on `58b6226` was 19.5 ms on the empty store and 26.4 ms with 32 themes (request_layout 10.0, taffy 11.0, prepaint 2.4, paint 3.3); the realistic floor after virtualizing the rows and cheapening the page's layout is 15 to 19 ms of draw, plus about 2 ms of handler and apply. What the live preview repaints is unchanged: the whole window follows the draft, and GPUI has no recolour-only path. Re-cut on 2026-09-21: candidates `ea31261`, `2030b3d` (indicative) and `23ee777` measured 26.348, 24.440 and 25.191 ms with about 3 ms of launch-to-launch spread, and the remaining elements are the editor dialog's kit `Input` and `ColorPicker` internals (674 of 1,882 elements), so the owner set the bound at 26 ms rather than spend further attempts.

## Omarchy theme

Added 2026-09-28 (item D, owner decisions of 2026-09-27 and 2026-09-28). On Linux, when `$HOME/.local/state/omarchy/current/theme/colors.toml` is a regular file, the picker offers an **Omarchy** card in its own one-card **Desktop** group above the light and dark palettes. Choosing it applies a palette mapped from the desktop's current Omarchy theme, and the palette follows each Omarchy theme switch while the card is selected. Elsewhere, and on a Linux desktop without that file, the card is absent and nothing is read.

**Selection and store.** `ThemeSelection::Omarchy` is stored as the bare string `"omarchy"`, and the store stays at version 6. A build without the variant reads the string through `ThemeChoice`'s `#[serde(other)]` as Midnight and keeps every other setting. Follow system never overrides an Omarchy selection (`AppSettings::resolved_theme`). While it is selected, the Follow system switch shows off and disabled, described "Omarchy follows your desktop theme", and choosing any other card releases it. Other platforms keep the stored selection but resolve it to the default theme, and no card shows.

**Reading.** `colors.toml` is followed as a regular file. Its metadata is checked before and after a nonblocking open, so a directory, FIFO or device is refused without blocking. At most 16 KiB plus one byte is read, and a larger file is refused. `theme.name` is read the same way up to 1 KiB, trimmed, title-cased on `-`, `_` and spaces ("tokyo-night" becomes "Tokyo Night") and bounded to 64 bytes on a character boundary. A missing or unusable name leaves the caption "Follows your Omarchy theme". Every read runs on the background executor, and nothing is written or spawned.

**Parser.** The parser reads flat `key = "#rrggbb"` lines. It accepts blank lines, `#` comments, either hex case, optional spaces around `=`, a trailing comment, single-quoted strings and `mode = "light"|"dark"` in any case. It ignores unknown keys, values that are not a `#rrggbb` string (such as `hyprland_active_border = "rgba(…) rgba(…) 45deg"`) and every key after a `[table]` header, because TOML places those keys in the table. It refuses input over 16 KiB, input that is not UTF-8 (a leading BOM is allowed), and input missing a required key: `background`, `foreground`, `accent`, `red`, `green`, `yellow`, `blue` or `magenta`. A required key with a malformed value counts as missing, so the reason reads "colors.toml is missing a background color". The mapping also reads `selection`, `muted`, `dark_background`, `lighter_background`, `dark_foreground` and `orange` when present. Other keys (`darker_background`, `light_foreground`, `bright_foreground`, `cyan`, `brown` and the `bright_*` keys) are accepted and not used. Without a usable `mode`, a background that contrasts more with black than with white is light.

**Mapping.** The mapping follows the plan the owner saw:

| Token | Source (fallback) | Fit |
| --- | --- | --- |
| canvas | `background` | within the lane limit |
| panel | `dark_background` (canvas 12% toward black) | within the lane limit with room for a 1.3:1 step |
| subtle | `lighter_background` (canvas 6% toward `foreground`) | within the lane limit |
| selected | `selection` (canvas 25% toward `accent`) | within the lane limit, then 1.15:1 off the panel on the foreground side |
| hover | midpoint of canvas and selected | within the lane limit, then 1.08:1 off the panel |
| border | canvas 70% toward `muted` (30% toward `foreground`) | 1.3:1 off the panel |
| accent | `accent` | 3.0:1 on the six surfaces, the hovered selected row computed with the candidate |
| accent foreground | canvas, else black or white | 4.75:1 on the accent, preferring canvas, then the extreme on canvas's side. When none reads, the accent moves away from the surfaces until the canvas-side extreme does |
| accent hover / active | the accent's HSL lightness 0.06 away from the label / 0.06 (or 0.03) toward it | active falls back to 0.12 away when the label would drop below 4.75:1 |
| added / removed | `green` / `red`, or GitTurtle's own (below) | status rules, plus 4.5:1 on their own diff tile |
| modified / renamed | `yellow` / `magenta` | 3.0:1 on the six surfaces and both diff tiles |
| warning | `orange` (`yellow`) | status rules, plus 4.5:1 on subtle |
| hunk | `blue` | 4.5:1 on canvas, panel and subtle; 3.0:1 on hover, selected and the hovered selected row |
| added / removed background | canvas tinted 14% (dark) or 10% (light) toward added / removed | within the lane limit |
| text | `foreground` | 4.5:1 on the six surfaces and both diff tiles |
| muted, line number | `dark_foreground` (`foreground` 40% toward canvas), blended toward text in 64 steps | as text (line numbers: canvas and panel only); muted never reads above text |

The status rules are 3.0:1 on the six surfaces and 4.5:1 against canvas, because `configure` paints canvas on status fills. "On the foreground side" means lighter than the surface in a dark palette and darker in a light one, and every contrast check in the fit requires that side.

**Fitter.** No built-in needed one: the built-ins were tuned by hand. `palette::fit` moves a token only along its HSL lightness toward the mode's extreme, in 256 steps, keeping hue and saturation. Foregrounds move toward white in a dark palette and black in a light one; surfaces move the other way. The fit stops at the first value that passes, and a token that already passes keeps the theme's value. A tuned token aims for its rule plus the 0.25 margin of [`DESIGN.md`](../../../DESIGN.md#semantic-palette-ownership) (0.02 for the surface steps) where that is reachable, and for the rule itself otherwise. The lane limit keeps every row surface at 3.1:1 against each lane of the mode's graph set (`graph::lane_colors`): at most luminance 0.108 in a dark palette and at least 0.501 in a light one, which also leaves black or white text above 4.5:1. The pressed-step rule is met last. While `Palette::pressed_step` is below 8 (the rule's 6 plus a margin), the fit lifts `selected` by 1/32 of its remaining lightness within the lane limit, or, once that is blocked, brings `hover` 1/32 back toward the panel while it keeps 1.10:1, refitting the accent each time, for at most 128 steps. Finally the palette must have no `readability_issues`, or the mode's default built-in replaces it whole: Braden for light and Midnight for dark. Each extreme passes its foreground rules against surfaces within the lane limit. All 22 bundled Omarchy themes fit with no findings and never reach the fallback. In the seeded property test, 600 arbitrary themes (a fifth with foreground equal to background, a fifth with red and green one step apart) all map with no findings, and the fit alone must carry at least 97% of them. A 2,000-theme exploration of the same fit, run outside the suite during development, measured 99.75%, and every miss there was the pressed step.

**Diff colors.** Owner decision of 2026-09-27: diffs keep the theme's red and green where they are distinguishable, and otherwise use GitTurtle's own added and removed hues for the mode (Braden's `#146744` and `#B53351` in light, Midnight's `#75E0BB` and `#FF95A8` in dark), fitted to the theme's surfaces. Distinguishable means that, after fitting, both colors keep at least 0.05 OKLCH chroma and their OKLCH hues are at least 60° apart. Among the bundled themes, ethereal, hackerman, last-horizon, lumon, lupine, solitude, vantablack and white take GitTurtle's hues: their greens are grey, green-on-green, blue-on-blue or purple. Miasma's muted red and green become distinguishable once fitted, so it keeps its own. `palette::tests::indistinguishable_red_and_green_mark_diffs_in_gitturtle_hues` covers Hackerman, White, Vantablack, Lumon and a synthetic theme whose red and green are one step apart.

**Fallback and status.** A missing, unreadable, oversized or malformed file never panics or blocks the UI. The last good palette stays for the session, and without one the default theme (Midnight) applies. The card caption says why: "Unavailable: colors.toml is missing. Using Midnight." or "… Keeping Tokyo Night." once a good palette was read. The card is offered while the file is a regular file, even a malformed one, and stays while the Omarchy theme is selected, so a selection whose file is gone remains visible with its reason. Before the first read finishes it says "Reading your Omarchy theme…".

**Live switching.** `omarchy-theme-set` removes `current/theme`, moves `next-theme` into place and only then writes `current/theme.name`, while `current/background` changes separately. While the Omarchy theme is selected, `current/` is watched without recursion through `notify`, with the watcher created on the background executor. Only events on `theme` or `theme.name` (or a watcher error or rescan) request a reread. `background`, `next-theme` and access events do not. A request waits for a quiet period of 250 ms on the GPUI executor's timer, capped at 2 s, and every request queued meanwhile joins it, so a switch is read once, after its writes settle. Rereads also run at launch, on focus regain (beside the desktop font lookup) and when the theme is selected, and a watch that could not start is retried on focus regain. Choosing another theme drops the watcher. Rereads run one at a time and each carries a generation: a result older than the latest one started is dropped. `appearance::omarchy::Omarchy` is the one owner of availability, name, last good palette and status. The card, the Follow system switch and `apply_appearance` read it, and it changes, notifying its observers, only when what it holds changes. A reread that finds the same theme therefore applies nothing. A change applies through `apply_appearance` unless the theme editor is previewing a draft, whose close applies the saved selection. Launch waits at most 200 ms for the first read, so the first frame can already show the theme.

**Evidence.** Tests in `appearance::omarchy::palette::tests` cover the 22 fixtures under `crates/app/tests/fixtures/omarchy/` (MIT, copied from Omarchy 4.0.4), the property test, the diff-color rule and the parser. `appearance::omarchy::tests` covers reading, names, captions, stale rereads, and the live watcher against a temporary `current/` driven as `omarchy-theme-set` drives it, with one application per switch and none for a wallpaper change or an unchanged rewrite. `settings::picker_tests::the_omarchy_card_selects_and_locks_follow_system` covers the card and the Follow system switch, and `preferences::tests` covers the store and resolution. Two traces measure the path: `gitturtle.omarchy_reread_ms` and `gitturtle.omarchy_apply_frame_ms` ([metrics](../../benchmarks/metrics.md)).

## Evidence map

| Part | Proof |
| --- | --- |
| Readability rules | Unit tests per rule with a deliberately failing palette; `appearance::` tests asserting no issues for `ThemeChoice::ALL` |
| Model and document format | Unit tests: token round trip, document round trip, each refusal, name rules, selection serde |
| Store migration | Version-5 and version-6 fixture tests, refusal tests, byte-identical round trip, merge test with a concurrent settings change |
| Editor | `#[gpui::test]` flow (new, edit, preview applied, save, cancel restores, delete); native-qa evidence in a light and a dark base at 1,000 × 680 and 1,440 × 900 with keyboard-only operation; design-reviewer check against `DESIGN.md` |
| Import/export | Round-trip and refusal tests; native-qa export/import/malformed-file evidence on the affected platform |
| Picker | `#[gpui::test]` for groups, labels, selection and geometry via `debug_bounds`; native evidence in both densities |
| Performance | Trace metric, no-worker-job test, dated release benchmark record attested by the performance-reviewer |

## Open questions and what would change the plan

- Whether GPUI Kit 0.6's `ColorPicker` supports keyboard operation and hex entry; if not, the hex `Input` is the primary control and the picker is pointer-only, which the editor task records. Settled by the editor task: the hex `Input` is the primary control, and the toolkit picker is a pointer-first popover that follows each row's hex field in the Tab order ([`DESIGN.md` §Custom theme editor](../../../DESIGN.md#custom-theme-editor), Keyboard).
- Whether `prompt_for_new_path` works through the Linux FileChooser portal on the validation desktop; if not, export needs the same guidance path as the folder picker and the native evidence for export comes from macOS. Settled on 2026-09-19: on the validation desktop `prompt_for_new_path` and `prompt_for_paths` reach the GNOME FileChooser portal, recorded as D-Bus transcripts because its dialogs cannot be captured, and with no FileChooser service both actions report the folder picker's guidance and change nothing ([validation entry](../../validation.md#september-19-theme-export-and-import)).
- If a chosen palette cannot satisfy the rules without losing its character, the batch task swaps it for an eligible candidate from the research note and says so.
- If the apply budget is missed because `Theme::sync_base` or a full-window repaint dominates, the fix belongs in the application path (skip unchanged tokens, avoid rebuilding decorations that did not change), never in the budget. Resolved in part on 2026-09-20: the edit budget's boundary, not its number, was the unmeetable element (see Performance); the bound at the new boundary is the owner's.
- Whether the Settings page's layout pass (taffy, 6.2 ms for the bare page with no dialog and no saved themes) can be made cheaper without patching GPUI; the diagnosis could not split it per subtree. If it cannot, the edit floor stays near the empty-store draw and only `themes-settings-view` (the page as a cached view) improves what typing in the dialog costs. Settled by `themes-settings-view` without patching GPUI: a keystroke in the dialog now rebuilds its token row, not the dialog and the page, and the UI-thread CPU per keystroke fell from a median of 20.406 ms to 7.191 ms ([the 2026-09-23 record](../../benchmarks/2026-09-23-theme-settings-view-keystroke.md)).
