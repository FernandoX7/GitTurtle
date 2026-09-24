@AGENTS.md

# Claude Code notes

The `AGENTS.md` above is the canonical, tool-neutral contract for this repository; this file only adds what Claude Code needs in order to find and enforce it. Nothing here overrides it.

- The crate and vendor guides load on demand: `crates/app`, `crates/git-core`, `crates/preview` and `vendor` each carry a `CLAUDE.md` that imports the guide beside it. Path-scoped rules in `.claude/rules/` add the Claude-specific test-harness and gate idioms for those paths.
- Roles live in `.claude/agents/`, procedures in `.claude/skills/` (the shared `.agents/skills/` entries are symlinked there), enforcement in `.claude/settings.json` and `.claude/hooks/`. Launch commands, the roster and the unattended loop are documented in `docs/development/claude-code.md`.
- Speed and visual polish are acceptance criteria, not follow-ups. The app is fast and stays fast: repository reads, parsing and decoding run off the UI thread, lists are virtualized, metadata loads before content, and a change on a hot path carries a release-mode measurement before it is done. The UI follows `DESIGN.md`; a visible change carries native evidence, not a description.
- Delegated review and QA subagents pin the `opus` alias and exploration pins `sonnet`, so a planning session on a larger model does not spend its quota on delegated work. Main-session roles inherit the launched model: choose it with `claude --model <alias> --agent <role>`.
- The fast gate is `python3 scripts/gate.py fast`; the full gate is `python3 scripts/gate.py full`. A Stop hook runs the fast gate when Rust files changed. The verifier and the controller own the final verdict; the implementer never grades its own work.
- In a controller session (`GITTURTLE_LOOP=1`) the controller owns the Git index and commits, and every policy path is protected. Return file changes and the requested JSON; do not stage or commit.

# Session length

Work to about 40% of the context window, then hand off to a fresh session. Past
roughly that point the model's output degrades — confident claims drift from what
the tools actually returned — and a handoff costs far less than re-checking work
produced after the drift starts. This is a discipline, not an enforced gate:
nothing measures it for you, so watch it yourself and stop early rather than at
the last moment.

Plan for the handoff from the start of anything long, especially an unattended
run or a batch of native QA. Keep `docs/development/HANDOFF.md` current **as you
go** rather than writing it at the end, and record run directories, candidate
shas and evidence paths as they are produced. Work that exists only in a run
directory or in this conversation is work the next session cannot use: preserve
accepted commits onto the branch before stopping, because a saved run pins its
controller by digest and will refuse to resume once the harness changes.

Handing off mid-task is normal and does not need permission. Say plainly what is
done, what is in flight with its exact identifiers, and what the next session
should do first.

# Compact instructions

Reaching compaction means the handoff happened too late; treat it as a signal to
wind down, not as a fresh budget.

When compacting, always preserve the task id and contract, the full list of modified files, every gate command run with its result, open findings, and the evidence path. Drop exploratory file contents and superseded diffs.
