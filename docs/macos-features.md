# Everyday feature semantics

## Blame and line history

Open a text file in Compare and choose **Blame**. Rows show a source line, its originating commit and author. Arrow keys, Home and End select lines; Command-C copies the selected line's exact source text. **Compare commit** (or Return) opens that line's originating commit and the existing rename-following file-history comparison. Back restores the attribution, selected line, scroll position and preceding comparison. Explicit repository, history scope, refresh and write actions exit these transient inspections. Up to four nested attribution and file-history contexts are retained.

Committed attribution uses the chosen immutable commit and follows Git's whole-file rename detection across all merge parents. Working attribution reads a bounded raw file snapshot and maps unchanged lines to HEAD. Both staged-only and unstaged edits are shown as **Uncommitted**; it is not an index-only view. New files and files in unborn repositories have uncommitted attribution. A staged rename keeps attribution through its original HEAD path. Missing/deleted files, conflicts, binary or non-UTF-8 text, symlinks and submodules show explicit unavailability. Stored symlink targets are never followed. Copy detection and moved-line tracking across files are not attempted.

**History of line** traces the selected committed line from its originating revision using Git's line-range history, following first parents and rename heuristics. Other merge-parent lineages and cross-file copies are excluded. It lists at most 100 changes and explicitly marks a full result as limited; hashes can be copied. Uncommitted lines have no committed line history. Shallow repositories show a boundary notice because attribution stops at locally available history. Other unavailable objects produce a Git error; neither case triggers a fetch.

Attribution and line history use the existing one-active, one-replaceable-pending worker. Cancellation terminates obsolete Git process groups; repository identity, target, selection and generation checks reject late results. Local automatic refresh defers while attribution is visible. Text is limited to 2 MiB and 100,000 lines, porcelain attribution output to 32 MiB, line-history metadata to 8 MiB, and each Git read to 15 seconds. Working reads reuse safe descriptor-relative traversal and have the core's 64 MiB raw input ceiling before the tighter text check; line matching has a 250 ms budget. Unmatched lines are conservatively uncommitted. No attribution read runs filters, textconv, external diff, network access or an index refresh.

Behavioral fixtures in [`crates/git-core/tests/blame.rs`](../crates/git-core/tests/blame.rs) cover committed origins, exact CRLF/no-final-newline source, renames, working staged/unstaged lines, unborn/untracked files, byte-safe tree paths, line-history filtering, text limits, cancellation, symlink refusal and unchanged index/worktree/filter markers. Native evidence and final build coverage belong to the milestone validation record; fixture success alone does not establish native behavior.

## Image comparison

Side by side, Overlay and Wipe share a common source coordinate system. Overlay adjusts After opacity; Wipe reveals Before on the left and After on the right with a draggable divider. Both also expose focusable decrease/increase/reset controls. Fit never enlarges the decoded comparison; 50%, 100% and 200% scale its bounded preview. The control strip states how 100% maps to source pixels, and each side retains original and decoded dimensions. If one original is larger, independent thumbnail reduction does not erase that size difference.

Both versions pan together by dragging, scrolling or the focusable directional buttons. Their top-left source coordinates stay aligned. Transparent regions use a checkerboard; absent or unavailable sides stay absent and the available side is shown at full opacity. Preview decoding limits and first-frame behavior are unchanged. The GPU composes existing images on interaction; dragging does not decode or allocate a new pixel buffer. Navigation retains comparison choice and position but never a live drag gesture.

## macOS conventions and appearance

The native menu bar groups File, Edit, View, Window and Help commands. Repository actions reflect availability and operation busy state. History and Working Changes use Command-1/2; Open, Settings, Refresh, Back, Hide and Minimize retain standard macOS shortcuts. Help → Keyboard Shortcuts lists navigation and image controls. Projects has one page header, with explicit progress/cancellation during clone or create.

Follow system appearance chooses Daylight for Light Mode and the selected dark palette for Dark Mode (Midnight when the manual choice was Daylight). Manual palette selection disables following. All six themes and both densities remain available; appearance updates refresh existing editor decorations without replacing their contents, selection or Find state.

File → Reveal Repository in Finder opens the captured current folder locally. Settings → Projects → External editor stores an application name or path, and File → Open Repository in Editor launches that editor with the captured repository folder. Arguments are passed directly without a shell. macOS launcher failures have an actionable message and a ten-second deadline. Linux uses the configured executable; the external editor owns its ongoing application lifetime. No repository write or network action accompanies either handoff.

The [Liquid Glass investigation](liquid-glass-investigation.md) records the native experiment and compositing limitation. The finished application retains opaque theme surfaces; it does not label ordinary transparency as Liquid Glass.
