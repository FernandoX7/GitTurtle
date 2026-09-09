# GitTurtle design system

An original native desktop workspace for everyday Git work and reading history. Prioritize clear targets, legible relationships, effortless comparisons, and immediate input feedback. Midnight combines deep slate surfaces with a mint accent and remains the default; Braden is the original light palette. Graphite, Tokyo Night, Catppuccin Mocha, Nord, Porcelain, Sandstone, Deep Sea, and Ember provide distinct alternatives. Braden retains the `daylight` storage identifier and its original palette. Porcelain and Sandstone are additional light palettes; Deep Sea and Ember are additional dark palettes, originally designed for GitTurtle. Repository content fills the window; keep decorative charts, gradients, and oversized empty headers out of the working views. Projects and Settings can use grouped surfaces to make their choices easy to find.

This is the canonical current design contract. [The active consistency milestone](docs/consistency-milestone.md) owns acceptance status, screenshots, measurements and final build identities; design rules and implemented controls do not establish that a pending native check passed. [The former design page](docs/design.md) is a compatibility link, and dated evidence keeps its original names and build identities.

## Reference and provenance

GitKraken's first-party [interface guide](https://help.gitkraken.com/gitkraken-desktop/interface/) and [interface screenshot](https://help.gitkraken.com/wp-content/uploads/interface.png) establish the reference-navigation → graph → selected-commit hierarchy, adjustable panels, reference labels, and colored topology. Its [graph legend](https://help.gitkraken.com/wp-content/uploads/graph-elements.png) reinforces that commit and merge shapes should remain distinguishable. These are interaction references, not assets to reuse.

