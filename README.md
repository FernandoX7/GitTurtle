# GitTurtle

GitTurtle is a native Git client built with Rust and GPUI for everyday Git work, history, code changes, and image comparisons. It uses your installed Git executable, with no Electron runtime or AI features. Browsing stays local; repository writes and network operations start from explicit actions.

Native builds have been exercised on macOS; the [validation notes](docs/validation.md) and [current milestone record](docs/review-milestone.md) identify the checked builds and limits. A [quality workflow](.github/workflows/quality.yml) is configured for macOS and Linux, but hosted CI execution and Linux builds/native interaction have not been verified here. See [the design specification](DESIGN.md) for the broader intended experience.

## Current source features

- A project hub with searchable recent repositories, native folder selection, clone, and create.
- Working changes with visible multi-file/range selection, path filtering, directory grouping, exact file/hunk/changed-line staging, and saved commit drafts per worktree.
- Explicit branch creation/switching, tracking, rename, safe deletion, upstream and remote configuration, fetch, fast-forward pull, and push to a chosen remote branch.
- Configured credential-helper and SSH-agent authentication, native fallback prompts, transfer progress, cancellation, and actionable credential/signing failures.
- A filterable local tag browser, annotation inspection, lightweight/annotated creation, captured local deletion, and explicit push of one named tag.
- Contextual ignore for untracked files or their containing directory, with exact shared `.gitignore` or local `info/exclude` rule review.
- Reviewed merge/rebase and native linear interactive rebase plans, with per-block conflict choices, manual result drafts, explicit saving/staging, and Continue, Abort, or Keep files actions.
- Reviewed worktree creation/removal and handoff; local operation activity, reflog inspection, and recovery branch creation that preserves current work.
- Named stashes with staged/unstaged/untracked inspection, separate restore and drop actions, amend, undo of an eligible last local commit, revert, and cherry-pick.
- Local and remote-tracking branches grouped into expandable folders, linked worktrees, cancellable repository-wide search, and file history that follows renames.
- Committed and working-file blame, explicit uncommitted lines, selected-line history, and commit comparison with retained return context.
- Full-height history with separate reference, graph, and commit-summary columns, alongside persistent commit details and changed files.
- Full-height file comparison, searchable Before/After local revision targets, endpoint or merge-base comparisons, root comparisons, and explicit merge-parent selection.
- Command-P Quick Open of tracked paths in the current worktree or a pinned revision, with direct File History and Blame access.
- Selectable, read-only unified and aligned split diffs with syntax coloring, intraline highlights, line numbers, whitespace review filtering, change navigation, bounded context expansion, and separate **Before** and **After** source tabs.
- Side-by-side, Overlay and draggable Wipe comparisons for PNG, JPEG, WebP, GIF, and supported static SVG previews, with transparency backgrounds, shared source geometry, linked zoom/pan, and keyboard controls.
- Verified local Git LFS text/image previews and an explicit reviewed download for one missing object, with authentication, cancellation, size and SHA-256 verification.
- Midnight, Daylight, Graphite, Tokyo Night, Catppuccin Mocha, and Nord themes; follow-system appearance; independent interface/code text sizes; comfortable/compact density; configurable history columns; recent-project and startup preferences.
- Native File/Edit/View/Window/Help menus, shortcut help, and explicit repository handoff to Finder or a preferred editor.
- Repository Git identity settings, resizable panes, keyboard navigation, and local refresh after filesystem changes or returning focus.

## Open a project and work with Git

Use **Projects** to reopen a recent repository, choose an existing folder, clone a URL or local repository, or create a new repository. Clone and Create take a parent folder and project-folder name; the destination must be new or empty. Create also lets you choose the initial branch.

**Working Changes** separates staged and unstaged files. Select either side to inspect it, then stage or unstage a file, a text hunk, selected changed lines, or the whole list. Command-click toggles files; Shift-click or Shift-Up/Down extends a range; Command-A selects visible rows while the file list has focus. **Stage N** and **Unstage N** review only the selected files in that area, including both rename paths. Filtering and changing directory grouping clear selection; refresh retains only visible, surviving paths. A repository switch clears selection. Filtered lists disable all-files actions. A file can appear in both lists when it has staged and further unstaged edits. The title and optional description commit only the staged snapshot; drafts are saved separately for each worktree and retained after a failure. Ordinary previews remain read-only.

