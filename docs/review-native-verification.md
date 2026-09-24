# Review milestone native verification — September 8–9, 2026

All ten feature areas in the [milestone ledger](review-milestone.md) are integrated, and the final verified release is installed at `/Applications/GitTurtle.app`. This record attributes native observations to their captured build identities and states the remaining environment/framework limits. All Git mutations and local transfers used disposable repositories, worktrees and local remotes. Existing user repositories were inspected passively. No hosted publication or distribution was performed.

## Builds and environment

Native checks ran on an Apple M4 Max, 128 GiB RAM, macOS 26.6.2 (25G83), arm64. The opaque GPUI application and existing turtle artwork remained in use; Liquid Glass was not revisited. Local development packaging is separate from installation verification.

| Exercised build | Source and observed scope |
| --- | --- |
| Intermediate debug `CAA0DAE9-9CC9-3F71-B3C2-95513CC6D331` | In-progress source. Quick Open immediate typing and source inspection; native linear rebase using reorder controls and Reword/Squash message pauses. This build exposed the Option-arrow binding conflict. |
| Integrated debug `19CF9A54-B217-3B1A-B578-24BAA90EFC7D` | In-progress integration before the release pass. Fixed Quick Open typing/Return/Escape, exact multi-file staging, text review/reset, Split/Find/copy and filtering. |
| Release `D16FD0B7-EF7B-3BFB-B084-B12468C9FF62` | Packaged from `06c2e7cbca165342e3109615e7bd055136949b28`. Conflict blocks and retained drafts, local LFS success/cancellation, activity, worktree safety/handoff, independent text sizes and available macOS accessibility settings. |
| Focused release `FDADF883-9C30-3B66-9C23-5D0BB9F9B149` | Packaged from `63ffac64075a9143469d3ba6d8981771a7bfc0b6`. The corrected Worktrees sidebar tab wrapped visibly at interface size 18 and the 1,000 × 680 content minimum; the full captured window is 1,000 × 712. This pass does not repeat the entire earlier matrix. |
| Focused release `02857B90-6809-337A-9A93-B675E619385C` | From `d296cd93aa990b28b5c76b6047970ee54d6b2ae1`. Corrected modal redraw/cancellation and attached return focus, comparison bounds, tracked source/history/blame and rebase keyboard behavior. |
| Final installed release `B003BB92-D6C0-3009-8A75-96A85EEBB133` | From `0c8eaec31ae9fdbc7fe7f98c5822cc0561f96ad4`. Final tall-dialog bounds and automatic Working layout repaint, focused native checks, installed identity and preserved preferences. |

The final `0c8eaec` executable inputs passed `cargo fmt --all -- --check`, `cargo test --locked --workspace`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, and `cargo build --release --locked -p gitturtle`, as recorded in the milestone ledger. The existing transitive `block 0.1.6` future-compatibility notice remained. Only documentation and screenshots changed afterward. Installed identity was checked separately below.

## Native workflow observations

### Intermediate and integrated debug checks

- Initial native inspection exercised endpoint comparison, Quick Open, worktree safety, activity and reflog recovery. Quick Open focus, duplicate acceptance, source labeling and retained-preview defects were corrected during iteration. These early observations did not capture a final-release identity.
- A failed worktree checkout caused by missing LFS content retained its newly created branch. Independent Git/filesystem inspection confirmed there was no resulting worktree destination and current work was unchanged. The UI was updated to describe that partial result rather than imply full creation or silently clean it up.
- Reflog recovery created a new branch at the selected commit while preserving HEAD, index and working content. This is explicit recovery-branch evidence, not universal undo or recovery of expired objects.
- The `CAA0…` debug package accepted Quick Open typing immediately and displayed a tracked source with direct File History/Blame actions. Native rebase reviewed three commits, reordered with Move up/down, selected Reword/Squash, and used native message pauses through completion. Edited message text survived closing/reopening the message review. Option-Up did not reorder in this build because the global text-navigation binding intercepted it; dedicated sequence bindings were subsequently added. Release `C984…` later verified both Option-Up and Option-Down changed the selected plan row order; HEAD stayed unchanged and the clone stayed clean. Closing the plan then exposed a separate return-focus defect, corrected in the final build.
- The `19CF…` debug package verified Quick Open typing, Return and Escape back to History. A two-file keyboard range was staged; independent Git inspection confirmed exactly those index changes and preservation of the unrelated file. Filtering cleared hidden selections.
- The same integrated debug pass exercised whitespace suppression, context expansion, Split/Find and literal source copying without gutter text. Reset restored the original Git diff, after which staging the whitespace-only hunk changed exactly that hunk. This is native interaction evidence for the exercised text fixture, not an exhaustive Unicode/line-ending or large-input matrix.

