# Native polish and pull-request review milestone

Started September 10, 2026 UTC (September 9 local), from clean `8e008bd`.
This record owns the finite acceptance list for the current task. The previous
[milestone](next-milestone.md) and its evidence retain their original identities.
The canonical product contract remains [DESIGN.md](../DESIGN.md).

## Acceptance

| ID | Required outcome | State and evidence |
| --- | --- | --- |
| P1 | Preserve installed bundle, complete genuine app state, original settings and artwork; inspect baseline before edits | Backed up before native interaction; identity below. Original system flags, VoiceOver off and Dark appearance restored after `28a5d43` checks. Complete genuine app data restored byte-identically and desktop preferences restored after the installed-app smoke check. |
| P2 | Coherent shared typography, icon geometry, focus/states, header hierarchy and progressive disclosure | Implemented; all ten native palettes in both densities inspected on `4ebe8fc`. Comparable final `eb3dd26` captures are in the [visual evidence](benchmarks/native-polish-20260910/visuals/README.md). |
| P3 | Deliberate overflow at minimum/ordinary/wide sizes, long Unicode names, enlarged text, both densities and eight tabs; preserve slim/deep graph | Native 1000 × 680, 1480 × 981 and 1604 × 780 exercised, including 18/24-point text. Corrected minimum History last-row/scrollbar separation passed on `28a5d43`. |
| P4 | Complete Projects, tabs/workspaces, History, Changes, conflicts/recovery, previews, Settings/palette and error/confirmation workflows with retained context | Native Projects, tabs/history, commit, conflict/recovery, previews and interruption checks recorded below. U15 restoration and exact staging passed on `cc4ffac`; U16 cold-history End and manual-scroll restart checks passed on `eb3dd26`. |
| P5 | Native PR discovery, captured overview/check/review identity, files, line/range composition and existing threads | Implemented and exercised through the offline native provider fixture on `4ebe8fc`; live GitHub remains unverified. |
| P6 | Collect/edit comments, deliberate captured review submission, durable unfinished drafts across navigation/restart, explicit moved-head/outdated recovery | Native navigation, restart recovery of both captured heads, exact collected/unfinished ranges and deliberate successful offline submission verified through `28a5d43`. Incompatible discussion-only mode normalizes to review mode when inline content restores. |
| P7 | Principal keyboard/AX semantics, focus containment/restoration, non-color cues, actual appearance/accessibility settings and controlled VoiceOver | Projects/PR/Markdown keyboard and focus checks passed. Native system appearance and Reduce Motion exercised; VoiceOver enabled for keyboard-only checks, without spoken-feedback verification. Prior spoken VoiceOver request remains pending. |
| P8 | Release measurements on affected paths and demanding fixtures; investigate reproducible stalls/peaks without unsupported speed/leak claims | Frozen `4ebe8fc` resource report published under `docs/benchmarks/native-polish-20260910`; initial Markdown peak separated from three matched small-companion cycles. No improvement or leak claim. |
| P9 | At least 20-minute release mixed native session, resource cleanup, external fixture changes, missing folder and restart recovery | A 21 min 58 sec mixed interval on `4ebe8fc` completed, followed by quiet settlement and normal Quit. Restart, slow-Fetch cancellation, missing-folder recovery and external Git-change checks completed on `28a5d43`; no sleep/wake claim. |
| P10 | Final fmt, workspace tests, strict all-target Clippy, release, available Linux checks and meaningful commits | macOS and available Linux formatting, workspace tests, strict all-target Clippy and release gates passed on final compiled source `eb3dd26`. Native Linux and hosted CI remain unverified. |
| P11 | Verified final plist, provenance, artwork, signature, UUID/SHA; install and launch exact `/Applications/GitTurtle.app`; restore genuine state/settings | Installed final `eb3dd26`, verified all five bundle files and strict signature, mapped running identity, and native Compare/Back/Settings. Genuine state and original system settings restored; details below. |

## Preservation and baseline

