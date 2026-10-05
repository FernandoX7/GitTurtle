#!/usr/bin/env python3
"""A disposable, deterministic 3,000-commit History fixture with merges and 120 branches.

Usage: make_history_fixture.py DEST [COMMITS]

DEST must be absent. Everything runs through `git fast-import` with no system or global
configuration, an empty template (no hooks), fixed identities and fixed dates, so the same Git
gives the same object IDs. Shape: blocks of 21 commits, each a 4-commit side branch and a
16-commit mainline run from the same fork point, then a merge of the side branch into main; the
commits of a block alternate in date between the two lines, so History draws two lanes and a
merge every 21 rows. Leftover commits are linear. `topic/NNN` names the side tip of each of the
last 120 blocks. Subjects vary in length from a fixed word list; every commit changes one of 40
small files.
"""
import os, subprocess, sys

DEST = os.path.abspath(sys.argv[1])
TOTAL = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
BRANCHES = 120
EPOCH = 1704067200  # 2024-01-01T00:00:00Z
STEP = 1800
AUTHORS = [("Avery Stone", "avery@example.invalid"), ("Blake Rivers", "blake@example.invalid"),
           ("Casey Moreau", "casey@example.invalid"), ("Dana Okafor", "dana@example.invalid"),
           ("Emery Lindqvist", "emery@example.invalid"), ("Frankie Osei", "frankie@example.invalid")]
VERBS = ["Refine", "Fix", "Add", "Remove", "Rename", "Document", "Simplify", "Measure", "Cache", "Split"]
NOUNS = ["history paging", "column layout", "graph lanes", "status refresh", "diff gutter", "blame reader",
         "preview cache", "branch menu", "scope toolbar", "file list", "search cursor", "worktree view"]
TAILS = ["", " for narrow windows", " when the window is resized", " after a refresh",
         " without allocating per row", " so the selection stays visible", " and keep the old behaviour behind a flag",
         " in the background worker"]
ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
ENV.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=os.path.dirname(DEST), TZ="UTC", LC_ALL="C")


def git(*args, **kw):
    return subprocess.run(["git", "-C", DEST, *args], env=ENV, check=True, **kw)


def main():
    if os.path.lexists(DEST):
        raise SystemExit(f"refusing: {DEST} exists")
    subprocess.run(["git", "init", "-q", "--template=", "-b", "main", DEST], env=ENV, check=True)
    out, n, mark = [], 0, 0

    def commit(ref, parents):
        nonlocal n, mark
        mark += 1
        name, email = AUTHORS[n % len(AUTHORS)]
        when = EPOCH + n * STEP
        subject = f"{VERBS[n % 10]} {NOUNS[(n * 7) % 12]}{TAILS[(n * 3) % 8]} ({n + 1})"
        body = f"Commit {n + 1} of the fixture.\n"
        msg = f"{subject}\n\n{body}".encode()
        data = f"line {n}\n{subject}\n".encode()
        out.append(f"commit {ref}\nmark :{mark}\n".encode())
        out.append(f"author {name} <{email}> {when} +0000\ncommitter {name} <{email}> {when} +0000\n".encode())
        out.append(b"data %d\n%s\n" % (len(msg), msg))
        if parents:
            out.append(f"from :{parents[0]}\n".encode())
            for p in parents[1:]:
                out.append(f"merge :{p}\n".encode())
        out.append(f"M 100644 inline src/module-{n % 40:02d}/notes.txt\n".encode())
        out.append(b"data %d\n%s\n" % (len(data), data))
        n += 1
        return mark

    main_tip = commit("refs/heads/main", [])
    side_tips = []
    while n + 21 <= TOTAL:
        fork, side = main_tip, main_tip
        for i in range(20):  # side commits at positions 1, 6, 11, 16 of the run
            if i % 5 == 1:
                side = commit("refs/heads/main", [side])
            else:
                main_tip = commit("refs/heads/main", [main_tip])
        side_tips.append(side)
        main_tip = commit("refs/heads/main", [main_tip, side])
    while n < TOTAL:
        main_tip = commit("refs/heads/main", [main_tip])
    out.append(f"reset refs/heads/main\nfrom :{main_tip}\n\n".encode())
    for i, tip in enumerate(side_tips[-BRANCHES:]):
        out.append(f"reset refs/heads/topic/{i:03d}\nfrom :{tip}\n\n".encode())
    subprocess.run(["git", "-C", DEST, "fast-import", "--quiet"], input=b"".join(out), env=ENV, check=True)
    git("reset", "-q", "--hard", "main")
    count = git("rev-list", "--count", "--all", capture_output=True, text=True).stdout.strip()
    head = git("rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
    heads = git("for-each-ref", "--count=1000", "refs/heads", capture_output=True, text=True).stdout.count("\n")
    print(f"{DEST}: {count} commits, {heads} branches, HEAD {head}")


if __name__ == "__main__":
    main()
