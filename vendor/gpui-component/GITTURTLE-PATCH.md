# GitTurtle patch to gpui-component 0.6.0

This directory contains the exact crates.io `gpui-component` 0.6.0 source under Apache-2.0. `LICENSE-APACHE` is byte-for-byte preserved, and `.cargo_vcs_info.json` identifies published source revision `94a313a72a2513aee2780240cd322d552b2395f0` (`crates/component`). The registry archive checksum is `9bd7938c395be32cea6127b92f7206501ad1fe7e121a609de1aafdedcb4d98be`. Dependency versions are unchanged. The registry-only `.cargo-ok`, `.cargo-checksum.json`, and package lockfile are omitted; GitTurtle uses the workspace lockfile. `upstream-files/` adds two files of that revision's repository that the package omits and the crate's tests include, test data only, described in the test-layout part at the end of this note.

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
other callers (Select, Radio, Combobox, NumberInput, DatePicker and OtpInput)
draw as before, and the focused Button's tinted border is unchanged. Input and
Checkbox draw the setting too, through the field and checkbox focus-ring patch
below. `Theme::focus_ring = false` and `Button::focus_ring(false)` still
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

The stateful-element patch also modifies `src/button/button.rs`. `Button`
implements `StatefulInteractiveElement`, as the base Button it wraps does:
upstream gives it `InteractiveElement` alone, and GPUI keeps the accessibility
states (`aria_expanded` and the others) on the stateful trait, so an application
could not write a disclosure state on a Button's node. The Button's inherent
methods shadow the trait's where both exist, so no other call changes. Blame's
compact source line (`GitTurtle::blame_source_scope`) is the consumer, and
`cargo test --locked -p gitturtle
blame_s_compact_source_line_says_whether_its_note_is_open` reads the node the
Button writes. Remove this part when upstream Button implements the trait.

The accessibility-disabled patch also modifies `src/button/button.rs`.
`Button::accessibility_disabled` passes its flag to the base Button's matching
setter (see [the base patch](../gpui-base/GITTURTLE-PATCH.md)), which reports
the node disabled while the Button keeps its enabled look, hover, focus and
click handler. GitTurtle refuses a press of a control a write leaves
unavailable in the control's own handler at once, but dims the control only
once the write has run 200 ms (`workspace::refusing`); without the flag an
assistive technology heard those controls enabled through the delay. Buttons
that do not set it are unchanged. `cargo test --locked -p gitturtle
a_refusing_control_is_announced_unavailable_before_it_dims` renders Buttons
announced unavailable alone, dimmed, and neither, to the nodes they write.
Remove this part with the base's when upstream Button separates its
accessibility state from its disabled styling.

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

gpui-base's Checkbox, Switch, Radio, Toggle, Link and ColorPicker swatch kept
the same enabled-only focus guard; the [base patch](../gpui-base/GITTURTLE-PATCH.md)
applies this rule in gpui-base, where they own it, so this crate's Checkbox and
Switch wrappers are unchanged.

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

The tooltip-through-press patch modifies `src/tooltip.rs` and
`src/button/button.rs`. Upstream's managed tooltip hides on mouse down, shown or
still waiting out its delay, and asks for it again only when the hover changes,
so after a click the tooltip stays hidden until the pointer leaves and enters
again; a small move inside the trigger does not bring it back. A text tooltip
is also built from the words the Button had when the pointer entered, so one
already shown kept them. An icon-only copy button answers its press in its
tooltip ("Copied", `DESIGN.md`'s copy feedback), which the press itself hid.
`ManagedTooltipExt::managed_tooltip_with_placement` gains `hide_on_press`, true
for every existing caller. `Button::tooltip_through_press` sets it false, so a
press leaves the tooltip shown, or still showing once its delay is up, while
the pointer stays on the Button. `Button::tooltip_view` sets the existing
`tooltip_builder`, which upstream keeps without a setter: the overlay calls it
each time the tooltip renders, so its content follows state that changes while
it is shown. Buttons that set neither behave as before.

The consumer is `copy_feedback::copy_button` (`crates/app/src/copy_feedback.rs`)
for an icon-only copy button, such as the compact inspector's Hash and Copy
message and the Compare header's copy-path icon.
`cargo test --locked -p gitturtle an_icon_only_copy_says_copied_in_its_tooltip_while_hovered`
hovers the compact Hash at 18 pt in a 461 × 490 window until its tooltip shows,
clicks it, and requires the tooltip to read Copied through the 1.5 s and its own
words after, then clicks it within the tooltip's delay and requires Copied once
the delay is up. It fails with the press hiding the tooltip and with the text
fixed when the pointer entered. Remove this part of the patch when upstream lets
a trigger keep its tooltip through a press and builds a shown tooltip's content
as it renders.

The disabled-switch patch modifies `src/switch.rs`. Upstream Switch fades a
disabled Switch's track to half opacity (`disabled_bg`) and paints its thumb in
`switch_thumb` at full strength. Its comment reasoned that the thumb is
`background`, so fading the track alone lands on the pixels a grouped fade
would, but the thumb is `switch_thumb`, which GitTurtle's palettes set to their
text color (`crates/app/src/appearance.rs`). A disabled Switch therefore drew a
full-strength thumb on a faded track and read as available. The consuming path
is Settings' Follow system appearance switch, which Settings disables while the
Omarchy card is selected (`.disabled(desktop_theme)` in
`crates/app/src/settings.rs`); every other disabled Switch gets the same fade.

While a Switch is disabled its thumb now takes `switch_thumb` at the track's
0.5, through the thumb's disabled state style (`SwitchThumb::disabled`), and the
comment names the thumb's actual token. GPUI multiplies each primitive's alpha
instead of compositing the control as one group, so the faded track shows
through the faded thumb; that is accepted. An enabled Switch paints exactly as
before, on or off, and nothing else changes: geometry, the thumb's travel, the
label, the disabled label color and cursor, activation and focus.

`cargo test --locked -p gitturtle a_disabled_switch_fades_its_thumb_with_its_track`
selects the Omarchy theme from a fixture in the application's Settings, which
disables Follow system, and reads the switch's track and thumb fills from the
rendered scene: both are the palette's at half opacity. With Midnight selected,
off and on, the enabled switch paints its track and thumb at full strength. It
runs on Linux only, where the Omarchy theme exists. How the faded switch renders
in the real window still needs native inspection. Remove this part of the patch
when upstream fades a disabled Switch's thumb with its track, and that
regression passes without it.

The checked-knob and disabled-label patch also modifies `src/switch.rs`.
Upstream paints the thumb in `switch_thumb` whether the Switch is on or off,
and a disabled Switch's label in `muted_foreground`. GitTurtle's palettes set
`switch_thumb` to their text colour (`crates/app/src/appearance.rs`), so on the
checked accent track the knob all but vanished: 1.37:1 in Midnight, and 1.0:1 in
the Omarchy Kanagawa theme, whose accent is its text colour. And the Omarchy
mapping may set secondary text as strong as text (Catppuccin Latte), so the
disabled Follow system appearance label did not dim. The consumers are every
Settings Switch (`crates/app/src/settings.rs`).

A checked thumb now takes `primary_foreground`, the colour primary Buttons put
on the accent (`thumb_color`); an unchecked thumb keeps `switch_thumb`. A
disabled Switch's label takes the dim disabled Buttons and menu items share,
`Theme::disabled_foreground` (`disabled_label_color`; see the
disabled-foreground patch below). Both functions are public so the
application's tests can read what the Switch paints with. Geometry, travel,
activation, focus and the unchecked look are unchanged.

`cargo test --locked -p gitturtle every_palette_paints_a_checked_switch_knob_that_reads_on_its_track`
applies every built-in palette and every embedded Omarchy fixture's palette,
draws a checked, an unchecked and a disabled checked Switch, and reads their
track and thumb fills from the scene; the checked thumb is the accent's label at
3:1 or more on its track.
`every_palette_shares_one_disabled_dim_that_reads_three_to_one` checks the
disabled label colour in the same palettes; GPUI's test scene exposes quads
but not glyph colours, so it reads the colour the Switch paints its label with
rather than the painted glyphs. Native frames record the rendered result. Remove
this part of the patch when upstream gives a checked thumb a colour that
contrasts with the accent and fades a disabled label from the enabled one, and
those tests pass without it.

The switch focus-ring and hover patch also modifies `src/switch.rs`, and adds
`Theme::switch_hover` to `src/theme/mod.rs`. Upstream Switch draws no focus
indication and no hover change, although the base Switch is a tab stop, so a
Tab onto a Settings switch showed nothing. The wrapper now owns the base
Switch's focus handle (the base patch's `Switch::track_focus`). While that
handle is focused and the Switch is enabled, its track draws
`Theme::button_focus_ring` through the shared `apply_focus_ring`, the same
absolutely positioned child a Button draws, which takes no layout space; the
border that helper tints is the track's transparent 2 px inset around the
thumb, so it stays transparent. An enabled, unchecked Switch's track takes
`Theme::switch_hover` while the pointer is over the Switch, its label included
(the root forms a group). `switch_hover` is `None` by default, set in code and
skipped by theme files, so an application that leaves it alone keeps the
resting fill. A checked track keeps its accent and a disabled Switch shows
neither ring nor hover, keeping its faded look.

The consumers are the Settings Switches (`crates/app/src/settings.rs`); the app
sets `switch_hover` to `Palette::switch_hover` where it installs the button
ring (`crates/app/src/appearance.rs`).
`cargo test --locked -p gitturtle a_focused_switch_draws_the_button_ring_whole_and_moves_nothing`
tabs through the real Settings page at 13 and 18 pt and reads the scene: each
enabled Switch draws the ring 3 px outside its track, whole inside its clips,
while no other quad moves and no unfocused Switch draws one; it then hovers
Follow system unchecked, checked and, on Linux, disabled by the Omarchy theme.
`every_palette_lifts_a_hovered_switch_track` requires the hovered track to move
at least 6 of 255 in one channel in every palette. Remove this part of the
patch when upstream Switch draws a focus indicator and a hover state, or offers
equivalent settings, and those tests pass without it.

The field and checkbox focus-ring patch modifies `src/input/input.rs`,
`src/checkbox.rs` and the documentation of `src/styled.rs` and
`src/theme/mod.rs`. Upstream Input and Checkbox draw `focus_ring_style`'s
ring, 3 px of the theme's `ring` at half opacity directly outside the border.
Over the canvas, panel or `subtle` that half ring read under 3:1 in 16 of
GitTurtle's 20 built-in palettes, More contrast off and on (Porcelain 2.27:1,
Sandstone 2.06:1, Kanagawa Lotus 1.88:1, Braden 2.09:1), where the Switch,
drawing `Theme::button_focus_ring` at full strength, held 3:1. A focused,
bordered Input and a focused Checkbox now draw `Theme::button_focus_ring`
through the shared `apply_focus_ring`, as a Button and a Switch do: the same
absolutely positioned child, which takes no layout space. Their own edges are
unchanged: a focused Input's 1 px border still takes full `ring`, as a focused
bordered Button's does, and a Checkbox's indicator, which the ring surrounds
with its label, keeps its border. At the setting's default the ring is
upstream's; GitTurtle's, 2 px with a 1 px gap, takes the same 3 px outside the
edge, so the room containers keep for it is unchanged. `Theme::focus_ring =
false` and `focus_ring(false)` still draw no ring, and key, focus, Enter and
accessibility handling are unchanged.

The consumers are every bordered kit Input the app draws, such as Create
branch…'s name, the Reflog's, the worktree manager's and Tags' filters and the
rebase base field, and every kit Checkbox, such as those in Settings and the
branch, ignore, recovery, profile, Compare and editor Find forms; the app
installs the setting where it applies a palette (`crates/app/src/appearance.rs`).
`cargo test --locked -p gitturtle every_palette_draws_kit_focus_rings_at_three_to_one`
focuses a field, a checkbox and a Switch in every built-in palette, More
contrast off and on, and composites each quad they paint in the ring colour
over the canvas, panel and `subtle`, requiring 3:1 with `ring` the palette's
accent. `focused_fields_and_checkboxes_draw_the_theme_button_focus_ring` reads
the ring a focused field and checkbox paint at the default and at GitTurtle's
setting (width, gap, corners and colour), the field's edge in `ring`, nothing
moved, and no ring with `Theme::focus_ring` off. `focus_ring::assert_input_ringed`
requires the installed ring, whole inside every clip, around the app's own
fields in the Reflog, worktree and interactive rebase tests. How the ring
renders in the real window still needs native inspection. Remove this part of
the patch when upstream Input and Checkbox draw a focus indicator of at least
3:1 against their surroundings or follow an equivalent setting, and those
tests pass without it.

The switch-row patch also modifies `src/switch.rs`. Upstream Switch draws only
a label beside its track, at the kit's text size, and sets no accessible
description. A Settings row drew its title and description beside a bare
Switch, so only the 36 × 20 px track answered the pointer and the row's text
was read apart from the switch; Follow system appearance also drew its words
as the Switch's label, which at 18 pt in a 461 px window wrapped its title to
three lines. `Switch::leading` now draws an element before the track inside
the base Switch, which then fills its width with the track at its trailing
edge and its top on the leading content's top, so the track stays on a
settings row's title line however many lines the description wraps to
(centred on the row, it drifted down with each one). `Switch::place_track`
hands the track to a caller's function, whose element takes the track's place
there: a Settings row wraps it in a slot one title line tall that centres it
on that line, as the row's other trailing controls are (with its top on the
line, the track sat 3.5 px above the title's cap centre at 18 pt), without
letting it rise above the row's top where it is taller than the line, as at
13 pt, and the track keeps its size, state, focus ring and hover. A press
anywhere on the row toggles it, the hover lift covers the row (the root's
group), a disabled Switch blocks the row and gives its text the disabled label
colour wherever it sets none of its own, and the leading text is drawn but not
read, provided no element in it has an ID (one that has writes a node of its
own, read apart from the Switch) and the label stays on its default side, the
right (a label on the left reverses the row, putting `leading` after the
track).
`Switch::accessibility_description` sets the base Switch's `aria_description`,
which assistive technology reads after the name. Without `leading`,
`place_track` or a description the Switch renders as before.

The consumers are the four Settings switch rows, three outside Linux, where
the desktop's monospace font row is not offered, and the disabled-dim probe
(`settings::switch_row`, `crates/app/src/settings.rs` and
`crates/app/src/appearance.rs`).
`cargo test --locked -p gitturtle each_switch_row_says_its_setting_once`
walks Settings at 13 and 18 pt in a 461 and a 1,480 px window and requires
each switch's own node, its base Switch rendered, named by its row's title and
described by its description, no row text read apart from it, and the track
ending the row;
`a_switch_row_answers_the_pointer_as_its_switch` presses a row's title and
description and hovers its title;
`each_settings_row_centres_its_control_on_its_title_line` requires each
track beside its text to be centred on its row title's first line, within
0.5 px, at 13 and 18 pt in a 1,480 and a 900 px window, Restore repository
tabs on startup's description wrapping to two lines or more at 13 pt in the
1,480 px one; `a_centred_control_keeps_its_focus_ring_inside_its_row`
focuses each switch and requires its ring inside the row at 18 pt and within
a ring's room of it at 13 pt; the kit's
own `place_track_positions_the_track_beside_leading_content`
(`scripts/vendor-crate-test.sh vendor/gpui-component switch`) requires the
track at the leading content's top without `place_track` and where the placing
element puts it with it, keeping its size, and a press on what that element
adds toggling the Switch; on Linux only,
`a_locked_follow_system_row_toggles_nothing` and
`a_locked_follow_system_dims_its_title` lock it with the Omarchy theme, so a
removal check elsewhere runs neither. Remove this part of the patch when
upstream Switch holds leading content, lets its caller place the track and
sets an accessible description, and those tests pass without it.

The test-node patch modifies `src/switch.rs` and `src/button/button.rs`.
`Switch::base` returns the base Switch that `render` draws inside its styled
box, and `Button::base` the base Button that `render` draws, so an
application's tests can render either and read its node: the test platform
builds no accessibility tree. Neither changes what is drawn; `render` returns
exactly what `base` builds. The consumers are tests only:
`each_switch_row_says_its_setting_once` reads each Settings switch's node
through `Switch::base`, and `tags::tests::review_tag_waits_for_the_busy_look`
reads Review tag's node through `Button::base` (both through
`views::tests::view_node`). Remove both methods when upstream exposes the built
base element or the test platform builds an accessibility tree, and those
tests read the nodes without them.

The diff-roles patch modifies `src/highlighter/registry.rs`,
`src/highlighter/wasm_stub.rs` and `src/highlighter/languages.rs`, and adds
`src/highlighter/languages/diff/highlights.scm`. Upstream builds the diff
language from tree-sitter-diff 0.1.0's own query, which captures added lines
and `+++` as `string`, removed lines and `---` as `keyword`, and `@@` lines as
`attribute`, and leaves context lines uncaptured, so a ```` ```diff ```` fence
in Markdown source drew a removed line in a theme's keyword colour and context
lines in the fence's code colour. The bundled query is that query with the
context lines, file headers, hunk locations and changed lines captured as
`diff.context`, `diff.header`, `diff.hunk`, `diff.minus` and `diff.plus`, and
`SyntaxColors` gains those five fields (theme keys of the same names) and
`HIGHLIGHT_NAMES` the five names; a
theme that leaves them unset draws those lines in its editor foreground. No
other grammar emits these names. The consumer is the application's syntax theme
(`crates/app/src/appearance.rs`), which maps them to its patch colours, covered
by `cargo test --locked -p gitturtle a_diff_fence_in_markdown_draws_the_patch_colours`.
Remove it when upstream gives diff captures names a theme can colour apart from
string, keyword and attribute, and that test passes without it.

The caller focus-handle patch also modifies `src/switch.rs` and
`src/checkbox.rs`. Upstream Switch and Checkbox keep their focus handles in
their own keyed state, so an application cannot tell whether one holds focus,
and Settings could not scroll a Tab-focused Switch or History column Checkbox
into view on its page. `Switch::track_focus` and `Checkbox::track_focus` now
take a caller-owned handle, exactly as `Button::track_focus` does, and the
control uses it in place of its keyed state; without one nothing changes. The
consumers are the Settings Switches and column Checkboxes
(`crates/app/src/settings.rs`), covered by
`cargo test --locked -p gitturtle tab_keeps_every_settings_control_in_view` and
`tab_keeps_the_identity_form_in_view`, which tab through the page in a
1,000 × 680 window and require every focused control in view. Remove it when
upstream Switch and Checkbox accept a caller focus handle or expose their
focus state.

The disabled-foreground patch modifies `src/theme/mod.rs`,
`src/button/button.rs`, `src/menu/menu_item.rs`, `src/menu/popup_menu.rs` and
`src/switch.rs`.
Upstream dims a disabled Button's label to `muted_foreground` at half strength
but draws a disabled menu item's label in plain `muted_foreground` and its
shortcut in the same `muted_foreground` as an enabled item's. GitTurtle's
Omarchy mapping can set secondary text as strong as text (Catppuccin Latte), so
a disabled item in the Linux Menu read like an available one. The consumers are
the Linux Menu, every context menu with an unavailable action, and the
application's command palette (`crates/app/src/command_palette.rs`), which
draws its own rows.

`Theme::disabled_foreground` is `muted_foreground` at half strength, the value
disabled Buttons already used; Buttons now read it from there, a disabled menu
item draws its label in it, its shortcut takes it through the Kbd's caller
style, and a disabled Switch's label takes it too. Enabled items and every
Button paint as before.

Half of a dark secondary text reads only 2.1:1 to 2.4:1 on a light palette's
surfaces, and the Switch label, which drew `foreground` at half strength, read
up to 5.3:1 in dark palettes, so the two dims disagreed. `Theme::disabled_label`, `None` by default, set in code and
skipped by theme files, replaces the half-strength value where an application
sets it; unset, the dim is upstream's Button dim. The app sets it to
`Palette::disabled_label` (`crates/app/src/appearance.rs`), `muted` at half
strength or at the first step toward three quarters that reads 3:1, with a
little room for 1x rendering, on the page's surfaces and the menu's and on the
translucent fill a disabled default Button paints on each of them.

`cargo test --locked -p gitturtle every_palette_shares_one_disabled_dim_that_reads_three_to_one`
applies every built-in and embedded Omarchy palette, requires the kit's dim and
the Switch's disabled label to be the palette's, draws disabled secondary,
primary, helper, ghost and default Buttons and a disabled Switch on each page surface and the
menu's, reads each Button's painted fill, and checks the dim against what lies
beneath and against enabled secondary text there and on the selected row. GPUI's
test scene does not expose glyph colours, so native frames of the Menu, the
command palette and Settings record the painted result. Remove this part of the
patch when upstream gives disabled menu items and their shortcuts a disabled
colour distinct from secondary text and lets a theme set it.

The disabled-primary patch modifies `src/button/button.rs`. Upstream draws a
disabled primary Button on a 15% tint of `button_primary`, and a disabled
custom variant on 15% of its own background, while a disabled secondary
Button rests on its opaque `button_secondary`. In GitTurtle's toolbar the
primary Push sits beside the secondary Fetch and Pull, so an unavailable Push
read as another kind of control: grey `#2A292F` in Omarchy Retro-82's navy
toolbar beside navy Fetch and Pull. The app's selected helper, which a palette
whose control label moved draws as a custom variant with a transparent
background (`Palette::selected_control_button` in
`crates/app/src/appearance.rs`), drew no fill at all when disabled, where the
secondary it stands in for draws `button_secondary`.