The current-branch menu exposes branch search, switching, and management. **Targets** expands branch creation and the selected remote and destination branch for network operations. Creating a branch also switches to it. Pull accepts fast-forward updates only; Push uses the current local branch and an explicit destination branch, without force. Branch selection in the history navigator still changes only the history scope.

Branch context actions manage tracking, names, safe deletion, and upstreams; **Manage remotes** in the current-branch menu edits remote URLs and fetch mappings. Merge and rebase review the chosen branch and affected paths before running. Conflicts show the base and both named sides, with a bounded manual resolution editor, side choices, and an external-editor handoff. Continue reviews staged paths. Abort protects work it cannot safely restore; **Keep files** ends the operation while retaining the current files and index.

The Working Changes **Actions** menu offers **Ignore selected untracked file…**, saves named stashes, browses saved work, amends the last commit, or undoes an eligible last local commit. Restoring a stash keeps it saved until a separate Drop action. Selected-commit actions offer revert and cherry-pick, with an explicit mainline choice for merge commits. Recovery actions review their target and affected files before writing.

**Tags…** in the current-branch menu filters and inspects local tags. Create resolves the chosen commit before review, preserves configured Git signing, and retains the form after failure. Delete checks the captured tag object ID; remote tags are unchanged. A separate named-tag Push identifies one remote and never broadens ordinary branch Push into pushing tags.

For untracked content, choose **Ignore…** from a working row's context menu or **Actions → Ignore selected untracked file…**. Preview shows the exact literal rule and destination before applying it. Shared `.gitignore` edits remain unstaged; local excludes stay in Git metadata. A containing-directory rule leaves tracked files tracked. Stale or symbolic destinations are refused; no ignore action untracks, stages, or deletes content. See [tag and ignore workflows](docs/macos-git-actions.md) for details and limits.

**Settings** controls manual or follow-system appearance, independent interface (11–18) and code (10–24) sizes with resets, density, startup behavior, the external editor, default branch for new projects, and history column visibility. Drag header dividers to resize columns; narrow windows scroll horizontally without hiding your choices. The commit-message column stays visible. Git name/email edits are saved only after **Save repository identity** for the displayed repository.

**Worktrees…** reviews a new or existing branch and destination before creation, refusing an occupied branch or existing destination. Inspect a worktree to see its local state and open it in GitTurtle, Finder, or the configured editor. Removal protects the main/current worktree and refuses dirty, untracked, ignored, locked, missing, conflicted, or stale targets. It does not delete the branch. [Worktree and reflog semantics](docs/parallel-work-recovery.md) describes the checks and partial-failure feedback.

**Activity…** (Command-Shift-A) records the latest 200 GitTurtle operations locally, including their target and result. It excludes arbitrary hook/server diagnostics from retained history and does not claim to cover other tools. **Local reflog & recovery…** inspects local reflog entries and can create a new branch at an available commit without checking it out. Expired entries or missing objects cannot always be recovered.

**Edit local commits…** prepares up to 100 commits from one linear sequence. Reorder with the controls or Option-Up/Down and choose Pick, Reword, Squash, Fixup, or Drop. The reviewed plan pins the branch, base, commits, and clean work state; it warns about known remote-tracking containment. Message pauses, conflicts, Continue, and Abort use native workflows while preserving configured hooks/signing. Merge-preserving plans remain unsupported. See [interactive rebase](docs/interactive-rebase.md) and [conflict blocks](docs/conflict-blocks.md).

A missing LFS preview offers **Download Before LFS…** or **Download After LFS…**. Review the exact file, object, configured source, and size before downloading that one object. GitTurtle verifies size and SHA-256 before displaying it; the action leaves working files and the index unchanged. Resolved LFS text keeps whole-file staging because Git still stores its pointer. See [LFS previews and download limits](docs/lfs-previews.md).

## Browse history, then open a comparison

