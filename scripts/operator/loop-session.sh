#!/usr/bin/env bash
# Internal: start.sh and resume.sh run this inside the tmux session.
# Usage: loop-session.sh LOG COMMAND...
#
# It drops every CLAUDE* variable here, at the start of the session, because the
# tmux server may have inherited them from a Claude Code session; a child session
# that sees them would run nested in the caller's configuration. It then runs
# COMMAND with its output appended to LOG, marks the end with
# "[loop process exited]" and keeps the pane open for a day so the output stays
# inspectable.
set -euo pipefail

log=$1
shift
while IFS= read -r name; do
  if [[ $name == CLAUDE* ]]; then
    unset "$name"
  fi
done < <(compgen -e)
export PYTHONUNBUFFERED=1
umask 077

status=0
"$@" 2>&1 | tee -a -- "$log" || status=$?
printf '%s\n' '[loop process exited]' | tee -a -- "$log"
echo "(exit status $status; this pane closes in 24 hours, or with: tmux kill-session -t gitturtle-loop)"
exec sleep 86400
