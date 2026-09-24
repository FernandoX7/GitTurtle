# Theme draw cost: editor live-preview edits and Settings switches, 2026-09-22

This records the release measurement of [`gitturtle.theme_edit_frame_ms`](metrics.md) (editor opens and live-preview edits, re-cut to end in the draw) and [`gitturtle.theme_apply_frame_ms`](metrics.md) (Settings switches, unchanged) for task `themes-draw-cost` (criteria `edit-budget` and `switch-no-regression`) on build `85a7d07`. Method and format follow the [theme editor record](2026-09-19-theme-editor.md) for edits and the [theme application baseline](2026-09-18-theme-apply.md) for switches, each run on the empty store and on the 32-theme bound. The raw data, including every sample, per-launch summaries and the switch samples, is in [2026-09-22-theme-draw-cost.json](2026-09-22-theme-draw-cost.json).

On the recorded launch (32-theme store, 60 pooled edits in six dialogs of ten, designated in runs/criterion-set-85a7d07.txt at 14:51:14Z before the launch started at 14:51:19Z) gitturtle.theme_edit_frame_ms has p95 (the 57th of 60 sorted samples) 25.372 ms against the 26 ms bound: edit-budget PASS, under the bound by 0.628 ms; median 21.778, min 17.238, max 26.719, 2 of 60 samples above 26 (26.333 and 26.719; the 56th sample is 25.239, the 58th 25.751) and 15 above the former 24 ms bound. The bound is 26 ms by the owner decision of 2026-09-21, re-cut from 24 after ea31261, 2030b3d and 23ee777 measured 26.348, 24.440 and 25.191 on this boundary; this launch would have missed 24. The designated ten (set 1, reported without a bound), in order: 17.836 20.481 22.499 19.967 22.577 25.197 17.950 21.815 24.529 25.176; median 22.157, max 25.197. Per-set medians 22.157 20.529 21.241 21.113 23.290 22.201; the six dialog opens (not edits) 34.206 25.321 20.594 25.411 25.216 26.726. The same procedure on the empty store: p95 23.929, median 19.508, min 15.183, max 24.661, 0 over 26. The warm-up of the same executable and store four minutes earlier (not eligible): p95 25.981, median 22.322, max 26.731, 3 over 26. UI-thread CPU per edit (150 ms from the 7th key), median: 27.808 ms with 32 themes and 24.979 ms empty (key-6 controls 27.983 / 24.242, idle 1.086 / 1.127). Against attempt 3 (23ee777, same boundary, procedure, store bytes and element count) the recorded p95 moved from 25.191 to 25.372 (+0.181) and the median from 20.354 to 21.778 (+1.424), inside the launch-to-launch spread (noise paragraph). switch-no-regression holds against the contract as it now reads: UI-thread CPU per switch median 13.793 ms on the empty store (bound 23) and 17.195 ms with 32 themes, +3.402 ms against the +4 ms allowance (widened from +3 by the coordinator on 2026-09-22 under the owner's 2026-09-21 instruction that a few ms off is acceptable, from these same runs, no launch repeated; budget.allowance_history); against the former +3 ms allowance, in force until 2026-09-22, the same +3.402 would have missed by 0.402 ms (23ee777 +1.565, 2030b3d +1.376, ea31261 +2.009); theme_apply_frame_ms median 4.598 / p95 7.807 (empty) and 3.699 / 7.921 (32 themes), all under 8 and 16, hold. The measured numbers are unchanged by the widening. The instrumented switch frames have exactly 23ee777's element counts (1946 with 32 themes, 1662 empty), the candidate's diff adds no element and no work of that size to a switch frame (the list stands on a row boundary, so the strips are absent), and every Settings frame class of this pass, hover and press controls included, is 2 to 4 ms of UI CPU above 23ee777's on both stores; the 0.402 ms excess over the former allowance is below the spread of the four passes' deltas and below the sampling error of a difference of medians at n = 60 (noise paragraph), which is the reading behind the widening. edit-frame-trace (observed, not graded here): every editor launch printed exactly 66 theme_edit_frame_ms lines and zero theme_apply_frame_ms lines.

- **`edit-budget`, pooled 60 edits** (recorded launch `recorded-32`, 32-theme store, n = 60): min 17.238 ms, median **21.778 ms**, p95 **25.372 ms** (the 57th of 60 sorted), max 26.719 ms; 2 of 60 edits over 26 ms. p95 ≤ 26 ms: **yes** (the bound is 26 ms by the owner decision of 2026-09-21, re-cut from 24 ms; attempts 1 to 3 were graded against 24).
- **Earlier candidates on this boundary and procedure** (pooled p95, 57th of 60, from their preserved records; context, not samples of this record): ea31261 (attempt 1) 26.348 ms (median 23.211, empty store p95 26.070, graded against 24 ms: no); 2030b3d (attempt 2, indicative, collapsed card) 24.440 ms (median 19.206, empty store p95 23.572, graded against 24 ms: no); 23ee777 (attempt 3) 25.191 ms (median 20.354, empty store p95 23.307, graded against 24 ms: no).
- **Designated set** (set 1, the first dialog's ten edits, named in writing before the launch; no bound): 17.836, 20.481, 22.499, 19.967, 22.577, 25.197, 17.950, 21.815, 24.529, 25.176 ms; median 22.157 ms, max 25.197 ms.
- **Same procedure, empty store** (launch `edits-empty`, n = 60): median 19.508 ms, p95 23.929 ms, max 24.661 ms; the 32-theme store adds +2.270 ms at the median and +1.443 ms at p95.
- **UI-thread CPU per edit** (150 ms from the 7th key, median): 27.808 ms on the 32-theme store, 24.979 ms on the empty store (+1.609 ms against the 2026-09-19 record's 23.370 ms, the one comparable number).
- **`switch-no-regression`**: UI-thread CPU per switch median **13.793 ms** on the empty store (≤ 23: **yes**) and **17.195 ms** on the 32-theme store (+3.402 ms; ≤ +4: **yes**; against the former +3 ms allowance, in force until 2026-09-22: no, +0.402 ms; see budget.allowance_history). `theme_apply_frame_ms` empty store median 4.598 / p95 7.807 ms, 32-theme store median 3.699 / p95 7.921 ms (median ≤ 8 and p95 ≤ 16 on both: **yes**). Criterion holds: **yes**.

The edit value contains the draw of the frame that shows the draft; a Settings switch's value ends before its frame is drawn. See [What the values measure](#what-the-values-measure) before comparing the two.

## Exercised build and environment

`--build-info` of the exercised executable (read by both drivers at the start of every launch):

```json
{
  "application": "GitTurtle",
  "version": "0.1.0",
  "source_revision": "85a7d07e4adb792f7ec5aca775e7487522f16d24",
  "source_tree": "clean",
  "target": "x86_64-unknown-linux-gnu",
  "profile": "release",
  "rustc": "rustc 1.98.0 (88d9e12ae 2026-08-18)",
  "build_unix_seconds": "1790037117"
}
```

The executable SHA-256 was `418a3b425cedb14fdf734ae02d756dce642896156dd214e6b165a9239a46aec6`. This record applies to that executable only. All four launches ran the same executable: yes.

Against 3927b57 (the committed editor record, empty store, callback boundary) the candidate carries the import-export feature of 5637cec and this task's changes as verified in src-85a7d07: the edit trace re-cut to gitturtle.theme_edit_frame_ms ending in a zero-size probe deferred above the dialog layer (theme_editor.rs edit_trace_probe, views.rs root render), the Your themes rows virtualized with uniform_list at min(rows, 8) x 30 px with the toolkit scrollbar (settings.rs render_custom_theme_rows), and, per the task contract's items 2 and 4 (not re-verified line by line here), 58b6226's cached ThemePreviewBody per miniature, save_preferences notifying only on a visible change, and the layout wins. Attempt 2 (2030b3d) keeps the whole ea31261 tree and changes settings.rs and theme_editor.rs only (plus DESIGN.md and content-and-layout.md): ring room for the row list (ROW_RING_ROOM 3 px list padding, the rows container 6 px taller with negative margins, the scrollbar pinned with viewport_from_layout and scroll_size), wheel chaining (on_scroll_wheel with stop_propagation) and element cuts (one canvas per swatch strip and per caption-dot run; the token row's 14 px spacer dropped), as seen in its diff against ea31261; not re-verified line by line here. Attempt 3 (23ee777) keeps the whole 2030b3d tree and changes settings.rs, theme_editor.rs and crates/app/docs/content-and-layout.md only (diff -ru src-2030b3d src-23ee777): the Your themes list is an absolute child (top and bottom -ROW_RING_ROOM) of a flow box exactly 30 px x min(rows, 8) tall with no negative margins (custom-themes-rows-box), so the card no longer collapses under taffy's max-content sizing, and the scrollbar track is that box (the_card_keeps_the_plain_stacks_geometry_around_the_rows pins the geometry for 0, 1, 2, 8 and 32 themes); the token row's hex wrapper div is gone (the kit Input takes the width, shrink and mono face itself); the picker caption lays out its name row only on the selected card that shows the badge; debug selectors on the card, its header, the empty line and the text-size settings. Not re-verified line by line here. Attempt 4 (85a7d07) keeps the whole 23ee777 tree and changes settings.rs, theme_editor.rs and crates/app/docs/content-and-layout.md only (diff -ru src-23ee777 src-85a7d07): settings.rs gains rows_off_boundary (45 lines above choose_theme) and render_custom_theme_rows adds two absolute 3 px strips of the card's surface over the list's ring room (custom-themes-ring-room-top/-bottom), present only while the list stands off a row boundary, so a row partly scrolled out of the viewport does not paint into the room; at offset 0, where the measured procedure keeps the list (focus in the dialog), the strips are absent and no element is added to an edit frame, the per-frame cost being one RefCell borrow, arithmetic and one palette read; theme_editor.rs gains the test strips_cover_the_ring_room_while_the_rows_stand_off_a_boundary; the doc gains one sentence. Not re-verified line by line here.

Host: Intel(R) Core(TM) Ultra 9 275HX (24 threads), 188.1 GiB RAM, Pop!_OS 24.04 LTS, Linux `7.1.5-76070105-generic`, git version 2.43.0, `rustc 1.98.0 (88d9e12ae 2026-08-18)`. The app ran as an X11 client of XWayland (`DISPLAY=:1`, `WAYLAND_DISPLAY` unset) under GNOME Shell 46.0 on a wayland session. Panel and mode: `eDP-1 connected primary 3840x2400+0+0 (normal left inverted right x axis y axis) 390mm x 240mm
   3840x2400    119.98*+`. GPUs: `00:02.0 VGA compatible controller: Intel Corporation Arrow Lake-U [Intel Graphics] (rev 06)
02:00.0 VGA compatible controller: NVIDIA Corporation Device 2c58 (rev a1)
80:14.5 Non-VGA unclassified device: Intel Corporation Device 7f2f (rev 10)
80:1c.0 PCI bridge: Intel Corporation Device 7f3d (rev 10)`. Renderer: `OpenGL renderer string: Mesa Intel(R) Graphics (ARL)
OpenGL version string: 4.6 (Compatibility Profile) Mesa 26.1.6-1pop0~1787580452~24.04~a5619ea`. The app used `GPUI_X11_SCALE_FACTOR=2` at its default 1480×980 logical window (2960×1960 physical), not resized.

Power: AC online = 1, governor `powersave`, EPP `balance_performance`. The owner's desktop session stayed active, so background load was uncontrolled. Load averages before and after each launch are in the launches table and the JSON.

Cache state: warm: a warm-up launch (60 edits, summarised in launches[] but not eligible to supply the criterion samples) of the same executable on the same fixture preceded the recorded launch; caches not dropped. Every launch used a fresh process and a fresh absolute scratch store written by seed.py. No cold-cache run.

## Store

version 6, theme midnight, follow_system false, custom_themes 'Seed 01'..'Seed 32' with bases cycling ThemeChoice::ALL (Seed 01 Midnight, Seed 02 Daylight, ..., Seed 21 Midnight, ...) and exactly one token (border, blue channel +1) differing from the base: the edit-budget bound. No row carries a readability warning or the check mark; New theme... and Import... are disabled at 32. The committed editor record was measured with an empty store; the bound puts eight virtualized rows on screen in every Settings and dialog frame, and the empty-store launches are the like-for-like comparison with the earlier records. Seed store sha256 (bytes as written, recorded by the driver at launch): `9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a` (seed.py 32 reproduces `9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a`); the empty store is `ae1731ac8307019b39aee6a216564410b55b656871ddb1b9d552e8e988e4e208`. The first bytes of the seeded store: `{"version": 6, "settings": {"theme": "midnight", "follow_system": false}, "custom_themes": [{"id": 1, "name": "Seed 01",…`.

## Fixture and state before the first edit

The fixture is the `themes-apply-trace` fixture, unmodified: 1,207 commits on `main`, HEAD `c8f20cfeb3f846168abbdcde270983c42bb27e71`. Clean with the same HEAD after all launches: yes.

| Precondition | How it was verified (recorded edit launch) |
| --- | --- |
| At least 1,000 commits loaded | The Older control printed `gitturtle.history_page_frame_ms=1.989`, and the header then read "All history 1–1000". |
| 2,000-line comparison in Split | Activating `large-module.ts` on row 1 (`c8f20cf`) printed `gitturtle.file_preview_frame_ms=4.795`. Split was then chosen. |
| Comparison retained behind Settings | Back showed the retained Split with 0 new file_preview_frame_ms lines; a second Back showed History 1-1000 |

## Procedure

tools-85a7d07/drive_theme_editor.py is the committed record's driver (tools/drive_theme_editor.py, sha256 457cc79a...) with the 58b6226 adaptation (--custom-themes N seeds the store through seed.py; at the 32-theme bound the dialog is opened through row 1's Edit...: Tab x28, then Tab one at a time until the kit's focus ring lies in row 1's band as its leftmost button, then Space), the ea31261 adaptation (adapt_ea31261.py), the 2030b3d re-pointing (adapt_2030b3d.py), the 23ee777 re-pointing (adapt_23ee777.py) and the 85a7d07 re-pointing (adapt_85a7d07.py: identifiers, the settings.rs switch-boundary line numbers, the fourth before build and the edit-budget bound re-cut to 26 ms): the traced line is gitturtle.theme_edit_frame_ms, the seed is the contract's bound (Seed 01..32, bases cycling, border differing) and its digest is recorded. The pixel boxes, the typed colours, the timing, the per-edit checks and summary() are unchanged; the pooled statistic was added to analyze_editor.py.

1. Ctrl+comma opened Settings over the comparison, and 15 wheel steps brought "Your themes" on screen; the card's top border was found at y = 1197 (x 373..1834) and row 1's swatch strip put the row's band at 1321..1381 physical px.
2. Tab ×28 put the focus ring on row 1's Edit… (1106 ring pixels, box [1378, 1321, 1497, 1380]).
3. Ctrl+comma opens Settings over the comparison; 15 wheel steps bring Your themes on screen; Tab to row 1's Edit... (count above); Space opens Edit theme for 'Seed 01' with base Midnight and Midnight's palette (border +1) as the draft; Tab x2 reaches the Canvas hex field. Before each later open the ring was checked to be back on Edit…; extra Tabs needed per set: [0, 0, 0, 0, 0, 0].
4. Ctrl+A, then '#rrggbb' typed with 60 ms between keys and 150 ms before the 7th key; the value is invalid through the 6th key and valid at the 7th: one preview per edit, asserted on every edit. >= 1.2 s without input before every edit.
5. 12 dark canvases cycled (#141a26 ... #111b28), each with Midnight's empty Readability list so every edit renders the same rows.
6. 6 dialogs x 10 edits in one launch; Escape closes each dialog (draft dropped, Midnight re-applied without a trace line); focus returns to Edit... and Space reopens.

Every edit was verified before the next one: no trace line from keys 1-6; exactly one new gitturtle.theme_edit_frame_ms line after key 7; dialog title strip (physical 1200-2000 x 204-236) 100% the typed colour; page margin (physical 16-200 x 700-1300) 100% one colour equal to the typed colour under the modal's 20% black overlay (+-1). Every set was also verified: the open printed exactly one line (recorded as an open sample, not an edit); store still names midnight with 32 custom themes; Escape restored the Midnight canvas and printed no line. Per launch: theme_apply_frame_ms lines in the editor launch: 0 (edit-frame-trace expects 0).

Switches: tools-85a7d07/drive_theme_apply.py: the baseline's driver with the launch inlined (binary, seeded store, identity recording) and a final store check; 60 real XTest clicks on the picker cards, same geometry, timing and per-switch verification as the baseline; once on the empty store and once on the 32-theme store.

Percentiles: statistics.median; nearest rank for p90/p95: the ceil(p/100*n)-th smallest (p95 of 60 = the 57th smallest; p95 of 10 = the maximum). Designated set: set 1 of the recorded launch, designated in writing before the launch ran (runs/criterion-set-85a7d07.txt); reported, no bound.

## Samples: edits

Values are in milliseconds (`gitturtle.theme_edit_frame_ms`), recorded launch, 32-theme store. Set 1 is the designated set; its typed colour is shown in parentheses.

| Edit | Set 1 | Set 2 | Set 3 | Set 4 | Set 5 | Set 6 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 17.836 (#141a26) | 17.576 | 22.352 | 20.910 | 20.474 | 24.339 |
| 2 | 20.481 (#0c1119) | 17.242 | 18.713 | 21.675 | 25.372 | 20.626 |
| 3 | 22.499 (#161c2a) | 22.377 | 18.067 | 19.439 | 24.765 | 22.661 |
| 4 | 19.967 (#0e1320) | 25.751 | 20.130 | 21.316 | 22.231 | 23.773 |
| 5 | 22.577 (#121824) | 19.991 | 18.203 | 23.571 | 24.757 | 23.759 |
| 6 | 25.197 (#0a0f16) | 26.333 | 19.125 | 23.940 | 25.239 | 20.800 |
| 7 | 17.950 (#181e2c) | 22.130 | 26.719 | 17.238 | 22.301 | 23.961 |
| 8 | 21.815 (#10161d) | 19.988 | 24.816 | 21.867 | 24.279 | 21.741 |
| 9 | 24.529 (#131627) | 19.508 | 25.004 | 20.667 | 19.845 | 18.035 |
| 10 | 25.176 (#0d151b) | 21.066 | 24.953 | 19.424 | 20.813 | 21.675 |

Sorted, all 60: 17.238, 17.242, 17.576, 17.836, 17.950, 18.035, 18.067, 18.203, 18.713, 19.125, 19.424, 19.439, 19.508, 19.845, 19.967, 19.988, 19.991, 20.130, 20.474, 20.481, 20.626, 20.667, 20.800, 20.813, 20.910, 21.066, 21.316, 21.675, 21.675, 21.741, 21.815, 21.867, 22.130, 22.231, 22.301, 22.352, 22.377, 22.499, 22.577, 22.661, 23.571, 23.759, 23.773, 23.940, 23.961, 24.279, 24.339, 24.529, 24.757, 24.765, 24.816, 24.953, 25.004, 25.176, 25.197, 25.239, 25.372, 25.751, 26.333, 26.719. The 57th is **25.372** ms.

| Set | n | min | median | mean | p90 | p95 | max | note |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| **1 (designated)** | 10 | 17.836 | 22.157 | 21.803 | 25.176 | 25.197 | 25.197 | reported, no bound |
| 2 | 10 | 17.242 | 20.529 | 21.196 | 25.751 | 26.333 | 26.333 |  |
| 3 | 10 | 18.067 | 21.241 | 21.808 | 25.004 | 26.719 | 26.719 |  |
| 4 | 10 | 17.238 | 21.113 | 21.005 | 23.571 | 23.940 | 23.940 |  |
| 5 | 10 | 19.845 | 23.290 | 23.008 | 25.239 | 25.372 | 25.372 |  |
| 6 | 10 | 18.035 | 22.201 | 22.137 | 23.961 | 24.339 | 24.339 |  |
| Pooled | 60 | 17.238 | 21.778 | 21.826 | 25.176 | 25.372 | 26.719 | p95 ≤ 26: yes |

| Launch | store | median | p95 (57th of 60) | max | UI-thread CPU per edit, median | key-6 control CPU, median | edits > 26 ms | apply lines |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| warmup-32 | 32 | 22.322 | 25.981 | 26.731 | 28.178 | 27.294 | 3 | 0 |
| **recorded-32** | 32 | 21.778 | 25.372 | 26.719 | 27.808 | 27.983 | 2 | 0 |
| edits-empty | 0 | 19.508 | 23.929 | 24.661 | 24.979 | 24.242 | 0 | 0 |

**Opens are traced too.** open_theme_editor stamps at its entry (theme_editor.rs:483) and applies the saved theme's palette through preview_theme_draft (line 523), so each dialog open also prints one theme_edit_frame_ms line; not an edit. Recorded launch: 34.206, 25.321, 20.594, 25.411, 25.216, 26.726 ms, median 25.366 ms; empty store median 23.972 ms. They are excluded from the edit samples.

## Samples: switches

### Launch `switches-empty`: empty (the baseline's store byte-for-byte)

Values in milliseconds (`gitturtle.theme_apply_frame_ms`), switch number in parentheses; rows follow the cycle order.

| Target | Cycle 1 | Cycle 2 | Cycle 3 |
| --- | ---: | ---: | ---: |
| daylight | 3.342 (#1) | 5.990 (#21) | 5.580 (#41) |
| graphite | 2.402 (#2) | 6.573 (#22) | 4.896 (#42) |
| tokyo_night | 2.413 (#3) | 7.203 (#23) | 3.802 (#43) |
| catppuccin_mocha | 6.757 (#4) | 5.240 (#24) | 2.713 (#44) |
| nord | 5.756 (#5) | 5.034 (#25) | 7.598 (#45) |
| porcelain | 3.316 (#6) | 1.636 (#26) | 1.332 (#46) |
| sandstone | 2.907 (#7) | 0.668 (#27) | 0.877 (#47) |
| deep_sea | 1.324 (#8) | 7.807 (#28) | 6.861 (#48) |
| ember | 4.618 (#9) | 7.269 (#29) | 4.578 (#49) |
| solarized_dark | 7.623 (#10) | 6.381 (#30) | 1.513 (#50) |
| solarized_light | 2.398 (#11) | 7.076 (#31) | 0.639 (#51) |
| one_dark | 3.080 (#12) | 6.520 (#32) | 2.515 (#52) |
| one_light | 5.583 (#13) | 4.363 (#33) | 0.651 (#53) |
| rose_pine | 6.450 (#14) | 8.066 (#34) | 6.011 (#54) |
| rose_pine_dawn | 1.396 (#15) | 0.466 (#35) | 0.672 (#55) |
| dracula | 6.796 (#16) | 7.120 (#36) | 0.546 (#56) |
| alucard | 6.742 (#17) | 3.917 (#37) | 8.003 (#57) |
| kanagawa_wave | 6.359 (#18) | 2.289 (#38) | 6.619 (#58) |
| kanagawa_lotus | 5.120 (#19) | 0.531 (#39) | 2.991 (#59) |
| midnight | 3.738 (#20) | 1.100 (#40) | 7.881 (#60) |

| n | min | median | mean | p90 | p95 | max | median ≤ 8 and p95 ≤ 16 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 60 | 0.466 | 4.598 | 4.327 | 7.269 | 7.807 | 8.066 | holds |

UI-thread CPU per switch (main-thread `schedstat` delta, just before the release to 150 ms after it): min 10.983, median **13.793**, p95 21.639, max 28.497 ms. Whole process: median 14.581 ms.
Control windows (150 ms of UI-thread CPU, median): idle 1.127 ms; hover repaint 11.502 ms; press repaint 12.613 ms.
Against the 2026-09-18 baseline (build 7924244, empty store): switch median +0.771 ms, p95 +0.138 ms; UI-thread CPU median -12.632 ms (-47.8%), p95 -7.262 ms.
Verification: every store write named the clicked theme (yes); target accent share min 1.0, previous card's accent share max 0.0; 0 custom themes in the store after the last switch; 0 new file_preview lines after Settings. Trace lines landed on the 8.334 ms grid with phase stdev 0.206 ms; release to line exceeded the printed value by a median of 0.209 ms (medians). Store sha256 `ae1731ac8307019b39aee6a216564410b55b656871ddb1b9d552e8e988e4e208`.

### Launch `switches-32`: 32 custom themes (the bound)

Values in milliseconds (`gitturtle.theme_apply_frame_ms`), switch number in parentheses; rows follow the cycle order.

| Target | Cycle 1 | Cycle 2 | Cycle 3 |
| --- | ---: | ---: | ---: |
| daylight | 0.764 (#1) | 4.043 (#21) | 2.392 (#41) |
| graphite | 3.680 (#2) | 1.749 (#22) | 7.549 (#42) |
| tokyo_night | 7.487 (#3) | 3.132 (#23) | 7.452 (#43) |
| catppuccin_mocha | 0.737 (#4) | 7.951 (#24) | 5.477 (#44) |
| nord | 2.929 (#5) | 8.049 (#25) | 5.629 (#45) |
| porcelain | 4.347 (#6) | 7.992 (#26) | 5.234 (#46) |
| sandstone | 1.758 (#7) | 7.510 (#27) | 2.698 (#47) |
| deep_sea | 1.017 (#8) | 4.314 (#28) | 4.901 (#48) |
| ember | 5.298 (#9) | 3.989 (#29) | 7.787 (#49) |
| solarized_dark | 4.797 (#10) | 2.793 (#30) | 7.921 (#50) |
| solarized_light | 1.155 (#11) | 0.460 (#31) | 0.732 (#51) |
| one_dark | 0.731 (#12) | 0.729 (#32) | 7.794 (#52) |
| one_light | 1.040 (#13) | 1.889 (#33) | 3.836 (#53) |
| rose_pine | 0.445 (#14) | 1.038 (#34) | 1.124 (#54) |
| rose_pine_dawn | 0.454 (#15) | 6.198 (#35) | 0.466 (#55) |
| dracula | 7.477 (#16) | 6.170 (#36) | 2.120 (#56) |
| alucard | 7.044 (#17) | 7.713 (#37) | 1.552 (#57) |
| kanagawa_wave | 1.502 (#18) | 3.691 (#38) | 1.022 (#58) |
| kanagawa_lotus | 4.378 (#19) | 3.844 (#39) | 0.532 (#59) |
| midnight | 4.458 (#20) | 3.707 (#40) | 3.687 (#60) |

| n | min | median | mean | p90 | p95 | max | median ≤ 8 and p95 ≤ 16 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 60 | 0.445 | 3.699 | 3.806 | 7.713 | 7.921 | 8.049 | holds |

UI-thread CPU per switch (main-thread `schedstat` delta, just before the release to 150 ms after it): min 12.427, median **17.195**, p95 23.411, max 23.831 ms. Whole process: median 17.999 ms.
Control windows (150 ms of UI-thread CPU, median): idle 0.994 ms; hover repaint 14.108 ms; press repaint 17.157 ms.
Against the 2026-09-18 baseline (build 7924244, empty store): switch median -0.128 ms, p95 +0.252 ms; UI-thread CPU median -9.230 ms (-34.9%), p95 -5.490 ms.
Verification: every store write named the clicked theme (yes); target accent share min 1.0, previous card's accent share max 0.0; 32 custom themes in the store after the last switch; 0 new file_preview lines after Settings. Trace lines landed on the 8.334 ms grid with phase stdev 0.204 ms; release to line exceeded the printed value by a median of 0.213 ms (medians). Store sha256 `9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a`.

Criterion arithmetic: empty-store UI-thread CPU median 13.793 ms against ≤ 23 ms; 32-theme store 17.195 ms, +3.402 ms against ≤ +4 ms. base 5637cec measured 26.4 ms UI-thread CPU per switch with the save completion's second render, 58b6226 without it 21.0 ms (tasks-2.json switch-no-regression); the 7924244 baseline comparison is kept in each launch block.

## What the values measure

**`gitturtle.theme_edit_frame_ms`.** Stamped at entry to ThemeForm::hex_changed (theme_editor.rs:1568-1569, trace_stamp before the parse) inside the 7th key's InputEvent::Change; a valid value reaches draft_changed (line 1632) and preview_theme_draft (line 575), which keeps the first stamp of the burst in State::edit_started (line 586) and calls apply_appearance (line 593). The root render (views.rs:2459-2464) takes the stamp after Root::render_dialog_layer and appends edit_trace_probe (theme_editor.rs:625-659): a zero-size canvas deferred at priority usize::MAX (EDIT_TRACE_PRIORITY, line 322), whose paint closure prints the line (line 637). It is the last element Window::draw paints in the first frame drawn after the draft, whether the platform tick or Window::dispatch_key_event draws it. The value holds the parse, the readability recompute, apply_appearance and the whole window's request_layout, layout, prepaint and paint with the dialog over it; it excludes next-frame callback waits, frame-finish bookkeeping, present, pre-handler input delivery and GPU execution. The frame-tick grid does not enter the value. Lower bound by construction: none from the frame grid: the end stamp is taken inside the draw, not at a tick.

Key release (after `XSync`) to the stamp was a median 6.080 ms (p95 7.847 ms). Line arrival phase on the 8.334 ms grid, stdev 2.535 ms (a uniform phase is expected now that the line prints from inside the draw); 60 of 60 edits were above two refresh periods.

**`gitturtle.theme_apply_frame_ms`.** Unchanged in kind, 45 lines lower in settings.rs than on 23ee777 (rows_off_boundary was added above choose_theme): stamped at choose_theme entry (settings.rs:547); apply_appearance (line 554), save_preferences (line 555), then trace_next_frame (line 557, main.rs:1353) registers a next-frame callback that prints. Next-frame callbacks run only on the platform frame tick, before that frame is laid out and painted, so the value is the handler plus the wait to the next tick of the 8.334 ms X11 grid and is blind to the draw. It cannot fall below the handler's own cost, and its spread is the keystroke's phase on the grid; UI-thread CPU per switch is the instrument for the frame's cost. Lower bound by construction: handler cost + phase on the 8.334 ms grid (0 to one period).

## Elements per frame and draw phases (instrumented scratch builds)

GPUI has no element counter in the release candidate and GITTURTLE_TRACE prints none; counts come from the instrumented scratch build (diagnosis-58b6226/diag-gpui.patch applied to a copy of the candidate's clean checkout, README-next.md section 6; since 2030b3d the dialog-layer probe of diag.patch is ported too, so the dialog's share is attributed) run with GITTURTLE_DIAG=1 under the same drivers; those runs are never criterion samples. before: the 58b6226 original, the ea31261 attempt 1, the 2030b3d attempt 2 (indicative, collapsed card) and the 23ee777 attempt 3; after: this candidate.

| Build | Run | Frame class | n | Elements | Draw median (ms) | Draw p95 (ms) |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 58b6226 (before, original) | diag-edits-32 | edit_or_open_frame (dialog, traced, one draw) | 66 | 2774 | 26.438 | 30.661 |
| 58b6226 (before, original) | diag-edits-32 | plain_keystroke_frame (dialog, keydown, untraced, one draw, no miniature rebuilt) | 419 | 2735 | 22.875 | 30.379 |
| 58b6226 (before, original) | diag-edits-32 | dialog_frame_without_input (caret blink etc., one draw) | 201 | 2735 | 22.773 | 30.755 |
| 58b6226 (before, original) | diag-edits-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 3 | 2772 | 17.841 | 22.861 |
| 58b6226 (before, original) | diag-edits-empty | edit_or_open_frame (dialog, traced, one draw) | 66 | 1784 | 19.457 | 21.269 |
| 58b6226 (before, original) | diag-edits-empty | plain_keystroke_frame (dialog, keydown, untraced, one draw, no miniature rebuilt) | 420 | 1745 | 17.791 | 21.886 |
| 58b6226 (before, original) | diag-edits-empty | dialog_frame_without_input (caret blink etc., one draw) | 212 | 1745 | 16.383 | 22.241 |
| 58b6226 (before, original) | diag-edits-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 3 | 1782 | 8.906 | 9.306 |
| 58b6226 (before, original) | diag-switches-32 | switch_frame (settings, no dialog, traced, one draw) | 60 | 2771 | 18.188 | 24.069 |
| 58b6226 (before, original) | diag-switches-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 1991 | 17.722 | 23.761 |
| 58b6226 (before, original) | diag-switches-empty | switch_frame (settings, no dialog, traced, one draw) | 60 | 1781 | 11.816 | 16.038 |
| 58b6226 (before, original) | diag-switches-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 1001 | 10.660 | 15.566 |
| ea31261 (before, attempt 1) | diag-edits-32 | edit_or_open_frame (dialog open, traced, one draw; relabelled, dialog probe absent) | 66 | 2098 | 20.008 | 23.766 |
| ea31261 (before, attempt 1) | diag-edits-32 | settings_pointer_frame (before the first open: no dialog, mouse input, one draw) | 3 | 2097 | 10.754 | 10.867 |
| ea31261 (before, attempt 1) | diag-edits-empty | edit_or_open_frame (dialog open, traced, one draw; relabelled, dialog probe absent) | 66 | 1785 | 18.236 | 21.326 |
| ea31261 (before, attempt 1) | diag-edits-empty | settings_pointer_frame (before the first open: no dialog, mouse input, one draw) | 3 | 1782 | 15.742 | 16.717 |
| ea31261 (before, attempt 1) | diag-switches-32 | switch_frame (settings, no dialog, traced, one draw) | 60 | 2094 | 10.879 | 18.260 |
| ea31261 (before, attempt 1) | diag-switches-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 1314 | 10.642 | 18.517 |
| ea31261 (before, attempt 1) | diag-switches-empty | switch_frame (settings, no dialog, traced, one draw) | 60 | 1781 | 9.495 | 16.418 |
| ea31261 (before, attempt 1) | diag-switches-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 1001 | 9.350 | 16.044 |
| 2030b3d (before, attempt 2, indicative) | diag-edits-32 | edit_or_open_frame (dialog, traced, one draw) | 66 | 1922 | 20.614 | 22.612 |
| 2030b3d (before, attempt 2, indicative) | diag-edits-32 | plain_keystroke_frame (dialog, keydown, untraced, one draw, no miniature rebuilt) | 420 | 1882 | 19.202 | 22.707 |
| 2030b3d (before, attempt 2, indicative) | diag-edits-32 | dialog_frame_without_input (caret blink etc., one draw) | 205 | 1881 | 16.184 | 23.277 |
| 2030b3d (before, attempt 2, indicative) | diag-edits-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 3 | 1967 | 10.536 | 16.261 |
| 2030b3d (before, attempt 2, indicative) | diag-edits-empty | edit_or_open_frame (dialog, traced, one draw) | 66 | 1639 | 17.853 | 21.280 |
| 2030b3d (before, attempt 2, indicative) | diag-edits-empty | plain_keystroke_frame (dialog, keydown, untraced, one draw, no miniature rebuilt) | 420 | 1599 | 17.230 | 21.049 |
| 2030b3d (before, attempt 2, indicative) | diag-edits-empty | dialog_frame_without_input (caret blink etc., one draw) | 210 | 1598 | 15.869 | 21.546 |
| 2030b3d (before, attempt 2, indicative) | diag-edits-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 3 | 1682 | 9.005 | 9.375 |
| 2030b3d (before, attempt 2, indicative) | diag-switches-32 | switch_frame (settings, no dialog, traced, one draw) | 60 | 1964 | 12.228 | 16.902 |
| 2030b3d (before, attempt 2, indicative) | diag-switches-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 1184 | 10.279 | 16.483 |
| 2030b3d (before, attempt 2, indicative) | diag-switches-empty | switch_frame (settings, no dialog, traced, one draw) | 60 | 1681 | 8.736 | 14.846 |
| 2030b3d (before, attempt 2, indicative) | diag-switches-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 901 | 8.553 | 14.404 |
| 23ee777 (before, attempt 3) | diag-edits-32 | edit_or_open_frame (dialog, traced, one draw) | 66 | 1882 | 19.869 | 22.430 |
| 23ee777 (before, attempt 3) | diag-edits-32 | plain_keystroke_frame (dialog, keydown, untraced, one draw, no miniature rebuilt) | 420 | 1842 | 18.708 | 22.506 |
| 23ee777 (before, attempt 3) | diag-edits-32 | dialog_frame_without_input (caret blink etc., one draw) | 201 | 1841 | 18.829 | 23.206 |
| 23ee777 (before, attempt 3) | diag-edits-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 3 | 1949 | 10.938 | 15.728 |
| 23ee777 (before, attempt 3) | diag-edits-empty | edit_or_open_frame (dialog, traced, one draw) | 66 | 1598 | 17.265 | 20.346 |
| 23ee777 (before, attempt 3) | diag-edits-empty | plain_keystroke_frame (dialog, keydown, untraced, one draw, no miniature rebuilt) | 420 | 1558 | 15.070 | 20.541 |
| 23ee777 (before, attempt 3) | diag-edits-empty | dialog_frame_without_input (caret blink etc., one draw) | 214 | 1557 | 14.549 | 20.784 |
| 23ee777 (before, attempt 3) | diag-edits-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 3 | 1663 | 8.639 | 9.013 |
| 23ee777 (before, attempt 3) | diag-switches-32 | switch_frame (settings, no dialog, traced, one draw) | 60 | 1946 | 9.896 | 16.560 |
| 23ee777 (before, attempt 3) | diag-switches-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 1166 | 9.653 | 16.368 |
| 23ee777 (before, attempt 3) | diag-switches-empty | switch_frame (settings, no dialog, traced, one draw) | 60 | 1662 | 8.788 | 15.451 |
| 23ee777 (before, attempt 3) | diag-switches-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 882 | 8.652 | 15.064 |
| 85a7d07 (after) | diag-edits-32 | edit_or_open_frame (dialog, traced, one draw) | 66 | 1882 | 20.588 | 24.075 |
| 85a7d07 (after) | diag-edits-32 | plain_keystroke_frame (dialog, keydown, untraced, one draw, no miniature rebuilt) | 420 | 1842 | 20.342 | 23.051 |
| 85a7d07 (after) | diag-edits-32 | dialog_frame_without_input (caret blink etc., one draw) | 211 | 1841 | 19.735 | 23.920 |
| 85a7d07 (after) | diag-edits-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 3 | 1949 | 14.504 | 16.062 |
| 85a7d07 (after) | diag-edits-empty | edit_or_open_frame (dialog, traced, one draw) | 66 | 1598 | 18.522 | 21.173 |
| 85a7d07 (after) | diag-edits-empty | plain_keystroke_frame (dialog, keydown, untraced, one draw, no miniature rebuilt) | 420 | 1558 | 18.086 | 20.180 |
| 85a7d07 (after) | diag-edits-empty | dialog_frame_without_input (caret blink etc., one draw) | 213 | 1557 | 16.636 | 21.389 |
| 85a7d07 (after) | diag-edits-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 3 | 1663 | 9.365 | 11.887 |
| 85a7d07 (after) | diag-switches-32 | switch_frame (settings, no dialog, traced, one draw) | 60 | 1946 | 11.060 | 17.326 |
| 85a7d07 (after) | diag-switches-32 | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 1166 | 10.771 | 16.463 |
| 85a7d07 (after) | diag-switches-empty | switch_frame (settings, no dialog, traced, one draw) | 60 | 1662 | 9.312 | 15.559 |
| 85a7d07 (after) | diag-switches-empty | settings_pointer_frame (settings, no dialog, untraced, mouse input, one draw) | 224 | 882 | 9.292 | 14.255 |

Instrumented 32-theme edit frame (gitturtle-85a7d07-DIAG, settings_page, picker, themes_card and dialog_layer probes): 1882 elements, exactly 23ee777's (58b6226 2774, ea31261 2098, 2030b3d 1922), as expected of a diff whose two strips are absent while the list stands on a row boundary; draw 20.588 ms median (p95 24.075): request_layout 7.735 (element construction and layout requests including the view renders), taffy 7.116, prepaint 1.373 + 0.888 deferred, paint 1.712 + 1.284 deferred, present 1.095 (no vsync blocking). The whole-window relayout, request_layout + taffy = 14.851 of 20.588 ms, remains the dominant cost, as on 23ee777 (14.576 of 19.869), 2030b3d (15.351 of 20.614) and ea31261 (14.8 of 20.0). The dialog layer is 674 of the 1882 elements (35.8 %) and 5.111 ms of request_layout on its own, the largest single subtree; the Settings page under it is 814 elements and 4.819 ms (request_layout 1.611, prepaint 1.692, paint 1.516; picker 338 elements, 1.526 ms; Your themes card 61 elements, 1.619 ms). Empty-store edit frame: 1598 elements (23ee777 1598), draw 18.522 (p95 21.173), dialog 674 / 4.880 ms; the 32-theme store costs +2.066 ms per instrumented edit frame (23ee777 +2.604). 59.1 % of edit frames were drawn synchronously in Window::dispatch_key_event (23ee777 48.5 %); the 20 picker miniatures are reused on every edit frame and only the draft's is rebuilt. The instrumented launches' own trace medians are 22.074 (32 themes) and 20.056 (empty), +0.296 and +0.548 ms over the release launches. Switch frames: 32 themes 1946 elements / 11.060 ms (23ee777 1946 / 9.896), empty 1662 / 9.312 (1662 / 8.788); every phase is up by a similar fraction on identical element counts (request_layout 2.311 vs 1.873, taffy 5.169 vs 4.877, prepaint 1.985 vs 1.534, paint 1.965 vs 1.618). This candidate changes no element and no phase of any frame class; the +0.5 to +1.2 ms of instrumented draw against 23ee777 on identical trees is the host of this pass, not the diff. What remains, at 26 ms, is the same as before: the dialog's and the page's element construction and the taffy pass; the row list (61 elements) and the swatch canvases are already small.

## Comparison with the editor record

docs/benchmarks/2026-09-19-theme-editor.json (build 3927b57, empty store) measured theme_apply_frame_ms at the callback boundary, which ended at a second next-frame callback and so carried at least one 8.334 ms period plus the keystroke's phase (diagnosis-58b6226 F1). The boundaries differ: these deltas are history, not a like-for-like speed comparison.

| Measure | 3927b57 (callback boundary, empty store) | 85a7d07 (draw-end boundary, 32 themes) | Δ | comparable |
| --- | ---: | ---: | ---: | --- |
| Pooled 60, median (ms) | 29.510 | 21.778 | -7.732 (-26.2%) | no |
| Pooled 60, p95 (ms) | 36.510 | 25.372 | -11.138 (-30.5%) | no |
| UI-thread CPU per edit, median (ms) | 23.370 | 27.808 | +4.438 (+19.0%) | yes |

Launch spread: the 85a7d07 32-theme edit launches span 0.544 ms of median (21.778, 22.322) and 0.609 ms of p95.

The two 32-theme launches of this executable and store agree: warm-up median 22.322 / p95 25.981 (14:47:02Z) against the recorded launch's 21.778 / 25.372 (14:51:19Z), 0.54 ms apart in the median and 0.61 in the p95, both under 26 with 3 and 2 of 60 samples over the bound; the empty-store launch (median 19.508, p95 23.929) sits 2.3 ms below at the median, consistent with its 284 fewer elements. Against 23ee777 the recorded p95 is +0.181 and the median +1.424 on an identical element count (1882), and the instrumented draw is +0.719 ms on the same tree: this pass ran on a busier host. The 1-minute load never fell under 2: the owner's IDE renderer (about one core, 28 h of CPU over the uptime) and gnome-shell kept it at 2.4 to 2.9 with the CPU 92 percent idle on 24 threads, so the coordinator waived the under-2 precondition at 14:50Z (launch at a 1-minute load under 3 with the CPU over 85 percent idle; no launch repeated because of its load). Loads: 2.83 at the warm-up decision (14:46:45Z, 92.5 percent idle; the driver's own reading at its start 3.16, after it 3.07), 2.85 at the recorded launch (14:51:19Z, 92.3 percent idle; 2.68 after), 2.75 / 3.05 / 2.86 at the starts of edits-empty, switches-empty and switches-32, 2.74 / 2.96 / 2.83 / 2.63 at the four instrumented launches (never criterion samples). The idle controls show no contention by the instrument the coordinator named: UI-thread CPU in an idle 150 ms window median 1.086 (recorded-32), 1.107 (warm-up), 1.127 (edits-empty), 1.127 (switches-empty), 0.994 (switches-32), inside the previous passes' 1.0 to 1.2. What the busier host did show is a uniform rise of every UI-thread CPU figure against 23ee777 on identical trees: per edit 25.470 to 27.808 (32 themes) and 22.175 to 24.979 (empty); per switch 11.846 to 13.793 (empty) and 13.411 to 17.195 (32 themes), with the hover controls 9.705 / 10.958 to 11.502 / 14.108 and the press controls 11.622 / 12.950 to 12.613 / 17.157; the 32-theme Settings frames rose more than the empty ones on both the switch and the control windows, which is what carries the store delta from +1.565 to +3.402: past the +3 ms allowance in force until 2026-09-22 by 0.402 ms and under the +4 ms allowance the contract now carries. That excess over the former allowance is smaller than the spread of the four passes' deltas (+1.376 to +3.402) and than the difference of medians' sampling error at n = 60 with a per-switch stdev of 3.5 to 3.9 ms (about 0.85 ms), so noise exceeds the difference; the coordinator widened the allowance on that reading and this record is graded against +4 from the same runs, the measured numbers unchanged. Governor powersave, EPP balance_performance, AC power. No foreign process on :1 during any launch: the native-qa agent released the display at 14:03:37Z and stopped, the coordinator's handover check at 14:04:38Z and mine at 14:04:55Z printed nothing (host_state.py's foreign_processes field is empty at every snapshot). At the draw-end boundary there is no frame-grid floor in the value: the line's phase on the 8.334 ms grid has stdev 2.535 ms (uniform), key-7 to stamp (input delivery before the handler, outside the value) median 6.080 ms, and 60 of 60 edits exceed two refresh periods. Present medians 0.67 to 1.10 ms in every frame class: no vsync blocking. One recorded launch per configuration, warm caches, synthetic XTest input, XWayland on GNOME Wayland only.

## Limitations

- Linux XWayland on GNOME Wayland only (not native Wayland, not macOS); synthetic XTest keys and clicks; 120 Hz display at 2x.
- One recorded launch of 60 edits per store and one of 60 switches per store; warm caches; uncontrolled desktop load on the owner's active session.
- These numbers bind to executable sha256 418a3b425cedb14fdf734ae02d756dce642896156dd214e6b165a9239a46aec6 only.
- No profiler on the host: draw phases and elements per frame come from the instrumented scratch build, not from the release candidate.
- One token (Canvas) edited through the hex field only; picker drags and edits that change the Readability list were not measured.
- The edit metric's boundary changed with this candidate, so its values are not comparable with the 2026-09-19 record's; UI-thread CPU per edit is.

## Reproduction

The drivers (`tools-85a7d07/drive_theme_editor.py`, sha256 `9b74287e2782cee010d134ac364b2ead03b0408b59914ddf96ddaf2f6deced6b`, and `tools-85a7d07/drive_theme_apply.py`, sha256 `8b9aec228c141e341dbeaf0a68e909f6a2bf1570d85340011de66bc6f7cedb2b`; `seed.py` `89763487296fcbc0a67e23eb6a2f590d1086526e4bd00ca6f4b026e6f0c1f349`), the analyzers and this record's builder are retained outside the repository in the themes evidence directory (`perf-draw-cost/tools-85a7d07/`) with per-launch data under `perf-draw-cost/runs/` (`stderr.log`, `stderr-timed.log`, `samples.json`, screenshots). The committed editor record's driver is sha256 `457cc79a0f078e01e4e76288e945b663ff85175567093973729c3b28ab5e12e4`; the adaptation scripts that produced these copies are beside them. All script digests are in the JSON.

Every launch used `xhelper.isolated_env` with an absolute, freshly created `XDG_CONFIG_HOME` under its run directory and `GITTURTLE_TRACE=1`; the window was found by `_NET_WM_PID`, and each driver terminated only the process it launched.

```sh
cd perf-draw-cost/tools-85a7d07
python3 drive_theme_editor.py <warmup-run> --binary /abs/path/gitturtle --custom-themes 32 --explore
python3 drive_theme_editor.py <recorded-run> --binary /abs/path/gitturtle --custom-themes 32      # after writing runs/criterion-set-<sha7>.txt
python3 drive_theme_editor.py <edits-empty-run> --binary /abs/path/gitturtle --custom-themes 0
python3 drive_theme_apply.py <switches-empty-run> --binary /abs/path/gitturtle --custom-themes 0
python3 drive_theme_apply.py <switches-32-run> --binary /abs/path/gitturtle --custom-themes 32
python3 build_record.py --recorded <recorded-run> --warmup <warmup-run> --switches <switches-32-run> --switches-empty <switches-empty-run> --edits-empty <edits-empty-run> --date <YYYY-MM-DD> --out /abs/new-record.json
python3 render_record_md.py /abs/new-record.json
```