A disabled primary Button now takes exactly the disabled secondary's fill and
border, and `ButtonCustomVariant::disabled` sets a custom variant's disabled
fill and border, which default to 15% of its background as before; the app
gives its selected custom variant the palette's `subtle`, its
`button_secondary`. Enabled Buttons, the outline and default variants, and the
danger, warning, success and info tints are unchanged. The consumers are the
toolbar's Push (`crates/app/src/workspace.rs`), the composer's Commit, the
other primary Buttons the app builds and the kit's own, and the shared
helper's selected look (`button(…, true)` in `crates/app/src/main.rs`).

`cargo test --locked -p gitturtle every_palette_rests_disabled_primary_buttons_on_the_neutral_fill`
draws disabled primary, secondary and selected helper Buttons on each page
surface and the menu's in every built-in and embedded Omarchy palette and reads
their painted fills; `disabled_primary_actions_rest_on_the_neutral_fill` makes
the real toolbar busy in Midnight, Braden and, on Linux, Omarchy Retro-82, and
reads Fetch, Pull, Push and the composer's Commit. Remove this part of the
patch when upstream draws a disabled primary Button on its secondary's
disabled fill and lets a custom variant set its disabled fill, and those tests
pass without it.

The popover-edge patch modifies `src/theme/mod.rs`, `src/styled.rs` and
`src/menu/popup_menu.rs`.
Upstream edges every popup surface (menus, popovers, selects, the combobox,
date picker and hover popovers) with a hairline ring of 10% of the foreground,
which a theme cannot tune. On a popup the same colour as the surface it opens
over, as GitTurtle's Linux Menu was over the header, the edge read 1.12:1 to
1.15:1 in several Omarchy palettes. `Theme::popover_edge` lets an application
set the ring's colour; unset, the ring is upstream's. `popover_ring` is now
public so the application's tests can read the colour it resolves to. The
consumers are the Linux Menu, context menus, selects and popovers, whose
surface GitTurtle sets to the lightest page surface and whose edge it fits to
1.4:1 against that surface and every page surface, room above the border rule's
1.3:1 for antialiasing (`Palette::raised` and `Palette::raised_edge` in
`crates/app/src/appearance.rs`). Menu separators
lie on that surface, where upstream's `border` token read 1.16:1 in Omarchy
Matte Black, so `menu_separator` draws them in the edge when the theme sets
one and in `border` otherwise. `popover_shadow` paints the ring after the two
shadow layers: GPUI draws later layers on top, the reverse of CSS, so the ring
upstream lists first lay under the ink, which darkened a fitted edge to 1.15:1
along a menu's bottom in Omarchy Everforest. Geometry, the shadow layers'
values and their animation are unchanged.

`cargo test --locked -p gitturtle every_palette_raises_menus_above_the_page_with_a_border_edge`
applies every built-in and embedded Omarchy palette and reads the popup surface
and the resolved ring and separator. Native frames of the Menu record the rendered edge.
Remove this part of the patch when upstream lets a theme set a popup's edge
colour.

The dialog-edge patch modifies `src/theme/mod.rs` and
`src/dialog/dialog.rs`. Upstream draws a dialog panel's 1 px border in the
shared `border`, which an application fits against its page, not against a
dialog over the backdrop that dims the page: GitTurtle's Braden `border` read
1.28:1 against the dialog and 1.08:1 against the dimmed page.
`Theme::dialog_edge`, `None` by default, set in code and skipped by theme
files, is the panel's border where an application sets one; unset, the panel
keeps `border`, as upstream. A caller's own `border_color` on a dialog still
wins, as GitTurtle's raised pickers set theirs. The consumers are every app
dialog; the app sets the edge to `Palette::dialog_edge`
(`crates/app/src/appearance.rs`), fitted to 3:1 against the dialog's surface
and each page surface its backdrop dims.
`cargo test --locked -p gitturtle a_dialog_draws_its_edge_in_the_themes_dialog_edge`
opens a bare kit dialog and reads its painted border, in `border` unset and in
the edge once set, and
`every_dialog_edge_holds_against_the_dialog_and_the_dimmed_page` applies every
built-in and embedded Omarchy palette and reads the edge each dialog draws.
Remove this part of the patch when upstream lets a theme set a dialog's border
apart from `border`, and those tests pass without it.

The dialog-bounds patch (2026-11-05) modifies `src/theme/mod.rs` and
`src/dialog/dialog.rs`. Upstream gives a dialog that sets no width of its
own 448 px at every window width, centred, so in a 461 px half tile it stood
6.5 px from either side, where GitTurtle's dialogs with a width of their own
keep 24 px (`appearance::dialog_width`). `Theme::dialog_margin`, `None` by
default, set in code and skipped by theme files, is the room such a dialog
keeps from the window's sides inside a client-decorated frame's padding:
set, the default width is 448 px or the window less that margin on either
side, whichever is narrower; unset, it is 448 px, as upstream. A width the
caller sets is its own, as before (the props now hold it as an `Option`).
Upstream also opens each nested dialog 16 px lower than the one beneath it
and keeps the caller's height cap, so a nested dialog under the same cap as
the first ran 16 px further down, past the margin GitTurtle keeps above its
status bar (`appearance::dialog_max_height`). A nested dialog's absolute
height cap now gives up its layer's offset, so its foot stays where the cap
puts the first dialog's; the first dialog, a cap given as a fraction or
none, are unchanged. The app sets the margin to `appearance::DIALOG_MARGIN`
beside the dialog edge. The consumers are the dialogs with no width of their
own (a custom theme's delete confirmation, Delete saved profile, the Markdown
link dialog and the repository picker's failure) and every dialog opened
over another. `cargo test --locked -p gitturtle width_walk::` opens the
delete confirmation in a 461 × 490 window at 13 and 18 pt and fails it
closer than 24 px to a side
(`dialogs_keep_inside_a_narrow_window_at_13_pt` and `_18_pt`), and opens an
error's details over the Reflog and fails its foot off the cap or closer
than 24 px to the status bar
(`a_nested_dialog_keeps_its_foot_above_the_status_bar_at_13_pt` and
`_18_pt`). The kit tests are
`dialog::dialog::tests::the_default_width_keeps_the_dialog_margin`, 448 px
unset, 24 px set, and a caller's 440 px kept, and
`dialog::dialog::tests::a_nested_dialog_gives_its_offset_up_from_its_cap`
(`scripts/vendor-crate-test.sh vendor/gpui-component dialog::dialog::tests`).
Remove this part of the patch when upstream lets an application
bound a dialog's default width and keeps a nested dialog's foot under its
cap, and those tests pass without it.

The dialog-sides patch (2026-11-05) modifies `src/dialog/dialog.rs` alone.
Upstream pads every side of a dialog's panel 16 px. The ghost Buttons,
transparent at rest, at the text edge of the ten GitTurtle dialogs named
below as its consumers stand their content, a leading icon, else their
label, on that edge (the Reflog's at a field's text inset past it, where its
text starts), and their frames and hover
fills their label inset (`crate::ghost_pull`: 10 px at 13 pt, 14 px at
18 pt) before it in the side padding, their 3 px focus ring outside the
frame (`DESIGN.md`, "One left edge"): 13 px at 13 pt, which 16 px holds, but
17 px at 18 pt, which it does not. `set_dialog_side_room`, a public
function beside the dialog, installs a callback asked as each dialog is drawn
for that window's room; a dialog whose caller sets no side padding of its own
pads its left and right sides by the larger of that room and 16 px
(`dialog_side_padding`), so the ring is drawn whole inside the panel, and
keeps 16 px at its top and foot. A caller's own padding is its own, and
unset, every side keeps upstream's 16 px. Keys, focus, Enter and the close
Button's placement relative to the right padding are unchanged. The app
installs `dialog_ghost_room` (`crates/app/src/main.rs`: the compact Button's
scaled label inset on the device pixel plus `appearance::button_ring_room`,
at least 16 px, rounded up to the device pixel, so the larger of it and
16 px is that room unchanged and lies on the device pixel at every scale)
from `native_accessibility::set_overlay_room`, which the app and its test
windows call, so every app dialog takes 16 px at 13 pt and 17 px at 18 pt,
each rounded up to the device pixel. The app's readers of
`dialog_side_padding` are
Keyboard Shortcuts' `shortcuts::help_width`, which lays out its count line in
the dialog's content width; Files in the GitHub panel
(`github_view/review.rs`), whose columns' measured chrome leaves the padding
out and whose `Panel::files_fit` adds it back for the columns' width, so a
text size change settles in one frame; the theme editor
(`theme_editor.rs`), which sizes its dialog as its scaled content and the
padding on either side; and, in tests, the width walk's dialog padding check
and `dialog_sides::the_ghost_room_lies_on_the_device_pixel`. The app's other
caller of `set_dialog_side_room` is the ring-room sweep
(`focus_ring::sweep`), which holds the sides at the installed room. The
consumers are the dialogs whose ghost Buttons stand on their text edge (Git
profiles and the profile form, Activity, Compare local revisions' choices,
the rewritten-series review, Download an LFS preview, Worktree created,
About's Copy bug diagnostics, Add remote's Fetch rules… and the Reflog's
Copy commit ID and Review recovery branch…), those readers,
and every other dialog, which keeps its text at the same edge. The kit test
`dialog::dialog::tests::the_sides_keep_the_room_an_application_sets`
(`scripts/vendor-crate-test.sh vendor/gpui-component dialog::dialog::tests`)
covers unset, larger, smaller and a caller's own padding on both sides; the
app test `dialog_sides::dialogs_pad_their_sides_for_a_ghost_buttons_ring`
opens ten of the app's dialogs (each consumer named above except Download
an LFS preview, Worktree created and the Reflog, plus the command palette, the GitHub
panel and the theme editor) at 13 and 18 pt in a 461 × 490 and a
1,480 × 800 window and finds their text, or for the GitHub panel its
footer's close Button, 16 px and 17 px inside the panel's edge, and every
ring Tab draws in those with a ghost Button at their text edge, and in the
palette, inside it. Download an LFS preview, Worktree created and the
Reflog are reached by `dialog_sides::ghost_buttons_stand_their_labels_on_a_dialogs_text_edge`,
which opens the first two from their openers, with a working file's pointer
and a worktree's plan the test makes, and the Reflog with its newest entry
selected, beside the other seven consumers, at the same sizes and windows,
and in the half tile at scale 1.25 too, and finds every ring Tab draws in
each of the ten inside the panel's edge. Remove this part of the patch when upstream lets
an application set a dialog's default side padding, and those tests pass
without it.

The dialog-enter patch modifies `src/root.rs`, with the base patch's held
Enter guard (base `GITTURTLE-PATCH.md`, "dialog-button"). `Root::open_dialog`
calls `gpui_base::dialog_opened`, so a dialog an Enter press opened waits for
that press to come up before it takes Enter, and nothing in the window takes
that press's repeats meanwhile, and the root's element passes every key-up
in the window to `gpui_base::note_key_up`, so an Enter that comes up with no
dialog open ends the press too, and a dialog opened later by a click takes
its first Enter. That key-up listener is the first key listener on every
focused path, which GPUI hands a key-down the base interceptor stopped; it
takes no key-down. Nothing else changes. Remove it with the base patch's
guard.

The dialog-focus patch modifies `src/dialog/dialog.rs` and `src/root.rs`.
Upstream closes a dialog its Confirm closes (Enter, or the OK Button, which
dispatches it) through `Root::defer_close_dialog`, which removes the dialog
at once but gives focus back to the control captured as it opened only when
a 250 ms timer ends, the length of its opening animation, and then whatever
was focused meanwhile: a control the person clicked after the close, or the
place the application had already returned focus to (GitTurtle's repair in
the frame after a close, `update_modal_focus` in
`crates/app/src/page_navigation.rs`, which rechecked 300 ms later to undo
it). A pending restore handed the captured control to a dialog opened during
those 250 ms. Patched, every close request gives focus back as the dialog
goes, as Escape and Cancel did (`WindowExt::close_dialog`): the timer and
the pending restore are gone, and a dialog opened after a close captures the
focus that close gave back, or whatever was focused since. The kit's
`a_confirmed_dialog_gives_focus_back_as_it_closes`
(`scripts/vendor-crate-test.sh vendor/gpui-component dialog::dialog::tests`)
and the app's `page_navigation::tests::a_focus_given_after_a_dialog_closes_keeps`
cover it. Remove it when upstream gives a confirmed dialog's focus back as
it closes, or never over focus moved since the close, and both tests pass
without it.

The dialog-close-draw patch modifies `src/root.rs`. GPUI hit-tests a press
against the frame the window last drew. A window draws as the platform next
asks for a frame, or as a key event arrives (`Window::dispatch_key_event`
draws a dirty window first), never as a mouse event does, so a dialog or
sheet that `Root::close_dialog`, `Root::close_all_dialogs` or
`Root::close_sheet` takes away stayed in that frame until the window drew
again: its backdrop swallowed a press there, and its Buttons took one, the
closed dialog's own action included. A click on a History row given before
the Return that confirmed the branch dialog came up was lost to the gone
dialog's backdrop, and a click where its Rename… had been opened the rename
form (native QA-106e D6). Patched, a close that takes a modal away has the
window draw again once the effects of what closed it have run, before the
platform hands it the next input (`Window::defer`), so a press after the
close acts on what the window draws without the modal, on the control the
close gave focus back to and the page beneath it; a close with nothing open
draws nothing. Closes in one effect cycle draw once (`Root::draw_pending`):
the draw waits one more turn of the cycle, behind every effect the code that
closed them queued, and a frame the window draws first, as for a key event,
takes its place. The frame is presented as the platform next asks for one,
as any frame drawn between frames is, and replaces that frame's own draw,
except where an effect queued later still changes what the window shows:
then that frame draws again. The kit's `two_closes_in_one_handler_draw_once`
(`scripts/vendor-crate-test.sh vendor/gpui-component root::`) counts the
draws of two dialogs closed in one handler. The app's `dialog_close_tests`
(`cargo test --locked -p gitturtle dialog_close_tests::`) click a History
row before Return on the branch dialog's Done comes up, after it, in the
frame after and two frames later, with motion reduced and not, and before
and after the key-up with the dialog opened from a stash's row, as on the
native route; where its Rename… was; after Return on the Tags dialog's
Done, reached with Tab; and after Escape. `close_all_dialogs` and
`close_sheet` draw as `close_dialog` does, untested while no product code
calls them. Remove it when upstream takes a closed modal out of the
window's input as it closes, or hit-tests a press against a frame drawn
since, and those tests pass without it.

The popup-close-draw patch extends it to `src/menu/popup_menu.rs`,
`src/select.rs` and `src/color_picker.rs`, through
`root::draw_after_popup_close`, which defers once and then takes
`draw_before_next_input`'s one draw (deferred, so a close inside an update of
`Root` cannot reenter it). A dropdown, a right-click menu or a Select that a
key closed, and a color picker's popover, stayed in the frame presses are
hit-tested against until the window next drew, so a click right after the
closing key could land on the gone popup: a menu's item took it, or the popup
swallowed it. `PopupMenu::dismiss` (Escape, Enter on an item, a click on an
item or outside it), `SelectState::set_open` going closed (Enter on an item,
Escape, losing focus, a click on the trigger) and the picker's `ClosingWatch`,
kept while its popover is drawn on either tab (every route that closes the
picker), now have the window draw before its next input. It adds a draw and no key handling: the held-Enter, X11
autorepeat and modified-Enter guards around them are unchanged.
`scripts/vendor-crate-test.sh vendor/gpui-component a_click_after_the_key_that_closes`
clicks where a dropdown's and a right-click menu's item, a Select's list and
a picker's popover were, before and after the closing key comes up. Remove it
with the dialog-close-draw patch.

