# Claude Code support beside the Codex configuration — September 17, 2026

## Decision and scope

- Question: how to let contributors and the repository owner use Claude Code (Fable 5.1 for planning and hard tasks, Opus 5 for execution and subagents, on a Max subscription) with the same contracts, gates and unattended controller the project already has, without changing the Codex configuration that built it.
- Recommendation: an additive layer. `CLAUDE.md` files import the existing `AGENTS.md` guides; roles, rules, skills, hooks and settings live under `.claude/`; shared skills are symlinked, not moved; the controller gains a `--tool claude` adapter with Codex as the unchanged default; a tiered gate script serves every tool. The Codex files under `.codex/` and `.agents/` and every `AGENTS.md` are untouched.
- Outside this decision: changing the product, the task queue contents, or the CI workflow; adopting agent teams or the Agent SDK (see alternatives).

## Evidence

| Source | Checked date/version | Relevant finding | Limit |
| --- | --- | --- | --- |
| [Claude Code memory docs](https://code.claude.com/docs/en/memory) | 2026-09-17, CLI 2.1.274 | `CLAUDE.md` is the only instruction file read natively; `@AGENTS.md` imports it; nested `CLAUDE.md` files load when files beneath them are read; `.claude/rules/*.md` take `paths:` frontmatter | Path-scoped rules are summarized on compaction |
| [Skills docs](https://code.claude.com/docs/en/skills) | 2026-09-17 | A `.claude/skills/<name>` entry may be a symlink; `.agents/skills` is not scanned | Symlinks must be preserved by contributors' checkouts |
| [Subagents docs](https://code.claude.com/docs/en/sub-agents) | 2026-09-17 | Frontmatter fields `model`, `effort`, `tools`, `permissionMode`, `maxTurns`, `skills`, `omitClaudeMd`; a project agent named `Explore` overrides the built-in; `disallowedTools: Bash(...)` removes all of Bash; a subagent's `permissionMode` is ignored under a bypass, auto or acceptEdits parent | Frontmatter hooks need a trust dialog that headless sessions never see, so hooks live in settings |
| [Hooks reference](https://code.claude.com/docs/en/hooks) | 2026-09-17 | Exit 2 blocks; Stop hooks block at most 8 consecutive turns; `SessionStart` has a `compact` matcher; `SubagentStop` matches `agent_type` | Hook timeouts are non-blocking |
| [Headless docs](https://code.claude.com/docs/en/headless) and `claude --help` | 2026-09-17, 2.1.274 | `-p` supports `--json-schema`, `--max-turns`, `--permission-prompts none`, `--settings`, `--strict-mcp-config`; `--bare` skips hooks, `CLAUDE.md` and OAuth | The error payload at a usage limit is undocumented; the adapter pauses conservatively |
| [Fable models on your plan](https://support.claude.com/en/articles/15424964-claude-fable-models-on-your-plan) | 2026-09-17 | Fable is included on Max up to 50% of the weekly limit, draws from the shared allowance faster, then bills usage credits | No plan-unit multiplier is published |
| [Prompting Claude Fable 5.1](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-fable-5-1), [Prompting Claude Opus 5](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-opus-5) | 2026-09-17 | Opus 5 needs delegation caps, scope discipline and conciseness; Fable 5.1 needs the autonomy and delivering-work blocks, a targeted-edit line and scope-and-tests guidance; prescriptive older prompts reduce quality | The two models need opposite narration instructions, so they live in per-agent files, never in `AGENTS.md` |
| Local host | 2026-09-17 | bubblewrap 0.11 present; cargo-nextest, cargo-deny, cargo-audit, cargo-machete, typos, cargo-insta, cargo-llvm-cov and cargo-mutants installed under `~/.cargo/bin`; the vendored `gpui-pre-macros` 0.3.4 honors `iterations`, `seeds`, `retries` and `on_failure` attribute arguments only | Other hosts install the tools themselves; the gate degrades to warnings without them |

## Compatibility and alternatives

- Agent teams: experimental, interactive only, not resumable, and several times the token cost; reserved for attended debugging, not the loop.
- Agent SDK: runs the same loop on API-key authentication and cannot draw on the Max subscription; the controller runs `claude -p` as a subprocess instead.
- A Stop-hook loop inside one session keeps a growing context; the controller's fresh session per attempt keeps the property the harness research favors.
- Model pinning versus the root guide's inheritance agreement: delegated review, QA and exploration subagents pin an alias (`opus`, `sonnet`) so a planning session on a larger model does not spend its quota on delegated reads; main-session roles (`planner`, `implementer`, `implementer-hard`, `native-qa`) inherit the launched model, and the controller passes the model per session. No project setting replaces the user's model or effort choice.

## Enforcement and acceptance

- Policy paths are protected three ways in unattended runs: the `PreToolUse` hook, the controller's patch validation, and the `.claude` entry in the controller's controlled paths.
- Definition of done is enforced by the Stop hook (fast gate, at most two blocks), the controller's full gate, a fresh read-only verifier with a bound JSON verdict, conditional security review and external attestations.
- `python3 scripts/check-agent-guidance.py` validates agent and skill frontmatter, rules, settings and hooks alongside the Codex files; controller and gate behavior have unit tests.
- Remaining uncertainty: the headless usage-limit payload; whether a saved workflow can be invoked by name from a headless prompt; the plan-unit cost of Fable on Max. Each is worked around rather than depended on.

## Native evidence and the changed-path inference

The controller inferred the `native` profile from paths alone: any `crates/app/**.rs` file in a
candidate demanded a native attestation. The first themes run stalled on it. `themes-palette-sources`
declares `["rust", "docs"]` and its candidate adds only `pub const … : u32` tables plus a
`#[cfg(test)]` module, with `mod sources;` behind `#[cfg_attr(not(test), allow(dead_code))]`; an
attestation for it would have recorded a screenshot of an app identical to the baseline. The defect
is systemic rather than particular to that task: `themes-custom-model` declares `["rust"]` and is
pure model code under the same directory, so it would have stalled the same way.

`scripts/agent_loop/rust_surface.py` now answers the narrower question the profile actually asks.
It blanks comments, removes `#[cfg(test)]` items, and compares what remains; a change keeps the
`native` profile unless it removes no non-blank line and every line it adds is a module, a non-glob
import, a constant, a lint-control attribute or a brace. The additive requirement carries the
safety: retuning `DEFAULT_INTERFACE_TEXT_SIZE` from 13 to 14 repaints every screen and looks exactly
like an inert constant, but it removes the old line, so it stays native. Literals are blanked for
brace matching and kept for comparison, so retitling a button is a rendering change. Anything the
module cannot positively recognize — a function, an `impl`, a `#[cfg(...)]` attribute, a glob import,
an item whose extent cannot be determined — keeps the profile. `profiles_for` falls back to the old
path rule when a caller cannot supply both revisions.

Checked against 120 commits of history: of the 69 that touch app Rust, 65 keep the native profile and
4 are classified inert; all four were read and are `#[cfg(test)]`-only. A saved run keeps its own
controller snapshot, so this applies to runs created after it.

## An untracked mirror stops a run from accepting anything

`clean()` uses `--untracked-files=all`, so a single untracked file anywhere in an
attempt checkout fails `validate_candidate` with "candidate source or HEAD
changed", and `source_root()` refuses a dirty source when a run is created. On
2026-09-18 a Codex mirror — `.codex/config.toml`, `.codex/hooks/`, six untracked
agent `.toml` files and copies of the five Claude-only skills under
`.agents/skills/` — appeared inside all four attempt checkouts of the running
loop at the same instant, including one nothing had touched since it was cloned.
Two candidates that had already passed their gates could not be validated, for
files unrelated to either of them.

The handoff records the same mirror being parked on 2026-09-17; this copy differs
from that one, so something recreates it. It is parked again under
`.local/codex-mirror-2026-09-18/`. Note that `.codex/` mixes tracked and
untracked entries: `implementer.toml`, `librarian.toml`, `security-reviewer.toml`
and `verifier.toml` are in the repository, so clearing the directory wholesale
deletes tracked policy files and `git checkout -- .codex` is needed afterwards.

A run that stops accepting for no visible reason is still worth checking with
`git status --porcelain --untracked-files=all` in the attempt checkout before
looking anywhere else.

Identified on 2026-09-19: the writer is an IDE-level sweep, not a Codex session.
The Orca IDE is running, the user-level Claude Stop hook reports every session
directory to it, and the sweep byte-copies `.claude/skills/*` into
`.agents/skills/*` and `.claude/agents/*.md` into `.codex/agents/*.toml` in every
directory a Claude session has run in, at IDE start and every 12 h (observed
2026-09-18 22:12:54 and 2026-09-19 10:12:54 local). That is why it appears in all
attempt checkouts at the same instant, including one nothing had touched since it
was cloned; it rejected the themes editor's second attempt again on 2026-09-19 at
15:13Z.

While a run is in flight the mitigation has to live outside the repository,
because a controller fix changes the digest that run resumes against:
`.local/themes-evidence/tooling/mirror_janitor.py <absolute run directory> 5`
polls every attempt checkout (`attempts/*/*/repo`) every five seconds and runs
`git clean -fd` with `.codex` and `.agents/skills` pathspecs, so it removes
untracked files only. It is a loop in an ignored directory rather than a service,
so restart it after a reboot and check it with an anchored
`pgrep -f '^python3 .*mirror_janitor.py'`. The controller-side fix is queued as
#23 below.

## Evidence gated by its own commit — September 19, 2026

The themes queue's editor task (`themes-editor`, run `20260919T143547Z-8e458d6a`) made the ordering cost
concrete. Its `native-evidence` criterion requires the captures under `docs/evidence/themes/` and a dated
`docs/validation.md` entry, and `no-reprepare-budget` a release record under `docs/benchmarks/` — that is,
inside the commit they validate. So the captures and the measurement were taken on candidate `3927b57`,
committed onto the run's accepted base, and the candidate the controller then rebuilt on that base,
`b4dd250`, was the one attested — after a pixel re-check against the committed set and a re-run of the
measurement established that its Rust was byte-identical to its predecessor. Five attempts went into the task: the mirror
sweep above rejected the second, the evidence and benchmark commits (`5d0d4b5`, `33e3e63`, `6096b86` in the
run's `accepted` checkout) forced the fifth, and the security-review coverage fault below ended the run
during that fifth attempt's review, after the review itself had passed.

What generalizes is written once, as the runbook's
[evidence gated by its own commit](README.md#evidence-gated-by-its-own-commit). The findings behind it:

- Every acceptance restales every other pending candidate and costs it an implementer attempt, so a queue of
  evidence-bound tasks needs `--max-attempts` headroom for rebuilds that contain no new work, and the tasks
  have to be gated one at a time.
- `attest` refuses an evidence file larger than 32 MiB and a candidate whose base is no longer `accepted_head`
  (`scripts/agent_loop/runner.py`), so the registered artifact is a short text or JSON summary naming the
  retained bundle under an ignored directory, never the bundle.
- Three lenses — native QA, performance and design — need the same executable on the same display. The QA and
  performance launches for `b4dd250` overlapped once and cost a capture launch and a warm-up; the attestation
  summary records the collision. Display ownership and the absolute, per-launch `XDG_CONFIG_HOME` are in the
  [usage guide](claude-code.md#driving-the-app-for-evidence) until the two role files can be edited.
- A delegated agent that starts a build, a gate or a capture driver and returns before it finishes reports on
  nothing the coordinator can attest, so waiting on a completion marker in the foreground is part of the
  delegation contract rather than the agent's own housekeeping.
- Accepted work and its evidence live in the run's `accepted` checkout. Cherry-pick each accepted commit onto
  the contribution branch as it lands (`git fetch <run>/accepted HEAD` then `git cherry-pick -x FETCH_HEAD`);
  a saved run pins its controller by digest, so a batch left for the end is stranded by the first harness fix.

## Queued edits — after the run ends

A saved run pins `.claude/**` and `scripts/**` by digest and refuses to resume once one of those files
changes, so these four wait for the run to finish and then land in their own commits, with tests where the
change is in the harness.

Landed on 2026-09-20 after run `20260919T143547Z-8e458d6a` ended: rows 1 and 2 (plus the empty-run-directory
rule, the 32 MiB attest-evidence constraint and the trace-boundary rule) as `21749e1`, #23 as `cdc54ad`
(untracked files under `.codex/` and `.agents/skills/` are left out of the candidate and recorded as
`mirror_untracked`; the janitor is retired, though `run` still refuses a sweep in the *source* checkout, so
clean it with `git clean -fd -- .codex .agents/skills` first) and #24 as `7af6e8c` (an empty coverage list is
accepted; misshapen paths are a structural retry). The table below records the pre-fix state.

| Edit | Exact change |
| --- | --- |
| `.claude/agents/native-qa.md` and the shared [native-QA skill](../../.agents/skills/gitturtle-native-qa/SKILL.md) | State that `XDG_CONFIG_HOME` must be absolute and seeded before **every** launch, not once per session: the app ignores a relative value and falls back to the operator's real `~/.config/gitturtle`, which is how the 2026-09-18 incident wrote it |
| `.claude/agents/native-qa.md` and `.claude/agents/performance-reviewer.md` | State that exactly one agent owns the display at a time, that ownership is handed over in writing, and that the previous owner stops launching first |
| Harness #23, mirror exclusion, in `scripts/agent_loop/runner.py` | Exclude untracked files under the protected roots (`.codex/`, `.agents/skills/`) from a candidate and record them instead of failing `validate_patch`; nothing a candidate may legitimately add lives there. Retires the janitor above |
| Harness #24, security coverage paths, in `scripts/agent_loop/security_review.py` | The schema says each changed path appears "each once", but line 144 requires a nonempty `paths` per coverage entry and raises a fatal `LoopError`. Either state `minItems` in the schema the session receives or accept an empty list, and make the mismatch a structural retry rather than fatal |

## Outcome

Recorded after implementation: see the commit series on `claude/claude-code-support` and the checks listed in its pull request description.