Private durable local evidence: `.local/native-polish-20260910` (ignored, mode 0700).
Raw records retain their original `/tmp/gitturtle-polish-20260910` recording paths.
Public fixture-only screenshots, sanitized measurements and final gate/installation
proof are in [the evidence directory](benchmarks/native-polish-20260910/README.md).
`baseline/` contains a `ditto` copy of the complete installed bundle and application
data directory, per-file SHA-256 manifest, original preference exports and build/
hardware identity. Genuine preferences, profiles/assignments, drafts, recent
repositories and pre-existing QA records are preserved as found. No resetting to
the earlier handoff, repository writes in unrelated repositories, or artwork
changes are part of the audit.

Installed baseline: compiled source `ed9aa2102ff2d35d4d6cb714d2cf6724ed79eb64`,
documentation HEAD `8e008bd8d7aebee9872ad78acef8afa9e77fe623`, arm64 executable UUID
`2CC15A0C-CB56-343B-929B-BAB099A289D8`, SHA-256
`c7fe1c6ae9351196a53cbb0d2c7b0848421493bf49b87182993863086b12b5b2`.
Strict local signature verification passed. Environment: Apple M4 Max, 16 logical
CPUs, 128 GiB memory, macOS 26.6.2 (25G83). This is a local development signature.

Unaltered initial screenshots in `evidence/`: `before-original-history.png`
(loading), `before-original-history-loaded.png`, `before-original-compare.png`,
and `before-projects.png`. Comparable Aurora fixture History/Compare captures exist in Midnight and Braden; `before-github.png` and `before-settings.png` record the original offline forms. The original repository was inspected passively.
Loading eventually completed with 500 commits and 650 local branches. The
existing slim graph retains readable commit metadata and visible lane controls.

## Findings and corrections

