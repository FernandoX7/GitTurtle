# Code font features and the bundled DejaVu Sans Mono, 2026-09-29

This driver backs the `measure` and `ligatures` criteria of the `bundled-font-feature-skip` task; the [record](../2026-09-29-code-font-features.md) gives its results. It measures per-line shaping time of the bundled code font, DejaVu Sans Mono, with the font features that the base and the candidate hand the text system for it, and it checks that a ligature font still draws code as typed. It is a standalone crate with its own `[workspace]`, so it is not a member of the GitTurtle workspace.

## What the two configurations are

| Configuration | Features | Build that hands them to the text system for the bundled family |
| --- | --- | --- |
| `base` | `calt=0`, `liga=0` | `83f65cc`, as on origin/main: `CodeFont::code_font` and `editor_find::Editor` applied `code_font_features()` to every code family |
| `candidate` | none | this change: `code_font_features_for(family)` returns an empty list for `desktop_text::BUNDLED_CODE_FAMILY` and `calt=0`, `liga=0` for every other family |

The tests pin both lists. `appearance::tests::code_text_in_the_bundled_family_shapes_without_features` paints a probe under `.code_font(cx)` with the bundled family and asserts an empty feature list. With the bundled family given the `calt`/`liga` list again, as on origin/main, it fails with `features: FontFeatures { calt: 0, liga: 0 }`. `appearance::tests::code_text_shapes_the_code_family_without_ligatures` asserts that a desktop family keeps `calt` and `liga` off. `editor_find::tests::editors_draw_code_without_ligatures` reads the style of the kit editor that the wrapper renders: `calt` and `liga` off for a desktop family, an empty list for the bundled family, and features a caller chose kept.

Skipping the features changes no glyph that the bundled font draws. No bundled face has contextual alternates (`calt`). The Regular and Bold faces register `liga` only for the Arabic script, where it forms lam-alef, which their required ligatures (`rlig`) form anyway; the oblique faces have no `liga`. The driver asserts identical glyph ids for both configurations over the whole corpus before it times anything. `desktop_text::tests::bundled_code_font_draws_the_same_glyphs_without_features` asserts the same through GPUI's own text system for all four faces, with a `dlig` control showing that the comparison notices a feature the faces do use.

## What the driver reproduces

`src/main.rs` shapes each line the way GPUI 0.3.4's Linux text system does (`gpui-pre-wgpu`, `CosmicTextSystem::layout_line_no_separators`): one span of the run's attributes over a default `AttrsList`, with the features cloned into the attributes for every line as GPUI does per run, then `ShapeLine::new` with advanced shaping and a tab width of 4 and `layout_to_buffer` with no wrap, ellipsis, alignment or hinting, at the default code size of 12 px. It links cosmic-text 0.19.0 with its default features, as `gpui-pre-wgpu` does. The committed `Cargo.lock` was derived from the workspace `Cargo.lock`: every package in it resolves to the version, with the same dependencies, that cosmic-text's subgraph has in the workspace (harfrust 0.5.2, fontdb 0.23.0, swash 0.2.10, skrifa 0.40.0 and 0.44.0), so the shaping engine is the app's. The workspace adds only optional serde and value-bag dependencies of `log` and `smol_str`, which other crates enable and shaping does not use. Build with `--locked` to keep it that way.

The font database holds only the four bundled faces from `assets/fonts/dejavu-sans-mono` (plus the ligature font for the glyph check), never a system copy of the family. A character that the Regular face lacks would fall back among those faces, the same way for both configurations; the driver counts such glyphs, and glyphs no face has, over the corpus. The driver leaves out what GPUI adds around the shaping call, which is the same for both configurations: per-run font matching, the conversion to `ShapedGlyph`, and the line layout cache. GPUI reuses the previous frame's line layouts, so the per-line cost applies to lines newly shown by scrolling, opening a file or changing a comparison.

