# GitTurtle patch to accesskit_unix 0.22.1

This directory contains the crates.io `accesskit_unix` 0.22.1 source,
licensed MIT OR Apache-2.0. The registry archive checksum is
`03e156ed3802e35eefe894ef2671bc6c889303d8a7e110b5e1b48f504b91362f`, the one
`Cargo.lock` recorded for it; `.cargo_vcs_info.json` records the published
source revision `c88605b96d04431f9c3c792464a0f2f253480e94` (tag
`accesskit_unix-v0.22.1`, path `platforms/unix`), the revision of the
vendored `accesskit_atspi_common` 0.19.1. Upstream:
https://github.com/AccessKit/accesskit

The published archive omits the license files its source headers name.
`LICENSE-MIT` and `LICENSE-APACHE` are copied unchanged from the repository
root at the same source revision (the files `vendor/accesskit_atspi_common`
carries); their SHA-256 digests match the supplemental notices in
`docs/licenses/dependencies.json`. No source in this crate names
`LICENSE.chromium`.

## Changes

One source file is modified, `src/atspi/bus.rs`, and the manifest gains a
test-only dependency. No other source or runtime dependency changes.

### Cache signals libatspi accepts (`src/atspi/bus.rs`)

`emit_cache_signal` carries the upstream commit
`bffadacde348b0b73d3d7201bfb0b7f69a54b0bf`, "fix: Preserve cache signal
structure arguments on Unix" (AccessKit/accesskit#819, merged 2026-10-11),
first released in `accesskit_unix` 0.24.1 (2026-10-25). GitHub's comparison
of the tags `accesskit_unix-v0.24.0` and `accesskit_unix-v0.24.1`, read on
2026-11-05, changes `adapters/unix/src/atspi/bus.rs` by this commit alone;
the release's other Unix change, #821 (adapter lists sorted by ID, in
`src/context.rs`), is not taken. The commit sends the cache signal's body as
a one-element tuple, `&(body,)`, with a `zvariant::Type` bound in place of
`DynamicType`.

The cache's `AddAccessible` and `RemoveAccessible` signals each carry one
structure. zbus 5.19 (`src/message/fields.rs:75-95`) writes a structure
body's signature without its parentheses, sending the structure's fields as
the message's arguments, so upstream 0.22.1 sent `RemoveAccessible` as `so`
and `AddAccessible` as `(so)(so)(so)iiassusau`. libatspi 2.60.6
(`atspi/atspi-misc.c`, tag `2.60.6`: `handle_remove_accessible`, lines
505-527, and the cache signal types at 879-891) accepts only `(so)` and
`((so)(so)(so)iiassusau)` (or the old `((so)(so)(so)a(so)assusau)`) and
drops anything else with a warning: Orca's stderr in the 2026-11-04 QA run
held 257 "Unknown signature so for RemoveAccessible" and 318 "AddAccessible
with unknown signature" lines. libatspi's cache therefore never learned of an
object GitTurtle added or removed.

With the backport libatspi takes both. On `AddAccessible` it caches the
object's name, role, description, parent, index in its parent, child count,
interfaces and states from the item `accesskit_atspi_common` builds; on
`RemoveAccessible` it disposes the object's proxy. A disposed proxy has no
application (`atspi/atspi-object.c`, `atspi_object_dispose`), so libatspi
answers nothing about it from its cache (`atspi/atspi-accessible.c:1876-1881`)
and fails every call with "The application no longer exists"
(`atspi/atspi-misc.c:1227-1234`), which Orca 51.1 takes as dead.

A GitTurtle adjustment: `emit_cache_signal` is a free function that takes
the connection, called with `&self.conn` by `emit_cache_add` and
`emit_cache_remove`, so a test can emit on a peer-to-peer connection.

### Test-only dependency (`Cargo.toml`)

`[dev-dependencies]` adds `zbus` 5.19 with its `async-io` and `p2p`
features, for the test's peer-to-peer connection. `Cargo.toml.orig` is
unchanged. As an excluded workspace member's dev-dependency it never enters
the application's dependency graph or `Cargo.lock`.

## Interaction with `accesskit_atspi_common`

The vendored `accesskit_atspi_common` keeps a selectable item that leaves the
tree on the bus, answering the parent it had, so that Orca, which last spoke
it, finds the list the item and the next one share and reads no window,
dialog or list before the next one. With this backport a `RemoveAccessible`
for such an item would dispose libatspi's proxy, Orca would take the item as
dead and read the dialog and the list from the window down. That crate
therefore withholds the cache's `RemoveAccessible` for an item it keeps and
sends it once the item leaves the bus
([its patch note](../accesskit_atspi_common/GITTURTLE-PATCH.md)).

## Consumer and coverage

GPUI's Linux backend `gpui-pre-linux` 0.3.4 depends on `accesskit_unix`
0.22 and creates one `accesskit_unix::Adapter` for each X11 or Wayland
window (`src/linux/x11/window.rs`, `src/linux/wayland/window.rs`); every
object it adds to or removes from the AT-SPI tree goes through
`emit_cache_add` and `emit_cache_remove`. macOS uses `accesskit_macos` and
is unaffected.

`atspi::bus::tests::cache_signals_carry_the_signatures_libatspi_accepts`
emits both signals, the `AddAccessible` item built for a window node by the
crate's own `cache_item_for_node`, over a peer-to-peer connection and reads
the body signature from the received message's header bytes (zbus parses `so`
and `(so)` alike): `((so)(so)(so)iiassusau)` and `(so)`. Run it with
`scripts/vendor-crate-test.sh vendor/accesskit_unix`. The application's
`native_accessibility::atspi_state_tests::Bus` models libatspi disposing a
proxy on the cache's `RemoveAccessible`, and the Keyboard Shortcuts and
command palette tests named in `accesskit_atspi_common`'s patch note hear
the result through Orca's model. Native Orca checks are recorded in
`docs/validation.md`.

## Removal

Remove this directory, its `[patch.crates-io]` entry, its workspace
exclusion and its `vendor/AGENTS.md` row when GitTurtle's GPUI Linux backend
depends on `accesskit_unix` 0.24.1 or later, which carries #819. That
release requires `accesskit_atspi_common` 0.21 or later, so check the
vendored `accesskit_atspi_common` changes against it in the same upgrade.