| ID / priority | Reproduction or concrete finding | Correction | Verification |
| --- | --- | --- | --- |
| U1 / P2 | Tab controls, duplicate root Projects/Back navigation, and expanded secondary Git target fields consume persistent vertical/horizontal space | Compact header hierarchy, contextual tab controls, collapsed Targets default | Comparable final `eb3dd26` captures in Braden and Midnight; sustained `4ebe8fc` native session |
| U2 / P2 | Icon-only helper uses label padding around a fixed icon; collapsed rail and preview toolbar do not scale consistently with interface text | Shared square hit areas, scaled icons/rail and adaptive comparison controls | Twenty theme/density captures and enlarged-text native checks on `4ebe8fc` |
| U3 / P1 | PR form has no native line selection/ranges/threads; opening Compare loses adjacent PR context | Persistent Overview/Files/Review workflow with bounded native patch rows | Native fixture discovery, range/threads/source and review workflow on `4ebe8fc`; restart/submission on `28a5d43` |
| U4 / P1 | Destination edit can drop the captured deferred review draft before its 500 ms quiet save | Flush captured draft before scope invalidation; preserve full inline draft text and positions | Quiet-period destination-change regression passed; exact collected/unfinished draft restart verified on `28a5d43` |
| U5 / P2 | Projects headings, field labels, destination and errors are drawn text without readable or live semantics in pinned toolkit | Explicit readable names/roles and scoped error announcements | Native named fields/headings/error semantics and keyboard navigation observed; spoken VoiceOver remains unverified |
| U6 / P1 | Accepted commit-draft saves lack app-lifetime shutdown ownership; closing last tab also lacks explicit Projects focus | Await accepted/coalesced draft completion at shutdown and restore visible root focus | Actual final-window/full-queue draft and last-tab focus regressions passed; exact native commit draft and staged commit exercised through `28a5d43` |
| U7 / P1 | Native Return in the new PR file list or selected patch dismisses the dialog instead of opening content/composition | Scoped review actions and visible focus destinations | Native file-list Return and Shift-Down/Return range composition passed on `4ebe8fc` without dismissing the modal |
| U8 / P1 | Fixed PR modal dimensions exceed the minimum window; description editor collapses in the ordinary window | Viewport-bounded modal, scrollable body and explicit editor heights | Captured overview and minimum-size PR composer/Add action exercised on `4ebe8fc` |
| U9 / P2 | At 1000 × 680 with 18-point interface text, inspector hash/actions clip, Message is below the metadata cutoff, and selected rows can leave view after resizing | Responsive inspector and selection reveal on viewport/text-size changes; reserve the painted horizontal scrollbar below History rows | Inspector/selected-file checks passed on `4ebe8fc`; minimum 1000 × 680, 18/24-point last row clears the scrollbar on `28a5d43` |
| U10 / P2 | Small colored status words miss 4.5:1 contrast in some selected/hover states | Readable primary text for status words; preserve distinct colored icons | Resolved palette audit plus all ten native themes/both densities on `4ebe8fc` |
| U11 / P2 | At minimum size with enlarged text, Projects Clone Tab navigation focuses a form field below the visible viewport | Reveal the newly focused form control | Native Tab/Shift-Tab revealed folder fields, Browse and Clone at minimum size/enlarged text on `4ebe8fc` |
| U12 / P1 | PR warm navigation loses panel context; initial local draft reads can be canceled permanently; retained templates need inclusion in memory accounting | Bounded per-worktree panel cache, retry interrupted local reads, and account for templates and complete drafts | Behavioral PR tests and native warm reopen passed on `4ebe8fc` |
| U13 / P1 | Closing tabs can restore focus to a detached old tab; shared viewport measurements can disturb another tab's saved scroll | Restore attached focus targets only and retain measured viewport with each warm tab | Behavioral regressions; native manual-scroll round trip and closing three preview tabs followed by Command-O passed on `4ebe8fc` |
| U14 / P1 | Captured Markdown companion Exact source leaves focus on the dialog; Command-F reaches the repository behind it | Focus the visible document mode and guard repository Search while a modal or sheet is open | Behavioral regression passed on macOS/Linux; immediate Command-F, retained local query and layered Escape passed natively on `28a5d43` |
| U15 / P1 | Restore saved draft replaces visible conflict text but leaves the accepted save payload and parsed block state stale; Save and stage result stays disabled | Synchronize the restored payload and rerun guarded block parsing (`cc4ffac`) | Reproduced natively on `28a5d43`; regression/gates passed. Native restart, explicit Restore and Save-and-stage on `cc4ffac` produced exact recovered Unicode bytes in both index and working file, preserving the unrelated note. |
| U16 / P2 | Cold history restoration retains the selected commit in the inspector but a queued initial scroll-to-top overwrites the saved list offset | Clear obsolete deferred list scrolling, reset loading geometry, and avoid replaying the bookmark after a user scroll | `eb3dd26` rendered regression and native restart checks passed: End restored item 499 fully visible; a manual scroll with offscreen selection retained all 22 visible rows on restart |

## Intermediate native iteration

The first combined release was built from `926881c88e32bafa0a44b1cb1209a58afe26e765`.
All 835 recorded compiled inputs were hash-checked unchanged after the successful
release build. The local intermediate bundle has UUID
`F2C7C33C-27E5-31B6-A9C3-7182B44B43BA` and executable SHA-256
`5ec0c385223de7bb53d98f3fa8ec5ec5684b1d9e98b996727f0375ad4ef00805`.
It is an iteration artifact, not the final installed package.

Unaltered Braden History/Compare captures use the same Aurora commit and file as
the baseline, at a traced 1480 × 981 content viewport, Comfortable density and
13/12-point text. The compact header, collapsed Targets and square icons are
visible. Native resizing reached exactly 1000 × 680; 18-point interface/code
text exposed U9 and U11. Projects headings, named clone fields, selection and
error semantics appeared in the native accessibility tree.

The offline PR fixture displayed captured identities, check/review status,
existing replies, explicitly outdated positions and four changed files. Clicking
After line 2 and Shift-Down selected the supported 2–3 range with two selected AX
rows and visible range marking. Return exposed U7. These observations establish
the reproductions, not a completed review workflow.

