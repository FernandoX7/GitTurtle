# Validation notes

Current milestone: [native polish and pull-request review](native-polish-milestone.md), with the canonical [design contract](../DESIGN.md). The prior [review and recovery native/build verification](review-native-verification.md) and [macOS milestone evidence](macos-native-verification.md) remain tied to their named builds.

This page contains current validation guidance and dated local evidence, with each completed stage tied to its exercised source/build. The September 7–8 records below cover earlier history, design and everyday Git workflows; the September 9 backend report covers its recorded review-milestone inputs. Native interaction evidence is macOS-specific. Current platform execution and access limits belong in the active milestone and [environment report](benchmarks/2026-09-09-milestone-environment.md). The configured [quality workflow](../.github/workflows/quality.yml) alone is not evidence of hosted CI execution. Distribution is outside the current milestone.

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

## Current validation guidance

The dated records below apply to their named builds, including the behavior and limitations those builds had. They do not establish native or package coverage for subsequent source changes. The [current feature overview](../README.md#current-source-features), [architecture and bounds](architecture.md), [active milestone](native-polish-milestone.md), [previous macOS milestone](macos-milestone.md), and [earlier everyday-work implementation record](everyday-work-plan.md) distinguish implemented behavior from build-specific evidence. Earlier package passes do not establish coverage for the ten current feature areas. The [preview matrix](file-previews.md), [profiles](profiles.md), [command palette](command-palette.md) and [rewritten-series review](rewritten-series.md) specify the corresponding implemented behavior; the active ledger records their native acceptance.

For changes to the current workflows, use disposable repositories and local remotes for mutations, and select the relevant checks below. Record the exercised source/build identity and independently inspect Git results; the presence of a control or a passing core fixture does not by itself verify its native interaction.

| Current workflow | Relevant validation |
| --- | --- |
| Repository tabs and local workspaces | Open a new repository after a search, verify independent inputs, canonical alias deduplication and linked-worktree identity, eight-tab bound, pin/group/reorder/close, captured in-flight writes, draft recovery and lazy restart bookmarks. Exercise moved/missing paths and explicit picker recovery. |
| Incremental ordinary history | Page across the5,000-row/64MiB window with an older selection retained; verify stable OIDs, connected graph frontier and native Older/Previous/Newest behavior. Inspect slim and lane-overflow graphs, rapid selection, cancellation and search discontinuity. Use the [120k fixture measurements](benchmarks/2026-09-10-history-pagination.md) for backend comparisons and separate native callbacks. |
| Interactive model comparison | Exercise pointer and keyboard orbit/pan/zoom/fit, standard views, linked and independent cameras, edges, orientation, units, missing sides and literal source. Compare curved analytic and mapped STEP fixtures within the [finite support matrix](file-previews.md), then repeat opens/tab changes/window closure while observing resource retirement. |
| PDF and rendered Markdown | Navigate PDF beyond page8 with entry/previous/next, unequal counts, linked positions, zoom, extracted text and literal copy; retain state across Back/tabs/restart and evict bounded cached pages. Check native Markdown prose/tables/code/Mermaid, revision-correct local images, explicit local/external links, linked scrolling, keyboard reading and exact source staging. |
| GitHub collaboration | Open/close the offline native panel through the palette, validate forms and recover local drafts without network activity. Mock captured account/repository/head identities, pagination, rate limits, partial failures and uncertain outcomes. Real connection, PRs/comments/reviews and hosted CI require the specifically authorized disposable context; record live results separately. |
| Precise staging and commits | Exercise hunk and changed-line stage/unstage with mixed index/worktree edits; verify unrelated index entries and working bytes. Check whole-file fallback explanations, exact Title/Description bytes, hook/signing failures, and worktree-specific draft retention through navigation and restart. |
| Conflicts and integration | Inspect base and both named sides, rebase labels, manual and complete-side resolution, external edits, stale-save refusal, and editor handoff. Verify Continue's staged-path review, external operation detection, Abort preservation, and Keep files without losing HEAD/index/worktree state. |
| Stashes and commit recovery | Inspect staged/unstaged/untracked saved content; restore with and without staged state; confirm the stash survives success and conflict until an explicit Drop. Check amend, eligible Undo, revert/cherry-pick and merge-parent choice, including stale targets, failures, and independent work. |
| Branches and remotes | Review actual switch/create/integration targets, invalid rename names and destination collisions, tracking/upstream changes, safe deletion, and linked-worktree occupancy. Verify remote configuration separately from explicit fetch/pull/push. |
| Search and file history | Find a match beyond loaded history, retain pinned scope across ref movement, cancel active work, and continue a bounded scan without claiming exhaustion. Cross file-history page and rename boundaries, inspect deletions/merge parents, and retain query, revision, selection, viewport, and focus through Compare/Back/Settings. |
| Diff and refresh interactions | Inspect unified and split alignment, Find, copying without padding/gutter text, opposite-side scrolling, and partial selections. Make external file/ref changes, switch focus away and back, and verify coalesced local refresh retains context/drafts while invalidating stale selections. Exercise watcher errors and manual recovery. |
| Authentication and cancellation | Use disposable loopback transports and configured helpers for username/token prompts, expired credentials, SSH-agent transport, host verification, configured commit/tag signatures and signing refusal, exact-secret diagnostic/progress masking, cancellation and no replay. Verify retained index/working state and explicit remote targets. Test detached helpers retaining output or input pipes; the app must stop and join its own I/O threads. [Local authentication/signing evidence](authentication.md#verification-and-limits) is separate from live-provider access, real Keychain unlock and hardware-backed signing. |
| Blame and line history | Exercise immutable revision attribution, raw working files against HEAD, staged-only and unstaged uncommitted lines, renames, shallow history and unavailable content. Copy exact source; inspect a line's commit and bounded first-parent history; verify Back, focus, selection, cancellation and stale-result rejection across nested inspections. |
| Tags and ignore | Filter and inspect tags; review lightweight/annotated creation and signing; refuse moved-tag deletion and changed remote destinations; verify that named-tag Push creates only that remote ref. Preview literal file/directory rules in shared/local destinations, preserve formatting and unrelated work, refuse stale/symbolic writes, and keep tracked paths tracked without staging. |
| Image comparison | Exercise side-by-side, Overlay opacity and draggable Wipe with linked pan/zoom, keyboard adjustment, checkerboards, different source sizes/downsample ratios and missing sides. Verify scale labels, gesture cancellation, retained navigation and unchanged decoder bounds. |
| macOS conventions and accessibility | Check menu availability, standard shortcuts, Help, Hide/Minimize/Close, captured Finder/editor handoff and launcher failures. Exercise follow-system appearance and manual themes without losing editor context. Inspect ordinary keyboard focus, names and supported selected/expanded/disabled states, and record available transparency/contrast/motion settings separately from unsupported hardware or OS versions. Exercise VoiceOver names, roles, selected/expanded/disabled states, current-row announcements, editing, modal containment, restored focus and status/error announcements using the [native accessibility contract](native-accessibility.md); record actual settings and build-specific results. |

Native checks should also cover narrow/wide layouts, long names, large lists, ten themes, both densities, keyboard focus, hover/selection/disabled states, and empty/loading/error states for the affected controls. Existing image and package evidence remains scoped to its dated records. Final combined Rust/dependency gates and package checks follow [the project validation agreement](../AGENTS.md#validation); a docs-only update requires link and diff review, without rebuilding the app.

### Retained review and recovery workflow checks

These rows describe required checks, not completed native passes. Record results and the exercised build in [the current milestone ledger](native-polish-milestone.md); keep final release, installed executable identity and platform/account-dependent evidence separate. Re-run a successful check only after relevant changes or a concrete unresolved concern.

| Required feature | Current validation scope |
| --- | --- |
| 1. Revision comparison | Use [the revision workflow](../README.md#browse-history-then-open-a-comparison) to compare diverged branches, tags and explicit commits in both directions and modes. Check resolved IDs, rename/mode/type changes, absent text/image sides, ambiguous names, moving refs, missing objects and unrelated/multiple-base ancestry. Cancel during a read; verify no checkout/fetch and Back/focus restoration, including a late preview after leaving Compare. |
| 2. Text review | Exercise [review variants](architecture.md#prepared-diff-presentation) in unified/split modes: intraline Unicode edits, CRLF, no final newline, long lines, whitespace suppression, context expansion through 192 lines, and Option-Up/Down. Verify literal source copy, Find, gutters and linked scrolling. Filtered/expanded variants must explain disabled partial staging; resetting must restore exact Git actions and preserve unrelated changes. |
| 3. Text size and accessibility | Follow [typography and density](../DESIGN.md#typography-and-density): independent interface/code settings and resets, persistence, both densities and ten themes at minimum/wide sizes. Retain selection, focus, Find and viewports through scaling. Inspect names/roles/supported states and Increase Contrast, Reduce Transparency and system light/dark behavior where available. Exercise VoiceOver names, roles, selected/expanded/disabled states, current-row announcements, editing, modal containment, restored focus and status/error announcements using the [native accessibility contract](native-accessibility.md); record actual settings and build-specific results. |
| 4. Quick Open and path filters | Exercise Command-P immediate typing, worktree versus pinned revision scope, keyboard selection/Return/Escape, Unicode/long/raw-byte paths, deleted/conflicted/unsupported files, no matches and visible truncation. Verify File History/Blame use the inspected target and Back restores an interrupted source preview. Check [bounded discovery](architecture.md#revision-inspection-review-and-recovery), rapid query replacement, repository switching, and changed/working file filters. |
| 5. Multi-file staging | Exercise Command-toggle, Shift-click/arrow ranges, Command-A, selected counts, directory grouping and separate staged/unstaged identities. Compare Git index/worktree bytes before/after exact selected Stage/Unstage, including renames, binaries and mixed states. Filtering/grouping clears selection; refresh retains only visible survivors; switching repositories clears it. Check stale plans, partial failures, filtered all-files disabling and existing hunk/line staging. See [working operations](../README.md#open-a-project-and-work-with-git). |
| 6. Worktree management | Follow [worktree semantics](parallel-work-recovery.md#worktrees): review and create existing/new branch destinations, inspect state and hand off to GitTurtle/Finder/editor. Refuse occupied branches, stale identities, dirty/untracked/ignored content, locked/missing/main/current worktrees and active conflicts. Verify shared versus private configuration/drafts, branch retention after removal, and honest partial-checkout failure feedback without recursive cleanup. |
| 7. Activity and reflog recovery | Exercise the bounded [activity/reflog workflows](parallel-work-recovery.md): captured repository/target/time, running and final outcomes, cancellation/uncertainty, restart, and explicit next actions without replay. Confirm retained activity excludes secrets and arbitrary diagnostics. Inspect available and expired/missing reflog commits; create the exact recovery branch after revalidation while preserving HEAD, index and working bytes. |
| 8. Conflict blocks | Follow [block resolution](conflict-blocks.md) across merge, rebase, cherry-pick and stash conflicts, including merge/diff3/zdiff3 markers. Test Previous/Next and shortcuts, Current/Incoming/Both, manual editing, unresolved counts, empty/CRLF sides, malformed markers and fallbacks. Save draft must leave the index conflicted; Save and stage must refuse remaining markers/stale sources and preserve unrelated entries. Retain drafts through file/view changes and unrelated refresh; exercise Continue/Abort/Keep files separately. |
| 9. Interactive rebase | Follow [native rebase](interactive-rebase.md): reviewed exclusive base, exact sequence, button and Option-arrow reorder, P/R/S/F/D actions, invalid squash/fixup positions, known-remote acknowledgment and stale plans. Verify native reword/squash messages, separate base/replayed-commit labels, hooks/signing failures, intermediate cancellation, conflicts, Continue/Abort and restart resume. Protect tracked/untracked/ignored work and explain unsupported histories; no automatic force-push. |
| 10. Explicit LFS downloads | Use [LFS preview semantics](lfs-previews.md) with a disposable local source. Verify reviewed file/object/source/size, one-object scope despite unrelated/recent pointers, missing tooling/configuration/credentials, cancellation, corrupt/unavailable objects and stale plans. Independently confirm SHA-256/size, unchanged HEAD/index/worktree and preview reload only for the still-selected target. Include raw working pointers and resolved LFS text's whole-file-only staging; ordinary browsing must remain passive. |

The [September 9 app preparation and path-filter report](benchmarks/2026-09-09-review-app.md) records seventeen release series with forty measurements and three warmups each, including bounded diff/intraline/split/context preparation and cached 50,000-path filters. It reports process memory and CPU-only timing separately from native frames.

The [September 9 release backend report](benchmarks/2026-09-09-review-backend.md) and [raw attempts](benchmarks/2026-09-09-review-backend.json) cover thirteen series with forty measured calls and three warmups each: endpoint/merge-base comparison, tracked-path searches, conflict parsing, rebase planning, worktree listing/details, reflog reads and passive LFS preparation. Source/fixture hashes were stable throughout the run. The report includes p50/p95/p99/max and substantial uncontrolled background load; it excludes native frames, transfer/cancellation latency, mutation, memory-growth analysis and final-package verification, and makes no speedup claim.

The [September 8 everyday backend report](benchmarks/2026-09-08-everyday-workflows.md) records release core measurements for status, working previews, search, file history, and changed-file reads with/without renames. Its raw data and fixture checks establish the stated backend observations; they exclude native frames, writes, watcher behavior, and package verification.

Current semantics and focused fixture commands are documented in [authentication](authentication.md), [tags and ignore](macos-git-actions.md), and [attribution, images and macOS conventions](macos-features.md). The [Liquid Glass investigation](liquid-glass-investigation.md) records the actual native prototype and compositing limitation; the integrated appearance remains opaque. The [previous macOS backend report](benchmarks/2026-09-08-macos-milestone-backend.md) identifies its source inputs and measurement scope independently of native frame evidence.

The authored [CI workflow](../.github/workflows/quality.yml) configures locked workspace tests, formatting, strict all-target Clippy and release compilation on macOS 15 and Ubuntu 24.04, using disposable mutation fixtures and no publication jobs. Record a hosted run separately when available. Local test/configuration validation is not a claim that either hosted runner passed.

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

A local macOS `.app` can be built with [the packaging script](../scripts/package-macos.sh). It is signed ad-hoc for local use, not notarized. The script's `--debug` option packages a debug build, while the default uses release; `--no-build` reuses the selected profile's existing executable.

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

- Ordinary history streams an immutable captured traversal in500-commit pages and retains a5,000-row/64MiB window, plus one selected inspection. Older continues forward; Previous replays and discards bounded earlier pages; Newest returns to the captured beginning. Refresh resolves current refs again. Repository-wide search is separate: it pins local tips or selected ancestry, supports cancellation and explicit continuation, and retains up to 10,000 matches or 64 MiB of metadata. Scan, byte, and time stops do not establish exhaustion.
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