### Release `D16F…` checks

| Workflow | Observed interaction and independent result |
| --- | --- |
| Conflict blocks | Inspected block/base/current/incoming content, accepted Both and Incoming, and retained the result through a pinned-revision Quick Open detour. Save draft changed working content while retaining all three unmerged index entries. Manual editing followed by explicit Save and stage resolved the selected file. Reviewed Continue completed a two-parent merge with a clean worktree. |
| Explicit LFS success | Before acceptance, the fixture had no `.git/lfs` directory. The reviewed 508-byte SHA-256 object downloaded from an isolated local bare source, verified and rendered as a 640 × 420 SVG. Independent byte comparisons confirmed unchanged index and pointer working file. |
| Explicit LFS cancellation | A second fixture's slow transfer adapter was observed running before native Cancel. After cancellation the adapter was gone, no object was cached, and index/working-file hashes were unchanged. This does not establish cancellation behavior for every hosted transport. |
| Activity | The native list showed success and cancellation with repository, object/remote-name target, time and controlled explanations. Cancellation retained an inspect-state warning and did not start an automatic retry. |
| Text size | Interface size 18 and code size 24 changed independently with live samples and saved settings. Switching repositories through Projects retained those values. Additional reset/theme checks are recorded below. |
| Worktree inspection and handoff | The manager exposed main/dirty protection and a clean linked target. Finder selected the exact inspected worktree. An unconfigured Editor action opened Settings with the relevant configuration path. |
| Worktree removal | Making the inspected clean target dirty caused the old Remove review to be refused, preserving its new untracked file. After deleting only that disposable test file, a fresh reviewed removal removed the worktree and retained its branch. Original staged/unstaged files were unchanged. |

The large interface size exposed a clipped Worktrees sidebar tab. Source `63ffac6` wraps the tab row; release `FDAD…` verified the correction at the minimum window. The earlier worktree-manager screenshot still shows the pre-correction sidebar and belongs to its original pass.

## Screenshots

These are captured native JPEGs. Screenshots establish the visible state; the mutation/preservation results above rely on the separately recorded Git and filesystem checks.

| Capture | Build and visible state |
| --- | --- |
| [Conflict blocks](evidence/review-milestone/conflict-blocks.jpg) | `D16F…`, 1,123 × 768. Named sides, one remaining block, accepted text in the result draft, and distinct save/stage controls. |
| [Downloaded LFS preview](evidence/review-milestone/lfs-download.jpg) | `D16F…`, 1,123 × 768. Verified-download notice, absent Before side and rendered 640 × 420 SVG. |
| [Operation activity](evidence/review-milestone/activity.jpg) | `D16F…`, 1,123 × 768. Cancelled and successful LFS transfers plus completed conflict operations, with explicit outcomes and targets. |
| [Independent text sizes](evidence/review-milestone/text-size-settings.jpg) | `D16F…`, 1,123 × 768. Interface 18 and code 24, samples, reset controls and saved-settings feedback. |
| [Worktree manager](evidence/review-milestone/worktree-management.jpg) | `D16F…`, 1,123 × 768. Selected clean linked worktree, branch/path/state and handoff/removal controls. The underlying clipped sidebar was corrected afterward. |
| [Large-text minimum History](evidence/review-milestone/large-text-minimum-history.jpg) | `FDAD…`, 1,000 × 712. Interface size 18 with the Worktrees tab on a visible second line at the minimum content size. |
| [Large-code split review](evidence/review-milestone/large-code-split.jpg) | `FDAD…`, 1,124 × 768. Code24, aligned gutters and Unicode intraline changes after keyboard navigation. |
| [Follow-system Light](evidence/review-milestone/system-light.jpg) | `FDAD…`, 1,124 × 768. Actual system Light appearance selected while Follow system is enabled. System Dark was restored afterward. |
| [Minimum-size revision comparison](evidence/review-milestone/revision-comparison.jpg) | `0285…`, 1,000 × 712. UI18/code24; scrolled pinned identities, accessible file list and an added file preview. |
| [Minimum-size Working review](evidence/review-milestone/minimum-working-review.jpg) | `B003…`, 1,000 × 712. UI18/code24, selected last working row, visible file list, text preview and reachable commit footer. |
| [Minimum-size Quick Open](evidence/review-milestone/quick-open-minimum.jpg) | `B003…`, 1,000 × 712. Bounded results with Cancel/Open controls visible at UI18. |
| [Keyboard rebase plan](evidence/review-milestone/rebase-keyboard.jpg) | `B003…`, 1,000 × 712. Option-Up reordered the selected row; the body scrolled to shared-history warning, review and Close controls. No rewrite was started in this focused check. |

