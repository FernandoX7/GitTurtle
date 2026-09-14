# Supplemental license notices

`dependencies.json` maps package/version pairs to license texts omitted from
their published Cargo archives. `texts/` stores exact fetched text, deduplicated
by SHA-256. Each entry records the public upstream repository, immutable revision,
source URL, and checksum. Most use the archive's `.cargo_vcs_info.json` revision;
entries whose old revision lacked the text explicitly identify a later upstream
snapshot. `hexf-parse` retains its declared CC0-1.0 terms using the standard text
from a pinned SPDX license dataset revision, rather than substituting its later
upstream ISC license. These records do not change dependency license terms.

`assets/sources.json` records the same provenance for theme palettes, toolkit
Lucide/Feather icons, and the native ufbx library. Preserve the complete original
copyright/license text. Upstream notices were inspected and captured on
September 14, 2026.

The [collector](../../scripts/collect-third-party-licenses.py) copies notices from
the resolved local crate archives first, including meshoptimizer's complete
embedded header notice, then these supplements. It verifies
supplement hashes and records only target-selected dependencies. It does not
fetch new notice text, rewrite licenses, or consider an SPDX declaration alone a
complete recovered notice. Cargo metadata may download missing locked crates.

```sh
python3 scripts/collect-third-party-licenses.py --target x86_64-unknown-linux-gnu /tmp/gitturtle-linux-licenses
# Required before distributing a public binary; currently reports missing notices.
python3 scripts/collect-third-party-licenses.py --require-complete --target x86_64-unknown-linux-gnu /tmp/gitturtle-release-licenses
```

Use a new or empty output directory. Exit 0 means the local collection completed,
not that release clearance passed; `REVIEW_REQUIRED.md` and `dependencies.json`
name unresolved notices. Strict mode returns 2 when such gaps exist. Invalid
inputs, changed supplemental hashes, unavailable metadata, or filesystem errors
return 1. The [top-level notices](../../THIRD_PARTY_NOTICES.md) explain scope and
distribution requirements.

The [Linux notice review](linux-notice-review.md) records exact package/source
identity for the remaining `mac` and Rust `ufbx` gaps, the upstream findings,
and prepared follow-ups. It supplies evidence and next steps, not replacement
license text or release clearance.