Native Projects checks created `native-create-国際化-workspace` with an unborn
`main`, and cloned the disposable local origin into `native-clone-界面-complete`
with clean `main` at `383ba02db0401256316538ed771e1bf8f1f2d28b`. Attempting an
existing nonempty clone destination reported the error and preserved it. An
earlier fixture clone had an unborn `master` because the synthetic bare origin's
HEAD still named the missing default branch; the fixture HEAD was corrected to
`main` before the successful clone. This setup correction is not an app failure.
No unrelated repository was written. Further regression checks use the corrected
fixtures and the final compiled build.

## Sustained native iteration on `4ebe8fc`

The packaged release compiled from `4ebe8fc834f7c03674828407e78ece29c6aa4d9c`
has UUID `E6FE91E6-4A09-3FF6-8B5C-8E5283E3A26D` and executable SHA-256
`1fc08c8560041f9232b0c9cad824dfd9488bf904343b981b19c1e532b80d6503`.
The 814 compiled input hashes were unchanged through macOS gates. The exact Linux
source archive passed formatting, workspace tests, strict all-target Clippy,
release build and executable/linkage checks. Aggregate test results were 566
passed on macOS and 559 on Linux, with zero failures and three documented opt-in
tests ignored on each platform. These counts exclude duplicated nested preview
subprocess output. Full identities, commands and logs remain in private `final/`.

Two automation startup mistakes launched a duplicate bundle before measurement;
the sampler rejected them. The old persistent helper was replaced with an
explicit canonical package binding, both duplicate processes were quit normally,
and a new clean observation began. The measured mixed interval ran from
04:52:24 to 05:14:22 UTC, with 274 samples through subsequent quiet settlement and
normal Quit. No compiler ran during this observation. It exercises this release;
later corrections require their own targeted native evidence.

Native evidence includes twenty unaltered theme/density captures of shared
history, tabs, inputs, status words, disabled actions and workspace menus. The
minimum 1000 × 680 window was exercised with 18-point interface and 24-point code
text in Braden Compact and Nord Comfortable. Projects Tab/Shift-Tab revealed the
folder fields, Browse and Clone action. The PR file list accepted Return, and
Shift-Down/Return composed a captured After 2–3 range without dismissing the
modal; the editor and Add to review remained visible at minimum size. The
inspector wrapped its controls and retained the selected file. History still
needed to reserve the horizontal scrollbar's painted overlay below its rows;
this was unresolved U9 behavior on that build, not a passed minimum-history claim.

The native offline review collected exact Unicode text, restored its review
section and summary on warm reopen, retained discussion-only mode, refused a
moved-head submission and kept the old draft's original positions available for
copying. Binary and incomplete patches disabled inline anchoring; renamed paths,
exact patch source/Find and outdated/orphaned discussions remained explicit.
Private disk evidence contains both captured head entries, a collected range and
an unfinished composer. No live account or network transport was used.

Other exercised paths include linked PDF navigation to After page 24 and Before
page 12, extracted page-text Find with layered Escape, native 12,800-triangle
model orbit/pan/zoom/edges/reset, deep history beyond the 500-row page boundary,
literal repository search, warm manual-scroll restoration, pin/reorder actions,
a disposable named workspace and the command palette. PDF, Markdown and model
tabs were closed, followed by successful native Command-O and picker cancellation
back to visible History focus.

Initial Markdown loading produced a transient 586.19 MiB RSS peak before opening
the small companion. It settled before the later image retirement, so neither
the peak nor the decline is attributed to companion navigation. Three matched
69-byte companion rendered/source/Find/close cycles used explicit editor focus
as a temporary U14 workaround and settled near 171–172 MiB. Their detailed
resource and callback results, including all outliers, are preserved in the
[dated resource report](benchmarks/native-polish-20260910/README.md). Font/renderer
initialization is a candidate explanation for the initial peak; no allocation profile establishes a cause or a leak.

## Corrected native checks and everyday completion on `28a5d43`

