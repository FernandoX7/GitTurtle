# AccessKit AT-SPI disabled state (2026-10-01)

Research for task `atspi-disabled-state` in [`tasks.json`](tasks.json), using the [research template](research-template.md).

## Decision and scope

- **Question:** how GitTurtle's disabled Buttons can report a disabled state to AT-SPI on Linux. On every build so far, a disabled Button reports `enabled` and `sensitive` ([validation, September 30](../validation.md#september-30-tab-leaves-a-focused-button-that-turns-disabled)). A screen-reader user cannot tell it from an enabled one.
- **Recommendation:** option (a). Vendor the published `accesskit_atspi_common` 0.19.1 with only the upstream #788 change, select it through `[patch.crates-io]`, and retire the patch when the toolkit moves to `accesskit_unix` 0.24 or later. No released GPUI stack carries the fix, and the backport is a +6/-4 change to one function. Its only new call, `is_read_only()`, already exists in the locked `accesskit_consumer` 0.38.0. Nothing else in the dependency graph changes.
- **Re-check:** the planner's facts of 2026-09-30 in the task contract hold on 2026-10-01; the evidence below adds exact requirements and revisions.
- **Outside this decision:** upgrading GPUI Kit or `gpui-pre`, `deny.toml`, the macOS adapter (`accesskit_macos`, VoiceOver), Windows, and Orca speech.

## Evidence

