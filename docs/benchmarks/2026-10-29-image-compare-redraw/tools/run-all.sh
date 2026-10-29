#!/bin/bash
# run-all.sh FIRST LAST: rounds FIRST..LAST; each round runs every cell (L13, L18, S18) x fixture
# (png, add, gif) with the base then the candidate, so the launches alternate ABAB... Before every
# launch it logs date -u, /proc/loadavg, running rustc/cargo processes, cores 4-11's idle share over
# 2 s and their frequencies to raw/runs.txt; drive.py logs the load at the launch's start and end.
# A launch whose label already has marks is skipped (resumable). analyse.py runs separately.
cd "$(dirname "$0")/.."; E=${GITTURTLE_RELEASES:-../../../.local/item8-2026-10-27}
idle() { local a b; a=$(awk '/^cpu([4-9]|1[01]) /{i+=$5+$6; t+=$2+$3+$4+$5+$6+$7+$8+$9} END{print i, t}' /proc/stat); sleep 2
  b=$(awk '/^cpu([4-9]|1[01]) /{i+=$5+$6; t+=$2+$3+$4+$5+$6+$7+$8+$9} END{print i, t}' /proc/stat); set -- $a $b; echo "idle4-11 $(( 100 * ($3 - $1) / ($4 - $2) ))%"; }
for r in $(seq $1 $2); do for c in L13 L18 S18; do for f in png add gif; do
  for b in 6660805 81db2e3; do L=r$r-$b-$f-$c
    [ -f raw/$L.marks.json ] && continue
    echo "$(date -u +%FT%TZ) $L start load $(cut -d' ' -f1-3 /proc/loadavg) builds $(pgrep -c 'rustc|cargo') $(idle) freq4-11 $(cat /sys/devices/system/cpu/cpu{4..11}/cpufreq/scaling_cur_freq | tr '\n' ' ')" >> raw/runs.txt
    timeout 300 tools/drive.py $E/gitturtle-$b $f $c $L >> raw/runs.txt 2>&1 || echo "$L FAILED $?" >> raw/runs.txt
    echo "$(date -u +%FT%TZ) $L end load $(cut -d' ' -f1-3 /proc/loadavg)" >> raw/runs.txt
  done; done; done; done
echo "ROUNDS $1-$2 DONE $(date -u +%FT%TZ)" >> raw/runs.txt
