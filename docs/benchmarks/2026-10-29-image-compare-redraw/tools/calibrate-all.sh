#!/bin/bash
# calibrate-all.sh: AT-SPI calibration of every build x fixture x cell (raw/cal/). Run with the
# accessibility bus enabled (busctl ... IsEnabled b true) and set it back to false afterwards.
cd "$(dirname "$0")/.."; E=${GITTURTLE_RELEASES:-../../../.local/item8-2026-10-27}
for b in ${BUILDS:-6660805 81db2e3}; do for f in ${FIXTURES:-png add gif}; do for c in ${CELLS:-L13 L18 S18}; do
  echo "$(date -u +%FT%TZ) cal-$b-$f-$c builds: $(pgrep -c 'rustc|cargo')"
  timeout 300 tools/drive.py $E/gitturtle-$b $f $c cal-$b-$f-$c calibrate 2>&1 | tail -2
done; done; done; echo CAL-DONE
