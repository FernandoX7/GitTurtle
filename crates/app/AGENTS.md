# Native application guidance

These instructions supplement the root agreements for `crates/app`.

## Ownership and entry points

- `main.rs`: `GitTurtle` interaction state; `request`/`receive`, `select_commit`, `select_file`, `change_parent`, `back_to_history`, and `ensure_editor` connect selection to worker results.
- `views.rs`: GPUI layout, virtualized rows, inspector, image controls, and pointer handlers. Keep the dense, full-height workspace and persistent inspector described in [the design](../../docs/design.md).
- `worker.rs`: replaceable `Job`/`Output` reads, `RepositorySession`, `PreviewCache`, cancellation, and content preparation. UI receives owned results.
- `workspace.rs` and `operations.rs`: working status, staged/unstaged selection, typed Git writes, clone/create, and bounded serial execution. Git command semantics live in `crates/git-core/src/work.rs`.
- `projects.rs`: project hub and Open/Clone/Create/Back events. `settings.rs`: app preferences and explicit repository identity edits. `appearance.rs` and `columns.rs`: theme/density tokens and shared history geometry.
- `navigation.rs` and `graph.rs`: pure models with focused tests. `preferences.rs` owns atomic storage outside repositories; settings and recents share the dedicated serial preference executor.
- `text.rs` and `diff_view.rs`: prepared patch metadata, lazy read-only editors, and a separately painted old/new gutter.

## Interaction contracts

Commit selection in History requests `Job::Changes`; a highlighted file is not an activation. Explicit activation enters Compare. `receive(Output::Preview)` may retain current content after Back, but cannot reopen Compare or initialize a hidden editor. Preserve generation checks and both the `AppPage::Repository` and workspace-mode guards when changing asynchronous code.

Back from Compare restores the retained scope, query, loaded history, selection, scroll position, and inspector without another Git request. Working Changes keeps history files separately in `retained_history_files`; writes that refresh history invalidate that snapshot, so Back may request `Job::Changes` for the selected commit. Do not reuse mutable working files as the history inspector. Refresh re-resolves mutable branch/worktree identities; session reuse must not freeze tips. Parent changes must keep the displayed comparison and selected file consistent. Use stable commit/file identity rather than a virtualized row index when rebuilding lists.

Keep the root `app_focus`, history `focus`, and `file_focus` handles distinct. Projects and Settings focus the root so application shortcuts remain reachable; returning restores focus for the retained repository mode. Keep `GitTurtleList` navigation out of editable form fields, and hidden editors must not steal focus. Retain the selected file's visibility as lists change. Temporary search expansion must not overwrite saved branch-folder expansion choices.

Projects and Settings are pages separate from the retained repository modes. Hub events carry intent; clone/init and destination checks run off the UI thread. Preserve native-picker path bytes, user-edited form values, cancellation, and nearby errors. A submitted write keeps its captured repository/target and cannot be replaced or retried by later navigation; show busy state and refresh relevant status/history after success or failure. `SerialExecutor::submit` preserves accepted writes after a dropped reply; `submit_read` may skip superseded queued status reads. Preference persistence uses a separate executor instance.

Working-status replies check both `work_generation` and canonical repository identity. Refresh and writes use `invalidate_read` to advance the preview generation and cancel obsolete worker work, even when the next selection is empty. Retain the user's current path/area at reply time, including movement between staged and unstaged groups; do not restore the selection captured at dispatch. Mutable previews bypass the immutable cache.

Working Changes distinguishes HEAD-to-index from index-to-worktree previews, including files present in both lists. Keep commit drafts scoped to the resolved canonical worktree after discovery; drafts currently survive repository switches within the session only. Retain commit text on failure and clear it only when a successful commit matches the submitted text. Pass both paths when staging a rename. Passive status/preview reads remain read-only; real operations follow the [core write contracts](../git-core/AGENTS.md). Run mutation checks only in disposable fixtures.

Use semantic appearance colors for native controls and custom content together. A theme change invalidates concrete editor decorations without rereading immutable content or eagerly rebuilding hidden editors. Headers and rows share one `ColumnLayout` and horizontal offset; preserve chosen columns at narrow widths and keep the commit-message column visible. Persist normalized widths/visibility, settings, and recents through the same preference writer without applying stale save replies over newer UI choices. Repository identity saves use the operation executor, not the app-preferences file.

Preference saves reread the current disk counterpart before atomically merging settings or recents; a stale UI snapshot must not replace both. Preserve read-only loading, supported-version migration, and byte-safe recent paths. Invalid or unsupported existing stores must produce a save error rather than being overwritten with defaults.

## Content presentation contracts

`worker::text_content` prepares `text::PatchPresentation` before delivery. `text::editor` and `diff_view::new` consume its shared rows, gutter width, and decoration ranges without rescanning the patch. Keep literal patch text unchanged for copy/search; presentation-only numbers belong in the gutter. Preserve byte-indexed decorations for UTF-8 and hunk-side line numbers for empty sides, CRLF, and no-newline markers.

Gutter scrolling follows the editor's actual text geometry and accumulates wheel input until the next paint; preserve horizontal offset and bounds. Patch wrapping/folding is disabled; enabling either requires coordinated gutter layout and native checks. Check both gutter and code-area scrolling after changes. Only construct the requested Diff, Before, or After editor.

Prepare render-image pixels on the worker. Before/After image sides share bounded pan state; active drags continue across the app content and clear on release or preview replacement. Preserve absent/error sides and the distinction between source dimensions and decoded-preview zoom.

The preview cache counts retained source buffers, patch metadata, and CPU image allocations. UI-held `Arc`s, editors, and GPU textures may outlive eviction; the cache budget is not a total-memory cap. Changes to `Content` must keep `Content::bytes` accurate.

Run relevant app tests with `cargo test --locked -p gitturtle`. Native interaction evidence and performance claims follow the root validation guidance and its linked skills; pure model tests do not validate rendered behavior.
