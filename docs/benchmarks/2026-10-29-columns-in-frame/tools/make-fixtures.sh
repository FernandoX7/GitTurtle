#!/bin/bash
# make-fixtures.sh: pristine disposable fixtures under ../fixtures/pristine/ (copied fresh per launch).
# Identity GitTurtle QA <qa@example.invalid>; no hooks, no remotes, no signing; fixed dates.
#  cols: the 3,000-commit History fixture of docs/benchmarks/2026-10-05 (make_history_fixture.py,
#        merges every 21 rows, 121 refs), then three commits on main:
#          "Add photo and long text"  photo.png (1600x1000, before) and long.txt (3,000 lines)
#          "Change photo"             photo.png changed (after)            -> image Compare (HEAD~1)
#          "Edit long text"           long.txt edited at 60 scattered lines -> text Compare (HEAD)
#  many: the Changes fixture of bench-changes-redraw-2026-10-29: 600 tracked files, all edited, none
#        staged (601 Changes rows).
set -e; T=$(cd "$(dirname "$0")" && pwd); R=$(dirname "$T")/fixtures/pristine; I=$(dirname "$T")/fixtures/images
rm -rf "$R" "$I"; mkdir -p "$R" "$I"; python3 -I "$T/make-images.py" "$I"
python3 -I "$T/make_history_fixture.py" "$R/cols" 3000 >/dev/null
g() { git -c commit.gpgsign=false -c core.hooksPath=/dev/null "$@"; }
id() { git config user.name 'GitTurtle QA'; git config user.email qa@example.invalid; git config commit.gpgsign false; }
cd "$R/cols"; id; git checkout -q main
export GIT_AUTHOR_DATE='2026-10-01T12:00:00Z' GIT_COMMITTER_DATE='2026-10-01T12:00:00Z'
cp "$I/before.png" photo.png
python3 -I -c "
for i in range(3000): print(f'{i:05d} the quick brown fox jumps over the lazy dog in column layout block {i // 50}')" > long.txt
g add photo.png long.txt; g commit -qm 'Add photo and long text'
cp "$I/after.png" photo.png; g add photo.png; g commit -qm 'Change photo'
python3 -I -c "
import sys
lines = open('long.txt').read().splitlines()
for k in range(60): j = 25 + k * 49; lines[j] = lines[j].replace('quick brown', 'slow grey') + ' (edited)'
open('long.txt', 'w').write('\n'.join(lines) + '\n')"
g add long.txt; g commit -qm 'Edit long text'
export GIT_AUTHOR_DATE='2026-10-01T12:00:00Z' GIT_COMMITTER_DATE='2026-10-01T12:00:00Z'
git init -q -b main "$R/many"; cd "$R/many"; id
for i in $(seq -w 0 599); do echo "item $i" > item-$i.txt; done; g add .; g commit -qm 'Add six hundred files'
for i in $(seq -w 0 599); do echo "edit $i" >> item-$i.txt; done
cd "$R/cols"; echo "cols: $(git rev-list --count HEAD) commits, $(git for-each-ref | wc -l) refs, HEAD $(git rev-parse --short HEAD) $(git log -1 --format=%s) | HEAD~1 $(git rev-parse --short HEAD~1) $(git log -1 --format=%s HEAD~1)"
cd "$R/many"; echo "many: HEAD $(git rev-parse --short HEAD), staged $(git diff --cached --name-only | wc -l), unstaged $(git diff --name-only | wc -l)"
