# Adapted palettes render below their contrast floors at 1x — September 18, 2026

Status: fixed in the palettes on 2026-09-23 (follow-up item 20, NEAR-FLOOR) and
recaptured at 1x on 2026-09-24, with the peaks measured in "Checking the rendered
result" below. Decided by the repository owner on 2026-09-18:
ship the batches as their contracts specify and correct every near-floor palette
in one follow-up rather than re-opening accepted work. The sections up to "What
the follow-up should do" are the record that follow-up started from; "What
landed" says what changed.

## The finding

`readability_issues` computes its ratios from the declared `Palette` tokens. The
rendered result is lower, because glyph antialiasing on this platform is
grayscale and even the brightest stem pixel of small text falls short of full
coverage. Measured from the committed 1x captures, muted text on a hovered
selected row:

| theme | declared | rendered peak | shortfall |
| --- | --- | --- | --- |
| Solarized Dark | 4.60 | 4.39 | 0.21 |
| Solarized Light | 4.60 | 4.32 | 0.28 |
| One Dark | 4.62 | 4.42 | 0.20 |
| One Light | 4.55 | 4.29 | 0.26 |
| Rosé Pine | 4.51 | 4.35 | 0.16 |
| Rosé Pine Dawn | 4.54 | 4.28 | 0.26 |
| Dracula | 4.53 | 4.31 | 0.22 |

In all seven, **no rendered pixel reaches 4.5:1**. At 2x the peak equals the
declared value exactly, so this is rasterization at 1x, not a palette defect. The
same shortfall applies to comfortable pairs — Dracula's `muted` on canvas is 7.77
declared and 7.36 rendered — so the effect is general and only matters where the
declared margin is thin.

[`DESIGN.md` §Semantic palette ownership](../../../DESIGN.md) makes the numeric
rules a floor and says native review still checks the rendered result. By that
standard these pairs are below floor, which is why this is worth correcting
rather than noting.

## Why the palettes sit there

Each batch contract says to tune a token *only as far as the named readability
rule requires*. Measured on declared tokens the implementers did exactly that:
they tuned until the rule passed and stopped. Eight pairs clear their floor by
less than 0.02 — the tightest are Rosé Pine `hover` vs `panel` at +0.0067 and
Rosé Pine Dawn `renamed` on a selected-row hover at +0.0072. The contract is what
is underspecified, not the work.

The native-QA passes read text ratios from full-coverage glyph cores, which
deliberately excludes this effect, so the attestations in this queue record
declared-equivalent values rather than rendered ones. Read them that way.

## What the follow-up should do

1. Nudge the tuned tokens by about 4/255 per channel. Proposed values, all of
   them already tuned non-upstream tokens whose rule row in `DESIGN.md` stays
   valid as written: Rosé Pine Dawn `added` `#42717a`→`#3e6d76`, `removed`
   `#985367`→`#944f63`, `modified`/`warning` `#986622`→`#94621e`, `renamed`
   `#86719d`→`#826d99`, `muted` `#5e5b73`→`#5a576f`, `line_number`
   `#716d89`→`#6d6985`; Dracula `muted` `#b8bfd6`→`#bcc3da`, `removed`
   `#ff6f6f`→`#ff7373`, `line_number` `#8b97bc`→`#8f9bc0`; Rosé Pine `muted`
   `#a19db7`→`#a5a1bb`. Worst text margins become +0.232 / +0.181 / +0.124,
   above the measured shortfall, at a perceptual cost of ΔE 1.50–2.12 — at or
   below a just-noticeable difference, and an order of magnitude smaller than the
   tuning already applied.
2. Make the target explicit in `DESIGN.md`'s tuning sentence: a tuned token
   clears its rule by at least 0.25 where the family's own values allow, because
   the rule is computed on declared tokens and 1x rasterization costs about 0.2.
3. Exempt Rosé Pine's `hover`/`panel` deliberately. Both sides are upstream
   (`overlay` on `surface`), so buying margin means leaving the palette. Record
   it as a decision so it does not read as an oversight.
