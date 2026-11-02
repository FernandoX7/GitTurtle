# GitTurtle patch to gpui-base 0.6.0

This directory contains the crates.io `gpui-base` 0.6.0 source, licensed Apache-2.0. The upstream copyright and license are preserved in `LICENSE-APACHE`; `.cargo_vcs_info.json` records the published source revision. The registry archive checksum is `2caaf00ebe0482774dd370a82a936edf4bf19a18a0d1a39353d20ecf61f70330`.

The modified upstream sources are `src/button.rs`, `src/focus_trap.rs`, `src/dialog.rs`, `src/alert_dialog.rs`, `src/lib.rs`, `src/input/base/state.rs`, `src/input/base/element.rs`, `src/input/base/blink_cursor.rs`, `src/input/mod.rs`, `src/otp_input.rs`, `src/checkbox.rs`, `src/switch.rs`, `src/radio.rs`, `src/toggle.rs`, `src/link.rs`, `src/color_picker.rs`, `src/popover.rs`, `src/popup.rs`, `src/positioner.rs`, `src/resizable/mod.rs` and `src/resizable/panel.rs`; `src/dialog_label.rs` and `src/disabled_focus.rs` are added private helpers. The existing Button element is wrapped by a private element that delegates identity, layout, painting, role, and synthetic children. When `disabled` is true, the wrapper also sets the AccessKit disabled property in `write_a11y_info`, as it does for a Button that `accessibility_disabled` reports disabled alone: that Button keeps its enabled styling, focus and click handler, for an application that refuses the press in its handler before it dims the control (GitTurtle's `workspace::refusing`, through the [component patch](../gpui-component/GITTURTLE-PATCH.md)). When the resolved role is Tab, it also reports the existing selected state without adding pressed metadata; ordinary button presentation selection remains separate. Existing disabled pointer blocking, absence of click actions and keyboard focus behavior are unchanged. Delegation preserves caller-provided synthetic children instead of replacing that hook. The Button test module no longer imports `gpui::Element`, which none of its tests use, so the crate's lib tests build without an unused-import warning; this test-only change goes with the module's tests.

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
ring itself. Left open: the clamps at the window's top margin and above
the bottom inset still move a popup by whole pixels from where it was
laid out, so a popup moved in there can stand up to a pixel off that
margin, as one moved in from a side did. Remove this part with the
component's window-margin patch.

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
Remove the patch when upstream shows the caret as a focus starts it,
pauses no cursor it has not started, starts none before the focus is
reported, shows a focused field's caret as it is focused again, and those
tests pass without it.
