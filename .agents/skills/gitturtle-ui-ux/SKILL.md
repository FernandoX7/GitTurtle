---
name: gitturtle-ui-ux
description: GitTurtle UI/UX design tokens, OKLab/WCAG contrast rules, focus rings, narrow layouts, and VoiceOver/Orca accessibility standards. Use for any visual, theme, layout, control, or accessibility change in crates/app or vendor/gpui-*.
---

# GitTurtle UI/UX and accessibility

Read [DESIGN.md](../../../DESIGN.md) and [crates/app/AGENTS.md](../../../crates/app/AGENTS.md) before changing layout, typography, colors, motion, focus, or accessibility.

## Living design invariants

- **Dense full-height workspace:** GitTurtle prioritizes crisp information density, persistent context, and immediate first-frame response without decorative chrome.
- **Tokens and geometry:** Follow the shared spacing scale, `7/12 px` corner radii, `28/34/36 px` control heights, and `13/12 pt` type scale. Avoid inventing one-off hex colors or arbitrary paddings.
- **Contrast floors:** Maintain WCAG contrast floors (`4.5:1` primary text, `3:1` UI components/large text, `2:1` subtle dividers/disabled boundaries) using resolved native component pairs — check foreground against the actual component background (`ThemeTokens` + `ThemeColor` synchronized in `ThemeChoice::configure` before `Theme::sync_base`), never palette swatches alone.
- **Focus & keyboard:** Every interactive control must expose a clear focus ring, logical keyboard tab order, and once-per-press activation semantics (no held-Enter / X11-repeat duplicate writes). Escape closes overlays and returns focus predictably.
- **Responsive narrow viewports:** Test narrow and half-tile layouts across multiple window sizes and scale factors (never a single size); ensure controls clip, wrap, or collapse gracefully without overlap or last-frame scroll feedback loops.
- **Assistive technology parity:** Expose matching roles, names, descriptions, modal flags, and disabled states for both macOS VoiceOver (`accesskit_macos`) and Linux Orca/AT-SPI (`accesskit_unix`). Keep `Role::TextRun` segments ≤255 graphemes linked with `previous_on_line`/`next_on_line`.

## Testing & open backlog awareness

- Prove color/focus/layout changes with `#[gpui::test]` using `ThemeChoice::Midnight.apply(None, cx)` + `window.painted_quads()` and `debug_bounds` (see [gitturtle-gpui-testing](../gitturtle-gpui-testing/SKILL.md)).
- Keep known open UI/a11y deficits in view rather than assuming parity: macOS VoiceOver off-screen clip bounds & alert readback need real macOS verification, Linux portal `high-contrast` -> `MoreContrast` wiring is pending, and narrow half-tile (`461 × 490`, 18 pt) project-pane layout remains tracked in [docs/native-accessibility.md](../../../docs/native-accessibility.md).
- Tag every UI/input/a11y PR explicitly with which host/backend was exercised vs unverified (e.g., `"Verified on Linux XWayland | Unverified on macOS / Wayland"`).
