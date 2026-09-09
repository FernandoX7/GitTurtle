# Navigation and refresh contracts

Read this when changing or reviewing page/mode transitions, focus, selection, worker scheduling, search, retained inspections, or local refresh. Source ownership is mapped in [app guidance](../AGENTS.md); the broader flow is in [architecture](../../../docs/architecture.md).

## Selection and asynchronous replies

Commit selection in History requests `Job::Changes`; a highlighted file is not an activation. Explicit activation enters Compare. `receive(Output::Preview)` may retain current content after Back, but cannot reopen Compare or initialize a hidden editor. Preserve generation checks and both the `AppPage::Repository` and workspace-mode guards when changing asynchronous code.

Back from Compare restores the retained scope, query, loaded history, selection, scroll position, and inspector without another Git request. Working Changes keeps history files separately in `retained_history_files`; do not reuse mutable working files as the history inspector. History-changing writes use `refresh_after_write` to queue a quiet refresh, preserving the valid selected scope, pinned query, and immutable comparison. Re-resolve mutable branch/worktree identities; session reuse must not freeze tips. Parent changes keep the displayed comparison and selected file consistent. Rebuild selections from stable commit/file identity rather than virtualized row indices.

Working-status replies check both `work_generation` and canonical repository identity. Refresh and writes use `invalidate_read` to advance the preview generation and cancel obsolete work when mutable content is invalidated, even when the next selection is empty. Retain the user's current path/area at reply time, including movement between staged and unstaged groups; do not restore the selection captured at dispatch. Mutable previews bypass the immutable cache.

## Pages and focus

Projects and Settings are pages separate from the retained repository modes. Construct elements only for the active `AppPage`; calling a hidden page renderer still builds its lists, controls and previews. Keep the root `app_focus`, history `focus`, and `file_focus` handles distinct. Projects and Settings focus the root so application shortcuts remain reachable. Keep `GitTurtleList` navigation out of editable form fields; hidden editors must not steal focus. Retain the selected file's visibility as lists change. Temporary search expansion must not overwrite saved branch-folder expansion choices.

Capture the current focus handle when leaving Repository for Projects or Settings and restore it once after the retained content is visible. Preserve an editor or Find field's exact focus rather than substituting generic file-list focus. Keep file-history editors alive across these page transitions; opening another repository clears the old return focus. Theme changes update retained file-history and split-editor decorations in place. Retained inspection state must not retain live drag gestures.

## Search and retained inspections

Repository-wide search pins scope tips across pages, shares the replaceable read worker, and propagates cancellation to the core history process. Retain the ordinary history separately and restore it on clearing search. Foreground commit/file activation pauses search without discarding its results or cursor. Keep Restart explicit so users can search new commits/current tips after a write while quiet refresh preserves the captured search. An explicit History Refresh also restarts scope. Distinguish exhaustion from page, scan, time, byte, and UI-retention bounds; never present a bounded partial scan as an exhaustive no-match result.

File history uses an explicit path and immutable anchor, preserves rename paths/absent sides, and restores the preceding repository context on Back. Its first-parent lineage and pagination limits stay visible. Attribution and line history also use bounded worker reads and check repository, target and selection generations. Attribution owns a separate accessibility focus handle. Nested inspections restore the preceding comparison and line selection; explicit repository/scope/write transitions unwind them.

## Local refresh

Filesystem callbacks only enqueue bounded local events. Resolve actual private/common Git directories and register watchers outside the UI thread; do not poll repository trees or invoke Git in the callback. Coalesce bursts, ignore passive access/lock noise, and defer quiet reads behind active previews, search, file-history inspections, status work or writes. Preserve refresh epochs and selection/status generations so superseded results cannot apply.

Quiet snapshots update local metadata without navigating away, clearing search, or replacing an unchanged immutable preview. Refresh changed mutable editors in place and retain manual conflict drafts by conflict identity. A vanished scope keeps the displayed history with an explanation. Watcher errors surface with manual Refresh available. Neither a filesystem event nor focus regain may trigger a write or network action.
