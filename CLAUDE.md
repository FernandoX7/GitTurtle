@AGENTS.md

# Claude Code notes

The `AGENTS.md` above is the canonical, tool-neutral contract for this repository; this file only adds what Claude Code needs in order to find and enforce it. Nothing here overrides it.

- The crate and vendor guides load on demand: `crates/app`, `crates/git-core`, `crates/preview` and `vendor` each carry a `CLAUDE.md` that imports the guide beside it. Path-scoped rules in `.claude/rules/` add the Claude-specific test-harness and gate idioms for those paths.
- Roles live in `.claude/agents/`, procedures in `.claude/skills/` (the shared `.agents/skills/` entries are symlinked there), enforcement in `.claude/settings.json` and `.claude/hooks/`. Launch commands, the roster and the unattended loop are documented in `docs/development/claude-code.md`.
- Speed and visual polish are acceptance criteria, not follow-ups. The app is fast and stays fast: repository reads, parsing and decoding run off the UI thread, lists are virtualized, metadata loads before content, and a change on a hot path carries a release-mode measurement before it is done. The UI follows `DESIGN.md`; a visible change carries native evidence, not a description.
- Every role runs on Opus 5.5 (`claude-opus-5-5`); roles differ only in effort. Delegated roles pin the full ID, since the `opus` alias can move, and an effort: `Explore` low, `librarian` medium, the code, design and performance reviewers high. Main-session roles, `verifier` and `security-reviewer` pin neither, so they take the launched selection or the controller's `--model` and `--effort`: `claude --model claude-opus-5-5 --effort <level> --agent <role>`.
- The gates are `python3 scripts/gate.py fast` and `full`, described in the `gitturtle-gates` skill; a Stop hook runs the fast gate when Rust files changed. The verifier and the controller own the final verdict; the implementer never grades its own work.
- When waiting on a cargo log, match its `Finished` line or a line that starts with `error`: crate names such as `thiserror` contain a bare "error". Release builds take about a minute on the Asahi development host, so build and measure in the foreground rather than polling.
- Delegated implementers with `isolation: worktree` build in their own worktree and target directory, never with `CARGO_TARGET_DIR` set to the main checkout's `target/`: cargo hashes path packages relative to the workspace root, so a worktree build there writes the same artifact names and silently replaces the coordinator's binaries with the worktree's sources (on 2026-10-27 the main test binary lost newly added tests, and a vendored kit change went missing, while implementers were building). Sequential sharing is unsafe too: cargo judges freshness by modification times, so a checkout whose sources are older than another checkout's artifacts reuses them (a release build of the main checkout once returned the worktree's executable in 0.2 s). Give every checkout its own target, say so in the implementers' brief, build your own checks in a separate `CARGO_TARGET_DIR` such as `.local/target-coord`, and confirm each release executable's embedded commit (`strings <exe> | grep <sha>`). The coordinator integrates their commits, owns native QA and release measurements, and builds release executables one at a time.
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