The menu-dialog-focus patch modifies `src/menu/popup_menu.rs`,
`src/menu/dropdown_menu.rs`, `src/button/button.rs` and `src/root.rs`, and adds
`PopupMenu::set_dialog_return_focus`, public for the app's right-click menus.
Upstream runs a menu item's handler, or dispatches its action, with focus on
the menu, as an action's dispatch needs. A dialog opened there records the
menu's handle as its opener, and the menu goes as the item finishes, so closing
the dialog focused nothing and keys reached nothing until a click. Behind the
item, in an effect queued after a dispatched action's,
`PopupMenu::return_dialogs_to_opener` has `Root::return_dialogs_to` replace an
opener that is a menu of the item's chain with the control that opened the
menu: the dropdown's trigger, or a menu's `previous_focus_handle` or
`action_context`, a submenu using its parent's. Items still run on the same
focus and the menu closes as before, so an item that opens no dialog, a dialog
that recorded anything else (a handler that moved focus first, as the Linux
Main Menu's do) and a dialog opened any other way are unchanged; a control gone
by the close leaves the dialog's existing fallback. A press on a dropdown's
Button leaves focus where it was, so what held focus as the menu opened need
not be the trigger: a Button trigger with no focus handle of its own now tracks
one the dropdown keeps (`DropdownMenu::tracking_opener_focus`), and a caller's
own handle stays its. Other triggers take no handle and keep the fallback, as
does a Button that is no Tab stop: a row's menu Button that shows only on
hover can be drawn at opacity 0. The dropdown then leaves what the menu's
builder set with `set_dialog_return_focus` standing, as the project pane's row
menu sets its tree.
`scripts/vendor-crate-test.sh vendor/gpui-component a_dialog_from_a_menu focus_a_menu_item_or`
covers a handler, an action and a submenu's item from a dropdown opened by key
and by pointer and from a context menu, on Escape and on close, and what stays
as it was; `cargo test --locked -p gitturtle -- stashes_from_the_actions_menu a_dialog_from_a_right_click_menu`
covers the consumers. Remove it when upstream gives such a dialog the menu's
opener.

The dialog-close Enter patch modifies `src/root.rs`, and pairs with the base
patch's `dialog_closing`. `Root::close_dialog` and `Root::close_all_dialogs`
call it as they close a dialog, whichever way the dialog opened, so an Enter
still down waits for its key-up. The close gives focus back to a control, and
that press's repeats reached it: held past the repeat delay, the Enter that
confirmed a dialog pressed the Button focus returned to, and under X11, whose
repeats are not marked held, it reopened a dropdown and ran the item under the
cursor. `scripts/vendor-crate-test.sh vendor/gpui-component the_enter_that_closes an_unmarked_repeat`
holds Enter through a menu's dialog, a plain dialog and `close_all_dialogs`.
Remove it with the base patch's Enter wait.

The secondary-edge patch modifies `src/theme/mod.rs` and
`src/button/button.rs`. Upstream draws a bordered secondary Button's edge in
the shared `border` at rest and sets it from `border` again in its hovered,
pressed and selected styles, over any `border_color` its caller gives it, so
an application cannot fit that edge to the surface the Button lies on.
GitTurtle's one bordered secondary is every dialog's Cancel
(`tags::dialog_cancel` in `crates/app/src/tags.rs`), whose `border` edge read
1.28:1 against Braden's dialog. `Theme::secondary_edge`, `None` by default,
set in code and skipped by theme files, is that edge in every one of those
states where an application sets one; unset, it is `border`, as upstream.
Disabled, an outline secondary Button draws it at half strength, where
upstream drew `border` at half strength; a bordered secondary that is not an
outline, the Cancel among them, draws the disabled edge below instead, where
an application sets one. The app sets it to its dialog edge,
`Palette::dialog_edge` (`crates/app/src/appearance.rs`), 3:1 against the
dialog and the page its backdrop dims. The kit's own
`a_secondary_buttons_edge_is_the_themes_secondary_edge`
(`scripts/vendor-crate-test.sh vendor/gpui-component`) reads the variant's
border in each state with the edge unset and set, and
`cargo test --locked -p gitturtle a_dialogs_cancel_draws_the_dialog_edge_in_every_state`
opens a dialog holding the app's Cancel in every built-in and embedded Omarchy
palette and reads its painted edge at rest, hovered and pressed. Remove this
part of the patch when upstream lets a theme set a secondary Button's border
apart from `border`, and those tests pass without it.

The disabled-edge patch modifies `src/theme/mod.rs` and
`src/button/button.rs`, with the disabled-primary patch above. A disabled
primary or secondary Button, and a custom variant that sets its own disabled
fill, rests on that fill with no edge, and where the fill nearly equals the
surface beneath, the Button draws only its dim label: GitTurtle's neutral
`subtle` stood 1.03 to 1.05:1 off its dialogs in five palettes, so a dialog's
disabled default action read as stray text. `Theme::disabled_edge`, `None` by
default, set in code and skipped by theme files, is a 1 px edge such a
disabled Button draws where an application sets one: one that is not an
outline and has a border its caller gave it (a `border_1`, as a dialog's
Cancel has) draws it as that border's disabled colour, and one with no border
takes it from a child laid over its bounds with the Button's corners,
absolutely placed and without a role, so it moves nothing and adds no
accessibility node. Unset, nothing changes. Enabled Buttons, outline Buttons,
which keep their half-strength disabled border and take no child, and the
ghost, link, text, default, danger, warning, success and info variants'
disabled looks are unchanged by this part. The consumers are every app
Button of those variants while disabled: dialogs' default actions, the
toolbar's Fetch, Pull and Push, the composer's Commit and the command
palette's Open command; the app sets the edge to `Palette::disabled_edge`
(`crates/app/src/appearance.rs`), fitted to 1.5:1 against every page surface,
the raised one and the fill, and drops the bordered copy it had drawn at those
four places only where the fill fell below 1.05:1. The kit's own
`a_disabled_filled_button_draws_the_themes_disabled_edge` reads a disabled
primary's painted edge with the theme's unset and set and that it keeps its
bounds, and that an enabled primary and a disabled ghost draw none;
`cargo test --locked -p gitturtle every_disabled_button_that_keeps_a_fill_keeps_its_shape_with_an_edge`
reads the edge of each disabled kind on each page surface in every built-in
and embedded Omarchy palette, and the toolbar, composer and palette tests read
it on the real controls. Remove this part of the patch when upstream lets a
theme give a disabled filled Button an edge, and those tests pass without it.

The resting-edge patch modifies `src/theme/mod.rs`, `src/button/button.rs`
and `src/switch.rs`, and shares the disabled-edge patch's layer. Upstream
gives an enabled control that rests on a fill of its own no edge but a
border, and a border takes room in its layout: a `border_1` widens an
auto-width Button by 2 px, and an unchecked Switch track's 2 px border is
the transparent inset that places its thumb. Where that fill nearly equals
the surface beneath, a secondary Button or an unchecked Switch shows only its
label or its thumb, which GitTurtle's More contrast appearance ("a 1 px edge
on resting controls", `docs/development/omarchy-full-support.md`) must not
allow. `Theme::resting_edge`, `None` by default, set in code and skipped by
theme files, is a 1 px edge such a control draws where an application sets
one, as a layer over its bounds with its corners, absolutely placed and
without a role, so it moves nothing and adds no accessibility node. An
enabled Button that is not an outline, has no border its caller gave it, is
not a default Button (which draws its own border) and rests on a fill of its
own (`ButtonVariant::keeps_a_resting_fill`: a secondary Button, and a custom
variant whose resting fill, or selected fill while selected, is not
transparent, as the app's selected segment is) draws it from the disabled
edge's layer at rest, hovered, pressed and selected; its fill, label and
focus ring are unchanged. A primary Button, whose accent fill already stands
off the surface, ghost, link and text Buttons, a custom variant transparent
at rest and the danger, warning, success and info variants draw none;
disabled, a Button keeps the disabled edge and draws no resting edge. An
enabled, unchecked Switch draws it from a layer under its thumb that reaches
back across the track's inset to the track's bounds, in every pointer state,
so the track's hover lift (`switch_hover`) still shows; a checked track, on
the accent, and a disabled one draw none, and the thumb, the label, the
focus ring and every bound are unchanged. Unset, nothing changes.

The consuming path is the app's More contrast appearance: it is to set the
edge from its palette (`crates/app/src/appearance.rs`) only while More
contrast is on, and to leave it unset otherwise. While it is set, the kit
draws it on every such control in the window: every enabled, unchecked
Switch and every enabled secondary, filled custom or selected custom Button
without a border of its own, the toolbar's selected shared button
(`button_frame`), Settings' density segments and the Projects mode tabs among
them. Nothing in the app sets it yet. The kit's own `an_enabled_filled_button_draws_the_themes_resting_edge_in_every_state`
reads one 1 px edge of the set colour over a secondary, a selected secondary,
a filled custom and a selected-segment Button, with its fill's corners, at
rest, hovered and pressed, with the bounds and fills it paints unset;
`only_an_enabled_button_on_a_fill_of_its_own_draws_the_resting_edge` reads
none on primary, ghost, default, danger, link, text, transparent custom,
outline, bordered and disabled Buttons, and the disabled edge alone on the
disabled filled ones once that is set;
`without_a_resting_edge_every_button_paints_as_upstream` reads, unset, each
of them painting the fill its variant's style gives it at rest, hovered,
pressed, selected or disabled, and no edge but the border a default, outline
or bordered Button draws itself, in that style's colour;
`an_unchecked_switch_draws_the_themes_resting_edge_in_every_pointer_state`
reads the edge over an unchecked track at rest, hovered (with the hover lift)
and pressed, with its track's corners and the bounds, track and thumb it
paints unset, and `a_checked_or_disabled_switch_draws_no_resting_edge` reads
none on checked and disabled Switches
(`scripts/vendor-crate-test.sh vendor/gpui-component resting_edge`). The
app's Button and Switch quad tests
(`cargo test --locked -p gitturtle -- settings::picker_tests:: views::tests::narrow_review_arrows_lift_on_hover appearance::`)
pass unchanged, since nothing sets the edge. Remove this part of the patch
when upstream offers a resting-edge or high-contrast style for filled
controls, and those tests pass without it.

The field-fill patch modifies `src/theme/mod.rs` and `src/input/input.rs`.
Upstream derives a field's fills from `input`, the colour of the field's
outline: a dark theme's field fill at 30% (`Theme::input_background`) and a
disabled field's fill at 80%, light or dark (`input::input_style`), so an
application cannot strengthen the outline without lifting the fills inside it
and everything that rests on them. GitTurtle's More contrast appearance raises
`input`, with the app's `border`, to a 3:1 edge: a dark field's fill rose with
it (Midnight `#1a2330` to `#2a323d`), the placeholder on it read worse
(Omarchy tokyo-night 5.99:1 to 5.32:1), and a disabled field's fill rose in
every palette. `Theme::input_fill_base`, `None` by default, set in code and
skipped by theme files, is the colour both fills derive from where an
application sets one; unset, it is `input`, as upstream. `input_background`
is the base at 30% in a dark theme and `background` in a light one, and
`Theme::disabled_input_background`, which `input_style` now reads, is the base
at 80% in either mode. `input` itself, the outline, is untouched, except a
disabled Input's below.

A disabled Input's outline takes the disabled edge beside a field-fill base.
Upstream draws every Input's outline in `input`, enabled or disabled, so once
an application strengthens `input` to an edge enabled controls must hold, a
disabled field draws that full edge (Midnight's 3:1 `#606976` under More
contrast) beside a disabled Button's fainter `Theme::disabled_edge` (1.66:1
`#313d4e`), and only its dim text marks that it cannot act. The styled Input
now takes its outline from `input::input_outline`: `input`, as upstream, unless
it is disabled while the theme sets both `Theme::input_fill_base` and
`Theme::disabled_edge`, and then that edge. Either unset, both outlines are
`input`; GitTurtle sets the disabled edge for Buttons in every appearance and
the base only under More contrast, so the outline it draws with More contrast
off is unchanged.

Every kit read of `input` was reviewed. Two fills derive from the base:
`Theme::input_background` and `Theme::disabled_input_background`, and the
styled Input's outline reads `input_outline`. The readers
of `input_background` take the base with no change of their own: the styled
Input's enabled fill (`input::input_style`), a disabled default Button's fill
(half of it) and an outline default Button's resting fill
(`button/button.rs`), an unchecked Checkbox's and Radio's fill, and the editor
background where the highlight theme sets none (`Theme::editor_background`).
`input_style`, with both fills, also styles the OTP input, Select, Combobox,
DatePicker and NumberInput, and the first four take `input_outline` too (the
disabled-field patch below). Unchanged, reading `input` itself: every other
outline (the enabled Input's, Checkbox's, Radio's and default Button's borders,
the half-strength borders of a disabled default Button, Radio and NumberInput,
and NumberInput's and ColorPicker's), and the fills derived from `input` at
other strengths, none of which the app draws: an outline default Button's
hover and pressed fills (50% and 70%), NumberInput's step buttons hovered and
pressed (`input` at 40% and 60%) and the theme-file fallbacks for `button`,
`button_hover` and `button_active` (`theme/schema.rs`), which apply only to
colours a theme file leaves unset. A Radio paints no fill from `input`: the
`input` at half it computes for an unchecked one is never drawn, since an
unchecked Radio fills with `input_background` (above), enabled or disabled,
and a checked one with `primary`, at half when disabled.
Under GitTurtle's More contrast those fills derive from the 3:1 outline, not
the field-fill base, and the warning, success and info Buttons rest, hover and
press on translucent tints of their status colours (10%, 20% and 40%,
`button/button.rs`) that stand barely off the surface; no app view draws an outline, warning, success or info Button, a NumberInput or a
Radio (`rg` over `crates/app/src`, 2026-11-05: only
`native_accessibility::control_tests` builds a bare base Radio, for its
accessibility node), so they are recorded here as unused and unchanged rather
than patched; fitting them is more than a line or two each and needs a
consumer to verify against.

The disabled-field patch modifies `src/select.rs`, `src/combobox.rs`,
`src/input/otp_input.rs` and `src/time/date_picker.rs`. Upstream draws each
of them disabled with the field's disabled tokens (`input_style`: the
`disabled_input_background` fill and `muted_foreground` text) but the enabled
outline, `input`, and fades the box it edges, with its text, to half. That
fade matches a disabled Input's own fill and text, which the styled Input
halves too, but not its outline: beside a field-fill base and the disabled
edge a disabled Select, Combobox, OTP digit or DatePicker drew the stronger
enabled outline at half strength, where a disabled Input draws the disabled
edge at full strength. Each now edges that box with `input_outline` and
leaves it unfaded, so disabled it draws the disabled field tokens at full
strength: the `disabled_input_background` fill, `muted_foreground` text and
`input_outline`'s disabled outline. That is not a disabled Input's look,
which halves its fill and its text (its value in the inherited text colour,
its placeholder in `muted_foreground`); only the outline is the same. With
either theme setting unset the outline is `input`, as upstream, without the
fade. No app view draws these four today (the vendor guide's note on the
three dropdowns), so the patch takes the Input's outline and drops the fade,
a line or two in each, rather than also halving their fill and text to the
Input's. ColorPicker has no disabled state, so it draws no disabled look to
change.
The kit's own `disabled_field_controls_draw_the_disabled_field_tokens_unfaded`
renders each disabled, with the field-fill base and the disabled edge unset
and set, and reads every box it edges from the scene: the disabled fill and
`input_outline`'s disabled outline, unfaded
(`scripts/vendor-crate-test.sh vendor/gpui-component disabled_field`). Remove
this part of the patch with the field-fill patch, whose `input_outline` it
reads, or when upstream edges these controls' disabled boxes in the outline
a disabled Input draws, unfaded, and that check passes without it. The input
patch's `src/input/input.rs` no longer imports `Colorize`, which it does not
use, so the kit builds without an unused-import warning.

The consuming path is the app's More contrast appearance: its `configure`
(`crates/app/src/appearance.rs`) sets the base to its palette's `border` as
chosen, before the adjustment, while More contrast is on, light or dark, so a
field's fill, enabled and disabled, the disabled Save's and an unchecked
Checkbox's keep the values they have with More contrast off while the outline
is adjusted, and leaves it unset otherwise; a disabled field's outline then
takes the 1.5:1 disabled edge it sets for Buttons in every appearance, the
palette as chosen, so it reads below the enabled field's 3:1 outline.
`cargo test --locked -p gitturtle more_contrast_keeps_the_field_and_save_fills_in_every_palette`
reads a field's and Settings' Save's painted fills, each enabled and
disabled, on each page surface in every built-in and embedded Omarchy palette
with More contrast on and off, the enabled field's outline adjusted, the
disabled field's the disabled edge below it on every surface, and both
`border` with More contrast off. The kit's own
`a_disabled_input_takes_the_disabled_edge_beside_a_field_fill_base` reads an
enabled and a disabled Input's painted outlines with neither, either and both
of the base and the disabled edge set: `input` in every case but a disabled
Input with both, which draws the edge, the bounds unchanged
(`scripts/vendor-crate-test.sh vendor/gpui-component disabled_input`). The
kit's own
`input_fill_base_sets_the_field_fills_and_leaves_the_outline` reads
`input_background` and `disabled_input_background` unset (`input` at 30% and
80%) and set (the base at 30% and 80%) in a dark theme, `background` and the
base at 80% in a light one, with `input` and its semantic token untouched
(`scripts/vendor-crate-test.sh vendor/gpui-component input_fill`). Remove
this part of the patch when upstream lets a theme set a field's fills apart
from its outline, and a disabled field's outline apart from an enabled one's,
and those checks pass without it.

The popup-inset patch modifies `src/menu/context_menu.rs`,
`src/native_menu/fallback.rs`, `src/popover.rs` and `src/styled.rs`, with the
base patch's `set_popup_bottom_inset`
([base patch](../gpui-base/GITTURTLE-PATCH.md)). Upstream keeps a popup only
inside its window: the base popup host, which places dropdown menus and
popovers, keeps 8 px from the window's edges, and a context menu's GPUI anchor
moved it only once it ran past the window. A menu opened at a low row, or
below a low button with less room than its items, could therefore end over
GitTurtle's status bar, and nothing an application set kept it off: at 13 pt
in a 461 × 490 window a right-click menu on History's lowest row ran 19.5 px
over the bar. A context menu is now placed by the base `Positioner`, as the
popup host places a dropdown: inside the window less the same 8 px and above
the room the application keeps at the window's foot
(`gpui_base::popup_bottom_inset`), moving up where it would run lower, as it
moves in from the window's sides. The drawn menu an input's right-click
shows where the platform has no OS-native popup, Linux among them
(`NativeMenu`'s fallback, which `Root`'s `FallbackMenuOverlay` draws), keeps
the same margin and inset and never covers the point it was opened at: the
base `Positioner::point` places it with its top at the pointer where its
items fit below it, and otherwise wholly above the pointer, its bottom edge
there. Its items are capped at the room the pointer leaves on the side with
more (`Positioner::point_room`) and scroll past it, with the capped menu's
fold and scrollbar, so where neither side holds them it still lies wholly on
one side of the point; a menu that holds a submenu, which a scrolling menu
cannot show, is not capped, and no input's menu holds one. A resized window
closes it and returns the focus it holds to the field it was opened on, as
the application's menus close, since its side and cap were chosen for the
old size. GPUI's anchor kept it only inside the window, less 8 px, so a
right-click near the foot of an editor low in the window ended over the
status bar, and placed as a context menu is, it then slid up just far enough
to clear the bar, over the line clicked. A popover's
quarter-rem gap from its
trigger is now a margin (`mt_1`, `mb_1`) rather than a relative offset
(`top_1`, `bottom_1`): the host measures and places the box, and a relative
offset lies outside it, so a surface the host moved to the inset ended a
quarter rem past it. The surface draws where it did. `POPOVER_EDGE_WIDTH`
names the 1 px band outside a popup's fill in which `popover_shadow` draws its
ring, so an application keeps the drawn edge, not only the fill, clear of
what it protects.

