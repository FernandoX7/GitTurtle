#!/bin/bash
# make-fixtures.sh: pristine disposable fixtures under ../fixtures/pristine/ (copied fresh per launch
# by drive.py). Identity GitTurtle QA <qa@example.invalid>; no hooks, no remotes, no signing.
#  png:  10 commits of README.md, then photo.png (1600x1000 RGB) added, then HEAD changes photo.png
#        (both sides drawn).
#  add:  10 commits of README.md, then HEAD adds photo.png (1600x1000; Before absent).
#  gif:  10 commits of README.md, then anim.gif (640x400, 24 frames, 50 ms) added, then HEAD
#        changes it (24 frames, other content; both sides animated).
# Images come from tools/make-images.py (Pillow, deterministic).
set -e; T=$(cd "$(dirname "$0")" && pwd); R=$(dirname "$T")/fixtures/pristine; I=$(dirname "$T")/fixtures/images
rm -rf "$R" "$I"; mkdir -p "$R" "$I"; python3 -I "$T/make-images.py" "$I"
g() { git -c commit.gpgsign=false -c core.hooksPath=/dev/null "$@"; }
id() { git config user.name 'GitTurtle QA'; git config user.email qa@example.invalid; git config commit.gpgsign false; }
export GIT_AUTHOR_DATE='2026-10-01T12:00:00Z' GIT_COMMITTER_DATE='2026-10-01T12:00:00Z'
base() { git init -q -b main "$R/$1"; cd "$R/$1"; id; for i in $(seq 1 10); do echo "line $i" >> README.md; g add README.md; g commit -qm "Commit $i"; done; }
base png; cp "$I/before.png" photo.png; g add photo.png; g commit -qm 'Add photo'; cp "$I/after.png" photo.png; g add photo.png; g commit -qm 'Change photo'
base add; cp "$I/after.png" photo.png; g add photo.png; g commit -qm 'Add photo'
base gif; cp "$I/before.gif" anim.gif; g add anim.gif; g commit -qm 'Add animation'; cp "$I/after.gif" anim.gif; g add anim.gif; g commit -qm 'Change animation'
for f in png add gif; do cd "$R/$f"; echo "$f: HEAD $(git rev-parse --short HEAD) $(git log -1 --format=%s) | $(git show --stat --format= HEAD | head -1)"; done
