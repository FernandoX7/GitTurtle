# GitTurtle integration patch

Vendored from the published MIT-licensed `mermaid-rs-renderer` 0.3.1 crate.
Upstream: https://github.com/1jehuang/mermaid-rs-renderer
Registry source revision: `2f993bd79a55235eb59a34d807852276ba25bea7`.

`src/text_metrics.rs` disables `cache_paths`, preventing the renderer from
reading or writing its `~/.cache/mmdr/font-cache`. The now-unused hashing import
is removed. System font discovery and process-memory metrics remain available;
Unicode source labels do not require a renderer-owned disk cache. No process
environment variables or user's files are changed.

All other renderer source remains as published. GitTurtle disables default
CLI/PNG features, uses the staged parser/layout/SVG API with bounded supplied
text, refuses actions/configuration/resources, and validates/rasterizes SVG
through its existing resource-free decoder. See `docs/file-previews.md` in the
GitTurtle repository for the deliberately finite supported subset and limits.

The upstream MIT license is retained in `LICENSE`.
