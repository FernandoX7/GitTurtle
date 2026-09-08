# Validation notes

This records local validation performed on September 7–8, 2026, with each stage tied to its exercised builds. The design and everyday Git workflow checks extend the historical history/comparison checks. Native interaction was exercised on macOS. Linux is a portability target, not a validated release platform; no CI or notarized distribution is claimed.

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
| Project opening and drafts | Opened a repository through its nested `src` folder using the native picker and retained the existing commit draft. Draft retention applies within the running session. |
| Create and first commit | Created an unborn repository on `trunk` and made its initial commit `cc69432`. |
| Staging and comparisons | Staged and unstaged the same file and verified that selecting its two groups showed the different staged and unstaged content. Image checks exercised Before/After, Fit, 200% zoom, and dragging. |
| Commit and push | Created commit `b54e318` on `main`, pushed to a local bare remote, and verified that the remote had the same OID. |
| Fetch and pull | A fixture peer created `0d6cbaa`; Fetch showed the local branch one commit behind, and fast-forward Pull updated HEAD to the peer commit. |
| Branch actions | Created/switched to `qa/native-workflow`, then switched to `main`; the remote-branch input tracked the selected local branch. |
| Clone and failures | Refused a nonempty destination. A missing-LFS smudge failure surfaced the normal Git error and preserved the partial destination. After the fixture peer removed the intentionally missing pointer, a complete clone into `native-clone-ready` succeeded. |
| Final Settings focus check | From a working-file preview, Command-, opened Settings. Switching to Graphite and pressing Escape restored the same README unified comparison and file-list focus; the editor remained read-only. |
| Final commit feedback | Staged and committed `61828ae` with the message “Verify final native workflow.” The result displayed the short OID and summary on one line, cleared the commit message, and showed a clean working state. |

Pull is fast-forward-only and Push does not force-update refs. These network-action checks used local remotes; remote authentication was not validated. Commit drafts are session-only. Linux interaction/builds, notarization, and remote credential flows remain outside this evidence.

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

These are manual checks of the initial native build, not an exhaustive platform or accessibility certification. The [design document](design.md) includes intended behavior beyond the implemented surface.

To create a fresh disposable demonstration repository:

```sh
python3 scripts/create-demo-repo.py
cargo run --release --locked -p gitturtle -- .local/demo-repository
```

The generator also creates a linked worktree. It refuses nonempty destinations, including existing repositories. For another run, choose a fresh destination with `--output /path/to/empty-or-new-directory`; do not point it at a working repository.

## Timing methodology

Initial backend observations, hardware/toolchain details, and the reproducible inspection command are recorded in the [Git service benchmark notes](../crates/git-core/README.md#initial-measurement-september-7-2026). The backend harness measures Git service work. It excludes UI dispatch, queuing, image decode, editor preparation, rendering, and presentation. Filesystem caches were not flushed, so those observations are not cold-disk results.

The native app has a separate optional trace:

```sh
GITTURTLE_TRACE=1 target/release/gitturtle /path/to/repository
```

`gitturtle.commit_files_frame_ms` now measures a History selection through its changed-file list frame. `gitturtle.file_preview_frame_ms` measures an explicit file activation through its prepared comparison frame. History selection does not eagerly prepare a file preview. Returning to History uses retained state without a new Git request. Both metrics start in the application handler and use a generation- and mode-checked GPUI callback; they exclude input delivery before the handler, OS presentation, and completed GPU execution. Superseded interactions emit no sample.

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

## Known limits and follow-up

- The UI loads 500 commits initially and adds 500 per **Load more**, up to 10,000. It reloads the full prefix and restores the selected commit/preferred file when possible, centering the history selection. It does not retain an incremental history traversal cursor. Search covers loaded commits.
- Refresh is manual and resolves the selected scope's current tip without fetching. Fetch, fast-forward Pull, and non-force Push require explicit actions; the current native checks use local remotes. Missing preview objects are reported without automatic downloads.
- Staging is whole-file. Conflict resolution, merge/rebase editing, force push, and submodule management are not provided. Commit drafts survive navigation within a session but are not persisted across restarts.
- Text uses a unified patch plus Before/After tabs. A separate old/new gutter accompanies unified patches; there is no aligned split view. Parent controls expose the first 128 parents of unusually large merge commits with an explicit count notice. The UI's initial file list omits rename detection, displaying additions and deletions instead.
- Images use a first-frame preview capped at a 1,600-pixel edge. Zoom percentages refer to that decoded preview; original dimensions and reduced preview dimensions are shown. SVG filters and embedded/external images are explicitly unsupported.
- Graph preparation allows at most 128 simultaneous lanes, 200,000 edges, and 200,000 parent entries. Above the budget, the UI explains why connections are hidden and shows isolated nodes instead of incomplete ancestry lines.
- The preview cache is limited to 32 entries and 128 MiB of retained CPU content allocations. UI-held references and GPU resources have separate lifetimes. Active decoder work stops at cooperative checkpoints; input and allocation limits are not a process sandbox or a hard end-to-end deadline.
- Native Linux builds and interactions, Linux packaging, automated CI, and notarized macOS distribution have not been validated or provided.
