# Public launch checklist

The owner made the repository public on September 14, 2026. No binary release
has been published. This record separates source publication and repository
settings from the remaining requirements for a public binary release.

## Source publication and repository settings

- [x] Apply the owner-approved [MIT license](../LICENSE) to GitTurtle-owned code
  and all three workspace manifests. Preserve [third-party terms](../THIRD_PARTY_NOTICES.md).
- [x] Prepare the README, contributor/support guidance, issue forms, PR template,
  and code of conduct. Issues are enabled. Existing `bug`, `enhancement`, and
  `question` labels distinguish defects, requests, and support; `documentation`,
  `accessibility`, `good first issue`, and `help wanted` cover focused follow-up.
- [x] Publish source through the owner's visibility change. The historical privacy
  findings below remain part of the record; publication does not erase earlier
  revisions or establish a new privacy audit. No history rewrite was requested
  or performed.
- [x] Record current [native validation](public-launch-validation.md) and genuine
  README screenshots. Actual desktop checks remain separate from clean builds,
  headless checks, virtual X11/Wayland sessions, and historical macOS results.
- [x] Configure repository description/topics, Dependabot alerts, full-commit
  pinning for Actions, and protection for `main`. Contributions require a pull
  request, resolved conversations, an up-to-date branch and both GitHub Actions
  checks: `Rust · macos-15` and `Rust · ubuntu-24.04`. Force pushes and branch
  deletion are disabled. No approving review is required while this is a solo
  project; the owner's administrator override remains available.
- [x] Enable [private vulnerability reporting](../SECURITY.md). The API reports it
  enabled, and the public Security page displays **Report a vulnerability**.
- [x] Require approval for **all external contributors**, enable secret scanning
  and push protection, and retain read-only Actions tokens with workflow PR
  approval disabled. These settings were verified after publication. See
  GitHub's [Actions settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository)
  and [security settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-security-and-analysis-settings-for-your-repository).
- [x] Use squash merges with the PR title/body, expose Update Branch and optional
  per-PR auto-merge, and delete merged contribution branches automatically.
  Keep documentation in the repository and disable the unused Wiki. Issues and
  support questions share the existing labels/forms; Discussions remain off.
- [x] Enable Dependabot security-update PRs and CodeQL default setup on standard
  GitHub runners. CodeQL selects the detected supported languages, including
  Rust, Python, Actions and JavaScript/TypeScript, with local and remote sources
  in its threat model. Enabled analysis is not a claim that every scan has passed.
