# GitTurtle patch to gpui-base 0.6.0

This directory contains the crates.io `gpui-base` 0.6.0 source, licensed Apache-2.0. The upstream copyright and license are preserved in `LICENSE-APACHE`; `.cargo_vcs_info.json` records the published source revision. The registry archive checksum is `2caaf00ebe0482774dd370a82a936edf4bf19a18a0d1a39353d20ecf61f70330`.

The modified upstream sources are `src/button.rs`, `src/focus_trap.rs`, `src/dialog.rs`, `src/alert_dialog.rs`, `src/lib.rs`, `src/input/base/state.rs`, `src/input/base/element.rs`, `src/input/base/blink_cursor.rs`, `src/input/editor/decorations.rs`, `src/input/editor/mod.rs`, `src/input/base/kind.rs`, `src/input/mod.rs`, `src/otp_input.rs`, `src/checkbox.rs`, `src/switch.rs`, `src/radio.rs`, `src/toggle.rs`, `src/link.rs`, `src/color_picker.rs`, `src/popover.rs`, `src/popup.rs`, `src/positioner.rs`, `src/resizable/mod.rs` and `src/resizable/panel.rs`; `src/dialog_label.rs` and `src/disabled_focus.rs` are added private helpers. The existing Button element is wrapped by a private element that delegates identity, layout, painting, role, and synthetic children. When `disabled` is true, the wrapper also sets the AccessKit disabled property in `write_a11y_info`, as it does for a Button that `accessibility_disabled` reports disabled alone: that Button keeps its enabled styling, focus and click handler, for an application that refuses the press in its handler before it dims the control (GitTurtle's `workspace::refusing`, through the [component patch](../gpui-component/GITTURTLE-PATCH.md)). When the resolved role is Tab, it also reports the existing selected state without adding pressed metadata; ordinary button presentation selection remains separate. Existing disabled pointer blocking, absence of click actions and keyboard focus behavior are unchanged. Delegation preserves caller-provided synthetic children instead of replacing that hook. The Button test module no longer imports `gpui::Element`, which none of its tests use, so the crate's lib tests build without an unused-import warning; this test-only change goes with the module's tests.

The existing rendered-node test now expects disabled metadata. GitTurtle also includes the equivalent rendered-node regression in its application test suite, where the matching GPUI Kit test-support feature is enabled. Run `cargo test --locked -p gitturtle native_accessibility` from the repository root. This verifies generated node role, label, disabled state, click availability and selected/unselected Tab metadata. Native VoiceOver behavior is a separate application validation gate.

The focus-trap wrapper now forwards its underlying role, accessibility metadata and synthetic subtree. Previously it dropped these methods, so focused dialog hosts had no accessible node. Dialog and AlertDialog roles also receive modal state; a generic focus-trap group does not. The app regression inspects wrapped dialog/alert/group nodes for preserved names, descriptions and correct modal state.

A modal also states its kind: the added public `DialogKind` (`Dialog`, the default, or `Alert`) is set with `Dialog::kind` and `AlertDialog::kind` and decides the host's role, `Role::Dialog` or `Role::AlertDialog`. Upstream fixed the role by type, so a modal hosted by `AlertDialog` could only be an alert. The base `AlertDialog` keeps the alert role until a caller sets another kind; the [component patch](../gpui-component/GITTURTLE-PATCH.md) gives its styled dialogs the plain kind by default, so only a confirmation that interrupts with a warning, or an error, is announced as an alert. Remove this part with the component's when upstream lets an alert dialog choose its role.

Modal focus traps capture their own visible plain-text DialogTitle during synchronous layout. A scoped, unwind-safe stack keeps nested titles separate; each new layout clears its previous captured title. String, SharedString, literal string, and Text titles supply a UTF-8-safe name limited to 16 KiB and a labelled-by relationship to a synthetic title label, whose text is its value: AccessKit's adapters name a label by its value and never by its label property, so the synthetic title upstream wrote as a label property alone reached AT-SPI without a name (GitTurtle's command palette title), and no label property repeats the value, which macOS would report as the title's `AXTitle` beside its `AXValue`. The reader unwraps at most 16 nested AnyElement layers: the pinned component Dialog and AlertDialog erase titles before passing them through `.child(title)`, which adds another erased wrapper. Explicit caller labels or labelled-by relationships remain authoritative. A styled title that names itself, a `Stateful<Div>` with a role and an accessible name (GitTurtle's Reflog, interactive rebase, GitHub, Tags, worktree and review titles), supplies that name, read as the adapters read it: a label's value, else its label property; GPUI already writes such a title as a node of its own, so no synthetic title label repeats it. Other rich titles without directly readable text use the truthful generic name “Dialog” or “Alert dialog.” In debug builds only, each modal trap also records the node it was laid out to write (role, modal state, name) and the synthetic title it adds, if any, and the doc-hidden `focused_modal_nodes` and `focused_modal_titles` return those of the modals holding focus: GPUI writes a window's tree only while an assistive technology is active, which a test never is, so the app's `native_accessibility::dialog_tests::every_repository_dialog_is_named_by_its_title` reads each real dialog's name there, and `a_plain_text_dialog_title_is_read_by_its_text` reads the command palette's synthetic title through AccessKit's filter and, on Linux, the AT-SPI adapter. Release builds record nothing. A rendered component-title regression exercises the same erased literal/String/SharedString path used by those modal surfaces.

Dialog also bridges the input module's distinct Escape action to dialog cancellation while keyboard dismissal is enabled. This bubble-phase bridge runs only after focused children can consume Escape to close Find or completion surfaces, preserving their first-Escape behavior. Application regressions render nested dialogs, verify current title names without leaking earlier titles, and exercise focused Input and read-only editor Escape dispatch, including disabled keyboard dismissal and Find-first handling.

The shared input engine now accepts a compact accessibility presentation from the matching vendored `gpui-component` 0.6.0 Input and puts its resolved role/name/identifier/placeholder/value and SetValue action on the existing editing-focus element. The frame keeps its separate focus-group handle. This corrects a focused element with no role, which GPUI otherwise maps to its accessibility root. The projection never stores text; values are built only for active accessibility clients and remain suppressed for masked/secret fields. Disabled and read-only node properties are applied through the supported synthetic-child builder, and those fields omit SetValue. The application regression covers the real styled component-to-engine projection for ordinary, renamed, disabled, read-only, password and multiline fields. See [the matching component patch](../gpui-component/GITTURTLE-PATCH.md).

The input viewport setter now updates the accepted, clamped scroll handle immediately when layout geometry exists, and carries that accepted offset into the next layout. Direct wheel input clears a superseded deferred request. This prevents linked split editors from reporting a stale intermediate paint offset as a new gesture and bouncing one another backwards during bursts. Cold editors retain the first-layout request. The consuming `scroll_tests` suite renders the actual Editor and covers burst setters, wheel precedence, bounds, visible rows, and retained selection/focus. Native before/after tracing also compares both panes in the same painted frame; `GITTURTLE_TRACE_SCROLL=1` enables observations without requesting extra frames.

The explicit `rescale_scroll_offset` operation preserves the logical viewport
when a font-size change will replace the layout geometry. It defers clamping
until the new bounds exist, avoiding the old document-end limit during font
growth, and updates the observable offset for linked panes. Vertical position
is retained as a fractional row against the previous measured line height and
resolved using the next layout’s actual rounded height; horizontal movement
uses the font-size ratio. Hidden editors retain that row across several size
changes before painting. Ordinary scroll setters and wheel precedence keep
their behavior and supersede a pending font-size request. The consuming
`font_growth_preserves_near_bottom_viewport_and_selection` regression exercises
real Editor layout across an enlarged font, then checks the reverse change.
`fractional_font_changes_preserve_measured_rows_and_pending_hidden_viewports`
checks high-row 18→23→18 line-height changes, hidden updates and wheel precedence.

`InputBaseState::set_selected_range` in `src/input/base/state.rs` also lets
a single-line field's next layout scroll its caret into view, runtime code
only. `set_value` keeps a single-line field's start in view for the next
frame, deliberately, though it leaves the caret at the end; the selection
set after it scrolled through `scroll_to`, which measures against the last
layout, still holding the text from before, and so kept that start; and the
element follows the caret only when the selection has changed since the
last frame, a change that frame used up. A long value restored with its
caret at the end, as a repository tab's return restores History's search
field (`crates/app/src/repository_tabs.rs`, `FieldSnapshot::restore`),
drew with its caret clipped past the field's end until the next key moved
it. A single-line field's `set_selected_range` now drops the deferred
offset and row and its record of the last frame's selection, so the
layout that holds the new text follows the caret in that frame; the
editor's `refresh_editor` (`crates/app/src/text.rs`), which sets a scroll
offset after the selection, still has that offset win, and a multi-line
field keeps `scroll_to`'s offset, since its layout follows the caret by a
line a frame. The kit test is
`input::state::tests::a_caret_set_after_a_long_value_is_drawn_inside_the_field`
(`scripts/vendor-crate-test.sh vendor/gpui-base a_caret_set_after`), a
focused field holding a short value given a long one and a caret at its
end, its caret inside the field in the first frame (past it at 4,886 px in a 1,536 px field
without the patch). Remove this part when upstream follows a programmatic
selection in the frame that lays out the text it was set in.

