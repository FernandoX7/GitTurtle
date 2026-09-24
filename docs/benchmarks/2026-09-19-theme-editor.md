# Theme editor live-preview trace, September 19, 2026

This records the release measurement of [`gitturtle.theme_apply_frame_ms`](metrics.md) for custom theme editor edits (task `themes-editor`, criterion `no-reprepare-budget`) on build `3927b57`, which applies the first draft of a frame in the edit handler. Method and format follow the [theme application baseline](2026-09-18-theme-apply.md). Two earlier readings of the same criterion exist and are retained with their run data outside the repository, not under `docs/benchmarks/`: build `bb9f332`, which deferred the application to the next frame callback, and build `cd563f6`, which carries this build's live-preview code without its editor polish (`perf-theme-editor/record/2026-09-18-theme-editor.md` and `perf-theme-editor/record-cd563f6/2026-09-19-theme-editor.md` in the themes evidence directory). This record replaces neither; it states the cost on the executable named below. The raw data, including every sample, per-launch summaries and the attribution data, is in [2026-09-19-theme-editor.json](2026-09-19-theme-editor.json).

By the [owner decision](../development/themes/spec.md#performance) of 2026-09-19 the 16 ms p95 edit budget belongs to `themes-draw-cost`, not to the editor. This record gates nothing; it states the cost, its boundary and its dominant term, and reports p95 for information.

**Result: an edit costs about a third less than on `bb9f332` and the same as on `cd563f6`, and p95 is still above 16 ms.**

- **Criterion set** (recorded launch, set 1, designated in writing before the launch ran; n = 10): min 24.275 ms, median **29.614 ms**, p95 **36.048 ms**, max 36.048 ms. With nearest rank, p95 of 10 samples is the 10th smallest, which is the maximum. p95 ≤ 16 ms: **no**.
- **All six sets of that launch** (n = 60): min 20.960 ms, median 29.510 ms, p95 36.510 ms, max 38.513 ms. Every one of the 60 edits exceeded 16 ms.
- **Against `cd563f6`**: the criterion set's median is 0.181 ms higher and its p95 1.357 ms higher, both inside the spread between launches of either build, and a same-day re-measurement of the `cd563f6` executable came out slower than every launch of this build. This build's editor polish has no cost this harness can resolve.
- **Against `bb9f332`**: the criterion set's median fell 15.641 ms (−34.6%) and its p95 fell 20.546 ms (−36.3%).

The edit value does not end where a Settings switch's value ends; see [What the value measures](#what-the-value-measures) before comparing it with the switch baseline.

## Exercised build and environment

`--build-info` of the exercised executable (read by the driver at the start of every launch):

```json
{
  "application": "GitTurtle",
  "version": "0.1.0",
  "source_revision": "3927b57bbb913e35ee4a8b48f12b5c7eaa19f686",
  "source_tree": "clean",
  "target": "x86_64-unknown-linux-gnu",
  "profile": "release",
  "rustc": "rustc 1.98.0 (88d9e12ae 2026-08-18)",
  "build_unix_seconds": "1789844613"
}
```

The executable SHA-256 was `1faff02061c9f07500cbf74827ae61d425d145caef6a153682230ef239bbe593`. This record applies to that executable only, and it is rebuilt once this evidence lands on the base.

Against `cd563f6` this build changes four things that touch what a Settings frame draws while the dialog is open: the token column shows an always-visible vertical scrollbar in a reserved 16 px track, the hex field widens from 70 to 78 logical px so a full seven-character value no longer scrolls its leading `#` out of view, a row whose value is not `#rrggbb` carries a square cross glyph, and the Your themes card caches its readability counts per theme and palette. The first three are drawn in every frame measured here. The fourth is not exercised at all: the scratch store holds no custom themes, so the card renders "No custom themes yet." and the cached loop is empty.

Host: the same machine as the apply baseline and both earlier editor records. It has an Intel Core Ultra 9 275HX (24 threads) and 188 GiB RAM, and runs Pop!_OS 24.04 LTS, Linux `7.1.5-76070105-generic` and Git 2.43.0. The app ran as an X11 client of XWayland (`DISPLAY=:1`, `WAYLAND_DISPLAY` unset) on GNOME Shell 46.0 Wayland. The eDP-1 panel runs 3840×2400 at 119.98 Hz with a Mutter scale of 2.0. The app used `GPUI_X11_SCALE_FACTOR=2` at its default 1480×980 logical window (2960×1960 physical), which was not resized. The renderer was not re-verified in these launches; the apply baseline found the Intel ARL iGPU (i915, Mesa 26.1.6).

The machine was on AC, with the `powersave` governor and `balance_performance` EPP. The owner's desktop session stayed active, so background load was uncontrolled. Load average was 0.80/1.21/1.14 before the recorded launch and 1.03/1.06/1.09 after it; the five launches began between 0.50 and 1.88.

Cache state: warm. One exploratory launch (one dialog, two edits) and one warm-up launch (60 edits) of the same executable on the same fixture preceded the recorded launch, and caches were not dropped. The warm-up is summarised below but was not eligible to supply the criterion's samples. Each launch used a fresh process and a fresh, absolute scratch preference store under the run directory, seeded as `{version 5, theme midnight}`. No cold-cache run was made.

## Fixture and state before the first edit

The fixture is the `themes-apply-trace` fixture, unmodified: 1,207 commits on `main`, HEAD `c8f20cfeb3f846168abbdcde270983c42bb27e71`. It was clean with the same HEAD after all launches.

| Precondition | How it was verified (recorded launch) |
| --- | --- |
| At least 1,000 commits loaded | The Older control printed `gitturtle.history_page_frame_ms=6.568`, and the header then read "All history 1–1000". |
| 2,000-line comparison in Split | Activating `large-module.ts` on row 1 (`c8f20cf`) printed `gitturtle.file_preview_frame_ms=5.154`. Split was then chosen and captured with both panes showing. |
| Comparison retained behind Settings | After the six dialogs, Back showed the same Split comparison with **no** new `file_preview_frame_ms` line. A second Back showed History at "1–1000" with row 1 selected. |

Every launch printed zero new preview lines after Settings, so no edit re-requested the comparison.

## Procedure

The driver needed no change for this build. The wider hex field and the reserved scrollbar track move the rows, not the two sampled boxes, and neither the scrollbar nor the invalid glyph is a tab stop. Its navigation, the Tab order to "New theme…", the Tab order to the Canvas hex field and its pixel boxes were re-verified by an exploratory launch and then by every per-edit assertion of the four recorded launches.

1. Ctrl+comma opened Settings over the comparison, and 15 wheel steps brought "Your themes" on screen.
2. Tab ×28 focused "New theme…".
3. Space opened New theme. Its base was Midnight, the active theme, and the Readability list was empty. Tab ×2 then reached the Canvas hex field.
4. Each **edit** was Ctrl+A followed by typing `#rrggbb`: 60 ms between keys, and 150 ms before the 7th key. `parse_hex` (`appearance/custom.rs`, line 265) accepts exactly six hex digits with an optional leading `#`, so the value stayed invalid through the 6th key and became valid at the 7th, and exactly one preview followed it. On this build the focused field shows the whole value, which the previous record reported clipped, and the invalid rows of keys 1–6 carry the new cross glyph; neither changes the value or the measurement.
5. At least 1.2 s without input preceded every edit.
6. The typed colours cycled through 12 dark canvases, `#141a26` to `#111b28`. Each keeps Midnight's empty Readability list, so every edit renders the same rows.
7. Six dialogs of ten edits ran in one launch. Escape closed each dialog, which drops the draft and re-applies Midnight without a trace line. Space then reopened it for the next set.

Every edit was verified before the next one:

- **Before the valid key:** keys 1–6 printed no trace line.
- **After it:**
  - exactly one new `gitturtle.theme_apply_frame_ms=` line appeared;
  - the dialog's title strip sampled 100% the typed colour;
  - the Settings page margin sampled a single colour equal to the typed colour under the modal's 20% black overlay (±1 per channel).

Every set was also verified: the open printed exactly one line; the scratch store still named `midnight` with no custom themes; Escape restored the Midnight canvas and printed nothing.

All five launches passed every check: 66 lines each (6 opens and 60 edits).

Percentiles use `statistics.median` and nearest rank for p90/p95, as in the baseline.

## Samples

Values are in milliseconds, recorded launch. Set 1 is the criterion's set; its typed colour is shown in parentheses. Every set cycles the same list from the next colour.

| Edit | Set 1 | Set 2 | Set 3 | Set 4 | Set 5 | Set 6 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 30.472 (#141a26) | 32.009 | 28.128 | 32.378 | 26.499 | 35.659 |
| 2 | 35.413 (#0c1119) | 28.531 | 30.079 | 29.192 | 35.972 | 31.550 |
| 3 | 31.905 (#161c2a) | 32.562 | 27.047 | 33.983 | 38.385 | 27.093 |
| 4 | 27.062 (#0e1320) | 22.388 | 24.843 | 21.607 | 36.182 | 25.534 |
| 5 | 26.287 (#121824) | 31.371 | 36.510 | 29.828 | 28.912 | 29.030 |
| 6 | 28.553 (#0a0f16) | 26.534 | 34.439 | 23.443 | 24.479 | 25.077 |
| 7 | 36.048 (#181e2c) | 30.445 | 35.909 | 28.776 | 38.513 | 37.953 |
| 8 | 28.757 (#10161d) | 24.317 | 27.917 | 25.743 | 30.897 | 33.912 |
| 9 | 24.275 (#131627) | 35.755 | 20.960 | 26.628 | 34.803 | 28.593 |
| 10 | 35.056 (#0d151b) | 30.626 | 23.116 | 30.713 | 25.866 | 32.605 |

| Set | n | min | median | mean | p90 | p95 | max | p95 ≤ 16 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| **1 (criterion)** | 10 | 24.275 | 29.614 | 30.383 | 35.413 | **36.048** | 36.048 | no |
| 2 | 10 | 22.388 | 30.535 | 29.454 | 32.562 | 35.755 | 35.755 | no |
| 3 | 10 | 20.960 | 28.023 | 28.895 | 35.909 | 36.510 | 36.510 | no |
| 4 | 10 | 21.607 | 28.984 | 28.229 | 32.378 | 33.983 | 33.983 | no |
| 5 | 10 | 24.479 | 32.850 | 32.051 | 38.385 | 38.513 | 38.513 | no |
| 6 | 10 | 25.077 | 30.290 | 30.701 | 35.659 | 37.953 | 37.953 | no |
| Pooled | 60 | 20.960 | 29.510 | 29.952 | 35.972 | 36.510 | 38.513 | no |

The four launches of this executable agree (n = 60 edits each), and the fifth line is the `cd563f6` executable re-measured after them under the same conditions:

| Launch | median | p95 | max | UI-thread CPU per edit, median |
| --- | ---: | ---: | ---: | ---: |
| Warm-up | 30.124 | 35.987 | 39.411 | 25.703 |
| **Recorded** | 29.510 | 36.510 | 38.513 | 23.370 |
| Attribution (`ZED_MEASUREMENTS=1` and 1 ms `/proc` polling) | 28.921 | 36.041 | 40.440 | 23.048 |
| Control with no retained comparison | 29.358 | 35.509 | 37.755 | 23.122 |
| Same-day re-measurement of `cd563f6` (not this build) | 30.385 | 37.250 | 37.834 | 26.480 |

The four launches of this build span 1.203 ms of median and 1.001 ms of p95, about a quarter of one sample's standard deviation (4.543 ms in the recorded launch).

**Opens are traced too.** `open_theme_editor` also calls `preview_theme_draft` (`theme_editor.rs`, line 252), so each dialog open prints one line under the same name. There were six per launch; in the recorded launch they were 24.767–30.886 ms, median 27.331 ms. They are excluded from the edit samples.

## What the value measures

The stamp is taken in `preview_theme_draft` (`crates/app/src/theme_editor.rs`, line 308), inside the 7th key's `InputEvent::Change` handler, after `hex_changed` has parsed the value, synced the picker and recomputed `readability_issues`. The first draft of a frame applies in that handler: `apply_appearance` runs one line later, at line 309, and `Palette::apply` and `apply_text_sizes` call `window.refresh()` (`appearance.rs`, lines 897 and 151). The handler then registers a next-frame callback (line 317) which applies any draft coalesced from later edits in the same frame and registers `trace_next_frame` (`crates/app/src/main.rs`, line 1326); the line prints from that second callback.

The printed value therefore spans:

1. `apply_appearance`;
2. the **layout, paint and present of the whole-window render that shows the draft**;
3. the wait for the frame request from which the line prints.

It excludes input delivery and the handler work before the stamp. From key release (after `XSync`) to the stamp, this was a median 5.663 ms (p95 6.703 ms, max 7.801 ms), which includes the X input-method round trip.

**Yes, the edit value contains the draw.** This is what separates it from a Settings switch, which applies inside its handler and ends at the first next-frame callback, before the new theme's frame is drawn. It was measured directly in the 20 of 60 attribution edits whose draft render went through `request_frame`: the render started a median 0.750 ms after the stamp (max 2.724 ms), took a median 20.352 ms of draw and present, and the line followed a median 5.956 ms after it ended. In the other 40 edits the same UI-thread CPU was spent with no `frame duration` line, that is, in the one production draw outside `measure()` that a keystroke can reach; see [Supplementary observations](#supplementary-observations). Either way the value holds exactly one whole-window render, and the driver's pixel checks confirm the draft was on screen when the line arrived.

This matches the boundary [the metric catalogue](metrics.md) states, which this build does not change: "The custom theme editor prints the same name once per open and once per burst of live-preview edits (`theme_editor::preview_theme_draft`), with its own edit boundary: from the edit handler through the frame that shows the draft." The catalogue describes the draft being painted by the frame that follows the first callback. On this host it is more often painted by the keystroke's own synchronous render, which lies inside the same span; the boundary and what the number contains are unaffected.

On the X11 timer (an 8.334 ms grid; lines landed within 0.411 ms of it) the value's floor is the render plus the wait for the following frame request, so a sample below two refresh periods is not reachable while one render costs more than one period. All 60 edits were above two periods.

## Dominant cost

**One whole-window render at 2×** of the Settings page with the editor dialog over it: a median 20.352 ms of draw and present, a median 23.370 ms of UI-thread CPU per edit. The palette application is the small remainder, of order 1 to 2 ms, and the rest of the value is the wait for the frame request that prints the line (median 5.956 ms).

The render, not the theme work, is the cost:

- UI-thread CPU for an edit is close to the **key-6 control**, a keystroke that changes the same text and renders the same rows but parses no valid colour and applies nothing: 23.370 against 22.462 ms median in the recorded launch (median per-edit difference 1.847 ms), and 23.048 against 22.386 ms in the attribution launch (median difference 0.987 ms).
- Measured draft renders (20) cost 20.352 ms median; measured key-6 renders (22) cost 21.280 ms median. The render costs the same whether or not the palette changed.
- Whole-process CPU per edit (23.668 ms median) is within 0.3 ms of the UI thread's, so no worker did the work and no job was submitted.
- All 60 attribution edits spent that CPU in exactly one continuous burst of at least 1 ms, leaving no gap in which a second render or a worker hand-off could sit.

A live-preview edit costs what typing a character into this dialog costs. Reducing it further is a matter of what the window redraws, which is the subject of `themes-draw-cost`.

## Supplementary observations

`perf` is not installed on this host, so costs are attributed to GPUI functions from frame timings and CPU, not to elements inside a frame.

**UI-thread CPU per edit.** The main thread's `/proc/<pid>/task/<pid>/schedstat` delta over 150 ms windows, recorded launch (min / median / p95 / max, ms):

| Window | UI-thread CPU (ms) |
| --- | ---: |
| Idle, before Ctrl+A (caret blinking) | 0.305 / 0.702 / 1.210 / 1.429 |
| Key 6: text change, invalid value, no preview | 18.581 / 22.462 / 28.615 / 32.299 |
| Edit: key 7, just before it to 150 ms after | 19.178 / 23.370 / 29.814 / 30.502 |

**One render per edit, sometimes measured and sometimes not.** In the attribution launch `ZED_MEASUREMENTS=1` made GPUI's `measure("frame duration")` print the draw-plus-present time of every frame drawn from `request_frame` (`gpui-pre` 0.3.4 `window.rs`, line 1781). 20 of 60 edits produced one such line and 40 produced none, while both groups spent the same UI-thread CPU in a single burst. No edit produced two. The only production draw outside `measure()` that a keystroke can reach is the synchronous flush at the top of `Window::dispatch_key_event` (`window.rs`, lines 5606–5608), which draws a dirty window before dispatching the next key event, here the key release of the same keystroke, after the handler has applied the draft. The draft render is that flush when it beats the frame timer and the measured frame otherwise; the same split appears on key 6 (22 of 60 measured), where no preview is involved, so it is a property of the input and timer interleaving rather than of the preview.

**No old-palette frame and no second render.** On `bb9f332` the application was deferred to the next frame callback, so 34 of 60 edits first rendered the typed character in the old palette and then rendered the draft, two whole-window renders inside one value. With the application in the handler, the keystroke's own render already shows the draft: no attribution edit had a render between the stamp and the draft frame, none had a second measured frame, and every edit's dialog strip and page margin sampled the typed colour when the line arrived.

**The retained comparison is not a measurable cost.** The control launch with no comparison open (Settings over History 1–1000) had a median of 29.358 ms, 0.152 ms *faster* than the recorded launch with the Split retained. The retained comparison's `refresh_theme` share is therefore below the spread between launches, as in the previous record.

**The new editor polish is not a measurable cost either.** The always-visible scrollbar, the wider hex field and the invalid-row glyph are drawn in every frame measured here, and the key-6 control, which draws them without any preview, is within about 1 ms of the edit on both this build and `cd563f6`.

## Comparison with the earlier readings

Same host, driver, fixture, window, procedure and statistics; only the build differs. The `bb9f332` and `cd563f6` figures come from the two earlier editor records, retained outside the repository at `perf-theme-editor/record/2026-09-18-theme-editor.json` and `perf-theme-editor/record-cd563f6/2026-09-19-theme-editor.json`.

| Measure | bb9f332 | cd563f6 | 3927b57 | vs cd563f6 | vs bb9f332 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Criterion set, median (ms) | 45.255 | 29.433 | 29.614 | +0.181 (+0.6%) | −15.641 (−34.6%) |
| Criterion set, p95 (ms) | 56.594 | 34.691 | 36.048 | +1.357 (+3.9%) | −20.546 (−36.3%) |
| Pooled 60, median (ms) | 40.136 | 28.895 | 29.510 | +0.615 (+2.1%) | −10.626 (−26.5%) |
| Pooled 60, p95 (ms) | 56.119 | 35.568 | 36.510 | +0.942 (+2.6%) | −19.609 (−34.9%) |
| Pooled 60, standard deviation (ms) | 10.154 | 4.200 | 4.543 | +0.343 | −5.611 |
| UI-thread CPU per edit, median (ms) | 36.052 | 22.442 | 23.370 | +0.928 (+4.1%) | −12.682 (−35.2%) |
| Opens, median (ms) | 28.867 | 30.296 | 27.331 | −2.965 (−9.8%) | −1.536 (−5.3%) |

**Against `cd563f6` the difference is not resolvable.** The four launches of this build span 1.203 ms of median and 1.001 ms of p95; the four `cd563f6` launches spanned 2.047 ms and 1.929 ms. Every edit difference in the table above is smaller than those spreads. The opens row is noisier still, six samples per launch: the open median ranges over 3.998 ms between this build's launches (27.331 to 31.329 ms) and over 3.624 ms between `cd563f6`'s (26.672 to 30.296 ms), so its −2.965 ms is not a difference either. Because the `cd563f6` record was taken earlier in the day under lighter load (its launches began at load averages 0.63 to 0.94, these at 0.50 to 1.88), the `cd563f6` executable was run once more, unchanged, after this build's four launches: it came out at a median of 30.385 ms, a p95 of 37.250 ms and 26.480 ms of UI-thread CPU per edit, slower at every one of those than every launch of this build. The two builds swap order between hours, which is what a difference below the noise floor looks like.

**Against `bb9f332` the difference is real.** The gap is many times either build's launch spread, and the CPU figure explains it: the edit went from two whole-window renders to one. Opens do not share that gain and sit within their own noise across all three builds, as expected, since an open has no keystroke render to reuse: the same-day `cd563f6` launch opened at a median of 27.240 ms against this build's recorded 27.331 ms.

## Limitations

- Linux XWayland on GNOME Wayland only, not native Wayland or macOS. Keys are synthetic XTest input through the X input method; the display is 120 Hz at 2×.
- One recorded launch of 60 edits, with warm caches and uncontrolled desktop load.
- The candidate is rebuilt after this evidence lands, so these numbers bind to the executable digest above only.
- There is no profiler, so costs are attributed to GPUI functions (`Window::draw`, `Window::dispatch_key_event`), not to Settings or dialog elements inside a frame.
- Only the Canvas token was edited, through its hex field. Picker drags, which exercise the coalescing path, and edits that change the Readability list were not measured. The at-most-one-application-per-frame half of the criterion is covered by the task's test, not here.
- This build's readability-count cache in the Your themes card is not exercised: the scratch store holds no custom themes, so the cached loop is empty in every frame measured here. Whether it helps can only be measured with saved custom themes, which this procedure never creates.
- The control bounds the retained comparison's share only to within the noise between launches.

## Reproduction

The driver (`drive_theme_editor.py`, with `analyze_editor.py` and `build_record.py` beside it) and per-launch data are retained outside the repository in the themes evidence directory (`perf-theme-editor/tools/`, `perf-theme-editor/tools-3927b57/` and `perf-theme-editor/runs/`). Per-launch data comprises `stderr.log`, `stderr-timed.log`, `samples.json` and screenshots. The driver that ran is the unmodified `perf-theme-editor/tools/drive_theme_editor.py`, SHA-256 `457cc79a0f078e01e4e76288e945b663ff85175567093973729c3b28ab5e12e4`, the same file that produced the previous record, recorded into every run's `samples.json`; the record builder for this build is the adapted copy in `tools-3927b57/`. All script digests are in the JSON.

Every launch used `xhelper.isolated_env` with an absolute, freshly created `XDG_CONFIG_HOME` under its run directory and `GITTURTLE_TRACE=1`. The window was found by `_NET_WM_PID`, and the driver terminated only the process it launched.

To repeat the measurement on another build, run from `perf-theme-editor/tools/`, giving each launch a new run name:

```sh
python3 drive_theme_editor.py <warmup-run> --binary /abs/path/gitturtle
python3 drive_theme_editor.py <recorded-run> --binary /abs/path/gitturtle
python3 drive_theme_editor.py <attribution-run> --binary /abs/path/gitturtle --bursts --frames
python3 drive_theme_editor.py <control-run> --binary /abs/path/gitturtle --no-compare
python3 ../tools-3927b57/build_record.py --warmup <warmup-run> --recorded <recorded-run> \
    --attribution <attribution-run> --control <control-run> --interleave <previous-build-run> \
    --date <YYYY-MM-DD> --out /abs/new-record.json
```
