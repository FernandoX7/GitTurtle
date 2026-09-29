# Code font features and the bundled DejaVu Sans Mono, 2026-09-29

This is the release measurement for the `measure` criterion of `bundled-font-feature-skip`. It times per-line shaping of the bundled code font, DejaVu Sans Mono, with the feature list the base hands the text system for it (`calt=0`, `liga=0`) against the one the candidate hands it (none). The [driver and its procedure](2026-09-29-code-font-features/README.md) are committed beside this record, and the raw samples are in [`results/`](2026-09-29-code-font-features/results/).

## Why a feature-list comparison is the base-against-candidate measurement

The change alters one shaping input: the feature list that code text in the bundled family carries. The base, `83f65cc`, hands `calt=0`, `liga=0` to every code family, through `code_font_features()` and `CodeFont::code_font` (`crates/app/src/appearance.rs:144-155`) and the editor wrapper (`crates/app/src/editor_find.rs:232-234`). The candidate hands the bundled family an empty list and every other family the same `calt=0`, `liga=0` as before, which the tests pin (`appearance::tests::code_text_in_the_bundled_family_shapes_without_features`, `appearance::tests::code_text_shapes_the_code_family_without_ligatures`, `editor_find::tests::editors_draw_code_without_ligatures`). GPUI 0.3.4's Linux text system turns the list into cosmic-text features in `cosmic_font_features` (`gpui-pre-wgpu` `src/cosmic_text_system.rs`), which maps an empty list to empty cosmic-text features. Everything else in the shaping call is the same in both builds, so shaping the same lines with the two lists measures the change.

Comparing two app builds directly would need a per-line shaping trace, which the app does not have. The difference is about 0.25 ms per 120-line screen of newly shown code, which cannot show through the frame-tick metrics.

## Setup

