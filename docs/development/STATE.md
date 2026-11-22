# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-22
- **Latest `main`:** `c35b267f` (PR #241 Wave 0A safety/CI fixes & PR #242 Wave 0B/0C harness/CI/bug sweep merged green).
- **Active branch / PR:** `perf/wave3a-quick-wins` (Roadmap PR 3 of 10 — High-ROI runtime performance & memory quick wins).
- **Completed in PR 3:**
  - Commit graph & preview memory: straight vertical pass-through/incoming graph lanes painted directly via uniform device-pixel `window.paint_quad(fill(...))` with horizontal viewport culling; replaced `ImageSide.image: Option<ImagePreview>` with `ImageMetadata` and swizzled RGBA→BGRA in place across `worker`, `gif_playback`, `pdf_view`, `model_view`, `markdown_view`, and `rich_preview`.
  - Lazy diff & startup work: deferred `SplitPresentation::prepare` behind `Option<Arc<SplitPresentation>>` and on-demand `Job::PrepareSplit` with exact immutable `PreviewCache::bytes` accounting; deferred `SettingsUi` (`SettingsPage` + 4 inputs), theme picker card entities, and `ProjectHub` search/form inputs until first entry.
  - Core Git spawn & polling elimination: 1-spawn `remotes()` with `insteadOf`/`pushInsteadOf` rewriter (-88.1%), `status()` reusing verified `git_dir` (-32.2%), `operation_state()` fast sentinel check (-97.9%), 1-spawn `Reported::by_git` (-33.7%) & `git_directories()`, 1-spawn `local_watch_policy()` (-55.1%), 0-spawn `RepositoryIdentity::capture`, single-blob `--porcelain` `blame(Working)` (-25.6%), narrowed `apply_partial` lock scope (-19.9%); replaced 1–5 ms `thread::sleep` loops with event-driven `CancelWaiters`/`Condvar`/`libc::poll` + `pidfd_open`/`waitid` across `git-core`, `github::transport`, `system_preview::launch`, and `operations`.
  - Linux zero-copy helper transfer: sealed read-only `memfd_create` + `PROT_READ` `mmap` before seccomp with safe pipe/unsealed fallback (`serve_buffer` / `serve_slice` / `prepare_data`), cutting 16 MiB PDF helper latency by -63.8% (13.70 → 4.96 ms).
  - Evidence recorded in [2026-11-22-wave3a-performance-wins.md](../benchmarks/2026-11-22-wave3a-performance-wins.md).
- **Next concrete steps:**
  1. PR 4 — Architecture modularity, Rust stability & fuzz/property verification (`RepositoryWorkspace`, `crates/app/src/lib.rs`, `worker/` split, pure headless crates, `crates/preview` unwrap removal + `objc2`/`bytemuck`/`rustix`, typed errors, proptest/fuzz/Miri).
  2. PRs 5–10 — Large-repo scalability, daily-driver Git power, ergonomics/syntax/GitHub v2/polish, macOS native craft, Omarchy/Linux craft + packaging prep, and vendor diet/website readiness.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12`, OS close/`Quit` refusal while a Git write runs, VoiceOver/Orca Slider `SetValue`, `About GitTurtle` in macOS menu bar, and Wave 3A straight-quad graph lanes + lazy Split/Settings transitions.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, `website/` Cloudflare deploy, and opt-in auto-update design.
