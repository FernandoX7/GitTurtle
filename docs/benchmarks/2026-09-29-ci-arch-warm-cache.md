# CI Arch Linux warm cache — September 29, 2026

Evidence for task `ci-arch-warm-cache`: give `Rust tests · archlinux` the shared
Rust cache, keyed on the Arch packages that reach compiled output. The mechanism is
described in [docs/ci.md](../ci.md#arch-linux-container-job). This record holds the
**before** baseline and the pull request's own cold run. The `main` seed and a warm
pull-request run can only follow the merge, because only a push to `main` saves an
entry; they are added to this record once measured. Each figure is one observation,
not a distribution.

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

The setup step printed the keyed versions (`Keyed Arch packages: binutils 2.47-4
clang 22.1.8-1 … gcc 16.2.1+r23+gd564253eb6c8-1 … glibc 2.44+r50+g1848099f063e-1 …
zstd 1.5.7-3`) and the key `debug-archlinux-c81f411e…`. Upstream's cache key,
`gitturtle-rust-v2-debug-archlinux-c81f411e…-Linux-x64-89bb60e7-31ad3f95`,
resolved Rust `1.98.0 x86_64-unknown-linux-gnu 88d9e12a…` through `setpriv`. The
restore found nothing, the ownership step ran, and every test passed as `builder`.
The finish phase's bound and save steps were skipped, as they are on every pull
request.
