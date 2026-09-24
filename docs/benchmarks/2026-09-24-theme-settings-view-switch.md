# Theme Settings page: switch, hover and press cost after the card prepaint fix, pinned to the P-cores, 2026-09-24

This records the release measurement for follow-up SV-PERF (item 19 of the [themes follow-ups](../development/themes/follow-ups.md)) on build `fbc4555` (`perf(settings): prepaint picker cards under the pointer state of the input's own frame`) against the designated same-session base `ab29fdd`, the accepted picker head. It supersedes the switch, hover and press figures and the switch delta (G5) of the [2026-09-23 settings-view record](2026-09-23-theme-settings-view-keystroke.md), where G5 measured +4.006 ms against 4.000 ms and passed only by coordinator ruling, and re-measures that record's key-6, edit-frame and blink figures beside. The diagnosis behind the fix (local bundle `diag-sv-perf-d099f43`, counter builds, timings not graded) counted two frames per hover, press and switch on the settings-view build `d099f43` and one with the fix, as on the base. Raw samples are in [the JSON](2026-09-24-theme-settings-view-switch.json).

`$EVIDENCE/` below is `<worktree>/.local/themes-evidence/perf-sv-fix-fbc4555/` (the local, unversioned bundle: plan, builds, runs, logs and tools).

## Result

Every graded launch ran pinned to CPUs 0-7 and every measured window ran on a P-core. On the letter, with no ruling, every criterion holds: **yes**.

| Criterion (plan.md section 3) | Measured | Bound | Holds |
| --- | ---: | ---: | --- |
| SV-1 empty/switch: candidate - base median | -0.155 ms | <= base spread 0.128 ms | **yes** |
| SV-1 empty/hover: candidate - base median | -0.660 ms | <= base spread 0.273 ms | **yes** |
| SV-1 empty/press: candidate - base median | -0.115 ms | <= base spread 0.019 ms | **yes** |
| SV-1 32/switch: candidate - base median | -0.108 ms | <= base spread 0.285 ms | **yes** |
| SV-1 32/hover: candidate - base median | -1.726 ms | <= base spread 0.671 ms | **yes** |
| SV-1 32/press: candidate - base median | -0.174 ms | <= base spread 0.132 ms | **yes** |
| G5 switch delta, 32 themes - empty (candidate) | +3.703 ms | <= 4.000 ms | **yes** |
| G4 switch UI CPU median, empty store | 10.948 ms | <= 23 ms | **yes** |
| G6 apply frame median / p95, empty | 3.822 / 8.010 ms | <= 8 / <= 16 ms | **yes** |
| G6 apply frame median / p95, 32 themes | 3.378 / 7.589 ms | <= 8 / <= 16 ms | **yes** |
| G1 key-6 UI CPU median | 7.496 ms | <= 14 ms | **yes** |
| G2 theme_edit_frame_ms pooled p95 (57th of 60) | 21.745 ms | <= 26 ms | **yes** |

G5 in detail: candidate 14.651 - 10.948 = +3.703 ms (bootstrap 95% 3.322 to 4.852); base +3.655, base repeat +3.498, spread 0.157; candidate minus base +0.047 ms; candidate repeat +3.831 (reported only). The a3 formula max(+4, base delta + spread) gives 4.000 ms.

## Switch, hover and press against the same-session base

UI-thread CPU per 150 ms window, ms, 60 windows per launch. d = candidate median - base median; spread = |base repeat - base|.

| Store / window | base median / p95 / max | base repeat median | candidate median / p95 / max | candidate repeat median | d | spread | d bootstrap 95% | holds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| empty/switch | 11.104 / 12.439 / 16.922 | 10.976 | 10.948 / 12.457 / 13.936 | 10.979 | -0.155 | 0.128 | -1.310 to 0.586 | **yes** |
| empty/hover | 10.547 / 11.954 / 12.871 | 10.820 | 9.887 / 11.383 / 12.229 | 10.081 | -0.660 | 0.273 | -1.229 to 0.036 | **yes** |
| empty/press | 10.665 / 12.163 / 12.845 | 10.646 | 10.550 / 12.343 / 13.235 | 10.794 | -0.115 | 0.019 | -1.655 to 1.076 | **yes** |
| empty/idle | 0.586 / 1.152 / 1.339 | 0.615 | 0.589 / 1.102 / 1.421 | 0.570 | +0.003 | 0.029 | -0.042 to 0.030 | control |
| 32/switch | 14.759 / 16.254 / 21.439 | 14.474 | 14.651 / 16.647 / 25.037 | 14.809 | -0.108 | 0.285 | -0.507 to 0.514 | **yes** |
| 32/hover | 14.697 / 15.708 / 16.705 | 14.026 | 12.971 / 14.071 / 15.516 | 12.922 | -1.726 | 0.671 | -2.120 to 0.489 | **yes** |
| 32/press | 14.332 / 15.488 / 16.786 | 14.200 | 14.157 / 15.991 / 16.434 | 14.385 | -0.174 | 0.132 | -0.601 to 0.330 | **yes** |
| 32/idle | 0.607 / 1.230 / 1.459 | 0.597 | 0.651 / 1.286 / 1.488 | 0.593 | +0.044 | 0.010 | -0.057 to 0.181 | control |

