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

Clipping: the ring is whole on the History segments, Workspaces, dialog footers, the Delete alert, the banner's Dismiss, Stage and the Your themes rows. Its 3 px footprint equals the toolkit ring's, but three containers cut it. The repository tab strip (`repository_tabs.rs`, an `overflow_x_scroll` list that GPUI masks on both axes) leaves a focused repository tab only its right arc and one left column, and cuts its close button's top and bottom. The Tags dialog body cuts the top of Create tag…. On base those three still showed #66's band inside the edge, so they now show less focus than base ([`porcelain-1000x680-clip-repository-tab.png`](evidence/themes/button-focus-ring/porcelain-1000x680-clip-repository-tab.png) against [`base-porcelain-1000x680-clip-repository-tab.png`](evidence/themes/button-focus-ring/base-porcelain-1000x680-clip-repository-tab.png)). The Tags and Reflog lists cut the sides of their full-width rows on both builds; a focused Tags row keeps only corner stubs. An Input's clear button takes no focus. A pre-existing trap on both builds: once a focused Switch turns disabled, neither Tab nor Shift+Tab moves focus off it until a pointer click.

Frames: [`evidence/themes/button-focus-ring/`](evidence/themes/button-focus-ring/), 68 files. They are the candidate at every captured state; the unfocused references; crops of each clipping container; and the base's three regressed crops and Solarized Light pair. A privacy scan of every proposed frame with the local template set came back clean. A `design-reviewer` pass accepted the ring, the selected hover and the disabled-hover pair. It asked for the clipped containers to be listed as open in `DESIGN.md` and the spec, which they are, and for a follow-up that gives the tab strip, the Tags body and the Tags and Reflog lists room for the footprint, starting with the repository tab. It also noted that the density and project-mode segments (`settings.rs`, `projects.rs`) still dim to 0.9 when selected and hovered, so two kinds of selected segment now answer the pointer differently. [`helper-focus-ring/`](evidence/themes/helper-focus-ring/) stays as the superseded #66 record.

Still open: macOS rendering, fractional scale factors, the clipping containers above, the two 0.9 dims, the keyboard trap, and the 49 committed frames that `button-focus-ring-recapture` retakes under the new ring.

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
  - the disabled Follow system thumb stays at full strength;
  - syntax colours on changed lines;
  - fixed colours on the first `@@`, `---` and `+++` lines, which predate this change;
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
- 30 committed frames that show a focused helper predate the band and were not recaptured, because they need the FileChooser portal or the no-portal flow: `themes/import-export/transfer-1000x680-{06,07,08,11,13…25}`, `transfer-1440x900-{07,13,21,25}`, `collision2-1000x680-02-collision-2`, `lotus-1000x680-01-refuse-notjson`, `noportal-1000x680-{01-export,03-import}-guidance`, `themes/linux-gaps/imported-{midnight,porcelain}-1000x680-focus-{beside,on}-highlight` and `button-states/kanagawa_lotus-1000x680-imported-button-hover`. The new `changes-stage` frames show the helper on the selected surface that the import-highlight frames showed.

Helpers that a caller turns into another variant also get the band. On the danger fill of the Delete theme confirmation it reads at most 1.50 in Porcelain and 1.68 in Midnight. On a primary button such as the editor's Save it is accent on accent, and only 48 corner pixels change. Both still rely on the toolkit's half-opacity ring, which reads 2.27 in Porcelain (unchanged) and 3.74 in Midnight. In Kanagawa Lotus the Changes Stage helper reads 3.73 focused on the selected row, and 2.71 focused and hovered: the documented row exception, where the base ring alone models at about 1.75. A focused helper that is disabled paints no band: the editor's `saving` frames, whose disabled Save shows a ring, are identical to base.

The design reviewer accepted the change. At 4x and 12x the band reads as the crisp inner edge of a two-tone ring on every surface, and the 7 px corners follow the arc without gaps. All 19 replacements were accepted; the reviewer's own diff found nothing else changed. On the danger fill the line is continuous and clear of the label and reads as part of the ring, but it adds no contrast and slightly muddies the corners (mauve in Porcelain, grey in Midnight), which a follow-up may revisit. The primary button was accepted unchanged, and the row exception was accepted as documented because it holds only while the pointer is on the button.

Still open: macOS, where the inset shadow, the first in the app or toolkit, has not been rendered; scale factors other than 1, where the band is one device pixel and can be under one logical pixel; primary, danger and directly built borderless buttons, whose half-opacity ring alone is below 3:1 in 16 of 20 palettes, and the row exception, both needing an outside stroke through a vendored Button change; the 30 frames listed above; and the focused disabled helper. The theme draw-cost driver finds row 1's Edit… by the old ring's pixels ([benchmark notes](benchmarks/2026-09-22-theme-draw-cost.md)) and may need re-pointing before its next run.

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
