# Orchestration v2 (plan of 2026-10-02)

The owner approved every item below on 2026-10-02 and delegated their design and order to the coordinator. This file is the plan and its acceptance; each item lands as its own PR, and the item's PR updates the guides it changes (`docs/development/README.md`, `docs/development/claude-code.md`, the skills and the agents). Mark an item done here, with its PR, when it lands.

## Why

The finish-all run `20261001T183630Z-4fa29335` (queue `queue-2026-10-02.json`) accepted six tasks between 18:36 and 00:00 UTC on 2026-10-01. What it showed:

- **The forced rebuild is the largest cost.** An evidence-gated task needs its frames on the base its candidate is rebuilt onto ("Evidence gated by its own commit" in the [README](README.md)). Every such task therefore spent one implementer attempt, a release build, a native re-check and a second `attest` on a patch that came back identical: 15 to 18 minutes per task, and one attempt each. `worktree-rows-ring-and-selection` needed all three attempts because of it.
- **Late failures waste evidence rounds.** The verifier only grades after the native attestation, so `worktree-rows-ring-and-selection` lost a whole evidence round to a test that asserted the builder flag rather than the AccessKit node.
- **No channel to the next attempt.** The design review's certain fixes (DESIGN.md wording) could not ride the free rebuild, and a candidate wrote "Native evidence: pending" into `docs/validation.md` that only an instruction could have prevented.
- **Brittle result parsing.** A security review passed with no findings, but wrote its JSON as text after a sentence, so the controller recorded `review_blocked`; a `resume` re-ran it.
- **Agents rebuild the same QA tooling.** Each `native-qa` run re-derived its driver, crops and ring analysis from older bundles (100k to 200k tokens per capture, 70k to 100k per re-check), although a re-check is fully mechanical.
- **Contracts missed states.** The worktree rows contract named a focused selected row but not a focused unselected one, where the kit's double outline remained, and asked for an AccessKit "selected" state that a Button does not report.
- **The coordinator's context is the binding limit.** Relaying events, landing PRs by hand and briefing agents ended the session after five tasks.
- **One chain serialises everything.** The queue chained 27 tasks to keep advances from restaling candidates, so the loop sat idle during every 30 to 60 minute evidence round.

## Order and items

Each item names its owner files, design and acceptance. Controller items change `scripts/agent_loop/**` and its tests; run `umask 022 && python3 -m unittest discover -s scripts/agent_loop -t scripts -p 'test_*.py'` and `python3 scripts/check-agent-guidance.py`, then the `review-candidate` skill, before each PR.

