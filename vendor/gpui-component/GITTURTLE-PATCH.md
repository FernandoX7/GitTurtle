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
without the patch; without it the build only warns. The crate's lib test
target does not compile as packaged: test code includes three files from the
upstream repository that the crates.io package omits, and two input
accessibility probes in `src/input/input.rs` fail the borrow checker. Its
highlighter tests therefore ran in a scratch workspace with the root patches
and lockfile, `--features tree-sitter-languages`, placeholders for those files
and the input test module compiled out. Remove this part of the patch when
upstream drops the import.

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

The menu-keys patch modifies `src/menu/popup_menu.rs`. Upstream's first Down
selects a menu's first entry whatever it is, Down past the last item returns
to it, and Right selects a submenu's first entry, so in a menu that opens with
a label the first Down highlighted text that cannot act and Enter did nothing.
Up and Down now move only between items that can act, from the first of them,
and wrap to the first or last such item; a label, separator or disabled item is
passed over, and a menu with none that can act selects nothing. A submenu opened
from the keyboard selects its first item that can act.

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
`the_first_down_in_a_row_menu_reaches_its_first_action` opens the project
pane's row menu with the keyboard and renames the group with Down and Enter.
Remove this part of the patch when upstream menus move only between items that
can act and those tests pass without it.

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
