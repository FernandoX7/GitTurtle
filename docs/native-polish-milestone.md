# Native polish and pull-request review milestone

Started September 10, 2026 UTC (September 9 local), from clean `8e008bd`.
This record owns the finite acceptance list for the current task. The previous
[milestone](next-milestone.md) and its evidence retain their original identities.
The canonical product contract remains [DESIGN.md](../DESIGN.md).

## Acceptance

| ID | Required outcome | State and evidence |
| --- | --- | --- |
| P1 | Preserve installed bundle, complete genuine app state, original settings and artwork; inspect baseline before edits | Backed up before native interaction; identity below. Restoration pending. |
| P2 | Coherent shared typography, icon geometry, focus/states, header hierarchy and progressive disclosure | Baseline audit complete; implementation in progress. |
| P3 | Deliberate overflow at minimum/ordinary/wide sizes, long Unicode names, enlarged text, both densities and eight tabs; preserve slim/deep graph | Shared sizing/layout implementation complete; native matrix pending. |
| P4 | Complete Projects, tabs/workspaces, History, Changes, conflicts/recovery, previews, Settings/palette and error/confirmation workflows with retained context | Bounded corrections and native checks pending. |
| P5 | Native PR discovery, captured overview/check/review identity, files, line/range composition and existing threads | Implementation in progress; fixed offline provider fixture first. |
| P6 | Collect/edit comments, deliberate captured review submission, durable unfinished drafts across navigation/restart, explicit moved-head/outdated recovery | Implementation and behavioral tests pending. No live outbound action authorized. |
| P7 | Principal keyboard/AX semantics, focus containment/restoration, non-color cues, actual appearance/accessibility settings and controlled VoiceOver | Projects semantic and draft-lifetime regressions passed; native recheck pending. Prior spoken VoiceOver manual request remains pending. |
| P8 | Release measurements on affected paths and demanding fixtures; investigate reproducible stalls/peaks without unsupported speed/leak claims | Pending. Prior 685.33 MiB transient peak is unattributed; retain prior evidence. |
| P9 | At least 20-minute release mixed native session, resource cleanup, external fixture changes, missing folder and restart recovery | Pending. Earlier sustained sessions do not establish this build's pass. |
| P10 | Final fmt, workspace tests, strict all-target Clippy, release, available Linux checks and meaningful commits | Pending integration. Linux Docker environment available; native Linux and hosted CI remain distinct. |
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
