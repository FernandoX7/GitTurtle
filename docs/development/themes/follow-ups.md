# Themes follow-ups

All 13 tasks in [`tasks.json`](tasks.json) are accepted and on `claude/themes`, which is open as pull request #23 with every check green at `5a17c4c`. This file is the verified list of what is still open, each item checked against HEAD `5a17c4c` on 2026-09-23. It is the work queue for finishing the initiative.

Sources are cited by file and section. `HANDOFF.md` means [the handoff](../HANDOFF.md) as of `5a17c4c`, which later sessions rewrite. Paths under `.local/themes-evidence/` are the coordinator's local, unversioned review bundles.

## Owner decisions

Settled by the owner on 2026-09-23.

- **PK-D2, a fallback palette for status marks: declined.** When the active palette fails canvas on warning, every warning glyph in the window degrades together. The count, the card tooltip and the Your themes row still carry the status, and the spec makes custom findings advisory.
- **IE-D3, the app-wide button fix: its own pull request from `main` after #23 merges.** It restyles the shared `button` helper for every page, not only themes, so it stays off this branch.
- **NEAR-FLOOR, the palette nudge: on this branch, as the last visual change,** so #23 ships palettes that meet their floors when rendered, and the three palette sets are recaptured once.
- **PK-D1, a 1 px ring around the caption's status marks: accepted,** in the same change as PK-N-R2a, which touches the same badge code and frames.
- **EVIDENCE-GAPS: do not block the merge.** The gaps stay listed as open in `docs/validation.md`, and the macOS pass runs when a Mac is available.
- **Taste notes the reviewers left to the owner: declined, no change.** These are the picker's 52 card tab stops without an arrow-key grid (picker round-1 N5), the two adjacent "Your themes" headings (N4), the editor's short side column in the wide layout (editor N3), and the transfer message at 12 px against the card's 11 px prose (import-export N4).

## On this branch

In the order to do them. Test-only and documentation items come first, then visible fixes, then the two changes that recapture whole sets.

1. **HYG-PATHS (done): absolute home paths in new benchmark records.**
   - Defect: five records new on this branch contain `/home/<user>/…` paths:
     - `docs/benchmarks/2026-09-23-theme-settings-view-keystroke.json`, 100 lines. Two of them are `foreign_processes` fields (lines 207 and 1425), each holding a 3.4 KB agent shell command with `~/.claude/shell-snapshots`, `/tmp/claude-1000` and grading-plan text.
     - `2026-09-23-theme-picker-switching.json`, 15 lines.
     - `2026-09-23-theme-picker-switching.md:41`.
     - `2026-09-23-theme-settings-view-keystroke.md:248`.
     - `2026-09-22-theme-draw-cost.json:79`.
   - Fix: follow [`2026-09-19-theme-editor.json`](../../benchmarks/2026-09-19-theme-editor.json) line 52, which writes `<worktree>/.local/themes-evidence/…`. Define `$EVIDENCE/` once per record, and keep run suffixes and recorded digests. Reduce each `foreign_processes` field to pid and process name, or drop it.
   - Guidance check: add a rule that fails on `/home/<name>/` or `/Users/<name>/` under `docs/`. It must allow the existing `/Users/REDACTED` form (`docs/public-launch.md:166` and three older records) and must not match `Checkout/home/…` in `docs/benchmarks/2026-09-10-glb-workflow/validation-manifest.json:90`.
   - Source: `HANDOFF.md` 2026-09-23 16:40Z bullet ("Next"); the path audit.
   - Acceptance: `git grep -nE '/(home|Users)/' -- docs/` finds only the redacted forms; each edited record keeps every measured value; the new rule has a unit test.
   - Evidence changed: the five records named above. Kind: docs.
