# macOS milestone backend measurements — September 8, 2026

The new attribution, tag discovery and ignore-preparation reads completed on a disposable fixture in release mode. Committed attribution had a 72.214 ms median and 79.221 ms p95; working attribution, which additionally reads status and maps raw working lines, had a 129.544 ms median and 147.245 ms p95. These are current costs under substantial background load, without a baseline or speed-improvement claim.

[The raw JSON](2026-09-08-macos-milestone-backend.json) contains every warmup and measured sample, result fingerprints, exact inputs, fixture generator, per-source SHA-256 values, binary identity, hardware/load observations and before/after preservation checks. [The harness](../../crates/git-core/examples/macos_bench.rs) emits raw CSV and performs no repository writes.

## Build and conditions

- Source base: `5e9a0de260412ace4a29a335cc22832e8f992f64`, with the milestone's uncommitted changes. The JSON preserves the actual dirty status and hashes every Git-core source, the harness, workspace/core manifests and Cargo.lock. Combined source identity: `6890b7e2892109b68b813bd001d3b1bc564d582c30ef3dc562170c0ae44e7464`. Those inputs were identical before compilation and after measurement; this does not identify a later native package.
- Build: `cargo build --locked --release -p gitturtle-core --example macos_bench`. Executable SHA-256: `2250bc1de2a8fa7e89f1a73d52f2d9cf26e5bf8db207c2b695f559782ab9545a`.
- Hardware/software: Apple M4 Max, Mac16,5, 128 GiB memory, 16 logical processors; aarch64 macOS 26.6.2 / 25G83; Rust 1.98.0 (`88d9e12ae`, 2026-08-18); Git 2.50.1 / Apple Git-155.
- Timing: September 8 at 19:56:05 UTC, 13.429 seconds for the complete run. Each series has three warmups and 24 measured successful calls in sequence. There were no failed or missing samples in this completed run.
- Cache: one retained `GitRepository` and object reader, no application content cache. Filesystem caches were not flushed. Fixture setup, initial status, and the complete before snapshot warmed filesystem data; result validation and fingerprints ran after each timer stopped.
- Load: project compilation and native QA were paused during the completed run. Other processes remained active: one-minute load was 13.22 before and 13.11 after; the recorded `ps` observations include several busy Node processes and `mediaanalysisd`. This was not a quiescent or controlled-load experiment.

The median averages sorted ranks 12 and 13. Nearest-rank p95 is rank 23 and maximum rank 24. Tables round to three decimals; raw values and every outlier are retained.

## Fixture and results

The fresh `/private/tmp` fixture had 129 commits, 303 tracked files, 384 local tags (128 annotated), and 91 distinct status entries. HEAD was `d104b0b6027aa72c94a3619b464a5d0ce2f44e0d`. The selected `src/renamed.rs` had 512 committed lines; it was renamed from `src/original.rs` at `fd620d7ca244d283718df689cc468a6e10447478`. Its index and raw worktree contain separate edits, and the raw file has four appended lines. Working attribution returned 516 lines, six marked uncommitted. No writes occurred between measurements.

| Synchronous core operation | Median (ms) | p95 (ms) | Maximum (ms) | Observed result |
| --- | ---: | ---: | ---: | --- |
| Committed blame | 72.214 | 79.221 | 80.996 | 512 attributed lines |
| Raw working-file blame | 129.544 | 147.245 | 175.961 | 516 lines; six uncommitted |
| Line history, line 64 | 52.611 | 54.602 | 68.333 | 65 commits; not truncated |
| Local tag discovery | 32.623 | 49.645 | 52.967 | 384 tags; not truncated |
| Passive status | 43.315 | 50.051 | 50.837 | 91 entries |
| Shared file-ignore preparation | 57.427 | 58.940 | 58.958 | 23-byte literal rule; zero tracked targets |
| Local directory-ignore preparation | 84.744 | 89.659 | 98.052 | Nine-byte rule; one tracked neighbor |

Line history follows the implementation's first-parent lineage for the selected line, including this fixture's rename; the result does not establish every ancestry route or cross-file copy tracking. Tag timing covers discovery, not annotation inspection, creation, deletion or network transport. The ignore samples prepare `scratch/cache[1].tmp` for shared `.gitignore`, and its immediate `scratch/` directory for `.git/info/exclude`. The latter reports the tracked `scratch/keep.txt` neighbor. Neither plan was applied or staged.

Before and after inventories matched for all 2,005 entries: 1,729 regular files and 276 directories, including the worktree, index, refs, reflogs, configuration and Git object storage. The comparison covered kind, mode, exact bytes and modification time, excluding access time. Both inventory digests were `cc1ce7f5e67f85982b06cb9f1f26067b7167d38e5e39f9cad9c232ae79e9f11c`. Initial/final status and all 27 results within each series were also stable.

## Aborted attempt

An earlier run used a fixture beneath the project in Documents and stalled in descriptor-relative `openat` during ignore preparation. A stack sample showed the object reader idle and the main thread in `ignore_directory`; its traversal descriptor referenced the user's home directory. This is consistent with macOS protected-folder permission mediation for a new standalone executable, rather than a demonstrated object-reader deadlock. The process was terminated and its child exited. The same unchanged core/harness completed on a freshly generated `/private/tmp` fixture. No incomplete-run samples contribute to the table; the raw report records that attempt separately.

## Reproduction and limits

The JSON embeds the exact Python fixture generator, whose SHA-256 is `ea6f2c39275c84cee0d697762daac6ad579d65258295fbfad9e1507c6622d981`. Run it from a temporary script; it creates only a fresh repository under `/tmp` and prints the fixture directory:

```sh
python3 - <<'PY'
import json, pathlib, subprocess, tempfile
record = json.loads(pathlib.Path('docs/benchmarks/2026-09-08-macos-milestone-backend.json').read_text())
folder = pathlib.Path(tempfile.mkdtemp(prefix='gitturtle-benchmark-script-'))
script = folder / 'create-fixture.py'
script.write_text(record['fixture_generator_python'])
subprocess.run(['python3', str(script)], check=True)
PY
cargo build --locked --release -p gitturtle-core --example macos_bench
./target/release/examples/macos_bench /printed/fixture/directory/work \
  src/renamed.rs 'scratch/cache[1].tmp' 64 > /tmp/macos-backend-samples.csv
```

Capture source/binary identities, hardware/load and complete fixture inventories before and after when producing another report. Git timestamps/content in the generator are fixed, but filesystem identities and local fixture paths change; raw result fingerprints containing these values are not cross-run identity guarantees.

These timings end when the synchronous core API returns. They exclude queue delay, result validation/destruction, app attribution presentation, editor construction, GPU work, OS input delivery and displayed frames. They do not measure image comparison, authentication, mutation/cancellation latency, memory growth, Liquid Glass, older hardware or Linux. OS permission prompts and uninterruptible filesystem calls are outside per-Git-process deadlines. The recorded costs and small sample establish neither a responsiveness guarantee nor a speedup.
