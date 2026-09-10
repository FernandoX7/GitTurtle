# Security, architecture and resource milestone — September 10, 2026

Status: the finite acceptance scope is complete for compiled source
`b4440f132999ebc45a81b1baa31eda4594bddd66`: prioritized corrections, focused audits,
macOS/Linux source gates, workload probes, the mixed native session and installed
smoke/restoration checks. This record distinguishes source inspection, adversarial
fixtures, release measurements and native evidence; it does not claim comprehensive security or
universal scalability.

The clean starting checkout was `b36efd036a4c61931844726ca526833aa1208098`.
The installed handoff executable was inspected and backed up before replacement;
the preceding [native milestone](native-polish-milestone.md) retains its evidence.
Private state, raw logs and build artifacts live in the ignored, mode-0700
`.local/security-milestone-20260910` directory. Genuine state was backed up both
before and after normal Quit. Only disposable fixtures receive test mutations.

## Threat model and ownership

Untrusted repositories supply configuration, byte paths, refs, objects, document
bytes and commit metadata. GitHub supplies remote identities, review text and
patches. Local app stores can be corrupt or replaced. Protect repository/index
integrity, credentials and drafts, correct reviewed targets, responsiveness and
bounded process/resource ownership. Same-user arbitrary code execution, hostile
OS/kernel and crash-proof transactions across independent external processes are
outside the application's security boundary.

| Boundary | Owner and invariant |
| --- | --- |
| UI intent → Git writes | Native captured review → serialized typed core operation; no passive write/network work and no replay after uncertain results |
| Repository → owned models | Core fixed argument arrays, byte-safe paths, passive command policy, bounded process I/O and immutable identities |
| Models → native UI | Replaceable worker reads, generation/repository checks, bounded queues/caches; virtualized UI receives owned prepared data |
| Document bytes → pixels/text | Preview supplied-byte decoders; no external resources; independent input, expansion and output budgets |
| Native text → persistence | Separate serialized/coalesced writer; bounded stores, exact draft ownership, atomic replacement and explicit failure |
| GitHub → review/submission | Captured account/repository/base/head, native literal rendering, explicit action, persistent uncertainty/duplicate prevention |
| Build → installed bundle | Locked dependencies, reviewed build/FFI surface, source manifest, signature, UUID/hash and exact-path launch |

## Finite acceptance checklist

The initial audit sets the scope below. Expand only for related regressions or a
new material security risk; retain justified residual limits instead of broad
rewrites. Independent owners audit core, previews and persistence/GitHub; the
coordinator alone integrates, commits, measures and operates native/package state.

- [x] S1: Correct passive-process deadline/pipe ownership failures with descendant,
  cancellation and cleanup regressions, preserving explicit Git semantics.
- [x] S2: Bound internal SVG reference expansion before renderer allocation, with
  ordinary supported references and adversarial fan-out/cycle tests.
- [x] S3: Make app-store reads descriptor-checked, bounded and nonblocking for
  special files; enforce encoded GitHub store limits before replacement.
- [x] S4: Audit GitHub target/credential/submission boundaries, dependency
  advisories, unsafe/FFI and package exposure; correct concrete material findings.
- [x] S5: Invalidate retained object readers/history when a worktree or Git
  administration directory is replaced at the same path, preserving warm reuse.
- [x] A1: Document cohesive ownership changes and concrete triggers/tests in
  focused audit records; verify scheduling, cache and accepted-write invariants.
- [x] P1: Profile the earlier initial Markdown transient using comparable local
  release fixtures; attribute causes only where profiling supports them.
- [x] P2: Exercise practical workload dimensions and recorded resource budgets:
  120,000-commit history, dense refs/worktrees, large status/search/diff and offline
  PR models, malformed previews, eight tabs, switching/refresh/cancel/shutdown.
- [x] V1: Targeted behavior/adversarial checks followed by formatting, locked
  workspace tests, strict all-target Clippy and release compilation.
- [x] V2: Execute available Linux checks using the existing isolated local image;
  distinguish container gates from native Linux UI and hosted CI.
- [x] N1: At least 20 minutes of mixed native release interaction, with raw
  resource samples, responsiveness, focus/context/draft and cleanup observations.
- [x] R1: Meaningful commits, verified original artwork/package identity,
  installation at `/Applications/GitTurtle.app`, exact-path running smoke and
  restoration of genuine app state/system settings.