The disabled-focus patch modifies `src/checkbox.rs`, `src/switch.rs`,
`src/radio.rs`, `src/toggle.rs`, `src/link.rs` and `src/color_picker.rs` (the
`ColorSwatch`), registers the added `src/disabled_focus.rs` in `src/lib.rs`,
and changes runtime code only. Upstream, these six controls track their focus
handle only while enabled (`.when(!disabled, |this| this.track_focus(…))`), so
one that turns disabled while it holds focus leaves the rendered frame with the
window's focus still on it. GPUI 0.3.4 then dispatches keys from the window's
root dispatch node, above the component `Root` context that binds Tab and
Shift+Tab, and focus stays on the dimmed control until a pointer click. Each
control now tracks its handle through `disabled_focus::track_control_focus`:
enabled, exactly as upstream; disabled and focused, as a target that is not a
tab stop, so `focus_next` and `focus_prev` step from its place to its
neighbours and Tab never lands on it; disabled and unfocused, not at all, as
upstream. The disabled control gains no click, change, hover or key
activation: it passes its click or change handler only while enabled, so GPUI
registers no Enter or Space keyboard click for it. It keeps the pointer
handling and the focus ring it already had: the Switch, Toggle and Link stop a
disabled mouse-down, and the Checkbox, Radio and swatch let it bubble. Keeping
the handle of a focused disabled control has the effects the component
Button's patch states for a disabled Button. A caller's `focus`,
`focus_visible` and `in_focus` styles apply to the control, as they do while
it is enabled. AccessKit reports it as the focused node instead of the window
root. Every ancestor key binding reaches it again, not only Tab and Shift+Tab.
A mouse-down on it focuses its own handle and calls `prevent_default`, so a
focusable ancestor no longer takes focus from it; this changes the Checkbox,
Radio and swatch, whose mouse-down still reaches the ancestor's other handlers,
while the Switch, Toggle and Link already stopped it before any ancestor.
`tab_stop(false)` also writes the handle's window-wide record, which the
enabled path rewrites on every render, so a control that is enabled again is a
tab stop again. This is the rule of the component Button's disabled-focus patch
([component patch](../gpui-component/GITTURTLE-PATCH.md)), applied here because
these controls own the guard; the base Button and the `ColorPicker` trigger,
which the app never disables, are unchanged, and gpui-component's Checkbox and
Switch wrappers, which pass their handles into these controls, need no change.

The app consumers are Settings' Follow system Switch while the Omarchy theme is
selected (`crates/app/src/settings.rs`), the diff view's partial-line Checkbox
while a stage runs (`crates/app/src/diff_view.rs`), the ignore dialog's
containing-directory Checkbox while its rule is prepared
(`crates/app/src/ignore.rs`) and the profile editor's signing Checkbox while it
saves (`crates/app/src/profiles.rs`); the always-disabled Subject column
Checkbox in Settings never holds focus and is unchanged. The regressions are
test-only app code; no vendored test changes.
`cargo test --locked -p gitturtle tab_and_shift_tab_leave_focused_kit_controls_that_turn_disabled`
renders a bare Radio, Toggle, Link, ColorSwatch, Checkbox and Switch between
two tab stops, focuses each with Tab, disables it, and sends Tab and Shift+Tab
as keystrokes: each leaves it, Tab skips it while it is disabled and unfocused,
and it is a tab stop again once enabled.
`cargo test --locked -p gitturtle tab_and_shift_tab_leave_follow_system_once_omarchy_disables_it`
and
`cargo test --locked -p gitturtle tab_and_shift_tab_leave_the_directory_checkbox_while_the_rule_is_prepared`
do the same for the Follow system Switch in the real Settings page and for the
ignore form's Checkbox. The Settings regression runs only on Linux, so all
three fail without this patch there and the other two elsewhere.
`cargo test --locked -p gitturtle enter_and_space_leave_focused_kit_controls_that_turn_disabled_inert`
renders the same six controls and a component Button between two tab stops,
focuses each with Tab, checks that Enter and Space, each sent as a key-down
and key-up, activate it while enabled, then disables it: neither key runs its
click or change handler or changes its checked, pressed or selected state, and
focus stays on it until Tab moves it on. Without this patch its Enter and
Space checks still hold, because the controls pass no handler while disabled,
and only its final Tab fails. Native keyboard behavior needs its own run.
Remove this part of the patch when upstream gpui-base keeps a focused disabled
control's handle in the frame, or GPUI moves Tab and Shift+Tab on from a
focused element that is no longer rendered, and those regressions pass without
it.

The input module also exports `LINE_NUMBER_RIGHT_MARGIN`, which upstream keeps
private to `src/input/base/element.rs`; `src/input/mod.rs` re-exports it and
its value and every use are unchanged. A code editor keeps this margin before
its text inside its padding whether or not it shows line numbers, and upstream
offers no way to learn it before the editor's first layout. The consumer is the
GitHub review's Changes mode (`crates/app/src/github_view/review.rs`), which
draws Compare's gutter itself and starts each row's text where decorated
Source's editor, with that gutter in place of its line numbers, draws its own:
the default Input padding and this margin. Changes is the mode shown first, so
it lays its rows out before Source's editor has laid out anything to measure.
`cargo test --locked -p gitturtle changes_and_source_start_their_text_at_one_place`
compares Changes' text with the position the editor reports for its first
character at 12 and 15 pt code. Remove this part when upstream exposes the
margin or the editor's text origin before layout.

All other upstream source, manifests and tests are unmodified. The registry-only `.cargo-ok`, `.cargo-checksum.json` and upstream package lockfile are omitted; the application uses the workspace lockfile. Remove this patch when the matching upstream toolkit publishes equivalent semantics and the application regressions pass against it. Native VoiceOver and macOS AX focus behavior remain separate runtime validation gates.

The switch focus-handle patch also modifies `src/switch.rs`. Upstream Switch
always keeps its focus handle in its own keyed state, so the `gpui-component`
wrapper, which renders under a different element-id scope, could not tell
whether the Switch it draws was focused. `Switch::track_focus` now takes a
caller-owned handle, exactly as upstream `Button::track_focus` does, and the
Switch uses it in place of its keyed state; without one nothing changes. The
consumer is the component Switch's focus ring
([component patch](../gpui-component/GITTURTLE-PATCH.md)), covered by
`cargo test --locked -p gitturtle a_focused_switch_draws_the_button_ring_whole_and_moves_nothing`.
Remove it when upstream Switch accepts a caller focus handle or exposes its
focus state.

The popup-inset patch modifies `src/popup.rs`, `src/positioner.rs` and
`src/lib.rs`. Upstream keeps every popup inside its window less a margin and
offers no way to keep one clear of anything else at the window's foot, such
as an application's status bar. `set_popup_bottom_inset` lets an application
give the room to keep clear there in each window, measured up from the
viewport's bottom edge, client inset included; `popup_bottom_inset` reads it,
nothing where it is unset. `Positioner::bottom_inset` resolves a popup in a
viewport shortened by the part of that room past its margin, so the side a
popup flips to and the clamp that moves it up both keep it, and the `Popup`
host passes the inset to every popover it places, dropdown menus among them.
The positioner still moves a popup by whole pixels, but where rounding to the
nearest one would carry a popup that ends within the bottom limit past it, it
rounds down instead, so a popup no longer ends half a pixel past that room. Unset, a popup keeps
only its margin, as upstream; tooltips and the component's Select, Combobox
and DatePicker dropdowns, which build their own positioners, take no inset.
The component's context menu takes the same inset
([component patch](../gpui-component/GITTURTLE-PATCH.md)), which also lists
the consumers: every app dropdown, through this host; no app menu opens
through the context menu.
`cargo test --locked -p gitturtle kit_menus_stay_above_the_bottom_inset_an_application_sets`
opens a kit dropdown and a kit right-click menu at the foot of a window under
an inset of 120.5 px and reads each above it; without the whole-pixel rule the
right-click menu ended half a pixel past it.
`menus_keep_their_edge_8_px_above_the_status_bar` reads the Working Actions
menu, a dropdown this host places, its edge 8 px
or more above the status bar. Remove this part of the patch when upstream lets
an application keep popups clear of a region at the window's foot, and those
tests pass without it.

The popup-margin patch also modifies `src/popup.rs`, `src/popover.rs` and
`src/positioner.rs`. Upstream's `Popup` host keeps a popup's box 8 px inside
the window and lets no caller choose otherwise, while the component kit draws
a popup's edge, its ring, in the band outside that box, so a dropdown moved in
from a window's side drew its edge 7 px from it. `Popup::margin` sets the
margin the host keeps instead of its 8 px, and `Popover::margin` passes one to
its host; neither changes a popup that sets none. The positioner also moves a
popup by whole pixels without crossing its side margins: where rounding its
offset to the nearest pixel would carry a popup that lies inside the margin
past it, it rounds toward the inside instead, as it already did at the foot,
so a popup moved in from a side up to half a pixel past its margin no longer
draws its ring nearer the side. The component's popover gives the host its
`popup_fill_margin`, 9 px, so its ring stands 8 px in
([component patch](../gpui-component/GITTURTLE-PATCH.md), the window-margin
patch, which lists the consumers and tests:
`menus_keep_their_drawn_edge_8_px_from_the_window_sides` and, on Linux,
`the_main_menu_keeps_its_drawn_edge_8_px_from_the_windows_side`, whose Main
Menu sets the margin on this host itself). Remove this part with the
component's window-margin patch.

The side-pixel patch also modifies `src/positioner.rs` and `src/lib.rs`.
The positioner moves a popup by whole pixels from where it was itself laid
out, and GPUI places that offset on the device pixel, rounding halves
toward zero. A host laid out beside a trigger part way into a pixel, as
the Linux Main Menu's button starts 6.5 px in at 13 pt, put a popup the
margin moved in from a side half a pixel further in at scale 2, so the
ring the component draws outside its fill stood 8.5 px from the side, 17
device pixels; at scale 1.5 a host at the window's side moved it 9 px,
13.5 device pixels, which GPUI placed at 13, the ring from device pixel
11, 7.3 px in. A popup on a side margin, moved in from that side or
placed at it, now lies on the device pixel at or inside the margin,
counted from the window's side (`device_pixel_at_or_after` and
`device_pixel_at_or_before`, public for the component's submenu and the
application's row menus); a popup inside both margins moves as before. Its
fill moves by at most a device pixel, the move that takes its ring to
8 px. `scripts/vendor-crate-test.sh vendor/gpui-base positioner::` runs
`a_popup_moved_in_from_a_side_lies_on_the_device_pixel_at_its_margin`,
which reads a popup moved in from either side of a window at scale 1, 1.5
and 2 from a positioner laid out part way into a pixel and at the
window's side; the old placement fails it at 1.5 and 2. The component's
`a_popover_moved_in_from_a_side_draws_its_edge_8_px_from_it` reads the
ring itself. The top and foot clamps do the same (2026-11-02): a popup
moved down to the window's top margin lies on the device pixel at or below
it, and one moved up to the margin above the window's foot, or above an
application's bottom inset, ends on the device pixel at or above it, its
drawn height a whole number of device pixels as GPUI lays it out, so its
ring stands exactly 8 px from the window's top or foot at every scale where
8 px is a whole number of device pixels (1, 1.25, 1.5 and 2); at 1.2, where
it is 9.6, the 9 px margin's 10.8 device pixels put the fill on device
pixel 11 and the ring GPUI paints over the device pixels its band touches
from device pixel 9, 7.5 px from the edge, as at the sides. Moved by whole
pixels, at 1.25 a popup clamped 9 px down, 11.25 device pixels, lay on
device pixel 11 and its ring 7.2 px from the top, and at 1.5 the 13.5
rounded to 13. `Positioner::point_room` measures the room to those device
pixels, so a popup capped there lies wholly on its side of the point and
inside them. A popup inside both margins moves as before.
`a_popup_clamped_to_the_top_or_foot_lies_on_the_device_pixel_at_its_margin`
reads a popup clamped to the top and to the foot of a window at scale 1,
1.25, 1.5 and 2 from a positioner laid out at the window's top, 6.5 px and
4/3 px down, and one inside both margins left where it was asked to be;
the whole-pixel clamps fail it at 1.25, 1.5 and 2.

