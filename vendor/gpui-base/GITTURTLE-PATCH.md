# GitTurtle patch to gpui-base 0.6.0

This directory contains the crates.io `gpui-base` 0.6.0 source, licensed Apache-2.0. The upstream copyright and license are preserved in `LICENSE-APACHE`; `.cargo_vcs_info.json` records the published source revision. The registry archive checksum is `2caaf00ebe0482774dd370a82a936edf4bf19a18a0d1a39353d20ecf61f70330`.

The modified upstream sources are `src/button.rs`, `src/focus_trap.rs`, `src/dialog.rs`, `src/alert_dialog.rs`, `src/lib.rs`, `src/input/base/state.rs`, `src/input/base/element.rs`, `src/input/mod.rs`, `src/checkbox.rs`, `src/switch.rs`, `src/radio.rs`, `src/toggle.rs`, `src/link.rs`, `src/color_picker.rs`, `src/popup.rs` and `src/positioner.rs`; `src/dialog_label.rs` and `src/disabled_focus.rs` are added private helpers. The existing Button element is wrapped by a private element that delegates identity, layout, painting, role, and synthetic children. When `disabled` is true, the wrapper also sets the AccessKit disabled property in `write_a11y_info`, as it does for a Button that `accessibility_disabled` reports disabled alone: that Button keeps its enabled styling, focus and click handler, for an application that refuses the press in its handler before it dims the control (GitTurtle's `workspace::refusing`, through the [component patch](../gpui-component/GITTURTLE-PATCH.md)). When the resolved role is Tab, it also reports the existing selected state without adding pressed metadata; ordinary button presentation selection remains separate. Existing disabled pointer blocking, absence of click actions and keyboard focus behavior are unchanged. Delegation preserves caller-provided synthetic children instead of replacing that hook.

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
the consumers.
`cargo test --locked -p gitturtle kit_menus_stay_above_the_bottom_inset_an_application_sets`
opens a kit dropdown and a kit right-click menu at the foot of a window under
an inset of 120.5 px and reads each above it; without the whole-pixel rule the
right-click menu ended half a pixel past it.
`menus_keep_their_edge_8_px_above_the_status_bar` reads the app's menus 8 px
or more above the status bar. Remove this part of the patch when upstream lets
an application keep popups clear of a region at the window's foot, and those
tests pass without it.
