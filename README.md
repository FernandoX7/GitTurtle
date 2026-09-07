# GitTurtle

A native, read-only Git client focused on fast commit inspection, beautiful history, and code and image changes. Built with Rust and GPUI. macOS first, with a shared Linux frontend.

Development is in progress. The initial design and stack research is in `output/pdf/gitturtle-stack-research.pdf`.

## Development

Requires Rust and a current Git installation. macOS builds require Xcode and its Metal toolchain.

```sh
cargo run -p gitturtle -- /path/to/repository
cargo test --workspace
cargo fmt --all -- --check
```

The application reads existing clones. Remote-tracking branches update when another tool fetches. GitTurtle does not stage, commit, checkout, fetch, or repair repositories.

Project instructions are in `AGENTS.md`. The Git engine, preview pipeline and UI have separate crate boundaries so expensive work can remain off the UI thread.
