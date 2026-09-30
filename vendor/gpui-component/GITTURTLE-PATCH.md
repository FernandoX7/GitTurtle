# GitTurtle patch to gpui-component 0.6.0

This directory contains the exact crates.io `gpui-component` 0.6.0 source under Apache-2.0. `LICENSE-APACHE` is byte-for-byte preserved, and `.cargo_vcs_info.json` identifies published source revision `94a313a72a2513aee2780240cd322d552b2395f0` (`crates/component`). The registry archive checksum is `9bd7938c395be32cea6127b92f7206501ad1fe7e121a609de1aafdedcb4d98be`. Dependency versions are unchanged. The registry-only `.cargo-ok`, `.cargo-checksum.json`, and package lockfile are omitted; GitTurtle uses the workspace lockfile.

The input patch modifies `src/input/input.rs` and `src/input/state.rs`. The styled Input previously attached its role, name, author identifier, placeholder, value and SetValue action to the surrounding frame, while the shared text editing state owned the active keyboard FocusHandle on a different element without a role. GPUI 0.3.4 only records a focused accessibility node when that same element has a node; otherwise its tree falls back to the window root. This was a separate source defect from the macOS first-responder adapter attachment.

The component now projects its resolved metadata to `gpui-base::input::InputBaseState` through the matching local base patch. The existing editing element exposes the field semantics and SetValue action. The frame keeps its distinct focus handle for containment and focus-ring behavior and becomes presentational. Input, Textarea and Editor continue to use their existing shared editing engine, key bindings, selection and focus handle. No duplicate focus handle registration or native focus changes are introduced.

The projection contains no text copy. Values remain lazily materialized only while an accessibility client is active; masked inputs and Password/NewPassword content types never expose values, including when their visible mask is switched off. Disabled and read-only fields do not advertise SetValue; the existing editing engine still enforces its mutation rules. Existing component regression probes now inspect the editing-state node, with the original action helper retained only for tests.

`cargo test --locked -p gitturtle input_semantics_follow_the_editing_focus_owner` checks the actual component-to-state projection, editing focus, role, updated names, author identifier, placeholder, presentational frame, and editable/disabled/read-only/password/multiline action metadata in a synthetic GPUI window. The platform has no active assistive client, so this is rendered-node and keyboard-focus evidence, not native VoiceOver evidence. macOS AX focused-element and VoiceOver checks remain a separate package gate. Remove the component and base input patches together once a matching upstream version supplies equivalent semantics and those regressions pass.

The control-geometry patch modifies `src/button/button.rs` and `src/icon.rs`.
Button's inner content previously replaced an explicitly supplied text size and
icon/label gap with its size-category defaults; Icon similarly replaced explicit
pixel geometry when its containing Button supplied a default icon size. The
inner content now honors the caller's explicit font size and horizontal gap,
and styled icon dimensions take precedence over a component's default size.
Controls without explicit overrides retain their original size-category behavior.
No palette, focus, disabled, loading or action behavior changes.

`cargo test --locked -p gitturtle button_content_preserves_explicit_type_and_icon_geometry`
lays out real component Buttons and observes their inherited text size and
centered icon geometry in a synthetic GPUI window. It covers unchanged defaults
alongside distinct explicit font/icon sizes. Actual rendered controls across
themes, densities and enlarged interfaces still require native inspection.

The focus-handle patch also modifies `src/button/button.rs`. Upstream Button
always tracks a focus handle it keeps in keyed element state, and GPUI tracks
one handle per element, so a caller's `track_focus` on a Button (the
`InteractiveElement` method) was silently replaced and the caller could not
tell whether its Button held focus or move focus to it. `Button::track_focus`
now takes a caller-owned handle, with the name and signature of gpui-base's
`Button::track_focus`, and the Button tracks it instead of creating keyed
state. Buttons that do not call it keep the keyed-state handle unchanged,
including their refusal of focus on mouse down. Nothing else changes: tab
order, the focus ring and click handling follow whichever handle is tracked.

Two app paths opt in. The Settings picker cards track the app's per-card
handles (`GitTurtle::theme_card_focus`), so the picker finds the card that
holds keyboard focus and draws its ring; the Your themes row actions track
app-owned handles, so focus can be placed on an action of a row that is not
drawn yet. `cargo test --locked -p gitturtle keyboard_focus_rings_the_picker_card_outside_its_border`
tabs through the picker cards on their app handles and checks where focus
lands and the ring the focused card draws.

Remove this part of the patch when upstream Button honors a caller-owned focus
handle, or when the Settings picker and the Your themes rows no longer need to
own their Buttons' focus.