**Redacted 2026-09-24.** `rebase-keyboard.jpg` showed the maintainer's name in the account label. It is filled with the surrounding background, and the image was re-encoded as JPEG at quality 95 with no other edit; the original remains in Git history.

## Accessibility and system settings

Increase Contrast was changed from off to on in macOS, enabling Reduce Transparency with it. The opaque History surface at interface size 18 remained stable and readable during inspection. Turning Increase Contrast off restored both original off values. This is an observed system-setting interaction, not a specialized high-contrast palette or a complete contrast audit.

VoiceOver was enabled through System Settings and the actual **Use VoiceOver** startup action; a VoiceOver process ran. Two Control-Option-Right navigation attempts produced no observable cursor, speech or caption output through the available automation surface. **Successful spoken navigation was not established.** VoiceOver was restored to off. Accessible names/roles and ordinary keyboard interaction are separate evidence and cannot substitute for spoken-navigation confirmation.

The pinned GPUI dependency has a specific disabled-state limitation: disabled buttons remove focus/click handling and render disabled styling, but the current property path does not emit AccessKit's disabled flag. Its dependency accessibility test documents that behavior. GitTurtle's activation guards remain effective; native semantic disabled announcements are unsupported/unverified. The dependency versions are pinned in [Cargo.lock](../Cargo.lock), and the milestone ledger records this limitation without claiming successful VoiceOver use.

## Backend, authentication and performance evidence

Behavioral fixtures cover cases beyond the native sessions above. [Interactive rebase](interactive-rebase.md) documents stale plans, hooks/signing refusal, conflicts and intermediate cancellation; [conflict blocks](conflict-blocks.md) covers marker styles, byte preservation and stale saves; [parallel work/reflog](parallel-work-recovery.md) and [LFS previews](lfs-previews.md) record safety and failure fixtures. A backend pass does not establish its corresponding native interaction.

