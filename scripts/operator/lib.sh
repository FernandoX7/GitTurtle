# Shared by the operator scripts; sourced, not run.
# Paths are absolute and derived from this checkout, so the scripts work from any directory.
# shellcheck shell=bash

OPERATOR_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd -- "$OPERATOR_DIR/../.." && pwd -P)
LOG_DIR="$ROOT/.local/agent-loop"
SESSION=gitturtle-loop
MARKER='[loop process exited]'

die() {
  printf '%s: %s\n' "$(basename -- "$0")" "$*" >&2
  exit 1
}

# The newest console log by modification time (plain `ls -t` may be an alias here).
newest_console_log() {
  local newest="" file
  for file in "$LOG_DIR"/console-*.log; do
    [[ -f $file ]] || continue
    if [[ -z $newest || $file -nt $newest ]]; then
      newest=$file
    fi
  done
  printf '%s' "$newest"
}

session_exists() {
  tmux has-session -t "=$SESSION" 2>/dev/null
}

# The log the running session writes, recorded when it started; else the newest console log.
session_log() {
  local log
  log=$(tmux show-options -v -t "=$SESSION" @gitturtle_log 2>/dev/null || true)
  [[ -n $log ]] || log=$(newest_console_log)
  printf '%s' "$log"
}

log_finished() {
  [[ -f $1 && $(tail -n 1 -- "$1") == "$MARKER" ]]
}

# none: no session; finished: the loop exited and the pane only keeps its output; alive: a loop runs.
session_state() {
  if ! session_exists; then
    echo none
  elif log_finished "$(session_log)"; then
    echo finished
  else
    echo alive
  fi
}

# Kill a finished session so a new one can take its name; refuse while a loop is alive.
replace_finished_session() {
  case $(session_state) in
    none) ;;
    finished)
      tmux kill-session -t "=$SESSION"
      echo "replaced the finished tmux session $SESSION" >&2
      ;;
    *)
      die "a loop is still running in tmux session $SESSION ($(session_log)); stop it with agent-loop.py stop, or attach with: tmux attach -t $SESSION"
      ;;
  esac
}

new_console_log() {
  local log
  mkdir -p -- "$LOG_DIR"
  log="$LOG_DIR/console-$(date -u +%Y%m%dT%H%M%SZ).log"
  [[ ! -e $log ]] || die "$log already exists; try again in a second"
  printf '%s' "$log"
}

# launch LOG COMMAND...: run COMMAND in a new detached session that appends its output to LOG.
launch() {
  local log=$1
  shift
  tmux new-session -d -s "$SESSION" -c "$ROOT" -- bash "$OPERATOR_DIR/loop-session.sh" "$log" "$@"
  tmux set-option -t "=$SESSION" @gitturtle_log "$log" >/dev/null 2>&1 || true
  printf '%s\n' "$log"
}
