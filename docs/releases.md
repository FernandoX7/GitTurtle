# Release preparation

## Tagged release workflow

**Status: source preparation and local simulated tests.** No release, protected
environment, signing credential or published download is established by these
files. The coordinator's [C4 checkpoint](development/commit-inspector-and-ci.md#c4--release-rehearsal-and-macos-distribution)
remains open. The user will verify macOS separately. Complete C0 license notices
are mandatory for every public binary, including temporary Actions artifacts.

[`release.yml`](../.github/workflows/release.yml) is an explicitly dispatched
workflow. Ordinary PR, main and tag pushes cannot publish. It supports exactly
`x86_64-unknown-linux-gnu` and `aarch64-apple-darwin`; choose `linux`, `macos` or
`linux,macos`. Linux-only runs allocate no macOS runner or Apple credentials.

The release owner chooses the existing tag, exact tag object, workspace version,
full source commit, supported platform set and successful full Quality run.
The chosen source must be the current reviewed `main` revision and the workflow
must execute from that same revision. Older branches, PR artifacts, moved tags
and mixed-source packages fail. This initial workflow intentionally does not
release an older maintenance commit; that needs a reviewed extension of the
trusted-ref policy. It never creates or moves a tag.

### Source, package and privilege boundaries

1. **Context preflight** uses the existing read-only
   [`identity.py`](../scripts/release/identity.py) to verify the exact tag object,
   peeled commit, workspace/member versions, lockfile and actual tracked source
   bytes. It also checks the remote tag and current main revision. The declared
   Quality run must be a successful `push` or `workflow_dispatch` run from this
   repository at that source. Actual successful jobs must include formatting,
   both platform test/Clippy jobs, both platform release jobs and the Quality
   gate. Skipped product validation is insufficient. A changed Quality attempt
   requires a newly reviewed context.
2. **Read-only build jobs** compile the selected explicit targets from clean
   source. They use the pinned compiler and existing dependency cache, with cache
   finish after package consumers. macOS jobs select the highest installed full
   Xcode 26+ before computing cache identity and check its Metal/package tools;
   signing selects the same supported toolchain family on its own runner. Missing
   supported tools fail explicitly. The existing packagers require clean release
   identity and complete target-specific notices. Each extracted archive receives
   the platform package checks before upload. License gaps fail before upload;
   development packages cannot become release inputs.
3. **Optional signing** consumes only the macOS package created by this workflow
   run and attempt. Archive and manifest digests, source, version, target, original
   executable digest, full notices and the ad-hoc signature are checked before
   the credential-bearing step. The constant `gitturtle-macos-signing`
   environment must authorize the precise context. The signing helper handles
   credentials and cleanup; no signing step builds Rust or runs PR code.
4. **Assembly** downloads same-run packages with artifact digest mismatch set to
   `error`, then independently checks package bytes and every inventory notice
   digest. Missing or extra promised platforms fail. Cross-platform assembly
   parses and hashes archives; it does not execute downloaded binaries. A signed
   Mac package additionally needs the accepted signing report, selected
   certificate/Team ID, final executable digest and final archive digest.
5. **Publication** is a separate Ubuntu job with `contents: write`, selected only
   by an explicit `publish: true`. The constant `gitturtle-release` environment
   must authorize the context after the exact assembled assets are reviewable.
   It verifies the assembly digest from the producer job, all local asset bytes,
   remote tag and unchanged successful Quality evidence before any release write.
   It rechecks the tag around draft creation and before/after publication; C4
   must additionally establish the protected-tag boundary described below.
   No PR-generated binary can enter this path.

The workflow uses immutable action commits. GitHub's
[manual workflow trigger](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch)
and [environments](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments)
provide the execution and approval boundaries. An environment name in YAML does
**not** configure protection. C4 requires the account owner to configure reviewer
protection, prevent self-approval where supported, restrict deployment to `main`
and review who can change the environment. Secrets must exist only in the
signing environment. Neither source tests nor a dispatch attempt certifies these
live settings.

### Concrete C4 activation and rehearsal

Before dispatch, retain a reviewed input record containing all workflow inputs:
`tag`, `version`, `commit`, `tag_object`, `quality_run`, `platforms`,
`macos_signing`, `certificate`, `team_id` and `publish`. The certificate is the
exact Developer ID Application SHA-1 fingerprint; the Team ID is also explicit.
Both must be empty for ad-hoc or Linux-only runs. Choose `publish: false` for
assembly rehearsal; this is the default. No example here invents a release tag
or authorizes publication.