2. **DOC-STATUS (done): stale status sentences.**
   - [`spec.md`](spec.md) line 3 still says "Status: planned."
   - Its "Open questions" (lines 130-136) still list three questions that are now settled:
     - the ColorPicker keyboard question: the hex Input is the primary control;
     - the portal `prompt_for_new_path` question: answered by the guidance path;
     - the Settings layout-pass question: answered by `themes-settings-view`.
   - [`README.md`](README.md) line 3 says ten tasks were accepted and offers `tasks-2.json` as the queue for a fresh run, but its three tasks landed in run `20260920T181431Z-aa8cb78d`.
   - `HANDOFF.md:5` says "nine of twelve themes tasks", and the file is 127 lines against its own 15-line rule.
   - Source: this audit. Acceptance: every status sentence says all 13 tasks landed and links this file. Evidence changed: none. Kind: docs.
3. **DOC-56 (done): palette facts in DESIGN.md.** [`DESIGN.md`](../../../DESIGN.md) line 56 says "One and Dracula define no panel or border" and "at the minimum surface ratios".
   - Source: `HANDOFF.md` "Follow-ups after the run" (1).
   - Acceptance: it reads "One and Dracula define no panel, and One, Rosé Pine and Dracula no border" and "at or above the minimum surface ratios", checked against `crates/app/src/appearance/sources.rs`.
   - Evidence changed: none. Kind: docs.
4. **DOC-APPLY (done): a record still asks for a fix that was made.** [`2026-09-18-theme-apply.md`](../../benchmarks/2026-09-18-theme-apply.md) line 104 says the catalog entry "should" end at the next-frame callback, but [`metrics.md`](../../benchmarks/metrics.md) line 7 already does.
   - Source: `HANDOFF.md` "Follow-ups after the run" (8).
   - Acceptance: the sentence is in the past tense and links the corrected entry.
   - Evidence changed: that record's prose only. Kind: docs.
5. **ED-N8 (done): the editor's warning count is not in the contract.** `crates/app/src/theme_editor.rs:2276` and `:2302` render "{count} warning(s)" beside the Readability heading. DESIGN.md lines 160 and 163 describe the list but not the count.
   - Source: `.local/themes-evidence/evidence-editor-3927b57/design-review.md` N8.
   - Acceptance: DESIGN.md line 163 describes the count and its accessible name.
   - Evidence changed: none. Kind: docs.
6. **STORE-MSG (done): a refused custom-themes section does not say what to do.** The error chain is `crates/app/src/preferences.rs:631` ("Invalid saved custom themes") then `:666` ("Read current preferences before saving"). It neither names the store nor asks the user to repair or remove the section, which `spec.md` line 103 requires. The test at `:1946` checks only `is_err`.
   - Source: `HANDOFF.md` "Follow-ups after the run" (2).
   - Acceptance: every refused save names the preferences file and says to repair or remove its `custom_themes` section, and the test asserts that text.
   - Evidence changed: none. Kind: test-only.
7. **A11Y-TEST (done): accessible names are not asserted.** No test checks the labels at `crates/app/src/settings.rs:1136` ("Import a theme file"), `:1477`, `:1509` and `:1519` ("Edit/Export/Delete ‹name› theme"). Also, New theme… (`:1118`) keeps its visible label while Import… is renamed.
   - Source: `.local/themes-evidence/evidence-import-export-301d82a/design-review.md` N5, N6.
   - Acceptance: a `#[gpui::test]` asserts all five names, following the picker's `card_names` hook (`settings.rs:1003-1006`); DESIGN.md line 155 states both header names.
   - Evidence changed: none. Kind: test-only.
8. **TEST-NOJOB (done): the no-job test uses a proxy.** `settings.rs:3539` detects a job through `GitTurtle::request`'s side effects, not through a worker submission counter.
   - Source: `HANDOFF.md` "Follow-ups after the run" (8).
   - Acceptance: the test asserts zero submissions on the worker queue.
   - Evidence changed: none. Kind: test-only.
9. **DEAD-ARM (done): an unreachable import arm.** `theme_editor.rs:1082` matches `Ok(Err(error))`, which cannot occur because the submitted job always returns `Ok` (`:1073-1075`).
   - Source: `HANDOFF.md` 2026-09-20 verifier notes.
   - Acceptance: the arm is removed by narrowing the job's type, and the refusal tests still pass.
   - Resolved differently: the arm is reachable. The job always answers `Ok`, but the preference executor answers `Err` for it when its queue is full or the job panicked, so the arm stays with a comment naming both cases, and a test drives the full-queue case through it.
   - Evidence changed: none. Kind: test-only.