The Button focus-ring patch modifies `src/styled.rs`, `src/theme/mod.rs` and
`src/button/button.rs`. Upstream Button draws its focus ring through
`focus_ring_style`, the ring every control shares: 3 px of the theme's `ring`
at half opacity directly outside the border, which takes full `ring`. Primary,
danger, ghost and custom Buttons draw no border, so for them the half-opacity
ring is the whole focus indicator, and on the surfaces GitTurtle places Buttons
on it falls below 3:1 in most palettes. Upstream offers no way to change the
ring for Buttons alone.

`Theme::button_focus_ring` is a `FocusRing` of `width`, `gap` outside the
border and `opacity` of `ring`, set in code and skipped when a theme file is
read or written. Its default is the shared ring, 3 px, no gap and 0.5, so an
application that leaves it alone draws exactly the upstream ring. A focused
Button, disabled or not, draws the setting through the same absolutely
positioned child `focus_ring_style` draws, which takes no layout space.
`focus_ring_style` passes the default through that shared code, so it and its
other callers (Input, Select, Checkbox, Radio, Combobox, NumberInput,
DatePicker and OtpInput) draw as before, and the focused Button's tinted border
is unchanged. `Theme::focus_ring = false` and `Button::focus_ring(false)` still
draw no ring.

`cargo test --locked -p gitturtle focused_buttons_draw_the_theme_button_focus_ring`
focuses a borderless, a primary and a disabled Button in a synthetic GPUI
window and reads the ring each paints from the rendered scene, at the default
and at 2 px, 1 px outside and full opacity. It also requires the Buttons' and
labels' bounds and the Buttons' radii not to move when focused, an unfocused
Button to draw no ring, `focus_ring_style` to keep the default ring under
either setting, and both switches to turn the ring off. How the ring renders
in the real window still needs native inspection.

The app adopts the setting where its palette application sets `ring`
(`crates/app/src/appearance.rs`), in task `app-button-focus-ring`; until then
it keeps the default. Remove this part of the patch when upstream Button draws
a focus indicator of at least 3:1 against its surroundings or offers an
equivalent setting.

The disabled-hover patch also modifies `src/button/button.rs`. A caller's
`hover` on a Button is GPUI's `InteractiveElement::hover`, which GPUI refines
over the element's base style, where the disabled style lives; the kit gates
only its own variant hover on the Button being enabled. A disabled Button with
a caller hover therefore lit up under the pointer as though it were available,
and the app cannot clear the style because `Interactivity::hover_style` is
private to GPUI. `Button::hover` now takes the caller's style with the same
signature, shadowing the trait method at every call site on a Button, and
applies it in the same order as before while the Button is enabled. While it is
disabled the hover stays registered but refines nothing, so the Button keeps its
disabled look under the pointer and while pressed. Enabled Buttons are
unchanged, and group hover styles are not affected.

`cargo test --locked -p gitturtle caller_hover_styles_only_enabled_buttons`
hovers and presses an enabled and a disabled selected Button, each with a
caller hover fill, in a synthetic GPUI window and reads their fills from the
rendered scene. Four app call sites resolve to the method: the shared `button`
helper's selected hover (`crates/app/src/main.rs`) and the selected mode,
density and worktree Buttons in `projects.rs`, `settings.rs` and
`worktrees.rs`. Remove this part of the patch when upstream Button keeps caller
hover styles off a disabled Button, or GPUI lets the kit clear them.

The disabled-focus patch also modifies `src/button/button.rs`. gpui-base's
Button tracks its focus handle only while it is enabled (`gpui-base`
`src/button.rs`, `.when(!disabled && self.focusable, …)`), so a Button that
turns disabled while it holds focus drops out of the rendered frame with the
window's focus still on it. GPUI 0.3.4 then dispatches keys from the window's
root dispatch node (`Window::focus_node_id_in_rendered_frame`), which lies above
`Root`'s `Root` key context, so Tab and Shift+Tab match no binding and focus
stays on the dimmed Button until a pointer click. Targets' Switch reaches this
after every completed switch, which clears the branch field that enables it.

While a disabled Button holds focus, it now tracks its handle through
`InteractiveElement::track_focus`, which gpui-base's disabled path leaves in
place, as a target that is not a tab stop. Keys dispatch through `Root` again,
and `focus_next` and `focus_prev` step from the Button's own place in the tab
order to its neighbours; Tab never lands on a disabled Button. The Button gains
no click, Enter or Space activation, a press on it still stops at its disabled
mouse-down handler, and it keeps the focus ring it already drew. AccessKit
reports the focused, disabled Button, with its Focus action and no Click,
instead of falling back to the window root. Every ancestor key binding reaches
it again, as for an enabled focused Button; no handler relies on a disabled
Button to block a write. Setting `tab_stop(false)` on the handle also writes
its window-wide record, which gpui-base's enabled path rewrites on every render,
so a Button that is enabled again is a tab stop again. Once focus leaves, and
for every enabled or unfocused Button, rendering is unchanged.

