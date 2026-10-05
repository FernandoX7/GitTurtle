#!/usr/bin/env python3
"""A disposable, deterministic fixture with two large text patches in one commit, and a large stash.

Usage: make_compare_fixture.py DEST [LINES]

DEST must be absent. Git runs with no system or global configuration, an empty template (no hooks),
fixed identities and fixed dates, so the same Git gives the same object IDs. Shape:
- commit 1 (root) adds `a-large.txt` and `b-large.txt`, LINES lines each (default 20,000), code-like
  ASCII lines of about 30 to 100 bytes;
- commit 2 (HEAD, `main`) rewrites every third line of both files, so each file's patch is a single
  hunk of about LINES + 2 * LINES / 3 lines, a third of them added and a third removed;
- `refs/stash` holds one stash on top of HEAD whose worktree side rewrites every fifth line of commit
  1's text of both files (with LINES = 20,000 each file's stashed patch against HEAD removes and adds
  9,333 lines in a single hunk); its date is older than HEAD's, but History lists it first.
The worktree and index are left clean at HEAD.
"""
import os, subprocess, sys

DEST = os.path.abspath(sys.argv[1])
LINES = int(sys.argv[2]) if len(sys.argv) > 2 else 20000
WORDS = ["buffer", "cursor", "layout", "render", "commit", "parent", "window", "gutter", "editor", "worker",
         "preview", "history", "column", "offset", "budget", "decor", "patch", "status", "branch", "stash"]
ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
ENV.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=os.path.dirname(DEST), TZ="UTC",
           LC_ALL="C", GIT_AUTHOR_NAME="Fixture Author", GIT_AUTHOR_EMAIL="fixture@example.invalid",
           GIT_COMMITTER_NAME="Fixture Author", GIT_COMMITTER_EMAIL="fixture@example.invalid")


def git(*args, when=None, **kw):
    env = dict(ENV)
    if when is not None:
        env.update(GIT_AUTHOR_DATE=f"{when} +0000", GIT_COMMITTER_DATE=f"{when} +0000")
    return subprocess.run(["git", "-C", DEST, *args], env=env, check=True, **kw)


def line(seed, n, edit=0):
    words = [WORDS[(seed * 7 + n * 3 + k * 5 + edit * 11) % len(WORDS)] for k in range(3 + (n + edit) % 5)]
    indent = "    " * (n % 3)
    tail = f"  // {seed}:{n}" + (f" edited {edit}" if edit else "")
    return f"{indent}let {words[0]}_{n} = {'.'.join(words[1:])}({n % 97});{tail}"


def text(seed, every=0, edit=0):
    return "".join(line(seed, n, edit if every and n % every == 0 else 0) + "\n" for n in range(LINES))


def write(name, data):
    with open(os.path.join(DEST, name), "w", newline="\n") as f:
        f.write(data)


def main():
    if os.path.lexists(DEST):
        raise SystemExit(f"refusing: {DEST} exists")
    subprocess.run(["git", "init", "-q", "--template=", "-b", "main", DEST], env=ENV, check=True)
    write("a-large.txt", text(1))
    write("b-large.txt", text(2))
    git("add", "a-large.txt", "b-large.txt")
    git("commit", "-q", "--no-gpg-sign", "-m", "Add two large text files", when=1704067200)
    write("a-large.txt", text(1, every=3, edit=1))
    write("b-large.txt", text(2, every=3, edit=1))
    git("commit", "-q", "--no-gpg-sign", "-am", "Rewrite every third line of both large files", when=1704070800)
    write("a-large.txt", text(1, every=5, edit=2))
    write("b-large.txt", text(2, every=5, edit=2))
    git("stash", "push", "-q", "-m", "Rewrite every fifth line of both large files", when=1704069000)
    status = git("status", "--porcelain", capture_output=True, text=True).stdout
    if status:
        raise SystemExit(f"the fixture is not clean: {status!r}")
    head = git("rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
    stash = git("rev-parse", "refs/stash", capture_output=True, text=True).stdout.strip()
    stat = git("diff", "--numstat", "HEAD~1", "HEAD", capture_output=True, text=True).stdout.split()
    patch = git("diff", "HEAD~1", "HEAD", "--", "a-large.txt", capture_output=True).stdout
    sizes = {name: os.path.getsize(os.path.join(DEST, name)) for name in ("a-large.txt", "b-large.txt")}
    print(f"{DEST}: HEAD {head}, stash {stash}, numstat {stat}, a-large.txt patch {len(patch)} bytes "
          f"{patch.count(b'\n')} lines, file bytes {sizes}")


if __name__ == "__main__":
    main()
