"""Deterministic fixture repositories built from a scenario's recipe.

A recipe (validated by `scenario.fixture_recipe`) lists Git operations:
commits with their files, branches, tags, checkouts, merges, resets, linked
worktrees, local bare remotes and pushes. Each runs with the QA identity and a
fixed date, its own `date` or the clock's start plus its index times the step,
for author, committer, tagger and reflog alike. No date is relative to now, so
one recipe on one Git version gives the same object IDs and reflogs, and the
frames that show them stay byte-stable; `expect` pins the IDs a frame shows.

Git runs with a throwaway HOME, no system or global configuration, an empty
template directory (no hooks), no prompts and no network: every remote is a
local bare repository inside the fixture. A fixture is built once under
`<fixtures>/<name>/` and reused while its recipe and repository state are
unchanged; anything else there is refused, never deleted.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from . import runenv

MANIFEST = "fixture-manifest.json"
DEFAULT_FIXTURES = runenv.EVIDENCE_ROOT / "fixtures"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def recipe_sha256(recipe: dict) -> str:
    return sha256(json.dumps(recipe, sort_keys=True, separators=(",", ":"), default=list).encode())


def isolated_env(home: Path, date: str | None = None) -> dict[str, str]:
    """Only what Git needs: no inherited GIT_* variable, configuration, locale or time zone."""
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(home), "XDG_CONFIG_HOME": str(home),
           "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": str(home / ".gitconfig"), "LC_ALL": "C", "TZ": "UTC",
           "GIT_TERMINAL_PROMPT": "0", "GIT_AUTHOR_NAME": runenv.QA_NAME, "GIT_AUTHOR_EMAIL": runenv.QA_EMAIL,
           "GIT_COMMITTER_NAME": runenv.QA_NAME, "GIT_COMMITTER_EMAIL": runenv.QA_EMAIL}
    if date is not None:
        env.update(GIT_AUTHOR_DATE=date, GIT_COMMITTER_DATE=date)
    return env


def read(repo: Path, *args: str) -> bytes:
    """A passive read: fixed arguments, no optional locks, no configuration but the repository's own."""
    with tempfile.TemporaryDirectory(prefix="gitturtle-recipe-read-") as home:
        Path(home, ".gitconfig").write_text("")
        result = subprocess.run(["git", "-C", str(repo), "--no-optional-locks", *args], capture_output=True,
                                env=isolated_env(Path(home)), stdin=subprocess.DEVNULL)
    return result.stdout if result.returncode == 0 else b"error: " + result.stderr.strip()


def tree_digest(root: Path) -> str:
    digest = hashlib.sha256()
    if root.is_dir():
        for path in sorted(root.rglob("*")):
            if path.is_file() and not path.is_symlink():
                digest.update(str(path.relative_to(root)).encode() + b"\0" + path.read_bytes() + b"\0")
    return digest.hexdigest()


def checkout_state(repo: Path) -> dict:
    """One working tree's HEAD, status and index."""
    index = Path(read(repo, "rev-parse", "--path-format=absolute", "--git-path", "index").decode().strip())
    return dict(head=read(repo, "rev-parse", "HEAD").decode(errors="replace").strip(),
                status_sha256=sha256(read(repo, "status", "--porcelain=v1", "-z", "--untracked-files=all")),
                index_sha256=sha256(index.read_bytes()) if index.is_file() else "absent")


def state(repo: Path) -> dict:
    """HEAD, status, index, refs, reflogs and every linked worktree's own HEAD, status and index, to show a
    launch left the fixture as it was."""
    common = Path(read(repo, "rev-parse", "--path-format=absolute", "--git-common-dir").decode().strip())
    refs = read(repo, "for-each-ref", "--format=%(refname) %(objectname)")
    listing = read(repo, "worktree", "list", "--porcelain")
    linked = [line[len(b"worktree "):].decode(errors="replace") for line in listing.splitlines()
              if line.startswith(b"worktree ")][1:]
    return dict(checkout_state(repo), refs_sha256=sha256(refs), ref_count=len(refs.splitlines()),
                reflogs_sha256=tree_digest(common / "logs"), worktrees_sha256=sha256(listing),
                linked_worktrees={path: checkout_state(Path(path)) for path in linked})


