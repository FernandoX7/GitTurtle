# Keyboard, menus, and accessibility

Living specification for keyboard navigation, once-per-press write protection, menus/popovers, command palette, and assistive-technology contracts. Root summary: [DESIGN.md](../../DESIGN.md#keyboard-contract).

## Keyboard focus & once-per-press activation

- Every shortcut lives in [`crates/app/src/shortcuts.rs`](../../crates/app/src/shortcuts.rs) (`Cmd` on macOS, `Ctrl` on Linux) and is discoverable via the Command Palette and Keyboard Shortcuts dialog.
- Lists with keyboard selection and the commit message editor each occupy one Tab stop in visual order; arrow keys navigate within lists.
- Opening a modal traps Tab/Shift-Tab across its interactive stops (wrapping cleanly regardless of background stop count), suppresses background shortcuts, and hides background page nodes from assistive technology until the modal closes.
- **Once-per-press write guard:** `Enter` and `Space` activate buttons, dialog confirmations, menus, popovers, and partial-staging controls only on a fresh key-down originating on that control — suppressing held-key repeats on Wayland, macOS, and X11. Opening or closing a dialog while `Enter` is held suppresses activation until that key releases (or an explicit fresh press occurs), preventing accidental confirmation or writes on newly focused targets.
- Read-only and disabled text inputs refuse IME text input (`accepts_text_input() == false`) so Wayland input methods (`fcitx5`) stay off and shortcuts/repeats work natively.

## Menus, popovers, and tooltips

- Menus size to their longest row up to `500 px`, clamped strictly inside the window minus an `8 px` margin on every side (counting the `1 px` drawn edge, snapped to whole device pixels) and above any application bottom inset, with a `4 px` base gap at `13 pt` scaled through `Theme::popup_gap` and device-pixel snapped.
- Open menus trap `Tab` and `Shift+Tab` (moving nothing), open popovers wrap focus within their own interactive stops, and keyboard-opened menus initialize selection on their first actionable item.
- Long menu rows truncate to one line with an ellipsis while retaining full text as their accessible name. Capped scrolling menus preserve whole visible rows and an always-visible scrollbar.
- Command Palette rows measure `54 px` scaled with interface text, sizing adaptively from 1 to 7 rows and highlighting the first runnable command when opened or filtered.
- Menu section labels render at `11 px` medium in plain secondary text (`Role::Label`, skipped by `Up`/`Down`/`Home`/`End`), distinct from actionable items (`11.4 px` regular) and disabled items.
- **Keyboard cursor vs pointer hover:** Keyboard navigation draws a `selected` fill plus a `2 px` leading `accent` marker (`>= 3:1`); pointer hover draws the quieter `hover` fill without a marker. When a menu opens or rebuilds beneath a resting pointer, the resting pointer never steals selection until it moves.
- Disabled items explaining why an action is blocked (`reason_stated`) remain reachable by keyboard arrows so screen-reader and keyboard users hear the explanation, while `Enter` and click do nothing.
- Context menus and fallback input menus open cleanly below or wholly above the pointer row (never covering the clicked row), closing on window resize and firing clicks only on the item where the pointer pressed down.
- Popovers and menus clip shadows to their side of the opener so shadows never darken their trigger button or row.

## Assistive technology (VoiceOver & Orca/AT-SPI)

- Custom controls expose explicit stable IDs, semantic roles, concise accessible names, and descriptions. `Role::Label` nodes carry text in their value property.
- Checkable menu items expose `Role::MenuItemCheckBox`; plain unticked items remain ordinary menu items.
- Editable text fields publish bounded cached `Role::TextRun` segments (`<= 255` graphemes per run, linked via `previous_on_line`/`next_on_line`, omitted on secret fields) so AT-SPI exposes the Text interface cleanly without unbounded per-frame re-segmentation.
- See [docs/native-accessibility.md](../native-accessibility.md) for current platform verification notes and open a11y backlog items.
