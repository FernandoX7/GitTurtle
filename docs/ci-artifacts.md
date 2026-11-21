# CI package downloads

Quality builds one optimized executable per declared native target, each in its
own leg of the `rust-release` matrix and on a runner of its own architecture:

| Job | Runner | Target | Package label |
| --- | --- | --- | --- |
| `Rust release · macos-26` | standard ARM64 macOS 26 | `aarch64-apple-darwin` | `macos-arm64` |
| `Rust release · ubuntu-24.04` | Ubuntu 24.04, x86-64 | `x86_64-unknown-linux-gnu` | `linux-x86_64` |
| `Rust release · ubuntu-24.04-arm` | Ubuntu 24.04, ARM64 | `aarch64-unknown-linux-gnu` | `linux-aarch64` |

The two Linux legs run the same steps: the pinned toolchain and their own cache
entry (the key takes the compiler's host triple and the installed packages with
their architecture), the build, strict notice collection, the `--distribution`
package, the local checks, the upload and the hand-off of what was uploaded to
the later [`Package transfer` job](#find-and-verify-a-download), which downloads
and verifies it. The three legs may run at once.
The same job passes `target/<target>/release/gitturtle`, its SHA-256, source commit
and version to the platform packager with `--no-build`. No second release build
or implicit target-path lookup is used. CI selects and verifies installed Xcode
26.3 build 17C529 before computing cache identity; it never falls back to another
toolchain. The packager still accepts full Xcode 26+ for local use and checks Metal
and icon tooling. Missing prerequisites fail.

macOS 15 retains workspace tests, doctests, strict Clippy and tooling checks.
Only optimized compilation/packaging moved after the bounded
[icon compiler experiment](benchmarks/2026-09-15-ci.md#icon-compiler-environment).
That experiment did not build an app package or verify older-OS compatibility.
Full hosted package validation on the new runner remains pending; the existing
deployment target, artwork and package acceptance requirements are unchanged.

Package creation, the local archive and installed-package checks and the upload
precede release cache cleanup. The download and the checks of the downloaded
package run in the later job, which restores and saves no cache. Cache accounting
covers the entire target root, including the explicit target subdirectory. The
stable Quality gate requires all three release matrix jobs and both package
transfer jobs, and for each Linux package the pair of what the two said. Product,
package, native asset, workflow and
`scripts/ci/packages.py` changes run those jobs; documentation-only PR routing remains
inexpensive. Every main push and manual run requires full coverage, including
documentation-, website- and tooling-only changes, to verify packages and seed
the trusted caches.

## License boundary and availability

Every run executes target-specific license collection with `--require-complete`.
Only exit 0 with a matching target/lock inventory and no missing notices lets a
job take the complete-notice path. Linux has no documented C0 gap since the
[license review](licenses/linux-notice-review.md) of November 15, 2026, so its
jobs, x86-64 and AArch64, take that path and any missing notice there fails them.
The documented C0 gaps are on macOS: `block-0.1.6`, `objc_exception-0.1.2`,
`leak-0.1.2` and `leaky-cow-0.1.1`.

Complete notices alone do not upload a package: its platform must also be
listed in `HOSTED_UPLOAD_PLATFORMS` in `scripts/ci/packages.py`. The project
owner decided on November 15, 2026 that the hosted upload is on for the Linux
archives, so the constant lists Linux and nothing else. A Linux job with
complete notices collects strictly, packages with `--distribution` and verifies
the archive, the executable and the installed package inside the job. It then
writes `eligible=true`, records `upload_eligible: true` and fills its `publish/`
directory, and the workflow uploads that directory as the artifact described
[below](#find-and-verify-a-download); a later job of the same run downloads it
and verifies it. Every
main push, manual run and product pull request does this. The artifact is a CI
build, not a release: [GitHub Releases](releases.md) is the release channel.

macOS is not listed, so a macOS archive never leaves its job, whatever its
notices. With complete notices its job would still collect strictly, package
with `--distribution` and verify; it would then write `eligible=false`, record
`upload_eligible: false` and make no `publish/` directory, so the workflow's
upload and hand-off steps are skipped and no later job has a leg for it, and the
Actions summary would say
**Download unavailable: the macOS upload is held until macOS distribution is
decided; C3 hosted transfer remains open**. Listing macOS would publish an
ad-hoc signed, not notarized archive as the same seven-day artifact from the
same events. That entry waits for the macOS distribution decision, which is made
with signing, and is a reviewed change of its own. Nothing else turns an upload
on: the helper reads no flag, environment variable or workflow input for it, and
the verifier refuses a prepared record that is not upload-eligible.

The upload set covers this workflow's CI package only. The explicitly dispatched
[release workflow](releases.md) does not read it: its Linux build required
complete notices, which it now has. A dispatch on `main` with an existing tag
and a successful Quality run of that commit uploads its three-day build
artifacts, for `linux` one package for each Linux target (x86-64 and AArch64)
and their assembly; only its GitHub Release also waits for `publish` and the
`gitturtle-release` environment, which is not set up yet.

While the macOS known gaps remain, exit 2 is checked against that exact inventory,
a development package is created and verified locally, and the Actions summary
says **Download unavailable; C0 incomplete and C3 open**, and that the macOS
upload is separately held. The archive is withheld;
it is never included in the diagnostics artifact. A newly missing notice, malformed
inventory or any other collector failure fails the package job. This allowance is
not license clearance and does not mark hosted transfer or the initiative complete.
When authoritative notices are supplied, the same code takes the strict
complete-notice path automatically; the macOS upload stays held as above.

## Find and verify a download

Each successful Linux release job uploads a seven-day Actions artifact named
with version, source SHA, package label, release profile, event trust label, run
ID and attempt, so a run has two:
`GitTurtle-<version>-<source SHA, 12 characters>-linux-x86_64-release-ci-build-<run ID>-<attempt>`
and the same name with `linux-aarch64`. The trust label is `untrusted-pr` on a
pull request. Each job's step summary names its artifact, and the run's summary
page lists both under Artifacts for anyone who can read the repository's Actions
runs. No macOS job uploads a package. The upload comes before the later job
verifies the downloaded copy, so a run whose `Package transfer` job fails, is
cancelled or never starts can leave an artifact for the seven days. Only an
artifact whose `Package transfer` job is green has passed the download check; a
green `Rust release` job alone does not say so, and its step summary says that
too. Each artifact contains exactly:

- The Linux `.tar.gz` package.
- The archive's `.sha256` checksum and `.manifest.json` provenance sidecar.
- `ci-provenance.json`, tying the package to this run and its local validation.

The Linux archive includes the per-user installer, complete notices, executable
and format-2 package identity. The macOS `.zip`, which stays in its job, holds
`GitTurtle.app` and a detached `build-info.json` outside the app's signature
seal. Linux binaries are unsigned;
macOS CI apps are ad-hoc signed and **not notarized**. These are early-development
builds, not published releases or a promise of production support.

The download and its verification run in a later job, `Package transfer ·
ubuntu-24.04` and `Package transfer · ubuntu-24.04-arm`: one leg for each Linux
release leg, on the runner label that built the package. The job needs the
release job, so it starts once all three release legs have ended. It builds
nothing, restores no cache and reads no secret, and it has no `permissions`
block of its own: the workflow's `contents: read` applies.

Each Linux release leg hands the later job three job outputs under its own
package label: `<label>-eligible`, the SHA-256 of the `ci-provenance.json` it
uploaded, and the artifact ID its upload step returned. A transfer leg's first
step reads them and, before any action does, refuses everything but two
shapes: `true` with an artifact ID of one to fifteen digits without a leading
zero and a whole lowercase SHA-256, or `false` with neither. The limits on the
ID are the download action's: it reads its input with `parseInt`, which takes
`12abc` as 12 and rounds a number above 2^53, it splits the input at commas,
and with an empty input it downloads every artifact of the run. A refusal names
the field and prints no value. For `false` the leg records `held` and runs
nothing further but its summary and diagnostics steps.

The leg then downloads the artifact once, by that exact ID; it does not search
by branch, name or pattern, and a download digest mismatch fails. The verifier
hashes the downloaded `ci-provenance.json` before it parses it and requires
that hash to be the one the release leg handed on. It then requires the
record's target, source commit and run ID to be the job's own, and its run
attempt to be no later than the job's. Only then does it check the downloaded
archive and sidecar digests against that record, the detached checksum,
inner/outer package identity, source/version/target/profile, original
executable digest, Cargo.lock, all inventoried notice bytes and the actual
extracted executable's `--build-info`, and run the installation checks
described below. It records `verified` as its last act, after every check, its
report and its summary line; nothing else writes that word. The
[Quality gate](ci.md#stable-gate-and-compatibility) accepts a Linux package
only as `eligible` `true` with `verified`, or `false` with `held`.

**Why a later job, and one attempt.** Until November 2026 the release job
downloaded its own upload in the same job, up to three times with waits of
about 15 s and 45 s. The download action finds an ID by listing the run's
artifacts, and that listing can lack a new artifact for a moment: in the
x86-64 job of Quality run 43747610529 (November 15, 2026) it came back 0.45 s
after the upload was finalized, with seven artifacts and without the new ID,
and the step failed with `None of the provided artifact IDs were found`,
although the package had been built, verified and uploaded. A check that needs
a retry has a fault to find. A measurement run of November 21, 2026
(44841956480) showed the lag and where it ends: of 44 jobs that asked for
their artifact in the step after their upload, 19 did not find it, 0.20 to
0.48 s after the upload was finalized, and all 88 jobs that needed their
uploader found it listed at their one attempt, the earliest 4.85 s after. In
Quality the later job starts after every release leg has ended. So the
download is one step with no retry, no wait and no tolerated failure, and the
release job downloads nothing.

**What a red download is.** If that one download fails, the job fails and its
next step prints fixed words: the artifact was not returned intact at the one
attempt, or it is past its seven days of retention. Do not re-run the job to
turn it green. A miss in a job that needs its uploader would contradict the
measurement above, and the choice it raises is the project owner's: this later
job, or no read-back in Quality at all. Keep the run and take it to the owner.

**A re-run of the later job alone.** GitHub can re-run only the failed jobs of
a run. Such a re-run of a transfer leg is assumed, and not yet observed, to
read the outputs and the artifact of the release leg's earlier attempt. The
verifier therefore accepts a record from an attempt no later than its own, and
the report and the summary name both attempts. A re-run more than seven days
after the upload finds no artifact and fails as above.

Extraction permits only bounded regular files and directories in a new private
destination; traversal, symlinks, hard links, special/sparse files, duplicate paths,
case-insensitive collisions and oversized content fail. ZIP64, inconsistent directory
locations and entry-count mismatches are refused before Python constructs archive
entry objects. File ownership and special
permission bits are not taken from the archive. Linux then runs installer fixtures,
installs into a disposable home, checks the installed executable, desktop entry and
native libraries, and verifies the explicit no-display startup explanation. macOS
checks app identity, resources, plist and the extracted ad-hoc signature.

Successful downloaded-package verification is recorded in
`package-transfer.json` in the package transfer job's own three-day diagnostics
artifact (`ci-diagnostics-package-transfer-…`, one a leg), with the artifact ID,
the provenance SHA-256 that was required and both run attempts: the upload's
and the verifying one. Its `hosted_transfer` is `passed`; the uploaded
`ci-provenance.json` keeps `not-performed`, which was true when the release leg
wrote it. The prepared identity, C0 state and `upload_eligible` are in
`package-prepared.json` in the release job's diagnostics artifact; commands
also use existing sanitized CI measurement logs. An upload, checksum or package
check failure fails the required release lane. A malformed hand-off, a download
that fails at its one attempt or a failed check of the downloaded package fails
the required package transfer job, and the Quality gate with it. A no-display check is not a native
GUI test: actual installation and desktop interaction on each supported platform
remain separate C3 evidence.

## Trust and release separation

PR artifacts include `untrusted-pr` in their names. All CI provenance sets
`release_input: false`; main/manual CI packages are not a release input either.
This workflow has read-only repository permissions, no signing secrets, no
`pull_request_target` execution, and no release publication. Tagged releases must
build independently from the validated tag in their reviewed workflow.

The artifact helper's Python API supports structural verification without executing
a foreign-platform binary (`probe=False`) for trusted release orchestration. Its
optional manifest validator accepts the release workflow's stricter signed-package
contract; the CI CLI always uses the standard unsigned/ad-hoc validator and executes
the native identity probe. Neither option turns arbitrary CI outputs into trusted
release inputs.

Action pins were verified against the official repositories on November 15, 2026:
[upload-artifact v7.0.2](https://github.com/actions/upload-artifact/releases/tag/v7.0.2)
(`cf430e030ddbb5b0abf93d22962f4752f3646cd9`) and
[download-artifact v8.0.2](https://github.com/actions/download-artifact/releases/tag/v8.0.2)
(`9000827ccba6bdab643e8b6fd33ac0654aef8333`); each tag resolves to the commit
the workflows pin.
The one download is bound to the artifact ID that the release leg's own upload
step returned, validated before the action reads it, and its
`digest-mismatch: error` input is explicit.

## Evidence still required

Source tests and workflow lint establish the helper's refusal and routing behavior.
They do not establish a successful Actions upload/download, a complete license
inventory, or native desktop behavior. The Linux upload and the AArch64 Linux
leg are in source since November 15, 2026; no hosted run with either is recorded
here. The package transfer job is in source in place of the release job's
same-job download and its retries; no hosted run of it is recorded here, and
its first is the run of the pull request that adds it. The two runs named
[above](#find-and-verify-a-download) are the failure and the measurement that
led to it, not evidence for it. Still to be observed in a hosted run: a
transfer leg on the ARM64 runner, the two matrices' per-label outputs as the
later job and the gate read them, and a re-run of the later job alone. The
hosted Linux runners have no display, so neither Linux
leg is a desktop check of its build. Before closing C3, record the actual run and
artifact IDs, archive hashes, source commit, OS/architecture, signing state, transfer
report and native installation/session evidence. macOS native verification is
currently assigned separately to the project owner.