The corpus is the first 5,000 non-empty lines of the `.rs` files directly in `crates/app/src`, in path order. The driver prints the commit the corpus comes from (`git rev-parse HEAD` in the corpus directory, or `--commit`) and whether the corpus differs from it, and the corpus's line count, bytes, FNV-1a hash and glyph count, so a result names the corpus it used; both configurations always shape the same corpus in the same process. It runs two warm-up rounds and then the requested samples. Each sample is one round that shapes the corpus once per configuration, back to back, and the order alternates between rounds. It prints every raw sample as milliseconds per corpus and microseconds per line, and p50, p95 and max per configuration. Percentiles are by nearest rank: the p-th percentile of n samples is the ⌈p/100 × n⌉-th smallest, so with 30 samples p50 is the 15th and p95 the 29th. It also prints the candidate/base ratio of each round, with its median, quartiles and the number of rounds in which the candidate was faster. `--json FILE` writes the same figures; its raw arrays are in round order, so the i-th base and candidate samples come from the same round.

## Glyph check

`--ligature-font FILE` shapes `->`, `!=`, `=>`, `==`, `<=`, `--` and `//` in that font with the desktop families' features (`calt=0`, `liga=0`) and with the font's defaults. A pair passes when the code features draw each character's own glyph from the font's character map. Counting glyphs is not enough: JetBrains Mono draws a ligature as a spacer glyph followed by the ligature glyph, so a joined `->` is still two glyphs. The check also fails if the defaults leave `->` or `!=` unjoined, because such a font cannot show that the features work. `--glyphs` runs only this check.

## Procedure

1. Record the host: CPU model and core count (`lscpu`), frequency governor (`/sys/devices/system/cpu/cpu1/cpufreq/scaling_governor`), kernel (`uname -r`), memory, and the GitTurtle commit (`git rev-parse HEAD`, clean tree). The procedure was written on an AMD 3020e with two cores, one thread each, `schedutil` and 5.7 GiB, on Linux 7.2.5 (Omarchy).
2. Build into the repository's git-ignored `.local/`, so no `target/` lands under `docs/` (a local `.gitignore` also ignores `target/` here), and record the executable's sha256:

   ```sh
   cd docs/benchmarks/2026-09-29-code-font-features
   CARGO_TARGET_DIR="$PWD/../../../.local/shapebench-target" cargo build --release --locked
   sha256sum ../../../.local/shapebench-target/release/code-font-features
   ```

   The repository's `rust-toolchain.toml` (Rust 1.98.0) applies here too.
3. Quiet the host: close other work, and wait until `uptime` shows a one-minute load average below 0.5 and `pgrep -x cargo; pgrep -x rustc` find nothing.
4. Run the glyph check and the measurement three times, pinned to one core, recording the time, load average and core frequency before and after each run. On a two-core host, `taskset -c 1` keeps it off core 0, where most interrupts land; on a hybrid host, pick one performance core and record which.

   ```sh
   for run in 1 2 3; do
     date -u; uptime; cat /sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq
     taskset -c 1 ../../../.local/shapebench-target/release/code-font-features \
       --samples 30 \
       --ligature-font "$(fc-match -f '%{file}' 'JetBrainsMono Nerd Font')" \
       --json results/run-$run.json
     date -u; uptime; cat /sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq
   done
   ```

   `--corpus DIR` and `--fonts DIR` override the default corpus and font directories, and `--lines N` the corpus size. The default corpus is the working tree; to measure a committed one, export it with `git archive <commit> crates/app/src | tar -x -C DIR` and pass `--corpus DIR/crates/app/src --commit <commit>`, as the record did.
5. Cache state: warm. The driver reads the corpus and maps the fonts before it times anything, and the glyph comparison and the two warm-up rounds shape every line, so the font pages are resident before the first sample. Nothing measures a cold cache.
6. Record host, commit, executable sha256, core, load average and frequency before and after, the glyph-check lines, the p50, p95 and max of both configurations with the candidate/base ratio at p50, and the per-round ratios.

A three-sample smoke run proves only that the driver builds and the glyph check passes; it is not a result.
