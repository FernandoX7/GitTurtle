# CI measurement and diagnostics

[Quality](../.github/workflows/quality.yml) validates development tooling and the
Rust workspace on macOS and Linux. The standard-library
[measurement helper](../scripts/ci/metrics.py) makes its time and failure costs
inspectable. It reports evidence; it does not change run policy, add a build cache,
retry jobs, publish reports or update repository settings.

The source patch prepares local tooling and workflow instrumentation. Hosted
behavior and improvement remain part of
[coordinator checkpoint C1](development/commit-inspector-and-ci.md#c1--hosted-quality-and-merge-protection).
A checked-in workflow or passing fixture is not evidence of an executed hosted run.

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
test-harness messages, up to 1 MiB per step. Optional `jobs[].steps[].cache` supplies
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

Every report keeps repository, run ID, attempt, commit and event together. Job and
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
observation; conflicting snapshots are rejected. Distinct push
and PR runs for the same commit are real separate executions: retain both,
identify the shared head, and measure their resource cost. Keep reruns separate
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
size and build timing artifacts. Quality currently has no Cargo/build cache, so
these cache fields are unavailable. A blank field is not a cache miss, and no
restore/save duration is assumed to be zero. A future cache owner must supply
actual restore/save evidence and invalidation context before comparing cache
benefit.

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
   unsuccessful work. Identify push/PR duplication by head SHA while retaining its
   actual runner cost.
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
Quality and CodeQL can overlap. Measure the full required-check creation-to-final
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
CodeQL may continue to determine the full required-check completion time.

### Later measurements and C1

Pending: candidate-bound hosted PR/main/manual observations, comparable cold/warm
samples on both platforms, cache overhead and invalidation evidence, preserved
failure diagnostics, and measured improvement. No later hosted timing is claimed
by this guide. Add sanitized run links, source identities, raw-sample locations,
counts/statistics and limitations here when the coordinator establishes them.

## Local verification

The controller's tooling profile does not automatically discover these CI helper
tests. Run them explicitly; Quality also runs them in its development-tooling jobs.
From the repository root:

```sh
python3 -B -m unittest discover -s scripts/ci/tests -p 'test_*.py' -v
python3 -B -c 'import ast; from pathlib import Path; ast.parse(Path("scripts/ci/metrics.py").read_text())'
python3 -B scripts/ci/metrics.py --help
python3 -B scripts/ci/metrics.py report --help
python3 -B scripts/ci/metrics.py measure --help
python3 -B scripts/ci/metrics.py summary --help
```

Validate changed Actions YAML with
[actionlint v1.7.12](https://github.com/rhysd/actionlint/releases/tag/v1.7.12),
the version pinned by Quality. Its [Linux amd64 release archive](https://github.com/rhysd/actionlint/releases/expanded_assets/v1.7.12) SHA-256 is
`8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8`;
the workflow verifies that digest before execution. With that version available:

```sh
actionlint -version
actionlint -shellcheck='' -pyflakes='' .github/workflows/quality.yml
```

The explicit flags make this Actions validation independent of optional installed
ShellCheck/Pyflakes versions. JSON/YAML parsing alone does not validate Actions
expressions and contexts. Focused fixtures cover run states, missing evidence,
concurrent execution, skipped-job timestamps, duplicate observations, bounded
collection, HTTP transport failures and credential redaction (including Basic,
Proxy-Authorization, quoted values and compound keys); they do not establish
hosted timing or cache reuse.

The CI fixtures place temporary files inside the checkout. For other tooling
tests that use the system temporary directory, use a checkout-local directory when
the session requires it: `mkdir -p .local/ci-observability/tmp`, then prefix the
test command with `TMPDIR="$PWD/.local/ci-observability/tmp"`.

### Source validation evidence

Validated September 15, 2026 on Linux x86_64 with Python 3.12.3, for the source
patch based on `076bb27ac1d94c84b5e4d5a8accff1195b15ff83`:

- The focused unittest command above passed **54 tests**. This includes skipped
  jobs, missing-start queue uncertainty, quoted and compound credential redaction
  in stdout/logs/build timing artifacts, paths containing spaces, truncated HTTP
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

It ran **132 tests and failed with 18 failures and 53 errors**. The sandbox
reports unexpected ownership on filesystem ancestors, and the inherited
controller Git-record destination is outside the writable checkout. These prevent
the controller's protected-record and Git-fixture setup. The local log is
`.local/ci-observability/tooling-tests.log`. No controller or permission controls
were changed; rerunning that gate in the controller's validation environment
remains pending.

These are local source checks. Rust/native builds were not run for this CI helper
patch. The controller owns the candidate commit and its independent gates.
Hosted C1 remains pending until candidate-bound observations exist.
Native, product-performance, package and vendor attestations are not required by
this tooling-only contract; controller-added requirements, if any, follow candidate
creation.

Implementation references: [GitHub workflow runs](https://docs.github.com/en/rest/actions/workflow-runs),
[jobs for a run attempt](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt),
and [Cargo build timings](https://doc.rust-lang.org/cargo/reference/timings.html).
