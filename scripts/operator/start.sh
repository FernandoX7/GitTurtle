#!/usr/bin/env bash
# Start the unattended loop on QUEUE in tmux session gitturtle-loop, without any
# CLAUDE* variable, appending to a new .local/agent-loop/console-<UTC>.log.
#
# Usage: scripts/operator/start.sh QUEUE [run options...]
#   QUEUE is a committed task file, relative to this checkout or the current directory.
#   Run options pass through to `agent-loop.py run` unchanged; the model, efforts and
#   budgets are the owner's choice, so this script supplies none of them.
#
# Refuses unless this checkout is clean and on main (runs start from main so that
# land.py can bring each accepted range back onto origin/main), and while a loop is
# alive; a finished session (its log ends in "[loop process exited]") is replaced.
# Prints the console log path, then the run directory once the controller names it.
set -euo pipefail
# shellcheck source=scripts/operator/lib.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

(( $# >= 1 )) || die "usage: start.sh QUEUE [run options...]"
queue=$1
shift
if [[ $queue != /* && ! -f $queue && -f $ROOT/$queue ]]; then
  queue=$ROOT/$queue
fi
[[ -f $queue ]] || die "no task file $queue"
queue=$(cd -- "$(dirname -- "$queue")" && pwd -P)/$(basename -- "$queue")
[[ $queue == "$ROOT"/* ]] || die "$queue is outside $ROOT"
queue=${queue#"$ROOT"/}

[[ -z $(git -C "$ROOT" status --porcelain) ]] || die "$ROOT is not clean"
[[ $(git -C "$ROOT" rev-parse --abbrev-ref HEAD) == main ]] || die "$ROOT is not on main"
python3 "$ROOT/scripts/agent-loop.py" validate --repo "$ROOT" --tasks "$queue" >/dev/null \
  || die "agent-loop.py validate refused $queue"

replace_finished_session
log=$(new_console_log)
launch "$log" python3 "$ROOT/scripts/agent-loop.py" run --tasks "$queue" "$@"

for _ in $(seq 1 30); do
  run_line=$(grep -m 1 '^run: ' -- "$log" 2>/dev/null || true)
  if [[ -n $run_line ]]; then
    printf '%s\n' "$run_line"
    exit 0
  fi
  if log_finished "$log"; then
    tail -n 5 -- "$log" >&2
    die "the loop exited at once"
  fi
  sleep 1
done
printf 'start.sh: warning: the controller has not named its run directory after 30 s; follow %s\n' "$log" >&2
