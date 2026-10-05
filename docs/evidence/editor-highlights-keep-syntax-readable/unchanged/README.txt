Clause `unchanged`: dumped-and-compared record
==============================================

Task:      editor-highlights-keep-syntax-readable, contract clause `unchanged`
Base:      e5afb228d53ebb4bf226cc8802c11ad7288ce737 (the candidate's parent, origin/main for this clause)
Candidate: 376148846a4431cfd2da88ac5cfd3f945a2a85b5
Recorded:  2026-10-05, Linux x86_64, rustc 1.99.0, test profile (debug)

Clause half covered: "Syntax colours and row and list selection are byte-identical to
origin/main for every built-in and Omarchy fixture palette (dumped and compared)". The
distinctness half (each fitted highlight against its line backgrounds) is not part of this
record.


Files
-----
base.json       The dump from the base tree.
candidate.json  The dump from the candidate tree, written by the same test.

Both are pretty-printed JSON with every object's keys sorted:

  { "builtin" | "omarchy": { <palette>: { "applied_is_light": bool,
                                          "dark":  <configuration>,
                                          "light": <configuration> } } }

`applied_is_light` is the configuration the app applies for that palette. Each configuration is
`Palette::configured_theme(is_light)` (the kit theme `Palette::apply` leaves: the mode's
default theme and highlight theme, then `configure`), dumped in three sections:

1_syntax (compared)
  highlight_theme           The configured kit `HighlightTheme` as serde serializes it: name,
                            appearance, the editor.* colors, the status colors and all 41
                            syntax styles, each with color, font_style and font_weight (every
                            field `ThemeStyle` holds).
  theme.colors.foreground   The editor foreground (also present as
                            highlight_theme.style["editor.foreground"]).

2_row_and_list_selection (compared)
  palette.selected                         The palette's row selection fill.
  palette.row_hover(true)                  A hovered selected row.
  palette.control_fill(palette.selected)   A selected/pressed shared control's fill.
  theme.colors and theme.tokens, each of:
    list_active, list_active_border        Kit list rows (list_item.rs).
    table_active, table_active_border      Kit table rows.
    sidebar_accent, sidebar_accent_foreground
                                           Kit sidebar selection.
    button_active, secondary_active, button_secondary_active
                                           Selected/pressed controls (all set from `selected`).
  These are every colour `configure` derives from `selected` other than the text selection,
  the two active borders it pairs with them, and the app-side colours derived from `selected`.
  The selected shared-button variant `apply` builds needs an application and is not dumped;
  it takes its fill from the compared `selected`.

3_info_text_selection (information only, not compared)
  theme.colors.selection, theme.tokens.selection
                            The text selection the candidate fits (expected to change).

Colours are "#rrggbbaa" as the kit serializes them (the palette's 0xrrggbb values written as
"#rrggbbff"); kit Hsla and Rgba values also carry their exact float channels, and tokens carry
their serialized background. Palette-level values repeat in both configurations.


Palettes
--------
builtin  All 20 `ThemeChoice::ALL`, by their Debug names.
omarchy  All 24 embedded fixtures in crates/app/tests/fixtures/omarchy/*.toml (the 22 bundled
         themes and alacritty-dark, alacritty-light), named "Omarchy <file stem>", each loaded
         and mapped as `omarchy::palette::tests::fixtures` and
         `fixture_syntax_colors_read_on_every_editor_background` do: `parse(bytes, false)`, then
         `map`, taking the mapped palette and its is_light.
44 palettes, each in both configurations: 88 configurations per dump.


How it was produced
-------------------
A temporary test module, never committed, was added to a fresh clone of each commit:
crates/app/src/appearance/omarchy/unchanged_dump.rs, declared by adding

    #[cfg(test)]
    mod unchanged_dump;

after `mod palette;` in crates/app/src/appearance/omarchy.rs. The module file and the
declaration are byte-identical in both trees: every API it reads (`Palette::configured_theme`,
`row_hover`, `control_fill`, `ThemeChoice::ALL`, the Omarchy `parse` and `map`, the kit
`Theme` colors, tokens and highlight theme) has the same signature at both commits, so no
adaptation was needed. Each tree was built and run alone, in its own fresh target directory:

    UNCHANGED_DUMP_OUT=<out>.json CARGO_TARGET_DIR=<clone>/target \
      cargo test --locked -p gitturtle --bin gitturtle \
      appearance::omarchy::unchanged_dump::dump_unchanged_colors -- --nocapture --exact

(The crate has no library target, so `--bin gitturtle` selects the unit tests.) Both builds
and runs passed on the first attempt; the compiler did not crash.


Comparison and result
---------------------
Sections 1 and 2: IDENTICAL. For each of the 44 palettes and both configurations, sections 1
and 2 serialize to the same bytes in both dumps: 0 differences in 88 configurations. With
section 3 removed, the two dumps re-serialize to identical bytes (sha256
c63842d9ae5632b991ead6040654e263eafc7ae95a11f3395ca4153805bd7a53 for both). A line diff of
base.json and candidate.json (48,802 lines each) changes 484 lines on each side, every one
inside a 3_info_text_selection object. `applied_is_light` is also identical for every palette.

Section 3 (information): on the base, colors.selection equals palette.selected in all 88
configurations. On the candidate it changes in 82 of 88 (41 of 44 palettes) and stays equal
in DeepSea, Graphite and Omarchy last-horizon, whose `selected` seed already fits. In both
trees it is the same in the dark and the light configuration, and tokens.selection carries
the same colour. Base -> candidate (applied configuration):

  Alucard                      light  #cfcfdeff -> #e6e5fcff
  CatppuccinMocha              dark   #36324bff -> #363054ff
  Daylight                     light  #daece6ff -> #bef6e4ff
  DeepSea                      dark   #21434cff -> #21434cff  (unchanged)
  Dracula                      dark   #44475aff -> #2f3356ff
  Ember                        dark   #48343dff -> #4c3526ff
  Graphite                     dark   #38324eff -> #38324eff  (unchanged)
  KanagawaLotus                light  #c7d7e0ff -> #cddce4ff
  KanagawaWave                 dark   #24364eff -> #1b2c50ff
  Midnight                     dark   #223b3bff -> #0b3e3eff
  Nord                         dark   #3b485cff -> #283f63ff
  OneDark                      dark   #323d52ff -> #273756ff
  OneLight                     light  #e9edffff -> #d9fcffff
  Porcelain                    light  #dce6f6ff -> #d8e6fdff
  RosePine                     dark   #2d2a45ff -> #2c2944ff
  RosePineDawn                 light  #e8dfe2ff -> #c4eeffff
  Sandstone                    light  #f2dcd0ff -> #ffe0bbff
  SolarizedDark                dark   #0b4154ff -> #002b4eff
  SolarizedLight               light  #d0dad5ff -> #cbeeddff
  TokyoNight                   dark   #2c3552ff -> #2a324eff
  Omarchy alacritty-dark       dark   #363a46ff -> #272e44ff
  Omarchy alacritty-light      light  #c7c5c0ff -> #d8e5f6ff
  Omarchy catppuccin           dark   #383949ff -> #363753ff
  Omarchy catppuccin-latte     light  #c6cbd6ff -> #cae6ffff
  Omarchy ethereal             dark   #232c52ff -> #1b184aff
  Omarchy everforest           dark   #414d52ff -> #28434eff
  Omarchy flexoki-light        light  #d5d4cbff -> #e0f0ffff
  Omarchy gruvbox              dark   #46403dff -> #1c3c36ff
  Omarchy hackerman            dark   #22283fff -> #202744ff
  Omarchy kanagawa             dark   #363646ff -> #2d2c47ff
  Omarchy last-horizon         dark   #322c2eff -> #322c2eff  (unchanged)
  Omarchy lumon                dark   #243d56ff -> #1f3d5bff
  Omarchy lupine               light  #d1d1d1ff -> #d9faffff
  Omarchy matte-black          dark   #2e2e2eff -> #3c2a18ff
  Omarchy miasma               dark   #3b3b3bff -> #2f3126ff
  Omarchy nord                 dark   #434c5eff -> #33415eff
  Omarchy osaka-jade           dark   #293a30ff -> #002916ff
  Omarchy retro-82             dark   #0e3a43ff -> #09242aff
  Omarchy ristretto            dark   #403e41ff -> #343335ff
  Omarchy rose-pine            light  #d4cdccff -> #c1eef7ff
  Omarchy solitude             dark   #2b3336ff -> #0b2f41ff
  Omarchy tokyo-night          dark   #2d3349ff -> #2c3147ff
  Omarchy vantablack           dark   #292929ff -> #1d222bff
  Omarchy white                light  #d7d7d7ff -> #dae8ffff

Scope note: the kit table's right-clicked-row outline draws with `colors.selection`; GitTurtle
does not use the kit table, so no row or list the app draws takes the changed colour.


sha256
------
8da0ba5a843338c60a23c8e97fb63a53104d8dfe5afa22d6f07a8097893a88b3  base.json
9f75d0ef7b363587ab034c1642bd2cf6c74227bed0467a122f6bed5cbed5522b  candidate.json
27c3f247780b94943eedd91d321f7e5fb6124f6d3d37f739567beb12384c7b13  unchanged_dump.rs, base tree
27c3f247780b94943eedd91d321f7e5fb6124f6d3d37f739567beb12384c7b13  unchanged_dump.rs, candidate tree
2b77d65b3bff1f2a889e877f964975425521b8b07b6f5564dc273ecfb5d321f4  `git diff -- crates/app/src/appearance/omarchy.rs` (the declaration), both trees