A popup clamped at both the top and the foot (2026-11-03), too tall for the
room between the device pixels at or inside the two margins but no taller
than the room from the window's top (the client inset, where a window has
one) to the foot's, keeps the foot's and gives up only as much of the top
margin as it needs. It took the top margin's, as a popup clamped there
alone does, and so ran past the foot's device pixel: by one where the
window was a device pixel too short for it, as at 1.2, where the 9 px
margins are 10.8 device pixels each and a 300 px window's room between
their device pixels, 338, is 0.4 of one shorter than its 282 px. Decided
then: each margin rounds outward, to the device pixel at or inside it, as
the side, top and foot clamps and `point_room` already round it, not to the
nearest device pixel, because the margin is the least the design promises a
popup's edge, and a margin rounded to the nearest pixel puts the fill up to
half a device pixel across it (at 1.25 the 9 px margin's 11.25 would round
to 11); the cost is under a device pixel of room at each edge. Where the
popup cannot keep both, it keeps the foot's, which keeps it off a status
bar or an application's bottom inset, while the top margin keeps nothing
beneath it clear. A popup taller still, which crosses the foot's wherever
it lies, keeps the top margin's, as before (reviewed 2026-11-03: the first
cut moved it to the window's top, which gave up the top margin and still
crossed the foot's). `a_popup_clamped_at_both_ends_on_the_device_pixel_at_the_foot_margin`
reads, in a 300 px window at scale 1, 1.2, 1.25, 1.5 and 2 from a
positioner laid out at the window's top and 4/3 px down, a popup one
device pixel too tall, which ends on the foot's device pixel and starts one
above the top's, one exactly as tall as the window down to the foot's,
which starts at the window's top and ends on the foot's, one a device pixel
taller than that and one taller than the window, which both start on the
top's, and one that fits, on both; placed on the top margin, the first
ends a device pixel past the foot's at every scale, and moved to the
window's top, the third and fourth start there. An occluding positioner,
as every `Popup` host is, blocks the mouse over the bounds its popup is
drawn at, after this placement (from the 2026-11-03 security re-review):
it blocked at the bounds resolved before it, so a popup moved up to the
window's top left an invisible strip below its foot, as tall as the move
and ending at the window's foot or a status bar's top, that took presses
from what lay behind it, and its own top strip unblocked.
`an_occluding_popup_blocks_the_mouse_where_it_is_drawn` presses half a
device pixel inside the drawn top and foot of such a popup, below its foot
and beside it, at scale 1, 1.2 and 2, laid out at the window's top and 4/3
px down; blocking at the resolved bounds fails it at each. Remove this
part with the component's window-margin patch.

The popup-frame patch also modifies `src/popup.rs` and `src/positioner.rs`.
Upstream a `Popup` asks for the next frame (`request_animation_frame`) the
first time its trigger is prepainted, so content it holds back until the
trigger's bounds are known is drawn then. It asked whether or not it held
any: a `Popover` hands it content only while open, so a menu trigger first
drawn closed asked for a frame that drew nothing new. GitTurtle's text
Compare folds its review options into such a menu in a narrower window, and
Compare chooses that form in the frame it draws at a new width, so on
Wayland every such width step drew a second frame. A popup now asks for the
frame only while content waits for its trigger's bounds; that content still
appears in the next frame, and a popup opened later is placed from the
bounds its trigger recorded as it was drawn. Upstream an open popup is
placed at the corner its trigger had in the frame before, as the popup is
built: a width step that moved the trigger left the popup where the trigger
had been until some other change drew a frame, which Compare's own refresh
after a width step once did. `Positioner::corner_following` reads the corner
as the popup is prepainted instead, from the cell the trigger's prepaint
fills before any deferred content is prepainted, so an open popup follows
its trigger in the frame that moves it and asks for no frame to do so.
`cargo test --locked -p gitturtle a_compare_width_step_draws_one_frame_where_it_settles`
delivers only the frames a width step asks for, and fails without the frame
part at 13 pt as the window narrows from 1,480 to 1,000 px;
`an_open_kit_dropdown_follows_its_button_when_the_window_changes_width`
reads a kit dropdown left open beside its button in the frame each width
change draws, and fails without the corner part. The crate's own
`popup::tests` cover both parts in the toolkit:
`a_popup_asks_for_a_frame_only_for_content_it_holds_back` and
`an_open_popup_follows_its_trigger_in_the_frame_that_moves_it`. The crate is
an excluded workspace member whose dev-dependencies the workspace's lockfile
does not resolve, so
`scripts/vendor-crate-test.sh vendor/gpui-base popup::tests positioner::`
runs them, and the `positioner` tests, in a copy of the crate under the
checkout's `.local/`, made its own workspace root. The copy takes every other
entry of the root manifest's `[patch.crates-io]`, so the AT-SPI adapter, the
`#[gpui::test]` macro and the macOS backend come from `vendor/` as they do in
the application; it builds with the checkout's pinned toolchain, and every
run starts from the workspace's `Cargo.lock`. The script resolves beyond that
lock only the dev-dependency `indoc`, which cargo fetches from the registry
once, lists what it added, and runs `cargo test --locked --lib` in
`.local/target-vendor`. Remove these parts when upstream asks for that frame
only for content waiting on the trigger's bounds and places an open popup at
its trigger's corner in the frame that lays the trigger out, and the tests
pass without them.

The point-placement patch also modifies `src/positioner.rs`. Upstream places
a popup at a pointer only by a corner (`Positioner::corner`), which keeps
that corner at the point and only clamps the popup into the window, so a
menu opened near the window's foot, or above an application's bottom inset,
slid up over the point it was opened at. `Positioner::point` places it as a
side placement on the device pixel row the point lies in at the window's
scale (`Strategy::Point`, resolved as `Strategy::Side` with a bottom
preference and leading alignment), with a gap past that row that
`Positioner::offset` sets, its far edges rounded out to the device pixels
at or past them at the window's scale: its
top-left corner just below the point where its height fits below,
otherwise its bottom-left corner just above it where it fits above,
otherwise on the side with more room, moving in from the window's sides as a
corner does. A popup wholly on one side of the point lies on that device
pixel as it is prepainted, its drawn height a whole number of device
pixels as GPUI lays it out, rather than moved by whole pixels from where
the positioner was laid out (2026-11-02): with the edges rounded out to
whole logical pixels, below the pointer at 1.2, 1.25 and 1.5 its fill started
part way into a device pixel that GPUI rounds an element's place to,
halves toward zero, back towards the pointer, and its ring covered the
pointer's own device row, and at 2 it left a device row empty between
them; above it, moved up to a whole pixel so that a height ending in half a
pixel kept its bottom out of the gap, it ended a device row short of the
gap at 2. The
gap is the room a caller keeps for the ring a popup draws outside its fill:
at the point, the ring lay on the row clicked. The row's lower edge, not the
point, bounds the room below, so a pointer on a whole pixel, as X11 reports
one, no longer lies inside a popup that starts there (GPUI's `Bounds::contains`
counts the top edge in). At a fractional scale such as 1.25, where GPUI
paints a shadow's ring over every device pixel it touches (`cover_bounds`),
a gap counted from the pointer's logical pixel let the ring reach the device
row under a pointer late in its pixel; counted from the device row, it
cannot. A popup that fits on neither side is still
clamped over the point, so `Positioner::point_room` gives the room on the
side with more, past the gap, with the positioner's margin and bottom inset,
at which a caller caps a scrolling popup so that it never covers the point
however short its window. The
consumer is the component's `NativeMenu` fallback, the drawn menu a
right-click in an input, text area or editor shows on Linux
([component patch](../gpui-component/GITTURTLE-PATCH.md)), which sets the
gap to its ring's width. `point_device_row` gives the device row's own edges, where the
component's input menu ends its shadow; a point within a thousandth of a
device pixel of a row's top lies in that row, since a pointer on a whole
device pixel, `k / scale` in logical pixels, can come back from the
division a hair short of `k` times the scale, as at 1.1, 1.333 and 1.75,
and took the row above
(`positioner::tests::a_pointer_on_a_whole_device_pixel_lies_in_that_pixels_row`
reads rows 0 to 3,999 at seven scales). `point_row_clear_by` gives the two
edges that gap places a popup from, above and below the device row a point
lies in, so a popup the positioner does not place can start where
`Positioner::point` starts it: the app's right-click row menus
(`RowAnchor::pointer` in `crates/app/src/native_accessibility.rs`) start
below the pointer on the same device pixel, through `point_device_row` and
`device_pixel_at_or_after`, with the same gap for their ring. With their
fill at the pointer, their ring lay on the pixel row above the pointer's;
`cargo test --locked -p gitturtle a_rows_menu_below_the_pointer_draws_its_edge_below_the_pointers_row`
reads it on the row below the pointer's at scales 1 and 1.5, for a pointer
on a whole pixel and part way into one.
`positioner::tests::point_positioning_opens_below_or_wholly_above_the_point`
covers the side it takes,
`point_positioning_keeps_its_ring_off_the_pointers_device_row` the gap at
scales 1, 2, 1.25 and 1.5, `point_room_is_the_room_on_the_side_with_more`
the room, `a_popup_above_a_point_ends_where_its_gap_does` a popup
100.5 px tall prepainted above a point at scale 2,
`a_popup_at_a_point_lies_on_the_device_pixel_past_its_gap` a popup
prepainted below and above a pointer on and part way into each of twenty
device rows at scales 1, 1.2, 1.25, 1.5 and 2, its fill on the device pixel
past its gap and its ring off the pointer's row (the whole-pixel placement
fails it at 1.2, 1.25 and 1.5, its ring on the pointer's row, and at 2), and
`a_popup_at_a_point_keeps_its_ring_off_the_pointers_row_at_scale_1_25` a
popup prepainted below and above a pointer late in its pixel in a window of
scale 1.25, which the window's scale places, through
`scripts/vendor-crate-test.sh vendor/gpui-base positioner::`;
`cargo test --locked -p gitturtle an_inputs_menu_opens` covers it in the
app at scales 1 and 2, the drawn edge on the row below the Working Changes
filter's pointer and on the row above the commit title's, and
`an_editors_menu_too_tall_for_either_side_scrolls_beside_the_point` covers
the room, capping a code editor's menu beside the point in a 461 × 330
window on the side with more room, above a pointer low in the editor, and
taller than the room below. Remove this part
of the patch when upstream places a popup at a point on the side that holds
it, never over the point, and those tests pass without it.

