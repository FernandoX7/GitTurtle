# Selecting a PR review file with an 8 MiB patch, 2026-10-27

This is the release measurement for the `measure` criterion of `review-file-preparation-lifecycle`. It times the UI thread's part of selecting a file in the PR review's Files list, `Panel::select_review_file`, when each file's patch is just under the 8 MiB response bound. The base copies the selected `PullFile`, patch included, before handing it to the background preparation; the candidate's `PullFile::patch` is an `Arc<str>`, so the copy shares the view's allocation. The raw samples and the scripts are in [`2026-10-27-review-file-selection/`](2026-10-27-review-file-selection/).

## Result

| Series | Build | Samples | p50 (ms) | p95 (ms) | max (ms) |
| --- | --- | ---: | ---: | ---: | ---: |
| After the previous file finished preparing | base | 180 | 0.582 | 0.929 | 1.393 |
| After the previous file finished preparing | candidate | 180 | 0.216 | 0.232 | 0.261 |
| While the previous file is still preparing | base | 180 | 0.674 | 0.777 | 0.794 |
| While the previous file is still preparing | candidate | 180 | 0.000 | 0.001 | 0.002 |

The candidate is faster in every round, at every percentile, and an earlier set of the same three rounds, run on the change before its last revision (the close now remembers whether a preparation was pending), gave the same figures within 0.01 ms at p50; the three rounds of each build agree within 0.08 ms at p50 ([`results/summary.txt`](2026-10-27-review-file-selection/results/summary.txt)). What remains of the candidate's 0.22 ms after a prepared file is freeing the previous file's 16,001 inline rows, which `select_review_file` clears in both builds. While a file is still preparing it has no rows, and the candidate's selection takes under 2 µs; the base's 0.67 ms there is the 8.3 MB copy plus freeing the cancelled preparation's copy.

## Setup

- Host: Apple M2 Pro (4 efficiency and 8 performance cores), 31 GiB, Asahi Omarchy on Linux 7.1.12-2-7-ARCH, `apple-cpufreq` with the `schedutil` governor, on AC power. Every run was pinned to performance core 8 with `taskset -c 8`.
- Toolchain: Rust 1.99.0 (the repository's `rust-toolchain.toml`), `cargo test --release`, so the app's release profile (`lto = "thin"`, `codegen-units = 8`).
- Driver: the opt-in test `github_view::review::tests::select_review_file_release_benchmark`. It builds the review panel on a repository page as the view tests do, with two files whose patches are one hunk of 8,000 deletions and 8,000 additions of 520 bytes: 8,336,022 bytes and 16,001 inline rows each, above Compare's 2 MiB bound, so no Source presentation is prepared. It alternates the selection between the two files and times only the `select_review_file` call, from before it to its return. GPUI's effect flush, rendering and the background preparation are outside the timed region. In the first series the previous selection's preparation runs to completion, untimed, between samples, so each selection also frees the previous file's rows; in the second it never runs, so each selection cancels a pending preparation.
- Builds: the candidate is the change's source; the base is `30b8868`'s product code with the candidate's test module, so both run the identical driver. The diffs from `30b8868` that each benchmark executable was built from, and the executables, are kept in `.local/review-prep-2026-10-27/` on the measuring host (sha256 base `a4505a99324a0c031a9bf9b5f244e82aafb0e9d32ffaca06e072172ea1c25f60`, candidate `4322c1d0c39cdca3f3baf859891a134e801482a0299069954eed0e94f41163c9`).
- Sampling: five warm-ups and 60 samples per series per run; three rounds alternating base and candidate, each started once the one-minute load average was below 1.5 (1.49 here, just after the build) with no `cargo` or `rustc` running ([`results/runs.txt`](2026-10-27-review-file-selection/results/runs.txt)). Percentiles are by nearest rank over the 180 pooled samples.
- Cache state: warm. The patches are built in memory before the first sample; no file or Git read is involved.

## Commands

From the repository root, once per build, with the build's source in the working tree:

```sh
cargo test --locked --release -p gitturtle --bin gitturtle --no-run
# copy the printed target/release/deps/gitturtle-<hash> to .local/review-prep-2026-10-27/bench-<build>
```

Then [`bench.sh`](2026-10-27-review-file-selection/bench.sh) `.local/review-prep-2026-10-27` runs the three alternating rounds and [`summary.py`](2026-10-27-review-file-selection/summary.py) prints the per-round and pooled percentiles.

## Limits

This measures the selection handler in a GPUI test window, not a frame of the running app: the effect flush and the draw that follow the selection are the same in both builds and are not timed. It is one host and a warm cache; the two-core host the September records used was not measured.
