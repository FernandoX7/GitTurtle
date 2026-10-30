#!/bin/sh
# Runs a vendored crate's own library tests, which workspace checks do not:
# every vendored crate is an excluded workspace member, and the workspace's
# lockfile does not resolve its dev-dependencies.
#
# The crate is copied, without its `target`, toolchain pin or `.cargo/`, to a
# fresh scratch directory under this checkout's `.local/`, so cargo and rustup
# read configuration and the toolchain pin only from directories the
# developer or the system owns, the checkout's own among them. The scratch
# directory stands for the crate's upstream repository, since upstream tests
# include files from beside their crate: the copy sits at `crates/<dir>`,
# every other patched crate whose `.cargo_vcs_info.json` names the same
# upstream revision sits, copied the same way, at the path in the repository
# that file gives (gpui-component's dock test reads gpui-base's
# `crates/base/src/dock/mod.rs`), the files of the crate's own
# `upstream-files/` sit at their repository paths, and the repository's
# README (gpui-base's text view test includes it) sits at the root:
# `upstream-files/README.md`, else the crate's. `upstream-files/` may hold
# only plainly named regular files and directories, no links, hidden names
# or toolchain pins, and nothing laid out may land in another's place or
# hold the copy.
#
# The copy is made a workspace root of its own whose `[patch.crates-io]`
# table points every other crate the root manifest patches at its directory
# under this checkout's `vendor/`, so the tests build against the same
# patched graph, with the same pinned toolchain, as the application does.
# The root manifest may patch only through single-line
# `name = { path = "..." }` entries of a `[patch.crates-io]` header written
# plainly, that resolve inside `vendor/`; any other patch or replace form,
# any spelling of that header with quotes, escapes or inner spaces, and any
# table header or key with an escape sequence, which could spell `patch`
# where this script cannot see it, stops the run. Each run copies the
# workspace's `Cargo.lock` in, resolves what that lock lacks (the crate's
# dev-dependencies), lists every package it resolved beyond the lock,
# refuses a registry or git copy of any package the workspace takes from a
# path (a member, a vendored patch or this crate), and runs
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
# Only single-line `name = { path = "..." }` entries under a plainly written
# `[patch.crates-io]` header are understood; anything else in the table, and
# any other patch or replace table or key in the manifest, stops the run
# rather than being left out of the copy's table and the registry check.
# Other headers and keys are compared without whitespace and quotes, so
# `["patch"."crates-io".x]` is a patch table too, and one with an escape
# sequence stops the run, since `"\u0070atch"` is `patch` to cargo.
patches=$(awk -v quote="'" '
  function bare(text) { gsub(/[[:space:]"]/, "", text); gsub(quote, "", text); return text }
  function trim(text) { sub(/^[[:space:]]+/, "", text); sub(/[[:space:]]+$/, "", text); return text }
  function refuse(what, line, why) { print "unsupported " what ": " line " (" why ")" > "/dev/stderr"; bad = 1 }
  /^[[:space:]]*\[/ {
    header = $0; sub(/#.*/, "", header); header = trim(header)
    in_table = (header == "[patch.crates-io]")
    if (in_table) next
    if (index(header, "\\")) refuse("table header", $0, "an escape sequence can hide which table it names")
    else if (bare(header) == "[patch.crates-io]") refuse("table header", $0, "write [patch.crates-io] plainly, without quotes or inner spaces")
    else if (bare(header) ~ /^\[+(patch|replace)([].]|$)/) refuse("table", $0, "only [patch.crates-io] is read")
    next
  }
  /^[[:space:]]*(#|$)/ { next }
  in_table && /^[A-Za-z0-9_-]+[[:space:]]*=[[:space:]]*\{[[:space:]]*path[[:space:]]*=[[:space:]]*"[^"\\]+"[[:space:]]*\}[[:space:]]*$/ {
    name = $0; sub(/[[:space:]]*=.*/, "", name)
    path = $0; sub(/^[^"]*"/, "", path); sub(/".*/, "", path)
    print name " " path
    next
  }
  in_table { refuse("[patch.crates-io] entry", $0, "only name = { path = \"...\" } without escapes is read"); next }
  /=/ {
    key = $0; sub(/=.*/, "", key)
    if (index(key, "\\")) refuse("key", $0, "an escape sequence can hide which key it is")
    else if (bare(key) ~ /^(patch|replace)(\.|$)/) refuse("key", $0, "only a [patch.crates-io] table is read")
  }
  END { exit bad }
' "$root/Cargo.toml") || fail "cannot read the patch table of $root/Cargo.toml"
[ -n "$patches" ] || fail "$root/Cargo.toml patches nothing"

# Every other patch as `name directory`, each directory inside `vendor/`.
others=''
found=''
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
  others="$others$name $path
"
done <<EOF
$patches
EOF
[ -n "$found" ] || fail "$root/Cargo.toml does not patch $crate"

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

# A field of a vendored crate's `.cargo_vcs_info.json`, or nothing.
vcs_field() { # vcs_field <crate directory> <field>
  [ -f "$1/.cargo_vcs_info.json" ] || return 0
  sed -n "s/.*\"$2\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" "$1/.cargo_vcs_info.json" | head -n 1
}
# A relative path whose every component is plainly named and not hidden.
plain_path='[A-Za-z0-9_-][A-Za-z0-9_.-]*(/[A-Za-z0-9_-][A-Za-z0-9_.-]*)*'

# The files of the upstream repository outside the crate that its tests
# include, at their repository paths.
files="$crate/upstream-files"
if [ -e "$files" ] || [ -L "$files" ]; then
  if [ -L "$files" ] || [ ! -d "$files" ]; then
    fail "$files is not a directory"
  fi
  odd=$(find "$files" -mindepth 1 \( \( ! -type f ! -type d \) -o -name '.*' -o -name '*[!A-Za-z0-9_.-]*' \
    -o -name rust-toolchain -o -name rust-toolchain.toml \) -print | head -n 1)
  [ -z "$odd" ] || fail "upstream-files may hold only plainly named regular files and directories, without links, hidden names or toolchain pins: $odd"
  listed=$(CDPATH='' cd -- "$files" && find . -type f | LC_ALL=C sort)
  while read -r file; do
    [ -n "$file" ] || continue
    file=${file#./}
    case "$file/" in "crates/$dir"/*) fail "upstream-files puts $file where the copy goes, at crates/$dir" ;; esac
    mkdir -p -- "$scratch/$(dirname -- "$file")"
    cp -- "$files/$file" "$scratch/$file"
  done <<EOF
$listed
EOF
fi

# The other vendored crates of the same upstream revision, at their paths
# in that repository.
revision=$(vcs_field "$crate" sha1)
case "$revision" in *[!0-9a-f]*) revision='' ;; esac
[ "${#revision}" -eq 40 ] || revision=''
if [ -n "$revision" ]; then
  while read -r name path; do
    [ -n "$name" ] || continue
    [ "$(vcs_field "$path" sha1)" = "$revision" ] || continue
    upstream=$(vcs_field "$path" path_in_vcs)
    if [ -z "$upstream" ]; then
      printf 'vendor-crate-test: %s names no path below its repository root; it is not laid out\n' "$path"
      continue
    fi
    printf '%s\n' "$upstream" | grep -Eqx -- "$plain_path" ||
      fail "$path gives the upstream path $upstream, which cannot be laid out inside the scratch directory"
    case "crates/$dir/" in "$upstream"/*) fail "$path's upstream path $upstream would hold the copy at crates/$dir" ;; esac
    case "$upstream/" in "crates/$dir"/*) fail "$path's upstream path $upstream lies inside the copy at crates/$dir" ;; esac
    [ ! -e "$scratch/$upstream" ] ||
      fail "cannot lay out $path at $upstream: upstream-files or another crate already put something there"
    mkdir -p -- "$scratch/$(dirname -- "$upstream")"
    cp -RP -- "$path" "$scratch/$upstream"
    rm -rf -- "$scratch/$upstream/target" "$scratch/$upstream/rust-toolchain" \
      "$scratch/$upstream/rust-toolchain.toml" "$scratch/$upstream/.cargo" "$scratch/$upstream/upstream-files"
  done <<EOF
$others
EOF
fi

mkdir -p -- "$scratch/crates"
[ ! -e "$copy" ] || fail "upstream-files or another crate already put something at crates/$dir, where the copy goes"
cp -RP -- "$crate" "$copy"
rm -rf -- "$copy/target" "$copy/rust-toolchain" "$copy/rust-toolchain.toml" "$copy/.cargo" "$copy/upstream-files"
if [ ! -e "$scratch/README.md" ]; then
  if [ -f "$crate/README.md" ]; then
    cp -- "$crate/README.md" "$scratch/README.md"
  else
    printf '# %s\n' "$dir" >"$scratch/README.md"
  fi
fi

# The copy is its own workspace root, so its patch table applies and cargo
# looks no further up for a workspace.
{
  printf '\n[workspace]\n\n[patch.crates-io]\n'
  while read -r name path; do
    [ -n "$name" ] || continue
    printf '%s = { path = "%s" }\n' "$name" "$path"
  done <<EOF
$others
EOF
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
