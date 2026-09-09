# Native application guidance

These instructions supplement the [root agreements](../../AGENTS.md) for `crates/app`. Paths below are relative to this crate. Read the linked contracts for the concern being changed or reviewed; unrelated subsystems do not need to be loaded.

## Ownership and entry points

| Concern | Source files | Focused contracts |
| --- | --- | --- |
| Pages, repository modes, selection, focus | `src/main.rs`, `src/views.rs`, `src/settings.rs`, `src/workspace.rs` | [Navigation and refresh](docs/navigation-and-refresh.md) |
| Replaceable reads, sessions, cancellation, cache | `src/worker.rs` | [Navigation and refresh](docs/navigation-and-refresh.md); [content preparation](docs/content-and-layout.md#prepared-content) |
| History search, path/rename lineage, attribution | `src/history_search.rs`, `src/file_history.rs`, `src/blame.rs` | [Search and retained inspections](docs/navigation-and-refresh.md#search-and-retained-inspections) |
| Local event subscriptions and quiet refresh | `src/local_refresh.rs`, `src/automatic_refresh.rs` | [Local refresh](docs/navigation-and-refresh.md#local-refresh) |
| Working status, composer, operation execution | `src/workspace.rs`, `src/operations.rs`, `src/commit_drafts.rs` | [Writes and persistence](docs/writes-and-persistence.md); [composer layout](docs/content-and-layout.md#working-composer) |
| Branch/remote forms, integration, conflicts, recovery/stash | `src/branch_actions.rs`, `src/integration.rs`, `src/conflicts.rs`, `src/recovery.rs` | [Writes and persistence](docs/writes-and-persistence.md) |
| Tags, ignore, authentication, native menus and local handoff | `src/tags.rs`, `src/ignore.rs`, `src/authentication.rs`, `src/platform_polish.rs` | [Writes and persistence](docs/writes-and-persistence.md) |
| Project hub, settings, recents, preference storage | `src/projects.rs`, `src/settings.rs`, `src/preferences.rs` | [Writes and persistence](docs/writes-and-persistence.md); [pages and focus](docs/navigation-and-refresh.md#pages-and-focus) |
| Theme, density, columns, branch tree and graph models | `src/appearance.rs`, `src/columns.rs`, `src/navigation.rs`, `src/graph.rs`, `src/views.rs` | [Content and layout](docs/content-and-layout.md) |
| Patch, split source, partial actions, editor Find | `src/text.rs`, `src/diff_view.rs`, `src/split_diff.rs`, `src/partial_view.rs`, `src/editor_find.rs` | [Content and layout](docs/content-and-layout.md) |
| Image comparison, overlay/wipe and pan | `src/image_compare.rs`, `src/worker.rs`, `src/views.rs` | [Images and cache accounting](docs/content-and-layout.md#images-and-cache-accounting) |

The [design](../../docs/design.md) describes the dense, full-height workspace and persistent inspector; the [architecture](../../docs/architecture.md) describes the end-to-end data flow and bounds. Keep subsystem details in the focused contracts instead of extending this file for each regression.

## Cross-cutting contracts

- `GitTurtle` owns interaction state. `select_commit` requests `Job::Changes`; a highlighted file is not activated. `select_file` explicitly enters Compare. A late `Output::Preview` cannot reopen Compare after Back or create an editor on a hidden page. Keep request generations, repository identity checks, and page/mode guards together.
- Back from Compare uses retained history, query, selection, scroll and inspector state. Working Changes retains history files separately. Resolve mutable scope identities again on refresh; accepted writes refresh local state without discarding browsing intent.
- Prepare repository reads, graph topology, patches, split rows and render-image pixels off the UI thread. Render only the active page, virtualize lists, and construct only the displayed editor mode. Selection uses stable commit/file identity across list changes.
- `src/worker.rs` handles replaceable reads. `src/operations.rs` serializes accepted typed writes, which survive dropped replies and are never silently retried. A separate executor serializes app preference saves. Git semantics belong in [core](../git-core/AGENTS.md); raster/SVG decoding belongs in [preview](../preview/AGENTS.md).

## Validation

Follow the root validation guidance and its performance/native-QA skill routing. Focus tests on the affected model, cancellation/retention boundary, or operation outcome; interaction changes also need native evidence. Documentation-only edits need path, symbol, link and diff checks, without a Rust rebuild. Theme changes require checking resolved native component backgrounds with foregrounds; palette contrast alone misses stale control tokens.
