# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-21
- **Latest green `main`:** `63514472` (PR #236 — free-tier CI cache/sharding/routing & CoW worktree tooling merged; prior Phase 0 PRs #234 and #235 merged)
- **Active branch:** `chore/platform-agnostic-ai-arch` (Phase 3A — platform-agnostic `.agents/` architecture, retired `scripts/agent_loop/` & `scripts/operator/`, `.githooks/`, `STATE.md`, compacted `AGENTS.md`/`vendor/AGENTS.md`/`crates/preview/AGENTS.md`)
- **Staged worktrees:**
  - `.local/wt/phase2-deps` (`deps/rust-1.100-jiff-workspace-lints`) — rebased cleanly on `main`, verified locally (Rust 1.100.0, `cargo-deny 0.21.0`, `jiff 1.0.2`, standalone crate bumps, `[workspace.lints]`, `git-core` `#![forbid(unsafe_code)]`, `hayro-font` Foxit notice retained)
  - `.local/wt/phase3a-ai-arch` (`chore/platform-agnostic-ai-arch`) — in progress

## Next concrete steps
1. Merge Phase 2 PR (`deps/rust-1.100-jiff-workspace-lints`) once `main` post-merge run `44953250030` clears macOS slots; re-warm `.local/target-base` on Rust 1.100.0 with `debug = false`.
2. Merge Phase 3A PR (`chore/platform-agnostic-ai-arch`) and Phase 3B PR (DESIGN.md split, Sept 2026 doc archive, public docs/website fixes, `scripts/check-docs-sync.py` + `docs/doc-manifest.json`).
3. Execute Phase 4 (`gpui-kit 0.6.0 -> 0.10.0`, `gpui-pre 0.3.4 -> 0.3.12`, `notify 7.0 -> 8.2`, dropping ~19 upstreamed vendor patches).

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Post-Phase-4 interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12` (focus rings, dialogs, VoiceOver/Orca).
- Phase 5 (explicitly held by owner): auto-update design session (requires amending `AGENTS.md` passive-network rule + 5 security invariants in `docs/development/security-review.md`), 4 macOS license notices, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, and `website/` Cloudflare deploy.
