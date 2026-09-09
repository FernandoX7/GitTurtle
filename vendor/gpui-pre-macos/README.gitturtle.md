# GitTurtle macOS frame-demand patch

This is the Apache-2.0 `gpui-pre-macos` 0.3.4 package, kept at the matching GPUI Kit dependency version. The original license is in [LICENSE-APACHE](LICENSE-APACHE). No editor code or framework migration is included.

Origin: [crates.io gpui-pre-macos 0.3.4](https://crates.io/crates/gpui-pre-macos/0.3.4), upstream Zed snapshot `69164008341295ad481bb11c0334a712ca8c23e3`, package checksum `15263eafcbe4e81aec7eb24a47d4c6c88f5e29ba2ba8ad86c430ab8e472883dc`. `Cargo.toml` is the published normalized manifest; dependencies keep their original requirements and the workspace lock pins their versions. Cargo caches, the package's independent lock, and release artifacts are excluded.

## Local change

The macOS backend relies on CVDisplayLink callbacks for presentation and does not implement GPUI's frame-demand waker. Native testing found that input could update the rendered/accessibility state while the visible frame and next-frame focus waited for a resize. `refresh()` alone only marks the window dirty. This patch adds an explicit, bounded wake path while preserving the existing display-link path:

- `WindowFrameSource::request_frame` merges one event into the existing per-window dispatch source, independently of display registration.
- `MacWindow::frame_waker` coalesces demand into one one-shot 16,667 µs timer. Animations can demand further frames, but cannot spin an unpaced main-queue loop. An idle window schedules no timer.
- The timer holds a weak window reference. It checks close state and the existing source, never recreates a source after teardown, and uses the source's existing cancel-before-view-release lifecycle.

The frame fix is 55 added source lines across `src/window.rs` and `src/display_link.rs`. Two lines at the crate root allow the existing deprecated Cocoa API usage; this avoids warning floods from the local dependency without suppressing other warning categories or changing APIs. A bounded standalone dispatch probe and its explicit example target are also included. Source data/configuration operations and Linux behavior are unchanged. Separate compilation with no default features passed; native visual, focus, close/reopen, and idle-resource verification must be recorded against the integrated executable in the milestone evidence. The wake mechanism does not by itself establish display latency or CPU measurements.

## Dispatch lifecycle probe

Run from the GitTurtle workspace after enabling the local dependency patch:

```sh
cargo test --locked -p gpui-pre-macos --example frame_demand_probe
```

The probe opens no window and never registers a display link. It pumps the main run loop to verify that 1,000 demanded frames coalesce to one callback, an idle source does not produce further callbacks, and dropping a source cancels its queued callback. Both the separate-copy run and the command above against the GitTurtle workspace lock passed all three assertions. This isolates dispatch/lifecycle behavior; it does not establish visible rendering or native focus correctness.
