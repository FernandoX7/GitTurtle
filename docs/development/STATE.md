# Active Development State

> Strictly `<= 60` lines (enforced by `scripts/check-agent-guidance.py`). Overwrite on task close; never append session logs here. Graduate durable lessons into `.agents/skills/` or `docs/decisions/`.

- **Updated:** 2026-11-22
- **Latest `main`:** `dfd73f00` (PRs #241 Wave 0A, #242 Wave 0B/0C+5, #243 Wave 3A, #244 Wave 4, and #245 Wave 3B merged green).
- **Active branch / PR:** `feat/wave1a-daily-driver-power` (Roadmap PR 6 of 10 — Clutter-free daily-driver Git power).
- **Completed in PR 6:**
  - Partial staging, unstaging, and discard in both Unified and Split diff views (`crates/diff-model/src/split_diff.rs`, `crates/git-core/src/work.rs`, `crates/app/src/{diff_view,split_diff,text_review,discard,workspace}.rs`): descriptor-pinned `O_NOFOLLOW` partial discard (`WriteCommand::DiscardPartial`), shared Before/After line selection, visible hunk Stage/Unstage + Discard controls, and multi-file discard (`DiscardMany`) plus file restore to revision (`RestoreFile`).
  - Complete branch, tag, remote, and recovery lifecycle (`crates/git-core/src/work/{branches,tags,recovery,discard}.rs`, `crates/app/src/{branch_actions,tags,recovery,workspace}.rs`): reviewed force-delete unmerged local branch (tip SHA + copy + reflog note, Enter-suppressed Alert dialog), exact-lease remote branch & tag deletion, create branch/tag at any commit, Fetch prune & Fetch all remotes (with optional prune), Soft/Mixed/Hard Reset (with dirty counts, locally known published-refs warning, Enter-suppressed Hard confirm), Checkout detached HEAD, path-scoped Stash selected files, copy commit patch (`commit_patch`), and Compare with HEAD / working tree / second Cmd/Ctrl-clicked commit.
  - Progressive-disclosure navigation & search (`crates/app/src/{lib,views,history_search,shortcuts,command_palette}.rs`): collapsible sidebar `Tags` and `Stashes` sections (hidden when empty, collapsed by default, auto-expanded on filter), `Cmd/Ctrl+Shift+H` Jump to HEAD, `Cmd/Ctrl+G` Jump to Revision, collapsible History Search filters (`-S`/`-G` diff pickaxe, author, since/until, path, regex, first-parent, no-merges) with active indicator dot, and compact native `PopupMenu::submenu` context menus across commits, working files, changed files, and sidebar refs.
  - External editor `file:line` & configurable terminal (`crates/prefs/src/preferences.rs`, `crates/app/src/{platform_polish,settings,conflicts}.rs`): Settings → Projects `External terminal` field, `file:line` flags for major GUI/TUI editors (wrapping TUI editors directly via terminal argv without `sh -c`), `ENOTDIR` parent-dir fix, and full ancestor-component symlink refusal before opening or revealing repository files.
- **Not exercised locally:** macOS compilation of `crates/preview`/`crates/app` and live macOS desktop runtime (`aarch64-apple-darwin` verified locally via cross-clippy on 5 headless crates with and without `test-support`; hosted macOS CI verifies full macOS build/tests on PR push).
- **Next concrete steps:**
  1. PR 7 — Power ergonomics, syntax/diff/GitHub v2, guided bisect/submodules/custom commands, and visual polish (Waves 1B + 1C + 1D).
  2. PRs 8–10 — First-class macOS native craft (unsigned-ready), Omarchy & general Linux native craft + packaging prep, and vendored patch diet / visual QA / pre-release website readiness.

## Deferred manual desktop checks (owner-driven)
- `QA-146` interactive macOS/Linux visual sanity on passive/write readiness waits (#234/#235).
- Interactive Omarchy & macOS desktop sanity on `gpui-kit 0.10.0` / `gpui-pre 0.3.12`, OS close/`Quit` refusal during writes, VoiceOver/Orca Slider `SetValue`, Wave 3A/3B/4 checks, and Wave 1A split-diff partial staging/discard, `More` submenus, Reset/Force-delete Alert dialogs, collapsible Tags/Stashes & History filter bar, and `file:line` external editor + terminal handoff.
- Held by owner (out of scope): macOS signing/notarization & `HOSTED_UPLOAD_PLATFORMS` flip, `gitturtle-release` setup, `v0.1.0` tag/release, AUR push, `website/` Cloudflare deploy, and opt-in auto-update design.
