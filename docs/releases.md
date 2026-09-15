# Release preparation

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