gpui-base's Checkbox, Switch, Radio, Toggle, Link and ColorPicker keep the same
enabled-only focus guard and are not patched here. The app disables a focused
Switch or Checkbox in Settings' Follow system (while the Omarchy theme is
selected), the diff view's partial-line Checkbox, the ignore dialog and the
profile editor.

`cargo test --locked -p gitturtle tab_and_shift_tab_leave_a_focused_switch_that_turns_disabled`
renders the application's Targets, tabs onto Switch, clears the branch field,
and sends Tab and Shift+Tab as keystrokes, requiring focus to reach the Remote
field past the disabled Create and to return to the branch field. Native
keyboard behavior needs its own run. Remove this part of the patch when
gpui-base keeps a focused disabled Button's handle in the frame, or GPUI moves
Tab and Shift+Tab on from a focused element that is no longer rendered, and
that regression passes without it.

The tooltip-width patch modifies `src/tooltip.rs`. Upstream Tooltip draws
its popup as wide as its unwrapped text, with a `m_3` margin inside the element
gpui-base's `TooltipPositioner` places. The positioner centres that element on
the trigger and clamps it into the viewport less its 4 px `WINDOW_MARGIN`, plus
the client inset under client-side decorations; an element wider than that
keeps only its left edge in view, so in a narrow window the rest of the tooltip
ran off the window's right edge. In a 461 px window this cut off the tooltips
of History's compact Latest and Older controls and of Compare's collapsed
review Options button, which carries the options' explanations. The app's
element tooltips render the same Tooltip view
(`div().id(..).tooltip(|window, cx| Tooltip::new(..).build(window, cx))`, as
on file rows, the review caption, blame, file history, the project pane,
Settings and the workspace), so they pass through the cap too.

The popup's width is now capped at the viewport width less twice the
positioner's margin, the client inset and the popup's `m_3` (0.75 rem at the
window's rem size). A wider tooltip wraps its text, left-aligned, where GPUI's
line wrapper breaks a line: at spaces, before a non-word character such as `/`,
which then starts the next line, and inside a word that has no such break and
is wider than the popup on its own. The text child can shrink below its
unwrapped width (`min_w_0`) so it wraps instead of overflowing the popup. A key
binding stays on the right of the first line: an empty strut in the popup's
text style makes its row one line tall, and the binding is centred in it as the
popup centres it beside a single line. A tooltip narrower than the cap keeps
its layout exactly, key binding included, and its text style, padding, border,
radius and shadow are unchanged. The positioner, its placement and flipping,
the enter and switch animations and hide-on-press are untouched, and the cap is
computed only while a tooltip renders. gpui-base keeps its margin private, so
the kit repeats the 4 px value; the regressions below fail if the two differ.

A control's tooltip, placed by the positioner, therefore wraps inside the
window with equal margins on both sides. An element tooltip wraps to the same
width, but GPUI places it itself (`Window::prepaint_tooltip`): below and right
of the pointer, flipped to the pointer's left when it overflows the window, and
at x = 0 when that overflows too, with no margin and no client inset. Near an
edge it can sit flush with that edge, inside only the popup's own margin, and
its two margins differ. An element tooltip whose wrapper was within 8 px of the
viewport's width (8 px plus twice the client inset under client-side
decorations) fitted on one line before and now wraps.

`cargo test --locked -p gitturtle tooltips_wider_than_the_window_wrap_inside_it`
lays out real kit Tooltips in the real positioner in a synthetic window, at
16 and 18 px rems. In a 461 px window the Options tooltip, clamped at either
edge and with a key binding, lies inside the viewport inset by the positioner's
margin and the popup's own on both sides, above its trigger, and wraps to more
than one line; a short tooltip keeps its unwrapped size, and every tooltip
keeps one line in a wide window.
`cargo test --locked -p gitturtle narrow_tooltips_wrap_inside_the_window`
hovers the compact Columns, Latest and Older controls and the Options button
in a 461 × 490 app window: Columns keeps one line of the popup's text style,
and the others wrap above their controls with the same inset. Both allow a
device pixel on each edge. The margins are equal only to within about 1.5 px
after rounding to device pixels, and the app test's hovered controls all sit
left of the window's centre; the synthetic test's triggers cover both edges.
Element tooltips' placement is GPUI's and no retained regression covers it.
No app tooltip carries a key binding, and its glyphs paint no quad the tests
can read, so its placement is not covered by a retained regression either.
Rendered tooltips across themes and interface sizes still require native
inspection. Remove this part of the patch when upstream Tooltip keeps a popup
wider than the window inside it, or its positioner constrains the popup's
width.
