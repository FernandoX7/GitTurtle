# Theme picker switching: twenty built-ins and two custom themes on the 32-theme store, 2026-09-23

This records the release measurement of [`gitturtle.theme_apply_frame_ms`](metrics.md) for task `themes-picker` (criterion `switching-budget`) on build `4cdd4df`: 60 Settings theme switches by real pointer clicks cycling all twenty built-in themes and two saved custom themes, with History at 1 to 1000 and a Split comparison of a 2,000-line file retained behind Settings, on the `themes-draw-cost` 32-theme store. Method and format follow the [theme draw-cost record](2026-09-22-theme-draw-cost.md) and the [theme application baseline](2026-09-18-theme-apply.md). The raw data, including every sample, is in [2026-09-23-theme-picker-switching.json](2026-09-23-theme-picker-switching.json).

On the designated launch (`recorded-32`, designated in runs/criterion-set-4cdd4df.txt at 01:19:21Z, started 01:19:25Z) `gitturtle.theme_apply_frame_ms` has median **3.600 ms** against ≤ 8 and p95 (the 57th of 60 sorted samples) **7.691 ms** against ≤ 16, max 8.009 ms, one sample over 8 and none over 16; no app job thread ran and no content trace printed during the 60 switches: `switching-budget` **PASS**. Both bounds are decided by the trace boundary rather than by this code: the value is the handler plus the wait to the next 8.334 ms tick, so every sample lies between 0.570 and one period plus a fraction of a millisecond. The six custom switches (median 6.661, p95 7.617) and the 54 built-in switches (median 3.252, p95 7.741) fall inside the same tick-phase distribution; six samples cannot separate them.

- **`switching-budget`**: median **3.600 ms** (≤ 8: yes), p95 (the 57th of 60) **7.691 ms** (≤ 16: yes), max 8.009; worker jobs during the switches: **0**. Criterion holds: **yes**.
- **UI-thread CPU per switch** (release to 150 ms after it, median): **17.294 ms** on the 32-theme store, beside the draw-cost record's **17.195 ms** (32-theme store, build 85a7d07; +0.099 ms, +0.6%) and 13.793 ms (empty store).
- Control `empty-builtin` (empty_store, 0 custom themes, cycle builtin, executable `3ef3e3a0b999…`): switch median 3.619 / p95 7.796 ms; UI-thread CPU per switch median 13.386 ms beside 13.793 ms (-0.407).
- Control `base-32` (base_build_32, 32 custom themes, cycle builtin (85a7d07 driver), executable `418a3b425ced…`): switch median 4.890 / p95 7.712 ms; UI-thread CPU per switch median 15.714 ms beside 17.195 ms (-1.481).

The value ends at the next-frame callback, before the switch's frame is drawn; see [What the value measures](#what-the-value-measures) before reading the bounds.

## Exercised build and environment

`--build-info` of the exercised executable (read by the driver at the start of every launch):

```json
{
  "application": "GitTurtle",
  "version": "0.1.0",
  "source_revision": "4cdd4df2c0767429ee4c182f73b8e5f82a4312e7",
  "source_tree": "clean",
  "target": "x86_64-unknown-linux-gnu",
  "profile": "release",
  "rustc": "rustc 1.98.0 (88d9e12ae 2026-08-18)",
  "build_unix_seconds": "1790124417"
}
```

The executable SHA-256 was `3ef3e3a0b999b00b76c64d2177f82a93814faf585de5452439bfbcd34894714e`. This record applies to that executable only. The warm-up ran the same executable: yes.

Host: Intel(R) Core(TM) Ultra 9 275HX (24 threads), 188.1 GiB RAM, Pop!_OS 24.04 LTS, Linux `7.1.5-76070105-generic`, git version 2.43.0. The app ran as an X11 client of XWayland (`DISPLAY=:1`, `WAYLAND_DISPLAY` unset) under GNOME Shell 46.0 on a wayland session. Panel and mode: `eDP-1 connected primary 3840x2400+0+0 (normal left inverted right x axis y axis) 390mm x 240mm
   3840x2400    119.98*+`. Renderer: `OpenGL renderer string: Mesa Intel(R) Graphics (ARL)
OpenGL version string: 4.6 (Compatibility Profile) Mesa 26.1.6-1pop0~1787580452~24.04~a5619ea`. The app used `GPUI_X11_SCALE_FACTOR=2` at its default 1480×980 logical window (2960×1960 physical), not resized.

