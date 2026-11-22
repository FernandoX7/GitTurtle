# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-22
- **Latest `main`:** `2b582850` (PRs #241 Wave 0A, #242 Wave 0B/0C+5, and #243 Wave 3A merged green).
- **Active branch / PR:** `refactor/wave4-architecture-stability` (Roadmap PR 4 of 10 — Architecture modularity, Rust stability, and property/fuzz verification).
- **Completed in PR 4:**
  - Headless workspace crate extraction (zero new external dependencies in `Cargo.lock`): extracted `crates/theme` (`gitturtle-theme`), `crates/diff-model` (`gitturtle-diff-model`), `crates/prefs` (`gitturtle-prefs`), and `crates/github` (`gitturtle-github`) with `#![cfg_attr(not(test), deny(clippy::unwrap_used))]`, workspace MSRV inheritance, and strict GPUI/unsafe/`test-support` isolation enforced by `scripts/dev/check_invariants.py`.
  - App structural modularity: exposed `crates/app/src/lib.rs` with thin 3-line `src/main.rs` entry point, removing all 6 `#[allow(dead_code)]` shims and all 6 duplicate `#[path = "../src/..."]` / `#[expect(dead_code)]` integration-test blocks; grouped 45 per-tab fields into `RepositoryWorkspace` (reducing `WarmTab` from 53 fields to 9); split `worker.rs` into `worker/{mod,types,session,cache,prepare,tests,image_tests,helper_image_tests}.rs`; extracted ~124.6k lines of inline tests from the 10 largest app files into sibling `*_tests.rs` modules; restored retained preview editors immediately on `close_file_history`.
  - Panic/unsafe elimination & typed errors: removed all 34 production `.unwrap()`s in `crates/preview` and enabled `#![cfg_attr(not(test), deny(clippy::unwrap_used))]`; added `CoreError` (`crates/git-core/src/error.rs`) and `PreviewError` (`crates/preview/src/error.rs`) via `thiserror`; migrated macOS PDF rendering in `crates/preview/src/native.rs` to safe `objc2-core-foundation`/`objc2-core-graphics`, `crates/preview/src/model3d/glb/compression.rs` to `bytemuck`, and Unix/Linux syscalls (`crates/app/src/{desktop_text,pdf_helper,system_preview/{launch,store}}.rs`, `crates/github/src/transport.rs`) to safe `rustix`.
  - Property testing, fuzz targets & stricter lints: added deterministic fixed-seed `proptest` suites in `crates/git-core` and `crates/preview` plus excluded `fuzz/` libFuzzer harnesses (`preview_parsers`, `model3d_parsers`, `git_parsers`, `secret_redaction`); enabled `large_futures`, `redundant_clone`, and `cloned_instead_of_copied` workspace-wide (boxing a 19.9 KiB future in `desktop_text.rs`).
- **Not exercised locally:** macOS compilation of `crates/preview`/`crates/app` and live macOS desktop runtime (`aarch64-apple-darwin` verified locally via cross-clippy on 5 headless crates with and without `test-support`; hosted macOS CI verifies full macOS build/tests on PR push); nightly `cargo-fuzz` / Miri (stable Rust `1.100.0` only on host).
- **Next concrete steps:**
  1. PR 5 — Large-repo and monorepo scalability (2-lane worker, passive-read benchmark evaluation, >100k-file / >500k-commit cliffs).
  2. PRs 6–10 — Daily-driver Git power, power ergonomics/syntax/GitHub v2/polish, macOS native craft, Omarchy/Linux craft + packaging prep, and vendored patch diet/website readiness.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12`, OS close/`Quit` refusal during active Git writes, VoiceOver/Orca Slider `SetValue`, `About GitTurtle` in macOS menu bar, Wave 3A straight-quad graph lanes & lazy Split/Settings transitions, and Wave 4 macOS `objc2` PDF rendering + `close_file_history` editor restore.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, `website/` Cloudflare deploy, and opt-in auto-update design.