10. **STORE-BOUND (optional) (done): unbounded custom-themes parsing.** `preferences.rs:221` parses `custom_themes` into an unbounded `Vec`, so a hostile 8 MB store costs 47 ms once.
    - Source: `HANDOFF.md` "Follow-ups after the run" (4).
    - Acceptance: deserialization stops after 33 entries with the same refusal, and a test covers it.
    - Evidence changed: none. Kind: test-only.
11. **RV: Tab can land on an undrawn row action.** `theme_editor.rs:273-276` returns early when the focused row index is unchanged. `revealed_row` is only ever set (`:277`), never cleared when the wheel or scrollbar moves the list. So after a wheel step hides the focused row, Tab to another action of that row leaves focus on an undrawn control with no ring, against DESIGN.md lines 27 and 155.
    - Source: `.local/themes-evidence/evidence-draw-cost-85a7d07/visual-unchanged/design-ruling.md` Round 2 (c); `HANDOFF.md` 2026-09-22 bullets.
    - Acceptance: the reveal is keyed by the focused action, or cleared on a wheel or scrollbar scroll. A gpui test checks that the Tab scrolls the row into view. One native capture from the frame-11 state shows the list at its end boundary with row 32's Delete… ringed.
    - Evidence changed: adds one frame to the [September 19 export and import entry](../../validation.md#september-19-theme-export-and-import); no existing frame changes. Kind: native, test.
12. **IE-N7: the bound tooltip withholds the remedy.** `settings.rs:1123` and `:1140` say only "Up to 32 custom themes can be saved". The actionable sentence at `theme_editor.rs:1116` is reachable only from the test at `:4021`, because both buttons are disabled at the bound (`settings.rs:1119`, `:1137`).
    - Source: `HANDOFF.md` 2026-09-20 bullets N7 and "One capture to fold"; import-export design review N7.
    - Acceptance: both disabled tooltips add "Delete one before …", a test asserts the text at 32 themes, and the unreachable branch is removed or documented as a race guard.
    - Evidence changed: adds `docs/evidence/themes/import-export/bound-1000x680-01-hover-import-disabled.png`, hovered at the button's centre (`y0+28`) after a motion that ends inside it. `bound-1000x680-00-bound-card.png` is unchanged. Kind: native, test.
13. **PK-N-R2a: the check badge does not scale with text size.** `settings.rs:3163` uses `.size(px(16.))` and `:3170` uses `icon("check", 11., …)`. At 11 and 12 pt the selected caption sits 3 or 2 px above its neighbours.
    - Source: `.local/themes-evidence/evidence-picker-f826dc6/design/design-review.md` Round 2 P11 and "Follow-ups".
    - Acceptance: `ui_size(16.)` and `ui_size(11.)`; `debug_bounds` shows the selected caption's top equal to its row neighbours at 11, 12, 13 and 18 pt. If the owner accepts PK-D1, the 1 px active-canvas ring around the badge and warning glyph (`settings.rs:3143-3171`) ships here.
    - Evidence changed: `docs/evidence/themes/picker/text11-1440x900-01-selected-warned.png` and `text12-1440x900-01-selected-warned.png`. With PK-D1, also `warning-1440x900-01-glyph-active-palette.png` and every picker frame with a selected card. Kind: native, design, test.
14. **PK-D3: focus is invisible on the selected picker card.** The card's focus shows only as its 1 px border (`settings.rs:1018-1033`), which is already accent when the card is selected. Focus there changes 48 corner pixels, against DESIGN.md line 27. The base behaves the same.
    - Source: picker design review Round 2 D3; `HANDOFF.md` 2026-09-23 16:40Z bullet (3).
    - Acceptance: a focused card draws a 2 px accent ring 1 px outside its border, inside the 10 px column and 6 px row gaps. The focused selected card then differs plainly from the unfocused selected card in a native diff, and a test covers the focus path.
    - Evidence changed: `docs/evidence/themes/picker/comfortable-1440x900-05-focus-custom.png` and `comfortable-1000x680-03-focus-custom.png`, plus a new selected-and-focused frame. Kind: native, design.
15. **ED-N6: the invalid `×` is hard to read.** `theme_editor.rs:391-404` draws U+00D7 at `ui_text(11.)`. Only 5 to 7 of its 41 ink pixels reach 4.5:1, against about 36 of 84 for the readability `!`.
    - Source: editor design review N6; `HANDOFF.md` "Editor follow-ups".
    - Acceptance: a stroke-drawn cross (a 24-unit SVG with 1.65-unit strokes, or two 2 px rules) with a similar share of ink at 4.5:1.
    - Evidence changed: `docs/evidence/themes/editor/light-1000x680-06-invalid-focused.png` and `dark-1440x900-06-invalid-focused.png`. Kind: native, design.
16. **ED-N7: the wide side column scrolls without a scrollbar.** `theme_editor.rs:2448-2461` makes it scrollable with no `Scrollbar`, so past about 17 warnings it overflows with no affordance. DESIGN.md line 160 scopes the scrollbar to the token column only.
    - Source: editor design review N7 and requirement 5.
    - Acceptance: a scrollbar that appears when the column overflows, or a DESIGN.md clause stating why none is needed, plus a frame with more than 17 warnings.
    - Evidence changed: a new overflow frame. If the scrollbar is always visible, also every `docs/evidence/themes/editor/*-1440x900-*.png` and `picker/editor-1440x900-01-preview-card.png`. Kind: native, design.
17. **ED-N9: the Accent row's clearance at 1000x680.** The longest description cleared the warning slot by 10 px on `3927b57`, which is off the 4/8/12/16 rhythm and the first row to collide at larger text. It has not been re-measured on HEAD.
    - Source: editor design review N9.
    - Acceptance: at least 12 px at 13 pt and no collision at 18 pt, measured on the current build.
    - Evidence changed: `docs/evidence/themes/editor/*-1000x680-05-edit-warning.png` only if the geometry moves; a new 18 pt frame. Kind: native, design.
18. **PORTAL-MSG: the portal error leads with a raw zbus message.** `crates/app/src/folder_picker.rs:84-86` puts the raw `zbus` error chain before the guidance sentence. This is cosmetic and shared with the folder picker.
    - Source: `.local/themes-evidence/evidence-import-export-301d82a/qa-report.md` Finding 3.
    - Acceptance: the first line is user-facing and the technical detail is secondary and clamped; the `folder_picker` tests are updated.
    - Evidence changed: `docs/evidence/themes/import-export/noportal-1000x680-01-export-guidance.png` and `noportal-1000x680-03-import-guidance.png`. Kind: native, test.
19. **SV-PERF: settings-view costs extra UI CPU on switch, hover and press.** Against the same-session pinned base, the settings-view build costs +1.555 ms (empty store) and +2.056 ms (32 themes) of UI CPU per switch, +1.4/+1.1 ms per hover window and +1.8/+1.8 ms per press window. Its G5 switch delta was +4.006 against a limit of 4.000 and passed only by coordinator ruling. The two unverified hypotheses are the card clip's extra frame on a hover change (`settings.rs:2683`) and a click refresh rebuilding every `ThemeCardBody` (`:2780`, reached through `:682`).
    - Source: `.local/themes-evidence/perf-settings-view-a3/attestation-summary.txt` "REGRESSION AGAINST THE SAME-SESSION BASE"; `HANDOFF.md` 2026-09-23 16:40Z bullet (1), (2).
    - Acceptance: a diagnostic build counts frames and card-body builds per window; the fix brings the pinned switch, hover and press deltas within the base's launch spread (about 0.13 ms) and G5 passes on the letter.
    - Evidence changed: a new dated record beside [`2026-09-23-theme-settings-view-keystroke.md`](../../benchmarks/2026-09-23-theme-settings-view-keystroke.md), and a native visual-unchanged pass if drawing changes. Kind: performance.
20. **NEAR-FLOOR: palettes render below their contrast floors.** The tuned tokens are unchanged, for example `crates/app/src/appearance.rs:712` (Rosé Pine `muted` `0xa19db7`), `:746` (Rosé Pine Dawn `added` `0x42717a`) and `:769` (Dracula `muted` `0xb8bfd6`). At 1x no rendered pixel of muted text on a hovered selected row reaches 4.5:1 in the seven adapted themes. `warning` used as text on `subtle` is under 4.5:1 for five built-ins: Kanagawa Lotus 3.95, Rosé Pine Dawn 4.25, One Light 4.27, Solarized Dark 4.34 and Solarized Light 4.38. No rule checks it (`spec.md` line 57 checks canvas on warning only).
    - Source: [`contrast-margins.md`](contrast-margins.md) "What the follow-up should do"; `HANDOFF.md` "Open, decided by the owner on 2026-09-18"; import-export design review N2; editor design review N4.
    - Acceptance:
      - the proposed values, plus Daylight `line_number` and Nord `hover`;
      - a warning-as-text rule in `spec.md` with appearance tests;
      - the 0.25 margin target in DESIGN.md's tuning sentence;
      - the rendered 1x peak at or above 4.5 on the named pairs;
      - one Kanagawa Lotus frame of a refused import.
    - Evidence changed: every frame in `docs/evidence/themes/solarized-one/`, `rose-pine-dracula/` and `alucard-kanagawa/`, plus any other committed frame drawn in a nudged palette (audit Nord and Daylight frames under `docs/evidence/`); recaptured once. Kind: native, design, test.
21. **IE-D3: the shared button's hover and pressed states are wrong (moved to its own pull request from `main` after #23 merges).** The `button` helper (`crates/app/src/main.rs:1787-1823`) keeps the kit's ghost defaults:
    - hover paints darker than a hovered row (1.16:1 the wrong way);
    - pressed differs from hover by 1.02:1;
    - DESIGN.md line 27 asks for the hover surface and `selected`.
    - Source: import-export design review D3; `HANDOFF.md` "Queued follow-ups from these lenses".
    - Acceptance: hover is `p.hover` composited over the surface beneath, and pressed is `p.selected`, measured distinct on rows and headers.
    - Evidence changed: resting frames are unchanged if only hover and pressed change. No committed themes frame is known to show a hovered or pressed helper button, because the drivers park the pointer, but audit `docs/evidence/` before claiming none. It adds new hover and pressed frames. Kind: native, design.
22. **EVIDENCE-GAPS: what no capture attests yet.**
    - macOS at 2x, and any scale factor other than 1.
    - The accessibility tree: GPUI does not register with AT-SPI on the Linux host, and VoiceOver is unchecked.
    - Hover and pressed states for row actions, header buttons and the scrollbar thumb.
    - A failure line in a light theme.
    - The portal dialogs themselves.
    - Editor states without a frame: Reset to base, Replace colors, a refused save with "Saving…", a failed delete, and text above 13 pt.
    - The two-column picker.
    - A focused row action beside the just-imported highlight.
    - A hovered action on a partly scrolled row.
    - Light palettes with the 32-theme list.

    Source: the "Requirements left open" sections of the import-export and `evidence-editor-3927b57` design reviews; "Open, not findings" in the draw-cost ruling; "Still open" in `.local/themes-evidence/evidence-settings-view-a3/design-ruling.md`. Acceptance: native frames for each, the macOS ones on a real Mac, recorded in the matching `docs/validation.md` entry. Evidence changed: new captures only. Kind: native.

## Outside this branch

**Commit inspector and CI queue.** [`docs/development/tasks.json`](../tasks.json) is resolved on `main`: ten tasks are integrated, and `ci-codeql` is superseded as recorded in [`docs/ci-codeql.md`](../../ci-codeql.md). The coordinator checkpoints C0 to C4 in [the initiative note](../commit-inspector-and-ci.md#coordinator-checkpoints) are still open. They need a real Mac, Apple credentials and hosted samples:
- the `ui-commit-messages` macOS matrix;
- the `dist-macos-package` run on a Mac;
- C1 cache medians, tails and negative cases;
- C3 artifact upload after C0;
- the C4 signing and release rehearsal.

Fix this stale text in a separate docs pull request from `main`:
- [`docs/development/README.md`](../README.md) line 15;
- [`commit-inspector-and-ci.md`](../commit-inspector-and-ci.md) lines 3-7 and its [C2 CodeQL section](../commit-inspector-and-ci.md#c2--hosted-codeql-and-full-merge-time);
- [`docs/ci.md`](../../ci.md) lines 100-106 and line 246;
- the `ci-codeql` entry in `tasks.json`, which has no status field, so record its supersession in the README.

**Controller follow-ups from the themes run.** These are development tooling, not product, and are validated by the agent-loop suite:
- a `blocked` status does not keep a task out of `select_ready` (`scripts/agent_loop/runner.py:394`);
- a gate failure replaces the coordinator note;
- no command advances `accepted_head` onto an evidence-only commit (the CLI at `runner.py:803-826`);
- a stale candidate whose patch still applies is rebuilt rather than rebased and re-gated.

Source: `HANDOFF.md` 2026-09-23 "Controller lessons", and its "Proposed, not yet built" bullet.

## Fixed or obsolete

- **Import-export:**
  - D1 and D2, the source clamp, N1 and N8 are fixed. Evidence: `settings.rs:1417-1418` keeps the import highlight on hover; `:1497-1502` shows the Export… tooltip; DESIGN.md line 155 says "one line … never two"; `theme_editor.rs` clamps both names. The [validation entry](../../validation.md#september-19-theme-export-and-import) confirms `5637cec`.
  - The verifier's unversioned-path comments are fixed in `17d8786`: no `.local/themes-evidence` path remains under `crates/`.
- **Picker B1-B3:** fixed on `4cdd4df` and approved in design round 2.
- **Settings-view:**
  - F1 is gone: no stale frame in 204 actions per build, and the design ruling approved every difference.
  - The picker's +3.9 ms edit p95 is recovered: G2 is 21.000 ms against a base of 21.913 ms.
- **Editor:**
  - D4 is fixed: DESIGN.md line 163 makes "Warnings do not prevent saving." conditional.
  - N10 is fixed: both dark `11-edit-return-commits` frames are committed under `docs/evidence/themes/editor/`.
  - The save goes through the preference writer (`theme_editor.rs:1161`).
- **Harness and QA guidance:**
  - The per-launch `XDG_CONFIG_HOME` rule is in `.agents/skills/gitturtle-native-qa/SKILL.md:20` and `.claude/agents/native-qa.md:9` (`21749e1`).
  - Harness items #23 and #24 landed as `cdc54ad` and `7af6e8c`.
  - The macOS tooling test fix `967bc4f` is green on #23.
- **Obsolete:**
  - **The owner's switch-metric question.** A completed save now asks for a frame only when it changed something (`settings.rs:397`). `spec.md` line 114 makes UI-thread CPU the switch measure, and `metrics.md` line 7 documents the callback boundary.
  - **The editor's one-pixel antialiasing flip at (187,183).** It is launch-variant rasterization that later rulings class as F.
  - **The kit-level Input/ColorPicker levers.** They matter only if the 26 ms edit bound is tightened, and the current p95 is 21.0 ms.
  - **The XWayland lit sidebar row.** It is not a themes defect and was seen with synthetic input only.

## Evidence rules for this list

- A visible change needs native-qa captures against the current branch build and a design-reviewer ruling on every difference.
- A performance claim needs a release measurement pinned to P-cores (`taskset -c 0-7`), with a same-session pinned base designated in writing before any graded launch.
- Committed captures that a fix changes are re-taken in the same pull request, and the dated `docs/validation.md` entry is amended to say which frames changed and why.
- A new benchmark record goes under `docs/benchmarks/` with a before and after against the record it supersedes.
- Keep absolute paths out of `docs/`: use `<worktree>/` or a `$EVIDENCE/` prefix defined once per record.
