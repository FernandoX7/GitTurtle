# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-21
- **Roadmap status (Phases 0–4):** Complete and verified
  - **Phase 0:** PR #234 (`28024eeb`, passive `waitid(WEXITED | WNOWAIT)`) & PR #235 (`383a9b2b`, framed `ProgressScanner`, event-driven `bounded_write_output`, `AskpassServer` `poll()`, retired `claude/ci-records-2026-11-21b` & `refs/keep/*`).
  - **Phase 1:** PR #236 (`63514472`, `debug = false` on dev deps, `vendor/` pure-doc cache exclusion, 3-way `macos-15` sharding, parallel `ci-tests`, smart `push: main` routing + weekly cache seed, `scripts/dev/{worktree.py,build_slot.py}`, disk cleanup 80 GB → 46 GB).
  - **Phase 2:** PR #237 (`28d6a579`, Rust `1.100.0`, `cargo-deny 0.21.0`, `dependabot.yml` `cooldown: { default-days: 0 }`, `chrono` → `jiff 1.0.2`, standalone crate bumps, `[workspace.lints]`, `git-core` `#![forbid(unsafe_code)]`).
  - **Phase 3A:** PR #238 (`b867c74d`, platform-agnostic `.agents/` architecture, retired `scripts/agent_loop/` & `scripts/operator/`, `.githooks/*`, `STATE.md`, compacted guidance files, pre-registered 5 auto-update invariants).
  - **Phase 3B:** PR #239 (`70f5bc6e`, split `DESIGN.md` & `docs/validation.md`, archived Sept 2026 docs, website legal-notice generator, `docs/doc-manifest.json` + `scripts/check-docs-sync.py`).
  - **Phase 4:** `build/upgrade-gpui-kit-0.10` (`gpui-kit 0.10.0` / `gpui-pre 0.3.12`, macOS `objc2` migration, rebased vendored patches with upstreamed patches dropped, `--require-complete` third-party notices passing on all 4 Linux & macOS targets).

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Post-Phase-4 interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12` (focus rings, dialogs, menus, theme live preview, VoiceOver/Orca).
- Phase 5 (explicitly held by owner): auto-update design session (requires amending `AGENTS.md` passive-network rule + 5 security invariants in `docs/development/security-review.md`), macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` decision, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, and `website/` Cloudflare deploy.
