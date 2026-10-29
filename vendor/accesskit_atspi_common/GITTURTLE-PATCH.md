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

Two files are modified: `src/node.rs` and `src/adapter.rs`. No other source,
manifest or dependency changes.

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
`accesskit_macos` and is unaffected.

The Linux-only application test
`native_accessibility::atspi_state_tests` maps an AccessKit tree with enabled,
disabled and focused disabled Buttons, a disabled Switch, a disabled
CheckBox, a disabled text input and an enabled read-only text input through this crate's adapter.
Its `a_page_shown_again_after_a_dialog_puts_everything_it_holds_on_the_bus`
feeds the adapter a page's frames around a dialog, with a recording callback
in place of `accesskit_unix`'s bus: a control that changes as the dialog opens
and a line that appears beneath it are on the bus after Escape, nothing the
page holds is while the dialog covers it, and its live status is not
announced again. Run
`cargo test --locked -p gitturtle --bin gitturtle native_accessibility::atspi_state_tests`
on Linux. Native AT-SPI reads are recorded in `docs/validation.md`.

## Removal

The `src/node.rs` change can go when GitTurtle moves to a GPUI Kit and
`gpui-pre` release whose Linux backend depends on `accesskit_unix` 0.24 or
later (which requires `accesskit_atspi_common` 0.21 or later), or when an
upstream 0.19.x release carries #788. The `src/adapter.rs` change can go once
the adapter GitTurtle uses handles a node that moves between `ExcludeNode` and
`ExcludeSubtree`; until then carry it onto the newer source. Retire this
patch, the `[patch.crates-io]` entry, the workspace exclusion and the
`vendor/AGENTS.md` row when neither change remains. Keep the application
tests; they then cover the upstream crate.
