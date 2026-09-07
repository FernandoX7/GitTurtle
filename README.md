# GitTurtle

GitTurtle v0.1 is a native, read-only Git browser built with Rust and GPUI. It displays local history, branches, worktrees, code changes, and image comparisons. It uses the installed Git executable for repository reads, with no Electron runtime or AI features.

The current build has been exercised on macOS. The shared Rust implementation is intended to support Linux, but Linux builds, packaging, and native interaction have not been validated. See [validation notes](docs/validation.md) for completed checks and limits, and [the design specification](docs/design.md) for the broader intended experience.

## Available in v0.1

- Local and remote-tracking branches, linked worktrees, a commit graph, and search within loaded history.
- Commit details, changed files, root comparisons, and explicit merge-parent selection.
- Selectable, read-only unified patches with syntax coloring, plus separate **Before** and **After** source tabs.
- Before/after PNG, JPEG, WebP, GIF, and supported static SVG previews, transparency backgrounds, linked zoom, and panning.
- Local Git LFS image previews when the stored object passes size and SHA-256 verification; explicit messages for unavailable or unsupported content.
- Native folder selection, remembered repository, resizable panes, and keyboard navigation.

## Run from source

Use Rust 1.98 or newer and an installed Git executable. On macOS, install Xcode and its Metal toolchain. Use the checked-in lockfile.

```sh
cargo run --locked -p gitturtle -- /path/to/repository

# Use the optimized build when evaluating interaction performance.
cargo run --release --locked -p gitturtle -- /path/to/repository
```

Open an existing clone or linked worktree. With no path argument, the app attempts to reopen the last remembered repository. Settings are stored separately from the inspected repository at `~/Library/Application Support/GitTurtle/preferences.json` on macOS. The Linux settings path is `$XDG_CONFIG_HOME/gitturtle/preferences.json`, falling back to `~/.config/gitturtle/preferences.json`.

## Package on macOS

```sh
./scripts/package-macos.sh
open dist/GitTurtle.app

# Package an existing release executable without rebuilding.
./scripts/package-macos.sh --no-build

# Build and package the debug profile instead.
./scripts/package-macos.sh --debug

# Pass a repository directly to the bundled executable.
dist/GitTurtle.app/Contents/MacOS/gitturtle /path/to/repository
```

The script produces a bundle for the build machine's architecture, includes the application assets, and applies a local ad-hoc signature. `--debug --no-build` packages an existing debug executable. An optional final argument changes the output `.app` path. This development bundle is not notarized and is not a universal binary.

## Keyboard controls

| macOS shortcut | Action |
| --- | --- |
| Command-O | Open a repository |
| Command-R | Refresh the local snapshot |
| Command-F in the history or file list | Focus loaded-history search |
| Command-F in a text preview | Open the editor's text search |
| Escape | Clear loaded-history search and return focus to history when the app handles the action; an editor search can handle Escape first |
| Up / Down | Select the previous / next item in the focused history or file list |
| Home / End | Select the first / last item in the focused list |
| Enter | Switch focus between history and files |
| Command-B | Show or hide the sidebar |
| Command-Q | Quit |

The non-macOS key bindings use Control instead of Command; their native behavior has not been tested on Linux. Text previews support selection and copying without editing repository content.

## Read-only behavior

GitTurtle reads existing local objects, references, and worktrees. It does not stage, commit, checkout, fetch, push, repair, or run repository maintenance. Selecting a branch or worktree changes the view without changing the checkout.

**Refresh rereads local state only.** It resolves the selected branch or worktree's current tip from the new snapshot. Remote-tracking branches reflect fetches performed by another tool. Missing partial-clone objects and LFS content are not downloaded. Repository hooks, external diff commands, text conversions, and LFS helpers are not run.

The [Git service documentation](crates/git-core/README.md) describes read safeguards, local LFS compatibility, and backend measurements. Those measurements exclude the native UI and are not interaction-latency guarantees.

## Current limits

- History starts with 500 commits. **Load more** increases the loaded prefix by 500, up to 10,000. Each increase reloads that prefix; it restores the selected commit and preferred file when available and centers the selected history row. Search covers only loaded commits. Refresh is manual.
- Text comparisons use a unified patch and separate Before/After tabs. Aligned split diffs and a dual old/new line-number gutter are not implemented. Text previews are limited to 2 MiB and 100,000 lines per side. Rename detection is disabled in the UI's initial file-list path, so renames appear as an addition and a deletion.
- Images display the first frame. The app requests previews with a maximum edge of 1,600 pixels; **100% means decoded preview size**, which may be smaller than the source. Original dimensions and reduced preview dimensions are shown. Encoded input is limited to 32 MiB, with additional pixel and decoder limits. SVG filters and embedded/external images are unsupported and produce an explicit error.
- Histories exceeding the graph's lane or edge budget show isolated commit nodes with an explanation. Selecting a branch can reduce the graph size while preserving access to commits and file previews.
- Linux validation, CI coverage, distribution signing, and notarization remain future work.

## Development and measurement

```sh
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --release --locked -p gitturtle
```

For optional application timing traces:

```sh
GITTURTLE_TRACE=1 cargo run --release --locked -p gitturtle -- /path/to/repository
```

The `gitturtle.selection_frame_ms` trace runs from the app's selection handler to a GPUI frame-completion callback after preview preparation. It excludes input delivery before the handler, OS display presentation, and GPU completion. The status bar's **content read** timing measures worker work only. Neither number is a complete physical click-to-display measurement. See [validation and measurement methodology](docs/validation.md).

Repository reading, preview decoding, graph layout, and cache management run on a background worker. Lists render visible rows, and selection generations reject stale results. Project agreements are in [AGENTS.md](AGENTS.md), with the review workflow in [.agents/skills/gitturtle-performance/SKILL.md](.agents/skills/gitturtle-performance/SKILL.md).
