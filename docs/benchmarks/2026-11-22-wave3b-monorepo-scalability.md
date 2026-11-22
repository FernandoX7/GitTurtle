# Wave 3B Large-Repository & Monorepo Scalability — Release Benchmark & Architecture Record

- **Date:** 2026-11-22
- **Baseline:** `main` at `9194bcaab` (post-Wave 4)
- **Candidate branch:** `perf/wave3b-monorepo-scalability`
- **Host:** `Linux-7.2.11-14-1-ARCH-aarch64-with-glibc2.43` (Apple M2 Pro, Omarchy Linux), pinned to P-cores `4-11` (`taskset -c 4-11`), quiet build slot active (`scripts/dev/build_slot.py quiet-on`)
- **Toolchain:** `rustc 1.100.0 (0ef1bc386 2026-11-09)`, `git version 2.56.0`, `--release` profile

## 1. Core Monorepo & Object-Pipeline Hot Paths (`gitturtle-core`)

Same-session alternating comparison (2 warmup rounds + 11 alternating rounds flipping baseline/candidate order each round) against disposable release fixtures:
- **12,000-file repository** (with 20 embedded Markdown image assets, 50 modified tracked text files, a 12,000-file untracked `node_modules/` tree across 60 subdirectories, and a 3-file untracked `small_dir/`)
- **4,000-commit repository** for 3-page continued `search_history` / streaming `HistorySearch`

- **Baseline binary SHA-256:** `9f4af702d8190f812cfe78edf098171b5bce9644d026c3e9b5a895f6755a4f80`
- **Candidate binary SHA-256:** `74724e0261dc6831e38a8c478033b7bd80688f6732bc6e05a6146b0bfd89b11f`

| Operation | Baseline median (IQR) | Candidate median (IQR) | Candidate p95 | Delta | Significance |
| --- | ---: | ---: | ---: | ---: | --- |
| `capture_preview_assets` (20 Markdown images, pipelined `BatchPool`) | 33.244 ms (1.526 ms) | 2.932 ms (0.391 ms) | 3.433 ms | **-91.2%** | Significant |
| Quick Open 5 sequential queries (12k tracked files, `TrackedPathIndex`) | 15.451 ms (2.450 ms) | 4.620 ms (1.291 ms) | 7.217 ms | **-70.1%** | Significant |
| `search_history` / `HistorySearch` 3 Continue pages (4k commits) | 63.874 ms (6.687 ms) | 28.063 ms (4.534 ms) | 32.479 ms | **-56.1%** | Significant |
| `local_watch_policy()` (12k tracked files, streamed `ls-files`) | 9.970 ms (1.964 ms) | 6.479 ms (0.409 ms) | 8.588 ms | **-35.0%** | Significant |
| `text_preview_with_sources` (50 pairs, pipelined `read_blob_pair`) | 6.782 ms (1.107 ms) | 4.517 ms (0.191 ms) | 5.242 ms | **-33.4%** | Significant |
| `status()` (12k untracked `node_modules/` + 3-file `small_dir/`) | 44.506 ms (4.506 ms) | 46.302 ms (11.473 ms) | 52.431 ms | +4.0% | Within noise floor (bounds rows: 12,003 → 4) |

Note on `status()`: on a 12k-file untracked folder, `--untracked-files=normal` plus the bounded descriptor-relative probe (`256` dirs / `5,000` files / `250 ms` cap) and streamed `ls-files --others` for small siblings runs within the noise floor of `--untracked-files=all` while reducing returned `StatusEntry` rows from **12,003** to **4** (`node_modules/` collapsed + 3 expanded `small_dir/` files). For larger untracked trees (100k+ files), baseline scales linearly with total untracked files and UI rows while candidate work stays O(1)-capped by the probe budget.

### Raw samples (ms, 11 rounds)

- `markdown_20_assets`:
  - baseline: `[35.511, 34.422, 31.974, 33.243, 33.095, 28.241, 32.109, 33.380, 32.873, 34.034, 33.999]`
  - candidate: `[3.250, 2.963, 3.144, 2.841, 3.142, 2.838, 2.932, 2.665, 2.575, 2.460, 3.433]`
- `quick_open_5_queries`:
  - baseline: `[14.682, 14.630, 14.819, 17.681, 17.547, 16.949, 16.490, 13.066, 14.833, 15.451, 17.451]`
  - candidate: `[5.747, 4.563, 4.620, 3.772, 5.096, 4.166, 3.870, 5.402, 4.401, 5.886, 7.217]`
