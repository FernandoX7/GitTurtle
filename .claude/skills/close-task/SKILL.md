---
name: close-task
description: Close out an interactive GitTurtle task - confirm scope, run the gates, record evidence, rewrite the handoff and make one atomic Conventional Commit. Use only in interactive sessions when the owner says the work is done; the unattended controller owns commits itself.
disable-model-invocation: true
---

# Close a task

The work is not done until each of these holds, in this order.

1. Scope: exactly one task or request is in play. Work that spilled into a second concern is split into its own commit or reverted; policy files, thresholds and task descriptions were not edited to make anything pass.
2. Gates: `python3 scripts/gate.py fast` is green, and `python3 scripts/gate.py full` is green for changes that touch shared interfaces, persistence, scheduling, platforms or dependencies. Read `.local/gate/report.md` first when a gate is red and fix the root cause.
3. Evidence: every acceptance criterion has the command and an output excerpt that shows it, and native, performance or package claims have evidence from the affected platform and build recorded in the dated evidence format the repository already uses. A criterion without evidence stays open and is reported as such.
4. Handoff: rewrite the human section of `docs/development/HANDOFF.md` under fifteen lines with what changed, what is next, the exact commands that work and known issues.
5. Commit: stage the explicit intended paths, inspect the staged diff, and create one Conventional Commit whose body explains why and names the task id when there is one. No `--no-verify`, no work-in-progress commits, no amending published history.
6. If the task cannot be finished, leave the tree green, say exactly where you stopped and what is needed next, and record it in the handoff.
