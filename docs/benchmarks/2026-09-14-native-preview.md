# Native preview investigation — 2026-09-14

The native captures establish a split-pane synchronization defect. Mismatched pane offsets fell from **161 of 195 baseline paint observations to 0 of 153 prototype observations**. The final installed executable also recorded **0 of 132 code-area and 0 of 134 gutter paint mismatches** on the physical Wayland desktop. These observations establish alignment, not higher frame rate, compositor presentation latency, or a text-rasterization change. The initial investigation and final acceptance runs are identified separately below.

## Build and environment

| Role | Identity |
| --- | --- |
| Previously installed application, inspected before changes | App source `7d18fef`, package source `326fa0f`; binary SHA-256 `276d3fa2e8fe259fc30c395ddea60b39da53f388714adb36d34360a35cc3e996` |
| Diagnostic baseline used for the numeric comparison | Archive of `dc95c15f5a19b3613ea846cc4c52c025affa1760`, with the retained [observation-only instrumentation patch](native-preview-20260914/baseline-instrumentation.patch); binary SHA-256 `e82f9f920a71a89162a775e4b204116a046199c0160146c0c7ffed47ea9f7443` |
| Diagnostic corrected prototype used for the numeric comparison and history observations | Binary SHA-256 `1025ce3f401f8d081daec369f1aa8035db5b2412b8ad50a0e5a71d9b880b38dc`; release build with immediate linked offsets and direct-gesture invalidation of old deferred requests |

The corrected prototype's embedded source revision/tree are `unknown`. Its temporary source manifest was subsequently overwritten by the build-root synchronization helper. The executable hash identifies the captured build, but this report does not claim that a complete immutable source manifest for it survives. In particular, it predates the final change that passes the **accepted, clamped** offset to visible-row layout. Commit `36886d6` contains that correction and four focused tests. The following preview build reports revision `013e3348a29735617982bfd217551fdf91e4c694`, tree `modified`; its final installed runtime results must be recorded separately.

Both diagnostic applications were optimized Linux builds using Rust 1.98.0 and the established Ubuntu 24.04 build root. The real desktop was Pop!_OS 24.04, Linux `7.1.5-76070105-generic`, GNOME Wayland, on an Intel Core Ultra 9 275HX. PCI inspection lists Intel graphics and an NVIDIA `2c58` device; the active rendering GPU was not established. The captured app reports a 1480 × 980 logical-pixel viewport, scale 2, Midnight theme, Comfortable density, interface size 13 and code size 12. Text line height was 18 logical pixels. No macOS, Windows, X11, different scale, or different compositor validation is represented here.

## Fixture and method

A disposable local repository contained commit `374af93d4e5f2affb045b32f1ff88456af86637c`, “Refine card labels and long-line layout,” compared with parent `9f15d2a9832523d1387f7f07b10bff01d135f91d`. The measured workflow selected this commit, opened `cards.rs`, chose Split, and scrolled the already loaded Before pane. The file has 12,000 lines, 1,020,150 bytes and maximum line length 85 bytes; its SHA-256 is `f03d344a81009373b6878b60286c62a0e0c191ac2c5e0efcf19d654a95655e79`.

The fixture also contains `long-lines.txt` with 3,000 lines, 675,798 bytes and maximum line length 1,722 bytes. Its presence does not establish that the long-line interaction was exercised. Numbered patch views deliberately disable soft wrapping to preserve line correspondence. Wrapping and long-line native coverage remain unclaimed here.

The matching gesture issued **240 axis events: 160 down, then 80 up**, each with portal `y=7.25` or `y=-7.25`, followed by a nominal 16 ms sleep. Every request waited synchronously for a local socket/desktop-portal response before sleeping. The app recorded deltas of −21.796875 and +21.796875 logical pixels in the code area. Consequently, “16 ms” describes the sender's sleep, not the delivered input cadence. [Gesture parameters](native-preview-20260914/gesture.json) and sender timestamps are retained.

