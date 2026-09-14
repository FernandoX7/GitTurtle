# GitTurtle integration patch

Vendored from the published MIT-licensed `mermaid-rs-renderer` 0.3.1 crate.
Upstream: https://github.com/1jehuang/mermaid-rs-renderer
Registry source revision: `2f993bd79a55235eb59a34d807852276ba25bea7`.

`src/text_metrics.rs` disables `cache_paths`, preventing the renderer from
reading or writing its `~/.cache/mmdr/font-cache`. The now-unused hashing import
is removed. System font discovery and process-memory metrics remain available;
Unicode source labels do not require a renderer-owned disk cache. No process
environment variables or user's files are changed.

The four filesystem CLI tests in `tests/cli_suite.rs` and two configuration
loader tests in `src/config.rs` use private `tempfile::TempDir` directories.
Upstream used fixed or PID-based names with `create_dir_all` or direct writes,
which could follow a preexisting temporary-path symlink and overwrite unrelated
files when those upstream tests ran. The temporary-directory guards also clean
up on normal return and panic. `Cargo.toml` adds `tempfile` only as a development
dependency; renderer runtime dependencies and APIs are unchanged. Retire these
test changes when upstream allocates private temporary directories atomically.

All other renderer source remains as published. GitTurtle disables default
CLI/PNG features, uses the staged parser/layout/SVG API with bounded supplied
text, refuses actions/configuration/resources, and validates/rasterizes SVG
through its existing resource-free decoder. See `docs/file-previews.md` in the
GitTurtle repository for the deliberately finite supported subset and limits.

The upstream MIT license is retained in `LICENSE`.
