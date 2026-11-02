# Linux PDF pages: architecture review (2026-11-02)

The [architecture review](architecture-review.md) of the first Linux parity item in [full Omarchy support](omarchy-full-support.md) (item 9): rendering captured PDF pages on Linux with `hayro` in GitTurtle's own helper process, as the [2026-10-28 research note](2026-10-28-linux-pdf-pages.md) recommends and the owner decided on 2026-10-28. Base revision: `11de4b50`. Verdict: **the helper process is accepted**, one helper per request for the first slices. Its confinement is decided by the independent [security review](security-review.md) (section 3 lists the questions).

## 1. Dependency refresh (checked 2026-11-02)

Sources: the crates.io API (`https://crates.io/api/v1/crates/<name>`) and the GitHub API for `LaurenzV/hayro`. `main` moved from `8558a8a5` to `04a801de` (2026-11-01); no release since `hayro-v0.8.0` (`ea9c81dc`).

| Item | 2026-11-02 |
| --- | --- |
| `hayro`, `hayro-syntax`, `hayro-interpret` 0.8.0; `hayro-jpeg2000` 0.4.1; `hayro-jbig2` 0.3.1; `hayro-ccitt` 0.4.0; `hayro-cmap` 0.1.0 | Unchanged: `Apache-2.0 OR MIT`, none yanked, same archive checksums. MSRV 1.92 against the pinned 1.99.0. Correction: `hayro-cmap` 0.1.0 declares no `rust-version`. |
| hayro features at `ea9c81dc` | `vello_cpu` without default features (`std`, `png`, `u8_pipeline`): single-threaded rendering. Fonts come from the embedded `embed-fonts` data, so no system font is read. Both simplify confinement. |
| #1549 (JPEG 2000 memory) | Open; confirmed 2026-10-26 to abort in under a second under a 4 GB `ulimit -v`. Fix PR #1558 open. |
| #1588 (bit-packed images), #1610 (inline-image recursion) | Open, no activity. |
| #1052 (cancellation) | Open; duplicate #1703 closed 2026-11-01; #1683 (a "fuel" callback) opened 2026-10-29. Nothing shipped. |
| #1649 (JPEG 2000 panics) | Open; fix PR #1679 not merged. |
| JBIG2 | #1550 and #1598 got smaller triggers on 2026-11-01 (187 bytes reach 6.4 GB and 28 s; 236 bytes take 38.9 s). New: #1695 (2 GB), #1700 (a loop that never ends), #1701 (1.4 GB, 20 s). #1634 (object streams decoded again per lookup) open. |
| Performance | #1613 closed 2026-11-01 by PR #1706 ("Cache rasterized glyphs"), on `main` only; `deny.toml` (`unknown-git = "deny"`) rules out a git dependency. |
| Confinement crates, only if the security review picks one | `seccompiler` 0.5.0 (2025-03-07, `Apache-2.0 OR BSD-3-Clause`) would need its own research note; `landlock` 0.4.7 (2026-07-27, `MIT OR Apache-2.0`). |

The recommendation stands, and the new reports strengthen the case for a separate process: more tiny inputs reach gigabytes, and #1700 never ends, which only a time limit stops.

## 2. Review

**Owners.** Today Linux refuses PDFs at `crates/preview/src/lib.rs:64-67` and `:85-88`; the macOS backend builds only on macOS (`lib.rs:23-24`).

- Preview crate, Linux only: `src/pdf_hayro.rs` renders a page and extracts its text with hayro under the existing bounds (`lib.rs:27-29`, `:91-104`, `:267-279`); `src/pdf_helper.rs` holds the message format (`encode_request`, a bounded `read_reply`) and `serve(Read, Write)`, the only public way into hayro, so nothing runs hayro inside the app process. `decode_pdf_page` and `decode_pdf_page_text` keep refusing on Linux.
- App crate: `src/pdf_helper.rs` (Linux) holds the helper's entry point, its self-confinement and the client (start, write, wait, deadline, kill, reap, concurrency limit). `pdf_view.rs` gains one `prepare_page(bytes, page, check)` used by both call sites (`rich_preview.rs:132` with `pdf_view.rs:163`, and `pdf_view.rs:232`/`:240`).
- Not Git core: its `process_io` is private Git plumbing.

**Launch.** Program `/proc/self/exe` (the running binary even after a package upgrade), `current_exe()` as fallback. Arguments `--gitturtle-pdf-helper <protocol version> <parent pid>`, plus test-only overrides that can only tighten limits. Dispatched as the first statement of `main()`, before askpass (`main.rs:2437`), the version flags and GPUI, matching exactly this shape so a repository path can never be mistaken for it. `env_clear()`, `current_dir("/")`, stdin and stdout piped, stderr to `/dev/null`. The helper confines itself before reading a byte (`close_range(3, ~0)`, resource limits, `no_new_privs`, the parent-death signal, then what the security review decides): doing it in the helper keeps Rust's `posix_spawn` path, where a `pre_exec` hook would force `fork` in the multithreaded GPUI process, and every launcher, the integration test included, gets the same confinement.

