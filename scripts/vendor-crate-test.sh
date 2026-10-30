#!/bin/sh
# Runs a vendored crate's own library tests, which workspace checks do not:
# every vendored crate is an excluded workspace member, and the workspace's
# lockfile does not resolve its dev-dependencies.
#
# The crate is copied to a fresh scratch directory outside any Cargo
# workspace, as `crates/<dir>` beside a README (an upstream gpui-kit test
# includes its repository's README from there), and made a workspace root of
# its own whose `[patch.crates-io]` table points every other crate the root
# manifest patches at this checkout's `vendor/` copy, so the tests build
# against the same patched graph, with the same pinned toolchain, as the
# application does. Each run copies the workspace's `Cargo.lock` in, resolves
# only what that lock lacks (the crate's dev-dependencies), lists every
# package it resolved beyond the lock, refuses a patched crate taken from the
# registry, and runs `cargo test --locked --lib` in `.local/target-vendor`, a
# target directory of this checkout's own. It reaches the network only
# through cargo's own registry fetch.
#
# Usage: scripts/vendor-crate-test.sh <vendored crate dir> [test filter...]
# Filters and any further arguments go to the test harness.
set -eu

usage() {
  echo "Usage: scripts/vendor-crate-test.sh <vendored crate dir> [test filter...]" >&2
  exit 2
}
fail() {
  echo "vendor-crate-test: $*" >&2
  exit 1
}

[ "$#" -ge 1 ] || usage
case "$1" in -*) usage ;; esac

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
crate=$(CDPATH='' cd -- "$1" && pwd -P) || fail "no directory $1"
shift
case "$root" in *'"'* | *'\'*) fail "checkout path $root cannot be written as a TOML string" ;; esac
case "$crate" in
  "$root"/vendor/*/*) fail "$crate is not a vendored crate directory" ;;
  "$root"/vendor/*) ;;
  *) fail "$crate is not under $root/vendor" ;;
esac
[ -f "$crate/Cargo.toml" ] || fail "$crate has no Cargo.toml"
dir=$(basename -- "$crate")

# The root manifest's `[patch.crates-io]` entries, one `name path` per line.
# Only single-line `name = { path = "..." }` entries are understood; anything
# else in the table stops the run rather than being left out.
patches=$(awk '
  /^\[/ { in_table = ($0 == "[patch.crates-io]"); next }
  !in_table || /^[[:space:]]*(#|$)/ { next }
  /^[A-Za-z0-9_-]+[[:space:]]*=[[:space:]]*\{[[:space:]]*path[[:space:]]*=[[:space:]]*"[^"]+"[[:space:]]*\}[[:space:]]*$/ {
    name = $0; sub(/[[:space:]]*=.*/, "", name)
    path = $0; sub(/^[^"]*"/, "", path); sub(/".*/, "", path)
    print name " " path
    next
  }
  { print "unsupported [patch.crates-io] entry: " $0 > "/dev/stderr"; bad = 1 }
  END { exit bad }
' "$root/Cargo.toml") || fail "cannot read the patch table of $root/Cargo.toml"
[ -n "$patches" ] || fail "$root/Cargo.toml patches nothing"

scratch=$(mktemp -d "${TMPDIR:-/tmp}/gitturtle-vendor-test.XXXXXX")
trap 'rm -rf -- "$scratch"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
copy="$scratch/crates/$dir"
mkdir -- "$scratch/crates"
cp -RP -- "$crate" "$copy"
rm -rf -- "$copy/target"
if [ -f "$crate/README.md" ]; then
  cp -- "$crate/README.md" "$scratch/README.md"
else
  printf '# %s\n' "$dir" >"$scratch/README.md"
fi
# rustup finds the checkout's toolchain pin only above the working directory.
if [ -f "$root/rust-toolchain.toml" ]; then
  cp -- "$root/rust-toolchain.toml" "$scratch/rust-toolchain.toml"
fi

# The copy is its own workspace root, so its patch table applies and cargo
# looks no further up for a workspace.
patched=''
found=''
{
  printf '\n[workspace]\n\n[patch.crates-io]\n'
  while read -r name path; do
    case "$path" in /*) ;; *) path="$root/$path" ;; esac
    path=$(CDPATH='' cd -- "$path" && pwd -P) || fail "patch $name names a missing directory"
    if [ "$path" = "$crate" ]; then
      found=$name
      continue
    fi
    [ -f "$path/Cargo.toml" ] || fail "patch $name at $path has no Cargo.toml"
    case "$path" in *'"'* | *'\'*) fail "patch path $path cannot be written as a TOML string" ;; esac
    patched="$patched $name"
    printf '%s = { path = "%s" }\n' "$name" "$path"
  done <<EOF
$patches
EOF
  [ -n "$found" ] || fail "$root/Cargo.toml does not patch $crate"
} >>"$copy/Cargo.toml"
cp -- "$root/Cargo.lock" "$copy/Cargo.lock"

export CARGO_TARGET_DIR="$root/.local/target-vendor"
mkdir -p -- "$CARGO_TARGET_DIR"
cd -- "$copy"
echo "vendor-crate-test: $(rustc --version) builds $dir in $CARGO_TARGET_DIR"

# Resolves what the copied lock lacks and keeps every package it has. Quiet:
# it would list each workspace package this crate does not use as removed;
# the lists below say what changed.
cargo update --workspace --quiet

# `name version source` of every entry under one lockfile header.
entries() {
  awk -v header="$1" '
    /^\[/ { if (name != "") print name " " version " " source; name = ""; version = ""; source = ""; in_entry = ($0 == header); next }
    in_entry && /^name = / { name = $3; gsub(/"/, "", name) }
    in_entry && /^version = / { version = $3; gsub(/"/, "", version) }
    in_entry && /^source = / { source = $3; gsub(/"/, "", source) }
    END { if (name != "") print name " " version " " source }
  ' "$2" | LC_ALL=C sort -u
}
entries '[[package]]' "$root/Cargo.lock" >"$scratch/workspace.packages"
entries '[[package]]' "$copy/Cargo.lock" >"$scratch/resolved.packages"
LC_ALL=C comm -13 "$scratch/workspace.packages" "$scratch/resolved.packages" >"$scratch/added.packages"
if [ -s "$scratch/added.packages" ]; then
  echo "vendor-crate-test: resolved beyond the workspace lock:"
  sed 's/^/  /' "$scratch/added.packages"
else
  echo "vendor-crate-test: resolved nothing beyond the workspace lock"
fi
entries '[[patch.unused]]' "$copy/Cargo.lock" >"$scratch/unused.patches"
if [ -s "$scratch/unused.patches" ]; then
  echo "vendor-crate-test: patches outside this crate's graph, which cargo reports as not used:"
  sed 's/ *$//; s/^/  /' "$scratch/unused.patches"
fi
while read -r name version source; do
  [ -n "$source" ] || continue
  for patch in $patched; do
    [ "$name" != "$patch" ] || fail "$name $version came from $source rather than its vendored patch"
  done
done <"$scratch/added.packages"

cargo test --locked --lib -- "$@"
