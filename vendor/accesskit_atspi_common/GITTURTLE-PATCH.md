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

## Change

`src/node.rs` is the only modified file. It carries the upstream commit
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
in the locked `accesskit_consumer` 0.38.0. No other source, manifest or
dependency changes.

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
CheckBox, a disabled text input and an enabled read-only text input through this crate's adapter. Run
`cargo test --locked -p gitturtle --bin gitturtle native_accessibility::atspi_state_tests`
on Linux. Native AT-SPI reads are recorded in `docs/validation.md`.

## Removal

Retire this patch, the `[patch.crates-io]` entry, the workspace exclusion and
the `vendor/AGENTS.md` row when GitTurtle moves to a GPUI Kit and `gpui-pre`
release whose Linux backend depends on `accesskit_unix` 0.24 or later (which
requires `accesskit_atspi_common` 0.21 or later), or when an upstream 0.19.x
release carries #788. Keep the application test; it then covers the upstream
crate.
