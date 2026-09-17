---
paths:
  - "vendor/**"
---
# Vendored dependency conventions

Read the patch note beside the dependency before editing. A vendor change identifies the missing upstream behavior, its consuming path in `crates/app` or `crates/preview`, and the smallest patch that supplies it; the patch note records provenance, modified files and the removal condition. Keep product behavior in the app or preview adapter whenever the upstream API can express it, never copy GPL Zed editor code, and validate toolkit behavior through the consuming app's rendered-node, input and control regressions rather than upstream examples.