The coordinator exercised release source
`28a5d43522dedc7fb33bbbb68046f588ab6c8bca` after the two corrections found during
the sustained session. At 1000 × 680 with interface/code text 18/24, the last
History row now clears the horizontal scrollbar. In the captured Markdown
companion, Exact source followed immediately by Command-F focuses local Find
without an extra editor click or background repository navigation. Its query
survives source/rendered switching; Escape closes Find before the dialog. These
are the affected U9/U14 native retests; the earlier 20-theme matrix and resource
interval retain their `4ebe8fc` identities.

Actual app restart restored the PR draft captured at head `111…`, including its
exact summary and collected After 2–3 comment. After explicitly moving and
refreshing the offline fixture to head `333…`, the exact new-head summary and
unfinished range composer also restored. The stored discussion-only flag was
true alongside that composer; restoration intentionally normalizes it off while
inline content exists. The text and original positions were retained. A
collected review was then deliberately confirmed and accepted by the offline
provider as `4242`. This is native fixture submission evidence, not a live GitHub
review or credential check.

Everyday fixture checks produced commit `616cd266` from the reviewed index while
preserving unrelated worktree changes. Conflict cancellation, review, manual
resolution and continuation produced merge `9073c93e`; the workflow required a
workaround for the exact-draft restoration bug recorded as U15 below. This
successful merge does not establish that the original restore path passed.
Slow Fetch was canceled; child processes `34727` and `34728` were reaped without
repository changes. An unavailable folder produced an actionable error, then
restored successfully; an external fixture commit `ebdab30` was observed. These
are disposable local fixture outcomes, not operations on unrelated repositories.

Native GIF checks covered Reduce Motion at startup and during playback, manual
frame stepping, overlay/wipe/zoom and no automatic playback resumption after
Reduce Motion was disabled. Follow System responded to actual macOS Light and
Dark changes. VoiceOver was enabled for keyboard-only interaction; spoken
feedback was not verified. All five exercised system accessibility flags were
restored to their original values, VoiceOver was turned off and Dark appearance
restored. Complete genuine application state was restored after final installation and the installed-app smoke check.

## Final corrections, installation and restoration

The `28a5d43` conflict workflow exposed U15: Restore saved draft replaced visible
text but left the accepted payload and parsed blocks stale. Correction `cc4ffac`
synchronizes the payload and reruns guarded parsing. Its rendered regression
reproduced the original failure and verifies disabled staging while parsing is
pending and exact Unicode submission afterward. In the rebuilt native app,
normal Quit/restart → Restore → Save and stage produced the exact recovered text
in both index and worktree (SHA-256
`c2c2f09f6507d717aea7f0059cefb34fcba39515a197ce0a7b4595a554083113`).
The fixture HEAD and unrelated note remained unchanged.

Cold restart then exposed U16: an obsolete initial scroll-to-top request replaced
the saved History offset while the selected commit remained in the inspector.
Correction `eb3dd26` clears that pending navigation, seeds restored geometry and
preserves subsequent manual scrolling while changed files load. The 500-commit
rendered regression and all 16 tab tests passed. In the native final release,
End selected item 499 and restart restored it fully visible at offset −16311.
Scrolling one page upward while retaining that offscreen selection and restarting
again preserved the same 22 visible row texts at offset −15298. This establishes
both selection-following End recovery and intentional manual-scroll recovery.

Final comparable Aurora History/Compare screenshots use final source `eb3dd26`
at 1480 × 981, Comfortable density and 13/12-point text in Braden and Midnight.
The selected commits and files match their baseline counterparts. A native
inspector splitter drag expanded the inspector from 320 to 449.853 points; its
width, the resulting navigation width, and selected context survived Settings →
Back. This records one actual inspector drag, not two independently verified
splitter drags. Closing the last QA tab returned to Projects, where Tab and
Command-O/picker cancellation retained the visible Open repository tab target.