| Source | Checked date/version | Relevant finding | Limit |
| --- | --- | --- | --- |
| crates.io API, `gpui-pre-linux` versions and dependencies | 2026-10-01; 0.3.4 to 0.3.7 | 0.3.7 (2026-09-28) is the newest, and 0.3.5, 0.3.6 and 0.3.7 all require `accesskit ^0.24.0` and `accesskit_unix ^0.22` on Linux and FreeBSD. `gpui-pre` 0.3.7 requires `accesskit ^0.24.0`. | Registry metadata only; the GPUI repository's unreleased branches were not checked. |
| crates.io API, `gpui-kit` | 2026-10-01; 0.6.0 to 0.7.0 | 0.7.0 (2026-09-28) is the newest and pins `gpui-pre =0.3.7`. 0.6.6 pins `=0.3.6`. GitTurtle pins `gpui-kit =0.6.0` on `gpui-pre` 0.3.4. | Upgrading the kit would also be a separate decision; it would not bring the fix. |
| crates.io API, `accesskit_unix` and `accesskit_atspi_common` | 2026-10-01 | `accesskit_unix` 0.24.0 (2026-09-25) requires `accesskit ^0.25.1` and `accesskit_atspi_common ^0.21.0`; 0.23.0 requires `^0.20.0`. `accesskit_atspi_common` 0.21.0 requires `accesskit ^0.25.1` and `accesskit_consumer ^0.39.1`; 0.19.1 requires `accesskit ^0.24.1` and `accesskit_consumer ^0.38.0`. | Semver ranges, not a build. |
| [AccessKit commit `6ee0558b`](https://github.com/AccessKit/accesskit/commit/6ee0558b6315b3ef1594db24ce45a030ecac7cb5) (#788) | 2026-10-01; commit dated 2026-09-01 | "fix: Expose non-disabled nodes as enabled and sensitive on Unix": +6/-4 in `adapters/atspi-common/src/node.rs`, `NodeWrapper::state`. A disabled node gets none of `Enabled`, `Sensitive` or `ReadOnly`; otherwise `ReadOnly` only when the role supports it and `is_read_only()`, else `Enabled \| Sensitive`. No test in the commit. | Upstream review of the change is theirs; its effect here is checked by the test below. |
| GitHub compare API from `6ee0558b` | 2026-10-01 | Tag `accesskit_atspi_common-v0.21.0` is 18 commits ahead of the fix (contains it); `-v0.20.0` is 2 and `-v0.19.1` 29 commits behind (neither contains it). | Tags as published on GitHub. |
| Published `accesskit_atspi_common` 0.19.1 archive | 2026-10-01; sha256 `023da0e5…d636`, same as `Cargo.lock` | `.cargo_vcs_info.json` names `c88605b96d04431f9c3c792464a0f2f253480e94`, path `platforms/atspi-common`, which is the commit tag `-v0.19.1` points at. `src/node.rs:373-377` holds the pre-fix branch, and the #788 diff applies to it unchanged at a 3-line offset. The archive has no license files. | The crate moved to `adapters/atspi-common` upstream after 0.19.1. |
| `accesskit_consumer` 0.38.0 `src/node.rs` | 2026-10-01; locked version | `is_disabled` (448), `is_read_only` (452, true for any role that cannot be read-only), `is_read_only_or_disabled` (461) and `is_read_only_supported` (844), whose list includes Switch and CheckBox but not Button. | — |
| `accesskit_unix` 0.22.1 `src/atspi/interfaces/accessible.rs:129`; `gpui-pre-linux` 0.3.4 `src/linux/x11/window.rs:1956`, `wayland/window.rs:2126` | 2026-10-01; locked versions | GPUI creates an `accesskit_unix::Adapter` per window, and AT-SPI `GetState` returns `PlatformNode::state()` from this crate. | — |
| License texts at `c88605b9` (`LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE.chromium`) | 2026-10-01 | SHA-256 `23f18e03…`, `62c7a1e3…` and `845022e0…` match the supplemental notices already in [`docs/licenses/dependencies.json`](../licenses/dependencies.json). | — |
| Local test, base and candidate | 2026-10-01 | See Outcome. | Adapter state sets, not a native AT-SPI read. |

## Compatibility and alternatives

- **Pinned stack:** Rust 1.98, GPUI Kit 0.6.0, `gpui-pre` 0.3.4, `accesskit` 0.24.1, `accesskit_consumer` 0.38.0, `accesskit_unix` 0.22.1, `accesskit_atspi_common` 0.19.1 and `atspi-common` 0.13.0. The vendored crate keeps its manifest, so `Cargo.lock` keeps every version and loses only the registry source and checksum of this one package.
- **(a) Vendor 0.19.1 with the #788 backport.** It takes effect now, keeps the toolkit dependency set, and follows the four existing vendor patches. Cost: one more vendored crate to retire. Upstream code and licenses (MIT OR Apache-2.0, with Chromium BSD notices) are kept as published.
- **(b) Ask AccessKit for a 0.19.x backport release.** It would remove the vendored copy, but it is outward-facing, so only the owner can make the request, and nothing guarantees a release. It is the owner's option, and it can run alongside (a): a 0.19.2 that carries #788 is also a removal condition.
- **(c) Wait for a `gpui-pre` release on `accesskit` 0.25.** As of 2026-10-01 none exists: 0.3.7, released three days after `accesskit_unix` 0.24.0, still requires `^0.22`. Taking it would also mean moving GPUI Kit, which is a separate decision. Until then, disabled Buttons stay wrong on Linux.
- **Removing or bypassing the toolkit adapter:** out of proportion and against the pinned-kit rule.
- **Could change the recommendation:** a `gpui-pre` release on `accesskit_unix` 0.24 or later that fits the kit, or an upstream 0.19.x release with #788.

## Enforcement and acceptance

- **Owners:** `vendor/accesskit_atspi_common/GITTURTLE-PATCH.md` records provenance, the change, the consuming path and the removal condition. `vendor/AGENTS.md` lists it, `THIRD_PARTY_NOTICES.md` names its license and patch record, and `Cargo.toml` selects and excludes it.
- **Regression:** `native_accessibility::atspi_state_tests` is a Linux-only application test, so every Linux run of the app's tests, Ubuntu CI included, runs it. It maps an AccessKit tree through the vendored adapter's `PlatformNode::state()`.
- **Native:** an AT-SPI read of base and candidate on Linux X11 on 2026-10-01, recorded in [`docs/validation.md`](../validation.md#october-1-disabled-buttons-report-disabled-to-at-spi). It covered disabled Buttons, not a disabled text input.
- **Dependency:** `cargo deny --locked check` passed (advisories, bans, licenses and sources), and the independent [security review](security-review.md) passed on 2026-10-01.
- **Remaining uncertainty:** whether Orca speaks the new state as intended (not covered), and VoiceOver, which this change does not touch.

## Outcome

Adopted option (a) on 2026-10-01 from base `151647a`. The vendored source is the 0.19.1 archive above plus upstream `6ee0558b` applied to `src/node.rs` only, and the three license files from `c88605b9`.

`cargo test --locked -p gitturtle --bin gitturtle native_accessibility::atspi_state_tests` on Ubuntu 26.04:
- with the registry crate (no `[patch.crates-io]` entry), it fails at its first disabled node: "disabled Button reports Enabled", state set `Enabled | Sensitive | Showing | Visible`;
- with the vendored backport, it passes.
