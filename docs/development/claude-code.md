# Claude Code support

GitTurtle's development guidance is tool-neutral: `AGENTS.md` and the crate guides are the contract, the Codex definitions under `.codex/` and `.agents/` are one integration, and the files described here are the Claude Code integration. Nothing in the Codex files changed to add it; Claude reads the same guides through `CLAUDE.md` imports and the same skills through symlinks. The decision record is [2026-09-17-claude-code-support.md](2026-09-17-claude-code-support.md).

## Layout

| Path | Role |
| --- | --- |
| `CLAUDE.md`, `crates/*/CLAUDE.md`, `vendor/CLAUDE.md` | Import the `AGENTS.md` beside them; the root file adds Claude-only notes and compaction instructions |
| `.claude/settings.json` | Shared permissions (read-only allow rules, a short deny list) and the hooks below |
| `.claude/agents/*.md` | Roles: `planner`, `implementer`, `implementer-hard`, `verifier`, `security-reviewer`, `code-reviewer`, `design-reviewer`, `performance-reviewer`, `native-qa`, `librarian`, and an `Explore` override |
| `.claude/rules/*.md` | Path-scoped conventions for the app, git-core, persistence, preview, vendor and tooling paths, plus unscoped validation and commit rules |
| `.claude/skills/` | Symlinks to the shared `gitturtle-feature-work`, `gitturtle-native-qa` and `gitturtle-performance` skills, plus `gitturtle-gates`, `gitturtle-gpui-testing`, `research`, `close-task` and `queue-intake` |
| `.claude/hooks/*.py` | `protect_paths.py` (policy files during controller sessions, plus the run's active task queue named by `GITTURTLE_TASKS_PATH`), `stop_gate.py` (fast gate before a turn ends), `reinject_task.py` (task contract after compaction) |
| `.claude/workflows/review-candidate.js` | Four read-only review lenses in parallel, then a refutation pass per finding |
| `scripts/gate.py`, `.config/nextest.toml`, `deny.toml` | The tiered quality gate every tool can run |
| `scripts/agent_loop/claude.py` | The unattended controller's Claude adapter (`--tool claude`) |
| `docs/development/HANDOFF.md` | Human handoff between interactive sessions |

## Working interactively

Start Claude Code at the repository root so `CLAUDE.md` and the shared settings load. The session model comes from your own settings, not from the project; if your default is Fable, start implementation sessions with `claude --model opus` and keep Fable for planning, because delegated subagents inherit the session model unless their agent file pins one. A background subagent's report reaches the caller only at the next turn boundary, so a planning turn that depends on Explore's answer must wait for that report before writing rather than proceeding in parallel. The same holds the other way round: a delegated agent that starts a build, a gate or a capture driver waits for it in the foreground — `until [ -f /absolute/path/done.marker ]; do sleep 10; done` — and keeps polling until it holds the result. Going idle with the job still running reports on nothing, and that report is what the coordinator attests.

- Plan or scope with the strongest model: `claude --model fable --agent planner`. The planner writes only specifications, dated research notes, notices and the task queue; it uses the `research` skill for facts that depend on current versions, and queues work through the `queue-intake` skill — a separate queue file goes to the controller with `--tasks <path>`.
- Implement with `claude` (or `claude --agent implementer` to preload the feature-work and gate skills). The implementer runs the fast gate before finishing; the Stop hook runs it again when Rust files changed and blocks at most twice per session.
- Review by asking for the `code-reviewer`, `design-reviewer`, `performance-reviewer` or `security-reviewer` subagent, or run all four with the `review-candidate` workflow. Ask for the `verifier` when you want an independent acceptance check with a JSON verdict. These roles pin the `opus` alias so a planning session on a larger model does not spend its quota on delegated reads; `Explore` pins `sonnet`.
- Validate native behavior in a desktop session with `claude --agent native-qa`; measure hot paths with `performance-reviewer`. Both register attestations for controller candidates with `python3 scripts/agent-loop.py attest`.
- Close a task with `/close-task`: scope check, gates, evidence, handoff, one Conventional Commit.

## Gates and hooks

The [`gitturtle-gates`](../../.claude/skills/gitturtle-gates/SKILL.md) skill describes the fast and full gates: their stages, strict mode, known failures and the red-gate report.

Hooks live in `.claude/settings.json` so they also fire in headless sessions. The Stop hook only builds when the Cargo target directory is already warm and gives up after two attempts in a session, so it never turns a turn end into a cold build. `stop_gate.py` can be skipped for a session with `GITTURTLE_SKIP_STOP_GATE=1`; `protect_paths.py` only blocks when the controller sets `GITTURTLE_LOOP=1`, and in that mode it also protects the run's active task queue named by `GITTURTLE_TASKS_PATH` (set by the controller for implementer sessions), whichever file the run uses. The deny list blocks force pushes, hook bypasses and blind snapshot acceptance; everything else follows your permission mode.

## Driving the app for evidence

`native-qa`, `performance-reviewer` and `design-reviewer` each need the running app, and on a single-seat
Linux host they share one X display. Exactly one of them owns `DISPLAY=:1` at a time. Say in writing when
the display becomes theirs, and only after the previous owner has confirmed that it stopped launching: on
2026-09-19 two concurrent launches of the same build cost a discarded QA launch and a discarded performance
warm-up, and the QA owner could only report the collision after the fact. Serializing the lenses costs
minutes; repeating a capture set costs an hour.

Every launch gets its own seeded preference store addressed by an **absolute** `XDG_CONFIG_HOME`, written
before that launch rather than once per session. GitTurtle ignores a relative value
(`absolute_environment_path` in `crates/app/src/preferences.rs`) and falls back to `~/.config/gitturtle`,
which is the operator's real store; the themes run's capture helper therefore refuses a launch whose seeded
store is not where the app will look. Record the executable identity, display, scale factor, window size and
fixture with the captures, and register evidence in the order the runbook fixes for
[evidence gated by its own commit](README.md#evidence-gated-by-its-own-commit).

## Models and budget

Fable 5.1 plans, takes the hard tail and reviews finished runs; Opus 5 implements, verifies, reviews and measures; Sonnet 5 explores and handles docs or tooling tasks. On Max plans Fable draws from the same weekly allowance, uses it faster, and is capped at half of it; past the cap, headless sessions bill usage credits without a prompt. Before an unattended run, check `/usage`, and consider disabling usage credits or setting a spend limit in your account settings. Effort defaults to `high`; raise it for a single session with `--effort xhigh` rather than in project settings.

## Session length

Work to about 40% of the context window, then hand off; the rule and its reasons
live in [`CLAUDE.md`](../../CLAUDE.md#session-length).

The unattended loop already holds to it structurally, and that is deliberate:
every attempt is a fresh process, agents cap `maxTurns` (40 for `Explore`, 60–120
for reviewers, 200 for implementers), and the controller caps `--max-turns` and
`--session-minutes`. Raising a cap so one session can finish a task trades a
bounded risk for an unbounded one; prefer splitting the task.

An interactive coordinator has no such cap and is where this goes wrong, because
a long coordinating session accumulates run state, evidence paths and candidate
shas that exist nowhere else. Two habits make stopping cheap:

- Keep [`HANDOFF.md`](HANDOFF.md) current as you go, recording run directories,
  candidate shas and evidence paths when they are produced rather than at the end.
- Preserve accepted work onto the branch before stopping. Accepted commits live
  in the run's `accepted` checkout, not your worktree, and a saved run pins its
  controller by digest — once a harness fix lands, that run refuses to resume and
  anything left only inside it is stranded. `git fetch <run>/accepted HEAD` then
  `git cherry-pick -x FETCH_HEAD`, once per accepted commit as it lands rather than
  as one batch at the end.

## Unattended loop

The existing controller runs Claude sessions with `python3 scripts/agent-loop.py run --tool claude ...`; the [runbook](README.md) documents the options, the per-attempt model routing (base model first, higher effort on the retry, the `implementer-hard` role on a stronger model afterwards, a lighter model for docs and tooling tasks), the settings snapshot each session receives, and the pause when a usage limit is reached. Task contracts, evidence, attestations, acceptance and the private accepted branch work exactly as they do for Codex. Every session is a fresh process with the task contract, the previous attempt's reason and a turn cap; the controller runs the gates and a separate read-only verifier and never trusts the implementer's summary. Implementer sessions run with bypass permissions inside the private attempt clone; `--sandbox on` additionally wraps Bash in the Claude Code sandbox, which on Linux needs `bwrap` and `socat` installed (`--sandbox auto`, the default, enables it only when both are present).

Watch the host for anything that writes into a checkout behind the run. An editor or another agent tool that mirrors Claude's configuration files into `.codex/` or `.agents/skills/` no longer rejects a candidate: untracked files under those two roots are left out of the candidate and listed as `mirror_untracked` on the task record, because nothing a candidate may add lives there. A change to a tracked file under them, or an untracked file anywhere else, is still judged by the task scope and the protected paths. The [decision record](2026-09-17-claude-code-support.md#an-untracked-mirror-stops-a-run-from-accepting-anything) records how the sweep was recognized and worked around while a run was pinned to the earlier controller digest.

## Definition of done

A change is done when the fast gate is green, the full gate is green on the candidate, an independent verifier confirmed each acceptance criterion with evidence, the security reviewer passed when trust boundaries changed, hot-path changes carry a release-mode measurement, visible changes carry native evidence from the affected platform, and the commit is one atomic Conventional Commit. Speed and visual polish are acceptance criteria throughout, not follow-ups.
