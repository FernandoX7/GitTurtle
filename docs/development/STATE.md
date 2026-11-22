# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-22
- **Latest green `main`:** `c3c39307` (Phases 0–4 complete: Rust `1.100.0`, `gpui-kit 0.10.0` / `gpui-pre 0.3.12`, `jiff 1.0.2`, `cargo-deny 0.21.0`).
- **Active branch / PR:** `fix/safety-ci-harness` (Next-Frontier Roadmap PR 1 of 10 — Wave 0A safety, stability, CI, and developer-harness fixes).
- **Completed in PR 1:**
  - Wired `window.on_window_should_close` and guarded `Quit` / `Quit GitTurtle` menu item on `operation_busy` across open windows (`crates/app/src/main.rs`, `platform_polish.rs`).
  - Migrated macOS PDF text extraction (`crates/preview/src/native/pdf_text.rs`) off raw `objc_msgSend` transmutes to `objc2` / `objc2-foundation`.
  - Fixed 4 UI-thread `.unwrap()` + slice-index panic sites in virtualized lists (`revision_inspection.rs`, `workspace.rs`, `commit_message.rs`).
  - Eliminated double `TextDiff::Patience` 250 ms timeout divergence in `crates/git-core/src/work.rs`.
  - Redacted failed-read `Details…` reports (`reported`, `OperationFailure::of_read`) and stripped Unicode bidi override controls (`U+202A..=U+202E`, `U+2066..=U+2069`) from Git diagnostics, banners, and tooltips.
  - Fixed `scripts/ci/packages.py` (`--event schedule` without upload; cleared stale macOS `KNOWN_GAPS` to `set()`), `release.yml` missing `target:`, `tooling_suites.py` (`-B`, retired sharding), `gate.py` (`RUSTUP_TOOLCHAIN`, fast doctests, `ci-tests`/`native_qa`/`website`), `guards.py` (`.rs` conflict scan), `worktree.py`, `.claude/hooks/*`, YAML skill quoting + parser, retired task JSONs, and synced stale docs.
- **Next concrete steps:**
  1. PR 2 — Engineering harness v2 (`check_invariants.py`, CLI smoke checks, offline reviewer evals, `bench_compare.py`), $0 CI upgrades (`cache_seed_plan`, skip `finish` on exact hits, `zizmor`, SLSA provenance + SBOM), and functional/UX/a11y bug sweep.
  2. PR 3 — Runtime performance & memory quick wins (`paint_quad` straight graph lanes, in-place BGRA image swizzle, reduced `git` spawns, lazy Split/Settings, event-driven waits, `memfd_create` helper transfer) with P-core release benchmarks.
  3. PRs 4–10 — Architecture modularity & fuzz/Miri, large-repo scalability, daily-driver Git power, syntax/diff/GitHub v2/polish, macOS native craft, Omarchy/Linux craft + packaging prep, and vendor diet/website readiness.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12` and OS close/`Quit` refusal while a Git write runs.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, `website/` Cloudflare deploy, and opt-in auto-update design.
