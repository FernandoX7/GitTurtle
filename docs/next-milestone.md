# Everyday workspaces, interactive previews and collaboration milestone

Started September 10, 2026 UTC (September 9 local time), from clean checkout
`2775a06`. This ledger tracks the complete authorized milestone. It is in progress;
unchecked rows are required work, not waived scope. Earlier evidence remains tied
to its own source and build. VoiceOver testing is explicitly included in this
milestone; earlier instructions excluding it are superseded.

## Finite acceptance

| ID | Required outcome | State |
| --- | --- | --- |
| A1 | Investigate installed macOS repository access; actionable failure guidance; explicit picker, reopening and restart | Native original-build reopen/picker/restart passed; classified actionable error handling implemented; final installed recheck pending |
| A2 | Open/select/reorder/close repository tabs; pin/group; bounded session retention/restoration; captured operation targets and recoverable drafts | Integrated; eight behavioral tests passed; final native session pending |
| A3 | Interactive linked/independent 3D cameras, standard views, edges, scale and orientation; bounded processing and resource cleanup | Integrated; interim native orbit, linked/independent cameras, edges and curved STEP passed; release lifetime checks pending |
| A4 | Feasible curved STEP/assembly support from maintained compatible implementation; explicit units/placements/fidelity limits | Finite analytic curved primitives and mapped representation assemblies implemented and tested; general trimmed/NURBS STEP remains explicitly unsupported |
| A5 | On-demand PDF pages with number entry, independent/linked navigation, bounded byte-accounted cache, cancellation and retained positions/zoom | Integrated, with extracted page text; interim native page20/unequal-count linking/Back passed; final tab/restart pending |
| A6 | Native rendered Markdown with integrated Mermaid, exact source/diffs, safe captured-revision images and explicit links | Integrated; interim native prose/table/diagram, historical images and local link passed; linked-scroll correction awaits final native recheck |
| A7 | Incremental bounded ordinary history; stable graph/selection; realistic 100k+ fixtures and comparable release measurements | Core and graph release measurements recorded for three120k fixtures; native frame/scroll/selection checks pending |
| A8 | GitHub secure account connection, paged PR inspection/create/changes/comments/reviews, drafts and stale/uncertain outcomes | Integrated;16 mock/process tests passed; disposable hosted repository/account authorization requested once and remains pending |
| A9 | Principal-workflow accessibility, toolkit semantics, keyboard/focus, actual VoiceOver and system-setting checks | Semantics and focused toolkit fixes integrated; native VoiceOver/settings session pending |
| A10 | Native UI corrections; all themes/densities, minimum/wide/enlarged text, states and restart; canonical design/README updates | Pending; user graph-width finding included |
| G1 | Targeted behavioral tests; final fmt/workspace tests/strict Clippy/release; available Linux checks | Pending |
| G2 | Reproducible release mixed native session of at least 20 minutes, resources/latency, screenshots and accessibility evidence | Pending |
| G3 | Meaningful local commits; final package provenance/plist/assets/signature/UUID/SHA; install exact path and launch | Pending |
| G4 | Restore genuine app state and original accessibility/system choices; remove only task QA state | Pending |

## Preservation and baseline

The installed executable at `/Applications/GitTurtle.app` has UUID
`0C794E6C-D59D-353C-B23C-31321BDD1245` and SHA-256
`bd1af7e3e8dbf93c952a03ea00b820cd3f10f195a54f72ddfb36e4f56d4829ad`.
It is the handoff build compiled from `92ea02c`.
Before native interaction, the full bundle and complete application-data directory
(two files) were backed up to the private local evidence directory
`/tmp/gitturtle-next-20260910`. That directory holds a per-file SHA-256 manifest,
original global/universal-access/VoiceOver/app preference exports, hardware and
installed identity records, and the unaltered baseline screenshot. No repository
write or protected privacy-database change is part of access diagnosis.

## Findings

| ID / severity | Reproduction | Correction | Verification / identity |
| --- | --- | --- | --- |
| N1 / P1 | Prior milestone original repository open exceeded passive deadline, with Git blocked in macOS access. Current installed launch first displayed loading, then loaded 500 commits and 650 local branches successfully. | Diagnosis and evidence-based recovery guidance pending | Unchanged installed handoff UUID above; current TCC attribution includes an allowed request. Earlier denial is not sufficient to identify the present cause. Native folder picker and normal restart both reopened the original repository in the handoff build. Historical cause cannot be inferred from current success; no permission setting was changed. |
| N2 / P2 | User screenshot: a single visible graph lane occupies the left edge of a very wide graph column, pushing commit summaries out of view. | Automatic graph allowance capped at one quarter of the history viewport (112–280 px), fixed lane spacing/clipping, explicit lane navigation, selected-lane reveal | Corrected interim UUID `6461A99E` native screenshot `slim-graph-after.png` in the original repository: subjects/authors/dates remain visible; 35 total lanes with 11 visible. No repository mutation. |

## Ownership and continuation

Coordinator owns integration, Git index/commits, all native UI interaction,
packaging, installation, app-state preservation and final combined gates.
Independent workers own bounded history/graph traversal, 3D/CAD decoder work and
GitHub collaboration modules. Shared integration APIs are coordinated before
editing. All mutation fixtures remain disposable. No arbitrary hosted repository
will receive branches, PRs, comments or reviews.

Every completed row must cite actual checks and source/build identities. Findings
record reproduction, severity, correction and recheck. Worker timings, native
frame callbacks, displayed results, hosted CI and Linux native interaction remain
separate evidence categories.

## Interim native checks, September 10 UTC

The initial debug package hung before its first paint. A one-second process
sample showed the main thread inside `EmbeddedAssets::get` → filesystem read of
source SVGs. `rust-embed` normally reads source files in debug mode; enabling its
`debug-embed` feature also unifies the toolkit asset dependency and makes packaged
debug executables self-contained. This is finding N3 / P1, separate from the
previous Git-child deadline. The corrected package launched and painted the
fixture successfully. No privacy database or protected permission was changed.

The corrected interim debug bundle has executable UUID
`6461A99E-74EE-39BF-B2C7-CFC42B8B4D8A`, SHA-256
`196138e8a6775850302a2ef2d2efb9bad511e57db277aacfac136c64762734d5`.
It includes the in-progress history, PDF, geometry and GitHub integration at this
point; it is not the final release artifact. The private evidence folder retains
the hung-process sample and unaltered `pdf-native-*.png` screenshots.

Native PDF fixture: before revision has twelve pages, after has twenty-four.
Explicit page-number entry opened After page20 while Before remained1. Turning
on linked pages clamped Before to12 while retaining After20. Back to History then
activating the same file retained12/20 and linked state. Actual page text and AX
page-button labels agreed. The independent untracked note was untouched. These
checks establish native paging beyond the old eight-page ceiling; final release,
resource, tab and restart checks remain pending.

Further native findings: N4 / P2: linked Markdown Top/End moved only one side, and wheel synchronization used the toolkit event's previous visible range. Explicit navigation now updates both sides, and wheel synchronization reads the committed logical offset after the list callback. N5 / P1: the toolkit focus-trap wrapper omitted its accessible role/node forwarding; its narrow patch preserves named modal semantics. Both require final native rechecks.

Original system observations: VoiceOver off, Increase Contrast off, Reduce Transparency off, Reduce Motion off, auto-play animated images on, and keyboard navigation off. Keyboard navigation was enabled explicitly for this controlled QA period; it must be restored to off. Unaltered settings screenshots and original preference exports are retained privately.
