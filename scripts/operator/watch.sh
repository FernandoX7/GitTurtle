#!/usr/bin/env bash
# Follow a console log and print only the lines a coordinator acts on, one per event:
#   - step lines ("12:00:00Z building TASK attempt 1"; gating, pre_verifying, security_reviewing,
#     evidence_gating, verifying, accepting and any later step, such as rebasing, share the shape),
#   - gate failures ("  clippy gate failed in 30s"),
#   - task outcomes (accepted, failed, review_blocked, blocked, stale, interrupted,
#     awaiting_evidence), inbox outcomes ("inbox native for TASK: applied" or "refused: ..."),
#     idle waits and their end when the controller prints them ("waiting for evidence on ...",
#     "wait ended after ..."), the final "paused|complete|blocked|baseline_failed: reason"
#     line, controller errors and tracebacks,
#   - "[loop process exited]", after which it exits 0,
#   - one "ALERT" line, then exit 1, if the tmux session vanishes without that marker.
# Task statuses a resume reprints before its first step are not news and stay hidden.
#
# Usage: scripts/operator/watch.sh [LOG]   (default: the newest console log by mtime)
set -euo pipefail
# shellcheck source=scripts/operator/lib.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

log=${1:-$(newest_console_log)}
[[ -n $log && -f $log ]] || die "no console log${1:+ $1}"

stamp='^[0-9]{2}:[0-9]{2}:[0-9]{2}Z '
step_re="${stamp}[a-z_]+ [^ ]+ attempt [0-9]+\$"
gate_re="${stamp} +[^ ]+ gate failed"
status_re="${stamp}[^ ]+: (accepted|failed|review_blocked|blocked|stale|interrupted|awaiting_evidence)( |\$)"
event_re="${stamp}(inbox [^:]+: |waiting for evidence |wait ended )"
summary_re='^(paused|complete|blocked|baseline_failed|idle): '
error_re='^(agent-loop: |Traceback \(most recent call last\))'

# Every line after the first `seen` is read through tail, so a line written while these
# checks run, the end marker included, still arrives.
seen=$(( $(wc -l < "$log") ))

# A log that already shows a step is past the resume's reprint of every task status.
seen_step=0
if head -n "$seen" -- "$log" | grep -Eq "$step_re"; then
  seen_step=1
fi

# On a quiet interval: stop at a finished log (the pane outlives the loop by a day),
# raise one ALERT if the session vanished without finishing it.
alert_if_gone() {
  if log_finished "$log"; then
    printf '%s\n' "$MARKER"
    exit 0
  fi
  session_exists && return 0
  printf 'ALERT: tmux session %s is gone and %s does not end in %s\n' "$SESSION" "$log" "$MARKER"
  exit 1
}

if (( seen > 0 )) && [[ $(sed -n "${seen}p" -- "$log") == "$MARKER" ]]; then
  printf '%s\n' "$MARKER"
  exit 0
fi

handle() {
  local line=$1
  if [[ $line == "$MARKER" ]]; then
    printf '%s\n' "$line"
    exit 0
  elif [[ $line =~ $step_re ]]; then
    seen_step=1
    printf '%s\n' "$line"
  elif [[ $line =~ $status_re ]]; then
    if (( seen_step )); then
      printf '%s\n' "$line"
    fi
  elif [[ $line =~ $gate_re || $line =~ $event_re || $line =~ $summary_re || $line =~ $error_re ]]; then
    printf '%s\n' "$line"
  fi
}

exec 3< <(exec tail -n "+$(( seen + 1 ))" -F -- "$log" 2>/dev/null)
tail_pid=$!
trap 'kill "$tail_pid" 2>/dev/null || true' EXIT

partial=""
while :; do
  chunk=""
  if IFS= read -r -t 30 chunk <&3; then
    handle "$partial$chunk"
    partial=""
  else
    status=$?
    if (( status > 128 )); then  # timed out: keep any partial line, then check the log and session
      partial+=$chunk
      alert_if_gone
      continue
    fi
    break
  fi
done
alert_if_gone