**Protocol.** Request: magic and version, page (u32), length (u32), the PDF bytes; the helper checks the 32 MiB limit, the `%PDF-` header and the page bound before allocating. Reply: magic and version, then success or a fixed error kind. Success carries the page count, original and output size, exactly width × height × 4 RGBA bytes, a text state (none, some, error kind) with a truncated flag and at most 256 KiB of UTF-8, and phase timings. The client maps error kinds to its own fixed wording and never displays text the helper sends.

**One helper per request** for the first slices: nothing outlives the request, so pause (`pdf_view.rs:187-191`), warm tabs, closing a tab and Before/After linking need nothing new; each request gets a fresh address space, so a hostile page cannot affect the next; cancelling already means killing. The release measurement can revisit it (section 5).

**Limits.** Existing bounds unchanged and checked in both processes: 32 MiB input, 1,000 px output edge from `render_dimensions()` before rasterizing plus `check_dimensions`, 256 KiB text cut at a character boundary as macOS does (`native/pdf_text.rs:129-175`). Reply at most 4,000,000 + 262,144 bytes plus header, else killed and reported unreadable. `RLIMIT_DATA` 1 GiB (the main cap), `RLIMIT_AS` 2 GiB as a backstop (to confirm against the helper's starting virtual size). `RLIMIT_CPU` soft 8 s, hard 9 s; a 10 s wall-clock SIGKILL from the client and `alarm(11)` inside. `RLIMIT_CORE` 0, `RLIMIT_FSIZE` 0, `RLIMIT_NOFILE` 3, `RLIMIT_NPROC` 0. At most two helpers across the app (requests come from each window's reader and the shared page lane, whose queue of 8 is unchanged).

