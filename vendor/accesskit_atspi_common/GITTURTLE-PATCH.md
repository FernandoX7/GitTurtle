# GitTurtle patch to accesskit_atspi_common 0.19.1

This directory contains the crates.io `accesskit_atspi_common` 0.19.1 source,
licensed MIT OR Apache-2.0. The registry archive checksum is
`023da0e5097f46df7092d5280b02efb9bbf8d93298daeced42652463e357d636`;
`.cargo_vcs_info.json` records the published source revision
`c88605b96d04431f9c3c792464a0f2f253480e94` (tag
`accesskit_atspi_common-v0.19.1`, path `platforms/atspi-common`). Upstream:
https://github.com/AccessKit/accesskit

The published archive omits the license files its source headers name.
`LICENSE-MIT`, `LICENSE-APACHE` and `LICENSE.chromium` are copied unchanged
from the repository root at the same source revision; their SHA-256 digests
match the supplemental notices in `docs/licenses/dependencies.json`.

## Changes

Two files are modified: `src/node.rs`, with three changes, and `src/adapter.rs`.
No other source, manifest or dependency changes.

### Disabled controls (`src/node.rs`)

`src/node.rs` carries the upstream commit
`6ee0558b6315b3ef1594db24ce45a030ecac7cb5`, "fix: Expose non-disabled nodes as
enabled and sensitive on Unix" (AccessKit/accesskit#788, released in
`accesskit_atspi_common` 0.21.0), applied unchanged to `NodeWrapper::state`.
Upstream 0.19.1 added `Enabled | Sensitive` to every node unless its role
supports read-only and the node was read-only or disabled. A disabled Button,
whose role does not support read-only in `accesskit_consumer` 0.38.0, therefore
reported `enabled` and `sensitive`, and a disabled Switch, CheckBox or text
input reported `read-only`. With the backport, a disabled node reports none of the three; an
enabled node keeps `enabled` and `sensitive`, or `read-only` when its role
supports it and it is read-only. The `is_read_only()` it calls already exists
in the locked `accesskit_consumer` 0.38.0.

### Off-screen nodes (`src/node.rs`)

A GitTurtle change, not a backport: upstream `NodeWrapper::state` inserts
`Visible | Showing` for every node AccessKit's filter includes, and AccessKit
0.24 has no off-screen property an application could set instead. The
filter keeps a clipped node's first child past each edge, for an assistive
technology to move into, and any focused node wherever it lies. GitTurtle
reports such a node where it lies (`native_accessibility::drawn_part`
reports the layout of text the window draws none of), so without this
change AT-SPI read the GitHub panel's notice, below its body's fold at 18 pt
in a 461 x 490 window, as shown over the dialog's Back to repository
button. Before that, GitTurtle reported such text as an empty rectangle on
the clip's edge, which AT-SPI listed 0 px tall at y 557.

`NodeWrapper::state` now inserts `Visible` for an included node and
`Showing` only when the node's bounding box lies wholly outside the
bounding box of none of its ancestors that clip their children
(`clipped_out`): off screen, as browsers report a scrolled-away element. A
partly drawn node, a node without bounds and a node with no clipping
ancestor keep `Showing`. GitTurtle's clipping nodes report the part of
themselves the window draws (`native_accessibility::clips_children`), and
one the window draws none of reports a pixel beyond one of its corners,
outside its extent along both axes and, wherever a corner allows, outside
what is drawn, so its children lie outside it. Where no
corner allows, it reports its top left one anyway, which then lies in what
is drawn or in its clipping ancestor's bounds, as for a container
collapsed to no height inside what is drawn; this adapter can then read it
as showing. Every clipping ancestor counts, not the nearest alone:
AccessKit's filter keeps an inner clipping node past an
outer one's fold as the child past that edge, so a node inside the inner one
can lie wholly outside the outer one, the one place nothing of it is drawn.
The work is one walk up the node's ancestors, ending at the first clip that
leaves the node out, when a client reads its state or the adapter compares a
changed node's states. The adapter sends the existing state-change event for
a change of `Showing` only when the node's own data or its place under its
parent changes, or focus moves onto or off it, in an update:
`accesskit_consumer` 0.38 reports no other node as updated (`tree.rs`). A
change of a clipping ancestor alone, such as a fold a resized window moves
across rows that stay where they lie, sends no event; a client sees the
node's new state when it next reads it. That is an open item for the Orca
check (`docs/native-accessibility.md`). macOS has no
showing state and uses `accesskit_macos`, which this does not touch: there a
node AccessKit keeps past a fold, which GitTurtle reports at its layout, has
nothing marking it off screen, and VoiceOver's cursor may outline what lies
there, as the GitHub panel's notice below its body's fold over Back to
repository. That is an open item, not yet checked with VoiceOver; a list the
window draws none of reports its pixel beyond a corner there too, so none of
its rows is reachable.

Upstream status: not proposed upstream. The published 0.19.1 source inserts
both states for every included node; whether a later release tells an
off-screen node apart has not been checked.

### Point queries past a clip (`src/node.rs`)

A GitTurtle change, not a backport. `PlatformNode::accessible_at_point`
answers AT-SPI's `Component.GetAccessibleAtPoint` with `accesskit_consumer`'s
hit test, which ignores clips and tries later siblings first, so a node kept
past a scrolling body's top fold, reported where it lies under the dialog's
title, answered for the title's pixels, and so did a control that fold
cuts, at the pixels the body hides, since the hit test only skipped a node
wholly outside its clip. The query now passes over a node wherever any of
its clipping ancestors leaves out the point (`clipped_at`, the same walk as
`clipped_out`, without allocating), as it passes over a node the filter
passes over (`ExcludeNode`): its children are still tried, and the point
finds what is drawn there, a sibling or the nearest ancestor that holds it.
The extra work is that ancestor walk for each included node the hit test
visits, only when a client asks for a point. macOS's hit test lives in
`accesskit_macos` and is unchanged.

