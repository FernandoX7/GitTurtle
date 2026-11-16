#!/bin/bash
# bench.sh DIR: three rounds alternating base and candidate on each fixture, pinned to CPUS
# (default core 8; for example CPUS=8-11 for the performance cluster). Results go to RESULTS
# (default DIR/results). A run starts when nothing compiles and cores 8-11 were at least 80% idle
# over two seconds (the host's editor keeps them 80-90% idle); the one-minute load average lags each
# run by minutes, so it is only recorded.
D=$1; R=${RESULTS:-$D/results}; C=${CPUS:-8}; mkdir -p $R
idle() { # percent idle of cores 8-11 over two seconds
  local a b; a=$(grep -E '^cpu(8|9|10|11) ' /proc/stat); sleep 2; b=$(grep -E '^cpu(8|9|10|11) ' /proc/stat)
  paste <(echo "$a") <(echo "$b") | awk '{t=0; for (i=2;i<=11;i++) t+=$(i+11)-$i; id=($(5+11)-$5)+($(6+11)-$6); T+=t; I+=id} END {printf "%d", T ? 100*I/T : 0}'; }
freq() { for f in /sys/devices/system/cpu/cpu{8..11}/cpufreq/scaling_cur_freq; do printf '%s ' $(cat $f); done; }
for round in 1 2 3; do
  for fixture in history-tags history-tags-packed history-scroll/history; do
    for build in base candidate; do
      while pgrep -x 'cargo|rustc' >/dev/null || [ "$(idle)" -lt 80 ]; do sleep 3; done
      tag=$(basename $fixture)-$build-$round
      echo "$tag start idle8-11 $(idle)% load $(cat /proc/loadavg) cpus $C kHz(8-11) $(freq)" >> $R/runs.txt
      GITTURTLE_SNAPSHOT_BENCH_REPO=/tmp/gitturtle-evidence/$fixture GITTURTLE_SNAPSHOT_BENCH_OUT=$R/$tag.json \
        taskset -c $C $D/bench-$build --ignored --exact worker::snapshot_bench::read_snapshot_release_benchmark --test-threads 1 >/dev/null 2>$R/$tag.err; rc=$?
      echo "$tag end $(cat /proc/loadavg) cpus $C kHz(8-11) $(freq) exit $rc" >> $R/runs.txt
    done
  done
done