History fills the center of the window. Repository navigation sits on the left; the right inspector holds the selected commit's details, parent choice, and changed-file list. Local and remote branch names form folders from their slash-separated prefixes, with remote names such as `origin` at the top of the remote tree. Filtering reveals matching branches without changing saved folder expansion.

Selecting a commit updates its details and file list while staying in History. Initially highlighting the preferred or first file does not prepare its preview. Click a changed file, or press Enter to open the highlighted file from History or the file list, to enter Compare: the file fills the center height, navigation collapses to a compact rail, and the same inspector and file list remain on the right. Select another file there to continue comparing.

**Back to history** returns to the retained graph, selected commit, scope, search, loaded history, and scroll position. Returning does not reopen the repository or reload its history. File content is loaded on explicit activation; revisiting content can reuse the bounded preview cache.

**Compare revisions…** (Command-Shift-C) accepts branch names, tags, hashes, and other local commit expressions. Choose **Endpoints** for Before → After, or **Changes since branching** for the unique merge base → After. Swap changes direction. The inspector shows resolved IDs; moving references do not silently change an open comparison. Refresh opens a new review. Missing objects, unrelated histories, and multiple merge bases are reported without fetching or switching branches.

**Quick Open File…** (Command-P) searches tracked paths in the current worktree or a chosen revision. Type a path fragment, use Up/Down, and press Return. Escape cancels; Back restores prior browsing. Revision results pin the resolved commit. Worktree scope includes tracked deleted paths, which report missing content. The search scans at most 100,000 entries or 16 MiB and returns at most 500 matches, with a visible limit message. The changed-file inspector also has its own path filter.

Text comparisons highlight intraline edits. **Hide whitespace** and **Context N +** change review presentation; **Reset review** restores the exact underlying snapshot. Partial staging is disabled while either review option is active, and whole-file actions still include every change. Previous/next buttons and Option-Up/Down navigate hunks. Context expands through 3, 12, 48, and 192 lines within preview bounds.

History search matches literal text in messages, author names, and hashes beyond the loaded rows. It searches all locally available refs or the selected branch/worktree ancestry, pins that scope for consistent pages, and shows matches, scanned commits, and whether more work remains. Cancel keeps the results already found; Continue searches another bounded page. Restart searches the current tips in the same scope, and clearing the query restores ordinary history. Local automatic refresh and completed writes retain the pinned search until Restart.

**File history** opens a contextual revision list from a changed file. It follows renames along the first parent of each merge and compares each revision using its actual paths, including deleted files. Closing it restores the prior comparison and browsing context.

**Blame** in a text comparison attributes each line. Committed views use the selected revision; Working Changes uses the raw working file against HEAD, so staged-only and unstaged edits both appear as **Uncommitted**. Select a committed line to **Compare commit** or inspect **History of line**; Back restores the prior attribution and comparison. Line history follows first parents and rename heuristics, with a visible 100-change boundary. Copy and moved-line detection across files are excluded.

Image comparisons offer **Side by side**, **Overlay** and **Wipe**. Overlay adjusts After opacity; Wipe reveals Before on the left and After on the right. Zoom/pan and keyboard adjustment controls use shared source coordinates, retaining dimension differences and absent sides. The scale label distinguishes the bounded comparison preview from source resolution. [Feature semantics](docs/macos-features.md) covers attribution, images and macOS conventions.

The application retains opaque theme surfaces after a real native Liquid Glass prototype exposed a GPUI compositing limitation. The [investigation and captured prototype](docs/liquid-glass-investigation.md) explain that result; native Liquid Glass is not advertised as an implemented appearance.

## Run from source

Use Rust 1.98 or newer and an installed Git executable. On macOS, install Xcode and its Metal toolchain. Use the checked-in lockfile.

```sh
cargo run --locked -p gitturtle -- /path/to/repository

# Use the optimized build when evaluating interaction performance.
cargo run --release --locked -p gitturtle -- /path/to/repository
```

Open an existing clone or linked worktree. With no path argument, the app reopens the last remembered repository when that setting is enabled; otherwise it opens Projects. App settings are stored separately from repositories at `~/Library/Application Support/GitTurtle/preferences.json` on macOS. The Linux settings path is `$XDG_CONFIG_HOME/gitturtle/preferences.json`, falling back to `~/.config/gitturtle/preferences.json`.

