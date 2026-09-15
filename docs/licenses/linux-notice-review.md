# Linux notice review — September 14, 2026

The locked Linux dependency inventory still has two unresolved notice records.
This review establishes package provenance and records the missing information;
it does not assert that a public binary has completed license review. No upstream
message was sent during this investigation.

## `mac` 0.1.1

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

Prepared follow-up for that existing issue:

> We are preparing a binary that includes mac 0.1.1, tag
> 66afc663b68a65633ea165c742b5a9c6734581c2. Its Cargo metadata and README declare
> MIT/Apache-2.0, but neither the published crate nor the tag contains the full
> license text or a copyright notice. Could you provide the complete upstream
> notice applicable to this release, including any required copyright or NOTICE
> text, so we can preserve it without inventing an attribution?

## Rust `ufbx` wrapper 0.11.3

The published Cargo archive has SHA-256
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
revision, and SHA-256 in `dependencies.json`. Do not add the unanswered questions
or the Cargo declarations as if they were complete license texts. Another option
is to change the affected dependency after evaluating the application impact;
that requires fresh dependency, native, and package validation.

Regenerate the target-specific inventory with
`python3 scripts/collect-third-party-licenses.py --require-complete --target x86_64-unknown-linux-gnu /tmp/gitturtle-release-licenses`
using a new output directory. Until these records are resolved, the expected
strict result is exit 2 with `mac-0.1.1` and `ufbx-0.11.3` listed. Other targets
have additional unresolved records and need their own review.
