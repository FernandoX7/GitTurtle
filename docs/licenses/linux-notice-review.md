# Linux notice review — September 14, 2026

The locked Linux dependency inventory had two unresolved notice records. The
`ufbx` record was resolved on October 25, 2026 and the `mac` record on
November 15, 2026, as their sections describe, so strict collection for the
Linux targets now completes.
This review establishes package provenance and records the missing information;
it does not assert that a public binary has completed license review. No upstream
message was sent during this investigation.

Completing the notices publishes no Linux binary. The owner decided on
November 15, 2026 that the notices land and the CI archive's upload stays off
until distribution (phase 4) decides, with signing, what is published.
`HOSTED_UPLOAD` in `scripts/ci/packages.py` is that hold: the Linux job collects
strictly, fails on any missing notice, builds and verifies the complete-notice
package, and uploads nothing
([CI package downloads](../ci-artifacts.md#license-boundary-and-availability)).

## `mac` 0.1.1

**Resolved by the project owner's license review, November 15, 2026.** The owner
decided that GitTurtle distributes `mac` 0.1.1 under its MIT option, and that its
notice is the standard MIT License text with the copyright line
`Copyright (c) Jonathan Reem`, the author the manifest and README below name.
Upstream supplies no such file, so `dependencies.json` records the notice as
composed, not fetched: the
[standard MIT text](https://raw.githubusercontent.com/spdx/license-list-data/16f3aa6c3bdd62e50f8b1cf618f32d2a510250ee/text/MIT.txt)
of the SPDX license dataset revision that `hexf-parse` already uses (SHA-256
`b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5`, fetched on
November 15, 2026), with its one placeholder line
`Copyright (c) <year> <copyright holders>` filled in. No other byte differs. The
composed text has SHA-256
`8a188924d8a3867afab78b11af96d1e10e6658ec82b7f9260d439d035b9fcff6`. The package no
longer needs review, and the prepared follow-up is no longer needed for
distribution; upstream's issue stays open. The findings below are kept as the
record.

The published Cargo archive has SHA-256
`c41e0c4fef86961ac6d6f8a82609f55f31b05e4fce149ac5710e439df7619ba4`.
Its upstream [0.1.1 release](https://github.com/reem/rust-mac/tree/66afc663b68a65633ea165c742b5a9c6734581c2)
is commit `66afc663b68a65633ea165c742b5a9c6734581c2`. All twelve supplied
source, manifest, README, and configuration files matched that tree byte for
byte. The complete upstream tree and published archive contain no license,
copyright, or notice file; the Rust source contains no embedded license text.

The [release README](https://raw.githubusercontent.com/reem/rust-mac/66afc663b68a65633ea165c742b5a9c6734581c2/README.md)
identifies Jonathan Reem as the author and declares MIT/Apache-2.0. The
[upstream relicensing commit](https://github.com/reem/rust-mac/commit/6b35d271ca8783a513aaa9bb47c2a926a68107d2)
also explicitly records that choice. These establish the declared terms, but
do not supply the omitted complete notice. Upstream's
[request to include LICENSE](https://github.com/reem/rust-mac/issues/14) remained
open when inspected.

Prepared follow-up for that existing issue (unsent; no longer needed for
distribution):

> We are preparing a binary that includes mac 0.1.1, tag
> 66afc663b68a65633ea165c742b5a9c6734581c2. Its Cargo metadata and README declare
> MIT/Apache-2.0, but neither the published crate nor the tag contains the full
> license text or a copyright notice. Could you provide the complete upstream
> notice applicable to this release, including any required copyright or NOTICE
> text, so we can preserve it without inventing an attribution?

## Rust `ufbx` wrapper 0.11.3

**Resolved by 0.11.5, October 25, 2026.** The dependency moved to 0.11.5 for the
native library's filename buffer overflow
([GHSA-6j9m-9g7j-w7vg](https://github.com/ufbx/ufbx/security/advisories/GHSA-6j9m-9g7j-w7vg)).
That archive (Cargo.lock checksum
`260522f1eb74139f0e78b29dd82a58e5437d728c5498024a39e5af7282694240`) records
commit `856c76840977cd504f053bf17ab4948b675b981a`, and its manifest declares
`MIT OR Unlicense`. The archive still omits a license file, but that revision's
[`LICENSE`](https://raw.githubusercontent.com/ufbx/ufbx-rust/856c76840977cd504f053bf17ab4948b675b981a/LICENSE),
added upstream on September 16, 2026, offers MIT (copyright Samuli Raivio) or the
Unlicense. It is byte-identical to the native notice (SHA-256
`0dd48ebadf52273c736256325c8f078c03c8bb4facee22a4122de0ad3f615391`), which
answers both questions below. It is recorded as a supplement in
`dependencies.json`, so the wrapper no longer needs review and the prepared question
is no longer needed. The 0.11.3 findings below are kept as the record of that version.

**Option chosen, November 15, 2026.** The project owner's license review decided
that GitTurtle distributes `ufbx` 0.11.5 under its Unlicense option, which
requires no attribution. The supplement's `note` records the choice. No text
changed: the upstream `LICENSE` above and the native library's notice in
`assets/` ship as before.

The 0.11.3 published Cargo archive has SHA-256
`e160c3af14ab5a02f206e3af48777cb20e09bc22a354aa77d4e5cf4acb4a11a0`.
Its `.cargo_vcs_info.json` records commit
`a78b5b848e28437aeed9536092fdc752cd0a73c8` in
[ufbx/ufbx-rust](https://github.com/ufbx/ufbx-rust/tree/a78b5b848e28437aeed9536092fdc752cd0a73c8).
The original Cargo manifest, generated Rust source, and native C/header files
match that revision exactly. The remaining supplied Rust files, build script,
and README match after normalizing CRLF to LF. The complete upstream tree and
published archive have no license, copyright, or notice file; the wrapper Rust
source contains no embedded license text.

The [exact release manifest](https://raw.githubusercontent.com/ufbx/ufbx-rust/a78b5b848e28437aeed9536092fdc752cd0a73c8/Cargo.toml)
declares `MIT OR PDDL-1.0` and identifies `bqqbarbhg` as the author. The separately
preserved native ufbx notice offers MIT or the Unlicense, with its own copyright
notice. PDDL-1.0 and the Unlicense are different texts. The native library's
notice therefore does not establish which full notice applies to the Rust
wrapper, and is not substituted for it.

Prepared question for the [wrapper maintainers](https://github.com/ufbx/ufbx-rust/issues):

> We are preparing a binary that includes ufbx 0.11.3, revision
> a78b5b848e28437aeed9536092fdc752cd0a73c8. Its manifest declares MIT OR PDDL-1.0,
> but the crate and upstream revision omit the full Rust-wrapper license notice.
> Could you provide the applicable full license and copyright notice? Could you
> also confirm the intended alternative: the native ufbx library's separate
> notice offers the Unlicense, while this wrapper declares PDDL-1.0?

## Completing collection

Obtain the missing authoritative notice or a documented review of the declared
license option and its required attribution for the exact dependency version.
Preserve any upstream text unchanged in `texts/`, then record its immutable URL,
revision, and SHA-256 in `dependencies.json`. A notice that such a review
decides, as `mac`'s was, is recorded as composed: it names the standard text it
starts from and what was filled in, and never a URL that does not serve its
bytes. Do not add the unanswered questions
or the Cargo declarations as if they were complete license texts. Another option
is to change the affected dependency after evaluating the application impact;
that requires fresh dependency, native, and package validation.

Regenerate the target-specific inventory with
`python3 scripts/collect-third-party-licenses.py --require-complete --target x86_64-unknown-linux-gnu /tmp/gitturtle-release-licenses`
using a new output directory. Since November 15, 2026 the expected strict result
for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` is exit 0 with
nothing listed (`mac-0.1.1` was listed until then, and `ufbx-0.11.3` before the
0.11.5 update). `aarch64-apple-darwin` still exits 2 with `block-0.1.6`,
`leak-0.1.2`, `leaky-cow-0.1.1` and `objc_exception-0.1.2`: those four are not
decided and need their own review.
