# Repository identity check, 2026-11-09

Release measurement of the per-read and per-write check that a repository still is the one a tab opened (`GitRepository::verify_at_path`), before the check gained the resolution of a repository's stored paths and the guard chosen by the file Git reads.

## Host and procedure

The Asahi development host (AArch64 Apple M2 Pro, 12 cores, Linux 7.2.11, btrfs on `/home`, Git 2.56.0, rustc 1.99.0). A release binary built outside the workspace against `gitturtle-core` at each revision: `1e2ad7da` (before the change) and `3afd9e14` (the reviewed tip, whose fast path the commits as landed keep). Five runs of each revision alternating, each timing 20,000 calls per layout after 1,000 calls of warm-up, and 500 spawns of `git --version` as a yardstick. Fixtures were created fresh on btrfs for each run, so the page cache is warm for the check itself. New builds were held and the host measured 4 % busy before the first run. Each figure below is the median across the five runs of that run's percentile.

## Result

Microseconds per check.

| Layout | p50 before | p50 after | p99 before | p99 after | p99.9 before | p99.9 after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Plain repository | 7.1 | 7.0 | 11.7 | 11.6 | 16.8 | 16.1 |
| Linked worktree | 10.0 | 14.0 | 15.1 | 19.6 | 18.5 | 23.2 |
| `core.worktree` a folder inside | 7.8 | 10.4 | 11.0 | 15.5 | 15.8 | 18.6 |
| `commondir` and `core.worktree` through links | 10.2 | 20.0 | 14.6 | 27.2 | 18.3 | 31.0 |

One Git process costs about 950 µs at p50 and 1,530 µs at p99 on this host.

A plain repository, the common case, is unchanged. The added cost is one path resolution per stored entry: about 4 µs for a linked worktree and 10 µs for the deepest layout, about 1 % of one Git process, and no layout starts a process on the fast path. Run maxima varied from one run to the next at every layout (a linked worktree's tip maxima 31 to 397 µs, a base run 27 to 29 µs, the through-links base 23 to 516 µs) and are not treated as a difference.

Not measured: macOS, and a repository on a network mount, where each added resolution is a round trip.
