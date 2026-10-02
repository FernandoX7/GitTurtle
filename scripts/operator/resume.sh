#!/usr/bin/env bash
# Resume a saved run in tmux session gitturtle-loop, without any CLAUDE* variable,
# appending to a new .local/agent-loop/console-<UTC>.log.
#
# Usage: scripts/operator/resume.sh RUN [resume options...]
#   RUN is a run directory, or its name under .local/agent-loop.
#   Resume options (--max-minutes, --max-tasks, ...) pass through to `agent-loop.py resume`.
#
# Replaces a finished session (its log ends in "[loop process exited]") and refuses
# while a loop is alive. Prints the console log path.
set -euo pipefail
# shellcheck source=scripts/operator/lib.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

(( $# >= 1 )) || die "usage: resume.sh RUN [resume options...]"
run=$1
shift
if [[ ! -d $run && $run != */* && -d $LOG_DIR/$run ]]; then
  run=$LOG_DIR/$run
fi
[[ -d $run && -f $run/state.json ]] || die "$run is not a run directory"
run=$(cd -- "$run" && pwd -P)

replace_finished_session
log=$(new_console_log)
launch "$log" python3 "$ROOT/scripts/agent-loop.py" resume --run "$run" "$@"
