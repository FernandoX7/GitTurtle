#!/usr/bin/env bash
# Release-build one source tree in its own fresh target directory, copy the
# executable to OUT_EXE, print its --build-info and sha256, and delete the target.
# Retries once, on the same target, if rustc dies with SIGSEGV. The build log is
# kept at OUT_EXE.build.log.
#
# Usage: scripts/operator/build-release.sh SRC_DIR OUT_EXE
#   e.g. build-release.sh .local/evidence/TASK/src-cand-abc1234 .local/evidence/TASK/gitturtle-cand-abc1234
# A separate target per build keeps two commits from sharing one binary; build one
# at a time, since each target takes several gigabytes.
set -euo pipefail
# shellcheck source=scripts/operator/lib.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

(( $# == 2 )) || die "usage: build-release.sh SRC_DIR OUT_EXE"
[[ -f $1/Cargo.toml ]] || die "$1 is not a GitTurtle source tree"
src=$(cd -- "$1" && pwd -P)
out=$2
[[ $out == /* ]] || out=$PWD/$out
[[ ! -e $out ]] || die "$out exists; give each build its own name"
out_dir=$(dirname -- "$out")
mkdir -p -- "$out_dir"
out_dir=$(cd -- "$out_dir" && pwd -P)
out=$out_dir/$(basename -- "$out")
log=$out.build.log
target=$(mktemp -d "$out_dir/.target-$(basename -- "$out").XXXXXX")

remove_target() {
  python3 -c 'import shutil, sys; shutil.rmtree(sys.argv[1], ignore_errors=True)' "$target"
}
trap remove_target EXIT

echo "source $(git -C "$src" rev-parse HEAD)$( [[ -z $(git -C "$src" status --porcelain) ]] || echo ' (dirty)')"
df -h -- "$out_dir" | tail -n 1
for attempt in 1 2; do
  status=0
  printf '== attempt %s, %s\n' "$attempt" "$(date -u +%FT%TZ)" >>"$log"
  (cd -- "$src" && CARGO_TARGET_DIR=$target cargo build --release --locked -p gitturtle) >>"$log" 2>&1 || status=$?
  if (( status == 0 )); then
    break
  fi
  if (( attempt == 1 )) && grep -q 'SIGSEGV' -- "$log"; then
    echo "rustc died with SIGSEGV; retrying once" >&2
    continue
  fi
  tail -n 20 -- "$log" >&2
  die "the build failed with status $status; log: $log"
done

cp -- "$target/release/gitturtle" "$out"
"$out" --build-info
if command -v sha256sum >/dev/null; then
  sha256sum -- "$out"
else
  shasum -a 256 -- "$out"
fi
remove_target
trap - EXIT
echo "deleted $target"
