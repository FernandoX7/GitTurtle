# Isolated large offline PR probe

Prepared from `b4440f132999ebc45a81b1baa31eda4594bddd66`; the release probe passed
on September 10 at 09:18:04 UTC after native mixed sampling ended. Preparation
and compilation remained separate from the native sample and runtime accounting.

The [preparer](prepare-large-pr-probe.py) copies the actual GitHub client,
review, draft and transport modules, plus Git core. It extracts the exact
`PendingFile`, `read_store`, `read_store_contents` and `atomic_write` implementation
from preferences. Only `settings_path` is replaced: it always errors, preventing
the isolated crate from finding genuine application state. The
[test-only harness](large-pr-probe.rs) is appended inside the copied draft module,
so it can use the production private APIs with an explicit temporary file.

The probe exercises:

- Three explicit 100-file pages: **300 unique files**, **401 parsed rows per
  file**, **120,300 rows** prepared across pages, exact 200-line added-side
  selection, cross-side refusal, and metadata-before-row preparation. Each page
  is released before loading the next.
- Twenty explicit 100-comment pages: **2,000 unique comments**, **1,000 roots and
  1,000 replies**. Per-page grouping marks replies whose roots are on earlier
  pages; a separate aggregate helper check reconstructs **1,000 two-comment
  threads**. The aggregate helper check exceeds the live panel's one-page
  retention, and is not a claim that the UI retains every thread together.
- **128 review drafts**, each with a 4 KiB description and **100 inline comments**
  containing more than 512 bytes each: save/reload preserves exact text. A batch
  that updates an existing draft then adds a 129th must preserve the entire
  original store; a subsequent valid update must retain the other 127 entries.
- Changed-head refusal after a page read, 101-file page refusal, truncated JSON,
  the 20,000-row patch boundary, invalid file page 31, pre-dispatch cancellation,
  and no retry after failed reads.

The synthetic `Transport` accepts only expected GET endpoints and never invokes
the live transport, credential storage, Git executable or network. Successful
page reads must issue exactly **69 synthetic GET calls**, including identity
checks before and after every page. There are no POSTs. A foreign URL in a Link
header is only a next-page signal; unexpected destinations fail the test.

## Preparation and execution

Run from the repository root into a **new** private output directory:

```sh
python3 docs/benchmarks/prepare-large-pr-probe.py \
  --base b4440f132999ebc45a81b1baa31eda4594bddd66 \
  --output .local/security-milestone-20260910/large-pr-probe
```

The preparer runs offline Cargo metadata resolution and verifies that all
**38 registry packages** retain baseline versions and checksums. It records
original/generated source hashes, the exact extraction hash, harness/preparer
hashes, and the isolated lockfile in `provenance.json`. It refuses an existing
output directory. No production source file is modified.

After native sampling ends:

```sh
cargo test --release --locked --offline \
  --manifest-path .local/security-milestone-20260910/large-pr-probe/Cargo.toml \
  large_offline_pr_probe -- --nocapture --test-threads=1
```

The build uses a private target directory, ordinary Rust/Serde/Git-core
dependencies and the already locked macOS security-framework bindings. It does
not compile GPUI or launch the app. Thin LTO and eight codegen units match the
release profile. Only the named probe executes; copied ordinary unit tests are
compiled but filtered out. Its output includes a `LARGE_PR_PROBE_RESULT` JSON
line with actual dimensions, store bytes and elapsed phase observations.

For process RSS/CPU evidence, first build with the same command's `--no-run`
option and retain the printed test executable path; then run that executable
with `large_offline_pr_probe --nocapture --test-threads=1` under `/usr/bin/time -l`.
This keeps compilation outside runtime accounting. Preserve its SHA-256, stdout,
resource output, exit status and source manifest. A single probe invocation is
behavioral and resource evidence, not a comparative performance benchmark.

This covers actual backend parsing, paging, grouping and persistence APIs with
supplied responses. It does not verify a hosted provider, network transport,
native rendering/focus at this scale, maximum PR capacity, or app-wide memory
growth. The four-file native offline fixture remains separate evidence.

## Executed result

The [sanitized raw result](large-pr-probe-20260910.json) retains the exact source,
extraction, harness and executable hashes; stdout; resource counters; and every
reported dimension. Compilation with the locked offline graph passed in 9.43
seconds. The separately invoked release executable SHA-256 was
`3f7cda07231c478bdc1e54ebd3d241629090fde0ac13e073a1f6828d71c26865`.
All copied source hashes matched before and after execution.

The named test passed, with 27 copied ordinary tests filtered out. It exercised
all workload and refusal assertions above. The largest file page contained
2,722,000 patch bytes; the initial 128-draft store contained 9,256,011 encoded
bytes. Successful page reads issued 69 synthetic GET calls. The transport
performed no live request or POST, and the explicit temporary draft directory
was removed when its owner was dropped.

On Apple M4 Max, 128 GiB RAM, macOS 26.6.2 arm64, `/usr/bin/time -l` reported
0.34 seconds real, 0.10 seconds user CPU and 0.02 seconds system CPU; maximum RSS
was 159,334,400 bytes (151.95 MiB), peak footprint 115,507,704 bytes (110.16 MiB),
and swaps were zero. One-minute load average was 6.75 before/after; background
desktop and installation work was uncontrolled. The probe's file phase took
38.97 ms, comment phase 5.09 ms and draft phase 72.41 ms. These phases include
synthetic response construction and assertions as well as production calls.

This was one behavioral/resource invocation, with no baseline or warmup series.
Full-corpus thread aggregation and draft/JSON equality checks contribute to the
probe's peak memory. Those peaks are neither application memory measurements nor
leak evidence, and the phase times do not establish native latency or a speedup.
