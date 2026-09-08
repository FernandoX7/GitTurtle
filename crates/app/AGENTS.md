# Native application guidance

These instructions supplement the root agreements for `crates/app`.

## Ownership and entry points

- `main.rs`: `GitTurtle` interaction state; `request`/`receive`, `select_commit`, `select_file`, `change_parent`, `back_to_history`, and `ensure_editor` connect selection to worker results.
- `views.rs`: GPUI layout, virtualized rows, inspector, image controls, and pointer handlers. Keep the dense, full-height workspace and persistent inspector described in [the design](../../docs/design.md).
- `worker.rs`: `Job`/`Output`, `RepositorySession`, `PreviewCache`, cancellation, Git reads, and content preparation. UI receives owned results.
- `navigation.rs` and `graph.rs`: pure models with focused tests. `preferences.rs` owns serialization/storage; the worker serializes writes.
- `text.rs` and `diff_view.rs`: prepared patch metadata, lazy read-only editors, and a separately painted old/new gutter.

## Interaction contracts

Commit selection in History requests `Job::Changes`; a highlighted file is not an activation. Explicit activation enters Compare. `receive(Output::Preview)` may retain current content after Back, but cannot reopen Compare or initialize a hidden editor. Preserve generation checks and the workspace-mode guard when changing asynchronous code.

Back restores the retained scope, query, loaded history, selection, scroll position, and inspector without another Git request. Refresh re-resolves mutable branch/worktree identities; session reuse must not freeze tips. Parent changes must keep the displayed comparison and selected file consistent. Use stable commit/file identity rather than a virtualized row index when rebuilding lists.

Keep history and file-list keyboard focus distinct. Retain the selected file's visibility as lists change. Temporary search expansion must not overwrite saved branch-folder expansion choices.

## Content presentation contracts

`worker::text_content` prepares `text::PatchPresentation` before delivery. `text::editor` and `diff_view::new` consume its shared rows, gutter width, and decoration ranges without rescanning the patch. Keep literal patch text unchanged for copy/search; presentation-only numbers belong in the gutter. Preserve byte-indexed decorations for UTF-8 and hunk-side line numbers for empty sides, CRLF, and no-newline markers.

Gutter scrolling follows the editor's actual text geometry and accumulates wheel input until the next paint; preserve horizontal offset and bounds. Patch wrapping/folding is disabled; enabling either requires coordinated gutter layout and native checks. Check both gutter and code-area scrolling after changes. Only construct the requested Diff, Before, or After editor.

Prepare render-image pixels on the worker. Before/After image sides share bounded pan state; active drags continue across the app content and clear on release or preview replacement. Preserve absent/error sides and the distinction between source dimensions and decoded-preview zoom.

The preview cache counts retained source buffers, patch metadata, and CPU image allocations. UI-held `Arc`s, editors, and GPU textures may outlive eviction; the cache budget is not a total-memory cap. Changes to `Content` must keep `Content::bytes` accurate.

Run relevant app tests with `cargo test --locked -p gitturtle`. Native interaction evidence and performance claims follow the root validation guidance and its linked skills; pure model tests do not validate rendered behavior.