Upstream status: not proposed upstream. It shares the off-screen change's
ancestor walk (`clips`), which stays while either change does.

### A page shown again after a modal dialog (`src/adapter.rs`)

A GitTurtle change, not a backport: AccessKit `main` at `58412b89d`
(2026-08-26) has the same `node_updated`. `accesskit_unix` keeps one D-Bus
object for each node that AccessKit's filter includes, registered and
unregistered by this crate's `AdapterChangeHandler`; a path with none answers
every AT-SPI call with "Unknown object". `node_updated` acted on a node's
filter change only when its old or new result was `Include`. GitTurtle's page
(`native_accessibility::beneath_modal`) is a generic container, which the
filter passes over (`ExcludeNode`), and is hidden while any dialog is open
(`ExcludeSubtree`), and nothing happened between those two results. A page
node that changed as a dialog opened left the bus (its own update went from
`Include` to `ExcludeSubtree`), one that appeared beneath the dialog never
reached it, and once the page was shown none of them came back, while the
window listed them among its children again. Native QA on 2026-10-29 (builds
a79b83c and 7abe8b2) closed the tracking-branch review with Escape and found
23 of the window's 30 AT-SPI children answering "Unknown object", 9 still
after switching pages; confirming the review changed most of the page in the
closing frame, so those nodes' own updates put them back.

`node_updated` now handles both transitions. From `ExcludeNode` to
`ExcludeSubtree`, the node's included descendants leave the bus as an included
node's subtree does (`remove_subtree`). From `ExcludeSubtree` to
`ExcludeNode`, `show_children` registers every included descendant again and
emits the cache's `AddAccessible` for it, skipping one this update already
added. Nothing in it is new, so it announces no live region it puts back and
queues no selection change; a node whose own update in the same frame includes
it is still added, and announced, by the existing branch. The work is one walk
of the page's included nodes when a dialog opens or closes while an assistive
technology is active.

## Consumer and coverage

GPUI's Linux backend `gpui-pre-linux` 0.3.4 creates an `accesskit_unix` 0.22.1
adapter for each X11 or Wayland window; `accesskit_unix` serves AT-SPI
`Accessible.GetState` from this crate's `PlatformNode::state()`. The vendored
`gpui-base` patch already sets the AccessKit disabled property on disabled
Buttons; this patch lets that property reach AT-SPI. macOS uses
`accesskit_macos` and is unaffected, so it keeps the open off-screen item
above.

The Linux-only application test
`native_accessibility::atspi_state_tests` maps an AccessKit tree with enabled,
disabled and focused disabled Buttons, a disabled Switch, a disabled
CheckBox, a disabled text input and an enabled read-only text input through this crate's adapter.
Its `a_page_shown_again_after_a_dialog_puts_everything_it_holds_on_the_bus`
feeds the adapter a page's frames around a dialog, with a recording callback
in place of `accesskit_unix`'s bus: a control that changes as the dialog opens
and a line that appears beneath it are on the bus after Escape, nothing the
page holds is while the dialog covers it, and its live status is not
announced again. Its `a_node_outside_its_clip_is_visible_but_not_showing`
maps a clipping scroll view whose children lie inside, across its edge,
wholly past it (kept by the filter) and far past it with focus, and a
control with no clipping ancestor: the two wholly outside are `Visible`
without `Showing` at their own extents, the others show. Its
`a_node_outside_an_outer_clip_is_not_showing` maps an inner clipping node
kept past an outer one's fold, with a row inside it: both are `Visible`
without `Showing`. Its `a_point_query_passes_over_a_node_its_clip_leaves_out`
asks for the point a control kept past a body's top fold shares with the
dialog's title, and gets the title, and its
`a_point_query_passes_over_a_control_where_its_clip_hides_it` asks for the
title's pixels over a control that fold cuts, and gets the title, and for
the control's drawn pixels, and gets the control. In the
application, `github_view::tests::the_notice_reports_the_part_the_panel_draws`
reads the GitHub panel's notice through the adapter at 13 and 18 pt in a
461 x 490 window and at 13 pt in 1,480 x 980 (showing where the body draws
it, `Visible` without `Showing` at its layout's extents below the fold at
18 pt), and `views::tests::message_pieces_report_the_part_the_details_draw`
the inspector's commit message, whose pieces, each drawn at least in part,
show at the part drawn. Run
`cargo test --locked -p gitturtle --bin gitturtle native_accessibility::atspi_state_tests`
on Linux. Native AT-SPI reads are recorded in `docs/validation.md`.

## Removal

The disabled-controls change can go when GitTurtle moves to a GPUI Kit and
`gpui-pre` release whose Linux backend depends on `accesskit_unix` 0.24 or
later (which requires `accesskit_atspi_common` 0.21 or later), or when an
upstream 0.19.x release carries #788. The `src/adapter.rs` change can go once
the adapter GitTurtle uses handles a node that moves between `ExcludeNode` and
`ExcludeSubtree`; until then carry it onto the newer source. The off-screen
change can go once the adapter GitTurtle uses withholds `Showing` from a node
wholly outside a clipping ancestor, or AccessKit offers an off-screen
property that GitTurtle sets instead; until then carry it too. The
point-query change can go only once AccessKit's own hit test passes over a
node its clips leave out at the queried point, not with the off-screen
change: an adapter that withholds `Showing` can still answer a point query
with a node nothing is drawn of there. Retire this patch, the
`[patch.crates-io]` entry, the workspace exclusion and the `vendor/AGENTS.md`
row when no change remains. Keep the application tests; they then cover the
upstream crate.
