# CI validation, measurement and diagnostics

[Quality](../.github/workflows/quality.yml) validates development tooling and the
Rust workspace on macOS and Linux. The standard-library
[measurement helper](../scripts/ci/metrics.py) makes its time and failure costs
inspectable. The helper reports evidence; the workflow and Rust setup action own
run policy and optional dependency caching. Collection does not retry jobs, publish
reports or update repository settings.

The source patch prepares local tooling and workflow instrumentation. Hosted
behavior and improvement remain part of
[coordinator checkpoint C1](development/commit-inspector-and-ci.md#c1--hosted-quality-and-merge-protection).
A checked-in workflow or passing fixture is not evidence of an executed hosted run.

The [September 15 hosted observations](benchmarks/2026-09-15-ci.md) record measured
passes, failures, routing behavior and the remaining cache/performance evidence.

## Events and required results

Quality owns normal validation: one `pull_request` run per opened, reopened or
synchronized PR, pushes to `main`, and explicit `workflow_dispatch`. Branch pushes
do not also start Quality. A branch without a PR can use manual dispatch. PR runs
test GitHub's generated **merge commit**, including its interaction with the base;
they do not claim a separate branch-head build. Main/manual runs test the selected
checkout. Main pushes and manual dispatch request all lanes; manual dispatch does
not replace PR-associated required checks. See GitHub's [event semantics](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request).

The concurrency group is specific to the PR number. A newer update cancels the
older run for that PR; unrelated PRs remain independent. Main pushes and manual
runs use unique run IDs and do not cancel each other or PR validation. A cancelled
run cannot satisfy the successful aggregate. These are the configured
[concurrency semantics](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency),
pending real superseding-run evidence at C1.

### Conservative change routing

The mandatory `Changes and CI policy` job checks guidance, all focused CI helper
fixtures and every checked-in Actions workflow with the pinned linter. The
[classifier](../scripts/ci/changes.py) then produces boolean product, tooling and
website outputs plus a versioned JSON plan. Downstream jobs consume those flags;
the final gate verifies the same recorded plan and actual job results.

For pull requests:

- Documentation and documentation images under `docs/`, plus the listed top-level
  contributor/design documents, use the mandatory inexpensive checks.
- `website/` changes also call Website's existing Python input check and JavaScript
  syntax check. Website is a reusable validation workflow with separate manual
  dispatch; Quality owns its PR/main triggers, avoiding another duplicate run.
- CI helpers, development-controller files, agent guides and development contracts
  also run the existing macOS and Ubuntu development-tooling matrix, including all
  CI helper tests.
- Rust, manifests, the lockfile/toolchain, native assets, vendored inputs, build or
  package scripts, workflow/repository-policy changes and unrecognized paths run
  **all** lanes. Rust keeps formatting, locked workspace tests with doctests,
  strict all-target Clippy, release compilation and both platform package checks.

Routing uses complete local Git diffs after `checkout` fetches history, avoiding
GitHub path-filter/API changed-file limits. PR classification checks the observed
merge parents against event base/head identities, compares the merge-base to the
PR head and also the base to the exercised merge tree. Main pushes validate the
event's before/after identities and request full coverage, including packages. A deleted or moved product file still requests product
coverage: `--no-renames` reports both sides of moves, including deletions.
Missing objects, stale/malformed event data, empty comparisons, unknown events,
ambiguous paths or comparisons above 10,000 paths/4 MiB select full validation.
Each local Git read has a 60-second timeout. No API or network fallback is needed.
Unknown inputs are expensive by design until their narrower coverage is reviewed.

Paths use NUL delimiters and never become shell fragments or workflow-output
lines. Only fixed flags and a bounded plan are output. PR titles, branch names and
other contributor fields are not interpolated into commands. Quality and the
Website caller/callee use only `contents: read`; checkout does not persist its
credentials. There is no `pull_request_target`, secret inheritance, publication,
Pages permission or signing credential. External-fork approval remains controlled
by GitHub's existing repository policy; C1 must observe a real approved fork run.

### Stable gate and compatibility

`Quality gate` runs with `always()` after classification, formatting, the debug
and release platform matrices, development tooling and the Website call. It
requires successful classification and success for each required lane. A skipped lane is accepted only when the recorded classification says it is
unneeded. Failures, cancellations, unexpected skips, absent jobs, mismatched
outputs or unknown classification versions fail. Each matrix keeps
`fail-fast: false` so one platform failure does not discard the other platform's
diagnostics. A checkout/evaluator failure also leaves the gate unsuccessful.

The current required names, **`Rust · macos-15`** and **`Rust · ubuntu-24.04`**, remain
as always-running compatibility jobs. They mirror the complete Quality gate,
while the actual platform work is named `Rust tests and Clippy · <platform>` and
`Rust release · <platform>`. `Rust formatting` checks the workspace once on Ubuntu.
These short compatibility jobs execute on Ubuntu; the names preserve required-check
identity, not a claim that their shell step compiles on macOS. A product change
cannot pass them unless both actual macOS/Ubuntu Rust matrices, formatting and
other required lanes succeeded. A docs-only change can pass after its justified
skips. This avoids GitHub's [skipped-job success behavior](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
silently bypassing a failed dependency.

No repository settings change is performed by this source patch. C1 must first
exercise actual positive/negative PR cases, cancellation, routing, forks and
main/manual runs. Once the new gate has passed on the intended revision, the
coordinator prepares and obtains authorization for this scoped migration:

1. Read the current main protection and preserve its strict/up-to-date setting,
   GitHub Actions app binding and review/conversation requirements. CodeQL was
   separately retired by the maintainer; do not reintroduce its inactive rule.
2. Add the observed `Quality gate` name/app binding alongside the two current
   Rust names using the [required-status-check endpoint](https://docs.github.com/en/rest/branches/branch-protection#update-status-check-protection).
   Verify the binding and actual positive/negative behavior.
3. Replace the old required names with the verified gate only after those checks.
   Remove compatibility jobs in a separately reviewed follow-up after the live
   migration is established.

Rollback restores the exact latest pre-migration required-check set through that
same scoped endpoint, with strictness and unrelated protection unchanged. Preserve
the compatibility jobs until the migration is proven. Source regressions use a
reviewed revert; do not remove protection to clear a failed check. The independent
[security review](development/security-review.md) is a development acceptance
requirement. It is not an automated GitHub status check, and a successful Quality
gate alone does not establish that the review happened.

## Parallel validation and coverage

After classification, the selected jobs have no build dependencies on each other:

- `Rust formatting` checks `cargo fmt --all -- --check` on Ubuntu with the pinned
  workspace toolchain. It installs no native packages and restores no target cache.
- `Rust tests and Clippy · macos-15` and `· ubuntu-24.04` each restore the **debug**
  cache, run `cargo test --locked --workspace --timings`, then
  `cargo clippy --locked --workspace --all-targets --timings -- -D warnings`.
  Cargo's default [test selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
  includes unit/integration tests and doctests. There is no package, test-name,
  target or feature filter that removes the existing platform-conditional tests.
- `Rust release · macos-15` and `· ubuntu-24.04` each restore the **release** cache
  and build `gitturtle` with `--release --locked --timings` and an explicit platform
  target. Each job packages that executable without rebuilding it. Linux checks
  archive extraction, isolated installation, installed bytes, notices, desktop
  entry, dynamic libraries and the expected no-display launch failure; macOS
  checks bundle identity and ad-hoc signatures. Complete license notices enable
  a same-run artifact upload/download and independent payload verification. Known
  C0 notice gaps explicitly withhold public binaries while development package
  checks continue; other collection or verification errors fail the job. See the
  [artifact runbook](ci-artifacts.md) for exact gates and evidence requirements.
  These checks do not establish an interactive native desktop or notarized build.
- Development-tooling checks still cover both OSes; Website remains reusable.
  The mandatory policy job runs all CI helper fixtures and pinned Actions-aware
  lint on every workflow even for documentation-only changes.

The existing change classifier owns selection, the setup action owns each cache's
lifetime, and the gate owns the final result. No new cross-job build artifact or
mutable shared target directory is introduced. Debug/release use separate fresh
runners and matching setup/finish profiles; a finish cannot remove another lane's
inputs. Package/artifact consumers must precede finish because its successful-main
cleanup can remove the application executable. The final gate requires the
formatter and **both** platform matrices, in addition to tooling/Website according
to the recorded plan. A successful debug matrix cannot cover a failed, cancelled,
missing or unexpectedly skipped release matrix. Legacy platform check names keep
mirroring this complete result until C1's verified protection migration.

### Why two compilation lanes per platform

The September 15 baseline spent roughly 13m18s (macOS) and 10m49s (Linux) compiling
workspace tests, followed by only 120.672s and 30.049s through the final doctest
result. The serial Clippy steps took 5m47s and 4m08s; optimized builds added 9m48s
and 7m03s. These are observations from the
[baseline PR run](https://github.com/FernandoX7/GitTurtle/actions/runs/34992350894),
not predictions for the new graph.

Release compilation has a distinct profile and can overlap debug work. Keeping
Clippy with tests avoids another cold debug dependency build and another cache
reader/writer family; Clippy remains a separate measured step with strict warnings.
A failing test stops that debug job before Clippy, as before; the independent
release job continues to retain its diagnostics. There is no automatic retry.
The short post-compilation test phase does not justify nextest, partitions or
additional test runners, so this change introduces none. If later measurements
show Clippy still dominates the warm critical path, compare a separate Clippy lane
against the extra cold compilation, setup, transfer and runner time before adopting it.

Each Rust matrix has two fixed OS entries, `fail-fast: false`, `max-parallel: 2`
and a 45-minute job timeout. Thus at most four compilation runners are requested
per Quality run, plus the independent inexpensive checks. Formatting is bounded
at five minutes, policy and the final gate at five, tooling at ten and compatibility
at two. GitHub may queue those jobs under the repository's existing concurrency
limits; matrix bounds do not promise simultaneous starts. See the documented
[matrix controls](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations).
Failed commands still retain their exit code, sanitized logs and available Cargo
build timing reports in the three-day diagnostics artifacts. A cancelled runner
may stop before artifact upload; do not claim a missing report was retained.

### C1 fan-out experiment and acceptance still open

The new `debug`/`release` identities do not reuse the earlier combined-profile
cache. Verify actual misses on the first cold experiment; an earlier combined
entry is not evidence that these lanes are warm.
Compare like-for-like product inputs and pinned runner/toolchain identities using
the [measurement procedure](#reproduce-coldwarm-and-prmain-measurements), then record:

1. Creation-to-format/policy feedback, per-job queue delay, native/toolchain setup,
   cache restore interval and observed hit state, test compilation/execution,
   Clippy, release/package time and the completed post-job save/cleanup interval.
2. The retained payload subset, compressed sizes and any downloads-only fallback
   under each 1.5 GiB budget. Separate cold PRs from a successful trusted-main
   seed and genuinely restored warm PR/main samples on **both** platforms.
3. Quality creation-to-completion and summed runner intervals across **all** jobs,
   including inexpensive jobs, cache post-actions and compatibility checks. More
   simultaneous jobs can increase queue contention and runner minutes even if
   the visible critical path falls. Duplicate Linux package setup/downloads and
   profile-specific build scripts can increase cold cost; report this regression.
4. The full merge path including every active required check. A faster release
   lane alone does not prove a faster merge. Record sample counts, median/tail and
   unavailable observations; a small set does not establish production p95.
5. Real negative PR cases for each Rust phase: failure, cancellation and unexpected
   skip, plus docs-only justified skips. Verify the actual gate and both legacy
   required names remain unsuccessful for a failed required platform. Local result
   fixtures establish the evaluator's contract, not GitHub's matrix execution.

No hosted improvement is established by this source patch. If cold overhead,
cache eviction or warm critical-path results miss the initiative targets, propose
measured tuning while retaining tests, doctests, strict lint and platform coverage.

## Collect a report

Use Python 3.11 or newer from the repository root. An exported input works offline:

```sh
python3 scripts/ci/metrics.py report --input export.json --format markdown
python3 scripts/ci/metrics.py report --input export.json --format json
```

For a reproducible synthetic example, use the checked-in fixture:

```sh
python3 scripts/ci/metrics.py report \
  --input scripts/ci/tests/fixtures/representative-runs.json --format markdown
```

Fixture timings demonstrate the model; they are not hosted measurements.

For an authorized read of a particular repository and run, name both explicitly:

```sh
python3 scripts/ci/metrics.py report \
  --repo OWNER/REPO --run-id RUN_ID --format markdown
python3 scripts/ci/metrics.py report \
  --repo OWNER/REPO --run-id PR_RUN_ID --run-id PUSH_RUN_ID --format json
```

Replace the uppercase placeholders with actual identifiers. Public reads can use
an unauthenticated request; where needed, set a read-authorized `GH_TOKEN` or
`GITHUB_TOKEN` through your existing secret mechanism. `GH_TOKEN` takes precedence.
Never place a token in a command argument or an export. Collection is limited to
ten runs, 500 jobs per run, 100 steps per job, a 16 MiB export/aggregate response
budget, 2 MiB per API response, five
100-job pages per run and a 20-second network timeout. Network mode makes only
bounded read-only requests; it does not infer a repository from Git remotes or
enumerate all historical runs. It uses the supported
[GitHub API version](https://docs.github.com/en/rest/about-the-rest-api/api-versions)
`2026-03-10`, requests
the named run and jobs for that run's current attempt, and refuses redirects.
It does not download logs or artifacts. To inspect a historical attempt, supply
its exported data. API and HTTP transport failures, including truncated responses,
produce a concise error and no report. Export mode never contacts GitHub.
Neither report mode appends to an Actions summary or uploads an artifact;
redirect output only to a deliberately chosen location. `--format json` emits the
normalized report, not another input export. Keep the original export when an
exact offline reproduction is needed.

Export files use the versioned envelope below. `run`, `jobs` and `steps` retain the
corresponding GitHub Actions REST field names. The omitted timestamps in this
minimal example produce unavailable durations, not example timing evidence.

```json
{
  "schema_version": 1,
  "runs": [
    {
      "repository": "owner/repository",
      "run": {
        "id": 123,
        "workflow_id": 456,
        "name": "Quality",
        "path": ".github/workflows/quality.yml",
        "run_attempt": 1,
        "head_sha": "0123456789012345678901234567890123456789",
        "event": "pull_request",
        "status": "completed",
        "conclusion": "success"
      },
      "jobs": [],
      "jobs_complete": false
    }
  ]
}
```

Set `jobs_complete` only when all jobs for that attempt were exported. Keep the
attempt number: a rerun is a new observation, and jobs from different attempts
must not be combined. Keep original timestamps and statuses rather than filling
missing fields. Optional `jobs[].steps[].log` text supplies Cargo completion and
test-harness messages, up to 1 MiB per step. The parser normalizes ANSI CSI
sequences, including the literal `^[[...m` caret form observed in saved terminal
exports. Keep the raw export for replay; the report does not rewrite it.
Optional `jobs[].steps[].cache` supplies
measured `restore_seconds`, `save_seconds`, `hit` (boolean) and `size_bytes` fields;
omit unavailable values or use `null`. Cache values are explicit evidence, not
inferred from a cache-like step name. REST responses alone contain neither logs
nor these cache measurements, so the breakdowns can remain unavailable even when
step durations are known. Supply runner labels when available; a job name alone
does not establish its operating system or architecture. Optional
`jobs[].runner` fields `os` and `architecture` can preserve actual runner context. Save source exports privately and
review generated summaries before sharing; raw API data and logs can contain
repository details that do not belong in a public report.

## Read the timing model

Every report keeps repository, workflow ID/name/path, run ID, attempt, commit and
event together. Missing workflow fields remain unavailable. Job and
step conclusions remain visible for successful, failed, cancelled and unfinished
runs. Missing evidence is represented as unavailable (`null` in JSON), with the
partial observations retained where possible. Invalid timestamps, reversed
intervals and conflicting duplicate snapshots produce a clear error.

- **Queue delay** is the interval from run creation to the first executed job start, for
  a first attempt with complete job coverage and known starts for every executed
  job. A missing executed-job start makes queue delay unavailable. This includes scheduling and runner
  wait before the first job; it does not isolate later jobs' dependency/runner
  waits. `run_started_at` alone does not establish runner allocation.
- **Job and step duration** is the recorded completion timestamp minus the start
  timestamp. Missing timestamps and unfinished work do not become zeroes.
  Skipped work has no measured execution cost, even when the API supplies
  placeholder timestamps; it does not move the execution start or end.
- **Observed critical path** (`critical_path_seconds`) spans the first executed job start
  to the last executed job completion. It includes gaps between jobs. It is the observed
  execution span, not a reconstruction of a dependency graph from job names.
- **Elapsed time** (`elapsed_seconds`) spans run creation to the last job
  completion for a completed first attempt with complete jobs. The boundary is
  the last job, not an exact workflow-completion timestamp. The REST run
  `updated_at` field is not used as completion evidence. Rerun queue and elapsed
  values are unavailable because creation belongs to the original run.
- **Runner time** (`total_runner_seconds`) adds measured job intervals to
  describe resource consumption. Concurrent jobs both consume runner time. Their
  sum must never be presented as workflow elapsed time or a critical path.
  `runner_busy_seconds` instead merges overlapping intervals to measure time
  with at least one runner active. `known_runner_seconds` retains the measured
  subset when missing intervals prevent a complete total.
- **Cargo compilation** (`cargo_compilation_seconds`) sums durations from Cargo
  `Finished` profile messages. **Test harness time** (`test_harness_seconds`) sums
  the durations explicitly reported by completed test harnesses. These distinguish
  build cost from measured test work without subtracting compilation from a step
  to invent execution time. Harness totals exclude startup and doctest compilation;
  parallel harness totals are not a wall-clock span. Missing markers leave the
  corresponding value unavailable; a failed or interrupted command can still have
  a known enclosing duration and a partial total from completed markers.

For example, two jobs that both run from 12:00 to 12:10 consume twenty runner
minutes in a ten-minute observed workflow window. Do not add run durations across
simultaneous push and PR workflows and label the result “time to feedback.” Record
their wall-clock span separately from the duplicated runner cost.

The same repository/run/attempt supplied twice with matching measurements is one
observation; conflicting snapshots are rejected. Distinct push and PR runs for
the same commit and workflow ID are potential duplicate work: retain both and
measure their resource cost. Other workflows, including historical CodeQL, and other events
are separate validation. Without a workflow ID, the report retains each run's
measurements but makes no duplicate-work claim; matching names or paths alone
do not establish workflow identity. Keep reruns separate
from first attempts when comparing success latency and failure/retry cost.

Runner seconds are a duration measure, not a monetary bill. Do not infer pricing,
CPU architecture, cache hit, completion time or successful coverage from absent
fields. A partially exported or active run cannot establish final workflow elapsed
or total runner cost.

## Workflow summaries and retained diagnostics

Quality wraps guidance checks, development-tooling tests, CI measurement fixtures,
formatting, workspace tests, Clippy, release compilation and Linux package checks
with `measure`. The wrapper preserves the command's result while recording timing
and a bounded, sanitized log. The summary step runs after failures and lists the
available measurements. Each command produces `<name>.json` and `<name>.log`.
Logs retain at most 1 MiB, with a truncation marker when older output is dropped;
individual lines over 16 KiB are omitted. Tests, Clippy and release builds request
Cargo `--timings`. The helper copies a newly produced timing report of up to 2 MiB
as sanitized plain text, for example `tests-timing.txt`, instead of retaining an
executable HTML artifact. Missing or oversized timing artifacts remain unavailable.
The uploaded diagnostic artifact expires after **three days**. Quality uses
the pinned [upload-artifact v7.0.1 action](https://github.com/actions/upload-artifact/releases/tag/v7.0.1);
its [inputs](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/action.yml)
support the explicit retention and exclusion of hidden files used here.
Each command's JSON captures the run, commit, event and runner context alongside
its measurements. In PR jobs, `GITHUB_SHA` can identify the tested merge commit;
the REST report's `head_sha` identifies the run's head. Retain both when comparing
observations. Use a fresh diagnostics directory for a new investigation; repeating
a command name replaces that command's previous files, including after a launch
failure, while other command records remain.

The summary reserves fields for cache restore duration, save duration, hit/miss,
size and build timing artifacts. Quality's setup action supplies explicit restore
and budget records; compressed size and completed post-job save duration need
hosted evidence. A blank field is not a cache miss, and no restore/save duration
is assumed to be zero. Supply actual restore/save evidence and invalidation
context before comparing cache benefit.

Use the same helper locally when investigating a command:

```sh
python3 scripts/ci/metrics.py measure --name tests --directory .local/ci-metrics \
  -- cargo test --locked --workspace --timings
python3 scripts/ci/metrics.py summary --directory .local/ci-metrics
```

These commands execute the explicitly supplied command and write diagnostics in
the named directory; they are distinct from the read-only collector. A test failure
must still fail the command and workflow. Cancellation or runner loss can prevent
post-job summary and artifact upload entirely; use the run/job API record to keep
that missing evidence visible. The summary is written before upload and post-job
actions; it cannot establish their success or duration.

Keep secrets out of measured command arguments and output. The helper's bounded
sanitized diagnostics reduce accidental exposure; they do not justify logging
a credential. Authorization and Proxy-Authorization values are redacted through
the end of their line, including Basic, Bearer and parameterized schemes.
Credential assignments use the same conservative boundary, including quoted values
with spaces or escaped quotes and compound keys such as `access_token`,
`refreshToken`, `client-secret` and `api_key`. Absolute
paths are redacted through the next quote, markup delimiter or newline so path
components containing spaces cannot leave a private suffix. This conservative
redaction can also omit diagnostic text following a path on the same line.
Publish only the intended diagnostic files, not arbitrary workspace
contents, environment dumps, raw exported API responses or a whole target
folder. Preserve timing evidence before its retention period expires if it is
needed for a checkpoint, after reviewing it for credentials and private paths.

## Reproduce cold/warm and PR/main measurements

Select a representative revision and a comparison revision with equivalent checks
and product coverage. Record full source SHA, run ID/attempt, event, workflow
revision, required-check names, runner OS/architecture/image, toolchain versions,
lockfile identity, job matrix, cache identity/state and relevant input changes.
Keep platform results separate. A moving hosted image or different toolchain is a
comparison variable, not an invisible improvement.

1. Choose a sampling plan before inspecting results. Start with at least five
   completed first-attempt samples per platform, event and cache condition where
   practical. Record the actual count if cost or availability prevents that plan;
   five samples are a starting comparison, not a reliable production p95.
2. Measure **cold** runs with evidence that the applicable dependency/build cache
   was absent or deliberately isolated. Record the mechanism used; elapsed time
   alone does not establish cold state. Cache mutation or workflow dispatch needs
   its own authorized context, outside the collector.
3. Measure **warm** runs only after a compatible preceding run has saved a cache
   and the next run establishes its restore result. Record exact-hit/fallback/miss,
   key and compatibility inputs, size, restore/save time and remaining compilation.
   A repeated run with no build cache is still a no-cache run.
4. Collect PR and main-push observations separately. Include manual runs as a
   separate event when testing the workflow. Keep first attempts, reruns,
   cancellation and failure categories visible rather than discarding expensive
   unsuccessful work. Identify push/PR duplication by workflow ID and head SHA
   while retaining its actual runner cost.
5. Save the raw observations, then report sample count, median and observed maximum
   for queue, end-to-end time, observed critical path, runner time, compilation,
   test execution and cache overhead. State how many observations were unavailable
   for each metric. Use the same boundary and sample selection before and after.
6. If reporting a percentile, name its calculation and sample size. With a small
   sample, report the observed maximum as an observed maximum; do not relabel it a
   reliable p95. For a larger sample, a nearest-rank p95 is the value at position
   `ceil(0.95 * n)` in sorted measurements. Keep raw samples so another reader can
   reproduce the statistic.

For failures, record the failing step, time until useful failure feedback and
runner time consumed before failure, together with retained diagnostics. Record
cancelled and queued runs as their actual states; they are not zero-cost successes.
Required checks can overlap. Measure the full required-check creation-to-final
completion window separately from each workflow and from summed runner minutes.

C1 must also exercise forks, superseded pushes, expected/unexpected skips and
required-job failures. This task adds observation; later run-policy and cache
changes must establish those behaviors on hosted Actions without losing coverage.

## Dated baseline: September 15, 2026

These observations come from the
[September 15 investigation](development/commit-inspector-and-ci.md#dated-baseline-and-performance-targets).
They were measured before this instrumentation and are not validation of this
patch. Approximate compilation splits retain the investigation's precision.

- [Quality PR run 34992350894](https://github.com/FernandoX7/GitTurtle/actions/runs/34992350894)
  took **32m40s from creation to completion**. macOS Rust took **31m18s**, including
  approximately **13m18s test compilation**, **2m test execution**, **5m47s Clippy**
  and **9m48s release compilation**. Linux Rust took **23m25s**, including
  approximately **10m49s test compilation**, **30s test execution**, **4m08s Clippy**
  and **7m03s release compilation**.
- [Push run 34992342236](https://github.com/FernandoX7/GitTurtle/actions/runs/34992342236)
  repeated validation for the same branch/head alongside the PR, consuming
  approximately **54 additional Rust runner minutes**. That is duplicated resource
  time, not 54 extra minutes of elapsed PR latency.
- [CodeQL Rust job 104459875718](https://github.com/FernandoX7/GitTurtle/actions/runs/34992342508/job/104459875718)
  took **22m43s**, including approximately **11m06s extraction**, **1m15s
  finalization** and **9m49s queries**. Its successful conclusion accompanied four
  file extraction errors; success alone does not settle coverage.
- Quality had **no Cargo/build caches**. The inspected cache inventory contained
  CodeQL JavaScript/Python overlay databases. These observations contain no new
  cold/warm sample distribution or measured cache benefit.

The initial targets are **under two minutes for inexpensive feedback** and
**under ten minutes for the warm Quality critical path** on ordinary product PRs.
They are targets, not measured results or authorization to reduce validation.
Record cold-run and runner-minute regressions alongside any warm improvement.
These baseline runs predate CodeQL retirement. Separate that scope change from
cache/fanout improvements when comparing the full required-check completion time.

### Later measurements and C1

Pending: candidate-bound hosted PR/main/manual observations, comparable cold/warm
samples on both platforms, cache overhead and invalidation evidence, preserved
failure diagnostics, and measured improvement. No later hosted timing is claimed
by this guide. Add sanitized run links, source identities, raw-sample locations,
counts/statistics and limitations here when the coordinator establishes them.

## Local verification

The controller's tooling profile does not automatically discover these CI helper
tests. Run them explicitly; Quality runs them in the mandatory inexpensive policy
job and on both selected development-tooling platforms.
From the repository root:

```sh
python3 -B -m unittest discover -s scripts/ci/tests -p 'test_*.py' -v
python3 -B -c 'import ast; from pathlib import Path; ast.parse(Path("scripts/ci/metrics.py").read_text())'
python3 -B scripts/ci/metrics.py --help
python3 -B scripts/ci/metrics.py report --help
python3 -B scripts/ci/metrics.py measure --help
python3 -B scripts/ci/metrics.py summary --help
python3 -B scripts/ci/changes.py classify --help
python3 -B scripts/ci/changes.py gate --help
```

Validate changed Actions YAML with
[actionlint v1.7.12](https://github.com/rhysd/actionlint/releases/tag/v1.7.12),
the version pinned by Quality. Its [Linux amd64 release archive](https://github.com/rhysd/actionlint/releases/expanded_assets/v1.7.12) SHA-256 is
`8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8`;
the workflow verifies that digest before execution. With that version available:

```sh
actionlint -version
actionlint -shellcheck='' -pyflakes=''
```

The explicit flags make this Actions validation independent of optional installed
ShellCheck/Pyflakes versions. JSON/YAML parsing alone does not validate Actions
expressions and contexts. Focused fixtures cover run states, missing evidence,
concurrent execution, skipped-job timestamps, duplicate observations, bounded
collection, HTTP transport failures and credential redaction (including Basic,
Proxy-Authorization, quoted values and compound keys); they do not establish
hosted timing or cache reuse.

Routing fixtures additionally cover docs/site/tooling/product/vendor/workflow
inputs, real Git renames and deletions, stale/missing merge data, literal hostile
filenames, malformed plans, required failures/cancellation/skips and successful
justified skips. For the small routing subset alone, run
`python3 -m unittest discover -s scripts/ci/tests -p 'test_changes.py'`.
These establish local policy behavior, not GitHub's execution of the DAG,
matrix-result association, fork approval or live branch protection; those remain
candidate-bound C1 evidence.

The CI fixtures place temporary files inside the checkout. For other tooling
tests that use the system temporary directory, use a checkout-local directory when
the session requires it: `mkdir -p .local/ci-observability/tmp`, then prefix the
test command with `TMPDIR="$PWD/.local/ci-observability/tmp"`.

### Source validation evidence

Validated September 15, 2026 on Linux x86_64 with Python 3.12.3, for the source
patch based on `076bb27ac1d94c84b5e4d5a8accff1195b15ff83`:

- The focused unittest command above passed **63 tests**. This includes skipped
  jobs, missing-start queue uncertainty, quoted and compound credential redaction
  including escaped and HTML-encoded Authorization headers in stdout/logs/build
  timing artifacts, historical exported-log replay, same-workflow duplicate
  detection, paths containing spaces, truncated HTTP
  responses, malformed Unicode, and child cleanup after output failure.
- Syntax parsing and all four help commands above passed. The test module also
  passed Python AST parsing.
- `actionlint -version` reported **1.7.12**; the exact linter command above passed.
- Bash syntax checks passed for all five multiline workflow commands.
- `python3 -B scripts/check-agent-guidance.py` passed its 25-file validation.
- `git diff --check` and local documentation link checks passed.

The existing controller suite was also run explicitly:

```sh
mkdir -p .local/ci-observability/tmp
TMPDIR="$PWD/.local/ci-observability/tmp" PYTHONDONTWRITEBYTECODE=1 \
  python3 -B -m unittest discover -s scripts/agent_loop -t scripts -p 'test_*.py'
```

The implementation sandbox run of **132 tests failed with 18 failures and 53
errors**: ancestor ownership and a controller Git-record destination outside the
writable checkout prevented protected-record and Git-fixture setup. That failure
is preserved in `.local/ci-observability/tooling-tests.log`. The controller
subsequently passed its guidance and tooling gates on candidate `1da53d7`, using
the proper private validation environment. No controller or permission controls
were changed. The integrated replay and redaction repairs passed the 63 helper
tests and workflow lint separately. Independent controller review remains paused
after two CLI provider failures; successful command gates do not establish task
acceptance. Interactive independent review and hosted C1 evidence are separate
requirements.

These are local source checks. Rust/native builds were not run for this CI helper
patch. The coordinator owns interactive candidate commits; the paused controller keeps
its original candidates and independent gate records.
Hosted C1 remains pending until candidate-bound observations exist.
Native, product-performance, package and vendor attestations are not required by
this tooling-only contract; controller-added requirements, if any, follow candidate
creation.

Implementation references: [GitHub workflow runs](https://docs.github.com/en/rest/actions/workflow-runs),
[jobs for a run attempt](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt),
and [Cargo build timings](https://doc.rust-lang.org/cargo/reference/timings.html).

## Bounded Rust dependency caching

Quality's [Rust setup action](../.github/actions/setup-rust/action.yml) owns native
setup and the lifetime of optional compilation caches. Each compilation job calls its
`setup` phase before validation and its matching `finish` phase **after every
consumer of `target`**, including package construction and installation checks.
Finish may remove compiled outputs to bound a successful main cache. Keep future
binary/artifact consumers before that boundary. The debug job always runs locked
workspace tests (including doctests) and strict all-target Clippy; the separate
release job always runs locked optimized compilation, regardless of a cache hit.
Formatting has no compilation cache or native setup.

### Tool choice and compatible reuse

The action pins
[Swatinem/rust-cache 2.9.2](https://github.com/Swatinem/rust-cache/tree/6323deb102c322ba6fcbdcafc7e3dddab59af2b6)
to `6323deb102c322ba6fcbdcafc7e3dddab59af2b6`, inspected September 15, 2026.
Its Node 24 implementation uses the GitHub cache service and removes nondependency
outputs before saving. No sccache, paid runner, toolchain change or product profile
optimization is introduced. The established action disables Cargo incremental
artifacts (`CARGO_INCREMENTAL=0`); workspace optimization, debug information, LTO
and codegen settings remain those in `Cargo.toml`.

The evaluated implementation's
[package selection](https://github.com/Swatinem/rust-cache/blob/6323deb102c322ba6fcbdcafc7e3dddab59af2b6/src/workspace.ts)
excludes every package beneath the workspace root, not only its three declared
members. Consequently GitTurtle's maintained local GPUI/Mermaid patches and the
application/core/preview crates rebuild after restoration. The cache reuses
registry dependency outputs and Cargo downloads when compatible; it does not
promise reuse of those expensive local libraries. The finish helper uses Cargo's
own whole-package cleanup for local path packages rather than retaining their
fingerprints accidentally. This limitation and remaining vendor compilation cost
must appear in C1 measurements before deciding whether a different established
cache strategy is justified.

[Upstream key construction](https://github.com/Swatinem/rust-cache/blob/6323deb102c322ba6fcbdcafc7e3dddab59af2b6/src/config.ts)
separates OS/architecture, installed Rust compiler release/host/commit identities,
and Cargo/Rust/native compiler flags. The local compatibility prefix additionally
hashes:

- The selected output family: `debug` for tests/doctests and Clippy, `release` for
  optimized compilation and packaging. These never restore one another's payload.
  The earlier combined `debug-release` identity is not used by the split workflow.
- Actual bytes of all tracked Cargo manifests/lockfiles, toolchain files,
  `build.rs` files, root Cargo configuration, the setup action and every tracked
  vendor file. A vendor C/Rust source edit invalidates without needing a manifest
  edit. Ordinary application Rust source edits reuse compatible dependency caches.
- Linux's installed package/version/architecture inventory, Clang, CMake and
  pkg-config versions, or macOS's Xcode version, SDK version/build, selected Clang
  and OS build. This includes native ABI/SDK changes and conservative hosted-image
  updates. `SDKROOT`, deployment target, pkg-config, library/linker and archiver
  environment inputs also participate through upstream's flag hashing.

The raw manifest/lock digest is part of the compatibility prefix; unlike upstream's
broader fallback, this deliberately does not restore an older dependency graph.
Compiler/flags/platform/source mismatches cannot fall back to an incompatible
prefix. The full upstream key remains in the Actions cache log; local diagnostics
retain its setup prefix. Changing the versioned `gitturtle-rust-v1` prefix is a
reviewable way to isolate a cold experiment without deleting another job's cache.

### Trust, failure and storage bounds

Cargo cache storage is isolated under the fresh runner's temporary directory.
Only its `registry` and `git` subtrees and the workspace `target` are eligible;
Cargo binaries, configuration and credential files are excluded. Checkout keeps
`persist-credentials: false`, and existing Git configuration isolation and Linux
native prerequisites remain in place. No cache is a trusted release input.

Setup always restores with saving disabled. Only a successful **push to
`FernandoX7/GitTurtle`'s `main`** can register the finish save; PRs, fork PRs and
manual runs are read-only consumers. Failed/cancelled validation cannot save.
A main job's finish action checks for the existing exact key without downloading
again and registers upstream's normal post-job save only when its payload budget
passes. An existing exact entry is immutable and needs no duplicate save.

The restore action's `cache-hit` means exact match only. A miss, partial match,
service/extraction error or missing/failed action result discards the restored
`target`, registry and Git payload before Cargo runs. This also removes a partial
archive. Cargo downloads and builds from the locked inputs normally. Unexpected
lockfile mutation during cache metadata discovery is an error, never a cached
success. The action cannot skip a test command; compilation and test failures
retain their normal failing result, without blind retries.

Before a save, [.github/actions/setup-rust/cache.py](../.github/actions/setup-rust/cache.py)
resolves locked all-feature metadata (matching the upstream save graph), then
accounts logical file bytes plus a conservative 4 KiB per directory/file entry,
counting hardlinked aliases separately. It permits at most 250,000 entries and
**1.5 GiB for each separate profile** (the helper retains a 3 GiB limit for its
older combined-profile mode). These are pre-cleanup payload ceilings, not
compressed archive measurements. Four current platform/profile families therefore
retain at most 6 GiB per compatible generation before compression, the same
aggregate ceiling as the two earlier combined-profile families.

After removing local package outputs through `cargo clean --locked --package`,
the helper may clean up to eight largest dependency package groups, stopping new
cleanup work after 90 seconds (each Cargo command has a 30-second timeout). Cargo
owns removal of fingerprints, libraries and generated native outputs together.
If the payload still exceeds its ceiling, the entire target is discarded and a
bounded downloads-only cache may be saved. If downloads alone exceed the limit,
or accounting/cleanup fails, no save is registered. It never deletes arbitrary
build-script `OUT_DIR` members while keeping their fingerprint. This bounds
retained data without changing the validation just completed; budget fallback may
reduce warm reuse and must be measured.

Upstream performs additional dependency/age cleanup in its post action. GitHub
also applies repository-wide
[cache limits and eviction](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#usage-limits-and-eviction-policy).
This patch does not change repository storage settings or delete caches through
the API. Old compatibility generations and retained historical CodeQL caches
share that quota. C1 must record
actual compressed cache sizes, eviction/miss rates and transfer cost; the local
baseline's full debug/release disk footprint is not the action's archive size.
Ordinary source-only PRs neither create new cache generations nor upload entries.
Do not add per-commit/per-run keys or job variants that continually evict useful
platform entries. If observed storage churn defeats reuse, adjust the retained
families/budgets in a reviewed change using those measurements.

### Cache observations and pending C1 evidence

`rust-cache-restore.json` uses the existing metrics schema and supplies exact-hit
state and a monotonic restore interval. Its documented boundary includes action
initialization, runner dispatch and recovery/cleanup; it is **not** isolated network
transfer time. `rust-cache-budget.json` records cleanup wall time, the payload
ceiling, retained pre-cleanup accounting, removed-package count, whole-target
fallback and save eligibility. Both live in the existing three-day diagnostics
artifact. The summary displays explicit cache values, preserving unknown fields
as unavailable and `false`/zero as observations.

Upstream exposes no compressed-byte or save-duration action outputs. Those remain
`null` before the job ends. The completed Actions job's restore/save step intervals,
upstream cache-size log and cache inventory provide later evidence. When preparing
a report export, attach a measured cache value only to its observed step; identify
whether save time includes cleanup/compression/upload and whether bytes describe
the compressed archive. Do not substitute the helper's pre-cleanup ceiling for a
compressed size. Include the final post action in end-to-end job and merge timing.

Local tests exercise source/vendor/native/profile invalidation, isolated path
refusal, partial restore removal, bounded whole-package cleanup and truthful cache
summaries. A disposable real Cargo fixture regenerates and tests its build-script
output after cleanup. Run that optional fixture explicitly with
`GITTURTLE_CACHE_CARGO_QA=1 python3 -B -m unittest discover -s scripts/ci/tests -p test_rust_cache.py`;
ordinary development-tooling jobs do not install a Rust toolchain just for that
fixture. These checks establish source behavior, not a hosted cache hit or speed
improvement. C1 still requires a reviewed main revision to seed trusted
entries, cold/warm PR/main samples on each platform, fork read-only behavior,
lock/toolchain/vendor/native invalidation, stale/corrupt cache refusal, post-save
cost, actual retained subset/eviction and all required tests executing on warm runs.
Manual branch repeats alone cannot establish a trusted-main warm cache.

## Security review and historical scanning

CodeQL was retired at the maintainer's request. The [retirement record](ci-codeql.md)
preserves the distinction between this scope change and measured CI optimization.
The [security reviewer](development/security-review.md) checks actual trust
boundaries before development acceptance; it does not publish per-finding PR
comments. Preserve historical scan measurements as historical observations.
