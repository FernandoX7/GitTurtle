#!/usr/bin/env bash
# Run the configure/fit timing harness for both builds, interleaved, on one P-core.
# Usage: run.sh BASE_TEST_EXE CAND_TEST_EXE OUT.jsonl [CPU] [PROCS] [FIRST_REPEATS]
# Steady state: PROCS processes per build (2 * PROCS in B C C B ... order), each BENCH_WARMUP (200) untimed then BENCH_N
# (1000) timed calls per case. First call: FIRST_REPEATS fresh processes per build and case, one call each.
set -euo pipefail
base=$1 cand=$2 out=$3 cpu=${4:-2} procs=${5:-10} first=${6:-5}
test=appearance::omarchy::highlights_bench_cases::bench_highlights
cases=(Midnight Daylight Graphite TokyoNight CatppuccinMocha Nord Porcelain Sandstone DeepSea Ember SolarizedDark
       SolarizedLight OneDark OneLight RosePine RosePineDawn Dracula Alucard KanagawaWave KanagawaLotus
       "Omarchy tokyo-night" "Omarchy catppuccin-latte")
one() { # exe proc mode [case]
  BENCH_OUT=$out BENCH_PROC=$2 BENCH_MODE=$3 BENCH_CASE=${4:-} taskset -c "$cpu" "$1" --ignored --exact "$test" \
    --test-threads 1 -q >/dev/null
}
order=(B C C B)
for ((p = 0; p < 2 * procs; p++)); do
  who=${order[p % 4]}
  if [[ $who == B ]]; then one "$base" "s$p" steady; else one "$cand" "s$p" steady; fi
done
for ((r = 0; r < first; r++)); do
  for c in "${cases[@]}"; do
    one "$base" "f$r" first "$c"
    one "$cand" "f$r" first "$c"
  done
done
