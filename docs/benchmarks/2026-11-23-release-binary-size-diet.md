# Release Binary Size Diet & Cold-Start Lean-Out — Release Benchmark Record

- **Date:** 2026-11-23
- **Baseline:** `main` at `31d31af2bfeafc4fd87a03447ca4be8a1b35ddcd` (`[profile.release]` `opt-level = 3` default, `lto = "thin"`, `codegen-units = 8`, `strip = "debuginfo"`)
- **Candidate branch:** `wt/binary-size-diet` (`[profile.release]` `opt-level = "s"`, `lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"`, per-package `opt-level = 3` for UI framework, app, macOS Metal/CoreText, Linux wgpu/text-shaping, core, diff-model, preview, syntax runtime, and raster/image hot crates, plus `-Wl,-x` on the release `gitturtle` binary via `crates/app/build.rs`)
- **Host:** `Linux-7.2.11-14-1-ARCH-aarch64-with-glibc2.43` (Apple M2 Pro, Omarchy Linux; 4 E-cores `0-3` @ 2.42 GHz, 8 P-cores `4-11` @ 3.50 GHz), pinned to P-cores `4-11` (`taskset -c 4-11`), quiet build slot active (`scripts/dev/build_slot.py quiet-on`)
- **Toolchain:** `rustc 1.100.0 (0ef1bc386 2026-11-09)`, `git version 2.56.0`, `--release` profile

## 1. Binary & Package Size Comparison (`aarch64-unknown-linux-gnu`)

- **Baseline binary SHA-256:** `71ae123360c827f7b2dfddc972f802a62ed267e8e767ea7d66ca3eada2ecaae2`
- **Candidate binary SHA-256:** `97c0bcf680ed437f2b4800fa27875734e5e605ac4a9f14e3ae485b9dfcc3c0e3`

| Artifact / ELF Section | Baseline (`31d31af2`) | Candidate | Delta |
| --- | ---: | ---: | ---: |
| Raw release binary (`gitturtle`) | 142.94 MiB (`149,881,976 B`) | **106.77 MiB** (`111,952,288 B`) | **-36.17 MiB (-25.3%)** |
| Packaged tarball (`scripts/package-linux.sh`) | 41.66 MiB (`43,686,234 B`) | **32.37 MiB** (`33,943,633 B`) | **-9.29 MiB (-22.3%)** |
| `.text` | 52.93 MiB | 39.53 MiB | -13.40 MiB (-25.3%) |
| `.rodata` | 43.33 MiB | 43.04 MiB | -0.29 MiB (-0.7%) |
| `.strtab` | 20.39 MiB | 9.59 MiB | -10.80 MiB (-53.0%) |
| `.symtab` | 10.68 MiB | 1.50 MiB | -9.18 MiB (-86.0%) |
| `.eh_frame` | 5.81 MiB | 4.61 MiB | -1.20 MiB (-20.7%) |
| `.gcc_except_table` | 3.10 MiB | 2.32 MiB | -0.78 MiB (-25.2%) |
| `.rela.dyn` | 3.23 MiB | 2.96 MiB | -0.27 MiB (-8.3%) |
| `.data.rel.ro` | 2.44 MiB | 2.22 MiB | -0.22 MiB (-8.8%) |

Notes:
- All 28 Tree-sitter grammars (~36.6 MiB static parse tables in `.rodata`), all preview formats, embedded DejaVu/IBM Plex fonts (2.35 MiB), and confined PDF helper embedded fonts/CMaps remain intact; `Cargo.lock` is unchanged.
- Panic unwinding (`panic = "unwind"`) is preserved for `catch_unwind` isolation in `crates/app/src/worker/` and `crates/app/src/operations.rs`.
- `scripts/package-linux.sh` ELF audit passes cleanly: `no NEEDED libdav1d, .symtab present, 0 dav1d_* symbols imported and none defined, no RPATH or RUNPATH`. `-Wl,-x` strips input local (`STB_LOCAL`) symbols (including 265k+ AArch64 `$d`/`$x` mapping symbols, `GCC_except_table*` locals, and `static` C helpers) while preserving `.symtab` and all global/weak symbols (including any statically linked C `dav1d_*` exports).
- Build-resource check: `lto = "fat"` peaked at `10.64 GiB` RSS (exceeding GitHub's 7 GB `macos-26` runner RAM); retaining `lto = "thin"` + `codegen-units = 1` peaks at `4.58 GiB` RSS (`4,685.8 MiB`) and builds `gitturtle --release` cold in ~2m 45s on M2 Pro with a `2.6 GiB` clean `target/release` footprint (inside CI's `3.5 GiB` release cache cap).
- Evaluated `opt-level = "z"` for cold dependencies (grew slightly larger than `-Os` due to ~21k extra non-inlined wrapper symbols inflating `.strtab`/`.eh_frame`) and `opt-level = "s"` for `crates/app` (caused +18–24% regressions in 20k-line diff review expansion, so `gitturtle`, `gpui-pre`, and `gpui-pre-wgpu` stay pinned at `opt-level = 3`).

