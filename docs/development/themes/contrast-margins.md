# Adapted palettes render below their contrast floors at 1x — September 18, 2026

Status: recorded, not fixed. Decided by the repository owner on 2026-09-18: ship
the batches as their contracts specify and correct every near-floor palette in
one follow-up rather than re-opening accepted work. This note is what that
follow-up needs.

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
