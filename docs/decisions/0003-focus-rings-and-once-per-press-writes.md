# ADR 0003: Device-Pixel Focus Rings and Once-Per-Press Write Protection

- **Date:** 2026-11-15 (consolidated 2026-11-21)
- **Status:** Accepted

## Context
Native testing at fractional display scales (`1.25×`, `1.5×`, `1.75×`) and on Linux X11/Wayland revealed clipped outer focus rings, accidental double-writes from X11 synthetic key repeats or rapid double-clicks when list rows shifted under a stationary pointer, and stale pointer releases after background diff refreshes.

## Decision
- Paint a uniform `2 px` accent focus ring `1 px` outside interactive controls, reserving `3 px` rounded up to whole device pixels inside every clipping parent and between neighboring tab-strip controls.
- Trigger keyboard write activations (`Enter`/`Space` on buttons, dialogs, menus, diff line staging) once per physical key press, ignoring held-key repeats on Wayland, macOS, and X11 and withholding activation across dialog open/close transitions until key-up.
- After single-row Stage/Unstage shifts rows beneath a stationary pointer, suppress hover, tooltips, and pointer clicks on newly arrived write controls until the pointer moves more than `5 px` along either axis.
- Bind `Stage hunk`, `Unstage hunk`, and diff checkbox presses strictly to the diff revision painted in the frame where the pointer pressed down.

## Consequences
- Focus indicators remain crisp and whole across every supported display scale.
- Accidental duplicate writes or writes targeting shifted rows/hunks are structurally eliminated.
