# History scrolling with the compact scope toolbar, 2026-09-30

This is the release measurement for `narrow-window-compare-height`, whose History scope toolbar renders on every scroll event. Below a breakpoint measured from shaped, cached label widths, the toolbar draws 28 px icon buttons; above it, the labelled toolbar takes the same code as before. The question is whether either path got slower. Raw per-notch samples are in [`2026-09-30-narrow-history-scroll.json`](2026-09-30-narrow-history-scroll.json).

## Setup

- Builds, designated in writing before the first recorded launch: base `main` `6db2269`, release, clean, sha256 `1a2dab0e…0ea0a`; candidate `37fe39f`, release, clean, sha256 `6da84f59…205ce` (`--build-info` and sha256 checked).
- Host: AMD 3020e, two cores and no P/E split, so pinning to P-cores does not apply; every launch ran under `taskset -c 1` and the driver on CPU 0. `schedutil`, on AC power. Omarchy, Hyprland 0.56.2, Mesa 26.2.2 RADV, native Wayland, a temporary headless output at 60 Hz. Load average 0.3 to 1.7.
- Fixture: a disposable 3,000-commit repository at `/tmp/gitturtle-evidence/perf-9/history` (HEAD `80558e9`, first page "All history 1–500"), unchanged after every run.
- Cache: each launch had a fresh HOME and XDG directories, so GPU shader caches started cold; 20 warm-up wheel notches ran before recording. The OS file cache was warm.
- Windows: wide 1480 × 800 fullscreen at scale 1, where both builds draw the labelled toolbar pixel for pixel; narrow 461 × 490 at scale 2, where the candidate draws the compact toolbar and the base its clipped labelled one.
- Input: per launch, 120 recorded wheel notches (30 down, 30 up, twice), one every 150 ms, from a virtual-pointer client, with the pointer on the History list; no keys. Launches interleaved: wide BCCBBC and narrow CBBCCB with the clock held at 2.54 to 2.59 GHz by a lowest-priority busy loop on CPU 1, then wide BCCB and narrow CBBC without it.

## Metrics

- Scene time: from the app handling a wheel event to the frame's content being handed to the renderer (scroll handling, the root render with `render_history` and the scope toolbar, layout, prepaint and paint), read from the app's Wayland log (`WAYLAND_DEBUG=1`, identical in both builds). An idle notch draws at once, so the 60 Hz tick does not enter it; notches that waited for a tick are reported separately.
- UI-thread CPU per notch: the main thread's CPU time over the 150 ms after the notch, from `/proc/<pid>/task/<pid>/schedstat`.
- Renderer submission, to the driver's `wl_surface.commit`, is about 1.0 ms wide and 0.75 ms narrow in both builds. GPU completion, compositing and scanout are not included.

## Results, clock held

Percentiles by nearest rank; Δ is candidate minus base with a 95% bootstrap interval (10,000 resamples within launches). Milliseconds.

| Path, metric | n (B/C) | base p50 / p95 / p99 / max | candidate p50 / p95 / p99 / max | Δp50 | Δp95 |
| --- | --- | --- | --- | --- | --- |
| Wide, scene | 360/360 | 9.692 / 11.923 / 14.433 / 16.245 | 9.624 / 11.129 / 13.265 / 15.403 | −0.068 [−0.101, −0.039] | −0.794 [−1.651, +0.169] |
| Wide, UI CPU | 360/360 | 11.236 / 12.324 / 13.122 / 13.667 | 11.124 / 12.056 / 12.458 / 12.889 | −0.111 [−0.146, −0.077] | −0.268 [−0.785, +0.250] |
| Narrow, scene (idle notches) | 314/313 | 6.599 / 8.015 / 10.329 / 14.382 | 6.506 / 7.534 / 8.451 / 29.392 | −0.093 [−0.112, −0.059] | −0.481 [−0.923, −0.009] |
| Narrow, UI CPU (idle notches) | 314/313 | 7.786 / 8.622 / 8.963 / 9.382 | 7.702 / 8.495 / 9.078 / 9.369 | −0.083 [−0.116, −0.056] | −0.127 [−0.310, +0.204] |

The per-launch medians of wide scene time spread by 0.10 to 0.20 ms within each build, as much as any p50 difference, and every p99 interval spans zero. The narrow candidate's 29.392 ms maximum is scheduling: its main thread waited 26.55 ms to run and spent a normal 9.36 ms of CPU; the base's 14.382 ms maximum is the same kind (a 12.6 ms wait). Without the clock held the costs are about three times higher and lead to the same conclusion (wide scene p50 29.033 base, 28.285 candidate; narrow 19.888 and 19.278).

## Verdict

No regression on either path: wide, the candidate is indistinguishable from the base; narrow, it is not slower. Its slightly lower narrow cost may come from the compact toolbar laying out no text where the base lays out clipped labels, but that is near the noise and not claimed as a speed-up. The measurement resolves about 0.1 ms at p50, so it bounds the added per-frame cost of the cached widths below that rather than showing it is zero.

Not measured: the commit-file-list and selection frames, Compare's review options row, and the tooltip, which lays out only while shown. The fixture is synthetic and only its first page was scrolled. In both builds a vertical wheel over narrow History also scrolls the table sideways, because the list's container scrolls only horizontally and GPUI turns vertical wheel movement into horizontal scrolling there; that predates this change.