**Cancellation and stale results.** The client polls non-blocking pipes in ticks of at most 25 ms, running the existing check (the page lane's generation check, `pdf_view.rs:220-229`, or `cancellation.check()` on the worker) and the deadline; a failed check is SIGKILL, `waitpid` and the check's error. Late results are still dropped at `pdf_view.rs:265`. A drop guard kills and reaps the child on every path. Dropping the reply channel alone would not stop the process.

**What reaches the UI.** Owned RGBA and `Option<String>` text, through `worker::render_image` off the UI thread as now. No hayro type enters the app.

**Wording on Linux.** "PDF pages are not rendered on this platform." (`lib.rs:66`) goes. Document errors reuse the macOS strings (`native.rs:347-377`). Helper failures get one sentence each: took longer than 10 seconds and was stopped; the PDF renderer stopped on this page; the renderer returned an unreadable result; the renderer could not start. Text failures stay "Page text is unavailable: …" (`pdf_view.rs:91`) with the pixels shown. Later pages offer Retry page (`pdf_view.rs:605-610`); a failed first page has none, as on macOS.

**Failure and recovery.** Crash, kill, timeout, unreadable reply and start failure are each one explicit page error. No automatic retry (a hostile input would burn the CPU again). Nothing is written to disk; the 4-page, 16 MiB cache is unaffected. The parent-death signal ends a helper whose app died.

**macOS unchanged.** hayro under `[target.'cfg(target_os = "linux")'.dependencies]`; new modules and the dispatch Linux-only; the macOS branch of `prepare_page` calls the same two functions in the same order. Proof: `cargo tree --target aarch64-apple-darwin -i hayro` is empty, then the later check on a Mac.

**Test seams.** Preview: in-process tests on benign generated fixtures and protocol refusals (the PDF generator at `native.rs:416` moves to a shared test helper). App client: a `HelperCommand` so tests use `/bin/sh -c` fakes; under `cfg(test)` a deterministic test renderer keeps `paged()` (`pdf_view.rs:698-730`) and its callers passing. Real helper: `crates/app/tests/pdf_helper.rs` drives the confined helper through `env!("CARGO_BIN_EXE_gitturtle")`. Hostile fixtures built in test code from #1549, #1588 (60000 × 60000 at 1 bit), #1610, #1550, #1598, #1700, #1649 and a flate bomb, each with its issue URL.

## 3. Confinement questions for the security review

1. **seccomp:** recommended, an allow-list applied after setup that kills on anything else (read on 0, write on 1, close, exit_group, mmap, munmap, mremap, brk, madvise, mprotect, rt_sigreturn, sigaltstack, futex, getrandom). Open: `seccompiler` (needs a research note) or a hand-built filter on `libc` with architecture checks for AArch64 and x86-64. Evidence: `Seccomp: 2` and the fixtures passing with the filter on.
2. **Landlock:** only if seccomp is rejected (deny filesystem, TCP, signals and abstract sockets where the kernel supports them).
3. **`no_new_privs`:** mandatory.
4. **Descriptors:** `close_range(3, ~0)` before reading input, then `RLIMIT_NOFILE` 3; a test passes an inheritable descriptor and checks `/proc/<pid>/fd` shows 0, 1 and 2 only.
5. **`PR_SET_PDEATHSIG`:** SIGKILL, then `getppid()` checked against the parent pid argument (the signal follows the spawning thread; the lane and reader threads are long-lived). `alarm` is the backstop.
6. **Also:** `PR_SET_DUMPABLE` 0 with `RLIMIT_CORE` 0, so systemd-coredump cannot store repository content from a crash. No `bwrap` or user namespaces in the first version.

## 4. Slices (in order, disjoint files)

- **T1** `feat(preview): render PDF pages and page text with hayro for a confined helper`: `crates/preview/{Cargo.toml, src/lib.rs (module declarations, test generator), src/pdf_hayro.rs, src/pdf_helper.rs, AGENTS.md}`, `Cargo.lock`, `docs/licenses/`, `THIRD_PARTY_NOTICES.md` (hayro's Foxit, CMap and CC0 notices through the collector's supplements). Linux behavior unchanged. Acceptance: `serve` on generated fixtures gives the page count, crop box and rotation within 1,000 px, a white background and the expected pixels, an empty-password file opens and a real password is refused; text present, absent and truncated at a character boundary past 256 KiB; every out-of-bounds frame refused before allocating; a hayro panic becomes an error kind; `cargo deny check` passes and the macOS `cargo tree` has no hayro.
- **T2** `feat(app): run PDF pages in a confined helper process on Linux` (after the security review): `crates/app/src/pdf_helper.rs`, the dispatch lines in `main.rs`, `crates/app/tests/pdf_helper.rs`. Acceptance: the real binary round-trips a page with `/proc` showing the decided confinement and only descriptors 0–2; every hostile fixture ends in its expected failure within the deadline plus 1 s; with shell fakes, cancellation leaves no child within 100 ms and deadline, unreadable or oversized reply, early exit and start failure each give their own error with no zombie; at most two helpers, a waiting request still cancels; every other argument launches the app as before.
- **T3** `feat(app): show PDF pages and page text on Linux`: `pdf_view.rs`, `rich_preview.rs`, `crates/app/AGENTS.md`, `docs/{document-previews.md, file-previews.md, linux.md}`, `README.md`, the benchmark record. Acceptance: both call sites through `prepare_page`, macOS unchanged; a late result after a page change or pause is dropped and its helper killed; Linux wording; Retry page after a deadline; native evidence and the release measurement.

## 5. Measurement and native evidence

**Release measurement** (an `#[ignore]` release test in `crates/app/tests/pdf_helper.rs` writing JSON to an environment-named file; fixtures in a disposable `.local/bench-pdf-<date>/` with SHA-256): a generated 300-page dense-text document (first, middle and last page); from hayro's corpus at `ea9c81dc` embedded CJK/CID, scanned JPEG 2000, JBIG2, CMYK, transparency with a soft mask and a complex shading; a 32 MiB image-heavy file. Time helper start, input write, parse, render, text and the whole request, at least 20 runs per page, first after start and warm, p50/p95/max; peak memory (`wait4` `ru_maxrss`), kill-to-reap time, the release executable's size before and after, and UI frame times while a 2 s page renders. Proposed budgets: warm next page p95 at most 120 ms (text) and 400 ms (image-heavy); a cold first page adds at most 60 ms; legitimate pages under 2 s; twice the largest legitimate peak within 1 GiB. If start, transfer and parse together exceed 40 ms at p95 or 35% of the warm next page, queue a per-document helper.

**Native evidence** (Omarchy, Wayland, Midnight and Daylight, 13 and 18 pt, scale 1 and fractional): a disposable repository with a PDF going from 3 to 5 pages, an added PDF, a password-protected PDF, a rotated or cropped page and a #1598 JBIG2 page that reaches the deadline. Previous/Next and direct entry, linked and independent sides with different page counts, Fit and 25–400% zoom, Read page text (dialog, selection, focus return), the absent side, error wording and Retry page, Back during a slow page (Compare stays closed; `ps` shows no helper left), the app responsive through the hostile page, AT-SPI labels, a running helper's `/proc/<pid>/status`, and whether "Preparing selected page…" flashes between fast pages (design review).

**Unknowns:** the helper's starting virtual size (sets `RLIMIT_AS`); hayro's fidelity on real repository PDFs; whether CoreGraphics draws annotation appearances (decides hayro's `render_annotations` for parity); whether `seccompiler` passes research; a hayro release with glyph caching or #1052, which would change per-page cost or reopen in-process rendering.
