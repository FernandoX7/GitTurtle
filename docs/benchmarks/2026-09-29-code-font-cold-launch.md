# Desktop code font at launch, cold and warm fontconfig caches, 2026-09-29

This is the release measurement for `code-font-cold-launch`. It times launch to first window, and launch to the end of the code font's family lookup, with **Use the desktop's monospace font** on and off, and with a cold and a warm fontconfig cache. The [driver](2026-09-29-code-font-cold-launch/cold_launch.py) is committed beside this record, and the raw samples and cache counts are in [`results/`](2026-09-29-code-font-cold-launch/results/). The contract asks for launch to the code font's application; this record has a lower bound for it, the end of the lookup, because the frame that applies the family is not observable without a product trace.

## What the app does

With the setting on, the app asks fontconfig for its `monospace` family off the UI thread. It runs `fc-match --format %{family} monospace` with a 500 ms bound (`FAMILY_WAIT`, `crates/app/src/desktop_text.rs:482`). A lookup that outlives the bound is killed and reported as "fontconfig did not answer."
- The app does not retry a failed lookup. Another lookup runs only when something asks for one, such as the window's activation (`observe_window_activation`, `crates/app/src/main.rs:901-905`, through `desktop_text::refresh`).
- Launch waits only for the first desktop-text snapshot, which the worker sends before the lookup.
- With the setting off, the app makes no such call.

## What the driver measures

Every launch gets a fresh HOME and XDG directories, the `GitTurtle QA` identity, `TZ=UTC`, Midnight and the setting on or off. So fontconfig's per-user cache is always new, and so is Mesa's shader cache. Every window time, in all four configurations, therefore includes cold GPU shader compilation. "Warm" means a warm fontconfig cache only. The on-against-off comparison is like with like, but the absolute window times are not those of an everyday launch.

Two fontconfig states are used:
- **cold:** `FONTCONFIG_FILE` points at a copy of `/etc/fonts/fonts.conf` whose three `<cachedir>` entries are replaced by one empty directory per launch, so `fc-match` has to scan every font directory.
- **warm:** the system configuration, with its populated `/var/cache/fontconfig`.

Nothing drops the kernel page cache, but the font files' residency was not checked. A partly evicted `/usr/share/fonts/noto` is not ruled out for the first launch of a run.

Two timestamps are taken, each in microseconds of `CLOCK_REALTIME`, from the driver's stamp just before it spawns the process (so they include Python's fork and exec):
- **window:** the driver's receipt of Hyprland's socket2 `openwindow` event for the app's class. Under xdg-shell a toplevel maps only after the app commits a buffer, so this follows the app's first frame commit. It excludes presentation on the 60 Hz output, up to one 16.7 ms tick, and it does not show whether that frame held the repository view.
- **lookup end:** the end of the first `monospace` call that finishes, logged with bash's `EPOCHREALTIME` by an `fc-match` wrapper first on `PATH`.
  - It ends before the wrapper writes the family to the app.
  - It excludes the app's 2 ms read polls, the family check on the worker, the hop to the UI thread, the window refresh and the frame that draws the family. That frame comes at least one 16.7 ms tick later, plus layout and paint.
  - Bash starts before the logged start, so its start-up falls outside each logged call but inside the app's 500 ms bound. It and the `setpriv` exec delay `fc-match` by an amount that was not measured.

`fc-match` runs under `setpriv --pdeathsig KILL`, so when the app kills a lookup at its bound, `fc-match` dies with it, as it would without the wrapper. In three one-round trials (12 launches) without this, the orphaned `fc-match` kept scanning, warmed the cache for the next lookup and competed for the two cores; those trials are not in the results. In the recorded runs, both launches whose calls were all killed still held a partial cache (7 of 12 directories) about 60 s later, where an orphan would have finished the scan.

Each round runs the four configurations once. The order rotates from round to round, and it starts every run with setting on and a cold cache. Each launch waits at least 3 s after the window, then until every started `fc-match` call has finished or been killed and, with the setting on, until a `monospace` call has finished, up to 60 s. The app is then stopped with SIGTERM, and every launch exited on it. The committed driver builds its `.gitconfig` path with `os.path.join` for the repository's home-path check; otherwise it is the file that produced the results (sha256 `5b9a0c14…`). Percentiles are by nearest rank: with 12 samples, p50 is the sixth smallest, and p95 is the largest, so it equals max.

## Setup

- **Build:** `d553cc0`, release, from a clean tree (sha256 `1f1fed5a25c1cb1c90f498db4159a032ae72a394628148ca98eadfee9a998004`, rustc 1.98.0). `crates/app/src/desktop_text.rs` is unchanged from `d553cc0` to `3ac967d`. In that range `main.rs` gains only a test helper, and the other product changes are in Markdown, review, rich-preview and Settings code.
- **Host:** an AMD 3020e with two identical cores, so core pinning does not apply, and 5.7 GiB. Omarchy on Linux 7.2.5-3-omarchy with the `schedutil` governor, Hyprland 0.56.2 and fontconfig 2.18.3. CPU frequency was not recorded.
- **Output:** a temporary 1480 × 800 headless output at scale 1 and 60 Hz, which the driver creates and removes. Every window mapped on its workspace; the samples record it.
- **Fixture:** the disposable `demo` repository at `/tmp/gitturtle-evidence/omarchy-d/demo`.
- **Run 1:** 19:23:42 to 19:28:43 UTC, one-minute load average 0.10 to 1.71 before each launch.
- **Run 2:** 19:30:11 to 19:35:12 UTC, 0.34 to 2.68.
  - From about 19:33:41, another worktree fetched and merged `origin/main` (`a5cc7f8`, 19:33:55), and the load rose from 0.55 to 2.68 over rounds 6 to 10.
  - Several run 2 figures fall in that period: the setting-on, warm window of 566 ms in round 7, and that launch's lookup end of 156 ms, the warm maximum; both cold windows of 483 ms in round 8; and the second lookups of rounds 8 to 11, which took 274 to 331 ms, with the family known 841 to 897 ms after spawn.
