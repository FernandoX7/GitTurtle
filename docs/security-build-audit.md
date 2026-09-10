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

## Final source gates completed

The final gate records identify compiled source
`b4440f132999ebc45a81b1baa31eda4594bddd66`. All gates below exited successfully
on September 10, 2026. Times include each command's build/cache work and are
verification durations, not application performance comparisons.

| Gate | macOS command | macOS elapsed | Linux container elapsed |
| --- | --- | ---: | ---: |
| Formatting | `cargo fmt --all -- --check` | 1.496 s | 6.258 s |
| Workspace tests | `cargo test --locked --workspace` | 97.883 s | 34.882 s |
| Strict lint | `cargo clippy --locked --workspace --all-targets -- -D warnings` | 22.653 s | 6.624 s |
| Release build | `cargo build --release --locked -p gitturtle` | 39.777 s | 81.389 s |

The macOS gate sequence began at `08:48:18.960 UTC`; its release build ended
at approximately `08:51:00.958 UTC`. Linux formatting began at
`08:48:32.395 UTC`; its release build ended at approximately
`08:50:41.552 UTC`, followed by successful executable identity inspection.
The Linux test command also used `--offline --no-fail-fast`; lint and release
used `--offline`.

The final macOS workspace run passed **589 top-level tests**, with zero failures
and five ignored tests: 294 application, 236 core unit/integration and 59 preview
tests. Linux passed **582 top-level tests**, with zero failures and five ignored:
294 application, 237 core and 51 preview tests. The platform counts differ because
coverage is conditionally compiled. A preview subprocess prints its own one-test
result; these totals exclude that nested result to avoid counting it twice.
The ordinary gates leave the installed-CLI origin fixture, release microbenchmark,
optional OpenPGP/loopback-SSH fixtures and bounded SVG mutation probe ignored.
Separately executed targeted evidence remains in the focused audit records; an
ignored test is not claimed to have run in these final default gates.

Linux ran in the existing `gitturtle-linux-qa-20260909:043f562` local image:
Linux `7.0.12-linuxkit aarch64`, Rust/Cargo 1.98.0 and Git 2.43.0. Commands used
read-only source/root filesystem, dropped capabilities, no new privileges,
six CPUs, 12 GiB memory, 4 GiB temporary storage and `--network none`.
The built executable is ELF64/AArch64, and `ldd` reported no missing libraries.
Its SHA-256 is
`53be653edc3cb1824a52d7cffce094ae6e6b2cb9a88f1a048cba747ed6ecb1e3`.
This establishes actual local Linux compilation and fixture execution; it does
not establish native Linux desktop interaction, x86-64 support or hosted CI.

## Packaged and installed artifact verified

The packaged macOS application is currently the private
`package/GitTurtle.app` artifact. Its identity record was written under its earlier
temporary build name; the relocated executable's SHA-256 was checked against
that record and matches. The record confirms unchanged compiled inputs, a plist
equal to the backed-up baseline, byte-identical original `AppIcon.icns`, and a
verified signature. The recorded provenance and resource identities are:

| Item | Identity |
| --- | --- |
| Compiled source | `b4440f132999ebc45a81b1baa31eda4594bddd66` |
| Compiled-input manifest digest | `8a407d51167f8761b24a447ca7a2689bd75db3d222b058456dee7ce780f37482` |
| macOS executable UUID | `2475FAA0-21B2-398D-9F9C-A2D57C92543A` |
| macOS executable SHA-256 | `3ce5c6e541c1c7d459982c63ea497705511680f837ec78fb32ea5a8e1e21d020` |
| `Info.plist` SHA-256 | `7bb42672a0c5377612a496e46230bfe06510f52d1eecab94f3747e041abf9f61` |
| `AppIcon.icns` SHA-256 | `5206a75c4c2ae53f7686565042bb7e4e67b1ca09ebc3d4f2e5a621f3be37fefb` |
| `Assets.car` SHA-256 | `10c87d69648cafa9b023c736b1f959c49c849b480b8cad2a431d94ac214d69eb` |
| Signature resource manifest SHA-256 | `8b50316fd3ac167097ca1fe4a81ade3d875206b483d890677fc1472e95c4899b` |

The private source-input record hashes to
`23c2a9195ad3afa6fad65a327308f95248489ce14a6ad00034f23c4c19dbdae5`;
the gate-summary record hashes to
`7817d027d652727d17c43a5866d5e60c6dbe99d83d66594b63c9a24b1c9d7a6a`;
and the package-identity record hashes to
`caa6751c38daedfbf9094a464f14b1ca52a9ec683b44831ac4d474532057f46e`.
Those records retain per-command log hashes without publishing private paths.

The [mixed native session](benchmarks/security-native-20260910.md) ran for
20 minutes 15.279 seconds with 244 samples. Normal Quit left no application
process and none of the 26 previously observed descendant PIDs. The verified
bundle was installed at `/Applications/GitTurtle.app` at `09:17:35 UTC`.
All five packaged files above match the installed copies; strict signature and
UUID checks passed again. PID 61133 launched that exact executable path, and
its mapped executable inode matched the hashed file. Installed smoke restored
eight tabs, exact Unicode commit/review drafts, an unfinished inline composer,
appearance preferences and the captured Markdown comparison. An offline
submission after fixture head movement was refused with its draft retained.

The original three-file application state was restored byte-for-byte at
`09:21:25 UTC`; global appearance and the complete captured accessibility
settings were unchanged. PID 72968 then launched the installed path with fixture
mode disabled and restored the genuine three tabs. Its executable mapping/hash
matched. Startup refreshed only the active development repository's pinned
history tip from handoff `b36efd0` to compiled source `b4440f1`; all other restored
state matched. Subsequent authorized documentation commits may naturally refresh
that same tip. The original backup remains private. The
[installed identity record](benchmarks/security-installed-20260910.json)
retains these checks and evidence hashes without publishing genuine repository
paths, application state or credentials. The prior
[Markdown baseline profiling](benchmarks/security-markdown-20260910.md) belongs to
the handoff executable; its observed memory peak is neither evidence of this
package's memory behaviour nor a before/after improvement claim.

## Residual boundaries

Git and deliberately configured hooks, credential helpers, filters and signing
programs retain user permissions for explicit operations. They are not sandboxed
by passive-read flags. App-store protection does not defeat a malicious process
already controlling the same account's parent directories. Provider creation
and filesystem operations do not become atomic transactions with independent
external actors. Native codec calls can stall within one call, and cache counts
do not include every editor, font, allocator or GPU allocation. These are explicit
limits, with no claim of full security or unrestricted scalability.
