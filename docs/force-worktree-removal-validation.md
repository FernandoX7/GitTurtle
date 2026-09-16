# September 16 force-worktree removal review

PR [#15](https://github.com/FernandoX7/GitTurtle/pull/15) was reviewed from
`9f18fa4de7cb89c5afc949fc439ca5541084971a` and integrated with main
`7c9dc6ca451104d653d6fedb28d0b3f9dfdb8172`. This record applies to clean
application source `c3e4a0f1a75af0f6c6b9dd177a2bbd31c87d9b61`; subsequent
evidence-only commits do not change the exercised implementation.

## Changes and independent review

The review reproduced loss of nested bare repositories, ignored content changed
after confirmation, and changes concealed by assume-unchanged or skip-worktree
index flags. Removal now requires a complete bounded, no-follow inventory and
content snapshot, revalidates it before dispatch, and refuses hidden index
flags, nested repositories, initialized or retained submodule repositories,
special files, unreadable entries and inspection-limit failures. The final
independent review also found the case-sensitive root `.GIT` exemption; the
fix exempts only exact root `.git`, with preservation regressions for variants.

The confirmation keeps the loss warning, exact target and actions visible while
the effects scroll. It separates deleted content from retained history, makes
the full path copyable, supports keyboard scrolling and explains blocked force
actions. Destructive buttons use synchronized theme tokens with readable text;
disabled controls retain their disabled appearance. The warning uses a plain
tinted panel without a left accent border.

Independent correctness and security/preservation reviews found no remaining
actionable defects at `a0240dba51a4f8954053fab188c8f19896ffaee4`. The subsequent
two presentation changes remove the warning border and apply danger text only
to enabled manager actions. Both were compiled, tested and checked in the native
app as part of the final source below.

## Automated checks

On the final implementation source:

- `cargo fmt --all -- --check`: passed.
- `cargo test --locked --workspace`: **803 passed, zero failed, five existing
  explicit ignores**, plus one successful subprocess repeat for the renderer
  cache isolation test.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo build --locked -p gitturtle`: passed.
- The 26 worktree/reflog integration tests exercise stale content (including
  equal-size edits with restored timestamps), directory/admin replacement,
  nested regular and bare repositories, `.GIT` variants, retained submodules,
  hidden index flags, symlinks, special files and inspection bounds, alongside
  successful removal and unrelated-state preservation.
- Two snapshot helper tests cover deterministic resource/cancellation bounds.
  Fourteen consuming worktree UI tests cover disabled actions, cancellation,
  stale context, full path copying, overflow counts and keyboard scrolling.
  Layout checks include 1000×680 windows at 13- and 18-point interface text in
  Daylight and Midnight. Resolved normal/hover/active danger-button tokens meet
  4.5:1 label contrast in all ten themes.

Builds reused the local Cargo cache with Rust 1.98.0 and Git 2.43.0. The host had
the X11 runtime library but lacked its development linker alias; a temporary
`LIBRARY_PATH` containing that alias supplied linking without changing system
packages or repository defaults.

## Native checks

The actual GPUI application ran through `cargo run --locked -p gitturtle --
<disposable-fixture>` on Pop!_OS 24.04, x86-64, **virtual X11 under Xvfb**, scale
1, with lavapipe software Vulkan and isolated application preferences. The debug
executable reported the clean source above through `--build-info`; SHA-256:
`8175a35842215ff88ef48bd069bee688d98dab7af1c45f81d4b952a40d3155ee`.

The disposable fixture had a main checkout with unrelated staged and unstaged
changes, a linked target with 16 changed and 16 ignored files, a sibling with a
nested repository and unique commit, and a separate removable dirty target.
Pointer and keyboard interaction verified:

1. Daylight, compact density, 18-point interface text, 1000×680: warning,
   target, Cancel and Force remove remained visible at both ends of the effect
   list. End scrolling and pointer cancellation worked. The warning has no
   left accent border, and the destructive button label is readable.
2. Midnight, comfortable density, 13-point text, 1480×980: context-menu entry,
   Home/End and wheel scrolling exposed the bounded changed/ignored lists,
   overflow counts and retained-history section without moving the warning.
3. A nested repository left both removal controls disabled, with an explanatory
   message naming the affected path and how to preserve it. Clicking the
   disabled force action did not open a confirmation.
4. Editing an existing ignored file after opening confirmation caused the
   explicit Force remove action to refuse with “The worktree identity or
   content changed after review.” Every tracked/ignored byte survived.
5. A fresh review and explicit Force remove on the separate disposable target
   succeeded. Independent Git/filesystem checks verified the directory, private
   administration and registration disappeared, all branch refs and source
   HEAD remained unchanged, the source index stayed byte-identical, unrelated
   working files stayed identical, and the sibling's unique commit survived.

Screenshots: [light confirmation](screenshots/force-worktree-removal/light-confirmation.png),
[light scrolled](screenshots/force-worktree-removal/light-scrolled.png),
[dark confirmation](screenshots/force-worktree-removal/dark-confirmation.png),
[dark scrolled](screenshots/force-worktree-removal/dark-scrolled.png),
[blocked nested repository](screenshots/force-worktree-removal/nested-refusal.png),
[stale refusal](screenshots/force-worktree-removal/stale-refusal.png), and
[successful removal](screenshots/force-worktree-removal/removal-success.png).
The [machine-readable record](screenshots/force-worktree-removal/validation.json)
contains build identity, checks and screenshot digests.

## Evidence boundaries

This is native virtual-X11 evidence, not a physical Wayland display, screen-reader,
macOS, release-package or hosted-CI acceptance run. No performance improvement is
claimed from the debug build. The filesystem deadline is cooperative and cannot
interrupt an individual stalled syscall. As with Git's own preflight, another
same-user process can race the final check and Git's deletion; removal is not an
atomic filesystem transaction. Application preferences and all mutation
fixtures were isolated, and the user's existing app and repositories were left
untouched.