The input's right-click menu (`on_mouse_up`, `on_blur`, the added
`drop_pending_context_menu` and `handle_right_click_menu` in
`src/input/base/state.rs`) opens, on every platform, only on a right-button
release over the input that ends a right-press begun on it, with no press
outside the input's bounds, other left or right release or reported blur of
the input between them, and only while the input holds focus; the caret goes to the press's
offset clipped to a character boundary of the text as it then stands.
Upstream a right-press kept its point and offset pending until a right
release over the same input took it, so a press that ended any other way
stayed pending: one dragged out and released elsewhere, a macOS
Control-click whose Control is let go before the button (a right press and
a left release, `vendor/gpui-pre-macos/src/window.rs`), one released while
the input was not drawn and one whose release never came. A later right
release over the input of a press begun elsewhere then opened its menu at
the old point. The menu's actions go to the window's focus (the
[component's](../gpui-component/GITTURTLE-PATCH.md) drawn fallback on
Linux), so with another field holding focus after a click in it, the
menu's Cut and Paste acted on that field; with the input holding focus, the
menu moved its caret to the old offset, which, once the text had changed,
could lie inside a character, and typing split it. Now a press beginning
outside the input's bounds (`on_mouse_down_out`), a left or right release
outside it
(`on_mouse_up_out`) or over it, and the input's blur, which GPUI reports as
it draws a frame where the input no longer holds focus because another
element took it, the input left the frame or the window went inactive, each
end a pending press. The release over the input opens the menu only while
its own focus handle is focused, as the press left it, since GPUI focuses a
focusable element under any mouse-down; that check also keeps the menu
closed when Tab or a shortcut moves focus before a frame reports the blur.
Decided by the owner's rule to keep focus where the person put it: a menu
opened over one field must not act on another, and a release whose press
began elsewhere, or ended, is no right-click on the field. The doc of
`handle_right_click_menu` said it is a native OS menu; it now says it is
the OS's own on macOS and Windows and drawn by the kit on Linux, which
release calls it, while the input holds the focus its press gave it, so the
menu's actions act on it, and that the offset is clipped.
`scripts/vendor-crate-test.sh vendor/gpui-base right_press a_right_release a_right_click`
runs the regressions in `input::state::tests`, each in two inputs, A and B,
with a right-click in A opening A's menu:
`a_right_release_opens_no_menu_for_a_press_begun_elsewhere`, a right-press
in A released over blank space and then one on blank space released over
A, with B focused by a click and again with A focused;
`a_right_release_opens_no_menu_once_focus_has_left_the_input`, focus moved
to B between a right-press in A and its release over A;
`a_release_of_either_button_ends_a_right_press`, a Control-click's left
release over A and outside it and a right release outside it, each followed
by a right release over A whose press neither input saw, and a Control-click
followed by a right-drag from blank space into A;
`a_press_outside_or_a_left_click_ends_a_right_press`, a right-press in A
whose release never comes followed by a right-drag from blank space into A,
or by a left-click on A and then a right release over A or that right-drag;
`an_input_that_loses_focus_ends_its_right_press`, focus moved to B and back
to A while the button is held;
`an_input_taken_out_of_the_frame_ends_its_right_press`, A taken out of the
drawn frame and back while it is held; and
`a_right_click_places_the_caret_on_a_character_boundary`, a press past the
end of "abcdef" released after the text became "aéééé", where the caret
goes before the third "é" and typing "x" gives "aééxéé". Each of the
press's other ends (a press outside, a left or right release outside, a
left release over the input, a blur), the focus check and the clip,
removed alone, fails at least one of them. In the app,
`cargo test --locked -p gitturtle an_inputs_menu_opens_only_for_a_right_click_pressed_on_it`
drags a right-press from the Working Changes filter onto the commit title
and one from the title into the filter, and neither opens a menu, focus
staying in the title. Remove this part when upstream ends a pending
right-press at any other press outside the input, any release and the
input's blur, opens its menu only while the input holds focus and clips its
offset to a character boundary, and those tests pass without it.

The popover-lifetime patch modifies `src/popover.rs`. An open
`PopoverState` holds the subscription to its own dismiss event, and
upstream's callback held the state strongly (`cx.entity()`). The app's
subscriber set keeps the callback until the subscription drops, and only
the state drops the subscription, so the state, its subscription and the
callback formed a cycle that no drop broke: a popover taken out of the tree
while open, as a page left by a shortcut with a dropdown's menu open or a
trigger scrolled out of a list, kept its state for the app's life, and
with it the deferred-popup registration an open state holds
(`DeferredPopover`), the token that exists so that a state dropped without
being closed takes its registration with it. The subscriber set and the
registry are the app's, so closing the popover's window cleared neither,
and the app opens several windows. With the registration left,
`GlobalState::is_in_deferred_context` stayed true, and every input in every
window then refused its right-click menu (`handle_right_click_menu` in
`src/input/base/state.rs`), the kit's drawn menu on Linux and the OS's own
on macOS, for the rest of the session and with nothing to say why. The
callback now dismisses the state the subscription hands it, which GPUI
upgrades from a weak handle as each event arrives, so the callback holds
the state only weakly. Opening and every dismissal are unchanged: Escape,
a press outside, the trigger pressed again and a dismiss event from the
content, as a chosen menu item sends, each close the popover and report
it, and a dismissal that finds focus inside the popover returns it to
where it was before the popover opened. Decided by the owner's rule to
say why something is unavailable: a popover that is gone must not leave
every field's menu unavailable with nothing on screen to explain it. The
consumers are the app's dropdowns (`AppDropdown` in
`crates/app/src/native_accessibility.rs`), built on the component's
`DropdownMenu` and `Popover`, Settings' profile dropdown among them; the
component's `DropdownMenu` keeps a dismiss subscription of its own on its
menu (`src/menu/dropdown_menu.rs`), and a dropdown taken away while open
goes with its element only where that subscription, too, holds neither
the popover state nor the menu strongly
([component patch](../gpui-component/GITTURTLE-PATCH.md)).
`scripts/vendor-crate-test.sh vendor/gpui-base popover::` runs
`popover::tests::a_popover_taken_away_while_open_lets_its_state_go`, which
opens a popover with a press on its trigger, drops the view that holds it
while it is open and draws a frame, reads its state no longer upgrading and
no deferred popup registered, and then right-clicks an input above it,
whose menu opens; with the callback holding the state strongly again, as
upstream, the state stays alive, its registration with it, and the input
opens no menu, though the view itself is released either way.
`every_dismissal_still_closes_the_popover` closes a popover with Escape,
with its trigger pressed again and with an item's dismiss event, and reads
focus back on the trigger after Escape and the item;
`unstyled_popover_owns_pointer_open_and_outside_dismiss` covers a press
outside. Remove this part of the patch when upstream's dismiss callback no
longer holds its state strongly, and those tests pass without it.

