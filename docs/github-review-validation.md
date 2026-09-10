# GitHub review conversation validation

This record covers the September 10, 2026 GitHub conversation work. Feature
semantics live in [GitHub collaboration](github-collaboration.md). Distribution,
notarization, installers, and automatic updates remain deferred.

## Baseline and live fixture

The baseline was inspected before implementation using a locked release rebuild
of `804dbf84365538fd6d00855e5900aceccbc53e97`, packaged locally with executable UUID
`2475FAA0-21B2-398D-9F9C-A2D57C92543A` (arm64). Native screenshots cover panel
entry, populated Overview, changed files, patch ranges, and immediate Return-to-
composer typing. Range selection, composer visibility, and Escape worked.
Conversation status/actions were absent. Native live inspection also exposed a
one-line multiline editor and no return from PR creation; the implementation
adds usable editor height, a creation Back action, and consistent outcome notices.

The active account was independently verified as `FernandoX7` before creating
the authorized private [test repository](https://github.com/FernandoX7/gitturtle-review-test).
Its files and commits are synthetic. All hosted mutations remain in that
repository, which is intentionally retained with its branches and test objects.
No GitTurtle source, credentials, or personal repository content was uploaded.

Baseline live evidence:

- Native Connect handed off the existing CLI authorization, verified the account,
  and reported successful macOS Keychain storage. After process restart, explicit
  Refresh PRs succeeded without reconnecting. No new OAuth login was needed.
- Native creation produced [draft PR #1](https://github.com/FernandoX7/gitturtle-review-test/pull/1).
  The provider independently confirmed draft status and captured head
  `fd43d67d12546fb0e45a6b44f5d5c572f2c3425c` against base
  `502fcfbaac8c28a6e1fa59624dfa9179973895a3`.
- Native file inspection, range selection, collection, confirmation and submission
  produced [Comment review 5168534415](https://github.com/FernandoX7/gitturtle-review-test/pull/1#pullrequestreview-5168534415)
  and [inline comment 3980272090](https://github.com/FernandoX7/gitturtle-review-test/pull/1#discussion_r3980272090).
  Independent REST inspection confirmed exact text, `RIGHT` side, lines 7–8,
  path `src/review.txt`, and the captured head.
- A separate native [discussion comment](https://github.com/FernandoX7/gitturtle-review-test/pull/1#issuecomment-5620530604)
  was independently confirmed. The authoritative GraphQL schema was read against
  its live review thread and matched the implemented identity/permission model.
- GitHub refused the author's approval submission; the app retained the text and
  surfaced refusal without retry. Successful Approve and Request changes require
  another eligible reviewer account; this account authored the test PRs.

## Integrated validation

At implementation increment `81f1c23`, targeted GitHub checks passed with 57 tests,
no failures, and one pre-existing explicitly ignored CLI-isolation test. These
covered provider, storage, process, stateful offline fixture, and actual GPUI
lifecycle behavior; its locked release build also passed.

Integrated offline native validation used source
`c2a97f37a1eb2b8b3216b799a65bc478e510e034` in the local `ValidatedGitTurtle`
bundle, with executable UUID `B2B06EB5-C8DD-3807-B85D-AF7751703C07` and SHA-256
`7030458c5a52698ac3719f05b9c6600cfd3532078d93e706b2fb6457a86e2f1c`.
Its local screenshots and interaction observations establish:

- Exact multiline reply text, including Unicode and whitespace, survived section
  and file navigation, panel closure/reopening, repository-tab changes, and app
  restart. The composer retained its original conversation and account.
- Reply submission followed by explicit refresh displayed the fixture reply.
  Resolve, refresh, and reopen exercised authoritative state changes. Two
  conversation pages were inspected; the longer thread showed 20/23 comments,
  then 3/23 on continuation, then 23/23 after refreshing its opening page.
- Outdated conversations kept understandable original context. Missing opening
  context and refused permissions displayed readable discussions with disabled
  actions.
- Actual narrow/wide window resizing was checked in Ember with 13/12 UI/code
  sizes and Comfortable density, and Braden and Graphite with 17/15 sizes and
  Compact density. The reply composer and exact-target confirmation remained
  visible, and Keep editing restored composer focus.

Inspection found completed empty replies still counted in recovery; `4798afd`
removes those entries from the visible recovery list. It also found Tab inserted
indentation into prose composers; `f07a9a9` makes Tab navigate without changing
text. Focused GPUI tests cover both fixes. These later increments are separate
from the native executable identity above.

## Integrated live validation

Live native validation used source `4798afd` in the local `LiveGitTurtle` bundle,
with executable UUID `5EA6F1F0-2246-397C-BFBD-189236D7BB94` and SHA-256
`4ba645c9557e361069ca805ad383e1093e1b1164ebafc09fe7260361350a009f`.
Supported Keychain access became available after the earlier access prompt.
Explicit Refresh used the persisted `FernandoX7` account. The following results
were observed in the native app and independently checked against GitHub:

- A reply draft containing exact Unicode and whitespace survived process restart
  and explicit PR selection. Native Reply → Send → explicit Refresh posted
  [reply 3980651763](https://github.com/FernandoX7/gitturtle-review-test/pull/1#discussion_r3980651763).
  GraphQL confirmed the exact body, opening comment `3980272090`, and thread
  `PRRT_kwDOUVI-ds6hHolY`.
- Native Resolve changed that thread's provider `isResolved` to `true`.
  After explicit refresh, native Reopen changed it back to `false`; both
  independent checks identified the same captured thread.
- Native creation produced [ready PR #2](https://github.com/FernandoX7/gitturtle-review-test/pull/2)
  from `codex/review-ready-20260910`. The API confirmed `draft: false`, title,
  description, branches, and unchanged head
  `fd43d67d12546fb0e45a6b44f5d5c572f2c3425c`.
- The synthetic PR #1 branch advanced to
  `db388b247eef8f3ac8443213429c921ae501af6a` through a one-file change to line 7.
  GitHub reported the original thread outdated with two comments. Native
  Refresh conversations refused the old head and disabled its actions. Disk
  inspection confirmed the exact unsent reply remained tied to the original
  `fd43d67d12546fb0e45a6b44f5d5c572f2c3425c` head, thread, and account node
  identities.

## Final interaction corrections

`fa7af40` prevents Return from closing the dialog during focused-button activation,
keeps refreshed PR rows visible alongside retained detail, and brings reply errors
into view beside the preserved composer. `1517753` makes account connection verify
the exact saved credential with normal macOS authorization and again without a
dialog; a successful Keychain update alone no longer reports a usable connection.
`951357c` preserves visible row heights in the bounded conversation, collected
comment, recovery and attempt regions. `891fc2f` restores visible-content focus
after a new PR head replaces the old reply, keeping Escape on the controlled
close and warm-recovery path.

Native inspection of `951357c` used `VerifiedGitTurtle`, UUID
`2833D353-EB55-313E-93B6-2BB3B6BC66FD`, SHA-256
`31d946583c92be1ee76d619a26773b1e07d6bf5d3424b3862651173574659ccd`.
At Graphite 17-point interface/15-point code with Compact density, Tab and Return
opened the exact reply confirmation, Keep editing restored unchanged text and
focus, and stale-head refusal displayed its warning beside the focused reply.
Refreshed PR rows remained visible and selectable. Nested recovery scrolling
reached the original live reply's Copy complete draft action; attempt rows stayed
visible within their own height limit. Explicit Back and warm reopening retained
the selected head, conversation context and recovery drawer.

After the live head moved, explicit inspection following restart displayed its
outdated range and original commit, offered the old draft through recovery, and
opened an empty reply for the new head. The old text was not remapped or posted.

## Final build and state restoration

Final code source `891fc2ffa0b55b2a3dfcd290c0cae2410de42ae1` passed:

- `cargo fmt --all -- --check`
- `cargo test --locked --workspace`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo build --release --locked -p gitturtle`
- Local packaging and strict ad-hoc signature verification.

The final workspace run reported no failures. Environment-dependent and opt-in
tests retain their explicit ignores; the installed-CLI isolation test was then
run explicitly and passed with its dummy credential and loopback fixture.
The existing `block v0.1.6` future-incompatibility warning remains.

The final local `AcceptedGitTurtle` bundle has executable UUID
`DF905BA5-5E75-3BF8-9921-920AA0C93088` (matching the release executable) and SHA-256
`6922cce933060c106c12c49ecc6dd120dd0c56dbe9914b483a409ca29f7dd12f`.
Native inspection confirmed that explicit selection of the moved fixture head
restored visible-content focus, Escape closed the panel, and warm reopening
retained the selected head and copy-only draft recovery.

On that same final executable, an initially refused stored Keychain read returned
an actionable error promptly. Explicit Connect subsequently verified the account
and secure readback, and Refresh PRs loaded both private test PRs. After quitting
and restarting the process, explicit Refresh succeeded without reconnecting.
PR #1 displayed the current `db388b2` head, authoritative unresolved/outdated
conversation, original range and commit, and the previously posted exact reply.
The recovery drawer retained its bounded, visible rows. These final checks
establish account persistence and UI corrections separately from the earlier
live mutation evidence.

All test app processes were stopped. The original activity, preferences and
repository-session files were restored byte-for-byte against their pre-test
SHA-256 hashes, including the original tabs, Ember theme, Comfortable density
and text sizes. The two QA-only GitHub stores were archived locally and removed
from the genuine app-data directory. The authorized GitTurtle Keychain connection
is retained; the installed application was not replaced.

Screenshots, command logs, build manifests, provider snapshots, QA drafts and
the restoration hash record are retained locally under the ignored
`.local/github-review-20260910/` directory. They were not uploaded or committed.
The final documentation commit changes no compiled inputs.

Native evidence is macOS-specific. Fixture success does not establish hosted
success; local commands do not establish hosted CI or native Linux coverage.