## 2. Shipped Binary Startup & Confined Helper Benchmarks (P-cores `4-11`)

Same-session alternating rounds flipping baseline/candidate order each round (5 warmups + 25 rounds for `--build-info`; 3 warmups + 15 rounds for each confined helper workload).

| Workload | Baseline median (IQR) | Candidate median (IQR) | Candidate p95 | Delta | Assessment |
| --- | ---: | ---: | ---: | ---: | --- |
| `exec --build-info` (startup) | 1.583 ms (0.377 ms) | 1.485 ms (0.264 ms) | 2.077 ms | -6.2% | Improved / within noise |
| Confined PDF helper (vector + text) | 4.711 ms (0.416 ms) | 4.691 ms (0.429 ms) | 5.253 ms | -0.4% | Within noise floor |
| Confined PDF helper (8 MiB PDF) | 23.826 ms (0.579 ms) | 24.071 ms (0.906 ms) | 25.602 ms | +1.0% | Within noise floor |
| Confined JPEG 2000 helper (`half-red-blue.jp2`) | 3.389 ms (0.368 ms) | 3.179 ms (0.423 ms) | 4.035 ms | -6.2% | Improved / within noise |
| Confined AVIF helper (`half-red-blue.avif`, `dlopen` `dav1d`) | 3.259 ms (0.708 ms) | 3.143 ms (0.509 ms) | 3.589 ms | -3.6% | Within noise floor |
| Confined 3D FBX helper (`ufbx` `tetra.fbx`) | 3.323 ms (0.470 ms) | 2.997 ms (0.632 ms) | 3.470 ms | -9.8% | Improved / within noise |

### Raw samples (ms)

- `exec_build_info` (25 rounds):
  - baseline: `[1.525, 1.358, 1.317, 1.301, 1.101, 1.293, 1.262, 2.465, 1.444, 1.430, 1.635, 2.102, 2.004, 1.511, 1.780, 1.572, 1.400, 1.675, 2.092, 1.696, 1.732, 1.731, 1.870, 1.583, 1.729]`
  - candidate: `[1.237, 1.450, 1.131, 1.358, 1.613, 1.471, 1.205, 1.420, 1.420, 1.429, 2.077, 1.498, 1.527, 1.583, 1.420, 1.485, 1.394, 1.543, 2.175, 1.282, 1.748, 1.967, 1.669, 1.673, 1.895]`
- `pdf_vector_text` (15 rounds):
  - baseline: `[4.711, 4.283, 4.859, 5.120, 4.858, 4.628, 4.612, 4.320, 3.874, 5.402, 4.763, 4.442, 4.809, 4.735, 4.612]`
  - candidate: `[4.691, 4.591, 4.414, 4.958, 4.636, 4.774, 4.764, 4.204, 4.919, 4.535, 4.289, 4.490, 4.810, 5.253, 4.926]`
- `pdf_8mib` (15 rounds):
  - baseline: `[24.048, 24.033, 23.180, 24.832, 23.840, 23.826, 23.561, 24.450, 23.251, 24.251, 24.140, 23.801, 23.775, 23.100, 23.764]`
  - candidate: `[24.655, 24.068, 23.138, 23.033, 23.675, 23.659, 24.556, 24.249, 24.585, 23.232, 24.256, 24.071, 25.602, 23.817, 24.565]`
- `jp2_helper` (15 rounds):
  - baseline: `[3.758, 3.113, 2.684, 3.335, 3.028, 3.389, 3.212, 4.085, 3.319, 3.408, 3.475, 3.710, 3.387, 3.404, 3.580]`
  - candidate: `[3.114, 2.899, 3.666, 3.367, 3.282, 3.537, 3.095, 3.151, 3.172, 3.130, 2.953, 4.035, 3.479, 3.791, 3.179]`