Tails, candidate p95 minus base p95 (reported, no bound): empty/switch +0.018; empty/hover -0.571; empty/press +0.180; 32/switch +0.393; 32/hover -1.637; 32/press +0.503. The largest single window was a 32-theme switch of 25.037 ms on the candidate (base max 21.439).

Against the mean of both base launches (reported): empty/switch -0.091; empty/hover -0.796; empty/press -0.106; 32/switch +0.034; 32/hover -1.391; 32/press -0.108.

Apply frame (`gitturtle.theme_apply_frame_ms`, median / p95 / max, ms): switches-empty 3.822 / 8.010 / 8.406; switches-32 3.378 / 7.589 / 8.184; base-switches-empty 3.509 / 7.575 / 7.987; base-switches-32 3.608 / 7.945 / 8.045; switches-empty-b 3.825 / 7.806 / 7.991; switches-32-b 4.021 / 7.377 / 7.915; base-switches-empty-b 4.020 / 7.803 / 7.970; base-switches-32-b 2.774 / 7.773 / 7.890.
Worker jobs woken per switch: 0 in every switch launch; every switch stored the target theme and moved the accent.

## Before and after

Pinned, each against its own same-session base: 2026-09-23 is `e73be2e` against `4cdd4df` ([record](2026-09-23-theme-settings-view-keystroke.md)); 2026-09-24 is `fbc4555` against `ab29fdd` (this record).

| Store / window | 2026-09-23 d (spread) | 2026-09-24 d (spread) | change in d |
| --- | ---: | ---: | ---: |
| empty/switch | +1.555 (0.133) | -0.155 (0.128) | -1.710 |
| empty/hover | +1.427 (0.061) | -0.660 (0.273) | -2.087 |
| empty/press | +1.802 (0.315) | -0.115 (0.019) | -1.917 |
| 32/switch | +2.056 (0.062) | -0.108 (0.285) | -2.164 |
| 32/hover | +1.123 (0.028) | -1.726 (0.671) | -2.849 |
| 32/press | +1.761 (0.154) | -0.174 (0.132) | -1.935 |

G5 switch delta: +4.006 ms (failed on the letter, passed by ruling) before; +3.703 ms (passes on the letter) after.

## Edit path, key 6 and blink

The badge ring (153d442) makes a canvas edit rebuild the selected card's body, so the dialog path is re-measured (`recorded-32` against `base-recorded-32`, 60 edits on the 32-theme store).

| Figure | base median / p95 / max | candidate median / p95 / max | d median | d bootstrap 95% |
| --- | ---: | ---: | ---: | ---: |
| key-6 UI CPU | 21.267 / 22.790 / 26.157 | 7.496 / 8.646 / 9.306 | -13.771 | -14.470 to -13.259 |
| theme_edit_frame_ms | 20.383 / 22.436 / 24.027 | 18.803 / 21.745 / 22.068 | -1.579 | -2.406 to -0.844 |
| UI CPU per edit | 22.747 / 25.499 / 26.151 | 21.425 / 23.090 / 23.672 | -1.322 | -2.068 to -0.626 |

Pooled edit p95 (57th of 60): candidate 21.745, base 22.436 (-0.691 ms); 0 of 60 over 26 in either. Idle control median 0.601 / base 0.624 ms.
Caret blink, set 1's 10 s idle window (reported): candidate 20 frames, draw + present median 7.172 ms (p95 7.439); base 20 frames, median 20.523 ms (p95 23.289).

## Grading method and designation

Base designated in `$EVIDENCE/plan.md` before any launch: ab29fdd, the accepted picker head: themes-settings-view was attempted on it and the 2026-09-23 attestation graded G5 and its regression lines against executable 4cdd4df, whose crates/, Cargo.toml, Cargo.lock and vendor/ are blob-identical to ab29fdd. Rebuilt in this session with the candidate's toolchain and target dir. The designation file `$EVIDENCE/runs/criterion-set.txt` was written at 2026-09-24T01:11:10Z, before the first graded or base launch (2026-09-24T01:11:25Z), after the explore and warm-up launches confirmed the geometry. Every launch ran the app under `taskset -c 0-7` (P-cores; 8-23 are E-cores); a launch counts only if the UI thread's Cpus_allowed_list was 0-7 when the window appeared and at the end and both placement reads of every window were on a P-core. The launch order alternated base and candidate (ABAB) so session drift falls on both. SV-1 and G5 are judged on unrounded medians.

