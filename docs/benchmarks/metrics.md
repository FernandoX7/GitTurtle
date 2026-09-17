# Trace metrics and harness boundaries

`gitturtle.commit_files_frame_ms` measures selection handling through the changed-file-list frame callback; `gitturtle.file_preview_frame_ms` measures history-file activation through the prepared-preview callback. `gitturtle.working_preview_frame_ms` measures working-file activation through that callback, not the preceding status refresh or Git write. `gitturtle.history_page_frame_ms` measures the ordinary page handler through its next frame callback. These exclude pre-handler input delivery, OS presentation, and completed GPU execution. The status bar's content-read duration is worker work only. Historical `selection_frame_ms` includes eager previews and is not directly comparable. Save new records under `docs/benchmarks/` without rewriting previous measurements or attributing them to newer builds.

This catalog is a documentation path tasks may extend: when a task adds a new `GITTURTLE_TRACE` measurement, add the metric's exact name, its start/end boundary and what it excludes here, in the same style as the existing entries.

`gitturtle.theme_apply_frame_ms` (theme application from the Settings `choose_theme` handler through the next frame callback) is planned by the [`themes-apply-trace`](../development/themes/tasks.json) task; it does not exist until that task lands, so do not cite it as an implemented metric before then.

| Affected path | Harness and evidence boundary |
| --- | --- |
| Ordinary history paging | [bench-history-pagination.py](../../scripts/bench-history-pagination.py) and the release `history_pagination_bench` core example compare prefix and captured traversal over generated fixtures. Keep results outside fixture directories; see [reproduction](2026-09-10-history-pagination.md#reproduction). |
| Incremental graph pages | [bench-history-graph.py](../../scripts/bench-history-graph.py) measures extracted graph preflight/layout over an existing disposable history fixture; Git, parsing, output destruction, GPUI and native frames are excluded. |
| Review preparation and path filtering | [bench-app-review.py](../../scripts/bench-app-review.py) builds a release test executable and records CPU preparation and process memory with compiled-input hashes; it does not launch GPUI. |
| Comparison, worktree/recovery and passive LFS preparation | [bench-review.py](../../scripts/bench-review.py) measures core operations in disposable fixtures with source and fixture inventories; it excludes native presentation and transfers. |
| Passive Git process I/O | [bench-passive-pipes.py](../../scripts/bench-passive-pipes.py) compares an explicit baseline and current core source in an isolated fixture; it measures blob, changed-file and history reads, excluding native and memory evidence. |
