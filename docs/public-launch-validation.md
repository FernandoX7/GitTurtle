# Public-launch native and package validation

September 14, 2026. Application commit `7d18fef` adds the Linux Menu entry,
shared shortcut metadata and searchable help. Package/notice/CI commit `326fa0f`
retains third-party notices and repairs the workflow's Git configuration setup.
The repository remains private; no release was published.

## Source and environment

The release executable is SHA-256
`276d3fa2e8fe259fc30c395ddea60b39da53f388714adb36d34360a35cc3e996`.
The [source identity record](benchmarks/public-launch-20260914/source-identity.json)
contains 900 source, dependency, manifest and asset hashes, plus the local archive
hash. The build used the in-progress application changes on `3c4fa51`, subsequently
committed in `7d18fef`. Only test assertions and a comment changed after this
release build: GPUI's `Some(false)` means a complete keystroke match, and a new
negative assertion rejects the old binding. Reversing the recorded
[test-only delta](benchmarks/public-launch-20260914/tested-source-delta.patch)
reconstructs the exact compiled source hash. Production code is unchanged; the
final test source was checked by the workspace gates below.

This run reused the previously provisioned Ubuntu Base 24.04.5 x86-64 userspace,
Rust 1.98.0 and Cargo build caches, under the Pop!_OS host kernel. It is an
incremental release build. The earlier [clean build and runtime-only installation
record](benchmarks/linux-ubuntu-20260914/README.md) remains separate.

Native interaction used real GPUI windows in virtual X11/Xvfb with Openbox, and
native Wayland through Weston 13 nested in Xvfb at integer 2× scale. The fixture
was created by `scripts/create-demo-repo.py` at the neutral disposable path
`/launch-qa/Aurora`, with synthetic authors, eight commits and no remote URL.
Application preferences were isolated from the user's normal settings. Git
status remained clean; these checks edited only application drafts/preferences.

## Executed checks

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed; [log](benchmarks/public-launch-20260914/fmt.log) is empty on success. |
| `cargo check --locked -p gitturtle` | Passed; [log](benchmarks/public-launch-20260914/check.log). |
| `cargo build --release --locked -p gitturtle` | Passed; [log](benchmarks/public-launch-20260914/release.log). |
| `cargo test --locked --workspace` | Passed; five intentional ignored tests; [complete log](benchmarks/public-launch-20260914/tests.log). |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed; [log](benchmarks/public-launch-20260914/clippy.log). |
| Python syntax and both packaging scripts' Bash syntax | Passed. |
| Quality workflow, actionlint 1.7.12 | Passed after replacing the invalid job-level `runner.temp` expression. |
| Community Markdown/YAML, links and final whitespace diff | Passed local rendering/parser/link checks. |

The shortcut/palette regressions cover duplicate bindings, context-scoped input
behavior, platform modifier labels, normalized `?` events, bounded search,
contextual command availability and single activation. A new test initially used
the wrong match-result assertion; it was corrected before the final passing run.

## Native results

| Interaction | Observed result |
| --- | --- |
| Linux Menu by mouse and F10 | Visible anchored menu, platform shortcut labels and keyboard selection. F10 → Up → Up → Return opens Keyboard Shortcuts. |
| Ctrl+P and Ctrl+Shift+P while Menu is open | Quick Open and Command Palette open through the existing command handlers; the menu closes. |
| Keyboard Shortcuts | Physical Ctrl+Shift+/ produces Ctrl+? and opens help; immediate search input works. `editing` finds seven entries, `tabs` finds fourteen; unmatched queries show the empty state. Arrow browsing and Escape work. |
| Focus and draft retention | Ctrl+A replaces the focused commit title; F10/Escape and help search/Escape each return focus to it. Subsequent typing produces exactly `Launch draft retained safely`, retained in saved preferences. |
| History/Compare context | Commit selection stays in History; explicit file activation enters Compare. Menu/Escape retains Compare and the selected file. |
| Disabled commands | Closing the last repository disables repository-only menu entries while application commands remain available. |
| Appearance | Dark comfortable 13-point view at 1480×800 and light compact 18-point view at 1000×680 remain readable; the help list scrolls within the window. Enlarged settings were preloaded into isolated QA preferences. |
| Wayland 2× | Menu and searchable help render and accept input. Maximize changes the control to Restore; Restore changes it back to Maximize. Close terminates the app. [Maximized tree](benchmarks/public-launch-20260914/wayland-maximized-tree.txt), [restored tree](benchmarks/public-launch-20260914/wayland-restored-tree.txt). |

