# Security, architecture and resource milestone — September 10, 2026

Status: implementation and verification in progress. This record distinguishes
source inspection, adversarial fixtures, release measurements and native evidence.
It does not claim comprehensive security or universal scalability.

The clean starting checkout was `b36efd036a4c61931844726ca526833aa1208098`.
The installed handoff executable is inspected and backed up before replacement;
the preceding [native milestone](native-polish-milestone.md) retains its evidence.
Private state, raw logs and build artifacts live in the ignored, mode-0700
`.local/security-milestone-20260910` directory. Genuine state is backed up both
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

- [ ] S1: Correct passive-process deadline/pipe ownership failures with descendant,
  cancellation and cleanup regressions, preserving explicit Git semantics.
- [ ] S2: Bound internal SVG reference expansion before renderer allocation, with
  ordinary supported references and adversarial fan-out/cycle tests.
- [ ] S3: Make app-store reads descriptor-checked, bounded and nonblocking for
  special files; enforce encoded GitHub store limits before replacement.
- [ ] S4: Audit GitHub target/credential/submission boundaries, dependency
  advisories, unsafe/FFI and package exposure; correct concrete material findings.
- [ ] S5: Invalidate retained object readers/history when a worktree or Git
  administration directory is replaced at the same path, preserving warm reuse.
- [ ] A1: Document cohesive ownership changes and concrete triggers/tests in
  focused audit records; verify scheduling, cache and accepted-write invariants.
- [ ] P1: Profile the earlier initial Markdown transient using comparable local
  release fixtures; attribute causes only where profiling supports them.
- [ ] P2: Exercise practical workload dimensions and recorded resource budgets:
  120,000-commit history, dense refs/worktrees, large status/search/diff and offline
  PR models, malformed previews, eight tabs, switching/refresh/cancel/shutdown.
- [ ] V1: Targeted behavior/adversarial checks followed by formatting, locked
  workspace tests, strict all-target Clippy and release compilation.
- [ ] V2: Execute available Linux checks using the existing isolated local image;
  distinguish container gates from native Linux UI and hosted CI.
- [ ] N1: At least 20 minutes of mixed native release interaction, with raw
  resource samples, responsiveness, focus/context/draft and cleanup observations.
- [ ] R1: Meaningful commits, verified original artwork/package identity,
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
 Pending evidence is not a passed check. Live GitHub and hosted checks remain
outside this local scope without an authorized disposable repository/account;
existing pending requests are preserved and no secrets will be requested.
