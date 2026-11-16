#!/bin/bash
# run-all.sh FIRST LAST: rounds FIRST..LAST of the width steps only (rerun 2026-10-30). Each round runs
# the cells L13 L18 S13 S18 (drive.py ... steps) with both builds: base first in odd rounds, candidate
# first in even rounds. Before every launch it waits until no rustc or cargo runs, then logs date -u,
# /proc/loadavg, the count of rustc/cargo processes, the AT-SPI IsEnabled state, cores 4-11's idle
# share over 2 s and their frequencies to raw/runs.txt. A launch whose marks exist is skipped.
# No Changes cells.
cd "$(dirname "$0")/.."; E=${GITTURTLE_RELEASES:-../../../../.local/item8-2026-10-27}; A=5e12885; Z=aeab555
idle() { local a b; a=$(awk '/^cpu([4-9]|1[01]) /{i+=$5+$6; t+=$2+$3+$4+$5+$6+$7+$8+$9} END{print i, t}' /proc/stat); sleep 2
  b=$(awk '/^cpu([4-9]|1[01]) /{i+=$5+$6; t+=$2+$3+$4+$5+$6+$7+$8+$9} END{print i, t}' /proc/stat); set -- $a $b; echo "idle4-11 $(( 100 * ($3 - $1) / ($4 - $2) ))%"; }
pre() { while pgrep -x rustc >/dev/null || pgrep cargo >/dev/null; do sleep 5; done
  echo "$(date -u +%FT%TZ) $1 start load $(cut -d' ' -f1-3 /proc/loadavg) builds $(pgrep -c 'rustc|cargo') atspi $(busctl --user get-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled) $(idle) freq4-11 $(cat /sys/devices/system/cpu/cpu{4..11}/cpufreq/scaling_cur_freq | tr '\n' ' ')" >> raw/runs.txt; }
for r in $(seq $1 $2); do
  if [ $((r % 2)) = 1 ]; then order="$A $Z"; else order="$Z $A"; fi
  for c in L13 L18 S13 S18; do for b in $order; do L=r$r-$b-$c
    [ -f raw/$L.marks.json ] && continue
    pre $L; timeout 300 tools/drive.py $E/gitturtle-$b $c $L steps >> raw/runs.txt 2>&1 || echo "$L FAILED $?" >> raw/runs.txt
    echo "$(date -u +%FT%TZ) $L end load $(cut -d' ' -f1-3 /proc/loadavg)" >> raw/runs.txt
  done; done
done
echo "ROUNDS $1-$2 DONE $(date -u +%FT%TZ)" >> raw/runs.txt
