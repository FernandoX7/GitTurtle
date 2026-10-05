#!/usr/bin/env bash
# Build the configure/fit timing harness for one build: export REV from a clone into SRC_DIR,
# append the harness module (common.rs.in + VARIANT.rs.in) to crates/app/src/appearance.rs and
# the cases (cases.rs.in) to crates/app/src/appearance/omarchy.rs, and build the gitturtle bin
# tests in release in TARGET_DIR. Prints the test executable's path.
# Usage: build.sh CLONE REV VARIANT(base|cand) SRC_DIR TARGET_DIR
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
clone=$1 rev=$2 variant=$3 src=$4 target=$5
[[ ! -e $src ]] || { echo "$src exists" >&2; exit 1; }
mkdir -p -- "$src"
git -C "$clone" archive "$rev" | tar -x -C "$src"
cat "$here/common.rs.in" "$here/$variant.rs.in" >>"$src/crates/app/src/appearance.rs"
cat "$here/cases.rs.in" >>"$src/crates/app/src/appearance/omarchy.rs"
cd -- "$src"
CARGO_TARGET_DIR=$target cargo test --release --locked -p gitturtle --bins --no-run --message-format=json 2>"$target.build.log" \
  | python3 -c 'import json,sys
for l in sys.stdin:
    try: m=json.loads(l)
    except ValueError: continue
    if m.get("reason")=="compiler-artifact" and m.get("executable") and m["target"]["name"]=="gitturtle" and m["profile"]["test"]: print(m["executable"])'