4. Cover Solarized Dark and Light, One Dark and Light, Rosé Pine, Dawn and
   Dracula together, and the two outliers among the original ten — Daylight
   `line_number` on canvas at +0.033 and Nord `hover` vs `panel` at +0.022. By
   count of text pairs under a 0.25 margin, Solarized Dark and Solarized Light
   are the worst at eleven each; Rosé Pine has one.
5. Recapture once. Changing these palettes invalidates the committed screenshots
   under `docs/evidence/themes/`, which would otherwise depict palettes the app
   no longer ships. One correction across every palette means one recapture and
   one consistent set; two partial corrections leave the repository half-done.

## What landed

`DESIGN.md`'s tuning sentence now sets the target: a tuned token clears its text
and glyph rules by at least 0.25 where the family's own values allow. Every
tuned token of the ten adapted palettes was checked against it, not only the
three families the proposal named, and `spec.md` gained a rule: `warning` as
text on `subtle` at 4.5:1, for the failure messages on grouped cards such as
Your themes.

The proposed values fell short of that target: Rosé Pine `muted` reached +0.232,
Dracula `muted` +0.228 and `removed` in its tile +0.124, Rosé Pine Dawn `renamed`
+0.181, and the proposed Dawn `warning` `#94621e` is 4.49:1 on `subtle`, below the
new rule. The landed values move each token in equal steps per channel, the
proposal's method, only as far as the target and a modelled 1x peak of 4.5
require (the model is below). Ratios are declared, before → after; each row
lists the pairs that needed the change:

| Theme | Token | Before → after | Pairs, before → after |
| --- | --- | --- | --- |
| Solarized Dark | muted | `#A5B0B0` → `#AAB5B5` | muted on hovered selected row 4.60→4.81 |
| Solarized Dark | accent | `#2E93D9` → `#3499DF` | accent on hovered selected row 3.07→3.26 |
| Solarized Dark | added | `#90A600` → `#92A802` | added on added tile 4.69→4.80 |
| Solarized Dark | removed | `#E66C6A` → `#EA706E` | removed on removed tile 4.62→4.84 |
| Solarized Dark | renamed | `#8488CD` → `#898DD2` | renamed on hovered selected row 3.11→3.28 |
| Solarized Dark | modified and warning | `#B58900` → `#BE9209` | canvas label on warning 4.68→5.22; warning on subtle 4.34→4.84; warning on hovered selected row 3.19→3.52 |
| Solarized Dark | hunk | `#2DABA2` → `#30AEA5` | hunk on panel 4.62→4.78 |
| Solarized Dark | line number | `#879DA5` → `#8AA0A8` | line number on panel 4.58→4.75 |
| Solarized Light | muted | `#495B61` → `#44565C` | muted on hovered selected row 4.60→4.93 |
| Solarized Light | accent | `#2178B6` → `#1C73B1` | accent foreground on accent 4.74→5.08; accent on hovered selected row 3.07→3.26 |
| Solarized Light | added | `#5F6D00` → `#5B6900` | added on added tile 4.63→4.91 |
| Solarized Light | removed | `#C62422` → `#C2201E` | removed on removed tile 4.59→4.79 |
| Solarized Light | renamed | `#656AC1` → `#6166BD` | renamed on hovered selected row 3.09→3.25 |
| Solarized Light | modified and warning | `#8C6A00` → `#846200` | canvas label on warning 4.66→5.22; warning on subtle 4.38→4.91; warning on hovered selected row 3.25→3.61 |
| Solarized Light | hunk | `#1D6CA2` → `#19689E` | hunk on panel 4.61→4.88 |
| Solarized Light | line number | `#566B72` → `#51666D` | line number on panel 4.58→4.94 |
| One Dark | text | `#ABB2BF` → `#AEB5C2` | text on hovered selected row 4.62→4.78, kept above muted |
| One Dark | muted | `#ADB2BB` → `#B0B5BC` | muted on hovered selected row 4.62→4.77 |
| One Dark | removed | `#E5858D` → `#E98991` | removed on removed tile 4.58→4.80 |
| One Dark | line number | `#979DA8` → `#9AA0AB` | line number on panel 4.65→4.82 |
| One Light | muted | `#62656F` → `#5C5F69` | muted on hovered selected row 4.55→4.98; muted on removed tile 4.86→5.32 |
| One Light | accent | `#2F6CF1` → `#2D6AEF` | accent foreground on accent 4.63→4.75 |
| One Light | added | `#3B763A` → `#377236` | added on added tile 4.65→4.93 |
| One Light | modified and warning | `#986801` → `#8E5E00` | canvas label on warning 4.66→5.36; warning on subtle 4.27→4.92 |
| One Light | hunk | `#0174A5` → `#0070A1` | hunk on subtle 4.56→4.81 |
| Rosé Pine | muted | `#A19DB7` → `#A6A2BC` | muted on hovered selected row 4.51→4.79 |
| Rosé Pine Dawn | muted | `#5E5B73` → `#59566E` | muted on hovered selected row 4.54→4.91 |
| Rosé Pine Dawn | added | `#42717A` → `#3C6B74` | added on added tile 4.51→4.93 |
| Rosé Pine Dawn | removed | `#985367` → `#934E62` | removed on removed tile 4.52→4.85 |
| Rosé Pine Dawn | renamed | `#86719D` → `#806B97` | renamed on hovered selected row 3.01→3.27 |
| Rosé Pine Dawn | modified and warning | `#986622` → `#8E5C18` | canvas label on warning 4.51→5.20; warning on subtle 4.25→4.89 |
| Rosé Pine Dawn | line number | `#716D89` → `#6B6783` | line number on canvas 4.52→4.94; line number on panel 4.76→5.19 |
| Dracula | muted | `#B8BFD6` → `#BDC4DB` | muted on hovered selected row 4.53→4.78 |
| Dracula | removed | `#FF6F6F` → `#FF7979` | removed on hovered selected row 3.07→3.27; removed on removed tile 4.51→4.80 |
| Dracula | line number | `#8B97BC` → `#909CC1` | line number on panel 4.51→4.79 |
| Alucard | muted | `#59543E` → `#534E38` | muted on hovered selected row 4.51→4.95 |
| Alucard | removed | `#BF3728` → `#BA3223` | removed on removed tile 4.52→4.80 |
| Kanagawa Wave | added | `#85A07A` → `#89A47E` | added on added tile 4.55→4.78 |
| Kanagawa Wave | line number | `#9090A9` → `#9494AD` | line number on panel 4.54→4.78 |
| Kanagawa Lotus | added | `#4E6643` → `#49613E` | added on added tile 4.51→4.87 |
| Kanagawa Lotus | modified | `#996900` → `#936300` | modified on hover 3.24→3.52; modified on hovered selected row 3.00→3.26; modified on removed tile 3.02→3.55 |
| Kanagawa Lotus | warning | `#9A5B00` → `#8B4C00` | canvas label on warning 4.52→5.59; warning on subtle 3.95→4.89 |
| Kanagawa Lotus | removed background | `#E6C8A8` → `#EED0B0` | text on removed tile 4.66→5.05; muted on removed tile 4.54→4.92; removed on removed tile 4.51→4.88; modified on removed tile 3.02→3.55 |
| Kanagawa Lotus | hunk | `#476190` → `#425C8B` | hunk on subtle 4.53→4.88 |
| Kanagawa Lotus | line number | `#6D6A5E` → `#676458` | line number on canvas 4.52→4.94; line number on panel 4.83→5.28 |
| Braden (Daylight) | line number | `#61728A` → `#5B6C84` | line number on canvas 4.53→4.95; line number on panel 4.90→5.35 |
| Nord | hover | `#3B4252` → `#3E4555` | hover on panel 1.10→1.15 |

`modified` keeps `warning`'s value in Solarized Dark, Solarized Light, One Light
and Rosé Pine Dawn, as each family maps one color to both roles. Solarized Dark's
yellow and One Light's orange-1 were upstream and are tuned now, because the new
rule needs them: the alternative, the derived `subtle`, would have to be darker
than Solarized Dark's canvas or nearly as light as One Light's panel.

The margin stops where the family's values set the limit. These keep their
thinner declared margin as a decision, not an oversight:

