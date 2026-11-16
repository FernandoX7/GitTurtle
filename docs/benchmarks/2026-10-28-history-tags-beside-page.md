# History's tag read beside its history page, 2026-10-28

This is the release measurement owed by `7bb43ab` (`perf(app): read History's tags beside its history page`). That change moves the passive `git for-each-ref refs/tags/` that [the earlier record](2026-10-28-history-snapshot-tags.md) measured from after the history page to a scoped thread that runs while the worker reads the page. It uses the same driver, fixtures and percentiles as that record; the raw samples are in [`2026-10-28-history-snapshot-tags/`](2026-10-28-history-snapshot-tags/).

## Result

Pinned to the four performance cores 8–11, where the tag read has a core of its own:

| Fixture | Series | Base p50 / p95 / max (ms) | Candidate p50 / p95 / max (ms) | Δ p50 |
| --- | --- | ---: | ---: | ---: |
| 1,000 tags, loose refs | Load | 38.08 / 45.77 / 48.10 | 29.60 / 33.91 / 36.92 | −8.48 |
| 1,000 tags, loose refs | Refresh | 36.53 / 43.75 / 47.54 | 29.59 / 34.57 / 41.06 | −6.94 |
| 1,000 tags, packed refs | Load | 32.63 / 39.50 / 39.88 | 27.34 / 30.37 / 33.13 | −5.29 |
| 1,000 tags, packed refs | Refresh | 31.51 / 38.52 / 42.72 | 25.58 / 30.10 / 32.86 | −5.93 |
| No tags | Load | 21.26 / 25.05 / 29.34 | 21.03 / 24.55 / 26.86 | −0.23 |
| No tags | Refresh | 18.41 / 23.97 / 30.77 | 18.52 / 24.30 / 26.98 | +0.11 |

Pinned to core 8 alone, where the tag read and the page share one core:

| Fixture | Series | Base p50 / p95 / max (ms) | Candidate p50 / p95 / max (ms) | Δ p50 |
| --- | --- | ---: | ---: | ---: |
| 1,000 tags, loose refs | Load | 40.89 / 42.48 / 44.66 | 38.88 / 41.75 / 44.38 | −2.01 |
| 1,000 tags, loose refs | Refresh | 38.15 / 39.74 / 40.55 | 39.38 / 42.90 / 45.22 | +1.23 |
| 1,000 tags, packed refs | Load | 35.90 / 37.78 / 42.54 | 36.44 / 39.56 / 42.37 | +0.54 |
| 1,000 tags, packed refs | Refresh | 33.38 / 35.99 / 39.07 | 39.30 / 42.98 / 45.55 | +5.92 |
| No tags | Load | 24.96 / 26.20 / 27.78 | 24.39 / 25.93 / 26.93 | −0.57 |
| No tags | Refresh | 22.26 / 23.49 / 25.25 | 19.89 / 21.82 / 22.95 | −2.37 |

Each cell pools 120 samples from three rounds; every round of a build agrees with the others within 2.7 ms at p50 (`summary.txt` in each results directory). With a second core the change does what it set out to do: at 1,000 tags a launch, a new tab or a Refresh shows History 5.3 to 8.5 ms sooner, a sixth to a fifth of the read, and a repository without tags is unchanged. On one core there is nothing to overlap, and Refresh at 1,000 tags gets slower, by 5.9 ms on packed refs; there the candidate's Refresh is slower than its own Load, the reverse of every other cell. The likely cause, not traced here, is that the reused session's page read waits on Git through pipes, and each round trip now queues behind the concurrent `for-each-ref` on the same core. A desktop has more than one core, but a fully loaded host or a container limited to one CPU behaves like this cell, so the follow-up is to read the tags after the page when `std::thread::available_parallelism` reports one CPU, and measure core 8 again.

## Setup

- Host: Apple M2 Pro (4 efficiency and 8 performance cores), 31 GiB, Asahi Omarchy on Linux 7.1.12-2-7-ARCH, `apple-cpufreq` with the `schedutil` governor, on AC power, Git 2.55.0. The accessibility bus was off and nothing compiled. The editor hosting the session kept cores 8–11 80 to 90% idle and the one-minute load average between 1.7 and 2.7, which a run's own work raises for minutes, so [`bench.sh`](2026-10-28-history-snapshot-tags/bench.sh) now starts each run once nothing compiles and cores 8–11 were at least 80% idle over two seconds, and records that share, the load average and the four cores' frequencies at each run's start and end (`runs.txt`). Base and candidate alternate within every run (40 pairs), so a load that drifts slowly reaches both.
- Driver: the opt-in test `worker::snapshot_bench::read_snapshot_release_benchmark`, as in [the earlier record](2026-10-28-history-snapshot-tags.md#setup): `read_snapshot` for the whole repository at History's page size of 500 commits, "Load" with a new `RepositorySession` per read, "Refresh" reusing one, five warm-ups of each, then 40 alternating pairs.
- Fixtures: the 3,000-commit, 121-branch fixture of [the October 5 record](2026-10-05-history-columns-steady-after-toggle.md) (`/tmp/gitturtle-evidence/history-scroll/history`, HEAD `1e65ca0`, no tags); a copy with 1,000 tags on every third commit, half annotated, as loose refs; and that copy after `git pack-refs --all`.
- Builds: the candidate is `7bb43ab`; the base is `93a5c29`, which adds the driver to `45547f7`'s read. Both built with `cargo test --locked --release -p gitturtle --bin gitturtle --no-run` (sha256 base `091997d1…edd07`, candidate `1cbb4da4…15df5d`); each executable embeds its revision.
- Cache state: warm, as in the earlier record.

## Commands

```sh
CPUS=8 RESULTS=<dir>/results-8 docs/benchmarks/2026-10-28-history-snapshot-tags/bench.sh <dir>
CPUS=8-11 RESULTS=<dir>/results-8-11 docs/benchmarks/2026-10-28-history-snapshot-tags/bench.sh <dir>
python3 docs/benchmarks/2026-10-28-history-snapshot-tags/summary.py <dir>/results-8
```

with `<dir>/bench-base` and `<dir>/bench-candidate` built as in [the earlier record](2026-10-28-history-snapshot-tags.md#commands).

## Limits

The worker's read only, as before: History's acceptance, layout and draw are not timed. One host with a warm cache; the cores were not fully idle, though base and candidate shared the same conditions run by run. The single-core cause is inferred from the cell's shape, not traced.