class Builder:
    def __init__(self, recipe: dict, root: Path, home: Path) -> None:
        self.recipe, self.root, self.home = recipe, root, home
        self.repo = root / recipe["repository"]
        self.template = home / "template"
        self.commits: list[str] = []
        self.log: list[dict] = []

    def git(self, *args: str, date: str | None = None, cwd: Path | None = None) -> str:
        cwd = cwd or self.repo
        result = subprocess.run(["git", *args], cwd=cwd, env=isolated_env(self.home, date), capture_output=True,
                                text=True, stdin=subprocess.DEVNULL)
        self.log.append(dict(argv=["git", *args], cwd=str(cwd), date=date, code=result.returncode))
        if result.returncode:
            raise SystemExit(f"fixture {self.recipe['name']}: git {' '.join(args)} failed: "
                             f"{result.stderr.strip()[:400]}")
        return result.stdout.strip()

    def rev(self, text: str) -> str:
        return self.commits[int(text[1:])] if text.startswith("@") else text

    def build(self) -> None:
        recipe = self.recipe
        self.template.mkdir()
        (self.home / ".gitconfig").write_text(
            f"[user]\n\tname = {runenv.QA_NAME}\n\temail = {runenv.QA_EMAIL}\n[init]\n\tdefaultBranch = "
            f"{recipe['branch']}\n[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n")
        self.repo.mkdir()
        self.git("init", "--quiet", f"--template={self.template}", "-b", recipe["branch"])
        for op in recipe["operations"]:
            getattr(self, f"op_{op['action']}")(op, op["date"])

    def op_commit(self, op: dict, date: str) -> None:
        for name, content in op["files"].items():
            path = self.repo / name
            if content is None:
                if not path.is_file():
                    raise SystemExit(f"fixture {self.recipe['name']}: cannot delete {name}, which does not exist")
                path.unlink()
                continue
            path.parent.mkdir(parents=True, exist_ok=True)
            data = base64.b64decode(content["base64"]) if isinstance(content, dict) else content.encode()
            path.write_bytes(data)
        self.git("add", "--all", date=date)
        self.git("commit", "--quiet", *(["--allow-empty"] if op["allow_empty"] else []), "-m", op["message"],
                 date=date)
        self.commits.append(self.git("rev-parse", "HEAD"))

    def op_branch(self, op: dict, date: str) -> None:
        self.git("branch", op["name"], self.rev(op["at"]), date=date)

    def op_tag(self, op: dict, date: str) -> None:
        annotated = ["-a", "-m", op["message"]] if "message" in op else []
        self.git("tag", *annotated, op["name"], self.rev(op["at"]), date=date)

    def op_checkout(self, op: dict, date: str) -> None:
        if op["create"]:
            self.git("checkout", "--quiet", "-b", op["target"], *([self.rev(op["at"])] if op["at"] else []), date=date)
        elif op["detach"]:
            self.git("checkout", "--quiet", "--detach", self.rev(op["target"]), date=date)
        else:
            self.git("checkout", "--quiet", self.rev(op["target"]), date=date)

    def op_merge(self, op: dict, date: str) -> None:
        self.git("merge", "--quiet", "--no-ff", "-m", op["message"], self.rev(op["target"]), date=date)
        self.commits.append(self.git("rev-parse", "HEAD"))

    def op_reset(self, op: dict, date: str) -> None:
        self.git("reset", "--quiet", f"--{op['mode']}", self.rev(op["target"]), date=date)

    def op_remote(self, op: dict, date: str) -> None:
        bare = self.root / op["bare"]
        bare.parent.mkdir(parents=True, exist_ok=True)
        self.git("init", "--quiet", "--bare", f"--template={self.template}", "-b", self.recipe["branch"], str(bare),
                 date=date, cwd=self.root)
        self.git("remote", "add", op["name"], str(bare), date=date)

    def op_push(self, op: dict, date: str) -> None:
        self.git("push", "--quiet", op["remote"], *op["refs"], date=date)

    def op_worktree(self, op: dict, date: str) -> None:
        path = self.root / op["path"]
        path.parent.mkdir(parents=True, exist_ok=True)
        start = ["-b", op["branch"]] if op["branch"] else ["--detach"]
        self.git("worktree", "add", "--quiet", *start, str(path), self.rev(op["at"]), date=date)

    def manifest(self, digest: str) -> dict:
        repo = self.repo
        expect = {}
        for rev, oid in self.recipe["expect"].items():
            found = self.git("rev-parse", "--verify", "--quiet", self.rev(rev))
            expect[rev] = dict(expected=oid, found=found)
        wrong = {rev: found for rev, found in expect.items() if found["expected"] != found["found"]}
        if wrong:
            raise SystemExit(f"fixture {self.recipe['name']}: object IDs differ from the recipe's expect "
                             f"{json.dumps(wrong)}; a frame showing them would change (another Git version, "
                             f"or a recipe edit?). {self.root} is left for inspection; remove it to rebuild")

        def lines(*args: str) -> list[str]:
            return read(repo, *args).decode(errors="replace").splitlines()

        refs = dict(line.split(" ", 1) for line in lines("for-each-ref", "--format=%(refname) %(objectname)"))
        return dict(version=1, made_by="scripts/native_qa/recipe.py", recipe_sha256=digest, recipe=self.recipe,
                    root=str(self.root), repository=str(repo), description=self.recipe["description"],
                    git_version=self.git("--version"), head=read(repo, "rev-parse", "HEAD").decode().strip(),
                    branch=read(repo, "symbolic-ref", "--quiet", "--short", "HEAD").decode().strip() or None,
                    commits=self.commits, refs=refs,
                    head_reflog=lines("reflog", "--format=%gd %h %gs", "HEAD"),
                    worktrees=lines("worktree", "list", "--porcelain"), remotes=lines("remote", "-v"),
                    expect=expect, state=state(repo), commands=self.log)


