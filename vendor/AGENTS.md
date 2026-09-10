# Vendored dependency guidance

These dependencies are selected by the root [Cargo patches](../Cargo.toml) and pinned through `Cargo.lock`. Read the affected patch note and the consuming [app](../crates/app/AGENTS.md) or [preview](../crates/preview/AGENTS.md) guide before editing or reviewing. The patch notes record upstream provenance, modified files and removal conditions; keep them current with the local change.

| Dependency | Local purpose and contract |
| --- | --- |
| `gpui-base` | [Accessibility and dialog/input semantics](gpui-base/GITTURTLE-PATCH.md) |
| `gpui-component` | [Input accessibility projection and explicit control geometry](gpui-component/GITTURTLE-PATCH.md); input changes pair with the base patch |
| `gpui-pre-macos` | [Demand-driven frames and accessibility responder alignment](gpui-pre-macos/README.gitturtle.md) |
| `mermaid-rs-renderer` | [Disable renderer-owned disk font caching](mermaid-rs-renderer/GITTURTLE-PATCH.md); bounded supplied-text integration remains in the preview crate |

Keep patches limited to their demonstrated integration need. Preserve license notices, recorded source provenance and the matching toolkit dependency set. On an upstream update, compare each local patch with the replacement and retire it only when the consuming behavior remains covered. Remove paired input patches together when upstream supplies both sides.

These packages are excluded workspace members. Workspace checks compile the used dependency code, but do not run every vendored package's own tests. Use the consuming app's rendered-node/input/control regressions for toolkit behavior, the macOS patch note's `frame_demand_probe` for dispatch lifetime changes, and preview Mermaid/SVG fixtures for renderer integration. Follow root combined Rust checks after integration; native presentation, focus and resource claims still require the affected native workflow and build identity. Do not run upstream CLI/filesystem examples to validate GitTurtle's passive preview path.

Patch-note or instruction-only changes need reference and diff checks without rebuilding the application.
