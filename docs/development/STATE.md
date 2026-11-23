# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-22
- **Latest `main`:** `387b293d` (PRs #241 Wave 0A, #242 Wave 0B/0C+5, #243 Wave 3A, #244 Wave 4, #245 Wave 3B, and #246 Wave 1A merged green).
- **Active branch / PR:** `feat/wave1b-power-ergonomics` (Roadmap PR 7 of 10 — Power ergonomics, syntax/diff/GitHub v2, guided bisect/submodules/custom commands, and visual polish).
- **Completed in PR 7:**
  - **Syntax, Diff & Previews (`crates/preview`, `crates/app`):** 14 additional tree-sitter grammars (26+ languages total, complete license notices); diff word wrap with wrapped display-row gutter & partial-staging alignment, tab width 2/4/8, Patience/Myers/Histogram algorithm selection; dark/light Mermaid sync; bounded CSV/TSV Table preview (focusable/scrollable) and streaming JSON/TOML/YAML Formatted preview; aligned Markdown Before/After headers; bounded multi-page PDF text search (macOS `objc2` `PDFDocument` + Linux confined `GTPS`/`GTPR` helper); syntax-highlighted 3-way conflict view; offline local-keyring GPG/SSH commit signature verification badge; Export/Apply `.patch` workflows.
  - **Commit, Merge, Rebase, Stash & Clone Ergonomics (`crates/git-core`, `crates/app`):** `Cmd/Ctrl+Enter` staged commit shortcut, compact unfocused description field (`QA-144`), staged heading unstage reveal (`QA-145`), signing badge, `Commit & Push`, `Insert Signed-off-by`, worktree-confined `commit.template`, recent commit message picker, explicit Pull strategy dialog (`FastForwardOnly`/`Merge`/`Rebase`/`RebaseAutostash`), Merge options dialog (`DefaultFf`/`NoFf`/`FfOnly`/`Squash` + `SQUASH_MSG` prefill), interactive rebase `Edit` pause, Stash `--keep-index`/`--staged`/optional name + row Pop/Create-branch actions, and collapsible Clone options (`depth`, `branch`, `recurse_submodules`).
  - **Progressive Power & GitHub v2 (`crates/prefs`, `crates/github`, `crates/theme`, `crates/app`):** Contextual active-only Git bisect banner and start dialog; empty-hidden collapsed Submodules sidebar section with URL-disclosed Init/Update confirmation; cross-platform custom code font selector (cached monospace list + installed-font safety probe); direct no-shell custom commands (validated program + single-pass placeholder expansion + bounded timeout/cancellation); drag-to-reorder repository tabs; GitHub PR filters, off-UI-thread rendered PR Markdown, structured check runs with hardened `https://` links, SHA-pinned PR branch checkout, Merge/Draft/Close-Reopen/Labels/Reviewers actions; OKLCH harmonized commit-graph lanes, Omarchy monochrome `4.5:1` accent floor, floating unmarked-copy toast, and dynamic Blame gutter widths.
- **Not exercised locally:** macOS compilation of `crates/preview`/`crates/app` and live macOS desktop runtime (`aarch64-apple-darwin` verified locally via cross-clippy on 5 headless crates with and without `test-support`; hosted macOS CI verifies full macOS build/tests on PR push).
- **Next concrete steps:**
  1. PR 8 — First-class macOS native craft (unsigned-ready, Wave 2A).
  2. PR 9 — First-class Omarchy & general Linux native craft + packaging prep (Waves 2B + 2C).
  3. PR 10 — Vendored patch diet, automated visual QA, and pre-release website readiness (Wave 4C + Wave 6 prep).

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12`, OS close/`Quit` refusal during writes, VoiceOver/Orca Slider `SetValue`, Wave 3A/3B/4/1A checks, and PR 7 workflows (soft-wrap gutter/hunk controls, dark Mermaid, CSV/JSON/PDF search, 3-way conflict view, bisect banner, submodules, custom commands, drag-reorder tabs, GitHub PR v2 actions, OKLCH graph lanes, copy toast).
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, `website/` Cloudflare deploy, and opt-in auto-update design.