def build(recipe: dict, fixtures: Path = DEFAULT_FIXTURES) -> dict:
    """The recipe's fixture under `fixtures/<name>`, built now or reused; returns its manifest."""
    fixtures = runenv.absolute(fixtures, "fixtures directory")
    root = fixtures / recipe["name"]
    digest = recipe_sha256(recipe)
    if root.exists() or root.is_symlink():
        return dict(reuse(root, digest), reused=True)
    if shutil.which("git") is None:
        raise SystemExit("refusing: building a fixture needs git")
    root.mkdir(parents=True)
    with tempfile.TemporaryDirectory(prefix="gitturtle-recipe-home-") as home:
        builder = Builder(recipe, root, Path(home))
        builder.build()
        manifest = builder.manifest(digest)
    partial = root / f".{MANIFEST}.partial"
    partial.write_text(json.dumps(manifest, indent=1))
    os.replace(partial, root / MANIFEST)
    return dict(manifest, reused=False)


def reuse(root: Path, digest: str) -> dict:
    """The manifest of an earlier build of the same recipe whose repository is still exactly as built."""
    path = root / MANIFEST
    if root.is_symlink() or not root.is_dir() or not path.is_file():
        raise SystemExit(f"refusing: {root} exists but holds no finished fixture ({MANIFEST}); remove it to rebuild, "
                         "or name another fixture")
    manifest = json.loads(path.read_text())
    if manifest.get("recipe_sha256") != digest:
        raise SystemExit(f"refusing: {root} was built from another recipe; remove it to rebuild, or rename the fixture")
    now = state(Path(manifest["repository"]))
    if now != manifest.get("state"):
        changed = sorted(key for key in now if now[key] != manifest.get("state", {}).get(key))
        raise SystemExit(f"refusing: {root} changed since it was built ({', '.join(changed)}); remove it to rebuild")
    return manifest


def summary(manifest: dict) -> str:
    """One line on what the fixture holds, for the attestation."""
    refs = manifest.get("refs", {})
    branches = sum(1 for name in refs if name.startswith("refs/heads/"))
    tags = sum(1 for name in refs if name.startswith("refs/tags/"))
    remotes = sorted({line.split()[0] for line in manifest.get("remotes", []) if line.strip()})
    worktrees = sum(1 for line in manifest.get("worktrees", []) if line.startswith("worktree ")) - 1
    parts = [f"{len(manifest.get('commits', []))} commits", f"{branches} local branches", f"{tags} tags",
             f"{len(manifest.get('head_reflog', []))} HEAD reflog entries"]
    if remotes:
        parts.append(f"local bare {', '.join(remotes)}")
    if worktrees > 0:
        parts.append(f"{worktrees} linked worktrees")
    head = manifest.get("head", "")[:7]
    where = f"HEAD {head} on {manifest['branch']}" if manifest.get("branch") else f"HEAD {head} detached"
    text = f"{manifest['repository']} ({'; '.join(parts)}; {where})"
    return f"{text}: {manifest['description']}" if manifest.get("description") else text