## Resource budgets and evidence rules

Existing bounds in [architecture](architecture.md#budgets-and-validation) are
the acceptance baseline: one active/one pending replaceable read per worker,
eight accepted serial queue entries, 32 preview cache entries/128 MiB counted
CPU content per cache, eight tabs, 5,000 history rows/64 MiB window, 10,000 search
matches/64 MiB and 15-second bounded core calls. These do not cap total process,
font, codec, native/GPU or retained-editor allocations.

Use warm filesystem caches without forced system cache eviction; record cold
process versus warm application caches, immutable fixtures, build identities,
hardware/background load, sample counts, median/tail/max and cleanup. Compare
equal work. The earlier 586.188-MiB Markdown peak is a transient observation,
not a leak diagnosis. Preserve every outlier. New measurements must remain
attached to their actual source; no application-wide speed claim follows from
parser/core-only tests.

## Findings and completion evidence

Implemented findings are recorded in the [Git process audit](security-git-audit.md),
[preview audit](security-preview-audit.md), [persistence/GitHub audit](security-persistence-review-audit.md)
and [build/dependency audit](security-build-audit.md).

Passive process cleanup now retains deadlines after the direct child exits and
wakes owned nonblocking I/O threads even when a detached descendant holds their
pipes. The private `process_io` module centralizes that ownership without changing
explicit hooks, filters, signing or credential semantics. The regressions
reproduce the original blocked/over-deadline behaviour and verify bounded return,
protocol invalidation and preservation of ordinary Git outcomes (S1).

The private SVG preflight now budgets expanded references and marker geometry
before renderer allocation. Compact fan-out, marker multiplication, cycles and
CSS/namespace bypass fixtures are refused, while pixel tests preserve supported
references, arrow markers and Mermaid diagrams. Shared structural limits avoid
duplicated expansion policy; documented conservative CSS/geometry refusals are
the compatibility tradeoff (S2).

Application stores now share descriptor-checked, byte-bounded reads with Unix
final-component no-follow and nonblocking opens. GitHub attempt writes check
encoded JSON size; coalesced draft batches publish once; session saves preserve
corrupt or unsupported restart data. API requests isolate CLI routing preferences
in a private temporary configuration and pin the initial GitHub.com API URL.
A dummy-token Unix-socket/loopback-proxy fixture verified the routing correction.
Credentials, durable drafts and accepted outbound attempts retain distinct
owners, and uncertain network actions are not replayed (S3, S4, A1).

**SESSION-1 / P2:** replacing a repository or its `.git` directory at the same
path reused the retained `cat-file` reader. A new-repository blob was reported
unavailable even though it existed. A second fixture redirected a same-inode,
same-length `.git` file and restored mtime, reproducing the same failure.
`worker::RepositorySession::open` now checks the root/private/common directory
identities and indirection metadata (including Unix ctime). A private
`worker/repository_identity.rs` owns those checks. All three targeted tests pass:
replacement, preserved-mtime redirection, and normal nested/linked-worktree reuse.
This adds local metadata checks on the worker; it preserves normal ref/index
changes and validates on Open/history Refresh. It is not a filesystem transaction
or validation of every already-captured preview job. Existing UI clones may retain
the old shared process until the snapshot is replaced; the test's weak Arc proves
release of the session owner only.

The core release comparison contains 80 samples per variant. Median 100-blob
batch time was 1.767 → 1.840 ms; changed-file read 13.363 → 13.244 ms; 100-commit
history 13.234 → 12.861 ms. Background load was uncontrolled; this supports similar
median backend latency, with no application speed claim.

The [native Markdown baseline investigation](benchmarks/security-markdown-20260910.md)
reproduced a 139.1 → 794.9 → 575.6 MiB sequence in the handoff executable.
VM snapshots identify 416.6 MiB of regions labelled `MALLOC_LARGE (empty)` and a
224.0 MiB graphics-residency increase; a nearby heap inventory reports 18.187 MiB
of live malloc allocations. Worker samples show SVG system-font initialization,
but failed Allocations attachment and lite-mode logging prevent attribution of
freed allocations or every peak cause. P1 is complete as a bounded investigation,
not a leak diagnosis or a final-build memory/speed improvement claim.

The [final source and package gates](security-build-audit.md#final-source-gates-completed)
passed for compiled source `b4440f132999ebc45a81b1baa31eda4594bddd66`:
formatting, locked workspace tests, strict all-target Clippy and release builds
on macOS and the isolated local Linux container. The runs passed 589 and 582
top-level tests respectively, with zero failures and five ignored tests each.
The private packaged macOS executable has UUID
`2475FAA0-21B2-398D-9F9C-A2D57C92543A` and SHA-256
`3ce5c6e541c1c7d459982c63ea497705511680f837ec78fb32ea5a8e1e21d020`;
provenance, original icon, plist and signature checks passed (V1, V2).

The [mixed native release report](benchmarks/security-native-20260910.md) and
[numerical record](benchmarks/security-native-20260910.json) cover 20 minutes
15.279 seconds, from `08:56:44.175944` to `09:16:59.454686 UTC`, with 244 resource
samples. Recorded workflows include 120,000-commit history, dense refs and linked
worktrees, large status/search/diff, malformed previews, eight tabs, repository
replacement, refresh/switching, local slow-fetch cancellation, Markdown/PDF/model
previews, exact Unicode drafts and an explicit disposable commit. After normal
Quit, the independent cleanup check found zero application processes and zero
survivors among the 26 observed descendants (P2, N1).

Mixed-session RSS was 114.094 → 241.172 MiB, with a 550.672 MiB sampled maximum;
five samples during ten alternating Markdown cycles were 235.391 MiB. A separate
warm VM snapshot still reported a process-lifetime peak of 805.1M and retained
graphics allocations. These different accounting measures and coarse samples
do not prove a leak, zero retained GPU resources or improvement over the baseline.
The report retains every callback outlier and explains its measurement boundaries.

The native offline PR fixture has four files. The separate
[large offline PR backend/model probe](benchmarks/large-pr-probe.md) and
[result](benchmarks/large-pr-probe-20260910.json) passed after native sampling:
300 files across three pages, 2,000 comments across twenty pages and 128 exact
review drafts in a 9,256,011-byte store. It verified page/patch limits, changed-head
refusal and preservation on a rejected 129th draft, using synthetic GET responses
and no live requests or POSTs. Its 0.34-second release invocation and 151.95 MiB
maximum RSS describe the probe process only. This completes the practical offline
workload evidence without claiming native large-PR rendering or maximum capacity
(P2).

The [installed smoke and restoration record](benchmarks/security-installed-20260910.json)
ties `/Applications/GitTurtle.app` to the same executable SHA-256 and UUID above.
The mapped installed smoke process (PID 61133) restored exact Unicode commit and
review drafts, an unfinished composer, Braden/Compact/14 preferences and Markdown
context. An offline stale-head Send was refused with its draft retained. Genuine
application state was then restored byte-for-byte across three files at
`09:21:25 UTC`; the final installed process (PID 72968) mapped the same executable
and opened the genuine three-tab session. Its normal launch advanced only
`repository-session.tabs[2].bookmark.pinned.Pinned[8]` from `b36efd` to `b444`,
matching the development repository's current HEAD; all other state matched.
The original backup is preserved. This is byte-exact restoration before launch,
with the documented bookmark update after launch. OS Dark appearance and the
complete captured universal-access settings remained unchanged (R1).

The implementation was committed in four meaningful increments: `8847733`
(passive process ownership), `9cdb1e0` (persistence/API routing), `51c8204`
(SVG expansion bounds) and `b4440f1` (repository replacement identity). The final
documentation commit does not alter the compiled artifact.

## Residual limits

The corrected limits do not sandbox in-process codecs or native frameworks, cap
system-font discovery or every CSS/rasterization cost, or impose a process-wide
budget across fonts, allocators, editors and GPU resources. Cancellation remains
cooperative within codec phases, and independently daemonized helpers remain
outside GitTurtle's process-group ownership. Same-account replacement races and
provider check-to-acceptance races are not atomic transactions. The dependency
catalog matched no vulnerability and six maintenance advisories; retaining those
pinned toolkit dependencies is documented, not a claim that they are safe.

No live GitHub account/write, hosted CI run or native Linux desktop interaction
has been established by these local gates. Hosted validation remains outside this
scope without an authorized disposable repository/account; existing pending
requests are preserved and no secrets were requested. These coverage limits
remain explicit after completion of this finite milestone.