## Launches

| Launch | build | store | started (UTC) | load 1/5/15 before | UI-thread pin at window / end | windows on P | exit | role |
| --- | --- | ---: | --- | --- | --- | --- | ---: | --- |
| explore-sw-32 | fbc4555 | 32 | 2026-09-24T01:07:00Z | 2.35 / 5.94 / 3.89 | 0-7 / 0-7 | all | 0 | geometry, not eligible |
| warmup-32 | fbc4555 | 32 | 2026-09-24T01:08:01Z | 1.57 / 5.06 / 3.71 | 0-7 / 0-7 | all | 0 | editor geometry, not eligible |
| base-switches-empty | ab29fdd | 0 | 2026-09-24T01:11:25Z | 1.76 / 3.35 / 3.29 | 0-7 / 0-7 | all | 0 | base control |
| switches-empty | fbc4555 | 0 | 2026-09-24T01:14:48Z | 0.73 / 2.20 / 2.86 | 0-7 / 0-7 | all | 0 | graded SV-1, G4, G5, G6 |
| base-switches-32 | ab29fdd | 32 | 2026-09-24T01:18:11Z | 1.65 / 1.74 / 2.53 | 0-7 / 0-7 | all | 0 | base control |
| switches-32 | fbc4555 | 32 | 2026-09-24T01:21:34Z | 1.26 / 1.51 / 2.28 | 0-7 / 0-7 | all | 0 | graded SV-1, G5, G6 |
| base-switches-empty-b | ab29fdd | 0 | 2026-09-24T01:24:57Z | 1.40 / 1.32 / 2.04 | 0-7 / 0-7 | all | 0 | base control (repeat, SV-1 spread) |
| switches-empty-b | fbc4555 | 0 | 2026-09-24T01:28:20Z | 1.25 / 1.22 / 1.85 | 0-7 / 0-7 | all | 0 | candidate repeat, reported |
| base-switches-32-b | ab29fdd | 32 | 2026-09-24T01:31:43Z | 1.30 / 1.14 / 1.68 | 0-7 / 0-7 | all | 0 | base control (repeat, SV-1 spread) |
| switches-32-b | fbc4555 | 32 | 2026-09-24T01:35:06Z | 1.27 / 1.21 / 1.60 | 0-7 / 0-7 | all | 0 | candidate repeat, reported |
| base-recorded-32 | ab29fdd | 32 | 2026-09-24T01:38:29Z | 1.13 / 1.08 / 1.47 | 0-7 / 0-7 | all | 0 | base control |
| recorded-32 | fbc4555 | 32 | 2026-09-24T01:41:28Z | 1.55 / 1.33 / 1.49 | 0-7 / 0-7 | all | 0 | graded G1, G2 |
| base-blink-32 | ab29fdd | 32 | 2026-09-24T01:44:26Z | 1.13 / 1.21 / 1.41 | 0-7 / 0-7 | all | 0 | base control |
| blink-32 | fbc4555 | 32 | 2026-09-24T01:48:27Z | 0.97 / 1.16 / 1.35 | 0-7 / 0-7 | all | 0 | reported G3 |

## Builds and environment

- Candidate `fbc4555b81d7ed28f3a63fe7236c3f77fbae7d71`, sha256 `0184f0d96436c37b8427a4171b535b1be3d0f2572795bd8c99abd1f83c09699f`; base `ab29fdd0cfcfd44d8f38662feb06bf4e9c7f6940`, sha256 `3500f4d036293dc5c185ea50dd331c9b809f3fde50e19594a8b2a36a735953e0`. Both clean release builds, rustc 1.98.0 (88d9e12ae 2026-08-18), `--locked`, from clones in `$EVIDENCE/src-<sha>` with one target dir (cargo build --release --locked -p gitturtle, one CARGO_TARGET_DIR, candidate then base, 2026-09-24T01:01-01:04Z).
- Host: Intel(R) Core(TM) Ultra 9 275HX (P-cores 0-7, E-cores 8-23), 188.1 GiB, Pop!_OS 24.04 LTS, Linux 7.1.5-76070105-generic, governor powersave, EPP balance_performance, AC 1, turbo on (no_turbo 0); GNOME Shell 46.0 Wayland session, the app on XWayland :1 at GPUI_X11_SCALE_FACTOR=2, 1480x980 logical (2960x1960 physical) window; panel eDP-1 3840x2400 at 119.98 Hz; AT-SPI IsEnabled false.
- Host quiet: 18 checks before launches, 0 with a compiler process; graded and base launches started at a 1-minute load of at most 1.76. The only process a host state listed was this role's own shell (pid and name in the JSON); no other GitTurtle or driver process ran on the display.
- Fixture: themes-evidence/perf-theme-apply/repo (the themes-apply-trace fixture) at `c8f20cfeb3f846168abbdcde270983c42bb27e71`; History 1-1000, src/generated/large-module.ts in Compare Split, retained behind Settings; clean before and after. Stores: seed.py 32 themes `9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a`, empty `ae1731ac8307019b39aee6a216564410b55b656871ddb1b9d552e8e988e4e208`.
- Cache state: warm OS page cache: explore-sw-32 (01:07Z) and warmup-32 (01:08Z) ran first; application caches empty per launch (fresh HOME/XDG); both executables were built minutes before and read from the page cache.

