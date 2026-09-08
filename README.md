# GitTurtle

GitTurtle is a native Git client built with Rust and GPUI for everyday Git work, history, code changes, and image comparisons. It uses your installed Git executable, with no Electron runtime or AI features. Browsing stays local; repository writes and network operations start from explicit actions.

Native builds have been exercised on macOS; the [validation notes](docs/validation.md) identify the checked builds and limits. The shared Rust implementation is intended to support Linux, but Linux builds, packaging, and native interaction have not been validated. See [the design specification](docs/design.md) for the broader intended experience.

## Current source features

- A project hub with searchable recent repositories, native folder selection, clone, and create.
- Working changes with file, hunk, and changed-line staging/unstaging, plus commit titles, descriptions, and saved drafts per worktree.
- Explicit branch creation/switching, tracking, rename, safe deletion, upstream and remote configuration, fetch, fast-forward pull, and push to a chosen remote branch.
- Deliberate merge/rebase, conflict inspection of the base and both sides, manual resolution or side selection, and Continue, Abort, or Keep files actions.
- Named stashes with staged/unstaged/untracked inspection, separate restore and drop actions, amend, undo of an eligible last local commit, revert, and cherry-pick.
- Local and remote-tracking branches grouped into expandable folders, linked worktrees, cancellable repository-wide search, and file history that follows renames.
- Full-height history with separate reference, graph, and commit-summary columns, alongside persistent commit details and changed files.
- Full-height file comparison, root comparisons, and explicit merge-parent selection.
- Selectable, read-only unified and aligned split diffs with syntax coloring and line numbers, plus separate **Before** and **After** source tabs.
- Before/after PNG, JPEG, WebP, GIF, and supported static SVG previews, transparency backgrounds, linked zoom, and drag-to-pan.
- Local Git LFS image previews when the stored object passes size and SHA-256 verification; explicit messages for unavailable or unsupported content.
- Midnight, Daylight, Graphite, Tokyo Night, Catppuccin Mocha, and Nord themes; comfortable/compact density; configurable history columns; recent-project and startup preferences.
- Repository Git identity settings, resizable panes, keyboard navigation, and local refresh after filesystem changes or returning focus.

## Open a project and work with Git

Use **Projects** to reopen a recent repository, choose an existing folder, clone a URL or local repository, or create a new repository. Clone and Create take a parent folder and project-folder name; the destination must be new or empty. Create also lets you choose the initial branch.

**Working Changes** separates staged and unstaged files. Select either side to inspect it, then stage or unstage a file, a text hunk, selected changed lines, or the whole list. A file can appear in both lists when it has staged and further unstaged edits. The title and optional description commit only the staged snapshot; drafts are saved separately for each worktree and retained after a failure. Ordinary previews remain read-only.

The current-branch menu exposes branch search, switching, and management. **Targets** expands branch creation and the selected remote and destination branch for network operations. Creating a branch also switches to it. Pull accepts fast-forward updates only; Push uses the current local branch and an explicit destination branch, without force. Branch selection in the history navigator still changes only the history scope.

Branch context actions manage tracking, names, safe deletion, and upstreams; **Manage remotes** in the current-branch menu edits remote URLs and fetch mappings. Merge and rebase review the chosen branch and affected paths before running. Conflicts show the base and both named sides, with a bounded manual resolution editor, side choices, and an external-editor handoff. Continue reviews staged paths. Abort protects work it cannot safely restore; **Keep files** ends the operation while retaining the current files and index.

The Working Changes **Actions** menu saves named stashes, browses saved work, amends the last commit, or undoes an eligible last local commit. Restoring a stash keeps it saved until a separate Drop action. Selected-commit actions offer revert and cherry-pick, with an explicit mainline choice for merge commits. Recovery actions review their target and affected files before writing.

**Settings** controls the theme, density, startup behavior, default branch for new projects, and history column visibility. Drag header dividers to resize columns; narrow windows scroll horizontally without hiding your choices. The commit-message column stays visible. Git name/email edits are saved only after **Save repository identity** for the displayed repository.

## Browse history, then open a comparison

History fills the center of the window. Repository navigation sits on the left; the right inspector holds the selected commit's details, parent choice, and changed-file list. Local and remote branch names form folders from their slash-separated prefixes, with remote names such as `origin` at the top of the remote tree. Filtering reveals matching branches without changing saved folder expansion.

Selecting a commit updates its details and file list while staying in History. Initially highlighting the preferred or first file does not prepare its preview. Click a changed file, or press Enter to open the highlighted file from History or the file list, to enter Compare: the file fills the center height, navigation collapses to a compact rail, and the same inspector and file list remain on the right. Select another file there to continue comparing.

