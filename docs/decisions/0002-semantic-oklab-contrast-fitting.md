# ADR 0002: Semantic Palette Ownership, OKLab Hover Ordering, and More Contrast

- **Date:** 2026-11-05 (consolidated 2026-11-21)
- **Status:** Accepted

## Context
Shipping 20 built-in themes plus live Linux Omarchy theme sync and custom themes exposed recurring contrast regressions: raw upstream palettes often failed WCAG text/icon floors on selected rows or diff tiles, hover fills sometimes outweighed selected rows, and toolkit controls diverged from custom canvas surfaces.

## Decision
- Own all semantic palettes centrally in `crates/app/src/appearance.rs` and synchronize `ThemeTokens` + `ThemeColor` before `Theme::sync_base` on every theme apply.
- Fit syntax roles toward `text` to `>= 4.75:1` (`>= 4.5:1` floor) and fit editor/Find/changed-word highlights to `>= 0.04 ΔEOK` separation while keeping `>= 4.5:1` contrast.
- Enforce that `hover` always sits strictly closer to `canvas`/`panel`/`subtle` than `selected` in OKLab lightness, ΔEOK, and luminance contrast, moving away from `canvas`.
- Implement **More contrast** via non-mutating `Palette::with_more_contrast()` raising resting text/syntax floors to `7:1`, borders/edges to `3:1`, disabled labels to `4.5:1`, and adding `1 px` bottom rules on changed-word diff spans.

## Consequences
- Built-in and Omarchy themes meet deterministic readability guarantees without hand-tuning every UI component.
- Custom themes remain unadulterated on disk while surfacing accurate advisory readability findings in the editor.
