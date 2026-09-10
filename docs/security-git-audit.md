# Passive Git process ownership audit — September 10, 2026

This bounded audit covers the non-GitHub core process boundary. It supplements
the coordinated security milestone; it does not establish that every repository
operation is secure or that every workload is scalable.

## Threat model and acceptance scope

Repository paths, stored filenames, Git objects and configuration are untrusted.
Passive inspection must read local objects without executing configured helpers,
following stored symlinks, fetching missing objects or changing repository state.
Explicit writes retain the user's hooks, filters, signing and credential behavior.
The Git executable and deliberately configured executable programs already run
with the user's permissions; this audit does not turn those programs into a
sandbox or a security boundary against other same-account processes.

The concrete scope established during inspection was one process-ownership
weakness spanning three directly related consumers:

- [x] Retain passive deadlines after the direct child exits.
- [x] Stop and join passive stdout/stderr and history stdin threads when another
  session retains their descriptors.
- [x] Keep object-batch protocol invalidation and ordinary-history backpressure.
- [x] Preserve ordinary command/filter/hook/path semantics with existing fixtures.
- [x] Check comparable release latency for object reads, changed-file lists and
  ordinary history; retain raw samples and limitations.

`lib.rs::git_command` supplies passive Git policy and `bounded_output` owns a
single bounded command. `BatchReader` shares an object protocol behind the
repository handle's mutex. `history.rs` owns replaceable searches and retained
ordinary traversals. The new private
[`process_io.rs`](../crates/git-core/src/process_io.rs) supplies descriptor setup,
readiness waiting and explicit pipe-stop ownership to these existing consumers.
It does not select repositories, accept writes, or change Git command policy.

## Finding and correction

**GIT-1: passive deadlines and cancellation could wait indefinitely on inherited
pipes.** Previously, `bounded_output` stopped checking its deadline as soon as
the direct child exited, then joined blocking stdout/stderr readers. A child
holding either descriptor could therefore defer EOF arbitrarily. Killing the
process group did not help `BatchReader`, `HistoryStream`, or `stream_history`
when a descendant had started another session. A history stdin writer could
also remain blocked after the direct child and output streams had completed.
The impact is blocked background work, cancellation or shutdown. A Git wrapper
or executable descendant retaining descriptors triggers it; this audit did not
demonstrate repository content alone executing such a descendant.

The original implementation was reproduced with `/bin/sh -c 'sleep 0.3 & exit 0'`:
a requested 30 ms deadline returned successful output after approximately
310 ms. The newly added regression failed on the original source with an
unexpected successful `Output`.

The correction prepares nonblocking descriptors before starting threads and
watches both pipe readiness and an anonymous Unix socket. Stopping the shared
control closes one socket direction; the resulting EOF wakes every blocked
reader/writer without consuming a notification. Idle history readers have no
periodic polling wakeups. Completion requires the process and required I/O
threads to finish. Failure cleanup stops all owned pipes, terminates/reaps the
process group and joins all threads while retaining the deadline/cancellation
error. Normal batch reads still discard a process after a protocol failure;
history still drops the bounded receiver before joining a backpressured sender.

The existing `rustix` dependency enables its `event` feature for `poll`; no new
dependency or version was introduced. Explicit-write subprocess behavior is
unchanged.

## Executed behavioral evidence

On macOS arm64, these targeted commands passed on the corrected source:

```sh
cargo test --locked -p gitturtle-core --lib
cargo test --locked -p gitturtle-core --test repository --test history --test workflow --test preview_assets
```

The unit run passed 28 tests, including five new tests covering exited parents,
detached output holders in passive commands and object batches, detached output
and blocked-input holders in history, and traversal cancellation. The latter
cases assert return within two seconds for a detached fixture that would retain
its descriptor for five seconds, then explicitly clean up that fixture process.
Production cleanup does not claim ownership of independently daemonized programs.

The four integration suites passed 52 tests. Relevant existing cases check that
hostile configured helpers do not run during passive reads, status does not run
clean filters or refresh the index, explicit staging/commits preserve filters and
hooks, failed signing does not create an unsigned commit, object errors do not
poison later reads, and symlink/byte-safe paths retain their intended semantics.
Those fixtures inspect repository/index/working outcomes in disposable locations.
The coordinator owns the final combined workspace gates and platform evidence.

## Release comparison

The reproducible
[`bench-passive-pipes.py`](../scripts/bench-passive-pipes.py) harness builds
standalone copies of the baseline and corrected core, with thin LTO and eight
codegen units. The baseline is `b36efd036a4c61931844726ca526833aa1208098`.
Each build's source hashes, executable SHA-256, generated standalone lock hash,
harness and raw samples are in
[`security-git-20260910.json`](benchmarks/security-git-20260910.json).
The offline standalone builds prune unrelated workspace lock entries; final
workspace checks remain the coordinator's locked builds.

Hardware: Apple M4 Max, 128 GiB RAM; macOS arm64; Rust 1.98.0; Apple Git 2.50.1.
The same disposable fixture contains 100 linear commits with one 8,192-byte file
per commit. Each attempt has three warmup rounds and 40 measured rounds, ordered
baseline/corrected/corrected/baseline. The table aggregates 80 samples per variant.
OS caches were not flushed. Desktop/development background activity was
uncontrolled; recorded one-minute load averages ranged from 7.62 to 8.61.

| Operation | Baseline p50 / p95 / max (ms) | Corrected p50 / p95 / max (ms) |
| --- | --- | --- |
| 100 sequential 8 KiB blob reads | 1.767 / 6.181 / 69.273 | 1.840 / 2.602 / 17.998 |
| Changed-file list | 13.363 / 21.910 / 35.154 | 13.244 / 15.583 / 25.792 |
| 100-commit ordinary traversal | 13.234 / 21.383 / 22.723 | 12.861 / 17.000 / 27.151 |

These small backend observations show similar median latency on this fixture;
they do not establish a speed improvement. They exclude UI dispatch, rendering,
image decoding, GPU work and process RSS/CPU profiling. Cancellation tests prove
the exercised return/cleanup behavior rather than production latency bounds.

## Residual risks and verification limits

- A configured executable can independently daemonize or have external side
  effects. Cleanup terminates Git's process group and joins GitTurtle's own
  threads; it does not sandbox or kill arbitrary same-account processes.
- This pipe implementation applies to macOS/Linux Unix descriptors. Non-Unix
  builds retain their prior blocking-pipe limitation; no Windows support is
  claimed. Linux execution and native interaction belong to the combined record.
- Per-request deadlines do not bound an entire multi-command interaction or
  uninterruptible filesystem I/O. Native responsiveness and shutdown require the
  coordinator's packaged-app session.
- Existing prepared-write guards check operation-specific paths and captured
  refs/index/content. They are not an atomic transaction against an external
  same-account process replacing an entire repository during a sequence of
  commands. The coordinated app audit separately addresses retained-session
  reuse after a repository is replaced at the same pathname.
- This is a finite process/path/policy review with regression fixtures, not
  exhaustive fuzzing, a full Git dependency audit, hosted-provider verification,
  or a claim of universal scalability.