[Authentication/signing evidence](authentication.md#verification-and-limits) includes generated SSH-agent commit/tag signatures, isolated OpenPGP commit/tag signatures, and an actual loopback SSH-agent Fetch with strict generated host-key verification and unknown-host rejection. All three explicitly invoked signing fixtures passed locally, along with the recorded HTTP/helper and authentication-unit fixtures. Apple `osxkeychain` and a login Keychain were present; no saved item was accessed or unlock exercised. These local transport/cryptographic checks do not establish hosted-account, native pinentry or hardware-key behavior.

Two release CPU reports provide separate measurement evidence:

- [Core review paths](benchmarks/2026-09-09-review-backend.md), with [raw attempts](benchmarks/2026-09-09-review-backend.json): 13 series, each with three warmups and 40 measurements, covering comparisons, tracked-path searches, conflict parsing, rebase planning, worktree/reflog reads and passive LFS planning. Source and fixture inventories were unchanged during the run.
- [App preparation and cached path filters](benchmarks/2026-09-09-review-app.md), with [raw attempts](benchmarks/2026-09-09-review-app.json): 17 series with the same warmup/sample counts, using production text/intraline/split/review and historical/working path-filter helpers. The release test executable did not launch GPUI; source hashes, dimensions and process-memory observations accompany the samples.

Both reports retain tail latencies and uncontrolled background-load conditions. They do not claim a speedup, native frame latency, cancellation latency or final-package performance. Manual-session process samples were approximately 124–131 MiB RSS; they are not a controlled long-duration leak test. No native frame-time or long-running memory-growth result is established here.

## Final focused checks and installation

Release `FDAD…` also exercised all six manual themes in Settings at interface18, both densities, system Follow appearance through Light and Dark, and independent resets (interface13 while code24 remained, then code12). The original system Dark appearance and manual Nord preference were restored. Split source at code24 scrolled both sides to the same line; Next change moved to line116 and Option-Up returned to the Unicode edit at line6, retaining aligned gutters and intraline styling. Grouping cleared the prior Working selection. These are sampled combinations, not every theme/density/workflow permutation.

The minimum-size pass found two additional layout defects: revision identities could push changed files below the viewport, and new working-filter controls consumed the reserved file-list space. The corrections and focused native results are recorded below. An invalid comparison correctly reported missing local objects; its input Escape incorrectly navigated behind the still-open modal, so final modal cancellation verification supersedes the earlier abbreviated Escape observation.

The final iteration also replaced rebase plan/message Close and Escape handling with synchronous cancellation and attached repository focus, preserving the draft. These workflows explicitly redraw the embedded dialog layer after open/close and footer busy-state changes. Native checks are recorded with the final build below.

Release `02857B90-6809-337A-9A93-B675E619385C` from `d296cd9` verified visible Quick Open entry, immediate typing, empty results, Escape dismissal and immediate Command-Shift-C; comparison initial focus, missing-ref error, Escape dismissal and immediate Command-P; tracked worktree source Return, File History/Back, and Blame marking the edited line Uncommitted. At UI18/code24, the bounded comparison inspector scrolled while files stayed accessible, and a selected added file rendered with its absent Before side. Rebase Option-Up visibly reordered its two rows, and Escape followed by Command-P verified attached return focus. The final follow-up bounds taller dialog bodies and ensures Working geometry changes schedule a repaint after drawing.

Release `B003…` verified wide-to-minimum resizing at interface18/code24, Compact density and expanded Targets. Working Changes settled automatically with its file list and fixed commit footer visible. Selecting a file then pressing End brought the last working row and its preview into view; the composer scrolled while the file list and footer remained reachable. Quick Open fit at the minimum size, and direct Cancel retained the selected working preview. Activity, reflog and worktree dialogs kept their final controls reachable. The two-commit rebase plan scrolled to its shared-history warning and Review control; Option-Up reordered the selected row, and Close followed by Command-P established usable return focus. Direct Git inspection confirmed unchanged HEAD and a clean rebase fixture. These were read-only plan checks, separate from the earlier completed rebase mutation fixture.

### Installed delivery

The final release was packaged with `scripts/package-macos.sh --no-build` after its release build. Plist and strict/deep local ad-hoc signature verification passed. `Assets.car` and the fallback `AppIcon.icns` were present, with the expected bundle icon names. Release and packaged executable UUIDs matched. The previous installed application was backed up before replacing `/Applications/GitTurtle.app` with the verified package.

| Identity | Verified value |
| --- | --- |
| Compiled source | `0c8eaec31ae9fdbc7fe7f98c5822cc0561f96ad4` |
| Bundle identifier/version | `com.gitturtle.desktop` / `0.1.0` |
| Release, package and installed UUID | `B003BB92-D6C0-3009-8A75-96A85EEBB133` (arm64) |
| Packaged and installed executable SHA-256 | `0f346ccc7a7149d5314cebc5c0893ee182d166e2c569c6b5051906b57dc69777` |
| Launched process | PID 61160, `/Applications/GitTurtle.app/Contents/MacOS/gitturtle` |

Before launch, the original preferences were restored byte for byte (backup SHA-256 `7c8e8968e6eda982f8c6b805694e0c2870637841a86bead566a2d0f309224b08`). After launch, all original nested settings, recent projects, columns and saved drafts still matched. Serialization added only the new size defaults, interface13/code12. Native Settings confirmed Nord, Comfortable, follow-system off and those sizes. The original saved draft list was empty; disposable-fixture draft retention is covered separately above. Fifteen activity entries belonging only to this milestone's disposable fixtures were removed after backing up the QA state; no unrelated entries were removed.

The installed executable reopened the original recent project in History. After Quick Open became visible, typing `README.md` populated matching tracked paths, and Escape returned to the retained History view. Settings and Back worked, and the installed app was left running in History. These installed checks were passive; no network or Git write was triggered in the user's project. The original macOS Dark appearance, contrast/transparency off and VoiceOver off were restored during the earlier system checks. Documentation and screenshot finalization did not change executable inputs, so no redundant rebuild followed.

Hosted CI run URLs/results, live-provider or hosted-LFS access, actual Keychain unlock, hardware-backed signing and Linux native/build evidence remain unavailable. The source checkout had no configured remote and no authorized hosted publication destination. Workflow configuration and local tests are not hosted execution. VoiceOver spoken navigation and GPUI semantic disabled state remain the specific accessibility limits above. Distribution, notarization and public installation media remain outside scope.
