---
paths:
  - "crates/app/src/**/*.rs"
---
# GPUI app conventions

The crate guide beside this code owns the module routing and contracts; this rule adds the harness and gate idioms Claude needs when it works here.

- `GitTurtle` owns interaction state; `src/worker.rs` handles replaceable reads and `src/operations.rs` serializes accepted typed writes. Keep request generations, repository identity checks and page/mode guards together, and never move a read, parse, layout preparation or decode onto the UI thread.
- A UI change is done when a `#[gpui::test]` builds the view with `cx.add_window_view`, drives it with `simulate_keystrokes`, `simulate_click` or `simulate_resize`, settles with `run_until_parked`, and asserts `debug_bounds`, focus and `accessibility_label` text. Render-node assertions are the evidence for layout; screenshots are not available from tests.
- Run the narrowest filter first: `cargo test --locked -p gitturtle <module>::`. Viewport changes need the consuming regressions in `scroll_tests`, `split_diff` and `repository_tabs`; history notices use `history_updates`, `automatic_refresh` and `local_refresh`. Linux portal and process tests in `desktop_text` do not run on macOS.
- Theme changes are checked as resolved native component backgrounds against their foregrounds, not palette contrast alone.
- Speed is an acceptance criterion: the first useful frame loads metadata before content, lists stay virtualized, and a hot-path change is measured in release mode with the performance skill before it is called neutral or faster. Visible changes follow `DESIGN.md` and need native evidence from the validation matrix.
- A test that checks colours applies a palette first (`ThemeChoice::Midnight.apply(None, cx)`); a test window otherwise keeps the toolkit's light defaults and the app's code-set toolkit colours stay unset. Read painted fills, borders and rings with `window.painted_quads()` after `simulate_mouse_move` and a draw (`views::tests::narrow_review_arrows_lift_on_hover`, `settings::picker_tests::a_focused_switch_draws_the_button_ring_whole_and_moves_nothing`).
- Prove each new test with a quick mutation: remove its fix, see it fail, restore. When a change seems ignored (a new field unseen, new tests missing from `--list`), check that nothing else builds into the same target (see `CLAUDE.md` on worktree implementers) before suspecting cargo; rebuilding in a separate `CARGO_TARGET_DIR` settles it.
- A UI-thread hot path without a frame trace is measured by an opt-in `#[ignore]` release `#[gpui::test]` that times the handler and writes JSON to an environment-named file, built with `cargo test --locked --release -p gitturtle --bin gitturtle --no-run` (the app crate has no library target); the `gitturtle-performance` skill has the procedure.
