# Development workflow

GitTurtle combines maintained code contracts with bounded feature work and independent acceptance. The [root guide](../../AGENTS.md) remains the shared contract. [Agent architecture](../agent-guidance.md) explains the role definitions and September 15, 2026 research decisions; [Contributing](../../CONTRIBUTING.md#commit-cohesive-changes) defines atomic Conventional Commits.

Use the shared guides and validation procedures with your chosen development tools. The checked-in agent configurations and unattended controller provide an optional Codex integration; other tools can follow the same contracts directly.

## Choose the working mode

For one coherent interactive milestone, keep acceptance criteria in the task. The coordinator can delegate bounded work while retaining integration and commit ownership. Record progress and evidence so work can continue across sessions with the contributor's chosen tools.

For an authorized list of independent features, the optional local [controller](../../scripts/agent-loop.py) provides fresh implementation sessions and separate verification through its current Codex adapter. It selects eligible work, runs declared checks and records acceptance. No background run starts merely because these files exist.

## Task contracts and ownership

[tasks.json](tasks.json) is the versioned feature specification; [task.schema.json](task.schema.json) describes its format. It holds the queue for the [commit inspector and CI/CD initiative](commit-inspector-and-ci.md), which is resolved on `main` as of September 23, 2026: ten tasks were integrated in `b5d681c` on September 15, and `ci-codeql` is superseded, not passed, as recorded in the [CodeQL retirement](../ci-codeql.md#initiative-scope-amendment). The schema has no status field, so this note records that supersession and the `ci-codeql` entry stays unchanged. The initiative's [coordinator checkpoints](commit-inspector-and-ci.md#coordinator-checkpoints) C0 to C4 remain open. Since September 25, 2026 it is also the backlog for open work left by finished initiatives: `themes-press-distinct`, `themes-evidence-gaps-linux` and `themes-evidence-gaps-macos` carry the last two [themes follow-ups](themes/follow-ups.md), whose list is closed. Eight tooling tasks approved by the owner on September 26, 2026 follow them in the queue; several touch protected paths and run interactively. On September 28, 2026 the owner approved three button focus-ring tasks, which follow those: `vendor-button-focus-ring`, `app-button-focus-ring` and `button-focus-ring-recapture`, in that order. Add only concrete, authorized work; historical milestone notes are evidence sources, not an automatically approved backlog.

A useful task states its observable outcome, acceptance steps, dependencies, owned paths and verification requirements. Keep a feature small enough to review and commit as one cohesive change, including necessary tests and documentation. State whether it promises core behavior, an integrated native workflow, a measurement or a package. Validate the graph before a run: duplicate IDs, missing dependencies and cycles cannot be repaired by guessing a new task order.

For changes to shared interfaces, retained state, persistence, scheduling, platforms or dependencies, apply the [architecture review](architecture-review.md) when defining the contract. Include the material failure, compatibility and lifecycle requirements before starting an unattended attempt. The implementer and verifier assess these through the existing scoped contracts; no extra standing agent or automatic full-project redesign is required.

| Owner | Responsibility |
| --- | --- |
| Coordinator/controller | Task contract, immutable run snapshot, candidate commits, checks, acceptance and integration decisions |
| [Implementer](../../.codex/agents/implementer.toml) | One task's source changes, focused tests and structured handoff; no index or commits |
| [Verifier](../../.codex/agents/verifier.toml) | Independent assessment of the identified candidate, its acceptance criteria and evidence; no source or acceptance edits |
| [Security reviewer](../../.codex/agents/security-reviewer.toml) | Separate assessment of affected trust boundaries and evidence under the [security review contract](security-review.md); no source, acceptance or external-service actions |
| [Librarian](../../.codex/agents/librarian.toml) | Assigned documentation, research and evidence reconciliation; no acceptance-state edits |
| Native/package owner | Exclusive app operation, fixture/state preservation and evidence for the exact build |

Workers cannot edit their grading rules during an attempt. Every `AGENTS.md`, agent/skill definitions, controller source, security-review policy, guidance checker and task specification/schema is protected from unattended patches. Maintain those files through an explicitly scoped interactive change with independent review. The controller records results outside `tasks.json`; a model's claim of success does not update the specification or satisfy missing evidence.

The implementer's `ready` response means its source patch is ready for controller gates. It reports planned external attestations as pending in its summary; the controller creates the candidate before collecting those attestations. `blocked` means a missing capability or decision prevents preparing the patch itself. Neither response is a final acceptance verdict.

## Verification profiles

Profiles are `docs`, `tooling`, `rust`, `native`, `performance`, `package` and `vendor`; a task may require several. Choose the applicable checks using these existing contracts; do not maintain a second copy of their full checklists. Core and preview changes use `rust` plus their relevant focused fixtures. Controller changes use `tooling` in interactive validation; the remaining four profiles require candidate-bound external attestations in addition to applicable executable checks.

Use the `performance` profile for an explicit latency/resource acceptance requirement or a material hot-path change that needs measurement. Applying the performance skill to passive-read correctness does not by itself require a benchmark or that profile.

The controller always runs guidance checks and conservatively adds profiles from the changed paths. Rust source/manifests/toolchain changes require Rust gates; scripts and CI require tooling checks; vendor changes also require vendor evidence. Rust changes under `crates/app/` and native UI toolkit patches require native evidence. Assets, package scripts and production release helpers require package evidence; package-macos.py and the shared package-identity.py are included. Schema-only files and unit-test fixtures do not establish a package claim. Declaring only `docs` cannot bypass these requirements. Use interactive review for a narrower justified validation plan instead of weakening the unattended classifier.

| Changed behavior | Required evidence source |
| --- | --- |
| Guidance/docs | Links, paths, symbols, command names, skill frontmatter and diff; [root validation](../../AGENTS.md#validation) |
| Development controller/tooling | Python unit tests for task contracts, process/Git isolation, acceptance and recovery; [controller](../../scripts/agent-loop.py) |
| Core Git | Relevant disposable fixtures with independent HEAD/ref/index/working-byte and preservation/refusal assertions; [core guide](../../crates/git-core/AGENTS.md#verification) |
| Preview decoder | Format/output behavior and resource/refusal boundaries; [preview guide](../../crates/preview/AGENTS.md#verification) |
| Rust integration | Required locked workspace tests, strict Clippy and formatting; [root validation](../../AGENTS.md#validation) |
| Native interaction | Affected real-app workflow, screenshots/semantic observations and exact source/build; [current matrix](../validation.md#current-validation-guidance), [native QA](../../.agents/skills/gitturtle-native-qa/SKILL.md) and its [visual evidence tiers](../../.agents/skills/gitturtle-native-qa/references/visual-evidence.md) |
| Performance/passive reads | Passive-preservation and scheduling correctness for the affected reads; release measurements with raw samples, cache conditions and tail latency when required by the performance contract or a concrete concern; [performance skill](../../.agents/skills/gitturtle-performance/SKILL.md) |
| Vendor/dependencies | Patch provenance, relevant paired consumers and excluded-package coverage limits; [vendor guide](../../vendor/AGENTS.md) |
| Package/platform | Artifact identity, resources, signature/installation and applicable native smoke; [macOS package procedure](../../.agents/skills/gitturtle-native-qa/references/macos-package.md) or [Linux runbook](../linux.md) |

Security-bearing changes additionally require the [independent security stage](security-review.md). It follows the general verifier (its pre-evidence pass when the candidate needs external evidence, so it too runs before the evidence round) and uses a separate read-only session. The controller conservatively routes all changes except recognized prose/static artwork, including unknown paths, rename endpoints and deletions. A missing or failed security verdict leaves acceptance open; the agent never posts automatic PR comments.

The controller's deterministic checks and the reviewers' independent judgment have different jobs. Reuse successful gate evidence for an unchanged candidate; rerun when inputs change, checks fail or a concrete concern remains. A core fixture, AX tree or compile cannot replace native interaction. A headless benchmark cannot establish native responsiveness.

## Run the controller

The requirements in this section apply to the optional Codex runner. For interactive Codex work, its goal support is also optional and requires an explicit user request; an ordinary feature request does not authorize creating a goal.

Runner requirements: macOS or Linux, Python 3.11 or later, Git with an author identity, the Codex CLI and an existing successful Codex sign-in. This runner uses saved Codex authentication; it does not require a new API key. Preflight checks the installed CLI's required flags; Rust/native prerequisites depend on the selected tasks. Check available commands before execution:

```sh
python3 scripts/agent-loop.py --help
python3 scripts/agent-loop.py validate
python3 scripts/agent-loop.py run --help
```

Start from a clean source checkout with a tracked, committed task specification. The controller snapshots its code, roles and task specification under the ignored run directory, creates an isolated `accepted` clone with independent Git metadata and removes its `origin` remote. Each attempt gets another private clone based on the last accepted commit. Submodule checkouts and patches that change symlink paths need interactive work.

The controller stages explicit intended paths, checks the staged diff and creates the task's atomic Conventional Commit in its attempt clone. After acceptance, it fetches that exact commit from the local attempt clone and fast-forwards the private `accepted` branch. These local Git operations do not update the caller's checkout, index, branch or remote. Candidate patches and failed attempt clones remain available for review.

`run` requires explicit `--model`, `--effort`, `--max-tasks`, `--max-attempts` and `--max-minutes`. Choose model and effort to match the active session; an independent CLI process cannot infer the desktop selection. The controller's own role files (implementer, implementer-hard, verifier and security-reviewer) contain no model or effort overrides, so the explicit selection always applies. Child sessions use controlled configuration and the explicitly supplied selection instead of inheriting unrelated global settings. The adapter disables desktop/browser/plugin tools and runs both reviewers with read-only sandboxes; actual native work belongs to the separate evidence owner.

The following is an illustrative bounded run, not a repository model/effort default or an enabled schedule:

```sh
python3 scripts/agent-loop.py run \
  --model gpt-6-astra --effort high \
  --max-tasks 3 --max-attempts 2 --max-minutes 180
```

`--session-minutes` bounds each child session and defaults to 45. `--max-tasks` caps tasks accepted by the run, and `--max-attempts` caps attempts per task. The task file carries no status, so a new run continues a queue from the branch: a task whose exact commit subject is already reachable from the source head, whose commit changed only paths in its scope, and whose dependencies also landed is recorded as accepted with its `landed` commit, satisfies dependents and does not count toward `--max-tasks`. A squash-merged pull request also counts: the subject may end in one ` (#N)` suffix, and the commit may also change `docs/development/HANDOFF.md` and the run's own queue file, the coordinator's bookkeeping, which is set aside before the scope check and never lands a task by itself. A commit that a later one reverts is not landed unless that revert was itself reverted. A revert names its target by `This reverts commit <sha>` in its body, by `Reverts <owner>/<repo>#N` or `Reverts #N` for the commit whose subject ends in ` (#N)` (what GitHub's Revert button leaves once squash-merged with its description), or by the subject `Revert "<subject>"`, where either subject may carry a ` (#N)` suffix. Any other path outside the scope, or a reworded subject, still leaves the task unrecognised and a run would redo it, so put new work in a fresh queue file and run it with `--tasks` rather than rerunning a queue whose tasks landed that way. Optional `--max-output-tokens` stops between sessions using reported output usage; it is not a hard token or billing cap. A blocked task does not prevent another eligible, unrelated task from running, and its dependents remain ineligible. Gate/child failure, a missing capability, a requested stop and exhausted budgets retain their recorded outcomes; none establishes acceptance.

While it works, `run` and `resume` print one timestamped line per step (`preflight`, then `building`, `gating`, `pre_verifying` for a candidate that needs external evidence, `verifying`, `security_reviewing` and `accepting` with the task and attempt), one per finished gate, and one per task outcome with its reason, such as `awaiting_evidence (required external evidence: native)`; a run in `tmux` or piped through `tee` can be followed from that output. Inspect a run and request a stop using its printed directory:

```sh
python3 scripts/agent-loop.py status --run /absolute/path/to/run
python3 scripts/agent-loop.py stop --run /absolute/path/to/run
```

Do not bypass a lock, manually rewrite the state file, or kill an unrelated Codex process to force progress. On interruption, retain the run directory and use the controller's reconciliation procedure below.

### Coordinator notes

The coordinator can give a task's next attempt guidance without touching its contract:

```sh
python3 scripts/agent-loop.py note --run /absolute/path/to/run --task TASK_ID \
  --text "Name every committed frame in the validation entry."
python3 scripts/agent-loop.py note --run /absolute/path/to/run --task TASK_ID --file /absolute/path/to/note.txt
```

A note is nonempty UTF-8 text of at most 8 KiB (a `--file` must be a regular file, not a symlink), and a task holds at most 32. Each is kept with its UTC time in the task's record in the run state, never in the contract snapshot, and outlives attempts. Every later implementer session receives the task's notes under a heading that says they never change the contract, its scope or its acceptance criteria, and that a note in conflict with the contract yields to it and is reported in the summary; the verifier and the security reviewer see them labelled as context only, not acceptance criteria. Like `attest`, `note` applies at once when the run is free and goes through the run inbox while a controller holds it (below). Changing scope or acceptance still takes a revised contract and a new run.

## Native and external attestations

The unattended controller cannot manufacture native, performance, package or vendor evidence. It preserves the candidate while the required attestation is missing. Dependent tasks wait; other eligible work can continue. A locked desktop, unavailable platform or missing fixture access remains an explicit capability limitation, not a passing check.

An evidence round costs far more than a review, so a candidate that needs external evidence is reviewed before anyone gathers it. Its order:

1. **Gates.** The controller runs the profile's gates on the candidate.
2. **Pre-evidence verification** (`pre_verifying`). The verifier is told which criteria wait for evidence and reports them `unverified`, or `pass` when the repository alone already proves them; it grades every other criterion fully. The verdict passes only when each of those passes. A `fail`, or a `pass` that leaves one of them open, fails the attempt at once, before any evidence is gathered; `blocked` leaves the candidate `review_blocked`.
3. **Security review**, when the changed paths route to one. It needs no external evidence, so it runs now, and a failure also fails the attempt at once.
4. **Park.** Only then is the candidate `awaiting_evidence`, with the missing kinds as its reason. `attest` refuses a candidate whose pre-evidence verification or security review has not passed.
5. **Final verification**, after `attest` and `resume`. The verifier grades the evidence-gated criteria against the attestations and confirms every other, with the pre-evidence verdict as context. The security verdict is reused while the base, candidate, gate record and changed paths are unchanged; a new attestation needs a new final verification only.

A task that needs no external evidence keeps a single verification followed by the security review. Which criteria wait for evidence is part of the contract: a criterion's optional `evidence` list names the kinds it waits for, each one the task's profiles require (an empty list marks one that waits for none); without it, a criterion whose id is `native`, `performance`, `package` or `vendor` waits for that kind and every other criterion waits for none. Mark every criterion that only a measurement or a native observation can settle, whatever its id: the pre-evidence verifier blocks on an unmarked one rather than pass it. A task that requires evidence but marks no criterion as waiting for it, as the contracts queued before October 2, 2026 do, keeps the earlier order: its candidate is parked as soon as its gates pass, `attest` does not wait for early reviews, and one verification and the security review follow the evidence.

Before expensive external checks, inspect `status`: the candidate's `base` must match `accepted_head`. If another task has since been accepted, resume first to rebuild the pending feature against that newer base, within its attempt limit. This creates a new candidate and requires new evidence; the controller refuses stale attestations.

The assigned owner exercises the exact candidate with the relevant existing skill or contract, preserving genuine app state and using disposable repositories for mutations. Record the build/executable identity, platform and desktop/session, fixture, steps, results and limitations. For measurements, retain raw samples and boundaries. Historical evidence for different code does not satisfy the new candidate.

Exactly one owner operates the app at a time, and ownership changes hands explicitly: the next owner starts only after the previous one has confirmed it stopped launching, because two concurrent launches of the same build in one desktop session invalidate each other's observations and waste the run's remaining budget. Give every launch its own seeded preference store addressed by an absolute path, written before that launch; a relative path is ignored and reaches the operator's real preferences.

After real evidence exists, register it with the exact task and candidate SHA:

```sh
python3 scripts/agent-loop.py attest \
  --run /absolute/path/to/run --task TASK_ID --candidate CANDIDATE_SHA \
  --kind native --evidence /absolute/path/to/evidence.json \
  --summary "Affected native workflow passed on the identified build and fixture"
python3 scripts/agent-loop.py resume --run /absolute/path/to/run
```

Use a supported evidence kind from `attest --help`. `attest` refuses a candidate whose base is no longer `accepted_head`, a symlinked evidence path and a file larger than 32 MiB, so register a short text or JSON summary that names the retained bundle and keep captures, raw samples and logs in the run directory or an ignored evidence directory. This records a responsible owner's assertion and evidence; it cannot prove the truth of an arbitrary file. The controller still requires the task's remaining checks and independent verdict. If the candidate changes, repeat the affected checks and attach evidence to the new identity.

`attest` works while the loop runs, so the loop does not have to exit and be resumed for it. With the run lock free it records the evidence at once, and `resume` then verifies the candidate. While a controller holds the lock, `attest` makes every check that needs no lock (its arguments, the evidence file, and the candidate, base and status in the current `state.json`), copies the evidence into the run's `inbox/` with its digest, and queues the request there; `note` queues the same way. The running controller applies queued requests in order under its lock, after reconciling and before it selects each next task, and checks each one again with the same rules, so a candidate that changed or went stale in the meantime is refused. An applied attestation returns its parked task to that run's queue, and the controller reviews it without a stop or `resume`. Each request then moves to `inbox/done/` with an outcome record, `applied` or `refused: <reason>`; a refusal never stops the loop. `status` lists the pending requests and the latest outcomes under `inbox`. A lock that another `note` or `attest` holds for a moment is waited for, up to three seconds, rather than taken for a running loop; a request queued just as the loop exits waits for the next `resume`, which applies it first.

A run records what its saved controller supports in `controller_features`. A run created before the inbox has none, so `note` refuses it, and so does an `attest` that would have to queue: stop that loop and attest with the run's saved controller, as before. With the lock free, `attest` on such a run records the evidence exactly as it always did. The controller's own sessions and gate commands carry `GITTURTLE_LOOP=1`, and `note` and `attest` refuse them whether the lock is free or not, as the lock alone did before the inbox existed; the Claude settings snapshot also denies every spelling of `agent-loop.py`. This restores the guard against workers invoking the controller CLI; it is not a boundary against hostile code running as the same user.

Live account/network operations, installation over a user's app, publication and releases need their specifically authorized context. Removing the clone's remote does not make arbitrary commands harmless or grant permission to contact other systems. The runner is development tooling, not a security boundary against arbitrary same-user code execution.

### Evidence gated by its own commit

A criterion can require the captures or the measurement record to be part of the commit they validate. That is reachable in only one order, because the evidence has to exist on the base the candidate is rebuilt onto:

1. Let the controller gate and pre-verify the candidate and park it `awaiting_evidence`. Do not collect evidence for a candidate that has not passed its gates, its pre-evidence verification and any security review.
2. Clone that attempt's checkout (`attempts/<task>/<attempt>/repo` inside the run) into an ignored working directory named after the candidate, build it in release with an absolute `CARGO_TARGET_DIR` shared by every candidate, and copy the executable to a name carrying the candidate SHA. The shared target directory keeps each rebuild incremental; the renamed executable keeps every observation tied to one identity.
3. Exercise that exact executable for each required lens — the affected native workflow, the measurement, and the visual review when the criterion is visual — at the window size and density the criterion names.
4. Commit the captures, any benchmark record and a dated [validation](../validation.md) entry on the run's `accepted` checkout, not in your contribution branch: those commits have to be the rebuilt candidate's ancestors.
5. Advance `accepted_head` to that commit and `resume`. The pending candidate is now stale, so the controller rebuilds it on the base that carries its evidence, which spends one implementer attempt on a rebuild that contains no new work.
6. Attest the rebuilt candidate only after re-checking it against the committed captures and re-running the measurement on its executable. The evidence has to describe the build being attested, not its predecessor. With the committed `docs/evidence/<task>/scenario.json`, the re-check is `qa.py recheck` and the attestation file `qa.py attestation` ([native-QA tooling](../../scripts/native_qa/README.md#scenarios)).
7. Bring each accepted commit onto the contribution branch as it lands, with `git fetch /absolute/path/to/run/accepted HEAD` followed by `git cherry-pick -x FETCH_HEAD`. Work that exists only inside a run directory is lost as soon as a harness change makes that run unresumable.

Advancing `accepted_head` restales every other pending candidate and costs each one an attempt, so gate and accept evidence-gated tasks strictly one at a time, budget `--max-attempts` for the forced rebuilds, and order the queue so the fewest candidates are waiting when a head advances.

## Evidence, interruption and continuation

Run state lives under ignored `.local/agent-loop/`. Each run retains its specification snapshot, isolated checkout, attempts, prompts, child transcripts, command results, candidate identities and its inbox of queued operator requests with their outcomes. Keep private logs and local paths there; copy only sanitized, useful evidence into versioned documentation. Retain failed candidates for diagnosis instead of discarding the only reproduction.

`resume` reconciles saved state, task/controller snapshots and candidate identity before more work. Inspect `status` and the last attempt first. An unexpected accepted-checkout change or modified evidence requires inspection; it is not silently reset. If the installed controller source has changed, use the saved controller path named by the error to resume with the run's original code. Correct a real failure or supply missing evidence; do not treat a crash, lost reply or budget stop as acceptance.

Child sessions, gates and Git writes have a watchdog and durable process records. The controller opens a private directory for each launch and passes its directory handle and held ownership lock to the watchdog. The watchdog checks that the lock belongs to that directory and uses fixed record names; its command line accepts inherited descriptor numbers, never a record path. Renaming or replacing the directory cannot redirect its writes. Record reads validate the type, ownership and byte limit through the same opened file; writes use exclusive temporary files and atomic replacement within the opened directory. Symlink, hardlink, nonregular and group/world-writable records or process logs are refused. These protections preserve the controller's file boundaries; they do not sandbox arbitrary code running as the same user.

If the controller dies, closing its private pipe requests termination of the owned process group. Resume waits for recorded cleanup before retrying. A substituted ownership lock or unreadable completion record leaves cleanup unconfirmed. If the watchdog itself was killed before confirming cleanup, the run refuses automatic recovery; inspect the retained process record and workload. Never delete the record or signal a saved PID to guess that cleanup succeeded. Commands that deliberately detach into another session fall outside process-group ownership and are unsupported.

An orderly pause retains its unused time. After an abrupt exit, recovery conservatively charges elapsed time since the last saved record, including downtime, and replays reported usage from child transcripts. With an output cap configured, incomplete usage requires explicit renewal of `--max-output-tokens` before another session. That renewal acknowledges an unknown amount; it does not turn reported output into a billing limit. The security stage shares those limits and usage accounting. A successful review is reused only while its inputs are unchanged: the pre-evidence verification binds the base, candidate and gate record, the security review those and its changed paths, and the final verification those and the attestations. An interrupted review keeps the successful reviews before it, and resume reruns only the interrupted one without a new attempt.

A provider/session or gate capability failure pauses the affected work for explicit continuation instead of repeatedly rebuilding it in the same run.

Resume can explicitly provide a fresh `--max-minutes` allowance and new total caps for `--max-tasks`, `--max-attempts` and `--max-output-tokens`; prior accepted tasks, attempts and reported output still count. Omitted options preserve the saved bounds. Read `resume --help` before changing these limits. Exact interruption behavior is covered by the controller's tests; no long-running model-driven feature run is established by this implementation alone.

Accepted candidates accumulate by local fetch and fast-forward in the private `accepted` clone. Review their commits and evidence before transferring changes into a contribution branch. Always preserve unrelated work, use atomic Conventional Commits and a Conventional Commit PR title, and follow the existing [PR policy](../../CONTRIBUTING.md#send-a-pull-request). The controller does not integrate them into your source branch, push them or publish a release.

## Maintain the workflow

Update the closest contract when behavior changes. Use the [research template](research-template.md) for decisions that need current primary sources, compatibility checks and explicit enforcement. The librarian reconciles claims with code and evidence; it does not keep stale instructions byte-identical for caching.

Validate development-tool changes with the controller's focused tests and guidance checks. Run Rust/native gates only when changed product code, dependencies or a concrete product concern requires them. A successful fake-child or disposable-runner test establishes controller behavior, not a completed model-driven product feature or an hours-long production run.

The [orchestration v2 plan](2026-10-02-orchestration-v2.md) lists the controller, QA and operator changes in progress since 2026-10-02 and their order. Until an item lands, the procedure above stays in force.

## Claude Code adapter

The controller can run its fresh sessions through Claude Code instead of Codex with `run --tool claude`. Codex remains the default; a saved run remembers its tool, so `resume` needs no flag. The adapter ([claude.py](../../scripts/agent_loop/claude.py)) launches one non-interactive `claude -p` process per implementer, verifier and security-review session with an explicit `--model`/`--effort`, `--permission-prompts none`, a turn cap, `--output-format json` and `--json-schema` for the same build/review/security result schemas the Codex path validates. Requirements: Claude Code 2.1.257 or later, a completed interactive sign-in (`claude auth status` must report `loggedIn: true`; subscription usage is billed to that account), and committed `.claude/agents/{implementer,implementer-hard,verifier,security-reviewer}.md` role files. Preflight checks all of these before creating a run.

Sessions load the checkout's `CLAUDE.md`, rules, skills and settings-file hooks (Claude Code runs project hooks in `-p` without a trust prompt), plus a settings snapshot the run writes to `controller/claude-settings.json` from [claude-settings.json](../../scripts/agent_loop/claude-settings.json): deny rules for pushes, hard resets, cleans, `--no-verify` commits and snapshot acceptance, an auto-compaction window of 300k tokens, and the child environment (`GITTURTLE_LOOP=1`, `GITTURTLE_TASK_CONTEXT` pointing at the task contract, background subagents disabled, spawn depth 1, at most four concurrent subagents, agent teams off, `INSTA_UPDATE=no`, `CARGO_TERM_COLOR=never`, `CARGO_TARGET_DIR` set to the run's shared build directory). The role file the session uses must match the run snapshot byte for byte, and `.claude/**` and every `CLAUDE.md` are protected paths in every run regardless of tool. Implementer sessions run with `--permission-mode bypassPermissions` inside the private attempt clone; reviewer sessions run with `--permission-mode dontAsk`, only `Read`, `Grep`, `Glob` and `Bash`, and an allowlist of `cargo *`, read-only `git`, `rustc -vV`, `python3 scripts/*` (every repository script, `scripts/gate.py` included; the settings deny the controller itself) and `python3 -m unittest discover -s` prefixes for `scripts/agent_loop`, `scripts/native_qa` and `scripts/ci/tests`, and the controller still fails any review that leaves the candidate dirty. `--sandbox on` additionally enables Claude Code's Bash sandbox with write access to `~/.cargo`, `~/.rustup` and the run build directory; `auto` (the default) enables it only when macOS Seatbelt or both `bubblewrap` and `socat` are available and a probe inside `bwrap` can create and map the nested user namespace Claude Code's sandbox needs, and `off` disables it. Ubuntu's AppArmor restriction (`kernel.apparmor_restrict_unprivileged_userns = 1`) allows `bwrap` but refuses that nested namespace, so there `on` stops at preflight and `auto` runs unsandboxed.

Routing is by attempt number because the task schema has no hardness field, and by default every session runs on the `--model` value, so steps differ only in effort. Attempt 1 uses `--model`/`--effort` (or `--light-model` at `--light-effort`, default `--model` at `medium`, for tasks whose declared profiles are only `docs`/`tooling`); attempt 2 runs on `--model` at `--retry-effort` (default one step above `--effort`), or at `--light-effort` for a docs/tooling task when that is higher, so a retry never drops below its first attempt; attempt 3 onward runs the `implementer-hard` agent on `--hard-model` at `--hard-effort` (default `--model` at the retry effort). Pass `none` to `--hard-model` or `--light-model` to disable a step, or another model to give that step its own. The verifier and security reviewer run on `--model` at `--review-effort` (default `--effort`); their role files pin no model or effort, so a frontmatter value cannot displace the flags the controller passes. Sessions do not inherit Claude Code variables that could replace that selection, such as `CLAUDE_CODE_EFFORT_LEVEL` (which outranks `--effort`), `ANTHROPIC_MODEL` or `CLAUDE_CODE_SUBAGENT_MODEL` (`SELECTION_OVERRIDES` in the adapter). `--max-turns` (default 200) and `--review-max-turns` (default 120) cap each session. `run` resolves every default once, so the run state records a concrete model and effort for each step, and each attempt's agent, model and effort, with the selection its reviews use, under `tasks.<id>.session`. Each session leaves `<role>.prompt.txt`, `<role>.stdout.log`, `<role>.stderr.log`, `<role>.session.json` (session ID, reported usage and cost) and `<role>.response.json` beside the attempt evidence; usage recovery after a crash replays the session records.

A session's result is its structured output when the CLI returns one. Otherwise the adapter reads the session's final message, where a long review often leaves its verdict as text after its reasoning: it takes the last JSON object there, fenced or not, that opens with one of the result's fields as its first key. Either way the object must satisfy the role's whole schema ([result_schema.py](../../scripts/agent_loop/result_schema.py) checks types, where a boolean is never a number, required and closed properties, enums and array items, and refuses a schema keyword it does not enforce before any session starts). A last object that breaks the schema or is not valid JSON is never replaced by an earlier valid one, because the session withdrew that one. The search is bounded (1 MiB of text, at most 64 objects decoded) because it runs after the session, outside its watchdog. When no object qualifies, the adapter tries once more: it resumes the same session (`--resume` with otherwise identical arguments, schema included) and asks only for the result object. A review without a usable session ID is rerun fresh instead; an implementer is not, since its first try already changed the checkout. The retry starts only if the runner's between-session budget (stop, run time, output cap) and the session time the first try left allow it; otherwise nothing launches. The second try leaves its own `<role>.retry.*` records beside the first, with the reason for it in `<role>.retry.json`; its usage counts, and a stop or usage limit applies to it as to any session. Only when it also fails is the result unreadable: a reviewer's task is then recorded `review_blocked` with its candidate, gates and attempt count kept, and `resume` reruns only that review, while an implementer's unreadable result fails its attempt like any unusable implementation. The task id, candidate sha and verdict rules are checked afterwards, as for any result.

A session that Claude Code refuses because the subscription's session, weekly or per-model allowance is exhausted is not a failed attempt: the task is recorded as `interrupted` (or `review_blocked` when a reviewer was refused, keeping the candidate), the run pauses with the limit as its reason, and `resume` continues after the window resets. The attempt number stays consumed; raise `--max-attempts` on resume when that matters. A session that exhausts its turn cap without a structured result is an ordinary failed attempt.

An illustrative bounded overnight run, not a repository default or an enabled schedule:

```sh
python3 scripts/agent-loop.py run --tool claude \
  --model claude-opus-5-5 --effort high --retry-effort xhigh --hard-effort max \
  --light-effort medium --review-effort high \
  --max-tasks 6 --max-attempts 3 --max-minutes 480 --session-minutes 90
```

`python3 scripts/check-agent-guidance.py` also validates the optional Claude Code configuration when `.claude/` exists: agent frontmatter (`name` equal to the file name, lowercase with hyphens except the documented `Explore` override, a nonempty `description`, and only documented `model`, `effort`, `permissionMode`, `memory`, `isolation`, `maxTurns` and list values), skill metadata under `.claude/skills` (symlinked entries included), `.claude/settings.json` structure and hook script paths, hook scripts parsing, and Markdown links in `CLAUDE.md`, agent and rule files.
