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

## Outcome

Recorded after implementation: see the commit series on `claude/claude-code-support` and the checks listed in its pull request description.
