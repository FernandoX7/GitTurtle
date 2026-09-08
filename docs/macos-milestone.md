# macOS craft and everyday Git milestone

Started September 8, 2026 from clean `6c44919`. Scope is the attached macOS-first milestone; distribution, installers, notarization and publishing are excluded. Root owns Git index/commits, native UI and local packaging. Existing workflows, six themes and turtle artwork remain supported.

## Finite acceptance checklist

- [ ] Review actual Projects, History, Working Changes, Compare, Settings and dialogs. Record prioritized reproducible findings and representative before/after screenshots; fix material findings.
- [ ] Add complementary macOS polish: native menu organization/shortcut help, follow-system appearance with manual themes, and repository Finder/editor handoff if missing. Evaluate accessibility and native window behavior.
- [ ] Research Apple Liquid Glass guidance and pinned GPUI/AppKit capabilities; prototype a restrained native material and verify it or document a concrete limitation with an opaque fallback. Never label ordinary transparency Liquid Glass.
- [ ] Complete configured SSH/helper/Keychain authentication paths and actionable signing/credential failures; explicit bounded prompts/cancellation, secret-free state/diagnostics, no retries or automatic network. Fixture verification is separate from live hosted auth.
- [ ] Integrate asynchronous bounded committed/working blame, uncommitted lines and line-history/commit navigation with retained context; expose rename/lineage limits and test behavioral fixtures.
- [ ] Integrate discover/filter/inspect/create/delete tags with captured targets, configured signing, explicit local/remote consequences and behavioral fixtures.
- [ ] Add overlay and draggable wipe image modes, linked zoom/pan, keyboard controls, missing/differing sides, transparency and source/preview scale labels.
- [ ] Add contextual ignore with exact escaped-rule/destination review, shared/local options, stale-safe content preservation, tracked-file explanation and behavioral fixtures.
- [ ] Add CI locked builds, formatting, tests, Clippy and release compilation without upload/distribution. Report hosted versus local execution precisely.
- [ ] Verify realistic native workflows, keyboard/focus/copy/scroll/resizing, six themes/two densities and available accessibility settings. Record actual VoiceOver and any unavailable platform checks.
- [ ] Measure affected release paths with fixture/hardware/cache/sample/tail evidence and bounded resource observations; no unsupported speedup claims.
- [ ] Run final format, workspace tests, strict all-target workspace Clippy, release build; package locally and verify final native artifact. Update docs and commit meaningful increments.

## Progress and ownership

- Initial checkout clean. Prior milestone is complete; its evidence remains tied to its recorded builds.
- Blame worker owns new core/app attribution modules and file-history/worker integration.
- Tags/ignore worker owns new core/app tag and ignore modules and narrow contextual entry points.
- Authentication worker owns explicit-process/authentication improvements, operation integration and CI.
- Root owns design review, image modes, native/macOS polish, integration, evidence and final gates.

## Findings and verification

Pending initial native inspection. Do not interpret this checklist as completed verification.

### Initial native review

Baseline package UUID `C81E4C94-08EB-3283-860E-BE6F84E69616`, matching the previous final recorded executable, on macOS 26.6.2 arm64. Disposable `macos-milestone-20260908` demo fixture; initial Graphite/Comfortable window about 1480 × 1012 including title bar. These are before observations, not new-feature verification.

| Priority | Finding and user impact | Reproduction / planned correction |
| --- | --- | --- |
| P1 | Image comparison requires scanning separated panels; subtle pixel changes lack direct spatial comparison. | Open mixed-media commit → overview.png. Add one-canvas overlay/wipe, shared geometry and accessible adjustment controls. [Before](evidence/macos-milestone/before-images.png). |
| P2 | Projects repeats the repository header above its own header, bringing unrelated identity/actions into project selection. | Open Projects from Compare. Use one Projects header with operation feedback when needed. |
| P2 | Native menu has only one app menu containing unrelated navigation, file and settings commands; standard Edit/Window affordances and shortcut discovery are absent. | Inspect menu bar. Add conventional menus, correct repository/busy enabled states and shortcut help. |
| P2 | Appearance is manual only, requiring repeated settings visits as system appearance changes. | Settings exposes six manual palettes. Add system following while retaining manual palette choice and editor context. [Before](evidence/macos-milestone/before-settings.png). |
| P2 | Repository name/path truncate and there is no repository-level Finder/editor handoff. | Inspect long fixture name in header. Add discoverable local handoff with explicit editor preference and useful failures. |
| P2 | New attribution/tag/ignore capabilities need consistent entry points, result/empty/error states and retained context. | Integrate into comparison, branch menu and Working Changes context, then exercise actual native flows. |
| P3 | Existing dense History and six-palette identity are coherent; preserve them while refining navigation and controls. | [Before History](evidence/macos-milestone/before-history.png). Further minimum-window and accessibility checks remain on checklist. |