The consumers are every app dropdown (`AppMenu`, built as `AppDropdown` on
the kit's dropdown menu and popover, which the base popup host places) in
`crates/app/src/native_accessibility.rs`, whose
`set_overlay_room` sets the inset to the status bar, the 8 px
above it and the edge band, and `menu_foot` there, which takes the edge band
from `POPOVER_EDGE_WIDTH` as it caps the app's menus. No app menu opens
through the kit's context menu: the app's right-click menus
(`AppContextMenu`, which builds a `PointerMenu`) and its keyboard row menus
(`KeyboardMenu`) place themselves beside their row (`RowMenu`, inside
`menu_bounds`), so the context-menu half reaches only the kit's own
`ContextMenuExt` callers (`Tree`, `Table` and the sidebar menu), none of which
the app uses, and the regression below, which opens a kit right-click menu
directly. The `Input`, `Editor` and `Textarea` the app does use, the commit
title and description among them, show their right-click menu through the
input's `NativeMenu` (by `Input::context_menu`), not through
`src/menu/context_menu.rs`: OS-native on macOS and Windows, and on Linux a
kit `PopupMenu` drawn by `Root`'s `FallbackMenuOverlay`, which this patch
keeps above the inset.
`cargo test --locked -p gitturtle kit_menus_stay_above_the_bottom_inset_an_application_sets`
sets an inset of 120.5 px and opens a kit dropdown and a kit right-click menu
at the foot of a 400 × 320 window: the host moves each up so that it ends
above the inset, the right-click menu up from the pointer. Without the margin
the dropdown ended 3.5 px past the inset.
`menus_keep_their_edge_8_px_above_the_status_bar` opens the Working Actions
menu, a dropdown, at 13 and 18 pt in 461 × 490 and reads its drawn edge 8 px
or more above the status bar; the right-click menu it opens on History's
lowest whole row there is the app's own `PointerMenu`, placed by `RowMenu`.
On Linux, `an_inputs_menu_opens_wholly_above_the_pointer_where_it_does_not_fit_below`
right-clicks in the commit title there, near the window's foot, and reads
the field's drawn menu, taller than the room below the pointer, wholly above
the pointer, its bottom edge there and its edge 8 px or more above the bar:
placed by GPUI's anchor, it ran over the bar, and moved up only far enough to
clear the bar, it covered the point clicked.
`an_inputs_menu_opens_at_the_pointer_where_it_fits_below` right-clicks in the
Working Changes filter, with room below, and reads the menu's top at the
pointer, never above it.
`an_editors_menu_too_tall_for_either_side_scrolls_beside_the_point`
right-clicks mid-window in a kit code editor at 18 pt in 461 × 330, under the
app's status-bar inset, and reads its eight-entry menu, which fits neither
side, capped beside the point with its scrollbar, Down scrolling Select All
into view, and closed by a resize with focus back in the editor: clamped, it
lay over the point.
Remove the dropdown half of this patch (`src/popover.rs`, `src/styled.rs`)
and the fallback menu's (`src/native_menu/fallback.rs`) when upstream lets an
application keep popups clear of a region at the window's foot, and those
tests pass without it. Remove the context-menu half
(`src/menu/context_menu.rs`) when upstream places a context menu as it places
a dropdown, and the first test passes without it; since no app menu uses the
kit's context menu, a kit update may instead drop that half rather than carry
it forward, with that test's right-click case.

The popup-gap patch modifies `src/theme/mod.rs`, `src/styled.rs` and
`src/popover.rs`. Upstream keeps a popover's fill a quarter rem from its
trigger (`mt_1`, `mb_1`), whatever gap an application keeps elsewhere: at
GitTurtle's 13 px root a dropdown's drawn edge, the ring outside its fill,
stood 2 to 2.5 px from its button and 3.5 px at 18 pt, while the app's own
row menus drew their edge on their row. `Theme::popup_gap`, set in code, is
the page a popup leaves between its drawn edge and its trigger;
`popup_offset` turns it into the margin a popover keeps to its fill, the gap
and `POPOVER_EDGE_WIDTH`, and keeps upstream's quarter rem where the theme
sets no gap. The popover's content takes that margin on its anchor's side,
as it took `mt_1` and `mb_1`; the hover card, which builds the same content,
follows it. The consumer is every app dropdown (`AppDropdown`): the app sets
the gap with its text sizes (`appearance::popup_gap`, 4 px at 13 pt, scaled
with the text) and reads `popup_offset` for the room on either side of a
button (`DropdownRoom`), its Linux Main Menu (`PrimaryMenu`, on the base
popup host) takes `popup_offset` as its top padding and for its room, and its
row menus keep the same gap from their row (`menu_offset`), so every menu
stands one gap from what it opened from.
`cargo test --locked -p gitturtle menus_keep_one_gap_from_their_button_or_row`
reads that gap below and above a button and above a right-click's row at 13
and 18 pt, `a_keyboard_row_menu_keeps_the_menu_gap_below_its_row` below
a keyboard's row, and, on Linux,
`the_main_menu_keeps_the_menu_gap_below_its_button` below the Main Menu's
button, whose 4 px of padding left its edge 3 px from it at 13 pt. Remove this part when upstream lets an application set the
gap a popover keeps from its trigger, and those tests pass without it.

The opener-clip patch modifies `src/styled.rs`, `src/popover.rs` and
`src/native_menu/fallback.rs`. A popup's shadow (`popover_shadow`) falls
4 px with a 3 px blur, the blur's sigma in GPUI's shader, so its ink reaches
about 9 px below the fill: a menu opened above its trigger, a row, or the
pointer of an input's right-click laid it over what it was opened from,
about 9 % black on the first pixels of a row 1 px below the fill and 2 %
still at a 4 px gap. `ClearOfOpener` wraps a popup so that it is drawn
within a content mask ending at the line where its opener begins, given by
`Opener`: above or below the fill by the popover's offset, or about a point,
the side taken from where the fill lies; the fill and its drawn edge lie
inside the mask, and only the shadow's tail on the opener's side is cut. It
adds no layout node, as GPUI's `deferred` adds none, so the popup host
measures and places the popup as before. The popover wraps its content on
its anchor's side, and the fallback menu wraps its menu at the device pixel
row the pointer lies in (`Opener::Row`, its edges from the base
`point_device_row`), so the mask ends exactly where that row begins, above
the menu or below it, wherever the menu's placement put its fill. Cut at the ring instead, `POPOVER_EDGE_WIDTH` past the fill, the
shadow stopped up to two device pixels short of the row above a pointer at
scale 2, where a menu half a pixel taller than a whole one moves up to a
whole pixel, while below a pointer at 1.5 GPUI's outward rounding of the
mask carried it to the row. `ClearOfOpener::mask` takes the window's
viewport, and the mask's band reaches the least power of two at least twice
its larger side past the window's origin on every side
(`ClearOfOpener::reach`), so it crosses any window whole and the f32 size
from its far edge keeps the line it ends at to a few thousandths of a pixel:
a band a million pixels out kept the line to a sixteenth, and GPUI's outward
rounding could carry a mask a whole device pixel past it at a fractional
scale, and one 10,000 px out clipped a popup in a window wider than that, as
an X11 window across three 4K monitors is. A row's edges are drawn a
hundredth of a pixel toward the popup, so that the rounding of their product
with the scale lands on the row's own boundary.
`scripts/vendor-crate-test.sh vendor/gpui-component styled::` reads every
opener's band across a 12,000 × 800 and a 600 × 12,000 window
(`an_openers_mask_crosses_a_window_of_any_size`) and a pointer row's line on
its device pixel at scale 1.5
(`an_openers_mask_keeps_its_line_on_its_device_pixel`). The consumers are the app's dropdowns
(`AppDropdown`), the theme editor's `ColorPicker` popover
(`crates/app/src/theme_editor.rs`), its row menus, which mask themselves
through `ClearOfOpener::mask` at their row (`RowMenu`), and the drawn menu of
every input on Linux. Three cases keep their shadow uncut: a popup whose fill
spans what it opened from, clamped over it in a window too short for either
side, takes no mask (`ClearOfOpener::mask` gives `None`); a popover anchored
at its trigger's side (`Anchor::LeftCenter`, `RightCenter`) takes no wrapper,
and no app popover is anchored so; and GPUI rounds every content mask out to
whole device pixels (`cover_bounds`), so a mask whose end falls part way into
one, as the 5.5 px gap at 18 pt can at scale 1, lets the shadow's tail reach
one device pixel into the button or row.
`cargo test --locked -p gitturtle menus_draw_nothing_over_their_button_or_row`
reads the content mask of a dropdown's surface below and above its button
and of a right-click menu above its row, and
`an_inputs_menu_draws_nothing_on_the_pointers_row` that of a code editor's
menu below and above the pointer's pixel row, at scales 1, 1.5 and 2 for a
pointer on a whole pixel and part way into one, beginning or ending exactly
at that device row's edge; each fails without the mask, and the last with a
mask cut at the ring.
Remove this part when upstream keeps a popup's shadow off what it opened
from, and those tests pass without it.

The input-menu patch also modifies `src/native_menu/fallback.rs`. The
fallback menu gives the base `Positioner::point` a gap of
`POPOVER_EDGE_WIDTH` ([base patch](../gpui-base/GITTURTLE-PATCH.md)), so its
drawn edge, the ring painted in the band outside its fill, lies on the pixel
row next to the pointer's, below it or above it, never on it, as the app's
row menus keep their edge clear of their row. With its fill at the pointer,
the ring lay on the row clicked, and a pointer on a whole pixel, as X11
reports one, lay inside the fill's top edge.
`an_inputs_menu_opens_at_the_pointer_where_it_fits_below` and
`an_inputs_menu_opens_wholly_above_the_pointer_where_it_does_not_fit_below`
read the drawn edge clear of the pointer's row at scales 1 and 2, for a
pointer on a whole pixel and part way into one.
`an_editors_menu_too_tall_for_either_side_scrolls_beside_the_point` now
right-clicks low in the code editor, where the room above the pointer is the
larger, and reads the capped menu drawn wholly above the point and taller
than the room below, so a cap at the smaller room fails it. The fallback
also keeps its 8 px window margin from its drawn edge rather than its fill,
giving the positioner a margin of 8 px and `POPOVER_EDGE_WIDTH`
(`popup_fill_margin`, below): the room
`Positioner::point_room` gives is measured to the fill, so a menu capped at
a room shorter than its first row, whose items column then takes the whole
room, ended its fill at the margin and drew its ring 7 px from the window's
top. The margin holds on all four sides, so the fallback's fill stays 9 px
inside the window's top and sides, as every popup host's now does (the
window-margin patch below). At the window's
foot the application's inset already counts the edge, so nothing there
moves.
`an_inputs_capped_menu_keeps_its_drawn_edge_inside_the_window_margin`
right-clicks a code editor at 18 pt in a 461 × 110 window, where the room
above the pointer holds less than one row, and reads the capped menu's
drawn edge 8 px inside the window's top. Remove this part with the fallback
menu's placement.

The fallback's items patch modifies `src/native_menu/fallback.rs`. Upstream
built every native item as a check item (`menu_with_check_and_disabled`, or
`PopupMenuItem::checked` for one with an icon), and since the
checked-menu-item patch makes a check item checkable, a Linux screen reader
heard each item of an input's menu, Cut, Copy, Paste and Select All among
them, as an unchecked check box. A `NativeMenu` item says only whether it is
ticked, as a macOS or Windows menu item's state does, so the fallback now
builds a check item only for a ticked one, as the kit's `with_menu_items`
makes only a ticked platform action checkable, and a plain item otherwise.
`cargo test --locked -p gitturtle an_inputs_menu_announces_its_items_as_plain_items`
right-clicks the Working Changes filter and reads each item's role in its
debug selector as a plain menu item, and
`scripts/vendor-crate-test.sh vendor/gpui-component native_menu` runs
`native_menu::fallback::tests::only_a_ticked_item_is_a_check_item`, which
builds ticked and unticked items, with an icon and without, and reads each
ticked one a ticked check item and each unticked one plain. Remove this
part when upstream builds the fallback's unticked items plain and those
tests pass without it.

Where the fallback menu's focus goes: its actions go to `action_context`,
the focus the window held as it opened, and focus the menu holds returns
there when it closes, by an item (`PopupMenu::dispatch_confirm_action`),
Escape (`PopupMenu::dismiss`) or a resized window. For an input's
right-click that is the input itself: GPUI focuses a focusable element under
any mouse-down, the right button's included, and the input opens its menu
on the release, so Cut, Copy and Paste act on the field clicked, whatever
held focus before, as on macOS.
`cargo test --locked -p gitturtle an_inputs_menu_acts_on_the_field_right_clicked`
guards it: with focus in the Working Changes list, a right-click in the
filter and its Paste put the clipboard's text in the filter, with the list's
selected row kept, and Escape and a resize return focus to the filter. The
docs of
`NativeMenu` (`src/native_menu/mod.rs`), `Input::context_menu`
(`src/input/input.rs`) and the fallback said the menu is a native OS menu,
or left where focus returns unsaid; they now say it is the OS's own on macOS
and Windows and drawn by the kit elsewhere, Linux among them, and where its
actions and focus go. These are comments only.

The menu-confirm patch modifies `src/menu/popup_menu.rs`. Upstream's
`confirm`, which Enter and a click run, acted on whatever entry was selected.
The pointer's hover selects any row, a disabled item or a label among them, so
Enter with the pointer resting on a disabled item ran its handler, a command
the menu showed as unavailable. `confirm` now acts only on an entry that can
act (`is_clickable`, the test the menu keys already use); on any other it does
nothing and the menu stays open. The consumers are every kit menu with a
disabled item, among them the Linux Menu, the Working row menus and the
History commit menu during a write.
`cargo test --locked -p gitturtle enter_under_the_pointer_acts_only_on_an_item_that_can_act`
rests the pointer on a disabled item, a label and a closing caption in turn
and presses Enter: nothing runs and the menu stays open; on an available item
Enter acts. Remove this part of the patch when upstream's `confirm` passes over
entries that cannot act, and that test passes without it.

The `usize`-constant patch modifies `src/highlighter/highlighter.rs`. Upstream
imports the `std::usize` module (`use std::{…, usize}`), so the `usize::MIN`
and `usize::MAX` in `HighlightSummary`'s `Summary::zero` and in
`unique_styles`'s boundary sentinel named that module's constants. Rust 1.99,
which `rust-toolchain.toml` pins, deprecates the module, and because this crate
is a path dependency its eight deprecation warnings, the import and seven uses,
appeared in every GitTurtle build. The import is removed, so the same paths now
name the primitive's associated constants, which have the same values. Nothing
else changes, and no behavior changes.

The consumers are the kit editors the app opens with a grammar (`text::editor`
in `crates/app/src/text.rs`, Compare's split diff in
`crates/app/src/split_diff.rs`, and the Markdown source view and pull request
descriptions, which use `markdown`), which reach `SyntaxHighlighter::styles`
and its `unique_styles` through `src/highlighter/input_adapter.rs`, and the
kit's code-block highlighter in `src/text/mod.rs`.

`cargo test --locked -p gitturtle -- text::tests:: split_diff::tests:: markdown_view::tests::`
runs those app paths, including
`patch_headers_and_changes_draw_palette_colors_in_every_built_in`, which
composes `SyntaxHighlighter::styles` with the patch decorations, and the
Markdown source view's find. The crate's own `highlighter` tests, among them
`test_unique_styles`, cover the sentinel directly. Both sets pass with and
without the patch; without it the build only warns. The highlighter tests
need `--features tree-sitter-languages`, which
`scripts/vendor-crate-test.sh` does not pass (its arguments go to the test
harness), so they ran in a scratch workspace with the root patches and
lockfile and that feature. Remove this part of the patch when upstream drops
the import.

The injection-deadline patch also modifies `src/highlighter/highlighter.rs`.
Upstream stops each injected layer's parse (a Markdown file's inline layer, a
fenced code block's language) after 20 ms of wall time and leaves that layer
out, even under `SyntaxHighlighter::update` without a timeout, which its
documentation says parses to completion. A complete highlight therefore
depended on how long the thread ran: on a loaded machine one pass over a short
Markdown document lost its inline layer (code spans, emphasis, links) and the
next kept it, and a long paragraph lost it on every pass. `update` without a
timeout now parses its injected layers to completion. With a timeout each
injected layer keeps the 20 ms bound, as does the editor's background reparse
(`compute_injection_layers`), so the editor's UI-thread work is unchanged. The
consumers are the kit's code-block highlighter in `src/text/mod.rs`, which
highlights without a timeout, and the application's syntax-theme tests in
`crates/app/src/appearance.rs`:
`cargo test --locked -p gitturtle a_complete_markdown_highlight_keeps_inline_styles_however_long_they_parse`
fails without the patch, and `a_diff_fence_in_markdown_draws_the_patch_colours`
failed in 6 of 30 runs on a loaded host without it. Remove this part of the
patch when upstream bounds injected parses only under a timeout and those tests
pass without it.

The ghost-hover patch modifies `src/button/button.rs` and adds
`Theme::ghost_hover` to `src/theme/mod.rs`. Upstream fills a hovered ghost
Button with `secondary` lightened by 10% in a dark theme, or darkened by 10% in
a light one, at 80% opacity. `Colorize::lighten` scales the lightness, so on a
dark `secondary` (GitTurtle's `subtle` surface) the fill moves by a step or two
and a hovered ghost Button, such as Compare's narrow review arrows, shows no
lift. A hovered, enabled ghost Button now takes `Theme::ghost_hover` when it is
set; `ghost_hover` is `None` by default, set in code and skipped by theme files,
so an application that leaves it alone keeps upstream's fill. A disabled ghost
Button still paints no hover, as every disabled Button does.

The consumers are every ghost Button the app draws or the kit draws for it: in
the app, Compare's review arrows, Options, Hide whitespace, Context and Reset
review (`crates/app/src/text_review.rs`), editor Find's buttons
(`crates/app/src/editor_find.rs`), the branch, tag, reflog, recovery, worktree,
project and project-pane rows and actions that call `.ghost()`; in the kit, the
dialog and sheet close buttons, input clear and search buttons, pagination,
tab-bar, dock, sidebar, notification and date-picker controls. The app sets
`ghost_hover` to `Palette::control_fill(hover)`, the hover its shared `button`
helper takes, where it installs the button ring (`crates/app/src/appearance.rs`).
`cargo test --locked -p gitturtle narrow_review_arrows_lift_on_hover` hovers the
narrow review arrows in Midnight and Daylight and reads the scene: no fill at
rest, the shared hover fill when hovered, and none when an operation disables
them. A pressed ghost Button, and a selected one, which keeps the pressed fill,
take `Theme::ghost_active` the same way: upstream's `secondary` lightened or
darkened by 20% at 80% drew a pressed ghost Button under its own hover in a dark
theme and a selected one (the Reflog's current entry) almost invisibly. The app
sets it to `Palette::control_fill(selected)`, the pressed layer its shared
helper takes; `pressed_ghost_buttons_take_the_selected_layer` holds the narrow
review arrow down in Midnight and Daylight and reads that fill. Remove this part
of the patch when upstream derives the ghost hover and pressed fills so a dark
`secondary` visibly lifts, or offers equivalent settings, and those tests pass
without it.

The checked-menu-item patch modifies `src/menu/popup_menu.rs` and
`src/menu/menu_item.rs`. Upstream draws a checked item's tick only as an icon
and gives every item `Role::MenuItem` with `aria_selected`, so a ticked item is
announced like any other and its state is lost. `PopupMenuItem::Item` and
`ElementItem` now carry `checkable`, which `PopupMenuItem::checked` sets and the
`menu_with_check*` builders reach through it; the plain builders no longer call
`checked`, so their items stay plain. A checkable item renders as
`Role::MenuItemCheckBox` with `aria_toggled` true or false; a plain item keeps
`Role::MenuItem`. A platform menu action (`with_menu_items`) says only whether it
is ticked, so only a ticked one becomes checkable. Radio semantics are not
added: the API does not say whether a group is exclusive. Each item also names
its role and state in a debug selector (`menu-item: <label> · <role>
[<toggled>]`), because GPUI's test platform never activates accessibility and a
test cannot read a menu's AccessKit nodes.

The consumers are the app's menus that tick a choice: the review Options menu
(`crates/app/src/text_review.rs`, Hide whitespace checkable, Context and Reset
review plain), the branch chooser (`branch_actions.rs`, every branch), the model
view and animation clip menus (`model_view.rs`, `model_view/playback.rs`), the
project pane's Move to menu (`project_pane.rs`), the profile menu
(`profiles.rs`), the repository tab list (`repository_tabs.rs`) and the theme
editor's base theme menu (`theme_editor.rs`).
`cargo test --locked -p gitturtle review_options_menu_announces_its_toggle_checked`
opens the narrow review Options menu and reads Hide whitespace as a check menu
item, unticked and then ticked, and Context and Reset review as plain items;
`text_review::tests::collapsed_options_menu_keeps_each_buttons_state` pins which
items are checkable. Remove this part of the patch when upstream exposes a
checkable item's state to accessibility and those tests pass without it.

The disabled-menu-item patch modifies `src/menu/menu_item.rs`. Upstream draws
a disabled menu item dim and gives it no click action, but GPUI's Div has no
aria-disabled setter, so the item's AccessKit node carried no disabled flag,
and Orca on Linux, which reads AT-SPI's enabled and sensitive states from that
flag, announced it as available. Each item now renders through
`AccessibleMenuItem`, which wraps the item's element as gpui-base's
`AccessibleButton` wraps a Button's: layout, paint, events and synthetic
children delegate to it, and writing the node adds only the disabled flag when
the item is disabled. The debug selector the checked-menu-item patch added is
now built, in debug builds only, from the node the wrapper writes, so it names
what the item announces: `menu-item: <label> · <role>[ <toggled>][ disabled]`.

The consumers are every menu with an unavailable action: the review Options
menu's Reset review, and More context at its last step
(`crates/app/src/text_review.rs`), the project pane's disabled actions and Move
to destinations (`project_pane.rs`), the profile menu's profiles where none can
apply (`profiles.rs`), the Linux Menu's unavailable actions
(`with_menu_items`) and the context menus.
`cargo test --locked -p gitturtle review_options_menu_announces_its_toggle_checked`
opens the narrow review Options menu and reads Reset review announced disabled
while nothing differs from the original diff, and available once whitespace is
hidden. How AT-SPI reports the flag is the `accesskit_atspi_common` backport's,
covered by `native_accessibility::atspi_state_tests`. Remove this part of the
patch when upstream menu items expose their disabled state to accessibility and
that test passes without it.

The menu-label patch modifies `src/menu/menu_item.rs`,
`src/menu/popup_menu.rs`, `src/styled.rs` and `src/theme/mod.rs`. Upstream renders a
`PopupMenuItem::Label` and a separator as disabled menu items: a label was
announced as one more command, which Orca counted as a fourth, dead item in
the review Options menu, and drew in the disabled colour, so the Options
menu's caption read 2.97:1 in Midnight and 2.19:1 in Braden, the colour of
the disabled Reset review above it, where in the row it was secondary text at
7.70:1 and 6.21:1. `MenuItemElement` now knows what an entry is (`Entry`): an
item keeps upstream's roles, highlight, click and disabled look; a label is
`Role::Label`, a separator `Role::Splitter`, which AT-SPI reports as a label and
a separator; neither takes the highlight, the selected state or the disabled
flag. A label draws in `menu_label_color`, the theme's `muted_foreground`, and
its text sets that colour and names it in a debug selector (`menu-label:
<label> · <rgba>`), because GPUI's test scene does not expose glyph colours.
Colour alone did not set a label apart: drawn in secondary text at an item's
size and weight, the profile menu's headings read only 1.86:1 off its items in
Midnight and 1.93:1 in Braden, and One Dark's and Solarized Dark's secondary
text is nearly their text, so a heading read as one more command. A label's
row now draws at medium weight (`MENU_LABEL_WEIGHT`) and at
`menu_label_size`: `Theme::menu_label_size`, `None` by default and set in code,
not read from theme files, or the items' own `text_sm`. GitTurtle sets it to
its 11 px section-label size, scaled with the interface text
(`appearance::SECTION_LABEL_TEXT`, through `set_theme_text_sizes` on every
palette and text-size change), beside items at 11.375 px, so weight sets a
heading apart without it outweighing the items, as 12 px did in One Dark and
Solarized Dark. The row names its size and
weight in a debug selector (`menu-label-style: <label> · <size> px · weight
<weight>`), since the test scene does not expose glyph sizes either.

The consumers are every menu with a label: the review Options menu's caption
(`crates/app/src/text_review.rs`), the project pane's row name and Move to
heading (`project_pane.rs`), the profile menu's current identity, Saved Git
profiles heading and notes (`profiles.rs`), the branch chooser's heading and notes
(`branch_actions.rs`), the model view and animation clip menus' headings
(`model_view.rs`, `model_view/playback.rs`), the repository tab list's heading
(`repository_tabs.rs`) and the navigator's branch and worktree context menus'
headings (`views.rs`).
`cargo test --locked -p gitturtle menu_labels_read_as_secondary_text_and_disabled_items_as_disabled`
opens a menu that starts with a label and ends with a caption below a
separator, reads both labels announced as labels and neither disabled, the
disabled item announced disabled, and, in every built-in and embedded Omarchy
palette, both labels' text in `muted_foreground`.
`menu_labels_take_the_section_label_size_and_differ_from_items_by_weight`
reads, in every such palette at 13 and 16 pt, both labels at the 11 px
section-label size, scaled, and medium weight, no larger than the items and
heavier than their regular weight.
`review_options_menu_announces_its_toggle_checked` reads the Options menu's
caption the same way, and `the_profile_menu_names_its_rows` the profile menu's
labels by name. Remove this part of the patch when upstream gives menu labels a
non-item role, a colour distinct from a disabled item's and a style distinct
from an item's, and those tests pass without it.

The menu-keys patch modifies `src/menu/popup_menu.rs` and
`src/menu/menu_item.rs`. Upstream's first Down
selects a menu's first entry whatever it is, Down past the last item returns
to it, and Right selects a submenu's first entry, so in a menu that opens with
a label the first Down highlighted text that cannot act and Enter did nothing.
Up and Down now move only between the items the keyboard's cursor stops on,
from the first of them, and wrap to the first or last such item; Home and End,
which upstream left unbound, move to the first and last (`SelectFirst`,
`SelectLast`). The cursor stops on an item that can act and on a disabled item
marked as unavailable for a reason it states (`PopupMenuItem::reason_stated`,
`is_keyboard_stop`); a label, separator or disabled item that states no reason
is passed over, and a menu with no stop selects nothing. On a stated reason's
item the cursor is drawn as on any other (`src/menu/menu_item.rs` draws the
keyboard's cursor on a disabled item, which only such an item carries) and the
item is marked selected, so AT-SPI reports it as the menu's selected item, its
accessible name stating the reason; Enter and a click do nothing there
(`confirm` acts only on `is_clickable`, and a disabled row is wired to no
click), and the next key moves on. Passed over as every disabled item was, the
Linux Main Menu's Browse stashes…, waiting for a fetch, was never reached, and
Orca, from F10 and Down, never spoke why it waited (QA-89); the owner decided
that an item stating a reason stays in the keyboard's order, and the
coordinator that this holds for a row that states why it is unavailable or
waits alike. The app marks every such row where it builds one
(`crates/app/src/native_accessibility.rs`, `detail_row`, behind both
`waiting_item`, a write's wait, and `detailed_item`, an item's own
condition, as a Working row's "Ignore…" with "Untracked files only" or the
branch picker's branch with "Another worktree"). A mark rather than
an inference from the label keeps a disabled item whose accessible name only
says more than its label, as "More context, 3 lines shown" does, passed over.
A submenu opened from the keyboard selects its first item that can act; a
submenu is never marked, since one opens where it is selected. `PopupMenu::select_first_item`
places the keyboard's cursor on a menu's first item that can act, as a menu the
keyboard opened shows it; upstream offers no way to set a menu's selection, so a
keyboard's menu highlighted nothing until the first Down. The caller decides when
the keyboard opened the menu: the app calls it from its one menu opener
(`crates/app/src/native_accessibility.rs`, `open_menu`) for a dropdown its
button's Enter or Space opened, a row's Shift-F10 or Menu key menu and the Linux
Main Menu opened with F10.

The consumers are the menus that open with a label: the project pane's row
menu, opened with Shift-F10 or its button (`crates/app/src/project_pane.rs`),
the profile menu (`profiles.rs`), the branch chooser (`branch_actions.rs`), the
model view and animation clip menus (`model_view.rs`,
`model_view/playback.rs`), the repository tab list (`repository_tabs.rs`) and
the navigator's context menus (`views.rs`); the review Options menu no longer
needs its caption last to keep the first Down (`text_review.rs`).
`cargo test --locked -p gitturtle menu_keys_pass_over_labels_and_disabled_items`
drives a menu that opens with a label and a disabled item and closes with a
caption, reads the highlight from the scene after each Down and Up, and chooses
the item Down reached with Enter;
`a_keyboard_row_menu_starts_on_its_first_action` opens the project pane's row
menu with the keyboard and renames the group with Enter alone;
`a_menu_the_keyboard_opens_starts_on_its_first_action` opens the same menu with
a click, which highlights nothing, and with Enter on its button, which
highlights its first available item.
The crate's own `menu::popup_menu::tests::the_menu_keys_rest_on_an_item_that_states_why_it_waits`
walks One, Two (disabled, stating why), Three (disabled, stating nothing),
Four and Five (an element item stating why) with Down, Up, Home and End,
reads the cursor on Two and Five, drawn in the cursor's fill and announced by
the names that state their reasons, presses Enter and clicks on Two with
nothing run and the menu open, and never reads it on Three;
`the_menu_keys_pass_over_a_disabled_item_that_states_no_reason` builds the
same items unmarked and reads Down, Up, Home and End passing over Two and
Five. With the walk back on `is_clickable` the first fails at Two, and with the
cursor drawn only on an item that can act it fails at Two's fill.
`cargo test --locked -p gitturtle the_main_menu_keys_rest_on_browse_stashes_while_it_waits`
opens the Linux Main Menu with F10 during a fetch, walks Down onto Browse
stashes…, reads the cursor drawn there and the row named "Browse stashes…,
available when fetching finishes", presses Enter with no Stashes opened and the
menu open, and moves on and back with Down, Up, Home and End; with the walk on
`is_clickable` Down passes it by and the test fails.
`cargo test --locked -p gitturtle a_keyboard_row_menu_rests_on_an_item_that_says_why_it_is_unavailable`
opens a tracked file's Working row menu with Shift-F10, walks Down from Stage
onto "Ignore…, untracked files only", presses Enter with no ignore prepared and
the menu open, and moves on and back with Down, Up, Home and End;
`menu_keys_stop_on_a_stated_reason_and_pass_over_a_silent_one` builds the app's
`detailed_item` row beside a disabled item that states nothing and reads the
keys stopping on the first and never on the second;
`the_branch_pickers_keys_rest_on_a_branch_another_worktree_holds` opens the
branch picker with a branch another worktree has checked out, walks Down from
Find or create a branch… onto "review, another worktree", presses Enter with
no switch started and the menu open, and moves on and back with Down and Up.
Without the mark in `detail_row`, each fails at Down.
Remove this part of the patch when upstream menus move only between items that
can act, or state why they are unavailable or wait, and bind Home and End, and those tests pass
without it.

The dropdown-trigger patch modifies `src/menu/dropdown_menu.rs` and
`src/popover.rs`. `DropdownMenu` copies its trigger's style into
`Popover::trigger_style`, which upstream stores and never applies. The popover
lays the trigger out in a box of its own, the box the trigger's siblings see,
so the trigger's flex-item properties never reached the flex item: a trigger
set to `flex_1` kept its content width. Its margins did reach the row, because
a block box counts its child's margins in its own size. The trigger's
placement, its margins, flex grow, shrink and basis and self-alignment, now
moves from the trigger to that box (`take_placement`), which `Popover::render`
styles with it. The rest of the trigger's style stays on the trigger: its size,
padding, colours and opacity would apply twice on both boxes.

The consumers are the Buttons that open a dropdown menu: the review Options
button and its `mr_neg_2` (`crates/app/src/text_review.rs`), the current-branch
button and its compact `flex_shrink(1.)` (`branch_actions.rs`), the project
pane's row button and its `flex_shrink_0` (`project_pane.rs`), and the profile,
Workspaces, recovery Actions, model view, animation clip and theme editor Base
buttons. None of them moves: their margins already applied, and their flex
values equal the box's default or cannot change its size.
`cargo test --locked -p gitturtle a_dropdown_trigger_keeps_its_place_among_its_siblings`
lays out a `flex_1` trigger with `mr_2` beside a 50 px sibling in a 300 px row
and reads the trigger filling the row with one margin before the sibling;
`narrow_review_caption_shows_whole_or_leaves_the_row` reads the review
caption's text one gap after Options, its margin applied once. Remove this
part of the patch when upstream applies a dropdown trigger's placement to the
box it lays the trigger out in, and those tests pass without it.

The menu-item accessible-name patch modifies `src/menu/popup_menu.rs`.
Upstream names an item's AccessKit node with its visible label and gives an
element item no name, so an item could not be announced with more than it
shows, as an icon Button can through its accessibility label.
`PopupMenuItem::accessibility_label` sets that name on an item, element item or
submenu; the drawn label is unchanged, and an item without one keeps
upstream's name. The consumer is the narrow review Options menu, whose "More
context (3 lines)" is announced "More context, 3 lines shown", the full row's
button's name (`crates/app/src/text_review.rs`).
`cargo test --locked -p gitturtle context_control_says_what_it_does_and_how_much`
opens the narrow Options menu and reads the item's node named as the button,
not by its label, and
`text_review::tests::collapsed_options_menu_keeps_each_buttons_state` pins
each item's label and name. Remove this part of the patch when upstream menu
items take an accessible name apart from their label and those tests pass
without it.

The menu-cursor patch modifies `src/menu/popup_menu.rs` and
`src/menu/menu_item.rs`. Upstream draws a menu's selected item, whether the
keyboard or the pointer put it there, in the theme's `accent` fill, which is
the keyboard's only cursor in a menu. GitTurtle sets that fill to its quiet
hover surface, 1.19:1 off the menu in Midnight and 1.23:1 in Braden, below the
3:1 a state indicator needs. `PopupMenu` now remembers whether the keyboard
placed the selection (Up, Down, or Right into a submenu, through
`set_selected_index`); the pointer entering an item takes the selection back.
`MenuItemElement::keyboard` passes that on, and a selected item the keyboard
placed draws as the kit's lists draw their active row: the `list_active` fill
and a 2 px leading border in `list_active_border`, on a child laid over the
item with the item's corner radii, so it moves nothing. The pointer's hover and
a pointer-placed selection keep upstream's `accent` fill, and a keyboard item
under a resting pointer keeps the cursor rather than the hover fill. A theme
that sets neither list token draws the kit's defaults.

The consumers are every kit menu the keyboard drives: the Linux Menu, context
menus and dropdown menus such as the review Options, profile and project row
menus. GitTurtle sets `list_active` to its `selected` row surface and
`list_active_border` to `accent` (`crates/app/src/appearance.rs`), which holds
3:1 on `selected` and every page surface; its pickers draw the same marker
(`appearance::cursor_marker`).
`cargo test --locked -p gitturtle the_keyboard_cursor_in_a_menu_reads_three_to_one_in_every_palette`
moves the cursor with Down and reads the item's painted fill and marker, at
3:1 or more against the fill and the menu, in every built-in and embedded
Omarchy palette, then hovers another item and reads the hover fill without a
marker there and nothing on the item the keyboard had;
`menu_keys_pass_over_labels_and_disabled_items` finds the cursor by its fill.
Remove this part of the patch when upstream marks a keyboard-selected menu item
apart from the pointer's hover with a cue a theme can hold at 3:1, and those
tests pass without it.

The menu-width patch modifies `src/menu/popup_menu.rs`. Upstream capped a menu
only at its own maximum, 500 px unless the caller sets one, and let a row's text
wrap wherever the menu was narrower than it. In a window narrower than that
maximum, such as a 461 px half tile, a menu with a long row, such as the profile
menu's current identity, ran past the window's edge, and an item's wrapped
text overflowed its fixed height. `PopupMenu::max_width` now also caps a menu at
its window's width less `popup_fill_margin` on either side, the 9 px the
popup hosts keep its fill from the window's sides, so its drawn edge stays
8 px in (the window-margin patch below). Every row's
text, an item's, a submenu's and a label's, keeps one line and ends with an
ellipsis where the menu is narrower than it (`menu_row_text`, and `truncate` on
a label's text), its row and text shrinking below their content (`min_w_0`);
its accessible name keeps the whole text. A menu as wide as its content is
unchanged; only one at its cap cuts a row, where a label used to wrap to more
lines instead. A test build names each item's and submenu's text in a debug
selector (`menu-row-text: <text>`).

The consumers are every kit menu: the profile menu's current identity and
saved profiles (`crates/app/src/profiles.rs`), and any menu whose row is
longer than its cap, such as a worktree's path heading in the navigator's
worktree menu (`crates/app/src/views.rs`). The profile picker's tooltip keeps
the whole current identity.
`cargo test --locked -p gitturtle the_profile_menu_keeps_a_long_identity_inside_a_narrow_window`
opens the profile menu in a 461 × 490 window with a long name and email and
reads each row inside the menu's fill, 9 px inside either side, named by its
whole text, and the current identity and the saved profile on one line as tall as a
short row's. Remove this part of the patch when upstream keeps a menu inside its
window and its rows on one line, and that test passes without it.

The window-margin patch modifies `src/styled.rs`, `src/popover.rs`,
`src/menu/context_menu.rs`, `src/menu/popup_menu.rs`,
`src/menu/app_menu_bar.rs` and `src/native_menu/fallback.rs`, with the base patch's `Popup::margin`,
`Popover::margin` and its whole-pixel rounding at a popup's side margins
([base patch](../gpui-base/GITTURTLE-PATCH.md)). Upstream's popup hosts keep a
popup's box 8 px inside the window, the base `Popup` host and the kit's
context menu by the positioner's margin and a submenu by GPUI's anchor, 4 px
in; a kit popup draws its edge, the ring, in the band outside that box
(`POPOVER_EDGE_WIDTH`), so a menu moved in from a window's side drew its edge
7 px from it, a submenu 3 px, while the input's drawn menu (the input-menu
patch above) kept its edge 8 px in. GitTurtle's decision (2026-11-01) is that
every menu's drawn edge, not its fill, stands 8 px from the window's sides,
since the edge is what the eye reads as a menu's side. `POPUP_WINDOW_MARGIN`
names those 8 px and `popup_fill_margin` the 9 px a host keeps the fill in.
The kit's `Popover` hands it to the base popover's host (`Popover::margin`),
so a dropdown menu and a popover keep it; `dropdown_popup`, the Select,
Combobox and DatePicker surface, and the context menu give it to their
positioner; the fallback menu takes it in place of its own constant; and
`PopupMenu::max_width` caps a menu at the window less it on either side, so a
menu as wide as its cap keeps its edge 8 px in on both. `AppMenuBar`'s
menus, which GPUI's anchor moves in only once they run past the window, snap
with it in place of 8 px. A submenu keeps GPUI's
anchor, which decides the side it opens on and so which of Left and Right
enters and leaves it, and is drawn through `InsideWindow`, which moves the
submenu as its anchor placed it, its offset from the corner included, by
whole pixels only as far as keeps its fill `popup_fill_margin` inside the
window: GPUI's anchor moved it only once that corner, not the submenu, ran
past the window. `InsideWindow` adds no layout node and moves nothing that
lies that far in already.

The consumers are every app dropdown (`AppDropdown`, on the kit's dropdown
menu and popover), the theme editor's `ColorPicker` popover, the app's row
menus, which take the same margin from `POPUP_WINDOW_MARGIN` for their room
(`menu_bounds` in `crates/app/src/native_accessibility.rs`), its Linux Main
Menu (`PrimaryMenu` in `crates/app/src/platform_polish.rs`, on the base popup
host with `Popup::margin(popup_fill_margin())`), and the drawn menu of every
input on Linux; no app menu has a submenu or opens through the kit's context
menu or `dropdown_popup`.
`cargo test --locked -p gitturtle menus_keep_their_drawn_edge_8_px_from_the_window_sides`
opens, at 13 and 18 pt in a 461 × 490 window, an app dropdown on a button at
either side of the window, a row's right-click menu 3 px from either side, and
a kit dropdown's submenu that runs past either side, and reads each drawn
edge 8 px from that side;
`the_main_menu_keeps_its_drawn_edge_8_px_from_the_windows_side`, on Linux,
reads the Main Menu's 8 px from the window's left side, where its button
leads the tab strip; and
`the_profile_menu_keeps_a_long_identity_inside_a_narrow_window` reads a menu
at its width cap, its fill 443 px wide in a 461 px window;
`scripts/vendor-crate-test.sh vendor/gpui-component app_menu_bar::` opens
a bar's Edit menu past a 150 px window's right side
(`a_bar_menu_keeps_its_drawn_edge_8_px_from_the_windows_side`), and no app
view has a menu bar. Remove this part
when upstream keeps a popup's drawn edge, not its box, inside the window's
margin, and those tests pass without it.

The edge-pixel patch modifies `src/menu/popup_menu.rs` and `src/popover.rs`
(a test), with the base patch's side-pixel rule
([base patch](../gpui-base/GITTURTLE-PATCH.md)). A popup's ring is a
shadow with no blur and a spread of `POPOVER_EDGE_WIDTH`, painted under the
fill over every device pixel the band outside the fill touches (GPUI's
`paint_drop_shadows`, `cover_bounds`), so its outer side stands 8 px from
the window's side only where the fill starts on a device pixel 9 px or
less than a device pixel more from it. The popup hosts moved a popup by
whole logical pixels from where they were laid out, which GPUI places on
the device pixel with halves toward zero: beside a trigger part way into a
pixel the fill lay half a pixel further in at scale 2, and at 1.5 a fill
could lie a device pixel nearer the side than its margin. Native QA
(2026-11-02) read the Linux Main Menu's edge at device x 17 to 18 and the
Settings profile menu's 17 device pixels from the window's right side,
8.5 px, where `menus_keep_their_drawn_edge_8_px_from_the_window_sides` and
`a_bar_menu_keeps_its_drawn_edge_8_px_from_the_windows_side` let up to a
pixel more pass. The base host now puts a popup on a side margin on the
device pixel at or inside it, and `InsideWindow` moves a submenu by whole
device pixels, `device_pixel_at_or_after` and `device_pixel_at_or_before`
from the base, so each fill moved in from a side lies on the device pixel
at or inside `popup_fill_margin` and its ring stands 8 px out from it at
scale 1, 1.5 and 2: 8, 12 and 16 device pixels. A popup inside the
margins, and its content, keep their places; a fill moved in from a side
moves by at most one device pixel, the move that takes its ring there.
`AppMenuBar`'s menus keep GPUI's anchor and its nearest pixel, and no app
view has a menu bar.
`scripts/vendor-crate-test.sh vendor/gpui-component a_popover_moved_in a_submenu_moved_in`
runs `a_popover_moved_in_from_a_side_draws_its_edge_8_px_from_it`, which
opens a popover from a trigger 6.5 px and 2 px from either side of a 460 px
window at each scale and reads its painted fill on the device pixels it was
laid out on and its ring's outer side by GPUI's shadow rule, and
`a_submenu_moved_in_from_a_side_draws_its_edge_8_px_from_it`, which moves
a submenu one and two device pixels past a whole pixel beyond either side
and the foot; the old placement fails each at scale 1.5 and 2. The app's
`menus_keep_their_drawn_edge_8_px_from_the_window_sides` reads each drawn
edge 8 px from its side, a dropdown beside a button half a pixel in among
them, and `a_rows_menu_keeps_its_drawn_edge_8_px_from_the_sides_at_fractional_scales`
reads a row's menu, which the app places itself in the same device-pixel
room, at 1.25 and 1.5. Remove this part with the window-margin patch.

The window-margin patch replaced the last use of `px` in
`src/menu/app_menu_bar.rs` outside its tests, the menu's 8 px snap
margin, and left the module importing it for its tests alone, through
`use super::*`: every build of the library on Linux and macOS warned
`unused import: px`. The test module imports `px` itself; the menu bar is
unchanged. Remove this with the window-margin patch.

The capped-menu patch modifies `src/menu/popup_menu.rs`. Upstream caps a
scrollable menu's items column (`max_h` with `scrollable`) but leaves its rows
free to shrink, as flex items are, so a menu shorter than its rows squeezed
every row and the gaps between them until they fit or reached their text's
height, and then cut its last row; nothing scrolled, and its scrollbar, in the
theme's default mode, showed only while scrolling. Every row now keeps its
height (`flex_shrink_0`). A menu whose rows run past its cap ends after its last
whole row with the padding an uncapped menu keeps below its last row, or at the
next row's top where the gap between rows is narrower, so no sliver of that row
shows; it never ends after a separator or a heading whose rows lie below
(`Fold`, `fold_rows`). Before its first layout the fold comes from the heights
the kit draws its entries at, each length snapped to device pixels as GPUI's
layout snaps it (`estimated_fold`), and each layout measures it from the rows'
bounds the column's scroll handle records (`measured_fold`); a menu whose
measured fold differs from the one it drew asks for the next frame, which draws
it again (the Working Actions menu's estimate matches its layout at 13 and 18
pt, so it draws once). The request is deferred out of the draw
(`Context::defer_in`): a notify in prepaint only marks the view, which that
frame has already rendered, and the draw clears the mark without asking for a
frame, so a fold the estimate missed stayed drawn at the estimate until
something else drew the menu. While
rows lie past the fold the column keeps a gutter of the scrollbar's width at
their right and the scrollbar, in a test build named `popup-menu-scrollbar`, is
always visible there, as a scrolling list's is; a menu whose rows fit draws
none and keeps its width. The
menu's own bounds now come from a canvas on its surface rather than inside the
scrolling column: upstream's canvas moved with the scroll offset and, as tall
as the column, made a menu whose rows fit scroll by the column's padding.

The consumers are every menu the app opens through
`native_accessibility::open_menu` (`crates/app/src/native_accessibility.rs`),
which caps each at the room above the status bar: the Working Actions menu,
the profile menu, the theme editor's Base menu and the Linux Main Menu among
them.
`cargo test --locked -p gitturtle a_capped_working_actions_menu_keeps_its_rows_and_scrolls`
opens the Working Actions menu at 13 and 18 pt in 1280 × 800, where it fits and
a wheel moves none of its rows, and in 461 × 490, where each row keeps its
height and its spacing from the row before it, the menu ends 8 px or more
above the status bar after a whole row, cuts none, and shows its scrollbar
beside its rows, and Up brings its last row whole into view;
`a_long_profile_menu_keeps_its_rows_and_scrolls`,
`the_base_menu_keeps_its_rows_and_scrolls` and, on Linux,
`a_capped_main_menu_keeps_its_rows` read the same of those menus. Remove this
part of the patch when upstream keeps a capped menu's rows whole and those
tests pass without it.

The still-pointer patch modifies `src/menu/popup_menu.rs` and
`src/menu/menu_item.rs`. Upstream lights a menu's rows twice over: the
pointer's selection, which an item takes when GPUI reports the pointer over
it, and the item's own group hover. When a wheel scrolls a capped menu under
a pointer that stays put, GPUI moves the selection to the row that arrived
under the pointer once the frame is drawn, but refreshes an element's own
group hover only on a pointer move, so the row that left kept its fill: at
13 pt in a 461 × 490 window one notch lit both Browse stashes…, which had left
the pointer, and Amend last commit…, which had arrived under it. A wheel that moves a scrollable menu's rows (`rows_scrolled`, on
the items column, after the column's own listener has added the notch to its
offset) now records the pointer's place; a pointer selection gives the
selection up and a keyboard one stays. Until the pointer moves from that
place, no item takes the selection from GPUI's hover reports and no item draws
the group hover, which stays registered with an empty style so that its flag
follows the pointer (`MenuItemElement::still`). The first move elsewhere
(`pointer_moved`, on the surface) gives the selection to the item GPUI last
reported under the pointer (`hovered_index`), where the last frame drew that
item's row under the pointer (`rows`, recorded as the items column is
prepainted: a column that does not scroll hands its rows' bounds to
`on_children_prepainted`, and a scrollable one's scroll handle records them
before its offset, so they are moved by the offset and clipped to the
column); a frame's synthetic move at the same place does not count. GPUI
reports nothing for a row it no longer draws, so a rebuild without the row
the pointer was on left that row's index behind, and a later rebuild could
put another item there, far from the pointer: with the pointer on Five, a
menu built again without Five and then with a separator above Two, which put
Four at Five's index, gave Four the selection on the pointer's first move
onto the menu's padding, where no row lies, a keyboard's cursor among it, and
Enter ran Four. A rebuild also forgets a hovered index past its drawn rows
and the rows laid out for the old items. A move that finds no reported row
drawn under it, as one in the frame after a rebuild, before the layout has
recorded the new rows, or one between a recorded row and the hit area GPUI
snapped for it, leaves the pointer still at its new place, so the next move
decides; it once cleared the pointer's stillness, and since GPUI reports no
hover for a row the pointer stays on and every later move then returned at
once, the row under the pointer stayed unselected, Enter running nothing,
until the pointer crossed into another row. A wheel that moves nothing, over a menu whose rows
fit or at the end of its rows, changes nothing.

A menu laid out at another place or size moves its rows under a still pointer
too: a row's menu follows its row when rows arrive above it, a menu opened
above its button moves as its height changes, and a menu built again in place
grows wider or taller, as History's commit menu gains Amend last commit… and
Undo last local commit… when its commit becomes HEAD. GPUI's paint compares each hover
listener's state with the frame's hit test and defers a hover report where
they differ, so once the frame was drawn the row that arrived under the
pointer took the selection: on a narrow Working row at 18 pt in a 461 px
window, the keys on Discard changes… and the pointer resting on it, a file
that sorts above notes.txt changed outside the app, the menu moved down with
its row, Stage arrived under the pointer, and Enter staged the file. The
surface's prepaint (`surface_laid_out`, beside the bounds it records) now
compares the bounds the surface is laid out at with the last frame's
(`surface`); where they differ with the pointer over the surface then or
now, it does what a wheel does, before that frame's hover reports run: it
records the pointer's place, so no row takes the selection until the pointer
moves, and a keyboard's selection stays. A pointer's selection stays while
its row stays under the pointer, as after a move sideways or by less than a
row, where GPUI reports no change of hover: the person put it there and it
is still there. That row's report as it leaves the still pointer gives the
selection up (the hover listener), and a pointer's selection on a row the
pointer had already left, as a submenu's row keeps one, is given up at once.
The prepaint then asks for the next frame from outside the draw, as the fold
measure does: given up there with a notify in prepaint, the pointer's
selection stayed drawn lit while Enter did nothing, and a hover the still
pointer no longer draws stayed drawn, until something else drew the menu.
The first layout moves nothing. Compared by its origin
alone, a menu that grew at its place under a pointer resting just beside or
below it let the row that arrived take the selection, a keyboard's cursor
among it, and Enter ran that row.

The hover itself never moves a keyboard's cursor under a pointer that has not
moved. The prepaint also records where the pointer was (`pointer_at_layout`);
a hover report that finds the pointer still there while the keyboard owns
the cursor came from rows that moved, as rows scrolled under the pointer
inside a surface that stays put, so the pointer is still from there: the row
only records its hover, and takes the selection once the pointer moves.

A menu built again under a resting pointer can move its rows under it while
the surface keeps its place and size, which the prepaint's comparison misses:
a capped menu scrolled to the end of its rows and built again with fewer keeps
its height, and GPUI's layout clamps the items column's offset to the shorter
content, so every row moves down. GPUI then reported the row that arrived
under the pointer hovered, and it took the selection: a capped menu of 128 ×
112.5 before and after, the pointer on Item 10 and Item 11 gone, selected
Item 9 and Enter ran it, as the branch switcher, capped and built again as a
write starts or ends, would check out the branch that arrived under the
pointer. `rebuild` now records the pointer's place, as a wheel does, where it
lies over the surface as last laid out and the keyboard does not own the
cursor, so no row takes the selection or draws the hover until the pointer
moves, though the rows may not have moved: nothing tells a row that arrived
from one that stayed, and the rebuild has cleared the pointer's selection, so
a row lit there would be one Enter does not run. A keyboard's cursor is left
to the hover's keyboard rule above, so the pointer leaving the cursor's row
still takes it.

The consumers are, for the wheel, every capped menu the app opens through
`native_accessibility::open_menu`, as the capped-menu patch lists; for a menu
laid out at another place or size, each row's menu, the keyboard's
(`KeyboardMenu`) and the right-click's (`PointerMenu`), drawn beside its row
as rows arrive above it, and every open menu built again in place as the
state its items read changes (`MenuInputs`: a row's menu, and a Button's
`AppMenu` that follows writes), as History's commit menu grows Amend last
commit… and Undo last local commit… when its commit becomes HEAD; and the
app's own rule for rows that move under a still pointer
(`motion::StillPointer`, `DESIGN.md`).
`cargo test --locked -p gitturtle a_wheel_over_a_capped_menu_lights_no_row_under_the_still_pointer`
rests the pointer on the Working Actions menu's last whole row at 13 and 18
pt in 461 × 490, scrolls one notch and reads no row with a fill of its own,
then none after a move to the same place, and only the row under the pointer
after a 1 px move.
`cargo test --locked -p gitturtle a_menu_that_moves_under_the_resting_pointer_selects_nothing`
opens that narrow Working row's keyboard menu, moves its cursor to Discard
changes… and rests the pointer there, then has a file above notes.txt change
outside the app: the menu moves down a row with its row, Stage under the
pointer, and no item draws a fill, Enter starts no write and nothing is
staged, until a 1 px move lights Stage. The crate's own
`menu::popup_menu::tests::a_menu_that_moves_under_the_still_pointer_selects_nothing`,
run out of tree like the rebuild tests below, draws a menu a row lower under
the pointer on Three and reads nothing selected, Two selected after the
pointer moves, and a keyboard's cursor on One kept as the menu moves back up
a row. `a_menu_that_grows_wider_under_the_still_pointer_keeps_the_cursor`
builds a menu again wider under a pointer resting 12 px beside it, the keys'
cursor on Three, and reads the cursor kept and Enter running Three, with Two
under the pointer;
`a_menu_that_grows_taller_under_the_still_pointer_selects_nothing` builds it
again with Five under a pointer resting 6 px below it and reads nothing
selected and nothing run until a 1 px move selects Five; and
`rows_scrolled_under_the_still_pointer_keep_the_keyboards_cursor` scrolls a
capped menu a row under the pointer, the cursor on Item 0, and reads the
cursor kept until a 1 px move selects Item 2.
`a_menu_moved_under_the_pointer_keeps_the_item_still_under_it` moves a menu
8 px sideways and then a third of a row down under the pointer on Three, with
no draw of its own, and reads Three selected and drawn lit and Enter running
Three; then, the keys' cursor on One and the pointer's hover drawn on Three, a
move draws only the cursor.
`a_capped_menu_draws_its_measured_fold_without_another_draw` caps a menu
whose tall element row the estimate misses and reads the drawn column as tall
as the measured fold.
`a_capped_menu_built_again_shorter_under_the_still_pointer_selects_nothing`
scrolls a capped menu to its end, rests the pointer on Item 10 and builds the
menu again without Item 11: the surface keeps its bounds and Item 9 comes
under the pointer, and nothing is selected or drawn lit and Enter runs nothing
until a 1 px move selects Item 9.
`a_move_onto_no_row_selects_no_row_the_pointer_left_by_a_rebuild` rests the
pointer on Five, builds the menu again without Five and then with a separator
that puts Four at Five's index away from the pointer, and moves the pointer
onto the menu's top padding: nothing is selected and Enter runs nothing.
`a_move_onto_padding_selects_no_reported_row_away_from_it` leaves GPUI's last
report on Four, an index in range but a row away from the still pointer, and
moves the pointer onto the padding: nothing is selected and Enter runs
nothing. `a_move_before_rebuilt_rows_are_laid_out_lets_the_next_move_decide`
rests the pointer on Two, builds the menu again and moves the pointer within
Two before a frame lays the rows out, then moves it again: the first move
selects nothing and the second selects Two, which Enter runs.
Without `surface_laid_out` the app test reads Stage lit
and the crate's reads Two selected; without the leaving row's report giving
the selection up, the crate's reads Three still selected; compared by the
origin alone, the taller menu's Five takes the selection; without the hover's
keyboard rule, Item 2 takes the cursor; without both, the wider menu's Two
takes it and Enter runs Two; giving the pointer's selection up in the
prepaint fails the sideways move, and a prepaint notify fails the moved
menu's last step and, for the fold, the capped menu; without the rebuild's
still pointer, Item 9 takes the selection from the menu built again shorter;
without both the row check and the rebuild's forgetting of a hovered index
past its rows, Four takes the selection on the move onto the padding, which
either alone prevents, and without the row check alone Four takes it in
`a_move_onto_padding_selects_no_reported_row_away_from_it`; with the
pointer's stillness cleared by a move that finds no row, the second move
within Two selects nothing. Remove this part of the patch when upstream
lights at most the row under the pointer after a wheel, a move of the menu or
a rebuild and those tests pass without it.

The menu-rebuild patch modifies `src/menu/popup_menu.rs`. Upstream's
`PopupMenu::rebuild`, which builds an open menu's items again in place,
cleared its selection, and a caller could put a cursor back only on the first
item that can act (`select_first_item`). GitTurtle builds an open menu again
whenever what its items read changes: a write starts or ends, an integration
pauses, a waiting Working row menu's reason changes, its row's file changes or
its rows land. A keyboard's cursor the keys had moved down a menu therefore
went back to the top, and a right-click's menu lost it, so Enter just after a
write ended could choose another item than the one the keys had reached.
`rebuild` now keeps the keyboard's cursor (`KeptCursor`) on the item it was
on, found again by its name (`PopupMenuItem::name`: an item's or a submenu's
label, an element item's accessible name) and its order among the items of
that name, wherever the builder now puts it; an item without a name is found
only at its own place, where the entry there has none either. Taken nearest
its old place, a repeated name let items added above move the cursor to the
other item of that name. It never moves the cursor to another
item. Where that item is unavailable or waits for a reason it states
(`PopupMenuItem::reason_stated`), as while a write refuses it and its name
says what it waits for, the cursor stays on it, as the menu keys rest it there
(`place_kept_cursor` places it on any `is_keyboard_stop`). Where that item is
gone, or can no longer act and states no reason, the cursor rests on
nothing and the menu remembers the item (`kept_cursor`), at the place it now
stands where it is still offered, across any further rebuild, until a
rebuild offers it able to act or stating why it is unavailable or waits and the cursor returns
to it, or a menu key or the pointer moves the selection, which forgets it. From rest, Up moves to the
last item the keys stop on before the remembered place and Down to the first
at or after it (`cursor_place`): the stops are the items that can act and
those that state why they are unavailable or wait (`is_keyboard_stop`). A remembered item takes the place of any selection the builder
makes (`select_first_item`); with none, the builder's selection stands. A
capped menu scrolls to the cursor only where the rebuild moves it to another
row, as its item moved or it returns from rest; the selection of a builder
whose choice a kept cursor replaces scrolls a handle nothing draws. A menu
the wheel had scrolled once scrolled back to the cursor at every rebuild. A
selection the pointer made is cleared, as upstream. Once the pointer has
taken the selection, or taken the keyboard's cursor off its row, since a menu
key last placed it (`pointer_selection`, which a menu key or a dismissal
clears), a rebuild that remembers no item selects nothing, though its builder
selects one, as a menu opened from the keyboard is built on its first item,
and that selection scrolls nothing; the builder's first item once took the
cursor of a keyboard-opened menu the pointer had moved over, so a narrow
Working row's menu, the keys on Discard changes… and the pointer over it, put
the cursor on Stage as a status change outside the app built it again, and
Enter staged the file. The menu keys are otherwise unchanged. An earlier
form of this patch moved the cursor to the
nearest item that could act and remembered that one, which left it on an
action the person had not chosen: a project's Rename project… while a write
refused Open project, and after the write; Discard changes… once Ignore… was
refused; Stage once a file became conflicted, where Enter marked the file
resolved; Cherry-pick once HEAD left a commit and took Amend and Undo away.

A rebuild that rests the keyboard's cursor because the item it was on,
which could act, is still offered but refused, on nothing or on the item
itself where it states why it is unavailable or waits, now reports that item to the
application, once, as the rest begins: `PopupMenu` emits `CursorRests`
(exported from `menu`), carrying the item's name and its accessible name,
which an item that states why it is unavailable or waits says apart from the command. The app
announces it because nothing establishes that a screen reader speaks the
selected item's name changing as it starts to wait (a native Orca read is
pending), so the reason is said either way. A
rebuild while the rest goes on emits nothing more, nor does one that finds
the cursor on a waiting item the menu keys put it on, which they reported
selected with its name stating the reason; an item that is gone emits nothing,
and a menu whose selection the pointer owns keeps no cursor to rest and
emits nothing. An element item's accessible name is also the name a rebuild
found it by, so an item whose accessible name states its condition ("Open
project, available when fetching remote finishes") read as a different
item from the plain one before it, and its refusal as the item having
gone; `PopupMenuItem::named` gives an element item the name it is found
by, its command, apart from its accessible name. The app subscribes in
`crates/app/src/native_accessibility.rs` (`announces_resting`, for every
button's, right-click's and keyboard's menu) and announces the accessible
name politely where it says more than the name. The kit has no
announcement of its own, which belongs to the application's live regions.
`a_rebuilt_menu_keeps_the_cursor_on_an_item_that_states_why_it_waits`
keeps the cursor on Two, where Down rested it, through a rebuild that reports
nothing, then on One as One starts to wait with a stated reason, drawn there
and reported once over two rebuilds, Enter running nothing until One can act
again; with `place_kept_cursor` placing only an item that can act, the cursor
leaves Two and the test fails.
`a_rebuild_that_rests_the_cursor_reports_the_waiting_item_once` rests the
cursor on a refused One, builds the menu again in that rest, returns the
cursor, rests it on an element item named One that states why, and then
rebuilds without One and with the pointer's selection: one report for each
rest, the second carrying the element item's accessible name, and none for
the gone item or the pointer's menu; without the report, or reporting on
every rebuild, it fails. Remove this part when upstream reports a resting
cursor or keeps the cursor on its item by a name of its own.

A click acts only on the item its press went down on. Rows are wired by
their item's index (`MenuItemElement::new(ix, …)`, the element ID under
which GPUI keeps a pending press across frames), and `on_click` ran
whatever item stood at that index when the button came up: a press on
Three, a rebuild without One that put Four at Three's index and place, and
the release ran Four, which the person never pressed. A row's press now
records its index (`pressed`, through the item element's `on_press` in
`src/menu/menu_item.rs`, called from the mouse-down listener an item that
acts already has) only where the row was drawn from the menu as last built
(`builds`, which each rebuild counts; `pressed_on`): GPUI delivers a press
to the rows of the last frame drawn, which can come between a rebuild and
the next draw, and such a press once recorded the old index. Every rebuild
forgets a recorded press, and `on_click` acts only where its press is
recorded for that index, so a press and release that a rebuild comes
between run nothing, and the person clicks again. Enter and the menu keys
are unchanged. A label is not an item's identity: an earlier form kept a
press across a rebuild where an item of the same label stood at its index,
and a press on the Changes Actions menu's Undo last local commit…, a commit
outside the app and its quiet refresh coming before the release, then
reviewed the Undo of the commit HEAD had moved to, and with two project
groups named Archive the press could run the other one. A dropped click
costs one more click; running another target than the one pressed costs a
review of a commit the person never chose.
`a_press_and_release_across_a_rebuild_run_nothing` presses Three, builds
the menu again without One and releases, and
`a_press_on_rows_drawn_before_a_rebuild_runs_nothing` delivers the press
after that rebuild but before the next draw: nothing runs either way, and a
fresh click then runs Four, the row drawn under the pointer; without the
check the first release runs Four, and without the build count the second
does. `a_press_across_a_rebuild_runs_nothing_whatever_its_label` presses
Two built for one target and releases after a rebuild for another with the
same labels, presses Two's row drawn before such a rebuild, and presses
the first of two items named Archive before a rebuild puts the second at
its index: nothing runs each time, and a fresh click runs the item drawn
under the pointer; with the press kept by its label, the first runs Two for
the new target. The app's
`motion::tests::waiting_menus::a_press_on_undo_across_a_head_move_runs_nothing`
presses Undo, commits outside the app and refreshes before the release: no
review opens. Remove this part when upstream ties a click to the item it
was pressed on.

A key confirms a menu only as a fresh press. GPUI hands an action no key
event, so upstream's Enter binding (`Confirm`) ran on every key-down the
platform repeats while the key is held, each marked `is_held` (Wayland's
repeat timer, macOS's `isARepeat`). Enter held on an item that waited ran
it on the first repeat after a rebuild made it able to act, as old as the
rest above (found in the QA89F-A security re-review). Enter or Space held on
a dropdown's button acted on the first item of the menu it opened: the
popover (`gpui-base` `popover.rs`) opens on the key-down, the menu opened
from the keyboard starts on that item, and the popover's own Space binding
confirms the menu inside it. The keys that confirm a menu
(`CONFIRM_KEYS` in `src/menu/popup_menu.rs`: Enter in every menu, Space in
one inside a `Popover`, the secondary Enter, Command- or Control-Enter, in
one inside a `List`) are now bound to GPUI's `NoAction` in the contexts
where they confirm, which keeps any binding from taking their key-down, so
it reaches the menu's own key listener (`PopupMenu::confirm_key`) with its
repeat flag. A fresh press dispatches the `Confirm` its binding did, on the
focused menu, so `confirm` and whatever an application captures of it (the
app's refusal of a paused integration's commit items) run as before; a
repeat dispatches nothing; either way the key goes no further, as the
binding took it. The listener matches each key-down against the keys and
contexts parsed once (`CONFIRM_KEYSTROKES`), as `KeyBinding::new` parses
them, and reads the focus path's contexts only for a key that matches;
the first cut parsed the three bindings again on every key-down in a menu
(simplified after the review, 2026-11-03, with no change in behaviour).
Space outside a popover and the secondary Enter outside a list reach their
bindings as before. A dropdown's button opens its menu on a fresh press
only too, as decided after the review (2026-11-03): the popover's own
Enter and Space bindings toggled it on every repeat, so a key still held
after a fresh press had chosen an item reopened the menu once focus
returned to the button, a flicker in which no item ran; the base crate's
popover-trigger patch ([base patch](../gpui-base/GITTURTLE-PATCH.md))
gives the trigger the same `NoAction` and fresh-press listener and lists
every path that opens a popover. After this change an item runs only
from `confirm`, which runs the selected item that can act, and `confirm`
runs only for: a fresh Enter, a fresh Space in a popover's menu or a fresh
secondary Enter in a list's (the `Confirm` action on the focused menu); a
`Confirm` an application or a test dispatches itself; and a click whose
press and release land on the item as the menu was last built (above),
which an assistive technology's Click also produces, since GPUI answers it
with a press and release at the row's centre (`Window::handle_a11y_action`).
GPUI's X11 backend marks no key-down held (`gpui-pre-linux`
`linux/x11/client.rs`), so under X11 a repeat still reads as a fresh press;
that is not a regression, as every repeat acted before. Nor did the rule
hold on Wayland with an input method, such as fcitx5, enabled through
text-input, which `gpui-pre-linux` 0.3.4 enabled whenever the focused
element had no input handler, as on a default Omarchy: a held Enter or
Space reached the app as a release and a press at the seat's repeat rate
(every 25 ms after about 243 ms on the probe's host), each a fresh press
GPUI cannot tell from a new one (native probe QA-90, 2026-11-03, fcitx5
5.1.22 on Hyprland, `WAYLAND_DEBUG=1`; that the input method forwards its
own repeat is inferred). The
[Linux backend patch](../gpui-pre-linux/GITTURTLE-PATCH.md), a backport of
upstream zed-industries/zed#63776, enables text input only while a focused
text field accepts it, so a focused menu or button gets GPUI's own
repeats, marked held, and from the first frame drawn after focus or
activation the rule holds on Wayland as on macOS, with or without an input
method (the backend's note records the one-frame transient after
activation).
`a_held_enter_runs_no_item_a_rebuild_makes_able_to_act` rests the cursor on
the waiting Two, presses Enter and repeats it, rebuilds the menu with Two
able to act and repeats again: nothing runs and the menu stays open, and a
fresh press runs Two. `a_held_enter_that_opened_the_menu_runs_nothing`
repeats Enter into a menu opened on its first item, and
`a_held_space_or_secondary_enter_runs_nothing` repeats Space in a menu
inside a popover's context and the secondary Enter in one inside a list's,
with those contexts' bindings: nothing runs until a fresh press, and in a
menu inside neither, neither key runs anything. Each fails with the repeat
check removed. The app's
`native_accessibility::menu_tests::a_held_key_that_opened_a_buttons_menu_runs_nothing`
holds Enter and then Space on a real dropdown's button, and
`a_held_key_that_chose_from_a_buttons_menu_reopens_nothing` holds the key
that chose an item on the button the menu returned focus to, which fails
with the trigger's repeat check removed. Remove this part
when GPUI hands an action its key event's repeat flag or upstream confirms
a menu only on a fresh press.

The consumers are every open menu the app builds again, in
`crates/app/src/native_accessibility.rs`: a keyboard's row menu
(`KeyboardMenu::rebuild`), a right-click's menu (`PointerMenu`, as writes
start or end, its row's inputs change or its rows land) and a Button's
dropdown that follows writes (`AppDropdown::follows_writes`), among them the
Working row menus (`workspace::working_row_menu`), the project rows' menus
(`project_pane::row_menu`), the branch picker and the profiles menu. The
crate's own `menu::popup_menu::tests::a_rebuilt_menu_keeps_the_keyboard_cursor_on_its_item`,
`a_rebuilt_menu_rests_the_cursor_until_its_item_can_act`,
`the_menu_keys_start_from_a_resting_cursors_item`,
`a_key_or_the_pointer_ends_a_resting_cursors_remembering`,
`a_rebuilt_menu_finds_a_repeated_name_by_its_order`,
`a_rebuild_scrolls_a_capped_menu_only_as_the_cursor_changes_row`,
`a_rebuild_after_the_pointer_leaves_the_cursors_row_keeps_no_cursor` and
`a_rebuilt_menu_clears_the_pointers_selection`, run out of tree with the
workspace's patch table since this crate is an excluded workspace member,
build a menu again with the same items, an item added above the cursor's, its
item refused or gone while another can act and the builder selects its first
item, then offered again, and read the cursor's item each time, Up and Down
from rest, a repeated name, a capped menu's scroll offset, and the pointer
on a row and leaving the cursor's row, where a builder that selects its
first item then selects nothing; without the kept cursor the first reads no
cursor once the menu is built again, with the nearest item that can act the
second reads the cursor on Rename, and with the builder's selection standing
where the pointer owns it the last two read the first item selected.
`cargo test --locked -p gitturtle a_right_click_row_menu_keeps_its_cursor_across_a_write`
and `a_keyboard_row_menu_keeps_its_cursor_across_a_write` move a Working row
menu's cursor down to Delete untracked file…, start and end a Stage of another
file, and read the cursor on no item while the write runs and back on Delete
untracked file… once its rows land, then on no item once Ignore…, where the
keys had moved it, can no longer act, and back on Ignore… once it can.
`a_right_click_menus_cursor_rests_as_its_file_becomes_conflicted` leaves a
narrow row's right-click menu on Discard changes… as a merge outside the app
conflicts its file, reads no cursor, presses Enter and finds no write started
and the file still conflicted; `a_keyboard_menu_closes_as_its_file_becomes_conflicted`
does the same with a keyboard's menu in an active window, where the conflicted
file's preview focuses its resolution draft and the menu closes, so no
rebuild reaches it (a test window starts inactive, where focus leaving the
menu closed nothing);
`a_keyboard_menu_the_pointer_moved_onto_selects_nothing_once_built_again`
moves a narrow row's keyboard menu's cursor to Discard changes…, moves the
pointer onto it, changes the file's mode outside the app, and reads no
cursor once the menu is built again and no write after Enter, where the
builder's Stage once took the cursor and Enter staged the file;
`project_pane::tests::a_project_rows_keyboard_menu_follows_writes` reads a
project's cursor on Open project, on none while a write refuses it, and on
Open project again after. Remove this part of the patch when upstream's
rebuild keeps the keyboard's selection on its item, never on another, and
those tests pass without it.

The tooltip-hide patch modifies `src/root.rs`. Upstream's managed tooltip asks
the window's overlay to show when its trigger's hover begins and to hide when
it ends, and a press hides it; nothing else does. A trigger that stops being
drawn under a pointer that stays put ends no hover, so the overlay keeps its
request, shown or still waiting out its delay, and draws it at the trigger's
last bounds over whatever took its place until the pointer moves: after
gamma's Stage moved gamma's row away, "Stage gamma.txt" appeared over
epsilon's row a second after the press. `Root::hide_tooltip` is public and
hides the window's tooltip, shown or waiting, through the overlay's existing
`hide`; the overlay itself is unchanged.

The consumer is `GitTurtle::rows_moved_under_pointer`
(`crates/app/src/motion.rs`), which hides it when the rows a row's own Stage or
Unstage produced put another row under the still pointer (`DESIGN.md`).
`cargo test --locked -p gitturtle a_still_pointer_lights_nothing_the_rows_moved_under_it`
presses gamma's Stage at once, after 300 ms on it (its tooltip waiting) and
after 600 ms (shown), and finds no tooltip painted for 1.6 s once epsilon's
row is under the pointer, then epsilon's own after a 1 px move; without the
call it fails with a tooltip painted 400 ms on. Remove this part of the patch
when upstream drops a managed tooltip whose trigger is no longer drawn and that
test passes without the call.

The dialog-kind patch modifies `src/dialog/dialog.rs` and
`src/dialog/alert_dialog.rs`, with the base patch's `DialogKind`. Upstream
AlertDialog always hosts the base `AlertDialog`, so every modal opened through
`open_alert_dialog`, the no-close-button surface GitTurtle opens each of its
dialogs with, was an alert dialog: AT-SPI's `alert`, which Orca announces as an
alert, for the Reflog, a picker or a sheet as for a discard review. The
component Dialog's props now carry a kind, `DialogKind::Dialog` by default,
which `Dialog::kind` and `AlertDialog::kind` set and the base host takes as its
role when the dialog is laid out (the declarative trigger paths carry it in the
props they copy). Only a caller that passes `DialogKind::Alert` keeps the alert
role. Nothing else changes: an AlertDialog still never closes from a backdrop
press, keeps its footer and buttons, and every dialog stays modal with its
captured title. The component re-exports the kind as `dialog::DialogKind`.

The consumers are every app dialog. The confirmations that interrupt with a
warning pass `DialogKind::Alert`: the reviewed write's last step shared by
every Git write (`confirm_git_write` in `crates/app/src/integration.rs`), the
discard and forced worktree-removal reviews, Delete saved profile, the custom
theme's delete confirmation, Verify SSH host and the repository picker's
failure. Every other dialog is a plain dialog.
`cargo test --locked -p gitturtle every_repository_dialog_is_named_by_its_title`
opens fifteen of the repository window's dialogs and reads each as a modal
dialog named by its title, then a reviewed write as a modal alert dialog; the
discard review's test reads its own alert. Remove this part of the patch when
upstream lets an AlertDialog choose its role, and those tests pass without it.

The context-menu lifetime patch modifies `src/menu/context_menu.rs`. A context
menu element keeps, in state it shares with its callbacks, the menu it opened
and the subscription to that menu's dismiss event, and upstream's dismiss
callback held that shared state strongly. The app's subscriber set keeps the
callback until the subscription drops, and a GPUI subscription holds the set
strongly, so the set, the callback, the shared state and the subscription
formed a cycle that no drop broke: once its element was gone, a context menu's
last menu stayed alive with everything its builder captured, for the rest of
the app's life and past it. The callback now holds the shared state weakly and
closes the menu only while that state lives. Opening, dismissing and replacing
a menu are unchanged: a dismissal (Escape, an item chosen, a press outside)
closes the menu, which the element keeps, closed, until its next right-click
replaces it or the element goes, and a right-click that opens a menu in place
of another lets the other go.

No app menu opens through the kit's context menu. The app's right-click menus,
the History and Working Changes rows' among them, are built by
`AppContextMenu::app_context_menu` as a `PointerMenu`
(`crates/app/src/native_accessibility.rs`), which keeps its own shared state
and holds it weakly from its dismissal the same way (`PointerMenuState`);
`cargo test --locked -p gitturtle native_accessibility::menu_tests::a_context_menu_goes_with_its_row`
covers that path, not this one. This part reaches the kit's own
`ContextMenuExt` callers (`Tree`, `Table` and the sidebar menu), none of which
the app uses; in the app it reaches only the probe of
`kit_menus_stay_above_the_bottom_inset_an_application_sets`. The `Editor` and
`Textarea`, which the app uses, show their right-click menu through the
input's `NativeMenu` (by `Input::context_menu`), not through this file:
OS-native on macOS and Windows; on Linux a kit `PopupMenu` drawn by `Root`'s
`FallbackMenuOverlay`, which holds its menu only while it is open.
The crate's own `menu::context_menu::tests::a_context_menu_goes_with_its_owner`,
run out of tree with the workspace's patch table, opens an element's menu
with a right-click, closes it with Escape and takes the element away, then
opens it again and takes the element away while it is open, and reads each
menu released with the value its item held; a right-click beside an open menu
opens another and lets the first go. With the shared state held strongly
again, as upstream held it, the menu dismissed first outlived its element.
Remove this part of the patch when upstream's dismiss callback no longer
holds the shared state strongly, and that test passes without it; since no
app menu uses the kit's context menu, a kit update may instead drop it rather
than carry it forward.

The input-overlay lifetime patch modifies
`src/input/popovers/completion_menu.rs`,
`src/input/popovers/code_action_menu.rs` and
`src/input/popovers/hover_popover.rs`. A code editor's overlay host
(`src/input/overlay.rs`) builds a completion menu and a code-action menu as
soon as any of its overlays opens, its search panel included, and drops the
host when the last one closes. Each menu owns a list whose delegate held the
menu strongly, a cycle no drop broke: once the host went, both menus and
their lists, with the items they last showed, stayed alive until the app
exited. GPUI's leak check cannot see it, since it runs as the app drops and
releases every entity at once. Each delegate now holds its menu weakly, and a
confirm that finds the menu gone does nothing. The hover popover held its
editor state strongly too. The overlay registry keeps a host only while its
state lives, so a state its owner let go while its hover showed was kept
alive by its own host's popover, and the host with it, for good. The popover
now holds the state weakly, as the diagnostic popover already did, and draws
nothing once the state is gone. Showing, moving through, confirming and
dismissing the menus and the hover are unchanged. No other popover the host
builds holds its owner strongly (the search panel, the diagnostic popover and
both menus already held the editor state weakly), and the kit's one other
list delegate, the Select and ComboBox adapter in `src/searchable_list/`,
reaches its state only through weak handles in its callbacks.

The consumers are the kit code editors the app opens (`text::editor` in
`crates/app/src/text.rs`): a host is built whenever a kit search session
opens in one, and the app's code-editor Find closes that session as it opens
its own (`FindBar::new` in `crates/app/src/editor_find.rs`), which drops the
host, so each such open and close left both menus and their lists behind.
The app sets no completion, code-action or hover provider, so its menus
never open and no hover shows; the hover half reaches only kit callers that
set a hover provider.

`scripts/vendor-crate-test.sh vendor/gpui-component input::overlay` runs
three tests this part adds to `src/input/overlay.rs`:
`a_dropped_host_releases_its_open_menus` drops a host with both menus open,
`closing_the_last_overlay_releases_the_hosts_views` opens and closes an
editor's kit search, and `a_state_dropped_with_its_hover_shown_is_released`
lets go of a state whose hover shows and sweeps its host with another
editor's frame; after each, the search panel, both menus, their lists and
the hover popover no longer upgrade. The menus' `is_open` and `list_handle`,
which the tests read, are test-only. With the delegates holding their menus
strongly again all three fail, each naming both menus and both lists alive,
and with the hover holding its state strongly again only the third fails, on
the state. Remove this part of the patch when upstream's list delegates and
hover popover no longer hold their menus and state strongly, and those tests
pass without it.

The dropdown lifetime patch modifies `src/menu/dropdown_menu.rs`. A dropdown
keeps the menu it opened in its keyed element state (`DropdownMenuState`), and
upstream subscribed to that menu's dismiss event with a callback that held the
state and the dropdown's popover state strongly, then detached the
subscription. GPUI keeps a detached callback while the menu it watches lives,
and the state holds the menu, so the callback, the state and the menu kept one
another: a dropdown taken away with its menu open, as a view drawn again
without it takes it, kept its state, its popover state and its menu, with
everything the menu's items held, until something dismissed that menu, which
no input could do once the dropdown was gone. The callback now holds both
states weakly and passes over one that is gone, and the state keeps the
subscription beside the menu it watches, so both go together when a dismissal
clears the menu or the state goes. Opening and closing are unchanged, as are
the menu's look, place and input: Escape, a press outside the menu, an item
chosen and a second press on the trigger each still close the popover and
clear the menu, and the next press builds it again.

This part relies on gpui-base's popover-lifetime patch
([base patch](../gpui-base/GITTURTLE-PATCH.md)). The popover state is the
base `PopoverState`, whose own dismiss subscription held it strongly while it
was open, and with it the deferred-popup registration that, left behind,
refuses every input's right-click menu. With both patches, a dropdown taken
away open lets its state, its menu and its popover state go with its element,
and its registration with them. With this part alone the popover state stayed
open for good, because the dismissal that once closed it no longer comes:
upstream's strong callback outlived the dropdown and still reached the
popover when the orphaned menu was dismissed, while here the menu, and
whatever would dismiss it, goes at once.

The consumers are every app dropdown, Settings' profile dropdown
(`crates/app/src/profiles.rs`) among them: each is an `AppDropdown`
(`crates/app/src/native_accessibility.rs`), which opens through the kit's
`dropdown_menu_with_anchor`. Each builder holds what its items act on, and
`AppDropdown`'s builder also registers, detached, an observer of the window's
bounds that dismisses its menu once the viewport changes. With upstream's
code, a dropdown its view stopped drawing while its menu was open kept its
menu and what its items held until that next viewport change, as a resize or
a tiling compositor's re-tile makes, which dismissed the orphan through the
strong callback, closed its popover and dropped the popover's registration;
until then every input refused its right-click menu.

`scripts/vendor-crate-test.sh vendor/gpui-component menu::dropdown_menu` runs
`a_dropdown_taken_away_open_releases_its_menu`, which this part adds. It opens
a dropdown with a press on its trigger and closes it each of the four ways,
reading after each that the menu is no longer drawn, its popover holds no
deferred-popup context, and the menu is released with the value its item
held, then opens it again and takes the dropdown away, reading its state, its
popover state and its menu released and no deferred popup registered. With
upstream's dropdown code under it, every closing check still passes and the
release check fails, naming the dropdown's state, its popover state and its
menu alive; with the base popover patch removed instead, it names the popover
state alive. Remove this part of the patch when upstream's dismiss callback no
longer holds the dropdown's state strongly, and that test passes without it.

The menu-bar lifetime patch modifies `src/menu/app_menu_bar.rs`. The
application menu bar (`AppMenuBar`) holds its menus, and each `AppMenu` held
the bar strongly, a pair no drop broke: a bar taken away kept itself, every
menu and each popup menu a menu had opened, for the app's life. Each menu now
holds the bar weakly; once the bar is gone a menu reads as closed and its
input does nothing, since there is no bar left to open in. Opening a menu
with a press, moving between open menus with Left and Right or by moving the
pointer onto another, and closing with Escape are unchanged, as are the
menus' look and place. No app view builds an `AppMenuBar`, so this part
reaches only kit callers.

`scripts/vendor-crate-test.sh vendor/gpui-component menu::app_menu_bar` runs
`a_dropped_bar_releases_its_menus`, which this part adds. It opens File with a
press, moves to Edit with Right and back with Left, moves the pointer onto
Edit and closes it with Escape, then drops the bar and reads the bar, both
menus and both popup menus released. With upstream's code under it, every
move and Escape still pass and the last check fails, naming the bar, both
menus and File's popup menu alive; Escape had let Edit's go. Remove this part
of the patch when upstream's menus no longer hold their bar strongly, and that
test passes without it.

The test-layout part is test-only and changes no built code. The crate's lib
test target compiles only beside its upstream repository: `src/dock/mod.rs`
includes gpui-base's `crates/base/src/dock/mod.rs`, `src/text/compat.rs` the
story crate's `crates/story/examples/fixtures/test.md`, and
`src/theme/schema.rs` the repository's `themes/aurora.json`, none of which the
crates.io package carries. `upstream-files/` holds the latter two at their
repository paths, byte for byte from revision
`94a313a72a2513aee2780240cd322d552b2395f0` under the repository's Apache-2.0
license: `crates/story/examples/fixtures/test.md`, SHA-256
`7ef8be1ae1179787c294cdc9d09a8767820b5a30ec85d2adad5c20fe6b99ca22`, and
`themes/aurora.json`, SHA-256
`996a637a69f8b3fe7c6ca6a8f29ccd45776759b433bbb9e9dd59a7783f5cbadb`.
`scripts/vendor-crate-test.sh vendor/gpui-component` lays them out at those
paths, and `vendor/gpui-base`, the same revision's `crates/base`, at its own,
beside the copy it builds, so the dock test compares this module with the
gpui-base the application builds. In `src/input/input.rs`,
`editable_input_offers_accessibility_write_action` and
`input_emits_accessibility_id` kept the styled Input's rendered frame in a
`_frame` binding while they rendered its state; the frame, the
`impl IntoElement` that `RenderOnce::render` returns, holds the `window` and
`cx` borrows until it drops, so neither probe borrow-checked (E0499, E0500). Each
now drops the frame before it renders the state. The frame projects into the
state as it renders, so what the probes read is unchanged. The test module of
`src/switch.rs` no longer imports `gpui::StatefulInteractiveElement`, which
none of its tests use, so the lib tests build without an unused-import
warning; that change goes with the module's tests. In
`src/input/overlay.rs`, `unchanged_content_keeps_the_overlay_signature` and
`facade_materializes_all_base_overlay_sessions` synced the input's overlays in
`cx.update`, outside any frame. A sync returns each overlay as an element
holding its popover's or search panel's view, and outside a draw GPUI
allocates elements in a thread-wide arena that nothing clears, so those views
outlived the test's app. Once the `gpui-pre-macros` teardown patch let each
seed's app drop, GPUI's leak check failed both tests on every run, with three
`CompletionMenu` handles and eleven popover and search-panel handles. Each
now syncs inside a draw (`in_frame`, through `VisualTestContext::draw`), as
the input's render does, which allocates the elements in the app's arena and
clears it as the draw ends; what the tests assert is unchanged. Remove
`upstream-files/` when a kit update's tests include no file outside its
package, drop the probe change with the input patch or when upstream's
probes compile, and drop the overlay tests' draw when upstream's tests sync
inside a frame.

The placeholder-fit patch, paired with the [base patch](../gpui-base/GITTURTLE-PATCH.md)
of the same name, modifies `src/input/input.rs`: the styled `Input` reports
its field's accessible placeholder (`InputPresentation::accessible_placeholder`)
rather than the placeholder it draws, and falls back to it for a missing
`aria_label`. Where the field fits its placeholder to its width that is the
whole form at every width; otherwise the two are the same, as upstream.
GitTurtle's width-step regressions compare it with a window opened at each
width (`cargo test --locked -p gitturtle draws_one_frame_where_it_settles`).

The field-description patch, paired with the [base patch](../gpui-base/GITTURTLE-PATCH.md)'s
`InputAccessibility::description`, modifies `src/input/input.rs`,
`src/input/textarea.rs` and `src/input/editor.rs`: the styled `Input`,
`Textarea` and `Editor` take an accessible description (`aria_description`),
as the patched `Switch` does, and the editing element that owns the field's
focus writes it as its node's description, which AT-SPI reports as the
object's description and assistive technology reads after the field's name,
role and value, so a field's visible helper text can be read with it. A
field given none writes none, as upstream. `Editor` forwards it to the
`Input` it renders, which an application cannot build for an editor's state
(`Input::from_state` is private to the crate); the consumer is the Diff
view, whose text says what Space and Tab do while its per-line staging
controls show (`crates/app/src/diff_view.rs`, through
`editor_find::Editor::aria_description`). The application's
`native_accessibility::input_tests::a_field_carries_its_accessible_description`
reads it from a single-line and a multi-line field's node,
`diff_view::tests::keyboard::the_text_describes_space_and_tab_while_lines_can_be_selected`
from the Diff view's in the unstaged and the staged area, and the kit's
`input::editor::tests::the_description_reaches_the_editing_element`
(`scripts/vendor-crate-test.sh vendor/gpui-component input::editor`) from a
bare Editor's, with one set and with none. Remove it when upstream's Input,
Textarea and Editor take a description.

The modal-test patch modifies only the test module of
`src/text/window_selection.rs` and changes no built code. Its
`setup_modal` sets `App::set_reduce_motion` before opening the window, so
a dialog or sheet a test opens draws at rest in its first frame. GPUI
times an animation that is not synced from the wall clock
(`gpui-pre` `elements/animation.rs`, `AnimationState::start`), not the
test clock that `settle` advanced, so the dialog's 250 ms `slide-down`
(`src/dialog/dialog.rs`) went on in real time between the frame whose
bounds `drag_inside_dialog_still_selects_its_text` read and the frames
its drag hit; on a loaded host the dialog moved under the press and the
test failed, on the vendored tree at `40b6bd43` and with the next round's
changes, none of which lies on the test's path (2 and 4 of 20 runs under
full CPU load; with 100 ms slept between the bounds and the drag, 20 of 20,
and none with the patch); upstream's own tree was not run. Remove it
when upstream's tests open their modals at rest.

The popover-motion test is test-only and changes no built code.
`reduce_motion_shows_and_hides_a_click_opened_popover_at_rest` in
`src/popover.rs` sets `App::set_reduce_motion` and opens a Popover with a
click: it is drawn in that frame, asks for no frame after it, stays where it
was drawn a frame later, and is gone in the frame after a click outside it.
It then opens and closes a dropdown surface (`dropdown_popup`, the 150 ms
slide and fade of Select, Combobox and DatePicker) with clicks on its
trigger: drawn at rest in the frame it opens, asking for no frame, and gone
after the closing click; without reduce motion the same click asks for the
slide's next frame. The kit's Popover, under every dropdown menu the
application opens, has no motion of its own, and the dropdown's motion goes
through GPUI's animation element, which draws an animation's last frame at
once while `App::reduce_motion` is set (`gpui-pre` `elements/animation.rs`),
whichever input opened it, as a dialog's slide and fade do, so a click
needed no runtime change. Remove the test with the dropdown motion, or when
upstream tests its popups under reduced motion.

The colour-picker keyboard patch modifies `src/color_picker.rs`, `src/lib.rs`
(its bindings) and `locales/ui.yml` (the swatches' color names), and uses
gpui-base's `ColorSwatch::track_focus`. Upstream every swatch of the styled
ColorPicker's palette was its own tab stop, the featured row's keyed by color
and so sharing their palette twins' element state and focus handles: Tab
reached 83 of the 112 stops and Shift+Tab looped among them. Enter and Space on
a focused swatch took the popover's binding and closed the picker with nothing
chosen, a focused swatch drew nothing that marked it, and a swatch's name was
its value alone. Patched, a palette is one stop of the popover's tab order
(`SwatchGrid`, read only while the picker is open, so a picker opened again
starts over on its color), with a focus handle per swatch under positional ids
and only the swatch at its cursor a tab stop. The arrow keys, in the
`ColorPickerSwatches` context, move focus along a row or to the swatch above
or below it, the one at the same share of its row, stopping at the palette's
edges, and
preview each color as a hover does; the preview goes back as the picker
closes. Enter and Space, bound to `NoAction` in that context, reach GPUI's
keyboard click, so the focused swatch is chosen once as the key comes up,
however long it is held, only for a press that went down on it. The focused
swatch draws `Theme::button_focus_ring` in gaps a pixel wider than the ring's
footprint at every text size (`swatch_gap`), so a neighbour close to the
accent stays set off from it. Palette swatches are named by family,
shade and value ("Red 500, #EF4444"), the theme's featured colors by name and
value, and a caller's `featured_colors` by value, the value in the digits the
picker's field and tooltip show and a save writes (gpui-base's `color_hex`).
Only a color's first swatch is announced as checked (`SwatchGrid::checked`),
as a radio group has one checked member, though the same color can be drawn
in the featured row and among the shades. The popover's content is built
only while the picker is open, so a closed picker builds no swatches and no
focus handles; `ColorPickerState::sync_pending_value` moved to `render` with
it. `scripts/vendor-crate-test.sh vendor/gpui-component color_picker::` covers
the arrows' geometry, the names and the cursor, and `cargo test --locked -p
gitturtle theme_editor::` drives the theme editor's real picker
(`tab_and_shift_tab_cycle_inside_the_open_colour_picker`,
`the_colour_pickers_arrow_keys_walk_its_one_stop_of_swatches`,
`enter_and_space_choose_the_focused_swatch_once`,
`a_focused_swatch_draws_the_kit_ring_whole`). Remove it when upstream gives a
palette roving focus, names its swatches and lets Enter and Space choose one.

The colour-picker close patch also modifies `src/color_picker.rs`
(`ClosingWatch`). A swatch's choice, by its click or keyboard click, closes
the picker through `ColorPickerState`, and gpui-base's
`PopoverState` gave focus back to the trigger only as the next frame built
the popover (`sync_open` in `Popover::render`), where GPUI's `Window::refresh`
does nothing: that frame drew the trigger unfocused, no frame followed, and a
trigger that draws a ring for focus waited for an unrelated redraw (QA-110
check 2, after Enter or Space, three of three; Escape, which closes through
the popover's own Cancel outside a draw, drew it at once). Patched, the
watch's observer, which runs as the close's notification is flushed and
before that frame, moves focus still inside the closing popover to the
trigger, so the focus change asks for the frame and that frame draws focus on
the trigger. Enter in the hex field is no route through the watch: the
single-line Input passes its Enter on, so the popover's own Enter binding
dispatches `Confirm`, which closes it through `PopoverState::toggle_open`
outside a draw, as Escape's Cancel does, and gives focus back there; the
commit the Input's `PressEnter` then makes finds the picker closed and
focus on the trigger, and the watch only clears the preview. `cargo test
--locked -p gitturtle
theme_editor::tests::choosing_a_swatch_by_key_rings_the_trigger_in_the_next_frame`
checks the theme editor's trigger ring in the one frame the key-up draws,
after Enter, Space and Enter in the hex field. Remove it when upstream's
popover gives focus back outside the draw.

The colour-picker press patch also modifies `src/color_picker.rs`
(`ColorPicker::render`). The popover's press listener on its trigger stops
the press, so GPUI's focus on press never reached the picker's root, which
tracks the trigger's focus handle: a picker opened by the pointer recorded
whichever control held focus before as the focus to give back, and Escape
returned there, another row's trigger in the theme editor (QA-110, found
outside the checks, 1). Patched, a left press on the trigger focuses it in
the capture phase, before the popover opens and records it, so every close
gives focus back to the trigger the picker was opened from. `cargo test
--locked -p gitturtle
theme_editor::tests::a_picker_opened_by_the_pointer_gives_focus_back_to_its_trigger`
covers Escape and a pointer choice after a click opened the picker. Remove it
when upstream's popover focuses its trigger on a press.

The colour-picker trigger patch also modifies `src/color_picker.rs` (`init`,
`fresh_enter`, `ColorPicker::render`). The base picker binds Enter on its
trigger to `Confirm`, which toggles the picker, so every key-down of a held
Enter toggled it: Enter held after the hex field's commit, which closes the
picker with focus on its trigger, reopened it through GPUI's held repeats on
macOS and Wayland, and under X11, where a held key's repeats arrive as fresh
presses, a repeat whose release and press a poll batch split after Enter had
chosen a swatch did the same (FU41-B's security review; it chose and wrote
nothing). Patched, the trigger also carries a `ColorPickerTrigger` context
that binds Enter to `NoAction` after the base binding, outranking it at the
same depth, and its key-down listener dispatches the `Confirm` only for a
fresh press, as a dialog's Button presses: not marked held and begun on the
trigger (gpui-base's `enter_press_began_on`). Focus inside the popover is
unchanged, since the popover's own contexts bind the key there. `cargo test
--locked -p gitturtle
theme_editor::tests::a_held_enter_never_reopens_the_picker_it_closed` holds
Enter after the hex field's commit and after a swatch's choice. Remove it when
upstream's picker opens only on a fresh press.
