# CodeQL retirement — September 15, 2026

The maintainer explicitly requested removing CodeQL and incorporating a dedicated
security reviewer into the development architecture. The
[security review process](development/security-review.md) is the current
acceptance procedure. AI review and CodeQL have different coverage; this change
does not establish that all earlier alerts were false positives or that AI review
can replace every kind of security test.

## Applied repository changes

- The advanced CodeQL workflow (GitHub workflow `358949575`) is disabled and its
  checked-in definition and unused optimization helpers are removed.
- Default setup remains `not-configured`.
- [CodeQL merge protection](https://github.com/FernandoX7/GitTurtle/rules/23351839)
  has `enforcement: disabled`. Its original rule is retained for an exact rollback.
- The superseded in-progress PR analysis was cancelled. Historical scans and
  individual alert dispositions are retained.
- Main's required platform checks, strict/up-to-date requirement, review and
  conversation settings were verified unchanged after the transition.

The independent security reviewer runs within development coordination. It is not
an automatic GitHub status check or a new PR-comment bot. Secret scanning, push
protection, Dependabot and private vulnerability reporting are separate controls.

## Initiative scope amendment

The original task queue at `076bb27` and its saved controller run remain immutable.
The CodeQL optimization task and C2's original scan-coverage contract are
**superseded by the maintainer's explicit instruction**, not marked as passed.
The already-prepared optimization source remains recoverable at `986345d`.
Historical baseline measurements in [CI diagnostics](ci.md) retain their actual
scan timings. Do not count the removal of a check as faster execution of that
check or compare changed coverage without identifying it.

## Rollback

Rollback is a deliberate maintainer decision, not an automatic response to a
failing Quality check. Restore a reviewed CodeQL workflow from history and verify
current supported action/extractor versions. Choose exactly one setup mode; do
not enable default setup alongside an advanced workflow. Re-enable the workflow,
collect current PR and main results for Actions, JavaScript/TypeScript, Python and
Rust, and resolve confirmed findings or unsupported coverage first. Then restore
the saved original ruleset enforcement, preserving unrelated branch protection.
The original rule used high/critical security and error thresholds with the
owner's existing bypass. Verify effective rules and the complete merge path after
restoration. Historical green scans do not validate current source.