The first-party [diff guide](https://help.gitkraken.com/gitkraken-desktop/diff/) documents selecting a commit then a file, plus hunk, inline, and split presentations. GitTurtle adopts that familiar entry path and preserves the history context while the comparison occupies the central workspace. Researched September 7, 2026. The design below describes GitTurtle's implementation and explicitly marked future targets, not GitKraken's internals. GitTurtle has its own artwork and layout; curated theme colors are credited below.

The user supplied two further GitKraken screenshots on September 7, 2026 (`17.01.58` and `17.02.14`). The history reference uses full-height navigation, a separate branch/tag column, a central graph/table, and a right-side file panel. The comparison reference gives the code most of the window, collapses navigation to a narrow rail, and keeps changed files on the right. Those spatial relationships establish the history and comparison design below. Screenshot text and controls are reference data. The subsequent authorized scope adds explicit Git writes, Projects, and Settings while retaining passive browsing behavior. Six later GitKraken references emphasize discoverable Git actions, restrained type hierarchy, hover feedback, and spacing in a dense workspace.

The current first-party [profiles guide](https://support.gitkraken.com/gitkraken-desktop/profiles/) supplies the header-picker → manage/create/edit interaction reference. The [graph focus guide](https://support.gitkraken.com/gitkraken-desktop/hiding-and-soloing/) informs discoverable scope and clutter controls. GitTurtle uses its own named repository scopes, reference filters, adjustable columns and original artwork. These references were checked September 9, 2026.

## Shared control contract

Use the existing native Button and shared app helpers for actions. At the default interface size, the compact app helper sets 28 px height/minimum width, 10 px horizontal padding and a 6 px icon/label gap; named Git toolbar actions use 34 px height, and the profile picker and commit action use 36 px. Shared corners are 7 px, with a 12 px large-surface token. Icon controls use the complete allocated target and a specific accessible name. Keep hit areas, borders and padding fixed across hover, pressed and focus states.

These explicit app dimensions coexist with the pinned toolkit's rem-based input, switch, menu and direct Button geometry. Native spacing and controls scale with the 13 px root rem, while explicit app dimensions use interface-size/13. Do not assume that `.small()` alone applies the 28 px app helper. Use a small spacing rhythm, balanced padding and full-row hit areas; native validation checks the actual resulting targets.

Resting secondary controls use panel/subtle surfaces; hover uses the palette hover surface, pressed uses selected, and keyboard focus uses a visible accent ring. Selected controls keep a persistent surface and a checkmark or selected/expanded semantic state; hover remains distinguishable. Primary actions use their separate accent foreground and resting/hover/pressed tokens. Disabled controls dim and suppress activation; do not use a pointer cursor or hover-only promise of an available action. Every icon action has a specific accessible name. The pinned Button suppresses disabled activation, but its public GPUI element API does not expose a disabled accessibility flag. Keep the visible state, accessible name and nearby disabled reason honest; record the framework limit in the active milestone. VoiceOver testing and investigation are excluded from this milestone.

Inventory shared controls by workflow: page navigation and profile picker; Git toolbar and Targets; branch/worktree folders and rows; history/column controls and commit inspector; changed/working file rows and staging; comparison modes/Find/image/document tools; project actions and recents; settings cards/switches/size controls; command palette rows; review forms and confirmation/cancel buttons. Native review must exercise the rendered hover/focus/disabled states, not only palette values.

## Navigation transition table

| Event | Destination and retained context |
| --- | --- |
| Select a commit | Remain in History; load only metadata/files. Explicit file activation enters Compare. |
| Enter an inspection | Retain the previous comparison, exact targets, selection, query, scroll and focus in the bounded existing inspection chain. |
| Back / primary-[ | Close the top inspection first, otherwise return from Compare/Changes to retained History. At root History, open Projects; page Back returns to its named preceding page. |
| Escape | Dismiss the active dialog, menu, Find or other transient surface first. Return through inspections or the current page. At root History, clear its query and restore History focus. |
| Explicit History | Exit repository inspections and show retained History. It is not a clear-query, reset-columns, or discard-drafts action. |
| Explicit Projects | Show only the project hub, with no repository status/errors/previews or hidden editor focus. Retain the current repository for a named return action. |
| Reopen the currently retained project | Restore its existing mode, inspection chain, selections, queries, scroll, pane sizes, focus and drafts. |
| Choose a different repository | Initialize History for the newly resolved canonical worktree. Clear previous repository targets/results; load only the new worktree's own persistent drafts and identity. Appearance and column preferences stay global. |
| Settings | Retain the preceding page and repository context; Back names and restores that page. Repeated visits and appearance changes preserve unfinished fields. Identity fields initialize for a different canonical repository or explicit Reset; unedited fields follow effective Git identity. |
| Cancel a modal | Cancel replaceable reads, refuse late reopening, restore attached visible focus. Accepted Git operations use their separate explicit Cancel and are never replayed. |

Keep a destination-labeled Back action in the upper-left page navigation position whenever a return exists. Comparison content can also provide its local Back beside the path; both invoke the same one-level return. Reset controls name their exact scope, such as Reset column layout or Clear search. Page navigation and repository shortcuts must not act beneath a modal or on a hidden Projects workspace. Restore focus only after its visible destination attaches; late previews may retain content but cannot navigate or construct hidden editors. The [navigation contract](crates/app/docs/navigation-and-refresh.md) records the retained-state boundaries.

## Semantic palette ownership

[`appearance.rs`](crates/app/src/appearance.rs) owns all ten palettes, shared by native controls, custom drawing, and editor surfaces. Midnight uses a `#10151F` canvas, `#171E2B` panels, and `#75E0BB` mint accent. Braden uses soft white surfaces with evergreen actions. Subtle surfaces group content without competing with the selected row; primary actions have distinct resting, hover, and pressed colors with their own readable foreground.

Follow system selects Braden in system Light appearance; in Dark appearance it uses the selected dark theme, or Midnight when the saved selection is light. Theme application updates both native component color tokens and editor/drawing colors together. Appearance, density and independent text-size preferences remain global across profile changes.

The curated themes adapt the primary [Tokyo Night palette](https://github.com/tokyo-night/tokyo-night-vscode-theme), [Catppuccin Mocha palette](https://catppuccin.com/palette/), and [Nord palette](https://www.nordtheme.com/). Small-label contrast takes precedence over exact source values where needed, notably Nord's red and secondary text. These are native palette adaptations; the code editor retains its base syntax-highlighting rules while its background, foreground, gutters, and diff decorations follow the chosen theme.

File status combines a dedicated symbol, a short label, and semantic color in both history and working changes:

| Status | Treatment |
| --- | --- |
| New or untracked | Green file-plus icon; “New” |
| Modified | Amber file-edit icon; “Modified” |
| Deleted | Rose file-minus icon; “Deleted” |
| Renamed | Violet file-arrow icon; “Renamed” |
| Type changed | Amber type-change icon; “Type” |
| Conflicted working file | Warning-colored conflict icon; “Conflict” |

Keep addition/deletion backgrounds quiet in diffs and status tiles. Color never carries status or selection by itself. Graph lanes have stable identities and separate light/dark color sets; a row must not change color merely because the viewport moved. Palette tests check text and primary-button contrast at 4.5:1, status icons and graph lanes at 3:1, across their tested surfaces; native review still checks the rendered result.

## Typography and density

The dimensions below describe the default 13 px interface and 12 px code sizes. Settings independently adjusts interface text from 11–18 and code text from 10–24, with separate reset controls. Scale relevant row/control heights and editor gutters with their text; retain selection, focus and viewports when changing sizes. Larger sizes must keep the minimum 1,000 × 680 content window usable through appropriate scrolling.

- UI: system sans serif, 13 px regular; 13 px medium for selected labels and panel headings. macOS uses the platform UI font; Linux uses its available system sans family.
- Code, hashes, and aligned numerical data: system monospace, 12 px, with gutters and aligned overlays using the native editor’s measured line height. Code text must support selection and copying.
- Main repository title: 14 px semibold. Commit summary in the right inspector: 15 px medium, wrapping to two or three lines before explicit expansion. Avoid large display typography in repository views.
- Section labels: 11 px medium, restrained letter spacing, uppercase only for short headings such as LOCAL BRANCHES. No all-uppercase sentences.
- Spacing: use a restrained 4/8/12/16 px rhythm for explicit app dimensions; native rem-based spacing follows the interface size. Keep pane and card padding consistent within each surface. Secondary controls stay compact; prominent Git actions are 34 px high and the commit action is 36 px. The native control radius is 7 px; grouped cards use 10–14 px corners. Reserve pill shapes for small reference labels and counts.
- Comfortable history/file rows: 34/44 px; Compact: 28/34 px. Navigation rows remain compact. Use full-row hit areas. Keep metadata baseline-aligned and columns consistent. Column titles and text/reference cells share a 10 px leading inset; the graph uses a matching 10 px node margin. Preserve this geometry when resizing or scrolling horizontally.
- Icons: original 24-unit SVGs rendered at 16 px in rows and around 16–18 px in controls; status symbols sit inside quiet 26 px tiles. White source strokes allow GPUI tinting. Standard strokes are 1.65 units, rounded ends and joins. Keep actionable icons paired with labels or accessible names.
- App identity: [the layered Icon Composer source](assets/icons/README.md) supplies the native macOS appearances and the static default artwork in `assets/app-icon.png` and embedded 128-pixel `assets/branding/app-icon.png`. The turtle sits on navy in Default and system charcoal in Dark; Mono provides clear and tinted appearances. Repository and Projects headers and the collapsed sidebar share the default artwork. Small action/status controls remain SVGs so they stay crisp and theme-tinted.

## Workspace modes and hierarchy

Projects, Repository, and Settings are separate application pages. The repository has History, Compare, and Working Changes modes. History and Compare share the context and persistent right inspector below; Working Changes reuses the comparison area with a staged/unstaged inspector. At a typical 1440 × 900 window, the repository uses a 56 px page header, a persistent Git action bar with optional target fields, and a 26 px status strip. The workspace uses all remaining height. Browsing transitions retain the user's place; an explicit write refreshes the affected state.

Projects and Settings temporarily hide the repository workspace. Returning restores the retained editor or list focus, including an open editor Find field, selection, and viewport. Dragged navigation and inspector widths survive those visits and History/Compare transitions. File-history inspection and comparison modes remain available after returning. Theme changes refresh retained editor decorations without replacing the editors or their search state.

### Projects

Give recent projects a searchable list with repository names and readable folder paths. Keep Open, Clone, and Create together in a clear action panel. Open uses the native folder picker; Clone has a repository URL/local path, parent folder, and new folder name; Create adds the initial branch. Suggest a clone folder name without overwriting the user's edit. Use real destinations, field validation, nearby errors, and a busy state that prevents duplicate submission. Folder-picker cancellation leaves the form intact.

Provide Back to repository when a project is already open. At narrow widths, stack the action panel and recents; preserve scrolling and keyboard access. Larger headings and generous spacing belong here, while the repository view stays dense.

Search shows the matching count against all recents and offers Clear search when no projects match. Clone/Create show the resulting repository folder before submission; truncated destinations retain a full-path tooltip.

### Settings

Group appearance, independent interface/code text sizes, history columns, startup/default branch, and repository Git identity. The ten theme choices appear in light/dark groups with three columns, or two below the scaled 720 px viewport breakpoint. Each card has a native miniature workspace preview, swatches, a short description and an active checkmark. Density offers Comfortable and Compact. Apply changes across controls, previews, gutters, and selected rows, then persist app preferences outside repositories. Show failed saves clearly.

Column controls affect visibility and widths, preserve the commit-message column, and offer a layout reset. Git identity is a separate, explicit save for the displayed repository; show the current identity and signing state without implying that appearance settings alter Git configuration. Retain unsaved default-branch and identity text through page visits and repeated Settings commands. An unedited identity follows external effective changes; a changed canonical worktree initializes its own identity. Reset edits explicitly restores the current repository value. Named profile management is linked here and in the header.

Below 1060 px, stack the settings groups into one scrollable column. Theme cards expose hover feedback in their caption and mark the selected choice explicitly. Save/Reset reflect edited values, Return submits the focused form, and a failed preference write offers Retry save.

### History and Compare

| Region | History mode | Compare mode |
| --- | --- | --- |
| Left | 220 px repository navigation | 44 px compact navigation rail |
| Center | Full-height commit graph and table | Full-height code or image comparison |
| Right | 320 px selected-commit details and changed files | Same 320 px details and changed-file list |
| Mode entry | Opening a repository or Back to history | Clicking a changed file or opening it with Return |

The earlier graph-above-inspector split is superseded. Vertical space belongs to the current task, and the right panel supplies context throughout. Use shared 1 px pane divisions and flat surfaces. Navigation resizes from 180–360 px, and the right inspector from 280–480 px; keep the center flexible. Use the native pane resize handles and visible column dividers. Preserve chosen navigation/inspector widths across page visits and restart; retain horizontal column scrolling and give the flexible center the remaining space.

### Shared repository header

Show the turtle mark, repository name and path, Projects, History, a Changes tab with its file count, Git identity, and Settings. Refresh remains available through the primary-modifier shortcut and the working-changes control. Distinguish the checked-out branch from the history scope. Keep write actions named and tied to the visible repository/remote target; do not label the whole application read-only. Place commit search in History's scope toolbar rather than allowing a large global search box to compete with code-space controls. Header groups align with pane edges; use 12–16 px horizontal padding and 8 px control gaps.

The action bar is always visible in repository modes. Its current-branch menu, ahead/behind counts, and named Fetch, Pull, and Push buttons make routine work discoverable; Push uses the primary treatment. Targets are expanded by default, showing a branch field with Switch/Create, the remote, the remote branch, and the push destination. Collapsing Targets leaves the main actions visible. The branch menu lists the current branch and up to 40 other matching local branches; its find/create entry focuses the branch field for larger repositories. Menus are built when opened.

The branch picker also opens contextual branch settings, a searchable branch chooser, tracking-branch creation, merge/rebase preparation, and remote configuration. Branch settings expose rename, safe deletion, and upstream choices with the selected branch and worktree occupancy visible. Right-clicking a navigator branch opens the same management context; its ordinary click remains browse-only. Remote forms distinguish fetch and push URLs and do not contact a remote when saved. Show destination and integration consequences in a review dialog before writing; keep these occasional actions out of the permanent action bar.

### Named Git profiles

The 36 px secondary header picker combines an identity label, user icon and dropdown caret, with balanced native padding and a full-control hover/focus target. Truncate long labels within the 230 px control limit and retain effective name/email and configuration scope in its tooltip. An assigned profile that differs from effective Git settings is visibly marked **differs**.

The keyboard menu lists saved profiles and **Manage profiles**. Management supports named definition creation, editing and deletion; saving a definition changes app storage. Applying a profile is a separate review of the displayed canonical worktree and exact configuration scope. Disclose linked-worktree sharing, preserve global/inherited settings and existing signing requirements, and show errors without replaying an accepted write. Cancellation preserves recoverable form text. Deleting a definition leaves repository configuration, work and signing keys intact. Profiles contain author identity and optional public signing references; hosted-provider login and private-key storage remain separate. The [profile contract](docs/profiles.md) specifies persistence, scope and signing behavior.

### History mode

The left column contains All history, Local branches, Remote branches grouped by remote, and Worktrees. Sections collapse and show subdued item counts. Branch prefixes can form folder groups to make thousands of references browsable. A compact filter narrows this hierarchy without losing the active selection. Mark the checked-out branch with a small HEAD indicator; selecting a different branch only browses it. Worktree rows show folder name and branch, with full path and detached/locked/prunable state available in details or a tooltip.

The center begins with a 40 px scope toolbar, a separate compact search row, and a 34 px column header. Below it, the virtualized table fills the remaining height. Use these columns in this order:

| Column | Starting width | Treatment |
| --- | --- | --- |
| References | 140 px | Separate label column with compact reference badges; full name on hover |
| Graph | 112 px | Thin, stable colored topology with a shared lane coordinate system |
| Commit message | 320 px, expands into spare room | Primary 13 px summary text; never displace it with inline ref chips |
| Author | 136 px | Muted 11–12 px text |
| Date | 92 px | Muted compact date; exact timestamp in inspector |
| SHA | 84 px | Monospace; also available with copy in inspector |

Visibility and widths are user choices. Keep the commit-message column visible, preserve selected columns at narrow sizes, and provide horizontal scrolling when they exceed the viewport. Header and rows use the same computed layout and horizontal offset; divider drags persist widths after release. A wider graph must not squeeze many branches into indistinguishable overlapping nodes. Keep one parent/child relationship per graph edge, with stable lane colors across scrolling and paging. Use 1.6 px graph strokes, a 3.8 px ordinary node radius, a 4.3 px merge node, and a 5 px selected node. Merge nodes have hollow centers and a distinct ring. Reference badges can carry a restrained tint from their associated lane, while the selected row uses the active theme's selection treatment. Avoid saturated full-row branch bands and avatar-heavy topology.

Settings exposes graph lane spacing from 12–32 px, default 20 px, scaled with interface text. The graph column expands to the lane geometry's required width even when its saved minimum is smaller; the table scrolls horizontally and the saved column preference remains intact. A shared 10 px node margin and the same loaded-graph lane coordinates keep headers, rows and crossings aligned. Branch identity and topology are prepared on the worker and lists remain virtualized. Discontinuous search results show isolated nodes; an explicit bounded fallback must not invent edges through hidden or unavailable history.

Clicking a commit immediately selects its row and updates the right panel. It does not enter Compare mode. The right panel presents the wrapped commit summary, author, timestamp, copyable short hash, and parent/base selection above the changed-file list. Keep metadata compact, roughly 120–170 px for an ordinary commit, with the body behind an explicit Message expansion. The changed-file heading and its count remain visible while the list scrolls. A file row has the semantic status icon and label described above, a filename, and a muted parent path. There are no staging checkboxes. A highlighted file may be remembered, but comparison begins only on the user's file action.

Commit search reads beyond the loaded history prefix. Show its selected branch/worktree scope or All local refs, progress, and explicit Cancel/Continue controls. Search matches commit messages, author names, and hashes; it pins local reference tips so additional pages belong to the same search snapshot. A scan, time, byte, or retained-result bound must say that searching paused, rather than claiming no matching commit exists. Selecting a result pauses background search and opens its metadata through the usual selection path. Clearing search restores the retained ordinary history; Back from Compare retains the search and its results. Completed writes keep that scope and query in place. Restart explicitly includes new local commits and moved tips without silently broadening the search to All local refs.

File history is a contextual inspector for one path and anchor revision. Present its first-parent lineage and rename-following behavior explicitly, with Newer/Older revision controls and paged navigation. Each revision retains its actual old/new path and absent sides. Selecting a revision displays its comparison without discarding the surrounding repository history or working selection; Back restores that context. Rename detection and lineage traversal remain bounded background work, with useful errors when a limit prevents reading deeper history.

### Compare mode

Clicking a changed file replaces the central history table with its comparison and collapses the left column to a 44 px rail. Keep the right inspector and changed-file list in the same location, with the active file visible. Switching files replaces the central content immediately without leaving Compare mode. Use a visible Back to history control with a left arrow at the top of the comparison; the compact rail also provides a labeled history-return action.

The comparison header is 40–44 px high and contains the path breadcrumb, copy-path action, and the relevant view controls. Text uses Diff / Split / Before / After; images use Fit and zoom controls. Give the remaining central width and height to content. A small secondary line can show object identity, file mode, or image dimensions when useful. Keep implementation details out of the main reading area.

Back to history restores the existing graph, selected commit, reference scope, search query, loaded pages, column widths, and scroll position. Restore keyboard focus to the selected history row without jumping to the top. Keep the right panel's selected file and scroll position. Preserve the selected file path across adjacent commits when it is changed in both; otherwise select the first file in the right list without forcing a mode transition.

At narrower windows, History can collapse its navigation to the same rail while its chosen columns scroll horizontally. Compare keeps its 44 px rail and right file list while the preview takes the remainder. Right-panel resizing remains available in either mode. Keep dragged pane sizes in the running workspace even when a page temporarily hides those panes.

### Working Changes

Separate staged and unstaged groups, with whole-file Stage/Unstage actions and clearly named all-files actions. A file with both kinds of edits appears in both groups; selection determines which comparison is shown. Keep a persistent “Create a commit” composer below the file groups, with the current branch, staged count, distinct Title and optional multiline Description fields, and full-width “Commit N files” action. The title counter gently flags summaries beyond 72 characters without rejecting them. Preserve title and description formatting in Git, persist each canonical worktree’s draft across restarts, and keep it on failure. Clear only the draft that matches a successful commit. Identify missing Git identity near the composer. Unified text previews place hunk actions and changed-line selection beside the relevant lines, with a temporary selected-lines action at the bottom. Unsupported partial changes retain whole-file actions. Conflicted files show both named sides and an optional base, with a focused manual-resolution editor, complete-side choices and an external-editor handoff. A pending-operation banner provides explicit Continue, Abort and Stop-and-keep-files actions; ordinary commit creation resumes after the operation ends.

Fit the composer to the space remaining beneath repository controls and feedback. Keep a usable file list and the commit button visible; scroll extra composer fields, guidance, and errors inside their own area. Resizing the window or changing density keeps the selected working file in view, including after returning from Settings.

Git actions show the current branch, remote, and destination branch. Switching and creating branches are explicit actions; clicking a branch in the navigator continues to browse only. Keep the familiar Pull label and state its fast-forward-only behavior and exact source/destination in the tooltip. Push targets the shown branch without force. Disable duplicate writes while an operation is running, show its outcome close to its origin, and refresh local state afterward. A timeout or missing result must explain that the effect may have occurred and require inspection before a deliberate retry.

Conflict resolution identifies the current branch and incoming operation side, including rebase's changed side meanings. Show an absent side explicitly for delete/modify conflicts. Retain manual text when selecting another conflict, changing views, or receiving an unrelated local refresh; a changed conflict identity requires reinspection before applying a resolution. Saving a manual result and staging a resolved file are deliberate actions. If stash restoration leaves conflicts, the saved stash remains available and resolution returns to ordinary Changes; do not invent a stash Continue or Abort operation.

### Pausing and recovering work

Keep recovery commands beside their targets: the Changes inspector's Actions menu holds stash save/browse and current-tip actions; selected commits expose an Actions menu and right-click commands. Avoid a universal Undo control. Each consequential action names the captured repository, branch, commit or stash, affected paths, and expected history or index changes before confirmation. If those inputs change, require a fresh plan instead of redirecting the old action.

Named stash creation offers an explicit Include untracked files choice. The stash inspector separates saved staged, unstaged, and untracked files and loads content only for an explicitly selected file. Its bounded, paged stash list and file list support keyboard navigation. Restoration offers an explicit Restore staged state checkbox; explain whether Git will attempt the original index boundary. Restoration always keeps the stash, including on failure or conflict. Drop is a separate confirmation that removes the saved entry without changing current working files.

Failed restoration should say whether conflicts were found and whether the reviewed stash is still saved, using fresh local evidence. If either check is unavailable, state that uncertainty. Direct conflict recovery to Changes and staging; keep Git's complete diagnostics in Details without suggesting a stash Continue/Abort action or automatically retrying.

Amend opens distinct Title and Description fields initialized from the current commit. Unedited messages retain their exact bytes; edited messages use the same conventional separator and formatting preservation as ordinary commits. Show the currently staged path count separately from all paths in the replacement commit and retain edited fields after failure. Locally known remote-tracking reachability prompts a history-rewrite warning. Undo last local commit uses a soft reset: index and working files stay intact and the undone commit's changes remain staged against its parent. Restrict it to the safe local-tip case supported by the backend. Revert and cherry-pick identify the selected commit; merge commits require an explicit mainline-parent choice. Their conflicts use the existing operation banner and resolution flow.

### Status strip

Use the bottom strip for local refresh state, loaded/visible history count, selection context, and concise keyboard hints. Show elapsed timing only when measured. No permanent green badge should imply an unmeasured performance guarantee. Keep this strip visually quieter than all reading surfaces.

## Selection, focus, and feedback

- Selected row: semantic selected background, 2 px accent leading marker, primary text. Hover: semantic hover background; selected rows retain their tint with a subtle accent blend. Selected controls preserve their surface with gentle hover feedback. A selected node has a contrasting ring. Selection remains visible after focus moves to another pane.
- Keyboard focus: a visible 1 px accent outline or inset outline on the active control/pane. All controls require accessible labels; color or tooltip is insufficient as the only label.
- Input response is immediate. In History, commit selection updates the right heading before changed-file work starts. A file action enters Compare immediately and sets the path heading before preview work starts. Load uncached metadata, changed-file lists, diffs, highlighting, and images in separate stages. Returning to History uses retained state and must not wait on an active preview request.
- Never display an old commit's diff under a new commit's header. On selection, clear or explicitly mark the pending preview; late results for previous selections cannot replace the current one.
- Loading: a small local progress label while background work remains pending. Skeletons, if used, occupy the exact content area and remain static. Avoid whole-window loading overlays and decorative shimmer. Keep navigation interactive and cancel obsolete requests.
- Empty repository: “No commits yet” in History, with Working Changes available for staging a first commit. An exhausted search can report no matches; a paused bounded search must retain its progress and Continue action. Clear search restores the preceding selection context. Empty commit: the right file list says “No file changes against this parent.” Empty remote/worktree sections have concise labels rather than failure styling. An absent image side explicitly says Added image or Deleted image.
- Errors stay close to their origin, with a plain explanation and retry when meaningful. Missing repository: show the path and Open repository without old repository labels under the new path. An unavailable blob/image remains in Compare with its file list and Back to history active. Oversized text/image: show size and a bounded preview or explicit unavailable message; do not freeze or silently truncate content.
- Refresh means reread local Git state. Describe remote-tracking references as locally available; never imply refresh fetched from a server. A status timestamp should explicitly mean last local refresh.
- Local filesystem and Git metadata notifications, plus window activation, request quiet refreshes. Coalesce bursts and defer work behind foreground reads or writes. Keep selection, scroll, search, editor focus, and manual conflict drafts stable; leave unchanged previews intact and refresh changed mutable content in its existing editor. A missing scoped branch retains the displayed context with an explanation. Watcher errors leave explicit Refresh available. Never start network operations or repository writes automatically.
- Authentication, SSH, credential-helper, hook, and signing failures preserve Git's original diagnostic and add a relevant next step. Long details belong in a scrollable operation-details view. Do not silently retry a write whose result is uncertain.
- Motion: optional 80–120 ms hover/focus transition. No selection animation, animated graph reflow, or spring scrolling. Honor reduced-motion preference.

## Text and image comparison

Text opens in the user's active Diff / Split / Before / After mode, defaulting to unified Diff for a new session. Make added/deleted lines explicit with signs and background tints; keep hunk headers blue and visually separate. Preserve indentation and literal patch text for copying. Source views show line numbers. Unified patches show separate old/new line numbers without inserting them into the copied text. Split diffs align changed rows with clear Before/After headings and synchronized vertical scrolling. Blank alignment rows are excluded from copied source text. Preserve each mode’s selection and viewport when switching modes. Respect horizontal scrolling for unwrapped lines. Word wrap, if exposed, is a user control rather than an implicit width-dependent change. Highlighting may arrive after readable plain text, without reflow. Build only the currently displayed editor mode.

Group Diff / Split / Before / After as one compact mode control. Working previews identify Staged or Unstaged beside the path so two versions of the same file remain unambiguous. Partial-staging selection survives a view-mode switch, while a changed working snapshot clears stale line selections. Split scrolling follows either side, including its gutter and Find navigation, without a feedback loop between editors. File rows and comparison paths provide full-path tooltips; keyboard hints follow the current page and workspace mode.

Find matches use a selection tint in place of the patch background while preserving source text colors; an accent underline identifies the active match. This treatment remains stable when the viewport changes in every text mode. Closing Find restores the underlying diff treatment without replacing the editor or losing its viewport. Opening Find is explicit; preparing its decoration handle must not create hidden inputs or steal focus.

Image comparison defaults to side-by-side Before/After on a subtle checkerboard. Show dimensions and byte sizes beside the labels, with Added/Deleted state replacing the absent image when appropriate. Fit is the default; 100%, zoom in/out, and fit controls are small and familiar. Link zoom and pan across both images. Overlay and Wipe share the same source coordinates and linked pan/zoom, with keyboard controls for opacity and divider position. Preserve aspect ratio, avoid upscaling by default, and keep checkerboard contrast below the artwork. SVG is a static image; never execute embedded content. A Git LFS pointer without available local content must be identified as such rather than presented as a broken image.

The transparency grid uses the current palette's canvas and subtle surfaces, including Braden, and image metadata truncates within its own pane. Missing LFS text or image content offers a separate reviewed download that names the selected object, source and known size. Keep transfer progress/cancellation visible and explain tooling, credential, unavailable-object and integrity errors. Browsing remains passive; decoding starts only after verified local content is available. See [LFS preview semantics](docs/lfs-previews.md).

### Documents, media and exact source

The [file-preview support matrix](docs/file-previews.md) is the source of truth for detection, real rendering, source/metadata fallbacks, limits and platform requirements across History, revision comparison, Quick Open and Working Changes. Extend these entry points through the same captured Before/After identity model.

BMP/TIFF/ICO use the image comparison surface; TIFF and animations remain a single selected image/first frame. AVIF and HEIF/HEIC use compatible macOS codecs and show an explicit platform/decoder limitation elsewhere. PDF uses independently labeled Before/After page controls, total counts and the visible first-eight-page cap; Quick Open gives its single source the available width. PDF comparison is visual and has no text-staging actions. Keep white document pages separate from themed chrome and checkerboards.

Media, Office/OpenDocument, archives and fonts present useful bounded metadata with an explicit system-preview action for supported captured bytes. Label metadata as metadata. Preserve literal source access for Markdown, HTML, CSV and ordinary code; repository HTML/scripts/macros are not executed. Decoded UTF-16 is identified as decoded text and cannot become a Git patch. SVG retains literal source copying even when rendering is refused. External preview opens a controlled captured-byte copy, never a substitute working file; missing sides and errors stay visible. Do not count external viewing, container recognition or a generic unavailable notice as in-app rendering.

## Command palette

**Primary-Shift-P** and **View → Command Palette** open a native, focused command search; **Primary-P** remains Quick Open. Search names and useful synonyms from the bounded existing-workflow registry. Rows show shortcuts and application/repository/selected-file scope; unavailable actions stay discoverable with a reason. Highlighting is inert. Return or a click revalidates the target and opens its existing workflow/review once; Escape/Cancel restores the preceding visible focus.

Use 54 px scaled rows, an adaptive one-to-seven-row result height, a compact empty-result explanation and a viewport cap at large text sizes. Up/Down moves selection and keeps it visible. Preserve query editing shortcuts, consume background navigation shortcuts and avoid opening over another active modal. The [command-palette contract](docs/command-palette.md) owns search bounds, availability and target validation.

## Keyboard contract

Use `Cmd` on macOS and `Ctrl` on Linux for the primary modifier. The [README](README.md#keyboard-controls) and native Keyboard Shortcuts help list implemented bindings. Keep this table, menus and action availability aligned with the current page; do not advertise unavailable actions in the application.

| Binding | Action |
| --- | --- |
| Primary + O | Open repository |
| Primary + Shift + O | Open Projects |
| Primary + P | Quick Open a tracked file in the shown repository worktree/revision scope |
| Primary + Shift + P | Open the command palette |
| Primary + 1 | Return to retained History |
| Primary + Shift + C | Compare local revisions |
| Primary + Shift + A | Open local GitTurtle operation activity |
| Primary + 2 | Open Working Changes |
| Primary + , | Open Settings |
| Primary + F | Focus commit search in History; use editor find while reading text in Compare |
| Escape | Dismiss active transient UI first, then return through inspection/page context; at root History clear its query and focus History |
| Primary + [ | Return through inspection/page context; Compare/Changes return to History, root History returns to Projects |
| Up / Down | Select previous/next row in the focused list |
| Home / End | First/last loaded row in the focused list |
| Page Up / Page Down | Move through the focused list by viewport |
| Left / Right | Collapse/expand a focused navigation section |
| Return | Open the highlighted file in Compare from either history or the file list |
| Option/Alt + Up / Down in text | Previous/next change |
| Option/Alt + Up / Down in the rebase sequence | Reorder the selected commit |
| P / R / S / F / D in the rebase sequence | Pick / Reword / Squash / Fixup / Drop |
| Primary + Option/Alt + Up / Down in conflicts | Previous/next unresolved block |
| Primary + click / Shift + click in Working Changes | Toggle a file / extend the selection range |
| Shift + Up / Down; Primary + A in Working Changes | Extend the range; select visible file rows |
| Tab / Shift + Tab | Traverse controls and panes in visual order |
| Primary + R | Refresh local repository state |
| Primary + C | Copy selected text; explicit copy buttons handle commit hash/path |
| Primary + B | Toggle expanded repository navigation in History |
| Primary + Shift + / | Open Keyboard Shortcuts |

The graph and file list must scroll the selected row into view during keyboard navigation. Search keystrokes should not trigger list navigation. Native text selection/copy behavior takes precedence in diff content. Escape in an editor find box closes that box before a subsequent Escape returns to History. Back to history preserves the history query; it is not a Clear search action. Primary + Q retains platform quit behavior.

## Review checklist

Choose checks for the affected surface. History/comparison checks use merges, long paths, non-ASCII text, large commits, and relevant image/missing-content cases. Exercise commit → file → another file → Back, including a repository-wide search, a scrolled graph, file-history pages, and a rename boundary. Verify focus, Escape precedence, stale-preview rejection, pane resizing, chosen-column alignment, and clipping at regular/narrow widths in each affected theme. Check Split scrolling and Find from both sides and literal selection/copy across alignment blanks. Project, partial staging, commit, branch, recovery, and network tests use disposable repositories and local remotes; check exact destinations, stale plans, failures/conflicts, and unrelated staged/unstaged content. Settings checks include reopening, failed persistence, identity scope, and retained editor focus. External file/Git changes must refresh without disturbing the current interaction. Record the build and observed results separately; a screenshot is not performance evidence.

## Platform appearance and evidence

The [everyday feature semantics](docs/macos-features.md) describe attribution, source-aligned image overlay/wipe, system appearance, native menus and local editor/Finder handoff. [Tags and contextual ignore](docs/macos-git-actions.md) use captured review dialogs and the existing operation feedback. [Authentication](docs/authentication.md) preserves configured Git mechanisms with transient native prompts and cancellation. Projects uses its own single header; repository context returns through Back.

The [Liquid Glass prototype](docs/liquid-glass-investigation.md) exposed a pinned-renderer compositing limitation. Current navigation stays opaque in all ten themes; no ordinary transparency is labelled native glass. The [earlier macOS milestone](docs/macos-milestone.md) retains its historical findings. The [active consistency milestone](docs/consistency-milestone.md) records the current native review, including presentation/focus corrections and remaining checks, against actual builds.

## Review and recovery controls

The current-branch menu and native View menu expose occasional comparison, Quick Open, activity, worktree and reflog workflows. Keep explicit Before → After labels and resolved commit identities visible. Since-branching comparisons identify the merge base; unavailable or ambiguous ancestry needs an actionable explanation. Quick Open presents a tracked source file without synthetic Added/Diff status and returns to its preceding inspection. Both modal searches accept keyboard input immediately, suppress duplicate acceptance, cancel obsolete reads and restore destination focus after closing.

Diff review controls remain together above text: whitespace suppression, bounded context, reset, and previous/next change. Active review variants explicitly disable partial staging; code/source copying stays literal. Interface and code sizes have independent reset controls in Settings. Scale text, gutters and relevant rows together while preserving each pane's scroll and selected content.

Working Changes names the count and area of selected files. Command-toggle and Shift-range selection use visible paths. Filtering/grouping clears selection predictably; all-files actions are disabled while a path query is active. Optional directory labels help large lists without creating implicit selected subtrees.

Worktree creation/removal and recovery branch creation review their exact target and consequences. Distinguish worktree removal from branch deletion and report retained branches after partial checkout failures. Activity shows local operation outcomes and controlled concise explanations, with explicit next actions and no automatic retry. See [parallel work and reflog recovery](docs/parallel-work-recovery.md).

[Conflict blocks](docs/conflict-blocks.md) expose current/incoming/base meaning, unresolved counts, navigation, both-side choices and a manual result. **Save draft** leaves the index conflicted; **Save and stage result** requires a reviewed result without recognized unresolved markers. [Interactive rebase](docs/interactive-rebase.md) reviews an exclusive base and at most 100 linear commits, with native message editing and focused keyboard reorder/action controls. Label the stable rebase base separately from the replayed original commit. Git message pauses and conflicts remain distinct, and the interface must explain unsupported histories and uncertain intermediate outcomes.

Unfinished conflict-result and rebase-message drafts use visible pending/error feedback and explicit restoration after their captured source identities are revalidated. Stale drafts keep recoverable text without blindly applying it. [Conflict drafts](docs/conflict-blocks.md) and [rebase messages](docs/interactive-rebase.md) specify the respective flows. The [rewritten-series review](docs/rewritten-series.md) shows original/replayed correspondence and ambiguity before a separate publication review. Publication names one remote branch and reviewed old/new OIDs with an exact lease; completing local rebase never publishes automatically.
