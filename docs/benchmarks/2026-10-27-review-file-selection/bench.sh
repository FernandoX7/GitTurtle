#!/bin/bash
# bench.sh [DIR]: three alternating rounds of the base and candidate selection benchmark, pinned
# to performance core 8, each after the one-minute load falls below 1.5 with no cargo or rustc.
# DIR holds the executables bench-base-30b8868 and bench-cand; samples go to DIR/results.
R=$(cd "${1:-.local/review-prep-2026-10-27}" && pwd); mkdir -p "$R/results"
T=github_view::review::tests::select_review_file_release_benchmark
for round in 1 2 3; do
  for build in base-30b8868 cand; do
    until [ "$(awk '{print ($1 < 1.5)}' /proc/loadavg)" = 1 ] && ! pgrep -x 'cargo|rustc' >/dev/null; do sleep 5; done
    start=$(date -u +%T); load=$(cut -d' ' -f1 /proc/loadavg); mhz=$(cat /sys/devices/system/cpu/cpu8/cpufreq/scaling_cur_freq)
    GITTURTLE_REVIEW_SELECT_BENCH_OUTPUT=$R/results/$build-$round.json taskset -c 8 "$R/bench-$build" --ignored --exact "$T" >/dev/null 2>&1 || echo "FAILED $build $round"
    echo "$round $build $start $(date -u +%T) load $load -> $(cut -d' ' -f1 /proc/loadavg) cpu8 ${mhz}kHz"
  done
done
