# Dependency, native and package boundary audit — September 10, 2026

The locked graph was checked with cargo-audit 0.22.2 against RustSec database
commit `b50980aad8b8f14f77e25a97b32dd94bf008b0af` (1,243 advisories, updated
September 9). No catalogued vulnerability matched; six maintenance advisories
matched. [The numeric record](benchmarks/security-dependencies-20260910.json)
retains exact versions, advisory links, database identity and lockfile hash.
This is catalog matching, not proof that dependencies are safe.

| Unmaintained package | Actual dependency path and decision |
| --- | --- |
| bincode 1.3.3 | syntect → GPUI base syntax support; retained with the pinned toolkit |
| instant 0.1.13 | GPUI and notify-types; retained with their matching versions |
| paste 1.0.15 | GPUI/Metal and image/exr compile-time macros; retained |
| rustls-pemfile 2.2.0 | GPUI reqwest adapter; retained; GitHub uses its separate CLI transport |
| rustybuzz 0.20.1 | GPUI's usvg text shaping; retained |
| ttf-parser 0.25.1 | GPUI, fontdb and Mermaid font metadata; retained |

No advisory suppression was added. A toolkit/font-stack migration would require
separate compatibility and visual work, with no current matching vulnerability
justifying that expansion. The first-party [RustSec database](https://rustsec.org/advisories/)
is the catalog source. Future-incompatibility output for the existing `block`
crate is recorded with gate logs; it is not a current Rust 1.98 compilation error.

## Source and native boundaries

The workspace has no custom build scripts. The local GPUI component build script
passes the assets crate's icon directory to Rust compilation and declares rerun
inputs. Reviewed path patches remain local source inputs, covered by the release
manifest rather than Cargo registry checksums. There is no new dependency version;
core enables readiness polling on its existing `rustix` dependency.

Application unsafe code is concentrated in Unix process flags/group cleanup and
the macOS accessibility observer. Descriptor use keeps a live owner and does not
retain borrowed pointers. The observer removes its notification token on Drop.
Core uses descriptor-relative no-follow filesystem operations through rustix.
Preview native references stay on the worker stack and return owned pixels/text;
CFData-backed ImageIO/CoreGraphics/PDFKit APIs do not open repository document
URLs. PDF text extraction retains its scoped autorelease pool and returned-text
limit. These reviewed call patterns are not exhaustive FFI verification or a
sandbox for native codecs. Supplied-byte decoding, cooperative cancellation and
resource limits are covered by the focused preview audit and existing contracts.

The macOS package script accepts a GitTurtle bundle destination, rebuilds the
original Icon Composer source, installs the executable/resources, writes its
plist and verifies an ad-hoc signature. It ships no credentials, fixture data or
source repository. Embedded control artwork requires recompilation; all original
artwork remains unchanged in this milestone. The final record verifies actual
bundle provenance, resources, UUID/hash and exact-path installation separately.
Ad-hoc signing does not establish notarization, a hardened runtime, distribution
trust, universal architecture support or an application sandbox.

The configured quality workflow pins checkout by commit, has read-only repository
permissions, disables persisted checkout credentials, and runs local fixture
tests without publication. An authored workflow is not evidence of a hosted run.
The existing isolated Linux image can exercise source gates with read-only source,
no network during tests, bounded CPU/memory and disposable temporary storage;
native Linux interaction and hosted checks remain separate limits.

## Residual boundaries

Git and deliberately configured hooks, credential helpers, filters and signing
programs retain user permissions for explicit operations. They are not sandboxed
by passive-read flags. App-store protection does not defeat a malicious process
already controlling the same account's parent directories. Provider creation
and filesystem operations do not become atomic transactions with independent
external actors. Native codec calls can stall within one call, and cache counts
do not include every editor, font, allocator or GPU allocation. These are explicit
limits, with no claim of full security or unrestricted scalability.