`GITTURTLE_TRACE_SCROLL=1` observes existing input, editor paint, link updates and a canvas painted after both panes. It does not request frames or alter event propagation. Its stderr formatting, portal control, capture work and unrelated desktop load still add measurement overhead. The desktop capture used PipeWire through a converting appsink with a two-buffer dropping queue. Capture delivery intervals cannot identify actual compositor scanout or support a 120 Hz smoothness claim.

Only the **first 240 input observations** are analyzed. The baseline full log also contains 75 later unified-view inputs; the corrected full log also contains later gestures. Split-paint observations are included from the first input through the 240th input plus a 500 ms tail, inclusive. Offset mismatch compares the two observed x/y pairs numerically, treating signed zero equally. This measures same-paint pane alignment, not glyph sharpness.

## Captured results

All adjacent intervals are included: 239 input intervals per gesture, 194 baseline and 152 corrected split-paint intervals. Percentiles use nearest rank, `sorted[ceil(p × n) − 1]`. The earlier temporary measurement JSON omitted one adjacent paint interval; the retained analyzer recomputes every interval from raw observations.

| Observation | Diagnostic baseline | Corrected prototype |
| --- | ---: | ---: |
| Input span, first to last event | 4,911.602 ms | 4,289.607 ms |
| Input intervals: p50 / p95 / max | 19.748 / 46.001 / 50.195 ms | 19.074 / 33.508 / 42.705 ms |
| Split-paint intervals: p50 / p95 / max | 20.779 / 44.632 / 66.970 ms | 26.746 / 41.597 / 67.365 ms |
| Split-paint observations | 195 | 153 |
| Paints with different pane offsets | 161 (82.6%) | 0 |

These runs support the alignment correction. The median paint interval increased and the maximum was similar, so they support no throughput or general smoothness claim. Each condition has one matched gesture, with different delivered input cadence; this is diagnostic evidence, not a statistically controlled benchmark. Resting screenshots cannot establish smooth scrolling or exclude compositor artifacts. The retained raw numeric traces avoid publishing unrelated applications visible behind the baseline app window.

The observed failure matches the code path: a linked editor's programmatic setter originally deferred its offset until layout. Notifications could therefore publish stale state to the other pane, and a pending programmatic request could later restore an older offset after a fresh wheel event. Updating a laid-out viewport immediately and superseding deferred requests on direct input removed this mismatch in the prototype. The final code also preserves cold-editor layout behavior and clamps the requested viewport before visible-row layout. It changes no text rasterizer, font size, scale, antialiasing or wrapping policy. The preexisting geometry-only ancestor notification guard was present in both diagnostic builds.

## History observations and automated validation

The same corrected prototype showed one external local commit at the top with “1 new commit.” A subsequent burst of 65 commits exposed the latest row with a coalesced “66 new commits” cue while an older commit remained in the inspector. While reading older history, one further local commit changed the cue to “67 new commits”; all 19 visible row labels, their order, the selected commit hash and history-list focus remained identical. [Retained accessibility observations](native-preview-20260914/history-observations.json) support those results. No successful native Show latest, commit/View commit, search preservation, tab switching, branch rewrite, fetch, pull or push result is claimed by this initial report.

The coordinator ran these checks in the complete Ubuntu build root; their [logs and hashes](native-preview-20260914/manifest.json) are retained:

```sh
cargo test --locked -p gitturtle local_refresh::tests
cargo test --locked -p gitturtle history_updates::tests
cargo test --locked -p gitturtle scroll_tests
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

The focused logs report 14 watcher, six history and four scroll tests passing. The combined workspace run reports no failures, including 365 app tests and 121 preview tests passing, plus the core suites. Five tests across the workspace were ignored: installed `gh` isolation, the opt-in review benchmark, GnuPG signing, local `sshd` authentication and the SVG mutation probe. Clippy completed successfully. These are test/build results, not substituted native or cross-platform observations. No checks were rerun to write this report.

## Retained data and pending coverage

The [evidence manifest](native-preview-20260914/manifest.json) hashes the original logs at extraction, unchanged selected trace lines, sender timestamps, instrumentation patch and check logs. [measurements.json](native-preview-20260914/measurements.json) contains every interval and observed offset. Recompute without launching the app:

**Redacted 2026-09-24.** The profile button in `corrected-cards-code-crop.png`, `corrected-long-horizontal-verified.png` and `corrected-selection-rest.png` showed the maintainer's name; it is filled with the surrounding background and no other pixel changed. `corrected-show-latest.json` and `final-compare-update.json` replace only that name with `REDACTED` (the profile label and one fixture commit's author). `manifest.json` hashes the original files, which remain in Git history.

```sh
python3 docs/benchmarks/native-preview-20260914/analyze-scroll.py \
  docs/benchmarks/native-preview-20260914/split-before.log \
  docs/benchmarks/native-preview-20260914/split-after.log
