---
name: gitturtle-gpui-testing
description: "Write, run, and stabilize #[gpui::test] view tests for GitTurtle's crates/app (TestAppContext, add_window_view, simulate_* input, run_until_parked, painted_quads, debug_bounds, accessibility_label, deterministic seeds, and iteration sweeps). Use for any UI behavior, layout, theme, focus, or accessibility change."
---

# GPUI view tests

A UI change is proven by a rendered-node test, not prose. Write `#[gpui::test] fn name(cx: &mut gpui::TestAppContext)`, initialize app-building tests with `views::tests::init_app(cx)` (runs `gpui::init` and isolates settings per seed; bare `cx.update(gpui::init)` is only for views that never build the app), mount with `cx.add_window_view(|window, cx| ...)`, drive with `simulate_keystrokes`, `simulate_click`, `simulate_mouse_move`, and `simulate_resize`, settle with `run_until_parked`, and assert `debug_bounds`, focus, `painted_quads()`, and `accessibility_label`.

## Scoping, seeds, and mutation proofs

- Run the narrowest filter first (`cargo test --locked -p gitturtle <module>::<test>`), then the consuming regression modules: viewport/layout changes need `scroll_tests`, `split_diff`, and `repository_tabs`; history notices need `history_updates`, `automatic_refresh`, and `local_refresh`. Note that Linux portal/process tests in `desktop_text` do not run on macOS.
- Reproduce failures with `SEED=<seed> cargo test --locked -p gitturtle <test> -- --nocapture`, and sweep for flakiness with `ITERATIONS=100 cargo test --locked -p gitturtle <test>`. Never commit `retries` on `#[gpui::test]` — a test needing retries hides a real scheduling or generation race.
- `scripts/gate.py fast` runs `ITERATIONS=20` on changed `#[gpui::test]` functions, but silently skips when >40 tests change; run `ITERATIONS=20` manually in that case and verify passed count > 0 (an empty filter exits 0).
- Prove every new test with a mutation check (remove the fix, watch the test fail both alone and in its full module filter, then restore). Always check precondition reachability so setup does not already satisfy the assertion vacuously.
- Keep isolated per-worktree `CARGO_TARGET_DIR` (seeded via `scripts/dev/worktree.py`): if a field or test appears ignored, never trust a shared `target/` directory.

## Colors, layout, motion, and accessibility idioms

- **Theme colors:** Always call `ThemeChoice::Midnight.apply(None, cx)` (or target palette) before asserting painted colors; otherwise test windows keep light defaults and code-set toolkit colors remain unset. Read painted fills, borders, and focus rings via `window.painted_quads()` after `simulate_mouse_move` and draw. Compare resolved native component backgrounds against their foregrounds, not raw palette contrast alone.
- **Scale factor order:** Always set `window.scale_factor` *after* resizing and assert `window.scale_factor()` — resizing after setting scale can silently reset scale to `2.0`.
- **Multi-size/multi-scale sweeps:** Single-size tests miss unsnapped lengths, ceiled text heights, and UI-scale vs device-scale cache bugs. Sweep across sizes and scales for layout decisions.
- **No last-frame layout feedback loops:** Derive toolbar/chrome layout strictly from current-frame inputs; reading whether the previous frame scrolled causes infinite redraw hangs.
- **Dialogs and motion:** Enable `set_reduce_motion(true)` (or wait until bounds match across two consecutive frames) before reading dialog geometry right after open, or tests sample the entrance animation mid-flight.
- **Screen-reader paths:** GPUI skips synthetic accessibility builders when no assistive technology is active. Factor synthetic a11y builders into directly callable/testable snapshots with the active flag enabled, or dead/broken branches pass default tests.
- **AccessKit `TextRun` limit:** Word offsets in AccessKit use `u8` (max 255 characters); runs >255 graphemes alias word lookups mod 256. Always split runs at ≤255 graphemes and link with `previous_on_line`/`next_on_line`.
- **Key-up and held-Enter tests:** Repeated `simulate_keystrokes("enter")` sends no key-up, looking like held repeats under the once-per-press/X11-repeat rule. Whenever focus moves onto an action button (Commit, Stage all, etc.), add a held-Enter / X11-repeat / double-click write-route test.

## Recurring races and cross-platform traps

1. **macOS-only layout failures are races, not font differences:** `#[gpui::test]` uses fixed-advance `NoopTextSystem` on every platform. Look for a late quiet read landing between passes and guard state-setting tests with `views::tests::hold_quiet_reads_for_test` (which `width_walk::open` does automatically).
2. **macOS-only path-keyed failures come from symlinked `TMPDIR`:** macOS maps `/var` -> `/private/var`. Reproduce on Linux by running under a symlinked `TMPDIR`, and compare canonical paths from the app rather than raw tempdir paths.
3. **Background preference saves and path filters:** Opening a repository triggers async preference writes and working-row path filters that `settle` does not wait for. Call `views::tests::land_recent_saves_for_test` or `land_every_reply` before setting recent lists or counting draw frames.
4. **`thread_local!` caches surviving seeds:** Per-thread measurement caches persist across `ITERATIONS` seeds on the same thread. Clear them at test start (e.g., `views::forget_status_text`) rather than masking with redundant changes.
5. **Real child processes & Git isolation:** Give test repositories explicit identities via `fixture_repository`/`fixture_identity`. When waiting for real child processes (e.g., `fc-match`), hold the test clock during spawn (`desktop_text`'s `until_clock_held`) so host load does not burn the product deadline. Raise the 30 s drain timeout on slow hosts only via `GITTURTLE_TEST_WAIT_SECS`. Every spawned read must settle or cancel before test exit.
6. **Release micro-benches:** Build opt-in `#[ignore]` release `#[gpui::test]` benchmarks with `cargo test --locked --release -p gitturtle --bin gitturtle --no-run` (the app crate has no library target); follow [gitturtle-performance](../gitturtle-performance/SKILL.md).
