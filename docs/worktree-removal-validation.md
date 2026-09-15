# Worktree removal validation

September 14, 2026. Core implementation `95d97a7`; native actions and regressions
`9585271`. This record covers registered linked-worktree removal only.

## Build and checks

Rust 1.98.0, Git 2.43.0, Linux x86-64 on Pop!_OS 24.04. The actual GPUI app ran
on an isolated Xvfb display with Openbox at 1480×900, with separate application
preferences. This was a debug source build, not a package or performance test.
The [build identity](evidence/worktree-removal/build-identity.json) records the
executable hash and source input digest. Embedded metadata reports modified
`c188020`, captured when the build started; runtime source inputs were verified
unchanged against the committed implementation. The new test-only module was
excluded from that native input digest because it was untracked at capture time.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo check --locked -p gitturtle` | Passed. |
| `cargo test --locked -p gitturtle-core --test worktrees_reflog` | All 12 fixtures passed. |
| Core cleanup-verification unit fixture | Passed: registration, folder, private metadata and dangling symlink leftovers reject success. |
| `cargo test --locked -p gitturtle worktrees::tests` | All four GPUI fixtures passed. |
| `cargo test --locked --workspace` | 743 passed; five intentionally ignored. |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed. |
| `cargo build --locked -p gitturtle` | Passed; this executable was used below. |
| `git diff --check` | Passed. |

Initial UI fixtures lacked debug selectors; adding selectors and dispatching
native dialog actions corrected the harness. The initial sandboxed workspace
run failed existing socket/process tests, including `Operation not permitted`
from askpass sockets. The complete final run outside that sandbox passed.

## Native interactions and Git results

The demo generator created disposable Aurora history and a locked worktree.
Additional clean and dirty worktrees used local branches; no remote was used.
A globally configured LFS smudge filter rejected the first fixture checkout;
Git rolled that checkout back and retained its branch. The fixture was then
created with `GIT_LFS_SKIP_SMUDGE=1`. Product Git policy was unchanged.

- Right-click exposed **Worktree actions…** and **Remove worktree…** without
  changing the current repository. Shift+F10 on the keyboard-focused worktree
  opened its manager with that exact row selected.
- Dirty, main and locked targets displayed their protection reason with removal
  disabled. The current repository and History selection remained visible.
- A clean target opened a confirmation showing its folder, branch and commit.
  Cancel kept its folder, registration and private metadata intact.
- Adding an untracked file after confirmation opened caused confirmation to
  fail with a stale-review message. Its bytes and metadata remained intact.
- After moving that test file outside the target and explicitly reviewing again,
  confirmation removed the clean folder, registration and private Git directory.
  The navigator count fell from four to three and showed a success notice.
- Independent [Git/filesystem assertions](evidence/worktree-removal/git-results.json)
  verified source HEAD/index and all refs were preserved, including the removed
  worktree's branch. Dirty sibling content, the locked sibling and the late test
  file were preserved. Core fixtures additionally verify exact source/sibling
  index, private configuration, HEAD/reflog and working bytes.

These unedited native captures use a synthetic Git identity set before capture:
[context menu](evidence/worktree-removal/context-menu.png) and
[protected target](evidence/worktree-removal/protected-worktree.png). Screenshots
were visually inspected. The QA app was closed after validation; normal user
preferences and repositories were untouched.

## Limits

macOS runtime, native Wayland and hosted CI were not exercised by this check.
Initialized submodule refusal and hidden index flags were verified in core
fixtures; they are not claimed as separately exercised native interactions.
Sparse worktrees with skip-worktree entries require external Git review.


## September 15 maintainer review of PR #5

The revised application was exercised from clean source
`3b0a958ed0533af816fb015fabfc374ba2e055db`. Its Linux x86-64 release executable
SHA-256 is `39cbefa6f13c689c7039da14f210b5768ad30de87dcc714564b310f194e2a767`;
embedded build metadata identifies Rust 1.98.0 and release profile. Subsequent
validation-record edits do not change this executable's source identity.

### Review fixes and automated validation

Removal now refreshes the selected manager row before confirmation and rejects
cancelled or obsolete asynchronous reviews. Core revalidation also detects
replacement checkout/admin directories, redirected private administration paths,
symlinked `.git` entries and ambiguous registrations. Regression fixtures cover
the reproduced replacement-directory and private-admin-symlink failures. Unix
fixtures retain byte-safe paths, with invalid UTF-8 pathname coverage confined to
Linux so the fixtures remain compatible with macOS filesystems. The existing
conservative refusal of assume-unchanged and skip-worktree entries remains.

The original Ubuntu CI failure was a draft-shutdown fixture timing race: real
filesystem saves could outlive GPUI's simulated quit polling. The recovery and
commit draft fixtures now synchronize shutdown with bounded worker completion,
while preserving the full save queue, removed window and exact final text
assertions. A controlled slow save reproduced the original failure and passed
after the change. Negative controls still failed when the quit observer was
removed or the wrong final text was saved. Production shutdown timing is unchanged.

On the exact source above, `cargo fmt --all -- --check`,
`cargo check --locked -p gitturtle`, `cargo test --locked --workspace`
(**775 passed, zero failed, five existing ignores**),
`cargo clippy --locked --workspace --all-targets -- -D warnings`, and
`cargo build --release --locked -p gitturtle` all passed. Workspace tests used
normal concurrency. Focused worktree coverage included nine GPUI tests,
fifteen core integration tests and two core unit tests.

### Native validation

The actual release application ran with isolated preferences on a nested Weston
Wayland compositor using software rendering, hosted by Xvfb/Openbox on Linux.
The disposable fixture contained main, clean, dirty, locked, assume-unchanged,
skip-worktree and unrelated worktrees. Native accessibility state and screenshots
were inspected after interaction.

- Keyboard navigation and Shift+F10 opened the exact worktree's manager without
  switching the current repository. The accessibility hint now describes both
  branch and worktree actions. Right-click exposed the removal action.
- Adding an ignored file after the manager's initial inspection made Remove
  refresh the details and display a disabled action with a protection reason.
- The confirmation displayed the target folder, branch and full commit ID.
  Cancel preserved the folder, registration and private administration directory.
- Adding an untracked file after confirmation opened caused a stale-review
  refusal. Its contents and worktree metadata survived.
- After preserving that file outside the target and explicitly reviewing again,
  confirmation removed only the clean checkout, its registration and its private
  administration directory. The navigator count changed from seven to six.
- Git/filesystem assertions verified unchanged main HEAD/index and all branch
  refs, including the removed worktree's branch. Every other registration, the
  locked state, hidden edits and dirty/unrelated contents survived.
- Main, dirty, locked, assume-unchanged and skip-worktree selections displayed
  their protection reasons with removal disabled. Escape dismissed the manager
  while retaining the current repository and history context.

The QA app closed normally, the temporary display servers stopped, and the two
accessibility flags were restored to their original values. The installed app
and its preferences were preserved. Raw captures and logs are local evidence;
they are not distribution artifacts.

### Scope

This run does not establish macOS native interaction or packaging, physical-GPU
behavior, performance, or exhaustive layout coverage. Hosted CI is recorded on
[PR #5](https://github.com/FernandoX7/GitTurtle/pull/5) separately. Preflight
identity checks do not make Git removal atomic against concurrent external
filesystem writers. No force removal or automatic metadata repair is attempted.
