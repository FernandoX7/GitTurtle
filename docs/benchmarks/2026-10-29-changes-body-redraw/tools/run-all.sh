#!/bin/bash
# run-all.sh [ROUNDS]: ROUNDS rounds (default 6); each round runs every cell (six/many x 13/18 pt)
# with the base then the candidate, so each cell's launches alternate ABAB... Before every launch
# it logs date -u, /proc/loadavg and cores 4-11's idle share over 2 s to raw/runs.txt; drive.py logs
# the load at the launch's start and end. Then analyse.py over every launch.
cd "$(dirname "$0")/.."; E=${GITTURTLE_RELEASES:-../../../.local/item8-2026-10-27}
idle() { local a b; a=$(awk '/^cpu([4-9]|1[01]) /{i+=$5+$6; t+=$2+$3+$4+$5+$6+$7+$8+$9} END{print i, t}' /proc/stat); sleep 2
  b=$(awk '/^cpu([4-9]|1[01]) /{i+=$5+$6; t+=$2+$3+$4+$5+$6+$7+$8+$9} END{print i, t}' /proc/stat); set -- $a $b; echo "idle4-11 $(( 100 * ($3 - $1) / ($4 - $2) ))%"; }
labels=()
for r in $(seq 1 ${1:-6}); do for c in "six 13" "six 18" "many 13" "many 18"; do set -- $c
  for b in ea58f87 12b7043; do L=r$r-$b-$1-$2
    echo "$(date -u +%FT%TZ) $L start load $(cut -d' ' -f1-3 /proc/loadavg) $(idle) freq4-11 $(cat /sys/devices/system/cpu/cpu{4..11}/cpufreq/scaling_cur_freq | tr '\n' ' ')" >> raw/runs.txt
    WARM=1 WRITES=16 RESIZES=10 timeout 300 tools/drive.py $E/gitturtle-$b $1 $2 $L >> raw/runs.txt 2>&1 || echo "$L FAILED $?" >> raw/runs.txt
    echo "$(date -u +%FT%TZ) $L end load $(cut -d' ' -f1-3 /proc/loadavg)" >> raw/runs.txt; labels+=($L)
  done; done; done
python3 tools/analyse.py "${labels[@]}" >> raw/runs.txt 2>&1
echo ALL-DONE >> raw/runs.txt