1. Complete C0 for all chosen targets, refresh the candidate and collect required
   native/package evidence on that exact build. Ensure the release workflow and
   package source have been reviewed and integrated into `main`.
2. The release owner explicitly chooses and approves the version/tag/source,
   platform and signing set. Establish the existing tag through that separate
   authorized operation and capture its object SHA; this workflow will not
   manufacture one. Retain a successful full Quality run for the exact commit.
3. Configure the protected environments above, leaving activation variables
   unset initially. For signing, provision the credentials in the component
   section below. Protect the selected release tag against deletion and force
   updates and exclude concurrent tag writers throughout publication. GitHub's
   release-creation API has no atomic “tag still equals this object” condition
   and can create a missing tag; local preflight cannot enforce remote tag
   immutability on its own. Do not make Linux-only operation depend on the
   signing environment.
4. Dispatch the reviewed input record with publication disabled:

   ```sh
   gh workflow run release.yml --ref main --json < reviewed-release-inputs.json
   ```

   The input file is the owner's concrete record, not a repository default. The
   preflight prints a context SHA-256 binding source, tag object, version,
   platforms, signing identity and exact Quality evidence. For a notarized
   rehearsal, set environment variable `GITTURTLE_SIGNING_CONTEXT` to that exact
   digest only after reviewing the context, then approve the waiting signing job.
   An absent or different activation value fails instead of falling back.
5. Download the `release-assembly-RUN-ATTEMPT` artifact. Keep its run URL, exact
   manifest SHA-256, asset hashes, notices and proposed `RELEASE_NOTES.md`.
   Hard-verify checksums, extract into disposable locations and follow the native
   package procedure. For macOS, establish actual Apple acceptance and the
   quarantined/offline Gatekeeper launch flow described below. These observations
   cannot be replaced by the source fixtures or a command-line signature check.
6. Rehearse collision/partial-failure handling with the local tests. Any live
   draft rehearsal that creates GitHub release objects needs its own approved
   context; there is no hidden live test publication. If actual public
   publication is selected, dispatch with `publish: true` and review that run's
   exact assembly before approving its queued publication job. The fresh build
   may have different bytes from an earlier rehearsal; verify the current hashes.
   Set environment variable `GITTURTLE_RELEASE_CONTEXT` to the reviewed context
   digest only for that explicitly authorized release.
7. After publication, download the actual release assets into fresh locations,
   verify hashes and package identity again, and retain the applicable install
   and native observations. Only then update user-facing download links. Clear
   both activation variables when the approved operation is finished.

A rehearsal is useful evidence, not authorization to publish a later arbitrary
version. The context digest deliberately excludes workflow run IDs so the same
chosen release can be rehearsed again, but receipts include the exact run and
attempt. Rerunning only failed jobs with an earlier context is refused; use a
new complete reviewed run. Build timestamps/signing may change asset bytes, so a
fresh run never establishes an identical publication retry merely by sharing a
version number.

### Release contents and signing provenance

Assembly contains the selected `.tar.gz` and/or `.zip` files, each detached
`.sha256` and `.manifest.json`, `SHA256SUMS`, `RELEASE_NOTES.md` and
`release-manifest.json`. The release manifest binds the context, platform/signing
labels and all other asset hashes. The manifest's own hash is recorded in the
workflow summary and publication report. Each archive contains complete license
notices and its format-2 package identity. macOS keeps `build-info.json` outside
the app seal. `SHA256SUMS` covers the downloadable payloads and notes; its own
hash and the other asset hashes are in the release manifest.

Ad-hoc macOS output is labelled `ad-hoc`; it is not Developer ID signed or
notarized. A selected notarized path never silently falls back. After the signing
helper completes credential cleanup, the wrapper preserves its signed ZIP entries
and app metadata, adds only detached package provenance, then computes the final
archive digest. A bounded extraction establishes ordinary entries; `ditto`
restores platform metadata into a second private tree. Actual app bytes and
executable modes are compared before strict signature and staple validation.

The detached `macos-signing-result.json` preserves the helper's original
`final_archive` as the stapled app ZIP **before** detached package provenance,
and adds `release_archive` for the exact distributed ZIP. It separately records
pre-sign, signed and post-staple executable hashes. The final package manifest
uses `developer-id-notarized` and the actual final executable digest; source,
version, target, original build input and complete notice checks still apply.
No already signed app file is edited to insert provenance.

### Publication retries and uncertain outcomes

