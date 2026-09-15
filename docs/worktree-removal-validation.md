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
