# History's snapshot read with tags, 2026-10-28

This is the release measurement owed by `45547f7` (`feat(app): show tags in History's references`). That change adds one passive `git for-each-ref refs/tags/` to every History snapshot, on the worker, and names each commit's tags after its branches (`worker::HistoryReferences`). The snapshot read, `worker::read_snapshot`, is what a launch or a new tab runs to show History, and what an explicit Refresh runs again; its duration is the "Local snapshot · N ms" the status bar reports. The raw samples and the scripts are in [`2026-10-28-history-snapshot-tags/`](2026-10-28-history-snapshot-tags/).

## Result

| Fixture | Series | Base p50 / p95 / max (ms) | Candidate p50 / p95 / max (ms) | Δ p50 |
| --- | --- | ---: | ---: | ---: |
| 1,000 tags, loose refs | Load | 31.74 / 34.78 / 36.10 | 40.59 / 43.92 / 48.92 | +8.85 |
| 1,000 tags, loose refs | Refresh | 28.97 / 30.79 / 31.81 | 37.68 / 40.28 / 42.07 | +8.71 |
| 1,000 tags, packed refs | Load | 29.09 / 30.97 / 31.60 | 35.83 / 37.93 / 39.59 | +6.74 |
| 1,000 tags, packed refs | Refresh | 26.36 / 28.28 / 30.30 | 33.30 / 35.72 / 38.54 | +6.94 |
| No tags | Load | 23.71 / 24.98 / 25.90 | 24.89 / 26.29 / 27.51 | +1.18 |
| No tags | Refresh | 19.88 / 21.74 / 22.51 | 21.98 / 23.52 / 23.98 | +2.10 |

Each cell pools 120 samples from three rounds; the rounds of each build agree within 1.1 ms at p50 ([`results/summary.txt`](2026-10-28-history-snapshot-tags/results/summary.txt)). The candidate is slower in every round. Without tags the cost is the extra process, 1 to 2 ms. With 1,000 tags it is 7 ms on packed refs, as a clone or `git gc` leaves them, and 9 ms on loose refs: a quarter of the read. The tag read runs after the history page and waits for it, although it needs nothing from it, so overlapping the two is the follow-up this record leaves open.

## Setup

- Host: Apple M2 Pro (4 efficiency and 8 performance cores), 31 GiB, Asahi Omarchy on Linux 7.1.12-2-7-ARCH, `apple-cpufreq` with the `schedutil` governor, on AC power, Git 2.55.0. Every run was pinned to performance core 8 with `taskset -c 8`; its frequency at each run's start and end is in [`results/runs.txt`](2026-10-28-history-snapshot-tags/results/runs.txt). The accessibility bus was off and nothing else was building; each run started once the one-minute load average was below 1.5.
- Driver: the opt-in test `worker::snapshot_bench::read_snapshot_release_benchmark`. It times `read_snapshot` for the whole repository at History's page size of 500 commits, from before the call to its return, freeing the snapshot outside the timed region. "Load" gives each read a new `RepositorySession`, as a launch or a new tab does; "Refresh" reuses one session, as an explicit Refresh in the window does. Five warm-ups of each, then 40 alternating pairs.
- Fixtures: the 3,000-commit, 121-branch fixture of [the October 5 record](2026-10-05-history-columns-steady-after-toggle.md) (`/tmp/gitturtle-evidence/history-scroll/history`, HEAD `1e65ca0`, no tags); a copy with 1,000 tags on every third commit, half annotated, as loose refs; and that copy after `git pack-refs --all`.
- Builds: the candidate is `45547f7`; the base is its parent `1832fa2`. Both carry the driver and nothing else, built with `cargo test --locked --release -p gitturtle --bin gitturtle --no-run` (sha256 base `c0943af7…a387c4`, candidate `fc6504e2…dcd67`); each executable embeds its revision.
- Cache state: warm. The warm-ups read the fixture into the page cache, and every sample reads the same objects.

## Commands

With each build's source plus the driver checked out:

```sh
cargo test --locked --release -p gitturtle --bin gitturtle --no-run
# copy the printed target/release/deps/gitturtle-<hash> to <dir>/bench-base or <dir>/bench-candidate
```

Then [`bench.sh`](2026-10-28-history-snapshot-tags/bench.sh) `<dir>` runs three rounds alternating base and candidate on each fixture, and [`summary.py`](2026-10-28-history-snapshot-tags/summary.py) `<dir>/results` prints the per-round and pooled percentiles (nearest rank).

## Limits

This times the worker's read, not the frame that shows History: the UI's acceptance of the snapshot, the row layout and the draw are not timed, and History's rows now also draw tag chips. It is one host with a warm cache and one fixture shape; a repository with tens of thousands of tags was not measured.