- [x] Require CodeQL results before merging to `main`, blocking high/critical
  security findings and code-scanning errors. The dedicated merge-protection
  ruleset preserves the owner's explicit bypass and complements the required
  Rust checks. See [CodeQL merge protection](https://github.com/FernandoX7/GitTurtle/rules/23351839).
- [x] Schedule weekly [Dependabot Actions updates](../.github/dependabot.yml),
  grouping minor/patch updates and limiting open version-update PRs to two.
  Cargo version upgrades remain reviewed maintenance because the matching GPUI
  stack and local vendor patches must be checked together; security-update PRs
  remain enabled. No dependency PR is automatically approved or merged.

No additional email alias, funding enrollment or visibility action is needed.
Use pull requests for future contributions and review their changes and checks
before merging; enabling auto-merge does not automatically merge every PR.
The owner retains administrator override for emergencies. The Quality workflow
uses standard `ubuntu-24.04` and `macos-15` runners, whose execution is free for
public repositories; earlier private runs used the account's allowance. See
[GitHub Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions).

The owner should select **Watch → Custom → Security alerts** on GitHub and check
their personal notification/email preferences so confidential reports reach
them. The connected token cannot inspect or change personal subscriptions;
repository-level private reporting is already enabled.

The owner's [GitHub Sponsors profile](https://github.com/sponsors/FernandoX7)
is live and verified, with one-time and monthly contributions. The
[funding configuration](../.github/FUNDING.yml) and README/support links use that
destination. GitHub's API recognizes the funding link after the configuration
reached the private default branch in `3c4fa51`. After publication, an anonymous
public-page check also found **Sponsor this project**, and the destination
offers one-time and monthly contributions; see the
[funding activation record](funding.md).

## Before a public binary release

- [ ] Resolve the target's `licenses/REVIEW_REQUIRED.md` entries and pass
  `scripts/collect-third-party-licenses.py --require-complete --target TARGET OUTPUT`.
  Local packages retain available notices and report unresolved attribution;
  they do not establish completed license clearance. See the
  [notice inventory and source requirements](../THIRD_PARTY_NOTICES.md).
- [ ] Build and test the exact release source/target, record the executable and
  archive hashes, and include the generated licenses and any required sources.
  Establish the source identity separately when using `--no-build`.
- [ ] Complete the relevant real macOS and Ubuntu 24.04 x86-64 desktop checks,
  including native menus, shortcut help, picker, window controls, and scaling.
  Do not describe an ad-hoc-signed local macOS bundle as notarized, or a local
  Linux archive as an officially published/downloadable release.
- [ ] Obtain separate owner approval to publish binaries or release notes.

## September 14, 2026 audit record

Baseline: `d904f2798659e37fd93837fc7739ebacf28921db`, before launch changes.
The bounded local scan covered **1,287 tracked files**, all **2,018 reachable
blobs across 130 commits**, and commit messages/author metadata. The blob limit
was 16 MiB; no reachable blob exceeded it. Patterns checked recognizable GitHub,
AWS, OpenAI/Anthropic, Slack and Google API token forms, private-key markers,
authenticated HTTP(S) URLs, and absolute home paths. No credentials matching
those token patterns were found. Private-key markers and authenticated URLs were
confirmed synthetic authentication/profile tests. The sole commit identity uses
a GitHub `noreply` address.

Three benchmark JSONs contained four personal home prefixes. Current copies of
`2026-09-08-macos-milestone-backend.json`, `2026-09-09-review-app.json`, and
`2026-09-09-review-backend.json` now replace only those prefixes with
`/Users/REDACTED`; metrics and all other bytes remain unchanged. Original blobs
remain in the named baseline/history. The vendored renderer's
`tests/quality_baseline.json` contains its upstream author's machine paths;
that published upstream baseline was retained unchanged. A fourth GitTurtle
JSON match was explanatory sanitization prose, not a personal path.

All **79 tracked historical images** in `docs/evidence` and `docs/screenshots`
(60 and 19 respectively) were inspected for metadata and OCRed with Tesseract
5.3.4. Metadata inspection found dimensions but no GPS, author, or comment tags.
OCR found no recognizable token/private-key/authenticated-URL patterns. All 14
flagged originals were also viewed: at least 11 display the maintainer's name,
seven show full or truncated home paths, and three Settings captures show a
GitHub `noreply` identity. Other email-like text was synthetic fixture data or
OCR error. Existing images were retained, including
`macos-milestone/before-settings.jpg`, `after-settings.jpg`, and
`after-ignore-review.jpg`, plus `consistency-milestone/before-settings.jpeg`.
Those disclosures were flagged before publication and remain in the published
history and evidence.

This is a pattern scan, metadata inspection, OCR, and selected visual review;
it is not proof that every secret or private detail is absent. Historical binary
images were scanned as bytes but not all separately OCRed at every revision;
unreachable Git objects, external artifacts, private services, and untracked files
were outside this publication scan. New launch screenshots have their own
fixture/build validation record.

The existing Quality workflow uses `pull_request`, read-only `contents`
permission, a full-commit-pinned checkout with persisted credentials disabled,
hosted runners, and timeouts. It references no secrets and uses no
`pull_request_target` or privileged follow-up workflow. The launch change adds
strict default shell error handling and verifies installed license retention.
Initial settings inspection found Actions enabled, default token permissions set to
read, and PR approval by Actions disabled. All Actions were permitted and `main`
had no branch protection. These observations are settings checks, not an audit
of every external dependency.

The subsequent owner-authorized settings update added a description and six
relevant topics, enabled Dependabot alerts, required full SHA pins for Actions,
and protected `main` as described above. Read-back verified those settings. The
dependency graph was populated and Dependabot reported no alerts at the time;
this does not replace the bounded source/privacy review or prove the absence of
vulnerabilities. Issues and the existing form labels were already configured.

Hosted Actions runs were inspected after the initial settings audit.
[Run 34879397939](https://github.com/FernandoX7/GitTurtle/actions/runs/34879397939)
at `3c4fa51e950e7cf0744e4a3e722d4f3706e97fb6` failed immediately at
18:12:33 UTC on September 14, 2026, with zero jobs/check runs and no job log.
[Run 34871557009](https://github.com/FernandoX7/GitTurtle/actions/runs/34871557009)
at `4ad8e27352c1f33cecd145eac866ee5ff9c33e26` failed in the same way at
16:55:09 UTC. Neither run executed Rust checks or platform builds.

Local actionlint 1.7.12 identified an invalid `runner.temp` expression in the
job-level `env` mapping; GitHub's
[context availability table](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#context-availability)
confirms `runner` is unavailable there. The workflow now sets the same isolated
Git-config path through `GITHUB_ENV` in an initial runner step, before checkout.
The corrected workflow passes actionlint. The zero-job failures and invalid
expression establish a workflow validation problem; they provide no evidence
of a Rust failure or an account/billing restriction.

After the correction reached private `main` in `326fa0f`,
[run 34881050919](https://github.com/FernandoX7/GitTurtle/actions/runs/34881050919)
successfully created both macOS and Ubuntu jobs. Git isolation and checkout
passed on both; macOS also passed toolchain setup and formatting and entered
workspace tests. Ubuntu entered native dependency installation. The workflow
validation failure is resolved; a completed hosted pass remains to be verified.

The later macOS job in
[run 34881541215](https://github.com/FernandoX7/GitTurtle/actions/runs/34881541215)
reached the preview tests and exposed missing ImageIO dimension metadata for a
JPEG 2000 fixture. Commit `712d99e` adds bounded JP2/codestream dimension checks
before native decoding and preserves the pixel/orientation assertions. The full
local workspace tests and strict Clippy passed in Ubuntu 24.04 userspace; these
Linux checks do not exercise ImageIO. The hosted rerun is
[run 34883934089](https://github.com/FernandoX7/GitTurtle/actions/runs/34883934089).

The initial Python and JavaScript CodeQL results were reviewed using their full
source-to-sink traces. Thirty-two path findings concerned intentionally selected
local fixture/reference paths or per-user installation paths; each dismissal
records its specific trust-boundary reasoning in GitHub. Commit `79c5a13` also
hardens the three benchmark scripts: baseline arguments resolve to verified
commit OIDs with Git option parsing stopped, and the measured executable resolves
once before hashing and execution. Syntax checks, 18 disposable revision probes,
and a competing-PATH executable probe passed. No benchmark performance claim is
made by these checks. Local and remote CodeQL threat sources remain enabled;
these findings do not justify excluding tooling from future scans.

The complete initial [CodeQL analysis](https://github.com/FernandoX7/GitTurtle/actions/runs/34883960956)
finished successfully for Actions, Python, JavaScript/TypeScript and Rust.
Its 291 findings were reviewed by source and sink. The intentional local-path,
fixture, structured-command and unused-toolkit flows have individual disposition
explanations in GitHub; a successful scanner job alone does not resolve alerts.
Inspection also found missing recovery-path validation before metadata traversal
(`3385807`) and predictable temporary paths in six upstream renderer tests
(`aee1879`). Regression checks cover malformed Git trees without changing
repository/outside-file state; renderer tests now use private temporary
directories. These findings did not demonstrate arbitrary production command
execution or writing. The scan continues to include tooling and local inputs.

The macOS rerun at `712d99e` passed JP2 pixels but failed raw J2K decoding.
An independent [native probe](benchmarks/jpeg2000-macos15-20260914.json) on macOS
15.7.9 confirmed that both direct decoding and thumbnail creation reject the raw
fixture. Commit `49e8c40` tests that documented codec-dependent boundary using an
independent fixed-fixture capability probe: capable systems retain all pixel,
dimension and orientation assertions; others must report the exact unsupported
result. JP2 remains unconditional. This does not claim raw J2K rendering on
macOS 15 or replace the separate historical macOS 26 evidence.

After integration, local Ubuntu 24.04 userspace passed formatting, app checking,
the complete workspace tests and strict workspace Clippy. The new recovery
regression also passed after its lint-only adjustment. All 26 recovery tests,
eight upstream renderer CLI tests and the two edited renderer configuration
test bodies passed. The published vendor archive omits fixtures needed to
compile its complete library test suite, so those two configuration test bodies
were exercised in a disposable integration harness instead. Hosted reruns remain
separate evidence; no binary release or native GUI check is implied.