The popover-trigger patch (2026-11-03) also modifies `src/popover.rs`. A
popover's trigger opens or closes it with Enter or Space only as the key
goes down, never on a held key's repeats, as a menu confirms only on a
fresh press ([component patch](../gpui-component/GITTURTLE-PATCH.md),
decided by the coordinator after its review). GPUI hands an action no key
event, so upstream's Enter and Space bindings (`Confirm` in the `Popover`
context, which the trigger and the content both have) toggled the popover
on every repeat the platform sends: after a fresh press had chosen a
dropdown's item and the closed menu had returned focus to its button, the
key still held reopened the menu once, a flicker in which no item ran. The
trigger's element now has the key context `Popover PopoverTrigger`, and
Enter and Space are bound to GPUI's `NoAction` in `PopoverTrigger`, after
upstream's bindings, so at the trigger's depth they outrank its `Confirm`
and their key-down reaches the trigger's capture-phase listener
(`open_key`) with its repeat flag. A fresh press dispatches the `Confirm`
the binding did, on the focused element, so the trigger's own `Confirm`
handler toggles the popover as before and whatever an application captures
of it still runs; a repeat dispatches nothing; either way the key goes no
further, as the binding took it, and taken in the capture phase it reaches
nothing inside the trigger, as the binding kept it from too. In the content
the innermost popover context is the content's own, whose `Confirm`
outranks the trigger's `NoAction`, so a key there reaches the content's
binding, or a menu's own listener, as before; the listener passes over it.
Both keys are parsed once (`OPEN_KEYSTROKES`). After this change a popover
opens only by: a press of its mouse button (left by default) on the
trigger, which an assistive technology's Click also produces; a fresh Enter
or Space with focus in the trigger; a `Confirm` that reaches the trigger's
handler from inside the trigger otherwise, as an application or a test
dispatches it, or as a binding of an enclosing context that the trigger's
`NoAction` does not outrank produces it (the component's `List` binds the
secondary Enter, Command- or Control-Enter, to it; no app dropdown lies in
a `List`); and in code, by `Popover::open(true)`, `default_open(true)` or
`PopoverState::show`, `set_open` or `toggle_open`, which the app's
dropdowns do not call. GPUI's X11 backend marks no key-down held, so under
X11 a repeat still reads as a fresh press there too; that is not a
regression, as every repeat toggled the popover before. On Wayland an input
method, such as fcitx5 on a default Omarchy, did the same while
`gpui-pre-linux` 0.3.4 turned text input on for a focused element with no
input handler: the held key arrived as a release and a press at the seat's
repeat rate (every 25 ms on the probe's host), each a fresh press (native
probe QA-90, 2026-11-03; details in the component patch). The
[Linux backend patch](../gpui-pre-linux/GITTURTLE-PATCH.md) turns text
input on only while a focused text field accepts it, so a focused trigger
gets GPUI's own repeats, marked held, and from the first frame drawn after
focus or activation the rule holds on Wayland as on macOS, with or without
an input method (the backend's note records the one-frame transient after
activation).
`popover::tests::a_held_key_on_the_trigger_opens_nothing` holds Enter and
then Space on a trigger, which opens nothing until a fresh press opens the
popover, closes it with Escape, which returns focus to the trigger, and
holds the key again, which reopens nothing until a fresh press, and Enter in
the content still closes it; the app's
`native_accessibility::menu_tests::a_held_key_that_chose_from_a_buttons_menu_reopens_nothing`
chooses a real dropdown's item with a fresh Enter and Space and holds the
key on the button the menu returned focus to. Both fail with the repeat
check removed. Remove this part when GPUI hands an action its key event's
repeat flag or upstream opens a popover only on a fresh press.

The panel-fit patch modifies `src/resizable/mod.rs` and
`src/resizable/panel.rs`. Upstream a
`ResizablePanelGroup` learns its size only as it is prepainted, after its
panels were laid out at their sizes for the old size, and only then rescales
them (`adjust_to_container_size`), so the first frame at a new size draws the
panels where they were, and the rescaled panels reach the screen only with
the next frame something asks for, as the next input: a notify during a draw
asks for none. In GitTurtle's content columns that frame drew the inspector
at its old width, held to its new range, and the comparison beside it up to
120 px wider than it settles. `ResizableState::fit_container` rescales the
panels for a size the caller knows before the group is laid out at it, as
the group would after, and records the size, so the group's own adjustment
then finds nothing to do; the group still adjusts itself where no caller
fits it, and notifies as it does. `fit_container` notifies no one, and
neither does a panel as it records, while it is prepainted, the bounds and
the size the frame laid it out at (`update_panel_size`, which no longer
takes a context): the frame that draws the group lays it out at those
sizes, and only a later build of the group reads what the panels record.
Upstream's panel record and the group's adjustment notified, and so did
the fit as first added here. Made while the frame is drawn, such a notify only
marks a state the window drew in the frame before, but GPUI delivers it to
the observers of one the window did not draw then once the frame is done,
and an observer that redraws, as GitTurtle's window observes its groups,
drew a second frame: every width step that widened History past its narrow
rail, which draws its navigation group again, drew twice
(`views::tests::history_navigation_drawn_again_by_a_widening_notifies_no_one`).
A drag, an insert, a removal and a reset still notify. The consumers are
GitTurtle's three groups: the root render fits the project pane's group, `render_repository` the content columns' group
and `render_content_columns` History's navigation group, each to the width
it has in its frame (an `InFrame`) before building it. They are covered on
every platform by
`cargo test --locked -p gitturtle a_resize_lays_each_panel_group_out_where_it_settles`,
and the project pane's fit to the width its body is drawn at, rather than to
the window's content, by `the_project_pane_is_fitted_to_the_body_it_is_drawn_in`:
a test window draws the body exactly as wide as its content, so a test-only
inset narrows the body by 100 px. Remove the patch when upstream lays a
group's panels out at their rescaled sizes in the first frame at a new size,
and those tests pass with the three `fit_container` calls removed.

The placeholder-fit patch modifies `src/input/base/state.rs` and
`src/input/base/element.rs`. An application that fits a field's placeholder
to the width the field gives its text, single-line or multi-line, as
GitTurtle shortens a search field's, a filter's and the commit composer's in
a narrow window, could only read that width
once the field had been painted (`text_bounds`), so a new width drew the
earlier width's placeholder and the application's new one only in a second
frame. `InputBaseState::set_placeholder_fit` gives the field a `PlaceholderFit`:
a function from the width its text is laid out at to its placeholder, and
the placeholder whole. The text element calls the function as it is
prepainted, with its bounds in that frame, before it shapes the placeholder,
and keeps the result as the field's own placeholder without a notify. The
field's accessible placeholder is the whole form at every width
(`InputPresentation::accessible_placeholder`, which the component patch's
`Input` reports): a field's accessible metadata is given as it is built,
before the frame lays it out, so one that followed the drawn form kept the
earlier width's form after a width step until something else redrew, and
no width bounds what a screen reader reads. Without a fit the field shows
and reports the placeholder it was given, as upstream. The text element
runs a fit for every field that has one, single-line or multi-line.
GitTurtle sets one through `workspace::placeholder_fit`, as the field is laid
out, on each field it fits, each covered by its own regression:

- History's search field: `views::tests::history_search_fits_its_placeholder_and_keeps_its_name`;
- the changed files' filter: `views::tests::changed_file_filter_fits_in_whole_words_and_keeps_its_name`;
- the Working filter: `views::tests::working_filter_placeholder_fits_its_field`;
- the navigation's filter: `views::tests::the_navigation_folds_its_modes_and_hint_before_they_wrap`;
- the tags filter: `tags::tests::tags_text_starts_at_the_filters_inset`;
- the commit title and the commit description, a five-row multi-line field:
  `views::tests::narrow_composer_keeps_its_texts_inside_the_column`.

History's search field and the changed files' filter are also stepped
between widths, their drawn and accessible placeholders compared with a
window opened at each width, by
`cargo test --locked -p gitturtle draws_one_frame_where_it_settles`. Remove
the patch when upstream chooses a placeholder from the laid-out width in the
frame.

The caret-start patch modifies `src/input/base/blink_cursor.rs` and changes
runtime code only, beside a test of its own. Upstream `BlinkCursor::start`
toggled the caret rather than showing it, and an input starts its cursor
as focus is reported, which GPUI does after the frame that moved focus.
A field given focus as a frame is drawn, as GitTurtle gives a warm
repository tab its focus back in the frame that draws the tab
(`repository_tabs::return_tab_focus`), so drew that frame with the caret
as its blink had left it and then a second frame that only showed the
caret, or hid it: a tab left in a text field drew two frames on its
return, against one frame per input. A programmatic focus
(`InputState::focus`) starts the cursor and then reports the focus that
starts it again, so it hid the caret at once. `start` now shows the caret,
notifying only where it was hidden, and schedules its first blink one
interval later, so a second start changes nothing; `stop`, on blur, and a
new cursor leave the caret shown for the next focus, which draws it in its
first frame (an unfocused input draws no caret). Blinking after that is
upstream's: hidden one interval after the start, shown the next, and held
shown while typing or moving pauses it.

