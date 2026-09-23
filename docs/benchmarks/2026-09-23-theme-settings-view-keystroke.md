# Theme Settings page as a cached view: dialog keystrokes and caret blinks, pinned to the P-cores, 2026-09-23

This records the release measurement for task `themes-settings-view` (attempt 3), criterion `keystroke-cpu`, on build `e73be2e`: UI-thread CPU per non-preview keystroke in the theme editor (the key-6 control of the [theme editor record](2026-09-19-theme-editor.md)), the count and draw cost of caret-blink frames while the dialog is idle for 10 s, and [`gitturtle.theme_edit_frame_ms`](metrics.md) and UI-thread CPU per Settings switch re-measured as in the [theme draw-cost record](2026-09-22-theme-draw-cost.md) against its bounds. Every graded launch ran pinned to the P-cores, and the accepted picker build `4cdd4df` ([picker record](2026-09-23-theme-picker-switching.md)) was measured pinned in the same session by the same drivers as the designated base control; see [Grading method](#grading-method). The raw data is in [2026-09-23-theme-settings-view-keystroke.json](2026-09-23-theme-settings-view-keystroke.json).

All 13 designated launches ran pinned or unpinned as designated and exited 0; every measured window of every pinned launch ran on a P-core. On the designated launch (`recorded-32`, designated in runs/criterion-set-a3.txt at 13:37:30Z, started 13:37:41Z at a 1-minute load of 1.66) the UI-thread CPU per non-preview keystroke has median **7.191 ms** against ≤ 14 (p95 8.930, max 9.215), against 20.406 ms for the pinned picker build in the same session (−13.215 ms, −64.8%): a keystroke in the hex field now rebuilds its token row, not the dialog and the page. The caret-blink frames follow: 20 frames in the designated 10 s window, median frame duration **7.197 ms** (base 20 frames, 20.125 ms), and all six windows hold exactly 20 frames with no short gap. `gitturtle.theme_edit_frame_ms` holds its bound with room: pooled p95 **21.000 ms** against ≤ 26 (median 18.404, none over 26), 0.913 ms under the pinned base's 21.913, so the card-body layer recovers the picker's edit cost rather than adding to it. Switches: empty-store UI-thread CPU median **12.457 ms** against ≤ 23 and `theme_apply_frame_ms` at 4.409 / 7.891 (empty) and 4.232 / 7.689 (32 themes) against 8 / 16 hold, but the switch delta **fails the coordinator's threshold by 0.006 ms**: the candidate's 32-minus-empty delta is +4.006 ms against max(+4, base delta +3.505 + base launch spread 0.195) = 4.000 ms. The miss is far inside the sampling error of one launch pair (bootstrap 95% interval of the candidate delta 3.474 to 4.408 ms), so it is reported as a FAIL by the rule and flagged to the coordinator as borderline. The larger finding under it is store-independent: every Settings frame class costs more on the candidate than on the base, on both stores and beyond the base's launch spread (0.06 to 0.13 ms): switch +1.555 ms empty and +2.056 ms with 32 themes, hover controls +1.427 / +1.123, press controls +1.802 / +1.761. Likely causes from the brief (not instrumented here): the card clip's extra frame on a hover-state change and the per-frame cost of rebuilding 20 to 52 cached card bodies and their deferred CardLayer draws when a click refreshes the window. On the letter, keystroke-cpu is **not met** on G5 alone (+4.006 ms against ≤ 4.000 ms, 0.006 ms over) and every other bound is met on the pinned figures; the coordinator's ruling of 2026-09-23, reported separately below, accepts G5 as within measurement resolution, with which the criterion holds.

- **UI-thread CPU per non-preview keystroke** (`recorded-32`, pinned `0-7`, 32-theme store, key 6 of each of the 60 edits, 150 ms window): min 4.969, median **7.191**, p95 8.930, max 9.215 (n = 60). Median ≤ 14 ms: **yes**. Base `4cdd4df` pinned, same session: min 17.747, median **20.406**, p95 22.128, max 23.767 (n = 60); candidate minus base -13.215 (-64.8%) ms.
- **Caret-blink frames, dialog idle for 10 s** (`blink-32`, set 1's window, designated before the launch; no bound): **20 frames** in 10000.125 ms (20 caret toggles expected), frame duration (draw + present) min 4.934, median **7.197**, p95 8.192, max 8.517 (n = 20); UI-thread CPU 8.923 ms per frame. Base `4cdd4df`: **20 frames**, min 17.867, median **20.125**, p95 21.082, max 22.759 (n = 20).
- **`gitturtle.theme_edit_frame_ms` re-measured** (`recorded-32`, 60 pooled edits): p95 (the 57th of 60) **21.000 ms**, median 18.404, max 21.410, 0 of 60 over 26. p95 ≤ 26 ms: **yes**. Base `4cdd4df` pinned: p95 21.913, median 19.657 (candidate minus base -0.913 (-4.2%) ms at the p95).
- **UI-thread CPU per switch** (release to +150 ms, median, pinned): empty store **12.457 ms** (≤ 23: **yes**), 32-theme store 16.463 ms, delta +4.006 ms; same-session pinned base `4cdd4df`: 10.902 / 14.407 ms, delta +3.505 ms; repeat +3.700 ms; base spread 0.195 ms (launch-to-launch (base repeats)); candidate minus base +0.501 ms. Candidate delta ≤ max(+4, base delta + spread) = 4.000 ms: **no**. Bootstrap 95% intervals of the deltas: candidate 3.474 to 4.408, base 3.033 to 4.607 ms. `gitturtle.theme_apply_frame_ms` median / p95: empty 4.409 / 7.891, 32 themes 4.232 / 7.689 (≤ 8 / ≤ 16 on both: **yes**).
- **Pinning held**: every window of every pinned launch on a P-core: **yes**; every launch pinned as designated: **yes**.
- **Overall**: on the letter, keystroke-cpu is **not met** on G5 alone: the switch delta measured **+4.006 ms** against ≤ **4.000 ms** (0.006 ms over). Every other bound is met on the pinned figures. With G5 accepted by the coordinator's ruling below, the criterion holds: **yes**.

**Coordinator ruling on G5** (separate from the measurement, recorded as `coordinator_ruling` in the JSON beside the measured verdict): on 2026-09-23 the coordinator (interactive session), after the performance reviewer's report accepted as within measurement resolution, on these grounds: the excess is 0.006 ms, about 1/30 of the base's 0.195 ms launch spread; the candidate delta's bootstrap 95% interval, 3.474 to 4.408 ms, contains the bound; the owner's standing 2026-09-21 instruction for these draw-cost bounds that 'a few ms off is acceptable', the instruction under which the allowance was widened from 3 to 4 ms. The candidate switch pair was not re-run: re-running a designated launch that failed would be shopping for a better verdict.

## Grading method

Owner and coordinator decision of 2026-09-23: every graded launch runs the app pinned to the P-cores (taskset -c 0-7; CPUs 0-7 are P-cores and 8-23 E-cores on this Intel Core Ultra 9 275HX), with the UI thread's core (/proc/<pid>/task/<pid>/stat field 39) and that CPU's scaling_cur_freq recorded at the start and end of every measurement window. The graded candidate launches and a same-session pinned base control are designated in writing (a file with a UTC timestamp) before either runs. Relative criteria (the switch-no-regression delta and its allowance, any regression) are judged against that same-session pinned base; absolute bounds on the pinned figures: key-6 UI-thread CPU median <= 14 ms, theme_edit_frame_ms p95 <= 26 ms over 60 pooled edits on the 32-theme store, theme_apply_frame_ms median <= 8 and p95 <= 16. The unpinned figures of the 2026-09-22 draw-cost record and the 2026-09-23 picker record are reported beside the pinned ones. Reason (diag-66b6af1/diagnosis.md, DIAG2): unpinned, this host's UI thread runs in streaks on E-cores, whose windows cost about 1.27x the P-core windows, so an unpinned median grades the core mix; whole sessions also shift by up to 1.35x, which the same-session base absorbs.

**As run.** Every graded launch and every base control ran the app under /usr/bin/taskset -c 0-7 (the P-cores of this Intel Core Ultra 9 275HX; 8-23 are E-cores); the UI thread's Cpus_allowed_list was 0-7 when the window appeared and at the end of every such launch. Per window the UI thread's core (stat field 39) and that CPU's frequency were read immediately before the opening and after the closing schedstat read: every measured window of every pinned launch (key 6, edits, idle controls, blink windows and their 1 ms samples, switches, hover and press controls) had both reads on P-cores. Six Mesa threads (the disk-cache queue disk$0-3 and the shader compiler sh0) set their own affinity back to 0-23 after the pin; they are recorded per launch (affinity.other_list_threads) and do not enter UI-thread metrics. Designations: the plan (plan.md section 3) was written in Phase A; the G5 threshold max(+4 ms, base delta + base launch spread) was set by the coordinator before Phase B; the validity rule was amended at 13:35Z (UI thread's list, not every thread's, after explore-sw-32 showed the Mesa threads); runs/criterion-set-a3.txt was written at 2026-09-23T13:37:30Z, before recorded-32 (13:37:41Z) and base-recorded-32 (13:40:49Z). The unpinned launch unpinned-recorded-32 (14:05:57Z) is reported beside and grades nothing.

Designation file `runs/criterion-set-a3.txt`, written before the graded and base launches started: **yes**.

```text
Graded launches and the same-session pinned base control, designated before any of them ran (plan.md section 3, written in
Phase A and amended at 13:35Z before this designation), task themes-settings-view attempt 4, criterion keystroke-cpu.
Every launch below except unpinned-* runs the app under taskset -c 0-7 (P-cores); a launch counts only if the UI thread's
Cpus_allowed_list is 0-7 and every measured window's placement reads are on P-cores.
- G1 recorded-32 (candidate, pinned, 32-theme store): median of all 60 ui_cpu_key6_ms windows <= 14 ms.
- G2 recorded-32: gitturtle.theme_edit_frame_ms pooled p95 (57th of 60) <= 26 ms; set 1 reported as the designated set, no bound.
- G3 blink-32 (candidate, pinned, --frames --bursts --idle 10): set 1's 10 s idle window gives the reported caret-blink count
  and median frame duration; windows 2-6 reported beside it.
- G4 switches-empty (candidate, pinned, built-in cycle, empty store): switch UI-thread CPU median <= 23 ms.
- G5 switch delta: PASS iff candidate delta (switches-32 minus switches-empty) <= max(+4 ms, base delta (base-switches-32
  minus base-switches-empty) + base launch spread |delta of base-switches-32-b / base-switches-empty-b minus base delta|),
  else FAIL (coordinator's threshold of 2026-09-23); both deltas, the spread and the difference reported.
- G6 theme_apply_frame_ms median <= 8 and p95 <= 16 in switches-empty and switches-32.
- Base controls (gitturtle-4cdd4df, pinned, same drivers and isolation): base-recorded-32, base-blink-32, base-switches-32,
  base-switches-empty, base-switches-empty-b, base-switches-32-b: relative criteria and regression lines only.
- Beside, grading nothing: unpinned-recorded-32 (candidate, unpinned); optional unpinned-switches-32; not eligible:
  explore-sw-32, warmup-32 (both already ran: geometry confirmed, card top 1188 at 25 steps, row 1 Edit... at 60 Tabs).
Candidate gitturtle-e73be2e sha256 f7f6aa14146ee481e65d843d8645541b55060a2460604c1926cc78ec76330b73 (e73be2e6f49b27282c7edc13a250e26d26003977, clean, release);
base gitturtle-4cdd4df sha256 3ef3e3a0b999b00b76c64d2177f82a93814faf585de5452439bfbcd34894714e; store sha256 9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a (32) / ae1731ac8307019b39aee6a216564410b55b656871ddb1b9d552e8e988e4e208 (empty).
Drivers: tools-a3/drive_theme_editor.py sha256 e639a769d35ea8399b5b38ede830ff21e81bcfb8c4ebbfef5135e62687e191a6, drive_theme_apply.py 1bb145ede292fdb40d3e868f8180d0147f7f31284e31115caf3599e1a98f7efb, cpu_place.py 78bf1760fa06c865046498b30689769250aef72619c492656fea155f76fc2a86.
Designated at 2026-09-23T13:37:30Z by the performance reviewer (perf-sv-a3); recorded-32 and base-recorded-32 had not started.
```

## Exercised builds and environment

Candidate `--build-info` (read by the driver at the start of every launch), executable SHA-256 `f7f6aa14146ee481e65d843d8645541b55060a2460604c1926cc78ec76330b73`:

```json
{
  "application": "GitTurtle",
  "version": "0.1.0",
  "source_revision": "e73be2e6f49b27282c7edc13a250e26d26003977",
  "source_tree": "clean",
  "target": "x86_64-unknown-linux-gnu",
  "profile": "release",
  "rustc": "rustc 1.98.0 (88d9e12ae 2026-08-18)",
  "build_unix_seconds": "1790170166"
}
```

Base `--build-info` (read by the driver at the start of every launch), executable SHA-256 `3ef3e3a0b999b00b76c64d2177f82a93814faf585de5452439bfbcd34894714e`:

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

Every candidate launch ran the same executable: **yes**; every base launch: **yes**.

Host: Intel(R) Core(TM) Ultra 9 275HX (24 threads; P-cores [0, 1, 2, 3, 4, 5, 6, 7], E-cores the rest), 188.1 GiB RAM, Pop!_OS 24.04 LTS, Linux `7.1.5-76070105-generic`. XWayland `DISPLAY=:1` at `GPUI_X11_SCALE_FACTOR=2`, 1480×980 logical window. Power: AC online = 1, governor `powersave`, EPP `balance_performance`; turbo on. Load before the graded launch: 08:37:30 up 4 days, 22:25,  1 user,  load average: 1.39, 2.76, 2.95.

Cache state: warm OS page cache: explore-sw-32 (13:32Z) and warmup-32 (13:34Z) of the candidate preceded the graded launches (recorded-32 at 13:37:41Z); caches not dropped (no sudo on this host). Every launch a fresh process with fresh absolute HOME, XDG_CONFIG_HOME and XDG_DATA_HOME under runs/<run>/ and XDG_CACHE_HOME unset, so user-level caches (fontconfig, Mesa shader and disk caches) start empty in every launch, candidate and base alike.

## Store and fixture

The themes-draw-cost 32-theme store (seed.py 32, sha256 `9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a`; the empty store `ae1731ac8307019b39aee6a216564410b55b656871ddb1b9d552e8e988e4e208`); the picker draws all 32 saved themes as cards in its Your themes group. Fixture: themes-evidence/perf-theme-apply/repo (the themes-apply-trace fixture, unmodified), HEAD `c8f20cfeb3f846168abbdcde270983c42bb27e71`; History 1-1000 (Older), src/generated/large-module.ts in Compare, Split, retained behind Settings; the themes-draw-cost fixture state.

## Launches

| Launch | build | pin | store | started | load before / after | exit |
| --- | --- | --- | ---: | --- | --- | ---: |
| warmup-32 | e73be2e | 0-7 | 32 | 2026-09-23T08:34:07-0500 | 2.513 / 1.216 | -15 |
| recorded-32 | e73be2e | 0-7 | 32 | 2026-09-23T08:37:41-0500 | 1.658 / 0.657 | -15 |
| blink-32 | e73be2e | 0-7 | 32 | 2026-09-23T08:47:59-0500 | 1.487 / 0.910 | -15 |
| switches-empty | e73be2e | 0-7 | 0 | 2026-09-23T08:52:07-0500 | 0.924 / 0.820 | -15 |
| switches-32 | e73be2e | 0-7 | 32 | 2026-09-23T08:55:34-0500 | 1.235 / 0.978 | -15 |
| explore-sw-32 | e73be2e | 0-7 | 32 | 2026-09-23T08:32:42-0500 | 2.773 / 2.552 | -15 |
| unpinned-recorded-32 | e73be2e | unpinned | 32 | 2026-09-23T09:05:57-0500 | 1.125 / 0.886 | -15 |
| base-recorded-32 | 4cdd4df | 0-7 | 32 | 2026-09-23T08:40:50-0500 | 0.802 / 1.507 | -15 |
| base-blink-32 | 4cdd4df | 0-7 | 32 | 2026-09-23T08:43:51-0500 | 1.576 / 0.856 | -15 |
| base-switches-32 | 4cdd4df | 0-7 | 32 | 2026-09-23T08:59:03-0500 | 1.276 / 0.838 | -15 |
| base-switches-empty | 4cdd4df | 0-7 | 0 | 2026-09-23T09:02:28-0500 | 1.012 / 1.061 | -15 |
| base-switches-32-b | 4cdd4df | 0-7 | 32 | 2026-09-23T09:12:26-0500 | 1.229 / 0.988 | -15 |
| base-switches-empty-b | 4cdd4df | 0-7 | 0 | 2026-09-23T09:08:59-0500 | 1.285 / 1.075 | -15 |

## Key 6: UI-thread CPU per non-preview keystroke

| Launch | pin | n | min | median | p95 | max | windows on P |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| recorded-32 | 0-7 | 60 | 4.969 | 7.191 | 8.930 | 9.215 | P 60; non-P none; end MHz 800–3300 (median 800) |
| base-recorded-32 | 0-7 | 60 | 17.747 | 20.406 | 22.128 | 23.767 | P 60; non-P none; end MHz 800–4766 (median 800) |
| unpinned-recorded-32 | unpinned | 60 | 4.862 | 8.877 | 11.198 | 11.448 | P 38, E 18, mixed 4; non-P [1, 2, 3, 4, 5, 9, 16, 17, 19, 26, 27, 32, 33, 34, 35, 36, 37, 38, 39, 47, 50, 58]; end MHz 800–3679 (median 925) |

Samples of `recorded-32` in order (ms, six dialogs of ten): 6.223 5.723 5.586 5.324 5.989 6.725 6.741 8.005 6.917 9.139 8.427 7.701 8.139 7.627 5.615 6.379 6.028 5.908 9.035 5.407 7.423 7.041 8.930 7.263 7.974 7.044 6.712 6.814 6.520 6.166 9.215 8.111 7.428 8.026 8.041 7.248 7.134 7.956 8.099 7.527 8.187 7.066 7.770 7.524 7.319 4.969 8.813 6.210 8.638 8.089 7.870 6.684 6.090 6.778 7.035 8.012 7.761 5.570 5.829 6.228.

Idle control (the same 150 ms without input): candidate median 0.631, base 0.598. Warm-up (not eligible): median 7.418.

## Caret-blink frames

| Launch | set | frames | draw + present median | p95 | UI CPU per frame | short gaps | core |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| blink-32 | 1 | 20 | 7.197 | 8.192 | 8.923 | 0 | P |
| blink-32 | 2 | 20 | 7.457 | 8.047 | 8.861 | 0 | P |
| blink-32 | 3 | 20 | 6.759 | 7.331 | 8.189 | 0 | P |
| blink-32 | 4 | 20 | 5.941 | 8.630 | 8.326 | 0 | P |
| blink-32 | 5 | 20 | 5.332 | 8.352 | 7.731 | 0 | P |
| blink-32 | 6 | 20 | 6.469 | 7.316 | 8.246 | 0 | P |
| base-blink-32 | 1 | 20 | 20.125 | 21.082 | 22.145 | 0 | P |
| base-blink-32 | 2 | 20 | 21.386 | 22.778 | 23.329 | 0 | P |
| base-blink-32 | 3 | 20 | 20.709 | 21.418 | 22.628 | 0 | P |
| base-blink-32 | 4 | 20 | 20.770 | 22.086 | 22.734 | 0 | P |
| base-blink-32 | 5 | 20 | 21.794 | 23.250 | 23.675 | 0 | P |
| base-blink-32 | 6 | 20 | 21.095 | 22.447 | 22.871 | 0 | P |

## Edit frame re-measure

| Launch | pin | p95 (57th of 60) | median | max | over 26 | UI CPU per edit, median |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| recorded-32 | 0-7 | 21.000 | 18.404 | 21.410 | 0 | 20.620 |
| base-recorded-32 | 0-7 | 21.913 | 19.657 | 23.052 | 0 | 21.675 |
| unpinned-recorded-32 | unpinned | 25.529 | 20.984 | 26.295 | 1 | 25.773 |

Designated set (set 1, reported, no bound): 18.715 17.333 15.532 17.938 18.263 20.513 19.836 17.311 17.099 15.207. Per-set medians: 17.636, 18.314, 18.953, 18.091, 19.502, 17.604.

## Switches

| Launch | pin | store | UI CPU median | p95 | apply median | apply p95 | windows on P |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| switches-empty | 0-7 | 0 | 12.457 | 14.006 | 4.409 | 7.891 | P 60; non-P none; end MHz 800–4295 (median 800) |
| switches-32 | 0-7 | 32 | 16.463 | 18.391 | 4.232 | 7.689 | P 60; non-P none; end MHz 800–3706 (median 1833) |
| base-switches-empty | 0-7 | 0 | 10.902 | 12.967 | 4.803 | 7.806 | P 60; non-P none; end MHz 800–3086 (median 1817) |
| base-switches-32 | 0-7 | 32 | 14.407 | 16.105 | 4.553 | 7.133 | P 60; non-P none; end MHz 800–3537 (median 1624) |

Coordinator ruling of 2026-09-23: PASS if the candidate's pinned 32-minus-empty delta <= max(+4 ms, the same-session pinned base's delta + the base's launch spread), otherwise FAIL. The +4 ms allowance bounds the delta itself; the base only decides whether an excess over +4 belongs to the accepted picker. A base delta over +4 ms is recorded as a property of the accepted picker, not of this task.

## Against the same-session base

| Figure | base | candidate | candidate − base |
| --- | ---: | ---: | ---: |
| key6_ui_cpu_median_ms | 20.406 | 7.191 | -13.215 (-64.8%) |
| edit_pooled_p95_ms | 21.913 | 21.000 | -0.913 (-4.2%) |
| edit_pooled_median_ms | 19.657 | 18.404 | -1.253 (-6.4%) |
| ui_cpu_per_edit_median_ms | 21.675 | 20.620 | -1.055 (-4.9%) |
| idle_control_median_ms | 0.598 | 0.631 | +0.033 (+5.5%) |
| blink_median_draw_present_ms | 20.125 | 7.197 | -12.928 (-64.2%) |
| switch_ui_cpu_median_empty_ms | 10.902 | 12.457 | +1.555 (+14.3%) |
| switch_ui_cpu_median_32_ms | 14.407 | 16.463 | +2.056 (+14.3%) |
| apply_frame_median_32_ms | 4.553 | 4.232 | -0.321 (-7.1%) |
| apply_frame_p95_32_ms | 7.133 | 7.689 | +0.556 (+7.8%) |

Blink count: base 20, candidate 20. Candidate higher than base: apply_frame_p95_32_ms, idle_control_median_ms, switch_ui_cpu_median_32_ms, switch_ui_cpu_median_empty_ms.

## Regression against the same-session base (follow-up, not graded)

| Window (median UI-thread CPU, 150 ms, pinned) | store | base | candidate | candidate − base |
| --- | ---: | ---: | ---: | ---: |
| switch (release to +150 ms) | empty | 10.902 | 12.457 | +1.555 |
| switch (release to +150 ms) | 32 | 14.407 | 16.463 | +2.056 |
| hover control | empty | 10.954 | 12.381 | +1.427 |
| hover control | 32 | 14.103 | 15.226 | +1.123 |
| press control | empty | 10.498 | 12.300 | +1.802 |
| press control | 32 | 14.012 | 15.773 | +1.761 |
| idle control | empty | 0.607 | 0.593 | -0.014 |
| idle control | 32 | 0.601 | 0.579 | -0.022 |

The base's switch repeats reproduced its first pair within 0.133 ms (empty) and 0.062 ms (32 themes), so the candidate's excess on switch, hover and press frames is beyond the base's launch spread on both stores; it is store-independent in the main (about +1.1 to +1.8 ms on the empty store's 20 cards as on the 32-theme store's 52). Two likely causes, unverified hypotheses (no instrumented build was run): (1) the card clip follows the previous frame's hover state and requests one extra frame when a card's hover state changes, which the pointer's arrival on the card (hover window) and the click (press and switch windows) both trigger; (2) a click refreshes the window, so every cached ThemeCardBody and its deferred CardLayer draw is rebuilt, a per-frame cost the base's in-page cards do not have. The smallest check is a DIAG build counting frames and card-body builds per hover, press and switch window on both stores. Unpinned figures beside (not graded): this session's unpinned candidate launch key 6 8.877 ms and edit p95 25.529 ms; the draw-cost record (85a7d07) switch CPU 13.793 / 17.195 ms; the picker record (4cdd4df) 13.386 ms empty and 17.294 ms (32, picker cycle).

## Core placement and frequency

| Launch | window | by core | non-P windows | CPUs at end | end MHz median (min–max) |
| --- | --- | --- | --- | --- | --- |
| recorded-32 | key6 | {'P': 60} | none | {'0': 8, '1': 14, '2': 4, '3': 2, '4': 1, '5': 5, '6': 6, '7': 20} | 800 (800–3300) |
| recorded-32 | edit | {'P': 60} | none | {'0': 8, '1': 12, '2': 4, '3': 2, '4': 2, '5': 4, '6': 7, '7': 21} | 1833 (800–3073) |
| recorded-32 | idle | {'P': 60} | none | {'0': 4, '1': 12, '2': 4, '3': 2, '4': 3, '5': 1, '6': 11, '7': 23} | 800 (800–4459) |
| base-recorded-32 | key6 | {'P': 60} | none | {'0': 5, '1': 5, '2': 5, '3': 6, '4': 11, '5': 14, '6': 7, '7': 7} | 800 (800–4766) |
| base-recorded-32 | edit | {'P': 60} | none | {'0': 5, '1': 5, '2': 4, '3': 7, '4': 12, '5': 14, '6': 7, '7': 6} | 800 (800–4279) |
| base-recorded-32 | idle | {'P': 60} | none | {'0': 4, '1': 2, '2': 7, '3': 8, '4': 12, '5': 15, '6': 7, '7': 5} | 800 (800–5258) |
| warmup-32 | key6 | {'P': 60} | none | {'0': 20, '1': 9, '2': 3, '3': 7, '4': 2, '5': 7, '6': 4, '7': 8} | 1553 (800–3102) |
| warmup-32 | edit | {'P': 60} | none | {'0': 15, '1': 8, '2': 3, '3': 8, '4': 4, '5': 9, '6': 6, '7': 7} | 883 (800–4100) |
| warmup-32 | idle | {'P': 60} | none | {'0': 12, '1': 8, '2': 4, '3': 5, '4': 6, '5': 12, '6': 7, '7': 6} | 1776 (800–4916) |
| blink-32 | key6 | {'P': 60} | none | {'0': 17, '1': 6, '2': 6, '3': 11, '4': 3, '5': 5, '6': 1, '7': 11} | 800 (800–3099) |
| blink-32 | edit | {'P': 60} | none | {'0': 20, '1': 7, '2': 7, '3': 11, '4': 1, '5': 5, '7': 9} | 800 (800–4867) |
| blink-32 | idle | {'P': 60} | none | {'0': 18, '1': 6, '2': 3, '3': 9, '4': 6, '5': 8, '6': 3, '7': 7} | 1834 (800–5133) |
| base-blink-32 | key6 | {'P': 60} | none | {'0': 7, '1': 14, '2': 12, '3': 3, '4': 11, '5': 3, '6': 2, '7': 8} | 800 (800–3784) |
| base-blink-32 | edit | {'P': 60} | none | {'0': 6, '1': 13, '2': 14, '3': 2, '4': 10, '5': 2, '6': 3, '7': 10} | 800 (800–3722) |
| base-blink-32 | idle | {'P': 60} | none | {'0': 4, '1': 11, '2': 10, '3': 3, '4': 15, '5': 3, '6': 5, '7': 9} | 1074 (800–4850) |
| switches-32 | switch | {'P': 60} | none | {'0': 5, '1': 3, '2': 5, '3': 10, '4': 15, '5': 6, '6': 8, '7': 8} | 1833 (800–3706) |
| switches-32 | idle | {'P': 60} | none | {'0': 3, '1': 5, '2': 7, '3': 7, '4': 18, '5': 6, '6': 7, '7': 7} | 871 (800–3201) |
| switches-32 | hover | {'P': 60} | none | {'0': 5, '1': 3, '2': 5, '3': 9, '4': 17, '5': 5, '6': 7, '7': 9} | 1306 (800–2790) |
| switches-32 | press | {'P': 60} | none | {'0': 5, '1': 3, '2': 5, '3': 11, '4': 15, '5': 6, '6': 7, '7': 8} | 800 (800–3400) |
| switches-empty | switch | {'P': 60} | none | {'0': 5, '1': 4, '2': 14, '3': 7, '4': 6, '5': 5, '6': 14, '7': 5} | 800 (800–4295) |
| switches-empty | idle | {'P': 60} | none | {'0': 6, '1': 6, '2': 7, '3': 13, '4': 4, '5': 8, '6': 11, '7': 5} | 1440 (800–3547) |
| switches-empty | hover | {'P': 60} | none | {'0': 7, '1': 5, '2': 9, '3': 9, '4': 4, '5': 7, '6': 13, '7': 6} | 800 (800–4964) |
| switches-empty | press | {'P': 60} | none | {'0': 4, '1': 4, '2': 13, '3': 8, '4': 6, '5': 5, '6': 14, '7': 6} | 800 (800–2871) |
| base-switches-32 | switch | {'P': 60} | none | {'0': 11, '1': 7, '2': 4, '3': 6, '4': 3, '5': 6, '6': 20, '7': 3} | 1624 (800–3537) |
| base-switches-32 | idle | {'P': 60} | none | {'0': 9, '1': 8, '2': 4, '3': 6, '4': 4, '5': 3, '6': 17, '7': 9} | 1832 (800–4860) |
| base-switches-32 | hover | {'P': 60} | none | {'0': 11, '1': 8, '2': 5, '3': 6, '4': 4, '5': 3, '6': 19, '7': 4} | 1816 (800–5029) |
| base-switches-32 | press | {'P': 60} | none | {'0': 10, '1': 8, '2': 3, '3': 8, '4': 3, '5': 4, '6': 20, '7': 4} | 1656 (800–3051) |
| base-switches-empty | switch | {'P': 60} | none | {'0': 6, '1': 3, '2': 11, '3': 11, '4': 4, '5': 7, '6': 7, '7': 11} | 1817 (800–3086) |
| base-switches-empty | idle | {'P': 60} | none | {'0': 5, '1': 4, '2': 5, '3': 12, '4': 4, '5': 9, '6': 11, '7': 10} | 1855 (800–5287) |
| base-switches-empty | hover | {'P': 60} | none | {'0': 6, '1': 3, '2': 11, '3': 9, '4': 4, '5': 5, '6': 10, '7': 12} | 800 (800–5227) |
| base-switches-empty | press | {'P': 60} | none | {'0': 6, '1': 3, '2': 11, '3': 10, '4': 4, '5': 6, '6': 8, '7': 12} | 1710 (800–5185) |
| unpinned-recorded-32 | key6 | {'P': 38, 'E': 18, 'mixed': 4} | [1, 2, 3, 4, 5, 9, 16, 17, 19, 26, 27, 32, 33, 34, 35, 36, 37, 38, 39, 47, 50, 58] | {'0': 3, '1': 4, '2': 5, '3': 8, '4': 4, '5': 6, '6': 5, '7': 4, '8': 8, '9': 3, '12': 2, '14': 3, '16': 2, '17': 2, '22': 1} | 925 (800–3679) |
| unpinned-recorded-32 | edit | {'P': 36, 'E': 17, 'mixed': 7} | [1, 2, 3, 4, 5, 9, 10, 11, 13, 16, 17, 19, 26, 27, 32, 33, 34, 35, 36, 37, 38, 39, 50, 58] | {'0': 4, '1': 5, '2': 5, '3': 6, '4': 4, '5': 8, '6': 5, '7': 3, '8': 6, '9': 3, '10': 1, '12': 1, '13': 1, '14': 2, '15': 1, '16': 1, '17': 1, '21': 1, '22': 2} | 1637 (800–4597) |
| unpinned-recorded-32 | idle | {'P': 41, 'E': 19} | [1, 2, 3, 4, 5, 6, 12, 14, 17, 27, 28, 33, 34, 36, 37, 38, 40, 41, 47] | {'0': 2, '1': 5, '2': 4, '3': 2, '4': 5, '5': 9, '6': 10, '7': 4, '8': 4, '9': 4, '13': 2, '14': 1, '15': 2, '16': 1, '17': 1, '21': 2, '23': 2} | 1249 (800–4919) |

## Unpinned figures beside (not graded)

| Source | build | key 6 median | edit p95 | switch CPU empty | switch CPU 32 | delta | apply 32 median / p95 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 2026-09-22 draw-cost record | 85a7d07 | 27.983 | 25.372 | 13.793 | 17.195 | +3.402 | 3.699 / 7.921 |
| 2026-09-23 picker record | 4cdd4df | n/a | n/a | 13.386 | 17.294 (picker cycle) | n/a | 3.600 / 7.691 |
| this session, unpinned-recorded-32 | e73be2e | 8.877 | 25.529 | n/a | n/a | n/a | n/a |

The picker record's secondary edit pair (unpinned, information): base-edit-32 (85a7d07) p95 29.079, edit-pair-32 (4cdd4df) p95 33.024.

## What the values measure

- **ui_cpu_key6_ms**: Main-thread (UI) run time from /proc/<pid>/task/<pid>/schedstat, read immediately before the 6th key's XTest press and again 150 ms after the XSync that follows its release: X delivery, GPUI key dispatch, the kit Input's edit, InputEvent::Change into ThemeForm::hex_changed's invalid branch (no palette, no preview, no trace), the draw that shows the character with its present, and every other main-thread slice in the window (idle tick callbacks). CPU time, not latency: the tick grid moves when the draw falls inside the window, not its cost, so it sets no floor; the floor is the idle control over the same 150 ms. Worker threads excluded.
- **blink_frame**: GPUI's frame duration line (ZED_MEASUREMENTS=1): Window::draw, Window::present and the arena clear inside the platform frame callback, printed when the callback finds the window dirty; wall time. Excludes the wait from the blink's notify to the tick and GPU completion; present may include a swapchain acquire. Count = lines arriving inside the 10 s window; the caret toggles every 500 ms (gpui-base BlinkCursor), 20 toggles per 10 s.
- **gitturtle.theme_edit_frame_ms**: Stamped at entry to ThemeForm::hex_changed (theme_editor.rs:1568-1569, trace_stamp before the parse) inside the 7th key's InputEvent::Change; a valid value reaches draft_changed (line 1632) and preview_theme_draft (line 575), which keeps the first stamp of the burst in State::edit_started (line 586) and calls apply_appearance (line 593). The root render (views.rs:2459-2464) takes the stamp after Root::render_dialog_layer and appends edit_trace_probe (theme_editor.rs:625-659): a zero-size canvas deferred at priority usize::MAX (EDIT_TRACE_PRIORITY, line 322), whose paint closure prints the line (line 637). It is the last element Window::draw paints in the first frame drawn after the draft, whether the platform tick or Window::dispatch_key_event draws it. The value holds the parse, the readability recompute, apply_appearance and the whole window's request_layout, layout, prepaint and paint with the dialog over it; it excludes next-frame callback waits, frame-finish bookkeeping, present, pre-handler input delivery and GPU execution. The frame-tick grid does not enter the value.
- **switch_ui_cpu**: Main-thread schedstat delta from just before the XTest button release on a picker card to 150 ms after it, as in the themes-draw-cost record; not bounded by the tick grid.
- **gitturtle.theme_apply_frame_ms**: Unchanged in kind, 45 lines lower in settings.rs than on 23ee777 (rows_off_boundary was added above choose_theme): stamped at choose_theme entry (settings.rs:547); apply_appearance (line 554), save_preferences (line 555), then trace_next_frame (line 557, main.rs:1353) registers a next-frame callback that prints. Next-frame callbacks run only on the platform frame tick, before that frame is laid out and painted, so the value is the handler plus the wait to the next tick of the 8.334 ms X11 grid and is blind to the draw. It cannot fall below the handler's own cost, and its spread is the keystroke's phase on the grid; UI-thread CPU per switch is the instrument for the frame's cost.
- **placement**: cpu_place.place() immediately before a window's opening schedstat read and immediately after its closing one: the CPU the UI thread last ran on (stat field 39) and that CPU's scaling_cur_freq and cpuinfo_avg_freq at the read. A window is P when both reads are P-cores.

Trace-boundary lines on the candidate source: {"source": "/home/fernandoramirez/Documents/GitTurtle/.local/worktrees/claude-code-support/.local/themes-evidence/src-e73be2e", "theme_editor.rs": {"hex_changed": 1714, "trace_stamp": 1716, "preview_theme_draft": 576, "edit_trace_probe": 626, "probe_eprintln": 638}, "settings.rs": {"choose_theme": 702, "stamp": 708, "apply_appearance": 721, "save_preferences": 722, "trace_call": 724, "settings_page_element": 213, "MiniatureLayer": null, "notify_settings_page": 248}, "views.rs": {"settings_page_element_call": 1979}, "main.rs": {"trace_next_frame": 1443}}.

## Noise

Pinning removed the core mix that decided earlier medians: in the unpinned launch of the same executable (`unpinned-recorded-32`, 14:05:57Z) 18 of 60 key-6 windows ran on E-cores and 4 were mixed, key 6 rose to 8.877 ms (P-core windows 8.417, E-core 8.877) and the edit p95 to 25.529 ms (one edit over 26), against 7.191 and 21.000 pinned. The pinned repetitions agree: key 6 7.418 (warm-up), 7.191 (graded) and 7.726 (blink launch, frames mode); edit p95 21.127, 21.000 and 19.744; base key 6 20.406 and 20.957, base edit p95 21.913 and 20.978; blink window medians 5.332 to 7.457 against the base's 20.125 to 21.794. The base's switch repeats (base-switches-empty-b and base-switches-32-b, 14:08Z to 14:15Z) reproduced its first pair within 0.133 ms (empty) and 0.062 ms (32 themes), so the base delta's launch spread is 0.195 ms (+3.505 then +3.700); the candidate has one designated pair, whose delta +4.006 has a bootstrap 95% interval of 3.474 to 4.408 ms (base 3.033 to 4.607): the 0.006 ms excess over 4.000 is noise-sized, while the candidate's per-store excess over the base (+1.555 and +2.056 ms) is ten times the base's launch spread. The idle controls (0.55 to 0.66 ms per 150 ms) show no contention; loads were 0.77 to 1.66 at every graded and base start. The frequencies read at window boundaries are mostly 800 MHz because the core has gone idle by the read; they identify the core type, not the draw's clock. Governor powersave, EPP balance_performance, AC power, turbo on. One graded launch per figure, warm caches, synthetic XTest input, XWayland on GNOME Wayland only.

## Limitations

- taskset pinned the UI thread and 30-32 other threads to CPUs 0-7; six Mesa threads (disk$0-3 and the shader compiler sh0) set their own affinity to 0-23 after the pin (affinity.other_list_threads); they do not enter UI-thread metrics.
- The frequency read after a window is the CPU's recent average at the read (scaling_cur_freq / cpuinfo_avg_freq): on a core that went idle after the draw it reads about 800 MHz, so it records the core's state at the boundary, not the draw's frequency.
- G5 (switch delta) misses the coordinator's threshold max(+4 ms, base delta + base launch spread) = 4.000 ms by 0.006 ms (+4.006), far inside one launch pair's sampling error (bootstrap 95% interval of the candidate delta in the record); one designated pair per build, no candidate repeat.
- Loads: the 1-minute load was 11.4 at the handover (13:30:55Z, the coordinator's release build had just finished); the first launch waited until 2.84 (13:32:39Z); every graded and base launch started between 0.80 and 1.66.
- Linux XWayland on GNOME Wayland only (not native Wayland, not macOS); synthetic XTest keys and clicks; 120 Hz panel at 2x.
- Pinned to the P-cores: the figures are the P-core case; the unpinned launch and records are beside them, not graded.
- One graded launch per figure; warm OS caches; uncontrolled desktop load on the owner's session (loads in launches[]).
- These numbers bind to executable sha256 f7f6aa14146ee481e65d843d8645541b55060a2460604c1926cc78ec76330b73 only; the base figures to 3ef3e3a0b999b00b76c64d2177f82a93814faf585de5452439bfbcd34894714e.
- Blink draw cost is GPUI's frame duration (draw + present, wall time); the release build has no draw-only trace.

## Reproduction

```sh
# tools-a3 (perf-settings-view-a3); one preset per call, in plan.md order, each after the previous exited
bash tools-a3/launch.sh <preset>
cd tools-a3 && python3 analyze_editor.py recorded-32 base-recorded-32 warmup-32 blink-32 base-blink-32 unpinned-recorded-32
python3 analyze_blink.py blink-32 base-blink-32
python3 analyze_switches.py switches-empty switches-32 base-switches-empty base-switches-32
python3 build_record_a3.py --date 2026-09-23 && python3 render_record_md_a3.py ../record/2026-09-23-theme-settings-view-keystroke.json
```
