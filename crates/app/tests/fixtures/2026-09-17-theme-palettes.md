# Built-in theme palettes for the twenty-theme set — September 17, 2026

## Decision and scope

- Question: which ten palettes join the ten in [`crates/app/src/appearance.rs`](../../crates/app/src/appearance.rs) (revision `a5a35d9`) so GitTurtle ships twenty built-in themes, and what each license requires of [`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md).
- User-visible outcome: ten more cards in the Settings theme picker — five widely used editor and terminal families, each in its dark and its light form — with the same readability guarantees as the existing themes.
- Recommendation: **Solarized** (Dark, Light), **One** (Dark, Light), **Rosé Pine** (Main, Dawn), **Dracula** (Dracula, Alucard) and **Kanagawa** (Wave, Lotus). All five families are MIT-licensed, each publishes both a dark and a light variant from one upstream source, and together they add five light palettes to a set that today has seven dark and three light. They complement rather than repeat the existing set: the current dark themes are slate, charcoal, city blue, pastel, arctic, ocean and plum; the new ones bring the two classics (Solarized, One), the most installed dark palette (Dracula), a rosé/muted family and an ink-and-paper family.
- Variant rule: one dark and one light entry per family, the family's primary variant of each. Secondary variants (Rosé Pine Moon, Kanagawa Dragon, Solarized's alternate terminal lightness) stay out; a user who wants one starts a custom theme from the nearest built-in.
- Outside this decision: the custom-theme feature (see [the specification](themes/spec.md)), changing existing palettes, syntax-highlighting themes for the editor, and terminal export.

## Evidence

Checked on 2026-09-17 with `gh api` (repository metadata and raw file contents) and `curl` against the pinned revisions below. Values are copied from the upstream file named in each row; nothing here comes from memory.

| Source | Checked date/version | Relevant finding | Limit |
| --- | --- | --- | --- |
| [Solarized README](https://github.com/altercation/solarized/blob/62f656a02f93c5190a8753159e34b385588d5ff3/README.md), `LICENSE` at the same revision | 2026-09-17, `62f656a` | The sixteen-value table (`base03`…`base3`, eight accents) and the pairing rule: dark uses `base03`/`base02` surfaces with `base0`/`base1` text, light inverts them. MIT, © 2011 Ethan Schoonover. | The README also documents reduced-lightness terminal variants; not used. |
| [Atom One Dark `styles/colors.less`](https://github.com/atom/one-dark-syntax/blob/9c96f4454362267ac45322063e193ccf9d2debb1/styles/colors.less), `LICENSE.md` | 2026-09-17, `9c96f44` (repository archived 2018) | Colors are defined in HSL; hex values below are computed from those definitions (`colorsys`, rounded). MIT, © 2016 GitHub Inc. | Archived, so no future upstream changes; the computed hex values are the ones the implementer must use. |
| [Atom One Light `styles/colors.less`](https://github.com/atom/one-light-syntax/blob/d84579027410c576086dfca14d934c4bd74b0438/styles/colors.less), `LICENSE.md` | 2026-09-17, `d845790` (archived) | Same structure; the license text is byte-identical to One Dark's, so one copy is stored. | As above. |
| [Rosé Pine `palette.json`](https://github.com/rose-pine/palette/blob/92af52b465ab6e47437aca223c9b8d3009a2023b/palette.json), `LICENSE` | 2026-09-17, `92af52b` (`palette.json` SHA-256 `8b71546357c65ee23715e77f144d914e5f415f44f3ef3b0179bbe1f17d69211d`) | Twelve roles for `main`, `moon` and `dawn`. MIT, © mvllow. The `rose-pine/rose-pine-theme` README carries no values. | Highlight surfaces (`highlight low/med/high`) are not in this file; the implementer derives hover/selected from `overlay` and `surface`. |
| [Dracula README](https://github.com/dracula/dracula-theme/blob/5962daae54e4608d281cb6f4eee2349e605d9e3c/README.md), `LICENSE` | 2026-09-17, `5962daae` | The Color Palette table and an official **Alucard** light table in the same repository. MIT, © 2023 Dracula Theme. | Alucard is recent; its "Current Line" `#6c664b` is a dark value that cannot serve as a light hover surface, so surfaces need tuning. |
| [Kanagawa `lua/kanagawa/colors.lua`](https://github.com/rebelot/kanagawa.nvim/blob/bb85e4bfc8d89b0e62c8fa53ccdd13d12e2f77b3/lua/kanagawa/colors.lua), `LICENSE` | 2026-09-17, `bb85e4b` | Named palette constants for Wave (`sumiInk*`, `fuji*`, …), Dragon and Lotus. MIT, © 2021 Tommaso Laurenzi. | Semantic mapping (which named color is "muted text") is documented only in the theme's Lua; the batch task decides and records it. |
| [Catppuccin `palette.json`](https://github.com/catppuccin/palette/blob/07d02aa110ef9eb7e7427afca5c73ba9cf7f8ebd/palette.json) | 2026-09-17, `07d02aa` (already pinned in `sources.json`) | Latte values, for the follow-up below. | Not part of the ten. |
| GitHub repository metadata (`gh api repos/<owner>/<repo>`) | 2026-09-17 | License SPDX id, archived flag, stars and last push for every candidate in the table below. | GitHub's classifier returns `NONE` when no license file exists and `NOASSERTION` when it cannot classify a text; both were checked by hand. |

### Candidates

| Palette | Upstream | License (checked) | Variants | Decision |
| --- | --- | --- | --- | --- |
| Solarized | `altercation/solarized` (16.0k ★, pushed 2024-07) | MIT | dark, light | **Chosen** |
| One Dark / One Light | `atom/one-dark-syntax`, `atom/one-light-syntax` (archived 2018) | MIT | dark, light | **Chosen** |
| Rosé Pine | `rose-pine/palette` values, `rose-pine/rose-pine-theme` (1.6k ★) | MIT | main, moon (dark); dawn (light) | **Chosen**: Main + Dawn |
| Dracula | `dracula/dracula-theme` (23.6k ★, pushed 2026-09) | MIT | Dracula (dark); Alucard (light) | **Chosen** |
| Kanagawa | `rebelot/kanagawa.nvim` (6.4k ★) | MIT | wave, dragon (dark); lotus (light) | **Chosen**: Wave + Lotus |
| Catppuccin Latte | `catppuccin/palette` | MIT (already attributed) | light sibling of the built-in Mocha | First follow-up: completes an existing family at no licensing cost; held back only by the count |
| Gruvbox | `morhetz/gruvbox` (15.7k ★) | **none** — no license file in the repository root; `GET /license` returns 404 | dark, light | Excluded: no license to comply with |
| Tomorrow | `chriskempson/tomorrow-theme` (14.0k ★) | MIT text in `LICENSE.md` (GitHub reports `NOASSERTION`) | Tomorrow (light), Night variants (dark) | Eligible, not chosen: One and Solarized cover its role |
| Everforest | `sainnhe/everforest` (4.2k ★) | MIT | dark, light | Eligible, next batch candidate |
| Ayu | `ayu-theme/ayu-colors` | MIT | dark, mirage, light | Eligible, next batch candidate |
| Night Owl | `sdras/night-owl-vscode-theme` (3.0k ★) | MIT | dark, light | Eligible, next batch candidate |
| Flexoki | `kepano/flexoki` (3.7k ★) | MIT | dark, light | Eligible, next batch candidate |
| Nightfox | `EdenEast/nightfox.nvim` (4.1k ★) | MIT | several dark, two light | Eligible, next batch candidate |
| Iceberg | `cocopon/iceberg.vim` (2.4k ★) | MIT | dark, light | Eligible |
| Selenized | `jan-warchol/selenized` | MIT | dark, black, light, white | Eligible |
| Modus | `protesilaos/modus-themes` | GPL-3.0 | dark, light | Excluded: copyleft |
| Primer | `primer/github-vscode-theme` | MIT | dark, light | Excluded: the theme carries a hosting brand's name |
| Monokai | no canonical open-source palette repository identified | unverified | dark | Not evaluated further |

### Source values for the chosen palettes

These are the upstream values the palette-sources task copies into `crates/app/src/appearance/sources.rs`; attempt sessions may run without network access, so the note carries them. Semantic mapping to GitTurtle's 21 tokens is the batch tasks' work and follows the same rule as Nord: readability thresholds win over exact source values, and every tuned token is recorded in `DESIGN.md`.

**Solarized** (`README.md`, `62f656a`): base03 `#002b36`, base02 `#073642`, base01 `#586e75`, base00 `#657b83`, base0 `#839496`, base1 `#93a1a1`, base2 `#eee8d5`, base3 `#fdf6e3`; yellow `#b58900`, orange `#cb4b16`, red `#dc322f`, magenta `#d33682`, violet `#6c71c4`, blue `#268bd2`, cyan `#2aa198`, green `#859900`. Dark: canvas base03, panel base02, text base0, muted base1. Light: canvas base3, panel base2, text base00, muted base01. Expect `base1` on dark and `base01` on light to need darkening or lightening for 4.5:1 on selected/hover surfaces.

**One Dark** (`styles/colors.less`, `9c96f44`, hex computed from HSL): bg `#282c34`, mono-1 `#abb2bf` (text), mono-2 `#828997`, mono-3 `#5c6370`, cyan `#56b6c2`, blue `#61afef`, purple `#c678dd`, green `#98c379`, red-1 `#e06c75`, red-2 `#be5046`, orange-1 `#d19a66`, orange-2 `#e5c07b`, accent `#528bff`.

**One Light** (`styles/colors.less`, `d845790`, hex computed from HSL): bg `#fafafa`, mono-1 `#383a42` (text), mono-2 `#696c77`, mono-3 `#a0a1a7`, cyan `#0184bc`, blue `#4078f2`, purple `#a626a4`, green `#50a14f`, red-1 `#e45649`, red-2 `#ca1243`, orange-1 `#986801`, orange-2 `#c18401`, accent `#526fff`.

**Rosé Pine Main** (`palette.json`, `92af52b`): base `#191724`, surface `#1f1d2e`, overlay `#26233a`, muted `#6e6a86`, subtle `#908caa`, text `#e0def4`, love `#eb6f92`, gold `#f6c177`, rose `#ebbcba`, pine `#31748f`, foam `#9ccfd8`, iris `#c4a7e7`.

**Rosé Pine Dawn** (`palette.json`, `92af52b`): base `#faf4ed`, surface `#fffaf3`, overlay `#f2e9e1`, muted `#9893a5`, subtle `#797593`, text `#464261`, love `#b4637a`, gold `#ea9d34`, rose `#d7827e`, pine `#286983`, foam `#56949f`, iris `#907aa9`. Note `muted #9893a5` is below 4.5:1 on the light surfaces and must be darkened for secondary text.

**Dracula** (`README.md`, `5962daae`): background `#282a36`, current line and selection `#44475a`, foreground `#f8f8f2`, comment `#6272a4`, cyan `#8be9fd`, green `#50fa7b`, orange `#ffb86c`, pink `#ff79c6`, purple `#bd93f9`, red `#ff5555`, yellow `#f1fa8c`.

**Alucard** (`README.md`, `5962daae`): background `#fffbeb`, current line `#6c664b`, selection `#cfcfde`, foreground `#1f1f1f`, comment `#6c664b`, cyan `#036a96`, green `#14710a`, orange `#a34d14`, pink `#a3144d`, purple `#644ac9`, red `#cb3a2a`, yellow `#846e15`.

**Kanagawa Wave** (`colors.lua`, `bb85e4b`): sumiInk0 `#16161D`, sumiInk1 `#181820`, sumiInk2 `#1a1a22`, sumiInk3 `#1F1F28`, sumiInk4 `#2A2A37`, sumiInk5 `#363646`, sumiInk6 `#54546D`, waveBlue1 `#223249`, waveBlue2 `#2D4F67`, winterGreen `#2B3328`, winterYellow `#49443C`, winterRed `#43242B`, winterBlue `#252535`, autumnGreen `#76946A`, autumnRed `#C34043`, autumnYellow `#DCA561`, samuraiRed `#E82424`, roninYellow `#FF9E3B`, waveAqua1 `#6A9589`, dragonBlue `#658594`, oldWhite `#C8C093`, fujiWhite `#DCD7BA`, fujiGray `#727169`, oniViolet `#957FB8`, crystalBlue `#7E9CD8`, springViolet1 `#938AA9`, springViolet2 `#9CABCA`, springBlue `#7FB4CA`, waveAqua2 `#7AA89F`, springGreen `#98BB6C`, boatYellow2 `#C0A36E`, carpYellow `#E6C384`, sakuraPink `#D27E99`, waveRed `#E46876`, peachRed `#FF5D62`, surimiOrange `#FFA066`, katanaGray `#717C7C`.

**Kanagawa Lotus** (`colors.lua`, `bb85e4b`): lotusInk1 `#545464`, lotusInk2 `#43436c`, lotusGray `#dcd7ba`, lotusGray2 `#716e61`, lotusGray3 `#8a8980`, lotusWhite0 `#d5cea3`, lotusWhite1 `#dcd5ac`, lotusWhite2 `#e5ddb0`, lotusWhite3 `#f2ecbc`, lotusWhite4 `#e7dba0`, lotusWhite5 `#e4d794`, lotusViolet1 `#a09cac`, lotusViolet2 `#766b90`, lotusViolet3 `#c9cbd1`, lotusViolet4 `#624c83`, lotusBlue1 `#c7d7e0`, lotusBlue2 `#b5cbd2`, lotusBlue3 `#9fb5c9`, lotusBlue4 `#4d699b`, lotusBlue5 `#5d57a3`, lotusGreen `#6f894e`, lotusGreen2 `#6e915f`, lotusGreen3 `#b7d0ae`, lotusPink `#b35b79`, lotusOrange `#cc6d00`, lotusOrange2 `#e98a00`, lotusYellow `#77713f`, lotusYellow2 `#836f4a`, lotusYellow3 `#de9800`, lotusYellow4 `#f9d791`, lotusRed `#c84053`, lotusRed2 `#d7474b`, lotusRed3 `#e82424`, lotusRed4 `#d9a594`, lotusAqua `#597b75`, lotusAqua2 `#5e857a`, lotusTeal1 `#4e8ca2`, lotusTeal2 `#6693bf`, lotusTeal3 `#5a7785`, lotusCyan `#d7e3d8`.

**Catppuccin Latte** (follow-up only, `palette.json`, `07d02aa`): rosewater `#dc8a78`, flamingo `#dd7878`, pink `#ea76cb`, mauve `#8839ef`, red `#d20f39`, maroon `#e64553`, peach `#fe640b`, yellow `#df8e1d`, green `#40a02b`, teal `#179299`, sky `#04a5e5`, sapphire `#209fb5`, blue `#1e66f5`, lavender `#7287fd`, text `#4c4f69`, subtext1 `#5c5f77`, subtext0 `#6c6f85`, overlay2 `#7c7f93`, overlay1 `#8c8fa1`, overlay0 `#9ca0b0`, surface2 `#acb0be`, surface1 `#bcc0cc`, surface0 `#ccd0da`, base `#eff1f5`, mantle `#e6e9ef`, crust `#dce0e8`.

## Compatibility and alternatives

- No dependency changes: `ThemeChoice` gains ten variants with snake_case serde names, `ALL` grows from 10 to 20, `is_light` and the picker's light/dark grouping adapt, and the existing `#[serde(other)]` default keeps older stores readable.
- MIT obligations are the copyright notice and the permission notice in copies of the software. GitTurtle stores each text under `docs/licenses/assets/`, pins it to the inspected revision with a SHA-256 in `sources.json` (verified by `scripts/collect-third-party-licenses.py`), and names the family in `THIRD_PARTY_NOTICES.md`. No in-app attribution string is required; theme names are used descriptively and imply no endorsement.
- Alternatives: a ten-family set with every variant (about seventeen entries) was rejected because the request fixes the count at twenty; Catppuccin Latte is the first addition if the count is relaxed. Gruvbox would otherwise be an obvious pick, but its repository has no license file, so a compliant copy cannot be documented; Gruvbox Material (`sainnhe/gruvbox-material`, MIT) is a derived palette and was not evaluated.
- What would change the recommendation: an upstream license change at a later revision (the pinned revisions are what was inspected), a preference for more dark-only palettes, or a readability mapping that cannot reach the thresholds without leaving the family recognizable — in that case the batch task reports it and the family is swapped for an eligible candidate above.

## Enforcement and acceptance

- Owners: `crates/app/src/appearance.rs` and its `sources.rs` submodule for values, `DESIGN.md` §Semantic palette ownership for the family list and tuned tokens, `THIRD_PARTY_NOTICES.md` and `docs/licenses/assets/` for attribution.
- Tests: `palettes_keep_text_and_diff_content_readable_in_each_theme` and `theme_switch_updates_resolved_component_backgrounds_with_foregrounds` over `ThemeChoice::ALL`, extended by the readability-rules task; serde round-trip; the collector's supplement hash check.
- Native evidence: each batch task records the Settings picker and History in every new theme at the minimum window size.
- Queue: [`themes/tasks.json`](themes/tasks.json) — `themes-palette-sources`, then the three batch tasks.
- Remaining uncertainty: semantic mapping of Solarized secondary text, Rosé Pine Dawn's muted text, Alucard's surfaces and Kanagawa's muted tones; each is settled by the readability rules during the batch, with tuned tokens recorded.

## Outcome

To be recorded by the batch tasks when the themes ship, with the exact revisions of `sources.rs` and the native evidence entries.
