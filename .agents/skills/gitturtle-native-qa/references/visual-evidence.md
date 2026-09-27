# Visual evidence tiers

This is the one home for the evidence a visible change needs: which frames to capture, what they are compared against, who reviews them, and the privacy check before any of them is committed. Other guides link here rather than restating it. Interaction checks follow the [native-QA skill](../SKILL.md), and latency or memory claims follow [gitturtle-performance](../../gitturtle-performance/SKILL.md).

The commands are the tracked [native-QA tooling](../../../../scripts/native_qa/README.md), run from the repository root as `python3 scripts/native_qa/qa.py <command>`. `launch` and `display-check` drive Linux under XWayland only; on another platform, capture with that platform's tools and keep every other step.

## Choose the tier

A change qualifies for the light tier when it is confined to one control, or one state of a control, and changes no palette, theme token or shared layout. Decide before the first capture and record the decision with its reason. Anything else, including a doubtful case, takes the full tier.

## Rules for both tiers

- Identify every build you capture from. `qa.py identity CANDIDATE`, or `qa.py identity BASE CANDIDATE` for a pair, records each executable's sha256 and `--build-info`, flags a `source_tree` other than `clean`, and refuses a pair with the same sha256 or `source_revision`. Frames from a build without that record are not evidence.
- Run `qa.py display-check` before taking the display and again when handing it back.
- Capture each launch with `qa.py launch --for-commit --binary B --fixture F --run-dir /tmp/gitturtle-evidence/runs/<name> --scenario S.json`, in its own empty run directory. A `capture` step named `N` writes `captures/N.png` and records its sha256 in `flow-log.json`; name a recapture of a committed frame exactly like the committed file.
- Committed frames that the change moves are re-taken in the same pull request, and its dated [validation](../../../../docs/validation.md) entry names the tier, the build identities, fixture, window size, backend and scale, each `compare` and `privacy scan` command with its verdict, and which frames changed and why. Keep absolute paths out of `docs/`: use `<worktree>/` or an `$EVIDENCE/` prefix defined once per record.
- Every frame proposed for commit passes the [privacy check](#privacy-check) on its exact bytes.

## Light tier

1. Identify the candidate with `qa.py identity CANDIDATE`.
2. For each frame to compare, write down the expected region, the rectangle `x0,y0,x1,y1` where the affected control is drawn, before running any comparison.
3. In one session, capture from the candidate every affected state of the control, every committed frame under `docs/evidence/` that shows an affected state on this platform (dated frames from another platform stay their builds' evidence), and one control frame: a committed frame that the change should leave untouched and that shows no per-launch timing text, taken with the fixture, store, window size and scenario its validation entry records.
4. Check the base. `qa.py compare docs/evidence/<set>/<control>.png <run-dir>/captures/<control>.png`, with no mask, must report `identical`. Only then do the committed frames serve as the base; the tooling reproduced a committed button-states frame byte for byte on 2026-09-25, which is what makes this possible. If the control frame differs, capture a base as the full tier requires.
5. Compare each recaptured committed frame with its committed file, masking the expected region: `qa.py compare docs/evidence/<set>/<frame>.png <run-dir>/captures/<frame>.png --mask status-timing --mask x0,y0,x1,y1 --json <run-dir>/<frame>-compare.json`. Masked pixels are still counted, so the report shows the change inside the region as well as anything outside it.
6. One design-review pass by the `design-reviewer` role covers the candidate captures of the affected states and the comparison reports, against [DESIGN.md](../../../../DESIGN.md), and attributes each difference to the change.
7. Run the privacy check on every frame proposed for commit.

Escalate to the full tier when any committed frame reports differing pixels outside its masks, when a palette or token turns out to change, or when the reviewer cannot attribute a difference. The light-tier captures do not stand in for the full tier's same-session pair.

## Full tier

1. Build base and candidate into separate executables and identify them with `qa.py identity BASE CANDIDATE`, which must report no problem.
2. In one session, capture base and candidate with the same scenario, fixture, store and window size, each launch in its own empty run directory.
3. Compare them: `qa.py compare <base-run>/captures <candidate-run>/captures --mask status-timing --json <file>`. `compare` pairs frames by file name. Add an explicit rectangle only for another per-launch message or scale, never to cover a region the change can reach.
4. A design reviewer rules on every difference other than per-launch timing text.
5. Commit replacements for the changed frames from the candidate run, after the privacy check.

## Privacy check

The repository is public, so a frame meant for commit must not show private details. Capture it from a fixture copied outside the home directory (for example `/tmp/gitturtle-evidence/<fixture>`, never under `/home` or `/Users`); `launch --for-commit` refuses a fixture outside `/tmp/gitturtle-evidence/` and a run directory under a home root. Every launch's HOME needs a `.gitconfig` whose identity is `GitTurtle QA <qa@example.invalid>`, which `launch` writes; without it, Git falls back to the account name and `user@hostname`, which the profile button and commit rows then display. Before committing, view every frame at full resolution for personal or account names, home paths, hostnames and email addresses, including the project path, profile button, commit author rows, dialogs and diagnostics, and retake any frame that shows one. The 2026-09-24 privacy scrub had to redact earlier frames that showed all four.

Then template-scan every frame proposed for commit: `qa.py privacy scan --redacted --templates <local template directory> <frames>`, on the exact files to be committed, after any crop, resize or re-encode, because an edited file is a different frame. Record the command, each frame's verdict and the wall time with the evidence; a frame without a clean scan of its committed bytes is not ready to commit. The scan costs about 18 s of one core per 1000x680 frame with the current 33 templates, and `--jobs N` scans frames in parallel ([native-QA tooling](../../../../scripts/native_qa/README.md#privacy-templates) keeps the measurement). Use `--redacted` in anything shared, because template file names can themselves be private, and never quote template names, scores or crops. The scan catches known strings but does not replace the viewing above.

`python3 scripts/gate.py full` repeats the scan on added or changed images when templates are configured, and the [CI image privacy scan](../../../../docs/ci.md#image-privacy-scan) does so only when the templates secret is available; a skipped scan is reported as not scanned, never as a pass. `python3 scripts/check-agent-guidance.py` separately rejects home paths, email addresses and configured private strings in the metadata of every tracked image.
