# Supplemental license notices

`dependencies.json` maps package/version pairs to license texts omitted from
their published Cargo archives. `texts/` stores exact fetched text, deduplicated
by SHA-256, and one composed text that its record marks as such (see below).
Each entry records the public upstream repository, immutable revision,
source URL, and checksum. Most use the archive's `.cargo_vcs_info.json` revision;
entries whose old revision lacked the text explicitly identify a later upstream
snapshot. `hexf-parse` retains its declared CC0-1.0 terms using the standard text
from a pinned SPDX license dataset revision, rather than substituting its later
upstream ISC license. hayro's repository `NOTICE.md` (for `hayro-font`,
`hayro-syntax` and `hayro-interpret`), `hayro-font`'s embedded Foxit font
provenance, `hayro-interpret`'s CMYK profile license and asset provenance, and
the `alloc-no-stdlib` repository license that `alloc-stdlib` omits are captured
at the archives' recorded revisions.
These records do not change dependency license terms.

Two records carry a decision of the project owner's license review of
November 15, 2026; each record's `note` states it:

- `mac` 0.1.1 declares MIT/Apache-2.0 and has no license file upstream or in its
  crate. GitTurtle distributes it under the MIT option. Its notice is the one
  text here that is **composed, not fetched**: the standard MIT License text
  from the SPDX dataset revision `hexf-parse` uses, with its one placeholder
  copyright line filled in as `Copyright (c) Jonathan Reem`.
- `ufbx` 0.11.5 declares MIT OR Unlicense. GitTurtle distributes it under the
  Unlicense option, which requires no attribution. The upstream `LICENSE` that
  already shipped, and the native library's notice in `assets/`, are unchanged.

A file record states exactly one source. A fetched text has the `url` that
serves its exact bytes. A composed text has no such URL, so it has none: it
names its `standard_text` URL and, in `composed`, what was filled in and who
decided it. The collector refuses a record with neither or with both, verifies
either kind's checksum, ships a composed text as `composed-N.txt` instead of
`upstream-N.txt`, and copies the composed record into the inventory's notice
record and `notice_provenance`.

`assets/sources.json` records the same provenance for theme palettes, toolkit
Lucide/Feather icons, the native ufbx library and the fonts Linux builds embed.
Preserve the complete original copyright/license text. Upstream notices were
inspected and captured on September 14, 2026; the five theme palette notices
added on September 17, 2026 and IBM Plex Sans's, added on October 27, 2026, are
pinned in the same file.

The [collector](../../scripts/collect-third-party-licenses.py) copies notices from
the resolved local crate archives first, including meshoptimizer's complete
embedded header notice, then these supplements. It verifies
supplement hashes and records only target-selected dependencies. It does not
fetch new notice text, rewrite licenses, or consider an SPDX declaration alone a
complete recovered notice. Cargo metadata may download missing locked crates.

```sh
python3 scripts/collect-third-party-licenses.py --target x86_64-unknown-linux-gnu /tmp/gitturtle-linux-licenses
# Required before distributing a public binary. Linux targets are complete;
# aarch64-apple-darwin still reports missing notices.
python3 scripts/collect-third-party-licenses.py --require-complete --target x86_64-unknown-linux-gnu /tmp/gitturtle-release-licenses
```

Use a new or empty output directory. Exit 0 means the local collection completed,
not that release clearance passed; `REVIEW_REQUIRED.md` and `dependencies.json`
name unresolved notices. Strict mode returns 2 when such gaps exist. Invalid
inputs, a supplement record without exactly one source, changed supplemental
hashes, unavailable metadata, or filesystem errors return 1. The [top-level notices](../../THIRD_PARTY_NOTICES.md) explain scope and
distribution requirements.

The [Linux notice review](linux-notice-review.md) records exact package/source
identity for the two Linux gaps, the upstream findings, and how each was
resolved: the Rust `ufbx` wrapper's by its 0.11.5 release, `mac`'s by the
owner's license review. The four macOS-only packages (`block`,
`objc_exception`, `leak` and `leaky-cow`) are not decided and stay
review-required.

Complete notices do not upload a CI package by themselves: its platform must
also be listed in `HOSTED_UPLOAD_PLATFORMS` in `scripts/ci/packages.py`. Linux
is listed, so Quality uploads the verified complete-notice Linux archive as a
seven-day Actions artifact, which is a CI build, not a release. macOS is not
listed and stays held until macOS distribution is decided; see
[CI package downloads](../ci-artifacts.md#license-boundary-and-availability).