[`github.py`](../scripts/release/github.py) creates one draft, uploads each
asset once, downloads and hashes the uploaded bytes, rechecks the complete asset
set and tag, then publishes once. It leaves `make_latest` false; promoting a
release to latest is a separate owner decision. The implementation follows the
[GitHub releases API](https://docs.github.com/en/rest/releases/releases) and
[release assets API](https://docs.github.com/en/rest/releases/assets). It does
not request broader workflow-write credentials; an API refusal remains a
visible unresolved operation.

Before each write, the private publication report records the potentially
uncertain operation. On an error, cancellation or lost reply, preserve that
report and inspect GitHub before retrying. An existing complete published release
is accepted only when identity, exact description, complete asset names/sizes
and downloaded hashes all match. A different asset, changed description,
extra/missing asset, existing draft or partial release stops for inspection.
There is no overwrite, deletion, tag movement or blind resume. If an authorized
owner decides to recover a partial draft, the owner must first inspect its
recorded ID and exact uploaded bytes; this initial workflow does not automate
that recovery.

A local retry reuses neither old signing credentials nor an unknown notarization
submission. Credential and service ambiguity follows the signing recovery
procedure below. Action artifacts expire after three days; retain the approved
release record outside transient run storage before relying on it.

### Source validation for assembly and publication

```sh
python3 -m py_compile scripts/release/identity.py scripts/release/github.py scripts/release/workflow.py
python3 scripts/release/workflow.py --help
python3 -m unittest discover -s scripts/ci/tests -p 'test_release*.py'
python3 -m unittest discover -s scripts/ci/tests -p 'test_sign_macos.py'
actionlint -shellcheck='' -pyflakes='' .github/workflows/release.yml
```

Fixtures exercise real local archive/hash/manifest validation with fabricated
binary headers, an in-memory GitHub service and explicitly simulated Apple
commands. They cover exact-source checks, selected platforms, complete notices,
unsafe paths, digest failures, changed certificates, final signing transforms,
collisions and uncertain publication. They do not establish hosted workflow
success, downloadable assets, Apple notarization or native launch. Primary
GitHub references were checked on 2026-09-15; hosted C4 rehearsal remains required.


## macOS Developer ID signing component

**Status:** source preparation only. The local tests simulate Apple tools and
service responses. They do not establish Developer ID signing, notarization,
Gatekeeper acceptance, a downloaded package, or native launch. The initiative's
[C4 checkpoint](development/commit-inspector-and-ci.md#c4--release-rehearsal-and-macos-distribution)
remains open; macOS verification is assigned to the user.

[`scripts/release/sign-macos.py`](../scripts/release/sign-macos.py) owns the
credential lifetime and the transformation of one trusted app into a signed,
notarized and stapled ZIP. The existing local
[`package-macos.sh`](../scripts/package-macos.sh) remains the ad-hoc development
path. The signing helper does not build, fetch artifacts, change tags, upload
GitHub assets or publish a release.

### Integration prerequisites

The outer release workflow must establish all of these **before** it provides
credentials or invokes the helper:

- An explicitly approved release context: exact source commit, version, platform
  set and artifact list; reviewed environment protection and trusted workflow/ref.
  A YAML environment name alone does not configure reviewer protection.
- Successful required checks on that exact source and a trusted package run.
  PR-generated artifacts and arbitrary local app paths are not signing inputs.
- Hard-failing verification of the downloaded input archive digest, package
  manifest, executable digest, notices/inventory and source/version/target.
  Complete target-specific license clearance at C0 is required before sending a
  distribution package to Apple. The helper does not duplicate the package
  manifest or certify its notices.
- Safe extraction into a private, immutable input directory. The helper executes
  the captured executable's `--build-info`; a matching caller-supplied digest is
  meaningful only after the caller establishes artifact trust.
- A dedicated, disposable macOS runner user, one signing process at a time,
  selected Xcode 26 or later, and sufficient job time for upload, the bounded
  notarization wait and cleanup. Do not run this secret-bearing path on PRs,
  untrusted refs, shared desktop sessions or persistent shared runner users.

The currently declared macOS distribution target is
`aarch64-apple-darwin`. Intel and universal delivery require a separately reviewed
target/package extension and their own evidence. The helper accepts a numeric
`major.minor.patch` version and requires the same value in both bundle version
fields. It does not silently map Cargo prerelease versions to Apple build
numbers. See Apple's [version-number definition](https://developer.apple.com/help/glossary/version-number/)
and [CFBundleVersion reference](https://developer.apple.com/documentation/BundleResources/Information-Property-List/CFBundleVersion).

### Credentials and cleanup

Supply these environment variables only to the protected signing step; do not
write them to a job summary, shell trace, cache, output manifest or artifact:

- `GITTURTLE_SIGNING_P12_BASE64`: base64-encoded PKCS#12 import bundle containing
  exactly the chosen Developer ID Application identity and private key.
- `GITTURTLE_SIGNING_P12_PASSWORD`: its nonempty import password.
- `GITTURTLE_NOTARY_KEY_BASE64`: base64-encoded App Store Connect **team** API
  private key in PKCS#8 `.p8` form.
- `GITTURTLE_NOTARY_KEY_ID` and `GITTURTLE_NOTARY_ISSUER_ID`: the corresponding key
  ID and issuer UUID. Individual API keys are not this authentication route.

Apple documents the `--key`, `--key-id` and `--issuer` authentication route in
[TN3147](https://developer.apple.com/documentation/technotes/tn3147-migrating-to-the-latest-notarization-tool).
The account owner selects the narrow appropriate API-key role and configures it
through the protected environment; no credentials belong in this repository.
[Apple's API-key guidance](https://developer.apple.com/documentation/appstoreconnectapi/creating-api-keys-for-app-store-connect-api)
distinguishes team and individual keys.

The helper uses a private temporary directory, a randomly passworded keychain,
explicit codesign keychain selection and a narrow subprocess environment. It
does not forward signing environment variables to child tools or print command
arguments/output. The `security` import/partition commands necessarily receive
passwords as arguments, so the dedicated disposable-user boundary matters.
Only the intended public certificate fingerprint, Team ID, signature properties,
submission ID and sanitized status/count fields enter the report.

Keychain search-list state is captured before creation and restored afterward.
Restoration and deletion each have a 20-second command limit and both are
attempted even if one fails. Normal success, exceptions, SIGINT and SIGTERM
remove the temporary keychain, decoded import bundle and API-key file. Cleanup
failure withholds the final artifact. A second ordinary termination signal is
ignored during those bounded keychain cleanup commands. SIGKILL, power loss and
runner destruction cannot run Python cleanup; the workflow must use a disposable
runner and an `always()` cleanup step for its own secret files. Do not upload a
workspace or runner temporary-directory wildcard.

### Helper interface and output

Invoke only after those prerequisites have been checked. All path arguments
must be absolute. Expected identity values come from the validated release and
package, not values newly trusted from the input app:

```sh
python3 scripts/release/sign-macos.py \
  --app "$TRUSTED_EXTRACTED_APP" \
  --output "$NEW_SIGNING_OUTPUT" \
  --source "$RELEASE_SOURCE_SHA" \
  --version "$RELEASE_VERSION" \
  --target aarch64-apple-darwin \
  --executable-sha256 "$EXPECTED_INPUT_EXECUTABLE_SHA256" \
  --identity "$EXPECTED_CERTIFICATE_SHA1" \
  --team-id "$EXPECTED_TEAM_ID" \
  --notary-timeout-seconds 1800
```

`--identity` is the exact 40-hex certificate fingerprint, not an ambiguous
certificate name. The imported keychain must expose exactly one valid signing
identity with that fingerprint and the expected Developer ID Application Team
ID. The input app stays unchanged. Existing outputs, including dangling
symlinks, are refused. The helper copies into a private workspace, compares
every input file's bytes/mode with the copy and checks compiled clean source,
version, release profile, physical Mach-O architecture, bundle identifier and
version fields. It rejects symlinks, special files, additional executable
resources, hidden Mach-O files and nested code bundles. Future helper executables
or frameworks need an explicit signing plan before this boundary is extended.

The current bundle has one main executable and no nested code. `codesign` signs
the app bundle to seal that executable and its resources, with a secure
timestamp, hardened runtime and an empty entitlement dictionary. There is no
`--deep` signing, inherited debugging entitlement, speculative JIT allowance or
disabled library validation. Strict deep verification is a separate operation.
If future code adds nested components, sign them from the inside out with their
own reviewed options before sealing the app, as described in Apple's
[distribution-signing guide](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/).
The empty entitlement set requires actual GPUI/native verification; add an
exception only for a reproduced requirement. Apple's
[Hardened Runtime reference](https://developer.apple.com/documentation/security/hardened-runtime)
and [notarization diagnostics](https://developer.apple.com/documentation/security/resolving-common-notarization-issues)
describe those constraints.

The signing helper performs these state transitions:

1. Verify the private copy and sign it; verify signature, timestamp, runtime,
   entitlements and unchanged compiled identity.
2. Archive that signed app with `ditto`, hash the exact submission, submit once,
   and persist the submission ID before waiting.
3. Wait for at most the configured 60–3600 seconds; require a matching terminal
   result and an accepted log bound to the submission ID and archive SHA-256.
   Retrieve a rejection log when the tool supplies a parsable terminal response,
   including on a nonzero wait exit. Never retry submission automatically.
4. Staple the ticket onto the app, validate the staple and strict signature, then
   recheck compiled identity and recreate the final ZIP. ZIP files themselves
   cannot be stapled. This ordering follows Apple's
   [custom notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).
5. Compute the final archive digest after stapling/repacking; clean credentials
   and keychain state before exposing the final artifact filename.

A successful helper directory contains only the final
`GitTurtle-VERSION-aarch64-apple-darwin-SOURCE12-notarized.zip`, `SHA256SUMS` and
`signing-result.json`. The result is a detached signing record, **not** a second
package manifest or release authorization. It distinguishes the pre-sign,
signed and post-staple executable hashes, submitted ZIP hash and final archive
name/size/hash, and records cleanup outcomes. Outer package/release provenance
must be updated from these final bytes without editing the already signed app.
The outer workflow must require `status: signed-notarized-stapled`, verify the
final file/digest and promised platform set, and keep publication separately
authorized. A filename containing `notarized` is insufficient.

### Failure and reviewable recovery

Failure exits nonzero and keeps a sanitized `signing-result.json` when the new
output directory was created. No ad-hoc fallback or publication occurs. A
submitted archive may remain as `notarization-input.zip`; it is a diagnostic
input, not the final distribution. Upload only explicitly approved sanitized
diagnostics; never upload the entire failure directory automatically.

The report retains command stage, bounded duration, exit code when available,
notary submission ID and an allowlist of notary status/issue counts/codes. It
omits free-form Apple messages and private paths. An authorized macOS owner can
retrieve the detailed log separately using the recorded ID. If a submission
reply is lost and no ID is available, inspect notary history for the captured
archive digest before deciding whether another submission is appropriate.

After a timeout, rejection, upload ambiguity or cleanup failure, preserve the
directory and investigate that exact submission. Do not rerun into the same
output path or blindly submit the source again. A reviewed recovery may use the
retained submitted archive and ID, revalidate all identity/digest boundaries,
then staple/repack if Apple accepted it. This initial helper intentionally does
not automate that recovery or reuse old credentials. A correction requiring new
input bytes is a new reviewed candidate and submission.

### Required real macOS evidence

Before a macOS release claim, record selected Xcode/notarytool/codesign versions,
OS/architecture, exact source, input/final archive hashes, signing Team ID,
certificate fingerprint, notary submission/log and cleanup outcome. Download
the actual uploaded final archive into a fresh location; hard-verify its checksum
and package/signing identities before extraction and launch.

On the extracted app, require strict signature verification and staple
validation. Test the actual quarantined download/install/launch flow on a clean
macOS machine or snapshot, including offline launch of the stapled app. Record
native resources and applicable GitTurtle smoke interactions, and preserve the
user's installed app/state. On macOS 14 and later, `syspolicy_check distribution`
is useful additional preflight; command-line assessment alone does not establish
the user flow. Follow [Apple DTS's distribution test procedure](https://developer.apple.com/forums/thread/130560)
and the [GitTurtle macOS package procedure](../.agents/skills/gitturtle-native-qa/references/macos-package.md).
Real signature/Gatekeeper/launch evidence remains pending until the user supplies
it; a successful mocked service response cannot close C4.

### Source validation

```sh
python3 -m py_compile scripts/release/sign-macos.py
python3 scripts/release/sign-macos.py --help
python3 -m unittest discover -s scripts/ci/tests -p 'test_sign_macos.py'
```

The tests use explicitly simulated Mach-O bytes, macOS command responses,
notary submissions and tickets. They check ordinary failure/cancellation cleanup,
source and output preservation, identity and architecture refusals, final-archive
hashing order and log credential isolation. Separate local subprocess fixtures
exercise output/time bounds and environment stripping using Python processes.
They make no Apple-service, package-installation or native-GPUI claim. Primary
Apple references above were rechecked on 2026-09-15; verify the actual runner's
tool help and real response formats during macOS integration.
