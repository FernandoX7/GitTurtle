#!/bin/bash
set -u
R=$(git rev-parse --show-toplevel); M=$R/.local/find-frame-2026-11-03; cd $R
touch $R/.local/build-slots/quiet
echo "quiet set $(date +%T)"
while fuser $R/.local/build-slots/1 $R/.local/build-slots/2 $R/.local/build-slots/3 >/dev/null 2>&1; do sleep 15; done
echo "slots drained $(date +%T); load $(cut -d' ' -f1-3 /proc/loadavg)"
for r in 1 2 3; do for b in base candidate; do
  g=$(mktemp); echo "run $b-$r start $(date +%T) load $(cut -d' ' -f1-3 /proc/loadavg)"
  env -u GIT_ASKPASS -u SSH_ASKPASS -u SSH_ASKPASS_REQUIRE -u GIT_CONFIG_PARAMETERS -u GIT_CONFIG_COUNT GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="$g" NO_AT_BRIDGE=1 \
    GITTURTLE_FIND_FRAME_BENCH_OUTPUT="$M/$b-$r.json" GITTURTLE_FIND_FRAME_BENCH_SAMPLES=200 \
    taskset -c 8 $M/bench-$b find_bench::find_bar_frame_release_benchmark --exact --ignored --test-threads=1 > $M/$b-$r.log 2>&1
  echo "run $b-$r exit $? end $(date +%T)"; rm -f "$g"
done; done
rm -f $R/.local/build-slots/quiet; echo "quiet cleared $(date +%T)"