## What the values measure

- **switch_ui_cpu**: Main-thread (UI) run time from /proc/<pid>/task/<pid>/schedstat, read just before the XTest button release on a picker card and 150 ms after it: the release's dispatch, choose_theme, the frame that shows the new theme and its present, and any idle tick work in the window. CPU time, not latency: the 8.334 ms tick grid moves when the frame falls, not its cost.
- **hover_control**: The same 150 ms UI-thread window after the pointer moves onto the target card (the hover-state frame), inside the quiet period before the press.
- **press_control**: The same 150 ms UI-thread window after the XTest button press (the pressed-state frame), before the release.
- **idle_control**: 150 ms of the quiet period with no input: the floor of the window (about 0.6 ms).
- **gitturtle.theme_apply_frame_ms**: choose_theme entry to the next-frame callback: the handler plus the phase on the 8.334 ms grid, blind to the draw; p95 <= 16 is decided by the boundary unless the handler nears 8 ms.
- **ui_cpu_key6_ms**: UI-thread run time from just before the 6th key's XTest press to 150 ms after its release: a hex-field edit without a valid colour (no preview).
- **gitturtle.theme_edit_frame_ms**: hex_changed entry to the paint of the probe deferred above the dialog layer (no callback wait, no present; no grid floor).
- **blink_frame**: GPUI's frame duration line (ZED_MEASUREMENTS=1): Window::draw + present inside the frame callback, per caret toggle in a 10 s idle window.
- **placement**: cpu_place.place() before a window's opening schedstat read and after its closing one: the CPU the UI thread last ran on (stat field 39) and that CPU's scaling_cur_freq; the end read is often 800 MHz because the core has idled by then.
- Source lines: candidate {"settings.rs choose_theme": 702, "settings.rs trace_next_frame call": 724, "main.rs trace_next_frame": 1461, "settings.rs CardProbe": 2450}; base {"settings.rs choose_theme": 575, "settings.rs trace_next_frame call": 597, "main.rs trace_next_frame": 1380}.

## Noise and limitations

- G5 passes on the letter at +3.703 ms, but its bootstrap 95% interval (3.322 to 4.852 ms) contains the 4.000 bound, and the reported candidate repeat measured +3.831 ms. The store-dependent part belongs to the accepted picker: the base's own delta was +3.655 (repeat +3.498) ms and the candidate's exceeds it by +0.047 ms.
- One graded launch per build and store (60 windows each); the bootstrap 95% intervals of the per-window differences are wider than the base's launch spread, so SV-1 passes because every difference is at or below zero, not because the method resolves 0.1 ms.
- The base is rebuilt ab29fdd, not the 4cdd4df executable of the superseded record; the two are blob-identical in crates/, Cargo and vendor/ but are different binaries, so the 2026-09-23 and 2026-09-24 absolute figures come from different sessions and executables. Compare differences, not absolutes, across the two records.
- Linux XWayland at scale 2 on one host; no macOS, other scale, or unpinned measurement in this record.
- Window-end frequencies are the idle core's reading; core type is the placement evidence.
- Six Mesa threads (disk$0-3, sh0) reset their own affinity to 0-23 after taskset in every launch; they are outside the UI-thread metrics.

## Reproduction

- `bash $EVIDENCE/build.sh`
- `bash $EVIDENCE/tools/run_sequence.sh <presets of plan.md section 6>`
- `cd $EVIDENCE/tools && python3 -B analyze_switches.py ... && python3 -B analyze_editor.py ... && python3 -B analyze_blink.py ...`
- `python3 -B $EVIDENCE/tools/grade_sv_fix.py > $EVIDENCE/runs/grade.json && python3 -B $EVIDENCE/tools/build_record_sv_fix.py`