- Run 1 is the quiet reference for tails. Each run started once the load fell below 0.6, with no build running.

## Commands

From a Hyprland session, with a release binary and a disposable repository:

```sh
python3 docs/benchmarks/2026-09-29-code-font-cold-launch/cold_launch.py BINARY FIXTURE OUT.json 12
```

## Result

Milliseconds from spawn, as p50 / p95 / max, with 12 launches per cell per run.

Launch to first window:

| Setting, fontconfig | Run 1 | Run 2 |
| --- | ---: | ---: |
| on, cold | 355 / 1104 / 1104 | 365 / 790 / 790 |
| off, cold | 332 / 356 / 356 | 336 / 483 / 483 |
| on, warm | 338 / 352 / 352 | 337 / 566 / 566 |
| off, warm | 331 / 354 / 354 | 330 / 347 / 347 |

Launch to the end of the family lookup, over the launches where a lookup finished:

| Setting, fontconfig | Run 1 | Run 2 |
| --- | ---: | ---: |
| on, cold | 794 / 838 / 838 (n = 11) | 813 / 897 / 897 (n = 11) |
| on, warm | 70 / 74 / 74 (n = 12) | 67 / 156 / 156 (n = 12) |

### The first window

The app does not hold its window for the lookup. In 22 of 24 cold launches the window mapped while the first lookup, started 39 to 56 ms after spawn, was still inside its 500 ms bound.

With a warm cache, the setting moves the window's p50 by 7 ms in both runs (338 against 331 ms, and 337 against 330 ms). That is smaller than the launch-to-launch spread: paired by round, setting on minus off ranges from −12 to +18 ms in run 1. It shows only in aggregate, with 19 of 24 rounds slower with the setting on.

With a cold cache, the setting moves the p50 by 23 and 29 ms (355 against 332 ms, and 365 against 336 ms), in 22 of 24 rounds, with a paired median of 25 ms. On this two-core host the cold scan runs during start-up.

The slowest windows, 1,104 and 790 ms, were the first launch of each run, both cold with the setting on. The rotation starts every run with that configuration, so for these two samples a first-launch effect cannot be separated from the configuration. Cold, setting-on windows were also higher in rounds 1 and 2 of run 1 (406 and 409 ms) and rounds 1 to 3 of run 2 (415, 465 and 403 ms), against 344 to 366 ms later. Leaving out the two first launches, cold, setting-on windows reach 483 ms at most.

### The lookup with a warm cache

Every launch made two `monospace` calls:
- The first starts 38 to 60 ms after spawn (median 48) and ends 58 to 156 ms after spawn, before the window. The 156 ms end falls in run 2's merge period; run 1's reach 74 ms.
- The second, requested by the window's activation, starts 342 to 623 ms after spawn, 14 to 77 ms after the window mapped.

### The lookup with a cold cache

In all 24 launches, the first call, started 39 to 93 ms after spawn, outlived the 500 ms bound and was killed. What followed took two forms:
- **22 launches:** the window mapped (344 to 483 ms after spawn) while the first call ran. So the lookup its activation requested started as soon as the killed call returned, 551 to 579 ms after spawn. That lookup finished in 213 to 331 ms, so the family was known 771 to 897 ms after spawn.
- **2 launches, the first of each run:** the window mapped only after the first call was killed, at 1,104 and 790 ms. So the activation's lookup started later, at 1,287 and 856 ms, and was killed as well. No further lookup ran and no family was found in the about 60 s observed after the window, so the code stayed in the bundled family.

A killed call keeps the directories it finished. fontconfig writes a directory's cache when that directory's scan completes, so the directory in progress is lost and the next lookup scans it again. Counted from the run directories after the runs (the driver does not record them; `results/cold-cache-counts.json` holds the counts):
- Each of the 22 launches whose second lookup finished ended with 12 directories cached: 48 cache entries, 1,507,552 bytes.
- Both launches whose second lookup was killed ended with 7: 28 entries, 155,686 bytes. Neither had the caches of `/usr/share/fonts/noto` (1,151,248 of the 1,507,552 bytes), `noto-cjk`, `ttf-ia-writer`, `omarchy` or `encodings/large`.

The counts are each launch's final state. The state after the first call alone was not recorded.

## For its own task

With no fontconfig cache at all, the first `fc-match monospace` did not finish within the 500 ms bound in any of 24 launches on this host.
- **The family came late.** It came only from the lookup that the window's activation requested: 771 to 897 ms after launch in 22 launches.
- **Twice it never came.** In the other 2, the first launch of each run, that lookup was killed too, and no further lookup ran in the about 60 s observed. Both lacked the Noto directory's cache, which a later lookup must scan again from the start.
- **A launch without activation, by the code.** The app does not retry a failed lookup, so a launch whose window is not activated after the first lookup keeps the bundled family until focus returns. That case was not measured.

The measured state has every fontconfig cache missing; a partly stale cache was not measured.

## Not measured

- The frame that applies the family; this record has launch to the lookup's end only.
- A warm GPU shader cache, and a cold kernel page cache, which needs root to drop. The font files' page-cache residency was not checked.
- The wrapper's own overhead against the 500 ms bound.
- A launch whose window is not activated.
- X11 or XWayland, GNOME, another host or a physical output.
- macOS, which makes no fontconfig lookup.
