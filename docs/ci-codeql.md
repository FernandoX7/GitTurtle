# CodeQL coverage and measured optimization

The [CodeQL workflow](../.github/workflows/codeql.yml) keeps security analysis on
pull requests to `main`, every `main` push and Tuesday's weekly scan. The native
Quality checks remain separate. This source change prepares an extraction-cache
experiment and reproducible diagnostics. It does **not** establish faster hosted
analysis or complete initiative checkpoint C2.

## Current setup and preserved identities

The September 15 baseline used GitHub default setup. Upstream commit
[`050be11`](https://github.com/FernandoX7/GitTurtle/commit/050be11009854d716e1b22d8b6423f36e551c6c5)
subsequently installed advanced setup so approved fork PRs receive scans. The
coordinator's later read-back found default setup `not-configured`. Preserve that
newer work: there is no activation variable or disabled PR-analysis path in this
optimization. The [existing setup/recovery procedure](public-launch.md#codeql-setup-and-recovery)
remains the route for account settings; preparing this patch changes none.

The four matrix jobs retain the names `Analyze (actions)`,
`Analyze (javascript-typescript)`, `Analyze (python)` and `Analyze (rust)`.
Their analysis categories remain `/language:actions`,
`/language:javascript-typescript`, `/language:python` and `/language:rust`.
These are deliberately the categories in `050be11`, without trailing slashes.
Historical default-setup Rust SARIF used `/language:rust/`; confirm its disposition
in the code-scanning configuration list during C2 instead of silently creating
another analysis category or deleting historical findings.

The default-setup API reported `actions`, `javascript`, `javascript-typescript`,
`python`, `rust` and `typescript`. The three JavaScript/TypeScript entries are
aliases for the same effective extractor, represented by one
`javascript-typescript` job. They are not six independent required scans.
[GitHub's language identifiers](https://docs.github.com/en/code-security/reference/code-scanning/workflow-configuration-options#changing-the-languages-that-are-analyzed)
describe that combined analysis.

The shared [configuration](../.github/codeql/config.yml) retains the default
security suites and sets `threat-models: local`, adding local sources to the
remote sources where supported. It is JSON-compatible YAML, so stdlib JSON
validation checks its syntax. There are no query, path, feature or vendor
exclusions. `remote_and_local` was the default-setup API value; it is not a
replacement workflow-input name. Threat-model support varies by language;
setting this option does not create support absent from an extractor.
[Configuration reference](https://docs.github.com/en/code-security/reference/code-scanning/workflow-configuration-options#extending-codeql-coverage-with-threat-models).

The existing [CodeQL merge ruleset](https://github.com/FernandoX7/GitTurtle/rules/23351839)
blocks high/critical security alerts and error-severity findings. Preserve that
rule, its existing bypass policy, and the Rust/Quality and review/conversation
requirements. A completed Actions job is not evidence that code scanning has
processed the correct revision or that its findings permit merging.

PR jobs keep the normal merge checkout, `pull_request` token restrictions,
`persist-credentials: false`, ephemeral hosted runners and no repository secrets.
There is no `pull_request_target`, fork-owner filter or privileged follow-up.
`security-events: write` requests the standard CodeQL upload capability;
GitHub applies the restricted fork context and
[accepts PR-event code-scanning uploads](https://docs.github.com/en/code-security/reference/code-scanning/troubleshoot-analysis-errors/resource-not-accessible).
`actions: read` permits the action's run metadata and diagnostics. No release,
package-publication, signing or notarization credentials enter analysis.

## What the measurements establish

Historical [run 34992342508](https://github.com/FernandoX7/GitTurtle/actions/runs/34992342508)
used CodeQL CLI **2.27.0**, Rust queries **0.1.42**, and action commit
`b96794f015dfd88f77b49b1c93e0fa7110f94c63` (**v4.38.0**). Its workflow lasted
**22m48s**, and its Rust job **22m43s**. Rust extraction took about **11m06s**:
manifest loading **6m09.705s**, source loading **1m56.108s**, project extraction
**33.007s**, and library extraction **1m27.603s**. Other extractor phases and
transition overhead account for the rest. Finalization took about **1m15s**;
query processing took about **9m49s**. Concurrent query evaluations share work,
so their individual elapsed times must not be added as a wall-clock total.

The paired historical Quality PR completed in **32m40s**, with the last required
check completing about **32m44s** after the earliest relevant run creation.
[CI measurement procedure](ci.md#reproduce-coldwarm-and-prmain-measurements) explains how to
recompute the entire required-check critical path. These observations predate
this source change and are neither new cold/warm distributions nor a speedup.

The pinned action and bundle are deliberately the measured versions. The
[immutable bundle release](https://github.com/github/codeql-action/releases/tag/codeql-bundle-v2.27.0)
contains the CLI, extractors and query packs. The installed version and extractor
schema are recorded. Update the bundle, the explicit compatibility check in
`scripts/ci/codeql.py`, and the measurements together; an Action dependency update
alone does not intentionally upgrade the pinned bundle.

### Supported Rust reuse and its limit

Rust's supported `build-mode: none` still loads Cargo manifests and can execute
build scripts and procedural macros. It does not remove query execution. The
[2.27.0 extractor schema](https://github.com/github/codeql/blob/codeql-cli/v2.27.0/rust/codeql-extractor.yml)
exposes `cargo_target_dir`; its default is a fresh scratch directory. The workflow
sets the source-supported `CODEQL_EXTRACTOR_RUST_OPTION_CARGO_TARGET_DIR` through
`GITHUB_ENV` **before init and analyze**. Rust extraction happens during analyze,
so an environment value scoped only to init would not apply. The schema is
verified in the installed bundle before restoring a cache.
[Extractor configuration source](https://github.com/github/codeql/blob/codeql-cli/v2.27.0/rust/extractor/src/config.rs).

The workflow installs the same relevant Linux native development dependencies
used by Quality before extraction. This tests whether native prerequisite errors
inflate manifest work or suppress macro semantics; it is not a claimed fix for
the baseline diagnostics. Both the installation cost and extraction time belong
in the comparison.

The dedicated cache contains only the extractor's target directory. It is
separate from Quality's Cargo cache, a CodeQL database, downloaded dependencies,
Cargo credentials and the query-result cache. Its exact key includes:

- The complete tracked Git tree, covering every vendored patch, manifest,
  lockfile, build script and tracked resource. This first experiment intentionally
  invalidates even a source-only edit; it has no broad restore prefix.
- The actual Rust compiler, CodeQL version and extractor schema.
- Runner OS/architecture/image version, installed native package versions and
  relevant compiler/extractor environment inputs, hashed without dumping the
  environment.

Restore and save are limited to trusted `main` pushes, schedules and dispatches
on `main`. PRs always extract from a fresh target. Standard cache lookup can see
an entry in the PR's own scope; a key copied by a PR is not proof it came from
trusted main. This deliberately avoids making that trust claim. Review
[GitHub cache access and matching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)
before expanding this boundary.

Only a successful analysis may save; debug runs do not seed the cache. The
candidate must contain ordinary files/directories, at most **100,000 entries**,
and at most **4 GiB** of uncompressed file lengths. A walk has a 30-second limit;
symlinks/special entries and oversized candidates skip saving without deleting
build outputs. This bounds each saved entry, not total repository cache storage;
GitHub's configured quota/eviction still applies. Hard-linked file lengths are
counted conservatively, and compressed bytes remain unavailable until the actual
cache service reports them. Cold dispatches skip restoration and can seed an
absent exact key. No hit, save, size or transfer duration is invented for a skipped
step.

This conservative cache can improve repeated main analysis if measured reuse
outweighs transfer cost. It **cannot establish a PR speedup**. C2 must report the
remaining PR critical path honestly, including native setup and diagnostic cost.

### Reuse and resources that are not available

At this action version, built-in
[dependency caching](https://github.com/github/codeql-action/blob/b96794f015dfd88f77b49b1c93e0fa7110f94c63/src/dependency-caching.ts)
has no Rust implementation. Rust also lacks the TRAP-cache schema and is absent
from the action's supported overlay-language map. No unsupported Rust overlay
flag, database replay, query omission or dependency-cache input is introduced.
Independent language jobs preserve automatic JavaScript/Python overlay
eligibility and automatic diff-informed PR analysis; their effective hosted
feature flags and cache use still need to be observed.
[Overlay implementation](https://github.com/github/codeql-action/blob/b96794f015dfd88f77b49b1c93e0fa7110f94c63/src/config-utils.ts).

The baseline already used **four threads** and **14,575 MB** of CodeQL RAM.
Standard public `ubuntu-24.04` x64 runners provide **four CPUs and 16 GB**.
The action defaults use the available resources; raising `threads` or `ram`
cannot add CPUs or memory. The Rust loader source sets one loader worker and one
proc-macro process and exposes no supported tuning option for them.
[Runner resources](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
[pinned action inputs](https://github.com/github/codeql-action/blob/b96794f015dfd88f77b49b1c93e0fa7110f94c63/init/action.yml),
[Rust loader](https://github.com/github/codeql/blob/codeql-cli/v2.27.0/rust/extractor/src/config.rs).

If PR Rust remains near 22 minutes, the largest measured opportunities are
manifest loading and the nearly ten-minute query phase. First inspect one
separate `diagnostics=true` run for failed build scripts/proc macros and the slow
query plans. Do not count its added logging as a comparable timing sample.
A concrete next A/B experiment is the existing defaults versus `threads: 2` and
`ram: 12288` on the same four-CPU/16-GB runner, retaining every query and collecting
at least three completed PR samples per arm. This tests whether lower concurrent
query memory pressure helps; it may instead be slower. Apply the same values to
init/analyze, record peak memory and query-plan diagnostics, and keep the default
unless the full required-check time improves without coverage loss. Defaults
already use the available allocation, so no improvement is assumed.
Faster hardware or additional macOS semantic analysis requires a separately
reviewed capability/resource choice; this workflow requests no paid runner.
Removing slow summary/security queries or excluding maintained vendor code is
not an accepted substitute.

## Four baseline warning files and platform coverage

The Rust summary reported **600 files without error** and **four files with
errors**. The latter metric explicitly includes warnings, not just fatal
extraction failures. The eleven WARN messages reconcile to these four files:

- `crates/app/src/shortcuts.rs`, lines 310 and 319: failed expansion of `shortcut`.
  The underlying expansion cause remains unresolved until diagnostic build-script
  and macro evidence is examined on the new source.
- `vendor/gpui-pre-macos/src/display_link.rs`, lines 175, 308, 430, 435, 438, 454
  and 463: failed `anyhow::bail`, `foreign_type` and `anyhow::ensure` expansions.
  This is maintained vendor code and stays in scope. Linux-host semantic context
  is a possible contributor, not a demonstrated complete explanation.
- `docs/benchmarks/large-pr-probe.rs`, line 1: no semantic analyzer because the
  file is outside loaded manifests. Syntax extraction remains; macro semantics
  are limited. Do not silently add a new Cargo member or drop the file.
- `docs/experiments/native-glass-prototype.rs`, line 1: the same known standalone
  Rust-file limitation. It remains analyzed with that limitation recorded.

The fixture [codeql-rust-baseline.txt](../scripts/ci/tests/fixtures/codeql-rust-baseline.txt)
contains sanitized genuine run diagnostics; it is a parser regression fixture,
not a new scan. The
[upstream summary query](https://github.com/github/codeql/blob/codeql-cli/v2.27.0/rust/ql/src/queries/summary/NumberOfFilesExtractedWithErrors.ql)
and [warning query](https://github.com/github/codeql/blob/codeql-cli/v2.27.0/rust/ql/src/queries/diagnostics/ExtractionWarnings.ql)
establish the warning-inclusive interpretation.

Additional INFO messages identify platform-excluded files with syntax-only
semantics, including macOS preview and toolkit modules. They do not appear in
that four-file warning total. A clean Linux job or a smaller file count would
not demonstrate equivalent macOS semantic coverage. Retain those identities;
macOS verification remains explicitly open under the initiative's platform
handoff. A Darwin target flag alone does not supply Xcode/SDK/build-script support.

## Collect comparable evidence

The Rust job saves `codeql-context-rust-RUN-ATTEMPT` for three days. It includes
compatibility identity, cache candidate bounds and the decoded existing default
query results for extracted-file identities, extraction errors/warnings, summary
counts and extractor telemetry. `capture` uses the pinned CLI's
[`bqrs decode`](https://docs.github.com/en/code-security/reference/code-scanning/codeql/codeql-cli-manual/bqrs-decode);
it does not run a second security query suite. Missing/failed result decoding is
explicitly unavailable in `rust-results-index.json`, not empty clean coverage.
Each command is limited to 30 seconds and 16 MiB of captured output. Only
sanitized result JSON is retained, with checkout-relative file identities.
The separate debug option can produce CodeQL's larger debug/database artifact;
use it deliberately and preserve the exact source/run identity.

After a run completes, retain its jobs, full logs and scan-analysis records in a
private coordinator evidence directory. The existing metrics collector supplies
job/step and full merge-path measurements. The offline Rust parser accepts a
saved full `gh run view --log` export:

```sh
python3 scripts/ci/codeql.py report saved-run.log --output rust-observations.json
```

The parser distinguishes extractor phase timings, extraction/finalization
boundaries and query-start-to-interpretation duration. Duplicate or absent
observations remain unavailable. Its query boundary includes interpretation
startup and differs slightly from CodeQL's internally reported query duration;
record the boundary when comparing. It never infers coverage equivalence.

For C2, preserve at least these distinct experiments:

1. The current advanced-workflow baseline on the same source/runner/bundle, then
   the optimized PR run. Record changed prerequisites and all four completed
   uploads, processed findings and expected categories. Historical default setup
   alone is insufficient to explain changes introduced by `050be11`.
2. A cold main dispatch (`cold=true`, `publish_results=false`, `diagnostics=false`),
   followed by a compatible main dispatch with restoration enabled. Record the
   saved/matched key, cache service bytes, restore/save time, extraction phases,
   finalization, queries and total runner time. Neither upload-never run satisfies
   merge protection. Cache hits on repeated PRs are not expected.
3. Exact-tree invalidation after source/vendor/lockfile/build-input changes, and
   an intentional analysis failure that stays failed even if prior cache entries
   exist. Confirm no failed/PR run saved reusable main state.
4. Compare file identities, the four warning files and platform-limited INFO
   paths against the actual changed source. Confirm query suites, local threat
   configuration, JavaScript/Python overlay behavior and PR diff ranges.
5. Compare successful same-repository and approved external-fork PR scans, then a
   full main scan. Include all required Quality and CodeQL checks in the final
   creation-to-processed-result critical path; keep canceled/failing runs separate.

C2 remains open until the coordinator records these actual observations and any
unresolved coverage/latency limits. The first cache experiment may be rejected
if transfer cost exceeds its benefit; retaining the diagnostics and native
prerequisites does not require claiming that cache design improved PR latency.

## Transition and rollback

**Current transition is advanced to optimized advanced.** Keep the existing
workflow name, job names, language categories and security policy. Push the
reviewed change through a PR and let all four automatic analyses upload/process.
Do not disable advanced scans while collecting manual upload-never measurements.
Only the coordinator integrates the source and changes account settings.

If a source defect prevents required scans, revert this optimization as an atomic
commit, restoring the `050be11` advanced workflow and rerun the same PR/main
contexts. Preserve failure logs and alert state. Do not turn off security rules,
raise thresholds or treat a manual rehearsal as a successful replacement scan.

**If default setup is restored in the future**, GitHub
[rejects advanced uploads while it is enabled](https://docs.github.com/en/code-security/reference/code-scanning/sarif-files/troubleshoot-sarif-uploads/default-setup-enabled).
Use the public-launch procedure with an explicit coordinator-controlled merge
hold during the switch, preserving the CodeQL ruleset throughout. Save the exact
prior default-setup, workflow-enabled and branch-protection states first. A
repository owner's temporary protected-branch lock is one enforceable hold;
record its prior value and restore only after verification. Do not assume two
separate settings API calls are atomic.

Rehearse source/configuration with upload `never` while default coverage remains
active. Then, under the merge hold, switch default setup to `not-configured`,
ensure this workflow is enabled, and rerun genuine PR events plus a full main
scan with uploads. Verify all four processed categories/findings and approved
fork behavior before lifting the hold. If switching fails, keep the hold, disable
the advanced workflow, restore the saved default setup (default suites, all four
effective languages, remote-and-local threat policy and weekly schedule), and
verify a genuine default scan before restoring the previous branch-lock state.
Default setup's fork limitation remains an explicit rollback limitation; a
rollback does not silently satisfy promised fork coverage.
