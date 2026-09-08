# GitTurtle

GitTurtle is a native Git client built with Rust and GPUI for everyday Git work, history, code changes, and image comparisons. It uses your installed Git executable, with no Electron runtime or AI features. Browsing stays local; repository writes and network operations start from explicit actions.

Native builds have been exercised on macOS; the [validation notes](docs/validation.md) identify the checked builds and limits. The shared Rust implementation is intended to support Linux, but Linux builds, packaging, and native interaction have not been validated. See [the design specification](docs/design.md) for the broader intended experience.

## Current source features

- A project hub with searchable recent repositories, native folder selection, clone, and create.
- Working changes with staged/unstaged previews, whole-file staging and unstaging, and commits.
- Explicit branch creation/switching, fetch, fast-forward pull, and push to a chosen remote branch.
- Local and remote-tracking branches grouped into expandable folders, linked worktrees, and search within loaded history.
- Full-height history with separate reference, graph, and commit-summary columns, alongside persistent commit details and changed files.
- Full-height file comparison, root comparisons, and explicit merge-parent selection.
- Selectable, read-only unified patches with syntax coloring and old/new line numbers, plus separate **Before** and **After** source tabs.
- Before/after PNG, JPEG, WebP, GIF, and supported static SVG previews, transparency backgrounds, linked zoom, and drag-to-pan.
- Local Git LFS image previews when the stored object passes size and SHA-256 verification; explicit messages for unavailable or unsupported content.
- Midnight, Graphite, and Daylight themes; comfortable/compact density; configurable history columns; recent-project and startup preferences.
- Repository Git identity settings, resizable panes, and keyboard navigation.

## Open a project and work with Git

Use **Projects** to reopen a recent repository, choose an existing folder, clone a URL or local repository, or create a new repository. Clone and Create take a parent folder and project-folder name; the destination must be new or empty. Create also lets you choose the initial branch.

**Working Changes** separates staged and unstaged files. Select either side to inspect it, stage or unstage individual files or the whole list, and enter a commit message to commit the staged changes. Previews remain read-only. A file can appear in both lists when it has staged and further unstaged edits.

**Git actions** exposes branch switching and creation, plus the selected remote and target branch for network operations. Creating a branch also switches to it. Pull accepts fast-forward updates only; Push uses the current local branch and an explicit destination branch, without force. Branch selection in the history navigator still changes only the history scope.

**Settings** controls the theme, density, startup behavior, default branch for new projects, and history column visibility. Drag header dividers to resize columns; narrow windows scroll horizontally without hiding your choices. The commit-message column stays visible. Git name/email edits are saved only after **Save repository identity** for the displayed repository.

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

Open an existing clone or linked worktree. With no path argument, the app reopens the last remembered repository when that setting is enabled; otherwise it opens Projects. App settings are stored separately from repositories at `~/Library/Application Support/GitTurtle/preferences.json` on macOS. The Linux settings path is `$XDG_CONFIG_HOME/gitturtle/preferences.json`, falling back to `~/.config/gitturtle/preferences.json`.

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
| Command-Shift-O | Open Projects |
| Command-2 | Open Working Changes |
| Command-, | Open Settings |
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

## Repository operations

History, status, and previews read local objects, references, and files without changing the index or checkout. Passive reads disable external diff/text-conversion helpers and filters; working-file previews show raw content and do not follow stored symlinks.

**Refresh rereads local state only.** It resolves the selected branch or worktree's current tip from the new snapshot. Remote-tracking branches change after an explicit fetch in GitTurtle or another tool. Missing partial-clone objects and preview LFS content are not downloaded automatically.

Staging, committing, switching branches, cloning, and network actions use a separate background executor. Real Git writes preserve configured hooks, identity, filters, signing, and credential helpers. Configure authentication through Git or an SSH agent beforehand; GitTurtle has no interactive credential dialog. Automatic maintenance and recursive submodule network operations are disabled.

An operation runs once. A timeout or lost result can leave local or remote state changed; GitTurtle reports the uncertainty rather than retrying automatically. Refresh and inspect the affected repository before retrying. Identity writes use worktree configuration when enabled, otherwise repository-local configuration shared by linked worktrees; global identity is not changed.

The [Git service documentation](crates/git-core/README.md) describes passive-read safeguards, explicit operations, local LFS compatibility, and backend measurements. Those measurements exclude the native UI and are not interaction-latency guarantees.

## Current limits

- Staging operates on whole files, not individual hunks. Conflict resolution, rebase/merge editing, branch deletion, force push, and submodule management are not provided. Resolve conflicts with your editor/Git tools, then refresh and stage the resolved files.
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

History reading, preview decoding, graph layout, and cache management run on a replaceable background read queue. Working status and explicit Git operations use a separate serialized executor; app preferences use their own serialized writer. The history worker retains the current worktree's Git session across local refreshes and scope changes, preserving its persistent object reader while rereading mutable refs and history. Lists render visible rows, and selection generations reject stale results. Project agreements and code routing are in [AGENTS.md](AGENTS.md). The [development agent guidance](docs/agent-guidance.md) explains the app-specific instructions, performance review skill, native validation skill, and GPT-6 Astra guidance audit.
