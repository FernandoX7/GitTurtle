# GitTurtle

A native, read-only Git client for fast history browsing and clear code and image comparisons. Built with Rust and GPUI, with a midnight/forest interface and an original mint turtle identity. macOS is the first validation target; the shared Rust frontend also targets Linux.

Development is in progress. The design and interaction specification is in [docs/design.md](docs/design.md). Features and platform support should be judged against the running build; Linux packaging and distribution signing are not yet provided.

## Run from source

Use a current Rust toolchain and an installed Git executable. On macOS, install Xcode and its Metal toolchain. Build with the checked-in lockfile.

```sh
cargo run --locked -p gitturtle -- /path/to/repository
# Use an optimized build when evaluating interaction performance.
cargo run --release --locked -p gitturtle -- /path/to/repository
```

Open an existing clone or linked worktree. The app can remember recent repositories in user settings: `~/Library/Application Support/GitTurtle/preferences.json` on macOS, or `$XDG_CONFIG_HOME/gitturtle/preferences.json` on Linux, falling back to `~/.config/gitturtle/preferences.json`. Settings are separate from the inspected repository.

## Package on macOS

```sh
./scripts/package-macos.sh
open dist/GitTurtle.app
# Pass a repository explicitly to the bundled executable:
dist/GitTurtle.app/Contents/MacOS/gitturtle /path/to/repository
```

The script creates a bundle for the build machine's architecture, includes app assets, and applies a local ad-hoc signature. Use `--no-build` to package an existing release executable. This is a local development bundle; it is not a notarized distribution release or a universal binary.

## Read-only behavior

GitTurtle reads existing local Git objects, references, and worktrees. It does not stage, commit, checkout, fetch, push, repair, or run repository maintenance. Selecting a branch or worktree changes the view; it does not change the checkout.

**Refresh rereads local state only.** Remote-tracking branches reflect the most recent fetch performed by another tool. Missing partial-clone objects and LFS content are not fetched automatically. The viewer must report unavailable content instead of running external filters or helpers. Code and image previews are bounded so unusually large files do not monopolize the interface.

## Development checks

```sh
cargo fmt --all -- --check
cargo test --locked --workspace
cargo check --locked -p gitturtle
```

The read service, preview pipeline, and native UI live in separate crates under `crates/`. Repository reads and decoding belong off the UI thread; lists render only their visible rows, and selection generations reject stale results. Project agreements are in [AGENTS.md](AGENTS.md), with the performance review workflow in [.agents/skills/gitturtle-performance/SKILL.md](.agents/skills/gitturtle-performance/SKILL.md).
