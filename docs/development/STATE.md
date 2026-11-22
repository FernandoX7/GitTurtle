# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-22
- **Latest `main`:** `9194bcaa` (PRs #241 Wave 0A, #242 Wave 0B/0C+5, #243 Wave 3A, and #244 Wave 4 merged green).
- **Active branch / PR:** `perf/wave3b-monorepo-scalability` (Roadmap PR 5 of 10 — Large-repository and monorepo scalability).
- **Completed in PR 5:**
  - Pipelined object reads (`BatchPool`, zero new dependencies): upgraded `crates/git-core` to a bounded 2-reader pool speaking `git cat-file --batch-command --buffer`, wiring `read_blob_pair`, `blobs_bounded_independent`, `text_preview_with_sources`, `capture_preview_assets`, `blame_blob`, `lfs_download_plan`, and `ensure_commit_object` (-91.2% on 20-asset Markdown capture, -33.4% on 50 paired text previews).
  - Monorepo cliff fixes in `crates/git-core`:
    - `LocalWatchCoverage::GitMetadataOnly` streamed `ls-files` early degradation with non-leaking ignore cache accounting and steady-state `LocalChange.notice` (no watcher error/rebuild thrash on focus or tab switches).
    - Bounded untracked-directory expansion (`--untracked-files=normal` + descriptor-relative Unix `openat`/`Dir` probe capped at 256 dirs / 5,000 files / depth 128 / 250 ms + streamed `ls-files --others` + `O(F log D)` prefix matching), collapsing oversize untracked folders safely with explicit discard/stash/worktree-remove guards and calm Changes UI feedback.
    - No-commit-graph chronological fallback (`HistoryOrder::Chronological`, 4s first-commit timeout) with node-only graph rows and accessible `history-graph-notice`; zero-allocation streaming `HistorySearch` cursor (-56.1% across 3 Continue pages) with ASCII-fast-path + KMP `CaseInsensitiveMatcher`.
    - Compact arena-backed `TrackedPathIndex` + zero-spawn `.git/index` `IndexFingerprint` (-70.1% across 5 sequential Quick Open queries).
  - Bounded 2-lane app worker (`crates/app/src/worker/`): split `gitturtle-reader` (metadata/history/search/quiet reads) and lazy `gitturtle-preview` (diff/image/document previews) with `TrackedPathCache` (2 entries / 24 MiB LRU), parallel snapshot `worktrees()`/`branches()`, off-UI-thread search teardown, and preserved `Active::Preview` takeover.
  - Benchmark & architecture record: `docs/benchmarks/2026-11-22-wave3b-monorepo-scalability.md` (P-core `--release` alternating runs + zero-new-deps `BatchPool` vs `gix-*` evaluation).
- **Not exercised locally:** macOS compilation of `crates/preview`/`crates/app` and live macOS desktop runtime (`aarch64-apple-darwin` verified locally via cross-clippy on 5 headless crates with and without `test-support`; hosted macOS CI verifies full macOS build/tests on PR push).
- **Next concrete steps:**
  1. PR 6 — Clutter-free daily-driver Git power (Wave 1A: reviewed force-delete unmerged branch, hunk/line/multi-file discard, split-diff partial staging, rich context menus, `file:line` editor + terminal, collapsible Tags/Stashes + Jump to HEAD, pickaxe/filter chips, full remote lifecycle).
  2. PRs 7–10 — Power ergonomics/syntax/GitHub v2/polish, macOS native craft, Omarchy/Linux craft + packaging prep, and vendored patch diet/website readiness.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12`, OS close/`Quit` refusal during active Git writes, VoiceOver/Orca Slider `SetValue`, `About GitTurtle` in macOS menu bar, Wave 3A straight-quad graph lanes & lazy Split/Settings transitions, Wave 4 macOS `objc2` PDF rendering + `close_file_history` restore, and Wave 3B 2-lane responsiveness / collapsed untracked folders / `GitMetadataOnly` watch notice.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, `website/` Cloudflare deploy, and opt-in auto-update design.
