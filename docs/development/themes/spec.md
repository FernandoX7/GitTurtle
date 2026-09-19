# Custom themes — specification (September 17, 2026)

Status: planned. Queue: [`tasks.json`](tasks.json). Palette selection and provenance: [the research note](../2026-09-17-theme-palettes.md). Design intent: [`DESIGN.md` §Semantic palette ownership](../../../DESIGN.md#semantic-palette-ownership). The [architecture review](../architecture-review.md) applies: this changes a shared interface (`ThemeChoice` becomes one case of `ThemeSelection`), a persistent store (preferences version 5 → 6) and a hot path (theme application).

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

`Palette::readability_issues() -> Vec<ReadabilityIssue>` where an issue names the foreground (a `TokenKind` or graph lane index), the background (a `TokenKind` or the selected-row hover blend), the measured ratio and the minimum. The contrast function moves out of the test module. Rules, with the six surfaces being canvas, panel, subtle, hover, selected and `row_hover(true)`:

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
| modified, renamed | added/removed backgrounds | 3.0 | status icons inside diff tiles (new) |
| warning | six surfaces | 3.0 | conflict and warning glyphs (new) |

All ten current palettes pass every row with margin (computed on 2026-09-17 from `appearance.rs` and `graph.rs`: lowest values are Nord hover/panel 1.10, Nord selected/panel 1.20, Daylight line_number 4.53). Built-in tests assert the list is empty for `ThemeChoice::ALL`. For custom themes the list is advisory.

## Store migration (version 5 → 6)

- `StoredPreferences` gains `custom_themes: Vec<StoredCustomTheme>` (default empty) beside `project_library`; `AppSettings::theme` becomes `ThemeSelection`. A version-6 file whose selection is a built-in is byte-compatible with version 5 for the `settings` object.
- Loading accepts `1..=6`. Files below 6 load with an empty custom section; loading never writes. The custom section is user-authored data: a malformed token, duplicate id, duplicate name, invalid name or more than 32 entries makes `load_from` fail, so later saves refuse rather than drop the section — the project-name policy. A selection naming a missing custom id resolves to the default theme without rewriting the file, and the picker shows the default as selected.
- Saving writes `"version": 6`. `Preferences::save_custom_themes(&[CustomTheme])` rereads the disk store, replaces the section and saves atomically on the preference executor; `save_settings` keeps carrying the disk's custom section. An older app reading a version-6 file refuses it, starts with defaults and never overwrites it — the existing unsupported-version contract.
- `AppSettings::resolved_theme(appearance, custom_themes) -> ResolvedTheme`. Not following the system: the selection's palette. Following the system: Light appearance selects Braden; Dark appearance keeps the selection when its palette is dark (built-in or custom) and otherwise Midnight — the existing rule, applied to custom palettes through `is_light`.
- Tests: a complete version-5 fixture (theme, recents, drafts, names, library) loads identically with no customs and no write; a version-6 fixture with two customs and a custom selection round-trips byte-for-byte; each refusal above leaves the original bytes; a missing custom id resolves without a write; the existing `saved_daylight_remains_braden…` test still sees `"daylight"` as a bare string.

## Editor (visual contract)

Settings gains a **Your themes** card directly under the theme picker, using the existing grouped-surface card style: a title, a one-line description, a list of custom themes (30 px rows: a five-token swatch strip — canvas, panel, accent, added, removed — the name in primary text, the base name in muted text, a warning glyph with a count when `readability_issues` is non-empty) with compact Edit…, Export… and Delete… actions, and **New theme…** and **Import…** buttons using the shared 28 px compact `button` helper. Actions carry specific accessible names ("Edit Sunset theme").

The editor is an alert dialog like the profile editor, titled **New theme** or **Edit theme**:

- Header row: **Name** input; **Base** dropdown listing the twenty built-ins by label (changing the base after edits asks before replacing the draft tokens).
- Body, two columns at ≥ 1,060 px window width, stacked below that: left, the token groups Surfaces, Text, Accent, Status, Diff, each with a muted 12 px group label and rows of `label · description · 16 px swatch · hex Input · ColorPicker`; right, the preview card (`theme_preview` generalized to a `Palette`) above the warnings list. Row height 30 px (the navigator geometry); the hex field is monospace, seven characters wide, accepts `#rrggbb` or `rrggbb` in either case.
- Footer: **Reset to base** (secondary), **Cancel** (secondary), **Save** (primary). The body scrolls within the window height so the footer stays reachable at large text sizes.
- Live preview: every valid edit applies the draft through `Palette::apply`, coalesced to one application per frame with `on_next_frame`. The whole window, including the dialog, follows the draft. Cancel, Escape, or closing the dialog re-applies the saved selection.
- Warnings: recomputed on each valid edit; each line names both tokens, the ratio and the minimum ("Muted text on Selected 3.9:1, needs 4.5:1"), and the affected row shows a warning glyph. Warnings never block Save; a saved theme with warnings keeps the glyph on its list row and picker card.
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

The two paths end at different frames. A switch applies inside its handler, so its trace ends at the next frame callback, before that frame is drawn ([baseline](../../benchmarks/2026-09-18-theme-apply.md)). An edit applies the coalesced draft from a frame callback, so its trace runs through the frame that shows the draft, draw included. An early editor build measured edits at median 45 ms and p95 56.6 ms, dominated by a whole-window draw of Settings plus the dialog (about 20 ms at 2x). By owner decision on 2026-09-19, `themes-editor` records the edit cost without gating on it, and `themes-draw-cost` owns meeting the 16 ms edit budget in the application path; the budget itself is unchanged.

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

- Whether GPUI Kit 0.6's `ColorPicker` supports keyboard operation and hex entry; if not, the hex `Input` is the primary control and the picker is pointer-only, which the editor task records.
- Whether `prompt_for_new_path` works through the Linux FileChooser portal on the validation desktop; if not, export needs the same guidance path as the folder picker and the native evidence for export comes from macOS.
- If a chosen palette cannot satisfy the rules without losing its character, the batch task swaps it for an eligible candidate from the research note and says so.
- If the apply budget is missed because `Theme::sync_base` or a full-window repaint dominates, the fix belongs in the application path (skip unchanged tokens, avoid rebuilding decorations that did not change), never in the budget.