- Host: AMD 3020e, two cores with one thread each, 5.7 GiB, Omarchy on Linux 7.2.5-3-omarchy. `acpi-cpufreq` with the `schedutil` governor, boost on, on AC power (battery full). Every run was pinned to core 1 with `taskset -c 1`.
- Toolchain: Rust 1.98.0 (the repository's `rust-toolchain.toml`), release profile as the app's (`lto = "thin"`, `codegen-units = 8`). The driver links cosmic-text 0.19.0 with its default features, and its `Cargo.lock` matches cosmic-text's subgraph in the workspace lock (harfrust 0.5.2, fontdb 0.23.0, swash 0.2.10).
- Driver: built from the tree at `0f324ed` with the review fixes that ship with this record (nearest-rank percentiles, per-round ratios, commit and fallback counts), executable sha256 `b6b682f263012c1751197fa915d944de52726b3feb0335608b533c80d4ebadba`.
- Corpus: `crates/app/src` at `0f324ed`, exported with `git archive`: the first 5,000 non-empty lines of its top-level `.rs` files, 214,029 bytes, FNV-1a `53b504f9b60b8acb`, 213,882 glyphs. All of them came from the Regular face: no fallback to another face and no missing glyph. The base and candidate glyph ids were identical for every line.
- Fonts: the four bundled faces from `assets/fonts/dejavu-sans-mono`, never a system copy, plus `/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf` for the glyph check. Code size 12 px.
- Cache state: warm. The driver reads the corpus and maps the fonts first; the glyph check, the base-against-candidate glyph comparison over the whole corpus and two warm-up rounds all shape before the first timed sample. No cold cache was measured.
- Sampling: 30 rounds per run after the warm-up. Each round shapes the corpus once per configuration, back to back, alternating which goes first. Percentiles are by nearest rank (with 30 samples, p50 is the 15th smallest and p95 the 29th).
- Quiet: the runs started once the one-minute load average fell below 0.5 with no `cargo` or `rustc` running.

| Run | Start and end (UTC) | Load before → after | cpu1 before → after |
| --- | --- | --- | --- |
| 1 | 16:10:40 – 16:10:50 | 0.39 → 0.49 | 878 MHz → 2,594 MHz |
| 2 | 16:10:50 – 16:10:59 | 0.49 → 0.57 | 2,595 MHz → 2,594 MHz |
| 3 | 16:10:59 – 16:11:09 | 0.57 → 0.63 | 2,594 MHz → 2,594 MHz |

The load average rises during the runs because the driver itself keeps core 1 busy. Core 1 was at its idle frequency before run 1 and boosted by the end of each run.

## Commands

From the repository root, with `$CORPUS` an empty directory outside the repository:

```sh
git archive 0f324ed crates/app/src | tar -x -C "$CORPUS"
cd docs/benchmarks/2026-09-29-code-font-features
CARGO_TARGET_DIR="$PWD/../../../.local/shapebench-target" cargo build --release --locked
sha256sum ../../../.local/shapebench-target/release/code-font-features
# wait for a one-minute load average below 0.5 and no cargo or rustc process
for run in 1 2 3; do
  date -u; uptime; cat /sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq
  taskset -c 1 ../../../.local/shapebench-target/release/code-font-features \
    --samples 30 \
    --corpus "$CORPUS/crates/app/src" \
    --commit 0f324ed50fd462f90ee3c3f8b9e13d4a7fa862f4 \
    --ligature-font "$(fc-match -f '%{file}' 'JetBrainsMono Nerd Font')" \
    --json results/run-$run.json
  date -u; uptime; cat /sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq
done
```

## Result

Microseconds per line, 30 samples per configuration per run:

| Run | Base p50 | Base p95 | Base max | Candidate p50 | Candidate p95 | Candidate max | Candidate / base at p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 29.62 | 32.68 | 46.04 | 27.71 | 32.96 | 51.87 | 0.936 |
| 2 | 29.03 | 32.50 | 35.20 | 27.14 | 56.29 | 61.11 | 0.935 |
| 3 | 29.60 | 34.01 | 44.21 | 27.40 | 32.13 | 48.74 | 0.926 |

Per corpus of 5,000 lines, the p50 went from 148.10, 145.17 and 147.98 ms to 138.56, 135.68 and 137.02 ms.

Paired by round, where both configurations ran back to back, the candidate/base ratio has a median of 0.934, with quartiles of 0.924 and 0.941 over all 90 rounds. The candidate was faster in 83 of the 90 rounds (28, 27 and 28 per run), and the per-run medians were 0.934, 0.935 and 0.923. Put the other way, the features cost the bundled font 7 to 8% more shaping time per line at p50, about 1.9 to 2.2 µs, or 0.23 to 0.26 ms for a 120-line screen of newly shown code on this host.

- **Tails.** With 30 samples, p95 is the second-largest sample, so p95 and max each come from one or two rounds. The largest samples of both configurations fall in the same rounds: rounds 10 and 25 in run 1, 12 and 19 in runs 2 and 3. Each of those host interruptions hit both configurations, landing most heavily on whichever was running. Run 2's candidate p95 of 56.29 µs and max of 61.11 µs come from rounds 11 and 19, where the base samples were also among that run's four slowest (30.28 µs, and its maximum of 35.20 µs). The tails describe the host, not the change; the p50s and the median and quartiles of the paired ratios are insensitive to them.
- **Glyphs.** Skipping the features changed no glyph of the bundled font over the corpus: the driver compares both configurations' glyph ids line by line before timing, and `desktop_text::tests::bundled_code_font_draws_the_same_glyphs_without_features` checks the same through GPUI's text system for all four faces.

### Glyph check

Every run printed the same check. JetBrainsMono Nerd Font under the desktop families' code features draws each pair as its characters' own glyphs; under its defaults, it joins every pair into a spacer glyph (12607) followed by a ligature glyph:

```text
glyphs: JetBrainsMono Nerd Font, desktop code features calt=0 liga=0 against its defaults
  ->  code features: 2 glyphs [17, 34] (separate); defaults: 2 glyphs [12607, 12389] (ligature)
  !=  code features: 2 glyphs [5, 33] (separate); defaults: 2 glyphs [12607, 12420] (ligature)
  =>  code features: 2 glyphs [33, 34] (separate); defaults: 2 glyphs [12607, 12492] (ligature)
  ==  code features: 2 glyphs [33, 33] (separate); defaults: 2 glyphs [12607, 12489] (ligature)
  <=  code features: 2 glyphs [32, 33] (separate); defaults: 2 glyphs [12607, 12525] (ligature)
  --  code features: 2 glyphs [17, 17] (separate); defaults: 2 glyphs [12607, 12384] (ligature)
  //  code features: 2 glyphs [19, 19] (separate); defaults: 2 glyphs [12607, 12447] (ligature)
glyphs: ok
```

## What the metric spans

Each timed line covers what GPUI's `layout_line_no_separators` does in cosmic-text: building the run's `Attrs` with a clone of its features, a default `AttrsList` with one span, `ShapeLine::new` with advanced shaping and a tab width of 4, and `layout_to_buffer` without wrapping, ellipsis, alignment or hinting, plus collecting the glyph ids. It excludes GPUI's font id resolution and fallback-chain spans, the conversion to `ShapedGlyph`, the line layout cache (lines laid out in the previous frame are reused, so the cost falls on newly shown lines), glyph rasterization, element layout and paint. Those are the same for both configurations.

## Agreement with earlier figures

- An earlier set of three runs by the performance reviewer on the same host, at 15:45 UTC with the pre-review driver (sha256 `a116ae94…`, rounding percentiles, working-tree corpus), gave p50 base 29.17, 29.48 and 29.21 µs against candidate 27.17, 27.25 and 27.05 µs, ratios of 0.931, 0.924 and 0.926. It agrees.
- The [September 27 entry](../validation.md#september-27-code-text-without-ligatures) reported 23.8 to 26.5 µs per line at p50 for the same two lists, about 11% more with the features. That came from a different, uncommitted harness at a load average of about 1.3. It agrees in direction; the size here is about 7%.

## Not measured

- No cold cache, no other host, and no other code size than 12 px. Only the Regular face was timed; the other faces are covered by the glyph test, not by timing.
- macOS is unchanged by this skip: its code family is Menlo, not the bundled family, so it keeps `calt` and `liga` off, and its CoreText shaping was not measured.
- A desktop family such as JetBrains Mono still gets `calt=0`, `liga=0`, so its shaping cost is unchanged.
- No native frame measurement; see above for why it could not resolve the difference.
