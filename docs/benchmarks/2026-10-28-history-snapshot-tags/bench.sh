#!/bin/bash
# bench.sh DIR: three rounds alternating base and candidate on each fixture, pinned to core 8.
D=$1; R=$D/results; mkdir -p $R
for round in 1 2 3; do
  for fixture in history-tags history-tags-packed history-scroll/history; do
    for build in base candidate; do
      while pgrep -x 'cargo|rustc' >/dev/null || awk '{exit !($1 >= 1.5)}' /proc/loadavg; do sleep 5; done
      tag=$(basename $fixture)-$build-$round
      echo "$tag start $(cat /proc/loadavg) cpu8 $(cat /sys/devices/system/cpu/cpu8/cpufreq/scaling_cur_freq)kHz" >> $R/runs.txt
      GITTURTLE_SNAPSHOT_BENCH_REPO=/tmp/gitturtle-evidence/$fixture GITTURTLE_SNAPSHOT_BENCH_OUT=$R/$tag.json \
        taskset -c 8 $D/bench-$build --ignored --exact worker::snapshot_bench::read_snapshot_release_benchmark --test-threads 1 >/dev/null 2>$R/$tag.err
      echo "$tag end $(cat /proc/loadavg) cpu8 $(cat /sys/devices/system/cpu/cpu8/cpufreq/scaling_cur_freq)kHz exit $?" >> $R/runs.txt
    done
  done
done
