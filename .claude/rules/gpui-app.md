---
paths:
  - "crates/app/src/**/*.rs"
---
# GPUI app conventions

Follow [crates/app/AGENTS.md](../../crates/app/AGENTS.md), [gitturtle-gpui-testing](../../.agents/skills/gitturtle-gpui-testing/SKILL.md), and [gitturtle-ui-ux](../../.agents/skills/gitturtle-ui-ux/SKILL.md).

- Keep reads, parsing, layout prep, and decoding off the UI thread (`worker.rs`, `operations.rs`).
- Prove UI changes with `#[gpui::test]` rendered-node assertions (`ThemeChoice::Midnight.apply(None, cx)`, `painted_quads()`, `debug_bounds`, `accessibility_label`) and a mutation check.
- Use isolated per-worktree `CARGO_TARGET_DIR` (`scripts/dev/worktree.py`), never a shared mutable target.