## Package locally on macOS

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

This is local development packaging for native verification. The script produces a bundle for the build machine's architecture, includes the application assets, and applies a local ad-hoc signature. `--debug --no-build` packages an existing debug executable. An optional final argument changes the output `.app` path. This development bundle is not notarized and is not a universal binary.

## Keyboard controls

| macOS shortcut | Action |
| --- | --- |
| Command-O | Open a repository |
| Command-Shift-O | Open Projects |
| Command-P | Quick Open a tracked file |
| Command-Shift-C | Compare local revisions |
| Command-Shift-A | Open local operation activity |
| Option-Up / Option-Down in a text preview | Previous / next change |
| Shift-Up / Shift-Down in Working Changes | Extend file selection |
| Command-A in the Working Changes file list | Select visible file rows |
| Command-1 / Command-2 | Open History / Working Changes |
| Command-, | Open Settings |
| Command-[ | Back through the current inspection; otherwise return to History without clearing the query |
| Command-R | Refresh the local snapshot |
| Command-F in the history or file list | Focus repository history search |
| Command-F in a text preview | Open the editor's text search |
| Escape | Close the active transient view or editor search first; otherwise return from Compare to retained History or clear focused history search |
| Up / Down | Select the previous / next commit in history, or open the previous / next file in the focused file list |
| Home / End | Select the first / last commit in history, or open the first / last file in the focused file list |
| Enter | Open the highlighted file in Compare, from History or the file list |
| Option-Up / Option-Down in the rebase sequence | Move the selected commit up / down |
| P / R / S / F / D in the rebase sequence | Pick / Reword / Squash / Fixup / Drop |
| Command-Option-Up / Command-Option-Down in the conflict viewer | Previous / next unresolved block |
| Command-B | Show or hide expanded repository navigation in History |
| Command-Shift-/ | Open Keyboard Shortcuts |
| Command-M / Command-W | Minimize / Close the window |
| Command-H / Command-Option-H | Hide GitTurtle / Hide other applications |
| Command-Q | Quit |

The non-macOS key bindings use Control instead of Command; their native behavior has not been tested on Linux. Text previews support selection and copying without editing repository content.

## Repository operations

History, status, and previews read local objects, references, and files without changing the index or checkout. Passive reads disable external diff/text-conversion helpers and filters; working-file previews show raw content and do not follow stored symlinks.

**Refresh rereads local state only.** It resolves the selected branch or worktree's current tip from the new snapshot. Remote-tracking branches change after an explicit fetch in GitTurtle or another tool. Missing partial-clone objects and preview LFS content are not downloaded automatically.

Local filesystem notifications are coalesced before reading status and changed refs; returning focus also requests a local rescan. These updates retain selection, comparison context, scroll position, and drafts. They wait behind active operations and foreground reads. A watcher failure is visible, and manual Refresh remains available.

Staging, committing, switching branches, cloning, and network actions use a separate background executor. Real Git writes preserve configured hooks, identity, filters, signing, credential helpers, SSH agents and host verification. Existing helpers and askpass programs retain their roles; when Git requests credentials without a configured askpass program, GitTurtle provides a native prompt with masked secrets. Configured helpers may save credentials, including through macOS Keychain; GitTurtle does not save them. Progress and Cancel appear for explicit operations. Automatic maintenance and recursive submodule network operations are disabled. See [authentication behavior and fixture coverage](docs/authentication.md); live-provider credentials and actual Keychain unlock have not been verified here.

An operation runs once. Cancellation, a timeout or a lost result can leave local or remote state changed; GitTurtle reports the uncertainty rather than retrying automatically. Refresh and inspect local state before retrying; checking a remote after an uncertain Push requires a separate explicit network action. Identity writes use worktree configuration when enabled, otherwise repository-local configuration shared by linked worktrees; global identity is not changed.

The [Git service documentation](crates/git-core/README.md) describes passive-read safeguards, explicit operations, LFS compatibility, and backend measurements. Those measurements exclude the native UI and are not interaction-latency guarantees.

## Current limits

- Partial staging is available for supported raw text changes. Binary, oversized, filtered/normalized, renamed, and mode/type-changing files use whole-file operations. Ambiguous selections around a missing final newline require the complete replacement or hunk. Text previews and manual conflict drafts are limited to 2 MiB and 100,000 lines per side.
- History starts with 500 commits. **Load more** increases the loaded prefix by 500, up to 10,000. Search separately scans repository history in bounded pages and retains up to 10,000 matches or 64 MiB of metadata. A scan, byte, or time limit means more may remain; the UI never treats that stop as an exhaustive result.
- Rename detection has a 1,000-candidate limit. File history uses 100-row pages and first-parent lineage, not every possible route through a merge. It replays the rename-following prefix to keep page boundaries correct; sufficiently deep histories can reach the 32 MiB or 15-second read limit and require an older revision as the anchor.
- Native interactive rebase supports up to 100 commits in a linear sequence after an exclusive base. Root/merge-preserving rewrites, apply-backend continuation, non-UTF-8 messages, and ambiguous external reword checkpoints require Git's configured tools. Unsaved rebase-message and conflict-result drafts survive navigation in memory, but are not persisted across app restarts. Force push and submodule management remain outside the UI. Undo requires a named local tip with exactly one parent and refuses commits contained in known remote-tracking refs; it cannot establish publication on remotes that have not been fetched.
- Conflict-block decisions support at most 4,096 recognized text blocks within the text preview bounds; unsupported or malformed markers retain manual/whole-file fallbacks. Review context expands to 192 lines; review variants are bounded to 100,000 combined source rows and disable partial staging until reset.
- Explicit LFS downloads require installed Git LFS and a configured source, and are capped at 32 MiB per object. Preview decoding limits still apply. Normal browsing never downloads missing Git or LFS objects.
- Images display the first frame. The app requests previews with a maximum edge of 1,600 pixels; **100% uses the bounded comparison preview scale**, which may be smaller than source resolution. The shared scale preserves original dimension ratios even when each side was downsampled differently. Source and decoded dimensions remain visible. Encoded input is limited to 32 MiB, with additional pixel and decoder limits. SVG filters and embedded/external images are unsupported and produce an explicit error.
- Histories exceeding the graph's lane or edge budget show isolated commit nodes with an explanation. Selecting a branch can reduce the graph size while preserving access to commits and file previews.
- Blame uses at most 2 MiB and 100,000 text lines; missing shallow history is marked, and unavailable objects never trigger a fetch. Working attribution includes the working file rather than an index-only snapshot.
- The tag browser loads at most 10,000 tags; large annotations above 256 KiB retain identity/actions with an unavailable-content message. Ignore destinations are limited to 1 MiB and cannot express filenames containing line breaks.
- Hosted CI, Linux builds/native interaction, live-provider authentication, actual Keychain unlock and hardware-backed signing remain unverified here. Distribution, installers, notarization and publishing are outside this milestone.

## Development and measurement

[The quality workflow](.github/workflows/quality.yml) configures the following gates on macOS 15 and Ubuntu 24.04 with Rust 1.98.0. It has no distribution or upload jobs. Local passes and workflow configuration do not establish a hosted CI pass or Linux coverage.

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

The [September 9 review backend report](docs/benchmarks/2026-09-09-review-backend.md) records thirteen passive paths with forty measured release calls each, raw tail latencies, source hashes, fixture preservation, and uncontrolled background-load context. It establishes backend costs for that recorded source, without a speedup or final-app latency claim.

History reading, search, preview decoding, graph layout, and cache management run on a replaceable background read queue. Search, file-history, blame and line-history cancellation can terminate the active Git process. Working status and explicit Git operations use a separate serialized executor; app preferences and coalesced commit drafts use their own serialized writer. The history worker retains the current worktree's Git session across local refreshes and scope changes, preserving its persistent object reader while rereading mutable refs and history. Lists render visible rows, and selection generations reject stale results. The [architecture](docs/architecture.md) describes scheduling and bounds. Project agreements and code routing are in [AGENTS.md](AGENTS.md); the [development agent guidance](docs/agent-guidance.md) records the focused skills and GPT-6 Astra guidance audit.
