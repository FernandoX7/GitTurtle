# GitTurtle integration patch

Vendored from the published MIT-licensed `mermaid-rs-renderer` 0.3.1 crate.
Upstream: https://github.com/1jehuang/mermaid-rs-renderer
Registry source revision: `2f993bd79a55235eb59a34d807852276ba25bea7`.
Registry archive SHA-256: `6cb5b469222ce6b9896edf0a969cf68464f5a028ef394fd6643f00bd943df117`.

`src/text_metrics.rs` disables `cache_paths`, preventing the renderer from
reading or writing its `~/.cache/mmdr/font-cache`. The now-unused hashing import
is removed. System font discovery and process-memory metrics remain available;
Unicode source labels do not require a renderer-owned disk cache. No process
environment variables or user's files are changed.

`src/parser.rs` tightens `parse_click_line` (`is_click_node_id`) so flowchart
edges starting from a node named `Link` or `Click` (such as `Link --> Regular`
or `Click -.-> Other`) are not misparsed as `click`/`link` directives with an
edge operator as the node ID. Real `click <node> <url>` and `link <node> <url>`
directives continue to parse into `graph.node_links` so GitTurtle's preview
guard still refuses them. Covered by `parser::tests::parses_nodes_named_link_and_click` and
`crates/preview/src/mermaid.rs` (`cargo test --locked -p gitturtle-preview mermaid`;
upstream crates.io tarball omits `docs/comparison_sources/` required by its own
layout unit tests).

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

## Removal

Upstream target: `https://github.com/1jehuang/mermaid-rs-renderer` (PR-ready candidates: an optional `disk-cache` Cargo feature gating `~/.cache/mmdr/font-cache` in `src/text_metrics.rs`, the `Link`/`Click` node-ID edge disambiguation in `src/parser.rs`, and atomic `tempfile::TempDir` isolation in `src/config.rs` and `tests/cli_suite.rs`). Drop this entire vendored directory, its `[patch.crates-io]` entry, workspace exclusion, `THIRD_PARTY_NOTICES.md` row, and `vendor/AGENTS.md` row once a published release covers all three fixes and `cargo test --locked -p gitturtle-preview mermaid` passes against crates.io directly.