- `history_search_3pages`:
  - baseline: `[60.129, 70.859, 70.378, 61.518, 50.759, 63.874, 64.703, 59.249, 56.934, 67.818, 64.934]`
  - candidate: `[26.632, 25.826, 26.675, 31.585, 26.420, 30.534, 26.272, 28.956, 28.063, 32.221, 32.479]`
- `local_watch_12k`:
  - baseline: `[9.655, 9.653, 9.536, 9.970, 10.501, 9.730, 11.766, 11.771, 11.470, 15.368, 9.535]`
  - candidate: `[6.741, 5.581, 7.186, 6.479, 6.659, 6.503, 8.588, 6.300, 6.374, 6.281, 6.162]`
- `text_preview_50_pairs`:
  - baseline: `[6.289, 6.098, 6.796, 6.782, 7.666, 6.471, 7.489, 6.050, 7.918, 4.970, 7.113]`
  - candidate: `[5.242, 4.281, 5.118, 4.556, 4.127, 4.516, 4.388, 4.555, 4.507, 4.414, 4.627]`
- `status_giant_untracked`:
  - baseline: `[42.841, 42.787, 41.997, 40.856, 46.915, 44.506, 41.775, 58.911, 45.774, 46.882, 48.382]`
  - candidate: `[50.125, 51.436, 37.067, 36.347, 46.302, 52.431, 36.273, 41.548, 46.189, 52.192, 47.179]`

## 2. Passive Object-Read Evaluation: Zero-New-Deps Pipelined `BatchPool` vs In-Process `gix-*`

Wave 3B evaluated replacing `git cat-file --batch-command` with pure-Rust `gix-odb`/`gix-ref` crates against upgrading GitTurtle's existing zero-dependency `cat-file` protocol:

- **Decision:** Keep zero new external dependencies (`Cargo.lock` unchanged) and upgrade `crates/git-core` to a bounded 2-reader `BatchPool` speaking pipelined `git cat-file --batch-command --buffer` (`info`/`contents`/`flush`).
- **Measured rationale:**
  - Pipelined `BatchPool` cuts 20-asset Markdown document captures by **-91.2%** (33.24 ms → 2.93 ms) and 50 paired text diff reads by **-33.4%** (6.78 ms → 4.52 ms) with zero new crates, zero `unsafe` (`#![forbid(unsafe_code)]` preserved), zero `memmap2` `SIGBUS` exposure during concurrent `git gc`/`repack`, full Git 2.45+ `reftable` and SHA-256 compatibility, and zero CI cache growth against the 10 GB free-tier budget.
  - Adding `gix-odb` + `gix-ref` would introduce ~25–40 new crates and extra license/advisory surface while still requiring `git` for pack-index/reftable/filter/blame/diff semantics and offering marginal gain over pipelined 2.9 ms multi-object batches.

## 3. Structural Monorepo Scalability Safeguards (Verified by Unit & GPUI Tests)

- **Bounded 2-lane worker (`crates/app/src/worker/`):** Splits metadata/history/search/quiet reads (`gitturtle-reader`) from diff/image/document preview preparation (lazy `gitturtle-preview`) so heavy previews never block commit selection, history paging, or quiet refresh, while preserving single-active + single-pending supersession, cross-lane takeover, panic isolation, and off-UI-thread search teardown.
- **Monorepo watch degradation (`LocalWatchCoverage::GitMetadataOnly`):** Streams `git ls-files --cached -z` with early exit past tracked-path/byte limits and degrades cleanly to watching Git metadata directories only without watch-failure error loops or focus/tab-switch watcher thrash.
- **No-commit-graph topological fallback (`HistoryOrder::Chronological`):** Detects missing `.git/objects/info/commit-graph` and falls back to chronological ordering when `--topo-order` startup stalls past 4 seconds, showing node-only graph rows and an accessible `history-graph-notice`.
- **Compact `TrackedPathIndex` & `TrackedPathCache`:** Arena-backed path/OID storage paired with zero-spawn `.git/index` checksum fingerprinting (`IndexFingerprint`) and a 2-entry / 24 MiB LRU worker cache for repeat Quick Open queries.

## 4. Scope & Unexercised Platforms

- Measured locally on Linux ARM64 (Apple M2 Pro P-cores). Live macOS desktop runtime and macOS FSEvents behavior under `GitMetadataOnly` were not exercised on local macOS hardware; `aarch64-apple-darwin` headless compilation was cross-checked locally and full macOS build/tests run on GitHub Actions `macos-15`.
