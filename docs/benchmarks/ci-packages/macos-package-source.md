# macOS package source contract

The macOS packager prepares **Apple Silicon (`aarch64-apple-darwin`)** local
ad-hoc bundles and optional ZIPs. Source fixtures below do not establish a Mac
package pass. Real candidate-bound packaging, native appearances, archive launch
and Apple-service verification remain open; the user owns that macOS follow-up.

## Prerequisites and commands

Use Python 3.11+, the locked Rust toolchain, an Apple Silicon Mac and full
**Xcode 26 or later** selected with `xcode-select`. The selected toolchain must
provide `actool`, `assetutil` and a working `metal --version`; Command Line Tools
alone are insufficient. Install the compatible Metal component through Xcode's
Components settings or `xcodebuild -downloadComponent MetalToolchain` when absent.
The packager checks and records selected Xcode, Metal and actool information;
it never installs tools or changes the selected Xcode. See Apple's
[component installation](https://developer.apple.com/documentation/xcode/downloading-and-installing-additional-xcode-components)
and [Icon Composer](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer)
guidance.

Choose a new local output until it is verified. The positional `.app` path and
`--debug` remain supported. Build commands force the target and package exactly
`target/aarch64-apple-darwin/{release,debug}/gitturtle`, independently of a default
Cargo target in the environment/configuration:

```sh
./scripts/package-macos.sh /absolute/path/to/new/GitTurtle.app
./scripts/package-macos.sh --debug /absolute/path/to/new/Debug.app

# Reuse an already reviewed, trusted executable, including older target layouts.
./scripts/package-macos.sh --no-build \
  --binary /absolute/path/to/intended/gitturtle \
  --expected-revision FULL_SOURCE_COMMIT \
  --expected-version 0.1.0 \
  --expected-sha256 PRE_SIGN_EXECUTABLE_SHA256 \
  --archive-dir /absolute/path/to/new/archive-directory \
  /absolute/path/to/new/GitTurtle.app
```

The SHA-256 expectation describes the executable **before** ad-hoc signing.
`--no-build` skips only Cargo compilation; icons, notices, identity, signature
and optional ZIP checks still run. Every mode verifies the compiled revision,
version, profile, target, Mach-O architecture, timestamp and Rust identity.
Local modified builds require an explicitly captured executable hash when reused;
that identifies the reviewed bytes, rather than proving which uncommitted source
produced them. A CI distribution requires clean compiled and packaging trees and
an exact clean-source build. Prerelease Cargo versions are rejected because a
numeric `CFBundleVersion`/short-version mapping has not been selected.

For distribution preparation add `--distribution --archive-dir DIRECTORY`.
This requires a release build and the existing notice collector's
`--require-complete` check. It **still creates an ad-hoc package**, never a
Developer ID signature or notarization. Public uploads remain subject to C0 and
the release workflow's separate approval and signing requirements.

## Resources, notices and provenance

The packager compiles the existing `assets/AppIcon.icon` on every invocation.
It requires nonempty `Assets.car`/`AppIcon.icns`, parses the generated icon metadata
and validates `CFBundleIconName`/`CFBundleIconFile` without allowing generated
metadata to override identity or execution keys. The package retains the decoded
`assetutil --info` catalog in its detached manifest. Native appearance rendering
still needs inspection on macOS; a nonempty catalog is not a visual pass.
Embedded control/branding artwork still requires an executable rebuild.

License collection uses the verified arm64 target and locked dependency inventory.
Local development preserves complete collected files plus any
`REVIEW_REQUIRED.md`; a strict collection failure happens before replacing an
existing bundle. The current six macOS notice gaps remain unresolved unless C0
has authoritative, version-matched supplemental texts. No notice text is invented
or omitted to make the strict path pass.

The staged `.app` is complete before signing. It contains one executable and no
nested application/framework code. `codesign --force --sign -` seals it once;
`codesign --verify --deep --strict` and displayed `Signature=adhoc` must succeed.
Build identity is checked again after signing. No generated metadata is then
written inside its signature seal.

Each `.app` has a sibling `GitTurtle.app.build-info.json` (using the selected app
name), in shared package manifest **format 2**. It records source, version,
profile/target, Cargo.lock/license-inventory hashes, notices, development versus
complete-notices mode, original `input_binary_sha256`, final `binary_sha256`,
actual ad-hoc status and tool/resource diagnostics. `notarization` remains
`not-performed`. The manifest is detached because changing the signed executable
hash inside sealed resources would create a circular dependency.

With `--archive-dir`, names follow:

```text
GitTurtle-{version}-{source_sha12}-macos-arm64-{profile}.zip
GitTurtle-{version}-{source_sha12}-macos-arm64-{profile}.zip.sha256
GitTurtle-{version}-{source_sha12}-macos-arm64-{profile}.zip.manifest.json
```

The ZIP contains `GitTurtle.app` and detached `build-info.json` at its root.
The outer manifest uses shared archive manifest **format 1**, including the ZIP
size/hash and detached manifest hash plus package metadata. The checksum sidecar
uses SHA-256's standard `digest  filename` format. Names and manifest semantics
are deterministic; byte-identical archives are not promised. `ditto` preserves
Mac packaging data, and the packager validates paths/CRC, extracts privately,
compares all app bytes/modes plus manifest, validates identity and verifies the
extracted signature before publishing local outputs. See Apple's
[distribution packaging](https://developer.apple.com/documentation/xcode/packaging-mac-software-for-distribution).

## Existing outputs and recovery

An existing conventional GitTurtle `.app` can be replaced after all checks pass.
Existing unrelated or malformed bundles, symlink payloads, malformed sidecar
identity, or any existing archive/checksum/manifest output are refused. A legacy
matching app without a sidecar remains a supported local input. Existing
archive outputs are never overwritten, including dangling symlinks.

A private sibling stage and output lock serialize invocations. Immediately before
publication the packager rechecks prior output fingerprints, writes a recovery
plan, then moves prior app/sidecar to backups and validates the captured bytes before
publishing prepared files. It checks captured backups again before discarding
them, restoring or retaining concurrent changes with diagnostics on a mismatch.
Exclusive renames and hard links refuse destinations that appear concurrently;
ordinary publication failure restores prior outputs. If another process changes
a published path, recovery retains backups instead of deleting that new content.
The selected volume must support exclusive renames/hard links; unsupported volumes
fail clearly. Apple's [exclusive rename capability](https://developer.apple.com/documentation/foundation/urlresourcevalues/volumesupportsexclusiverenaming)
defines the macOS filesystem requirement.

A failed attempt keeps `.gitturtle-package-*`/`.gitturtle-archive-*` diagnostic
staging outside the final outputs. `failure.txt` identifies incomplete work;
`publication.json` maps intended destinations and prior backup hashes when
publication began. Abrupt process/host termination can leave the lock and a
partially published output set: inspect that plan and verify/restore prior outputs
before removing a stale `.APPNAME.package-lock`. The multi-output change is not a
single filesystem transaction. An ordinary failed preflight does not replace a
working app. The script never launches the app, modifies application preferences,
or installs over `/Applications` unless that output was explicitly selected.

## Focused source validation and remaining native evidence

`python3 scripts/test-package-macos.py` exercises the actual orchestrator in
private disposable Git/filesystem fixtures, replacing only Apple/build/collector
operations and executable probes with explicitly simulated responses. Coverage
includes stale/missing/wrong inputs; modified and strict-mode refusals; local
debug/legacy replacement; missing resources and tool failures; strict notice
failure; signing/hash order; archive extraction mismatch; symlink/output
collisions; cancellation; concurrent changes; exclusive destination refusal and
rollback. Shared package-identity fixtures test the real bounded executable probe
and manifest validation separately.

These fixtures must not be called a successful macOS package or native run.
On the final clean candidate, the macOS owner still needs to:

1. Build/package with the selected Xcode/Metal versions, record exact source,
   target/profile and pre-/post-sign executable hashes, and verify plist/resources.
2. Retain real signature output and catalog inspection; check default, dark,
   clear/tinted icon appearances in the app/Finder/Dock where available.
3. Verify strict notice refusal while C0 is open. Do not upload development ZIPs
   as public distribution artifacts.
4. Extract the actual ZIP, independently verify outer and executable hashes,
   and launch that exact extracted app against a disposable repository. Exercise
   About/build diagnostics, core navigation and embedded/bundled resources.
5. Record OS/architecture/session, artifact hashes and limitations. Developer ID,
   notarization, quarantine/Gatekeeper and another-machine acceptance remain C4.