Final compiled source is **`eb3dd264c3d03eda7a3e937d307bf100c603ef02`**.
All 814 compiled inputs remained hash-identical across the macOS gates. The
immutable Linux archive was verified before and after its gates. Both platforms
passed formatting, workspace tests, strict all-target Clippy and release build.
The final workspace logs record 570 passing tests on macOS and 563 on Linux,
zero failures and three documented opt-in tests ignored on each platform;
nested preview subprocess output is counted only once. The linked
[validation.json](benchmarks/native-polish-20260910/validation.json) records
commands, durations, log hashes, source and binary identities. The earlier Linux
test-only Command-F mismatch was corrected to the platform shortcut in
`28a5d43`; its failed log is retained rather than relabeled as passed.

The final bundle is installed at **`/Applications/GitTurtle.app`**, version 0.1.0,
bundle identifier `com.gitturtle.desktop`, arm64 executable UUID
**`37B51CFC-5658-3D0D-A83A-0C87DC4FA564`**, executable SHA-256
**`b2deab6aec9d33a77caab0fad7a814a927fbe862c7705f9effed3dcfdd70b610`**.
All five installed files match the tested package. Its plist matches the original;
the original turtle source artwork and ICNS hash are unchanged. Strict local
ad-hoc signature verification passed; this is not Developer ID notarization.
The exact installed executable launched without the offline-provider override.
Process-path lookup, mapped device/inode, executable hash and UUID established
its running identity. The original two-tab workspace, History selection and
changed file restored; file activation opened native read-only Compare, Escape
returned the selected History row, and Settings showed the original Ember,
Comfortable and 13/12-point values. Back retained context.

After normal Quit, the complete original application data was restored
byte-for-byte again, and the desktop preference dictionary matched the backup.
The app remains stopped, as it was before this task. Task-created draft/attempt
files were archived privately and removed from live state; all original
preferences, profiles, assignments, recents and saved sessions were preserved.
The baseline bundle remains in the private backup. Original Dark appearance,
keyboard navigation, VoiceOver, Reduce Motion, Increase Contrast, Reduce
Transparency and Differentiate Without Color settings are restored. The last
flag was initially absent (default off) and is now explicitly false; its effective
setting is unchanged.

Live GitHub transport/credentials, hosted CI, native Linux interaction, spoken
VoiceOver feedback and sleep/wake remain unverified. Existing authorization and
manual VoiceOver requests remain pending without repetition. Local fixture PR
submission and keyboard/AX observations do not establish those claims. The
21 min 58 sec resource session remains attached to `4ebe8fc`; later fixes have
separate affected native checks and are not assigned its measurements.

## Meaningful increments

| Commit | Completed increment |
| --- | --- |
| `d624215` | Shared native header/control geometry, workspaces and Projects semantics |
| `926881c` | Initial native PR Files, captured ranges, threads and collected review |
| `a8d5b7d` | Accepted/coalesced recovery-draft shutdown ownership |
| `26214cb` | Readable status text, responsive inspector and focused-control reveal |
| `c5ad343`, `5ff7b3a` | Attached tab focus and per-tab measured viewport preservation |
| `4ebe8fc` | Bounded responsive PR workflow, durable drafts and warm navigation |
| `aa39920`, `28a5d43` | History scrollbar clearance and modal source Find; portable shortcut test |
| `cc4ffac` | Exact conflict draft restoration and guarded parser refresh |
| `eb3dd26` | Cold History scroll restoration without overriding manual navigation |

The final evidence commit changes documentation and fixture screenshots only;
it does not change the compiled source identified above.

## Ownership and evidence rules

The coordinator owns Git index/commits, integration, all native interaction,
system settings, packaging and installation. Independent workers own shared view
geometry, PR/provider/draft modules, and bounded Projects/persistence corrections.
Mutations use disposable local fixtures; no hosted repository/account has been
newly authorized. Preserve the existing hosted authorization and spoken
VoiceOver requests without repeating them.

Record exact source and executable for native checks. Separate worker timing,
handler-to-frame callbacks and actual displayed results. Retain raw outliers,
cache state and background-load limitations. Tests and AX trees alone cannot
establish visual polish or spoken VoiceOver usability. Update completed rows with
actual evidence; finish this finite list without adding unrelated feature work.