| Pair | Declared | Why it stays |
| --- | --- | --- |
| Rosé Pine hover on panel | 1.087 | both upstream (`overlay` on `surface`) |
| Kanagawa Lotus text on the hovered selected row | 4.64 | text, selected row and accent are all upstream |
| Kanagawa Lotus secondary text on the hovered selected row | 4.52 | darkening `muted` one step further puts it above its upstream text |
| Kanagawa Lotus primary-button label | 4.59 | both upstream (`lotusWhite3` on `lotusBlue4`) |
| Kanagawa Wave removed lines in their tile | 4.58 | both upstream (`peachRed` on `winterRed`) |
| Kanagawa Wave renamed icons on the hovered selected row | 3.15 | upstream icon and accent; the tuned selected row is held by its panel rule |
| One Dark secondary text against body text | muted 0.07% darker in luminance | muted needs its margin on the hovered selected row and may not read above text; only a darker derived selected row would let it drop below text with margin |

Secondary text never reads above body text: a test holds it on every surface of
every built-in. One Dark's shipped `muted` was a hair brighter than its upstream
text, and its named pair needs more than that text's 4.62, so One Dark's `text`
(mono-1) is tuned as well, the one upstream text this follow-up moves. The two
now sit at parity: muted `#B0B5BC` is 0.07% darker in luminance than text
`#AEB5C2` and differs from it only in hue and chroma (ΔE00 2.7), so One Dark's
secondary text is not subordinate by lightness. It was brighter than text
before; "not above" is what the rule requires, and the table records the parity
as a decision. Clearing
the rows above would mean tuning more upstream text or accent colors, which the
coordinator decided against on 2026-09-23: they are not named pairs.

Among the original ten, Braden's `line_number` and Nord's `hover` were nudged as
named.

Open, outside this follow-up: secondary text on the hovered selected row in the
default themes, declared (modelled 1x peak) Midnight 4.73 (4.51), Graphite 4.58
(4.34) and Braden 4.60 (4.27).

### Checking the rendered result

A model fitted to the September 18 measurements, glyph coverage 0.958 blended in
sRGB (the seven measured pairs fit 0.961 to 0.976), predicts the 1x peak. It
predicts 4.51 to 4.56 for muted text on the hovered selected row in the seven
themes and 4.52 to 4.55 for warning text on `subtle` in the five, and it predicts
the original values lower than they measured. These are predictions: the native
recapture measures the peaks, and a pair still below 4.5 needs a further step.

Measured on 2026-09-24 from `5899e2a` at 1x (1480x980, the September 18 method,
`.local/themes-evidence/evidence-followups-5899e2a/`): muted text on the hovered
selected row peaks at 4.57-4.68 in the Author column in all seven themes, the
named pairs; the dates in the same row peak at 4.22-4.35, and the monospaced SHA
reaches the declared ratio. Warning text on subtle reaches its declared
4.84-4.92. The named pairs are read from the Author column, as the September 18
table was. All three columns of the hovered selected row:

| theme | declared | Author (named pair) | Date | SHA |
| --- | --- | --- | --- | --- |
| Solarized Dark | 4.809 | 4.580 | 4.318 | 4.809 |
| Solarized Light | 4.931 | 4.634 | 4.229 | 4.931 |
| One Dark | 4.774 | 4.568 | 4.305 | 4.774 |
| One Light | 4.981 | 4.681 | 4.291 | 4.981 |
| Rosé Pine | 4.789 | 4.621 | 4.350 | 4.789 |
| Rosé Pine Dawn | 4.905 | 4.618 | 4.220 | 4.905 |
| Dracula | 4.778 | 4.580 | 4.304 | 4.778 |

In each theme 2 of the Author column's 258 ink pixels reach 4.5; the "Sep 01"
dates have none. The 1x loss is 0.17-0.30 in the Author column but 0.44-0.70 in
the dates, more than the "about 0.2" of `DESIGN.md`'s tuning sentence. No named
pair is below 4.5, so no further step is due. Whether dates and other thin glyphs
should also reach 4.5 at 1x is an owner decision: it would take about 0.5 more
declared margin, which One Dark (text at parity) and Kanagawa Lotus (upstream
limits) cannot give without tuning more upstream colors, and the coordinator
declined that on 2026-09-23.

The failure line of a refused import reaches full coverage, so warning text on
`subtle` peaks at its declared ratio: Kanagawa Lotus 4.886, Rosé Pine Dawn
4.894, One Light 4.916, Solarized Dark 4.844 and Solarized Light 4.908.
