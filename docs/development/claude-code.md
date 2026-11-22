# Claude Code support

GitTurtle's development guidance is tool-neutral: `AGENTS.md` and the crate guides are the contract, the Codex definitions under `.codex/` and `.agents/` are one integration, and the files described here are the Claude Code integration. Nothing in the Codex files changed to add it; Claude reads the same guides through `CLAUDE.md` imports and the same skills through symlinks. The decision record is [2026-09-17-claude-code-support.md](2026-09-17-claude-code-support.md).

## Layout

| Path | Role |
| --- | --- |
| `CLAUDE.md`, `crates/*/CLAUDE.md`, `vendor/CLAUDE.md` | Import the `AGENTS.md` beside them; the root file adds Claude-only notes and compaction instructions |
| `.claude/settings.json` | Shared permissions (read-only allow rules, a short deny list) and the hooks below |
| `.claude/agents/*.md` | Roles: `planner`, `implementer`, `implementer-hard`, `verifier`, `security-reviewer`, `code-reviewer`, `design-reviewer`, `performance-reviewer`, `native-qa`, `librarian`, and an `Explore` override |
| `.claude/rules/*.md` | Path-scoped conventions for the app, git-core, persistence, preview, vendor and tooling paths, plus unscoped validation and commit rules |
| `.claude/skills/` | Symlinks to the shared `gitturtle-feature-work`, `gitturtle-native-qa`, `gitturtle-performance` and `gitturtle-operate-loop` skills, plus `gitturtle-gates`, `gitturtle-gpui-testing`, `research`, `close-task` and `queue-intake` |
| `.claude/hooks/*.py` | `protect_paths.py` (policy files during controller sessions, plus the run's active task queue named by `GITTURTLE_TASKS_PATH`), `stop_gate.py` (fast gate before a turn ends), `reinject_task.py` (task contract after compaction), `rm_shapes.py` (refuses `rm` shapes that always wait for approval) |
| `.claude/workflows/review-candidate.js` | Four read-only review lenses in parallel, then a refutation pass per finding; `args.base` sets the diff base and `args.dir` the worktree under review, and the results come back compact |
| `scripts/gate.py`, `.config/nextest.toml`, `deny.toml` | The tiered quality gate every tool can run |
| `scripts/agent_loop/claude.py` | The unattended controller's Claude adapter (`--tool claude`) |
| `scripts/operator/` | The coordinator's tools for a run: `start.sh`, `resume.sh`, `watch.sh`, `build-release.sh` and `land.py` ([runbook](README.md#operate-a-run)) |
| `docs/development/HANDOFF.md` | Human handoff between interactive sessions |

## Working interactively

Start Claude Code at the repository root so `CLAUDE.md` and the shared settings load. The session model comes from your own settings, not from the project; start sessions with `claude --model claude-opus-5-5`, because main-session roles and any subagent whose agent file pins no model inherit the session model. A background subagent's report reaches the caller only at the next turn boundary, so a planning turn that depends on Explore's answer must wait for that report before writing rather than proceeding in parallel. The same holds the other way round: a delegated agent that starts a build, a gate or a capture driver waits for it in the foreground — `until [ -f /absolute/path/done.marker ]; do sleep 10; done` — and keeps polling until it holds the result. Going idle with the job still running reports on nothing, and that report is what the coordinator attests.

- Plan or scope with `claude --model claude-opus-5-5 --agent planner`, raising `--effort` for a hard investigation. The planner writes only specifications, dated research notes, notices and the task queue; it uses the `research` skill for facts that depend on current versions, and queues work through the `queue-intake` skill — a separate queue file goes to the controller with `--tasks <path>`.
- Implement with `claude` (or `claude --agent implementer` to preload the feature-work and gate skills). The implementer runs the fast gate before finishing; the Stop hook runs it again when Rust files changed and blocks at most twice per session.
- Review by asking for the `code-reviewer`, `design-reviewer`, `performance-reviewer` or `security-reviewer` subagent, or run all four with the `review-candidate` workflow. Ask for the `verifier` when you want an independent acceptance check with a JSON verdict. The code, design and performance reviewers pin `claude-opus-5-5` at `high`; `verifier` and `security-reviewer` pin nothing and take the session's model and effort.
- Validate native behavior in a desktop session with `claude --agent native-qa`; measure hot paths with `performance-reviewer`. Both register attestations for controller candidates with `python3 scripts/agent-loop.py attest`, which works while the loop runs: it queues in the run's inbox and the controller applies it when the step in progress ends, within 30 seconds while the loop idles for evidence.
- Close a task with `/close-task`: scope check, gates, evidence, handoff, one Conventional Commit.

## Gates and hooks

The [`gitturtle-gates`](../../.claude/skills/gitturtle-gates/SKILL.md) skill describes the fast and full gates: their stages, strict mode, Git isolation and the red-gate report.

Hooks live in `.claude/settings.json` so they also fire in headless sessions. The Stop hook only builds when the Cargo target directory is already warm and gives up after two attempts in a session, so it never turns a turn end into a cold build. `stop_gate.py` can be skipped for a session with `GITTURTLE_SKIP_STOP_GATE=1`; `protect_paths.py` only blocks when the controller sets `GITTURTLE_LOOP=1`, and in that mode it also protects the run's active task queue named by `GITTURTLE_TASKS_PATH` (set by the controller for implementer sessions), whichever file the run uses. `rm_shapes.py` exists because Claude Code holds a removal whose target it cannot resolve statically for explicit approval, and neither an allow rule nor the bypass mode skips that prompt, so one such command parks a long session until somebody answers. The hook refuses the two recurring shapes first, a relative glob after a `cd` (`cd run && rm -rf cfg/*`) and a target that starts with a shell expansion and can be a top-level path when it is empty (`rm -f $E/*.png`), and tells the agent to write the literal absolute path instead; `python3 -m unittest scripts/test_claude_rm_hook.py` covers it. The deny list blocks force pushes, hook bypasses and blind snapshot acceptance; everything else follows your permission mode.

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
[evidence committed on top of the candidate](README.md#evidence-committed-on-top-of-the-candidate).

## Models and budget

Every role runs on Opus 5.5 (`claude-opus-5-5`); roles differ only in effort. Agent files and long unattended runs name the full ID, because the `opus` alias can move to a newer model. `Explore` runs at `low`, `librarian` at `medium`, and the code, design and performance reviewers at `high`; the planner, both implementers, `native-qa`, `verifier` and `security-reviewer` pin nothing and take the session's or the controller's selection. Opus 5.5 accepts `low`, `medium`, `high`, `xhigh` and `max` and defaults to `medium`; thinking is always adaptive, and effort sets its depth ([research note](2026-09-30-opus-5-5-effort.md)). Before an unattended run, check `/usage`, and consider disabling usage credits or setting a spend limit in your account settings. Choose effort per session with `--effort` rather than in project settings.

## Session length

Work in bounded sessions and hand off cleanly before context degrades. Keep [`STATE.md`](STATE.md) (`<= 60` lines, overwritten) current with active branches, commits, and next steps, and graduate durable lessons into `.agents/skills/`.

## Retired unattended controller

The earlier standalone unattended loop (`scripts/agent-loop.py`, `scripts/agent_loop/`, `scripts/operator/`, and `gitturtle-operate-loop`) was retired in November 2026 in favor of direct multi-agent coordinator sessions, isolated CoW worktrees (`scripts/dev/worktree.py`), canonical `.agents/roles/` and `.agents/skills/` synced via `scripts/dev/sync_adapters.py`, and universal `.githooks/`.

## Models and inheritance

All canonical roles in `.agents/roles/` and generated adapters in `.claude/agents/` and `.codex/agents/` omit hardcoded `model` and `effort` overrides so delegated work inherits the caller's active session settings (enforced by `scripts/check-agent-guidance.py`). Choose model and effort per session rather than pinning them in project config.

## Definition of done

A change is done when the fast gate is green, the full gate is green on the candidate, an independent verifier confirmed each acceptance criterion with evidence, the security reviewer passed when trust boundaries changed, hot-path changes carry a release-mode measurement, visible changes carry native evidence from the affected platform, and the commit is one atomic Conventional Commit. Speed and visual polish are acceptance criteria throughout, not follow-ups.