Leaving the caret shown has one side effect, which `pause` now guards.
An edit pauses a shown caret (`replace_text_in_range`), focused or not,
and upstream's pause schedules a blink as it ends; with the caret left
shown, text set in an unfocused field (`set_value`, which a warm
repository tab's return calls on each of its six fields) set the field
blinking for good, notifying it, and so redrawing the window, twice a
second while idle. A cursor that is not blinking, a new one or one stopped
on blur (epoch 0, `BlinkCursor::started`), now pauses nothing and
schedules nothing. So a click into an unfocused field, which places the
caret before the focus is reported, no longer holds the caret through a
300 ms pause: the caret shows as the click focuses the field and first
blinks off one interval after the focus starts it, as a field focused by
its keys or a warm tab's return does. A focused field still holds its
caret through a pause while typing and blinks again after it. `stop` also
ends a pause: a warm tab's switch sets a still-focused field's text, which
pauses it, just before the blur stops it, and that pause's own end finds a
later epoch and blinks nothing, so a start that waited for it left the
caret shown for good on the tab's return.

The caret-start patch also modifies `InputState::focus` in
`src/input/base/state.rs`, runtime code only. Upstream's `focus` started
the cursor as it asked for focus, before GPUI reported the focus and so
before any blur could stop it: a field focused there and left before a
frame was drawn, as the project hub's name field would be if focus moved
on before a frame after the folder picker returns and focuses it
(`projects.rs`), never had the blur that stops its cursor and blinked,
notifying the field and redrawing the window twice a second, until it was
next focused and left. `focus` no longer starts the cursor: `on_focus`
starts it as the focus is reported, after the frame that focuses the
field, which draws the caret shown, as a new or stopped cursor leaves it;
the caret still first blinks off one interval after that frame. `focus`
pauses the cursor instead, which does nothing to a cursor not started, so
a field focused there and left before a frame still never blinks. A field
that holds focus already, whose focus GPUI does not report again, has its
caret shown at once and held through the pause, then blinking again: the
edits that empty such a field pause only a caret that is shown, so
without the pause one emptied while its caret was hidden in its blink drew
no caret for up to an interval. The callers that empty a focused field and
then focus it are the toolkit Input's clear button
(`vendor/gpui-component/src/input/input.rs`, `clean` and then `focus`
through `InputSurface::focus` in `src/input/state.rs`; the button takes no
focus as it is pressed), which the app's cleanable filters show, and the
project hub's search, cleared with `set_value` and then focused
(`projects.rs`). The kit's `OtpState` (`src/otp_input.rs`), whose `focus`
already left the start to its focus report, shares `BlinkCursor` and so
`start`, `stop` and `pause` above; the application uses no `OtpInput`.

The caret-start patch also removes the window-activation observer from
`InputBaseState::new_in_mode` in `src/input/base/state.rs` and from
`OtpState::new` in `src/otp_input.rs`, runtime code only. Upstream's
observer started the cursor of whichever field held the window's focus as
the window turned active, drawn or not: a field on a tab not shown, in a
dialog closed with its state kept, or focused from code and left for
another before a frame, had no blur to stop it and blinked, notifying it
twice a second, until it was next focused and left. GPUI already reports
focus in the first frame it draws after the window turns active, and a
blur in the first after it turns inactive, to the field focused and drawn
in that frame (`WindowFocusEvent` counts an inactive frame's focus path
empty), so `on_focus` gives a drawn field its caret back, shown in that
frame and blinking off one interval later, and `on_blur` stops it; the
observer only ever added a start for a field that frame did not draw. The
kit tests are
`input::state::tests::window_activation_blinks_only_the_drawn_focused_input`,
a field drawn and focused blinking, stopping while the window is inactive
and blinking again once it is active and drawn, while one taken out of the
frame, holding the window's focus, starts nothing as the window turns
inactive and active (ten blinks in five seconds with the observer), and
`otp_input::tests::window_activation_blinks_only_a_drawn_focused_field`,
the same for an OTP field (`scripts/vendor-crate-test.sh vendor/gpui-base
window_activation`). Remove this part with the rest of the caret-start
patch.

The consumers are every text field: History's search field, the filters
and the commit composer among them. The kit tests are
`input::blink_cursor::tests::a_start_shows_the_caret_at_once_and_blinks_an_interval_later`
and `input::blink_cursor::tests::a_cursor_not_blinking_pauses_nothing`
(`scripts/vendor-crate-test.sh vendor/gpui-base blink_cursor`); the
application's are `views::tests::an_unfocused_field_given_text_stays_still`,
`views::tests::a_focused_field_holds_its_caret_while_typing_and_blinks_after`,
`views::tests::a_warm_tab_left_in_a_field_draws_one_frame_with_its_caret`,
a tab left with its caret shown and with it hidden in its blink, each
drawing one frame on its return with the caret in it,
`views::tests::a_field_focused_by_its_keys_or_a_click_blinks_its_caret`
and `views::tests::a_field_focused_from_code_blinks_only_while_it_holds_focus`,
History's search field focused through `InputState::focus` blinking off
one interval after the frame that focuses it, and, focused there again and
left for the commit list before any frame, notifying nothing and drawing
no frame for five seconds (10 notifications with upstream's `focus`), and
`views::tests::a_focused_field_emptied_and_focused_from_code_shows_its_caret`,
History's search field holding focus with its caret hidden in its blink,
emptied with `clean` and focused from code, drawing its caret in the next
frame (no caret without the pause in `focus`) and blinking again after.
`caret_focus_tests::history_s_search_field_draws_its_caret_as_it_takes_focus`,
`_the_commit_message_` and `_compare_s_editor_`, and
`command_palette::render_tests::the_palette_s_query_draws_its_caret_as_it_takes_focus`,
hold the caret to the first frame for each kind of field: History's search
field (single-line), the composer's commit message (multi-line), Compare's
editor (the code editor) and the command palette's query (a dialog's
field), each left with its caret hidden in its blink and focused again by
Tab, by a click and from code (`InputState::focus`), draw it in the frame
drawn in the update that focuses the field, before GPUI reports the focus,
and the palette's query in the one frame its opening draws (FU23-B,
2026-11-06: no kind drew it late).
Remove the patch when upstream shows the caret as a focus starts it,
pauses no cursor it has not started, starts none before the focus is
reported, shows a focused field's caret as it is focused again, and those
tests pass without it.

A text decoration can carry a bottom rule: `TextDecoration::bottom_rule`, an
optional colour set with `TextDecoration::with_bottom_rule`, which
`src/input/editor/decorations.rs` adds and keeps when it clips a range, and
`src/input/base/element.rs` paints. Upstream a decoration carries only a GPUI
`HighlightStyle`, whose `underline` GPUI's text pass draws as an `Underline`
primitive at a font-metric offset (the line's centred padding, the ascent and
0.618 of the descent below the line's top), not on the edge of the box a
background fills: in the bundled DejaVu Sans Mono at 13 px in a 20 px line its
top lies 16.4 px down, its 1 px stroke ending 2.6 px above the box's bottom (5
device pixels at scale 2), and at 18 px in a 27 px line 22.4 px down, 3.6 px
above it (7 device pixels); it would also be the property Find's accent
underline on the current match takes. GitTurtle's Diff view and Split mark
each changed word under More contrast with a 1 px rule inside its highlight
box's bottom edge, on a decoration of its own over the word's range
(`crates/app/src/text.rs`'s `theme_decorations` and
`crates/app/src/split_diff.rs`'s `SplitSide::decorations`, the colour from
`appearance::EditorHighlights::word_rule`). The decoration collections keep,
as their decorations are created, set, appended and cleared, whether any
holds a rule, which the editor reports through the added
`InputExtras::has_bottom_rules` (`src/input/base/kind.rs`, false for other
inputs, and `src/input/editor/mod.rs`); without one, a frame composes the
decorations with upstream's `compose_decoration_collections` unchanged and
paints no rule, so More contrast off pays one flag check a frame and a
branch a visible line. Where one holds a rule,
`compose_ruled_decoration_collections` reads each collection's visible rules
in the pass that composes the styles,
so a frame walks each decoration once, and leaves a rule out where an earlier
collection draws an underline, so two strokes never run one above the other:
Find's current match, in the application's first collection, draws its accent
underline about 2.6 px above the rule and hides it, while Find's other
matches, backgrounds alone, keep it, since the application removes the word's
tint under every match and the rule is then what marks a matched word as
changed. The underlines are sorted once with the furthest each prefix reaches,
so a rule is checked only against the underlines that can reach it (Find
highlights up to 4,096 matches at once). The element keeps the
rules in its prepaint state, and the background pass paints each line's
rules after that line's backgrounds, each visual line's rules in one layer as
the background pass paints each line's boxes, so the scene orders them once a
line and they lie over the backgrounds and under the indent guides,
selections and text. A rule spans the box the
background pass fills for its range on each visual line, walking the glyphs
as GPUI's `paint_line_background` walks them, so its edges are the same
floats and land on the same device pixels; one walk of a visual line's glyphs
finds every rule's edges, answering the edges' bytes in ascending order, each
at the first glyph whose index reaches it, which holds in any glyph order, so a 16 KiB line with 81 changed words costs one walk, not two
a rule; it takes the box's bottom edge on
its device pixel and a 1 px stroke's device pixels above it (one at scales up
to 1.5, two at 2, halves toward zero as GPUI snaps a stroke), adjacent rules
of one colour join as adjacent backgrounds of one colour fill one box, and a
range without width paints none. `TextDecoration` grows by the
`Option<Hsla>`, which the application's retained-byte accounting counts
through `size_of`. A decoration without a rule paints exactly as before, the styles are composed as before, and IME's marked-text underline
and every other underline are unchanged. The kit tests are
`input::element::tests::a_bottom_rule_is_painted_inside_its_boxs_bottom_edge`,
at scales 1, 1.2, 1.25, 1.5 and 2, with the rule on a decoration of its own
and on the one that sets the background, and the frame otherwise as without
the rules,
`input::element::tests::a_wrapped_range_draws_its_bottom_rule_under_each_visual_line`,
a soft-wrapped range ruled under each of its visual lines' boxes,
`input::element::tests::bottom_rules_are_hidden_only_by_an_earlier_collections_underline`
(`scripts/vendor-crate-test.sh vendor/gpui-base bottom_rule first_glyphs
decorations`), which also reads a frame without rules leaving them unread,
`input::element::tests::first_glyphs_at_answers_as_a_search_in_walk_order`,
the walk against a first-match search over ascending, shuffled and descending
glyphs, and
`input::decorations::tests::collections_know_whether_any_decoration_has_a_bottom_rule`,
with clipping in
`input::decorations::tests::collections_are_independent_and_ranges_are_clipped`;
the application's are `text::tests::more_contrast_rules_each_changed_word_in_the_diff_view`,
`text::tests::find_hides_a_changed_word_s_rule_only_under_its_current_match`
and `split_diff::tests::more_contrast_rules_each_emphasized_word_in_split`.
Remove this part when upstream decorations can draw a rule along their box's
bottom edge.

