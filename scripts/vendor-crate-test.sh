#!/bin/sh
# Runs a vendored crate's own library tests, which workspace checks do not:
# every vendored crate is an excluded workspace member, and the workspace's
# lockfile does not resolve its dev-dependencies.
#
# The crate is copied, without its `target`, toolchain pin or `.cargo/`, to a
# fresh scratch directory under this checkout's `.local/`, so cargo and rustup
# read configuration and the toolchain pin only from directories the
# developer or the system owns, the checkout's own among them. The copy sits
# at `crates/<dir>` beside a README (an upstream gpui-kit test includes its
# repository's README from there) and is made a workspace root of its own
# whose `[patch.crates-io]` table points every other crate the root manifest
# patches at its directory under this checkout's `vendor/`, so the tests
# build against the same patched graph, with the same pinned toolchain, as
# the application does. The root manifest may patch only through single-line
# `name = { path = "..." }` entries of `[patch.crates-io]` that resolve inside
# `vendor/`; any other patch or replace form stops the run. Each run copies
# the workspace's `Cargo.lock` in, resolves what that lock lacks (the crate's
# dev-dependencies), lists every package it resolved beyond the lock, refuses
# a registry or git copy of any package the workspace takes from a path (a
# member, a vendored patch or this crate), and runs
# `cargo test --locked --lib` in `.local/target-vendor`, a target directory of
# this checkout's own. It reaches the network only through cargo's own
# registry fetch.
#
# Usage: scripts/vendor-crate-test.sh <vendored crate dir> [test filter...]
# Filters and any further arguments go to the test harness.
set -eu

usage() {
  printf '%s\n' "Usage: scripts/vendor-crate-test.sh <vendored crate dir> [test filter...]" >&2
  exit 2
}
fail() {
  printf 'vendor-crate-test: %s\n' "$*" >&2
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
# The copy's manifest is appended to and its lock replaced; through a link
# either would write to the file the link names.
for file in Cargo.toml Cargo.lock; do
  [ ! -L "$crate/$file" ] || fail "$crate/$file is a symbolic link"
done
[ -f "$crate/Cargo.toml" ] || fail "$crate has no Cargo.toml"
dir=$(basename -- "$crate")

# The root manifest's `[patch.crates-io]` entries, one `name path` per line.
# Only single-line `name = { path = "..." }` entries are understood; anything
# else in the table, and any other patch or replace table or key in the
# manifest, stops the run rather than being left out of the copy's table and
# the registry check. Headers and keys are compared without whitespace and
# quotes, so `["patch"."crates-io"]` is the same table.
patches=$(awk -v quote="'" '
  function bare(text) { gsub(/[[:space:]"]/, "", text); gsub(quote, "", text); return text }
  /^[[:space:]]*\[/ {
    header = $0; sub(/#.*/, "", header); header = bare(header)
    in_table = (header == "[patch.crates-io]")
    if (!in_table && header ~ /^\[+(patch|replace)([].]|$)/) {
      print "unsupported table: " $0 > "/dev/stderr"; bad = 1
    }
    next
  }
  /^[[:space:]]*(#|$)/ { next }
  in_table && /^[A-Za-z0-9_-]+[[:space:]]*=[[:space:]]*\{[[:space:]]*path[[:space:]]*=[[:space:]]*"[^"]+"[[:space:]]*\}[[:space:]]*$/ {
    name = $0; sub(/[[:space:]]*=.*/, "", name)
    path = $0; sub(/^[^"]*"/, "", path); sub(/".*/, "", path)
    print name " " path
    next
  }
  in_table { print "unsupported [patch.crates-io] entry: " $0 > "/dev/stderr"; bad = 1; next }
  /=/ {
    key = $0; sub(/=.*/, "", key); key = bare(key)
    if (key ~ /^(patch|replace)(\.|$)/) { print "unsupported key: " $0 > "/dev/stderr"; bad = 1 }
  }
  END { exit bad }
' "$root/Cargo.toml") || fail "cannot read the patch table of $root/Cargo.toml"
[ -n "$patches" ] || fail "$root/Cargo.toml patches nothing"

# Cargo merges `.cargo/config.toml` from every directory above the one it runs
# in, and rustup takes the nearest toolchain pin above it, so the scratch
# directory lives in the checkout rather than a shared temporary directory.
scratch=''
trap '[ -z "$scratch" ] || rm -rf -- "$scratch"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p -- "$root/.local"
scratch=$(mktemp -d "$root/.local/vendor-test.XXXXXX")
case "$(CDPATH='' cd -- "$scratch" && pwd -P)" in
  "$root"/?*) ;;
  *) fail "$root/.local leads outside the checkout; cargo would read configuration from the directories above it" ;;
esac
copy="$scratch/crates/$dir"
mkdir -- "$scratch/crates"
cp -RP -- "$crate" "$copy"
rm -rf -- "$copy/target" "$copy/rust-toolchain" "$copy/rust-toolchain.toml" "$copy/.cargo"
if [ -f "$crate/README.md" ]; then
  cp -- "$crate/README.md" "$scratch/README.md"
else
  printf '# %s\n' "$dir" >"$scratch/README.md"
fi

# The copy is its own workspace root, so its patch table applies and cargo
# looks no further up for a workspace.
found=''
{
  printf '\n[workspace]\n\n[patch.crates-io]\n'
  while read -r name path; do
    case "$path" in /*) ;; *) path="$root/$path" ;; esac
    path=$(CDPATH='' cd -- "$path" && pwd -P) || fail "patch $name names a missing directory"
    case "$path" in
      "$root"/vendor/?*) ;;
      *) fail "patch $name resolves to $path, outside $root/vendor" ;;
    esac
    if [ "$path" = "$crate" ]; then
      found=$name
      continue
    fi
    [ -f "$path/Cargo.toml" ] || fail "patch $name at $path has no Cargo.toml"
    case "$path" in *'"'* | *'\'*) fail "patch path $path cannot be written as a TOML string" ;; esac
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
printf 'vendor-crate-test: %s builds %s in %s\n' "$(rustc --version)" "$dir" "$CARGO_TARGET_DIR"

# Resolves what the copied lock lacks. Cargo keeps a locked version only while
# it still satisfies the copy's requirements, so this does not hold the lock
# fixed: the list below names every package it resolved beyond the lock.
# Quiet: it would list each workspace package this crate does not use as
# removed.
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
  printf '%s\n' "vendor-crate-test: resolved beyond the workspace lock:"
  sed 's/^/  /' "$scratch/added.packages"
else
  printf '%s\n' "vendor-crate-test: resolved nothing beyond the workspace lock"
fi
entries '[[patch.unused]]' "$copy/Cargo.lock" >"$scratch/unused.patches"
if [ -s "$scratch/unused.patches" ]; then
  printf '%s\n' "vendor-crate-test: patches outside this crate's graph, which cargo reports as not used:"
  sed 's/ *$//; s/^/  /' "$scratch/unused.patches"
fi
# Every package the workspace takes from a path (its members, the vendored
# patches and this crate, which its own copy does not patch) must come from a
# path here too, or the tests would run code the application does not.
{
  awk '$3 == "" { print $1 }' "$scratch/workspace.packages"
  printf '%s\n' "$patches" | awk '{ print $1 }'
} >"$scratch/path.names"
while read -r name version source; do
  [ -n "$source" ] || continue
  if grep -qxF -- "$name" "$scratch/path.names"; then
    fail "$name $version came from $source, but the workspace takes $name from a path"
  fi
done <"$scratch/added.packages"

cargo test --locked --lib -- ${1+"$@"}
