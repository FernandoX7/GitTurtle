# Desktop code font at launch, cold and warm fontconfig caches, base against candidate, 2026-10-04

This is the release measurement for `code-font-cold-launch-wait`, attempt 1: candidate `effdb2c` against its same-session base `b5b6dad`. It times launch to first window, and launch to the end of the code font's family lookup, with **Use the desktop's monospace font** on, a cold and a warm fontconfig cache, on this Linux host under XWayland. The driver, `2026-10-04-code-font-cold-launch-wait/cold_launch_x11.py` (sha256 `7b0c01ee…`), lands beside this record in its own commit, because the controller's evidence commits hold no code; and every raw sample, call and cache count is in [`results/run-1.json`](2026-10-04-code-font-cold-launch-wait/results/run-1.json) (sha256 `c789c1bc…`). The method follows the [September 29 record](2026-09-29-code-font-cold-launch.md), with an X11 window stamp in place of Hyprland's.

## What changed

The base kills the `monospace` lookup at its 500 ms `FAMILY_WAIT`, and only a later window activation looks again. The candidate keeps the lookup running under a 4 s bound, reads its pipe on the executor's timer, and runs one `fc-match` at a time. Nothing else in launch changed.

## What the driver measures

Every launch runs `taskset -c 0-7 BINARY FIXTURE` with the setting on, a fresh HOME and XDG directories, the `GitTurtle QA` identity, `TZ=UTC`, Midnight, `DISPLAY=:0`, `GPUI_X11_SCALE_FACTOR=1`, and `WAYLAND_DISPLAY`, `GIT_*` and an inherited `FONTCONFIG_FILE` removed. The driver checked that each app process had affinity 0–7. Because every XDG cache is new, Mesa's shader cache and the app's own caches are cold in every launch of every cell. "Warm" means a warm fontconfig cache only.

Two fontconfig states:
- **cold:** `FONTCONFIG_FILE` names a copy of the host's `/etc/fonts/fonts.conf`. In the copy the `conf.d` include is made absolute, and the only `<cachedir>` is the launch's `XDG_CACHE_HOME/fontconfig`. The driver asserted that directory absent before each launch, so `fc-match` scans every font directory. A fresh HOME alone stays warm on this host, because `/var/cache/fontconfig` covers every font directory.
- **warm:** the stock configuration and its `/var/cache/fontconfig` (30 files, 1,438,272 bytes).

The kernel page cache was not dropped. The fonts had been read by the native round and the pilots minutes earlier, so their files were most likely resident; this was not checked.

