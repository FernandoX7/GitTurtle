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
- Claude Code configuration (`.claude/**` and any file named `CLAUDE.md`) routes
  like `AGENTS.md` and `.agents/`. No build or package reads it: the guidance check
  validates its agents, skills, settings structure, hook paths and syntax and links,
  and the agent-loop suite in the tooling lane runs the path-protection hook
  (`test_claude_hooks`) and the controller's Claude adapter. A change that also
  touches `crates/`, `vendor/` (other than its guides), a manifest or `.github/`
  still runs all lanes.
- Rust, manifests, the lockfile/toolchain, native assets, vendored inputs, build or
  package scripts, workflow/repository-policy changes and unrecognized paths run
  **all** lanes. Rust keeps formatting, locked workspace tests and doctests,
  strict all-target Clippy, release compilation, both platform package checks
  and the [Arch Linux container tests](#arch-linux-container-job).

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
Pages permission or signing credential. The one secret Quality reads is the
optional `NATIVE_QA_PRIVACY_TEMPLATES`, passed only to the
[image privacy scan](#image-privacy-scan) step; GitHub withholds it from fork pull
requests. External-fork approval remains controlled
by GitHub's existing repository policy; C1 must observe a real approved fork run.

### Stable gate and compatibility

`Quality gate` runs with `always()` after classification, formatting, the test,
Clippy and release platform matrices, the Arch Linux container job, development tooling,
the image privacy scan and the Website call. It requires successful classification
and success for each required lane and for the image privacy job, whose recorded status it prints. A skipped lane is accepted only when the recorded classification says it is
unneeded. Failures, cancellations, unexpected skips, absent jobs, mismatched
outputs or unknown classification versions fail. Each matrix keeps
`fail-fast: false` so one platform failure does not discard the other platform's
diagnostics. A checkout/evaluator failure also leaves the gate unsuccessful.

The actual platform work is named `Rust tests · <platform> · <shard>/2`,
`Rust Clippy · <platform>` and `Rust release · <platform>`. The test platforms are
`macos-15`, `ubuntu-24.04` and `archlinux`, each in [two shards](#test-shards)
(until November 3, 2026 tests and Clippy were one `Rust tests and Clippy · <platform>`
job, and until November 19, 2026 each platform's tests were one `Rust tests · <platform>`
job). `Rust formatting` checks the workspace once on Ubuntu. The aggregate cannot pass a product change unless
the three platform matrices, the Arch Linux matrix, formatting and other required lanes
succeed; documentation changes may pass after
justified skips. This avoids GitHub's [skipped-job success behavior](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
silently bypassing a failed dependency.

The transitional **`Rust · macos-15`** and **`Rust · ubuntu-24.04`** jobs previously
mirrored the complete gate from short Ubuntu jobs. On September 15, 2026 at
23:11:43 UTC, the coordinator applied and verified gate-only main protection:
`Quality gate`, strict/up-to-date checks enabled, GitHub Actions app ID `15368`.
The readback at main `b5d681c1c6db21bfb74b3e07da461c3f8de588dc` preserved all
unrelated protections and the retired CodeQL state. This dated migration result
does not establish completion of the remaining C1 cache, fork or runtime evidence.

The cleanup that removed only the two transitional mirrors landed on main as
`bfd6e24` on September 16, 2026, after independent general and security reviews.
The first main run on it,
[Quality 35107317146](https://github.com/FernandoX7/GitTurtle/actions/runs/35107317146),
passed ten jobs, including `Quality gate`, with no `Rust · <platform>` job. The
classifier, aggregate and every underlying validation phase are unchanged. The
cleanup does not establish the remaining C1 evidence either.

Repository settings are managed separately through the
[required-status-check endpoint](https://docs.github.com/en/rest/branches/branch-protection#update-status-check-protection).
The migration first retained the two Rust requirements while adding the observed
`Quality gate` app binding, then switched to the verified gate-only requirement.
Fresh capture and preserving readback cover strictness, app binding and unrelated
review/conversation settings at each transition. CodeQL was separately retired by
the maintainer; this cleanup does not reintroduce its inactive rule or modify
repository settings.

Before the source cleanup, rollback can restore the latest applicable captured
required-check set through that scoped endpoint while its jobs still exist.
After cleanup, restore the compatibility jobs **before** requiring their names:

1. Prepare a reviewed revert of the cleanup commit against current main. Restore
   the removed job block without overwriting newer workflow changes; keep the
   existing `Quality gate` requirement in place.
2. Integrate the restoration through the protected PR flow and observe the restored
   jobs passing on the intended main revision and current main-targeting PR, with
   the exact GitHub Actions app binding. Source restoration alone is insufficient.
3. Capture fresh protection, then add the observed Rust checks alongside the gate
   through the scoped endpoint. Verify the full readback before any separately
   reviewed change to the aggregate requirement. Preserve strictness, unrelated
   protections and retired CodeQL state; do not replay a stale settings snapshot.

If a settings request has an uncertain result, read back the live state before
another action. Never remove protection to clear a failed check. The independent
[security review](development/security-review.md) is a development acceptance
requirement. It is not an automated GitHub status check, and a successful Quality
gate alone does not establish that the review happened.

### Image privacy scan

The repository is public, and committed screenshots have leaked an account name,
home paths and a hostname-derived Git identity in their pixels. Two checks guard
images. The guidance check in the mandatory `Changes and CI policy` job parses the
text metadata of every tracked image (PNG `tEXt`, `zTXt`, `iTXt` and `eXIf`
chunks, JPEG APP1 EXIF and XMP and COM segments, SVG source) for home paths and
email addresses outside the reserved example domains. Its declared limits: ICC
profiles, JPEG APP13 (Photoshop and IPTC) and C2PA manifests, bytes after a PNG's
`IEND` or a JPEG's end-of-image marker, and pixel data are not read; other formats
and PNG or JPEG files that do not parse are searched whole; decompressed PNG text
is capped at 1 MiB per image, and an image over the cap fails as not fully
checked. Pixels need template matching against the private strings themselves,
which no tracked file may hold.

The `Image privacy` job runs on every Quality event. [`image_privacy.py`](../scripts/ci/image_privacy.py)
lists the PNG, JPEG, GIF and WebP files the event adds or changes, using the
classifier's verified comparisons (the merge-base to the PR head and the base to
the merge commit; a main push's before/after) with deletions excluded. When the
optional `NATIVE_QA_PRIVACY_TEMPLATES` repository secret is present, the job
installs Pillow from the Ubuntu archive and runs the
[native-QA scan](../scripts/native_qa/README.md#automated-scans) with
`--redacted`, which prints each image's path and `clean` or `MATCH` only. Every
frame of an animated GIF, WebP or PNG is scanned; an image with more than 64
frames fails as unscannable rather than being sampled. A match
fails the job and therefore `Quality gate`. The job records one of four statuses,
and the gate prints it:

- `scanned`: every added or changed image was scanned and none matched (by all
  eight [shards](#shards) together).
- `no-images`: the event adds or changes no raster image.
- `unavailable`: the secret is absent, which is always the case for a fork pull
  request. The job emits a `::notice::` and a step-summary line saying the images
  were **not** scanned; the gate line says so too. It is not a pass.
- `no-comparison`: manual dispatch, or a main push without a previous commit, has
  nothing to compare, and the notice says the images were not scanned.

#### Shards

The matching is brute force: its cost is the images' pixels times the templates'
pixels. As one 45-minute job it was cancelled before it finished the 735 images
(477 megapixels) of a pull request, printing nothing, so `Quality gate` failed
([Quality 43781503794](https://github.com/FernandoX7/GitTurtle/actions/runs/43781503794)).
The job is therefore a matrix of eight shards, `Image privacy · 1/8` to
`Image privacy · 8/8`, with `fail-fast: false` and 90 minutes each. Every shard
checks out the same commit and makes the same plan; `image_privacy.py slice`
then gives it every eighth image of the sorted list, counted from its own
number. The slices are a pure function of the list, the number and the count:
each image is in exactly one of them, and a directory of large frames spreads
over all eight. `slice` is told the shard's number and the matrix's real job
count (`strategy.job-total`) and refuses a number outside 1 to 8, a count other
than 8, an empty list, and a list that is not sorted and unique, so a matrix
edited to fewer jobs fails instead of leaving slices that no job scans. The
shards change which runner scans an image and nothing else: the comparisons,
suffixes, name refusal, animation-frame rule, templates, threshold, engine and
`--redacted` are the same, and no image is scanned twice.

A shard whose slice is empty, which happens when fewer than eight images are
listed, records `scanned` in the slice step and runs neither the Pillow
installation nor the scan step, so the secret is not mapped into that job. A
shard with images prints its slice's size and then each image's verdict line,
`path: clean` or `path: MATCH`, when that image's scan ends. The lines keep the
list's order, so one waits for the images listed before it while the runner
scans an image per core. The log of a shard cancelled at its limit therefore
shows the images it finished; those after its last line were not scanned.

The 90 minutes are a budget, not a measurement of the secret's set, whose size
is unknown outside the secret. At the cost the
[tooling guide](../scripts/native_qa/README.md#privacy-templates) records for
the local set (17.6 s of one core per 1000x680 frame, about 26 core-seconds per
megapixel), 477 megapixels need about 3.4 core-hours on that faster core,
before the extra readings of magnified crops. Eight four-core shards of 90
minutes allow 48. No hosted run of the shards is recorded here yet.

`Quality gate` reads the matrix as the one job `needs.image-privacy`, exactly
as before, and still requires the result `success` together with one of the
four statuses ([`gate`](../scripts/ci/changes.py)):

- **Outputs.** GitHub combines a matrix's job outputs from all of its jobs, the
  last job to set a name overrides the others, an empty value sets nothing, and
  the jobs run in no guaranteed order
  ([workflow syntax, `jobs.<job_id>.outputs`](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idoutputs)).
  So every shard writes a status that is true of the whole list once all eight
  succeed. The plan's status is the same in every shard, because each reads the
  same event, commit and secret. A shard replaces `scan` with `scanned` only
  when its whole slice was scanned without a match, or was empty; a shard that
  fails, times out or is cancelled leaves `scan`, which the gate does not
  accept.
- **Result.** GitHub's [`needs` context](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#needs-context)
  lists `success`, `failure`, `cancelled` and `skipped` for
  `needs.<job_id>.result` and does not say how a matrix's jobs combine into it.
  The gate relies here on what it already relies on for the test, Clippy,
  release and tooling matrices: the result is `success` only when every job of
  the matrix succeeded. The [validation matrix](validation.md) records both
  cases on this repository: one failed macOS test job beside a green Ubuntu one
  failed the gate in Quality 41900575513, and one test job cancelled at its
  time limit did in Quality 41757534359.

With all eight shards successful, every slice was scanned clean and every shard
says `scanned`, whichever sets the output last. With one failed, cancelled,
timed out or skipped, the result is not `success` and the gate fails whatever
the output says. One limit remains: each shard reads at its own start whether
the secret exists. Do not create or remove the secret while a Quality run is in
progress, and re-run the whole run, not only its failed jobs, after changing
it: shards that disagree about the secret would report `unavailable` and
`scanned` side by side, and the gate would print whichever was set last.

#### The secret

The secret is safe to hold because GitHub passes secrets only to runs from this
repository's own branches, whose authors already have write access; a fork pull
request, `pull_request_target` or a Dependabot run never sees it. It carries
grayscale template pixels without file names (`qa.py privacy pack`), fits under
GitHub's 48 KB limit, and reaches only the scan step's environment. That step
writes it to a mode-0600 file under `$RUNNER_TEMP`, strips it from the scanner's
environment, keeps the scanner's temporary files under `$RUNNER_TEMP`, and
removes the file afterwards. GitHub masks the value in logs; the scan prints no
template name, score, position or crop, so nothing else derived from it can
reach a log, and nothing is uploaded or published. Image paths travel
NUL-delimited in a file and unprintable names fail the job. The step stops
workflow commands before the scanner starts, passes each of the scanner's lines
to the log as it is read and resumes commands when the scanner has ended,
with a token the scanner is never given, so a crafted file name cannot become a
workflow command. A step that is interrupted or fails while it passes lines on
kills the scanner first, and still removes the file and resumes commands. The owner creates or rotates the secret with the steps in the
[native-QA tooling guide](../scripts/native_qa/README.md#automated-scans); until
then every run reports `unavailable` or `no-images`. Locally, `python3
scripts/gate.py full` runs the same scan when a templates directory is configured.

## Parallel validation and coverage

After classification, the selected jobs have no build dependencies on each other:

- `Rust formatting` checks `cargo fmt --all -- --check` on Ubuntu with the pinned
  workspace toolchain. It installs no native packages and restores no target cache.
- `Rust tests · macos-15 · 1/2` and `· 2/2`, and the same two for
  `ubuntu-24.04`, each restore the **debug** cache, install the
  [pinned cargo-nextest](#test-execution-with-nextest) and run
  `cargo nextest run --locked --workspace -P ci --no-fail-fast --timings
  --partition slice:<shard>/2`: every shard builds the whole workspace's tests
  and runs its half of them (see [Test shards](#test-shards)). Shard 1 alone
  then runs `cargo test --locked --workspace --timings -- "(line "` and, on a
  push to `main`, saves the debug cache once its own steps have succeeded.
  nextest builds the same targets as Cargo's default
  [test selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html) and
  runs its unit and integration tests; the doctest step runs the rest of that
  selection. There is no package, test-name, target or feature filter that removes
  the existing platform-conditional tests.
- `Rust Clippy · macos-15` and `· ubuntu-24.04` run beside them on the same
  platforms and trigger. Each restores the same platform's **debug** cache (same
  profile, runner and environment, so the same key) and runs
  `cargo clippy --locked --workspace --all-targets --timings -- -D warnings`. It
  never saves: it has no `finish` phase, so shard 1 of the test job remains the only writer of
  that entry (see [Why separate test and Clippy jobs](#why-separate-test-and-clippy-jobs)).
- `Rust release · macos-26`, `· ubuntu-24.04` and `· ubuntu-24.04-arm` each restore
  the **release** cache
  and build `gitturtle` with `--release --locked --timings` and an explicit platform
  target. Each job packages that executable without rebuilding it. Linux checks
  archive extraction, isolated installation, installed bytes, notices, desktop
  entry, dynamic libraries and the expected no-display launch failure; macOS
  checks bundle identity and ad-hoc signatures. Complete license notices on a
  platform in the upload set (Linux; the macOS upload is held) enable
  a same-run artifact upload/download and independent payload verification. The
  download asks for the uploaded artifact's exact ID up to three times (at once,
  then about 15 s and about 45 s more after a failure), because the run's
  artifact listing can lag the upload; the first two attempts may fail without
  failing the job, the third fails it, and the verification still requires the
  downloaded files. Known
  C0 notice gaps explicitly withhold public binaries while development package
  checks continue; other collection or verification errors fail the job. See the
  [artifact runbook](ci-artifacts.md) for exact gates and evidence requirements.
  These checks do not establish an interactive native desktop or notarized build.
- `Rust tests · archlinux · 1/2` and `· 2/2` run the same nextest and doctest commands as the debug
  jobs, in the same two shards, inside a digest-pinned Arch Linux container, against that day's Arch
  packages, with a Rust cache keyed on the Arch packages that reach compiled
  output. See
  [Arch Linux container job](#arch-linux-container-job).
- Only macOS optimized compilation and packaging use the standard ARM64 macOS 26
  runner. Tests, doctests, strict Clippy and development tooling remain on macOS 15.
  The shared `scripts/release/workflow.py select-xcode` command requires installed
  Xcode 26.3 build 17C529 before cache identity; missing or changed tools fail without
  falling back. This bounded workaround follows the retained
  [Apple-tool observations](benchmarks/2026-09-15-ci.md#icon-compiler-environment).
  It removes macOS 15 optimized-package execution from CI; continued macOS 15 tests
  do not establish compatibility of a package built on macOS 26. The new OS build
  changes the existing native cache identity, so earlier cache timings cannot be
  reused as warm evidence. Full hosted package validation remains required.
- Development-tooling checks still cover both OSes; Website remains reusable.
  The mandatory policy job runs all CI helper fixtures and pinned Actions-aware
  lint on every workflow even for documentation-only changes.

The existing change classifier owns selection, the setup action owns each cache's
lifetime, and the gate owns the final result. No new cross-job build artifact or
mutable shared target directory is introduced. Debug/release use separate fresh
runners and matching setup/finish profiles; a finish cannot remove another lane's
inputs. The Clippy jobs and the second test shard only read the debug entry, so they cannot race the
first shard's save or leave it an entry without the test build.
Package/artifact consumers must precede finish because its successful-main
cleanup can remove the application executable. The final gate requires the
formatter, the test, Clippy and release platform matrices and the Arch Linux matrix, in addition to
tooling/Website according to the recorded plan. A successful test matrix cannot
cover a failed, cancelled, missing or unexpectedly skipped Clippy or release matrix, and a test matrix
is successful only when each of its [shards](#test-shards) is. Removing the transitional mirrors
does not change that dependency or result contract.

### Why separate test and Clippy jobs

On November 3, 2026 Clippy moved out of the test job into `Rust Clippy · <platform>`.
Quality run [41757534359](https://github.com/FernandoX7/GitTurtle/actions/runs/41757534359)
on `36d977aa` cancelled `Rust tests and Clippy · macos-15` at its 45-minute limit
with 1,747 of 1,910 tests passed and none failed. Against the last green run,
[41745637784](https://github.com/FernandoX7/GitTurtle/actions/runs/41745637784)
(same `macos-15-arm64` image 20261028.0489, same cache key), its test build took
17m43s instead of 12m22s and its tests ran 1.8 times slower at the median (4.25
times at p90): a slow runner, not the code. In that green job the nextest step
took 1,470 s (about 12.4 minutes of build and 715 s of tests) and Clippy 267 s,
about 29 minutes in all; the twelve runs before it took 22 to 36 minutes. With
Clippy beside it, the test job's path is the build and the tests only, and a
failing test no longer hides Clippy's result (runs 41505789304 and 41525180845
never reached macOS Clippy after a test failed).

Ubuntu moved too, though its job (about 20 minutes) was not near its limit: one
matrix keeps a single Clippy definition and one job name per platform instead of
an OS condition in each job, and standard hosted runners cost nothing for this
public repository. The cost is runner time off the critical path: each Clippy job
sets up its own runner and, since the test job no longer runs Clippy, the debug
entry no longer holds Clippy's checked (metadata-only) dependency outputs, so each
run checks the dependencies again; build-script and procedural-macro outputs that
match the test build's are restored rather than rebuilt. A separate cache family
for the checked outputs would need a new setup-action profile; the next hosted runs
show whether that time warrants one.

### Why two compilation lanes per platform

The September 15 baseline spent roughly 13m18s (macOS) and 10m49s (Linux) compiling
workspace tests, followed by only 120.672s and 30.049s through the final doctest
result. The serial Clippy steps took 5m47s and 4m08s; optimized builds added 9m48s
and 7m03s. These are observations from the
[baseline PR run](https://github.com/FernandoX7/GitTurtle/actions/runs/34992350894),
not predictions for the new graph.

Release compilation has a distinct profile and can overlap debug work. Clippy
stayed in the test job until November 2026 to avoid another cold debug dependency
build and another cache reader/writer family; it now has its own job, which only
reads the debug entry (see [above](#why-separate-test-and-clippy-jobs)), so there
is still one debug writer per platform. A failing test fails its shard of the test job, and one in
shard 1 stops that shard before the doctests; the other shard and the independent Clippy and release
jobs retain their diagnostics. There
is no automatic retry. The September 15 fan-out judged the short test phase too
small to justify nextest, partitions or additional test runners. Later warm runs
made test execution itself visible (see [Test execution with nextest](#test-execution-with-nextest)), and
the local gate already ran nextest, so the tests step uses it. By November 2026 the tests
themselves were the longest part of a run, and each platform's now run in two partitions on
two runners (see [Test shards](#test-shards)).

### Test execution with nextest

`cargo test` ran the workspace's 25 test binaries one after another, each with
its own thread pool: in warm main run
[36164627195](https://github.com/FernandoX7/GitTurtle/actions/runs/36164627195)
libtest reported 146 s of summed test execution on macOS against 59 s on Linux. The local gate already ran
[cargo-nextest](https://nexte.st/) with `-P ci`, so CI and local results could
differ. The tests step now runs `cargo nextest run --locked --workspace -P ci
--no-fail-fast --timings`, and since the [shards](#test-shards) also
`--partition slice:<shard>/2`. nextest builds the same unit, integration and example
targets as `cargo test` (passing `--locked` and `--timings` through to Cargo), then
runs each test in its own process across the runner's CPUs. It cannot run
doctests, so a separate `Locked workspace doctests` step runs `cargo test --locked
--workspace --timings -- "(line "` and records its own `doctests` measurement.
The step keeps Cargo's default target selection so that it reuses every library
the nextest step built. `cargo test --doc` selects library targets only; on Linux
the app's bin-only dependencies then leave the unit graph, Cargo no longer shares
`quote`, `syn` and the proc-macro crates between host and target, and it rebuilt
about 30 of them with build-override debug information: the first hosted sample,
PR run [36300227546](https://github.com/FernandoX7/GitTurtle/actions/runs/36300227546),
spent 107 s compiling for 0 doctests. The filter matches every doctest, whose
libtest name is `<file> - <item> (line <n>)`, and no unit or integration test,
whose name is a Rust path; each test binary starts once and reports every test
filtered out. Every Cargo command after the first would still recompile the app:
[`crates/app/build.rs`](../crates/app/build.rs) watches `.git/packed-refs`, a
shallow checkout has none, and Cargo reruns a build script whose watched file is
missing. The second sample, PR run
[36321021870](https://github.com/FernandoX7/GitTurtle/actions/runs/36321021870),
spent 32 s (Linux) and 72 s (macOS) doing that in the doctest step, so the job
first runs `git pack-refs --all --no-prune`, which writes the file and keeps the
loose refs the build script also watches.

The [`ci` profile](../.config/nextest.toml) sets `fail-fast = false` (the step also
passes `--no-fail-fast`), `retries = 0`, `failure-output = "immediate"` and
`status-level = "skip"`. A failing test's output appears in the log where it fails
and the step still exits non-zero after every test has run; unlike `cargo test`,
a failure in one binary no longer hides the others' results. No test is retried,
so a flaky test fails the job as before. The status level prints one `PASS`,
`FAIL` or `SKIP` line per test, so the log lists every executed and ignored test
as libtest's `test … ok|ignored` lines did. The inherited default profile reports
a test running longer than 60 s as slow and terminates it after 120 s; `cargo test`
had no per-test limit, so a test that hangs now fails with a `TIMEOUT` line
instead of holding the job until its 60-minute timeout. The profile's JUnit report
(`target/nextest/ci/junit.xml`) is not uploaded: it holds unsanitized captured
output of failing tests, and the sanitized `tests.log` already lists every result.
The measurement helper reads nextest's `Summary [ … ]` run duration as the test
harness time; being one parallel run, it is a wall-clock span rather than a sum
of per-binary times.

The pinned version is installed by
[`scripts/ci/tools.py install-nextest`](../scripts/ci/tools.py) before the tests
step, measured as `nextest-install`. It downloads the official release archive
over HTTPS, verifies its SHA-256 against the digest committed in that script before
reading it, extracts only the regular `cargo-nextest` file into
`$RUNNER_TEMP/cargo-nextest`, checks that `cargo nextest --version` resolves to the
pinned release, and only then adds the directory to `GITHUB_PATH`. A transport
failure is retried twice; a checksum, archive or version mismatch fails the step.
The step needs no token or permission beyond the workflow's `contents: read`, and
no third-party install action is used. It lives outside the Rust setup action, whose
files are hashed into the [cache key](#tool-choice-and-compatible-reuse), so
changing the pin does not invalidate the Rust caches. The archive is fetched on
each run rather than cached: about 12 MB (Linux) or 17 MB (macOS).

| Runner | Release archive (cargo-nextest 0.9.146) | SHA-256 |
| --- | --- | --- |
| ubuntu-24.04 (Linux X64) | [`cargo-nextest-0.9.146-x86_64-unknown-linux-gnu.tar.gz`](https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.146/cargo-nextest-0.9.146-x86_64-unknown-linux-gnu.tar.gz) | `682c21b777c333e96fd532e114d3a5a894e0729ab88d94c0a9f20f8419695428` |
| macos-15 (macOS ARM64) | [`cargo-nextest-0.9.146-universal-apple-darwin.tar.gz`](https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.146/cargo-nextest-0.9.146-universal-apple-darwin.tar.gz) | `39785160b3c2f6ed9a765049cf4fa79f3b39aa02eb7598a5a0e2a1a0b9ffb9a8` |

Version 0.9.146 (September 21, 2026) was the latest release when this was pinned.
It differs from 0.9.145, whose status-line format `scripts/gate.py` parses, only in
replacing yanked dependency versions. Both digests matched the release's published
`.sha256` files, GitHub's recorded asset digests, and a local `sha256sum` of the
downloaded archives on September 27, 2026. To move the pin, update the version and
both digests in `tools.py` and this table from the new release's `.sha256` assets,
check that its output format still matches the gate's parser, and run the CI helper
tests.

Reproduce the CI test phase locally with the same isolation as the job:

```sh
GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
  cargo nextest run --locked --workspace -P ci --no-fail-fast
GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null cargo test --locked --workspace -- "(line "
```

These run every test in one go. One [shard](#test-shards)'s share is the first
command with `--partition slice:1/2` or `--partition slice:2/2` added.

To check that two runs executed the same tests, download each Rust test job's
log (`gh api repos/FernandoX7/GitTurtle/actions/jobs/<job-id>/logs`) and compare them,
one platform and one attempt at a time. Each side takes one job's log or several
taken together, so a platform's two [shards](#test-shards) compare with an unsharded
run or with another run's shards:

```sh
python3 scripts/ci/executed_tests.py compare --before cargo-test-job.log --after nextest-job.log
python3 scripts/ci/executed_tests.py compare --before unsharded-job.log --after shard-1-job.log shard-2-job.log
```

[`executed_tests.py`](../scripts/ci/executed_tests.py) reads libtest `Running` and
`Doc-tests` headers with their `test … ok|ignored|FAILED` lines, and the result
lines of a nextest run. It keys each test by target kind and Cargo crate name
(`lib:gitturtle_core history::…`, `bin:gitturtle …`, `test:worktrees_reflog …`,
`doc:gitturtle_core …`).

It compares counts, not only names, because a name in a hosted log need not be
one test. The wrapper's
[credential redaction](#workflow-summaries-and-retained-diagnostics) cuts a line
at a key such as `secret` followed by a colon, so every test of
`github::transport::secret_service` prints as
`github::transport::secret_service=[redacted]`: on Linux 28 results under one
name, 23 run and 5 ignored, in a log whose 3,613 tests have 3,591 names. Each key
therefore has a number of executed and a number of ignored results, and two sides
are equal only when every key's numbers are. Several logs of one side are the
shards of one run on one platform, one attempt each, so pass each job's log as its
own file, and only once. Their executed counts add. A key's ignored count is what
the shards list for it less what they executed, because a shard's log prints every
test and marks the other shard's with the same `SKIP` line as an ignored one. A key
executed less often after than before is listed as lost, and one executed more
often as run more than once, each with both numbers and every log's share. A key
the shards executed more often than they list it is an error on its own side, since
a partition runs each test once, and shards that do not list the same tests are
refused as not one run's.

Before its verdict the tool reconciles every log with nextest's own totals. A log
holds one run, from its `Starting … tests across … binaries (… tests skipped)` line
to its `Summary [ … ] N tests run: … passed, … failed, … skipped` line, and the
result lines counted between them must equal the Summary line's tests run, passed,
failed and skipped. The two sides' tests run, summed over each side's logs, must be
equal too. A finished test's line carries its place in the run, such as `(  9/192)`;
a `SLOW` progress line does not and is not a result, and the failures nextest
repeats after the Summary line are not counted again. A result line that is
missing, repeated or printed by a test, a second Starting or Summary line, a run
without a Summary line and a log with no test run each leave the log unreconciled:
the report names the log and gives both numbers, so a hidden line or a forged line
alone cannot make two different runs compare equal. Both together still can: a real
result the tool does not count, with a forged one printed in its place, keeps every
figure. Printing cannot remove a real result line, so that takes a test name the
pattern cannot read or a write into nextest's own stream; the comparison is evidence
about a run that is not trying to deceive it. A count of more than nine digits is
refused as unreadable. These are cargo-nextest 0.9.146's shapes,
taken from three runs of `gitturtle-preview` (unsharded, `slice:1/2` and
`slice:2/2`) and the retained hosted logs. A Summary line in another shape is
refused rather than guessed, and so is a log whose `PASS` and `FAIL` lines are not
its passed and failed figures.

`compare` prints the counts, what does not reconcile and every difference, then
`identical`, `different` or `unreconciled`, and exits 0, 1 or 3; an unreconciled
log exits 3 whatever the comparison says. `list` prints one line for each executed
or ignored result. Test names and unread lines come from logs that a pull request's
test output can shape, so the text output escapes every character outside printable
text (control characters, the ESC or CSI that starts a terminal sequence, invisible
formatting characters, and the backslash so an escape stays distinct), as `--json`
does. Two limits remain. Under a redacted name the tool knows how many
results ran, not which: a split that ran one of those tests twice and another not
at all has the same numbers. And libtest lines (the doctests) have no totals to
reconcile with; test output that interrupts a libtest result line can hide that
test, so investigate any listed difference in the log before calling the sets
different. The dated
[nextest record](benchmarks/2026-09-27-ci-nextest-tests.md) holds the hosted
before/after durations and the test-list comparison.

The Clippy matrix has two fixed OS entries and `max-parallel: 2`. The test matrix has
those two platforms in [two shards](#test-shards) each and `max-parallel: 4`, the Arch
Linux matrix its two shards and `max-parallel: 2`, and the release
matrix three legs (macOS 26, Ubuntu 24.04 and Ubuntu 24.04 on ARM) and `max-parallel: 3`.
All use `fail-fast: false`.
The test jobs, the [Arch Linux job](#arch-linux-container-job) among them, are
bounded at 60 minutes a shard, and the Clippy and release jobs at 45. A test job's limit
only backs up a hung step, since the profile above terminates any test after
120 s. Unsharded, the test jobs took about 25 to 31 minutes on a normal runner, and
[quality run 41757534359](https://github.com/FernandoX7/GitTurtle/actions/runs/41757534359)
met a macOS runner 1.8 times slower at the median: at that pace the Arch job's
28 minutes become about 50, inside 60 and past the earlier 45 (see
[Why separate test and Clippy jobs](#why-separate-test-and-clippy-jobs)). The
shards keep that limit: each still builds the whole workspace, and the limit was
never the tests' own budget. A
workflow policy test in `scripts/ci/tests/test_nextest.py` holds the Ubuntu and
macOS test matrix and the Arch Linux matrix at 60 minutes. With two shards on
each of the three test platforms, at most eleven compilation runners (four on macOS) are
requested per Quality run, plus the independent inexpensive checks, of which
development tooling is the fifth macOS job. Formatting is bounded
at five minutes, policy and the final gate at five, and tooling at ten. The
[image privacy scan](#shards) adds eight Ubuntu jobs of at most 90 minutes to every
run. When the plan is `no-images`, `unavailable` or `no-comparison`, each ends after
its checkout and plan; a shard with an empty slice of a scanned list ends after its
checkout, plan and slice.
GitHub may queue those jobs under the repository's existing concurrency
limits; matrix bounds do not promise simultaneous starts. See the documented
[matrix controls](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations).
Failed commands still retain their exit code, sanitized logs and available Cargo
build timing reports in the three-day diagnostics artifacts. A cancelled runner
may stop before artifact upload; do not claim a missing report was retained.

### Test shards

Since November 19, 2026 each platform's tests run in two jobs. `rust-debug`'s
matrix is its two platforms times two shards (`Rust tests · macos-15 · 1/2`,
`Rust tests · macos-15 · 2/2`, `Rust tests · ubuntu-24.04 · 1/2` and
`Rust tests · ubuntu-24.04 · 2/2`), and `rust-arch` is a matrix of its two
(`Rust tests · archlinux · 1/2` and `Rust tests · archlinux · 2/2`). The job ids,
and so the gate's `needs`, are unchanged.

**Why.** By November 2026 the tests themselves were the longest part of a run.
An audit of the last 150 Quality runs on November 17, 2026 found a full
pull-request run taking 42 minutes at the median and 54 at p95, with
`Rust tests · macos-15` its critical path in 14 of 14 runs: 2,364 s of running
at the median, against 1,456 s for `· ubuntu-24.04` and 1,281 s for
`· archlinux`. In 30 sampled logs nextest's own wall time was 1,976 s on macOS,
1,291 s on Ubuntu and 1,062 s on Arch, after a first Cargo build of 133 to 252 s
on a cache hit. The summed per-test seconds divided by that wall time came to
3.0 on macOS and 4.0 on Linux, the runners' core counts: the tests already use
every core, so only more runners shorten them.

**Why two.** Simulated from those logs' per-test medians, a round-robin split in
two leaves the slower shard 959 s of tests on macOS, 653 s on Ubuntu and 568 s
on Arch. A split balanced by measured duration would give 946, 628 and 546 s, so
nextest's own partitioning is within 5 % of the best split and needs no duration
data. On standard runners GitHub runs at most five macOS jobs at once across an
account's repositories on every plan below Enterprise
([Actions limits](https://docs.github.com/en/actions/reference/limits)),
and a full run already started four: development tooling, Clippy and the tests
on `macos-15`, and the release build on `macos-26`. Two test shards make it
exactly five; a third would wait in the queue. These figures are measurements of
the unsharded jobs and a simulation from them. The hosted figures of the shards
are recorded after their first runs.

**The partition.** Every shard runs the same step, `cargo nextest run --locked
--workspace -P ci --no-fail-fast --timings --partition "slice:$SHARD/$SHARDS"`,
with its number and the total from the job's matrix (`shard: [1, 2]` and
`shards: [2]`, the one place the total is written). Nothing else about selection
changed: no filter, no package or target selection, the same profile, no
retries. nextest's [partitioning page](https://nexte.st/docs/ci-features/partitioning/)
describes `slice:` (added in 0.9.127; the pin is 0.9.146): "nextest lists all
tests across all specified binaries, then assigns tests to each bucket in a
round-robin fashion", and "Sliced partitioning applies after all other test
filters." Both shards of a platform build the same commit with the same command,
so they list the same tests and each test falls in exactly one of them.

**What a shard builds and what only shard 1 runs.** The partition chooses among
the built tests at run time. The same page says "By default, each job has to do
its own build before starting a test run", and no archive passes between these
jobs: each shard builds every test binary of the workspace. Every shard runs the
checkout, the packed refs, the cache restore, the nextest installation, its
slice of the tests, the summary and the diagnostics upload. Every Linux shard,
on Ubuntu and on Arch, also prints the libdav1d link and initializer records:
each shard's runner or container installs its own library, the two Arch
containers upgrade separately, and the helper tests run in whichever shard the
slice puts them, so each shard's log carries the records of the library its own
tests met. What must happen once per platform happens in shard 1 alone
(`if: matrix.shard == 1`): the doctest step and the setup action's `finish`
phase. Shard 2 skips the `finish` phase, the only
place the action registers a save, and the restore in its setup phase passes
`save-if: false`, so like the Clippy job it can never save. Each
platform's debug entry keeps exactly one writer, whose target directory holds
what the unsharded job's held: the whole workspace's test build and the doctest
build. One thing differs: on `main`, shard 1 saves when its own steps succeeded,
even if a test in shard 2 failed. The entry holds dependency outputs keyed by
their inputs, not test results, and the run as a whole still fails.

**The cache key** does not see the shard. [`cache.py`](../.github/actions/setup-rust/cache.py)
keys an entry on the profile, the container name, the tracked build inputs
(`vendor/`, the setup action, manifests, the lockfile, build scripts, the
toolchain file and Cargo configuration), the platform's native inputs (on Ubuntu
the packages a build reads, not the image's inventory: [Ubuntu native
key](#ubuntu-native-key)), `rustc -vV` and the compiler-flag environment. The job's name, its matrix and the
workflow file are not among them, and rust-cache takes that key as its
`shared-key` in place of its job-based one. The shard number reaches only the
tests step's own environment (`SHARD` and `SHARDS`). Both shards of a platform
therefore compute the key of that platform's Clippy job and restore the entry
`main` saved. On Arch the two shards upgrade their containers separately: if
Arch publishes a keyed package between their starts, they build against
different versions and one misses the cache, and each still runs its own share
of the same test list.

**Results.** `Quality gate` still needs `rust-debug` and `rust-arch` and reads
one result for each ([`gate`](../scripts/ci/changes.py)). As for the
[image privacy shards](#shards), a matrix's result is `success` only when every
one of its jobs succeeded, so a failed, cancelled or timed-out shard fails the
gate exactly as a failed test job did. Neither job has `continue-on-error`,
`include` or `exclude`, and `fail-fast: false` lets the other shards finish and
keep their diagnostics. The policy tests in
[`test_nextest.py`](../scripts/ci/tests/test_nextest.py) hold those, the six leg
names, the partition's form and total, what shard 1 alone runs, the libdav1d
records in every Linux shard, the artifact name and the 60-minute limit. The
[release preflight](releases.md#source-package-and-privilege-boundaries) requires
the four macOS and Ubuntu shards by name.

**Diagnostics.** Two shards of one platform share the job id, the OS and the
architecture, and the upload action refuses a name already taken in the run, so
a test job's artifact is named
`ci-diagnostics-<job>-<OS>-<architecture>-shard-<n>-<run>-<attempt>`. The `tests`
measurement and log are per shard; the `doctests` measurement and the cache
budget record are in shard 1's artifact only. A shard's nextest log also lists
the other shard's tests: "Tests not in the current bucket are marked skipped."
A `SKIP` line in one shard's log therefore does not by itself mean an ignored
test.

### Arch Linux container job

`Rust tests · archlinux · 1/2` and `· 2/2` build the workspace and run its tests on Arch Linux, a
rolling release that picks up new compilers, glibc and native libraries before
Ubuntu does. The job (`rust-arch`) runs on `ubuntu-24.04` inside the official
`archlinux:base-devel` image whenever the classification requests the product lane,
and the final gate requires it like the other Rust jobs. It runs the Ubuntu and
macOS test jobs' commands unchanged, in the same [two shards](#test-shards): the
[pinned cargo-nextest](#test-execution-with-nextest) with
`cargo nextest run --locked --workspace -P ci --no-fail-fast --timings --partition slice:<shard>/2`, then
in shard 1 `cargo test --locked --workspace --timings -- "(line "`, after the same
`git pack-refs --all --no-prune`. Each shard is its own container and upgrades and
installs its own packages. Clippy, release compilation and packaging stay on
the Ubuntu and macOS jobs, and no display or desktop session is involved.

Steps, in order:

1. As root, first check that the image's pacman requires trusted package signatures
   (`pacman-conf SigLevel` includes `PackageRequired` and none of `PackageNever`,
   `PackageOptional` or `PackageTrustAll`, globally and in any repository that sets its
   own level), because package install scripts run as root. Then `pacman -Sy
   archlinux-keyring` and `pacman -Su` with the build and
   test packages: `base-devel clang cmake git openssh python rustup ca-certificates`
   plus the GPUI native libraries (`fontconfig freetype2 wayland libxkbcommon
   libxkbcommon-x11 libx11 libxcb vulkan-icd-loader vulkan-headers openssl zstd`)
   and `dav1d`, the system AV1 decoder that `dav1d-sys` links, the Arch
   counterparts of the Ubuntu jobs' packages, and `shared-mime-info`, the
   MIME database GIO types files with: without it `gio` types every file
   `application/octet-stream` and the real-GIO check in `system_preview::launch`
   fails, naming the missing database. It checks the libraries' `pkg-config`
   modules and prints the installed versions of the main packages. Git must be installed
   before checkout, which otherwise downloads a tarball without `.git`.
2. As root, create a `builder` account with the UID that owns `RUNNER_TEMP` (the
   hosted runner's account, so the account can write `HOME` and the step command
   files), and check that it holds no effective capabilities. Every later `run`
   step executes as `builder` through the job's default shell (`setpriv --reuid
   --regid --init-groups`); only the package, account and ownership steps name the
   root shell. The container starts as root, and root ignores file permissions:
   the preference and theme export tests that expect a write into a read-only
   folder to be refused would fail, and the discard test that needs an unreadable
   file skips itself for root.
3. Point `GIT_CONFIG_GLOBAL` at a fresh file under `RUNNER_TEMP`, as the other
   Rust jobs do (with `GIT_CONFIG_NOSYSTEM=1`), and keep `RUSTUP_HOME` under
   `RUNNER_TEMP`. The setup action sets `CARGO_HOME` to its own isolated root.
4. Check out with `persist-credentials: false`, then `chown -R builder` the
   workspace. Checkout runs as root and adds only the workspace path as
   `safe.directory`, in that isolated file; after the `chown`, `builder` owns the
   repository, so its own Git commands and `crates/app/build.rs` pass Git's
   ownership check without the exception.
5. `rustup toolchain install` reads `rust-toolchain.toml`, so the job builds with the
   same Rust 1.99.0 as the other runners.
6. Call the [setup action](#bounded-rust-dependency-caching) with `profile: debug`,
   `vendor-reuse: true`, `container: archlinux` and `run-as: builder`, which keys
   and restores the cache (below), then install nextest and run the shard's tests. In
   shard 1 only, after the doctests, the action's `finish` phase bounds the payload and,
   on a push to `main` only, registers the save.

The job keeps the workflow's `contents: read` permission, reads no secret and uses
only the pinned `actions/checkout` and `actions/upload-artifact` and the setup
action, whose one third-party action is the pinned `Swatinem/rust-cache`. Its measurements
and sanitized logs go into the same three-day diagnostics artifacts as the other
jobs, one per shard (`ci-diagnostics-rust-arch-Linux-X64-shard-<n>-…`), including a
`rust-toolchain` measurement.

**Image pin.** The image is pinned by the digest of its multi-platform index,
`archlinux:base-devel@sha256:8745817f349ed24373341ddb92776209eeec3f0364ea48f7f645ac5800d30a50`.
It was read from the Docker Hub registry (`registry-1.docker.io`, the `base-devel`
manifest's `Docker-Content-Digest`) on September 28, 2026, and matches the Docker
Hub tag API. The index holds one `linux/amd64` image
(`sha256:8185e444e45ba166146b244b41f1cba2d7d91f3eddce533a839cc9591e0fa785`) and its
attestation, created September 21, 2026 from
[archlinux-docker](https://gitlab.archlinux.org/archlinux/archlinux-docker) revision
`40bf6d7fea891afa211c6ec66458b84aa979c39a`. To move the pin, read the current
digest and update the workflow's image and dated comment and this paragraph:

```sh
token=$(curl -fsS 'https://auth.docker.io/token?service=registry.docker.io&scope=repository:library/archlinux:pull' \
  | python3 -c 'import json, sys; print(json.load(sys.stdin)["token"])')
curl -fsSI -H "Authorization: Bearer $token" \
  -H 'Accept: application/vnd.oci.image.index.v1+json' \
  -H 'Accept: application/vnd.docker.distribution.manifest.list.v2+json' \
  https://registry-1.docker.io/v2/library/archlinux/manifests/base-devel | grep -i docker-content-digest
curl -fsS https://hub.docker.com/v2/repositories/library/archlinux/tags/base-devel \
  | python3 -c 'import json, sys; d = json.load(sys.stdin); print(d["digest"], d["last_updated"])'
```

A `.github/` change runs every lane, so the pull request that moves the pin also
runs this job. Refresh it when the upgrade step fails to verify package signatures
(an old image's keyring predates new packager keys) or when the upgrade grows
slow because the image has fallen far behind the repositories.

**Rolling release.** The pin fixes the starting image, not the packages. `pacman
-Syu` installs the current Arch packages at run time, as Arch requires: it does not
support partial upgrades, so installing new packages onto an old image without the
upgrade can break. Two runs of the same commit can therefore build against
different compilers and libraries, and a failure can come from an Arch update
rather than the change under test. Compare the printed package versions of a
failing run with the last passing one before blaming the change. A mirror or
keyring failure fails the job; nothing retries it.

**Cache.** The job shares the other lanes' mechanism: the [setup
action](#bounded-rust-dependency-caching) and its pinned `rust-cache`, with the same
`debug` payload limit, cleanup, vendored reuse and main-only save. Its
`container: archlinux` input changes three things.

- *Key.* The native part of the key is `pacman -Q` over a fixed list of packages
  whose versions reach compiled output, `ARCH_NATIVE_PACKAGES` in the
  [helper](../.github/actions/setup-rust/cache.py): the C toolchain and its
  headers (`binutils clang cmake gcc gcc-libs glibc linux-api-headers llvm-libs
  make pkgconf`), and the libraries that build scripts probe or the test binaries
  link (`dav1d fontconfig freetype2 libx11 libxcb libxkbcommon libxkbcommon-x11
  openssl wayland`). `freetype-sys` links the system FreeType, `dav1d-sys`,
  `yeslogic-fontconfig-sys` and `aws-lc-sys` probe pkg-config (`dav1d-sys` for
  the libdav1d it links, `aws-lc-sys` for `libcrypto` and `openssl`), and the app
  links `libxcb` and `libxkbcommon`. The job's other packages
  (`ARCH_RUNTIME_PACKAGES`) are used only at run time: `vulkan-headers`,
  `vulkan-icd-loader` and `zstd` by the tests and its `pkg-config --exists`
  check, and `shared-mime-info` by the tests, as the MIME database `gio` reads.
  No build script probes them and nothing links them at build time. Nothing else
  in the rolling inventory is keyed, so an update to, say, `git`, `python`, `systemd` or Vulkan
  keeps the entry. An update to any listed package gives a new key and a full cold
  build, never objects built against the old libraries. A test compares both
  lists with the job's install line, so a library added to the job but in neither
  list fails. `pacman -Q` fails the step if a listed package is missing. The setup step
  prints the keyed versions (`Keyed Arch packages: …`) next to the upgrade step's
  own list. The key string reads `debug-archlinux-<hash>`, so the entry is told
  apart from the Ubuntu debug entry in the cache list, although both run on Linux
  x64 runners.
- *Setup.* The action installs no `apt` packages and no toolchain: the job's own
  steps install both first, and the toolchain must exist before the key is
  computed because the key hashes `rustc -vV`.
- *Account.* The container starts as root. The helper becomes `builder` (every
  user and group ID, which also clears its capabilities, and it refuses to run as
  root, and requires the account's primary group to share its name, as
  `useradd --user-group` gives `builder`) before it touches Cargo storage, and
  upstream's `rustc` and `cargo`
  commands run through `setpriv --reuid=builder` via its `cmd-format` input.
  Cargo run as root would leave root-owned registry, lock and target files that
  `builder` cannot update. Upstream's archive extraction itself runs as root and
  keeps the archived owners, so a root step hands everything under the target
  and Cargo home to `builder` (`chown -R -h -P`, never following a link) before
  the helper checks the restore. The job still runs every test as `builder`
  and stores no credential.

Every run still upgrades to that day's packages first, behind the unchanged
signature check, so between two `main` pushes a pull request whose keyed packages
moved in Arch restores nothing and builds cold; the next `main` push whose first
shard succeeds seeds the new key. In the [hosted record](benchmarks/2026-09-29-ci-arch-warm-cache.md):
- The cold job took 927 to 974 s.
- The first `main` push saved the entry in 22 s after an 11 s bound, 4.73 GB of logical payload with no dependency evicted.
- On the first warm product pull request the job took 182 s. The restore took 26 s, 873 of 912 units were fresh, and only the three workspace members compiled. That was 103 s less than the slowest warm `Rust tests and Clippy` job of the same run.

**Budget.** The Arch payload is held to the `debug` limit (7 GiB of logical bytes
before registration) by the same helper and upstream cleanup as the Ubuntu debug
lane. Its first archive is 1,083,913,297 bytes (1.08 GB), against about 1.42 GB for
the Ubuntu debug lane. Only the first shard of a push to `main` registers a
save, after its own steps succeeded and only when no entry has that exact key, so the lane adds one entry per
new key that `main` builds: a keyed package update or a keyed source change. GitHub removes an entry 7 days after its last
access and, once the repository is over its 10 GB limit, evicts by last access. An
Arch entry superseded by a package update is no longer restored, so it goes before
the Ubuntu and macOS entries that every product run restores. The repository was
already over the limit before the first Arch entry: at 22:48 UTC on September 29,
2026, it held 11 entries, 11.36 GB. The four other lanes' keys then also moved with the
runners' native inventories, and in five hours `main` saved 11 of their entries
with no keyed source change (the record has the list). By 00:14 UTC on September 30,
GitHub had evicted only the seven oldest entries, last accessed by 20:19, and the
Ubuntu, macOS and Arch entries that current runs restore all survived. They total
7.47 GB, counting two Ubuntu keys, because two runner images were in service. The
other 4.11 GB, the four lanes' entries from before #92, were superseded and go first.
A third Ubuntu image (2.28 GB more) and a macOS rollout (1.83 GB) at the same time
would pass the limit and evict entries still in use. Since November 19, 2026 the
Ubuntu lanes no longer key the image's inventory: like this lane they key only the
packages a build reads, found as a dependency closure instead of a written list
([Ubuntu native key](#ubuntu-native-key)), so two images that install the same build
packages share one entry.

**Cost.** Before the cache, every run was cold: an image pull (about 308 MB
compressed), the package upgrade and install, the toolchain download, a full debug
build of every dependency, then the tests. Its first five hosted runs, on
September 28, 2026, took 12m22s to 16m06s: runs 36395638428 (pull request #69),
36431876305, 36452704324 and 36458949036 (`main` at `1a9fc3e`, `7184b08` and
`8b9fd3d`) and 36501903050 (pull request #74). In run 36622842027 the container
start took 16 s, the upgrade and install 10 s and the toolchain 6 s, while the
nextest step took 902 s, almost all of it compiling dependencies. With an exact
hit the image pull, upgrade and toolchain remain, the dependencies are restored
fresh, and only the workspace members and tests compile, as on the Ubuntu lane.

**Docker Hub limits.** The runner pulls the image from Docker Hub without
credentials. Docker [counts unauthenticated pulls](https://docs.docker.com/docker-hub/usage/pulls/)
per IPv4 address or IPv6 /64 subnet and documents no exemption for GitHub-hosted
runners; users have reported rate-limit errors on hosted runners since mid-2024
([discussion](https://github.com/orgs/community/discussions/130794)). A rate-limited
pull fails the job before any step runs, and a rerun is the remedy. Authenticating
the pull would add a secret to Quality and would not help fork pull requests,
which receive no secrets.

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
   under each profile's pre-registration budget. Separate cold PRs from a successful trusted-main
   seed and genuinely restored warm PR/main samples on **both** platforms.
3. Quality creation-to-completion and summed runner intervals across **all** jobs,
   including inexpensive jobs and cache post-actions. Include compatibility checks
   in historical runs that emitted them. More simultaneous jobs can increase
   queue contention and runner minutes even if
   the visible critical path falls. Duplicate Linux package setup/downloads and
   profile-specific build scripts can increase cold cost; report this regression.
4. The full merge path including every active required check. A faster release
   lane alone does not prove a faster merge. Record sample counts, median/tail and
   unavailable observations; a small set does not establish production p95.
5. Real negative PR cases for each Rust phase: failure, cancellation and unexpected
   skip, plus docs-only justified skips. Verify the actual gate remains unsuccessful
   for a failed required platform; the earlier overlap evidence must also cover
   both legacy required names. Local result fixtures establish the evaluator's
   contract, not GitHub's matrix execution.

The [September 16 record](benchmarks/2026-09-16-ci.md) holds the first hosted
warm observations: one controlled PR run reached the Quality gate 7m39s after
creation against 20m33s cold. Those are single samples, so C1 stays open until
the record's [remaining evidence](benchmarks/2026-09-16-ci.md#remaining-evidence)
is collected. If cold overhead,
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
  the durations explicitly reported by completed test harnesses: libtest's
  `test result: … finished in` lines and nextest's `Summary [ … ]` run duration. These distinguish
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
formatting, the nextest installation, workspace tests, doctests, Clippy, release
compilation and Linux package checks with `measure`. The wrapper preserves the command's result while recording timing
and a bounded, sanitized log. The summary step runs after failures and lists the
available measurements. Each command produces `<name>.json` and `<name>.log`.
Logs retain at most 1 MiB, with a truncation marker when older output is dropped;
individual lines over 16 KiB are omitted. Tests (nextest passes the flag to its
Cargo build), doctests, Clippy and release builds request Cargo `--timings`. The helper copies a newly produced timing report of up to 2 MiB
as sanitized plain text, for example `tests-timing.txt`, instead of retaining an
executable HTML artifact. Missing or oversized timing artifacts remain unavailable.
The uploaded diagnostic artifact expires after **three days**. Quality uses
the pinned [upload-artifact v7.0.2 action](https://github.com/actions/upload-artifact/releases/tag/v7.0.2);
its [inputs](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/action.yml)
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
  -- cargo nextest run --locked --workspace -P ci --no-fail-fast --timings
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

### Development-tooling job on macOS

For documentation and tooling pull requests, the macOS development-tooling job
is the critical path, and the agent-loop suite takes most of it. Never-merge
probe [run 36284689192](https://github.com/FernandoX7/GitTurtle/actions/runs/36284689192)
(September 27, 2026, `unittest --durations`) found that all 233 tests passed and
four end-to-end controller classes took 94% of the macOS time. Nearly all of
that time was spent inside controller Git writes. Each of the suite's roughly
1,000 writes goes through `run_process`, and the supervisor and watchdog each
waited for a fixed 50 ms tick. A 5-20 ms write therefore cost 110-135 ms on
Linux and 230-350 ms on macOS. The watchdog's Python start-up, the guard
process and seven fsyncs per write make up the rest. Those are the controller's
crash-safety and durability mechanisms, so they stay.

The supervisor now waits on the watchdog with `Popen.wait(timeout=0.05)`. The
watchdog waits on its control pipe, backing off from 1 ms to 50 ms. Short writes
finish within milliseconds, and a stop request or supervisor exit wakes the
watchdog at once. Nothing else changes: spawning, process groups, the guard,
signals, cleanup and durable records. Every agent-loop test still runs on both
platforms, and none was skipped or removed.

| Measurement (macos-15 unless noted) | Before | After |
| --- | --- | --- |
| Agent-loop suite, probe runs | 251 s, 278 s ([36284689192](https://github.com/FernandoX7/GitTurtle/actions/runs/36284689192)) | 119 s, 117 s ([36285858330](https://github.com/FernandoX7/GitTurtle/actions/runs/36285858330)) |
| Agent-loop suite, ubuntu-24.04 probe | 137 s | 69 s |
| Agent-loop step inside the tooling job | 281 s, 293 s (runs 36284332195, 36284341103) | 162.5 s, 141.4 s (run 36285853931 attempts 1 and 2) |
| `Development tooling · macos-15` job | 343 s, 351 s (same runs) | 230 s, 206 s (same run and attempts) |

The before and after runs differ only in `scripts/agent_loop/process.py` and
`records.py`. On a 3-CPU hosted runner, both the per-test profile and the job
durations vary by about 10%.

### Later measurements and C1

The [September 16 record](benchmarks/2026-09-16-ci.md) documents the first
trusted-main seed after the capacity repair (all four lanes saved; 3.91 GB of
archives), a controlled warm PR run that reached the Quality gate 7m39s after
creation against 20m33s cold, an external fork contribution that restored
the same archives read-only, and a warm main push that found every key already
present and uploaded nothing. Those are single observations; medians and tails,
a new cache generation after a dependency change, hosted invalidation and
corrupt-restore diagnostics, and save overhead on later main pushes remain
pending.

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
setup and the lifetime of optional compilation caches. Each compilation job that
writes a cache, including the [Arch Linux container job](#arch-linux-container-job) in its
`container: archlinux` mode, calls its
`setup` phase before validation and its matching `finish` phase **after every
consumer of `target`**, including package construction and installation checks.
Finish may remove compiled outputs to bound a successful main cache. Keep future
binary/artifact consumers before that boundary. The Clippy jobs and the second
[test shard](#test-shards) call `setup` only:
they restore their platform's debug entry and never save, so the test job's first shard stays its
only writer. Every test shard always runs its slice of the locked workspace tests with nextest, and
the first then runs the doctests; the Clippy job always runs strict all-target Clippy; the separate
release job always runs locked optimized compilation, regardless of a cache hit.
Formatting has no compilation cache or native setup.

### Tool choice and compatible reuse

The action pins
[Swatinem/rust-cache 2.9.3](https://github.com/Swatinem/rust-cache/tree/d1527c8f67ef9af357290475d4af4b0fab8f12e0)
to `d1527c8f67ef9af357290475d4af4b0fab8f12e0`. Version 2.9.2 (`6323deb102c322ba6fcbdcafc7e3dddab59af2b6`) was inspected on
September 15, 2026, and the move to 2.9.3 on November 18, 2026: between the two
commits `src/workspace.ts` and `src/save.ts` are unchanged, `src/config.ts` differs
by one log level, and `src/cleanup.ts` awaits two nested target clean-ups and lets
the week-old sweep, which runs only after a partial match that this action discards,
pass every entry instead of stopping at the first. The upstream key hashes are
unchanged; this action's own files are hashed into the key, so the first `main` run
after the move seeds every lane cold.
Its Node 24 implementation uses the GitHub cache service and removes nondependency
outputs before saving. No sccache, paid runner, toolchain change or product profile
optimization is introduced. The established action disables Cargo incremental
artifacts (`CARGO_INCREMENTAL=0`); workspace optimization, debug information, LTO
and codegen settings remain those in `Cargo.toml`.

#### Vendored path packages

The pinned implementation's
[package selection](https://github.com/Swatinem/rust-cache/blob/d1527c8f67ef9af357290475d4af4b0fab8f12e0/src/workspace.ts)
keeps outputs only for packages whose manifest lies outside the configured
workspace root, plus the declared members when `cache-workspace-crates` is true
([save.ts](https://github.com/Swatinem/rust-cache/blob/d1527c8f67ef9af357290475d4af4b0fab8f12e0/src/save.ts)).
The four vendored GPUI/Mermaid patches are neither: they sit beneath the root, are
excluded from the workspace and enter through `[patch.crates-io]`, so with the root
at `.` they were pruned on every save whatever that flag said. They then rebuilt on
every warm run and held the critical path: on the warm runs recorded in
[the vendor warm-reuse record](benchmarks/2026-09-26-ci-vendor-warm-reuse.md)
`gpui-component` alone compiled for 110-158 s of a 154-234 s test build.

The action therefore roots upstream at `crates -> ../target`. Cargo metadata run
from `crates/` still resolves the root workspace and the same `target`, but now
every registry, Git and `vendor/` package lies outside the root and keeps its
outputs, while the three members under `crates/` are still pruned.
`cache-workspace-crates` stays false: member sources are not part of the key and
change on ordinary pull requests, so their outputs would only cost space. The
finish helper cleans local path packages with Cargo's whole-package cleanup, except
those under `vendor/`, and never selects a vendored package for dependency
eviction; an oversized payload falls back to discarding the whole target as
before. The budget record reports `retained_vendor_packages` and
`retained_vendor_bytes`. This relies on the pinned upstream's selection rule and
its `cargo metadata` working directory; re-verify both before changing the
pinned commit. With the root at `crates/`, upstream no longer finds `Cargo.lock`
for its own lockfile hash; the local key below hashes the lockfile's bytes, so
compatibility is unchanged.

Kept outputs alone would still rebuild. Cargo treats a path package as dirty when
any source listed in its dep-info is newer than that dep-info, and checkout gives
every file the current time. Quality's four compilation jobs (`rust-debug`,
`rust-clippy`, `rust-release` and `rust-arch`) therefore pass the action's `vendor-reuse: true` input, and setup rewinds every tracked file under
`vendor/`, and each directory up to `vendor/` (a build script may watch a
directory), to one fixed time: Cargo's own deterministic registry timestamp,
2006-07-24. It runs in the prepare step, after the key is computed and before the
restore and any Cargo command, on hits and misses alike (a cold build has no
fingerprints to satisfy). This is safe only because every rewound file's bytes are
in the key and a non-exact restore is discarded, so a vendor change is always a
new key and a full rebuild, never a rewound source beside a stale output. It is
never applied to `crates/`, whose sources the key does not hash. A symlink or
other nonregular vendored entry refuses the step without rewinding anything; the
restore record then shows `vendor_mtimes: refused` and the vendored packages
rebuild. Paths under Cargo's home, such as the registry icon directory that
`gpui-component`'s build script watches, are skipped by Cargo's own staleness
check and need no rewinding.

Vendored reuse is Quality-only. The input defaults to false and the release
workflow keeps that default, so its vendored sources keep their checkout times:
any vendored output restored from a Quality-seeded entry is dirty, and Cargo
compiles the GPUI/Mermaid patches from the tagged checkout, as it did before
vendored outputs were cached. The restore record then shows
`vendor_mtimes: disabled`. The finish helper exempts vendored packages from
cleanup and eviction regardless of the input, because only Quality's pushes to
`main` register a save.

[Upstream key construction](https://github.com/Swatinem/rust-cache/blob/d1527c8f67ef9af357290475d4af4b0fab8f12e0/src/config.ts)
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
- The platform's native inputs. On Ubuntu: the version of every installed package
  in the dependency closure of the build packages the action installs, the file
  and version behind each build tool name on `PATH`, and pkg-config's search order
  ([Ubuntu native key](#ubuntu-native-key)); the rest of the hosted image's
  inventory is not keyed. On macOS: the Xcode version, SDK version/build, selected
  Clang and OS build, so a hosted-image update there still gives a new key. The
  Arch container keys its listed packages' pacman versions
  ([Arch cache](#arch-linux-container-job)). Each covers native ABI/SDK changes.
- The active compiler's full `rustc -vV` and the target triple it reports as
  `host` (each keyed job compiles for its host), and the build flags set in the
  environment by name: `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `RUSTDOCFLAGS`,
  `RUSTC`, `RUSTC_WRAPPER`, `RUSTC_BOOTSTRAP`, `CARGO_INCREMENTAL`, `CC`, `CXX`,
  `AR`, `CFLAGS`, `CXXFLAGS`, `CPPFLAGS`, `LDFLAGS`, `LIBRARY_PATH`, `SDKROOT`,
  `MACOSX_DEPLOYMENT_TARGET`, and any `CARGO_BUILD_*`, `CARGO_PROFILE_*`,
  `CARGO_TARGET_*` (except `CARGO_TARGET_DIR`), `CMAKE*`, `PKG_CONFIG*` and
  per-target `CC_*`, `CXX_*`, `AR_*`, `CFLAGS_*` or `CXXFLAGS_*` variable. Upstream's
  own key hashes the same compiler and much of the same environment; these are
  in the local key too, so the helper's tests can show each one invalidates.

The raw manifest/lock digest is part of the compatibility prefix; unlike upstream's
broader fallback, this deliberately does not restore an older dependency graph.
Compiler/flags/platform/source mismatches cannot fall back to an incompatible
prefix. The full upstream key remains in the Actions cache log; local diagnostics
retain its setup prefix. Changing the versioned `gitturtle-rust-v2` prefix is a
reviewable way to isolate a cold experiment without deleting another job's cache.
Any edit to the setup action already changes the key because its files are hashed;
the prefix moved from `v1` to `v2` with vendored reuse so the two generations are
distinguishable in the cache list.

#### Ubuntu native key

Since November 19, 2026 the Ubuntu lanes key the packages a build reads instead of
the hosted image's whole package inventory. The
[helper](../.github/actions/setup-rust/cache.py) takes one `dpkg-query` snapshot
after the action's install step (package, architecture, version, status,
Pre-Depends, Depends and Provides of every package; nothing queries dpkg a second
time) and hashes three things.

- *Packages.* `name:arch=version` of every installed package in the dependency
  closure of the twelve packages a build compiles with, probes or links
  (`UBUNTU_BUILD_PACKAGES`: `build-essential clang cmake pkg-config
  libfontconfig-dev libwayland-dev libxkbcommon-x11-dev libx11-xcb-dev libssl-dev
  libzstd-dev libvulkan-dev libdav1d-dev`), and nothing else from the inventory.
  The closure follows Pre-Depends and Depends, not Recommends or Suggests (the
  action installs with `--no-install-recommends`). A name resolves on every
  architecture and through every package that Provides it, and every alternative
  that resolves is followed, so the result is a superset of what apt needed. Only
  a package in dpkg state `ii ` counts as installed. The action's seven other
  packages (`UBUNTU_TOOL_PACKAGES`: `git openssh-client ca-certificates python3
  python3-pil desktop-file-utils xdg-utils`) are run by the checkout, the tests or
  the packaging checks and are never read into a cached dependency output: `git`
  is run only by `crates/app/build.rs`, a member whose output is never in an
  entry. A test compares both lists with the action's install line, so a package
  added to the action but to neither list fails.
- *Tools.* For each of `cc`, `c++`, `ar`, `pkg-config`, `clang` and `cmake`: the
  real file the name resolves to on `PATH` and its `--version` output. `cmake3`,
  the cmake crate's other name, is keyed as absent or by the same two values. A
  required name that does not resolve fails the step.
- *pkg-config's view.* The search order (`pkg-config --variable pc_path
  pkg-config`) and, for each directory on it outside `/usr/lib` and `/usr/share`,
  the names and sizes of its `.pc` files. A clean image has none; a `.pc` under
  `/usr/local` would shadow a keyed library's. `PKG_CONFIG*` variables stay in the
  flags identity above.

**Why.** GitHub serves the `ubuntu-24.04` label from two clouds at once. In 64
retained job logs, image versions 20261111.370.1 and 20261116.376.1 report
"Cloud: Azure" and 20261111.127.1 and 20261116.133.1 report "Cloud: AWS". Each
cloud has its own image build and inventory, so with the inventory in the key one
commit computed two Ubuntu keys, and four during a weekly rollout. Of 47 Ubuntu
jobs that computed a key, 19 found no cache, and a miss costs a test shard about
28 minutes against about 13. The cache held three Ubuntu debug entries of 1.1 GB
each against the 10 GB limit for everything, and a fourth key had none. On all
four images the action's `apt-get` step printed identical lines, so the keys
differed only through packages the action does not install.

**Why a closure.** A build of the workspace's tests was traced with `strace`
(`cargo test --no-run` in an `ubuntu:24.04` arm64 container holding the action's
install list). It read files of 75 packages. 71 lie in the closure, which holds
195 of that container's 278 packages. The other four are `dash` (`/bin/sh`),
`git`, `libc-bin` (`/etc/ld.so.conf`) and `openssl` (`/usr/lib/ssl/cert.pem`). A
written list like [Arch's](#arch-linux-container-job) would silently lose
Ubuntu's version- and architecture-named toolchain packages, such as
`gcc-13-aarch64-linux-gnu` and `libstdc++-13-dev`; the closure reaches them, and
it keeps `linux-libc-dev`, whose headers `libc6-dev` includes. The container's
snapshot is the tests'
[fixture](../scripts/ci/tests/fixtures/ubuntu-24.04-arm64-dpkg.tsv).

**What the narrower key gives up.** The image's other packages, about 2,000: a
build script that found a library, header or tool the action does not install
would no longer change the key (the first precondition below). And the state
outside dpkg that the old key followed only by accident, because a new image
changed some package: `PATH` order, `/usr/local` and the alternatives. The tool
and pkg-config parts key that state directly for the build tools and for
pkg-config. The hosted image carries a CMake under `/usr/local` that is no dpkg
package, and a version alone does not say which binary `PATH` and the
alternatives select.

**Fallback.** A build package that is not installed fails the step, as `pacman
-Q` does on Arch. Every other abnormality sends the key to the complete
inventory, never to a partial closure and never past a skipped dependency: a
snapshot line without six fields, a dependency or provided name outside dpkg's
grammar, a group of alternatives that nothing installed satisfies, or a package
the closure reaches in another state than `ii ` (unpacked, half installed, or
removed with its configuration left). The fallback hashes every line's package,
architecture, version and status, and the step prints a `::warning::` that names
the first cause. The digest's input starts with the mode (`closure` or
`inventory`), so a fallback key can never equal a closure key. Failing there
would let an optional cache block a release, and skipping would be a stale key;
the fallback is one entry per image again, slower and never stale.

**Preconditions.** The width holds only while two things stay true.

1. Every locked dependency whose build script probes for a library, header or
   tool finds it in the build packages. The image holds about 2,000 packages the
   action never installs, and a dependency added later could probe one of them
   without the install line changing. Today the users of `pkg-config`,
   `system-deps`, `cmake` and `bindgen` in `Cargo.lock` are `aws-lc-sys`,
   `dav1d-sys`, `freetype-sys`, `gpui-pre`, `gpui-pre-media`, `khronos-egl`,
   `system-deps`, `wayland-sys`, `x11` and `yeslogic-fontconfig-sys`. A test
   reads the lockfile and requires that set to equal the keys of a table, filled
   from each build script, that maps a user to the build packages supplying what
   it probes on Linux or to the reason it probes nothing there. A new one fails
   the test until it is mapped, and the package it probes is added to the
   action's install list and to `UBUNTU_BUILD_PACKAGES` if it is new. The table
   is by name: read the build script again when one of these packages is
   updated.
2. `dav1d-sys` links the system `libdav1d-dev`. Its build script can instead
   clone and build dav1d with git, meson and ninja, none of them keyed, when a
   `SYSTEM_DEPS_` variable asks for it. The action's install step refuses every
   such variable, and the key relies on that refusal. A test runs the step's
   script with such a variable set and requires it to stop before anything is
   installed.

The snapshot is also the state the build sees only while no job that uses the
action installs packages in its own steps. A test refuses `apt-get`, `apt` and
`dpkg -i` in every such job of `quality.yml` and `release.yml`.

**What the step prints**, so two hosted runs can be compared line by line: the
mode; `Keyed Ubuntu packages: <n> of <m> installed`; inside a collapsed group,
each keyed `name:arch=version` on its own line, sorted; for each tool its real
path, every match on `PATH` in order, a short digest of its whole version output
and that output's first line; the `pc_path` and the `.pc` files; and the native
digest. Each line is built from the hashed values themselves.

The action's files are hashed into the key, so this change makes every lane cold
once. No hosted run has computed the new key yet: the hosted keys of the two
clouds are recorded here after the first runs.

### Trust, failure and storage bounds

Cargo cache storage is isolated under the fresh runner's temporary directory.
Only its `registry` and `git` subtrees and the workspace `target` are eligible;
Cargo binaries, configuration and credential files are excluded. Checkout keeps
`persist-credentials: false`, and existing Git configuration isolation and Linux
native prerequisites remain in place. No cache is a trusted release input; release
builds do not opt into [vendored reuse](#vendored-path-packages).

Setup always restores with saving disabled. Only the `finish` phase registers a
save, and only on a **push to `FernandoX7/GitTurtle`'s `main`**; PRs, fork PRs and
manual runs are read-only consumers. On every event so are the jobs that have no
`finish` phase: the Clippy jobs and the second [test shard](#test-shards) of each
platform. Each debug entry therefore has one writer, the first shard of its
platform's tests, and each release entry its release job. A job saves only after
its own earlier steps succeeded, so a job whose validation failed or was cancelled
cannot save. The save does not wait for the run's other jobs: on `main` the first
shard saves when its own slice of the tests and the doctests passed, even if a
test in the second shard, Clippy or a release build failed. The entry holds
dependency outputs keyed by their inputs, not results, and the run as a whole
still fails.
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

Before registering a save,
[.github/actions/setup-rust/cache.py](../.github/actions/setup-rust/cache.py)
resolves locked all-feature metadata (matching the upstream save graph), then
accounts logical file bytes plus a conservative 4 KiB per directory/file entry,
counting hardlinked aliases separately. Every entry is checked, including source
trees excluded from the projected payload; symlinks, special files, traversal
errors, more than 250,000 entries or an expired accounting deadline refuse saving.
The verified logical payload limits are **7 GiB for `debug` and 3.5 GiB for
`release`**; the older combined mode uses their 10.5 GiB sum. The debug limit was
6 GiB until September 28, 2026, for the reason in the paragraph after next.
The limits were first sized from
the [second trusted-main seed](benchmarks/2026-09-15-ci.md#four-budget-timeouts)
(run 35036670099), whose projected payloads after local cleanup and source pruning
were about 5.9 GB (Linux debug), 3.9 GB (macOS debug), 3.1 GB (Linux release) and
2.8 GB (macOS release), including about 0.46 GB of retained downloads per lane,
and from local `zstd -3 --long=30` measurements of a CI-like payload of the same
workspace: roughly 6.3× for debug outputs, 5.0× for release outputs and 1.2× for
archives and index. Applied to those hosted sizes, one generation of four archives
is estimated at roughly 3.3 GB compressed (under 4 GB even if every retained
download compressed only 1.2×) against the repository's 10 GB cache quota, of
which 0.2 GB was in use by historical CodeQL entries. Keeping vendored outputs adds
roughly 0.33-0.36 GB of logical payload per debug lane and 0.14 GB per release lane
(local measurement of the rlib/rmeta files, Linux), about 0.25 GB compressed per
generation at the observed archive ratios; the Linux debug lane's headroom under
its original 6 GiB limit fell to roughly 0.2 GB. These are local
pre-registration measurements and compression estimates, not compressed archive
sizes or unconditional archive ceilings.

The first measured generation, from the `main` run for `1a9fc3e` (run 36431876305,
September 28, 2026), confirmed that headroom: the Linux debug lane retained 6.21 GB
of its 6.44 GB (6 GiB) limit, 0.33 GB of it vendored outputs, and evicted no
dependency. The four lanes retained 6.21 GB (Linux debug), 3.74 GB (macOS debug),
2.88 GB (Linux release) and 2.68 GB (macOS release), and the repository's cache
inventory listed their archives at 1.42 GB, 1.04 GB, 0.86 GB and 0.79 GB: 4.11 GB
per generation rather than the 3.3 GB estimate, because the Linux debug payload
compressed about 4.4× rather than 6.3×. With 0.23 GB left, the next growth in
dependencies would have evicted up to eight dependency groups before every Linux
debug save, so that lane's warm runs would recompile them without any failure to
show it. The debug limit therefore became 7 GiB (7.52 GB). At the observed ratio the
extra 1.07 GB of payload costs at most about 0.25 GB of archive, so one generation
stays under about 4.4 GB of the 10 GB quota; the macOS debug lane is far below
either limit. The helper is part of every lane's cache identity, so the first `main`
run after the change seeds all four lanes cold. The AArch64 Linux release leg added on
2026-11-15 saves an entry of its own from `main` pushes; its size has not been measured
against the quota.

The helper projects only the extracted registry package directories that pinned
[rust-cache registry cleanup](https://github.com/Swatinem/rust-cache/blob/d1527c8f67ef9af357290475d4af4b0fab8f12e0/src/cleanup.ts)
removes. It preserves current `*-sys` source directories because native build
scripts can depend on their timestamps, and counts all Git databases and checkouts.
The [Cargo cache layout](https://doc.rust-lang.org/cargo/guide/cargo-home.html)
contains both downloadable archives and extracted sources. The helper previously
counted those removable copies against the compiled-output allowance. The first
trusted-main seed discarded every target and refused all four saves, but its
diagnostics did not measure the removable-source volume. The second seed measured
them (about 1.33 GB per lane) and supplied the payload sizes above, then failed
every lane by exhausting its budget. Both
[observed failures](benchmarks/2026-09-15-ci.md#four-cache-budget-refusals)
are retained separately from repair validation.

After removing local package outputs outside `vendor/` through `cargo clean --locked
--profile <profile> --package`, the helper may clean up to eight largest dependency package
groups. Packages are ranked by bytes attributed only from structured locations
relative to the target root: the artifact file directly under `deps`, or the whole
package directory directly under `build` or `.fingerprint`, keyed by the name
before its trailing hash and matched against the package name and its library-kind
target names (`lib`, `rlib`, `dylib`, `cdylib`, `staticlib` and `proc-macro`, the
pinned upstream save targets). Test, example, bench, binary and build-script target
names and path components above the target root never score, so a dependency test
named `debug` cannot claim a whole profile tree and each `build_script_build`
binary counts only for its own package. Eviction is a growth safety valve rather
than the primary fit mechanism: with this attribution the eight largest dependency
groups hold roughly 1.0 GB (debug) and 0.5 GB (release) on a local build of this
workspace. Two separate time bounds apply. A 90-second scheduling budget governs
starting cleanup work: another dependency eviction begins only while the remaining
budget covers the last observed cleanup duration plus twice the last measurement
duration, one measurement of headroom for scan variance (each Cargo command has a
30-second timeout). A separate 90-second allowance then bounds
source pruning, the verification scan, the optional whole-target fallback and its
scan, so an exhausted eviction loop no longer fails the fallback. A scan that
overruns either bound refuses saving. Both local and dependency cleanup select
Cargo's `dev` profile for a debug job and `release` for a release job; the older
combined mode visits both profiles within each selected package group. A job
that compiles with an explicit `--target` passes that triple to its finish phase
through the action's `target` input (the release jobs pass their matrix target,
validated as a plain triple in both the action and the helper); each selected
group is then cleaned once for the host layout (`target/<profile>`, build scripts
and procedural macros) and once more with `--target` for the triple layout
(`target/<triple>/<profile>`, the dependency artifacts). Package-scoped clean
otherwise defaults to `dev` and, without `--target`, touches only the host layout,
so the earlier release cleanups were no-ops for both reasons even when the command
succeeded. The triple appears only in Cargo arguments, never in diagnostics.
Cargo owns removal of fingerprints, libraries and generated native outputs together.
Extracted sources stay present through every Cargo command, then only the
projected source directories are removed. A new scan must equal the projection
before save registration. If the actual payload still exceeds its ceiling, the
entire target is discarded and a bounded downloads-only payload may be prepared.
If downloads alone exceed the limit, or accounting/cleanup fails, no save is
registered. It never deletes individual build-script `OUT_DIR` members while
keeping their fingerprint. Budget fallback may reduce warm reuse and must be
measured.

Pinned upstream post-save metadata discovery recreates the removed pure-Rust
sources, then its registry cleanup removes them again. The pinned Cargo fixture
covers that sequence and a real local archive round trip. Upstream catches
cleanup errors, so the verified pre-registration bound is **not a guarantee that
every saved archive stays under its profile limit**: a failed upstream cleanup may
retain recreated sources. Actual post-save archive sizes, post-save upstream
behavior, useful compiled payloads and warm reuse on each hosted platform/profile
remain unverified until a new trusted-main seed. No aggregate archive ceiling
follows from the four local logical limits; the 3.3 GB figure is a compression
estimate, not an observed size.

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
ceiling (also on failure), retained pre-registration accounting, removed-package
count, whole-target fallback and save eligibility. Snapshots before/after local
cleanup, each dependency cleanup, source pruning and target fallback separate
logical bytes and entry
overhead for target, registry sources (retained/removable), archives/index, and Git
databases/checkouts. They count library files, generated build-script output files
and fingerprint files, with their logical bytes. Evictions include bounded safe
package names/versions and metadata ordinals, never full package IDs or source URLs.
Fixed stage names and elapsed times separate metadata, profile-specific Cargo
cleanup, dependency selection, accounting, source pruning and projection
verification. Completed snapshots,
stage timings and safe package identities survive a later budget failure with a
fixed failure reason and failed-stage name. Each package attempt reports whether
its cleanup commands completed; only the snapshots establish the resulting byte
change. No Cargo output, package source URL or local path is copied into diagnostics.
Both records live in the existing three-day diagnostics
artifact. The summary displays explicit cache values, preserving unknown fields
as unavailable and `false`/zero as observations.

Upstream exposes no compressed-byte or save-duration action outputs. Those remain
`null` before the job ends. The completed Actions job's restore/save step intervals,
upstream cache-size log and cache inventory provide later evidence. When preparing
a report export, attach a measured cache value only to its observed step; identify
whether save time includes cleanup/compression/upload and whether bytes describe
the compressed archive. Do not substitute the helper's pre-registration limit for a
compressed size. Include the final post action in end-to-end job and merge timing.

Local tests exercise source/vendor/native/profile invalidation, isolated path
refusal, partial restore removal, bounded whole-package cleanup and truthful cache
summaries. The opt-in
[consumer fixture](../scripts/ci/tests/test_rust_cache_consumer.py) uses the workspace's
pinned Cargo with a disposable loopback sparse registry (pure-Rust and native
`*-sys` packages), a commit-pinned local Git dependency and generated build output.
It checks debug and release source recreation/pruning and local archive restoration, then proves
retained dependencies are `Fresh`, the local consumer recompiles, preserved native
source/output timestamps stay valid, and a fully evicted native package rebuilds
and links successfully. A separate local-crate recovery fixture covers both profiles
in the older combined mode. A vendored-reuse fixture builds an excluded `vendor/`
path package and a `crates/` member, shows that checkout times alone recompile the
kept vendored package, that after rewinding it stays `Fresh` while the cleaned
member recompiles, and that a vendored content change produces a new key. Run the optional Cargo fixtures explicitly with
`GITTURTLE_CACHE_CARGO_QA=1 python3 -B -m unittest discover -s scripts/ci/tests -p 'test_rust_cache*.py'`;
ordinary development-tooling jobs do not install a Rust toolchain just for these
fixtures. These checks establish source behavior, not a hosted cache hit, pinned
action archive execution or speed improvement. C1 still requires a reviewed main revision to seed trusted
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
