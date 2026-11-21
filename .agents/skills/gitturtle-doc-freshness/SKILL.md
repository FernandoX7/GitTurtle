---
name: gitturtle-doc-freshness
description: Keep GitTurtle's README, CONTRIBUTING, SUPPORT, docs, website guides, legal notices, and guidance files fresh and consistent with verified code and validation records. Use when updating docs, closing milestones, or fixing documentation drift.
---

# Documentation & website freshness

GitTurtle prevents doc drift mechanically with zero PR spam: update affected docs in the **same atomic PR** as the code change, and run batched reconciliation passes at milestone boundaries.

## Automated checks (all <2 s, no Rust build)

- **`python3 scripts/check-agent-guidance.py`**: Validates local Markdown links, anchors, skill frontmatter, role/adapter sync (`scripts/dev/sync_adapters.py --check`), merge-conflict markers, size/line caps (`AGENTS.md`, `STATE.md`, tool shims), `docs/` home paths, and tracked image metadata.
- **`python3 scripts/check-docs-sync.py`** *(and `--changed-since origin/main`)*: Checks `docs/doc-manifest.json` parity — Rust toolchain version, crate/vendor versions (`Cargo.lock`), theme/tab/history constants, workflow names, and diff-scoped advisories showing which docs/website pages a code change affects.
- **`python3 website/build_guides.py --check`** and **`python3 website/check.py`**: Verifies all static website pages, generated `website/public/legal/notices/*` parity against root `LICENSE`, `THIRD_PARTY_NOTICES.md`, and `docs/licenses/assets/*`, asset provenance hashes, and zero orphaned files under `website/public/assets/`.
- **`typos --format brief`**: Catches spelling regressions across docs and code.

## Reconciliation rules

1. **Verify primary sources first:** Never copy claims from stale handoffs. Reconcile `README.md`, `CONTRIBUTING.md`, `SUPPORT.md`, `docs/user-guide.md`, `docs/linux.md`, `docs/file-previews.md`, `docs/native-accessibility.md`, `docs/public-launch.md`, and `website/guides/` directly against live source code, `Cargo.lock`, `docs/ci.md`, and `docs/validation.md`.
2. **Keep living specs concise and timeless:** `DESIGN.md` (`<= 25 KB`) and `docs/design/*.md` hold current rules only — no date stamps, no historical "before it did X" narrative, no Rust `::tests::` function names, and no wall-of-text paragraphs. Put historical rationales in `docs/decisions/` ADRs and archive frozen milestone records under `docs/archive/`.
3. **Sync legal notices & adapters mechanically:** Regenerate `website/public/legal/notices/*` via `website/build_guides.py` and `.claude/`/`.codex/` adapters via `scripts/dev/sync_adapters.py --write` rather than hand-copying files.
4. **Verify downstream publication effects:** Completing third-party license records changes CI artifact publication behavior — always test with `python3 scripts/collect-third-party-licenses.py --require-complete` directly.