0. **Retire run `20261001T183630Z-4fa29335`.** Land the in-flight `tab-reveals-branch-and-tag-rows` with the old procedure, then `agent-loop.py stop` it. Its directory stays (deleting runs is the owner's). From then on, editing `scripts/agent_loop/**` is authorised: the run will not resume, and its remaining 21 tasks move to the new queue (item 8).

1. **Tolerant reviewer results** (`scripts/agent_loop/` Claude adapter and runner). When a reviewer session has no `structured_output`, take the last top-level JSON object in its `result` text and validate it strictly against the role's schema. If none validates, retry the session once, then record `review_blocked`. Tests cover text plus JSON, invalid JSON and prose only.

   Decisions:
   - **Every role, one bar.** The implementer, verifier and security reviewer share the mechanism, and structured output is held to the same strict check as text: a resumed turn that only returns the object is cheap, and two bars would let the laxer one decide.
   - **The last result-shaped object decides.** Candidates are objects that open like a result (a brace, then one of the schema's top-level properties as the first key); only they are decoded (`raw_decode`), and a candidate decoded whole is skipped past. The last candidate must validate, and an earlier valid one never stands in for it. That holds whether the later one breaks the schema or is not valid JSON at all (a trailing comma, a stray quote, a cut-off end, a repeated key): a candidate that fails to decode decides when it starts after the last decoded one or its decode error lies past that one's start, so it encloses it. An unrelated object quoted after the verdict cannot hide it, a broken draft before a valid final does not block it, and a pass followed by a malformed corrected verdict is retried rather than accepted as a pass the session withdrew. The review pass found the unparseable-correction case (the base would have accepted the draft too).
   - **Structured output wins when present.** A dict in `structured_output` is the result, and if it fails the schema the session is retried; the text is read only when there is no structured output, so the two sources are never mixed.
   - **A closed, strict validator** (`scripts/agent_loop/result_schema.py`): every node names one type (a boolean is neither an integer nor a number, NaN is no number), objects must be closed, arrays name their items, enums match their type, and any keyword beyond `type`, `properties`, `required`, `additionalProperties`, `enum`, `items` and `description` stops the session before it starts. A later schema change therefore cannot weaken the check silently. Duplicate keys and `NaN`/`Infinity` are not JSON objects to it, so a repeated `verdict` key cannot flip a verdict unseen.
   - **Resume with identical arguments.** The retry adds `--resume <session_id>` ahead of the variadic tool options and changes nothing else, so agent, model, effort, permissions, settings, environment and `--json-schema` stay the same; its fixed prompt asks for the object only, without redoing the work. Only a UUID session ID is resumed, so a reported value cannot become an option. Without one a review is rerun fresh with its original prompt, as its checkout is read-only; an implementer is not, because its first try already changed the checkout, and its unreadable result fails the attempt. Preflight now requires `--resume` in `claude --help`.
   - **The runner's budget, checked first.** The retry is a second session inside one adapter call, so before it starts the adapter asks the runner's between-session check (`budget_stop`: stop, run time, output cap and incomplete usage, handed over when the runner builds a Claude adapter), the stop flag and the session time the first try left (`timeout - elapsed`). If any is spent, nothing launches: a review raises `EnvironmentBlocked` (`review_blocked`, candidate kept, then the run pauses) and an implementer raises `LoopError` (the attempt ends as the runner ends one past its budget). A session refused before launch writes no prompt record and does not mark usage incomplete, so it cannot falsely pause a run under `--max-output-tokens`.
   - **Separate records.** The second try writes `<role>.retry.{prompt.txt,stdout.log,stderr.log,process.json,session.json}` and `<role>.retry.json` (the reason and the resumed session), and usage recovery after a crash counts retry sessions too.
   - **Unreadable after the retry.** Only then does the adapter raise `MalformedResponse`. After the gates (verifier or security review) the runner still records `review_blocked` and keeps the candidate and the attempt count. Before a candidate exists (the implementer's result), it now records the attempt `failed` with the reason, spending it like any unusable implementation: the old `review_blocked` there had nothing to review, and every later `resume` crashed on the missing `required_evidence`. The stricter check makes that path likelier, so the fix lands with it.
   - **A defensive repair, not a migration.** A run resumes only under the controller that created it, so no earlier controller's records reach this code, and no path here writes a `review_blocked` record without a candidate any more. `reconcile` still turns one (from a defect or a hand-edited state) into `failed`, so it is retried while attempts remain, or counts against the limit, instead of crashing the selection loop.
   - **Bounded text.** The search runs after the session, outside its watchdog and stop flag, so it is bounded: a final message over 1 MiB is not searched, at most 64 candidates are decoded (more is refused), each decode reads the text at most once, and a decoder recursion error counts as invalid JSON. Every refusal is retried like any unreadable result. Before this, a message of nested unclosed openers rescanned the text from each brace (measured at 1.1 to 14 s for 1000 of them in 1 MiB; 0.07 s now).

2. **Coordinator notes** (`agent-loop.py note --run R --task T (--text S | --file F)`). Notes are appended under the run's lock, with UTC time, to the task's record, and included in that task's next implementer prompt as coordinator guidance. They never change the contract or its acceptance; the verifier sees them as context only. Tests cover the lock, the prompt and a note that tries to amend acceptance (shown, not applied).

3. **Verify before evidence.** After the gates pass, and before parking a task `awaiting_evidence`, run the verifier on every criterion that needs no external evidence, and the security review, which needs none. A failure spends the attempt now, before any evidence round. After `attest`, the final verification grades the evidence criteria and the security verdict is reused while the candidate's sha is unchanged. Tests cover a pre-evidence failure, a pass and the reuse.

4. **Evidence on top of the candidate** (replaces the forced rebuild). The evidence owner commits frames, benchmark records and the dated validation entry as a child `E` of candidate `C` and registers it with `attest --evidence-commit E --evidence-repo PATH`. The controller accepts `E` only if:
   - its only parent is `C`;
   - its diff touches only evidence paths inside the task's scope (`docs/evidence/<task>/**`, `docs/validation.md`, `docs/benchmarks/**`) and nothing built into the executable;
   - the docs gate passes on it.

   The verifier grades `C` and `E` together, and acceptance fast-forwards `accepted` to `E`. No rebuild, no re-check, no `accepted_head` edit, no spent attempt. Rewrite the README's "Evidence gated by its own commit" and the `gitturtle-feature-work` and `gitturtle-native-qa` skills to match. Implementers are told not to write evidence entries or "pending" placeholders; the evidence owner writes the dated entry. Find out whether the verifier session can view committed PNGs; if it can, have it view each frame the entry names. `advance-head.py` becomes unnecessary.

5. **Native QA as tooling** (`scripts/native_qa/**`; can run in parallel with items 1 to 4).
   - **Scenarios:** `qa.py scenario run SPEC --build base=EXE --build cand=EXE --out BUNDLE` runs a declarative scenario: launch, palette commands, keys, wheel, pointer parking and named captures. Its analysis is ring sides and contrast around a control, clearance to a neighbour, fills against a surface, and masked compares. It then crops with named boxes and writes the commit manifest and the privacy scan.
   - **Re-check:** `qa.py recheck SPEC --exe EXE --committed DIR` re-captures a candidate and compares bytes with the committed crops.
   - **Attestation:** `qa.py attestation` writes the attestation JSON from a bundle.
   - **Committed spec:** the scenario spec is committed beside the frames (`docs/evidence/<task>/scenario.json`), so any later build can be re-checked by command.
   - **Tests:** unit tests for the analysis on synthetic images.

6. **Operator tooling in the repository** (`scripts/operator/**`, a `gitturtle-operate-loop` skill; after item 4). The procedures the coordinator now carries in its prompt become tracked scripts:
   - `resume.sh` strips `CLAUDE*` variables and replaces a finished tmux session (log ends in `[loop process exited]`), refusing while a loop is alive.
   - `watch.sh` emits only step, gate-failure and outcome lines, and reports a vanished session.
   - `build-release.sh` builds in its own target, retries once on a rustc SIGSEGV, copies the executable and deletes the target.
   - `land.py TASK` runs the whole landing:
     - a worktree from `origin/main`, then the accepted commits fetched and cherry-picked with `-x`;
     - a patch-identity check, and a `docs/validation.md` merge that keeps both entries, newest first, with the evidence lines unchanged;
     - an optional HANDOFF commit and a PR body built from the attestation;
     - CI waited on, merged on green, `main` fast-forwarded, and the worktree and sources cleaned up.

   The skill states the operator loop, the evidence-owner duties (display ownership, `idle-delay`, the lock check) and the notification points, so a new session needs a short prompt.

7. **Rebase without an implementer, and work during evidence** (after item 4).
   - **Mechanical rebase:** when `accepted_head` advances, the controller first rebases each pending candidate (and its evidence commit, if any) without an implementer session. If the rebase is clean and the gates pass, it keeps the attempt count and asks only for `qa.py recheck` evidence. If the frames differ, the evidence is redone; a conflict or a red gate falls back to a normal attempt.
   - **Concurrency:** with `--max-awaiting-evidence N` (default 3), the controller keeps implementing eligible tasks while up to N candidates wait for evidence.
   - **Graph:** dependencies become real ones only (shared files or semantics); the planner stops chaining for restale avoidance.
   - **Tests:** a clean rebase, a conflicting rebase, a red gate after rebase, and the evidence cap.

8. **Contracts that name every state, and a re-cut queue** (planner agent, `queue-intake` skill, a new `queue-2026-10-03.json`).
   - **States:** a visual task's native criterion lists the states it covers (selected, focused, hovered, pressed, disabled, in combination where they meet), the palettes and the text sizes.
   - **Feasibility:** each assertion names how it can be observed (for example, AccessKit reports `is_selected()` as `None` for a Button, so ask for `toggled()`).
   - **Re-cut queue:**
     - the 21 remaining tasks of `queue-2026-10-02.json`, with real dependencies only;
     - the follow-ups in HANDOFF (the kit's focused Button border, the selected-row accent marker, the worktree rows' hover, the Reflog file-row tooltip and entry-list scrollbar, the same-frame reveal, DESIGN.md's reveal sentence, scroll cues for the chooser and Tags, and centred worktree labels);
     - the gpui-base Checkbox and Switch AccessKit disabled flag, after the Switch tasks.

9. **Pilot, then the full run.** Start a new run on two or three tasks from the new queue (one rust-only, one native, one with evidence concurrency) to exercise items 1 to 7 end to end; fix what it shows, then run the rest.

## What stays

The evidence standard does not change: release builds of base and candidate in their own targets, `qa.py identity`, Mutter input on `:0`, privacy scans of committed bytes, a design review of visible changes, a dated validation entry naming every frame, and performance measured per [gitturtle-performance](../../.agents/skills/gitturtle-performance/SKILL.md). The verifier and the controller still own acceptance; implementers never grade their own work.

## Status

| Item | State | PR |
| --- | --- | --- |
| 0 Retire the run | done 2026-10-02: `tab-reveals-branch-and-tag-rows` landed, run stopped | #136 |
| 1 Tolerant reviewer results | done 2026-10-02 | this PR |
| 2 Coordinator notes | open | |
| 3 Verify before evidence | open | |
| 4 Evidence on top | open | |
| 5 Native QA tooling | open | |
| 6 Operator tooling | open | |
| 7 Rebase and concurrency | open | |
| 8 Contracts and queue | open | |
| 9 Pilot and run | open | |
