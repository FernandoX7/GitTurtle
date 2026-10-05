#!/usr/bin/env python3
"""The themes budget fixture (docs/development/themes/spec.md, Performance): the
`scripts/create-demo-repo.py` repository plus 1,200 linear commits, the newest of which changes 23
lines of the 2,000-line `src/generated/large-module.ts`.

Usage: make_theme_fixture.py DEST

DEST must be absent; the demo repository is created at DEST (and its linked worktree at
DEST-worktree, as create-demo-repo.py does). The repository is found from GITTURTLE_REPO or from this
file's location (two levels up from docs/benchmarks/<record>/). Git runs with no system or global
configuration, an empty template, fixed identities and fixed dates, so the same Git gives the same
object IDs. Commits 1 to 1,199 each rewrite one line of `src/generated/ledger.txt`; commit 1,200 (the
new HEAD, "Rebalance the generated ledger module") adds nothing else and changes 23 lines of
`src/generated/large-module.ts`, which commit 1 adds with 2,000 lines. The worktree is left clean.
"""
import os, subprocess, sys
from pathlib import Path

DEST = os.path.abspath(sys.argv[1])
here = Path(__file__).resolve()
REPO = Path(os.environ.get("GITTURTLE_REPO") or (here.parents[3] if len(here.parents) > 3 else here.parent))
ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
ENV.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", TZ="UTC", LC_ALL="C")
WHO = "Fixture Author <fixture@example.invalid>"
START = 1767225600  # 2026-01-01T00:00:00Z
WORDS = ["ledger", "balance", "account", "entry", "period", "credit", "debit", "total", "rate", "batch"]


def git(*args, **kw):
    return subprocess.run(["git", "-C", DEST, *args], env=ENV, check=True, **kw)


def module(changed=False):
    lines = []
    for n in range(2000):
        a, b = WORDS[n % 10], WORDS[(n * 7) % 10]
        edit = changed and n % 87 == 5  # 23 lines: n = 5, 92, ..., 1919
        value = n * 3 + (1 if edit else 0)
        lines.append(f"export const {a}{n} = compute{b.title()}({value}, \"{b}-{n}\");"
                     + (" // rebalanced" if edit else ""))
    return "\n".join(lines) + "\n"


def blob(data):
    data = data.encode()
    return b"data %d\n" % len(data) + data + b"\n"


def main():
    if os.path.lexists(DEST):
        raise SystemExit(f"refusing: {DEST} exists")
    subprocess.run([sys.executable, str(REPO / "scripts" / "create-demo-repo.py"), "--output", DEST],
                   env=ENV, check=True, stdout=subprocess.DEVNULL)
    base = git("rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
    branch = git("symbolic-ref", "--short", "HEAD", capture_output=True, text=True).stdout.strip()
    stream = bytearray()
    for i in range(1, 1201):
        when = START + i * 600
        msg = ("Rebalance the generated ledger module" if i == 1200 else f"Update ledger entry {i}").encode()
        stream += b"commit refs/heads/%s\n" % branch.encode()
        stream += b"committer %s %d +0000\n" % (WHO.encode(), when)
        stream += b"data %d\n%s\n" % (len(msg), msg)
        if i == 1:
            stream += b"from %s\n" % base.encode()
            stream += b"M 100644 inline src/generated/large-module.ts\n" + blob(module())
        if i < 1200:
            stream += b"M 100644 inline src/generated/ledger.txt\n" + blob(f"entry {i}: {WORDS[i % 10]}\n")
        else:
            stream += b"M 100644 inline src/generated/large-module.ts\n" + blob(module(changed=True))
    subprocess.run(["git", "-C", DEST, "fast-import", "--quiet"], input=bytes(stream), env=ENV, check=True)
    git("reset", "-q", "--hard", branch)
    status = git("status", "--porcelain", capture_output=True, text=True).stdout
    if status:
        raise SystemExit(f"the fixture is not clean: {status!r}")
    head = git("rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
    count = git("rev-list", "--count", "HEAD", capture_output=True, text=True).stdout.strip()
    stat = git("diff", "--numstat", "HEAD~1", "HEAD", capture_output=True, text=True).stdout.split()
    print(f"{DEST}: HEAD {head}, {count} commits, numstat {stat}")


if __name__ == "__main__":
    main()
