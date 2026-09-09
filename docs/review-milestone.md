# Review, navigation and recovery milestone

Completed September 9, 2026, starting from `e85883b` on September 8. All ten feature areas are implemented and integrated, the affected native usability defects are corrected, and the verified final release is installed and running at `/Applications/GitTurtle.app`. Checked entries describe the evidence below; they do not imply unavailable platform, accessibility or hosted-account checks passed.

The scope remains Rust/GPUI, macOS first, with the six themes, turtle artwork, opaque appearance, existing workflows, preferences and drafts preserved. Development mutations use disposable repositories and local remotes. Public distribution, notarization, hosted publication, framework migration and Liquid Glass are outside this milestone.

## Feature acceptance

- [x] **1. Revision comparison:** searchable local Before/After targets, endpoint or merge-base comparison, swap/direction, pinned identities, retained navigation and explicit missing/unrelated/ambiguous cases. Comparisons remain asynchronous and read-only.
- [x] **2. Text review:** intraline highlighting, explicit whitespace filter, change controls/shortcuts and bounded context expansion. Literal copy, Find, syntax, gutters, split/source views and exact partial staging are preserved; filtered/expanded staging combinations explain their limits.
- [x] **3. Text size and accessibility:** independent interface/code preferences and resets, scaled rows/editors/gutters, focus and semantic improvements. Available system-setting and VoiceOver attempts are recorded with limits. Large-text layouts were corrected and verified at the minimum window size.
- [x] **4. Quick Open and path filters:** keyboard tracked-file search in worktree/revision scopes, source/history/blame actions, retained context and bounded background discovery/filtering. History and Working Changes reuse cached background path indexes. Escape and modal return focus were corrected and verified.
- [x] **5. Multi-file staging:** mouse/keyboard/range selection, visible scoped stage/unstage, directory grouping and predictable selection clearing/reconciliation. Exact literal targets and unrelated staged/unstaged work are preserved.
- [x] **6. Worktree management:** reviewed existing/new-branch creation, application/Finder/editor handoff and reviewed removal, with dirty/locked/missing/main/occupied/stale protection. Removal does not imply branch deletion.
- [x] **7. Activity and recovery:** bounded secret-free operation outcomes, targets and timing; read-only reflog inspection and explicitly reviewed recovery branches. Expiration, missing objects and uncertain outcomes are explained without automatic retries.
- [x] **8. Conflict blocks:** named sides, block navigation/decisions, manual result editing, retained drafts and revalidated explicit save/stage. Continue/Abort/Keep files and unsupported-content fallbacks remain available.
- [x] **9. Interactive rebase:** reviewed linear sequences with reorder/reword/squash/fixup/drop, keyboard alternatives, hooks/signing and stale/publication safeguards, message/conflict pauses, continue/abort/resume and intermediate failure/cancellation coverage.
- [x] **10. Missing LFS previews:** explicit object/source/size review, scoped download, authentication/progress/cancel, verified identity/size and bounded decoding. Passive browsing does not fetch or rewrite the index, pointer or working file.

## Evidence and completed checks

[Native verification and screenshots](review-native-verification.md) attributes each observed workflow to its exercised build. Final source `0c8eaec31ae9fdbc7fe7f98c5822cc0561f96ad4` produced release `B003BB92-D6C0-3009-8A75-96A85EEBB133`, which passed the focused correction checks and installed-delivery verification. Earlier builds retain their own evidence; the final pass does not relabel all prior checks as repeated.