Timestamps are microseconds of `CLOCK_REALTIME`, from `t0`, the driver's stamp just before `Popen`. So every value includes Python's fork and exec and `taskset`'s exec. Trace boundaries:
- **window:** `t0` to the driver's receipt of the X `MapNotify` of the app's top-level window (`_NET_WM_PID` equal to the launch's PID). GPUI maps the window at the end of `Window::new`, before its first frame. The value is not on the output's frame grid: it waits for no frame callback.
- **frame:** `t0` to receipt of the second X `DamageNotify` (raw rectangles) on that window. Composite damages the whole window when it maps, and that first event coincides with the map. The window has no background pixel, so the next damage is the app's first presented frame copied into the window's pixmap. Xwayland paces presents by Mutter's frame callbacks on the 137.95 Hz output (a 7.25 ms tick), so frame minus window (5.2 to 11.9 ms here) contains up to one tick. Mutter's composite and scanout, up to one more tick, are excluded.
- **family:** `t0` to the end of the first `monospace` call that exits 0, logged with bash's `EPOCHREALTIME` by an `fc-match` wrapper first on `PATH`. It is a lower bound for the family's application. It excludes the app's read of the pipe, the family check on the worker, the hop to the UI thread and the frame that draws the family. It waits for no frame. The wrapper runs `fc-match` under `setpriv --pdeathsig KILL`, as on September 29, and it also logs each call's output.
- **activation:** receipt of the root `_NET_ACTIVE_WINDOW` change that names the app's window, recorded beside the others.

"Applied" means the launch had a `monospace` call that exited 0, was not killed, and answered `DejaVu Sans Mono`. That family is also the bundled one, so the toolkit has it loaded. Nothing toggled the setting, so no generation changed. The frame that applies the family is not observed; the [native round](../validation.md#october-4-the-desktop-code-font-row-waits-for-a-cold-fontconfig-lookup) shows the row naming the family after a cold launch in both builds.

Each round runs the four cells (build × cache) once, in an order that rotates by round and starts round 1 with candidate, cold. Before the rounds, one warm launch of each build ran and is kept apart (`warmup` in the results). Each launch waited at least 3 s after the window, then until no `fc-match` call ran and a `monospace` call had exited 0, up to 12 s after the window. Then it was stopped with SIGTERM, and every launch exited on it. Percentiles are by nearest rank: with 24 samples, p50 is the 12th smallest and p95 the 23rd.

## Setup

- **Builds:** designated in writing by the coordinator at about 17:17 UTC, before any launch of this round.
  - Base `b5b6dad`, the candidate's parent: `.local/evidence/shared-base/gitturtle-base-b5b6dad`, sha256 `12861797036bde8dcecd9e6511916162b8048c8d5976fd7eb51417efa1f92943`.
  - Candidate `effdb2c`: `.local/evidence/code-font-cold-launch-wait/gitturtle-cand-effdb2c`, sha256 `1018c6836e987429ed8200066a25dffdbd0093c5cac57df4f41a92bb8c197fb5`.
  - Both are release builds of clean trees, `x86_64-unknown-linux-gnu`, rustc 1.99.0, by `--build-info`. `qa.py identity` checked the pair earlier the same day.
- **Host:** Intel Core i9-10900K, 10 cores and 20 threads, 61 GiB. CPUs 0–9 are the ten physical cores and 10–19 their SMT siblings, so `taskset -c 0-7` holds eight distinct cores; this CPU has no E-cores. `intel_pstate` active, `powersave` governor, energy preference `performance`, turbo on. Each launch's `cpu MHz` snapshot of CPUs 0–7 at spawn read 800 to 5,328 MHz, median 800: idle cores, not the frequency under load, which was not recorded.
- **Software:** Ubuntu 26.04.1 LTS, Linux 7.0.0-34-generic, GNOME Shell 50.1 (Wayland session), Xwayland 24.1.10 on `:0` at scale factor 1, a 3840 × 2160 output at 137.95 Hz, fontconfig 2.17.1.
- **Fixture:** `/tmp/gitturtle-evidence/fixtures/code-font-cold-launch-wait/repo`, the task scenario's recipe: one commit under the QA identity, built by `qa.py scenario fixture`.
- **Quiet host:** the unattended loop was idle, nothing was building, and the owner's own GitTurtle (PID 1448889) stayed open and idle. `qa.py display-check` found the desktop unlocked and no other QA process at 17:15:34, and the same at 17:30:39 after the run. No input was sent.
- **Run:** 17:20:02 to 17:28:04 UTC, 98 launches. The one-minute load average before each launch was 0.00 to 0.79, above 0.5 only in rounds 11 to 13.
- **Pilots, not recorded:** 8 launches from 17:18:36 to 17:19:46 UTC validated the driver. They are not in the results.

## Commands

On an X11 display (XWayland), with the QA virtual environment's python-xlib, from the repository root:

```sh
.local/qa-venv/bin/python3 docs/benchmarks/2026-10-04-code-font-cold-launch-wait/cold_launch_x11.py \
  BASE_BINARY CAND_BINARY FIXTURE OUT.json 24
```

The results file has the binaries' basenames in place of their local paths.

## Result

Milliseconds from spawn, p50 / p95 / max, 24 launches per cell, all pinned to CPUs 0–7:

| Cell | Window (map) | First frame | Family (lookup end) | Applied |
| --- | ---: | ---: | ---: | ---: |
| base, cold | 493.7 / 512.9 / 514.0 | 501.5 / 518.5 / 521.2 | 364.0 / 368.8 / 369.8 | 24 of 24 |
| candidate, cold | 492.8 / 508.7 / 519.0 | 498.3 / 517.3 / 525.9 | 365.7 / 367.0 / 377.2 | 24 of 24 |
| base, warm | 485.5 / 495.7 / 499.9 | 495.6 / 502.6 / 511.1 | 21.9 / 22.7 / 22.7 | 24 of 24 |
| candidate, warm | 488.5 / 505.1 / 514.2 | 495.5 / 511.6 / 520.5 | 21.9 / 22.4 / 22.6 | 24 of 24 |

Paired by round, candidate minus base, median (range), and rounds where the candidate was slower:

| Cache | Window | First frame | Family |
| --- | ---: | ---: | ---: |
| cold | +4.8 (−20.2 to +16.7), 14 of 24 | +3.1 (−20.3 to +17.0), 13 of 24 | +1.1 (−5.6 to +12.6), 16 of 24 |
| warm | +2.2 (−16.3 to +24.3), 14 of 24 | +0.1 (−20.1 to +24.9), 12 of 24 | −0.1 (−1.0 to +0.7), 11 of 24 |

### Noise

Within one cell the window ranges over 25 to 46 ms (warm base 475.2 to 499.9 ms; warm candidate 468.3 to 514.2 ms), and the paired differences run from about −20 to +25 ms. The differences in p50 (−3.2 to +3.0 ms for window and frame) are under a tenth of that spread, and the sign counts (12 to 14 of 24) are what chance gives. The warm family lookup is tight, 21.0 to 22.7 ms in both builds, and its p50 is equal. The cold family's p50 is 1.7 ms later in the candidate, with 16 of 24 rounds slower. That is inside the base's own 360.6 to 369.8 ms range, and the candidate's 377.2 ms maximum is one sample (round 16). The call starts 14.7 to 16.5 ms after spawn in both builds, and the stamp ends at `fc-match`'s exit, before the app reads anything. So nothing the candidate changed is inside this interval, and the difference is taken as noise.

### Raw samples

Milliseconds from spawn, by round; each round's four launches ran within about 20 s. Every call, its output, the activation, the damage events and the cache counts are in the results file.

With a cold cache:

| Round | Base window | Base frame | Base family | Candidate window | Candidate frame | Candidate family |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 499.2 | 505.5 | 363.5 | 486.7 | 493.1 | 366.8 |
| 2 | 496.1 | 501.9 | 365.6 | 483.9 | 489.3 | 363.8 |
| 3 | 512.9 | 518.5 | 364.0 | 492.8 | 498.2 | 366.0 |
| 4 | 514.0 | 521.2 | 362.9 | 503.0 | 508.3 | 365.7 |
| 5 | 507.2 | 512.9 | 365.8 | 487.4 | 493.6 | 363.9 |
| 6 | 497.8 | 503.2 | 364.0 | 488.9 | 496.9 | 363.3 |
| 7 | 488.4 | 499.6 | 364.9 | 492.9 | 498.3 | 363.3 |
| 8 | 490.6 | 497.3 | 363.0 | 499.9 | 506.2 | 366.3 |
| 9 | 492.8 | 499.0 | 363.7 | 490.8 | 496.8 | 364.7 |
| 10 | 491.7 | 497.5 | 364.5 | 504.5 | 512.0 | 362.6 |
| 11 | 498.7 | 504.1 | 368.8 | 503.9 | 509.7 | 363.3 |
| 12 | 484.4 | 490.8 | 364.0 | 490.1 | 497.4 | 363.3 |
| 13 | 502.3 | 508.9 | 369.8 | 519.0 | 525.9 | 366.3 |
| 14 | 479.4 | 484.8 | 361.6 | 489.8 | 496.3 | 366.4 |
| 15 | 495.8 | 503.1 | 364.3 | 485.3 | 490.6 | 365.4 |
| 16 | 483.0 | 492.2 | 364.6 | 492.1 | 499.0 | 377.2 |
| 17 | 492.6 | 501.5 | 363.2 | 498.5 | 505.7 | 367.0 |
| 18 | 479.5 | 485.3 | 360.6 | 495.9 | 501.4 | 366.9 |
| 19 | 493.7 | 500.3 | 365.4 | 485.5 | 491.6 | 365.9 |
| 20 | 486.2 | 491.9 | 362.2 | 488.6 | 494.0 | 364.2 |
| 21 | 500.7 | 506.2 | 363.0 | 508.7 | 517.3 | 363.4 |
| 22 | 495.0 | 502.3 | 361.4 | 504.6 | 513.4 | 365.7 |
| 23 | 511.7 | 517.2 | 365.5 | 497.8 | 503.1 | 366.3 |
| 24 | 490.7 | 496.5 | 361.3 | 496.6 | 502.3 | 366.0 |

With a warm cache:

| Round | Base window | Base frame | Base family | Candidate window | Candidate frame | Candidate family |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 490.0 | 495.6 | 21.8 | 514.2 | 520.5 | 21.3 |
| 2 | 482.6 | 488.1 | 21.9 | 497.8 | 503.6 | 21.7 |
| 3 | 490.5 | 495.7 | 21.7 | 486.5 | 492.0 | 22.2 |
| 4 | 490.8 | 496.3 | 22.3 | 487.3 | 494.4 | 22.1 |
| 5 | 484.6 | 491.1 | 22.0 | 468.3 | 473.9 | 21.0 |
| 6 | 475.2 | 481.8 | 21.5 | 484.1 | 489.6 | 21.4 |
| 7 | 489.3 | 498.8 | 22.1 | 490.4 | 496.9 | 22.1 |
| 8 | 481.4 | 486.9 | 22.2 | 495.3 | 500.8 | 21.8 |
| 9 | 495.1 | 502.6 | 22.7 | 495.8 | 501.3 | 22.3 |
| 10 | 490.6 | 497.3 | 21.7 | 497.0 | 502.3 | 22.4 |
| 11 | 489.0 | 498.0 | 21.6 | 486.6 | 491.9 | 21.9 |
| 12 | 479.0 | 485.9 | 21.5 | 490.2 | 496.8 | 21.9 |
| 13 | 480.5 | 487.4 | 22.6 | 478.4 | 484.6 | 22.0 |
| 14 | 483.1 | 488.9 | 21.4 | 493.5 | 501.9 | 21.9 |
| 15 | 484.1 | 493.9 | 21.5 | 505.1 | 511.6 | 21.8 |
| 16 | 499.9 | 511.1 | 21.2 | 485.6 | 491.0 | 21.7 |
| 17 | 485.5 | 497.4 | 22.1 | 490.7 | 499.0 | 21.5 |
| 18 | 476.3 | 483.8 | 21.7 | 479.6 | 485.3 | 21.8 |
| 19 | 482.6 | 489.2 | 22.4 | 500.0 | 505.7 | 22.1 |
| 20 | 493.3 | 500.3 | 22.1 | 482.0 | 487.2 | 21.9 |
| 21 | 493.1 | 499.2 | 21.7 | 488.5 | 495.5 | 21.4 |
| 22 | 483.6 | 491.7 | 21.9 | 490.6 | 496.4 | 22.3 |
| 23 | 492.5 | 498.1 | 22.7 | 481.2 | 487.0 | 21.8 |
| 24 | 495.7 | 501.2 | 21.9 | 488.3 | 493.8 | 22.6 |

Warm-ups, excluded: base window 505.4, frame 512.0, family 21.7; candidate window 482.4, frame 488.7, family 21.4. Both applied the family.

### The lookup on this host

In all 96 launches the app made two `monospace` calls, and none was killed:
- **The first** started 14.7 to 16.5 ms after spawn. With a cold cache it ended 360.6 to 377.2 ms after spawn, within the base's 500 ms bound, and it wrote 30 cache files (1,438,272 bytes) to the launch's own cache directory, the same in all 48 cold launches. With a warm cache it ended at 21.0 to 22.7 ms and wrote nothing.
- **The second**, requested by the window's activation, started 5.3 to 13.2 ms after the activation, which came 0.2 to 0.6 ms after the map. It took 6.1 to 10.0 ms, cold or warm, because the first call had written the cache.

The first call ended before the window mapped in every launch. Its cold scan here takes about 365 ms, against September 29's more than 500 ms on a two-core AMD 3020e, so the base's bound never ran out in this run, and the base applied the family in all 24 cold launches as well.

## For its own task

The criterion's claims, on this host:
- **Cold cache absent before each of 24 launches:** held. The launch's cache directory was asserted absent before every launch, in every cell.
- **The family applies in every candidate launch:** held in 24 of 24 cold and 24 of 24 warm launches, by the lookup-end proxy above.
- **Time-to-family and first-window time recorded against the base:** in the tables above.
- **Warm cache unchanged within noise:** held. Window p50 485.5 against 488.5 ms, first frame 495.6 against 495.5 ms, family 21.9 against 21.9 ms, with paired differences centred near zero and the same sign counts as chance.
- **The improvement itself is not shown here.** The base also applied the family in 24 of 24 cold launches, because this host's cold scan finishes inside 500 ms. The cold cell shows that the candidate costs nothing measurable at launch. It does not show the late family arriving, which only a scan longer than 500 ms would exercise. The tests in the task's validation entry cover that case. Nothing here added load or slowed the lookup.

## Not measured

- The frame that applies the family; this record has launch to the lookup's end only.
- A cold lookup longer than 500 ms, where the base and candidate differ: on this host that would need added load or a slower disk.
- A warm GPU shader cache, and a cold kernel page cache.
- CPU frequency under load.
- The wrapper's own overhead against the base's 500 ms bound.
- A launch whose window is not activated.
- Native Wayland, Hyprland, another host, fractional scale and macOS, which makes no fontconfig lookup.
