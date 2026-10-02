---
name: gitturtle-operate-loop
description: Operate GitTurtle's unattended controller as its coordinator with the tracked scripts/operator tools - start, resume, stop and watch a run, diagnose failed or review-blocked tasks, steer with notes, own the evidence round and land accepted tasks. Use in an interactive coordinator session; never inside a controller session.
---

# Operate the GitTurtle loop

The controller owns attempts, gates and acceptance; the coordinator runs it, relays what happens, collects the evidence it cannot, and lands what it accepted. Read the current run section of [HANDOFF](../../../docs/development/HANDOFF.md) and the [controller runbook](../../../docs/development/README.md#run-the-controller) first. Never edit a run's `state.json`, bypass its lock or delete a run directory (the owner's).

## Start, resume, stop

```sh
scripts/operator/start.sh docs/development/<queue>.json --tool claude --model <model> --effort <effort> \
  --max-tasks N --max-attempts N --max-minutes N [other run options]
scripts/operator/resume.sh <run> [--max-minutes N --max-tasks N ...]
python3 scripts/agent-loop.py stop --run <run>
```

- The model, efforts and budgets are the owner's; take them from the brief or HANDOFF, never from habit. The scripts add none.
- Both scripts strip every `CLAUDE*` variable, run the loop in tmux session `gitturtle-loop` and print the new console log (`.local/agent-loop/console-<UTC>.log`); `start.sh` then prints the run directory. `<run>` may be a directory or its name under `.local/agent-loop`.
- `resume.sh` runs the run's saved controller (`<run>/controller/scripts/agent-loop.py`), the only copy a run resumes on after `main` changes the controller, and falls back to this checkout's for a run without one; stderr names the one it used.
- `start.sh` needs a clean checkout on `main`. A finished session (its log ends in `[loop process exited]`) is replaced; while a loop is alive both refuse.
- `stop` ends the loop at its next safe point; the log then ends in `[loop process exited]`. Record the run, the log and the reason in HANDOFF when you start or stop one.

## Watch

Arm the Monitor tool on `scripts/operator/watch.sh` (no argument: the newest console log) at its maximum timeout, and re-arm it whenever it times out. It prints only step lines, gate failures, task outcomes, inbox outcomes (`inbox <kind> for <task>: applied`, or `refused: <reason>`, which needs you), idle waits, the final `paused|complete|blocked: reason` line, controller errors, `[loop process exited]` (then exits), and one `ALERT` line if the tmux session vanished without that marker. Relay each line to the owner as one line. After a resume, arm it on the new log.

On `ALERT` or an exit you did not expect, read the last 40 lines of the log and `python3 scripts/agent-loop.py status --run <run>` before anything else.

## When a task fails or is review-blocked

1. `status --run <run>`: the record's `reason` and `directory` (the attempt).
2. A gate failure: `<attempt>/checks/gates.json` and the failing `<attempt>/checks/<gate>.log`. A verifier failure, before the evidence round (`pre_verifying`) or after it, or a security failure: the `verdict.json` the record's `review` or `security_review` names. A session problem: `<attempt>/<role>.stderr.log` and the log tail.
3. Tell the owner the cause in two or three sentences, and whether another attempt is worth it: yes for a fixable defect in the patch with attempts left; no when the contract is wrong or infeasible (re-cut it with the `queue-intake` skill), a capability is missing, or the same failure repeats.
4. `review_blocked` with no usable verdict re-runs only the review on a plain `resume`; it spends no attempt.

Steer the next attempt with `python3 scripts/agent-loop.py note --run <run> --task <task> --text "..."` (or `--file`). A note reaches the implementer as guidance; it never changes the contract or its acceptance. It works while the loop runs.

## Own the evidence round

When a task reaches `awaiting_evidence`, the coordinator is its evidence owner, or hands it to exactly one `native-qa` or `performance-reviewer` agent with the display.

- **One owner of the display at a time**, handed over in writing, after the previous owner confirms it stopped launching.
- `gsettings set org.gnome.desktop.session idle-delay 0` when evidence work starts; `gsettings set org.gnome.desktop.session idle-delay 900` at handoff.
- Before every display handover, `gdbus call --session --dest org.gnome.ScreenSaver --object-path /org/gnome/ScreenSaver --method org.gnome.ScreenSaver.GetActive` must print `(false,)`; otherwise ask the owner to unlock and wait. Then run `python3 scripts/native_qa/qa.py display-check`.

The round, with candidate `C` on base `B` (from `status`; `B` must equal `accepted_head`):

1. Clone each source into `.local/evidence/<task>/src-base-<B7>` and `src-cand-<C7>`, and build them one at a time: `scripts/operator/build-release.sh <src> .local/evidence/<task>/gitturtle-<base|cand>-<sha7>`. It uses a fresh target, retries a rustc SIGSEGV once, prints `--build-info` and sha256, and deletes the target. Delete the base clone once built; `land.py` removes `src-cand-*`.
2. `qa.py identity` for both executables, then `qa.py scenario run` with the task's scenario (or `qa.py recheck` for a rebased candidate) as the [native-QA skill](../gitturtle-native-qa/SKILL.md) and [scripts/native_qa/README.md](../../../scripts/native_qa/README.md) describe; measurements follow the [performance skill](../gitturtle-performance/SKILL.md).
3. Ask the `design-reviewer` for every visible change, and record each decision you take with its reason in the validation entry and HANDOFF.
4. Commit `E` on top of `C` in the candidate clone: the crops and `scenario.json` under `docs/evidence/<task>/`, any `docs/benchmarks/` record, and the whole dated `docs/validation.md` entry naming every frame, within the rules of [evidence committed on top of the candidate](../../../docs/development/README.md#evidence-committed-on-top-of-the-candidate).
5. `qa.py attestation --bundle <bundle> --task <task> --candidate C --base B --evidence-commit E --out .local/evidence/<task>/attestation-<C7>-native.json` writes the attestation; `land.py` builds the PR body from its fields. Register it with `python3 scripts/agent-loop.py attest --run <run> --task <task> --candidate C --kind native --evidence <json> --summary "..." --evidence-commit E --evidence-repo <candidate clone>`. While the loop runs, the request is queued and `watch.sh` shows its inbox outcome; keep the clone until then. A red docs gate on `E` keeps the task `awaiting_evidence` without spending an attempt: commit a corrected `E` and attest again with `--replace-evidence-commit`.

The [feature-work skill](../gitturtle-feature-work/SKILL.md) and the [runbook's attestation section](../../../docs/development/README.md#native-and-external-attestations) own the evidence rules; `--help` of each command is authoritative for its flags.

## Review delegated work

Before you commit or open a PR for work an agent did in its own worktree, run the `review-candidate` workflow (`.claude/workflows/review-candidate.js`) with `{base: "origin/main", dir: "<the agent's worktree>"}`. Every lens works in that tree, and the result stays small: the confirmed findings in brief, the refuted and unverified ones as one line each. Send the confirmed ones back to the agent with the fixes you want.

## Land

Land accepted tasks one at a time, in acceptance order, since each range sits on the previous one:

```sh
python3 scripts/operator/land.py --run <run> --task <task> --dry-run
python3 scripts/operator/land.py --run <run> --task <task> [--handoff-commit <patch>]
```

`land.py` refuses a task while any task the run accepted ahead of it is missing from `origin/main`. The dry run cherry-picks onto a fresh `origin/main` worktree, checks patch identity and prints the PR it would open, then removes its worktree. The real run then pushes `claude/land-<task>`, opens the PR, waits for its checks, squash-merges that exact head on green, fast-forwards the main checkout when it is clean and on `main`, and cleans up; a source clone that is dirty or holds other commits is kept and named. For the HANDOFF commit, edit HANDOFF in the main checkout, `git diff -- docs/development/HANDOFF.md > .local/land/<task>-handoff.patch`, then restore the file; the patch may touch only HANDOFF and the run's queue file. If checks fail, read `gh run view --log-failed`, then rerun a flaky job or report the defect; `land.py` prints the commands that finish the landing by hand.

## Tell the owner

One line each: anything only the owner can do (unlocking the desktop, another machine, credentials, deleting runs or data, anything outward-facing beyond merging a PR); a run stopping (`paused`, `blocked`, `complete`, `baseline_failed`, `ALERT`); each acceptance and each merged PR; and every decision you took for them, with its reason.

## Hand off before the context runs out

Follow the [session-length rule](../../../CLAUDE.md#session-length): hand off at about 40% of the context window. Keep HANDOFF current as you go (run directory, console log, candidate and evidence shas, executables, bundles, `idle-delay` state), land accepted work before stopping, restore `idle-delay` to 900 unless evidence work continues, and say whether the loop is left running or stopped and what the next session does first.
