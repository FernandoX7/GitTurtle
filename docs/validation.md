# Validation notes

Use the [current validation guidance](#current-validation-guidance) for the affected workflow and the canonical [design contract](../DESIGN.md) for expected behavior. Dated records below describe their identified builds; the completed [security, architecture and resource milestone](security-quality-milestone.md), [native polish](native-polish-milestone.md), [review and recovery verification](review-native-verification.md) and [macOS milestone](macos-native-verification.md) remain historical evidence. New development tasks and their acceptance requirements belong in the [development workflow](development/README.md).

This page contains current validation guidance and dated local evidence, with each completed stage tied to its exercised source/build. The September 7–8 records below cover earlier history, design and everyday Git workflows; the September 9 backend report covers its recorded review-milestone inputs. Native workflow evidence is primarily macOS-specific; the September 14 entries add Pop!_OS startup and clean Ubuntu/virtual-native checks. Platform execution and access limits belong to the applicable dated record and [platform runbook](linux.md); the earlier [environment report](benchmarks/2026-09-09-milestone-environment.md) describes its own session. The configured [quality workflow](../.github/workflows/quality.yml) alone is not evidence of hosted CI execution. Public binary release prerequisites belong in the [launch checklist](public-launch.md); the Linux runbook includes a local teammate bundle.

## Current validation guidance

The dated records on this page apply to their named builds and environments. They do not establish native or package coverage for later source changes. Use the [current feature overview](user-guide.md#current-source-features), [architecture and bounds](architecture.md), [preview matrix](file-previews.md), [profiles](profiles.md), [command palette](command-palette.md) and [rewritten-series review](rewritten-series.md) for implemented behavior. The completed [security milestone](security-quality-milestone.md), [macOS milestone](macos-milestone.md) and [everyday-work record](everyday-work-plan.md) remain evidence for their identified inputs, rather than an active feature queue.

For changes to the current workflows, use disposable repositories and local remotes for mutations, and select the relevant checks below. Record source revision and relevant dirty-input identity, executable/package identity, target/profile, OS, display backend and scale. Compare `--build-info` with the exercised artifact and independently inspect Git results; a control or passing core fixture does not by itself verify native interaction. A visible change's captures, comparison, design review and privacy check follow the [visual evidence tiers](../.agents/skills/gitturtle-native-qa/references/visual-evidence.md). Required evidence belongs to the current [task contract](development/README.md#task-contracts-and-ownership), with new results recorded under their actual date/build.

| Current workflow | Relevant validation |
| --- | --- |
| Repository tabs and local workspaces | Open a new repository after a search, verify independent inputs, canonical alias deduplication and linked-worktree identity, eight-tab bound, pin/group/reorder/close, captured in-flight writes, draft recovery and lazy restart bookmarks. Exercise moved/missing paths and explicit picker recovery. |
| Projects and repository opening | Open/cancel the native picker, search/clear recents, clone from a local remote and create an unborn repository. Check paths with spaces, nonempty destinations, missing/moved repositories, picker/tool failures, duplicate submission, retained form input and return to the captured repository. A failed open must preserve the prior worktree's outcome and cannot apply late content from another repository. |
| Persistent app state and shutdown | Use disposable app stores for supported-version migration, corrupt/unsupported/nonregular stores, capacity and failed-save paths. Verify visible Saving/Saved/error feedback, exact commit/conflict/GitHub draft text, tab/order/bookmark retention and normal quit/final-window close followed by restart. Preserve invalid originals and newer edits after a failed older save; a Git write followed by app-store failure must not replay Git. Follow the [persistence contract](../crates/app/docs/writes-and-persistence.md#preferences-and-commit-drafts): confirmed Saved is the durability boundary for slow I/O or forced termination; normal-shutdown evidence does not establish crash-time saving. |
| Incremental ordinary history | Page across the 5,000-row/64 MiB window with an older selection retained; verify stable OIDs, connected graph frontier and native Older/Previous/Latest behavior, top following without selection changes, and the Show latest cue while browsing older rows or Compare. Inspect slim and lane-overflow graphs, rapid selection, cancellation and search discontinuity. Use the [120k fixture measurements](benchmarks/2026-09-10-history-pagination.md) for backend comparisons and separate native callbacks. |
| Interactive model comparison | Exercise pointer and keyboard orbit/pan/zoom/fit, standard views, linked and independent cameras, edges, orientation, units, missing sides and captured originals. Compare GLB material-only and texture-only revisions, static skins/morphs, clip selection, Play/Pause, scrubbing, different durations and Reduce Motion. Check fixed cameras and current-pose Fit, malformed appearance fallback, paused/hidden quiescence, rapid activation/Back, tabs and closure. Compare changed transforms/scale, absent and unsupported sides in History and Working Changes; retain separate path filters through navigation and refresh. Compare curved analytic and mapped STEP fixtures within the [finite support matrix](file-previews.md), then repeat opens/tab changes/window closure while observing resource retirement. |
| PDF and rendered Markdown | On a supported platform, navigate PDF beyond page 8 with entry/previous/next, unequal counts, linked positions, zoom, extracted text and literal copy; retain state across Back/tabs/restart and evict bounded cached pages. Check native Markdown prose/tables/code/Mermaid, revision-correct local images, explicit local/external links, linked scrolling, keyboard reading and exact source staging. Verify explicit unsupported-format feedback against the [platform limits](linux.md#platform-limits), without treating metadata recognition as rendering. |
| GitHub collaboration | Open/close the offline native panel through the palette; check keyboard activation, visible composers, confirmations, refreshed PR and recovery lists at narrow/wide sizes, both densities, larger text and light/dark themes. Reply, resolve/reopen, page authoritative conversations and retain exact drafts through navigation, tabs and restart. Check stale account/head/thread identities, missing context, permissions, rate limits, partial failures, persistence failures, cancellation and uncertainty. Real connection, PRs/comments/reviews and hosted CI require specifically authorized disposable context; record independently verified live results and credential access separately. |
| Precise staging and commits | Exercise hunk and changed-line stage/unstage with mixed index/worktree edits; verify unrelated index entries and working bytes. Check whole-file fallback explanations, exact Title/Description bytes, hook/signing failures, and worktree-specific draft retention through navigation and restart. Exercise Discard changes and Delete untracked file from a working row's context menu on modified, staged, renamed, added, deleted and untracked rows: verify the review text, the stale refusal after a later edit, restored HEAD content, the deleted untracked file, and untouched sibling changes. |
| Conflicts and integration | Inspect base and both named sides, rebase labels, manual and complete-side resolution, external edits, stale-save refusal, and editor handoff. Verify Continue's staged-path review, external operation detection, Abort preservation, and Keep files without losing HEAD/index/worktree state. |
| Stashes and commit recovery | Inspect staged/unstaged/untracked saved content; restore with and without staged state; confirm the stash survives success and conflict until an explicit Drop. Check amend, eligible Undo, revert/cherry-pick and merge-parent choice, including stale targets, failures, and independent work. |
| Branches and remotes | Review actual switch/create/integration targets, invalid rename names and destination collisions, tracking/upstream changes, safe deletion, and linked-worktree occupancy. Verify remote configuration separately from explicit fetch/pull/push. |
| Named profiles and identity | Follow [profile semantics](profiles.md): create/edit/delete definitions without changing Git, then review and explicitly apply the actual author/signing settings and shared/private worktree scope. Check retained editor text, stale definitions/configuration, held locks, active-operation refusal and visible mismatch after external edits. Preserve includes, unrelated configuration and signing requirements. An assignment-save failure after a successful Git write must surface the storage problem without repeating or undoing that write. |
| Rewritten-series review and publication | Use the [native rewrite fixtures](recovery-rewrite-native-cases.md) and [series contract](rewritten-series.md) for original/new messages, changed/reordered/possible/ambiguous pairs, missing objects, inspector activation and Back/close cancellation. Publication requires a separate explicit destination check and reviewed exact lease. Exercise local/remote/URL movement, hook refusal, cancellation after remote update, explicit completion detection and fresh review after movement against a disposable local remote; returning focus or reopening the app must not publish or retry. |
| Search and file history | Find a match beyond loaded history, retain pinned scope across ref movement, cancel active work, and continue a bounded scan without claiming exhaustion. Cross file-history page and rename boundaries, inspect deletions/merge parents, and retain query, revision, selection, viewport, and focus through Compare/Back/Settings. |
| Diff and refresh interactions | Inspect unified and split alignment, Find, copying without padding/gutter text, opposite-side scrolling, and partial selections. Make external file/ref changes, switch focus away and back, and verify coalesced local refresh retains context/drafts while invalidating stale selections. Exercise watcher errors and manual recovery. |
| Authentication and cancellation | Use disposable loopback transports and configured helpers for username/token prompts, expired credentials, SSH-agent transport, host verification, configured commit/tag signatures and signing refusal, exact-secret diagnostic/progress masking, cancellation and no replay. Verify retained index/working state and explicit remote targets. Test detached helpers retaining output or input pipes; the app must stop and join its own I/O threads. [Local authentication/signing evidence](authentication.md#verification-and-limits) is separate from live-provider access, real Keychain unlock and hardware-backed signing. |
| Blame and line history | Exercise immutable revision attribution, raw working files against HEAD, staged-only and unstaged uncommitted lines, renames, shallow history and unavailable content. Copy exact source; inspect a line's commit and bounded first-parent history; verify Back, focus, selection, cancellation and stale-result rejection across nested inspections. |
| Tags and ignore | Filter and inspect tags; review lightweight/annotated creation and signing; refuse moved-tag deletion and changed remote destinations; verify that named-tag Push creates only that remote ref. Preview literal file/directory rules in shared/local destinations, preserve formatting and unrelated work, refuse stale/symbolic writes, and keep tracked paths tracked without staging. |
| Image comparison | Exercise side-by-side, Overlay opacity and draggable Wipe with linked pan/zoom, keyboard adjustment, checkerboards, different source sizes/downsample ratios and missing sides. Verify scale labels, gesture cancellation, retained navigation and unchanged decoder bounds. |
| macOS conventions and accessibility | Check menu availability, standard shortcuts, Help, Hide/Minimize/Close, captured Finder/editor handoff and launcher failures. Exercise follow-system appearance and manual themes without losing editor context. Inspect ordinary keyboard focus, names and supported selected/expanded/disabled states, and record available transparency/contrast/motion settings separately from unsupported hardware or OS versions. Exercise VoiceOver names, roles, selected/expanded/disabled states, current-row announcements, editing, modal containment, restored focus and status/error announcements using the [native accessibility contract](native-accessibility.md); record actual settings and build-specific results. |
| Linux desktop integration and text | Follow the [desktop checklist](linux.md#ubuntu-desktop-acceptance-checklist) for the affected session: visible Menu and shortcut help, Control-based shortcuts, client/server window decorations, move/maximize/restore/close, picker success/cancel/portal failure and explicit editor launch. Check live Wayland portal text-size/antialiasing changes, missing-portal/fontconfig fallback, focus-return recovery and X11 DPI without applying a second text multiplier. Retain logical source rows, split/gutter alignment, selection, Find and hidden-tab context through scaling. Record physical/nested/virtual backend, compositor, GPU and scale; semantic-tree exposure is not screen-reader or IME acceptance. |
| Themes and custom themes | Open Settings with custom themes saved and check the picker at 1,000 × 680 and 1,440 × 900 in both densities: the Light, Dark and Your themes groups of 132 px cards in four, three and two columns by width, hover, selected and focus states, the warning glyph on a card whose palette has readability findings, a click that applies a custom theme, and follow-system switching with a custom theme selected. Exercise the editor (New theme…, Edit…, live preview, Reset to base, Cancel, Save, Delete… with its confirmation, keyboard-only operation) in a light and a dark base, and export/import through the platform dialogs, including a malformed file, a newer-version file and the 32-theme bound. When the switch path changes, record `gitturtle.theme_apply_frame_ms` and UI-thread CPU per switch in release mode on the 32-theme store against the [themes specification](development/themes/spec.md#performance). |
| Packages, upgrades and build diagnostics | Use the [macOS](../.agents/skills/gitturtle-native-qa/references/macos-package.md) or [Linux](../.agents/skills/gitturtle-native-qa/references/linux-package.md) package procedure for the affected target. Match source/compiled identity, executable and artifact hashes, metadata, embedded/bundled assets, notices and the running path. Check About/Copy bug diagnostics and exact information flags without opening app state. On Linux, use package/installer fixtures and the actual extracted archive for identity/notice refusal, checksum verification, relocation, active-process refusal, corruption and rollback preservation. Match the archive and installed executable hashes; keep strict distribution refusal distinct from a development-bundle pass. Synthetic payload/tool tests do not establish archive or native acceptance; on macOS, distinguish local ad-hoc signing from notarization and other-machine acceptance. Package/library/headless checks do not replace real desktop interaction or clear the [public release requirements](public-launch.md#before-a-public-binary-release). |

Native checks should cover relevant narrow/wide layouts, long names, large lists, themes, densities, independent interface/code text sizes, keyboard focus, hover/selection/disabled states, and empty/loading/error states for the affected controls. Changes shared across the palette or scaling system need representative light/dark and boundary-size coverage; use all supported themes when the change affects every palette. Distinguish pointer, keyboard and screen-reader results. Final combined Rust/dependency gates and package checks follow [the project validation agreement](../AGENTS.md#validation); a docs-only update requires link and diff review, without rebuilding the app.

### Retained review and recovery workflow checks

These rows describe checks to select for the affected feature, not completed native passes or a mandatory full sweep. Record new results with the exercised build and task; retain [milestone evidence](security-quality-milestone.md) under its original identity. Keep final release, installed executable identity and platform/account-dependent evidence separate. Re-run a successful check only after relevant changes or a concrete unresolved concern.

| Required feature | Current validation scope |
| --- | --- |
| 1. Revision comparison | Use [the revision workflow](user-guide.md#browse-history-then-open-a-comparison) to compare diverged branches, tags and explicit commits in both directions and modes. Check resolved IDs, rename/mode/type changes, absent text/image sides, ambiguous names, moving refs, missing objects and unrelated/multiple-base ancestry. Cancel during a read; verify no checkout/fetch and Back/focus restoration, including a late preview after leaving Compare. |
| 2. Text review | Exercise [review variants](architecture.md#prepared-diff-presentation) in unified/split modes: intraline Unicode edits, CRLF, no final newline, long lines, whitespace suppression, context expansion through 192 lines, and Option-Up/Down. Verify literal source copy, Find, gutters and linked scrolling. Filtered/expanded variants must explain disabled partial staging; resetting must restore exact Git actions and preserve unrelated changes. |
| 3. Text size and accessibility | Follow [typography and density](../DESIGN.md#typography-and-density): independent interface/code settings and resets, persistence, both densities and the twenty built-in themes plus a custom theme at minimum/wide sizes. Retain selection, focus, Find and viewports through scaling. Inspect names/roles/supported states and Increase Contrast, Reduce Transparency and system light/dark behavior where available. Exercise VoiceOver names, roles, selected/expanded/disabled states, current-row announcements, editing, modal containment, restored focus and status/error announcements using the [native accessibility contract](native-accessibility.md); record actual settings and build-specific results. |
| 4. Quick Open and path filters | Exercise Command-P immediate typing, worktree versus pinned revision scope, keyboard selection/Return/Escape, Unicode/long/raw-byte paths, deleted/conflicted/unsupported files, no matches and visible truncation. Verify File History/Blame use the inspected target and Back restores an interrupted source preview. Check [bounded discovery](architecture.md#revision-inspection-review-and-recovery), rapid query replacement, repository switching, and changed/working file filters. |
| 5. Multi-file staging | Exercise Command-toggle, Shift-click/arrow ranges, Command-A, selected counts, directory grouping and separate staged/unstaged identities. Compare Git index/worktree bytes before/after exact selected Stage/Unstage, including renames, binaries and mixed states. Filtering/grouping clears selection; refresh retains only visible survivors; switching repositories clears it. Check stale plans, partial failures, filtered all-files disabling and existing hunk/line staging. See [working operations](user-guide.md#open-a-project-and-work-with-git). |
| 6. Worktree management | Follow [worktree semantics](parallel-work-recovery.md#worktrees): review and create existing/new branch destinations, inspect state and hand off to GitTurtle/Finder/editor. Refuse occupied branches, stale identities, dirty/untracked/ignored content, locked/missing/main/current worktrees and active conflicts. Exercise Force remove worktree on a dirty target: its confirmation shows the file counts, ordinary removal stays refused, force removal deletes the folder and retains the branch, the confirmation lists the deleted paths and unfinished state, and locked/main/current/missing targets, held lock files, submodules and nested repositories stay refused. Verify shared versus private configuration/drafts, branch retention after removal, and honest partial-checkout failure feedback without recursive cleanup. |
| 7. Activity and reflog recovery | Exercise the bounded [activity/reflog workflows](parallel-work-recovery.md): captured repository/target/time, running and final outcomes, cancellation/uncertainty, restart, and explicit next actions without replay. Confirm retained activity excludes secrets and arbitrary diagnostics. Inspect available and expired/missing reflog commits; create the exact recovery branch after revalidation while preserving HEAD, index and working bytes. |
| 8. Conflict blocks | Follow [block resolution](conflict-blocks.md) across merge, rebase, cherry-pick and stash conflicts, including merge/diff3/zdiff3 markers. Test Previous/Next and shortcuts, Current/Incoming/Both, manual editing, unresolved counts, empty/CRLF sides, malformed markers and fallbacks. Save draft must leave the index conflicted; Save and stage must refuse remaining markers/stale sources and preserve unrelated entries. Retain drafts through file/view changes and unrelated refresh; exercise Continue/Abort/Keep files separately. |
| 9. Interactive rebase | Follow [native rebase](interactive-rebase.md): reviewed exclusive base, exact sequence, button and Option-arrow reorder, P/R/S/F/D actions, invalid squash/fixup positions, known-remote acknowledgment and stale plans. Verify native reword/squash messages, separate base/replayed-commit labels, hooks/signing failures, intermediate cancellation, conflicts, Continue/Abort and restart resume. Protect tracked/untracked/ignored work and explain unsupported histories; no automatic force-push. |
| 10. Explicit LFS downloads | Use [LFS preview semantics](lfs-previews.md) with a disposable local source. Verify reviewed file/object/source/size, one-object scope despite unrelated/recent pointers, missing tooling/configuration/credentials, cancellation, corrupt/unavailable objects and stale plans. Independently confirm SHA-256/size, unchanged HEAD/index/worktree and preview reload only for the still-selected target. Include raw working pointers and resolved LFS text's whole-file-only staging; ordinary browsing must remain passive. |

The [September 9 app preparation and path-filter report](benchmarks/2026-09-09-review-app.md) records seventeen release series with forty measurements and three warmups each, including bounded diff/intraline/split/context preparation and cached 50,000-path filters. It reports process memory and CPU-only timing separately from native frames.

The [September 9 release backend report](benchmarks/2026-09-09-review-backend.md) and [raw attempts](benchmarks/2026-09-09-review-backend.json) cover thirteen series with forty measured calls and three warmups each: endpoint/merge-base comparison, tracked-path searches, conflict parsing, rebase planning, worktree listing/details, reflog reads and passive LFS preparation. Source/fixture hashes were stable throughout the run. The report includes p50/p95/p99/max and substantial uncontrolled background load; it excludes native frames, transfer/cancellation latency, mutation, memory-growth analysis and final-package verification, and makes no speedup claim.

The [September 8 everyday backend report](benchmarks/2026-09-08-everyday-workflows.md) records release core measurements for status, working previews, search, file history, and changed-file reads with/without renames. Its raw data and fixture checks establish the stated backend observations; they exclude native frames, writes, watcher behavior, and package verification.

Current semantics and focused fixture commands are documented in [authentication](authentication.md), [tags and ignore](macos-git-actions.md), and [attribution, images and macOS conventions](macos-features.md). The [Liquid Glass investigation](liquid-glass-investigation.md) records the actual native prototype and compositing limitation; the integrated appearance remains opaque. The [previous macOS backend report](benchmarks/2026-09-08-macos-milestone-backend.md) identifies its source inputs and measurement scope independently of native frame evidence.

The [CI workflow](../.github/workflows/quality.yml) configures locked workspace tests and strict all-target Clippy on macOS 15 and Ubuntu 24.04, formatting, and release compilation/package checks on macOS 26 and Ubuntu 24.04. Python guidance/controller checks run on macOS 15 and Ubuntu 24.04. Disposable package checks cover identity, notices and applicable installation, ELF or Mach-O verification; diagnostics are uploaded, while [binary artifacts](ci-artifacts.md) require complete notices. [Actual hosted results](benchmarks/2026-09-15-ci.md) remain distinct from configured coverage and from physical-desktop, screen-reader, native-package or distribution acceptance.

## October 3 Refresh opens All history when the scoped branch is gone

Task `history-vanished-scope-refresh`, attempt 2. History scoped to a local branch that has since been deleted no longer reports the repository unavailable on an explicit Refresh (Ctrl+R). The worker resolves the scope again, finds it gone and reads All history. The page then lists the repository's commits, the tab has no "· unavailable" suffix, and one polite status names the vanished branch and ends "Showing All history." The base reopened with the stale scope, so the whole snapshot failed: "Could not open repository" and a tab marked unavailable until All history was chosen. A worktree scope behaves the same way. Quiet refresh still keeps the displayed history with its explanation, but reports a vanished scope once: after it is dismissed, later quiet refreshes and accepted writes do not raise it again. History search under a vanished scope reports the scope change instead of a failed read. The status wraps beside its Dismiss action instead of being cut. `crates/app/docs/navigation-and-refresh.md` states the explicit-Refresh rule beside the quiet one.

Automated, in `cargo test --locked -p gitturtle`, run by the candidate's gates before this round:
- `views::tests::refresh_opens_all_history_when_the_scoped_branch_is_gone` and `views::tests::refresh_opens_all_history_when_the_scoped_worktree_is_gone` assert All history, no page error, no unavailable tab and no assertive alert. The one notice starts "History scope changed: ", names the vanished scope and ends "Showing All history." It is drawn as the polite status (`operation-notice-summary`), and the assertive summary (`operation-error-summary`) is absent. The polite announcement is asserted by these view tests, not by the frames.
- `views::tests::a_vanished_scope_is_reported_once_across_later_writes` makes two writes after the deletion and counts a single report.
- `views::tests::search_under_a_vanished_scope_reports_the_scope_change` expects "Search could not complete" with "History scope changed: Local branch 'departing'…", no page error and no unavailable tab.
- `worker::image_tests::explicit_open_with_a_vanished_scope_reads_all_history_and_names_the_scope` is new. `worker::image_tests::quiet_deleted_scope_keeps_history_context_but_updates_navigation_metadata` now also asserts the vanished scope's identity.

Native evidence, full tier. Let `$EVIDENCE` be `/tmp/gitturtle-evidence`.
- **Builds:** base `50cc974` (origin/main when the task started; sha256 `95a19b469c3eedc6385336bcabba7e8eb02d881e6b24268dd9a0f7fc112eb813`) and candidate `cbfca06` (sha256 `45bb0c072c960cf8b56fa8827bc8fad12b25c5a3f2a4073a9ad2dab078a9a3ad`), the candidate as the controller rebased it onto the run's accepted head `fa484fe`. Both are release builds of clean trees, each in its own fresh target directory, `x86_64-unknown-linux-gnu`, rustc 1.99.0, and each was identity-checked; `qa.py identity` accepted the pair. The base executable is the attested release build of exactly `50cc974` that the `focus-reveal-same-frame` round also used.
- **Earlier captures:** the first run captured candidate `1683c04` into `$EVIDENCE/runs/history-vanished-scope-refresh` (2026-10-02 20:50:38–20:56:27 UTC, tooling of #163, `c03ab86`), and `qa.py recheck` reproduced its 8 candidate crops byte-identical on the first rebase, `6656764` on `f695dce`. The controller then rebased the candidate onto `fa484fe` as `cbfca06`, with the same `git patch-id --stable` (`0a2c8aa…`) as both. Its recheck (2026-10-03 from 15:10:20 UTC, `$EVIDENCE/runs/history-vanished-scope-refresh-recheck-cbfca06`) matched the 4 `dismissed` and `after-push` crops but not the 4 `refreshed` ones: each differed by 1,078 px, two 1 px rows across the History list (x 181–719). There the new frames draw the accent colour, (117, 224, 187) in Midnight and (52, 85, 166) in Porcelain, where the old ones drew the divider. They are the list's keyboard-focus edges after Ctrl+R. In the first run, the pointer park after keyboard input crossed the window and put GPUI into mouse mode, which hides them; #166 (`cc6e50d`) fixed the park. Frames that differ mean the evidence is redone, so the scenario ran again on `cbfca06`, and every frame and figure below comes from that run.
- **Host:** 2026-10-03, Ubuntu 26.04.1 LTS, GNOME 50.1, XWayland `:0` at scale factor 1, `--input mutter`: input went through Mutter RemoteDesktop with X focus verified, never XTest. The window was at the app's 1000 × 680 minimum (`main.rs:2095` on the base, `:2094` on the candidate). Interface text was 13 and 18 pt, in the palettes Midnight and Porcelain.
- **Run:** `qa.py scenario run` of `docs/evidence/history-vanished-scope-refresh/scenario.json` with both builds into `$EVIDENCE/runs/history-vanished-scope-refresh-cbfca06`, 2026-10-03 from 15:13:49 to 15:19:31 UTC, with the tooling on `main` at `cc6e50d` (#163's fixture copies and #166's park). It exited 0: 8 launches, every guard passed, 32 of 32 analyses as expected (none recorded), and no crop matched a privacy template. The spec's sha256 is `66505bd1…`.
- **Fixture:** the spec's recipe at `$EVIDENCE/fixtures/history-vanished-scope-refresh/repo`. It has three commits on `main` (`6701e6d`, `26dd660`, HEAD `7793c5a`) and `feature/refresh-explains-a-vanished-branch` at the second, which `main` contains, so a safe delete accepts it. A local bare `origin` holds `main` at `26dd660`, one commit behind; `main` tracks it, so a later Push writes without adding an upstream. Each launch opened its own fresh copy, `$EVIDENCE/fixtures/history-vanished-scope-refresh_copy/` (tooling #163), and the recipe build was unchanged by every launch. Each launch had its own HOME and XDG directories, a generated store with Follow system off, and the `GitTurtle QA` identity.
- **Writes:** through the app each launch deleted the scoped branch, pressed Ctrl+R, dismissed the notice and pushed `main` to the copy's `origin`. Every launch's writes verdict matched the declaration, in both builds:
  - `refs/heads/feature/refresh-explains-a-vanished-branch` was deleted, with its reflog;
  - `refs/remotes/origin/main` and the bare `origin`'s `refs/heads/main` went from `26dd660` to `7793c5a`, the tracking ref's reflog gaining one entry;
  - nothing else changed, config included.
- **Route, the same input in both builds:** the launch settles on All history. A click expands the `feature` folder under LOCAL BRANCHES, and a click on `refresh-explains-a-vanished-branch` scopes History to it (guarded: the scope header changed). Shift+F10 opens the branch's actions and a click on Delete branch… opens the confirmation, each guarded. Its Delete branch deletes the branch, and the quiet refresh that follows raises the red scope report in both builds (`deleted`). Next come Ctrl+R (`refreshed`) and a click on the banner's Dismiss (`dismissed`). Ctrl+F focuses History's search, and Shift+Tab ×8 reaches Push in both builds. A click could not be used there, since the base's remaining notice keeps its toolbar 40 px lower. Return pushes (`after-push`).

Frames in [`evidence/history-vanished-scope-refresh/`](evidence/history-vanished-scope-refresh/), 14 crops of the whole window above the status bar, from (0, 0): 1000 × 654 at 13 pt and 1000 × 644 at 18 pt. The spec beside them is the run's byte for byte, and is unchanged from the first run. In every `refreshed` frame both builds draw the History list's keyboard-focus edges, since Ctrl+R was the last input: 1 px accent rules under the column header and along the list's foot, at rows 289 and 637 at 13 pt, and at rows 400 (base) or 411 (candidate) and 627 at 18 pt.
- `base-midnight-13pt-1000x680-refreshed.png`, `base-porcelain-13pt-1000x680-refreshed.png`: on the base at 13 pt after Ctrl+R, the tab reads "repo · unavailable". The scope header still names `feature/refresh-explains…` with 0–0, the page reads "Could not open repository" with the vanished branch in its diagnostic, and the inspector is empty. The red scope banner, beside Details… and Dismiss, is cut with an ellipsis ("…The displayed history is retained; c…").
- `candidate-midnight-13pt-1000x680-refreshed.png`, `candidate-porcelain-13pt-1000x680-refreshed.png`: on the candidate at 13 pt after Ctrl+R, the tab reads "repo" with no suffix, and the header reads "All history 1–3" over the fixture's three commits. One explanation, on one line in the notice colours instead of the error red, names the branch and ends "Showing All history.", with Dismiss beside it.
- `candidate-midnight-13pt-1000x680-dismissed.png`, `candidate-porcelain-13pt-1000x680-dismissed.png`: on the candidate, the explanation dismissed. All history shows no banner, the toolbar takes its place, and the selected commit and its inspector are unchanged.
- `base-midnight-13pt-1000x680-after-push.png`, `base-porcelain-13pt-1000x680-after-push.png`: on the base, after the later Push, the scope report is raised again, red with Details… and Dismiss, above "History updated · Show latest". The repository is still unavailable, while the counts read 0 ahead and 0 behind and Push keeps its focus ring. The uncommitted base `dismissed` captures prove the banner had been dismissed first: `base/midnight-13pt/captures/dismissed.png` (sha256 `7119d2ada8af8ab237db1a1b8a0f8f68c754ddcd6a47a9c622007944fd584b72`) and `base/porcelain-13pt/captures/dismissed.png` (`cad4a737856f13d5bc93d25c862ea31170ee7a215612a3ea29dc15b05c206514`) in the run's bundle. Each shows the red report gone, with the delete's "Deleted local branch 'feature/refresh-explains-a-vanished-branch'" notice in its place.
- `candidate-midnight-13pt-1000x680-after-push.png`, `candidate-porcelain-13pt-1000x680-after-push.png`: on the candidate, after the later Push, only "Pushed main to origin/main" appears, with no second scope report. All history marks the newest commit `main +1`, and the counts read 0 ahead and 0 behind.
- `base-midnight-18pt-1000x680-refreshed.png`, `base-porcelain-18pt-1000x680-refreshed.png`: on the base at 18 pt after Ctrl+R, the tab reads "repo · unavailable" and the page "Could not open repository", its centred text clipped at the inspector's edge. The red scope banner is cut with an ellipsis four letters into "local", after "…no longer exists in the".
- `candidate-midnight-18pt-1000x680-refreshed.png`, `candidate-porcelain-18pt-1000x680-refreshed.png`: on the candidate at 18 pt after Ctrl+R, the tab reads "repo", and the header reads "All history 1–3". The explanation wraps to two lines and ends "Showing All history.", with Dismiss beside it.

The 18 pt wrap, measured on the candidate's 18 pt frames in both palettes:
- The banner is 68 px tall: rows 123–190 at x 895, between the header's rule and the toolbar. Attempt 1's single cut line was 57 px.
- Its last line's ink ends at y 177, which clears the toolbar's top edge at y 191 by 13 px (`explanation-last-line-in-banner-18pt` allows at most 16). The scroll bound therefore clips no line.
- The full text, quoted from the frame: "History scope changed: Local branch 'feature/refresh-explains-a-vanished-branch' no longer exists in the local repository snapshot. Showing All history." The first line breaks after "repository".
- Dismiss sits vertically centred beside it: its label's ink spans rows 150–163, centred on the banner's 156.5.
- The text is (117, 224, 187) on (34, 59, 59) in Midnight, about 7.5:1, and (52, 85, 166) on (220, 230, 246) in Porcelain, about 5.6:1.

Analyses, 32 of 32 as expected. The base-against-candidate compares are masked with `status-timing`, which covered 0 px.
- **Before Refresh** (`deleted-same-on-both-*`): the builds' `deleted` frames are identical, 0 px in all 4 variants. The captures are byte-identical too.
- **After Refresh** (`refreshed-differs-*`): the builds differ by 102,083 px at 13 pt (both palettes), and by 215,530 (Midnight) and 205,899 px (Porcelain) at 18 pt.
- **In the banner region** ((600, 92)–(900, 128) at 13 pt, (500, 126)–(880, 178) at 18 pt; `refreshed-replaces-the-report-*`): from `deleted` to `refreshed`, the candidate replaces the red report, changing 10,800 px at 13 pt and 19,760 px at 18 pt. The base is unchanged (0 px).
- **After Push**, in the same region (`after-push-no-second-report-*`): the candidate again differs from `deleted` by 10,800 and 19,760 px, showing only "Pushed main to origin/main". The base shows the scope report again, 0 px from the report the delete raised. Across the page (`after-push-differs-*`) the builds differ by 222,652 (Midnight) and 212,721 px (Porcelain) at 13 pt, and by 312,500 and 297,921 px at 18 pt.
- **The wrap** (`explanation-full-text-visible-18pt`, at least 68 px; `explanation-last-line-in-banner-18pt`, at most 16 px): 68 px and 13 px in both palettes.

Coordinator decisions, each delegated by the owner:
- **(a) A long realistic branch name.** The window cannot go below 1000 × 680, so a long name is the only way to show the 18 pt wrap. At that width the 13 pt explanation still fits on one line.
- **(b) A wrap threshold of 68 px, the measured two lines.** The first estimate of 80 would have failed a banner showing the full text, and 57 px is one line.
- **(c) Whole-page crops above the status bar.** This is History, not the project hub, and the tab strip and the page must appear together.
- **(d) Captured again on the second rebase.** The first run's capture of C (`1683c04`) re-checked byte-identical on C′ (`6656764`). On C″ (`cbfca06`) the `refreshed` crops differed only by the focus edges #166 made visible, so the evidence was captured again on C″ rather than attested with frames that differ. The base stays `50cc974`, origin/main when the task started, as the contract names it.

History: attempt 1 (`fa3b388`) failed natively. At 18 pt its explanation was one 57 px line, truncated after "…local repository snap…" (`$EVIDENCE/runs/history-vanished-scope-refresh-attempt1`, 20:19:33–20:25:32 UTC; `explanation-full-text-visible-18pt` measured 57 px in both palettes, against the 80 px estimate then in the spec). The coordinator attested the failure with a fixing note, the verifier failed attempt 1, and attempt 2 wraps the notice.

Design review: passed, 2026-10-02 about 21:10 UTC, and passed again on 2026-10-03 about 15:30 UTC for the 8 recaptured `refreshed` crops. Each differs from the frame reviewed first only by the two accent rows; the banner, its wrap and Dismiss are pixel-identical. The focus edges are DESIGN.md:260's 1 px accent outline, while the list's sides keep the 2 px pane dividers, as both builds draw them. Pre-existing, not caused by this change: at 18 pt, with any notice shown, the Changed files split clips the inspector's parent chip "P1 · 6701e6d". The candidate's 18 pt `refreshed` frames show only the chip's top edge, without its label. The base's `deleted` frame, byte-identical to the candidate's, hides it entirely. Taste note: "…no longer exists in the local repository snapshot" is internal wording.

`qa.py privacy scan --redacted --jobs 4` with the local template set (28 templates) found all 14 committed crops clean on their committed bytes (31.0 s wall), as did the run's own scan of the same bytes (16.3 s). The crops show fixture branch names, the fixture's `/tmp` path and app labels only.

Not covered natively:
- worktree scopes, which the view test covers;
- macOS, native Wayland and fractional scale factors;
- windows narrower than 1000 px, which tiling managers allow since they ignore the minimum (`DESIGN.md:231`);
- a notice tall enough to scroll;
- hover, pressed and keyboard focus on Dismiss.

The 18 pt `dismissed` and `after-push` captures, the base's `dismissed` captures and both builds' `scoped`, `deleted` and `push-focused` captures stay uncommitted in the run's bundle.

## October 2 Enter and Space leave a focused disabled kit control inert

Task `gpui-base-disabled-focus-note`, the coordinator's follow-up to the non-blocking findings of [Tab leaves a focused kit control that turns disabled](#october-1-tab-leaves-a-focused-kit-control-that-turns-disabled) (#124). No runtime source changes. The disabled-focus entry in the [gpui-base patch record](../vendor/gpui-base/GITTURTLE-PATCH.md) now names the effects of keeping a focused disabled Checkbox, Switch, Radio, Toggle, Link or ColorPicker swatch's handle:
- a caller's `focus`, `focus_visible` and `in_focus` styles apply to it;
- AccessKit reports it as the focused node instead of the window root;
- every ancestor key binding reaches it;
- a mouse-down on it keeps focus on it rather than letting a focusable ancestor take it.

It also says that the controls pass their click or change handler only while enabled, so GPUI registers no Enter or Space keyboard click for a disabled one. The Settings regression runs only on Linux, so all three regressions fail without the patch there and two elsewhere. The entry names the new test.

Automated, `cargo test --locked -p gitturtle`: `native_accessibility::control_tests::enter_and_space_leave_focused_kit_controls_that_turn_disabled_inert` (new) renders a bare Radio, Toggle, Link, ColorPicker swatch, Checkbox and Switch, then a component Button, each between two tab stops. It focuses each with Tab and checks that Enter and Space, each sent as a key-down and a key-up, activate the control while it is enabled. Then it disables the control. Enter, Space and Enter again run no click or change handler and leave its state unchanged, focus stays on it, and Tab moves on to the next tab stop. The mutation run below shows the test catches a disabled Switch that keeps its change handler.

Native evidence, full tier. Let `$EVIDENCE` be `/tmp/gitturtle-evidence`.
- **Builds:** the evidence was captured on the original candidate `059dde0` (sha256 `78bc49c745ba3cbe3d6a95ae216fefb90d79d398555c7dc15c4e8b6de6452e2f`) against its parent, base `50cc974` (sha256 `95a19b469c3eedc6385336bcabba7e8eb02d881e6b24268dd9a0f7fc112eb813`). Both are release builds of clean trees, each in its own `CARGO_TARGET_DIR`, `x86_64-unknown-linux-gnu`, rustc 1.99.0, and `qa.py identity` accepted the pair. The base executable is the attested release build of exactly `50cc974`, reused from the `focus-reveal-same-frame` round. The controller then rebased the candidate mechanically onto `f695dce` (`focus-reveal-same-frame` accepted) as `d20f8f4`, with an unchanged patch. A fresh release build of `d20f8f4` (sha256 `65f9b46733011ceaeeaee85e7951fbf2013b5efea8848e6ff29047b7de9edf7c`; clean tree, its own `CARGO_TARGET_DIR`, rustc 1.99.0, accepted by `qa.py identity`) re-captured the committed crops with `qa.py recheck`.
  The re-check (`$EVIDENCE/runs/gpui-base-disabled-focus-note-recheck-d20f8f4`, 2026-10-02 from 21:05:05 to 21:05:43 UTC, the same spec, host and Mutter input) exited 0 with verdict identical. All four crops were re-captured byte-identical, its 4 guards passed and the fixture was unchanged. Its readings repeat the run's: AT-SPI focus on "Follow system appearance" (toggle button, not pressed) before the click and after Space and Enter, and on "Omarchy theme" (toggle button, pressed) after Tab. The store was `3794c701…` after the click and byte-identical after the keys, and `IsEnabled` was `false`, set `true` for the launch, then restored and read back `false`.
- **Host:** Ubuntu 26.04.1 LTS, GNOME 50.1, XWayland `:0` at scale factor 1, window 1000 × 680, interface text 13 pt.
- **Run:** `qa.py scenario run` of `docs/evidence/gpui-base-disabled-focus-note/scenario.json` (sha256 `af66e698…`) with both builds into `$EVIDENCE/runs/gpui-base-disabled-focus-note`, 2026-10-02 from 20:32:37 to 20:33:58 UTC. The base launch ran from 20:32:37 to 20:33:16 and the candidate's from 20:33:17 to 20:33:55. Every input went through Mutter RemoteDesktop, never XTest. The run exited 0 with verdict pass:
  - 2 launches, each with all 4 guards passed;
  - 24 of 24 analyses as expected, none recorded;
  - no crop matched a privacy template.
- **Fixture:** the spec's recipe, one commit under the QA identity (HEAD `e16ee60` on `main`) at `$EVIDENCE/fixtures/gpui-base-disabled-focus-note/repo`. Its HEAD, status and index were unchanged by both launches. Each launch had its own HOME and XDG directories and the same generated store (sha256 `c92f36ba…`): Midnight, with Follow system off.
- **Omarchy theme:** as in #124's run, each launch's HOME was seeded with `.local/state/omarchy/current/theme.name` (`tokyo-night`) and `theme/colors.toml` with the Tokyo Night colours (accent `#7aa2f7`), so Settings offered the Omarchy card "Follows Tokyo Night".
- **AT-SPI:** `org.a11y.Status IsEnabled` was `false` before each of the two launches, set `true` for that launch only, then restored and read back `false` after each (`run.json`).

Keys and focus order, the same input in both builds:
1. Ctrl+comma opens Settings with nothing focused (`settings-open`, not committed).
2. Tab 7 focuses the Follow system Switch; Tab 6 is Back to repository, #124's order. The Switch draws no ring, so the frame equals `settings-open` (a guard, 0 px with the status-timing mask). AT-SPI reading `switch-focused`, then the store snapshot `store-before-click`.
3. A pointer click on the Omarchy card at window (187, 340) selects it. Tokyo Night applies (a guard: 499,171 px change), and the Switch turns disabled while it holds focus. Store snapshot `store-after-click`, then AT-SPI reading `focus-before-keys`.
4. Space, then Enter, on the focused disabled Switch. Store snapshot `store-after-keys`, then AT-SPI reading `focus-after-keys`.
5. A park moves the pointer off the window, then Tab. `after-tab` is captured with no further park (`keep_pointer`), then AT-SPI reading `focus-after-tab`.

The window's first change came 0.065–0.067 s after Ctrl+comma, 0.036–0.074 s after each of the 7 Tabs, and 0.055 s (candidate) and 0.078 s (base) after the last Tab. Each is a single sample.

AT-SPI's focused node, the same on both builds (76 nodes in every reading):

| Reading | Focused node | Role | States |
| --- | --- | --- | --- |
| `switch-focused`, after Tab 7 | Follow system appearance | toggle button | focused, focusable, enabled, sensitive, showing, visible; not pressed (off) |
| `focus-before-keys`, after the click | Follow system appearance | toggle button | the same |
| `focus-after-keys`, after Space and Enter | Follow system appearance | toggle button | the same (`keys-keep-focus-and-state`) |
| `focus-after-tab` | Omarchy theme | toggle button | focused, focusable, enabled, sensitive, pressed, showing, visible |

The disabled Switch still reports `enabled` and `sensitive`, as #124 found. gpui-base's Switch sets no AccessKit disabled flag; `gpui-base-disabled-accesskit-flag` follows that up.

The preference file `config/gitturtle/preferences.json`, each snapshot taken once the file had been quiet for 1 s, was the same on both builds:

| Snapshot | sha256 | Bytes | `settings.theme` | `settings.follow_system` |
| --- | --- | --- | --- | --- |
| `store-before-click` | `ff01ead52de5417683e79bf2752961e6f4f24e0b5aeaa2e5a5a0e1244c00afb6` | 1230 | `midnight` | `false` |
| `store-after-click` | `3794c701589786db630bdb5e152141e9592ede3aa66a65db26898595b163a2dc` | 1229 | `omarchy` | `false` |
| `store-after-keys` | `3794c701589786db630bdb5e152141e9592ede3aa66a65db26898595b163a2dc` | 1229 | `omarchy` | `false` |

The click changed only `settings.theme` (`click-keeps-follow-system-off`). Space and Enter left the file byte-identical and unwritten, with the same inode and mtime (`keys-keep-the-store`).

Pixels: on either build, neither Space nor Enter changed a pixel within the 2 s wait after it. The full-window `switch-disabled`, `after-space` and `after-enter` frames are byte-identical (`space-changes-nothing` and `enter-changes-nothing`: 0 px, and the status-timing mask covered 0 px). So the committed `after-enter` crop is byte-identical to `switch-disabled`. Base against candidate: each committed capture is 0 px apart in the crop (`*-base-equals-cand`), and the builds' full-window captures have the same sha256 at every step.

The card after Tab: on both builds `tab-reaches-the-card` finds a whole ring with its outer box at [33, 287, 342, 425] in window pixels. It is 2 px of (122, 162, 247), `#7AA2F7`, continuous on all four sides, at 7.29:1 against the surface (19, 20, 28), `#13141C`. It sits 1 px outside the card's 1 px selected border, with 1 px of surface between them. That is the box and contrast #124 measured. The card's box in this run's `after-tab` capture is pixel-identical to #124's committed `omarchy-1000x680-settings-after-tab.png`. Outside the card, the two frames differ only at the disabled Switch's thumb (216 px, (770, 222)–(785, 237)): #124's frame draws it in (169, 177, 214), this run's builds in (102, 108, 135). The cause was not traced. Against `after-enter`, only the ring's pixels change (1,808 px, bbox [33, 287, 342, 425]; `tab-moves-only-the-ring`).

Decision by the coordinator: an earlier capture lost this ring. The QA runner's pointer park moved the pointer into the window, which switches GPUI to mouse mode, and the card draws its ring only while the last input was a key (`window.last_input_was_keyboard()`, `crates/app/src/settings.rs:1245-1249`). The diagnostic bundles `$EVIDENCE/runs/diag-park-a`, `diag-park-b` and `diag-park-c` (local only, not committed) show it. With the pointer kept where it was after Tab, the ring is whole at [33, 287, 342, 425]. After the usual park only the 1 px selected border, [36, 290, 339, 422], remains. The spec therefore parks before the final Tab and captures `after-tab` with `keep_pointer: true`. Reason: Tab is then the last input, as for a keyboard user, and the pointer stays off the window, so nothing is hovered. The park itself is being fixed in the tooling.

The Follow system Switch draws no focus ring, enabled or disabled, on either build. `switch-focused` equals Settings with nothing focused, and `switch-disabled` and `after-enter` show none while AT-SPI places focus on the Switch. That is `switch-focus-ring-and-hover`.

Frames in [`evidence/gpui-base-disabled-focus-note/`](evidence/gpui-base-disabled-focus-note/): four candidate crops, 980 × 305 at window (10, 130), of Settings' Appearance section with the Follow system row and the Desktop group's Omarchy card. Their names carry `midnight-13pt`, the variant's id, which names the starting palette; the card click switched the app to Omarchy Tokyo Night before the last three frames. No base crops are committed, because the builds' frames are byte-identical.
- `candidate-midnight-13pt-1000x680-switch-focused.png`: Midnight after Tab 7. The Follow system Switch is off, holds keyboard focus and draws no ring, and its row reads "Braden in Light Mode; your selected dark palette in Dark Mode." The Omarchy card, "Follows Tokyo Night", is not selected. AT-SPI names the Switch as focused.
- `candidate-midnight-13pt-1000x680-switch-disabled.png`: after the pointer click on the Omarchy card. Tokyo Night is applied, and the card is selected, with its 1 px accent border and check badge. The Switch is off, faded and disabled, and its row reads "Omarchy follows your desktop theme". AT-SPI still names the Switch as focused.
- `candidate-midnight-13pt-1000x680-after-enter.png`: after Space and Enter on the focused disabled Switch. It is byte-identical to `switch-disabled` (sha256 `a0eb602e…`): the Switch, the card and the palette are unchanged.
- `candidate-midnight-13pt-1000x680-after-tab.png`: Tab has left the Switch for the Omarchy card, which draws its whole 2 px ring outside the selected border. AT-SPI names "Omarchy theme" as focused.

`qa.py privacy scan --redacted --jobs 4` with the local template set (28 templates) found all four committed crops clean on their committed bytes (1.9 s wall), as did the run's own scan of the same bytes (2.4 s). The crops show app labels and theme names only.

Vendor evidence, from a throwaway clone of the original candidate `059dde0` built in its own `CARGO_TARGET_DIR` with rustc 1.99.0 (b940084d7 2026-09-28) and cargo 1.99.0. The mutation passes the Switch's change handler while it is disabled (`vendor/gpui-base/src/switch.rs:381`):

```diff
diff --git a/vendor/gpui-base/src/switch.rs b/vendor/gpui-base/src/switch.rs
index ee5d5bf..48cd31b 100644
--- a/vendor/gpui-base/src/switch.rs
+++ b/vendor/gpui-base/src/switch.rs
@@ -378,7 +378,7 @@ impl RenderOnce for Switch {
                 })
             })
             .when_some(
-                (!disabled).then_some(self.on_change).flatten(),
+                self.on_change,
                 |this, on_change| {
                     this.on_click(move |event, window, cx| {
                         on_change(!checked, event, window, cx);
```

Each run executed only the new test in the `gitturtle` binary's unit tests (681 others filtered out), on 2026-10-02 between about 18:17 and 18:22 UTC:
1. **Unmodified sources:** passes, after a fresh build of 4 min 31 s (271 s).
2. **Mutated:** fails at `crates/app/src/native_accessibility/control_tests.rs:925:13`:
   ```text
   assertion `left == right` failed: enter runs no handler and changes no state on the disabled switch
     left: (true, 1)
    right: (false, 0)
   ```
   The Radio, Toggle, Link, ColorPicker swatch and Checkbox passed every check first, as did the enabled Switch's Enter and Space. The disabled Switch's first Enter then ran its change handler once and turned it on.
3. **Sources restored:** passes.

The clone and its target directory were deleted afterwards. The diff and the three logs are kept locally in `.local/evidence/gpui-base-disabled-focus-note/` (`vendor-mutation.diff`, `vendor-run1.log`, `vendor-run2-mutated.log`, `vendor-run3-restored.log`).

Decision by the coordinator: these runs carry over to the rebased candidate `d20f8f4` by identity rather than being repeated. Reason: `d20f8f4` adds and removes exactly `059dde0`'s lines in `control_tests.rs` and `GITTURTLE-PATCH.md`. `git diff --stat 50cc974 f695dce` shows that the change it was rebased over touches only `DESIGN.md`, `crates/app/src/focus_reveal.rs`, `branch_actions.rs` and its tests, `projects.rs`, `tags.rs`, this file and `docs/evidence/focus-reveal-same-frame/`. Nothing under `vendor/`, in `native_accessibility/`, or in `Cargo.toml` or `Cargo.lock` changed, so a repeat would run the same test against the same vendor sources and dependencies.

Code review: a read-only `code-reviewer` pass (2026-10-02, about 18:20 UTC) checked each effect the note names against the code at `059dde0`, whose vendor sources `d20f8f4` keeps unchanged. Every one holds:
- the handle tracking in `vendor/gpui-base/src/disabled_focus.rs:29-35`;
- the handler gating at `switch.rs:381` and its equivalents in the other five controls;
- in gpui-pre 0.3.4's `div.rs`, the focus styles and AccessKit focus.

One LOW wording finding: the note's lines 56-59 say a mouse-down on a disabled control "focuses its own handle and calls `prevent_default`". For the Switch, Toggle and Link, their `stop_propagation` runs first and halts dispatch before that listener (gpui `div.rs:2719-2733`, `window.rs:5576-5579`). The observable result is the same: focus stays on the control and no ancestor takes it. Two omissions: AccessKit also offers the Focus action for the tracked handle (`div.rs:3530`), and a caller's `on_focus` and `on_blur` listeners may fire for it. Decision by the coordinator: non-blocking. The finding is recorded here, and the sentence will be fixed in a later interactive vendor-guidance change. Reason: the note states the observable behaviour correctly, and rewording one clause is not worth an attempt and an evidence round.

Design review: passed, 2026-10-02; the `after-tab` crop was re-reviewed at about 20:37 UTC. After Tab leaves the disabled Follow system Switch, the selected Omarchy card holds keyboard focus with its whole 2 px Tokyo Night accent ring (#7AA2F7, 7.29:1 on #13141C) 1 px outside its 1 px selected border on all four sides, no hover fill (pointer parked and kept off the window), and AT-SPI reports the same focus. The focused selected card shows a double outline: the 1 px selected border inside the 2 px ring, with a 1 px surface gap. `DESIGN.md:149` describes that ring and border, so the outline stands for this task. Removing it is `theme-card-selection-check`'s.

Not covered: macOS, native Wayland, fractional scale factors and a screen reader speaking. Natively, the other five controls and the component Button are not covered either; the view test covers them.

## October 2 a focused row is revealed in the frame that first draws it

Task `focus-reveal-same-frame`. `focus_reveal::FocusReveal` now scrolls a newly focused control into view before the frame that first draws its focus is laid out. When focus moves onto a control the last frame drew, the view that builds the container sets the offset from that frame's bounds while it renders, so the frame prepaints and paints the control whole. The base applied the offset after painting (`window.defer`), so #136's probe saw the newly focused row cut at the list's edge for one frame. The rule is unchanged: the least scroll that shows the whole control plus the ring's 3 px room, to the nearest edge, without animation; a click and a redraw without a focus change scroll nothing; and the project hub keeps its 12 px and its reveal on a click. It covers the branch chooser, the remote manager, Tags, the tag inspector's Push to… list and the project hub. A control the previous frame did not draw, or one that has moved since, is still revealed after painting. `DESIGN.md:333` now says the list scrolls "in the frame that first draws the focus, so no painted frame shows the control cut", and that the hub's page does so "in the same frame".

Automated, in `cargo test --locked -p gitturtle` at 1000 × 680 with the installed ring, run by the candidate's gates before this round:
- `branch_actions::tests::branch_chooser_reveals_rows_in_the_frame_that_draws_focus` (30 branches) and `tags::tests::tags_reveal_rows_in_the_frame_that_draws_focus` (30 tags) record every frame painted after each `tab` and `shift-tab`. In each frame, the focused row grown by the ring's room must lie inside the list.
- `projects::tests::project_hub_reveals_fields_in_the_frame_that_draws_focus` makes the same per-frame check for the hub at 18 pt: 12 px clear after each Tab and Shift+Tab, and after a click on a field cut by the page's lower edge in a 420 px window.

These view tests see every painted frame; the native probe below samples them.

Native evidence, full tier. Let `$EVIDENCE` be `/tmp/gitturtle-evidence`.
- **Builds:** base `50cc974` (origin/main when the task started; sha256 `95a19b469c3eedc6385336bcabba7e8eb02d881e6b24268dd9a0f7fc112eb813`) and candidate `26376bc` (sha256 `38166547ef495542c0c9127eec8be789e5f30305be23e07c1384ab2ab5d1be8d`). Both are release builds of clean trees, each in its own fresh target directory, `x86_64-unknown-linux-gnu`, rustc 1.99.0, and `qa.py identity` accepted the pair. The base executable is the attested release build of exactly `50cc974`, which two other tasks' evidence rounds also used as their base.
- **Host:** Ubuntu 26.04.1 LTS, GNOME 50.1, XWayland `:0` at scale factor 1, window 1000 × 680, interface text 13 and 18 pt, palettes Midnight and Porcelain.
- **Run:** `qa.py scenario run` of `docs/evidence/focus-reveal-same-frame/scenario.json` with both builds into `$EVIDENCE/runs/focus-reveal-same-frame`, 2026-10-02 from 19:54:06 to 20:11:56 UTC, with the tooling at `ddb4614` (#162). Input went through Mutter RemoteDesktop with X focus verified, never XTest. The run exited 0:
  - 8 launches, every guard passed;
  - 12 probes per launch;
  - 236 of 236 analyses as expected, 32 of them recorded rather than graded: the base's 24 list probes and 8 hub probes at 18 pt;
  - no crop matched a privacy template.

  The spec's sha256 is `dd330060…`.
- **Fixture:** the spec's recipe, which is `tab-reveals-branch-and-tag-rows`'s under this task's name, so it has the same objects and refs. It holds five commits with HEAD `75842df` on `main`, 30 local branches (`main` and `topic-01` to `topic-29`), 30 tags (`v1.00` to `v1.29`, the even ones annotated) and a local bare `origin`, at `$EVIDENCE/fixtures/focus-reveal-same-frame/repo`. Its HEAD, status, index, refs and reflogs were unchanged by every launch, and no write control was activated. Each launch had its own HOME and XDG directories, a generated store with Follow system off, and the `GitTurtle QA` identity.
- **Route, the same input in both builds** (counts at 13 pt / 18 pt; a probe grabs only the list's or the hub's crop back to back from each press until it rests):
  - **Chooser:** the toolbar's branch button, Up ×12 to "Manage another branch…" and Return open "Manage a local branch", each step guarded. Tab ×2 focuses row 0 (`topic-02`), guarded on its ring. Under probes, Tab ×9 / ×7 reaches the first row wholly below the lower edge (`chooser-tab-down`), then come 11 more Tabs and Shift+Tab ×9 / ×7 back to `topic-24` at the upper edge (`chooser-shift-tab-up`). The pointer then glides onto `topic-24` (`chooser-hover`).
  - **Pointer press:** one wheel step (63 / 87 px) leaves a row cut by the lower edge. A probe presses the button on that row and releases it on the dialog's title. The button is then pressed on the row again and held (`chooser-press`), moved off the row and released; Escape closes the chooser.
  - **Tags,** from the command palette's "Browse and manage tags": the same route, with Tab ×3 to `v1.00`, Tab ×10 / ×8 (`tags-tab-down`), Tab ×11, Shift+Tab ×10 / ×8 to `v1.11` (`tags-shift-tab-up`), the pointer on it (`tags-hover`), one wheel step, the press probe and the held press (`tags-press`).
  - **Project hub,** from "Go to projects": a click on Create focuses Project folder name. Under probes, Tab moves to Parent folder (`hub-tab`), Tab ×2 to Browse… and Initial branch, and Tab to Create project, which is focused only. At 18 pt one wheel step up (87 px) then leaves Initial branch's input 2 px above the page's lower edge. Last, a click probe on Initial branch's input (`hub-click`).

Probe results. Each build received 252 probed presses: 224 Tab and Shift+Tab in the two lists, 16 Tabs in the hub, 8 pointer presses on list rows and 4 hub clicks.
- **Candidate, lists:** after every one of the 224 list keys, every captured frame shows exactly one ring, whole on all four sides, 2 px wide and inside the list's clip. It is (117, 224, 187) on (16, 21, 31) in Midnight, 11.43:1, and (52, 85, 166) on (246, 247, 252) in Porcelain, 6.55:1. Its margin to the clip is 0 px at the left and right in every frame, and at the edge it was revealed to (104 frames at the lower edge, 16 at the upper). That is #136's 3 px ring room: the ring fills it, and the row lies 3 px inside the clip. Every press drew one new frame, and that frame was already the settled one.
- **Candidate, other presses:** after the 8 pointer presses, the 16 hub Tabs and the 4 hub clicks, every frame equals the frame before the press or the settled frame, 0 px apart outside the masked caret column of the hub's inputs (`p-*-endpoints-*`). Two Porcelain 18 pt hub Tabs drew a second frame equal to the settled one outside that column.
- **Resolution:** a probe sees only what its grabs return. Each press's resolution is the longest time it went without a grab. Over the candidate's 252 presses that resolution had a median of 4.1 ms, a 95th percentile of 10.0 ms and a maximum of 26.3 ms (Tags, Tab press 6 of the first probe, Midnight 13 pt). The median interval between grabs was 0.75 to 3.4 ms per press. The finding is therefore "no failing frame at each press's recorded resolution".
- **What a gap could hide:** the largest gaps just before a candidate press's first change were 12.29 ms (Porcelain 13 pt, the chooser's pointer press), 10.94 ms (Midnight 18 pt, the hub click) and 10.0 ms (Porcelain 13 pt, the first of the chooser's 11 further Tabs). The base's cut frames stayed on screen for at least 6.3 to 27.2 ms (first to last grab that showed them), so a cut frame as short as the shortest of them could fall inside such a gap. The per-frame view tests above are the exhaustive check.
- **Base, recorded with timing** (`probe_ring` and `probe_endpoints` with `"record"`): 33 failing frames, each count a lower bound for the same reason. The 30 ring cuts were first seen 10.3 to 42.7 ms after the press, and each was followed by the settled frame:

| List | Presses per build | Base cut frames | Only a 2 px strip of the ring in view | Row partly in view, one side cut | First seen after the press | On screen, first to last grab |
| --- | --- | --- | --- | --- | --- | --- |
| Chooser, 13 pt | 58 | 2 (Midnight 1, Porcelain 1) | 2 | 0 | 15.5–16.8 ms | 11.1–16.4 ms |
| Chooser, 18 pt | 50 | 12 (Midnight 8, Porcelain 4) | 8 | 4 (30 of the ring's 53 rows) | 11.1–42.7 ms | 6.3–27.2 ms |
| Tags, 13 pt | 62 | 7 (Midnight 5, Porcelain 2) | 4 | 3 (33 of 40 rows) | 10.3–22.9 ms | 8.3–14.5 ms |
| Tags, 18 pt | 54 | 9 (Midnight 6, Porcelain 3) | 9 | 0 | 10.7–21.1 ms | 7.3–17.8 ms |

The other three base failures were intermediate frames of the 18 pt hub:
- After Tab in Midnight, 14.0 to 19.5 ms after the press, Project folder name had lost its focus border while Parent folder was still below the page's lower edge.
- After the click, at 10.6 ms in Midnight (shown until 18.2 ms) and 17.8 ms in Porcelain (one grab), Initial branch was already focused at the unscrolled page, 2 px from its lower edge instead of 12.

The base's other 18 pt hub presses, and all of its 13 pt hub and pointer presses, drew no intermediate frame.

Frames in [`evidence/focus-reveal-same-frame/`](evidence/focus-reveal-same-frame/), 40 candidate crops of the settled frame after the step that names them, `candidate-{midnight,porcelain}-{13,18}pt-1000x680-{capture}.png`. The chooser crops are 540 × 490 (13 pt) and 540 × 559 (18 pt) at (230, 68); the Tags crops are 600 × 521 and 600 × 563 at (200, 68); and the hub crops are 420 × 574 at (571, 84) and 420 × 530 at (563, 120), the page's right column only. Coordinates below are window pixels; values are the same in both palettes unless given per palette.
- `candidate-…-chooser-tab-down.png`: Tab past the chooser's lower edge reveals `topic-14` at 13 pt and `topic-04` at 18 pt at the bottom, by the least amount. The ring's outer box is (244, 462)–(756, 502) and (244, 508)–(756, 561), whole, 0 px from the list's clip at the bottom, left and right.
- `candidate-…-chooser-shift-tab-up.png`: Shift+Tab past the upper edge reveals `topic-24` at the top. The ring is at (244, 166)–(756, 206) and (244, 225)–(756, 278), whole, 0 px from the clip at the top.
- `candidate-…-chooser-hover.png`: `topic-24` focused with the pointer on it. Inside the 2 px ring there is 1 px of list surface, then the hover fill: (20, 27, 39) in Midnight (1.06:1 against the surface) and (213, 218, 237) in Porcelain (1.30:1). The row's tooltip, `refs/heads/topic-24 · 75842df`, sits over the search field.
- `candidate-…-chooser-press.png`: a pointer press held on the row cut by the lower edge, with no ring. 26 px of `topic-11` is in view at 13 pt (rows 476–501), and 12 px of the next row at 18 pt (rows 549–560, no label in view). Its pressed fill is (22, 29, 41) in Midnight and (186, 195, 225) in Porcelain. Against the frame before the press, only that band changes (13,142 to 13,144 px at 13 pt, 6,060 at 18 pt). Nothing scrolls, and after the release off the row the list is pixel-identical to the frame before the press.
- `candidate-…-tags-tab-down.png`: Tab past Tags' lower edge reveals `v1.10` at 13 pt and `v1.08` at 18 pt at the bottom. The ring is at (214, 493)–(786, 533) and (214, 512)–(786, 565), whole, 0 px from the clip at the bottom, left and right.
- `candidate-…-tags-shift-tab-up.png`: Shift+Tab past the upper edge reveals `v1.11` at the top. The ring is at (214, 167)–(786, 207) and (214, 199)–(786, 252), whole, 0 px from the clip at the top.
- `candidate-…-tags-hover.png`: `v1.11` focused and hovered: the ring, 1 px of list surface, then the same hover fill as the chooser's, with no tooltip.
- `candidate-…-tags-press.png`: a pointer press held on the tag cut by the lower edge, with no ring. 19 px of `v1.22` is in view at 13 pt (rows 514–532), and 42 px of `v1.19` at 18 pt (rows 523–564). The pressed fill is the chooser's. Only that band changes (10,724 to 10,741 px at 13 pt, 23,495 to 23,560 at 18 pt), nothing scrolls, and after the release the list is pixel-identical to the frame before.
- `candidate-…-hub-tab.png`: Parent folder focused by Tab.
  - At 13 pt nothing scrolls: the panel's title and mode tabs, (581, 218)–(981, 380), are pixel-identical before and after, and every probe frame is the one before the key or the settled one.
  - At 18 pt the page scrolls 60 px in the first frame that draws the focus: the content's best row alignment between the frame before Tab and the settled frame is a 60 px shift, in both palettes. Parent folder's input then ends at y 632, 12 px clear of the page's lower edge at y 644: its 3 px glow, then 9 px of panel ((23, 30, 43) in Midnight, (255, 255, 255) in Porcelain).
- `candidate-…-hub-click.png`: Initial branch focused by a click on its input.
  - At 13 pt nothing scrolls (the same title and tabs region, pixel-identical).
  - At 18 pt the page scrolls 10 px in the first changed frame: from 2 px clear of the lower edge to 12 px (3 px glow, 9 px panel).

Coordinator decisions, each delegated by the owner:
- **(a) Only candidate crops are committed.** The base's cut frames are recorded with their timing through `probe_ring` with `"record"`, as the contract asks, and they live in the bundle's probe frames and `analysis.json`. Committing base crops of the settled frames would show nothing, since they equal the candidate's.
- **(b) The press frames show no ring.** Pointer focus draws no focus-visible ring, and the rule for a click is that it scrolls nothing. The four-side ring rule applies to keyboard-focus frames. The press is held and released off the row because a full click activates the row: it chooses the branch and closes the chooser, or opens the tag.
- **(c) The 13 pt hub has nothing to reveal at 1000 × 680:** the whole Create form, down to Create project, fits the page. Its frames therefore show that nothing scrolls.
- **(d) The probe tooling was extended in #162 for this task.** Before it, the scenario format kept only settled frames and could not keep a frame sequence.

Base against candidate: all 40 settled frames are pixel-identical between the builds (`*-base-equals-cand-*`, 0 px in all 40 pairs), so spacing, clipping, text and colours are unchanged; only the probes tell the builds apart.

Design review: passed, 2026-10-02 about 20:20 UTC. Every crop meets `DESIGN.md:333` as the candidate rewrites it. The review noted four things that are the same on both builds and not caused by this change:
- Tags centres its labels, unlike the chooser.
- The chooser's hover tooltip covers the search field.
- At 18 pt the hub's Parent folder placeholder is clipped mid-glyph ("Choose where your project v") with no ellipsis.
- The Midnight hover step is faint (1.06:1), and the ring carries the cue.

`qa.py privacy scan --redacted --jobs 4` with the local template set (28 templates) found all 40 committed crops clean on their committed bytes (26.0 s wall), as did the run's own scan of the same bytes (14.7 s). The crops show fixture branch, tag and object names and app labels only.

Not covered: macOS, native Wayland and fractional scale factors; other window sizes and text sizes; and, natively, the remote manager and the tag inspector's Push to… list, which the view tests and #136's tests cover. The reveal's fallback path still applies after painting, for a control the previous frame did not draw or one that has moved. The native evidence covers only rows the previous frame drew.

## October 2 the review's Source patch draws Compare's gutter

Task `pr-source-gutter-like-compare`. The pull request review's Source patch now wraps its editor in Compare's unified-patch view (`diff_view::new`) once the file's `PatchPresentation`, already prepared off the UI thread since #116, and the editor both exist. It draws the same old and new line-number columns and the same added and removed gutter tints as Compare, and the editor's own line numbers and folding are turned off. A file above Compare's bounds, or one still preparing, keeps plain text with the editor's own numbers. `docs/github-collaboration.md` says so.

Folding (decision F2): the contract asks for Compare's hunk folding, but Compare's unified patch has folding turned off (`crates/app/src/diff_view.rs:162-197`, one gutter row per patch line), while the base's Source was built with folding on (`crates/app/src/text.rs:100`, `.folding(diff.is_none())` with no presentation). Matching Compare therefore removes the Source's fold, and the folded state does not apply. Decision by the coordinator (delegated by the owner, 2026-10-02): match Compare, because the owner's decision was to match Compare. The base frames below show no fold marker. Line 1 is the base's current line, and the toolkit paints a current line's chevron when that line is a fold candidate (`vendor/gpui-base/src/input/base/element.rs:1281-1287`). A folded line always paints its chevron, so no base frame shows a folded hunk; whether any base line could fold at all the frames cannot show, because fold candidates come only from a language highlighter and the base Source is plain text (`text.rs:91-95`). Hovering the gutter, which paints every candidate's chevron, was not exercised. The app's written rule agrees: "Patch wrapping/folding stays disabled" (`crates/app/docs/content-and-layout.md:115`).

Design review (2026-10-02): pass with follow-ups. Every candidate frame matches its Compare counterpart in both palettes: the old and new columns (40 and 41 px at code 12 pt, 61 and 61 px at 18 pt), separators, gutter fill, tints, border and Find bar placement are the same, gutter numbers stay above 4.5:1 on their fills, and the dropped current-line highlight follows Compare, whose editor paints none once its own line numbers are off (`vendor/gpui-base/src/input/base/element.rs:2138-2159`, `diff_view.rs:195`). The follow-ups are the decorated Source's accessible name, which becomes Compare's "Read-only unified patch" (`diff_view.rs:175`) instead of the base's "Exact GitHub supplied patch source" once preparation finishes; the Find bar's labels, drawn in the code font in Source and Compare alike (`diff_view.rs:254`); `crates/app/docs/content-and-layout.md:111`, which still says Source keeps its own line numbers; and the patch panes' missing focus outline, the same on both builds and in Compare. Decision by the coordinator (delegated by the owner): accept this candidate and queue the accessible name with the stale line as their own small task, and the Find bar font and the pane outline as follow-ups. Reason: the contract's criteria (gutter, Find, preparation thread, native) are met, the new name is accurate though less specific, and an attempt plus an evidence round for a one-line, pixel-neutral fix would cost more than a follow-up.

Automated, `cargo test --locked -p gitturtle`, in `github_view::review::tests`:
- `source_patch_draws_compares_gutter` (new): the offline fixture's `src/review/session.rs` patch in the review's Source and Compare's unified patch built from the same text and drawn in the Source frame's place have the same gutter width, text inset, editor size and line height; the gutter spans at least both number columns. The Source's patch layer draws Compare's decorations, and its prepared rows, change rows and column width equal Compare's presentation. A file change while Source waits, and close, cancel the preparation, and nothing lands afterwards.
- `source_patch_takes_the_decorations_its_file_preparation_made` (strengthened): Compare's view is absent until the presentation prepared off the UI thread arrives and present once it does. A superseded file's Source never gains it, and a Source opened after its file's preparation finished has it at once.
- `source_presentation_stops_at_compares_bounds` passes unchanged.

Native evidence, full tier. Let `$EVIDENCE` be `/tmp/gitturtle-evidence`.
- **Builds:** base `629d951` (origin/main when the task started; sha256 `1803562a…`) and candidate `b64f5d0` (sha256 `972b0b75…`), release builds of clean trees in separate target directories, `x86_64-unknown-linux-gnu`, rustc 1.98.0. `qa.py identity` accepted the pair before the run.
- **Host:** Ubuntu 26.04.1 LTS, GNOME 50.1, XWayland `:0` at scale factor 1, window 1000 × 680, interface text 13 pt with code text 12 pt (the default) and 18 pt (the variants' `code_text_size` setting), palettes Midnight and Daylight.
- **Run:** `qa.py scenario run docs/evidence/pr-source-gutter-like-compare/scenario.json --build base=… --build cand=… --input mutter --display :0 --jobs 4 --out $EVIDENCE/runs/pr-source-gutter-like-compare-2`, 2026-10-02 from 14:44:10 to 14:52:04 UTC (474 s), input through Mutter RemoteDesktop with X focus verified, never XTest. Exit 0: 8 launches, every guard passed, 64 of 64 analyses as expected, no crop matched a privacy template. The spec's sha256 is `1a64c79a…`.
- **Fixture:** the spec's recipe, two commits on `main` (`2288af3`, HEAD `34bca84`) at `$EVIDENCE/fixtures/pr-source-gutter-like-compare/repo`. The second commit changes `src/review/session.rs` as the offline review fixture's PR #42 does, so Compare's first hunk (`@@ -1,5 +1,7 @@`) is the PR's first hunk line for line. The GitHub views ran with `GITTURTLE_GITHUB_FIXTURE=review`, with no account, network or credential. The fixture's HEAD, status, index, refs and reflogs were unchanged by every launch. Each launch had its own HOME and XDG directories, a generated store with Follow system off, and the `GitTurtle QA` identity.
- **Steps, the same input in both builds:** the launch settles on History with HEAD's one changed file selected (guarded by the selected row's colour). Return opens it in Compare's Diff. A click on patch row 0 gives the editor focus (`compare-rest`). Ctrl+F, `draft` and Return move Find to its second match, 2/2 (`compare-find`), and Escape closes it. Next come the command palette's **GitHub pull requests**, **Refresh fixture PRs**, #42, **Files · 4**, and Return to select `src/review/session.rs`. The **Source** toggle follows, then 10 wheel steps beside the Source box to the dialog's end (`source-rest`). Last come Ctrl+F, `draft` and Return (`source-find`), and Escape. Guards stop the run if a step lands elsewhere, and opening Find must change nothing outside the Source box.
- **Latency:** the window's first change came 36–53 ms after Return opened Compare, 32–48 ms after Ctrl+F in Compare, 28–36 and 32–49 ms after Find's Return in Compare and Source, 31–37 and 51–63 ms after Escape, and 37–55 ms after Return in the file list, in both builds. After Ctrl+F in Source the first change came 50–120 ms later in four launches, and 10–15 ms later in the other four. The tool times the window's first change, so those fast four are more likely the editor's blinking caret than the Find bar.

Measured on the candidate at scale 1, each value the same in Midnight and Daylight, Source against Compare's unified patch of the same first hunk:

| | Code 12 pt | Code 18 pt |
| --- | --- | --- |
| Old number column: gutter fill from the box's border to the first separator (`*-old-column`) | Source 40 px (border x 261, separator x 302); Compare 40 px (x 43, x 84) | Source 61 px (x 261, x 323); Compare 61 px (x 43, x 105) |
| New number column: from that separator to the second (`*-new-column`) | Source 41 px (x 302 to x 344); Compare 41 px (x 84 to x 126) | Source 61 px (x 323 to x 385); Compare 61 px (x 105 to x 167) |
| Separators past the box's border | +41 and +83 in both | +62 and +124 in both |

The gutter's own fill (on the first hunk header row) and the removed and added rows' tints in the empty column are the same colour in Source and Compare, contrast 1.00 in every case, at rest and with Find open, at both code sizes (`source-*-tint*`, `source-gutter-fill*`). In Midnight these are gutter (23, 30, 43), removed (56, 37, 49) and added (25, 50, 45). In Daylight they are gutter (255, 255, 255), removed (249, 229, 235) and added (226, 241, 233). With Find open, both patches move down 47 px at code 12 pt.

Masked compares, both palettes and both code sizes:
- `compare-*-unchanged*`: Compare draws the same in both builds, 0 px in all 8 pairs. At code 12 pt one pixel column is masked: (262, 418–422) at rest and (262, 465–469) with Find, in the second hunk header's last glyph. In run 1 (`$EVIDENCE/runs/pr-source-gutter-like-compare-1`, 14:34:28–14:42:23 UTC) only those 2 to 4 pixels differed between the builds, by one channel unit. Launches of the base drew both (26, 33, 47) and (26, 34, 48) there, as did launches of the candidate, so the difference varies between launches, not between builds. The candidate changes only a doc comment in `text.rs` and nothing on Compare's path. In run 2 the mask covered 0 px.
- `source-*-only-the-patch`: in the dialog (24, 70)–(976, 626), the builds differ only inside the Source box and its 1 px border, (261, 194)–(959, 440): 0 px outside it at rest and with Find in all 8 pairs.
- `source-rest-patch-changed`: inside the box 44,856–59,543 px differ, as the gutter replaces the editor's numbers.
- `source-find-keeps-border`: beside the Find bar the box's left border, (261, 200–243), keeps its rest colour on the candidate (contrast 1.00). On the base the bar's panel covers it: (23, 30, 43) over (43, 55, 73) in Midnight and (255, 255, 255) over (210, 220, 232) in Daylight, contrast 1.39. The base's Find bar spans the whole box, number column and border included. The candidate's sits over the text column inside the border, as Compare's does.

Frames in [`evidence/pr-source-gutter-like-compare/`](evidence/pr-source-gutter-like-compare/), 32 crops, each in Midnight and Daylight at code 12 pt (`{palette}-13pt`) and 18 pt (`{palette}-13pt-code-18pt`). The Source crops are 711 × 313 at (255, 134): the file name, the Changes, Source and Threads toggles and the Source box. The Compare crops are 676 × 432 at (44, 200): the patch and its gutter.
- `base-…-compare-rest.png` and `candidate-…-compare-rest.png`: Compare's unified Diff of the fixture commit at rest, with header rows, then the old and new number columns and tints of a first hunk that is PR #42's first hunk line for line. This is the reference for Source, and the builds draw it the same.
- `base-…-compare-find.png` and `candidate-…-compare-find.png`: Compare with Find open on `draft` at 2/2, its current match underlined. The Find bar sits over the text column beside the gutter, whose columns and tints move down with their rows.
- `base-…-source-rest.png`: on the base, PR #42's Source patch at rest. It shows the editor's own single column of line numbers, 1 to 13 in view at code 12 pt and 1 to 9 (line 9 partly cut) at code 18 pt, an untinted gutter and a current-line highlight on line 1. In two of the four (Midnight 13 pt and Daylight code 18 pt) the editor's caret is drawn before `@@`.
- `candidate-…-source-rest.png`: on the candidate, the Source patch at rest with Compare's gutter. It has old and new number columns, the removed and added tints across both, empty gutter rows beside each hunk header in view (one at code 18 pt), and no line numbers, current-line highlight or folds of the editor's own.
- `base-…-source-find.png`: on the base, Source with Find open on `draft` at 2/2. The Find bar, in the interface font, spans the whole box over the line numbers and the box's border.
- `candidate-…-source-find.png`: on the candidate, Source with Find open on `draft` at 2/2, its current match underlined. The Find bar, in the code font as in Compare, sits over the text column inside the border, and the gutter's columns and tints move down with their rows.

`qa.py privacy scan --redacted --jobs 4` with the local template set found all 32 committed files clean (19.2 s wall), as did the run's own scan of the same bytes (20.1 s). Each crop was also viewed at full size: they show fixture text and app labels only.

How the spec was fixed: its coordinates were estimates. Two `qa.py launch` probes of the candidate gave the Source box's place, (261, 194)–(959, 440) after the dialog's scroll, and the rows' 18 px pitch, 8 px top padding and the 47 px Find offset. A scenario probe then stopped at the base's guard that opening Find changes nothing outside the box (802 px). A base `qa.py launch` showed the cause: its Find bar paints over the box's border, (261, 194)–(959, 248). The guard and the box masks therefore include the border, and `source-find-keeps-border` measures it. Run 1 then met every expectation but the four code 12 pt Compare compares, as described above.

Seen in the frames, for review: the candidate's Source loses the base's current-line highlight and takes Compare's Find bar placement and font. Its second hunk is the PR's (`@@ -20,3 +22,4 @@`), which differs from the fixture commit's in Compare (`@@ -18,5 +20,6 @@`), so only the first hunk matches row for row.

Not covered: the hunk-folded state (decision F2 above), hover over either gutter, wheel scrolling over the gutter and over the code inside the patch, the switch from the plain patch to the gutter when preparation finishes after Source opens, a patch above Compare's bounds (`source_presentation_stops_at_compares_bounds` covers it), the accessibility tree (`diff_view::new` labels its view "Read-only unified patch"; AT-SPI was not read), interface text sizes other than 13 pt, X11 outside XWayland, native Wayland, macOS, fractional scales and live GitHub. The Source editor's caret and the Find field's caret blink every 500 ms, so the committed crops show either phase. A later `qa.py recheck` can therefore report a crop different at its caret alone.

## October 2 the theme editor stays clear of the status bar

Task `theme-editor-panel-bounds`. The New theme and Edit theme editor's height cap now ends the panel's lower edge 16 px above the status bar's top edge, where it used to end 16 px above the window's bottom edge and so crossed the bar. This holds at every interface text size and inside the toolkit's Linux client-decoration frame (`ThemeForm::max_height_for` with `WindowFrame`). The footer (Reset to base, the save error, Cancel and Save) is now the form's last row inside that cap. A save error that wraps takes its room from the scrolling body and never moves the panel's edge. As the last row of the dialog's clipped body, the footer keeps 3 px under its buttons inside the clip for the focus ring, which the alert's gap above its empty footer slot gives back. So a focused footer button's ring is whole, and the buttons keep the panel's 16 px padding under them. `DESIGN.md` says so in the editor paragraph. Its Status strip section adds the rule for every window-capped modal: under a modal the strip stays in place, dimmed and inert under the backdrop, and a panel edge never crosses it.

History: attempt 1 (`a3f743d`) clipped the footer buttons' focus rings at each button's lower edge, which a native probe showed (`$EVIDENCE/runs/theme-editor-footer-ring-probe-3`, the bottom 2 px band missing for Save, Cancel and Reset to base in both palettes at 13 and 18 pt). The final verifier failed that attempt, and attempt 2 (`c82f3f8`) gives the footer the ring's room.

Design review: passed, 2026-10-02 16:01 UTC. In the 12 focus crops each focused button's ring is a whole 2 px accent band 1 px off the button on all four sides, 11.4:1 against the Midnight panel and 6.6:1 against Porcelain, with 13 px of panel below it and the 16 px above the status bar intact; nothing else in the footer differs from `rest`, and against the probe's frames of attempt 1 only the three rows under each focused button differ, so attempt 1's footer-ring finding is resolved. The 24 rest, end and save-error crops are byte-identical to attempt 1's, so the ring's room took nothing from the panel's 16 px padding or the body. A focused footer button while the save error shows is not captured natively; it rests on `a_focused_footer_buttons_ring_lies_inside_the_body_in_every_state`.

Automated, `cargo test --locked -p gitturtle`, in `theme_editor::tests`:
- `the_panel_ends_16_px_above_the_status_bar_at_every_text_size` (formerly `…_above_the_window_…`): at 1000 × 680 and 1440 × 900, at 11, 13, 14, 15 and 18 pt, the painted panel ends 17 px under Save and exactly 16 px above the painted status bar.
- `the_cap_leaves_16_px_under_the_panel_at_scale_1_and_2`: GPUI's placement of the alert around the cap gives exactly 16 px. This covers scales 1 and 2, three window heights and five text sizes, with server decorations and inside an untiled client-decorated frame.
- `the_cap_keeps_the_panel_inside_client_decorations`: with 20 px window paddings and the frame's 1 px border, at 13 and 18 pt and scales 1 and 2, the panel opens and ends inside the frame and at least 16 px above the bar. A tiled bottom edge and server decorations have no frame.
- `a_wrapped_save_error_takes_its_room_from_the_body` (Unix): a save refused by a read-only preference directory at 1000 × 680, 13 and 18 pt, wraps the error to two lines. The panel keeps its outer bounds and the body gives up exactly the footer's growth.
- `a_focused_footer_buttons_ring_lies_inside_the_body_in_every_state` (new in attempt 2): at 1000 × 680, 13 and 18 pt, at rest and with a save error wrapped to two lines, Shift+Tab from Name focuses Save, Cancel and Reset to base in turn. Each one's installed ring lies inside the body's clip and paints all four edges. The header's Name field and Base button keep the room at the body's top edge, and at rest the buttons keep the panel's 16 px padding under them.
- `strips_cover_the_ring_room_while_the_rows_stand_off_a_boundary` passes unchanged.

Native evidence, full tier. Let `$EVIDENCE` be `/tmp/gitturtle-evidence`.
- **Builds:** base `629d951` (origin/main when the task started; sha256 `1803562a…`) and candidate `c82f3f8` (sha256 `72862de5…`), release builds of clean trees in separate target directories, `x86_64-unknown-linux-gnu`, rustc 1.98.0. `qa.py identity` accepted the pair.
- **Host:** Ubuntu 26.04.1 LTS, GNOME 50.1, XWayland `:0` at scale factor 1 with server decorations, window 1000 × 680, interface text 13 and 18 pt, palettes Midnight and Porcelain.
- **Run:** `qa.py scenario run docs/evidence/theme-editor-panel-bounds/scenario.json --build base=… --build cand=… --input mutter --display :0 --jobs 4 --out $EVIDENCE/runs/theme-editor-panel-bounds-3`, 2026-10-02 from 15:44:42 to 15:53:57 UTC (555 s), input through Mutter RemoteDesktop with X focus verified, never XTest. Exit 0: 8 launches, every guard passed, 152 of 152 analyses as expected, no crop matched a privacy template. The spec's sha256 is `a53a0d3f…`. It is attempt 1's spec unchanged, with the footer focus states, their guards and their analyses added after `rest`.
- **Fixture:** the spec's recipe, three commits on `main` (HEAD `d33146b`) at `$EVIDENCE/fixtures/theme-editor-panel-bounds/repo`. Its HEAD, status, index, refs and reflogs were unchanged by every launch. Each launch had its own HOME and XDG directories, a generated store with Follow system off, and the `GitTurtle QA` identity.
- **Steps, the same keys in both builds:**
  - Ctrl+, opens Settings. 14 wheel steps at 13 pt (16 at 18 pt) bring Your themes on screen.
  - Tab ×28 from the window's focus reaches **New theme…**. Guards require its whole focus ring and no other change. Space opens the editor (`rest`).
  - Shift+Tab from Name focuses Save, then Cancel, then Reset to base (`save-focus`, `cancel-focus`, `reset-focus`). Each press is guarded on a ring appearing around that button and nothing outside the footer band and Name's field changing, so the body never scrolls.
  - Tab ×3 brings focus back through Cancel and Save to Name. A guard requires the editor to draw as at `rest` outside Name's field.
  - 60 wheel steps over the body reach its end, which 5 more steps leave unchanged (`end`).
  - The launch's `config/gitturtle` becomes read-only (0500), and Return in Name saves. The store refuses with "Could not save the theme: Save preferences: Permission denied (os error 13)" (`save-error`). Escape cancels.
- **Latency:** in both builds, the first visible change came 46–55 ms after Ctrl+,, 49–67 ms after Space, 65–81 ms after Shift+Tab, 48–87 ms after Tab, 73–79 ms after Return and 48–76 ms after Escape.

Measured, in window rows at scale 1. The status bar's top rule is at y 654 at 13 pt and y 644 at 18 pt, and both palettes gave the same values:

| | 13 pt | 18 pt |
| --- | --- | --- |
| Candidate, rows of backdrop from the panel's lower border to the bar's rule, at rest, at the end and with the save error | 16, 16, 16 (border at y 637) | 16, 16, 16 (border at y 627) |
| Base, rows of the bar under the panel including its border, at rest and at the end | 10 (border at y 663) | 20 (border at y 663) |
| Base, the same with the save error | 20 (border at y 673) | 35 (border at y 678) |

The candidate's clearance (`clearance-*`) scans x 500 and 700 over plain page and stops at the bar's rule: (32, 42, 55) in Midnight and (183, 190, 204) in Porcelain. Its tolerance of 10 per channel was set from probe frames: Porcelain's shadowed gap spans −8 to +7 around its sample, and the rule differs from the gap by 15 or more in both palettes. The ceiling of 16 means a scan that ran past the rule could not pass. On the candidate the panel's background never reaches the bar (`overlap-*`, 0). The base's overlap is measured in the panel's side padding (x 190 and 809 at 13 pt, 75 and 924 at 18 pt). It is 9, 19, 19 and 34 rows of panel background below the bar's top edge, plus the panel's 1 px border.

Focused footer buttons, in both builds, both palettes, at 13 and 18 pt. Every ring is whole on all four sides (`*-ring-*`):
- **The ring:** a continuous 2 px band of the accent, (117, 224, 187) in Midnight and (52, 85, 166) in Porcelain, 1 px off the button.
- **Candidate rows:** buttons are at y 593–620 at 13 pt and y 572–610 at 18 pt, with ring bands at rows 590–591 and 622–623 (13 pt) and 569–570 and 612–613 (18 pt).
- **Base rows:** buttons are at y 619–646 and y 608–646, with ring bands at rows 616–617 and 648–649, and 605–606 and 648–649.
- **Ring outer edges:** Save x 759 to 809 and Cancel x 697 to 758 at 13 pt, Reset to base x 190 to 288; at 18 pt Save x 857 to 925, Cancel x 770 to 853 and Reset to base x 75 to 209.
- **Bottom band** (`*-bottom-band-*`): the 2 px band 1 px below each button has the top band's colour, contrast 1.00. This is the decisive check for Save, whose own fill is the accent.
- **Below the ring** (`*-ring-to-border-*`): 13 panel rows from the ring's lower edge to the panel's lower border, on the candidate at y 637 (13 pt) and 627 (18 pt), and on the base at y 663.
- **At rest** (`*-padding-rest-*`): every button keeps the panel's 16 px padding to that border.

The ring scans run 2 px outside the 1 px gap around each button and 2 px in. In the probe's frames of attempt 1 (`theme-editor-footer-ring-probe-3`) the same scans found no bottom band on Cancel and Reset to base, and only Save's own fill row on Save.

Masked compares, both palettes:
- `unchanged-*`: the base and the candidate draw the same, 0 px, above y 560 at 13 pt and y 540 at 18 pt in all 12 pairs: the page, the backdrop and the dialog's title and header, and at rest also the first rows. Masked: the Name field's caret, the scrollbar thumb at rest or the scrolled body otherwise, and at 13 pt the box [60, 90, 76, 104] around one caption pixel of the page behind, (66, 96).
- `keeps-edge-*`: below y 590 at 13 pt and y 575 at 18 pt, outside the candidate's panel interior, the refused save changes 0 px on the candidate, so its lower border, the gap and the bar stay put. On the base 11,198 and 14,323 px change at 13 pt (Midnight, Porcelain) and 17,600 and 21,621 px at 18 pt as the footer grows the panel.

The pixel at (66, 96) varies between launches of one build and has nothing to do with the change. It lies in the Catppuccin Mocha card's caption on the Settings page, which the change does not touch. In attempt 1's first run (`$EVIDENCE/runs/theme-editor-panel-bounds-1`) it was the only difference between the builds in the three 13 pt Porcelain compares, already on the Settings page before the editor opened. A separate launch of the same build drew the other value. In run 3 it again differs by one unit in both palettes at 13 pt: (44, 44, 60) against (44, 45, 60), and (52, 52, 71) against (52, 53, 71). The 13 pt compares therefore mask that one box and still count its pixels as masked. Attempt 1's first run also matched a privacy template on the 13 pt `end` and `save-error` crops, at the word "informational" in the app's own token description "Hunk headers, links and informational messages." Viewed at full size, it is app copy, not personal data, and the `lower` crop starts at y 392, below that row in both builds.

Frames in [`evidence/theme-editor-panel-bounds/`](evidence/theme-editor-panel-bounds/), 36 crops:
- 24 crops of the window's lower part, 1000 × 288 at (0, 392), each in Midnight and Porcelain at 13 and 18 pt:
  - `base-{midnight,porcelain}-{13,18}pt-1000x680-rest.png`: on the base, the editor at rest. Its panel ends 16 px above the window and across the status bar, at 18 pt through the bar's text.
  - `base-…-end.png`: on the base, the body scrolled to its end (the preview card and Readability), the panel still across the bar.
  - `base-…-save-error.png`: on the base, the refused save's error on two lines beside Cancel and Save. The footer grows and the panel's border moves to y 673 at 13 pt and y 678 at 18 pt, covering most of the bar.
  - `candidate-{midnight,porcelain}-{13,18}pt-1000x680-rest.png`: on the candidate, the footer as the form's last row and the panel's border 16 px above the bar's rule. The bar's message and hints are whole.
  - `candidate-…-end.png`: on the candidate, the body at its end above the footer, with the same 16 px.
  - `candidate-…-save-error.png`: on the candidate, the error on two lines in the footer and the body shorter by the footer's growth. The panel's edge and the 16 px are unchanged.
- 12 crops of the candidate's footer with a focused button, in Midnight and Porcelain, 646 × 104 at (177, 576) at 13 pt and 877 × 132 at (62, 548) at 18 pt, each showing the footer, the panel's lower border and the status bar's rule:
  - `candidate-{midnight,porcelain}-{13,18}pt-1000x680-save-focus.png`: Save holding keyboard focus, its ring whole on all four sides, 13 px of panel below it to the border and the bar 16 px below that.
  - `candidate-…-cancel-focus.png`: Cancel holding focus, its ring whole.
  - `candidate-…-reset-focus.png`: Reset to base holding focus, its ring whole.

The base's focused footer frames were captured and measured (whole rings) but are not committed.

`qa.py privacy scan --redacted --jobs 4` with the local template set found all 36 committed files clean (21.5 s wall), as did the run's own scan of the same bytes (21.3 s). Each frame was also viewed at full size.

After the save error, the candidate's body keeps its scroll offset while it shrinks. A body scrolled to its end therefore shows its last line, "Every readability rule is met.", cut at the body's new lower edge until it is scrolled again, at both sizes and in both palettes. The design review (14:45 UTC) ruled this ordinary scroll-region behaviour: the footer's growth goes to the body by decision, focus stays in Name, and the line stays reachable by scrolling.

The Reflog does not follow the rule; this is arithmetic from the code, not a measurement. `ReflogBrowser` caps its content at `px(590.).min(body_height)` plus the ring room, with `body_height` the viewport less 240 px (`reflog.rs:318`). That reserves neither the scaled status bar nor the client-decoration frame. Take the alert's title as one rem and its Done button as the toolkit's `h_8` (2 rem). In a 1000 × 680 window with server decorations, the panel then ends about 51 px above the bar at 13 pt and 23 px at 18 pt. At 18 pt the clearance falls below 16 px for window heights between about 750 and 840 px, to about 8 px near 830 px. Under untiled client decorations at 13 pt and 680 px it is about 14 px: under the rule, but not across the bar. These figures depend on the title's line height and the button's height, which a native measurement would settle; the Reflog is unchanged.

Not covered: native Wayland with client decorations, the `window_paddings` case, was not exercised, because `qa.py` launches only X11 clients and removes `WAYLAND_DISPLAY`. That case rests on `the_cap_keeps_the_panel_inside_client_decorations`. The footer buttons' focus was captured at rest only; with the save error it rests on `a_focused_footer_buttons_ring_lies_inside_the_body_in_every_state`. Edit theme, other window sizes and text sizes, macOS and fractional scale factors are not covered natively.

## October 1 Tab reveals branch, remote and tag rows

Task `tab-reveals-branch-and-tag-rows`. When keyboard focus moves onto a row that lies wholly or partly outside the branch chooser, the remote manager, Tags or the tag inspector's Push to… list, the list scrolls by the least amount that shows the whole row and its ring, to the nearest edge and without animation; a pointer press scrolls nothing. The project hub keeps its reveal through the same shared helper.

Automated, `cargo test --locked -p gitturtle`, each in a 1000 × 680 window with the installed ring:
- `branch_actions::tests::tab_reveals_every_branch_chooser_row`: 30 branches. Real `tab` keystrokes go from the filter through every row and `shift-tab` keystrokes come back; after each key the focused row, grown by the ring's gap plus width, lies inside the list's bounds, and at both ends inside the content mask it paints in. With the list then scrolled by hand so a row lies across its lower edge, two redraws and a click on that row leave the scroll and focus where they were.
- `branch_actions::tests::tab_reveals_every_remote_manager_control`: the same through every Edit… and Remove… of 12 remotes on local bare repositories.
- `tags::tests::tab_reveals_every_tag_row`: the same through 30 tags.
- `tags::tests::tab_reveals_every_push_destination`: the same through Push to… for 10 remotes on local bare repositories.

With the lists' reveal disabled, as on the base, all four fail at the first control past the list's lower edge (`branch-choice-9`, `edit-remote-4`, `tag-row-9`, `push-tag-remote-5`). The project hub's `keyboard_focus_reveals_project_fields_and_submit_at_small_window`, `branch_chooser_keeps_room_for_rings_at_either_end`, `tag_browser_keeps_room_for_every_focus_ring` and `tag_inspector_keeps_room_for_every_push_ring` pass unchanged.

Native evidence, full tier:
- **Builds:** base `ee87282` (the run's `accepted_head`, the attested release of `reflog-selection-and-scroll-cues`; sha256 `2f6c30d8…`) and candidate `a65720f` (sha256 `0b6bd46c…`), release builds from clean trees, each in its own target directory. `qa.py identity` reported no problem.
- **Host:** Ubuntu 26.04, GNOME 50, XWayland `:0` at scale factor 1, window 1000 × 680, default text size, one session on 2026-10-01 from 23:36 to 23:46 UTC.
- **Input:** a local driver on the `qa.py` library (sha256 `379e49a9…`), through Mutter RemoteDesktop with X focus verified, never XTest.
- **Fixture:** a disposable repository with 30 local branches, 30 tags and a local bare `origin`. Its state was byte-identical before and after the session, and no write control was activated.
- **Steps, the same keys in both builds:** the branch chooser opened from the branch button's "Manage another branch…", Tab 2 to its first row, Tab 9 to `topic-14` (the first row wholly below the list's lower edge), then Tab 11 and Shift+Tab 9 to `topic-24`, and Shift+Tab 3 more; Tags opened from the command palette, Tab 3 to its first row, Tab 10 to `v1.10`, then Tab 11 and Shift+Tab 10 to `v1.11`, and Shift+Tab 3 more.

The candidate shows the focused row with its ring whole, the ring 0 px inside the list's clip on the revealed edge, in both lists and both palettes (11.43:1 Midnight, 6.55:1 Porcelain against the list's surface). It scrolls the least amount on every one of 142 keys: one 37 px row per key past an edge, 7 px onto a partly visible row, and nothing across rows already in view. The base never scrolls, so the focused row stays out of view: below the lower edge after Tab, and still below it after Shift+Tab, since the list never moved; three more Shift+Tabs bring focus back into view on row 8, which confirms focus stayed in the list. Nothing outside the lists moves in either build, and a pointer press on a partly visible Tags row (candidate, Midnight) scrolls nothing.

The reveal is applied one frame after the focus change, as the project hub's reveal it shares already was: a fast probe saw the newly focused row unrevealed, its ring cut at the list's edge, for one 6 to 12 ms frame on 2 of 8 scrolling keys before the list moved. Coordinator decision, with the design review: accept it here, since it never persists and the contract keeps the hub's behaviour unchanged, and queue a follow-up that reveals before the frame is painted, for every list and the hub, with a frame-by-frame probe finding no cut ring.

Frames in [`evidence/tab-reveals-branch-and-tag-rows/`](evidence/tab-reveals-branch-and-tag-rows/), 20 crops of the dialog, the chooser 540 × 490 at (230, 68) and Tags 600 × 521 at (200, 68):
- `base-{midnight,porcelain}-1000x680-branch-chooser-tab-down.png`: on the base, `topic-14` focused below the lower edge, only a 3 px line of its ring's top showing at the list's edge;
- `base-{midnight,porcelain}-1000x680-branch-chooser-shift-tab-up.png` and `base-…-tags-tab-down.png` and `…-tags-shift-tab-up.png`: on the base, the focused row out of view and no ring in the list (the two Tags frames are identical);
- `base-{midnight,porcelain}-1000x680-{branch-chooser,tags}-shift-tab-confirm.png`: on the base, three Shift+Tabs later, row 8 focused at the unscrolled list;
- `candidate-{midnight,porcelain}-1000x680-branch-chooser-tab-down.png` and `…-tags-tab-down.png`: on the candidate, `topic-14` and `v1.10` revealed at the lower edge with the ring whole;
- `candidate-{midnight,porcelain}-1000x680-branch-chooser-shift-tab-up.png` and `…-tags-shift-tab-up.png`: on the candidate, `topic-24` and `v1.11` revealed at the upper edge with the ring whole.

`qa.py privacy scan --redacted` with the local template set found all 20 clean on their committed bytes. A `design-reviewer` pass approved the frames. Also queued from it: DESIGN.md's sentence should name every keyboard focus move, not only Tab, and give the project hub's reveal on a click its own sentence; and neither list shows a scroll cue once scrolled. DESIGN.md now names every keyboard focus move and gives the project hub's reveal on a click its own sentence; the scroll cue remains queued.

Not covered natively: the remote manager and the Push to… list (their view tests cover them), other text and window sizes, macOS, native Wayland and fractional scale factors.

Not covered: the project hub's recent projects are a virtualized `uniform_list` (`recent-projects`), whose unrendered rows Tab cannot reach at all, so they are unchanged; the other virtualized lists (History, the file lists, the command palette, the project pane) keep one tab stop and selection keys. A list scrolls itself only: the kit dialog keeps its body's scroll handle to itself, so a dialog body that overflows at large text sizes is not scrolled to show the list. The nested worktree, Reflog and profile lists follow in `tab-reveals-worktree-reflog-profile-rows`; the GitHub review panel keeps its own copy of the reveal.

## October 1 a marked Reflog selection and cues for what scrolls

Task `reflog-selection-and-scroll-cues`. In the Reflog, the selected entry now paints the selected surface; the dialog's scrolling content shows the always-visible vertical scrollbar of the discard and worktree removal reviews whenever it overflows; and the changed-file list shows whole rows only, as many as fit in 110 px at 13 pt (5), kept as that row count at every text size, with the same scrollbar when it holds more files.

Native evidence, full tier:
- **Builds:** base `7e1caa9` (the run's `accepted_head`, the attested release of `worktree-rows-ring-and-selection`; sha256 `536b8123…`) and candidate `2400891` (sha256 `4d451355…`), release builds from clean trees, each in its own target directory. `qa.py identity` reported no problem.
- **Host:** Ubuntu 26.04, GNOME 50, XWayland `:0` at scale factor 1, window 1000 × 680, one session on 2026-10-01 from 22:33 to 22:40 UTC.
- **Input:** a local driver on the `qa.py` library (sha256 `0fc29d90…`), through Mutter RemoteDesktop with X focus verified, never XTest.
- **Fixture:** a disposable `gallery` repository with 14 HEAD reflog entries, 16 tags and a local bare `origin`. `HEAD@{0}` points at a commit that changes 9 files whose paths all have descenders; `HEAD@{1}` changes 3. Its state was byte-identical before and after the session, and no write control was activated.
- **Steps:** the command palette's "browse reflog", a click on the entry, the pointer parked outside the window, then the mouse wheel over the dialog's explanation until the content stopped moving. Each launch's store seeded Midnight or Porcelain at 13 or 18 pt.

Measured, the same in both palettes unless named:

| | Base | Candidate |
| --- | --- | --- |
| Selected entry's fill against the dialog | none (1.00:1) | `selected`: 1.53:1 Midnight, 1.18:1 Porcelain |
| Content scrollbar at rest and at the end | none | a thumb at the top, then at the bottom |
| File list at 13 pt, 9 files | 110 px; 5 whole rows, then the 6th cut through its descenders; no scrollbar | 95 px, 5 whole 19 px rows, and a thumb in its gutter |
| File list at 18 pt, 9 files | 110 px; 4 whole rows and no cue for the other 5 | 135 px, 5 whole 27 px rows, and a thumb |
| File list at 13 pt, 3 files | 3 rows, no scrollbar | the same, identical |

With nothing selected at 13 pt, `qa.py compare --mask status-timing` finds both builds identical. With the entry selected they differ only in the fill and the thumbs, and in the end frames the content above the file list sits 15 px higher, the list's shorter bound.

Coordinator decisions, with the design review: the Porcelain selected fill stays at 1.18:1, above the 1.15:1 floor DESIGN.md sets for selected surfaces and the same `selected` the worktree rows and other lists use; and the file list growing to 135 px at 18 pt is the contract's row count, cued by the content's scrollbar. File rows no longer wrap, so a long path now ends in an ellipsis without a tooltip; no captured path was long enough to show it. Queued as follow-ups: a tooltip with the full path on each file row, keeping the file name visible; the 2 px accent leading marker DESIGN.md gives a selected row, for the Reflog and worktree lists together; and the same scrollbar for the Reflog's entry list, which shows 6 of 14 entries without a cue.

Frames in [`evidence/reflog-selection-and-scroll-cues/`](evidence/reflog-selection-and-scroll-cues/), 14 crops of the Reflog dialog, 740 × 535 at (130, 68) at 13 pt and 740 × 553 at 18 pt:
- `base-{midnight,porcelain}-13pt-1000x680-reflog-selected-rest.png`: on the base, `HEAD@{0}` selected without a surface, and no scrollbar;
- `base-{midnight,porcelain}-13pt-1000x680-reflog-selected-end.png`: on the base, scrolled to the end, the file list's 6th row cut through its descenders, and no scrollbar;
- `candidate-{midnight,porcelain}-13pt-1000x680-reflog-selected-rest.png`: on the candidate, the selected fill on `HEAD@{0}` and the content's thumb at the top;
- `candidate-{midnight,porcelain}-13pt-1000x680-reflog-selected-end.png`: on the candidate, scrolled to the end, 5 whole file rows with the list's thumb, and the content's thumb at the bottom;
- `base-{midnight,porcelain}-18pt-1000x680-reflog-files.png`: on the base at 18 pt, 4 file rows and no cue for the other 5;
- `candidate-{midnight,porcelain}-18pt-1000x680-reflog-files.png`: on the candidate at 18 pt, 5 whole rows with the list's thumb, and the content's thumb;
- `base-midnight-13pt-1000x680-reflog-files-few.png` and `candidate-…-files-few.png`: `HEAD@{1}` with 3 files, no file-list scrollbar in either build, and on the candidate the content's thumb.

`qa.py privacy scan --redacted` with the local template set found all 14 clean on their committed bytes. A `design-reviewer` pass approved the frames.

Not covered: keyboard focus and hover on the selected entry (the ring room is covered by `reflog_browser_keeps_room_for_every_focus_ring`), truncated paths, 11 pt, macOS, native Wayland and fractional scale factors.

## October 1 worktree rows keep the ring clear and selection off the accent

Task `worktree-rows-ring-and-selection`. Rows in the worktree manager's list of worktrees now stand apart by the ring room plus 2 px (5 px at the default ring), so a focused row's ring keeps 2 px of the list's surface before its neighbours' borders. A selected row keeps the neutral border with its selected fill and selected and toggled state, so an accent outline at rest no longer means selection.

Native evidence, full tier:
- **Builds:** base `f84b2ab` (the run's `accepted_head`, the attested release of `worktree-branch-choices-ring`; sha256 `3cf85eb9…`) and candidate `b8f4030` (sha256 `b851901c…`), release builds from clean trees, each in its own target directory. `qa.py identity` reported no problem.
- **Host:** Ubuntu 26.04, GNOME 50, XWayland `:0` at scale factor 1, window 1000 × 680, default text size, one session on 2026-10-01 from 21:06 to 21:09 UTC.
- **Input:** a local driver on the `qa.py` library (sha256 `845f79da…`), through Mutter RemoteDesktop with X focus verified, never XTest.
- **Fixture:** a disposable repository on `main` with three linked worktrees, `alpha`, `beta` and `gamma`, and a local bare `origin`, so the manager lists four rows without scrolling and opens with `main` selected. Its state was byte-identical before and after the session, and no write control was activated.
- **Steps:** the command palette's "manage worktrees", then Tab 5 to the selected row, `main`, and Tab 1 more to the unselected row `alpha`.

Measured on the focused frames, the same in both palettes:

| | Base | Candidate |
| --- | --- | --- |
| Gap between rows | 3 px | 5 px |
| Surface between a focused row's ring and the next row's border | 0 px (above and below) | 2 px (above and below) |
| Accent outlines on each edge of the focused selected row | 2 (accent border and ring) | 1 (the ring; the neutral border sits inside the 1 px gap) |
| Accent outlines on each edge of a focused unselected row | 2 | 2 |

The ring is present on all four sides of every focused row, its strongest pixel 11.43:1 (Midnight) and 6.55:1 (Porcelain) against the same pixel unfocused. A focused unselected row still shows the ring plus an accent border in both builds: the kit's `apply_focus_ring` turns a focused Button's border to the ring colour, and only the selected state's styling puts the row's own border back. Coordinator decision, with the design review: accept this candidate on its contract, since an accent outline now always means focus, and queue the kit change (a focused Button keeps its own border, so the ring alone shows focus, as DESIGN.md:23 asks) as a follow-up that reaches every bordered Button.

Unfocused, `qa.py compare --mask status-timing` finds the expected differences: `main`'s border changes from accent to neutral, rows 1 to 3 move down 2, 4 and 6 px and are otherwise identical, and the details below and the dialog's bottom move down 6 px. The design review accepted the 6 px: the list's height cap is unchanged, and 5 px between rows still reads well below the gap between sections.

Frames in [`evidence/worktree-rows-ring-and-selection/`](evidence/worktree-rows-ring-and-selection/), 12 crops of the list, 688 × 172 at (156, 170):
- `base-{midnight,porcelain}-1000x680-worktree-rows-rest.png`: on the base, `main` selected with an accent border and 3 px between rows;
- `base-{midnight,porcelain}-1000x680-worktree-selected-focus.png`: on the base, the focused selected row with two accent outlines, its ring touching `alpha`'s border;
- `base-{midnight,porcelain}-1000x680-worktree-row-focus.png`: on the base, the focused `alpha` row, its ring touching `main`'s and `beta`'s borders;
- `candidate-{midnight,porcelain}-1000x680-worktree-rows-rest.png`: on the candidate, `main` selected with the neutral border and 5 px between rows;
- `candidate-{midnight,porcelain}-1000x680-worktree-selected-focus.png`: on the candidate, the focused selected row with one ring, 2 px clear of `alpha`;
- `candidate-{midnight,porcelain}-1000x680-worktree-row-focus.png`: on the candidate, the focused `alpha` row, 2 px clear of both neighbours, still with the kit's accent border inside its ring.

`qa.py privacy scan --redacted` with the local template set found all 12 clean on their committed bytes. A `design-reviewer` pass approved the frames on the contract and raised one more follow-up, already present on the base: the unselected rows hover with the kit's ghost fill rather than the palette's hover surface, which in Porcelain (1.30:1 against the dialog) outweighs the selected fill (1.18:1).

Not covered: other text and window sizes, hover frames, macOS, native Wayland and fractional scale factors.

## October 1 whole focus rings in the worktree branch choices

Task `worktree-branch-choices-ring`. The worktree manager's list of branch choices, shown while creating a worktree from an existing branch, now keeps the installed ring room inside its scrolling clip and gives it back through its margins, as #122 did for the other lists, so a focused first or last choice keeps its ring whole and no control moves.

Native evidence, full tier:
- **Builds:** base `6906a24` (the run's `accepted_head`, the attested release of `tab-strip-ring-room-small-text`; sha256 `a819ff65…`) and candidate `b8c8d05` (sha256 `ba3bb8d7…`), release builds from clean trees, each in its own target directory. `qa.py identity` reported no problem.
- **Host:** Ubuntu 26.04, GNOME 50, XWayland `:0` at scale factor 1, window 1000 × 680, default text size, one session on 2026-10-01 from 20:19 to 20:24 UTC.
- **Input:** a local driver on the `qa.py` library (sha256 `78f03b0c…`), through Mutter RemoteDesktop with X focus verified, never XTest.
- **Fixture:** a disposable repository with 14 local branches on distinct commits, a detached HEAD so no choice is in use, no linked worktree and a local bare `origin`; the form lists the newest 12, `main` through `topic-11`. Its state was byte-identical before and after the session, and nothing was created.
- **Steps:** the command palette's "manage worktrees", Tab 2 and Space for Create worktree…, Tab 2 and Space for Existing branch, then Tab 3 to the first choice. For the last choice, the mouse wheel scrolled the list to its end, then Tab 11 reached `topic-11`; Tab does not scroll a choice into view in either build (queued as `tab-reveals-worktree-reflog-profile-rows`).

The base keeps only the side of the ring that faces into the list: the first choice's bottom and the last choice's top. The candidate draws it on all four sides. Ring pixels in the 1 to 3 px band outside each edge, corners excluded (top / right / bottom / left), each against the same launch's unfocused frame at the same scroll position, the same in both palettes:

| | Base | Candidate |
| --- | --- | --- |
| First choice, list at its top | 0 / 0 / 1338 / 0 | 1338 / 62 / 1338 / 62 |
| Last choice, list at its end | 1338 / 0 / 0 / 0 | 1338 / 62 / 1338 / 62 |

The strongest ring pixel on every present side stands 11.43:1 (Midnight) and 6.55:1 (Porcelain) against the same pixel unfocused. The choices' boxes are identical in both builds.

Unfocused, `qa.py compare --mask status-timing` finds the form identical to the base apart from 53 px in both palettes: the list's clip now reaches the ring room, 3 px past its old edges, so the partly scrolled row at the edge shows 3 more pixel rows of its label (`topic-03` at the bottom, 11 of 12 rows instead of 8; at the end, `topic-08` at the top whole instead of 10 of 12). Coordinator decision, with the design review: accept this inset. The band is exactly where the ring paints, so the visible area cannot stay at its old bounds without cutting the ring again; the partial labels now read as whole words rather than being cut through the letters; and every #122 list shows the same 3 px.

Frames in [`evidence/worktree-branch-choices-ring/`](evidence/worktree-branch-choices-ring/), 12 crops of the create form, 688 × 231 at (156, 135):
- `base-{midnight,porcelain}-1000x680-branch-choice-first-focus.png`: on the base, the focused first choice, `main`, with only the bottom of its ring;
- `base-{midnight,porcelain}-1000x680-branch-choice-last-focus.png`: on the base, the focused last choice, `topic-11`, at the list's end, with only the top of its ring;
- `candidate-{midnight,porcelain}-1000x680-branch-choice-first-focus.png` and `…-last-focus.png`: on the candidate, the same choices with the ring and its gap whole on all four sides;
- `base-{midnight,porcelain}-1000x680-branch-choices-rest.png` and `candidate-…-rest.png`: the unfocused form, focus on Existing branch, where the candidate's bottom row shows 3 more pixel rows of `topic-03`.

`qa.py privacy scan --redacted` with the local template set found all 12 clean on their committed bytes. A `design-reviewer` pass approved the frames and the inset.

Not covered: other text sizes and window sizes, macOS, native Wayland and fractional scale factors.

## October 1 the tab strip keeps the ring room at small text

Task `tab-strip-ring-room-small-text`. The repository tab strip's minimum height is now the larger of `ui_size(36)` and a tab or close button plus twice the installed ring room, so a focused tab's ring keeps its gap inside the window at every text size; nothing moves where the room already fitted (12 pt and above at desktop scale 1.0).

Native evidence, full tier:
- **Builds:** base `2197371` (the candidate's parent; sha256 `1f9776e3…`) and candidate `399062e` (sha256 `2bffdda5…`), release builds from clean trees, each in its own target directory. `qa.py identity` reported no problem.
- **Host:** Ubuntu 26.04, GNOME 50, XWayland `:0` at scale factor 1, window 1000 × 680, one session on 2026-10-01 from 19:17 to 19:22 UTC.
- **Input:** a local driver on the `qa.py` library (sha256 `36a5dbd2…`), through Mutter RemoteDesktop with X focus verified, never XTest.
- **Fixture:** a disposable `tabs` fixture with `alpha`, `beta` and `gamma` open as tabs, unchanged by the session; each launch's store seeded Midnight or Porcelain at 11 or 13 pt.
- **Steps:** Tab 7 from the launch focus reaches the first tab and Tab 8 its close button.

At 11 pt (an interface scale of 0.846) the base draws the tab from y 2, so the ring's top rows, y 0 and 1, touch the tab's fill and part of its top band falls outside the window. The candidate's strip is 1 px taller: ring, 1 px gap, fill, 1 px gap, ring, then the strip's rule, the same on all four sides, and everything below moves down 1 px at 11 pt only. Ring pixels found in the 1 to 3 px band (top / right / bottom / left), the same in both palettes:

| 11 pt | Base | Candidate |
| --- | --- | --- |
| First tab | 78 / 55 / 90 / 55 | 90 / 56 / 90 / 56 |
| Close button | 44 / 55 / 56 / 55 | 56 / 56 / 56 / 56 |

The ring stands 10.45:1 (Midnight) and 7.01:1 (Porcelain) off the panel. At 13 pt, `qa.py compare --mask status-timing` finds the rest, tab and close-button frames identical to the base in both palettes.

Frames in [`evidence/tab-strip-ring-room-small-text/`](evidence/tab-strip-ring-room-small-text/), 10 strip crops 1000 px wide:
- `base-{midnight,porcelain}-11pt-1000x680-strip-tab-focus.png` and `…-strip-tab-close-focus.png`: on the base, the focused first tab and its focused close button, whose ring meets the control's fill at the top;
- `candidate-{midnight,porcelain}-11pt-1000x680-strip-tab-focus.png` and `…-strip-tab-close-focus.png`: on the candidate, the same controls with the ring and its gap whole on all four sides;
- `candidate-{midnight,porcelain}-13pt-1000x680-strip-rest.png`: the unfocused strip at 13 pt, byte-identical to the base's crop.

`qa.py privacy scan --redacted` with the local template set found all 10 clean on their committed bytes. A `design-reviewer` pass approved the frames. At 11 pt the ring's bottom row sits on the strip's rule, as its top row sits on the window edge at every size. The rule is the strip's own edge, and the ring stands 7.53:1 (Midnight) and 4.67:1 (Porcelain) off it, so no surface row is added. A row there would grow the strip at 12 pt too.

Not covered: desktop text scales below 1.0, which only the GPUI test `tab_strip_keeps_every_focus_ring_inside_the_window_at_every_text_size` reaches, other text sizes, the last tab, macOS, native Wayland and fractional scale factors.

## October 1 disabled Buttons report disabled to AT-SPI

Task `atspi-disabled-state`, the owner's request of 2026-09-30, from the open items of [Tab leaves a focused Button that turns disabled](#september-30-tab-leaves-a-focused-button-that-turns-disabled). On Linux every disabled Button reported `enabled` and `sensitive` to AT-SPI. The pinned `accesskit_atspi_common` 0.19.1 added `Enabled | Sensitive` to every node unless its role supports read-only and the node was read-only or disabled (`src/node.rs:373-377`), and `accesskit_consumer` 0.38.0 does not count Button among those roles. A Switch, CheckBox or text input carrying that flag reported `read-only` instead; in the app only the kit's text input sets it besides Button (open items below). So the AccessKit disabled flag that the [gpui-base patch](native-accessibility.md#toolkit-patch) sets never reached AT-SPI.

No released GPUI stack carries AccessKit's fix (#788, in `accesskit_atspi_common` 0.21.0): `gpui-pre-linux` 0.3.7 still requires `accesskit_unix ^0.22`. The [research note](development/2026-10-01-accesskit-atspi-disabled.md) compares the options. `vendor/accesskit_atspi_common` now holds the published 0.19.1 source with only upstream commit `6ee0558b` applied to `src/node.rs`, selected through `[patch.crates-io]` ([patch record](../vendor/accesskit_atspi_common/GITTURTLE-PATCH.md)). A disabled node reports none of `enabled`, `sensitive` or `read-only`; enabled nodes keep `enabled` and `sensitive`. Every other version in `Cargo.lock` is unchanged.

Test: `native_accessibility::atspi_state_tests::disabled_controls_are_neither_enabled_sensitive_nor_read_only` (Linux only) maps an AccessKit tree with an enabled Button, a disabled Button, a focused disabled Button, a disabled Switch, a disabled CheckBox, a disabled text input and an enabled read-only text input through the adapter's `PlatformNode::state()`, which `accesskit_unix` serves as AT-SPI `GetState`. It requires the disabled ones to carry none of `Enabled`, `Sensitive` or `ReadOnly`, the enabled one to keep `Enabled` and `Sensitive`, the focused disabled one to keep `Focusable` and `Focused`, and the read-only text input to stay `ReadOnly`. `cargo test --locked -p gitturtle --bin gitturtle native_accessibility::atspi_state_tests` on Ubuntu 26.04 fails with the registry crate (no `[patch.crates-io]` entry): "disabled Button reports Enabled", whose state set is `Enabled | Sensitive | Showing | Visible`. With the vendored backport it passes (2026-10-01).

Native evidence, 2026-10-01 from 18:29 to 18:30 UTC: release builds base `ae55fc4` (sha256 `a441c5ee…`) and candidate `f1d95a1` (`10fe09f2…`), both clean; `qa.py identity` reported no problem. Ubuntu 26.04, GNOME 50, XWayland `:0` at scale factor 1, 1000 × 680, Midnight; input went through Mutter RemoteDesktop with verified X focus, never XTest. Each launch opened a fresh repository with `main`, a `feature` branch at the same commit and no remote, and followed the September 30 route: Tab 17 to Targets, Space, `feature`, Tab to Switch, Space. A local driver read the app's AT-SPI tree straight from the accessibility bus five times per launch, Settings included. On the base, Create, Switch (unfocused, and focused after the switch), Fetch, Pull and Push, Previous, Older and seven disabled Settings Buttons reported `enabled` and `sensitive`; on the candidate they reported neither. Every other node matched across builds, the focused disabled Switch button kept `focusable` and `focused`, and the eight context frames were byte-identical between builds (not committed). `org.a11y.Status IsEnabled` was set for those launches only and read back `false` after each.

Open items: Settings' always-disabled Commit message CheckBox reported `checkable checked enabled sensitive` on both builds and never `read-only`, because gpui-base's Checkbox and Switch set no AccessKit disabled flag (only Button, `vendor/gpui-base/src/button.rs`, and the text input, `vendor/gpui-base/src/input/base/state.rs`, call `set_disabled()`), so disabled kit check boxes and switches still read as enabled on Linux. A disabled text input, which reported `read-only` on the base, now reports none of `read-only`, `enabled` or `sensitive`; the unit test covers it, but the native read did not reach one.

Not covered: VoiceOver on macOS (the macOS adapter is untouched) and Orca speech, which no run here exercises; Windows.

## October 1 authentication guidance without macOS Keychain on Linux

Task `auth-prompt-platform-wording` (Omarchy finding B2, from [September 30 Git writes and network actions on Omarchy](#september-30-git-writes-and-network-actions-on-omarchy); the owner widened its scope on 2026-10-01 to the Git failure diagnostics). On Linux the Git authentication prompt told the user that their credential helper may save the response "including in macOS Keychain". Off macOS the prompt's guidance and its accessible label, which share one string, now read "Requested by the Git operation you started. Your configured credential helper may save this response. GitTurtle does not save it." On macOS the text is unchanged. The failure diagnostics in `crates/git-core/src/work/diagnostics.rs` follow the same rule. Off macOS, the SSH key diagnostic drops its `UseKeychain` clause, and the HTTP authentication diagnostic gives "Git Credential Manager or a Secret Service helper" as its example instead of "Git Credential Manager or macOS osxkeychain". `authentication::tests::pending_credential_prompt_names_keychain_only_on_macos` and `work::diagnostics::tests::authentication_guidance_names_macos_stores_only_on_macos` assert the exact sentences on each platform. The other Keychain strings in the app (the GitHub account notices and errors) are macOS-only code, or follow a credential save that always fails off macOS.

Native evidence, light tier, on Omarchy 4.0.4 (Hyprland 0.56.2, native Wayland, Git 2.55.0):
- Builds: release builds, clean, in one session. The base is `60cd48b` (`gitturtle-60cd48b`, sha256 `59dffd49…`) and the candidate `c445081` (sha256 `43896199…`). The branch's later commits change only tests, the user guide, the evidence and this entry.
- Output: a temporary 1480 × 800 headless output at scale 1, with the window fullscreen. Each launch had a fresh HOME with the `GitTurtle QA` identity and fresh XDG directories.
- Fixture: a disposable repository whose `origin` is a smart-HTTP remote served by `git http-backend` on 127.0.0.1 under throwaway Basic credentials.
- Steps: Fetch raised the username prompt. The run entered a username and a wrong password, and the failure's Details were copied as text.
- Palettes: Midnight and Daylight.
- Comparison (`qa.py compare --mask status-timing`, plus the top 26 px strip where Omarchy's bar can draw): with the prompt open, only the guidance's second line (`[564, 171]`–`[885, 183]`) and the text caret differ. The opening frame, the failure banner and the Details dialog are pixel-identical. The copied diagnostic names osxkeychain on the base and a Secret Service helper on the candidate.
- Credentials: no launch's HOME contains the throwaway password.
- Frames and records: the prompt frames and the copied diagnostics, base and candidate in both palettes, are in [`evidence/auth-prompt-wording/`](evidence/auth-prompt-wording/). A privacy scan of the four committed frames with the local template set came back clean, and each frame was viewed at full size.

A `design-reviewer` pass approved the frames: the shorter guidance still wraps to two whole lines, and the dialog keeps its size in both palettes. The Details viewer does not wrap, so the changed diagnostic is evidenced by the copied text rather than by a frame.

Not covered: macOS, where the text is unchanged and checked only by reading; the SSH key diagnostic, natively; XWayland; and X11.

## October 1 Tab leaves a focused kit control that turns disabled

Task `gpui-base-sibling-focus-traps`, the owner's request of 2026-09-30, from the open items of [Tab leaves a focused Button that turns disabled](#september-30-tab-leaves-a-focused-button-that-turns-disabled). gpui-base's Checkbox, Switch, Radio, Toggle, Link and ColorPicker swatch tracked their focus handle only while enabled. One that turned disabled while it held focus left the rendered frame with the window's focus still on it, and Tab and Shift+Tab did nothing until a pointer click. Each now tracks its handle through `disabled_focus::track_control_focus` (`vendor/gpui-base/src/disabled_focus.rs`):
- enabled, as upstream;
- disabled and focused, as a target that is not a tab stop;
- disabled and unfocused, not at all.

This is #102's rule for the component Button. `vendor/gpui-base/GITTURTLE-PATCH.md` records it, and gpui-component's wrappers are unchanged.

Tests: three tests send real `tab` and `shift-tab` keystrokes and require each key to reach a neighbouring tab stop:
- `tab_and_shift_tab_leave_follow_system_once_omarchy_disables_it` (`settings.rs`, Linux only) focuses Settings' Follow system Switch and then selects the Omarchy theme;
- `tab_and_shift_tab_leave_the_directory_checkbox_while_the_rule_is_prepared` (`ignore.rs`) disables the ignore dialog's Checkbox while its rule is prepared;
- `tab_and_shift_tab_leave_focused_kit_controls_that_turn_disabled` (`native_accessibility/control_tests.rs`) does the same for a bare Radio, Toggle, Link, ColorPicker swatch, Checkbox and Switch, and requires Tab to skip each one while it is disabled and unfocused.
With the candidate's sources, `cargo test --locked -p gitturtle -- tab_and_shift_tab_leave` passes all three and #102's Targets test. With `vendor/gpui-base/src` taken from `27177da` and everything else unchanged, all three fail at their first Tab after the disable ("Tab leaves the disabled …"), while #102's test still passes (2026-10-01, 16:47 to 16:50 UTC). A `code-reviewer` pass over the vendor patch found no defect. It checked the helper and its six call sites against the pinned gpui-pre 0.3.4, the patch notes, and the unchanged `Cargo.toml`, `Cargo.lock` and license files. Non-blocking:
- the patch note does not say that a focused disabled control now also takes a caller's focus styles, reports AccessKit focus and keeps a mouse-down's focus from reaching an ancestor;
- the Settings test runs on Linux only;
- no test presses Enter or Space on a focused disabled control.

Native evidence, full tier, since the base had to show the trap in the same session as the fix. It is keyboard only, as the change moves focus and no pixel of any control:
- **Builds:** base `27177da` (sha256 `20744763…`), the candidate's parent, and candidate `b64d919` (sha256 `4898ca9f…`). Both are release builds from clean trees, each in its own `CARGO_TARGET_DIR`, and `qa.py identity` reported no problem. The controller rebuilds the candidate on the commit that adds this entry. Its native attestation re-checks the rebuilt executable.
- **Host:** Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680. One session on 2026-10-01 from 16:33 to 16:44 UTC ran a rehearsal, a dry run, 4 paired launches and 4 AT-SPI launches.
- **Input:** local drivers on the `qa.py` library (`capture.py`, sha256 `82bbba25…`, on `sfl.py`, `3604f234…`) sent every input through Mutter RemoteDesktop, never XTest. X focus was checked before every key, and the pointer was confirmed on the window before the click.
- **Fixture:** a disposable repository with one commit under the QA identity (HEAD `3e027be`), unchanged by every launch.
- **Omarchy theme:** each launch's own HOME carried one at `.local/state/omarchy/current`, with the colours of the Tokyo Night test fixture and `theme.name` `tokyo-night`, so Settings offered "Omarchy · Follows Tokyo Night". The store started on Midnight with Follow system off.
- **Route:**
  - Ctrl+comma opens Settings with nothing focused. Tab 6 focuses Back to repository, Tab 7 the Follow system Switch and Tab 8 the Omarchy card, on both builds, each step within 0.031 to 0.050 s.
  - With the Switch focused, a pointer click on the Omarchy card selects it. The palette becomes Tokyo Night, the Switch turns disabled, and keyboard focus stays on the Switch.

Focus after each key once the Switch is disabled, by the ring's box and, on the candidate, AT-SPI's focused node:

| Scenario | Key | Base | Candidate |
| --- | --- | --- | --- |
| Tab first | Tab | no change within 2 s | the Omarchy card ("Omarchy theme"), after 0.052 s |
| | then Shift+Tab | no change | Back to repository, after 0.050 s, past the Switch |
| | then Tab | no change | the Omarchy card, after 0.033 s, past the Switch |
| Shift+Tab first | Shift+Tab | no change | Back to repository, after 0.049 s |
| | then Tab | no change | the Omarchy card, after 0.033 s, past the Switch |
| | then Shift+Tab | no change | Back to repository, after 0.031 s |

On the base every frame from the click on has the same bytes, the frame 2 s after the click and the frames after each key included. `qa.py compare <base> <candidate> --mask status-timing` found the frames with the Switch focused and with it disabled identical in both scenarios. The frames after a key differ only at the two rings they move between.

AT-SPI, with `org.a11y.Status IsEnabled` set for the rehearsal and the four AT-SPI launches only and read back `false` after each:
- On both builds, the focused Switch is "Follow system appearance", a toggle button that is `enabled`, `sensitive`, `focusable` and `focused`.
- On the candidate it keeps those states after the click, so the click moved no focus. After Tab or Shift+Tab, AT-SPI focuses "Omarchy theme" or "Back to repository", and the Switch drops `focusable`.
- On the base, after the click and after every key, AT-SPI names only the "GitTurtle" frame as focused. AT-SPI cannot say where GPUI's focus is. That focus stays on the Switch's untracked handle is inferred from the code and from the candidate.
- As #102 found for Buttons, the disabled Switch still reports `enabled` and `sensitive`.

The Settings Switch draws no focus ring, enabled or disabled, on either build. A frame with it focused matches Settings with nothing focused, so focus on it shows only in AT-SPI and in where the next key lands. That is `switch-focus-ring-and-hover` in `tasks.json`.

Frames: [`evidence/gpui-base-sibling-focus-traps/`](evidence/gpui-base-sibling-focus-traps/) holds four frames:
- `midnight-1000x680-settings-switch-focused` and `omarchy-1000x680-settings-switch-disabled`, whose bytes both builds share;
- the candidate's `omarchy-1000x680-settings-after-tab` and `omarchy-1000x680-settings-after-shift-tab`.

`qa.py privacy scan --redacted` with the local template set found all four clean. A view of each shows only product UI and the fixture's tab name.

Design check: pass with notes, by a `design-reviewer` pass over the four frames. Focus stays on the disabled control until the keyboard moves it, which is #102's rule. Both rings the keys move between are whole: the card's 2 px of accent sits 1 px outside its selected border, about 7:1 against the surface, and Back to repository's ring fits its header slot. Notes:
- **Missing Switch ring:** it is pre-existing and non-blocking. As queued, `switch-focus-ring-and-hover` paints no ring on a disabled Switch. A disabled Switch can now hold focus, so that wording would leave it with no visible focus, unlike #102's Buttons, whose ring stays and fades with them. That is the owner's call for that task.
- **Disabled Switch in Tokyo Night:** it keeps #117's faded thumb. The row's description, not colour alone, says why it is disabled.

Not covered:
- macOS, native Wayland and fractional scale factors;
- the other five controls, and the diff view, ignore dialog and profile editor Checkboxes, natively (the view tests cover them);
- Space, Enter or another binding on the focused disabled Switch;
- the Switch turning enabled again under focus;
- a screen reader speaking.
Each latency is a single sample.

## October 1 whole focus rings in the chooser, worktree and Push to… lists

Task `ring-clipping-lists`, the owner's request of 2026-09-30, from [the whole focus ring in Tags and Reflog](#september-29-whole-focus-ring-in-tags-and-reflog). Three more containers clipped the 3 px a focused Button's ring takes outside its edge: the branch chooser's scrolling list, the worktree manager's scrolling content and its list of worktrees, and the tag inspector's list of Push to… buttons. A focused full-width row lost its sides and its top or bottom edge, and Manage lost its top and left. Each container now keeps `appearance::button_ring_room` inside its clip and gives it back through its margins. The branch chooser and the tag inspector also give it back through the dialog's gap above the footer, and the worktree manager through its title's margin and that gap (`branch_actions.rs`, `worktrees.rs`, `tags.rs`). `DESIGN.md` now lists them among the containers that keep the room. The worktree manager's list of branch choices when creating a worktree stays open there.

Tests: `branch_chooser_keeps_room_for_rings_at_either_end` (`branch_actions/tests.rs`), `manager_keeps_room_for_every_focus_ring` (`worktrees.rs`) and `tag_inspector_keeps_room_for_every_push_ring` (`tags.rs`) lay out each view on the Tags and Reflog clipping fixtures. They require each list's first and last row, and Manage, Create worktree… and Refresh, grown by the installed ring's gap plus width, to lie inside every ancestor content mask, and nothing to move with a zero-width ring.

Native evidence:
- **Builds:** base `20cf4a7` (sha256 `f70708fe…`), the candidate's parent, is the release executable attested for `ring-room-helper`, reused rather than rebuilt. Candidate `00e14c3` (sha256 `96b6a726…`). Both are release builds from clean trees, each in its own `CARGO_TARGET_DIR`, and `qa.py identity` reported no problem. The controller rebuilds the candidate on the commit that adds these frames. Its native attestation re-checks the rebuilt executable against them.
- **Host:** Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680, Midnight and Porcelain. One session on 2026-10-01 from 15:36 to 15:53 UTC ran base then candidate in each palette, 4 launches, after one rehearsal launch of the candidate.
- **Input:** a local driver on the `qa.py` library (`session.Session` and `mutter.MutterDriver`; `drive_rcl.py`, sha256 `aca18071…`) sent every input through Mutter RemoteDesktop with X focus verified, never through XTest. Each launch had its own empty run directory under `/tmp/gitturtle-evidence/runs/ring-clipping-lists`, its own store, and the QA identity.
- **Fixture:** a disposable `ring-clipping` repository under `/tmp/gitturtle-evidence/fixtures/`, made by a script (sha256 `7c673be0…`), with 20 local branches, 3 linked worktrees, the tags `v0.9` and `v1.0`, and the remotes `origin` and `upstream`, local bare repositories filled by a local push. Nothing used the network. The repository, its worktrees and both remotes were the same before and after the session: no action was activated, and each dialog closed with Escape.
- **Routes:** the same keys on both builds.
  - **Branch chooser:** the toolbar's branch menu, Manage another branch…. Tab 2 focuses the first row; after wheeling the list to its bottom, Tab 19 more focuses the last.
  - **Worktree manager:** Manage worktrees… from the command palette. Tab 1 focuses Manage, Tab 4 more the first row (`main`, selected), and Tab 3 more the last.
  - **Tag inspector:** Browse and manage tags… from the command palette, then Tab 3 and Space open `v0.9`. Tab 3 focuses Push to origin…, and one more Push to upstream….

Ring pixels: against the same build's unfocused frame at the same scroll, the 1 to 3 px band outside each control, counted per side (top / right / bottom / left). The counts are the same in both palettes:

| Control | Base `20cf4a7` | Candidate `00e14c3` |
| --- | --- | --- |
| First branch-chooser row | 0 / 0 / 1018 / 0 | 1018 / 74 / 1018 / 74 |
| Last branch-chooser row | 1018 / 0 / 0 / 0 | 1018 / 74 / 1018 / 74 |
| Manage | 0 / 62 / 134 / 0 | 134 / 62 / 134 / 62 |
| First worktree row | 0 / 0 / 1338 / 0 | 1338 / 78 / 1338 / 78 |
| Last worktree row | 1338 / 0 / 0 / 0 | 1338 / 78 / 1338 / 78 |
| Push to origin… | 0 / 0 / 1218 / 0 | 1218 / 62 / 1218 / 62 |
| Push to upstream… | 1218 / 0 / 0 / 0 | 1218 / 62 / 1218 / 62 |

On every side the candidate draws, its strongest ring pixel is 11.43:1 against the same pixel unfocused in Midnight and 6.55:1 in Porcelain. No focus frame differs from its unfocused frame outside the control's box grown by 3 px.

Compares, each `qa.py compare <base> <candidate> --mask status-timing`, unfocused and the same in both palettes:
- The branch chooser at rest and wheeled to its bottom: identical, 0 px outside the mask.
- The worktree manager at rest: 2 px at (166, 375) and (166, 376), the left edge of the "/" that begins the selected worktree's path. The content's clip used to cut that glyph's overhang at x = 167; it now ends at x = 164, the ring's room.
- The tag inspector at rest: 1 px at (248, 145) in the "Tag object:" line, one level brighter in each channel (Midnight `(43,48,58)` to `(44,49,59)`).
The design review accepts both. The first is the ring's room: every pixel from x = 167 on is the same, so the text did not move, and the same 2 px show in every worktree frame. The second is not an inset but rasterisation noise. The kit dialog's `.gap()` sets only the space above the footer, so the content's added bottom room and the smaller gap leave every child where it was. A real shift of the text would redraw the edge of every glyph on that line, not one pixel. Its cause is not proven.

The focus frames differ from the base only in the ring band. The masked pixels, 65 in each frame, are the status bar's timing.

Frames: [`evidence/ring-clipping-lists/`](evidence/ring-clipping-lists/) holds the candidate's 14 focus frames, `candidate-{midnight,porcelain}-1000x680-{chooser-first-focus,chooser-last-focus,worktrees-manage-focus,worktrees-row-first-focus,worktrees-row-last-focus,push-first-focus,push-second-focus}.png`. Each shows the focused control's whole ring. The base's frames and the unfocused frames are not committed. `qa.py privacy scan --redacted` with the local template set found all 44 frames of the session clean. A full-resolution view shows only:
- product UI;
- `/tmp/gitturtle-evidence` paths;
- the QA identity;
- the fixture's branch, tag and remote names.

Design check: pass with notes, by a `design-reviewer` pass over the 14 frames. Each ring is 1 px of surface, then 2 px of full accent (`#75e0bb` in Midnight, `#3455a6` in Porcelain), on all four sides; every straight-edge ring pixel is present, and the four corners match. Non-blocking, each also true of the base:
- **Worktree rows:** a focused worktree row's ring meets the next row's 1 px border, because the rows' 3.25 px gap equals the ring's footprint.
- **Selected worktree row:** the selected row reads as a double outline, the ring and then its own accent border.
- **Branch chooser:** with the chooser wheeled to its bottom, Tab moves focus through rows above the view without scrolling them in. That stays out of this task's scope.

"At whole scale factors nothing moves" is checked natively only at scale factor 1; scale factor 2 rests on the view tests.

Not covered:
- the worktree list scrolled, since four rows fit (its last row sits at the list's bottom edge without scrolling);
- the inspector for an annotated tag, and AT-SPI;
- macOS, native Wayland, fractional scale factors, and other window and text sizes.

## October 1 one ring room for every clip

Task `ring-room-helper`, the owner's cleanup request of 2026-09-30 after #94, #96 and #99. Four clips keep room for the ring a focused Button draws outside its edge: the repository tab strip, the Tags and Reflog dialogs, and Settings' Your themes rows. Each computed that room on its own, and the Your themes rows kept a fixed 3 px (`ROW_RING_ROOM`) while the page's reveal read the installed ring. All four now take `appearance::button_ring_room`, the installed `Theme::button_focus_ring`'s gap plus width. `ROW_RING_ROOM` is gone, and `rows_off_boundary` and the theme editor's `reveal_focused_row` stay in step with the rows' room.

The Your themes rows' fill layer also drops its hover fill after a touch, as GPUI's own hover styles do. GPUI keeps `Window::last_input_was_touch` crate-private, so the row asks GPUI's `Interactivity::compute_style` whether a hover style would show (`settings.rs`, `row_hovered`). The answer follows every modality rule GPUI applies to hover styles.

Tests: `every_ring_room_follows_the_installed_ring` (`repository_tabs.rs`) installs a ring of another size. It requires each of the four clips to keep exactly that room, and the Your themes list's room to agree with the page reveal and `rows_off_boundary`. `a_touch_takes_a_rows_hover_fill_away_until_the_mouse_moves` (`theme_editor.rs`) hovers a row with the mouse and sends a touch over it. It requires the resting fill after the touch, and the hover fill again after the next mouse move. The existing ring-room tests pass unchanged.

Native evidence, light tier, since at the default ring (2 px and a 1 px gap) the change is meant to move no pixel:
- **Builds:** base `469b549` (sha256 `5aa34e3a…`), the candidate's parent, and candidate `04690dc` (sha256 `cc75b9a2…`). Both are release builds from clean trees, each in its own `CARGO_TARGET_DIR`, and `qa.py identity` reported no problem. The controller rebuilds the candidate on the commit that adds these frames. Its native attestation re-checks the rebuilt executable against them.
- **Host:** Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680, Midnight. One session on 2026-10-01 from 14:03 to 14:17 UTC ran base then candidate for each scenario, 10 launches.
- **Input:** a local driver on the `qa.py` library (`session.Session`, `mutter.MutterDriver`, and `portal.Keyboard` for the file chooser; `drive_rrh.py`, sha256 `e2b4218f…`) sent every input through Mutter RemoteDesktop with X focus verified, never through XTest. Each launch had its own empty run directory under `/tmp/gitturtle-evidence/runs/ring-room-helper`, its own store, and the QA identity.
- **Routes:** Tab keys move the focus from where a launch starts, and nothing is activated except Import…. The fixtures were used read-only; their HEAD, status, index, refs and reflogs were the same before and after the session.
  - **Tab strip:** the `tabs` fixture's three repositories, the first active; Tab 7 focuses the first tab.
  - **Your themes:** `theme-fixture`, with a store holding three custom themes. A click on Import… adds Imported Harbor through the GNOME file chooser, which draws the import highlight. Tab 40 focuses Edit… on the row above it, and Tab 43 focuses Imported Harbor's Edit…, flush against the status bar once the page scrolls 3 px to reveal it.
  - **Tags and Reflog:** the `tags-reflog` fixture under `/tmp/gitturtle-evidence/fixtures/`. Tab 3 focuses the first Tags row, and Tab 4 the first Reflog entry.

Compares, each `qa.py compare <base> <candidate> --mask status-timing`: all five sites are identical, with 0 px outside the mask. The masked pixels, 0 to 75, are the status bar's timing. Each focus frame shows the ring. Against the same build's unfocused frame, the 1 to 3 px band outside the target differs on all four sides: 336 px for the tab, 352 for either Your themes row, 2,424 for the Tags row and 2,992 for the Reflog entry, the same on both builds. Another 13 frames from the same launches are identical too:
- the rest frames;
- the middle tab, its close button and the last tab;
- the last Tags row and the last Reflog entry;
- a Your themes row hovered by the mouse, with and without focus.

The base frames reproduce the committed Your themes highlight, focus-beside-highlight and flush frames and the Tags row frame of #96 and #99 byte for byte.

Frames: [`evidence/ring-room-helper/`](evidence/ring-room-helper/) holds the candidate's five frames: `candidate-midnight-1000x680-{tab-focus,themes-row-focus-above-import,themes-row-focus-flush,tags-row-focus,reflog-entry-focus}.png`. The base's frames are identical and not committed. `qa.py privacy scan --redacted` with the local template set found all five clean. A full-resolution view shows only:
- product UI;
- `/tmp/gitturtle-evidence` paths;
- the QA identity;
- the fixture's author.

Design check: pass, by a `design-reviewer` pass over the five frames. Each ring is whole: 2 px of accent (`#75e0bb`) 1 px outside the control on all four sides. The tab strip does not clip the tab's ring, and the flush row's ring keeps a 1 px gap above the status bar's border. The touch rule changes when a row shows its hover fill, not the fill's colour. Non-blocking: touch and a ring of another size rest on the view tests. The tab's ring touches the window's top pixel row, which is whole and pre-existing.

Not covered:
- touch natively, since this host has no touch input (the touch view test covers it);
- a ring of another size natively, since the app installs only the default (the ring-room view test covers it);
- palettes other than Midnight, macOS, native Wayland, fractional scale factors, and other window and text sizes.

## October 1 unselected segments like the shared helper

Task `segments-unselected-like-history`, the owner's request and decision of 2026-09-30, which partly reverses "unselected segments keep the ghost" in [the selected segments entry](#september-30-selected-segments-like-the-shared-helper). That entry's design review left the pressed fill as a follow-up. Settings' density segments and the project hub's mode segments now give an unselected segment `appearance::control_button_variant(false)`, the shared `button()` helper's unselected look that History's segments use, instead of the kit's ghost (`settings.rs`, `projects.rs`). A selected segment keeps #100's look, and while the hub is busy every mode keeps the ghost's disabled look. `appearance::assert_ghost_button` became `assert_unselected_button`, which checks the helper's unselected look at rest, hovered and pressed.

Tests: `settings::segment_tests::every_density_rests_hovers_and_presses_like_the_shared_helper` and `projects::tests::every_mode_rests_hovers_and_presses_like_the_shared_helper_unless_busy`, in Midnight, Porcelain, Kanagawa Lotus and One Dark. An unselected segment must rest, hover and, under a held simulated press, press with the fills of `control_button_variant(false)`; a selected one keeps #100's fills, and a busy hub's modes keep the ghost's disabled look.

Native evidence, full tier:
- **Builds:** base `bc27842` (sha256 `4e4c26c2…`), the candidate's parent, and candidate `7d5f6d5` (sha256 `fff27099…`). Both are release builds from clean trees, each in its own `CARGO_TARGET_DIR`, and `qa.py identity` reported no problem. The controller rebuilds the candidate on the commit that adds these frames. Its native attestation re-checks the rebuilt executable against them.
- **Host:** Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680. One session on 2026-10-01 from 13:04 to 13:11 UTC ran base then candidate for each palette and control, 16 launches.
- **Input:** a local driver on the `qa.py` library (`session.Session` and `mutter.MutterDriver`, `drive_segunsel.py`, sha256 `b732820b…`) sent every input through Mutter RemoteDesktop with X focus verified, never through XTest. Each launch had its own empty run directory under `$RUN` (`/tmp/gitturtle-evidence/runs/segments-unselected`), a generated store with Follow system off, and the QA identity.
- **Fixture and routes:** the density control in Settings on the `theme-fixture` repository (HEAD `52f471a`; its HEAD, status and index were the same before and after the session), reached with Ctrl+comma and a 24-step wheel scroll. The library's `wheel N` gives N effective steps; #100 sent 25 clicks, of which Mutter dropped one, and the base's Midnight density rest frame is byte-identical to #100's. The mode control on the hub, opened with no repository. Hover moves the pointer onto Compact or Clone. Press holds button 1 there for the capture, then moves the pointer, still held, to blank space and releases it, so no click fires: a frame taken afterwards equals the rest frame in all 16 launches.

Compares, each `qa.py compare <base> <candidate>`, with `--mask status-timing` on the full-window density frames. Every difference lies inside the unselected segments' boxes, Compact (903,179)-(961,198) and, in the panel crops, Clone (142,23)-(260,55) and Create (263,23)-(381,55); none is in the selected segment's box or outside the unselected boxes:

| Pair | Midnight | Porcelain | Kanagawa Lotus | One Dark |
| --- | --- | --- | --- | --- |
| density rest | identical | identical | 189 px, Compact's label | 185 px, Compact's label |
| density hovered | 1,078 px, Compact | 1,078 | 1,078 | 1,078 |
| density pressed | 1,078 px, Compact | 1,078 | 1,078 | 1,078 |
| mode rest | identical | identical | 482 px, the Clone and Create labels | 465 px, the same labels |
| mode hovered | 3,746 px, Clone | 3,751 | 4,005, Clone and Create's label | 3,997, the same |
| mode pressed | 3,748 px, Clone | 3,751 | 4,005, Clone and Create's label | 3,997, the same |

At rest the helper's unselected look paints no fill, as the ghost did, so Midnight and Porcelain rest frames are identical. In Kanagawa Lotus and One Dark the unselected labels change, because the helper's look takes `control_label` there.

The pressed-to-selected step: the held unselected segment's most common color against the selected segment's resting fill in the same frame, the same for both controls. The contract's computed base figures (1.65, 1.26 and 1.03:1) measure the pressed fill against `selected_hover` instead; the frames reproduce them, 1.645, 1.258 and 1.028:1. History's Changes segment, measured the same way in extra candidate launches (not committed), presses at 1.047, 1.120, 1.163 and 1.203:1.

| Palette | Selected | Base pressed | Base step | Candidate pressed | Candidate step |
| --- | --- | --- | --- | --- | --- |
| Midnight | `#223b3b` | `#161d29` | 1.415:1, hue 218° to 180° | `#1c3331` | 1.121:1, hue 175° to 180° |
| Porcelain | `#dce6f6` | `#bac3e1` | 1.393:1 | `#d4dff3` | 1.067:1 |
| Kanagawa Lotus | `#c7d7e0` | `#d8cb82` | 1.113:1, hue 51° to 202° | `#c4d2d2` | 1.054:1, hue 180° to 202° |
| One Dark | `#323d52` | `#282c34`, the track itself | 1.283:1 | `#2d374a` | 1.096:1 |

On the base, One Dark's ghost pressed fill equals the segment track, so a pressed frame is identical to rest and the press shows nothing. The candidate's pressed fill is close to History's but not equal (`#1c3331` against `#1f3836` in Midnight), since `control_fill` is translucent and History's track is a different color.

Labels, the darkest (light palettes) or lightest (dark palettes) glyph pixel in each box. Density labels reach that value in only one or two pixels at this text size, so they are approximate:

| Palette | Selected label, density / mode | Base unselected label | Candidate unselected label |
| --- | --- | --- | --- |
| Midnight | `#dfe6ee` / `#e8eef7` | `#dde3ec` / `#e7edf6` | unchanged |
| Porcelain | `#303a55` / `#242e49` | `#333c56` / `#252f4a` | unchanged |
| Kanagawa Lotus | `#494a57` / `#41414e` | `#5e5e6a` / `#555564`, 1.37:1 from the selected label | `#4d4c55` / `#42424e`, 1.03 and 1.01:1 |
| One Dark | `#b4b9c5` / `#bdc2cd` | `#a5acb9` / `#adb4c1`, 1.16 and 1.17:1 | `#b2b7c2` / `#bcc1cc`, 1.01 and 1.02:1 |

Frames: [`evidence/segments-unselected/`](evidence/segments-unselected/) holds base and candidate in each palette (48 files): `{base,candidate}-<palette>-1000x680-density-{rest,unselected-hover,unselected-pressed}.png` and `{base,candidate}-<palette>-1000x680-mode-{rest,unselected-hover,unselected-pressed}-panel.png`. Every frame shows the affected controls. The mode files are crops of the hub's action panel, window (580,215)-(985,480), as in #100, since full hub frames do not pass the local privacy scan. `qa.py privacy scan --redacted --jobs 10` with the local template set found all 48 clean, and the frames viewed show only product UI and the `theme-fixture` tab.

Design review: pass, with no blocking finding, and no `DESIGN.md` line needed, since its button rule ("hover uses the palette hover surface, pressed uses selected") already describes the new look.
- **Press:** the ghost broke that rule twice. Its pressed fill was off the selected hue (38° in Midnight, yellow against blue in Kanagawa Lotus), and in One Dark it did not show. The candidate's pressed fill stays near the selected hue and steps to it by 1.05 to 1.12:1, inside History's 1.05 to 1.20:1. The hovered-to-pressed step is at least 9 in one channel in every palette.
- **Selection:** a resting selected segment still leads the track by 1.18 to 1.53:1, unchanged. A hovered unselected segment stands 1.32, 1.03, 1.07 (with a change of hue) and 1.18:1 from the selected one in Midnight, Porcelain, Kanagawa Lotus and One Dark, where History's stands 1.24, 1.09, 1.20 and 1.29:1. The reviewer accepted this as History's pattern.
- **Labels:** unselected labels reach 5.5 to 15.5:1 on their fills at rest, hovered and pressed. The lowest, about 5.5:1, is Kanagawa Lotus density hovered and pressed, from a one- or two-pixel sample. The mixed labels #100 accepted are gone.
- **Non-blocking, pre-existing:** in Porcelain a hovered unselected segment is only 1.03:1 from the selected one (1.09 in History), and Kanagawa Lotus is close in luminance. A rule for that separation, or a Porcelain `hover` adjustment, is left to a separate task. Settings' density track is `canvas`, while `control_fill` is tuned for panels, which is why its fills are not pixel-identical to History's.

Not covered:
- macOS, native Wayland, fractional scale factors, and text and window sizes other than the default and 1000x680;
- a busy hub natively (the project test covers it), keyboard focus on an unselected segment, and a segment becoming selected by a click or Space;
- the accessibility tree;
- the other built-in palettes and custom or imported themes.

## October 1 whole lists and message for a selected Reflog entry

Task `reflog-selected-entry-collapse` fixes a defect found during [the whole focus ring in Tags and Reflog](#september-29-whole-focus-ring-in-tags-and-reflog). At 1000x680, selecting a Reflog entry collapsed three children: the entry list, the changed-file list and the message editor. The frames `evidence/tags-focus-ring/{midnight,porcelain}-1000x680-reflog-selected-rest.png` stay as the record of the defect.

**Cause.** The Reflog content, `reflog-browser-content` (`reflog.rs`, `Render for ReflogBrowser`), is a flex column with a maximum height (`px(590.).min(body_height)` plus the ring's room) that scrolls on overflow. A selected entry adds the metadata, the message, the changed files and the branch form, which take the column past that bound. Flex layout first shrinks the children (default flex-shrink 1), and only a child's automatic minimum height stops it. Three children had no such minimum:
- the entry list's wrapper, which set `min_h_0()`;
- the message editor, whose `editor_find::Editor` renders its own `h(..).min_h_0()` box (`editor_find.rs`, `RenderOnce for Editor`);
- `reflog-commit-files`, which scrolls, and taffy gives a scroll container an automatic minimum of 0.

Labels, inputs and buttons keep their content height, so the shrinking fell on those three, until nothing was left to scroll. With only debug selectors added, a view test measured the wrapper and the changed-file list at 0 px, and the list at its 6 px of ring padding.

**Fix.** The fix belongs in the Reflog's own content. The column chooses between scrolling and shrinking, and `Editor`'s zero minimum lets it fill a flex parent in other views. The entry list's wrapper, the message editor (now in a `flex_shrink_0` box of its own) and the changed-file list no longer shrink. The column scrolls instead. The ring room of #99 is unchanged. The unselected content at this size fits under its bound, so it lays out as before.

**Test.** `reflog::tests::selected_entry_keeps_lists_and_message_whole` opens the Reflog at 1000x680 at the default text size. Its fixture has 12 HEAD reflog entries, the newest a commit changing three files, which the test selects. It requires:
- the first entry whole inside the list and the content;
- the first changed file whole inside its list;
- the message editor at 110 px;
- the entry list, metadata, message, file list and branch-name label stacked without overlap;
- the review button below the content's view at rest, and whole inside it after one wheel scroll to the end.

On the layout before the fix, with only the selectors added, it fails at its first check: the entry list measures 6 px. `reflog::tests::reflog_browser_keeps_room_for_every_focus_ring` passes unchanged.

**Worktree manager.** Its content, `worktree-manager-content` (`worktrees.rs`), has the same shape and was measured, not changed: a 1000x680 view test with temporary debug selectors, since reverted.
- **Manage:** with a worktree selected, the content takes 296 of its 440 px bound and nothing shrinks.
- **Create, Existing branch:** with 15 local branches, the content fills its 440 px bound, and the branch-choice list, a scroll container, shrinks from 140 to 112.5 px instead of the content scrolling. The list stays visible and still scrolls, so nothing collapses as in the Reflog. Applying the same fix there is left to a separate task.

Native evidence, full tier, since the change moves the layout of the Reflog's content rather than one control:

- **Builds:** base `aaf6fd4` (sha256 `b9667d4b…`), the candidate's parent, and candidate `2c967fb` (sha256 `e8a8bee5…`). Both are release builds from clean trees, each in its own `CARGO_TARGET_DIR`, and `qa.py identity` reported no problem. The controller rebuilds the candidate on the commit that adds these frames. Its native attestation re-checks the rebuilt executable against them.
- **Host:** Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680, Midnight and Porcelain. One session on 2026-10-01 from 09:50 to 09:54 UTC ran base then candidate for each scenario and palette.
- **Input:** `qa.py launch --input mutter` sent every input through Mutter RemoteDesktop with X focus verified, never through XTest.
- **Launches:** each had its own empty run directory under `$RUN` (`/tmp/gitturtle-evidence/runs/reflog-selected`), a generated store with Follow system off, and the QA identity.
- **Fixture:** the `tags-reflog` repository of [the refresh icon entry](#october-1-refresh-icon-on-the-refresh-buttons), used read-only: 16 tags, 12 HEAD reflog entries, HEAD `52f471a`. Its HEAD, status, index, refs and HEAD reflog were the same before and after the session.
- **Route:** Ctrl+Shift+P "browse reflog", then the Reflog at rest. Three wheels over the entry list bring it to its end, where the unselected content cannot scroll. A click selects HEAD@{11} (`3668b4e`, the initial commit), whose 11 changed files against the empty tree are the most of any entry. A wheel over the explanation text then scrolls the content to its end. Each wheel step reaches every scroll container under the pointer, so the list went to its end first; a step that later lands on it cannot move it.

Compares, each `qa.py compare $RUN/ev/base/selected-<palette>/captures/<frame> $RUN/ev/cand/selected-<palette>/captures/<frame> --mask status-timing`:

- **Unselected:** identical in both palettes, at rest and with the list wheeled to its end. The base's frame is also identical to `evidence/tags-focus-ring/<palette>-1000x680-reflog-rest.png`.
- **Selected, at rest:** the base shows no entry row; its message editor is two 1 px borders, with its first line drawn below them against the branch label; it shows no file row. The candidate shows six whole entries, HEAD@{6} to HEAD@{11}, above the selection details.
- **Selected, scrolled to the end:** the base cannot scroll, so its frame is byte-identical to its frame at rest. The candidate shows the message editor at 110 px with all four lines and five whole changed-file rows, the sixth cut by the list's 110 px bound. Editor, file list, branch label, name and Review recovery branch… stack without overlap, and no editor line reaches the label.
- **Defect record:** the September 29 route (a click on HEAD@{0}) on the base is identical to `evidence/tags-focus-ring/<palette>-1000x680-reflog-selected-rest.png`, so those frames remain this base's record of the defect.

Frames: [`evidence/reflog-selected/`](evidence/reflog-selected/) holds base and candidate in each palette (12 files): `{base,candidate}-{midnight,porcelain}-1000x680-reflog-{unselected-rest,selected-rest,selected-scrolled-end}.png`. `qa.py privacy scan --redacted` with the local template set found all 12 clean. A full-resolution view shows only:
- product UI;
- the fixture's name;
- the QA identity;
- the fixture's fictional author.

Design review: pass, with no blocking finding.
- **Layout:** the defect is gone in both palettes. Consecutive blocks keep the 12 px rhythm and the content's left edge.
- **Contrast:** text reaches 10.9:1 to 15.7:1, and the editor border clears 1.3:1.
- **Non-blocking, all pre-existing and left to follow-up tasks:**
  - A selected entry draws no surface of its own: `.toggled(...)` sets only the accessible state, and `.selected(...)` would add the surface DESIGN.md's selected-control rule asks for.
  - At rest, nothing hints that the selection details and branch form sit below the fold. The other review dialogs show an always-visible scrollbar.
  - The changed-file list's 110 px bound cuts only the sixth row's descenders.

Not covered:
- macOS, native Wayland, fractional scale factors, and text sizes and window sizes other than the default and 1000x680;
- the worktree manager natively (its result above comes from the view test);
- the changed-file list's own scrolling, keyboard selection and focus rings with an entry selected;
- the accessibility tree, and Review recovery branch… (no write was made);
- palettes other than Midnight and Porcelain.

## October 1 refresh icon on the refresh buttons

Task `refresh-cw-icon` was found on the way in [the whole focus ring in Tags and Reflog](#september-29-whole-focus-ring-in-tags-and-reflog). Six buttons named the icon `refresh-cw`, which neither the app's `assets/icons/` nor gpui-kit-assets 0.6.0 carries, so each drew an empty icon slot. They are Read log (Reflog), the worktree manager's Refresh, Refresh PRs, Read configured source (LFS download), Refresh conversations and Refresh thread. They now draw the app's `refresh.svg`, as the other refresh and retry buttons do. The Download Before, After and Source LFS… buttons named `download`, which neither set carries either, and now draw `pull.svg`. Labels, accessible names, tooltips, disabled states and actions are unchanged.

Native evidence, full tier, since the change reaches seven controls in five views:

- **Builds:** base `d339883` (sha256 `cbf3b55f…`) and candidate `84c9e97` (sha256 `58144aa0…`). The base is the candidate's parent, and its app code is main's `ec1d7d8`. Both are release builds from clean trees, each in its own `CARGO_TARGET_DIR`, and `qa.py identity` reported no problem. The controller rebuilds the candidate on the commit that adds this entry. Its native attestation re-checks the rebuilt executable against these frames.
- **Host:** Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680, Midnight and Porcelain. One session on 2026-10-01 from 01:32 to 01:46 UTC ran base then candidate for each scenario and palette.
- **Input:** `qa.py launch --input mutter` sent every input through Mutter RemoteDesktop with X focus verified, never through XTest.
- **Launches:** each had its own empty run directory under `$RUN` (`/tmp/gitturtle-evidence/runs/refresh-cw-icon`), a generated store with Follow system off, and the QA identity.
- **Fixtures:** three `scripts/create-demo-repo.py` repositories at HEAD `52f471a`, none changed by any launch:
  - `tags-reflog`, with the 15 tags of the Tags and Reflog entry;
  - `theme-fixture`;
  - `lfs-source`, whose `origin` is a local bare repository.

  The GitHub views ran with `GITTURTLE_GITHUB_FIXTURE=review` on `theme-fixture`.
- **Routes:**
  - **Reflog:** Ctrl+Shift+P "browse reflog".
  - **Worktree manager:** "manage worktrees".
  - **GitHub:** "github pull requests". With the fixture: Refresh fixture PRs, then #42, whose Overview holds Refresh conversations; a four-step wheel brings two Refresh thread buttons into view. Without it: the panel at rest with Refresh PRs and no account.
  - **LFS:** in History, commit `aac36de`, then `public/lfs/canvas-photo.png`, whose Compare shows Download After LFS…. That button opens the download dialog. Git LFS is not installed on this host, so the dialog shows "Git LFS tooling is unavailable…" beside Read configured source, at rest.

Each of the 28 frame pairs was compared with `qa.py compare $RUN/base/<scenario>/captures/<frame> $RUN/cand/<scenario>/captures/<frame> --mask status-timing --mask <button box>`. The button boxes were written down from candidate frames before the first compare. The two boxes for the panel without the fixture were added after it. Each button differs only inside its icon slot, the same in both palettes. No label, button box or neighbour moves:

| Button | Icon slot | Pixels |
| --- | --- | --- |
| Read log | (774,160)-(788,172) | 90 |
| Worktree manager Refresh | (397,112)-(411,124) | 90 |
| Refresh fixture PRs / Refresh PRs | (338,158)-(352,170) / (338,112)-(352,124) | 90 |
| Refresh conversations | (797,540)-(811,552), scrolled (797,288)-(811,300) | 90 |
| Refresh thread, two threads | (199,347)-(213,359), (199,519)-(213,531) | 90 each |
| Download After LFS… | (337,192)-(349,206) | 70 |
| Read configured source | (426,294)-(440,306) | 90 |

- **Outside the masks:** 22 pairs are identical. Six differ by 1 to 3 px, by one level in 255, on glyph edges far from any button, at (942,591), (942,340), (158,212), (730,436) and (865,669). A second run of each build shows the same spots between two runs of one build, so they are per-launch text rendering.
- **Caret:** the panel without the fixture also masks its focused destination field's caret, (50,109)-(56,127), which blinks per launch.

Frames: [`evidence/refresh-cw-icon/`](evidence/refresh-cw-icon/) holds base and candidate of each view in each palette (28 files): `{base,candidate}-{midnight,porcelain}-1000x680-{reflog-rest,worktrees-rest,github-panel-rest,github-pr42-overview,github-pr42-conversations,lfs-compare-download-after,lfs-download-dialog}`. `qa.py privacy scan --redacted` with the local template set found all 42 staged files clean. A full-resolution view shows only:
- product UI;
- the fixtures' names and `/tmp/gitturtle-evidence/` paths;
- the QA identity;
- fictional authors and reviewers.

Committed frames:
- **Recaptured and replaced:** the 12 Reflog frames in [`evidence/tags-focus-ring/`](evidence/tags-focus-ring/) and `evidence/themes/button-focus-ring/{midnight,porcelain}-1000x680-reflog-open.png`. Each was recaptured on both builds with its fixture and route, and is identical outside the Read log box. The candidate's recaptures replace them, so they now draw the icon. They differ from the September 29 captures by this host's renderer and by the fixture's reflog timestamps.
- **Kept as their builds' record:** `evidence/themes/button-focus-ring/{midnight,porcelain}-1000x680-reflog-row-focus.png` (`650a76e`) and the macOS `evidence/review-milestone/worktree-management.jpg`. Both still show the empty slot.

Design review: pass, with no blocking finding.
- **Glyph:** each refresh glyph draws 14x12 px of ink, as the existing Working Changes refresh does. It sits within 0.5 px of the label's cap-height centre, in the label's colour (12.6:1 to 15.7:1 on its surface).
- **Rows:** none changes, since the base already reserved the empty slot.
- **`pull`:** acceptable for the LFS download. It is a generic arrow-into-tray glyph, the button is labelled, and the Pull action sits in another region. A dedicated download glyph would be a separate assets task with a package attestation.

Not covered:
- macOS, native Wayland and fractional scale factors;
- the accessibility tree (AT-SPI is off on this desktop; the change passes only an icon name);
- hover, pressed and disabled states, and focus other than the recaptured Read log focus;
- a resolved LFS source (Git LFS is absent), and the Download Before and Source LFS… variants;
- live GitHub, and palettes other than Midnight and Porcelain.

## September 30 disabled switch thumb fades with its track

Task `switch-disabled-thumb` (Omarchy follow-up D1; the owner chose on 2026-09-29 to fade the thumb with the track). While the Omarchy card is selected, Settings disables **Follow system appearance**. The vendored Switch faded only its track to half opacity and painted its thumb in `switch_thumb`, the palette's text, at full strength, so the locked switch read as live. The vendored Switch now paints the thumb at the track's 0.5 whenever it is disabled, through the thumb's disabled style, so every disabled Switch gets it; GPUI multiplies each primitive's alpha, so the faded track shows through the faded thumb, which is accepted ([patch note](../vendor/gpui-component/GITTURTLE-PATCH.md)). `DESIGN.md` states the rule beside the other disabled-control rules. `settings::picker_tests::a_disabled_switch_fades_its_thumb_with_its_track` reads the switch's track and thumb fills from the rendered scene: with the Omarchy card selected both are the palette's at half opacity, and with Midnight selected, off and on, both are at full strength. On `origin/main` the disabled case fails with the thumb at alpha 1.0. It runs on Linux only, where the Omarchy theme exists.

Native evidence, full tier, on Omarchy 4.0.4 (Hyprland 0.56.2, native Wayland):
- Builds, release, clean, in one session: base `ca7b826` (`gitturtle-ca7b826`, product code identical to `main` at `733ec33`) and candidate `b769b4c` (sha256 `ef69f623…`, `--build-info` clean). The branch's later commits change only `DESIGN.md`, the evidence and this entry.
- Output: a temporary 1480 × 800 headless output at scale 1, the window fullscreen. Each launch had a fresh HOME with the `GitTurtle QA` identity, fresh XDG directories and the disposable fixture `/tmp/gitturtle-evidence/narrow-8/demo`, and opened Settings with Ctrl+, after checking the active window's PID.
- Palettes: the Omarchy card following Tokyo Night (dark) and Catppuccin Latte (light), from a copy of the bundled theme in the launch's own `current/`, which disables the switch; Midnight (dark) and Daylight (light), which leave it enabled. In every frame the Projects card's two switches are enabled, one on and one off.
- Comparison (`qa.py compare --mask status-timing`): in both Omarchy palettes only 216 px differ, all inside the disabled thumb ([722, 222]–[738, 238]); Midnight and Daylight are pixel-identical, the enabled switches included. The thumb went from `#A9B1D6` to `#666C87` in Tokyo Night and from `#4C4F69` to `#8E91A2` in Catppuccin Latte, the text colour at 0.5 over the faded track.
- Frames: the eight Settings frames and a 4× crop of the disabled switch per Omarchy palette, base above candidate, are in [`evidence/switch-disabled-thumb/`](evidence/switch-disabled-thumb/). A privacy scan of the 10 committed files with the local template set came back clean, and each was viewed at full size.

A `design-reviewer` pass approved the frames: the disabled switch now reads as unavailable in both Omarchy palettes and its thumb still reads as off. The faded thumb measures 2.85:1 on its track in Tokyo Night and 2.11:1 in Catppuccin Latte (6.99:1 and 5.38:1 before), against 5.22:1 and 4.58:1 for the enabled off switch in the same frame. Neither `DESIGN.md` nor the readability rules set a contrast floor for a disabled control, and WCAG exempts inactive components.

**Findings**, for the owner and not fixed here:
- **The disabled label barely dims.** The Switch's disabled label takes `muted_foreground`. Omarchy's mapping raises `muted` to `text` in Catppuccin Latte, so the label, the row title and its description all draw `#4C4F69`, and in Tokyo Night the label (`#A0A8CE`) is close to the title (`#A9B1D6`). The thumb is the only part of the control that visibly dims in Latte.
- **A disabled checked Switch** (a faded thumb on a faded accent track) has no consumer: Follow system is the only Switch Settings disables, and it is always off while disabled. Neither the test nor the frames cover it.

Not covered: focus and hover on the disabled switch, XWayland, X11, scales other than 1, and macOS, where the Omarchy card does not exist.

## September 30 diff headers and syntax colours from the palette

Task `diff-syntax-colours-from-palette` (Omarchy follow-ups D2 and D6, which predate the Omarchy theme). On `main` the unified diff highlighted with the `diff` grammar in the toolkit's default highlight theme, so the `---` and `+++` lines, and changed lines wherever their decoration did not win, drew colours no palette chose, and syntax tokens everywhere took that same default theme; [`themes/spec.md`](development/themes/spec.md) records where each came from. Owner decisions (2026-09-30): the syntax roles take mapped hues (keyword renamed, string added, number, boolean and constant warning, type and constructor modified, function hunk, tag, attribute, property and link accent, comment muted, variables and punctuation text), each mixed toward text until it reads at 4.5:1 or better (aiming at 4.75) on the editor background, the panel and the added and removed tints; the unified diff drops the grammar and draws plain text with the palette's decorations, with `---`/`+++` headers in the text colour at medium weight; the PR review's patch is decorated like Compare; Omarchy comments stay `muted`.

What changed:
- `Palette::configure` fits the syntax roles for every palette, built-in, custom and Omarchy-mapped, as precomputed targets. The unified patch draws as plain text with the palette's decorations, so its `---` and `+++` file headers draw in the editor's text colour at medium weight and follow a theme change in Compare, recovery and rewrite review alike.
- The PR review's Source patch prepares its decorations with its file on GPUI's background pool, within Compare's 2 MiB / 100,000-line bounds (a larger patch stays plain text), keeps them for the editor's life, follows palette changes, counts them in the panel's retained bytes and catches a panicking preparation as an error. Find refreshes when decorations attach under open matches.

Tests: `appearance::tests::syntax_colors_read_on_every_editor_background` (all twenty built-ins and a generated custom theme) and `appearance::omarchy::palette::tests::fixture_syntax_colors_read_on_every_editor_background` (the embedded Omarchy fixtures, dark and light) hold every syntax token colour the diff and source editors use to 4.5:1 on those four backgrounds; on `main` 382 of 4,380 pairs fell short (Daylight's attribute at 3.84:1). `text::tests::patch_headers_and_changes_draw_palette_colors_in_every_built_in` asserts the `@@`, `---` and `+++` lines draw palette colours in every built-in, also under decorations prepared in another palette; on `main` Midnight's `---` drew `#87b1f6`. `syntax_fields_take_their_palette_roles` and `syntax_draws_in_text_where_text_misses_the_rule` pin the role table and its fallback. `github_view::review::tests::source_patch_takes_the_decorations_its_file_preparation_made`, `source_presentation_stops_at_compares_bounds` and `editor_find::tests::decorations_attached_under_open_find_take_no_background_under_its_matches` cover the PR patch's lifecycle, bounds and Find.

Lowest token contrast before and after, on `canvas` (the editor background, where hunk headers also draw), `panel` (the active line) and the added and removed line tints ([details](development/themes/spec.md#diff-and-syntax-colors)):

| Group | Before | After |
| --- | --- | --- |
| Built-in dark | 3.88 | 4.75 |
| Built-in light | 2.61 | 4.75 |
| Omarchy | 2.62 | 4.75 |
| Custom | 3.18 | 4.76 |

Native evidence, full tier, on Omarchy 4.0.4 (Hyprland 0.56.2, native Wayland):
- Builds: release, clean, in one session: base `ca7b826` (`gitturtle-ca7b826`, sha256 `4c2cb18e…`; its product code is identical to `main` through `ec1d7d8`) and candidate `e71ed69` (sha256 `95eef802…`, `--build-info` clean). The round 1 build `26a6390` (sha256 `f7a96b71…`) was captured the same way on the same day.
- Compare: the disposable repository `/tmp/gitturtle-evidence/syntax-10/repo`, whose commit `fa41ee5` edits `src/lib.rs`, opened from History in Compare's Diff, then Split. Each launch had a fresh HOME with the `GitTurtle QA` identity and fresh XDG directories on a temporary 1480 × 800 headless output at scale 1; pointer clicks went through a Wayland virtual pointer after checking the active window's PID. Palettes: Midnight, Daylight, and the Omarchy card following Tokyo Night from a copy of the bundled theme in the launch's own `current/`.
- PR review patch: the offline review fixture ([`GITTURTLE_GITHUB_FIXTURE=review`](github-collaboration.md#offline-native-review-fixture)): PR #42, Files, `src/review/session.rs`, Changes, then Source, in the same three palettes. No account, network or credential is involved.
- Comparison: `qa.py compare --mask status-timing`, plus the output's top 26 px, where Omarchy's bar drew over the headless output in some runs. Compare differs from base only in the editor text (Diff 4,754–4,806 px, Split 25,152–27,172 px, plus one pixel of raster noise at (584, 189) in Midnight) and is pixel-identical to the `26a6390` frames in all three palettes. In the PR review, Changes is pixel-identical and Source differs only in its editor text, hunk-header rows included: [488, 365]–[777, 600] in the 1480 × 800 capture, 26 px higher in the committed crops.
- Frames: cropped below the top 26 px to 1480 × 774, the 18 frames are in [`evidence/diff-syntax-colours/`](evidence/diff-syntax-colours/): Compare Diff and Split and PR Source, base and candidate, per palette. A privacy scan of the 18 committed files with the local template set came back clean.

Two `design-reviewer` passes approved with notes: the round 1 Compare frames on September 30 (session 10), and the final PR Source frames. PR Source now draws the same colours as Compare in each palette (Midnight: removed `#FF95A8` on its line tint `#382531`, added `#75E0BB` on `#19322D`, hunks `#95BAFF`). Measured in the frames: context text 8.10–15.67:1, hunk headers 5.40–9.35:1, removed text on its line tint 4.90–6.85:1, added 5.89–8.56:1. The word tints, which are not targets by decision, read 3.40:1 (Daylight, removed) and 3.43:1 (Tokyo Night) at the lowest; the base's Daylight hunk gold read about 4.2:1.

Performance ([record](benchmarks/2026-09-30-diff-syntax-colours.md)), a `performance-reviewer` measurement in release against the same base on this host (AMD 3020e, two cores, the app pinned to one): `Palette::configure` alone took 40.1 to 46.7 µs at p50 per palette against 4.75 to 4.89 µs, about 36 µs of it the per-field serde round trips of the kit's highlight theme; in the running app an Omarchy switch applied in 0.592 ms at p50 and 0.741 ms at p95 against 0.463 and 0.589 ms, far inside the theme budget (median 8 ms, p95 16 ms). The picker switch and the theme editor's live preview run the same single `configure` and were not measured natively: the app's window could not take keyboard focus during the run.

**Findings**, for the owner and not fixed here:
- Find and selection highlights are not colour targets: syntax colours read at 3.78:1 there.
- Markdown ```` ```diff ```` blocks still use the `diff` grammar, now drawn in the palette's roles.
- The PR review's Source shows single line numbers, no hunk folding and an untinted canvas gutter, unlike Compare (as before).
- Roles share colours where the palette's do (modified and warning, for example), and the word tints, which are not targets, read at 2.58:1 at the lowest.
- Medium weight is invisible in the bundled DejaVu Sans Mono, which has no Medium face, so the `---` and `+++` headers there look exactly like body text.
- Thin glyphs measure 4.44:1 at scale 1 where antialiasing thins them.
- Tokyo Night's comments (`#A0A8CE`) sit close to its text (`#A9B1D6`); the owner kept comments `muted`.
- Compare's, recovery's and rewrite review's decoration lists are not counted in any retained-bytes budget.
- `configure`'s 42 per-field serde round trips could become one, or a cached result per palette, if theme application ever needs the 36 µs back.

Not covered: macOS, XWayland, X11, scales other than 1, and a live GitHub account (the PR patch frames come from the offline fixture).

## September 30 Git writes and network actions on Omarchy

Task `omarchy-git-writes-evidence` checks Git writes and network actions in the release build on Omarchy, on native Wayland. It is evidence only; no product code changed. The run took place on 2026-09-30 between 19:22 and 19:29 UTC.

- Host: Omarchy 4.0.4-1, Hyprland 0.56.2 (`efb5099`), native Wayland, on a temporary headless output at 1480 × 800 and scale 1 with the window fullscreen; AMD Radeon Vega (Picasso/Raven 2) with Mesa 26.2.2 and `vulkan-radeon`. The monitor layout (mode, scale and position in `hyprctl monitors -j`) matched its reading from before the run.
- Build: `main` `ca7b826`, release, clean, sha256 `4c2cb18e…`.
- Launch: fresh HOME and XDG directories whose `~/.gitconfig` holds only the `GitTurtle QA <qa@example.invalid>` identity, so no credential helper was configured (none is set system-wide on this host), and Midnight selected.
- Fixture: a disposable repository at `/tmp/gitturtle-evidence/writes-10/work`, cloned from the bare `remote.git` beside it, with `notes.txt` edited and `story.txt` edited in two hunks (lines 3 and 36). Before the launch, a second clone put "Upstream change" on `remote.git` and "Change on the HTTP remote" on a second bare repository, `http/lan.git`. Every commit, the one made in the app included, has the QA identity as author and committer.
- Remotes: `origin` at `file:///tmp/gitturtle-evidence/writes-10/remote.git`, and `lan` at `http://127.0.0.1:18710/lan.git`, served by `git http-backend` through a small Python CGI bridge that answers 401 to any request without the throwaway Basic credentials. Its request log recorded each 401 and each authenticated request.
- Input: pointer clicks through a Wayland virtual pointer (`zwlr_virtual_pointer_v1`) at a position set with `hl.dsp.cursor.move`, and typing with `wtype`, each after checking that the active window's PID was the app's.

Results, in the order they ran. The fixture's `git status --porcelain=v2 --branch` and `git log --all --format='%H %an %s %D'` after each step are in `git/` beside the frames:
- **Fetch and pull over `file://`.** Fetch showed "Fetched origin" with the branch 1 behind, and Pull fast-forwarded `main` to `d14f1d2` ("Pulled origin/main").
- **Fetch and pull over HTTP.** With `lan` typed as the Targets remote, Fetch opened GitTurtle's **Git authentication** dialog for the user name, then for the password, which it masks; the password prompt shows the URL with `[redacted]` in place of the user name. Once both were answered, "Fetched lan" brought `lan/main` at `af9df9d`. Pull asked for both again and fast-forwarded `main` to `af9df9d`.
- **Stage a file.** The row's Stage button put `notes.txt` in the index ("Staged notes.txt"; `M.`).
- **Stage a hunk.** In `story.txt`'s diff, the first hunk's **Stage hunk** staged only the line 3 edit ("Selected changes staged"; `MM`). The index's `story.txt` blob, `41633d6`, which the commit below recorded, differs from `HEAD`'s only at line 3.
- **Unstage.** The staged row's Unstage button returned `notes.txt` to the working tree (`.M`).
- **Commit.** With the title "Edit the story's first hunk", **Commit 1 file** made `8fee510` ("Committed 8fee510 · Edit the story's first hunk") with the staged hunk only; the second hunk and `notes.txt` stayed unstaged.
- **Branches.** **Create** made `qa-topic` at `8fee510` and switched to it, **Switch** with `main` returned to `main`, and the `qa-topic` row's actions (Shift+F10), **Delete branch…** and its confirmation removed it ("Deleted local branch 'qa-topic'").
- **Push.** Push to `origin/main` moved `remote.git`'s `refs/heads/main` to `8fee510`. Push to `lan/main` asked for the credentials again and moved `lan.git`'s `refs/heads/main` to `8fee510`; as the branch Push sets the pushed branch's upstream (`--set-upstream`), `main` then tracked `lan/main`.
- **Credentials.** After the run, `grep -r` for the password, the Basic token and the user name found no match in the launch's HOME, its XDG config, data, cache and state directories, its stdout and stderr, or the fixture's `.git` (`git/credential-grep.txt`). Apart from shader caches, GitTurtle wrote only `preferences.json`, `activity.json` and `repository-session.json` there.

The 17 frames and the records are in [`evidence/omarchy-git-writes/`](evidence/omarchy-git-writes/). `python3 scripts/native_qa/qa.py privacy scan --redacted --jobs 2` with the local template set passed all 17 committed frames (0 matched, 276 s wall, 19:49 UTC), and each was viewed at full size.

Found, not fixed, and reported for its own task: History was scoped to `qa-topic` when it was deleted. A red "History scope changed" banner appeared at once and returned after each later push, and an explicit Refresh (Ctrl+R) then reported "Could not open repository" for the whole repository and marked its tab unavailable until **All history** was chosen (`27b-after-refresh.png`, `28-all-history-after-refresh.png`). Also, the authentication prompt mentions macOS Keychain on Linux.

Not covered: SSH remotes and host verification, a credential helper that stores answers, cancelling a prompt, a rejected push, a pull that cannot fast-forward, conflicts, and Git writes under XWayland.

## September 30 paging and the diff in narrow windows

Task `narrow-window-compare-height` closes the follow-ups of [the fractional-scales check](#september-27-fractional-scales-on-hyprland) for side-by-side tiles on a display about 1,000 logical pixels wide, 461 to 493 px each. The owner's decisions (2026-09-29): History's paging becomes icon-only below the width its labelled scope toolbar needs, Compare's review options become arrows and an Options menu below the width their row needs, the composer heading squeezed out at 461 × 490 is recorded rather than changed, and an 18 pt interface text size at 461 × 490 is recorded for a later task. On 2026-09-30 the owner added the kit's tooltip (`vendor/gpui-component`) to the task's scope, so a tooltip wider than the window wraps inside it.

What changed, all below breakpoints measured from the interface font at the interface size, so wider layouts take the same code path as before:
- **History.** Below the width the labelled scope toolbar needs, Columns, Latest, Previous and Older become 28 px icon buttons, a quarter rem apart, with their accessible names and tooltips that lead with those names. Latest shows an up arrow, Previous and Older up and down chevrons. As the column narrows, the scope name leaves first, then the count; the count is drawn whole or not at all, and the name only beside it and only where its first character and ellipsis fit. The branch icon's tooltip names the scope and count. A steady render asks the text system nothing: the label widths are shaped once and cached by font, interface scale and rem size.
- **Compare.** Below the width the review options row needs, Previous change and Next change become arrows, and Hide whitespace, Context and Reset review move into an Options menu whose tooltip carries their explanations. The row stays one line, and its caption ("Original Git diff") stays on it, aligned with the buttons' text and truncated with its full text in a tooltip.
- **Tooltips.** A control's tooltip wider than the window wraps inside it with equal margins (the positioner's 4 px, any client inset and the popup's 0.75 rem on each side); an element tooltip wraps to the same width, but GPUI places it from the pointer, so its margins can differ. A tooltip narrower than the cap lays out as before.

Tests:
- `views::tests::narrow_history_keeps_paging_in_the_column` checks Latest, Previous and Older inside the column and the window at 461 × 490, 493 and 514 px, and that each acts; it fails on `main`.
- `views::tests::narrow_compare_keeps_three_diff_lines`, beside `narrow_windows_keep_the_inspector_whole_and_the_header_on_one_row`, checks at least three diff lines at 461 × 490; it fails on `main`. At an 18 pt interface text size the diff gets no line, before and after (the header takes 183 px, the Git action bar 113 px and Compare's toolbar 265 px), so that size is recorded rather than asserted.
- `views::tests::review_caption_aligns_with_the_button_labels` checks the caption's left edge against the buttons' text, wide and narrow.
- `views::tests::scope_toolbar_grows_in_order_as_history_widens` sweeps three bands of window widths and checks that the count is whole or absent, that the name appears only beside it and at least a character and its ellipsis wide, and that the states never go backwards as the column widens; `scope_name_keeps_a_character_before_its_ellipsis` pins the name's minimum.
- `views::tests::narrow_review_options_act_from_the_menu`, `text_review::tests::collapsed_options_menu_keeps_each_buttons_state` and `views::tests::compact_controls_carry_their_accessible_names` cover the menu's actions and states and the compact controls' names. GPUI's test context cannot read an AccessKit label from a kit Button, so the last reads the names the app passes.
- `native_accessibility::control_tests::tooltips_wider_than_the_window_wrap_inside_it` places real kit tooltips in 461 and 2,000 px windows at two rem sizes, and `views::tests::narrow_tooltips_wrap_inside_the_window` hovers the compact controls at 461 × 490; both fail without the cap.

Native evidence, on Omarchy 4.0.4 (Hyprland 0.56.2, native Wayland), from one session on 2026-09-30:
- Builds: base `main` `6db2269`, release, clean, sha256 `1a2dab0e…`; candidate `37fe39f`, release, clean, sha256 `6da84f59…` (`--build-info`). Later commits on the branch change only documentation and test code, apart from a merge of `main` that brings in #102.
- Outputs: temporary headless outputs. The narrow windows are tiled beside a throwaway foot window, as two windows side by side: 461 × 490 on 1920 × 1080 at scale 2, 477 × 490 on 1984 × 1080 at scale 2, and 493 × 526 on 1280 × 720 at scale 1.25. The wide windows are fullscreen at scale 1: 1000 × 680 and 1480 × 800. `hyprctl monitors -j` matched its reading from before each run.
- Launches: each had a fresh HOME with the `GitTurtle QA` identity and fresh XDG directories. The fixture was a `scripts/create-demo-repo.py` repository at `/tmp/gitturtle-evidence/narrow-8/demo` with two staged files and one unstaged; its HEAD, status and staged diff were the same after every launch. History selected row 7 (`e96f692`), Compare opened `src/App.tsx`, and Changes (Ctrl+2) showed the staged files and the composer.
- Input: keys through `wtype` after checking the active window's PID; tooltips by moving the compositor's pointer onto each control after it rested for 1.2 s on a spot without a tooltip. Hovering from one control straight to the next inside the kit's 300 ms grace period sometimes showed no tooltip on the third and later hovers; with the rest, every tooltip showed.

Results:
- **461 × 490 at scale 2.** The base hides History's paging past the column's clipped edge, and its Compare review options wrap one per row, leaving the diff no line. The candidate keeps Columns, Latest, Previous and Older whole in the 176 px column, with no name or count, and Compare shows five whole diff rows and half of a sixth.
- **477 and 493 px.** The count "1–8" is drawn whole with no name beside it, never as a clipped ellipsis. At 493 × 526 Compare shows seven whole diff rows; the base shows four and part of a fifth.
- **Tooltips.** Before the cap, the Latest, Older and Options tooltips ran past the 461 px window's right edge, and Options lost its whole last sentence. With it, every compact control's tooltip at 461 × 490, 493 × 526 and 18 pt wraps inside the window above its control, with margins within a pixel of each other (28 and 29 px at 461, 17 and 17 at 493, 35 and 36 at 18 pt, in physical pixels), and Options reads through "…requires the original diff." Previous, which fitted with 2 px to spare, now takes two lines. The Columns, branch icon, caption and arrow tooltips, which fit, are unchanged, and at 1480 × 800 the labelled Latest's tooltip is identical to the base's.
- **Composer.** With two staged files the composer at 461 × 490 is 38.5 px tall and its fields area 0 px, so the heading is squeezed out; the base is the same. Pinning it would need about 31 px, which is a design choice the owner left for later.
- **18 pt at 461 × 490** (identical in the base): Latest and Previous are now reachable, but Older is cut off, because the 176 px column minimum does not scale with the interface size. Compare shows no diff line. The inspector's Hash is drawn over "Changed files" in History and Compare, History shows no rows, and in Changes the placeholder is cut and "Filter working paths…" runs past the right edge.
- **Wide layouts.** At 1000 × 680 and 1480 × 800, History, Compare and Changes are identical to the base under `qa.py compare --mask status-timing`. At 1480 × 800 the first base launch differed from the candidate by one level of green in one glyph-edge pixel at (1305, 358); a second base launch matched the candidate, and two base launches differ at that pixel in the same way. The candidate's narrow frames match the previous round's build apart from the timing text and a 1–2 px glyph speck in the mode control, which two base launches also show.
- **Changes** is identical to the base at every size.
- **Scrolling.** A same-session release measurement of History scrolled by the wheel found no regression: at 1480 × 800 the candidate's scene time is indistinguishable from the base's (p50 9.624 against 9.692 ms, p95 11.129 against 11.923), and at 461 × 490, with the compact toolbar, it is not slower (p50 6.506 against 6.599 ms) ([record](benchmarks/2026-09-30-narrow-history-scroll.md)).

The 26 frames are in [`evidence/narrow-window/`](evidence/narrow-window/): History, Compare and Changes for the base and the candidate at 461 × 490, 493 × 526 and 461 × 490 at 18 pt, and eight of the candidate's tooltips. A privacy scan of the committed files with the local template set came back clean, and each was viewed at full size.

A `design-reviewer` pass approved the final frames with notes, after two earlier rounds asked for the whole count at 493 px, names in the compact tooltips, the caption's tooltip and the tooltip wrap. It measured the wrapped tooltips' margins as even as whole pixels allow, and found the resting frames, the short tooltips and the wide frames unchanged from the previous round apart from the timing text. `code-reviewer` accepted every round; the wording and test changes its last pass asked for are in the final commits.

**Findings**, for the owner and not fixed here:
- The review row's arrows are the kit's small ghost buttons, about 19.5 px square on a 22.5 px pitch, under the 24 px target spacing; the path row's icon buttons above are 28 px. Enlarging them would cost about half a diff line at 461 px.
- The review row's buttons show no hover fill, in the base's labelled row too (the background darkens by two levels).
- Element tooltips (the branch icon's, the caption's) appear at the pointer, and control tooltips above their control.
- Disabled Previous and Older still describe their action, with no reason they are disabled (as in the base).
- At 493 × 526, Compare leaves a 12 px band empty below its seventh row, where the base draws part of the next row.
- In narrow History a vertical wheel also scrolls the table sideways, because the list's container scrolls only horizontally and GPUI turns vertical wheel movement into horizontal scrolling there (as in the base).
- The kit's menu items do not expose a checked state (`vendor/gpui-component/src/menu/menu_item.rs` ~95-99), so the Options menu's Hide whitespace item cannot announce it.
- A test cannot read a kit Button's AccessKit label; a `Button::accessible_name()` accessor in the kit would let one.
- Residuals from code review, not observed: at a scale of 1.5, device-pixel snapping could leave the scope name a pixel or two short of its minimum just above the labelled breakpoint, and a first letter that kerns against the ellipsis could do the same; tooltip margins agree only to within about 1.5 px after rounding.

Not covered: macOS, X11, a light palette, client-side decorations with an inset (the tiled windows here had none), a wrapped element tooltip (the fixture's paths and caption are short), a scale of 1.5, a pointer click (the tests click; natively the keyboard drove every step), a live window resize, and History during a search, where a long count can switch the toolbar to compact at 1000 × 680.

## September 30 Linux antialiasing without a settings portal

Task `linux-text-antialias-without-portal`. The no-portal frames of the [recapture](#september-29-frames-retaken-under-the-solid-focus-ring), `themes/import-export/noportal-1000x680-{01-export,03-import}-guidance.png`, draw their text with subpixel antialiasing. `DESIGN.md` said Linux follows the desktop's preference, grayscale unless the session asks for subpixel. On this host (Ubuntu 26.04, GNOME 50) the session asks for grayscale: `gsettings get org.gnome.desktop.interface font-antialiasing` is `'grayscale'` and `font-rendering` is `'manual'`. The app reads those keys only through the settings portal, and the no-portal flow ran on a private session bus with no portal, so the app fell back to fontconfig. `fc-match -v sans-serif` there resolves Noto Sans with `antialias: True`, `rgba: 1` (RGB), `hintstyle: 1` and `lcdfilter: 1`. The `rgba` comes from `/etc/fonts/conf.d/10-sub-pixel-rgb.conf`, which Ubuntu ships in `fontconfig-config`; no user fontconfig configuration exists. A throwaway probe of the app's own `connect`, `rendering_from_gnome`, `fontconfig_defaults` and `Observed::resolve` gave subpixel on a bus with no services and an empty HOME, and grayscale on the real session bus. The frames' pixels agree: on text-only lines the no-portal frame's per-pixel chroma peaks at 106 to 177, a portal frame's at 20 to 43, and on the title, tab name and status bar the two no-portal frames are the only outliers among the 28 import-export frames.

Owner decision (2026-09-30): the behaviour stays, because where no portal serves GNOME's keys fontconfig is where the desktop's preference lives, and `fc-match` cannot tell a distribution default from an opt-in. On this host's real session bus the portal answers those keys: its configuration sends Settings to `xdg-desktop-portal-gnome`, then `xdg-desktop-portal-gtk`, and both return `'grayscale'`. `DESIGN.md`, the [Linux runbook](linux.md#platform-limits) and the module docs now state the order: the portal's GNOME `font-rendering`, then `font-antialiasing`; then fontconfig's resolved `sans-serif` `antialias`/`rgba`, distribution defaults included; then grayscale.

Tests: `desktop_text::tests::antialiasing_order_each_source_decides_once_the_earlier_ones_are_absent`, `…_fontconfig_answers_grayscale_subpixel_or_nothing` and `…_gnome_keys_win_over_fontconfig` exercise `Observed::resolve` with no process or bus. The behaviour is unchanged, so they pass on main as well. No native run: no pixel changes.

A `code-reviewer` pass found no defect. The stated order matches `Observed::resolve` and its callers, and the tests fail on a mutant that reads fontconfig before GNOME's keys or maps BGR to grayscale. It asked for the unverified desktop examples to go and for `fc-match`'s answers with nothing configured (`|`, `True|`) to be covered, both done, and noted the untested `observe` choice below. A read-only `verifier` pass re-ran the host commands, confirmed the private bus had no portal, reproduced the fringes, ran the tests and the fast gate, which passed, and checked that the documents and the code state one order. It failed the entry twice: first for an unestablished portal sentence and a too-narrow list of what is not covered, then for a merge marker left in this file. It passed once both were fixed.

Not covered: any other desktop's portal and fontconfig answers, among them whether Omarchy's portal sends Settings to `xdg-desktop-portal-gtk`, which would let GNOME's keys decide its text before fontconfig; `observe`'s choice to read fontconfig whenever the portal's GNOME keys do not decide, and its re-read after a settings signal, which the tests of `Observed::resolve` do not reach (on the private bus the portal session most likely opened and its reads failed, since ashpd maps a failed version read to version 1); and macOS, which does not use this path.

## September 30 Tab leaves a focused Button that turns disabled

Task `disabled-focus-tab-trap`, from the design review of the [solid focus ring](#september-29-solid-focus-ring-outside-every-button). In Targets, a focused Switch turns disabled while it switches and stays disabled once the switch clears the branch field. Tab and Shift+Tab then did nothing until a pointer click. gpui-base's Button tracks its focus handle only while enabled (`vendor/gpui-base/src/button.rs:238-244`), so the disabled Switch left the rendered frame with the window's focus still on it. GPUI dispatches a key for a focused element outside the frame from the root dispatch node (gpui-pre 0.3.4 `window.rs` ~6040), above the kit `Root` context where Tab and Shift+Tab are bound (`vendor/gpui-component/src/root.rs:24-25`). While it is focused, a disabled kit Button now keeps its handle in the frame as a target that is not a tab stop, so Tab and Shift+Tab step from its place to the next enabled stop, and Tab never lands on a disabled Button (`vendor/gpui-component/src/button/button.rs`, the "disabled-focus patch" in `GITTURTLE-PATCH.md`). It covers every Button that turns disabled under focus; enabled and unfocused disabled Buttons are unchanged.

Test: `workspace::targets_focus_tests::tab_and_shift_tab_leave_a_focused_switch_that_turns_disabled` renders the application's Targets on a disposable repository, focuses Switch, clears the branch field as a completed switch does, and sends real `tab` and `shift-tab` keystrokes. It requires Tab to reach the Remote field past the disabled Create, and Shift+Tab to return to the branch field. With the kit's `button.rs` from main it fails with focus still on the Switch.

Native evidence, full tier, since the base had to show the trap in the same session as the fix; keyboard only, as the change moves focus and no pixel of any control:
- Builds: base `18a6819` (sha256 `2385dd97…`, main's code when the task started) and candidate `725cdcb` (sha256 `c13a94a9…`; `a4ef856` is the fix and `725cdcb` changes only a comment and the patch record), both debug and clean, the candidate built in a target of its own; `qa.py identity` reported no problem. The branch then merged `main`, whose Settings change (#100) does not reach Targets.
- Host: Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680, Midnight, one session from 13:52 to 14:09 UTC. Input went through Mutter RemoteDesktop with verified X focus, never XTest. Local qaflow drivers on `drive_tabtrap_lib.py` (sha256 `467d6b2b…`) sent every key: `drive_tabtrap_capture.py` (`b2ecd9f1…`) in the paired runs, launched by `drive_tabtrap_pair.py` (`83610d19…`), and directly in the first AT-SPI launch, `drive_tabtrap_rehearse.py` (`be994763…`) in the order walks, and `drive_tabtrap_atspi.py` (`097f5d5e…`) in the AT-SPI comparison.
- Fixture: a fresh repository for each launch, `main` with one commit under the QA identity at a fixed date (HEAD `2ae4f4f`) and a `feature` branch at the same commit, no remote. The switch to `feature` was the only Git write; each switched fixture ended on `feature` with a clean status.
- Route: from launch, Tab 16 reaches the branch picker and Tab 17 Targets; Space opens it with focus on its toggle. With `feature` typed, both builds tab through the Targets toggle, the branch field, Switch, Create, the Remote field, the Remote branch field and the sidebar's Local segment, and Shift+Tab walks the same stops back. With the field empty both skip Switch and Create.
- Switching: with `feature` typed, Tab to Switch and Space. The switch took effect within 0.08 s, too fast to capture its busy state. A "Switched to feature" notice then moves Targets down 40 px, the field clears, and the ring stays on the disabled Switch, identically on both builds. A capture about 4 s later was identical on each of the five switched launches that took one, so the refresh after a switch moves no focus.

Focus after each key once the switched Switch is disabled, by the ring's box and, on the candidate, AT-SPI's focused node:

| Scenario | Key | Base | Candidate |
| --- | --- | --- | --- |
| Tab first | Tab | Switch, no change within 2 s | the Remote field, after 0.050 s |
| | then Shift+Tab | Switch, no change | the branch field, after 0.056 s |
| Shift+Tab first | Shift+Tab | Switch, no change | the branch field, after 0.050 s |
| | then Tab | Switch, no change | the Remote field, after 0.065 s, past both disabled Buttons |

On the base every frame after the switch has the same bytes, the frames after each key included. `qa.py compare <base> <candidate> --mask status-timing` found the frames with Switch focused and with Switch disabled identical in both scenarios, apart from the masked status timing. The frames after a key differ only at the rings they move between. AT-SPI on the candidate named the focused node "Switch" before and after the switch, "Git remote target" after Tab and "Branch to switch to or create" after Shift+Tab. `org.a11y.Status IsEnabled` was set for those launches only and read back `false` after each.

Accessibility: on both builds every disabled Button, among them Create, the unfocused Switch and Fetch, Pull and Push without a remote, reports `enabled` and `sensitive` to AT-SPI, and differs from an enabled Button only by lacking `focusable`. On the candidate the focused disabled Switch also reports `focusable` and `focused`, so its state set matches an enabled, focused Button's. The base's focused disabled Switch was not read. The likely cause is AccessKit's AT-SPI adapter as pinned (`accesskit_atspi_common` 0.19.1, `node.rs:373-377`), which adds `Enabled` and `Sensitive` to every node whose role cannot be read-only, Button among them. So the disabled flag the vendored gpui-base patch sets ([native accessibility](native-accessibility.md#toolkit-patch)) never clears `enabled` or `sensitive` on Linux, and a screen-reader user there lands on a focused "Switch" reported as enabled, on which, by the code, Space does nothing.

Frames: [`evidence/disabled-focus-tab-trap/`](evidence/disabled-focus-tab-trap/), the candidate's `midnight-1000x680-targets-{switch-focused,switch-disabled,after-tab,after-shift-tab}`: `switch-focused` and `after-tab` from the Tab-first launch, `after-shift-tab` from the Shift+Tab-first one, and `switch-disabled`, whose bytes every switched launch on both builds shares. `qa.py privacy scan --redacted --jobs 8` with the local template set found all four clean, and a view of each shows only product UI, the QA identity and a shortened `/tmp/gitturtle-evidence/fixtures/tabtr…` path.

A `code-reviewer` pass found no defect. It confirmed the cause against the pinned gpui-pre source and the test's failure with only the kit's `button.rs` reverted. It asked for the siblings below to be named, and for the patch record to describe the handle's window-wide tab-stop record, AccessKit's Focus action and the ancestor key bindings a focused disabled Button now reaches, all done in `725cdcb`. A `design-reviewer` pass accepted the change. Keeping focus and its full ring on the disabled Button until the keyboard moves it is right for the kit: moving focus there would pull it off Switch during every busy switch, failed ones included. It found the frames as described, and asked for the tier, the accessibility cause and a wider list of what is not covered, all added here. A read-only `verifier` pass checked the cause against the pinned gpui-pre source, ran the test 12 times, and reproduced its failure with only the kit's `button.rs` from main in a separate tree and target. It matched the entry to the run logs, the frames to the captures and the compare, rescanned the frames for privacy, and ran the fast gate, which passed. It failed the entry once, for naming the wrong driver, and passed once that was corrected.

Still open:
- gpui-base's Checkbox, Switch, Radio, Toggle, Link and ColorPicker keep the same enabled-only focus guard, which this patch does not reach. The app disables a focused one in Settings' Follow system Switch while the Omarchy theme is selected (`settings.rs` ~2078), which persists, the diff view's partial-line Checkbox while a stage runs (`diff_view.rs` ~489), the ignore dialog's Checkbox while its plan is prepared (`ignore.rs` ~153) and the profile editor's Checkbox while it saves (`profiles.rs` ~563). Resolved in [October 1 Tab leaves a focused kit control that turns disabled](#october-1-tab-leaves-a-focused-kit-control-that-turns-disabled).
- No disabled Button reports a disabled state to AT-SPI on Linux, above, although [native accessibility](native-accessibility.md#toolkit-patch) says the vendored patch gives it AccessKit's disabled flag. Addressed in [October 1 disabled Buttons report disabled to AT-SPI](#october-1-disabled-buttons-report-disabled-to-at-spi).
- A focused control that leaves the frame, rather than turning disabled, probably traps Tab the same way, since GPUI keeps its focus and neither the app nor the kit registers a focus-lost listener. One example is Cancel remote check (`rewrite_review.rs` ~681), which renders only while a check runs. This is from reading the code, not from a run.

Not covered: macOS, native Wayland, fractional scale factors, release builds, the switch's busy phase, a failed switch, where Switch turns enabled again under focus, and other Buttons that turn disabled under focus, which only the patch's reach, not a run, covers. No run or test pressed Space, Enter or another ancestor binding while the disabled Switch held focus, which the patch now lets through. Only AT-SPI states were read; no screen reader spoke (Orca, or VoiceOver and whether it calls the Button dimmed).

## September 30 selected segments like the shared helper

Task `selected-segment-hover-helper`, from the design review of the [solid focus ring](#september-29-solid-focus-ring-outside-every-button). Settings' density segments and the project hub's mode segments dimmed a hovered selected segment to 0.9 opacity, and its focus ring with it. They now hover with `appearance::control_selected_hover`, as the shared `button()` helper and History's segments do: full opacity and the palette's `selected_hover`. By the owner's decision of 2026-09-30, a selected segment also rests on the palette's opaque `selected` through `control_button_variant(true)`, as History's do, instead of the kit's translucent ghost selected fill, so hover takes the helper's step rather than a jump from the ghost fill (a hue flip in Kanagawa Lotus). Unselected segments keep the ghost, and while the hub is busy every mode keeps the ghost's disabled look (`settings.rs`, `projects.rs`). `DESIGN.md` no longer says these segments dim.

Tests: `settings::segment_tests::a_selected_density_rests_and_hovers_like_the_shared_helper_and_the_others_like_the_ghost` and `projects::tests::a_selected_mode_rests_and_hovers_like_the_shared_helper_and_the_others_like_the_ghost`, in Midnight, Porcelain and Kanagawa Lotus. Each first proves that Tab reached the segment. It then requires the selected segment to paint `selected` at rest and `selected_hover` under the pointer, focused or not, with a full accent ring exactly when focused. It requires an unselected segment to paint nothing at rest and the ghost's own hover under the pointer. The project test also holds a busy hub's selected mode to no fill. On main's product code, with only the tests' new debug selectors added, both fail at the resting fill. Giving unselected segments `control_button_variant(false)` fails both at the hovered unselected segment. The test platform paints no glyphs, so no view test checks the label's color; the native frames below do.

Native evidence, full tier:
- Builds: base `703d900` (sha256 `0f2008b1…`, main's code when the task started) and candidate `deb1ee9` (sha256 `3a007da3…`), both debug and clean; `qa.py identity` reported no problem. The later commit `0500dbf` changes only tests. The branch then merged `main`, whose Settings change (#96) does not reach the density control.
- Host: Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680, one session from 13:11 to 13:20 UTC, base then candidate for each palette and control. Input went through Mutter RemoteDesktop with verified X focus. A local qaflow driver, `drive_segments_capture.py` (sha256 `ffee9444…`, library `drive_segments_lib.py`, `1712d13b…`), took every frame.
- Fixture and routes: the density control in Settings on the `theme-fixture` repository (HEAD `52f471a`, unchanged by every launch), reached with Ctrl+comma and a 25-step wheel scroll; Tab 35 focuses the selected Comfortable. The mode control on the hub, opened with no repository; Tab 5 focuses the selected Open. The pointer moves after the last Tab, since GPUI shows no hover while the last input was a key. A busy hub was not captured, because holding it busy needs a network action or a write; the project test covers it.
- `qa.py compare <base> <candidate> --mask status-timing`, every palette. Each frame differs in one region, inside the selected segment's box, and nowhere outside that box grown by the ring's 3 px. Outside `selected-focus-hover`, where the base's faded ring arcs reach them, the box's unchanged pixels are its 24 rounded-corner pixels, plus, in the Midnight and Porcelain mode frames `rest`, `selected-focus` and `unselected-hover`, a 10 px run at (635-644, 252) that both builds draw alike:
  - density, box (823,179)-(900,198): 1,439 px in `rest`, `selected-hover`, `selected-focus` and `unselected-hover`, and 1,863 in `selected-focus-hover`, 400 of them the ring's band, where the base's ring is faded;
  - mode, box (601,238)-(719,270): 3,742 px in `rest`, `selected-focus` and `unselected-hover` (3,752 in Kanagawa Lotus), 3,752 in `selected-hover`, and 4,392 in `selected-focus-hover`, 616 of them the band.

Fills, the selected segment's most common color, identical for both controls, at rest and hovered, with the contrast between the two:

| Palette | Base | Candidate and History's selected segment |
| --- | --- | --- |
| Midnight | `#161d29` → `#151c28`, 1.011:1 | `#223b3b` → `#274643`, 1.163:1 |
| Porcelain | `#bac3e1` → `#c0c8e3`, 1.053:1 | `#dce6f6` → `#d0dbf0`, 1.107:1 |
| Kanagawa Lotus | `#d8cb82` → `#dace87`, 1.029:1 | `#c7d7e0` → `#becfdb`, 1.083:1 |

History's selected segment, sampled in the same launches, is the same on both builds, and the candidate's segments match it exactly. The selected label, against the resting and the hovered fill, reads 9.50 and 8.17:1 (density) and 10.25 and 8.81:1 (mode) in Midnight, 8.96 and 8.13, and 10.69 and 9.65 in Porcelain, and 5.93 and 5.47, and 6.80 and 6.28 in Kanagawa Lotus, where the base's density label read 4.07 and 3.53:1. In Kanagawa Lotus the selected label now takes `control_label` (`#494a57` and `#41414e`, History's is `#484956`) while its ghost neighbours keep `text`, 1.37:1 apart; on the base they were within 1.05:1. Hovered, the candidate's label keeps its color, where the base's faded.

Ring pixels in the 1 to 3 px band outside the focused selected segment, against the same build's unfocused frame (focused against rest, and focused and hovered against hovered). Each cell gives the differing pixels out of the band; the counts are the same on both builds and in every palette:

| Control | Top | Bottom | Left | Right |
| --- | --- | --- | --- | --- |
| Density, Comfortable | 160/231 | 160/231 | 44/57 | 44/57 |
| Mode, Open | 242/354 | 242/354 | 70/96 | 70/96 |

The band's 3x3 corner squares hold no ring; the ring's rounded corners fall inside the box. No neighbour covers a side: the right side sits in the 3 px gap before Compact and Clone. Every side's strongest pixel:

| Palette | Candidate, focused and focused-hovered | Base, focused | Base, focused and hovered |
| --- | --- | --- | --- |
| Midnight | `#75e0bb`, 11.43:1 | 11.43:1 | `#6bcbab`, 9.36:1 |
| Porcelain | `#3455a6`, 6.55:1 | 6.55:1 | `#4866af`, 5.17:1 |
| Kanagawa Lotus | `#4d699b`, 4.59:1 | 4.59:1 | `#5e769e`, 3.84:1 |

Frames: [`evidence/selected-segments/`](evidence/selected-segments/), the candidate's five states of each control in each palette (30 files): `<palette>-1000x680-density-{rest,selected-hover,selected-focus,selected-focus-hover,unselected-hover}` and `<palette>-1000x680-mode-<state>-panel`. The mode files are crops of the hub's action panel, window (580,215)-(985,480), which holds the whole mode control and its ring: the full-window hub frames did not pass the privacy scan with the local template set and are not committed. `qa.py privacy scan --redacted --jobs 8` found all 30 committed files clean, and a view of each shows only product UI and the `theme-fixture` tab.

A `code-reviewer` pass on each of the two product commits found no defect. The second asked for the widened contract to be recorded in `tasks.json` and for a guard on the unselected segments, both done. A `design-reviewer` pass accepted the change. The selected segments keep their surface with a gentle hover and a whole ring at full accent, and the Kanagawa Lotus labels, 3.53 to 4.52:1 on the base (the hovered ones under 4.5:1), now clear 4.5:1 (5.47 to 6.80). It ruled that the hovered unselected segment's lead below is acceptable, since History's segments, the model, show the same. It also accepted the mixed labels, which come with that label fix. It left the pressed fill below as a follow-up, and asked for the corrections to this entry that are now made. A read-only `verifier` pass ran the tests 20 times without a failure, reproduced their failures on main's product code, the unselected mutant and a busy-look mutant, reran the masked compare on all 30 pairs, recounted the ring bands in Midnight and Kanagawa Lotus on both builds, matched the committed frames to the captures and crops, rescanned them for privacy and passed the fast gate. It found a miscount and an imprecise sentence in this entry, now corrected, and passed once the design review was recorded.

Found on the way:
- The selected segment's lead over an unselected one changed. In Porcelain a hovered unselected segment (`#d5daed`, 1.302:1 against the track) now stands further off the track than the resting selected one (`#dce6f6`, 1.176:1), where the base's selected segment led (1.638:1). In Kanagawa Lotus the two have the same luminance and differ in hue. In Midnight the selected segment still leads, 1.529:1 against 1.058:1.
- Pressing an unselected segment still shows the ghost's translucent pressed fill, which on release gives way to `selected_hover` while the pointer stays on it, and to the opaque `selected` once it leaves. On the base the pressed and the selected fill were the same ghost fill, so the change was seamless. Computed from the tokens and the kit's formulas, not captured, the pressed fill steps to `selected_hover` by 1.65:1 in Midnight, 1.26:1 in Porcelain and 1.03:1 with a change of hue in Kanagawa Lotus, and to `selected` by 1.42:1 (`#151d2a` to `#223b3b`), 1.39:1 (`#bac3e1` to `#dce6f6`) and 1.11:1 (`#d8cb82` to `#c7d7e0`). History's unselected segments, on the helper's unselected look, press to almost their selected fill (1.05 to 1.16:1).
- Settings' Tab order passes about 20 controls below the page's view without scrolling them into view, on both builds.

Not covered: macOS, fractional scale factors, release builds, the accessibility tree, a busy hub on the native build, the pressed state and a segment becoming selected by a click or Space, the other built-in palettes (One Dark among them, the other palette that moves the control label) and custom or imported themes, and the merged branch's Settings route: the frames come from `deb1ee9`, and the merged `main` adds the Settings page's scroll tracking (#96), which does not change how a segment paints.

## September 29 desktop code font row states

Task `code-font-row-evidence` closes the open items of [the desktop monospace font check](#september-27-desktop-monospace-font-on-linux) for Settings' **Use the desktop's monospace font** row. No product behaviour changed; the only source changes are for the test: Linux test hooks in `desktop_text.rs` that read the current lookup's generation and answer a given lookup through the real generation check, and a debug selector on the setting description, which only test builds read (the owner added `desktop_text.rs` to the task's scope on 2026-09-30). `settings::picker_tests::the_code_font_row_describes_off_pending_and_on` clicks the switch and asserts the text the row passes as its accessible label, "Use the desktop's monospace font. ‹description›", in each state the setting passes through. Off reads "Code uses the bundled DejaVu Sans Mono. Turn on to use fontconfig's monospace font when it is fixed-width.", pending reads "Looking up the desktop's monospace font…", on reads "Code uses ‹family›, the desktop's monospace font.", and unavailable reads "‹reason› Code uses the bundled DejaVu Sans Mono." A lookup's reply that arrives after the setting is turned off leaves the row off, and once it is on again an earlier lookup's reply leaves it pending until the current lookup answers. The description in the label is the text the row draws, but GPUI's test context cannot read drawn text, so the drawn copy is checked only in the frames below.

Native evidence, full tier, on Omarchy 4.0.4 (Hyprland 0.56.2, native Wayland):
- Build: `main` `9f55de0`, release, clean, sha256 `0d053ea1…` (`qa.py identity`). The branch changes the row only by a selector that test builds alone read, so it draws the same.
- Outputs: temporary headless outputs, fullscreen, tall enough to hold the whole Settings page, because Settings does not scroll a control that Tab focuses into view (see the findings). Wide layout 1480 × 2000 at scale 1. Stacked layout, below the 1,060 px breakpoint, 1000 × 2400. An 18 pt interface text size at 1480 × 2600. Scale 1.25 on a 1850 × 2500 output (1480 × 2000 logical).
- Launches: each had a fresh HOME with the `GitTurtle QA` identity and fresh XDG directories, and opened Settings with Ctrl+,. The fixture was a disposable `scripts/create-demo-repo.py` repository at `/tmp/gitturtle-evidence/narrow-8/demo`. The desktop's `monospace` alias resolves to JetBrainsMono Nerd Font.
- Input: keyboard only, through `wtype` after checking the active window's PID. Tab 34 times from the Settings entry reaches the switch, and Space then toggles it. Hover moved the compositor's pointer onto the switch, and the pointer is drawn in those frames.
- Pending: an `fc-match` wrapper on the launch's PATH answered after 0.42 s (0.3 s for the stacked Daylight run), within the app's 500 ms bound. Whole-output grabs every 50–170 ms caught the description in its pending text, with the knob still travelling in the switch's spring animation.
- Frames: cropped from the whole-output grabs to the Code text size sample, the row and its separator. All 27 are in [`evidence/code-font-row/`](evidence/code-font-row/): wide Midnight and Daylight in off, off focused, pending, on focused, hovered on and hovered off; stacked Midnight and Daylight in off, pending and on; 18 pt Midnight and 1.25 Midnight in off, pending, on and hovered on. One stacked Daylight lookup overran the 500 ms bound while a build loaded the host, and its frame shows the unavailable state: "fontconfig did not answer. Code uses the bundled DejaVu Sans Mono." A privacy scan of the 27 committed files with the local template set came back clean, and each was viewed at full size.

A `design-reviewer` pass found the frames complete. The title and muted description use the Settings helper's sizes. The title's left edge and the switch's right edge line up with the code sample in every layout, and the row's spacing is balanced. At 18 pt the description wraps without truncation, and at 1.25 the text, switch and separator edges are sharp.

**Findings**, for the owner and not fixed here:
- **No focus indication.** The focused switch draws no focus indication: `wide-midnight-off-focused` and `wide-daylight-off-focused` are byte-identical to their unfocused frames, and no focused frame shows a ring, against `DESIGN.md`'s visible focus rule. The Settings switches come from `vendor/gpui-component/src/switch.rs`, whose render sets no focus style, so all four Settings switches are affected.
- **No hover change.** Hovering changes nothing but the pointer image, off or on.
- **Low knob contrast.** The checked knob sits at 1.37:1 on its track in Midnight (`#E8EEF7` on `#75E0BB`) and 2.28:1 in Daylight (`#253247` on `#08755D`), under 3:1. The off track reads 1.39:1 against the card, though the knob outlines the control at 9–10:1. No palette test covers these switch tokens.
- **The switch moves at 18 pt.** The off description wraps to two lines and the other states' take one, so toggling moves the switch 11 px up under the pointer.
- **Focus outside the view.** Settings does not scroll a Tab-focused control into view. At 1480 × 800, 19 consecutive Tabs moved focus below the fold without changing a pixel.

Not covered: pointer clicks (no pointer-click tool on this host; the view test toggles the switch), the turning-off transition, and macOS, where the row does not exist.

## September 29 whole focus ring in Tags and Reflog

Task `tags-reflog-focus-ring`, from the design review of the [solid focus ring](#september-29-solid-focus-ring-outside-every-button). The Tags dialog clips its body, and the Tags and Reflog lists scroll, so GPUI masks each to its own bounds on both axes (gpui-pre 0.3.4 `Style::overflow_mask`). A focused Create tag… lost its ring's top, a focused Tags row kept only its corners, and a Reflog entry lost its top or bottom and both sides. Each container now keeps the installed ring's gap plus width as room inside its clip and gives it back, so nothing moves unfocused (`tags.rs`, `reflog.rs`):
- the Tags body through the title's margin and the dialog's gap above the footer;
- the Reflog content through its side margins and that gap;
- each list through its own margin, the Reflog list through a wrapper, because its scrolling parent counts each child's whole box.

Tests:
- `tags::tests::tag_browser_keeps_room_for_every_focus_ring` (Create tag…, the first and last rows) and `reflog::tests::reflog_browser_keeps_room_for_every_focus_ring` (Read log, the first and last entries) require each control, grown by the ring's gap plus width, to lie inside every ancestor content mask. With a zero-width ring, which is main's layout, they require every painted rectangle and named element to stay where it was, and a wheel step over each control to move nothing. On main's layout both failed, naming every control ("create-tag: ring … mask …").
- `tags::tests::dialog_footer_gap_is_the_kits` and its Reflog twin measure a kit dialog's gap above the footer with no override, and require it to equal the 16 px the room is taken from.

Native evidence, full tier:
- Builds: base `703d900` (sha256 `0f2008b1…`) and candidate `6c61c45` (sha256 `8f15425e…`), both debug and clean; `qa.py identity` reported no problem. The later commit `7efc226` changes only tests. The branch then merged `main`, whose Settings change (#96) does not reach these dialogs.
- Host: Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680. Input went through Mutter RemoteDesktop with verified X focus, and a local qaflow driver, `drive_tags_pair.py` (sha256 `56fa10e4…`), took every frame.
- Fixture: a `scripts/create-demo-repo.py` repository with 15 more tags under the QA identity, 16 in all, and 12 HEAD reflog entries (HEAD `52f471a`), unchanged by every launch. Both lists overflow: Tags shows rows 0 to 9 and Reflog rows 0 to 5, the last partly.
- Route: the command palette's "browse and manage tags" and "browse reflog" open each dialog with nothing focused. In Tags, Tab 1 reaches the filter, 2 Create tag…, 3 the first row and 18 the last. In Reflog, Tab 1 reaches the scope, 2 Read log, 3 the filter, 4 the first entry and 15 the last. Neither list scrolls a focused row into view on either build, so for the last rows each list was first scrolled to its end with the wheel.
- The first Porcelain Reflog pair differed by one pixel outside the ring band. Each build's first run differs from its own rerun by that one antialiased glyph pixel, so the rerun is the one reported.
- `qa.py compare <base> <candidate> --mask status-timing`, both palettes. These are identical: the Tags and Reflog rest frames, the Reflog with an entry selected, and both lists scrolled to their end unfocused. The lists now clip 3 px further out, but in these frames that band holds only the dialog's surface on both builds. Mid-scroll, a partly visible row's text or fill is cut 3 px further out, into the gaps above and below each list, as a tab scrolled partly out of the strip is; no frame shows it. Each focus frame differs only in the 3 px band around the focused control:
  - 228 px for Create tag…, 1,286 for the first or last Tags row, 1,574 for the first or last Reflog entry and 62 for Read log;
  - 1,339, 70 and 140 for the Tags filter, the Reflog scope and the Reflog filter, Inputs whose toolkit ring the containers also cut.

Ring pixels in the 1 to 3 px band outside each focused Button, against the same build's unfocused frame at the same scroll. Each cell gives the differing pixels out of the band; the counts are the same in both palettes:

| Control | Build | Top | Bottom | Left | Right |
| --- | --- | --- | --- | --- | --- |
| Create tag… | base | 0/333 | 228/333 | 62/84 | 62/84 |
| | candidate | 228/333 | 228/333 | 62/84 | 62/84 |
| First Tags row | base | 0/1,698 | 1,138/1,698 | 0/102 | 0/102 |
| | candidate | 1,138/1,698 | 1,138/1,698 | 74/102 | 74/102 |
| Last Tags row, list at its end | base | 1,138 | 0 | 0 | 0 |
| | candidate | 1,138 | 1,138 | 74 | 74 |
| First Reflog entry | base | 0/2,118 | 1,418/2,118 | 0/108 | 0/108 |
| | candidate | 1,418/2,118 | 1,418/2,118 | 78/108 | 78/108 |
| Last Reflog entry, list at its end | base | 1,418 | 0 | 0 | 0 |
| | candidate | 1,418 | 1,418 | 78 | 78 |
| Read log | base | 186/270 | 186/270 | 62/84 | 0/84 |
| | candidate | 186/270 | 186/270 | 62/84 | 62/84 |

The base's corner stubs are where the ring's rounded corners fall inside the row's box, which the band does not count. Every candidate Button side reads 11.43:1 in Midnight and 6.55:1 in Porcelain. The Inputs draw the toolkit's own focus style, unchanged: an accent border at 7.53:1 and 4.67:1 and a half-accent outer band at 3.69:1 and 2.27:1, now on all four sides.

Frames: [`evidence/tags-focus-ring/`](evidence/tags-focus-ring/), the candidate's eleven states in each palette (22 files): `tags-{rest,filter-focus,create-focus,row-first-focus,row-last-focus}` and `reflog-{rest,scope-focus,readlog-focus,row-first-focus,row-last-focus,selected-rest}`. `reflog-selected-rest` records the collapse described below, on both builds, not an accepted layout. `qa.py privacy scan --redacted --jobs 8` with the local template set found all 22 clean. A full-resolution view shows only the demo repository's content and fictional author, the QA identity, a shortened `/tmp/gitturtl…` path and the `tags-reflog` tab. The fix changes the views in the #85 frames `evidence/themes/button-focus-ring/{,base-}porcelain-1000x680-clip-tags-create-tag.png`, `porcelain-1000x680-clip-tags-row.png`, `porcelain-1000x680-clip-reflog-row{1,2}.png` and `{midnight,porcelain}-1000x680-reflog-row-focus.png`, which stay as that build's record of the defect. A `design-reviewer` pass accepted the frames. Every named control's ring is whole and no inset shows. The line a focused row out of view draws is acceptable, since the base showed no focus there at all, and a follow-up that reveals the focused row would remove it. A read-only `verifier` pass reran the compare and the privacy scan, the tests (and their failures on main's layout, rebuilt in a scratch copy) and the fast gate. It found one blocking defect outside the criteria, the narrowed open lists, and passed once they named the containers that still clip and the design review had accepted the frames.

Found on the way, on both builds and not changed here:
- Neither list scrolls a focused row into view. With the Reflog entry just below the list's view focused, the candidate draws the top of its ring as a 2 px accent line in the list's new room, where the base showed no focus at all.
- Selecting a Reflog entry at 1000x680 collapses the entry list and the changed-file list to nothing (the metadata says "3 changed files" and none shows), and the message editor to its two borders, so its first line paints over "Create a new branch at this commit…" (`reflog-selected-rest`). The dialog growing to its height bound is expected. The likely cause, unverified, is that the scrolling content's children keep the default flex-shrink and have no automatic minimum height, so the column shrinks them instead of scrolling.
- Other scrolling lists of full-width Buttons keep no room for the ring, among them the branch chooser (`branch_actions.rs`), the worktree manager (`worktrees.rs`) and the tag inspector's Push to… list.
- Read log's `refresh-cw` icon exists in neither the app's icons nor gpui-kit-assets, so it and five other buttons show an empty icon slot.

Not covered: macOS, fractional scale factors (at 1.5 the room and the reduced gap snap separately, so the footer can rise one device pixel), release builds, the accessibility tree, and the branch menu's route to either dialog.

## September 29 whole focus ring on Your themes rows

Task `your-themes-rows-whole-focus-ring`, from the finding in [frames retaken under the solid focus ring](#september-29-frames-retaken-under-the-solid-focus-ring); the owner kept `DESIGN.md`'s whole-ring promise. Three faults cut the bottom edge of the ring around a focused Edit…, Export… or Delete… in Settings' Your themes list:
- **Import highlight.** The highlighted row painted its fill as its own background, after the row above it, so the fill covered the ring on that row's actions.
- **Hover.** A hovered plain row's fill did the same to the row above it.
- **Reveal.** A focused row revealed flush against the status bar lost the ring to the Settings page's viewport, which never scrolled for it.

Every row's fill, the import highlight and the hover surface alike, is now painted in one layer beneath all the rows, with each row's bounds, corners, color and clip, so unfocused pixels are unchanged (`settings.rs`). The Settings page has its own scroll handle and reveals a focused row with the installed ring's gap plus width of room, once per focus change, placed where the list will stand after its own reveal (`settings::planned_rows_top`, shared with `rows_off_boundary`). A row that was exactly flush moves the page up 3 px.

Tests:
- `theme_editor::tests::a_focused_row_actions_ring_is_whole_beside_the_import_highlight` failed on the old paint order ("a fill drawn after the ring covers its bottom edge") and without the rows' empty hover style ("the pointer alone … changes the rows' fills").
- `theme_editor::tests::a_revealed_row_keeps_its_ring_inside_the_settings_page` failed without the page reveal ("the ring … 62×34 lies inside the page's viewport (0, 84) 1000×570").

Native evidence, full tier:
- Builds: base `de09c65` (sha256 `c0c51988…`) and candidate `3f4fec1` (sha256 `6b88ed4a…`), both debug and clean; `qa.py identity` reported no problem. The branch later merged `main`, whose [tab-strip change](#september-29-focus-ring-inside-the-repository-tab-strip) moves nothing unfocused and touches no Settings code.
- Host: Ubuntu 26.04, GNOME 50 on Wayland, XWayland `:0` at scale factor 1, window 1000x680. Input went through Mutter RemoteDesktop with verified X focus.
- Fixture: `theme-fixture` (HEAD `52f471a`), unchanged by every launch. The store holds Seed 01 Midnight, Seed 02 Braden and Seed 03 Graphite with the palette active, as the committed frames did. Import… adds "Imported Harbor" (base Midnight) through the Nautilus FileChooser. It is clicked with the pointer, so keyboard focus stays where Settings put it, and the store it writes was byte-identical on both builds. The flush and rest launches start from that store with the fourth theme already saved, and import nothing.
- Focus: from Settings' entry focus, Tab 37 reaches Seed 02's Edit…, 40 Seed 03's and 43 Imported Harbor's, the same on both builds. For the hover frame the pointer then moves onto the plain row below, because GPUI shows no hover while the last input was a key.
- `qa.py compare <base> <candidate> --mask status-timing`, both palettes:
  - `highlight` and `rest`: identical.
  - `focus-hover-below` and `focus-beside-highlight`: 100 px each, one region along the ring's bottom edge.
  - `focus-on-highlight` and `flush`: the page moved up 3 px (83,834 and 78,558 px in Midnight, 85,126 and 79,859 in Porcelain). Inside the page's viewport (y 84 to 653) the candidate's y 84 to 650 equals the base's y 87 to 653 exactly, everything outside the viewport is identical, and of the three rows it reveals at the bottom, y 651 and 652 carry the ring's bottom edge and y 653 is the card's surface, 1 px above the status bar's border.

Ring pixels in the 1 to 3 px band outside the focused Edit…, against the same build's unfocused frame at the same scroll. Each cell gives the differing pixels out of the band; the counts are the same in all four frames and both palettes:

| Build | Top | Bottom | Left | Right |
| --- | --- | --- | --- | --- |
| base | 114/162 | 14/162 | 62/84 | 62/84 |
| candidate | 114/162 | 114/162 | 62/84 | 62/84 |

The base's 14 bottom pixels all lie where the rounded corners bend into the band; between them it has no bottom edge. The candidate's strongest ring pixel reads 10.93:1 on Midnight's list surface, 8.80:1 on a hovered row's fill and 7.48:1 on the highlight, and 6.21:1, 5.77:1 and 5.57:1 in Porcelain. Every side reads at least 5.57:1.

Frames, in [`evidence/themes/linux-gaps/`](evidence/themes/linux-gaps/): the six `imported-{midnight,porcelain}-1000x680-{highlight,focus-beside-highlight,focus-on-highlight}.png` are retaken on the candidate, and six are new: `…-focus-hover-below`, `…-flush` (Imported Harbor's Edit… focused at the list's end against the status bar, with no import) and `…-rest` (the same view unfocused). The base's beside and on frames reproduced byte for byte the frames #89 committed from build `650a76e`, so the route matches theirs. The retaken `highlight` frames differ from the ones they replace, taken on another host (`84df3b3`), in 505 and 652 px of text antialiasing by at most one color level; base and candidate are identical there. A local qaflow driver, `drive_themes_capture.py` (sha256 `22bf5b47…`), took every frame. `qa.py privacy scan --redacted --jobs 8` with the local template set found all 12 clean, and a full-resolution view shows only theme names, the `theme-fixture` tab and Settings text. A `design-reviewer` pass accepted the frames: the ring is whole on every side, the unfocused fills are unchanged, and the 3 px page move is acceptable, because it is exact, happens only when the row lacks room, and matches the list's own 3 px. A read-only `verifier` pass recounted the ring pixels, reran the compare, the two tests (and their failures with the fix taken out) and the fast gate, and passed the criteria once the earlier finding pointed here.

Not covered: macOS, fractional scale factors, release builds, the accessibility tree and touch input: the fill layer checks `Hitbox::is_hovered`, which unlike GPUI's own `.hover()` does not ask whether the last input was a touch, so after a touch the row under it would keep its hover fill; no backend GitTurtle ships sends touch input yet.

## September 29 GitTurtle under XWayland on Omarchy

Task `omarchy-xwayland-evidence` checks the release build as an X11 client on Omarchy, launched as `docs/linux.md` suggests with `env -u WAYLAND_DISPLAY`. It is evidence only; no product code changed. The runs took place between 2026-09-29 23:17 and 2026-09-30 00:20 UTC.

- Host: Omarchy 4.0.4-1, Hyprland 0.56.2 (`efb5099`), `xorg-xwayland` 24.1.13-1, on the 1366 × 768 panel (310 × 170 mm) at scale 1. `hyprctl getoption xwayland:force_zero_scaling` read `true` before and after.
- Build: `main` `9f55de0`, release, clean, sha256 `0d053ea1…` (`qa.py identity`).
- Outputs: temporary headless outputs at 1480 × 800 and scale 1, and at 1850 × 1000 and scale 1.25 (1480 × 800 logical), with the window fullscreen. `hyprctl monitors -j` matched its reading from before each run.
- Launches: every launch had a fresh HOME and XDG directories, the `GitTurtle QA` identity, and a fake `~/.local/state/omarchy/current/` seeded from Tokyo Night, with the preference set to the Omarchy theme. For X11, only the app's environment dropped `WAYLAND_DISPLAY` (`DISPLAY=:0`, no `GPUI_X11_SCALE_FACTOR`). `hyprctl clients -j` listed the window as `com.gitturtle.desktop` with `"xwayland": true`, and every native launch with `false`.
- Fixtures: a `scripts/create-demo-repo.py` repository with one staged file at `/tmp/gitturtle-evidence/xwayland-8/demo`, and a disposable repository whose commit `ee9b9ce` adds `badge.svg`. Neither changed during the runs: their HEAD, `git status --porcelain=v2` and index digest were the same after every launch.
- Input: `wtype` cannot drive an X11 window here, because its keymap never reaches XWayland; a logging X11 window received its Down, Return and Ctrl+, as Escape. The checks therefore went through Hyprland's own `hl.dsp.send_shortcut({ mods, key, window = "pid:…" })`, each after checking the active window's PID. The same logging window received every key correctly under XWayland and under native Wayland. History and Compare frames at both scales opened from a seeded session restore (commit `e96f692`, `src/App.tsx`). On native Wayland, keyboard runs of the same flows matched the restored frames apart from the selected file row and a few pixels.

Results under XWayland, each with a native Wayland run of the same flow for comparison:
- **Theme.** The Omarchy card is selected and reads "Follows Tokyo Night" at both scales, and History, Compare and Settings draw the same three most common colours as native Wayland.
- **Shortcuts.** Ctrl+B collapses the branch sidebar to the rail and brings it back. Ctrl+Shift+P opens the command palette with its search focused, and Escape closes it. Ctrl+, opens Settings.
- **Typing.** On Changes (Ctrl+2, then Tab three times), "Add QA note" appeared in the commit title with the 11/72 count, and the button read "Commit 1 file". Nothing was committed.
- **Clipboard.** On `badge.svg` in Compare, Shift+Tab 14 times focused **Copy After source**, and Space copied it. `wl-paste --no-newline` then returned 290 bytes, identical to `git show ee9b9ce:badge.svg`. The clipboard's earlier content was neither read nor kept.

What differs from native Wayland:
- **Scale.** An X11 window takes its scale from `GPUI_X11_SCALE_FACTOR`, then `Xft.dpi`, then the monitor's physical size (`gpui-pre-linux` 0.3.4, `linux/x11/client.rs`). It does not take Hyprland's output scale, which with `force_zero_scaling` on does not scale X11 windows either. With none set, GitTurtle drew at about 7/6 on both outputs, which matches this panel's 112 dpi. The full-width rules under the tab bar, the header and the branch bar sat at 41, 103 and 158 px, and the status bar was 29 px tall. Native Wayland drew them at 35, 89, 136 and 25 px at scale 1, and at 44, 109, 167 and 31 px at scale 1.25. So the X11 window is about 17% larger than native at scale 1 and about 7% smaller at 1.25. At 1480 × 800, History's SHA column no longer fits, the Local, Remote and Worktrees tabs wrap, and the inspector cuts off before the parent chip.
- **Clipboard types.** While GitTurtle owned the clipboard, XWayland offered `SAVE_TARGETS` and `text/plain;charset=utf-8`/`UTF-8`, where native Wayland offered `text/plain;charset=utf-8`, `UTF8_STRING`, `text/plain` and a `pid/‹pid›` type. `wl-paste` received the same bytes either way.

The 16 frames are in [`evidence/omarchy-xwayland/`](evidence/omarchy-xwayland/). They cover History, Compare and Settings on each backend at both scales, plus X11 frames of Ctrl+B, the palette, the typed commit title and the focused Copy After source. Every committed file's sha256 matches a frame that a privacy scan with the local template set passed, and each was viewed at full size.

Not covered: pointer hover and clicks under XWayland (moving the pointer produced no hover, and this host has no click tool), `GPUI_X11_SCALE_FACTOR` or `Xft.dpi` set by hand, and a paste target that asks only for bare `text/plain`. Two observations outside this task: Copy After source gives no visible feedback on either backend, and on native Wayland, after Ctrl+B twice, History's Author, Date and SHA columns sat 1–2 px to the right of where they started.

## September 29 focus ring inside the repository tab strip

The repository tab list scrolls horizontally, and GPUI masks any scrolling element to its own bounds on both axes (gpui-pre 0.3.4 `Style::overflow_mask`). So the [solid focus ring](#september-29-solid-focus-ring-outside-every-button), 2 px of accent 1 px outside the Button, lost its top and bottom on every tab and close button, and the first tab lost its left side too. The list now keeps the installed ring's width plus gap as padding on every side and gives it back as a negative margin (`repository_tabs.rs`, `render_repository_tabs`), so no tab moves.

Test: `repository_tabs::tests::repository_tab_list_keeps_room_for_every_focus_ring` opens two tabs, reads the tab list's content mask from the painted scene and requires every tab and close button, grown by the installed ring's gap plus width, to lie inside it, with either tab selected. With the ring's footprint set to zero, which is main's styling, it also requires identical bounds for every tab, close button and the header below. On main's layout it failed: "the ring around repository-tab-0 … lies outside the tab list's content mask".

Native evidence, full tier:
- Builds: base `8089625` (sha256 `5631d40c…`) and candidate `cf54ac7` (sha256 `e9c7d074…`), both debug and clean; `qa.py identity` reported no problem.
- Host: Ubuntu 26.04.1, GNOME Shell 50.1 on Wayland, XWayland `:0` at scale factor 1, window 1000x680. Input went through Mutter RemoteDesktop with verified X focus.
- Fixtures: three `scripts/create-demo-repo.py` repositories, `alpha`, `beta` and `gamma`, opened as tabs from a seeded `repository-session.json`. Stores: `qaflow.generated_store` for Midnight and Porcelain.
- Focus: from the launch focus, Tab 7 reaches the first tab, 9 the middle tab, 10 its close button and 11 the last tab, the same order on both builds.
- `qa.py compare <base>/captures <candidate>/captures --mask status-timing`: rest and hover frames identical in both palettes. Each focus frame differs only in the 3 px band around the focused control, 0 px inside its box or elsewhere: 274 px (first tab), 192 (middle), 124 (close button) and 252 (last), in both palettes.

Ring pixels in the 1 to 3 px band outside each focused control, against the same build's unfocused frame. Each cell gives the differing pixels out of the band, then the strongest contrast:

| Palette, control | Build | Top | Bottom | Left | Right |
| --- | --- | --- | --- | --- | --- |
| Midnight, first tab | base | 0/150 | 0/150 | 0/84 | 62/84, 7.48 |
| | candidate | 106/150, 10.45 | 106/150, 10.45 | 62/84, 10.45 | 62/84, 7.48 |
| Midnight, middle tab | base | 0/135 | 0/135 | 62/84, 10.45 | 62/84, 10.45 |
| | candidate | 96/135, 10.45 | 96/135, 10.45 | 62/84, 10.45 | 62/84, 10.45 |
| Midnight, close button | base | 0/84 | 0/84 | 62/84, 10.45 | 62/84, 10.45 |
| | candidate | 62/84, 10.45 | 62/84, 10.45 | 62/84, 10.45 | 62/84, 10.45 |
| Midnight, last tab | base | 0/180 | 0/180 | 62/84, 10.45 | 62/84, 10.45 |
| | candidate | 126/180, 10.45 | 126/180, 10.45 | 62/84, 10.45 | 62/84, 10.45 |
| Porcelain, first tab | base | 0/150 | 0/150 | 0/84 | 62/84, 5.57 |
| | candidate | 106/150, 7.01 | 106/150, 7.01 | 62/84, 7.01 | 62/84, 5.57 |
| Porcelain, middle tab, close button and last tab | base | 0 | 0 | 62/84, 7.01 | 62/84, 7.01 |
| | candidate | 96/135, 62/84 and 126/180, 7.01 | the same | 62/84, 7.01 | 62/84, 7.01 |

The first tab's right side lies on the selected fill, which explains its lower figure; every other side lies on the panel. The ring's rounded corners leave the band's 3 × 3 corner squares unchanged on both builds.

Frames: [`evidence/tabs-focus-ring/`](evidence/tabs-focus-ring/), the candidate's rest and four focus frames in each palette (10 files). `qa.py privacy scan --redacted --jobs 8` with the local template set found them clean (26.3 s for 12 frames, and again on the 10 committed files), and a full-resolution view shows only the QA identity, `/tmp/gitturtle-evidence` paths and the fixture's synthetic author. The #85 crops `evidence/themes/button-focus-ring/{,base-}porcelain-1000x680-clip-{repository-tab,tab-close}.png` stay as that build's record of the defect; no other committed frame shows a focused repository tab. A `design-reviewer` pass accepted the frames: the ring is whole on every captured control, and every difference is the ring's band. It noted that the selected tab's own close button and the last close button were not captured natively (the view test covers both), and that the ring's top row is the window's first row. A read-only `verifier` pass recounted the bands and reran the compare, privacy scan, test, Clippy and the app tests, and passed all four criteria. DESIGN.md's controls paragraph drops the tab strip from the clipping containers.

Not covered: macOS, fractional scale factors, text scales below about 0.875 (where the strip leaves less than 3 px above and below a tab, so the ring's top row would leave the window), release builds and the accessibility tree. With the tabs overflowing, a tab scrolled partly out of view is now cut 3 px further out, into the 4 px gap beside the menu and + buttons; no frame shows an overflowing strip.

## September 29 desktop code font at a cold launch

Measurement only, in the [benchmark record](benchmarks/2026-09-29-code-font-cold-launch.md) with its driver, raw samples and cache counts.

Setup:
- Build: `d553cc0`, release, clean, sha256 `1f1fed5a…`.
- Host and output: the two-core AMD 3020e on Omarchy (Hyprland 0.56.2, fontconfig 2.18.3), with a temporary 1480 × 800 headless output.
- Launches: two runs of 12 interleaved rounds, each round launching with **Use the desktop's monospace font** on and off, and with a cold fontconfig cache (an empty cache directory per launch) and a warm one. Every launch had fresh HOME and XDG directories, so Mesa's shader cache was cold in all four configurations.
- Timings: launch to Hyprland's `openwindow` event, and launch to the end of the app's `fc-match monospace` call, through a timing wrapper on `PATH`. That end is a lower bound for the family's application; the frame that applies it is not observed.

Does the setting delay the first window? Slightly at p50, more with a cold cache:
- With a warm cache it adds 7 ms at p50 (338 against 331 ms, and 337 against 330 ms), within the launch-to-launch spread.
- With a cold cache it adds 23 to 29 ms at p50 (355 against 332 ms, and 365 against 336 ms), in 22 of 24 rounds.
- The two slowest windows, 1,104 and 790 ms, were the first launch of each run, cold with the setting on. Every run started with that configuration, so a first-launch effect cannot be separated from it.
- Run 2's tails overlap a fetch and merge in another worktree; run 1 is the quiet reference.

With a warm cache the family is known 58 to 156 ms after spawn, before the window. With a cold cache, the first lookup outlived the app's 500 ms bound in all 24 launches, and the app does not retry a failed lookup:
- In 22 launches, the lookup requested by the window's activation, which ran as soon as the killed call returned, found the family 771 to 897 ms after spawn.
- In 2 launches, the first of each run, the window mapped after the kill and its lookup was killed too. The code stayed in the bundled family for the about 60 s observed after the window, with no further lookup.

That cold-cache miss is reported for its own task, not changed here. Not covered: the frame that applies the family, a warm GPU shader cache, a cold kernel page cache, a launch without window activation, X11 or XWayland, GNOME, other hosts and macOS.

## September 29 bundled code font without features

Code text turned `calt` and `liga` off for every code family. Any explicit feature makes cosmic-text shape the embedded DejaVu Sans Mono more slowly, although the font has no ligatures to turn off. `appearance::code_font_features_for` now returns no features for `desktop_text::BUNDLED_CODE_FAMILY` and keeps both off for every other family, in `CodeFont` and in the editor wrapper. The wrapper applies them in one `native` step at render time, from the family the kit editor draws in, unless the caller chose features.

Tests:
- `appearance::tests::code_text_in_the_bundled_family_shapes_without_features` and `appearance::tests::code_text_shapes_the_code_family_without_ligatures` pin both feature lists.
- `editor_find::tests::editors_draw_code_without_ligatures` checks the style handed to the kit editor for a desktop family, the bundled family, a caller's family and a caller's features. With the wrapper's `native` step returning the kit editor unchanged, so that no code features are applied, the test failed at `editor_find.rs:883`, where the desktop family expects features.
- `desktop_text::tests::bundled_code_font_draws_the_same_glyphs_without_features` shapes a line of operators, `fi`, `ffl` and Arabic lam-alef in all four bundled faces, with and without the features, and gets the same glyphs.

Measurement, in the [benchmark record](benchmarks/2026-09-29-code-font-features.md) with its driver and raw samples:
- Setup: release builds pinned to one core of the AMD 3020e, warm cache, the first 5,000 lines of `crates/app/src`, three runs of 30 interleaved rounds.
- p50 per line: 29.03 to 29.62 µs with the features against 27.14 to 27.71 µs without, ratios of 0.926 to 0.936.
- Paired by round, the median ratio is 0.934, and the candidate was faster in 83 of the 90 rounds. The largest samples of both lists fall in the same rounds, so the tails come from the host.
- `performance-reviewer` accepted the method and an earlier set of three runs (ratios 0.924 to 0.931), which these runs agree with.

JetBrainsMono Nerd Font under the desktop features still draws `->`, `!=`, `=>`, `==`, `<=`, `--` and `//` as separate glyphs. There is no visible change, since the bundled font's glyphs are identical, so there are no native frames. Not covered:
- a cold cache, another host, or a code size other than 12 px;
- macOS, whose code family is Menlo and keeps both features off;
- a frame-level timing, which cannot resolve about 0.25 ms per 120 newly shown lines.

## September 29 Omarchy caption on two lines

The Omarchy card's caption ("Follows ‹Theme›", "Keeping ‹Theme›" or "Using Midnight") was cut to one line, so a long custom theme name lost most of itself. Following the owner's "grow on demand" decision, a caption that needs a second line takes it, wrapped at word boundaries, and ends in an ellipsis only when two lines cannot hold it. The miniature above it then draws only whole rows, three instead of four at the default size. A one-line caption paints as before, and the card stays 132 px tall and in place. `docs/development/themes/spec.md` states the rule.

`settings::picker_tests::a_long_omarchy_caption_takes_a_second_line` drives a 64-byte `theme.name` at 1,400 and 1,000 px and at 11 and 18 pt, and checks:
- the drawn lines and the ellipsis;
- that the card's bounds and the group below it do not move;
- that the rows go from four to three, all whole, and back after the name shortens;
- that a one-line card lays out like Nord's.
It does not build on `main`, which lacks its test hooks, and `main`'s one-line cut would fail its two-line check.

Native evidence, full tier, on Omarchy 4.0.4 (Hyprland 0.56.2, native Wayland):
- Output: a temporary headless output at 1400 × 800 and at the 1000 × 680 minimum, scale 1, fullscreen, with a fresh HOME per launch and the `GitTurtle QA` identity.
- Fixture: the disposable `demo` repository at `/tmp/gitturtle-evidence/omarchy-d/demo`.
- Builds: `python3 scripts/native_qa/qa.py identity` reports both as clean release builds. The base is `d553cc0` (sha256 `1f1fed5a…`), and the candidate is `483b92e` (sha256 `126cb7c0…`), the branch with `main` at `0a7d995` merged in.
- Theme state: each launch had a fake `~/.local/state/omarchy/current/` with the preference set to Omarchy, and opened Settings with Ctrl+,.
- States: Tokyo Night's stock `theme.name`; the test's 64-byte `aurora-borealis-over-the-northern-fjords-at-midnight-in-deep-win`; and "Unavailable" with "Using Midnight", from a staged Tokyo Night whose `colors.toml` has no background.
- Frames are in `docs/evidence/omarchy-caption/`, compared with `python3 scripts/native_qa/qa.py compare BASE CANDIDATE`:
  - The stock and Unavailable frames are identical to the base at both sizes (0 px).
  - With the long name, the difference is inside the card only: 4,603 px in `[141, 349, 313, 412]` at 1400 × 800, and 6,999 px in `[40, 349, 335, 412]` at 1000 × 680.
  - At 1400 × 800 the base shows "Follows Aurora Borealis Over…", and the candidate "Follows Aurora Borealis Over / The Northern Fjords At Midnig…".
  - At 1000 × 680, where the cards are wider, the base cuts it at "…At Mi…", and the candidate shows the whole name on two lines.
  - The card's outline and the Light palettes group stay where they were. The swatch row sits 1 px lower: a one-line caption has 1.4 px of spare room under its 54 px minimum, and the second line takes it.
- Live changes: one launch per build with Settings open, rewriting `theme.name` as `omarchy-theme-set` does. The sequence was the stock name, the long name, the stock name again, the long name again, and then the long-named theme with its `colors.toml` background removed.
  - The frames with the stock name are identical to the base and to each other in each build, so the miniature takes back its fourth row.
  - The long-name frames differ from the base only inside the card.
  - The last frame shows "Unavailable" with "Keeping Aurora Borealis Over / The Northern Fjords At Midnig…" over three whole rows, where the base cuts it after one line.

`python3 scripts/native_qa/qa.py privacy scan --redacted --jobs 2` on the committed bytes found 9 clean frames, and each was viewed at full size. A `design-reviewer` pass approved, and measured the committed frames:
- The card's outline, the Light palettes group and everything above the miniature's fourth row are pixel-identical to the base.
- The wrapped caption keeps the one-line card's spacing: 10 px from the border to the name, 17 px from the name's baseline to the description's, and 7 px from the last baseline to the swatches. Its lines are set 13 px apart on the name's left edge, and its ellipsis ends inside the padding.
- The miniature's third row ends 3 px above a one-pixel caption border, with nothing of a fourth row left.
- The muted caption measures 7.9:1 on the caption surface (5.9:1 hovered, 5.4:1 pressed).

Not covered:
- macOS, where there is no Omarchy card.
- Natively: an unselected, hovered or focused card with a long caption, fractional scales and an 18 pt interface text size. The test covers the layout of each.
- A name that the unkerned width check cuts although its shaped text would just fit on two lines. GPUI decides a line limit from per-character widths, as the Desktop reason line already does.

## September 29 code font in Markdown, review and rich previews

The rendered Markdown code block and the pull request review's patch lines named the family "Menlo", which Linux does not have, so their code fell back to the proportional interface font; rich previews drew decoded source in the fixed bundled family. None of the three followed **Use the desktop's monospace font** or turned ligatures off. Each now builds its code container through a small helper under `.code_font(cx)`: `markdown_view::code_block`, `review::patch_line` and `rich_preview::decoded_source`. `grep '"Menlo"' crates/app/src` finds only `mono()`. GPUI's test platform shapes with a placeholder text system, so `markdown_view::tests::markdown_code_blocks_use_the_code_font`, `github_view::review::tests::patch_lines_use_the_code_font` and `rich_preview::tests::decoded_source_excerpts_use_the_code_font` paint a probe under each helper and read the family and the disabled `calt` and `liga` it inherits. Putting back each site's old font call failed its test (`left: "Menlo"`, `"Menlo"`, `"DejaVu Sans Mono"` against the sentinel family).

Native evidence, full tier, on Omarchy 4.0.4 (Hyprland 0.56.2, native Wayland) on a temporary 1480 × 800 headless output at scale 1, fullscreen, Midnight, with a fresh HOME per launch and the `GitTurtle QA` identity. The base was `83f65cc` (release, clean, sha256 `2e8d572b…`) and the candidate `acdb86e` (release, clean, sha256 `a87b5b49…`), its child. `main` at `d553cc0` was then merged into the branch without conflicts, and the merged commits touch none of the three sites. The fixture is a disposable repository with a README whose Rust fence holds `->`, `!=`, `-->`, `==>` and `<=>`, and a `flow.mmd`; the desktop monospace font is JetBrainsMono Nerd Font, which has programming ligatures. Frames are in `docs/evidence/code-font-sites/`, taken with the setting off and on, and compared with `python3 scripts/native_qa/qa.py compare --mask status-timing <base> <candidate>`:
- **Markdown, Rendered tab:** the base draws the code block in the proportional fallback with the setting off or on (the two frames are identical outside the mask), and that font joins `->`, `==>` and `<=>` into arrows. The candidate draws it in DejaVu Sans Mono with the setting off and in JetBrains Mono with it on, every character as typed. Only the code block's region differs.
- **Rich preview, Diagrams tab of `flow.mmd`:** with the setting off the frames are identical, since both draw the bundled family. With it on, only the decoded-source excerpt differs, now in JetBrains Mono without ligatures, plus a 3 × 7 px strip of the status bar's timing text just outside the mask.
- The review excerpt has no native frame because no live GitHub account was used; its view test covers it.

`python3 scripts/native_qa/qa.py privacy scan --redacted --jobs 2` on the committed bytes found 5 clean frames, and each was viewed at full size. A `design-reviewer` pass approved: pitch stays 19 px in all three fonts, the code panel keeps its 85 px and x 602 origin, code ink starts flush with the language label, nothing clips or scrolls, and contrast is unchanged (14.32:1 on the Markdown code panel). It noted two older issues outside this task: the review's line-number columns stay 36 px wide at larger code sizes (`github_view/review.rs:1008-1009`), and blame's source text still uses the fixed `mono()` family (`blame.rs:598`). The longer status message pushes its timing past the named mask; a later run should mask `96,782,240,793`. Not covered: macOS, where Menlo is the code family and the Markdown and review sites already drew it; other palettes and sizes, which share the code font path; the review excerpt natively.

## September 29 frames retaken under the solid focus ring

Task `button-focus-ring-recapture` retakes the 64 committed frames that showed a focused Button drawn before the [solid focus ring](#september-29-solid-focus-ring-outside-every-button): the 49 that the [September 27 helper focus ring entry](#september-27-helper-focus-ring-at-full-accent) lists (30 it did not recapture and 19 it replaced) and 15 more that the sweep below found. No product code changed.

Native runs took place on 2026-09-29 (accepted pairs 17:10 to 19:25 UTC, draw-cost runs 19:40 to 19:48 UTC) on a different host from the earlier theme evidence: Ubuntu 26.04, GNOME Shell 50 on Wayland, NVIDIA RTX 3090, and the app under XWayland (`DISPLAY=:0`, `WAYLAND_DISPLAY` unset, `GPUI_X11_SCALE_FACTOR=1`) at the size in each file name. Every replaced frame attests the adopted build `650a76e` (origin/main with #85), debug, sha256 `aec59c36…7e63`, with `--build-info` reporting a clean source tree. Its same-session base was `105ef2f` (origin/main before #85), debug, sha256 `aa82fb14…24ba`, also clean.

The fixture is `scripts/create-demo-repo.py`'s repository at HEAD `52f471a1` (`theme-fixture`, under `/tmp/gitturtle-evidence/`), unchanged by every launch. Every launch had a fresh HOME and XDG directories and the `GitTurtle QA <qa@example.invalid>` identity.

The stores came from three sources:
- The 32-theme store was regenerated byte for byte (`9deaa6ee…04d23a`).
- Harbor Dusk comes from the committed export document.
- Every other store was rebuilt from what its committed frame shows, recorded by sha256 in the local bundle.

The drivers are new: the earlier host's drivers and bundle were not available here.

Two host differences shaped the runs:
- **Input.** XWayland here routes XTest through the RemoteDesktop portal (`-enable-ei-portal`). GNOME raised an "Allow Remote Interaction" prompt partway through; it held back the pending keys, and the owner denied it. From then on every key, pointer move, click and wheel step went through Mutter's `org.gnome.Mutter.RemoteDesktop`, and a key was sent only while the app held X focus.
- **File chooser.** The FileChooser is Nautilus 50 through xdg-desktop-portal-gnome 50:
  - Save opened in the launch's HOME with the suggested name.
  - Open took the typed path and two Returns.
  - The symbolic link came back unresolved and was refused.
  - The folder was navigated into, and nothing reached the app.
  - The no-portal frames used a private session bus without a portal, so their text took fontconfig's subpixel default rather than the session's grayscale ([explained on September 30](#september-30-linux-antialiasing-without-a-settings-portal)).
  - Keys went to a dialog only while a widget inside it held focus, and no dialog was captured.

Each frame ran on base and then the adopted build in the same session, with the same steps and run directory. The exported path therefore reads as the committed text does. Each pair was compared with the status timing masked:
- **Focused Buttons.** In every pair the only difference is one region, the focused Button's box plus 3 px on each side (478 to 1,408 px): the ring's footprint.
- **Theme picker cards.** The four picker frames differ by 44 px, in 5x5 patches at the card's corners where its own accent border meets the ring; the outer ring is unchanged. A picker card is a Button (`settings.rs`, `theme_card`).
- **Focused disabled Buttons** now show the solid ring: Export… while the Save dialog is up, and Save while saving. On base both had the 3 px half-accent ring without #66's inner band.

Outside the ring each replaced frame equals its same-session base capture. Apart from the later changes listed below, both are within 2 levels of the frame they replace. The exception is `transfer-1000x680-25-unknown-base`, whose Aurora Light store was rebuilt from the committed frame's pixels: its card title renders up to 5 levels lighter. Two base runs here were identical. A probe against `button-focus-ring/midnight-1000x680-history-rest.png` differed by up to 11 levels at glyph edges, so that set is not a byte-for-byte reference on this host.

Every base capture showed the committed frame's state and focused control. They also show later changes:
- #65's "Use the desktop's monospace font" row in Settings (`transfer-1000x680-11`, the four `transfer-1440x900` frames, the editor `04` and `16` frames, and the other 1440x900 editor frames).
- #81's Readability copy and inset.
- #83's mark on the Accent row.

**Sweep.** Two passes looked for further focused Buttons:
- **By name.** `git ls-files 'docs/evidence/*.png'` (357 files), filtered with `grep -iE 'focus|confirm|sav(e|ing)|cancel|delet|refus|kept|chosen|tab-|question|replaced|dialog|guidance|alert|pressed|keyboard'`, gives 99 outside `helper-focus-ring/` and `button-focus-ring/`.
- **By eye.** Every frame outside those two directories was viewed on contact sheets, and at full size wherever a ring was plausible.

Beyond the 49, the sweep found 15 frames with a focused Button:
- `themes/editor/{dark,light}-{1000x680,1440x900}-04-base-chosen` and `fixes-1440x900-07-kept-focus-on-base` (Base).
- `themes/linux-gaps/editor-{midnight,daylight}-1000x680-{replace-question,colors-replaced}` (Base).
- `themes/linux-gaps/editor-{midnight,daylight}-1000x680-saving` (the disabled Save).
- `themes/picker/comfortable-{1000x680-03,1000x680-03a,1440x900-05,1440x900-05a}-*focus*`.

The daylight `replace-question`'s half-strength Braden ring on its `#304050` canvas was faint enough to miss on the contact sheet; the base-against-adopted pair showed it. Its adopted ring reads 1.88:1 there. That canvas belongs to a custom draft with readability findings on its Canvas and Accent rows, outside the built-in palettes' 3:1 guarantee, so the frame is no evidence for that rule.

These were rechecked and are unchanged, identical to base apart from the status timing:
- `themes/linux-gaps/editor-{midnight,daylight}-1000x680-{edited,reset-to-base,delete-failed}`.
- `themes/picker/comfortable-1440x900-03-hover-custom`, whose heavier border is the card's hover state, not focus.
- The editor's Readability list focused (the `07-warnings-focused` state in the editor flows).
- A focused History "Filter changed paths" field in Porcelain.

Not rechecked, because their focus is on a control the Button ring does not style: the History filter field (`*-history-focus` in four theme directories), the editor's Name and hex fields (among them `editor/*-10-edit-reopened`, `*-06-invalid-focused`, `*-11-edit-return-commits`, `followups-*`, `marks-*`, `overflow-*`, `text18-*`, `linux-gaps/editor-*-text18*` and `linux-gaps/scale2-*-editor`), and the Readability list in `fixes-1000x680-04-readability-focused`, whose `07-warnings-focused` counterpart was rechecked. The first two kinds were spot-checked identical. `worktree-removal/protected-worktree.png` shows a selected worktree row without keyboard focus, so no ring. `helper-focus-ring/` and `button-focus-ring/` are untouched.

**Finding: the ring loses its bottom edge next to the import highlight.** In the four `linux-gaps/imported-*-focus-*-highlight` frames, the adopted ring on the Your themes row's Edit… keeps its top edge but has no accent pixels along its bottom edge:
- On the row above the highlight ("beside"), the highlighted row's fill paints over the 2 px where the ring falls.
- On the highlighted row itself ("on"), the imported row sits flush against the status bar, and the Settings page's viewport cuts the ring at the status bar's top border. The list itself keeps 3 px below its last row: row 32's Delete… in `list32-…-12` is whole.

Base still showed its inner band and 1 px of its half-accent ring there, so these frames show less focus than base did. The design reviewer placed "beside" with `DESIGN.md`'s adjacent-segment case: a later sibling painting over the ring, fixed by paint order rather than container room. They placed "on" as a scroll-into-view margin question. `DESIGN.md` still says a focused row action's ring "is whole in every slot", and neither `DESIGN.md`'s nor the theme spec's open list names these cases. This task changes no contract text, so both are left for the owner. The owner kept the promise, and the product was fixed: see [whole focus ring on Your themes rows](#september-29-whole-focus-ring-on-your-themes-rows).

**Theme draw-cost procedure.** Step 2 of the [draw-cost procedure](benchmarks/2026-09-22-theme-draw-cost.md#procedure) found row 1's Edit… by the old ring's colour. It is re-pointed to the solid ring and re-implemented as `draw_cost_focus.py` (sha256 `2ef19e3b…af73`), because the original driver was not on this host. The procedure's lineage paragraph records the adaptation; no recorded measurement changed.

- **Adopted build.** On release `650a76e` (sha256 `394c5d17…cbac`) at scale 2, it lands on row 1's Edit…: 480 ring pixels, box [1378, 1313, 1497, 1372]. That takes 25 wheel steps and 60 Tabs, because the picker now has a card per custom theme.
- **Controls.** The old colour finds 1144 pixels on base and 16 on the adopted build.

A privacy scan of the 64 committed files with the local template set (28 crops made on this host) came back clean in 57 s. Every frame was also viewed at full size: each shows only the QA identity, theme names, `theme-fixture` and `/tmp/gitturtle-evidence` paths.

Frames replaced, all attesting `650a76e`:
- **The 30 not recaptured on September 27:**
  - `themes/import-export/transfer-1000x680-{06,07,08,11,13,14,15,16,17,18,19,20,21,22,23,24,25}-*.png` and `transfer-1440x900-{07,13,21,25}-*.png`
  - `collision2-1000x680-02-collision-2.png`, `lotus-1000x680-01-refuse-notjson.png` and `noportal-1000x680-{01-export,03-import}-guidance.png`
  - `themes/linux-gaps/imported-{midnight,porcelain}-1000x680-focus-{beside,on}-highlight.png`
  - `button-states/kanagawa_lotus-1000x680-imported-button-hover.png`
- **The 19 replaced on September 27:**
  - `themes/editor/{dark,light}-{1000x680,1440x900}-{08-saved,13-cancelled,16-deleted}.png` and `themes/editor/light-1000x680-{12-cancel-focused,15-delete-confirm}.png`
  - `themes/linux-gaps/editor-{midnight,daylight}-1000x680-{delete-confirm,save-refused}.png`
  - `themes/import-export/list32-1000x680-12-tab-reveals-row32-delete.png`
- **The 15 from the sweep,** listed above.

## September 29 solid focus ring outside every Button

Task `app-button-focus-ring`, approved by the owner on 2026-09-28. Where the palette application sets `ring`, it now installs `BUTTON_FOCUS_RING` through the toolkit setting from the [toolkit focus ring setting](#september-29-toolkit-button-focus-ring-setting): 2 px of full `accent`, 1 px outside the edge of every focused Button. That covers primary and danger helpers, the Buttons built without the helper (Tags, Reflog, find, recovery, project pane and project rows) with no edit at those sites, the focused disabled helper and the row exception. The #66 inset band (`control_focus_edge`, its width constant and the helper's focus shadow) is gone, and so is the 2.5:1 row exception. The selected helper's hover no longer dims the whole Button, ring included, to 0.9 opacity; it paints `Palette::row_hover(true)`, the selected fill blended 7 % toward accent, which a test holds to the pressed step and 4.5:1 labels. The Omarchy theme needs no case of its own: a fitted Omarchy palette is used only when it has no readability issue, so its accent clears 3:1 on all six surfaces, and so does the ring drawn in it.

The failing check is `appearance::tests::focused_button_ring_clears_the_graphic_rule_on_every_surface_and_state`. It composites the ring the installed setting describes over the six readability surfaces of every built-in palette, in every state the helper paints and with any opacity a state applies to the whole Button. It requires 3:1, a ring at least 2 px wide and at least 1 px of gap. With the toolkit's default setting it fails, naming Porcelain on the subtle surface (2.23:1) and Sandstone on the selected row (1.98:1); that output is in the pull request.

Native evidence ran in one session on 2026-09-29 (13:39 to 14:55 UTC) on Linux under XWayland (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`), at 1000x680, 1000x2100 for the Your themes list and 1000x900 for the rebase plan. It compared the candidate `043048f` (sha256 `624f4259…47ea`: origin/main `c807955` with #77, the [disabled-hover fix](#september-29-disabled-buttons-keep-their-look-under-the-pointer) and this change) against the base `c807955` (sha256 `8a810cfd…00d2`), both debug builds from clean trees. Every launch had a fresh HOME and XDG directories and the QA identity, with `theme-fixture` or a disposable copy of it. Focus moved only through the drivers' focus-checked keys and hover through XTest pointer motion. The branch was rebased onto #78, #81 and #83 afterwards: `appearance.rs` is byte-identical to the captured build and `main.rs` differs only by #78's narrow-History announcement. The theme editor frames predate #81's 1 px inset fix at 13 pt.

Strongest ring pixel against the same pixel unfocused, over the named surface, candidate (base):

| Palette | Panel | Subtle | Canvas | Selected row | Selected row, hovered | Active segment, hovered |
| --- | --- | --- | --- | --- | --- | --- |
| Porcelain | 7.01 (7.01) | 6.21 (6.21) | 6.55 (6.55) | 5.57 (5.57) | 5.03 (4.21) | 6.21 (4.57) |
| Sandstone | 5.87 (5.87) | 5.02 (5.02) | 5.44 (5.44) | 4.56 (4.56) | 4.17 (3.38) | 5.02 (3.86) |
| Midnight | 10.45 (10.45) | 10.93 (10.93) | 11.43 (11.43) | 7.48 (7.48) | 6.56 (5.42) | 10.93 (6.63) |
| Solarized Light | 4.14 (4.14) | 4.42 (4.42) | 4.71 (4.71) | 3.55 (3.55) | 3.26 (3.00) | 4.42 (3.14) |

The surfaces are Projects on the panel, the Changes segment on the subtle track, Latest on the canvas, and Stage on a selected Changes row. On resting helpers the base's strongest pixel was #66's band, so the numbers match; the hovered columns show the removed 0.9 dim. The named controls rose further: editor Save (primary), Delete theme (danger) and a Reflog row (a Button built directly) read 6.55:1 in Porcelain and 11.43:1 in Midnight, against 2.27 and 3.74 on base. A disabled Switch focused with an empty branch field reads 7.01 and 10.45, against 2.32 and 3.61. The candidate minimum is 3.26:1, Solarized Light's hovered selected row ([`solarized_light-1000x680-changes-stage-selected-focus-hover.png`](evidence/themes/button-focus-ring/solarized_light-1000x680-changes-stage-selected-focus-hover.png)). Across the edge of every filled Button, Porcelain and Midnight read ring, ring, surface, fill: a 2 px ring of solid accent and a 1 px gap. The base reads 3 px of half accent directly outside, plus the band on helpers.

No-shift: 37 of 50 unfocused resting and hovered frames are identical to base, masked for the status timing digits. The rest differ where expected: the selected helper's new hover fill on the History segment and Local, in all four palettes; the disabled Review rebase… under the pointer, which the vendor fix keeps at rest; and five frames with 1 to 2 px one channel apart at antialiased text, which reruns attribute to run-to-run noise. Hovered unselected helpers are identical to base.

Clipping: the ring is whole on the History segments, Workspaces, dialog footers, the Delete alert, the banner's Dismiss, Stage and the Your themes rows. Its 3 px footprint equals the toolkit ring's, but three containers cut it. The repository tab strip (`repository_tabs.rs`, an `overflow_x_scroll` list that GPUI masks on both axes) leaves a focused repository tab only its right arc and one left column, and cuts its close button's top and bottom. The Tags dialog body cuts the top of Create tag…. On base those three still showed #66's band inside the edge, so they now show less focus than base ([`porcelain-1000x680-clip-repository-tab.png`](evidence/themes/button-focus-ring/porcelain-1000x680-clip-repository-tab.png) against [`base-porcelain-1000x680-clip-repository-tab.png`](evidence/themes/button-focus-ring/base-porcelain-1000x680-clip-repository-tab.png)). The Tags and Reflog lists cut the sides of their full-width rows on both builds; a focused Tags row keeps only corner stubs. An Input's clear button takes no focus. A pre-existing trap on both builds: once a focused Switch turns disabled, neither Tab nor Shift+Tab moves focus off it until a pointer click ([fixed later](#september-30-tab-leaves-a-focused-button-that-turns-disabled)).

Frames: [`evidence/themes/button-focus-ring/`](evidence/themes/button-focus-ring/), 68 files. They are the candidate at every captured state; the unfocused references; crops of each clipping container; and the base's three regressed crops and Solarized Light pair. A privacy scan of every proposed frame with the local template set came back clean. A `design-reviewer` pass accepted the ring, the selected hover and the disabled-hover pair. It asked for the clipped containers to be listed as open in `DESIGN.md` and the spec, which they are, and for a follow-up that gives the tab strip, the Tags body and the Tags and Reflog lists room for the footprint, starting with the repository tab. It also noted that the density and project-mode segments (`settings.rs`, `projects.rs`) still dim to 0.9 when selected and hovered, so two kinds of selected segment now answer the pointer differently ([fixed later](#september-30-selected-segments-like-the-shared-helper)). [`helper-focus-ring/`](evidence/themes/helper-focus-ring/) stays as the superseded #66 record.

Still open: macOS rendering, fractional scale factors, the clipping containers above (the tab strip [fixed later](#september-29-focus-ring-inside-the-repository-tab-strip), and the Tags and Reflog ones [too](#september-29-whole-focus-ring-in-tags-and-reflog)), the two 0.9 dims ([fixed later](#september-30-selected-segments-like-the-shared-helper)), the keyboard trap ([fixed later](#september-30-tab-leaves-a-focused-button-that-turns-disabled)), and the Your themes rows beside the import highlight, where the next row's fill paints over the ring's bottom edge (a row flush against the status bar loses it to the page viewport; [fixed later](#september-29-whole-focus-ring-on-your-themes-rows)). The 49 stale frames, and 15 more a sweep found, were [retaken under the solid ring](#september-29-frames-retaken-under-the-solid-focus-ring), which is where that clipping showed.

## September 29 narrow History Ctrl+B announcement

Task follow-up from [September 27 navigation choice in narrow History windows](#september-27-navigation-choice-in-narrow-history-windows): Ctrl+B in a narrow History window gave no feedback, neither visible nor announced. `b77951e` posts a polite status announcement, "Branches and worktrees unavailable: widen the window", as a new node per press so a repeat is heard again; the count resets once narrow History is no longer drawn. `8704e06` sets that node's text as both name and value, because AccessKit's macOS adapter announces a newly added live node's value rather than its name; AT-SPI still reads the name. Neither commit draws a pixel: the rail button stays the only visible cue, disabled with its "Widen the window to show branches and worktrees" tooltip.

Evidence ran on Linux under XWayland on the GNOME 46 Wayland desktop (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`), one session, 2026-09-29 13:01–13:05 UTC. The candidate was `d12997b` (debug, clean, sha256 `15a9e97a…`); the rebase onto `main` that followed, `8704e06`, was confirmed to draw and announce identically before this evidence was recorded, so the frames and the announcement result below attest both. The base was `1a9fc3e` (debug, clean, sha256 `f2a54741…`), the `main` commit this branch is built on. Each launch had a fresh HOME with the `GitTurtle QA <qa@example.invalid>` identity and fresh XDG directories, on the fixture from `scripts/create-demo-repo.py` at `/tmp/gitturtle-evidence/fixtures/theme-fixture` (HEAD `52f471a`), byte-identical before and after. History has no window-manager rail below the platform's 1,000 × 680 minimum, so the driver lowered the window's `WM_NORMAL_HINTS` minimum to 600 × 500, as a tiling window manager ignores it, and resized to 760 × 680, putting the repository area under the 800 px [narrow threshold](#september-27-navigation-choice-in-narrow-history-windows).

- **Frames.** [`candidate-narrow-history-760x680.png`](evidence/narrow-history/candidate-narrow-history-760x680.png) shows narrow History with focus on the commit table; it is byte-identical to the frame after Ctrl+B, because Ctrl+B changes nothing visible and the feedback is the announcement. `qa.py compare --mask status-timing` found it and two repeat captures identical against the base and against each other, apart from the file-list timing text the mask covers.
- **Tooltip.** [`candidate-narrow-rail-tooltip-760x680.png`](evidence/narrow-history/candidate-narrow-rail-tooltip-760x680.png) shows the pointer resting on the rail's navigation button with the tooltip "Widen the window to show branches and worktrees" visible, closing the second open follow-up from September 27.
- **Announcement (AT-SPI).** Accessibility was enabled for one launch only (`IsEnabled` false, false before; true during; false, false after, restored), with `dbus-monitor` on the AT-SPI bus watching both the `Announcement` match and every `org.a11y.atspi.Event.Object` signal as a liveness check (344 object events seen on the candidate launch, 340 on base). One `org.a11y.atspi.Event.Object.Announcement` fired per Ctrl+B on the candidate in the narrow window (2 total), each with args `"" 1 0 "Branches and worktrees unavailable: widen the window"`; none fired after resizing wide and pressing Ctrl+B there, and base fired none narrow or wide.
- **Privacy.** `python3 scripts/native_qa/qa.py privacy scan --redacted --templates .local/privacy/templates docs/evidence/narrow-history/*.png` found 0 template matches on the two committed frames; they were viewed and show only the fixture's demo commits, the `/tmp` fixture path and the QA identity.

Not covered: Orca speech and VoiceOver. The AT-SPI monitor records that the signal fired with its text, not what a screen reader speaks aloud; VoiceOver was not checked on any host. Both open follow-ups from September 27 are resolved by this record.

## September 29 toolkit Button focus ring setting

Task `vendor-button-focus-ring` adds `Theme::button_focus_ring` (`vendor/gpui-component/src/styled.rs`, `theme/mod.rs`, `button/button.rs`): a per-Button ring width/gap/opacity, set in code only, defaulting to the existing ring — 3 px of `ring` at 0.5 opacity directly outside the edge — so a Button paints exactly as before until an application sets it. `focus_ring_style`'s other eight callers are unchanged. The app does not consume the setting yet (`app-button-focus-ring` will), so this entry attests that the vendor change alone changes nothing drawn.

Native evidence ran on Linux X11 under XWayland (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`, 1000x680), debug builds of base `1a9fc3e` and candidate `97dd89c`, on the unchanged `/tmp/gitturtle-evidence/fixtures/theme-fixture` (HEAD `52f471a`). Two later commits on this branch, `008c0e8` and `167ff01`, touch only an out-of-scope routing row, tests and a doc comment, so these frames still attest the patch as landed. 22 states per build across Midnight and Porcelain — the Tags dialog unfocused and focused on its helper, its directly built row and its primary button, Settings unfocused, the delete alert focused on its helper and its danger button, and the editor unfocused, focused on its primary and secondary buttons and with the primary disabled — were captured twice; `qa.py compare --mask status-timing` found candidate and same-session base identical in both repetitions (22/22, 0 px outside the mask), and a same-session base rerun matched the prior day's base 22/22. No frames are committed for this task: every candidate frame is byte-identical to base, so this entry is the recorded evidence. Not covered: macOS.

## September 29 disabled Buttons keep their look under the pointer

A follow-up that the `app-button-focus-ring` review found. A caller's `hover` style on a toolkit Button is GPUI's, which refines the element's base style, where the disabled style lives. The toolkit gates only its own hover on the Button being enabled, so a disabled Button with a caller hover lit up under the pointer as though it were available (`DESIGN.md`: no hover-only promise of an action). The shared helper's selected hover is one such style. The vendored `Button::hover` now applies a caller's style only while the Button is enabled ([patch notes](../vendor/gpui-component/GITTURTLE-PATCH.md)). Four app call sites resolve to it: the helper's selected hover and the selected mode, density and worktree Buttons. `caller_hover_styles_only_enabled_buttons` hovers and presses an enabled and a disabled selected Button, each with a caller hover fill, and reads their fills from the rendered scene. The upstream provenance is unchanged from the [toolkit focus ring setting](#september-29-toolkit-button-focus-ring-setting)'s attestation.

Light-tier native evidence ran in one session on 2026-09-29 on Linux under XWayland (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`), in Porcelain at 1000x900. It compared the candidate `becdbd3` (sha256 `fd048fd7…78e2`, origin/main with #77 and this fix) against the base `c807955` (sha256 `8a810cfd…00d2`), both debug builds from clean trees. The subject was **Review rebase…** in the interactive rebase plan (`interactive_rebase.rs:586`), a selected helper that stays disabled until the published-commit warning is acknowledged, on a disposable copy of `theme-fixture` with `feature/activity-feed` checked out. Under the pointer the base dims it to 0.9 opacity: 19,176 px differ, all within the Button, with the fill going from `#eff1f8` to `#f0f2f8`. The candidate's hovered frame is identical to its frame at rest, and so is the frame of the `app-button-focus-ring` build stacked on it. At rest, base and candidate differ only in the status timing digits. Frame: [`porcelain-1000x900-disabled-selected-hover.png`](evidence/button-states/porcelain-1000x900-disabled-selected-hover.png); its privacy scan came back clean. A `design-reviewer` pass accepted the pair as meeting `DESIGN.md`'s rule against a hover-only promise that a disabled control is available; the frames cannot show the cursor shape. Enabled Buttons are unchanged, and so are group hover styles; macOS is not covered.

## September 29 readability marks on blended surfaces

A follow-up the [theme editor readability follow-ups](#september-29-theme-editor-readability-follow-ups) review found. A warning on the hovered selected row marked only the Selected row, although that surface is Selected blended 7 % toward Accent (`Palette::row_hover(true)`). A graph-lane warning did not mark Text, although `Palette::is_light` picks the lane set by comparing Canvas with Text. `ReadabilityIssue::tokens` in `appearance/custom.rs` now derives the marked rows from each measure's own foreground and background definitions, shared with the pressed-step measure's surface list, and the editor's hand-kept list is gone. Tests: `an_issue_marks_its_named_and_blended_tokens`, and `every_issue_marks_the_tokens_its_measure_reads`, which moves each token one channel at a time over every failing issue and requires the tokens that change an issue to equal the tokens it marks.

Light-tier native evidence ran in one session on 2026-09-29 (13:33 to 13:36 UTC) on Linux under XWayland (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`), at 1000x680 and 13 pt. It compares the candidate `bfd304e` (sha256 `45052227…1a96`) with its parent `8f90b72` (sha256 `bcfb4b6f…2aa8`), both debug builds from clean trees, with a fresh HOME, the QA identity and the unchanged `theme-fixture` for every launch. The custom theme "Row hover warning" is Midnight with Muted text `#a1a5ad`: 4.84:1 on Selected and at least 4.5:1 on every other surface, but 4.16:1 on the hovered selected row (`#274643`). So its single warning, "Muted text on Selected row hover 4.1:1, needs 4.5:1", exists only because of the accent share. The candidate marks Selected, Muted text and Accent; the base marks Selected and Muted text. The frames differ only in the Accent row's glyph (172 px), and the Readability list is byte-identical. Each build's two captures were identical, and a privacy scan of all eight frames came back clean. Frames: [`marks-1000x680-13pt-row-hover-warning.png`](evidence/themes/editor/marks-1000x680-13pt-row-hover-warning.png) and [`marks-1000x680-13pt-row-hover-warning-entry.png`](evidence/themes/editor/marks-1000x680-13pt-row-hover-warning-entry.png).

Only the unit tests cover the graph-lane clause (Canvas and Text); on screen it is the same glyph in the same slot. A `design-reviewer` pass accepted the Accent mark, since the warning names the surface that blends it, and found `DESIGN.md` consistent. It raised two points for the owner, neither changed here: a lane warning's Text mark points to a fix that would invert the theme, and the glyph's accessible name, "‹label› has a readability warning", now also covers rows that only take part in a warning.

## September 29 theme editor readability follow-ups

Task `theme-editor-readability-followups` closes the three editor items the [September 27 pressed-buttons entry](#september-27-pressed-buttons-distinct-from-hover) left open. The empty Readability list now reads "Every readability rule is met.", because the rules include the pressed step as well as the contrast minimums. A pressed-step warning now marks every token row its measure composites over (Canvas, Panel, Subtle surface, Hover, Selected and Accent), not only Selected and Hover. And the editor panel ends 16 px above the window at every interface text size: `ThemeForm::max_height` had counted the title gap as fixed pixels, scaled the fixed bottom margin with the text size and left out the panel's borders, so at 18 pt it ended 19 px above. `DESIGN.md` names the new copy, the marked rows and every term of the height cap.

Tests: `the_empty_list_names_every_rule_and_a_step_warning_marks_its_rows` (view test, copy and glyph rows), `a_step_warning_marks_every_token_its_measure_composites_over` (fails on the old `issue_tokens`), `a_step_warning_marks_the_tokens_its_measure_reads` (moves each token one channel at a time and requires the tokens that change the warning to equal the marked rows), `the_cap_leaves_16_px_under_the_panel_at_scale_1_and_2` (11, 13, 14, 15 and 18 pt in three window heights, exactly 16 px) and `rows_keep_their_geometry_and_the_dialog_fills_the_window` (16 px with no tolerance). An earlier candidate, `652ae6b`, measured 17 px at 13 pt natively: at scale 1 GPUI snaps the half-rem title gap (6.5 px) to whole pixels, and its unit test ran only at 2x. The cap now snaps every chrome term with `window.pixel_snap`, the rounding the layout draws with.

Native evidence ran on Linux under XWayland on the GNOME 46 Wayland desktop (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`) in one session on 2026-09-29: the candidate `8f90b72` (sha256 `bcfb4b6f…2aa8`) from 13:18 to 13:25 UTC and the base `c807955`, origin/main (sha256 `8a810cfd…00d2`), from 13:25 to 13:32 UTC, both debug builds from clean trees. Each of the 16 launches had a fresh HOME and XDG directories and the QA identity, and left the `theme-fixture` repository unchanged. Two custom themes drive it in Midnight: a copy with no warnings and a theme whose Selected equals its Hover (one step warning), at 13 pt and 18 pt, 1000x680, and at 18 pt, 1440x900. Every frame was captured twice per build and each pair was identical. The base's nine frames are also identical to the September 28 base `1a9fc3e`'s.

| Inset under the panel | 13 pt, 1000x680 | 18 pt, 1000x680 | 18 pt, 1440x900 |
| --- | --- | --- | --- |
| Base `c807955` | 16 px | 19 px | 19 px |
| Candidate `8f90b72` | 16 px | 16 px | 16 px |

The inset is the number of rows between the panel's 1 px bottom border and the window's last row, measured at three columns in both repetitions. Against the base, the candidate differs only where the change reaches: the copy line; the four new glyphs at 13 pt (Canvas, Panel, Subtle surface and Accent; 688 px in four 15 px boxes) and the Accent glyph after scrolling (172 px); and at 18 pt the footer band, the scrollbar thumb and the body, which is 3 px taller and so shifts its rows down 3 px. Shifted back, only the copy line differs. The frames that show no copy or glyph change are identical.

Frames: [`followups-1000x680-13pt-readability-empty.png`](evidence/themes/editor/followups-1000x680-13pt-readability-empty.png), [`followups-1000x680-18pt-readability-empty.png`](evidence/themes/editor/followups-1000x680-18pt-readability-empty.png), [`followups-1440x900-18pt-readability-empty.png`](evidence/themes/editor/followups-1440x900-18pt-readability-empty.png) and [`followups-1000x680-13pt-step-warning-glyphs.png`](evidence/themes/editor/followups-1000x680-13pt-step-warning-glyphs.png). A privacy scan of all 36 frames with the local template set came back clean.

A `design-reviewer` pass over every proposed frame at full size accepted each difference: the new glyphs are pixel-identical to the existing Selected glyph and centred on their rows, the copy sits on the old baseline, and the footer band, scrollbar thumb and 3 px body shift all follow from the panel ending 3 px lower. It found `DESIGN.md` consistent after one wording fix (the row is "Subtle surface"). It noted three pre-existing details for later, none caused by this change: at 18 pt the panel's lower edge now crosses the status-bar hints through their lowercase letters (on base it crossed their tops), since a 16 px inset overlaps any taller status bar and `DESIGN.md` does not say how the status bar looks under a modal; a scrolled row label is clipped under the Name field as any scrolled row is; and the Readability heading starts 4 px right of the row labels.

Still open: under untiled Linux client decorations `max_height` ignores the kit's window paddings, so the panel would overrun by about 20 px; a long save error that wraps makes the footer taller than the cap assumes. Neither occurs on X11 or macOS. macOS and fractional scale factors are not covered.

## September 28 README screenshots on Omarchy

The README's screenshots were retaken on the owner's Omarchy laptop, replacing the September 14 frames captured in Ubuntu virtual X11 from build `7d18fef`. Those frames stay under `docs/screenshots/public-launch/` as the [public-launch record](public-launch-validation.md)'s evidence. No product code changed, so there is no base and candidate pair; the rules both [visual evidence tiers](../.agents/skills/gitturtle-native-qa/references/visual-evidence.md) share still applied.

The build was the release executable of `5b7c25c` from the Omarchy theme entry below (clean tree, sha256 `cba30b88…`), accepted again by `qa.py identity`. Its crates, vendored patches, lockfile, toolchain file and assets are identical to `main` at `8bdec0d`, where #74 landed. Capture used the same method: Omarchy 4.0.4, Hyprland 0.56.2 on native Wayland, a temporary headless output at scale 1 sized to the frame, the window fullscreen, and `grim -o` cropped to the window. `qa.py launch` drives X11 only, so it did not take these frames. Each launch had fresh HOME and XDG directories, the `GitTurtle QA <qa@example.invalid>` identity, `TZ=UTC`, and a fake `~/.local/state/omarchy/current/` inside that HOME, copied from `/usr/share/omarchy/themes/tokyo-night`. The owner's own Omarchy state was not read. Let `$EVIDENCE` be `/tmp/gitturtle-evidence`. The fixture was `scripts/create-demo-repo.py` at `$EVIDENCE/readme/Aurora`, with the same commits as the September 14 hero. Two one-commit repositories, `$EVIDENCE/readme/Harbor` and `$EVIDENCE/readme/Lighthouse`, fill the project list, in the groups Studio and Tools. `qa.py display-check` found no GitTurtle executable or QA driver before or after (its X11 window listing is inconclusive without python-xlib), and afterwards `hyprctl` listed only the laptop panel.

| Frame | Size | State | sha256 |
| --- | --- | --- | --- |
| [history.png](screenshots/readme/history.png) | 1480 × 800 | Midnight, project list on, `e96f692` selected | `9a8f72a7…` |
| [settings-themes.png](screenshots/readme/settings-themes.png) | 1480 × 940 | Settings, the Omarchy card selected and following Tokyo Night | `ee109e08…` |
| [keyboard-shortcuts.png](screenshots/readme/keyboard-shortcuts.png) | 1480 × 800 | Midnight, Keyboard Shortcuts opened from the command palette | `aaa14d3b…` |

Privacy: this host had no template set, so a local one was built first and stays out of the repository. It holds 18 crops from native frames of a disposable repository under the home directory, launched with the owner's Git identity. Those frames showed the owner's name, both email addresses, the GitHub account name, the local user name and the home path at the sizes History, the inspector, the profile button and Settings draw them. Before the last two crops were added, `qa.py privacy scan --redacted` flagged two of those frames that no template was cut from (exit 1) and passed the September 14 README frames (exit 0). Each committed frame was viewed at full resolution. `python3 scripts/native_qa/qa.py privacy scan --redacted --jobs 2 --templates <local templates> docs/screenshots/readme/history.png docs/screenshots/readme/settings-themes.png docs/screenshots/readme/keyboard-shortcuts.png` reported all three clean (exit 0) in 41 s.

Not covered: no Compare frame was taken, and none of these frames comes from macOS or X11.

## September 28 Omarchy theme on Linux

On Linux, when `~/.local/state/omarchy/current/theme/colors.toml` exists, Settings offers an **Omarchy** card in its own **Desktop** group. Choosing it gives GitTurtle a palette mapped from the desktop's current Omarchy theme and fitted to the readability rules every theme passes, and GitTurtle follows each `omarchy theme set` while the card is selected. GitTurtle only reads `current/`. The [themes specification](development/themes/spec.md#omarchy-theme) holds the mapping, the owner's decisions and the test list: 22 embedded Omarchy themes and two hand-built ones map with no readability findings, a seeded property test fits 600 of 600 generated themes, and GPUI tests drive the watcher against a temporary `current/` switched the way `omarchy-theme-set` switches it. The fast gate passed on `5b7c25c` in 107 s. `typos` and `cargo-machete` were not installed, nextest is missing so the tests ran under `cargo test`, and the GPUI rerun stage was skipped because 74 GPUI tests exceed its cap of 40.

The evidence is full tier, from one session on the owner's Omarchy 4.0.4 laptop (AMD 3020e, two cores): Hyprland 0.56.2 on native Wayland, a temporary 1400 × 2100 headless output at scale 1, the window fullscreen, `grim -o` whole-output grabs. The candidate was `5b7c25c` (release, clean, sha256 `cba30b88…`) and the base `1a9fc3e` (release, clean, sha256 `0e074362…`). The branch also merges `main` up to `8b9fd3d`, but `1a9fc3e..8b9fd3d` changes no product code: #73 is inside `mod tests`, and the rest is documentation and CI. `qa.py identity` accepted the pair. Every launch had a fresh HOME and XDG directories and the `GitTurtle QA <qa@example.invalid>` identity. Every launch but one also had its own fake `current/`, copied from `/usr/share/omarchy/themes/` and switched with `omarchy-theme-set`'s writes in its order. The fixture from `scripts/create-demo-repo.py` at `/tmp/gitturtle-evidence/omarchy-d/demo` had the same fingerprint before and after every run. Run directories are under `/tmp/gitturtle-evidence/runs/`; the drivers, from local session notes, are not committed. Frames stay local, because this host has no privacy templates.

- **No Omarchy state** (`omarchy-d-none-{base,candidate}-1790638674`): History, Compare and Settings are identical between base and candidate outside the timing mask.
- **All 22 themes switched live** (`omarchy-d-themes-1790638777`): 66 frames (Settings with the card, History with e96f692, Compare on `src/App.tsx`). The trace shows 21 applications for 21 switches and none at launch.
- **Launch equals live switching** (`omarchy-d-startup-*-1790639298`): launching already on Catppuccin Latte, White or Hackerman gave the same 9 frames as switching to them, identical under `qa.py compare`.
- **Renamed status in every theme** (`omarchy-d-renamed-1790639201`): History with dac00d8, whose files are New, Renamed and Modified.
- **Unavailable theme** (`omarchy-d-fallback-{launch,live}-1790639441`). A malformed `colors.toml` at launch shows "Unavailable" and "Using Midnight", with the reason under the Desktop group. A malformed file or a theme without `colors.toml` mid-session keeps Tokyo Night and says why. White then recovers.
- **Choosing the card after an unseen switch** (`omarchy-d-flash-1790639484`): one read, then a single application of White. No stale palette appears first.
- **Captions** (`omarchy-d-captions-1790639555`): "Follows Tokyo Night", and "Follows your desktop" once `theme.name` is removed. "Reading your theme…" shows only until the first read lands, so only `settings::picker_tests` covers it.
- **Hover on secondary buttons** (`omarchy-d-hover-1790639577`): with the pointer on the branch picker and the profile picker, the fill steps 1.101 to 1.115:1 off rest in Tokyo Night, Everforest, Miasma, Kanagawa, Osaka Jade, Ristretto, Nord, Gruvbox, Matte Black, Hackerman, Catppuccin Latte and White. On `f58440c` it stepped 1.007 to 1.077.
- **Real `omarchy theme set`** (`omarchy-d-real-1790640213`, approved in advance by the owner): the desktop's own state switched to White, Vantablack, Hackerman and Catppuccin Latte, then back to Tokyo Night. Afterwards `theme.name` and the `colors.toml` sha256 matched the owner's recorded state. Each real switch applied once. All 20 frames match the fake-state frames of the same theme, apart from 1 px of antialiasing in three Latte frames, outside the top 26 px. There, Omarchy's bar, which `omarchy theme set` restarts, draws over the fullscreen window on the QA output. Each switch runs `hyprctl reload`, which resets the headless output, so the driver re-applies its mode and checks the window's size and position before each capture.

Release timings are in [the benchmark record](benchmarks/2026-09-28-omarchy-theme.md). A reread takes at most 2.5 ms, off the UI thread, and a switch applies in about 0.5 ms up to the next frame callback. Choosing the card reads first by owner decision, so its `theme_apply_frame_ms` is 56 ms at p50 and 73 ms at most, inside the 100 ms wait.

Design review took four passes. The first, on `3846dc1`, found a truncated reason, a stale card with a flash of the old theme, colliding status hues, flat surfaces and monochrome hunk headers; the owner chose the card's states and the read-first behaviour, and delegated the mapping fixes. The second, on `d7fb671`, found subtle surfaces on or under the canvas and hunk colours outside blue. The third, on `f58440c`, found secondary-button hover within 1.08:1 of rest and Modified drawn in blue in Lupine and Lumon, whose `yellow` is blue; the owner then chose hue windows for Modified, Warning and Renamed. The fourth approved `5b7c25c`. Its closest calls are Hackerman's selected row, only 1.095:1 beyond its hover, and Nord's Renamed and Deleted, which differ by label and glyph more than by colour.

Not covered:
- The warning colour appears in no frame; only the hue-window test covers it.
- Focus rings, AT-SPI announcements and the accessible description were not checked natively; `settings::picker_tests` covers the accessible name.
- XWayland, X11, multiple monitors and scales other than 1 were not exercised.
- Other Arch desktops, and a cold page cache, were not exercised either.
- Open follow-ups:
  - the disabled Follow system thumb stays at full strength (fixed: [September 30](#september-30-disabled-switch-thumb-fades-with-its-track));
  - syntax colours on changed lines (fixed: [September 30](#september-30-diff-headers-and-syntax-colours-from-the-palette));
  - fixed colours on the first `@@`, `---` and `+++` lines, which predate this change (fixed: [September 30](#september-30-diff-headers-and-syntax-colours-from-the-palette));
  - Nord and Rosé Pine hunks take GitTurtle's blue;
  - a custom theme name up to 64 bytes is cut short on the card, though its tooltip and accessible name carry it in full;
  - GitTurtle's own light palettes keep hover close to subtle (One Light 1.010:1), and Braden is the light fallback.

## September 27 code text without ligatures

Code text turns off the `calt` and `liga` font features wherever the code family is used, so a ligature font shows the characters as typed. That covers the diff view, every editor built through `editor_find::Editor`, and the Settings code sample. This closes the ligature question the [desktop monospace font](#september-27-desktop-monospace-font-on-linux) review left open. GPUI hands the features to both text backends (cosmic-text on Linux, CoreText on macOS), so the toolkit is unchanged. `appearance::tests::code_text_shapes_the_code_family_without_ligatures` paints a probe under `CodeFont` and reads the font it inherits, which is the font the kit editor and the diff gutter shape with. `editor_find::tests::editors_draw_code_without_ligatures` checks the editor wrapper's style. Each of three mutants failed one of the two tests: features that allow ligatures, `CodeFont` setting only the family, and the wrapper dropping the features. The fast gate passed in 225 s; `typos` and `cargo-machete` were not installed and were skipped, and the tests ran under `cargo test` because nextest is missing.

The evidence is full tier, from one session on the owner's Omarchy desktop: Hyprland 0.56 on native Wayland, a temporary 1400 × 2100 headless output at scale 1, fullscreen, `grim -o` whole-output grabs. `hyprctl monitors -j` matched the reading taken before each run. The base was `cea2012` (release, clean, sha256 `38ba9a6d…`) and the candidate `d63141c` (release, clean, sha256 `d32cd8ca…`); `qa.py identity` accepted the pair. Each launch had a fresh HOME with the `GitTurtle QA <qa@example.invalid>` identity, fresh XDG directories, and the desktop monospace font preference seeded on. The fixture `/tmp/gitturtle-evidence/c2-ligatures/demo` is a two-commit repository whose `src/pairs.rs` holds `--`, `->`, `=>`, `!=`, `==`, `<=`, `>=`, `::`, `//` and `===`; it was byte-identical before and after. The frames are `s1-settings` (the Code text size sample), `c1-compare` (History, Enter, the unified diff) and `f1-file` (Quick Open's file view). The captures came from local drivers rather than `qa.py launch`, because `qa.py` drives XWayland, so the runs have no `flow-log.json`; `qa.py display-check` was inconclusive on this host, because python-xlib is not installed.

- **JetBrainsMono Nerd Font** (each launch's `$XDG_CONFIG_HOME/fontconfig/fonts.conf` set the `monospace` alias, in `omarchy-font-set`'s format). `qa.py compare --mask status-timing` found differences only inside the glyph cells of the pairs. The base drew `->`, `=>`, `!=`, `==`, `===`, `<=`, `>=`, `&&`, `//` and the `---`/`+++` header rules as joined glyphs, and closed the gap in `--`; the candidate draws the characters. The text after each pair keeps its column. Line pitch, gutters, the diff tiles and the word-diff highlight edge are unchanged. In Settings only the sample's `//` changed (79 px). Frames: base `432b0179…`, `35c73886…`, `ea713600…`; candidate `894c716d…`, `fe74b0cf…`, `368f7bce…` (c1, f1, s1).
- **The embedded DejaVu Sans Mono** (the preference off): all three frames `identical` outside the timing mask. DejaVu has no Latin ligatures, so no committed frame moves.

A `design-reviewer` pass approved the pixels and the `DESIGN.md` sentence. Its follow-ups are outside this change. Markdown code blocks (`markdown_view.rs`) and GitHub patch rows (`github_view/review.rs`) name "Menlo" literally, which Linux does not have, so there they likely fall back to a proportional font. Decoded source in rich previews uses the fixed bundled family at the code size.

Shaping cost, in release mode: a scratch binary built against cosmic-text 0.19.0 made GPUI 0.3.4's `layout_line` calls (`ShapeLine::new` and `layout_to_buffer`, advanced shaping, no wrap) on 5,000 non-empty lines of `crates/app/src`. It ran 30 interleaved samples after two warm-up rounds, pinned to one core of this AMD 3020e, with a load average of about 1.3. JetBrainsMono Nerd Font went from 48.6 to 28.9 µs per line at p50 (p95 244.9 to 145.9 ms per 5,000 lines). The embedded DejaVu Sans Mono went from 23.8 to 26.5 µs per line (p95 121.0 to 134.1 ms). Any explicit feature costs DejaVu about the same, even `zero`, which it does not use. GPUI keeps the previous frame's line layouts, so the cost applies to newly shown lines: about 0.3 ms for a full 120-line screen on this host.

Not covered: frames are not committed, because this host has no privacy templates. No frame shows a selection across a former ligature: with no pointer input, keyboard focus did not reach the file view's editor. Split view, the conflict editor and larger code sizes share the code path but were not captured. macOS was not checked; Menlo has no programming ligatures, but its shaping now receives the feature list too.

## September 27 helper focus ring at full accent

Superseded on September 29 by the [solid focus ring outside every Button](#september-29-solid-focus-ring-outside-every-button); the frames under `docs/evidence/themes/helper-focus-ring/` stay as the dated record of this change.

Task `themes-helper-focus-ring` fixes the shared `button` helper's focus ring. The helper has no border, so when focused its only ring was the toolkit's 3 px layer at half of `accent` (`FOCUS_RING_OPACITY`), below the 3:1 focus-ring rule in most palettes. Focused, the helper now also paints a 1 px band of full accent just inside its edge, as an inset shadow that hover and selection do not restyle (`appearance::control_focus_edge`). `appearance::tests::shared_button_focus_ring_clears_the_graphic_rule_in_every_state` composites what a focused helper paints over each readability surface of every built-in palette, at rest, selected, selected and hovered, hovered and pressed, and requires 3:1. The one exception is hovered or pressed on a hovered or selected row, where the band lies on the helper's own fill over the row and the test requires the 2.5:1 that `DESIGN.md` publishes. With the band removed, which is the `origin/main` helper, the test fails 344 times across all 20 palettes, among them "Porcelain at rest on the subtle eff1f8: 2.23:1" and "Sandstone at rest on the selected row f2dcd0: 1.98:1". `shared_button_tests::focusing_the_shared_button_keeps_its_box` renders an icon-only and a labelled helper and requires the same bounds at rest, focused, and focused while hovered.

Native evidence ran on Linux under XWayland on the GNOME Wayland desktop (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`, 1000x680). Base and candidate ran one after the other in the same session. Base was `e5ca32f` (sha256 `f53d4f9a…`) and the candidate `56a3fe5` (sha256 `ff30dd2c…`), both debug builds from clean trees. The branch's later commits change only a test floor and documentation, and merge #61 and #64 from `main`, which change the layout only below 800 px and at fractional scales; these frames attest `56a3fe5`. Every launch had a fresh HOME and XDG directories, the `GitTurtle QA <qa@example.invalid>` identity and a fixture under `/tmp/gitturtle-evidence/` (`theme-fixture`, `ie-d3-demo`), which no launch changed. Focus was reached with Tab only. Strongest ring pixel against the same pixel unfocused, base → candidate:

| Surface | Porcelain | Sandstone | Midnight |
| --- | --- | --- | --- |
| Panel (Projects…, the 28 px Settings) | 2.32 → 7.01 | 2.18 → 5.87 | 3.61 → 10.45 |
| Subtle (Edit…, New theme…, the Changes segment) | 2.23 → 6.21 | 2.06 → 5.02 | 3.67 → 10.93 |
| Canvas (Latest) | 2.27 → 6.55 | 2.13 → 5.44 | 3.74 → 11.43 |
| Selected row (Stage) | 2.15 → 5.57 | 1.98 → 4.56 | 3.11 → 7.48 |

The active History segment, focused and hovered, reads 4.57, 3.86 and 6.63. Of 30 resting and hovered frames, 23 are identical to base and 7 Porcelain frames differ only at antialiased text pixels, one channel by 1, which two base runs also show. All 36 focused frames differ from base only on the helper's outermost device pixel and in its 7 px corners; the 3 px outer ring and everything inside are unchanged. A privacy scan of all 46 committed frames with the local template set came back clean.

Frames changed and why:
- [`evidence/themes/helper-focus-ring/`](evidence/themes/helper-focus-ring/) (27) is new. For Porcelain, Sandstone and Midnight it holds a focused helper on each surface: `history-projects` and `history-settings` (panel), `history-latest` (canvas), `history-tab` and `settings-edit` (subtle), `changes-stage` (selected row), `history-local` (the active sidebar item) and `history-tab-focus-hover`. It adds `porcelain-…-delete-confirm-danger-focus`, `porcelain-…-editor-save-focus` and `kanagawa_lotus-…-changes-stage-focus-hover`, described below.
- 19 committed frames are replaced; each changes only by the band. They are `themes/editor/{dark,light}-{1000x680,1440x900}-{08-saved,13-cancelled,16-deleted}.png`, `themes/editor/light-1000x680-{12-cancel-focused,15-delete-confirm}.png`, `themes/linux-gaps/editor-{midnight,daylight}-1000x680-{delete-confirm,save-refused}.png` and `themes/import-export/list32-1000x680-12-tab-reveals-row32-delete.png`. Base reproduced each of them byte for byte first.
- 30 committed frames that show a focused helper predate the band and were not recaptured, because they need the FileChooser portal or the no-portal flow: `themes/import-export/transfer-1000x680-{06,07,08,11,13…25}`, `transfer-1440x900-{07,13,21,25}`, `collision2-1000x680-02-collision-2`, `lotus-1000x680-01-refuse-notjson`, `noportal-1000x680-{01-export,03-import}-guidance`, `themes/linux-gaps/imported-{midnight,porcelain}-1000x680-focus-{beside,on}-highlight` and `button-states/kanagawa_lotus-1000x680-imported-button-hover`. All 49 frames named here were retaken on September 29 ([frames retaken under the solid focus ring](#september-29-frames-retaken-under-the-solid-focus-ring)). The new `changes-stage` frames show the helper on the selected surface that the import-highlight frames showed.

Helpers that a caller turns into another variant also get the band. On the danger fill of the Delete theme confirmation it reads at most 1.50 in Porcelain and 1.68 in Midnight. On a primary button such as the editor's Save it is accent on accent, and only 48 corner pixels change. Both still rely on the toolkit's half-opacity ring, which reads 2.27 in Porcelain (unchanged) and 3.74 in Midnight. In Kanagawa Lotus the Changes Stage helper reads 3.73 focused on the selected row, and 2.71 focused and hovered: the documented row exception, where the base ring alone models at about 1.75. A focused helper that is disabled paints no band: the editor's `saving` frames, whose disabled Save shows a ring, are identical to base.

The design reviewer accepted the change. At 4x and 12x the band reads as the crisp inner edge of a two-tone ring on every surface, and the 7 px corners follow the arc without gaps. All 19 replacements were accepted; the reviewer's own diff found nothing else changed. On the danger fill the line is continuous and clear of the label and reads as part of the ring, but it adds no contrast and slightly muddies the corners (mauve in Porcelain, grey in Midnight), which a follow-up may revisit. The primary button was accepted unchanged, and the row exception was accepted as documented because it holds only while the pointer is on the button.

Still open: macOS, where the inset shadow, the first in the app or toolkit, has not been rendered; scale factors other than 1, where the band is one device pixel and can be under one logical pixel; primary, danger and directly built borderless buttons, whose half-opacity ring alone is below 3:1 in 16 of 20 palettes, and the row exception, both needing an outside stroke through a vendored Button change; the 30 frames listed above (retaken on September 29, [frames retaken under the solid focus ring](#september-29-frames-retaken-under-the-solid-focus-ring)); and the focused disabled helper. The theme draw-cost driver finds row 1's Edit… by the old ring's pixels ([benchmark notes](benchmarks/2026-09-22-theme-draw-cost.md)) and may need re-pointing before its next run. It was re-pointed on September 29 ([frames retaken under the solid focus ring](#september-29-frames-retaken-under-the-solid-focus-ring)).

## September 27 desktop monospace font on Linux

Linux Settings gains **Use the desktop's monospace font**, below **Code text size** and off by default. When it is on, code uses fontconfig's `monospace` family, but only a family the toolkit loaded that draws basic Latin at one advance. Otherwise the row gives the reason and code keeps the embedded DejaVu Sans Mono. The desktop-text worker looks the family up at launch, on focus return and when the setting turns on, with one bounded `fc-match --format %{family} monospace` per connection. Each reply carries the generation of the setting it answers. `desktop_text::tests` covers the family list parser, the one-advance rule, the lookup against a text system without system fonts, and a stale reply, a theme change and turning the setting off. `settings::picker_tests::the_code_font_switch_saves_the_choice_and_starts_the_lookup` clicks the switch on and off. Dropping the generation check fails the reply test. A mutant without the first draft's re-assertion in `Palette::apply` passed, because `Theme::change` keeps the family whenever no theme config names one, so that line was dropped.

The evidence is full tier, from one session on the owner's Omarchy desktop: Hyprland 0.56 on native Wayland. Every frame came from a temporary 1400 × 2100 headless output at scale 1, fullscreen, via `grim -o` whole-output grabs, so the owner's panel kept its workspace. Its `hyprctl monitors -j` matched the reading from before each run. The base was `f0aa814` (release, clean, sha256 `28785a42…`). The only later product change on `main` `f9ac8d4` is the rail button's accessible name, which draws no pixel. The candidate was `e605251` (release, clean, sha256 `e54d5d2d…`). The multi-line source-view frames came from its parent `58c7915` (release, clean, sha256 `5ed31ea1…`), which differs only in the order of the launch snapshot and the lookup, and in two strings no source frame shows. Each launch had a fresh HOME with the `GitTurtle QA <qa@example.invalid>` identity and fresh XDG directories. The fixture from `scripts/create-demo-repo.py` at `/tmp/gitturtle-evidence/code-font-c/demo` was byte-identical before and after. Each launch's `$XDG_CONFIG_HOME/fontconfig/fonts.conf`, in the format `omarchy-font-set` writes, set the `monospace` alias. Without that file, Omarchy's system default resolves to JetBrainsMono Nerd Font.

- **Off.** `qa.py compare --mask status-timing` found the base and candidate source views identical. In Settings, the candidate adds the row, switch off, reading "Code uses the bundled DejaVu Sans Mono. Turn on to use fontconfig's monospace font when it is fixed-width." The rows below it move down by the row's 54 px.
- **On at launch** (seeded preference): the row reads "Code uses JetBrainsMono Nerd Font, the desktop's monospace font." The Code text size sample and the source view draw in it. The lookup runs after the launch snapshot, so launch no longer waits on it.
- **Focus return.** With the alias changed to Liberation Mono, returning focus redrew Settings and an open source editor in Liberation Mono, with the columns and gutter intact. With Liberation Sans, the row read "Liberation Sans is not monospace. Code uses the bundled DejaVu Sans Mono.", and the source view was pixel-identical to the setting-off frame. With the alias removed, JetBrainsMono Nerd Font returned, and the editor frame was identical to the one before the switches.

The worker's `fc-match` query took 17.2 ms at p50, 18.8 ms at p95 and 39.8 ms at most, over 55 runs after 5 warmups, on this two-core host with a warm fontconfig cache and a load average of about 1.2. It runs on the desktop-text worker only while the setting is on, and is bounded at 500 ms and 1 KiB. The in-process check (`all_font_names`, one face, seven advances) was not timed separately.

A `design-reviewer` pass found nothing blocking in the pixels. The base Settings rows match the candidate's rows shifted down by 54 px, byte for byte. The row uses the Settings description helper and switch alignment, and code keeps an 18 px line pitch with the gutter within 1 px in all three fonts. On its advice, `e605251` stopped launch from waiting on the lookup, tells the user to relaunch after installing a family GitTurtle has not loaded, and rewords the off description. Its open items were these. JetBrains Mono joins `--` into one stroke, which in a diff can read as a dash; whether to turn ligatures off for code is the owner's call. The pending, turning-off, hover and focus states were not captured natively. Neither was a light theme, the stacked layout, enlarged text or a fractional scale. No test asserts the row's description text. And launch time with the setting on, from a cold fontconfig cache, was not measured.

Not covered: frames are not committed, because this host has no privacy templates, so they stay in the local QA bundle. The switch was not clicked natively, because pointer input was unavailable; the view test clicks it, and the native "on" state came from a seeded preference. GNOME's own `monospace-font-name` setting is not read, only fontconfig's alias. A font installed while GitTurtle runs is found after a relaunch. macOS is unchanged, and the row is Linux-only.

## September 27 navigation choice in narrow History windows

Below an 800 px repository width, History shows the rail instead of its navigation ([fractional scales](#september-27-fractional-scales-on-hyprland)). Before this change, Toggle Sidebar still flipped the saved choice there with no visible effect. With navigation hidden by choice, the rail's navigation button stayed enabled, and a click only set a state the width hid again. Now nothing in a narrow History window changes the choice. The rail button is disabled, with its "Widen the window to show branches and worktrees" tooltip, whether navigation was shown or hidden. The shortcut, the menu bar action and the command palette share `toggle_sidebar`, which leaves the choice alone there, and the palette lists Toggle repository sidebar as unavailable with the same reason. `set_menus`' View menu is drawn only on macOS, where the 1,000 px minimum keeps History wide enough. The Linux main menu does not list the command. `views::tests::narrow_history_keeps_the_navigation_choice` fails when the old shortcut, rail or palette logic is restored, one at a time, and passes on the fix.

The evidence is full tier, from one session on the owner's Omarchy desktop: Hyprland 0.56 on native Wayland, the 1366 × 768 panel at scale 1. The base was `44e50f4` (release, clean, sha256 `6c56af46…`), whose product source is that of `main` `8191205`. The candidate was `f0aa814` (release, clean, sha256 `28785a42…`). Each launch had a fresh HOME with the `GitTurtle QA <qa@example.invalid>` identity and fresh XDG directories. The fixture from `scripts/create-demo-repo.py` at `/tmp/gitturtle-evidence/sidebar-b/demo` was byte-identical before and after. The two builds were tiled side by side on one workspace. Each was captured in turn from the left tile (664 × 718) and fullscreen (1366 × 768), with the same `wtype` keys, from `grim -o` whole-output grabs. `qa.py compare --mask status-timing` found the launch frame, the frame after hiding navigation wide, and the frame after Ctrl+B with navigation hidden and narrow equal. A first launch frame differed by one step in every pixel, because the base window was unfocused and Hyprland draws inactive windows translucent; retaken focused, it matched. The differences were these:

- After Ctrl+B in the narrow tile, and in the narrow tile with navigation hidden by choice, the base draws the rail button enabled and the candidate draws it disabled (52 px).
- In the palette filtered to "sidebar", the base offers Toggle repository sidebar. The candidate shows it unavailable, with the reason beneath it and Open command disabled.
- Fullscreen after Ctrl+B in the narrow tile, the base has lost its navigation and the candidate still shows it. Fullscreen after Ctrl+B with navigation hidden and narrow, the base shows navigation and the candidate keeps it hidden.

A `design-reviewer` pass over the pairs found nothing blocking and attributed every non-timing difference to the change. The disabled rail glyph is drawn in the same colour as the disabled Fetch and Pull in the same frame, and the palette's row uses its existing unavailable treatment. Following that review, the disabled button's accessible name keeps its action and adds the reason ("Show branches and worktrees. Unavailable: widen the window"), as the palette's unavailable commands read, and the view test asserts the name, tooltip and disabled state. That commit changes no drawn pixel, so the frames above still attest `f0aa814`. Two follow-ups remained open: Ctrl+B in a narrow History window gave no feedback, neither visible nor announced; and no frame showed the disabled button's tooltip on hover. Both are resolved in [September 29 narrow History Ctrl+B announcement](#september-29-narrow-history-ctrlb-announcement).

Not covered: frames are not committed, because this host has no privacy templates, so they stay in the local QA bundle. Pointer clicks on the rail button were not driven natively (the view test clicks it), and neither was macOS, whose minimum window size keeps History wide.

## September 27 fractional scales on Hyprland

The owner asked for checks at scales 1.25 and 1.5 on the desktop from [the Omarchy check](#september-27-omarchy-and-hyprland), a 1366 × 768 panel. Hyprland 0.56 accepts only a scale that gives the output a whole logical size, so that panel takes 1 or 2: `hyprctl eval 'hl.monitor({ output = "eDP-1", …, scale = 1.25 })'` left it at 1, and 1.5 at 2, without an error. So 1.25 ran with the panel in its 1280 × 720 mode (1024 × 576 logical), and 1.5 and 2 on `hyprctl output create headless` outputs, 1536 × 864 at 1.5 (1024 × 576) and 1920 × 1080 at 2 (960 × 540). Rules were applied at runtime and no configuration file changed. `hyprctl reload` and removing the headless output restored the panel, and its `hyprctl monitors -j` matched the reading from before the check: 1366 × 768 at 60.003 Hz, position 0 × 0, scale 1, no transform.

GPUI renders through `wp_fractional_scale_v1` and `wp_viewporter`: a 1000 × 526 window drew 1250 × 657 pixels at 1.25 and 1500 × 789 at 1.5, and the app followed a live scale change without a relaunch; a fresh launch at 1.25 measured the same. Text was as sharp as at scale 1. Over one commit title, the share of edge pixels that were only partly covered was 0.53 fullscreen and 0.56 tiled at 1.25, 0.46 at 1.5 and 0.58 at scale 1. A `grim -g` crop at a tile's half-pixel origin (y 38 logical is 47.5 physical) resamples the frame and reads 0.75, so frames came from whole-output grabs cropped on physical pixels. In a 1000 × 526 window at both scales, History, Compare with an image and a text diff, Changes, Settings with the theme picker, Quick Open, the command palette and Projects drew whole, and lists scrolled rather than clipped. At 493 × 526 and 1.5, Settings showed the two-column theme picker, which no window under X11 reaches, since X11 keeps the 1,000 px minimum.

**Finding.** Two windows side by side on a display about 1,000 logical pixels wide are 493 px wide each, and 461 px at 960. Below 564 px, the rail, the 240 px content column and the 280 px inspector no longer fit, and the inspector ran off the window mid-word in History, Compare and Changes. The header and the Git action bar wrapped onto a second row, as designed. `634542d` lets the inspector yield to 240 px and then the content column to 176 px, so the inspector stays whole down to about 460 px. That left Compare's path row wider than its column, with Blame drawn under the inspector as "Blam"; `44e50f4` caps the row at the column and wraps its actions. `views::tests::narrow_windows_keep_the_inspector_whole_and_the_header_on_one_row` now resizes through 560, 493 × 526 and 460 × 526 and then opens Compare. It fails on the previous minimums at 560 px (the inspector's right edge at 564 px) and on `634542d`'s path row at 493 px (Blame's right edge at 273.5 px, the files at 254 px).

Evidence came from one session, first with the panel at 1.25 and then on a 1920 × 1080 headless output at 2. The base was `319eae6` (release, clean, sha256 `1f6e0438…`), whose product source is that of `main` `4ac3953`; the candidate was `44e50f4` (release, clean, sha256 `6c56af46…`). Every launch had a fresh HOME carrying the `GitTurtle QA <qa@example.invalid>` identity and fresh XDG directories, on a fixture from `scripts/create-demo-repo.py` at `/tmp/gitturtle-evidence/omarchy-scale/demo`, which was byte-identical before and after. In a single 1000 × 526 window, `qa.py compare --mask status-timing` found the two builds' History and Compare frames equal except for the timing text and two glyph-edge specks of 2 and 3 px; two launches of the base differ in the same way. In side-by-side tiles, 493 × 526 at 1.25 and 461 × 490 at 2, the base clips the inspector in History, Compare and Changes, while the candidate keeps it whole, along with the file badges and Compare's Blame. At 461 × 490, the candidate's 176 px Compare column wraps the review options one per row and leaves the diff no visible line (two lines show at 493 × 526), because the header and the Git action bar each take two rows below 640 px. A `design-reviewer` pass over the pairs approved the change: the control differences are noise, and nothing blocks. It left these follow-ups open: in History's 176 px column the paging buttons are hidden; at 490 px the review options, the mode group and the second header and action rows could collapse to give Compare's diff its height; "Original Git diff" sits 6 px left of the wrapped buttons; and the composer's heading scrolled out of view at 461 × 490, which was not checked with staged files.

Not covered: frames are not committed, because this host has no privacy templates, so they stay in the local QA bundle. Once, after a series of live scale changes and a fullscreen toggle, Tab in Settings moved no focus; it did not reproduce in three later attempts at 1 and 1.25. Also not covered: an enlarged interface text size (the rail scales, but the 240 and 176 px minimums do not), several monitors with different scales, XWayland clients at a fractional scale, and pointer interaction, since every step was driven with `wtype`.

## September 27 themes states on Linux

Task `themes-evidence-gaps-linux` captures the themes states that no Linux frame attested (themes follow-up 22, the Linux part). All 65 frames are in [`evidence/themes/linux-gaps/`](evidence/themes/linux-gaps/) and come from one build, merged `main` `9a0a17b`: debug, sha256 `5a784da4…`, `--build-info` source tree clean. These frames attest that build, apart from `editor-{midnight,daylight}-1000x680-{delete-confirm,save-refused}`, which were replaced from `56a3fe5` when the [helper focus ring](#september-27-helper-focus-ring-at-full-accent) gained its band. The later #59 (`4ac3953`) embeds a Linux code font and changes the layout below 800 px, so code text may render differently in later builds.

The app ran on Linux under XWayland on the GNOME 46 Wayland desktop (`DISPLAY=:1`), at `GPUI_X11_SCALE_FACTOR=1` and 1000x680 unless a frame's name says otherwise. Every launch had a fresh HOME and XDG directories, the `GitTurtle QA <qa@example.invalid>` identity and a fixture under `/tmp/gitturtle-evidence/`, and both fixtures were byte-identical before and after. The driver copies and flow logs stay in the local QA bundle. A privacy scan of all 65 frames with the local template set came back clean.

- **Helper buttons directly on `panel` and `canvas`** (`helper-*`, Midnight and Porcelain). The History header's Pull requests sits on `panel`, and the history toolbar's Latest on `canvas`. In Midnight, hover and pressed read (34,44,60) and (34,59,59) on `panel`, which are exactly `hover` and `selected`, and (27,35,49) and (28,51,49) on `canvas`, as `Palette::control_fill` composites them.
- **Scrollbar thumb** (`thumb-*`, in the 32-theme Your themes list, Midnight and Braden). Hovering widens the thumb and paints it `muted`, and dragging moves the thumb and the list. A pressed thumb is pixel-identical to a hovered one. The design reviewer ruled this toolkit behaviour rather than a design gap: `DESIGN.md`'s pressed-uses-selected rule covers buttons, the toolkit defines only thumb and thumb-hover colours (`vendor/gpui-component/src/theme/schema.rs`), and the thumb following the drag is the feedback.
- **Scale factor 2** (`scale2-*`, `GPUI_X11_SCALE_FACTOR=2`, 2000x1360 physical): History, Settings with the theme picker, and the theme editor, in Midnight and Braden.
- **The FileChooser portal dialogs** (`portal-*`). These are Wayland clients, which the XWayland grabs used until now cannot see. They were captured with Mutter's ScreenCast `RecordArea` and cropped to the dialog; the same route matched an X11 grab of the app window pixel for pixel.
  - The Save dialog for Export… opens in the launch's HOME with the suggested `<slug>.gitturtle-theme.json` name.
  - The Open dialog for Import… starts in Recent, which lists the desktop user's own files, so it was captured only after opening a per-launch folder that holds the two theme documents.
  - The portal runs in the desktop session, not in the launch's isolated HOME, so its sidebar shows the host's two GTK bookmarks, generic folder names.
- **Editor states** (`editor-*`, Midnight and Braden): an edited draft, Reset to base, the Replace colors question and its result, the delete confirmation (captured after Tab moved focus to Delete theme; the alert opens with Cancel focused, as `DESIGN.md` requires), and a failed delete's error. Also shown is a refused save: "Saving…" appeared 64–73 ms after Space, and 143–150 ms after it was replaced by "Could not save the theme: Save preferences: Permission denied (os error 13)". Holding "Saving…" long enough to capture took a store padded to 7.9 MB and a read-only `config/gitturtle` in the launch directory, restored afterwards. The editor at 18 pt interface text is shown at the top and wheeled to its end with the footer in view (`editor-midnight-1000x680-text18*`). At 18 pt the dialog's panel ends 19 px above the window's bottom edge, against the 16 px `DESIGN.md` gives and the 16 px measured at 13 pt.
- **Focus beside the import highlight** (`imported-*`, Midnight and Porcelain), reached from the keyboard: the imported row highlighted, Edit… focused on the row directly above it with its ring against the highlight, then the highlighted row's own Edit…. The ring stays whole beside and on the highlight. **Finding (accessibility):** the shared helper has no border, so its focus ring is only the toolkit's 3 px ring layer at 50% of `accent` (`FOCUS_RING_OPACITY`, `vendor/gpui-component/src/styled.rs`). Measured, it reads 2.23:1 on Porcelain's subtle surface and 2.15:1 on the highlight (Midnight 3.67:1 and 3.11:1), below the 3:1 the focus-ring rule sets for `accent`. The rule is computed on the declared accent, not on the rendered ring. The reviewer's computation for the ten original palettes has only Midnight clearing 3:1 everywhere, with Sandstone lowest at 1.98:1. A product follow-up could paint the helper's ring at full accent, for example with a 1 px border so the ring's full-accent edge shows.
- **A partly scrolled row** (`partial-*`): after five wheel steps the list sits half a row off a boundary. Edit… is hovered and held on the half-hidden top row and hovered on the half-visible bottom row.
- **Light palettes with the 32-theme list** (`list32-*`): Braden, Porcelain and Kanagawa Lotus, at the list's top and end.

Hover and press grabs changed the frame 19–59 ms after the input, in a debug build; this is not a release measurement.

**AT-SPI, as observed.** The host runs with accessibility off (`busctl --user get-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled ScreenReaderEnabled` read `false false`, and `gsettings get org.gnome.desktop.interface toolkit-accessibility` read `false`). In that state, GitTurtle appeared neither among the children of `Atspi.get_desktop(0)` (queried through `python3` with `gi.repository.Atspi`) nor in `busctl --address=<the address from org.a11y.Bus GetAddress> list`. That is why the earlier themes passes saw no registration.

For one launch, `busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true` turned accessibility on. GitTurtle then appeared as desktop child `'gitturtle' … application 1` with its own connection on the accessibility bus. Its tree held a `frame` "GitTurtle" with 30 children, including the `button` "Main Menu", the `page tab list` "Repository tabs" with the selected tab and its close button named, the `toggle button`s History and Changes, and `button`s such as "Pull requests" and "Settings". `IsEnabled` was then set back to `false` and read back as `false false`, with `toolkit-accessibility` still `false`. No screen reader ran, so what Orca announces is not attested.

Still open:
- **The two-column picker.** The window keeps its 1,000 px minimum width (a 990 px resize stayed at 1000x680). The picker drops to two columns only below `ui_size(720)`, which is 996.9 px at the 18 pt maximum interface text, and X11 does not apply GNOME text scaling. So no Linux X11 window reaches it.
- **Scale factors other than 1 and 2.**
- **What a screen reader announces on Linux.**
- **The shared helper's focus ring below 3:1 in light palettes** (the finding above), a product change outside this evidence task.
- **The macOS items**, which need a real Mac (task `themes-evidence-gaps-macos`): the Settings theme picker, the editor and import-export at 2x, a failure line in a light theme, the native file dialogs, the shared button's hover and pressed states, and VoiceOver. No Mac run exists, so none of these is claimed.

## September 27 Omarchy and Hyprland

The owner asked whether GitTurtle supports Omarchy. The check ran on the owner's own desktop: Omarchy 4.0.4 (Arch), Hyprland 0.56.2 on native Wayland (the window reports `xwayland: false`), AMD Radeon Vega (Picasso/Raven 2) with Mesa 26.2.2 and `vulkan-radeon`, one 1366 × 768 display at scale 1, two CPU threads, `xdg-desktop-portal` 1.22.1 with the `hyprland` 1.4.1 and `gtk` 1.15.3 backends, and no DejaVu, IBM Plex or Lilex fonts installed. The repository was this checkout, read passively; no write, staging or network action ran.

The base, `f3836ec` (release, clean, sha256 `a5299020…`), was built with `scripts/package-linux.sh` after `pacman -S rustup python-pillow cmake` (a cold build took 38 min 42 s) and installed with the bundle's `install.py`. It launched on native Wayland with class and app ID `com.gitturtle.desktop`, matching the installed entry (`desktop-file-validate` clean, eight icon sizes present). Hyprland grants server-side decorations without a title bar, so no client window controls are drawn. The app followed the dark preference and showed History (257 commits) with its graph and inspector, Quick Open and the source view; local refresh reported working-tree edits and new commits. **Ctrl+O** opened `xdg-desktop-portal-gtk`'s Open Folder dialog about 4 s after the key on the first open, and cancelling it left the view and focus unchanged. **Ctrl+Q** quit with status 0 and relaunching restored the selected commit.

Two defects were found. Code, diffs and hashes drew in the proportional Adwaita Sans, because the toolkit matches family names exactly and DejaVu Sans Mono was not installed. And tiled windows, which Hyprland sizes below the 1000 × 680 minimum, clipped: in 900, 800, 700 and 600 px tiles the layout fit exactly at 800 px, while at 700 px the inspector ran off the right edge mid-word and the header wrapped to two rows (three at 600 px, with the action bar wrapping too).

The candidate `319eae6` (release, clean, sha256 `1f6e0438…`) embeds DejaVu Sans Mono on Linux (`b45ae28`), shows the History rail and a compact header below 800 px (`a1ea120`), and keeps the compact profile button's icon whole (`319eae6`; `a1ea120`, sha256 `0d6b27e4…`, drew it as a 26 px sliver). In a 664 × 718 tile the header stayed on one row, History showed the rail, and the inspector stayed whole in History, the source view and a Cargo.lock diff, whose comparison modes wrapped onto their own line as designed. Code, diff gutters and the SHA column drew in DejaVu Sans Mono. Maximized (1342 × 718), the navigation, Projects label and full profile button returned unchanged.

Launch to first window, measured with `hyprctl` polling on an empty workspace, fresh XDG directories per launch and a warm page cache, ten launches each and interleaved: `a1ea120` median 380 ms (354–468) against `f3836ec` 408 ms (363–439). The font registration shows no measurable cost; this is not a speed claim.

`desktop_text::tests::bundled_code_font_is_monospace_in_every_style_without_system_fonts` loads the faces with no system fonts. `views::tests::narrow_windows_keep_the_inspector_whole_and_the_header_on_one_row` resizes a repository window through 1000, 800, 664 and 600 px; it fails on `f3836ec`'s layout with the inspector's right edge at 800 px in a 664 px window, and on `a1ea120` with the 26 px profile button.

The owner then used the installed `319eae6` on the same desktop: Omarchy's launcher (**Super+Space**) lists GitTurtle with its title and icon, and the owner reported their everyday workflows working. That report is the owner's, not a recorded procedure. Not covered by a recorded check: fractional or HiDPI scales, multiple monitors, XWayland on this desktop, a rail tooltip hovered with a pointer, and write and network workflows. The frames show the owner's desktop notifications and stay out of the repository.

## September 27 pressed buttons distinct from hover

Task `themes-press-distinct` (themes follow-up 23) adds the pressed-step [readability rule](development/themes/spec.md#readability-rules): the shared button's pressed layer, `control_fill(selected)`, stays at least 6 of 255 in one channel from its hover layer, `control_fill(hover)`, composited over each of the six surfaces. Sandstone (3 apart on every surface) and Porcelain (4 over the hovered selected row) failed it, so their `selected` now leans toward their accent's hue: Sandstone `#EDDFD0` → `#F2DCD0`, Porcelain `#DFE6F6` → `#DCE6F6`, both 8 apart on every surface. Kanagawa Wave meets the rule unchanged, because its pressed state steps by hue. The spec records why the rule is 6 rather than the proposed 8 and has no lift clause. `CONTROL_PRESS_DISTANCE` in `appearance::tests::shared_button_fills_lift_every_surface_and_keep_the_label_readable` is now the rule's 6. With Sandstone's old `selected`, that test fails ("Sandstone pressed eddfd0 is 3 from hover eae1d3 on the panel"), and so does `palettes_keep_text_and_diff_content_readable_in_each_theme` ("Selected against Hover differs by 3, needs 6", the wording at `192a625`).

Native evidence ran on Linux under XWayland on the GNOME Wayland desktop (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`, 1000x680, and 1440x900 for the 1440 picker frames). Base and candidate ran one after the other in the same session. Base was `1cd69b8` (sha256 `d47f1b31…`) and the candidate `192a625` (sha256 `f3bbc6d2…`), both debug builds from clean trees. The branch's later commits change only the step warning's wording and documentation, and no captured frame shows that text. Every launch had a fresh HOME and XDG directories, the `GitTurtle QA <qa@example.invalid>` identity and a fixture under `/tmp/gitturtle-evidence/` (`theme-fixture`, `ie-d3-demo`), which all 52 launches left unchanged. The button-state driver is the IE-D3 driver ported onto `scripts/native_qa`; base reproduced the committed Porcelain row frames byte for byte. Every press was released off the control, and every after-release frame matched rest. Measured fills (8-bit RGB, contrast against the surface beneath, and the largest one-channel step from hover to pressed):

| Theme, control (surface) | Base hover / pressed, step | Candidate hover / pressed, step |
| --- | --- | --- |
| Sandstone, Edit… and Stage (234,225,211) | (216,202,181) 1.244:1 / (219,200,179) 1.254:1, 3 | (216,202,181) 1.244:1 / (224,197,179) 1.266:1, 8 |
| Sandstone, New theme… (240,234,222) | (221,210,191) 1.248:1 / (224,208,188) 1.259:1, 3 | same hover / (229,205,188) 1.271:1, 8 |
| Porcelain, Edit… and Stage (229,233,244) | (206,213,234) 1.207:1 / (200,211,236) 1.237:1, 6 | same hover / (198,211,236) 1.241:1, 8 |
| Porcelain, New theme… (239,241,248) | (215,220,238) 1.211:1 / (209,218,240) 1.241:1, 6 | same hover / (206,218,240) 1.248:1, 9 |
| Kanagawa Wave, Edit… and Stage (54,54,70) | (65,65,84) 1.190:1 / (46,64,91) 1.128:1, 19 | identical |
| Kanagawa Wave, New theme… (26,26,34) | (39,39,50) 1.171:1 / (22,40,60) 1.155:1, 17 | identical |

Every fill is within 1 per channel of the formula. Button labels read 7.7–9.7:1 at their glyph cores. The thin, antialiased strokes of Sandstone's Stage `+` icon peak at 4.43–4.49:1 on both builds, although the label color itself is 7.8:1. In Sandstone and Porcelain the candidate differs from base only at the selected repository tab, the selected Changes segment and the held button. Kanagawa Wave is identical in all its frames.

Frames changed and why:
- `button-states/sandstone-*` and `button-states/kanagawa_wave-*` (six each) are new. They show hover and pressed on a row action, a header button and the Stage button.
- `themes/sandstone-porcelain/` (ten) is new. For each palette it holds the palette-set driver's History selected, hover and focus frames, a Compare frame and the Settings picker. The hover frame has the selected commit row beside a hovered one: Sandstone's selected row now stands 8 from a hovered row (3 on base, 1.018:1), and Porcelain's 9 (6 on base). Sandstone's Compare frame has the selected file row beside removed lines, 13 from their tile (1.065:1). Against base, these frames differ only on selected surfaces: the repository tab, the History and Diff segments, the sidebar item, the selected commit and file rows and the miniatures.
- `button-states/porcelain-*` (six) are replaced because the selected repository tab and Changes segment use the new `selected`, and so does the held button in the pressed frames.
- Ten frames change only in the Porcelain and Sandstone miniatures of the Settings theme picker, whose second row paints each theme's `selected`. They are `themes/alucard-kanagawa/{alucard,kanagawa_lotus}-settings-picker.png`, `themes/solarized-one/{one_light,solarized_light}-settings-picker.png`, `themes/rose-pine-dracula/rose_pine_dawn-settings-picker.png`, `themes/editor/dark-1000x680-17-base-selected.png`, and `themes/picker/comfortable-1440x900-{01-light-dark,06-follow-system-on,09-follow-system-cleared}.png` and `compact-1000x680-01-light.png`.
- Three Midnight frames change only by a new "(!) 1" warning glyph. The 32-theme store holds custom copies of Porcelain and Sandstone saved with the old `selected`, and under the new rule each reports one warning. The frames are `themes/import-export/list32-1000x680-12-tab-reveals-row32-delete.png` and `themes/picker/store32-{compact-1440x900-01-list-density,1440x900-01-your-themes-rows-1-3}.png`.

Base matched every committed frame it reran (53) apart from one status-bar pixel that the committed frame also shows. The other 143 PNGs under `docs/evidence/themes/` and `docs/evidence/button-states/` were checked statically: none contains a pixel of Porcelain's or Sandstone's old or new `selected`, accent or hover. Every custom theme in the stores behind them keeps a pressed step of 11 to 13, so no other warning count moves. The replacements carry that session's timing digits. A privacy scan of the 41 committed frames with the local template set came back clean.

The design reviewer accepted every difference. Sandstone's pressed state is now a clear clay tint (CIEDE2000 6.4 from hover, 2.3 on base), and Porcelain's is bluer, distinct but only just (2.5–3.0). The selected tab, segment and rows read as the palettes' intent, and Sandstone's selected row is a blush beside a tan hovered row. Its selected file row and the removed lines never share a pane, and neither relies on color alone. Kanagawa Wave was accepted unchanged, with no exemption: its darker, bluer press is 17 to 19 from hover (CIEDE2000 7.4), follows its waveBlue selected row, and stays lifted at least 1.128:1. The miniature-only and warning-glyph frames were accepted as replacements, and `DESIGN.md` was found consistent.

Still open: macOS; scale factors other than 1; helper buttons sitting directly on `panel` or `canvas`, which only the unit test covers; and Solarized Dark's pressed label at 4.575:1 over the hovered selected row, above the rule but inside the 0.25 margin. The editor's empty-list copy, "Every pair meets its contrast minimum.", now also covers the step rule. A step warning marks only the Selected and Hover rows, although Panel, Canvas, Subtle and Accent also enter the measure. Both are in `theme_editor.rs`, outside this task. Both were resolved on September 29 ([theme editor readability follow-ups](#september-29-theme-editor-readability-follow-ups)).

## September 24 shared button hover and pressed states

Themes follow-up 21 (IE-D3) replaces the kit's ghost styles on the shared 28 px `button` helper (`crates/app/src/main.rs`). Before, the helper's hover painted darker than a hovered row (1.16:1 the wrong way), and pressed differed from hover by (1, 2, 2). Now its hover and pressed fills are translucent layers from `Palette::control_fill` (`ec341d6`). Over `panel` they composite to exactly `hover` and `selected`; on any other surface they add nearly the same step, so an action in a hovered or selected row still lifts. `Palette::control_label` keeps the label at least 4.5:1 on every fill (`79d5c65`, `1625b2f`). Only Kanagawa Lotus (`545464` to `41414e`) and One Dark (`aeb5c2` to `bdc2cd`) move their helper label, at rest too, because the kit paints one foreground for every state. Selected helpers in those two themes use the same label. The other 18 themes keep `text`. Two deliberate departures from the item's literal wording were accepted by the design reviewer:
- hover equals `p.hover` exactly only over `panel`;
- pressed is a translucent `selected` layer, not an opaque `selected` fill. An opaque fill lands 1 per channel from a hovered button on a hovered row in Rosé Pine, and disappears on selected rows.

`appearance::tests::shared_button_fills_lift_every_surface_and_keep_the_label_readable` checks all 20 built-in themes on panel, subtle, canvas, hovered, selected and hovered-selected rows, for lift, hover-to-pressed distance and label contrast. `palette_application_hands_the_shared_button_its_fills_and_label` checks the variants that palette application installs. Both fail on the unchanged or partially fixed helper. `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` and the agent-loop suite (umask 022) passed on this Linux host.

Native evidence ran on Linux under XWayland on the GNOME Wayland desktop (`DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`, 1000x680). It used debug builds of base `ff06709` and each candidate, a fresh HOME and XDG directories per launch with the `GitTurtle QA <qa@example.invalid>` identity, and fixtures under `/tmp/gitturtle-evidence/`. Every press was released off the control, and every after-release frame matched rest. Measured button fills (8-bit RGB, contrast against the surface beneath):

| Theme, surface | Base hover / pressed | Candidate hover / pressed |
| --- | --- | --- |
| Midnight, hovered row (34,44,60) | (24,32,45) 1.16:1 darker / (25,34,47), 2 from hover | (44,57,75) 1.20:1 / (44,71,74), 14 from hover |
| Midnight, subtle header (19,26,37) | (21,28,40) 1.02:1 / 3 from hover | (30,40,54) 1.18:1 / (31,56,54) 1.40:1, 16 from hover |
| Porcelain, hovered row (229,233,244) | (210,215,235) 1.18:1 / 27 from hover | (206,213,234) 1.21:1 / (200,211,236), 6 from hover |
| One Dark, row | 1.21:1 darker / 1.16:1 darker | 1.09:1 / 1.15:1 lift |

Every candidate fill is within 1 per channel of the formula. Kanagawa Lotus and One Dark labels read 4.92–7.82:1 on the candidate's fills, including the hovered imported theme row, where `ec341d6` would have given 3.61:1. Porcelain's pressed state is quieter than the ghost's, because its own `hover` and `selected` tokens are 6 apart. While Stage is held, the working-file row keeps its hover colour, as in base.

Resting frames are pixel-identical to base in the other 18 themes, apart from per-launch status-bar timing text. In Kanagawa Lotus and One Dark only helper labels and icons change, so this change replaces those themes' eleven committed frames:
- `themes/alucard-kanagawa/kanagawa_lotus-*` and `themes/solarized-one/one_dark-*` (five each);
- `themes/import-export/lotus-1000x680-01-refuse-notjson.png`.
They were recaptured with the pass-B2 drivers alongside a same-session base run. Base matched the committed frames apart from timing text and one pixel that base also shows. The replacements carry that session's timing digits. The new frames under `evidence/button-states/` show hover and pressed on row actions, header buttons and the Stage button in Midnight and Porcelain, a hovered button on the subtle header and on the hovered imported row in Kanagawa Lotus, and One Dark's row pair.

Audit of `docs/evidence/`: no themes frame shows a hovered or pressed helper button, because those drivers park the pointer. Sixteen older consistency-milestone and macOS-milestone frames and four review-milestone frames have the macOS pointer resting on a helper button. Most show no fill, because the old hover barely registered; `consistency-milestone/recovery-retry-result.jpeg` shows the old fill. They remain dated evidence for their builds.

Still open (follow-ups 22 and 23): macOS, scale factors other than 1, helper buttons sitting directly on `panel` or `canvas` (covered by the unit test only), Sandstone's `selected` only 3 from its `hover`, Kanagawa Wave's `selected` darker than its `hover`, and Solarized Dark's pressed label at 4.575:1 over the hovered selected row. Focus and disabled states are unchanged by the variant, except that a disabled selected helper in Kanagawa Lotus or One Dark now has a transparent background.

## September 23 Theme picker with custom themes

Native QA for `themes-picker`, which lists saved custom themes in the Settings
theme picker as a **Your themes** group on the same preview cards as the
built-ins, tightens the grid to 132 px cards (four columns at and above the
scaled 1,060 px breakpoint, three below it), and makes the editor's preview the
same card. Linux/XWayland (GNOME Shell 46.0 on Wayland, x86_64, `DISPLAY=:1`,
`WAYLAND_DISPLAY` unset, `GPUI_X11_SCALE_FACTOR` 1), windows 1000x680, the app's
`window_min_size`, and 1440x900, with absolute throwaway `XDG_CONFIG_HOME`,
`XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` and `HOME` per launch, each
seeded just before that launch. The stores were a version-6 store with two custom
themes (Harbor Dusk, based on Midnight with no readability findings, and Paper Fog,
based on Braden with eight), the same with three (adding Lantern Grey, based on
Tokyo Night with its warning color equal to its own panel), the empty store, and
the 32-theme store of `themes-draw-cost` (sha256
`9deaa6eedec8ac129a9bb64e084b265b59f42e1bd9bba430f6ba366e4f04d23a`). Fixture:
`scripts/create-demo-repo.py` at HEAD
`52f471a1137c617fd8e36db2e6251a18f58c23eb`, clean before every launch and
afterwards; the picker writes only app preferences, and no network action was
taken. Follow system was turned on and off with the app's own switch; the
desktop color scheme (`prefer-dark`) was read, never changed.

The captures were taken from build `4cdd4df2c0767429ee4c182f73b8e5f82a4312e7`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`3ef3e3a0b999b00b76c64d2177f82a93814faf585de5452439bfbcd34894714e`). As with the
entries below, evidence committed to a repository cannot describe the commit
that contains it, so this entry names the revision under test; a later build
that ships the picker reuses these captures only when it renders them
identically. An earlier build of the same patch, `f826dc6`, passed the same flow
but its design review found three defects: the card caption had a fixed 54 px
height, so at interface text size 11 the description was cut through its glyphs
(Paper Fog kept 4 of its 8 ink rows); the warning glyph on a card was drawn in
that card's own palette, so a theme whose warning color equals its panel showed
an invisible disc (1.0:1); and the editor's preview card stayed 166 px tall, which
left 39 px of empty canvas under its miniature. In `4cdd4df` the caption grows at
text sizes 12 and 11 and the miniature gives up the difference (every name and
description line is drawn in full), the glyph uses the active palette as the check
badge does, and the editor's preview is the 132 px picker card: its inner pixels
match the picker's Harbor Dusk card exactly. Apart from Paper Fog's warning
glyph, every picker frame is pixel-identical to the `f826dc6` frames.

Compared with base `85a7d07` (binary sha256
`418a3b425cedb14fdf734ae02d756dce642896156dd214e6b165a9239a46aec6`; `08cef56` has
the same crates and Cargo files), History differed only in its status-bar timing
digits and the Your themes card was pixel-identical at both sizes. In the Edit
theme dialog the only difference was the preview card: at 1440x900 the side
column below it moved up exactly 34 px with no other change, and at 1000x680 the
stacked dialog differed only in its scrollbar thumb, because the dialog's content
is now 34 px shorter. With custom themes present the picker showed three groups:
8 light, 12 dark and 2 (or 32) custom cards. The empty store showed two. At
1440x900 the twenty built-ins took five rows. The picker had four columns at
1,060 and 1,440 px and three at 1,000 and 1,059 px at text size 13, and three at
every width at text size 18. A 700 px resize request was held at 1000x680 by the
window's minimum size, so the two-column layout, which needs a width below the
scaled 720 px, cannot be reached natively; `picker_cards_keep_the_grid_geometry`
covers it. Every compact-density Settings frame was pixel-identical to its
comfortable counterpart. The density shows in History and in the List density
control. The comfortable picker frames are byte-identical to the compact ones
(both 1000x680 top-of-Settings captures, for example, have sha256
`b39d5b8d…`), so apart from `compact-1000x680-01-light.png` the compact
captures listed below show History and the List density control instead of
repeating the picker. Accessible names could not be observed: AT-SPI does not list the app on
this host, so the "‹name› theme" labels rest on the `#[gpui::test]` suite. macOS
was not exercised.

The captures are under
[`docs/evidence/themes/picker/`](evidence/themes/picker/):

- `comfortable-1440x900-01-light-dark.png`: Settings at the top with Follow
  system appearance off, the Light group in two rows of four and the Dark group
  with Midnight marked.
- `comfortable-1440x900-02-your-themes.png`: the Your themes group at rest, with
  Harbor Dusk and Paper Fog. Paper Fog shows the warning glyph with its count, 8.
- `comfortable-1440x900-03-hover-custom.png`: the pointer resting on Harbor Dusk,
  with the accent inner border and the caption hover surface. Nothing outside the
  card changes.
- `comfortable-1440x900-04-selected-custom.png`: Harbor Dusk chosen with a click.
  Its card has the accent edge and the check badge, the window uses its
  `#101a2c` canvas, and the store saved `{"custom": 1}`.
- `comfortable-1440x900-05-focus-custom.png`: keyboard focus on Paper Fog,
  reached with Tab (Harbor Dusk at the 28th Tab, Paper Fog at the 29th). The
  card's 1 px outer border takes the accent color all the way round, as a
  built-in card's does on the base build. On the selected card, which already
  has that edge, focus changes only its 48 corner pixels.
- `comfortable-1440x900-06-follow-system-on.png` and
  `-07-follow-system-on-your-themes.png`: Follow system turned on with the
  switch, with Harbor Dusk still the saved selection. No built-in or custom card
  is marked.
- `comfortable-1440x900-08-custom-chosen-again.png` and
  `-09-follow-system-cleared.png`: Harbor Dusk chosen again. Its card is marked,
  the switch is back off, and the store saved `follow_system: false`.
- `comfortable-1000x680-01` to `-05`: the same group, selected, focus,
  follow-system-on and chosen-again states in the three-column layout.
- `compact-1000x680-01-light.png` and `compact-1440x900-01-history.png`: the
  picker in compact density, and History with compact rows.
- `store32-1440x900-01-your-themes-rows-1-3.png` and
  `store32-compact-1440x900-01-list-density.png`: the 32-theme store's custom
  group, and the Your themes list card above List density with Compact selected.
- `empty-1440x900-01-no-your-themes-group.png`: with no custom theme, the picker
  ends after the Dark group.
- `text12-1440x900-01-selected-warned.png` and
  `text11-1440x900-01-selected-warned.png`: at interface text sizes 12 and 11 with
  Paper Fog selected and warned. Its name row holds the glyph and the check badge,
  and Lantern Grey is warned. Every name and description line is drawn in full.
- `warning-1440x900-01-glyph-active-palette.png`: Midnight active. Both warned
  cards draw the glyph in Midnight's warning color with a Midnight-canvas "!". On
  Lantern Grey that disc is 8.9:1 against the caption.
- `editor-1440x900-01-preview-card.png` and
  `editor-1000x680-01-preview-card.png`: the Edit theme dialog for Harbor Dusk.
  Its preview is the 132 px picker card, with a 70 px miniature over a 54 px
  caption.

In the four accepted interaction runs, hover repainted the card 36–64 ms after
the pointer arrived, and a click changed the next grabbed frame within 14–31 ms.
These figures are non-authoritative: they are frame grabs on a shared host with a
one-minute load average of 2.4–3.1, and they are not a performance measurement.
A first compact 1000x680 run, at load 6.7, grabbed its hover frame after the
card's tooltip had begun to appear. It was repeated for the captures listed
here.

Re-taken on September 24: 22 of the 24 captures listed above were replaced by
captures of build `5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (GitTurtle 0.1.0,
`source_tree` clean, release, `x86_64-unknown-linux-gnu`, rustc 1.98.0
(88d9e12ae 2026-08-18), binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`), on the same
host, windows, stores and fixture and with the same driver. Each difference was
attributed in the same session against the build without the change: `fbc4555`
against `e73be2e` for [follow-ups](development/themes/follow-ups.md) 11 to 17,
and `5899e2a` against `fc2a355` for 20. Follow-up 13, which sizes the check
badge to the name's line and rings the badge and the warning glyph in 1 px of
the applied canvas, changed `comfortable-1440x900-01` to `-04`, `-07` and `-08`,
`comfortable-1000x680-01`, `-02`, `-04` and `-05`, `empty-1440x900-01` and
`warning-1440x900-01`; in `text11-` and `text12-1440x900-01` it also starts
Paper Fog's caption level with its neighbours. Follow-ups 13 and 14 changed
`comfortable-1440x900-05` and `comfortable-1000x680-03`, where the focused card
now has a 2 px accent ring 1 px outside its border. Follow-up 17, which widens
the stacked Edit theme dialog from 640 to 648 px, changed
`editor-1000x680-01-preview-card.png`. Follow-up 20 changed each of the other
21 re-taken captures: the built-in cards' captions and miniatures are drawn in the
nudged palettes, and the new warning-message rule adds one readability finding
to a saved theme whose warning is below 4.5:1 on its subtle surface, so Lantern
Grey counts 8 instead of 7 and, in the 32-theme store, Seed 11 and Seed 12,
saved with Solarized's earlier warning colors, show the glyph with a count of 1.
`comfortable-1440x900-06` and `-09`, `compact-1000x680-01-light.png` and the two
`store32-` captures changed for follow-up 20 alone.
`compact-1440x900-01-history.png`, which differs only in its status-bar timing
digits, and `editor-1440x900-01-preview-card.png`, which is byte-identical, were
not changed by any follow-up. Two new captures show follow-up 14 on the selected
card, Harbor Dusk chosen and focused with the ring outside its accent edge:
`comfortable-1440x900-05a-focus-selected-custom.png` and
`comfortable-1000x680-03a-focus-selected-custom.png`. The 24 picker launches, 12
per build, passed every check. Linux/XWayland at scale 1 only; macOS was not
exercised, and accessible names still rest on the `#[gpui::test]` suite.

The captures in this entry, the new ones included, were then taken again on
September 24 from `5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`, the same
binary) with the same drivers and stores, the fixture copied to
`/tmp/gitturtle-evidence/theme-fixture` at the same HEAD, and each launch's Git
identity set to GitTurtle QA `<qa@example.invalid>`, so that no account's path,
name or host shows; these are the committed files. Every driver's checks matched
the first run's, and a cross-correlation scan for the earlier path, name and
host text finds none in them. Seventeen differ from the first run, all at
1440x900. In `compact-1440x900-01-history.png` the path and the identity button
in the title bar and the status-bar timing digits change. In the other sixteen,
every 1440x900 capture but `store32-compact-1440x900-01-list-density.png`, the
Settings page's Git identity card is shorter because the neutral path and
identity each fit one line. The other nine are pixel-identical, and outside
those regions `text11-` and `text12-1440x900-01` differ in eight isolated pixels
of channel delta 1. This re-take also replaces `compact-1440x900-01-history.png`
and `editor-1440x900-01-preview-card.png`, whose committed files showed the
capturing account's path and name. macOS, scale factors other than 1 and the
accessibility tree, which AT-SPI does not expose on this host, remain unchecked.

## September 19 Theme export and import

Native QA for `themes-import-export`, which adds **Export…** to each custom
theme row and **Import…** to the Settings › Your themes card, writing and
reading a `gitturtle-theme` JSON document through the platform's own save and
open dialogs. Linux/XWayland (GNOME 46.0 on Wayland, `DISPLAY=:1`,
`WAYLAND_DISPLAY` unset, `GPUI_X11_SCALE_FACTOR` 1), windows 1000x680, the app's
`window_min_size`, and 1440x900, with an absolute throwaway `XDG_CONFIG_HOME`
per launch seeded as a version-6 store. Fixture: `scripts/create-demo-repo.py`
at HEAD `52f471a1137c617fd8e36db2e6251a18f58c23eb`, unmodified and clean
afterwards; no network action was taken, and the only files written outside the
throwaway stores were the theme documents the test chose itself.

The captures were taken from build `301d82af8d1cd06fa8a1d7ded1892e89ffd1a4e0`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`84d80a41545fe6e40318cb5cedb773f187eaa28c60756523ff4e2d858f39acd2`). As with the
editor entry above, evidence committed to a repository cannot describe the commit
that contains it, so this entry names the revision under test; the build that
ships export and import reuses these captures only when it renders them
identically, which the evidence driver re-checks pixel for pixel. Two captures,
`transfer-1000x680-07-exported.png` and `transfer-1440x900-07-exported.png`,
quote the absolute path they wrote, so that re-check has to pass the same output
directories it used here, and in fact they did not: the shipping revision clamps that
path, so the notice became one line instead of two and the card 134 px instead of
153. **Those two frames were therefore re-taken from the shipping revision
`ea70c6a169882433c3b9659db5b391187c1488a1` and are the only two here not from the
revision named above**; the other 24 artifacts are byte-identical between the two
builds, which is what the re-check established.

Five launches, 52 checks passed and none failed. Export, an Escape-cancelled
export, a delete, an import of that document, two name collisions, an
Escape-cancelled import, nine refusals and the unknown-base notice were each
exercised at 1000x680 and the central ones again at 1440x900. Verified from the
preference store's own bytes rather than from the screen: the exported document
is 690 bytes, sha256
`c4c8271ed6eca156074fea6eb8b65f7b22ef6a078b247a3b6e4fe0fbd32b9ad8`, carrying
exactly the keys `format`, `version`, `name`, `base`, `tokens` and exactly the 21
snake_case token names in spec order, byte-identical from both window sizes and
equal to the stored theme; every refusal and every cancelled dialog left the
store byte-identical; the selection stayed `"midnight"` through all imports, with
the page colour unchanged, which is what "added, not applied" means; the
collisions saved `Harbor Dusk (imported)` then `Harbor Dusk (imported) (2)`; and
a document naming a base this build does not know was stored against the
`daylight` fallback with the notice the spec requires. Latency was dominated by
the portal, not the app: 1.25 s from the keystroke to a visible save dialog,
0.599 s from accepting it to the written-path report, and 1.228–1.244 s from
accepting the open dialog to the message across ten imports and refusals,
including a 70 KiB file refused on size.

**On the dialogs themselves this record is deliberately not a screenshot.**
`prompt_for_new_path` and `prompt_for_paths` reach a real
`xdg-desktop-portal-gnome` dialog here, which is a Wayland window of the
compositor: `org.gnome.Shell.Screenshot.ScreenshotArea` and
`Introspect.GetWindows` both refuse, and XTest cannot drive it. The dialogs are
therefore recorded as D-Bus transcripts and AT-SPI reads — `SaveFile` with
`current_name` `harbor-dusk.gitturtle-theme.json`, `OpenFile` with
`directory false`, `multiple false` and the app's own `Import theme` accept
label, `Response(0, uris)` on acceptance and `Response(2)` on Escape — which
establishes the spec's properties on the wire rather than by reading a picture,
and leaves the dialog's *appearance* unrecorded. That appearance belongs to the
portal backend rather than to GitTurtle. The app's own surfaces, where the
messages live, are captured normally. Because the portal works on this host, the
spec's guidance branch was exercised separately on a private session bus with no
FileChooser service, where both actions reported the picker guidance and changed
nothing.

Two limits of this record, and two defects found on the revision under test.
GPUI does not register with AT-SPI on this desktop, so the accessible names of
the new controls rest on the `#[gpui::test]` assertions rather than on anything
observed; and on Linux the portal navigates into a folder instead of returning
it, so the folder refusal cannot be reached. The defects: nothing bounds the
document text interpolated into the card's message, so a document that is valid
except for a ~64 KiB unknown `base` imports successfully and leaves about 64,700
characters in the notice until the next card action — the app stays responsive
(first repaint 0.582–0.600 s, 0.12–0.13 s of CPU, one Tab repainting in
0.030–0.065 s) but the card's lower border and the settings below it are pushed
off screen, the page growing from 30 wheel steps to 145; and the row's
**Export…** tooltip never appears, at either window size and after 5.2 s, while
the header's **Import…** tooltip appears in the same launch. Both are required to
be fixed in the revision that ships, together with two `docs/user-guide.md`
inaccuracies found here — the suggested file name is a slug of the theme name
rather than the name itself, and the promised folder refusal cannot occur on
Linux. None of those fixes changes a resting frame, which is why these captures
can still describe the shipping build; the pixel re-check against this set is
what establishes that, and it is a precondition of the attestation. The
measurements behind the defects are retained outside the repository with the rest
of the bundle.

Confirmed on September 20: the revision that shipped (`5637cec`) fixed both
defects. Every interpolated fragment is clamped at the source (64 characters for
a key, value or name, 48 for a quoted base) and the message to two lines that end
in an ellipsis when cut, and the row's **Export…** tooltip appears at 2.2 s and
5.2 s at both window sizes. The pixel re-check of that build reproduced all 26
committed artifacts byte-identically with 64 PASS / 0 FAIL, and the long-name
cases (a 64-character collision, an unknown base, and both in one import) read to
their end at both sizes. The two `docs/user-guide.md` corrections landed in the
same revision.

Re-taken on September 24: the 25 captures were replaced by captures of build
`5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (GitTurtle 0.1.0, `source_tree`
clean, release, `x86_64-unknown-linux-gnu`, rustc 1.98.0 (88d9e12ae 2026-08-18),
binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`), on the same
host and fixture; `transfer-1000x680-exported-document.json` stays, because the
exported document was byte-identical from both sizes on every build compared.
The drivers follow the later Settings page: their wheel counts and the Tab count
to **New theme…** changed with the picker build `4cdd4df`. Seventeen captures
predate that build, the virtualized 30 px Your themes list and the settings-view
build, and no follow-up before 20 changed them: `transfer-1000x680-11`, `-13` to
`-24`, `transfer-1440x900-13` and `-21`, `collision2-1000x680-02` and
`bound-1000x680-00`, whose list shows rows 3 to 10 because the driver's page
wheel lands over the card on every build. Follow-up 13's rings changed
`transfer-1000x680-06`, `-07`, `-08` and `-25` and `transfer-1440x900-07` and
`-25`; the two `-07` captures quote a path under the run's own throwaway home,
shortened where the message is built. Follow-up 18 changed
`noportal-1000x680-01` and `-03`: the guidance comes first and the service's
quoted error follows on its own line in muted text. Follow-up 20 changed every
capture except `bound-1000x680-00`: the dark built-in cards at the top are drawn
in the nudged palettes, and in the `-25` captures Aurora Light counts 53
readability findings instead of 52 under the new warning-message rule. Each
difference was attributed in the same session against the build without the
change: `fbc4555` against `e73be2e` for
[follow-ups](development/themes/follow-ups.md) 11 to 17, `fc2a355` against
`fbc4555` for 18, and `5899e2a` against `fc2a355` for 20. Three captures are
new. `list32-1000x680-12-tab-reveals-row32-delete.png` (follow-up 11): Tab to
row 32's **Delete…** at the list's end draws its whole focus ring, clear of the
scrollbar. `bound-1000x680-01-hover-import-disabled.png` (follow-up 12): the
disabled **Import…** tooltip gives the reason and the remedy, "Delete one before
importing another." `lotus-1000x680-01-refuse-notjson.png` (follow-up 20): a
refused import's failure line in Kanagawa Lotus, `#8B4C00` on its subtle surface
at 4.89:1, which is also its rendered peak at 1x (3.95:1 on `fc2a355`); it is
the first committed failure line in a light theme. Each transfer run passed 23
checks with one NOTE (the folder refusal, which this portal cannot reach);
noportal passed 2 of 2, collision2 1 of 1 and list32 7 with 2 NOTE. The bound
run passed 2 and failed 1 on both `5899e2a` and `fc2a355`: "the click leaves the
page as it was" allows a 300 px change and follow-up 12's tooltip is 415 px
wide, a harness threshold. `manifest.txt` lists the re-taken files;
`import-export-verification.txt` still records the `301d82a` run. Linux/XWayland
at scale 1 only.

The captures in this entry, the new ones included, were then taken again on
September 24 from `5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`, the same
binary) with the same drivers and stores, the fixture copied to
`/tmp/gitturtle-evidence/theme-fixture` at the same HEAD, and each launch's Git
identity set to GitTurtle QA `<qa@example.invalid>`, so that no account's path,
name or host shows; these are the committed files. Every driver's checks matched
the first run's, and a cross-correlation scan for the earlier path, name and
host text finds none in them. Four differ from the first run, the
`transfer-1440x900` captures, and only in the Settings page's Git identity card,
which is shorter because the neutral path and identity each fit one line; the
other 24 are pixel-identical. A first attempt of these runs with its output
under `/tmp` failed its imports, for a reason not established, and was
superseded by the reruns these captures come from. macOS, scale factors other
than 1 and the accessibility tree, which AT-SPI does not expose on this host,
remain unchecked.

## September 19 Custom theme editor

Native QA for `themes-editor`, which adds Settings › Your themes (New theme…,
Edit…, Delete…) and the New theme / Edit theme dialog with a live preview.
Linux/XWayland (GNOME on Wayland, `DISPLAY=:1`, `WAYLAND_DISPLAY` unset,
`GPUI_X11_SCALE_FACTOR` 1), windows 1000x680, which is the app's
`window_min_size` and gives the stacked 640 px dialog, and 1440x900, which gives
the 1,000 px two-column dialog, with an absolute throwaway `XDG_CONFIG_HOME` per
launch seeded as a version-6 store. Fixture: `scripts/create-demo-repo.py` at
HEAD `52f471a1137c617fd8e36db2e6251a18f58c23eb`; the editor writes only app
preferences, nothing was written to the fixture and no network action was taken.

The captures were taken from build `3927b57bbb913e35ee4a8b48f12b5c7eaa19f686`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`1faff02061c9f07500cbf74827ae61d425d145caef6a153682230ef239bbe593`). Evidence
committed to a repository can never describe the commit that contains it, so
this entry names the revision under test; a later build that ships the editor
reuses it only when it renders the same captures, which the evidence driver
re-captures for a pixel comparison. Earlier builds of the same patch were
exercised first: in `a6cc84d` Return anywhere in the dialog saved, Keep colors
lost keyboard focus and focus moved to rows out of view; `bb9f332` fixed those
but squeezed the token rows to about 20 px, left the focused Readability list
out of view at 1000x680 and deleted the theme on Return in the Delete
confirmation's Cancel; `30e41b8` fixed those but opened the Delete confirmation
with keyboard focus on the title bar's Menu button behind it, scrolled the
focused Readability list into view at 1000x680 only on the next input event,
and returned focus to New theme… after a delete in only four of eight runs;
`cd563f6` fixed those and passed this flow, but its design review found three
defects: the token column scrolled with no scrollbar and hid the Diff group at
1440x900 and nine of twenty-one tokens at 1000x680 at rest, the 70 px hex field
scrolled the leading `#` out of view once seven characters were typed and kept
it hidden after blur (the driver had masked this by pressing Home before every
read), and an invalid hex value was signalled by the removed-colour outline
alone.

The dialog was operated from the keyboard alone, in a light base (Braden,
stored `daylight`, starting from Porcelain) and a dark base (Midnight, starting
from Graphite) at both sizes; the pointer only wheel-scrolled Settings to the
Your themes card and, after Delete, to the base's picker card. Token rows are
30 px apart with the group labels intact, Name and Base are 28 px tall, Base is
200 px with a menu wider than it and sized to the window, the picker shows a
1 px border-color edge at rest and a 2 px accent ring focused, and the Your
themes row aligns with the card title. The first valid edit recolored the
dialog and the page behind it to exactly the typed canvas in the frame of its
keystroke (no intermediate frame in 51–140 grabs). Two warnings appeared in
each base ("Muted text on Selected 3.9:1, needs 4.5:1"), invalid hex and an
invalid Name were outlined in the removed color while focused and disabled
Save, Save stored the theme in `custom_themes` of a version-6 store and
selected it, Edit… reopened it, Return in a hex field rewrote the value as
lowercase `#rrggbb`, kept the dialog open and saved nothing, Return on Cancel
and Escape restored a frame pixel-identical to the one before the dialog
opened, the Delete confirmation opened with focus on Cancel with Tab contained
and Escape closing it, the focused Readability list was in view at 1000x680 in
the first frame that showed its focus, and Space opened New theme after every
delete: 12 of 12 across 12 launches.

The three defects of `cd563f6` do not reproduce. The token column shows a 6 px
scrollbar thumb in the border color at rest inside a 16 px track at its right
edge in the wide and the stacked layout and in both bases (the track is painted
in the canvas color, like every scrollbar track in the app, so the thumb is what
shows); the track holds nothing but its background and the thumb, and the
focused picker's ring ends before it, 16 px left of where it ended on `cd563f6`.
Tab to the last row scrolls the column to its end with the thumb at the bottom
of the track and the Diff group label and rows painted at both sizes. The hex
field is 78 px: a typed `#rrggbb` shows all seven characters with the caret
after the last one, the value is complete after focus leaves, the typed row's
field is pixel-identical to the same value reopened from the store, and
Return's rewrite shows the full value, all read with no Home press. An invalid
value shows a 14 px rounded square on the removed fill with a contrasting × in
the row's warning slot in both bases at both sizes, replacing the readability
glyph while the value is invalid and staying after blur; a valid value removes
it and restores the readability glyph. The 39 screenshots are under
[`docs/evidence/themes/editor/`](evidence/themes/editor/) with
`flow-verification.txt`, which gives the key sequence and 221 checks, all
passing. A second run of the same binary passed the same 221 checks and
reproduced 133 of 134 frames pixel for pixel, including all 39 committed here;
the one bundle-only frame that differs (the Settings picker after the delete
at 1440x900) does so in one anti-aliased glyph-edge pixel by one level of one
channel.

Not covered: the 32-theme bound, name messages other than a built-in name, Reset
to base, **Replace colors**, a failed or refused save, closing the window with
the editor open, restart persistence, follow-system mode, other text sizes or
density, 2x scale, the picker popover itself, pointer scrolling of the token
column and dragging the thumb, the `theme_apply_frame_ms` budget (the
performance review's), and import and export (the next task); Linux/XWayland
only, with no macOS, native Wayland, packaging or accessibility-label coverage
(the app does not register with AT-SPI on this desktop, so the invalid row's
"value is not #rrggbb" label and the scrollbar's name rest on the `gpui::test`
assertions alone).

Re-taken on September 24: the 39 captures were replaced by captures of build
`5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (GitTurtle 0.1.0, `source_tree`
clean, release, `x86_64-unknown-linux-gnu`, rustc 1.98.0 (88d9e12ae 2026-08-18),
binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`), on the same
host and fixture, with the driver's page wheel and Tab counts moved to the
picker build `4cdd4df`. Fifteen captures predate that build, **Import…** in the
Your themes card header, the virtualized 30 px Your themes list and the
settings-view build, and no follow-up before 20 changed them: the page shows the
grouped picker's 132 px cards and, in the wide dialog, the side column's preview
is the 132 px card with the Readability list following its shorter miniature;
the token column is unchanged. They are `dark-1000x680-16`, `dark-1440x900-04`,
`-05`, `-07`, `-10`, `-11` and `-16`, `fixes-1440x900-05`, `-07` and `-08b`,
`light-1000x680-16` and `light-1440x900-04`, `-05`, `-10` and `-16`. Follow-up
17, which widens the stacked dialog from 640 to 648 px, moved its content 4 px
outward in `dark-1000x680-04`, `-05`, `-10` and `-11`, `fixes-1000x680-02` and
`-04`, and `light-1000x680-04`, `-04b`, `-05`, `-06`, `-10`, `-11` and `-12`.
Follow-up 15 draws the invalid value's mark as a stroked cross that fills its 14
px square, in `dark-1440x900-06` and `light-1000x680-06`. Follow-up 13's rings
changed the Settings page behind or after the dialog in `dark-1000x680-08`,
`-13` and `-17`, `dark-1440x900-08` and `-13`, `light-1000x680-08`, `-13` and
`-15`, and `light-1440x900-08` and `-13`. Follow-up 20 changed all 39: the
built-in cards' captions on the page and, in the Braden-based captures that show
it, the Line number row's value, `#5B6C84` instead of `#61728A`. Each difference
was attributed in the same session against the build without the change:
`fbc4555` against `e73be2e` for [follow-ups](development/themes/follow-ups.md)
11 to 17, and `5899e2a` against `fc2a355` for 20. Three captures are new.
`overflow-1440x900-02-side-column-scrollbar.png` and
`overflow-1440x900-03-side-column-end.png` (follow-up 16): with 25 readability
warnings the wide side column shows the toolkit scrollbar in its own track, and
at its end "Warnings do not prevent saving." sits above the footer.
`text18-1000x680-03-accent-row.png` (follow-up 17): at interface text size 18
the Accent row's description is drawn in full, 24 px clear of its warning slot.
`flow-verification.txt` still records the `3927b57` run. Its 221 checks read
that build's geometry: on `5899e2a` 191 passed and 30 failed, the same list as
on `fbc4555`. Twenty-six of the failures are on the 1000x680 dialog, which is 8
px wider since follow-up 17, and read fixed x positions; with that geometry
moved 4 px, the `fbc4555` run passed 83 of those scenarios' 90 checks, including
every focus, same-frame reveal, invalid-outline and Return check. The 7 left
there and the 4 at 1440x900 test thresholds of the replaced design: the
`cd563f6` picker edge and the old ×'s fill count. Linux/XWayland at scale 1
only.

The captures in this entry, the new ones included, were then taken again on
September 24 from `5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`, the same
binary) with the same drivers and stores, the fixture copied to
`/tmp/gitturtle-evidence/theme-fixture` at the same HEAD, and each launch's Git
identity set to GitTurtle QA `<qa@example.invalid>`, so that no account's path,
name or host shows; these are the committed files. Every driver's checks matched
the first run's, and a cross-correlation scan for the earlier path, name and
host text finds none in them. Twenty-four differ from the first run. The twenty
1440x900 captures differ in the Git identity card of the Settings page behind
the dialog, which is shorter because the neutral path and identity each fit one
line. Outside that card, ten captures differ in 19 isolated pixels of channel
delta 1, antialiasing variance between launches; four of them,
`dark-1000x680-05`, `-10` and `-11` and `light-1000x680-04b`, differ in nothing
else. The other 18 are pixel-identical. macOS, scale factors other than 1 and
the accessibility tree, which AT-SPI does not expose on this host, remain
unchecked.

## September 18 Alucard and Kanagawa built-in themes

Native QA for `themes-batch-alucard-kanagawa`, which adds the Alucard, Kanagawa
Wave and Kanagawa Lotus built-ins. Linux/XWayland (GNOME on Wayland, `DISPLAY=:1`,
`GPUI_X11_SCALE_FACTOR` 1 and 2), window 1000x680, which is the app's
`window_min_size`, with an absolute throwaway `XDG_CONFIG_HOME` per launch.
Fixture: `scripts/create-demo-repo.py` at HEAD
`52f471a1137c617fd8e36db2e6251a18f58c23eb`, plus a disposable copy with a staged
rename, an edit, an untracked file and a deletion for the Compare and Changes
captures; nothing was committed to either and no network action was taken.

The captures were taken from build `e00e864ff8949e2e61d343feecca0561e9784173`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`b97b7393e31e247d26faa1c88088d222cc9539ef59914f56db2132c04ab06cf2`). Evidence
committed to a repository can never describe the commit that contains it, so
this entry names the revision under test; a later build that ships these themes
reuses it only when it renders the same captures.

Each theme was recorded in the Settings picker with its own card scrolled into
view and checkmarked, and in History with a selected row, a hovered row, a
visible accent focus ring, and a diff showing added and removed lines. The
fifteen screenshots are under
[`docs/evidence/themes/alucard-kanagawa/`](evidence/themes/alucard-kanagawa/)
with the per-capture palette check beside them. History for all twenty themes
at 1x and 2x matches every declared token (120 readings), and the seventeen
existing themes are unchanged against their earlier captures.

No reading fell below its rule floor on full-coverage glyph cores. The narrowest
margins are muted text on a hovered selected row at 4.51:1 (Alucard) and 4.52:1
(Kanagawa Lotus) against 4.5, Kanagawa Wave's selected row against panel at
1.153:1 against 1.15, and Kanagawa Lotus's added and removed text on their diff
tiles at 4.51:1. Rendered antialiased text sits below those floors, as recorded
in [the contrast-margins note](development/themes/contrast-margins.md). Not
covered: warning and conflict states, the canvas-label-on-fill rule, the primary
button's hover and pressed states, the picker at 2x, Split, Blame and image
diffs, and follow-system mode; `accent_foreground` is exact on screen at 2x
only; Linux/XWayland only, with no macOS, native Wayland, packaging or
accessibility-label coverage.

Re-taken on September 24: the fifteen captures were replaced by captures of
build `5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (GitTurtle 0.1.0,
`source_tree` clean, release, `x86_64-unknown-linux-gnu`, rustc 1.98.0
(88d9e12ae 2026-08-18), binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`), with the
same flow at 1000x680 and the same fixture, for
[follow-up](development/themes/follow-ups.md) 20. That follow-up moves the tuned
tokens to clear their rules by 0.25 where the family's values allow
([`DESIGN.md`](../DESIGN.md#semantic-palette-ownership),
[contrast margins](development/themes/contrast-margins.md#what-landed)): Alucard's
muted and removed, Kanagawa Wave's added and line numbers, and Kanagawa Lotus's
added, modified, warning, removed tile, hunk and line numbers. Alucard's muted
text on a hovered selected row is now 4.95:1 declared, and Kanagawa Lotus's
added and removed text on their tiles 4.87 and 4.88:1. Lotus's text (4.64) and
secondary text (4.52) on the hovered selected row, its primary-button label
(4.59), and Wave's removed lines (4.58) and renamed icons on the hovered selected
row (3.15) keep their thinner margins as recorded decisions, since their values
are upstream. The History and diff captures differ from the September 18 files
only in the tuned tokens, the fixture path in the title bar and the status-bar
timing digits. The settings-picker captures also show the grouped picker's
132 px cards from the picker build `4cdd4df`, and follow-up 13's check badge,
sized to the name's line with a 1 px ring. The flow's 9 checks passed for each
theme, and each difference from `fc2a355`, taken in the same session, is a tuned
token's value or its antialiased edge. Kanagawa Lotus's warning as text on its
subtle surface is recorded in the export and import entry's
`lotus-1000x680-01-refuse-notjson.png` at 4.89:1 (3.95:1 before).
`palette-verification.txt` still records the September 18 captures of
`e00e864`. The 1x and 2x readings described above were not repeated: this
re-take is at scale 1 only, on Linux/XWayland.

The captures in this entry were then taken again on September 24 from
`5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`, the same
binary) with the same drivers and stores, the fixture copied to
`/tmp/gitturtle-evidence/theme-fixture` at the same HEAD, and each launch's Git
identity set to GitTurtle QA `<qa@example.invalid>`, so that no account's path,
name or host shows; these are the committed files. Every driver's checks matched
the first run's, and a cross-correlation scan for the earlier path, name and
host text finds none in them. The twelve History and diff captures differ from
the first run only in the path and the identity button in the title bar and the
status-bar timing digits. `kanagawa_wave-settings-picker.png` differs in one
pixel of channel delta 1, and the other two settings-picker captures are
pixel-identical. macOS, scale factors other than 1 and the accessibility tree,
which AT-SPI does not expose on this host, remain unchecked.

## September 18 Rosé Pine and Dracula built-in themes

Native QA for `themes-batch-rose-pine-dracula`, which adds the Rosé Pine, Rosé
Pine Dawn and Dracula built-ins. Linux/XWayland (GNOME on Wayland, `DISPLAY=:1`,
`GPUI_X11_SCALE_FACTOR` pinned per launch), window 1000x680, which is the app's
`window_min_size`, with a throwaway `XDG_CONFIG_HOME` per launch. Fixture:
`scripts/create-demo-repo.py` at HEAD `52f471a1137c617fd8e36db2e6251a18f58c23eb`,
plus a disposable copy with changed paths for the Compare and Changes captures;
nothing was committed to either and no network action was taken.

The captures were taken from build `83eaf81ae71bc077abe29c26d10e8f7afa97522b`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`531c6f63acd37dcc897394767a2338209a05af4e0e726de43e171a7d8031ff2a`). Evidence
committed to a repository can never describe the commit that contains it, so
this entry names the revision under test; a later build that ships these themes
reuses it only when its palettes resolve identically to that revision's.

Each theme was recorded in the Settings picker with its own card checkmarked, and
in History with a selected row, a hovered row, a visible accent focus ring, and a
diff showing added and removed lines. The fifteen screenshots are under
[`docs/evidence/themes/rose-pine-dracula/`](evidence/themes/rose-pine-dracula/)
with the per-capture palette check beside them; every sampled surface equals the
token the build declares, resolved from `crates/app/src/appearance.rs` and
`crates/app/src/appearance/sources.rs`.

No reading fell below its rule floor, measured on full-coverage glyph cores, but
these are the narrowest margins of any batch: Rosé Pine's hover against panel is
1.087:1 against a 1.08 floor, and muted text on a hovered selected row is 4.51–4.54:1
against 4.5 in all three themes. Rendered antialiased text sits below those
floors, as recorded in [the contrast-margins note](development/themes/contrast-margins.md).
Not covered: `warning` shares `modified`'s value in all three themes and no
warning or conflict state was reached; `accent_hover` and `accent_active` were
not exercised; `accent_foreground` is exact on screen at 2x only; the picker's
grouping belongs to the picker task; Linux/XWayland only, with no macOS,
packaging or accessibility-label coverage.

Re-taken on September 24: the fifteen captures were replaced by captures of
build `5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (GitTurtle 0.1.0,
`source_tree` clean, release, `x86_64-unknown-linux-gnu`, rustc 1.98.0
(88d9e12ae 2026-08-18), binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`), with the
same flow at 1000x680 and the same fixture, for
[follow-up](development/themes/follow-ups.md) 20. That follow-up moves the tuned
tokens to clear their rules by 0.25
([`DESIGN.md`](../DESIGN.md#semantic-palette-ownership),
[contrast margins](development/themes/contrast-margins.md#what-landed)): Rosé
Pine's muted; Rosé Pine Dawn's muted, added, removed, modified and warning,
renamed and line numbers; and Dracula's muted, removed and line numbers. Rosé
Pine's hover against panel stays at 1.087:1 as a recorded decision, because both
of its values are upstream. The History and diff captures differ from the
September 18 files only in the tuned tokens, the fixture path in the title bar
and the status-bar timing digits. The settings-picker captures also show the
grouped picker's 132 px cards from the picker build `4cdd4df`, and follow-up
13's check badge, sized to the name's line with a 1 px ring. The flow's 9 checks
passed for each theme, and each difference from `fc2a355`, taken in the same
session, is a tuned token's value or its antialiased edge. Measured at 1x in
separate 1480x980 launches with the September 18 method, muted text on the
hovered selected row now peaks at 4.62 (Rosé Pine), 4.62 (Rosé Pine Dawn) and
4.58:1 (Dracula), against 4.35, 4.28 and 4.31 on `fc2a355`. The dates in that
row peak at 4.22 to 4.35, and the monospaced SHA reaches its declared ratio.
Rosé Pine Dawn's warning as text on its subtle surface reaches its declared
4.89:1 (4.25 before). `palette-verification.txt` still records the September 18
captures of `83eaf81`. Linux/XWayland at scale 1 only.

The captures in this entry were then taken again on September 24 from
`5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`, the same
binary) with the same drivers and stores, the fixture copied to
`/tmp/gitturtle-evidence/theme-fixture` at the same HEAD, and each launch's Git
identity set to GitTurtle QA `<qa@example.invalid>`, so that no account's path,
name or host shows; these are the committed files. Every driver's checks matched
the first run's, and a cross-correlation scan for the earlier path, name and
host text finds none in them. The twelve History and diff captures differ from
the first run only in the path and the identity button in the title bar and the
status-bar timing digits. `rose_pine-settings-picker.png` differs in one pixel
of channel delta 1, and the other two settings-picker captures are
pixel-identical. macOS, scale factors other than 1 and the accessibility tree,
which AT-SPI does not expose on this host, remain unchecked.

## September 18 Solarized and One built-in themes

Native QA for `themes-batch-solarized-one`, which adds the Solarized Dark,
Solarized Light, One Dark and One Light built-ins. Linux/XWayland (GNOME on
Wayland, `DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`), window 1000x680, which is the
app's `window_min_size`. Fixture: `scripts/create-demo-repo.py` at HEAD
`52f471a1137c617fd8e36db2e6251a18f58c23eb`, plus a disposable copy with one
renamed, modified and added path for the Compare and Changes captures; nothing
was committed to either and no network action was taken.

The captures were taken from build `d4ac46bd031d93ab83fa0e549988f1669dc9b4eb`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`4de9f6fa54463ef58f8dfcbe0b754b050d260b750e8aacc5526a170b414dd332`). Evidence
committed to a repository can never describe the commit that contains it, so
that revision is the one under test and this entry names it; the shipped
palettes are byte-identical to the ones photographed.

Each theme was recorded in the Settings picker with its own card checkmarked, and
in History with a selected row, a hovered row, a visible accent focus ring, and a
diff showing added and removed lines. The twenty screenshots are under
[`docs/evidence/themes/solarized-one/`](evidence/themes/solarized-one/) with the
per-capture palette check beside them; every sampled surface equals the token the
build declares, resolved from `crates/app/src/appearance.rs` and
`crates/app/src/appearance/sources.rs`.

No reading fell below its rule floor. The least headroom is the Solarized Dark
focus ring at 3.03:1 against the field fill it is painted on, against a 3:1
minimum. Not covered: `warning` shares `modified`'s value in all four themes and
no warning or conflict state was reached, so it is unverified as a distinct
token; `accent_foreground` is confirmed on screen at 2x and passes its declared
pair at 1x, where a small button label never reaches full glyph coverage; the
picker's grouping and breakpoints belong to the picker task; Linux/XWayland only,
with no macOS, packaging or accessibility-label coverage.

Re-taken on September 24: the twenty captures were replaced by captures of
build `5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (GitTurtle 0.1.0,
`source_tree` clean, release, `x86_64-unknown-linux-gnu`, rustc 1.98.0
(88d9e12ae 2026-08-18), binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`), with the
same flow at 1000x680 and the same fixture, for
[follow-up](development/themes/follow-ups.md) 20. That follow-up moves the tuned
tokens to clear their rules by 0.25
([`DESIGN.md`](../DESIGN.md#semantic-palette-ownership),
[contrast margins](development/themes/contrast-margins.md#what-landed)), and it
tunes three upstream colors as well: Solarized Dark's yellow (`#B58900` →
`#BE9209`, for modified and warning), One Light's orange-1 (`#986801` →
`#8E5E00`, for modified and warning) and One Dark's mono-1 text (`#ABB2BF` →
`#AEB5C2`), so that One Dark's secondary text stays at or below its body text.
The History and diff captures differ from the September 18 files only in the
tuned tokens, the fixture path in the title bar and the status-bar timing
digits. The settings-picker captures also show the grouped picker's 132 px
cards from the picker build `4cdd4df`, and follow-up 13's check badge, sized to
the name's line with a 1 px ring. The flow's 9 checks passed for each theme,
and each difference from `fc2a355`, taken in the same session, is a tuned
token's value or its antialiased edge. Measured at 1x in separate 1480x980
launches with the September 18 method, muted text on the hovered selected row
now peaks at 4.58 (Solarized Dark), 4.63 (Solarized Light), 4.57 (One Dark) and
4.68:1 (One Light), against 4.39, 4.32, 4.42 and 4.29 on `fc2a355`. The dates in
that row peak at 4.23 to 4.32, and the monospaced SHA reaches its declared
ratio. Warning text on the subtle surface reaches its declared 4.84 (Solarized
Dark), 4.91 (Solarized Light) and 4.92:1 (One Light), against 4.34, 4.38 and
4.27, so the warning and conflict states this entry left unverified now have a
failure-line reading in three of these themes; no committed frame shows it.
`palette-verification.txt` still records the September 18 captures of
`d4ac46b`. Linux/XWayland at scale 1 only.

The captures in this entry were then taken again on September 24 from
`5899e2ac1744bb35bc4e70c3d4a0c8b8420c08ea` (binary sha256
`8e62cfe1f8284eb7e7e4d01bbbc77b1e14af3b36b666fcbc695ecf9e0478fcdc`, the same
binary) with the same drivers and stores, the fixture copied to
`/tmp/gitturtle-evidence/theme-fixture` at the same HEAD, and each launch's Git
identity set to GitTurtle QA `<qa@example.invalid>`, so that no account's path,
name or host shows; these are the committed files. Every driver's checks matched
the first run's, and a cross-correlation scan for the earlier path, name and
host text finds none in them. The sixteen History and diff captures differ from
the first run only in the path and the identity button in the title bar and the
status-bar timing digits. The four settings-picker captures are pixel-identical.
macOS, scale factors other than 1 and the accessibility tree, which AT-SPI does
not expose on this host, remain unchecked.

## September 16 project list pane

PR #22 adds an optional project list pane, a saved project library with nested
user-named groups, and the **Show the project list** setting. Its initial
revision (`f075c04` on main `7cd3744`) passed the Rust gates and an Xvfb
session on the contributor's Linux host. The review revision reworked the pane
into a keyboard tree that shares the History navigator's rows and bindings,
sorted presentation, hover/right-click/Shift-F10 actions with a shared menu,
disabled impossible move destinations, an in-pane error strip with Retry, a
pending-save guard against stale replies, cached presentation rows, and a View
menu / palette toggle. `cargo fmt --all -- --check`, `cargo clippy --locked
--workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`
passed on this Linux host for the working tree over `f075c04` (debug profile;
app 456 passed with two existing ignores, preview 121 passed with one ignore,
core 32 of 33). The one core failure, the configured-askpass fixture, fails
identically on untouched main in this host's Git 2.43 environment and passed in
the PR's hosted Ubuntu and macOS runs, so it is environmental and unrelated.

A debug build of that working tree ran under XWayland on a GNOME Wayland
desktop (Pop!_OS, Linux 7.1.5, window 1480x980 logical at about 2.17 scale)
against three disposable `scripts/create-demo-repo.py` fixtures plus a second
copy named `alpha`, with `XDG_CONFIG_HOME` in a scratch directory. Pointer and
key input came from a local XTest helper. Verified by screenshot and by reading
the store: pane rows match the navigator's 30 px geometry beside it; groups
come first and each level is sorted by name; the two `alpha` projects show their
parent folders; the open project keeps the selected surface and accent text;
hovering a row reveals its actions control; the group menu omits the group's
own subgroups and checks its current place, and the project menu checks its
group; a right-click opens the same menu; Down moves the cursor marker,
Shift-F10 opens the menu under the cursor row, Down selects an item, Return
runs **Open project** and marks `beta`, and focus returns to the tree; **+**
opens the group dialog with the Rename project layout; Settings shows the pane
and the switch; the palette lists **Show or hide the project list**; replacing
the store with a directory made a collapse fail with the explanation and Retry
inside the pane while the collapse stayed on screen, and Retry after restoring
the file saved `collapsed: true`.

Limitations: debug profile only, so no timing claim. The compositor refused a
programmatic resize, so the 1000x680 clamp has automated coverage only. macOS,
native Wayland, a physical pointer session, screen readers, package builds, and
theme, density and text-size variations of the pane were not exercised. Group
and project removal, the depth and count limits, the stale-reply guard and the
refusal of a malformed saved list have automated coverage only.

## September 16 PR #17 file-discard review

Clean application source `e9b631f` includes main `5466b8a` and passed 847
workspace tests (five existing ignores), strict Clippy, formatting and native
debug compilation. The [discard validation record](discard-validation.md)
documents four preservation fixes, independent general/security review,
bounded destructive confirmation and actual Linux/X11 pointer/keyboard checks.
Native evidence covers stale content, rename/add/delete, untracked deletion,
conflict and directory-transition refusal, long paths, enlarged light UI,
dark UI, cancellation and unrelated index/worktree/ref preservation. macOS
native, physical desktop and hosted CI evidence remain separately scoped.

## September 16 PR #13 project-name review

The [project-name validation record](project-names-validation.md) covers main
integration, independent review fixes, 816 passing workspace tests (five existing
ignores), strict Clippy and real native Linux/X11 checks of clean source
`2c98108`. Screenshots cover dark/default and minimum-size light/enlarged text,
inline save failure/retry, focused editing, canceled edits, canonical aliases,
long names and restoring the folder label. macOS native interaction, screen
readers and installed-package checks remain outside this evidence.

## September 16 PR #15 force-removal review

Clean source `c3e4a0f` includes main `7c9dc6c` and passed 803 workspace tests
(five existing ignores), strict workspace Clippy, formatting and native debug
compilation. The [force-removal validation record](force-worktree-removal-validation.md)
documents preservation regressions, independent review, accessible destructive
confirmation, and real GPUI interaction on virtual Linux X11. Native checks
covered minimum-size enlarged light UI, dark UI, cancellation, nested-repository
refusal, stale ignored-content refusal and successful removal with independent
branch/index/sibling preservation. macOS, physical desktop and hosted-CI results
remain separate from this local evidence.

## Commit-inspector validation requirements

The persistent inspector requires candidate-bound native evidence in addition to
its consuming Rust layout/state tests. Exercise short/empty/Unicode/trailer
messages, long titles, many paragraphs, near-limit unbroken text and merge
parents. Check both densities and light/dark themes at minimum window size,
280-point inspector width and enlarged interface text. Confirm full-message/hash
copy, keyboard scrolling, selected changed-file visibility, rapid commit
selection, Compare/Back, nested File History, tab return and Projects/Settings.
Native records must identify the exact binary and platform; source tests do not
establish Linux/macOS runtime quality. The initiative's user-deferred macOS
verification remains open until that evidence is supplied.

## September 15 commit-inspector and Linux package checks

Clean release `eebf47a` exercised the persistent message inspector and an actual
Linux archive/install/relaunch in virtual Ubuntu 24.04 X11. The
[dated evidence](benchmarks/2026-09-15-commit-inspector.md) records layout, Unicode,
large-message copying, native accessibility, merge parents, retained navigation,
package digests and installer recovery. C0 license clearance, hosted artifact
transfer and user-owned macOS/Apple verification remain open. Later source and
fixture commits do not turn this into evidence for another executable.

## September 15 PR #5 macOS merge review

Source `0d4d8ce` passed 771 workspace tests, strict Clippy, formatting, app check
and release compilation. The [worktree-removal review](worktree-removal-validation.md#september-15-macos-merge-review-and-security-fixes)
records CodeQL flow fixes, explicit fixture copies, verified local macOS package
identity, native cancellation/refusal/removal checks, and independent branch,
index and sibling-content preservation. Hosted results are recorded on PR #5;
this entry does not claim a new Linux native or notarized-distribution pass.

## September 15 PR #5 maintainer review

Clean release source `3b0a958` passed 775 workspace tests (five existing ignores),
formatting, app check, strict workspace Clippy and release compilation. The
[maintainer validation record](worktree-removal-validation.md#september-15-maintainer-review-of-pr-5)
documents stale-review cancellation, checkout/admin identity guards, the Ubuntu
draft-shutdown fixture correction, and native Wayland checks for protected
targets, cancellation, fresh review, cleanup and branch retention. macOS native
interaction and hosted checks are scoped separately from this local evidence.

## September 14 guarded worktree removal

Implementation commits `95d97a7` and `9585271` add navigator removal actions,
keyboard access, hidden-change/lock guards and verified cleanup. The
[worktree removal validation record](worktree-removal-validation.md) documents
743 passing workspace tests (five intentionally ignored), strict Clippy, and
native Linux/X11 checks of protected targets, cancellation, stale review,
successful Git/filesystem cleanup and branch retention. macOS runtime and
hosted CI are separate from this local evidence.

## September 14 PR #4 desktop text and security validation

Clean application source `0700984951001289a6c7490f81e72b749cd4120e` was built,
packaged and installed as Linux x86-64 release 0.1.0, using Rust 1.98.0 and the
existing Ubuntu 24.04 build environment. The executable SHA-256 is
`ba54f643d8262c409715933c165bda0c91e052d70cd0a0d8f42eb3dc1e4b3a2c`.
Subsequent validation-record edits do not change that executable's source identity.

Formatting, `cargo check --locked -p gitturtle`,
`cargo test --locked --workspace` (**756 passed, zero failed, five explicit
ignores**), strict workspace Clippy with all targets, and release compilation
passed. Focused checks exercised observer cancellation and bounds, the repeated
asynchronous history fixture, actual rendered editor/list metrics, two-window
notifications, retained tabs, nested lists and passive-refresh anchors. The
[security review](pr4-security-review.md) accounts for every initial CodeQL alert,
the four reproduced tooling/fixture defects and the new defensive-check finding.
Fresh hosted checks and merged-main alert state are tracked separately in
[PR #4](https://github.com/FernandoX7/GitTurtle/pull/4).

Native interaction used the same release executable, disposable two-commit
repositories with a 600-line source file, multiple changed ranges, an inserted
alignment row and long lines. A native Wayland window ran under nested Weston
with software graphics and the host's real GNOME settings portal. Its desktop
lacks the newer `font-rendering` key, exercising that fallback. Screenshots and
scroll traces verified:

- Live 100%, 125% and 150% text sizes retained the selected source and logical
  viewport. Both split editors kept row 546 with offsets 9828, 12558 and 14742
  pixels for measured line heights 18, 23 and 27 pixels. Unified view retained
  patch row 150 through the fractional change.
- Find query, match, source selection and keyboard focus survived live changes;
  a retained repository tab reopened at the same logical row after a hidden
  size change. Literal source copying excluded gutters, and typing into the
  read-only source left the fixture unchanged.
- Code/gutter wheel input, reversal, horizontal scrolling and Back to the
  selected history context worked. Matching pane offsets and visible row ranges
  were checked in the rendered frames.
- Changing grayscale to `rgba` repainted the glyphs. `none` retained grayscale,
  matching the documented toolkit limitation. Original desktop preferences were
  restored after testing.

A separate native X11 window under Xvfb kept 18-pixel source lines at a portal
text factor of 150%, confirming no extra portal multiplier on that backend.
This is virtual X11 and nested Wayland evidence, not a physical-display,
Ubuntu-desktop, KDE, mixed-DPI or native macOS acceptance run. It establishes
neither a frame-rate improvement nor physical-panel sharpness. The cold split
regression verifies a retained initial-row request resolves after a measured
editor notification; it does not establish positioning on the first paint alone.

All **11 Linux installer tests** passed with the final real bundle, including
relocation and rollback. The installed executable matched the validated hash;
all three genuine configuration files remained byte-identical across installation,
and all four repository tabs retained their order and selection on restart.
The previous executable and all 1,228 recovery entries passed checksum/ownership
validation. Recovery uses the installed script's `--rollback` option described in
[the Linux runbook](linux.md). Existing public-binary notice gaps remain documented;
this was a local installation. Private captures and machine/repository details
are not part of the public evidence.

## September 14 refresh, history and split-diff preview

Source `417b5e8` was installed for this earlier Linux x86-64 release 0.1.0 Preview session, binary
SHA-256 `45606c5a195d1696096b93ea0fa3a38b67b025990ea794f25f91d4211e5729c2`.
The [native investigation and final acceptance](benchmarks/2026-09-14-native-preview.md)
record physical GNOME/Wayland scale-2 code/gutter scrolling, long lines,
Find/selection/copy, Show latest, build diagnostics and recoverable installation.
Final code and gutter runs had zero mismatched pane offsets; the evidence does
not establish a general frame-rate improvement. Supplemental native X11 checks
cover disposable stage/commit/View commit/fetch/pull/push and a linked-worktree
picker open. All genuine tabs and preferences survived the host upgrade.

The [refresh investigation](benchmarks/2026-09-14-refresh-reliability.md)
documents the reproduced directory limit, disappearing-directory handling,
ignore/tracked policy, linked-worktree roots and the native sibling-event
regression. Final workspace tests passed 729 tests with five explicit ignores;
strict Clippy and release build passed. Public binary notice gaps, untested
platforms and compositor/scale limits remain explicit. The separate static
[website](../website/README.md) has current native captures and a Cloudflare
deployment plan; no public deployment or DNS change is implied.

## September 15 client-side project names

Working-tree source (branch `fix/linux-watch-skip-ignored`, on `4dfc334`) adds a
client-only project name that replaces a project's folder name in the project
hub, repository tabs and their menu, the repository heading, saved workspaces and
operation status, while leaving the folder, the repository and Git configuration
untouched. Names are stored in the existing preferences file under a new
version-4 `project_names` section, read through the shared bounded store reader
and written through the serialized preference executor.

`cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --
-D warnings` and `cargo test --locked --workspace` passed on this Linux host
(debug profile; 351 app tests including the new preference round-trip, bound and
hub-interaction cases).

A debug build was exercised in a virtual X11 session (Xvfb `:77`, 1600x1000)
against a disposable `scripts/create-demo-repo.py` fixture, with
`XDG_CONFIG_HOME` pointed at a scratch directory so no developer preferences were
touched. Verified by screenshot and by reading the store: the hub shows a chosen
name over the folder name and location; the rename dialog opens from the recent
row and from **Workspaces → Rename current project…**; saving writes
`project_names` and updates the hub, tab strip, repository heading and tab menu;
submitting an empty field removes the entry and restores the folder name
everywhere; the fixture directory keeps its own name throughout.

Limitations: debug profile only, so no timing claim. X11 pointer and key input
came from a local XTest helper rather than a desktop session, and macOS, Wayland,
VoiceOver/Orca, theme and density variations for the new dialog were not
exercised.

## September 14 public-launch preparation

Application commit `7d18fef` adds a visible Linux Menu and grouped searchable
shortcut help, with shared bindings and platform labels. Release build,
formatting, locked app check, locked workspace tests and strict workspace Clippy
passed in the existing Ubuntu userspace with reused caches. Virtual X11 and
native Wayland under nested Weston verified menus, keyboard search, focus/draft
retention, contextual disabling, narrow/enlarged layouts and 2× window controls.

Package commit `326fa0f` adds collected third-party notices, source obligations
and installed notice retention. Isolated Linux installation, relocation,
reinstallation, full notice checksums and rejection of corrupted/unchecked notices
passed. The two Linux and six macOS notice gaps remain public-binary release
prerequisites. macOS packaging received syntax/inventory checks only.

The [launch validation record](public-launch-validation.md) contains exact
commands/results, executable/source hashes, genuine README screenshots and
platform boundaries. This is not a new clean build, actual Ubuntu desktop pass,
or current native macOS pass. The corrected workflow started hosted jobs; a
completed hosted pass remains separately verifiable. The
[publication checklist](public-launch.md) covers privacy and owner decisions.

## September 14 Ubuntu teammate readiness

App source `6824c7d` and packaging source `d482a3f` add a pinned Rust toolchain,
a relocatable Linux user-local bundle/installer, complete build/runtime setup,
and visible no-display/picker-failure diagnostics. The [teammate runbook](linux.md)
includes existing feature limits and the remaining Ubuntu desktop checklist.

A verified Ubuntu Base 24.04.5 x86-64 root built the locked release from fresh
Cargo caches without host libraries or the local linker workaround. Formatting,
`cargo check`, **694 workspace tests (five intentionally ignored)** and strict
all-target Clippy passed. A separate pristine runtime root passed installation,
relocation/reinstallation, executable/ELF, launcher metadata, eight icon sizes,
and useful negative/headless checks with only the documented runtime packages.

The installed executable SHA-256 is
`3c39630d691de172ee8302ab0e8bf30976dbc9cd80c957919f357ff230c1af75`.
Virtual X11/Openbox and nested Wayland/Weston screenshots and native input
verified representative History/Compare/Back, text/PNG/SVG/Markdown/GLB previews,
whole-file stage/unstage, draft/session restoration, quitting and integer 2×
rendering. The physical Pop!_OS GNOME/Wayland probe established launch and
native picker exposure only; its automation could not verify later actions.

[Detailed evidence, failures, exact boundaries and screenshots](benchmarks/linux-ubuntu-20260914/README.md)
separate the clean build, runtime-only, headless, virtual native and physical-host
checks. Rootless package ownership needed a provisioning-only fakeroot repair;
GUI portal PID namespaces needed adjustment. Successful directory selection,
actual Ubuntu GNOME shell/driver integration, fractional/mixed-monitor scaling,
macOS runtime and hosted CI remain unverified. The roots shared the host kernel;
this is not evidence of a complete Ubuntu desktop installation.

Final host-created archive extraction exposed unmapped builder ownership in
the rootless runtime namespace. Neutral numeric archive ownership fixed it;
ordinary extraction and the isolated install/headless checks passed again.
The runbook also pins the manual Cargo target directory to match packaging.

## September 14 Linux icon and window-control correction

Source `f3bf253` corrects the user-reported missing launcher icon and absent
Linux window controls. The initial icon installation used a
`hicolor/1024x1024/apps` directory absent from this desktop's theme index;
GTK failed to resolve the icon by name at every checked size. The installed
desktop entry now references the unchanged PNG by absolute path. Its
`Gio.DesktopAppInfo` icon resolved to that file, GTK successfully decoded it
at 32, 48, 128, 256 and 512 pixels, and `desktop-file-validate` passed.

The Linux tab strip now supplies standard window controls when GPUI reports
client decorations, following the desktop's left/right button order and the
compositor's supported actions. Server decorations and macOS traffic lights
remain native. The existing close action and shutdown observers are reused;
the keyboard help now lists the existing Ctrl+Q / Cmd+Q shortcut. The
[Linux guide](linux.md#window-controls-and-quitting) records the platform
conventions and corrected installation recipe.

On the same Pop!_OS/GNOME Wayland machine, Rust 1.98.0 formatting, the locked
release build, locked workspace tests and strict all-target workspace Clippy
passed. The local linker setup is
the same as the initial startup check below. The installed release SHA-256 is
`9955debcfab6487f21e54f3c74c48e02cda91d51221324e34e72f650bc76d6fd`;
all 647 recorded source/manifest/asset inputs remained unchanged through the
build. Installation used an atomic executable replacement.

The user confirmed Ctrl+Q closed the preceding build. The new installed
build opened the disposable demo repository with isolated application
preferences. Enabling the session's accessibility bridge before launch made
its full native tree available: it exposed Minimize, Maximize and Close
buttons, and semantic activation of Maximize changed the control's label to
Restore. A following automated Restore activation did not establish a state
change; tool delivery alone is not recorded as a successful interaction.
The user then confirmed that the icon and controls worked after being asked
to maximize, restore and close the window. Separately, semantic activation
of the new Close button terminated the installed process and the session
file was saved at shutdown.

The temporary accessibility bridge setting was restored to its original
disabled value, including GNOME's `toolkit-accessibility` preference; the
screen reader remained disabled throughout. The disposable repository
remained clean. The application launcher then reopened the installed build
with the user's normal preferences/session, without the QA environment.
macOS runtime checks were unavailable on this Linux machine; no new macOS
native pass is claimed. Automated drag, minimize and alternate desktop
button-layout gestures remain outside this check.

## September 14 Linux installation and Wayland startup

Source `4ad8e27352c1f33cecd145eac866ee5ff9c33e26` built and launched on
Pop!_OS 24.04 LTS, x86-64, kernel `7.1.5-76070105-generic`, with a GNOME
Wayland session. Rust `1.98.0 (88d9e12ae 2026-08-18)` was installed alongside
the existing default toolchain. Vulkan enumerated Intel Graphics (ARL), an
NVIDIA GeForce RTX 5090 Laptop GPU and llvmpipe; the app's selected adapter
was not established.

`cargo +1.98.0 fmt --all -- --check` passed. The first
`cargo +1.98.0 build --release --locked -p gitturtle` reached linking and
failed on missing `-lxkbcommon-x11`. The distro runtime
`libxkbcommon-x11.so.0` was already installed, but its unversioned development
link was absent. A local `target/linux-native-lib/libxkbcommon-x11.so` link
to that system library, supplied through `LIBRARY_PATH`, allowed the same
release command to pass. No Rust source, lockfile, system package or global
toolchain default was changed. Installing the documented development packages
is the normal setup; see the [Linux guide](linux.md).

The release binary and installed `~/.local/bin/gitturtle` share SHA-256
`12b19bd7ea4ac98947f11b35d2fcc82187040d2c8a94be37605b06502fa961f6`.
`ldd` resolved every dependency; the ELF records the system library's versioned
SONAME and has no build-directory RPATH. The existing 1024-pixel app icon and
`com.gitturtle.desktop.desktop` application entry were installed for the user.
`desktop-file-validate` passed, the desktop database was updated, and
`gio launch` successfully started the installed executable on the GitTurtle
source repository. The running `/proc` executable matched the installed path.

The initial Wayland run used `scripts/create-demo-repo.py`'s disposable fixture
and isolated `XDG_CONFIG_HOME`. The saved session contained the expected
commit, changed-file selection and History mode. The user confirmed that
the demo history window looked correct and supplied a screenshot. Visual
inspection confirmed readable text, app/control icons, all eight fixture
commits, graph edges, branch navigation, selection, commit details and the
two changed files, with no obvious clipping or rendering failure at the
captured size. Fixture Git status remained clean.
The fixture instance was then stopped and the application launcher opened
the source repository with normal user preferences. Both launches produced
empty stderr/stdout logs during these checks.

This establishes a local release build, installation, repository startup and
visually inspected window appearance. The desktop tool could not discover the
GPUI window, advertised no screenshot capability, and GNOME denied direct
window screenshot access. The visual evidence came from the user's supplied
screenshot; no automated native gesture evidence was obtained. Compare/Back,
keyboard shortcuts, picker behavior, previews,
staging, network operations and accessibility remain unverified on this
desktop. Workspace tests and Clippy were not rerun for this installation-only
task; older Linux test results retain their own source/platform identities.
The [Linux platform limits](linux.md#platform-limits) still apply.

## September 10 GitHub review conversations

The [GitHub review validation record](github-review-validation.md) records native
conversation actions and durable reply recovery, live draft/ready PR creation,
range comments and collected reviews, live reply/resolve/reopen, and stale-head
refusal in the authorized private synthetic repository. Its source/build identities
keep baseline, integrated live, and final correction evidence separate.

## September 10 native GLB previews

The [appearance and animation workflow](glb-workflow-validation.md) records
material/texture revision comparisons, skins, morphs, native playback, quiet
hidden previews and restored app state at source `8908b35`. Its
[independent reference and current corpus](glb-workflow-reference.md) and
[release CPU/native measurements](benchmarks/2026-09-10-glb-workflow.md) distinguish
geometry support, approximate appearance, processing time and frame callbacks.

The preceding [meshopt validation record](meshopt-preview-validation.md) covers
embedded compression, independently verified current corpus coverage, native
comparison refinements and restart behavior. Its [release measurements](benchmarks/2026-09-10-meshopt.md)
separate actual decode/render coverage and CPU timings from native observations.

The [GLB validation record](glb-preview-validation.md) covers bounded static
geometry, faithful revision placement and scale, native camera controls,
History/Working Changes and retained-filter isolation, representative appearance
and window sizes, and restored application state. It identifies the exercised
local macOS packages separately from the [release decoder/raster measurements](benchmarks/2026-09-10-glb.md).
Compression, deformation and omitted appearance remain explicit in the
[finite GLB support contract](interactive-3d.md#glb-20-static-geometry).

## Final September 10 security milestone

[Source `b4440f1`](security-quality-milestone.md) passed the macOS and local
Linux formatting, workspace tests, strict Clippy and release gates. The
[20 minute 15 second native session](benchmarks/security-native-20260910.md)
covered eight tabs, demanding history/status/diff and preview fixtures,
same-path repository replacement, local cancellation and saved drafts.
The [large offline PR probe](benchmarks/large-pr-probe.md) separately exercised
300 files, 2,000 comments and 128 substantial drafts through actual backend APIs.
The [installed identity and restoration record](benchmarks/security-installed-20260910.json)
verifies UUID `2475FAA0-21B2-398D-9F9C-A2D57C92543A`, exact-path native smoke,
genuine state restoration and unchanged system settings. Live authenticated
GitHub, native Linux UI, hosted CI and maximum-size native PR interaction remain
outside this evidence. No application-wide speedup or leak conclusion is claimed.

## Final September 10 native polish milestone

[Final source `eb3dd26`](native-polish-milestone.md#final-corrections-installation-and-restoration)
passed macOS and Linux formatting, workspace tests, strict all-target Clippy and
release builds. The exact installed `/Applications/GitTurtle.app` passed bundle,
running-identity and native Compare/Back/Settings checks; original application
state and system settings were restored. [Validation identities and log hashes](benchmarks/native-polish-20260910/validation.json),
[32 unaltered native visuals](benchmarks/native-polish-20260910/visuals/README.md)
and the separately identified [21 min 58 sec resource session](benchmarks/native-polish-20260910/README.md)
retain their exercised source boundaries. Live GitHub, hosted CI, native Linux UI,
spoken VoiceOver and sleep/wake remain unverified.

## Final September 9 consistency milestone

[Source `92ea02c` installation and restoration](consistency-milestone.md#final-build-installation-and-state-restoration) records the installed UUID/signature/resource checks, 461 macOS and 457 Linux passing tests, native workflow coverage and original-state restoration. The [mixed native session](benchmarks/2026-09-09-milestone-native.md), [graph measurements](benchmarks/2026-09-09-milestone-graph.md) and [image-retention correction](benchmarks/2026-09-09-preview-lifetime.md) preserve distinct measurement boundaries and raw observations. That milestone ended with an original-repository access limitation; the current milestone records successful reopening of the unchanged build without permission changes. Its hosted/Keychain/hardware/Linux-window and tool-specific limits remain explicit in that record.

## Review and recovery installed release — September 9, 2026

Final compiled source `0c8eaec31ae9fdbc7fe7f98c5822cc0561f96ad4` passed formatting, locked workspace tests, strict all-target workspace Clippy and release compilation. The package and installed `/Applications/GitTurtle.app` share UUID `B003BB92-D6C0-3009-8A75-96A85EEBB133` and executable SHA-256 `0f346ccc7a7149d5314cebc5c0893ee182d166e2c569c6b5051906b57dc69777`. Plist/signature checks, exact installed-path launch, essential native interactions and preservation of the original preferences/drafts passed.

The [complete native record](review-native-verification.md) attributes comparison, text review, file navigation, selection/staging, worktree/recovery, conflict/rebase and local LFS checks to their actual builds, including final minimum-size and modal-focus corrections. It includes representative screenshots, system appearance/accessibility observations and precise VoiceOver/GPUI limitations. The [finite acceptance ledger](review-milestone.md) is complete. Live-provider/Keychain/hardware and hosted CI/Linux checks remain unverified for the stated environment/access reasons; none is implied by these local passes.

## Everyday workflows release review — September 8, 2026

Source `f68bd2070d4c71eec00af02cb7f67b1be107e728` passed workspace tests, strict workspace Clippy, and a release build. The packaged executable has UUID `83F6445B-F09F-3AAC-A138-BA72E7D77CAF` and SHA-256 `672615cd826e9c7001e5ef774c08b90d350ee7d6935857668c336b7e315049b1`; package signature verification passed and its UUID matched the release executable. Native checks used that release on Apple M4 Max with 128 GiB memory and macOS 26.6.2. This is local macOS package evidence, without notarization or Linux validation.

The rich disposable fixture at HEAD `bcf41e2d24ff5582a145be5d34ebbdca774c95b4` contained 653 commits, 91 local branches, 1,043 files in the selected commit, and 996 distinct working changes. All six themes—Midnight, Daylight, Graphite, Tokyo Night, Catppuccin Mocha, and Nord—were inspected in both Comfortable and Compact density at the minimum 1,000 × 680 content size. Full-window screenshots measured 1,000 × 712 including the title bar. Resizing to approximately 1,480 × 980 also exercised History and a Midnight image comparison. Narrow checks concentrated on Split, Find, and the composer; this is not a claim that every workflow was repeated in every theme, density, and wide layout.

| Area | Observed behavior in `f68bd2` |
| --- | --- |
| Appearance and navigation | Inspected source colors, selected and hovered rows, focus transitions, disabled network controls when no remote was configured, status icons, and long nested Unicode paths. Find focus returned exactly after Settings and theme changes. |
| Editor Find | Copying the query while all source text was selected copied the query. The exercised match changed from zero with case sensitivity enabled to one with it disabled. Enter, Shift+Enter, Escape, and Cmd+Shift+F routing worked in the exercised comparison. |
| Working images | Added/untracked PNGs showed an absent Before side; modified transparent PNGs showed both sides at 100% with synchronized horizontal pan. Fit reset the view, and a deleted PNG showed an absent After side. |
| Projects | A no-match search and Clear updated immediately. Canceling the native Create picker retained the name and `trunk` branch input. Create produced the empty, clean disposable `native-release-created-20260908` repository with exact `refs/heads/trunk` identity. Clone refused that populated destination without changing it, then cloned a local bare origin into `native-release-cloned-20260908` at `3c0324d5a9bb8bdbf4ddbf4b70934820d159ce47`. |
| Identity settings | Native Save was exercised for repository-local name `Native Release QA` and email `native-release@example.invalid`. Direct `git config --local` inspection verified both exact values in the cloned fixture. |

The minimum-height review exposed insufficient Working files space beneath the expanded Targets and composer. Density/viewport changes could leave the selected file offscreen, and leaving Repository for Settings or Projects discarded manually resized pane widths. Find highlighting over patch colors and an anonymous Projects clear control also needed correction. These changes are implemented in subsequent source `b20dddb6bdc40ea332872333e2a59db80c0a7b9f`, which passed workspace tests, strict workspace Clippy, and a release build. Its package and focused native checks are recorded below; the `f68bd2` matrix does not itself validate those corrections.

Follow-up direct Git inspection completed the earlier debug branch checks from executable UUID `289AC96C-08CF-3C84-A839-125F76446075`. The native workflow removed only the local `main-native-renamed` branch, set the exact upstream `origin/release/stable`, and edited the origin URL to its intended trailing-slash form. Removing origin then removed its remote-tracking refs and upstream configuration while preserving HEAD `6f07fd3`, stash `64d6…`, and the mixed working changes. These results supplement that earlier debug workflow; they were not repeated as `f68bd2` native branch actions.

### Focused release follow-up

The `b20dddb` package passed signature/plist checks and launched with executable UUID `312A7611-5F59-33E8-9AD7-FFE389CBBFEB`, SHA-256 `5bcf7378e2b388f24d7c4d351684af2266a09a3e7abbadcba6b32828d44986d8`. The release executable and packaged UUID matched. Native checks used only disposable fixtures and the existing local bare remote.

| Area | Observed behavior in `b20dddb` |
| --- | --- |
| Scoped recovery | Reverting `a304837` on `qa/scoped-recovery` created `541d9f4` while retaining that scope, its query, and the pinned old result. Restart showed the revert and original commit, excluding the matching unrelated branch commit `0213cb3`. |
| Stash conflict | Restoring `9891d154` showed “Stash restoration produced conflicts. The stash remains saved.” Full Details retained Git output. Direct inspection verified the same stash OID, the saved untracked note, an unmerged file, and no merge/rebase/cherry-pick/revert operation metadata. |
| Responsive layout | At minimum size with Targets open, Daylight Compact and Graphite Comfortable kept the selected working file visible with approximately four to five file rows and a multiline Description. Collapsing Targets increased list space. Manual inspector and navigator widths survived Settings/Back and Projects/Back; inspector width also survived History/Compare transitions. |
| Find and project search | Unified active Find contrast over an added line passed. The accessible Projects “Clear project search” button cleared immediately and retained input focus, verified by typing the next filter without clicking the field. Split still showed intermittent background precedence defects; a subsequent correction is required. |
| Large branch lists | The menu bounded its alternatives to forty. Searching `component-001` reached an entry beyond that initial list. `review/accessibility` appeared disabled and labeled as occupied by another worktree. The search command was buried beneath the unfiltered list; a subsequent presentation change moves it first. |
| Local remote workflow | Explicit Fetch changed the clone from zero known commits behind to one; fast-forward Pull reached `4f8112a`. Native staging, Title/Description commit, and Push produced `7cffcf32` in both clone and bare remote. Raw commit bytes exactly preserved the title, blank line, and two-paragraph description. |
| Divergence and merge | Explicit Fetch showed one local and one upstream commit. Pull refused fast-forward and retained visible Merge/Rebase choices. Prepared Merge named the branches and both differing paths; execution created `3483b47b` with exact parents `10ca8026` and `74c8187c`. Both disjoint files and earlier pushed/pulled files remained, with a clean index/worktree. |
| Missing image and narrow Projects | A stored LFS pointer displayed its unavailable local object and stated that no download was attempted. At minimum size, the Projects Clone form retained readable fields and its lower action remained reachable by scrolling. |
| Focus and external changes | With TextEdit active, an external fixture file was created. Returning to GitTurtle exposed that exact new file without manual Refresh; this exercises watcher/focus handling together, not an isolated focus-event latency measurement. |

A complete before/after inventory of the rich fixture verified all 1,236 file/symlink entries, sizes, hashes, and modification times unchanged after these read-only checks, excluding immutable Git objects and access/directory metadata. Direct local-remote verification is retained in the disposable `native-final-remote-evidence.json` record. This pass also found a previous repository's error banner surviving a switch and a fast-forward failure headline dominated by Git's fetch progress. Their fixes, the Split highlight correction, and the branch-search ordering change require a subsequent package/native check; none is counted as verified here.

### Final package verification

Final application source `1eebcb2a54813c3d0c96e77e80d983234e7264ae` includes the native corrections above and the divergent Pull guidance from `d48d940`. Formatting, `cargo test --locked --workspace`, strict all-target workspace Clippy, and `cargo build --release --locked -p gitturtle` passed together on this source. The package was rebuilt with `--no-build` after closing the previous app. Plist and strict ad-hoc signature checks passed; the release and packaged executable UUIDs both equal `C81E4C94-08EB-3283-860E-BE6F84E69616` (arm64). The packaged executable SHA-256 is `01577ad8a2bd7ba162c17840de7fc75bbd9051b70dd9575baecb29b7ed099f2b`.

The exact package launched and passed the focused correction checks:

- Explicit Fetch showed two local and one upstream commit. Pull refused the fast-forward with “Branches have diverged; choose Merge or Rebase to continue.” The branch and merge HEAD `3483b47b` remained unchanged, as did the independent untracked focus-test note. Native invalid Rename retained `bad name` with an example and specific naming guidance, without reaching a write confirmation.
- The operation error survived Projects/Back to the same repository, cleared when another repository opened, and did not return when the first repository reopened. The current-branch menu placed Find/Create immediately above its forty bounded alternatives.
- Split Find on an added source line retained readable syntax, a distinct selected background, and an accent underline across all six themes at minimum size. Graphite was also checked before and after wide/minimum resizing. Closing Find restored the complete patch background; Unified Find highlighted both removed and added matches. Settings/theme/density changes retained Find focus, demonstrated by replacing the query without clicking its field. Selected-file visibility and the adaptive composer settled correctly after resizing.
- Graphite and Comfortable density were restored, Targets remained expanded, and the package returned to the GitTurtle project's History page for passive inspection. The rich fixture's 1,236 protected file/symlink entries retained their recorded sizes, hashes, and modification times after the final checks.

All artwork under `assets/` remains unchanged from the pre-goal `88153ce` revision; the rebuilt package continues to consume the same Icon Composer light/dark/clear sources and embedded turtle artwork. The dated Finder appearance evidence below remains attached to that unchanged artwork. The later documentation commit changes no executable source or dependencies, so it does not require another Rust build or package.

These results complete the requested implementation and relevant local validation. The earlier native workflows and measurements retain their stated build identities and measurement boundaries. Linux, hosting-provider credential interaction, universal binaries, and notarized distribution are not claimed; the configured-helper failure fixtures and local-remote native workflows establish their narrower coverage.

### Everyday release timing

The `f68bd2` package measured twenty callbacks in each of four native selection paths on the rich fixture. [Raw samples, phase boundaries, environment, cache assumptions, and preservation checks](benchmarks/2026-09-08-everyday-native.json) are retained.

| Interaction | Samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Commit to changed-file frame | 20 | 21.241 ms | 45.238 ms | 62.794 ms |
| Immutable file to prepared text-preview frame | 20 | 4.898 ms | 7.428 ms | 13.216 ms |
| Staged file to prepared working-preview frame | 20 | 21.838 ms | 21.947 ms | 22.043 ms |
| Unstaged file to prepared working-preview frame | 20 | 23.248 ms | 24.800 ms | 24.838 ms |

The fresh release process opened only the rich fixture. History, immutable-file, staged, and unstaged phases ran in that order with Daylight and Comfortable density. Initial activations were excluded, with no separate warmup series; filesystem caches were not flushed and return immutable selections could use the 32-entry preview cache. Mutable previews bypass that cache. History selection produced zero file-preview frames. No build, test, backend benchmark, or unrelated native interaction ran during the measured phases; other desktop applications remained open and OS load was uncontrolled.

These timings span the input handler to its generation/mode-checked GPUI callback. They exclude input delivery before the handler, OS display presentation, and completed GPU work; working-preview measurements also exclude status refresh and writes. The 1,236 recorded fixture entries retained their kinds, sizes, hashes, and modification times after the phases, with no Git writes during timing. This small uncontrolled sample has no before/after baseline and establishes neither a speed improvement nor a latency guarantee. Watcher, cancellation, write, image, and memory performance are outside these measurements.

## Native icon appearances — September 8, 2026

Source revision `174a68d` replaces the premasked tile with a layered Icon Composer document: the selected mint turtle on a full-bleed background, with macOS providing the final enclosure. The document was inspected and saved in Icon Composer from Xcode 26.6. Native renders verified [light](../assets/icon-previews/light.png), [dark](../assets/icon-previews/dark.png), [clear light](../assets/icon-previews/clear-light.png), and [clear dark](../assets/icon-previews/clear-dark.png); Mono also supplies tinted appearances. [The provenance record](../assets/app-icon.prompt.json) preserves both the original selection and foreground-extraction prompt.

The release executable was rebuilt because the embedded branding PNG changed. Both `dist/GitTurtle.app` and `.local/GitTurtle.app` were repackaged and registered with Launch Services. Each passed plist and strict ad-hoc signature verification and has executable UUID `F71AC1CB-DD6B-3552-8DCF-03EC31C8EB19`, matching the release executable. Their compiled catalogs contain Aqua, Dark Aqua, and tintable icon stacks; both fallback ICNS files have SHA-256 `5206a75c4c2ae53f7686565042bb7e4e67b1ca09ebc3d4f2e5a621f3be37fefb`. Generated metadata sets both `CFBundleIconName` and `CFBundleIconFile` to `AppIcon`. Raw artwork is excluded from the bundle. The catalog targets macOS 11.0, matching the executable's recorded minimum; the fallback was not exercised on an older operating system.

On macOS 26.6.2, Finder displayed the exact `dist` package in Default, Dark, Clear Light, and Clear Dark styles. Each used one enclosure with no inset tile. Cycling the icon style refreshed the initially cached image without deleting global caches. The packaged app displayed the updated embedded branding and returned to the user's repository in History with Nord preserved. System appearance preferences were restored to their original values. The Dock surface itself was unavailable to native capture, so the visual appearance checks above refer to Finder and the app. No Git write or network action was performed in the user's repository.

Both shell scripts passed syntax checks, the render and packaging commands completed successfully, metadata hashes matched their source assets, and the final diff passed whitespace checks. Rust and dependencies are unchanged from `e8f7d42`, whose 115 workspace tests and strict Clippy passed as recorded below; these gates were not repeated for this artwork and packaging change.

## Consistent selected icon — September 8, 2026

At `e8f7d42`, the repository header, Projects header, and collapsed sidebar switched from the old vector turtle to a shared 128-pixel PNG derived from the selected `assets/app-icon.png`. The original source remains byte-identical to its recorded SHA-256 `b40bbffd6d33be3981094f1886e953a114fada7ede83dcaa0393b415bd553209`. Regenerating all ten iconset sizes produced an ICNS byte-identical to the existing selected icon. The obsolete vector variants were removed; packaging now replaces its assets directory so retired files cannot linger.

Both `dist/GitTurtle.app` and the older Launch Services–registered `.local/GitTurtle.app` were rebuilt from the final release executable and re-registered. Both passed plist and ad-hoc signature verification and have executable UUID `7C5A16FA-4C95-3FB6-BD17-94B01CED85E4`, matching the release build. Their icon, original artwork, and small branding PNG match the source assets byte-for-byte; neither contains the retired vector files. Formatting, `cargo check`, all **115 workspace tests**, strict workspace Clippy, release build, and packaging-script syntax checks passed.

The exact `dist` package was relaunched. Native screenshots verified the selected turtle in both headers and the collapsed sidebar; Finder's icon view also displayed the selected artwork. History and the expanded sidebar were restored with the user's current Nord theme and repository. The Dock surface could not be captured through native automation, so Dock appearance itself is not claimed as a visual check; bundle resources and Launch Services registration were verified without resetting global icon caches. No repository write or network action was performed.

## Native refinement — September 8, 2026

Final source revision `76f7d9a` passed formatting, **115 tests: 69 app, 34 core, and 12 preview**, strict workspace Clippy, and release packaging. The release and packaged arm64 executable have UUID `6B05721F-2BDD-3DE0-9927-93431CC12047`; plist and ad-hoc signature verification passed. The last source adjustment isolated parallel test fixture directories with an atomic sequence after a timestamp collision; it does not change the release executable. The existing app icon was retained, as requested.

Native checks used packaged revisions `ddab7e3`, `1f7e6a4`, and final `76f7d9a` on Apple M4 Max with 128 GiB memory and macOS 26.6.2. The first exercised all application pages and Git actions; the second exercised named staging feedback, six themes, and successful clone/create; the final verified both project-search clear controls, clean-state guidance, and large-repository navigation and timing. Only disposable fixtures and local remotes were mutated.

| Area | Observed behavior |
| --- | --- |
| Hover and selection | Selected history, file, navigation, theme, and density controls retained selection while showing hover feedback. Primary action hover, input focus rings, disabled actions, and status icons were inspected in the native app. Palette tests cover selected-hover surfaces across all six themes, requiring 4.5:1 secondary-text and 3:1 status-icon contrast. |
| Settings and narrow layouts | Inspected Midnight, Daylight, Graphite, Tokyo Night, Catppuccin Mocha, and Nord. At the approximately 1,000-pixel minimum window width, settings groups stacked and lower identity/default-branch fields remained accessible by scrolling. Enter saved fixture-local identity and the default branch; Reset restored edited values. Restored Midnight, Comfortable density, and `main` for new repositories. |
| Working changes | New, modified, deleted, and renamed entries had distinct status presentation. Staged and unstaged README previews showed their different content and target badges. Stage all, Unstage all, and single-file staging worked; the latter named the affected path in feedback. The compact composer fitted at narrow width, and empty repositories displayed useful clean-state guidance. |
| Commit and preservation | Native commit `1eef17d` included the staged README introduction, new file, and rename, cleared the message, and preserved the unstaged README draft, stylesheet edit, and deletion. OIDs, index contents, and remaining changes were independently checked. |
| Local remote actions | Native Push placed `1eef17d` in a local bare remote. After a fixture peer created `5cd88b3`, Fetch displayed one commit behind and fast-forward Pull advanced HEAD to that exact OID while preserving unstaged work. |
| Branch actions | A fixture with fifty additional branches displayed the current branch and bounded alternatives. Find focused the branch field; filtering and switching to `review/option-49` worked and cleared the filter. Created/switched to `design/native-refinement`, then returned to `main`; remote targets followed the selection. |
| Projects | No-match searches displayed a clear recovery action. Native review found that clearing search did not immediately rebuild recent results; final `76f7d9a` fixed this and both clear controls restored the list. Clone errors appeared inline; canceling the native destination picker retained form input. A complete local clone succeeded, and Create produced an empty repository on the configured `trunk` branch. |
| Comparison and navigation | Diff, Before, and After showed the expected content. Settings and Escape retained comparison context. An added image had an absent Before side and a checkerboard matching Daylight. On a long multi-hunk patch, both code-area and gutter scrolling remained aligned; Back restored the selected commit and file. |
| History columns | Hiding References removed its contents while the other headings and cells remained aligned. References was restored after the check. The large repository retained the selected commit and its 1,429-file inspector during navigation. |

The final app passively inspected `world-of-claudecraft` with 500 commits loaded and 649 local branches. No Git write or network action was performed in that repository. These checks validate macOS interaction and local-remote workflows; they do not validate remote authentication, Linux, or notarized distribution. Earlier image zoom/pan and literal patch-copy checks remain tied to their builds below.

### Refinement release timing

The final package measured twenty History selections and twenty text-file selections, each ten Down followed by ten Up from commit `a461924`. Initial selection and file activation were excluded. History traversal produced zero file-preview frames.

| Interaction | Samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Commit to changed-file frame | 20 | 22.795 ms | 37.439 ms | 54.086 ms |
| File to prepared text-preview frame | 20 | 4.517 ms | 7.402 ms | 18.539 ms |

Each input was followed by a native accessibility observation. The fresh process had inspected an empty fixture and Projects before opening the large repository once. Filesystem caches were not flushed; return file selections could use the immutable preview cache. Other desktop applications remained running, with no Cargo build or test during timing. These handler-to-GPUI-frame measurements exclude pre-handler input delivery, OS presentation, and completed GPU work. They are a small uncontrolled sample, not an end-to-end speedup claim or latency guarantee. [Raw samples and conditions](benchmarks/2026-09-08-native-refinement.json) are retained.

The graph now shares worker-prepared edge arrays when visible rows are cloned. An optimized in-memory harness measured sixty-row clone medians of 1.161 → 0.120 µs for a linear fixture and 1.692 → 0.121 µs for a wide fixture. The tradeoff was a one-time worker layout increase: 1.196 → 1.616 ms for 20,000 linear commits and 4.018 → 4.288 ms for 8,001 commits with 21 lanes. Each case used 100 samples after warmup. The harness excludes Git and GPUI, and desktop load was uncontrolled; it supports the narrower row-clone improvement, not an application-wide speedup. [Raw graph results](benchmarks/2026-09-08-shared-graph-edges.json) and [the reproduction script](../scripts/bench-graph-layout.py) are retained.

## Selected icon package — September 8, 2026

At `9a2ae28`, the user selected the first generated turtle. `assets/app-icon.png` preserves that output byte-for-byte; [its record](../assets/app-icon.prompt.json) retains the original built-in ImageGen prompt and SHA-256. The RGBA source is 1254 × 1254 pixels. The existing `render_icon` example generated all ten iconset representations from 16 to 1024 pixels, and `iconutil` rebuilt the ICNS. The source and 32-pixel rendering were visually inspected.

No Rust source or dependencies changed from the tested `ca8d805` build. The existing release executable and previous package had matching UUID `567C6E63-F6C9-36D2-AF6F-6CF34C028A53`; the app was closed before repackaging with `--no-build`. The new bundle passed plist and ad-hoc signature verification, retained that UUID, and contained an ICNS byte-identical to the selected asset. The packaged app launched successfully. This validates the resource update; it does not rerun or replace the native workflow and performance evidence below.

## Design and interaction validation

Source revision `ca8d805` passed formatting, **115 tests: 69 app, 34 core, and 12 preview**, strict workspace Clippy, and a release build. The arm64 macOS package was ad-hoc signed and verified; executable UUID `567C6E63-F6C9-36D2-AF6F-6CF34C028A53` matched the release executable. Native checks exercised the design build leading to `b8b5799`, that packaged revision, and final `ca8d805` on Apple M4 Max with macOS 26.6.2. Only disposable fixtures and local remotes were mutated.

| Area | Observed behavior |
| --- | --- |
| Actions and branches | Fetch, Pull, and Push remained visible with Targets expanded by default. Created/switched to `design/native-check` and `design/final-check`, then returned to `main`. Successful branch creation cleared the input filter and restored other branch choices. The final build displayed the explicit target in Fetch feedback. |
| Staging and commit | A partially staged README showed different staged and unstaged previews. Commit `ae4ceb7` included the staged introduction, cleared its message, and retained the unstaged draft and two new files. Stage all and Unstage all moved all three files between groups; disabled commit prompts explained the next required action. |
| Local remote actions | Push placed the fixture commit in a local bare remote. A peer pushed `5fb9340`; Fetch showed one commit behind, and Pull advanced HEAD to that exact commit while preserving the unstaged work. OIDs and file/index contents were checked outside the UI. |
| Themes and hierarchy | Inspected Midnight, Daylight, Graphite, Tokyo Night, Catppuccin Mocha, and Nord in the real comparison view. Primary buttons used each theme's accent and readable foreground. Settings previews, file status colors/icons, commit details, and Comfortable/Compact spacing were exercised. |
| Columns and scrolling | Resized Graph, narrowed the window, and dragged the horizontal scrollbar to Author, Date, and SHA; headings and cells remained aligned. Vertical history scrolling retained the selected commit's inspector. Restored default columns, Comfortable density, and Midnight. |
| Projects and errors | Filtered recent projects and opened a result. Incomplete clone submission showed its nearby destination error. Opened and canceled the native folder picker without losing the form. |
| Comparison and focus | Settings and Escape restored the same comparison context. Typing did not edit a read-only patch; copying and pasting into a disposable draft preserved literal patch text without gutter numbers. Back retained the commit and selected file. |
| Icon and package | Inspected the new turtle/branch icon at small size. The production PNG has a real alpha channel; ICNS includes 16–1024 px representations. Large branding files are excluded from the application's embedded UI asset set. |

The final application also passively inspected `world-of-claudecraft` with 500 commits loaded and 649 local branches. Its branch menu showed the current branch, bounded alternatives, and Find/Create without building the entire branch list into the menu. No Git write or network action was performed in that repository. Remote authentication, Linux, and notarization remain outside this evidence; image comparison checks from earlier builds are recorded separately below.

### Design release timing

The final packaged release measured twenty History selections and twenty text-file selections, each ten Down followed by ten Up from commit `8cf37f7`. Initial activation was excluded. History traversal produced zero file-preview frames.

| Interaction | Samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Commit to changed-file frame | 20 | 24.052 ms | 28.125 ms | 29.046 ms |
| File to prepared text-preview frame | 20 | 4.336 ms | 7.438 ms | 7.523 ms |

Each input was followed by a native accessibility observation. Filesystem caches were not flushed, the process had already inspected fixtures, and return traversal could use the preview cache. Other desktop applications remained running. These handler-to-GPUI-frame measurements exclude pre-handler input delivery, OS presentation, and completed GPU work. They are a small uncontrolled sample, not an application speedup claim or latency guarantee. [Raw samples and conditions](benchmarks/2026-09-07-design-native.json) are retained.

A separate optimized Rust graph-layout harness measured a 20,000-commit linear fixture at 2.836 ms before and 1.203 ms after (median), and an 8,001-commit, 21-lane fixture at 4.807 ms before and 4.047 ms after. It used 20 warmups and 100 samples per case, reused in-memory inputs, and excluded Git and GPUI. The updated layout includes cancellation checkpoints. Development CPU load was uncontrolled; [the graph benchmark](benchmarks/2026-09-07-graph-layout.json) records raw values, tail latency, source hashes, and [reproduction instructions](../scripts/bench-graph-layout.py).

## Everyday Git workflow validation

Final source revision `55e7f14` passed formatting, **113 tests: 67 app, 34 core, and 12 preview**, strict workspace Clippy, and a release build. Native checks exercised packaged revisions `ba0ccb7`, `ceea5d9`, `2742212`, and the final `55e7f14` on Apple M4 Max with 128 GB memory and macOS 26.6.2. Only generated `.local` fixtures and local remotes were mutated during development validation.

The final arm64 macOS package was ad-hoc signed and verified. Its executable UUID, `984A3FEA-07EF-3446-93E4-7CE0412C5C87`, matched `target/release/gitturtle`.

| Area | Observed behavior |
| --- | --- |
| History columns | Resized References from 140 to 219 px, then reset the layout; hid Author; set Graph to 415 px and confirmed that width after restarting. Dragging the horizontal scrollbar revealed SHA while keeping header and rows aligned. |
| Appearance and settings | Exercised Daylight, Graphite, and Midnight, plus Compact and Comfortable density. Saved `trunk` as the default branch and used it for a new project. Repository identity edits updated repository-local configuration. |
| Project opening and drafts | Opened a repository through its nested `src` folder using the native picker and retained the existing commit draft. At this build, draft retention applied within the running session. |
| Create and first commit | Created an unborn repository on `trunk` and made its initial commit `cc69432`. |
| Staging and comparisons | Staged and unstaged the same file and verified that selecting its two groups showed the different staged and unstaged content. Image checks exercised Before/After, Fit, 200% zoom, and dragging. |
| Commit and push | Created commit `b54e318` on `main`, pushed to a local bare remote, and verified that the remote had the same OID. |
| Fetch and pull | A fixture peer created `0d6cbaa`; Fetch showed the local branch one commit behind, and fast-forward Pull updated HEAD to the peer commit. |
| Branch actions | Created/switched to `qa/native-workflow`, then switched to `main`; the remote-branch input tracked the selected local branch. |
| Clone and failures | Refused a nonempty destination. A missing-LFS smudge failure surfaced the normal Git error and preserved the partial destination. After the fixture peer removed the intentionally missing pointer, a complete clone into `native-clone-ready` succeeded. |
| Final Settings focus check | From a working-file preview, Command-, opened Settings. Switching to Graphite and pressing Escape restored the same README unified comparison and file-list focus; the editor remained read-only. |
| Final commit feedback | Staged and committed `61828ae` with the message “Verify final native workflow.” The result displayed the short OID and summary on one line, cleared the commit message, and showed a clean working state. |

Pull was fast-forward-only and Push did not force-update refs. These network-action checks used local remotes; remote authentication was not validated. Commit drafts were session-only in `55e7f14`. Linux interaction/builds, notarization, and remote credential flows remain outside this evidence.

The final application was also used for passive inspection of `world-of-claudecraft` with 500 commits loaded. Code-area scrolling kept the unified patch and old/new gutter aligned; Back retained the commit, selected file, and search query. Activating an image followed by Escape stayed in History. The final preferences were returned to Midnight, Comfortable density, default columns, and `main` for new repositories.

### Final release timing check

Release `55e7f14` used the hardware and package above. Twenty History selections (ten Down, ten Up from `69ffdab`) produced zero file-preview frames. A separate forty-selection code traversal in `1e11554` started at `headless/gathering_goal_protocol.ts`, moved twenty files down to `src/sim/professions/material_goal_projection.ts`, and returned. Each action was followed by a native accessibility observation.

| Interaction | Samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Commit to changed-file frame | 20 | 23.671 ms | 26.119 ms | 48.195 ms |
| File to prepared text-preview frame | 40 | 5.508 ms | 7.646 ms | 8.057 ms |

The initial selection and file activation were excluded. Filesystem caches were not flushed; the process had already inspected disposable fixtures, and return selections can use the 32-entry preview cache. Other desktop applications and development work remained running. These are application-handler-to-frame-callback measurements, excluding pre-handler input delivery, OS presentation, and completed GPU work. The small uncontrolled sample is not a speedup claim or latency guarantee. Raw values and conditions are in [the everyday-workflow timing record](benchmarks/2026-09-07-everyday-workflow.json). Historical measurements below remain tied to their original builds.

## Historical automated and build checks

The earlier history/comparison workspace run passed **73 tests**, strict workspace Clippy, and a release build. Tests used disposable repositories for operations that created Git objects, refs, or worktrees.

```sh
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --release --locked -p gitturtle
```

Coverage includes repository roots and merge parents, branches and linked worktrees, unusual path bytes, binary/text/mode/type changes, SHA-256 repositories, missing partial-clone objects, local LFS integrity and symlink rejection, Git process deadlines, and repository-file snapshots with hostile configured helpers. Preview tests cover decoding limits, alpha handling, SVG resource rejection, and LFS pointer recognition. App tests exercise queue replacement, stale-work cancellation, cache identity and accounting, BGRA conversion, local LFS arrival, refreshed branch/worktree tips, and graph budgets. New coverage includes branch folder expansion/filtering, retained repository sessions, and old/new line numbering for unified patches, including header-like source text, accumulated gutter-wheel movement, worker-prepared patch metadata, and its cache allocation budget.

A local Apple Silicon `.app` can be built with [the packaging script](../scripts/package-macos.sh). It is signed ad-hoc for local use, not notarized. The [current package procedure and evidence boundary](benchmarks/ci-packages/macos-package-source.md) describe explicit arm64 target output, verified `--no-build` inputs, detached build identity, optional ZIPs and existing-output recovery. The new source fixtures do not update the historical Mac package passes; exact-candidate macOS execution remains open.

## Historical column layout update

The updated release has a full-height history table and persistent right-hand commit/file inspector. Explicit file activation opens a full-height comparison; Back returns to retained history. Local and remote references are grouped into branch folders. Unified patches now have a separately painted old/new line-number gutter, preserving the literal editor text.

The final run for this historical layout passed 73 tests (44 app, 17 core, 12 preview), strict workspace Clippy, and release packaging. Native checks resumed after the Mac was unlocked. On the demonstration repository, the full-height history/comparison layout, separately aligned old/new gutter, read-only typing, literal patch copying, keyboard activation, and Back navigation were exercised. The right inspector retained its dragged width across mode changes. Branch-folder expansion, temporary search expansion, branch scoping, merge-parent changes in both modes, added/modified/deleted images, and missing-LFS messages behaved as expected. Opening a non-repository folder cleared previous navigation and content and displayed the error.

The final scrolling pass verified linked drag-to-pan on both axes at 200%, reversal to the origin, and dragging across the preview toolbar. The CUA horizontal wheel gesture emitted a zero x/y delta during diagnostic tracing despite a positive horizontal scroll range, so horizontal trackpad behavior remains unverified by that tool. Vertical wheel panning was verified.

On `world-of-claudecraft`, both code-area and gutter wheel scrolling kept a 114-line patch aligned. Back restored the same history viewport (first visible `8b337c1`, selected `1e11554`), file selection, and inspector. Worktree filtering and opening the `feature/freeholds` worktree succeeded; the navigator showed 130 worktrees. An immediate image-open-and-Escape action remained in History after the preview completed. Earlier native checks below apply to the previous layout and are retained as historical evidence.

At `3053ac9`, patch gutter rows, width, and decoration ranges moved to the repository worker, with their retained allocations counted in the preview cache. The packaged release was checked again: gutter/code scrolling remained aligned, Before was empty for an added file, After displayed syntax-highlighted source, and Back retained the selected commit and file.

## Historical initial native macOS checks (before column layout)

The application was opened against the locally available `world-of-claudecraft` repository containing **5,577 branches and 130 worktrees**, and against a generated demonstration repository.

| Area | Exercised behavior |
| --- | --- |
| Repository navigation | Large branch/worktree lists, local/remote branches, native folder picker, invalid-repository errors, and draggable history/sidebar dividers |
| Commit inspection | Changed-file selection, explicit merge-parent comparisons, locked-worktree display, and selected-commit preservation while expanding history |
| Text | Unified patch and Before/After views; typing did not edit the preview; text selection and copying worked |
| Images | Modified, added, and deleted PNGs; transparent pixels; linked zoom and vertical panning |
| Unavailable content | Binary file information and missing local LFS image messages |

The earlier horizontal-wheel check was inconclusive. The column-layout update above adds and verifies two-axis mouse dragging; horizontal trackpad gestures remain unverified.

These are manual checks of the initial native build, not an exhaustive platform or accessibility certification. The [design document](../DESIGN.md) includes intended behavior beyond the implemented surface.

To create a fresh disposable demonstration repository:

```sh
python3 scripts/create-demo-repo.py
cargo run --release --locked -p gitturtle -- .local/demo-repository
```

The generator also creates a linked worktree. It refuses nonempty destinations, including existing repositories. For another run, choose a fresh destination with `--output /path/to/empty-or-new-directory`; do not point it at a working repository.

## Timing methodology

Initial backend observations, hardware/toolchain details, and the reproducible inspection command are recorded in the [Git service benchmark notes](../crates/git-core/README.md#initial-measurement-september-7-2026). The backend harness measures Git service work. It excludes UI dispatch, queuing, image decode, editor preparation, rendering, and presentation. Filesystem caches were not flushed, so those observations are not cold-disk results.

The later [everyday backend report](benchmarks/2026-09-08-everyday-workflows.md) provides p50/p95/maximum values from two warmups and twenty measured calls per eligible series, with [reproduction instructions](benchmarks/everyday-bench.md). It records current costs without a baseline or speed-improvement claim. The rich fixture's two-page file-history sample retains a continuation; the project staged-preview series is skipped because it had no staged changes. Neither result may be represented as zero latency or exhaustive work that was not performed.

The native app has a separate optional trace:

```sh
GITTURTLE_TRACE=1 target/release/gitturtle /path/to/repository
```

`gitturtle.commit_files_frame_ms` now measures a History selection through its changed-file list frame. `gitturtle.file_preview_frame_ms` measures an explicit file activation through its prepared comparison frame. History selection does not eagerly prepare a file preview. Returning to History uses retained state without a new Git request. Both metrics start in the application handler and use a generation- and mode-checked GPUI callback; they exclude input delivery before the handler, OS presentation, and completed GPU execution. Superseded interactions emit no sample.

`gitturtle.working_preview_frame_ms` measures working-file activation through the prepared-preview callback. It excludes the preceding status refresh or Git write. Working measurements must identify the staged/unstaged area and stable HEAD/index/worktree content; do not combine them with immutable-history preview samples or backend-only timings.

The earlier `gitturtle.selection_frame_ms` trace, used for the historical measurements below, starts in the application selection handler. For a commit selection, it includes reading the changed-file list and preparing the chosen file preview; a direct file selection starts at that file's handler. The value is emitted at a GPUI frame-completion callback after the current preview is prepared, with a generation check to suppress superseded results. It does not measure input delivery before the handler, OS display presentation, or completed GPU execution. Interactions without a completed preview do not produce this sample.

The status bar's **content read** duration covers worker processing, including a content-cache lookup on a hit. It excludes time waiting for the worker and subsequent editor construction or frame work. Comparing this value directly with another client's click-to-visible delay would be misleading.

Native trace samples should be reported with the build profile, repository, selected content, sample count, system load, and cache conditions. No frame-latency or memory guarantee is established by the current checks.

## Historical column-layout native measurements

On Apple M4 Max / macOS 26.6.2, release `9b47e08` opened `world-of-claudecraft` with 500 commits loaded. Twenty Down actions followed by twenty Up actions produced 40 changed-file-list frames and **zero file-preview frames** during History navigation. A separate 40-selection text/code traversal inspected commit `1e11554`.

| Build and interaction | Samples | Median | 95th percentile | Maximum |
| --- | ---: | ---: | ---: | ---: |
| `9b47e08`: commit to changed-file list | 40 | 29.913 ms | 46.964 ms | 64.041 ms |
| `9b47e08`: file to prepared text preview | 40 | 7.690 ms | 10.497 ms | 10.736 ms |
| `3053ac9`: file to prepared text preview | 40 | 5.523 ms | 7.517 ms | 7.722 ms |

The `3053ac9` confirmation used a fresh application process and the same immutable commit and file traversal after moving patch metadata preparation to the worker. Its initial file-open frame was excluded from traversal statistics. Both file runs started and ended on `headless/gathering_goal_protocol.ts`, traversing through `src/sim/professions/material_goal_projection.ts` before returning. Each action was followed by a native accessibility observation. Return selections can hit the 32-entry content cache; editors are constructed lazily.

Filesystem caches were not flushed, and other desktop applications and development activity remained running. These are small local measurements with different process/cache conditions, not a controlled comparison or proof of an improvement. The callbacks do not measure completed display presentation. Neither file run measures image decoding, and no latency or memory guarantee follows from these samples.

RSS after outbound/return traversal was approximately 129.9/130.0 MiB for History and 140.3/140.6 MiB for text comparisons at `9b47e08`; the final text run at `3053ac9` recorded 133.1/128.1 MiB. These are process snapshots, excluding separate GPU accounting. Raw samples, initial-frame exclusions, cache conditions, and boundaries are saved in [the column-layout record](benchmarks/2026-09-07-columns.json) and [the final prepared-diff record](benchmarks/2026-09-07-prepared-diffs.json).

## Historical initial optimized native measurements (before column layout)

On Apple M4 Max / macOS 26.6.2, the release build at `8b0799e` opened `world-of-claudecraft` with 500 loaded commits. We sent 25 Down actions and then 25 Up actions, observing the native accessibility state after each. The OS filesystem cache was not flushed; other desktop applications and development activity remained running. This is a small first measurement, not a comparative benchmark or latency guarantee.

| Completed preview samples | Count | Median | 95th percentile | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Navigation, excluding initial selection | 49 | 21.882 ms | 29.838 ms | 308.598 ms |
| Return traversal (subset above) | 25 | 21.910 ms | 27.176 ms | 27.216 ms |

The initial selection callback took 72.441 ms; that excludes repository discovery and history loading and is not application startup time. Fifty navigation actions emitted 49 completed-preview samples; absent samples are not counted as zero latency. The 308.598 ms outlier is retained, and this trace does not isolate its cause. The raw samples, method, and limits are in [the measurement record](benchmarks/2026-09-07-native.json).

Process RSS snapshots were about 142.3 MiB after the outbound traversal and 143.4 MiB after the return. These snapshots exclude GPU memory accounting and do not establish a memory-growth guarantee. The locally packaged app occupies about 30 MiB on disk.

## Current limits and follow-up

- Ordinary history streams an immutable captured traversal in500-commit pages and retains a5,000-row/64MiB window, plus one selected inspection. Older continues forward; Previous replays and discards bounded earlier pages; Latest captures current local tips and returns to row zero while retaining the selected inspector. Refresh resolves current refs again. Repository-wide search is separate: it pins local tips or selected ancestry, supports cancellation and explicit continuation, and retains up to 10,000 matches or 64 MiB of metadata. Scan, byte, and time stops do not establish exhaustion.
- Local filesystem and regained-focus events request coalesced read-only refreshes; manual Refresh remains available. Active writes and foreground reads take priority, and failed watchers report a recovery action. No refresh fetches objects. Fetch, fast-forward Pull, and non-force Push require explicit actions; the dated native network checks used local remotes, not a hosting provider's credential flow.
- Supported text changes allow hunk and changed-line staging/unstaging. Binary, oversized, filtered/normalized, renamed, and mode/type-changing files use whole-file actions; ambiguous missing-final-newline selections require a complete replacement or hunk. Commit Title/Description drafts are persisted per worktree, subject to the bounded preference file.
- Unified and aligned split diffs coexist with Before/After source tabs. Text previews and manual conflict editors are bounded to 2 MiB and 100,000 lines per side. Parent controls expose the first 128 parents of unusually large merge commits with an explicit count notice. Rename detection uses a 1,000-candidate limit.
- File history follows first-parent lineage with exact revision paths, rather than every ancestry route through a merge. UI pages contain 100 rows; the core replays the bounded rename-following prefix, which can reach its 32 MiB or 15-second limit on deep pages and require an older anchor.
- Merge/rebase and conflict resolution support deliberate Continue, Abort, and Keep files actions. Native interactive rebase reviews up to 100 linear commits, with reword/squash message editing and interruption recovery. Root/merge-preserving rewrites, apply-backend continuation, non-UTF-8 messages and ambiguous external reword checkpoints require Git's configured tools. Abort refuses independent work it cannot safely preserve; rebase has a stricter dirty-work guard. A separate [rewritten-series review](rewritten-series.md) can publish one explicitly reviewed branch with an exact expected-OID lease; ordinary Push remains non-force. Submodule management remains outside the UI. See [rebase semantics](interactive-rebase.md).
- Conflict parsing supports at most 4,096 blocks within the 2 MiB/100,000-line text bound, preserving surrounding bytes. Save draft and Save and stage are separate actions. Conflict and rebase-message drafts persist atomically outside repositories, keyed by canonical worktree and exact source identities. Saving/Saved/error states distinguish pending persistence; stale drafts remain copyable and are never restored blindly. The separate recovery store holds at most 256 entries / 64 MiB of text and retains completed/stale drafts until explicit discard. [Block semantics](conflict-blocks.md) documents source validation and fallbacks.
- Stash restore keeps saved work until a separate Drop; prepared recovery actions refuse stale targets. Undo requires a named local tip with one parent and refuses known remote-tracking containment. Local refs cannot establish whether a commit is published on an unfetched remote. See [core recovery semantics and bounds](../crates/git-core/README.md#explicit-everyday-operations).
- Static images use a preview capped at a 1,600-pixel edge. GIF comparison supports explicit playback and frame stepping with bounded decoded frames at an 800-pixel edge; secondary image inspectors retain the first frame. JPEG 2000 uses macOS ImageIO, with a codec explanation on Linux. Side-by-side, Overlay and Wipe share a source coordinate system; zoom percentages use a bounded comparison scale and preserve original dimension differences. Source/decoded dimensions and the scale relationship are shown. See the [file-preview support matrix](file-previews.md) for per-format limits; SVG filters and embedded/external images remain unsupported.
- Missing LFS previews offer an explicit one-object download capped at 32 MiB, requiring Git LFS and a configured source. Size/SHA-256 and existing decoder bounds apply; no checkout, smudge or index/worktree rewrite is used for display. [Local LFS fixtures](lfs-previews.md#local-evidence) do not establish hosted transport coverage.
- Incremental graph preparation preserves a bounded frontier across pages, with at most128 simultaneous lanes and200,000 parent/edge budget entries as defined by the worker. Above the budget, the UI explains why connections are hidden and shows isolated nodes instead of incomplete ancestry lines.
- The preview cache is limited to 32 entries and 128 MiB of retained CPU content allocations. UI-held references and GPU resources have separate lifetimes. Search and file history can terminate active Git processes; other reads/decodes retain their individual bounds and cancellation checkpoints. Input/allocation limits are not a process sandbox or a hard end-to-end deadline.
- Blame and line history have bounded text/output/lineage, explicit uncommitted and shallow-boundary states, and cancellable Git reads. Tag actions preserve configured signing and captured local/remote identities; ignore actions preserve destination content, tracked files and unrelated index/worktree data. Their current semantics do not retroactively expand the historical native evidence.
- Available Linux/aarch64 checks and candidate failures are recorded by exact snapshot in the [current Linux evidence](benchmarks/native-polish-20260910/validation.json). Earlier source `92ea02c` passed457 unique tests as recorded in its [environment evidence](benchmarks/2026-09-09-milestone-environment.md#image-lifetime-correction-linux-validation). Linux native-window interaction, hosted CI execution, live-provider sign-in, real Keychain unlock and hardware-backed signing remain unverified here. A quality workflow is authored; local .app packaging is for development verification. Distribution, installers, notarization and publishing are outside this milestone.
