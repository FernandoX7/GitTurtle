# Public launch checklist

The repository remains private. Changing visibility, publishing releases, and
announcing GitTurtle require the owner's explicit approval. This checklist
separates source publication from a later public binary release.

## Before source publication

- [x] Apply the owner-approved [MIT license](../LICENSE) to GitTurtle-owned code
  and all three workspace manifests. Preserve [third-party terms](../THIRD_PARTY_NOTICES.md).
- [x] Prepare the README, contributor/support guidance, issue forms, PR template,
  and code of conduct. Issues are enabled. Existing `bug`, `enhancement`, and
  `question` labels distinguish defects, requests, and support; `documentation`,
  `accessibility`, `good first issue`, and `help wanted` cover focused follow-up.
- [ ] Review the historical privacy findings below and decide whether the existing
  history and evidence can be public. Current-text redaction does not remove
  earlier revisions. A history rewrite needs separate explicit approval.
- [x] Record current [native validation](public-launch-validation.md) and genuine
  README screenshots. Actual desktop checks remain separate from clean builds,
  headless checks, virtual X11/Wayland sessions, and historical macOS results.
- [ ] Review GitHub Actions approval settings for outside contributors and choose
  whether `main` should require a passing Quality check. Branch protection was
  absent when inspected; do not claim protected branches or a hosted green run.
- [ ] Approve source publication explicitly, then enable and verify
  [private vulnerability reporting](../SECURITY.md) at publication before accepting
  public vulnerability reports. GitHub currently offers the feature on public
  repositories; it cannot be represented as live while this repository is private.

The owner's [GitHub Sponsors profile](https://github.com/sponsors/FernandoX7)
is live and verified, with one-time and monthly contributions. The
[funding configuration](../.github/FUNDING.yml) and README/support links use that
destination. GitHub's API recognizes the funding link after the configuration
reached the private default branch in `3c4fa51`. Visual verification of the
repository Sponsor button was unavailable because no browser was connected;
see the [funding activation record](funding.md). Sponsorship does not require
making this repository public.

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
The owner should review those disclosures before approving publication.

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
Live settings inspection found Actions enabled, default token permissions set to
read, and PR approval by Actions disabled. All Actions were permitted and `main`
had no branch protection. These observations are settings checks, not an audit
of every external dependency.

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
