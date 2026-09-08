# GitTurtle

GitTurtle v0.1 is a native, read-only Git browser built with Rust and GPUI. It displays local history, branches, worktrees, code changes, and image comparisons. It uses the installed Git executable for repository reads, with no Electron runtime or AI features.

Native builds have been exercised on macOS; the [validation notes](docs/validation.md) identify the checked builds and limits. The shared Rust implementation is intended to support Linux, but Linux builds, packaging, and native interaction have not been validated. See [the design specification](docs/design.md) for the broader intended experience.

## Available in v0.1

- Local and remote-tracking branches grouped into expandable folders, linked worktrees, and search within loaded history.
- Full-height history with separate reference, graph, and commit-summary columns, alongside persistent commit details and changed files.
- Full-height file comparison, root comparisons, and explicit merge-parent selection.
- Selectable, read-only unified patches with syntax coloring and old/new line numbers, plus separate **Before** and **After** source tabs.
- Before/after PNG, JPEG, WebP, GIF, and supported static SVG previews, transparency backgrounds, linked zoom, and drag-to-pan.
- Local Git LFS image previews when the stored object passes size and SHA-256 verification; explicit messages for unavailable or unsupported content.
- Native folder selection, remembered repository, resizable panes, and keyboard navigation.

## Browse history, then open a comparison

History fills the center of the window. Repository navigation sits on the left; the right inspector holds the selected commit's details, parent choice, and changed-file list. Local and remote branch names form folders from their slash-separated prefixes, with remote names such as `origin` at the top of the remote tree. Filtering reveals matching branches without changing saved folder expansion.

Selecting a commit updates its details and file list while staying in History. Initially highlighting the preferred or first file does not prepare its preview. Click a changed file, or press Enter to open the highlighted file from History or the file list, to enter Compare: the file fills the center height, navigation collapses to a compact rail, and the same inspector and file list remain on the right. Select another file there to continue comparing.

**Back to history** returns to the retained graph, selected commit, scope, search, loaded history, and scroll position. Returning does not reopen the repository or reload its history. File content is loaded on explicit activation; revisiting content can reuse the bounded preview cache.

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
| Command-[ | Back to History without clearing the query |
| Command-R | Refresh the local snapshot |
| Command-F in the history or file list | Focus loaded-history search |
| Command-F in a text preview | Open the editor's text search |
| Escape | Return from Compare to History when the app handles the action; in History, clear loaded-history search. An editor search can handle Escape first |
| Up / Down | Select the previous / next commit in history, or open the previous / next file in the focused file list |
| Home / End | Select the first / last commit in history, or open the first / last file in the focused file list |
| Enter | Open the highlighted file in Compare, from History or the file list |
| Command-B | Show or hide expanded repository navigation in History |
| Command-Q | Quit |

The non-macOS key bindings use Control instead of Command; their native behavior has not been tested on Linux. Text previews support selection and copying without editing repository content.

## Read-only behavior

GitTurtle reads existing local objects, references, and worktrees. It does not stage, commit, checkout, fetch, push, repair, or run repository maintenance. Selecting a branch or worktree changes the view without changing the checkout.

**Refresh rereads local state only.** It resolves the selected branch or worktree's current tip from the new snapshot. Remote-tracking branches reflect fetches performed by another tool. Missing partial-clone objects and LFS content are not downloaded. Repository hooks, external diff commands, text conversions, and LFS helpers are not run.

The [Git service documentation](crates/git-core/README.md) describes read safeguards, local LFS compatibility, and backend measurements. Those measurements exclude the native UI and are not interaction-latency guarantees.

## Current limits

- History starts with 500 commits. **Load more** increases the loaded prefix by 500, up to 10,000. Each increase reloads that prefix; it restores the selected commit and preferred file when available and centers the selected history row. Back to history retains the existing prefix instead. Search covers only loaded commits. Refresh is manual.
- Text comparisons use a unified patch and separate Before/After tabs. Aligned split diffs are not implemented. Text previews are limited to 2 MiB and 100,000 lines per side. Rename detection is disabled in the UI's initial file-list path, so renames appear as an addition and a deletion.
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

Tracing separates `gitturtle.commit_files_frame_ms`, from commit selection to a GPUI frame callback after its changed-file list arrives, and `gitturtle.file_preview_frame_ms`, from file activation to a frame callback after the visible preview is prepared. Generation and mode checks suppress superseded callbacks. These traces exclude input delivery before the handler, OS display presentation, and GPU completion. The status bar's **content read** timing measures worker work only. None is a complete physical click-to-display measurement. Existing native measurements in [the validation record](docs/validation.md) describe their recorded build and must not be treated as measurements of a later workspace revision.

Repository reading, preview decoding, graph layout, and cache management run on a background worker. The worker retains the current worktree's Git session across local refreshes and scope changes, preserving its persistent object reader while rereading mutable refs and history. Lists render visible rows, and selection generations reject stale results. Project agreements are in [AGENTS.md](AGENTS.md), with the review workflow in [.agents/skills/gitturtle-performance/SKILL.md](.agents/skills/gitturtle-performance/SKILL.md).
