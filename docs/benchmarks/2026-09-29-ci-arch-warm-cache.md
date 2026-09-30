# CI Arch Linux warm cache — September 29, 2026

Evidence for task `ci-arch-warm-cache`: give `Rust tests · archlinux` the shared
Rust cache, keyed on the Arch packages that reach compiled output. The mechanism is
described in [docs/ci.md](../ci.md#arch-linux-container-job). This record holds the
**before** baseline, the pull request's own cold runs, the `main` seed and the first
warm pull-request run. Only a push to `main` saves an entry, so the seed and the warm
run followed the merge of #92. Each figure is one observation, not a distribution.

## Measurement boundary

- **Job**: the job's `started_at` to `completed_at` from
  `gh api repos/FernandoX7/GitTurtle/actions/runs/<run>/jobs`, including container
  start, setup, restore, tests and the post-job cache action.
- **Tests**: the `Locked workspace tests with nextest` step's interval from the same
  API. It compiles every stale unit, then runs the tests.
- **Setup**: the `Prepare Rust and restore bounded dependencies` step, which keys
  the cache and restores it (with a miss, it only keys).
- **Cache sizes**: the cache service's `sizeInBytes` from `gh cache list`; the
  repository total from `gh api repos/FernandoX7/GitTurtle/actions/cache/usage`,
  against the 10 GB repository limit.

## Before: no Arch cache

Job and nextest-step durations in seconds. The Ubuntu and macOS debug jobs are
`Rust tests and Clippy`; they restore their own caches (warm unless marked cold).

| Run | Event | Head | Arch job | Arch tests | Ubuntu debug job | macOS debug job |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| [36599383720](https://github.com/FernandoX7/GitTurtle/actions/runs/36599383720) | push `main` | `d553cc0` | 943 | 898 | 193 | 278 |
| [36608513381](https://github.com/FernandoX7/GitTurtle/actions/runs/36608513381) | pull request | `b043cd4` | 947 | 903 | 206 | 347 |
| [36618045403](https://github.com/FernandoX7/GitTurtle/actions/runs/36618045403) | pull request | `7136265` | 945 | 896 | 193 | 343 |
| [36620122201](https://github.com/FernandoX7/GitTurtle/actions/runs/36620122201) | push `main` | `3ac967d` | 947 | 900 | 1290 (cold) | 303 |
| [36622842027](https://github.com/FernandoX7/GitTurtle/actions/runs/36622842027) | pull request | `9e44082` | 947 | 902 | 1230 (cold) | 1207 (cold) |
| [36639649705](https://github.com/FernandoX7/GitTurtle/actions/runs/36639649705) | push `main` | `8089625` | 735 | 690 | 202 | 323 |

On every run where both debug jobs were warm, the Arch job took 412 to 665 s longer
than the slower of them, so it set the pull request's time to a green gate. In run
36622842027 the container start took 16 s, the upgrade and install 10 s and the
toolchain 6 s; the nextest step, almost all of it dependency compilation, took 902 s.

## Cache inventory before the first Arch entry

At 22:48 UTC on September 29, before any Arch entry existed, the repository held 11
entries, 11.36 GB, against the 10 GB limit. One entry per lane is about 1.42 GB
(Linux debug), 1.04 GB (macOS debug), 0.86 GB (Linux release) and 0.79 GB (macOS
release), 4.11 GB in all. The 11 entries were three Linux debug keys, four Linux
release keys, two macOS debug keys and two macOS release keys, all saved by `main`
pushes between 16:18 and 21:28 UTC. No keyed source changed between those pushes
(`git diff d553cc0 8089625` over the manifests, lockfile, build scripts, `vendor/`,
toolchain file and setup action is empty), so the keys moved with the runners'
native inventories, most likely because runners on two hosted images took jobs
during a rollout, each with its own `dpkg` or Xcode identity. GitHub evicts by last
access once the total is over the limit, so superseded generations go before the
entries that current runs restore.

## This pull request (cold)

The branch changes the setup action, whose files are part of every lane's key, so
every lane of the pull request's run misses and builds cold; the pull request saves
nothing.

| Run | Event | Head | Arch job | Arch tests | Arch setup | Result |
| --- | --- | --- | ---: | ---: | ---: | --- |
| [36641354890](https://github.com/FernandoX7/GitTurtle/actions/runs/36641354890) | pull request #92 | `eb9b487` | 927 | 878 | 2 | miss (`No cache found.`), passed |
| [36644238670](https://github.com/FernandoX7/GitTurtle/actions/runs/36644238670) | pull request #92 | `fec42c7` | 958 | 910 | 2 | miss, passed |

The second row is the merged head, after the review changed the keyed package list. The setup step of the first printed the keyed versions (`Keyed Arch packages: binutils 2.47-4
clang 22.1.8-1 … gcc 16.2.1+r23+gd564253eb6c8-1 … glibc 2.44+r50+g1848099f063e-1 …
zstd 1.5.7-3`) and the key `debug-archlinux-c81f411e…`. Upstream's cache key,
`gitturtle-rust-v2-debug-archlinux-c81f411e…-Linux-x64-89bb60e7-31ad3f95`,
resolved Rust `1.98.0 x86_64-unknown-linux-gnu 88d9e12a…` through `setpriv`. The
restore found nothing, the ownership step ran, and every test passed as `builder`.
The finish phase's bound and save steps were skipped, as they are on every pull
request.

## Seed: the first `main` push

#92 merged as `d274147` at 23:38 UTC. GitHub created that push's run only at 23:48:52, two seconds before the run for the next merge, `de09c65` (#93), so two `main` runs seeded at once with the same Arch key.

| Run | Head | Arch job | Arch tests | Compile | Bound | Save (post) | Result |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| [36647160203](https://github.com/FernandoX7/GitTurtle/actions/runs/36647160203) | `d274147` | 970 | 893 | 827.7 | 11 | 22 | saved `debug-archlinux-6959537f…` |
| [36647162498](https://github.com/FernandoX7/GitTurtle/actions/runs/36647162498) | `de09c65` | 974 | 918 | — | 11 | 0 | exact entry found, no save |

The compile figure is the nextest step's `--timings` total: 912 units, none fresh. The helper's budget record (`rust-cache-budget.json`) for the saving run:

- limit 7,516,192,768 bytes (7 GiB);
- 6,055,353,835 bytes before cleanup and 4,728,790,982 retained;
- the four vendored packages kept (315,815,511 bytes);
- no dependency evicted, 879 extracted source directories pruned;
- no target fallback, and the save was registered.

Its stages took 3.5 s (metadata), 1.2 s and 1.1 s (the two scans), 0.5 s (member cleanup) and 3.1 s (source pruning). Upstream's post step then saved the archive in 22 s. The cache service lists the entry at **1,083,913,297 bytes** (1.08 GB), created at 00:05:30 UTC on September 30. The second run's finish looked the key up at 00:06:12, found it, and registered no save.

## Warm: the first product pull request

[#94](https://github.com/FernandoX7/GitTurtle/pull/94) (`a6e6205`) changes `crates/app/src/repository_tabs.rs` and docs. It leaves the lockfile, `vendor/`, the toolchain and the setup action unchanged.

| Run | Lane | Job | Tests | Setup |
| --- | --- | ---: | ---: | ---: |
| [36648691987](https://github.com/FernandoX7/GitTurtle/actions/runs/36648691987) | Rust tests · archlinux | 182 | 104 | 26 |
| | Rust tests and Clippy · macos-15 | 285 | 196 | 37 |
| | Rust tests and Clippy · ubuntu-24.04 | 189 | 115 | 43 |

- **Restore:** an exact hit. The 1,083,913,297-byte archive downloaded in about 6 s, at 92 to 179 MB/s.
- **Setup step:** 26 s, which includes extraction and the ownership hand-off. The helper's restore record measured 24.9 s from its start marker (`hit: true`, `vendor_mtimes: 743`).
- **`--timings`:** 873 of 912 units fresh; the nextest step compiled for 46.1 s. The log's only `Compiling` lines are the three workspace members: `gitturtle`, `gitturtle-core` and `gitturtle-preview`.
- **The contract's bound:** the Arch job took 182 s, 103 s less than the slowest warm `Rust tests and Clippy` job (macOS, 285 s) of the same run. The bound was to finish within 2 minutes of that job.
- **Where the time went:** the container start (15 s), the pacman upgrade and install (10 s), checkout and toolchain (15 s) and the restore (26 s) remain; the tests step fell from 878 to 918 s to 104 s.

## Cache inventory after the seed and the warm run

At 00:14 UTC on September 30 the repository held 11 entries, 11.58 GB. Between 22:48 and 00:14, GitHub evicted the seven oldest entries (created 16:18 to 19:55, last accessed by 20:19 UTC) and kept every entry accessed later. The Arch entry, 1,083,913,297 bytes, was last accessed at 00:08:19 by #94's run, which restored the current Linux and macOS entries too, all warm.

| Entries | Size | Last accessed | Role |
| --- | ---: | --- | --- |
| Arch debug | 1.08 GB | 00:08 | current |
| Linux debug, two keys (`fc3c393a…`, `2be386be…`) | 2 × 1.42 GB | 00:08 and 00:10 | current, one per runner image |
| Linux release, two keys (`8f34a534…`, `10f8769b…`) | 2 × 0.86 GB | 00:08 and 00:00 | current, one per runner image |
| macOS debug and release | 1.04 + 0.79 GB | 00:14 and 00:08 | current |
| the four lanes before #92 | 4.11 GB | 22:31 to 22:36 | superseded: #92 changed the setup action, which every key hashes |

The superseded 4.11 GB are the least recently used, so the next eviction takes them, leaving the 7.47 GB that current runs restore. Each extra Ubuntu image in service adds 2.28 GB (a debug and a release entry) and each extra macOS image 1.83 GB. A third Ubuntu image would bring the set to 9.75 GB. That image together with a macOS rollout would pass the 10 GB limit and evict entries that current runs still restore. The Arch entry is 1.08 GB of that set. The Ubuntu lanes' `dpkg` inventory key produced one entry per runner image in both inventories here, with no keyed source change.