Native iteration corrected menu focus capture, shortcut dispatch while the menu
is open, transient popup spacing, and the help binding's shifted punctuation.
An X11 accessibility-bus connection later expired; the last draft and keyboard
navigation checks used native keyboard input, direct framebuffer capture and
saved preferences instead. Initial nested-Wayland setup attempts hit an absent
helper and stale display/socket; a fresh display/session established the successful
run. Portal warnings in the container are not evidence of working directory selection.

The README images are actual client-area framebuffer captures, without compositing
or invented product content. The supporting Wayland image retains the full virtual
display. All new images were visually inspected and contain only fixture content.

| Evidence | Scope |
| --- | --- |
| [History](screenshots/public-launch/history.png) | README hero, 1480×800, dark X11. |
| [Keyboard Shortcuts](screenshots/public-launch/keyboard-shortcuts.png) | README help view, 1480×800, dark X11. |
| [Enlarged Menu](screenshots/public-launch/light-large-narrow-menu.png) and [help search](screenshots/public-launch/light-large-narrow-shortcuts.png) | Light, 18-point UI, 1000×680. |
| [Retained draft](screenshots/public-launch/draft-restored.png) | Title text and editor focus after both transient views. |
| [No repository](screenshots/public-launch/no-repository-menu.png) | Disabled repository commands. |
| [Wayland help](screenshots/public-launch/wayland-scale2-help.png) | Weston integer 2×, full virtual display. |

## Package and notice checks

The local Linux archive was built with
`scripts/package-linux.sh --no-build --binary PATH OUTPUT`, reusing the identified
release executable. Its [packaging log](benchmarks/public-launch-20260914/package-final.log)
records 692 target-filtered dependency records and the two known notice gaps.
All 1,229 archive checksums passed, including the notice inventory and required
MPL source archive. Inventory records contain no personal home paths.

The archive was extracted and moved to a path containing spaces, then installed
with disposable `HOME` and `XDG_DATA_HOME` values in Ubuntu. All **1,216 license
files** matched the installed bytes. Reinstallation and desktop-entry validation
passed. Corrupting a notice or adding an unchecked notice correctly failed before
replacing the executable. Removing the extracted bundle preserved the executable
and every installed notice. No real user installation was modified.

The collector's strict `--require-complete` check correctly rejects unresolved
notices. Linux has two remaining package notice gaps; a separately generated
macOS inventory has six. Normal local packages include `REVIEW_REQUIRED.md`;
public binaries require resolving that target's entries. See
[third-party notices](../THIRD_PARTY_NOTICES.md) and the
[publication checklist](public-launch.md). Generating a macOS inventory on Linux
and checking shell syntax do not establish a native macOS package pass.

## Coverage still pending

This run does not establish actual Ubuntu GNOME desktop integration, physical
Pop!_OS interaction, fractional/mixed-monitor scaling, a screen-reader usability
pass, successful native folder selection, or macOS native/menu/package behavior.
The host desktop tooling could not provide sufficient physical-window access;
the user's existing app and accessibility settings were left unchanged. Earlier
platform evidence remains tied to its own source/build.

Hosted run [34881050919](https://github.com/FernandoX7/GitTurtle/actions/runs/34881050919)
at `326fa0f` started both Ubuntu and macOS jobs after the workflow correction;
their final results were pending when recorded. This proves the prior workflow
validation failure is resolved, not that hosted platform checks passed.