- [x] **Behavioral fixtures:** comparison/tracked paths, exact text/partial staging, preferences, selection/filtering, activity, conflict drafts, worktree/reflog safeguards, rebase interruption and scoped LFS success/refusal/cancellation passed. Focused semantics and evidence are in [interactive rebase](interactive-rebase.md), [conflict blocks](conflict-blocks.md), [parallel work/recovery](parallel-work-recovery.md), [LFS previews](lfs-previews.md) and the native record.
- [x] **Final quality gates:** `cargo fmt --all -- --check`, `cargo test --locked --workspace`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, and `cargo build --release --locked -p gitturtle` passed on the final executable inputs at `0c8eaec`. The existing transitive `block 0.1.6` future-compatibility notice is not a new failure. Documentation and screenshots were finalized afterward without changing compiled inputs.
- [x] **Affected-path release measurements:** [core review report](benchmarks/2026-09-09-review-backend.md) and [app preparation/filter report](benchmarks/2026-09-09-review-app.md) retain raw samples, source/executable hashes, hardware, cache/load conditions and tails. Each series has three warmups and forty measured calls; the app report includes process-memory observations. These CPU measurements do not establish speedups, native-frame latency, queue responsiveness or long-running memory growth.
- [x] **Authentication and access-dependent verification audit completed with limits:** [authentication evidence](authentication.md#verification-and-limits) records three real local signing/SSH fixtures, three HTTP/helper integration tests and six authentication unit tests. Generated isolated SSH-agent and OpenPGP keys signed real commits/tags; loopback SSH Fetch retained strict host verification. Signing failure did not trigger unsigned fallback. Helper rejection and cancellation were exercised.
- [x] **Workflow and environment audit completed with limits:** the [macOS/Linux quality workflow](../.github/workflows/quality.yml) was inspected and Linux SSH-client prerequisites added. There is no configured source remote or authorized hosted destination at this checkpoint, so no hosted CI run URL/result or live-provider/LFS result exists. Linux compilation/native interaction, actual Keychain unlock, hardware-backed signing and native pinentry remain unverified. Apple `osxkeychain` and a login Keychain were present; stored items were not accessed.

Increase Contrast/Reduce Transparency and system Light/Dark transitions were exercised, and original system settings restored as described in the native record. VoiceOver was enabled and navigation attempted, but no observable speech/cursor confirmation was obtained; successful spoken navigation is unverified. The pinned GPUI disabled-button path does not emit the AccessKit disabled flag, although visual state and activation guards work. These specific limits remain explicit rather than being recorded as passed accessibility checks.

## Durable increments

| Source | Completed increment |
| --- | --- |
| `75cdb8a` | Finite milestone plan and baseline/native inspection record. |
| `defbd9a` | Core comparison, rebase, recovery, worktree and scoped LFS services with safety fixtures. |
| `06c2e7c` | Integrated native review, file navigation and recovery workflows; full quality gates and release package used for the main native pass. |
| `63ffac6` | Wrapped large-text sidebar navigation and reproducible release evidence; full gates and focused minimum-window native verification. |
| `57606a9` | Corrected Compare/Quick Open cancellation, comparison inspector bounds and minimum-size Working composition. |
| `d296cd9` | Restored attached focus and explicit redraw after review dialogs close. |
| `0c8eaec` | Bounded tall review dialogs and deferred Working layout repaint; final gates, native checks and installed release. |

The original saved application state was backed up before native QA and restored before installed launch. All original settings, recent projects, columns and drafts were verified preserved afterward; preference serialization added only the new default interface13/code12 settings. Only this milestone's 15 disposable-fixture activity entries were removed after backing them up. The previous installed app was also backed up locally. Previous milestones retain their original [build attribution](macos-native-verification.md).

## Final acceptance and delivery

- [x] Correct and verify interface-size-18 minimum-window Working Changes file-list space, automatic resize settling, scrolling composer and revision-comparison header layout.
- [x] Correct and verify Compare/Quick Open Escape and return focus, including focused inputs and modal cancellation; verify rebase keyboard reorder, reachable controls and Close return focus.
- [x] Complete the final combined gates and release/package verification after production corrections. Update representative screenshots and attribute each check to its source/build.
- [x] Complete meaningful local implementation and documentation increments. Install the verified bundle, launch that exact path, verify executable identity and essential interactions, and restore original preferences while preserving drafts.

| Final delivery identity | Result |
| --- | --- |
| Final compiled source commit | `0c8eaec31ae9fdbc7fe7f98c5822cc0561f96ad4`; later evidence/docs edits do not change executable inputs. |
| Release, package and installed executable UUID | `B003BB92-D6C0-3009-8A75-96A85EEBB133` (arm64) |
| Packaged and installed executable SHA-256 | `0f346ccc7a7149d5314cebc5c0893ee182d166e2c569c6b5051906b57dc69777` |
| Installed verification | `/Applications/GitTurtle.app`; plist and strict/deep local ad-hoc signature passed. PID 61160 ran that exact executable. Original project reopened in History; visible Quick Open search, Escape, Settings and Back passed. |
| State preservation | Original Nord/Comfortable, follow-system off, editor/default-branch/reopen settings, column layout, all ten recent projects and saved drafts preserved. New sizes default to interface13/code12. Original macOS Dark, contrast/transparency off and VoiceOver off restored. |
