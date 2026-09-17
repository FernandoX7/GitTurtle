---
name: gitturtle-gpui-testing
description: Write, run and stabilize #[gpui::test] view tests for GitTurtle's crates/app using the vendored GPUI harness (TestAppContext, add_window_view, simulate_* input, run_until_parked, debug_bounds, accessibility_label), including deterministic seeds and iteration sweeps for flaky tests. Use for any UI behavior, layout, focus or accessibility change.
---

# GPUI view tests

A UI change is proven by a rendered-node test, not a screenshot. Write `#[gpui::test] fn name(cx: &mut gpui::TestAppContext)`, call `cx.update(gpui::init)`, build the view with `cx.add_window_view(|window, cx| ...)`, drive it with `simulate_keystrokes`, `simulate_click` and `simulate_resize`, settle with `run_until_parked`, and assert `debug_bounds`, focus and `accessibility_label` text. Follow the neighboring test module's setup helpers rather than inventing new fixtures; `split_diff.rs`, `scroll_tests.rs` and `repository_tabs.rs` show measured-viewport assertions.

Run the narrowest filter first (`cargo test --locked -p gitturtle <module>::<test>`), then the module, then the fast gate. The harness is deterministic under a seed: reproduce a failure with `SEED=<seed> cargo test --locked -p gitturtle <test> -- --nocapture` using the seed the failure printed, and sweep for flakiness with `ITERATIONS=100 cargo test --locked -p gitturtle <test>`; the vendored runtime (`gpui-pre` 0.3.4, `src/test.rs`) reads both variables, and the macro also accepts `iterations`, `seeds`, `retries` and `on_failure` attribute arguments for a permanent per-test setting. The fast gate reruns changed `#[gpui::test]` functions with `ITERATIONS=20`. Never commit `retries`, which masks failures: a test that only passes with retries has a real scheduling or generation bug.

Time and background work are controlled: tests must not depend on wall-clock timing, and every spawned read must be settled or cancelled by the test's end so the leak timeout in `.config/nextest.toml` does not report it. Theme assertions compare resolved native component backgrounds with foregrounds. Visual correctness of the real window still needs the native-QA evidence rows; say so instead of claiming it from a test.