- `avif_helper` (15 rounds):
  - baseline: `[3.106, 3.880, 2.958, 2.688, 3.414, 2.739, 3.329, 3.518, 2.852, 2.782, 3.560, 3.671, 4.784, 3.259, 3.064]`
  - candidate: `[3.434, 3.142, 2.925, 2.517, 3.220, 3.143, 3.498, 2.779, 3.396, 2.974, 3.589, 3.457, 3.295, 3.117, 2.834]`
- `fbx_helper` (15 rounds):
  - baseline: `[3.190, 3.617, 3.382, 3.380, 3.417, 3.269, 3.337, 3.393, 2.728, 3.342, 3.323, 2.913, 3.200, 2.547, 2.830]`
  - candidate: `[3.231, 2.548, 3.061, 2.712, 3.179, 3.046, 3.470, 2.369, 2.494, 2.301, 2.997, 2.674, 3.369, 3.080, 2.871]`

## 3. App CPU Preparation & Path-Filter Benchmark (20k Lines / 50k Paths, P-cores `4-11`)

Same-session alternating rounds (3 alternating runs × 40 post-warmup samples = 120 samples per series) of `working_selection::benchmarks::prepared_review_and_cached_paths_release_benchmark`.

- **Baseline release test binary SHA-256:** `423b7902531c6c2fadf4b30eac9e3db1deee8d0540731b79917fb9062b4cd43f`
- **Candidate release test binary SHA-256:** `f06958f4c1de479c50f0b421d87582da1b203dbd5319209490752ffd0b175901`

| Series (`n = 120` each) | Baseline median (IQR) | Candidate median (IQR) | Candidate p95 | Delta |
| --- | ---: | ---: | ---: | ---: |
| `history_cached_absent_50k` | 0.489 ms (0.012 ms) | 0.488 ms (0.016 ms) | 0.512 ms | -0.2% |
| `history_cached_all_50k` | 0.323 ms (0.010 ms) | 0.321 ms (0.010 ms) | 0.344 ms | -0.8% |
| `history_cached_renamed_50k` | 0.378 ms (0.012 ms) | 0.378 ms (0.010 ms) | 0.400 ms | 0.0% |
| `history_cached_selective_50k` | 0.717 ms (0.018 ms) | 0.717 ms (0.022 ms) | 0.756 ms | 0.0% |
| `history_prepare_index_50k` | 7.514 ms (0.139 ms) | 7.494 ms (0.102 ms) | 7.602 ms | -0.3% |
| `intraline_24k_byte_line_fallback` | 0.009 ms (0.000 ms) | 0.009 ms (0.000 ms) | 0.009 ms | +0.4% |
| `patch_intraline_201_hunks` | 1.490 ms (0.055 ms) | 1.504 ms (0.080 ms) | 1.721 ms | +1.0% |
| `review_expand_context192` | 5.328 ms (0.147 ms) | 5.268 ms (0.099 ms) | 5.386 ms | -1.1% |
| `review_expand_context48` | 5.403 ms (0.124 ms) | 5.372 ms (0.074 ms) | 5.475 ms | -0.6% |
| `review_hide_whitespace_context3` | 2.822 ms (0.063 ms) | 2.822 ms (0.078 ms) | 2.957 ms | 0.0% |
| `review_hide_whitespace_context48` | 4.006 ms (0.164 ms) | 3.945 ms (0.101 ms) | 4.114 ms | -1.5% |
| `split_intraline_20k_lines` | 2.556 ms (0.111 ms) | 2.539 ms (0.093 ms) | 2.687 ms | -0.7% |
| `working_cached_flat_selective_50k` | 0.582 ms (0.024 ms) | 0.588 ms (0.029 ms) | 0.653 ms | +0.9% |
| `working_cached_grouped_absent_50k` | 0.449 ms (0.012 ms) | 0.445 ms (0.009 ms) | 0.468 ms | -0.8% |
| `working_cached_grouped_all_50k` | 51.593 ms (6.377 ms) | 50.217 ms (4.984 ms) | 56.776 ms | -2.7% |
| `working_cached_grouped_selective_50k` | 0.766 ms (0.023 ms) | 0.766 ms (0.023 ms) | 0.840 ms | +0.1% |
| `working_prepare_index_50k` | 4.274 ms (0.156 ms) | 4.240 ms (0.158 ms) | 4.426 ms | -0.8% |

## 4. Scope & Unexercised Platforms

- Measured locally on Linux ARM64 (Apple M2 Pro P-cores `4-11`). Linux x86-64 (`rust-lld`) and macOS ARM64 (Xcode 26 `ld`) release compilation, packaging, and `-Wl,-x` link behavior were not exercised locally; pending hosted GitHub Actions `ubuntu-24.04` and `macos-26` runs.
