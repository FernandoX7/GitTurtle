# GitTurtle macOS accessibility responder patch

This is the Apache-2.0 `gpui-pre-macos` 0.3.12 package, kept at the matching GPUI Kit 0.10.0 dependency version. The original license is in [LICENSE-APACHE](LICENSE-APACHE). No editor code or framework migration is included.

Origin: [crates.io gpui-pre-macos 0.3.12](https://crates.io/crates/gpui-pre-macos/0.3.12), upstream Zed snapshot `d2a4fb6ded7c470be25d1541281da3882495b65b`, package checksum `2de7d3f4ad51ac387e822495db363fcc1188d349793365731d054444c4cb06ee`. `Cargo.toml` is the published normalized manifest; dependencies keep their original requirements and the workspace lock pins their versions. Cargo caches, the package's independent lock, and release artifacts are excluded. Apart from this note, `src/window.rs` is the only file that differs from the published archive.

## Upstreamed in 0.3.12 (retired local patch)

Upstream `gpui-pre-macos` 0.3.12 migrated from deprecated `cocoa`/`objc`/`block` bindings to `objc2` and implemented `MacWindow::frame_waker` via `CAMetalDisplayLink` (`metal_frames.wake()`) on macOS 14+. The previous local frame-demand wake patch and standalone `frame_demand_probe` example are therefore retired.

## Accessibility responder alignment

`MacWindow` creates its rendering `native_view` as a child of `NSWindow.contentView` and makes that child the first responder. Upstream `a11y_init` in `0.3.12` still uses AccessKit's `SubclassingAdapter::for_window`, which subclasses the parent content view. As a result, the accessibility tree and `accessibilityFocusedUIElement` override attach to a different view than the keyboard responder.

The focused patch in `src/window.rs::a11y_init` uses the pinned `accesskit_macos` `SubclassingAdapter::new` API with the live rendering view pointer (`lock.native_view.as_ptr() as *mut c_void`). The adapter retains that view. It also seeds the adapter's host-focus state using `lock.native_window().isKeyWindow()`, because initial activation may precede callback installation. Subsequent activation continues through the original window-key handler. Any returned event batch is raised after releasing the window-state lock. No focus action, settings mutation, dependency update, Git operation, or Linux behavior is added.

Compilation and automated tests run on hosted macOS CI; native VoiceOver and descendant-focus behavior must be recorded against the integrated macOS binary when re-verifying native QA.

## Removal

Upstream target: `https://github.com/zed-industries/zed` (`crates/gpui_macos`, PR-ready single-file fix in `src/window.rs`). Remove this vendored directory, its `[patch.crates-io]` entry, workspace exclusion, `THIRD_PARTY_NOTICES.md` row, and `vendor/AGENTS.md` row once upstream `gpui-pre-macos` attaches `SubclassingAdapter` directly to the first-responder rendering `native_view` and seeds initial key-window state.
