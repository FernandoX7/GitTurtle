# Native polish and pull-request review milestone

Started September 10, 2026 UTC (September 9 local), from clean `8e008bd`.
This record owns the finite acceptance list for the current task. The previous
[milestone](next-milestone.md) and its evidence retain their original identities.
The canonical product contract remains [DESIGN.md](../DESIGN.md).

## Acceptance

| ID | Required outcome | State and evidence |
| --- | --- | --- |
| P1 | Preserve installed bundle, complete genuine app state, original settings and artwork; inspect baseline before edits | Backed up before native interaction; identity below. Restoration pending. |
| P2 | Coherent shared typography, icon geometry, focus/states, header hierarchy and progressive disclosure | Implemented; all ten native palettes in both densities inspected on `4ebe8fc`. Final comparable captures pending. |
| P3 | Deliberate overflow at minimum/ordinary/wide sizes, long Unicode names, enlarged text, both densities and eight tabs; preserve slim/deep graph | Native 1000 × 680, 1480 × 981 and 1604 × 780 exercised, including 18/24-point text. Remaining history scrollbar overlap correction awaiting native recheck. |
| P4 | Complete Projects, tabs/workspaces, History, Changes, conflicts/recovery, previews, Settings/palette and error/confirmation workflows with retained context | Bounded corrections and native checks pending. |
| P5 | Native PR discovery, captured overview/check/review identity, files, line/range composition and existing threads | Implemented and exercised through the offline native provider fixture on `4ebe8fc`; live GitHub remains unverified. |
| P6 | Collect/edit comments, deliberate captured review submission, durable unfinished drafts across navigation/restart, explicit moved-head/outdated recovery | Native range collection, exact summary, warm reopen, discussion-only mode, stale-head refusal and old-head recovery verified. Disk snapshots retain collected and unfinished ranges; restart and successful fixture submission pending. |
| P7 | Principal keyboard/AX semantics, focus containment/restoration, non-color cues, actual appearance/accessibility settings and controlled VoiceOver | Projects semantic and draft-lifetime regressions passed; native recheck pending. Prior spoken VoiceOver manual request remains pending. |
| P8 | Release measurements on affected paths and demanding fixtures; investigate reproducible stalls/peaks without unsupported speed/leak claims | Sustained `4ebe8fc` observations retained; initial Markdown peak separated from three matched small-companion cycles. Frozen analysis underway; no improvement or leak claim. |
| P9 | At least 20-minute release mixed native session, resource cleanup, external fixture changes, missing folder and restart recovery | A 21 min 58 sec mixed interval on `4ebe8fc` completed, followed by quiet settlement and normal Quit. Everyday interruption/restart checks remain pending. |
| P10 | Final fmt, workspace tests, strict all-target Clippy, release, available Linux checks and meaningful commits | All macOS and Linux gates passed on `4ebe8fc`; two native corrections require another integrated gate run. Native Linux and hosted CI remain unverified. |
| P11 | Verified final plist, provenance, artwork, signature, UUID/SHA; install and launch exact `/Applications/GitTurtle.app`; restore genuine state/settings | Pending final package. |

## Preservation and baseline

Private local evidence: `/tmp/gitturtle-polish-20260910` (mode 0700).
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
| U1 / P2 | Tab controls, duplicate root Projects/Back navigation, and expanded secondary Git target fields consume persistent vertical/horizontal space | Compact header hierarchy, contextual tab controls, collapsed Targets default | Pending native comparison |
| U2 / P2 | Icon-only helper uses label padding around a fixed icon; collapsed rail and preview toolbar do not scale consistently with interface text | Shared square hit areas, scaled icons/rail and adaptive comparison controls | Pending native matrix |
| U3 / P1 | PR form has no native line selection/ranges/threads; opening Compare loses adjacent PR context | Persistent Overview/Files/Review workflow with bounded native patch rows | Pending implementation |
| U4 / P1 | Destination edit can drop the captured deferred review draft before its 500 ms quiet save | Flush captured draft before scope invalidation; preserve full inline draft text and positions | Regression pending |
| U5 / P2 | Projects headings, field labels, destination and errors are drawn text without readable or live semantics in pinned toolkit | Explicit readable names/roles and scoped error announcements | Pending native check |
| U6 / P1 | Accepted commit-draft saves lack app-lifetime shutdown ownership; closing last tab also lacks explicit Projects focus | Await accepted/coalesced draft completion at shutdown and restore visible root focus | Actual final-window/full-queue draft regression and last-tab focus regression passed; native recheck pending |
| U7 / P1 | Native Return in the new PR file list or selected patch dismisses the dialog instead of opening content/composition | Scoped review actions and visible focus destinations | Reproduced in the intermediate release; correction and native recheck pending |
| U8 / P1 | Fixed PR modal dimensions exceed the minimum window; description editor collapses in the ordinary window | Viewport-bounded modal, scrollable body and explicit editor heights | Native description reproduction and toolkit sizing review; recheck pending |
| U9 / P2 | At 1000 × 680 with 18-point interface text, inspector hash/actions clip, Message is below the metadata cutoff, and selected rows can leave view after resizing | Responsive inspector and selection reveal on viewport/text-size changes | Reproduced in the intermediate release; correction pending |
| U10 / P2 | Small colored status words miss 4.5:1 contrast in some selected/hover states | Readable primary text for status words; preserve distinct colored icons | Resolved palette audit; native corrected-state recheck pending |
| U11 / P2 | At minimum size with enlarged text, Projects Clone Tab navigation focuses a form field below the visible viewport | Reveal the newly focused form control | Reproduced with native keyboard and visible screenshot; correction pending |
| U12 / P1 | PR warm navigation loses panel context; initial local draft reads can be canceled permanently; retained templates need inclusion in memory accounting | Bounded per-worktree panel cache, retry interrupted local reads, and account for templates and complete drafts | Behavioral PR tests and native warm reopen passed on `4ebe8fc` |
| U13 / P1 | Closing tabs can restore focus to a detached old tab; shared viewport measurements can disturb another tab's saved scroll | Restore attached focus targets only and retain measured viewport with each warm tab | Behavioral regressions; native manual-scroll round trip and closing three preview tabs followed by Command-O passed on `4ebe8fc` |
| U14 / P1 | Captured Markdown companion Exact source leaves focus on the dialog; Command-F reaches the repository behind it | Focus the visible document mode and guard repository Search while a modal or sheet is open | Reproduced on `4ebe8fc`; correction and regression test prepared, native recheck pending |

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
this is the remaining U9 correction, not a passed minimum-history claim.

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
resource and callback results, including all outliers, are being frozen for the
dated evidence report. Font/renderer initialization is a candidate explanation
for the initial peak; no allocation profile establishes a cause or a leak.

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
