# GitTurtle local patch to `accesskit_macos 0.26.3`

- **Upstream crate:** `accesskit_macos` `0.26.3` (AccessKit monorepo commit `c88605b96d04431f9c3c792464a0f2f253480e94`, `MIT OR Apache-2.0`, `LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE.chromium` preserved verbatim).
- **Modified files:**
  - `src/util.rs` — adds `clips`, `clipped_bounds`, and `clipped_at` helpers plus `clip_tests` unit coverage.
  - `src/adapter.rs` — filters `Adapter::hit_test` through `clipped_at` so VoiceOver pointer/trackpad exploration and Accessibility Inspector skip scrolled-out or clipped halves of descendants behind a clipping ancestor.
  - `src/node.rs` — intersects `PlatformNode::frame` (`accessibilityFrame`) with every clipping ancestor via `clipped_bounds`, keeping partially visible VoiceOver focus frames inside the scroll viewport and reporting `NSRect::ZERO` for wholly clipped-out nodes kept past a fold.
- **Why local:** Matches GitTurtle's Linux AT-SPI clipping parity in `vendor/accesskit_atspi_common/GITTURTLE-PATCH.md` (`clips_children()` in `crates/app/src/native_accessibility.rs`). Upstream `accesskit_macos 0.26.3` delegates `hit_test` directly to `accesskit_consumer::Node::node_at_point` (which ignores `clips_children`) and returns unclipped `node.bounding_box()` from `accessibilityFrame`.
- **Removal condition:** Drop this vendored patch once GPUI macOS upgrades to an `accesskit_macos` release that clips `accessibilityFrame` and `hit_test` against `clips_children()` ancestors.
