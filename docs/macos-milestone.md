# macOS craft and everyday Git milestone

Started September 8, 2026 from clean `6c44919`. Scope is the attached macOS-first milestone; distribution, installers, notarization and publishing are excluded. Root owns Git index/commits, native UI and local packaging. Existing workflows, six themes and turtle artwork remain supported.

## Finite acceptance checklist

- [x] Review actual Projects, History, Working Changes, Compare, Settings and dialogs. Record prioritized reproducible findings and representative before/after screenshots; fix material findings.
- [x] Add complementary macOS polish: native menu organization/shortcut help, follow-system appearance with manual themes, and repository Finder/editor handoff if missing. Evaluate accessibility and native window behavior.
- [x] Research Apple Liquid Glass guidance and pinned GPUI/AppKit capabilities; prototype a restrained native material and verify it or document a concrete limitation with an opaque fallback. Never label ordinary transparency Liquid Glass.
- [x] Complete configured SSH/helper/Keychain authentication paths and actionable signing/credential failures; explicit bounded prompts/cancellation, secret-free state/diagnostics, no retries or automatic network. Fixture verification is separate from live hosted auth.
- [x] Integrate asynchronous bounded committed/working blame, uncommitted lines and line-history/commit navigation with retained context; expose rename/lineage limits and test behavioral fixtures.
- [x] Integrate discover/filter/inspect/create/delete tags with captured targets, configured signing, explicit local/remote consequences and behavioral fixtures.
- [x] Add overlay and draggable wipe image modes, linked zoom/pan, keyboard controls, missing/differing sides, transparency and source/preview scale labels.
- [x] Add contextual ignore with exact escaped-rule/destination review, shared/local options, stale-safe content preservation, tracked-file explanation and behavioral fixtures.
- [x] Add CI locked builds, formatting, tests, Clippy and release compilation without upload/distribution. Report hosted versus local execution precisely.
- [x] Verify realistic native workflows, keyboard/focus/copy/scroll/resizing, six themes/two densities and available accessibility settings. Record actual VoiceOver and any unavailable platform checks.
- [x] Measure affected release paths with fixture/hardware/cache/sample/tail evidence and bounded resource observations; no unsupported speedup claims.
- [x] Run final format, workspace tests, strict all-target workspace Clippy, release build; package locally and verify final native artifact. Update docs and commit meaningful increments.

## Progress and ownership

- Initial checkout clean. Prior milestone is complete; its evidence remains tied to its recorded builds.
- Blame worker owns new core/app attribution modules and file-history/worker integration.
- Tags/ignore worker owns new core/app tag and ignore modules and narrow contextual entry points.
- Authentication worker owns explicit-process/authentication improvements, operation integration and CI.
- Root owns design review, image modes, native/macOS polish, integration, evidence and final gates.

## Findings and verification

The baseline findings below were implemented and checked. The [native verification record](macos-native-verification.md) distinguishes debug, release and final-correction evidence, including unavailable checks.

### Initial native review

Baseline package UUID `C81E4C94-08EB-3283-860E-BE6F84E69616`, matching the previous final recorded executable, on macOS 26.6.2 arm64. Disposable `macos-milestone-20260908` demo fixture; initial Graphite/Comfortable window about 1480 × 1012 including title bar. These are before observations, not new-feature verification.

| Priority | Finding and user impact | Reproduction / planned correction |
| --- | --- | --- |
| P1 | Image comparison requires scanning separated panels; subtle pixel changes lack direct spatial comparison. | Open mixed-media commit → overview.png. Add one-canvas overlay/wipe, shared geometry and accessible adjustment controls. [Before](evidence/macos-milestone/before-images.jpg). |
| P2 | Projects repeats the repository header above its own header, bringing unrelated identity/actions into project selection. | Open Projects from Compare. Use one Projects header with operation feedback when needed. |
| P2 | Native menu has only one app menu containing unrelated navigation, file and settings commands; standard Edit/Window affordances and shortcut discovery are absent. | Inspect menu bar. Add conventional menus, correct repository/busy enabled states and shortcut help. |
| P2 | Appearance is manual only, requiring repeated settings visits as system appearance changes. | Settings exposes six manual palettes. Add system following while retaining manual palette choice and editor context. [Before](evidence/macos-milestone/before-settings.jpg). |
| P2 | Repository name/path truncate and there is no repository-level Finder/editor handoff. | Inspect long fixture name in header. Add discoverable local handoff with explicit editor preference and useful failures. |
| P2 | New attribution/tag/ignore capabilities need consistent entry points, result/empty/error states and retained context. | Integrate into comparison, branch menu and Working Changes context, then exercise actual native flows. |
| P3 | Existing dense History and six-palette identity are coherent; preserve them while refining navigation and controls. | [Before History](evidence/macos-milestone/before-history.jpg). Minimum-window, six-theme and accessibility results are in the native record. |

### First integrated native pass

Debug package UUID `D01EF923-C66A-3F4A-BEFD-F9C5F291C512` exercised side-by-side, overlay and draggable wipe on the demo's modified transparent PNG. Wipe moved from 50% to about 69%; overlay showed aligned differences. This predates subsequent integration fixes and is not final release evidence.

The native NSGlassEffectView experiment blurred the GPUI header's own labels once the window became active. [Prototype evidence](evidence/macos-milestone/glass-prototype-obscured-toolbar.jpg) and [investigation](liquid-glass-investigation.md) document why the supported final path remains opaque. The experiment is retained as source under `docs/experiments`; no glass is shipped or claimed as verified production rendering.

Native explicit Fetch and Pull authenticated with a synthetic token using a local SSH transport helper, without contacting the displayed fixture hostname. Independent Git inspection verified fast-forward HEAD `240a6aa4a5a10e400689d0f1abb4029c967f610b` and clean content. A delayed Fetch was cancelled through the visible control; the app reported possible partial results and no retry. Credential autofocus/accessibility was improved and passed rebuilt keyboard-only entry. Final release testing also found and fixed the dialog Cancel/Escape callback; see the native record.

The initial Blame exit was reproduced as a GPUI duplicate-focus assertion. A distinct retained attribution focus handle fixed it; committed/working attribution, rename history, copy and nested navigation then passed native checks.

## Completed increments and final evidence

- `bfaa5d0`: bounded authentication/attribution/tag/ignore services, regression fixtures, backend benchmark and macOS/Linux CI configuration.
- `6e9d7a2`: complete native feature interactions, shared image geometry, conventional Mac menus/handoff/system appearance, screen-reader descriptions and the documented Liquid Glass limitation.
- `e27a7ac`: native authentication Cancel/Escape regression fix, verified with both dismissal paths and a subsequent successful local Fetch.
- `5f69288`: explicit History destinations leave nested inspections; Back retains one-level restoration. Final matching release/package UUID `07F73B06-AFAD-3F3C-8FD6-09ABD981D2B1` passed both native regressions and the image rendering check. [Final wipe](evidence/macos-milestone/after-final-wipe.jpg).
- [Native verification](macos-native-verification.md), [release backend measurements](benchmarks/2026-09-08-macos-milestone-backend.md), and [native image sample](benchmarks/2026-09-08-macos-native-images.json) retain provenance and limits. CI has not been executed on a hosted runner. Actual VoiceOver launched, but spoken/cursor navigation could not be observed through the available automation surface; AX/keyboard evidence is reported separately.
- No distribution work was performed. All test writes and local transport actions used disposable fixtures. System Dark, VoiceOver off, contrast/transparency off and original app theme/density/editor preferences were restored.