Power: AC online = 1, governor `powersave`, EPP `balance_performance`. Load at `2026-09-23T01:19:21Z` before the recorded launch: 20:19:21 up 4 days, 10:07,  1 user,  load average: 2.15, 2.49, 3.53.

Cache state: warm: explore-32 (4 switches) and warmup-32 (60 switches, not eligible) of the same executable, store and procedure preceded the recorded launch; OS page cache not dropped. Every launch used a fresh process with fresh absolute XDG_CONFIG_HOME, XDG_DATA_HOME and HOME under its run directory (XDG_CACHE_HOME unset), so user-level caches such as the Mesa shader cache started empty in every launch, including the base control. No cold-cache run.

Isolation: every launch had absolute, freshly created `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `HOME` under its run directory (recorded launch: `HOME=/home/fernandoramirez/Documents/GitTurtle/.local/worktrees/claude-code-support/.local/themes-evidence/perf-picker/runs/recorded-32/home`), `XDG_CACHE_HOME` unset, X authority through `XAUTHORITY`, and `GITTURTLE_TRACE=1`.

## Store

version 6, theme midnight, follow_system false, custom_themes 'Seed 01'..'Seed 32' with bases cycling ThemeChoice::ALL and exactly one token (border, blue +1) differing from the base: the themes-draw-cost 32-theme store. On this build the picker's Your themes group holds all 32 cards (its largest count). Seed store sha256 (bytes as written, recorded by the driver): `9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a`. First bytes: `{"version": 6, "settings": {"theme": "midnight", "follow_system": false}, "custom_themes": [{"id": 1, "name": "Seed 01",…`.

## Fixture and state before the first switch

The fixture is the `themes-apply-trace` fixture, unmodified, HEAD `c8f20cfeb3f846168abbdcde270983c42bb27e71`.

| Precondition | How it was verified (recorded launch) |
| --- | --- |
| At least 1,000 commits loaded | The Older control printed `gitturtle.history_page_frame_ms=7.057`; header 1–1000 |
| 2,000-line comparison in Split | Activating `large-module.ts` printed `gitturtle.file_preview_frame_ms=7.967`; Split chosen |
| Comparison retained behind Settings | Back showed the retained Split with 0 new file_preview_frame_ms lines; a second Back showed History |

## Procedure

Driver `tools-4cdd4df/drive_theme_apply.py` (sha256 `0aceb4eb2bef82ba401bddf9ba16c396fe5328b889f76077323fef6ba7e3350b`): the themes-draw-cost switch driver with the picker geometry of this build (4 columns, 264 physical px cards), all switches at one scroll offset (3 wheel steps, where every target card is on screen), cycle `picker`, per-thread worker accounting and isolated homes; CHANGES-from-85a7d07.txt lists every edit. Order from Midnight: custom:2, custom:3, daylight, graphite, tokyo_night, catppuccin_mocha, nord, porcelain, sandstone, deep_sea, ember, solarized_dark, solarized_light, one_dark, one_light, rose_pine, rose_pine_dawn, dracula, alucard, kanagawa_wave, kanagawa_lotus, midnight, then again.

Each switch: at least 1.2 s without input; pointer onto the card (150 ms), button held 150 ms, release. Before the click the target card's top border lay at its expected row in the previous theme's border colour; after it, the card's top border was in the new theme's accent, the previous card had lost it, the store named the new selection (a custom one as `{"custom": id}`), and exactly one new line had printed.

Percentiles: statistics.median; nearest rank for p90/p95: the ceil(p/100*n)-th smallest (p95 of 60 = the 57th smallest). Designation: criterion-set-4cdd4df.txt, written before the recorded launch started at 2026-09-22T20:19:25-0500:

> Criterion launch designated before it ran: run "recorded-32" (launch.sh recorded-32), all 60 switches, cycle picker from Midnight (custom:2 Seed 02, custom:3 Seed 03, ThemeChoice::ALL[1..19], ALL[0]; 22-cycle x 2 + 16), 32-theme store (sha256 9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a), History 1-1000 and the Split comparison of large-module.ts retained behind Settings.
> Graded (themes-picker switching-budget): gitturtle.theme_apply_frame_ms median <= 8 ms and p95 (57th of 60 sorted) <= 16 ms; worker jobs zero = 0 timeslices on the app job threads over the switch loop and 0 content trace lines; UI-thread CPU per switch (release to +150 ms, median) reported beside 17.195 ms (2026-09-22 draw-cost record, 32-theme store).
> Designated at 2026-09-23T01:19:21Z by the performance reviewer; the launch had not started. Build under test: gitturtle-4cdd4df, sha256 3ef3e3a0b999b00b76c64d2177f82a93814faf585de5452439bfbcd34894714e; driver tools-4cdd4df/drive_theme_apply.py sha256 0aceb4eb2bef82ba401bddf9ba16c396fe5328b889f76077323fef6ba7e3350b.

## Samples

Values in milliseconds (`gitturtle.theme_apply_frame_ms`), switch number in parentheses; rows follow the cycle order.

| Target | kind | Cycle 1 | Cycle 2 | Cycle 3 |
| --- | --- | ---: | ---: | ---: |
| custom:2 | custom | 7.617 (#1) | 6.214 (#23) | 7.209 (#45) |
| custom:3 | custom | 5.185 (#2) | 7.107 (#24) | 2.732 (#46) |
| daylight | builtin | 2.732 (#3) | 0.801 (#25) | 5.965 (#47) |
| graphite | builtin | 0.570 (#4) | 6.691 (#26) | 3.130 (#48) |
| tokyo_night | builtin | 0.797 (#5) | 5.970 (#27) | 1.120 (#49) |
| catppuccin_mocha | builtin | 2.393 (#6) | 7.741 (#28) | 7.318 (#50) |
| nord | builtin | 0.831 (#7) | 1.502 (#29) | 3.393 (#51) |
| porcelain | builtin | 3.112 (#8) | 0.854 (#30) | 8.009 (#52) |
| sandstone | builtin | 0.741 (#9) | 7.988 (#31) | 3.872 (#53) |
| deep_sea | builtin | 1.991 (#10) | 6.204 (#32) | 1.627 (#54) |
| ember | builtin | 6.594 (#11) | 0.823 (#33) | 5.017 (#55) |
| solarized_dark | builtin | 0.710 (#12) | 7.691 (#34) | 1.495 (#56) |
| solarized_light | builtin | 0.723 (#13) | 6.922 (#35) | 5.973 (#57) |
| one_dark | builtin | 5.229 (#14) | 3.327 (#36) | 0.576 (#58) |
| one_light | builtin | 0.815 (#15) | 5.997 (#37) | 2.869 (#59) |
| rose_pine | builtin | 6.588 (#16) | 3.176 (#38) | 5.956 (#60) |
| rose_pine_dawn | builtin | 6.320 (#17) | 0.879 (#39) |  |
| dracula | builtin | 4.231 (#18) | 6.266 (#40) |  |
| alucard | builtin | 2.685 (#19) | 3.847 (#41) |  |
| kanagawa_wave | builtin | 3.156 (#20) | 7.393 (#42) |  |
| kanagawa_lotus | builtin | 5.807 (#21) | 3.807 (#43) |  |
| midnight | builtin | 2.671 (#22) | 1.340 (#44) |  |

Sorted, all 60: 0.570, 0.576, 0.710, 0.723, 0.741, 0.797, 0.801, 0.815, 0.823, 0.831, 0.854, 0.879, 1.120, 1.340, 1.495, 1.502, 1.627, 1.991, 2.393, 2.671, 2.685, 2.732, 2.732, 2.869, 3.112, 3.130, 3.156, 3.176, 3.327, 3.393, 3.807, 3.847, 3.872, 4.231, 5.017, 5.185, 5.229, 5.807, 5.956, 5.965, 5.970, 5.973, 5.997, 6.204, 6.214, 6.266, 6.320, 6.588, 6.594, 6.691, 6.922, 7.107, 7.209, 7.318, 7.393, 7.617, 7.691, 7.741, 7.988, 8.009.

| Set | n | min | median | mean | p90 | p95 | max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **all** | 60 | 0.570 | 3.600 | 4.005 | 7.318 | 7.691 | 8.009 |
| builtin | 54 | 0.570 | 3.252 | 3.782 | 7.318 | 7.741 | 8.009 |
| custom | 6 | 2.732 | 6.661 | 6.011 | 7.617 | 7.617 | 7.617 |

## Worker jobs

Per-thread /proc/<pid>/task/*/{comm,schedstat} snapshots just before the first switch's quiet period and 1 s after the last switch (loop), and around each switch (before its quiet period, 0.35 s after its CPU window). A thread woken by a job gains timeslices (schedstat field 3); the app's job threads block on a condition variable or channel receive when idle, so zero timeslices on every one of them (WORKER_THREADS: the source's thread names) over the loop means no job ran; gitturtle-preferences is the preference writer, expected once per switch. The content traces (commit_files, file_preview, working_preview, history_page and theme_edit_frame_ms) are counted over the same loop.

Over the loop: worker-thread timeslices **0**, content trace lines {'gitturtle.commit_files_frame_ms=': 0, 'gitturtle.file_preview_frame_ms=': 0, 'gitturtle.working_preview_frame_ms=': 0, 'gitturtle.history_page_frame_ms=': 0, 'gitturtle.theme_edit_frame_ms=': 0}, preference-writer timeslices 243; per-switch windows: worker timeslices 0 in total. Threads at the loop's start: {'Clipboard': 1, 'Timer': 1, 'UI (main)': 1, 'WSI swapchain e': 1, 'WSI swapchain q': 1, 'Worker-0': 1, 'Worker-1': 1, 'Worker-10': 1, 'Worker-11': 1, 'Worker-12': 1, 'Worker-13': 1, 'Worker-14': 1, 'Worker-15': 1, 'Worker-16': 1, 'Worker-17': 1, 'Worker-18': 1, 'Worker-19': 1, 'Worker-2': 1, 'Worker-20': 1, 'Worker-21': 1, 'Worker-22': 1, 'Worker-23': 1, 'Worker-3': 1, 'Worker-4': 1, 'Worker-5': 1, 'Worker-6': 1, 'Worker-7': 1, 'Worker-8': 1, 'Worker-9': 1, 'async-io': 1, 'blocking-1': 1, 'blocking-2': 1, 'gittur:traceq0': 2, 'gitturt:disk$0': 2, 'gitturt:disk$1': 1, 'gitturt:disk$2': 1, 'gitturt:disk$3': 1, 'gitturtl:gdrv0': 1, 'gitturtle-4cdd4': 1, 'gitturtle-:sh0': 1, 'gitturtle-local': 1, 'gitturtle-opera': 1, 'gitturtle-path-': 1, 'gitturtle-prefe': 1, 'gitturtle-reade': 2, 'notify-rs inoti': 1, 'smol-1': 1, 'zbus::Connectio': 3}. Zero worker jobs: **yes**.

## UI-thread CPU per switch

UI (main) thread run time from /proc/<pid>/task/<pid>/schedstat, just before the release to 150 ms after it: the handler plus the new theme's frame and anything else in that window (drive_theme_apply.py, unchanged from the themes-draw-cost record's driver).

| Launch | store | cycle | median | p95 | max | beside (draw-cost, same store) | Δ |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| recorded-32 | 32 | picker | 17.294 | 26.315 | 35.474 | 17.195 | +0.099 |
| warmup-32 | 32 | picker | 23.108 | 26.437 | 41.035 | 17.195 | +5.913 |
| empty-builtin (empty_store) | 0 | builtin | 13.386 | 19.582 | 21.629 | 13.793 | -0.407 |
| base-32 (base_build_32) | 32 | builtin (85a7d07 driver) | 15.714 | 24.383 | 38.062 | 17.195 | -1.481 |

- builtin switches: UI-thread CPU median 17.236 ms (n = 54).
- custom switches: UI-thread CPU median 19.239 ms (n = 6).
- Control windows, recorded launch (150 ms of UI-thread CPU, median): idle 1.050 ms; hover 15.840 ms; press 16.343 ms.
- Whole process, recorded launch: median 18.870 ms.

UI-thread CPU per switch, the number that carries the picker's draw, has median **17.294 ms** beside the draw-cost record's **17.195 ms** (+0.099 ms). That difference is below the noise: the warm-up of the same executable, store and procedure four minutes earlier measured 23.108 ms, with its hover and press control windows up by the same amount (21.299 and 22.637 against 15.840 and 16.343 ms), so whole launches moved by about 6 ms. The base build measured in the same session under the same isolation (`base-32`, 85a7d07, the draw-cost driver) gave 15.714 ms, so the candidate is +1.580 ms (+10.1%) over today's base. That is also within the launch spread, but its direction matches the diff: a Settings frame on the 32-theme store now builds 52 picker cards where 85a7d07 built 20. With no custom themes saved (`empty-builtin`) the candidate measured 13.386 ms beside 13.793 ms (−0.407), so the store without custom themes pays nothing. No speed claim is made either way for switches.

## What the value measures

Stamped at choose_theme entry (settings.rs:581, choose_theme at 575), for a built-in and a custom selection alike on this build; a reselection returns before any work, and a custom id that is no longer saved returns unapplied. apply_appearance (line 594) applies the palette, text sizes and the in-place restyle of the retained patch, split, file-history and revision editors and notifies the root; save_preferences (line 595) submits the store write to the gitturtle-preferences executor; trace_next_frame (line 597, main.rs:1380) registers window.on_next_frame (main.rs:1381), whose callback prints the line. Next-frame callbacks run on the platform frame tick before that frame is laid out and painted, so the value spans the handler plus the wait to the next tick of the 8.334 ms X11 grid and excludes the layout, prepaint and paint that show the new theme, pre-handler input delivery, the store write, present and GPU execution.

By construction the value is at least the handler's own cost and at most the handler plus one tick period (8.334 ms) while the UI thread is idle at the tick; with the release's phase on the grid uniform, the median sits near handler + 4.2 ms and the p95 near handler + 7.9 ms. The 16 ms p95 bound is therefore decided by the boundary unless the handler alone approaches 8 ms, and the 8 ms median bound unless it approaches 4 ms; neither can see the cost of drawing the picker's cards. UI-thread CPU per switch (release to 150 ms after it) is the number that moves with this code.

XTest release to the line's arrival exceeded the printed value by the input delivery and pipe latency: release to line median 3.868 ms. Line arrival phase on the 8.334 ms grid: stdev 0.259 ms (lines land on the tick). Samples above one period: 0.

## Secondary check: editor edit frames on this build (information, not graded here)

`gitturtle.theme_edit_frame_ms` on the 32-theme store with the themes-draw-cost procedure (60 pooled edits in six dialogs of ten, p95 = the 57th of 60). The dialog opens through row 1's Edit…, which on this build takes 25 wheel steps and 60 Tabs (15 and 28 on 85a7d07; one more Tab per custom picker card). Against the draw-cost record's 25.372 / 21.778 ms the candidate looks about 7.6 ms slower at the p95, but host drift was large on this day: 85a7d07 itself measured p95 29.079 ms here. A same-session pair was therefore run, designated in writing before either launch (runs/edit-pair-set.txt), base then candidate back to back with the same driver, store and isolation:

| Launch | build | start | p95 | median | edits > 26 ms | UI-thread CPU per edit, median |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| base-edit-32 | 85a7d07 | 20:41 local | 29.079 | 24.114 | 21 | 32.354 |
| edit-pair-32 | 4cdd4df | 20:44 local | 33.024 | 26.244 | 31 | 31.625 |
| edit-recorded-32 | 4cdd4df | 20:35 local | 32.984 | 28.824 | 41 | 37.945 |
| edit-warmup-32 | 4cdd4df | 20:32 local | 32.878 | 27.997 | 38 | 37.448 |

Same session, the candidate is **+3.945 ms at the p95** (+13.6%) and +2.130 ms at the median, with +10 edits over 26 ms. Its three launches agree within 0.146 ms at the p95, so that difference is outside their spread; only one base launch was run, and another lens reported the base at p95 28.696 ms earlier on the same day with the same isolation, 0.38 ms from this one. UI-thread CPU per edit does not separate the builds (the candidate's launches span 31.625 to 37.945 ms against the base's 32.354). On this host on this day neither build meets `themes-draw-cost`'s 26 ms p95 bound; the candidate adds about 4 ms to it. The likely cause, inferred from the diff and not measured per phase, is that every live-preview edit lays out and paints the Settings page under the dialog, whose picker now holds 52 cards instead of 20. The smallest fix is to keep the page out of the edit frame (the cached Settings page of `themes-settings-view`, or a picker that is not rebuilt while the dialog is open).

## Limitations

- Linux XWayland on GNOME Wayland only (not native Wayland, not macOS); synthetic XTest clicks; 120 Hz panel at 2x.
- One recorded launch of 60 switches; warm caches; uncontrolled desktop load on the owner's active session (1-minute load 2.15 at the recorded start, 3.04 after).
- These numbers bind to executable sha256 3ef3e3a0b999b00b76c64d2177f82a93814faf585de5452439bfbcd34894714e only.
- The graded metric ends at the next-frame callback before the frame is drawn, so its bounds are decided by the 8.334 ms tick grid plus the handler; UI-thread CPU per switch is the number that carries the picker's draw, and its launch-to-launch spread (warm-up 23.108 against recorded 17.294 ms on the same executable) exceeds every difference reported beside it.
- HOME and XDG_DATA_HOME were isolated, unlike the draw-cost record's launches: Git read no global config, so the Settings identity line shows a host-derived address and wraps to three lines instead of two (base-32 screenshot against the draw-cost record's), and Compare's code text rasterizes differently (the owner's user fonts are not visible); Compare is not drawn during the switches. The base-32 control ran under the same isolation.
- Worker jobs are observed natively as thread wake-ups (schedstat timeslices of the app's named job threads) and content trace lines; GPUI's own background pool (Worker-N), Mesa/WSI and timer threads ran and are reported apart as not app work. A job thread that was created and exited inside the loop would not appear in the end snapshot; none of the app's job threads is transient.
- Six of the 60 switches target custom themes (Seed 02 and Seed 03, three each); all switches ran at one scroll offset (3 wheel steps).
- The editor edit-frame check is information for themes-draw-cost's bound, not part of this criterion; no instrumented build was made, so its cause is inferred from the diff, not measured per phase.

## Reproduction

The driver, analyzers and this record's builder are retained outside the repository in the themes evidence directory (`perf-picker/tools-4cdd4df/`) with per-launch data under `perf-picker/runs/`; all script digests are in the JSON. Every launch had fresh absolute `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `HOME` under its run directory; the window was found by `_NET_WM_PID`, and the driver terminated only the process it launched.

```sh
cd perf-picker/tools-4cdd4df
python3 drive_theme_apply.py <explore-run> --binary /abs/path/gitturtle --custom-themes 32 --switches 4 --explore
python3 drive_theme_apply.py <warmup-run> --binary /abs/path/gitturtle --custom-themes 32
python3 drive_theme_apply.py <recorded-run> --binary /abs/path/gitturtle --custom-themes 32   # after writing runs/criterion-set-<sha7>.txt
python3 drive_theme_apply.py <empty-run> --binary /abs/path/gitturtle --custom-themes 0 --cycle builtin
python3 build_picker_record.py --recorded <recorded-run> --warmup <warmup-run> [--explore <explore-run>] [--empty-control <empty-run>] [--base-control <base-run>] --date <YYYY-MM-DD> --out /abs/new-record.json
python3 render_picker_record_md.py /abs/new-record.json
```
