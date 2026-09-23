# Commit inspector and delivery initiative

Status: authorized feature intake, September 15, 2026. The source queue is
[tasks.json](tasks.json); this document describes its intended outcomes and the
coordinator checkpoints that the current runner cannot complete by itself.
No implementation, unattended run, repository-setting change or release was
started when this list was created.

As of September 23, 2026, the queue is resolved on `main`. Ten tasks were
integrated in `b5d681c` on September 15, and `ci-codeql` is superseded, not
passed, as recorded in the [CodeQL retirement](../ci-codeql.md#initiative-scope-amendment).
The [coordinator checkpoints](#coordinator-checkpoints) C0 to C4 remain open,
together with the macOS evidence of two integrated tasks. They need a real Mac,
Apple credentials and hosted samples: the `ui-commit-messages` macOS screenshot and
interaction matrix, the `dist-macos-package` run on a Mac, C1's repeated cache
samples for medians and tails and its hosted invalidation and corrupt-restore
cases, C3's artifact upload after C0, and the C4 signing and release rehearsal.

## Outcomes

1. A beautiful native inspector that makes the complete commit title and message
   readable immediately in History, Compare and File History.
2. Fast, trustworthy feedback for contributors, with measured compilation reuse,
   appropriate parallel work and preserved platform/security coverage.
3. Downloadable packages whose source, contents and installation have been
   verified, followed by controlled tagged releases and notarized macOS delivery.

Task acceptance and initiative completion are different boundaries. Each task
defines exactly what its candidate must establish. Source preparation for hosted
workflows is useful progress; the corresponding checkpoints below must also pass
before we claim fast CI, working delivery or a notarized release.

## Feature queue

The runner processes eligible tasks serially in list order. Dependencies represent
actual prerequisites; UI and platform packaging do not depend on unrelated CI
work or Apple account availability.

### CI foundations: first five tasks

1. **ci-observability — CI measurement and failure diagnostics.**
   Add reproducible timing reports, compilation-versus-test breakdowns, queue and
   cache costs, critical-path reporting and useful retained failure evidence.
2. **ci-run-policy — One validation per PR and a reliable merge gate.**
   Remove duplicate push/PR work, cancel superseded runs, classify changes
   conservatively and make required failures/skips explicit. Retain compatible
   required-check names during the live protection migration.
3. **ci-rust-cache — Correct compilation reuse.**
   Reuse dependency/build outputs with platform, toolchain, profile and vendor
   invalidation, bounded storage and observable cache behavior.
4. **ci-parallel-checks — Independent cached validation.**
   Run inexpensive checks, tests, Clippy and optimized builds in suitable lanes.
   Preserve macOS/Linux workspace and doctest coverage. Test sharding is
   conditional on measured benefit after compilation reuse.
5. **ci-codeql — Measured security analysis and migration preparation.**
   Prepare maintained configuration, investigate extraction/query cost and
   account for baseline extraction errors while preserving security coverage.

Dependencies: cache and CodeQL use observability; parallel validation uses run
policy and caching. These five tasks are locally reviewable source work. C1 and
C2 establish their hosted results.

### Native and package milestones

6. **ui-commit-messages — Persistent full-message inspector.**
   One cohesive UI change across History, Compare and File History, including
   empty/long messages, accessible copy, retained navigation, small windows and
   visual QA. Requires Rust checks and native evidence on macOS and Linux.
7. **dist-macos-package — Verified macOS package inputs.**
   Establish exact executable identity, resources, complete licenses and archive
   metadata while preserving local ad-hoc packaging. Requires macOS package
   evidence; Developer ID credentials are unnecessary for this task.
8. **dist-linux-package — Verified Linux distribution bundle.**
   Establish exact identity, complete notices, archive integrity and isolated
   install/upgrade behavior using the existing Ubuntu x86-64 tarball. Requires
   Linux package evidence.
9. **dist-ci-artifacts — Downloadable, verified platform artifacts.**
   Depends on parallel CI and both verified packagers. Upload identifiable
   archives and validate the downloaded bytes. Requires package evidence plus
   hosted checkpoint C3.
10. **dist-release-workflow — Controlled tagged release assembly.**
    Depends on CI artifacts. Prepare exact tag/version/source checks, release
    assets, permissions and safe retry behavior. Local tests establish source
    readiness; C4 establishes actual release operation.
11. **dist-macos-signing — Developer ID and notarization integration.**
    Depends on macOS packaging and release workflow source. Prepare isolated
    signing, notarization, stapling and failure handling. Actual Apple-service
    and distribution-package verification belongs to C4.

The combined artifact task requires both platform package attestations. Missing
macOS machine access therefore postpones combined artifacts and downstream release
workflow preparation; missing Apple signing credentials does not. Local Linux
package work remains independent.

Existing distribution-notice gaps are another prerequisite: the current records
identify two Linux and six macOS gaps. Platform package tasks may establish
local-development packaging and correct strict distribution refusal while C0 is
open. Public binary uploads/releases must wait for C0; missing notices are never
silently treated as complete. The package scopes include supplemental notice
records so authoritative resolutions can be incorporated when available.

The final two tasks intentionally promise tested workflow preparation. They do
not require workers to possess signing secrets or publish a release, and their
acceptance does not close the distribution milestone.

## Visual direction

The title leads; the message follows immediately; author, date, hash and parent
controls form compact supporting information. Use existing GitTurtle type,
semantic colors, spacing and density. Preserve readable paragraphs and give the
message enough breathing room without turning the inspector into a set of cards.

An empty body adds no filler. Long content scrolls with a visible scrollbar; the
ordinary inspector still reserves at least half its height for changed files.
File History keeps usable revision navigation and path/rename context. Copy and
recovery controls remain discoverable and accessible at narrow widths and larger
text sizes.

Acceptance includes actual screenshots and interactions in light/dark themes and
both densities, short and very long messages, merge commits, rapid selection,
Compare/Back and File History. Large allowed messages must not freeze text layout.
A new commit starts at its message beginning; returning to the same inspection
preserves the appropriate context. The task also updates the older design wording
that explicitly requires the Message disclosure and two-line title clamp.

## Dated baseline and performance targets

Observed September 15, 2026; these are existing-run measurements, not validation
of the future queue:

- [Quality PR run](https://github.com/FernandoX7/GitTurtle/actions/runs/34992350894):
  32m40s from creation to completion. macOS Rust took 31m18s:
  approximately 13m18s test compilation, 2m actual test execution, 5m47s Clippy and
  9m48s release compilation. Linux Rust took 23m25s:
  approximately 10m49s test compilation, 30s execution, 4m08s Clippy and
  7m03s release compilation.
- [Duplicate push run](https://github.com/FernandoX7/GitTurtle/actions/runs/34992342236)
  validated the same branch/head alongside the PR, repeating approximately
  54 minutes of combined Rust runner time.
- [CodeQL Rust job](https://github.com/FernandoX7/GitTurtle/actions/runs/34992342508/job/104459875718):
  22m43s, including approximately 11m06s extraction, 1m15s finalization and
  9m49s queries. Logs reported 600 files extracted without error and four with
  errors despite job success; those errors are an investigation lead.
- Quality had no Cargo/build caches. The inspected cache inventory contained
  CodeQL JavaScript/Python overlay databases.
- No published releases or checked-in release workflow existed. Linux bundle
  installation was checked in Quality; macOS bundling was not.

Initial engineering targets are sub-two-minute inexpensive feedback and a
sub-ten-minute warm Quality critical path on ordinary product PRs. These are
targets to validate, not promised results or permission to reduce coverage.
Measure the full required-check path as well: CodeQL can remain the limiting job
after Quality improves.

C1/C2 must demonstrate a real improvement against comparable pre-change runs,
identify any cold-run or runner-minute regression and account for retained
coverage. If the targets are missed, investigate the largest remaining measured
cost and return a concrete tuning proposal to the coordinator; do not silently
change criteria or declare the initiative perfect. Record sample count and
median/tail honestly; a small sample is not a reliable production p95.

Relevant primary references checked during the investigation:
[Rust caching](https://github.com/Swatinem/rust-cache),
[GitHub cache boundaries](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching),
[nextest build reuse and partitioning](https://nexte.st/docs/ci-features/partitioning/),
[required-check behavior](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks),
[CodeQL Rust build behavior](https://docs.github.com/en/code-security/reference/code-scanning/codeql/build-options-for-compiled-languages).
Implementation tasks must recheck evolving tool/platform details before adopting
them. Standard hosted runners are free for this public repository; larger runners
and storage have separate limits/policies, so paid infrastructure is not a default
assumption.

## Coordinator checkpoints

These are mandatory completion conditions, not additional schema fields or
verification-only runner tasks. The controller has no hosted-CI, repository-admin
or publication attestation kind. Record sanitized evidence in the relevant CI or
release guide after integration; do not relabel local fixtures as hosted results.

### C0 — Complete distribution notices

- Reconcile the exact locked target inventories with the known
  [Linux notice review](../licenses/linux-notice-review.md) and
  [macOS/Linux validation record](../validation.md#september-14-public-launch-preparation).
  Linux strict collection currently documents unresolved mac 0.1.1 and the Rust
  ufbx 0.11.3 wrapper; other targets have additional gaps.
- Preserve authoritative, version-matched full notices and immutable
  URL/revision/checksum provenance in the existing supplements. A Cargo license
  declaration or a different component's notice is not a substitute. If required
  information remains unavailable, retain the explicit blocker; upstream
  outreach needs its own authorization.
- Run target-specific strict collection with --require-complete before any
  public binary artifact or release. Local-development packaging and verified
  refusal are useful source evidence while this remains open. A dependency
  replacement requires a separately scoped reviewed change and its appropriate
  Rust/native/vendor evidence; this queue does not silently authorize that swap.

### C1 — Hosted Quality and merge protection

- Exercise PR/main/manual runs, fork contribution behavior, superseding pushes,
  docs/site/tooling-only changes and conservative product/vendor/workflow routing.
- Demonstrate one intended PR run and no duplicate branch-push Quality run.
  Exercise required-job failure, cancellation and expected/unexpected skips.
- Collect comparable cold/warm samples on each standard platform, including
  cache restore/save overhead, invalidation cases, actual test execution, queue
  time, critical path and total runner minutes. Verify stale caches cannot produce
  a false success.
- Once replacement checks exist and pass on the intended revision, migrate branch
  protection from the current Rust platform names to the verified stable gate.
  Preserve CodeQL security rules and existing review/conversation requirements.
  Keep a concrete rollback route and remove transitional checks in a reviewed
  follow-up only after the migration is proven.

### C2 — Hosted CodeQL and full merge time

The maintainer retired CodeQL on September 15, 2026. That supersedes this
checkpoint's CodeQL setup, coverage and stage-timing conditions; they are not
passed, as recorded in the [CodeQL retirement](../ci-codeql.md#initiative-scope-amendment).
As of September 23, 2026, `Quality gate` is the only required check on main, so
the full merge time is Quality's creation-to-gate time. That comparison stays
open: it needs repeated hosted samples against the dated baseline above, and it
must identify the removed CodeQL coverage instead of counting it as faster
scanning. The original conditions follow for the record.

- Coordinate default-to-checked-in setup without an unprotected gap or permanent
  duplicate scanning. Check the effective languages, queries, threat model and
  required result on PR and main revisions.
- Compare analyzed source coverage and resolve or explicitly diagnose the four
  baseline extraction errors. Do not hide product/maintained-vendor coverage to
  lower timings.
- Record extraction, finalization, queries and end-to-end required-check time
  before/after. A faster Quality workflow alone does not complete this checkpoint.

### C3 — Download and installation

- Complete C0 before enabling public binary artifact uploads.
- Download artifacts from the actual hosted run; independently verify archive
  checksums, source/version/target, executable identity and complete licenses.
- Extract/install into disposable locations and perform the affected native smoke
  on macOS and Linux. Record the exact OS/architecture/session and signing status.
- Confirm that ordinary failures retain useful diagnostics and that PR artifacts
  cannot be selected as trusted release inputs.

### C4 — Release rehearsal and macOS distribution

- Complete C0 for every platform included in the chosen public release.
- Establish the intended version, supported platform set, release commit and
  concrete artifact list. Configure the reviewed release environment and narrowly
  scoped credentials through the authorized account owner.
- Rehearse release assembly and retry/partial-failure behavior against the exact
  source/assets. Verify version/tag agreement, checksums, notices and provenance.
- For macOS distribution, supply Developer ID/notarization credentials, sign the
  exact package, notarize/staple it and verify the downloaded result with signature,
  Gatekeeper and native launch checks. Keep this condition open until real evidence
  exists; local ad-hoc or simulated success cannot replace it.
- Confirm the concrete publication context before publishing. Creating this queue
  does not publish a release or authorize arbitrary future version/tag choices.
  Linux-only delivery may proceed with an explicitly chosen platform set while
  Apple credentials are unavailable; promised macOS assets must not disappear.
- Update user-facing download instructions only after actual assets exist.

## Running with the existing controller

Validate a queue with:

```sh
python3 scripts/agent-loop.py validate
```

Follow the existing [run and resume procedure](README.md#run-the-controller) for
any future queue. As of September 23, 2026, this queue has no remaining runnable
tasks: ten are integrated on `main` and `ci-codeql` is superseded. Its open
checkpoints C0 to C4 need a real Mac, Apple credentials and hosted samples, which
a controller run cannot supply. For a future run, choose the session's
model/effort explicitly and supply task, attempt and time limits when starting;
no run or schedule is created by this feature intake.

The controller requires private record files/directories and rejects writable
non-sticky ancestors. Use a private source clone whose ancestor permissions meet
that requirement, and set a private creation mask (umask 077) in the launch/test
shell. The environment inspected during intake has group-writable Codex worktree
ancestors, so this checkout needs a suitable private clone before a real run;
changing the mask does not repair existing directory permissions. Do not change
shared Codex directory permissions or weaken the runner checks to force a launch.

Native/package attestations need a separate capable owner. The runner disables
desktop tools in its child sessions and accepts only candidate-bound evidence.
It continues other eligible work when a candidate is waiting, but a later accepted
commit makes an earlier pending candidate stale. Merely setting a task cap does
not guarantee it will stop at the first pending candidate: the cap counts accepted
tasks.

Before collecting expensive evidence, stop the run cleanly, inspect status and
ensure candidate base equals accepted head. Register evidence only while the run
is not holding its execution lock, then resume. Process pending native/package
candidates deliberately: once one is accepted, resume/rebuild any stale sibling
before collecting its evidence, allowing sufficient attempts. The current runner
has no task-selection switch; do not invent one or edit its immutable snapshot.

The native/package owner can be an interactive agent with the required machine
access. Missing macOS access, locked desktops or credentials leave the relevant
checkpoint open while eligible source work continues. These are existing runner
boundaries, not reasons to remove acceptance criteria.

The controller's automatic tooling gate runs its existing Python suite; it does
not automatically discover new CI helper tests or lint Actions YAML. Contracts
therefore require implementers to run focused checks, retain their results and
wire them into CI. The independent verifier inspects that evidence.

This intake changes only the specification and its explanation. Workers may not
edit task grading rules, agent policy or controller code. Accepted candidates stay
in the isolated accepted clone until the coordinator reviews and transfers them;
no automatic push, branch-rule update or publication occurs.