A text decoration can also fill its lines' whole rows:
`TextDecoration::row_background`, an optional colour set with
`TextDecoration::with_row_background`, which `src/input/editor/decorations.rs`
adds and keeps when it clips a range, and `src/input/base/element.rs` paints.
Upstream a decoration's background is a GPUI `HighlightStyle` property that
the text system's `paint_line_background` fills run by run, from a run's first
glyph to the glyph after its last, so it ends where the line's text ends; a
line's break is never shaped, so a range that includes it paints nothing past
the text, and a blank line, which has no glyphs, paints nothing at all. The
editor's only full-width fill, the active line, follows the cursor in one
theme colour and is drawn only with line numbers. GitTurtle's Diff view and
Split carry each changed line's tint across the editor's whole row, so a
hunk's changed lines read as one block with a straight right edge and a blank
changed line is tinted across its row (decision, 2026-11-05): before this
part, at 13 pt in a 900 px window the Diff view's tint ended 671.5 px short of
the visible right edge on `-call(red, 7);` and 772.5 px short on a blank `-`
line, under its sign alone, Split drew no tint on a blank changed line, and
scrolled to the far right a long changed line's tint ended 10 px short of the
edge, the editor's right margin. The application gives each run of adjacent
changed lines of one side one decoration without a background that sets the
row background in the side's line tint (`crates/app/src/text.rs`'s
`theme_decorations` and `crates/app/src/split_diff.rs`'s
`SplitSide::decorations`, which leaves Split's filler rows out); having no
background, it passes whole through GitTurtle's Find, which rebuilds every
decoration that sets one around its matches, and the matches' opaque tints
are painted over it as over the line's own runs. A range reaches a line where
it covers some of the line's text or its line break. Both compositions,
upstream's `compose_decoration_collections` and
`compose_ruled_decoration_collections`, read each decoration's row background
in the pass that composes the styles, so a frame walks each decoration once,
and keep those reaching the visible lines, one over only the last visible
line's break included (the styles' visible range ends before that byte), with
their collection's index, sorted once by start; a decoration without one costs
that pass one `Option` check, and a frame with none paints as upstream. The
element keeps them in its prepaint state, and the background pass fills each
visible line one reaches from the text area's left edge, where a left-aligned
line's first glyph and a background on its first byte start, to the editor's
visible right edge (the element's right edge) or the line's end where that
lies further, over every visual row of the line. It paints a run of adjacent
lines filled in one colour to one right edge as one quad, from the run's first
visual row's top to its last's bottom, at the run's first line, before any of
its lines' backgrounds, so a block of changed lines costs one quad and one
scene entry rather than one a line; a run ends at a line filled otherwise or
not at all, and at the gap ghost lines open below the cursor's row, and the
pixels are those the lines' own quads painted. FU18-A measured the fills'
cost per filled line, about 1.2 µs in release on the Asahi host, 6 to 10 % of
a dense diff's scroll frame, which includes the composition's walk and its
sort as well as the quad. Painting a run as one quad recovers about half of
it: on the Asahi host, against 99cd8cb3 in
`split_diff::tests::diff_scroll_release_benchmark`, three alternating runs
each, the dense Diff view and Split scroll 4 to 5 % faster (the Diff view at
13 pt 0.773 to 0.736 ms a step, Split at 13 pt with More contrast 1.858 to
1.770 ms), and the many-blocks and crafted worst cases take as long as before
(×0.99 to 1.00). It is
drawn under the line's decoration backgrounds, its bottom rules,
the indent guides, selections, Find's matches and text, which are painted
after it in the same order as before, and under the line-number gutter, which
covers the rows that run under it when the text scrolls sideways, as it covers
the runs. Where several reach a line, the first collection's is painted, and
within one collection the one starting first. With line numbers on, the
active line, which the element paints before this pass, is painted again over
a fill on its row, one quad, so its colour still shows past the text there as
on a row without one. No current GitTurtle consumer combines line numbers with
row fills (the diff views turn the editor's line numbers off and draw their
own gutter); the repaint keeps the toolkit's active line visible for any
editor that does. It allocates nothing a line,
and the list a frame reads them into keeps eight inline, so a frame showing
no more than eight filled blocks allocates nothing for them.
`TextDecoration` grows by another `Option<Hsla>`, which the application's
retained-byte accounting counts through `size_of`. A decoration without a row
background paints exactly as before. Upstream's
`input::element::tests::test_first_decoration_collection_has_precedence`
passes the composition its new list, which it leaves empty. The kit tests are
`input::element::tests::row_backgrounds_are_read_where_they_reach_the_visible_lines`,
both compositions reading the fills that reach the visible bytes or the last
line's break, by start, the styles as without them, and a line taking the
first collection's fill, then the one starting first,
`input::element::tests::a_row_background_fills_its_lines_rows_to_the_visible_right_edge`,
at scales 2 and 1.25 and scrolled sideways, a short line's and a blank line's
rows in one quad from the text's left edge to the visible right edge and a
long line's to its end, under the line's background, and the frame otherwise
as without them,
`input::element::tests::adjacent_filled_lines_paint_one_quad_a_run`, three
adjacent filled lines one quad, an unfilled line between two runs two, and a
line in another colour after a run its own,
`input::element::tests::ghost_lines_divide_a_run_of_filled_lines`, an inline
completion's ghost lines below a filled cursor line dividing the run around
it into two quads that do not touch, the ghost lines' height apart, and
`input::element::tests::a_wrapped_line_s_row_background_covers_each_visual_row`
and `input::element::tests::the_active_line_shows_over_a_row_background`, the
active line painted over its row's fill, the caret on a run's first line or
its second, and not over another filled row's
(`scripts/vendor-crate-test.sh vendor/gpui-base row_background filled_lines`), with
clipping in
`input::decorations::tests::collections_are_independent_and_ranges_are_clipped`;
the application's are
`text::tests::a_changed_line_s_tint_spans_its_row_in_the_diff_view` and
`split_diff::tests::a_changed_line_s_tint_spans_its_row_on_each_split_side`.
Remove this part when upstream decorations can fill a line's whole row,
blank lines included.

The dialog-button patch (2026-11-05) modifies `src/dialog.rs` and
`src/button.rs`. Enter on a focused Button in a dialog presses that Button,
once per press, as the key goes down, rather than the dialog's default
action (decided by the coordinator, owner-delegated: the focus ring shows
the control the keyboard acts on, and GTK and Windows activate the focused
button). Upstream binds Enter to `Confirm` in the `Dialog` context, and a
binding is dispatched before any key listener, so GPUI's key-up activation
of the focused Button never saw the key: Enter with the ring on a body's
Button, or on Cancel, ran the dialog's default action and pressed nothing.
The base Button now carries the key context `Button` from its constructor,
which a caller's own `key_context` replaces, and Enter is bound to GPUI's
`NoAction` in `Dialog > Button`, which at the Button's depth outranks the
dialog's `Confirm`, so the key-down reaches the Button. An enabled,
focusable Button with a click handler takes it in the capture phase
(`dialog::enter_presses_button`, which also requires a `Dialog` context
around the Button, so outside a dialog nothing changes): a fresh press calls
the click handler with a keyboard click on the Button's laid-out bounds, a
repeat (`is_held`) calls nothing, and either way the key goes no further, so
GPUI's key-up activation does not press it again. Under X11 a held key
arrives as fresh presses, which this cannot tell apart, as the
popover-trigger patch notes. A listener an ancestor captures the key with
still runs first, as the popover trigger's and the app's About's do. Enter
in a single-line field, on the dialog itself and on anything but a Button
is the dialog's `Confirm` as before, the default Button's click is that
action, a multi-line field keeps Enter as a new line, and Escape and Space
are unchanged. The consumers are every app dialog; About's Copy bug
diagnostics and the GitHub panel's Buttons, which bound Enter to `NoAction`
in contexts of their own to the same end, keep working, as does the custom
theme's delete confirmation, whose footer routed `Confirm` to its focused
Button. The kit test is
`dialog::tests::enter_on_a_focused_button_presses_it_once`
(`scripts/vendor-crate-test.sh vendor/gpui-base dialog:: button::`), and the
application's is
`appearance::tests::enter_presses_the_focused_button_in_a_dialog`, which
Tabs onto a body's Button and presses Enter, holds it, and presses Enter in
a single-line field, a multi-line field and on the default Button, with
`width_walk`'s About test pressing Copy bug diagnostics once. Remove this
part when upstream leaves Enter on a focused Button in a dialog to the
Button, and those tests pass without it.

The same patch keeps a held Enter from confirming a dialog (decided by the
coordinator after the security re-review of 2026-11-05). Bound to
`Confirm`, Enter confirmed on every repeat a held key sends, since GPUI
hands an action no key event: held on a body Button that opens a
confirmation (Stashes' Drop…, Git profiles' Delete…), the fresh press
opened it, `Root` focused it, and the next repeat ran its write, one
physical press. Enter is now bound to `NoAction` in `Dialog`, so it reaches
no binding outside the dialog, and to a hidden `EnterKey` after that at the
same depth, which outranks it there and marks the press as the dialog's;
the dialog host's capture-phase key-down listener then dispatches the
`Confirm` the binding did, on the focused element, only for a fresh press
(`is_held` unset), and takes the key either way, as the binding did. A
press something inside the dialog binds (a single-line field, whose
binding hands it on, a multi-line field, which keeps it as a new line, a
Button, a popover's trigger) reaches it as before. Under X11 a held key's
repeats arrive as fresh presses, so a dialog opened while Enter is down
also takes no Enter, neither its `Confirm` nor a press of its Buttons,
until that Enter comes up: a keystroke interceptor marks Enter down in its
window before any binding runs, `dialog_opened`, which the component
`Root` calls as it opens a dialog, turns the mark into a wait, and
`note_key_up`, which the dialog host and the component `Root` call on every
key-up in the window, ends both. A dialog opened by a click or with no key
down takes its first Enter as before; Escape and Space are unchanged. The
kit tests are `dialog::tests::a_held_enter_confirms_a_dialog_once` and
`dialog::tests::a_dialog_waits_for_the_enter_that_opened_it`, and the
application's are
`appearance::tests::a_held_enter_never_confirms_what_it_opened_over_its_dialog`
and `_in_its_dialogs_place`,
`appearance::tests::a_repeat_that_looks_fresh_waits_for_the_enter_that_opened_the_dialog`
and `appearance::tests::a_dialog_opened_with_no_enter_down_confirms_on_the_first_press`,
which also holds Space on a body Button.

The same patch keeps every repeat of an Enter that opened a dialog from
anything in its window until that Enter comes up (decided by the
coordinator, 2026-11-06: an Enter that opens a dialog from anywhere never
acts inside that dialog or behind it until its key-up; after the key-up
Enter works as usual). Two paths escaped the wait above. A binding runs
before any key listener, so while the dialog's wait held off its own
confirmation and its Buttons, a field's or a list's own Enter binding inside
it took every repeat: held on the command palette's "Quick Open File…", the
repeats reached Quick Open's query, which opened the highlighted file. And
on Wayland an input method sends each repeat of a key held in a text field
as a release and a press together (fcitx5 on Omarchy;
`docs/development/2026-11-03-wayland-text-input-focus.md`), so the release
ended the press: held on "Browse reflog and recover a commit…", where the
palette's query and the Reflog's filter are both text fields, the first
repeat confirmed the Reflog and the next reached History's changed files
behind it, which opened Compare (QA-105). The keystroke interceptor now
stops, before any binding, every repeat of a press a dialog waits out, in
the dialog, behind it once it has gone and anywhere else in its window: an
Enter going down again with no Enter key-up or other key since, as a held
key repeats on every platform, or right after an Enter key-up that came with
it. An Enter key-up no longer ends a recorded press at once (`note_key_up`):
the press ends once the application next runs a task (`end_released`),
unless an Enter goes down first, and a dialog opened in between waits too
(`dialog_opened`). That task runs in the event loop's next idle pass, not
within the batch of input events the key-up came in: on Linux the spawn
wakes the main-thread channel, a ping source read in the loop's current
iteration if its ping is already pending, or else in the next, and its
runnable runs as an idle callback at the end of that iteration, after every
input source of the key-up's batch
(`vendor/gpui-pre-linux/src/linux/dispatcher.rs`, `wayland/client.rs`). A
person who releases Enter and presses it again
within that window, as within one slow frame, loses that press, as a press
whose key-up never arrives does (below), which fails safe. The pairing
assumes the compositor delivers an input method's release and press
together, as every one of the 426 fcitx5 pairs recorded in QA-90's and
QA-91's Wayland logs arrived, 3 to 54 µs apart with no other event between
(FU21-E's security review). Such a stale press that a dialog
waited out also takes the next Return in the window, until that Return comes
up, another key goes down or an Enter key-up in another of the application's
windows ends it (below). A key the interceptor stops still reaches the
first key listener on the focused path, as GPUI hands it one: the component
`Root`'s key-up listener, which takes no key-down, or in a window without it
a dialog host's, which takes no Enter while the dialog waits. On macOS a
Command-Enter still records nothing and is never stopped; every other
modified Enter is stopped as a plain one is. A residual there, unverified
without a Mac: a Command pressed during the hold is another key to the
interceptor and ends the press's record of where it began, so the first
plain repeat after Command comes up is not taken for a repeat and reaches a
field's or a list's own Enter binding, though never the dialog's
confirmation or a Button, since macOS marks repeats held; whether Enter
goes on repeating across the Command at all is unverified. Under X11 the unpaired release
at a poll batch's end (below) still ends the press, since the repeat's
press arrives in the next batch. The kit tests are
`dialog::tests::a_dialog_waits_out_an_input_method_s_repeats` and
`dialog::tests::a_held_enter_reaches_nothing_in_or_behind_the_dialog_it_opened`,
and the application's is
`command_palette::render_tests::a_held_return_on_a_palette_command_opens_its_dialog_once`,
which holds Return on the Reflog's and Quick Open's commands with repeats
marked held, as X11 sends them and as an input method sends them.

The same patch keeps a held Enter on the control its press began on
(decided by the coordinator, owner-delegated, 2026-11-05: a held Enter that
moves focus acts once, and a Button never acts on an Enter whose press
began while another control had focus). A held Enter on a Button that keeps
focus still presses it on every repeat under X11, as the dialog-button part
above says, since nothing tells those repeats from fresh presses. Under X11
a held key's repeats arrive as fresh presses,
so a press that moved focus to a Button, as Return on the application's
rewritten-series list moves it to Back to series, had the next repeat press
that Button, which moved focus back, and the next act again: the review
toggled, starting and cancelling a read, with every repeat. The keystroke
interceptor also records, in each window, the focus where its Enter press
began, at the press's first key-down since that window's last Enter key-up
(`note_key_up`) or any other key, which ends a held key's repeats as it
does on every platform; the Button presses only on an Enter whose press
began on it (`enter_press_began_on`, exported from `src/lib.rs` so an
application's own Enter handlers ask the same, as the series list does). An
Enter pressed with Shift, Control, Alt, Fn or, off macOS, Super held is an
Enter press as a plain one is, for this record and for the `down` mark
above, since its key-up arrives (decided by the coordinator,
owner-delegated, 2026-11-05, after the security re-check): under X11 each
repeat of a held key carries the modifiers held as it repeats, so a held
Shift- or Control-Enter left unrecorded (a one-line field takes either for
Enter) opened a reviewed write's confirmation that did not wait for its
key-up, and the plain repeat after the modifier came up confirmed it, or
pressed the Button focus had moved to. Only on macOS is a Command-Enter (a
list's, menu's or combobox's secondary Enter) such another key, marking
nothing (`key_up_withheld`): Cocoa sends no key-up for a key pressed with
Command held, and a recorded one stayed down and took the next Return on
another control for its repeat. One residual remains there, unverified
without a Mac: Enter pressed alone, then Command, then Enter released while
Command is held, a key-up Cocoa may drop as well; the press then stays
recorded, and the next plain Return presses no Button but the one the press
began on, nor confirms a dialog opened meanwhile, until its own key-up ends
the press: one Return swallowed, as by a press whose key-up never arrives
(below), which fails safe. A press whose key-up
never arrives counts as held until another key or an Enter key-up, in its
window or in another of the application's windows with no press of its own
(below), so a Button focused by a pointer meanwhile takes one press before
it acts. Under X11 a repeat's synthetic
release can also end the press: the backend holds a key release back to
pair it with the press of an autorepeat, but sends one still unpaired at
the end of a poll batch (`vendor/gpui-pre-linux/src/linux/x11/client.rs`,
after its event loop), which `note_key_up` takes for the key coming up, so
a rare repeat can still begin a fresh press on the control focus moved to.
On macOS and Wayland repeats are marked held, so nothing else changes
there. The kit tests are
`dialog::tests::enter_on_a_focused_button_presses_it_once`, which moves
focus to the Button while a press is held and then presses it with a press
of its own, and on macOS presses it with a Return after a Command-Enter
that never came up, or elsewhere only after a Super-Enter's key-up;
`dialog::tests::a_dialog_opened_by_a_held_shift_enter_waits_for_its_key_up`
and `_control_enter_`,
`dialog::tests::a_button_in_a_dialog_opened_by_a_held_shift_enter_takes_no_repeat`
and
`dialog::tests::a_control_tap_during_a_held_enter_lets_no_repeat_press_the_button`
send a modified Enter's repeats as X11 delivers them; and the application's
is `rewrite_review::tests::a_held_return_on_a_series_row_inspects_once`.
Remove this part with the rest of the patch when upstream leaves Enter on a
focused Button to the Button.

The same patch ends a press only on an Enter key-up, never as its window
loses focus (decided by the coordinator, 2026-11-06, after FU23-B's
security re-check, replacing a rule that forgot a window's press as the
window was deactivated): in its own window, or in another of the
application's windows with focus inside its root and no press of its own,
once no Enter goes down in any window before the application next runs a
task. `note_key_up` in such a window ends, through `released` and
`end_released`, the press of every window still holding Enter down, so on
Wayland and macOS an Enter held in one window while focus moves to another,
and released there, costs the first window nothing (FU18-C's re-check: the
next Enter there was taken for a repeat and lost); under X11 the first
window swallows one Return (below). Such a key-up shows the key came up
after focus left the window it went down in, except X11's unpaired release
at a poll batch's end (above), whose press arrives only in the next batch,
after that task: the recorded residual, which can arrive through another
window too, as the first repeat pair after focus enters it splits at a
batch's end. A key-up in a window with nothing focused reaches no `Root`'s
`capture_key_up`, so the press stays recorded and one Return is swallowed,
which fails safe. Any Enter going down in any window before that task,
Command-Enter included, cancels the end and leaves those presses held
(decided by the coordinator, 2026-11-06, after FU23-B's code and security
re-checks): an input method's repeat, as fcitx5 on Wayland sends each repeat
of a key held in a text field, delivers its release where focus moved to,
with no press recorded there, and its press after it in the same window,
not in the one whose press the release ended, so the release ended that
press and the next Enter there, the key still down, confirmed the dialog it
opened. A press
whose repeats another key ended, which only holds the key down, is one of
them, so a dialog that opens in its window afterwards still waits for the
key-up. A person who releases Enter in the other window and presses it
again before that task, as within one slow frame, leaves the first window's
press held, and its next Return swallowed, which fails safe. Under X11 a
held Enter's autorepeats arrive as fresh presses (`is_held` unset), and the
backend turns every FocusIn and FocusOut into the window's activation
changing (`vendor/gpui-pre-linux/src/linux/x11/client.rs`), including the
pair another client's keyboard grab causes, as a global shortcut, a window
move or a screenshot tool takes the keyboard and gives it back while the key
stays down. Forgetting the press on deactivation therefore let the first
autorepeat after such a bounce confirm the reviewed write the press's dialog
asked about (deleting a tag, dropping a stash, a merge, a forced worktree
removal) or press a Button focused in it, and a dialog that opened while the
window was inactive did not wait at all; a press that keeps its record
through the bounce stops those repeats as before. An Enter key-up delivered
to another application reaches none of the application's windows, so the
press stays recorded and the next Return in its window, a press of its own,
is taken for one of its repeats and swallowed, until its own key-up, which
fails safe. Under X11 the autorepeats of an Enter held while focus moves to
another of the application's windows reach that window, which then records a
press of its own, so its key-up ends only that press, and the first window
swallows one Return the same way (decided by the coordinator, 2026-11-06,
after FU23-B's security re-check: had that key-up ended every window's
press, an unpaired release at a batch's end reaching the other window would
end the first window's too, and back there the next autorepeat, a fresh
press, would confirm the write its dialog asked about); GPUI's Wayland
client starts no repeat after the keyboard leaves a window, so there the
other window has no press of its own and its key-up ends the first window's.
The kit tests are
`dialog::tests::a_held_enter_waits_out_a_focus_bounce`,
`dialog::tests::a_dialog_opened_while_its_window_lost_focus_waits_for_the_key_up`,
`dialog::tests::an_enter_released_in_another_window_ends_its_press`,
`dialog::tests::an_input_method_s_repeat_in_another_window_ends_no_press`,
`dialog::tests::an_input_method_s_repeat_in_another_window_keeps_a_press_held_past_another_key`,
`dialog::tests::an_enter_released_where_it_repeated_ends_only_that_window_s_press`
and
`dialog::tests::an_enter_released_in_another_application_swallows_one_return`.
