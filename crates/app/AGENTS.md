# Native application guidance

These instructions supplement the [root agreements](../../AGENTS.md) for `crates/app`. Paths below are relative to this crate. Read the linked contracts for the concern being changed or reviewed; unrelated subsystems do not need to be loaded.

## Ownership and entry points

| Concern | Source files | Focused contracts |
| --- | --- | --- |
| Pages, repository modes, selection, focus | `src/main.rs`, `src/page_navigation.rs`, `src/views.rs`, `src/workspace.rs` | [Navigation and refresh](docs/navigation-and-refresh.md) |
| Replaceable reads, sessions, cancellation, cache | `src/worker.rs`, `src/worker/repository_identity.rs` | [Navigation and refresh](docs/navigation-and-refresh.md); [content preparation](docs/content-and-layout.md#prepared-content) |
| History windows, graph traversal and repository search | `src/history_paging.rs`, `src/graph.rs`, `src/history_search.rs` | [History paging](docs/navigation-and-refresh.md#ordinary-history-paging); [search](docs/navigation-and-refresh.md#search-and-retained-inspections) |
| File history, attribution, revision comparison and Quick Open | `src/file_history.rs`, `src/blame.rs`, `src/revision_inspection.rs` | [Search and retained inspections](docs/navigation-and-refresh.md#search-and-retained-inspections) |
| Changed-file filtering and working multiselection | `src/path_filter.rs`, `src/working_selection.rs` | [Search and retained inspections](docs/navigation-and-refresh.md#search-and-retained-inspections); [working changes](docs/writes-and-persistence.md#working-changes-and-recovery) |
| Local event subscriptions and quiet refresh | `src/local_refresh.rs`, `src/automatic_refresh.rs` | [Local refresh](docs/navigation-and-refresh.md#local-refresh) |
| Repository tabs, retained workspaces and unavailable paths | `src/repository_tabs.rs`, `src/repository_access.rs` | [Repository tabs](docs/navigation-and-refresh.md#repository-tabs); [restart bounds](../../docs/repository-tabs.md) |
| Working status, composer, operation execution and activity | `src/workspace.rs`, `src/operations.rs`, `src/commit_drafts.rs`, `src/activity.rs` | [Writes and persistence](docs/writes-and-persistence.md); [composer layout](docs/content-and-layout.md#working-composer) |
| Branch/remote/worktree forms, tags, ignore and LFS downloads | `src/branch_actions.rs`, `src/worktrees.rs`, `src/tags.rs`, `src/ignore.rs`, `src/lfs_download.rs` | [Writes and persistence](docs/writes-and-persistence.md); [LFS previews](../../docs/lfs-previews.md) |
| Integration, conflicts, recovery/stash and reflog | `src/integration.rs`, `src/conflicts.rs`, `src/recovery.rs`, `src/recovery_drafts.rs`, `src/reflog.rs` | [Working changes and recovery](docs/writes-and-persistence.md#working-changes-and-recovery); [conflict blocks](../../docs/conflict-blocks.md) |
| Interactive rebase and rewritten-series review/publication | `src/interactive_rebase.rs`, `src/rewrite_review.rs` | [Interactive rebase](../../docs/interactive-rebase.md); [rewritten series](../../docs/rewritten-series.md) |
| Git authentication, native menus and local handoff | `src/authentication.rs`, `src/platform_polish.rs` | [Writes and persistence](docs/writes-and-persistence.md); [authentication](../../docs/authentication.md) |
| GitHub account, PR inspection, reviews and durable outbound attempts | `src/github.rs`, `src/github/`, `src/github_view.rs`, `src/github_view/review.rs` | [GitHub collaboration](../../docs/github-collaboration.md); [collaboration boundary](docs/writes-and-persistence.md#github-collaboration) |
| Project hub, settings, recents, preferences and named profiles | `src/projects.rs`, `src/settings.rs`, `src/preferences.rs`, `src/profiles.rs`, `src/profiles/store.rs` | [Writes and persistence](docs/writes-and-persistence.md); [profiles](../../docs/profiles.md) |
| Command dispatch, keyboard focus, accessibility and OS display settings | `src/command_palette.rs`, `src/native_accessibility.rs`, `src/native_accessibility/`, `src/desktop_text.rs`, `src/page_navigation.rs` | [Command palette](../../docs/command-palette.md); [native accessibility](../../docs/native-accessibility.md); [pages and focus](docs/navigation-and-refresh.md#pages-and-focus) |
| Theme, density, columns, branch tree and graph models | `src/appearance.rs`, `src/columns.rs`, `src/navigation.rs`, `src/graph.rs`, `src/views.rs` | [Content and layout](docs/content-and-layout.md) |
| Patch, split source, review options, partial actions and editor Find | `src/text.rs`, `src/diff_view.rs`, `src/split_diff.rs`, `src/text_review.rs`, `src/partial_view.rs`, `src/editor_find.rs` | [Content and layout](docs/content-and-layout.md) |
| Image comparison, GIF playback and renderer image lifetime | `src/image_compare.rs`, `src/gif_playback.rs`, `src/image_lifetime.rs`, `src/worker.rs` | [Images and cache accounting](docs/content-and-layout.md#images-and-cache-accounting) |
| Captured PDF, Markdown/Mermaid, local resources and interactive models | `src/rich_preview.rs`, `src/pdf_view.rs`, `src/markdown_view.rs`, `src/model_view.rs` | [Captured documents](docs/content-and-layout.md#captured-documents); [document preview contract](../../docs/document-previews.md); [3D formats](../../docs/interactive-3d.md) |
| Prepared-content and cached-path CPU benchmarks | `src/review_bench.rs` (test module in `src/working_selection.rs`) | [App benchmark runner](../../scripts/bench-app-review.py); [recorded measurement scope](../../docs/benchmarks/2026-09-09-review-app.md) |

The [design](../../DESIGN.md) describes the dense, full-height workspace and persistent inspector; the [architecture](../../docs/architecture.md) describes the end-to-end data flow and bounds. Keep subsystem details in the focused contracts instead of extending this file for each regression.

## Cross-cutting contracts

- `GitTurtle` owns interaction state. `select_commit` requests `Job::Changes`; a highlighted file is not activated. `select_file` explicitly enters Compare. A late `Output::Preview` cannot reopen Compare after Back or create an editor on a hidden page. Keep request generations, repository identity checks, and page/mode guards together.
- Back from Compare uses retained history, query, selection, scroll and inspector state. Working Changes retains history files separately. Resolve mutable scope identities again on refresh; accepted writes refresh local state without discarding browsing intent.
- Prepare repository reads, graph topology, patches, split rows and render-image pixels off the UI thread. Render only the active page, virtualize lists, and construct only the displayed editor mode. Selection uses stable commit/file identity across list changes.
- `src/worker.rs` handles replaceable reads. `src/operations.rs` serializes accepted typed writes, which survive dropped replies and are never silently retried. A separate executor serializes app preference saves. Git semantics belong in [core](../git-core/AGENTS.md); supplied-byte decoding belongs in [preview](../preview/AGENTS.md).
- GitHub collaboration uses app-owned transport, captured provider identities and the operation executor. Preserve its separate credentials, local drafts and durable attempt records; local reads and panel reopening must not dispatch network work. Read the linked collaboration contract before changing that boundary.

## Validation

Follow the root validation guidance and its performance/native-QA skill routing. Focus tests on the affected model, cancellation/retention boundary, or operation outcome; interaction changes also need native evidence. Documentation-only edits need path, symbol, link and diff checks, without a Rust rebuild. Theme changes require checking resolved native component backgrounds with foregrounds; palette contrast alone misses stale control tokens.
