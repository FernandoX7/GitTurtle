#!/bin/bash
# make-fixtures.sh: pristine disposable fixtures under ../fixtures/pristine/ (restored per launch
# by drive.py). Identity GitTurtle QA <qa@example.invalid>; no hooks, no remotes, no signing.
#  six:  40 commits of README.md, then six new files alpha..zeta.txt, none staged (the files of
#        motion::tests::six_files without its two staged; 7 Changes rows). Toggled: alpha.txt.
#  many: 600 tracked files item-000..599.txt in one commit, all 600 edited, none staged
#        (601 Changes rows). Toggled: item-000.txt.
# Nothing is staged so that the toggled file's Stage and Unstage controls share one point at
# 13 and 18 pt: first row under the Unstaged heading, or only row under the Staged heading.
set -e; R=$(cd "$(dirname "$0")/.." && pwd)/fixtures/pristine; rm -rf "$R"; mkdir -p "$R"
g() { git -c commit.gpgsign=false -c core.hooksPath=/dev/null "$@"; }
id() { git config user.name 'GitTurtle QA'; git config user.email qa@example.invalid; git config commit.gpgsign false; }
export GIT_AUTHOR_DATE='2026-10-01T12:00:00Z' GIT_COMMITTER_DATE='2026-10-01T12:00:00Z'
git init -q -b main "$R/six"; cd "$R/six"; id
for i in $(seq 1 40); do echo "line $i" >> README.md; g add README.md; g commit -qm "Commit $i"; done
for n in alpha beta gamma delta epsilon zeta; do echo "$n" > $n.txt; done
git init -q -b main "$R/many"; cd "$R/many"; id
for i in $(seq -w 0 599); do echo "item $i" > item-$i.txt; done; g add .; g commit -qm 'Add six hundred files'
for i in $(seq -w 0 599); do echo "edit $i" >> item-$i.txt; done
for f in six many; do cd "$R/$f"; echo "$f: HEAD $(git rev-parse --short HEAD), staged $(git diff --cached --name-only | wc -l), unstaged $(git diff --name-only | wc -l), untracked $(git ls-files -o --exclude-standard | wc -l)"; done