```

The initial prototype's later capture filename suggests a gutter gesture, but its recorded input area is `code`; only the final run below establishes gutter coverage. Smoothness beyond the alignment result requires presentation-aware evidence; no unchecked platform or compositor is marked validated.

## Final build, physical desktop and installation

The installed release is **0.1.0 Preview**, source `417b5e8d0c739ca3746d2cab379db58efa4d8ea9`, executable SHA-256 `45606c5a195d1696096b93ea0fa3a38b67b025990ea794f25f91d4211e5729c2`. It includes the accepted-offset clamp and the final watcher sibling-event correction. All native inputs were committed. The embedded tree state is honestly `modified` because documentation and website work were untracked when the complete source snapshot was made; this is not a clean-source release claim. The retained build-root source manifest SHA-256 is `50c622c5a1cc40cab1f65d608c229f30d629ed202b638a96c939817b8024c4ad`.

The same executable was exercised directly on the physical Pop!_OS/GNOME Wayland display, then installed with `scripts/package-linux.sh --no-build --binary …` and the bundle's `install.py`. The compiled identity and executable checksum agree before packaging, in the package, and after installation. All bundle checksums passed. The archive is retained locally at `.local/public-launch/gitturtle-linux-x86_64-417b5e8.tar.gz`; it is not a published download. Its collected notices still identify two unresolved dependency texts, so this is a local preview installation, not a cleared public binary release.

The previous installed app was closed normally after confirming it had no child Git operation. Its previous files remain in `~/.local/share/gitturtle/install-backups/1789422696270865645-qkd8diea`. After quitting GitTurtle, rollback is:

```sh
python3 ~/.local/share/gitturtle/install.py --rollback
```

Preferences were byte-identical before and after installation. Normal shutdown updated the repository-session bookmark; all four genuine repository tabs returned. The GitTurtle checkout opened with the new commits visible and no previous watch-limit banner. The [About dialog](native-preview-20260914/installed-about-modal.png) and [copied diagnostics](native-preview-20260914/installed-diagnostics.txt) identify the actual installed process, Wayland, scale 2, Midnight/Comfortable, and 13/12 text sizes. A real pointer click was used for clipboard verification: an accessibility-only click changed the button feedback but lacked a fresh Wayland input serial for replacing the system clipboard.

### Final physical scroll and input checks

Two separate 240-event gestures used the same `cards.rs` fixture and down/reverse pattern. The first targeted code; the second recorded `area=gutter` for every input. The observation window for each ends 500 ms after its final input. [Raw traces and numeric measurements](native-preview-20260914/corrected-cards-measurements.json) retain every interval, with nearest-rank percentiles.

| Observation | Code area | Gutter |
| --- | ---: | ---: |
| Inputs | 240 | 240 |
| Split paints / x-or-y mismatches | 132 / **0** | 134 / **0** |
| Input intervals p50 / p95 / max | 22.787 / 35.868 / 43.484 ms | 21.723 / 34.332 / 42.726 ms |
| Paint intervals p50 / p95 / max | 28.898 / 44.238 / 47.653 ms | 28.051 / 44.853 / 57.216 ms |

The [resting code capture](native-preview-20260914/corrected-cards-code-crop.png) has matching line numbers and crisp resting text. Ctrl+F, a real `Card` query, Enter match navigation, Escape back to source, Home/Shift+End selection and Ctrl+C worked. The [selected source](native-preview-20260914/corrected-selection-rest.png) copied exactly one literal source line without gutter text. Typing into that read-only selection and copying again produced identical bytes in the retained copy files.

The 3,000-line long-line fixture was opened in Split and Unified, vertically scrolled, and horizontally scrolled in Split. [The final unobstructed horizontal capture](native-preview-20260914/corrected-long-horizontal-verified.png) shows clipped long text with fixed, aligned gutters and independent horizontal offsets. Soft wrapping is intentionally disabled in these numbered patch/source views; no wrapped-editor result is claimed. Another application briefly covered the later long-line run, so its numeric timing and occluded screenshot are excluded from the clean benchmark. No generalized smoothness or frame-rate gain is inferred from that run.

While the final app was inspecting `374af93` in Compare, an external disposable empty commit produced **1 new commit · Show latest**, retaining the file, comparison and focused changed-file row. Clicking Show latest exposed the actual current local tip `169bfa9` while the inspector still held `374af93`. Creating, renaming and removing a sibling directory caused no degraded-watch status in this final app. The installed app's actual large checkout likewise retained automatic refresh.

### Supplemental native X11 workflow

The preceding release executable, source `013e3348a29735617982bfd217551fdf91e4c694`, SHA-256 `164a93b7100ad9be311645f680bb6c7d76ebb6dde8cfea520a6c7e41d098e47c`, was exercised in the complete Ubuntu 24.04 userspace with Xvfb/Openbox at scale 1. The final `417b5e8` change only corrects sibling sentinel event classification; the Git writes, history UI and rendering inputs are identical. These are real GPUI interactions on a virtual display, not a physical Ubuntu/GNOME certification.

In the disposable Aurora repository, native actions staged `notes/preview-check.md`, committed it as `159f91a`, and displayed success with View commit while leaving an unrelated `App.tsx` edit unstaged. A next-commit draft survived View commit and Back. Native staging and committing of `App.tsx` produced `6631c6`; native Push advanced the local bare remote. A separate disposable peer then committed `e82a2b1` to that remote. Native Fetch updated the tracking ref and native Pull fast-forwarded the worktree. Independent Git inspection confirmed HEAD, tracking ref and bare-remote tip all equal `e82a2b1`, with a clean worktree ([results](native-preview-20260914/native-git-results.json)). These operations used local filesystem remotes, not hosted authentication or Internet transfers.

Show latest reached the freshly pulled tip without changing the inspector's selected commit. A captured `Refine card` history search remained in place through comparison navigation and a new external commit. The native GTK folder picker opened the existing linked worktree in a third tab, with branch `review/accessibility`; it required switching out of the empty Recent view before its Open button became available. Initially misconfigured virtual portal activation was corrected by exporting display settings before service startup; that earlier failure is not counted as an app failure or a passed interaction.

### Final checks and remaining limits

After the last Rust change, the locked workspace suite passed **729 tests, 0 failures, 5 ignored** (367 app, 241 core, 121 preview); the opt-in/host-dependent ignores remain explicit in the logs. Strict all-target workspace Clippy and release build passed. Four rendered scroll regressions cover burst setters, direct-wheel precedence, clamping/visible rows and cold layout. Seven isolated installer tests cover upgrade/rollback, corrupted checksums, interrupted upgrades, running-executable refusal and settings preservation. Formatting and focused website checks passed; documentation updates did not trigger another native rebuild.

No new macOS, Windows, fractional-scale, mixed-monitor, alternate physical compositor or hosted-CI pass is claimed. There is no compositor presentation-time or memory-peak benchmark. The fix adds no unbounded history/editor buffers and retains existing preview limits. Public binary publication, missing license texts, platform-specific signing/notarization, and Cloudflare publication remain separate release steps in [the release checklist](../public-launch.md).