**Back to history** returns to the retained graph, selected commit, scope, search, loaded history, and scroll position. Returning does not reopen the repository or reload its history. File content is loaded on explicit activation; revisiting content can reuse the bounded preview cache.

History search matches literal text in messages, author names, and hashes beyond the loaded rows. It searches all locally available refs or the selected branch/worktree ancestry, pins that scope for consistent pages, and shows matches, scanned commits, and whether more work remains. Cancel keeps the results already found; Continue searches another bounded page. Restart searches the current tips in the same scope, and clearing the query restores ordinary history. Local automatic refresh and completed writes retain the pinned search until Restart.

**File history** opens a contextual revision list from a changed file. It follows renames along the first parent of each merge and compares each revision using its actual paths, including deleted files. Closing it restores the prior comparison and browsing context.

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
| Command-F in the history or file list | Focus repository history search |
| Command-F in a text preview | Open the editor's text search |
| Escape | Return from Compare to History when the app handles the action; in History, clear history search. An editor search can handle Escape first |
| Up / Down | Select the previous / next commit in history, or open the previous / next file in the focused file list |
| Home / End | Select the first / last commit in history, or open the first / last file in the focused file list |
| Enter | Open the highlighted file in Compare, from History or the file list |
| Command-B | Show or hide expanded repository navigation in History |
| Command-Q | Quit |

The non-macOS key bindings use Control instead of Command; their native behavior has not been tested on Linux. Text previews support selection and copying without editing repository content.

## Repository operations

History, status, and previews read local objects, references, and files without changing the index or checkout. Passive reads disable external diff/text-conversion helpers and filters; working-file previews show raw content and do not follow stored symlinks.

**Refresh rereads local state only.** It resolves the selected branch or worktree's current tip from the new snapshot. Remote-tracking branches change after an explicit fetch in GitTurtle or another tool. Missing partial-clone objects and preview LFS content are not downloaded automatically.

Local filesystem notifications are coalesced before reading status and changed refs; returning focus also requests a local rescan. These updates retain selection, comparison context, scroll position, and drafts. They wait behind active operations and foreground reads. A watcher failure is visible, and manual Refresh remains available.

Staging, committing, switching branches, cloning, and network actions use a separate background executor. Real Git writes preserve configured hooks, identity, filters, signing, and credential helpers. Configure authentication through Git or an SSH agent beforehand; GitTurtle has no interactive credential dialog. Automatic maintenance and recursive submodule network operations are disabled.

An operation runs once. A timeout or lost result can leave local or remote state changed; GitTurtle reports the uncertainty rather than retrying automatically. Refresh and inspect the affected repository before retrying. Identity writes use worktree configuration when enabled, otherwise repository-local configuration shared by linked worktrees; global identity is not changed.

The [Git service documentation](crates/git-core/README.md) describes passive-read safeguards, explicit operations, local LFS compatibility, and backend measurements. Those measurements exclude the native UI and are not interaction-latency guarantees.

## Current limits

- Partial staging is available for supported raw text changes. Binary, oversized, filtered/normalized, renamed, and mode/type-changing files use whole-file operations. Ambiguous selections around a missing final newline require the complete replacement or hunk. Text previews and manual conflict drafts are limited to 2 MiB and 100,000 lines per side.
- History starts with 500 commits. **Load more** increases the loaded prefix by 500, up to 10,000. Search separately scans repository history in bounded pages and retains up to 10,000 matches or 64 MiB of metadata. A scan, byte, or time limit means more may remain; the UI never treats that stop as an exhaustive result.
- Rename detection has a 1,000-candidate limit. File history uses 100-row pages and first-parent lineage, not every possible route through a merge. It replays the rename-following prefix to keep page boundaries correct; sufficiently deep histories can reach the 32 MiB or 15-second read limit and require an older revision as the anchor.
- Elaborate interactive history rewriting, force push, and submodule management remain outside the UI. External rebase steps that require editing a commit message must finish in Git's configured editor. Undo requires a named local tip with exactly one parent and refuses commits contained in known remote-tracking refs; it cannot establish publication on remotes that have not been fetched.
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

History reading, search, preview decoding, graph layout, and cache management run on a replaceable background read queue. Search and file-history cancellation can terminate the active Git process. Working status and explicit Git operations use a separate serialized executor; app preferences and coalesced commit drafts use their own serialized writer. The history worker retains the current worktree's Git session across local refreshes and scope changes, preserving its persistent object reader while rereading mutable refs and history. Lists render visible rows, and selection generations reject stale results. The [architecture](docs/architecture.md) describes scheduling and bounds. Project agreements and code routing are in [AGENTS.md](AGENTS.md); the [development agent guidance](docs/agent-guidance.md) records the focused skills and GPT-6 Astra guidance audit.
