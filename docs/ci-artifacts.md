# CI package downloads

Quality builds one optimized executable per declared native target:
`aarch64-apple-darwin` on standard ARM64 macOS 26 and `x86_64-unknown-linux-gnu` on Ubuntu 24.04.
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

Package creation, local archive checks, upload, download and installed-package
checks precede release cache cleanup. Cache accounting covers the entire target
root, including the explicit target subdirectory. The stable Quality gate requires
both release matrix jobs. Product, package, native asset, workflow and
`scripts/ci/packages.py` changes run those jobs; documentation-only PR routing remains
inexpensive. Every main push and manual run requires full coverage, including
documentation-, website- and tooling-only changes, to verify packages and seed
the trusted caches.

## License boundary and availability

Every run executes target-specific license collection with `--require-complete`.
Only exit 0 with a matching target/lock inventory and no missing notices permits a
public binary upload. The current documented C0 gaps are `mac-0.1.1` and
`ufbx-0.11.3` on Linux; macOS additionally has `block-0.1.6`,
`objc_exception-0.1.2`, `leak-0.1.2` and `leaky-cow-0.1.1`.

While these known gaps remain, exit 2 is checked against that exact inventory,
a development package is created and verified locally, and the Actions summary
says **Download unavailable; C0 incomplete and C3 open**. The archive is withheld;
it is never included in the diagnostics artifact. A newly missing notice, malformed
inventory or any other collector failure fails the package job. This allowance is
not license clearance and does not mark hosted transfer or the initiative complete.
When authoritative notices are supplied, the same code takes the strict
complete-notice path automatically.

## Find and verify a download

After C0 is cleared, successful eligible jobs upload a seven-day Actions artifact
named with version, source SHA, platform, release profile, event trust label, run
ID and attempt. It contains exactly:

- The Linux `.tar.gz` or macOS `.zip` package.
- The archive's `.sha256` checksum and `.manifest.json` provenance sidecar.
- `ci-provenance.json`, tying the package to this run and its local validation.

The Linux archive includes the per-user installer, complete notices, executable
and format-2 package identity. macOS includes `GitTurtle.app` and a detached
`build-info.json` outside the app's signature seal. Linux binaries are unsigned;
macOS CI apps are ad-hoc signed and **not notarized**. These are early-development
builds, not published releases or a promise of production support.

The workflow downloads the exact artifact ID returned by its own upload step;
it does not search by branch or latest artifact name. Download digest mismatches
fail. The downloaded archive and sidecar digests must also match the local
pre-upload record. The verifier checks the detached checksum, inner/outer package
identity, source/version/target/profile, original executable digest, Cargo.lock,
all inventoried notice bytes and the actual extracted executable's `--build-info`.

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
`package-transfer.json` in the separate three-day diagnostics artifact. The
prepared identity and C0 state are in `package-prepared.json`; commands also use
existing sanitized CI measurement logs. An upload, download, checksum or package
check failure fails the required release lane. A no-display check is not a native
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

Action pins were verified against the official repositories on September 15, 2026:
[upload-artifact v7.0.1](https://github.com/actions/upload-artifact/releases/tag/v7.0.1)
and [download-artifact v8.0.1](https://github.com/actions/download-artifact/releases/tag/v8.0.1).
The download is bound to the current run's returned artifact ID, and its
`digest-mismatch: error` input is explicit.

## Evidence still required

Source tests and workflow lint establish the helper's refusal and routing behavior.
They do not establish a successful Actions upload/download, a complete license
inventory, or native desktop behavior. Before closing C3, record the actual run and
artifact IDs, archive hashes, source commit, OS/architecture, signing state, transfer
report and native installation/session evidence. macOS native verification is
currently assigned separately to the project owner.
